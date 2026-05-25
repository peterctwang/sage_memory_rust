//! `DeterministicEmbedder` — hashed bag-of-words. Pure function, no I/O, ideal
//! for tests and as a sanity baseline. Replaceable by real models in M3+.

use std::sync::Arc;

use async_trait::async_trait;
use sage_core::{Embedder, Result};

#[derive(Debug, Clone)]
pub struct DeterministicEmbedder {
    dim: usize,
    seed: u64,
}

impl DeterministicEmbedder {
    /// # Panics
    /// Panics if `dim < 4`.
    pub fn new(dim: usize) -> Self {
        assert!(dim >= 4, "dim must be ≥ 4");
        Self {
            dim,
            seed: 0x00C0_FFEE,
        }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let bh = ahash::RandomState::with_seeds(self.seed, self.seed.wrapping_add(1), 0, 0);
        for token in text.split(|c: char| !c.is_alphanumeric()) {
            if token.is_empty() {
                continue;
            }
            let lowered = token.to_lowercase();
            let h = std::hash::BuildHasher::hash_one(&bh, &lowered);
            let idx = (h as usize) % self.dim;
            let sign = if (h >> 1) & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += sign;
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

#[async_trait]
impl Embedder for DeterministicEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Arc<[f32]>>> {
        Ok(texts
            .iter()
            .map(|t| Arc::<[f32]>::from(self.embed_one(t)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_core::cosine;

    #[tokio::test]
    async fn embed_returns_correct_dim() {
        let e = DeterministicEmbedder::new(64);
        let v = e.embed(&["hello world"]).await.unwrap();
        assert_eq!(v[0].len(), 64);
    }

    #[tokio::test]
    async fn embed_is_deterministic() {
        let e = DeterministicEmbedder::new(32);
        let a = e.embed(&["alice founded acme"]).await.unwrap();
        let b = e.embed(&["alice founded acme"]).await.unwrap();
        assert_eq!(a[0].as_ref(), b[0].as_ref());
    }

    #[tokio::test]
    async fn embed_similar_texts_higher_cos_than_random() {
        let e = DeterministicEmbedder::new(256);
        let v = e
            .embed(&[
                "alice founded acme corporation",
                "alice started acme corporation",
                "weather report sunny tomorrow",
            ])
            .await
            .unwrap();
        let same_topic = cosine(&v[0], &v[1]);
        let off_topic = cosine(&v[0], &v[2]);
        assert!(
            same_topic > off_topic,
            "same_topic={same_topic} should beat off_topic={off_topic}"
        );
    }

    #[tokio::test]
    async fn empty_text_yields_zero_vector() {
        let e = DeterministicEmbedder::new(16);
        let v = e.embed(&[""]).await.unwrap();
        assert!(v[0].iter().all(|x| *x == 0.0));
    }

    #[tokio::test]
    async fn unit_norm_when_nonempty() {
        let e = DeterministicEmbedder::new(32);
        let v = e.embed(&["any nontrivial text"]).await.unwrap();
        let norm: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm={norm}");
    }
}
