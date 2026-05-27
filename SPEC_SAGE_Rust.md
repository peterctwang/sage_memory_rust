# SAGE Memory Module — Rust 開發規範 (SPEC)

**Version:** 0.2.0 (post audit-pass 1)
**Target:** Rust 1.78+ (stable, edition 2021)
**Crate name:** `sage-memory`
**Reference paper:** [SAGE_Paper.md](SAGE_Paper.md) (arXiv 2605.12061)
**讀法：** 第 §0–§13 章為實作合約；§A 為論文公式對照、§B 為審查筆記、§C 為跨模組規範。本版已把 §A/B/C 的關鍵決策回填至主章節，附錄保留追溯用途。

---

## 0. 設計目標

打造一個 **自演化的圖記憶引擎**，作為 LLM Agent 的長期記憶層。核心循環：

```
       ┌────────────┐  triples   ┌──────────────┐
 input │   Writer   │ ─────────► │  Graph Store │
 ────► │ (Policy π) │            └──────┬───────┘
       └────▲───────┘                   │ query
            │ reward                    ▼
            │            ┌──────────────────────────┐
            └────────────│  Reader (GFM)            │──► evidence + answer
                         │  (Plan → Activate → Prop)│
                         └──────────────────────────┘
```

設計準則：

1. **零拷貝 / 借用優先** — 圖節點與 embedding 走 `Arc<[f32]>` 或 `ndarray::ArrayView`。
2. **trait-based 替換性** — 任何 LLM / Embedding / Judge / Storage 後端均可換。
3. **async-first** — I/O 與 LLM 呼叫一律 `async`；計算密集走 `rayon`。
4. **可觀測** — 透過 `tracing` 暴露 writer 步驟與 reader 激活路徑。
5. **可重現** — 訓練/演化 step 都記錄 seed 與 trajectory，可 replay。

---

## 1. Crate 拓撲

```
sage-memory/
├── Cargo.toml                 # workspace root
├── crates/
│   ├── sage-core/             # 共用型別 + Reader/Writer/GraphStore trait + reward 介面
│   ├── sage-graph/            # 圖存儲實作 (in-mem / sled / petgraph)
│   ├── sage-writer/           # Policy writer 實作 + GRPO trainer
│   ├── sage-reader/           # GFM reader 實作 (planning / addressing / propagation)
│   ├── sage-embed/            # embedding 抽象 + 內建 backends
│   ├── sage-llm/              # LLM client trait + OpenAI/Anthropic/local
│   ├── sage-eval/             # 評估 (Recall@k, F1, judge)
│   ├── sage-runtime/          # 整合 writer/reader/evolve 的 high-level API
│   └── sage-cli/              # 命令列工具 (ingest / query / evolve)
└── examples/
    ├── ingest_corpus.rs
    ├── query.rs
    └── self_evolve.rs
```

依賴關係（避免循環依賴）：`sage-writer` 與 `sage-reader` 都只 `use sage-core::*`，互不引用；reward 路徑由 `sage-core::reward` 中性介面承接，runtime 注入具體實例。

依賴策略：

| 用途 | crate |
|---|---|
| async runtime | `tokio` |
| 張量 / 線代 | `ndarray`, `nalgebra` |
| GNN / autodiff | `candle-core` + `candle-nn` (預設) ｜ 可插 `burn` |
| 圖 | `petgraph` (in-mem 拓撲), `sled` 或 `rocksdb` (持久化) |
| 向量索引 | `hnsw_rs` 或 `usearch` |
| 序列化 | `serde`, `bincode`, `serde_json` |
| 並行 | `rayon`, `dashmap` |
| 雜湊 / 字串 | `ahash`, `smol_str`, `smallvec` |
| 觀測 | `tracing`, `tracing-subscriber` |
| 錯誤 | `thiserror`, `anyhow` |
| 設定 | `figment` 或 `config` |
| 隨機 | `rand` (`StdRng`，禁用 `thread_rng`) |

---

## 2. 核心資料型別 (`sage-core`)

```rust
pub type EntityId = u64;
pub type DocId    = u64;
pub type Score    = f32;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantId(pub u64);
impl TenantId { pub const DEFAULT: TenantId = TenantId(0); }

pub const ENTITY_SCHEMA_V: u16 = 1;
pub const EDGE_SCHEMA_V:   u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub schema_version: u16,           // 預設 ENTITY_SCHEMA_V，bincode 前 2 byte
    pub id: EntityId,
    pub tenant: TenantId,
    pub name: SmolStr,
    pub aliases: SmallVec<[SmolStr; 4]>,
    pub etype: EntityType,
    pub desc: Option<Arc<str>>,
    pub embedding: Option<Arc<[f32]>>,
    pub source_docs: SmallVec<[DocId; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub schema_version: u16,           // EDGE_SCHEMA_V
    pub tenant: TenantId,
    pub src: EntityId,
    pub dst: EntityId,
    pub relation: SmolStr,
    pub weight: f32,           // habituation / repetition aware
    pub provenance: DocId,
    pub created_at: u64,       // logical clock
}

#[derive(Clone, Debug)]
pub struct Document {
    pub id: DocId,
    pub text: Arc<str>,
    pub embedding: Option<Arc<[f32]>>,
    pub meta: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub text: Arc<str>,
    pub embedding: Option<Arc<[f32]>>,
    pub k: usize,
}
```

