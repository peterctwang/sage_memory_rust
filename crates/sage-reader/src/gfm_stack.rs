//! Multi-layer GFM + Context/Schema decomposition (SPEC §4.2.4 / §A.5).
//!
//! Stacks L `GfmLayer`s and adds a two-channel head:
//!   - **Context channel** (`H_ctx`): straightforward iterated forward pass.
//!   - **Schema channel** (`H_sch`): same propagation seeded by a learnable
//!     mixture of K prompt bases — captures cross-graph structural priors.
//!
//! Final representation: `H = H_ctx + β_sch · H_sch`. Calibration:
//! `h̃⁽⁰⁾ = p_feature ⊙ h⁽⁰⁾`.
//!
//! ⚠️ §7.1 honesty: with random-init weights neither channel does anything
//! useful. The full SPEC `F_prompt` (per-layer prompt-conditioned MLP) is
//! collapsed into a per-layer additive schema bias here — sufficient to
//! exercise the dual-channel surface and prove tensor shapes line up.
//! Real `F_prompt` lands with M3-full training.

use sage_core::{ops::scatter_add_rows, Result, SageError};

use crate::gfm::{GfmConfig, GfmGraphView, GfmLayer};

/// Calibration + dual-channel mixing head — SPEC §A.5.
pub struct ContextSchemaHead {
    /// `p_feature` ∈ ℝ^D — broadcast over every row of h⁽⁰⁾.
    p_feature: Vec<f32>,
    /// K prompt bases per layer; each base is `[D]`. Layer l mixes them with
    /// learnable weights `ω_j^(l)` — here those weights default to uniform
    /// (1/K) and the schema "prompt" reduces to an additive bias.
    /// Shape: `[num_layers][K][D]` row-major-of-row-major.
    schema_bases: Vec<Vec<Vec<f32>>>,
    /// `β_sch` in `H = H_ctx + β_sch · H_sch`.
    beta_sch: f32,
}

impl std::fmt::Debug for ContextSchemaHead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextSchemaHead")
            .field("dim", &self.p_feature.len())
            .field("num_layers", &self.schema_bases.len())
            .field(
                "k_prompt_bases",
                &self.schema_bases.first().map_or(0, Vec::len),
            )
            .field("beta_sch", &self.beta_sch)
            .finish()
    }
}

impl ContextSchemaHead {
    /// Initialize identity-like: `p_feature = 1`, all schema bases zero.
    /// With zero schema bases the schema channel matches the context channel
    /// exactly until a training run differentiates them — by design.
    pub fn identity(dim: usize, num_layers: usize, k_bases: usize, beta_sch: f32) -> Self {
        Self {
            p_feature: vec![1.0; dim],
            schema_bases: vec![vec![vec![0.0; dim]; k_bases]; num_layers],
            beta_sch,
        }
    }

    pub fn beta_sch(&self) -> f32 {
        self.beta_sch
    }
    pub fn num_layers(&self) -> usize {
        self.schema_bases.len()
    }
    pub fn k_bases(&self) -> usize {
        self.schema_bases.first().map_or(0, Vec::len)
    }

    /// `h̃⁽⁰⁾ = p_feature ⊙ h⁽⁰⁾`. Broadcast `p_feature` over `n_nodes` rows.
    pub fn calibrate(&self, h_in: &mut [f32]) {
        let d = self.p_feature.len();
        for row in h_in.chunks_mut(d) {
            for (j, x) in row.iter_mut().enumerate() {
                *x *= self.p_feature[j];
            }
        }
    }

    /// `P_schema^(l) = mean_j(P_j^(l))` (uniform mixture in the spike).
    /// Returns the additive bias broadcast over nodes at layer `l`.
    /// Returns `None` if `layer` is out of range.
    pub fn schema_bias_for_layer(&self, layer: usize) -> Option<Vec<f32>> {
        let bases = self.schema_bases.get(layer)?;
        if bases.is_empty() {
            return Some(vec![0.0; self.p_feature.len()]);
        }
        let d = self.p_feature.len();
        let k = bases.len() as f32;
        let mut mixed = vec![0.0f32; d];
        for base in bases {
            for (j, x) in base.iter().enumerate() {
                mixed[j] += x / k;
            }
        }
        Some(mixed)
    }
}

