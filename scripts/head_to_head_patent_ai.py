#!/usr/bin/env python3
"""Head-to-head: SAGE vs patent-ai on identical patent-research questions.

We run the SAME question through both systems and inspect what each returns.
The systems are different in shape:
  - SAGE returns a ranked doc-id list (`sage query --k N`).
  - patent-ai returns a synthesised natural-language answer (`patent ask`).

So the comparison is asymmetric: SAGE is judged on whether the right docs
surface; patent-ai is judged on whether the answer is factually correct.
We capture BOTH outputs and grade by hand.

Usage:
    python scripts/head_to_head_patent_ai.py \
        --sage-bin   ./target/release/sage.exe \
        --sage-db    /tmp/sage_peplink.sled \
        --tenant     1 \
        --patent-bin "C:/Users/User/Desktop/專利系統RUST/patent-ai/target/release/patent.exe" \
        --patent-cwd "C:/Users/User/Desktop/專利系統RUST/patent-ai" \
        --out        examples/eval_peplink/head_to_head.md
"""
from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path


QUESTIONS = [
    # 1) Exact lookup
    {"kind": "exact-lookup",
     "question": "Who invented patent US9019827B1?"},
    # 2) Single-filter retrieval
    {"kind": "single-filter",
     "question": "List patents assigned to Pismo Labs Technology"},
    # 3) Topic-focused
    {"kind": "topic",
     "question": "What patents cover WAN bonding methods?"},
    # 4) Multi-hop entity
    {"kind": "multi-hop",
     "question": "Who are the top inventors at Pismo Labs working on aggregated connection?"},
    # 5) Bridge / 2-hop
    {"kind": "bridge",
     "question": "What CPC classes does Pismo Labs's SIM card portfolio cover?"},
    # 6) Cross-assignee comparison
    {"kind": "competitive",
     "question": "Compare Pismo Labs vs Cisco on tunnel-based VPN technology"},
    # 7) Conceptual / paraphrase
    {"kind": "paraphrase",
     "question": "Which patents address combining multiple internet uplinks into a single logical link?"},
    # 8) Synthesis / strategy
    {"kind": "strategy",
     "question": "Where is Pismo Labs's strongest IP moat — which technology areas does only Pismo file?"},
]


def run_sage(sage_bin: Path, db: Path, tenant: int, k: int, q: str) -> dict:
    t0 = time.time()
    r = subprocess.run(
        [str(sage_bin), "query", "--db", str(db), "--tenant", str(tenant),
         "--k", str(k), q],
        capture_output=True, text=True, encoding="utf-8", timeout=60,
    )
    dt = time.time() - t0
    try:
        out = json.loads(r.stdout)
        docs = out.get("docs", [])
    except Exception:
        docs = []
    return {"latency_s": round(dt, 2),
            "n_docs": len(docs),
            "doc_ids": [d["id"] for d in docs],
            "scores": [round(d.get("score", 0), 3) for d in docs],
            "stderr_tail": r.stderr[-200:] if r.returncode else ""}


def run_patent_ai(bin_path: Path, cwd: Path, q: str) -> dict:
    """patent-ai uses RAG: retrieve docs from KG + LLM synthesise answer."""
    t0 = time.time()
    r = subprocess.run(
        [str(bin_path), "ask", q],
        capture_output=True, text=True, encoding="utf-8", timeout=300,
        cwd=str(cwd),
    )
    dt = time.time() - t0
    return {"latency_s": round(dt, 2),
            "exit_code": r.returncode,
            "answer": r.stdout[-4000:] if r.stdout else "",  # tail to limit
            "stderr_tail": r.stderr[-300:] if r.stderr else ""}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sage-bin", required=True, type=Path)
    ap.add_argument("--sage-db", required=True, type=Path)
    ap.add_argument("--tenant", type=int, default=1)
    ap.add_argument("--patent-bin", required=True, type=Path)
    ap.add_argument("--patent-cwd", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--k", type=int, default=5)
    args = ap.parse_args()

    results = []
    for i, spec in enumerate(QUESTIONS, 1):
        print(f"\n[{i}/{len(QUESTIONS)}] {spec['question']}")
        sage_r = run_sage(args.sage_bin, args.sage_db, args.tenant, args.k, spec["question"])
        print(f"  SAGE: {sage_r['latency_s']}s, {sage_r['n_docs']} docs")
        patent_r = run_patent_ai(args.patent_bin, args.patent_cwd, spec["question"])
        print(f"  patent-ai: {patent_r['latency_s']}s, exit={patent_r['exit_code']}")
        results.append({"spec": spec, "sage": sage_r, "patent_ai": patent_r})

    # Write markdown
    md = ["# Head-to-Head: SAGE vs patent-ai",
          "",
          "> Same 8 questions, both systems, side-by-side outputs.",
          "> SAGE returns ranked doc_ids; patent-ai returns LLM-synthesised text answer.",
          ""]
    # Summary table
    md.append("## Latency summary")
    md.append("")
    md.append("| # | Question kind | SAGE (s) | patent-ai (s) |")
    md.append("|---|---|---|---|")
    for i, r in enumerate(results, 1):
        md.append(f"| {i} | {r['spec']['kind']} | {r['sage']['latency_s']} | {r['patent_ai']['latency_s']} |")
    md.append("")
    md.append("---")
    md.append("")
    # Per-question detail
    for i, r in enumerate(results, 1):
        md.append(f"## {i}. [{r['spec']['kind']}] {r['spec']['question']}")
        md.append("")
        md.append(f"### SAGE (`sage query --k {args.k}`)")
        md.append(f"- latency: {r['sage']['latency_s']}s")
        md.append(f"- docs returned: {r['sage']['n_docs']}")
        if r["sage"]["doc_ids"]:
            md.append("- top doc_ids + scores:")
            for did, sc in zip(r["sage"]["doc_ids"], r["sage"]["scores"]):
                md.append(f"  - `{did}` (score {sc})")
        md.append("")
        md.append(f"### patent-ai (`patent ask`)")
        md.append(f"- latency: {r['patent_ai']['latency_s']}s")
        md.append(f"- exit: {r['patent_ai']['exit_code']}")
        md.append("")
        md.append("```")
        md.append(r["patent_ai"]["answer"][:2000])
        md.append("```")
        md.append("")
        md.append("---")
        md.append("")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(md), encoding="utf-8")
    print(f"\n→ {args.out}")


if __name__ == "__main__":
    main()
