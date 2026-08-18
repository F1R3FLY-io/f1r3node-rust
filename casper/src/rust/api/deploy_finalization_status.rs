use std::collections::{HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use prost::bytes::Bytes;

/// Convenience alias matching `BlockAPI`'s error type.
type ApiErr<T> = eyre::Result<T>;

const MAX_DEPLOY_STATUS_SCAN_BLOCKS: usize = 4096;

struct ScanBudget {
    remaining: usize,
}

impl ScanBudget {
    fn new(limit: usize) -> Self { Self { remaining: limit } }

    fn consume(&mut self) -> ApiErr<()> {
        if self.remaining == 0 {
            return Err(eyre::eyre!(
                "deploy_finalization_status: scan exceeded {} block reads",
                MAX_DEPLOY_STATUS_SCAN_BLOCKS
            ));
        }
        self.remaining -= 1;
        Ok(())
    }
}

/// Sentinel error for the deploy-index inconsistency case (a sig is
/// indexed at a block whose body does not list it). Propagated as an
/// `Err` so `repeat_deploy` falls back to its conservative-fail branch
/// (keep the sig in the check set rather than exempting it as recovery).
/// `BlockAPI::deploy_finalization_status` downcasts to this type at the
/// HTTP/gRPC boundary and converts to `pending_unknown` so callers see
/// a tractable response instead of a 500.
#[derive(Debug)]
pub struct DeployFinalizationCorruption {
    pub sig: Bytes,
    pub block_hash: BlockHash,
}

impl std::fmt::Display for DeployFinalizationCorruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "deploy_finalization_status: sig {} indexed at block {} \
             but missing from that block's body.deploys",
            hex::encode(&self.sig),
            PrettyPrinter::build_string_bytes(&self.block_hash),
        )
    }
}

impl std::error::Error for DeployFinalizationCorruption {}

/// Terminal or transitional state of a deploy as observed from the local DAG.
///
/// Clients poll `deploy_finalization_status` by deploy signature to learn
/// whether a deploy has canonically landed. Block-hash polling is insufficient
/// because a block can finalize while the effects of some of its deploys
/// were dropped during merge — `Finalized` here means the effects are in
/// canonical state, not merely that some block containing the sig finalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeployFinalizationState {
    /// Deploy appears in a finalized block's `body.deploys` with
    /// `is_failed=false`, and does not appear in any finalized descendant's
    /// `body.rejected_deploys`. Effects are in canonical state. Terminal.
    Finalized,
    /// Deploy appears in a finalized block with `is_failed=true` — the
    /// Rholang execution itself failed (e.g., insufficient phlo, contract
    /// error). Effects will never apply. Terminal.
    Failed,
    /// Deploy has not yet reached a canonical-finalized inclusion and has
    /// not expired. May be in deploy storage, in a non-finalized block, in
    /// the rejected-deploy buffer awaiting re-proposal, or in a block that
    /// has not yet finalized. Client should keep polling.
    Pending,
    /// `valid_after_block_number + deployLifespan` has elapsed without
    /// successful canonical inclusion. The deploy can never land. Terminal.
    Expired,
}

/// Full response payload for a deploy-finalization-status query.
#[derive(Clone, Debug)]
pub struct DeployFinalizationStatus {
    pub state: DeployFinalizationState,
    /// Number of finalized blocks in which the sig appears in
    /// `body.rejected_deploys`. Zero at submission; monotonically
    /// increases with each merge rejection that finalizes. Gives
    /// operators visibility into deploys that are contending.
    pub rejection_count: u32,
    /// Hash of the highest-block-number canonical block that contains
    /// the sig in either `body.deploys` or `body.rejected_deploys`.
    /// `None` when the sig has not yet been included in any block.
    pub latest_block_hash: Option<BlockHash>,
}

impl DeployFinalizationStatus {
    pub fn pending_unknown() -> Self {
        Self {
            state: DeployFinalizationState::Pending,
            rejection_count: 0,
            latest_block_hash: None,
        }
    }
}

