//! Convenience helper that wires `Reader` + sample → `compute_reward`.
//!
//! Use this when you want a `WriterReward` derived from a real retrieval pass.
//! Judge-based components (`r_ded`, `r_ans`) are caller-supplied; this helper
//! handles the pure-data terms (`r_rec`, `r_pre`, `rep_penalty`).

use sage_core::{
    compute_reward, DocId, EntityId, ReaderGraph, Result, RewardInputs, TenantId, WriterReward,
};
use smol_str::SmolStr;

use crate::runner::EvalSample;

/// Inputs that are supplied externally because they depend on an LLM judge or
/// answer-string matcher (M3+).
#[derive(Debug, Default, Clone, Copy)]
pub struct JudgeInputs {
    pub r_ded: f32,
    pub r_ans: f32,
    pub fmt_bonus: f32,
}

pub async fn compute_reward_for_sample<R: sage_core::Reader + ?Sized>(
    reader: &R,
    tenant: TenantId,
    graph: &dyn ReaderGraph,
    sample: &EvalSample,
    triples: &[(EntityId, SmolStr, EntityId)],
    judge: JudgeInputs,
) -> Result<WriterReward> {
    let out = reader.read(tenant, &sample.query, graph).await?;
    let retrieved: Vec<DocId> = out.docs.iter().map(|(d, _)| *d).collect();
    Ok(compute_reward(RewardInputs {
        retrieved: &retrieved,
        ground_truth: &sample.ground_truth,
        triples,
        r_ded: judge.r_ded,
        r_ans: judge.r_ans,
        fmt_bonus: judge.fmt_bonus,
    }))
}
