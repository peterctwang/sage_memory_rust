//! In-memory `GraphStore` for tests, prototyping, and single-process agents.

use std::collections::HashMap;

use ahash::AHashMap;
use async_trait::async_trait;
use parking_lot::RwLock;
use sage_core::{
    DocId, Edge, Entity, EntityId, GraphStore, Result, SageError, SnapshotId, Subgraph, TenantId,
};

#[derive(Default, Debug, Clone)]
struct TenantData {
    entities: AHashMap<EntityId, Entity>,
    /// adjacency: src -> Vec<Edge>
    out_edges: AHashMap<EntityId, Vec<Edge>>,
    /// reverse map for docs_of_entity
    doc_index: AHashMap<EntityId, Vec<DocId>>,
    /// snapshots
    snapshots: HashMap<SnapshotId, Box<SnapshotPayload>>,
    next_snap: u64,
}

#[derive(Debug, Clone)]
struct SnapshotPayload {
    entities: AHashMap<EntityId, Entity>,
    out_edges: AHashMap<EntityId, Vec<Edge>>,
    doc_index: AHashMap<EntityId, Vec<DocId>>,
}

#[derive(Default, Debug)]
pub struct MemGraphStore {
    tenants: RwLock<AHashMap<TenantId, TenantData>>,
}

impl MemGraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_tenant_mut<R>(&self, t: TenantId, f: impl FnOnce(&mut TenantData) -> R) -> R {
        let mut g = self.tenants.write();
        let data = g.entry(t).or_default();
        f(data)
    }

    fn with_tenant<R>(&self, t: TenantId, f: impl FnOnce(&TenantData) -> R) -> Option<R> {
        let g = self.tenants.read();
        g.get(&t).map(f)
    }
}

#[async_trait]
impl GraphStore for MemGraphStore {
    async fn upsert_entity(&self, t: TenantId, mut e: Entity) -> Result<EntityId> {
        e.tenant = t;
        self.with_tenant_mut(t, |data| {
            for d in &e.source_docs {
                data.doc_index.entry(e.id).or_default().push(*d);
            }
            data.entities.insert(e.id, e.clone());
            Ok(e.id)
        })
    }

    async fn upsert_edge(&self, t: TenantId, mut e: Edge) -> Result<()> {
        e.tenant = t;
        self.with_tenant_mut(t, |data| {
            if !data.entities.contains_key(&e.src) || !data.entities.contains_key(&e.dst) {
                return Err(SageError::Invalid(format!(
                    "edge endpoints missing: {} -> {}",
                    e.src, e.dst
                )));
            }
            data.out_edges.entry(e.src).or_default().push(e);
            Ok(())
        })
    }

    async fn get_entity(&self, t: TenantId, id: EntityId) -> Result<Option<Entity>> {
        Ok(self
            .with_tenant(t, |data| data.entities.get(&id).cloned())
            .flatten())
    }