/// Per-sig BFS state accumulated during the finalized-window scan.
/// Lifted out of `resolve` so one scan can serve every sig it tracks.
struct ResolverState {
    sig_bytes: Bytes,
    valid_after_block_number: i64,
    first_seen_block_hash: BlockHash,
    rejection_count: u32,
    failed_finalized_events: Vec<(i64, BlockHash)>,
    clean_finalized_events: Vec<(i64, BlockHash)>,
    latest_event: Option<(i64, BlockHash)>,
    latest_rejected_event: Option<(i64, BlockHash)>,
    finalized_rejected_events: Vec<(i64, BlockHash)>,
    unfinalized_rejected_events: Vec<(i64, BlockHash)>,
}

impl ResolverState {
    fn new(
        sig_bytes: Bytes,
        first_seen_block_hash: BlockHash,
        valid_after_block_number: i64,
    ) -> Self {
        Self {
            sig_bytes,
            valid_after_block_number,
            first_seen_block_hash,
            rejection_count: 0,
            failed_finalized_events: Vec::new(),
            clean_finalized_events: Vec::new(),
            latest_event: None,
            latest_rejected_event: None,
            finalized_rejected_events: Vec::new(),
            unfinalized_rejected_events: Vec::new(),
        }
    }
}

/// Outcome of looking up a sig's deploy-index entry and reading its
/// first-seen block.
enum PreludeOutcome {
    /// Sig is in the deploy index and the first-seen block was readable.
    /// Carries initialized scan state.
    Active(ResolverState),
    /// Sig is unknown (deploy index miss) or first-seen block is absent
    /// from the store; either way, status is `pending_unknown()`.
    Unknown,
}

/// Per-sig prelude: deploy-index lookup, first-seen block fetch, and
/// extraction of `valid_after_block_number`. Error semantics:
///
/// - `Ok(Active(state))` — sig is in the index and the first-seen block
///   was readable.
/// - `Ok(Unknown)` — sig is not in the index, or first-seen block body
///   is absent from the store (typed at `pending_unknown` by callers).
/// - `Err(DeployFinalizationCorruption)` — sig is indexed at a block
///   whose body does not list it. Returned as a typed sentinel so the
///   consensus path conservative-fails (keep in repeat-check) while
///   `BlockAPI::deploy_finalization_status` downcasts and converts to
///   `pending_unknown` for HTTP/gRPC callers.
/// - `Err(other)` — genuine I/O failures from `block_store.get` etc.,
///   propagated unchanged.
fn run_prelude(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    sig: &[u8],
) -> ApiErr<PreludeOutcome> {
    let Some(first_seen_block_hash) = dag
        .lookup_by_deploy_id(&sig.to_vec())
        .map_err(|e| eyre::eyre!("deploy index lookup failed: {}", e))?
    else {
        return Ok(PreludeOutcome::Unknown);
    };

    run_prelude_from_block(block_store, sig, &first_seen_block_hash)
}

