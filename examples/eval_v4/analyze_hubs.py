"""Analyze entity hub distribution after ingest.

Helps surface optimization opportunities:
  - Which entities appear in MANY docs (hubs that dilute retrieval)?
  - Which docs have NO unique entities (impossible to retrieve specifically)?

Run after `sage ingest-batch` finishes:
    cargo run -q -p sage-cli -- list --db /tmp/sage_eval_v4.sled \
      --limit 9999 > /tmp/v4_entities.json
    python examples/eval_v4/analyze_hubs.py /tmp/v4_entities.json
"""
import json
import sys
from collections import Counter


def main(path: str) -> None:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    ents = data["entities"]
    doc_appearance = Counter()
    docs_per_ent = []
    for e in ents:
        n = len(e["source_docs"])
        doc_appearance[n] += 1
        docs_per_ent.append((n, e["name"]))

    print(f"total entities: {len(ents)}")
    print(f"entities with embedding: {sum(1 for e in ents if e['has_embedding'])}")
    print()
    print("appearance distribution (entities appearing in N docs):")
    for n in sorted(doc_appearance.keys()):
        bar = "#" * min(doc_appearance[n], 50)
        print(f"  {n:>3} docs -> {doc_appearance[n]:>4}  {bar}")
    print()
    print("top 20 hub entities (appearing in most docs):")
    for n, name in sorted(docs_per_ent, reverse=True)[:20]:
        print(f"  {n:>3} docs -> {name}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/tmp/v4_entities.json")
