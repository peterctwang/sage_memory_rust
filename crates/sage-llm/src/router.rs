//! Heuristic LLM router — picks `light` vs `deep` backend per request based
//! on the **user-message complexity profile**.
//!
//! Calibrated against `eval_v7` (100-doc multi-hop benchmark):
//! - Single-fact one-sentence prompts → light backend (cheap, fast).
//! - Multi-sentence paragraphs with co-mentioned entities → deep backend
//!   (Codex / Claude — both demonstrated +0.03~+0.05 multi-hop R@3 over
//!   MiniMax on v7's tier-5 / tier-6 queries).
//!
//! Routing is transparent: `HeuristicRouter` implements `LlmClient`, so any
//! existing caller (writer / planner / judge) sees the same trait surface.
//! The judge path always uses `light` — YES/NO verdicts don't benefit from
//! deeper reasoning and judging in bulk on the deep model wastes 20× cost.
//!
//! ## Profile score
//!   score = char_len / 50
//!         + capitalized_phrase_count * 5
//!         + sentence_count * 2
//!
//! Empirically derived weights:
//! - char_len/50: ~5 points for a 250-char paragraph.
//! - 5 per capitalized phrase: an entity-rich doc (6 entities) adds 30.
//! - 2 per sentence: structural complexity.
//!
//! Threshold default 25 splits v7's two extremes cleanly:
//!   "Bill Gates founded Microsoft in 1975."          → score ~13 → light
//!   doc 1001 (Gates/Ballmer/Nadella paragraph)        → score ~41 → deep

use std::sync::Arc;

use async_trait::async_trait;
use sage_core::Result;

use crate::{ChatRequest, ChatResponse, LlmClient, Role};

const DEFAULT_THRESHOLD: u32 = 25;

#[derive(Debug, Clone, Copy)]
pub struct DocProfile {
    pub char_len: u32,
    pub sentence_count: u32,
    /// Rough count of `[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*` runs — a proxy for
    /// named-entity density without dragging in a real NER model.
    pub capitalized_phrase_count: u32,
}

impl DocProfile {
    /// Single complexity score; higher = needs deeper model.
    /// See module docs for the weight derivation.
    pub fn score(&self) -> u32 {
        self.char_len / 50 + self.capitalized_phrase_count * 5 + self.sentence_count * 2
    }
}

/// Compute the profile from the concatenated User-role content of a request.
/// System messages are excluded — they're typically fixed boilerplate that
/// doesn't reflect the document under test.
pub fn profile_user_content(req: &ChatRequest) -> DocProfile {
    let mut buf = String::new();
    for m in &req.messages {
        if matches!(m.role, Role::User) {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(&m.content);
        }
    }
    profile_text(&buf)
}

/// Profile a raw text blob. Exposed for tests and direct use by callers
/// that already have the doc text (e.g. writer-side per-doc routing).
pub fn profile_text(text: &str) -> DocProfile {
    let char_len = text.chars().count() as u32;

    let sentence_count = text
        .chars()
        .filter(|c| matches!(c, '.' | '?' | '!'))
        .count() as u32;

    // Capitalized-phrase detection: walk tokens, count runs of consecutive
    // tokens that begin with an uppercase letter and aren't at sentence start.
    // We accept the false-positive on sentence-leading words because they
    // also tend to encode information and the cost is symmetric.
    let mut phrase_count = 0u32;
    let mut in_phrase = false;
    for tok in text.split(|c: char| !c.is_alphanumeric()) {
        let first = tok.chars().next();
        let is_cap = first.is_some_and(char::is_uppercase);
        if is_cap {
            if !in_phrase {
                phrase_count += 1;
                in_phrase = true;
            }
        } else {
            in_phrase = false;
        }
    }

    DocProfile {
        char_len,
        sentence_count,
        capitalized_phrase_count: phrase_count,
    }
}

