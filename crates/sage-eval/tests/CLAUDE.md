# sage-eval/tests/

> 端到端整合測試 — ingest + reader + EvalRunner 一氣呵成。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `runner_it.rs` | test | 2 case：完美 recall 命名匹配、空樣本退化 |

## Public Surface
internal only。

## Invariants
- 只透過 `pub` API 進入；不窺探任何 crate 內部結構。

## Tests
本目錄即為測試入口。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M5-eval。
