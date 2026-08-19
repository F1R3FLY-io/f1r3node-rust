//! Runtime state healing: fetch a named rspace root from peers while the node
//! runs.
//!
//! LFS restore fetches state once, for the blocks the DAG holds at that
//! moment. Anything admitted afterwards — a settled-history straggler the
//! restore's closure missed — arrives with bytes but no state, and a block
//! built on it cannot replay. The replay reports the missing root typed
//! ([`RootError::RootNotFound`] → [`BlockError::AwaitingState`]); the block
//! defers as a pendant and retries; this module is what makes the retry
//! eventually succeed: it asks a peer for the root's trie, imports the
//! content-addressed chunks, and records the root once verification passes.
//!
//! Trust model, unchanged from the restore-time horizon sync this reuses the
//! wire protocol of: chunks are radix nodes validated against their own
//! hashes by the importer, so a peer cannot substitute state for a root — it
//! can only fail to serve, which leaves the waiting blocks deferred under
//! their existing retry bounds. Roots enter the queue from exactly two
//! places, both authenticated: the declared state of an admitted block
//! (bonded-citer-gated, budgeted) and a replay of a signature-checked block.
//!
//! [`RootError::RootNotFound`]: rspace_plus_plus::rspace::errors::RootError::RootNotFound
//! [`BlockError::AwaitingState`]: crate::rust::block_status::BlockError::AwaitingState

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::protocol::casper_message::{StoreItemsMessage, StoreItemsMessageRequest};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::rust::engine::lfs_horizon_requester::{HasRootFn, HorizonRequesterOps};
use crate::rust::engine::lfs_tuple_space_requester::{StatePartPath, PAGE_SIZE};
use crate::rust::errors::CasperError;

/// Lifetime cap on roots this node will fetch at runtime. A healthy join needs
/// a handful (two per admitted straggler); the cap prices the worst case the
/// same way the admission budget does — bounded, alarmed, degrade to deferral.
const ROOT_FETCH_BUDGET: u64 = 1_024;

/// Re-request cadence for outstanding chunks, and how long a root may stay
/// outstanding before it is dropped with an alarm. Waiting blocks keep their
/// own deferral bounds either way.
const RESEND_INTERVAL: Duration = Duration::from_secs(10);
const ROOT_DEADLINE: Duration = Duration::from_secs(120);

struct PendingRoot {
    started: Instant,
    /// Chunk paths currently outstanding for this root's pagination walk.
    outstanding: HashSet<StatePartPath>,
    chunks_imported: usize,
}

/// The requester's decision core: pure state transitions in, chunk paths to
/// request out. The spawned task owns the channels and the wire; every rule
/// lives here where a test can drive it deterministically.
struct Core {
    pending: HashMap<Blake2b256Hash, PendingRoot>,
    path_to_root: HashMap<StatePartPath, Blake2b256Hash>,
    fetched_total: u64,
    importer: Arc<dyn RSpaceImporter>,
    has_root: HasRootFn,
}

impl Core {
    fn new(importer: Arc<dyn RSpaceImporter>, has_root: HasRootFn) -> Self {
        Self {
            pending: HashMap::new(),
            path_to_root: HashMap::new(),
            fetched_total: 0,
            importer,
            has_root,
        }
    }

    /// A root was named as missing. Returns the chunk path to request, or
    /// `None` when nothing should be sent (already present, already in
    /// flight, or budget exhausted).
    fn on_fetch(&mut self, root: Blake2b256Hash) -> Option<StatePartPath> {
        if self.pending.contains_key(&root) {
            return None;
        }
        if (self.has_root)(&root).unwrap_or(false) {
            return None;
        }
        if self.fetched_total >= ROOT_FETCH_BUDGET {
            tracing::warn!(
                %root,
                budget = ROOT_FETCH_BUDGET,
                "State-root fetch budget exhausted; the root stays absent and its \
                 dependents stay deferred"
            );
            return None;
        }
        self.fetched_total += 1;
        if self.fetched_total == ROOT_FETCH_BUDGET / 2 {
            tracing::warn!(
                fetched = self.fetched_total,
                budget = ROOT_FETCH_BUDGET,
                "State-root fetches at half budget; a healthy node needs a handful — \
                 investigate what keeps missing state"
            );
        }
        let path: StatePartPath = vec![(root.clone(), None)];
        self.pending.insert(root.clone(), PendingRoot {
            started: Instant::now(),
            outstanding: HashSet::from([path.clone()]),
            chunks_imported: 0,
        });
        self.path_to_root.insert(path.clone(), root);
        Some(path)
    }

