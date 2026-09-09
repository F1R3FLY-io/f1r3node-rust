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

use comm::rust::errors::CommError;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{StoreItemsMessage, StoreItemsMessageRequest};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::rust::engine::lfs_horizon_requester::{HasRootFn, HorizonRequesterOps};
use crate::rust::engine::lfs_tuple_space_requester::{StatePartPath, PAGE_SIZE};
use crate::rust::engine::recovery_actor_inbox::{RecoveryEvent, RecoveryInbox};
use crate::rust::errors::CasperError;
use crate::rust::recovery_budget::{RecoveryDispatch, RecoveryRegistration, RecoveryWindow};

const MAX_TRACKED_ROOTS: usize = 256;
const MAX_OWNERS_PER_ROOT: usize = 1_024;
const MAX_CHUNKS_PER_ROOT: usize = 65_536;
const MAX_DISPATCH_PER_TICK: usize = 16;
const RESEND_INTERVAL: Duration = Duration::from_secs(10);
const ROOT_DEADLINE: Duration = Duration::from_secs(120);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

struct PendingRoot {
    owners: HashSet<BlockHash>,
    outstanding: HashSet<StatePartPath>,
    chunks_imported: usize,
    last_progress: Instant,
    deadline_deferred: bool,
}

struct RootDispatch {
    path: StatePartPath,
    dispatch: RecoveryDispatch<Blake2b256Hash>,
}

struct Core {
    pending: HashMap<Blake2b256Hash, PendingRoot>,
    path_to_root: HashMap<StatePartPath, Blake2b256Hash>,
    recovery: RecoveryWindow<Blake2b256Hash>,
    importer: Arc<dyn RSpaceImporter>,
    has_root: HasRootFn,
}

impl Core {
    fn new(
        importer: Arc<dyn RSpaceImporter>,
        has_root: HasRootFn,
        dispatch_timeout: Duration,
    ) -> Self {
        Self {
            pending: HashMap::new(),
            path_to_root: HashMap::new(),
            recovery: RecoveryWindow::new(
                MAX_TRACKED_ROOTS,
                RESEND_INTERVAL,
                MAX_RETRY_DELAY,
                dispatch_timeout,
            ),
            importer,
            has_root,
        }
    }

    fn on_fetch(
        &mut self,
        root: Blake2b256Hash,
        owner: BlockHash,
        now: Instant,
    ) -> Option<RootDispatch> {
        if (self.has_root)(&root).unwrap_or(false) {
            return None;
        }
        if let Some(pending) = self.pending.get_mut(&root) {
            if pending.owners.len() >= MAX_OWNERS_PER_ROOT && !pending.owners.contains(&owner) {
                tracing::warn!(%root, "state requester owner capacity reached");
                return None;
            }
            pending.owners.insert(owner);
            return self.dispatch_root(&root, now);
        }
        if self.recovery.register(root.clone(), now) == RecoveryRegistration::Capacity {
            tracing::warn!(
                %root,
                capacity = MAX_TRACKED_ROOTS,
                "state requester root capacity reached"
            );
            return None;
        }
        let path: StatePartPath = vec![(root.clone(), None)];
        self.pending.insert(root.clone(), PendingRoot {
            owners: HashSet::from([owner]),
            outstanding: HashSet::from([path.clone()]),
            chunks_imported: 0,
            last_progress: now,
            deadline_deferred: false,
        });
        self.path_to_root.insert(path, root.clone());
        self.dispatch_root(&root, now)
    }

