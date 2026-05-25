//! Anthropic Messages API client.
//!
//! Feature-gated behind `anthropic`. Implements exponential-backoff retry on
//! transient (5xx, 429, network) failures. Real-network testing is the
//! caller's responsibility — unit tests here cover construction and the retry
//! policy logic in isolation.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sage_core::{Result, SageError};
use serde::{Deserialize, Serialize};

use crate::{ChatMessage, ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_API_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-opus-4-7";

#[derive(Clone, Debug)]
pub struct RetryCfg {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryCfg {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 250,
            max_delay_ms: 8_000,
        }
    }
}

#[derive(Clone)]
pub struct AnthropicLlm {
    api_key: Arc<str>,
    model: Arc<str>,
    base_url: Arc<str>,
    version: Arc<str>,
    client: reqwest::Client,
    retry: RetryCfg,
}

impl std::fmt::Debug for AnthropicLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicLlm")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl AnthropicLlm {
    pub fn new(api_key: impl Into<Arc<str>>) -> Self {
        Self {
            api_key: api_key.into(),
            model: Arc::<str>::from(DEFAULT_MODEL),
            base_url: Arc::<str>::from(DEFAULT_BASE_URL),
            version: Arc::<str>::from(DEFAULT_API_VERSION),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            retry: RetryCfg::default(),
        }
    }

    pub fn with_model(mut self, model: impl Into<Arc<str>>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, url: impl Into<Arc<str>>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_retry(mut self, retry: RetryCfg) -> Self {
        self.retry = retry;
        self
    }

    pub fn retry(&self) -> &RetryCfg {
        &self.retry
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[derive(Serialize)]
struct AnthropicReq<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    system: Option<&'a str>,
    messages: Vec<AnthropicMsg<'a>>,
}

#[derive(Serialize)]
struct AnthropicMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResp {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

fn build_request<'a>(model: &'a str, req: &'a ChatRequest) -> AnthropicReq<'a> {
    // Collapse system messages into the dedicated `system` field.
    let mut system: Option<&str> = None;
    let mut messages: Vec<AnthropicMsg> = Vec::with_capacity(req.messages.len());
    for m in &req.messages {
        match m.role {
            Role::System => {
                system = Some(m.content.as_str());
            }
            Role::User => messages.push(AnthropicMsg {
                role: "user",
                content: &m.content,
            }),
            Role::Assistant => messages.push(AnthropicMsg {
                role: "assistant",
                content: &m.content,
            }),
        }
    }
    AnthropicReq {
        model,
        max_tokens: req.max_tokens.unwrap_or(1024),
        temperature: req.temperature,
        system,
        messages,
    }
}

fn should_retry(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

fn backoff_delay(attempt: u32, cfg: &RetryCfg) -> Duration {
    let exp = cfg.base_delay_ms.saturating_mul(1u64 << attempt.min(10));
    Duration::from_millis(exp.min(cfg.max_delay_ms))
}

#[async_trait]
impl LlmClient for AnthropicLlm {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let body = build_request(&self.model, &req);
        let mut last_err = String::from("no attempts made");
        for attempt in 0..self.retry.max_attempts {
            let resp = self
                .client
                .post(self.base_url.as_ref())
                .header("x-api-key", self.api_key.as_ref())
                .header("anthropic-version", self.version.as_ref())
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status.is_success() {
                        let parsed: AnthropicResp = r
                            .json()
                            .await
                            .map_err(|e| SageError::Llm(format!("decode: {e}")))?;
                        let text = parsed
                            .content
                            .into_iter()
                            .find_map(|c| match c {
                                AnthropicContent::Text { text } => Some(text),
                                AnthropicContent::Other => None,
                            })
                            .unwrap_or_default();
                        return Ok(ChatResponse { content: text });
                    }
                    last_err = format!("HTTP {status}");
                    if !should_retry(status) {
                        return Err(SageError::Llm(last_err));
                    }
                    tracing::warn!(?status, attempt, "Anthropic retrying");
                }
                Err(e) => {
                    last_err = format!("network: {e}");
                    tracing::warn!(err = %e, attempt, "Anthropic transport error, retrying");
                }
            }
            tokio::time::sleep(backoff_delay(attempt, &self.retry)).await;
        }
        Err(SageError::Llm(format!("exhausted retries: {last_err}")))
    }

    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool> {
        let prompt = format!(
            "Question: {q}\nProposed answer: {y}\nEvidence:\n- {}\n\n\
             Reply with exactly 'YES' or 'NO' — does the evidence support the answer?",
            ev.join("\n- ")
        );
        let resp = self
            .complete(ChatRequest {
                messages: vec![ChatMessage::user(prompt)],
                temperature: 0.0,
                max_tokens: Some(4),
            })
            .await?;
        Ok(resp.content.trim().to_uppercase().starts_with('Y'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_defaults() {
        let c = AnthropicLlm::new("sk-test");
        assert_eq!(c.model(), DEFAULT_MODEL);
        assert!(c.base_url().contains("anthropic.com"));
        assert_eq!(c.retry().max_attempts, 5);
    }

    #[test]
    fn builder_overrides() {
        let c = AnthropicLlm::new("k")
            .with_model("claude-haiku-4-5-20251001")
            .with_base_url("https://mock.local/v1/messages")
            .with_retry(RetryCfg {
                max_attempts: 1,
                base_delay_ms: 10,
                max_delay_ms: 10,
            });
        assert_eq!(c.model(), "claude-haiku-4-5-20251001");
        assert_eq!(c.base_url(), "https://mock.local/v1/messages");
        assert_eq!(c.retry().max_attempts, 1);
    }

    #[test]
    fn backoff_caps_at_max() {
        let cfg = RetryCfg {
            max_attempts: 10,
            base_delay_ms: 1000,
            max_delay_ms: 5000,
        };
        assert_eq!(backoff_delay(0, &cfg).as_millis(), 1000);
        assert_eq!(backoff_delay(1, &cfg).as_millis(), 2000);
        assert_eq!(backoff_delay(2, &cfg).as_millis(), 4000);
        assert_eq!(backoff_delay(3, &cfg).as_millis(), 5000); // capped
        assert_eq!(backoff_delay(8, &cfg).as_millis(), 5000); // still capped
    }

    #[test]
    fn should_retry_logic() {
        assert!(should_retry(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(should_retry(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(should_retry(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(!should_retry(reqwest::StatusCode::BAD_REQUEST));
        assert!(!should_retry(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!should_retry(reqwest::StatusCode::OK));
    }

    #[test]
    fn build_request_separates_system_message() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("you are helpful"),
                ChatMessage::user("hi"),
            ],
            temperature: 0.0,
            max_tokens: Some(64),
        };
        let body = build_request("test-model", &req);
        assert_eq!(body.system, Some("you are helpful"));
        assert_eq!(body.messages.len(), 1);
        assert_eq!(body.messages[0].role, "user");
    }

    #[tokio::test]
    async fn rejects_unreachable_endpoint_after_retries() {
        // 127.0.0.1:1 is reserved and unreachable; tight retry config makes this fast.
        let c = AnthropicLlm::new("sk-test")
            .with_base_url("http://127.0.0.1:1/v1/messages")
            .with_retry(RetryCfg {
                max_attempts: 2,
                base_delay_ms: 1,
                max_delay_ms: 2,
            });
        let r = c
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("ping")],
                temperature: 0.0,
                max_tokens: Some(8),
            })
            .await;
        assert!(
            matches!(r, Err(SageError::Llm(_))),
            "expected Llm error, got {r:?}"
        );
    }
}
