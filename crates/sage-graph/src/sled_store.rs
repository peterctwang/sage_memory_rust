//! Persistent `GraphStore` + `EntityScan` backed by `sled`.
//!
//! Key layout (single tree):
//! ```text
//! e<t:u64_be><id:u64_be>                  → Entity (JSON)
//! o<t:u64_be><src:u64_be><seq:u64_be>     → Edge (JSON)
//! d<t:u64_be><id:u64_be>                  → Vec<DocId> (JSON)
//! s<t:u64_be><snap:u64_be>                → SnapshotPayload (JSON)
//! m<t:u64_be>                             → TenantMeta (JSON)
//! ```

use std::path::Path;

use async_trait::async_trait;
use sage_core::{
    DocId, Edge, Entity, EntityId, EntityScan, GraphStore, Result, SageError, SnapshotId, Subgraph,
    TenantId,
};
use serde::{Deserialize, Serialize};

const K_ENT: u8 = b'e';
const K_OUT: u8 = b'o';
const K_DOC: u8 = b'd';
const K_SNP: u8 = b's';
const K_MET: u8 = b'm';

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct TenantMeta {
    next_snap: u64,
    next_edge_seq: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SnapshotPayload {
    entities: Vec<Entity>,
    out_edges: Vec<Edge>,
    doc_index: Vec<(EntityId, Vec<DocId>)>,
}

pub struct SledGraphStore {
    db: sled::Db,
}

impl std::fmt::Debug for SledGraphStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SledGraphStore")
            .field("path", &self.db.size_on_disk().ok())
            .finish_non_exhaustive()
    }
}

impl SledGraphStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = sled::open(path).map_err(|e| SageError::Graph(e.to_string()))?;
        Ok(Self { db })
    }

    pub fn temporary() -> Result<Self> {
        let db = sled::Config::new()
            .temporary(true)
            .open()
            .map_err(|e| SageError::Graph(e.to_string()))?;
        Ok(Self { db })
    }

    fn json_to<T: Serialize>(v: &T) -> Result<Vec<u8>> {
        serde_json::to_vec(v).map_err(SageError::from)
    }
    fn json_from<T: for<'de> Deserialize<'de>>(b: &[u8]) -> Result<T> {
        serde_json::from_slice(b).map_err(SageError::from)
    }

    fn ent_key(t: TenantId, id: EntityId) -> Vec<u8> {
        let mut k = Vec::with_capacity(17);
        k.push(K_ENT);
        k.extend_from_slice(&t.0.to_be_bytes());
        k.extend_from_slice(&id.to_be_bytes());
        k
    }
    fn ent_prefix(t: TenantId) -> Vec<u8> {
        let mut k = Vec::with_capacity(9);
        k.push(K_ENT);
        k.extend_from_slice(&t.0.to_be_bytes());
        k
    }
    fn out_key(t: TenantId, src: EntityId, seq: u64) -> Vec<u8> {
        let mut k = Vec::with_capacity(25);
        k.push(K_OUT);
        k.extend_from_slice(&t.0.to_be_bytes());
        k.extend_from_slice(&src.to_be_bytes());
        k.extend_from_slice(&seq.to_be_bytes());
        k
    }
    fn out_prefix_src(t: TenantId, src: EntityId) -> Vec<u8> {
        let mut k = Vec::with_capacity(17);
        k.push(K_OUT);
        k.extend_from_slice(&t.0.to_be_bytes());
        k.extend_from_slice(&src.to_be_bytes());
        k
    }
    fn out_prefix_tenant(t: TenantId) -> Vec<u8> {
        let mut k = Vec::with_capacity(9);
        k.push(K_OUT);
        k.extend_from_slice(&t.0.to_be_bytes());
        k
    }
    fn doc_key(t: TenantId, id: EntityId) -> Vec<u8> {
        let mut k = Vec::with_capacity(17);
        k.push(K_DOC);
        k.extend_from_slice(&t.0.to_be_bytes());
        k.extend_from_slice(&id.to_be_bytes());
        k
    }
    fn doc_prefix(t: TenantId) -> Vec<u8> {
        let mut k = Vec::with_capacity(9);
        k.push(K_DOC);
        k.extend_from_slice(&t.0.to_be_bytes());
        k
    }
    fn snap_key(t: TenantId, snap: u64) -> Vec<u8> {
        let mut k = Vec::with_capacity(17);
        k.push(K_SNP);
        k.extend_from_slice(&t.0.to_be_bytes());
        k.extend_from_slice(&snap.to_be_bytes());
        k
    }
    fn meta_key(t: TenantId) -> Vec<u8> {
        let mut k = Vec::with_capacity(9);
        k.push(K_MET);
        k.extend_from_slice(&t.0.to_be_bytes());
        k
    }

    fn read_meta(&self, t: TenantId) -> Result<TenantMeta> {
        match self.db.get(Self::meta_key(t)).map_err(sled_err)? {
            Some(b) => Self::json_from(&b),
            None => Ok(TenantMeta::default()),
        }
    }
    fn write_meta(&self, t: TenantId, m: &TenantMeta) -> Result<()> {
        self.db
            .insert(Self::meta_key(t), Self::json_to(m)?)
            .map_err(sled_err)?;
        Ok(())
    }

    fn collect_outgoing(&self, t: TenantId, src: EntityId) -> Result<Vec<Edge>> {
        let prefix = Self::out_prefix_src(t, src);
        let mut out = Vec::new();
        for kv in self.db.scan_prefix(&prefix) {
            let (_, v) = kv.map_err(sled_err)?;
            out.push(Self::json_from::<Edge>(&v)?);
        }
        Ok(out)
    }
}

