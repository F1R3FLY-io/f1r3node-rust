use std::sync::Mutex;

use async_trait::async_trait;
use chroma::embed::EmbeddingFunction;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

// Struct to store the model for embedding documents
pub struct SBERTEmbeddings {
    model: Mutex<TextEmbedding>,
}

impl SBERTEmbeddings {
    /// Download the SBERT model and cache it.
    pub fn new() -> Result<Self, SBERTEmbeddingsError> {
        // fastembed's TextEmbedding is not Send+Sync by itself; wrap in a
        // Mutex so the service can be shared across tokio tasks.
        let model = TextEmbedding::try_new(TextInitOptions::new(EmbeddingModel::AllMiniLML6V2))
            .map_err(|err| SBERTEmbeddingsError::ModelError(err.to_string()))?;
        let model = Mutex::new(model);

        Ok(Self { model })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SBERTEmbeddingsError {
    #[error("Could not read model: {0}")]
    ThreadingError(String),
    #[error("Could not encode documents: {0}")]
    ModelError(String),
}

// Helper SBERT embedding function to be used in ChromaDB.
#[async_trait]
impl EmbeddingFunction for SBERTEmbeddings {
    type Embedding = Vec<f32>;
    type Error = SBERTEmbeddingsError;

    async fn embed_strs(&self, docs: &[&str]) -> Result<Vec<Self::Embedding>, Self::Error> {
        let owned: Vec<String> = docs.iter().map(|s| (*s).to_owned()).collect();
        let res = self
            .model
            .lock()
            .map_err(|err| SBERTEmbeddingsError::ThreadingError(err.to_string()))?
            .embed(owned, None)
            .map_err(|err| SBERTEmbeddingsError::ModelError(err.to_string()))?;
        Ok(res)
    }
}
