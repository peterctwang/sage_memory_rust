//! `SledGraphStore` integration: trait-level parity with `MemGraphStore`,
//! plus reload-from-disk survives writer→graph state.

#![cfg(feature = "sled")]

use sage_core::{GraphStore, TenantId};
use sage_graph::SledGraphStore;
use tests_support::{edge, entity};

#[tokio::test]
async fn fixture_helpers_walk_two_hops_on_disk() {
    let g = SledGraphStore::temporary().unwrap();
    let t = TenantId::DEFAULT;
    g.upsert_entity(t, entity(1, "A")).await.unwrap();
    g.upsert_entity(t, entity(2, "B")).await.unwrap();
    g.upsert_entity(t, entity(3, "C")).await.unwrap();
    g.upsert_edge(t, edge(1, 2, "next")).await.unwrap();
    g.upsert_edge(t, edge(2, 3, "next")).await.unwrap();

    let sg = g.k_hop(t, &[1], 2).await.unwrap();
    let ids: Vec<u64> = sg.entities.iter().map(|e| e.id).collect();
    assert!(ids.contains(&3), "got {ids:?}");
}

#[tokio::test]
async fn reload_preserves_graph() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.sled");

    {
        let g = SledGraphStore::open(&path).unwrap();
        let t = TenantId::DEFAULT;
        g.upsert_entity(t, entity(1, "A")).await.unwrap();
        g.upsert_entity(t, entity(2, "B")).await.unwrap();
        g.upsert_edge(t, edge(1, 2, "knows")).await.unwrap();
    }
    {
        let g = SledGraphStore::open(&path).unwrap();
        let t = TenantId::DEFAULT;
        assert_eq!(g.entity_count(t).await.unwrap(), 2);
        let nbrs = g.neighbors(t, 1, 10).await.unwrap();
        assert_eq!(nbrs.len(), 1);
        assert_eq!(nbrs[0].0, 2);
    }
}
