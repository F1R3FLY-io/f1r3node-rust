//! Forward-horizon rspace history sync.
//!
//! Companion to `lfs_block_requester` (per-block message sync) and
//! `lfs_tuple_space_requester` (LFB-state subtree sync). This module
//! syncs rspace post-state roots for every block within the
//! forward-horizon window — every block that an honest proposer could
//! legitimately reference as a parent of an upcoming proposal.
//!
//! Together with `validate::parents`' parent-depth check, this guarantees
//! that every legitimately-validatable block has its parents' rspace state
//! local at validation time. No `UnknownRootError` can fire on
//! consensus-valid blocks; out-of-horizon blocks are rejected on consensus
//! rules in `validate::parents`.
//!
//! ## What is requested
//!
//! Horizon states share nearly all of their trie with each other and with the
//! LFB state that is already local. `StateWalk` walks every root through the
//! local store and names the trie nodes that are absent; each is requested
//! once, whichever roots reach it. When no node is absent any more, the leaf
//! data that is absent is requested. A held node is never assumed to have a
//! complete subtree: its children are walked too. Roots are recorded at the
//! end, when nothing is absent.
//!
//! Wire format (`StoreItemsMessageRequest` / `StoreItemsMessage`) is shared
//! with `lfs_tuple_space_requester`. A request path starts at the absent node
//! itself, and a response is matched to its request by `start_path`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::Stream;
use models::rust::casper::protocol::casper_message::StoreItemsMessage;
use rspace_plus_plus::rspace::errors::RadixTreeError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_importer::{RSpaceImporter, RSpaceImporterInstance};
use rspace_plus_plus::rspace::state::state_walk::{StateWalk, WALK_PAGE_SIZE};
use tokio::sync::mpsc;

use crate::rust::engine::lfs_tuple_space_requester::{StatePartPath, StoreItems};
use crate::rust::errors::CasperError;

/// Per-request chunk size, carried in the request (the responder pages by
/// it); independent of `lfs_tuple_space_requester::PAGE_SIZE`.
pub const PAGE_SIZE: i32 = WALK_PAGE_SIZE;

pub const MAX_IN_FLIGHT_REQUESTS: usize = 64;

/// Network operations needed by the horizon requester. Decoupled from
/// `TransportLayer` so unit tests can stub it without spinning up a
/// transport stack. Mirrors `lfs_tuple_space_requester::TupleSpaceRequesterOps`.
#[async_trait]
pub trait HorizonRequesterOps: Send + Sync {
    /// Send a `StoreItemsMessageRequest` for the given chunk path. The
    /// production impl unicasts to bootstrap; the response arrives on the
    /// shared `mpsc::Receiver<StoreItemsMessage>` correlated by `start_path`.
    async fn request_for_horizon_chunk(
        &self,
        path: &StatePartPath,
        page_size: i32,
    ) -> Result<(), CasperError>;
}

/// Closure type for "is this root already in the joiner's roots store?".
/// Decoupled from `RuntimeManager` so the orchestrator can be unit-tested
/// against a fake roots store without spinning up the runtime stack.
pub type HasRootFn = Arc<dyn Fn(&Blake2b256Hash) -> Result<bool, CasperError> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizonProgress {
    requested: usize,
    done: usize,
    finished: bool,
}

impl HorizonProgress {
    pub fn is_finished(&self) -> bool { self.finished }

    pub fn len(&self) -> usize { self.requested }

    pub fn is_empty(&self) -> bool { self.requested == 0 }

    pub fn done_count(&self) -> usize { self.done }
}

#[derive(Debug, Clone)]
enum Wanted {
    Node(Blake2b256Hash),
    Data {
        kind: Option<u8>,
        data: HashSet<Blake2b256Hash>,
    },
}

struct PathTimer {
    sent_at: Instant,
    timeout: Duration,
}

#[derive(Default)]
struct Requests {
    wanted: HashMap<StatePartPath, Wanted>,
    queue: VecDeque<StatePartPath>,
    in_flight: HashMap<StatePartPath, PathTimer>,
    done: usize,
}

impl Requests {
    fn add(&mut self, path: StatePartPath, wanted: Wanted) {
        self.queue.push_back(path.clone());
        self.wanted.insert(path, wanted);
    }

    fn add_nodes(&mut self, nodes: Vec<Blake2b256Hash>) {
        for node in nodes {
            self.add(vec![(node.clone(), None)], Wanted::Node(node));
        }
    }

