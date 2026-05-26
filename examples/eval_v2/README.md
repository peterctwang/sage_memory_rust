# Eval v2 — Layered Effectiveness Test Set

A real benchmark for SAGE v0.1.0 effectiveness testing. 30 documents,
20 queries across 4 difficulty tiers, with measured baselines and
regression thresholds.

This is **not** a smoke test (see `../eval_dataset/` for that — 8 docs).
This set is large enough to expose retrieval weaknesses without taking
forever to ingest.

## What's here

| File | Purpose |
|---|---|
| `docs.jsonl` | 30 docs across 5 topic clusters (tech founders, scientists, inventors, mathematicians, computer scientists) |
| `queries.json` | 20 queries with `tier` + `kind` annotations + `ground_truth` |
| `baselines.json` | measured baselines from a real run (regression thresholds included) |

### Doc ID schema

The first digit groups topic clusters — useful for reading entity collisions:

- `1xx` tech founders / language creators
- `2xx` scientists (physics, chemistry, biology)
- `3xx` classical inventors
- `4xx` mathematicians + computer-science foundations
- `5xx` modern computer-science pioneers

### Query tier rubric

| Tier | Pattern | Example | Why this exists |
|---|---|---|---|
| 1 | **Exact entity** | "Who created Linux" | sanity floor — must work |
| 2 | **Multi-token** | "Who released the Python programming language" | tests soft addressing + cos similarity for partial-anchor entities |
| 3 | **Descriptive** | "Who imaged the DNA double helix" | tests cross-token semantic match without surface anchor |
| 4 | **Cross-domain** | "What protocols connect today's Internet" | tests retrieval when query phrasing diverges hardest from doc text |

`kind` field annotates the secondary characteristic (alias / abbrev /
paraphrase / etc.) for finer error analysis.

## How to run

```bash
# 1. Ingest (≈ 30 × 15s with real Claude ≈ 7 min)
export SAGE_CLAUDE_BIN="$(which claude)"   # Windows: claude.cmd
cargo run -p sage-cli -- ingest-batch \
    --db /tmp/sage_eval_v2.sled \
    --jsonl examples/eval_v2/docs.jsonl

# 2. Overall eval
cargo run -p sage-cli -- eval --db /tmp/sage_eval_v2.sled --k 3 \
    < examples/eval_v2/queries.json

# 3. Per-tier breakdown (POSIX shell + python)
for tier in 1 2 3 4; do
  echo "=== tier $tier ==="
  python -c "import json; d=json.load(open('examples/eval_v2/queries.json')); print(json.dumps([q for q in d if q['tier']==$tier]))" \
    | cargo run -q -p sage-cli -- eval --db /tmp/sage_eval_v2.sled --k 3
done
```

## Measured baseline (2026-05-26)

Pipeline: `HeuristicReader + DeterministicEmbedder(128) + HnswIndex`,
ingest via real `claude` binary.

| Metric | k=1 | k=3 | k=5 |
|---|---|---|---|
| Recall@k | 0.50 | **0.55** | 0.65 |
| MRR | 0.50 | 0.48 | 0.54 |
| Precision@k | 0.50 | 0.18 | 0.13 |
| F1@k | 0.50 | 0.275 | 0.22 |

Per-tier at k=3:

| Tier | Pattern | Recall@3 | MRR |
|---|---|---|---|
| 1 | exact | **0.60** | 0.47 |
| 2 | multi-token | 0.60 | 0.60 |
| 3 | descriptive | 0.60 | 0.60 |
| 4 | cross-domain | **0.40** | 0.40 |

### How to read the numbers

- **Recall@3 ≈ 0.55 overall** is the headline. Half-plus of queries land
  the truth doc in top-3.
- **Tier-4 drops to 0.40** — descriptive queries without surface anchors
  are the system's weak point. Expected: this is what trained embedder (M3)
  and trained GFM (M4) should fix.
- **Tier-1 MRR 0.47 < Tier-2/3 MRR 0.60** is counterintuitive: exact-match
  queries do worse on MRR than multi-token / descriptive. Reason: when an
  exact-token like "Linux" appears in multiple entities (e.g. linked via
  `created` edge → both "Linus Torvalds" AND "Linux" get high scores),
  the truth doc may rank #2 instead of #1. Trained GFM should fix this by
  learning relationship-aware ranking.

## Regression thresholds

`baselines.json` records *minimum* values for CI / human review:

```json
"regression_thresholds": {
  "overall_recall_at_3_min": 0.45,
  "overall_mrr_min":         0.40,
  "tier_4_recall_at_3_min":  0.25
}
```

A future change that drops below any of these — **something broke**.
Likely culprits if you see a regression:
- Embedder dim changed but HnswIndex wasn't rebuilt
- Soft-addressing `λ_cos` weight dropped
- Entity name-to-id hash collision started biting
- Claude triple extraction format drifted (check `failures: []` in ingest output)

## What this test set does NOT test

- **Multi-doc reasoning** (every truth is single-doc). Use HotpotQA proper for that.
- **Negative queries** (no answer in corpus). All queries here have a known truth doc.
- **Adversarial / poisoned data** ingestion robustness.
- **Long-context** (every doc is 1 sentence). Real-world docs are paragraphs.
- **Multilingual** (English only).

For each missing dimension, build a focused dataset rather than expanding this one.

## Reproducibility

- `DeterministicEmbedder` is seedable → deterministic
- `HnswIndex` cosine search is deterministic given same insert order
- Claude triple extraction is **not** deterministic → ±0.05 drift per re-ingest
  is normal. The structural baselines should hold; per-query rank can flip.
