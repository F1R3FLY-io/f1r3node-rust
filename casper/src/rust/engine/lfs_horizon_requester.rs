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
//! ## Streaming-parallel orchestration
//!
//! Mirrors `lfs_tuple_space_requester` and `lfs_block_requester`. Each
//! horizon root enters an `ST<StatePartPath>` state machine as
//! `Init → Requested → Received → Done`. A request loop fans out pending
//! paths in parallel, at most `MAX_IN_FLIGHT_REQUESTS` at once, while a response loop
//! demultiplexes incoming `StoreItemsMessage`s by `start_path`, applies
//! items, paginates within each root, and on the terminal cursor calls
//! `set_root`.
//!
//! Wire format (`StoreItemsMessageRequest` / `StoreItemsMessage`) is shared
//! with `lfs_tuple_space_requester`. Pagination semantics: a response with
//! `last_path == start_path` is the terminal cursor; non-terminal responses
//! enqueue `last_path` as the next chunk to request for that same root.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::Stream;
use models::rust::casper::protocol::casper_message::StoreItemsMessage;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use tokio::sync::mpsc;

use crate::rust::engine::lfs_tuple_space_requester::{
    validate_store_items, StatePartPath, StoreItems, ValidChunk, ST,
};
use crate::rust::errors::CasperError;

/// Per-request chunk size, carried in the request (the responder pages by
/// it); independent of `lfs_tuple_space_requester::PAGE_SIZE`.
pub const PAGE_SIZE: i32 = 1024;

pub const MAX_IN_FLIGHT_REQUESTS: usize = 4;

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

    async fn validate_horizon_chunk(
        &self,
        history_items: StoreItems,
        data_items: StoreItems,
        start_path: StatePartPath,
        last_path: StatePartPath,
        get_from_history: Arc<dyn RSpaceImporter>,
    ) -> Result<ValidChunk, CasperError> {
        validate_store_items(
            history_items,
            data_items,
            start_path,
            last_path,
            PAGE_SIZE,
            get_from_history,
        )
        .await
    }
}

/// Per-root pagination accounting. Used to detect the "terminal cursor on
/// first chunk + zero items" byzantine signal (peer doesn't have this root).
#[derive(Debug, Clone, Default)]
struct RootProgress {
    chunk_count: usize,
    total_history: usize,
    total_data: usize,
}

/// Closure type for "is this root already in the joiner's roots store?".
/// Decoupled from `RuntimeManager` so the orchestrator can be unit-tested
/// against a fake roots store without spinning up the runtime stack.
pub type HasRootFn = Arc<dyn Fn(&Blake2b256Hash) -> Result<bool, CasperError> + Send + Sync>;

struct HorizonStreamProcessor<T: HorizonRequesterOps> {
    request_ops: T,
    has_root: HasRootFn,
    state_importer: Arc<dyn RSpaceImporter>,
    st: Arc<Mutex<ST<StatePartPath>>>,
    /// Roots not yet verified present in the local store. This, not the path
    /// state machine, is what "the sync finished" means: paths are pagination
    /// bookkeeping and can all read Done while a root was never tagged.
    roots_pending: Arc<Mutex<HashSet<Blake2b256Hash>>>,
    /// Maps each chunk path to the SET of roots paginating through it.
    /// Initial paths `[(root, None)]` map to `{root}`. When two roots'
    /// pagination chains converge on a shared cursor (radix tries derived
    /// from sibling chain states routinely share suffix paths), the cursor
    /// maps to BOTH roots; one wire request satisfies both, and the
    /// terminal completion fires `set_root` for each.
    path_to_root: Arc<Mutex<HashMap<StatePartPath, HashSet<Blake2b256Hash>>>>,
    /// Per-root chunk-count + byte counters for byzantine-peer detection.
    root_progress: Arc<Mutex<HashMap<Blake2b256Hash, RootProgress>>>,
    request_tx: mpsc::Sender<bool>,
    in_flight: Mutex<HashMap<StatePartPath, PathTimer>>,
    request_timeout: Duration,
    max_request_timeout: Duration,
}

struct PathTimer {
    sent_at: Instant,
    timeout: Duration,
}