    fn next(&mut self, request_timeout: Duration) -> Vec<StatePartPath> {
        let free = MAX_IN_FLIGHT_REQUESTS.saturating_sub(self.in_flight.len());
        let count = free.min(self.queue.len());
        let paths: Vec<StatePartPath> = self.queue.drain(..count).collect();
        let sent_at = Instant::now();
        for path in &paths {
            self.in_flight.insert(path.clone(), PathTimer {
                sent_at,
                timeout: request_timeout,
            });
        }
        paths
    }

    fn expired(&mut self, max_request_timeout: Duration) -> Vec<StatePartPath> {
        let now = Instant::now();
        self.in_flight
            .iter_mut()
            .filter(|(_, timer)| now.duration_since(timer.sent_at) >= timer.timeout)
            .map(|(path, timer)| {
                timer.sent_at = now;
                timer.timeout = timer.timeout.saturating_mul(2).min(max_request_timeout);
                path.clone()
            })
            .collect()
    }

    fn received(&mut self, path: &StatePartPath) -> Option<Wanted> {
        self.in_flight.remove(path)?;
        self.wanted.get(path).cloned()
    }

    fn rejected(&mut self, path: StatePartPath) { self.queue.push_back(path); }

    fn imported(&mut self, path: &StatePartPath) {
        self.wanted.remove(path);
        self.done += 1;
    }

    fn all_done(&self) -> bool { self.wanted.is_empty() }

    fn progress(&self, finished: bool) -> HorizonProgress {
        HorizonProgress {
            requested: self.done + self.wanted.len(),
            done: self.done,
            finished,
        }
    }
}

struct ValidPage {
    path: StatePartPath,
    wanted: Wanted,
    history_items: StoreItems,
    data_items: StoreItems,
}

fn walk_error(error: RadixTreeError) -> CasperError {
    CasperError::RuntimeError(format!("LFS forward-horizon: state walk failed: {}", error))
}

async fn blocking<R: Send + 'static>(
    task: impl FnOnce() -> R + Send + 'static,
) -> Result<R, CasperError> {
    tokio::task::spawn_blocking(task).await.map_err(|error| {
        CasperError::RuntimeError(format!("LFS forward-horizon: task failed: {}", error))
    })
}

struct HorizonSync<T: HorizonRequesterOps> {
    request_ops: T,
    has_root: HasRootFn,
    state_importer: Arc<dyn RSpaceImporter>,
    roots: Vec<Blake2b256Hash>,
    walk: Arc<Mutex<StateWalk>>,
    requests: Requests,
    request_timeout: Duration,
    max_request_timeout: Duration,
}

impl<T: HorizonRequesterOps> HorizonSync<T> {
    async fn start(&mut self) -> Result<(), CasperError> {
        let walk = self.walk.clone();
        let roots = self.roots.clone();
        let missing = blocking(move || {
            let mut walk = walk.lock().expect("walk lock");
            let mut missing = Vec::new();
            for root in &roots {
                missing.extend(walk.add_root(root)?);
            }
            Ok(missing)
        })
        .await?
        .map_err(walk_error)?;
        self.requests.add_nodes(missing);
        Ok(())
    }

