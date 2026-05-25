//! Graph Foundation Model — single-layer forward pass (M3 spike).
//!
//! Implements the SPEC §4.2.3 / §A.4 architecture without autodiff:
//!   m_{u→v} = g_uv ⊙ W_m · h_u
//!   h_v⁽ˡ⁾ = LN( h_v⁽ˡ⁻¹⁾ + PReLU( b + Σ_u m_{u→v} ) )
//!
//! Vector gate uses raw structural features (degrees + graph summary) — the
//! `E_n / E_p / E_g` embedding stage is collapsed into a single 2-layer MLP for
//! this CPU spike. Random-init weights are seedable for reproducibility.
//!
//! Aggregation goes through `sage_core::ops::scatter_add_rows` — when a tensor
//! backend (candle / burn) lands, only the scatter primitive needs to be
//! re-pointed; the forward semantics stay identical.

use sage_core::{ops::scatter_add_rows, Result};

/// Hyperparameters for a single GFM layer.
#[derive(Clone, Copy, Debug)]
pub struct GfmConfig {
    pub hidden_dim: usize,
    pub gate_hidden: usize,
    pub delta: f32,
    pub prelu_init: f32,
}

impl Default for GfmConfig {
    fn default() -> Self {
        Self {
            hidden_dim: 32,
            gate_hidden: 16,
            delta: 0.1,
            prelu_init: 0.25,
        }
    }
}

/// Structural features fed into the gate MLP per edge.
/// Length = 5: [deg_u, deg_v, |deg_u−deg_v|, mean_deg, density]
const STRUCT_DIM: usize = 5;

/// Read-only graph view used for one forward pass.
#[derive(Clone, Debug)]
pub struct GfmGraphView<'a> {
    pub n_nodes: usize,
    /// Edge list as (src_idx, dst_idx) into `h_in`. Length = E.
    pub edges: &'a [(usize, usize)],
    /// Per-node out-degree (used for ϕ).
    pub deg: &'a [f32],
}

pub struct GfmLayer {
    cfg: GfmConfig,
    // Message transform [D, D] row-major
    w_msg: Vec<f32>,
    b_msg: Vec<f32>,
    // Gate MLP: [STRUCT_DIM] → [gate_hidden] → [hidden_dim]
    w_g1: Vec<f32>, // [gate_hidden, STRUCT_DIM]
    b_g1: Vec<f32>, // [gate_hidden]
    w_g2: Vec<f32>, // [hidden_dim, gate_hidden]
    b_g2: Vec<f32>, // [hidden_dim]
    // LayerNorm scale & shift
    ln_gamma: Vec<f32>,
    ln_beta: Vec<f32>,
    // PReLU slope (per-channel)
    prelu_a: Vec<f32>,
}

