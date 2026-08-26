use std::sync::Mutex;

use async_trait::async_trait;
use chroma::embed::EmbeddingFunction;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

// Struct to store the model for embedding documents
pub struct SBERTEmbeddings {
    model: Mutex<TextEmbedding>,
}

impl SBERTEmbeddings {
    pub fn new() -> Result<Self, SBERTEmbeddingsError> {
        let options =
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false);
        let model = TextEmbedding::try_new(options).map_err(SBERTEmbeddingsError::ModelError)?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SBERTEmbeddingsError {
    #[error("Could not read model: {0}")]
    ThreadingError(String),
    #[error("Could not encode documents: {0}")]
    ModelError(fastembed::Error),
}

// Helper SBERT embedding function to be used in ChromaDB.
#[async_trait]
impl EmbeddingFunction for SBERTEmbeddings {
    type Embedding = Vec<f32>;
    type Error = SBERTEmbeddingsError;

    async fn embed_strs(&self, docs: &[&str]) -> Result<Vec<Self::Embedding>, Self::Error> {
        let res = self
            .model
            .lock()
            .map_err(|err| SBERTEmbeddingsError::ThreadingError(err.to_string()))?
            .embed(docs, None)
            .map_err(SBERTEmbeddingsError::ModelError)?;
        Ok(res)
    }
}
