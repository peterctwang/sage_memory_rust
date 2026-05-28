#!/usr/bin/env python3
"""Competitor-patent breakthrough finder — combined SAGE + patent-ai workflow.

One CLI invocation. Produces an actionable markdown report identifying
breakthrough opportunities against a target competitor in a given topic
area. Built on L009's discovery that Strategy A (zero-token structured
ingest) gives the best retrieval quality.

## Pipeline (orchestrated by this script)

  1. (optional) Build Strategy A SAGE sled from patent-ai cache
                — 0 LLM tokens, ~4s per 91 patents
  2. SAGE retrieval: competitive landscape (top-15 docs)
                — 3 seconds, gives full competitor list
  3. SAGE entity scan: target's portfolio profile
                — Python aggregation, ~5s
  4. Python: gap matrix across 10 themes
                — 0 LLM, exact set arithmetic
  5. patent-ai associate: adjacent concept candidates (optional)
                — ~5 LLM calls if patent-ai is available
  6. Python: strategic moat detection (focal-dominant topics)
                — 0 LLM
  7. patent-ai ask: prose synthesis of findings (optional)
                — 1 LLM call, ~70s
  8. Compose final report

Steps 5 / 7 are gated on patent-ai availability; if it's down, the
report still ships using only SAGE + Python (which we proved in
STRATEGY_PARETO.md is enough for 90% of the value).

## Usage

    python scripts/competitor_breakthrough.py \\
        --target  "Pismo Labs Technology Ltd" \\
        --topic   "robust WAN link" \\
        --sage-db C:/Users/User/AppData/Local/Temp/sage_peplink_strategy_a.sled \\
        --docs    examples/eval_peplink/docs.jsonl \\
        --map     examples/eval_peplink/patents.map.jsonl \\
        --sage-bin ./target/release/sage.exe \\
        --patent-bin "C:/.../patent.exe"    # optional
        --patent-cwd "C:/.../patent-ai"     # optional
        --out      output/breakthrough_robust_link.md
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path


# ---------- Themes used for gap matrix (extendable via CLI flag) ----------

DEFAULT_THEMES = {
    "Reinforcement learning policy":
        ["reinforcement learning", "q-learning", "policy gradient"],
    "ML link quality prediction":
        ["machine learning", "neural network", "predict", "forecast"],
    "Stateful recovery":
        ["recovery", "restoration", "session preserv", "in-flight"],
    "Anomaly detection":
        ["anomaly", "outlier", "deviation"],
    "Telemetry analytics":
        ["telemetry", "observability", "tracing"],
    "Federated learning":
        ["federated", "distributed learning"],
    "Multi-path TCP / QUIC":
        ["multipath TCP", "MPTCP", "QUIC", "multipath quic"],
    "Adaptive congestion control":
        ["congestion", "rate adapt", "feedback control"],
    "Jitter-aware scheduling":
        ["jitter", "interactive traffic"],
    "Encrypted traffic classification":
        ["encrypted traffic", "classification"],
}


# ---------- Corpus load ----------

def load_corpus(docs_path: Path, map_path: Path):
    re_assignee = re.compile(r"Assigned to ([^.]+)\.")
    re_inventor = re.compile(r" by ([^.]+?)(?:\. Assigned| \.)")
    re_cpc = re.compile(r"CPC classes: ([^.]+)\.")
    docs = {}
    for line in open(docs_path, encoding="utf-8"):
        d = json.loads(line)
        text = d["text"]
        am = re_assignee.search(text)
        assignees = [x.strip() for x in am.group(1).split(",")] if am else []
        im = re_inventor.search(text)
        inventors = [x.strip() for x in im.group(1).split(",")] if im else []
        cm = re_cpc.search(text)
        cpcs = [x.strip() for x in cm.group(1).split(",")] if cm else []
        docs[d["doc_id"]] = {
            "text": text,
            "assignees": [a for a in assignees if a],
            "inventors": [i for i in inventors if i],
            "cpcs": [c for c in cpcs if c],
        }
    meta = {}
    for line in open(map_path, encoding="utf-8"):
        r = json.loads(line)
        meta[r["doc_id"]] = r
    return docs, meta


# ---------- SAGE retrieval ----------

def sage_query(sage_bin: Path, db: Path, tenant: int, k: int, q: str,
               ) -> tuple[float, list[int]]:
    t0 = time.time()
    r = subprocess.run(
        [str(sage_bin), "query", "--db", str(db), "--tenant", str(tenant),
         "--k", str(k), q],
        capture_output=True, text=True, encoding="utf-8", timeout=60,
    )
    dt = time.time() - t0
    try:
        out = json.loads(r.stdout)
        return dt, [d["id"] for d in out.get("docs", [])]
    except Exception:
        return dt, []


# ---------- Steps ----------

def step1_competitive_landscape(target, topic, docs, meta, sage, sage_db, tenant):
    """Use SAGE to retrieve top docs for the topic, bucket by assignee."""
    dt, top_ids = sage_query(sage, sage_db, tenant, 15, topic)
    by_assignee = Counter()
    landscape = []
    for did in top_ids:
        if did not in docs: continue
        assignees = docs[did]["assignees"]
        for a in assignees:
            by_assignee[a] += 1
        landscape.append({
            "doc_id":             did,
            "publication_number": meta[did]["publication_number"],
            "title":              meta[did]["title"][:80],
            "assignee":           assignees[0] if assignees else "?",
            "is_target":          target in assignees,
        })
    return {"latency_s": round(dt, 2), "top_docs": landscape,
            "assignee_distribution": dict(by_assignee.most_common(20))}


def step2_target_profile(target, docs):
    """Aggregate target company's portfolio characteristics."""
    target_docs = [d for d in docs.values() if target in d["assignees"]]
    if not target_docs:
        return {"error": f"no docs found for target '{target}'"}
    inv = Counter(i for d in target_docs for i in d["inventors"])
    cpc = Counter(c[:5] for d in target_docs for c in d["cpcs"])
    return {
        "patent_count": len(target_docs),
        "top_inventors": dict(inv.most_common(8)),
        "top_cpc_prefixes": dict(cpc.most_common(8)),
        "inventor_concentration_top3_pct":
            round(100 * sum(n for _, n in inv.most_common(3)) / max(sum(inv.values()), 1), 1),
    }


