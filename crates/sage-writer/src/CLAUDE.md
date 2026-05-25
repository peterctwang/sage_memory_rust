# sage-writer/src/

> Writer 模組層：拆 action / policy / llm / sanitizer / apply。

## Purpose
把寫入流程切成可單獨測試的純步驟：產生 action → sanitize → 落圖。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export |
| `action.rs` | module | `WriterAction`, `WriterState<'a>`, `EntityRef`, `RawTriple` |
| `policy.rs` | module | `WriterPolicy` trait（async）|
| `llm_policy.rs` | module | `LlmWriterPolicy<L>` — prompt 構造 + JSON 解析 + sanitize |
| `sanitizer.rs` | module | `TripleSanitizer` + `SanitizerCfg` + `RejectReason` |
| `apply.rs` | module | `apply_action` + `name_to_id` + `ApplyReport` |

## Public Surface
見父層 CLAUDE.md。

## Invariants
- `lib.rs` re-export 是穩定 API 表面。
- `name_to_id` 為純函式且 case-insensitive；同字串永遠回同 ID（單一 process 內）。
- `parse_etype` 未識別字串落入 `EntityType::Custom`，不丟錯。

## Tests
- `sanitizer.rs#cfg(test)`：7。
- `llm_policy.rs#cfg(test)`：3。
- `apply.rs#cfg(test)`：3（name_to_id 性質測試 + ApplyReport 預設）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M1。
