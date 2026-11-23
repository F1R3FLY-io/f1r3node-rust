use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use chroma::embed::EmbeddingFunction;
use hf_hub::api::sync::Api;
use hf_hub::{Repo, RepoType};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy};

const MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";

pub struct SBERTEmbeddings {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl SBERTEmbeddings {
    /// Download the SBERT model weights and tokenizer from the Hugging Face
    /// Hub (cached across runs) and load them into a candle BERT model.
    pub fn new() -> Result<Self, SBERTEmbeddingsError> {
        let api = Api::new().map_err(|e| SBERTEmbeddingsError::Hub(e.to_string()))?;
        let repo = api.repo(Repo::new(MODEL_REPO.to_string(), RepoType::Model));

        let fetch = |filename: &str| -> Result<std::path::PathBuf, SBERTEmbeddingsError> {
            repo.get(filename).map_err(|e| {
                SBERTEmbeddingsError::Hub(format!(
                    "Failed to fetch {filename} from {MODEL_REPO}: {e}"
                ))
            })
        };

        let config_path = fetch("config.json")?;
        let tokenizer_path = fetch("tokenizer.json")?;
        let weights_path = fetch("model.safetensors")?;

        let config_bytes = std::fs::read(&config_path).map_err(SBERTEmbeddingsError::Io)?;
        let config: Config = serde_json::from_slice(&config_bytes)
            .map_err(|e| SBERTEmbeddingsError::Config(e.to_string()))?;

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| SBERTEmbeddingsError::Tokenizer(e.to_string()))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..PaddingParams::default()
        }));
        // All-MiniLM-L6-v2 has 512 absolute position embeddings. Longer
        // inputs must be truncated before the forward pass — the model would
        // otherwise read past the position embedding table.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: config.max_position_embeddings,
                strategy: TruncationStrategy::LongestFirst,
                ..TruncationParams::default()
            }))
            .map_err(|e| SBERTEmbeddingsError::Tokenizer(e.to_string()))?;

        let device = Device::Cpu;
        // Safety: only our process reads these files for the lifetime of the
        // VarBuilder, and candle documents the safety requirement as inherited
        // from `memmap2::MmapOptions`.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&weights_path], DTYPE, &device)
                .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?
        };
        let model =
            BertModel::load(vb, &config).map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    fn encode_batch(&self, docs: &[&str]) -> Result<Vec<Vec<f32>>, SBERTEmbeddingsError> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let encodings = self
            .tokenizer
            .encode_batch(
                docs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
                true,
            )
            .map_err(|e| SBERTEmbeddingsError::Tokenizer(e.to_string()))?;

        let batch = encodings.len();
        let seq_len = encodings.first().map(|e| e.get_ids().len()).unwrap_or(0);

        let (mut ids, mut types, mut masks) = (
            Vec::with_capacity(batch * seq_len),
            Vec::with_capacity(batch * seq_len),
            Vec::with_capacity(batch * seq_len),
        );
        for enc in &encodings {
            ids.extend_from_slice(enc.get_ids());
            types.extend_from_slice(enc.get_type_ids());
            masks.extend_from_slice(enc.get_attention_mask());
        }

        let shape = (batch, seq_len);
        let input_ids = Tensor::from_vec(ids, shape, &self.device)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let token_type_ids = Tensor::from_vec(types, shape, &self.device)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let attention_mask = Tensor::from_vec(masks, shape, &self.device)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;

        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;

        // Mean-pool with the attention mask, then L2-normalize — the
        // sentence-transformers all-MiniLM-L6-v2 pipeline.
        let mask_f = attention_mask
            .to_dtype(DType::F32)
            .and_then(|m| m.unsqueeze(2))
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let summed = hidden
            .broadcast_mul(&mask_f)
            .and_then(|t| t.sum(1))
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let counts = mask_f
            .sum(1)
            .and_then(|t| t.clamp(1e-9f32, f32::INFINITY))
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let mean = summed
            .broadcast_div(&counts)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let norms = mean
            .sqr()
            .and_then(|t| t.sum_keepdim(1))
            .and_then(|t| t.sqrt())
            .and_then(|t| t.clamp(1e-12f32, f32::INFINITY))
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
        let normalized = mean
            .broadcast_div(&norms)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;

        normalized
            .to_vec2::<f32>()
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SBERTEmbeddingsError {
    #[error("Failed to access Hugging Face Hub: {0}")]
    Hub(String),
    #[error("Failed to read model file: {0}")]
    Io(std::io::Error),
    #[error("Failed to parse model config: {0}")]
    Config(String),
    #[error("Failed to load tokenizer: {0}")]
    Tokenizer(String),
    #[error("Failed to run embedding model: {0}")]
    Model(String),
}

#[async_trait]
impl EmbeddingFunction for SBERTEmbeddings {
    type Embedding = Vec<f32>;
    type Error = SBERTEmbeddingsError;

    async fn embed_strs(&self, docs: &[&str]) -> Result<Vec<Self::Embedding>, Self::Error> {
        self.encode_batch(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Downloads ~90MB on first run; cached across runs in ~/.cache/huggingface.
    // Ignored by default so offline / sandboxed runs don't hang.
    #[tokio::test]
    #[ignore]
    async fn embeds_batch_with_padding_and_normalization() {
        let embedder = tokio::task::spawn_blocking(SBERTEmbeddings::new)
            .await
            .unwrap()
            .expect("model load");

        let docs = [
            "A cat sits on the mat.",
            "The quick brown fox jumps over the lazy dog, and then keeps going.",
        ];
        let embs = embedder.embed_strs(&docs).await.expect("embed");

        assert_eq!(embs.len(), 2);
        // all-MiniLM-L6-v2 produces 384-dim embeddings.
        assert_eq!(embs[0].len(), 384);
        assert_eq!(embs[1].len(), 384);
        for e in &embs {
            let norm = e.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-4,
                "expected unit L2 norm, got {norm}"
            );
        }
    }
}
