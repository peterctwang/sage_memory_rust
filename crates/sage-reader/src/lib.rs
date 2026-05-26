//! SAGE memory reader.
//!
//! M2 ships heuristic planner + soft addressing (SPEC §5.1–§5.2).
//! GFM propagation (§5.3) and Context/Schema head (§5.4) are deferred to M3.

pub mod addressing;
pub mod gfm;
pub mod gfm_reader;
pub mod gfm_stack;
pub mod heuristic;
pub mod llm_planner;
pub mod planner;

pub use addressing::{score_entry, softmax_entry, AddressingWeights};
pub use gfm::{GfmConfig, GfmGraphView, GfmLayer};
pub use gfm_reader::GfmReader;
pub use gfm_stack::{ContextSchemaHead, GfmStack};
pub use heuristic::HeuristicReader;
pub use llm_planner::LlmQueryPlanner;
pub use planner::{HeuristicPlanner, QueryPlanner};
