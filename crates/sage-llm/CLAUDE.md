# sage-llm/

> LLM client trait 與測試用 mock；真實 backend (Anthropic / OpenAI) 留待 M1。

## Purpose
為 SAGE 提供與具體 LLM 供應商解耦的呼叫介面，配套一個確定性 mock，讓 writer/reader 訓練與整合測試不需網路。

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
