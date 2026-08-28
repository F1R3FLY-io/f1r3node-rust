//! The deploy-lifecycle register: terminal deploy verdicts — Finalized,
//! Expired, Failed — determined ONCE, at the moment their monotone inputs
//! make them true, and written write-once into the DAG storage's lifecycle
//! tables. The status API reads; it never computes.
//!
//! Every input to a verdict is frozen consensus data: the floor clock
//! (monotone) and the state-construction facts blocks record (`deploys`,
//! `applied_from_scope`, `merge_base`). Records feed the recovery plane
//! and the display row; they do NOT decide verdicts — a verdict computed
//! from a node's record row is a function of record ARRIVAL ORDER, which
//! write-once semantics then freeze (verified divergence: hunt specimen
//! 87a5d970).
//!
//! Verdict rules:
//! - **Finalized** ⟺ the sig's effect is in the FLOOR's committed state
//!   (membership by recorded construction pointers). Floor-covered effects
//!   are in every future merge base — membership at the floor is monotone
//!   — so Finalized is decided AT COVERAGE, re-checked per floor advance
//!   with a per-sig checked-up-to memo bounding each walk to the new
//!   lineage segment.
//! - **Failed** ⟺ beyond the contestability bound, not in the state, and
//!   a floor-covered `is_failed` execution exists (it ran and failed; the
//!   charge landed).
//! - **Expired** ⟺ beyond the bound, not in the state, and the whole
//!   lineage segment down to the bound was READABLE: on a truncated node a
//!   sig whose walk crosses the restore horizon is unknowable, and the
//!   register abstains (Pending) rather than invent a terminal verdict.
//!
//! The CONTESTABILITY BOUND is `max(window_end, last inclusion) +
//! citability horizon`, past which no admissible block can adjudicate,
//! re-apply, or re-include the sig (the parent-depth spread rule refuses
//! late citations). The horizon derives from `max_parent_depth` ALONE —
//! shard config, never a node-local value or a constant; the unlimited
//! sentinel disables only the Expired/Failed side (nothing is ever
//! provably beyond contest), never Finalized.
//!
//! Event rows are fed by `BlockDagKeyValueStorage::insert` itself, so the
//! register never walks bodies. This module owns only the VOLATILE
//! schedule (thresholds, clocks) — rebuilt from the persisted open rows
//! at startup — and the evaluation logic. The terminal write is
//! `put_deploy_terminal_if_absent`: never-flip is enforced by the store,
//! not argued per call site.

use std::collections::{BTreeMap, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::deploy_lifecycle_types::{
    LifecycleEventKind, LifecycleEvents, TerminalRecord, TerminalState,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use super::floor::{in_floor_closure, Floor};
use crate::rust::errors::CasperError;

/// The citability horizon: how far below the floor an admissible block can
/// still cite. Derives from `max_parent_depth` ALONE (shard config — the
/// parent-spread validity rule makes deeper citations InvalidParents);
/// the unlimited sentinel means nothing is ever provably uncitable.
pub(crate) fn citability_horizon(max_parent_depth: i32) -> Option<i64> {
    (max_parent_depth != i32::MAX).then_some(i64::from(max_parent_depth))
}

/// True iff `block_hash`'s committed state contains `sig`'s effect: some
/// block on its base lineage either executed it fresh (a non-failed
/// `deploys` entry) or re-applied its chain from scope
/// (`applied_from_scope`). The lineage is the recorded `merge_base` chain;
/// where the recorded base is empty the header derives it (single parent)
/// or the lineage is exhausted (genesis).
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
    effect_in_state_of_above(block_store, block_hash, sig, min_height, None)
}

/// `effect_in_state_of` with an optional early stop: `checked_below` names
/// a lineage block whose segment (itself and below) a previous evaluation
/// already answered FALSE for — reaching it ends the walk without
/// re-reading the old segment.
fn effect_in_state_of_above(
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    sig: &Bytes,
    min_height: i64,
    checked_below: Option<&BlockHash>,
) -> Result<bool, CasperError> {
    let mut cur = block_hash.clone();
    loop {
        if checked_below == Some(&cur) {
            return Ok(false);
        }
        // Absence is a statement about THIS node's history (a truncated
        // node lacks bodies below its restore horizon), never a judgement:
        // typed so every availability classifier downstream can defer,
        // fetch, or abstain instead of laundering it into a verdict.
        let Some(block) = block_store.get(&cur)? else {
            return Err(CasperError::BlockNotHeld(cur));
        };
        if block.body.state.block_number < min_height {
            return Ok(false);
        }
        // A failed execution's deploy is in the body while its effect is
        // NOT in the state — only successful executions count.
        if block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == *sig && !pd.is_failed)
        {
            return Ok(true);
        }
        if block.body.applied_from_scope.iter().any(|s| s == sig) {
            return Ok(true);
        }
        cur = if !block.body.merge_base.is_empty() {
            block.body.merge_base.clone()
        } else {
            match block.header.parents_hash_list.as_slice() {
                // Genesis: the lineage is exhausted.
                [] => return Ok(false),
                // Single parent: the base is the sole parent, already
                // consensus data in the header — not re-recorded.
                [parent] => parent.clone(),
                // A multi-parent block's state parent is NOT derivable from
                // the header alone (merged: the floor; fast-path: the
                // covering parent). Its absence is a malformed block, never
                // a guess.
                _ => {
                    return Err(CasperError::Other(format!(
                        "effect_in_state_of: multi-parent block {} carries \
                         no recorded merge_base — refusing to guess its \
                         state lineage",
                        hex::encode(&cur[..8.min(cur.len())]),
                    )))
                }
            }
        };
    }
}

