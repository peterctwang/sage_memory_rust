# sage-eval/

> 檢索評估指標 + 驅動樣本跑分；Recall@k / Precision@k / F1@k / MRR。

## Purpose
給 reader（無論 heuristic 還是未來的 GFM）一套通用評估基準。M3+ 自演化時，writer 的 reward 也會依賴本 crate 提供的純函式。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core, async-trait, ahash, serde |
| `src/` | dir | metrics + runner — 見 `src/CLAUDE.md` |
| `tests/` | dir | 端到端整合 — 見 `tests/CLAUDE.md` |

## Public Surface
- 純函式：`recall_at_k`, `precision_at_k`, `f1_at_k`, `mrr`
- `EvalSample { query, ground_truth }`
- `EvalReport { samples, recall_at_k, precision_at_k, f1_at_k, mrr, k }`
- `EvalRunner<R: Reader>`：`new(reader, k)` / `with_tenant(t)` / `run(graph, samples)`

## Invariants
- 所有指標值域 `[0, 1]`。
- `gt` 為空 ⇒ recall = 0；`k = 0` ⇒ precision = 0。
- `EvalReport` 為平均值（macro-averaging across samples）。

## Tests
- 單元（11）：每個指標的邊界 + 半命中 + cutoff 行為。
- 整合（2）：透過 LlmWriterPolicy + HeuristicReader 驗證完美 recall + 空樣本退化。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§4.1 reward`, `§5 reader`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M5-eval 初版。
