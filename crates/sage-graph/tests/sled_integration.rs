//! `SledGraphStore` integration: same contract suite as MemGraphStore, plus
//! reload-from-disk to prove on-disk durability.

#![cfg(feature = "sled")]

use sage_core::{GraphStore, TenantId};
use sage_graph::SledGraphStore;
use tests_support::{contracts, edge, entity};

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

// === Shared contract suite (same fns as mem_integration.rs) ===

#[tokio::test]
async fn contract_edge_requires_endpoints() {
    contracts::edge_requires_endpoints(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
#[tokio::test]
async fn contract_neighbors_outgoing_with_cap() {
    contracts::neighbors_outgoing_with_cap(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
#[tokio::test]
async fn contract_k_hop_walks_n_hops() {
    contracts::k_hop_walks_n_hops(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
#[tokio::test]
async fn contract_tenants_isolated() {
    contracts::tenants_isolated(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
#[tokio::test]
async fn contract_snapshot_restore_roundtrip() {
    contracts::snapshot_restore_roundtrip(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
#[tokio::test]
async fn contract_find_by_name_case_insensitive() {
    contracts::find_by_name_case_insensitive(&SledGraphStore::temporary().unwrap())
        .await
        .unwrap();
}
