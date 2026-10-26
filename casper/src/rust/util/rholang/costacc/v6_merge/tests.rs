//! DR-120 (gap G9): tests of the v6 merge rule (design v3 §7 and v4 §7).
//!
//! The scenarios build synthetic chains over an in-memory history, a test
//! DAG and an in-memory block store, and run dev's merge entry point
//! (`dag_merger::merge_with_rule`) under both rules, the fast path and the
//! ordered pass. The property tests run 256 cases each.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::{ListParWithRandom, PCost};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::{get_random_block_default, processed_deploy_gen};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedUserDeploy, RejectedDeploy,
};
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::native_cost_evidence::{
    NativeCostEvidenceV1, NativeCostFailureClass, NativeFundingCaseSource, NativeFundingCaseV1,
    NativeFundingObligation, NativePrepaidDeltaV1,
};
use models::rust::native_wallet_receipt::{
    NativeWalletReceiptLimits, NativeWalletReceiptRow, NativeWalletReceiptV1,
};
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use parking_lot::RwLock as PlRwLock;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieInsertAction, TrieInsertBinaryProduce,
};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{self, MergeType, NumberChannelsDiff};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::serializers::serializers;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use shared::rust::hashable_set::HashableSet;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::claims::{self, FoldedClaims};
use super::evidence::{every_deploy_has_fee_transition, has_fee_transition};
use super::fast::{self, FastPathInputs, FastPathOutcome};
use super::ledger::{self, Failure, PurseLedger, SignSplit};
use super::order::{k_cmp, last_by_k, sort_by_k};
use super::ordered::{self, OrderedPassInputs, Rejections, Witness};
use super::{compose, rule_for, walk_branch, MergeRule, RhoHistoryReader};
use crate::rust::errors::CasperError;
use crate::rust::merging::conflict_set_merger::{Branch, ResolvedConflicts};
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::{DeployChainIndex, DeployIdWithCost};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

// ─── Fixtures ──────────────────────────────────────────────────────────────

/// The genesis block of every test DAG.
const GENESIS: u8 = 0x00;
/// The merge base of every test DAG: the main parent, at height 1.
const BASE: u8 = 0xB0;
/// The floor and the validity window of every scenario. A chain is late when
/// its `valid_after` is at most `FLOOR - LIFESPAN`.
const FLOOR: i64 = 10;
const LIFESPAN: i64 = 5;
const OPEN_WINDOW: i64 = 9;
const CLOSED_WINDOW: i64 = 0;

fn hash32(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

fn block_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

fn deploy_id(byte: u8) -> Bytes { Bytes::from(vec![byte; 32]) }

fn event_hash(channel: u8, salt: u8, kind: u8) -> Blake2b256Hash {
    let mut bytes = vec![channel; 32];
    bytes[30] = kind;
    bytes[31] = salt;
    Blake2b256Hash::from_bytes(bytes)
}

/// A non-persistent produce on `channel`; `salt` tells produces apart.
fn produce(channel: u8, salt: u8) -> Produce {
    Produce {
        channel_hash: hash32(channel),
        hash: event_hash(channel, salt, 0x50),
        persistent: false,
        is_deterministic: true,
        output_value: vec![],
        failed: false,
    }
}

/// A non-persistent consume on `channel`; `salt` tells consumes apart.
fn consume(channel: u8, salt: u8) -> Consume {
    Consume {
        channel_hashes: vec![hash32(channel)],
        hash: event_hash(channel, salt, 0xC0),
        persistent: false,
    }
}

/// An encoded numeric datum on `channel`; `seed` gives it its own random
/// state, so two datums with one value still differ.
fn number_datum(channel: &Blake2b256Hash, value: i64, seed: u8) -> Vec<u8> {
    let rnd = Blake2b512Random::create_from_bytes(&[seed; 32]);
    let par_with_rnd = ListParWithRandom {
        pars: vec![RhoNumber::create_par(value)],
        random_state: rnd.to_bytes(),
        ..Default::default()
    };
    let data_hash = stable_hash_provider::hash_produce(channel.bytes(), &par_with_rnd, false);
    serializers::encode_datum(&Datum {
        a: par_with_rnd,
        persist: false,
        source: Produce {
            channel_hash: channel.clone(),
            hash: data_hash,
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        },
    })
}

/// The data that one chain describes.
#[derive(Clone)]
struct ChainSpec {
    deploy: u8,
    cost: u64,
    block: u8,
    height: i64,
    user_log: EventLogIndex,
    system_numbers: NumberChannelsDiff,
    changes: Vec<(Blake2b256Hash, Vec<Vec<u8>>, Vec<Vec<u8>>)>,
    valid_after: Option<i64>,
}

impl ChainSpec {
    fn new(deploy: u8, block: u8, height: i64) -> Self {
        ChainSpec {
            deploy,
            cost: 1,
            block,
            height,
            user_log: EventLogIndex::empty(),
            system_numbers: NumberChannelsDiff::new(),
            changes: Vec::new(),
            valid_after: Some(OPEN_WINDOW),
        }
    }

    fn cost(mut self, cost: u64) -> Self {
        self.cost = cost;
        self
    }

    fn produces(mut self, channel: u8, salt: u8) -> Self {
        self.user_log
            .produces_linear
            .0
            .insert(produce(channel, salt));
        self
    }

    fn consumes(mut self, channel: u8, salt: u8) -> Self {
        self.user_log
            .consumes_linear_and_peeks
            .0
            .insert(consume(channel, salt));
        self
    }

    /// The chain consumed `produce` in a COMM.
    fn consumed(mut self, consumed: Produce) -> Self {
        self.user_log.produces_consumed.0.insert(consumed);
        self
    }

    fn number(mut self, channel: u8, diff: i64, merge_type: MergeType) -> Self {
        self.user_log
            .number_channels_data
            .insert(hash32(channel), (diff, merge_type));
        self
    }

    fn system_number(mut self, channel: u8, diff: i64) -> Self {
        self.system_numbers
            .insert(hash32(channel), (diff, MergeType::IntegerAdd));
        self
    }

    fn change(mut self, channel: u8, removed: Vec<Vec<u8>>, added: Vec<Vec<u8>>) -> Self {
        self.changes.push((hash32(channel), removed, added));
        self
    }

    fn late(mut self) -> Self {
        self.valid_after = Some(CLOSED_WINDOW);
        self
    }

    /// The chain, or `None` when its user and system parts do not combine.
    /// Index building rejects such a chain too (deploy_chain_index.rs:98-99).
    fn try_build(self) -> Option<DeployChainIndex> {
        let deploys = HashableSet(HashSet::from([DeployIdWithCost {
            deploy_id: deploy_id(self.deploy),
            cost: self.cost,
        }]));
        let changes: HashMap<Blake2b256Hash, ChannelChange<Vec<u8>>> = self
            .changes
            .into_iter()
            .map(|(channel, removed, added)| (channel, ChannelChange { added, removed }))
            .collect();
        let mut chain = DeployChainIndex::from_parts(
            deploys,
            Blake2b256Hash::from_bytes(vec![self.deploy; 32]),
            self.user_log,
            StateChange::from_parts(changes, HashMap::new(), HashMap::new()),
            block_hash(self.block),
            self.height,
        );
        chain.system_event_log_index.number_channels_data = self.system_numbers;
        chain.event_log_index =
            EventLogIndex::combine(&chain.user_event_log_index, &chain.system_event_log_index)
                .ok()?;
        chain.deploy_windows = match self.valid_after {
            Some(valid_after) => HashMap::from([(deploy_id(self.deploy), valid_after)]),
            None => HashMap::new(),
        };
        Some(chain)
    }

    fn build(self) -> DeployChainIndex { self.try_build().expect("the test chain's parts combine") }
}

/// An in-memory history repository at the empty root.
fn history() -> RhoHistoryRepository {
    let stores = RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    };
    let (_manager, repository) = RuntimeManager::create_with_history(
        stores,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    repository
}

/// A committed state that holds exactly `data` on its channels.
fn state_with(
    repository: &RhoHistoryRepository,
    data: &[(Blake2b256Hash, Vec<Vec<u8>>)],
) -> Blake2b256Hash {
    let actions = data
        .iter()
        .map(|(hash, data)| {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                TrieInsertBinaryProduce {
                    hash: hash.clone(),
                    data: data.clone(),
                },
            ))
        })
        .collect();
    repository.do_checkpoint(actions).root()
}

fn block_metadata(hash: &BlockHash, height: i64, parents: &[BlockHash]) -> BlockMetadata {
    BlockMetadata {
        block_hash: hash.clone(),
        parents: parents.to_vec(),
        sender: Bytes::new(),
        justifications: Vec::new(),
        weight_map: BTreeMap::new(),
        block_number: height,
        sequence_number: 0,
        invalid: false,
        directly_finalized: false,
        finalized: false,
        fault_tolerance_value: 0.0,
        merge_base: Bytes::new(),
    }
}

/// A test DAG. `blocks` lists (block, height, parents) in topological order;
/// the first parent is the main parent.
fn dag(blocks: &[(BlockHash, i64, Vec<BlockHash>)]) -> KeyValueDagRepresentation {
    let mut metadata = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(Arc::new(
        InMemoryKeyValueStore::new(),
    )));
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut main_parent_map = imbl::HashMap::new();
    let mut child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>> = imbl::HashMap::new();
    let mut height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>> = imbl::OrdMap::new();
    for (hash, height, parents) in blocks {
        metadata
            .add(block_metadata(hash, *height, parents))
            .expect("the test DAG stores its metadata");
        dag_set.insert(hash.clone());
        block_number_map.insert(hash.clone(), *height);
        height_map.entry(*height).or_default().insert(hash.clone());
        if let Some(main_parent) = parents.first() {
            main_parent_map.insert(hash.clone(), main_parent.clone());
        }
        for parent in parents {
            child_map
                .entry(parent.clone())
                .or_default()
                .insert(hash.clone());
        }
    }
    KeyValueDagRepresentation {
        dag_set,
        latest_messages_map: imbl::HashMap::new(),
        child_map,
        height_map,
        block_number_map,
        main_parent_map,
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: block_hash(GENESIS),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(PlRwLock::new(metadata)),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
        carrier_index: Arc::new(PlRwLock::new(
            block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
        )),
    }
}

/// A block with the given identity, height and parents, and no deploys.
fn empty_block(hash: &BlockHash, height: i64, parents: &[BlockHash]) -> BlockMessage {
    let mut block = get_random_block_default();
    block.block_hash = hash.clone();
    block.header.parents_hash_list = parents.to_vec();
    block.body.state.block_number = height;
    block.body.deploys = Vec::new();
    block.body.system_deploys = Vec::new();
    block.body.rejected_deploys = Vec::new();
    block
}

type MergeOutcome = (
    Blake2b256Hash,
    Vec<RejectedDeploy>,
    Vec<(Bytes, BlockHash)>,
    HashSet<Bytes>,
);

