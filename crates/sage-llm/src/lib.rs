//! LLM client trait + in-process mock backend + optional real backends.

#[cfg(feature = "anthropic")]
pub mod anthropic;

#[cfg(feature = "claude-cli")]
pub mod claude_cli;

pub mod mock;

use async_trait::async_trait;
use sage_core::Result;
use serde::{Deserialize, Serialize};

#[cfg(feature = "anthropic")]
pub use anthropic::{AnthropicLlm, RetryCfg};

#[cfg(feature = "claude-cli")]
pub use claude_cli::ClaudeCliLlm;

pub use mock::MockLlm;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(s: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: s.into(),
        }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: s.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse>;
    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool>;
}
