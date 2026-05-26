//! `GfmReader` — Reader trait impl driven by a `GfmLayer` forward pass.
//!
//! ⚠️ **Spike caveat (§7.1 honesty)**: with random-init weights this reader
//! will **not** outperform `HeuristicReader`. The point of this module is to
//! prove the wiring: real graph → `GfmGraphView` → `GfmLayer.forward` →
//! doc scores via `Reader::read`. Once training lands (M4), only the weight
//! source changes — every other surface stays identical.
//!
//! Pipeline per query:
//!   1. plan(q) — same `HeuristicPlanner` as `HeuristicReader`
//!   2. enumerate all entities in tenant; build flat (u_idx, v_idx) edges
//!   3. seed h⁽⁰⁾ from soft-addressing softmax × query embedding projection
//!   4. layer.forward(view, h0) → h⁽¹⁾
//!   5. pool: doc_score(d) = Σ_{e ∈ d.source_docs} ‖h_e‖₁ · p₀(e|q)
//!   6. emit `ReadOutput`

use std::sync::Arc;

use ahash::AHashMap;
use async_trait::async_trait;
use sage_core::{
    DocId, Embedder, EntityId, Query, ReadOutput, Reader, ReaderGraph, Result, SageError, Score,
    Subgraph, TenantId,
};

use crate::addressing::{score_entry, softmax_entry, AddressingWeights};
use crate::gfm::{GfmConfig, GfmGraphView, GfmLayer};
use crate::planner::{HeuristicPlanner, QueryPlanner};

pub struct GfmReader<P: QueryPlanner = HeuristicPlanner> {
    planner: P,
    weights: AddressingWeights,
    embedder: Arc<dyn Embedder>,
    layer: Arc<GfmLayer>,
}

impl<P: QueryPlanner + std::fmt::Debug> std::fmt::Debug for GfmReader<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GfmReader")
            .field("planner", &self.planner)
            .field("weights", &self.weights)
            .field("embedder_dim", &self.embedder.dim())
            .field("gfm_cfg", self.layer.cfg())
            .finish_non_exhaustive()
    }
}

impl GfmReader<HeuristicPlanner> {
    /// Construct with default planner + freshly-initialized GFM layer.
    ///
    /// `embedder.dim()` MUST equal `cfg.hidden_dim` — the seed projection skips
    /// `W_q` and uses the embedding vector directly as h⁽⁰⁾ scale.
    pub fn new(embedder: Arc<dyn Embedder>, cfg: GfmConfig, seed: u64) -> Self {
        Self {
            planner: HeuristicPlanner::new(),
            weights: AddressingWeights::default(),
            embedder,
            layer: Arc::new(GfmLayer::new(cfg, seed)),
        }
    }
}

impl<P: QueryPlanner> GfmReader<P> {
    pub fn with_planner_and_layer(
        planner: P,
        embedder: Arc<dyn Embedder>,
        layer: Arc<GfmLayer>,
    ) -> Self {
        Self {
            planner,
            weights: AddressingWeights::default(),
            embedder,
            layer,
        }
    }

    pub fn with_weights(mut self, w: AddressingWeights) -> Self {
        self.weights = w;
        self
    }

    pub fn layer(&self) -> &Arc<GfmLayer> {
        &self.layer
    }
}

