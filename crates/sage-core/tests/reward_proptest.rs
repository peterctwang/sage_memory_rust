//! Property tests for reward primitives — invariants from SPEC §A.2 / §4.2.

use proptest::prelude::*;
use sage_core::{precision, recovery, repetition_penalty, RewardCfg, TaskWeights, WriterReward};
use tests_support::strategies::{arb_doc_ids, arb_triple_simple};

proptest! {
    #[test]
    fn recovery_in_unit_interval(retrieved in arb_doc_ids(20), gt in arb_doc_ids(20)) {
        let v = recovery(&retrieved, &gt);
        prop_assert!((0.0..=1.0).contains(&v), "recovery={v}");
    }

    #[test]
    fn precision_in_unit_interval(retrieved in arb_doc_ids(20), gt in arb_doc_ids(20)) {
        let v = precision(&retrieved, &gt);
        prop_assert!((0.0..=1.0).contains(&v), "precision={v}");
    }

    #[test]
    fn repetition_penalty_in_unit_interval(t in prop::collection::vec(arb_triple_simple(), 0..30)) {
        let v = repetition_penalty(&t);
        prop_assert!((0.0..=1.0).contains(&v), "ρ_rep={v}");
    }

    #[test]
    fn task_reward_in_unit_when_inputs_clamped(
        rec in 0.0f32..=1.0,
        pre in 0.0f32..=1.0,
        ded in 0.0f32..=1.0,
    ) {
        let r = WriterReward { r_rec: rec, r_pre: pre, r_ded: ded, ..Default::default() };
        let v = r.task(&TaskWeights::default());
        prop_assert!((0.0..=1.0).contains(&v), "task={v}");
    }

    #[test]
    fn trajectory_subtracts_penalty(
        rec in 0.0f32..=1.0,
        pen in 0.0f32..=1.0,
    ) {
        let r = WriterReward { r_rec: rec, rep_penalty: pen, ..Default::default() };
        let cfg = RewardCfg::default();
        let trj = r.trajectory(&cfg);
        let task = r.task(&cfg.weights);
        prop_assert!(trj <= task + 1e-6,
            "trajectory ({trj}) must not exceed task ({task}) when fmt_bonus=0");
    }
}
