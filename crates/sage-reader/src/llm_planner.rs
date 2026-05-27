//! `LlmQueryPlanner` — uses an LLM to do cognition-inspired query expansion
//! (paper §4.2.1 / SPEC §5.1).
//!
//! For each query, the LLM outputs:
//!   - **expansions**: 3-5 named entities or key nouns the answer concerns
//!   - **aliases**: alternative names / abbreviations
//!   - **etype**: type hint (Person / Org / Concept / Event / Time)
//!   - **probes**: 1-3 paraphrases of the original question
//!
//! Designed to fix Tier-4 paraphrase failures in `eval_v3` where surface
//! tokens don't match doc text ("Liberator of South Africa" → Nelson Mandela).
//!
//! Results are cached per (query.text) — LLM calls are ~1-2s and queries
//! often repeat (eval harness, demo, dogfooding).

use std::sync::Arc;

use ahash::AHashMap;
use async_trait::async_trait;
use parking_lot::Mutex;
use sage_core::{EntityType, Probe, Query, QueryPlan, Result};
use sage_llm::{ChatMessage, ChatRequest, LlmClient};
use serde::Deserialize;
use smol_str::SmolStr;

use crate::planner::{HeuristicPlanner, QueryPlanner};

const SYSTEM_PROMPT: &str = r#"You are a retrieval query planner.
Given the user's question, output ONLY a JSON object (no prose, no markdown fences):
{
  "expansions": ["3-8 likely answer entities or key nouns"],
  "aliases":    ["common alternative names or abbreviations"],
  "etype":      "Person" | "Org" | "Concept" | "Event" | "Time" | null,
  "probes":     ["1-3 paraphrases of the question"]
}
Rules:
- expansions/aliases: short, just the name/term (no full sentences)
- etype: pick the type of the ANSWER entity, not the question's subject
- probes: full-sentence rewordings
- **Multi-entity queries** ("Two X who Y", "Three X who Y", "Both X", "X and Y"):
  list EVERY plausible answer entity by name in `expansions`. For example,
  "Two CEOs of Microsoft" → ["Bill Gates","Satya Nadella","Steve Ballmer"];
  "Two co-founders of OpenAI" → ["Sam Altman","Elon Musk","Greg Brockman","Ilya Sutskever"].
  Your enumeration directly drives retrieval — be exhaustive within 8 items."#;

#[derive(Debug, Deserialize)]
struct LlmPlanJson {
    #[serde(default)]
    expansions: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    etype: Option<String>,
    #[serde(default)]
    probes: Vec<String>,
}

fn parse_etype(s: Option<&str>) -> Option<EntityType> {
    match s.map(str::trim) {
        Some("Person") => Some(EntityType::Person),
        Some("Org") => Some(EntityType::Org),
        Some("Concept") => Some(EntityType::Concept),
        Some("Event") => Some(EntityType::Event),
        Some("Time") => Some(EntityType::Time),
        Some(other) if !other.is_empty() && other != "null" => {
            Some(EntityType::Custom(SmolStr::new(other)))
        }
        _ => None,
    }
}

fn tokenize_for_fallback(text: &str) -> Vec<SmolStr> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(str::to_lowercase)
        .map(SmolStr::new)
        .collect()
}

/// Extract the first JSON object from a response — Claude sometimes wraps it
/// in ```json fences or prose despite the system prompt.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

pub struct LlmQueryPlanner<L: LlmClient + ?Sized> {
    llm: Arc<L>,
    /// Fallback when LLM fails — keeps query path resilient.
    fallback: HeuristicPlanner,
    cache: Mutex<AHashMap<Arc<str>, QueryPlan>>,
    max_tokens: u32,
}

impl<L: LlmClient + ?Sized> std::fmt::Debug for LlmQueryPlanner<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmQueryPlanner")
            .field("cache_size", &self.cache.lock().len())
            .field("max_tokens", &self.max_tokens)
            .finish_non_exhaustive()
    }
}

impl<L: LlmClient + ?Sized> LlmQueryPlanner<L> {
    pub fn new(llm: Arc<L>) -> Self {
        Self {
            llm,
            fallback: HeuristicPlanner::new(),
            cache: Mutex::new(AHashMap::new()),
            max_tokens: 256,
        }
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn cache_size(&self) -> usize {
        self.cache.lock().len()
    }

    fn build_plan(q: &Query, raw: &LlmPlanJson) -> QueryPlan {
        let expansions: Vec<SmolStr> = raw
            .expansions
            .iter()
            .map(|s| SmolStr::new(s.trim().to_lowercase()))
            .filter(|s| !s.is_empty())
            .collect();
        let aliases: Vec<SmolStr> = raw
            .aliases
            .iter()
            .map(|s| SmolStr::new(s.trim().to_lowercase()))
            .filter(|s| !s.is_empty())
            .collect();
        let etype_hint = parse_etype(raw.etype.as_deref());

        // Build probes: full question always included with alpha=1.0;
        // LLM paraphrases get alpha=0.7 to stay slightly below the canonical query.
        let mut probes = vec![Probe {
            text: Arc::clone(&q.text),
            alpha: 1.0,
            etype: etype_hint.clone(),
        }];
        for p in &raw.probes {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                continue;
            }
            probes.push(Probe {
                text: Arc::<str>::from(trimmed),
                alpha: 0.7,
                etype: etype_hint.clone(),
            });
        }

        // Merge LLM expansions/aliases with heuristic tokens to keep recall
        // floor — if LLM misses a surface token the query had, we still match.
        let mut expansions_merged = expansions;
        for t in tokenize_for_fallback(&q.text) {
            if !expansions_merged.iter().any(|e| e == &t) {
                expansions_merged.push(t);
            }
        }
        QueryPlan {
            expansions: expansions_merged,
            aliases,
            relations: Vec::new(),
            hard_constraints: Vec::new(),
            etype_hint,
            probes,
        }
    }
}

