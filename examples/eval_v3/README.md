# Eval v3 — Broad Effectiveness Test (100 docs, multi-hop)

100 documents × 4 topic clusters × mixed lengths.
40 queries × 5 tiers × including multi-hop.

This is the **honest scale stress test**. Smaller sets (`../eval_dataset/`,
`../eval_v2/`) hide problems. v3 reveals them.

## Files

| File | Purpose |
|---|---|
| `docs.jsonl` | 100 docs across tech (1xxx), science (2xxx), history (3xxx), culture (4xxx) |
| `queries.json` | 40 queries with `tier` (1-5) + `kind` + `ground_truth` |
| `baselines.json` | live-measured numbers + key findings + regression thresholds |

## Doc ID schema
- `1001-1025` tech founders / language creators / web protocols
- `2001-2025` scientists (physics, chemistry, biology, medicine)
- `3001-3025` historic figures / political leaders / explorers
- `4001-4025` writers / painters / composers / filmmakers

## Tier rubric
| Tier | Kind | Example |
|---|---|---|
| 1 | exact entity | "Who created Linux" |
| 2 | multi-token surface | "Who designed Java at Sun Microsystems" |
| 3 | descriptive (no surface anchor) | "Who proposed black holes emit radiation" |
| 4 | paraphrase / cross-domain | "Liberator of South Africa from apartheid rule" |
| 5 | **multi-hop** (≥2 truth docs) | "Two Nobel Peace Prize winners who led liberation movements" |

## How to run

```bash
export SAGE_CLAUDE_BIN="$(which claude)"  # Windows: claude.cmd

# Ingest (≈ 20 min on real Claude — 100 docs × ~12s each)
cargo run -p sage-cli -- ingest-batch \
    --db /tmp/sage_eval_v3.sled \
    --jsonl examples/eval_v3/docs.jsonl
# expected: 100 docs / ~440 entities / ~380 edges / 0 failures

# Overall eval
cargo run -p sage-cli -- eval --db /tmp/sage_eval_v3.sled --k 3 \
    < examples/eval_v3/queries.json

# Per-tier (note: Windows + Chinese locale needs explicit utf-8)
for tier in 1 2 3 4 5; do
  echo "=== TIER $tier ==="
  python -c "import json,sys; \
    d=json.load(open('examples/eval_v3/queries.json',encoding='utf-8')); \
    sys.stdout.reconfigure(encoding='utf-8'); \
    print(json.dumps([q for q in d if q['tier']==$tier]))" \
    | cargo run -q -p sage-cli -- eval --db /tmp/sage_eval_v3.sled --k 3
done
```

## Measured baseline (2026-05-26, 100 docs / 439 entities indexed)

### Overall
| Metric | k=1 | k=3 | k=5 |
|---|---|---|---|
| Recall@k | 0.25 | **0.30** | 0.46 |
| MRR | 0.25 | 0.26 | 0.36 |
| Precision@k | 0.25 | 0.10 | 0.095 |
| F1@k | 0.25 | 0.15 | 0.16 |

### Per-tier @ k=3
| Tier | Pattern | Recall@3 | MRR |
|---|---|---|---|
| 1 | exact (10) | **0.40** | 0.33 |
| 2 | multi-token (10) | **0.20** | 0.13 |
| 3 | descriptive (10) | 0.30 | 0.30 |
| 4 | paraphrase (5) | 0.40 | 0.30 |
| 5 | **multi-hop** (5) | **0.00** | 0.00 |

## Key findings (the honest story)

1. **Scale hurts hard.** Overall Recall@3 dropped from **0.55 (eval_v2, 30 docs)** to
   **0.30 (eval_v3, 100 docs)**. Cause: softmax dilution — more entities competing
   for prior mass means the right one gets less weight.

2. **Multi-hop is total failure.** Tier-5 Recall@3 = **0.00**. HeuristicReader scores
   per-entity, pools per-doc, with no joint reasoning across docs. This is
   exactly the gap that trained GFM is meant to close (paper §4.2.3 — structurally-
   conditioned propagation across the graph).

3. **Multi-token entity names regress hardest.** Tier-2 = 0.20 vs Tier-1 = 0.40.
   `DeterministicEmbedder` (hash bag-of-words) treats each token independently —
   "Sun Microsystems" splits into hash-bucket noise. Real semantic embedder
   (BGE-M3) would directly fix this.

4. **Even Tier-1 exact match drops below 50%** at this scale. The system can
   match "Linux" but loses ranking against the dozens of other docs whose
   entities also include common words.

## What this means for "is SAGE v0.1.0 ready?"

**For toy / smoke validation:** yes (eval_dataset 87.5% Recall@3, eval_v2 0.55).

**For 100+ doc real corpora:** **no, not yet.** Three known fixes — all multi-day:
- M3 BGE/E5 embedder → likely doubles Tier-2/3 performance
- M3 GFM training → enables Tier-5 multi-hop
- Tuned AddressingWeights via gradient → mitigates softmax dilution

Until those land, v0.1.0 is a **plumbing baseline** suitable for:
- Architectural research / iteration
- < 50-doc personal knowledge bases
- Smoke-testing changes to the SAGE pipeline itself

## Regression thresholds

`baselines.json.regression_thresholds`:
```json
{ "overall_recall_at_3_min": 0.20,
  "overall_mrr_min":         0.15,
  "tier_1_recall_at_3_min":  0.30 }
```

Floor; below = something broke. Note these are below v0.1.0's measured
baselines on purpose — Claude's triple extraction is non-deterministic, so
the actual numbers will vary ±0.05 per re-ingest. Floors give wiggle room.

## What this set does **not** test

- Long documents (every doc here is 1-3 sentences). Real-world docs are paragraphs.
- Negative queries (every query has a truth doc). Robustness to "no answer" untested.
- Adversarial / poisoned data.
- Multilingual (English only).
- Streaming / incremental ingest.
- > 1000-doc scale (sled scaling, HNSW rebuild cost).

Build a focused dataset for each missing dimension rather than expanding this one.
