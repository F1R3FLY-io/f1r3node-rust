//! The test backends of the recorded VectorDB processes `rho:vectordb:embed` and
//! `rho:vectordb:ann` (P6 step 7, design v3 4.9).
//!
//! The services of a node live in one registry of the process (`vectordb_services`). The tests of
//! one binary run on several threads, so a test that uses the registry holds [`ServicesLock`]
//! from its start to its end:
//!
//! * [`install`] takes the lock and installs services;
//! * [`without_services`] takes the lock and removes the services, so the processes see the
//!   unavailable services of a node without the `chromadb` feature;
//! * [`ServicesLock::replace`] swaps the services under the lock, for example between play and
//!   replay;
//! * the drop of the lock removes the services, also when the test panics.
//!
//! [`MockEmbedder`] gives each text a vector of exact `f32` values, and [`FakeMirror`] answers
//! fixed neighbours. Both count their calls, so a test can check that replay never asks a
//! service. The fake mirror also applies the writes of the mirror writer (P6 step 7b): it keeps
//! the records of each collection and a log of the write calls. A stalled fake mirror holds its
//! writes until the test releases them, and a down fake mirror refuses them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
// Changed by decision 114: no reconciler reads a registry of channels.
// use rholang::rust::interpreter::reified::vectordb_services::{
//     self, EmbedFailure, Embedder, MirrorBackend, MirrorFailure, MirrorRegistry, MirrorWriter,
//     Neighbour, VectorDbServices, COLLECTION_PREFIX,
// };
use rholang::rust::interpreter::reified::vectordb_services::{
    self, EmbedFailure, Embedder, MirrorBackend, MirrorFailure, MirrorWriter, Neighbour,
    VectorDbServices, COLLECTION_PREFIX,
};
use rholang::rust::interpreter::spaces::mirror::{MirrorMetric, MirrorRecord};
// Retired by decision 114: a collection keeps no registered space.
// use rholang::rust::interpreter::spaces::namespace::SpaceId;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard, Semaphore};

/// The lock of the services of the process.
static SERVICES: AsyncMutex<()> = AsyncMutex::const_new(());

/// The services of the process for one test. The drop removes the services.
pub struct ServicesLock {
    _guard: MutexGuard<'static, ()>,
}

impl ServicesLock {
    /// Replaces the services while the lock is held.
    pub fn replace(&self, services: VectorDbServices) { vectordb_services::install(services); }

    /// Removes the services while the lock is held.
    pub fn remove(&self) { vectordb_services::uninstall(); }
}

impl Drop for ServicesLock {
    fn drop(&mut self) { vectordb_services::uninstall(); }
}

/// Takes the lock and installs `services`.
pub async fn install(services: VectorDbServices) -> ServicesLock {
    let guard = SERVICES.lock().await;
    vectordb_services::install(services);
    ServicesLock { _guard: guard }
}

/// Takes the lock and removes the services.
pub async fn without_services() -> ServicesLock {
    let guard = SERVICES.lock().await;
    vectordb_services::uninstall();
    ServicesLock { _guard: guard }
}

/// The services of `embedder` and `mirror`, with a writer to `mirror`. The writer spawns its
/// task, so the call needs a Tokio runtime.
pub fn services(embedder: &Arc<MockEmbedder>, mirror: &Arc<FakeMirror>) -> VectorDbServices {
    VectorDbServices::with_writer(
        Arc::clone(embedder) as Arc<dyn Embedder>,
        Arc::clone(mirror) as Arc<dyn MirrorBackend>,
    )
}

/// The services of `embedder` and `mirror`, with a writer to `mirror` whose queue holds
/// `capacity` batches.
pub fn services_with_queue(
    embedder: &Arc<MockEmbedder>,
    mirror: &Arc<FakeMirror>,
    capacity: usize,
) -> VectorDbServices {
    let backend = Arc::clone(mirror) as Arc<dyn MirrorBackend>;
    VectorDbServices {
        embedder: Arc::clone(embedder) as Arc<dyn Embedder>,
        mirror: Arc::clone(&backend),
        writer: Some(MirrorWriter::spawn_with(backend, capacity)),
    }
}

/// The writer of the services of the process.
pub fn writer() -> Arc<MirrorWriter> {
    vectordb_services::current()
        .writer
        .clone()
        .expect("the services of the test have a mirror writer")
}