`EntityType` 採開放枚舉：`Person | Org | Concept | Event | Time | Custom(SmolStr)`。

---

## 3. 圖存儲 (`sage-graph`)

```rust
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn upsert_entity(&self, t: TenantId, e: Entity) -> Result<EntityId>;
    async fn upsert_edge(&self, t: TenantId, e: Edge)   -> Result<()>;
    async fn get_entity(&self, t: TenantId, id: EntityId) -> Result<Option<Entity>>;
    async fn neighbors(&self, t: TenantId, id: EntityId, max: usize) -> Result<Vec<(EntityId, Edge)>>;
    async fn k_hop(&self, t: TenantId, seeds: &[EntityId], hops: u8) -> Result<Subgraph>;

    // 𝒢ₕ₊₁ = 𝒢ₕ ⊕ aₕ — writer 動作落圖（事務性）
    async fn apply_action(&self, t: TenantId, a: &WriterAction) -> Result<GraphDelta>;

    // 結構特徵 (φ, ψ, r_𝒢) 預計算 / 快取
    async fn structural_features(&self, t: TenantId) -> Result<StructuralCache>;

    // 文件對映
    async fn docs_of_entity(&self, t: TenantId, id: EntityId) -> Result<Vec<DocId>>;

    // 自演化期間支援快照
    async fn snapshot(&self, t: TenantId) -> Result<SnapshotId>;
    async fn restore(&self, t: TenantId, snap: SnapshotId) -> Result<()>;
}

/// 訓練 / 推論熱路徑使用：取得當前圖的不可變視圖，避免 async overhead。
pub trait GraphStoreBatch: GraphStore {
    fn neighbors_batch(&self, t: TenantId, ids: &[EntityId], max: usize)
        -> Vec<Vec<(EntityId, Edge)>>;
    fn k_hop_batch(&self, t: TenantId, seeds: &[Vec<EntityId>], hops: u8) -> Vec<Subgraph>;
    fn snapshot_view(&self, t: TenantId) -> Arc<GraphSnapshot>;   // CSR + ndarray
}
```

實作：

- **`MemGraphStore`** — `petgraph` + `DashMap`，本機/單元測試用。
- **`SledGraphStore`** — `sled` KV，持久化，預設選項。
- 預留 trait 給未來 `Neo4jStore` / `SurrealStore`。

向量索引由 `EntityVectorIndex` (HNSW) 旁路維護，避免污染圖介面。

---

## 4. Memory Writer (`sage-writer`)

對應論文 §4.1。

### 4.1 Policy trait

```rust
#[async_trait]
pub trait WriterPolicy: Send + Sync {
    /// 給定狀態，產生下一步動作 (triples + 來源 anchor)。
    async fn step(&self, state: &WriterState) -> Result<WriterAction>;
}

pub struct WriterState<'a> {
    pub query: Option<&'a Query>,         // 訓練/條件式寫入時使用
    pub candidates: &'a [Document],
    pub partial_graph: &'a SubgraphView<'a>,
    pub processed: &'a [DocId],
    pub step: u32,
}

pub struct WriterAction {
    pub triples: SmallVec<[(EntityRef, SmolStr, EntityRef); 8]>,
    pub source: DocId,
    pub stop: bool,
}

pub enum EntityRef {
    Existing(EntityId),
    New { name: SmolStr, etype: EntityType, desc: Option<String> },
}
```

預設實作 **`LlmWriterPolicy<L: LlmClient>`**：以 LLM 抽取 triples，prompt 包含 partial graph 摘要與 reader 上一輪失敗線索。

### 4.2 Reward 計算

```rust
pub struct TaskWeights { pub alpha: f32, pub beta: f32, pub gamma: f32 }
pub struct RewardCfg {
    pub weights:    TaskWeights,
    pub lambda_rep: f32,
    pub lambda_fmt: f32,
}

pub struct WriterReward {
    pub r_rec: f32,
    pub r_pre: f32,
    pub r_ded: f32,
    pub r_ans: f32,
    pub rep_penalty: f32,
    pub fmt_bonus: f32,
}

impl WriterReward {
    pub fn task(&self, w: &TaskWeights) -> f32 {
        (w.alpha*self.r_rec + w.beta*self.r_pre + w.gamma*self.r_ded)
            / (w.alpha + w.beta + w.gamma)
    }
    pub fn trajectory(&self, cfg: &RewardCfg) -> f32 {
        self.task(&cfg.weights) - cfg.lambda_rep*self.rep_penalty
            + cfg.lambda_fmt*self.fmt_bonus
    }
}
```

`compute_reward(reader, sample, graph) -> WriterReward` 定義於 `sage-core::reward`，依賴中性 `Reader` trait，由 runtime 注入具體 reader，避免 writer↔reader 循環依賴。

### 4.3 訓練器

```rust
pub struct GrpoTrainer<P: WriterPolicy, R: Reader> { ... }
impl<P, R> GrpoTrainer<P, R> {
    pub async fn step(&mut self, batch: &[Sample]) -> Result<TrainStats>;
}
```

GRPO 實作要點：

