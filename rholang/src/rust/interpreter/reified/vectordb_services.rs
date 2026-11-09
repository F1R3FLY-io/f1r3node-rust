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
//! produce event of the call (`non_deterministic_ops`, and in native funded execution the recorded
//! set of DR-114, decision 68), and in replay it produces the recorded reply without a service
//! call. So two nodes with different services, or with none, compute the same state.
//!
//! The services of a node live in one registry: [`install`] sets them, and [`current`] reads them
//! when a process runs. Before an install, [`current`] gives the unavailable services. Changed by
//! decision 105: `embed` and `ann` then reply the base code `EXT_FAILED`. Before, they replied
//! `EmbeddingUnavailable` and `MirrorUnavailable`. [`node_services`] builds the services of the
//! build: with the `chromadb` feature, the SBERT embedder and the ChromaDB mirror of the
//! environment, and otherwise the unavailable services. Changed by decision 109: a validator
//! installs them at start, and an observer installs the unavailable services, as base observers
//! get the no-op Chroma service. Before, the reconciler of every node installed them (P6 step
//! 7c). Tests install a mock embedder and a fake mirror.
//!
//! The names of the mirror are pure functions of consensus values:
//!
//! * the collection of a space is `rrs-` and the lowercase hex of the first 16 bytes of its space
//!   id ([`collection_name`]);
//! * the channel digest of a collection key is the lowercase hex of `Blake2b256` of its canonical
//!   inner channel bytes ([`channel_digest`]), the bytes that name the data channel `D(sid, key)`.

// Changed by decision 114: only the registry, the registration and the dirty marks used the
// map and the lock.
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
// use std::sync::{Arc, Mutex, RwLock};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use linkme::distributed_slice;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Par};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{self};
use tokio::sync::Notify;

use crate::rust::interpreter::spaces::disciplines::vectordb::ID_LEN;
// Added by decision 127 (kind registry).
use crate::rust::interpreter::spaces::families::{SpaceKind, SPACE_KINDS};
use crate::rust::interpreter::spaces::mirror::{MirrorBatch, MirrorMetric, MirrorRecord};
// Changed by decisions 105 and 114: only the registry used the space id, and only the retired
// reply test read the sole expression.
// use crate::rust::interpreter::spaces::namespace::{CollectionKey, SpaceId, SID_LEN};
// use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};
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