/// Multi-layer GFM with Context/Schema head.
pub struct GfmStack {
    layers: Vec<GfmLayer>,
    head: ContextSchemaHead,
}

impl std::fmt::Debug for GfmStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GfmStack")
            .field("num_layers", &self.layers.len())
            .field("head", &self.head)
            .finish()
    }
}

impl GfmStack {
    /// Build a fresh stack: `num_layers` identical-config GFM layers (each
    /// seeded distinctly) plus an identity-initialised Context/Schema head.
    pub fn new(cfg: GfmConfig, num_layers: usize, k_bases: usize, seed: u64) -> Self {
        let layers = (0..num_layers.max(1))
            .map(|l| GfmLayer::new(cfg, seed.wrapping_add(l as u64 + 1)))
            .collect();
        let head = ContextSchemaHead::identity(cfg.hidden_dim, num_layers.max(1), k_bases, 0.5);
        Self { layers, head }
    }

    pub fn with_head(mut self, head: ContextSchemaHead) -> Self {
        self.head = head;
        self
    }

    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }
    pub fn head(&self) -> &ContextSchemaHead {
        &self.head
    }
    pub fn cfg(&self) -> &GfmConfig {
        self.layers[0].cfg()
    }

    /// Forward through L layers with the Context/Schema decomposition.
    ///
    /// `H = H_ctx + β_sch · H_sch` where:
    /// - `H_ctx` = iterated layer forward on calibrated `h̃⁽⁰⁾`
    /// - `H_sch` = same propagation but each layer adds its mixed schema bias
    ///   to the incoming hidden state before the layer's own forward pass
    pub fn forward(&self, graph: &GfmGraphView<'_>, h_in: &[f32]) -> Result<Vec<f32>> {
        let d = self.cfg().hidden_dim;
        if h_in.len() != graph.n_nodes * d {
            return Err(SageError::Reader(format!(
                "GfmStack::forward: h_in.len()={} != n_nodes*hidden_dim={}",
                h_in.len(),
                graph.n_nodes * d
            )));
        }

        // Calibrate h⁽⁰⁾.
        let mut h_calibrated = h_in.to_vec();
        self.head.calibrate(&mut h_calibrated);

        // Context channel.
        let mut h_ctx = h_calibrated.clone();
        for layer in &self.layers {
            h_ctx = layer.forward(graph, &h_ctx)?;
        }

        // Schema channel — same chain, schema bias broadcast and scatter-added
        // pre-forward at each layer to mirror the SPEC's prompt-conditioned
        // propagation in a tensor-shape-equivalent way.
        let mut h_sch = h_calibrated;
        for (l, layer) in self.layers.iter().enumerate() {
            if let Some(bias) = self.head.schema_bias_for_layer(l) {
                // Broadcast bias over all nodes via scatter_add_rows with the
                // identity index list — exercises the same kernel the real
                // F_prompt will use.
                let indices: Vec<usize> = (0..graph.n_nodes).collect();
                let mut src = Vec::with_capacity(graph.n_nodes * d);
                for _ in 0..graph.n_nodes {
                    src.extend_from_slice(&bias);
                }
                scatter_add_rows(&mut h_sch, d, &indices, &src)?;
            }
            h_sch = layer.forward(graph, &h_sch)?;
        }

        // Combine: H = H_ctx + β_sch · H_sch
        let beta = self.head.beta_sch();
        let mut out = h_ctx;
        for (j, x) in h_sch.iter().enumerate() {
            out[j] += beta * x;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_graph<'a>(n: usize, edges: &'a [(usize, usize)], deg: &'a [f32]) -> GfmGraphView<'a> {
        GfmGraphView {
            n_nodes: n,
            edges,
            deg,
        }
    }

    fn cfg(dim: usize) -> GfmConfig {
        GfmConfig {
            hidden_dim: dim,
            gate_hidden: 4,
            delta: 0.1,
            prelu_init: 0.25,
        }
    }

    #[test]
    fn stack_forward_produces_correct_shape() {
        let stack = GfmStack::new(cfg(8), 3, 4, 0xABCD);
        let g = toy_graph(3, &[(0, 1), (1, 2)], &[1.0, 1.0, 0.0]);
        let h_in = vec![0.1f32; 3 * 8];
        let h_out = stack.forward(&g, &h_in).unwrap();
        assert_eq!(h_out.len(), 3 * 8);
        for x in &h_out {
            assert!(x.is_finite(), "non-finite output: {x}");
        }
    }

    #[test]
    fn stack_with_zero_schema_matches_doubled_context() {
        // With identity head (schema_bases all zero) and β_sch = 0.5, the
        // schema chain receives no extra signal at any layer, so H_sch == H_ctx
        // → final H == 1.5 · H_ctx (element-wise).
        let stack = GfmStack::new(cfg(4), 2, 2, 7);
        let g = toy_graph(2, &[(0, 1)], &[1.0, 0.0]);
        let h_in = vec![0.3f32; 2 * 4];
        let out = stack.forward(&g, &h_in).unwrap();
        // Re-run a "context-only" baseline manually.
        let mut ctx = h_in.clone();
        for layer in &stack.layers {
            ctx = layer.forward(&g, &ctx).unwrap();
        }
        for (o, c) in out.iter().zip(ctx.iter()) {
            let expected = c * (1.0 + stack.head().beta_sch());
            assert!(
                (o - expected).abs() < 1e-4,
                "stack output should equal (1 + β_sch)·ctx with zero schema; got {o} vs {expected}"
            );
        }
    }

    #[test]
    fn stack_dim_mismatch_errors() {
        let stack = GfmStack::new(cfg(8), 1, 1, 0);
        let g = toy_graph(2, &[], &[0.0, 0.0]);
        let r = stack.forward(&g, &[0.1f32; 10]);
        assert!(matches!(r, Err(SageError::Reader(_))));
    }

    #[test]
    fn stack_layers_increase_with_num_layers() {
        let s1 = GfmStack::new(cfg(4), 1, 1, 0);
        let s5 = GfmStack::new(cfg(4), 5, 1, 0);
        assert_eq!(s1.num_layers(), 1);
        assert_eq!(s5.num_layers(), 5);
    }

    #[test]
    fn head_calibrate_with_non_identity_p_feature_scales_input() {
        let mut head = ContextSchemaHead::identity(4, 1, 1, 0.0);
        head.p_feature = vec![2.0, 1.0, 0.5, 1.0];
        let mut h = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]; // 2 nodes × 4 dim
        head.calibrate(&mut h);
        assert_eq!(h, vec![2.0, 1.0, 0.5, 1.0, 2.0, 1.0, 0.5, 1.0]);
    }

    #[test]
    fn schema_bias_is_mean_of_bases() {
        let mut head = ContextSchemaHead::identity(3, 2, 2, 0.5);
        head.schema_bases[0][0] = vec![1.0, 0.0, 0.0];
        head.schema_bases[0][1] = vec![0.0, 1.0, 0.0];
        let bias = head.schema_bias_for_layer(0).unwrap();
        assert!((bias[0] - 0.5).abs() < 1e-6);
        assert!((bias[1] - 0.5).abs() < 1e-6);
        assert!(bias[2].abs() < 1e-6);
    }

    #[test]
    fn schema_bias_out_of_range_returns_none() {
        let head = ContextSchemaHead::identity(4, 2, 1, 0.5);
        assert!(head.schema_bias_for_layer(2).is_none());
    }

    #[test]
    fn stack_determinism() {
        let g = toy_graph(3, &[(0, 1), (1, 2)], &[1.0, 1.0, 0.0]);
        let h_in = vec![0.2f32; 3 * 4];
        let a = GfmStack::new(cfg(4), 3, 2, 555);
        let b = GfmStack::new(cfg(4), 3, 2, 555);
        assert_eq!(a.forward(&g, &h_in).unwrap(), b.forward(&g, &h_in).unwrap());
    }
}