/// One cached lineage step: everything the batched walk needs from a block
/// without re-reading its body. Content-addressed by block hash, so an
/// entry can never go stale.
struct LineageStep {
    block_number: i64,
    /// Next block on the state lineage; `None` means genesis (exhausted).
    base: Option<BlockHash>,
    /// The block's applied-sig facts: sigs of its non-failed `deploys`
    /// entries plus its `applied_from_scope` list.
    sigs: std::sync::Arc<HashSet<Bytes>>,
}

/// Byte budget for the per-block lineage-step cache, tracked from each
/// entry's measured sig bytes (an entry-count cap understates fat blocks:
/// 128 sigs × ~70B is ~9KB for one entry). On overflow the cache is
/// cleared rather than evicted piecewise — entries are pure functions of
/// immutable bodies, so losing them costs only a re-read. Repeated
/// clear-rebuild thrash would need one walk's entries to approach the
/// budget, and walk depth is bounded far below it by the deterministic
/// floor-distance merge backstop (`merge_scope_backstop_exceeded`).
const LINEAGE_STEP_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

/// Per-entry overhead estimate added to the measured sig bytes: map slot,
/// `Arc` headers, hash key.
const LINEAGE_STEP_ENTRY_OVERHEAD_BYTES: usize = 128;

#[derive(Default)]
struct LineageStepCache {
    map: HashMap<BlockHash, std::sync::Arc<LineageStep>>,
    approx_bytes: usize,
}

impl LineageStepCache {
    fn insert(&mut self, hash: BlockHash, step: std::sync::Arc<LineageStep>) {
        let entry_bytes =
            LINEAGE_STEP_ENTRY_OVERHEAD_BYTES + step.sigs.iter().map(Bytes::len).sum::<usize>();
        if self.approx_bytes + entry_bytes > LINEAGE_STEP_CACHE_MAX_BYTES {
            self.map.clear();
            self.approx_bytes = 0;
        }
        self.approx_bytes += entry_bytes;
        self.map.insert(hash, step);
    }
}

fn lineage_step_cache() -> &'static parking_lot::Mutex<LineageStepCache> {
    static CACHE: std::sync::OnceLock<parking_lot::Mutex<LineageStepCache>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(LineageStepCache::default()))
}