    async fn neighbors(
        &self,
        t: TenantId,
        id: EntityId,
        max: usize,
    ) -> Result<Vec<(EntityId, Edge)>> {
        Ok(self
            .with_tenant(t, |data| {
                data.out_edges
                    .get(&id)
                    .map(|v| v.iter().take(max).map(|e| (e.dst, e.clone())).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default())
    }

    async fn k_hop(&self, t: TenantId, seeds: &[EntityId], hops: u8) -> Result<Subgraph> {
        let mut sg = Subgraph::default();
        let mut frontier: Vec<EntityId> = seeds.to_vec();
        let mut seen_entities: ahash::AHashSet<EntityId> = frontier.iter().copied().collect();

        let snapshot = {
            let g = self.tenants.read();
            g.get(&t).cloned()
        };
        let Some(data) = snapshot else {
            return Ok(sg);
        };

        for &id in &frontier {
            if let Some(e) = data.entities.get(&id) {
                sg.entities.push(e.clone());
            }
        }
        for _ in 0..hops {
            let mut next: Vec<EntityId> = Vec::new();
            for &v in &frontier {
                if let Some(edges) = data.out_edges.get(&v) {
                    for e in edges {
                        sg.edges.push(e.clone());
                        if seen_entities.insert(e.dst) {
                            if let Some(ent) = data.entities.get(&e.dst) {
                                sg.entities.push(ent.clone());
                            }
                            next.push(e.dst);
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(sg)
    }

    async fn docs_of_entity(&self, t: TenantId, id: EntityId) -> Result<Vec<DocId>> {
        Ok(self
            .with_tenant(t, |data| {
                data.doc_index.get(&id).cloned().unwrap_or_default()
            })
            .unwrap_or_default())
    }

    async fn snapshot(&self, t: TenantId) -> Result<SnapshotId> {
        self.with_tenant_mut(t, |data| {
            let id = SnapshotId(data.next_snap);
            data.next_snap += 1;
            let payload = SnapshotPayload {
                entities: data.entities.clone(),
                out_edges: data.out_edges.clone(),
                doc_index: data.doc_index.clone(),
            };
            data.snapshots.insert(id, Box::new(payload));
            Ok(id)
        })
    }

    async fn restore(&self, t: TenantId, snap: SnapshotId) -> Result<()> {
        self.with_tenant_mut(t, |data| {
            let p = data
                .snapshots
                .get(&snap)
                .ok_or_else(|| SageError::NotFound(format!("snapshot {snap:?}")))?
                .clone();
            data.entities = p.entities;
            data.out_edges = p.out_edges;
            data.doc_index = p.doc_index;
            Ok(())
        })
    }

    async fn entity_count(&self, t: TenantId) -> Result<usize> {
        Ok(self.with_tenant(t, |d| d.entities.len()).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_core::EntityType;

    fn ent(id: EntityId, name: &str) -> Entity {
        Entity::new(id, name, EntityType::Concept)
    }

    #[tokio::test]
    async fn upsert_and_get_entity() {
        let g = MemGraphStore::new();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "A"))
            .await
            .unwrap();
        let got = g.get_entity(TenantId::DEFAULT, 1).await.unwrap();
        assert_eq!(got.unwrap().name, "A");
    }

    #[tokio::test]
    async fn upsert_edge_requires_endpoints() {
        let g = MemGraphStore::new();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "A"))
            .await
            .unwrap();
        let bad = Edge::new(1, 99, "rel", 0);
        let res = g.upsert_edge(TenantId::DEFAULT, bad).await;
        assert!(matches!(res, Err(SageError::Invalid(_))));
    }

    #[tokio::test]
    async fn neighbors_returns_outgoing() {
        let g = MemGraphStore::new();
        for i in 1..=3 {
            g.upsert_entity(TenantId::DEFAULT, ent(i, "n"))
                .await
                .unwrap();
        }
        g.upsert_edge(TenantId::DEFAULT, Edge::new(1, 2, "r", 0))
            .await
            .unwrap();
        g.upsert_edge(TenantId::DEFAULT, Edge::new(1, 3, "r", 0))
            .await
            .unwrap();
        let n = g.neighbors(TenantId::DEFAULT, 1, 10).await.unwrap();
        assert_eq!(n.len(), 2);
    }

    #[tokio::test]
    async fn k_hop_walks_two_hops() {
        let g = MemGraphStore::new();
        for i in 1..=4 {
            g.upsert_entity(TenantId::DEFAULT, ent(i, "n"))
                .await
                .unwrap();
        }
        g.upsert_edge(TenantId::DEFAULT, Edge::new(1, 2, "r", 0))
            .await
            .unwrap();
        g.upsert_edge(TenantId::DEFAULT, Edge::new(2, 3, "r", 0))
            .await
            .unwrap();
        g.upsert_edge(TenantId::DEFAULT, Edge::new(3, 4, "r", 0))
            .await
            .unwrap();

        let sg1 = g.k_hop(TenantId::DEFAULT, &[1], 1).await.unwrap();
        let ids1: ahash::AHashSet<_> = sg1.entities.iter().map(|e| e.id).collect();
        assert!(ids1.contains(&1) && ids1.contains(&2));
        assert!(!ids1.contains(&4));

        let sg2 = g.k_hop(TenantId::DEFAULT, &[1], 2).await.unwrap();
        let ids2: ahash::AHashSet<_> = sg2.entities.iter().map(|e| e.id).collect();
        assert!(ids2.contains(&3));
    }

    #[tokio::test]
    async fn tenants_are_isolated() {
        let g = MemGraphStore::new();
        let t1 = TenantId(1);
        let t2 = TenantId(2);
        g.upsert_entity(t1, ent(1, "A")).await.unwrap();
        assert_eq!(g.entity_count(t1).await.unwrap(), 1);
        assert_eq!(g.entity_count(t2).await.unwrap(), 0);
        assert!(g.get_entity(t2, 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn snapshot_restore_roundtrip() {
        let g = MemGraphStore::new();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "A"))
            .await
            .unwrap();
        let snap = g.snapshot(TenantId::DEFAULT).await.unwrap();
        g.upsert_entity(TenantId::DEFAULT, ent(2, "B"))
            .await
            .unwrap();
        assert_eq!(g.entity_count(TenantId::DEFAULT).await.unwrap(), 2);
        g.restore(TenantId::DEFAULT, snap).await.unwrap();
        assert_eq!(g.entity_count(TenantId::DEFAULT).await.unwrap(), 1);
    }
}
