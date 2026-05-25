//! Property tests for `TripleSanitizer` — must never panic on arbitrary input
//! and accepted output must always satisfy the sanitizer's own validation rules.

use proptest::prelude::*;
use sage_writer::{RawTriple, TripleSanitizer};
use smol_str::SmolStr;

fn arb_raw() -> impl Strategy<Value = RawTriple> {
    (".{0,150}", ".{0,40}", ".{0,150}").prop_map(|(s, r, d)| RawTriple {
        src_name: SmolStr::new(s),
        relation: SmolStr::new(r),
        dst_name: SmolStr::new(d),
        src_type: None,
        dst_type: None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn sanitize_never_panics(raw in arb_raw()) {
        let s = TripleSanitizer::default();
        let _ = s.sanitize(raw);
    }

    #[test]
    fn accepted_triple_is_idempotent(raw in arb_raw()) {
        let s = TripleSanitizer::default();
        if let Ok(first) = s.sanitize(raw) {
            let again = s.sanitize(first.clone()).expect("re-sanitize must succeed");
            prop_assert_eq!(first.src_name, again.src_name);
            prop_assert_eq!(first.dst_name, again.dst_name);
            prop_assert_eq!(first.relation, again.relation);
        }
    }

    #[test]
    fn accepted_names_within_length_limit(raw in arb_raw()) {
        let s = TripleSanitizer::default();
        if let Ok(t) = s.sanitize(raw) {
            prop_assert!(t.src_name.len() <= s.cfg().max_name_len);
            prop_assert!(t.dst_name.len() <= s.cfg().max_name_len);
            prop_assert!(!t.src_name.is_empty());
            prop_assert!(!t.dst_name.is_empty());
        }
    }
}