    /// A chunk arrived. Imports its items and returns the continuation path to
    /// request, if the walk paginates further.
    fn on_items(&mut self, message: StoreItemsMessage) -> Option<StatePartPath> {
        let StoreItemsMessage {
            start_path,
            last_path,
            history_items,
            data_items,
        } = message;

        let Some(root) = self.path_to_root.remove(&start_path) else {
            tracing::debug!("state requester: chunk for unknown/stale path; ignoring");
            return None;
        };
        let pending = self.pending.get_mut(&root)?;
        pending.outstanding.remove(&start_path);

        let is_terminal = last_path == start_path;
        let first_chunk = pending.chunks_imported == 0;
        if is_terminal && first_chunk && history_items.is_empty() {
            // Terminal cursor on an empty first chunk: the peer does not have
            // this root. Same byzantine signal the restore-time horizon sync
            // fails loudly on.
            tracing::error!(
                %root,
                "state requester: peer served an empty terminal first chunk — it does \
                 not have this root; dropping the fetch"
            );
            self.drop_root(&root);
            return None;
        }

        self.importer.set_history_items(
            history_items
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect(),
        );
        self.importer.set_data_items(
            data_items
                .into_iter()
                .map(|(hash, bytes)| (hash, bytes.to_vec()))
                .collect(),
        );
        pending.chunks_imported += 1;

        if is_terminal {
            self.importer.set_root(&root);
            let verified = (self.has_root)(&root).unwrap_or(false);
            if verified {
                let chunks = pending.chunks_imported;
                tracing::info!(
                    %root,
                    chunks,
                    "state requester: root imported and verified present"
                );
            } else {
                tracing::error!(
                    %root,
                    "state requester: import completed but the root did not verify — \
                     peer shipped incomplete data; dropping the fetch"
                );
            }
            self.drop_root(&root);
            None
        } else {
            pending.outstanding.insert(last_path.clone());
            self.path_to_root.insert(last_path.clone(), root);
            Some(last_path)
        }
    }

    /// Periodic maintenance: paths to re-request, with expired roots dropped.
    fn on_tick(&mut self) -> Vec<StatePartPath> {
        let expired: Vec<Blake2b256Hash> = self
            .pending
            .iter()
            .filter(|(_, p)| p.started.elapsed() >= ROOT_DEADLINE)
            .map(|(root, _)| root.clone())
            .collect();
        for root in expired {
            tracing::error!(
                %root,
                deadline = ?ROOT_DEADLINE,
                "state requester: no peer served this root within the deadline; \
                 dropping the fetch — dependents stay deferred"
            );
            self.drop_root(&root);
        }
        self.pending
            .values()
            .flat_map(|p| p.outstanding.iter().cloned())
            .collect()
    }

    fn drop_root(&mut self, root: &Blake2b256Hash) {
        if let Some(pending) = self.pending.remove(root) {
            for path in pending.outstanding {
                self.path_to_root.remove(&path);
            }
        }
    }
}

/// Sends chunk requests to the bootstrap, exactly as the restore-time
/// requesters do.
struct BootstrapChunkSender<T: TransportLayer + Send + Sync> {
    transport: Arc<T>,
    rp_conf: RPConf,
}

#[async_trait::async_trait]
impl<T: TransportLayer + Send + Sync> HorizonRequesterOps for BootstrapChunkSender<T> {
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        page_size: i32,
    ) -> Result<(), CasperError> {
        let message = StoreItemsMessageRequest {
            start_path: path.clone(),
            skip: 0,
            take: page_size,
        };
        self.transport
            .send_to_bootstrap(&self.rp_conf, Arc::new(message.to_proto()))
            .await?;
        Ok(())
    }
}

/// The senders the rest of the node holds: `fetch_tx` for naming missing
/// roots, `items_tx` for routing incoming [`StoreItemsMessage`]s from Running.
#[derive(Clone)]
pub struct StateRequesterHandles {
    pub fetch_tx: mpsc::Sender<Blake2b256Hash>,
    pub items_tx: mpsc::Sender<StoreItemsMessage>,
}

