# sage-core/

> SAGE 共用型別、trait 介面與 reward 中性層；所有其他 crate 的依賴基石。

## Purpose
集中定義不依賴具體後端的型別與 trait，避免 writer/reader/graph 之間循環依賴（SPEC §B.2）。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴：serde、async-trait、thiserror、smol_str、smallvec、ahash |
| `src/` | dir | 模組樹 — 見 `src/CLAUDE.md` |

## Public Surface
- `Entity`, `Edge`, `EntityType`, `Document`, `Query`, `QueryPlan`, `Probe`, `Constraint`
- `TenantId`, `EntityId`, `DocId`, `Score`
- `GraphStore` (trait), `Subgraph`, `SnapshotId`
- `WriterReward`, `TaskWeights`, `RewardCfg`, `repetition_penalty`
- `SageError`, `Result`
- 常數：`ENTITY_SCHEMA_V`, `EDGE_SCHEMA_V`

## Invariants
- `TenantId::DEFAULT == TenantId(0)`，永不變動。
- Reward 各分量定義域 `[0, 1]`，`task` / `trajectory` 為純函式。
- Schema version 一旦發布只增不減。

## Tests
- 單元：各 module 內 `#[cfg(test)] mod tests`（15 個）。
- 整合：暫無（M0 直接由 `sage-graph/tests/` 經由 trait 行為測試）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§2, §3, §4.2, §5`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M0 initial types.
