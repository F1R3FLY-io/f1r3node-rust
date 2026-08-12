// See casper/src/main/scala/coop/rchain/casper/util/rholang/InterpreterUtil.scala

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, ProcessedDeploy, ProcessedSystemDeploy, RejectedDeploy,
    SystemDeployData,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use super::replay_failure::ReplayFailure;
use super::runtime_manager::RuntimeManager;
use crate::rust::block_status::BlockStatus;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::metrics_constants::{BLOCK_PROCESSING_REPLAY_TIME_METRIC, CASPER_METRICS_SOURCE};
use crate::rust::util::proto_util;
use crate::rust::BlockProcessing;

pub fn mk_term(rho: &str, normalizer_env: HashMap<String, Par>) -> Result<Par, InterpreterError> {
    Compiler::source_to_adt_with_normalizer_env(rho, normalizer_env)
}

/// Sigs whose LATEST canonical disposition across the FULL merge scope is a WIN.
/// BFS-walks the closure of ALL `parents` (not just the main-parent chain) down to
/// the deploy-lifespan floor, recording each sig's highest-block disposition: in a
/// block's `body.deploys` it is a WIN, in `body.rejected_deploys` a REJECTION.
/// Returns the sigs whose highest-block disposition is a WIN — their effect is in
/// the merge base the proposing block builds on, so re-proposing them would
/// double-apply. Walking ALL parents (not only `parents[0]`) catches a deploy
/// already won on a CO-PARENT of a multi-parent merge, which a main-parent-only
/// walk misses and re-proposes into a double-apply; a real recovery loser is still
/// exempt because the merge that rejected it sits at a strictly higher block than
/// its inclusion. Reads only signed block bodies, so the result is node-identical.
pub fn canonical_won_sigs(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<HashSet<Bytes>, CasperError> {
    canonical_disposition_sigs(block_store, parents, earliest_block_number, true)
}

fn block_in_floor_merge_scope(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
    floor_hash: &BlockHash,
    floor_block_number: i64,
) -> Result<bool, CasperError> {
    let meta = dag.lookup_unsafe(hash)?;
    if meta.block_number > floor_block_number {
        return Ok(true);
    }
    Ok(!dag.is_dag_ancestor(hash, floor_hash)?)
}

/// True iff a deploy's SUCCESSFUL effect is already present in `base_state`
/// — i.e. the deploy already executed in the lineage that state derives
/// from. Applying (or re-executing) such a deploy again would double its
/// per-deploy cells.
///
/// Signal: the deploy's own created number cells. Each carries a
/// sig-derived rnd, so its channel NAME is unique to this deploy and stable
/// across executions even when the VALUE is base-dependent — a match is
/// false-positive-free even on shared channels. The check restricts to
/// channels the deploy CREATED (absent in the executing block's pre-state);
/// shared channels the pre-charge merely reads are excluded.
///
/// CONTRACT — the probe's blind spots, both deliberate:
/// - A deploy that creates NO number cells is invisible here and always
///   reads not-settled. Same-sig copies that meet in one merge scope are
///   still deduped by the freshest-copy rule regardless; only a
///   base-settled no-cell deploy re-entering via scope escapes both, and
///   its duplicate effect is additive datums on multi-datum channels,
///   never a single-value-cell violation.
/// - A FAILED execution's created cells are not in any committed state, so
///   the probe correctly reads not-settled: a failed run's charge landed
///   but its effect did not, and re-execution is legitimate.
pub(crate) fn deploy_effect_in_state(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    base_state: &Blake2b256Hash,
    sig: &Bytes,
) -> Result<bool, CasperError> {
    let Some(origin_hash) = dag
        .lookup_by_deploy_id(&sig.to_vec())
        .map_err(CasperError::KvStoreError)?
    else {
        return Ok(false);
    };
    let Some(origin) = block_store.get(&origin_hash)? else {
        return Ok(false);
    };
    let Some(idx) = origin
        .body
        .deploys
        .iter()
        .position(|pd| pd.deploy.sig == *sig)
    else {
        return Ok(false);
    };

    let mergeable_chs = runtime_manager.load_mergeable_channels(
        &origin.body.state.post_state_hash,
        origin.sender.clone(),
        origin.seq_num,
    )?;
    let Some(merge_chs) = mergeable_chs.get(idx) else {
        return Ok(false);
    };
    if merge_chs.is_empty() {
        return Ok(false);
    }

    let origin_pre = Blake2b256Hash::from_bytes_prost(&origin.body.state.pre_state_hash);
    let origin_pre_reader = runtime_manager
        .history_repo
        .get_history_reader(&origin_pre)
        .map_err(CasperError::HistoryError)?;
    let base_reader = runtime_manager
        .history_repo
        .get_history_reader(base_state)
        .map_err(CasperError::HistoryError)?;
    for (ch, _) in merge_chs.iter() {
        let created_by_deploy = origin_pre_reader
            .get_data(ch)
            .map_err(CasperError::HistoryError)?
            .is_empty();
        if !created_by_deploy {
            continue;
        }
        let in_base = !base_reader
            .get_data(ch)
            .map_err(CasperError::HistoryError)?
            .is_empty();
        tracing::debug!(
            target: "f1r3.trace.basecheck",
            sig = %hex::encode(&sig[..sig.len().min(8)]),
            channel = %hex::encode(&ch.bytes()[..6]),
            in_base,
            "base-check per-deploy created cell"
        );
        if in_base {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Per-sig canonical disposition facts over one operation's parent cone.
/// One walk collects everything recovery reads: the latest disposition
/// (who currently stands), the latest kept rejection record (the
/// adjudication a retry settles against), and the first carrier (whose
/// sender is the deploy's recovery owner).
#[derive(Debug, Clone)]
pub(crate) struct SigDisposition {
    /// Latest disposition as `(height, won)`. A win and a rejection never
    /// share a height (a merge sits one block above its siblings), so the
    /// highest-block disposition is the latest; a same-height tie prefers
    /// REJECTION so a keep-one loser stays re-proposable.
    pub latest: (i64, bool),
    /// The highest-block kept rejection RECORD naming this sig, as
    /// `(height, record-bearing block)`. A same-height tie breaks to the
    /// lexicographically LARGER hash so every node lands on one block.
    pub latest_kept_rejection: Option<(i64, BlockHash)>,
    /// The lowest-block INCLUSION of this sig, as
    /// `(height, carrier, carrier sender)`. A same-height tie breaks to
    /// the lexicographically SMALLER hash.
    pub first_carrier: Option<(i64, BlockHash, Bytes)>,
}

impl SigDisposition {
    pub fn won(&self) -> bool { self.latest.1 }
}

/// The latest-disposition tie logic shared by every disposition map: keep
/// the higher block; at equal height an existing REJECTION stands.
fn update_latest(latest: &mut (i64, bool), bn: i64, won: bool) {
    let (best_bn, best_won) = *latest;
    if best_bn > bn || (best_bn == bn && !best_won) {
        return;
    }
    *latest = (bn, won);
}

fn note_inclusion(
    dispositions: &mut HashMap<Bytes, SigDisposition>,
    sig: Bytes,
    bn: i64,
    carrier: &BlockHash,
    sender: &Bytes,
) {
    let disposition = dispositions.entry(sig).or_insert(SigDisposition {
        latest: (bn, true),
        latest_kept_rejection: None,
        first_carrier: None,
    });
    update_latest(&mut disposition.latest, bn, true);
    match &disposition.first_carrier {
        Some((h, b, _)) if *h < bn || (*h == bn && b <= carrier) => {}
        _ => disposition.first_carrier = Some((bn, carrier.clone(), sender.clone())),
    }
}

fn note_kept_rejection(
    dispositions: &mut HashMap<Bytes, SigDisposition>,
    sig: Bytes,
    bn: i64,
    record_block: &BlockHash,
) {
    let disposition = dispositions.entry(sig).or_insert(SigDisposition {
        latest: (bn, false),
        latest_kept_rejection: None,
        first_carrier: None,
    });
    update_latest(&mut disposition.latest, bn, false);
    match &disposition.latest_kept_rejection {
        Some((h, b)) if *h > bn || (*h == bn && b >= record_block) => {}
        _ => disposition.latest_kept_rejection = Some((bn, record_block.clone())),
    }
}

/// BFS-walk the closure of `parents` down to `earliest_block_number`,
/// collecting each sig's `SigDisposition`.
pub(crate) fn canonical_dispositions(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<HashMap<Bytes, SigDisposition>, CasperError> {
    let mut dispositions: HashMap<Bytes, SigDisposition> = HashMap::new();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut queue: VecDeque<BlockHash> = parents.iter().cloned().collect();
    while let Some(hash) = queue.pop_front() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        let Some(block) = block_store.get(&hash)? else {
            continue;
        };
        let bn = block.body.state.block_number;
        if bn < earliest_block_number {
            continue;
        }
        for pd in &block.body.deploys {
            note_inclusion(
                &mut dispositions,
                pd.deploy.sig.clone(),
                bn,
                &hash,
                &block.sender,
            );
        }
        for rd in proto_util::kept_rejected_records(&block) {
            note_kept_rejection(&mut dispositions, rd.sig.clone(), bn, &hash);
        }
        for p in &block.header.parents_hash_list {
            queue.push_back(p.clone());
        }
    }
    Ok(dispositions)
}

fn canonical_disposition_sigs(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
    target_won: bool,
) -> Result<HashSet<Bytes>, CasperError> {
    Ok(
        canonical_dispositions(block_store, parents, earliest_block_number)?
            .into_iter()
            .filter_map(|(sig, disposition)| (disposition.won() == target_won).then_some(sig))
            .collect(),
    )
}

fn rejected_sig_has_visible_non_source_win(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
    sig: &Bytes,
    source_block: &BlockHash,
) -> Result<bool, CasperError> {
    let mut disposition: HashMap<Bytes, (i64, bool)> = HashMap::new();
    for hash in visible_blocks {
        if hash == source_block {
            continue;
        }
        let Some(block) = block_store.get(hash)? else {
            continue;
        };
        let bn = block.body.state.block_number;
        for pd in &block.body.deploys {
            if pd.deploy.sig == *sig {
                record_disposition(&mut disposition, pd.deploy.sig.clone(), bn, true);
            }
        }
        for rd in proto_util::kept_rejected_records(&block) {
            if rd.sig == *sig {
                record_disposition(&mut disposition, rd.sig.clone(), bn, false);
            }
        }
    }
    Ok(disposition.get(sig).map(|(_, won)| *won).unwrap_or(false))
}

/// Record-emission dedup, carrier-exact: a computed record is redundant —
/// and only then suppressible — when a visible block already carries the
/// IDENTICAL record (same sig, same carrier, same duplicate flag). A
/// record adjudicates exactly the copy its carrier names; a visible record
/// naming a sibling carrier of the same sig covers nothing about this one,
/// and suppressing on it leaves an adjudicated copy permanently
/// unrecorded — the copy then reads as a standing win and recovery stands
/// down on lost work. Flag-differing records are distinct testimony (the
/// same copy can be adjudicated redundant by one merge and effect-dropping
/// by another base) and never suppress each other.
fn suppress_already_recorded_rejections(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
    rejected_records: &mut Vec<RejectedDeploy>,
) -> Result<usize, CasperError> {
    if rejected_records.is_empty() {
        return Ok(0);
    }

    let mut visible_records: HashSet<RejectedDeploy> = HashSet::new();
    for hash in visible_blocks {
        let Some(block) = block_store.get(hash)? else {
            continue;
        };
        visible_records.extend(block.body.rejected_deploys.iter().cloned());
    }
    if visible_records.is_empty() {
        return Ok(0);
    }

    let before = rejected_records.len();
    rejected_records.retain(|record| !visible_records.contains(record));
    Ok(before.saturating_sub(rejected_records.len()))
}

/// Buffer retain as a pure floor-window rule: keep a rejected deploy while
/// its validity window is open at the merging block's floor
/// (node-identical — the floor derives from the block's frozen
/// justifications), drop it only once the floor passes the window. A
/// window-closed deploy could never land again anyway, and no node-local
/// verdict may shorten the buffer's custody: the floor is the only
/// irreversibility line — a win merely marked finalized by this node's
/// finalizer can still sit above the floor where a later merge can reject
/// it, and the buffer holds the only re-proposable copy. Settled work
/// leaves through the terminal purge or at window close, never through a
/// local verdict.
fn retain_recoverable_rejected_deploys_for_buffer(
    floor_block_number: i64,
    deploy_lifespan: i64,
    deploys: &mut Vec<Signed<DeployData>>,
) -> usize {
    let before = deploys.len();
    deploys.retain(|deploy| {
        deploy
            .data
            .valid_after_block_number
            .saturating_add(deploy_lifespan)
            > floor_block_number
    });
    before - deploys.len()
}

/// Update `disposition[sig]` toward the latest (highest-block) verdict. A higher
/// block wins; at a tie a REJECTION overrides a WIN (a loser must stay re-proposable).
fn record_disposition(
    disposition: &mut HashMap<Bytes, (i64, bool)>,
    sig: Bytes,
    bn: i64,
    won: bool,
) {
    match disposition.entry(sig) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            update_latest(entry.get_mut(), bn, won)
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert((bn, won));
        }
    }
}

/// The applied set and merge base are frozen merge facts the
/// state-membership walk reads; a block recording facts its
/// validator-recomputed merge did not produce is invalid, the same
/// contract as the rejected records.
fn state_facts_mismatch(
    block: &BlockMessage,
    merged: &super::runtime_manager::MergedPreState,
) -> bool {
    let block_applied: HashSet<Bytes> = block.body.applied_from_scope.iter().cloned().collect();
    let block_base: Option<BlockHash> = if block.body.merge_base.is_empty() {
        None
    } else {
        Some(block.body.merge_base.clone())
    };
    block_applied != merged.applied_from_scope || block_base != merged.merge_base
}

// Returns (None, checkpoints) if the block's tuplespace hash
// does not match the computed hash based on the deploys
pub async fn validate_block_checkpoint(
    block: &BlockMessage,
    block_store: &KeyValueBlockStore,
    s: &mut CasperSnapshot,
    runtime_manager: &RuntimeManager,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    floor_ctx: Option<&crate::rust::finality::floor_context::FloorContext>,
    // This node's validator identity. The rejected-deploy buffer is
    // OWNER-SCOPED: only the sender of a rejected copy's carrier buffers
    // its retries, so the populate must know who "we" are. `None`
    // (observers, exploratory contexts) buffers nothing.
    local_validator: Option<&Validator>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    tracing::trace!(target: "f1r3fly.casper.block_validation", "before-unsafe-get-parents");
    let incoming_pre_state_hash = proto_util::pre_state_hash(block);
    let parents = proto_util::get_parents(block_store, block);
    tracing::debug!(target: "f1r3fly.casper.block_validation", block = %hex::encode(&block.block_hash[..8.min(block.block_hash.len())]), seq = block.seq_num, n_parents = parents.len(), "validate.block_checkpoint ENTER (recompute parents post-state, then replay)");
    tracing::trace!(target: "f1r3fly.casper.block_validation", parent_count = parents.len(), "before-compute-parents-post-state");
    let parents_post_state_start = std::time::Instant::now();
    // Validate: the floor must be derived from the BLOCK's own recorded
    // justifications (node-identical), not the validating node's live view.
    let latest_messages: BTreeMap<Validator, BlockHash> = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let computed_parents_info = compute_parents_post_state(
        block_store,
        parents.clone(),
        s,
        runtime_manager,
        &latest_messages,
        None,
        rejected_deploy_buffer,
        floor_ctx,
        local_validator,
    )
    .await;
    metrics::histogram!(
        crate::rust::metrics_constants::BLOCK_PROCESSING_PARENTS_POST_STATE_TIME_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .record(parents_post_state_start.elapsed().as_secs_f64());

    tracing::info!(
        "Computed parents post state for {}.",
        PrettyPrinter::build_string_block_message(block, false)
    );

    match computed_parents_info {
        Ok(merged) => {
            let computed_pre_state_hash = merged.state.clone();
            // The rejected-record equality is exact and total: every record
            // the recomputed merge formed must appear in the block — sig,
            // carrier, and duplicate flag — and nothing else may. There is
            // deliberately NO carve-out for sigs the block also carries in
            // `body.deploys`: a record is consensus content regardless of
            // whether its subject rides fresh in the same block, and
            // excusing that overlap let a block drop the adjudication its
            // own merge computed while executing the adjudicated deploy.
            let computed_rejected: HashSet<RejectedDeploy> =
                merged.rejected_user.iter().cloned().collect();
            let block_rejected: HashSet<RejectedDeploy> =
                block.body.rejected_deploys.iter().cloned().collect();

            if incoming_pre_state_hash != computed_pre_state_hash {
                tracing::debug!(target: "f1r3fly.casper.block_validation", block = %hex::encode(&block.block_hash[..8.min(block.block_hash.len())]), computed = %hex::encode(&computed_pre_state_hash[..8.min(computed_pre_state_hash.len())]), incoming = %hex::encode(&incoming_pre_state_hash[..8.min(incoming_pre_state_hash.len())]), "validate.block_checkpoint: PRE-STATE MISMATCH (recomputed merge != block's recorded pre-state) -> reject, NO replay");
                // TODO: at this point we may just as well terminate the replay, there's no way it will succeed.
                tracing::warn!(
                    "Computed pre-state hash {} does not equal block's pre-state hash {}.",
                    PrettyPrinter::build_string_bytes(&computed_pre_state_hash),
                    PrettyPrinter::build_string_bytes(&incoming_pre_state_hash)
                );

                Ok(Either::Right(None))
            } else if computed_rejected != block_rejected {
                // Detailed logging for InvalidRejectedDeploy mismatch: a
                // record differing in ANY field — sig, carrier, or
                // duplicate flag — is a disagreement.
                let describe = |record: &RejectedDeploy| {
                    format!(
                        "{}@{}{}",
                        hex::encode(&record.sig[..8.min(record.sig.len())]),
                        hex::encode(&record.carrier[..8.min(record.carrier.len())]),
                        if record.duplicate { "(dup)" } else { "" }
                    )
                };
                let extra_in_computed: Vec<String> = computed_rejected
                    .difference(&block_rejected)
                    .map(describe)
                    .collect();
                let missing_in_computed: Vec<String> = block_rejected
                    .difference(&computed_rejected)
                    .map(describe)
                    .collect();

                // Find duplicates across all deploy sigs in the block
                let mut sig_counts: HashMap<Bytes, usize> = HashMap::new();
                for pd in &block.body.deploys {
                    *sig_counts.entry(pd.deploy.sig.clone()).or_insert(0) += 1;
                }
                for rd in &block.body.rejected_deploys {
                    *sig_counts.entry(rd.sig.clone()).or_insert(0) += 1;
                }
                let duplicate_count = sig_counts.values().filter(|&&c| c > 1).count();

                tracing::error!(
                    block_num = block.body.state.block_number,
                    block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    sender = %PrettyPrinter::build_string_bytes(&block.sender),
                    validator_rejected = computed_rejected.len(),
                    block_rejected = block_rejected.len(),
                    extra_count = extra_in_computed.len(),
                    missing_count = missing_in_computed.len(),
                    extra_in_computed = ?extra_in_computed,
                    missing_in_computed = ?missing_in_computed,
                    duplicate_count,
                    "rejected deploy mismatch: validator and block creator disagree on rejected records"
                );

                Ok(Either::Left(BlockStatus::invalid_rejected_deploy()))
            } else if state_facts_mismatch(block, &merged) {
                tracing::error!(
                    target: "f1r3fly.casper.block_validation",
                    block_num = block.body.state.block_number,
                    block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    sender = %PrettyPrinter::build_string_bytes(&block.sender),
                    block_applied = block.body.applied_from_scope.len(),
                    validator_applied = merged.applied_from_scope.len(),
                    block_base = %PrettyPrinter::build_string_bytes(&block.body.merge_base),
                    "applied-from-scope / merge-base mismatch: validator and \
                     block creator disagree on the merge's recorded state facts"
                );

                Ok(Either::Left(BlockStatus::invalid_rejected_deploy()))
            } else {
                tracing::debug!(target: "f1r3fly.casper.replay_block", "before-process-pre-state-hash");
                // Using tracing events for async - Span[F] equivalent from Scala
                tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-started");
                let replay_start = std::time::Instant::now();
                let replay_result =
                    replay_block(incoming_pre_state_hash, block, &mut s.dag, runtime_manager)
                        .await?;
                metrics::histogram!(BLOCK_PROCESSING_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(replay_start.elapsed().as_secs_f64());
                tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-finished");

                handle_errors(proto_util::post_state_hash(block), replay_result)
            }
        }
        Err(ex) => Ok(Either::Left(BlockStatus::exception(ex))),
    }
}

async fn replay_block(
    initial_state_hash: StateHash,
    block: &BlockMessage,
    dag: &mut KeyValueDagRepresentation,
    runtime_manager: &RuntimeManager,
) -> Result<Either<ReplayFailure, StateHash>, CasperError> {
    // Extract deploys and system deploys from the block
    let internal_deploys = proto_util::deploys(block);
    let internal_system_deploys = proto_util::system_deploys(block);

    // Check for duplicate deploys in the block before replay
    let mut all_deploy_sigs: Vec<Bytes> = internal_deploys
        .iter()
        .map(|pd| pd.deploy.sig.clone())
        .collect();
    all_deploy_sigs.extend(block.body.rejected_deploys.iter().map(|rd| rd.sig.clone()));

    let mut sig_counts: HashMap<Bytes, usize> = HashMap::new();
    for sig in &all_deploy_sigs {
        *sig_counts.entry(sig.clone()).or_insert(0) += 1;
    }
    let deploy_duplicates: HashMap<Bytes, usize> = sig_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect();

    if !deploy_duplicates.is_empty() {
        let duplicates_str: String = deploy_duplicates
            .iter()
            .map(|(sig, count)| {
                format!(
                    "  {} (appears {} times)",
                    PrettyPrinter::build_string_bytes(sig),
                    count
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        tracing::warn!(
            "\n=== Duplicate Deploys Detected in Block ===\n\
            Block #{} ({})\n\
            Found {} duplicate deploy signatures:\n{}\n\
            Total deploys: {}\n\
            Total rejected: {}\n\
            ============================================",
            block.body.state.block_number,
            PrettyPrinter::build_string_bytes(&block.block_hash),
            deploy_duplicates.len(),
            duplicates_str,
            internal_deploys.len(),
            block.body.rejected_deploys.len()
        );
    } else {
        tracing::debug!(
            "Block #{}: replaying {} deploys, {} rejected",
            block.body.state.block_number,
            internal_deploys.len(),
            block.body.rejected_deploys.len()
        );
    }

    // Invalid-blocks map (hash -> sender) for the PoS slash deploys: derived from
    // this block's own recorded slash targets so it is byte-identical at block
    // creation and replay (see proto_util::slashed_block_senders). A DAG-derived
    // view is node-view-dependent and makes the slash deploy fail replay
    // (ConsumeFailed) because the map is produced into a content-addressed COMM.
    let slashed_hashes: Vec<BlockHash> = block
        .body
        .system_deploys
        .iter()
        .filter_map(|psd| match psd {
            ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::Slash {
                        invalid_block_hash, ..
                    },
                ..
            } => Some(invalid_block_hash.clone()),
            _ => None,
        })
        .collect();
    let invalid_blocks: HashMap<BlockHash, Validator> =
        proto_util::slashed_block_senders(dag, &slashed_hashes)?;

    // Create block data and check if genesis
    let block_data = BlockData::from_block(block);
    let is_genesis = block.header.parents_hash_list.is_empty();

    // Implement retry logic with limit of 3 retries
    let mut attempts = 0;
    const MAX_RETRIES: usize = 3;

    loop {
        // Call the async replay_compute_state method
        let replay_result = runtime_manager
            .replay_compute_state(
                &initial_state_hash,
                internal_deploys.clone(),
                internal_system_deploys.clone(),
                &block_data,
                Some(invalid_blocks.clone()),
                is_genesis,
            )
            .await;

        match replay_result {
            Ok(computed_state_hash) => {
                // Check if computed hash matches expected hash
                if computed_state_hash == block.body.state.post_state_hash {
                    // Success - hashes match
                    return Ok(Either::Right(computed_state_hash));
                } else if attempts >= MAX_RETRIES {
                    // Give up after max retries
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        expected = %PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
                        computed = %PrettyPrinter::build_string_no_limit(&computed_state_hash),
                        attempts,
                        "replay tuple space mismatch: giving up after max retries"
                    );
                    return Ok(Either::Right(computed_state_hash));
                } else {
                    // Retry - log error and continue
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        expected = %PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
                        computed = %PrettyPrinter::build_string_no_limit(&computed_state_hash),
                        attempt = attempts + 1,
                        "replay tuple space mismatch: retrying"
                    );
                    attempts += 1;
                }
            }
            Err(replay_error) => {
                if attempts >= MAX_RETRIES {
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        error = ?replay_error,
                        attempts,
                        "replay failed: giving up after max retries"
                    );
                    return Ok(Either::Left(match replay_error {
                        CasperError::ReplayFailure(replay_failure) => replay_failure,
                        other => ReplayFailure::internal_error(other.to_string()),
                    }));
                } else {
                    // Retry - log error and continue
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        error = ?replay_error,
                        attempt = attempts + 1,
                        "replay failed: retrying"
                    );
                    attempts += 1;
                }
            }
        }
    }
}

fn handle_errors(
    ts_hash: StateHash,
    result: Either<ReplayFailure, StateHash>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    match result {
        Either::Left(replay_failure) => match replay_failure {
            ReplayFailure::InternalError { msg } => {
                let exception = CasperError::RuntimeError(format!(
                    "Internal errors encountered while processing deploy: {}",
                    msg
                ));
                Ok(Either::Left(BlockStatus::exception(exception)))
            }

            ReplayFailure::ReplayStatusMismatch {
                initial_failed,
                replay_failed,
            } => {
                tracing::warn!(
                    "Found replay status mismatch; replay failure is {} and orig failure is {}",
                    replay_failed,
                    initial_failed
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::UnusedCOMMEvent { msg } => {
                tracing::warn!("Found replay exception: {}", msg);
                Ok(Either::Right(None))
            }

            ReplayFailure::ReplayCostMismatch {
                initial_cost,
                replay_cost,
            } => {
                tracing::warn!(
                    "Found replay cost mismatch: initial deploy cost = {}, replay deploy cost = {}",
                    initial_cost,
                    replay_cost
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::SystemDeployErrorMismatch {
                play_error,
                replay_error,
            } => {
                tracing::warn!(
                        "Found system deploy error mismatch: initial deploy error message = {}, replay deploy error message = {}",
                        play_error, replay_error
                    );
                Ok(Either::Right(None))
            }
        },

        Either::Right(computed_state_hash) => {
            if ts_hash == computed_state_hash {
                // State hash in block matches computed hash!
                Ok(Either::Right(Some(computed_state_hash)))
            } else {
                // State hash in block does not match computed hash -- invalid!
                // return no state hash, do not update the state hash set
                tracing::warn!(
                    "Tuplespace hash {} does not match computed hash {}.",
                    PrettyPrinter::build_string_bytes(&ts_hash),
                    PrettyPrinter::build_string_bytes(&computed_state_hash)
                );
                Ok(Either::Right(None))
            }
        }
    }
}

pub fn print_deploy_errors(deploy_sig: &Bytes, errors: &[InterpreterError]) {
    let deploy_info = PrettyPrinter::build_string_sig(deploy_sig);
    let error_messages: String = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    tracing::warn!("Deploy ({}) errors: {}", deploy_info, error_messages);
}

/// The result of executing a block's deploys against its merged pre-state
/// — everything block packaging needs, named. The rejected records travel
/// exactly as the merge formed them: sig, carrier, and duplicate flag are
/// all consensus content, re-derived and compared by every validator.
pub struct DeploysCheckpoint {
    pub pre_state_hash: StateHash,
    pub post_state_hash: StateHash,
    pub deploys: Vec<ProcessedDeploy>,
    pub rejected_deploys: Vec<RejectedDeploy>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
    pub bonds: Vec<Bond>,
    // The merge's recorded state-construction facts, packaged into the
    // block body (sorted sigs; base empty where the header derives it)
    // and consensus-checked at validation beside the rejected records.
    pub applied_from_scope: Vec<Bytes>,
    pub merge_base: Option<BlockHash>,
}

pub async fn compute_deploys_checkpoint(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<Signed<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    floor_ctx: Option<&crate::rust::finality::floor_context::FloorContext>,
    local_validator: Option<&Validator>,
) -> Result<DeploysCheckpoint, CasperError> {
    let checkpoint_started = std::time::Instant::now();
    // Using tracing events for async - Span[F] equivalent from Scala
    tracing::debug!(target: "f1r3fly.casper.compute_deploys_checkpoint", "compute-deploys-checkpoint-started");
    tracing::debug!(target: "f1r3fly.casper.compute_deploys_checkpoint", n_deploys = deploys.len(), n_parents = parents.len(), "propose.compute_deploys_checkpoint ENTER (merge parents, then run deploys)");
    // Ensure parents are not empty
    if parents.is_empty() {
        return Err(CasperError::RuntimeError(
            "Parents must not be empty".to_string(),
        ));
    }

    // Compute parents post state
    let parents_started = std::time::Instant::now();
    // Propose: the floor is derived from the proposer's justification snapshot,
    // which is exactly what gets packaged into this block's header — so the floor
    // the proposer builds on equals the floor every validator re-derives.
    let latest_messages: BTreeMap<Validator, BlockHash> = s
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let computed_parents_info = compute_parents_post_state(
        block_store,
        parents,
        s,
        runtime_manager,
        &latest_messages,
        None,
        rejected_deploy_buffer,
        floor_ctx,
        local_validator,
    )
    .await?;
    let parents_ms = parents_started.elapsed().as_millis();
    let pre_state_hash = computed_parents_info.state.clone();
    let rejected_deploys: Vec<RejectedDeploy> = computed_parents_info.rejected_user.clone();
    // Sorted so the packaged field is deterministic — the equality check at
    // validation compares recomputed sets, but the block bytes must not
    // depend on HashSet iteration order.
    let applied_from_scope: Vec<Bytes> = {
        let mut sigs: Vec<Bytes> = computed_parents_info
            .applied_from_scope
            .iter()
            .cloned()
            .collect();
        sigs.sort();
        sigs
    };
    let merge_base = computed_parents_info.merge_base.clone();

    // Compute state and bonds using one spawned runtime
    let compute_state_started = std::time::Instant::now();
    let result = runtime_manager
        .compute_state_with_bonds(
            &pre_state_hash,
            deploys,
            system_deploys,
            block_data,
            Some(invalid_blocks),
        )
        .await?;
    let compute_state_ms = compute_state_started.elapsed().as_millis();

    let (post_state_hash, processed_deploys, processed_system_deploys, bonds) = result;
    tracing::debug!(
        target: "f1r3fly.casper.compute_deploys_checkpoint.timing",
        "compute_deploys_checkpoint timing: parents_post_state_ms={}, compute_state_ms={}, total_ms={}, processed_deploys={}, processed_system_deploys={}, rejected_deploys={}",
        parents_ms,
        compute_state_ms,
        checkpoint_started.elapsed().as_millis(),
        processed_deploys.len(),
        processed_system_deploys.len(),
        rejected_deploys.len()
    );

    Ok(DeploysCheckpoint {
        pre_state_hash,
        post_state_hash,
        deploys: processed_deploys,
        rejected_deploys,
        system_deploys: processed_system_deploys,
        bonds,
        applied_from_scope,
        merge_base,
    })
}

/// Ensure every block in the merge `scope` has its mergeable-channels entry
/// materialized before `dag_merger::merge` reads them through the (synchronous)
/// `block_index_f` closure. A block imported via LFS without replay — or rejected
/// locally and never replayed — lacks it, and `load_mergeable_channels`
/// hard-errors `KeyNotFound`, which made merge validity node-local (the same
/// block could finalize on nodes that replayed it and be rejected on nodes that
/// did not — forking the DAG). Recomputing the missing entries here (a
/// deterministic full replay) makes mergeable presence a function of block
/// content on every node. Healthy nodes pay only a cached-presence check.
pub(crate) async fn ensure_scope_mergeable_present(
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    dag: &KeyValueDagRepresentation,
    scope: &HashSet<BlockHash>,
) -> Result<(), CasperError> {
    for hash in scope {
        // Fast path: a cached BlockIndex implies load_mergeable_channels already
        // succeeded for this block, so its persistent entry is present.
        if runtime_manager.block_index_cache.contains_key(hash) {
            continue;
        }
        let block = block_store.get_unsafe(hash);
        if runtime_manager.has_mergeable_entry(&block)? {
            continue;
        }

        // The recompute replays the block; its invalid-blocks map must match the
        // one validation/creation used (slashed_block_senders — the block's own
        // slash targets) so the replay reproduces the block's post_state_hash and
        // the entry is stored under the correct key.
        let slashed_hashes: Vec<BlockHash> = block
            .body
            .system_deploys
            .iter()
            .filter_map(|psd| match psd {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy:
                        SystemDeployData::Slash {
                            invalid_block_hash, ..
                        },
                    ..
                } => Some(invalid_block_hash.clone()),
                _ => None,
            })
            .collect();
        let invalid_blocks: HashMap<BlockHash, Validator> =
            proto_util::slashed_block_senders(dag, &slashed_hashes)?;

        runtime_manager
            .ensure_mergeable_entry(&block, invalid_blocks)
            .await?;

        tracing::info!(
            target: "f1r3fly.casper.mergeable_recompute",
            block_hash = %hex::encode(&hash[..hash.len().min(8)]),
            seq_num = block.seq_num,
            "recomputed missing mergeable entry for merge-scope block",
        );
    }

    Ok(())
}

/// Cap on the finalized-floor distance Δ = num(maxParent) − num(floor) that a
/// single multi-parent merge may span. The DETERMINISTIC backstop: Δ is a pure
/// function of the block's frozen justifications, so every honest node computes
/// the same Δ. Beyond the cap, `compute_parents_post_state` returns `Err` (the
/// proposer parks; a validator deterministically rejects) instead of silently
/// substituting one parent's post-state and dropping the others' writes.
pub(crate) const MAX_FLOOR_DISTANCE_BLOCKS: i64 = 256;

/// Advisory cap on the merge scope `|visible_blocks|`. This is NOT node-
/// deterministic (branch width differs across each node's view), so it MUST NOT
/// gate admission — a divergent reject would fork. Recorded as a metric/warning
/// only; the deterministic Δ backstop above is what bounds the merge.
pub(crate) const MAX_PARENT_MERGE_SCOPE_BLOCKS: usize = 512;

/// The deterministic merge-scope backstop decision: refuse (`Err`) iff the floor
/// distance exceeds the cap. A pure function of Δ ONLY — the scope size never
/// gates (see `MAX_PARENT_MERGE_SCOPE_BLOCKS`). Extracted for direct unit testing
/// of the "reject on Δ only, never on scope" property.
pub(crate) fn merge_scope_backstop_exceeded(floor_distance: i64) -> bool {
    floor_distance > MAX_FLOOR_DISTANCE_BLOCKS
}

/// Compute the merged post-state from multiple parent blocks.
///
/// For exploratory deploy, pass `disable_late_block_filtering_override = Some(true)` to
/// always disable late block filtering (see full merged state).
/// For normal block creation, pass `None` to use the shard config value.
pub async fn compute_parents_post_state(
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    // The block's frozen justification snapshot (proposer's justifications at
    // propose, the block's recorded justifications at validate) — NEVER the live
    // DAG view. The finalized floor is derived from this so it is node-identical.
    latest_messages: &BTreeMap<Validator, BlockHash>,
    disable_late_block_filtering_override: Option<bool>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    // The caller's per-operation derivation context. When present, the
    // floor (and its post-state) come from it instead of a second
    // derivation, and the settled-sig probes share its memo. `None`
    // derives locally — same inputs, same result.
    floor_ctx: Option<&crate::rust::finality::floor_context::FloorContext>,
    // This node's validator identity for the OWNER-SCOPED buffer populate;
    // `None` buffers nothing.
    local_validator: Option<&Validator>,
) -> Result<super::runtime_manager::MergedPreState, CasperError> {
    use super::runtime_manager::MergedPreState;
    let total_started = std::time::Instant::now();

    // No entered span guard here: the function is async (it awaits floor
    // derivation), and an `.entered()` guard is not `Send` across an await.
    // The individual tracing events below carry their own targets.
    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "compute_parents_post_state.ENTER",
        n_parents = parents.len(),
        latest_messages = latest_messages.len(),
        "merge.step: enter compute_parents_post_state"
    );
    match parents.len() {
        // For genesis, use empty trie's root hash
        0 => {
            let state = RuntimeManager::empty_state_hash_fixed();
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=genesis, parents=0, total_ms={}",
                total_started.elapsed().as_millis()
            );
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.EXIT",
                path = "genesis",
                post_state = %hex::encode(&state[..8.min(state.len())]),
                "merge.step: exit compute_parents_post_state"
            );
            Ok(MergedPreState {
                state,
                rejected_user: Vec::new(),
                rejected_slashes: Vec::new(),
                applied_from_scope: HashSet::new(),
                merge_base: None,
            })
        }

        // For single parent, get its post state hash
        1 => {
            let parent = &parents[0];
            let state = proto_util::post_state_hash(parent);
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=single_parent, parents=1, total_ms={}",
                total_started.elapsed().as_millis()
            );
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.EXIT",
                path = "single_parent",
                post_state = %hex::encode(&state[..8.min(state.len())]),
                "merge.step: exit compute_parents_post_state"
            );
            Ok(MergedPreState {
                state,
                rejected_user: Vec::new(),
                rejected_slashes: Vec::new(),
                applied_from_scope: HashSet::new(),
                merge_base: None,
            })
        }

        // Multiple parents - we might want to take some data from the parent with the most stake,
        // e.g. bonds map, slashing deploys, bonding deploys.
        // such system deploys are not mergeable, so take them from one of the parents.
        _ => {
            let cache_lookup_started = std::time::Instant::now();
            // A parent that DAG-covers every other parent already carries their
            // effects in its post-state, deploys included — merging a block with
            // its own ancestors is degenerate (the merger assumes siblings), so
            // the covering parent short-circuits regardless of deploy content.
            // (Port note: 6981b37a's empty-deploys guard is deliberately NOT
            // taken — it re-exposed ancestor-merges on linear chains here.)
            for candidate in &parents {
                let covers_all = parents
                    .iter()
                    .filter(|p| p.block_hash != candidate.block_hash)
                    .all(|p| {
                        s.dag
                            .is_dag_ancestor(&p.block_hash, &candidate.block_hash)
                            .unwrap_or(false)
                    });
                if covers_all {
                    tracing::debug!(
                        target: "f1r3fly.casper.compute_parents_post_state.fast_path",
                        "compute_parents_post_state fast path: descendant parent {} covers all {} parents",
                        PrettyPrinter::build_string_bytes(&candidate.block_hash),
                        parents.len()
                    );
                    let state = proto_util::post_state_hash(candidate);
                    tracing::debug!(
                        target: "f1r3fly.casper.compute_parents_post_state.timing",
                        "compute_parents_post_state timing: path=descendant_fast_path, parents={}, cache_lookup_ms={}, total_ms={}",
                        parents.len(),
                        cache_lookup_started.elapsed().as_millis(),
                        total_started.elapsed().as_millis()
                    );
                    tracing::debug!(
                        target: "f1r3fly.merge.step",
                        step = "compute_parents_post_state.EXIT",
                        path = "descendant_fast_path",
                        post_state = %hex::encode(&state[..8.min(state.len())]),
                        "merge.step: exit compute_parents_post_state"
                    );
                    // The covering parent is the state parent. Multi-parent
                    // headers cannot derive which parent carried the state,
                    // so the fast path records it — the state-membership
                    // walk continues there instead of guessing.
                    return Ok(MergedPreState {
                        state,
                        rejected_user: Vec::new(),
                        rejected_slashes: Vec::new(),
                        applied_from_scope: HashSet::new(),
                        merge_base: Some(candidate.block_hash.clone()),
                    });
                }
            }

            let mut parent_hashes_for_key: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            parent_hashes_for_key.sort();
            let disable_late_block_filtering = disable_late_block_filtering_override
                .unwrap_or(s.on_chain_state.shard_conf.disable_late_block_filtering);
            let cache_key = super::runtime_manager::ParentsPostStateCacheKey {
                sorted_parent_hashes: parent_hashes_for_key,
                snapshot_lfb_hash: s.last_finalized_block.clone(),
                // BTreeMap iteration is key-ordered, so this is deterministic.
                sorted_latest_messages: latest_messages
                    .iter()
                    .map(|(v, h)| (v.clone(), h.clone()))
                    .collect(),
                disable_late_block_filtering,
                buffer_populated: rejected_deploy_buffer.is_some(),
            };
            if let Some(cached) = runtime_manager.get_cached_parents_post_state(&cache_key) {
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.cache",
                    "compute_parents_post_state cache hit: parents={}, rejected_deploys={}, rejected_slashes={}",
                    cache_key.sorted_parent_hashes.len(),
                    cached.rejected_user.len(),
                    cached.rejected_slashes.len()
                );
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.timing",
                    "compute_parents_post_state timing: path=cache_hit, parents={}, cache_lookup_ms={}, total_ms={}",
                    cache_key.sorted_parent_hashes.len(),
                    cache_lookup_started.elapsed().as_millis(),
                    total_started.elapsed().as_millis()
                );
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_parents_post_state.CACHE_SKIP",
                    path = "cache_hit",
                    post_state = %hex::encode(&cached.state[..8.min(cached.state.len())]),
                    n_rejected = cached.rejected_user.len(),
                    n_rejected_slash = cached.rejected_slashes.len(),
                    "merge.step: cache hit, merge skipped"
                );
                return Ok(cached);
            }
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

            // Function to get or compute BlockIndex for each parent block hash
            let block_index_f = |v: &BlockHash| -> Result<BlockIndex, CasperError> {
                // Try cache first
                if let Some(cached) = runtime_manager.block_index_cache.get(v) {
                    return Ok((*cached.value()).clone());
                }

                // Cache miss - compute the BlockIndex
                let b = block_store.get_unsafe(v);
                let pre_state = &b.body.state.pre_state_hash;
                let post_state = &b.body.state.post_state_hash;
                let sender = b.sender.clone();
                let seq_num = b.seq_num;

                let mergeable_chs =
                    runtime_manager.load_mergeable_channels(post_state, sender, seq_num)?;

                let block_index = crate::rust::merging::block_index::new(
                    &b.block_hash,
                    b.body.state.block_number,
                    &b.body.deploys,
                    &b.body.system_deploys,
                    &Blake2b256Hash::from_bytes_prost(pre_state),
                    &Blake2b256Hash::from_bytes_prost(post_state),
                    &runtime_manager.history_repo,
                    &mergeable_chs,
                )?;

                // Cache the result
                runtime_manager
                    .block_index_cache
                    .insert(v.clone(), block_index.clone());

                Ok(block_index)
            };

            // Compute scope: the unsealed band not represented by the finalized floor.
            //
            // H3: derive the floor BEFORE collecting the merge scope so the
            // ancestor walk can drop only blocks represented by the floor's
            // DAG past. A below-floor sibling can still be a direct parent, and
            // its effects are not in the floor's post-state.
            let parent_hashes: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            let max_parent_block_number = parents
                .iter()
                .map(|p| p.body.state.block_number)
                .max()
                .unwrap_or(0);

            // Node-deterministic finalized floor, derived from the block's frozen
            // justification snapshot — the cut the merge builds on. Replaces the
            // LCA base: the floor is finalization-aware and advance-only, so the
            // merged state is MONOTONE, where the LCA base let it churn
            // (path-dependent FS).
            let floor_derive_started = std::time::Instant::now();
            let (floor_hash, floor_state, floor_block_number) = match floor_ctx {
                Some(ctx) => (
                    ctx.floor.hash.clone(),
                    ctx.floor_state_hash(),
                    ctx.floor.block_number,
                ),
                None => {
                    let floor = crate::rust::finality::floor::finalized_floor(
                        &s.dag,
                        &parent_hashes,
                        latest_messages,
                        crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                            s.on_chain_state.shard_conf.fault_tolerance_threshold_ppm,
                        ),
                    )
                    .await?;
                    let floor_hash = floor.hash.clone();

                    // The floor block's committed post-state is FS(floor) — the merge base.
                    // The floor is an ancestor of the parents (which are stored), so this is
                    // present in normal operation; surface a clear error instead of panicking
                    // if the block store and DAG ever desynchronize.
                    let floor_block = block_store.get(&floor_hash)?.ok_or_else(|| {
                        CasperError::RuntimeError(format!(
                            "finalized-floor block {} not in block store (DAG/store desync)",
                            hex::encode(&floor_hash[..8.min(floor_hash.len())])
                        ))
                    })?;
                    let floor_state =
                        Blake2b256Hash::from_bytes_prost(&floor_block.body.state.post_state_hash);
                    (floor_hash, floor_state, floor_block.body.state.block_number)
                }
            };
            let floor_derive_ms = floor_derive_started.elapsed().as_millis();

            let include_visible_ancestor =
                |hash: &BlockHash, dag: &KeyValueDagRepresentation| -> bool {
                    block_in_floor_merge_scope(dag, hash, &floor_hash, floor_block_number)
                        .unwrap_or(false)
                };
            // Get all ancestors of all parents (including the parents themselves)
            // Use bounded traversal that stops once the floor's represented past is reached.
            let collect_ancestors_started = std::time::Instant::now();
            let mut visible_ancestor_sets_with_parents: Vec<HashSet<BlockHash>> = Vec::new();
            for parent_hash in &parent_hashes {
                let visible_ancestors = s.dag.with_ancestors(parent_hash.clone(), |bh| {
                    include_visible_ancestor(bh, &s.dag)
                })?;
                let mut visible_ancestors_with_parent = visible_ancestors;
                visible_ancestors_with_parent.insert(parent_hash.clone());
                visible_ancestor_sets_with_parents.push(visible_ancestors_with_parent);
            }
            let collect_ancestors_ms = collect_ancestors_started.elapsed().as_millis();

            // Flatten all ancestor sets to get visible blocks
            let flatten_visible_started = std::time::Instant::now();
            let mut visible_blocks: HashSet<BlockHash> = visible_ancestor_sets_with_parents
                .iter()
                .flat_map(|s| s.iter().cloned())
                .collect();
            let flatten_visible_ms = flatten_visible_started.elapsed().as_millis();

            // Scope visible_blocks to blocks not represented by the floor.
            let pre_filter_count = visible_blocks.len();
            let pre_filter_blocks: Option<Vec<BlockHash>> = if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG)
            {
                Some(visible_blocks.iter().cloned().collect())
            } else {
                None
            };
            visible_blocks.retain(|bh| {
                block_in_floor_merge_scope(&s.dag, bh, &floor_hash, floor_block_number)
                    .unwrap_or(true)
            });
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let dropped: Vec<String> = match &pre_filter_blocks {
                    Some(pre) => pre
                        .iter()
                        .filter(|bh| !visible_blocks.contains(*bh))
                        .map(|bh| hex::encode(&bh[..8.min(bh.len())]))
                        .collect(),
                    None => Vec::new(),
                };
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_parents_post_state.FLOOR",
                    floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                    floor_block = floor_block_number,
                    base_state = %hex::encode(&floor_state.bytes()[..8]),
                    scope_before = pre_filter_count,
                    scope_after = visible_blocks.len(),
                    n_dropped = dropped.len(),
                    dropped = ?dropped,
                    n_parents = parents.len(),
                    "merge.step: floor derived; base=floor.post_state; scope filtered to >= floor block"
                );
            }
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]), floor_block = floor_block_number, base_state = %hex::encode(&floor_state.bytes()[..8]), scope_blocks = visible_blocks.len(), n_parents = parents.len(), "merge.compute_parents_post_state: floor+base+scope computed");
            if visible_blocks.len() < pre_filter_count {
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state",
                    "floor-scoped merge: reduced visible_blocks from {} to {} (floor at block #{})",
                    pre_filter_count,
                    visible_blocks.len(),
                    floor_block_number,
                );
            }

            if tracing::enabled!(tracing::Level::DEBUG) {
                let parent_hash_str: Vec<String> = parent_hashes
                    .iter()
                    .map(|h| hex::encode(&h[..std::cmp::min(10, h.len())]))
                    .collect();
                tracing::debug!(
                    "computeParentsPostState: parents=[{}], floor={} (block {}), visibleBlocks={}",
                    parent_hash_str.join(", "),
                    hex::encode(&floor_hash[..std::cmp::min(10, floor_hash.len())]),
                    floor_block_number,
                    visible_blocks.len()
                );
            }

            let floor_distance = max_parent_block_number - floor_block_number;
            let visible_blocks_len = visible_blocks.len();
            // Observability: the (node-deterministic) floor distance Δ and the
            // (NOT node-deterministic) scope size. Only Δ gates admission.
            metrics::histogram!(
                crate::rust::metrics_constants::FLOOR_DISTANCE_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(floor_distance as f64);
            metrics::histogram!(
                crate::rust::metrics_constants::MERGE_SCOPE_SIZE_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(visible_blocks_len as f64);
            if visible_blocks_len > MAX_PARENT_MERGE_SCOPE_BLOCKS {
                // The scope size is NOT node-deterministic (branch width differs
                // across each node's view), so it must NEVER gate admission — a
                // reject that differs across nodes would fork. Log as an anomaly
                // only; the deterministic floor-distance backstop below is what
                // actually bounds the merge.
                tracing::warn!(
                    target: "f1r3fly.casper.compute_parents_post_state",
                    visible_blocks = visible_blocks_len,
                    floor_distance,
                    max_scope = MAX_PARENT_MERGE_SCOPE_BLOCKS,
                    "merge scope unusually large; NOT rejecting (scope size is not node-deterministic) — watch floor distance"
                );
            }
            // Deterministic backstop: the floor distance Δ is a pure function of
            // the block's frozen justifications, so every honest node computes the
            // same Δ. When Δ exceeds the cap we REFUSE to build a merge rather than
            // silently substituting the single highest parent's post-state — the
            // former behaviour dropped every other parent's committed writes and
            // could strand a dropped co-parent's deploys as non-re-proposable
            // (the ~400-block "fails under load" bug: S5/¬T-K1/¬T-NDA). On propose
            // this `Err` parks the round (retried once finality advances and Δ
            // shrinks); on validate an over-Δ block is deterministically invalid —
            // both sides compute the same Δ, so there is no fork.
            if merge_scope_backstop_exceeded(floor_distance) {
                metrics::counter!(
                    crate::rust::metrics_constants::MERGE_SCOPE_BACKSTOP_ERROR_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::warn!(
                    target: "f1r3fly.casper.compute_parents_post_state.backstop",
                    floor_distance,
                    max_floor_distance = MAX_FLOOR_DISTANCE_BLOCKS,
                    visible_blocks = visible_blocks_len,
                    floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                    floor_block = floor_block_number,
                    n_parents = parents.len(),
                    "compute_parents_post_state backstop: floor distance exceeds cap; refusing lossy merge"
                );
                return Err(CasperError::RuntimeError(format!(
                    "finalized-floor merge backstop: floor distance {} exceeds cap {} (parents={}, floor block #{}); \
                     refusing to build a lossy single-parent merge — finality must advance first",
                    floor_distance,
                    MAX_FLOOR_DISTANCE_BLOCKS,
                    parents.len(),
                    floor_block_number,
                )));
            }

            // Every scope block's mergeable entry must be loadable before the
            // merge builds indices; recompute any this node never replayed
            // (otherwise a missing entry makes the merge fail node-locally → fork).
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.ENSURE_MERGEABLE_PRE",
                scope_blocks = visible_blocks_len,
                "merge.step: ensure_scope_mergeable_present begin"
            );
            ensure_scope_mergeable_present(block_store, runtime_manager, &s.dag, &visible_blocks)
                .await?;
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.ENSURE_MERGEABLE_POST",
                scope_blocks = visible_blocks_len,
                "merge.step: ensure_scope_mergeable_present done"
            );

            // Use DagMerger to merge parent states with scope
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.MERGE_PRE",
                floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                base_state = %hex::encode(&floor_state.bytes()[..8.min(floor_state.bytes().len())]),
                scope_blocks = visible_blocks_len,
                disable_late_block_filtering,
                "merge.step: dag_merger::merge begin"
            );
            let merge_started = std::time::Instant::now();
            // Settled-sig probe for the merge dedup: a sig whose effect is
            // already in the base's committed state has no legitimate scope
            // copy — every one is a stale duplicate no rejection record
            // covers. Memoization lives inside the merge (one probe per
            // unique sig per merge call).
            let sig_settled_in_base = |sig: &Bytes| -> Result<bool, CasperError> {
                match floor_ctx {
                    Some(ctx) => {
                        ctx.effect_settled_in_floor(&s.dag, block_store, runtime_manager, sig)
                    }
                    None => deploy_effect_in_state(
                        &s.dag,
                        block_store,
                        runtime_manager,
                        &floor_state,
                        sig,
                    ),
                }
            };
            let merger_result = dag_merger::merge(
                &s.dag,
                &floor_hash,
                &floor_state,
                |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
                    let block_index = block_index_f(hash)?;
                    Ok(block_index.deploy_chains)
                },
                &runtime_manager.history_repo,
                dag_merger::cost_optimal_rejection_alg(),
                Some(visible_blocks.clone()),
                disable_late_block_filtering,
                floor_block_number,
                s.on_chain_state.shard_conf.deploy_lifespan,
                &sig_settled_in_base,
            )?;
            let merge_ms = merge_started.elapsed().as_millis();

            let (state, mut rejected_user_records, rejected_slash_pairs, applied_user_sigs) =
                merger_result;
            let suppressed = suppress_already_recorded_rejections(
                block_store,
                &visible_blocks,
                &mut rejected_user_records,
            )?;
            if suppressed > 0 {
                tracing::debug!(
                    target: "f1r3fly.casper.recovery",
                    "compute_parents_post_state: suppressed {} already-recorded rejection(s)",
                    suppressed
                );
            }
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.MERGE_POST",
                new_state = %hex::encode(&state.bytes()[..8.min(state.bytes().len())]),
                n_rejected_user = rejected_user_records.len(),
                n_rejected_slash = rejected_slash_pairs.len(),
                merge_ms,
                "merge.step: dag_merger::merge returned"
            );
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", merged_state = %hex::encode(&state.bytes()[..8.min(state.bytes().len())]), n_rejected_user = rejected_user_records.len(), n_rejected_slash = rejected_slash_pairs.len(), merge_ms, "merge.compute_parents_post_state: DagMerger produced merged state");

            // Populate the rejected-deploy buffer from the records' (sig,
            // carrier) naming. Looking up the `Signed<DeployData>` from the
            // carrier block lets the block creator re-propose these deploys
            // in a subsequent block. Fetching each carrier at most once
            // keeps the cost proportional to the number of distinct
            // carriers. Duplicate-flagged records buffer nothing: their
            // copy's effect already stands, so a re-proposal could only be
            // executed onto a state that carries it.
            if let Some(buffer) = rejected_deploy_buffer {
                if !rejected_user_records.is_empty() {
                    // Bounded to the deploy-lifespan window below the floor:
                    // a win deeper than that belongs to a deploy whose
                    // validity window is closed at this floor, and the
                    // buffer retain drops window-closed deploys regardless —
                    // so the unbounded walk to genesis bought nothing.
                    let floor_won = canonical_won_sigs(
                        block_store,
                        std::slice::from_ref(&floor_hash),
                        floor_block_number - s.on_chain_state.shard_conf.deploy_lifespan,
                    )?;
                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for record in &rejected_user_records {
                        let (sig, src_block) = (&record.sig, &record.carrier);
                        if record.duplicate {
                            tracing::debug!(
                                target: "f1r3fly.casper.recovery",
                                "RejectedDeployBuffer populate: skipped duplicate-flagged sig {} from {}",
                                PrettyPrinter::build_string_bytes(sig),
                                PrettyPrinter::build_string_bytes(src_block)
                            );
                            continue;
                        }
                        if floor_won.contains(sig)
                            || rejected_sig_has_visible_non_source_win(
                                block_store,
                                &visible_blocks,
                                sig,
                                src_block,
                            )?
                        {
                            tracing::debug!(
                                target: "f1r3fly.casper.recovery",
                                "RejectedDeployBuffer populate: skipped already-won sig {} from {}",
                                PrettyPrinter::build_string_bytes(sig),
                                PrettyPrinter::build_string_bytes(src_block)
                            );
                            continue;
                        }
                        by_block
                            .entry(src_block.clone())
                            .or_default()
                            .push(sig.clone());
                    }
                    let mut deploys_to_buffer: Vec<Signed<DeployData>> = Vec::new();
                    for (src_block, sigs) in by_block {
                        let sig_set: HashSet<Bytes> = sigs.into_iter().collect();
                        match block_store.get(&src_block) {
                            Ok(Some(block)) => {
                                // Owner-only recovery custody: the buffer
                                // belongs to the validator that sent the
                                // rejected copy's carrier — the deploy's
                                // single holder, matching the one-validator
                                // deploy model. Everyone else drops the pair
                                // (the deploy body stays on-chain in the
                                // carrier; nothing is lost), which
                                // structurally eliminates same-sig retry
                                // fan-out. `None` (observers, exploratory
                                // contexts) buffers nothing.
                                let owned = local_validator.is_some_and(|me| block.sender == *me);
                                if !owned {
                                    tracing::debug!(
                                        target: "f1r3fly.casper.recovery",
                                        "RejectedDeployBuffer populate: not the owner of carrier {}; dropping {} pair(s)",
                                        PrettyPrinter::build_string_bytes(&src_block),
                                        sig_set.len()
                                    );
                                    continue;
                                }
                                for pd in &block.body.deploys {
                                    if sig_set.contains(&pd.deploy.sig) {
                                        deploys_to_buffer.push(pd.deploy.clone());
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer populate: source block {} not in store",
                                    PrettyPrinter::build_string_bytes(&src_block)
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer populate: failed to load {}: {}",
                                    PrettyPrinter::build_string_bytes(&src_block),
                                    err
                                );
                            }
                        }
                    }
                    let skipped = retain_recoverable_rejected_deploys_for_buffer(
                        floor_block_number,
                        s.on_chain_state.shard_conf.deploy_lifespan,
                        &mut deploys_to_buffer,
                    );
                    if skipped > 0 {
                        tracing::info!(
                            target: "f1r3fly.casper.recovery",
                            "RejectedDeployBuffer populate: skipped {} window-closed deploy(s)",
                            skipped
                        );
                    }
                    if !deploys_to_buffer.is_empty() {
                        match buffer.lock() {
                            Ok(mut guard) => {
                                if let Err(err) = guard.add(deploys_to_buffer) {
                                    tracing::warn!("RejectedDeployBuffer add failed: {}", err);
                                }
                            }
                            Err(_) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer lock poisoned; skipping populate"
                                );
                            }
                        }
                    }
                }
            }

            // Recover rejected-slash metadata by reading each source block's
            // system_deploys once. The block creator uses these to dedup
            // slashes into the merge block's body; without this the slash
            // effect would be lost to cost-optimal rejection.
            let rejected_slashes: Vec<crate::rust::merging::rejected_slash::RejectedSlash> =
                if rejected_slash_pairs.is_empty() {
                    Vec::new()
                } else {
                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for (sig, src_block) in &rejected_slash_pairs {
                        by_block
                            .entry(src_block.clone())
                            .or_default()
                            .push(sig.clone());
                    }
                    let mut out = Vec::new();
                    for (src_block, _sigs) in by_block {
                        match block_store.get(&src_block) {
                            Ok(Some(block)) => {
                                for psd in &block.body.system_deploys {
                                    if let models::rust::casper::protocol::casper_message::ProcessedSystemDeploy::Succeeded {
                                    system_deploy:
                                        models::rust::casper::protocol::casper_message::SystemDeployData::Slash {
                                            invalid_block_hash,
                                            issuer_public_key,
                                            target_activation_epoch: _,
                                        },
                                    ..
                                } = psd
                                {
                                    out.push(
                                        crate::rust::merging::rejected_slash::RejectedSlash {
                                            invalid_block_hash: invalid_block_hash.clone(),
                                            issuer_public_key: issuer_public_key.clone(),
                                            source_block_hash: src_block.clone(),
                                        },
                                    );
                                }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "RejectedSlash extract: source block {} not in store",
                                    PrettyPrinter::build_string_bytes(&src_block)
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "RejectedSlash extract: failed to load {}: {}",
                                    PrettyPrinter::build_string_bytes(&src_block),
                                    err
                                );
                            }
                        }
                    }
                    out
                };

            let computed_state = prost::bytes::Bytes::copy_from_slice(&state.bytes());
            let merged = MergedPreState {
                state: computed_state.clone(),
                rejected_user: rejected_user_records,
                rejected_slashes,
                applied_from_scope: applied_user_sigs,
                merge_base: Some(floor_hash.clone()),
            };
            // The floor is a deterministic function of the block's justifications,
            // so the merged state is always cacheable (no snapshot-LFB fallback).
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.CACHE_PUT",
                path = "merged",
                post_state = %hex::encode(&computed_state[..8.min(computed_state.len())]),
                n_rejected = merged.rejected_user.len(),
                n_rejected_slash = merged.rejected_slashes.len(),
                n_applied = merged.applied_from_scope.len(),
                "merge.step: cache put merged parents-post-state"
            );
            runtime_manager.put_cached_parents_post_state(cache_key, merged.clone());
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=merged, parents={}, cache_lookup_ms={}, collect_ancestors_ms={}, flatten_visible_ms={}, floor_derive_ms={}, merge_ms={}, visible_blocks={}, rejected_deploys={}, rejected_slashes={}, total_ms={}",
                parents.len(),
                cache_lookup_ms,
                collect_ancestors_ms,
                flatten_visible_ms,
                floor_derive_ms,
                merge_ms,
                visible_blocks_len,
                merged.rejected_user.len(),
                merged.rejected_slashes.len(),
                total_started.elapsed().as_millis()
            );
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.EXIT",
                path = "merged",
                post_state = %hex::encode(&computed_state[..8.min(computed_state.len())]),
                n_rejected = merged.rejected_user.len(),
                n_rejected_slash = merged.rejected_slashes.len(),
                "merge.step: exit compute_parents_post_state"
            );
            Ok(merged)
        }
    }
}