#[async_trait]
impl<P: QueryPlanner + std::fmt::Debug> Reader for GfmReader<P> {
    #[allow(clippy::many_single_char_names)] // Reader trait signature uses (t, q, g)
    async fn read(&self, t: TenantId, q: &Query, g: &dyn ReaderGraph) -> Result<ReadOutput> {
        let d = self.layer.cfg().hidden_dim;
        if self.embedder.dim() != d {
            return Err(SageError::Reader(format!(
                "GfmReader: embedder.dim()={} != gfm.hidden_dim={d}",
                self.embedder.dim()
            )));
        }

        let plan = self.planner.plan(q).await?;
        let entities = g.all_entities(t).await?;
        if entities.is_empty() {
            return Ok(ReadOutput::default());
        }

        // Embed the query into the same dim as h.
        let q_vec_arr = self.embedder.embed(&[q.text.as_ref()]).await?;
        let q_emb = q_vec_arr
            .first()
            .ok_or_else(|| SageError::Reader("embedder returned empty".into()))?
            .clone();

        // Soft-addressing scores → softmax probabilities for seeding h⁽⁰⁾.
        let raw_scores: Vec<Score> = entities
            .iter()
            .map(|e| score_entry(e, &plan, &self.weights, Some(&q_emb)))
            .collect();
        let probs = softmax_entry(&raw_scores, self.weights.t0);

        // Build the GFM graph view: index entities into row positions, collect
        // edges among them, compute out-degree.
        let id_to_idx: AHashMap<EntityId, usize> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let n = entities.len();

        let mut edges_flat: Vec<(usize, usize)> = Vec::new();
        let mut deg = vec![0.0f32; n];
        for (u_idx, e) in entities.iter().enumerate() {
            let nbrs = g.neighbors(t, e.id, usize::MAX).await?;
            for (dst, _edge) in nbrs {
                if let Some(&v_idx) = id_to_idx.get(&dst) {
                    edges_flat.push((u_idx, v_idx));
                    deg[u_idx] += 1.0;
                }
            }
        }

        // Seed h⁽⁰⁾: each row = p₀(e|q) · query_embedding. With identity-like
        // weights this collapses to a per-node scalar; with trained W_q it
        // would project query semantics into the GFM hidden space.
        let mut h_in = vec![0.0f32; n * d];
        for (i, p) in probs.iter().enumerate() {
            let scale = p.powf(self.weights.eta);
            let row = i * d;
            for (j, qv) in q_emb.iter().enumerate() {
                h_in[row + j] = scale * qv;
            }
        }

        let view = GfmGraphView {
            n_nodes: n,
            edges: &edges_flat,
            deg: &deg,
        };
        let h_out = self.layer.forward(&view, &h_in)?;

        // Entity-level score: L1 norm of h_out row, mixed with the soft-addressing
        // prior so noise-amplifying random weights can't completely override
        // the prior signal.
        let mut entity_scores: Vec<(EntityId, Score)> = Vec::with_capacity(n);
        for (i, e) in entities.iter().enumerate() {
            let row = &h_out[i * d..(i + 1) * d];
            let l1: f32 = row.iter().map(|x| x.abs()).sum();
            let blended = 0.5 * probs[i] + 0.5 * (l1 / (l1 + 1.0));
            entity_scores.push((e.id, blended));
        }
        entity_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Aggregate into doc scores via source_docs.
        let mut doc_scores: AHashMap<DocId, Score> = AHashMap::new();
        for (eid, s) in &entity_scores {
            let docs = g.docs_of_entity(t, *eid).await?;
            for did in docs {
                *doc_scores.entry(did).or_default() += *s;
            }
        }
        let mut docs: Vec<(DocId, Score)> = doc_scores.into_iter().collect();
        docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        docs.truncate(q.k.max(1));

        Ok(ReadOutput {
            docs,
            entities: entity_scores,
            subgraph: Subgraph::default(),
            paths: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedder(d: usize) -> Arc<dyn Embedder> {
        Arc::new(sage_embed::DeterministicEmbedder::new(d))
    }

    #[test]
    fn dim_mismatch_constructor_is_caught_at_read_time() {
        // Embedder=64, gfm hidden=32 — pipeline must surface SageError::Reader.
        let cfg = GfmConfig {
            hidden_dim: 32,
            ..GfmConfig::default()
        };
        let r = GfmReader::new(make_embedder(64), cfg, 0);
        assert_eq!(r.layer().cfg().hidden_dim, 32);
        assert_eq!(r.embedder.dim(), 64);
    }
}