- 對每個 sample 採樣 G 條軌跡，計算 group-relative advantage。
- 採 clipped ratio (`ε = 0.2`)。
- 寫入 `tracing` span，便於離線分析。
- 第一版可暫不訓練 LLM 權重，僅做 **prompt / temperature / triple-filter 的策略空間** 探索，再升級成 LoRA / 完整微調。

---

## 5. Memory Reader (`sage-reader`)

對應論文 §4.2。介面（trait 定義於 `sage-core`，實作在 `sage-reader`）：

```rust
#[async_trait]
pub trait Reader: Send + Sync {
    async fn read(&self, t: TenantId, q: &Query, g: Arc<GraphSnapshot>) -> Result<ReadOutput>;
}

// ℛφ(q, 𝒢, M) -> (𝒟̂ₖ, 𝒢̂q, Πq)
pub struct ReadOutput {
    pub docs:     Vec<(DocId, Score)>,   // 𝒟̂ₖ
    pub entities: Vec<(EntityId, Score)>,
    pub subgraph: Subgraph,              // 𝒢̂q
    pub paths:    Vec<RelationPath>,     // Πq (可選)
}
```

熱路徑統一吃 `Arc<GraphSnapshot>`（CSR + ndarray 視圖），訓練/推論皆從 `GraphStoreBatch::snapshot_view` 取得，避免在 propagation 內 await。

### 5.1 Query Planner `𝒫ω`

```rust
pub struct QueryPlan {
    pub expansions: Vec<SmolStr>,           // ℰ_exp
    pub aliases:    Vec<SmolStr>,           // 𝒜
    pub relations:  Vec<SmolStr>,           // 𝒞_rel
    pub hard_constraints: Vec<Constraint>,  // 𝒞_hard
    pub etype_hint: Option<EntityType>,     // τ
    pub probes: Vec<Probe>,                 // {(q̃_m, α_m, t_m)}
}
pub struct Probe { pub text: Arc<str>, pub alpha: f32, pub etype: Option<EntityType> }
```

預設 `LlmQueryPlanner`；另提供 `HeuristicPlanner` (NER + alias dict) 作 fallback。

### 5.2 Soft Addressing

純計算函式 (CPU/SIMD 友善，可 `rayon`)：

```rust
pub fn score_entry(
    entity: &Entity,
    plan: &QueryPlan,
    weights: &AddressingWeights,   // λ₁..λ₆
    embed: &dyn Embedder,
) -> f32;
```

對應論文公式 `sₑ(q)`。softmax 統一在 `activate` 內 (溫度 `T₀` 可配置)。

### 5.3 結構化傳播 (GFM)

放在 `sage-reader::gfm`，使用 **`candle`** 撰寫：

- `StructFeatExtractor` — 算 `ϕ(v)`, `ψ(u,v)`, `r_𝒢`。
- `GfmLayer { gate: MLP, msg: Linear, norm: LayerNorm }`。
- `forward(graph, h0) -> H`：vector gating `g = 1 + δ·tanh(MLP(z))`。
- `ContextSchemaHead` — 雙通道 `H = H_ctx + β_sch · H_sch`，schema 通道維持 K 組可訓練 prompt bases。

權重支援：

1. 隨機初始化 (測試)。
2. 載入預訓練 checkpoint (`safetensors`)。
3. 訓練 API：`Reader::train_contrastive` + `Reader::train_supervised`。

### 5.4 文件投影

`p_φ(d | q, 𝒢, 𝒟) = σ( Wd · [pool(H_{ent∈d}); query]; mask )` — 對 entity-to-doc mapping 採稀疏矩陣 + `sparse-dot`。

---

## 6. 自演化 (`sage-runtime`)

```rust
pub struct SageEngine<W, R, S, E, L>
where
    W: WriterPolicy, R: Reader,
    S: GraphStore, E: Embedder, L: LlmClient,
{ ... }

impl<...> SageEngine<...> {
    pub async fn ingest(&self, docs: impl IntoIterator<Item = Document>) -> Result<IngestReport>;
    pub async fn query(&self, q: &Query) -> Result<AnswerBundle>;
    pub async fn evolve(&mut self, cfg: EvolveCfg) -> Result<EvolveReport>;
}
```

`evolve()` 對應論文 §4.3：

```text
for round in 1..=cfg.rounds {
    1. freeze reader; train writer on labeled samples using reward(reader)
    2. writer regenerates graph snapshot
    3. resume reader training (contrastive + supervised) on new graph
    4. checkpoint everything (writer, reader, graph snapshot)
}
```

漂移控制：每輪計算 `‖H_t − H_{t-1}‖` 作為穩定性指標，超過閾值則回退到上一個 snapshot (Proposition 1-iii)。

---

## 7. 抽象後端 traits

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, req: ChatRequest)  -> Result<ChatResponse>;
    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool>;
}

