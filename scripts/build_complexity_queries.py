#!/usr/bin/env python3
"""Build a complexity-stratified query set for multi-angle SAGE validation.

The existing `queries.json` validates SAGE on 6 ad-hoc tiers. This script
generates a NEW eval suite (`queries_complexity.json`) that systematically
varies complexity along 8 monotone-difficulty levels:

  1. Surface keyword              — 1-token literal match
  2. Multi-token phrase           — 2-3 word specific phrase
  3. Single filter                — 1 constraint (assignee / inventor / CPC)
  4. Dual filter (AND)            — 2 constraints intersect
  5. Triple filter                — 3 constraints intersect
  6. Bridge (2-hop entity chain)  — find X through Y
  7. Multi-entity enumeration     — "Two/Three X" w/ broad GT
  8. Cross-topic intersection     — multiple topic intersections (hardest)

Each query is graded by complexity, kind, and carries GT enumerated from
real corpus data so the eval is grounded.

Usage:
    python scripts/build_complexity_queries.py \
        --docs examples/eval_peplink/docs.jsonl \
        --map  examples/eval_peplink/patents.map.jsonl \
        --out  examples/eval_peplink/queries_complexity.json
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


# Selector spec used by every query. Each query is evaluated against
# the corpus by ANDing whatever selectors are present.
#
# Fields:
#   keyword_any   : doc text contains any of these substrings (OR)
#   keyword_all   : doc text contains ALL of these substrings (AND)
#   exclude_kw    : doc text does NOT contain these (NOT)
#   assignee      : exact match on assignee
#   not_assignee  : exclude this assignee
#   inventor      : exact match on inventor
#   cpc_prefix    : at least one CPC starts with this prefix
#   by_pubnum     : pin to a specific publication number (1 GT doc)

QUERY_SPECS = [
    # =========================================================
    # Complexity 1 — Surface keyword (single literal token)
    # =========================================================
    {"complexity": 1, "kind": "surface-keyword",
     "query": "Bonding",
     "keyword_any": ["bond"]},
    {"complexity": 1, "kind": "surface-keyword",
     "query": "SIM",
     "keyword_any": ["SIM"]},
    {"complexity": 1, "kind": "surface-keyword",
     "query": "Tunnel",
     "keyword_any": ["tunnel"]},
    {"complexity": 1, "kind": "surface-keyword",
     "query": "Overlay",
     "keyword_any": ["overlay"]},
    {"complexity": 1, "kind": "surface-keyword",
     "query": "Failover",
     "keyword_any": ["failover"]},

    # =========================================================
    # Complexity 2 — Multi-token specific phrase
    # =========================================================
    {"complexity": 2, "kind": "multi-token-phrase",
     "query": "Aggregated connection methods",
     "keyword_all": ["aggregated", "connection"]},
    {"complexity": 2, "kind": "multi-token-phrase",
     "query": "Virtual wide area network overlay",
     "keyword_all": ["virtual", "overlay"]},
    {"complexity": 2, "kind": "multi-token-phrase",
     "query": "Multi-WAN load balancing",
     "keyword_all": ["multi-WAN", "load"]},
    {"complexity": 2, "kind": "multi-token-phrase",
     "query": "SIM card management",
     "keyword_all": ["SIM"]},
    {"complexity": 2, "kind": "multi-token-phrase",
     "query": "Wireless load balancing channel selection",
     "keyword_all": ["load balanc"]},

    # =========================================================
    # Complexity 3 — Single filter (1 constraint)
    # =========================================================
    {"complexity": 3, "kind": "single-filter-assignee",
     "query": "Patents from Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd"},
    {"complexity": 3, "kind": "single-filter-assignee",
     "query": "Cisco patents in the corpus",
     "assignee": "Cisco Technology Inc"},
    {"complexity": 3, "kind": "single-filter-assignee",
     "query": "VMware patents in the corpus",
     "assignee": "VMware LLC"},
    {"complexity": 3, "kind": "single-filter-inventor",
     "query": "Patents by Patrick Ho Wai Sung",
     "inventor": "Patrick Ho Wai Sung"},
    {"complexity": 3, "kind": "single-filter-inventor",
     "query": "Patents by Kam Chiu NG",
     "inventor": "Kam Chiu NG"},
    {"complexity": 3, "kind": "single-filter-cpc",
     "query": "Patents in CPC class H04L",
     "cpc_prefix": "H04L"},

    # =========================================================
    # Complexity 4 — Dual filter (AND)
    # =========================================================
    {"complexity": 4, "kind": "dual-filter",
     "query": "Pismo Labs patents about bonding",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["bond"]},
    {"complexity": 4, "kind": "dual-filter",
     "query": "Pismo Labs patents about SIM card",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["SIM"]},
    {"complexity": 4, "kind": "dual-filter",
     "query": "Pismo Labs patents about tunnels",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["tunnel"]},
    {"complexity": 4, "kind": "dual-filter",
     "query": "Cisco patents about overlay",
     "assignee": "Cisco Technology Inc",
     "keyword_all": ["overlay"]},
    {"complexity": 4, "kind": "dual-filter",
     "query": "Patrick Ho Wai Sung patents about bonding",
     "inventor": "Patrick Ho Wai Sung",
     "keyword_all": ["bond"]},
    {"complexity": 4, "kind": "dual-filter",
     "query": "Patents in CPC H04L assigned to Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd",
     "cpc_prefix": "H04L"},

    # =========================================================
    # Complexity 5 — Triple filter
    # =========================================================
    {"complexity": 5, "kind": "triple-filter",
     "query": "Pismo Labs bonding patents in CPC H04L",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["bond"],
     "cpc_prefix": "H04L"},
    {"complexity": 5, "kind": "triple-filter",
     "query": "Pismo Labs SIM patents in CPC H04W",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["SIM"],
     "cpc_prefix": "H04W"},
    {"complexity": 5, "kind": "triple-filter",
     "query": "Patrick Ho Wai Sung tunnel patents at Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd",
     "inventor": "Patrick Ho Wai Sung",
     "keyword_all": ["tunnel"]},
    {"complexity": 5, "kind": "triple-filter",
     "query": "Pismo Labs aggregated-connection patents in CPC H04L",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["aggregated"],
     "cpc_prefix": "H04L"},
    {"complexity": 5, "kind": "triple-filter",
     "query": "Kam Chiu NG bonding patents at Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd",
     "inventor": "Kam Chiu NG",
     "keyword_all": ["bond"]},

    # =========================================================
    # Complexity 6 — Bridge / 2-hop reasoning
    # =========================================================
    {"complexity": 6, "kind": "bridge",
     "query": "Inventors of WAN bonding patents at Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["bond"],
     "_note": "Bridge: bonding-keyword + Pismo Labs → patents → inventor entities"},
    {"complexity": 6, "kind": "bridge",
     "query": "CPC classes covered by Pismo Labs SIM patents",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["SIM"]},
    {"complexity": 6, "kind": "bridge",
     "query": "Patents in same CPC family as bonding-throughput patents",
     "keyword_all": ["bond", "throughput"]},
    {"complexity": 6, "kind": "bridge",
     "query": "Co-inventors with Patrick Ho Wai Sung at Pismo Labs",
     "inventor": "Patrick Ho Wai Sung"},
    {"complexity": 6, "kind": "bridge",
     "query": "Pismo Labs patents about both bonding and tunnels",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["bond", "tunnel"]},

    # =========================================================
    # Complexity 7 — Multi-entity enumeration
    # =========================================================
    {"complexity": 7, "kind": "multi-entity",
     "query": "Three Pismo Labs patents about aggregated connection",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["aggregated"]},
    {"complexity": 7, "kind": "multi-entity",
     "query": "Two patents about SIM card from Pismo Labs",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["SIM"]},
    {"complexity": 7, "kind": "multi-entity",
     "query": "Multiple Cisco patents about overlay networks",
     "assignee": "Cisco Technology Inc",
     "keyword_all": ["overlay"]},
    {"complexity": 7, "kind": "multi-entity",
     "query": "Two patents invented by Patrick Ho Wai Sung",
     "inventor": "Patrick Ho Wai Sung"},
    {"complexity": 7, "kind": "multi-entity",
     "query": "Three Pismo Labs patents involving tunnels",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["tunnel"]},

    # =========================================================
    # Complexity 8 — Cross-topic intersection (hardest)
    # =========================================================
    {"complexity": 8, "kind": "cross-topic",
     "query": "Pismo Labs patents about bonding AND failover",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["bond", "failover"]},
    {"complexity": 8, "kind": "cross-topic",
     "query": "Patents about SIM AND tunnel AND bonding",
     "keyword_all": ["SIM", "tunnel", "bond"]},
    {"complexity": 8, "kind": "cross-topic",
     "query": "Pismo Labs aggregated-connection patents with SIM management",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["aggregated", "SIM"]},
    {"complexity": 8, "kind": "cross-topic",
     "query": "Patents about both overlay networks and load balancing",
     "keyword_all": ["overlay", "load"]},
    {"complexity": 8, "kind": "cross-topic",
     "query": "Pismo Labs patents covering CPC H04L AND H04W with SIM",
     "assignee": "Pismo Labs Technology Ltd",
     "keyword_all": ["SIM"],
     "cpc_prefix": "H04L"},
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--docs", required=True, type=Path)
    ap.add_argument("--map", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    docs = [json.loads(l) for l in open(args.docs, encoding="utf-8")]
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
                if ok and "exclude_kw" in spec:
                    if any(k.lower() in tl for k in spec["exclude_kw"]):
                        ok = False
                if ok and "assignee" in spec:
                    if spec["assignee"] not in assignees_of(text):
                        ok = False
                if ok and "not_assignee" in spec:
                    if spec["not_assignee"] in assignees_of(text):
                        ok = False
                if ok and "inventor" in spec:
                    if spec["inventor"] not in inventors_of(text):
                        ok = False
                if ok and "cpc_prefix" in spec:
                    if not any(c.startswith(spec["cpc_prefix"]) for c in cpcs_of(text)):
                        ok = False
                if ok:
                    gt.append(d["doc_id"])

        # NOTE: sage-cli eval format expects {"query", "ground_truth"}.
        # Use synthetic tier number = complexity for backward compat
        # (the eval driver doesn't care about tier semantics).
        out = {
            "tier": spec["complexity"],
            "kind": spec["kind"],
            "query": spec["query"],
            "ground_truth": gt,
            "_gt_count": len(gt),
            "_complexity": spec["complexity"],
        }
        if "_note" in spec:
            out["_note"] = spec["_note"]
        queries.append(out)

    nonempty = [q for q in queries if q["ground_truth"]]
    dropped = len(queries) - len(nonempty)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(nonempty, f, indent=2, ensure_ascii=False)

    from collections import Counter
    by_complexity = Counter(q["_complexity"] for q in nonempty)
    gt_stats = {
        c: {
            "n_queries": by_complexity[c],
            "gt_min": min(q["_gt_count"] for q in nonempty if q["_complexity"] == c),
            "gt_max": max(q["_gt_count"] for q in nonempty if q["_complexity"] == c),
        }
        for c in sorted(by_complexity)
    }
    print(json.dumps({
        "out": str(args.out),
        "total_queries": len(nonempty),
        "dropped_zero_gt": dropped,
        "by_complexity": gt_stats,
    }, indent=2))


if __name__ == "__main__":
    main()
