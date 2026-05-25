# sage-cli/

> `sage` 命令列工具：demo / stats / query。對 MemGraphStore 與 SledGraphStore 都可用。

## Purpose
給使用者一個可立即執行的入口，無需先寫 Rust 程式。對應 SPEC §1 中的 sage-cli。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | bin crate；依賴所有 sage-* + clap |
| `src/` | dir | bin entry — 見 `src/CLAUDE.md` |

## Public Surface
- bin `sage` — 子命令：
  - `sage demo` — 內建 ingest+query 場景（in-memory，無外部 LLM）
  - `sage stats --db <p> [--tenant N]` — sled 圖統計
  - `sage query --db <p> [--k N] [--tenant N] <text>` — 對 sled 圖跑 HeuristicReader

## Invariants
- 所有子命令以 JSON 形式輸出（除 demo 用人類可讀格式），便於管線串接。
- 不假設使用者已有 LLM 金鑰；demo 使用 `MockLlm` 內建腳本。

## Tests
M5-partial 暫無自動化 CLI 測試（手動 `cargo run -p sage-cli -- demo` 驗證）。
後續可加 `assert_cmd` 為基礎的 golden output 比對。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- SPEC：[`§1 Crate 拓撲`](../../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-25 — M5-partial 初版。