/// Threshold below which a response is considered "empty" and triggers
/// the cross-arm failover. Matches the FallbackLlm behavior so the two
/// composition layers (router-level failover, single-backend FallbackLlm)
/// have consistent semantics.
const EMPTY_THRESHOLD_CHARS: usize = 2;

fn is_empty_payload(s: &str) -> bool {
    s.trim().chars().count() < EMPTY_THRESHOLD_CHARS
}

/// Pick `light` vs `deep` based on a `DocProfile` score threshold, with
/// **automatic cross-arm failover** — if the primary arm errors or
/// returns an empty payload, the other arm is tried before bubbling the
/// failure up.
///
/// Failover use cases observed in production:
/// - Codex (deep arm) hits ChatGPT subscription rate limit mid-batch.
///   Router falls back to MiniMax (light arm) which has its own quota.
/// - MiniMax (light arm) hits the 1500-call / 5h hard cap. Router falls
///   back to Codex (deep) for short docs.
/// - Either subprocess CLI crashes on a malformed input. The other arm
///   gets a chance.
#[derive(Clone)]
pub struct HeuristicRouter {
    light: Arc<dyn LlmClient>,
    deep: Arc<dyn LlmClient>,
    threshold: u32,
    /// Atomic counters for telemetry (which arm was used how often).
    light_hits: Arc<std::sync::atomic::AtomicU64>,
    deep_hits: Arc<std::sync::atomic::AtomicU64>,
    /// Counts cross-arm failovers (primary arm produced empty / errored,
    /// alternate arm was tried).
    failover_hits: Arc<std::sync::atomic::AtomicU64>,
}

