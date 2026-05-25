# scripts/

> 開發輔助工具：CLAUDE.md 索引生成、未來的 CI 護欄與檢查腳本。

## Purpose
集中 workspace 級工具，每個工具一個獨立 cargo crate；產品 crate 不得依賴本目錄。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `gen-claude-index/` | crate | `CLAUDE_INDEX.md` 生成 / `--check` 模式 |

## Public Surface
N/A — 工具以可執行檔形式對外。

## Invariants
- 本目錄下的 crate 永遠 `publish = false`。
- 任何工具新增必同步加入根 `CLAUDE_INDEX.md` 與 CI workflow。

## Tests
- 工具自身：以小型 fixture dir + golden output 驗證（M0 暫缺，列為後續）。
- 對 CLAUDE.md hierarchy 的 `--check` 等同於 §3.5 的 CI 護欄。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 憲法：[`CONSTITUTION §3.5–§3.6`](../CONSTITUTION.md)

## Last Updated
2026-05-25 — M0。
