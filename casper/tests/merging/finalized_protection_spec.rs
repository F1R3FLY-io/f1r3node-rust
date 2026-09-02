// Settled-content protection at the merge (issue #341).
//
// `partition_base_conflicts` protects the BASE lineage: committed content
// wins deterministically and is never adjudicated. But finality is wider
// than one lineage — under multi-parent finality a SETTLED block (at or
// below the merging block's finalized floor) can sit on a sibling branch
// of the merging block's base, its effects absent from the base state.
// Pre-fix, such a block's chains entered cost adjudication like any scope
// chain and could LOSE, rejecting settled effects; recovery then purged
// them ("finalized canonical wins") and the state entry vanished. Observed
// twice: the bridge-admin registry flake, and the #341 bond loss (a
// freshly-bonded validator's bond disappearing after an epoch-boundary
// merge).
//
// The invariant this spec pins: content whose carrier is at or below the
// merging block's floor is committed — a merge may never reject it (not by
// cost adjudication, not by the validity-window rule), and a scope chain
// conflicting with it loses deterministically, exactly as against the
// base. The classifier is the caller's `sig_settled_in_floor` probe — the
// tripwire's settled definition, derived from the merging block's frozen
// justification snapshot — never the node-local finalized set, which
// differs across nodes and would make replay non-deterministic, and never
// carrier height alone, which cannot discriminate a finalized sibling
// from a silent validator's dead one (see batch2/merge_window_spec.rs).

#![allow(clippy::mutable_key_type)]
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use casper::rust::errors::CasperError;
use casper::rust::merging::dag_merger;
use casper::rust::merging::deploy_chain_index::{DeployChainIndex, DeployIdWithCost};
use models::rust::block_hash::BlockHash;
use parking_lot::RwLock as PlRwLock;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use shared::rust::hashable_set::HashableSet;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

fn block_hash(seed: u8) -> BlockHash { Bytes::from(vec![seed; 32]) }

fn produce_on(channel_byte: u8, salt: u8) -> Produce {
    let mut ch = [0u8; 32];
    ch[0] = channel_byte;
    let mut ph = ch;
    ph[4] = salt;
    Produce {
        channel_hash: Blake2b256Hash::from_bytes(ch.to_vec()),
        hash: Blake2b256Hash::from_bytes(ph.to_vec()),
        persistent: false,
        is_deterministic: true,
        output_value: vec![],
        failed: false,
    }
}

fn consume_on(channel_byte: u8, salt: u8) -> Consume {
    let mut ch = [0u8; 32];
    ch[0] = channel_byte;
    let mut cs = ch;
    cs[5] = salt;
    Consume {
        channel_hashes: vec![Blake2b256Hash::from_bytes(ch.to_vec())],
        hash: Blake2b256Hash::from_bytes(cs.to_vec()),
        persistent: false,
    }
}

/// One user deploy consuming on `channel_byte`, carried by `source`.
fn chain_consuming_on(
    deploy_byte: u8,
    cost: u64,
    channel_byte: u8,
    source: &BlockHash,
    source_number: i64,
) -> DeployChainIndex {
    let mut event_log = EventLogIndex::empty();
    let mut consumes = HashSet::new();
    consumes.insert(consume_on(channel_byte, deploy_byte));
    event_log.consumes_linear_and_peeks = HashableSet(consumes);
    chain_from(deploy_byte, cost, event_log, source, source_number)
}

/// One user deploy producing on `channel_byte`, carried by `source`.
fn chain_producing_on(
    deploy_byte: u8,
    cost: u64,
    channel_byte: u8,
    source: &BlockHash,
    source_number: i64,
) -> DeployChainIndex {
    let mut event_log = EventLogIndex::empty();
    let mut produces = HashSet::new();
    produces.insert(produce_on(channel_byte, deploy_byte));
    event_log.produces_linear = HashableSet(produces);
    chain_from(deploy_byte, cost, event_log, source, source_number)
}

fn chain_from(
    deploy_byte: u8,
    cost: u64,
    event_log: EventLogIndex,
    source: &BlockHash,
    source_number: i64,
) -> DeployChainIndex {
    let mut deploys = HashSet::new();
    deploys.insert(DeployIdWithCost {
        deploy_id: Bytes::from(vec![deploy_byte]),
        cost,
    });
    DeployChainIndex::from_parts(
        HashableSet(deploys),
        Blake2b256Hash::from_bytes(vec![deploy_byte; 32]),
        event_log,
        rspace_plus_plus::rspace::merger::state_change::StateChange::empty(),
        source.clone(),
        source_number,
    )
}

