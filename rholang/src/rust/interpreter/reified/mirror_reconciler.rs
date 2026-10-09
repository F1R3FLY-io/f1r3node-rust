//! The reconciler of the ChromaDB mirror of the VectorDB spaces (P6 step 7c, design v3 4.9;
//! register decisions 58 and 61, plan decisions 66 and 70).
//!
//! Each node publishes the net writes of its deploys to its mirror optimistically (step 7b). The
//! reconciler repairs the mirror from the finalized state: on each `BlockFinalised` event of the
//! node, it reads the records of every channel that the journal of the node registered, at the
//! post-state of the last finalized block, and it makes the mirror hold exactly those records.
//!
//! * A record that the mirror holds and the state lacks is an orphan: a fork, a merge rejection
//!   or a later revert wrote it. The pass deletes it.
//! * A record that the state holds and the mirror lacks is missing: a full queue, a failed batch
//!   or a later removal took it out. The pass adds it.
//! * After the last channel of a collection, the pass sets the watermark of the collection to
//!   the number of the finalized block. `rho:vectordb:ann` reports the watermark with each
//!   answer.
//!
//! The reconciler reads Casper state only through read-only calls that the node passes in
//! ([`FinalizedState`]): the post-state root of a block, and a runtime at a root. It writes only
//! the mirror. The mirror never feeds consensus: `ann` answers are recorded, and the exact
//! engine decides with `ids=`.
//!
//! A pass repairs a collection when its watermark is below the finalized block, or when the
//! writer marked it dirty. A failed call ends the repair of the collection without a new
//! watermark, so the next pass repeats it. The events coalesce: the loop always takes the newest
//! finalized block, and one pass runs at a time.
//!
//! Limits, stated in the documents: the reconciler repairs only the registered channels. After
//! an LFS bootstrap, the records of channels that this node never journaled are absent until the
//! channels change. After a restart, a batch that failed before the restart is repaired only
//! when its channels were registered before the failure or change again.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::shared::f1r3fly_event::F1r3flyEvent;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tokio::sync::watch;

use super::vectordb_services::{
    self, channel_digest, DirtyCollection, MirrorBackend, MirrorFailure, MirrorRegistry,
    VectorDbServices,
};
use crate::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use crate::rust::interpreter::spaces::disciplines::vectordb::ID_LEN;
use crate::rust::interpreter::spaces::mirror::{record_of_datum, MirrorRecord};
use crate::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, NativeSpace, SpaceId,
};

/// A finalized block: its number and the root of its post-state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finalized {
    pub block_number: u64,
    pub root: Blake2b256Hash,
}

/// The finalized state of a node, as its reconciler reads it. The node implements it with
/// read-only calls: its block store and a runtime of its runtime manager.
#[async_trait]
pub trait FinalizedState: Send + Sync {
    /// The post-state root of the block `block_hash`, or `None` when the node does not hold the
    /// block.
    async fn post_state(&self, block_hash: &[u8]) -> Result<Option<Blake2b256Hash>, String>;

    /// A runtime at the state `root`.
    async fn runtime_at(&self, root: &Blake2b256Hash) -> Result<RhoRuntimeImpl, String>;
}

/// A [`FinalizedState`] of two closures, so a node passes its read-only calls without a type of
/// its own: `post_state` maps a block hash to the post-state root of the block, and `runtime_at`
/// gives a runtime at a root.
pub struct ClosureState<P, R> {
    post_state: P,
    runtime_at: R,
}

impl<P, R> ClosureState<P, R> {
    /// The finalized state of the closures `post_state` and `runtime_at`.
    pub fn new(post_state: P, runtime_at: R) -> Self {
        Self {
            post_state,
            runtime_at,
        }
    }
}

#[async_trait]
impl<P, PF, R, RF> FinalizedState for ClosureState<P, R>
where
    P: Fn(Vec<u8>) -> PF + Send + Sync,
    PF: Future<Output = Result<Option<Blake2b256Hash>, String>> + Send,
    R: Fn(Blake2b256Hash) -> RF + Send + Sync,
    RF: Future<Output = Result<RhoRuntimeImpl, String>> + Send,
{
    async fn post_state(&self, block_hash: &[u8]) -> Result<Option<Blake2b256Hash>, String> {
        (self.post_state)(block_hash.to_vec()).await
    }

    async fn runtime_at(&self, root: &Blake2b256Hash) -> Result<RhoRuntimeImpl, String> {
        (self.runtime_at)(root.clone()).await
    }
}

