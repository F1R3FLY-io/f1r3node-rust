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
//! service.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rholang::rust::interpreter::reified::vectordb_services::{
    self, EmbedFailure, Embedder, MirrorBackend, MirrorFailure, Neighbour, VectorDbServices,
};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

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

/// The services of `embedder` and `mirror`.
pub fn services(embedder: &Arc<MockEmbedder>, mirror: &Arc<FakeMirror>) -> VectorDbServices {
    VectorDbServices {
        embedder: Arc::clone(embedder) as Arc<dyn Embedder>,
        mirror: Arc::clone(mirror) as Arc<dyn MirrorBackend>,
    }
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

/// A mirror that answers fixed neighbours, nearest first, and a fixed watermark.
pub struct FakeMirror {
    neighbours: Vec<Neighbour>,
    watermark: u64,
    available: bool,
    queries: Mutex<Vec<MirrorQuery>>,
}

impl FakeMirror {
    /// A mirror that answers the first `k` of `neighbours` and the watermark `watermark`.
    pub fn new(neighbours: Vec<Neighbour>, watermark: u64) -> Arc<Self> {
        Arc::new(Self {
            neighbours,
            watermark,
            available: true,
            queries: Mutex::new(Vec::new()),
        })
    }

    /// A mirror that does not answer.
    pub fn down() -> Arc<Self> {
        Arc::new(Self {
            neighbours: Vec::new(),
            watermark: 0,
            available: false,
            queries: Mutex::new(Vec::new()),
        })
    }

    /// The queries that the mirror answered or refused, in call order.
    pub fn queries(&self) -> Vec<MirrorQuery> {
        self.queries
            .lock()
            .expect("the query log of the fake mirror")
            .clone()
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

    async fn watermark(&self, _collection: &str) -> Result<u64, MirrorFailure> {
        match self.available {
            true => Ok(self.watermark),
            false => Err(MirrorFailure::Unavailable),
        }
    }
}
