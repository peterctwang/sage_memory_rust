//! Writer action types — paper §4.1: `aₜ` contains entity-relation triples with source anchor.

use sage_core::{DocId, Document, EntityId, EntityType, Query};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use smol_str::SmolStr;

/// Reference to either an existing graph entity or a new one to materialize.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EntityRef {
    Existing(EntityId),
    New {
        name: SmolStr,
        etype: EntityType,
        desc: Option<String>,
    },
}

/// Pre-sanitization triple emitted by an LLM policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawTriple {
    pub src_name: SmolStr,
    pub relation: SmolStr,
    pub dst_name: SmolStr,
    pub src_type: Option<EntityType>,
    pub dst_type: Option<EntityType>,
}

/// One step of the writer policy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WriterAction {
    pub triples: SmallVec<[(EntityRef, SmolStr, EntityRef); 8]>,
    pub source: DocId,
    pub stop: bool,
}

#[derive(Clone, Debug)]
pub struct WriterState<'a> {
    pub query: Option<&'a Query>,
    pub candidates: &'a [Document],
    pub processed: &'a [DocId],
    pub step: u32,
}