fn run_prelude_from_block(
    block_store: &KeyValueBlockStore,
    sig: &[u8],
    first_seen_block_hash: &BlockHash,
) -> ApiErr<PreludeOutcome> {
    let sig_bytes: Bytes = Bytes::copy_from_slice(sig);

    let first_seen_block = match block_store.get(first_seen_block_hash) {
        Ok(Some(b)) => b,
        Ok(None) => {
            tracing::warn!(
                target: "f1r3fly.casper.deploy_finalization.validation",
                "sig {} indexed at block {} but block body absent from store",
                hex::encode(&sig_bytes),
                PrettyPrinter::build_string_bytes(first_seen_block_hash)
            );
            return Ok(PreludeOutcome::Unknown);
        }
        Err(e) => {
            return Err(eyre::eyre!(
                "block_store.get failed for first-seen block {}: {}",
                PrettyPrinter::build_string_bytes(first_seen_block_hash),
                e
            ));
        }
    };
    let valid_after_block_number = match first_seen_block
        .body
        .deploys
        .iter()
        .find(|pd| pd.deploy.sig == sig_bytes)
        .map(|pd| pd.deploy.data.valid_after_block_number)
    {
        Some(n) => n,
        None => {
            // Indexed-but-missing-from-body: the deploy index points at a
            // block whose body does not claim the sig. Logged on the
            // dedicated warn target for operator visibility, returned as
            // a typed `DeployFinalizationCorruption` error so the
            // consensus path (`repeat_deploy`) conservative-fails (keep
            // sig in the check set) and the HTTP/gRPC layer
            // (`BlockAPI::deploy_finalization_status`) downcasts and
            // converts to `pending_unknown` for callers.
            tracing::warn!(
                target: "f1r3fly.casper.deploy_finalization.validation",
                "sig {} indexed at block {} but missing from that block's \
                 body.deploys — check deploy index vs block store consistency",
                hex::encode(&sig_bytes),
                PrettyPrinter::build_string_bytes(first_seen_block_hash),
            );
            return Err(eyre::Report::new(DeployFinalizationCorruption {
                sig: sig_bytes,
                block_hash: first_seen_block_hash.clone(),
            }));
        }
    };

    Ok(PreludeOutcome::Active(ResolverState::new(
        sig_bytes,
        first_seen_block_hash.clone(),
        valid_after_block_number,
    )))
}

/// Walk finalized ancestors of LFB once, updating each active sig's
/// `ResolverState` for events found in `body.deploys` and
/// `body.rejected_deploys`. The caller passes the per-sig states keyed
/// by sig; this function mutates those states in place.
///
/// Cost: one block fetch per visited block in the deploy_lifespan
/// window, regardless of how many sigs are being tracked. Sig matching
/// inside each block is a HashSet membership check.
fn bfs_finalized_window(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    per_sig: &mut HashMap<Bytes, ResolverState>,
    scan_budget: &mut ScanBudget,
) -> ApiErr<HashSet<BlockHash>> {
    if per_sig.is_empty() {
        return Ok(HashSet::new());
    }

    let lfb_hash = dag.last_finalized_block();
    let lfb_height = dag.block_number(&lfb_hash).ok_or_else(|| {
        eyre::eyre!(
            "deploy_finalization_status: LFB {} has no block_number entry",
            PrettyPrinter::build_string_bytes(&lfb_hash),
        )
    })?;
    let active_sig_floor = per_sig
        .values()
        .map(|state| state.valid_after_block_number)
        .min()
        .unwrap_or(lfb_height);
    let rolling_floor =
        crate::rust::util::deploy_window::earliest_valid_after(lfb_height, deploy_lifespan)?;
    let scan_floor = rolling_floor.min(active_sig_floor).max(0);

    // Active sigs as a HashSet for O(1) membership checks during body scans.
    // Cloning sig bytes once here avoids per-block-per-sig clones.
    let active_sigs: HashSet<Bytes> = per_sig.keys().cloned().collect();

    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut frontier: Vec<BlockHash> = vec![lfb_hash.clone()];
    while let Some(candidate_hash) = frontier.pop() {
        if !visited.insert(candidate_hash.clone()) {
            continue;
        }
        let height = match dag.block_number(&candidate_hash) {
            Some(h) => h,
            None => {
                tracing::debug!(
                    "deploy_finalization_status: no block_number for candidate {} — \
                     skipping (likely cleanup race or partial DAG)",
                    PrettyPrinter::build_string_bytes(&candidate_hash)
                );
                continue;
            }
        };
        if height < scan_floor {
            continue;
        }
        scan_budget.consume()?;
        let candidate_block = match block_store.get(&candidate_hash) {
            Ok(Some(b)) => b,
            Ok(None) => {
                tracing::warn!(
                    "deploy_finalization_status: finalized-ancestor block {} absent from store — \
                     scan may miss deploy events in this block",
                    PrettyPrinter::build_string_bytes(&candidate_hash)
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    "deploy_finalization_status: block_store.get failed for {}: {} — \
                     continuing scan; result may be incomplete",
                    PrettyPrinter::build_string_bytes(&candidate_hash),
                    e
                );
                continue;
            }
        };

        // Enqueue every parent slot. Main-parent-only walks miss blocks
        // that reached canonical state via secondary-parent merging.
        for parent in &candidate_block.header.parents_hash_list {
            if !visited.contains(parent) {
                frontier.push(parent.clone());
            }
        }

        // Sigs found in this block — used to update each sig's
        // `latest_event` once after both scans (a sig may appear in
        // both body.deploys and body.rejected_deploys of the same block
        // in pathological dedup paths; we still only bump latest_event
        // once for that sig at this height).
        let mut seen_sigs_here: HashSet<Bytes> = HashSet::new();

        for pd in &candidate_block.body.deploys {
            if active_sigs.contains(&pd.deploy.sig) {
                seen_sigs_here.insert(pd.deploy.sig.clone());
                let state = per_sig
                    .get_mut(&pd.deploy.sig)
                    .expect("active_sigs and per_sig must agree on key set");
                if pd.is_failed {
                    state
                        .failed_finalized_events
                        .push((height, candidate_hash.clone()));
                } else {
                    state
                        .clean_finalized_events
                        .push((height, candidate_hash.clone()));
                }
            }
        }
        for rd in &candidate_block.body.rejected_deploys {
            if active_sigs.contains(&rd.sig) {
                seen_sigs_here.insert(rd.sig.clone());
                let state = per_sig
                    .get_mut(&rd.sig)
                    .expect("active_sigs and per_sig must agree on key set");
                state.rejection_count = state.rejection_count.saturating_add(1);
                if !rd.duplicate {
                    state
                        .finalized_rejected_events
                        .push((height, candidate_hash.clone()));
                }
                if !rd.duplicate
                    && state
                        .latest_rejected_event
                        .as_ref()
                        .map(|(h, _)| height > *h)
                        .unwrap_or(true)
                {
                    state.latest_rejected_event = Some((height, candidate_hash.clone()));
                }
            }
        }
        for sig in &seen_sigs_here {
            let state = per_sig
                .get_mut(sig)
                .expect("seen_sigs_here is drawn from active_sigs / per_sig");
            if state
                .latest_event
                .as_ref()
                .map(|(h, _)| height > *h)
                .unwrap_or(true)
            {
                state.latest_event = Some((height, candidate_hash.clone()));
            }
        }
    }

    Ok(visited)
}

