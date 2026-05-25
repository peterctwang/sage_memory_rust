//! `SageEngine` — orchestrates ingest (writer) and query (reader) against a
//! single graph store + LLM. Minimum viable surface for M2.5.

use std::sync::Arc;

use sage_core::{Document, Embedder, Query, ReadOutput, Reader, ReaderGraph, Result, TenantId};
use sage_llm::LlmClient;
use sage_writer::{apply_action_embedded, ApplyReport, WriterPolicy, WriterState};

#[derive(Debug, Default, Clone)]
pub struct IngestReport {
    pub docs_processed: usize,
    pub entities_added: usize,
    pub edges_added: usize,
    pub triples_skipped: usize,
}

impl IngestReport {
    fn merge(&mut self, r: ApplyReport) {
        self.docs_processed += 1;
        self.entities_added += r.entities_added;
        self.edges_added += r.edges_added;
        self.triples_skipped += r.triples_skipped;
    }
}

#[derive(Debug, Clone)]
pub struct AnswerBundle {
    pub text: String,
    pub evidence: Vec<sage_core::DocId>,
    pub read: ReadOutput,
}

pub struct SageEngine<W, R, G, L>
where
    W: WriterPolicy,
    R: Reader,
    G: ReaderGraph + 'static,
    L: LlmClient,
{
    writer: W,
    reader: R,
    graph: Arc<G>,
    llm: Arc<L>,
    embedder: Option<Arc<dyn Embedder>>,
    tenant: TenantId,
}

impl<W, R, G, L> std::fmt::Debug for SageEngine<W, R, G, L>
where
    W: WriterPolicy,
    R: Reader,
    G: ReaderGraph + 'static,
    L: LlmClient,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SageEngine")
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

impl<W, R, G, L> SageEngine<W, R, G, L>
where
    W: WriterPolicy,
    R: Reader,
    G: ReaderGraph + 'static,
    L: LlmClient,
{
    pub fn new(writer: W, reader: R, graph: Arc<G>, llm: Arc<L>) -> Self {
        Self {
            writer,
            reader,
            graph,
            llm,
            embedder: None,
            tenant: TenantId::DEFAULT,
        }
    }

    pub fn with_tenant(mut self, t: TenantId) -> Self {
        self.tenant = t;
        self
    }

    pub fn with_embedder(mut self, e: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(e);
        self
    }

    pub fn tenant(&self) -> TenantId {
        self.tenant
    }
    pub fn graph(&self) -> &Arc<G> {
        &self.graph
    }
    pub fn llm(&self) -> &Arc<L> {
        &self.llm
    }
    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }

    pub async fn ingest_one(&self, doc: Document) -> Result<ApplyReport> {
        let state = WriterState {
            query: None,
            candidates: &[],
            processed: &[],
            step: 0,
        };
        let action = self.writer.step(&state, &doc).await?;
        apply_action_embedded(
            self.graph.as_ref(),
            self.embedder.as_deref(),
            self.tenant,
            &action,
        )
        .await
    }

    pub async fn ingest<I>(&self, docs: I) -> Result<IngestReport>
    where
        I: IntoIterator<Item = Document>,
        I::IntoIter: Send,
    {
        let mut report = IngestReport::default();
        for d in docs {
            let r = self.ingest_one(d).await?;
            report.merge(r);
        }
        Ok(report)
    }

    pub async fn query(&self, q: &Query) -> Result<ReadOutput> {
        self.reader.read(self.tenant, q, self.graph.as_ref()).await
    }

    /// Convenience: query + ask the LLM to synthesize an answer from retrieved evidence.
    /// The LLM is given the doc IDs only; the caller is responsible for resolving them
    /// if richer context is needed (M3 will wire a doc store).
    pub async fn query_with_answer(&self, q: &Query) -> Result<AnswerBundle> {
        let out = self.query(q).await?;
        let evidence: Vec<sage_core::DocId> = out.docs.iter().map(|(d, _)| *d).collect();
        let prompt = format!(
            "Question: {}\nEvidence doc IDs: {:?}\nAnswer concisely.",
            q.text, evidence
        );
        let resp = self
            .llm
            .complete(sage_llm::ChatRequest {
                messages: vec![sage_llm::ChatMessage::user(prompt)],
                temperature: 0.0,
                max_tokens: Some(256),
            })
            .await?;
        Ok(AnswerBundle {
            text: resp.content,
            evidence,
            read: out,
        })
    }
}
