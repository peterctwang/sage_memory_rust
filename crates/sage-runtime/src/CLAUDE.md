# sage-runtime/src/

> Engine 模組層；單一 `engine.rs` 承載所有黏合邏輯。

## Purpose
保持 runtime 表面最小：只有 `SageEngine` 與兩個 report 型別；複雜性留給下游 crate。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export |
| `engine.rs` | module | `SageEngine<W, R, G, L>` + `IngestReport` + `AnswerBundle` |

## Public Surface
見父層 CLAUDE.md。

## Invariants
- `engine.rs` 不引入 candle / 神經網路依賴；保留為純編排層。
- Debug 實作用 `finish_non_exhaustive`，避免 LLM/Writer 等私有欄位逸出。

## Tests
無單元；行為由整合測試覆蓋。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M2.5。
