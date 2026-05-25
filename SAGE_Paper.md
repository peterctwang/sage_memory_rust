# SAGE: A Self-Evolving Agentic Graph-Memory Engine for Structure-Aware Associative Memory

**ArXiv:** 2605.12061v1 [cs.AI] · 2026-05-12
**Source:** https://arxiv.org/html/2605.12061v1

## Authors
Juntong Wang¹,², Haoyue Zhao³, Guanghui Pan³, Yanbo Wang¹,², Xiyuan Wang¹,², Qiyan Deng³, Muhan Zhang¹

1. Institute for Artificial Intelligence, Peking University
2. School of Intelligence Science and Technology, Peking University
3. School of Computer Science and Technology, Beijing Institute of Technology

---

## Abstract

Memory is a critical bottleneck for language-based agents operating over extended timeframes. Existing RAG and GraphRAG approaches treat memory graphs as static retrieval indexes, which limits their capacity to reconstruct complete reasoning chains from incomplete information and improve themselves through usage feedback.

SAGE introduces a dynamic memory framework coupling two components:

- **Memory Writer** — incrementally constructs graph memory from interaction records.
- **Graph-Foundation-Model Reader** — enables retrieval and feedback generation.

Across multi-hop QA, open-domain retrieval, domain-specific review analysis, and long-term agent benchmarks, SAGE shows measurable improvements in evidence-chain recovery, answer grounding, and retrieval performance. After two self-evolution iterations, it achieves the best average ranking on multi-hop tasks; in zero-shot transfer it reaches **82.5 / 91.6 Recall@2/5** on Natural Questions.

---

## 1. Introduction

Modern LLMs have moved from isolated QA toward agent frameworks supporting multi-turn dialogue, personalization, collaboration, and exploration. The bottleneck has shifted from instantaneous response quality to **sustained memory accumulation, organization, invocation, and refinement**.

### Three Core Challenges

**Challenge I — Associative Reading from Partial Cues.**
Reasoning-chain recovery requires more than semantic similarity. Agent memory often encounters episodic hints, entity aliases, or conceptual references rather than explicit pointers. Vector retrieval finds locally similar content; graph propagation usually activates from query-matched anchor nodes — both can miss the bridge entities required for chain completion.

**Challenge II — Learned Structural Exploitation.**
Graph structure needs contextual interpretation, not fixed rules. Current GraphRAG variants use pre-built communities, predetermined paths, or heuristic expansion. But hub nodes deserve selective expansion; bridge structures must be preserved even when unactivated; noise shortcuts must be distinguished from genuine evidence paths. A structure-aware reader has to **learn** these interpretations.

**Challenge III — Self-Evolving Memory.**
Existing approaches optimize retrieval over static memory. Long-horizon agents need the memory **graph itself** to improve from usage feedback — repeated retrieval failures signal missing structural elements that should be strengthened.

### SAGE Framework

A coupled write–read–update cycle:

1. **Memory Writer** — incrementally constructs and refines graph memory through policy-based sequential decisions.
2. **GFM Reader** — performs query-conditioned activation exploiting learned structural patterns.
3. **Closed-Loop Evolution** — reader retrieval outcomes drive writer improvements.

---

## 2. Related Work

**RAG / GraphRAG.** Non-parametric memory interfaces; variants vary on retrieval timing, reasoning integration, adaptive strategy, and hierarchical organization. GraphRAG handles cross-document dependencies and multi-hop evidence; recent advances include query-focused summarization, heterogeneous node structures, efficiency optimizations, and neurobiologically-inspired methods.

**Agent Memory.** Production-ready systems, agentic mechanisms, temporal knowledge organization, hierarchical structures, conversational persistence. Surveys highlight human-inspired and graph-based architectures; benchmarks evaluate long-term consistency, temporal dynamics, multi-session reasoning, modification, forgetting, and hallucination control.

**Graph Foundation Models.** Large-scale multi-graph pretraining for transferable structural representations across tasks/domains/graphs (cross-network contrastive learning, generative pretraining, augmentation-based contrastive methods).

---

## 3. Preliminary

Given a knowledge-intensive memory sample `x = (q, 𝒟, 𝒟⁺, y)`:

- `q` — query
- `𝒟 = {dᵢ}` — candidate historical memory fragments
- `𝒟⁺ ⊆ 𝒟` — supporting evidence
- `y` — ground-truth answer

