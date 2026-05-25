//! GRPO numerical primitives — paper §4.1 / SPEC §4.3.
//!
//! These are pure-function math building blocks. They do not perform autodiff
//! and do not update any model — they're meant to be called by a future
//! tensor-backend trainer (candle / burn) to compute the loss surface.

/// Group-relative advantage: `A_i = (r_i − mean) / (std + ε)`.
///
/// Empty input returns empty. Single-element returns `[0.0]`. Zero-variance
/// groups return `[0.0; n]` to avoid divide-by-zero.
pub fn group_relative_advantage(rewards: &[f32]) -> Vec<f32> {
    if rewards.is_empty() {
        return Vec::new();
    }
    let n = rewards.len() as f32;
    let mean: f32 = rewards.iter().sum::<f32>() / n;
    let variance: f32 = rewards.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / n;
    let std = variance.sqrt();
    if std < 1e-9 {
        return vec![0.0; rewards.len()];
    }
    rewards.iter().map(|r| (r - mean) / std).collect()
}

/// Clipped probability ratio `clip(exp(new − old), 1−ε, 1+ε)` — used in PPO/GRPO.
pub fn clipped_ratio(new_logp: f32, old_logp: f32, epsilon: f32) -> f32 {
    let r = (new_logp - old_logp).exp();
    r.clamp(1.0 - epsilon, 1.0 + epsilon)
}

/// PPO/GRPO surrogate **objective** — we maximize this (returns positive value).
/// Note: the *loss* is `-objective`; trainers using gradient ascent flip the sign.
///
/// `objective = mean_i [ min(ratio_i · A_i, clip(ratio_i) · A_i) ]`
/// where `ratio_i = exp(new_logp_i − old_logp_i)`.
///
/// Returns 0.0 for empty input. Errors silently to 0.0 on length mismatch
/// rather than panicking (caller is responsible for shape correctness).
pub fn grpo_objective(
    advantages: &[f32],
    new_logps: &[f32],
    old_logps: &[f32],
    epsilon: f32,
) -> f32 {
    if advantages.is_empty()
        || advantages.len() != new_logps.len()
        || advantages.len() != old_logps.len()
    {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for ((a, n), o) in advantages
        .iter()
        .zip(new_logps.iter())
        .zip(old_logps.iter())
    {
        let r = (n - o).exp();
        let r_clip = r.clamp(1.0 - epsilon, 1.0 + epsilon);
        sum += (r * a).min(r_clip * a);
    }
    sum / advantages.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advantage_zero_mean() {
        let a = group_relative_advantage(&[1.0, 2.0, 3.0]);
        let mean: f32 = a.iter().sum::<f32>() / a.len() as f32;
        assert!(mean.abs() < 1e-5);
    }

    #[test]
    fn advantage_unit_variance() {
        let a = group_relative_advantage(&[1.0, 2.0, 3.0, 4.0]);
        let mean: f32 = a.iter().sum::<f32>() / a.len() as f32;
        let var: f32 = a.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / a.len() as f32;
        assert!((var - 1.0).abs() < 1e-4, "var={var}");
    }

    #[test]
    fn advantage_zero_variance_returns_zeros() {
        let a = group_relative_advantage(&[5.0, 5.0, 5.0]);
        assert_eq!(a, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn advantage_empty_returns_empty() {
        assert!(group_relative_advantage(&[]).is_empty());
    }

    #[test]
    fn clipped_ratio_within_bounds_unchanged() {
        // ratio = exp(0.1 - 0.0) ≈ 1.105, within [0.8, 1.2]
        let r = clipped_ratio(0.1, 0.0, 0.2);
        assert!((r - 1.105_170_9).abs() < 1e-5);
    }

    #[test]
    fn clipped_ratio_clamps_upper() {
        // ratio = exp(2.0) ≈ 7.4, clamped to 1.2
        assert!((clipped_ratio(2.0, 0.0, 0.2) - 1.2).abs() < 1e-6);
    }

    #[test]
    fn clipped_ratio_clamps_lower() {
        // ratio = exp(-2.0) ≈ 0.135, clamped to 0.8
        assert!((clipped_ratio(-2.0, 0.0, 0.2) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn grpo_objective_zero_advantage_zero_objective() {
        let v = grpo_objective(&[0.0, 0.0], &[0.1, 0.2], &[0.0, 0.0], 0.2);
        assert!(v.abs() < 1e-6);
    }

    #[test]
    fn grpo_objective_positive_advantage_with_same_logp_gives_advantage() {
        // ratio = 1, so objective = mean(A)
        let v = grpo_objective(&[1.0, 1.0], &[0.0, 0.0], &[0.0, 0.0], 0.2);
        assert!((v - 1.0).abs() < 1e-6);
    }

    #[test]
    fn grpo_objective_mismatched_lengths_return_zero() {
        let v = grpo_objective(&[1.0], &[0.1, 0.2], &[0.0], 0.2);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn grpo_objective_clip_protects_against_runaway_ratio() {
        // Large new_logp would give ratio ≫ 1+ε; objective uses min, so clipped value is taken.
        let unclipped = (5.0f32).exp() * 1.0; // huge
        let v = grpo_objective(&[1.0], &[5.0], &[0.0], 0.2);
        let expected = (1.0f32 + 0.2) * 1.0; // 1.2
        assert!(v <= unclipped);
        assert!((v - expected).abs() < 1e-6);
    }
}
