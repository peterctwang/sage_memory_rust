//! Embedder trait + helpers — SPEC §7.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;

#[async_trait]
pub trait Embedder: Send + Sync {
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Arc<[f32]>>>;
}

/// L2 cosine similarity of two equal-length vectors. Returns 0 if either norm is 0.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na.sqrt() * nb.sqrt()).max(1e-12);
    if denom < 1e-12 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = [1.0, 0.0];
        let b = [0.0, 1.0];
        assert!(cosine(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_identical_is_one() {
        let a = [1.0, 2.0, 3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_mismatched_length_is_zero() {
        let a = [1.0, 2.0];
        let b = [1.0, 2.0, 3.0];
        assert_eq!(cosine(&a, &b), 0.0);
    }

    #[test]
    fn cosine_zero_vector_is_zero() {
        let a = [0.0, 0.0];
        let b = [1.0, 1.0];
        assert_eq!(cosine(&a, &b), 0.0);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = [1.0, 0.0];
        let b = [-1.0, 0.0];
        assert!((cosine(&a, &b) + 1.0).abs() < 1e-6);
    }
}