/// A merge scenario: base B at height 1 over genesis, scope blocks above or
/// beside it, the chains of every block, and a base state.
struct World {
    repository: RhoHistoryRepository,
    base_state: Blake2b256Hash,
    dag: KeyValueDagRepresentation,
    block_store: KeyValueBlockStore,
    chains: HashMap<BlockHash, Vec<DeployChainIndex>>,
    scope: HashSet<BlockHash>,
    base_lineage: HashSet<BlockHash>,
    prior_rejection_counts: HashMap<Bytes, u64>,
    settled: HashSet<Bytes>,
}

struct WorldBuilder {
    base_data: Vec<(Blake2b256Hash, Vec<Vec<u8>>)>,
    blocks: Vec<(BlockHash, i64, Vec<BlockHash>)>,
    chains: Vec<DeployChainIndex>,
    base_lineage_chains: Vec<DeployChainIndex>,
    prior_rejection_counts: HashMap<Bytes, u64>,
    settled: HashSet<Bytes>,
}

impl WorldBuilder {
    fn new() -> Self {
        WorldBuilder {
            base_data: Vec::new(),
            blocks: vec![
                (block_hash(GENESIS), 0, Vec::new()),
                (block_hash(BASE), 1, vec![block_hash(GENESIS)]),
            ],
            chains: Vec::new(),
            base_lineage_chains: Vec::new(),
            prior_rejection_counts: HashMap::new(),
            settled: HashSet::new(),
        }
    }

    fn base_data(mut self, channel: u8, data: Vec<Vec<u8>>) -> Self {
        self.base_data.push((hash32(channel), data));
        self
    }

    /// A scope block at `height` with `parents` (genesis when empty).
    fn block(mut self, block: u8, height: i64, parents: &[u8]) -> Self {
        let parents = match parents.is_empty() {
            true => vec![block_hash(GENESIS)],
            false => parents.iter().map(|byte| block_hash(*byte)).collect(),
        };
        self.blocks.push((block_hash(block), height, parents));
        self
    }

    fn chain(mut self, chain: DeployChainIndex) -> Self {
        self.chains.push(chain);
        self
    }

    /// A chain that the base's own lineage carries (in the base block).
    fn base_chain(mut self, chain: DeployChainIndex) -> Self {
        self.base_lineage_chains.push(chain);
        self
    }

    fn losses(mut self, deploy: u8, losses: u64) -> Self {
        self.prior_rejection_counts
            .insert(deploy_id(deploy), losses);
        self
    }

    fn settled(mut self, deploy: u8) -> Self {
        self.settled.insert(deploy_id(deploy));
        self
    }

    fn build(self) -> World {
        let repository = history();
        let base_state = state_with(&repository, &self.base_data);
        let block_store = KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        for (hash, height, parents) in &self.blocks {
            block_store
                .put(hash.clone(), &empty_block(hash, *height, parents))
                .expect("the test block store keeps the block");
        }
        let mut chains: HashMap<BlockHash, Vec<DeployChainIndex>> = HashMap::new();
        let mut scope = HashSet::new();
        for chain in self.chains {
            scope.insert(chain.source_block_hash.clone());
            chains
                .entry(chain.source_block_hash.clone())
                .or_default()
                .push(chain);
        }
        let mut base_lineage = HashSet::new();
        for chain in self.base_lineage_chains {
            base_lineage.insert(block_hash(BASE));
            chains.entry(block_hash(BASE)).or_default().push(chain);
        }
        World {
            repository,
            base_state,
            dag: dag(&self.blocks),
            block_store,
            chains,
            scope,
            base_lineage,
            prior_rejection_counts: self.prior_rejection_counts,
            settled: self.settled,
        }
    }
}

impl World {
    fn index(&self, hash: &BlockHash) -> Result<Vec<DeployChainIndex>, CasperError> {
        Ok(self.chains.get(hash).cloned().unwrap_or_default())
    }

    fn merge(&self, rule: MergeRule) -> Result<MergeOutcome, CasperError> {
        dag_merger::merge_with_rule(
            rule,
            &self.dag,
            &block_hash(BASE),
            &self.base_state,
            |hash: &BlockHash| self.index(hash),
            &self.repository,
            dag_merger::cost_optimal_rejection_alg(),
            Some(self.scope.clone()),
            FLOOR,
            LIFESPAN,
            &|_| Ok(false),
            &|sig: &Bytes| Ok(self.settled.contains(sig)),
            &self.base_lineage,
            &self.prior_rejection_counts,
        )
    }

    fn fast(&self) -> Result<Option<FastPathOutcome>, CasperError> {
        fast::try_compose(&FastPathInputs {
            dag: &self.dag,
            block_store: &self.block_store,
            history_repository: &self.repository,
            base: &block_hash(BASE),
            base_post_state: &self.base_state,
            base_holds_floor: true,
            scope: &self.scope,
            base_lineage_blocks: &self.base_lineage,
            floor_block_number: FLOOR,
            deploy_lifespan: LIFESPAN,
            index: &|hash| self.index(hash),
            prior_rejection_counts: &self.prior_rejection_counts,
        })
    }

    fn reader(&self) -> RhoHistoryReader {
        self.repository
            .get_history_reader(&self.base_state)
            .expect("the base state has a reader")
    }

    fn all_chains(&self) -> Vec<DeployChainIndex> {
        let mut blocks: Vec<&BlockHash> = self.scope.iter().collect();
        blocks.sort();
        let mut chains: Vec<DeployChainIndex> = blocks
            .into_iter()
            .flat_map(|hash| self.chains.get(hash).cloned().unwrap_or_default())
            .collect();
        dag_merger::stamp_prior_rejections(&mut chains, &self.prior_rejection_counts);
        chains
    }
}

fn rejected_ids(outcome: &MergeOutcome) -> BTreeSet<u8> {
    outcome.1.iter().map(|record| record.sig[0]).collect()
}

fn applied_ids(outcome: &MergeOutcome) -> BTreeSet<u8> {
    outcome.3.iter().map(|sig| sig[0]).collect()
}

/// A branch-level conflict map with dev's semantics: the combined event log
/// of each branch (dag_merger.rs:1397-1444), the event-indexed checks, and
/// the same-user-deploy-id pass (dag_merger.rs:1688-1722).
fn dev_conflict_map(
    branches: &HashableSet<Branch<DeployChainIndex>>,
) -> Result<
    HashMap<Branch<DeployChainIndex>, HashableSet<Branch<DeployChainIndex>>>,
    rspace_plus_plus::rspace::errors::HistoryError,
> {
    let owned: Vec<Branch<DeployChainIndex>> = branches.0.iter().cloned().collect();
    let mut logs = Vec::with_capacity(owned.len());
    for branch in &owned {
        let mut user = EventLogIndex::empty();
        let mut system = EventLogIndex::empty();
        for chain in branch.0.iter() {
            user = EventLogIndex::combine(&user, &chain.user_event_log_index)?;
            system = EventLogIndex::combine(&system, &chain.system_event_log_index)?;
        }
        logs.push(EventLogIndex::combine(&user, &system)?);
    }
    let log_refs: Vec<&EventLogIndex> = logs.iter().collect();
    let mut map = merging_logic::compute_conflict_map_event_indexed(&owned, &log_refs);
    for (a_index, a) in owned.iter().enumerate() {
        for b in owned.iter().skip(a_index + 1) {
            let shared = a.0.iter().any(|x| {
                b.0.iter().any(|y| {
                    x.deploys_with_cost.0.iter().any(|d| {
                        y.deploys_with_cost
                            .0
                            .iter()
                            .any(|e| e.deploy_id == d.deploy_id)
                    })
                })
            });
            if shared {
                map.get_mut(a)
                    .expect("every branch is a key")
                    .0
                    .insert(b.clone());
                map.get_mut(b)
                    .expect("every branch is a key")
                    .0
                    .insert(a.clone());
            }
        }
    }
    Ok(map)
}

fn dev_branches(chains: &HashableSet<DeployChainIndex>) -> HashableSet<Branch<DeployChainIndex>> {
    let chains: Vec<DeployChainIndex> = chains.0.iter().cloned().collect();
    HashableSet(fast::compute_branches(&chains).into_iter().collect())
}

/// Runs the ordered pass directly, with dev-equivalent relations.
fn run_ordered_pass(
    world: &World,
    actual: &[DeployChainIndex],
    late: &[DeployChainIndex],
    pinned: &HashSet<DeployChainIndex>,
) -> (ResolvedConflicts<DeployChainIndex>, usize, Rejections) {
    let reader = world.reader();
    ordered::ordered_pass_witnessed(&OrderedPassInputs {
        dag: &world.dag,
        actual,
        late,
        pinned,
        base_conflicting: &[],
        settled_conflicting: &[],
        depends: &super::chain_depends,
        compute_branches: &dev_branches,
        compute_conflict_map: &dev_conflict_map,
        history_reader: &reader,
    })
    .expect("the ordered pass succeeds")
}

fn kept_ids(resolved: &ResolvedConflicts<DeployChainIndex>) -> BTreeSet<u8> {
    resolved
        .to_merge
        .iter()
        .flat_map(|branch| branch.0.iter())
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .map(|deploy| deploy.deploy_id[0])
        .collect()
}

fn witness_of(rejections: &Rejections, deploy: u8) -> Option<&Witness> {
    rejections.iter().find_map(|(chain, witness)| {
        chain
            .deploys_with_cost
            .0
            .iter()
            .any(|d| d.deploy_id[0] == deploy)
            .then_some(witness)
    })
}

fn candidate(chains: &[&DeployChainIndex]) -> Branch<DeployChainIndex> {
    Arc::new(HashableSet(
        chains.iter().map(|chain| (*chain).clone()).collect(),
    ))
}

// ─── The rule: the deploy formats decide (C6, user decision 2026-10-10) ────

fn legacy_deploy() -> ProcessedUserDeploy {
    let deploy = processed_deploy_gen()
        .new_tree(&mut TestRunner::default())
        .expect("a legacy deploy generates")
        .current();
    ProcessedUserDeploy::Legacy(deploy)
}

fn test_schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: vec![PhloResourceClassV1 {
            identity: b"compute",
            measurement_unit: b"unit",
            measurement_rule: [1; 32],
            valuation_rule: [2; 32],
            weight: 1,
        }],
        actual_price: 2,
        compatibility_rule: [3; 32],
    }
}

