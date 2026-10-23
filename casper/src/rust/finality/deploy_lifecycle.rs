//! The deploy-lifecycle register: terminal deploy verdicts — Finalized,
//! Expired, Failed — written write-once into the DAG storage's lifecycle
//! tables. The status API reads them; it never computes.
//!
//! A verdict is a pure function of the adopted LFB, block bodies and the
//! sig's open event row, so it can change only when the LFB advances. The
//! finalizer runs [`settle`] after every cycle, so an advance is settled even
//! when the cycle that persisted it did not finish; nothing else writes
//! verdicts.
//!
//! Verdict rules, against the adopted LFB:
//! - **Finalized** ⟺ a successful execution or a re-application of the sig
//!   lies on the LFB's state lineage (the recorded `merge_base` chain).
//! - **Failed** ⟺ beyond the contestability bound, not on that lineage, and
//!   an `is_failed` execution lies in the LFB's DAG closure.
//! - **Expired** ⟺ beyond the bound, not on that lineage, and no failed
//!   execution in the closure.
//!
//! A sig whose lineage segment crosses this node's restore horizon stays
//! Pending: "not on the lineage" is unknowable over an unreadable segment,
//! and only a readable re-application above the horizon can settle it.
//!
//! The CONTESTABILITY BOUND is `max(window_end, last inclusion) +
//! citability horizon`, past which no admissible block can include or
//! re-apply the sig. The horizon derives from `max_parent_depth` alone; the
//! unlimited sentinel disables Expired and Failed, never Finalized.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::deploy_lifecycle_types::{
    LifecycleEventKind, LifecycleEvents, TerminalRecord, TerminalState,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::MissingBlockContext;

use super::block_facts::{block_facts, BlockFacts, LineageNext};
use crate::rust::errors::CasperError;

/// The citability horizon: how far below the LFB an admissible block can
/// still cite. Derives from `max_parent_depth` ALONE (shard config — the
/// parent-spread validity rule makes deeper citations InvalidParents);
/// the unlimited sentinel means nothing is ever provably uncitable.
pub(crate) fn citability_horizon(max_parent_depth: i32) -> Option<i64> {
    (max_parent_depth != i32::MAX).then_some(i64::from(max_parent_depth))
}

