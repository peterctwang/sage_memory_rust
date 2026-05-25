# sage-llm/

> LLM client trait 與測試用 mock；真實 backend (Anthropic / OpenAI) 留待 M1。

## Purpose
為 SAGE 提供與具體 LLM 供應商解耦的呼叫介面。三套後端：
- `MockLlm`（預設）— 確定性 FIFO 腳本，CI/單元測試用，零外部依賴。
- `AnthropicLlm`（feature `anthropic`）— Anthropic Messages API，reqwest + 指數退避。
- `ClaudeCliLlm`（feature `claude-cli`）— 本地 `claude` 二進位 subprocess，借用 user 既有 Claude Code 訂閱，無需 API key。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core, async-trait, parking_lot, serde |
| `src/` | dir | 模組 — 見 `src/CLAUDE.md` |

## Public Surface
- `LlmClient` (trait)
- `ChatRequest`, `ChatResponse`, `ChatMessage`, `Role`
- `MockLlm` — FIFO scripted responses + judge verdicts

## Invariants
- `MockLlm` 為確定性：腳本耗盡 ⇒ 回 `SageError::Llm`，不會 panic。
- `LlmClient` 必為 `Send + Sync`，所有方法 `async`。

## Tests
- 單元（3）：scripted response、exhaustion、judge queue。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§7 後端 traits`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M0 trait + mock。
