# eval_v2/

> 30-doc / 20-query 分層有效性測試集，含 measured baseline 與 regression 閾值。

## Purpose
給使用者一個**真實大小的測試集**判斷 SAGE v0.1.0 哪裡會壞：
- 比 `../eval_dataset/` (8 docs smoke set) 大，但仍能在 10 分鐘內跑完
- 4 層難度 (exact / multi-token / descriptive / cross-domain) 各 5 題
- 附 `baselines.json` 鎖定 regression 閾值

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 30 doc 跨 5 topic cluster (1xx / 2xx / 3xx / 4xx / 5xx)|
| `queries.json` | data | 20 query, 每筆有 `tier` + `kind` + `ground_truth` |
| `baselines.json` | data | 實機跑出來的整體 + per-tier 數字，含 regression 閾值 |
| `README.md` | doc | 跑法、結果解讀、不測試什麼 |

## Public Surface
資料檔。

## Invariants
- doc_id 採 1xx / 2xx / 3xx / 4xx / 5xx 分組編碼，read time 一眼看出 topic
- queries 的 `ground_truth` 只能引用 docs.jsonl 中存在的 doc_id
- `baselines.json.regression_thresholds` 是 floor，未來 PR 低於這值代表壞了

## Tests
無自動化（資料集）。
端到端流程：`sage ingest-batch` → `sage eval` → 比對 baselines.json。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- Smoke set: [`../eval_dataset/`](../eval_dataset/)
- CLI：[`../../crates/sage-cli/CLAUDE.md`](../../crates/sage-cli/CLAUDE.md)

## Last Updated
2026-05-26 — initial; baseline measured live via real claude binary。
