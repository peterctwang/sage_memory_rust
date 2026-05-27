# eval_v7/

> 100-doc **multi-hop stress benchmark** — designed specifically to expose
> whether the writer can extract a complete knowledge graph from
> paragraph-level docs containing multiple co-mentioned entities.

## Purpose
v3-v6 used single-fact docs. v7 uses **paragraph docs (2-4 sentences,
3-6 entities each)** so the writer's coverage rule + apply-layer dedup
fix are stressed in the way real Wikipedia content would stress them.

The corpus is intentionally interconnected:
- Doc 1001 names Gates / Ballmer / Nadella — `Microsoft` entity must
  link to it from THREE different angles.
- Doc 1015 names DeepMind founders AND Google acquisition AND AlphaGo
  win — `Google` entity overlaps with cluster 1xxx.
- Doc 4014 names Nolan AND Oppenheimer (film) AND Manhattan Project,
  bridging cluster 4xxx (film) and 2xxx (science).

## Contents
| 路徑 | 種類 | 用途 |
|---|---|---|
| `docs.jsonl` | data | 100 paragraph-style docs across 7 clusters |
| `queries.json` | data | 45 queries weighted to multi-hop (21 of 45 = 47%) |
| `baselines.json` | data | live-measured Claude / Codex / Gemini comparison |

## Doc clusters
| Range  | Domain      | Count | Cross-cluster edges                          |
|--------|-------------|-------|----------------------------------------------|
| 1001-1025 | Tech/CS    | 25 | OpenAI co-founders → Tesla; DeepMind → Google |
| 2001-2020 | Science    | 20 | Pauling = Chemistry+Peace Nobel; Oppenheimer in tech docs |
| 3001-3015 | Politics   | 15 | Obama→Biden→Trump succession; Churchill→Roosevelt allies |
| 4001-4015 | Arts/Film  | 15 | Nolan/Oppenheimer bridges to 2xxx           |
| 5001-5010 | Sports     | 10 | NBA dynasties span multiple players          |
| 6001-6005 | Literature | 5  | Plato→Aristotle→Alexander bridges to 3xxx   |
| 7001-7010 | Business   | 10 | Buffett/Munger pair; Sony founders pair      |

## Query tiers
| Tier | Kind        | Count | What it tests                                          |
|------|-------------|-------|--------------------------------------------------------|
| 1    | exact       | 6  | Surface token → single-doc retrieval                   |
| 2    | multi-token | 6  | Phrase coverage with multi-entity context              |
| 3    | descriptive | 6  | Paraphrased role/identity (no surface overlap)         |
| 4    | paraphrase  | 6  | Metaphorical labels ("father of X")                    |
| 5    | multi-hop   | 15 | "Two X" / "Three X" — needs ALL co-mentioned entities  |
| 6    | bridge      | 6  | Chain reasoning: X → relation → answer                 |

Tier 5 + Tier 6 = 21 queries = 47% of the benchmark. This is the
benchmark to look at when judging multi-hop quality.

## Invariants
- All `ground_truth` doc_ids exist in `docs.jsonl`.
- Each multi-hop query's GT doc(s) literally contain ALL the named
  entities the answer requires — there are no "ghost" requirements.
- Tier 6 (bridge) queries require entity-to-entity traversal: the
  query mentions one entity, the answer requires identifying the
  related entity inside the GT doc.

## Tests
無自動化；端到端：`sage ingest-batch` → `sage eval`。

## Backends compared
1. **Claude CLI** (`--llm claude-cli`) — baseline (Opus, expensive)
2. **Codex CLI** (`--llm codex-cli`) — OpenAI subscription
3. **Gemini CLI** (`--llm gemini-cli`) — Google OAuth (gemini-2.5-pro)

MiniMax intentionally excluded from this run — its quota model
(1500 tokens / 5h on Starter) is too small for 100-doc runs.

## Related
- 上層：[`../CLAUDE.md`](../CLAUDE.md)
- 對照規模：v3 (100 single-fact) / **v7 (100 paragraph)**

## Last Updated
2026-05-27 — multi-hop benchmark seed.
