//! Triple sanitizer — `SPEC §C.5`.
//!
//! All LLM-emitted triples MUST pass through this layer before reaching the graph.

use ahash::AHashSet;
use regex::Regex;
use smol_str::SmolStr;

use crate::action::RawTriple;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    NameTooLong,
    NameEmpty,
    NameInvalid,
    RelationEmpty,
    RelationUnknown,
    Blocked,
}

#[derive(Clone, Debug)]
pub struct SanitizerCfg {
    pub max_name_len: usize,
    pub max_triples_per_doc: usize,
    pub relation_vocab: AHashSet<SmolStr>,
    pub entity_name_re: Regex,
    pub blocklist: Vec<String>,
    /// If true, unknown relations are coerced to `related_to` instead of rejected.
    pub coerce_unknown_relation: bool,
}

impl Default for SanitizerCfg {
    fn default() -> Self {
        Self {
            max_name_len: 96,
            max_triples_per_doc: 64,
            relation_vocab: default_vocab(),
            entity_name_re: Regex::new(r"^[A-Za-z0-9 _.\-,/()']+$")
                .expect("default entity_name_re must compile"),
            blocklist: Vec::new(),
            coerce_unknown_relation: true,
        }
    }
}

fn default_vocab() -> AHashSet<SmolStr> {
    [
        "is_a",
        "part_of",
        "located_in",
        "founded_by",
        "founded",
        "works_at",
        "knows",
        "related_to",
        "causes",
        "treats",
        "next",
        "previous",
        "subclass_of",
        "instance_of",
        "occurred_at",
        "produced_by",
        "produced",
        "uses",
        "used_by",
        "depends_on",
        "supports",
    ]
    .into_iter()
    .map(SmolStr::new)
    .collect()
}

#[derive(Debug, Default)]
pub struct TripleSanitizer {
    cfg: SanitizerCfg,
}

impl TripleSanitizer {
    pub fn new(cfg: SanitizerCfg) -> Self {
        Self { cfg }
    }
    pub fn cfg(&self) -> &SanitizerCfg {
        &self.cfg
    }

    /// Returns `Ok(sanitized)` if the triple is acceptable (possibly with relation coerced),
    /// `Err(reason)` if it must be dropped.
    pub fn sanitize(&self, raw: RawTriple) -> Result<RawTriple, RejectReason> {
        check_name(&raw.src_name, &self.cfg)?;
        check_name(&raw.dst_name, &self.cfg)?;
        if raw.relation.is_empty() {
            return Err(RejectReason::RelationEmpty);
        }
        for needle in &self.cfg.blocklist {
            if raw.src_name.contains(needle.as_str())
                || raw.dst_name.contains(needle.as_str())
                || raw.relation.contains(needle.as_str())
            {
                return Err(RejectReason::Blocked);
            }
        }
        let mut out = raw;
        if !self.cfg.relation_vocab.contains(&out.relation) {
            if self.cfg.coerce_unknown_relation {
                out.relation = SmolStr::new("related_to");
            } else {
                return Err(RejectReason::RelationUnknown);
            }
        }
        Ok(out)
    }
}

fn check_name(name: &str, cfg: &SanitizerCfg) -> Result<(), RejectReason> {
    if name.is_empty() {
        return Err(RejectReason::NameEmpty);
    }
    if name.len() > cfg.max_name_len {
        return Err(RejectReason::NameTooLong);
    }
    if !cfg.entity_name_re.is_match(name) {
        return Err(RejectReason::NameInvalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(s: &str, r: &str, d: &str) -> RawTriple {
        RawTriple {
            src_name: SmolStr::new(s),
            relation: SmolStr::new(r),
            dst_name: SmolStr::new(d),
            src_type: None,
            dst_type: None,
        }
    }

    #[test]
    fn accepts_clean_triple_in_vocab() {
        let s = TripleSanitizer::default();
        let t = s.sanitize(raw("Alice", "knows", "Bob")).unwrap();
        assert_eq!(t.relation, "knows");
    }

    #[test]
    fn rejects_empty_name() {
        let s = TripleSanitizer::default();
        assert_eq!(
            s.sanitize(raw("", "knows", "Bob")).unwrap_err(),
            RejectReason::NameEmpty
        );
    }

    #[test]
    fn rejects_name_too_long() {
        let s = TripleSanitizer::default();
        let long = "a".repeat(200);
        assert_eq!(
            s.sanitize(raw(&long, "knows", "Bob")).unwrap_err(),
            RejectReason::NameTooLong
        );
    }

    #[test]
    fn rejects_invalid_chars() {
        let s = TripleSanitizer::default();
        assert_eq!(
            s.sanitize(raw("Alice<script>", "knows", "Bob"))
                .unwrap_err(),
            RejectReason::NameInvalid
        );
    }

    #[test]
    fn coerces_unknown_relation_when_enabled() {
        let s = TripleSanitizer::default();
        let t = s.sanitize(raw("Alice", "frobnicates", "Bob")).unwrap();
        assert_eq!(t.relation, "related_to");
    }

    #[test]
    fn rejects_unknown_relation_when_disabled() {
        let cfg = SanitizerCfg {
            coerce_unknown_relation: false,
            ..SanitizerCfg::default()
        };
        let s = TripleSanitizer::new(cfg);
        assert_eq!(
            s.sanitize(raw("Alice", "frobnicates", "Bob")).unwrap_err(),
            RejectReason::RelationUnknown
        );
    }

    #[test]
    fn blocklist_rejects_substring() {
        let cfg = SanitizerCfg {
            blocklist: vec!["secret".to_string()],
            ..SanitizerCfg::default()
        };
        let s = TripleSanitizer::new(cfg);
        assert_eq!(
            s.sanitize(raw("Alice secret", "knows", "Bob")).unwrap_err(),
            RejectReason::Blocked
        );
    }
}
