//! OpenAI Codex CLI subprocess backend (feature `codex-cli`).
//!
//! Invocation pattern lifted verbatim from the **Mission Framework**
//! (Python harness) which has been tested in production against ChatGPT-
//! subscription accounts. The flags below all matter:
//!
//! - `exec --json` — non-interactive, JSONL event stream to stdout
//! - `--skip-git-repo-check` — don't refuse to run outside a git repo
//! - `--ignore-user-config` — don't load `~/.codex/config.toml` (deterministic)
//! - `--dangerously-bypass-approvals-and-sandbox` — same intent as Claude's
//!   `--permission-mode acceptEdits`; required for headless runs
//! - `-o <file>` — final assistant message goes to this file (NOT stdout)
//! - `-m <model>` — OPTIONAL. ChatGPT-subscription accounts reject explicit
//!   `-m`; leave the model unset (`""`) to inherit the account default
//! - stdin = the prompt (so we dodge Windows' 8191-char argv limit)
//!
//! Quota signals come back in the JSONL stream / stderr as one of:
//! "usage limit", "rate limit exceeded", "weekly limit", "exceeded your",
//! "quota exceeded" — we fail fast on those so a batch run aborts in
//! seconds instead of burning 30 min retrying every call.
//!
//! Required env: a working `codex` binary on PATH or pointed at by
//! `SAGE_CODEX_BIN`. On Windows that's the `.cmd` shim from
//! `npm i -g @openai/codex`.
//!
//! Optional env:
//!   - `SAGE_CODEX_MODEL` — leave unset to use the account default.
//!
//! Auth is handled by the codex CLI itself.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sage_core::{Result, SageError};

use crate::{ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// Substrings that, when seen in stderr / JSONL output, mean "this call
/// will never succeed without the user topping up / waiting for reset" —
/// we surface a hard error so the batch loop can bail out.
const QUOTA_MARKERS: &[&str] = &[
    "usage limit",
    "rate limit exceeded",
    "weekly limit",
    "exceeded your",
    "quota exceeded",
];

#[derive(Clone)]
pub struct CodexCliLlm {
    binary: Arc<str>,
    /// Empty string means "let codex pick the account default" — required
    /// for ChatGPT-subscription accounts which reject explicit `-m`.
    model: Arc<str>,
    cwd: Option<Arc<str>>,
    timeout_secs: u64,
}

impl std::fmt::Debug for CodexCliLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexCliLlm")
            .field("binary", &self.binary)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("timeout_secs", &self.timeout_secs)
            .finish_non_exhaustive()
    }
}

impl Default for CodexCliLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexCliLlm {
    /// Construct with the `codex` binary on PATH and no explicit model.
    pub fn new() -> Self {
        Self {
            binary: Arc::<str>::from("codex"),
            model: Arc::<str>::from(""),
            cwd: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Build from env: honors `SAGE_CODEX_BIN` and `SAGE_CODEX_MODEL`.
    /// Leave `SAGE_CODEX_MODEL` unset (or empty) for ChatGPT-subscription
    /// accounts — explicit `-m` will be rejected.
    pub fn from_env() -> Self {
        let mut c = Self::new();
        if let Ok(b) = std::env::var("SAGE_CODEX_BIN") {
            c = c.with_binary(b);
        }
        if let Ok(m) = std::env::var("SAGE_CODEX_MODEL") {
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

    /// Build the argv passed to `codex`. The `-o <out_path>` flag is
    /// supplied at call time so the helper here only assembles the
    /// fixed prefix. Exposed for tests.
    pub fn build_argv_prefix(&self) -> Vec<String> {
        let mut cmd: Vec<String> = vec![
            "exec".into(),
            "--json".into(),
            "--skip-git-repo-check".into(),
            "--ignore-user-config".into(),
            "--dangerously-bypass-approvals-and-sandbox".into(),
        ];
        if !self.model.is_empty() {
            cmd.push("-m".into());
            cmd.push(self.model.to_string());
        }
        if let Some(c) = self.cwd.as_deref() {
            cmd.push("-C".into());
            cmd.push(c.to_string());
            cmd.push("-s".into());
            cmd.push("workspace-write".into());
        }
        cmd
    }

    /// Collapse system+user into a single text payload codex can read on
    /// stdin. Mission Framework uses `[SYSTEM] ... [USER] ...` markers;
    /// we keep the same convention so prompts written for that harness
    /// remain re-usable.
    fn build_prompt(req: &ChatRequest) -> String {
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
            Some(s) if !s.trim().is_empty() => format!("[SYSTEM]\n{s}\n\n[USER]\n{user_buf}"),
            _ => user_buf,
        }
    }
}

fn check_quota_signal(blob: &str) -> Option<&'static str> {
    let lo = blob.to_lowercase();
    QUOTA_MARKERS.iter().find(|m| lo.contains(*m)).copied()
}

/// Pick a unique temp path for the codex `-o` final-message file.
/// We don't add the `tempfile` crate to keep deps minimal — the codex CLI
/// itself is the only consumer of this path and we always delete after.
fn temp_out_path() -> PathBuf {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = std::process::id();
    let mut p = std::env::temp_dir();
    p.push(format!("sage-codex-{pid}-{ns}.txt"));
    p
}

#[async_trait]
impl LlmClient for CodexCliLlm {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let prompt = Self::build_prompt(&req);
        let mut argv = self.build_argv_prefix();
        let out_path = temp_out_path();
        argv.push("-o".into());
        argv.push(out_path.display().to_string());
        argv.push("-".into()); // read prompt from stdin

        let binary = self.binary.clone();
        let cwd = self.cwd.clone();
        let timeout = std::time::Duration::from_secs(self.timeout_secs);
        let out_path_for_job = out_path.clone();

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
                .map_err(|e| SageError::Llm(format!("spawn codex: {e}")))?;
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(prompt.as_bytes()) {
                    let _ = child.kill();
                    return Err(SageError::Llm(format!("write codex stdin: {e}")));
                }
                drop(stdin);
            }
            let out = child
                .wait_with_output()
                .map_err(|e| SageError::Llm(format!("wait codex: {e}")))?;

            let jsonl = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Combined quota check across both streams (the marker can land
            // in either depending on codex version).
            if let Some(marker) = check_quota_signal(&format!("{jsonl}\n{stderr}")) {
                return Err(SageError::Llm(format!(
                    "codex quota exhausted (marker: {marker})"
                )));
            }

            // Final message lives in the -o file. Read it (if present) and
            // fall back to stdout content otherwise.
            let text = match std::fs::read_to_string(&out_path_for_job) {
                Ok(s) => s.trim().to_string(),
                Err(_) => String::new(),
            };
            if !out.status.success() && text.is_empty() {
                return Err(SageError::Llm(format!(
                    "codex CLI exit {}: {}",
                    out.status,
                    stderr.chars().take(300).collect::<String>()
                )));
            }
            if text.is_empty() {
                Ok(jsonl.trim().to_string())
            } else {
                Ok(text)
            }
        });