def step3_gap_matrix(target, docs, themes):
    target_docs = [d for d in docs.values() if target in d["assignees"]]
    other_docs = [d for d in docs.values() if target not in d["assignees"]]
    rows = []
    for theme_name, kws in themes.items():
        t_hit = sum(1 for d in target_docs
                    if any(kw.lower() in d["text"].lower() for kw in kws))
        o_hit = sum(1 for d in other_docs
                    if any(kw.lower() in d["text"].lower() for kw in kws))
        if o_hit == 0 and t_hit == 0:
            status = "WHITE-SPACE"
        elif t_hit == 0 and o_hit > 0:
            status = "CLEAR-GAP"
        elif t_hit < o_hit / 3 and o_hit >= 3:
            status = "UNDER-INVESTED"
        elif t_hit >= o_hit:
            status = "TARGET-LEAD"
        else:
            status = "CONTESTED"
        severity = (o_hit + 1) / (t_hit + 1)
        rows.append({"theme": theme_name, "target": t_hit, "others": o_hit,
                     "status": status, "severity": round(severity, 2)})
    rows.sort(key=lambda r: -r["severity"] if r["status"] in
              ("CLEAR-GAP", "UNDER-INVESTED") else 0)
    return rows


def step4_strategic_moats(target, docs):
    target_docs = [d for d in docs.values() if target in d["assignees"]]
    other_docs = [d for d in docs.values() if target not in d["assignees"]]
    moat_keywords = ["bond", "aggregated connection", "multi-WAN", "throughput",
                     "tunnel", "SIM", "overlay", "load balanc", "VPN", "failover"]
    moats = []
    for kw in moat_keywords:
        t = sum(1 for d in target_docs if kw.lower() in d["text"].lower())
        o = sum(1 for d in other_docs if kw.lower() in d["text"].lower())
        if t == 0: continue
        ratio = (t + 1) / (o + 1)
        moats.append({"keyword": kw, "target": t, "others": o,
                      "moat_ratio": round(ratio, 2)})
    moats.sort(key=lambda m: -m["moat_ratio"])
    return moats


