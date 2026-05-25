//! Deterministic in-process LLM for tests. Scripts are matched by message-prefix.

use std::collections::VecDeque;

use async_trait::async_trait;
use parking_lot::Mutex;
use sage_core::{Result, SageError};

use crate::{ChatRequest, ChatResponse, LlmClient};

#[derive(Debug, Default)]
pub struct MockLlm {
    /// Pre-loaded responses returned FIFO.
    scripted: Mutex<VecDeque<String>>,
    /// Pre-loaded judge verdicts returned FIFO.
    judges: Mutex<VecDeque<bool>>,
    /// Count of `complete` calls.
    completions: Mutex<usize>,
}

impl MockLlm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, response: impl Into<String>) -> &Self {
        self.scripted.lock().push_back(response.into());
        self
    }

    pub fn push_judge(&self, verdict: bool) -> &Self {
        self.judges.lock().push_back(verdict);
        self
    }

    pub fn completion_count(&self) -> usize {
        *self.completions.lock()
    }
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, _req: ChatRequest) -> Result<ChatResponse> {
        *self.completions.lock() += 1;
        let next = self
            .scripted
            .lock()
            .pop_front()
            .ok_or_else(|| SageError::Llm("MockLlm script exhausted".into()))?;
        Ok(ChatResponse { content: next })
    }

    async fn judge(&self, _q: &str, _y: &str, _ev: &[String]) -> Result<bool> {
        self.judges
            .lock()
            .pop_front()
            .ok_or_else(|| SageError::Llm("MockLlm judge script exhausted".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatMessage, ChatRequest};

    fn req(prompt: &str) -> ChatRequest {
        ChatRequest {
            messages: vec![ChatMessage::user(prompt)],
            temperature: 0.0,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn returns_scripted_response() {
        let m = MockLlm::new();
        m.push("hello");
        let r = m.complete(req("anything")).await.unwrap();
        assert_eq!(r.content, "hello");
        assert_eq!(m.completion_count(), 1);
    }

    #[tokio::test]
    async fn errors_when_exhausted() {
        let m = MockLlm::new();
        assert!(m.complete(req("x")).await.is_err());
    }

    #[tokio::test]
    async fn judge_uses_queue() {
        let m = MockLlm::new();
        m.push_judge(true).push_judge(false);
        assert!(m.judge("q", "y", &[]).await.unwrap());
        assert!(!m.judge("q", "y", &[]).await.unwrap());
    }
}
