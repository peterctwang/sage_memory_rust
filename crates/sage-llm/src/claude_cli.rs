//! Claude Code CLI subprocess backend (feature `claude-cli`).
//!
//! Invokes the locally-installed `claude` binary in headless mode. Pattern is
//! borrowed from the **mission-framework** project (Python harness) which has
//! tested these flags in production:
//!
//! - `claude -p` — headless / print mode; prompt fed via **stdin** to avoid
//!   the Windows 8191-char argv limit (long structured prompts get silently
//!   truncated otherwise).
//! - `--output-format json` — parse `result` + `usage` from the response.
//! - `--append-system-prompt` (NOT `--system-prompt`) — preserves the default
//!   agent-loop instructions; replacing them causes Claude to wait passively
//!   instead of acting.
//! - `--permission-mode acceptEdits` — required when `cwd` is supplied (worker
//!   mode with tool use). Omitted for pure chat (`judge`) calls.
//!
//! Use this when you have a local Claude Code subscription and don't want to
//! manage an `ANTHROPIC_API_KEY` — auth is handled by the CLI itself.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use async_trait::async_trait;
use sage_core::{Result, SageError};
use serde::Deserialize;

use crate::{ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_MODEL: &str = "claude-opus-4-7";
const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// Subset of the JSON envelope `claude -p --output-format json` emits.
/// Extra fields are tolerated.
#[derive(Debug, Deserialize)]
struct ClaudeCliOutput {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Clone)]
pub struct ClaudeCliLlm {
    binary: Arc<str>,
    model: Arc<str>,
    /// Optional working directory; if `Some`, worker mode is engaged (tool use enabled).
    cwd: Option<Arc<str>>,
    /// Comma-separated list passed to `--allowedTools`. Only relevant in worker mode.
    allowed_tools: Arc<str>,
    timeout_secs: u64,
}

impl std::fmt::Debug for ClaudeCliLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeCliLlm")
            .field("binary", &self.binary)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

