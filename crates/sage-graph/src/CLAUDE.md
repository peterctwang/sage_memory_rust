# sage-graph/src/

> 圖儲存後端模組。

## Purpose
依後端切分；每個模組對應一種儲存型態，公開一個實作 `GraphStore` 的型別。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export 各後端 (含 feature gate) |
| `mem.rs` | module | `MemGraphStore` — `parking_lot::RwLock<AHashMap>` |
| `sled_store.rs` | module | `SledGraphStore` — sled KV，JSON value，前綴 key layout |

## Public Surface
- `MemGraphStore::new()`
- `SledGraphStore::{open(path), temporary()}` (feature `sled`)

## Invariants
- 鎖粒度：MemGraphStore 用 tenant map 單一 `RwLock`。SledGraphStore 由 sled 內部處理。
- 不存在 `unsafe`。
- Sled key prefix bytes：`e/o/d/s/m` 分別代表 entity / out-edges / doc-index / snapshot / tenant-meta。

## Tests
- `mem.rs#cfg(test)`：6 個單元測試。
- `sled_store.rs#cfg(test)`：7 個單元測試（含跨 reopen 持久化）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
