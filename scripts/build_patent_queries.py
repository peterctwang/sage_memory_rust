#!/usr/bin/env python3
"""
Generate `examples/eval_patents/queries.json` from the live export.

We build queries with KNOWN ground-truth doc_id lists derived directly
from the corpus, so the eval metrics aren't lying. For SAGE's
`Recall@k` metric a query succeeds when at least one of its GT doc_ids
appears in the top-k retrieved docs — so a GT list of "all 187 Qualcomm
patents" is perfectly valid for a query like "Patents at Qualcomm".

## Tier design (patent domain)
| Tier | Pattern                                  | n |
|------|------------------------------------------|---|
| 1 exact       | Specific patent by unique title phrase    | 8 |
| 2 multi-token | Title-keyword + assignee combo            | 8 |
| 3 descriptive | Tech-area description w/o exact keywords  | 8 |
| 4 paraphrase  | Industry-style descriptions               | 8 |
| 5 multi-hop   | "Multiple patents at X" / "Two inventors" | 10|
| 6 bridge      | inventor → assignee, CPC → assignee, etc.  | 8 |

## How it works
1. Stream the docs.jsonl + patents.map.jsonl.
2. Index by assignee, inventor, CPC code, and title keywords.
3. Emit queries with GT = list of all doc_ids matching the predicate.

Usage:
    python scripts/build_patent_queries.py \\
        --docs examples/eval_patents/docs.jsonl \\
        --map  examples/eval_patents/patents.map.jsonl \\
        --out  examples/eval_patents/queries.json
"""
from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from pathlib import Path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--docs", required=True, type=Path)
    ap.add_argument("--map", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    # ---- Index the corpus ----
    by_assignee: dict[str, list[int]] = defaultdict(list)
    by_inventor: dict[str, list[int]] = defaultdict(list)
    by_cpc_prefix: dict[str, list[int]] = defaultdict(list)
    by_pub: dict[str, int] = {}
    title_by_id: dict[int, str] = {}

    with open(args.map, encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            by_pub[r["publication_number"]] = r["doc_id"]
            title_by_id[r["doc_id"]] = r["title"]

    re_assignee = re.compile(r"Assigned to ([^.]+)\.")
    re_inventors = re.compile(r" by ([^.]+?)(?:\. Assigned| \.)")
    re_cpc = re.compile(r"CPC classes: ([^.]+)\.")

    with open(args.docs, encoding="utf-8") as f:
        for line in f:
            d = json.loads(line)
            did = d["doc_id"]
            text = d["text"]
            m = re_assignee.search(text)
            if m:
                for a in [x.strip() for x in m.group(1).split(",")]:
                    if a and a != "Individual":
                        by_assignee[a].append(did)
            m = re_inventors.search(text)
            if m:
                for inv in [x.strip() for x in m.group(1).split(",")]:
                    if inv and len(inv) > 2:
                        by_inventor[inv].append(did)
            m = re_cpc.search(text)
            if m:
                for c in [x.strip() for x in m.group(1).split(",")]:
                    if c:
                        by_cpc_prefix[c[:5]].append(did)  # e.g. "H04W7"

    # ---- Build queries ----
    queries: list[dict] = []

    # Helper: keep only assignees/inventors with at least N patents (so the
    # multi-hop "Two/Three patents" question has enough GT).
    big_assignees = {a: ids for a, ids in by_assignee.items() if len(ids) >= 15}
    big_inventors = {i: ids for i, ids in by_inventor.items() if len(ids) >= 10}
    big_cpc = {c: ids for c, ids in by_cpc_prefix.items() if len(ids) >= 50}

    # Pick the 8 biggest assignees / inventors deterministically.
    top_assignees = sorted(big_assignees.items(), key=lambda kv: -len(kv[1]))[:10]
    top_inventors = sorted(big_inventors.items(), key=lambda kv: -len(kv[1]))[:10]
    top_cpc = sorted(big_cpc.items(), key=lambda kv: -len(kv[1]))[:8]

    # ---- Tier 5: Multi-hop "Multiple patents at X" ----
    for company, ids in top_assignees[:6]:
        queries.append(
            {
                "tier": 5,
                "kind": "multi-hop",
                "query": f"Two patents assigned to {company}",
                "ground_truth": ids,
                "_note": f"{len(ids)} candidate docs; any match in top-k counts.",
            }
        )

    for inv, ids in top_inventors[:4]:
        queries.append(
            {
                "tier": 5,
                "kind": "multi-hop",
                "query": f"Two patents invented by {inv}",
                "ground_truth": ids,
                "_note": f"{len(ids)} candidate docs.",
            }
        )

    # ---- Tier 6: Bridge — inventor → also patents in same CPC, etc. ----
    # "Patents in CPC class H04W74 assigned to Qualcomm" — intersection.
    for company, c_ids in top_assignees[:4]:
        for cpc, cpc_ids in top_cpc[:2]:
            inter = list(set(c_ids) & set(cpc_ids))
            if len(inter) >= 3:
                queries.append(
                    {
                        "tier": 6,
                        "kind": "bridge",
                        "query": f"Patents in CPC {cpc} assigned to {company}",
                        "ground_truth": inter,
                        "_note": f"{len(inter)} candidates in intersection.",
                    }
                )

    # ---- Tier 1: Exact — well-known assignee in single-word form ----
    # Tier-1 queries need a clear single doc; we pick by inventor with few patents
    # so retrieval is precise. Pick 8 inventors with exactly 2-3 patents.
    medium_inventors = [
        (i, ids) for i, ids in by_inventor.items() if 2 <= len(ids) <= 3 and len(i) > 5
    ][:8]
    for inv, ids in medium_inventors:
        queries.append(
            {
                "tier": 1,
                "kind": "exact",
                "query": f"Patents by {inv}",
                "ground_truth": ids,
            }
        )

    # ---- Tier 2: Multi-token — title keyword ----
    # Find patents with specific tech keywords in title.
    keyword_examples = [
        ("WiFi band steering", "band steering"),
        ("Traffic identification machine learning", "traffic identification"),
        ("multi-link operation", "multi-link"),
        ("Power saving wireless", "power saving"),
        ("Beamforming antenna array", "beamforming"),
        ("Channel access wireless network", "channel access"),
        ("Mesh network routing", "mesh"),
        ("Quality of service traffic", "qos"),
    ]
    for query_text, needle in keyword_examples:
        hits = []
        with open(args.docs, encoding="utf-8") as f:
            for line in f:
                d = json.loads(line)
                if needle.lower() in d["text"].lower():
                    hits.append(d["doc_id"])
                    if len(hits) >= 20:
                        break
        if hits:
            queries.append(
                {
                    "tier": 2,
                    "kind": "multi-token",
                    "query": query_text,
                    "ground_truth": hits,
                }
            )

    # ---- Tier 3: Descriptive — paraphrased tech area ----
    descriptive = [
        ("Wireless network handover between cells", "handover"),
        ("Spectrum sharing between licensed and unlicensed", "unlicensed"),
        ("Low-latency data transmission", "low latency"),
        ("Authentication protocol for wireless devices", "authentication"),
        ("Energy harvesting in IoT devices", "energy harvest"),
        ("Encryption key exchange", "key exchange"),
        ("Network slicing in 5G", "slicing"),
        ("Edge computing offloading", "edge computing"),
    ]
    for query_text, needle in descriptive:
        hits = []
        with open(args.docs, encoding="utf-8") as f:
            for line in f:
                d = json.loads(line)
                if needle.lower() in d["text"].lower():
                    hits.append(d["doc_id"])
                    if len(hits) >= 20:
                        break
        if hits:
            queries.append(
                {
                    "tier": 3,
                    "kind": "descriptive",
                    "query": query_text,
                    "ground_truth": hits,
                }
            )

    # ---- Tier 4: Paraphrase — Industry-style ----
    paraphrase_examples = [
        ("Carrier-grade routing equipment innovations", "Cisco Technology Inc"),
        ("Smartphone manufacturer patents", "Apple Inc"),
        ("Chinese telecom giant patents", "Huawei Technologies Co Ltd"),
        ("Korean consumer electronics patents", "LG Electronics Inc"),
        ("Networking gear maker innovations", "Juniper Networks Inc"),
        ("Mobile chipset maker patents", "Qualcomm Inc"),
        ("Swedish telecom equipment vendor patents", "Telefonaktiebolaget LM Ericsson AB"),
        ("Korean memory and consumer electronics patents", "Samsung Electronics Co Ltd"),
    ]
    for query_text, company in paraphrase_examples:
        if company in by_assignee:
            queries.append(
                {
                    "tier": 4,
                    "kind": "paraphrase",
                    "query": query_text,
                    "ground_truth": by_assignee[company][:20],
                    "_note": f"Industry shorthand for '{company}'.",
                }
            )

    # ---- Write ----
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(queries, f, indent=2, ensure_ascii=False)

    # Summary
    from collections import Counter

    c = Counter(q["tier"] for q in queries)
    print(
        json.dumps(
            {
                "out": str(args.out),
                "total_queries": len(queries),
                "by_tier": dict(sorted(c.items())),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
