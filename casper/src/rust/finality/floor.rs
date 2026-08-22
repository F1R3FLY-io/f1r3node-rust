//! The finalized floor — the per-block finalized cut, and the one finality
//! clock built on it.
//!
//! `floor(B)` is the highest STATE-SOUND finalized candidate over B's frozen
//! (parents, justifications) pair: the clique oracle certifies candidates
//! finalized over the block's own justification snapshot
//! ([`CliqueOracle::ft_witnessed_exact`], exact `>= θ`), and candidacy is
//! containment-gated — a candidate must contain every inherited floor's
//! settled effects ([`state_contains`]: sig-set inclusion over the recorded
//! positive state-construction facts) or provably re-collect them through
//! the merge it will base, so consecutive floors are state-monotone, never
//! merely DAG parent/child. Every input is consensus-checked block content
//! (bodies, signed justifications, immutable ancestor metadata), so every
//! honest node derives the same floor for the same block — no node-local
//! finality state participates. This is the linear-finality analog of
//! RChain's per-message fringe: the cut the block's merge builds on.
//!
//! [`floor_of_view`] runs the same derivation over the live frontier and is
//! the single LFB decision: the finalization runner and the API path both
//! consume it, and it advances only onto a floor that captures the current
//! LFB — the same soundness the per-block derivation runs. There is no
//! second finality clock.

use std::collections::{BTreeMap, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::Bond;
use models::rust::validator::Validator;
use prost::bytes::Bytes;

use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// The finalized cut a block builds on. Under linear finality this is a single
/// block: the highest witnessed-finalized ancestor across the block's parents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub hash: BlockHash,
    pub block_number: i64,
}

/// True iff `hash` is the floor block or one of its DAG ancestors —
/// i.e., its contents are represented in every future merge base.
pub(crate) fn in_floor_closure(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
    floor: &Floor,
) -> Result<bool, CasperError> {
    if *hash == floor.hash {
        return Ok(true);
    }
    let Some(height) = dag.block_number(hash) else {
        return Ok(false);
    };
    if height > floor.block_number {
        return Ok(false);
    }
    dag.is_dag_ancestor(hash, &floor.hash)
        .map_err(CasperError::from)
}

/// Per-block introduced-sig memo shared across the containment checks of
/// one derivation (the same settled segments are re-walked per candidate).
pub(crate) type IntroducedSigsMemo = HashMap<BlockHash, HashSet<Bytes>>;

/// The state-parent of a block, from metadata: the recorded merge base,
/// else the sole parent, else none at a root. A multi-parent block with no
/// recorded base has an underivable state lineage — refused, never
/// guessed.
fn state_parent_of(
    meta: &models::rust::block_metadata::BlockMetadata,
) -> Result<Option<BlockHash>, CasperError> {
    if !meta.merge_base.is_empty() {
        return Ok(Some(meta.merge_base.clone()));
    }
    match meta.parents.as_slice() {
        [] => Ok(None),
        [parent] => Ok(Some(parent.clone())),
        _ => Err(CasperError::Other(format!(
            "state lineage: multi-parent block {} carries no recorded merge \
             base — refusing to guess its state parent",
            PrettyPrinter::build_string_bytes(&meta.block_hash),
        ))),
    }
}

/// Metadata for a block a walk needs, or [`CasperError::BlockNotHeld`].
///
/// Every descent in this file can leave the blocks a node holds — a node
/// restored from a sync anchor has no history below it, and no walk here is
/// bounded by that anchor. Absence is therefore a statement about this node,
/// not about the blocks being compared, and the difference is load-bearing:
/// the state-lineage walk would otherwise report "disconnected", the floor
/// recursion would report a storage failure, and both become verdicts against
/// whoever proposed the block. Naming the missing block lets the caller fetch
/// it and retry.
fn held_meta(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
) -> Result<models::rust::block_metadata::BlockMetadata, CasperError> {
    dag.lookup(hash)
        .map_err(CasperError::from)?
        .ok_or_else(|| CasperError::BlockNotHeld(hash.clone()))
}

/// The block number of a block a walk needs, or [`CasperError::BlockNotHeld`].
fn held_number(dag: &KeyValueDagRepresentation, hash: &BlockHash) -> Result<i64, CasperError> {
    Ok(held_meta(dag, hash)?.block_number)
}

/// The outcome of walking two state lineages toward each other. Truncation is
/// deliberately NOT a variant: a lineage that leaves the blocks this node holds
/// is unreadable, not disconnected, and the two must never collapse into one
/// answer — the callers turn `Disconnected` into a containment refusal and a
/// skipped floor candidate, so deciding it from local retention would make the
/// floor node-local, which R-DET forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StateLineage {
    /// The lineages meet at this block.
    Meet(BlockHash),
    /// Both lineages reached a root without meeting — a genuinely incompatible fork.
    Disconnected,
}

/// The meet of two blocks' state lineages: the lowest common ancestor in
/// the state-parent tree. Every block has exactly one state-parent, so two
/// lineages from a common root meet exactly once.
///
/// The walk is deliberately unbounded. A depth cap would be a node-local limit
/// on a value every node must derive identically, so two nodes with different
/// caps could return different verdicts; the depth is only reported.
fn state_lineage_meet(
    dag: &KeyValueDagRepresentation,
    a: &BlockHash,
    b: &BlockHash,
) -> Result<StateLineage, CasperError> {
    let mut a = a.clone();
    let mut b = b.clone();
    let mut steps: usize = 0;
    loop {
        if a == b {
            return Ok(StateLineage::Meet(a));
        }
        let meta_a = held_meta(dag, &a)?;
        let meta_b = held_meta(dag, &b)?;
        if meta_a.block_number > meta_b.block_number {
            match state_parent_of(&meta_a)? {
                Some(parent) => a = parent,
                None => return Ok(StateLineage::Disconnected),
            }
        } else if meta_b.block_number > meta_a.block_number {
            match state_parent_of(&meta_b)? {
                Some(parent) => b = parent,
                None => return Ok(StateLineage::Disconnected),
            }
        } else {
            match (state_parent_of(&meta_a)?, state_parent_of(&meta_b)?) {
                (Some(pa), Some(pb)) => {
                    a = pa;
                    b = pb;
                }
                _ => return Ok(StateLineage::Disconnected),
            }
        }
        steps += 1;
        if steps == DEEP_WALK_WARN_THRESHOLD {
            tracing::warn!(
                target: "f1r3.trace.floor",
                "state-lineage meet walk unusually deep"
            );
        }
    }
}

/// The sigs a single block's construction step introduces into its state:
/// non-failed fresh executions plus chains its merge applied from scope.
/// Reads the block body; an absent body is refused, never guessed.
fn introduced_sigs<'m>(
    block_store: &KeyValueBlockStore,
    hash: &BlockHash,
    memo: &'m mut IntroducedSigsMemo,
) -> Result<&'m HashSet<Bytes>, CasperError> {
    if !memo.contains_key(hash) {
        let block = block_store.get(hash)?.ok_or_else(|| {
            CasperError::Other(format!(
                "state containment: lineage block {} is absent from the block \
                 store — refusing to judge membership from an incomplete lineage",
                PrettyPrinter::build_string_bytes(hash),
            ))
        })?;
        let mut sigs: HashSet<Bytes> = HashSet::new();
        for pd in &block.body.deploys {
            if !pd.is_failed {
                sigs.insert(pd.deploy.sig.clone());
            }
        }
        for sig in &block.body.applied_from_scope {
            sigs.insert(sig.clone());
        }
        memo.insert(hash.clone(), sigs);
    }
    Ok(memo.get(hash).expect("inserted above"))
}

/// The sigs introduced on `from`'s state lineage STRICTLY above `meet`.
fn segment_introduced_sigs(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    from: &BlockHash,
    meet: &BlockHash,
    memo: &mut IntroducedSigsMemo,
) -> Result<HashSet<Bytes>, CasperError> {
    let mut sigs: HashSet<Bytes> = HashSet::new();
    let mut cur = from.clone();
    while cur != *meet {
        sigs.extend(introduced_sigs(block_store, &cur, memo)?.iter().cloned());
        let meta = held_meta(dag, &cur)?;
        cur = state_parent_of(&meta)?.ok_or_else(|| {
            CasperError::Other(format!(
                "state containment: lineage of {} reached a root without \
                 passing its meet {} — state-parent pointers are inconsistent",
                PrettyPrinter::build_string_bytes(from),
                PrettyPrinter::build_string_bytes(meet),
            ))
        })?;
    }
    Ok(sigs)
}

/// True iff `cand`'s committed state contains every effect settled in
/// `x`'s state, decided at sig granularity from the recorded POSITIVE
/// construction facts alone: `state(B) = state(state-parent(B)) +
/// applied_from_scope(B) + non-failed deploys(B)`, all consensus-checked
/// block content. The two state lineages meet at a unique block (the
/// state-parent pointers form a tree); every sig at-or-below the meet is
/// shared by construction, so containment reduces to set inclusion of the
/// sigs introduced on the two segments above the meet. Rejection records
/// play no part: testimony about what a merge kept OUT can be suppressed
/// at emission (the 5fdb9bfe erasure — an eraser whose identical record
/// lived only on a parent edge carried clean metadata), while the
/// positive facts cannot be absent without the block being invalid.
pub(crate) fn state_contains(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    cand: &Floor,
    x: &Floor,
    memo: &mut IntroducedSigsMemo,
) -> Result<bool, CasperError> {
    if cand.hash == x.hash {
        trace_containment(cand, x, "same-block", 0);
        return Ok(true);
    }
    let StateLineage::Meet(meet) = state_lineage_meet(dag, &cand.hash, &x.hash)? else {
        trace_containment(cand, x, "disconnected-lineages", 0);
        return Ok(false);
    };
    if meet == x.hash {
        trace_containment(cand, x, "on-lineage", 0);
        return Ok(true);
    }
    let settled = segment_introduced_sigs(dag, block_store, &x.hash, &meet, memo)?;
    if settled.is_empty() {
        trace_containment(cand, x, "no-settled-content", 0);
        return Ok(true);
    }
    let carried = segment_introduced_sigs(dag, block_store, &cand.hash, &meet, memo)?;
    let missing = settled.difference(&carried).count();
    if missing == 0 {
        trace_containment(cand, x, "contained", 0);
        Ok(true)
    } else {
        trace_containment(cand, x, "missing-settled-sigs", missing);
        Ok(false)
    }
}

/// Every containment verdict is logged with its exit reason: the check
/// decides whether settled state survives a floor advance, and a live
/// erasure investigation must be able to read WHY an advance was allowed
/// or refused without re-deriving the walk.
fn trace_containment(cand: &Floor, x: &Floor, verdict: &str, missing: usize) {
    tracing::debug!(
        target: "f1r3.trace.floor",
        cand = %PrettyPrinter::build_string_bytes(&cand.hash),
        cand_number = cand.block_number,
        settled = %PrettyPrinter::build_string_bytes(&x.hash),
        settled_number = x.block_number,
        missing_sigs = missing,
        verdict,
        "state-containment verdict"
    );
}

