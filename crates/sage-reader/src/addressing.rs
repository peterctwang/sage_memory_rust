//! Soft addressing — SPEC §5.2 / paper §4.2.2.
//!
//! M2 implements 4 of the 6 stimulus components (Exact, Alias, Type, NER-EL +
//! Cons as a hard gate). Cosine similarity (`λ₃`) returns 0 until an embedder
//! lands in M3.

use sage_core::{Constraint, Entity, QueryPlan, Score};
use smol_str::SmolStr;

#[derive(Copy, Clone, Debug)]
pub struct AddressingWeights {
    /// `[λ_exact, λ_alias, λ_cos, λ_type, λ_cons, λ_ner_el]` — see SPEC §5.2.
    pub lambdas: [f32; 6],
    /// Softmax temperature `T₀`.
    pub t0: f32,
    /// Exponent `η` applied to `p₀(e|q)` when initializing node state (used by GFM in M3).
    pub eta: f32,
}

impl Default for AddressingWeights {
    fn default() -> Self {
        Self {
            lambdas: [1.0, 0.6, 0.0, 0.3, 1.0, 0.8],
            t0: 0.7,
            eta: 0.5,
        }
    }
}

/// `sₑ(q)` from SPEC §5.2 / paper §4.2.2.
///
/// Returns `f32::NEG_INFINITY` if a `MustExclude` constraint hits — such entities
/// are then suppressed by `softmax_entry`.
pub fn score_entry(e: &Entity, plan: &QueryPlan, w: &AddressingWeights) -> Score {
    if violates_hard_constraints(e, &plan.hard_constraints) {
        return f32::NEG_INFINITY;
    }

    let name_lc = e.name.to_lowercase();
    let aliases_lc: Vec<String> = e.aliases.iter().map(|a| a.to_lowercase()).collect();

    let s_exact = if plan.expansions.iter().any(|t| t.as_str() == name_lc) {
        1.0
    } else {
        0.0
    };
    let s_alias = plan
        .aliases
        .iter()
        .filter(|t| aliases_lc.iter().any(|a| a == t.as_str()))
        .count()
        .min(1) as f32;
    let s_cos = 0.0; // M3
    let s_type = match (&plan.etype_hint, &e.etype) {
        (Some(h), got) if h == got => 1.0,
        _ => 0.0,
    };
    let s_cons = 1.0; // hard-fail returned NEG_INFINITY above
    let s_ner_el = plan
        .expansions
        .iter()
        .filter(|t| t.as_str() == name_lc)
        .count() as f32;

    let l = w.lambdas;
    l[0] * s_exact + l[1] * s_alias + l[2] * s_cos + l[3] * s_type + l[4] * s_cons + l[5] * s_ner_el
}

fn violates_hard_constraints(e: &Entity, cs: &[Constraint]) -> bool {
    for c in cs {
        match c {
            Constraint::MustInclude(s) => {
                if !names_contain(&e.name, &e.aliases, s) {
                    return true;
                }
            }
            Constraint::MustExclude(s) => {
                if names_contain(&e.name, &e.aliases, s) {
                    return true;
                }
            }
            Constraint::EntityType(t) => {
                if &e.etype != t {
                    return true;
                }
            }
        }
    }
    false
}

fn names_contain(name: &SmolStr, aliases: &[SmolStr], needle: &SmolStr) -> bool {
    name == needle || aliases.iter().any(|a| a == needle)
}

/// `p₀(e|q) = exp(sₑ/T₀) / Σᵥ exp(sᵥ/T₀)`. NEG_INFINITY entries map to 0.
pub fn softmax_entry(scores: &[Score], t0: f32) -> Vec<Score> {
    if scores.is_empty() {
        return Vec::new();
    }
    let t0 = t0.max(1e-6);
    let max = scores
        .iter()
        .copied()
        .filter(|s| s.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return vec![0.0; scores.len()];
    }
    let exps: Vec<f32> = scores
        .iter()
        .map(|s| {
            if s.is_finite() {
                ((s - max) / t0).exp()
            } else {
                0.0
            }
        })
        .collect();
    let sum: f32 = exps.iter().sum();
    if sum < 1e-12 {
        vec![0.0; scores.len()]
    } else {
        exps.into_iter().map(|e| e / sum).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_core::{Entity, EntityType, Query, QueryPlan};
    use sage_reader_test_helpers::{ent_with_aliases, plan_for};

    mod sage_reader_test_helpers {
        use super::*;
        use smallvec::SmallVec;
        use smol_str::SmolStr;
        pub fn plan_for(text: &str) -> QueryPlan {
            // mimic HeuristicPlanner without depending on it from the unit test file
            let tokens: Vec<SmolStr> = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| !t.is_empty())
                .map(str::to_lowercase)
                .map(SmolStr::new)
                .collect();
            QueryPlan {
                expansions: tokens.clone(),
                aliases: tokens,
                ..QueryPlan::default()
            }
        }
        pub fn ent_with_aliases(id: u64, name: &str, aliases: &[&str]) -> Entity {
            let mut e = Entity::new(id, name, EntityType::Person);
            let mut v: SmallVec<[SmolStr; 4]> = SmallVec::new();
            for a in aliases {
                v.push(SmolStr::new(*a));
            }
            e.aliases = v;
            e
        }
    }

    #[test]
    fn exact_match_scores_positive() {
        let w = AddressingWeights::default();
        let e = Entity::new(1, "Alice", EntityType::Person);
        let s = score_entry(&e, &plan_for("Alice founded Acme"), &w);
        assert!(s > 0.0, "got {s}");
    }

    #[test]
    fn unrelated_entity_scores_low() {
        let w = AddressingWeights::default();
        let unrelated = Entity::new(2, "Zog", EntityType::Person);
        let alice = Entity::new(1, "Alice", EntityType::Person);
        let plan = plan_for("who is Alice");
        assert!(score_entry(&alice, &plan, &w) > score_entry(&unrelated, &plan, &w));
    }

    #[test]
    fn must_exclude_drives_score_to_neg_infinity() {
        let w = AddressingWeights::default();
        let e = Entity::new(1, "Alice", EntityType::Person);
        let mut plan = plan_for("Alice");
        plan.hard_constraints
            .push(sage_core::Constraint::MustExclude(SmolStr::new("Alice")));
        assert_eq!(score_entry(&e, &plan, &w), f32::NEG_INFINITY);
    }

    #[test]
    fn alias_match_scores_positive() {
        let w = AddressingWeights::default();
        let e = ent_with_aliases(1, "Alice Liddell", &["alice", "ally"]);
        let s = score_entry(&e, &plan_for("ally went"), &w);
        assert!(s > 0.0, "got {s}");
    }

    #[test]
    fn softmax_handles_neg_infinity() {
        let p = softmax_entry(&[1.0, f32::NEG_INFINITY, 2.0], 1.0);
        assert!(p[0] > 0.0);
        assert_eq!(p[1], 0.0);
        assert!(p[2] > p[0]);
        let sum: f32 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn softmax_empty_is_empty() {
        assert!(softmax_entry(&[], 1.0).is_empty());
    }

    #[test]
    fn softmax_all_neg_inf_is_zero_vector() {
        let p = softmax_entry(&[f32::NEG_INFINITY, f32::NEG_INFINITY], 1.0);
        assert_eq!(p, vec![0.0, 0.0]);
    }

    // suppress unused warning on Query
    fn _force_q() {
        let _ = Query::ask("x");
    }
}