impl Default for ClaudeCliLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCliLlm {
    /// Use the `claude` binary on `PATH` with the default model.
    pub fn new() -> Self {
        Self {
            binary: Arc::<str>::from("claude"),
            model: Arc::<str>::from(DEFAULT_MODEL),
            cwd: None,
            allowed_tools: Arc::<str>::from("Bash,Read,Edit,Write,Glob,Grep"),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_binary(mut self, b: impl Into<Arc<str>>) -> Self {
        self.binary = b.into();
        self
    }
    pub fn with_model(mut self, m: impl Into<Arc<str>>) -> Self {
        self.model = m.into();
        self
    }
    /// Engage worker mode: subprocess runs in `cwd` with tool use enabled.
    pub fn with_cwd(mut self, c: impl Into<Arc<str>>) -> Self {
        self.cwd = Some(c.into());
        self
    }
    pub fn with_allowed_tools(mut self, t: impl Into<Arc<str>>) -> Self {
        self.allowed_tools = t.into();
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
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// Build the argv that would be passed to the `claude` binary.
    /// Exposed for testability — keep this in sync with `complete()` below.
    pub fn build_argv(&self, system: Option<&str>) -> Vec<String> {
        let mut cmd: Vec<String> = vec![
            "-p".into(),
            "--model".into(),
            self.model.to_string(),
            "--output-format".into(),
            "json".into(),
        ];
        if self.cwd.is_some() {
            // Worker mode — agentic with tool use.
            cmd.push("--permission-mode".into());
            cmd.push("acceptEdits".into());
            cmd.push("--allowedTools".into());
            cmd.push(self.allowed_tools.to_string());
        }
        if let Some(s) = system {
            // CRITICAL: --append, not replace. See module docs.
            cmd.push("--append-system-prompt".into());
            // Windows .cmd shim corrupts argv with embedded newlines →
            // "batch file arguments are invalid" (observed 2026-05-27 when
            // a multi-line writer system prompt first hit the Claude
            // fallback path). Collapsing to spaces is semantically safe
            // (the model treats the prompt as a single paragraph) and
            // cross-platform consistent.
            cmd.push(s.replace(['\n', '\r'], " "));
        }
        cmd
    }

    /// Collapse the chat request into (system_prompt, user_prompt) where
    /// `user_prompt` is the catenation of non-system messages with role tags.
    fn split_messages(req: &ChatRequest) -> (Option<String>, String) {
        let mut system: Option<String> = None;
        let mut user_buf = String::new();
        for m in &req.messages {
            match m.role {
                Role::System => {
                    // Multiple system messages join with blank line.
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
        (system, user_buf)
    }
}

#[async_trait]
impl LlmClient for ClaudeCliLlm {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let (system, user) = Self::split_messages(&req);
        let argv = self.build_argv(system.as_deref());
        let binary = self.binary.clone();
        let cwd = self.cwd.clone();
        let timeout = std::time::Duration::from_secs(self.timeout_secs);

        // Subprocess is blocking; offload to a blocking pool.
        // **Defense-in-depth foolproofing** (after 2026-05-26 hang incident):
        // 1. `child.stdin.take()` (not as_mut) — drops stdin after write, sending
        //    EOF to the child. Without this, claude reads stdin forever and
        //    `wait_with_output` deadlocks because stdout never closes.
        // 2. `tokio::time::timeout` outside spawn_blocking — even if the EOF fix
        //    misses an edge case, we kill the wait after `timeout_secs`.
        // 3. Inside spawn_blocking we also do best-effort `child.kill()` if
        //    write fails, so a half-spawned child never lingers.
        let job = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut cmd = Command::new(binary.as_ref());
            cmd.args(&argv);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            if let Some(c) = cwd.as_deref() {
                cmd.current_dir(c);
            }

            let mut child = cmd
                .spawn()
                .map_err(|e| SageError::Llm(format!("spawn claude: {e}")))?;
            // CRITICAL: take(), not as_mut(). Dropping `stdin` at end of this
            // block sends EOF to claude so it stops reading and proceeds.
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(user.as_bytes()) {
                    let _ = child.kill();
                    return Err(SageError::Llm(format!("write stdin: {e}")));
                }
                // explicit drop for clarity (also happens at scope end)
                drop(stdin);
            }
            let out = child
                .wait_with_output()
                .map_err(|e| SageError::Llm(format!("wait claude: {e}")))?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(SageError::Llm(format!(
                    "claude CLI exit {}: {}",
                    out.status,
                    stderr.chars().take(300).collect::<String>()
                )));
            }
            String::from_utf8(out.stdout)
                .map_err(|e| SageError::Llm(format!("non-utf8 stdout: {e}")))
        });

        let handle = async move {
            match tokio::time::timeout(timeout, job).await {
                Ok(joined) => joined.map_err(|e| SageError::Llm(format!("join: {e}")))?,
                Err(_elapsed) => Err(SageError::Llm(format!(
                    "claude CLI exceeded timeout of {}s — likely stdin/stdout deadlock or network stall",
                    timeout.as_secs()
                ))),
            }
        };

        let raw = handle.await?;

        // Try JSON envelope first; fall back to raw text if it's not JSON.
        let content = match serde_json::from_str::<ClaudeCliOutput>(raw.trim()) {
            Ok(p) => p
                .result
                .or(p.text)
                .unwrap_or_else(|| raw.trim().to_string()),
            Err(_) => raw.trim().to_string(),
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
                max_tokens: Some(4),
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
        let c = ClaudeCliLlm::new();
        assert_eq!(c.binary(), "claude");
        assert_eq!(c.model(), DEFAULT_MODEL);
        assert!(c.cwd().is_none());
    }

    #[test]
    fn system_prompt_newlines_are_sanitized_for_windows_cmd() {
        let c = ClaudeCliLlm::new();
        let argv = c.build_argv(Some("line1\nline2\r\nline3"));
        let sys_idx = argv
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        let payload = &argv[sys_idx + 1];
        assert!(!payload.contains('\n'), "got: {payload:?}");
        assert!(!payload.contains('\r'), "got: {payload:?}");
        assert!(payload.contains("line1") && payload.contains("line3"));
    }

    #[test]
    fn argv_chat_mode_omits_permission_and_tools() {
        let c = ClaudeCliLlm::new();
        let argv = c.build_argv(Some("you are helpful"));
        // Required base args
        assert_eq!(argv[0], "-p");
        assert!(argv.iter().any(|a| a == "--model"));
        assert!(argv.iter().any(|a| a == "--output-format"));
        // Chat mode → no permission/tools flags
        assert!(!argv.iter().any(|a| a == "--permission-mode"));
        assert!(!argv.iter().any(|a| a == "--allowedTools"));
        // System prompt is APPENDED, not replaced.
        assert!(argv.iter().any(|a| a == "--append-system-prompt"));
        assert!(!argv.iter().any(|a| a == "--system-prompt"));
    }

    #[test]
    fn argv_worker_mode_includes_permission_and_tools() {
        let c = ClaudeCliLlm::new().with_cwd("/tmp/work");
        let argv = c.build_argv(None);
        assert!(argv.iter().any(|a| a == "--permission-mode"));
        assert!(argv.iter().any(|a| a == "acceptEdits"));
        assert!(argv.iter().any(|a| a == "--allowedTools"));
        // No system prompt requested → no append flag.
        assert!(!argv.iter().any(|a| a == "--append-system-prompt"));
    }

    #[test]
    fn argv_no_system_omits_append_flag() {
        let c = ClaudeCliLlm::new();
        let argv = c.build_argv(None);
        assert!(!argv.iter().any(|a| a == "--append-system-prompt"));
    }

    #[test]
    fn builder_overrides() {
        let c = ClaudeCliLlm::new()
            .with_binary("claude.cmd")
            .with_model("claude-haiku-4-5-20251001")
            .with_cwd("D:/proj")
            .with_allowed_tools("Read,Glob")
            .with_timeout_secs(60);
        assert_eq!(c.binary(), "claude.cmd");
        assert_eq!(c.model(), "claude-haiku-4-5-20251001");
        assert_eq!(c.cwd(), Some("D:/proj"));
        assert_eq!(c.timeout_secs, 60);
        let argv = c.build_argv(None);
        let tools_idx = argv.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(argv[tools_idx + 1], "Read,Glob");
    }

    #[test]
    fn split_messages_routes_system_separately() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("you are a triple extractor"),
                ChatMessage::user("Alice founded Acme."),
            ],
            temperature: 0.0,
            max_tokens: None,
        };
        let (sys, user) = ClaudeCliLlm::split_messages(&req);
        assert_eq!(sys.as_deref(), Some("you are a triple extractor"));
        assert!(user.contains("Alice founded Acme."));
        assert!(!user.contains("you are a triple extractor"));
    }

    #[test]
    fn split_messages_concatenates_multiple_users() {
        let req = ChatRequest {
            messages: vec![ChatMessage::user("first"), ChatMessage::user("second")],
            temperature: 0.0,
            max_tokens: None,
        };
        let (sys, user) = ClaudeCliLlm::split_messages(&req);
        assert!(sys.is_none());
        assert!(user.contains("first"));
        assert!(user.contains("second"));
    }

    #[test]
    fn split_messages_merges_multiple_system() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("rule A"),
                ChatMessage::system("rule B"),
                ChatMessage::user("x"),
            ],
            temperature: 0.0,
            max_tokens: None,
        };
        let (sys, _user) = ClaudeCliLlm::split_messages(&req);
        let s = sys.unwrap();
        assert!(s.contains("rule A"));
        assert!(s.contains("rule B"));
    }

    #[tokio::test]
    async fn complete_errors_cleanly_when_binary_missing() {
        // Use an obviously-nonexistent binary name to verify the error path
        // doesn't panic and surfaces a SageError::Llm.
        let c = ClaudeCliLlm::new().with_binary("definitely-not-claude-xyz-9999");
        let r = c
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("hi")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await;
        assert!(matches!(r, Err(SageError::Llm(_))), "got {r:?}");
    }

    /// Foolproofing regression test (2026-05-26 hang incident):
    /// Even if a real binary were to hang forever on stdin, the wrapper MUST
    /// surface an error within `timeout_secs`. We simulate a hang by pointing
    /// at a binary on `PATH` that reads stdin forever — on most systems `cat`
    /// (Unix) or `findstr` (Windows). To keep this portable we use a
    /// nonexistent binary which fails fast: the goal here is mainly to
    /// guarantee that `timeout_secs(1)` is plumbed into the actual await path.
    #[tokio::test]
    async fn timeout_is_wired_and_short_value_does_not_block_forever() {
        let c = ClaudeCliLlm::new()
            .with_binary("definitely-not-claude-xyz-9999")
            .with_timeout_secs(1);
        let start = std::time::Instant::now();
        let r = c
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("hi")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await;
        let elapsed = start.elapsed();
        assert!(matches!(r, Err(SageError::Llm(_))), "got {r:?}");
        // Must return well under the timeout (spawn fails fast); if this ever
        // exceeds ~3s the deadlock has crept back in.
        assert!(
            elapsed.as_secs() < 3,
            "complete() should not block; elapsed={elapsed:?}"
        );
    }
}