#[async_trait]
pub trait Embedder: Send + Sync {
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Arc<[f32]>>>;
}
```

內建：`AnthropicLlm` (feature `anthropic`)、`ClaudeCliLlm`/`CodexCliLlm`/`GeminiCliLlm` (subprocess backends, feature-gated)、`MinimaxLlm` (feature `minimax`)、`MockLlm`；`BgeEmbedder`, `E5Embedder` via `candle`.

### 7.1 Composite clients

兩個 wrapper 同樣實作 `LlmClient`，可以**透明套在任何呼叫點**（writer / planner / judge），不必改現有流程。

#### `FallbackLlm<P, F>` — 縱向兩段保險

```rust
pub struct FallbackLlm<P: LlmClient + ?Sized, F: LlmClient + ?Sized> {
    primary:  Arc<P>,
    fallback: Arc<F>,
    fallback_count: AtomicU64,
}
```

行為（`complete`）：
1. 呼 `primary.complete()`。
2. 若 `Ok` 且 `content.trim().chars().count() >= 2` → 直接 return。
3. 否則（空回應 / Error）→ 呼 `fallback.complete()`，遞增 `fallback_count`。

`judge()` 只在 primary `Err` 時 fallback（YES/NO 沒有「empty payload」概念）。

設計用途：包單一不穩定的 backend (例如 MiniMax) 加一層 Claude 兜底。

#### `HeuristicRouter` — 橫向兩臂 + 跨臂 failover

```rust
pub struct HeuristicRouter {
    light: Arc<dyn LlmClient>,
    deep:  Arc<dyn LlmClient>,
    threshold: u32,
    light_hits / deep_hits / failover_hits: AtomicU64,
}
```

行為（`complete`）：
1. `profile = profile_user_content(&req)` — 計算 user message 的 `(char_len, sentence_count, capitalized_phrase_count)`。
2. `score = char_len/50 + caps×5 + sentences×2`。
3. 若 `score >= threshold` → primary = deep, alternate = light；反之亦然。
4. 試 primary。空回應 / Error → 自動切 alternate（跨臂 failover），遞增 `failover_hits`。
5. 兩臂都失敗才 bubble error。

`judge()`：永遠先打 light（YES/NO 不需深推理）；light 失敗才打 deep。

`profile_user_content` 故意只計算 `Role::User` 訊息 — system prompt 是固定 boilerplate，不該影響決策。

**Invariants**：
- `EMPTY_THRESHOLD_CHARS = 2`。`FallbackLlm` 與 `HeuristicRouter` 共用同一定義以維持兩層 composition 語意一致。
- Router arms 可遞迴 — `light` 或 `deep` 可以自己是另一個 `FallbackLlm` 或 `HeuristicRouter`（CLI 用 `build_llm_client(kind)` 構造，但禁止 arm 本身是 `"router"`，避免無限遞迴）。
- 兩個 wrapper 都 `Clone`-able，shared state 走 `Arc<AtomicU64>`。

**Telemetry**：`router.failover_hits()` 跑完批次後可查實際救援次數，作為品質/可靠性指標。

**Roadmap (M5+)**：拓展為 N-arm chain (priority list)，依優先序逐一 failover；目前 2-arm 足夠對應「便宜 + 高品質」與「主 + 備援」兩個常見場景。

---

## 8. 設定 / Config

`sage.toml` 範例：

```toml
[writer]
policy        = "llm"
llm           = "anthropic:claude-opus-4-7"
max_steps     = 12
[writer.reward]
alpha = 0.4; beta = 0.3; gamma = 0.3
lambda_rep = 0.2; lambda_fmt = 0.05

[reader]
gfm_ckpt        = "models/gfm-base.safetensors"
layers          = 3
hidden          = 256
temperature_t0  = 0.7
beta_schema     = 0.5
prompt_bases_k  = 8
[reader.addressing]
lambdas = [1.0, 0.6, 1.2, 0.3, 0.4, 0.8]
eta     = 0.5

[graph]
backend = "sled"
path    = "./data/graph.sled"

[embed]
model = "bge-m3"
dim   = 1024

[evolve]
rounds          = 2
drift_threshold = 0.15
```

---

## 9. 錯誤與觀測

```rust
#[derive(Error, Debug)]
pub enum SageError {
    #[error("graph store: {0}")]   Graph(String),
    #[error("llm: {0}")]            Llm(String),
    #[error("reader inference: {0}")] Reader(String),
    #[error("writer policy: {0}")] Writer(String),
    #[error("config: {0}")]         Config(String),
    #[error(transparent)]           Other(#[from] anyhow::Error),
}
pub type Result<T> = std::result::Result<T, SageError>;
```

觀測：

- `tracing` span 名稱：`sage.writer.step`, `sage.reader.plan`, `sage.reader.propagate`, `sage.evolve.round`.
- 事件指標：`triples_added`, `rec`, `pre`, `ded`, `retrieval_latency_ms`, `drift`.
- 可選 exporter：OpenTelemetry (feature `otel`).

---

## 10. 測試策略

| 層級 | 內容 |
|---|---|
| **單元** | `score_entry`, 結構特徵, gating 數值穩定性, GRPO advantage |
| **屬性測試** | `proptest` — 圖更新冪等性、reward 邊界 (0..=1) |
| **快照測試** | `insta` — query plan、subgraph 結果 |
| **整合** | 小型 toy corpus (HotpotQA 子集 20 題) 端到端 |
| **基準** | `criterion` — `ingest`, `query`, `evolve_round` |
| **回放** | 訓練 trajectory bincode 可重新跑分析 |

CI：`cargo test --workspace --all-features` + `clippy -D warnings` + `cargo deny`.

---

## 11. 公開 API 範例

```rust
use sage_runtime::{SageEngine, EvolveCfg};

