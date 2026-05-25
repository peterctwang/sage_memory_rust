//! Land a `WriterAction` onto a `GraphStore`.
//!
//! Names are resolved to `EntityId` via a per-call hash; existing entities are
//! reused. Edges link the source document via `provenance`.

use ahash::AHashMap;
use sage_core::{Edge, Entity, EntityId, GraphStore, Result, TenantId};
use smol_str::SmolStr;

use crate::action::{EntityRef, WriterAction};

#[derive(Debug, Default)]
pub struct ApplyReport {
    pub entities_added: usize,
    pub edges_added: usize,
    pub triples_skipped: usize,
}

/// Stable id for a freshly-named entity within this tenant.
/// Uses ahash for speed; collisions across tenants are isolated by store impl.
fn name_to_id(name: &str) -> EntityId {
    let bh = ahash::RandomState::with_seeds(0xA5A5, 0x5A5A, 0xC3C3, 0x3C3C);
    let v = std::hash::BuildHasher::hash_one(&bh, name.to_lowercase());
    if v == 0 {
        1
    } else {
        v
    }
}

pub async fn apply_action(
    store: &dyn GraphStore,
    tenant: TenantId,
    action: &WriterAction,
) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    let mut local: AHashMap<SmolStr, EntityId> = AHashMap::new();

    for (src, rel, dst) in &action.triples {
        let src_id = resolve(store, tenant, src, &mut local, action.source, &mut report).await?;
        let dst_id = resolve(store, tenant, dst, &mut local, action.source, &mut report).await?;
        let mut edge = Edge::new(src_id, dst_id, rel.clone(), action.source);
        edge.tenant = tenant;
        match store.upsert_edge(tenant, edge).await {
            Ok(()) => report.edges_added += 1,
            Err(_) => report.triples_skipped += 1,
        }
    }
    Ok(report)
}

async fn resolve(
    store: &dyn GraphStore,
    tenant: TenantId,
    r: &EntityRef,
    local: &mut AHashMap<SmolStr, EntityId>,
    provenance: sage_core::DocId,
    report: &mut ApplyReport,
) -> Result<EntityId> {
    match r {
        EntityRef::Existing(id) => Ok(*id),
        EntityRef::New { name, etype, desc } => {
            if let Some(&id) = local.get(name) {
                return Ok(id);
            }
            let id = name_to_id(name);
            local.insert(name.clone(), id);
            if store.get_entity(tenant, id).await?.is_none() {
                let mut e = Entity::new(id, name.clone(), etype.clone());
                e.tenant = tenant;
                if let Some(d) = desc {
                    e.desc = Some(std::sync::Arc::<str>::from(d.as_str()));
                }
                e.source_docs.push(provenance);
                store.upsert_entity(tenant, e).await?;
                report.entities_added += 1;
            }
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_to_id_is_deterministic_and_case_insensitive() {
        let a = name_to_id("Alice");
        let b = name_to_id("alice");
        assert_eq!(a, b);
        assert_ne!(a, name_to_id("Bob"));
    }

    #[test]
    fn name_to_id_never_zero() {
        for n in ["a", "b", "", "long name here"] {
            assert_ne!(name_to_id(n), 0, "id for {n:?}");
        }
    }

    #[test]
    fn apply_report_default_is_zero() {
        let r = ApplyReport::default();
        assert_eq!(r.entities_added, 0);
        assert_eq!(r.edges_added, 0);
        assert_eq!(r.triples_skipped, 0);
    }
}
