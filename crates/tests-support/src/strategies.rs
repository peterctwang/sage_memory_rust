//! Proptest strategies for SAGE core types.

use proptest::prelude::*;
use sage_core::{Edge, Entity, EntityType};
use smol_str::SmolStr;

pub fn arb_entity() -> impl Strategy<Value = Entity> {
    (1u64..1_000_000, "[a-zA-Z][a-zA-Z0-9 ]{0,30}")
        .prop_map(|(id, name)| Entity::new(id, SmolStr::new(name), EntityType::Concept))
}

pub fn arb_edge(max_id: u64) -> impl Strategy<Value = Edge> {
    (1..max_id, 1..max_id, "[a-z_]{1,12}").prop_map(|(s, d, r)| Edge::new(s, d, SmolStr::new(r), 0))
}