impl std::fmt::Debug for GfmLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GfmLayer")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl GfmLayer {
    /// Construct with deterministic seeded random init. Uses a simple LCG to
    /// avoid a `rand` dependency in `sage-reader`'s production deps.
    pub fn new(cfg: GfmConfig, seed: u64) -> Self {
        let h = cfg.hidden_dim;
        let g = cfg.gate_hidden;
        let mut rng = Lcg::new(seed);
        let init = |rng: &mut Lcg, fan_in: usize, n: usize| -> Vec<f32> {
            let scale = (2.0_f32 / fan_in as f32).sqrt(); // He init
            (0..n).map(|_| rng.gauss() * scale).collect()
        };
        Self {
            w_msg: init(&mut rng, h, h * h),
            b_msg: vec![0.0; h],
            w_g1: init(&mut rng, STRUCT_DIM, g * STRUCT_DIM),
            b_g1: vec![0.0; g],
            w_g2: init(&mut rng, g, h * g),
            b_g2: vec![0.0; h],
            ln_gamma: vec![1.0; h],
            ln_beta: vec![0.0; h],
            prelu_a: vec![cfg.prelu_init; h],
            cfg,
        }
    }

    pub fn cfg(&self) -> &GfmConfig {
        &self.cfg
    }

    /// Forward pass — returns `h_out` of shape `[n_nodes, hidden_dim]` row-major.
    pub fn forward(&self, graph: &GfmGraphView<'_>, h_in: &[f32]) -> Result<Vec<f32>> {
        let d = self.cfg.hidden_dim;
        let n = graph.n_nodes;
        if h_in.len() != n * d {
            return Err(sage_core::SageError::Reader(format!(
                "GfmLayer::forward: h_in.len()={} != n_nodes*hidden_dim={}",
                h_in.len(),
                n * d
            )));
        }
        if graph.deg.len() != n {
            return Err(sage_core::SageError::Reader(format!(
                "GfmLayer::forward: deg.len()={} != n_nodes={n}",
                graph.deg.len()
            )));
        }

        // Graph-level summary: mean degree + density.
        let mean_deg: f32 = if n == 0 {
            0.0
        } else {
            graph.deg.iter().sum::<f32>() / n as f32
        };
        let max_edges = if n < 2 { 1.0 } else { (n * (n - 1)) as f32 };
        let density = graph.edges.len() as f32 / max_edges;

        // 1. Compute m_{u→v} per edge into a flat [E, D] buffer.
        let e = graph.edges.len();
        let mut messages = vec![0.0f32; e * d];
        for (i, &(u, v)) in graph.edges.iter().enumerate() {
            if u >= n || v >= n {
                return Err(sage_core::SageError::Reader(format!(
                    "GfmLayer::forward: edge ({u},{v}) out of bounds for n={n}"
                )));
            }
            // Structural features (5 dims).
            let du = graph.deg[u];
            let dv = graph.deg[v];
            let z = [du, dv, (du - dv).abs(), mean_deg, density];

            // Gate: g = 1 + δ · tanh( W_g2 · tanh(W_g1 · z + b_g1) + b_g2 )
            let mut hid = vec![0.0f32; self.cfg.gate_hidden];
            mat_vec(&self.w_g1, &z, &mut hid, self.cfg.gate_hidden, STRUCT_DIM);
            add_inplace(&mut hid, &self.b_g1);
            for x in &mut hid {
                *x = x.tanh();
            }
            let mut gate = vec![0.0f32; d];
            mat_vec(&self.w_g2, &hid, &mut gate, d, self.cfg.gate_hidden);
            add_inplace(&mut gate, &self.b_g2);
            for x in &mut gate {
                *x = 1.0 + self.cfg.delta * x.tanh();
            }

            // W_m · h_u
            let h_u = &h_in[u * d..(u + 1) * d];
            let mut transformed = vec![0.0f32; d];
            mat_vec(&self.w_msg, h_u, &mut transformed, d, d);
            add_inplace(&mut transformed, &self.b_msg);

            // m = gate ⊙ transformed
            for j in 0..d {
                messages[i * d + j] = gate[j] * transformed[j];
            }
        }

        // 2. Aggregate messages into per-node sums via scatter_add_rows.
        let mut aggr = vec![0.0f32; n * d];
        let dst_indices: Vec<usize> = graph.edges.iter().map(|(_, v)| *v).collect();
        scatter_add_rows(&mut aggr, d, &dst_indices, &messages)?;

        // 3. h_v = LN( h_v + PReLU(aggr) ).
        let mut h_out = vec![0.0f32; n * d];
        for v in 0..n {
            let row = v * d;
            let mut sum = vec![0.0f32; d];
            for j in 0..d {
                let x = aggr[row + j];
                let prelu = if x >= 0.0 { x } else { self.prelu_a[j] * x };
                sum[j] = h_in[row + j] + prelu;
            }
            // LayerNorm
            let mean: f32 = sum.iter().sum::<f32>() / d as f32;
            let variance: f32 = sum.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / d as f32;
            let inv = (variance + 1e-5).sqrt().recip();
            for j in 0..d {
                h_out[row + j] = ((sum[j] - mean) * inv) * self.ln_gamma[j] + self.ln_beta[j];
            }
        }
        Ok(h_out)
    }
}

// --- helpers ---------------------------------------------------------------

fn mat_vec(w: &[f32], x: &[f32], out: &mut [f32], rows: usize, cols: usize) {
    debug_assert_eq!(w.len(), rows * cols);
    debug_assert_eq!(x.len(), cols);
    debug_assert_eq!(out.len(), rows);
    for i in 0..rows {
        let row = &w[i * cols..(i + 1) * cols];
        let mut acc = 0.0f32;
        for j in 0..cols {
            acc += row[j] * x[j];
        }
        out[i] = acc;
    }
}

fn add_inplace(a: &mut [f32], b: &[f32]) {
    for (x, y) in a.iter_mut().zip(b.iter()) {
        *x += *y;
    }
}

