//! SAGE core types & traits. Backend-agnostic.
//!
//! See `SPEC_SAGE_Rust.md` §2–§5 and `CONSTITUTION.md` §2 for boundaries.

pub mod document;
pub mod entity;
pub mod error;
pub mod graph;
pub mod ids;
pub mod query;
pub mod reward;

pub use document::Document;
pub use entity::{Edge, Entity, EntityType, EDGE_SCHEMA_V, ENTITY_SCHEMA_V};
pub use error::{Result, SageError};
pub use graph::{GraphStore, SnapshotId, Subgraph};
pub use ids::{DocId, EntityId, Score, TenantId};
pub use query::{Constraint, Probe, Query, QueryPlan};
pub use reward::{repetition_penalty, RewardCfg, TaskWeights, WriterReward};