**Writer policy.** At step `h`, state `sₕ` → action `aₕ ~ πθ(·|sₕ)`; partial graph updates as `𝒢ₕ₊₁ = 𝒢ₕ ⊕ aₕ`.

**Reader.** Query-conditioned graph propagation yields entity relevance `sₑ = fφ(q, 𝒢) ∈ ℝ^|𝒱ₑ|`, projected to fragment scores. Outputs:

```
(𝒟̂ₖ, 𝒢̂q, Πq) = ℛφ(q, 𝒢, M)
```

where `𝒟̂ₖ = TopK_d∈𝒟 (s_D(d))`, `𝒢̂q` is the activated subgraph, `Πq` is optional relational paths.

**Generation.** `ŷ = LLM(q, 𝒟̂ₖ, Πq)`.

---

## 4. Method

### 4.1 Memory Writer — Graph Writing via Reading Feedback

**State at time t:** `sₜ = (q, 𝒟, 𝒢ₜ₋₁, 𝒟ₜ₋₁^proc)` — query, candidates, partial graph, processed documents.

**Action.** Entity-relation triples `(u, r, v)` with source anchors `(u, source, d)`.

**Reader-aware reward.** Given graph `𝒢` and frozen reader, evidence set `Pₖ(q, 𝒢)` is returned. Reward components:

1. **Sufficiency:** `r_ded(q, y, 𝒢) = 𝕀[Judge(q, y | Pₖ) = Yes]`
2. **Recovery:** `r_rec = |Pₖ ∩ 𝒟⁺| / |𝒟⁺|`
3. **Precision:** `r_pre = |Pₖ ∩ 𝒟⁺| / |Pₖ|`
4. **Answer alignment:** `r_ans = max_{y'∈𝒴(y)} F1(ŷ, y')` where `ŷ = LLM(q, Pₖ)`

**Hybrid task reward:**

```
r_task = (α·r_rec + β·r_pre + γ·r_ded) / (α + β + γ)
```

**Repetition penalty** (prevents graph inflation):

```
ρ_rep(𝒢) = (|𝒯(𝒢)| − |uniq(𝒯(𝒢))|) / |𝒯(𝒢)|
```

**Trajectory return:**

```
R(τ) = r_task(τ) − λ_rep·ρ_rep(𝒢τ) + λ_fmt · Σₜ r_t^fmt
```

Optimization uses clipped GRPO.

---

### 4.2 Memory Reader — Graph Foundation Model Retrieval

Reader must remain stable while the writer continuously evolves the graph. Dense retrievers miss structure; standard GNNs generalize poorly. SAGE uses a GFM reader with multi-graph pretraining for transferable structural priors.

```
fφ(q, 𝒢, 𝒟) = ( pφ(e|q,𝒢),  pφ(d|q,𝒢,𝒟),  𝒢q )
```

#### 4.2.1 Cognition-Inspired Structured Query Planning

A planner `𝒫ω` decomposes the query into associative probes:

```
𝒫ω(q) = ( ℰ_exp, 𝒜, 𝒞_rel, 𝒞_hard, τ, {(q̃_m, α_m, t_m)}_{m=1..M} )
```

Multi-path concurrent retrieval overcomes alias misalignment and bridge-entity loss, reconstructing forgotten implicit relations.

#### 4.2.2 Soft Addressing & Pre-activation

Entry score aggregating stimulus across memory dimensions:

```
sₑ(q) = λ₁·Exact(e, ℰ_exp)
      + λ₂·Alias(e, 𝒜)
      + λ₃·max_{m≤M} cos(Emb(desc(e)), Emb(q̃_m))
      + λ₄·Type(e, τ)
      + λ₅·Cons(e, 𝒞_hard)
      + λ₆·Σ_{ξ∈NER(q)} EL(e | ξ)
```

Attention-style softmax with temperature `T₀`:

```
p₀(e|q) = exp(sₑ/T₀) / Σ_v exp(sᵥ/T₀)
```

Initial node state:

```
hₑ⁽⁰⁾ = (p₀(e|q))^η · Wq·Emb(q)  +  Wₓ·xₑ
```

`xₑ` = solidified long-term entity memory; `p₀` = working-memory context.

#### 4.2.3 Synapse-Inspired Structurally-Conditioned Propagation

