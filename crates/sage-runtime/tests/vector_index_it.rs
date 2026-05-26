//! HNSW VectorIndex auto-wiring in SageEngine.
//!
//! Verifies: (a) ingest auto-inserts entity embeddings into the attached index,
//! (b) reader narrows candidates via the index and still returns the right doc,
//! (c) result matches the non-indexed reader on the same data (correctness).

use std::sync::Arc;

use sage_core::{Document, Query, VectorIndex};
use sage_embed::{DeterministicEmbedder, HnswIndex};
use sage_graph::MemGraphStore;
use sage_llm::MockLlm;
use sage_reader::HeuristicReader;
use sage_runtime::SageEngine;
use sage_writer::LlmWriterPolicy;

const DIM: usize = 128;

fn push_n_triples(llm: &Arc<MockLlm>, n: usize) {
    for i in 0..n {
        llm.push(format!(
            r#"{{"triples":[{{"src":"Person{i}","rel":"knows","dst":"Topic{i}"}}],"stop":true}}"#
        ));
    }
}

#[tokio::test]
async fn engine_auto_indexes_new_entities() {
    let llm = Arc::new(MockLlm::new());
    push_n_triples(&llm, 3);

    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(DIM));
    let index: Arc<dyn VectorIndex> = Arc::new(HnswIndex::new(DIM));

    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default()
            .with_embedder(Arc::clone(&embedder))
            .with_vector_index(Arc::clone(&index)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder)
    .with_vector_index(Arc::clone(&index));

    engine
        .ingest(
            (0..3u64)
                .map(|i| Document::new(i + 1, format!("doc {i}")))
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();

    // 3 docs × 2 entities each = 6 entities — all should land in the index.
    assert_eq!(index.len(), 6, "every embedded entity must be auto-indexed");
}

#[tokio::test]
async fn indexed_reader_returns_correct_top_doc() {
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);

    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(DIM));
    let index: Arc<dyn VectorIndex> = Arc::new(HnswIndex::new(DIM));
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default()
            .with_embedder(Arc::clone(&embedder))
            .with_vector_index(Arc::clone(&index))
            .with_narrow_k(4),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder)
    .with_vector_index(index);

    engine
        .ingest(vec![
            Document::new(1, "Alice founded Acme."),
            Document::new(2, "Bob works at Globex."),
        ])
        .await
        .unwrap();

    let out = engine.query(&Query::ask("Acme")).await.unwrap();
    assert_eq!(
        out.docs[0].0, 1,
        "indexed reader must still pick the Acme doc, got {:?}",
        out.docs
    );
}

#[tokio::test]
async fn indexed_vs_unindexed_agree_on_top_doc() {
    // Two engines on the same data, one with index, one without.
    // Both must rank the same top doc; this is the correctness guard against
    // the narrowing path silently dropping the right answer.
    let llm = Arc::new(MockLlm::new());
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Carol","rel":"runs","dst":"Initech"}],"stop":true}"#);
    // duplicate for second engine
    llm.push(r#"{"triples":[{"src":"Alice","rel":"founded","dst":"Acme"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Bob","rel":"works_at","dst":"Globex"}],"stop":true}"#);
    llm.push(r#"{"triples":[{"src":"Carol","rel":"runs","dst":"Initech"}],"stop":true}"#);

    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(DIM));
    let index: Arc<dyn VectorIndex> = Arc::new(HnswIndex::new(DIM));

    let mk_docs = || {
        vec![
            Document::new(1, "Alice founded Acme."),
            Document::new(2, "Bob works at Globex."),
            Document::new(3, "Carol runs Initech."),
        ]
    };

    // --- with index ---
    let with_idx = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default()
            .with_embedder(Arc::clone(&embedder))
            .with_vector_index(Arc::clone(&index))
            .with_narrow_k(10),
        Arc::new(MemGraphStore::new()),
        Arc::clone(&llm),
    )
    .with_embedder(Arc::clone(&embedder))
    .with_vector_index(index);
    with_idx.ingest(mk_docs()).await.unwrap();
    let out_idx = with_idx.query(&Query::ask("Globex")).await.unwrap();

    // --- without index ---
    let no_idx = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default().with_embedder(Arc::clone(&embedder)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder);
    no_idx.ingest(mk_docs()).await.unwrap();
    let out_no = no_idx.query(&Query::ask("Globex")).await.unwrap();

    assert_eq!(
        out_idx.docs[0].0, out_no.docs[0].0,
        "indexed vs unindexed must agree on top doc; got idx={:?} no_idx={:?}",
        out_idx.docs, out_no.docs
    );
}

#[tokio::test]
async fn empty_index_does_not_break_query() {
    let llm = Arc::new(MockLlm::new());
    let embedder: Arc<dyn sage_core::Embedder> = Arc::new(DeterministicEmbedder::new(DIM));
    let index: Arc<dyn VectorIndex> = Arc::new(HnswIndex::new(DIM));
    let engine = SageEngine::new(
        LlmWriterPolicy::new(Arc::clone(&llm)),
        HeuristicReader::default()
            .with_embedder(Arc::clone(&embedder))
            .with_vector_index(Arc::clone(&index)),
        Arc::new(MemGraphStore::new()),
        llm,
    )
    .with_embedder(embedder)
    .with_vector_index(index);

    // No ingest → empty graph + empty index. Reader must not panic.
    let out = engine.query(&Query::ask("anything")).await.unwrap();
    assert!(out.docs.is_empty());
}
