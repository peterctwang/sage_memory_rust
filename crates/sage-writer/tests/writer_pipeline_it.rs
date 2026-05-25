//! End-to-end writer pipeline: MockLlm → LlmWriterPolicy → apply_action → MemGraphStore.

use std::sync::Arc;

use sage_core::{Document, GraphStore, TenantId};
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_writer::{apply_action, LlmWriterPolicy, WriterPolicy, WriterState};

fn empty_state() -> WriterState<'static> {
    WriterState {
        query: None,
        candidates: &[],
        processed: &[],
        step: 0,
    }
}

#[tokio::test]
async fn writes_two_triples_into_graph() {
    let llm = Arc::new(MockLlm::new());
    llm.push(
        r#"{"triples":[
            {"src":"Alice","rel":"knows","dst":"Bob"},
            {"src":"Bob","rel":"works_at","dst":"Acme"}
        ],"stop":true}"#,
    );
    let policy = LlmWriterPolicy::new(llm);
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;

    let doc = Document::new(7, "Alice knows Bob who works at Acme.");
    let action = policy.step(&empty_state(), &doc).await.unwrap();
    let report = apply_action(&graph, t, &action).await.unwrap();

    assert_eq!(report.entities_added, 3, "Alice, Bob, Acme");
    assert_eq!(report.edges_added, 2);
    assert_eq!(graph.entity_count(t).await.unwrap(), 3);
}

#[tokio::test]
async fn second_doc_reuses_existing_entities() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"knows","dst":"Bob"}],"stop":false}"#);
    llm.push(r#"{"triples":[{"src":"Alice","rel":"knows","dst":"Carol"}],"stop":true}"#);
    let policy = LlmWriterPolicy::new(llm);
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;

    let r1 = apply_action(
        &graph,
        t,
        &policy
            .step(&empty_state(), &Document::new(1, "x"))
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let r2 = apply_action(
        &graph,
        t,
        &policy
            .step(&empty_state(), &Document::new(2, "y"))
            .await
            .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(r1.entities_added, 2);
    assert_eq!(
        r2.entities_added, 1,
        "Alice already exists, only Carol is new"
    );
    assert_eq!(graph.entity_count(t).await.unwrap(), 3);
}

#[tokio::test]
async fn malformed_llm_output_propagates_as_error() {
    let llm = Arc::new(MockLlm::new());
    llm.push("totally not json");
    let policy = LlmWriterPolicy::new(llm);
    let res = policy.step(&empty_state(), &Document::new(1, "x")).await;
    assert!(res.is_err());
}
