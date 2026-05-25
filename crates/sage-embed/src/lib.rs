//! SAGE embedder backends.

pub mod deterministic;

#[cfg(feature = "hnsw")]
pub mod hnsw_index;

pub use deterministic::DeterministicEmbedder;

#[cfg(feature = "hnsw")]
pub use hnsw_index::HnswIndex;
