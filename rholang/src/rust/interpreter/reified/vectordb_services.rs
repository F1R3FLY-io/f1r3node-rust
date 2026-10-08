//! The backends of the recorded VectorDB processes (P6 step 7, design v3 4.9; decisions 57 and
//! 58).
//!
//! `rho:vectordb:embed` and `rho:vectordb:ann` ask two node-local services:
//!
//! * the [`Embedder`] turns texts into vectors (the SBERT model `all-MiniLM-L6-v2` of 384
//!   dimensions, with the `chromadb` feature);
//! * the [`MirrorBackend`] answers advisory nearest-neighbour queries from the ChromaDB mirror of
//!   a VectorDB space (with the `chromadb` feature).
//!
//! A service never reaches consensus state. In play, a process records its whole reply on the
//! produce event of the call (`non_deterministic_ops`), and in replay it produces the recorded
//! reply without a service call. So two nodes with different services, or with none, compute the
//! same state.
//!
//! The services of a node live in one registry: [`install`] sets them, and [`current`] reads them
//! when a process runs. Before an install, [`current`] gives the unavailable services, so `embed`
//! replies `EmbeddingUnavailable` and `ann` replies `MirrorUnavailable`. [`node_services`] builds
//! the services of the build: with the `chromadb` feature, the SBERT embedder and the ChromaDB
//! mirror of the environment, and otherwise the unavailable services. The node installs them at
//! start (P6 step 7c). Tests install a mock embedder and a fake mirror.
//!
//! The names of the mirror are pure functions of consensus values:
//!
//! * the collection of a space is `rrs-` and the lowercase hex of the first 16 bytes of its space
//!   id ([`collection_name`]);
//! * the channel digest of a collection key is the lowercase hex of `Blake2b256` of its canonical
//!   inner channel bytes ([`channel_digest`]), the bytes that name the data channel `D(sid, key)`.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Par};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::rust::interpreter::spaces::namespace::{CollectionKey, SID_LEN};
use crate::rust::interpreter::spaces::record::expr_par;

/// The name of the embedding model that the replies of `embed` name.
pub const MODEL: &str = "all-MiniLM-L6-v2";

/// The dimensions of the vectors of [`MODEL`].
pub const MODEL_DIMENSIONS: usize = 384;

/// The node-local quota of one call of `embed`: at most this many texts. The CPU time of the model
/// is not priced, so the node refuses a larger call (design v3 4.9, B.2).
pub const MAX_TEXTS: usize = 64;

/// The node-local quota of one call of `embed`: at most this many bytes of text.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

/// The bytes of the space id in the name of its mirror collection.
const COLLECTION_ID_BYTES: usize = 16;

/// A failure that `embed` or `ann` replies as `[false, code, detail]`. The reply is recorded, so
/// replay produces it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceReplyCode {
    /// An argument of another shape than the call takes.
    BadArgument,
    /// The node has no embedding model, or the model fails.
    EmbeddingUnavailable,
    /// The call exceeds the node-local quota of `embed`.
    EmbeddingRateLimited,
    /// The model gives a value that is NaN or an infinity.
    NonFiniteEmbedding,
    /// The space is not a native VectorDB space.
    NotAVectorDbSpace,
    /// The VectorDB space has no mirror.
    MirrorDisabled,
    /// The node has no mirror, or the mirror does not answer.
    MirrorUnavailable,
    /// The query vector does not have the dimensions of the space.
    DimensionMismatch,
    /// The options are not a map with the one key `k`, an `Int` in `[1, capacity]`.
    InvalidOption,
}

impl ServiceReplyCode {
    /// Every reply code.
    pub const ALL: [ServiceReplyCode; 9] = [
        ServiceReplyCode::BadArgument,
        ServiceReplyCode::EmbeddingUnavailable,
        ServiceReplyCode::EmbeddingRateLimited,
        ServiceReplyCode::NonFiniteEmbedding,
        ServiceReplyCode::NotAVectorDbSpace,
        ServiceReplyCode::MirrorDisabled,
        ServiceReplyCode::MirrorUnavailable,
        ServiceReplyCode::DimensionMismatch,
        ServiceReplyCode::InvalidOption,
    ];