/// A processed offered deploy whose committed funding case records
/// `fee_next_cursor`, with consistent evidence and wallet receipt.
fn offered_deploy(fee_next_cursor: Option<u32>, time_stamp: i64) -> ProcessedUserDeploy {
    let protocol = offered_funded_v6_limits();
    let payload_limits = protocol.envelope.payload;
    let schedule = test_schedule();
    let commitment = schedule
        .digest(PhloGenesisPolicy::LIMITS)
        .expect("the schedule digests");
    let source = PhloSourcePolicyV1::new(b"purse", 100, 100, true, vec![], PhloSourceLimits {
        wire: payload_limits.funding.wire,
        resource_permissions: 0,
        authority_nodes: 0,
    })
    .expect("the source policy is valid");
    let intent = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 10,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![schedule],
            },
            schedule_commitment: commitment,
            total_exposure: 100,
            sources: vec![source],
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: payload_limits.funding.wire,
        base: payload_limits.funding,
        grant_uses: payload_limits.funding.wire.total_bytes / 8,
        grant_id_bytes: payload_limits.funding.wire.field_bytes,
        quote_evidence_bytes: payload_limits.funding.wire.field_bytes,
    })
    .expect("the funding intent encodes");
    let body = DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, intent, 10, 2, payload_limits)
        .expect("the offered payload is valid");
    let signed = Cosigned::create_single_envelope(
        payload,
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[2; 32]),
    )
    .expect("the owner signs the offer");
    let envelope = DeployEnvelope::from_proto(
        OfferedFundedDeploy::to_proto(&signed).expect("the offer encodes"),
        protocol.envelope,
    )
    .expect("the envelope decodes");
    let envelope_commitment: [u8; 32] = envelope
        .identity()
        .as_bytes()
        .try_into()
        .expect("an offered identity has 32 bytes");
    let evidence_limits = protocol.evidence;
    let wallet_receipt = NativeWalletReceiptV1 {
        rows: vec![NativeWalletReceiptRow {
            address: b"purse",
            resource_rev: 14,
            fee_rev: 1,
            post_balance: 85,
        }],
        resource_rev: 14,
        fee_rev: 1,
    }
    .encode(NativeWalletReceiptLimits {
        wire: PhloWireLimits {
            total_bytes: evidence_limits.field_bytes,
            field_bytes: evidence_limits.field_bytes,
        },
        payers: payload_limits.funding.sources,
    })
    .expect("the wallet receipt encodes");
    let funding_case = NativeFundingCaseV1 {
        sources: vec![NativeFundingCaseSource {
            custody: b"purse",
            capacity: 100,
            exposure_limit: 100,
            debit_limit: 100,
            hold: 15,
            debit: 15,
            fee: 1,
            refund: 0,
        }],
        obligations: vec![NativeFundingObligation {
            key: b"cost",
            quantity: 1,
            amount: 15,
        }],
        eligible: vec![vec![true]],
        assignment: vec![vec![15]],
        resource_next_cursor: Some(0),
        fee_next_cursor,
        resource_unrestricted: true,
        resource_restriction_witness: None,
        possible_fee_payers: vec![true],
    }
    .encode(protocol.funding_case)
    .expect("the funding case encodes");
    let prepaid_delta = NativePrepaidDeltaV1 {
        draws: Vec::new(),
        births: Vec::new(),
        replacements: Vec::new(),
    }
    .encode(protocol.prepaid_delta)
    .expect("the prepaid delta encodes");
    let evidence = NativeCostEvidenceV1 {
        wallet_settlement_log_events: 0,
        envelope_commitment,
        genesis_policy_commitment: [1; 32],
        schedule_commitment: [2; 32],
        original_funding_root: [3; 32],
        settlement_runtime_root: [4; 32],
        post_state_root: [5; 32],
        phlo_used: 7,
        fresh_phlo: 7,
        retained_phlo: 0,
        phlo_limit: 10,
        phlo_price: 2,
        fee_rev: 1,
        failure_class: NativeCostFailureClass::Success,
        budget_recording: b"budget",
        operation_journal: b"journal",
        funding_case: &funding_case,
        prepaid_delta: &prepaid_delta,
        wallet_settlement: &wallet_receipt,
    }
    .encode(evidence_limits)
    .expect("the evidence encodes");
    ProcessedUserDeploy::Offered(
        OfferedProcessedDeploy::new(
            envelope,
            PCost { cost: 7 },
            Vec::new(),
            false,
            evidence,
            evidence_limits,
        )
        .expect("the processed offered deploy is valid"),
    )
}

fn block_with(byte: u8, parents: &[u8], deploys: Vec<ProcessedUserDeploy>) -> BlockMessage {
    let parents: Vec<BlockHash> = parents.iter().map(|b| block_hash(*b)).collect();
    let mut block = empty_block(&block_hash(byte), 1, &parents);
    block.body.deploys = deploys;
    block
}

fn store_with(blocks: &[&BlockMessage]) -> KeyValueBlockStore {
    let store = KeyValueBlockStore::new(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    for block in blocks {
        store
            .put_block_message(block)
            .expect("the test block store keeps the block");
    }
    store
}

fn scope_of(blocks: &[&BlockMessage]) -> HashSet<BlockHash> {
    blocks
        .iter()
        .map(|block| block.block_hash.clone())
        .collect()
}

#[test]
fn rule_for_exempts_genesis() {
    let legacy_genesis = block_with(GENESIS, &[], vec![legacy_deploy()]);
    let offered_genesis = block_with(GENESIS, &[], vec![offered_deploy(Some(0), 1)]);
    for genesis in [legacy_genesis, offered_genesis] {
        let store = store_with(&[&genesis]);
        assert_eq!(
            rule_for(
                std::slice::from_ref(&genesis),
                &scope_of(&[&genesis]),
                &store
            )
            .expect("rule"),
            MergeRule::Dev
        );
    }
}

#[test]
fn rule_for_offered_parent_decides_without_a_store_read() {
    let parent = block_with(0x11, &[GENESIS], vec![offered_deploy(Some(0), 1)]);
    let absent = block_with(0x12, &[GENESIS], Vec::new());
    let empty_store = store_with(&[]);
    assert_eq!(
        rule_for(
            std::slice::from_ref(&parent),
            &scope_of(&[&parent, &absent]),
            &empty_store
        )
        .expect("an offered parent decides before any scope block is read"),
        MergeRule::OfferedV6
    );
}

#[test]
fn rule_for_keeps_dev_on_legacy_and_system_only_blocks() {
    let legacy = block_with(0x11, &[GENESIS], vec![legacy_deploy()]);
    let system_only = block_with(0x12, &[GENESIS], Vec::new());
    let legacy_ancestor = block_with(0x13, &[GENESIS], vec![legacy_deploy()]);
    let store = store_with(&[&legacy, &system_only, &legacy_ancestor]);
    assert_eq!(
        rule_for(
            &[legacy.clone(), system_only.clone()],
            &scope_of(&[&legacy, &system_only, &legacy_ancestor]),
            &store
        )
        .expect("rule"),
        MergeRule::Dev
    );
}

#[test]
fn rule_for_reads_an_offered_scope_block() {
    let offered = block_with(0x11, &[GENESIS], vec![offered_deploy(Some(0), 1)]);
    let first = block_with(0x12, &[0x11], Vec::new());
    let second = block_with(0x13, &[0x11], Vec::new());
    let store = store_with(&[&offered, &first, &second]);
    assert_eq!(
        rule_for(
            &[first.clone(), second.clone()],
            &scope_of(&[&offered, &first, &second]),
            &store
        )
        .expect("rule"),
        MergeRule::OfferedV6
    );
}

#[test]
fn rule_for_rejects_a_scope_block_missing_from_the_store() {
    let present = block_with(0x12, &[GENESIS], Vec::new());
    let absent = block_with(0x13, &[GENESIS], Vec::new());
    let store = store_with(&[&present]);
    assert!(matches!(
        rule_for(
            std::slice::from_ref(&present),
            &scope_of(&[&present, &absent]),
            &store
        ),
        Err(CasperError::RuntimeError(_))
    ));
}

// ─── P2, the fee-cursor evidence (C9) ──────────────────────────────────────

#[test]
fn p2_requires_fee_next_cursor() {
    let charged = block_with(0x11, &[GENESIS], vec![offered_deploy(Some(0), 1)]);
    let uncharged = block_with(0x12, &[GENESIS], vec![offered_deploy(None, 2)]);
    let system_only = block_with(0x13, &[GENESIS], Vec::new());
    let legacy = block_with(0x14, &[GENESIS], vec![legacy_deploy()]);
    assert!(every_deploy_has_fee_transition(&charged).expect("charged decodes"));
    assert!(!every_deploy_has_fee_transition(&uncharged).expect("uncharged decodes"));
    assert!(every_deploy_has_fee_transition(&system_only).expect("no user deploy"));
    assert!(matches!(
        every_deploy_has_fee_transition(&legacy),
        Err(CasperError::RuntimeError(_))
    ));
    let mixed = block_with(0x15, &[GENESIS], vec![
        offered_deploy(Some(0), 3),
        offered_deploy(None, 4),
    ]);
    assert!(!every_deploy_has_fee_transition(&mixed).expect("mixed decodes"));
}

#[test]
fn fee_transition_reads_only_the_fee_cursor() {
    let mut funding = NativeFundingCaseV1 {
        sources: Vec::new(),
        obligations: Vec::new(),
        eligible: Vec::new(),
        assignment: Vec::new(),
        resource_next_cursor: Some(3),
        fee_next_cursor: None,
        resource_unrestricted: true,
        resource_restriction_witness: None,
        possible_fee_payers: Vec::new(),
    };
    assert!(!has_fee_transition(&funding));
    funding.fee_next_cursor = Some(0);
    assert!(has_fee_transition(&funding));
}

// ─── Claims (C1, C2) ───────────────────────────────────────────────────────

#[test]
fn raw_list_detects_equal_cost_copies() {
    let first = ChainSpec::new(0x21, 0x11, 1)
        .change(0x71, vec![], vec![vec![0xA1]])
        .build();
    let copy = ChainSpec::new(0x21, 0x12, 1)
        .change(0x72, vec![], vec![vec![0xA2]])
        .build();
    let other = ChainSpec::new(0x22, 0x12, 1).build();
    assert!(claims::has_repeated_user_id(&[first.clone(), copy.clone()]));
    assert!(!claims::has_repeated_user_id(&[first.clone(), other]));
    let world = WorldBuilder::new()
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .chain(first)
        .chain(copy)
        .build();
    assert!(
        world.fast().expect("the fast path runs").is_none(),
        "two copies of one deploy never compose on the fast path"
    );
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    let reader = world
        .repository
        .get_history_reader(&merged.0)
        .expect("the merged state has a reader");
    let applied_copies = [0x71u8, 0x72]
        .iter()
        .filter(|channel| {
            !reader
                .get_data_proj_binary(&hash32(**channel))
                .expect("read")
                .is_empty()
        })
        .count();
    assert_eq!(applied_copies, 1, "the merge applies exactly one copy");
}

/// Control (C2): chain equality reads only the deploy set, so a set of chains
/// silently collapses two equal-cost copies.
#[test]
fn hashable_set_collapses_equal_cost_copies() {
    let first = ChainSpec::new(0x21, 0x11, 1).build();
    let copy = ChainSpec::new(0x21, 0x12, 1).build();
    assert_ne!(first.source_block_hash, copy.source_block_hash);
    assert_eq!(HashableSet(HashSet::from([first, copy])).0.len(), 1);
}

/// C1: a plain change on a channel that another kept branch folds is
/// rejected. Without the check, the number override keeps the folded value
/// and silently drops the plain one.
#[test]
fn folded_mixing_rejects_plain_change() {
    let channel = hash32(0x60);
    let base = number_datum(&channel, 10, 1);
    let folding = ChainSpec::new(0x31, 0x11, 1)
        .cost(2)
        .number(0x60, 5, MergeType::IntegerAdd)
        .change(0x60, vec![base.clone()], vec![number_datum(
            &channel, 15, 2,
        )])
        .build();
    let plain = ChainSpec::new(0x32, 0x12, 1)
        .cost(1)
        .change(0x60, vec![base.clone()], vec![number_datum(&channel, 7, 3)])
        .build();
    let world = WorldBuilder::new()
        .base_data(0x60, vec![base])
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .chain(folding.clone())
        .chain(plain.clone())
        .build();
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x32]));
    let (_, _, witnesses) = run_ordered_pass(&world, &world.all_chains(), &[], &HashSet::new());
    assert!(matches!(
        witness_of(&witnesses, 0x32),
        Some(Witness::Mixing)
    ));
    let mut claims_of_kept = FoldedClaims::default();
    claims_of_kept.add(&HashableSet(HashSet::from([folding.clone()])));
    assert!(claims_of_kept.mixes(&HashableSet(HashSet::from([plain.clone()]))));
    // The control: composing both keeps one datum, base + folded diff, and
    // drops the plain write.
    let both = resolved_with(vec![vec![folding], vec![plain]]);
    let state = compose::compose(&world.repository, &world.base_state, &both)
        .expect("the number override composes");
    let data = world
        .repository
        .get_history_reader(&state)
        .expect("reader")
        .get_data(&channel)
        .expect("read");
    let values: Vec<i64> = data
        .iter()
        .filter_map(|datum| {
            rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic::try_get_number_with_rnd(&datum.a)
                .map(|(value, _)| value)
        })
        .collect();
    assert_eq!(
        values,
        vec![15],
        "the plain value 7 is lost without the claim check"
    );
}

