//! Two-level fallback `LlmClient`: try primary, fall back on empty / error.
//!
//! Intended pairing: primary=MiniMax (cheap, occasionally returns empty
//! or unparseable JSON), fallback=Claude (expensive, reliable). On
//! ~93% of calls we pay the cheap price; the ~7% slop falls through.
//!
//! "Empty" is defined deliberately broadly: empty string, whitespace,
//! or a tiny payload (<8 chars after trim) that would never be valid
//! JSON / a YES-NO verdict. This catches the M2.5 failure mode where
//! the model exhausts its visible-token budget inside `<think>`.

use std::sync::Arc;

use async_trait::async_trait;
use sage_core::Result;

use crate::{ChatRequest, ChatResponse, LlmClient};

/// Threshold under which a primary response is considered "empty" and the
/// fallback is invoked. Calibrated so a single character or stray whitespace
/// triggers fallback, but a tiny valid payload like `{}` or `YES` does not.
const EMPTY_THRESHOLD_CHARS: usize = 2;

#[derive(Clone)]
pub struct FallbackLlm<P, F>
where
    P: LlmClient + ?Sized,
    F: LlmClient + ?Sized,
{
    primary: Arc<P>,
    fallback: Arc<F>,
    /// Cumulative count of fallback invocations. Read with `fallback_count()`.
    fallback_count: Arc<std::sync::atomic::AtomicU64>,
}

impl<P, F> std::fmt::Debug for FallbackLlm<P, F>
where
    P: LlmClient + ?Sized,
    F: LlmClient + ?Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackLlm")
            .field(
                "fallback_count",
                &self
                    .fallback_count
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl<P, F> FallbackLlm<P, F>
where
    P: LlmClient + ?Sized,
    F: LlmClient + ?Sized,
{
    pub fn new(primary: Arc<P>, fallback: Arc<F>) -> Self {
        Self {
            primary,
            fallback,
            fallback_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// How many times the fallback was invoked across the lifetime of this wrapper.
    pub fn fallback_count(&self) -> u64 {
        self.fallback_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn is_empty_payload(s: &str) -> bool {
    s.trim().chars().count() < EMPTY_THRESHOLD_CHARS
}

#[async_trait]
impl<P, F> LlmClient for FallbackLlm<P, F>
where
    P: LlmClient + ?Sized,
    F: LlmClient + ?Sized,
{
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        match self.primary.complete(req.clone()).await {
            Ok(r) if !is_empty_payload(&r.content) => Ok(r),
            Ok(_) => {
                tracing::warn!("primary returned empty payload — falling back");
                self.fallback_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.fallback.complete(req).await
            }
            Err(e) => {
                tracing::warn!(error = %e, "primary errored — falling back");
                self.fallback_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.fallback.complete(req).await
            }
        }
    }

    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool> {
        // Judge returns bool, no "empty" signal — only catch error path.
        match self.primary.judge(q, y, ev).await {
            Ok(b) => Ok(b),
            Err(e) => {
                tracing::warn!(error = %e, "primary judge errored — falling back");
                self.fallback_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.fallback.judge(q, y, ev).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatMessage, MockLlm};

    fn req(s: &str) -> ChatRequest {
        ChatRequest {
            messages: vec![ChatMessage::user(s)],
            temperature: 0.0,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn primary_success_skips_fallback() {
        let p = Arc::new(MockLlm::new());
        p.push("real answer with content");
        let f = Arc::new(MockLlm::new()); // empty queue; would panic if used
        let fb = FallbackLlm::new(p, f);
        let r = fb.complete(req("x")).await.unwrap();
        assert_eq!(r.content, "real answer with content");
        assert_eq!(fb.fallback_count(), 0);
    }

    #[tokio::test]
    async fn empty_primary_triggers_fallback() {
        let p = Arc::new(MockLlm::new());
        p.push(""); // empty → fallback
        let f = Arc::new(MockLlm::new());
        f.push("rescued");
        let fb = FallbackLlm::new(p, f);
        let r = fb.complete(req("x")).await.unwrap();
        assert_eq!(r.content, "rescued");
        assert_eq!(fb.fallback_count(), 1);
    }

    #[tokio::test]
    async fn primary_error_triggers_fallback() {
        let p = Arc::new(MockLlm::new()); // queue empty → returns SageError::Llm
        let f = Arc::new(MockLlm::new());
        f.push("rescued");
        let fb = FallbackLlm::new(p, f);
        let r = fb.complete(req("x")).await.unwrap();
        assert_eq!(r.content, "rescued");
        assert_eq!(fb.fallback_count(), 1);
    }

    #[tokio::test]
    async fn whitespace_only_counts_as_empty() {
        let p = Arc::new(MockLlm::new());
        p.push("   \n   ");
        let f = Arc::new(MockLlm::new());
        f.push("rescued");
        let fb = FallbackLlm::new(p, f);
        let r = fb.complete(req("x")).await.unwrap();
        assert_eq!(r.content, "rescued");
        assert_eq!(fb.fallback_count(), 1);
    }

    #[test]
    fn is_empty_threshold() {
        assert!(is_empty_payload(""));
        assert!(is_empty_payload(" "));
        assert!(is_empty_payload("\n"));
        assert!(is_empty_payload("a")); // 1 char — below threshold
        assert!(!is_empty_payload("{}"));
        assert!(!is_empty_payload("YES"));
    }
}
