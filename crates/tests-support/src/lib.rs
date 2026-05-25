//! Shared fixtures + proptest strategies + reusable contract suites.
//!
//! Must only be referenced via `[dev-dependencies]` or by other test-support crates.

pub mod contracts;
pub mod strategies;

use sage_core::{Document, Edge, Entity, EntityType};

pub fn entity(id: u64, name: &str) -> Entity {
    Entity::new(id, name, EntityType::Concept)
}

pub fn edge(src: u64, dst: u64, rel: &str) -> Edge {
    Edge::new(src, dst, rel, 0)
}

pub fn doc(id: u64, text: &str) -> Document {
    Document::new(id, text)
}