/// Spawn the requester task and return its handles.
pub fn spawn<T: TransportLayer + Send + Sync + 'static>(
    transport: Arc<T>,
    rp_conf: RPConf,
    importer: Arc<dyn RSpaceImporter>,
    has_root: HasRootFn,
) -> StateRequesterHandles {
    let (fetch_tx, mut fetch_rx) = mpsc::channel::<Blake2b256Hash>(256);
    let (items_tx, mut items_rx) = mpsc::channel::<StoreItemsMessage>(64);
    let sender = BootstrapChunkSender { transport, rp_conf };

    tokio::spawn(async move {
        let mut core = Core::new(importer, has_root);
        let mut resend = tokio::time::interval(RESEND_INTERVAL);
        resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let to_request: Vec<StatePartPath> = tokio::select! {
                biased;
                Some(message) = items_rx.recv() => core.on_items(message).into_iter().collect(),
                Some(root) = fetch_rx.recv() => core.on_fetch(root).into_iter().collect(),
                _ = resend.tick() => core.on_tick(),
                else => break,
            };
            for path in to_request {
                if let Err(e) = sender.request_for_horizon_chunk(&path, PAGE_SIZE).await {
                    tracing::warn!(error = %e, "state requester: chunk request failed; the resend tick will retry");
                }
            }
        }
        tracing::info!("state requester: channels closed, stopping");
    });

    StateRequesterHandles { fetch_tx, items_tx }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rspace_plus_plus::rspace::shared::trie_importer::TrieImporter;

    use super::*;

    /// Records every `set_root`; `has_root` reads the same set, so the core's
    /// post-import verification sees exactly what was recorded.
    struct RecordingImporter {
        recorded: Arc<Mutex<HashSet<Blake2b256Hash>>>,
    }

    impl TrieImporter for RecordingImporter {
        fn set_history_items(&self, _data: Vec<(Blake2b256Hash, Vec<u8>)>) {}
        fn set_data_items(&self, _data: Vec<(Blake2b256Hash, Vec<u8>)>) {}
        fn set_root(&self, key: &Blake2b256Hash) {
            self.recorded.lock().unwrap().insert(key.clone());
        }
    }

    impl RSpaceImporter for RecordingImporter {
        fn get_history_item(&self, _hash: Blake2b256Hash) -> Option<Vec<u8>> { None }
    }

    fn core_with_store() -> (Core, Arc<Mutex<HashSet<Blake2b256Hash>>>) {
        let recorded = Arc::new(Mutex::new(HashSet::new()));
        let importer: Arc<dyn RSpaceImporter> = Arc::new(RecordingImporter {
            recorded: recorded.clone(),
        });
        let has_root: HasRootFn = {
            let recorded = recorded.clone();
            Arc::new(move |root| Ok(recorded.lock().unwrap().contains(root)))
        };
        (Core::new(importer, has_root), recorded)
    }

    fn root(tag: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![tag; 32]) }

    fn chunk(start: StatePartPath, last: StatePartPath, items: usize) -> StoreItemsMessage {
        StoreItemsMessage {
            start_path: start,
            last_path: last,
            history_items: (0..items)
                .map(|i| (root(0xE0 + i as u8), prost::bytes::Bytes::from_static(b"x")))
                .collect(),
            data_items: vec![],
        }
    }

    /// The reason this module exists: a missing root, fetched and verified, so
    /// the block that deferred on it can pass on its next retry. Run 12's
    /// joiner recorded four slashable verdicts and poisoned ninety-one blocks
    /// for want of exactly this.
    #[test]
    fn a_fetched_root_is_imported_verified_and_visible() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);

        let path = core.on_fetch(r.clone()).expect("an absent root is fetched");
        assert_eq!(path, vec![(r.clone(), None)]);

        let more = core.on_items(chunk(path.clone(), path.clone(), 2));
        assert!(more.is_none(), "a terminal chunk ends the walk");
        assert!(
            recorded.lock().unwrap().contains(&r),
            "the root must be recorded after its trie is imported — this is the \
             moment the deferred block's next retry starts succeeding"
        );
        assert!(core.pending.is_empty(), "nothing left outstanding");
    }

    /// Pagination continues the walk under the same root until the cursor is
    /// terminal; the root is recorded only at the end.
    #[test]
    fn a_paginated_walk_records_the_root_only_at_the_terminal() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);
        let cursor: StatePartPath = vec![(root(0x11), Some(3))];

        let first = core.on_fetch(r.clone()).expect("fetch starts");
        let next = core
            .on_items(chunk(first, cursor.clone(), 1))
            .expect("non-terminal chunk paginates");
        assert_eq!(next, cursor);
        assert!(
            !recorded.lock().unwrap().contains(&r),
            "a root recorded before its trie is complete would verify against a lie"
        );

        let done = core.on_items(chunk(cursor.clone(), cursor, 1));
        assert!(done.is_none());
        assert!(recorded.lock().unwrap().contains(&r));
    }

    /// A peer that answers "terminal, empty" on the first chunk does not have
    /// the root. The fetch is dropped loudly and the root is never recorded —
    /// recording it would make later resets succeed against absent state.
    #[test]
    fn an_empty_terminal_first_chunk_is_a_peer_without_the_root() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);

        let path = core.on_fetch(r.clone()).expect("fetch starts");
        let more = core.on_items(chunk(path.clone(), path, 0));
        assert!(more.is_none());
        assert!(
            !recorded.lock().unwrap().contains(&r),
            "a root the peer could not serve must not be recorded"
        );
        assert!(
            core.pending.is_empty(),
            "the failed fetch is dropped, not retried forever"
        );
    }

    /// Present and in-flight roots are not re-fetched; the budget bounds the
    /// lifetime total and degrades to deferral past it.
    #[test]
    fn fetches_are_deduplicated_and_budgeted() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);

        assert!(core.on_fetch(r.clone()).is_some());
        assert!(
            core.on_fetch(r.clone()).is_none(),
            "in flight: no second request"
        );

        recorded.lock().unwrap().insert(root(0x50));
        assert!(
            core.on_fetch(root(0x50)).is_none(),
            "a root already present is never fetched"
        );

        core.fetched_total = ROOT_FETCH_BUDGET;
        assert!(
            core.on_fetch(root(0x51)).is_none(),
            "past the budget the node degrades to deferral, never silent growth"
        );
    }
}