    fn on_items(&mut self, message: StoreItemsMessage, now: Instant) -> Option<RootDispatch> {
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
                 not have this root"
            );
            pending.outstanding.insert(start_path.clone());
            self.path_to_root.insert(start_path, root.clone());
            pending.last_progress = now;
            pending.deadline_deferred = true;
            self.recovery.defer_to_maximum(&root, now);
            return None;
        }

        if pending.chunks_imported >= MAX_CHUNKS_PER_ROOT {
            tracing::error!(
                %root,
                capacity = MAX_CHUNKS_PER_ROOT,
                "state requester chunk capacity reached"
            );
            pending.outstanding.insert(start_path.clone());
            self.path_to_root.insert(start_path, root.clone());
            pending.last_progress = now;
            pending.deadline_deferred = true;
            self.recovery.defer_to_maximum(&root, now);
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
        pending.last_progress = now;
        pending.deadline_deferred = false;
        self.recovery.record_progress(&root, now);

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
                     peer shipped incomplete data"
                );
                pending.outstanding.insert(start_path.clone());
                self.path_to_root.insert(start_path, root.clone());
                pending.deadline_deferred = true;
                self.recovery.defer_to_maximum(&root, now);
                return None;
            }
            self.drop_root(&root);
            None
        } else {
            pending.outstanding.insert(last_path.clone());
            self.path_to_root.insert(last_path, root.clone());
            self.dispatch_root(&root, now)
        }
    }

    fn on_tick(&mut self, now: Instant) -> Vec<RootDispatch> {
        let stalled: Vec<Blake2b256Hash> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                !pending.deadline_deferred
                    && now.saturating_duration_since(pending.last_progress) >= ROOT_DEADLINE
            })
            .map(|(root, _)| root.clone())
            .collect();
        for root in stalled {
            tracing::error!(
                %root,
                deadline = ?ROOT_DEADLINE,
                "state requester: no peer served this root within the deadline; \
                 using maximum retry backoff"
            );
            if let Some(pending) = self.pending.get_mut(&root) {
                pending.deadline_deferred = true;
            }
            self.recovery.defer_to_maximum(&root, now);
        }
        self.recovery
            .take_ready_batch(now, MAX_DISPATCH_PER_TICK)
            .into_iter()
            .filter_map(|dispatch| {
                let path = self
                    .pending
                    .get(dispatch.key())?
                    .outstanding
                    .iter()
                    .next()?
                    .clone();
                Some(RootDispatch { path, dispatch })
            })
            .collect()
    }

    fn finish_dispatch(&mut self, request: RootDispatch, now: Instant, timed_out: bool) {
        if timed_out {
            self.recovery.expire_dispatch(request.dispatch, now);
        } else {
            self.recovery.finish_dispatch(request.dispatch, now);
        }
    }

    fn release_owner(&mut self, owner: &BlockHash) {
        let abandoned = self
            .pending
            .iter_mut()
            .filter_map(|(root, pending)| {
                pending.owners.remove(owner);
                pending.owners.is_empty().then(|| root.clone())
            })
            .collect::<Vec<_>>();
        for root in abandoned {
            self.drop_root(&root);
        }
    }

    fn dispatch_root(&mut self, root: &Blake2b256Hash, now: Instant) -> Option<RootDispatch> {
        let dispatch = self.recovery.take_ready_key(root, now)?;
        let path = self.pending.get(root)?.outstanding.iter().next()?.clone();
        Some(RootDispatch { path, dispatch })
    }

    fn on_command(&mut self, command: StateRootFetchCommand, now: Instant) -> Option<RootDispatch> {
        match command {
            StateRootFetchCommand::Acquire { root, owner } => self.on_fetch(root, owner, now),
            StateRootFetchCommand::ReleaseOwner(owner) => {
                self.release_owner(&owner);
                None
            }
        }
    }

    fn drop_root(&mut self, root: &Blake2b256Hash) {
        if let Some(pending) = self.pending.remove(root) {
            for path in pending.outstanding {
                self.path_to_root.remove(&path);
            }
        }
        self.recovery.resolve(root);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateRootFetchCommand {
    Acquire {
        root: Blake2b256Hash,
        owner: BlockHash,
    },
    ReleaseOwner(BlockHash),
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
        let send = self
            .transport
            .send_to_bootstrap(&self.rp_conf, Arc::new(message.to_proto()));
        match tokio::time::timeout(self.rp_conf.default_timeout, send).await {
            Ok(result) => result.map_err(CasperError::from),
            Err(_) => Err(CasperError::CommError(CommError::TimeOut)),
        }
    }
}

/// The senders the rest of the node holds: `fetch_tx` for naming missing
/// roots, `items_tx` for routing incoming [`StoreItemsMessage`]s from Running.
#[derive(Clone)]
pub struct StateRequesterHandles {
    pub fetch_tx: mpsc::Sender<StateRootFetchCommand>,
    pub items_tx: mpsc::Sender<StoreItemsMessage>,
}