impl<T: HorizonRequesterOps> HorizonStreamProcessor<T> {
    async fn process_store_items_message(
        &self,
        message: StoreItemsMessage,
    ) -> Result<bool, CasperError> {
        let StoreItemsMessage {
            start_path,
            last_path,
            history_items,
            data_items,
        } = message;

        let history_items: Vec<(Blake2b256Hash, Vec<u8>)> = history_items
            .into_iter()
            .map(|(hash, bytes)| (hash, bytes.to_vec()))
            .collect();
        let data_items: Vec<(Blake2b256Hash, Vec<u8>)> = data_items
            .into_iter()
            .map(|(hash, bytes)| (hash, bytes.to_vec()))
            .collect();

        // Mark this chunk as Received in the state machine. If the path
        // wasn't previously Requested (or Init, in the fast in-memory test
        // case), treat the chunk as stale and ignore.
        let was_known = {
            let mut state = self.st.lock().expect("ST lock");
            let (new_state, is_valid) = state.received(start_path.clone());
            *state = new_state;
            is_valid
        };
        if !was_known {
            tracing::debug!("LFS forward-horizon: ignoring chunk for unknown/stale path");
            return Ok(false);
        }
        self.in_flight
            .lock()
            .expect("in_flight lock")
            .remove(&start_path);

        // Find which roots this chunk's pagination chain belongs to.
        // Multiple roots may share the same cursor — the chunk's content
        // satisfies all of them in one shot.
        let roots: Vec<Blake2b256Hash> = {
            let map = self.path_to_root.lock().expect("path_to_root lock");
            match map.get(&start_path) {
                Some(set) => set.iter().cloned().collect(),
                None => Vec::new(),
            }
        };
        if roots.is_empty() {
            tracing::warn!(
                "LFS forward-horizon: received chunk for path with no root mapping; ignoring"
            );
            return Ok(false);
        }

        if last_path == start_path && history_items.is_empty() && data_items.is_empty() {
            // Byzantine signal: terminal cursor on first chunk + no data
            // means the peer doesn't have this root. Fail loud if ANY root
            // sharing this terminal saw only an empty first chunk — that's
            // still an unrecoverable signal even if other roots happened to
            // have legitimate data on a prior path.
            let progress_map = self.root_progress.lock().expect("root_progress lock");
            for root in &roots {
                let progress = progress_map.get(root).cloned().unwrap_or_default();
                if progress.chunk_count == 0 {
                    return Err(CasperError::RuntimeError(format!(
                        "LFS forward-horizon: bootstrap signalled empty/missing root {} \
                         (terminal cursor on first chunk)",
                        root
                    )));
                }
            }
        }

        let valid = match self
            .request_ops
            .validate_horizon_chunk(
                history_items,
                data_items,
                start_path.clone(),
                last_path.clone(),
                self.state_importer.clone(),
            )
            .await
        {
            Ok(valid) => valid,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "LFS forward-horizon: rejected invalid chunk for path (length: {}); it will be requested again",
                    start_path.len()
                );
                {
                    let mut state = self.st.lock().expect("ST lock");
                    *state = state.rejected(start_path);
                }
                let _ = self.request_tx.try_send(false);
                return Ok(false);
            }
        };

        let history_count = valid.history_items.len();
        let data_count = valid.data_items.len();
        let complete = valid.complete;
        self.state_importer.set_history_items(valid.history_items);
        self.state_importer.set_data_items(valid.data_items);

        // Update per-root progress for every root sharing this chunk.
        {
            let mut progress_map = self.root_progress.lock().expect("root_progress lock");
            for root in &roots {
                let entry = progress_map.entry(root.clone()).or_default();
                entry.chunk_count += 1;
                entry.total_history += history_count;
                entry.total_data += data_count;
            }
        }

        if last_path == start_path || complete {
            // Record the root tag in roots_store and verify the import
            // reconstructed each expected root.
            for root in &roots {
                self.state_importer.set_root(root);
                let now_have = (self.has_root)(root)?;
                if !now_have {
                    return Err(CasperError::RuntimeError(format!(
                        "LFS forward-horizon: root {} not in store after import; \
                         peer shipped invalid data",
                        root
                    )));
                }
                // Verified present: this root is synced. Completion is measured
                // over roots and not over chunk paths, because paths are an
                // implementation detail of pagination and a root can be stranded
                // while every path reads Done.
                self.roots_pending
                    .lock()
                    .expect("roots_pending lock")
                    .remove(root);
                let progress = {
                    let progress_map = self.root_progress.lock().expect("root_progress lock");
                    progress_map.get(root).cloned().unwrap_or_default()
                };
                tracing::debug!(
                    target: "f1r3.trace.lfs",
                    "LFS forward-horizon: completed root {} ({} chunks, {} history, {} data)",
                    root,
                    progress.chunk_count,
                    progress.total_history,
                    progress.total_data
                );
            }

            // Mark the path Done and free its multi-root mapping. ST
            // tracks paths, not roots, so a single done() retires the
            // shared chunk for all associated roots.
            {
                let mut state = self.st.lock().expect("ST lock");
                let new_state = state.done(start_path.clone());
                *state = new_state;
            }
            {
                let mut map = self.path_to_root.lock().expect("path_to_root lock");
                map.remove(&start_path);
            }

            // Trigger another request cycle so any unstarted Init paths get
            // sent now that one has finished.
            let _ = self.request_tx.try_send(false);
        } else {
            // Non-terminal: continuation cursor inherits the full root set.
            // If `last_path` is already tracked (another root's chunk
            // already converged on the same cursor), merge into the
            // existing set so its terminal completion fires set_root for
            // every root, not just the first writer.
            {
                let mut map = self.path_to_root.lock().expect("path_to_root lock");
                let entry = map.entry(last_path.clone()).or_default();
                for root in &roots {
                    entry.insert(root.clone());
                }
            }
            {
                let mut state = self.st.lock().expect("ST lock");
                // Re-arm, not add: this cursor may be one another root already
                // finished, and `add` would leave it Done — successor never
                // re-walked, this root's set_root never fired, every entry Done
                // so the run reports success without it.
                let new_state = state.re_arm(last_path);
                // Mark the just-processed chunk Done so it doesn't keep
                // counting against is_finished. (Pagination continues via
                // the re-armed Init entry.)
                let new_state = new_state.done(start_path.clone());
                *state = new_state;
            }
            {
                let mut map = self.path_to_root.lock().expect("path_to_root lock");
                map.remove(&start_path);
            }

            let _ = self.request_tx.try_send(false);
        }

        Ok(true)
    }

    async fn request_next(&self, resend: bool) -> Result<(), CasperError> {
        // Snapshot is_finished and pull next-paths under one lock.
        let (is_finished, fresh) = {
            let mut state = self.st.lock().expect("ST lock");
            if state.is_finished() {
                (true, Vec::new())
            } else {
                let (new_state, paths) = state.get_next_bounded(false, MAX_IN_FLIGHT_REQUESTS);
                *state = new_state;
                (false, paths)
            }
        };
        if is_finished {
            return Ok(());
        }

        let now = Instant::now();
        let expired: Vec<StatePartPath> = {
            let mut in_flight = self.in_flight.lock().expect("in_flight lock");
            let expired = if resend {
                in_flight
                    .iter_mut()
                    .filter(|(_, timer)| now.duration_since(timer.sent_at) >= timer.timeout)
                    .map(|(path, timer)| {
                        timer.sent_at = now;
                        timer.timeout = timer
                            .timeout
                            .saturating_mul(2)
                            .min(self.max_request_timeout);
                        path.clone()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            for path in &fresh {
                in_flight.insert(path.clone(), PathTimer {
                    sent_at: now,
                    timeout: self.request_timeout,
                });
            }
            expired
        };
        if fresh.is_empty() && expired.is_empty() {
            return Ok(());
        }

        if !expired.is_empty() {
            tracing::info!(
                "LFS forward-horizon: resending {} chunk request(s) with no response within their timeout",
                expired.len()
            );
        }
        if !fresh.is_empty() {
            tracing::debug!(
                "LFS forward-horizon: dispatching {} chunk requests in parallel",
                fresh.len()
            );
        }

        let paths: Vec<StatePartPath> = expired.into_iter().chain(fresh).collect();
        let request_futures: Vec<_> = paths
            .iter()
            .map(|path| async move {
                self.request_ops
                    .request_for_horizon_chunk(path, PAGE_SIZE)
                    .await
            })
            .collect();
        let results = futures::future::join_all(request_futures).await;
        let failed = results.iter().filter(|result| result.is_err()).count();
        if failed > 0 {
            tracing::warn!(
                "LFS forward-horizon: {}/{} chunk send(s) failed; paths remain pending for resend",
                failed,
                results.len()
            );
        }

        Ok(())
    }
}

/// Streaming forward-horizon sync orchestrator. Roots already present in
/// the joiner's roots_store (per `runtime_manager.has_root`) are filtered
/// out before the stream starts. The remaining roots are enqueued in `ST`
/// and processed in parallel via the streaming select-loop below.
///
/// Returns a `Stream<Item = ST<StatePartPath>>` that emits an updated state
/// snapshot on each response/request cycle and terminates when all roots
/// reach `Done`. Caller should consume the stream to completion and check
/// `is_finished()` on the final state to ensure no roots were left
/// unsynced (incomplete sync = caller must NOT transition to Running).
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
        impl Stream<Item = ST<StatePartPath>>,
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

    // Build initial ST: one path per root, of the form `[(root, None)]`.
    // Each initial path uniquely identifies its root, so the singleton
    // sets here will only ever grow when pagination cursors converge.
    let mut initial_paths: Vec<StatePartPath> = Vec::with_capacity(filtered.len());
    let mut path_to_root_init: HashMap<StatePartPath, HashSet<Blake2b256Hash>> = HashMap::new();
    for root in &filtered {
        let path: StatePartPath = vec![(root.clone(), None)];
        let mut set = HashSet::new();
        set.insert(root.clone());
        path_to_root_init.insert(path.clone(), set);
        initial_paths.push(path);
    }

    let st = Arc::new(Mutex::new(ST::new(initial_paths)));
    let path_to_root = Arc::new(Mutex::new(path_to_root_init));
    let root_progress = Arc::new(Mutex::new(HashMap::new()));
    let roots_pending: Arc<Mutex<HashSet<Blake2b256Hash>>> =
        Arc::new(Mutex::new(filtered.iter().cloned().collect()));

    let requested_root_count = filtered.len();

    // Bounded request queue (cap 2 — one resend + one new). Matches
    // tuple_space_requester::stream channel sizing.
    let (request_tx, mut request_rx) = mpsc::channel::<bool>(2);

    // Tracked-out-of-band so byzantine-detection / has_root errors inside
    // the response handler can be surfaced to the caller via the wrapper
    // below. async_stream! yields ST snapshots only.
    let last_error: Arc<Mutex<Option<CasperError>>> = Arc::new(Mutex::new(None));

    let max_request_timeout = crate::rust::engine::lfs_block_requester::LFS_MAX_REQUEST_TIMEOUT;

    let processor = Arc::new(HorizonStreamProcessor {
        request_ops,
        has_root,
        state_importer,
        st: st.clone(),
        roots_pending: roots_pending.clone(),
        path_to_root,
        root_progress,
        request_tx: request_tx.clone(),
        in_flight: Mutex::new(HashMap::new()),
        request_timeout,
        max_request_timeout,
    });

    // Empty-input shortcut: yield a finished ST and exit so callers see
    // the same streaming contract regardless of input size.
    let nothing_to_do = filtered.is_empty();
    let last_error_for_stream = last_error.clone();
    let st_for_stream = st.clone();
    let roots_pending_for_stream = roots_pending.clone();

    // Initial request kick-off (only if we have roots to fetch).
    if !nothing_to_do {
        request_tx.send(false).await.map_err(|_| {
            CasperError::RuntimeError(
                "LFS forward-horizon: initial request enqueue failed".to_string(),
            )
        })?;
    }

    let stream = async_stream::stream! {
        if nothing_to_do {
            let final_state = st_for_stream.lock().expect("ST lock").clone();
            yield final_state;
            return;
        }

        let mut resend_tick =
            tokio::time::interval_at(tokio::time::Instant::now() + request_timeout, request_timeout);
        resend_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut no_progress_deadline = Box::pin(tokio::time::sleep(overall_deadline));
        let mut last_progress = std::time::Instant::now();
        let outstanding_roots =
            || roots_pending_for_stream.lock().expect("roots_pending lock").len();

        loop {
            tokio::select! {
                // biased: prefer responses (drain side) over new requests so
                // we don't starve the demux while issuing more sends.
                biased;

                _ = &mut no_progress_deadline => {
                    tracing::error!(
                        "LFS forward-horizon: no progress for {:?} (deadline {:?}); giving up",
                        last_progress.elapsed(),
                        overall_deadline
                    );
                    *last_error_for_stream.lock().expect("last_error lock") =
                        Some(CasperError::RuntimeError(format!(
                            "LFS forward-horizon: peer made no progress within {:?}",
                            overall_deadline
                        )));
                    break;
                }

                Some(message) = store_items_message_receiver.recv() => {
                    let imported = match processor.process_store_items_message(message).await {
                        Ok(imported) => imported,
                        Err(e) => {
                            tracing::error!(error = ?e, "LFS forward-horizon store items processing failed; terminating stream");
                            *last_error_for_stream.lock().expect("last_error lock") = Some(e);
                            break;
                        }
                    };

                    let current_state = st_for_stream
                        .lock()
                        .expect("ST lock")
                        .clone();
                    if imported {
                        last_progress = std::time::Instant::now();
                        no_progress_deadline
                            .as_mut()
                            .reset(tokio::time::Instant::now() + overall_deadline);
                    }
                    let is_finished = current_state.is_finished();

                    if is_finished {
                        let outstanding = outstanding_roots();
                        if outstanding > 0 {
                            // Every path is Done and a root is still unsynced:
                            // the path bookkeeping lost one. Reporting success
                            // here is what makes the failure arrive later, as an
                            // unknown root during replay, on a node that has
                            // already discarded the sync.
                            tracing::error!(
                                outstanding,
                                requested = requested_root_count,
                                "LFS forward-horizon: all chunk paths done but roots remain unsynced"
                            );
                            *last_error_for_stream.lock().expect("last_error lock") =
                                Some(CasperError::RuntimeError(format!(
                                    "LFS forward-horizon: {} of {} roots unsynced although every \
                                     chunk path completed",
                                    outstanding, requested_root_count
                                )));
                            break;
                        }
                        // `ST` is keyed by chunk PATH, not by root: pagination
                        // adds paths as cursors advance, so this count exceeds
                        // the root count and must not be reported as roots.
                        tracing::info!(
                            "LFS forward-horizon: complete ({} chunk paths done for {} roots requested)",
                            current_state.len(),
                            requested_root_count
                        );
                        yield current_state;
                        break;
                    }

                    yield current_state;
                }

                Some(resend_flag) = request_rx.recv() => {
                    match processor.request_next(resend_flag).await {
                        Ok(()) => {
                            let current_state = st_for_stream
                                .lock()
                                .expect("ST lock")
                                .clone();
                            if current_state.is_finished() && outstanding_roots() == 0 {
                                yield current_state;
                                break;
                            }

                            yield current_state;
                        }
                        Err(e) => {
                            tracing::warn!(error = ?e, "LFS forward-horizon request dispatch error; resend will retry");
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
                    if request_tx.try_send(true).is_err() {
                        tracing::warn!(
                            "LFS forward-horizon: request queue full on resend trigger"
                        );
                    }
                }
            }
        }

        // Final state emission so the caller sees the terminal ST snapshot
        // even if the loop exited via break (error path).
        let final_state = st_for_stream
            .lock()
            .expect("ST lock")
            .clone();
        yield final_state;
    };

    // Keep last_error alive in the same scope as the stream by wrapping it
    // in a struct... actually, the stream already captures last_error_for_stream
    // by move and writes to it. The wrapper below reads it. We stash an Arc
    // clone in the stream's parent scope (here) — but stream() returns the
    // stream and drops the local Arc. That's fine: last_error_for_stream
    // (moved into the async block) shares the Arc with the wrapper's clone.

    // last_error is captured by the async block via last_error_for_stream;
    // however, we don't expose it through this stream's public surface.
    // Callers detect failure via `final ST.is_finished() == false`.
    // Specific error context is logged via `tracing::error!` in the loop.
    Ok((stream, last_error))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ST<Key> state machine ────────────────────────────────────────────
    //
    // Mirrors the `Init → Requested → Received → Done` lifecycle that the
    // streaming orchestrator drives every horizon root through. These tests
    // pin the algebraic semantics so future changes to the orchestrator or
    // the `lfs_tuple_space_requester` parallel structure can't silently
    // diverge from the contract the stream loop assumes.

    #[test]
    fn st_new_initializes_all_entries_as_init() {
        let st: ST<i32> = ST::new(vec![1, 2, 3]);
        assert_eq!(st.len(), 3);
        assert_eq!(st.done_count(), 0);
        assert!(!st.is_finished());
    }

    #[test]
    fn st_add_inserts_new_keys_as_init_and_leaves_existing() {
        // Existing key in Requested state must NOT be reset to Init by add().
        let st: ST<i32> = ST::new(vec![1]);
        let (st, _) = st.get_next(false); // 1 -> Requested
        let mut new_keys = HashSet::new();
        new_keys.insert(2);
        new_keys.insert(1); // existing
        let st = st.add(new_keys);
        // 2 should be Init now; 1 should still be Requested → only 2 picked.
        let (_, requested) = st.get_next(false);
        assert_eq!(requested, vec![2]);
    }

    /// The companion boundary: plain `add` never reopens a Done key — only
    /// `re_arm` does. A Done key re-added stays Done and the run stays
    /// finished.
    #[test]
    fn st_add_leaves_done_keys_done() {
        let st: ST<i32> = ST::new(vec![1]);
        let (st, _) = st.get_next(false);
        let (st, _) = st.received(1);
        let st = st.done(1);
        let mut existing = HashSet::new();
        existing.insert(1);
        assert!(st.add(existing).is_finished());
    }

    #[test]
    fn st_get_next_without_resend_picks_only_init() {
        let st: ST<i32> = ST::new(vec![1, 2]);
        let (st, batch) = st.get_next(false);
        let mut sorted = batch.clone();
        sorted.sort();
        assert_eq!(sorted, vec![1, 2]);
        // Calling again without resend yields nothing: both are Requested.
        let (_, batch2) = st.get_next(false);
        assert!(batch2.is_empty());
    }

    #[test]
    fn st_get_next_with_resend_picks_init_and_requested() {
        let st: ST<i32> = ST::new(vec![1]);
        let (st, _) = st.get_next(false); // 1 -> Requested
        let (_, batch) = st.get_next(true); // resend: pick Requested again
        assert_eq!(batch, vec![1]);
    }

    #[test]
    fn st_get_next_bounded_keeps_at_most_limit_requested() {
        let st: ST<i32> = ST::new((1..=10).collect());
        let (st, first) = st.get_next_bounded(false, 3);
        assert_eq!(first.len(), 3);
        let (st, none) = st.get_next_bounded(false, 3);
        assert!(none.is_empty());

        let (st, _) = st.received(first[0]);
        let (st, next) = st.get_next_bounded(false, 3);
        assert_eq!(next.len(), 1);
        assert!(!first.contains(&next[0]));

        let (_, resent) = st.get_next_bounded(true, 3);
        let mut resent = resent;
        resent.sort();
        let mut expected = vec![first[1], first[2], next[0]];
        expected.sort();
        assert_eq!(resent, expected);
    }

    #[test]
    fn st_received_valid_only_on_requested_or_init() {
        let st: ST<i32> = ST::new(vec![1, 2]);
        // Init: received() accepts (handles fast in-memory test scenarios
        // where responses arrive before the request loop runs).
        let (st, valid) = st.received(1);
        assert!(valid);
        // Requested: received() accepts.
        let (st, _) = st.get_next(false); // 2 -> Requested
        let (st, valid) = st.received(2);
        assert!(valid);
        // Already-Received: received() rejects (idempotent).
        let (_, valid) = st.received(1);
        assert!(!valid);
    }

    #[test]
    fn st_done_only_valid_on_received() {
        let st: ST<i32> = ST::new(vec![1]);
        // Init -> done() is a no-op (key NOT advanced).
        let st = st.done(1);
        assert!(!st.is_finished());
        // After received() -> done() advances.
        let (st, _) = st.received(1);
        let st = st.done(1);
        assert!(st.is_finished());
    }

    #[test]
    fn st_is_finished_only_when_all_done() {
        let st: ST<i32> = ST::new(vec![1, 2]);
        let (st, _) = st.received(1);
        let st = st.done(1);
        assert!(!st.is_finished()); // 2 still Init
        let (st, _) = st.received(2);
        let st = st.done(2);
        assert!(st.is_finished());
    }

    #[test]
    fn st_done_count_matches_done_entries() {
        let st: ST<i32> = ST::new(vec![1, 2, 3]);
        let (st, _) = st.received(1);
        let st = st.done(1);
        let (st, _) = st.received(2);
        let st = st.done(2);
        assert_eq!(st.done_count(), 2);
        assert_eq!(st.len() - st.done_count(), 1);
    }

    // ── stream() behavioral tests with mock HorizonRequesterOps ─────────
    //
    // These tests construct the orchestrator with a mock network layer so
    // we can assert on:
    //   - skip-already-present pre-filter (mock has_root returns true)
    //   - terminal cursor handling (single-chunk happy path)
    //   - byzantine peer detection (terminal+empty on first chunk)
    //   - pagination (non-terminal cursor enqueues continuation)
    //   - parallel fan-out (mock counts max simultaneous in-flight requests)
    //
    // The mock RSpaceImporter is a no-op since the orchestrator only uses
    // it as a passive sink for items we feed via canned responses.

    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use futures::StreamExt;
    use rspace_plus_plus::rspace::shared::trie_importer::TrieImporter;
    use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
    use tokio::sync::mpsc as test_mpsc;

    /// No-op RSpaceImporter — orchestrator just calls set_history_items /
    /// set_data_items / set_root with the bytes we feed it; tests don't
    /// inspect what was imported, only what the stream did with the calls.
    struct NoopImporter;

    impl TrieImporter for NoopImporter {
        fn set_history_items(&self, _data: Vec<(Blake2b256Hash, Vec<u8>)>) {}
        fn set_data_items(&self, _data: Vec<(Blake2b256Hash, Vec<u8>)>) {}
        fn set_root(&self, _key: &Blake2b256Hash) {}
    }

    impl RSpaceImporter for NoopImporter {
        fn get_history_item(&self, _hash: Blake2b256Hash) -> Option<Vec<u8>> { None }
    }

    /// Recording RSpaceImporter — captures every root passed to `set_root`.
    /// Pair with a `has_root` closure that reads from the same set so the
    /// orchestrator's post-import verification reflects what was recorded.
    struct RecordingImporter {
        recorded: Arc<Mutex<HashSet<Blake2b256Hash>>>,
    }

    impl RecordingImporter {
        fn new() -> (Self, Arc<Mutex<HashSet<Blake2b256Hash>>>) {
            let recorded = Arc::new(Mutex::new(HashSet::new()));
            (
                Self {
                    recorded: recorded.clone(),
                },
                recorded,
            )
        }
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

    /// Mock HorizonRequesterOps: records every send and tracks max
    /// simultaneous in-flight requests. Each send increments the in-flight
    /// counter and returns immediately; the test loop is responsible for
    /// feeding canned responses and decrementing. For the parallel-fan-out
    /// test we observe `max_in_flight` AFTER request_next has dispatched
    /// the batch but BEFORE responses arrive.
    struct MockOps {
        sends: Arc<Mutex<Vec<StatePartPath>>>,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    impl MockOps {
        fn new() -> Self {
            Self {
                sends: Arc::new(Mutex::new(Vec::new())),
                in_flight: Arc::new(AtomicUsize::new(0)),
                max_in_flight: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn handles(&self) -> (Arc<Mutex<Vec<StatePartPath>>>, Arc<AtomicUsize>) {
            (self.sends.clone(), self.max_in_flight.clone())
        }
    }

    #[async_trait]
    impl HorizonRequesterOps for MockOps {
        async fn request_for_horizon_chunk(
            &self,
            path: &StatePartPath,
            _page_size: i32,
        ) -> Result<(), CasperError> {
            self.sends.lock().unwrap().push(path.clone());
            let now = self.in_flight.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, AtomicOrdering::SeqCst);
            // Simulate the request being "outstanding" — the test loop
            // decrements via a shared counter when it feeds a response.
            // We don't decrement here because the response handler
            // is what unblocks the next request cycle.
            Ok(())
        }

        async fn validate_horizon_chunk(
            &self,
            history_items: StoreItems,
            data_items: StoreItems,
            _start_path: StatePartPath,
            _last_path: StatePartPath,
            _get_from_history: Arc<dyn RSpaceImporter>,
        ) -> Result<ValidChunk, CasperError> {
            Ok(ValidChunk {
                history_items,
                data_items,
                complete: false,
            })
        }
    }

    fn hash_for(seed: u8) -> Blake2b256Hash {
        let mut bytes = vec![0u8; 32];
        bytes[0] = seed;
        Blake2b256Hash::from_bytes(bytes)
    }

    fn make_response(
        start_path: StatePartPath,
        last_path: StatePartPath,
        history_item: Option<Blake2b256Hash>,
    ) -> StoreItemsMessage {
        let history_items = history_item
            .map(|h| vec![(h, prost::bytes::Bytes::from_static(b"x"))])
            .unwrap_or_default();
        StoreItemsMessage {
            start_path,
            last_path,
            history_items,
            data_items: vec![],
        }
    }

    /// Helper to drain an `impl Stream` to its final `ST`.
    async fn drain<S>(stream: S) -> Option<ST<StatePartPath>>
    where S: Stream<Item = ST<StatePartPath>> {
        let mut stream = Box::pin(stream);
        let mut last = None;
        while let Some(st) = stream.next().await {
            last = Some(st);
        }
        last
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_skips_already_present_roots() {
        // has_root returns true for all roots → filtered set is empty.
        // Stream yields a finished ST and exits without any request.
        let r1 = hash_for(1);
        let r2 = hash_for(2);
        let has_root: HasRootFn = Arc::new(|_| Ok(true));
        let importer: Arc<dyn RSpaceImporter> = Arc::new(NoopImporter);
        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (_tx, rx) = test_mpsc::channel::<StoreItemsMessage>(2);

        let (stream, _) = stream(
            vec![r1, r2],
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        assert!(final_st.is_finished());
        assert_eq!(final_st.len(), 0);
        assert!(
            sends.lock().unwrap().is_empty(),
            "no requests should have been sent for already-present roots"
        );
    }

    /// Two roots whose pagination converges on one cursor must BOTH be synced.
    ///
    /// Adjacent block states share almost all of their trie, so cursors
    /// converging is ordinary, not exotic. When the second root arrives at a
    /// cursor the first has already finished, `ST::add` sees the path present
    /// and leaves it `Done`: the successor is never re-armed, the chunk is never
    /// reprocessed, and `set_root` never fires for the second root. Every path
    /// entry is `Done`, so the run reports success while the root is missing
    /// from the store — and the failure only surfaces later, as an unknown root
    /// during replay, on a node that has already thrown away the sync.
    ///
    /// Observed live: 106 roots requested, 92 completed, "complete" logged, and
    /// a joiner wedged on the pre-state of the first block it tried to replay.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_converged_cursor_does_not_strand_its_root() {
        let first = hash_for(1);
        let second = hash_for(2);
        let shared_cursor: StatePartPath = vec![(hash_for(3), Some(0))];
        let first_start: StatePartPath = vec![(first.clone(), None)];
        let second_start: StatePartPath = vec![(second.clone(), None)];

        let (recording, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(recording);
        // Pre-filter sees nothing present; post-import verification sees exactly
        // what `set_root` recorded.
        let has_root: HasRootFn = {
            let recorded = recorded.clone();
            Arc::new(move |query| Ok(recorded.lock().unwrap().contains(query)))
        };

        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(8);
        // The first root paginates to the shared cursor and finishes there.
        tx.send(make_response(
            first_start,
            shared_cursor.clone(),
            Some(hash_for(4)),
        ))
        .await
        .unwrap();
        tx.send(make_response(
            shared_cursor.clone(),
            shared_cursor.clone(),
            Some(hash_for(5)),
        ))
        .await
        .unwrap();
        // The second root then arrives at the same cursor, now already Done.
        tx.send(make_response(
            second_start,
            shared_cursor.clone(),
            Some(hash_for(6)),
        ))
        .await
        .unwrap();
        // The cursor re-armed for the second root is walked again, and this
        // time terminates for it.
        tx.send(make_response(
            shared_cursor.clone(),
            shared_cursor.clone(),
            Some(hash_for(5)),
        ))
        .await
        .unwrap();
        drop(tx);

        let (stream, _) = stream(
            vec![first.clone(), second.clone()],
            has_root,
            importer,
            MockOps::new(),
            rx,
            Duration::from_secs(1),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        let _ = drain(stream).await;

        let synced = recorded.lock().unwrap().clone();
        assert!(
            synced.contains(&second),
            "the second root converged onto a cursor the first had finished, so its \
             state was never tagged; a run that leaves a root unsynced must not \
             finish. Roots synced: {:?}",
            synced.len()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_walk_over_finished_cursors_holds_off_the_deadline() {
        let first = hash_for(1);
        let second = hash_for(2);
        let cursors: Vec<StatePartPath> =
            (3..6).map(|seed| vec![(hash_for(seed), Some(0))]).collect();
        let chain = |start: StatePartPath| {
            let mut paths = vec![start];
            paths.extend(cursors.iter().cloned());
            let last = paths.last().unwrap().clone();
            let mut chunks: Vec<StoreItemsMessage> = paths
                .windows(2)
                .map(|pair| make_response(pair[0].clone(), pair[1].clone(), Some(hash_for(9))))
                .collect();
            chunks.push(make_response(last.clone(), last, Some(hash_for(9))));
            chunks
        };
        let first_chain = chain(vec![(first.clone(), None)]);
        let second_chain = chain(vec![(second.clone(), None)]);

        let (recording, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(recording);
        let has_root: HasRootFn = {
            let recorded = recorded.clone();
            Arc::new(move |query| Ok(recorded.lock().unwrap().contains(query)))
        };
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(16);
        let feeder = tokio::spawn(async move {
            for chunk in first_chain {
                let _ = tx.send(chunk).await;
            }
            for chunk in second_chain {
                tokio::time::sleep(Duration::from_millis(400)).await;
                let _ = tx.send(chunk).await;
            }
            tx
        });

        let (stream, last_error) = stream(
            vec![first.clone(), second.clone()],
            has_root,
            importer,
            MockOps::new(),
            rx,
            Duration::from_secs(5),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let final_state = drain(stream).await;
        let _tx = feeder.await.unwrap();

        assert!(
            last_error.lock().unwrap().is_none(),
            "the deadline fired while chunks were imported: {:?}",
            last_error.lock().unwrap()
        );
        assert!(final_state.expect("a final state").is_finished());
        assert!(recorded.lock().unwrap().contains(&second));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_completes_on_terminal_cursor_with_data() {
        // Single root, single chunk. Response is terminal (last_path ==
        // start_path) and contains one history item → root marked Done.
        let root = hash_for(7);
        let importer: Arc<dyn RSpaceImporter> = Arc::new(NoopImporter);
        // has_root flips: false on the pre-filter call (so the root gets
        // added to ST), then true on the post-import verification (so the
        // import is accepted as having reconstructed the root).
        let post_import_has_root: HasRootFn = {
            let r = root.clone();
            let imported = Arc::new(Mutex::new(false));
            Arc::new(move |q| {
                if *q == r && *imported.lock().unwrap() {
                    Ok(true)
                } else {
                    let was = *imported.lock().unwrap();
                    *imported.lock().unwrap() = true;
                    Ok(was)
                }
            })
        };
        let ops = MockOps::new();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(4);

        // Pre-feed the response.
        let start = vec![(root.clone(), None)];
        tx.send(make_response(
            start.clone(),
            start.clone(),
            Some(hash_for(8)),
        ))
        .await
        .unwrap();
        // Drop tx so the receiver eventually sees channel-closed if loop
        // doesn't terminate via is_finished.
        drop(tx);

        let (stream, _) = stream(
            vec![root],
            post_import_has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        assert!(
            final_st.is_finished(),
            "single-root happy path must mark all paths Done; final state: {:?}",
            final_st
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_detects_byzantine_peer_terminal_empty_first_chunk() {
        // Single root. Response is terminal AND empty (no history items,
        // no data items) on the FIRST chunk. This is the contract for
        // "peer doesn't have this root" — orchestrator must error out.
        let root = hash_for(5);
        let has_root: HasRootFn = Arc::new(|_| Ok(false));
        let importer: Arc<dyn RSpaceImporter> = Arc::new(NoopImporter);
        let ops = MockOps::new();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(4);

        let start = vec![(root.clone(), None)];
        tx.send(make_response(start.clone(), start.clone(), None))
            .await
            .unwrap();
        drop(tx);

        let (stream, _) = stream(
            vec![root],
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        // Byzantine signal causes the loop to break before marking Done →
        // final ST is NOT finished. Caller's loud-fail check then triggers.
        assert!(
            !final_st.is_finished(),
            "byzantine peer (terminal+empty) must NOT mark the root Done"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_paginates_non_terminal_responses() {
        // Single root, two chunks. Response 1 is non-terminal: last_path
        // points at a continuation. Response 2 is terminal at that
        // continuation. Orchestrator must enqueue Response 2's start_path
        // (= Response 1's last_path) as a new Init entry, then fan a
        // request out for it, then mark Done on terminal arrival.
        //
        // Responses are fed by a separate task with a small yield between
        // sends so the orchestrator's `request_next` cycle can fire for the
        // continuation path before response 2 lands. Without this, the
        // biased-response select drains both responses before any request
        // ever ships and the test would assert against an empty sends log.
        let root = hash_for(9);
        let cont_path = vec![(hash_for(10), Some(0u8))];
        let post_import_has_root: HasRootFn = {
            let r = root.clone();
            let imported = Arc::new(Mutex::new(false));
            Arc::new(move |q| {
                if *q == r && *imported.lock().unwrap() {
                    Ok(true)
                } else {
                    let was = *imported.lock().unwrap();
                    *imported.lock().unwrap() = true;
                    Ok(was)
                }
            })
        };
        let importer: Arc<dyn RSpaceImporter> = Arc::new(NoopImporter);
        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(8);

        let start = vec![(root.clone(), None)];
        let resp1 = make_response(start.clone(), cont_path.clone(), Some(hash_for(11)));
        let resp2 = make_response(cont_path.clone(), cont_path.clone(), Some(hash_for(12)));

        let feeder_sends = sends.clone();
        let feeder_start = start.clone();
        let feeder_cont = cont_path.clone();
        let feeder = tokio::spawn(async move {
            // Wait until the initial start_path has actually been requested.
            loop {
                if feeder_sends.lock().unwrap().contains(&feeder_start) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            tx.send(resp1).await.unwrap();
            // Wait for the continuation path to be requested (proves the
            // orchestrator enqueued it after processing response 1).
            loop {
                if feeder_sends.lock().unwrap().contains(&feeder_cont) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            tx.send(resp2).await.unwrap();
            drop(tx);
        });

        let (stream, _) = stream(
            vec![root],
            post_import_has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        feeder.await.unwrap();
        assert!(
            final_st.is_finished(),
            "after 2 chunks (paginated) the root should be Done"
        );
        let sent_paths = sends.lock().unwrap().clone();
        assert!(sent_paths.contains(&start), "initial start_path was sent");
        assert!(
            sent_paths.contains(&cont_path),
            "continuation path (= response 1's last_path) was enqueued and sent"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_fans_out_in_parallel() {
        // Four roots. Don't feed any responses. Observe `max_in_flight`
        // after the request loop dispatches the initial batch. With
        // try_join_all the orchestrator dispatches all 4 requests
        // simultaneously before any response would be needed.
        let roots: Vec<Blake2b256Hash> = (1..=4u8).map(hash_for).collect();
        let has_root: HasRootFn = Arc::new(|_| Ok(false));
        let importer: Arc<dyn RSpaceImporter> = Arc::new(NoopImporter);
        let ops = MockOps::new();
        let (_, max_in_flight) = ops.handles();
        let (_tx, rx) = test_mpsc::channel::<StoreItemsMessage>(8);

        // Launch the stream and let it pump the initial request cycle.
        let (stream, _) = stream(
            roots.clone(),
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        // Pull one item (the post-request_next snapshot) which proves the
        // initial fan-out completed. We don't drain to completion because
        // there are no responses.
        let mut stream = Box::pin(stream);
        let _first = stream.next().await;

        let observed = max_in_flight.load(AtomicOrdering::SeqCst);
        assert_eq!(
            observed, 4,
            "orchestrator must dispatch all 4 root requests in parallel; \
             max_in_flight={}",
            observed
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_keeps_at_most_max_in_flight_requests_outstanding() {
        let roots: Vec<Blake2b256Hash> = (1..=100u8).map(hash_for).collect();
        let (recording, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(recording);
        let has_root: HasRootFn = {
            let recorded = recorded.clone();
            Arc::new(move |query| Ok(recorded.lock().unwrap().contains(query)))
        };
        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(256);

        let (stream, _) = stream(
            roots.clone(),
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(30),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let mut stream = Box::pin(stream);

        let mut answered = 0usize;
        let mut max_outstanding = 0usize;
        let mut last = None;
        while let Some(st) = stream.next().await {
            let sent: Vec<StatePartPath> = sends.lock().unwrap().clone();
            max_outstanding = max_outstanding.max(sent.len() - answered);
            for path in &sent[answered..] {
                tx.send(make_response(path.clone(), path.clone(), Some(hash_for(0))))
                    .await
                    .unwrap();
            }
            answered = sent.len();
            last = Some(st);
        }

        assert!(
            max_outstanding <= MAX_IN_FLIGHT_REQUESTS,
            "at most {} requests may be outstanding, observed {}",
            MAX_IN_FLIGHT_REQUESTS,
            max_outstanding
        );
        assert!(last.expect("stream yields a final state").is_finished());
        assert_eq!(recorded.lock().unwrap().len(), roots.len());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_lost_response_is_resent_with_backoff_while_other_roots_progress() {
        const STEPS: u8 = 40;
        let walking = hash_for(1);
        let lost = hash_for(2);
        let lost_path: StatePartPath = vec![(lost.clone(), None)];
        let (recording, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(recording);
        let has_root: HasRootFn = {
            let recorded = recorded.clone();
            Arc::new(move |query| Ok(recorded.lock().unwrap().contains(query)))
        };
        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(256);

        let (stream, _) = stream(
            vec![walking.clone(), lost.clone()],
            has_root,
            importer,
            ops,
            rx,
            Duration::from_millis(50),
            Duration::from_secs(20),
        )
        .await
        .unwrap();

        let responder = {
            let sends = sends.clone();
            let recorded = recorded.clone();
            let walking = walking.clone();
            let lost_path = lost_path.clone();
            tokio::spawn(async move {
                let mut seen = 0usize;
                loop {
                    let sent: Vec<StatePartPath> = sends.lock().unwrap().clone();
                    for path in &sent[seen..] {
                        if path[0].0 != walking {
                            continue;
                        }
                        let step = path[0].1.map_or(0, |i| i);
                        let next = if step == STEPS {
                            path.clone()
                        } else {
                            vec![(walking.clone(), Some(step + 1))]
                        };
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        let _ = tx
                            .send(make_response(path.clone(), next, Some(hash_for(0))))
                            .await;
                    }
                    seen = sent.len();
                    if recorded.lock().unwrap().contains(&walking) {
                        let lost_sends = sent.iter().filter(|p| **p == lost_path).count();
                        let _ = tx
                            .send(make_response(
                                lost_path.clone(),
                                lost_path.clone(),
                                Some(hash_for(0)),
                            ))
                            .await;
                        return lost_sends;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
        };

        let final_st = tokio::time::timeout(Duration::from_secs(20), drain(stream))
            .await
            .expect("the sync ends")
            .expect("stream yields a final state");
        let lost_sends = responder.await.unwrap();

        assert!(final_st.is_finished());
        assert!(recorded.lock().unwrap().contains(&lost));
        assert!(
            lost_sends >= 2,
            "a path with no response must be resent while other roots import chunks; sent {} time(s)",
            lost_sends
        );
        assert!(
            lost_sends <= 10,
            "resends of one path must back off; sent {} times while one root walked {} chunks",
            lost_sends,
            STEPS
        );
    }

    /// Regression: two roots whose first chunks return the SAME continuation
    /// cursor must BOTH receive `set_root` after the shared continuation
    /// terminates. Without per-path multi-root tracking, the orchestrator's
    /// HashMap<Path, Hash> mapping silently drops all but the last writer,
    /// `set_root` fires for only one root, and the other is left with an
    /// imported trie but no roots-store tag — causing later `reset(root)` to
    /// fail with `RootRepositoryDivergence`. Radix-trie pagination shares
    /// suffix cursors across roots derived from sibling chain states, so
    /// this collision is the common case, not an edge case.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_completes_all_roots_when_first_chunks_share_last_path() {
        let r1 = hash_for(20);
        let r2 = hash_for(21);
        let shared_cont = vec![(hash_for(99), Some(0u8))];

        let (importer_inner, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(importer_inner);

        // has_root: read from the recorded set so post-import verify sees
        // what set_root just wrote. Pre-filter sees nothing → both roots
        // enter the orchestrator.
        let recorded_for_has_root = recorded.clone();
        let has_root: HasRootFn =
            Arc::new(move |q| Ok(recorded_for_has_root.lock().unwrap().contains(q)));

        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(8);

        let start1 = vec![(r1.clone(), None)];
        let start2 = vec![(r2.clone(), None)];

        // Both first chunks are non-terminal and end at the same cursor.
        let resp1 = make_response(start1.clone(), shared_cont.clone(), Some(hash_for(101)));
        let resp2 = make_response(start2.clone(), shared_cont.clone(), Some(hash_for(102)));
        // The shared continuation terminates with one item (proves it's not
        // an empty-byzantine signal).
        let resp3 = make_response(
            shared_cont.clone(),
            shared_cont.clone(),
            Some(hash_for(103)),
        );

        let feeder_sends = sends.clone();
        let feeder_start1 = start1.clone();
        let feeder_start2 = start2.clone();
        let feeder_cont = shared_cont.clone();
        let feeder = tokio::spawn(async move {
            // Wait until both initial start_paths have been requested before
            // feeding either response, so the orchestrator has both roots in
            // ST when it processes them.
            loop {
                let both_sent = {
                    let s = feeder_sends.lock().unwrap();
                    s.contains(&feeder_start1) && s.contains(&feeder_start2)
                };
                if both_sent {
                    break;
                }
                tokio::task::yield_now().await;
            }
            tx.send(resp1).await.unwrap();
            tx.send(resp2).await.unwrap();
            // Wait for the shared continuation to be requested.
            loop {
                if feeder_sends.lock().unwrap().contains(&feeder_cont) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            tx.send(resp3).await.unwrap();
            drop(tx);
        });

        let (stream, _) = stream(
            vec![r1.clone(), r2.clone()],
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        feeder.await.unwrap();

        assert!(
            final_st.is_finished(),
            "orchestrator should mark all paths Done; final state: {:?}",
            final_st
        );

        let recorded = recorded.lock().unwrap();
        assert!(
            recorded.contains(&r1),
            "set_root must fire for r1 (was silently dropped by HashMap collision); recorded = {:?}",
            recorded
        );
        assert!(
            recorded.contains(&r2),
            "set_root must fire for r2; recorded = {:?}",
            recorded
        );
        assert_eq!(
            recorded.len(),
            2,
            "exactly 2 roots should have been recorded; recorded = {:?}",
            recorded
        );
    }
    /// A completed cursor reopens when a LATER root reaches its shared
    /// continuation: two roots whose tries converge on one continuation
    /// path, delivered far enough apart that the first fully finishes the
    /// path before the second arrives. The stream must re-request the
    /// shared path for the late root (`re_arm`), and both roots must be
    /// recorded. Reopened continuations repeat identical key-value writes
    /// idempotently.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_reopens_shared_continuation_for_late_root() {
        let r1 = hash_for(30);
        let r2 = hash_for(31);
        let shared_cont = vec![(hash_for(109), Some(0u8))];

        let (importer_inner, recorded) = RecordingImporter::new();
        let importer: Arc<dyn RSpaceImporter> = Arc::new(importer_inner);
        let recorded_for_has_root = recorded.clone();
        let has_root: HasRootFn =
            Arc::new(move |q| Ok(recorded_for_has_root.lock().unwrap().contains(q)));

        let ops = MockOps::new();
        let (sends, _) = ops.handles();
        let (tx, rx) = test_mpsc::channel::<StoreItemsMessage>(8);

        let start1 = vec![(r1.clone(), None)];
        let start2 = vec![(r2.clone(), None)];
        let resp1 = make_response(start1.clone(), shared_cont.clone(), Some(hash_for(111)));
        let resp2 = make_response(start2.clone(), shared_cont.clone(), Some(hash_for(112)));
        let terminal = make_response(
            shared_cont.clone(),
            shared_cont.clone(),
            Some(hash_for(113)),
        );

        let feeder_sends = sends.clone();
        let feeder_recorded = recorded.clone();
        let feeder_r1 = r1.clone();
        let feeder_start1 = start1.clone();
        let feeder_start2 = start2.clone();
        let feeder_cont = shared_cont.clone();
        let feeder = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), async move {
                loop {
                    let both_sent = {
                        let sent = feeder_sends.lock().unwrap();
                        sent.contains(&feeder_start1) && sent.contains(&feeder_start2)
                    };
                    if both_sent {
                        break;
                    }
                    tokio::task::yield_now().await;
                }

                tx.send(resp1).await.unwrap();
                loop {
                    if feeder_sends.lock().unwrap().contains(&feeder_cont) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }

                tx.send(terminal.clone()).await.unwrap();
                loop {
                    if feeder_recorded.lock().unwrap().contains(&feeder_r1) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }

                tx.send(resp2).await.unwrap();
                let continuation_reopened = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let request_count = feeder_sends
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|path| **path == feeder_cont)
                            .count();
                        if request_count >= 2 {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .is_ok();
                assert!(
                    continuation_reopened,
                    "shared continuation was not requested for the late root"
                );

                tx.send(terminal).await.unwrap();
                drop(tx);
            })
            .await
            .expect("late-root feeder timed out");
        });

        let (stream, _) = stream(
            vec![r1.clone(), r2.clone()],
            has_root,
            importer,
            ops,
            rx,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let final_st = drain(stream).await.expect("stream yields final state");
        feeder.await.unwrap();

        assert!(final_st.is_finished());
        let recorded = recorded.lock().unwrap();
        assert!(recorded.contains(&r1));
        assert!(recorded.contains(&r2));
    }
}
