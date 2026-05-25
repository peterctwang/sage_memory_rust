# sage-writer/tests/

> 整合測試 — 把 LlmWriterPolicy + apply_action 串到真的 MemGraphStore。

## Purpose
證明 writer 端到端流程：MockLlm 餵 JSON → policy 解析 sanitize → apply 落圖 → 圖中可見預期 entities/edges。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `writer_pipeline_it.rs` | test | 3 case：兩 triples 落圖、跨文件 entity reuse、bad JSON 回傳 error |

## Public Surface
internal only。

## Invariants
- 只透過 `sage_core::*` / `sage_graph::*` / `sage_llm::*` / `sage_writer::*` 的 `pub` API。
- 不窺探 writer / graph 私有結構。

## Tests
本目錄即為測試；由 `cargo test -p sage-writer --test writer_pipeline_it` 觸發。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M1。
