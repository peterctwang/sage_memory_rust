# sage-reader/tests/

> 整合測試 — ingest（writer）→ query（reader）端到端走 MemGraphStore。

## Purpose
驗證 reader 不只是單元層通，整體 pipeline 能從文件抽 triple、落圖、查詢命中正確 doc。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `reader_pipeline_it.rs` | test | 4 case：命名 entity 命中、空圖、無關 query 數值穩定、跨租戶隔離 |

## Public Surface
internal only。

## Invariants
- 僅透過 `sage_core::*` / `sage_graph::*` / `sage_llm::*` / `sage_writer::*` / `sage_reader::*` 的 `pub` API。

## Tests
本目錄即為測試；`cargo test -p sage-reader --test reader_pipeline_it`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M2。
