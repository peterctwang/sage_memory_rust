//! Vector index trait — concrete impls live in `sage-embed`.

use crate::error::Result;
use crate::ids::{EntityId, Score};

/// Approximate nearest-neighbor index over entity embeddings.
///
/// Both `insert` and `search` take `&self` — impls handle thread safety
/// internally (RwLock / atomic / lock-free). This allows storing an index
/// behind `Arc<dyn VectorIndex>` shared across reader and writer paths.
///
/// `search` returns the `k` closest IDs by cosine similarity (higher = closer).
pub trait VectorIndex: Send + Sync {
    fn dim(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn insert(&self, id: EntityId, vec: &[f32]) -> Result<()>;
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(EntityId, Score)>>;
}