def step5_top_inventor_coverage(target, docs, themes):
    """For target's top inventor, show theme coverage — reveals structural blind spots."""
    target_docs = [d for d in docs.values() if target in d["assignees"]]
    if not target_docs: return {}
    inv_counter = Counter(i for d in target_docs for i in d["inventors"])
    top_inv, top_count = inv_counter.most_common(1)[0] if inv_counter else ("?", 0)
    inv_docs = [d for d in target_docs if top_inv in d["inventors"]]
    coverage = {}
    for theme_name, kws in themes.items():
        n = sum(1 for d in inv_docs
                if any(kw.lower() in d["text"].lower() for kw in kws))
        coverage[theme_name] = f"{n}/{len(inv_docs)}"
    return {"top_inventor": top_inv, "patent_count": top_count,
            "theme_coverage": coverage}


def step6_patent_ai_associate(concepts, patent_bin, patent_cwd, top_n=10):
    """Optional — call patent-ai for concept neighbors. Fails silently if unavailable."""
    if not patent_bin or not patent_bin.exists():
        return None
    out = {}
    for c in concepts:
        try:
            r = subprocess.run(
                [str(patent_bin), "associate", c, "-n", str(top_n), "-e", "0.7"],
                capture_output=True, text=True, encoding="utf-8", timeout=60,
                cwd=str(patent_cwd) if patent_cwd else None,
            )
            # Parse the table out of stdout
            lines = [l.strip() for l in r.stdout.splitlines()
                     if l.strip() and not l.startswith("[") and "─" not in l]
            neighbors = []
            for line in lines[1:]:   # skip header
                parts = line.split()
                if len(parts) < 4: continue
                neighbors.append(" ".join(parts[:-3]))
                if len(neighbors) >= 8: break
            out[c] = neighbors
        except Exception as e:
            out[c] = f"error: {e}"
    return out


def step7_patent_ai_ask(question, patent_bin, patent_cwd):
    """Optional — RAG synthesis. Fails silently if unavailable."""
    if not patent_bin or not patent_bin.exists():
        return None
    try:
        r = subprocess.run(
            [str(patent_bin), "ask", question],
            capture_output=True, text=True, encoding="utf-8", timeout=300,
            cwd=str(patent_cwd) if patent_cwd else None,
        )
        return r.stdout[-4000:] if r.returncode == 0 else f"error code {r.returncode}"
    except Exception as e:
        return f"error: {e}"


# ---------- Report ----------

