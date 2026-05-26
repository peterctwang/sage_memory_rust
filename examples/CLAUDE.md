# examples/

> 對外可運行的端到端示例與資料集；CI 不跑，使用者手動觸發。

## Purpose
讓使用者拿到一個可立即驗證的 baseline：固定 corpus + 固定 query + 已知 ground truth → 跑出 Recall@k / MRR 數字確認 pipeline 沒壞。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `eval_dataset/` | dir | 8-doc synthetic eval set + queries — 見 `eval_dataset/README.md` |

## Public Surface
資料檔，無程式碼介面。

## Invariants
- `docs.jsonl` schema 與 `sage ingest-batch` 期待 schema 一致。
- `queries.json` schema 與 `sage eval` 期待 schema 一致。
- 兩者 doc_id 必須對齊（queries 的 ground_truth 引用 docs 的 doc_id）。

## Tests
無自動化（資料集，非程式碼）。
端到端流程驗證透過 `sage ingest-batch` → `sage eval` 手動跑。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- CLI：[`crates/sage-cli/CLAUDE.md`](../crates/sage-cli/CLAUDE.md)
- Eval lib：[`crates/sage-eval/CLAUDE.md`](../crates/sage-eval/CLAUDE.md)

## Last Updated
2026-05-26 — synthetic 8-doc eval dataset initial。
