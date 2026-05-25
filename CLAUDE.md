# SAGE Memory Framework — Workspace Root

> Rust workspace 實作 SAGE 自演化圖記憶引擎；遵循 `CONSTITUTION.md` 流程合約。

## Purpose
本倉庫實作 `SPEC_SAGE_Rust.md`：把論文 `SAGE_Paper.md` (arXiv 2605.12061) 的 Writer + GFM Reader + 自演化迴圈落地為 Rust crate workspace，供 LLM Agent 作為長期記憶層。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | workspace root，集中依賴與 lints |
| `CONSTITUTION.md` | doc | 開發憲法（TDD / 測試隔離 / CLAUDE.md / 行為準則）|
| `SPEC_SAGE_Rust.md` | doc | 系統規格 |
| `SAGE_Paper.md` | doc | 論文重點翻譯 |
| `CLAUDE_INDEX.md` | doc | 自動生成的階層索引（勿手改）|
| `crates/` | dir | 全部產品 crate — 見 `crates/CLAUDE.md` |
| `scripts/` | dir | 工具腳本 — 見 `scripts/CLAUDE.md` |

## Public Surface
工作區對外無 `pub` 介面。對使用者的進入點為 `sage-core` re-export（未來經 `sage-runtime` 整合）。

## Invariants
- `unsafe_code = "forbid"`（workspace lint）。
- 所有產品 crate 不得 `[dependencies]` 引入測試專用 crate（CONSTITUTION §2.3）。
- 任何目錄新增／刪除必同 PR 更新本層與根 `CLAUDE_INDEX.md`（CONSTITUTION §3.4）。

## Tests
- `cargo test --workspace`：總體入口。
- 整合測試分布於 `crates/<n>/tests/`。

## Related
- 規格：[SPEC_SAGE_Rust.md](SPEC_SAGE_Rust.md)
- 流程：[CONSTITUTION.md](CONSTITUTION.md)
- 論文：[SAGE_Paper.md](SAGE_Paper.md)

## Last Updated
2026-05-25 — M0 skeleton bootstrap.
