use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ids::DocId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub text: Arc<str>,
    pub embedding: Option<Arc<[f32]>>,
    #[serde(default)]
    pub meta: serde_json::Value,
}

impl Document {
    pub fn new(id: DocId, text: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            text: text.into(),
            embedding: None,
            meta: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_new() {
        let d = Document::new(42, "hello world");
        assert_eq!(d.id, 42);
        assert_eq!(&*d.text, "hello world");
        assert!(d.embedding.is_none());
    }
}
