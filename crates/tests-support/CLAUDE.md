# tests-support/

> 共用測試 fixture、mock、proptest 策略 — **test-only**，不得進入產品 dependency 樹。

## Purpose
落實 CONSTITUTION §2：把 fixture 工廠、proptest strategies 集中於獨立 crate，產品 src 維持純淨。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `Cargo.toml` | manifest | 依賴 sage-core、proptest、rand |
| `src/` | dir | 模組 — 見 `src/CLAUDE.md` |

## Public Surface
- 建構工廠：`entity(id, name)`, `edge(src, dst, rel)`, `doc(id, text)`
- Strategies：`strategies::arb_entity`, `strategies::arb_edge`

## Invariants
- **絕對不可**出現在任何 crate 的 `[dependencies]`；只允許 `[dev-dependencies]`。違者由 CI 阻擋（將於 §C.3 feature flag check 落實）。
- 不依賴任何具體後端 crate（sage-graph / sage-llm），只用 sage-core。

## Tests
本 crate 自身無測試；其健康度由「下游整合測試是否能引用」反映。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 憲法：[`CONSTITUTION §2.3`](../../CONSTITUTION.md)

## Last Updated
2026-05-25 — M0。