async fn run_requester(
    core: &mut Core,
    mut inbox: RecoveryInbox<StoreItemsMessage, StateRootFetchCommand>,
    sender: &impl HorizonRequesterOps,
) {
    while let Some(event) = inbox.next().await {
        let to_request: Vec<RootDispatch> = match event {
            RecoveryEvent::Item(Some(message)) => {
                core.on_items(message, Instant::now()).into_iter().collect()
            }
            RecoveryEvent::Command(Some(command)) => {
                let mut batch = Vec::with_capacity(MAX_DISPATCH_PER_TICK);
                if let Some(request) = core.on_command(command, Instant::now()) {
                    batch.push(request);
                }
                for _ in 1..MAX_DISPATCH_PER_TICK {
                    let Ok(command) = inbox.try_command() else {
                        break;
                    };
                    if let Some(request) = core.on_command(command, Instant::now()) {
                        batch.push(request);
                    }
                }
                batch
            }
            RecoveryEvent::Tick => core.on_tick(Instant::now()),
            RecoveryEvent::Item(None) | RecoveryEvent::Command(None) => continue,
        };
        let results = futures::future::join_all(
            to_request
                .iter()
                .map(|request| sender.request_for_horizon_chunk(&request.path, PAGE_SIZE)),
        )
        .await;
        for (request, result) in to_request.into_iter().zip(results) {
            let timed_out = matches!(result, Err(CasperError::CommError(CommError::TimeOut)));
            if let Err(e) = result {
                tracing::warn!(error = %e, "state requester: chunk request failed; the resend tick will retry");
            }
            core.finish_dispatch(request, Instant::now(), timed_out);
        }
    }
    tracing::info!("state requester: channels closed, stopping");
}

/// Spawn the requester task and return its handles.
pub fn spawn<T: TransportLayer + Send + Sync + 'static>(
    transport: Arc<T>,
    rp_conf: RPConf,
    importer: Arc<dyn RSpaceImporter>,
    has_root: HasRootFn,
) -> StateRequesterHandles {
    let (fetch_tx, fetch_rx) = mpsc::channel::<StateRootFetchCommand>(256);
    let (items_tx, items_rx) = mpsc::channel::<StoreItemsMessage>(64);
    let dispatch_timeout = rp_conf.default_timeout;
    let sender = BootstrapChunkSender { transport, rp_conf };

    tokio::spawn(async move {
        let mut core = Core::new(importer, has_root, dispatch_timeout);
        let mut resend = tokio::time::interval(RESEND_INTERVAL);
        resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let inbox = RecoveryInbox::new(items_rx, fetch_rx, resend);
        run_requester(&mut core, inbox, &sender).await;
    });

    StateRequesterHandles { fetch_tx, items_tx }
}

