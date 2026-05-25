# tests-support/src/

> Fixture 與 proptest 策略模組。

## Purpose
集中 SAGE 全 workspace 共用的測試輔助代碼，避免在產品 src 內出現 `make_fake_*` / `MockXxx`。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | `entity`, `edge`, `doc` 三個工廠函式 |
| `strategies.rs` | module | `arb_entity`, `arb_edge(max_id)` proptest 策略 |

## Public Surface
- `entity(id: u64, name: &str) -> Entity`
- `edge(src: u64, dst: u64, rel: &str) -> Edge`
- `doc(id: u64, text: &str) -> Document`
- `strategies::arb_entity`, `strategies::arb_edge`

## Invariants
- 工廠函式必為純函數（無 I/O、無時間相依）。
- proptest 策略生成的值必為 valid（不要產出非法輸入除非該策略本身就是負例策略）。

## Tests
無；本目錄的代碼由下游整合測試行為驗證。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M0。