/// The result of one pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PassReport {
    /// The collections that the pass repaired, and whose watermark it set.
    pub repaired: BTreeSet<String>,
    /// The collections at the watermark of the pass, which it left as they are.
    pub current: BTreeSet<String>,
    /// The collections whose repair failed. The next pass repeats them.
    pub failed: BTreeSet<String>,
    /// The orphan records that the pass deleted.
    pub deleted: usize,
    /// The missing records that the pass added.
    pub added: usize,
}

/// Why the repair of one collection stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RepairError {
    /// A call of the mirror failed.
    Mirror,
    /// The state could not be read, or the registry names a channel that it cannot hold.
    State(String),
}

impl From<MirrorFailure> for RepairError {
    fn from(_: MirrorFailure) -> Self { RepairError::Mirror }
}

/// One pass of the reconciler for the finalized block `finalized`, over the services
/// `services`. The pass takes the dirty collections of the writer, makes and registers them,
/// and then repairs every collection whose watermark is below the block, or that is dirty.
pub async fn reconcile(
    services: &VectorDbServices,
    state: &dyn FinalizedState,
    finalized: &Finalized,
) -> PassReport {
    let mirror = services.mirror.as_ref();
    let mut report = PassReport::default();
    let dirty = match &services.writer {
        Some(writer) => writer.take_dirty(),
        None => BTreeMap::new(),
    };
    let mut unprepared: BTreeSet<String> = BTreeSet::new();
    for (collection, entry) in &dirty {
        if prepare_dirty(mirror, collection, entry).await.is_err() {
            unprepared.insert(collection.to_string());
        }
    }
    let collections = match mirror.collections().await {
        Ok(collections) => collections,
        Err(_) => {
            remark(services, &dirty, dirty.keys());
            report.failed.extend(dirty.keys().cloned());
            return report;
        }
    };
    let mut runtime = None;
    for collection in collections {
        if unprepared.contains(&collection) {
            report.failed.insert(collection);
            continue;
        }
        let repaired = match mirror.registry(&collection).await {
            Ok(registry)
                if registry.watermark >= finalized.block_number
                    && !dirty.contains_key(&collection) =>
            {
                report.current.insert(collection);
                continue;
            }
            Ok(registry) => {
                repair(
                    mirror,
                    state,
                    &mut runtime,
                    &collection,
                    &registry,
                    finalized,
                )
                .await
            }
            Err(failure) => Err(RepairError::from(failure)),
        };
        match repaired {
            Ok((deleted, added)) => {
                report.deleted += deleted;
                report.added += added;
                report.repaired.insert(collection);
            }
            Err(error) => {
                tracing::debug!(
                    collection = %collection,
                    error = ?error,
                    "the mirror reconciler did not finish a collection"
                );
                report.failed.insert(collection);
            }
        }
    }
    for collection in &unprepared {
        report.failed.insert(collection.clone());
    }
    remark(services, &dirty, report.failed.iter());
    report
}

/// Makes the dirty collection `collection` and registers the channels of `entry`, so a repair
/// knows them.
async fn prepare_dirty(
    mirror: &dyn MirrorBackend,
    collection: &str,
    entry: &DirtyCollection,
) -> Result<(), MirrorFailure> {
    mirror.ensure_collection(collection, entry.metric).await?;
    match entry.channels.is_empty() {
        true => Ok(()),
        false => {
            mirror
                .register(collection, &entry.space, &entry.channels)
                .await
        }
    }
}

/// Marks each collection of `collections` that was dirty dirty again, after a pass that did not
/// repair it. A collection that was not dirty needs no mark: its watermark stays below the next
/// finalized block.
fn remark<'c>(
    services: &VectorDbServices,
    dirty: &BTreeMap<String, DirtyCollection>,
    collections: impl Iterator<Item = &'c String>,
) {
    let Some(writer) = &services.writer else {
        return;
    };
    for collection in collections {
        if let Some(entry) = dirty.get(collection) {
            writer.mark_dirty(collection.clone(), entry.clone());
        }
    }
}

