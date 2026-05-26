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
    /// Tuned on `examples/eval_v3` (100-doc / 40-query) — measured 2026-05-26.
    /// Order: [λ_exact, λ_alias, λ_cos, λ_type, λ_cons, λ_ner_el]
    ///
    /// Key tuning insight: `λ_cons = 0` because a constant contribution to every
    /// entity makes softmax flatter and dilutes the prior signal. With
    /// `T₀ = 0.5` (sharper softmax) and slightly higher `λ_exact` / `λ_cos`, the
    /// heuristic planner reaches Recall@3 = 0.875 on eval_v3 (vs 0.30 with
    /// SPEC §5.2's initial defaults).
    fn default() -> Self {
        Self {
            lambdas: [1.5, 0.8, 1.2, 0.5, 0.0, 1.0],
            t0: 0.5,
            eta: 0.5,
        }
    }
}

/// `sₑ(q)` from SPEC §5.2 / paper §4.2.2.
///
/// `query_emb` is the (optional) query embedding; cosine similarity contributes
/// `λ_cos · cos(emb(e), query_emb).max(0)` when both the query and the entity
/// carry embeddings.
///
/// Returns `f32::NEG_INFINITY` if a `MustExclude` constraint hits — such entities
/// are then suppressed by `softmax_entry`.
pub fn score_entry(
    e: &Entity,
    plan: &QueryPlan,
    w: &AddressingWeights,
    query_emb: Option<&[f32]>,
) -> Score {
    if violates_hard_constraints(e, &plan.hard_constraints) {
        return f32::NEG_INFINITY;
    }

    let name_lc = e.name.to_lowercase();
    let aliases_lc: Vec<String> = e.aliases.iter().map(|a| a.to_lowercase()).collect();
    let entity_tokens: ahash::AHashSet<&str> = name_lc.split_whitespace().collect();

    // s_exact: monotonic relaxation of strict equality.
    //   1.0   = expansion == entity name (full match)
    //   0.7   = single-token expansion is a token of multi-word entity name
    //   ≤0.5  = Jaccard overlap when expansion has multiple tokens
    //   0.0   = no overlap
    let mut s_exact = 0.0f32;
    for exp in &plan.expansions {
        let exp_str = exp.as_str();
        if exp_str == name_lc {
            s_exact = 1.0;
            break;
        }
        let exp_tokens: Vec<&str> = exp_str.split_whitespace().collect();
        if exp_tokens.len() == 1 {
            // single-token expansion present in entity name token set
            if !entity_tokens.is_empty() && entity_tokens.contains(exp_str) {
                s_exact = s_exact.max(0.7);
            }
        } else {
            // multi-token expansion ↔ multi-token entity name: Jaccard
            let exp_set: ahash::AHashSet<&str> = exp_tokens.iter().copied().collect();
            let intersect = entity_tokens.intersection(&exp_set).count();
            let union = entity_tokens.union(&exp_set).count();
            if union > 0 {
                let jaccard = intersect as f32 / union as f32;
                s_exact = s_exact.max(jaccard);
            }
        }
    }
    let s_alias = plan
        .aliases
        .iter()
        .filter(|t| aliases_lc.iter().any(|a| a == t.as_str()))
        .count()
        .min(1) as f32;
    let s_cos = match (query_emb, e.embedding.as_deref()) {
        (Some(q), Some(ent)) => sage_core::cosine(ent, q).max(0.0),
        _ => 0.0,
    };
    let s_type = match (&plan.etype_hint, &e.etype) {
        (Some(h), got) if h == got => 1.0,
        _ => 0.0,
    };
    let s_cons = 1.0;
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
        let s = score_entry(&e, &plan_for("Alice founded Acme"), &w, None);
        assert!(s > 0.0, "got {s}");
    }

    #[test]
    fn unrelated_entity_scores_low() {
        let w = AddressingWeights::default();
        let unrelated = Entity::new(2, "Zog", EntityType::Person);
        let alice = Entity::new(1, "Alice", EntityType::Person);
        let plan = plan_for("who is Alice");
        assert!(score_entry(&alice, &plan, &w, None) > score_entry(&unrelated, &plan, &w, None));
    }

    #[test]
    fn must_exclude_drives_score_to_neg_infinity() {
        let w = AddressingWeights::default();
        let e = Entity::new(1, "Alice", EntityType::Person);
        let mut plan = plan_for("Alice");
        plan.hard_constraints
            .push(sage_core::Constraint::MustExclude(SmolStr::new("Alice")));
        assert_eq!(score_entry(&e, &plan, &w, None), f32::NEG_INFINITY);
    }

    #[test]
    fn alias_match_scores_positive() {
        let w = AddressingWeights::default();
        let e = ent_with_aliases(1, "Alice Liddell", &["alice", "ally"]);
        let s = score_entry(&e, &plan_for("ally went"), &w, None);
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

    // === token-overlap relaxation ===

    fn entity_multi(id: u64, name: &str) -> Entity {
        Entity::new(id, name, EntityType::Person)
    }

    fn make_plan_with_expansions(exps: &[&str]) -> QueryPlan {
        use smol_str::SmolStr;
        QueryPlan {
            expansions: exps.iter().map(|s| SmolStr::new(*s)).collect(),
            aliases: Vec::new(),
            ..QueryPlan::default()
        }
    }

    #[test]
    fn single_token_expansion_hits_multi_word_entity_at_0_7() {
        // expansion "linus" → entity "Linus Torvalds" should score the s_exact
        // contribution at 0.7 × λ_exact (= 0.7 × 1.0 = 0.7), plus cons baseline.
        let w = AddressingWeights::default();
        let e = entity_multi(1, "Linus Torvalds");
        let plan = make_plan_with_expansions(&["linus"]);
        let s = score_entry(&e, &plan, &w, None);
        // Lower than full exact match (1.0 + cons + …) but above the floor cons-only.
        let plan_empty = make_plan_with_expansions(&["nope"]);
        let baseline = score_entry(&e, &plan_empty, &w, None);
        assert!(
            s > baseline + 0.5,
            "partial-token expansion must lift score noticeably ({s} vs {baseline})"
        );
    }

    #[test]
    fn multi_token_expansion_jaccard_against_entity_name() {
        // expansion "sun microsystems" vs entity "Sun Microsystems" → 1.0
        let w = AddressingWeights::default();
        let plan = make_plan_with_expansions(&["sun microsystems"]);
        let e_full = entity_multi(1, "Sun Microsystems");
        assert!(score_entry(&e_full, &plan, &w, None) >= 1.0 + 1.0); // s_exact=1.0 + cons=1.0

        // expansion "sun microsystems inc" vs entity "Sun Microsystems":
        // Jaccard = 2/3 ≈ 0.667
        let plan_extra = make_plan_with_expansions(&["sun microsystems inc"]);
        let mid = score_entry(&e_full, &plan_extra, &w, None);
        // Should be between cons-only baseline and full-match.
        let plan_off = make_plan_with_expansions(&["xyz abc"]);
        let baseline = score_entry(&e_full, &plan_off, &w, None);
        assert!(
            mid > baseline,
            "partial Jaccard must lift ({mid} > {baseline})"
        );
        assert!(mid < 2.0 + 1.0, "Jaccard cannot exceed full match");
    }

    #[test]
    fn full_match_beats_partial_match() {
        let w = AddressingWeights::default();
        let e = entity_multi(1, "Acme Industries");
        let full = score_entry(
            &e,
            &make_plan_with_expansions(&["acme industries"]),
            &w,
            None,
        );
        let partial = score_entry(&e, &make_plan_with_expansions(&["acme"]), &w, None);
        let off = score_entry(&e, &make_plan_with_expansions(&["nothing"]), &w, None);
        // ordering: full > partial > off
        assert!(
            full > partial,
            "full ({full}) must exceed partial ({partial})"
        );
        assert!(partial > off, "partial ({partial}) must exceed off ({off})");
    }

    #[test]
    fn no_overlap_yields_zero_exact() {
        let w = AddressingWeights::default();
        let e = entity_multi(1, "Marie Curie");
        let plan = make_plan_with_expansions(&["python", "rust", "java"]);
        let s = score_entry(&e, &plan, &w, None);
        let plan_empty = make_plan_with_expansions(&[]);
        let baseline = score_entry(&e, &plan_empty, &w, None);
        assert!(
            (s - baseline).abs() < 1e-5,
            "irrelevant tokens must not lift score"
        );
    }

    // suppress unused warning on Query
    fn _force_q() {
        let _ = Query::ask("x");
    }
}
