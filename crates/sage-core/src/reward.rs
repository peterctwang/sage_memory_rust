//! Writer reward — SPEC §4.2, paper §4.1.

use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::ids::EntityId;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct TaskWeights {
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
}

impl Default for TaskWeights {
    fn default() -> Self {
        Self {
            alpha: 0.4,
            beta: 0.3,
            gamma: 0.3,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RewardCfg {
    pub weights: TaskWeights,
    pub lambda_rep: f32,
    pub lambda_fmt: f32,
}

impl Default for RewardCfg {
    fn default() -> Self {
        Self {
            weights: TaskWeights::default(),
            lambda_rep: 0.2,
            lambda_fmt: 0.05,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct WriterReward {
    pub r_rec: f32,
    pub r_pre: f32,
    pub r_ded: f32,
    pub r_ans: f32,
    pub rep_penalty: f32,
    pub fmt_bonus: f32,
}

impl WriterReward {
    /// r_task = (α·r_rec + β·r_pre + γ·r_ded) / (α+β+γ)
    pub fn task(&self, w: &TaskWeights) -> f32 {
        let denom = w.alpha + w.beta + w.gamma;
        if denom.abs() < f32::EPSILON {
            return 0.0;
        }
        (w.alpha * self.r_rec + w.beta * self.r_pre + w.gamma * self.r_ded) / denom
    }

    /// R(τ) = r_task − λ_rep·ρ_rep + λ_fmt·Σ r_fmt
    pub fn trajectory(&self, cfg: &RewardCfg) -> f32 {
        self.task(&cfg.weights) - cfg.lambda_rep * self.rep_penalty
            + cfg.lambda_fmt * self.fmt_bonus
    }
}

/// Pure inputs needed to score one writer trajectory; the LLM-dependent
/// terms (`r_ded`, `r_ans`, `fmt_bonus`) are passed in from the orchestrating
/// runtime so that this module stays backend-neutral.
#[derive(Debug, Clone, Copy)]
pub struct RewardInputs<'a> {
    pub retrieved: &'a [crate::DocId],
    pub ground_truth: &'a [crate::DocId],
    pub triples: &'a [(EntityId, SmolStr, EntityId)],
    pub r_ded: f32,
    pub r_ans: f32,
    pub fmt_bonus: f32,
}

/// Assemble a full `WriterReward` from precomputed inputs.
pub fn compute_reward(inp: RewardInputs<'_>) -> WriterReward {
    WriterReward {
        r_rec: recovery(inp.retrieved, inp.ground_truth),
        r_pre: precision(inp.retrieved, inp.ground_truth),
        r_ded: inp.r_ded.clamp(0.0, 1.0),
        r_ans: inp.r_ans.clamp(0.0, 1.0),
        rep_penalty: repetition_penalty(inp.triples),
        fmt_bonus: inp.fmt_bonus.clamp(0.0, 1.0),
    }
}

/// Edge habituation — attenuates weight with each repeated activation.
/// `w / (1 + rate · hits)`. SPEC §13 #4.
pub fn habituation(weight: f32, hit_count: u32, rate: f32) -> f32 {
    let denom = 1.0 + rate.max(0.0) * hit_count as f32;
    if denom < 1e-12 {
        weight
    } else {
        weight / denom
    }
}

/// Age-based forgetting — exponential decay over time.
/// `w · exp(-λ · age)`. SPEC §13 #4.
pub fn forgetting(weight: f32, age_secs: u64, lambda: f32) -> f32 {
    let lam = lambda.max(0.0);
    weight * (-lam * age_secs as f32).exp()
}

/// r_rec = |P_k ∩ 𝒟⁺| / |𝒟⁺|
pub fn recovery(retrieved: &[crate::DocId], ground_truth: &[crate::DocId]) -> f32 {
    if ground_truth.is_empty() {
        return 0.0;
    }
    let gt: ahash::AHashSet<_> = ground_truth.iter().collect();
    let hit = retrieved.iter().filter(|d| gt.contains(d)).count();
    hit as f32 / ground_truth.len() as f32
}

/// r_pre = |P_k ∩ 𝒟⁺| / |P_k|
pub fn precision(retrieved: &[crate::DocId], ground_truth: &[crate::DocId]) -> f32 {
    if retrieved.is_empty() {
        return 0.0;
    }
    let gt: ahash::AHashSet<_> = ground_truth.iter().collect();
    let hit = retrieved.iter().filter(|d| gt.contains(d)).count();
    hit as f32 / retrieved.len() as f32
}

/// ρ_rep(𝒢) = (|𝒯| − |uniq(𝒯)|) / |𝒯|
pub fn repetition_penalty(triples: &[(EntityId, SmolStr, EntityId)]) -> f32 {
    if triples.is_empty() {
        return 0.0;
    }
    let total = triples.len();
    let uniq: ahash::AHashSet<_> = triples.iter().collect();
    (total - uniq.len()) as f32 / total as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_zero_when_all_zero() {
        let r = WriterReward::default();
        assert_eq!(r.task(&TaskWeights::default()), 0.0);
    }

    #[test]
    fn task_equals_one_when_perfect() {
        let r = WriterReward {
            r_rec: 1.0,
            r_pre: 1.0,
            r_ded: 1.0,
            ..Default::default()
        };
        let v = r.task(&TaskWeights::default());
        assert!((v - 1.0).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn task_handles_zero_denominator() {
        let r = WriterReward {
            r_rec: 1.0,
            ..Default::default()
        };
        let w = TaskWeights {
            alpha: 0.0,
            beta: 0.0,
            gamma: 0.0,
        };
        assert_eq!(r.task(&w), 0.0);
    }

    #[test]
    fn trajectory_subtracts_repetition() {
        let r = WriterReward {
            r_rec: 1.0,
            r_pre: 1.0,
            r_ded: 1.0,
            rep_penalty: 0.5,
            fmt_bonus: 0.0,
            r_ans: 0.0,
        };
        let cfg = RewardCfg::default();
        let v = r.trajectory(&cfg);
        assert!((v - (1.0 - 0.2 * 0.5)).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn recovery_empty_gt_is_zero() {
        assert_eq!(recovery(&[1, 2], &[]), 0.0);
    }

    #[test]
    fn recovery_full_hit_is_one() {
        assert!((recovery(&[1, 2, 3], &[1, 2]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recovery_half_hit() {
        assert!((recovery(&[1], &[1, 2]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn precision_empty_retrieved_is_zero() {
        assert_eq!(precision(&[], &[1]), 0.0);
    }

    #[test]
    fn precision_all_relevant_is_one() {
        assert!((precision(&[1, 2], &[1, 2, 3]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn precision_half_relevant() {
        assert!((precision(&[1, 99], &[1, 2]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn habituation_zero_hits_is_identity() {
        assert!((habituation(1.0, 0, 0.5) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn habituation_attenuates_with_hits() {
        let a = habituation(1.0, 0, 0.5);
        let b = habituation(1.0, 5, 0.5);
        let c = habituation(1.0, 50, 0.5);
        assert!(a > b && b > c, "monotone decrease: {a} > {b} > {c}");
        assert!(c > 0.0);
    }

    #[test]
    fn habituation_negative_rate_clamps_to_zero() {
        // negative rate must not amplify the weight
        assert!((habituation(1.0, 5, -1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn forgetting_zero_age_is_identity() {
        assert!((forgetting(1.0, 0, 0.1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn forgetting_decays_over_time() {
        let a = forgetting(1.0, 10, 0.1);
        let b = forgetting(1.0, 100, 0.1);
        assert!(a > b);
        assert!(b > 0.0);
    }

    #[test]
    fn forgetting_zero_lambda_is_identity() {
        assert!((forgetting(1.0, 9999, 0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn repetition_penalty_empty_is_zero() {
        assert_eq!(repetition_penalty(&[]), 0.0);
    }

    #[test]
    fn repetition_penalty_all_unique_is_zero() {
        let v = vec![
            (1u64, SmolStr::new("r"), 2u64),
            (2u64, SmolStr::new("r"), 3u64),
        ];
        assert_eq!(repetition_penalty(&v), 0.0);
    }

    #[test]
    fn repetition_penalty_half_dup() {
        let t = (1u64, SmolStr::new("r"), 2u64);
        let v = vec![
            t.clone(),
            t.clone(),
            (3u64, SmolStr::new("r"), 4u64),
            (5u64, SmolStr::new("r"), 6u64),
        ];
        // 4 triples, 3 unique → 1/4
        let p = repetition_penalty(&v);
        assert!((p - 0.25).abs() < 1e-6, "got {p}");
    }
}
