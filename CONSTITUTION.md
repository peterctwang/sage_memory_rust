# SAGE 專案開發憲法 (Constitution)

**Version:** 1.1.0
**Status:** Ratified — 2026-05-25 (Amended for behavioral guidelines)
**Scope:** `sage-memory` 全 workspace 與所有衍生產物。
**Precedence:** 本憲法高於 `SPEC_SAGE_Rust.md` 與任何 RFC；若衝突，以本文為準，並開立修憲 PR。

---

## 第零條 — 適用範圍與名詞

- **源代碼 (source)**：`crates/*/src/**`、`examples/**`、`benches/**` 中執行於產品路徑的程式碼。
- **測試代碼 (test)**：單元測試、整合測試、屬性測試、快照、fuzz、mock、fixtures。
- **索引 (index)**：`CLAUDE.md` 階層導覽檔與根目錄 `CLAUDE_INDEX.md`。
- **即時 (real-time)**：與當次代碼變更同 commit／同 PR；不允許「之後再補」。

---

## 第一條 — TDD 優先 (Test-Driven Development First)

### 1.1 Red → Green → Refactor 強制循環

任何新功能、bug 修復、refactor 都必須遵守：

1. **Red** — 先寫一個會失敗的測試，commit 訊息以 `test(red):` 起頭。
2. **Green** — 寫最小量產品碼讓測試通過，commit 訊息以 `feat(green):` 或 `fix(green):` 起頭。
3. **Refactor** — 在綠燈下重構，commit 訊息以 `refactor:` 起頭，**不得**新增功能。

> 例外：純文件、CI 設定、依賴升級可不走 TDD，但需於 PR 描述標明 `[no-tdd]` 並說明理由。

### 1.2 測試先行的證明

PR 模板必含「TDD 證據」欄位：列出 Red commit SHA 與 Green commit SHA。

```
TDD evidence:
  red:   abc1234  test(red): writer reward sums to 1 when α=β=γ
  green: def5678  feat(green): implement WriterReward::task normalization
```

無此欄位的 PR **拒絕合併**。

### 1.3 覆蓋率底線

- 行覆蓋率 ≥ 80%（以 `cargo-llvm-cov` 量測）。
- 分支覆蓋率 ≥ 70%。
- 任何 PR 不得使全局覆蓋率下降超過 0.5%。
- 公開 API（`pub` 且非 `#[doc(hidden)]`）覆蓋率 = 100%。

### 1.4 必備測試層

每個新增的 `pub fn` / `pub struct` 至少對應一項：

| 類型 | 必備測試 |
|---|---|
| 純函式（數學/公式） | 單元測試 + `proptest` 邊界 |
| Trait 實作 | 對 trait 契約的單元測試（共用 test suite） |
| async I/O | 整合測試（`tokio::test`） |
| 序列化結構 | round-trip 測試 + `insta` 快照 |
| 數值層 (GFM / reward) | 數值穩定性測試（NaN / Inf / 梯度爆炸） |

---

## 第二條 — 測試代碼不污染源代碼

### 2.1 物理分離

```
crates/<name>/
├── src/                  # 僅產品代碼，禁止任何 #[cfg(test)] 模組內含 fixture 工廠
│   ├── lib.rs
│   └── ...
├── tests/                # 整合測試（cargo 自動發現）
├── benches/              # criterion 基準
└── tests-support/        # 共用 fixtures / mocks，獨立 crate
```

- `src/**/*.rs` **不得**出現 `mod tests { ... }` 內超過 30 行的測試或任何 fixture 函式。
- 單元測試允許 `#[cfg(test)] mod tests`，但**僅限**呼叫被測模組與 `tests-support` crate；**禁止**在 src 內定義 `fn make_fake_*`、`const FIXTURE_*`、`struct MockXxx`。
- 任何 mock / fake / stub / spy 一律放 `crates/<name>/tests-support/`，以 `dev-dependencies` 引用。

### 2.2 命名與路徑規範

