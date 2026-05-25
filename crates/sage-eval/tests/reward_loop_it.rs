//! Closes the writer reward loop end-to-end:
//!   ingest → query → extract retrieved doc IDs → compute_reward(RewardInputs)
//!
//! Demonstrates that the pure-function reward path in `sage-core::reward`
//! composes correctly with a real `Reader` (here `HeuristicReader`).

use std::sync::Arc;

use sage_core::{compute_reward, Document, Query, RewardCfg, RewardInputs, TenantId, WriterReward};
use sage_eval::{compute_reward_batch, compute_reward_for_sample, EvalSample, JudgeInputs};
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
async fn compute_reward_for_sample_composes_reader_and_reward() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    let writer = LlmWriterPolicy::new(Arc::clone(&llm));
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;
    let action = writer
        .step(&empty_state(), &Document::new(1, "Alice founded Acme."))
        .await
        .unwrap();
    apply_action(&graph, t, &action).await.unwrap();

    let reader = HeuristicReader::default();
    let sample = EvalSample {
        query: Query::ask("Acme").with_k(1),
        ground_truth: vec![1],
    };
    let r = compute_reward_for_sample(&reader, t, &graph, &sample, &[], JudgeInputs::default())
        .await
        .unwrap();
    assert!(r.r_rec > 0.99, "got r_rec={}", r.r_rec);
    assert!(r.r_pre > 0.99, "got r_pre={}", r.r_pre);
}

#[tokio::test]
async fn batch_returns_one_reward_per_sample() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    let writer = LlmWriterPolicy::new(Arc::clone(&llm));
    let graph = MemGraphStore::new();
    let t = TenantId::DEFAULT;
    for i in 1..=2u64 {
        let a = writer
            .step(&empty_state(), &Document::new(i, format!("d{i}")))
            .await
            .unwrap();
        apply_action(&graph, t, &a).await.unwrap();
    }
    let reader = HeuristicReader::default();
    let samples = vec![
        EvalSample {
            query: Query::ask("Acme").with_k(1),
            ground_truth: vec![1],
        },
        EvalSample {
            query: Query::ask("Globex").with_k(1),
            ground_truth: vec![2],
        },
    ];
    let rs = compute_reward_batch(&reader, t, &graph, &samples, &[], &[])
        .await
        .unwrap();
    assert_eq!(rs.len(), 2);
    for r in &rs {
        assert!(r.r_rec > 0.99, "got {}", r.r_rec);
    }
}

#[tokio::test]
async fn batch_rejects_mismatched_triples_length() {
    let graph = MemGraphStore::new();
    let reader = HeuristicReader::default();
    let samples = vec![EvalSample {
        query: Query::ask("x"),
        ground_truth: vec![1],
    }];
    let res = compute_reward_batch(
        &reader,
        TenantId::DEFAULT,
        &graph,
        &samples,
        &vec![vec![]; 5],
        &[],
    )
    .await;
    assert!(matches!(res, Err(sage_core::SageError::Invalid(_))));
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