fn scan_visible_unfinalized_rejections(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    finalized_window: &HashSet<BlockHash>,
    per_sig: &mut HashMap<Bytes, ResolverState>,
    scan_budget: &mut ScanBudget,
) -> ApiErr<()> {
    if per_sig.is_empty() {
        return Ok(());
    }

    let lfb_height = dag
        .block_number(&dag.last_finalized_block())
        .ok_or_else(|| eyre::eyre!("deploy_finalization_status: LFB height is unavailable"))?;
    let active_sig_floor = per_sig
        .values()
        .map(|state| state.valid_after_block_number)
        .min()
        .unwrap_or(lfb_height);
    let rolling_floor =
        crate::rust::util::deploy_window::earliest_valid_after(lfb_height, deploy_lifespan)?;
    let scan_floor = rolling_floor.min(active_sig_floor).max(0);
    let active_sigs: HashSet<Bytes> = per_sig.keys().cloned().collect();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut frontier: Vec<BlockHash> = dag
        .latest_message_hashes()
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();

    while let Some(candidate_hash) = frontier.pop() {
        if !visited.insert(candidate_hash.clone()) {
            continue;
        }
        let Some(height) = dag.block_number(&candidate_hash) else {
            continue;
        };
        if height < scan_floor || finalized_window.contains(&candidate_hash) {
            continue;
        }
        scan_budget.consume()?;
        let Some(candidate_block) = block_store.get(&candidate_hash)? else {
            continue;
        };
        for parent in &candidate_block.header.parents_hash_list {
            if !visited.contains(parent) {
                frontier.push(parent.clone());
            }
        }
        for rejected in &candidate_block.body.rejected_deploys {
            if rejected.duplicate || !active_sigs.contains(&rejected.sig) {
                continue;
            }
            let state = per_sig
                .get_mut(&rejected.sig)
                .expect("active_sigs and per_sig must agree on key set");
            state
                .unfinalized_rejected_events
                .push((height, candidate_hash.clone()));
            if state
                .latest_event
                .as_ref()
                .map(|(event_height, event_hash)| {
                    height > *event_height
                        || (height == *event_height && candidate_hash > *event_hash)
                })
                .unwrap_or(true)
            {
                state.latest_event = Some((height, candidate_hash.clone()));
            }
        }
    }

    Ok(())
}