#[tokio::main]
async fn main() -> sage_memory::Result<()> {
    let cfg = sage_memory::Config::from_file("sage.toml")?;
    let mut engine = SageEngine::from_config(cfg).await?;

    engine.ingest(load_corpus("docs/")?).await?;

    let answer = engine.query(&Query::ask("Who founded the lab that built SAGE?")).await?;
    println!("{}\nEvidence: {:?}", answer.text, answer.evidence);

    engine.evolve(EvolveCfg { rounds: 2, ..Default::default() }).await?;
    Ok(())
}
```

---

## 12. Roadmap / Milestones

| Phase | 範圍 | 驗收 |
|---|---|---|
| **M0** 骨架 (週 1) | crate 拓撲、核心型別、`MemGraphStore`、`LlmClient` trait + Anthropic backend | `cargo test` 通過、ingest 寫入 toy 圖 |
| **M1** Writer v1 (週 2-3) | `LlmWriterPolicy`、reward 計算、無訓練版本 | toy HotpotQA 子集 Recall@5 ≥ baseline RAG |
| **M2** Reader v1 (週 4-5) | Query Planner + Soft Addressing + 1 層 GFM、無 schema 通道 | 端到端 query 工作、latency < 100 ms / query |
| **M3** GFM 完整 (週 6-8) | 多層 GFM、context+schema、contrastive 預訓練腳本 | 在 NQ 子集 Recall@2 ≥ 70 |
| **M4** 自演化 (週 9-10) | GRPO trainer + evolve loop + drift guard | 兩輪後 Recall@5 相對 +5pp |
| **M5** 持久化與生產 (週 11-12) | `SledGraphStore`、HNSW、CLI、OTel | 1M 文件規模壓測通過 |

---

## 13. 開放議題

1. **GFM 預訓練資料** — 自建 / 取現成多圖 corpus？建議先利用 OGB + 自抽 Wiki triples。
2. **Writer 是否微調 LLM 權重** — 第一版以 prompt-level GRPO 控制空間，待證明 reward 信號有效再上 LoRA。
3. **多租戶隔離** — `TenantId` 已進入 §2 / §3 trait 簽章（單租戶用 `TenantId::DEFAULT`）；待解決：跨租戶 schema_bases 是否共享，以及配額/限流策略。
4. **遺忘機制** — 論文未深入；建議引入 `decay(weight, age, hit_count)`，可在 M5 加上。
5. **scatter_add 後端選型** — candle 自寫 vs. 切到 `burn` 的成本/收益尚待 M0 spike 結論（見 §B.3）。

---

*本 SPEC 為 SAGE Rust 實作的合約。任何偏離請以 RFC 形式新增於 `docs/rfcs/`。*

---

## A. 公式對照表 (Formula Traceability)

逐一對照論文 §3 / §4 公式 → SPEC 對應位置 → Rust 符號。所有未在原章節落實的公式於本章補上具體 Rust 簽章。

| 論文公式 | 章節 | SPEC 位置 | Rust 符號 |
|---|---|---|---|
| `𝒢ₕ₊₁ = 𝒢ₕ ⊕ aₕ` | §3 | §4.1 + §A.1 | `GraphStore::apply_action` |
| `(𝒟̂ₖ, 𝒢̂q, Πq) = ℛφ(q,𝒢,M)` | §3 | §5 + §A.1 | `Reader::read -> ReadOutput` |
| `r_ded / r_rec / r_pre / r_ans` | §4.1 | §4.2 | `WriterReward` 欄位 |
| `r_task = (αr_rec + βr_pre + γr_ded)/(α+β+γ)` | §4.1 | §4.2 | `WriterReward::task` |
| `ρ_rep(𝒢)` | §4.1 | §4.2 + §A.2 | `repetition_penalty` |
| `R(τ) = r_task − λ_rep ρ_rep + λ_fmt Σ r_t^fmt` | §4.1 | §4.2 | `WriterReward::trajectory` |
| `sₑ(q) = Σ λᵢ·…` (6 項) | §4.2.2 | §5.2 + §A.3 | `score_entry` + `AddressingWeights` |
| `p₀(e|q) = softmax(sₑ/T₀)` | §4.2.2 | §5.2 + §A.3 | `softmax_entry` |
| `hₑ⁽⁰⁾ = p₀^η Wq·Emb(q) + Wₓ·xₑ` | §4.2.2 | §5.3 + §A.3 | `init_node_state` |
| `ϕ(v), ψ(u,v), r_𝒢` | §4.2.3 | §5.3 + §A.4 | `StructFeatExtractor` |
| `z_uv^(l)` | §4.2.3 | §5.3 + §A.4 | `EdgeContext` |
| `g_uv = 1 + δ·tanh(MLP(z))` | §4.2.3 | §5.3 + §A.4 | `GfmLayer::gate` |
| `m_{u→v}` + node update | §4.2.3 | §5.3 + §A.4 | `GfmLayer::forward` |
| `h̃ₑ⁽⁰⁾ = p_f ⊙ hₑ⁽⁰⁾` | §4.2.4 | §5.3 + §A.5 | `ContextSchemaHead::calibrate` |
| `P_schema = Σ ω_j P_j` | §4.2.4 | §5.3 + §A.5 | `ContextSchemaHead::mix_schema` |
| `H = H_ctx + β_sch·H_sch` | §4.2.4 | §5.3 + §A.5 | `ContextSchemaHead::combine` |
| Proposition 1 (i)(ii)(iii) | §4.3 | §6 + §A.6 | `DriftGuard`, `BudgetCfg` |

### A.1 Writer/Reader 介面細化

> 已回填至 §3 (`GraphStore::apply_action`) 與 §5 (`ReadOutput`)。此處僅保留追溯。

### A.2 Repetition penalty 落實

```rust
// ρ_rep(𝒢) = (|𝒯(𝒢)| − |uniq(𝒯(𝒢))|) / |𝒯(𝒢)|
pub fn repetition_penalty(triples: &[(EntityId, SmolStr, EntityId)]) -> f32 {
    if triples.is_empty() { return 0.0; }
    let total = triples.len() as f32;
    let uniq  = triples.iter().collect::<ahash::AHashSet<_>>().len() as f32;
    (total - uniq) / total
}
```

### A.3 Soft Addressing 完整簽章

```rust
pub struct AddressingWeights {
    pub lambdas: [f32; 6],   // λ₁..λ₆
    pub t0: f32,             // softmax 溫度
    pub eta: f32,            // p₀ 的次方
}

