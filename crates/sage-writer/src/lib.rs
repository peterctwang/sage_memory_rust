//! SAGE memory writer — see `SPEC §4.1` and paper §4.1.
//!
//! M1 surface: `WriterPolicy` trait, `LlmWriterPolicy`, `TripleSanitizer`,
//! and the `apply_action` function that lands a writer action on a `GraphStore`.

pub mod action;
pub mod apply;
pub mod llm_policy;
pub mod policy;
pub mod sanitizer;

pub use action::{EntityRef, RawTriple, WriterAction, WriterState};
pub use apply::apply_action;
pub use llm_policy::LlmWriterPolicy;
pub use policy::WriterPolicy;
pub use sanitizer::{RejectReason, SanitizerCfg, TripleSanitizer};