/// Small linear congruential generator for deterministic Box-Muller-ish Gaussian
/// noise. Quality is poor but adequate for spike-test weight init.
struct Lcg {
    state: u64,
}
impl Lcg {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1),
        }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
    /// Box-Muller — one Gaussian per call (discards the partner).
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_u32() as f32 / u32::MAX as f32).max(1e-6);
        let u2 = self.next_u32() as f32 / u32::MAX as f32;
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_graph(
        n: usize,
        edges: &'static [(usize, usize)],
        deg: &'static [f32],
    ) -> GfmGraphView<'static> {
        GfmGraphView {
            n_nodes: n,
            edges,
            deg,
        }
    }

    #[test]
    fn forward_produces_correct_shape() {
        let cfg = GfmConfig {
            hidden_dim: 8,
            ..GfmConfig::default()
        };
        let layer = GfmLayer::new(cfg, 42);
        let g = toy_graph(3, &[(0, 1), (1, 2)], &[1.0, 1.0, 0.0]);
        let h_in = vec![0.1f32; 3 * 8];
        let h_out = layer.forward(&g, &h_in).unwrap();
        assert_eq!(h_out.len(), 3 * 8);
        for x in &h_out {
            assert!(x.is_finite(), "non-finite value in h_out: {x}");
        }
    }

    #[test]
    fn forward_isolated_node_unchanged_except_layernorm() {
        // Node 2 has no incoming edges → aggregated message is zero → PReLU(0)=0 →
        // h_v⁽¹⁾ = LN(h_v⁽⁰⁾ + 0). With uniform h_in row, LN normalizes to zero mean.
        let cfg = GfmConfig {
            hidden_dim: 4,
            ..GfmConfig::default()
        };
        let layer = GfmLayer::new(cfg, 7);
        let g = toy_graph(3, &[(0, 1)], &[1.0, 0.0, 0.0]); // only 0→1; node 2 isolated
        let h_in = vec![1.0; 3 * 4];
        let h_out = layer.forward(&g, &h_in).unwrap();
        // Node 2's row should sum to ~0 because LN centers it.
        let row2: &[f32] = &h_out[8..12];
        let mean = row2.iter().sum::<f32>() / 4.0;
        assert!(
            mean.abs() < 1e-5,
            "LN should zero-center isolated node, got mean={mean}"
        );
    }

    #[test]
    fn forward_determinism() {
        let cfg = GfmConfig::default();
        let a = GfmLayer::new(cfg, 12345);
        let b = GfmLayer::new(cfg, 12345);
        let g = toy_graph(4, &[(0, 1), (1, 2), (2, 3), (0, 3)], &[2.0, 1.0, 1.0, 0.0]);
        let h_in = vec![0.1f32; 4 * cfg.hidden_dim];
        let oa = a.forward(&g, &h_in).unwrap();
        let ob = b.forward(&g, &h_in).unwrap();
        assert_eq!(oa, ob, "same seed must produce same forward output");
    }

    #[test]
    fn shape_mismatch_errors() {
        let layer = GfmLayer::new(GfmConfig::default(), 0);
        let g = toy_graph(3, &[], &[0.0; 3]);
        let r = layer.forward(&g, &[1.0; 10]);
        assert!(r.is_err());
    }

    #[test]
    fn edge_out_of_bounds_errors() {
        let layer = GfmLayer::new(GfmConfig::default(), 0);
        let g = toy_graph(2, &[(0, 5)], &[1.0; 2]);
        let h_in = vec![0.1; 2 * 32];
        let r = layer.forward(&g, &h_in);
        assert!(r.is_err());
    }

    #[test]
    fn empty_graph_is_pure_layernorm() {
        // No edges → aggregate is zero → h_out = LN(h_in).
        let layer = GfmLayer::new(
            GfmConfig {
                hidden_dim: 4,
                ..GfmConfig::default()
            },
            0,
        );
        let g = toy_graph(2, &[], &[0.0, 0.0]);
        let h_in = vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0];
        let h_out = layer.forward(&g, &h_in).unwrap();
        // LN of [1,2,3,4] yields zero-mean
        let row0_mean: f32 = h_out[..4].iter().sum::<f32>() / 4.0;
        assert!(row0_mean.abs() < 1e-5);
    }
}
