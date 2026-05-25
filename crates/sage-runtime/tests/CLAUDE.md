# sage-runtime/tests/

> 端到端整合測試：透過 SageEngine 驗證跨 crate 黏合正確。

## Purpose
這是 SAGE 對外的 smoke layer：如果這裡綠燈，使用者照範例 API 就能跑起來。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `engine_it.rs` | test | 4 case：ingest+query、query_with_answer、tenant 隔離、single doc |

## Public Surface
internal only。

## Invariants
- 不窺探任何 crate 私有結構；嚴格走 `pub` API。
- MockLlm 腳本順序：先所有 writer payload，再所有 answer payload（FIFO）。

## Tests
本目錄即為測試。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M2.5。
