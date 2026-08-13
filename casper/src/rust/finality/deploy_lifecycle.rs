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
//! - **Expired** ⟺ beyond the bound, not in the state otherwise: the
//!   window is closed and nothing can ever apply it.
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

use super::floor::{self, in_floor_closure, Floor};
use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::FtThreshold;

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
        let Some(block) = block_store.get(&cur)? else {
            return Err(CasperError::Other(format!(
                "effect_in_state_of: block {} on the base lineage is absent \
                 from the store — refusing to judge membership from an \
                 incomplete lineage",
                hex::encode(&cur[..8.min(cur.len())]),
            )));
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
        let open = dag
            .open_lifecycle_sigs()
            .map_err(CasperError::KvStoreError)?;
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
        ftt: FtThreshold,
        citability_horizon: Option<i64>,
    ) -> Result<Vec<Bytes>, CasperError> {
        // The async floor read happens OUTSIDE the schedule lock; it is
        // memoized in the persisted floor index.
        let block_floor = floor::floor_of_block(dag, &block.block_hash, ftt).await?;

        let mut schedule = self.schedule.lock();
        if !schedule.rebuilt {
            drop(schedule);
            self.rebuild_schedule(dag)?;
            schedule = self.schedule.lock();
        }

        // The floor clock (monotone).
        let floor_advanced = match &schedule.max_floor {
            Some(current) => block_floor.block_number > current.block_number,
            None => true,
        };
        if floor_advanced {
            schedule.max_floor = Some(block_floor);
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
        .map_err(CasperError::KvStoreError)?
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
    let member = effect_in_state_of_above(
        block_store,
        &max_floor.hash,
        sig,
        valid_after,
        checked_below.as_ref(),
    )?;
    if member {
        write_terminal(dag, sig, TerminalState::Finalized, &row, terminalized)?;
        schedule.checked.remove(sig);
        return Ok(());
    }
    schedule.checked.insert(sig.clone(), max_floor.hash.clone());

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
/// the latest event names the sig's most recent canonical appearance.
fn frozen_display(row: &LifecycleEvents) -> (u32, i64, Vec<u8>) {
    let rejection_count = row
        .events
        .iter()
        .filter(|e| matches!(e.kind, LifecycleEventKind::Rejected { .. }))
        .count() as u32;
    let latest = row
        .events
        .iter()
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
        .map_err(CasperError::KvStoreError)?;
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
}
