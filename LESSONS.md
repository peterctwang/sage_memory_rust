# LESSONS.md

> SAGE 生產實戰中踩到的具體問題、根因、與修法。每條都對應一個 git commit
> 與一組 regression test，避免下次重複犯錯。

時間倒序排列（最新在上）。

---

## L008 — Router 必須有跨臂 failover（不只靠單臂 fallback）

**發生** 2026-05-27, Peplink 91-doc 完整 patent ingest

**症狀**
- `--llm router (light=minimax, deep=codex-cli)` 跑到第 82 doc，Codex 開始連續回傳
  `"usage limit"` (ChatGPT 訂閱 rate-limit)。
- Router 一旦 score>=threshold 就把 doc 派到 deep arm，**deep arm 失敗就直接記 failure**，
  不會降級 retry。
- 結果：9/91 docs 失敗，graph 缺一塊。
- 用戶評語：「router 裡面應該要有 fallback 自動選次好的」。

**根因**
`HeuristicRouter::complete` 設計只做 *routing*（pick 一臂），沒做 *recovery*（pick 失敗時換手）。
單臂 wrapper `FallbackLlm` 雖然存在但只能保護單一 backend，無法跨 light↔deep 切換。

**修法** (commit `ce273e1`)
1. `pick_pair()` 同時回 `(primary, alternate)`。
2. `complete()` 嘗試 primary；空回應 / Error → 自動切 alternate，遞增 `failover_hits`。
3. `judge()`：light 失敗 → 切 deep。
4. 兩個 composition layer（`FallbackLlm` 與 `HeuristicRouter`）共用 `EMPTY_THRESHOLD_CHARS = 2`，
   語意一致。

**事後驗證**
重跑 9 docs 同樣配置：
```
WARN router primary errored — failing over to alternate arm primary="deep" error=codex quota exhausted
INFO ingest-batch row applied doc_id=... entities=5 edges=8
[ingest-batch] doc ... ok (1/1)
```
9 docs 中 8 個被 MiniMax 接手救起 (89%)，**0 個再失敗於 quota**。

**Regression**
`crates/sage-llm/src/router.rs`：
- `deep_arm_error_failovers_to_light`
- `light_arm_error_failovers_to_deep`
- `deep_arm_empty_response_failovers_to_light`
- `both_arms_error_bubbles_failure`
- `judge_failovers_from_light_to_deep`

**衍生教訓**
- **永遠不要相信 LLM backend 不會 rate-limit**。subscription model 沒有單一 "valid token" 概念，
  限制隨機觸發。
- **品質敏感的批次任務**：light arm 不該選與 deep arm 同類型限制的 backend。
  MiniMax 32k context 無法接 100k char patent，failover 救得起來但品質掉。
  改建議：`light=gemini-cli` (1M context, OAuth 寬鬆) — 看 LESSON L009。

---

## L007 — 完整 patent 抽取需要 ≥8k output token 預算

**發生** 2026-05-27, Peplink full-patent export

**症狀**
- 段落版 v7 (~1k chars/doc) 抽 10 triple OK，writer `max_tokens: Some(512)` 夠。
- 完整版 Peplink (100k chars/doc) 預期應抽 50-150 triple，**`stop=true` 卻提早觸發**，
  下游 graph 僅 ~15 entity/doc，遠不到 patent 應有密度。

**根因**
`LlmWriterPolicy::build_prompt` 硬編碼 `max_tokens: Some(512)`。LLM 抽到 ~12 triple 就達 token cap，
強制 `"stop":true`，剩餘 entity 全丟。512 對段落 OK，對完整 patent 不夠。

**修法** (commit `3ba9276`)
```rust
max_tokens: Some(8192),  // 從 512 → 8192
```
單純漲 output cap。

**事後驗證**
重跑 Peplink full：每 doc 抽 ~50 entity / 64 edges，輸出 JSON 約 ~3000 tokens 即終止 — 8192
是安全有空間餘。Codex / Claude / Gemini 訊息 cap 都 >> 8192，沒副作用。

**衍生教訓**
- **抽 triple 任務的 output budget 隨輸入規模線性增加**。粗估：每 1k 輸入 chars 對應約 1 triple 額度，
  完整 patent (100k chars) 需 ~100 × 50 = 5000 tokens 輸出，所以 8192 是合理。
- 短文本不會浪費 — LLM `stop=true` 提早終止。

---

## L006 — apply.rs 在 entity 已存在時不更新 `source_docs`

**發生** 2026-05-27, v6 multi-hop tier 5 卡死 0.30

**症狀**
- 問 "Two CEOs of Microsoft" → 永遠只找到 doc 1015 (Bill Gates)。
- doc 1027 (Satya Nadella) 雖在 sled、文字明寫 "Microsoft CEO"，但 entity "Microsoft" 的
  `source_docs` **只列 1015**。
