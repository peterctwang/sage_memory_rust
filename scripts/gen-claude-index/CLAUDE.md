# gen-claude-index/

> 掃描 workspace、讀取每個 CLAUDE.md 的 one-liner，輸出根 `CLAUDE_INDEX.md`。

## Purpose
落實 CONSTITUTION §3.3 / §3.6：把 CLAUDE.md 階層轉成單一根索引，並提供 `--check` 給 CI 比對。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | bin crate，無外部依賴 |
| `src/` | dir | 主程式 — 見 `src/CLAUDE.md` |

## Public Surface
- bin：`gen-claude-index` （無參數寫入 / `--check` 模式）。

## Invariants
- 例外目錄清單 `EXCLUDED` 與隱藏目錄 (`.` 開頭) 一律跳過。
- one-liner 取自每份 CLAUDE.md 第一行 `> ` 開頭的引述。
- `--check` 失敗 = stale index → CI block。

## Tests
M0 暫無；M1 加 golden fixture 測試。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