// Retired by decision 105: the failure replies of `embed` and `ann` use the base codes
// `EXT_BAD_ARG`, `EXT_FAILED` and `EXT_OUTPUT_TOO_LARGE` (DR-114 item 4).
// /// A failure that `embed` or `ann` replies as `[false, code, detail]`. The reply is recorded, so
// /// replay produces it again.
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum ServiceReplyCode {
//     /// An argument of another shape than the call takes.
//     BadArgument,
//     /// The node has no embedding model, or the model fails.
//     EmbeddingUnavailable,
//     /// The call exceeds the node-local quota of `embed`.
//     EmbeddingRateLimited,
//     /// The model gives a value that is NaN or an infinity.
//     NonFiniteEmbedding,
//     /// The space is not a native VectorDB space.
//     NotAVectorDbSpace,
//     /// The VectorDB space has no mirror.
//     MirrorDisabled,
//     /// The node has no mirror, or the mirror does not answer.
//     MirrorUnavailable,
//     /// The query vector does not have the dimensions of the space.
//     DimensionMismatch,
//     /// The options are not a map with the one key `k`, an `Int` in `[1, capacity]`.
//     InvalidOption,
//     /// A success that does not fit the record room of the deploy (decision 68, DR-114 of
//     /// `feature/cost-accounted-rho`).
//     OutputTooLarge,
// }
//
// impl ServiceReplyCode {
//     /// Every reply code.
//     pub const ALL: [ServiceReplyCode; 10] = [
//         ServiceReplyCode::BadArgument,
//         ServiceReplyCode::EmbeddingUnavailable,
//         ServiceReplyCode::EmbeddingRateLimited,
//         ServiceReplyCode::NonFiniteEmbedding,
//         ServiceReplyCode::NotAVectorDbSpace,
//         ServiceReplyCode::MirrorDisabled,
//         ServiceReplyCode::MirrorUnavailable,
//         ServiceReplyCode::DimensionMismatch,
//         ServiceReplyCode::InvalidOption,
//         ServiceReplyCode::OutputTooLarge,
//     ];
//
//     /// The code string of the reply.
//     pub const fn code(self) -> &'static str {
//         match self {
//             ServiceReplyCode::BadArgument => "BadArgument",
//             ServiceReplyCode::EmbeddingUnavailable => "EmbeddingUnavailable",
//             ServiceReplyCode::EmbeddingRateLimited => "EmbeddingRateLimited",
//             ServiceReplyCode::NonFiniteEmbedding => "NonFiniteEmbedding",
//             ServiceReplyCode::NotAVectorDbSpace => "NotAVectorDbSpace",
//             ServiceReplyCode::MirrorDisabled => "MirrorDisabled",
//             ServiceReplyCode::MirrorUnavailable => "MirrorUnavailable",
//             ServiceReplyCode::DimensionMismatch => "DimensionMismatch",
//             ServiceReplyCode::InvalidOption => "InvalidOption",
//             ServiceReplyCode::OutputTooLarge => "OutputTooLarge",
//         }
//     }
//
//     /// The detail of the reply: a fixed text, so a reply never echoes an argument.
//     pub const fn detail(self) -> &'static str {
//         match self {
//             ServiceReplyCode::BadArgument => "an argument has another shape than the call takes",
//             ServiceReplyCode::EmbeddingUnavailable => "the node has no embedding model",
//             ServiceReplyCode::EmbeddingRateLimited => "the call exceeds the quota of the node",
//             ServiceReplyCode::NonFiniteEmbedding => "the model gives a value that is not finite",
//             ServiceReplyCode::NotAVectorDbSpace => "the space is not a VectorDB space",
//             ServiceReplyCode::MirrorDisabled => "the space has no mirror",
//             ServiceReplyCode::MirrorUnavailable => "the mirror of the node does not answer",
//             ServiceReplyCode::DimensionMismatch => "the query has other dimensions than the space",
//             ServiceReplyCode::InvalidOption => {
//                 "the options are a map of k, an Int in [1, capacity]"
//             }
//             ServiceReplyCode::OutputTooLarge => "the reply exceeds the record room of the deploy",
//         }
//     }
// }

/// The reply `[true, values…]`.
pub fn ok_reply(values: Vec<Par>) -> Par {
    let mut parts = Vec::with_capacity(values.len() + 1);
    parts.push(expr_par(ExprInstance::GBool(true)));
    parts.extend(values);
    list_par(parts)
}

// Retired by decision 105: `embed` and `ann` reply the base codes through `native_failure_reply`
// and `native_played_reply`, so no Reified reply test or failure builder remains.
// /// True when `reply` is a success `[true, values…]` of [`ok_reply`]. Every reply of `embed` and
// /// `ann` comes from [`ok_reply`] or [`failed_reply`], so the head of the list decides.
// pub fn is_ok_reply(reply: &Par) -> bool {
//     match sole_expr(reply) {
//         Some(ExprInstance::EListBody(EList { ps, .. })) => {
//             matches!(
//                 ps.first().and_then(sole_expr),
//                 Some(ExprInstance::GBool(true))
//             )
//         }
//         _ => false,
//     }
// }
//
// /// The reply `[false, code, detail]` of `code`.
// pub fn failed_reply(code: ServiceReplyCode) -> Par {
//     list_par(vec![
//         expr_par(ExprInstance::GBool(false)),
//         expr_par(ExprInstance::GString(code.code().to_string())),
//         expr_par(ExprInstance::GString(code.detail().to_string())),
//     ])
// }

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

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // /// The watermark of the collection `collection`: the last finalized block that the
    // /// reconciler has applied, or 0.
    // async fn watermark(&self, collection: &str) -> Result<u64, MirrorFailure>;

    /// Creates the collection `collection` with the distance `metric` when the mirror does not
    /// hold it.
    async fn ensure_collection(
        &self,
        collection: &str,
        metric: MirrorMetric,
    ) -> Result<(), MirrorFailure>;

    /// Writes `records` to the collection `collection`. A record replaces the record with its
    /// id.
    async fn upsert(&self, collection: &str, records: &[MirrorRecord])
        -> Result<(), MirrorFailure>;

    /// Removes the records `ids` from the collection `collection`. An id that the collection
    /// does not hold is no error.
    async fn delete(&self, collection: &str, ids: &[[u8; ID_LEN]]) -> Result<(), MirrorFailure>;

    // Retired by decision 114: a batch registers no channels.
    // /// Registers the channels `channels` (the key bytes of each channel digest) of the space
    // /// `space` in the collection `collection`, so the reconciler can repair them.
    // async fn register(
    //     &self,
    //     collection: &str,
    //     space: &SpaceId,
    //     channels: &BTreeMap<String, Vec<u8>>,
    // ) -> Result<(), MirrorFailure>;

    /// The mirror collections of the node: the collections whose name starts with
    /// [`COLLECTION_PREFIX`].
    async fn collections(&self) -> Result<Vec<String>, MirrorFailure>;

    // Retired by decision 114: no reconciler reads a registry of channels.
    // /// The registry of the collection `collection`: its space, its registered channels and its
    // /// watermark.
    // async fn registry(&self, collection: &str) -> Result<MirrorRegistry, MirrorFailure>;

    /// The ids of the records of the channel `channel` in the collection `collection`.
    async fn ids(
        &self,
        collection: &str,
        channel: &str,
    ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure>;

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // /// Sets the watermark of the collection `collection` to `watermark`.
    // async fn set_watermark(&self, collection: &str, watermark: u64) -> Result<(), MirrorFailure>;
}

// Retired by decision 114: no reconciler reads a registry of channels.
// /// The registry of a mirror collection (P6 step 7c): the space of the collection, the channels
// /// that the journal of the node wrote, by channel digest, and the last finalized block that the
// /// reconciler applied.
// #[derive(Clone, Debug, Default, PartialEq, Eq)]
// pub struct MirrorRegistry {
//     /// The space of the collection, or `None` before the first registration.
//     pub space: Option<SpaceId>,
//     /// The key bytes of each registered channel, by channel digest.
//     pub channels: BTreeMap<String, Vec<u8>>,
//     /// The last finalized block that the reconciler applied, or 0.
//     pub watermark: u64,
// }

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

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> {
    //     Err(MirrorFailure::Unavailable)
    // }

    async fn ensure_collection(
        &self,
        _collection: &str,
        _metric: MirrorMetric,
    ) -> Result<(), MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    async fn upsert(
        &self,
        _collection: &str,
        _records: &[MirrorRecord],
    ) -> Result<(), MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    async fn delete(&self, _collection: &str, _ids: &[[u8; ID_LEN]]) -> Result<(), MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    // Retired by decision 114: a batch registers no channels.
    // async fn register(
    //     &self,
    //     _collection: &str,
    //     _space: &SpaceId,
    //     _channels: &BTreeMap<String, Vec<u8>>,
    // ) -> Result<(), MirrorFailure> {
    //     Err(MirrorFailure::Unavailable)
    // }

    async fn collections(&self) -> Result<Vec<String>, MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    // Retired by decision 114: no reconciler reads a registry of channels.
    // async fn registry(&self, _collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
    //     Err(MirrorFailure::Unavailable)
    // }

    async fn ids(
        &self,
        _collection: &str,
        _channel: &str,
    ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
        Err(MirrorFailure::Unavailable)
    }

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // async fn set_watermark(&self, _collection: &str, _watermark: u64) -> Result<(), MirrorFailure> {
    //     Err(MirrorFailure::Unavailable)
    // }
}

/// The writes of one published batch, applied in this order: the collection, the upserts, the
/// deletes. Changed by decision 114: a batch registers no channels. Before, the registration of
/// its channels came after the collection.
async fn apply(mirror: &dyn MirrorBackend, batch: &MirrorBatch) -> Result<(), MirrorFailure> {
    mirror
        .ensure_collection(&batch.collection, batch.metric)
        .await?;
    // Retired by decision 114: a batch registers no channels.
    // if !batch.channels.is_empty() {
    //     mirror
    //         .register(&batch.collection, &batch.space, &batch.channels)
    //         .await?;
    // }
    if !batch.upserts.is_empty() {
        mirror.upsert(&batch.collection, &batch.upserts).await?;
    }
    if !batch.deletes.is_empty() {
        mirror.delete(&batch.collection, &batch.deletes).await?;
    }
    Ok(())
}

// Retired by decision 114: nothing repairs the mirror, so no collection is dirty.
// /// A collection whose mirror misses writes, with what its repair needs: the space, the distance
// /// and the channels of the batches that the writer did not apply.
// #[derive(Clone, Debug, PartialEq, Eq)]
// pub struct DirtyCollection {
//     pub space: SpaceId,
//     pub metric: MirrorMetric,
//     pub channels: BTreeMap<String, Vec<u8>>,
// }
//
// impl DirtyCollection {
//     /// The dirty state of the batch `batch`.
//     fn of(batch: &MirrorBatch) -> Self {
//         Self {
//             space: batch.space,
//             metric: batch.metric,
//             channels: batch.channels.clone(),
//         }
//     }
// }

/// The progress of a mirror writer: the batches that its queue took and the batches that its
/// task finished. Changed by decision 114: it keeps no dirty marks. Before, it also kept the
/// collections whose mirror missed writes.
#[derive(Default)]
struct WriterProgress {
    sent: AtomicU64,
    finished: AtomicU64,
    changed: Notify,
    // Retired by decision 114: the writer keeps no dirty marks.
    // dirty: Mutex<BTreeMap<String, DirtyCollection>>,
}

// Retired by decision 114: nothing repairs the mirror, so the writer keeps no dirty marks.
// impl WriterProgress {
//     /// Marks the collection `collection` dirty with `state`, merged with an earlier mark.
//     fn mark_dirty(&self, collection: String, state: DirtyCollection) {
//         let mut dirty = self
//             .dirty
//             .lock()
//             .unwrap_or_else(|poisoned| poisoned.into_inner());
//         match dirty.get_mut(&collection) {
//             Some(known) => known.channels.extend(state.channels),
//             None => {
//                 dirty.insert(collection, state);
//             }
//         }
//     }
// }

/// The mirror writer of a node (P6 step 7b, decision 58). One task applies the published batches
/// to the mirror in publication order, and it retries a failed batch a bounded number of times.
/// Publication never waits: when the bounded queue is full, or when a batch still fails after
/// its retries, the writer drops the batch (decision 114). Before, it marked the collection
/// dirty, and the reconciler of the node repaired it.
pub struct MirrorWriter {
    queue: mpsc::Sender<MirrorBatch>,
    progress: Arc<WriterProgress>,
}

impl MirrorWriter {
    /// The batches that the queue of a node holds.
    pub const QUEUE: usize = 1024;

    /// The attempts of a batch before the writer drops it (decision 114).
    pub const ATTEMPTS: u32 = 3;

    /// The pause before the second attempt of a batch. The next pause doubles.
    pub const RETRY_PAUSE: Duration = Duration::from_millis(100);

    /// A writer to `mirror` with a queue of [`MirrorWriter::QUEUE`] batches. It spawns its task,
    /// so it needs a Tokio runtime.
    pub fn spawn(mirror: Arc<dyn MirrorBackend>) -> Arc<Self> {
        Self::spawn_with(mirror, Self::QUEUE)
    }

    /// A writer to `mirror` with a queue of `capacity` batches.
    pub fn spawn_with(mirror: Arc<dyn MirrorBackend>, capacity: usize) -> Arc<Self> {
        let (queue, mut batches) = mpsc::channel::<MirrorBatch>(capacity.max(1));
        let progress = Arc::new(WriterProgress::default());
        let task_progress = Arc::clone(&progress);
        tokio::spawn(async move {
            while let Some(batch) = batches.recv().await {
                let mut pause = Self::RETRY_PAUSE;
                // Retired by decision 114: a batch that still fails after its attempts is dropped.
                // let mut applied = false;
                for attempt in 1..=Self::ATTEMPTS {
                    if apply(mirror.as_ref(), &batch).await.is_ok() {
                        // applied = true;
                        break;
                    }
                    if attempt < Self::ATTEMPTS {
                        tokio::time::sleep(pause).await;
                        pause *= 2;
                    }
                }
                // if !applied {
                //     let state = DirtyCollection::of(&batch);
                //     task_progress.mark_dirty(batch.collection, state);
                // }
                task_progress.finished.fetch_add(1, Ordering::SeqCst);
                task_progress.changed.notify_waiters();
            }
        });
        Arc::new(Self { queue, progress })
    }

    /// Hands `batches` to the queue and never waits. A batch that the queue does not take is
    /// dropped (decision 114). Before, it marked its collection dirty.
    pub fn publish(&self, batches: Vec<MirrorBatch>) {
        for batch in batches {
            self.progress.sent.fetch_add(1, Ordering::SeqCst);
            match self.queue.try_send(batch) {
                Ok(()) => {}
                // Changed by decision 114: nothing repairs the mirror, so a batch that the queue
                // does not take is dropped.
                // Err(TrySendError::Full(batch) | TrySendError::Closed(batch)) => {
                //     self.progress.sent.fetch_sub(1, Ordering::SeqCst);
                //     let state = DirtyCollection::of(&batch);
                //     self.progress.mark_dirty(batch.collection, state);
                //     self.progress.changed.notify_waiters();
                // }
                Err(TrySendError::Full(_) | TrySendError::Closed(_)) => {
                    self.progress.sent.fetch_sub(1, Ordering::SeqCst);
                    self.progress.changed.notify_waiters();
                }
            }
        }
    }

    /// Waits until the task has finished every batch that the queue took.
    pub async fn flush(&self) {
        loop {
            let changed = self.progress.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.progress.finished.load(Ordering::SeqCst)
                >= self.progress.sent.load(Ordering::SeqCst)
            {
                return;
            }
            changed.await;
        }
    }

    // Retired by decision 114: nothing repairs the mirror, so the writer keeps no dirty marks.
    // /// The names of the collections whose mirror misses writes, without removing them.
    // pub fn dirty(&self) -> BTreeSet<String> {
    //     self.progress
    //         .dirty
    //         .lock()
    //         .unwrap_or_else(|poisoned| poisoned.into_inner())
    //         .keys()
    //         .cloned()
    //         .collect()
    // }
    //
    // /// Removes and returns the collections whose mirror misses writes, with what their repair
    // /// needs.
    // pub fn take_dirty(&self) -> BTreeMap<String, DirtyCollection> {
    //     std::mem::take(
    //         &mut *self
    //             .progress
    //             .dirty
    //             .lock()
    //             .unwrap_or_else(|poisoned| poisoned.into_inner()),
    //     )
    // }
    //
    // /// Marks the collection `collection` dirty again, after a repair that did not finish.
    // pub fn mark_dirty(&self, collection: String, state: DirtyCollection) {
    //     self.progress.mark_dirty(collection, state);
    // }
}

/// The services of a node.
#[derive(Clone)]
pub struct VectorDbServices {
    pub embedder: Arc<dyn Embedder>,
    pub mirror: Arc<dyn MirrorBackend>,
    /// The writer of the mirror, or `None` for a node without a mirror: its publication drops
    /// the batches. Changed by decision 98: the batches of each commit. Before, the journal.
    pub writer: Option<Arc<MirrorWriter>>,
}

impl VectorDbServices {
    /// The services of a node without the `chromadb` feature, or without a model and a server.
    pub fn unavailable() -> Self {
        Self {
            embedder: Arc::new(UnavailableEmbedder),
            mirror: Arc::new(UnavailableMirror),
            writer: None,
        }
    }

    /// The services of `embedder` and `mirror`, with a writer to `mirror`. It spawns the task of
    /// the writer, so it needs a Tokio runtime.
    pub fn with_writer(embedder: Arc<dyn Embedder>, mirror: Arc<dyn MirrorBackend>) -> Self {
        let writer = MirrorWriter::spawn(Arc::clone(&mirror));
        Self {
            embedder,
            mirror,
            writer: Some(writer),
        }
    }

    /// Publishes `batches` to the writer of the mirror, and drops them without one.
    pub fn publish(&self, batches: Vec<MirrorBatch>) {
        if let Some(writer) = &self.writer {
            writer.publish(batches);
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
    format!(
        "{COLLECTION_PREFIX}{}",
        hex::encode(&sid[..COLLECTION_ID_BYTES])
    )
}

// Retired by decision 114: no reconciler reads a registry of channels.
// /// The registry of the collection metadata `metadata`: the space id, the registered channels and
// /// the watermark. A value of another shape is `Unavailable`, because only this node writes the
// /// metadata of its collections.
// pub fn registry_of<'m>(
//     metadata: impl IntoIterator<Item = (&'m str, RegistryValue<'m>)>,
// ) -> Result<MirrorRegistry, MirrorFailure> {
//     let mut registry = MirrorRegistry::default();
//     for (key, value) in metadata {
//         match (key, value) {
//             (SPACE_ID_METADATA, RegistryValue::Text(text)) => {
//                 let bytes = hex::decode(text).map_err(|_| MirrorFailure::Unavailable)?;
//                 registry.space = Some(
//                     <SpaceId>::try_from(bytes.as_slice())
//                         .map_err(|_| MirrorFailure::Unavailable)?,
//                 );
//             }
//             (WATERMARK_METADATA, RegistryValue::Int(watermark)) => {
//                 registry.watermark =
//                     u64::try_from(watermark).map_err(|_| MirrorFailure::Unavailable)?;
//             }
//             (key, RegistryValue::Text(text)) if key.starts_with(REGISTERED_CHANNEL_PREFIX) => {
//                 let digest = &key[REGISTERED_CHANNEL_PREFIX.len()..];
//                 let bytes = hex::decode(text).map_err(|_| MirrorFailure::Unavailable)?;
//                 registry.channels.insert(digest.to_string(), bytes);
//             }
//             (SPACE_ID_METADATA | WATERMARK_METADATA, _) => return Err(MirrorFailure::Unavailable),
//             _ => {}
//         }
//     }
//     Ok(registry)
// }
//
// /// A metadata value of a mirror collection, as [`registry_of`] reads it.
// #[derive(Clone, Copy, Debug)]
// pub enum RegistryValue<'m> {
//     Text(&'m str),
//     Int(i64),
//     Other,
// }

/// The digest of the collection key `key` in the mirror: the lowercase hex of `Blake2b256` of its
/// canonical inner channel bytes.
pub fn channel_digest(key: &CollectionKey) -> String {
    hex::encode(Blake2b256Hash::new(key.as_bytes()).bytes())
}

/// The key of the metadata of a mirror record that holds the channel digest of its collection key.
/// `ann` asks the mirror only for the records of its channel.
pub const CHANNEL_METADATA: &str = "channel";

// Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
// /// The key of the metadata of a mirror collection that holds its watermark.
// pub const WATERMARK_METADATA: &str = "watermark";

/// The prefix of the name of every mirror collection.
pub const COLLECTION_PREFIX: &str = "rrs-";

// Retired by decision 114: no reconciler reads the space of a collection.
// /// The metadata key of a mirror collection that holds the lowercase hex of its space id.
// pub const SPACE_ID_METADATA: &str = "space";

// Retired by decision 114: a batch registers no channels.
// /// The prefix of the metadata key of a registered channel: the prefix and the channel digest. The
// /// value is the lowercase hex of the key bytes of the channel.
// pub const REGISTERED_CHANNEL_PREFIX: &str = "channel:";

/// The metadata key of a mirror record that holds its persistence.
pub const PERSISTENT_METADATA: &str = "persistent";

/// The metadata key of a mirror collection that holds its distance (ChromaDB `hnsw:space`).
pub const SPACE_METADATA: &str = "hnsw:space";

/// The services of a node. With the `chromadb` feature, they are the SBERT embedder and the
/// ChromaDB mirror of the environment of the node (`CHROMA_ENDPOINT`, `CHROMA_TENANT` and
/// `CHROMA_DATABASE`, one database for each node), or the unavailable mirror when the
/// environment names no valid endpoint. Without the feature, they are the unavailable services.
#[cfg(feature = "chromadb")]
pub fn node_services() -> VectorDbServices {
    let embedder: Arc<dyn Embedder> = Arc::new(chroma_backend::SbertEmbedder::new());
    match chroma_backend::ChromaMirror::from_env() {
        Some(mirror) => VectorDbServices::with_writer(embedder, Arc::new(mirror)),
        None => VectorDbServices {
            embedder,
            mirror: Arc::new(UnavailableMirror),
            writer: None,
        },
    }
}

/// The services of a node. With the `chromadb` feature, they are the SBERT embedder and the
/// ChromaDB mirror of the environment of the node. Without the feature, they are the unavailable
/// services.
#[cfg(not(feature = "chromadb"))]
pub fn node_services() -> VectorDbServices { VectorDbServices::unavailable() }

/// Added by decisions 109 and 127: the installer of the VectorDB services. A validator installs
/// the services of the build, and an observer installs the unavailable services, as
/// `ExternalServices::for_node_type` does.
fn install_node_services(is_validator: bool) {
    let services = match is_validator {
        true => node_services(),
        false => VectorDbServices::unavailable(),
    };
    install(services);
}

/// Added by decision 127 (kind registry): the VectorDB kind registers the installer of its
/// services. Its families and processes stay with the built-in families and processes.
#[distributed_slice(SPACE_KINDS)]
static VECTORDB_KIND: SpaceKind = SpaceKind {
    families: &[],
    processes: &[],
    services: Some(install_node_services),
};

#[cfg(feature = "chromadb")]
pub use chroma_backend::{ChromaMirror, SbertEmbedder};

/// The backends of the `chromadb` feature.
#[cfg(feature = "chromadb")]
mod chroma_backend {
    // Changed by decision 114: only the registration of channels used the map and the lock.
    // use std::collections::{BTreeMap, BTreeSet};
    // use std::sync::{Arc, Mutex};
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use async_trait::async_trait;
    use chroma::client::ChromaHttpClientError;
    use chroma::embed::EmbeddingFunction;
    use chroma::types::{
        Include, IncludeList, Metadata, MetadataComparison, MetadataExpression, MetadataValue,
        PrimitiveOperator, UpdateMetadata, UpdateMetadataValue, Where,
    };
    use chroma::{ChromaCollection, ChromaHttpClient};
    // Changed by decision 114: no read-modify-write of collection metadata remains.
    // use tokio::sync::{Mutex as AsyncMutex, OnceCell};
    use tokio::sync::OnceCell;

    // Changed by decision 114: the registry, the registration and the watermark are retired.
    // use super::{
    //     registry_of, EmbedFailure, Embedder, MirrorBackend, MirrorFailure, MirrorMetric,
    //     MirrorRecord, MirrorRegistry, Neighbour, RegistryValue, SpaceId, CHANNEL_METADATA,
    //     COLLECTION_PREFIX, ID_LEN, MODEL, PERSISTENT_METADATA, REGISTERED_CHANNEL_PREFIX,
    //     SPACE_ID_METADATA, SPACE_METADATA, WATERMARK_METADATA,
    // };
    use super::{
        EmbedFailure, Embedder, MirrorBackend, MirrorFailure, MirrorMetric, MirrorRecord,
        Neighbour, CHANNEL_METADATA, COLLECTION_PREFIX, ID_LEN, MODEL, PERSISTENT_METADATA,
        SPACE_METADATA,
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

    /// The collections of one page of `list_collections`.
    const COLLECTION_PAGE: usize = 100;

    /// The record ids of one page of `get`.
    const ID_PAGE: u32 = 1000;

    /// The ChromaDB mirror of a node. The writer of the node is its only writer, and the
    /// database of the node has no other writer. Changed by decision 114: no read-modify-write
    /// of collection metadata remains. Before, `register` and `set_watermark` held
    /// `metadata_lock`, and the reconciler shared this mirror.
    pub struct ChromaMirror {
        client: ChromaHttpClient,
        /// Set once the database of the node exists ([`ChromaMirror::ready`]).
        database: OnceCell<()>,
        // Retired by decision 114: the registration of channels and the watermark used them.
        // metadata_lock: AsyncMutex<()>,
        // /// The registered channel digests of each collection, as far as this mirror wrote or
        // /// read them.
        // registered: Mutex<BTreeMap<String, BTreeSet<String>>>,
    }

    impl ChromaMirror {
        /// The mirror of the environment of the node, or `None` when the environment names no
        /// valid endpoint.
        pub fn from_env() -> Option<Self> {
            ChromaHttpClient::from_env().ok().map(|client| Self {
                client,
                database: OnceCell::new(),
                // Retired by decision 114.
                // metadata_lock: AsyncMutex::new(()),
                // registered: Mutex::new(BTreeMap::new()),
            })
        }

        /// Makes sure that the database of the node (`CHROMA_DATABASE`) exists, once. A node has
        /// its own database, which may not exist yet. Every call that reads or writes the
        /// database waits for it.
        async fn ready(&self) -> Result<(), MirrorFailure> {
            self.database
                .get_or_try_init(|| async {
                    let name = self
                        .client
                        .get_database_name()
                        .await
                        .map_err(|_| MirrorFailure::Unavailable)?;
                    let databases = self
                        .client
                        .list_databases()
                        .await
                        .map_err(|_| MirrorFailure::Unavailable)?;
                    match databases.iter().any(|database| database.name == name) {
                        true => Ok(()),
                        false => self
                            .client
                            .create_database(&name)
                            .await
                            .map_err(|_| MirrorFailure::Unavailable),
                    }
                })
                .await
                .map(drop)
        }

        // Retired by decision 114: only the registration of channels read what this mirror knew.
        // /// True when every digest of `digests` is registered in `collection`, as far as this
        // /// mirror knows.
        // fn knows(&self, collection: &str, digests: impl Iterator<Item = String>) -> bool {
        //     let registered = self
        //         .registered
        //         .lock()
        //         .unwrap_or_else(|poisoned| poisoned.into_inner());
        //     let Some(known) = registered.get(collection) else {
        //         return false;
        //     };
        //     let mut digests = digests;
        //     digests.all(|digest| known.contains(&digest))
        // }
        //
        // /// Records that `digests` are registered in `collection`.
        // fn remember(&self, collection: &str, digests: impl Iterator<Item = String>) {
        //     self.registered
        //         .lock()
        //         .unwrap_or_else(|poisoned| poisoned.into_inner())
        //         .entry(collection.to_string())
        //         .or_default()
        //         .extend(digests);
        // }

        /// The collection `name`, or `None` when the server does not hold it: the mirror then
        /// holds no record of the space yet.
        async fn collection(&self, name: &str) -> Result<Option<ChromaCollection>, MirrorFailure> {
            self.ready().await?;
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
            let response = collection
                .query(
                    vec![vector.to_vec()],
                    Some(n_results),
                    Some(channel_filter(channel)),
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

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn watermark(&self, collection: &str) -> Result<u64, MirrorFailure> {
        //     let Some(collection) = self.collection(collection).await? else {
        //         return Ok(0);
        //     };
        //     match collection
        //         .metadata()
        //         .as_ref()
        //         .and_then(|metadata| metadata.get(WATERMARK_METADATA))
        //     {
        //         None => Ok(0),
        //         Some(MetadataValue::Int(watermark)) => {
        //             u64::try_from(*watermark).map_err(|_| MirrorFailure::Unavailable)
        //         }
        //         Some(_) => Err(MirrorFailure::Unavailable),
        //     }
        // }

        async fn ensure_collection(
            &self,
            collection: &str,
            metric: MirrorMetric,
        ) -> Result<(), MirrorFailure> {
            // An existing collection keeps its metadata. Changed by decision 114: the metadata
            // holds no watermark of a reconciler now.
            if self.collection(collection).await?.is_some() {
                return Ok(());
            }
            let metadata = Metadata::from([(
                SPACE_METADATA.to_string(),
                MetadataValue::Str(metric.space().to_string()),
            )]);
            self.ready().await?;
            self.client
                .get_or_create_collection(collection, None, Some(metadata))
                .await
                .map(drop)
                .map_err(|_| MirrorFailure::Unavailable)
        }

        async fn upsert(
            &self,
            collection: &str,
            records: &[MirrorRecord],
        ) -> Result<(), MirrorFailure> {
            let collection = self
                .collection(collection)
                .await?
                .ok_or(MirrorFailure::Unavailable)?;
            let mut ids = Vec::with_capacity(records.len());
            let mut embeddings = Vec::with_capacity(records.len());
            let mut documents = Vec::with_capacity(records.len());
            let mut metadatas = Vec::with_capacity(records.len());
            for record in records {
                ids.push(hex::encode(record.id));
                embeddings.push(record.embedding.clone());
                documents.push(record.document.clone());
                metadatas.push(Some(UpdateMetadata::from([
                    (
                        CHANNEL_METADATA.to_string(),
                        UpdateMetadataValue::Str(record.channel.clone()),
                    ),
                    (
                        PERSISTENT_METADATA.to_string(),
                        UpdateMetadataValue::Bool(record.persistent),
                    ),
                ])));
            }
            collection
                .upsert(ids, embeddings, Some(documents), None, Some(metadatas))
                .await
                .map(drop)
                .map_err(|_| MirrorFailure::Unavailable)
        }

        async fn delete(
            &self,
            collection: &str,
            ids: &[[u8; ID_LEN]],
        ) -> Result<(), MirrorFailure> {
            // A collection that the server does not hold holds none of the records.
            let Some(collection) = self.collection(collection).await? else {
                return Ok(());
            };
            let ids = ids.iter().map(hex::encode).collect();
            collection
                .delete(Some(ids), None)
                .await
                .map(drop)
                .map_err(|_| MirrorFailure::Unavailable)
        }

        // Retired by decision 114: a batch registers no channels.
        // async fn register(
        //     &self,
        //     collection: &str,
        //     space: &SpaceId,
        //     channels: &BTreeMap<String, Vec<u8>>,
        // ) -> Result<(), MirrorFailure> {
        //     if self.knows(collection, channels.keys().cloned()) {
        //         return Ok(());
        //     }
        //     let _guard = self.metadata_lock.lock().await;
        //     let mut held = self
        //         .collection(collection)
        //         .await?
        //         .ok_or(MirrorFailure::Unavailable)?;
        //     let mut metadata = held.metadata().clone().unwrap_or_default();
        //     let mut changed = false;
        //     if !metadata.contains_key(SPACE_ID_METADATA) {
        //         metadata.insert(
        //             SPACE_ID_METADATA.to_string(),
        //             MetadataValue::Str(hex::encode(space)),
        //         );
        //         changed = true;
        //     }
        //     for (digest, key) in channels {
        //         let name = format!("{REGISTERED_CHANNEL_PREFIX}{digest}");
        //         if !metadata.contains_key(&name) {
        //             metadata.insert(name, MetadataValue::Str(hex::encode(key)));
        //             changed = true;
        //         }
        //     }
        //     if changed {
        //         held.modify(None::<&str>, Some(metadata.clone()))
        //             .await
        //             .map_err(|_| MirrorFailure::Unavailable)?;
        //     }
        //     self.remember(
        //         collection,
        //         metadata.keys().filter_map(|key| {
        //             key.strip_prefix(REGISTERED_CHANNEL_PREFIX)
        //                 .map(str::to_string)
        //         }),
        //     );
        //     Ok(())
        // }

        async fn collections(&self) -> Result<Vec<String>, MirrorFailure> {
            self.ready().await?;
            let mut names = Vec::new();
            let mut offset = 0;
            loop {
                let page = self
                    .client
                    .list_collections(COLLECTION_PAGE, Some(offset))
                    .await
                    .map_err(|_| MirrorFailure::Unavailable)?;
                let count = page.len();
                names.extend(
                    page.iter()
                        .map(|collection| collection.name().to_string())
                        .filter(|name| name.starts_with(COLLECTION_PREFIX)),
                );
                if count < COLLECTION_PAGE {
                    return Ok(names);
                }
                offset += count;
            }
        }

        // Retired by decision 114: no reconciler reads a registry of channels.
        // async fn registry(&self, collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
        //     let Some(held) = self.collection(collection).await? else {
        //         return Ok(MirrorRegistry::default());
        //     };
        //     let metadata = held.metadata().clone().unwrap_or_default();
        //     let registry = registry_of(metadata.iter().map(|(key, value)| {
        //         let value = match value {
        //             MetadataValue::Str(text) => RegistryValue::Text(text),
        //             MetadataValue::Int(number) => RegistryValue::Int(*number),
        //             _ => RegistryValue::Other,
        //         };
        //         (key.as_str(), value)
        //     }))?;
        //     self.remember(collection, registry.channels.keys().cloned());
        //     Ok(registry)
        // }

        async fn ids(
            &self,
            collection: &str,
            channel: &str,
        ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
            let Some(held) = self.collection(collection).await? else {
                return Ok(BTreeSet::new());
            };
            let mut ids = BTreeSet::new();
            let mut offset = 0;
            loop {
                let page = held
                    .get(
                        None,
                        Some(channel_filter(channel)),
                        Some(ID_PAGE),
                        Some(offset),
                        Some(IncludeList(Vec::new())),
                    )
                    .await
                    .map_err(|_| MirrorFailure::Unavailable)?;
                let count = page.ids.len();
                // An id that is not the hex of 32 bytes names no record of a space.
                ids.extend(page.ids.iter().filter_map(|id| {
                    <[u8; ID_LEN]>::try_from(hex::decode(id).ok()?.as_slice()).ok()
                }));
                if count < ID_PAGE as usize {
                    return Ok(ids);
                }
                offset += u32::try_from(count).map_err(|_| MirrorFailure::Unavailable)?;
            }
        }

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn set_watermark(
        //     &self,
        //     collection: &str,
        //     watermark: u64,
        // ) -> Result<(), MirrorFailure> {
        //     let watermark = i64::try_from(watermark).map_err(|_| MirrorFailure::Unavailable)?;
        //     let _guard = self.metadata_lock.lock().await;
        //     let mut held = self
        //         .collection(collection)
        //         .await?
        //         .ok_or(MirrorFailure::Unavailable)?;
        //     let mut metadata = held.metadata().clone().unwrap_or_default();
        //     metadata.insert(
        //         WATERMARK_METADATA.to_string(),
        //         MetadataValue::Int(watermark),
        //     );
        //     held.modify(None::<&str>, Some(metadata))
        //         .await
        //         .map_err(|_| MirrorFailure::Unavailable)
        // }
    }

    /// The filter of the records of the channel `channel`.
    fn channel_filter(channel: &str) -> Where {
        Where::Metadata(MetadataExpression {
            key: CHANNEL_METADATA.to_string(),
            comparison: MetadataComparison::Primitive(
                PrimitiveOperator::Equal,
                MetadataValue::Str(channel.to_string()),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    // Added by decision 114: the main module no longer imports the map and the lock.
    use std::collections::BTreeMap;
    use std::sync::Mutex;

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
        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // assert_eq!(
        //     services.mirror.watermark("rrs-00").await,
        //     Err(MirrorFailure::Unavailable)
        // );
        assert_eq!(
            services
                .mirror
                .ensure_collection("rrs-00", MirrorMetric::Cosine)
                .await,
            Err(MirrorFailure::Unavailable)
        );
        assert_eq!(
            services.mirror.upsert("rrs-00", &[]).await,
            Err(MirrorFailure::Unavailable)
        );
        assert_eq!(
            services.mirror.delete("rrs-00", &[]).await,
            Err(MirrorFailure::Unavailable)
        );
        assert!(
            services.writer.is_none(),
            // Changed by decision 98: the batches of each commit, not a journal.
            // "the publication of a node without a mirror drops the journal"
            "the publication of a node without a mirror drops the batches"
        );
        services.publish(vec![batch("rrs-00", 1)]);
    }

    /// A mirror that logs each write call. It refuses the first `refusals` calls, and it holds
    /// each call until its gate opens when it is gated.
    struct ScriptedMirror {
        calls: Mutex<Vec<String>>,
        refusals: AtomicU64,
        gate: Option<tokio::sync::Semaphore>,
    }

    impl ScriptedMirror {
        fn new(refusals: u64, gated: bool) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                refusals: AtomicU64::new(refusals),
                gate: gated.then(|| tokio::sync::Semaphore::new(0)),
            })
        }

        fn open(&self) {
            if let Some(gate) = &self.gate {
                gate.add_permits(tokio::sync::Semaphore::MAX_PERMITS / 2);
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("the call log of the scripted mirror")
                .clone()
        }

        async fn call(&self, call: String) -> Result<(), MirrorFailure> {
            self.calls
                .lock()
                .expect("the call log of the scripted mirror")
                .push(call);
            if let Some(gate) = &self.gate {
                gate.acquire()
                    .await
                    .map(drop)
                    .map_err(|_| MirrorFailure::Unavailable)?;
            }
            let refused = self
                .refusals
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok();
            match refused {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(()),
            }
        }
    }

    #[async_trait]
    impl MirrorBackend for ScriptedMirror {
        async fn query(
            &self,
            _collection: &str,
            _channel: &str,
            _vector: &[f32],
            _k: usize,
        ) -> Result<Vec<Neighbour>, MirrorFailure> {
            Ok(Vec::new())
        }

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> { Ok(0) }

        async fn ensure_collection(
            &self,
            collection: &str,
            metric: MirrorMetric,
        ) -> Result<(), MirrorFailure> {
            self.call(format!("ensure {collection} {}", metric.space()))
                .await
        }

        async fn upsert(
            &self,
            collection: &str,
            records: &[MirrorRecord],
        ) -> Result<(), MirrorFailure> {
            let ids: Vec<u8> = records.iter().map(|record| record.id[0]).collect();
            self.call(format!("upsert {collection} {ids:?}")).await
        }

        async fn delete(
            &self,
            collection: &str,
            ids: &[[u8; ID_LEN]],
        ) -> Result<(), MirrorFailure> {
            let ids: Vec<u8> = ids.iter().map(|id| id[0]).collect();
            self.call(format!("delete {collection} {ids:?}")).await
        }

        // Retired by decision 114: a batch registers no channels.
        // async fn register(
        //     &self,
        //     collection: &str,
        //     space: &SpaceId,
        //     channels: &BTreeMap<String, Vec<u8>>,
        // ) -> Result<(), MirrorFailure> {
        //     let digests: Vec<&String> = channels.keys().collect();
        //     self.call(format!("register {collection} {} {digests:?}", space[0]))
        //         .await
        // }

        async fn collections(&self) -> Result<Vec<String>, MirrorFailure> { Ok(Vec::new()) }

        // Retired by decision 114: no reconciler reads a registry of channels.
        // async fn registry(&self, _collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
        //     Ok(MirrorRegistry::default())
        // }

        async fn ids(
            &self,
            _collection: &str,
            _channel: &str,
        ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
            Ok(BTreeSet::new())
        }

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn set_watermark(
        //     &self,
        //     collection: &str,
        //     watermark: u64,
        // ) -> Result<(), MirrorFailure> {
        //     self.call(format!("watermark {collection} {watermark}"))
        //         .await
        // }
    }

    /// A batch of `collection` that upserts the record `byte` and deletes the record `byte + 1`.
    fn batch(collection: &str, byte: u8) -> MirrorBatch {
        MirrorBatch {
            space: [9; SID_LEN],
            collection: collection.to_string(),
            metric: MirrorMetric::Cosine,
            upserts: vec![MirrorRecord {
                id: [byte; ID_LEN],
                embedding: vec![1.0, 0.0],
                channel: "c".to_string(),
                persistent: false,
                document: None,
            }],
            deletes: vec![[byte + 1; ID_LEN]],
            // Retired by decision 114: a batch registers no channels.
            // channels: BTreeMap::new(),
        }
    }

    // Retired by decision 114: a batch registers no channels.
    // /// [`batch`] with the channels `channels`, each with the key bytes `[channel]`.
    // fn batch_on(collection: &str, byte: u8, channels: &[u8]) -> MirrorBatch {
    //     MirrorBatch {
    //         channels: channels
    //             .iter()
    //             .map(|channel| (format!("d{channel}"), vec![*channel]))
    //             .collect(),
    //         ..batch(collection, byte)
    //     }
    // }

    /// The task applies the batches in publication order: the collection, the upserts, the
    /// deletes. A batch without upserts or deletes makes no such call.
    #[tokio::test]
    async fn mirror_writer_applies_batches_in_publication_order() {
        let mirror = ScriptedMirror::new(0, false);
        let writer = MirrorWriter::spawn(Arc::clone(&mirror) as Arc<dyn MirrorBackend>);
        let mut only_deletes = batch("rrs-c", 5);
        only_deletes.upserts.clear();
        writer.publish(vec![batch("rrs-b", 1), batch("rrs-a", 3)]);
        writer.publish(vec![only_deletes]);
        writer.flush().await;
        assert_eq!(mirror.calls(), vec![
            "ensure rrs-b cosine",
            "upsert rrs-b [1]",
            "delete rrs-b [2]",
            "ensure rrs-a cosine",
            "upsert rrs-a [3]",
            "delete rrs-a [4]",
            "ensure rrs-c cosine",
            "delete rrs-c [6]",
        ]);
        // Retired by decision 114: the writer keeps no dirty marks.
        // assert!(writer.dirty().is_empty());
    }

    /// The attempts of a batch, as the design fixes them. The tests count with this literal, not
    /// with [`MirrorWriter::ATTEMPTS`], so a change of the constant fails them.
    const DESIGNED_ATTEMPTS: u64 = 3;

    #[test]
    fn mirror_writer_makes_the_designed_attempts() {
        assert_eq!(u64::from(MirrorWriter::ATTEMPTS), DESIGNED_ATTEMPTS);
    }

    /// A batch that fails is applied again from its start, and a batch that succeeds at its last
    /// attempt is applied whole. Changed by decision 114: no dirty mark remains to check.
    #[tokio::test]
    async fn mirror_writer_retries_a_failed_batch() {
        let refusals = DESIGNED_ATTEMPTS - 1;
        let mirror = ScriptedMirror::new(refusals, false);
        let writer = MirrorWriter::spawn(Arc::clone(&mirror) as Arc<dyn MirrorBackend>);
        writer.publish(vec![batch("rrs-a", 1)]);
        writer.flush().await;
        let mut expected = vec!["ensure rrs-a cosine"; usize::try_from(refusals).expect("a count")];
        expected.extend([
            "ensure rrs-a cosine",
            "upsert rrs-a [1]",
            "delete rrs-a [2]",
        ]);
        assert_eq!(mirror.calls(), expected);
        // Retired by decision 114: the writer keeps no dirty marks.
        // assert!(writer.dirty().is_empty());
    }

    /// A batch that still fails at its last attempt is dropped (decision 114), and the next batch
    /// is applied. Before, the failed batch marked its collection dirty.
    #[tokio::test]
    // Changed by decision 114: a batch that fails after its attempts is dropped.
    // async fn mirror_writer_marks_a_failed_batch_dirty() {
    async fn mirror_writer_drops_a_failed_batch() {
        let mirror = ScriptedMirror::new(DESIGNED_ATTEMPTS, false);
        let writer = MirrorWriter::spawn(Arc::clone(&mirror) as Arc<dyn MirrorBackend>);
        writer.publish(vec![batch("rrs-a", 1), batch("rrs-b", 3)]);
        writer.flush().await;
        let mut expected =
            vec!["ensure rrs-a cosine"; usize::try_from(DESIGNED_ATTEMPTS).expect("a count")];
        expected.extend([
            "ensure rrs-b cosine",
            "upsert rrs-b [3]",
            "delete rrs-b [4]",
        ]);
        assert_eq!(mirror.calls(), expected);
        // Retired by decision 114: the writer keeps no dirty marks.
        // assert_eq!(writer.dirty(), BTreeSet::from(["rrs-a".to_string()]));
        // assert_eq!(
        //     writer.take_dirty().into_keys().collect::<BTreeSet<_>>(),
        //     BTreeSet::from(["rrs-a".to_string()])
        // );
        // assert!(
        //     writer.dirty().is_empty(),
        //     "take_dirty removes the collections"
        // );
    }

    // Retired by decision 114: a batch registers no channels.
    // /// A batch with channels registers them after it makes the collection and before its
    // /// upserts (P6 step 7c).
    // #[tokio::test]
    // async fn mirror_writer_registers_the_channels_of_a_batch() {
    //     let mirror = ScriptedMirror::new(0, false);
    //     let writer = MirrorWriter::spawn(Arc::clone(&mirror) as Arc<dyn MirrorBackend>);
    //     writer.publish(vec![batch_on("rrs-a", 1, &[3, 4])]);
    //     writer.flush().await;
    //     assert_eq!(mirror.calls(), vec![
    //         "ensure rrs-a cosine",
    //         "register rrs-a 9 [\"d3\", \"d4\"]",
    //         "upsert rrs-a [1]",
    //         "delete rrs-a [2]",
    //     ]);
    // }

    // Retired by decision 114: nothing repairs the mirror, so a dirty collection keeps nothing.
    // /// A dirty collection keeps what its repair needs: the space, the distance and the union of
    // /// the channels of every batch that the writer did not apply (P6 step 7c). A repair that does
    // /// not finish marks the collection dirty again.
    // #[tokio::test]
    // async fn mirror_writer_keeps_what_a_repair_needs() {
    //     let mirror = ScriptedMirror::new(2 * DESIGNED_ATTEMPTS, false);
    //     let writer = MirrorWriter::spawn(Arc::clone(&mirror) as Arc<dyn MirrorBackend>);
    //     writer.publish(vec![batch_on("rrs-a", 1, &[3]), batch_on("rrs-a", 5, &[4])]);
    //     writer.flush().await;
    //     let expected = DirtyCollection {
    //         space: [9; SID_LEN],
    //         metric: MirrorMetric::Cosine,
    //         channels: BTreeMap::from([("d3".to_string(), vec![3]), ("d4".to_string(), vec![4])]),
    //     };
    //     let dirty = writer.take_dirty();
    //     assert_eq!(
    //         dirty,
    //         BTreeMap::from([("rrs-a".to_string(), expected.clone())])
    //     );
    //     writer.mark_dirty("rrs-a".to_string(), expected.clone());
    //     assert_eq!(
    //         writer.take_dirty(),
    //         BTreeMap::from([("rrs-a".to_string(), expected)])
    //     );
    // }

    // Retired by decision 114: no reconciler reads a registry of channels.
    // /// The registry of a collection reads the space id, the registered channels and the
    // /// watermark from its metadata. Other keys are ignored, and a malformed registry key fails.
    // #[test]
    // fn vectordb_services_registry_of_reads_the_metadata() {
    //     let space = hex::encode([7; SID_LEN]);
    //     let key = hex::encode([1, 2, 3]);
    //     let channel = format!("{REGISTERED_CHANNEL_PREFIX}abc");
    //     let registry = registry_of([
    //         (SPACE_ID_METADATA, RegistryValue::Text(&space)),
    //         (channel.as_str(), RegistryValue::Text(&key)),
    //         (WATERMARK_METADATA, RegistryValue::Int(12)),
    //         (SPACE_METADATA, RegistryValue::Text("cosine")),
    //         ("other", RegistryValue::Other),
    //     ])
    //     .expect("a registry");
    //     assert_eq!(registry, MirrorRegistry {
    //         space: Some([7; SID_LEN]),
    //         channels: BTreeMap::from([("abc".to_string(), vec![1, 2, 3])]),
    //         watermark: 12,
    //     });
    //     assert_eq!(registry_of([]), Ok(MirrorRegistry::default()));
    //     for (key, value) in [
    //         (SPACE_ID_METADATA, RegistryValue::Text("not hex")),
    //         (SPACE_ID_METADATA, RegistryValue::Text("0102")),
    //         (SPACE_ID_METADATA, RegistryValue::Int(1)),
    //         (WATERMARK_METADATA, RegistryValue::Int(-1)),
    //         (WATERMARK_METADATA, RegistryValue::Text("1")),
    //         (channel.as_str(), RegistryValue::Text("not hex")),
    //     ] {
    //         assert_eq!(
    //             registry_of([(key, value)]),
    //             Err(MirrorFailure::Unavailable),
    //             "{key} {value:?}"
    //         );
    //     }
    // }

    /// Publication never waits: with a stalled mirror, a batch that finds the queue full is
    /// dropped at once (decision 114). Before, it marked its collection dirty. The task runs only
    /// when the test awaits, so the first batch fills the queue of one batch.
    #[tokio::test]
    // Changed by decision 114: a batch that finds the queue full is dropped.
    // async fn mirror_writer_marks_a_full_queue_dirty() {
    async fn mirror_writer_drops_a_batch_that_finds_the_queue_full() {
        let mirror = ScriptedMirror::new(0, true);
        let writer = MirrorWriter::spawn_with(Arc::clone(&mirror) as Arc<dyn MirrorBackend>, 1);
        writer.publish(vec![batch("rrs-a", 1), batch("rrs-b", 3)]);
        writer.publish(vec![batch("rrs-c", 5)]);
        // Retired by decision 114: the writer keeps no dirty marks.
        // assert_eq!(
        //     writer.dirty(),
        //     BTreeSet::from(["rrs-b".to_string(), "rrs-c".to_string()])
        // );
        mirror.open();
        writer.flush().await;
        assert_eq!(mirror.calls(), vec![
            "ensure rrs-a cosine",
            "upsert rrs-a [1]",
            "delete rrs-a [2]",
        ]);
    }

    use proptest::prelude::*;

    /// The id byte that makes [`PoisonMirror`] refuse an upsert.
    const POISON: u8 = 0xFF;

    /// A mirror that holds the ids of each collection (by their first byte), that refuses every
    /// upsert of a record with the id byte [`POISON`], and that waits at a gate until the test
    /// opens it.
    struct PoisonMirror {
        held: Mutex<BTreeMap<String, BTreeSet<u8>>>,
        gate: tokio::sync::Semaphore,
    }

    impl PoisonMirror {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                held: Mutex::new(BTreeMap::new()),
                gate: tokio::sync::Semaphore::new(0),
            })
        }

        fn open(&self) {
            self.gate
                .add_permits(tokio::sync::Semaphore::MAX_PERMITS / 2);
        }

        async fn pass(&self) -> Result<(), MirrorFailure> {
            self.gate
                .acquire()
                .await
                .map(drop)
                .map_err(|_| MirrorFailure::Unavailable)
        }

        fn held(&self) -> BTreeMap<String, BTreeSet<u8>> {
            self.held
                .lock()
                .expect("the ids of the poison mirror")
                .clone()
        }
    }

    #[async_trait]
    impl MirrorBackend for PoisonMirror {
        async fn query(
            &self,
            _collection: &str,
            _channel: &str,
            _vector: &[f32],
            _k: usize,
        ) -> Result<Vec<Neighbour>, MirrorFailure> {
            Ok(Vec::new())
        }

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> { Ok(0) }

        async fn ensure_collection(
            &self,
            collection: &str,
            _metric: MirrorMetric,
        ) -> Result<(), MirrorFailure> {
            self.pass().await?;
            self.held
                .lock()
                .expect("the ids of the poison mirror")
                .entry(collection.to_string())
                .or_default();
            Ok(())
        }

        async fn upsert(
            &self,
            collection: &str,
            records: &[MirrorRecord],
        ) -> Result<(), MirrorFailure> {
            self.pass().await?;
            if records.iter().any(|record| record.id[0] == POISON) {
                return Err(MirrorFailure::Unavailable);
            }
            let mut held = self.held.lock().expect("the ids of the poison mirror");
            let ids = held.entry(collection.to_string()).or_default();
            ids.extend(records.iter().map(|record| record.id[0]));
            Ok(())
        }

        async fn delete(
            &self,
            collection: &str,
            ids: &[[u8; ID_LEN]],
        ) -> Result<(), MirrorFailure> {
            self.pass().await?;
            let mut held = self.held.lock().expect("the ids of the poison mirror");
            let entry = held.entry(collection.to_string()).or_default();
            for id in ids {
                entry.remove(&id[0]);
            }
            Ok(())
        }

        // Retired by decision 114: a batch registers no channels.
        // async fn register(
        //     &self,
        //     _collection: &str,
        //     _space: &SpaceId,
        //     _channels: &BTreeMap<String, Vec<u8>>,
        // ) -> Result<(), MirrorFailure> {
        //     self.pass().await
        // }

        async fn collections(&self) -> Result<Vec<String>, MirrorFailure> { Ok(Vec::new()) }

        // Retired by decision 114: no reconciler reads a registry of channels.
        // async fn registry(&self, _collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
        //     Ok(MirrorRegistry::default())
        // }

        async fn ids(
            &self,
            _collection: &str,
            _channel: &str,
        ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
            Ok(BTreeSet::new())
        }

        // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
        // async fn set_watermark(
        //     &self,
        //     _collection: &str,
        //     _watermark: u64,
        // ) -> Result<(), MirrorFailure> {
        //     Ok(())
        // }
    }

    /// A generated batch: its collection, the ids that it upserts and deletes, and whether the
    /// mirror refuses it (a poisoned batch carries the id [`POISON`]).
    fn generated_batch() -> impl Strategy<Value = (u8, Vec<u8>, Vec<u8>, bool)> {
        (
            0u8..3,
            proptest::collection::vec(1u8..6, 0..3),
            proptest::collection::vec(1u8..6, 0..3),
            proptest::bool::weighted(0.15),
        )
    }

    fn property_config() -> ProptestConfig {
        ProptestConfig {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x0F6E_5EED_2026_1009),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(property_config())]

        /// The writer applies the batches that its queue takes, in publication order, and it
        /// drops each batch that the queue refuses and each batch that fails after its attempts
        /// (ChromaMirror.tla, PublishNeverWaits, decision 114). Before, it marked their
        /// collections dirty. Publication never waits: the mirror is stalled while the program
        /// publishes, so a queue of `capacity` batches takes the first `capacity` batches and
        /// refuses the others.
        #[test]
        fn prop_mirror_writer_applies_the_accepted_batches(
            capacity in 1usize..4,
            generated in proptest::collection::vec(generated_batch(), 0..7),
        ) {
            let poisoned = generated.iter().filter(|(_, _, _, poison)| *poison).count();
            prop_assume!(poisoned <= 2);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("a runtime");
            let batches: Vec<MirrorBatch> = generated
                .iter()
                .map(|(collection, upserts, deletes, poison)| {
                    let record = |byte: u8| MirrorRecord {
                        id: [byte; ID_LEN],
                        embedding: vec![1.0, 0.0],
                        channel: "c".to_string(),
                        persistent: false,
                        document: None,
                    };
                    let mut upserts: Vec<MirrorRecord> = upserts.iter().copied().map(record).collect();
                    if *poison {
                        upserts.push(record(POISON));
                    }
                    MirrorBatch {
                        space: [9; SID_LEN],
                        collection: format!("rrs-{collection}"),
                        metric: MirrorMetric::Cosine,
                        upserts,
                        deletes: deletes.iter().map(|byte| [*byte; ID_LEN]).collect(),
                        // Retired by decision 114: a batch registers no channels.
                        // channels: BTreeMap::new(),
                    }
                })
                .collect();
            // Changed by decision 114: the writer keeps no dirty marks.
            // let (held, dirty) = runtime.block_on(async {
            let held = runtime.block_on(async {
                let mirror = PoisonMirror::new();
                let writer =
                    MirrorWriter::spawn_with(Arc::clone(&mirror) as Arc<dyn MirrorBackend>, capacity);
                writer.publish(batches.clone());
                mirror.open();
                writer.flush().await;
                // (mirror.held(), writer.dirty())
                mirror.held()
            });
            let mut expected_held: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
            // Retired by decision 114: the writer keeps no dirty marks.
            // let mut expected_dirty: BTreeSet<String> = BTreeSet::new();
            for (index, (batch, (_, _, _, poison))) in batches.iter().zip(&generated).enumerate() {
                match (index < capacity, poison) {
                    (false, _) | (true, true) => {
                        // Retired by decision 114: the writer keeps no dirty marks.
                        // expected_dirty.insert(batch.collection.clone());
                        if index < capacity {
                            expected_held.entry(batch.collection.clone()).or_default();
                        }
                    }
                    (true, false) => {
                        let ids = expected_held.entry(batch.collection.clone()).or_default();
                        ids.extend(batch.upserts.iter().map(|record| record.id[0]));
                        for id in &batch.deletes {
                            ids.remove(&id[0]);
                        }
                    }
                }
            }
            prop_assert_eq!(held, expected_held);
            // Retired by decision 114: the writer keeps no dirty marks.
            // prop_assert_eq!(dirty, expected_dirty);
        }
    }
}