    /// The code string of the reply.
    pub const fn code(self) -> &'static str {
        match self {
            ServiceReplyCode::BadArgument => "BadArgument",
            ServiceReplyCode::EmbeddingUnavailable => "EmbeddingUnavailable",
            ServiceReplyCode::EmbeddingRateLimited => "EmbeddingRateLimited",
            ServiceReplyCode::NonFiniteEmbedding => "NonFiniteEmbedding",
            ServiceReplyCode::NotAVectorDbSpace => "NotAVectorDbSpace",
            ServiceReplyCode::MirrorDisabled => "MirrorDisabled",
            ServiceReplyCode::MirrorUnavailable => "MirrorUnavailable",
            ServiceReplyCode::DimensionMismatch => "DimensionMismatch",
            ServiceReplyCode::InvalidOption => "InvalidOption",
        }
    }

    /// The detail of the reply: a fixed text, so a reply never echoes an argument.
    pub const fn detail(self) -> &'static str {
        match self {
            ServiceReplyCode::BadArgument => "an argument has another shape than the call takes",
            ServiceReplyCode::EmbeddingUnavailable => "the node has no embedding model",
            ServiceReplyCode::EmbeddingRateLimited => "the call exceeds the quota of the node",
            ServiceReplyCode::NonFiniteEmbedding => "the model gives a value that is not finite",
            ServiceReplyCode::NotAVectorDbSpace => "the space is not a VectorDB space",
            ServiceReplyCode::MirrorDisabled => "the space has no mirror",
            ServiceReplyCode::MirrorUnavailable => "the mirror of the node does not answer",
            ServiceReplyCode::DimensionMismatch => "the query has other dimensions than the space",
            ServiceReplyCode::InvalidOption => {
                "the options are a map of k, an Int in [1, capacity]"
            }
        }
    }
}

/// The reply `[true, values…]`.
pub fn ok_reply(values: Vec<Par>) -> Par {
    let mut parts = Vec::with_capacity(values.len() + 1);
    parts.push(expr_par(ExprInstance::GBool(true)));
    parts.extend(values);
    list_par(parts)
}

/// The reply `[false, code, detail]` of `code`.
pub fn failed_reply(code: ServiceReplyCode) -> Par {
    list_par(vec![
        expr_par(ExprInstance::GBool(false)),
        expr_par(ExprInstance::GString(code.code().to_string())),
        expr_par(ExprInstance::GString(code.detail().to_string())),
    ])
}

/// The list of the ground values `values`.
pub fn list_par(values: Vec<Par>) -> Par {
    expr_par(ExprInstance::EListBody(EList {
        ps: values,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

/// The vector `values` as a list of `GFloat32`, the exact output of the model.
pub fn f32_list(values: &[f32]) -> Par {
    list_par(
        values
            .iter()
            .map(|value| expr_par(ExprInstance::GFloat32(value.to_bits())))
            .collect(),
    )
}

/// A failure of the embedder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedFailure {
    /// The node has no embedding model, or the model fails.
    Unavailable,
    /// The model gives a value that is NaN or an infinity.
    NonFinite,
}

/// A failure of the mirror.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorFailure {
    /// The node has no mirror, or the mirror does not answer.
    Unavailable,
}

/// The embedder of a node.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// The name of the model.
    fn model(&self) -> &str;

    /// One vector for each text of `texts`, in order.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedFailure>;
}

/// One neighbour of an advisory answer: the id of a record and its distance from the query.
#[derive(Clone, Debug, PartialEq)]
pub struct Neighbour {
    /// The id of the record: the 32-byte hash of its source produce, as `ids=` takes it.
    pub id: Vec<u8>,
    /// The distance that the mirror reports, nearest first.
    pub distance: f32,
}

/// The mirror of a node.
#[async_trait]
pub trait MirrorBackend: Send + Sync {
    /// The `k` neighbours of `vector` among the records of the channel `channel` in the
    /// collection `collection`, nearest first.
    async fn query(
        &self,
        collection: &str,
        channel: &str,
        vector: &[f32],
        k: usize,
    ) -> Result<Vec<Neighbour>, MirrorFailure>;

    /// The watermark of the collection `collection`: the last finalized block that the
    /// reconciler has applied, or 0.
    async fn watermark(&self, collection: &str) -> Result<u64, MirrorFailure>;
}

/// The embedder of a node without a model.
pub struct UnavailableEmbedder;

#[async_trait]
impl Embedder for UnavailableEmbedder {
    fn model(&self) -> &str { MODEL }

    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedFailure> {
        Err(EmbedFailure::Unavailable)
    }
}

/// The mirror of a node without ChromaDB.
pub struct UnavailableMirror;

#[async_trait]
impl MirrorBackend for UnavailableMirror {
    async fn query(
        &self,
        _collection: &str,
        _channel: &str,
        _vector: &[f32],
        _k: usize,
    ) -> Result<Vec<Neighbour>, MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }
}

