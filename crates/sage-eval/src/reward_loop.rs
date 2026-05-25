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

/// Batch reward computation — useful for GRPO trajectory groups (M4).
///
/// `triples_per_sample` must be the same length as `samples`; element `i` is
/// the triple set the writer emitted for sample `i`. Pass empty slice if you
/// only want the retrieval-based components.
///
/// `judge_per_sample` likewise — pass `vec![JudgeInputs::default(); N]` if
/// you have no judge LLM yet.
pub async fn compute_reward_batch<R: sage_core::Reader + ?Sized>(
    reader: &R,
    tenant: TenantId,
    graph: &dyn ReaderGraph,
    samples: &[EvalSample],
    triples_per_sample: &[Vec<(EntityId, SmolStr, EntityId)>],
    judge_per_sample: &[JudgeInputs],
) -> Result<Vec<WriterReward>> {
    if !triples_per_sample.is_empty() && triples_per_sample.len() != samples.len() {
        return Err(sage_core::SageError::Invalid(format!(
            "triples_per_sample.len()={} != samples.len()={}",
            triples_per_sample.len(),
            samples.len()
        )));
    }
    if !judge_per_sample.is_empty() && judge_per_sample.len() != samples.len() {
        return Err(sage_core::SageError::Invalid(format!(
            "judge_per_sample.len()={} != samples.len()={}",
            judge_per_sample.len(),
            samples.len()
        )));
    }
    let mut out = Vec::with_capacity(samples.len());
    for (i, sample) in samples.iter().enumerate() {
        let triples: &[(EntityId, SmolStr, EntityId)] =
            triples_per_sample.get(i).map_or(&[][..], Vec::as_slice);
        let judge = judge_per_sample.get(i).copied().unwrap_or_default();
        out.push(compute_reward_for_sample(reader, tenant, graph, sample, triples, judge).await?);
    }
    Ok(out)
}