| 類別 | 允許路徑 | 禁止路徑 |
|---|---|---|
| 單元測試 | `src/**/*.rs` 內 `#[cfg(test)] mod tests` | `src/**/test_*.rs`、`src/**/*_test.rs` |
| 整合測試 | `tests/**/*.rs` | `src/**/integration_*.rs` |
| 共用 fixture | `tests-support/src/**/*.rs` | `src/fixtures.rs`、`src/test_utils.rs` |
| 基準 | `benches/**/*.rs` | `src/bench.rs` |
| Property 策略 | `tests-support/src/strategies.rs` | `src/strategies.rs` |

### 2.3 依賴隔離

- 測試專用 crate（`proptest`, `insta`, `mockall`, `criterion`, `tempfile`, `wiremock` …）**只能**出現在 `[dev-dependencies]` 或 `tests-support` crate 的 `[dependencies]`。
- 違規以 `cargo deny` + 自訂 lint script 在 CI 阻擋。

### 2.4 編譯閘

CI 必跑：

```bash
cargo build --release --workspace          # 純 src 必須能單獨編譯
cargo test  --workspace --all-features
cargo hack check --each-feature --no-dev-deps
```

`--no-dev-deps` 失敗 = 源代碼漏依賴測試碼，PR block。

---

## 第三條 — CLAUDE.md 階層索引

### 3.1 強制覆蓋

**每一個資料夾**（含 `crates/<name>/`、`crates/<name>/src/`、子模組目錄、`examples/`、`benches/`、`tests/`、`docs/`、`scripts/` …）都必須有 `CLAUDE.md`。例外只有：

- `target/`（建置產物）
- `.git/`、`.github/workflows/`（已由 GitHub 文件規範）
- 純空目錄 + `.gitkeep`

### 3.2 標準格式

每份 `CLAUDE.md` 採以下骨架，**所有欄位皆必填**：

