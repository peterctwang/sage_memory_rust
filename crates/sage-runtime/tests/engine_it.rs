//! `SageEngine` end-to-end: ingest a small corpus, query, optionally synthesize an answer.

use std::sync::Arc;

use sage_core::{Document, GraphStore, Query, TenantId};
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::LlmWriterPolicy;

fn make_engine(
    llm_writer_payloads: &[&str],
    llm_answer_payloads: &[&str],
) -> SageEngine<LlmWriterPolicy<MockLlm>, HeuristicReader, MemGraphStore, MockLlm> {
    let llm = Arc::new(MockLlm::new());
    for p in llm_writer_payloads {
        llm.push(*p);
    }
    for p in llm_answer_payloads {
        llm.push(*p);
    }
    let writer = LlmWriterPolicy::new(Arc::clone(&llm));
    let reader = HeuristicReader::default();
    let graph = Arc::new(MemGraphStore::new());
    SageEngine::new(writer, reader, graph, llm)
}

#[tokio::test]
async fn ingest_then_query() {
    let engine = make_engine(
        &[
            r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#,
            r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#,
        ],
        &[],
    );

    let report = engine
        .ingest(vec![
            Document::new(1, "Alice founded Acme."),
            Document::new(2, "Bob works at Globex."),
        ])
        .await
        .unwrap();
    assert_eq!(report.docs_processed, 2);
    assert_eq!(report.entities_added, 4);
    assert_eq!(report.edges_added, 2);

    let out = engine
        .query(&Query::ask("Who founded Acme?"))
        .await
        .unwrap();
    assert_eq!(out.docs[0].0, 1, "top doc should be the Acme doc");
}

#[tokio::test]
async fn query_with_answer_uses_llm() {
    let engine = make_engine(
        &[r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#],
        &["Alice founded Acme."],
    );
    engine
        .ingest(vec![Document::new(1, "Alice founded Acme.")])
        .await
        .unwrap();
    let ans = engine
        .query_with_answer(&Query::ask("Who founded Acme?"))
        .await
        .unwrap();
    assert!(!ans.text.is_empty());
    assert!(ans.evidence.contains(&1));
}

#[tokio::test]
async fn engine_respects_tenant() {
    let engine = make_engine(
        &[r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#],
        &[],
    )
    .with_tenant(TenantId(42));
    engine.ingest(vec![Document::new(1, "x")]).await.unwrap();
    assert_eq!(engine.tenant(), TenantId(42));
    assert_eq!(engine.graph().entity_count(TenantId(42)).await.unwrap(), 2);
    assert_eq!(
        engine
            .graph()
            .entity_count(TenantId::DEFAULT)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn snapshot_and_restore_round_trip() {
    let engine = make_engine(
        &[
            r#"{"triples":[{"src":"A","rel":"knows","dst":"B"}],"stop":true}"#,
            r#"{"triples":[{"src":"C","rel":"knows","dst":"D"}],"stop":true}"#,
        ],
        &[],
    );
    engine
        .ingest(vec![sage_core::Document::new(1, "x")])
        .await
        .unwrap();
    let snap = engine.snapshot().await.unwrap();
    engine
        .ingest(vec![sage_core::Document::new(2, "y")])
        .await
        .unwrap();
    assert_eq!(
        engine.graph().entity_count(engine.tenant()).await.unwrap(),
        4
    );
    engine.restore(snap).await.unwrap();
    assert_eq!(
        engine.graph().entity_count(engine.tenant()).await.unwrap(),
        2,
        "post-restore must match snapshot state"
    );
}

#[tokio::test]
async fn ingest_single_doc() {
    let engine = make_engine(
        &[r#"{"triples":[{"src":"A","rel":"knows","dst":"B"}],"stop":true}"#],
        &[],
    );
    let r = engine
        .ingest_one(Document::new(1, "A knows B"))
        .await
        .unwrap();
    assert_eq!(r.entities_added, 2);
    assert_eq!(r.edges_added, 1);
}
