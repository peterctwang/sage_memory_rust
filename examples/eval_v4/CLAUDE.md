# eval_v4/

> 200-doc / 60-query 大規模測試集；用來找 100→200 doc 時新冒出的失敗模式。

## Purpose
比 `eval_v3/` (100 doc) 大 2x；測試在「中等規模」(200 docs / ~900 entities) 下：
- 哪些原本能跑的 query 開始壞
- HnswIndex narrow_k=256 是否仍夠用
- 多 hop / paraphrase 是否進一步退化
- entity 命名衝突（同名不同 entity）是否引發誤配

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 200 docs：v3 100 doc 全集 + 100 new（tech CEO/AI / 醫學 / 古代史 / 現代文藝）|
| `queries.json` | data | 60 query × 5 tier（每 tier 12 個）|
| `baselines.json` | data | 待 live ingest 完成後寫入；含 vs eval_v3 退化分析 |

## Tier rubric（同 v3）
| tier | kind | example |
|---|---|---|
| 1 | exact 單一 entity | "Who created Linux" |
| 2 | multi-token 整句 | "Who created the C programming language at Bell Labs" |
| 3 | descriptive 無 surface anchor | "Who proposed black holes emit radiation" |
| 4 | paraphrase / cross-domain | "Architect of perestroika in the Soviet Union" |
| 5 | multi-hop（≥ 2 truth docs）| "Two CEOs of Microsoft" |

## Invariants
- doc_id 四位數編碼；1xxx tech / 2xxx science / 3xxx history / 4xxx culture
- queries 的 `ground_truth` 只引用 docs.jsonl 中存在的 doc_id
- tier 5 必有 ≥ 2 doc 作為 ground truth

## Tests
無自動化（資料集）；端到端：`sage ingest-batch` → `sage eval`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 對照規模：[`../eval_dataset/`](../eval_dataset/) (8) / [`../eval_v2/`](../eval_v2/) (30) / [`../eval_v3/`](../eval_v3/) (100)

## Last Updated
2026-05-26 — 200-doc scale-out 初版；baseline 待 ingest 完。
