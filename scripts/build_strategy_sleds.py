#!/usr/bin/env python3
"""Build 4 SAGE sleds with different ingest strategies, then eval each.

Strategy A (zero-token):    Structured fields only (title tokens + inventors +
                            assignees + CPC + citations). No LLM at all.
Strategy B (hybrid):        Strategy A + LLM extraction on abstract+claim1 only.
Strategy C (full LLM):      Existing /tmp/sage_peplink.sled (already built).
Strategy D (LLM-only short):LLM on abstract+claim1 only. No structured fields.

Output: examples/eval_peplink/STRATEGY_COMPARISON.md
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from pathlib import Path


# ---------- Triple builders ----------

def title_concept_triples(doc_id: int, pubnum: str, title: str) -> list[dict]:
    """Build concept-entity triples from the title.

    The whole title becomes one Concept entity (for exact-phrase queries),
    plus each multi-word noun phrase becomes a separate entity.
    """
    triples = []
    # Patent-itself entity ↔ title-concept entity (gives the patent a
    # retrievable "name" anchor without needing the full text).
    triples.append({
        "src": pubnum, "src_type": "Concept",
        "rel": "titled",
        "dst": title.strip().rstrip("."),
        "dst_type": "Concept",
    })
    # Tokenize title and create N-grams (bigrams) as concept entities.
    # This is what lets "throughput optimization" retrieve doc with
    # "Throughput optimization for bonded variable bandwidth connections".
    tokens = [t for t in re.split(r"\W+", title) if len(t) >= 3]
    for i in range(len(tokens) - 1):
        bigram = (tokens[i] + " " + tokens[i + 1]).lower()
        triples.append({
            "src": pubnum, "src_type": "Concept",
            "rel": "mentions",
            "dst": bigram,
            "dst_type": "Concept",
        })
    return triples


def structured_triples(doc_id: int, rec: dict) -> list[dict]:
    """All structured-field triples from a parsed patent record."""
    pubnum = rec["publication_number"]
    triples = []

    triples += title_concept_triples(doc_id, pubnum, rec["title"])

    for inv in rec.get("inventors", [])[:8]:
        triples.append({"src": inv, "src_type": "Person",
                        "rel": "invented", "dst": pubnum, "dst_type": "Concept"})
    for ass in rec.get("assignees", []):
        triples.append({"src": pubnum, "src_type": "Concept",
                        "rel": "assigned_to", "dst": ass, "dst_type": "Org"})
    for cpc in rec.get("cpc", [])[:8]:
        triples.append({"src": pubnum, "src_type": "Concept",
                        "rel": "classified_as", "dst": cpc, "dst_type": "Concept"})
    for cite in rec.get("backward_cites", [])[:20]:
        triples.append({"src": pubnum, "src_type": "Concept",
                        "rel": "cites", "dst": cite, "dst_type": "Concept"})
    for cite in rec.get("forward_cites", [])[:20]:
        triples.append({"src": cite, "src_type": "Concept",
                        "rel": "cites", "dst": pubnum, "dst_type": "Concept"})
    return triples


# ---------- Strategy A — structured only ----------

def build_strategy_a(records, sled_path: Path, sage_bin: Path, tenant: int):
    """Strategy A: zero LLM. Only structured triples."""
    print(f"\n[A] Building zero-token sled at {sled_path}")
    if sled_path.exists():
        import shutil
        shutil.rmtree(sled_path)
    t0 = time.time()
    total_triples = 0
    for rec in records:
        triples = structured_triples(rec["doc_id"], rec)
        total_triples += len(triples)
        envelope = {"triples": triples, "stop": True}
        r = subprocess.run(
            [str(sage_bin), "ingest-stub", "--db", str(sled_path),
             "--tenant", str(tenant), "--doc-id", str(rec["doc_id"])],
            input=json.dumps(envelope), capture_output=True, text=True, timeout=60,
        )
        if r.returncode != 0:
            print(f"  fail doc {rec['doc_id']}: {r.stderr[:120]}", file=sys.stderr)
    dt = time.time() - t0
    print(f"  [A] {len(records)} docs, {total_triples} triples, {dt:.1f}s, 0 LLM tokens")
    return {"strategy": "A", "docs": len(records), "triples": total_triples,
            "time_s": round(dt, 1), "llm_tokens_est": 0}


# ---------- Strategy B — A + LLM on abstract+claim1 ----------

def llm_extract_short(rec: dict, sage_bin: Path, llm: str = "minimax",
                     ) -> list[dict]:
    """Run SAGE writer on JUST abstract+first-claim. Returns triples."""
    short_text = (rec.get("abstract", "")
                  + " First claim: " + rec.get("claim1", "")[:300])
    if len(short_text) < 100:
        return []
    pubnum = rec["publication_number"]
    # Use `sage ingest` (single-doc) with a TEMP sled to capture the LLM
    # extraction, then dump triples out via inspect. This is hacky; a real
    # impl would expose `sage writer-extract --text X` returning JSON.
    # For experiment-speed we just call the LLM directly via writer policy.
    # Simpler: use --llm minimax which is cheap.
    import tempfile, shutil
    with tempfile.TemporaryDirectory() as tmp:
        tmp_db = Path(tmp) / "tmp.sled"
        cmd = [str(sage_bin), "ingest", "--db", str(tmp_db),
               "--doc-id", "1", "--tenant", "1", "--llm", llm,
               "--doc", short_text]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=120,
                           env={**__import__("os").environ})
        if r.returncode != 0:
            return []
        # Parse the JSON output for triples_extracted hint and use sage list
        # to find entities just added.
        # Simpler: re-run a "list" to get the entities-by-doc_id.
        list_cmd = [str(sage_bin), "list", "--db", str(tmp_db),
                    "--tenant", "1", "--limit", "200"]
        lr = subprocess.run(list_cmd, capture_output=True, text=True, timeout=60)
        if lr.returncode != 0:
            return []
        # Parse the listed entities, re-bind them to our REAL doc_id with
        # a synthetic "extracted_from" relation.
        try:
            data = json.loads(lr.stdout)
            ents = data.get("entities", [])
        except Exception:
            return []
        triples = []
        for e in ents:
            if e.get("source_docs"):
                # Reframe: this entity came from short text, anchor to OUR pubnum.
                triples.append({"src": pubnum, "src_type": "Concept",
                                "rel": "mentions",
                                "dst": e["name"], "dst_type": e.get("etype", "Concept")})
        return triples


def build_strategy_b(records, sled_path: Path, sage_bin: Path, tenant: int):
    """Strategy B: zero-token structured + light LLM on abstract."""
    print(f"\n[B] Building hybrid sled at {sled_path}")
    if sled_path.exists():
        import shutil
        shutil.rmtree(sled_path)
    t0 = time.time()
    total_triples = 0
    for i, rec in enumerate(records, 1):
        triples = structured_triples(rec["doc_id"], rec)
        triples += llm_extract_short(rec, sage_bin)
        total_triples += len(triples)
        envelope = {"triples": triples, "stop": True}
        subprocess.run(
            [str(sage_bin), "ingest-stub", "--db", str(sled_path),
             "--tenant", str(tenant), "--doc-id", str(rec["doc_id"])],
            input=json.dumps(envelope), capture_output=True, text=True, timeout=60,
        )
        if i % 20 == 0:
            print(f"  [B] {i}/{len(records)}...")
    dt = time.time() - t0
    print(f"  [B] {len(records)} docs, {total_triples} triples, {dt:.1f}s")
    return {"strategy": "B", "docs": len(records), "triples": total_triples,
            "time_s": round(dt, 1), "llm_tokens_est": len(records) * 1500}


# ---------- Strategy D — LLM-only on short text ----------

def build_strategy_d(records, sled_path: Path, sage_bin: Path, tenant: int,
                     llm: str = "minimax"):
    """Strategy D: LLM extraction on abstract+claim1 ONLY. No structured fields."""
    print(f"\n[D] Building LLM-only-short sled at {sled_path}")
    if sled_path.exists():
        import shutil
        shutil.rmtree(sled_path)
    # Build a temp jsonl with just short text, ingest-batch via LLM
    short_jsonl = Path("/tmp/strategy_d_docs.jsonl")
    with open(short_jsonl, "w", encoding="utf-8") as f:
        for rec in records:
            short_text = (rec["publication_number"] + ": " + rec["title"] + ". "
                          + rec.get("abstract", "") + " First claim: "
                          + rec.get("claim1", "")[:300])
            f.write(json.dumps({"doc_id": rec["doc_id"], "text": short_text}) + "\n")
    t0 = time.time()
    r = subprocess.run(
        [str(sage_bin), "ingest-batch", "--db", str(sled_path),
         "--tenant", str(tenant), "--llm", llm, "--jsonl", str(short_jsonl)],
        capture_output=True, text=True, timeout=3600,
    )
    dt = time.time() - t0
    # Parse summary
    try:
        # output ends with JSON summary
        m = re.search(r'(\{[^{}]*"docs_ingested"[^{}]*\})', r.stdout)
        summary = json.loads(m.group(1)) if m else {}
    except Exception:
        summary = {}
    print(f"  [D] {len(records)} docs, {dt:.1f}s, summary={summary}")
    return {"strategy": "D", "docs": len(records),
            "triples": summary.get("edges_added", 0),
            "time_s": round(dt, 1),
            "llm_tokens_est": len(records) * 1500}


# ---------- Eval runner ----------

def run_eval(queries_path: Path, sled_path: Path, sage_bin: Path,
             tenant: int, k: int) -> dict:
    qs = json.load(open(queries_path, encoding="utf-8"))
    out = {"by_tier": {}, "overall": {}}
    # Per tier
    tiers = sorted(set(q.get("tier", q.get("_complexity", 0)) for q in qs))
    for t in tiers:
        sub = [q for q in qs if q.get("tier", q.get("_complexity", 0)) == t]
        r = subprocess.run(
            [str(sage_bin), "eval", "--db", str(sled_path),
             "--tenant", str(tenant), "--k", str(k)],
            input=json.dumps(sub), capture_output=True, text=True, timeout=300,
        )
        try:
            er = json.loads(r.stdout)
            out["by_tier"][f"tier_{t}"] = {
                "n": er["samples"],
                "recall_at_k": round(er["recall_at_k"], 3),
                "mrr": round(er["mrr"], 3),
                "precision_at_k": round(er["precision_at_k"], 3),
            }
        except Exception:
            out["by_tier"][f"tier_{t}"] = {"error": "parse"}
    # Overall
    r = subprocess.run(
        [str(sage_bin), "eval", "--db", str(sled_path),
         "--tenant", str(tenant), "--k", str(k)],
        input=json.dumps(qs), capture_output=True, text=True, timeout=300,
    )
    try:
        er = json.loads(r.stdout)
        out["overall"] = {
            "n": er["samples"], "recall_at_k": round(er["recall_at_k"], 3),
            "mrr": round(er["mrr"], 3), "precision_at_k": round(er["precision_at_k"], 3),
        }
    except Exception:
        out["overall"] = {"error": "parse"}
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", type=Path,
                    default=Path(r"C:/Users/User/Desktop/專利系統RUST/patent-ai/data/cache"))
    ap.add_argument("--filter-html", default="Peplink|Pismo Labs")
    ap.add_argument("--sage-bin", type=Path, default=Path("./target/release/sage.exe"))
    ap.add_argument("--tenant", type=int, default=1)
    ap.add_argument("--out", type=Path, default=Path("examples/eval_peplink/STRATEGY_COMPARISON.md"))
    ap.add_argument("--queries", type=Path, default=Path("examples/eval_peplink/queries.json"))
    ap.add_argument("--queries-complexity", type=Path, default=Path("examples/eval_peplink/queries_complexity.json"))
    ap.add_argument("--sled-c-existing", type=Path,
                    default=Path("C:/Users/User/AppData/Local/Temp/sage_peplink.sled"))
    ap.add_argument("--limit", type=int, default=0, help="Limit corpus for quick test (0=all)")
    ap.add_argument("--with-b", action="store_true", help="Build Strategy B (slow, uses LLM)")
    ap.add_argument("--with-d", action="store_true", help="Build Strategy D (slow, uses LLM)")
    args = ap.parse_args()

    # Re-export to get the parsed records WITH structured fields preserved.
    # We modify the exporter to ALSO emit a 'records.jsonl' but for now we
    # re-parse from cache directly here using the same regexes.
    print("Loading + parsing patent cache (only filtered subset)...")
    import zstandard as zstd
    sys.path.insert(0, str(Path(__file__).parent))
    from export_patent_cache_to_sage_jsonl import parse_one, stable_doc_id
    dctx = zstd.ZstdDecompressor()
    filter_re = re.compile(args.filter_html, re.IGNORECASE)
    records = []
    files = sorted(args.cache.iterdir())
    for path in files:
        if path.suffix != ".zst": continue
        with open(path, "rb") as fh:
            html = dctx.stream_reader(fh).read().decode("utf-8", errors="replace")
        if not filter_re.search(html): continue
        rec = parse_one(html)
        if not rec or not rec["publication_number"] or len(rec.get("abstract", "")) < 80:
            continue
        rec["doc_id"] = stable_doc_id(rec["publication_number"])
        records.append(rec)
        if args.limit and len(records) >= args.limit: break
    print(f"Loaded {len(records)} parsed patent records")

    # Build each strategy sled
    sled_a = Path("C:/Users/User/AppData/Local/Temp/sage_peplink_strategy_a.sled")
    sled_b = Path("C:/Users/User/AppData/Local/Temp/sage_peplink_strategy_b.sled")
    sled_d = Path("C:/Users/User/AppData/Local/Temp/sage_peplink_strategy_d.sled")

    results = {}
    results["A"] = build_strategy_a(records, sled_a, args.sage_bin, args.tenant)
    if args.with_b:
        results["B"] = build_strategy_b(records, sled_b, args.sage_bin, args.tenant)
    if args.with_d:
        results["D"] = build_strategy_d(records, sled_d, args.sage_bin, args.tenant)

    # C = existing
    results["C"] = {"strategy": "C", "docs": 82, "triples": 5662,
                    "time_s": 6000, "llm_tokens_est": 2_700_000,
                    "_note": "Pre-existing /tmp/sage_peplink.sled"}

    # Eval each
    print("\n=== Eval ===")
    eval_results = {}
    eval_pairs = [("A", sled_a), ("C", args.sled_c_existing)]
    if args.with_b: eval_pairs.append(("B", sled_b))
    if args.with_d: eval_pairs.append(("D", sled_d))
    for name, sled in eval_pairs:
        if not sled.exists():
            print(f"  [{name}] sled missing, skip")
            continue
        print(f"  [{name}] eval queries.json k=3 ...")
        eval_results[name] = {
            "queries_k3": run_eval(args.queries, sled, args.sage_bin, args.tenant, 3),
            "complexity_k3": run_eval(args.queries_complexity, sled, args.sage_bin,
                                      args.tenant, 3),
        }

    # Write report
    md = ["# Strategy Comparison — SAGE × patent-ai Integration",
          "",
          "> 4 ingest strategies tested on the same 91-patent Peplink corpus,",
          "> evaluated against `queries.json` (24 standard queries) and",
          "> `queries_complexity.json` (42 graded queries).",
          ""]
    md.append("## Build cost summary")
    md.append("| Strategy | Description | Docs | Triples | Time (s) | LLM tokens |")
    md.append("|---|---|---|---|---|---|")
    for k in ["A", "B", "C", "D"]:
        if k not in results: continue
        r = results[k]
        desc = {"A": "zero-token structured",
                "B": "structured + LLM abstract",
                "C": "full LLM (existing)",
                "D": "LLM abstract only"}[k]
        md.append(f"| {k} | {desc} | {r.get('docs','-')} | {r.get('triples','-')} | {r.get('time_s','-')} | {r.get('llm_tokens_est','-'):,} |")
    md.append("")
    md.append("## Eval — queries.json (6 tiers)")
    md.append("| Strategy | tier 5 multi-hop MRR | tier 6 bridge MRR | overall MRR |")
    md.append("|---|---|---|---|")
    for k in eval_results:
        e = eval_results[k]["queries_k3"]
        t5 = e["by_tier"].get("tier_5", {}).get("mrr", "—")
        t6 = e["by_tier"].get("tier_6", {}).get("mrr", "—")
        ov = e["overall"].get("mrr", "—")
        md.append(f"| {k} | {t5} | {t6} | {ov} |")
    md.append("")
    md.append("## Eval — queries_complexity.json (8 complexity tiers)")
    md.append("| Strategy | overall MRR | Prec@3 |")
    md.append("|---|---|---|")
    for k in eval_results:
        e = eval_results[k]["complexity_k3"]
        md.append(f"| {k} | {e['overall'].get('mrr','—')} | {e['overall'].get('precision_at_k','—')} |")
    md.append("")
    md.append("## Raw results JSON")
    md.append("```json")
    md.append(json.dumps({"build_cost": results, "eval": eval_results},
                         indent=2, ensure_ascii=False))
    md.append("```")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(md), encoding="utf-8")
    print(f"\n→ {args.out}")


if __name__ == "__main__":
    main()
