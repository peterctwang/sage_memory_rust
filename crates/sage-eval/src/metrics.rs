//! Pure retrieval metrics over `DocId` lists.

use ahash::AHashSet;
use sage_core::DocId;

/// `Recall@k = |top_k ∩ gt| / |gt|`. Returns 0 if `gt` is empty.
pub fn recall_at_k(retrieved: &[DocId], gt: &[DocId], k: usize) -> f32 {
    if gt.is_empty() {
        return 0.0;
    }
    let top: AHashSet<&DocId> = retrieved.iter().take(k).collect();
    let gt_set: AHashSet<&DocId> = gt.iter().collect();
    let hit = gt_set.intersection(&top).count();
    hit as f32 / gt.len() as f32
}

/// `Precision@k = |top_k ∩ gt| / k`. Returns 0 if `k == 0` or `retrieved` is shorter.
pub fn precision_at_k(retrieved: &[DocId], gt: &[DocId], k: usize) -> f32 {
    if k == 0 {
        return 0.0;
    }
    let top: AHashSet<&DocId> = retrieved.iter().take(k).collect();
    let gt_set: AHashSet<&DocId> = gt.iter().collect();
    let hit = gt_set.intersection(&top).count();
    let denom = retrieved.len().min(k).max(1);
    hit as f32 / denom as f32
}

/// `F1@k = 2·P·R / (P + R)`; 0 when either is 0.
pub fn f1_at_k(retrieved: &[DocId], gt: &[DocId], k: usize) -> f32 {
    let p = precision_at_k(retrieved, gt, k);
    let r = recall_at_k(retrieved, gt, k);
    if p + r < 1e-12 {
        0.0
    } else {
        2.0 * p * r / (p + r)
    }
}

/// Mean reciprocal rank over a single sample: `1 / rank_of_first_hit`, or 0.
pub fn mrr(retrieved: &[DocId], gt: &[DocId]) -> f32 {
    let gt_set: AHashSet<&DocId> = gt.iter().collect();
    for (i, d) in retrieved.iter().enumerate() {
        if gt_set.contains(d) {
            return 1.0 / (i + 1) as f32;
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_full_hit() {
        assert!((recall_at_k(&[1, 2, 3], &[1, 2], 3) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recall_half_hit() {
        assert!((recall_at_k(&[1, 9], &[1, 2], 2) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn recall_empty_gt_is_zero() {
        assert_eq!(recall_at_k(&[1, 2], &[], 5), 0.0);
    }

    #[test]
    fn recall_respects_k_cutoff() {
        // first slot misses, second hits — at k=1 recall is 0
        assert_eq!(recall_at_k(&[9, 1], &[1], 1), 0.0);
        assert!((recall_at_k(&[9, 1], &[1], 2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn precision_full_relevant() {
        assert!((precision_at_k(&[1, 2], &[1, 2, 3], 2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn precision_zero_k_is_zero() {
        assert_eq!(precision_at_k(&[1], &[1], 0), 0.0);
    }

    #[test]
    fn f1_combines_p_and_r() {
        let f = f1_at_k(&[1, 9], &[1, 2], 2);
        // P = 1/2, R = 1/2, F1 = 0.5
        assert!((f - 0.5).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn f1_zero_when_both_zero() {
        assert_eq!(f1_at_k(&[9], &[1], 1), 0.0);
    }

    #[test]
    fn mrr_first_hit_is_one() {
        assert!((mrr(&[1, 2], &[1]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mrr_second_hit_is_half() {
        assert!((mrr(&[9, 1], &[1]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mrr_no_hit_is_zero() {
        assert_eq!(mrr(&[9, 8], &[1]), 0.0);
    }
}