/// Apply the per-sig post-loop rules: canonical-descendant invalidation
/// of clean inclusions, latest_block_hash fallback to the first-seen
/// block, expiry rule, and final state determination.
///
/// Returns `ApiErr` rather than swallowing failures from `is_in_main_chain`.
/// The resolver is an API/observability surface (deploy status reporting and
/// the catchup buffer-admission gate); consensus validation (`repeat_deploy`)
/// deliberately does NOT read it — the resolver reflects the node's LOCAL
/// finalization progress, and gating validation on it forked honest nodes
/// whose finality lagged by a step. Fail loudly rather than guessing under
/// transient I/O all the same.
fn finalize_sig_state(
    dag: &KeyValueDagRepresentation,
    deploy_lifespan: i64,
    state: ResolverState,
) -> ApiErr<DeployFinalizationStatus> {
    let lfb_hash = dag.last_finalized_block();

    let canonical_block = |block: &BlockHash| -> ApiErr<bool> {
        Ok(block == &lfb_hash || dag.is_in_main_chain(block, &lfb_hash)?)
    };

    let rejection_invalidates = |inclusion_height: i64| {
        state
            .finalized_rejected_events
            .iter()
            .any(|(reject_height, _)| *reject_height > inclusion_height)
    };

    let mut clean_candidates = state.clean_finalized_events.clone();
    clean_candidates.sort_by(|(left_height, left_hash), (right_height, right_hash)| {
        right_height
            .cmp(left_height)
            .then_with(|| right_hash.cmp(left_hash))
    });
    let mut clean_canonical: Option<(i64, BlockHash)> = None;
    for (height, block) in clean_candidates {
        if !rejection_invalidates(height) {
            clean_canonical = Some((height, block));
            break;
        }
    }

    let mut failed_candidates = state.failed_finalized_events.clone();
    failed_candidates.sort_by(|(left_height, left_hash), (right_height, right_hash)| {
        right_height
            .cmp(left_height)
            .then_with(|| right_hash.cmp(left_hash))
    });
    let mut failed_canonical: Option<(i64, BlockHash)> = None;
    for (height, block) in failed_candidates {
        if canonical_block(&block)? && !rejection_invalidates(height) {
            failed_canonical = Some((height, block));
            break;
        }
    }

    // Latest-canonical-wins: if both clean and failed canonical events
    // survived their gates, the higher-height one represents the most
    // recent canonical state of the sig.
    let clean_finalized_height: Option<i64> = match (&clean_canonical, &failed_canonical) {
        (Some((ch, _)), Some((fh, _))) if ch > fh => Some(*ch),
        (Some((ch, _)), None) => Some(*ch),
        _ => None,
    };
    let failed_finalized: bool = match (&clean_canonical, &failed_canonical) {
        (Some((ch, _)), Some((fh, _))) => fh > ch,
        (None, Some(_)) => true,
        _ => false,
    };
    let mut unresolved_rejected_event: Option<(i64, BlockHash)> = None;
    if let Some((clean_height, clean_block)) = &clean_canonical {
        if clean_finalized_height == Some(*clean_height) {
            for (reject_height, reject_block) in &state.unfinalized_rejected_events {
                if reject_height > clean_height
                    && dag.is_dag_ancestor(clean_block, reject_block)?
                    && unresolved_rejected_event
                        .as_ref()
                        .map(|(current_height, current_block)| {
                            reject_height > current_height
                                || (reject_height == current_height && reject_block > current_block)
                        })
                        .unwrap_or(true)
                {
                    unresolved_rejected_event = Some((*reject_height, reject_block.clone()));
                }
            }
        }
    }

    // Account for latest_block_hash via the first-seen lookup —
    // covers the case where the sig lives only in a non-finalized
    // block (outside the finalized scan). If the first-seen block
    // somehow has no height entry, skip this fallback rather than
    // record a block_number=0 which would mis-sort against real
    // canonical events.
    let mut latest_event = state.latest_event;
    if latest_event.is_none() {
        if let Some(first_seen_height) = dag.block_number(&state.first_seen_block_hash) {
            latest_event = Some((first_seen_height, state.first_seen_block_hash.clone()));
        } else {
            tracing::debug!(
                "deploy_finalization_status: first-seen block {} has no block_number — \
                 leaving latest_block_hash empty rather than record with bogus height",
                PrettyPrinter::build_string_bytes(&state.first_seen_block_hash)
            );
        }
    }

    // Expiry rule: LFB height strictly past `valid_after + deployLifespan`
    // AND no clean finalized inclusion. Anchored to LFB rather than tip:
    // a sig present in an unfinalized block at tip is still in flight —
    // its host block can finalize and the deploy's effects can land —
    // so it must not be reported as `Expired`. The buffer's purge
    // condition is tip-based and lives on a separate code path.
    let lfb_height = dag
        .block_number(&dag.last_finalized_block())
        .ok_or_else(|| {
            eyre::eyre!(
                "deploy_finalization_status: LFB {} has no block_number entry",
                PrettyPrinter::build_string_bytes(&dag.last_finalized_block()),
            )
        })?;
    let expired = crate::rust::util::deploy_window::is_past_expiration_cutoff(
        state.valid_after_block_number,
        lfb_height,
        deploy_lifespan,
    )? && clean_finalized_height.is_none();

    let final_state = if failed_finalized {
        DeployFinalizationState::Failed
    } else if clean_finalized_height.is_some() && unresolved_rejected_event.is_none() {
        DeployFinalizationState::Finalized
    } else if expired {
        DeployFinalizationState::Expired
    } else {
        DeployFinalizationState::Pending
    };

    let latest_block_hash = latest_event.map(|(_, hash)| hash);
    if state.rejection_count > 0
        || unresolved_rejected_event.is_some()
        || !matches!(&final_state, DeployFinalizationState::Pending)
    {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "status_resolved",
            deploy_sig = %hex::encode(&state.sig_bytes),
            resolved_state = ?final_state,
            rejection_count = state.rejection_count,
            valid_after_block = state.valid_after_block_number,
            lfb_hash = %hex::encode(&lfb_hash),
            lfb_height,
            clean_height = ?clean_canonical.as_ref().map(|(height, _)| *height),
            clean_block = ?clean_canonical.as_ref().map(|(_, hash)| hex::encode(hash)),
            failed_height = ?failed_canonical.as_ref().map(|(height, _)| *height),
            failed_block = ?failed_canonical.as_ref().map(|(_, hash)| hex::encode(hash)),
            rejected_height = ?state.latest_rejected_event.as_ref().map(|(height, _)| *height),
            rejected_block = ?state.latest_rejected_event.as_ref().map(|(_, hash)| hex::encode(hash)),
            unfinalized_rejected_height = ?unresolved_rejected_event.as_ref().map(|(height, _)| *height),
            unfinalized_rejected_block = ?unresolved_rejected_event.as_ref().map(|(_, hash)| hex::encode(hash)),
            latest_block = ?latest_block_hash.as_ref().map(hex::encode),
            expired,
            "deploy lifecycle"
        );
    }

    Ok(DeployFinalizationStatus {
        state: final_state,
        rejection_count: state.rejection_count,
        latest_block_hash,
    })
}

