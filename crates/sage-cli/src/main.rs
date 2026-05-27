//! `sage` — CLI for the SAGE memory framework.
//!
//! Subcommands:
//!   `sage demo`             Run the canned ingest+query scenario (in-memory).
//!   `sage stats --db <p>`   Print entity/edge counts in a sled-backed graph.
//!   `sage query --db <p> <text>`  Query a sled-backed graph and print top-k docs.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sage_core::VectorIndex;
use sage_core::{EntityScan, EntityType, GraphStore, Query, Reader, ReaderGraph, TenantId};
use sage_embed::{DeterministicEmbedder, HnswIndex};
use sage_eval::{EvalRunner, EvalSample};
use sage_graph::{MemGraphStore, SledGraphStore};
use sage_llm::{
    ClaudeCliLlm, CodexCliLlm, FallbackLlm, GeminiCliLlm, HeuristicRouter, LlmClient, MinimaxLlm,
    MockLlm,
};
use sage_reader::{AddressingWeights, HeuristicReader, LlmQueryPlanner};
use sage_runtime::SageEngine;
use sage_writer::{
    apply_action, apply_action_embedded, EntityRef, LlmWriterPolicy, WriterAction, WriterPolicy,
    WriterState,
};
use serde::Deserialize;
use smallvec::SmallVec;
use smol_str::SmolStr;

#[derive(Parser, Debug)]
#[command(name = "sage", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run a canned in-memory demo (no external LLM, no disk).
    Demo,
    /// Show entity/edge counts for a sled-backed graph.
    Stats {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
    },
    /// Query a sled-backed graph (uses heuristic reader + deterministic embedder).
    Query {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 5)]
        k: usize,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
        /// Query text. Use quotes for multi-word.
        text: String,
    },
    /// Ingest pre-parsed triples (no LLM) into a sled-backed graph.
    /// Reads JSON envelope from stdin:
    ///   {"triples":[{"src":"A","rel":"r","dst":"B"}], "stop":true}
    IngestStub {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        doc_id: u64,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
    },
    /// Run retrieval eval against a sled-backed graph.
    /// Reads a JSON array from stdin:
    ///   [{"query":"Who founded Acme?","ground_truth":[1]}, ...]
    /// Prints aggregated Recall@k / Precision@k / F1@k / MRR as JSON.
    Eval {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 5)]
        k: usize,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
        /// Use an LLM to expand each query (LlmQueryPlanner). ~1-2s LLM call
        /// per unique query; cached within run.
        #[arg(long, default_value_t = false)]
        llm_plan: bool,
        /// Which backend to use when --llm-plan is set: claude-cli or minimax.
        /// `minimax` auto-composes Claude fallback when SAGE_CLAUDE_BIN is set.
        #[arg(long, default_value = "claude-cli")]
        planner_llm: String,
        /// Override AddressingWeights for tuning sweeps. Format:
        ///   "lambda_exact,lambda_alias,lambda_cos,lambda_type,lambda_cons,lambda_ner_el[,T0]"
        /// Example: --weights "1.0,0.6,0.8,0.3,0.1,0.8"
        #[arg(long)]
        weights: Option<String>,
    },
    /// Ingest a real document via LLM-extracted triples into a sled-backed graph.
    ///
    /// Uses the local `claude` binary by default. Honors $SAGE_CLAUDE_BIN to
    /// override the binary path (useful for Windows .cmd shims). Doc text is
    /// taken from --doc; if omitted, stdin is read.
    Ingest {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        doc_id: u64,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
        #[arg(long)]
        doc: Option<String>,
        /// LLM backend: claude-cli (default) or mock (testing only — requires --mock-response).
        #[arg(long, default_value = "claude-cli")]
        llm: String,
        /// Pre-canned LLM response (only honored when --llm=mock).
        #[arg(long)]
        mock_response: Option<String>,
    },
    /// Batch ingest from a JSONL file.
    ///
    /// Each line: {"doc_id": N, "text": "..."}
    /// Lines failing JSON parse or LLM extraction are recorded in the summary
    /// but do not abort the batch.
    IngestBatch {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        jsonl: PathBuf,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
        #[arg(long, default_value = "claude-cli")]
        llm: String,
    },
    /// List entities in a sled-backed graph.
    List {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 0)]
        tenant: u64,
        /// Case-insensitive substring filter on entity name/aliases.
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "warn,sage=info".parse().expect("static filter must parse"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Demo => run_demo().await,
        Cmd::Stats { db, tenant } => run_stats(db, TenantId(tenant)).await,
        Cmd::Query {
            db,
            k,
            tenant,
            text,
        } => run_query(db, TenantId(tenant), k, text).await,
        Cmd::IngestStub { db, doc_id, tenant } => {
            run_ingest_stub(db, TenantId(tenant), doc_id).await
        }
        Cmd::Eval {
            db,
            k,
            tenant,
            llm_plan,
            planner_llm,
            weights,
        } => run_eval(db, TenantId(tenant), k, llm_plan, &planner_llm, weights).await,
        Cmd::Ingest {
            db,
            doc_id,
            tenant,
            doc,
            llm,
            mock_response,
        } => run_ingest(db, TenantId(tenant), doc_id, doc, &llm, mock_response).await,
        Cmd::IngestBatch {
            db,
            jsonl,
            tenant,
            llm,
        } => run_ingest_batch(db, jsonl, TenantId(tenant), &llm).await,
        Cmd::List {
            db,
            tenant,
            name,
            limit,
        } => run_list(db, TenantId(tenant), name, limit).await,
    }
}