/// The answers of a [`MockEmbedder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedMode {
    /// One vector of [`MockEmbedder::vector`] for each text.
    Vectors,
    /// One vector for each text, with a NaN element.
    NonFinite,
    /// The model fails.
    Unavailable,
    /// One vector less than the texts.
    Short,
}

/// An embedder whose vectors are exact functions of the texts.
pub struct MockEmbedder {
    name: &'static str,
    dimensions: usize,
    offset: f32,
    mode: EmbedMode,
    calls: AtomicUsize,
}

impl MockEmbedder {
    /// An embedder of the model `name` that gives vectors of `dimensions` elements, shifted by
    /// `offset`.
    pub fn new(name: &'static str, dimensions: usize, offset: f32, mode: EmbedMode) -> Arc<Self> {
        Arc::new(Self {
            name,
            dimensions,
            offset,
            mode,
            calls: AtomicUsize::new(0),
        })
    }

    /// The vector of `text`: element `i` is `offset + len(text) + i`, an exact `f32` value for
    /// the short texts of the tests.
    pub fn vector(&self, text: &str) -> Vec<f32> {
        (0..self.dimensions)
            .map(|index| self.offset + text.len() as f32 + index as f32)
            .collect()
    }

    /// The number of calls of [`Embedder::embed`].
    pub fn calls(&self) -> usize { self.calls.load(Ordering::SeqCst) }
}

#[async_trait]
impl Embedder for MockEmbedder {
    fn model(&self) -> &str { self.name }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let vectors = texts.iter().map(|text| self.vector(text));
        match self.mode {
            EmbedMode::Vectors => Ok(vectors.collect()),
            EmbedMode::NonFinite => Ok(vectors
                .map(|mut vector| {
                    vector[0] = f32::NAN;
                    vector
                })
                .collect()),
            EmbedMode::Unavailable => Err(EmbedFailure::Unavailable),
            EmbedMode::Short => Ok(vectors.skip(1).collect()),
        }
    }
}

/// One query that a [`FakeMirror`] answered.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorQuery {
    pub collection: String,
    pub channel: String,
    pub vector: Vec<f32>,
    pub k: usize,
}

/// One write call that a [`FakeMirror`] applied or refused, in call order.
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorCall {
    Ensure {
        collection: String,
        metric: MirrorMetric,
    },
    Upsert {
        collection: String,
        records: Vec<MirrorRecord>,
    },
    Delete {
        collection: String,
        ids: Vec<[u8; 32]>,
    },
}

/// The writes of a [`FakeMirror`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// The mirror applies each write.
    Apply,
    /// The mirror holds each write until [`FakeMirror::release`], and then applies it.
    Stall,
    /// The mirror refuses each write.
    Refuse,
}

/// One collection of a [`FakeMirror`]: its distance and its records.
///
/// Changed by decision 114: a collection keeps no registry. Before, it also kept the registered
/// space, the registered channels and the watermark of the reconciler (P6 step 7c).
#[derive(Clone, Debug, PartialEq)]
pub struct FakeCollection {
    pub metric: MirrorMetric,
    pub records: BTreeMap<[u8; 32], MirrorRecord>,
    // Retired by decision 114: a collection keeps no registry.
    // pub space: Option<SpaceId>,
    // pub channels: BTreeMap<String, Vec<u8>>,
    // pub watermark: u64,
}

impl FakeCollection {
    /// An empty collection with the distance `metric`.
    pub fn new(metric: MirrorMetric) -> Self {
        Self {
            metric,
            records: BTreeMap::new(),
            // Retired by decision 114: a collection keeps no registry.
            // space: None,
            // channels: BTreeMap::new(),
            // watermark: 0,
        }
    }
}

/// A mirror that answers fixed neighbours, nearest first.
///
/// Changed by decision 114: the mirror answers no watermark. Before, it also answered a fixed
/// watermark.
pub struct FakeMirror {
    neighbours: Vec<Neighbour>,
    // Retired by decision 114: the mirror answers no watermark.
    // watermark: u64,
    available: bool,
    queries: Mutex<Vec<MirrorQuery>>,
    writes: Mutex<WriteMode>,
    gate: Semaphore,
    calls: Mutex<Vec<MirrorCall>>,
    collections: Mutex<BTreeMap<String, FakeCollection>>,
}

