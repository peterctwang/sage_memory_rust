# sage-llm/src/

> LLM client 模組層。

## Purpose
按後端切模組；`lib.rs` 集中 trait 與 DTO，每個後端一個 module。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | `LlmClient` trait + `ChatRequest/Response/Message/Role` |
| `mock.rs` | module | `MockLlm` — `parking_lot::Mutex<VecDeque>` 腳本佇列 |
| `anthropic.rs` | module | `AnthropicLlm` + `RetryCfg` — reqwest + exp backoff (feature `anthropic`) |
| `claude_cli.rs` | module | `ClaudeCliLlm` — local `claude` binary subprocess (feature `claude-cli`); 模式參考 mission-framework Python harness |

## Public Surface
- `LlmClient`, `ChatRequest`, `ChatResponse`, `ChatMessage`, `Role`
- `MockLlm::{new, push, push_judge, completion_count}`

## Invariants
- `MockLlm` 內部 lock 從不跨 `.await` 持有。
- 真實 backend 進入時必須在 module 級別 feature-gate（M1+）。

## Tests
- `mock.rs#cfg(test)`：3 個（見父層 CLAUDE.md）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
