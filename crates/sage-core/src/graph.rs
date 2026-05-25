//! Graph store trait & shared graph types.
//!
//! Implementations live in `sage-graph`. M0 surface is intentionally minimal —
//! `apply_action`, `GraphSnapshot`, `StructuralCache` etc. land in later milestones.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::entity::{Edge, Entity};
use crate::error::Result;
use crate::ids::{DocId, EntityId, TenantId};

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct SnapshotId(pub u64);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Subgraph {
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
}

impl Subgraph {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.edges.is_empty()
    }
}

#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn upsert_entity(&self, t: TenantId, e: Entity) -> Result<EntityId>;
    async fn upsert_edge(&self, t: TenantId, e: Edge) -> Result<()>;
    async fn get_entity(&self, t: TenantId, id: EntityId) -> Result<Option<Entity>>;
    async fn neighbors(
        &self,
        t: TenantId,
        id: EntityId,
        max: usize,
    ) -> Result<Vec<(EntityId, Edge)>>;
    async fn k_hop(&self, t: TenantId, seeds: &[EntityId], hops: u8) -> Result<Subgraph>;
    async fn docs_of_entity(&self, t: TenantId, id: EntityId) -> Result<Vec<DocId>>;
    async fn snapshot(&self, t: TenantId) -> Result<SnapshotId>;
    async fn restore(&self, t: TenantId, snap: SnapshotId) -> Result<()>;

    /// Total entity count for this tenant (test/observability helper).
    async fn entity_count(&self, t: TenantId) -> Result<usize>;
}
