//! Full ingest → query pipeline: writer + reader against MemGraphStore.

use std::sync::Arc;

use sage_core::{Query, Reader, TenantId};
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_writer::{apply_action, LlmWriterPolicy, WriterPolicy, WriterState};

fn empty_state() -> WriterState<'static> {
    WriterState {
        query: None,
        candidates: &[],
        processed: &[],
        step: 0,
    }
}

async fn ingest(llm_payloads: &[&str], graph: &MemGraphStore) {
    let llm = Arc::new(MockLlm::new());
    for p in llm_payloads {
        llm.push(*p);
    }
    let writer = LlmWriterPolicy::new(llm);
    let t = TenantId::DEFAULT;
    for (i, _) in llm_payloads.iter().enumerate() {
        let doc = sage_core::Document::new((i + 1) as u64, format!("doc {}", i + 1));
        let action = writer.step(&empty_state(), &doc).await.unwrap();
        apply_action(graph, t, &action).await.unwrap();
    }
}

#[tokio::test]
async fn query_returns_doc_containing_named_entity() {
    let graph = MemGraphStore::new();
    ingest(
        &[
            r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#,
            r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#,
        ],
        &graph,
    )
    .await;

    let reader = HeuristicReader::default();
    let out = reader
        .read(TenantId::DEFAULT, &Query::ask("Who founded Acme?"), &graph)
        .await
        .unwrap();

    assert!(!out.docs.is_empty(), "should return at least one doc");
    // The top doc must be #1 — the one that mentions Acme / Alice.
    assert_eq!(
        out.docs[0].0, 1,
        "top doc should be the Acme doc, got {:?}",
        out.docs
    );
}

#[tokio::test]
async fn empty_graph_yields_empty_output() {
    let graph = MemGraphStore::new();
    let reader = HeuristicReader::default();
    let out = reader
        .read(TenantId::DEFAULT, &Query::ask("anything"), &graph)
        .await
        .unwrap();
    assert!(out.docs.is_empty());
    assert!(out.entities.is_empty());
}

#[tokio::test]
async fn unrelated_query_still_yields_finite_top_k() {
    let graph = MemGraphStore::new();
    ingest(
        &[r#"{"triples":[{"src":"Alice","rel":"knows","dst":"Bob"}],"stop":true}"#],
        &graph,
    )
    .await;
    let reader = HeuristicReader::default();
    let out = reader
        .read(TenantId::DEFAULT, &Query::ask("foobar baz"), &graph)
        .await
        .unwrap();
    // Softmax over uniformly-zero scores distributes mass evenly: reader returns
    // top-k docs with finite scores, not an empty result. Contract: no NaN, no panic.
    for (_, s) in &out.docs {
        assert!(s.is_finite(), "doc score must be finite, got {s}");
        assert!(*s >= 0.0, "score must be non-negative, got {s}");
    }
}

#[tokio::test]
async fn tenant_isolation_is_respected_by_reader() {
    let graph = MemGraphStore::new();
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    let writer = LlmWriterPolicy::new(llm);
    let action = writer
        .step(&empty_state(), &sage_core::Document::new(1, "x"))
        .await
        .unwrap();
    apply_action(&graph, TenantId(1), &action).await.unwrap();

    let reader = HeuristicReader::default();
    let out_t1 = reader
        .read(TenantId(1), &Query::ask("Acme"), &graph)
        .await
        .unwrap();
    let out_t2 = reader
        .read(TenantId(2), &Query::ask("Acme"), &graph)
        .await
        .unwrap();
    assert!(!out_t1.docs.is_empty());
    assert!(out_t2.docs.is_empty(), "tenant 2 has no data");
}