    async fn import(&mut self, messages: Vec<StoreItemsMessage>) -> Result<bool, CasperError> {
        let mut validations = Vec::new();
        for message in messages {
            let StoreItemsMessage {
                start_path,
                last_path,
                history_items,
                data_items,
            } = message;
            let Some(wanted) = self.requests.received(&start_path) else {
                tracing::debug!("LFS forward-horizon: ignoring chunk for unknown/stale path");
                continue;
            };
            if let Wanted::Node(node) = &wanted {
                let empty = history_items.is_empty() && data_items.is_empty();
                if empty && last_path == start_path && self.roots.contains(node) {
                    return Err(CasperError::RuntimeError(format!(
                        "LFS forward-horizon: bootstrap signalled empty/missing root {} \
                         (terminal cursor on first chunk)",
                        node
                    )));
                }
            }
            let kind = match &wanted {
                Wanted::Node(node) => self.walk.lock().expect("walk lock").leaf_kind(node),
                Wanted::Data { kind, .. } => *kind,
            };
            let importer = self.state_importer.clone();
            validations.push(blocking(move || {
                let bytes = |items: Vec<(Blake2b256Hash, prost::bytes::Bytes)>| -> StoreItems {
                    items
                        .into_iter()
                        .map(|(hash, bytes)| (hash, bytes.to_vec()))
                        .collect()
                };
                let page = ValidPage {
                    path: start_path,
                    wanted,
                    history_items: bytes(history_items),
                    data_items: bytes(data_items),
                };
                match RSpaceImporterInstance::validate_subtree_items(
                    &page.history_items,
                    &page.data_items,
                    page.path.clone(),
                    &last_path,
                    PAGE_SIZE,
                    0,
                    kind,
                    importer,
                ) {
                    Ok(()) => Ok(page),
                    Err(error) => Err((page.path, error)),
                }
            }));
        }

        let mut pages = Vec::new();
        for validated in futures::future::join_all(validations).await {
            match validated? {
                Ok(page) => pages.push(page),
                Err((path, error)) => {
                    tracing::warn!(
                        %error,
                        "LFS forward-horizon: rejected invalid chunk for path (length: {}); it will be requested again",
                        path.len()
                    );
                    self.requests.rejected(path);
                }
            }
        }
        if pages.is_empty() {
            return Ok(false);
        }

        let mut history_items = Vec::new();
        let mut data_items = Vec::new();
        let mut nodes = Vec::new();
        for page in pages {
            match page.wanted {
                Wanted::Node(node) => nodes.push(node),
                Wanted::Data { data, .. } => {
                    let received: HashSet<&Blake2b256Hash> =
                        page.data_items.iter().map(|(hash, _)| hash).collect();
                    if let Some(absent) = data.iter().find(|hash| !received.contains(hash)) {
                        return Err(CasperError::RuntimeError(format!(
                            "LFS forward-horizon: a valid chunk does not carry the leaf {}",
                            absent
                        )));
                    }
                }
            }
            self.requests.imported(&page.path);
            history_items.extend(page.history_items);
            data_items.extend(page.data_items);
        }

        let walk = self.walk.clone();
        let importer = self.state_importer.clone();
        let missing = blocking(move || {
            importer.set_history_items(history_items);
            importer.set_data_items(data_items);
            let mut walk = walk.lock().expect("walk lock");
            let mut missing = Vec::new();
            for node in &nodes {
                missing.extend(walk.node_imported(node)?);
            }
            Ok(missing)
        })
        .await?
        .map_err(walk_error)?;
        self.requests.add_nodes(missing);
        Ok(true)
    }

    async fn finish_if_complete(&mut self) -> Result<bool, CasperError> {
        if !self.requests.all_done() {
            return Ok(false);
        }
        let walk = self.walk.clone();
        let missing_data = blocking(move || walk.lock().expect("walk lock").take_missing_data())
            .await?
            .map_err(walk_error)?;
        if !missing_data.is_empty() {
            tracing::info!(
                "LFS forward-horizon: all trie nodes are held, requesting data in {} chunks",
                missing_data.len()
            );
            for missing in missing_data {
                self.requests.add(missing.path, Wanted::Data {
                    kind: missing.kind,
                    data: missing.data,
                });
            }
            return Ok(false);
        }
        for root in &self.roots {
            self.state_importer.set_root(root);
            if !(self.has_root)(root)? {
                return Err(CasperError::RuntimeError(format!(
                    "LFS forward-horizon: root {} not in store after import",
                    root
                )));
            }
        }
        Ok(true)
    }

    async fn request_next(&mut self, resend: bool) {
        let expired = if resend {
            self.requests.expired(self.max_request_timeout)
        } else {
            Vec::new()
        };
        if !expired.is_empty() {
            tracing::info!(
                "LFS forward-horizon: resending {} chunk request(s) with no response within their timeout",
                expired.len()
            );
        }
        let fresh = self.requests.next(self.request_timeout);
        let paths: Vec<StatePartPath> = expired.into_iter().chain(fresh).collect();
        let results = futures::future::join_all(
            paths
                .iter()
                .map(|path| self.request_ops.request_for_horizon_chunk(path, PAGE_SIZE)),
        )
        .await;
        let failed = results.iter().filter(|result| result.is_err()).count();
        if failed > 0 {
            tracing::warn!(
                "LFS forward-horizon: {}/{} chunk send(s) failed; paths remain pending for resend",
                failed,
                results.len()
            );
        }
    }
}

/// Streaming forward-horizon sync orchestrator. Roots already present in
/// the joiner's roots_store (per `runtime_manager.has_root`) are filtered
/// out before the stream starts.
///
/// Returns a `Stream` that emits the progress after each import and request
/// cycle and ends when every root is recorded, or on an error. Caller should
/// consume the stream to completion and check `is_finished()` on the final
/// progress to ensure no roots were left unsynced (incomplete sync = caller
/// must NOT transition to Running).
pub async fn stream<T: HorizonRequesterOps>(
    horizon_roots: Vec<Blake2b256Hash>,
    has_root: HasRootFn,
    state_importer: Arc<dyn RSpaceImporter>,
    request_ops: T,
    mut store_items_message_receiver: mpsc::Receiver<StoreItemsMessage>,
    request_timeout: Duration,
    overall_deadline: Duration,
) -> Result<
    (
        impl Stream<Item = HorizonProgress>,
        Arc<Mutex<Option<CasperError>>>,
    ),
    CasperError,
