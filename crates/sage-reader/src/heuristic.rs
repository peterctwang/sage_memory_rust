//! `HeuristicReader` — `HeuristicPlanner` + soft addressing + entity-to-doc
//! aggregation via `source_docs`. Pure CPU; latency O(|V_t|) per query.

use ahash::AHashMap;
use async_trait::async_trait;
use sage_core::{
    DocId, EntityId, Query, ReadOutput, Reader, ReaderGraph, Result, Score, Subgraph, TenantId,
};

use crate::addressing::{score_entry, softmax_entry, AddressingWeights};
use crate::planner::{HeuristicPlanner, QueryPlanner};

#[derive(Debug)]
pub struct HeuristicReader<P: QueryPlanner = HeuristicPlanner> {
    planner: P,
    weights: AddressingWeights,
    subgraph_hops: u8,
    max_subgraph_seeds: usize,
}

impl Default for HeuristicReader<HeuristicPlanner> {
    fn default() -> Self {
        Self {
            planner: HeuristicPlanner::new(),
            weights: AddressingWeights::default(),
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
            subgraph_hops: 1,
            max_subgraph_seeds: 16,
        }
    }

    pub fn with_weights(mut self, w: AddressingWeights) -> Self {
        self.weights = w;
        self
    }
}

#[async_trait]
impl<P: QueryPlanner + std::fmt::Debug> Reader for HeuristicReader<P> {
    async fn read(&self, t: TenantId, q: &Query, g: &dyn ReaderGraph) -> Result<ReadOutput> {
        let plan = self.planner.plan(q);
        let all = g.all_entities(t).await?;
        if all.is_empty() {
            return Ok(ReadOutput::default());
        }

        let raw_scores: Vec<Score> = all
            .iter()
            .map(|e| score_entry(e, &plan, &self.weights))
            .collect();
        let probs = softmax_entry(&raw_scores, self.weights.t0);

        let mut entities: Vec<(EntityId, Score)> = all
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