/// True iff `block_hash`'s committed state contains `sig`'s effect: some
/// block on its base lineage either executed it fresh (a non-failed
/// `deploys` entry) or re-applied its chain from scope
/// (`applied_from_scope`) — a failed execution's deploy is in the body while
/// its effect is not. An absent lineage body is `BlockNotHeld`, a fact about
/// this node's history, never an answer.
///
/// `min_height` bounds the walk below: blocks under it cannot carry the
/// effect. Callers holding the deploy pass its `valid_after_block_number`
/// (no execution precedes validity); sig-only callers pass the validity
/// window's floor bound (`floor_number - deploy_lifespan` — a scope-live
/// sig's window was open at its execution, so nothing deeper can hold it).
pub(crate) fn effect_in_state_of(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    sig: &Bytes,
    min_height: i64,
) -> Result<bool, CasperError> {
    let mut found = false;
    let (_, end) = walk_lineage(
        block_store,
        block_hash,
        min_height,
        "membership carrier-body read",
        None,
        &mut |facts| {
            if facts.applied_sigs.contains(sig) {
                found = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )?;
    match end {
        WalkEnd::Gap(gap) => Err(CasperError::BlockNotHeld(gap.missing, gap.context)),
        WalkEnd::Done | WalkEnd::Stopped => Ok(found),
    }
}

/// Where a lineage walk stopped short of its bound: the first body this node
/// does not hold, and the height of the lowest block it did read.
struct Gap {
    missing: BlockHash,
    context: MissingBlockContext,
    lowest_read: i64,
}

enum WalkEnd {
    /// The bound, genesis, or a `visit` break.
    Done,
    /// The `stop_at` block, which was not visited.
    Stopped,
    Gap(Gap),
}

/// Walks `from`'s state lineage (the recorded `merge_base` chain; a block
/// with no recorded base continues to its sole parent, and genesis ends it)
/// down to `min_height`, handing each block's facts to `visit` until it
/// breaks. The walk ends before `stop_at` if it reaches that block. An absent
/// body ends the walk with a [`Gap`] rather than an error, so each caller
/// decides whether a partial answer is acceptable. A multi-parent block with
/// no recorded base is malformed: its state parent is not derivable from the
/// header, so the walk refuses to guess it. Returns the number of blocks
/// visited.
fn walk_lineage(
    block_store: &KeyValueBlockStore,
    from: &BlockHash,
    min_height: i64,
    accessor: &'static str,
    stop_at: Option<&BlockHash>,
    visit: &mut dyn FnMut(&BlockFacts) -> ControlFlow<()>,
) -> Result<(usize, WalkEnd), CasperError> {
    let mut walked = 0usize;
    let mut lowest_read = i64::MAX;
    let mut cur = from.clone();
    loop {
        if stop_at == Some(&cur) {
            return Ok((walked, WalkEnd::Stopped));
        }
        let facts = match block_facts(block_store, &cur, accessor) {
            Ok(facts) => facts,
            Err(CasperError::BlockNotHeld(missing, context)) => {
                return Ok((
                    walked,
                    WalkEnd::Gap(Gap {
                        missing,
                        context,
                        lowest_read,
                    }),
                ))
            }
            Err(other) => return Err(other),
        };
        if facts.block_number < min_height {
            return Ok((walked, WalkEnd::Done));
        }
        walked += 1;
        lowest_read = facts.block_number;
        if visit(&facts).is_break() {
            return Ok((walked, WalkEnd::Done));
        }
        match &facts.lineage_next {
            LineageNext::Base(base) => cur = base.clone(),
            LineageNext::Genesis => return Ok((walked, WalkEnd::Done)),
            LineageNext::MalformedMultiParent => {
                return Err(CasperError::Other(format!(
                    "multi-parent block {} carries no recorded merge_base — \
                     refusing to guess its state lineage",
                    hex::encode(&cur[..8.min(cur.len())]),
                )))
            }
        }
    }
}

/// One walk, every sig: the applied-sig union of `block_hash`'s state
/// lineage down to `min_height` — the batched form of
/// [`effect_in_state_of`] (CLAIM-FINALITY-001, C2:
/// `docs/claims/settled-effect-probe-equivalence.md`). A caller holding
/// the returned set answers any per-sig probe by membership, turning the
/// merge's ~30 per-sig lineage walks into one walk plus O(1) lookups.
/// Also returns the number of blocks walked, for the caller's metrics.
///
/// Stepping, bounds, and the non-failed/`applied_from_scope` fact kinds
/// are exactly the reference walk's. One deliberate strengthening: this
/// walk always covers the FULL segment, so an absent body (or a malformed
/// multi-parent block without a recorded base) anywhere above the bound
/// refuses the whole answer — even when a probed sig is applied above the
/// gap and the per-sig reference walk would have answered TRUE without
/// reaching it. Availability must not shape verdicts (the claim's
/// availability-deferral seam premise); a deferral where the reference
/// sometimes answered is the fail-closed direction.
pub(crate) fn settled_sigs_of_lineage(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    min_height: i64,
) -> Result<(HashSet<Bytes>, usize), CasperError> {
    let mut settled: HashSet<Bytes> = HashSet::new();
    let (walked, end) = walk_lineage(
        block_store,
        block_hash,
        min_height,
        "settled-lineage body read",
        None,
        &mut |facts| {
            settled.extend(facts.applied_sigs.iter().cloned());
            ControlFlow::Continue(())
        },
    )?;
    if let WalkEnd::Gap(gap) = end {
        return Err(CasperError::BlockNotHeld(gap.missing, gap.context));
    }
    Ok((settled, walked))
}

/// Batched multi-floor settled probe with the reference loop's per-floor
/// short-circuit. The reference form checks floors IN ORDER and returns
/// TRUE at the first floor whose lineage holds the sig; floors after the
/// answering one are never read. This probe builds each floor's applied-sig
/// set lazily via [`settled_sigs_of_lineage`] the first time the in-order
/// scan reaches that floor, so an unavailable LATER floor cannot poison a
/// probe an earlier floor answers — the error surface matches the
/// reference at floor granularity (within one floor's segment the
/// fail-closed strengthening of the batched walk applies).
pub(crate) struct FloorSettledProbe {
    /// `(floor hash, min_height)` in the reference scan order.
    floors: Vec<(BlockHash, i64)>,
    /// Lazily built per-floor applied-sig sets, index-aligned with
    /// `floors`.
    sets: Vec<Option<HashSet<Bytes>>>,
    /// Blocks walked across every set built so far, for the caller's
    /// metrics.
    pub(crate) total_walked: usize,
}

impl FloorSettledProbe {
    pub(crate) fn new(floors: Vec<(BlockHash, i64)>) -> Self {
        let sets = floors.iter().map(|_| None).collect();
        Self {
            floors,
            sets,
            total_walked: 0,
        }
    }

    /// TRUE iff some floor's lineage (down to that floor's bound) holds
    /// the sig, scanning floors in order and stopping at the first hit.
    pub(crate) fn settled(
        &mut self,
        block_store: &KeyValueBlockStore,
        sig: &Bytes,
    ) -> Result<bool, CasperError> {
        for i in 0..self.floors.len() {
            if self.sets[i].is_none() {
                let (hash, min_height) = &self.floors[i];
                let (sigs, walked) = settled_sigs_of_lineage(block_store, hash, *min_height)?;
                self.total_walked += walked;
                self.sets[i] = Some(sigs);
            }
            if self.sets[i].as_ref().expect("just built").contains(sig) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub(crate) fn record_settle_timing(settle_ms: u128, prune_ms: Option<u128>) {
    use crate::rust::metrics_constants::{
        CASPER_METRICS_SOURCE, DEPLOY_LIFECYCLE_CARRIER_PRUNE_TIME_METRIC,
        DEPLOY_LIFECYCLE_SETTLE_TIME_METRIC,
    };
    metrics::histogram!(DEPLOY_LIFECYCLE_SETTLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
        .record(settle_ms as f64 / 1000.0);
    if let Some(prune_ms) = prune_ms {
        metrics::histogram!(
            DEPLOY_LIFECYCLE_CARRIER_PRUNE_TIME_METRIC,
            "source" => CASPER_METRICS_SOURCE
        )
        .record(prune_ms as f64 / 1000.0);
    }
}

/// What the last settle's walk learned, kept for the next settle. A block's
/// lineage below it is fixed by its body, so a later walk that reaches
/// `start` can take every answer below it from here instead of walking on.
#[derive(Default)]
pub(crate) struct LineageMemo {
    last: Option<WalkedLineage>,
}

struct WalkedLineage {
    start: BlockHash,
    start_height: i64,
    /// The sigs this walk answered, each down to its `valid_after`.
    answered: HashSet<Bytes>,
    applied_at: HashMap<Bytes, i64>,
    gap_lowest_read: Option<i64>,
}

pub(crate) struct Settled {
    /// The sigs this settle made terminal.
    pub(crate) terminal: Vec<Bytes>,
    /// The lineage blocks this settle walked.
    pub(crate) walked: usize,
}

/// Writes every verdict the adopted LFB in `dag` now decides. Every input
/// comes from `dag` and the store, so the clock and the view can never
/// disagree; `memo` only spares re-reading lineage an earlier settle read.
pub(crate) fn settle(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    citability_horizon: Option<i64>,
    memo: &mut LineageMemo,
) -> Result<Settled, CasperError> {
    let rows = dag.open_lifecycle_rows().map_err(CasperError::from)?;
    let (pass, walked) = SettlePass::walk(
        dag,
        block_store,
        &rows,
        deploy_lifespan,
        citability_horizon,
        memo,
    )?;
    // One row's failure costs that row, never the rows after it: each is
    // retried by the next settle.
    let mut terminal = Vec::new();
    for (sig, row) in rows {
        let sig = Bytes::from(sig);
        match pass.settle_row(dag, &sig, &row) {
            Ok(Some(_)) => terminal.push(sig),
            Ok(None) => {}
            Err(e) => {
                metrics::counter!(
                    crate::rust::metrics_constants::DEPLOY_LIFECYCLE_SETTLE_ROW_FAILURES_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::error!(
                    target: "f1r3fly.casper.lifecycle",
                    sig = %hex::encode(&sig[..8.min(sig.len())]),
                    error = %e,
                    "deploy verdict could not be settled; the next settle retries it"
                );
            }
        }
    }
    Ok(Settled { terminal, walked })
}

/// One settle's view of the adopted LFB: its lineage, walked once for every
/// open row, and the bounds the verdicts are judged against.
struct SettlePass {
    lfb: BlockHash,
    lfb_height: i64,
    /// The highest lineage height that applied each open sig.
    applied_at: HashMap<Bytes, i64>,
    /// Set when the lineage crosses this node's restore horizon.
    gap_lowest_read: Option<i64>,
    deploy_lifespan: i64,
    citability_horizon: Option<i64>,
}

impl SettlePass {
    /// Walks the LFB's lineage down to the lowest open `valid_after`, or only
    /// down to the last walk's start when that walk still answers every open
    /// sig. Returns the pass and the number of blocks walked.
    fn walk(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        rows: &[(Vec<u8>, LifecycleEvents)],
        deploy_lifespan: i64,
        citability_horizon: Option<i64>,
        memo: &mut LineageMemo,
    ) -> Result<(Self, usize), CasperError> {
        let lfb = dag.last_finalized_block();
        let lfb_height = dag.block_number_unsafe(&lfb).map_err(CasperError::from)?;
        let open: HashMap<Bytes, i64> = rows
            .iter()
            .filter_map(|(sig, row)| row.valid_after.map(|va| (Bytes::from(sig.clone()), va)))
            .collect();
        let mut applied_at: HashMap<Bytes, i64> = HashMap::new();
        let mut gap_lowest_read = None;
        let mut walked = 0;
        if let Some(min_valid_after) = open.values().min().copied() {
            // A sig valid only above the last start cannot apply below it.
            let last = memo.last.as_ref().filter(|last| {
                open.iter()
                    .all(|(sig, va)| last.answered.contains(sig) || *va > last.start_height)
            });
            // Highest first, so a sig's first recorded height is its highest.
            let (count, end) = walk_lineage(
                block_store,
                &lfb,
                min_valid_after,
                "settle lineage body read",
                last.map(|last| &last.start),
                &mut |facts| {
                    for sig in &facts.applied_sigs {
                        if open.contains_key(sig) {
                            applied_at.entry(sig.clone()).or_insert(facts.block_number);
                        }
                    }
                    ControlFlow::Continue(())
                },
            )?;
            walked = count;
            match end {
                WalkEnd::Done => {}
                WalkEnd::Stopped => {
                    let last = last.expect("a walk stops only at the last walk's start");
                    for (sig, height) in &last.applied_at {
                        if open.contains_key(sig) {
                            applied_at.entry(sig.clone()).or_insert(*height);
                        }
                    }
                    gap_lowest_read = last.gap_lowest_read;
                }
                WalkEnd::Gap(gap) => {
                    tracing::warn!(
                        target: "f1r3fly.casper.lifecycle",
                        missing = %hex::encode(&gap.missing[..8.min(gap.missing.len())]),
                        lowest_read = gap.lowest_read,
                        "settle walk crossed the restore horizon: sigs valid below it stay Pending"
                    );
                    gap_lowest_read = Some(gap.lowest_read);
                }
            }
            memo.last = Some(WalkedLineage {
                start: lfb.clone(),
                start_height: lfb_height,
                answered: open.keys().cloned().collect(),
                applied_at: applied_at.clone(),
                gap_lowest_read,
            });
        }
        Ok((
            Self {
                lfb,
                lfb_height,
                applied_at,
                gap_lowest_read,
                deploy_lifespan,
                citability_horizon,
            },
            walked,
        ))
    }

    /// Writes the verdict this pass decides for one open row, and returns it.
    fn settle_row(
        &self,
        dag: &KeyValueDagRepresentation,
        sig: &Bytes,
        row: &LifecycleEvents,
    ) -> Result<Option<TerminalState>, CasperError> {
        let Some(valid_after) = row.valid_after else {
            dag.discard_lifecycle_events_if(sig, |current| {
                current.valid_after.is_none() && self.record_only_past_contest(current)
            })
            .map_err(CasperError::from)?;
            return Ok(None);
        };
        let state = if self
            .applied_at
            .get(sig)
            .is_some_and(|height| *height >= valid_after)
        {
            Some(TerminalState::Finalized)
        } else if self
            .gap_lowest_read
            .is_some_and(|lowest_read| valid_after < lowest_read)
        {
            None
        } else {
            self.beyond_contest_state(dag, row, valid_after)?
        };
        if let Some(state) = state {
            write_terminal(dag, sig, state, row)?;
        }
        Ok(state)
    }

    /// Expired or Failed once the LFB is past the contestability bound,
    /// `None` before it.
    fn beyond_contest_state(
        &self,
        dag: &KeyValueDagRepresentation,
        row: &LifecycleEvents,
        valid_after: i64,
    ) -> Result<Option<TerminalState>, CasperError> {
        let Some(bound) = self.citability_horizon else {
            return Ok(None);
        };
        let decide_at = row
            .events
            .iter()
            .filter(|e| matches!(e.kind, LifecycleEventKind::Included { .. }))
            .map(|e| e.height)
            .fold(valid_after + self.deploy_lifespan, i64::max)
            + bound;
        if self.lfb_height <= decide_at {
            return Ok(None);
        }
        for event in &row.events {
            if matches!(event.kind, LifecycleEventKind::Included { is_failed: true })
                && self.in_lfb_closure(dag, &Bytes::from(event.block_hash.clone()))?
            {
                return Ok(Some(TerminalState::Failed));
            }
        }
        Ok(Some(TerminalState::Expired))
    }

    /// A record names a carrier at or below the record's own height, so the
    /// sig's validity window closed by that height plus the lifespan. Past
    /// the bound beyond it, no block can include the sig and a row holding
    /// only records can never become terminal.
    fn record_only_past_contest(&self, row: &LifecycleEvents) -> bool {
        let Some(bound) = self.citability_horizon else {
            return false;
        };
        row.events
            .iter()
            .map(|e| e.height)
            .max()
            .is_some_and(|height| self.lfb_height > height + self.deploy_lifespan + bound)
    }

    /// True iff `hash` is the LFB or one of its DAG ancestors.
    fn in_lfb_closure(
        &self,
        dag: &KeyValueDagRepresentation,
        hash: &BlockHash,
    ) -> Result<bool, CasperError> {
        if *hash == self.lfb {
            return Ok(true);
        }
        let Some(height) = dag.block_number(hash) else {
            return Ok(false);
        };
        if height > self.lfb_height {
            return Ok(false);
        }
        dag.is_dag_ancestor(hash, &self.lfb)
            .map_err(CasperError::from)
    }
}

/// Removes every pool copy whose sig has a terminal verdict, and returns how
/// many it removed. It keeps no state, so a failed removal, or a crash after
/// the verdict write, is repaired by the next call.
pub(crate) fn release_settled(
    dag: &KeyValueDagRepresentation,
    deploy_storage: &parking_lot::Mutex<KeyValueDeployStorage>,
) -> Result<usize, CasperError> {
    let pooled = deploy_storage.lock().sigs().map_err(CasperError::from)?;
    let mut settled = Vec::new();
    for sig in pooled {
        if dag
            .deploy_terminal(&sig)
            .map_err(CasperError::from)?
            .is_some()
        {
            settled.push(sig);
        }
    }
    if settled.is_empty() {
        return Ok(0);
    }
    let mut pool = deploy_storage.lock();
    let mut released = 0usize;
    for sig in settled {
        match pool.remove_by_sig(&sig) {
            Ok(_) => released += 1,
            Err(e) => {
                metrics::counter!(
                    crate::rust::metrics_constants::DEPLOY_LIFECYCLE_RELEASE_FAILURES_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::error!(
                    target: "f1r3fly.casper.lifecycle",
                    sig = %hex::encode(&sig[..8.min(sig.len())]),
                    error = %e,
                    "pool copy of a terminal deploy was not released; the next cycle retries"
                );
            }
        }
    }
    Ok(released)
}

/// Display fields for a terminal record, frozen from the event row it
/// prunes: every record event counts toward `rejection_count` (duplicates
/// included — the count is observability, not the causal ordering), and
/// the latest DAG-VISIBLE inclusion event names the sig's most recent
/// canonical appearance. The visibility filter matters here because the
/// terminal record outlives the row: an orphan event from a crash inside
/// the ingest-first insert window must not freeze into a write-once
/// record that `canonical_appearance` then returns unfiltered forever.
/// A rejection record's block does not carry the deploy — a
/// record-carrier here sends every consumer that fetches the named block
/// looking for a deploy that is not in it.
fn frozen_display(
    row: &LifecycleEvents,
    is_visible: &dyn Fn(&[u8]) -> bool,
) -> (u32, i64, Vec<u8>) {
    let rejection_count = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Rejected { .. }))
        .count() as u32;
    let latest = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Included { .. }))
        .filter(|e| is_visible(&e.block_hash))
        .max_by(|a, b| {
            a.height
                .cmp(&b.height)
                .then_with(|| a.block_hash.cmp(&b.block_hash))
        })
        .map(|e| (e.height, e.block_hash.clone()));
    let (latest_height, latest_block_hash) = latest.unwrap_or((0, Vec::new()));
    (rejection_count, latest_height, latest_block_hash)
}

fn write_terminal(
    dag: &KeyValueDagRepresentation,
    sig: &Bytes,
    state: TerminalState,
    row: &LifecycleEvents,
) -> Result<(), CasperError> {
    let (rejection_count, latest_height, latest_block_hash) = frozen_display(row, &|h| {
        dag.contains(&prost::bytes::Bytes::copy_from_slice(h))
    });
    let written = dag
        .put_deploy_terminal_if_absent(sig, TerminalRecord {
            state,
            rejection_count,
            latest_height,
            latest_block_hash,
        })
        .map_err(CasperError::from)?;
    tracing::info!(
        target: "f1r3fly.casper.lifecycle",
        sig = %hex::encode(&sig[..8.min(sig.len())]),
        state = ?written.state,
        rejection_count = written.rejection_count,
        "deploy lifecycle terminal verdict written"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::{BlockMessage, RejectedDeploy};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn block_at(height: i64, parents: Vec<BlockHash>, seq: i32) -> BlockMessage {
        get_random_block(
            Some(height),
            Some(seq),
            None,
            None,
            None,
            None,
            Some(i64::from(seq)),
            Some(parents),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("test-shard".to_string()),
            None,
        )
    }

    /// A genuinely signed deploy (the store re-verifies deploy signatures
    /// on decode) with a distinct sig per `n`, wrapped as processed.
    fn processed(
        n: i32,
        failed: bool,
    ) -> (
        Bytes,
        models::rust::casper::protocol::casper_message::ProcessedDeploy,
    ) {
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(n, None, None)
            .expect("deploy data");
        let sig = deploy.sig.clone();
        let mut pd = models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy);
        pd.is_failed = failed;
        (sig, pd)
    }

    async fn store() -> KeyValueBlockStore {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store")
    }

    /// The membership rule over raw block reads, independent of
    /// `walk_lineage` and the block-facts cache: the oracle the batched and
    /// per-sig walks are both checked against.
    fn reference_effect_in_state(
        block_store: &KeyValueBlockStore,
        block_hash: &BlockHash,
        sig: &Bytes,
        min_height: i64,
    ) -> Result<bool, CasperError> {
        let mut cur = block_hash.clone();
        loop {
            let Some(block) = block_store.get(&cur)? else {
                return Err(CasperError::BlockNotHeld(
                    cur,
                    MissingBlockContext::new("reference walk"),
                ));
            };
            if block.body.state.block_number < min_height {
                return Ok(false);
            }
            if block
                .body
                .deploys
                .iter()
                .any(|pd| pd.deploy.sig == *sig && !pd.is_failed)
                || block.body.applied_from_scope.iter().any(|s| s == sig)
            {
                return Ok(true);
            }
            cur = if !block.body.merge_base.is_empty() {
                block.body.merge_base.clone()
            } else {
                match block.header.parents_hash_list.as_slice() {
                    [] => return Ok(false),
                    [parent] => parent.clone(),
                    _ => return Err(CasperError::Other("multi-parent without base".into())),
                }
            };
        }
    }

    /// The register's clock is the node's ADOPTED LFB, never an admitted
    /// block's frozen floor: a frozen floor on a branch this node has not
    /// adopted must not write a verdict. The verdict lands exactly when the
    /// node itself adopts a covering LFB.
    #[tokio::test]
    async fn verdicts_key_on_the_adopted_lfb_never_a_frozen_floor() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };
        use models::rust::block_implicits::get_random_block;

        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let mk = |number: i64,
                  seq: i32,
                  parents: Vec<BlockHash>,
                  deploys: Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>| {
            get_random_block(
                Some(number),
                Some(seq),
                None,
                None,
                None,
                None,
                Some(number),
                Some(parents),
                Some(Vec::new()),
                Some(deploys),
                Some(Vec::new()),
                None,
                Some("test".to_string()),
                None,
            )
        };

        let (sig, pd) = {
            let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
                1,
                None,
                Some("test".to_string()),
            )
            .expect("deploy data");
            let sig = deploy.sig.clone();
            (
                sig,
                models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy),
            )
        };

        let genesis = mk(0, 0, Vec::new(), Vec::new());
        let a = mk(1, 1, vec![genesis.block_hash.clone()], vec![pd]);
        let b = mk(2, 2, vec![a.block_hash.clone()], Vec::new());
        let c = mk(3, 3, vec![b.block_hash.clone()], Vec::new());

        for block in [&genesis, &a, &b, &c] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&a, &b, &c] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        // Another validator's claim: block b's FROZEN floor is b itself —
        // covering the deploy's carrier — while THIS node has adopted
        // nothing past genesis.
        let dag = dag_storage.get_representation().expect("dag");
        dag.put_cached_floor(b.block_hash.clone(), b.block_hash.clone())
            .expect("seed frozen floor");

        let terminalized = settle(
            &dag,
            &block_store,
            10,
            Some(10),
            &mut LineageMemo::default(),
        )
        .expect("settle at genesis")
        .terminal;
        assert!(
            terminalized.is_empty(),
            "no verdict may be written off an admitted block's frozen floor \
             while the adopted LFB is still genesis; got {:?}",
            terminalized
        );

        // The node itself adopts b; the next settle writes the verdict
        // against the adopted chain.
        dag_storage
            .record_directly_finalized(b.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt b");
        let dag = dag_storage.get_representation().expect("dag");
        let terminalized = settle(
            &dag,
            &block_store,
            10,
            Some(10),
            &mut LineageMemo::default(),
        )
        .expect("settle at b")
        .terminal;
        assert_eq!(
            terminalized,
            vec![sig],
            "adoption of a covering LFB must land the Finalized verdict"
        );
    }

    /// A sig with BOTH a floor-covered failed execution and a later clean
    /// win whose effect is in floor state must write `Finalized`, never
    /// `Failed` — even past the contestability bound, where the Failed arm
    /// is live. Coverage is checked first and a failed execution is not
    /// membership, so the clean win answers before the Failed arm can
    /// read the failed inclusion.
    #[tokio::test]
    async fn clean_covered_win_supersedes_a_covered_failed_execution() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };
        use models::rust::block_implicits::get_random_block;

        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let mk = |number: i64,
                  seq: i32,
                  parents: Vec<BlockHash>,
                  deploys: Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>| {
            get_random_block(
                Some(number),
                Some(seq),
                None,
                None,
                None,
                None,
                Some(number),
                Some(parents),
                Some(Vec::new()),
                Some(deploys),
                Some(Vec::new()),
                None,
                Some("test".to_string()),
                None,
            )
        };

        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy data");
        let sig = deploy.sig.clone();
        let mut failed_copy =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy.clone());
        failed_copy.is_failed = true;
        let clean_copy =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy);

        let genesis = mk(0, 0, Vec::new(), Vec::new());
        let a = mk(1, 1, vec![genesis.block_hash.clone()], vec![failed_copy]);
        let b = mk(2, 2, vec![a.block_hash.clone()], vec![clean_copy]);
        let c = mk(3, 3, vec![b.block_hash.clone()], Vec::new());
        let d = mk(4, 4, vec![c.block_hash.clone()], Vec::new());

        for block in [&genesis, &a, &b, &c, &d] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&a, &b, &c, &d] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let dag = dag_storage.get_representation().expect("dag");
        let early = settle(&dag, &block_store, 1, Some(1), &mut LineageMemo::default())
            .expect("settle at genesis")
            .terminal;
        assert!(early.is_empty(), "no verdict before a covering adoption");

        // Adopt d (height 4): past decide_at = max(window_end, last
        // inclusion at 2) + bound = 3, so the Failed arm is LIVE — and the
        // failed execution at `a` is inside the adopted floor's closure.
        dag_storage
            .record_directly_finalized(d.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt d");
        let dag = dag_storage.get_representation().expect("dag");
        let terminalized = settle(&dag, &block_store, 1, Some(1), &mut LineageMemo::default())
            .expect("settle after adoption")
            .terminal;

        assert_eq!(terminalized, vec![sig.clone()]);
        let record = dag
            .deploy_terminal(&sig)
            .expect("terminal lookup")
            .expect("terminal record written");
        assert_eq!(
            record.state,
            TerminalState::Finalized,
            "the clean covered win must answer before the live Failed arm \
             can read the covered failed execution"
        );
    }

    /// The terminal record's frozen appearance names a block that CARRIES
    /// the deploy: a rejection record at a greater height than the latest
    /// inclusion counts toward `rejection_count` but never becomes the
    /// display carrier — the record's block has no such deploy in its body.
    #[test]
    fn frozen_display_names_the_latest_inclusion_never_a_record_carrier() {
        use block_storage::rust::dag::deploy_lifecycle_types::{
            LifecycleEvent, LifecycleEventKind, LifecycleEvents,
        };

        let inclusion_block = vec![0x11u8; 32];
        let record_block = vec![0x22u8; 32];
        let row = LifecycleEvents {
            valid_after: Some(1),
            events: vec![
                LifecycleEvent {
                    height: 10,
                    block_hash: inclusion_block.clone(),
                    kind: LifecycleEventKind::Included { is_failed: false },
                },
                LifecycleEvent {
                    height: 20,
                    block_hash: record_block,
                    kind: LifecycleEventKind::Rejected { duplicate: false },
                },
            ],
        };

        let (rejection_count, latest_height, latest_block_hash) = frozen_display(&row, &|_| true);
        assert_eq!(rejection_count, 1);

        let (_, orphan_height, orphan_hash) = frozen_display(&row, &|_| false);
        assert_eq!(
            (orphan_height, orphan_hash),
            (0, Vec::new()),
            "a never-DAG-visible inclusion must not freeze into the display"
        );
        assert_eq!(rejection_count, 1);
        assert_eq!(
            (latest_height, latest_block_hash),
            (10, inclusion_block),
            "the height-20 record event must not displace the inclusion \
             carrier from the frozen display"
        );
    }

    /// genesis(0) <- a(1, fresh sig_a) <- m(2, base=a, applied sig_b):
    /// membership walks the recorded lineage for both fact kinds and
    /// exhausts at genesis for unknown sigs.
    #[tokio::test]
    async fn walks_fresh_and_applied_facts_to_genesis() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_a, pd_a) = processed(1, false);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_a];
        let sig_b = Bytes::from_static(b"applied_sig");
        let mut m = block_at(2, vec![a.block_hash.clone(), genesis.block_hash.clone()], 2);
        m.body.merge_base = a.block_hash.clone();
        m.body.applied_from_scope = vec![sig_b.clone()];
        for b in [&genesis, &a, &m] {
            store.put_block_message(b).expect("store block");
        }

        let sig_c = Bytes::from_static(b"unknown_sig");
        assert!(effect_in_state_of(&store, &m.block_hash, &sig_b, 0).expect("walk"));
        assert!(effect_in_state_of(&store, &m.block_hash, &sig_a, 0).expect("walk"));
        assert!(!effect_in_state_of(&store, &m.block_hash, &sig_c, 0).expect("walk"));
    }

    /// A failed execution's deploy rides the body while its effect is not
    /// in the state: the walk must not count it.
    #[tokio::test]
    async fn a_failed_execution_is_not_membership() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_f, pd_f) = processed(1, true);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_f];
        for b in [&genesis, &a] {
            store.put_block_message(b).expect("store block");
        }
        assert!(!effect_in_state_of(&store, &a.block_hash, &sig_f, 0).expect("walk"));
    }

    /// The bound stops the walk: an execution below `min_height` is
    /// invisible by construction, so the walk need not read it.
    #[tokio::test]
    async fn min_height_bounds_the_walk() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let (sig_a, pd_a) = processed(1, false);
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 1);
        a.body.deploys = vec![pd_a];
        let b = block_at(2, vec![a.block_hash.clone()], 2);
        for blk in [&genesis, &a, &b] {
            store.put_block_message(blk).expect("store block");
        }
        assert!(effect_in_state_of(&store, &b.block_hash, &sig_a, 1).expect("walk"));
        assert!(!effect_in_state_of(&store, &b.block_hash, &sig_a, 2).expect("walk"));
    }

    /// A multi-parent block with no recorded base is malformed: the walk
    /// refuses to guess its state lineage.
    #[tokio::test]
    async fn multi_parent_without_base_is_an_error() {
        let store = store().await;
        let genesis = block_at(0, vec![], 0);
        let a = block_at(1, vec![genesis.block_hash.clone()], 1);
        let m = block_at(2, vec![a.block_hash.clone(), genesis.block_hash.clone()], 2);
        for b in [&genesis, &a, &m] {
            store.put_block_message(b).expect("store block");
        }
        let sig = Bytes::from_static(b"sig_x");
        assert!(effect_in_state_of(&store, &m.block_hash, &sig, 0).is_err());
    }

    /// An absent lineage block is a statement about THIS node's history —
    /// a truncated node legitimately lacks bodies below its restore
    /// horizon — so the refusal must be the typed [`CasperError::BlockNotHeld`]
    /// naming the block, never a stringified `Other` that bypasses every
    /// availability classifier downstream.
    #[tokio::test]
    async fn an_absent_lineage_block_is_a_typed_block_not_held() {
        let store = store().await;
        let absent = block_at(1, vec![], 1);
        let b = block_at(2, vec![absent.block_hash.clone()], 2);
        store.put_block_message(&b).expect("store block");

        let sig = Bytes::from_static(b"sig_x");
        let err = effect_in_state_of(&store, &b.block_hash, &sig, 0)
            .expect_err("an unreadable lineage segment must refuse, not answer");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h, _) if *h == absent.block_hash),
            "absence must carry the missing block's name typed; got: {}",
            err
        );
    }

    /// A truncated node whose settle walk crosses the restore horizon must
    /// absorb it: the blocked sig stays Pending (no invented Expired past the
    /// contestability bound — "not in the state" is unknowable over an
    /// unreadable segment), sibling sigs still settle, later settles do not
    /// error, and a readable re-application above the horizon still lands
    /// Finalized.
    #[tokio::test]
    async fn a_settle_absorbs_the_restore_horizon() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };
        use models::rust::block_implicits::get_random_block;

        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        // Bonds are EMPTY: a truncated DAG holds no height-0 block, so a
        // bonded-validator insert would (correctly) demand the genesis
        // sentinel this test does not need.
        let mk = |number: i64,
                  seq: i32,
                  parents: Vec<BlockHash>,
                  deploys: Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>| {
            get_random_block(
                Some(number),
                Some(seq),
                None,
                None,
                None,
                None,
                Some(number),
                Some(parents),
                Some(Vec::new()),
                Some(deploys),
                Some(Vec::new()),
                Some(Vec::new()),
                Some("test".to_string()),
                None,
            )
        };

        // The horizon: block #5 exists only as a hash pointer — its body
        // was never restored. The window: w1(#6, anchor) <- w2(#7).
        let absent = mk(5, 5, Vec::new(), Vec::new());

        let blocked_deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy data");
        let sig_blocked = blocked_deploy.sig.clone();
        let mut blocked_pd =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(blocked_deploy);
        blocked_pd.is_failed = true;

        let live_deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            2,
            None,
            Some("test".to_string()),
        )
        .expect("deploy data");
        let sig_live = live_deploy.sig.clone();
        let live_pd =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(live_deploy);

        // w1 carries a FAILED execution of the blocked sig: the row exists
        // (valid_after 0) but membership must keep walking — straight into
        // the absent block.
        let w1 = mk(6, 6, vec![absent.block_hash.clone()], vec![blocked_pd]);
        let w2 = mk(7, 7, vec![w1.block_hash.clone()], vec![live_pd]);

        for block in [&w1, &w2] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&w1, InsertMode::Approved)
            .expect("insert anchor");
        dag_storage
            .insert(&w2, InsertMode::Normal)
            .expect("insert w2");
        dag_storage
            .record_directly_finalized(w2.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt w2");

        let dag = dag_storage.get_representation().expect("dag");
        let mut memo = LineageMemo::default();
        let terminalized = settle(&dag, &block_store, 1, Some(1), &mut memo)
            .expect("a horizon crossing must not error")
            .terminal;
        assert_eq!(
            terminalized,
            vec![sig_live.clone()],
            "the sibling sig must still reach its verdict"
        );
        assert!(
            dag.deploy_terminal(&sig_blocked)
                .expect("terminal lookup")
                .is_none(),
            "no verdict may be invented for a sig whose lineage is unreadable"
        );

        // Past the contestability bound (decide_at = max(0+1, 6) + 1 = 7 <
        // floor 8): the Expired arm is live, and must stay suppressed for
        // the horizon-blocked sig. The observation must not re-error either.
        let w3 = mk(8, 8, vec![w2.block_hash.clone()], Vec::new());
        block_store.put_block_message(&w3).expect("store w3");
        dag_storage
            .insert(&w3, InsertMode::Normal)
            .expect("insert w3");
        dag_storage
            .record_directly_finalized(w3.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt w3");
        let dag = dag_storage.get_representation().expect("dag");
        let settled = settle(&dag, &block_store, 1, Some(1), &mut memo)
            .expect("later settles must not error");
        assert_eq!(
            settled.walked, 1,
            "only w3 is new: the gap below w2 comes from the last walk"
        );
        assert!(settled.terminal.is_empty());
        assert!(
            dag.deploy_terminal(&sig_blocked)
                .expect("terminal lookup")
                .is_none(),
            "Expired past the bound would be an invented verdict: membership \
             below the horizon is unknowable"
        );

        // A readable re-application above the horizon answers the question
        // the unreadable segment could not: Finalized still lands.
        let mut w4 = mk(9, 9, vec![w3.block_hash.clone()], Vec::new());
        w4.body.applied_from_scope = vec![sig_blocked.clone()];
        block_store.put_block_message(&w4).expect("store w4");
        dag_storage
            .insert(&w4, InsertMode::Normal)
            .expect("insert w4");
        dag_storage
            .record_directly_finalized(w4.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt w4");
        let dag = dag_storage.get_representation().expect("dag");
        let terminalized = settle(&dag, &block_store, 1, Some(1), &mut memo)
            .expect("settle at w4")
            .terminal;
        assert_eq!(terminalized, vec![sig_blocked.clone()]);
        let record = dag
            .deploy_terminal(&sig_blocked)
            .expect("terminal lookup")
            .expect("terminal record written");
        assert_eq!(
            record.state,
            TerminalState::Finalized,
            "a readable re-application must still finalize a horizon-blocked sig"
        );
    }

    /// CLAIM-FINALITY-001 C2 bridge (discharge plan item 2): on generated
    /// lineages the batched walk answers exactly as the per-sig reference
    /// walk — for every fresh sig (failed and non-failed), every
    /// `applied_from_scope` sig, decoy sigs living only on a non-base
    /// parent, and absent sigs, across low/mid/above-tip bounds. Each
    /// (lineage, bound) runs twice so the second pass exercises the
    /// block-facts cache-hit path against the same oracle.
    #[tokio::test]
    async fn batched_walk_matches_the_reference_walk_on_generated_lineages() {
        let store = store().await;
        let mut lcg: u64 = 0x5eed_cafe_0000_0001;
        let mut rand = move |modulo: u32| -> u32 {
            lcg = lcg
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((lcg >> 33) as u32) % modulo
        };
        let mut deploy_n: i32 = 100;
        let mut seq: i32 = 1000;
        let mut applied_n: u32 = 0;

        for _config in 0..12 {
            let len: i64 = 4 + i64::from(rand(6));
            let mut probe_sigs: Vec<Bytes> = Vec::new();
            let genesis = {
                seq += 1;
                block_at(0, vec![], seq)
            };
            store.put_block_message(&genesis).expect("store genesis");
            let mut prev = genesis;
            for h in 1..=len {
                seq += 1;
                let mut b = if h >= 2 && rand(3) == 0 {
                    // A merged block: the side parent is stored but NOT on
                    // the state lineage; its decoy applied sig must stay
                    // invisible to both walks.
                    seq += 1;
                    let mut side = block_at(h, vec![prev.block_hash.clone()], seq);
                    applied_n += 1;
                    let decoy = Bytes::from(format!("decoy-{}", applied_n).into_bytes());
                    side.body.applied_from_scope = vec![decoy.clone()];
                    store.put_block_message(&side).expect("store side");
                    probe_sigs.push(decoy);
                    seq += 1;
                    let mut m = block_at(
                        h,
                        vec![prev.block_hash.clone(), side.block_hash.clone()],
                        seq,
                    );
                    m.body.merge_base = prev.block_hash.clone();
                    m
                } else {
                    block_at(h, vec![prev.block_hash.clone()], seq)
                };
                for _ in 0..rand(3) {
                    deploy_n += 1;
                    let failed = rand(4) == 0;
                    let (sig, pd) = processed(deploy_n, failed);
                    b.body.deploys.push(pd);
                    probe_sigs.push(sig);
                }
                for _ in 0..rand(3) {
                    applied_n += 1;
                    let sig = Bytes::from(format!("applied-{}", applied_n).into_bytes());
                    b.body.applied_from_scope.push(sig.clone());
                    probe_sigs.push(sig);
                }
                store.put_block_message(&b).expect("store block");
                prev = b;
            }
            probe_sigs.push(Bytes::from_static(b"never-seen-anywhere"));

            let tip = prev.block_hash.clone();
            for bound in [0, len / 2, len + 1] {
                for _pass in 0..2 {
                    let (set, _walked) = settled_sigs_of_lineage(&store, &tip, bound)
                        .expect("batched walk on a complete segment");
                    for sig in &probe_sigs {
                        let reference = reference_effect_in_state(&store, &tip, sig, bound)
                            .expect("reference walk on a complete segment");
                        let label = hex::encode(&sig[..8.min(sig.len())]);
                        assert_eq!(
                            reference,
                            set.contains(sig),
                            "batched membership diverged from the reference walk \
                             (bound {bound}, sig {label})",
                        );
                        assert_eq!(
                            reference,
                            effect_in_state_of(&store, &tip, sig, bound).expect("per-sig walk"),
                            "per-sig membership diverged from the reference walk \
                             (bound {bound}, sig {label})",
                        );
                    }
                }
            }
        }
        assert!(
            crate::rust::finality::block_facts::cache_bytes()
                <= crate::rust::finality::block_facts::MAX_CACHE_BYTES,
            "block-facts cache must respect its byte budget"
        );
    }

    /// The multi-floor probe answers exactly as the reference per-floor
    /// loop (first floor whose lineage holds the sig wins), across sigs
    /// held by the first floor, only a later floor, or none.
    #[tokio::test]
    async fn floor_probe_matches_the_reference_floor_loop() {
        let store = store().await;
        let genesis = block_at(0, vec![], 300);
        let (sig_a, pd_a) = processed(301, false);
        let mut floor1 = block_at(1, vec![genesis.block_hash.clone()], 301);
        floor1.body.deploys = vec![pd_a];
        let (sig_b, pd_b) = processed(302, false);
        let mut mid = block_at(1, vec![genesis.block_hash.clone()], 302);
        mid.body.deploys = vec![pd_b];
        let floor2 = block_at(2, vec![mid.block_hash.clone()], 303);
        for b in [&genesis, &floor1, &mid, &floor2] {
            store.put_block_message(b).expect("store block");
        }
        let floors = vec![
            (floor1.block_hash.clone(), 0),
            (floor2.block_hash.clone(), 0),
        ];
        let sig_absent = Bytes::from_static(b"absent-floor-sig");
        let mut probe = FloorSettledProbe::new(floors.clone());
        for sig in [&sig_a, &sig_b, &sig_absent] {
            let reference = floors
                .iter()
                .map(|(hash, bound)| reference_effect_in_state(&store, hash, sig, *bound))
                .try_fold(false, |acc, r| r.map(|hit| acc || hit))
                .expect("reference loop");
            assert_eq!(
                reference,
                probe.settled(&store, sig).expect("probe"),
                "floor probe diverged from the reference loop for sig {}",
                hex::encode(&sig[..8.min(sig.len())]),
            );
        }
    }

    /// The probe keeps the reference loop's short-circuit: a sig the FIRST
    /// floor holds answers TRUE without reading the second floor at all,
    /// so a gap in the later floor's lineage cannot poison that probe. A
    /// sig no floor holds must still reach the gap and refuse, exactly as
    /// the reference loop does.
    #[tokio::test]
    async fn floor_probe_short_circuit_skips_unavailable_later_floors() {
        let store = store().await;
        let genesis = block_at(0, vec![], 310);
        let (sig_a, pd_a) = processed(311, false);
        let mut floor1 = block_at(1, vec![genesis.block_hash.clone()], 311);
        floor1.body.deploys = vec![pd_a];
        let absent = block_at(1, vec![], 312);
        let floor2 = block_at(2, vec![absent.block_hash.clone()], 313);
        for b in [&genesis, &floor1, &floor2] {
            store.put_block_message(b).expect("store block");
        }
        let floors = vec![
            (floor1.block_hash.clone(), 0),
            (floor2.block_hash.clone(), 0),
        ];

        let mut probe = FloorSettledProbe::new(floors.clone());
        assert!(
            probe.settled(&store, &sig_a).expect("first floor answers"),
            "a first-floor hit must not read the gapped later floor"
        );

        let sig_unknown = Bytes::from_static(b"unknown-floor-sig");
        let err = probe
            .settled(&store, &sig_unknown)
            .expect_err("an unanswered probe must reach the gap and refuse");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h, _) if *h == absent.block_hash),
            "the refusal must name the missing block typed; got: {}",
            err
        );
    }

    /// Cached facts serve `body.rejected_deploys` verbatim, refuse an absent
    /// block typed, still serve a malformed multi-parent block's records
    /// (only stepping through it is refused), and stay a function of the
    /// supplied store: a store that does not hold a cached block gets
    /// `BlockNotHeld` under the cache-revalidation label.
    #[tokio::test]
    async fn rejected_records_load_through_the_cache_per_store() {
        let rejected_records_of = |store: &KeyValueBlockStore, hash: &BlockHash| {
            block_facts(store, hash, "test").map(|facts| facts.rejected.clone())
        };
        let store = store().await;
        let genesis = block_at(0, vec![], 320);
        let record = RejectedDeploy {
            sig: Bytes::from_static(b"rejected-sig"),
            carrier: Bytes::from_static(b"carrier-hash-000000000000000000"),
            duplicate: false,
        };
        let mut a = block_at(1, vec![genesis.block_hash.clone()], 321);
        a.body.rejected_deploys = vec![record.clone()];
        // Malformed: two parents, no recorded merge_base.
        let mut m = block_at(
            2,
            vec![a.block_hash.clone(), genesis.block_hash.clone()],
            322,
        );
        m.body.rejected_deploys = vec![record.clone()];
        for b in [&genesis, &a, &m] {
            store.put_block_message(b).expect("store block");
        }

        let records = rejected_records_of(&store, &a.block_hash).expect("load records");
        assert_eq!(records.as_ref(), &vec![record.clone()]);
        let cached = rejected_records_of(&store, &a.block_hash).expect("cache hit");
        assert_eq!(cached.as_ref(), &vec![record.clone()]);

        let malformed = rejected_records_of(&store, &m.block_hash)
            .expect("a malformed block still serves its own records");
        assert_eq!(malformed.as_ref(), &vec![record.clone()]);
        assert!(
            settled_sigs_of_lineage(&store, &m.block_hash, 0).is_err(),
            "stepping through the malformed block must still refuse"
        );

        let absent = block_at(3, vec![], 323);
        let err =
            rejected_records_of(&store, &absent.block_hash).expect_err("absence must refuse typed");
        assert!(matches!(
            err,
            CasperError::BlockNotHeld(ref h, ref site)
                if *h == absent.block_hash && site.accessor() == "test"
        ));

        let other_store = store_fn_second().await;
        let err = rejected_records_of(&other_store, &a.block_hash)
            .expect_err("a store that does not hold the block must refuse despite the cache");
        assert!(
            matches!(
                err,
                CasperError::BlockNotHeld(ref h, ref site)
                    if *h == a.block_hash && site.accessor() == "block-facts cache revalidation"
            ),
            "a cache hit the caller's store does not hold must be labelled apart from plain \
             absence; got: {}",
            err
        );
    }

    async fn store_fn_second() -> KeyValueBlockStore {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("second block store")
    }

    /// The batched walk's documented strengthening: it covers the FULL
    /// segment, so a gap below an applied sig refuses the whole answer
    /// (typed `BlockNotHeld`), where the per-sig reference walk answers
    /// TRUE for that sig without ever reaching the gap. Fail-closed is the
    /// safe direction — availability must not shape verdicts.
    #[tokio::test]
    async fn batched_walk_is_fail_closed_on_a_gapped_segment() {
        let store = store().await;
        let absent = block_at(1, vec![], 1);
        let (sig_top, pd) = processed(90, false);
        let mut b = block_at(2, vec![absent.block_hash.clone()], 2);
        b.body.deploys = vec![pd];
        store.put_block_message(&b).expect("store block");

        assert!(
            effect_in_state_of(&store, &b.block_hash, &sig_top, 0)
                .expect("the reference walk answers above the gap"),
            "reference: the sig applied above the gap is TRUE without \
             reading the gap"
        );
        let err = settled_sigs_of_lineage(&store, &b.block_hash, 0)
            .expect_err("the batched walk must refuse a gapped segment");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h, _) if *h == absent.block_hash),
            "the refusal must carry the missing block typed; got: {}",
            err
        );
    }
}

