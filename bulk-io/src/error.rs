use std::path::Path;

use rholang::rust::interpreter::io::bulk::BulkTreeError;

#[derive(Debug, thiserror::Error)]
pub enum BulkError {
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
    #[error("manifest: {0}")]
    Manifest(String),
    #[error("source: {0}")]
    Source(String),
    #[error("record: {0}")]
    Record(String),
    #[error("tree: {0}")]
    Tree(#[from] BulkTreeError),
    #[error("export: {0}")]
    Export(String),
    #[error("verification failed: {0}")]
    Verify(String),
}

impl BulkError {
    pub fn io(path: &Path, e: std::io::Error) -> Self {
        BulkError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        }
    }
}
