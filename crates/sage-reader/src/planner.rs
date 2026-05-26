//! Query planner — SPEC §5.1.
//!
//! `HeuristicPlanner` derives expansions/aliases/probes from the raw query string
//! without an LLM (M2 baseline and fallback). `LlmQueryPlanner` (separate module)
//! uses an LLM for cognition-inspired expansion (SPEC §5.1 / paper §4.2.1).

use async_trait::async_trait;
use sage_core::{Probe, Query, QueryPlan, Result};
use smol_str::SmolStr;
use std::sync::Arc;

#[async_trait]
pub trait QueryPlanner: Send + Sync {
    async fn plan(&self, q: &Query) -> Result<QueryPlan>;
}

#[derive(Debug, Default)]
pub struct HeuristicPlanner {
    pub min_token_len: usize,
}

impl HeuristicPlanner {
    pub fn new() -> Self {
        Self { min_token_len: 2 }
    }

    /// Synchronous flavor — useful from non-async contexts and tests.
    pub fn plan_sync(&self, q: &Query) -> QueryPlan {
        let tokens: Vec<SmolStr> = q
            .text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= self.min_token_len)
            .map(str::to_lowercase)
            .filter(|t| !is_stopword(t))
            .map(SmolStr::new)
            .collect();

        let probes = vec![Probe {
            text: Arc::clone(&q.text),
            alpha: 1.0,
            etype: None,
        }];

        QueryPlan {
            expansions: tokens.clone(),
            aliases: tokens,
            relations: Vec::new(),
            hard_constraints: Vec::new(),
            etype_hint: None,
            probes,
        }
    }
}

#[async_trait]
impl QueryPlanner for HeuristicPlanner {
    async fn plan(&self, q: &Query) -> Result<QueryPlan> {
        Ok(self.plan_sync(q))
    }
}

fn is_stopword(t: &str) -> bool {
    matches!(
        t,
        "the"
            | "a"
            | "an"
            | "of"
            | "to"
            | "in"
            | "is"
            | "and"
            | "or"
            | "for"
            | "on"
            | "at"
            | "by"
            | "who"
            | "what"
            | "when"
            | "where"
            | "why"
            | "how"
            | "did"
            | "does"
            | "do"
            | "was"
            | "were"
            | "be"
            | "been"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_lowercased_tokens() {
        let p = HeuristicPlanner::new();
        let plan = p.plan_sync(&Query::ask("Who founded Acme Corp?"));
        assert!(plan.expansions.iter().any(|t| t == "founded"));
        assert!(plan.expansions.iter().any(|t| t == "acme"));
        assert!(plan.expansions.iter().any(|t| t == "corp"));
    }

    #[test]
    fn drops_stopwords() {
        let p = HeuristicPlanner::new();
        let plan = p.plan_sync(&Query::ask("the cat is on the mat"));
        assert!(!plan.expansions.iter().any(|t| t == "the"));
        assert!(!plan.expansions.iter().any(|t| t == "is"));
        assert!(plan.expansions.iter().any(|t| t == "cat"));
    }

    #[test]
    fn produces_one_probe_with_full_text() {
        let p = HeuristicPlanner::new();
        let plan = p.plan_sync(&Query::ask("xyz"));
        assert_eq!(plan.probes.len(), 1);
        assert_eq!(plan.probes[0].alpha, 1.0);
    }
}
