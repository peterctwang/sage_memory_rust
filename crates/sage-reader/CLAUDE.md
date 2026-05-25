# sage-reader/

> 純 CPU 記憶讀取器：HeuristicPlanner + Soft Addressing + entity→doc 聚合。GFM 留 M3。

## Purpose
落實 SPEC §5.1–§5.2 / 論文 §4.2.2 的可解釋部分；給 M2 baseline，也作為 production fallback。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core, ahash, smol_str, async-trait, tracing |
| `src/` | dir | 模組 — 見 `src/CLAUDE.md` |
| `tests/` | dir | 整合測試 — 見 `tests/CLAUDE.md` |

## Public Surface
- `HeuristicReader<P>`（預設 P = `HeuristicPlanner`）— `impl Reader`
- `HeuristicPlanner` / `QueryPlanner` (trait)
- `score_entry(entity, plan, weights) -> Score`
- `softmax_entry(scores, t0) -> Vec<Score>`
- `AddressingWeights`（含 `lambdas[6]`、`t0`、`eta`）

## Invariants
- M2 的 `λ_cos = 0`（無 embedder），SPEC §5.2 6 個 stimulus 中其餘 5 個皆有實作。
- `score_entry` 對 `MustExclude` 命中回 `f32::NEG_INFINITY`，由 softmax 吸收為 0。
- `softmax_entry` 對全 `NEG_INFINITY` 或空輸入皆回穩定的 0 向量，從不 panic / NaN。
- Reader 嚴格守 tenant 隔離（透過 `EntityScan::all_entities(t)`）。

## Tests
- 單元（10）：addressing (7) + planner (3)。
- 整合（4）：`tests/reader_pipeline_it.rs` — ingest + query 端到端、空圖、無關 query、跨租戶隔離。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§5.1, §5.2`](../../SPEC_SAGE_Rust.md), [`§A.3`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M2 initial: HeuristicReader without GFM。
