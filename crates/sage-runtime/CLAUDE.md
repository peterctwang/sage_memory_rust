# sage-runtime/

> SageEngine — 把 writer / reader / graph / llm 串成單一高層 API。

## Purpose
SPEC §6 的可用化入口；使用者唯一需要實例化的型別。M2.5 不含 `evolve()`，留 M4。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴所有 sage-* crate |
| `src/` | dir | engine 模組 — 見 `src/CLAUDE.md` |
| `tests/` | dir | 端到端整合測試 — 見 `tests/CLAUDE.md` |

## Public Surface
- `SageEngine<W, R, G, L>` — 泛型於 `WriterPolicy / Reader / ReaderGraph / LlmClient`。
- `SageEngine::{new, with_tenant, ingest_one, ingest, query, query_with_answer}`
- `IngestReport`, `AnswerBundle`

## Invariants
- 引擎內部 `tenant` 預設 `TenantId::DEFAULT`；`with_tenant` 設定後所有 ingest/query 皆繫於該租戶。
- `query_with_answer` 對 LLM 的 prompt 只含 doc ID 與問題；不洩漏圖內部結構。
- 不直接持有可變狀態；圖透過 `Arc<G>` 共享，可跨 task 安全使用。

## Tests
- 整合（4）：`tests/engine_it.rs` — ingest+query、query_with_answer、tenant 隔離、single doc。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§6 自演化`](../../SPEC_SAGE_Rust.md), [`§11 公開 API 範例`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M2.5 engine glue。