/// 6 項加權 stimulus：Exact / Alias / Cos / Type / Cons / NER-EL
pub fn score_entry(
    e: &Entity, plan: &QueryPlan, w: &AddressingWeights, emb: &dyn Embedder,
) -> f32 {
    let s1 = w.lambdas[0] * exact_match(e, &plan.expansions);
    let s2 = w.lambdas[1] * alias_match(e, &plan.aliases);
    let s3 = w.lambdas[2] * max_cos_probe(e, &plan.probes, emb);
    let s4 = w.lambdas[3] * etype_score(e, plan.etype_hint.as_ref());
    let s5 = w.lambdas[4] * hard_constraint(e, &plan.hard_constraints);
    let s6 = w.lambdas[5] * ner_el_sum(e, plan);
    s1 + s2 + s3 + s4 + s5 + s6
}

pub fn softmax_entry(scores: &[f32], t0: f32) -> Vec<f32> { /* exp(s/T₀) / Σ */ }

// hₑ⁽⁰⁾ = (p₀)^η · Wq · Emb(q) + Wₓ · xₑ
pub fn init_node_state(
    p0: f32, eta: f32,
    wq: &Tensor, emb_q: &Tensor,
    wx: &Tensor, x_e: &Tensor,
) -> candle_core::Result<Tensor> {
    let scale = p0.powf(eta);
    (wq.matmul(emb_q)? * scale as f64)? + wx.matmul(x_e)?
}
```

### A.4 結構特徵 / Gated Propagation

```rust
pub struct NodeStruct  { pub log_deg: f32, pub clustering: f32, pub kcore: f32, pub mean_nbr_deg: f32 }   // ϕ(v)
pub struct EdgePairStruct { pub deg_diff: f32, pub common_nbrs: f32, pub jaccard: f32 }                  // ψ(u,v)
pub struct GraphSummary  { pub mean_phi: Vec<f32>, pub std_phi: Vec<f32>, pub density: f32 }              // r_𝒢

pub struct EdgeContext { pub z: Tensor }   // z_uv^(l) = [E_n(ϕu); E_n(ϕv); E_p(ψ); E_g(r_𝒢)]

pub struct GfmLayer {
    pub e_node: Linear, pub e_pair: Linear, pub e_graph: Linear,
    pub mlp_g:  Mlp,         // gate MLP
    pub w_msg:  Linear,
    pub norm:   LayerNorm,
    pub delta:  f32,         // δ in g = 1 + δ·tanh(MLP(z))
}

impl GfmLayer {
    /// g_uv = 1 + δ·tanh(MLP_g(z_uv))
    pub fn gate(&self, z: &Tensor) -> Result<Tensor> { /* ... */ }

    /// m_{u→v} = η_uv · g_uv ⊙ W_m h_u^{l-1}
    /// h_v^l = LN( h_v^{l-1} + PReLU(b + Σ_u m_{u→v}) )
    pub fn forward(&self, h_prev: &Tensor, graph: &SubgraphView,
                   eta_edge: &Tensor) -> Result<Tensor>;
}
```

### A.5 Context-Schema Head

```rust
pub struct ContextSchemaHead {
    pub p_feature: Tensor,           // p_f (calibration prompt)
    pub schema_bases: Vec<Tensor>,   // {P_j^(l)}，shape: [K, L, D, D]
    pub attn_a: Tensor,              // a^(l) for ω
    pub temp_p: f32,                 // T_p
    pub beta_sch: f32,               // β_sch
}

impl ContextSchemaHead {
    /// h̃ₑ⁽⁰⁾ = p_f ⊙ hₑ⁽⁰⁾
    pub fn calibrate(&self, h0: &Tensor) -> Result<Tensor>;

    /// P_schema^(l) = Σ_j softmax(a^(l)/T_p)_j · P_j^(l)
    pub fn mix_schema(&self, layer: usize) -> Result<Tensor>;

