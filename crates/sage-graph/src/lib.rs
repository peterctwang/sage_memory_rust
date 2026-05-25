//! Graph storage implementations.

pub mod mem;

#[cfg(feature = "sled")]
pub mod sled_store;

pub use mem::MemGraphStore;

#[cfg(feature = "sled")]
pub use sled_store::SledGraphStore;