/// The outcome of one LFB decision over the live view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorOfView {
    /// A strictly higher floor whose state contains the current LFB's
    /// settled effects — adopt it.
    Advance(Floor),
    /// Nothing to do: empty view, or no strictly higher floor derived.
    NoAdvance,
    /// A strictly higher floor was derived and REFUSED: its state is
    /// missing effects settled under the current LFB. One refusal is not a
    /// diagnosis — a rejected-then-recovered deploy legitimately fails
    /// containment until its re-homed carrier finalizes — but a streak of
    /// refusals with RISING derived floors is the shard finalizing without
    /// this node: a finality divergence. The finalization runner's
    /// `DivergenceMonitor` tracks exactly that.
    ContainmentHold { derived: Floor },
}

impl FloorOfView {
    /// The adopted floor, if any — for callers that only care whether the
    /// LFB advanced (the API read path, tests). The runner matches all
    /// three arms so containment holds reach the `DivergenceMonitor`.
    pub fn advanced(self) -> Option<Floor> {
        match self {
            FloorOfView::Advance(floor) => Some(floor),
            FloorOfView::NoAdvance | FloorOfView::ContainmentHold { .. } => None,
        }
    }
}

/// The LFB decision over the LIVE view — the one finality clock: derive the
/// floor of the current frontier (the deduped latest-message blocks, over
/// the live snapshot) and advance only onto a strictly higher floor whose
/// state CONTAINS the current LFB's settled effects — the same containment
/// check the per-block derivation runs, so the read surface can never
/// designate a state missing settled content. Both the finalization runner
/// and the API path consume exactly this.
pub async fn floor_of_view(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    current: &Floor,
    ftt: FtThreshold,
) -> Result<FloorOfView, CasperError> {
    let mut tips: Vec<BlockHash> = dag
        .latest_message_hashes()
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();
    tips.sort();
    tips.dedup();
    if tips.is_empty() {
        return Ok(FloorOfView::NoAdvance);
    }
    let live_snapshot: BTreeMap<Validator, BlockHash> =
        dag.latest_message_hashes().into_iter().collect();
    let derived = finalized_floor(dag, block_store, &tips, &live_snapshot, ftt).await?;
    if derived.hash == current.hash || derived.block_number <= current.block_number {
        return Ok(FloorOfView::NoAdvance);
    }
    let mut memo = IntroducedSigsMemo::new();
    if state_contains(dag, block_store, &derived, current, &mut memo)? {
        Ok(FloorOfView::Advance(derived))
    } else {
        tracing::warn!(
            target: "f1r3fly.finalizer",
            derived = %PrettyPrinter::build_string_bytes(&derived.hash),
            derived_number = derived.block_number,
            current = %PrettyPrinter::build_string_bytes(&current.hash),
            "floor-of-view does not capture the current LFB; holding"
        );
        Ok(FloorOfView::ContainmentHold { derived })
    }
}

/// The active committee derived from a finalized-floor block's post-state: the
/// PoS bonds at that state, filtered to the currently-active validator set.
///
/// Both the proposer (packaging `block.bonds`, `block_creator.rs`) and the
/// validator (recomputing the bonds cache, `validate.rs::bonds_cache_from_floor`)
/// call THIS ONE function on the same floor state hash, so their committees are
/// identical by construction — the PLAY≡REPLAY the `InvalidBondsCache` check
/// relies on (a block whose `bonds` set differs from the floor committee is
/// rejected). Extracting the two previously byte-identical inline copies into a
/// single helper removes the standing risk that a future edit to one site
/// silently diverges the two (the committee is `Selection.committee_is_floor_bonds`
/// — a pure function of the floor — realized in Rust).
pub async fn floor_committee(
    runtime_manager: &RuntimeManager,
    floor_state: &StateHash,
) -> Result<Vec<Bond>, CasperError> {
    let floor_bonds = runtime_manager.compute_bonds(floor_state).await?;
    let active = runtime_manager.get_active_validators(floor_state).await?;
    Ok(floor_bonds
        .into_iter()
        .filter(|bond| active.contains(&bond.validator))
        .collect())
}

/// Walk depth past which a floor walk is reported as unusually deep (cold
/// start after restart, or a finality stall). Visibility only — the walk
/// always terminates: main-parent chains end at genesis, and genesis is
/// finalized by definition.
const DEEP_WALK_WARN_THRESHOLD: usize = 256;

/// Compute `floor(B)` for a block whose parents and justification snapshot are
/// given. `latest_messages` must be the block's own justifications (validate)
/// or the justification set about to be packaged into the block (propose) —
/// never the live DAG view.
///
/// The floor is computed from two candidate sources and is MONOTONE along
/// ancestry:
///
/// 1. **Inheritance** — every parent's own floor. A child can never carry a
///    lower cut than any parent, so a race sealed at some cut can never be
///    re-litigated by a descendant whose justifications happen to lag behind
///    that cut's finalization. This is RChain's fringe advancement
///    (`calculateFinalization` starts from `latestFringe(parents)` and only
///    moves up); deriving the floor fresh from the oracle per block — without
///    inheritance — allowed exactly that re-litigation.
/// 2. **Advancement** — per parent, the highest main-chain ancestor with
///    `ft_witnessed >= ft_threshold` over the justification snapshot; a block
///    with no main parent is genesis, finalized by definition.
///
/// The floor is the maximum candidate. Both sources are pure functions of the
/// block (parents' floors are themselves block-structural facts), so the
/// result stays node-identical. Linear finality requires every candidate to
/// lie on the floor's own main chain — a violation is a consensus-safety break
/// and is surfaced as an error, never papered over.
pub async fn finalized_floor(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let (floor, _settled) =
        finalized_floor_with_candidates(dag, block_store, parents, latest_messages, ftt).await?;
    Ok(floor)
}

/// `finalized_floor`, also returning the SETTLED candidate set: the chosen
/// floor plus every inherited parent floor (deduped). These are the
/// positions state monotonicity protects — the merge-time settled-rejection
/// tripwire checks rejected chains against exactly this set.
pub async fn finalized_floor_with_candidates(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<(Floor, Vec<Floor>), CasperError> {
    let mut inherited: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        inherited.push(floor_of_block(dag, block_store, parent, ftt).await?);
    }
    let (floor, _main_parent_frontier) = derive_floor(
        dag,
        block_store,
        parents,
        latest_messages,
        ftt,
        inherited.clone(),
    )
    .await?;
    let mut settled: Vec<Floor> = Vec::with_capacity(inherited.len() + 1);
    for f in inherited.into_iter().chain(std::iter::once(floor.clone())) {
        if !settled.iter().any(|s| s.hash == f.hash) {
            settled.push(f);
        }
    }
    Ok((floor, settled))
}