/// The services of a node.
#[derive(Clone)]
pub struct VectorDbServices {
    pub embedder: Arc<dyn Embedder>,
    pub mirror: Arc<dyn MirrorBackend>,
}

impl VectorDbServices {
    /// The services of a node without the `chromadb` feature, or without a model and a server.
    pub fn unavailable() -> Self {
        Self {
            embedder: Arc::new(UnavailableEmbedder),
            mirror: Arc::new(UnavailableMirror),
        }
    }
}

/// The services that [`install`] set, or `None` before the first install.
static SERVICES: RwLock<Option<Arc<VectorDbServices>>> = RwLock::new(None);

/// Sets the services of the node, and returns the services that it replaces.
pub fn install(services: VectorDbServices) -> Option<Arc<VectorDbServices>> {
    let mut slot = SERVICES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.replace(Arc::new(services))
}

/// Removes the services of the node, and returns them. [`current`] then gives the unavailable
/// services.
pub fn uninstall() -> Option<Arc<VectorDbServices>> {
    let mut slot = SERVICES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.take()
}

/// The services of the node: the installed services, or the unavailable services.
pub fn current() -> Arc<VectorDbServices> {
    let slot = SERVICES
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match slot.as_ref() {
        Some(services) => Arc::clone(services),
        None => Arc::new(VectorDbServices::unavailable()),
    }
}

/// The name of the mirror collection of the space `sid`: `rrs-` and the lowercase hex of the
/// first 16 bytes of the space id.
pub fn collection_name(sid: &[u8; SID_LEN]) -> String {
    format!("rrs-{}", hex::encode(&sid[..COLLECTION_ID_BYTES]))
}

/// The digest of the collection key `key` in the mirror: the lowercase hex of `Blake2b256` of its
/// canonical inner channel bytes.
pub fn channel_digest(key: &CollectionKey) -> String {
    hex::encode(Blake2b256Hash::new(key.as_bytes()).bytes())
}

/// The key of the metadata of a mirror record that holds the channel digest of its collection key.
/// `ann` asks the mirror only for the records of its channel.
pub const CHANNEL_METADATA: &str = "channel";

/// The key of the metadata of a mirror collection that holds its watermark.
pub const WATERMARK_METADATA: &str = "watermark";

/// The services of a node. With the `chromadb` feature, they are the SBERT embedder and the
/// ChromaDB mirror of the environment of the node (`CHROMA_ENDPOINT`, `CHROMA_TENANT` and
/// `CHROMA_DATABASE`, one database for each node), or the unavailable mirror when the
/// environment names no valid endpoint. Without the feature, they are the unavailable services.
#[cfg(feature = "chromadb")]
pub fn node_services() -> VectorDbServices {
    let mirror: Arc<dyn MirrorBackend> = match chroma_backend::ChromaMirror::from_env() {
        Some(mirror) => Arc::new(mirror),
        None => Arc::new(UnavailableMirror),
    };
    VectorDbServices {
        embedder: Arc::new(chroma_backend::SbertEmbedder::new()),
        mirror,
    }
}