    /// H = H_ctx + β_sch · H_sch
    pub fn combine(&self, h_ctx: &Tensor, h_sch: &Tensor) -> Result<Tensor>;
}
```

張量形狀約定（hidden = D, layers = L, K = prompt_bases_k）：
`p_feature: [D]`、`schema_bases[j]: [L, D, D]`、`attn_a: [L, K]`。

### A.6 Proposition 1 對應實作

```rust
pub struct BudgetCfg {        // (i) signal-budget
    pub topk_d: usize,
    pub topk_e: usize,
    pub min_gate: f32,        // 低於此值的 edge 視為 habituated 丟棄
}

pub struct DriftGuard {       // (iii) evolution stability
    pub threshold: f32,                  // ‖H_t − H_{t-1}‖₂ 上限
    pub rollback_to: Option<SnapshotId>,
}

impl DriftGuard {
    pub fn check(&self, h_prev: &Tensor, h_curr: &Tensor) -> Result<DriftStatus>;
}
```

`(ii) context-schema decomposition` 已由 §A.5 雙通道結構直接體現；evolve loop 每輪僅更新 `H_ctx` 殘差，`schema_bases` 以更小學習率 (`lr_sch = 0.1 * lr_ctx`) 更新。

---

## B. 架構審查筆記

### B.1 `GraphStore` async 介面在訓練熱路徑的瓶頸

每個 GRPO step 對單一 sample 採樣 G 條軌跡，每條軌跡 H 步，每步多次 `neighbors / k_hop / structural_features` 呼叫 → 數量級 `G·H·N` async call。`async fn` 在每次 await 都有 task-poll overhead，且 `Box<dyn Future>` 阻擋 inlining。

**建議**：新增 batched 同步 API，trait 維持 async 為 I/O 邊界，但提供以下批次方法：

```rust
pub trait GraphStoreBatch: GraphStore {
    fn neighbors_batch(&self, ids: &[EntityId], max: usize) -> Vec<Vec<(EntityId, Edge)>>;
    fn k_hop_batch(&self, seeds: &[Vec<EntityId>], hops: u8) -> Vec<Subgraph>;
    fn snapshot_view(&self) -> Arc<GraphSnapshot>;   // 訓練前 freeze
}
```

訓練時先 `snapshot_view()` 取得 `Arc<GraphSnapshot>` (CSR + ndarray)，所有 propagation 在純 CPU/GPU 計算層完成，避免 await。

### B.2 Writer ↔ Reader 循環依賴

目前 `compute_reward` 在 `sage-writer` 內呼叫 `Reader` trait，而 `sage-reader` 又可能在訓練腳本中以 writer 產生的 graph 為輸入。

**解耦方式**：把 `Reader` / `ReadOutput` / `Query` 等 trait 定義**全部上移至 `sage-core`**，`sage-writer` 與 `sage-reader` 各自只依賴 `sage-core`，不互相 use。reward 模組改為 `sage-core::reward` 中性介面，由 runtime 注入具體 reader 實例。

### B.3 PyTorch-scatter 語意於 candle

candle 目前無等價於 `torch_scatter.scatter_add`。GFM message passing 需要 `Σ_{u∈𝒩(v)} m_{u→v}` (edge → node aggregation)。

**建議**：在 `sage-reader::gfm::ops` 自寫 `scatter_add_(dst: &mut Tensor, index: &Tensor, src: &Tensor, dim: usize)`：

- CPU 路徑：`rayon::par_chunks` + `AtomicF32` 或 per-thread local accumulator + reduce。
- CUDA 路徑：以 `candle::CustomOp` 包裝 cub `DeviceSegmentedReduce` 或自寫 atomicAdd kernel。
- 先期可用 `index_add` + dense mask 作 fallback，性能爛但語意正確，用於 M2。

### B.4 自演化的 Checkpoint Manifest

Writer policy / Reader weights / Graph snapshot 三者必須同代際綁定，否則 replay 時會發生 reward 與 retrieval 對不上的「跨代污染」。

```rust
#[derive(Serialize, Deserialize)]
pub struct EvolveManifest {
    pub round: u32,
    pub seed: u64,
    pub writer_ckpt: PathBuf,    // 含 sha256
    pub reader_ckpt: PathBuf,
    pub graph_snap:  SnapshotId,
    pub config_hash: [u8; 32],
    pub created_at:  i64,
}
```

存於 `runs/<run_id>/round_<n>/manifest.json`，evolve loop 每輪原子寫入。

### B.5 多租戶 (TenantId) 提前進 trait

延後加入會是破壞性變更。立刻把 `TenantId(pub u64)` 加進所有 GraphStore / Reader / Writer 方法簽章：

```rust
async fn neighbors(&self, t: TenantId, id: EntityId, max: usize) -> Result<Vec<(EntityId, Edge)>>;
```

單租戶情境用 `TenantId::DEFAULT = TenantId(0)`，零成本。Sled key 前綴 `t{tenant_id:016x}/...`。

### B.6 HNSW 旁路索引的 rebuild 策略

`hnsw_rs` 是 append-only，writer 每輪 upsert 大量 entity (含改名/合併) 會造成索引脹大且品質下降。

**策略**：

1. **增量**：新 entity 直接 `insert`，更新 entity 改 embedding 走「刪除標記 + 重 insert」(`hnsw_rs` 不支援真刪，維持 `tombstone: RoaringBitmap`)。
2. **rebuild trigger**：tombstone ratio > 20% **或** evolve round 結束 → 背景 task 全量重建到 `index.next`，原子 swap (`ArcSwap<Hnsw>`).
3. **影子查詢**：rebuild 期間 query 仍走 `index.current`，無 downtime。

---

## C. 額外規範

### C.1 確定性與隨機性

所有隨機路徑必須吃同一個 `rngs::StdRng`，禁止 `thread_rng()`：

```rust
pub struct SageRng { inner: rand::rngs::StdRng }
impl SageRng {
    pub fn from_seed(seed: u64) -> Self { Self { inner: StdRng::seed_from_u64(seed) } }
    pub fn fork(&mut self, label: &str) -> Self { /* hash(seed, label) */ }
}
```

GRPO 採樣、graph augmentation (edge drop / feature mask)、HNSW ef search 抖動皆從 `engine.rng.fork("grpo")` 等獨立子串流抽取，避免相互干擾。`EvolveManifest.seed` 完整記錄。

### C.2 Schema 版本管理

```rust
pub const ENTITY_SCHEMA_V: u16 = 1;
pub const EDGE_SCHEMA_V:   u16 = 1;

