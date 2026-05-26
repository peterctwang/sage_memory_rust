# eval_dataset/

> 8 篇合成 doc + 8 條 ground-truth query；smoke-grade retrieval baseline。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 8 文件 — `{doc_id, text}` 一行一筆 |
| `queries.json` | data | 8 query + 單一正解 — `[{query, ground_truth: [doc_id]}]` |
| `README.md` | doc | 如何用 `sage ingest-batch` + `sage eval` 跑 |

## Public Surface
資料檔。

## Invariants
- `docs.jsonl` 與 `sage ingest-batch --jsonl` schema 對齊。
- `queries.json` 與 `sage eval` stdin schema 對齊。
- queries 的 `ground_truth` 必須只引用 docs.jsonl 中存在的 doc_id。

## Tests
無；資料集純人工驗證。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-26 — initial 8-doc synthetic set。