- 任何 "Microsoft" 為 anchor 的查詢都看不見 1027。

**根因**
```rust
// apply.rs::resolve()  原始版本
if store.get_entity(tenant, id).await?.is_none() {
    // create new entity with source_docs = [provenance]
}
Ok(id)  // ← entity 已存在時，返回 id 但 NEVER 更新 source_docs
```
第一個提到 "Microsoft" 的 doc 寫入後，所有後續 doc 對它的 mention 都被「dedup 吞掉」。

**修法** (commit `4c04716`)
```rust
match store.get_entity(tenant, id).await? {
    None => /* create as before */
    Some(mut e) => {
        if !e.source_docs.contains(&provenance) {
            e.source_docs.push(provenance);
            store.upsert_entity(tenant, e).await?;
        }
    }
}
```

**事後驗證**
v7 (100-doc paragraph corpus) tier 5 multi-hop R@3 從 v6 的 0.30 → **0.689 (+2.3×)**。

**Regression**
`apply::tests::shared_entity_accumulates_source_docs_across_docs` — 用 Gates + Nadella 場景
重現，斷言 Microsoft entity 同時連回兩 doc。

**衍生教訓**
- **graph 寫入路徑的 dedup ≠ skip update**。dedup 用來避免「重複建 entity」，但 metadata
  （`source_docs`, aliases, descriptions）必須累積。
- **跨領域**：在任何「圖節點存在即跳過」型 storage 邏輯，要特別檢查是否有「邊」或「屬性」
  該被合併。

---

## L005 — Writer prompt 不主動抽 co-mentioned entity

**發生** 2026-05-27, multi-hop 診斷

**症狀**
- writer prompt 只說「抽 triple」，沒指定 coverage 規則。
- M2.5 / Codex / Claude 預設只抽「主語人物」，doc 1027 ("Nadella took over as Microsoft CEO")
  只 emit `(Nadella, role, CEO)`，**漏掉 `(Nadella, works_at, Microsoft)`**。
- 即使 apply.rs dedup bug 修了，writer 沒抽出三角形，下游一樣斷掉。

**修法** (commit `ae3a1c5`)
prompt 加 COVERAGE RULE：
> Extract one triple for **every** named entity (person, org, place, product, work)
> explicitly mentioned in the doc, not just the main subject.

附具體 worked example（Nadella + Microsoft）讓 LLM 知道何為 coverage。

**事後驗證**
v2 30-doc 上 Recall@3 從 0.80 → **0.95 (+19%)**。COVERAGE RULE 對所有 backend 有效
（Codex / Claude / Gemini）。

**衍生教訓**
- **抽 entity 的 LLM 默認偏「敘事主角」**。不主動指示，就會漏 co-mentioned 的 org/time/place。
- **每個 prompt 改動都附 worked example**。「指令」沒用，「範例」最有效。

---

## L004 — Claude CLI argv 嚴禁含換行（Windows .cmd 限制）

**發生** 2026-05-27, FallbackLlm 第一次端到端觸發

**症狀**
- MiniMax 觸發 quota 後，所有後續 doc 切到 Claude fallback。
- 每個 Claude 子進程直接死 `"batch file arguments are invalid"`，670 docs 全 cascade fail。
- 詭異點：相同 Claude CLI 之前直接呼叫過幾百次都沒事。

**根因**
新加的 writer COVERAGE prompt 含字面 `\n\n`。`ClaudeCliLlm::build_argv` 把 system prompt
塞進 `--append-system-prompt <text>` 走 argv，Windows 的 `claude.cmd` shim 在 argv 含換行時
**整段被 cmd.exe 視為無效命令**。Unix 上不會發生。

**修法** (commit `ec633c6`)
```rust
cmd.push(s.replace(['\n', '\r'], " "));  // sanitize before passing to argv
```
另把 writer prompt 自身也壓成單行（防禦性，多一層保險）。

**Regression**
`claude_cli::tests::system_prompt_newlines_are_sanitized_for_windows_cmd`。

**衍生教訓**
- **跨平台 CLI 包裝層**：任何 argv 內容都要假設「最弱平台的 quoting 規則」。Windows .cmd
  特別誇張，不只換行，`&|<>^` 也都會炸。
- **Fallback 路徑要做端到端測試**：之前 fallback 從未在 prod 觸發，這個 bug 潛伏了好幾天。

---

## L003 — MiniMax 1500-call / 5h quota 會在 sustained batch 跑滿

**發生** 2026-05-27, v6 1001-doc ingest

**症狀**
- 跑到 ~331 doc 時，MiniMax HTTP 500：`"usage limit exceeded, 5-hour usage limit reached for
  Token Plan Starter (1500/1500 used)"`。
- 在這之前完全沒徵兆。