fn resolved_with(branches: Vec<Vec<DeployChainIndex>>) -> ResolvedConflicts<DeployChainIndex> {
    ResolvedConflicts {
        to_merge: branches
            .into_iter()
            .map(|chains| HashableSet(chains.into_iter().collect()))
            .collect(),
        rejected: HashableSet(HashSet::new()),
        late_set_size: 0,
        actual_set_size: 0,
        branches_count: 0,
        rejected_as_dependents_count: 0,
        optimal_rejection_count: 0,
        conflict_map_conflicts_count: 0,
        rejection_options_count: 0,
        branches_time: std::time::Duration::ZERO,
        conflicts_map_time: std::time::Duration::ZERO,
        rejection_options_time: std::time::Duration::ZERO,
    }
}

// ─── The ledger (C3, C4, LOW-4, HIGH-1) ────────────────────────────────────

#[test]
fn ledger_prevents_compose_overflow() {
    let world = WorldBuilder::new()
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .block(0x13, 1, &[])
        .chain(
            ChainSpec::new(0x41, 0x11, 1)
                .cost(3)
                .number(0x61, i64::MAX, MergeType::IntegerAdd)
                .build(),
        )
        .chain(
            ChainSpec::new(0x42, 0x12, 1)
                .cost(2)
                .number(0x61, 1, MergeType::IntegerAdd)
                .build(),
        )
        .chain(
            ChainSpec::new(0x43, 0x13, 1)
                .cost(1)
                .number(0x61, -1, MergeType::IntegerAdd)
                .build(),
        )
        .build();
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the ordered pass never aborts");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x42]));
    let (resolved, _, witnesses) =
        run_ordered_pass(&world, &world.all_chains(), &[], &HashSet::new());
    assert!(matches!(
        witness_of(&witnesses, 0x42),
        Some(Witness::Ledger)
    ));
    let kept: Vec<&HashableSet<DeployChainIndex>> = resolved.to_merge.iter().collect();
    let ledger_of_kept = PurseLedger::from_branches(kept.iter().copied());
    assert!(ledger_of_kept
        .first_failure(&mut |_| Ok(0))
        .expect("no read")
        .is_none());
    // The control: the kept diffs fold in every order without overflow,
    // while the full set's net total fits but its fold overflows.
    assert!(merging_logic::combine_mergeable_value(i64::MAX, 1, MergeType::IntegerAdd).is_none());
    assert_eq!(i128::from(i64::MAX) + 1 - 1, i128::from(i64::MAX));
}

/// Control (C3): a branch whose net total fits can still overflow its chain
/// fold. The sign split catches it.
#[test]
fn branch_net_totals_miss_chain_overflow() {
    let first = ChainSpec::new(0x41, 0x11, 1)
        .number(0x61, i64::MAX, MergeType::IntegerAdd)
        .produces(0x31, 1)
        .build();
    let second = ChainSpec::new(0x42, 0x12, 2)
        .number(0x61, 1, MergeType::IntegerAdd)
        .consumed(produce(0x31, 1))
        .produces(0x32, 2)
        .build();
    let third = ChainSpec::new(0x43, 0x13, 3)
        .number(0x61, -1, MergeType::IntegerAdd)
        .consumed(produce(0x32, 2))
        .build();
    let branch = HashableSet(HashSet::from([first.clone(), second.clone(), third]));
    let net: i128 = branch
        .0
        .iter()
        .map(|chain| i128::from(chain.event_log_index.number_channels_data[&hash32(0x61)].0))
        .sum();
    assert!(net <= i128::from(i64::MAX), "the branch's net total fits");
    assert!(
        !ledger::branch_valid(&branch),
        "the sign split exceeds i64::MAX"
    );
    assert!(
        EventLogIndex::combine(&first.event_log_index, &second.event_log_index).is_err(),
        "the chain fold overflows"
    );
}

/// C4: a branch whose folds overflow aborts dev's merge, and the ordered pass
/// rejects it instead.
#[test]
fn branch_precheck_rejects_without_error() {
    let world = WorldBuilder::new()
        .block(0x11, 1, &[])
        .block(0x12, 2, &[0x11])
        .chain(
            ChainSpec::new(0x41, 0x11, 1)
                .number(0x61, i64::MAX, MergeType::IntegerAdd)
                .produces(0x31, 1)
                .build(),
        )
        .chain(
            ChainSpec::new(0x42, 0x12, 2)
                .number(0x61, 1, MergeType::IntegerAdd)
                .consumed(produce(0x31, 1))
                .build(),
        )
        .build();
    assert!(
        world.merge(MergeRule::Dev).is_err(),
        "dev's folds abort the merge"
    );
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the ordered pass rejects the branch");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x41, 0x42]));
    let (_, _, witnesses) = run_ordered_pass(&world, &world.all_chains(), &[], &HashSet::new());
    assert!(matches!(
        witness_of(&witnesses, 0x41),
        Some(Witness::InvalidBranch)
    ));
}

/// LOW-4: the pre-check bounds the user parts and the system parts
/// separately. A bound on the combined parts alone misses the user fold.
#[test]
fn branch_valid_bounds_user_and_system_parts() {
    let first = ChainSpec::new(0x41, 0x11, 1)
        .number(0x61, i64::MAX, MergeType::IntegerAdd)
        .system_number(0x61, -i64::MAX)
        .produces(0x31, 1)
        .build();
    let second = ChainSpec::new(0x42, 0x12, 2)
        .number(0x61, 1, MergeType::IntegerAdd)
        .consumed(produce(0x31, 1))
        .build();
    assert_eq!(
        first.event_log_index.number_channels_data[&hash32(0x61)].0,
        0
    );
    let branch = HashableSet(HashSet::from([first.clone(), second.clone()]));
    assert!(!ledger::branch_valid(&branch));
    let combined_only = branch
        .0
        .iter()
        .fold(SignSplit::default(), |mut split, chain| {
            let diff = chain.event_log_index.number_channels_data[&hash32(0x61)].0;
            match diff > 0 {
                true => split.pos += i128::from(diff),
                false => split.neg += i128::from(diff),
            }
            split
        });
    assert!(combined_only.fits(), "the combined parts alone fit");
    assert!(
        EventLogIndex::combine(&first.user_event_log_index, &second.user_event_log_index).is_err(),
        "the user fold of compute_branch_derived overflows"
    );
    let balanced = ChainSpec::new(0x43, 0x13, 1)
        .number(0x61, 5, MergeType::IntegerAdd)
        .system_number(0x61, -2)
        .build();
    assert!(ledger::branch_valid(&HashableSet(HashSet::from([
        balanced
    ]))));
}

#[test]
fn cross_branch_mergetype_mismatch_rejected() {
    let adding = ChainSpec::new(0x41, 0x11, 1)
        .cost(2)
        .number(0x61, 1, MergeType::IntegerAdd)
        .build();
    let masking = ChainSpec::new(0x42, 0x12, 1)
        .cost(1)
        .number(0x61, 1, MergeType::BitmaskOr)
        .build();
    let mut purse_ledger = PurseLedger::default();
    assert!(purse_ledger.try_add(&HashableSet(HashSet::from([adding.clone()]))));
    assert!(!purse_ledger.try_add(&HashableSet(HashSet::from([masking.clone()]))));
    let world = WorldBuilder::new()
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .chain(adding.clone())
        .chain(masking.clone())
        .build();
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the ordered pass never aborts");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x42]));
    // The control: composing both aborts on the mismatch.
    let both = resolved_with(vec![vec![adding], vec![masking]]);
    assert!(compose::compose(&world.repository, &world.base_state, &both).is_err());
}

/// HIGH-1: with a negative base and only positive contributors, a repair that
/// drops only negative contributors would stall. The fallback drops any
/// contributor until the channel has none.
#[test]
fn negative_base_repair_drops_any_contributor() {
    let channel = hash32(0x61);
    let world = WorldBuilder::new()
        .base_data(0x61, vec![number_datum(&channel, -5, 1)])
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .chain(
            ChainSpec::new(0x41, 0x11, 1)
                .cost(2)
                .number(0x61, 1, MergeType::IntegerAdd)
                .build(),
        )
        .chain(
            ChainSpec::new(0x42, 0x12, 1)
                .cost(1)
                .number(0x61, 2, MergeType::IntegerAdd)
                .build(),
        )
        .build();
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the repair loop ends");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x41, 0x42]));
    let (_, _, witnesses) = run_ordered_pass(&world, &world.all_chains(), &[], &HashSet::new());
    for deploy in [0x41, 0x42] {
        assert!(matches!(
            witness_of(&witnesses, deploy),
            Some(Witness::Balance(ch, Failure::Negative)) if *ch == channel
        ));
    }
    // The control: a sign-only repair finds no victim.
    let mut kept: Vec<Branch<DeployChainIndex>> =
        world.all_chains().iter().map(|c| candidate(&[c])).collect();
    sort_by_k(&mut kept, &HashSet::new());
    let sign_only = last_by_k(&kept, &HashSet::new(), |chain| {
        chain
            .event_log_index
            .number_channels_data
            .get(&channel)
            .is_some_and(|(diff, _)| *diff < 0)
    });
    assert_eq!(sign_only, None, "no negative contributor exists");
}

