#!/usr/bin/env python3
"""
Export patent-ai's `data/cache/*.zst` archive into a SAGE-ingestible JSONL.

Each cache file is a zstd-compressed Google Patents HTML page. We parse the
key fields (title, abstract, inventors, assignees, CPC codes, first claim)
and emit one JSONL row per patent in the format SAGE's `ingest-batch`
expects:

    {"doc_id": <u64>, "text": "<paragraph>"}

The paragraph is hand-crafted to maximise multi-hop / bridge friendliness:
it names ALL co-mentioned entities (inventors + assignee + tech codes) in
one cohesive 300-600 char block, exactly the shape the v7 corpus showed
the writer extracts cleanly with the COVERAGE RULE prompt.

`doc_id` is a deterministic u64 derived from the publication number, so
re-running the exporter is idempotent. A sidecar `patents.map.jsonl` maps
doc_id → publication_number for downstream eval / citation lookup.

Usage:
    python scripts/export_patent_cache_to_sage_jsonl.py \\
        --cache  "C:/Users/User/Desktop/專利系統RUST/patent-ai/data/cache" \\
        --out    examples/eval_patents/docs.jsonl \\
        --map    examples/eval_patents/patents.map.jsonl \\
        [--limit 100]

Dependencies: zstandard (pip install zstandard).  We use stdlib regex
instead of BeautifulSoup/lxml because:
  * Google Patents HTML is regular enough that 5 regexes do the job.
  * One less dep keeps the script trivially portable.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path

try:
    import zstandard as zstd  # type: ignore
except ImportError:
    sys.stderr.write("missing dep: pip install zstandard\n")
    sys.exit(2)


# ----- HTML field extractors -----
# Each returns "" if not found so the composer can safely concatenate.

RE_TITLE = re.compile(r'<meta name="DC\.title" content="([^"]+)"', re.IGNORECASE)
RE_PUBNUM = re.compile(r'<dd itemprop="publicationNumber"[^>]*>([^<]+)</dd>')
RE_ABSTRACT_TAG = re.compile(r"<abstract[^>]*>(.*?)</abstract>", re.DOTALL | re.IGNORECASE)
RE_ABSTRACT_CLASS = re.compile(
    r'class="abstract[^"]*"[^>]*>(.*?)</div>', re.DOTALL | re.IGNORECASE
)
RE_INVENTOR = re.compile(r'<dd itemprop="inventor"[^>]*>([^<]+)</dd>')
RE_ASSIGNEE = re.compile(r'<dd itemprop="assigneeOriginal"[^>]*>([^<]+)</dd>')
RE_CPC = re.compile(r'<span itemprop="Code">([^<]+)</span>')
RE_FILING = re.compile(r'<dd itemprop="filingDate"[^>]*>([^<]+)</dd>')
RE_FIRST_CLAIM = re.compile(
    r'itemprop="claim"[^>]*>\s*<div[^>]*>(.*?)</div>', re.DOTALL | re.IGNORECASE
)

# Collapse runs of whitespace and strip remaining HTML tags from a fragment.
RE_TAG = re.compile(r"<[^>]+>")
RE_WS = re.compile(r"\s+")


def _clean(s: str) -> str:
    s = RE_TAG.sub(" ", s)
    s = RE_WS.sub(" ", s)
    return s.strip()


def parse_one(html: str) -> dict | None:
    """Pull the SAGE-relevant fields. Returns None if title is missing
    (signals a fetch error / robot-challenge page in the cache)."""
    title_m = RE_TITLE.search(html)
    if not title_m:
        return None
    title = _clean(title_m.group(1))

    pubnum_m = RE_PUBNUM.search(html)
    pubnum = pubnum_m.group(1).strip() if pubnum_m else ""

    abstract = ""
    am = RE_ABSTRACT_TAG.search(html) or RE_ABSTRACT_CLASS.search(html)
    if am:
        abstract = _clean(am.group(1))

    inventors = [_clean(m) for m in RE_INVENTOR.findall(html)]
    assignees = [_clean(m) for m in RE_ASSIGNEE.findall(html)]
    # CPC codes: keep only the most specific (longest) 3-5 to limit noise.
    cpc_all = [_clean(m) for m in RE_CPC.findall(html)]
    cpc = sorted(set(cpc_all), key=len, reverse=True)[:5]

    filing_m = RE_FILING.search(html)
    filing = _clean(filing_m.group(1)) if filing_m else ""

    claim1_m = RE_FIRST_CLAIM.search(html)
    claim1 = _clean(claim1_m.group(1)) if claim1_m else ""

    return {
        "publication_number": pubnum,
        "title": title,
        "abstract": abstract,
        "inventors": inventors,
        "assignees": assignees,
        "cpc": cpc,
        "filing": filing,
        "claim1": claim1,
    }


def compose_paragraph(rec: dict, max_chars: int = 1500) -> str:
    """Build a single dense paragraph that names every co-mentioned entity.

    Shape (one paragraph, no newlines):
        "{pubnum}: {title}. Filed {filing} by {inventor_list}.
         Assigned to {assignee_list}. CPC classes: {cpc_list}.
         Abstract: {abstract}.  First claim: {claim1[:300]}."

    Every entity that should later be addressable via a query lives in this
    single string. SAGE's writer (with COVERAGE RULE) extracts one triple
    per named entity → multi-hop queries like "Two patents assigned to
    Qualcomm" or "Inventors of patents in CPC class H04L67" become tractable.
    """
    pub = rec["publication_number"] or "PATENT"
    title = rec["title"]
    parts = [f"{pub}: {title}."]
    if rec["filing"]:
        parts.append(f"Filed {rec['filing']}")
    if rec["inventors"]:
        # Limit to first 5 inventors — many patents have very long lists
        # that just dilute the entity-extraction signal.
        inv = ", ".join(rec["inventors"][:5])
        parts.append(f"by {inv}.")
    elif rec["filing"]:
        parts[-1] = parts[-1] + "."
    if rec["assignees"]:
        parts.append("Assigned to " + ", ".join(rec["assignees"]) + ".")
    if rec["cpc"]:
        parts.append("CPC classes: " + ", ".join(rec["cpc"]) + ".")
    if rec["abstract"]:
        parts.append("Abstract: " + rec["abstract"])
        # Ensure the abstract ends with a period for sentence-count
        if not parts[-1].endswith("."):
            parts[-1] = parts[-1] + "."
    if rec["claim1"]:
        # Claims tend to be huge; first 250 chars captures the protected scope.
        parts.append("First claim: " + rec["claim1"][:250] + "...")
    text = " ".join(parts)
    if len(text) > max_chars:
        text = text[: max_chars - 4] + " ..."
    return text


def stable_doc_id(pubnum: str) -> int:
    """Deterministic u64 doc_id from publication number.

    sha256 → first 8 bytes as big-endian u64. Never returns 0 (SAGE's
    `name_to_id` invariant — 0 is reserved). Collisions are astronomically
    unlikely (~1 in 2^32 pairs at 3225 inputs).
    """
    h = hashlib.sha256(pubnum.encode("utf-8")).digest()
    val = int.from_bytes(h[:8], "big")
    return val if val != 0 else 1


def iter_cache(cache_dir: Path):
    dctx = zstd.ZstdDecompressor()
    for path in sorted(cache_dir.iterdir()):
        if path.suffix != ".zst":
            continue
        try:
            with open(path, "rb") as fh:
                html = dctx.stream_reader(fh).read().decode("utf-8", errors="replace")
        except Exception as e:
            sys.stderr.write(f"[skip] {path.name}: decompress error {e}\n")
            continue
        yield path.name, html


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument(
        "--map",
        type=Path,
        default=None,
        help="Optional sidecar mapping doc_id → publication_number (one JSON per line)",
    )
    ap.add_argument(
        "--limit", type=int, default=0, help="Process at most N cache entries (0 = all)"
    )
    ap.add_argument(
        "--min-abstract-chars",
        type=int,
        default=80,
        help="Skip patents whose abstract is shorter than this (fetched-but-blocked pages)",
    )
    ap.add_argument(
        "--max-text-chars",
        type=int,
        default=1500,
        help="Truncate final paragraph to this many chars",
    )
    ap.add_argument(
        "--filter-html",
        default=None,
        help="Regex; only emit patents whose raw HTML contains this pattern. "
        "Useful for assignee-scoped exports (e.g. 'Peplink|Pismo Labs').",
    )
    args = ap.parse_args()
    filter_re = re.compile(args.filter_html, re.IGNORECASE) if args.filter_html else None

    if not args.cache.is_dir():
        sys.stderr.write(f"cache dir not found: {args.cache}\n")
        sys.exit(1)
    args.out.parent.mkdir(parents=True, exist_ok=True)

    n_seen = n_emit = n_skip_noparse = n_skip_thin = 0
    seen_ids: dict[int, str] = {}

    with open(args.out, "w", encoding="utf-8") as out_fh, (
        open(args.map, "w", encoding="utf-8") if args.map else _nullctx()
    ) as map_fh:
        for fname, html in iter_cache(args.cache):
            if args.limit and n_seen >= args.limit:
                break
            n_seen += 1
            if filter_re is not None and not filter_re.search(html):
                continue
            rec = parse_one(html)
            if rec is None or not rec["publication_number"]:
                n_skip_noparse += 1
                continue
            if len(rec["abstract"]) < args.min_abstract_chars:
                n_skip_thin += 1
                continue
            doc_id = stable_doc_id(rec["publication_number"])
            if doc_id in seen_ids:
                # Two cache files for the same patent (e.g. same number fetched
                # twice). Skip the duplicate — first one wins.
                continue
            seen_ids[doc_id] = rec["publication_number"]
            text = compose_paragraph(rec, max_chars=args.max_text_chars)
            out_fh.write(json.dumps({"doc_id": doc_id, "text": text}, ensure_ascii=False))
            out_fh.write("\n")
            if map_fh is not None:
                map_fh.write(
                    json.dumps(
                        {
                            "doc_id": doc_id,
                            "publication_number": rec["publication_number"],
                            "title": rec["title"],
                            "cache_file": fname,
                        },
                        ensure_ascii=False,
                    )
                )
                map_fh.write("\n")
            n_emit += 1
            if n_emit % 200 == 0:
                sys.stderr.write(f"[progress] {n_emit} patents emitted\n")

    print(
        json.dumps(
            {
                "scanned": n_seen,
                "emitted": n_emit,
                "skipped_unparseable": n_skip_noparse,
                "skipped_thin_abstract": n_skip_thin,
                "out": str(args.out),
                "map": str(args.map) if args.map else None,
            },
            indent=2,
        )
    )


class _nullctx:
    def __enter__(self):
        return None

    def __exit__(self, *a):
        return False


if __name__ == "__main__":
    main()
