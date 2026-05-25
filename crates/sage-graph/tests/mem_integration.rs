//! Integration tests for `MemGraphStore` — exercises trait via `tests-support` fixtures.

use sage_core::{GraphStore, TenantId};
use sage_graph::MemGraphStore;
use tests_support::{edge, entity};

#[tokio::test]
async fn fixture_helpers_build_a_walkable_graph() {
    let g = MemGraphStore::new();
    let t = TenantId::DEFAULT;
    g.upsert_entity(t, entity(1, "A")).await.unwrap();
    g.upsert_entity(t, entity(2, "B")).await.unwrap();
    g.upsert_entity(t, entity(3, "C")).await.unwrap();
    g.upsert_edge(t, edge(1, 2, "next")).await.unwrap();
    g.upsert_edge(t, edge(2, 3, "next")).await.unwrap();

    let sg = g.k_hop(t, &[1], 2).await.unwrap();
    let ids: Vec<u64> = sg.entities.iter().map(|e| e.id).collect();
    assert!(ids.contains(&3), "2-hop walk should reach C, got {ids:?}");
}

#[tokio::test]
async fn separate_tenants_do_not_leak() {
    let g = MemGraphStore::new();
    g.upsert_entity(TenantId(1), entity(1, "A")).await.unwrap();
    g.upsert_entity(TenantId(2), entity(1, "Z")).await.unwrap();
    let a = g.get_entity(TenantId(1), 1).await.unwrap().unwrap();
    let z = g.get_entity(TenantId(2), 1).await.unwrap().unwrap();
    assert_eq!(a.name, "A");
    assert_eq!(z.name, "Z");
}
