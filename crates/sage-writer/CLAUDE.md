# sage-writer/

> LLM-driven memory writer：抽 triples、sanitize、落圖；對應論文 §4.1 / SPEC §4。

## Purpose
把文件轉為圖記憶。M1 採無訓練版本（policy = 純 LLM call），預留 trait 給 M4 GRPO 訓練器接入。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core, sage-llm, regex, ahash, serde |
| `src/` | dir | 模組 — 見 `src/CLAUDE.md` |
| `tests/` | dir | 整合測試 — 見 `tests/CLAUDE.md` |

## Public Surface
- `WriterPolicy` (trait, async)
- `LlmWriterPolicy<L: LlmClient>`
- `WriterAction`, `WriterState<'a>`, `EntityRef`, `RawTriple`
- `TripleSanitizer`, `SanitizerCfg`, `RejectReason`
- `apply_action(store, tenant, action) -> ApplyReport`

## Invariants
- 所有 LLM 輸出**必須**經 `TripleSanitizer::sanitize` 才能落圖（SPEC §C.5）。
- `LlmWriterPolicy` 把不合規 triple 靜默 drop 並 `tracing::debug` 記錄；不 panic、不擴散錯誤。
- `apply_action` 對同名 entity（case-insensitive）穩定產生同一 `EntityId`，跨呼叫 deterministic。
- 不依賴 `sage-reader`（避免循環依賴；SPEC §B.2）。

## Tests
- 單元：sanitizer (7) + llm_policy (3) + apply (3) = 13。
- 整合：`tests/writer_pipeline_it.rs` — MockLlm → policy → apply → MemGraphStore（3 case）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§4.1`](../../SPEC_SAGE_Rust.md), [`§C.5`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M1 initial: LlmWriterPolicy + sanitizer + apply。