/// Genesis G at height 0; base B and settled sibling F at height 1,
/// contender C at height 2, all with main parent G. F is finalized in the
/// DAG for realism, but the merge classifies by height against the floor.
fn dag_with_finalized_sibling(
    genesis: &BlockHash,
    base: &BlockHash,
    finalized_sibling: &BlockHash,
    contender: &BlockHash,
    contender_number: i64,
) -> KeyValueDagRepresentation {
    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut main_parent_map = imbl::HashMap::new();
    let mut child_map = imbl::HashMap::new();
    let mut height_map = imbl::OrdMap::new();

    let mut children_of_genesis = imbl::HashSet::new();
    for (hash, number) in [
        (genesis, 0i64),
        (base, 1),
        (finalized_sibling, 1),
        (contender, contender_number),
    ] {
        dag_set.insert(hash.clone());
        block_number_map.insert(hash.clone(), number);
        height_map
            .entry(number)
            .or_insert_with(imbl::HashSet::new)
            .insert(hash.clone());
        if number > 0 {
            main_parent_map.insert(hash.clone(), genesis.clone());
            children_of_genesis.insert(hash.clone());
        }
    }
    child_map.insert(genesis.clone(), children_of_genesis);

    let mut finalized_blocks_set = imbl::HashSet::new();
    finalized_blocks_set.insert(genesis.clone());
    finalized_blocks_set.insert(finalized_sibling.clone());

    KeyValueDagRepresentation {
        dag_set,
        latest_messages_map: imbl::HashMap::new(),
        child_map,
        height_map,
        block_number_map,
        main_parent_map,
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: finalized_sibling.clone(),
        finalized_blocks_set,
        block_metadata_index: Arc::new(PlRwLock::new(BlockMetadataStore::new(metadata_store))),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
    }
}

/// A settled sibling's deploy conflicts with a far more expensive scope
/// contender above the floor. Cost adjudication alone would reject the
/// settled (cheaper) side. The merge must instead treat the settled
/// carrier's content as committed: the settled deploy survives, the
/// contender is rejected. The floor is 1 — the merging block's floor
/// witnessed F's finalization — and the contender sits above it.
#[tokio::test(flavor = "multi_thread")]
async fn a_finalized_siblings_deploy_is_never_rejected_by_cost_adjudication() {
    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .expect("genesis context");
    let genesis_block = genesis_context.genesis_block.clone();
    let mut kvm =
        crate::util::rholang::resources::mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (_rt, history_repo) =
        crate::util::rholang::resources::mk_runtime_manager_with_history_at(&mut *kvm).await;
    let base_post_state =
        Blake2b256Hash::from_bytes(genesis_block.body.state.post_state_hash.to_vec());

    let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
    let base = block_hash(0xB0);
    let finalized_sibling = block_hash(0xF0);
    let contender = block_hash(0xC0);
    let dag = dag_with_finalized_sibling(&genesis, &base, &finalized_sibling, &contender, 2);

    const FINALIZED_DEPLOY: u8 = 0xF1;
    const CONTENDER_DEPLOY: u8 = 0xC1;
    let finalized_chain = chain_consuming_on(FINALIZED_DEPLOY, 1, 0xA0, &finalized_sibling, 1);
    let contender_chain = chain_producing_on(CONTENDER_DEPLOY, 1_000_000, 0xA0, &contender, 2);

    let scope: HashSet<BlockHash> = HashSet::from([finalized_sibling.clone(), contender.clone()]);
    let finalized_for_index = finalized_chain.clone();
    let contender_for_index = contender_chain.clone();
    let finalized_sibling_for_index = finalized_sibling.clone();
    let contender_for_index_hash = contender.clone();

    let (_state, rejected, _mergeable, _applied) = dag_merger::merge(
        &dag,
        &base,
        &base_post_state,
        move |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
            if *hash == finalized_sibling_for_index {
                Ok(vec![finalized_for_index.clone()])
            } else if *hash == contender_for_index_hash {
                Ok(vec![contender_for_index.clone()])
            } else {
                Ok(vec![])
            }
        },
        &history_repo,
        |chain: &DeployChainIndex| chain.deploys_with_cost.0.iter().map(|d| d.cost).sum(),
        Some(scope),
        1,
        50,
        &|_| Ok(false),
        &|sig| Ok(sig[0] == FINALIZED_DEPLOY),
        &HashSet::new(),
        &std::collections::HashMap::new(),
    )
    .expect("merge");

    let rejected_ids: VecDeque<u8> = rejected.iter().map(|r| r.sig[0]).collect();
    assert!(
        !rejected_ids.contains(&FINALIZED_DEPLOY),
        "a deploy whose carrier block is FINALIZED must never be rejected by \
         cost adjudication (finalized content is committed), got rejections {rejected_ids:?}"
    );
    assert!(
        rejected_ids.contains(&CONTENDER_DEPLOY),
        "the scope chain conflicting with finalized content must lose \
         deterministically, got rejections {rejected_ids:?}"
    );
}