// ─── The order K ───────────────────────────────────────────────────────────

#[test]
fn k_pinned_then_losses_then_height_then_dev_order() {
    let stamp = |mut chain: DeployChainIndex, losses: u64| {
        chain.prior_rejections = losses;
        chain
    };
    let pinned_chain = stamp(ChainSpec::new(0x51, 0x11, 2).build(), 0);
    let lossy = stamp(ChainSpec::new(0x52, 0x12, 2).build(), 2);
    let low = stamp(ChainSpec::new(0x53, 0x13, 1).build(), 0);
    let costly = stamp(ChainSpec::new(0x54, 0x14, 2).cost(5).build(), 0);
    let cheap = stamp(ChainSpec::new(0x55, 0x15, 2).cost(3).build(), 0);
    let pinned = HashSet::from([pinned_chain.clone()]);
    let mut candidates = vec![
        candidate(&[&cheap]),
        candidate(&[&costly]),
        candidate(&[&low]),
        candidate(&[&lossy]),
        candidate(&[&pinned_chain]),
    ];
    sort_by_k(&mut candidates, &pinned);
    let order: Vec<u8> = candidates
        .iter()
        .map(|c| {
            c.0.iter()
                .next()
                .expect("one chain")
                .deploys_with_cost
                .0
                .iter()
                .next()
                .expect("one deploy")
                .deploy_id[0]
        })
        .collect();
    assert_eq!(order, vec![0x51, 0x52, 0x53, 0x54, 0x55]);
    // The maximum loss outranks the loss sum.
    let one_a = stamp(ChainSpec::new(0x56, 0x16, 1).build(), 1);
    let one_b = stamp(ChainSpec::new(0x57, 0x17, 1).build(), 1);
    let two = stamp(ChainSpec::new(0x58, 0x18, 1).build(), 2);
    let pair = candidate(&[&one_a, &one_b]);
    let single = candidate(&[&two]);
    assert_eq!(
        k_cmp(&single, &pair, &HashSet::new()),
        std::cmp::Ordering::Less
    );
    // K is strict on distinct candidates.
    assert_ne!(
        k_cmp(
            &candidate(&[&costly]),
            &candidate(&[&cheap]),
            &HashSet::new()
        ),
        std::cmp::Ordering::Equal
    );
}

// ─── The overfill dry run (LOW-3) ──────────────────────────────────────────

#[test]
fn overfill_dry_run_matches_apply_time_guard() {
    let cell = hash32(0x70);
    let base = number_datum(&cell, 1, 1);
    let replace = |deploy: u8, block: u8, seed: u8| {
        ChainSpec::new(deploy, block, 1)
            .change(0x70, vec![base.clone()], vec![number_datum(
                &cell,
                i64::from(seed),
                seed,
            )])
            .build()
    };
    let first = replace(0x61, 0x11, 2);
    let second = replace(0x62, 0x12, 3);
    let world = WorldBuilder::new()
        .base_data(0x70, vec![base.clone()])
        .block(0x11, 1, &[])
        .block(0x12, 1, &[])
        .chain(first.clone())
        .chain(second.clone())
        .build();
    let reader = world.reader();
    let one = HashableSet(HashSet::from([first.clone()]));
    let two = HashableSet(HashSet::from([second.clone()]));
    assert_eq!(
        compose::first_overfill(&[&one, &two], &reader).expect("read"),
        Some(cell.clone())
    );
    assert!(compose::compose(
        &world.repository,
        &world.base_state,
        &resolved_with(vec![vec![first.clone()], vec![second]])
    )
    .is_err());
    assert_eq!(
        compose::first_overfill(&[&one], &reader).expect("read"),
        None
    );
    assert!(compose::compose(
        &world.repository,
        &world.base_state,
        &resolved_with(vec![vec![first]])
    )
    .is_ok());

    // A produce, then a consume, inside one branch: the normalized fold
    // nets the intermediate datum, and so does the apply-time guard.
    let middle = number_datum(&cell, 5, 5);
    let end = number_datum(&cell, 6, 6);
    let producer = ChainSpec::new(0x63, 0x13, 1)
        .change(0x70, vec![base.clone()], vec![middle.clone()])
        .produces(0x70, 9)
        .build();
    let consumer = ChainSpec::new(0x64, 0x14, 2)
        .change(0x70, vec![middle.clone()], vec![end.clone()])
        .consumed(produce(0x70, 9))
        .build();
    let branch = HashableSet(HashSet::from([producer.clone(), consumer.clone()]));
    assert_eq!(
        compose::first_overfill(&[&branch], &reader).expect("read"),
        None
    );
    assert!(compose::compose(
        &world.repository,
        &world.base_state,
        &resolved_with(vec![vec![producer, consumer]])
    )
    .is_ok());
    // The control: an unnormalized sum, as dev's keep-one counts it
    // (dag_merger.rs:462-466), sees two values on the cell.
    let kept_base = StateChange::multiset_diff(&[base.clone()], &[base, middle.clone()]);
    assert_eq!(kept_base.len() + [middle, end].len(), 2);
}

// ─── Ordered-pass scenarios (MEDIUM-1, LOW-5, L8, L9) ──────────────────────

#[test]
fn partial_lineage_conflict_is_rechecked() {
    let late = ChainSpec::new(0x71, 0x21, 1).late().build();
    let producer = ChainSpec::new(0x72, 0x22, 1)
        .cost(2)
        .produces(0x40, 1)
        .build();
    let stale = ChainSpec::new(0x73, 0x23, 2)
        .cost(2)
        .consumed(produce(0x40, 1))
        .build();
    let listener = ChainSpec::new(0x74, 0x24, 1)
        .cost(1)
        .consumes(0x40, 2)
        .build();
    let world = WorldBuilder::new()
        .block(0x21, 1, &[])
        .block(0x22, 1, &[])
        .block(0x23, 2, &[0x21])
        .block(0x24, 1, &[])
        .chain(late.clone())
        .chain(producer.clone())
        .chain(stale.clone())
        .chain(listener.clone())
        .build();
    let actual: Vec<DeployChainIndex> = vec![producer.clone(), stale, listener.clone()];
    let (resolved, _, witnesses) = run_ordered_pass(&world, &actual, &[late], &HashSet::new());
    assert_eq!(kept_ids(&resolved), BTreeSet::from([0x72]));
    assert!(matches!(witness_of(&witnesses, 0x71), Some(Witness::Late)));
    assert!(
        matches!(witness_of(&witnesses, 0x73), Some(Witness::StaleLineage(ancestor)) if *ancestor == block_hash(0x21))
    );
    assert!(matches!(
        witness_of(&witnesses, 0x74),
        Some(Witness::LineageConflict(_))
    ));
    // The control: without the re-check the shrunken producer and the
    // listener would both stay, and they conflict.
    let shrunk = HashableSet(HashSet::from([
        candidate(&[&producer]),
        candidate(&[&listener]),
    ]));
    let map = dev_conflict_map(&shrunk).expect("the map builds");
    assert!(map.values().any(|others| !others.0.is_empty()));
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the merge succeeds");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x71, 0x73, 0x74]));
    assert_eq!(applied_ids(&merged), BTreeSet::from([0x72]));
}

#[test]
fn removal_created_overfill_is_repaired() {
    let cell = hash32(0x70);
    let first_value = number_datum(&cell, 3, 3);
    let late = ChainSpec::new(0x81, 0x31, 1).late().build();
    let writer = ChainSpec::new(0x82, 0x32, 1)
        .cost(2)
        .change(0x70, vec![], vec![first_value.clone()])
        .produces(0x70, 1)
        .build();
    let eraser = ChainSpec::new(0x83, 0x33, 2)
        .cost(2)
        .change(0x70, vec![first_value], vec![])
        .consumed(produce(0x70, 1))
        .build();
    let other = ChainSpec::new(0x84, 0x34, 1)
        .cost(1)
        .change(0x70, vec![], vec![number_datum(&cell, 4, 4)])
        .produces(0x70, 2)
        .build();
    let world = WorldBuilder::new()
        .block(0x31, 1, &[])
        .block(0x32, 1, &[])
        .block(0x33, 2, &[0x31])
        .block(0x34, 1, &[])
        .chain(late.clone())
        .chain(writer.clone())
        .chain(eraser.clone())
        .chain(other.clone())
        .build();
    let reader = world.reader();
    let before = HashableSet(HashSet::from([writer.clone(), eraser.clone()]));
    let other_set = HashableSet(HashSet::from([other.clone()]));
    assert_eq!(
        compose::first_overfill(&[&before, &other_set], &reader).expect("read"),
        None
    );
    let actual = vec![writer.clone(), eraser, other.clone()];
    let (resolved, _, witnesses) = run_ordered_pass(&world, &actual, &[late], &HashSet::new());
    assert_eq!(kept_ids(&resolved), BTreeSet::from([0x82]));
    assert!(matches!(witness_of(&witnesses, 0x84), Some(Witness::Overfill(ch)) if *ch == cell));
    // The control: after the lineage removal, the two writers overfill.
    assert!(compose::compose(
        &world.repository,
        &world.base_state,
        &resolved_with(vec![vec![writer], vec![other]])
    )
    .is_err());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the repaired merge composes");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0x81, 0x83, 0x84]));
}

/// L8: a pinned candidate wins a conflict against any unpinned candidate,
/// and it is dropped only as the last contributor in a repair.
#[test]
fn pinned_branch_kept_unless_forced() {
    let channel = hash32(0x61);
    let settled = ChainSpec::new(0x91, 0x41, 1)
        .cost(1)
        .consumed(produce(0x48, 0xFF))
        .build();
    let rich = ChainSpec::new(0x92, 0x42, 1)
        .cost(1000)
        .consumed(produce(0x48, 0xFF))
        .build();
    let world = WorldBuilder::new()
        .block(0x41, 1, &[])
        .block(0x42, 1, &[])
        .chain(settled.clone())
        .chain(rich.clone())
        .build();
    let pinned = HashSet::from([settled.clone()]);
    let (resolved, _, witnesses) =
        run_ordered_pass(&world, &[settled.clone(), rich.clone()], &[], &pinned);
    assert_eq!(kept_ids(&resolved), BTreeSet::from([0x91]));
    assert!(
        matches!(witness_of(&witnesses, 0x92), Some(Witness::Conflict(kept)) if kept.0.contains(&settled))
    );
    // Forced: a negative base needs every contributor dropped, unpinned first.
    let forced_pinned = ChainSpec::new(0x93, 0x43, 1)
        .number(0x61, 0, MergeType::IntegerAdd)
        .build();
    let unpinned = ChainSpec::new(0x94, 0x44, 1)
        .number(0x61, 0, MergeType::IntegerAdd)
        .build();
    let forced_world = WorldBuilder::new()
        .base_data(0x61, vec![number_datum(&channel, -1, 1)])
        .block(0x43, 1, &[])
        .block(0x44, 1, &[])
        .chain(forced_pinned.clone())
        .chain(unpinned.clone())
        .build();
    let pinned = HashSet::from([forced_pinned.clone()]);
    let (resolved, _, witnesses) =
        run_ordered_pass(&forced_world, &[forced_pinned, unpinned], &[], &pinned);
    assert!(kept_ids(&resolved).is_empty());
    assert!(matches!(
        witness_of(&witnesses, 0x93),
        Some(Witness::Balance(_, Failure::Negative))
    ));
    assert!(matches!(
        witness_of(&witnesses, 0x94),
        Some(Witness::Balance(_, Failure::Negative))
    ));
}

