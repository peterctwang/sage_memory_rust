# sage-graph/

> `GraphStore` 後端實作集合：`MemGraphStore`（M0）+ `SledGraphStore`（M5）。

## Purpose
把 `sage-core::GraphStore` trait 落地到具體儲存層。實作必須完整通過 trait 契約測試。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core、parking_lot、ahash、async-trait |
| `src/` | dir | 模組樹 — 見 `src/CLAUDE.md` |
| `tests/` | dir | 整合測試 — 見 `tests/CLAUDE.md` |

## Public Surface
- `MemGraphStore` — in-process，`parking_lot::RwLock` 保護的 `AHashMap`。
- `SledGraphStore`（feature `sled`，預設啟用）— 持久化於 `sled::Db`，JSON 序列化。
  - `open(path)` / `temporary()`

## Invariants
- 多租戶完全隔離：`TenantId(a) ≠ TenantId(b)` ⇒ 兩者 entities/edges/snapshots 互不可見（兩個實作皆遵守）。
- `upsert_edge` 必驗證端點存在，否則回 `SageError::Invalid`。
- `snapshot` 為深拷貝；`restore` 取代當前 tenant 完整狀態（snapshots 本身不被覆寫）。
- 兩個實作對外行為等價；新增測試請對兩者都覆蓋（避免暗中分歧）。

## Tests
- `MemGraphStore`：6 unit + 2 integration。
- `SledGraphStore`：7 unit（含 `persists_across_reopen`）+ 2 integration（含 `reload_preserves_graph`）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- Trait 定義：[`../sage-core/src/graph.rs`](../sage-core/src/graph.rs)
- SPEC：[`§3`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M0 MemGraphStore。
