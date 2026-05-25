use std::sync::Arc;

use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::entity::EntityType;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Query {
    pub text: Arc<str>,
    pub embedding: Option<Arc<[f32]>>,
    pub k: usize,
}

impl Query {
    pub fn ask(text: impl Into<Arc<str>>) -> Self {
        Self {
            text: text.into(),
            embedding: None,
            k: 5,
        }
    }

    pub fn with_k(mut self, k: usize) -> Self {
        self.k = k;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Probe {
    pub text: Arc<str>,
    pub alpha: f32,
    pub etype: Option<EntityType>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Constraint {
    MustInclude(SmolStr),
    MustExclude(SmolStr),
    EntityType(EntityType),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueryPlan {
    pub expansions: Vec<SmolStr>,
    pub aliases: Vec<SmolStr>,
    pub relations: Vec<SmolStr>,
    pub hard_constraints: Vec<Constraint>,
    pub etype_hint: Option<EntityType>,
    pub probes: Vec<Probe>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_ask_defaults_k_to_5() {
        let q = Query::ask("who?");
        assert_eq!(q.k, 5);
    }

    #[test]
    fn query_with_k() {
        assert_eq!(Query::ask("?").with_k(20).k, 20);
    }

    #[test]
    fn query_plan_default_is_empty() {
        let p = QueryPlan::default();
        assert!(p.expansions.is_empty());
        assert!(p.probes.is_empty());
        assert!(p.etype_hint.is_none());
    }
}