#[cfg(test)]
fn lineage_step_cache_bytes() -> usize { lineage_step_cache().lock().approx_bytes }

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
    let mut walked = 0usize;
    let mut cur = block_hash.clone();
    loop {
        // The cache is process-global and keyed by hash alone, while this
        // function's answer must be a function of the SUPPLIED store: a hit
        // is revalidated with a raw key-existence check (no decompression,
        // no decode) so a block this store does not hold is `BlockNotHeld`
        // exactly as on the cold path — availability semantics survive the
        // cache, and one process serving several stores (tests) cannot
        // cross-answer.
        let cached = lineage_step_cache().lock().map.get(&cur).cloned();
        let cached = match cached {
            Some(step) if block_store.contains_key(&cur)? => Some(step),
            _ => None,
        };
        let step = match cached {
            Some(step) => step,
            None => {
                // Miss path: two short lock acquisitions (lookup above,
                // insert below) are deliberate — the store read between
                // them is I/O and must not run under the lock.
                let Some(block) = block_store.get(&cur)? else {
                    return Err(CasperError::BlockNotHeld(cur));
                };
                let base = if !block.body.merge_base.is_empty() {
                    Some(block.body.merge_base.clone())
                } else {
                    match block.header.parents_hash_list.as_slice() {
                        [] => None,
                        [parent] => Some(parent.clone()),
                        _ => {
                            return Err(CasperError::Other(format!(
                                "settled_sigs_of_lineage: multi-parent block {} \
                                 carries no recorded merge_base — refusing to \
                                 guess its state lineage",
                                hex::encode(&cur[..8.min(cur.len())]),
                            )))
                        }
                    }
                };
                let mut sigs: HashSet<Bytes> = block
                    .body
                    .deploys
                    .iter()
                    .filter(|pd| !pd.is_failed)
                    .map(|pd| pd.deploy.sig.clone())
                    .collect();
                sigs.extend(block.body.applied_from_scope.iter().cloned());
                let step = std::sync::Arc::new(LineageStep {
                    block_number: block.body.state.block_number,
                    base,
                    sigs: std::sync::Arc::new(sigs),
                });
                lineage_step_cache()
                    .lock()
                    .insert(cur.clone(), step.clone());
                step
            }
        };
        if step.block_number < min_height {
            return Ok((settled, walked));
        }
        walked += 1;
        settled.extend(step.sigs.iter().cloned());
        match &step.base {
            None => return Ok((settled, walked)),
            Some(base) => cur = base.clone(),
        }
    }
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

#[derive(Default)]
struct Schedule {
    /// Sigs to re-evaluate once the max frozen lm-floor height reaches the
    /// key (next floor advance for coverage re-checks; the contestability
    /// bound for Expired/Failed).
    floor_thresholds: BTreeMap<i64, HashSet<Bytes>>,
    /// The monotone max frozen latest-message floor — the highest floor
    /// any known canonical block carries. The register's ONE clock.
    max_floor: Option<Floor>,
    /// Per-sig coverage memo: the floor block whose lineage a previous
    /// membership check already answered FALSE for. The next check walks
    /// only the new segment above it.
    checked: HashMap<Bytes, BlockHash>,
    /// Sigs whose membership walk crossed the restore horizon: the segment
    /// below is unreadable on this node, so "not in the state" — the
    /// premise of Expired and Failed — is unknowable for them. They stay
    /// Pending; only a readable re-application above the horizon (found by
    /// the ongoing coverage re-checks) can still settle them Finalized.
    horizon_blocked: HashSet<Bytes>,
    /// Set once `rebuild_schedule` has armed the persisted open rows.
    rebuilt: bool,
}

/// The register's volatile half: schedule state plus the evaluation
/// driver. One per casper instance; all persisted state lives in the DAG
/// storage's lifecycle tables.
#[derive(Default)]
pub struct DeployLifecycle {
    schedule: parking_lot::Mutex<Schedule>,
}

impl DeployLifecycle {
    /// Arm every persisted open sig for evaluation at the next observed
    /// block (threshold 0 crosses immediately). Verdicts only get MORE
    /// settled by waiting, so a conservative cold start is sound; a crash
    /// between an insert and its evaluation costs nothing but delay.
    pub fn rebuild_schedule(&self, dag: &KeyValueDagRepresentation) -> Result<(), CasperError> {
        let mut schedule = self.schedule.lock();
        let open = dag.open_lifecycle_sigs().map_err(CasperError::from)?;
        let armed = schedule.floor_thresholds.entry(0).or_default();
        for sig in open {
            armed.insert(Bytes::from(sig));
        }
        schedule.rebuilt = true;
        Ok(())
    }

