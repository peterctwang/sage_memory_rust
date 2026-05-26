//! Integration tests for `GfmReader` — end-to-end through the `Reader` trait.
//!
//! ⚠️ With random GFM weights we cannot assert that this reader's ranking
//! is *correct* (it isn't trained). What we CAN assert: pipeline doesn't
//! NaN/panic, dim contracts hold, soft-addressing prior keeps the system
//! pointing at the right entity for trivial single-token exact matches.

use std::sync::Arc;

use sage_core::{Document, Embedder, Query, Reader, TenantId};
use sage_embed::DeterministicEmbedder;
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::{GfmConfig, GfmReader};
use sage_writer::{apply_action_embedded, LlmWriterPolicy, WriterPolicy, WriterState};

const D: usize = 16; // small dim — keeps test fast

fn empty_state() -> WriterState<'static> {
    WriterState {
        query: None,
        candidates: &[],
        processed: &[],
        step: 0,
    }
}

async fn ingest_two(
    llm_payloads: &[&str],
    graph: &MemGraphStore,
    embedder: &DeterministicEmbedder,
) {
    let llm = Arc::new(MockLlm::new());
    for p in llm_payloads {
        llm.push(*p);
    }
    let writer = LlmWriterPolicy::new(llm);
    let t = TenantId::DEFAULT;
    for (i, _) in llm_payloads.iter().enumerate() {
        let doc = Document::new((i + 1) as u64, format!("doc {}", i + 1));
        let action = writer.step(&empty_state(), &doc).await.unwrap();
        apply_action_embedded(graph, Some(embedder), t, &action)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn gfm_reader_returns_finite_scores() {
    let embedder = DeterministicEmbedder::new(D);
    let graph = MemGraphStore::new();
    ingest_two(
        &[
            r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#,
            r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#,
        ],
        &graph,
        &embedder,
    )
    .await;

    let reader = GfmReader::new(
        Arc::new(embedder) as Arc<dyn Embedder>,
        GfmConfig {
            hidden_dim: D,
            gate_hidden: 8,
            ..GfmConfig::default()
        },
        42,
    );
    let out = reader
        .read(TenantId::DEFAULT, &Query::ask("Acme"), &graph)
        .await
        .unwrap();

    for (_, s) in &out.docs {
        assert!(s.is_finite(), "doc score non-finite: {s}");
        assert!(*s >= 0.0, "doc score negative: {s}");
    }
    for (_, s) in &out.entities {
        assert!(s.is_finite(), "entity score non-finite: {s}");
    }
    assert!(!out.docs.is_empty(), "expected at least one doc");
}

#[tokio::test]
async fn gfm_reader_dim_mismatch_errors_cleanly() {
    let graph = MemGraphStore::new();
    let embedder = DeterministicEmbedder::new(D);
    ingest_two(
        &[r#"{"triples":[{"src":"A","rel":"r","dst":"B"}],"stop":true}"#],
        &graph,
        &embedder,
    )
    .await;

    // Embedder dim 16 (D) ≠ GFM hidden_dim 32 — must produce SageError::Reader.
    let reader = GfmReader::new(
        Arc::new(embedder) as Arc<dyn Embedder>,
        GfmConfig {
            hidden_dim: 32,
            gate_hidden: 8,
            ..GfmConfig::default()
        },
        0,
    );
    let r = reader
        .read(TenantId::DEFAULT, &Query::ask("x"), &graph)
        .await;
    assert!(
        matches!(r, Err(sage_core::SageError::Reader(_))),
        "expected Reader err, got {r:?}"
    );
}

#[tokio::test]
async fn gfm_reader_empty_graph_yields_empty_output() {
    let graph = MemGraphStore::new();
    let embedder = DeterministicEmbedder::new(D);
    let reader = GfmReader::new(
        Arc::new(embedder) as Arc<dyn Embedder>,
        GfmConfig {
            hidden_dim: D,
            gate_hidden: 8,
            ..GfmConfig::default()
        },
        0,
    );
    let out = reader
        .read(TenantId::DEFAULT, &Query::ask("anything"), &graph)
        .await
        .unwrap();
    assert!(out.docs.is_empty());
    assert!(out.entities.is_empty());
}

#[tokio::test]
async fn gfm_reader_seed_determinism_reflected_in_output() {
    // Same seed, same data → same output. Different seed → different
    // entity ranking (probabilistically; we just check inequality of the
    // first entity score, not full vectors, since LayerNorm can incidentally
    // collapse low-magnitude differences).
    let embedder = DeterministicEmbedder::new(D);
    let graph = MemGraphStore::new();
    ingest_two(
        &[
            r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#,
            r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#,
        ],
        &graph,
        &embedder,
    )
    .await;

    let emb_arc: Arc<dyn Embedder> = Arc::new(embedder);
    let cfg = GfmConfig {
        hidden_dim: D,
        gate_hidden: 8,
        ..GfmConfig::default()
    };
    let r_a = GfmReader::new(Arc::clone(&emb_arc), cfg, 100);
    let r_b = GfmReader::new(Arc::clone(&emb_arc), cfg, 100);
    let r_c = GfmReader::new(Arc::clone(&emb_arc), cfg, 999);

    let q = Query::ask("Acme");
    let oa = r_a.read(TenantId::DEFAULT, &q, &graph).await.unwrap();
    let ob = r_b.read(TenantId::DEFAULT, &q, &graph).await.unwrap();
    let oc = r_c.read(TenantId::DEFAULT, &q, &graph).await.unwrap();

    assert_eq!(
        oa.entities, ob.entities,
        "same seed must produce identical entity scores"
    );
    // Different seed — at least one entity should differ in score.
    let any_diff = oa
        .entities
        .iter()
        .zip(oc.entities.iter())
        .any(|((_, a), (_, c))| (a - c).abs() > 1e-6);
    assert!(any_diff, "different seed should change at least one score");
}
