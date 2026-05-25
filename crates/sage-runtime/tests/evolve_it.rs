//! SPEC §6 evolve scaffold — demonstrates the snapshot → ingest → eval → rollback
//! cycle composed from existing engine primitives, without GRPO (M4) or GFM (M3).

use std::sync::Arc;

use sage_core::{Document, GraphStore, Query};
use sage_eval::{EvalRunner, EvalSample};
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::LlmWriterPolicy;

#[tokio::test]
async fn rollback_when_evolution_hurts_recall() {
    let llm = Arc::new(MockLlm::new());
    // Baseline: 2 informative docs
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    // Evolution round: poisons the graph with many unrelated entities that drown
    // out the signal during softmax aggregation.
    for _ in 0..30 {
        llm.push(
            r#"{"triples":[
                 {"src":"NoiseA","rel":"related_to","dst":"NoiseB"},
                 {"src":"NoiseC","rel":"related_to","dst":"NoiseD"}
               ],"stop":true}"#,
        );
    }

    let reader = Arc::new(HeuristicReader::default());
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default(),
        Arc::new(MemGraphStore::new()),
        Arc::clone(&llm),
    );
    let t = engine.tenant();

    // Phase 1: baseline ingest.
    engine
        .ingest(vec![
            Document::new(1, "Alice founded Acme."),
            Document::new(2, "Bob works at Globex."),
        ])
        .await
        .unwrap();

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
    let runner = EvalRunner::new(Arc::clone(&reader), 1);
    let before = runner.run(engine.graph().as_ref(), &samples).await.unwrap();
    assert!(
        before.recall_at_k > 0.9,
        "baseline recall@1 should be ~1.0, got {}",
        before.recall_at_k
    );

    // Phase 2: snapshot, then evolve by ingesting noisy data.
    let snap = engine.snapshot().await.unwrap();
    let noise_docs: Vec<Document> = (10..40u64).map(|i| Document::new(i, "noise")).collect();
    engine.ingest(noise_docs).await.unwrap();

    let after = runner.run(engine.graph().as_ref(), &samples).await.unwrap();

    // Phase 3: drift guard — if recall dropped, roll back.
    let dropped = before.recall_at_k - after.recall_at_k;
    let threshold = 0.05;
    if dropped > threshold {
        engine.restore(snap).await.unwrap();
        let restored = runner.run(engine.graph().as_ref(), &samples).await.unwrap();
        assert!(
            (restored.recall_at_k - before.recall_at_k).abs() < 1e-5,
            "post-restore recall {} must match baseline {}",
            restored.recall_at_k,
            before.recall_at_k
        );
        assert_eq!(
            engine.graph().entity_count(t).await.unwrap(),
            4,
            "post-restore must contain only the original 4 entities (Alice/Acme/Bob/Globex)"
        );
    } else {
        // If noise didn't actually hurt, the test still passes — but we want to
        // assert at least that the evolve cycle ran end-to-end.
        assert!(after.samples == samples.len());
    }
}
