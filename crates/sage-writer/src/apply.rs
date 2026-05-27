//! Land a `WriterAction` onto a `GraphStore`.
//!
//! Names are resolved to `EntityId` via a per-call hash; existing entities are
//! reused. Edges link the source document via `provenance`.

use ahash::AHashMap;
use sage_core::{Edge, Embedder, Entity, EntityId, GraphStore, Result, TenantId};
use smol_str::SmolStr;

use crate::action::{EntityRef, WriterAction};

#[derive(Debug, Default)]
pub struct ApplyReport {
    pub entities_added: usize,
    pub edges_added: usize,
    pub triples_skipped: usize,
    pub added_entity_ids: Vec<EntityId>,
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

/// Land an action onto the store. If `embedder` is provided, freshly-created
/// entities have their `embedding` populated with `embedder(name)`.
pub async fn apply_action(
    store: &dyn GraphStore,
    tenant: TenantId,
    action: &WriterAction,
) -> Result<ApplyReport> {
    apply_action_embedded(store, None, tenant, action).await
}

pub async fn apply_action_embedded(
    store: &dyn GraphStore,
    embedder: Option<&dyn Embedder>,
    tenant: TenantId,
    action: &WriterAction,
) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    let mut local: AHashMap<SmolStr, EntityId> = AHashMap::new();

    for (src, rel, dst) in &action.triples {
        let src_id = resolve(
            store,
            embedder,
            tenant,
            src,
            &mut local,
            action.source,
            &mut report,
        )
        .await?;
        let dst_id = resolve(
            store,
            embedder,
            tenant,
            dst,
            &mut local,
            action.source,
            &mut report,
        )
        .await?;
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
    embedder: Option<&dyn Embedder>,
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
            match store.get_entity(tenant, id).await? {
                None => {
                    let mut e = Entity::new(id, name.clone(), etype.clone());
                    e.tenant = tenant;
                    if let Some(d) = desc {
                        e.desc = Some(std::sync::Arc::<str>::from(d.as_str()));
                    }
                    e.source_docs.push(provenance);
                    if let Some(emb) = embedder {
                        let text = match &e.desc {
                            Some(d) => format!("{name} {d}"),
                            None => name.to_string(),
                        };
                        let vecs = emb.embed(&[text.as_str()]).await?;
                        if let Some(v) = vecs.into_iter().next() {
                            e.embedding = Some(v);
                        }
                    }
                    store.upsert_entity(tenant, e).await?;
                    report.entities_added += 1;
                    report.added_entity_ids.push(id);
                }
                // BUG FIX (2026-05-27 multi-hop diagnosis): when an entity
                // already exists, the prior code returned its id without
                // recording the new doc as a provenance — so "Microsoft"
                // ended up linked only to whichever doc mentioned it FIRST,
                // making every other Microsoft-mentioning doc unreachable
                // via Microsoft-anchored queries. We now append the new
                // provenance and persist.
                Some(mut e) => {
                    if !e.source_docs.contains(&provenance) {
                        e.source_docs.push(provenance);
                        store.upsert_entity(tenant, e).await?;
                    }
                }
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

    /// REGRESSION (2026-05-27): the writer extracted "Microsoft" from
    /// both doc 1015 (Bill Gates) and doc 1027 (Satya Nadella), but the
    /// shared "Microsoft" entity only listed doc 1015 in `source_docs`
    /// because `apply_action` skipped provenance updates whenever the
    /// entity already existed. As a result no Microsoft-anchored query
    /// could ever reach doc 1027 — tier-5 multi-hop "Two CEOs of
    /// Microsoft" was forced to find at most one of the two GT docs.
    /// This test pins the dedup-with-merge behavior.
    #[tokio::test]
    async fn shared_entity_accumulates_source_docs_across_docs() {
        use crate::action::{EntityRef, WriterAction};
        use sage_core::{EntityType, GraphStore, TenantId};
        use sage_graph::MemGraphStore;
        use smallvec::smallvec;

        let store = MemGraphStore::new();
        let t = TenantId::DEFAULT;

        let mk_ref = |n: &str| EntityRef::New {
            name: SmolStr::new(n),
            etype: EntityType::Org,
            desc: None,
        };

        // doc 1: "Bill Gates founded Microsoft"
        apply_action(
            &store,
            t,
            &WriterAction {
                triples: smallvec![(
                    mk_ref("Bill Gates"),
                    SmolStr::new("founded"),
                    mk_ref("Microsoft")
                )],
                source: 1015,
                stop: true,
            },
        )
        .await
        .unwrap();

        // doc 2: "Satya Nadella ceo_of Microsoft"
        apply_action(
            &store,
            t,
            &WriterAction {
                triples: smallvec![(
                    mk_ref("Satya Nadella"),
                    SmolStr::new("works_at"),
                    mk_ref("Microsoft")
                )],
                source: 1027,
                stop: true,
            },
        )
        .await
        .unwrap();

        let ms_id = name_to_id("Microsoft");
        let docs = store.docs_of_entity(t, ms_id).await.unwrap();
        assert!(
            docs.contains(&1015) && docs.contains(&1027),
            "Microsoft entity must link to both source docs, got {docs:?}"
        );
    }
}
