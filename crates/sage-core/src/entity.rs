use std::sync::Arc;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::ids::{DocId, EntityId, TenantId};

pub const ENTITY_SCHEMA_V: u16 = 1;
pub const EDGE_SCHEMA_V: u16 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Org,
    #[default]
    Concept,
    Event,
    Time,
    Custom(SmolStr),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    #[serde(default = "default_entity_schema")]
    pub schema_version: u16,
    pub id: EntityId,
    #[serde(default)]
    pub tenant: TenantId,
    pub name: SmolStr,
    #[serde(default)]
    pub aliases: SmallVec<[SmolStr; 4]>,
    pub etype: EntityType,
    pub desc: Option<Arc<str>>,
    pub embedding: Option<Arc<[f32]>>,
    #[serde(default)]
    pub source_docs: SmallVec<[DocId; 4]>,
}

fn default_entity_schema() -> u16 {
    ENTITY_SCHEMA_V
}
fn default_edge_schema() -> u16 {
    EDGE_SCHEMA_V
}

impl Entity {
    pub fn new(id: EntityId, name: impl Into<SmolStr>, etype: EntityType) -> Self {
        Self {
            schema_version: ENTITY_SCHEMA_V,
            id,
            tenant: TenantId::DEFAULT,
            name: name.into(),
            aliases: SmallVec::new(),
            etype,
            desc: None,
            embedding: None,
            source_docs: SmallVec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    #[serde(default = "default_edge_schema")]
    pub schema_version: u16,
    #[serde(default)]
    pub tenant: TenantId,
    pub src: EntityId,
    pub dst: EntityId,
    pub relation: SmolStr,
    pub weight: f32,
    pub provenance: DocId,
    pub created_at: u64,
}

impl Edge {
    pub fn new(
        src: EntityId,
        dst: EntityId,
        relation: impl Into<SmolStr>,
        provenance: DocId,
    ) -> Self {
        Self {
            schema_version: EDGE_SCHEMA_V,
            tenant: TenantId::DEFAULT,
            src,
            dst,
            relation: relation.into(),
            weight: 1.0,
            provenance,
            created_at: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_new_defaults() {
        let e = Entity::new(7, "Alice", EntityType::Person);
        assert_eq!(e.id, 7);
        assert_eq!(e.name, "Alice");
        assert_eq!(e.tenant, TenantId::DEFAULT);
        assert_eq!(e.schema_version, ENTITY_SCHEMA_V);
        assert!(e.aliases.is_empty());
    }

    #[test]
    fn edge_new_defaults() {
        let e = Edge::new(1, 2, "knows", 100);
        assert_eq!(e.src, 1);
        assert_eq!(e.dst, 2);
        assert_eq!(e.relation, "knows");
        assert!((e.weight - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn entity_serde_roundtrip() {
        let e = Entity::new(1, "X", EntityType::Person);
        let j = serde_json::to_string(&e).unwrap();
        let back: Entity = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, 1);
        assert_eq!(back.name, "X");
    }

    #[test]
    fn entity_type_custom_serde() {
        let t = EntityType::Custom(SmolStr::new("Drug"));
        let j = serde_json::to_string(&t).unwrap();
        let back: EntityType = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }
}
