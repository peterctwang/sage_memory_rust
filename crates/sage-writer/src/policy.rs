//! Writer policy trait.

use async_trait::async_trait;
use sage_core::{Document, Result};

use crate::action::{WriterAction, WriterState};

/// Sequential decision-making policy: state → action.
#[async_trait]
pub trait WriterPolicy: Send + Sync {
    async fn step(&self, state: &WriterState<'_>, doc: &Document) -> Result<WriterAction>;
}