/// The services of a node. With the `chromadb` feature, they are the SBERT embedder and the
/// ChromaDB mirror of the environment of the node. Without the feature, they are the unavailable
/// services.
#[cfg(not(feature = "chromadb"))]
pub fn node_services() -> VectorDbServices { VectorDbServices::unavailable() }

#[cfg(feature = "chromadb")]
pub use chroma_backend::{ChromaMirror, SbertEmbedder};

/// The backends of the `chromadb` feature.
#[cfg(feature = "chromadb")]
mod chroma_backend {
    use std::sync::Arc;

    use async_trait::async_trait;
    use chroma::client::ChromaHttpClientError;
    use chroma::embed::EmbeddingFunction;
    use chroma::types::{
        Include, IncludeList, MetadataComparison, MetadataExpression, MetadataValue,
        PrimitiveOperator, Where,
    };
    use chroma::{ChromaCollection, ChromaHttpClient};
    use tokio::sync::OnceCell;

    use super::{
        EmbedFailure, Embedder, MirrorBackend, MirrorFailure, Neighbour, CHANNEL_METADATA, MODEL,
        WATERMARK_METADATA,
    };
    use crate::rust::interpreter::util::sbert_embeddings::SBERTEmbeddings;

    /// The HTTP status of a collection that the server does not hold.
    const NOT_FOUND: u16 = 404;

    /// The SBERT embedder of [`MODEL`]. It loads the model on its first call, so a node that
    /// never embeds never loads it. A failed load is not kept, so the next call loads again. The
    /// model runs on a blocking thread, so a call never blocks a worker of the runtime.
    pub struct SbertEmbedder {
        model: OnceCell<Arc<SBERTEmbeddings>>,
    }

    impl SbertEmbedder {
        /// An embedder that has not loaded its model yet.
        pub fn new() -> Self {
            Self {
                model: OnceCell::new(),
            }
        }

        /// The model, loaded on the first call.
        async fn loaded(&self) -> Result<Arc<SBERTEmbeddings>, EmbedFailure> {
            self.model
                .get_or_try_init(|| async {
                    match tokio::task::spawn_blocking(SBERTEmbeddings::new).await {
                        Ok(Ok(model)) => Ok(Arc::new(model)),
                        Ok(Err(_)) | Err(_) => Err(EmbedFailure::Unavailable),
                    }
                })
                .await
                .map(Arc::clone)
        }
    }

    impl Default for SbertEmbedder {
        fn default() -> Self { Self::new() }
    }

    #[async_trait]
    impl Embedder for SbertEmbedder {
        fn model(&self) -> &str { MODEL }

        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedFailure> {
            let model = self.loaded().await?;
            let texts = texts.to_vec();
            let vectors = tokio::task::spawn_blocking(move || {
                let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
                futures::executor::block_on(model.embed_strs(&texts))
            })
            .await;
            match vectors {
                Ok(Ok(vectors)) => Ok(vectors),
                Ok(Err(_)) | Err(_) => Err(EmbedFailure::Unavailable),
            }
        }
    }

    /// The ChromaDB mirror of a node.
    pub struct ChromaMirror {
        client: ChromaHttpClient,
    }

    impl ChromaMirror {
        /// The mirror of the environment of the node, or `None` when the environment names no
        /// valid endpoint.
        pub fn from_env() -> Option<Self> {
            ChromaHttpClient::from_env()
                .ok()
                .map(|client| Self { client })
        }

