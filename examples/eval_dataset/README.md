# Synthetic Eval Dataset

8 hand-crafted documents about famous inventors/engineers, paired with
8 single-doc-truth queries. Use this to sanity-check the SAGE retrieval
pipeline end-to-end and measure baseline Recall@k / MRR.

## Files

| File | Purpose |
|---|---|
| `docs.jsonl` | 8 ingestable documents, one JSON per line |
| `queries.json` | 8 queries with `ground_truth: [doc_id]` per query |

## How to run

```bash
# 1. Ingest the dataset via real Claude into a sled-backed graph
export SAGE_CLAUDE_BIN="$(which claude)"   # Windows: claude.cmd
cargo run -p sage-cli -- ingest-batch \
    --db /tmp/sage_eval.sled \
    --jsonl examples/eval_dataset/docs.jsonl

# 2. Run the eval harness
cargo run -p sage-cli -- eval --db /tmp/sage_eval.sled --k 3 \
    < examples/eval_dataset/queries.json
```

Expected on a clean baseline (HeuristicReader + DeterministicEmbedder, 128 dim):

```json
{
  "samples":        8,
  "k":              3,
  "recall_at_k":    1.0,   // every truth doc appears in top 3
  "precision_at_k": 0.33,  // 1/3 by definition (one truth per query × k=3)
  "f1_at_k":        0.5,
  "mrr":            1.0
}
```

Exact numbers depend on what triples Claude extracts per document. The
ground-truth schema rewards retrieving *the right document*, not the right
triples — so as long as entity names like "Linus Torvalds" or "Apollo"
end up in the right doc's `source_docs` list, retrieval lands at the
correct top hit.

## What this dataset does **not** measure

- Multi-hop reasoning across documents (every query is single-doc).
- Temporal consistency.
- Performance under noisy/poisoned data.

For those you need HotpotQA / MuSiQue / 2WikiMultiHopQA proper. This
fixture is a smoke-grade baseline you can run in seconds.

## Reproducibility

`DeterministicEmbedder` is seedable; Claude responses are not. Re-running
ingest gives slightly different triples each pass. Two consecutive runs
typically agree on top-1 doc but may disagree on entity ordering — that's
expected behavior, not a bug.