**Node features:** `ϕ(v) = [log(1+d_v), c_v, κ_v, d̄_𝒩(v)]`
**Edge-pair features:** `ψ(u,v) = [|d_u−d_v|, |𝒩(u)∩𝒩(v)|, Jaccard]`
**Graph summary:** `r_𝒢 = [mean ϕ; std ϕ; density]`

Edge structural context at layer l:

```
z_uv^(l) = [ E_n^(l)(ϕ(u)); E_n^(l)(ϕ(v)); E_p^(l)(ψ(u,v)); E_g^(l)(r_𝒢) ]
```

Vector gating:

```
g_uv^(l) = 1 + δ · tanh( MLP_g^(l)(z_uv^(l)) )
```

Message & node update:

```
m_{u→v}^(l) = η_uv · g_uv^(l) ⊙ W_m^(l) h_u^(l−1)
h_v^(l)     = LayerNorm( h_v^(l−1) + PReLU( b^(l) + Σ_u m_{u→v}^(l) ) )
```

Effect: suppress generic hub edges, preserve cross-cluster bridges, habituate redundant paths.

#### 4.2.4 Target Graph Calibration & Cross-Graph Priors

Feature prompt vector for calibration:

```
h̃ₑ⁽⁰⁾ = p_f ⊙ hₑ⁽⁰⁾
```

Contextual channel — gated propagation on current graph:

```
H_ctx = F_gate( H̃⁽⁰⁾, 𝒢; Θ_gate )
```

Schema channel — mixture over K structural prompt bases:

```
ω_j^(l) = softmax_j( a^(l) / T_p )
P_schema^(l) = Σ_j ω_j^(l) · P_j^(l)
H_sch = F_prompt( H̃⁽⁰⁾, 𝒢; {P_schema^(l)} )
```

Final entity representation:

```
H(q, 𝒢) = H_ctx + β_sch · H_sch
```

`H_ctx` = current structural state; `H_sch` = stable patterns (bridges, community boundaries, core-periphery, noise rejection).

#### 4.2.5 Reader Training

1. **Structural contrastive pretraining** on augmented graph views.
2. **Supervised fine-tuning** with weighted classification + multi-positive ranking objectives.

---

### 4.3 Writer–Reader Self-Evolution

Each iteration:

1. Fix reader → train writer using retrieval results as rewards.
2. Use updated writer to regenerate graphs → continue reader training.

**Proposition 1 (theoretical consequences).**

1. *Signal-budget efficiency* — soft addressing + structural gating + controlled entity-to-document projection raise evidence-to-noise ratio, reducing top-k requirements.
2. *Context-schema decomposition* — transferable priors plus target calibration mean the reader only needs to correct graph-specific residuals.
3. *Evolution stability* — under bounded graph drift, consecutive writer updates induce bounded document-score changes.

---

## 5. Experiments

### 5.1 End-to-End Effectiveness

**Multi-hop QA** (HotpotQA, MuSiQue, 2WikiMultiHopQA): SAGE attains best average ranking after two self-evolution rounds. **Zero-shot transfer** on Natural Questions reaches **82.5 / 91.6 Recall@2/5**, beating most trained baselines.

**Domain-specific (AmazonQA):** consistent gains over neural baselines, strong improvement through training.

**Retrieval efficiency:** 0.019–0.034 s, vs. GraphRAG 2.759 s and LightRAG 0.861 s.

### 5.2 Long-Term Agent Memory

Evaluated on LongMemEval and HaluMem. SAGE +1 round is competitive — temporal reasoning 49.2 %, multi-session 66.2 %. Remaining gaps: memory updating and comprehensive extraction.

### 5.3 Further Analysis

Subgraph visualizations show interpretable retrieval paths. Writer/reader ablations detailed in Appendices G–H.

---

## 6. Conclusion

SAGE treats memory as a dynamic substrate for **writing, reading, and continuous improvement**. Measurable gains in evidence recovery, answer grounding, and retrieval efficiency establish graph-based self-improving memory as a foundation for long-horizon agent systems.

---

## Key Innovations

1. **Memory Writer** — policy-based graph construction with reader-aware reward, addressing extraction-stage hallucination.
2. **GFM Reader** — structure-aware retrieval exploiting learned topology across domains.
3. **Closed-Loop Evolution** — mutual improvement of writer and reader.
4. **Theoretical Grounding** — signal-budget efficiency, context-schema decomposition, evolution stability.
