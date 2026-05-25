use serde::{Deserialize, Serialize};

pub type EntityId = u64;
pub type DocId = u64;
pub type Score = f32;

/// Multi-tenant isolation key. `DEFAULT` reserved for single-tenant deployments.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct TenantId(pub u64);

impl TenantId {
    pub const DEFAULT: TenantId = TenantId(0);
}

impl Default for TenantId {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{:016x}", self.0)
    }
}
