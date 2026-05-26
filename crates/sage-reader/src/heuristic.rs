//! `HeuristicReader` — `HeuristicPlanner` + soft addressing + entity-to-doc
//! aggregation via `source_docs`. Pure CPU; latency O(|V_t|) per query.

use std::sync::Arc;

use ahash::AHashMap;
use async_trait::async_trait;
use sage_core::{
    DocId, Embedder, Entity, EntityId, Query, ReadOutput, Reader, ReaderGraph, Result, Score,
    Subgraph, TenantId, VectorIndex,
};

use crate::addressing::{score_entry, softmax_entry, AddressingWeights};
use crate::planner::{HeuristicPlanner, QueryPlanner};

pub struct HeuristicReader<P: QueryPlanner = HeuristicPlanner> {
    planner: P,
    weights: AddressingWeights,
    embedder: Option<Arc<dyn Embedder>>,
    /// Optional ANN accelerator. When present together with `embedder`, the
    /// reader narrows scoring from `O(|V_t|)` to `O(narrow_k · log |V_t|)`.
    vector_index: Option<Arc<dyn VectorIndex>>,
    /// Top-K candidate cap pulled from the index per query (default 256).
    narrow_k: usize,
    subgraph_hops: u8,
    max_subgraph_seeds: usize,
}

impl<P: QueryPlanner + std::fmt::Debug> std::fmt::Debug for HeuristicReader<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeuristicReader")
            .field("planner", &self.planner)
            .field("weights", &self.weights)
            .field("embedder", &self.embedder.as_ref().map(|e| e.dim()))
            .field("vector_index", &self.vector_index.as_ref().map(|i| i.len()))
            .field("narrow_k", &self.narrow_k)
            .field("subgraph_hops", &self.subgraph_hops)
            .finish_non_exhaustive()
    }
}

impl Default for HeuristicReader<HeuristicPlanner> {
    fn default() -> Self {
        Self {
            planner: HeuristicPlanner::new(),
            weights: AddressingWeights::default(),
            embedder: None,
            vector_index: None,
            narrow_k: 256,
            subgraph_hops: 1,
            max_subgraph_seeds: 16,
        }
    }
}

impl<P: QueryPlanner> HeuristicReader<P> {
    pub fn with_planner(planner: P) -> Self {
        Self {
            planner,
            weights: AddressingWeights::default(),
            embedder: None,
            vector_index: None,
            narrow_k: 256,
            subgraph_hops: 1,
            max_subgraph_seeds: 16,
        }
    }

    pub fn with_weights(mut self, w: AddressingWeights) -> Self {
        self.weights = w;
        self
    }

    pub fn with_embedder(mut self, e: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(e);
        self
    }

    /// Attach an ANN accelerator. Without an embedder, this is a no-op
    /// (the reader still walks the full entity set).
    pub fn with_vector_index(mut self, idx: Arc<dyn VectorIndex>) -> Self {
        self.vector_index = Some(idx);
        self
    }

    /// Override the top-K cap used to narrow candidates from the ANN index.
    pub fn with_narrow_k(mut self, k: usize) -> Self {
        self.narrow_k = k.max(1);
        self
    }

    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }
    pub fn vector_index(&self) -> Option<&Arc<dyn VectorIndex>> {
        self.vector_index.as_ref()
    }
}

#[async_trait]
impl<P: QueryPlanner + std::fmt::Debug> Reader for HeuristicReader<P> {
    async fn read(&self, t: TenantId, q: &Query, g: &dyn ReaderGraph) -> Result<ReadOutput> {
        let plan = self.planner.plan(q).await?;

        let query_emb: Option<Arc<[f32]>> = match &self.embedder {
            Some(e) => Some(Arc::clone(
                e.embed(&[q.text.as_ref()]).await?.first().ok_or_else(|| {
                    sage_core::SageError::Reader("embedder returned empty".into())
                })?,
            )),
            None => None,
        };
        let q_emb_ref: Option<&[f32]> = query_emb.as_deref();

        // Candidate selection: if ANN index + query embedding both present, narrow
        // to top-K from the index — O(narrow_k · log |V|) instead of O(|V|).
        // Otherwise fall back to scanning all entities.
        let candidates: Vec<Entity> = match (&self.vector_index, q_emb_ref) {
            (Some(idx), Some(q_vec)) if !idx.is_empty() => {
                let hits = idx.search(q_vec, self.narrow_k)?;
                let mut ents = Vec::with_capacity(hits.len());
                for (eid, _) in &hits {
                    if let Some(e) = g.get_entity(t, *eid).await? {
                        ents.push(e);
                    }
                }
                ents
            }
            _ => g.all_entities(t).await?,
        };
        if candidates.is_empty() {
            return Ok(ReadOutput::default());
        }

        let raw_scores: Vec<Score> = candidates
            .iter()
            .map(|e| score_entry(e, &plan, &self.weights, q_emb_ref))
            .collect();
        let probs = softmax_entry(&raw_scores, self.weights.t0);

        let mut entities: Vec<(EntityId, Score)> = candidates
            .iter()
            .zip(probs.iter().copied())
            .filter(|(_, p)| *p > 0.0)
            .map(|(e, p)| (e.id, p))
            .collect();
        entities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut doc_scores: AHashMap<DocId, Score> = AHashMap::new();
        for (eid, p) in &entities {
            let docs = g.docs_of_entity(t, *eid).await?;
            for d in docs {
                *doc_scores.entry(d).or_default() += *p;
            }
        }
        let mut docs: Vec<(DocId, Score)> = doc_scores.into_iter().collect();
        docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        docs.truncate(q.k.max(1));

        let seed_ids: Vec<EntityId> = entities
            .iter()
            .take(self.max_subgraph_seeds)
            .map(|(id, _)| *id)
            .collect();
        let subgraph = if seed_ids.is_empty() {
            Subgraph::default()
        } else {
            g.k_hop(t, &seed_ids, self.subgraph_hops).await?
        };

        Ok(ReadOutput {
            docs,
            entities,
            subgraph,
            paths: Vec::new(),
        })
    }
}
