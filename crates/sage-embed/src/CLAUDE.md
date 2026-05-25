# sage-embed/src/

> Embedder 實作模組。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | re-export |
| `deterministic.rs` | module | `DeterministicEmbedder` — ahash + signed bag-of-words |
| `hnsw_index.rs` | module | `HnswIndex` — `VectorIndex` impl via `hnsw_rs` (feature `hnsw`) |

## Public Surface
- `DeterministicEmbedder::{new, with_seed, dim, embed}`

## Invariants
- `embed_one` 不分配大暫存區（一個 `Vec<f32>`）。
- 不持有任何 mutable state；所有方法 `&self`。

## Tests
- `deterministic.rs#cfg(test)`：5 tests。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M3-partial。