/// Pure resolver for deploy finalization state, single-sig entry point.
/// Does not depend on the engine cell; callable from any context that
/// has a DAG representation, a block store, and the shard-level
/// `deploy_lifespan`. The gRPC / HTTP wrappers call this under their
/// async unwrap of the Casper instance.
///
/// Error semantics: deploy-index
/// inconsistencies (sig indexed at a block whose body does not contain
/// the sig) propagate as `Err(DeployFinalizationCorruption)`, so the
/// consensus path can conservative-fail and the HTTP/gRPC layer can
/// downcast and convert to `pending_unknown`. Truly absent data
/// (unknown sig, first-seen body missing from the store) returns
/// `pending_unknown` directly.
///
/// The state machine is a canonical-chain scan:
///
/// 1. Look up the sig in the deploy index. Unknown sig → `Pending`.
/// 2. Fetch the first-seen block to read `valid_after_block_number`.
/// 3. Walk the finalized chain from LFB backward for `deploy_lifespan`
///    blocks, tallying clean inclusions, failed inclusions, rejections,
///    and `latest_block_hash`.
/// 4. Apply the state rules: failed finalized → `Failed`; clean finalized
///    without a later canonical-descendant rejection → `Finalized`;
///    beyond lifespan without a clean inclusion → `Expired`; otherwise
///    → `Pending`.
pub fn resolve(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    sig: &[u8],
) -> ApiErr<DeployFinalizationStatus> {
    resolve_with_known_block(dag, block_store, deploy_lifespan, sig, None)
}

