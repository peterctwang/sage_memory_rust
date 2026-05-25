# sage-graph/src/

> 圖儲存後端模組。

## Purpose
依後端切分；每個模組對應一種儲存型態，公開一個實作 `GraphStore` 的型別。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export 各後端 |
| `mem.rs` | module | `MemGraphStore` — `parking_lot::RwLock<AHashMap>` |

## Public Surface
- `MemGraphStore::new()`

## Invariants
- 鎖粒度：tenant map 單一 `RwLock`；M0 不要求高並發。
- 不存在 `unsafe`。

## Tests
- `mem.rs#cfg(test)`：6 個單元測試（見父層 CLAUDE.md）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
