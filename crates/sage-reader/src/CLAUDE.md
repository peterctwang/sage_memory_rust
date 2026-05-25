# sage-reader/src/

> Reader 模組：planner / addressing / heuristic 三段管線。

## Purpose
把讀取流程切成可單測的純步驟：plan → score → softmax → aggregate。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export |
| `planner.rs` | module | `QueryPlanner` trait + `HeuristicPlanner`（token + stopword）|
| `addressing.rs` | module | `score_entry`, `softmax_entry`, `AddressingWeights` |
| `heuristic.rs` | module | `HeuristicReader<P>` — Reader trait 實作 |
| `gfm.rs` | module | `GfmLayer` + `GfmConfig` + `GfmGraphView` — 單層 forward (M3 spike) |

## Public Surface
見父層 CLAUDE.md。

## Invariants
- `planner.rs` 不引入 LLM 依賴；HeuristicPlanner 為純函式。
- `addressing.rs` 純 CPU、無 IO、可 `rayon` 化（M3 再做）。
- `heuristic.rs` 不持有圖快照，每次查詢都向 `EntityScan` 拿最新狀態。

## Tests
- `planner.rs#cfg(test)`：3。
- `addressing.rs#cfg(test)`：7。
- `heuristic.rs`：無單元（透過 integration test 覆蓋）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M2。