    /// The register's advance step, run for every accepted block (the
    /// proposer and validator paths both flow through block admission):
    /// bump the floor clock and evaluate the sigs whose thresholds
    /// crossed, plus the sigs this block touched. Ingest already happened
    /// inside the DAG insert.
    ///
    /// Returns the sigs that reached a TERMINAL verdict in this pass.
    /// Every terminal state means no further proposal is possible or
    /// needed, so the caller releases the proposer's pool copy against
    /// exactly this list; verdicts are write-once, so a sig is released
    /// at most once.
    pub async fn observe_block(
        &self,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        block: &BlockMessage,
        deploy_lifespan: i64,
        citability_horizon: Option<i64>,
    ) -> Result<Vec<Bytes>, CasperError> {
        // The register's clock is the node's ADOPTED LFB — the output of
        // `floor_of_view`, which is containment-guarded — never an admitted
        // block's frozen floor. A frozen floor is another validator's claim
        // about ITS chain: under a sibling-fork race it can sit on a branch
        // this node never adopted, and a write-once verdict keyed on it is
        // permanently incoherent with this node's read surface (the ucc
        // ca7197d8 fork wrote Finalized network-wide off one side's frozen
        // floor while two nodes served the other side).
        let adopted_hash = dag.last_finalized_block();
        let adopted_number = dag
            .lookup_unsafe(&adopted_hash)
            .map_err(CasperError::from)?
            .block_number;

        let mut schedule = self.schedule.lock();
        if !schedule.rebuilt {
            drop(schedule);
            self.rebuild_schedule(dag)?;
            schedule = self.schedule.lock();
        }

        // The floor clock (monotone; adoption itself is monotone per node).
        let floor_advanced = match &schedule.max_floor {
            Some(current) => adopted_number > current.block_number,
            None => true,
        };
        if floor_advanced {
            schedule.max_floor = Some(Floor {
                hash: adopted_hash,
                block_number: adopted_number,
            });
        }

        // Due: crossed thresholds plus the block's own touched sigs.
        let mut due: HashSet<Bytes> = HashSet::new();
        for pd in &block.body.deploys {
            due.insert(pd.deploy.sig.clone());
        }
        for rd in &block.body.rejected_deploys {
            due.insert(rd.sig.clone());
        }
        let floor_height = schedule
            .max_floor
            .as_ref()
            .map(|f| f.block_number)
            .unwrap_or(0);
        let crossed_floor: Vec<i64> = schedule
            .floor_thresholds
            .range(..=floor_height)
            .map(|(k, _)| *k)
            .collect();
        for key in crossed_floor {
            if let Some(sigs) = schedule.floor_thresholds.remove(&key) {
                due.extend(sigs);
            }
        }

        let mut due: Vec<Bytes> = due.into_iter().collect();
        due.sort();
        let mut terminalized: Vec<Bytes> = Vec::new();
        for sig in due {
            evaluate(
                &mut schedule,
                dag,
                block_store,
                &sig,
                deploy_lifespan,
                citability_horizon,
                &mut terminalized,
            )?;
        }
        Ok(terminalized)
    }
}