#[cfg(test)]
#[path = "runtime_state_import_tests.rs"]
mod import_validity_tests;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use prost::bytes::Bytes;
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
        (
            Core::new(importer, has_root, Duration::from_millis(10)),
            recorded,
        )
    }

    fn root(tag: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![tag; 32]) }

    fn owner(tag: u8) -> BlockHash { prost::bytes::Bytes::from(vec![tag; 32]) }

    fn peer() -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from_static(b"state-peer"),
            },
            endpoint: Endpoint {
                host: "host".to_string(),
                tcp_port: 40400,
                udp_port: 40400,
            },
        }
    }

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

    struct ConcurrentSender {
        barrier: tokio::sync::Barrier,
        active: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl HorizonRequesterOps for ConcurrentSender {
        async fn request_for_horizon_chunk(
            &self,
            _path: &StatePartPath,
            _page_size: i32,
        ) -> Result<(), CasperError> {
            use std::sync::atomic::Ordering::SeqCst;

            let active = self.active.fetch_add(1, SeqCst) + 1;
            self.peak.fetch_max(active, SeqCst);
            self.calls.fetch_add(1, SeqCst);
            self.barrier.wait().await;
            self.active.fetch_sub(1, SeqCst);
            Ok(())
        }
    }

    fn concurrent_sender(count: usize) -> ConcurrentSender {
        ConcurrentSender {
            barrier: tokio::sync::Barrier::new(count),
            active: std::sync::atomic::AtomicUsize::new(0),
            peak: std::sync::atomic::AtomicUsize::new(0),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[tokio::test]
    async fn actor_drains_owner_release_and_stops_after_both_channels_close() {
        let (mut core, _) = core_with_store();
        let now = Instant::now();
        let request = core.on_fetch(root(1), owner(1), now).unwrap();
        core.finish_dispatch(request, now, false);
        let (items_tx, items_rx) = mpsc::channel(64);
        let (commands_tx, commands_rx) = mpsc::channel(16);
        for _ in 0..64 {
            items_tx
                .send(chunk(vec![(root(99), None)], vec![], 0))
                .await
                .unwrap();
        }
        commands_tx
            .send(StateRootFetchCommand::ReleaseOwner(owner(1)))
            .await
            .unwrap();
        drop(items_tx);
        drop(commands_tx);
        let resend = tokio::time::interval(RESEND_INTERVAL);
        let inbox = RecoveryInbox::new(items_rx, commands_rx, resend);
        let sender = concurrent_sender(1);
        tokio::time::timeout(
            Duration::from_secs(2),
            run_requester(&mut core, inbox, &sender),
        )
        .await
        .expect("closed actor channels must terminate without waiting for the periodic timer");
        assert!(core.pending.is_empty());
        assert!(core.path_to_root.is_empty());
        assert!(!core.recovery.contains(&root(1)));
        assert_eq!(sender.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn actor_keeps_dispatches_concurrent_and_bounded_within_each_batch() {
        let (mut core, _) = core_with_store();
        let (items_tx, items_rx) = mpsc::channel(1);
        let (commands_tx, commands_rx) = mpsc::channel(64);
        for tag in 0..(2 * MAX_DISPATCH_PER_TICK) {
            commands_tx
                .send(StateRootFetchCommand::Acquire {
                    root: root(tag as u8),
                    owner: owner(tag as u8),
                })
                .await
                .unwrap();
        }
        drop(items_tx);
        drop(commands_tx);
        let resend = tokio::time::interval_at(
            tokio::time::Instant::now() + RESEND_INTERVAL,
            RESEND_INTERVAL,
        );
        let inbox = RecoveryInbox::new(items_rx, commands_rx, resend);
        let sender = concurrent_sender(MAX_DISPATCH_PER_TICK);
        tokio::time::timeout(
            Duration::from_secs(2),
            run_requester(&mut core, inbox, &sender),
        )
        .await
        .expect("each dispatch batch must reach the concurrent barrier and the actor must stop");
        use std::sync::atomic::Ordering::SeqCst;
        assert_eq!(sender.peak.load(SeqCst), MAX_DISPATCH_PER_TICK);
        assert_eq!(sender.calls.load(SeqCst), 2 * MAX_DISPATCH_PER_TICK);
        assert_eq!(sender.active.load(SeqCst), 0);
        assert_eq!(core.pending.len(), 2 * MAX_DISPATCH_PER_TICK);
    }

    /// The reason this module exists: a missing root, fetched and verified, so
    /// the block that deferred on it can pass on its next retry. Run 12's
    /// joiner recorded four slashable verdicts and poisoned ninety-one blocks
    /// for want of exactly this.
    #[test]
    fn a_fetched_root_is_imported_verified_and_visible() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);
        let now = Instant::now();

        let request = core
            .on_fetch(r.clone(), owner(1), now)
            .expect("an absent root is fetched");
        assert_eq!(request.path, vec![(r.clone(), None)]);

        let more = core.on_items(chunk(request.path.clone(), request.path, 2), now);
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
        let now = Instant::now();

        let first = core
            .on_fetch(r.clone(), owner(1), now)
            .expect("fetch starts");
        let next = core
            .on_items(chunk(first.path, cursor.clone(), 1), now)
            .expect("non-terminal chunk paginates");
        assert_eq!(next.path, cursor);
        assert!(
            !recorded.lock().unwrap().contains(&r),
            "a root recorded before its trie is complete would verify against a lie"
        );

        let done = core.on_items(chunk(cursor.clone(), cursor, 1), now);
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
        let now = Instant::now();

        let request = core
            .on_fetch(r.clone(), owner(1), now)
            .expect("fetch starts");
        let more = core.on_items(chunk(request.path.clone(), request.path, 0), now);
        assert!(more.is_none());
        assert!(
            !recorded.lock().unwrap().contains(&r),
            "a root the peer could not serve must not be recorded"
        );
        assert!(
            core.pending.contains_key(&r),
            "the failed fetch keeps its bounded recovery obligation"
        );
    }

    #[test]
    fn fetches_are_deduplicated_and_budgeted() {
        let (mut core, recorded) = core_with_store();
        let r = root(0x4F);
        let now = Instant::now();

        assert!(core.on_fetch(r.clone(), owner(1), now).is_some());
        assert!(
            core.on_fetch(r.clone(), owner(1), now).is_none(),
            "in flight: no second request"
        );

        recorded.lock().unwrap().insert(root(0x50));
        assert!(
            core.on_fetch(root(0x50), owner(2), now).is_none(),
            "a root already present is never fetched"
        );

        for tag in 0..MAX_TRACKED_ROOTS {
            let mut bytes = vec![0; 32];
            bytes[24..].copy_from_slice(&(tag as u64).to_be_bytes());
            let candidate = Blake2b256Hash::from_bytes(bytes);
            core.on_fetch(candidate, owner((tag % 255) as u8), now);
        }
        assert!(
            core.on_fetch(root(0x51), owner(3), now).is_none(),
            "at capacity the node defers new roots without unbounded growth"
        );
        assert_eq!(core.pending.len(), MAX_TRACKED_ROOTS);
    }

    #[test]
    fn releasing_the_last_owner_releases_the_root_capacity() {
        let (mut core, _) = core_with_store();
        let now = Instant::now();
        let r = root(0x4F);
        let owner = owner(1);
        assert!(core.on_fetch(r.clone(), owner.clone(), now).is_some());
        core.release_owner(&owner);
        assert!(!core.pending.contains_key(&r));
        assert!(!core.recovery.contains(&r));
    }

    #[test]
    fn deadline_deferral_retains_the_root_and_resumes_after_maximum_backoff() {
        let (mut core, _) = core_with_store();
        let now = Instant::now();
        let r = root(0x4F);
        let first = core.on_fetch(r.clone(), owner(1), now).unwrap();
        core.finish_dispatch(first, now, false);

        assert!(core.on_tick(now + ROOT_DEADLINE).is_empty());
        assert!(core.pending.contains_key(&r));
        assert!(core.recovery.contains(&r));

        let resumed = core.on_tick(now + ROOT_DEADLINE + MAX_RETRY_DELAY);
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].dispatch.key(), &r);
    }

    #[test]
    fn tick_batches_rotate_across_all_ready_roots() {
        let (mut core, _) = core_with_store();
        let now = Instant::now();
        let roots = (0..(MAX_DISPATCH_PER_TICK + 4))
            .map(|tag| {
                let mut bytes = vec![0; 32];
                bytes[24..].copy_from_slice(&(tag as u64).to_be_bytes());
                Blake2b256Hash::from_bytes(bytes)
            })
            .collect::<Vec<_>>();
        for (index, root) in roots.iter().enumerate() {
            let dispatch = core
                .on_fetch(root.clone(), owner(index as u8), now)
                .unwrap();
            core.finish_dispatch(dispatch, now, false);
            core.recovery.make_ready(root, now);
        }

        let first = core.on_tick(now);
        assert_eq!(first.len(), MAX_DISPATCH_PER_TICK);
        let first_keys = first
            .iter()
            .map(|dispatch| dispatch.dispatch.key().clone())
            .collect::<Vec<_>>();
        for dispatch in first {
            core.finish_dispatch(dispatch, now, false);
        }
        let second = core.on_tick(now);
        assert_eq!(second.len(), 4);

        let requested = first_keys
            .into_iter()
            .chain(
                second
                    .into_iter()
                    .map(|dispatch| dispatch.dispatch.key().clone()),
            )
            .collect::<HashSet<_>>();
        assert_eq!(requested, roots.into_iter().collect());
    }

    #[test]
    fn chunk_capacity_retains_the_root_without_importing_more_data() {
        let (mut core, recorded) = core_with_store();
        let now = Instant::now();
        let r = root(0x4F);
        let request = core.on_fetch(r.clone(), owner(1), now).unwrap();
        core.pending.get_mut(&r).unwrap().chunks_imported = MAX_CHUNKS_PER_ROOT;

        assert!(core
            .on_items(chunk(request.path.clone(), request.path, 1), now)
            .is_none());
        assert!(core.pending.contains_key(&r));
        assert!(core.recovery.contains(&r));
        assert!(!recorded.lock().unwrap().contains(&r));
    }

    #[tokio::test]
    async fn bootstrap_send_has_a_bounded_transport_deadline() {
        let transport = Arc::new(TransportLayerStub::new());
        transport.set_response_delay(Duration::from_secs(5));
        let sender = BootstrapChunkSender {
            transport,
            rp_conf: create_rp_conf_ask(peer(), Some(Duration::from_millis(1)), None),
        };
        let path = vec![(root(1), None)];

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            sender.request_for_horizon_chunk(&path, PAGE_SIZE),
        )
        .await
        .expect("the bootstrap request has a bounded deadline")
        .unwrap_err();
        assert_eq!(error, CasperError::CommError(CommError::TimeOut));
    }
}