#[cfg(test)]
mod backstop_tests {
    use std::collections::HashSet;

    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, ProcessedDeploy, RejectedDeploy,
    };
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{
        block_in_floor_merge_scope, canonical_dispositions, canonical_won_sigs,
        merge_scope_backstop_exceeded, rejected_sig_has_visible_non_source_win,
        retain_recoverable_rejected_deploys_for_buffer, suppress_already_recorded_rejections,
        MAX_FLOOR_DISTANCE_BLOCKS,
    };
    use crate::rust::util::construct_deploy;

    #[test]
    fn backstop_rejects_only_on_floor_distance_over_cap() {
        // At or below the cap: proceed (no backstop, build the merge).
        assert!(!merge_scope_backstop_exceeded(0));
        assert!(!merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS));
        // Strictly above the cap: refuse — deterministic Err (propose parks;
        // validate rejects), never a silent lossy single-parent substitution.
        assert!(merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS + 1));
        assert!(merge_scope_backstop_exceeded(i64::MAX));
    }

    #[test]
    fn backstop_decision_depends_on_floor_distance_only() {
        // The backstop is a pure function of the (node-deterministic) floor
        // distance Δ. The scope size |visible_blocks| is NOT a parameter — it
        // cannot influence the decision by construction, which is exactly the
        // anti-fork property (scope size is not node-deterministic, so gating on
        // it could reject differently across nodes and fork). This test pins that
        // the signature carries Δ only.
        let below = merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS);
        let above = merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS + 1);
        assert!(!below && above);
    }

    /// The walk collects three facts per sig in ONE pass; each has its own
    /// determinism contract. First carrier: LOWEST block, same-height tie to
    /// the lexicographically SMALLER hash; its sender is the recovery owner.
    /// Latest disposition: highest block, rejection wins a same-height tie.
    #[tokio::test]
    async fn disposition_walk_first_carrier_is_lowest_block_smaller_hash_tie() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@7!(7)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let secp = Secp256k1;
        let (_, s_root) = secp.new_key_pair();
        let (_, s_b) = secp.new_key_pair();
        let (_, s_c) = secp.new_key_pair();

        let root = disposition_test_block(1, s_root.bytes.clone(), Vec::new(), Vec::new());
        // Two same-height sibling carriers with distinct senders, then a
        // higher inclusion — the walk must pick the smaller-hash sibling
        // and ignore the later copy for first-carrier purposes.
        let carrier_b =
            disposition_test_block(2, s_b.bytes.clone(), vec![root.block_hash.clone()], vec![
                ProcessedDeploy::empty(deploy.clone()),
            ]);
        let carrier_c =
            disposition_test_block(2, s_c.bytes.clone(), vec![root.block_hash.clone()], vec![
                ProcessedDeploy::empty(deploy.clone()),
            ]);
        let tip = disposition_test_block(
            3,
            s_root.bytes.clone(),
            vec![carrier_b.block_hash.clone(), carrier_c.block_hash.clone()],
            vec![ProcessedDeploy::empty(deploy)],
        );
        for block in [&root, &carrier_b, &carrier_c, &tip] {
            block_store.put_block_message(block).expect("store block");
        }

        let dispositions =
            canonical_dispositions(&block_store, &[tip.block_hash.clone()], i64::MIN)
                .expect("walk");
        let disposition = dispositions.get(&sig).expect("sig disposition");

        assert_eq!(disposition.latest, (3, true), "latest is the tip inclusion");
        let (expected_hash, expected_sender) = if carrier_b.block_hash <= carrier_c.block_hash {
            (carrier_b.block_hash.clone(), s_b.bytes.clone())
        } else {
            (carrier_c.block_hash.clone(), s_c.bytes.clone())
        };
        assert_eq!(
            disposition.first_carrier,
            Some((2, expected_hash, expected_sender)),
            "first carrier is the lowest inclusion; the same-height tie \
             breaks to the smaller hash, and its sender is the owner",
        );
    }

    /// Latest kept rejection: HIGHEST record-bearing block, same-height tie
    /// to the lexicographically LARGER hash — the adjudication the retry
    /// gate settles against must be node-identical.
    #[tokio::test]
    async fn disposition_walk_latest_kept_rejection_is_highest_block_larger_hash_tie() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@8!(8)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let secp = Secp256k1;
        let (_, s_root) = secp.new_key_pair();

        let carrier = disposition_test_block(1, s_root.bytes.clone(), Vec::new(), vec![
            ProcessedDeploy::empty(deploy),
        ]);
        let mut record_d = disposition_test_block(
            2,
            s_root.bytes.clone(),
            vec![carrier.block_hash.clone()],
            Vec::new(),
        );
        record_d.body.rejected_deploys = vec![RejectedDeploy {
            sig: sig.clone(),
            duplicate: false,
            carrier: carrier.block_hash.clone(),
        }];
        let mut record_e = disposition_test_block(
            2,
            s_root.bytes.clone(),
            vec![carrier.block_hash.clone()],
            Vec::new(),
        );
        record_e.body.rejected_deploys = vec![RejectedDeploy {
            sig: sig.clone(),
            duplicate: false,
            carrier: carrier.block_hash.clone(),
        }];
        let tip = disposition_test_block(
            3,
            s_root.bytes.clone(),
            vec![record_d.block_hash.clone(), record_e.block_hash.clone()],
            Vec::new(),
        );
        for block in [&carrier, &record_d, &record_e, &tip] {
            block_store.put_block_message(block).expect("store block");
        }

        let dispositions =
            canonical_dispositions(&block_store, &[tip.block_hash.clone()], i64::MIN)
                .expect("walk");
        let disposition = dispositions.get(&sig).expect("sig disposition");

        assert_eq!(
            disposition.latest,
            (2, false),
            "the height-2 rejection outranks the height-1 inclusion",
        );
        let expected_hash = std::cmp::max(record_d.block_hash.clone(), record_e.block_hash.clone());
        assert_eq!(
            disposition.latest_kept_rejection,
            Some((2, expected_hash)),
            "the latest kept rejection is the highest record-bearing block; \
             the same-height tie breaks to the larger hash",
        );
        assert_eq!(
            disposition
                .first_carrier
                .as_ref()
                .map(|(h, b, _)| (*h, b.clone())),
            Some((1, carrier.block_hash.clone())),
            "the height-1 inclusion is the first carrier",
        );
    }

    fn disposition_test_block(
        block_number: i64,
        sender: Bytes,
        parents: Vec<BlockHash>,
        deploys: Vec<ProcessedDeploy>,
    ) -> BlockMessage {
        block_implicits::get_random_block(
            Some(block_number),
            Some(1),
            None,
            None,
            Some(sender),
            Some(1),
            Some(0),
            Some(parents),
            Some(Vec::new()),
            Some(deploys),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        )
    }

    fn scope_test_block(
        block_number: i64,
        seq_num: i32,
        sender: Bytes,
        parents: Vec<BlockHash>,
    ) -> BlockMessage {
        block_implicits::get_random_block(
            Some(block_number),
            Some(seq_num),
            None,
            None,
            Some(sender),
            Some(1),
            Some(0),
            Some(parents),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        )
    }

    #[tokio::test]
    async fn floor_scope_drops_finalized_off_main_dag_ancestors() {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let secp = Secp256k1;
        let (_, v1_pk) = secp.new_key_pair();
        let (_, v2_pk) = secp.new_key_pair();
        let (_, v3_pk) = secp.new_key_pair();
        let v1 = v1_pk.bytes;
        let v2 = v2_pk.bytes;
        let v3 = v3_pk.bytes;
        let genesis = scope_test_block(0, 0, v1.clone(), Vec::new());
        let main = scope_test_block(1, 1, v1.clone(), vec![genesis.block_hash.clone()]);
        let off_main = scope_test_block(1, 1, v2.clone(), vec![genesis.block_hash.clone()]);
        let floor = scope_test_block(2, 2, v1.clone(), vec![
            main.block_hash.clone(),
            off_main.block_hash.clone(),
        ]);
        let outside = scope_test_block(1, 1, v3.clone(), vec![genesis.block_hash.clone()]);
        let outside_same_height =
            scope_test_block(2, 2, v3.clone(), vec![outside.block_hash.clone()]);
        let descendant = scope_test_block(3, 3, v1, vec![floor.block_hash.clone()]);

        for (block, mode) in [
            (&genesis, InsertMode::Approved),
            (&main, InsertMode::Normal),
            (&off_main, InsertMode::Normal),
            (&floor, InsertMode::Normal),
            (&outside, InsertMode::Normal),
            (&outside_same_height, InsertMode::Normal),
            (&descendant, InsertMode::Normal),
        ] {
            dag_storage.insert(block, mode).expect("insert block");
        }
        dag_storage
            .record_directly_finalized(floor.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record floor finalized");
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        let floor_block_number = dag
            .lookup_unsafe(&floor.block_hash)
            .expect("floor metadata")
            .block_number;

        assert!(dag.is_finalized(&off_main.block_hash));
        assert!(!dag
            .is_in_main_chain(&off_main.block_hash, &floor.block_hash)
            .expect("main-chain query"));
        assert!(dag
            .is_dag_ancestor(&off_main.block_hash, &floor.block_hash)
            .expect("dag ancestor query"));

        let mut visible_blocks: HashSet<_> = [
            genesis.block_hash.clone(),
            main.block_hash.clone(),
            off_main.block_hash.clone(),
            floor.block_hash.clone(),
            outside.block_hash.clone(),
            outside_same_height.block_hash.clone(),
            descendant.block_hash.clone(),
        ]
        .into_iter()
        .collect();
        visible_blocks.retain(|hash| {
            block_in_floor_merge_scope(&dag, hash, &floor.block_hash, floor_block_number)
                .expect("scope predicate")
        });

        assert!(!visible_blocks.contains(&genesis.block_hash));
        assert!(!visible_blocks.contains(&main.block_hash));
        assert!(!visible_blocks.contains(&off_main.block_hash));
        assert!(!visible_blocks.contains(&floor.block_hash));
        assert!(visible_blocks.contains(&outside.block_hash));
        assert!(visible_blocks.contains(&outside_same_height.block_hash));
        assert!(visible_blocks.contains(&descendant.block_hash));
    }

    #[tokio::test]
    async fn visible_non_source_win_in_lower_block_still_blocks_recovery_admission() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@9!(9)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let lower_clean = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let higher_source = block_implicits::get_random_block(
            Some(23),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&lower_clean)
            .expect("store lower clean");
        block_store
            .put_block_message(&higher_source)
            .expect("store source");

        let visible_blocks: HashSet<_> = [
            lower_clean.block_hash.clone(),
            higher_source.block_hash.clone(),
        ]
        .into_iter()
        .collect();

        assert!(rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &sig,
            &higher_source.block_hash,
        )
        .expect("visible win check"));
    }

    #[tokio::test]
    async fn later_visible_non_source_rejection_reopens_recovery_admission() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@9!(9)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let lower_clean = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let higher_source = block_implicits::get_random_block(
            Some(23),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let mut later_rejection = block_implicits::get_random_block(
            Some(24),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        later_rejection.body.rejected_deploys = vec![RejectedDeploy {
            sig: sig.clone(),
            duplicate: false,
            carrier: Bytes::new(),
        }];
        block_store
            .put_block_message(&lower_clean)
            .expect("store lower clean");
        block_store
            .put_block_message(&higher_source)
            .expect("store source");
        block_store
            .put_block_message(&later_rejection)
            .expect("store later rejection");

        let visible_blocks: HashSet<_> = [
            lower_clean.block_hash.clone(),
            higher_source.block_hash.clone(),
            later_rejection.block_hash.clone(),
        ]
        .into_iter()
        .collect();

        assert!(!rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &sig,
            &higher_source.block_hash,
        )
        .expect("visible win check"));
    }

    #[tokio::test]
    async fn floor_rejection_is_not_canonical_win() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_sig = Bytes::from_static(b"floor-rejected");
        let deploy = construct_deploy::source_deploy_now_full(
            "@7!(7)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let mut rejected_floor = block_implicits::get_random_block(
            Some(10),
            Some(1),
            None,
            None,
            None,
            None,
            Some(10),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        rejected_floor.body.rejected_deploys = vec![RejectedDeploy {
            sig: rejected_sig.clone(),
            duplicate: false,
            carrier: Bytes::new(),
        }];
        let clean_floor = block_implicits::get_random_block(
            Some(11),
            Some(1),
            None,
            None,
            None,
            None,
            Some(11),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&rejected_floor)
            .expect("store rejected floor");
        block_store
            .put_block_message(&clean_floor)
            .expect("store clean floor");

        let rejected_floor_wins = canonical_won_sigs(
            &block_store,
            std::slice::from_ref(&rejected_floor.block_hash),
            i64::MIN,
        )
        .expect("canonical wins for rejected floor");
        let clean_floor_wins = canonical_won_sigs(
            &block_store,
            std::slice::from_ref(&clean_floor.block_hash),
            i64::MIN,
        )
        .expect("canonical wins for clean floor");

        assert!(!rejected_floor_wins.contains(&rejected_sig));
        assert!(clean_floor_wins.contains(&deploy.sig));
    }

    /// Emission dedup is exact: only a visible record IDENTICAL in sig,
    /// carrier, and flag makes re-emission redundant. A visible record of
    /// the same sig naming a different carrier is testimony about a
    /// different copy and suppresses nothing.
    #[tokio::test]
    async fn only_the_identical_visible_record_suppresses_re_emission() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let recovered_sig = Bytes::from_static(b"recovered");
        let duplicate_sig = Bytes::from_static(b"duplicate");
        let source = block_implicits::get_random_block(
            Some(20),
            Some(1),
            None,
            None,
            None,
            None,
            Some(20),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        // A visible record of `recovered_sig` naming a DIFFERENT carrier:
        // it adjudicated a sibling copy, not the one at `source`.
        let mut sibling_record_block = block_implicits::get_random_block(
            Some(10),
            Some(1),
            None,
            None,
            None,
            None,
            Some(10),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        sibling_record_block.body.rejected_deploys = vec![RejectedDeploy {
            sig: recovered_sig.clone(),
            duplicate: false,
            carrier: Bytes::from(vec![0xD7; 32]),
        }];
        // A visible record IDENTICAL to the computed one for
        // `duplicate_sig`: same sig, same carrier, same flag.
        let mut identical_record_block = block_implicits::get_random_block(
            Some(21),
            Some(1),
            None,
            None,
            None,
            None,
            Some(21),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        identical_record_block.body.rejected_deploys = vec![RejectedDeploy {
            sig: duplicate_sig.clone(),
            duplicate: false,
            carrier: source.block_hash.clone(),
        }];
        block_store
            .put_block_message(&sibling_record_block)
            .expect("store sibling record");
        block_store
            .put_block_message(&source)
            .expect("store source");
        block_store
            .put_block_message(&identical_record_block)
            .expect("store identical record");

        let visible_blocks: HashSet<_> = [
            sibling_record_block.block_hash.clone(),
            source.block_hash.clone(),
            identical_record_block.block_hash.clone(),
        ]
        .into_iter()
        .collect();
        let retained_record = RejectedDeploy {
            sig: recovered_sig.clone(),
            duplicate: false,
            carrier: source.block_hash.clone(),
        };
        let mut rejected_records = vec![retained_record.clone(), RejectedDeploy {
            sig: duplicate_sig.clone(),
            duplicate: false,
            carrier: source.block_hash.clone(),
        }];

        let suppressed = suppress_already_recorded_rejections(
            &block_store,
            &visible_blocks,
            &mut rejected_records,
        )
        .expect("suppress rejected records");

        assert_eq!(suppressed, 1);
        assert_eq!(rejected_records, vec![retained_record]);
    }

    /// A record adjudicates the copy its carrier names, and ONLY that copy.
    /// A visible record naming a SIBLING carrier of the same sig covers
    /// nothing about this carrier's copy — suppressing this pair on it
    /// leaves the sibling's adjudication permanently unrecorded: the copy
    /// reads as a standing win, recovery stands down, and the work is lost
    /// while the record plane looks complete.
    #[tokio::test]
    async fn record_naming_a_sibling_carrier_does_not_suppress_this_carriers_pair() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@11!(11)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();

        // This carrier: holds the copy the computed pair adjudicates.
        let this_carrier = block_implicits::get_random_block(
            Some(20),
            Some(1),
            None,
            None,
            None,
            None,
            Some(20),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        // A later visible record of the SAME sig naming a DIFFERENT carrier
        // (a sibling copy adjudicated elsewhere).
        let mut sibling_record_block = block_implicits::get_random_block(
            Some(21),
            Some(1),
            None,
            None,
            None,
            None,
            Some(21),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        sibling_record_block.body.rejected_deploys = vec![RejectedDeploy {
            sig: sig.clone(),
            duplicate: false,
            carrier: Bytes::from(vec![0xC4; 32]),
        }];
        block_store
            .put_block_message(&this_carrier)
            .expect("store this carrier");
        block_store
            .put_block_message(&sibling_record_block)
            .expect("store sibling record");

        let visible_blocks: HashSet<BlockHash> = [
            this_carrier.block_hash.clone(),
            sibling_record_block.block_hash.clone(),
        ]
        .into_iter()
        .collect();
        let this_record = RejectedDeploy {
            sig: sig.clone(),
            duplicate: false,
            carrier: this_carrier.block_hash.clone(),
        };
        let mut rejected_records = vec![this_record.clone()];

        let suppressed = suppress_already_recorded_rejections(
            &block_store,
            &visible_blocks,
            &mut rejected_records,
        )
        .expect("suppress rejected records");

        assert_eq!(
            suppressed, 0,
            "a record naming a sibling carrier covers nothing about this copy"
        );
        assert_eq!(
            rejected_records,
            vec![this_record],
            "the record for this carrier must be emitted"
        );
    }

    /// The retain is a pure floor-window rule: a rejected deploy whose
    /// validity window is OPEN at the merging block's floor stays
    /// buffered, regardless of any node-local verdict about it. The floor
    /// is the only irreversibility line — a win merely marked finalized by
    /// this node's finalizer can still sit above the floor, inside the
    /// merge scope of future blocks, where a later merge can reject it;
    /// the buffer holds the only re-proposable copy. Settled work leaves
    /// through the purge (effect in floor state) or at window close.
    #[test]
    fn window_open_rejected_deploys_stay_buffered_and_window_closed_drop() {
        let open = construct_deploy::source_deploy_now_full(
            "@31!(31)".to_string(),
            None,
            None,
            None,
            Some(60),
            None,
        )
        .expect("window-open deploy");
        let closed = construct_deploy::source_deploy_now_full(
            "@32!(32)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("window-closed deploy");

        // Floor at #100, lifespan 50: valid_after 60 keeps its window open
        // (60 + 50 > 100); valid_after 0 closed it at floor #50.
        let mut deploys = vec![open.clone(), closed.clone()];
        let skipped = retain_recoverable_rejected_deploys_for_buffer(100, 50, &mut deploys);

        assert_eq!(
            skipped, 1,
            "exactly the window-closed deploy leaves the buffer"
        );
        assert_eq!(deploys.len(), 1);
        assert_eq!(
            deploys[0].sig, open.sig,
            "the window-open deploy stays buffered regardless of local verdicts"
        );

        // At the exact boundary (valid_after + lifespan == floor) the
        // window is closed — the same boundary the merge window rule and
        // selection expiry use on the floor clock.
        let mut boundary = vec![closed.clone()];
        let skipped = retain_recoverable_rejected_deploys_for_buffer(50, 50, &mut boundary);
        assert_eq!(skipped, 1);
        assert!(boundary.is_empty());
    }
}
