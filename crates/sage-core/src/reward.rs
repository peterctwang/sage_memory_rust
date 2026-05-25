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
