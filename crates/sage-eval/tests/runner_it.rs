//! End-to-end eval: ingest a tiny corpus, run a labeled sample set, assert metrics.

use std::sync::Arc;

use sage_core::{Document, Query, TenantId};
use sage_eval::{EvalRunner, EvalSample};
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

#[tokio::test]
async fn perfect_recall_when_query_names_match_entities() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    let writer = LlmWriterPolicy::new(llm);
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;
    for i in 1..=2u64 {
        let a = writer
            .step(&empty_state(), &Document::new(i, format!("doc {i}")))
            .await
            .unwrap();
        apply_action(&graph, t, &a).await.unwrap();
    }

    let reader = Arc::new(HeuristicReader::default());
    let runner = EvalRunner::new(Arc::clone(&reader), 1);
    let samples = vec![
        EvalSample {
            query: Query::ask("Acme"),
            ground_truth: vec![1],
        },
        EvalSample {
            query: Query::ask("Globex"),
            ground_truth: vec![2],
        },
    ];
    let r = runner.run(&graph, &samples).await.unwrap();
    assert_eq!(r.samples, 2);
    assert!(
        r.recall_at_k > 0.99,
        "recall@1 should be ~1.0, got {}",
        r.recall_at_k
    );
    assert!(r.mrr > 0.99, "mrr should be ~1.0, got {}", r.mrr);
}

#[tokio::test]
async fn empty_samples_yield_zero_report() {
    let reader = Arc::new(HeuristicReader::default());
    let runner = EvalRunner::new(reader, 5);
    let graph = MemGraphStore::new();
    let r = runner.run(&graph, &[]).await.unwrap();
    assert_eq!(r.samples, 0);
    assert_eq!(r.recall_at_k, 0.0);
    assert_eq!(r.k, 5);
}
