//! SAGE core types & traits. Backend-agnostic.
//!
//! See `SPEC_SAGE_Rust.md` §2–§5 and `CONSTITUTION.md` §2 for boundaries.

pub mod document;
pub mod embed;
pub mod entity;
pub mod error;
pub mod graph;
pub mod ids;
pub mod ops;
pub mod query;
pub mod reader;
pub mod reward;

pub use document::Document;
pub use embed::{cosine, Embedder};
pub use entity::{Edge, Entity, EntityType, EDGE_SCHEMA_V, ENTITY_SCHEMA_V};
pub use error::{Result, SageError};
pub use graph::{GraphStore, SnapshotId, Subgraph};
pub use ids::{DocId, EntityId, Score, TenantId};
pub use ops::{scatter_add_1d, scatter_add_rows};
pub use query::{Constraint, Probe, Query, QueryPlan};
pub use reader::{EntityScan, ReadOutput, Reader, ReaderGraph, RelationPath};
pub use reward::{
    compute_reward, forgetting, habituation, precision, recovery, repetition_penalty, RewardCfg,
    RewardInputs, TaskWeights, WriterReward,
};