fn evaluate(
    schedule: &mut Schedule,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    sig: &Bytes,
    deploy_lifespan: i64,
    citability_horizon: Option<i64>,
    terminalized: &mut Vec<Bytes>,
) -> Result<(), CasperError> {
    let Some(row) = dag
        .deploy_lifecycle_events(sig)
        .map_err(CasperError::from)?
    else {
        return Ok(());
    };
    let Some(max_floor) = schedule.max_floor.clone() else {
        return Ok(());
    };
    let floor_height = max_floor.block_number;

    // A record-only row (carrier never observed) has no window basis yet;
    // its inclusion event will arm it.
    let Some(valid_after) = row.valid_after else {
        return Ok(());
    };

    // FINALIZED AT COVERAGE. Membership at the floor is monotone (a
    // floor-covered effect is in every future merge base), so the first
    // true answer is the verdict. The memo bounds the walk to the lineage
    // segment above the last floor already answered false.
    let checked_below = schedule.checked.get(sig).cloned();
    let member = match effect_in_state_of_above(
        block_store,
        &max_floor.hash,
        sig,
        valid_after,
        checked_below.as_ref(),
    ) {
        Ok(member) => member,
        // The walk crossed the restore horizon: the sig's verdict is
        // unknowable on this node, and that is a fact about the node,
        // never a failure of the block whose admission ran this
        // evaluation. Abstain: flag the sig, memoize the boundary (the
        // absent hash is on the lineage, so the early stop ends every
        // later walk there without re-reading), and re-arm — a readable
        // re-application above the horizon can still finalize it.
        Err(CasperError::BlockNotHeld(missing)) => {
            tracing::warn!(
                target: "f1r3fly.casper.lifecycle",
                sig = %hex::encode(&sig[..8.min(sig.len())]),
                missing = %hex::encode(&missing[..8.min(missing.len())]),
                "membership walk crossed the restore horizon: verdict \
                 unknowable on this node, sig stays Pending"
            );
            schedule.horizon_blocked.insert(sig.clone());
            schedule.checked.insert(sig.clone(), missing);
            schedule
                .floor_thresholds
                .entry(floor_height + 1)
                .or_default()
                .insert(sig.clone());
            return Ok(());
        }
        Err(other) => return Err(other),
    };
    if member {
        write_terminal(dag, sig, TerminalState::Finalized, &row, terminalized)?;
        schedule.checked.remove(sig);
        schedule.horizon_blocked.remove(sig);
        return Ok(());
    }
    schedule.checked.insert(sig.clone(), max_floor.hash.clone());

    // "Not in the state" over an unreadable segment is not established —
    // it is unknowable. Expired/Failed for a horizon-blocked sig would be
    // an invented terminal verdict; keep re-checking coverage instead.
    if schedule.horizon_blocked.contains(sig) {
        schedule
            .floor_thresholds
            .entry(floor_height + 1)
            .or_default()
            .insert(sig.clone());
        return Ok(());
    }

    // EXPIRED / FAILED — only beyond the contestability bound, past which
    // no admissible block can adjudicate, re-apply, or re-include the sig,
    // so "not in the state" is stable. `None` (depth checking disabled)
    // means nothing is ever provably beyond contest: the sig stays
    // Pending by design.
    let Some(bound) = citability_horizon else {
        schedule
            .floor_thresholds
            .entry(floor_height + 1)
            .or_default()
            .insert(sig.clone());
        return Ok(());
    };
    let window_end = valid_after + deploy_lifespan;
    let last_inclusion = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Included { .. }))
        .map(|e| e.height)
        .max()
        .unwrap_or(window_end);
    let decide_at = window_end.max(last_inclusion) + bound;
    if floor_height <= decide_at {
        // Not yet decidable: re-arm on the FLOOR clock — at the next
        // advance for the coverage re-check (a threshold that always
        // comes due, so a verdict is always eventually written).
        schedule
            .floor_thresholds
            .entry(floor_height + 1)
            .or_default()
            .insert(sig.clone());
        return Ok(());
    }

    let mut ran_and_failed = false;
    for event in &row.events {
        if matches!(event.kind, LifecycleEventKind::Included { is_failed: true }) {
            let block = Bytes::from(event.block_hash.clone());
            if in_floor_closure(dag, &block, &max_floor)? {
                ran_and_failed = true;
                break;
            }
        }
    }
    let state = if ran_and_failed {
        TerminalState::Failed
    } else {
        TerminalState::Expired
    };
    write_terminal(dag, sig, state, &row, terminalized)?;
    schedule.checked.remove(sig);
    Ok(())
}

