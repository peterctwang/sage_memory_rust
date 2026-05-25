# crates/

> 全部產品 crate 的容器；每個子目錄是一個獨立 Cargo crate。

## Purpose
依 SPEC §1 切分 workspace；`sage-core` 為基石，其他 crate 只依賴 core，互不引用（CONSTITUTION §1 / SPEC §B.2）。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `sage-core/` | crate | 共用型別 + trait + reward 介面 |
| `sage-graph/` | crate | `GraphStore` 實作（M0：`MemGraphStore`）|
| `sage-llm/` | crate | LLM client trait + `MockLlm` |
| `sage-reader/` | crate | Heuristic reader + soft addressing (M2; GFM in M3) |
| `sage-runtime/` | crate | SageEngine 高層 API；ingest + query + query_with_answer (M2.5) |
| `sage-writer/` | crate | LLM-driven writer policy + sanitizer + apply (M1) |
| `tests-support/` | crate | 共用 fixture / proptest 策略（test-only）|

## Public Surface
N/A — 各子 crate 各自負責。

## Invariants
- `tests-support` 僅可出現於其他 crate 的 `[dev-dependencies]`。
- `sage-writer` ↔ `sage-reader` 永遠不得互相 `use`（待 M1+ 新增時遵守）。

## Tests
逐 crate `cargo test -p <name>`；workspace 級 `cargo test --workspace`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§1 Crate 拓撲`](../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M0 crate set.
