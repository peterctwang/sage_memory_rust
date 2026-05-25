# sage-graph/tests/

> 整合測試 — 透過 trait 公開介面驗證 `MemGraphStore` 行為。

## Purpose
證明 §2.3 依賴隔離可行：整合測試以 `tests-support` 取得 fixture，從 trait 角度跑端到端流程，不碰 src 內部結構。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `mem_integration.rs` | test | 2 個 case：fixture walkable graph、tenant 隔離 |

## Public Surface
internal only。

## Invariants
- 只可透過 `sage_core::*` 與 `sage_graph::*` 的 `pub` API 進入；不得 `use sage_graph::mem::TenantData`。
- 不得從 `crates/sage-graph/src/` 內部複製私有結構。

## Tests
本目錄自身即為測試；由 `cargo test -p sage-graph --test mem_integration` 觸發。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- Fixtures：[`../../tests-support/src/lib.rs`](../../tests-support/src/lib.rs)

## Last Updated
2026-05-25 — M0。
