//! Google Gemini CLI subprocess backend (feature `gemini-cli`).
//!
//! Invocation pattern verbatim from the **Mission Framework** Python
//! harness (verified against
//! https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/headless.md).
//!
//! Key constraints — learned from broken older invocations:
//! - `--yolo` is deprecated; use `--approval-mode yolo`.
//! - `--approval-mode plan` is broken in headless mode (issue #24814);
//!   never use it from this wrapper.
//! - **There is NO `--system-prompt` / `--append-system-prompt` flag**.
//!   The system role must be embedded directly inside the user prompt.
//! - Prompt via stdin is required on Windows to dodge the 8191-char
//!   argv limit; `-p ""` forces headless even with stdin attached.
//! - Several env vars defend against first-run wizards and auto-update
//!   prompts that otherwise hang the CLI silently.
//!
//! Required env: a working `gemini` binary on PATH or pointed at by
//! `SAGE_GEMINI_BIN`. On Windows that's the `.cmd` shim from
//! `npm i -g @google/gemini-cli`.
//!
//! Optional env:
//! - `SAGE_GEMINI_MODEL` (default `gemini-2.5-pro`)
//!
//! Auth is handled by the Gemini CLI's OAuth flow — no API key in SAGE.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use async_trait::async_trait;
use sage_core::{Result, SageError};
use serde::Deserialize;

