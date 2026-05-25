use thiserror::Error;

#[derive(Error, Debug)]
pub enum SageError {
    #[error("graph store: {0}")]
    Graph(String),
    #[error("llm: {0}")]
    Llm(String),
    #[error("reader: {0}")]
    Reader(String),
    #[error("writer: {0}")]
    Writer(String),
    #[error("config: {0}")]
    Config(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("serde: {0}")]
    Serde(String),
}

pub type Result<T> = std::result::Result<T, SageError>;

impl From<serde_json::Error> for SageError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e.to_string())
    }
}
