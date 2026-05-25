# sage-eval/src/

> 指標 + 驅動實作。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export |
| `metrics.rs` | module | 純指標：`recall_at_k`, `precision_at_k`, `f1_at_k`, `mrr` |
| `runner.rs` | module | `EvalRunner<R>` + `EvalSample` + `EvalReport` |

## Public Surface
見父層 CLAUDE.md。

## Invariants
- `metrics.rs` 全為純函式（無 IO、無分配大暫存）。
- `runner.rs` 使用 macro-averaging；不依賴具體 reader 實作。

## Tests
- `metrics.rs#cfg(test)`：11 個邊界與半命中測試。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M5-eval。
