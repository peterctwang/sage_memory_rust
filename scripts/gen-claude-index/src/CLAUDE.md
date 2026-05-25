# gen-claude-index/src/

> 索引生成器主程式 — 純標準函式庫，無外部依賴。

## Purpose
單檔 `main.rs` 實作三件事：遞迴掃描、讀 one-liner、render markdown；`--check` 模式做正規化比對後決定 exit code。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `main.rs` | bin | `Node` 結構 + `collect / render / build / normalize` + `main` |

## Public Surface
internal only — 全部是 `fn`，bin entry。

## Invariants
- `normalize`：把 CRLF 統一為 LF + 去尾空白，避免 Windows/Unix 差異造成 false-positive。
- `collect` 只把「擁有 CLAUDE.md 的目錄」納入；無 CLAUDE.md 的目錄不出現於索引也不阻擋掃描其子目錄（M0 行為，未來可改）。
- 不寫入任何 CLAUDE.md，只動 `CLAUDE_INDEX.md`。

## Tests
M0 暫無單元測試（M1 補：對 tempdir fixture 比對 golden markdown）。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