fn sled_err(e: sled::Error) -> SageError {
    SageError::Graph(e.to_string())
}

#[async_trait]
impl GraphStore for SledGraphStore {
    async fn upsert_entity(&self, t: TenantId, mut e: Entity) -> Result<EntityId> {
        e.tenant = t;
        // Update doc_index for any new source_docs.
        let dkey = Self::doc_key(t, e.id);
        let mut docs: Vec<DocId> = self
            .db
            .get(&dkey)
            .map_err(sled_err)?
            .map(|b| Self::json_from::<Vec<DocId>>(&b))
            .transpose()?
            .unwrap_or_default();
        for d in &e.source_docs {
            if !docs.contains(d) {
                docs.push(*d);
            }
        }
        if !docs.is_empty() {
            self.db
                .insert(&dkey, Self::json_to(&docs)?)
                .map_err(sled_err)?;
        }
        self.db
            .insert(Self::ent_key(t, e.id), Self::json_to(&e)?)
            .map_err(sled_err)?;
        Ok(e.id)
    }

    async fn upsert_edge(&self, t: TenantId, mut e: Edge) -> Result<()> {
        e.tenant = t;
        if self
            .db
            .get(Self::ent_key(t, e.src))
            .map_err(sled_err)?
            .is_none()
            || self
                .db
                .get(Self::ent_key(t, e.dst))
                .map_err(sled_err)?
                .is_none()
        {
            return Err(SageError::Invalid(format!(
                "edge endpoints missing: {} -> {}",
                e.src, e.dst
            )));
        }
        let mut meta = self.read_meta(t)?;
        let seq = meta.next_edge_seq;
        meta.next_edge_seq += 1;
        self.db
            .insert(Self::out_key(t, e.src, seq), Self::json_to(&e)?)
            .map_err(sled_err)?;
        self.write_meta(t, &meta)?;
        Ok(())
    }

    async fn get_entity(&self, t: TenantId, id: EntityId) -> Result<Option<Entity>> {
        match self.db.get(Self::ent_key(t, id)).map_err(sled_err)? {
            Some(b) => Ok(Some(Self::json_from(&b)?)),
            None => Ok(None),
        }
    }

    async fn neighbors(
        &self,
        t: TenantId,
        id: EntityId,
        max: usize,
    ) -> Result<Vec<(EntityId, Edge)>> {
        let edges = self.collect_outgoing(t, id)?;
        Ok(edges.into_iter().take(max).map(|e| (e.dst, e)).collect())
    }

