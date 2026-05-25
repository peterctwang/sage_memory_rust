//! Demonstrates that wiring an `Embedder` into the engine activates `λ_cos`
//! and lets the reader rank entities whose names don't exactly match any
//! query token.

use std::sync::Arc;

use sage_core::{Document, GraphStore, Query, TenantId};
use sage_embed::DeterministicEmbedder;
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::LlmWriterPolicy;

#[tokio::test]
async fn entities_get_embeddings_when_engine_has_embedder() {
    let llm = Arc::new(MockLlm::new());
    llm.push(
        r#"{"triples":[{"src":"Alice Liddell","rel":"founded","dst":"Acme Industries"}],"stop":true}"#,
    );
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(64));

    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default().with_embedder(Arc::clone(&embedder)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder);

    engine
        .ingest(vec![Document::new(1, "Alice founded Acme.")])
        .await
        .unwrap();

    let g = engine.graph();
    let ids = sage_core::EntityScan::find_by_name(g.as_ref(), TenantId::DEFAULT, "Acme Industries")
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "entity should be reachable by name");

    let e = g
        .get_entity(TenantId::DEFAULT, ids[0])
        .await
        .unwrap()
        .unwrap();
    let emb = e.embedding.expect("entity must carry an embedding");
    assert_eq!(emb.len(), 64);
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "embedding norm={norm}");
}

#[tokio::test]
async fn cos_boosts_partial_name_match_over_unrelated() {
    // "Who founded Acme?" tokenizes to ["founded","acme"]. Entity names are
    // multi-token: exact match misses for both entities. Without embeddings the
    // Cons baseline ties all entities; with embeddings, the entity whose name
    // shares the "acme" token wins.

    let llm = Arc::new(MockLlm::new());
    llm.push(
        r#"{"triples":[{"src":"Alice Liddell","rel":"founded","dst":"Acme Industries"}],"stop":true}"#,
    );
    llm.push(
        r#"{"triples":[{"src":"Bob Jones","rel":"works_at","dst":"Globex Holdings"}],"stop":true}"#,
    );

    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(256));
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default().with_embedder(Arc::clone(&embedder)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder);

    engine
        .ingest(vec![
            Document::new(1, "Alice Liddell founded Acme Industries."),
            Document::new(2, "Bob Jones works at Globex Holdings."),
        ])
        .await
        .unwrap();

    let out = engine
        .query(&Query::ask("Who founded Acme?"))
        .await
        .unwrap();
    assert!(!out.docs.is_empty(), "got {:?}", out.docs);
    assert_eq!(
        out.docs[0].0, 1,
        "doc 1 (Acme Industries) should rank above doc 2 (Globex Holdings); got {:?}",
        out.docs
    );
}

#[tokio::test]
async fn engine_without_embedder_still_works() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default(),
        Arc::new(MemGraphStore::new()),
        llm,
    );
    engine.ingest(vec![Document::new(1, "x")]).await.unwrap();
    let out = engine.query(&Query::ask("Acme")).await.unwrap();
    assert_eq!(out.docs[0].0, 1);
}
