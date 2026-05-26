# sage-llm/examples/

> 對外可執行的 smoke / demo 例子；非單元測試，不進 CI 計數。

## Purpose
讓使用者用 `cargo run --example <name>` 一鍵驗證後端是否真的接得通本機環境（PATH 上的 binary、API key、網路等）。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `smoke_claude_cli.rs` | bin | 對本機 `claude` 二進位丟 prompt，印出回傳 JSON triples。讀 `SAGE_CLAUDE_BIN` 覆寫路徑 |

## Public Surface
internal — Cargo examples 自動發現，無 lib 表面。

## Invariants
- 失敗時 exit 0 並印診斷訊息（smoke 不是 gate；不該因為環境缺 binary 就讓 PR 紅）。
- 不依賴 dev-only crate（保持 example 在 prod feature 下也能編）。

## Tests
無自動化；user 手動執行驗證。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 來源 pattern：`C:/Users/User/Desktop/Mission 架構/harness/providers/claude_cli.py`

## Last Updated
2026-05-26 — `smoke_claude_cli` 初版。