/// L9: the first unpinned candidate in K never loses to a later candidate's
/// conflict, however much the later candidate pays.
#[test]
fn first_unpinned_in_k_not_lost_to_later() {
    let first = ChainSpec::new(0xA1, 0x51, 1)
        .cost(1)
        .consumed(produce(0x48, 0xFE))
        .build();
    let costly = ChainSpec::new(0xA2, 0x52, 1)
        .cost(1000)
        .consumed(produce(0x48, 0xFE))
        .build();
    let also = ChainSpec::new(0xA3, 0x53, 1)
        .cost(500)
        .consumed(produce(0x48, 0xFE))
        .build();
    let world = WorldBuilder::new()
        .block(0x51, 1, &[])
        .block(0x52, 1, &[])
        .block(0x53, 1, &[])
        .losses(0xA1, 1)
        .chain(first)
        .chain(costly)
        .chain(also)
        .build();
    let (resolved, _, witnesses) =
        run_ordered_pass(&world, &world.all_chains(), &[], &HashSet::new());
    assert_eq!(kept_ids(&resolved), BTreeSet::from([0xA1]));
    for deploy in [0xA2, 0xA3] {
        assert!(matches!(
            witness_of(&witnesses, deploy),
            Some(Witness::Conflict(_))
        ));
    }
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the merge succeeds");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0xA2, 0xA3]));
}

/// LOW-5: an ordinary chain that conflicts with a settled chain of its own
/// branch escapes a branch-level check. The chain-level check sends the merge
/// to the slow path, where dev's settled partition rejects it.
#[test]
fn intra_branch_settled_conflict_takes_slow_path() {
    let settled = ChainSpec::new(0xB1, 0x61, 1)
        .produces(0x40, 1)
        .produces(0x45, 1)
        .build();
    let ordinary = ChainSpec::new(0xB2, 0x62, 2)
        .consumed(produce(0x45, 1))
        .consumes(0x40, 2)
        .build();
    let world = WorldBuilder::new()
        .block(0x61, 1, &[])
        .block(0x62, 2, &[0x61])
        .settled(0xB1)
        .chain(settled.clone())
        .chain(ordinary.clone())
        .build();
    assert!(fast::has_chain_level_conflict(&[
        settled.clone(),
        ordinary.clone()
    ]));
    let one_branch = HashableSet(HashSet::from([candidate(&[&settled, &ordinary])]));
    assert!(
        dev_conflict_map(&one_branch)
            .expect("map")
            .values()
            .all(|others| others.0.is_empty()),
        "the control: a branch-level check sees no conflict"
    );
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0xB2]));
}

// ─── The fast path (MEDIUM-2, MEDIUM-3, L5) ────────────────────────────────

/// MEDIUM-2: the availability walk is order-dependent, and the slow path
/// walks stamped chains. The fast path stamps them too, so it falls back
/// exactly where the slow path's walk rejects.
#[test]
fn fast_path_stamps_chains_like_the_slow_path() {
    let cell = hash32(0x70);
    let base = number_datum(&cell, 1, 1);
    let remover = ChainSpec::new(0xC1, 0x71, 1)
        .cost(3)
        .change(0x70, vec![base.clone()], vec![])
        .produces(0x46, 1)
        .build();
    let adder = ChainSpec::new(0xC2, 0x72, 1)
        .cost(2)
        .change(0x70, vec![], vec![number_datum(&cell, 2, 2)])
        .produces(0x47, 1)
        .build();
    let joiner = ChainSpec::new(0xC3, 0x73, 2)
        .cost(1)
        .consumed(produce(0x46, 1))
        .consumed(produce(0x47, 1))
        .build();
    let world = WorldBuilder::new()
        .base_data(0x70, vec![base])
        .block(0x71, 1, &[])
        .block(0x72, 1, &[])
        .block(0x73, 2, &[0x71, 0x72])
        .losses(0xC2, 1)
        .chain(remover.clone())
        .chain(adder.clone())
        .chain(joiner.clone())
        .build();
    let reader = world.reader();
    let unstamped = HashableSet(HashSet::from([remover, adder, joiner]));
    let (_, unavailable) = walk_branch(unstamped, &super::chain_depends, &reader).expect("walk");
    assert!(
        unavailable.0.is_empty(),
        "the control: the unstamped walk rejects nothing"
    );
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0xC2, 0xC3]));
}

/// MEDIUM-3: the drift guard. When dev's merge rejects nothing, the fast
/// path's copied composition gives dev's state and applied set.
#[test]
fn fast_path_equals_dev_merge_when_dev_rejects_nothing() {
    let first_number = hash32(0x62);
    let second_number = hash32(0x65);
    let world = WorldBuilder::new()
        .base_data(0x62, vec![number_datum(&first_number, 5, 1)])
        .base_data(0x65, vec![number_datum(&second_number, 4, 2)])
        .block(0x81, 1, &[])
        .block(0x82, 1, &[])
        .block(0x83, 2, &[0x81])
        .chain(
            ChainSpec::new(0xD1, 0x81, 1)
                .number(0x62, 3, MergeType::IntegerAdd)
                .change(0x62, vec![number_datum(&first_number, 5, 1)], vec![
                    number_datum(&first_number, 8, 3),
                ])
                .change(0x63, vec![], vec![vec![0xD1]])
                .produces(0x68, 1)
                .build(),
        )
        .chain(
            ChainSpec::new(0xD2, 0x82, 1)
                .number(0x65, -2, MergeType::IntegerAdd)
                .change(0x65, vec![number_datum(&second_number, 4, 2)], vec![
                    number_datum(&second_number, 2, 4),
                ])
                .change(0x66, vec![], vec![vec![0xD2]])
                .build(),
        )
        .chain(
            ChainSpec::new(0xD3, 0x83, 2)
                .consumed(produce(0x68, 1))
                .change(0x67, vec![], vec![vec![0xD3]])
                .build(),
        )
        .build();
    let dev = world.merge(MergeRule::Dev).expect("dev merges");
    assert!(dev.1.is_empty(), "the precondition: dev rejects nothing");
    let composed = world.fast().expect("the fast path runs").expect("P holds");
    assert_eq!(
        composed.state, dev.0,
        "the copied composition gives dev's root"
    );
    assert_eq!(composed.applied_user_sigs, dev.3);
    let v6 = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    assert_eq!(v6.0, dev.0);
    assert!(v6.1.is_empty());
    assert_eq!(v6.3, dev.3);
}

// ─── Property tests (256 cases each) ───────────────────────────────────────

/// One chain of a generated scenario.
#[derive(Clone, Debug)]
struct ChainDraw {
    cost: u64,
    parent: Option<usize>,
    consumes_parent: bool,
    produce_on: Option<u8>,
    consume_on: Option<u8>,
    race_on: Option<u8>,
    numbers: Vec<(u8, i64)>,
    system_number: Option<(u8, i64)>,
    replaces_cell: bool,
    losses: u64,
    settled: bool,
    late: bool,
}

/// A generated scenario: two to five chains and the base values of three
/// number channels (absent, negative or positive).
#[derive(Clone, Debug)]
struct Scenario {
    chains: Vec<ChainDraw>,
    base_numbers: Vec<i64>,
}

fn diff_strategy() -> impl Strategy<Value = i64> {
    prop_oneof![
        8 => -3i64..=3,
        1 => Just(i64::MAX),
        1 => Just(i64::MIN + 1),
    ]
}

fn chain_draw() -> impl Strategy<Value = ChainDraw> {
    (
        1u64..=4,
        proptest::option::of(0usize..4),
        any::<bool>(),
        proptest::option::of(0u8..3),
        proptest::option::of(0u8..3),
        proptest::option::of(0u8..2),
        proptest::collection::vec((0u8..3, diff_strategy()), 0..3),
        proptest::option::of((0u8..3, -3i64..=3)),
        any::<bool>(),
        0u64..=2,
        proptest::bool::weighted(0.2),
        proptest::bool::weighted(0.15),
    )
        .prop_map(
            |(
                cost,
                parent,
                consumes_parent,
                produce_on,
                consume_on,
                race_on,
                numbers,
                system_number,
                replaces_cell,
                losses,
                settled,
                late,
            )| ChainDraw {
                cost,
                parent,
                consumes_parent,
                produce_on,
                consume_on,
                race_on,
                numbers,
                system_number,
                replaces_cell,
                losses,
                settled,
                late,
            },
        )
}

fn wild_scenario() -> impl Strategy<Value = Scenario> {
    (
        proptest::collection::vec(chain_draw(), 2..=5),
        proptest::collection::vec(-2i64..=3, 3),
    )
        .prop_map(|(chains, base_numbers)| Scenario {
            chains,
            base_numbers,
        })
}

/// A conflict-free variant of a scenario: no races, no waiting consumes, no
/// late chains, one cell writer and non-negative bases. With `signed`, the
/// number diffs keep their sign in -3..=3, so a canonical prefix can dip
/// below zero while the full sum stays in range.
fn calm(mut scenario: Scenario, signed: bool) -> Scenario {
    for (index, draw) in scenario.chains.iter_mut().enumerate() {
        draw.consume_on = None;
        draw.race_on = None;
        draw.late = false;
        draw.replaces_cell = draw.replaces_cell && index == 0;
        for (_, diff) in draw.numbers.iter_mut() {
            *diff = match signed {
                true => (*diff).clamp(-3, 3),
                false => (*diff).clamp(0, 3),
            };
        }
        if let Some((_, diff)) = draw.system_number.as_mut() {
            *diff = diff.abs();
        }
    }
    for value in scenario.base_numbers.iter_mut() {
        *value = value.abs();
    }
    scenario
}

fn scenario() -> impl Strategy<Value = Scenario> {
    prop_oneof![
        2 => wild_scenario(),
        1 => wild_scenario().prop_map(|s| calm(s, false)),
        1 => wild_scenario().prop_map(|s| calm(s, true)),
    ]
}

const CELL: u8 = 0x70;