def compose_report(target, topic, landscape, profile, gaps, moats, inventor,
                   adjacency, prose, latency_total):
    md = [
        f"# 競品突破點分析 — {target} 在「{topic}」",
        f"",
        f"> 自動生成 by `scripts/competitor_breakthrough.py`",
        f"> 整合 SAGE (Strategy A 零 token retrieval) + patent-ai (concept network + RAG)",
        f"> + Python 量化聚合。總耗時 ≈ {latency_total:.1f}s。",
        "",
        "## 摘要",
        "",
    ]
    # Top-3 actionable gaps
    clear_gaps = [g for g in gaps if g["status"] in ("CLEAR-GAP", "UNDER-INVESTED")][:3]
    if clear_gaps:
        md.append("### Top 3 突破方向(依嚴重度)")
        md.append("")
        for i, g in enumerate(clear_gaps, 1):
            md.append(f"{i}. **{g['theme']}** — {target} 件數 {g['target']} / "
                      f"競品件數 {g['others']} ({g['status']}, severity {g['severity']}×)")
        md.append("")

    md.append(f"## §1 競品景觀(SAGE 對 \"{topic}\" 的 top-15 retrieval)")
    md.append("")
    md.append("| Assignee | top-15 中件數 |")
    md.append("|---|---|")
    for a, n in landscape["assignee_distribution"].items():
        md.append(f"| {a} | {n} |")
    md.append("")
    md.append(f"_SAGE 查詢延遲: {landscape['latency_s']}s_")
    md.append("")
    md.append("**Top-15 docs**:")
    md.append("")
    md.append("| Rank | Pubnum | Assignee | Title |")
    md.append("|---|---|---|---|")
    for i, d in enumerate(landscape["top_docs"], 1):
        marker = "★" if d["is_target"] else ""
        md.append(f"| {i} | {d['publication_number']} | {marker} {d['assignee'][:25]} | {d['title']} |")
    md.append("")

    md.append(f"## §2 {target} 的 IP 組合")
    md.append("")
    md.append(f"- 在此 corpus 中專利件數: **{profile['patent_count']}**")
    md.append(f"- Top-3 發明人佔比: **{profile['inventor_concentration_top3_pct']}%** of inventor-links")
    md.append("")
    md.append("**Top 發明人**:")
    md.append("")
    md.append("| Rank | Inventor | Patents |")
    md.append("|---|---|---|")
    for i, (inv, n) in enumerate(profile["top_inventors"].items(), 1):
        md.append(f"| {i} | {inv} | {n} |")
    md.append("")
    md.append("**Top CPC prefixes**:")
    md.append("")
    md.append("| CPC | Patents |")
    md.append("|---|---|")
    for c, n in profile["top_cpc_prefixes"].items():
        md.append(f"| {c} | {n} |")
    md.append("")

    md.append("## §3 主題缺口矩陣(主指標)")
    md.append("")
    md.append("| Theme | Target | Others | Status | Severity |")
    md.append("|---|---|---|---|---|")
    for g in gaps:
        md.append(f"| {g['theme']} | {g['target']} | {g['others']} | "
                  f"**{g['status']}** | {g['severity']}× |")
    md.append("")
    md.append("**讀法**: CLEAR-GAP = target 0 件競品多件、UNDER-INVESTED = target 件數遠落後、"
              "WHITE-SPACE = 全部 0 件(藍海)、CONTESTED = 雙方差不多、TARGET-LEAD = target 領先。")
    md.append("")

    md.append("## §4 戰略護城河(target 鎖死的領土,避雷區)")
    md.append("")
    md.append("| Keyword | Target patents | Others | Moat ratio |")
    md.append("|---|---|---|---|")
    for m in moats:
        md.append(f"| {m['keyword']} | {m['target']} | {m['others']} | {m['moat_ratio']}× |")
    md.append("")

    if inventor:
        md.append(f"## §5 結構性盲點 — 首席發明人 {inventor['top_inventor']} 的覆蓋")
        md.append("")
        md.append(f"佔 target 全部 {profile['patent_count']} 件中的 {inventor['patent_count']} 件 "
                  f"({100*inventor['patent_count']/profile['patent_count']:.0f}%)。")
        md.append("")
        md.append("| Theme | Coverage |")
        md.append("|---|---|")
        for theme, cov in inventor["theme_coverage"].items():
            n = int(cov.split("/")[0])
            mark = "⚠️ **盲點**" if n == 0 else ""
            md.append(f"| {theme} | {cov} {mark} |")
        md.append("")
        md.append("**解讀**: 首席發明人 0 件的主題,代表 target 公司在這方向沒有 in-house expertise — "
                  "競品進場面對的是 commodity prior art 而非強大現有專家。")
        md.append("")

    if adjacency:
        md.append("## §6 鄰近領域(patent-ai concept neighbors,-e 0.7 explore)")
        md.append("")
        for concept, neighbors in adjacency.items():
            md.append(f"**\"{concept}\" 的鄰近概念**:")
            if isinstance(neighbors, list):
                md.append(", ".join(neighbors[:8]) if neighbors else "(無資料)")
            else:
                md.append(str(neighbors))
            md.append("")

    if prose:
        md.append("## §7 patent-ai RAG 合成意見")
        md.append("")
        md.append("```")
        md.append(prose[:3000])
        md.append("```")
        md.append("")

    md.append("## §8 行動建議(自動生成,需人工審核)")
    md.append("")
    for i, g in enumerate(clear_gaps, 1):
        md.append(f"{i}. **{g['theme']}** — 嚴重度 {g['severity']}×")
        md.append(f"   - 申請 provisional patent 主張 {g['theme']} 的新方法")
        md.append(f"   - 避開 §4 護城河列出的 keyword")
        if inventor:
            blind = [t for t, c in inventor["theme_coverage"].items()
                     if c.startswith("0/") and t == g["theme"]]
            if blind:
                md.append(f"   - 結構性窗口: {inventor['top_inventor']} 在此 0 件,"
                          f"target 短期內難跟進")
        md.append("")
    md.append("---")
    md.append("")
    md.append("**注意**:本報告由自動 pipeline 生成。具體 claim 仍需專利律師展開,"
              "且建議在申請前做正式 FTO (freedom-to-operate) 分析。")
    return "\n".join(md)