        let res = match tokio::time::timeout(timeout, job).await {
            Ok(joined) => joined.map_err(|e| SageError::Llm(format!("join: {e}"))),
            Err(_) => Err(SageError::Llm(format!(
                "codex CLI exceeded {}s timeout — likely stdin/stdout deadlock",
                timeout.as_secs()
            ))),
        };
        // Always cleanup the temp -o file before returning.
        let _ = std::fs::remove_file(&out_path);
        let content = res??;
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
    fn defaults_omit_explicit_model() {
        let c = CodexCliLlm::new();
        assert_eq!(c.binary(), "codex");
        assert_eq!(c.model(), "");
        let argv = c.build_argv_prefix();
        assert!(!argv.iter().any(|a| a == "-m"));
    }

    #[test]
    fn argv_prefix_has_all_required_flags() {
        let c = CodexCliLlm::new();
        let argv = c.build_argv_prefix();
        for flag in [
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--ignore-user-config",
            "--dangerously-bypass-approvals-and-sandbox",
        ] {
            assert!(argv.iter().any(|a| a == flag), "missing {flag}: {argv:?}");
        }
    }

    #[test]
    fn explicit_model_adds_dash_m() {
        let c = CodexCliLlm::new().with_model("gpt-5-codex");
        let argv = c.build_argv_prefix();
        let i = argv.iter().position(|a| a == "-m").unwrap();
        assert_eq!(argv[i + 1], "gpt-5-codex");
    }

    #[test]
    fn worker_mode_adds_cwd_and_workspace_write() {
        let c = CodexCliLlm::new().with_cwd("/tmp/work");
        let argv = c.build_argv_prefix();
        assert!(argv.iter().any(|a| a == "-C"));
        assert!(argv.iter().any(|a| a == "/tmp/work"));
        let s_idx = argv.iter().position(|a| a == "-s").unwrap();
        assert_eq!(argv[s_idx + 1], "workspace-write");
    }

    #[test]
    fn build_prompt_inlines_system_with_markers() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("you are a triple extractor"),
                ChatMessage::user("Alice founded Acme."),
            ],
            temperature: 0.0,
            max_tokens: None,
        };
        let p = CodexCliLlm::build_prompt(&req);
        assert!(p.contains("[SYSTEM]"));
        assert!(p.contains("[USER]"));
        assert!(p.contains("triple extractor"));
        assert!(p.contains("Alice founded Acme."));
    }

    #[test]
    fn build_prompt_without_system_omits_markers() {
        let req = ChatRequest {
            messages: vec![ChatMessage::user("just user")],
            temperature: 0.0,
            max_tokens: None,
        };
        let p = CodexCliLlm::build_prompt(&req);
        assert!(!p.contains("[SYSTEM]"));
        assert!(p.contains("just user"));
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

    #[test]
    fn temp_out_path_is_unique() {
        let a = temp_out_path();
        // tiny sleep avoids identical nanos on faster machines.
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let b = temp_out_path();
        assert_ne!(a, b, "temp paths must differ across calls");
    }

    #[tokio::test]
    async fn missing_binary_errors_cleanly() {
        let c = CodexCliLlm::new().with_binary("definitely-not-codex-xyz-9999");
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