#[async_trait]
impl<L: LlmClient + ?Sized> QueryPlanner for LlmQueryPlanner<L> {
    async fn plan(&self, q: &Query) -> Result<QueryPlan> {
        if let Some(p) = self.cache.lock().get(&q.text).cloned() {
            return Ok(p);
        }

        // Inline the system instructions into the user message so the entire
        // payload travels via stdin (avoids Windows .cmd argv quoting limits
        // for multi-line system prompts on the ClaudeCliLlm backend).
        let combined = format!("{SYSTEM_PROMPT}\n\nQuestion: {}", q.text);
        let req = ChatRequest {
            messages: vec![ChatMessage::user(combined)],
            temperature: 0.0,
            max_tokens: Some(self.max_tokens),
        };

        let resp = match self.llm.complete(req).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "LlmQueryPlanner: LLM call failed, falling back to heuristic");
                return Ok(self.fallback.plan_sync(q));
            }
        };

        let Some(json_blob) = extract_json_object(&resp.content) else {
            tracing::warn!(content = %resp.content, "LlmQueryPlanner: no JSON object found, falling back");
            return Ok(self.fallback.plan_sync(q));
        };
        let parsed: LlmPlanJson = match serde_json::from_str(json_blob) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, blob = %json_blob, "LlmQueryPlanner: JSON parse failed, falling back");
                return Ok(self.fallback.plan_sync(q));
            }
        };

        let plan = Self::build_plan(q, &parsed);
        self.cache.lock().insert(Arc::clone(&q.text), plan.clone());
        Ok(plan)
    }
}

// Convenience: allow constructing via `From<Arc<dyn LlmClient>>` etc. Skipped to
// keep the surface minimal — `Arc<MockLlm>`, `Arc<AnthropicLlm>`, `Arc<ClaudeCliLlm>`
// all work with `LlmQueryPlanner::new(arc)`.

#[cfg(test)]
mod tests {
    use super::*;
    use sage_llm::MockLlm;

    fn mock_with(json: &str) -> LlmQueryPlanner<MockLlm> {
        let m = Arc::new(MockLlm::new());
        m.push(json);
        LlmQueryPlanner::new(m)
    }

    #[tokio::test]
    async fn parses_well_formed_json() {
        let p = mock_with(
            r#"{"expansions":["Linus Torvalds","Linux"],"aliases":["Torvalds"],"etype":"Person","probes":["Who is the creator of Linux?"]}"#,
        );
        let plan = p.plan(&Query::ask("Who created Linux")).await.unwrap();
        assert!(plan.expansions.iter().any(|t| t == "linus torvalds"));
        assert!(plan.expansions.iter().any(|t| t == "linux"));
        assert!(plan.aliases.iter().any(|t| t == "torvalds"));
        assert_eq!(plan.etype_hint, Some(EntityType::Person));
        assert!(plan.probes.iter().any(|p| p.text.contains("creator")));
        // Original query is always preserved as the first probe with alpha=1.0
        assert_eq!(plan.probes[0].alpha, 1.0);
    }

    #[tokio::test]
    async fn falls_back_on_unparseable_response() {
        let p = mock_with("I'm sorry, I cannot do that.");
        let plan = p.plan(&Query::ask("Who created Linux")).await.unwrap();
        // Heuristic fallback still tokenizes the query
        assert!(plan.expansions.iter().any(|t| t == "linux"));
    }

    #[tokio::test]
    async fn falls_back_on_llm_error() {
        let p = LlmQueryPlanner::new(Arc::new(MockLlm::new())); // no scripted response
        let plan = p.plan(&Query::ask("anything")).await.unwrap();
        assert!(!plan.expansions.is_empty(), "fallback must produce tokens");
    }

    #[tokio::test]
    async fn caches_repeated_queries() {
        let p = mock_with(r#"{"expansions":["x"]}"#);
        let q = Query::ask("once");
        let a = p.plan(&q).await.unwrap();
        // Second call must NOT need another scripted response (MockLlm would error).
        let b = p.plan(&q).await.unwrap();
        assert_eq!(a.expansions, b.expansions);
        assert_eq!(p.cache_size(), 1);
    }

    #[tokio::test]
    async fn extracts_json_from_fenced_response() {
        let p = mock_with("```json\n{\"expansions\":[\"alice\"]}\n```");
        let plan = p.plan(&Query::ask("who is alice")).await.unwrap();
        assert!(plan.expansions.iter().any(|t| t == "alice"));
    }

    #[test]
    fn etype_parser_handles_known_and_unknown() {
        assert_eq!(parse_etype(Some("Person")), Some(EntityType::Person));
        assert_eq!(parse_etype(Some("Org")), Some(EntityType::Org));
        assert_eq!(parse_etype(Some("null")), None);
        assert_eq!(parse_etype(Some("")), None);
        assert_eq!(parse_etype(None), None);
        match parse_etype(Some("Country")) {
            Some(EntityType::Custom(s)) => assert_eq!(s.as_str(), "Country"),
            _ => panic!("unknown etype should become Custom"),
        }
    }
}
