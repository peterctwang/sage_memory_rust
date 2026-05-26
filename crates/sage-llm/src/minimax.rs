//! MiniMax chat backend (feature `minimax`).
//!
//! OpenAI-compatible chat-completions client adapted from the **patent-ai**
//! project. Used to cut SAGE ingest cost ~20× vs Claude Opus on shallow
//! structured-extract workloads (writer triple extraction, query expansion,
//! YES/NO judging).
//!
//! Required env: `MINIMAX_API_KEY`.
//! Optional env:
//! - `MINIMAX_MODEL`   default `MiniMax-M2.5` (reasoning model)
//! - `MINIMAX_API_URL` default `https://api.minimax.io/v1/chat/completions`
//! - `MINIMAX_COOLDOWN_SEC` default `20` — global throttle after any 429
//!
//! ## Defense-in-depth (mirrors patent-ai/minimax.rs)
//! 1. Retry on 429 / 5xx with exponential backoff + jitter.
//! 2. Global cooldown after 429: every concurrent request sleeps until the
//!    deadline before sending. Solves per-minute RPM limits that per-call
//!    backoff alone can't.
//! 3. On 400 "context window exceeds limit (N)" → shrink the last user
//!    message to fit and retry (up to 3 rounds).
//! 4. Fast-fail on hard quota exhaustion (code 2056 / "usage limit exceeded")
//!    so a batch run aborts in seconds instead of hours.
//! 5. Strip `<think>...</think>` reasoning blocks M2.5 emits — they pollute
//!    JSON parsers downstream.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sage_core::{Result, SageError};
use serde::{Deserialize, Serialize};

use crate::{ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_URL: &str = "https://api.minimax.io/v1/chat/completions";
const DEFAULT_MODEL: &str = "MiniMax-M2.5";

/// Global cooldown shared by all `MinimaxLlm` instances in the process —
/// when any request gets 429 we set this and every sibling waits.
static MINIMAX_COOLDOWN_UNTIL: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn cooldown_secs() -> u64 {
    std::env::var("MINIMAX_COOLDOWN_SEC")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
}

fn set_cooldown() {
    let secs = cooldown_secs();
    if secs == 0 {
        return;
    }
    let until = Instant::now() + Duration::from_secs(secs);
    let mut g = MINIMAX_COOLDOWN_UNTIL.lock().unwrap();
    if g.map(|t| until > t).unwrap_or(true) {
        *g = Some(until);
        tracing::warn!("MiniMax global cooldown {secs}s (429 throttle)");
    }
}

async fn wait_cooldown() {
    let until = { *MINIMAX_COOLDOWN_UNTIL.lock().unwrap() };
    if let Some(t) = until {
        let now = Instant::now();
        if t > now {
            tokio::time::sleep(t - now).await;
        }
    }
}

/// Cheap non-crypto RNG in [0.0, 1.0); good enough for retry jitter.
fn rand_f64() -> f64 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    f64::from(n) / 1_000_000_000.0
}

fn parse_context_limit(body: &str) -> Option<usize> {
    let s = body.find("context window exceeds limit")?;
    let rest = &body[s..];
    let lp = rest.find('(')?;
    let rp = rest[lp..].find(')')?;
    rest[lp + 1..lp + rp].parse::<usize>().ok()
}

