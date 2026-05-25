# sage-core/src/

> sage-core 的模組實作層。

## Purpose
按關注點切分核心型別 module；對外只透過 `lib.rs` re-export，子模組是穩定 API 邊界。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | 對外 re-export 與模組宣告 |
| `error.rs` | module | `SageError`, `Result<T>`，serde_json 錯誤 from |
| `ids.rs` | module | `EntityId`, `DocId`, `Score`, `TenantId(+Display)` |
| `entity.rs` | module | `Entity`, `Edge`, `EntityType`, schema 常數 |
| `document.rs` | module | `Document` |
| `query.rs` | module | `Query`, `QueryPlan`, `Probe`, `Constraint` |
| `graph.rs` | module | `GraphStore` trait, `Subgraph`, `SnapshotId` |
| `reader.rs` | module | `Reader` trait, `ReaderGraph`, `EntityScan`, `ReadOutput` |
| `reward.rs` | module | `WriterReward`, reward math, `habituation`, `forgetting` |
| `embed.rs` | module | `Embedder` trait + `cosine` helper |
| `ops.rs` | module | `scatter_add_1d` / `scatter_add_rows` CPU kernels (M3 spike) |
| `vector_index.rs` | module | `VectorIndex` trait — impls in `sage-embed` |

## Public Surface
見 `crates/sage-core/CLAUDE.md` 的 Public Surface 區塊。

## Invariants
- `lib.rs` 的 `pub use` 是穩定 API 表面；新增 `pub` 項目需同步更新本檔。
- 模組間禁止循環引用（目前自然樹狀）。

## Tests
- `error.rs`：無（薄包裝）。
- `ids.rs`：Display 由型別保證。
- `entity.rs`：4 tests — defaults / serde roundtrip / custom variant。
- `document.rs`：1 test。
- `query.rs`：3 tests — defaults / builder / empty plan。
- `reward.rs`：7 tests — task / trajectory / ρ_rep 邊界。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0 initial.
