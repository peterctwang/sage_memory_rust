# eval_v3/

> 100-doc / 40-query 廣度測試集，含 multi-hop 與 paraphrase 題型。

## Purpose
比 `eval_v2/` (30 doc / 20 query) **大 3x**、覆蓋題型更廣，用來判斷 SAGE 在「比較像真實使用」的規模下會壞在哪。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 100 docs across 4 topic clusters (1xxx tech / 2xxx science / 3xxx history / 4xxx culture)，多句長度混合 |
| `queries.json` | data | 40 query × 5 tier；tier 5 為 multi-hop（ground_truth ≥ 2 docs）|
| `baselines.json` | data | live-measured baseline + regression 閾值（首次填充由本 ingest 任務產生）|

## Tier rubric
| tier | kind | example |
|---|---|---|
| 1 | exact 單一 entity | "Who created Linux" |
| 2 | multi-token 整句 | "Who designed Java at Sun Microsystems" |
| 3 | descriptive 無 surface anchor | "Who proposed black holes emit radiation" |
| 4 | paraphrase / cross-domain | "Liberator of South Africa from apartheid rule" |
| 5 | **multi-hop** | "Two Nobel Peace Prize winners who led liberation movements" |

## Invariants
- doc_id 用 4 digit 編碼：1xxx / 2xxx / 3xxx / 4xxx 對應 cluster
- queries 的 `ground_truth` 必須只引用 docs.jsonl 中存在的 doc_id
- multi-hop tier 5 的 ground_truth 必為 ≥ 2 doc

## Tests
無自動化（資料集）；端到端走 `sage ingest-batch` → `sage eval`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 比較規模：[`../eval_dataset/`](../eval_dataset/) (8 doc smoke) / [`../eval_v2/`](../eval_v2/) (30 doc, 4 tier)

## Last Updated
2026-05-26 — initial 100-doc broad eval set。