async fn run_ingest(
    db_path: PathBuf,
    t: TenantId,
    doc_id: u64,
    doc: Option<String>,
    llm_kind: &str,
    mock_response: Option<String>,
) -> Result<()> {
    use std::io::Read;
    let text = if let Some(s) = doc {
        s
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read stdin")?;
        if buf.trim().is_empty() {
            anyhow::bail!("--doc not given and stdin is empty");
        }
        buf
    };

    let store = SledGraphStore::open(&db_path).context("open sled")?;
    let document = sage_core::Document::new(doc_id, text);
    let state = WriterState {
        query: None,
        candidates: &[],
        processed: &[],
        step: 0,
    };

    let action = if llm_kind == "mock" {
        let resp =
            mock_response.ok_or_else(|| anyhow::anyhow!("--llm=mock requires --mock-response"))?;
        let m = Arc::new(MockLlm::new());
        m.push(resp);
        let policy = LlmWriterPolicy::new(m);
        policy.step(&state, &document).await?
    } else {
        // Delegate real LLM backends to the shared helper so claude/codex/
        // gemini/minimax all flow through the same construction + logging
        // logic. Single-doc ingest gets the same Claude-fallback wiring as
        // batch ingest when --llm=minimax.
        let llm = build_llm_client(llm_kind)?;
        tracing::info!(llm_kind, doc_id, "ingest via shared LLM client");
        let policy = LlmWriterPolicy::new(llm);
        policy
            .step(&state, &document)
            .await
            .context("LlmWriterPolicy step failed")?
    };

    // Use the same DeterministicEmbedder dim as `query` / `eval` so cos similarity
    // can light up at retrieval time. Without this, all entities land with
    // has_embedding=false and λ_cos collapses to 0.
    let embedder = DeterministicEmbedder::new(128);
    let report = apply_action_embedded(&store, Some(&embedder), t, &action).await?;
    let json = serde_json::json!({
        "doc_id":           doc_id,
        "tenant":           t.0,
        "llm":              llm_kind,
        "triples_extracted": action.triples.len(),
        "stop":             action.stop,
        "entities_added":   report.entities_added,
        "edges_added":      report.edges_added,
        "triples_skipped":  report.triples_skipped,
        "embedded":         true,
    });
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

#[derive(Deserialize)]
struct JsonlRow {
    doc_id: u64,
    text: String,
}

#[allow(clippy::too_many_lines)] // CLI handler — control flow stays linear on purpose
async fn run_ingest_batch(
    db_path: PathBuf,
    jsonl: PathBuf,
    t: TenantId,
    llm_kind: &str,
) -> Result<()> {
    use std::io::BufRead;
    let jsonl_display = jsonl.display().to_string();
    let file =
        std::fs::File::open(&jsonl).with_context(|| format!("open jsonl {jsonl_display}"))?;
    let reader = std::io::BufReader::new(file);

    let store = SledGraphStore::open(&db_path).context("open sled")?;
    let embedder = DeterministicEmbedder::new(128);

    // `minimax` automatically composes with Claude as fallback when
    // SAGE_CLAUDE_BIN is set — empty/erroring MiniMax calls fall through
    // so we never lose a doc to the ~5-10% slop.
    if llm_kind == "mock" {
        anyhow::bail!(
            "--llm mock not supported for ingest-batch; use per-doc 'sage ingest --llm mock'"
        );
    }
    let llm_client = build_llm_client(llm_kind)?;
    let policy = LlmWriterPolicy::new(llm_client.clone());

    let mut total_docs = 0usize;
    let mut total_ok = 0usize;
    let mut total_entities = 0usize;
    let mut total_edges = 0usize;
    let mut total_skipped = 0usize;
    let mut failures: Vec<serde_json::Value> = Vec::new();

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read line {}", line_no + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        total_docs += 1;
        let row: JsonlRow = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                failures.push(serde_json::json!({
                    "line": line_no + 1,
                    "stage": "parse",
                    "error": e.to_string(),
                }));
                continue;
            }
        };
        let doc = sage_core::Document::new(row.doc_id, row.text);
        let state = WriterState {
            query: None,
            candidates: &[],
            processed: &[],
            step: 0,
        };
        let action = match policy.step(&state, &doc).await {
            Ok(a) => a,
            Err(e) => {
                failures.push(serde_json::json!({
                    "line": line_no + 1,
                    "doc_id": row.doc_id,
                    "stage": "llm",
                    "error": e.to_string(),
                }));
                continue;
            }
        };
        match apply_action_embedded(&store, Some(&embedder), t, &action).await {
            Ok(r) => {
                total_ok += 1;
                total_entities += r.entities_added;
                total_edges += r.edges_added;
                total_skipped += r.triples_skipped;
                tracing::info!(
                    doc_id = row.doc_id,
                    entities = r.entities_added,
                    edges = r.edges_added,
                    "ingest-batch row applied"
                );
                // Per-row stderr heartbeat (defense against silent hangs —
                // 2026-05-26 incident). Flushed immediately so the user can
                // tell the pipeline is alive without enabling tracing.
                let mut err = std::io::stderr().lock();
                let _ = std::io::Write::write_all(
                    &mut err,
                    format!(
                        "[ingest-batch] doc {} ok ({}/{}) entities+={} edges+={} totals e={} ed={}\n",
                        row.doc_id,
                        total_ok,
                        total_docs,
                        r.entities_added,
                        r.edges_added,
                        total_entities,
                        total_edges
                    ).as_bytes(),
                );
                let _ = std::io::Write::flush(&mut err);
            }
            Err(e) => {
                failures.push(serde_json::json!({
                    "line": line_no + 1,
                    "doc_id": row.doc_id,
                    "stage": "apply",
                    "error": e.to_string(),
                }));
            }
        }
    }

    let summary = serde_json::json!({
        "tenant":           t.0,
        "llm":              llm_kind,
        "docs_seen":        total_docs,
        "docs_ingested":    total_ok,
        "entities_added":   total_entities,
        "edges_added":      total_edges,
        "triples_skipped":  total_skipped,
        "failures":         failures,
    });
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn run_list(
    db_path: PathBuf,
    t: TenantId,
    needle: Option<String>,
    limit: usize,
) -> Result<()> {
    let g = SledGraphStore::open(&db_path).context("open sled")?;
    let needle_lc = needle.map(|s| s.to_lowercase());
    let ents = g.all_entities(t).await?;
    let filtered: Vec<_> = ents
        .into_iter()
        .filter(|e| match &needle_lc {
            None => true,
            Some(n) => {
                e.name.to_lowercase().contains(n.as_str())
                    || e.aliases
                        .iter()
                        .any(|a| a.to_lowercase().contains(n.as_str()))
            }
        })
        .take(limit)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "name": e.name.as_str(),
                "etype": format!("{:?}", e.etype),
                "aliases": e.aliases.iter().map(smol_str::SmolStr::as_str).collect::<Vec<_>>(),
                "has_embedding": e.embedding.is_some(),
                "source_docs": e.source_docs.iter().copied().collect::<Vec<_>>(),
            })
        })
        .collect();
    let out = serde_json::json!({
        "tenant": t.0,
        "count":  filtered.len(),
        "entities": filtered,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[derive(Deserialize)]
struct EvalSampleJson {
    query: String,
    ground_truth: Vec<sage_core::DocId>,
}

/// Walk all entities in the sled store and rebuild an in-memory HnswIndex.
/// Cost: O(N) inserts on startup, then O(log N) per search. For demo /
/// dev-scale graphs (< 1M entities) the rebuild is negligible compared to
/// LLM-driven ingest. Production should persist the index — see issue #TBD.
async fn build_index_for_query(
    store: &SledGraphStore,
    t: TenantId,
    dim: usize,
) -> Result<Arc<dyn VectorIndex>> {
    let idx = HnswIndex::new(dim);
    let mut indexed = 0usize;
    for entity in store.all_entities(t).await? {
        if let Some(emb) = &entity.embedding {
            if emb.len() == dim {
                idx.insert(entity.id, emb)?;
                indexed += 1;
            }
        }
    }
    tracing::info!(
        indexed,
        tenant = t.0,
        "built in-memory HnswIndex for query path"
    );
    Ok(Arc::new(idx))
}

fn build_claude_cli() -> ClaudeCliLlm {
    let mut c = ClaudeCliLlm::new();
    if let Ok(p) = std::env::var("SAGE_CLAUDE_BIN") {
        c = c.with_binary(p);
    }
    c
}

/// Build a configured LlmClient. `"claude-cli"` returns Claude only;
/// `"minimax"` returns MiniMax+Claude-fallback if both are configured,
/// MiniMax only otherwise.
fn build_llm_client(kind: &str) -> Result<Arc<dyn LlmClient>> {
    match kind {
        "claude-cli" => Ok(Arc::new(build_claude_cli())),
        "codex-cli" => {
            eprintln!("[sage] LLM: Codex CLI (account default model)");
            Ok(Arc::new(CodexCliLlm::from_env()))
        }
        "gemini-cli" => {
            let g = GeminiCliLlm::from_env();
            eprintln!("[sage] LLM: Gemini CLI ({})", g.model());
            Ok(Arc::new(g))
        }
        "minimax" => {
            let mm =
                MinimaxLlm::from_env().context("MinimaxLlm::from_env (need MINIMAX_API_KEY)")?;
            if std::env::var("SAGE_CLAUDE_BIN").is_ok() {
                let claude = Arc::new(build_claude_cli()) as Arc<dyn LlmClient>;
                eprintln!("[sage] LLM: MiniMax (primary) + Claude (fallback on empty/error)");
                Ok(Arc::new(FallbackLlm::new(
                    Arc::new(mm) as Arc<dyn LlmClient>,
                    claude,
                )))
            } else {
                eprintln!("[sage] LLM: MiniMax (no fallback — SAGE_CLAUDE_BIN unset)");
                Ok(Arc::new(mm))
            }
        }
        "router" => {
            // Heuristic two-arm router. Defaults derived from eval_v7:
            //   light = minimax  (fast + cheap, fine on simple docs)
            //   deep  = codex-cli (top multi-hop / bridge scorer)
            // Override via env: SAGE_ROUTER_LIGHT_LLM, SAGE_ROUTER_DEEP_LLM,
            // SAGE_ROUTER_THRESHOLD.
            let light_kind = std::env::var("SAGE_ROUTER_LIGHT_LLM")
                .unwrap_or_else(|_| "minimax".to_string());
            let deep_kind = std::env::var("SAGE_ROUTER_DEEP_LLM")
                .unwrap_or_else(|_| "codex-cli".to_string());
            if light_kind == "router" || deep_kind == "router" {
                anyhow::bail!("router arms cannot themselves be 'router' (no recursion allowed)");
            }
            let light = build_llm_client(&light_kind)
                .with_context(|| format!("constructing router LIGHT arm = {light_kind}"))?;
            let deep = build_llm_client(&deep_kind)
                .with_context(|| format!("constructing router DEEP arm = {deep_kind}"))?;
            let mut router = HeuristicRouter::new(light, deep);
            if let Ok(t) = std::env::var("SAGE_ROUTER_THRESHOLD") {
                if let Ok(n) = t.parse::<u32>() {
                    router = router.with_threshold(n);
                }
            }
            eprintln!(
                "[sage] LLM: Router(light={light_kind}, deep={deep_kind}, threshold={})",
                router.threshold()
            );
            Ok(Arc::new(router))
        }
        other => anyhow::bail!(
            "unknown llm kind '{other}'; use 'claude-cli' / 'codex-cli' / 'gemini-cli' / 'minimax' / 'router'"
        ),
    }
}

fn parse_weights(s: &str) -> Result<AddressingWeights> {
    let parts: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("parsing --weights {s:?}"))?;
    if parts.len() < 6 || parts.len() > 7 {
        anyhow::bail!(
            "--weights expects 6 lambdas + optional T0 (7 values total); got {}",
            parts.len()
        );
    }
    let mut lambdas = [0.0f32; 6];
    lambdas.copy_from_slice(&parts[..6]);
    let t0 = if parts.len() == 7 { parts[6] } else { 0.7 };
    Ok(AddressingWeights {
        lambdas,
        t0,
        eta: 0.5,
    })
}

async fn run_eval(
    db_path: PathBuf,
    t: TenantId,
    k: usize,
    llm_plan: bool,
    planner_llm: &str,
    weights: Option<String>,
) -> Result<()> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .context("read stdin")?;
    let raw: Vec<EvalSampleJson> = serde_json::from_str(&s).context("stdin not a sample array")?;
    let samples: Vec<EvalSample> = raw
        .into_iter()
        .map(|r| EvalSample {
            query: Query::ask(r.query).with_k(k),
            ground_truth: r.ground_truth,
        })
        .collect();

    let g: Arc<SledGraphStore> = Arc::new(SledGraphStore::open(&db_path).context("open sled")?);
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(128));
    let index = build_index_for_query(g.as_ref(), t, 128).await?;

    let resolved_weights = match weights.as_deref() {
        Some(s) => parse_weights(s)?,
        None => AddressingWeights::default(),
    };
    tracing::info!(weights = ?resolved_weights, "AddressingWeights resolved");

    let report = if llm_plan {
        tracing::info!(planner_llm, "eval using LlmQueryPlanner");
        let planner = LlmQueryPlanner::new(build_llm_client(planner_llm)?);
        let reader = Arc::new(
            HeuristicReader::with_planner(planner)
                .with_embedder(embedder)
                .with_vector_index(index)
                .with_weights(resolved_weights),
        );
        EvalRunner::new(reader, k)
            .with_tenant(t)
            .run(g.as_ref(), &samples)
            .await?
    } else {
        let reader = Arc::new(
            HeuristicReader::default()
                .with_embedder(embedder)
                .with_vector_index(index)
                .with_weights(resolved_weights),
        );
        EvalRunner::new(reader, k)
            .with_tenant(t)
            .run(g.as_ref(), &samples)
            .await?
    };
    let out = serde_json::json!({
        "samples":        report.samples,
        "k":              report.k,
        "recall_at_k":    report.recall_at_k,
        "precision_at_k": report.precision_at_k,
        "f1_at_k":        report.f1_at_k,
        "mrr":            report.mrr,
        "planner":        if llm_plan { "llm" } else { "heuristic" },
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[derive(Deserialize)]
struct StubTriple {
    src: String,
    rel: String,
    dst: String,
}

#[derive(Deserialize)]
struct StubEnvelope {
    triples: Vec<StubTriple>,
}

async fn run_ingest_stub(db_path: PathBuf, t: TenantId, doc_id: u64) -> Result<()> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .context("read stdin")?;
    let env: StubEnvelope = serde_json::from_str(&s).context("stdin not JSON envelope")?;

    let mut triples: SmallVec<[(EntityRef, SmolStr, EntityRef); 8]> = SmallVec::new();
    for t in env.triples {
        triples.push((
            EntityRef::New {
                name: SmolStr::new(t.src.trim()),
                etype: EntityType::Concept,
                desc: None,
            },
            SmolStr::new(t.rel.trim()),
            EntityRef::New {
                name: SmolStr::new(t.dst.trim()),
                etype: EntityType::Concept,
                desc: None,
            },
        ));
    }
    let action = WriterAction {
        triples,
        source: doc_id,
        stop: true,
    };

    let store = SledGraphStore::open(&db_path).context("open sled")?;
    let report = apply_action(&store, t, &action).await?;
    let out = serde_json::json!({
        "doc_id": doc_id,
        "tenant": t.0,
        "entities_added": report.entities_added,
        "edges_added":    report.edges_added,
        "triples_skipped": report.triples_skipped,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

async fn run_demo() -> Result<()> {
    let llm = Arc::new(MockLlm::new());
    llm.push(
        r#"{"triples":[{"src":"Alice Liddell","rel":"founded","dst":"Acme Industries"}],"stop":true}"#,
    );
    llm.push(
        r#"{"triples":[{"src":"Bob Jones","rel":"works_at","dst":"Globex Holdings"}],"stop":true}"#,
    );
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(128));
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default().with_embedder(Arc::clone(&embedder)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder);

    let report = engine
        .ingest(vec![
            sage_core::Document::new(1, "Alice Liddell founded Acme Industries."),
            sage_core::Document::new(2, "Bob Jones works at Globex Holdings."),
        ])
        .await
        .context("ingest failed")?;
    println!("INGEST: {report:?}");

    let q = Query::ask("Who founded Acme?");
    let out = engine.query(&q).await.context("query failed")?;
    println!(
        "QUERY top-{} for {:?}:",
        out.docs.len().min(3),
        q.text.as_ref()
    );
    for (i, (doc, score)) in out.docs.iter().take(3).enumerate() {
        println!("  #{}  doc={}  score={:.4}", i + 1, doc, score);
    }
    Ok(())
}

async fn run_stats(db_path: PathBuf, t: TenantId) -> Result<()> {
    let g = SledGraphStore::open(&db_path).context("open sled")?;
    let ents = g.all_entities(t).await?;
    let mut edge_count = 0usize;
    for e in &ents {
        edge_count += g.neighbors(t, e.id, usize::MAX).await?.len();
    }
    let json = serde_json::json!({
        "db_path": db_path,
        "tenant":  t.0,
        "entities": ents.len(),
        "edges":    edge_count,
    });
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

async fn run_query(db_path: PathBuf, t: TenantId, k: usize, text: String) -> Result<()> {
    let store = Arc::new(SledGraphStore::open(&db_path).context("open sled")?);
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(128));
    let index = build_index_for_query(store.as_ref(), t, 128).await?;
    let reader = HeuristicReader::default()
        .with_embedder(embedder)
        .with_vector_index(index);
    let g: Arc<dyn ReaderGraph + 'static> = store;
    let q = Query::ask(text).with_k(k);
    let out = reader.read(t, &q, g.as_ref()).await?;
    let json = serde_json::json!({
        "query":    q.text.as_ref(),
        "docs":     out.docs.iter().map(|(d, s)| serde_json::json!({"id": d, "score": s})).collect::<Vec<_>>(),
        "entities": out.entities.iter().take(10)
                       .map(|(e, s)| serde_json::json!({"id": e, "score": s})).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}
