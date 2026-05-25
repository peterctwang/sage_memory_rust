# sage-cli/src/

> bin entry。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `main.rs` | bin | `Cli` / `Cmd` clap 定義 + 三個 async 處理器 |

## Public Surface
internal only（bin）。

## Invariants
- `main.rs` 用 `tokio::main(flavor = "current_thread")` 避免拖入 multi-thread runtime overhead。
- 所有錯誤經 `anyhow::Result` 冒泡，由 main 統一處理（不 panic）。

## Tests
無；功能由 `cargo run -p sage-cli -- demo` 端到端驗證。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)

## Last Updated
2026-05-25 — M5-partial。
