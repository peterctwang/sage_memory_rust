# docs/

> 設計文件、handoff 規格、跨 session 知識交接。SPEC/CONSTITUTION 仍在 repo root。

## Purpose
存放比根目錄 `SPEC_SAGE_Rust.md` 更窄、針對單一未實作模組的設計細節。每份文件都是給「下一個動手寫的人」的完整 brief。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `M3_BGE_DESIGN.md` | design | BGE/E5 真實 embedder 整合計畫；解釋為何在當前 session 不交付，以及完整接入步驟 |

## Public Surface
無程式碼介面。

## Invariants
- 每份 design doc 必含「為何 defer / acceptance criteria / open questions」三段，便於後人 pick up。
- 不重複 `SPEC_SAGE_Rust.md` 已有內容；只補單一模組的實作層細節。

## Tests
無。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 高層規格：[`../SPEC_SAGE_Rust.md`](../SPEC_SAGE_Rust.md)

## Last Updated
2026-05-26 — M3 BGE/E5 handoff doc。
