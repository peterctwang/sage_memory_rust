# Peplink Patent Graph — Cross-Document Insights

> Generated from SAGE graph at `C:\Users\User\AppData\Local\Temp\sage_peplink.sled` covering 91 patents.
> Each insight composes 1-N queries / aggregations across the multi-document
> entity graph. NOT a benchmark — a qualitative demonstration of synthesis.

## 1. Pismo Labs's most prolific inventors

**Method**: Aggregate inventor counts across all 40 Pismo-Labs-assigned patents.

| rank | inventor | patent_count |
|---|---|---|
| 1 | Patrick Ho Wai Sung | 28 |
| 2 | Kam Chiu NG | 26 |
| 3 | Wan Chun Leung | 16 |
| 4 | Ho Ming Chan | 15 |
| 5 | Kit Wai Chau | 12 |
| 6 | Alex Wing Hong Chan | 11 |
| 7 | Ming Pui Chong | 4 |
| 8 | Ying Kwan | 4 |
| 9 | Chan Neng Leong | 3 |
| 10 | Uzair Ahmed CHUGHTAI | 2 |

**Insight**: Top contributor: **Patrick Ho Wai Sung** with 28 patents — 70% of Pismo's patent volume. Top-3 inventors together cover 53% of all inventor-patent links.

---

## 2. Pismo Labs CPC technology territory

**Method**: Count CPC code 5-char prefixes across Pismo's 40 patents.

| rank | cpc_prefix | patent_count |
|---|---|---|
| 1 | H04L1 | 70 |
| 2 | H04L4 | 53 |
| 3 | H04L6 | 32 |
| 4 | G09G2 | 15 |
| 5 | H04L2 | 13 |
| 6 | H04M1 | 5 |
| 7 | H04W7 | 3 |
| 8 | H04W8 | 2 |

**Insight**: Strongest territory: **H04L1** (70 patents, 35% of Pismo's CPC-tag mass). Top-3 prefixes cover 78%.

---

## 3. Pismo Labs vs Cisco — technology territory overlap

**Method**: Count keyword hits in each company's patent text. Overlap = both > 0.

| topic | pismo_patents | cisco_patents |
|---|---|---|
| SIM | 39 | 8 |
| VPN | 39 | 5 |
| tunnel | 37 | 2 |
| throughput | 27 | 6 |
| encryption | 29 | 3 |
| bond | 31 | 0 |
| aggregated connection | 15 | 0 |
| load balanc | 6 | 5 |
| failover | 6 | 0 |
| overlay | 0 | 1 |

**Insight**: **Direct competitive overlap**: 6 topic areas where both Pismo Labs AND Cisco have patents — SIM, VPN, tunnel, throughput, encryption. **Pismo-exclusive**: 3 topics — bond, aggregated connection, failover. The bonding / aggregated-connection territory is largely Pismo's, while overlay / VPN sees direct Cisco competition.

---

## 4. Pismo Labs SIM card specialist

**Method**: Filter to {assignee=Pismo Labs, text contains 'SIM'}; count inventors. Cross-check by SAGE query.

| inventor | sim_patents |
|---|---|
| Wan Chun Leung | 8 |
| Kit Wai Chau | 3 |
| Ming Pui Chong | 3 |
| Chan Neng Leong | 3 |
| Patrick Ho Wai Sung | 3 |

**SAGE-retrieved evidence**:
- {'inventor': 'Wan Chun Leung', 'appearances_in_top5': 3}
- {'inventor': 'Ming Pui Chong', 'appearances_in_top5': 2}
- {'inventor': 'Chan Neng Leong', 'appearances_in_top5': 2}

**Insight**: **SIM specialist: Wan Chun Leung** with 8 SIM-mentioning Pismo Labs patents. SAGE's own retrieval for 'SIM card patents at Pismo Labs' returns top-5 docs co-authored by Wan Chun Leung (3/5 appearances) — consistent with the manual count.

---

## 5. Tightest co-inventor collaborations

**Method**: Count pairwise co-authorship across all 91 patents (sort by frequency).

| rank | pair | co_authored |
|---|---|---|
| 1 | Kam Chiu NG ↔ Patrick Ho Wai Sung | 26 |
| 2 | Ho Ming Chan ↔ Kam Chiu NG | 11 |
| 3 | Ho Ming Chan ↔ Patrick Ho Wai Sung | 11 |
| 4 | Kam Chiu NG ↔ Wan Chun Leung | 11 |
| 5 | Patrick Ho Wai Sung ↔ Wan Chun Leung | 11 |
| 6 | Kam Chiu NG ↔ Kit Wai Chau | 9 |
| 7 | Kit Wai Chau ↔ Patrick Ho Wai Sung | 9 |
| 8 | Alex Wing Hong Chan ↔ Kam Chiu NG | 6 |
| 9 | Alex Wing Hong Chan ↔ Patrick Ho Wai Sung | 6 |
| 10 | Ho Ming Chan ↔ Kit Wai Chau | 5 |

**Insight**: Tightest pair: **Kam Chiu NG ↔ Patrick Ho Wai Sung** co-authored 26 patents. This is the strongest team signal in the corpus — likely a core R&D dyad at Pismo Labs.

---

## 6. Pismo Labs's strategic IP moat

**Method**: For each tech keyword, ratio = (Pismo patents) / (other assignees' patents + 1). Higher ratio = stronger moat.

| topic | pismo_patents | other_patents | moat_ratio |
|---|---|---|---|
| aggregated connection | 15 | 0 | 15.0 |
| throughput optim | 10 | 0 | 10.0 |
| bond | 31 | 8 | 3.44 |
| tunnel | 37 | 37 | 0.97 |
| SIM | 39 | 51 | 0.75 |
| multi-WAN | 2 | 2 | 0.67 |
| load balanc | 6 | 30 | 0.19 |

**Insight**: **Strongest moat: 'aggregated connection'** — 15 Pismo patents vs 0 others (ratio 15.0×). Pismo Labs dominates this technology area in the corpus.

---

## 7. SAGE-retrieved cross-assignee coverage of 'aggregated connection'

**Method**: `sage query --k 10 'aggregated connection methods for transmitting data'` → bucket top-10 by assignee.

| assignee | docs_in_top10 |
|---|---|
| Pismo Labs Technology Ltd | 8 |
| Cisco Technology Inc | 1 |
| Juniper Networks Inc | 1 |

**Insight**: SAGE-retrieved top-10 docs span 3 distinct assignees — Pismo Labs Technology Ltd (8), Cisco Technology Inc (1), Juniper Networks Inc (1). This is genuine cross-assignee synthesis the graph enables.

---
