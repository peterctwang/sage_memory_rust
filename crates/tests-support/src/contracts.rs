//! Reusable contract suites for `GraphStore` + `EntityScan` implementations.
//!
//! Both `MemGraphStore` and `SledGraphStore` MUST satisfy every contract here.
//! Add a new contract once; both integration tests pick it up automatically.

use sage_core::{Edge, EntityScan, GraphStore, Result, SageError, TenantId};

use crate::{edge, entity};

/// `upsert_edge` must reject edges whose endpoints don't exist.
pub async fn edge_requires_endpoints<G: GraphStore>(g: &G) -> Result<()> {
    let t = TenantId::DEFAULT;
    g.upsert_entity(t, entity(1, "A")).await?;
    let bad = Edge::new(1, 99, "rel", 0);
    let r = g.upsert_edge(t, bad).await;
    assert!(
        matches!(r, Err(SageError::Invalid(_))),
        "expected Invalid, got {r:?}"
    );
    Ok(())
}

/// `neighbors` returns outgoing edges with the requested cap honored.
pub async fn neighbors_outgoing_with_cap<G: GraphStore>(g: &G) -> Result<()> {
    let t = TenantId::DEFAULT;
    for i in 1..=4 {
        g.upsert_entity(t, entity(i, "n")).await?;
    }
    g.upsert_edge(t, edge(1, 2, "r")).await?;
    g.upsert_edge(t, edge(1, 3, "r")).await?;
    g.upsert_edge(t, edge(1, 4, "r")).await?;
    let all = g.neighbors(t, 1, 10).await?;
    assert_eq!(all.len(), 3);
    let capped = g.neighbors(t, 1, 2).await?;
    assert_eq!(capped.len(), 2);
    Ok(())
}

/// `k_hop` traverses up to `hops` edges away from each seed.
pub async fn k_hop_walks_n_hops<G: GraphStore>(g: &G) -> Result<()> {
    let t = TenantId::DEFAULT;
    for i in 1..=4 {
        g.upsert_entity(t, entity(i, "n")).await?;
    }
    g.upsert_edge(t, edge(1, 2, "r")).await?;
    g.upsert_edge(t, edge(2, 3, "r")).await?;
    g.upsert_edge(t, edge(3, 4, "r")).await?;
    let h1 = g.k_hop(t, &[1], 1).await?;
    let ids1: ahash::AHashSet<_> = h1.entities.iter().map(|e| e.id).collect();
    assert!(ids1.contains(&2));
    assert!(!ids1.contains(&4));
    let h2 = g.k_hop(t, &[1], 2).await?;
    let ids2: ahash::AHashSet<_> = h2.entities.iter().map(|e| e.id).collect();
    assert!(ids2.contains(&3));
    Ok(())
}

/// Tenants must be completely isolated.
pub async fn tenants_isolated<G: GraphStore>(g: &G) -> Result<()> {
    g.upsert_entity(TenantId(1), entity(1, "A")).await?;
    g.upsert_entity(TenantId(2), entity(1, "Z")).await?;
    let a = g.get_entity(TenantId(1), 1).await?.unwrap();
    let z = g.get_entity(TenantId(2), 1).await?.unwrap();
    assert_eq!(a.name, "A");
    assert_eq!(z.name, "Z");
    Ok(())
}

/// `snapshot` then `restore` returns the graph to its prior state.
pub async fn snapshot_restore_roundtrip<G: GraphStore>(g: &G) -> Result<()> {
    let t = TenantId::DEFAULT;
    g.upsert_entity(t, entity(1, "A")).await?;
    let snap = g.snapshot(t).await?;
    g.upsert_entity(t, entity(2, "B")).await?;
    assert_eq!(g.entity_count(t).await?, 2);
    g.restore(t, snap).await?;
    assert_eq!(g.entity_count(t).await?, 1);
    Ok(())
}

/// `EntityScan::find_by_name` is case-insensitive and respects tenant scope.
pub async fn find_by_name_case_insensitive<G: GraphStore + EntityScan>(g: &G) -> Result<()> {
    let t = TenantId::DEFAULT;
    g.upsert_entity(t, entity(7, "Alice")).await?;
    g.upsert_entity(t, entity(8, "Bob")).await?;
    let ids = g.find_by_name(t, "alice").await?;
    assert_eq!(ids, vec![7]);
    let other = g.find_by_name(TenantId(99), "alice").await?;
    assert!(other.is_empty());
    Ok(())
}