/// Repairs the collection `collection` of the registry `registry` to the state of `finalized`,
/// and returns the numbers of the deleted and of the added records. The runtime at the root of
/// the block is made on the first repair of the pass.
async fn repair(
    mirror: &dyn MirrorBackend,
    state: &dyn FinalizedState,
    runtime: &mut Option<RhoRuntimeImpl>,
    collection: &str,
    registry: &MirrorRegistry,
    finalized: &Finalized,
) -> Result<(usize, usize), RepairError> {
    let mut deleted = 0;
    let mut added = 0;
    if let (Some(space), false) = (registry.space, registry.channels.is_empty()) {
        if runtime.is_none() {
            *runtime = Some(
                state
                    .runtime_at(&finalized.root)
                    .await
                    .map_err(RepairError::State)?,
            );
        }
        let Some(runtime) = runtime.as_ref() else {
            return Err(RepairError::State(
                "the runtime of the pass is missing".to_string(),
            ));
        };
        for (digest, key) in &registry.channels {
            let (orphans, missing) =
                channel_repair(mirror, runtime, collection, space, digest, key).await?;
            if !orphans.is_empty() {
                mirror.delete(collection, &orphans).await?;
            }
            if !missing.is_empty() {
                mirror.upsert(collection, &missing).await?;
            }
            deleted += orphans.len();
            added += missing.len();
        }
    }
    // The watermark never decreases: a dirty collection may be repaired at an older block than a
    // pass already applied.
    mirror
        .set_watermark(collection, finalized.block_number.max(registry.watermark))
        .await?;
    Ok((deleted, added))
}

/// The orphan ids and the missing records of the registered channel `digest` (key bytes `key`)
/// of the space `space` in `collection`, at the state of `runtime`.
async fn channel_repair(
    mirror: &dyn MirrorBackend,
    runtime: &RhoRuntimeImpl,
    collection: &str,
    space: SpaceId,
    digest: &str,
    key: &[u8],
) -> Result<(Vec<[u8; ID_LEN]>, Vec<MirrorRecord>), RepairError> {
    let key = CollectionKey::from_bytes(key.to_vec())
        .ok_or_else(|| RepairError::State(format!("the channel {digest} has no key")))?;
    if channel_digest(&key) != digest {
        return Err(RepairError::State(format!(
            "the key of the channel {digest} has another digest"
        )));
    }
    // A space with a mirror has the default qualifier (gate 9), so its channels are durable.
    let channel = namespace::gprivate(namespace::data_channel(
        NativeSpace {
            sid: space,
            durability: Durability::Durable,
        },
        &key,
    ));
    let mut expected: BTreeMap<[u8; ID_LEN], MirrorRecord> = BTreeMap::new();
    for datum in runtime.get_data(&channel).await {
        let Ok(id) = <[u8; ID_LEN]>::try_from(datum.source.hash.bytes().as_slice()) else {
            continue;
        };
        if let Some(record) = record_of_datum(id, &datum.a, digest, datum.persist) {
            expected.insert(id, record);
        }
    }
    let held = mirror.ids(collection, digest).await?;
    let orphans = held
        .iter()
        .filter(|id| !expected.contains_key(*id))
        .copied()
        .collect();
    let missing = expected
        .into_iter()
        .filter(|(id, _)| !held.contains(id))
        .map(|(_, record)| record)
        .collect();
    Ok((orphans, missing))
}

/// Runs passes over the finalized blocks of `finalized`, newest first: a pass that starts takes
/// the newest block of the channel, so the blocks that arrive during a pass coalesce.
pub async fn run(
    mut finalized: watch::Receiver<Option<Finalized>>,
    state: Arc<dyn FinalizedState>,
) {
    while finalized.changed().await.is_ok() {
        let Some(block) = finalized.borrow_and_update().clone() else {
            continue;
        };
        let services = vectordb_services::current();
        let report = reconcile(&services, state.as_ref(), &block).await;
        tracing::debug!(
            block_number = block.block_number,
            repaired = report.repaired.len(),
            current = report.current.len(),
            failed = report.failed.len(),
            deleted = report.deleted,
            added = report.added,
            "mirror reconciler pass"
        );
    }
}

/// Starts the VectorDB services of the node and its mirror reconciler (decision 61). It installs
/// `node_services()` and then calls [`start`]. A node without the `chromadb` feature, or without
/// a ChromaDB endpoint, has no writer and starts no task. It needs a Tokio runtime.
pub fn spawn(events: &F1r3flyEvents, state: Arc<dyn FinalizedState>) {
    vectordb_services::install(vectordb_services::node_services());
    start(events, state);
}