/// The validity-window rule must not reject settled content either. The
/// settled carrier's deploy window is CLOSED at the merge's floor
/// (valid_after 0 at floor 5 with lifespan 5), which pre-fix routed the
/// chain into the late set for unconditional rejection. An ordinary
/// above-floor chain with the same closed window is still window-rejected.
#[tokio::test(flavor = "multi_thread")]
async fn a_settled_carriers_closed_window_deploy_is_not_window_rejected() {
    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .expect("genesis context");
    let genesis_block = genesis_context.genesis_block.clone();
    let mut kvm =
        crate::util::rholang::resources::mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (_rt, history_repo) =
        crate::util::rholang::resources::mk_runtime_manager_with_history_at(&mut *kvm).await;
    let base_post_state =
        Blake2b256Hash::from_bytes(genesis_block.body.state.post_state_hash.to_vec());

    let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
    let base = block_hash(0xB0);
    let settled_sibling = block_hash(0xF0);
    let late_contender = block_hash(0xC0);
    let dag = dag_with_finalized_sibling(&genesis, &base, &settled_sibling, &late_contender, 6);

    const SETTLED_DEPLOY: u8 = 0xF2;
    const LATE_DEPLOY: u8 = 0xC2;
    // Disjoint channels: no conflicts, so any rejection can only come from
    // the window rule. from_parts stamps valid_after = source_number - 1:
    // the settled chain's window (valid_after 0) is closed at floor 5 with
    // lifespan 5; the ordinary chain's window (valid_after 1) is open.
    let settled_chain = chain_consuming_on(SETTLED_DEPLOY, 1, 0xA0, &settled_sibling, 1);
    let mut late_chain = chain_producing_on(LATE_DEPLOY, 1, 0xB0, &late_contender, 6);
    late_chain
        .deploy_windows
        .insert(Bytes::from(vec![LATE_DEPLOY]), 0);

    let scope: HashSet<BlockHash> =
        HashSet::from([settled_sibling.clone(), late_contender.clone()]);
    let settled_for_index = settled_chain.clone();
    let late_for_index = late_chain.clone();
    let settled_sibling_for_index = settled_sibling.clone();
    let late_contender_for_index = late_contender.clone();

    let (_state, rejected, _mergeable, _applied) = dag_merger::merge(
        &dag,
        &base,
        &base_post_state,
        move |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
            if *hash == settled_sibling_for_index {
                Ok(vec![settled_for_index.clone()])
            } else if *hash == late_contender_for_index {
                Ok(vec![late_for_index.clone()])
            } else {
                Ok(vec![])
            }
        },
        &history_repo,
        |chain: &DeployChainIndex| chain.deploys_with_cost.0.iter().map(|d| d.cost).sum(),
        Some(scope),
        5,
        5,
        &|_| Ok(false),
        &|sig| Ok(sig[0] == SETTLED_DEPLOY),
        &HashSet::new(),
        &std::collections::HashMap::new(),
    )
    .expect("merge");

    let rejected_ids: VecDeque<u8> = rejected.iter().map(|r| r.sig[0]).collect();
    assert!(
        !rejected_ids.contains(&SETTLED_DEPLOY),
        "a settled carrier's deploy must be exempt from the validity-window \
         rule (settled content is committed), got rejections {rejected_ids:?}"
    );
    assert!(
        rejected_ids.contains(&LATE_DEPLOY),
        "an ordinary above-floor chain with a closed window must still be \
         window-rejected, got rejections {rejected_ids:?}"
    );
}