use crate::{ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_MODEL: &str = "gemini-2.5-pro";
const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// Quota signals seen in stderr (production-observed by Mission Framework).
const QUOTA_MARKERS: &[&str] = &[
    "quota exceeded",
    "resource_exhausted",
    "rate limit",
    "usage limit",
    "daily limit",
];

#[derive(Clone)]
pub struct GeminiCliLlm {
    binary: Arc<str>,
    model: Arc<str>,
    cwd: Option<Arc<str>>,
    timeout_secs: u64,
}

impl std::fmt::Debug for GeminiCliLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiCliLlm")
            .field("binary", &self.binary)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

impl Default for GeminiCliLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiCliLlm {
    pub fn new() -> Self {
        Self {
            binary: Arc::<str>::from("gemini"),
            model: Arc::<str>::from(DEFAULT_MODEL),
            cwd: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Build from env: honors `SAGE_GEMINI_BIN` and `SAGE_GEMINI_MODEL`.
    pub fn from_env() -> Self {
        let mut c = Self::new();
        if let Ok(b) = std::env::var("SAGE_GEMINI_BIN") {
            c = c.with_binary(b);
        }
        if let Ok(m) = std::env::var("SAGE_GEMINI_MODEL") {
            if !m.trim().is_empty() {
                c = c.with_model(m);
            }
        }
        c
    }

    pub fn with_binary(mut self, b: impl Into<Arc<str>>) -> Self {
        self.binary = b.into();
        self
    }
    pub fn with_model(mut self, m: impl Into<Arc<str>>) -> Self {
        self.model = m.into();
        self
    }
    pub fn with_cwd(mut self, c: impl Into<Arc<str>>) -> Self {
        self.cwd = Some(c.into());
        self
    }
    pub fn with_timeout_secs(mut self, s: u64) -> Self {
        self.timeout_secs = s.max(1);
        self
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }
    pub fn model(&self) -> &str {
        &self.model
    }

    /// `gemini -m <model> -o json --approval-mode <mode> --skip-trust -p ""`
    /// `-p ""` forces headless even with stdin piped; absent it the CLI
    /// may try to attach to a TTY. Exposed for tests.
    pub fn build_argv(&self) -> Vec<String> {
        // cwd present = Worker (needs tool use) → yolo; else read-only `default`.
        let approval = if self.cwd.is_some() {
            "yolo"
        } else {
            "default"
        };
        vec![
            "-m".into(),
            self.model.to_string(),
            "-o".into(),
            "json".into(),
            "--approval-mode".into(),
            approval.into(),
            "--skip-trust".into(),
            "-p".into(),
            String::new(), // empty arg forces headless even with piped stdin
        ]
    }

    /// Defensive env vars that prevent the CLI hanging on TUIs / auto-updates.
    /// Returned as `Vec<(K, V)>` so the caller can apply via `Command::env_clear()`
    /// + `envs()` or `env(k, v)` per key.
    pub fn build_env_overrides() -> Vec<(&'static str, &'static str)> {
        vec![
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
            ("GEMINI_CLI_DISABLE_TELEMETRY", "1"),
            ("GEMINI_CLI_DISABLE_AUTO_UPDATE", "1"),
        ]
    }

    /// Gemini has no system-prompt flag — we inline a directive + the
    /// caller's system prompt before the user content. Mirrors Mission
    /// Framework's `_DIRECTIVE` but trimmed to SAGE's single-shot needs.
    fn build_prompt(req: &ChatRequest) -> String {
        const DIRECTIVE: &str = "INSTRUCTIONS — read carefully and follow EXACTLY: \
            (1) This is a single-shot request, not a conversation. \
            (2) Respond with what's asked; do not write a self-introduction. \
            (3) Do not ask clarifying questions. \
            (4) When asked for JSON, output ONLY valid JSON with no markdown fences or prose.";
        let mut system: Option<String> = None;
        let mut user_buf = String::new();
        for m in &req.messages {
            match m.role {
                Role::System => {
                    system = Some(match system.take() {
                        Some(prev) => format!("{prev}\n\n{}", m.content),
                        None => m.content.clone(),
                    });
                }
                Role::User => {
                    if !user_buf.is_empty() {
                        user_buf.push_str("\n\n");
                    }
                    user_buf.push_str(&m.content);
                }
                Role::Assistant => {
                    user_buf.push_str("\n\n[assistant]: ");
                    user_buf.push_str(&m.content);
                }
            }
        }
        match system {
            Some(s) if !s.trim().is_empty() => {
                format!("{DIRECTIVE}\n\n=== ROLE & TASK ===\n{s}\n\n=== REQUEST ===\n{user_buf}")
            }
            _ => format!("{DIRECTIVE}\n\n=== REQUEST ===\n{user_buf}"),
        }
    }
}

fn check_quota_signal(blob: &str) -> Option<&'static str> {
    let lo = blob.to_lowercase();
    QUOTA_MARKERS.iter().find(|m| lo.contains(*m)).copied()
}

#[derive(Deserialize)]
struct WireEnvelope {
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    output: Option<String>,
    // We don't parse `stats` here — usage tokens aren't surfaced via the
    // `LlmClient` trait yet. Add later if/when needed.
}

/// Peel a single ```...``` (or ```json ... ```) fence pair if it brackets
/// the entire trimmed payload — Gemini wraps JSON in fences despite the
/// directive forbidding it. Mirrors MiniMax's strip_markdown_fence so
/// the writer's JSON parser sees the raw object.
fn strip_markdown_fence(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let after_open = match t.find('\n') {
        Some(i) => &t[i + 1..],
        None => return t.to_string(),
    };
    let body = after_open.trim_end();
    let body = body.strip_suffix("```").unwrap_or(body);
    body.trim().to_string()
}

/// Parse the gemini `-o json` envelope. Returns the response text or an
/// empty string when the envelope shape is foreign — caller can decide
/// whether to fall back.
fn parse_envelope(stdout: &str) -> String {
    let s = stdout.trim();
    if s.is_empty() {
        return String::new();
    }
    if let Ok(env) = serde_json::from_str::<WireEnvelope>(s) {
        if let Some(t) = env.response.or(env.text).or(env.output) {
            return strip_markdown_fence(&t);
        }
    }
    // Stream-NDJSON fallback: scan for the last response/text/output field.
    let mut last = String::new();
    for line in s.lines() {
        if let Ok(env) = serde_json::from_str::<WireEnvelope>(line) {
            if let Some(t) = env.response.or(env.text).or(env.output) {
                if !t.is_empty() {
                    last = t;
                }
            }
        }
    }
    strip_markdown_fence(&last)
}

#[async_trait]
impl LlmClient for GeminiCliLlm {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let prompt = Self::build_prompt(&req);
        let argv = self.build_argv();
        let binary = self.binary.clone();
        let cwd = self.cwd.clone();
        let timeout = std::time::Duration::from_secs(self.timeout_secs);

        let job = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut cmd = Command::new(binary.as_ref());
            cmd.args(&argv);
            for (k, v) in GeminiCliLlm::build_env_overrides() {
                cmd.env(k, v);
            }
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            if let Some(c) = cwd.as_deref() {
                cmd.current_dir(c);
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| SageError::Llm(format!("spawn gemini: {e}")))?;
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(prompt.as_bytes()) {
                    let _ = child.kill();
                    return Err(SageError::Llm(format!("write gemini stdin: {e}")));
                }
                drop(stdin);
            }
            let out = child
                .wait_with_output()
                .map_err(|e| SageError::Llm(format!("wait gemini: {e}")))?;
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

            if !out.status.success() {
                // Map quota markers BEFORE generic exit-code error so the
                // batch loop can abort fast instead of retrying.
                if let Some(marker) = check_quota_signal(&stderr) {
                    return Err(SageError::Llm(format!(
                        "gemini quota exhausted (marker: {marker})"
                    )));
                }
                let tail: String = stderr.chars().rev().take(400).collect();
                let tail: String = tail.chars().rev().collect();
                return Err(SageError::Llm(format!(
                    "gemini CLI exit {}: {}",
                    out.status, tail
                )));
            }
            let text = parse_envelope(&stdout);
            if text.is_empty() {
                Ok(stdout.trim().to_string())
            } else {
                Ok(text)
            }
        });

        let content = match tokio::time::timeout(timeout, job).await {
            Ok(joined) => joined.map_err(|e| SageError::Llm(format!("join: {e}")))??,
            Err(_) => {
                return Err(SageError::Llm(format!(
                    "gemini CLI exceeded {}s timeout — likely stdin/stdout deadlock",
                    timeout.as_secs()
                )));
            }
        };
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
    use crate::ChatMessage;

    #[test]
    fn defaults() {
        let c = GeminiCliLlm::new();
        assert_eq!(c.binary(), "gemini");
        assert_eq!(c.model(), DEFAULT_MODEL);
    }

    #[test]
    fn argv_has_required_flags_and_empty_p() {
        let c = GeminiCliLlm::new();
        let argv = c.build_argv();
        assert!(argv.iter().any(|a| a == "-m"));
        let o_idx = argv.iter().position(|a| a == "-o").unwrap();
        assert_eq!(argv[o_idx + 1], "json");
        let m_idx = argv.iter().position(|a| a == "--approval-mode").unwrap();
        assert_eq!(argv[m_idx + 1], "default"); // chat mode → read-only
        assert!(argv.iter().any(|a| a == "--skip-trust"));
        let p_idx = argv.iter().position(|a| a == "-p").unwrap();
        assert_eq!(argv[p_idx + 1], ""); // force-headless empty arg
    }

    #[test]
    fn worker_mode_switches_to_yolo() {
        let c = GeminiCliLlm::new().with_cwd("/tmp/work");
        let argv = c.build_argv();
        let m_idx = argv.iter().position(|a| a == "--approval-mode").unwrap();
        assert_eq!(argv[m_idx + 1], "yolo");
    }

    #[test]
    fn env_overrides_contain_anti_hang_vars() {
        let env = GeminiCliLlm::build_env_overrides();
        for key in [
            "NO_COLOR",
            "TERM",
            "GEMINI_CLI_DISABLE_TELEMETRY",
            "GEMINI_CLI_DISABLE_AUTO_UPDATE",
        ] {
            assert!(env.iter().any(|(k, _)| *k == key), "missing env {key}");
        }
    }

    #[test]
    fn build_prompt_inlines_system_with_role_markers() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("you are a triple extractor"),
                ChatMessage::user("Alice founded Acme."),
            ],
            temperature: 0.0,
            max_tokens: None,
        };
        let p = GeminiCliLlm::build_prompt(&req);
        assert!(p.contains("INSTRUCTIONS"));
        assert!(p.contains("=== ROLE & TASK ==="));
        assert!(p.contains("=== REQUEST ==="));
        assert!(p.contains("triple extractor"));
        assert!(p.contains("Alice founded Acme."));
    }

    #[test]
    fn build_prompt_without_system_still_has_directive() {
        let req = ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            temperature: 0.0,
            max_tokens: None,
        };
        let p = GeminiCliLlm::build_prompt(&req);
        assert!(p.contains("INSTRUCTIONS"));
        assert!(p.contains("=== REQUEST ==="));
        assert!(!p.contains("=== ROLE & TASK ==="));
    }

    #[test]
    fn strip_markdown_fence_peels_json_block() {
        assert_eq!(
            strip_markdown_fence("```json\n{\"triples\":[]}\n```"),
            "{\"triples\":[]}"
        );
    }

    #[test]
    fn strip_markdown_fence_peels_bare_block() {
        assert_eq!(strip_markdown_fence("```\n{\"x\":1}\n```"), "{\"x\":1}");
    }

    #[test]
    fn strip_markdown_fence_passes_through_when_no_fence() {
        assert_eq!(strip_markdown_fence("{\"x\":1}"), "{\"x\":1}");
    }

    #[test]
    fn parse_envelope_strips_fenced_response() {
        let raw = r#"{"response":"```json\n{\"triples\":[]}\n```"}"#;
        assert_eq!(parse_envelope(raw), "{\"triples\":[]}");
    }

    #[test]
    fn parse_envelope_new_schema() {
        let raw = r#"{"response":"answer 42","stats":{"models":{"gemini-2.5-pro":{"tokens":{"prompt":100}}}}}"#;
        assert_eq!(parse_envelope(raw), "answer 42");
    }

    #[test]
    fn parse_envelope_text_field() {
        let raw = r#"{"text":"alt schema"}"#;
        assert_eq!(parse_envelope(raw), "alt schema");
    }

    #[test]
    fn parse_envelope_ndjson_fallback() {
        let raw = "{\"x\":1}\n{\"response\":\"streamed\"}\n";
        assert_eq!(parse_envelope(raw), "streamed");
    }

    #[test]
    fn parse_envelope_empty_returns_empty() {
        assert!(parse_envelope("").is_empty());
        assert!(parse_envelope("not json at all").is_empty());
    }

    #[test]
    fn quota_markers_detected() {
        for marker in QUOTA_MARKERS {
            let body = format!("noise {marker} more noise");
            assert!(
                check_quota_signal(&body).is_some(),
                "marker {marker} not detected"
            );
        }
        assert!(check_quota_signal("nothing suspicious").is_none());
    }

    #[tokio::test]
    async fn missing_binary_errors_cleanly() {
        let c = GeminiCliLlm::new().with_binary("definitely-not-gemini-xyz-9999");
        let r = c
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("hi")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await;
        assert!(matches!(r, Err(SageError::Llm(_))), "got {r:?}");
    }
}