/// Core derivation: max over (inherited parent floors ∪ oracle frontiers),
/// with the one-chain safety check. `inherited` must hold the parents' own
/// floors; the caller resolves them so this stays non-recursive.
///
/// Returns `(floor, F(B))` where `F(B)` is the main parent's frontier over this
/// block's snapshot — i.e. `parent_frontier(parents[0], latest_messages)`. Since
/// a block is never witnessed-finalized over its own justifications, this equals
/// the block's OWN frontier `parent_frontier(B, just(B))`, a pure function of the
/// block. `floor_of_block` persists it so later merges resolve their frontiers
/// by an O(advance) up-walk from the cached pivot instead of an O(Δ) down-walk.
async fn derive_floor(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
    inherited: Vec<Floor>,
) -> Result<(Floor, Floor), CasperError> {
    if parents.is_empty() {
        return Err(CasperError::Other(
            "finalized_floor requires a non-empty parent set; genesis pre-state comes from config"
                .to_string(),
        ));
    }

    let inherited_floors = inherited.clone();
    let mut candidates = inherited;
    let inherited_max = candidates.iter().map(|f| f.block_number).max();
    let mut frontiers: Vec<Floor> = Vec::with_capacity(parents.len());
    for parent in parents {
        frontiers.push(parent_frontier(dag, parent, latest_messages, ftt).await?);
    }
    // parents[0] is the main parent; its frontier over this snapshot is F(B).
    let main_parent_frontier = frontiers[0].clone();
    candidates.extend(frontiers);

    // The floor is the position settled truth advances to — and the base of
    // last resort when the block's main parent does not hold that truth — so
    // a candidate is sound only when choosing it cannot regress settled
    // state. What monotonicity protects is the INHERITED floors: positions
    // some parent's chain actually held. Frontier candidates are merely
    // witnessed (orphan-safe) blocks — a witnessed carrier whose chain lost
    // a merge is adjudicated by the record and re-landed by recovery, not
    // owed containment — so soundness quantifies over the inherited floors
    // only. Candidates are considered from the top down; `cand` is sound when
    // every inherited floor `x` satisfies one of:
    //
    //   A. `cand`'s state CONTAINS `x`'s settled effects (`state_contains`):
    //      decided exactly, at sig granularity, from the recorded positive
    //      construction facts. Rejection records are deliberately not
    //      consulted — a merge's own record can be suppressed at emission
    //      when an identical record is visible on a parent edge, leaving an
    //      eraser with clean lineage testimony (the 5fdb9bfe erasure); the
    //      positive facts cannot be absent without the block being invalid.
    //
    //   B. `x` re-enters THIS merge as diffs: `x` is NOT in `cand`'s DAG
    //      past — sound only for a PURE CUT (`cand` introduces no sigs of
    //      its own above its meet with `x`; a competing branch's own
    //      content must never become the settled position). Covers the
    //      co-finalized-sibling descend (test_trim_state / run
    //      28135973777); the merge-time settled-rejection tripwire guards
    //      this arm: a re-collected settled chain must land, never be
    //      keep-one'd out.
    //
    //      CAVEAT: the re-collection this arm relies on was derived when
    //      the merge based on the FLOOR, where "not in `cand`'s DAG past"
    //      did imply "retained by the scope filter". The merge now bases on
    //      its main parent and the filter is relative to THAT base, so the
    //      implication no longer follows — an `x` that is a DAG ancestor of
    //      `parents[0]` whose chains that parent's merge rejected is in
    //      neither the base nor the scope. Whether the arm admits such a
    //      candidate is open; `base_holds_floor` checks containment against
    //      the floor, not against each inherited `x`, so it would not catch
    //      it. Not observed; not disproven.
    //
    // The highest candidate satisfying neither is skipped; if NO candidate is
    // sound (no finalized cut common to all parents), that is a genuinely
    // incompatible finalized fork and is surfaced as an error, never papered
    // over.
    let mut ordered: Vec<&Floor> = candidates.iter().collect();
    ordered.sort_by(|a, b| {
        b.block_number
            .cmp(&a.block_number)
            .then_with(|| b.hash.cmp(&a.hash))
    });

    let mut chosen: Option<Floor> = None;
    let mut memo = IntroducedSigsMemo::new();
    'cands: for cand in ordered {
        for other in &inherited_floors {
            if other.hash == cand.hash {
                continue;
            }
            let sound_with_other = if state_contains(dag, block_store, cand, other, &mut memo)? {
                true
            } else if !dag.is_dag_ancestor(&other.hash, &cand.hash)? {
                // `other` is NOT in `cand`'s DAG past, so its chains are
                // expected back as this merge's diffs, with the merge-time
                // settled-rejection tripwire guarding the re-application.
                // (That expectation is weaker than it reads since the base
                // became the main parent — see the CAVEAT above.) Sound ONLY when
                // `cand` is a pure cut — it introduces no sigs of its own
                // relative to its meet with `other`. A competing branch
                // with content of its own must never become the settled
                // position: its content can be exactly what the canonical
                // chain rejected, and every future canonical floor would
                // then be refused against it (the floor deadlocks instead
                // of advancing — observed when the reproduction's eraser R
                // slipped in as a floor through the join block's parents).
                let mut re_merged = false;
                for parent in parents {
                    if dag.is_dag_ancestor(&other.hash, parent)?
                        && dag.is_dag_ancestor(&cand.hash, parent)?
                    {
                        re_merged = true;
                        break;
                    }
                }
                if re_merged {
                    match state_lineage_meet(dag, &cand.hash, &other.hash)? {
                        StateLineage::Meet(meet) => {
                            segment_introduced_sigs(dag, block_store, &cand.hash, &meet, &mut memo)?
                                .is_empty()
                        }
                        StateLineage::Disconnected => false,
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if !sound_with_other {
                tracing::debug!(
                    target: "f1r3.trace.floor",
                    candidate = %PrettyPrinter::build_string_bytes(&cand.hash),
                    candidate_number = cand.block_number,
                    inherited = %PrettyPrinter::build_string_bytes(&other.hash),
                    inherited_number = other.block_number,
                    "floor candidate skipped: neither captures nor re-merges an inherited floor"
                );
                continue 'cands;
            }
        }
        chosen = Some(cand.clone());
        break;
    }

    let floor = chosen.ok_or_else(|| {
        CasperError::Other(format!(
            "finalized-floor safety violation: no finalized candidate is a sound merge base over \
             parents [{}] (candidates [{}]) — incompatible finalized fork",
            parents
                .iter()
                .map(|p| PrettyPrinter::build_string_bytes(p))
                .collect::<Vec<_>>()
                .join(", "),
            candidates
                .iter()
                .map(|c| format!(
                    "{}#{}",
                    PrettyPrinter::build_string_bytes(&c.hash),
                    c.block_number
                ))
                .collect::<Vec<_>>()
                .join(", "),
        ))
    })?;

    tracing::debug!(
        target: "f1r3.trace.floor_walk",
        candidates = ?candidates.iter().map(|c| format!("{}#{}", PrettyPrinter::build_string_bytes(&c.hash), c.block_number)).collect::<Vec<_>>(),
        chosen = %PrettyPrinter::build_string_bytes(&floor.hash),
        chosen_number = floor.block_number,
        "derive_floor candidates + chosen"
    );

    tracing::debug!(
        target: "f1r3.trace.floor",
        floor = %PrettyPrinter::build_string_bytes(&floor.hash),
        floor_number = floor.block_number,
        inherited_max = inherited_max.unwrap_or(-1),
        parent_count = parents.len(),
        "finalized floor derived (inheritance + advancement)"
    );

    Ok((floor, main_parent_frontier))
}

/// `floor(B)` for an already-inserted block, resolved through the persisted
/// floor cache. On a miss the floor is derived from the block's own metadata
/// (its parents and signed justifications) and cached — the floor is a pure
/// function of the block, so the cache can never go stale.
///
/// Resolution is iterative: ancestors whose floors are not yet cached are
/// pushed onto an explicit stack and computed bottom-up, so inheritance never
/// recurses. In steady state every parent is already cached (each block's
/// floor is computed when it is first merged on), making this a single cache
/// read.
///
/// A block with no parents is genesis: its own floor by definition, the
/// terminal cut of the floor-of-floor recursion.
pub async fn floor_of_block(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    block_hash: &BlockHash,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut stack: Vec<BlockHash> = vec![block_hash.clone()];
    while let Some(current) = stack.last().cloned() {
        if dag.get_cached_floor(&current)?.is_some() {
            stack.pop();
            continue;
        }

        let metadata = held_meta(dag, &current)?;
        if metadata.parents.is_empty() {
            dag.put_cached_floor(current.clone(), current.clone())?;
            stack.pop();
            continue;
        }

        let mut missing: Vec<BlockHash> = Vec::new();
        for parent in &metadata.parents {
            if dag.get_cached_floor(parent)?.is_none() {
                missing.push(parent.clone());
            }
        }
        if !missing.is_empty() {
            stack.extend(missing);
            continue;
        }

        let mut inherited: Vec<Floor> = Vec::with_capacity(metadata.parents.len());
        for parent in &metadata.parents {
            let hash = dag.get_cached_floor(parent)?.expect(
                "parent floor must be cached: the missing set was empty for this stack entry",
            );
            inherited.push(Floor {
                block_number: held_number(dag, &hash)?,
                hash,
            });
        }
        let latest_messages: BTreeMap<Validator, BlockHash> = metadata
            .justifications
            .iter()
            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
            .collect();
        let (floor, frontier) = derive_floor(
            dag,
            block_store,
            &metadata.parents,
            &latest_messages,
            ftt,
            inherited,
        )
        .await?;

        dag.put_cached_floor(current.clone(), floor.hash.clone())?;
        // Persist F(current) = the block's own frontier over its own snapshot,
        // a pure function of the block. Later merges read this as the up-walk
        // pivot in `parent_frontier`, collapsing the O(Δ²·V) walk ratchet.
        dag.put_cached_frontier(current.clone(), frontier.hash.clone())?;
        tracing::trace!(
            target: "f1r3.trace.floor",
            block = %PrettyPrinter::build_string_bytes(&current),
            floor = %PrettyPrinter::build_string_bytes(&floor.hash),
            floor_number = floor.block_number,
            "floor of inserted block computed and cached"
        );
        stack.pop();
    }

    let hash = dag
        .get_cached_floor(block_hash)?
        .expect("floor must be cached: the resolution stack drained for this block");
    Ok(Floor {
        block_number: held_number(dag, &hash)?,
        hash,
    })
}

/// The highest witnessed-finalized block on one parent's main chain, over the
/// given justification snapshot.
///
/// Two paths, both yielding the identical frontier — the cache is a transparent
/// optimization, proven so by L-ANC + L-SNAP (see
/// `docs/casper/theory/finalized-floor/finalized-floor-verification.md`):
///
/// * **Warm** ([`incremental_frontier`]) — when `parent`'s own frontier
///   `F(parent)` is cached (persisted by [`floor_of_block`] on insertion).
///   `F(parent)` is the frontier over `parent`'s OWN snapshot; the snapshot here
///   (`latest_messages` = the child's justifications) is a superset, so by L-SNAP
///   the true frontier sits at height ≥ `F(parent)`. We take it as a pivot and
///   walk UP the spine toward `parent`, advancing while each block stays
///   finalized. By L-ANC finalization is downward-closed on the spine, so the
///   walk stops at the first non-finalized block after only O(advance) oracle
///   calls — amortized O(1). The band itself is collected with cheap
///   `main_parent` hops (no oracle calls).
///
/// * **Cold** ([`cold_parent_frontier`]) — no cached pivot, the pivot is off
///   `parent`'s spine, the committee changed across the band (L-ANC's premise
///   fails), or the pivot no longer finalizes over the larger snapshot (L-SNAP's
///   premise fails): the original top-down walk from `parent`, one oracle call
///   per step down to the first finalized block (or genesis).
pub(crate) async fn parent_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    if let Some(pivot_hash) = dag.get_cached_frontier(parent)? {
        if let Some(frontier) =
            incremental_frontier(dag, parent, &pivot_hash, latest_messages, ftt).await?
        {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_HIT_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            return Ok(frontier);
        }
    }
    metrics::counter!(
        crate::rust::metrics_constants::FLOOR_FRONTIER_CACHE_MISS_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(1);
    cold_parent_frontier(dag, parent, latest_messages, ftt).await
}

/// Warm frontier: resolve `parent`'s frontier over the (larger) `latest_messages`
/// snapshot by an incremental UP-walk from the cached pivot `F(parent)`. Returns
/// `Ok(None)` when a determinism guard trips, signalling the caller to fall back
/// to the cold walk (which yields the identical result); the cache thus never
/// changes the derived frontier, only the work done to find it.
async fn incremental_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    pivot_hash: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Option<Floor>, CasperError> {
    let pivot_number = held_number(dag, pivot_hash)?;

    // Collect the spine band [parent .. pivot] with cheap `main_parent` hops
    // (NO oracle calls). `spine[0]` = parent (top); the tail descends the main
    // spine down to the block reached at the pivot's height.
    let mut spine: Vec<BlockHash> = Vec::new();
    spine.push(parent.clone());
    spine.extend(dag.main_parent_chain(parent.clone(), pivot_number)?);
    // The pivot must be exactly the bottom of the band; otherwise it is not on
    // `parent`'s main spine (a fork at equal height) — fall back to cold.
    match spine.last() {
        Some(last) if last == pivot_hash => {}
        _ => return Ok(None),
    }

    // L-ANC guard: the committee (corresponding weight map, exactly what
    // `ft_witnessed` uses) must be constant across the band, else finalization
    // need not be downward-closed and the up-walk could disagree with the cold
    // walk. This is O(band) cheap metadata reads — bounded by the floor-distance
    // backstop — and never an oracle call.
    let pivot_committee = CliqueOracle::get_corresponding_weight_map(pivot_hash, dag).await?;
    for block in &spine {
        let committee = CliqueOracle::get_corresponding_weight_map(block, dag).await?;
        if committee != pivot_committee {
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_INCREMENTAL_GUARD_FALLBACK_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            return Ok(None);
        }
    }

    // L-SNAP guard: the pivot must still be witnessed-finalized over the larger
    // snapshot. It was finalized over `parent`'s own snapshot, and a superset can
    // only raise the fault tolerance — but a bonding event in the band can break
    // that monotonicity, so we verify rather than assume.
    let mut oracle_calls: u64 = 1;
    // A9 exact ≥-semantics (floor path): the pivot must still be witnessed-
    // finalized over the larger snapshot. `strict=false` ⇒ (2q−S)/S ≥ θ.
    let pivot_finalized =
        CliqueOracle::ft_witnessed_exact(pivot_hash, dag, latest_messages, ftt, false).await?;
    if !pivot_finalized {
        metrics::counter!(
            crate::rust::metrics_constants::FLOOR_INCREMENTAL_GUARD_FALLBACK_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .increment(1);
        return Ok(None);
    }

    // Up-walk: from just above the pivot toward `parent`, advancing while each
    // block stays finalized. By L-ANC (constant committee, verified above) the
    // finalized blocks form a downward-closed prefix, so the first non-finalized
    // block ends it and the highest finalized block is the frontier.
    let mut best_hash = pivot_hash.clone();
    let mut best_number = pivot_number;
    let mut advance: u64 = 0;
    for candidate in spine[..spine.len() - 1].iter().rev() {
        // A9 exact ≥-semantics (floor path): advance while each block stays
        // witnessed-finalized over the snapshot.
        let finalized =
            CliqueOracle::ft_witnessed_exact(candidate, dag, latest_messages, ftt, false).await?;
        oracle_calls += 1;
        if finalized {
            best_hash = candidate.clone();
            best_number = held_number(dag, candidate)?;
            advance += 1;
        } else {
            break;
        }
    }

    metrics::counter!(
        crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(oracle_calls);
    metrics::histogram!(
        crate::rust::metrics_constants::FLOOR_FRONTIER_ADVANCE_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .record(advance as f64);
    trace_frontier(
        parent,
        &best_hash,
        best_number,
        advance as usize,
        "warm-up-walk",
    );
    Ok(Some(Floor {
        hash: best_hash,
        block_number: best_number,
    }))
}

/// Cold frontier: the top-down walk from `parent`, one clique-oracle call per
/// step, returning the first witnessed-finalized block (or genesis). Used on a
/// cache miss or when a warm-path determinism guard trips; also the genesis
/// terminator. Always terminates — main-parent chains end at genesis.
async fn cold_parent_frontier(
    dag: &KeyValueDagRepresentation,
    parent: &BlockHash,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    ftt: FtThreshold,
) -> Result<Floor, CasperError> {
    let mut current = parent.clone();
    let mut walked: usize = 0;
    let mut oracle_calls: u64 = 0;
    loop {
        // A9 exact ≥-semantics (floor path): first witnessed-finalized block down
        // the main-parent chain is the frontier.
        let finalized =
            CliqueOracle::ft_witnessed_exact(&current, dag, latest_messages, ftt, false).await?;
        oracle_calls += 1;
        tracing::debug!(
            target: "f1r3.trace.floor_walk",
            parent = %PrettyPrinter::build_string_bytes(parent),
            current = %PrettyPrinter::build_string_bytes(&current),
            current_number = held_number(dag, &current)?,
            finalized,
            walked,
            "floor walk step"
        );
        if finalized {
            let block_number = held_number(dag, &current)?;
            metrics::counter!(
                crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(oracle_calls);
            trace_frontier(
                parent,
                &current,
                block_number,
                walked,
                "witnessed-finalized",
            );
            return Ok(Floor {
                hash: current,
                block_number,
            });
        }
        match dag.main_parent(&current) {
            Some(main_parent) => {
                current = main_parent;
                walked += 1;
                if walked == DEEP_WALK_WARN_THRESHOLD {
                    tracing::warn!(
                        target: "f1r3.trace.floor",
                        parent = %PrettyPrinter::build_string_bytes(parent),
                        walked,
                        "floor walk unusually deep; finality is lagging or this is a cold start"
                    );
                }
            }
            None => {
                // No main parent: `current` is genesis, finalized by definition.
                let block_number = held_number(dag, &current)?;
                metrics::counter!(
                    crate::rust::metrics_constants::FLOOR_WALK_ORACLE_CALLS_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(oracle_calls);
                trace_frontier(parent, &current, block_number, walked, "genesis");
                return Ok(Floor {
                    hash: current,
                    block_number,
                });
            }
        }
    }
}

fn trace_frontier(
    parent: &BlockHash,
    frontier: &BlockHash,
    frontier_number: i64,
    walked: usize,
    kind: &str,
) {
    tracing::trace!(
        target: "f1r3.trace.floor",
        parent = %PrettyPrinter::build_string_bytes(parent),
        frontier = %PrettyPrinter::build_string_bytes(frontier),
        frontier_number,
        walked,
        kind,
        "per-parent finalized frontier"
    );
}

#[cfg(test)]
mod frontier_determinism_tests {
    //! The determinism linchpin's "tested" leg: on a real finalizing DAG the WARM
    //! up-walk (`incremental_frontier` from a cached pivot) returns the identical
    //! frontier as the COLD down-walk (`cold_parent_frontier`), and the floor a
    //! block derives is invariant to whether the caches are cold or warm
    //! (transparency ⇒ no fork). Complements the axiom-free Rocq proof
    //! (Floor.frontier_cache_transparent) and the 400+-block soak.
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn h(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }
    fn val() -> Bytes { Bytes::from(vec![9; 65]) }

    fn md(hash: Bytes, parents: Vec<Bytes>, num: i64, v: &Bytes) -> BlockMetadata {
        let mut wm = BTreeMap::new();
        wm.insert(v.clone(), 1i64);
        BlockMetadata {
            block_hash: hash,
            parents,
            sender: v.clone(),
            justifications: vec![],
            weight_map: wm,
            block_number: num,
            sequence_number: num as i32,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            merge_base: Bytes::new(),
        }
    }

    /// A single-validator linear chain genesis <- b1 <- b2 <- b3, committee {v:1}.
    /// Over the snapshot J = {v -> b2}, the clique oracle finalizes genesis, b1,
    /// and b2 (v's latest message b2 DAG-descends from each), but not b3. So the
    /// frontier of b3 over J is b2.
    fn mk_dag() -> (
        KeyValueDagRepresentation,
        Bytes,
        (Bytes, Bytes, Bytes, Bytes),
    ) {
        let v = val();
        let (g, b1, b2, b3) = (h(0), h(1), h(2), h(3));

        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        bms.add(md(g.clone(), vec![], 0, &v)).unwrap();
        bms.add(md(b1.clone(), vec![g.clone()], 1, &v)).unwrap();
        bms.add(md(b2.clone(), vec![b1.clone()], 2, &v)).unwrap();
        bms.add(md(b3.clone(), vec![b2.clone()], 3, &v)).unwrap();

        let mut dag_set = imbl::HashSet::new();
        for x in [&g, &b1, &b2, &b3] {
            dag_set.insert(x.clone());
        }
        let mut bnum = imbl::HashMap::new();
        bnum.insert(g.clone(), 0);
        bnum.insert(b1.clone(), 1);
        bnum.insert(b2.clone(), 2);
        bnum.insert(b3.clone(), 3);
        let mut mp = imbl::HashMap::new();
        mp.insert(b1.clone(), g.clone());
        mp.insert(b2.clone(), b1.clone());
        mp.insert(b3.clone(), b2.clone());

        let dag = KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        };
        (dag, v, (g, b1, b2, b3))
    }

    /// The chain a restored node holds: blocks 84..88, with 84's own parent 83
    /// below the sync window and absent. One validator, so the committee is
    /// constant and the oracle's verdicts are decidable from what is held.
    fn mk_truncated_dag() -> (KeyValueDagRepresentation, Bytes, Vec<Bytes>) {
        let v = val();
        let absent = h(83);
        let held: Vec<Bytes> = (84u8..=88).map(h).collect();

        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let mut dag_set = imbl::HashSet::new();
        let mut bnum = imbl::HashMap::new();
        let mut mp = imbl::HashMap::new();

        for (i, hash) in held.iter().enumerate() {
            let number = 84 + i as i64;
            let parent = if i == 0 {
                absent.clone()
            } else {
                held[i - 1].clone()
            };
            let mut meta = md(hash.clone(), vec![parent.clone()], number, &v);
            // The child names the anchor as its justification, which is what the
            // floor derivation freezes as its snapshot.
            meta.justifications = vec![
                models::rust::casper::protocol::casper_message::Justification {
                    validator: v.clone(),
                    latest_block_hash: held[i.saturating_sub(1)].clone(),
                },
            ];
            bms.add(meta).unwrap();
            dag_set.insert(hash.clone());
            bnum.insert(hash.clone(), number);
            // Recorded for EVERY held block, including the lowest, whose parent
            // is not held — which is why a walk can step off the end.
            mp.insert(hash.clone(), parent);
        }

        let dag = KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        };
        (dag, absent, held)
    }

    /// Phase 2's whole bet, in miniature.
    ///
    /// A restored node cannot derive its anchor's floor: the recursion runs
    /// through the anchor's parents, and it holds none of them. Seeding the
    /// anchor's floor and frontier is supposed to be enough on its own, because
    /// floors derive FORWARD — the block above the anchor inherits from the
    /// seeded entry and caches its own, and the recursion terminates from then
    /// on without ever reaching for history below the window.
    ///
    /// Without the seed the same derivation walks off the end of what is held,
    /// which is the second half of the test: the seed is doing the work, not the
    /// fixture.
    #[tokio::test]
    async fn a_seeded_anchor_derives_forward_without_reaching_below_the_window() {
        let thr = FtThreshold::from_f32_lossy(0.1);
        let (dag, absent, held) = mk_truncated_dag();
        let (anchor, child) = (held[3].clone(), held[4].clone());
        let seed = held[1].clone();

        let unseeded = floor_of_block(&dag, &mk_store(), &child, thr)
            .await
            .expect_err("without a seed the derivation must run out of history");
        assert!(
            matches!(unseeded, CasperError::BlockNotHeld(ref h) if *h == absent),
            "the unseeded derivation must fail by naming the block below the window, \
             or this fixture is not truncated and proves nothing; got {unseeded}"
        );

        dag.put_cached_floor(anchor.clone(), seed.clone()).unwrap();
        dag.put_cached_frontier(anchor.clone(), seed.clone())
            .unwrap();

        let floor = floor_of_block(&dag, &mk_store(), &child, thr)
            .await
            .expect("a seeded anchor must let the block above it derive a floor");
        assert!(
            floor.block_number >= seed_number(&dag, &seed),
            "the derived floor can never sit below the seed it inherited"
        );
        assert_eq!(
            dag.get_cached_floor(&child).unwrap(),
            Some(floor.hash.clone()),
            "the child's own floor must be cached, so the block above IT inherits \
             from the child rather than reaching for the anchor again"
        );
    }

    fn seed_number(dag: &KeyValueDagRepresentation, hash: &Bytes) -> i64 {
        dag.lookup(hash).unwrap().expect("held").block_number
    }

    #[tokio::test]
    async fn warm_up_walk_equals_cold_down_walk() {
        let (dag, v, (_g, b1, b2, b3)) = mk_dag();
        let mut j = BTreeMap::new();
        j.insert(v, b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // Cold: top-down from b3 → first finalized is b2.
        let cold = cold_parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        assert_eq!(cold.hash, b2, "cold frontier of b3 over J must be b2");

        // Warm: from a pivot BELOW the true frontier (b1) → the up-walk must
        // advance to b2 and stop (b3 not finalized), matching the cold result.
        let warm = incremental_frontier(&dag, &b3, &b1, &j, thr).await.unwrap();
        assert!(
            warm.is_some(),
            "warm path must apply (committee constant across the band, pivot finalized)"
        );
        assert_eq!(
            warm.unwrap().hash,
            cold.hash,
            "warm up-walk must equal the cold down-walk (frontier cache is transparent)"
        );
    }

    #[tokio::test]
    async fn finalized_floor_is_cache_transparent() {
        let (dag, v, (_g, _b1, b2, b3)) = mk_dag();
        let mut j = BTreeMap::new();
        j.insert(v, b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // First call populates the floor/frontier caches (cold internally);
        // the second reads them (warm). The derived floor must be identical, and
        // it must be the sound Case-A base b2 (the highest finalized ancestor of
        // the single parent b3).
        let floor_cold = finalized_floor(&dag, &mk_store(), &[b3.clone()], &j, thr)
            .await
            .unwrap();
        let floor_warm = finalized_floor(&dag, &mk_store(), &[b3.clone()], &j, thr)
            .await
            .unwrap();
        assert_eq!(
            floor_cold.hash, b2,
            "derive_floor must select the sound base b2"
        );
        assert_eq!(
            floor_cold, floor_warm,
            "enabling the caches must not change the derived floor (no fork)"
        );
    }

    // ---- Phase-7 W7.2: guard-trip, Case-B soundness, incompatible-fork Err ----

    fn md_wm(
        hash: Bytes,
        parents: Vec<Bytes>,
        num: i64,
        sender: &Bytes,
        wm: Vec<(Bytes, i64)>,
    ) -> BlockMetadata {
        let mut weight_map = BTreeMap::new();
        for (validator, weight) in wm {
            weight_map.insert(validator, weight);
        }
        BlockMetadata {
            block_hash: hash,
            parents,
            sender: sender.clone(),
            justifications: vec![],
            weight_map,
            block_number: num,
            sequence_number: num as i32,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            merge_base: Bytes::new(),
        }
    }

    /// Assemble a DAG from an explicit block list, deriving `dag_set`,
    /// `block_number_map`, and `main_parent_map` (parents[0]) from the metadata.
    fn build_dag(blocks: Vec<BlockMetadata>) -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let mut dag_set = imbl::HashSet::new();
        let mut bnum = imbl::HashMap::new();
        let mut mp = imbl::HashMap::new();
        for b in &blocks {
            dag_set.insert(b.block_hash.clone());
            bnum.insert(b.block_hash.clone(), b.block_number);
            if let Some(main) = b.parents.first() {
                mp.insert(b.block_hash.clone(), main.clone());
            }
        }
        for b in blocks {
            bms.add(b).unwrap();
        }
        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        }
    }

    fn based(mut m: BlockMetadata, base: &Bytes) -> BlockMetadata {
        m.merge_base = base.clone();
        m
    }

    /// An empty in-memory block store for stagings whose settled segments
    /// are empty (the containment check short-circuits before any body
    /// read, so no blocks are required).
    fn mk_store() -> KeyValueBlockStore {
        use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
        KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
    }

    /// A stored block body matching a metadata fixture: `applied` are the
    /// sigs this block's merge applied from scope (raw bytes — not
    /// signature-verified on decode, unlike `deploys` entries), `base` the
    /// recorded merge base.
    fn body_block(
        hash: &Bytes,
        parents: Vec<Bytes>,
        num: i64,
        applied: Vec<Bytes>,
        base: Option<Bytes>,
        deploys: Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>,
    ) -> models::rust::casper::protocol::casper_message::BlockMessage {
        use models::rust::casper::protocol::casper_message::{
            BlockMessage, Body, F1r3flyState, Header,
        };
        BlockMessage {
            block_hash: hash.clone(),
            header: Header {
                parents_hash_list: parents,
                timestamp: 0,
                version: 0,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: Vec::new(),
                    block_number: num,
                },
                deploys,
                rejected_deploys: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Bytes::new(),
                applied_from_scope: applied,
                merge_base: base.unwrap_or_default(),
            },
            justifications: Vec::new(),
            sender: Bytes::new(),
            seq_num: num as i32,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    fn store_with(
        blocks: Vec<models::rust::casper::protocol::casper_message::BlockMessage>,
    ) -> KeyValueBlockStore {
        let store = mk_store();
        for block in &blocks {
            store.put_block_message(block).expect("store fixture block");
        }
        store
    }

    /// Containment is decided from the positive construction facts: a block
    /// on the candidate's own state lineage is contained by construction; a
    /// merge that APPLIED a sibling's chain from scope contains it via the
    /// recorded applied set; a block that introduced nothing is owed
    /// nothing.
    #[test]
    fn state_contains_decides_membership_from_construction_facts() {
        let v = val();
        let (e, c, d, m) = (h(0), h(1), h(2), h(3));
        let sig = Bytes::from_static(b"settled_sig_facts");
        let dag = build_dag(vec![
            md(e.clone(), vec![], 0, &v),
            md(c.clone(), vec![e.clone()], 1, &v),
            md(d.clone(), vec![e.clone()], 1, &v),
            based(md(m.clone(), vec![c.clone(), d.clone()], 2, &v), &e),
        ]);
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&c, vec![e.clone()], 1, vec![sig.clone()], None, vec![]),
            body_block(&d, vec![e.clone()], 1, vec![], None, vec![]),
            body_block(
                &m,
                vec![c.clone(), d.clone()],
                2,
                vec![sig.clone()],
                Some(e.clone()),
                vec![],
            ),
        ]);
        let at = |hash: &Bytes, n: i64| Floor {
            hash: hash.clone(),
            block_number: n,
        };
        let mut memo = IntroducedSigsMemo::new();
        // e is on every lineage: contained by construction.
        assert!(state_contains(&dag, &store, &at(&m, 2), &at(&e, 0), &mut memo).unwrap());
        assert!(state_contains(&dag, &store, &at(&c, 1), &at(&e, 0), &mut memo).unwrap());
        // m applied c's chain from scope: contained via the applied set.
        assert!(state_contains(&dag, &store, &at(&m, 2), &at(&c, 1), &mut memo).unwrap());
        // d introduced nothing: every state trivially contains it.
        assert!(state_contains(&dag, &store, &at(&m, 2), &at(&d, 1), &mut memo).unwrap());
        assert!(state_contains(&dag, &store, &at(&c, 1), &at(&d, 1), &mut memo).unwrap());
        // e's state does NOT contain m's settled sig (introduced above it).
        assert!(!state_contains(&dag, &store, &at(&e, 0), &at(&m, 2), &mut memo).unwrap());
    }

    /// A lineage merge that dropped a settled chain — with or WITHOUT a
    /// record of its own — fails containment on the positive facts alone,
    /// and the failure persists up the lineage: blocks built on the
    /// dropping merge inherit the missing sig.
    #[test]
    fn an_unrecorded_drop_on_the_lineage_defeats_containment() {
        let v = val();
        let (e, c, d, m, t) = (h(0), h(1), h(2), h(3), h(4));
        let sig = Bytes::from_static(b"settled_sig_drop");
        let dag = build_dag(vec![
            md(e.clone(), vec![], 0, &v),
            md(c.clone(), vec![e.clone()], 1, &v),
            md(d.clone(), vec![e.clone()], 1, &v),
            based(md(m.clone(), vec![c.clone(), d.clone()], 2, &v), &e),
            md(t.clone(), vec![m.clone()], 3, &v),
        ]);
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&c, vec![e.clone()], 1, vec![sig.clone()], None, vec![]),
            body_block(&d, vec![e.clone()], 1, vec![], None, vec![]),
            // m's merge dropped c's chain: applied set empty, no record
            // needed for the refusal.
            body_block(
                &m,
                vec![c.clone(), d.clone()],
                2,
                vec![],
                Some(e.clone()),
                vec![],
            ),
            body_block(&t, vec![m.clone()], 3, vec![], None, vec![]),
        ]);
        let at = |hash: &Bytes, n: i64| Floor {
            hash: hash.clone(),
            block_number: n,
        };
        let mut memo = IntroducedSigsMemo::new();
        assert!(!state_contains(&dag, &store, &at(&m, 2), &at(&c, 1), &mut memo).unwrap());
        assert!(!state_contains(&dag, &store, &at(&t, 3), &at(&c, 1), &mut memo).unwrap());
        assert!(state_contains(&dag, &store, &at(&m, 2), &at(&d, 1), &mut memo).unwrap());
        assert!(state_contains(&dag, &store, &at(&t, 3), &at(&e, 0), &mut memo).unwrap());
    }

    /// A multi-parent block with no recorded base has an underivable state
    /// lineage — the meet walk refuses rather than guesses.
    #[test]
    fn state_containment_refuses_multi_parent_without_base() {
        let v = val();
        let (e, c, d, m) = (h(0), h(1), h(2), h(3));
        let dag = build_dag(vec![
            md(e.clone(), vec![], 0, &v),
            md(c.clone(), vec![e.clone()], 1, &v),
            md(d.clone(), vec![e.clone()], 1, &v),
            md(m.clone(), vec![c.clone(), d.clone()], 2, &v),
        ]);
        let err = state_contains(
            &dag,
            &mk_store(),
            &Floor {
                hash: m.clone(),
                block_number: 2,
            },
            &Floor {
                hash: e.clone(),
                block_number: 0,
            },
            &mut IntroducedSigsMemo::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("no recorded merge base"),
            "must refuse to guess a multi-parent block's lineage: {err}"
        );
    }

    /// A failed execution rides the body while its effect is NOT in the
    /// state: enumeration must not count it, so a candidate missing it is
    /// still containing.
    #[tokio::test]
    async fn a_failed_execution_is_not_settled_content() {
        let v = val();
        let (e, c, d, m) = (h(0), h(1), h(2), h(3));
        let failed_deploy =
            crate::rust::util::construct_deploy::basic_deploy_data(7, None, None).expect("deploy");
        let mut failed_pd =
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(failed_deploy);
        failed_pd.is_failed = true;
        let dag = build_dag(vec![
            md(e.clone(), vec![], 0, &v),
            md(c.clone(), vec![e.clone()], 1, &v),
            md(d.clone(), vec![e.clone()], 1, &v),
            based(md(m.clone(), vec![c.clone(), d.clone()], 2, &v), &e),
        ]);
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&c, vec![e.clone()], 1, vec![], None, vec![failed_pd]),
            body_block(&d, vec![e.clone()], 1, vec![], None, vec![]),
            body_block(
                &m,
                vec![c.clone(), d.clone()],
                2,
                vec![],
                Some(e.clone()),
                vec![],
            ),
        ]);
        let mut memo = IntroducedSigsMemo::new();
        assert!(
            state_contains(
                &dag,
                &store,
                &Floor {
                    hash: m.clone(),
                    block_number: 2
                },
                &Floor {
                    hash: c.clone(),
                    block_number: 1
                },
                &mut memo,
            )
            .unwrap(),
            "a failed execution is not settled content — nothing is owed"
        );
    }

    /// THE ucc round-0 erasure falsifier (session 1f9bbf8f): the floor
    /// reached a carrier block C, and the next witnessed spine block S — a
    /// pre-existing merge whose recorded base PREDATES C and which recorded
    /// a rejection of C's chain — was accepted as the next floor because C
    /// is in S's DAG past. S's state never contained C's effects;
    /// designating it the floor erased settled state. The floor must SKIP S
    /// (its record defeats capture of the inherited floor C) and hold at C.
    #[tokio::test]
    async fn derive_floor_skips_witnessed_candidate_that_rejected_the_floors_content() {
        let v = val();
        let (e, c, d, s) = (h(0), h(1), h(2), h(3));
        let dag = build_dag(vec![
            md_wm(e.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(c.clone(), vec![e.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(d.clone(), vec![e.clone()], 1, &v, vec![(v.clone(), 1)]),
            based(
                md_wm(s.clone(), vec![c.clone(), d.clone()], 2, &v, vec![(
                    v.clone(),
                    1,
                )]),
                &e,
            ),
        ]);
        let seed = Bytes::from_static(b"settled_sig_ucc");
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&c, vec![e.clone()], 1, vec![seed.clone()], None, vec![]),
            body_block(&d, vec![e.clone()], 1, vec![], None, vec![]),
            body_block(
                &s,
                vec![c.clone(), d.clone()],
                2,
                vec![],
                Some(e.clone()),
                vec![],
            ),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), s.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![Floor {
            hash: c.clone(),
            block_number: 1,
        }];

        let (floor, _frontier) = derive_floor(&dag, &store, &[s.clone()], &j, thr, inherited)
            .await
            .expect("derive_floor");

        assert_eq!(
            floor.hash,
            c,
            "the floor must hold at the settled carrier C: the witnessed spine \
             block S recorded a rejection of C's chain and never captured C's \
             state — chosen {}#{}",
            PrettyPrinter::build_string_bytes(&floor.hash),
            floor.block_number,
        );
    }

    /// THE layer-F escape (ucc session 5fdb9bfe): record-emission
    /// suppression blinds capture. The settled carrier C's chain is kept
    /// out by TWO merges over the same stale base M: R records the
    /// rejection (non-duplicate, carrier C — the live #14), and S — the
    /// next spine block — dropped the chain too, but its identical record
    /// was SUPPRESSED at emission (the live #15, `884f978a`), so S's body
    /// and metadata carry no testimony. R sits on S's PARENT edge, not on
    /// S's base lineage (S's recorded base is M), so the capture walk from
    /// S sees no defeating record, height-exits below C, and answers
    /// "captured" — the floor advances onto a state missing C's settled
    /// content. The floor must hold at C.
    #[tokio::test]
    async fn derive_floor_skips_suppressed_record_spine_block() {
        let v = val();
        let (e, m, c, d, r, s) = (h(0), h(1), h(2), h(3), h(4), h(5));
        let wm = || vec![(v.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(e.clone(), vec![], 0, &v, wm()),
            md_wm(m.clone(), vec![e.clone()], 1, &v, wm()),
            md_wm(c.clone(), vec![m.clone()], 2, &v, wm()),
            md_wm(d.clone(), vec![m.clone()], 2, &v, wm()),
            // R: the recording merge (its record lives in its BODY on the
            // live path; the predicate reads no records either way).
            based(
                md_wm(r.clone(), vec![c.clone(), d.clone()], 3, &v, wm()),
                &m,
            ),
            // S: the suppressed-record merge — same stale base, C's chain
            // equally kept out of its state, but NO record of its own.
            based(
                md_wm(s.clone(), vec![r.clone(), c.clone()], 4, &v, wm()),
                &m,
            ),
        ]);
        let seed = Bytes::from_static(b"settled_sig_5fdb");
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&m, vec![e.clone()], 1, vec![], None, vec![]),
            body_block(&c, vec![m.clone()], 2, vec![seed.clone()], None, vec![]),
            body_block(&d, vec![m.clone()], 2, vec![], None, vec![]),
            body_block(
                &r,
                vec![c.clone(), d.clone()],
                3,
                vec![],
                Some(m.clone()),
                vec![],
            ),
            body_block(
                &s,
                vec![r.clone(), c.clone()],
                4,
                vec![],
                Some(m.clone()),
                vec![],
            ),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), s.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![Floor {
            hash: c.clone(),
            block_number: 2,
        }];

        let (floor, _frontier) = derive_floor(&dag, &store, &[s.clone()], &j, thr, inherited)
            .await
            .expect("derive_floor");

        assert_eq!(
            floor.hash,
            c,
            "the floor must hold at the settled carrier C: S's state derives \
             from the stale base M with C's chain kept out — the suppression \
             of S's own record must not launder the erasure — chosen {}#{}",
            PrettyPrinter::build_string_bytes(&floor.hash),
            floor.block_number,
        );
    }

    /// The complement of the falsifier — the geometry that WEDGED the first
    /// predicate: a witnessed merge that absorbed the floor's branch WITHOUT
    /// rejecting anything captures it (applied from scope), and the floor
    /// must advance onto it.
    #[tokio::test]
    async fn derive_floor_advances_onto_a_candidate_that_absorbed_the_floor() {
        let v = val();
        let (e, c, d, s) = (h(0), h(1), h(2), h(3));
        let dag = build_dag(vec![
            md_wm(e.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(c.clone(), vec![e.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(d.clone(), vec![e.clone()], 1, &v, vec![(v.clone(), 1)]),
            based(
                md_wm(s.clone(), vec![c.clone(), d.clone()], 2, &v, vec![(
                    v.clone(),
                    1,
                )]),
                &e,
            ),
        ]);
        let absorbed = Bytes::from_static(b"settled_sig_abs");
        let store = store_with(vec![
            body_block(&e, vec![], 0, vec![], None, vec![]),
            body_block(&c, vec![e.clone()], 1, vec![absorbed.clone()], None, vec![]),
            body_block(&d, vec![e.clone()], 1, vec![], None, vec![]),
            body_block(
                &s,
                vec![c.clone(), d.clone()],
                2,
                vec![absorbed.clone()],
                Some(e.clone()),
                vec![],
            ),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), s.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![Floor {
            hash: c.clone(),
            block_number: 1,
        }];

        let (floor, _frontier) = derive_floor(&dag, &store, &[s.clone()], &j, thr, inherited)
            .await
            .expect("derive_floor");

        assert_eq!(
            floor.hash, s,
            "a record-free absorbing merge captures the floor's content \
             (applied from scope) and soundly becomes the next floor"
        );
    }

    /// Guard-trip: a committee CHANGE inside the band (a bonding / re-stake between
    /// the pivot and the parent) breaks L-ANC's constant-committee premise. The warm
    /// up-walk MUST decline (`Ok(None)`) rather than serve a frontier computed under
    /// an inconsistent committee, and the dispatcher MUST fall back to the cold walk,
    /// yielding the identical frontier. This is the "tested" leg of the guard whose
    /// soundness `GuardBridge.chain_adj_AdjDC` derives in Rocq.
    #[tokio::test]
    async fn guard_trip_committee_change_falls_back_to_cold() {
        let v = h(50);
        let (g, b1, b2, b3) = (h(0), h(1), h(2), h(3));
        // v's weight changes 1 -> 2 at b2, so committee(b3) = wm(b2) = {v:2} differs
        // from pivot_committee = committee(b1) = wm(g) = {v:1}.
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(b1.clone(), vec![g.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(b2.clone(), vec![b1.clone()], 2, &v, vec![(v.clone(), 2)]),
            md_wm(b3.clone(), vec![b2.clone()], 3, &v, vec![(v.clone(), 2)]),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), b2.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);

        // Warm up-walk from pivot b1 must DECLINE (committee changes at b3 in the band).
        let warm = incremental_frontier(&dag, &b3, &b1, &j, thr).await.unwrap();
        assert!(
            warm.is_none(),
            "incremental_frontier must return Ok(None) on a committee change in the band"
        );

        // Seed the pivot so the dispatcher attempts (and must abandon) the warm path;
        // it must fall back to the cold walk and return the identical frontier.
        dag.put_cached_frontier(b3.clone(), b1.clone()).unwrap();
        let dispatched = parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        let cold = cold_parent_frontier(&dag, &b3, &j, thr).await.unwrap();
        assert_eq!(
            dispatched, cold,
            "on a guard trip the dispatched frontier must equal the cold walk (transparent)"
        );
    }

    /// Case-B (in-place finalization-advance dominates): the highest finalized
    /// candidate `c` is NOT a general ancestor of every parent (Case-A fails), yet
    /// every other candidate lies in `c`'s DAG past, so `c` is a sound base and is
    /// chosen. DAG: g <- t <- c <- p1 (validator v) plus t <- p2 (validator w, not in
    /// the committee). Over j={v:c}, c and t finalize but p1, p2 do not, so
    /// frontier(p1)=c and frontier(p2)=t. `c` is not an ancestor of p2, but t (p2's
    /// frontier) is in c's past ⇒ Case-B selects c. Mirrors Selection.case_b_compatible.
    #[tokio::test]
    async fn derive_floor_case_b_selects_dominating_finalized_tip() {
        let v = h(50);
        let w = h(51);
        let (g, t, c, p1, p2) = (h(0), h(1), h(2), h(3), h(4));
        let wm = || vec![(v.clone(), 1)]; // committee is always {v:1}; w never votes
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            md_wm(t.clone(), vec![g.clone()], 1, &v, wm()),
            md_wm(c.clone(), vec![t.clone()], 2, &v, wm()),
            md_wm(p1.clone(), vec![c.clone()], 3, &v, wm()),
            md_wm(p2.clone(), vec![t.clone()], 2, &w, wm()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone()); // v's frozen latest is c (before it made p1)
        let thr = FtThreshold::from_f32_lossy(0.1);

        let inherited = vec![
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
            Floor {
                hash: t.clone(),
                block_number: 1,
            },
        ];
        let (floor, _f) = derive_floor(
            &dag,
            &mk_store(),
            &[p1.clone(), p2.clone()],
            &j,
            thr,
            inherited,
        )
        .await
        .unwrap();
        assert_eq!(
            floor.hash, c,
            "Case-B must select the dominating finalized tip c (Case-A fails: c is not an \
             ancestor of p2; but t — the only other candidate — is in c's past)"
        );
    }

    /// Incompatible-fork Err: two parents with NO common finalized candidate must be
    /// surfaced as an error, never papered over with an unsound base. Modeled with a
    /// deliberately DISCONNECTED DAG (independent roots g_a, g_b) — the only shape in
    /// which two honest cuts are truly incompatible, since a `{1,1}` quorum can never
    /// finalize competing forks (both validators would have to agree). Each parent's
    /// frontier is its own root, so no candidate is a common ancestor and neither
    /// Case-A nor Case-B holds ⇒ the safety error fires (Selection.select_none_correct).
    #[tokio::test]
    async fn derive_floor_incompatible_fork_errors() {
        let v = h(50);
        let w = h(51);
        let (g_a, a1, g_b, b1) = (h(0), h(1), h(5), h(6));
        let dag = build_dag(vec![
            md_wm(g_a.clone(), vec![], 0, &v, vec![(v.clone(), 1)]),
            md_wm(a1.clone(), vec![g_a.clone()], 1, &v, vec![(v.clone(), 1)]),
            md_wm(g_b.clone(), vec![], 0, &w, vec![(w.clone(), 1)]),
            md_wm(b1.clone(), vec![g_b.clone()], 1, &w, vec![(w.clone(), 1)]),
        ]);
        let j = BTreeMap::new(); // nothing finalizes by quorum; frontiers fall to the roots
        let thr = FtThreshold::from_f32_lossy(0.1);

        let inherited = vec![
            Floor {
                hash: g_a.clone(),
                block_number: 0,
            },
            Floor {
                hash: g_b.clone(),
                block_number: 0,
            },
        ];
        let result = derive_floor(
            &dag,
            &mk_store(),
            &[a1.clone(), b1.clone()],
            &j,
            thr,
            inherited,
        )
        .await;
        match result {
            Err(CasperError::Other(msg)) => assert!(
                msg.contains("incompatible finalized fork"),
                "expected the incompatible-fork safety error, got: {msg}"
            ),
            other => panic!("expected Err(incompatible fork), got {other:?}"),
        }
    }

    /// A shared Case-A DAG: `g <- t <- c`, with BOTH parents `p1` (v) and `p2` (w)
    /// children of `c`, so `c` is a common ancestor of `{p1,p2}`. The committee is
    /// `{v:1}` (w never votes) and `j={v:c}`, so the whole chain `g,t,c` finalizes and
    /// `c` is the highest finalized common ancestor. Returns `(dag, j, thr, [p1,p2],
    /// inherited, g,t,c,p1,p2)`.
    #[allow(clippy::type_complexity)]
    fn case_a_fixture() -> (
        KeyValueDagRepresentation,
        BTreeMap<Bytes, Bytes>,
        FtThreshold,
        Vec<Bytes>,
        Vec<Floor>,
        (Bytes, Bytes, Bytes, Bytes, Bytes),
    ) {
        let v = h(50);
        let w = h(51);
        let (g, t, c, p1, p2) = (h(0), h(1), h(2), h(3), h(4));
        let wm = || vec![(v.clone(), 1)];
        let dag = build_dag(vec![
            md_wm(g.clone(), vec![], 0, &v, wm()),
            md_wm(t.clone(), vec![g.clone()], 1, &v, wm()),
            md_wm(c.clone(), vec![t.clone()], 2, &v, wm()),
            md_wm(p1.clone(), vec![c.clone()], 3, &v, wm()),
            md_wm(p2.clone(), vec![c.clone()], 3, &w, wm()),
        ]);
        let mut j = BTreeMap::new();
        j.insert(v.clone(), c.clone());
        let thr = FtThreshold::from_f32_lossy(0.1);
        let inherited = vec![
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
            Floor {
                hash: c.clone(),
                block_number: 2,
            },
        ];
        (
            dag,
            j,
            thr,
            vec![p1.clone(), p2.clone()],
            inherited,
            (g, t, c, p1, p2),
        )
    }

    /// T-LIN (`Selection.case_a_common_ancestor`): when the highest finalized candidate
    /// is a DAG-ancestor of EVERY parent (Case-A), `derive_floor` selects it AND the
    /// result is a genuine common ancestor of all parents — asserted via the sibling
    /// `is_dag_ancestor` primitive. (Previously only the Case-B pick and the
    /// incompatible-fork `Err` were tested; the Case-A common-ancestor property was not.)
    #[tokio::test]
    async fn derive_floor_case_a_floor_is_common_ancestor_of_all_parents() {
        let (dag, j, thr, parents, inherited, (_g, _t, c, p1, p2)) = case_a_fixture();
        let (floor, _f) = derive_floor(&dag, &mk_store(), &parents, &j, thr, inherited)
            .await
            .expect("derive_floor");
        assert_eq!(
            floor.hash, c,
            "Case-A selects the common-ancestor finalized candidate c"
        );
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p1)
                .expect("is_dag_ancestor"),
            "the Case-A floor must be a DAG-ancestor of parent p1"
        );
        assert!(
            dag.is_dag_ancestor(&floor.hash, &p2)
                .expect("is_dag_ancestor"),
            "the Case-A floor must be a DAG-ancestor of parent p2"
        );
    }

    /// Maximality / T-DET (`Selection.select_highest_sound`): the inheritance+advancement
    /// case. The two parents carry LAGGING inherited floors (`t@1`, `g@0` — older cuts),
    /// but the justification snapshot `j={v:c}` has since finalized `c`, so advancement
    /// surfaces `c@2` as a frontier candidate. The candidate multiset is therefore
    /// genuinely `{g@0, t@1, c@2}` — three sound bases of distinct height — and
    /// `derive_floor` must pick the strict MAXIMUM (`c@2`), never a lagging inherited cut.
    /// This is exactly the "a child never carries a lower cut than any parent, and
    /// advances to the highest newly-finalized candidate" contract (docstring on
    /// `finalized_floor`). `g` and `t` are shown to be sound competitors (common
    /// ancestors of both parents) and strictly lower, so the choice is a real maximum.
    #[tokio::test]
    async fn derive_floor_selects_highest_sound_finalized_candidate() {
        let (dag, j, thr, parents, _inherited, (g, t, _c, p1, p2)) = case_a_fixture();
        // Override the inherited floors with the LAGGING cuts the parents carried before
        // c finalized, forcing g@0 and t@1 into the candidate sort alongside advancement's c@2.
        let lagging = vec![
            Floor {
                hash: t.clone(),
                block_number: 1,
            },
            Floor {
                hash: g.clone(),
                block_number: 0,
            },
        ];
        let (floor, _f) = derive_floor(&dag, &mk_store(), &parents, &j, thr, lagging)
            .await
            .expect("derive_floor");
        assert_eq!(
            floor.block_number, 2,
            "advancement selects the highest sound candidate c (num 2), not the lagging inherited t=1 or g=0"
        );
        for (lower, lower_num) in [(&t, 1i64), (&g, 0)] {
            assert!(
                dag.is_dag_ancestor(lower, &p1).expect("anc")
                    && dag.is_dag_ancestor(lower, &p2).expect("anc"),
                "the lagging candidate (num {lower_num}) is ALSO a common ancestor of both parents (a competing sound base)"
            );
            assert!(
                lower_num < floor.block_number,
                "and is strictly lower-numbered than the chosen floor (maximality)"
            );
        }
    }

    /// T-FIN (`Selection.select_finalized` / `GuardBridge.upgo_finalized`): the floor
    /// `derive_floor` returns is itself `Finalized` over the justification snapshot — it
    /// clears the exact FT threshold (floor path, `≥`) per the same clique oracle the
    /// node runs (`CliqueOracle::ft_witnessed_exact`). Confirms the result is a genuinely
    /// finalized cut, not merely a well-formed ancestor.
    #[tokio::test]
    async fn derive_floor_result_is_finalized_over_justifications() {
        let (dag, j, thr, parents, inherited, _hashes) = case_a_fixture();
        let (floor, _f) = derive_floor(&dag, &mk_store(), &parents, &j, thr, inherited)
            .await
            .expect("derive_floor");
        let finalized = crate::rust::safety::clique_oracle::CliqueOracle::ft_witnessed_exact(
            &floor.hash,
            &dag,
            &j,
            thr,
            false,
        )
        .await
        .expect("ft_witnessed_exact");
        assert!(
            finalized,
            "the derive_floor result must be Finalized over the justification snapshot (T-FIN)"
        );
    }

    // ---- state-lineage: truncation is not a fork ----

    /// A hash of the length `KeyValueDagRepresentation::contains` requires.
    fn full_hash(tag: u8) -> Bytes { Bytes::from(vec![tag; models::rust::block_hash::LENGTH]) }

    fn md_base(hash: Bytes, parents: Vec<Bytes>, num: i64, base: Bytes) -> BlockMetadata {
        let sender = Bytes::from_static(b"sender");
        let mut meta = md_wm(hash, parents, num, &sender, vec![]);
        meta.merge_base = base;
        meta
    }

    /// "I cannot read that history" and "these lineages share no root" are
    /// different facts with the same shape, and only the second is a verdict.
    /// `state_contains` turns a missing meet into containment-refused and
    /// `derive_floor` turns it into candidate-skipped, so answering it from what
    /// this node happens to hold would make the floor node-local — the thing
    /// R-DET forbids. A lineage that leaves the blocks we hold must raise, and
    /// must stay distinguishable from one that genuinely diverges.
    #[test]
    fn truncated_state_lineage_is_an_error_not_a_disconnection() {
        let root = Bytes::from_static(b"root");
        let gone = Bytes::from_static(b"never-downloaded");
        let a = Bytes::from_static(b"a");
        let b = Bytes::from_static(b"b");

        // `a`'s state lineage runs off the blocks this node holds; `b`'s is whole.
        let dag = build_dag(vec![
            md_base(root.clone(), vec![], 1, Bytes::new()),
            md_base(a.clone(), vec![root.clone()], 5, gone.clone()),
            md_base(b.clone(), vec![root.clone()], 5, root.clone()),
        ]);

        let err = state_lineage_meet(&dag, &a, &b)
            .expect_err("a lineage that leaves the held blocks cannot yield a verdict");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == gone),
            "truncation must be a TYPED error naming the block this node does not hold, so \
             the caller can request it and retry instead of turning it into a verdict; got {err}"
        );
    }

    /// The floor's own recursion has the same edge. `floor_of_block` walks down
    /// to a cached floor or a parentless block; a node whose history was
    /// truncated has neither, so the walk leaves the blocks it holds. That is a
    /// gap to be filled, not a fact about the block, and it has to be reported
    /// as one — the caller turns anything else into a slashable verdict against
    /// whoever proposed the block.
    #[tokio::test]
    async fn floor_of_block_reports_the_block_it_does_not_hold() {
        let gone = Bytes::from_static(b"below-the-sync-window");
        let oldest = Bytes::from_static(b"oldest-retained");
        let tip = Bytes::from_static(b"tip");

        let dag = build_dag(vec![
            md_base(oldest.clone(), vec![gone.clone()], 36, gone.clone()),
            md_base(tip.clone(), vec![oldest.clone()], 37, oldest.clone()),
        ]);

        let err = floor_of_block(&dag, &mk_store(), &tip, FtThreshold::from_f32_lossy(0.1))
            .await
            .expect_err("a floor recursion that leaves the held blocks cannot yield a floor");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == gone),
            "the floor recursion must name the block it does not hold; got {err}"
        );
    }

    /// The frontier walk meets absence one level lower than the floor recursion
    /// does: it reaches the clique oracle, whose every DAG read goes through the
    /// `_unsafe` primitives. Those report a block the node does not hold as a
    /// plain store error, which the caller folds into the storage-failure class
    /// and converts to a slashable verdict. Absence has to keep its name all the
    /// way down, or the deferral guarantee stops at `floor.rs` and a joiner that
    /// is one block short of the history it needs accuses whoever proposed.
    #[tokio::test]
    async fn the_oracle_reports_the_block_it_does_not_hold() {
        // Full-length hashes: `dag.contains` length-checks, and a short hash
        // would short-circuit the oracle before it reads any parent.
        let gone = full_hash(0xB0);
        let tip = full_hash(0x37);

        // The oldest block this node kept, whose own main parent is below the
        // window: the oracle needs that parent for the corresponding weight map.
        let dag = build_dag(vec![md_base(
            tip.clone(),
            vec![gone.clone()],
            37,
            gone.clone(),
        )]);

        let err = parent_frontier(
            &dag,
            &tip,
            &BTreeMap::new(),
            FtThreshold::from_f32_lossy(0.1),
        )
        .await
        .expect_err("a frontier walk that leaves the held blocks cannot yield a frontier");
        assert!(
            matches!(err, CasperError::BlockNotHeld(ref h) if *h == gone),
            "the oracle must name the block it does not hold, so the caller can request \
             it and retry instead of recording a verdict against the proposer; got {err}"
        );
    }

    /// The other half: two lineages that both reach a root without meeting are
    /// genuinely disconnected, and that IS a verdict the caller may act on.
    #[test]
    fn disconnected_state_lineages_are_reported_as_disconnected() {
        let root_a = Bytes::from_static(b"root-a");
        let root_b = Bytes::from_static(b"root-b");
        let a = Bytes::from_static(b"a");
        let b = Bytes::from_static(b"b");

        let dag = build_dag(vec![
            md_base(root_a.clone(), vec![], 1, Bytes::new()),
            md_base(root_b.clone(), vec![], 1, Bytes::new()),
            md_base(a.clone(), vec![root_a.clone()], 5, root_a.clone()),
            md_base(b.clone(), vec![root_b.clone()], 5, root_b.clone()),
        ]);

        let meet = state_lineage_meet(&dag, &a, &b).expect("rooted lineages must not raise");
        assert!(
            matches!(meet, StateLineage::Disconnected),
            "two lineages that reach separate roots share no state history"
        );
    }

    // ---- Phase-4 T-DET maximality: derive_floor picks the HIGHEST sound candidate ----

    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    lazy_static::lazy_static! {
        // Shared Tokio runtime for the `#[test]` proptest (it cannot be `#[tokio::test]`);
        // mirrors casper/tests/fork_choice/prop_estimator_determinism.rs.
        static ref FLOOR_RUNTIME: tokio::runtime::Runtime =
            tokio::runtime::Runtime::new().expect("tokio runtime");
    }

    prop_compose! {
        /// A single-validator linear chain `g=b0 <- b1 <- … <- b_depth` ({v:1}
        /// committee), a frontier index `k ∈ [1, depth)` (so `frontier(b_depth over
        /// {v:b_k}) = b_k`, strictly below the parent), and a random `inherited_mask`
        /// selecting which strict ancestors `b_0..b_{depth-1}` are also fed as inherited
        /// floors. Every candidate is a DAG-ancestor of the single parent `b_depth`
        /// (linear chain) ⇒ every candidate is a Case-A sound base ⇒ maximality alone
        /// decides the floor.
        fn chain_scenario()(depth in 2usize..=6)(
            k in 1usize..depth,
            inherited_mask in prop::collection::vec(any::<bool>(), depth),
            depth in Just(depth),
        ) -> (usize, usize, Vec<bool>) {
            (depth, k, inherited_mask)
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 24, max_shrink_iters: 8, ..ProptestConfig::default() })]

        // maximality / T-DET (`Selection.select_highest_sound`): over a
        // descending-sorted candidate set of Case-A sound bases (inherited parent floors
        // ∪ the per-parent frontier), `derive_floor` returns the candidate of GREATEST
        // block number — the highest sound base. This is the general (proptest) form of
        // the hand-built `derive_floor_selects_highest_sound_finalized_candidate` example:
        // the candidate multiset is randomly `{inherited b_i} ∪ {frontier b_k}`, and the
        // chosen floor must always be its maximum, regardless of whether inheritance or
        // advancement supplies it (monotonicity in BOTH directions).
        #[test]
        fn derive_floor_selects_highest_sound_candidate_over_chain((depth, k, mask) in chain_scenario()) {
            FLOOR_RUNTIME.block_on(async move {
                let v = h(50);
                let mut blocks: Vec<BlockMetadata> = Vec::with_capacity(depth + 1);
                for i in 0..=depth {
                    let parents = if i == 0 { vec![] } else { vec![h((i - 1) as u8)] };
                    blocks.push(md_wm(h(i as u8), parents, i as i64, &v, vec![(v.clone(), 1)]));
                }
                let dag = build_dag(blocks);
                let parent = h(depth as u8);

                // frontier(parent over {v:b_k}) = b_k (single-validator chain).
                let mut j = BTreeMap::new();
                j.insert(v.clone(), h(k as u8));
                let thr = FtThreshold::from_f32_lossy(0.1);

                // Inherited candidates: the selected strict ancestors b_0..b_{depth-1}.
                let mut inherited: Vec<Floor> = Vec::new();
                for (i, &selected) in mask.iter().enumerate().take(depth) {
                    if selected {
                        inherited.push(Floor { hash: h(i as u8), block_number: i as i64 });
                    }
                }

                let (floor, _f) = derive_floor(&dag, &mk_store(), &[parent], &j, thr, inherited.clone())
                    .await
                    .expect("derive_floor");

                // Oracle: the maximum block number over inherited ∪ {frontier k}. All
                // candidates are distinct chain positions, so the max hash is unambiguous.
                let expected_num = inherited
                    .iter()
                    .map(|f| f.block_number)
                    .chain(std::iter::once(k as i64))
                    .max()
                    .expect("candidate set is non-empty (frontier always present)");
                prop_assert_eq!(
                    floor.block_number, expected_num,
                    "derive_floor must select the HIGHEST sound candidate's block number"
                );
                prop_assert_eq!(
                    floor.hash, h(expected_num as u8),
                    "derive_floor must select the chain block at the highest candidate number"
                );
                Ok::<(), TestCaseError>(())
            })?;
        }
    }

    prop_compose! {
        /// A settled chain `b_0 <- … <- b_n`, a carrier `c` on `b_n`
        /// introducing a settled sig, and a witnessed merge `s` above `c`
        /// whose RECORDED base is `b_j` (`j < n`). In the ERASURE arm `s`'s
        /// merge dropped `c`'s chain (its applied set is empty — a record
        /// may or may not exist; containment does not consult records); in
        /// the absorption arm the chain was applied from scope. The
        /// inherited floor is `c` in both.
        fn stale_spine_scenario()(n in 2usize..=5)(
            j in 0usize..n,
            erasure in any::<bool>(),
            n in Just(n),
        ) -> (usize, usize, bool) {
            (n, j, erasure)
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, max_shrink_iters: 8, ..ProptestConfig::default() })]

        // State-monotone advancement at randomized depths: a witnessed
        // candidate whose state DROPPED the inherited floor's settled chain
        // is never chosen over it — recorded or not — while the same
        // candidate absorbing that chain (applied from scope) advances
        // soundly.
        #[test]
        fn derive_floor_advance_is_containment_gated(
            (n, j, erasure) in stale_spine_scenario()
        ) {
            FLOOR_RUNTIME.block_on(async move {
                let v = h(50);
                let (c, s) = (h(20), h(21));
                let settled_sig = Bytes::from_static(b"settled_sig_prop");
                let mut blocks: Vec<BlockMetadata> = Vec::new();
                let mut bodies = Vec::new();
                for i in 0..=n {
                    let parents = if i == 0 { vec![] } else { vec![h((i - 1) as u8)] };
                    blocks.push(md_wm(h(i as u8), parents.clone(), i as i64, &v, vec![(v.clone(), 1)]));
                    bodies.push(body_block(&h(i as u8), parents, i as i64, vec![], None, vec![]));
                }
                blocks.push(md_wm(
                    c.clone(),
                    vec![h(n as u8)],
                    (n + 1) as i64,
                    &v,
                    vec![(v.clone(), 1)],
                ));
                bodies.push(body_block(
                    &c,
                    vec![h(n as u8)],
                    (n + 1) as i64,
                    vec![settled_sig.clone()],
                    None,
                    vec![],
                ));
                let s_meta = based(
                    md_wm(
                        s.clone(),
                        vec![c.clone(), h(n as u8)],
                        (n + 2) as i64,
                        &v,
                        vec![(v.clone(), 1)],
                    ),
                    &h(j as u8),
                );
                blocks.push(s_meta);
                bodies.push(body_block(
                    &s,
                    vec![c.clone(), h(n as u8)],
                    (n + 2) as i64,
                    if erasure { vec![] } else { vec![settled_sig.clone()] },
                    Some(h(j as u8)),
                    vec![],
                ));
                let dag = build_dag(blocks);
                let store = store_with(bodies);

                let mut jmap = BTreeMap::new();
                jmap.insert(v.clone(), s.clone());
                let thr = FtThreshold::from_f32_lossy(0.1);
                let inherited =
                    vec![Floor { hash: c.clone(), block_number: (n + 1) as i64 }];

                let (floor, _f) = derive_floor(&dag, &store, &[s.clone()], &jmap, thr, inherited)
                    .await
                    .expect("derive_floor");

                if erasure {
                    prop_assert_eq!(
                        floor.hash, c,
                        "a candidate whose state dropped the floor's settled \
                         chain must never displace it"
                    );
                } else {
                    prop_assert_eq!(
                        floor.hash, s,
                        "an absorbing candidate contains the floor's settled \
                         content and advances soundly"
                    );
                }
                Ok::<(), TestCaseError>(())
            })?;
        }
    }
}