/// M2.5 sometimes emits `<think>...</think>` blocks even when not asked.
/// Strip them in one place so downstream JSON parsers don't choke.
fn strip_think(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end_rel) = rest[start..].find("</think>") {
            rest = &rest[start + end_rel + "</think>".len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[derive(Clone, Serialize)]
struct WireMessage {
    role: String,
    content: String,
}

#[derive(Clone, Serialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct WireResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: RespMessage,
}

#[derive(Deserialize)]
struct RespMessage {
    content: String,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

#[derive(Clone)]
pub struct MinimaxLlm {
    api_key: Arc<str>,
    model: Arc<str>,
    url: Arc<str>,
    client: reqwest::Client,
    timeout: Duration,
}

impl std::fmt::Debug for MinimaxLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinimaxLlm")
            .field("model", &self.model)
            .field("url", &self.url)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl MinimaxLlm {
    /// Build from `MINIMAX_API_KEY` env. Returns error if unset.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("MINIMAX_API_KEY")
            .map_err(|_| SageError::Llm("MINIMAX_API_KEY not set".into()))?;
        let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());
        let url = std::env::var("MINIMAX_API_URL").unwrap_or_else(|_| DEFAULT_URL.into());
        Ok(Self::new(api_key, model, url))
    }

    pub fn new(
        api_key: impl Into<Arc<str>>,
        model: impl Into<Arc<str>>,
        url: impl Into<Arc<str>>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self {
            api_key: api_key.into(),
            model: model.into(),
            url: url.into(),
            client,
            timeout: Duration::from_secs(120),
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Reasoning models (M2, M2.5) emit hidden `<think>...</think>` chains
    /// BEFORE the visible answer. A 512-token cap leaves zero budget for the
    /// actual JSON. Multiply caller's request by 8 for reasoning models so
    /// the visible output isn't truncated mid-string.
    fn effective_max_tokens(&self, requested: Option<u32>) -> u32 {
        let base = requested.unwrap_or(1024);
        let is_reasoning = {
            let m = self.model.to_ascii_lowercase();
            m.contains("m2") || m.contains("m2.5") || m.contains("reasoning")
        };
        if is_reasoning {
            // Floor of 4096 + 8× the requested visible budget, capped at 16384.
            base.saturating_mul(8).clamp(4096, 16384)
        } else {
            base
        }
    }

    fn to_wire(&self, req: &ChatRequest) -> WireRequest {
        let messages = req
            .messages
            .iter()
            .map(|m| WireMessage {
                role: match m.role {
                    Role::System => "system".into(),
                    Role::User => "user".into(),
                    Role::Assistant => "assistant".into(),
                },
                content: m.content.clone(),
            })
            .collect();
        WireRequest {
            model: self.model.to_string(),
            messages,
            max_tokens: self.effective_max_tokens(req.max_tokens),
            temperature: req.temperature,
        }
    }

    /// Shrink the LAST message's content to fit `n` tokens (≈ n*3 chars).
    fn shrink(req: &mut WireRequest, n: usize) {
        let budget = n.saturating_mul(3).max(500);
        if let Some(last) = req.messages.last_mut() {
            if last.content.chars().count() > budget {
                last.content = last.content.chars().take(budget).collect();
            }
        }
    }

    async fn post(&self, mut req: WireRequest) -> Result<String> {
        let mut shrink_rounds: u8 = 0;
        'outer: loop {
            let mut last_status: Option<reqwest::StatusCode> = None;
            let mut last_body = String::new();
            for (attempt, delay_ms) in [0u64, 2000, 6000, 15000, 30000].iter().enumerate() {
                if *delay_ms > 0 {
                    // SAFETY: delay_ms ≤ 30000 → fits in f64 exactly.
                    #[allow(clippy::cast_precision_loss)]
                    let base = *delay_ms as f64;
                    let jitter = base * 0.25 * (rand_f64() * 2.0 - 1.0);
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let actual = (base + jitter).max(0.0) as u64;
                    tokio::time::sleep(Duration::from_millis(actual)).await;
                }
                wait_cooldown().await;
                let resp = match self
                    .client
                    .post(self.url.as_ref())
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&req)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => return Err(SageError::Llm(format!("MiniMax send: {e}"))),
                };
                let status = resp.status();
                if status.is_success() {
                    let parsed = resp
                        .json::<WireResponse>()
                        .await
                        .map_err(|e| SageError::Llm(format!("MiniMax parse: {e}")))?;
                    let usage = parsed.usage.unwrap_or_default();
                    tracing::debug!(
                        input = usage.prompt_tokens,
                        output = usage.completion_tokens,
                        "MiniMax usage"
                    );
                    let raw = parsed
                        .choices
                        .into_iter()
                        .next()
                        .map(|c| c.message.content)
                        .unwrap_or_default();
                    return Ok(strip_think(&raw));
                }
                last_status = Some(status);
                last_body = resp.text().await.unwrap_or_default();
                let code = status.as_u16();
                // Hard quota — fail fast, don't burn 30min retrying.
                if code == 429
                    && (last_body.contains("usage limit exceeded") || last_body.contains("(2056)"))
                {
                    return Err(SageError::Llm(format!(
                        "MiniMax quota exhausted (2056): {last_body}"
                    )));
                }
                if code == 429 {
                    set_cooldown();
                }
                if code == 400 {
                    if let Some(n) = parse_context_limit(&last_body) {
                        if shrink_rounds < 3 {
                            shrink_rounds += 1;
                            tracing::warn!(
                                n,
                                shrink_rounds,
                                "MiniMax context overflow — shrinking"
                            );
                            Self::shrink(&mut req, n);
                            continue 'outer;
                        }
                    }
                    return Err(SageError::Llm(format!("MiniMax HTTP 400: {last_body}")));
                }
                let retryable = code == 429 || code == 502 || code == 503 || code == 504;
                if !retryable {
                    break;
                }
                tracing::warn!(attempt, %status, "retrying MiniMax");
            }
            return Err(SageError::Llm(format!(
                "MiniMax HTTP {} {}",
                last_status.map_or(0, |s| s.as_u16()),
                last_body
            )));
        }
    }
}

