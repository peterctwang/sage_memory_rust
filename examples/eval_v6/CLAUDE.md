# eval_v6/

> 1001-doc / 97-query 全規模測試集；終極壓力測試。

## Purpose
觀察 1000 doc / ~2500+ entity 規模下：
- 系統是否仍能保持基本功能
- Softmax 在 thousands of entities 下還能否區分
- HnswIndex 真正派上用場（線性掃描太慢）
- 跨 cluster 多 hop 是否完全失效

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 1001 docs：v5 525 + 10xxx 動物 / 11xxx 食物 / 12xxx 遊戲 / 13xxx 汽車 / 14xxx 電視 + 1-9xxx 擴充 |
| `queries.json` | data | 97 query × 5 tier |
| `baselines.json` | data | live-measured + 跨規模 8/30/100/200/500/1000 完整對照 |

## Invariants
- doc_id 四位/五位數編碼：1xxx-14xxx 對應 14 個 cluster
- queries 的 `ground_truth` 只引用 docs.jsonl 中存在的 doc_id

## Tests
無自動化；端到端：`sage ingest-batch` → `sage eval`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 對照規模：v3 (100) / v4 (200) / v5 (500) / **v6 (1001)**

## Last Updated
2026-05-26 — 1001-doc 終極規模；baseline 待 ingest 完。