impl std::fmt::Debug for HeuristicRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeuristicRouter")
            .field("threshold", &self.threshold)
            .field(
                "light_hits",
                &self.light_hits.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field(
                "deep_hits",
                &self.deep_hits.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field(
                "failover_hits",
                &self
                    .failover_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl HeuristicRouter {
    pub fn new(light: Arc<dyn LlmClient>, deep: Arc<dyn LlmClient>) -> Self {
        Self {
            light,
            deep,
            threshold: DEFAULT_THRESHOLD,
            light_hits: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            deep_hits: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            failover_hits: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn with_threshold(mut self, t: u32) -> Self {
        self.threshold = t;
        self
    }

    pub fn light_hits(&self) -> u64 {
        self.light_hits.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn deep_hits(&self) -> u64 {
        self.deep_hits.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// How many times the primary arm errored / returned empty and the
    /// alternate arm was tried.
    pub fn failover_hits(&self) -> u64 {
        self.failover_hits
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Pick the (primary, alternate) backend pair for a given profile.
    /// Bumps the relevant primary-arm hit counter. The alternate is the
    /// arm we fall back to if the primary errors or returns empty.
    fn pick_pair(
        &self,
        profile: &DocProfile,
    ) -> (&Arc<dyn LlmClient>, &Arc<dyn LlmClient>, &'static str) {
        if profile.score() >= self.threshold {
            self.deep_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (&self.deep, &self.light, "deep")
        } else {
            self.light_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (&self.light, &self.deep, "light")
        }
    }
}

#[async_trait]
impl LlmClient for HeuristicRouter {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let profile = profile_user_content(&req);
        let (primary, alternate, primary_name) = self.pick_pair(&profile);
        tracing::debug!(
            score = profile.score(),
            threshold = self.threshold,
            arm = primary_name,
            "router routing decision"
        );

        // Try the primary arm first. On error OR empty payload, fall over
        // to the alternate arm so a single quota / rate-limit / parse
        // failure doesn't drop the doc.
        match primary.complete(req.clone()).await {
            Ok(r) if !is_empty_payload(&r.content) => Ok(r),
            Ok(_) => {
                tracing::warn!(
                    primary = primary_name,
                    "router primary returned empty — failing over to alternate arm"
                );
                self.failover_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                alternate.complete(req).await
            }
            Err(e) => {
                tracing::warn!(
                    primary = primary_name,
                    error = %e,
                    "router primary errored — failing over to alternate arm"
                );
                self.failover_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                alternate.complete(req).await
            }
        }
    }

    async fn judge(&self, q: &str, y: &str, ev: &[String]) -> Result<bool> {
        // Judge is always a YES/NO classification — never benefits from the
        // deep model. Force `light` regardless of evidence size; on error,
        // fall over to deep so a transient failure still gets a verdict.
        self.light_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match self.light.judge(q, y, ev).await {
            Ok(b) => Ok(b),
            Err(e) => {
                tracing::warn!(error = %e, "router judge light errored — failing over to deep arm");
                self.failover_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.deep.judge(q, y, ev).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatMessage, MockLlm};

    #[test]
    fn score_of_short_simple_fact_is_low() {
        let p = profile_text("Bill Gates founded Microsoft in 1975.");
        // chars≈37→0; entities≈2→10; sentences=1→2; total=12
        assert!(p.score() < DEFAULT_THRESHOLD, "got {p:?}");
    }

    #[test]
    fn score_of_paragraph_with_many_entities_is_high() {
        let p = profile_text(
            "Bill Gates co-founded Microsoft with Paul Allen in 1975. \
             He served as CEO until 2000, when Steve Ballmer took over. \
             In 2014, Satya Nadella succeeded Ballmer and pivoted to Azure.",
        );
        // Long + many caps → should score well above threshold.
        assert!(p.score() >= DEFAULT_THRESHOLD, "got {p:?}");
    }

    #[test]
    fn capitalized_phrase_count_tracks_runs_not_words() {
        // "Bill Gates" = 1 phrase (two consecutive caps), "Paul Allen" = 1.
        let p = profile_text("Bill Gates founded Microsoft. Paul Allen co-founded it.");
        // Bill Gates / Microsoft / Paul Allen → 3 phrases.
        assert_eq!(p.capitalized_phrase_count, 3, "got {p:?}");
    }

    #[test]
    fn sentence_count_handles_multiple_terminators() {
        let p = profile_text("Yes. Really? Absolutely!");
        assert_eq!(p.sentence_count, 3);
    }

    #[test]
    fn profile_user_content_ignores_system_role() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::system("You are a long detailed system prompt with many capitalized words like Foo, Bar, Baz, Qux."),
                ChatMessage::user("short."),
            ],
            temperature: 0.0,
            max_tokens: None,
        };
        let p = profile_user_content(&req);
        // Just "short." → low score regardless of system.
        assert!(p.score() < 5, "got {p:?}");
    }

    #[tokio::test]
    async fn router_picks_light_for_short_prompt() {
        let light = Arc::new(MockLlm::new());
        light.push("from light");
        let deep = Arc::new(MockLlm::new()); // empty — would error if hit
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("hi.")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "from light");
        assert_eq!(router.light_hits(), 1);
        assert_eq!(router.deep_hits(), 0);
    }

    #[tokio::test]
    async fn router_picks_deep_for_long_multi_entity_paragraph() {
        let light = Arc::new(MockLlm::new()); // empty — would error if hit
        let deep = Arc::new(MockLlm::new());
        deep.push("from deep");
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user(
                    "Bill Gates co-founded Microsoft with Paul Allen in 1975. \
                     He served as CEO until 2000, when Steve Ballmer took over. \
                     In 2014, Satya Nadella succeeded Ballmer.",
                )],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "from deep");
        assert_eq!(router.light_hits(), 0);
        assert_eq!(router.deep_hits(), 1);
    }

    #[tokio::test]
    async fn judge_always_uses_light_regardless_of_evidence_size() {
        let light = Arc::new(MockLlm::new());
        light.push_judge(true);
        let deep = Arc::new(MockLlm::new()); // empty — would error if hit
        let router = HeuristicRouter::new(light, deep);
        // Construct a huge evidence list that would otherwise tip toward deep.
        let big_ev: Vec<String> = (0..50)
            .map(|i| {
                format!(
                    "Massive evidence line number {i} with Many Capitalized Names like Foo Bar Baz"
                )
            })
            .collect();
        let verdict = router.judge("Q?", "A", &big_ev).await.unwrap();
        assert!(verdict);
        assert_eq!(router.light_hits(), 1);
        assert_eq!(router.deep_hits(), 0);
    }

    #[tokio::test]
    async fn deep_arm_error_failovers_to_light() {
        // Long paragraph → routes to deep. Deep errors (empty queue) → light catches.
        let light = Arc::new(MockLlm::new());
        light.push("rescued by light");
        let deep = Arc::new(MockLlm::new()); // empty queue → MockLlm returns error
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user(
                    "Bill Gates co-founded Microsoft with Paul Allen in 1975. \
                     He served as CEO until 2000, when Steve Ballmer took over. \
                     In 2014, Satya Nadella succeeded Ballmer.",
                )],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "rescued by light");
        assert_eq!(router.deep_hits(), 1, "deep was the primary arm");
        assert_eq!(router.failover_hits(), 1, "failover fired exactly once");
    }

    #[tokio::test]
    async fn light_arm_error_failovers_to_deep() {
        // Short prompt → routes to light. Light errors → deep catches.
        let light = Arc::new(MockLlm::new()); // empty queue
        let deep = Arc::new(MockLlm::new());
        deep.push("rescued by deep");
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("hi.")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "rescued by deep");
        assert_eq!(router.light_hits(), 1);
        assert_eq!(router.failover_hits(), 1);
    }

    #[tokio::test]
    async fn deep_arm_empty_response_failovers_to_light() {
        let light = Arc::new(MockLlm::new());
        light.push("from light");
        let deep = Arc::new(MockLlm::new());
        deep.push(""); // empty payload → failover
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user(
                    "Bill Gates co-founded Microsoft with Paul Allen in 1975. \
                     He served as CEO until 2000, when Steve Ballmer took over. \
                     In 2014, Satya Nadella succeeded Ballmer and pivoted to Azure.",
                )],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "from light");
        assert_eq!(router.failover_hits(), 1);
    }

    #[tokio::test]
    async fn both_arms_error_bubbles_failure() {
        let light = Arc::new(MockLlm::new()); // empty
        let deep = Arc::new(MockLlm::new()); // empty
        let router = HeuristicRouter::new(light, deep);
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("anything")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await;
        assert!(r.is_err(), "both arms failed — error must propagate");
        // Failover was attempted once.
        assert_eq!(router.failover_hits(), 1);
    }

    #[tokio::test]
    async fn judge_failovers_from_light_to_deep() {
        let light = Arc::new(MockLlm::new()); // empty → judge will error
        let deep = Arc::new(MockLlm::new());
        deep.push_judge(true);
        let router = HeuristicRouter::new(light, deep);
        let v = router.judge("q", "y", &["e".to_string()]).await.unwrap();
        assert!(v);
        assert_eq!(router.failover_hits(), 1);
    }

    #[tokio::test]
    async fn threshold_override_changes_decision_boundary() {
        let light = Arc::new(MockLlm::new()); // would error if hit
        light.push("light-response");
        let deep = Arc::new(MockLlm::new());
        deep.push("deep-response"); // 2+ chars so failover doesn't misfire
        let router = HeuristicRouter::new(light, deep).with_threshold(5);
        // Even "Bill Gates founded Microsoft" should now route to deep.
        let r = router
            .complete(ChatRequest {
                messages: vec![ChatMessage::user("Bill Gates founded Microsoft.")],
                temperature: 0.0,
                max_tokens: None,
            })
            .await
            .unwrap();
        assert_eq!(r.content, "deep-response");
        assert_eq!(router.threshold(), 5);
        assert_eq!(router.failover_hits(), 0, "deep succeeded; no failover");
    }
}