> {
    let total_input = horizon_roots.len();

    // Pre-filter roots already present (LFB root is the typical hit, since
    // `lfs_tuple_space_requester` has already imported it).
    let mut filtered: Vec<Blake2b256Hash> = Vec::with_capacity(total_input);
    let mut skipped = 0usize;
    for root in horizon_roots {
        if has_root(&root)? {
            skipped += 1;
        } else {
            filtered.push(root);
        }
    }

    tracing::info!(
        "LFS forward-horizon: starting parallel sync for {} roots ({} already present, {} input)",
        filtered.len(),
        skipped,
        total_input
    );
    // The set actually requested, after the has_root pre-filter. Pairs with
    // the walk's `emitted` list: a root in one and not the other localises the
    // loss to selection or to the pre-filter.
    tracing::debug!(
        target: "f1r3.trace.lfs",
        requested = %filtered
            .iter()
            .map(|r| hex::encode(&r.bytes()[..8]))
            .collect::<Vec<_>>()
            .join(","),
        "LFS forward-horizon: requested root set"
    );

    let mut sync = HorizonSync {
        request_ops,
        has_root,
        walk: Arc::new(Mutex::new(StateWalk::new(state_importer.clone()))),
        state_importer,
        roots: filtered,
        requests: Requests::default(),
        request_timeout,
        max_request_timeout: crate::rust::engine::lfs_block_requester::LFS_MAX_REQUEST_TIMEOUT,
    };
    sync.start().await?;

    let last_error: Arc<Mutex<Option<CasperError>>> = Arc::new(Mutex::new(None));
    let last_error_for_stream = last_error.clone();

    let stream = async_stream::stream! {
        let fail = |error: CasperError| {
            tracing::error!(error = ?error, "LFS forward-horizon: sync failed; terminating stream");
            *last_error_for_stream.lock().expect("last_error lock") = Some(error);
        };
        let mut resend_tick =
            tokio::time::interval_at(tokio::time::Instant::now() + request_timeout, request_timeout);
        resend_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut no_progress_deadline = Box::pin(tokio::time::sleep(overall_deadline));
        let mut last_progress = Instant::now();
        let mut messages = Vec::with_capacity(MAX_IN_FLIGHT_REQUESTS);

        loop {
            match sync.finish_if_complete().await {
                Ok(true) => {
                    tracing::info!(
                        "LFS forward-horizon: complete ({} chunk paths done for {} roots requested)",
                        sync.requests.done,
                        sync.roots.len()
                    );
                    yield sync.requests.progress(true);
                    return;
                }
                Ok(false) => {}
                Err(error) => {
                    fail(error);
                    break;
                }
            }
            sync.request_next(false).await;
            yield sync.requests.progress(false);

            let mut imported = false;
            while !imported {
                tokio::select! {
                    biased;

                    _ = &mut no_progress_deadline => {
                        fail(CasperError::RuntimeError(format!(
                            "LFS forward-horizon: peer made no progress within {:?}",
                            overall_deadline
                        )));
                        break;
                    }

                    received = store_items_message_receiver
                        .recv_many(&mut messages, MAX_IN_FLIGHT_REQUESTS), if !store_items_message_receiver.is_closed() || !store_items_message_receiver.is_empty() =>
                    {
                        if received > 0 {
                            match sync.import(std::mem::take(&mut messages)).await {
                                Ok(any) => imported = any,
                                Err(error) => {
                                    fail(error);
                                    break;
                                }
                            }
                            if !imported {
                                sync.request_next(false).await;
                            }
                        }
                    }

                    _ = resend_tick.tick() => {
                        let stalled_for = last_progress.elapsed();
                        if stalled_for >= request_timeout {
                            tracing::warn!(
                                "LFS forward-horizon: no imported chunk for {:?} of {:?}",
                                stalled_for, overall_deadline
                            );
                        }
                        sync.request_next(true).await;
                    }
                }
            }
            if !imported {
                break;
            }
            last_progress = Instant::now();
            no_progress_deadline
                .as_mut()
                .reset(tokio::time::Instant::now() + overall_deadline);
        }

        yield sync.requests.progress(false);
    };

    Ok((stream, last_error))
}