#[async_trait]
impl LlmClient for MinimaxLlm {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let wire = self.to_wire(&req);
        let content = self.post(wire).await?;
        Ok(ChatResponse { content })
    }

    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool> {
        let prompt = format!(
            "Question: {q}\nProposed answer: {y}\nEvidence:\n- {}\n\n\
             Reply with exactly 'YES' or 'NO' — does the evidence support the answer?",
            ev.join("\n- ")
        );
        let resp = self
            .complete(ChatRequest {
                messages: vec![crate::ChatMessage::user(prompt)],
                temperature: 0.0,
                max_tokens: Some(8),
            })
            .await?;
        Ok(resp.content.trim().to_uppercase().starts_with('Y'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_think_removes_blocks() {
        let s = "<think>internal</think>actual answer";
        assert_eq!(strip_think(s), "actual answer");
    }

    #[test]
    fn strip_think_handles_unterminated() {
        let s = "good<think>bad with no close";
        assert_eq!(strip_think(s), "good");
    }

    #[test]
    fn strip_think_passes_through_clean() {
        assert_eq!(strip_think("hello world"), "hello world");
    }

    #[test]
    fn parse_context_limit_extracts_n() {
        let body = r#"{"error":"context window exceeds limit (32768)"}"#;
        assert_eq!(parse_context_limit(body), Some(32768));
    }

    #[test]
    fn parse_context_limit_returns_none_on_other_errors() {
        assert!(parse_context_limit("some other 400").is_none());
    }

    #[test]
    fn shrink_truncates_last_message_to_budget() {
        let mut req = WireRequest {
            model: "m".into(),
            messages: vec![
                WireMessage {
                    role: "system".into(),
                    content: "keep".into(),
                },
                WireMessage {
                    role: "user".into(),
                    content: "x".repeat(100_000),
                },
            ],
            max_tokens: 100,
            temperature: 0.0,
        };
        MinimaxLlm::shrink(&mut req, 1000); // budget ≈ 3000 chars
        assert_eq!(req.messages[0].content, "keep");
        assert!(req.messages[1].content.chars().count() <= 3000);
    }

    #[test]
    fn reasoning_models_get_inflated_token_budget() {
        let m25 = MinimaxLlm::new("k", "MiniMax-M2.5", "u");
        assert!(m25.effective_max_tokens(Some(512)) >= 4096);
        let text01 = MinimaxLlm::new("k", "MiniMax-Text-01", "u");
        assert_eq!(text01.effective_max_tokens(Some(512)), 512);
    }

    #[test]
    fn to_wire_maps_roles_and_defaults_max_tokens() {
        let llm = MinimaxLlm::new("k", "m", "u");
        let req = ChatRequest {
            messages: vec![
                crate::ChatMessage::system("sys"),
                crate::ChatMessage::user("hi"),
            ],
            temperature: 0.3,
            max_tokens: None,
        };
        let w = llm.to_wire(&req);
        assert_eq!(w.messages[0].role, "system");
        assert_eq!(w.messages[1].role, "user");
        assert_eq!(w.max_tokens, 1024);
        assert!((w.temperature - 0.3).abs() < 1e-6);
    }
}
