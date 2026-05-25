# sage-graph/

> `GraphStore` 後端實作集合；M0 只含 in-memory，sled/rocksdb 留待 M5。

## Purpose
把 `sage-core::GraphStore` trait 落地到具體儲存層。實作必須完整通過 trait 契約測試。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core、parking_lot、ahash、async-trait |
| `src/` | dir | 模組樹 — 見 `src/CLAUDE.md` |
| `tests/` | dir | 整合測試 — 見 `tests/CLAUDE.md` |

## Public Surface
- `MemGraphStore` — `Send + Sync`，所有狀態由 `parking_lot::RwLock` 保護。

## Invariants
- 多租戶完全隔離：`TenantId(a) ≠ TenantId(b)` ⇒ 兩者 entities/edges/snapshots 互不可見。
- `upsert_edge` 必驗證端點存在，否則回 `SageError::Invalid`。
- `snapshot` 為 deep clone；`restore` 取代當前 tenant 完整狀態。

## Tests
- 單元（6）：upsert/get、edge endpoint 檢查、neighbors、k_hop、tenant 隔離、snapshot roundtrip。
- 整合（2）：`tests/mem_integration.rs` — 透過 `tests-support` fixture 走 trait API。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- Trait 定義：[`../sage-core/src/graph.rs`](../sage-core/src/graph.rs)
- SPEC：[`§3`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M0 MemGraphStore。