# ---------- Main ----------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", required=True, help="Target company (e.g. 'Pismo Labs Technology Ltd')")
    ap.add_argument("--topic", required=True, help="Topic to analyze (e.g. 'robust WAN link')")
    ap.add_argument("--sage-bin", type=Path, required=True)
    ap.add_argument("--sage-db", type=Path, required=True)
    ap.add_argument("--tenant", type=int, default=1)
    ap.add_argument("--docs", type=Path, required=True)
    ap.add_argument("--map", type=Path, required=True)
    ap.add_argument("--patent-bin", type=Path, default=None,
                    help="Optional patent-ai binary for concept network + RAG")
    ap.add_argument("--patent-cwd", type=Path, default=None)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--skip-rag", action="store_true",
                    help="Skip patent-ai ask (the slow ~70s RAG call)")
    args = ap.parse_args()

    t_total = time.time()
    print(f"[load] corpus from {args.docs}")
    docs, meta = load_corpus(args.docs, args.map)
    print(f"  loaded {len(docs)} docs")

    print(f"[step 1] SAGE retrieval for topic = '{args.topic}'")
    landscape = step1_competitive_landscape(args.target, args.topic, docs, meta,
                                            args.sage_bin, args.sage_db, args.tenant)
    print(f"  top {len(landscape['top_docs'])} docs, "
          f"{len(landscape['assignee_distribution'])} assignees")

    print(f"[step 2] target profile for {args.target}")
    profile = step2_target_profile(args.target, docs)
    if "error" in profile:
        print(f"  ERROR: {profile['error']}", file=sys.stderr)
        sys.exit(1)
    print(f"  {profile['patent_count']} patents, top inventor = "
          f"{list(profile['top_inventors'].keys())[0] if profile['top_inventors'] else '?'}")

    print(f"[step 3] gap matrix across {len(DEFAULT_THEMES)} themes")
    gaps = step3_gap_matrix(args.target, docs, DEFAULT_THEMES)
    clear = [g for g in gaps if g["status"] == "CLEAR-GAP"]
    print(f"  found {len(clear)} CLEAR-GAP themes")

    print(f"[step 4] strategic moats")
    moats = step4_strategic_moats(args.target, docs)
    print(f"  top moat: {moats[0]['keyword']} ratio={moats[0]['moat_ratio']}×")

    print(f"[step 5] top-inventor structural coverage")
    inventor = step5_top_inventor_coverage(args.target, docs, DEFAULT_THEMES)

    adjacency = None
    if args.patent_bin and args.patent_bin.exists():
        print(f"[step 6] patent-ai concept neighbors")
        top_concepts = [g["theme"].split(" ")[0].lower() for g in gaps[:3]]
        adjacency = step6_patent_ai_associate(top_concepts,
                                              args.patent_bin, args.patent_cwd)

    prose = None
    if args.patent_bin and args.patent_bin.exists() and not args.skip_rag:
        print(f"[step 7] patent-ai RAG synthesis (~70s)")
        question = (f"For company {args.target} in the area of '{args.topic}', "
                    f"what are the unexplored patent opportunities that competitors "
                    f"have addressed but {args.target} has not?")
        prose = step7_patent_ai_ask(question, args.patent_bin, args.patent_cwd)

    t_dt = time.time() - t_total
    print(f"[compose] writing {args.out}")
    md = compose_report(args.target, args.topic, landscape, profile, gaps,
                        moats, inventor, adjacency, prose, t_dt)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(md, encoding="utf-8")
    print(f"\n→ {args.out}  (total {t_dt:.1f}s)")


if __name__ == "__main__":
    main()
