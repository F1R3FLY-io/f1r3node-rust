use std::time::Duration;

use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use chroma::embed::EmbeddingFunction;
use hf_hub::api::sync::{Api, ApiError, ApiRepo};
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

        let config_path = fetch_with_retry(&repo, "config.json")?;
        let tokenizer_path = fetch_with_retry(&repo, "tokenizer.json")?;
        let sentence_bert_config_path = fetch_with_retry(&repo, "sentence_bert_config.json")?;
        let weights_path = fetch_with_retry(&repo, "model.safetensors")?;

        let config_bytes = std::fs::read(&config_path).map_err(SBERTEmbeddingsError::Io)?;
        let config: Config = serde_json::from_slice(&config_bytes)
            .map_err(|e| SBERTEmbeddingsError::Config(e.to_string()))?;

        // sentence-transformers pipelines publish their own token cap in
        // sentence_bert_config.json's max_seq_length. For all-MiniLM-L6-v2
        // the sentence cap is 256 even though BERT's position table has
        // 512 slots, and rust-bert honored that cap. Truncate at
        // max_seq_length so existing ChromaDB collections keep their
        // ranking under this swap.
        let sentence_bert_config_bytes =
            std::fs::read(&sentence_bert_config_path).map_err(SBERTEmbeddingsError::Io)?;
        let sentence_bert_config: SentenceBertConfig =
            serde_json::from_slice(&sentence_bert_config_bytes)
                .map_err(|e| SBERTEmbeddingsError::Config(e.to_string()))?;
        if sentence_bert_config.max_seq_length > config.max_position_embeddings {
            return Err(SBERTEmbeddingsError::Config(format!(
                "max_seq_length={} exceeds max_position_embeddings={}",
                sentence_bert_config.max_seq_length, config.max_position_embeddings,
            )));
        }

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| SBERTEmbeddingsError::Tokenizer(e.to_string()))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..PaddingParams::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: sentence_bert_config.max_seq_length,
                strategy: TruncationStrategy::LongestFirst,
                ..TruncationParams::default()
            }))
            .map_err(|e| SBERTEmbeddingsError::Tokenizer(e.to_string()))?;

        let device = Device::Cpu;
        // Load weights into an owned buffer rather than mmap'ing the HF
        // cache file. Two safetensors are published side-by-side in the
        // shared cache; if another process ever truncates or replaces
        // model.safetensors while the mapping lives, the behavior under
        // mmap is undefined. Buffering copies the ~90MB once at startup.
        let weights_bytes = std::fs::read(&weights_path).map_err(SBERTEmbeddingsError::Io)?;
        let vb = VarBuilder::from_buffered_safetensors(weights_bytes, DTYPE, &device)
            .map_err(|e| SBERTEmbeddingsError::Model(e.to_string()))?;
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

/// Fetch a repo file, retrying on download-lock contention.
///
/// `ApiRepo::get` acquires an exclusive lock per file while it downloads.
/// On first start, two processes can race; the loser gets
/// `ApiError::LockAcquisition` after ~5s and bails. That would leave
/// ChromaDB in NoOp mode until the next restart. Re-run `get` a few
/// times: after each retry it re-checks the cache first, so the loser
/// picks up whatever the winner finished writing without re-downloading.
fn fetch_with_retry(
    repo: &ApiRepo,
    filename: &str,
) -> Result<std::path::PathBuf, SBERTEmbeddingsError> {
    const MAX_ATTEMPTS: u32 = 4;
    let mut delay = Duration::from_secs(2);
    let mut last_lock_path: Option<std::path::PathBuf> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match repo.get(filename) {
            Ok(path) => return Ok(path),
            Err(ApiError::LockAcquisition(path)) if attempt < MAX_ATTEMPTS => {
                last_lock_path = Some(path);
                std::thread::sleep(delay);
                delay *= 2;
            }
            Err(e) => {
                return Err(SBERTEmbeddingsError::Hub(format!(
                    "Failed to fetch {filename} from {MODEL_REPO}: {e}"
                )))
            }
        }
    }
    Err(SBERTEmbeddingsError::Hub(format!(
        "Failed to fetch {filename} from {MODEL_REPO}: lock {} held after {} attempts",
        last_lock_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        MAX_ATTEMPTS,
    )))
}

#[derive(serde::Deserialize)]
struct SentenceBertConfig {
    max_seq_length: usize,
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

    const EMB_DIM: usize = 384;

    // Known-good prefix of the embedding for SHORT_DOC captured from this
    // implementation. Future runs of the same model and inputs must land
    // within TOLERANCE of each element; a wider drift indicates a
    // regression in tokenization, pooling, or normalization.
    const SHORT_DOC: &str = "The quick brown fox jumps over the lazy dog.";
    const SHORT_HEAD: [f32; 8] = GOLDEN_HEAD;
    const TOLERANCE: f32 = 1e-4;

    // Captured from a clean-cache run on arm64-linux (via rust:1-bookworm).
    // Regenerate with
    //   SBERT_PRINT_GOLDEN=1 cargo test -p rholang --features chromadb --lib embedding
    // if the model id or pipeline ever changes.
    const GOLDEN_HEAD: [f32; 8] = [
        0.043933567,
        0.05893438,
        0.048178382,
        0.07754817,
        0.02674441,
        -0.03762952,
        -0.0026050932,
        -0.05994302,
    ];

    fn blocking_embedder() -> SBERTEmbeddings {
        // Loading downloads ~90MB on a cold HF cache. SBERTEmbeddings::new
        // builds a blocking ureq-backed HF client, so run it off the tokio
        // runtime via spawn_blocking.
        tokio::task::block_in_place(|| SBERTEmbeddings::new().expect("model load"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn embeds_short_doc_matches_golden_head() {
        let embedder = blocking_embedder();
        let embs = embedder.embed_strs(&[SHORT_DOC]).await.expect("embed");
        assert_eq!(embs.len(), 1);
        assert_eq!(embs[0].len(), EMB_DIM);

        let norm = embs[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < TOLERANCE, "non-unit norm: {norm}");

        if std::env::var_os("SBERT_PRINT_GOLDEN").is_some() {
            eprintln!("GOLDEN_HEAD = {:?}", &embs[0][..SHORT_HEAD.len()]);
        }

        for (i, (&got, &want)) in embs[0].iter().zip(SHORT_HEAD.iter()).enumerate() {
            assert!(
                (got - want).abs() < TOLERANCE,
                "embedding drift at index {i}: got {got}, want {want}",
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn truncates_inputs_longer_than_sentence_limit() {
        // "foo " is one token per repetition; 400 copies clears the 256
        // sentence cap and would overrun the 512 position-embedding table
        // without truncation.
        let long = "foo ".repeat(400);
        let embedder = blocking_embedder();
        let embs = embedder.embed_strs(&[long.as_str()]).await.expect("embed");
        assert_eq!(embs[0].len(), EMB_DIM);
        let norm = embs[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < TOLERANCE, "non-unit norm: {norm}");
    }
}