impl FakeMirror {
    // Changed by decision 114: the mirror answers no watermark, so no constructor takes one.
    // fn with(
    //     neighbours: Vec<Neighbour>,
    //     watermark: u64,
    //     available: bool,
    //     writes: WriteMode,
    // ) -> Arc<Self> {
    fn with(neighbours: Vec<Neighbour>, available: bool, writes: WriteMode) -> Arc<Self> {
        Arc::new(Self {
            neighbours,
            // watermark,
            available,
            queries: Mutex::new(Vec::new()),
            writes: Mutex::new(writes),
            gate: Semaphore::new(0),
            calls: Mutex::new(Vec::new()),
            collections: Mutex::new(BTreeMap::new()),
        })
    }

    /// A mirror that answers the first `k` of `neighbours`, and applies each write.
    ///
    /// Changed by decision 114: the mirror answers no watermark. Before, `new` also took the
    /// watermark that the mirror answered.
    // pub fn new(neighbours: Vec<Neighbour>, watermark: u64) -> Arc<Self> {
    //     Self::with(neighbours, watermark, true, WriteMode::Apply)
    // }
    pub fn new(neighbours: Vec<Neighbour>) -> Arc<Self> {
        Self::with(neighbours, true, WriteMode::Apply)
    }

    /// A mirror that does not answer and refuses each write.
    // pub fn down() -> Arc<Self> { Self::with(Vec::new(), 0, false, WriteMode::Refuse) }
    pub fn down() -> Arc<Self> { Self::with(Vec::new(), false, WriteMode::Refuse) }

    /// A mirror that holds each write until [`FakeMirror::release`].
    // pub fn stalled() -> Arc<Self> { Self::with(Vec::new(), 0, true, WriteMode::Stall) }
    pub fn stalled() -> Arc<Self> { Self::with(Vec::new(), true, WriteMode::Stall) }

    /// Changes the write mode of the mirror, for example to refuse the writes of one deploy.
    pub fn set_writes(&self, mode: WriteMode) {
        *self
            .writes
            .lock()
            .expect("the write mode of the fake mirror") = mode;
    }

    /// Lets the held writes of a stalled mirror, and all later writes, proceed.
    pub fn release(&self) { self.gate.add_permits(Semaphore::MAX_PERMITS / 2); }

    /// The queries that the mirror answered or refused, in call order.
    pub fn queries(&self) -> Vec<MirrorQuery> {
        self.queries
            .lock()
            .expect("the query log of the fake mirror")
            .clone()
    }

    /// The write calls that the mirror applied or refused, in call order.
    pub fn calls(&self) -> Vec<MirrorCall> {
        self.calls
            .lock()
            .expect("the write log of the fake mirror")
            .clone()
    }

    /// The collection `name`, or `None` when the mirror does not hold it.
    pub fn collection(&self, name: &str) -> Option<FakeCollection> {
        self.collections
            .lock()
            .expect("the collections of the fake mirror")
            .get(name)
            .cloned()
    }

    /// Puts the collection `collection` into the mirror as it is, for example a collection that
    /// another writer left or a record that a test removes behind the writer.
    pub fn put(&self, name: &str, collection: FakeCollection) {
        self.collections
            .lock()
            .expect("the collections of the fake mirror")
            .insert(name.to_string(), collection);
    }

    /// Logs the record-write call `call`, and then holds it, refuses it or lets it proceed.
    async fn admit(&self, call: MirrorCall) -> Result<(), MirrorFailure> {
        self.calls
            .lock()
            .expect("the write log of the fake mirror")
            .push(call);
        self.gate().await
    }

    /// Holds, refuses or lets proceed a write, as the write mode says.
    ///
    /// Changed by decision 114: only the logged write calls go through it. Before, the registry
    /// calls also went through it, without a log entry.
    async fn gate(&self) -> Result<(), MirrorFailure> {
        let mode = *self
            .writes
            .lock()
            .expect("the write mode of the fake mirror");
        match mode {
            WriteMode::Apply => Ok(()),
            WriteMode::Stall => self
                .gate
                .acquire()
                .await
                .map(drop)
                .map_err(|_| MirrorFailure::Unavailable),
            WriteMode::Refuse => Err(MirrorFailure::Unavailable),
        }
    }
}