/// The world of a scenario. Chain `i` has deploy `0x80 + i` and block
/// `0x10 + i`; a chain with an earlier parent sits at height 2 above the
/// parent's block, and may consume the parent's link produce.
fn scenario_world(scenario: &Scenario) -> (World, Vec<DeployChainIndex>) {
    let cell = hash32(CELL);
    let cell_base = number_datum(&cell, 1, 0xEF);
    let mut builder = WorldBuilder::new().base_data(CELL, vec![cell_base.clone()]);
    for (index, value) in scenario.base_numbers.iter().enumerate() {
        let channel = 0x60 + index as u8;
        if *value != 0 {
            builder =
                builder.base_data(channel, vec![number_datum(&hash32(channel), *value, 0xEE)]);
        }
    }
    let mut chains = Vec::with_capacity(scenario.chains.len());
    for (index, draw) in scenario.chains.iter().enumerate() {
        let deploy = 0x80 + index as u8;
        let block = 0x10 + index as u8;
        let parent = draw.parent.filter(|parent| *parent < index);
        let (height, parents) = match parent {
            Some(parent) => (2, vec![0x10 + parent as u8]),
            None => (1, Vec::new()),
        };
        builder = builder.block(block, height, &parents);
        let mut spec = ChainSpec::new(deploy, block, height)
            .cost(draw.cost)
            .produces(0x30 + index as u8, index as u8);
        if let (Some(parent), true) = (parent, draw.consumes_parent) {
            spec = spec.consumed(produce(0x30 + parent as u8, parent as u8));
        }
        if let Some(channel) = draw.produce_on {
            spec = spec.produces(0x40 + channel, deploy);
        }
        if let Some(channel) = draw.consume_on {
            spec = spec.consumes(0x40 + channel, deploy);
        }
        if let Some(race) = draw.race_on {
            spec = spec.consumed(produce(0x48 + race, 0xFF));
        }
        for (channel, diff) in &draw.numbers {
            // A settled chain keeps small diffs: dev folds the settled chains' logs
            // before any adjudication (dag_merger.rs:1215-1222), and an overflow
            // there is a dev abort that the arbiter reported to the Casper team.
            let diff = match draw.settled {
                true => diff.clamp(&-3, &3),
                false => diff,
            };
            spec = spec.number(0x60 + channel, *diff, MergeType::IntegerAdd);
        }
        if let Some((channel, diff)) = draw.system_number {
            spec = spec.system_number(0x60 + channel, diff);
        }
        if draw.replaces_cell {
            spec = spec.change(CELL, vec![cell_base.clone()], vec![number_datum(
                &cell,
                10 + index as i64,
                deploy,
            )]);
        }
        if draw.late {
            spec = spec.late();
        }
        let Some(chain) = spec.try_build() else {
            continue;
        };
        if draw.losses > 0 {
            builder = builder.losses(deploy, draw.losses);
        }
        if draw.settled {
            builder = builder.settled(deploy);
        }
        chains.push(chain.clone());
        builder = builder.chain(chain);
    }
    (builder.build(), chains)
}

/// The direct inputs of the ordered pass for a scenario: the stamped
/// non-late chains, the late chains and the settled chains.
fn pass_inputs(
    world: &World,
    chains: &[DeployChainIndex],
) -> (
    Vec<DeployChainIndex>,
    Vec<DeployChainIndex>,
    HashSet<DeployChainIndex>,
) {
    let mut stamped = chains.to_vec();
    dag_merger::stamp_prior_rejections(&mut stamped, &world.prior_rejection_counts);
    let (actual, late): (Vec<_>, Vec<_>) = stamped.into_iter().partition(|chain| {
        chain
            .deploy_windows
            .values()
            .all(|valid_after| *valid_after > FLOOR - LIFESPAN)
    });
    let pinned = actual
        .iter()
        .filter(|chain| {
            chain
                .deploys_with_cost
                .0
                .iter()
                .all(|d| world.settled.contains(&d.deploy_id))
        })
        .cloned()
        .collect();
    (actual, late, pinned)
}

fn kept_shape(resolved: &ResolvedConflicts<DeployChainIndex>) -> BTreeSet<BTreeSet<u8>> {
    resolved
        .to_merge
        .iter()
        .map(|branch| {
            branch
                .0
                .iter()
                .flat_map(|chain| chain.deploys_with_cost.0.iter())
                .map(|deploy| deploy.deploy_id[0])
                .collect()
        })
        .collect()
}

fn rejected_shape(resolved: &ResolvedConflicts<DeployChainIndex>) -> BTreeSet<u8> {
    resolved
        .rejected
        .0
        .iter()
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .map(|deploy| deploy.deploy_id[0])
        .collect()
}

/// The survivors of the pass are a valid kept set: pairwise free of
/// conflicts and folded mixing, every sign split fits, every final balance
/// is in range, the guard flags nothing, and compose succeeds.
fn assert_valid_kept_set(world: &World, resolved: &ResolvedConflicts<DeployChainIndex>) {
    let kept: Vec<Branch<DeployChainIndex>> =
        resolved.to_merge.iter().cloned().map(Arc::new).collect();
    let map =
        dev_conflict_map(&HashableSet(kept.iter().cloned().collect())).expect("the map builds");
    assert!(
        map.values().all(|others| others.0.is_empty()),
        "survivors conflict"
    );
    let mut folded_claims = FoldedClaims::default();
    let mut purse_ledger = PurseLedger::default();
    for candidate in &kept {
        assert!(
            !folded_claims.mixes(candidate),
            "survivors mix folded and plain changes"
        );
        folded_claims.add(candidate);
        assert!(
            purse_ledger.try_add(candidate),
            "survivors break a sign split"
        );
    }
    let reader = world.reader();
    assert!(
        purse_ledger
            .first_failure(&mut |channel| fast::base_number(&reader, channel))
            .expect("read")
            .is_none(),
        "a survivor balance is out of range"
    );
    let sets: Vec<&HashableSet<DeployChainIndex>> = resolved.to_merge.iter().collect();
    assert_eq!(
        compose::first_overfill(&sets, &reader).expect("read"),
        None,
        "survivors overfill"
    );
    compose::compose(&world.repository, &world.base_state, resolved).expect("survivors compose");
}

/// The candidates of S1 to S3, recomputed: the late chains and their
/// dependents leave, the rest form branches, and each valid branch keeps the
/// survivors of dev's availability walk. Sorted by K.
fn oracle_candidates(
    world: &World,
    actual: &[DeployChainIndex],
    late: &[DeployChainIndex],
    pinned: &HashSet<DeployChainIndex>,
) -> (Vec<Branch<DeployChainIndex>>, Vec<Branch<DeployChainIndex>>) {
    let reader = world.reader();
    let merge_set: HashSet<DeployChainIndex> = actual
        .iter()
        .filter(|chain| {
            !late
                .iter()
                .any(|late_chain| super::chain_depends(chain, late_chain))
        })
        .cloned()
        .collect();
    let branches: Vec<Branch<DeployChainIndex>> = dev_branches(&HashableSet(merge_set))
        .0
        .into_iter()
        .collect();
    let mut candidates = Vec::with_capacity(branches.len());
    for branch in &branches {
        if !ledger::branch_valid(branch) {
            continue;
        }
        let (survivors, _) =
            walk_branch((**branch).clone(), &super::chain_depends, &reader).expect("walk");
        if let Some(survivors) = survivors {
            candidates.push(Arc::new(survivors));
        }
    }
    sort_by_k(&mut candidates, pinned);
    (branches, candidates)
}

/// True when some chain of `left` and some chain of `right` conflict at
/// chain level or share a user deploy id. A conflict between the combined
/// logs of two sets implies such a pair, so this is a necessary condition of
/// dev's branch-level conflict.
fn cross_conflict(
    left: &HashableSet<DeployChainIndex>,
    right: &HashableSet<DeployChainIndex>,
) -> bool {
    left.0.iter().any(|a| {
        right.0.iter().any(|b| {
            fast::has_chain_level_conflict(&[a.clone(), b.clone()])
                || a.deploys_with_cost.0.iter().any(|d| {
                    b.deploys_with_cost
                        .0
                        .iter()
                        .any(|e| e.deploy_id == d.deploy_id)
                })
        })
    })
}

/// The sign split and merge-type condition of `try_add`, over `earlier`
/// followed by `candidate`: true when it fails. `try_add` is monotone, so a
/// failure against the kept set at the time implies this failure over every
/// earlier candidate.
fn ledger_fails(
    earlier: &[Branch<DeployChainIndex>],
    candidate: &HashableSet<DeployChainIndex>,
) -> bool {
    let mut types: HashMap<Blake2b256Hash, MergeType> = HashMap::new();
    let mut splits: HashMap<Blake2b256Hash, (i128, i128)> = HashMap::new();
    for branch in earlier
        .iter()
        .map(|b| &**b)
        .chain(std::iter::once(candidate))
    {
        for chain in branch.0.iter() {
            for (channel, (diff, merge_type)) in chain.event_log_index.number_channels_data.iter() {
                if *types.entry(channel.clone()).or_insert(*merge_type) != *merge_type {
                    return true;
                }
                let split = splits.entry(channel.clone()).or_default();
                match *diff > 0 {
                    true => split.0 += i128::from(*diff),
                    false => split.1 += i128::from(*diff),
                }
            }
        }
    }
    splits.iter().any(|(channel, (pos, neg))| {
        types[channel] == MergeType::IntegerAdd
            && (*pos > i128::from(i64::MAX) || *neg < i128::from(i64::MIN))
    })
}

/// The chains whose witness equals `witness_matches`.
fn witness_group(
    witnesses: &Rejections,
    witness_matches: impl Fn(&Witness) -> bool,
) -> HashableSet<DeployChainIndex> {
    HashableSet(
        witnesses
            .iter()
            .filter(|(_, witness)| witness_matches(witness))
            .map(|(chain, _)| chain.clone())
            .collect(),
    )
}

