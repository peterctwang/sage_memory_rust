//! LLM-driven writer policy.
//!
//! Prompts the LLM for triples in a strict JSON envelope, parses, sanitizes,
//! and emits a `WriterAction`.

use std::sync::Arc;

use async_trait::async_trait;
use sage_core::{Document, Result, SageError};
use sage_llm::{ChatMessage, ChatRequest, LlmClient};
use serde::Deserialize;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::action::{EntityRef, RawTriple, WriterAction, WriterState};
use crate::policy::WriterPolicy;
use crate::sanitizer::TripleSanitizer;

#[derive(Debug, Deserialize)]
struct LlmTriple {
    src: String,
    rel: String,
    dst: String,
    #[serde(default)]
    src_type: Option<String>,
    #[serde(default)]
    dst_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmEnvelope {
    triples: Vec<LlmTriple>,
    #[serde(default)]
    stop: bool,
}

#[derive(Debug)]
pub struct LlmWriterPolicy<L: LlmClient> {
    llm: Arc<L>,
    sanitizer: TripleSanitizer,
    temperature: f32,
}

impl<L: LlmClient> LlmWriterPolicy<L> {
    pub fn new(llm: Arc<L>) -> Self {
        Self {
            llm,
            sanitizer: TripleSanitizer::default(),
            temperature: 0.0,
        }
    }

    pub fn with_sanitizer(mut self, s: TripleSanitizer) -> Self {
        self.sanitizer = s;
        self
    }

    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = t;
        self
    }

    fn build_prompt(&self, doc: &Document) -> ChatRequest {
        let system = ChatMessage::system(
            "You extract knowledge-graph triples. Respond with ONLY a JSON object: \
             {\"triples\":[{\"src\":..,\"rel\":..,\"dst\":..}],\"stop\":bool}. \
             Keep entity names short and canonical.",
        );
        let user = ChatMessage::user(format!(
            "Document #{id}:\n{text}\n\nExtract triples now.",
            id = doc.id,
            text = doc.text
        ));
        ChatRequest {
            messages: vec![system, user],
            temperature: self.temperature,
            max_tokens: Some(512),
        }
    }
}

#[async_trait]
impl<L: LlmClient> WriterPolicy for LlmWriterPolicy<L> {
    async fn step(&self, _state: &WriterState<'_>, doc: &Document) -> Result<WriterAction> {
        let req = self.build_prompt(doc);
        let resp = self.llm.complete(req).await?;
        let env: LlmEnvelope = serde_json::from_str(resp.content.trim())
            .map_err(|e| SageError::Writer(format!("LLM response not JSON: {e}")))?;

        let mut triples: SmallVec<[(EntityRef, SmolStr, EntityRef); 8]> = SmallVec::new();
        let cap = self.sanitizer.cfg().max_triples_per_doc;
        for t in env.triples.into_iter().take(cap) {
            let raw = RawTriple {
                src_name: SmolStr::new(t.src.trim()),
                relation: SmolStr::new(t.rel.trim()),
                dst_name: SmolStr::new(t.dst.trim()),
                src_type: t.src_type.as_deref().map(parse_etype),
                dst_type: t.dst_type.as_deref().map(parse_etype),
            };
            match self.sanitizer.sanitize(raw) {
                Ok(clean) => triples.push((
                    EntityRef::New {
                        name: clean.src_name,
                        etype: clean.src_type.unwrap_or_default(),
                        desc: None,
                    },
                    clean.relation,
                    EntityRef::New {
                        name: clean.dst_name,
                        etype: clean.dst_type.unwrap_or_default(),
                        desc: None,
                    },
                )),
                Err(reason) => {
                    tracing::debug!(?reason, "sanitizer dropped triple");
                }
            }
        }

        Ok(WriterAction {
            triples,
            source: doc.id,
            stop: env.stop,
        })
    }
}

fn parse_etype(s: &str) -> sage_core::EntityType {
    use sage_core::EntityType;
    match s.to_lowercase().as_str() {
        "person" => EntityType::Person,
        "org" | "organization" => EntityType::Org,
        "concept" => EntityType::Concept,
        "event" => EntityType::Event,
        "time" => EntityType::Time,
        _ => EntityType::Custom(SmolStr::new(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_llm::MockLlm;

    fn state(processed: &[sage_core::DocId]) -> WriterState<'_> {
        WriterState {
            query: None,
            candidates: &[],
            processed,
            step: 0,
        }
    }

    #[tokio::test]
    async fn parses_clean_envelope() {
        let llm = Arc::new(MockLlm::new());
        llm.push(r#"{"triples":[{"src":"Alice","rel":"knows","dst":"Bob"}],"stop":false}"#);
        let pol = LlmWriterPolicy::new(llm);
        let doc = Document::new(1, "Alice knows Bob.");
        let a = pol.step(&state(&[]), &doc).await.unwrap();
        assert_eq!(a.triples.len(), 1);
        assert_eq!(a.source, 1);
        assert!(!a.stop);
    }

    #[tokio::test]
    async fn drops_invalid_triples_silently() {
        let llm = Arc::new(MockLlm::new());
        // first triple has invalid name characters; second is OK
        llm.push(
            r#"{"triples":[{"src":"X<>","rel":"knows","dst":"Y"},
                          {"src":"Carol","rel":"knows","dst":"Dan"}],"stop":true}"#,
        );
        let pol = LlmWriterPolicy::new(llm);
        let a = pol
            .step(&state(&[]), &Document::new(2, "irrelevant"))
            .await
            .unwrap();
        assert_eq!(a.triples.len(), 1);
        assert!(a.stop);
    }

    #[tokio::test]
    async fn bad_json_yields_writer_error() {
        let llm = Arc::new(MockLlm::new());
        llm.push("not json at all");
        let pol = LlmWriterPolicy::new(llm);
        let r = pol.step(&state(&[]), &Document::new(3, "x")).await;
        assert!(matches!(r, Err(SageError::Writer(_))));
    }
}
