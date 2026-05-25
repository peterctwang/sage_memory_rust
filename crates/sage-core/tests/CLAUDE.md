# sage-core/tests/

> sage-core 的整合測試與屬性測試。

## Purpose
證明跨 module 的不變式（reward 數值範圍、互動關係）在外部觀察點仍成立。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `reward_proptest.rs` | test | 5 條屬性：recovery / precision / ρ_rep 在 [0,1]、task ∈ [0,1]、trajectory ≤ task 當 fmt_bonus=0 |

## Public Surface
internal only。

## Invariants
- 不窺探私有結構；只用 `sage_core::*` 的 `pub` API。
- 屬性測試 256 cases / proptest 預設，無 NaN、無 panic。

## Tests
本目錄自身即為測試入口。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M1.5 reward 屬性測試。
