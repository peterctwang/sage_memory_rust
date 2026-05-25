# sage-embed/

> Embedder 後端集合：M3 提供 `DeterministicEmbedder`（hash bag-of-words）；真實模型（BGE / E5）留待後續。

## Purpose
讓 SAGE 在不引入 candle / ONNX 的情況下就能驗證 `λ_cos` 通路。`DeterministicEmbedder` 是純函式、確定性、可直接進 CI。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core, async-trait, ahash |
| `src/` | dir | 模組 — 見 `src/CLAUDE.md` |

## Public Surface
- `DeterministicEmbedder` — `impl sage_core::Embedder`。

## Invariants
- 同字串 → 同向量（給定 dim + seed）。
- 非空文字輸出 L2 norm = 1；空文字輸出全零。
- `dim` 必須 ≥ 4，否則 panic。

## Tests
- 單元（5）：dim、deterministic、similar > random、空字串、單位範數。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- Trait 定義：[`../sage-core/src/embed.rs`](../sage-core/src/embed.rs)
- SPEC：[`§7`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M3-partial: DeterministicEmbedder 接入 λ_cos。