#[derive(Serialize, Deserialize)]
pub struct Entity {
    #[serde(default = "Entity::default_schema")]
    pub schema_version: u16,
    /* ...原欄位... */
}

pub fn migrate_entity(raw: &[u8]) -> Result<Entity> {
    let v: u16 = bincode::deserialize(&raw[..2])?;
    match v {
        0 => migrations::entity_v0_to_v1(raw),
        1 => bincode::deserialize(raw).map_err(Into::into),
        n => Err(SageError::Config(format!("unknown entity schema v{n}"))),
    }
}
```

`Edge` 同理。Sled value 前 2 byte 永遠是 `schema_version` little-endian。

### C.3 Feature flags

`Cargo.toml`：

```toml
[features]
default      = ["sled", "candle", "anthropic"]
sled         = ["dep:sled"]
rocksdb      = ["dep:rocksdb"]
candle       = ["dep:candle-core", "dep:candle-nn"]
burn         = ["dep:burn"]
anthropic    = ["dep:reqwest", "sage-llm/anthropic"]
openai       = ["sage-llm/openai"]
local-llama  = ["sage-llm/llama-cpp"]
bge          = ["sage-embed/bge"]
e5           = ["sage-embed/e5"]
hnsw         = ["dep:hnsw_rs"]
usearch      = ["dep:usearch"]
otel         = ["dep:opentelemetry", "dep:tracing-opentelemetry"]
```

CI matrix 必跑：`--no-default-features`、`default`、`--all-features`。

### C.4 Benchmark 目標與硬體基線

硬體基線：**AMD EPYC 7763 64C / 256 GB DDR4 / NVIDIA L4 24 GB / NVMe Gen4**。

| 指標 | 目標 | 條件 |
|---|---|---|
| Ingest throughput | ≥ 120 doc/s | doc 平均 800 token，含 LLM triple 抽取（Claude Haiku 旁路）|
| Ingest CPU-only | ≥ 800 doc/s | 跳過 LLM、用快取 triples |
| Query p50 | ≤ 35 ms | 1M entities / 5M edges, k=5, GFM 3 層 |
| Query p99 | ≤ 120 ms | 同上 |
| Evolve round wall time | ≤ 25 min | 50k samples, G=4 trajectories, H=8 steps |
| Drift guard 開銷 | ≤ 3% wall | 計算 ‖ΔH‖₂ |
| HNSW rebuild (1M) | ≤ 90 s | L4 + 16 threads |

`criterion` 報告必含 throughput plot 與 p50/p99 直方圖，PR 退步 > 5% 直接 block。

### C.5 安全 — LLM 抽取 triple 的 Sanitizer

LLM 輸出在進入 `GraphStore::upsert_*` 前**必經** `TripleSanitizer`：

```rust
pub struct SanitizerCfg {
    pub max_name_len:       usize,           // 預設 96
    pub max_desc_len:       usize,           // 預設 1024
    pub max_triples_per_doc: usize,          // 預設 64
    pub relation_vocab:     ahash::AHashSet<SmolStr>,  // 白名單
    pub entity_name_re:     regex::Regex,    // 預設 ^[\p{L}\p{N}\s\-_.,/()'’]+$
    pub blocklist:          aho_corasick::AhoCorasick, // 機敏字串
}

pub fn sanitize(raw: RawTriple, cfg: &SanitizerCfg) -> Result<Option<Triple>, RejectReason>;
```

拒絕原因 (`RejectReason`) 計入 `tracing` metric `sage.writer.sanitizer.reject{reason=...}`，超過閾值自動降級該批次（不寫入 + 寫入 quarantine 表）。Relation 詞表預設為論文常見 30 條 + 使用者擴充，未知 relation 走 `relation = "related_to"` fallback 並打標 `coerced = true`。

---

*Audit pass 1 by sub-agent — 2026-05-25*
