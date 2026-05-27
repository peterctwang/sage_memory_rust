# eval_patents/

> SAGE-ingestible JSONL exported from **patent-ai**'s `data/cache/` corpus
> (3225 Google Patents HTML pages, zstd-compressed).

## Purpose
Real-world stress test: production patent data — long, dense, full of
co-mentioned entities (inventors, assignees, CPC codes, citations) — fed
through the writer pipeline to verify multi-hop / bridge logic at
realistic scale.

Bridges two repos:
- **Source**: `C:/Users/User/Desktop/專利系統RUST/patent-ai/data/cache/*.zst`
- **Consumer**: `sage ingest-batch --jsonl examples/eval_patents/docs.jsonl`

## Pipeline
```
patent-ai/data/cache/*.zst                  3225 zst-compressed HTML pages
        ↓ zstd decompress
        ↓ regex extract (title / abstract / inventors / assignees / cpc / claim1)
        ↓ scripts/export_patent_cache_to_sage_jsonl.py
docs.jsonl                                  one JSONL row per patent
        ↓
patents.map.jsonl                           sidecar: doc_id → publication_number
```

Each emitted row is shaped like the v7 paragraph corpus:
```json
{"doc_id": 9489981148023508,
 "text": "US20240154912A1: Traffic identification using machine learning. by
          Gaurang NAIK, Sai Yiu Duncan Ho, George Cherian, ... Assigned to
          Qualcomm Inc. CPC classes: H04L47/2441, ... Abstract: ... First
          claim: ..."}
```

doc_id = sha256(publication_number)[:8] big-endian u64, deterministic
across re-exports.

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | Full export (~2500-3000 patents after thin-abstract filter) |
| `docs.preview.jsonl` | data | 50-cache sample for fast smoke tests |
| `patents.map.jsonl` | sidecar | doc_id ↔ publication_number ↔ title ↔ cache_file |
| `queries.json` | data | (TBD) targeted multi-hop / bridge queries |
| `baselines.json` | data | (TBD) measured Recall/MRR per backend |

## How to regenerate
```bash
python scripts/export_patent_cache_to_sage_jsonl.py \
  --cache "C:/Users/User/Desktop/專利系統RUST/patent-ai/data/cache" \
  --out   examples/eval_patents/docs.jsonl \
  --map   examples/eval_patents/patents.map.jsonl
```

Optional flags:
- `--limit N`             : process at most N cache entries
- `--min-abstract-chars X`: skip patents whose abstract is shorter than X
- `--max-text-chars X`    : truncate each paragraph to X chars (default 1500)

## How to ingest into SAGE
```bash
# Router (best quality on paragraph corpus per eval_v7):
SAGE_CLAUDE_BIN="C:/Users/User/AppData/Roaming/npm/claude.cmd" \
SAGE_CODEX_BIN="C:/Users/User/AppData/Roaming/npm/codex.cmd" \
SAGE_ROUTER_LIGHT_LLM=minimax \
SAGE_ROUTER_DEEP_LLM=codex-cli \
sage ingest-batch --db /tmp/sage_patents.sled \
                  --jsonl examples/eval_patents/docs.jsonl \
                  --tenant 1 --llm router

# Then query / stats / eval as usual:
sage query --db /tmp/sage_patents.sled --tenant 1 "Inventors at Qualcomm"
sage stats --db /tmp/sage_patents.sled --tenant 1
```

## Why no full-text or description?
patent-ai stores the full HTML so it could re-extract. We deliberately
drop the description body (often 20k+ tokens) because:
- SAGE's writer policy caps each call at ~512 visible output tokens.
- Multi-hop retrieval depends on canonical-entity coverage, not text bulk.
- Title + abstract + first claim already names every entity we want
  addressable (inventors / assignees / CPC / technology nouns).

If a downstream task needs the description body, look up the patent in
`patents.map.jsonl` and re-decompress the original cache file.

## Invariants
- `doc_id` is deterministic: sha256(publication_number)[:8] as u64.
- Identical patent appearing in two cache files → first one wins, no
  doc_id collision.
- Cache files that don't parse (missing `<meta name="DC.title">`) are
  silently skipped and counted in the script's summary.

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 來源系統：`peterctwang/patent-ai` (separate repo)
- 對照：[`../eval_v7/`](../eval_v7/) — synthetic 100-doc paragraph benchmark

## Last Updated
2026-05-27 — exporter script + 50-patent preview.
