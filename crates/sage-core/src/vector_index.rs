//! Vector index trait — concrete impls live in `sage-embed`.

use crate::error::Result;
use crate::ids::{EntityId, Score};

/// Approximate nearest-neighbor index over entity embeddings.
///
/// `insert` is incremental; `search` returns the `k` closest IDs by cosine
/// similarity (higher = closer). `len` reports the indexed cardinality.
pub trait VectorIndex: Send + Sync {
    fn dim(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn insert(&mut self, id: EntityId, vec: &[f32]) -> Result<()>;
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(EntityId, Score)>>;
}