    async fn k_hop(&self, t: TenantId, seeds: &[EntityId], hops: u8) -> Result<Subgraph> {
        let mut sg = Subgraph::default();
        let mut frontier: Vec<EntityId> = seeds.to_vec();
        let mut seen: ahash::AHashSet<EntityId> = frontier.iter().copied().collect();
        for &id in &frontier {
            if let Some(e) = self.get_entity(t, id).await? {
                sg.entities.push(e);
            }
        }
        for _ in 0..hops {
            let mut next: Vec<EntityId> = Vec::new();
            for &v in &frontier {
                let edges = self.collect_outgoing(t, v)?;
                for e in edges {
                    sg.edges.push(e.clone());
                    if seen.insert(e.dst) {
                        if let Some(ent) = self.get_entity(t, e.dst).await? {
                            sg.entities.push(ent);
                        }
                        next.push(e.dst);
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
        match self.db.get(Self::doc_key(t, id)).map_err(sled_err)? {
            Some(b) => Self::json_from(&b),
            None => Ok(Vec::new()),
        }
    }

    async fn snapshot(&self, t: TenantId) -> Result<SnapshotId> {
        let mut meta = self.read_meta(t)?;
        let id = SnapshotId(meta.next_snap);
        meta.next_snap += 1;

        let mut entities: Vec<Entity> = Vec::new();
        for kv in self.db.scan_prefix(Self::ent_prefix(t)) {
            let (_, v) = kv.map_err(sled_err)?;
            entities.push(Self::json_from(&v)?);
        }
        let mut out_edges: Vec<Edge> = Vec::new();
        for kv in self.db.scan_prefix(Self::out_prefix_tenant(t)) {
            let (_, v) = kv.map_err(sled_err)?;
            out_edges.push(Self::json_from(&v)?);
        }
        let mut doc_index: Vec<(EntityId, Vec<DocId>)> = Vec::new();
        for kv in self.db.scan_prefix(Self::doc_prefix(t)) {
            let (k, v) = kv.map_err(sled_err)?;
            let eid = u64::from_be_bytes(
                k[9..17]
                    .try_into()
                    .map_err(|_| SageError::Graph("doc key shape".into()))?,
            );
            doc_index.push((eid, Self::json_from(&v)?));
        }
        let payload = SnapshotPayload {
            entities,
            out_edges,
            doc_index,
        };
        self.db
            .insert(Self::snap_key(t, id.0), Self::json_to(&payload)?)
            .map_err(sled_err)?;
        self.write_meta(t, &meta)?;
        Ok(id)
    }

    async fn restore(&self, t: TenantId, snap: SnapshotId) -> Result<()> {
        let payload: SnapshotPayload =
            match self.db.get(Self::snap_key(t, snap.0)).map_err(sled_err)? {
                Some(b) => Self::json_from(&b)?,
                None => return Err(SageError::NotFound(format!("snapshot {snap:?}"))),
            };
        // Clear tenant data (except snapshot table).
        let prefixes = [
            Self::ent_prefix(t),
            Self::out_prefix_tenant(t),
            Self::doc_prefix(t),
        ];
        for p in &prefixes {
            let keys: Vec<Vec<u8>> = self
                .db
                .scan_prefix(p)
                .filter_map(|kv| kv.ok().map(|(k, _)| k.to_vec()))
                .collect();
            for k in keys {
                self.db.remove(k).map_err(sled_err)?;
            }
        }
        // Reset edge sequence counter, preserve snap counter.
        let mut meta = self.read_meta(t)?;
        meta.next_edge_seq = payload.out_edges.len() as u64;
        self.write_meta(t, &meta)?;

        for e in &payload.entities {
            self.db
                .insert(Self::ent_key(t, e.id), Self::json_to(e)?)
                .map_err(sled_err)?;
        }
        for (seq, e) in payload.out_edges.iter().enumerate() {
            self.db
                .insert(Self::out_key(t, e.src, seq as u64), Self::json_to(e)?)
                .map_err(sled_err)?;
        }
        for (eid, docs) in &payload.doc_index {
            if !docs.is_empty() {
                self.db
                    .insert(Self::doc_key(t, *eid), Self::json_to(docs)?)
                    .map_err(sled_err)?;
            }
        }
        Ok(())
    }

    async fn entity_count(&self, t: TenantId) -> Result<usize> {
        Ok(self.db.scan_prefix(Self::ent_prefix(t)).count())
    }
}

#[async_trait]
impl EntityScan for SledGraphStore {
    async fn all_entities(&self, t: TenantId) -> Result<Vec<Entity>> {
        let mut out = Vec::new();
        for kv in self.db.scan_prefix(Self::ent_prefix(t)) {
            let (_, v) = kv.map_err(sled_err)?;
            out.push(Self::json_from(&v)?);
        }
        Ok(out)
    }

    async fn find_by_name(&self, t: TenantId, name: &str) -> Result<Vec<EntityId>> {
        let needle = name.to_lowercase();
        let mut out = Vec::new();
        for kv in self.db.scan_prefix(Self::ent_prefix(t)) {
            let (_, v) = kv.map_err(sled_err)?;
            let e: Entity = Self::json_from(&v)?;
            if e.name.to_lowercase() == needle
                || e.aliases.iter().any(|a| a.to_lowercase() == needle)
            {
                out.push(e.id);
            }
        }
        Ok(out)
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
    async fn upsert_and_get() {
        let g = SledGraphStore::temporary().unwrap();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "A"))
            .await
            .unwrap();
        let got = g.get_entity(TenantId::DEFAULT, 1).await.unwrap();
        assert_eq!(got.unwrap().name, "A");
    }

    #[tokio::test]
    async fn edge_requires_endpoints() {
        let g = SledGraphStore::temporary().unwrap();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "A"))
            .await
            .unwrap();
        let bad = Edge::new(1, 99, "rel", 0);
        let res = g.upsert_edge(TenantId::DEFAULT, bad).await;
        assert!(matches!(res, Err(SageError::Invalid(_))));
    }

    #[tokio::test]
    async fn k_hop_walks_two_hops() {
        let g = SledGraphStore::temporary().unwrap();
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
        let sg = g.k_hop(TenantId::DEFAULT, &[1], 2).await.unwrap();
        let ids: ahash::AHashSet<_> = sg.entities.iter().map(|e| e.id).collect();
        assert!(ids.contains(&3));
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let g = SledGraphStore::temporary().unwrap();
        g.upsert_entity(TenantId(1), ent(1, "A")).await.unwrap();
        g.upsert_entity(TenantId(2), ent(1, "Z")).await.unwrap();
        assert_eq!(
            g.get_entity(TenantId(1), 1).await.unwrap().unwrap().name,
            "A"
        );
        assert_eq!(
            g.get_entity(TenantId(2), 1).await.unwrap().unwrap().name,
            "Z"
        );
    }

    #[tokio::test]
    async fn snapshot_restore_roundtrip() {
        let g = SledGraphStore::temporary().unwrap();
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

    #[tokio::test]
    async fn all_entities_and_find_by_name() {
        let g = SledGraphStore::temporary().unwrap();
        g.upsert_entity(TenantId::DEFAULT, ent(1, "Alice"))
            .await
            .unwrap();
        g.upsert_entity(TenantId::DEFAULT, ent(2, "Bob"))
            .await
            .unwrap();
        assert_eq!(g.all_entities(TenantId::DEFAULT).await.unwrap().len(), 2);
        let ids = g.find_by_name(TenantId::DEFAULT, "alice").await.unwrap();
        assert_eq!(ids, vec![1]);
    }

    #[tokio::test]
    async fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.sled");
        {
            let g = SledGraphStore::open(&path).unwrap();
            g.upsert_entity(TenantId::DEFAULT, ent(42, "PersistMe"))
                .await
                .unwrap();
        }
        {
            let g = SledGraphStore::open(&path).unwrap();
            let e = g.get_entity(TenantId::DEFAULT, 42).await.unwrap().unwrap();
            assert_eq!(e.name, "PersistMe");
        }
    }
}
