# eval_v5/

> 525-doc / 80-query 規模測試集；v4 (200) → v5 (500) 的 2.5x scale up。

## Purpose
觀察 500 doc / ~1300 entity 規模下：
- v4 已發現的失敗模式是否惡化
- 新引入的 5xxx-9xxx topic cluster 是否引入新衝突
- HnswIndex narrow_k=256 在 1300 entity 時是否成為瓶頸

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 525 docs：v4 200 doc + 5xxx 體育 / 6xxx 地理 / 7xxx 商業 / 8xxx 物理 / 9xxx 文藝 + 1-4xxx 擴充 |
| `queries.json` | data | 80 query × 5 tier |
| `baselines.json` | data | live-measured baseline + 與 v4/v3 退化分析 |

## Invariants
- doc_id 四位數編碼：1xxx-9xxx 對應 9 個 cluster
- queries 的 `ground_truth` 只引用 docs.jsonl 中存在的 doc_id

## Tests
無自動化；端到端：`sage ingest-batch` → `sage eval`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 對照規模：v3 (100) / v4 (200) / **v5 (500)** / v6 (1000)

## Last Updated
2026-05-26 — 525-doc scale; baseline 寫入待 ingest 完成。