/// Display fields for a terminal record, frozen from the event row it
/// prunes: every record event counts toward `rejection_count` (duplicates
/// included — the count is observability, not the causal ordering), and
/// the latest INCLUSION event names the sig's most recent canonical
/// appearance. A rejection record's block does not carry the deploy — a
/// record-carrier here sends every consumer that fetches the named block
/// looking for a deploy that is not in it.
fn frozen_display(row: &LifecycleEvents) -> (u32, i64, Vec<u8>) {
    let rejection_count = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Rejected { .. }))
        .count() as u32;
    let latest = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Included { .. }))
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
    terminalized: &mut Vec<Bytes>,
) -> Result<(), CasperError> {
    let (rejection_count, latest_height, latest_block_hash) = frozen_display(row);
    let written = dag
        .put_deploy_terminal_if_absent(sig, TerminalRecord {
            state,
            rejection_count,
            latest_height,
            latest_block_hash,
        })
        .map_err(CasperError::from)?;
    // The sig is now irreversibly settled on the floor clock, whatever
    // the verdict: the caller releases the proposer's pool copy against
    // exactly this list.
    terminalized.push(sig.clone());
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
    use models::rust::casper::protocol::casper_message::BlockMessage;
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

    /// The register's clock is the node's ADOPTED LFB, never an admitted
    /// block's frozen floor: a frozen floor on a branch this node has not
    /// adopted must not advance the clock or write a verdict (the ucc
    /// ca7197d8 fork wrote Finalized off one side's frozen floor while two
    /// nodes served the other side). The verdict lands exactly when the
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

        let register = DeployLifecycle::default();
        let terminalized = register
            .observe_block(&dag, &block_store, &b, 10, Some(10))
            .await
            .expect("observe b");
        assert!(
            terminalized.is_empty(),
            "no verdict may be written off an admitted block's frozen floor \
             while the adopted LFB is still genesis; got {:?}",
            terminalized
        );

        // The node itself adopts b; the very next observation writes the
        // verdict against the adopted chain.
        dag_storage
            .record_directly_finalized(b.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt b");
        let dag = dag_storage.get_representation().expect("dag");
        let terminalized = register
            .observe_block(&dag, &block_store, &c, 10, Some(10))
            .await
            .expect("observe c");
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

        // Arm the sig (adopted LFB still genesis: no verdict possible yet).
        let dag = dag_storage.get_representation().expect("dag");
        let register = DeployLifecycle::default();
        let armed = register
            .observe_block(&dag, &block_store, &b, 1, Some(1))
            .await
            .expect("observe b");
        assert!(armed.is_empty(), "no verdict before a covering adoption");

        // Adopt d (height 4): past decide_at = max(window_end, last
        // inclusion at 2) + bound = 3, so the Failed arm is LIVE — and the
        // failed execution at `a` is inside the adopted floor's closure.
        dag_storage
            .record_directly_finalized(d.block_hash.clone(), 0.5, |_| async { Ok(()) })
            .await
            .expect("adopt d");
        let dag = dag_storage.get_representation().expect("dag");
        let terminalized = register
            .observe_block(&dag, &block_store, &c, 1, Some(1))
            .await
            .expect("observe after adoption");

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
                    kind: LifecycleEventKind::Rejected {
                        duplicate: false,
                        carrier: inclusion_block.clone(),
                    },
                },
            ],
        };

        let (rejection_count, latest_height, latest_block_hash) = frozen_display(&row);
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
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == absent.block_hash),
            "absence must carry the missing block's name typed; got: {}",
            err
        );
    }

    /// A truncated node whose membership walk crosses the restore horizon
    /// must ABSORB the refusal, not error block admission: the blocked sig
    /// stays Pending (no invented Expired past the contestability bound —
    /// "not in the state" is unknowable over an unreadable segment), sibling
    /// sigs still evaluate, later observations do not re-error, and a
    /// readable re-application above the horizon still lands Finalized.
    #[tokio::test]
    async fn the_register_absorbs_the_horizon_instead_of_erroring_admission() {
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
        let register = DeployLifecycle::default();
        let terminalized = register
            .observe_block(&dag, &block_store, &w2, 1, Some(1))
            .await
            .expect("a horizon crossing must not error block admission");
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
        let terminalized = register
            .observe_block(&dag, &block_store, &w3, 1, Some(1))
            .await
            .expect("later observations must not re-error");
        assert!(terminalized.is_empty());
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
        let terminalized = register
            .observe_block(&dag, &block_store, &w4, 1, Some(1))
            .await
            .expect("observe w4");
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
    /// lineage-step cache-hit path against the same oracle.
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
                        let reference = effect_in_state_of(&store, &tip, sig, bound)
                            .expect("reference walk on a complete segment");
                        assert_eq!(
                            reference,
                            set.contains(sig),
                            "batched membership diverged from the reference walk \
                             (bound {}, sig {})",
                            bound,
                            hex::encode(&sig[..8.min(sig.len())]),
                        );
                    }
                }
            }
        }
        assert!(
            lineage_step_cache_bytes() <= LINEAGE_STEP_CACHE_MAX_BYTES,
            "lineage-step cache must respect its byte budget"
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
                .map(|(hash, bound)| effect_in_state_of(&store, hash, sig, *bound))
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
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == absent.block_hash),
            "the refusal must name the missing block typed; got: {}",
            err
        );
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
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == absent.block_hash),
            "the refusal must carry the missing block typed; got: {}",
            err
        );
    }
}