#[cfg(test)]
mod settle_and_release_tests {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::{BlockMessage, RejectedDeploy};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn chain_block(number: i64, parent: Option<&BlockMessage>) -> BlockMessage {
        get_random_block(
            Some(number),
            Some(number as i32),
            None,
            None,
            None,
            None,
            Some(number),
            Some(
                parent
                    .map(|p| vec![p.block_hash.clone()])
                    .unwrap_or_default(),
            ),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("test".to_string()),
            None,
        )
    }

    /// A row holding only a rejection record has no inclusion this node
    /// ingested, so it can never become terminal. Once the LFB is past the
    /// bound beyond the record, no block can include the sig, and the row is
    /// dropped instead of being scanned by every later settle.
    #[tokio::test]
    async fn a_record_only_row_is_dropped_once_past_contest() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let sig = Bytes::from_static(b"record-only-sig");
        let genesis = chain_block(0, None);
        let mut recorder = chain_block(1, Some(&genesis));
        recorder.body.rejected_deploys = vec![RejectedDeploy {
            sig: sig.clone(),
            carrier: Bytes::from_static(b"carrier-never-ingested-here-000"),
            duplicate: false,
        }];
        let mut chain = vec![genesis, recorder];
        for number in 2..=4 {
            let next = chain_block(number, chain.last());
            chain.push(next);
        }
        for block in &chain {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&chain[0], InsertMode::Approved)
            .expect("insert genesis");
        for block in &chain[1..] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }
        let row_open = |dag: &KeyValueDagRepresentation| {
            dag.open_lifecycle_rows()
                .expect("open rows")
                .iter()
                .any(|(open, _)| *open == sig.to_vec())
        };

        // Record at height 1, lifespan 1, bound 1: decidable only past 3.
        dag_storage
            .record_directly_finalized(chain[3].block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt #3");
        let dag = dag_storage.get_representation().expect("dag");
        settle(&dag, &block_store, 1, Some(1), &mut LineageMemo::default()).expect("settle at #3");
        assert!(row_open(&dag), "at the bound the row must still stand");

        dag_storage
            .record_directly_finalized(chain[4].block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt #4");
        let dag = dag_storage.get_representation().expect("dag");
        let terminal = settle(&dag, &block_store, 1, Some(1), &mut LineageMemo::default())
            .expect("settle at #4")
            .terminal;
        assert!(terminal.is_empty(), "a record-only row is never terminal");
        assert!(!row_open(&dag), "past the bound the row is dropped");
        assert!(dag.deploy_terminal(&sig).expect("lookup").is_none());
    }

    /// An error deciding one row costs that row only: the other rows still
    /// settle, and the failing row stays open for the next settle.
    #[tokio::test]
    async fn an_error_on_one_row_does_not_stop_the_others() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let deploy = |n: i32, valid_after: i64| {
            crate::rust::util::construct_deploy::source_deploy_now_full(
                format!("@\"row_isolation_{n}\"!(1)"),
                None,
                None,
                None,
                Some(valid_after),
                Some("test".to_string()),
            )
            .expect("deploy")
        };

        // The restore horizon: #5 exists only as a hash pointer.
        let absent = chain_block(5, None);
        let anchor = chain_block(6, Some(&absent));
        // A failed execution off the LFB's closure: deciding Failed walks the
        // LFB's ancestry down past the anchor and reaches the absent block.
        let failing = deploy(1, 6);
        let mut side = chain_block(4, None);
        let mut failed_pd =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(failing.clone());
        failed_pd.is_failed = true;
        side.body.deploys = vec![failed_pd];
        let clean = deploy(2, 6);
        let mut carrier = chain_block(7, Some(&anchor));
        carrier.body.deploys = vec![
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(clean.clone()),
        ];
        let next = chain_block(8, Some(&carrier));
        let lfb = chain_block(9, Some(&next));

        for block in [&anchor, &side, &carrier, &next, &lfb] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&anchor, InsertMode::Approved)
            .expect("insert anchor");
        for block in [&side, &carrier, &next, &lfb] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }
        dag_storage
            .record_directly_finalized(lfb.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt the tip");
        let dag = dag_storage.get_representation().expect("dag");

        let terminal = settle(&dag, &block_store, 1, Some(1), &mut LineageMemo::default())
            .expect("a row error must not fail the settle")
            .terminal;
        assert_eq!(terminal, vec![clean.sig.clone()]);
        assert_eq!(
            dag.deploy_terminal(&clean.sig)
                .expect("lookup")
                .map(|t| t.state),
            Some(TerminalState::Finalized)
        );
        assert!(
            dag.deploy_terminal(&failing.sig).expect("lookup").is_none(),
            "the row that errored stays open for the next settle"
        );
    }

    /// The release reads only the pool and the terminal table, so it removes
    /// exactly the copies that have a verdict, and a second call finds
    /// nothing left to do.
    #[tokio::test]
    async fn the_release_removes_exactly_the_pool_copies_with_a_verdict() {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let pool = parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        );
        let genesis = chain_block(0, None);
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");

        let settled =
            crate::rust::util::construct_deploy::basic_deploy_data(1, None, None).expect("deploy");
        let pending =
            crate::rust::util::construct_deploy::basic_deploy_data(2, None, None).expect("deploy");
        pool.lock()
            .add(vec![settled.clone(), pending.clone()])
            .expect("seed pool");

        let dag = dag_storage.get_representation().expect("dag");
        dag.put_deploy_terminal_if_absent(&settled.sig, TerminalRecord {
            state: TerminalState::Finalized,
            rejection_count: 0,
            latest_height: 0,
            latest_block_hash: Vec::new(),
        })
        .expect("terminal");

        assert_eq!(release_settled(&dag, &pool).expect("release"), 1);
        assert!(!pool.lock().contains_sig(&settled.sig).expect("lookup"));
        assert!(pool.lock().contains_sig(&pending.sig).expect("lookup"));
        assert_eq!(
            release_settled(&dag, &pool).expect("second release"),
            0,
            "nothing is left to release"
        );
    }

    fn processed(
        deploy: crypto::rust::signatures::signed::Signed<
            models::rust::casper::protocol::casper_message::DeployData,
        >,
        is_failed: bool,
    ) -> models::rust::casper::protocol::casper_message::ProcessedDeploy {
        let mut pd = models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(deploy);
        pd.is_failed = is_failed;
        pd
    }

    fn valid_after(
        n: i32,
        valid_after: i64,
    ) -> crypto::rust::signatures::signed::Signed<
        models::rust::casper::protocol::casper_message::DeployData,
    > {
        crate::rust::util::construct_deploy::source_deploy_now_full(
            format!("@\"memo_{n}\"!(1)"),
            None,
            None,
            None,
            Some(valid_after),
            Some("test".to_string()),
        )
        .expect("deploy")
    }

    /// A row that never closes (a failed execution under the unlimited
    /// sentinel) keeps the walk's bound at its `valid_after`. Each settle
    /// still walks only the lineage above the last one, and the verdicts match
    /// a settle that walks it all.
    #[tokio::test]
    async fn a_settle_walks_only_the_lineage_above_the_last_walk() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let stuck = valid_after(1, 1);
        let late = valid_after(2, 29);
        let genesis = chain_block(0, None);
        let mut chain = vec![genesis];
        for number in 1..=40 {
            let mut next = chain_block(number, chain.last());
            match number {
                1 => next.body.deploys = vec![processed(stuck.clone(), true)],
                30 => next.body.deploys = vec![processed(late.clone(), false)],
                _ => {}
            }
            chain.push(next);
        }
        for block in &chain {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&chain[0], InsertMode::Approved)
            .expect("insert genesis");
        for block in &chain[1..] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let mut memo = LineageMemo::default();
        for height in 2..=40usize {
            dag_storage
                .record_directly_finalized(chain[height].block_hash.clone(), 0.5, |_| async {
                    Ok(())
                })
                .await
                .expect("adopt");
            let dag = dag_storage.get_representation().expect("dag");
            let settled = settle(&dag, &block_store, 50, None, &mut memo).expect("settle");
            let expected_walk = if height == 2 { 2 } else { 1 };
            assert_eq!(
                settled.walked, expected_walk,
                "at #{height} only the blocks above the last walk's start are read"
            );
            assert_eq!(
                settle(&dag, &block_store, 50, None, &mut memo)
                    .expect("repeat settle")
                    .walked,
                0,
                "an unchanged LFB reads nothing"
            );
            assert_eq!(
                dag.deploy_terminal(&late.sig)
                    .expect("lookup")
                    .map(|t| t.state),
                (height >= 30).then_some(TerminalState::Finalized),
                "at #{height}"
            );
            assert!(dag.deploy_terminal(&stuck.sig).expect("lookup").is_none());
        }

        let dag = dag_storage.get_representation().expect("dag");
        assert_eq!(
            settle(&dag, &block_store, 50, None, &mut LineageMemo::default())
                .expect("settle without a memo")
                .walked,
            40,
            "without the memo every settle walks down to the stuck row"
        );
    }

    /// A new LFB whose lineage does not pass through the last walk's start
    /// (a merge over a lower base) gets a full walk: the memo answers only
    /// for the lineage below that start.
    #[tokio::test]
    async fn a_lineage_that_misses_the_last_start_is_walked_in_full() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let deploy = valid_after(3, 0);
        let genesis = chain_block(0, None);
        let a1 = chain_block(1, Some(&genesis));
        let a2 = chain_block(2, Some(&a1));
        let a3 = chain_block(3, Some(&a2));
        let mut b1 = chain_block(1, Some(&genesis));
        b1.body.deploys = vec![processed(deploy.clone(), false)];
        let mut merge = get_random_block(
            Some(4),
            Some(4),
            None,
            None,
            None,
            None,
            Some(4),
            Some(vec![a3.block_hash.clone(), b1.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("test".to_string()),
            None,
        );
        merge.body.merge_base = b1.block_hash.clone();

        for block in [&genesis, &a1, &a2, &a3, &b1, &merge] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&a1, &a2, &a3, &b1, &merge] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let mut memo = LineageMemo::default();
        dag_storage
            .record_directly_finalized(a3.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt a3");
        let dag = dag_storage.get_representation().expect("dag");
        let settled = settle(&dag, &block_store, 50, Some(50), &mut memo).expect("settle at a3");
        assert!(
            settled.terminal.is_empty(),
            "fixture precondition: a3's lineage does not apply the deploy"
        );

        dag_storage
            .record_directly_finalized(merge.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt the merge");
        let dag = dag_storage.get_representation().expect("dag");
        let settled =
            settle(&dag, &block_store, 50, Some(50), &mut memo).expect("settle at the merge");
        assert_eq!(settled.walked, 3, "merge, b1 and genesis");
        assert_eq!(settled.terminal, vec![deploy.sig.clone()]);
        assert_eq!(
            dag.deploy_terminal(&deploy.sig)
                .expect("lookup")
                .map(|t| t.state),
            Some(TerminalState::Finalized)
        );
    }
}

#[cfg(test)]
mod stage_metric_tests {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    use super::*;
    use crate::rust::metrics_constants::{
        DEPLOY_LIFECYCLE_CARRIER_PRUNE_TIME_METRIC, DEPLOY_LIFECYCLE_SETTLE_TIME_METRIC,
    };

    #[test]
    fn a_settle_records_its_time_and_the_carrier_prune_only_when_it_ran() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);

        record_settle_timing(2_500, Some(2_000));
        record_settle_timing(40, None);

        let mut samples: std::collections::HashMap<String, Vec<f64>> =
            std::collections::HashMap::new();
        for (key, (_, _, value)) in snapshotter.snapshot().into_hashmap() {
            if let DebugValue::Histogram(s) = value {
                samples
                    .entry(key.key().name().to_owned())
                    .or_default()
                    .extend(s.iter().map(|v| v.into_inner()));
            }
        }
        assert_eq!(
            samples.get(DEPLOY_LIFECYCLE_SETTLE_TIME_METRIC),
            Some(&vec![2.5, 0.04])
        );
        assert_eq!(
            samples.get(DEPLOY_LIFECYCLE_CARRIER_PRUNE_TIME_METRIC),
            Some(&vec![2.0])
        );
    }
}
