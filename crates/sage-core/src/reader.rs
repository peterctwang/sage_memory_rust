//! Reader trait — SPEC §5 / paper §4.2.
//!
//! M2 reader implementations (heuristic) take `&dyn GraphStore` directly.
//! Future GFM readers will move to `Arc<GraphSnapshot>` per SPEC §B.1.

use async_trait::async_trait;

use crate::entity::Entity;
use crate::error::Result;
use crate::graph::{GraphStore, Subgraph};
use crate::ids::{DocId, EntityId, Score, TenantId};
use crate::query::Query;

#[derive(Clone, Debug)]
pub struct RelationPath {
    pub nodes: Vec<EntityId>,
    pub relations: Vec<smol_str::SmolStr>,
}

#[derive(Clone, Debug, Default)]
pub struct ReadOutput {
    pub docs: Vec<(DocId, Score)>,
    pub entities: Vec<(EntityId, Score)>,
    pub subgraph: Subgraph,
    pub paths: Vec<RelationPath>,
}

/// Helper trait re-exported for readers that need to enumerate entities.
/// Not part of the M0 base trait so that minimal backends can skip it.
#[async_trait]
pub trait EntityScan: Send + Sync {
    async fn all_entities(&self, tenant: TenantId) -> Result<Vec<Entity>>;
    async fn find_by_name(&self, tenant: TenantId, name: &str) -> Result<Vec<EntityId>>;
}

/// Composite trait used by readers — any backend that implements both
/// `GraphStore` and `EntityScan` gets `ReaderGraph` for free.
pub trait ReaderGraph: GraphStore + EntityScan {}
impl<T: GraphStore + EntityScan + ?Sized> ReaderGraph for T {}

#[async_trait]
pub trait Reader: Send + Sync {
    async fn read(&self, tenant: TenantId, q: &Query, g: &dyn ReaderGraph) -> Result<ReadOutput>;
}
