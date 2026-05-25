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
use sage_core::{EntityScan, EntityType, GraphStore, Query, Reader, ReaderGraph, TenantId};
use sage_embed::DeterministicEmbedder;
use sage_eval::{EvalRunner, EvalSample};
use sage_graph::{MemGraphStore, SledGraphStore};
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::{apply_action, EntityRef, LlmWriterPolicy, WriterAction};
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
        Cmd::Eval { db, k, tenant } => run_eval(db, TenantId(tenant), k).await,
    }
}

#[derive(Deserialize)]
struct EvalSampleJson {
    query: String,
    ground_truth: Vec<sage_core::DocId>,
}

async fn run_eval(db_path: PathBuf, t: TenantId, k: usize) -> Result<()> {
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
    let reader = Arc::new(HeuristicReader::default().with_embedder(embedder));
    let runner = EvalRunner::new(reader, k).with_tenant(t);
    let report = runner.run(g.as_ref(), &samples).await?;
    let out = serde_json::json!({
        "samples":        report.samples,
        "k":              report.k,
        "recall_at_k":    report.recall_at_k,
        "precision_at_k": report.precision_at_k,
        "f1_at_k":        report.f1_at_k,
        "mrr":            report.mrr,
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
    let g: Arc<dyn ReaderGraph + 'static> =
        Arc::new(SledGraphStore::open(&db_path).context("open sled")?);
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(128));
    let reader = HeuristicReader::default().with_embedder(embedder);
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