/// Each witness names a reason that held when its step rejected the chain.
/// S1 to S5 witnesses are checked exactly against the recomputed candidates.
/// S6 witnesses are checked through necessary conditions on the chains that
/// share the witness.
fn assert_witnesses_hold(
    world: &World,
    actual: &[DeployChainIndex],
    late: &[DeployChainIndex],
    pinned: &HashSet<DeployChainIndex>,
    resolved: &ResolvedConflicts<DeployChainIndex>,
    witnesses: &Rejections,
) {
    assert_eq!(
        witnesses.len(),
        resolved.rejected.0.len(),
        "one witness per rejected chain"
    );
    for chain in resolved.rejected.0.iter() {
        assert!(
            witnesses.contains_key(chain),
            "a rejected chain has no witness"
        );
    }
    let reader = world.reader();
    let (branches, candidates) = oracle_candidates(world, actual, late, pinned);
    let candidate_of =
        |chain: &DeployChainIndex| candidates.iter().position(|c| c.0.contains(chain));
    let branch_of = |chain: &DeployChainIndex| {
        branches
            .iter()
            .find(|b| b.0.contains(chain))
            .cloned()
            .expect("the chain has a branch")
    };
    for (chain, witness) in witnesses {
        assert!(
            !resolved
                .to_merge
                .iter()
                .any(|branch| branch.0.contains(chain)),
            "a rejected chain is also kept"
        );
        match witness {
            Witness::Late => assert!(late.contains(chain)),
            Witness::DependsOnLate => {
                assert!(late
                    .iter()
                    .any(|late_chain| super::chain_depends(chain, late_chain)))
            }
            Witness::InvalidBranch => assert!(!ledger::branch_valid(&branch_of(chain))),
            Witness::Unavailable => {
                let (_, unavailable) =
                    walk_branch((*branch_of(chain)).clone(), &super::chain_depends, &reader)
                        .expect("walk");
                assert!(unavailable.0.contains(chain));
            }
            Witness::Conflict(kept) => {
                let position = candidate_of(chain).expect("a conflict loser was a candidate");
                let ours = candidates[position].clone();
                assert_eq!(
                    k_cmp(kept, &ours, pinned),
                    std::cmp::Ordering::Less,
                    "the kept candidate comes first"
                );
                let pair = HashableSet(HashSet::from([ours.clone(), kept.clone()]));
                let map = dev_conflict_map(&pair).expect("map");
                assert!(
                    map[&ours].0.contains(kept),
                    "the witness names a conflicting candidate"
                );
            }
            Witness::Mixing => {
                let position = candidate_of(chain).expect("a mixing loser was a candidate");
                let mut earlier = FoldedClaims::default();
                for previous in &candidates[..position] {
                    earlier.add(previous);
                }
                assert!(earlier.mixes(&candidates[position]));
            }
            Witness::Ledger => {
                let position = candidate_of(chain).expect("a ledger loser was a candidate");
                assert!(ledger_fails(&candidates[..position], &candidates[position]));
            }
            Witness::StaleLineage(ancestor) => {
                assert!(!pinned.contains(chain));
                assert_ne!(*ancestor, chain.source_block_hash);
                assert!(witnesses
                    .keys()
                    .any(|rejected| rejected.source_block_hash == *ancestor));
                assert!(world
                    .dag
                    .is_dag_ancestor(ancestor, &chain.source_block_hash)
                    .expect("ancestry"));
            }
            Witness::LineageConflict(earlier) => {
                let group = witness_group(
                    witnesses,
                    |w| matches!(w, Witness::LineageConflict(e) if e == earlier),
                );
                assert!(
                    cross_conflict(earlier, &group),
                    "the re-check names a conflicting candidate"
                );
            }
            Witness::Overfill(channel) => {
                let group = witness_group(
                    witnesses,
                    |w| matches!(w, Witness::Overfill(c) if c == channel),
                );
                assert!(group.0.iter().any(|c| c
                    .state_changes
                    .datums_changes
                    .get(channel)
                    .is_some_and(|change| !change.added.is_empty())));
            }
            Witness::Balance(channel, failure) => {
                let group = witness_group(
                    witnesses,
                    |w| matches!(w, Witness::Balance(c, f) if c == channel && f == failure),
                );
                let diffs: Vec<i64> = group
                    .0
                    .iter()
                    .filter_map(|c| {
                        c.event_log_index
                            .number_channels_data
                            .get(channel)
                            .map(|(diff, _)| *diff)
                    })
                    .collect();
                assert!(
                    !diffs.is_empty(),
                    "a balance victim contributes to the channel"
                );
                if let Failure::Overflow = failure {
                    assert!(diffs.iter().any(|diff| *diff > 0));
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    /// L1: the result depends only on the set of chains, not on their order.
    #[test]
    fn ordered_pass_permutation_invariant(scenario in scenario()) {
        let (world, chains) = scenario_world(&scenario);
        let (actual, late, pinned) = pass_inputs(&world, &chains);
        let (forward, _, _) = run_ordered_pass(&world, &actual, &late, &pinned);
        let mut reversed_actual = actual.clone();
        reversed_actual.reverse();
        let mut reversed_late = late.clone();
        reversed_late.reverse();
        let (backward, _, _) = run_ordered_pass(&world, &reversed_actual, &reversed_late, &pinned);
        prop_assert_eq!(kept_shape(&forward), kept_shape(&backward));
        prop_assert_eq!(rejected_shape(&forward), rejected_shape(&backward));
    }

    /// L2, L3 and L4: the survivors are pairwise conflict-free, every check
    /// of the kept set holds, and compose succeeds, so the merge never aborts.
    #[test]
    fn survivors_conflict_free(scenario in scenario()) {
        let (world, chains) = scenario_world(&scenario);
        let (actual, late, pinned) = pass_inputs(&world, &chains);
        let (resolved, _, _) = run_ordered_pass(&world, &actual, &late, &pinned);
        assert_valid_kept_set(&world, &resolved);
    }

    /// L4: the repair loop ends with a valid set, and the full merge entry
    /// point never aborts under the v6 rule.
    #[test]
    fn repair_loop_terminates_valid(scenario in scenario()) {
        let (world, _) = scenario_world(&scenario);
        let merged = world.merge(MergeRule::OfferedV6);
        prop_assert!(merged.is_ok(), "the v6 merge aborted: {:?}", merged.err());
    }

    /// L5: whenever the fast path composes, the slow path returns the same
    /// state, no records and the same applied set.
    #[test]
    fn fast_path_equals_ordered_pass(scenario in scenario()) {
        let (world, _) = scenario_world(&scenario);
        if let Some(composed) = world.fast().expect("the fast path never errors here") {
            let slow = world.merge(MergeRule::OfferedV6).expect("the slow path merges");
            prop_assert!(slow.1.is_empty(), "the slow path rejected under P");
            prop_assert_eq!(composed.state, slow.0);
            prop_assert_eq!(composed.applied_user_sigs, slow.3);
        }
    }

    /// Every rejected chain has a witness whose reason holds.
    #[test]
    fn rejection_has_witness(scenario in scenario()) {
        let (world, chains) = scenario_world(&scenario);
        let (actual, late, pinned) = pass_inputs(&world, &chains);
        let (resolved, _, witnesses) = run_ordered_pass(&world, &actual, &late, &pinned);
        assert_witnesses_hold(&world, &actual, &late, &pinned, &resolved, &witnesses);
    }

    /// L9: the first unpinned candidate in K never loses a conflict to a
    /// later candidate. Every candidate before it is pinned, so a conflict
    /// witness against it names a pinned candidate.
    #[test]
    fn first_unpinned_conflict_loss_names_a_pinned_candidate(scenario in scenario()) {
        let (world, chains) = scenario_world(&scenario);
        let (actual, late, pinned) = pass_inputs(&world, &chains);
        let (_, _, witnesses) = run_ordered_pass(&world, &actual, &late, &pinned);
        let (_, candidates) = oracle_candidates(&world, &actual, &late, &pinned);
        if let Some(first_unpinned) = candidates.iter().find(|c| !super::order::is_pinned(c, &pinned)) {
            for chain in first_unpinned.0.iter() {
                if let Some(Witness::Conflict(kept)) = witnesses.get(chain) {
                    prop_assert!(super::order::is_pinned(kept, &pinned));
                }
            }
        }
    }
}

/// P4: a scope chain that conflicts with the base's own content never
/// composes on the fast path. Dev's base partition rejects it on the slow
/// path (dag_merger.rs:1332-1347).
#[test]
fn base_conflict_takes_slow_path() {
    let base_side = ChainSpec::new(0xE1, BASE, 1)
        .consumed(produce(0x49, 0xFD))
        .build();
    let scope_side = ChainSpec::new(0xE2, 0x91, 1)
        .consumed(produce(0x49, 0xFD))
        .build();
    let world = WorldBuilder::new()
        .block(0x91, 1, &[])
        .base_chain(base_side)
        .chain(scope_side)
        .build();
    assert!(world.fast().expect("the fast path runs").is_none());
    let merged = world
        .merge(MergeRule::OfferedV6)
        .expect("the slow path merges");
    assert_eq!(rejected_ids(&merged), BTreeSet::from([0xE2]));
}

/// Anti-vacuity guard for the property tests: the generator must reach the
/// fast path, the slow path's rejections and each repair step often enough
/// that the properties above test something. It also counts the scenarios
/// where the fast path composes but dev's merge would reject or abort.
#[test]
fn generated_scenarios_exercise_both_paths() {
    let mut runner = TestRunner::deterministic();
    let mut fast_composed = 0usize;
    let mut fast_where_dev_rejects = 0usize;
    let mut fast_where_dev_aborts = 0usize;
    let mut slow_rejects = 0usize;
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for _ in 0..256 {
        let scenario = scenario()
            .new_tree(&mut runner)
            .expect("a scenario generates")
            .current();
        let (world, chains) = scenario_world(&scenario);
        if let Some(composed) = world.fast().expect("the fast path runs") {
            fast_composed += 1;
            match world.merge(MergeRule::Dev) {
                Ok(dev) if dev.1.is_empty() => {
                    assert_eq!(composed.state, dev.0, "the fast path drifted from dev");
                    assert_eq!(composed.applied_user_sigs, dev.3);
                }
                Ok(dev) => {
                    fast_where_dev_rejects += 1;
                    eprintln!(
                        "fast path composes where dev rejects {:?}: {scenario:?}",
                        rejected_ids(&dev)
                    );
                }
                Err(_) => fast_where_dev_aborts += 1,
            }
        }
        let (actual, late, pinned) = pass_inputs(&world, &chains);
        let (_, _, witnesses) = run_ordered_pass(&world, &actual, &late, &pinned);
        if !witnesses.is_empty() {
            slow_rejects += 1;
        }
        for witness in witnesses.values() {
            let kind = match witness {
                Witness::Late => "late",
                Witness::DependsOnLate => "depends-on-late",
                Witness::InvalidBranch => "invalid-branch",
                Witness::Unavailable => "unavailable",
                Witness::Conflict(_) => "conflict",
                Witness::Mixing => "mixing",
                Witness::Ledger => "ledger",
                Witness::StaleLineage(_) => "stale-lineage",
                Witness::LineageConflict(_) => "lineage-conflict",
                Witness::Overfill(_) => "overfill",
                Witness::Balance(..) => "balance",
            };
            *kinds.entry(kind).or_default() += 1;
        }
    }
    eprintln!(
        "generated scenarios: fast={fast_composed} fast-dev-rejects={fast_where_dev_rejects} \
         fast-dev-aborts={fast_where_dev_aborts} slow-rejects={slow_rejects} kinds={kinds:?}"
    );
    assert!(
        fast_composed >= 16,
        "the generator rarely reaches the fast path"
    );
    assert!(
        slow_rejects >= 64,
        "the generator rarely makes the pass reject"
    );
    for kind in [
        "late",
        "conflict",
        "unavailable",
        "stale-lineage",
        "overfill",
        "balance",
        "ledger",
    ] {
        assert!(
            kinds.get(kind).copied().unwrap_or(0) > 0,
            "no {kind} witness in 256 scenarios"
        );
    }
}