**根因**
MiniMax Starter plan 5h 滾動視窗共 1500 call (~5 call/min)。SAGE ingest sustained 在
**~7 call/min** 跑了 50 分鐘 = 350 call，但 visible quota counter 是 token 數，每 doc ~5
個 token 算下來 1500 用完即 cap。

**修法 (兩件事)**
1. `minimax.rs::post_chat` 加 fast-fail 偵測：
   ```rust
   if code == 429 && (body.contains("usage limit exceeded") || body.contains("(2056)")) {
       return Err(SageError::Llm(format!("MiniMax quota exhausted ...")))
   }
   ```
   不再做指數退避（quota 短期不會恢復，重試浪費時間）。
2. 加 `FallbackLlm` wrapper 讓 Claude 接手（見 L004）。

**衍生教訓**
- **subscription-style quota** 的失敗訊號不是 transient，要 fast-fail 而非 retry。
- 文件上 "1500 calls per 5h" 不等於「1500 tokens」也不等於「5h 後一定 reset」— quota 計算
  時常黑箱，要在程式碼裡識別具體錯誤訊息。

---

## L002 — Claude CLI subprocess 在 `wait_with_output` deadlock

**發生** 2026-05-26, v5 ingest 跑 14 小時都沒進度

**症狀**
- SAGE 對 Claude CLI 的呼叫掛在某個 doc，沒 output 沒 stderr 也沒 exit。
- 同時間在 task manager 看到 claude.exe child 一直存活但無 CPU 活動。

**根因**
```rust
// claude_cli.rs  原始版本
if let Some(stdin) = child.stdin.as_mut() {
    stdin.write_all(user.as_bytes())?;  // ← `as_mut` 借走，沒 drop
}
let out = child.wait_with_output()?;     // hang 在這
```
`stdin.as_mut()` 返回 `&mut Stdin`，scope 結束時 *沒有 drop*（borrow 結束 ≠ owner 釋放）。
Claude CLI 一直認為還有更多 stdin，永遠不收到 EOF，永遠不 produce output。

**修法** (commit `9134a8a`)
```rust
if let Some(mut stdin) = child.stdin.take() {  // take() 拿 owner
    stdin.write_all(user.as_bytes())?;
}
// stdin auto-drop here, sends EOF
let out = child.wait_with_output()?;
```
另外加 `tokio::time::timeout` 包圍整段，超過 `timeout_secs` 強制 kill。

**Regression**
`claude_cli::tests::timeout_is_wired_and_short_value_does_not_block_forever`。

**衍生教訓**
- **rust subprocess 介面**：`stdin.as_mut()` vs `stdin.take()` 是 silent bug — 編譯通過，
  行為差 180°。
- **所有 subprocess 呼叫都要 wrap timeout**，當 sanity net。

---

## L001 — Quantifier ("two/three/both") 沒進 stopword

**發生** 2026-05-26, v6 tier 5 multi-hop diagnosis

**症狀**
- 問 "Two CEOs of Microsoft" / "Two physicists" / "Two co-founders of OpenAI" — top 1 永遠是
  doc 2046 ("Frederick Sanger won **two** Nobel Prizes")。
- 19 條 tier 5 query 中 14 條的首位 hit 都是 2046。

**根因**
`HeuristicPlanner` 的 stopword 集合涵蓋 "the/a/of/who/when" 等常見虛詞，但 "two/three/both/
between" 這類 quantifier 沒列入。Query "Two CEOs of Microsoft" → expansions=["two", "ceos",
"microsoft"]。doc 2046 有 entity "two Nobel Prizes" / token "two" → match 命中。

**修法** (commit `45bad9d`)
```rust
const QUANTIFIERS: &[&str] = &["two","three","four","five","both","between","either"];
// 加進 STOPWORDS
```

**事後驗證**
v6 tier 5 R@3 從 **0.244 → 0.306 (+25%)**。doc 2046 不再 dominate。

**衍生教訓**
- **stopword 集合要看「實際語料中常出現但語意低」的詞**。教科書 stopword list (a/the/of)
  不夠，需要 corpus-specific augment。
- 看 ranker 為什麼把某個 doc 排頂 — 99% 是某個 token 雜訊放大。

---

## How to add a lesson

每次重大 bug 修完，pattern：
1. 拿一個 `L<NNN>` 編號（往下流水）。
2. **發生**：what corpus / what command / what date。
3. **症狀**：使用者觀察到的現象（不是 root cause）。
4. **根因**：定位到具體 commit / line / 邏輯錯誤。
5. **修法**：commit hash + 程式碼片段。
6. **事後驗證**：跑了什麼 dataset 證明修對了，數字前後對照。
7. **Regression**：哪個 test pin 住，下次別人改 code 不要再翻車。
8. **衍生教訓**：1-3 條跨領域可遷移的 takeaway。

避免「修了就忘」。