```markdown
# <資料夾名稱>

> One-liner — 一句話描述本目錄存在的理由（≤ 80 字元）。

## Purpose
本目錄在系統中扮演的角色，與上游/下游模組的關係。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `foo.rs` | module | 實作 X，公開 `Foo` |
| `bar/` | dir | 子模組 — 見 `bar/CLAUDE.md` |

## Public Surface
本目錄對外公開的型別 / 函式 / trait（含一行說明）。若為私有目錄填「internal only」。

## Invariants
本目錄代碼必須維持的不變式（數值範圍、執行緒安全、生命週期…）。

## Tests
- 單元：`src/<file>.rs#cfg(test)`
- 整合：`crates/<name>/tests/<file>.rs`
- 基準：`benches/<file>.rs`

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 參考：[`SPEC §X.Y`](../../SPEC_SAGE_Rust.md#xy)

## Last Updated
2026-05-25 — <reason / PR #>
```

### 3.3 根目錄 `CLAUDE_INDEX.md`

根目錄維護一份**全 workspace 樹狀索引**，自動由 `scripts/gen_claude_index.rs` 生成：

```markdown
# CLAUDE Structure Index

> 自動產生，請勿手改。來源：`scripts/gen_claude_index.rs`
> 最後同步：<git commit short SHA> @ <ISO timestamp>

- [`crates/sage-core/`](crates/sage-core/CLAUDE.md) — 核心型別 + trait + reward 介面
  - [`src/`](crates/sage-core/src/CLAUDE.md) — 對外 public API 入口
    - [`entity.rs`](crates/sage-core/src/CLAUDE.md#entityrs) — `Entity`, `EntityId`, `TenantId`
    - [`graph.rs`](crates/sage-core/src/CLAUDE.md#graphrs) — `GraphStore` trait, `GraphSnapshot`
    - ...
  - [`tests/`](crates/sage-core/tests/CLAUDE.md) — 整合測試
- [`crates/sage-graph/`](crates/sage-graph/CLAUDE.md) — `MemGraphStore`, `SledGraphStore`
- ...
```

### 3.4 即時更新規則 (Real-Time Sync)

任何 PR 若觸碰以下變更，**必須在同一 PR** 內更新對應 `CLAUDE.md` 與 `CLAUDE_INDEX.md`：

| 觸發條件 | 應更新 |
|---|---|
| 新增 / 刪除 / 改名 檔案或目錄 | 該目錄與父目錄的 `CLAUDE.md` + 根 `CLAUDE_INDEX.md` |
| 新增 / 移除 `pub` 項目 | 該目錄 `CLAUDE.md` 的 `Public Surface` 區塊 |
| 變更不變式（例如數值範圍） | 該目錄 `CLAUDE.md` 的 `Invariants` 區塊 |
| 增刪測試檔 | 該目錄 `CLAUDE.md` 的 `Tests` 區塊 |
| 跨檔案重構 | 所有受影響目錄的 `CLAUDE.md` |

`Last Updated` 欄位每次必須改為當日 ISO 日期與 PR 編號／commit 訊息片段。

### 3.5 自動化護欄

CI 必跑下列 check（全部出現於 `.github/workflows/constitution.yml`）：

1. **`claude-md-presence`** — 掃描所有非例外目錄，缺 `CLAUDE.md` → fail。
2. **`claude-md-schema`** — 用簡易 markdown parser 確認 8 個必填章節皆存在。
3. **`claude-index-fresh`** — 跑 `scripts/gen_claude_index.rs --check`，若會產生 diff → fail。
4. **`claude-md-touched`** — 若 PR 改動 `crates/X/src/Y.rs`，但未動 `crates/X/src/CLAUDE.md` → fail，除非 PR 標籤含 `claude-md-exempt` 並由 maintainer 批准。
5. **`stale-timestamp`** — `Last Updated` 與最後一次目錄內代碼變更超過 14 天 → warn（M5 後升為 fail）。

### 3.6 索引產生器規格

`scripts/gen_claude_index.rs`：

- 從 workspace root 深度優先掃描，跳過例外目錄。
- 讀取每份 `CLAUDE.md` 的 one-liner 作為節點描述。
- 輸出純 markdown，行尾不含 trailing whitespace。
- `--check` 模式比對現有 `CLAUDE_INDEX.md`，差異即非零 exit。
- 自身亦受本憲法管轄：其 `CLAUDE.md` 描述產生器規格。

---

## 第四條 — 變更流程

### 4.1 一個 PR 一個意圖

- 一個 PR 不可同時做「新功能 + refactor + 文件大改」。
- 例外：跟隨新功能的最小幅 CLAUDE.md 更新（屬同意圖）。

### 4.2 修憲程序

修改本 `CONSTITUTION.md` 須：

1. 新增 RFC 至 `docs/rfcs/NNN-amend-constitution-<slug>.md`。
2. 至少兩位 maintainer approve。
3. 版本號依語意：破壞性 → major；新增條款 → minor；澄清 → patch。
4. 同 PR 更新所有受影響的 `CLAUDE.md`。

### 4.3 緊急豁免

生產環境 P0 事故可由值班 maintainer 發 hotfix 跳過 TDD 與 CLAUDE.md 同步，但須在事後 72 小時內補：

- 對應的 red→green 回填 commit
- 受影響目錄 `CLAUDE.md` 更新
- 事後檢討 (postmortem) 連結寫入 `docs/incidents/`

---

## 第五條 — 工具與自動化的最低要求

- `rustfmt`：全 workspace `cargo fmt --check` 強制過。
- `clippy`：`-D warnings`，禁用 `#[allow(clippy::...)]` 除非附 `// reason:` 註解。
- `cargo-deny`：許可、來源、版本三方面。
- `cargo-llvm-cov`：覆蓋率閘門（§1.3）。
- `cargo-hack`：feature matrix 與 `--no-dev-deps` 檢查。
- `pre-commit` hook：本地跑 `cargo fmt`、`cargo clippy -q`、`scripts/gen_claude_index.rs --check`。
- `commitlint`：強制 Conventional Commits + TDD prefix（§1.1）。

---

## 第六條 — 違憲處理

| 違反條款 | 後果 |
|---|---|
| §1（TDD） | PR 自動標 `needs-tdd`，CI block，requires red+green proof |
| §2（測試污染） | `cargo hack --no-dev-deps` 失敗，CI block |
| §3（CLAUDE.md） | `claude-md-*` check 失敗，CI block |
| §4.1（PR 意圖） | maintainer 可要求拆 PR |
| §5（工具） | CI block |
| §4.3（緊急豁免逾期未補） | 事後 PR 標 `constitution-violation`，列入季度回顧 |
| §7（行為準則四項未全勾） | reviewer `request changes`，必要時退回重做 |

---

## 第七條 — LLM 協作行為準則 (Behavioral Guidelines)

本條規範人類開發者與 LLM 協作（含 Claude Code / Copilot / Cursor 等）時 LLM 的行為。**人工 review 必須以本條為檢查清單**；不符合者退回重做。

> 取捨聲明：本條偏向「謹慎優於速度」。瑣碎任務（rename、typo、格式化）可依判斷略過，但凡涉及邏輯、API、依賴變動，**一律適用**。

### 7.1 編碼前先思考 (Think Before Coding)

不要臆測。不要藏起困惑。把取捨攤開。

實作之前：

- **明確陳述假設**。不確定就問。
- **存在多種詮釋**時，列出來讓使用者選；**不准**自行挑一個悶頭做。
- **若有更簡單的方法**，講出來；正當情況要回推使用者的方案。
- **不清楚就停下來**，指出哪裡不清楚，發問。

> 對照憲法：違反 §7.1 等同跳過 §1.1 Red 階段（沒先把問題想清楚就先寫 code）。

### 7.2 先求簡單 (Simplicity First)

解決問題所需的最少程式碼，不做任何投機性設計。

- **不加**沒要求的功能。
- **不為**單一使用點做抽象。
- **不為**沒要求的「彈性」或「可配置」加 hook。
- **不為**不可能發生的情境寫錯誤處理。
- **若寫了 200 行而其實 50 行就夠**，重寫。

自問：「資深工程師會不會覺得這太複雜？」會 → 簡化。

> 對照憲法：本條與 SPEC §0 設計準則互補；違反者於 PR review 直接以 `over-engineered` 標籤退件。

### 7.3 外科手術式變更 (Surgical Changes)

只動非動不可的部分。只清理自己造成的爛攤子。

編輯既有代碼時：

- **不**「順手改進」周邊代碼、註解或格式。
- **不**重構沒壞的東西。
- **遵循既有風格**，即使你會用不同寫法。
- **看到無關的死碼**：提出來，不要刪。

變更造成孤兒時：

- 移除**你的變更**造成未使用的 import / 變數 / 函式。
- **不要**刪除既存的死碼，除非被要求。

**驗收測試**：每一行被改動的程式碼都要能直接追溯到使用者的需求。做不到 → 那行就不該改。

> 對照憲法：違反 §7.3 直接觸發 §4.1（一個 PR 一個意圖）的拆 PR 要求。

### 7.4 以目標驅動執行 (Goal-Driven Execution)

定義成功條件，循環直到驗證通過。

把任務翻譯成可驗證目標：

- 「加上驗證」 → 「為非法輸入寫測試，然後讓它們通過」
- 「修這個 bug」 → 「先寫一個能重現 bug 的測試，再讓它通過」
- 「重構 X」 → 「保證重構前後測試都通過」

多步驟任務需先說明計畫：

```
1. [步驟] → verify: [檢查方式]
2. [步驟] → verify: [檢查方式]
3. [步驟] → verify: [檢查方式]
```

強的成功條件 → 可獨立 loop 到完成。弱的成功條件（「讓它動」）→ 需要反覆澄清，**不接受**。

> 對照憲法：§7.4 與 §1（TDD）天然吻合；Red 測試本身就是「可驗證的成功條件」。

### 7.5 自我檢核訊號 (Self-check signals)

本條若有發揮作用，會看到：

- diff 中**少了**不必要的變更。
- **少了**因過度複雜而導致的重寫。
- 澄清問題出現於**實作之前**，而非錯誤之後。

CI 無法量測本條；由 PR reviewer 在 review 模板勾選：

```
[ ] §7.1 假設已明確聲明 / 多解已列出
[ ] §7.2 已自問是否過度設計
[ ] §7.3 每一行改動可追溯回需求
[ ] §7.4 任務已轉為可驗證目標
```

四項未全勾 → PR `request changes`。

---

## 第八條 — 與 SPEC 的關係

- `SPEC_SAGE_Rust.md` 規範**「做什麼」**（架構、公式、API 形狀）。
- `CONSTITUTION.md` 規範**「怎麼做」**（流程、品質、可維護性）。
- 兩者衝突時：本憲法優先；同步更新 SPEC 並升其版本號。

---

## 附錄 A — 範例：建立新模組的完整 checklist

新建 `crates/sage-reader/src/addressing.rs`：

1. ☐ 建立 `crates/sage-reader/src/addressing/CLAUDE.md`（若該模組是目錄）或在 `src/CLAUDE.md` 的 `Contents` 區加一行。
2. ☐ Red commit：在 `tests-support` 加 strategy，在 `crates/sage-reader/tests/addressing_it.rs` 寫失敗測試。
3. ☐ Green commit：實作 `pub fn score_entry(...)`。
4. ☐ 更新 `crates/sage-reader/src/CLAUDE.md` 的 `Public Surface` / `Invariants` / `Tests`。
5. ☐ 跑 `scripts/gen_claude_index.rs` 更新根 `CLAUDE_INDEX.md`。
6. ☐ 更新 `Last Updated`。
7. ☐ 開 PR，填妥 TDD evidence。
8. ☐ CI 全綠後合併。

---

## 附錄 B — 範例 `CLAUDE.md`（`crates/sage-core/src/`）

```markdown
# sage-core/src

> SAGE 共用型別、trait 介面與 reward 中性層；任何模組都會依賴本目錄。

## Purpose
集中定義不依賴具體後端的型別與 trait，讓 sage-writer / sage-reader / sage-graph
能各自實作而互不引用。

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `lib.rs` | module | 對外 re-export |
| `entity.rs` | module | `Entity`, `Edge`, `TenantId`, schema 版本常數 |
| `graph.rs` | module | `GraphStore`, `GraphStoreBatch`, `GraphSnapshot` trait |
| `reader.rs` | module | `Reader` trait + `ReadOutput` |
| `reward.rs` | module | `WriterReward`, `TaskWeights`, `RewardCfg`, `compute_reward` |
| `query.rs` | module | `Query`, `QueryPlan`, `Probe` |
| `error.rs` | module | `SageError`, `Result<T>` |

## Public Surface
- `Entity`, `Edge`, `Document`, `Query`, `TenantId`
- `GraphStore`, `GraphStoreBatch`, `Reader` (trait)
- `WriterReward::{task, trajectory}`, `compute_reward`
- `SageError`, `Result<T>`

## Invariants
- `TenantId::DEFAULT == TenantId(0)`，永不變動。
- `schema_version` 一旦發布即只增不減。
- `Reward` 各分量定義域為 `[0, 1]`，組合後 `task ∈ [0, 1]`。

## Tests
- 單元：`src/entity.rs#cfg(test)` 等。
- 整合：`crates/sage-core/tests/serde_roundtrip.rs`, `tests/reward_invariants.rs`
- 屬性：`tests-support/src/strategies.rs` 提供 `arb_entity`, `arb_edge`。

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 參考：[`SPEC §2, §3, §4.2, §5`](../../../SPEC_SAGE_Rust.md#2-核心資料型別-sage-core)

## Last Updated
2026-05-25 — initial draft (#0001)
```

---

*Ratified by repository maintainers — 2026-05-25.*
*Any deviation must reference this document and propose an amendment via RFC.*