#[async_trait]
impl MirrorBackend for FakeMirror {
    async fn query(
        &self,
        collection: &str,
        channel: &str,
        vector: &[f32],
        k: usize,
    ) -> Result<Vec<Neighbour>, MirrorFailure> {
        self.queries
            .lock()
            .expect("the query log of the fake mirror")
            .push(MirrorQuery {
                collection: collection.to_string(),
                channel: channel.to_string(),
                vector: vector.to_vec(),
                k,
            });
        match self.available {
            true => Ok(self.neighbours.iter().take(k).cloned().collect()),
            false => Err(MirrorFailure::Unavailable),
        }
    }

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> {
    //     match self.available {
    //         true => Ok(self.watermark),
    //         false => Err(MirrorFailure::Unavailable),
    //     }
    // }

    async fn ensure_collection(
        &self,
        collection: &str,
        metric: MirrorMetric,
    ) -> Result<(), MirrorFailure> {
        self.admit(MirrorCall::Ensure {
            collection: collection.to_string(),
            metric,
        })
        .await?;
        self.collections
            .lock()
            .expect("the collections of the fake mirror")
            .entry(collection.to_string())
            .or_insert_with(|| FakeCollection::new(metric));
        Ok(())
    }

    async fn upsert(
        &self,
        collection: &str,
        records: &[MirrorRecord],
    ) -> Result<(), MirrorFailure> {
        self.admit(MirrorCall::Upsert {
            collection: collection.to_string(),
            records: records.to_vec(),
        })
        .await?;
        let mut collections = self
            .collections
            .lock()
            .expect("the collections of the fake mirror");
        let held = collections
            .get_mut(collection)
            .ok_or(MirrorFailure::Unavailable)?;
        for record in records {
            held.records.insert(record.id, record.clone());
        }
        Ok(())
    }

    async fn delete(&self, collection: &str, ids: &[[u8; 32]]) -> Result<(), MirrorFailure> {
        self.admit(MirrorCall::Delete {
            collection: collection.to_string(),
            ids: ids.to_vec(),
        })
        .await?;
        if let Some(held) = self
            .collections
            .lock()
            .expect("the collections of the fake mirror")
            .get_mut(collection)
        {
            for id in ids {
                held.records.remove(id);
            }
        }
        Ok(())
    }

    // Retired by decision 114: a batch registers no channels.
    // async fn register(
    //     &self,
    //     collection: &str,
    //     space: &SpaceId,
    //     channels: &BTreeMap<String, Vec<u8>>,
    // ) -> Result<(), MirrorFailure> {
    //     self.gate().await?;
    //     let mut collections = self
    //         .collections
    //         .lock()
    //         .expect("the collections of the fake mirror");
    //     let held = collections
    //         .get_mut(collection)
    //         .ok_or(MirrorFailure::Unavailable)?;
    //     held.space.get_or_insert(*space);
    //     for (digest, key) in channels {
    //         held.channels.entry(digest.clone()).or_insert(key.clone());
    //     }
    //     Ok(())
    // }

    async fn collections(&self) -> Result<Vec<String>, MirrorFailure> {
        match self.available {
            true => Ok(self
                .collections
                .lock()
                .expect("the collections of the fake mirror")
                .keys()
                .filter(|name| name.starts_with(COLLECTION_PREFIX))
                .cloned()
                .collect()),
            false => Err(MirrorFailure::Unavailable),
        }
    }

    // Retired by decision 114: no reconciler reads a registry of channels.
    // async fn registry(&self, collection: &str) -> Result<MirrorRegistry, MirrorFailure> {
    //     if !self.available {
    //         return Err(MirrorFailure::Unavailable);
    //     }
    //     Ok(self
    //         .collections
    //         .lock()
    //         .expect("the collections of the fake mirror")
    //         .get(collection)
    //         .map(|held| MirrorRegistry {
    //             space: held.space,
    //             channels: held.channels.clone(),
    //             watermark: held.watermark,
    //         })
    //         .unwrap_or_default())
    // }

    async fn ids(
        &self,
        collection: &str,
        channel: &str,
    ) -> Result<BTreeSet<[u8; 32]>, MirrorFailure> {
        if !self.available {
            return Err(MirrorFailure::Unavailable);
        }
        Ok(self
            .collections
            .lock()
            .expect("the collections of the fake mirror")
            .get(collection)
            .map(|held| {
                held.records
                    .values()
                    .filter(|record| record.channel == channel)
                    .map(|record| record.id)
                    .collect()
            })
            .unwrap_or_default())
    }

    // Retired by decision 114: nothing repairs the mirror, so it keeps no watermark.
    // async fn set_watermark(&self, collection: &str, watermark: u64) -> Result<(), MirrorFailure> {
    //     self.gate().await?;
    //     self.collections
    //         .lock()
    //         .expect("the collections of the fake mirror")
    //         .get_mut(collection)
    //         .ok_or(MirrorFailure::Unavailable)?
    //         .watermark = watermark;
    //     Ok(())
    // }
}