fn resolve_from_state(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    state: ResolverState,
) -> ApiErr<DeployFinalizationStatus> {
    let mut per_sig: HashMap<Bytes, ResolverState> = HashMap::new();
    per_sig.insert(state.sig_bytes.clone(), state);
    let mut scan_budget = ScanBudget::new(MAX_DEPLOY_STATUS_SCAN_BLOCKS);
    let finalized_window = bfs_finalized_window(
        dag,
        block_store,
        deploy_lifespan,
        &mut per_sig,
        &mut scan_budget,
    )?;
    scan_visible_unfinalized_rejections(
        dag,
        block_store,
        deploy_lifespan,
        &finalized_window,
        &mut per_sig,
        &mut scan_budget,
    )?;
    let (_, state) = per_sig
        .into_iter()
        .next()
        .expect("per_sig was populated with one entry above");
    finalize_sig_state(dag, deploy_lifespan, state)
}

pub fn resolve_with_known_block(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    sig: &[u8],
    known_block_hash: Option<&BlockHash>,
) -> ApiErr<DeployFinalizationStatus> {
    let prelude = run_prelude(dag, block_store, sig)?;
    let state = match prelude {
        PreludeOutcome::Active(state) => state,
        PreludeOutcome::Unknown => match known_block_hash {
            Some(block_hash) => match run_prelude_from_block(block_store, sig, block_hash)? {
                PreludeOutcome::Active(state) => state,
                PreludeOutcome::Unknown => return Ok(DeployFinalizationStatus::pending_unknown()),
            },
            None => return Ok(DeployFinalizationStatus::pending_unknown()),
        },
    };

    resolve_from_state(dag, block_store, deploy_lifespan, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_unknown_has_empty_fields() {
        let s = DeployFinalizationStatus::pending_unknown();
        assert_eq!(s.state, DeployFinalizationState::Pending);
        assert_eq!(s.rejection_count, 0);
        assert!(s.latest_block_hash.is_none());
    }

    #[test]
    fn scan_budget_rejects_work_past_limit() {
        let mut budget = ScanBudget::new(2);
        assert!(budget.consume().is_ok());
        assert!(budget.consume().is_ok());
        assert!(budget.consume().is_err());
    }

    #[test]
    fn states_are_distinct() {
        let all = [
            DeployFinalizationState::Finalized,
            DeployFinalizationState::Failed,
            DeployFinalizationState::Pending,
            DeployFinalizationState::Expired,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(
                    a == b,
                    i == j,
                    "state equality mismatch: {:?} vs {:?}",
                    a,
                    b
                );
            }
        }
    }
}