/// Starts the mirror reconciler over the installed services, when they have a mirror writer: one
/// task turns the `BlockFinalised` events of `events` into finalized blocks through `state`, and
/// one task runs the passes. A newer block replaces an older block that no pass took yet.
pub fn start(events: &F1r3flyEvents, state: Arc<dyn FinalizedState>) {
    if vectordb_services::current().writer.is_none() {
        return;
    }
    let (sender, receiver) = watch::channel::<Option<Finalized>>(None);
    let mut stream = events.consume();
    let lookup = Arc::clone(&state);
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let F1r3flyEvent::BlockFinalised(block) = event else {
                continue;
            };
            let (Ok(hash), Ok(block_number)) = (
                hex::decode(&block.block_hash),
                u64::try_from(block.block_number),
            ) else {
                continue;
            };
            if let Ok(Some(root)) = lookup.post_state(&hash).await {
                let newer = sender
                    .borrow()
                    .as_ref()
                    .is_none_or(|known| block_number > known.block_number);
                if newer && sender.send(Some(Finalized { block_number, root })).is_err() {
                    return;
                }
            }
        }
    });
    tokio::spawn(run(receiver, state));
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::rust::interpreter::reified::vectordb_services::{
        MirrorWriter, Neighbour, UnavailableEmbedder,
    };
    use crate::rust::interpreter::spaces::mirror::MirrorMetric;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;

    /// A mirror with fixed registries. It logs every write call, and it refuses the kinds of
    /// call that `refuse` names.
    struct RegistryMirror {
        registries: BTreeMap<String, MirrorRegistry>,
        refuse: BTreeSet<&'static str>,
        calls: Mutex<Vec<String>>,
    }

    impl RegistryMirror {
        fn new(registries: Vec<(&str, MirrorRegistry)>, refuse: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                registries: registries
                    .into_iter()
                    .map(|(name, registry)| (name.to_string(), registry))
                    .collect(),
                refuse: refuse.iter().copied().collect(),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn call(&self, kind: &'static str, text: String) -> Result<(), MirrorFailure> {
            self.calls
                .lock()
                .expect("the call log of the registry mirror")
                .push(text);
            match self.refuse.contains(kind) {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("the call log of the registry mirror")
                .clone()
        }
    }

    #[async_trait]
    impl MirrorBackend for RegistryMirror {
        async fn query(
            &self,
            _collection: &str,
            _channel: &str,
            _vector: &[f32],
            _k: usize,
        ) -> Result<Vec<Neighbour>, MirrorFailure> {
            Ok(Vec::new())
        }

        async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> { Ok(0) }

        async fn ensure_collection(
            &self,
            collection: &str,
            metric: MirrorMetric,
        ) -> Result<(), MirrorFailure> {
            self.call("ensure", format!("ensure {collection} {}", metric.space()))
        }

        async fn upsert(
            &self,
            collection: &str,
            records: &[MirrorRecord],
        ) -> Result<(), MirrorFailure> {
            self.call("upsert", format!("upsert {collection} {}", records.len()))
        }

        async fn delete(
            &self,
            collection: &str,
            ids: &[[u8; ID_LEN]],
        ) -> Result<(), MirrorFailure> {
            self.call("delete", format!("delete {collection} {}", ids.len()))
        }

        async fn register(
            &self,
            collection: &str,
            _space: &SpaceId,
            channels: &BTreeMap<String, Vec<u8>>,
        ) -> Result<(), MirrorFailure> {
            self.call(
                "register",
                format!("register {collection} {}", channels.len()),
            )
        }

        async fn collections(&self) -> Result<Vec<String>, MirrorFailure> {
            match self.refuse.contains("collections") {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(self.registries.keys().cloned().collect()),
            }
        }

        async fn registry(&self, collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
            match self.refuse.contains("registry") {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(self.registries.get(collection).cloned().unwrap_or_default()),
            }
        }

        async fn ids(
            &self,
            _collection: &str,
            _channel: &str,
        ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
            Ok(BTreeSet::new())
        }

        async fn set_watermark(
            &self,
            collection: &str,
            watermark: u64,
        ) -> Result<(), MirrorFailure> {
            self.call("watermark", format!("watermark {collection} {watermark}"))
        }
    }

    /// A finalized state that gives no runtime.
    struct NoRuntime;

    #[async_trait]
    impl FinalizedState for NoRuntime {
        async fn post_state(&self, _block_hash: &[u8]) -> Result<Option<Blake2b256Hash>, String> {
            Ok(None)
        }

        async fn runtime_at(&self, _root: &Blake2b256Hash) -> Result<RhoRuntimeImpl, String> {
            Err("no runtime".to_string())
        }
    }

    fn services(mirror: &Arc<RegistryMirror>) -> VectorDbServices {
        let backend = Arc::clone(mirror) as Arc<dyn MirrorBackend>;
        VectorDbServices {
            embedder: Arc::new(UnavailableEmbedder),
            mirror: Arc::clone(&backend),
            writer: Some(MirrorWriter::spawn(backend)),
        }
    }

    fn block(number: u64) -> Finalized {
        Finalized {
            block_number: number,
            root: Blake2b256Hash::new(b"root"),
        }
    }

    /// A registry at the watermark `watermark`, with one channel when `channel` holds.
    fn registry(watermark: u64, channel: bool) -> MirrorRegistry {
        MirrorRegistry {
            space: Some([1; SID_LEN]),
            channels: match channel {
                true => BTreeMap::from([("d".to_string(), vec![1])]),
                false => BTreeMap::new(),
            },
            watermark,
        }
    }

    fn dirty_entry() -> DirtyCollection {
        DirtyCollection {
            space: [1; SID_LEN],
            metric: MirrorMetric::Cosine,
            channels: BTreeMap::new(),
        }
    }

    fn names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[tokio::test]
    async fn mirror_reconciler_skips_a_collection_at_its_watermark() {
        let mirror = RegistryMirror::new(vec![("rrs-a", registry(5, true))], &[]);
        let report = reconcile(&services(&mirror), &NoRuntime, &block(5)).await;
        assert_eq!(report.current, names(&["rrs-a"]));
        assert_eq!(mirror.calls(), Vec::<String>::new());
    }

    /// A collection without registered channels needs no runtime, and its watermark advances.
    #[tokio::test]
    async fn mirror_reconciler_sets_the_watermark_of_a_collection_without_channels() {
        let mirror = RegistryMirror::new(vec![("rrs-a", registry(0, false))], &[]);
        let report = reconcile(&services(&mirror), &NoRuntime, &block(4)).await;
        assert_eq!(report.repaired, names(&["rrs-a"]));
        assert_eq!(mirror.calls(), vec!["watermark rrs-a 4"]);
    }

    /// Without a runtime at the root, the repair stops before the watermark.
    #[tokio::test]
    async fn mirror_reconciler_keeps_the_watermark_without_a_runtime() {
        let mirror = RegistryMirror::new(vec![("rrs-a", registry(0, true))], &[]);
        let report = reconcile(&services(&mirror), &NoRuntime, &block(4)).await;
        assert_eq!(report.failed, names(&["rrs-a"]));
        assert_eq!(mirror.calls(), Vec::<String>::new());
    }

    /// A refused listing fails the pass, and every dirty collection stays dirty.
    #[tokio::test]
    async fn mirror_reconciler_keeps_the_dirty_marks_of_a_refused_listing() {
        let mirror = RegistryMirror::new(vec![("rrs-a", registry(0, false))], &["collections"]);
        let services = services(&mirror);
        let writer = services.writer.clone().expect("a writer");
        writer.mark_dirty("rrs-b".to_string(), dirty_entry());
        let report = reconcile(&services, &NoRuntime, &block(4)).await;
        assert_eq!(report.failed, names(&["rrs-b"]));
        assert_eq!(writer.dirty(), names(&["rrs-b"]));
    }

    /// A dirty collection that the pass cannot make stays dirty, and the other collections are
    /// repaired.
    #[tokio::test]
    async fn mirror_reconciler_keeps_the_dirty_mark_of_a_collection_that_it_cannot_make() {
        let mirror = RegistryMirror::new(
            vec![("rrs-a", registry(0, false)), ("rrs-b", registry(0, false))],
            &["ensure"],
        );
        let services = services(&mirror);
        let writer = services.writer.clone().expect("a writer");
        writer.mark_dirty("rrs-b".to_string(), dirty_entry());
        let report = reconcile(&services, &NoRuntime, &block(4)).await;
        assert_eq!(report.failed, names(&["rrs-b"]));
        assert_eq!(report.repaired, names(&["rrs-a"]));
        assert_eq!(writer.dirty(), names(&["rrs-b"]));
    }

    /// A dirty collection is repaired even at its watermark, and the watermark never decreases.
    #[tokio::test]
    async fn mirror_reconciler_repairs_a_dirty_collection_at_its_watermark() {
        let mirror = RegistryMirror::new(vec![("rrs-a", registry(9, false))], &[]);
        let services = services(&mirror);
        let writer = services.writer.clone().expect("a writer");
        writer.mark_dirty("rrs-a".to_string(), dirty_entry());
        let report = reconcile(&services, &NoRuntime, &block(5)).await;
        assert_eq!(report.repaired, names(&["rrs-a"]));
        assert_eq!(mirror.calls(), vec![
            "ensure rrs-a cosine",
            "watermark rrs-a 9"
        ]);
        assert!(writer.dirty().is_empty());
    }

    use proptest::prelude::*;

    /// A mirror whose registries change with each watermark that it sets, and whose refusals
    /// the test chooses before each pass: a set of (collection, kind of call).
    struct PassMirror {
        registries: Mutex<BTreeMap<String, MirrorRegistry>>,
        refuse: Mutex<BTreeSet<(String, &'static str)>>,
        listing_refused: Mutex<bool>,
    }

    impl PassMirror {
        fn refused(&self, collection: &str, kind: &'static str) -> Result<(), MirrorFailure> {
            match self
                .refuse
                .lock()
                .expect("the refusals of the pass mirror")
                .contains(&(collection.to_string(), kind))
            {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(()),
            }
        }

        fn watermarks(&self) -> BTreeMap<String, u64> {
            self.registries
                .lock()
                .expect("the registries of the pass mirror")
                .iter()
                .map(|(name, registry)| (name.clone(), registry.watermark))
                .collect()
        }
    }

    #[async_trait]
    impl MirrorBackend for PassMirror {
        async fn query(
            &self,
            _collection: &str,
            _channel: &str,
            _vector: &[f32],
            _k: usize,
        ) -> Result<Vec<Neighbour>, MirrorFailure> {
            Ok(Vec::new())
        }

        async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> { Ok(0) }

        async fn ensure_collection(
            &self,
            collection: &str,
            _metric: MirrorMetric,
        ) -> Result<(), MirrorFailure> {
            self.refused(collection, "ensure")
        }

        async fn upsert(
            &self,
            collection: &str,
            _records: &[MirrorRecord],
        ) -> Result<(), MirrorFailure> {
            self.refused(collection, "upsert")
        }

        async fn delete(
            &self,
            collection: &str,
            _ids: &[[u8; ID_LEN]],
        ) -> Result<(), MirrorFailure> {
            self.refused(collection, "delete")
        }

        async fn register(
            &self,
            collection: &str,
            _space: &SpaceId,
            _channels: &BTreeMap<String, Vec<u8>>,
        ) -> Result<(), MirrorFailure> {
            self.refused(collection, "register")
        }

        async fn collections(&self) -> Result<Vec<String>, MirrorFailure> {
            match *self.listing_refused.lock().expect("the listing flag") {
                true => Err(MirrorFailure::Unavailable),
                false => Ok(self
                    .registries
                    .lock()
                    .expect("the registries of the pass mirror")
                    .keys()
                    .cloned()
                    .collect()),
            }
        }

        async fn registry(&self, collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
            self.refused(collection, "registry")?;
            Ok(self
                .registries
                .lock()
                .expect("the registries of the pass mirror")
                .get(collection)
                .cloned()
                .unwrap_or_default())
        }

        async fn ids(
            &self,
            collection: &str,
            _channel: &str,
        ) -> Result<BTreeSet<[u8; ID_LEN]>, MirrorFailure> {
            self.refused(collection, "ids")?;
            Ok(BTreeSet::new())
        }

        async fn set_watermark(
            &self,
            collection: &str,
            watermark: u64,
        ) -> Result<(), MirrorFailure> {
            self.refused(collection, "watermark")?;
            if let Some(registry) = self
                .registries
                .lock()
                .expect("the registries of the pass mirror")
                .get_mut(collection)
            {
                registry.watermark = watermark;
            }
            Ok(())
        }
    }

    /// A generated pass: the finalized block, the collections that the test marks dirty
    /// before it, the refused calls, and whether the listing of the collections fails.
    #[derive(Clone, Debug)]
    struct GeneratedPass {
        block: u64,
        dirty: BTreeSet<usize>,
        refuse: BTreeSet<(usize, &'static str)>,
        listing_refused: bool,
    }

    fn generated_pass(collections: usize) -> impl Strategy<Value = GeneratedPass> {
        (
            1u64..8,
            proptest::collection::btree_set(0..collections, 0..=collections),
            proptest::collection::btree_set(
                (
                    0..collections,
                    proptest::sample::select(vec!["ensure", "registry", "watermark"]),
                ),
                0..3,
            ),
            proptest::bool::weighted(0.1),
        )
            .prop_map(|(block, dirty, refuse, listing_refused)| GeneratedPass {
                block,
                dirty,
                refuse,
                listing_refused,
            })
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

        /// The watermark of a collection changes only through a repair that finishes, to the
        /// larger of the block and the old watermark, so it never decreases and never passes
        /// the block of the last finished repair (ChromaMirror.tla, WatermarkHonest). A failed
        /// call leaves the watermark, and a dirty collection that a pass does not finish stays
        /// dirty. Without a runtime, the repair of a collection with registered channels
        /// fails, and a collection without channels finishes.
        #[test]
        fn prop_mirror_reconciler_watermarks_follow_finished_repairs(
            (shapes, passes) in (2usize..4).prop_flat_map(|collections| (
                proptest::collection::vec((0u64..4, proptest::bool::ANY), collections),
                proptest::collection::vec(generated_pass(collections), 1..6),
            )),
        ) {
            let names: Vec<String> = (0..shapes.len()).map(|index| format!("rrs-{index}")).collect();
            let mirror = Arc::new(PassMirror {
                registries: Mutex::new(
                    names
                        .iter()
                        .zip(&shapes)
                        .map(|(name, (watermark, channel))| (name.clone(), registry(*watermark, *channel)))
                        .collect(),
                ),
                refuse: Mutex::new(BTreeSet::new()),
                listing_refused: Mutex::new(false),
            });
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("a runtime");
            let backend = Arc::clone(&mirror) as Arc<dyn MirrorBackend>;
            let (services, writer) = runtime.block_on(async {
                let writer = MirrorWriter::spawn(Arc::clone(&backend));
                (
                    VectorDbServices {
                        embedder: Arc::new(UnavailableEmbedder),
                        mirror: backend,
                        writer: Some(Arc::clone(&writer)),
                    },
                    writer,
                )
            });
            let mut watermarks: BTreeMap<String, u64> = mirror.watermarks();
            let mut dirty: BTreeSet<String> = BTreeSet::new();
            for pass in &passes {
                for index in &pass.dirty {
                    writer.mark_dirty(names[*index].clone(), dirty_entry());
                    dirty.insert(names[*index].clone());
                }
                *mirror.refuse.lock().expect("the refusals") = pass
                    .refuse
                    .iter()
                    .map(|(index, kind)| (names[*index].clone(), *kind))
                    .collect();
                *mirror.listing_refused.lock().expect("the listing flag") = pass.listing_refused;
                let before = mirror.watermarks();
                runtime.block_on(reconcile(&services, &NoRuntime, &block(pass.block)));
                let refused = |name: &String, kind: &'static str| {
                    pass.refuse.iter().any(|(index, refused)| names[*index] == *name && *refused == kind)
                };
                if !pass.listing_refused {
                    for (index, name) in names.iter().enumerate() {
                        let was_dirty = dirty.contains(name);
                        let finished = match (was_dirty && refused(name, "ensure"), refused(name, "registry")) {
                            (true, _) | (false, true) => false,
                            (false, false) => match (watermarks[name] >= pass.block && !was_dirty, shapes[index].1) {
                                (true, _) => {
                                    continue;
                                }
                                (false, true) => false,
                                (false, false) => !refused(name, "watermark"),
                            },
                        };
                        if finished {
                            let old = watermarks[name];
                            watermarks.insert(name.clone(), old.max(pass.block));
                            dirty.remove(name);
                        }
                    }
                }
                let now = mirror.watermarks();
                for name in &names {
                    prop_assert!(now[name] >= before[name], "the watermark of {} decreased", name);
                }
                prop_assert_eq!(now, watermarks.clone());
                prop_assert_eq!(writer.dirty(), dirty.clone());
            }
        }
    }
}
