//! Closes the writer reward loop end-to-end:
//!   ingest → query → extract retrieved doc IDs → compute_reward(RewardInputs)
//!
//! Demonstrates that the pure-function reward path in `sage-core::reward`
//! composes correctly with a real `Reader` (here `HeuristicReader`).

use std::sync::Arc;

use sage_core::{compute_reward, Document, Query, RewardCfg, RewardInputs, TenantId, WriterReward};
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
async fn perfect_retrieval_gives_high_reward() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    let writer = LlmWriterPolicy::new(Arc::clone(&llm));
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;

    let mut all_triples = Vec::new();
    for i in 1..=2u64 {
        let a = writer
            .step(&empty_state(), &Document::new(i, format!("doc {i}")))
            .await
            .unwrap();
        for (_, rel, _) in &a.triples {
            all_triples.push((i, rel.clone(), i)); // dummy triples for ρ_rep
        }
        apply_action(&graph, t, &a).await.unwrap();
    }

    let reader = HeuristicReader::default();
    let out = sage_core::Reader::read(&reader, t, &Query::ask("Acme").with_k(1), &graph)
        .await
        .unwrap();
    let retrieved: Vec<u64> = out.docs.iter().map(|(d, _)| *d).collect();

    let inputs = RewardInputs {
        retrieved: &retrieved,
        ground_truth: &[1],
        triples: &all_triples,
        r_ded: 1.0,
        r_ans: 1.0,
        fmt_bonus: 0.0,
    };
    let r: WriterReward = compute_reward(inputs);
    assert!(r.r_rec > 0.99, "recovery should be ~1.0, got {}", r.r_rec);
    assert!(r.r_pre > 0.99, "precision should be ~1.0, got {}", r.r_pre);
    let task = r.task(&sage_core::TaskWeights::default());
    let traj = r.trajectory(&RewardCfg::default());
    assert!(task > 0.99, "task reward ~1.0, got {task}");
    assert!(
        traj > 0.9,
        "trajectory reward should remain high, got {traj}"
    );
}

#[tokio::test]
async fn empty_retrieval_yields_zero_recall() {
    // Graph has no entities → reader returns no docs → reward shows zero r_rec / r_pre.
    let graph = MemGraphStore::new();
    let reader = HeuristicReader::default();
    let out = sage_core::Reader::read(&reader, TenantId::DEFAULT, &Query::ask("x"), &graph)
        .await
        .unwrap();
    let retrieved: Vec<u64> = out.docs.iter().map(|(d, _)| *d).collect();

    let r = compute_reward(RewardInputs {
        retrieved: &retrieved,
        ground_truth: &[1, 2],
        triples: &[],
        r_ded: 0.0,
        r_ans: 0.0,
        fmt_bonus: 0.0,
    });
    assert_eq!(r.r_rec, 0.0);
    assert_eq!(r.r_pre, 0.0);
    assert_eq!(r.rep_penalty, 0.0);
}
