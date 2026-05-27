#!/usr/bin/env python3
"""Build a Peplink/Pismo Labs-focused query set with hand-crafted multi-hop
queries that test SAGE's ability to retrieve ANY of N candidate docs.

Each query spec uses one or more selector predicates:
  - by_pubnum            : exact pin to one US/EP/CA patent number
  - keyword_any          : doc text contains any of the listed substrings
  - keyword_all          : doc text contains all of the listed substrings
  - assignee_filter      : doc's assignee equals this string (after intersection
                            with other selectors)
  - inventor_filter      : doc names this inventor
  - cpc_prefix           : doc lists a CPC code starting with this prefix

GT for each query = list of doc_ids satisfying the conjunction of its selectors.
Recall@k succeeds when ANY GT doc appears in top-k.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


QUERY_SPECS = [
    # ---- Tier 1: exact single-patent pins ----
    {"tier": 1, "kind": "exact",
     "query": "Throughput optimization for bonded variable bandwidth connections",
     "by_pubnum": "US9019827B1"},
    {"tier": 1, "kind": "exact",
     "query": "Systems and methods for wireless load balancing and channel selection",
     "by_pubnum": "US9402199B2"},
    {"tier": 1, "kind": "exact",
     "query": "Data transmission via a virtual wide area network overlay",
     "by_pubnum": "US10805840B2"},
    {"tier": 1, "kind": "exact",
     "query": "Methods and systems for transferring SIM card information",
     "by_pubnum": "US10009754B2"},

    # ---- Tier 2: multi-token tech keyword + (optional) assignee ----
    {"tier": 2, "kind": "multi-token",
     "query": "WAN bonding methods at Pismo Labs",
     "keyword_any": ["bond"],
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 2, "kind": "multi-token",
     "query": "Aggregated connection methods for transmitting data",
     "keyword_any": ["aggregated connection"]},
    {"tier": 2, "kind": "multi-token",
     "query": "SIM card management in wireless gateway",
     "keyword_any": ["SIM"]},
    {"tier": 2, "kind": "multi-token",
     "query": "Tunnel methods for transmitting packets",
     "keyword_any": ["tunnel"],
     "assignee_filter": "Pismo Labs Technology Ltd"},

    # ---- Tier 3: descriptive paraphrase ----
    {"tier": 3, "kind": "descriptive",
     "query": "Combining multiple internet links into one logical connection",
     "keyword_any": ["aggregated connection", "bond"]},
    {"tier": 3, "kind": "descriptive",
     "query": "Switching traffic between cellular and wired uplinks",
     "keyword_any": ["failover", "load balanc"]},
    {"tier": 3, "kind": "descriptive",
     "query": "Routing data over multiple WAN links simultaneously",
     "keyword_any": ["aggregated connection", "multi-WAN", "multiple network"]},

    # ---- Tier 4: industry-paraphrase ----
    {"tier": 4, "kind": "paraphrase",
     "query": "SD-WAN router maker patents",
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 4, "kind": "paraphrase",
     "query": "Hong Kong networking equipment manufacturer patents",
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 4, "kind": "paraphrase",
     "query": "WAN bonding pioneer patents",
     "assignee_filter": "Pismo Labs Technology Ltd"},

    # ---- Tier 5: multi-hop (the headline test) ----
    {"tier": 5, "kind": "multi-hop",
     "query": "Two patents assigned to Pismo Labs Technology Ltd",
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Two Pismo Labs patents about aggregated connection",
     "keyword_all": ["aggregated connection"],
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Two Pismo Labs patents about SIM card",
     "keyword_all": ["SIM"],
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Two Pismo Labs patents involving tunnels",
     "keyword_all": ["tunnel"],
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Two patents invented by Patrick Ho Wai Sung at Pismo Labs",
     "inventor_filter": "Patrick Ho Wai Sung"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Two patents invented by Kam Chiu NG at Pismo Labs",
     "inventor_filter": "Kam Chiu NG"},
    {"tier": 5, "kind": "multi-hop",
     "query": "Three Pismo Labs CPC H04L1 patents",
     "cpc_prefix": "H04L1",
     "assignee_filter": "Pismo Labs Technology Ltd"},

    # ---- Tier 6: bridge (chain reasoning) ----
    {"tier": 6, "kind": "bridge",
     "query": "WAN bonding inventors who also worked on SIM card management",
     "keyword_all": ["SIM"],
     "_note": "Cross-topic: GT = SIM patents at Pismo Labs (which by assumption share inventors with bonding patents)",
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 6, "kind": "bridge",
     "query": "Patents in CPC H04L1 assigned to Pismo Labs",
     "cpc_prefix": "H04L1",
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 6, "kind": "bridge",
     "query": "Pismo Labs inventors of tunnel-based packet transmission patents",
     "keyword_all": ["tunnel"],
     "assignee_filter": "Pismo Labs Technology Ltd"},
    {"tier": 6, "kind": "bridge",
     "query": "Aggregated connection patents also using overlay networks",
     "keyword_all": ["aggregated connection", "overlay"]},
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--docs", required=True, type=Path)
    ap.add_argument("--map", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    # Index
    docs = []
    with open(args.docs, encoding="utf-8") as f:
        for line in f:
            d = json.loads(line)
            docs.append(d)
    by_pubnum: dict[str, int] = {}
    with open(args.map, encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            by_pubnum[r["publication_number"]] = r["doc_id"]

    re_assignee = re.compile(r"Assigned to ([^.]+)\.")
    re_inventor = re.compile(r" by ([^.]+?)(?:\. Assigned| \.)")
    re_cpc = re.compile(r"CPC classes: ([^.]+)\.")

    def assignees_of(text: str) -> list[str]:
        m = re_assignee.search(text)
        if not m:
            return []
        return [x.strip() for x in m.group(1).split(",")]

    def inventors_of(text: str) -> list[str]:
        m = re_inventor.search(text)
        if not m:
            return []
        return [x.strip() for x in m.group(1).split(",")]

    def cpcs_of(text: str) -> list[str]:
        m = re_cpc.search(text)
        if not m:
            return []
        return [x.strip() for x in m.group(1).split(",")]

    queries = []
    for spec in QUERY_SPECS:
        gt: list[int] = []
        if "by_pubnum" in spec:
            pn = spec["by_pubnum"]
            if pn in by_pubnum:
                gt = [by_pubnum[pn]]
        else:
            for d in docs:
                text = d["text"]
                tl = text.lower()
                ok = True
                if "keyword_any" in spec:
                    if not any(k.lower() in tl for k in spec["keyword_any"]):
                        ok = False
                if ok and "keyword_all" in spec:
                    if not all(k.lower() in tl for k in spec["keyword_all"]):
                        ok = False
                if ok and "assignee_filter" in spec:
                    if spec["assignee_filter"] not in assignees_of(text):
                        ok = False
                if ok and "inventor_filter" in spec:
                    if spec["inventor_filter"] not in inventors_of(text):
                        ok = False
                if ok and "cpc_prefix" in spec:
                    if not any(c.startswith(spec["cpc_prefix"]) for c in cpcs_of(text)):
                        ok = False
                if ok:
                    gt.append(d["doc_id"])

        out = {"tier": spec["tier"], "kind": spec["kind"], "query": spec["query"]}
        out["ground_truth"] = gt
        if "_note" in spec:
            out["_note"] = spec["_note"]
        out["_gt_count"] = len(gt)
        queries.append(out)

    # Drop queries that found zero GT — they'd just zero out the eval.
    nonempty = [q for q in queries if q["ground_truth"]]
    dropped = len(queries) - len(nonempty)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(nonempty, f, indent=2, ensure_ascii=False)

    from collections import Counter
    c = Counter(q["tier"] for q in nonempty)
    print(json.dumps({
        "out": str(args.out),
        "total": len(nonempty),
        "dropped_zero_gt": dropped,
        "by_tier": dict(sorted(c.items())),
        "gt_size_distribution": {
            f"tier{t}": {
                "min": min(q["_gt_count"] for q in nonempty if q["tier"]==t),
                "max": max(q["_gt_count"] for q in nonempty if q["tier"]==t),
            } for t in sorted(c)
        }
    }, indent=2))


if __name__ == "__main__":
    main()
