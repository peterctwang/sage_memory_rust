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
use sage_core::{EntityScan, GraphStore, Query, Reader, ReaderGraph, TenantId};
use sage_embed::DeterministicEmbedder;
use sage_graph::{MemGraphStore, SledGraphStore};
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::LlmWriterPolicy;

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
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
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
    }
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
