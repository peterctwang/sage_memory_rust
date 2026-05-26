//! HNSW-backed `VectorIndex` (feature `hnsw`).
//!
//! Maintains a `Vec<EntityId>` so the HNSW's internal `usize` ids can be mapped
//! back to SAGE entity ids on search. Insertions are O(log N) expected; search
//! is `O(ef_search · log N)`.

use parking_lot::RwLock;
use sage_core::{EntityId, Result, SageError, Score, VectorIndex};

use hnsw_rs::prelude::{DistCosine, Hnsw};

const DEFAULT_M: usize = 16;
const DEFAULT_EF_C: usize = 200;
const DEFAULT_MAX_LAYER: usize = 16;
const DEFAULT_EF_S: usize = 64;
const DEFAULT_CAPACITY: usize = 100_000;

pub struct HnswIndex {
    dim: usize,
    ef_search: usize,
    /// Maps HNSW internal usize ids to caller-facing `EntityId`.
    id_map: RwLock<Vec<EntityId>>,
    inner: Hnsw<'static, f32, DistCosine>,
}

impl std::fmt::Debug for HnswIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HnswIndex")
            .field("dim", &self.dim)
            .field("len", &self.id_map.read().len())
            .field("ef_search", &self.ef_search)
            .finish_non_exhaustive()
    }
}

impl HnswIndex {
    /// Build with default tuning (M=16, ef_c=200, ef_s=64, cap=100k).
    pub fn new(dim: usize) -> Self {
        Self::with_capacity(dim, DEFAULT_CAPACITY)
    }

    pub fn with_capacity(dim: usize, max_elements: usize) -> Self {
        let inner = Hnsw::<f32, DistCosine>::new(
            DEFAULT_M,
            max_elements,
            DEFAULT_MAX_LAYER,
            DEFAULT_EF_C,
            DistCosine {},
        );
        Self {
            dim,
            ef_search: DEFAULT_EF_S,
            id_map: RwLock::new(Vec::new()),
            inner,
        }
    }

    pub fn with_ef_search(mut self, ef: usize) -> Self {
        self.ef_search = ef.max(1);
        self
    }
}

impl VectorIndex for HnswIndex {
    fn dim(&self) -> usize {
        self.dim
    }

    fn len(&self) -> usize {
        self.id_map.read().len()
    }

    fn insert(&self, id: EntityId, vec: &[f32]) -> Result<()> {
        if vec.len() != self.dim {
            return Err(SageError::Invalid(format!(
                "HnswIndex: vec.len()={} != dim={}",
                vec.len(),
                self.dim
            )));
        }
        let internal_id = {
            let mut m = self.id_map.write();
            let i = m.len();
            m.push(id);
            i
        };
        self.inner.insert((vec, internal_id));
        Ok(())
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<(EntityId, Score)>> {
        if query.len() != self.dim {
            return Err(SageError::Invalid(format!(
                "HnswIndex: query.len()={} != dim={}",
                query.len(),
                self.dim
            )));
        }
        if k == 0 {
            return Ok(Vec::new());
        }
        let neighbours = self.inner.search(query, k, self.ef_search);
        let map = self.id_map.read();
        let mut out = Vec::with_capacity(neighbours.len());
        for n in neighbours {
            // hnsw_rs returns cosine *distance* in [0, 2]; convert to similarity in [-1, 1].
            let sim = 1.0 - n.distance;
            if let Some(&eid) = map.get(n.d_id) {
                out.push((eid, sim));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: Vec<f32>) -> Vec<f32> {
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        v.into_iter().map(|x| x / n).collect()
    }

    #[test]
    fn dim_and_len_reported_correctly() {
        let idx = HnswIndex::new(4);
        assert_eq!(idx.dim(), 4);
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        idx.insert(1, &unit(vec![1.0, 0.0, 0.0, 0.0])).unwrap();
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn search_finds_nearest_first() {
        let idx = HnswIndex::new(4);
        idx.insert(10, &unit(vec![1.0, 0.0, 0.0, 0.0])).unwrap();
        idx.insert(20, &unit(vec![0.0, 1.0, 0.0, 0.0])).unwrap();
        idx.insert(30, &unit(vec![0.0, 0.0, 1.0, 0.0])).unwrap();

        let q = unit(vec![0.9, 0.1, 0.0, 0.0]);
        let r = idx.search(&q, 3).unwrap();
        assert_eq!(r[0].0, 10, "nearest should be 10, got {r:?}");
        assert!(r[0].1 > r[1].1, "similarity should decrease");
    }

    #[test]
    fn search_handles_zero_k() {
        let idx = HnswIndex::new(4);
        let r = idx.search(&[0.0, 0.0, 0.0, 0.0], 0).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn dim_mismatch_errors_on_insert() {
        let idx = HnswIndex::new(4);
        let r = idx.insert(1, &[1.0, 0.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn dim_mismatch_errors_on_search() {
        let idx = HnswIndex::new(4);
        let r = idx.search(&[1.0], 1);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn searching_empty_index_returns_empty() {
        let idx = HnswIndex::new(4);
        let r = idx.search(&unit(vec![1.0, 0.0, 0.0, 0.0]), 5).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn identical_vector_has_high_similarity() {
        let idx = HnswIndex::new(4);
        let v = unit(vec![1.0, 2.0, 3.0, 4.0]);
        idx.insert(42, &v).unwrap();
        let r = idx.search(&v, 1).unwrap();
        assert_eq!(r[0].0, 42);
        assert!(
            r[0].1 > 0.99,
            "identical search similarity should be ~1, got {}",
            r[0].1
        );
    }
}
