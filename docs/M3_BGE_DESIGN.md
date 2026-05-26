# M3 — Real Embedder Integration (BGE / E5)

**Status**: Deferred. Reason: genuine multi-day work; cannot land "stable +
tests passing" in a continuous session. This doc is the handoff so the next
implementer can pick up cleanly.

**Trait owner**: `sage_core::Embedder` (already in main).
**Drop-in slot**: `sage-embed::DeterministicEmbedder` — anything implementing
`Embedder` is a drop-in replacement; no caller needs to change.

---

## Why this is multi-day

| Step | Effort | Notes |
|---|---|---|
| Pick runtime (ort vs candle) | 0.5 day | both have tradeoffs (see below) |
| Download + bundle model weights | 0.5 day | 110–400 MB depending on model; LFS or download-on-first-run |
| Tokenizer integration | 1 day | HuggingFace `tokenizers` crate; vocab + special tokens |
| Pre-processing pipeline (truncate / pad / attention mask) | 0.5 day | matches model's training input |
| Pooling strategy (mean / CLS / last-token) | 0.5 day | spec-correct per model card |
| L2 normalization | trivial | already do this in `DeterministicEmbedder` |
| Batching for performance | 1 day | minimum to be usable on real corpora |
| GPU optional path | 0.5 day | feature-gated CUDA; CPU fallback default |
| Tests with a tiny model + golden vectors | 1 day | regression guard |

**Total**: ~5–6 working days for someone familiar with the runtime.

---

## Decision matrix: runtime choice

| Criterion | `ort` (ONNX Runtime) | `candle` |
|---|---|---|
| Binary size impact | ~50 MB | ~30 MB |
| BGE-M3 / E5 ONNX availability | ✅ official + community | ⚠️ needs safetensors conversion |
| GPU support | ✅ CUDA / CoreML / DirectML | ✅ CUDA / Metal |
| Tokenizer | bring-your-own (`tokenizers` crate) | bring-your-own |
| Builds on Windows? | ✅ (cmake quirks but works) | ✅ |
| Maintenance | Microsoft-backed, stable | HuggingFace-backed, fast-moving |
| Lines of code to integrate | ~300 | ~500 |

**Recommendation**: start with `ort` for production embedders. Reserve
`candle` for the GFM neural net (where ONNX export is painful). They can
coexist behind separate feature flags.

---

## Model menu

| Model | Dim | Size | Strength |
|---|---|---|---|
| `BAAI/bge-small-en-v1.5` | 384 | 130 MB | fast, English |
| `BAAI/bge-base-en-v1.5` | 768 | 440 MB | mid-quality English |
| `BAAI/bge-m3` | 1024 | 2.3 GB | multilingual + long context |
| `intfloat/e5-small-v2` | 384 | 130 MB | fast, instruction-tuned |
| `intfloat/multilingual-e5-base` | 768 | 1.1 GB | multilingual |

**Recommendation**: ship `bge-small-en-v1.5` by default — best size/quality
ratio. Make model selection a config option.

---

## Proposed crate layout

```
crates/sage-embed/
├── Cargo.toml                       # adds features: ort, bge, e5
└── src/
    ├── lib.rs                       # re-exports
    ├── deterministic.rs             # already done
    ├── hnsw_index.rs                # already done
    ├── onnx/                        # NEW
    │   ├── mod.rs                   # OnnxEmbedder trait shim
    │   ├── session.rs               # ort::Session wrapper, batching
    │   ├── tokenizer.rs             # HF tokenizers loader
    │   ├── bge.rs                   # BgeEmbedder — pool + L2 norm
    │   └── e5.rs                    # E5Embedder — instruction prefix + pool
    └── download.rs                  # NEW — optional first-run model download
```

Cargo features:

```toml
[features]
default     = ["hnsw"]
hnsw        = ["dep:hnsw_rs"]
onnx        = ["dep:ort", "dep:tokenizers", "dep:ndarray"]
bge         = ["onnx"]
e5          = ["onnx"]
auto-download = ["dep:reqwest"]
```

---

## Proposed API

```rust
pub struct BgeEmbedder {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
    max_len: usize,
    pool: Pooling,  // Mean | Cls | LastToken
}

impl BgeEmbedder {
    pub async fn load(model_path: impl AsRef<Path>) -> Result<Self>;
    pub async fn load_named(name: &str) -> Result<Self>;  // with auto-download
    pub fn with_max_len(mut self, n: usize) -> Self;
}

#[async_trait]
impl Embedder for BgeEmbedder {
    fn dim(&self) -> usize { /* from session output shape */ }
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Arc<[f32]>>> {
        // 1. tokenize batch (returns input_ids + attention_mask)
        // 2. pad to max in batch
        // 3. ort run → hidden states
        // 4. pool with attention mask
        // 5. L2 normalize each row
        // 6. wrap into Arc<[f32]>
    }
}
```

---

## Tests required

1. **Golden vector test**: small fixed input → expected embedding (within ε)
   against a Python reference. Pin model version in test.
2. **Batch == singleton**: `embed([a, b])` == `[embed([a]), embed([b])]`
3. **Tokenizer round-trip**: encode/decode preserves text up to whitespace
4. **Empty string is zero vector** (matches `DeterministicEmbedder` contract)
5. **Unit norm**: L2 norm of every output is 1.0 ± 1e-5

---

## Wiring into existing pipeline (zero downstream changes)

```rust
// crates/sage-cli/src/main.rs  — run_query
let embedder: Arc<dyn Embedder> = if cfg!(feature = "bge") {
    Arc::new(BgeEmbedder::load_named("bge-small-en-v1.5").await?)
} else {
    Arc::new(DeterministicEmbedder::new(384))
};
```

`HeuristicReader::with_embedder`, `apply_action_embedded`, `SageEngine::with_embedder`,
and `HnswIndex` all accept `Arc<dyn Embedder>` — none need to change.

The only dim-related consideration: HNSW index dim must match embedder dim.
Recommend reading `embedder.dim()` and passing to `HnswIndex::new(...)` at
runtime rather than hard-coding 128 in `sage-cli`.

---

## Open questions for the implementer

1. **Where to host model files?** GitHub releases (200MB per binary), git LFS
   (cheap but flaky), or download-on-first-run from HF Hub (network dep)?
2. **CPU-only default?** Many SAGE users won't have a GPU. Bench CPU
   inference on a typical eval workload (the 8-doc fixture) and see if it's
   tolerable (<200 ms/doc).
3. **Tokenizer caching?** HF tokenizers are slow to construct. Hold one per
   `BgeEmbedder` instance and reuse.
4. **Should `Embedder::embed` move to `&[String]` for owned input?** Current
   `&[&str]` forces callers to manage lifetimes when batching.

---

## Acceptance criteria for "M3 done"

- [ ] `cargo run --features bge -p sage-cli -- ingest-batch \
       --db /tmp/g.sled --jsonl examples/eval_dataset/docs.jsonl` works
- [ ] `cargo run --features bge -p sage-cli -- eval --db /tmp/g.sled --k 3` ≥
      M2 baseline (Recall@3 = 0.875 with DeterministicEmbedder) — i.e., a real
      embedder should match or beat the hash baseline.
- [ ] 5 unit tests above all pass.
- [ ] `cargo test --workspace --features bge` green.
- [ ] CI on GitHub Actions stays green (model file handling matters).
- [ ] README quick-start updated to show `--features bge`.

---

*Authored: 2026-05-26. Deferred per §7.1 (multi-week scope honesty).
The architecture is intentionally laid out so this slot is a drop-in.*