        /// The collection `name`, or `None` when the server does not hold it: the mirror then
        /// holds no record of the space yet.
        async fn collection(&self, name: &str) -> Result<Option<ChromaCollection>, MirrorFailure> {
            match self.client.get_collection(name).await {
                Ok(collection) => Ok(Some(collection)),
                Err(ChromaHttpClientError::ApiError(_, status)) if status.as_u16() == NOT_FOUND => {
                    Ok(None)
                }
                Err(_) => Err(MirrorFailure::Unavailable),
            }
        }
    }

    #[async_trait]
    impl MirrorBackend for ChromaMirror {
        async fn query(
            &self,
            collection: &str,
            channel: &str,
            vector: &[f32],
            k: usize,
        ) -> Result<Vec<Neighbour>, MirrorFailure> {
            let Some(collection) = self.collection(collection).await? else {
                return Ok(Vec::new());
            };
            let n_results = u32::try_from(k).map_err(|_| MirrorFailure::Unavailable)?;
            let filter = Where::Metadata(MetadataExpression {
                key: CHANNEL_METADATA.to_string(),
                comparison: MetadataComparison::Primitive(
                    PrimitiveOperator::Equal,
                    MetadataValue::Str(channel.to_string()),
                ),
            });
            let response = collection
                .query(
                    vec![vector.to_vec()],
                    Some(n_results),
                    Some(filter),
                    None,
                    Some(IncludeList(vec![Include::Distance])),
                )
                .await
                .map_err(|_| MirrorFailure::Unavailable)?;
            let ids = response.ids.into_iter().next().unwrap_or_default();
            let distances = response
                .distances
                .and_then(|distances| distances.into_iter().next())
                .unwrap_or_default();
            if ids.len() != distances.len() {
                return Err(MirrorFailure::Unavailable);
            }
            // A record id of the mirror is the lowercase hex of the 32-byte record id. An id
            // that is not hex, or a neighbour without a distance, names no record.
            Ok(ids
                .into_iter()
                .zip(distances)
                .filter_map(|(id, distance)| {
                    Some(Neighbour {
                        id: hex::decode(id).ok()?,
                        distance: distance?,
                    })
                })
                .collect())
        }

        async fn watermark(&self, collection: &str) -> Result<u64, MirrorFailure> {
            let Some(collection) = self.collection(collection).await? else {
                return Ok(0);
            };
            match collection
                .metadata()
                .as_ref()
                .and_then(|metadata| metadata.get(WATERMARK_METADATA))
            {
                None => Ok(0),
                Some(MetadataValue::Int(watermark)) => {
                    u64::try_from(*watermark).map_err(|_| MirrorFailure::Unavailable)
                }
                Some(_) => Err(MirrorFailure::Unavailable),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectordb_services_names_are_pure_functions_of_consensus_values() {
        let sid = [0xAB; SID_LEN];
        assert_eq!(
            collection_name(&sid),
            format!("rrs-{}", "ab".repeat(COLLECTION_ID_BYTES))
        );
        let mut other = sid;
        other[COLLECTION_ID_BYTES] = 0;
        assert_eq!(
            collection_name(&other),
            collection_name(&sid),
            "only the first 16 bytes name the collection"
        );
        other[0] = 0;
        assert_ne!(collection_name(&other), collection_name(&sid));
        let key = CollectionKey::from_bytes(vec![1, 2, 3]).expect("a key");
        let digest = channel_digest(&key);
        assert_eq!(digest.len(), 64);
        assert_eq!(digest, hex::encode(Blake2b256Hash::new(&[1, 2, 3]).bytes()));
    }

    #[tokio::test]
    async fn vectordb_services_default_to_unavailable() {
        let services = VectorDbServices::unavailable();
        assert_eq!(services.embedder.model(), MODEL);
        assert_eq!(
            services.embedder.embed(&["text".to_string()]).await,
            Err(EmbedFailure::Unavailable)
        );
        assert_eq!(
            services.mirror.query("rrs-00", "00", &[1.0], 1).await,
            Err(MirrorFailure::Unavailable)
        );
        assert_eq!(
            services.mirror.watermark("rrs-00").await,
            Err(MirrorFailure::Unavailable)
        );
    }
}
