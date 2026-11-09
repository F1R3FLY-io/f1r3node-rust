//! P5d integration tests of the Array and Vector outer stores and of their allocator (P5 design
//! v3 A.10, P5 design v2 section 3.3, F7, F8 and F11).
//!
//! Most tests run the agents of the families `array:queue` and `vector:queue` directly
//! (`GenericSpace<ArrayOuter, QueueDiscipline>` and `GenericSpace<VectorOuter, QueueDiscipline>`)
//! over an in-memory RSpace with the shared harness of P5 (`support::agent`). The creation of a
//! space stages the allocator `A(sid) = 0`, and each allocation job takes it and puts its next
//! value. A collection of either store lives on the channels of the HashMap store (A5), and only
//! the index names of the space below its size reach it (F8). The test
//! `space_array_all_disciplines_supported` runs the family agent instead, so it covers the
//! dispatch arm of every wired Array and Vector family.
//!
//! The property tests restate the headlines of the Rocq modules `OuterStorage.v`,
//! `VectorStore.v` and `HashMapStore.v` and the invariant `NoDuplicateIndex` of
//! `CounterConflict.tla`, one property for each. A model of the allocator is the oracle: the
//! rules of `ALLOC` in P5 design v3 A.10, computed in `u128` so that the oracle cannot overflow.
//!
//! The Rholang deploys that allocate index names run in `reified_hooks_p5.rs`: a `new` inside a
//! `use` block, a `NEWSPACE` annotation, `gensym` through a handle, and the fail-closed rule of
//! native execution. The factories of the Array and Vector families run in
//! `reified_families.rs`.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostAuthority, CostRegion, CostSignature, Expr, ListParWithRandom, Par};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_gbool_par;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
    NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, NativeRuntimeConfig, RuntimeBudget,
};
// Disabled by decision 90 (DR-132): the scheduler module is gone.
// use rholang::rust::interpreter::deterministic_reduction::{root, ReductionCoordinator};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::runtime;
// Disabled by decision 90 (DR-132): only the scheduler test, now commented out, named it.
// use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::agent::{
    AllocOp, Allocation, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
};
use rholang::rust::interpreter::spaces::allocator::{self, AllocationRule};
// Changed by decision 90 (decision 69): only the retired `allocation_legacy_charges_equal_host`
// built the take of the allocator (Option P, removed with the legacy meter by DR-134).
// use rholang::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::cell::CellDiscipline;
use rholang::rust::interpreter::spaces::disciplines::priority_queue::PriorityQueueDiscipline;
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::disciplines::set::SetDiscipline;
use rholang::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::generic::{
    AllocCx, CollectionChannels, Discipline, GenericSpace, OuterStore,
};
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, NameTail, NativeSpace, RandomPurpose, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::array::{ArrayOuter, CYCLIC_PARAM, SIZE_PARAM};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::outer::vector::VectorOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    compute_related_sets, conflict_reason, conflicts, depends, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use shared::rust::hashable_set::HashableSet;
// Changed by decision 90 (C6, step D): the rigged replay modes retire, so this file no longer uses
// `play_and_replay` and `ParReplay`. The text before:
// ```
// use support::agent::{
//     any, budget, create, creation, data_channel, env_over, exactly, fired, int, play,
//     play_and_replay, receive_on, records_on, run, send_on, string, waiters_on, ParRSpace,
//     ParReplay,
// };
// ```
use support::agent::{
    any, budget, create, creation, data_channel, env_over, exactly, fired, int, play, receive_on,
    records_on, run, send_on, string, waiters_on, ParRSpace,
};
// Changed by decision 90 (decision 69): only the retired `allocation_legacy_charges_equal_host`
// ran a host program (Option P, removed with the legacy meter by DR-134).
// use support::legacy::{charging_host, host_consume, host_produce};
use support::props::{check, pure_config, tokio_runtime};

/// The agent of the family `array:queue`.
static ARRAY_QUEUE: GenericSpace<ArrayOuter, QueueDiscipline> =
    GenericSpace::new(ArrayOuter, QueueDiscipline);

/// The agent of the family `vector:queue`.
static VECTOR_QUEUE: GenericSpace<VectorOuter, QueueDiscipline> =
    GenericSpace::new(VectorOuter, QueueDiscipline);

/// The agent of the family `hashmap:queue`.
static HASHMAP_QUEUE: GenericSpace<HashMapOuter, QueueDiscipline> =
    GenericSpace::new(HashMapOuter, QueueDiscipline);

/// The space of every test.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x5D; SID_LEN],
    durability: Durability::Durable,
};

/// Another space, for the names that the store of [`SPACE`] must reject.
const OTHER: NativeSpace = NativeSpace {
    sid: [0x5E; SID_LEN],
    durability: Durability::Durable,
};

/// The store of a test space: a fixed or a cyclic Array space with its size, or a Vector space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Store {
    Fixed(i64),
    Cyclic(i64),
    Vector,
}

impl Store {
    /// The agent of the `queue` family of the store.
    fn agent(self) -> &'static dyn SpaceAgent {
        match self {
            Store::Fixed(_) | Store::Cyclic(_) => &ARRAY_QUEUE,
            Store::Vector => &VECTOR_QUEUE,
        }
    }

    /// The decoded config of a space of the store.
    fn space_config(self, space: NativeSpace) -> SpaceConfig {
        let outer = match self {
            Store::Fixed(_) | Store::Cyclic(_) => ArrayOuter::NAME,
            Store::Vector => VectorOuter::NAME,
        };
        let mut config = SpaceConfig::new(
            space.sid,
            outer,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        );
        match self {
            Store::Fixed(size) => {
                config.params.insert(SIZE_PARAM.to_string(), int(size));
            }
            Store::Cyclic(size) => {
                config.params.insert(SIZE_PARAM.to_string(), int(size));
                config.params.insert(
                    CYCLIC_PARAM.to_string(),
                    new_gbool_par(true, Vec::new(), false),
                );
            }
            Store::Vector => {}
        }
        config
    }

    /// The config map of [`SPACE`] with the store.
    fn config(self) -> Par { self.space_config(SPACE).to_par() }

    /// The family of the factory of the store.
    fn family(self) -> SpaceFactoryUrn {
        let outer = match self {
            Store::Fixed(_) | Store::Cyclic(_) => SpaceOuter::Array,
            Store::Vector => SpaceOuter::Vector,
        };
        SpaceFactoryUrn {
            outer,
            inner: SpaceInner::Queue,
        }
    }

    /// The allocation rule of the store.
    fn rule(self) -> AllocationRule {
        match self {
            Store::Fixed(size) => AllocationRule::Fixed { size },
            Store::Cyclic(size) => AllocationRule::Cyclic { size },
            Store::Vector => AllocationRule::Unbounded,
        }
    }
}

/// The allocator model of `OuterStorage.v` and `VectorStore.v`: the indices and the next value of
/// an allocation of `count` names from the value `value`, or `None` for `OutOfNames`.
fn model(store: Store, value: i64, count: u64) -> Option<(Vec<u64>, i64)> {
    let start = u128::try_from(value).expect("an allocator value is natural");
    let count = u128::from(count);
    let narrow = |wide: u128| u64::try_from(wide).expect("an index fits in u64");
    let value_of = |wide: u128| i64::try_from(wide).expect("a value fits in i64");
    match store {
        Store::Fixed(size) => {
            let size = u128::try_from(size).expect("a positive size");
            (start + count <= size).then(|| {
                (
                    (start..start + count).map(narrow).collect(),
                    value_of(start + count),
                )
            })
        }
        Store::Cyclic(size) => {
            let size = u128::try_from(size).expect("a positive size");
            Some((
                (0..count).map(|i| narrow((start + i) % size)).collect(),
                value_of((start + count) % size),
            ))
        }
        Store::Vector => (start + count <= u128::from(u64::try_from(i64::MAX).expect("i64::MAX")))
            .then(|| {
                (
                    (start..start + count).map(narrow).collect(),
                    value_of(start + count),
                )
            }),
    }
}

/// A space of a store over a fresh in-memory RSpace, after its creation.
struct Fixture {
    space: ParRSpace,
    env: SpaceEnv,
    store: Store,
}

/// Creates a space of `store` in `space`, with `env`, and forgets the events of the creation.
async fn create_store(store: Store, space: &ParRSpace, env: &SpaceEnv) {
    create(
        store.agent(),
        env,
        creation(SPACE, store.config(), store.family()),
    )
    .await;
    space.take_event_log().await;
}

async fn fixture(store: Store) -> Fixture {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create_store(store, &space, &env).await;
    Fixture { space, env, store }
}

/// The random state of the allocation job `job`. Each job has its own.
fn job_random(job: u64) -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(format!("rrs-p5d allocation job {job}").as_bytes())
}

/// The allocation of `count` names of [`SPACE`] by the job `job`.
fn alloc_op(job: u64, count: u64) -> AllocOp {
    AllocOp {
        space: SPACE,
        count,
        random_state: job_random(job).to_bytes(),
        authority: CostAuthority::default(),
    }
}

/// Runs the allocation `op` with `agent` in one job and commits it.
async fn allocate_with(
    agent: &dyn SpaceAgent,
    env: &SpaceEnv,
    op: AllocOp,
) -> Result<Vec<u64>, SpaceError> {
    match agent.allocate(env, op).await? {
        Allocation::Indices(prepared) => prepared.commit().await,
        Allocation::Baseline => panic!("an Array or a Vector space has an allocator"),
    }
}

impl Fixture {
    async fn allocate(&self, job: u64, count: u64) -> Result<Vec<u64>, SpaceError> {
        allocate_with(self.store.agent(), &self.env, alloc_op(job, count)).await
    }

    /// The values of the allocator records of the space.
    async fn allocator(&self) -> Vec<i64> { allocator_values(&self.space).await }

    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(self.store.agent(), &self.env, op).await
    }
}

/// The values of the allocator records of [`SPACE`] in `space`.
async fn allocator_values(space: &ParRSpace) -> Vec<i64> {
    space
        .get_data(&allocator::allocator_channel(SPACE))
        .await
        .iter()
        .map(|datum| record::counter_value(&datum.a).expect("an allocator record"))
        .collect()
}

/// The index name `i` of [`SPACE`].
fn name(index: u64) -> Par { allocator::index_name(SPACE, index) }

/// The labels of the events of `log` on the allocator channel of [`SPACE`]: `produce A`,
/// `consume A` and `comm A`. Every other event is `other`.
fn allocator_labels(log: &[Event]) -> Vec<&'static str> {
    let channel = hash(&allocator::allocator_channel(SPACE));
    log.iter()
        .map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == channel => {
                "produce A"
            }
            Event::IoEvent(IOEvent::Consume(consume))
                if consume.channel_hashes == vec![channel.clone()] =>
            {
                "consume A"
            }
            Event::Comm(comm) if comm.consume.channel_hashes == vec![channel.clone()] => "comm A",
            Event::IoEvent(_) | Event::Comm(_) => "other",
        })
        .collect()
}

/// The identities of the produce and consume events of `log`, with their kind.
fn identities(log: &[Event]) -> Vec<(&'static str, Blake2b256Hash)> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => Some(("produce", produce.hash.clone())),
            Event::IoEvent(IOEvent::Consume(consume)) => Some(("consume", consume.hash.clone())),
            Event::Comm(_) => None,
        })
        .collect()
}

/// The integer of a payload of one integer.
fn value_of(pars: &[Par]) -> i64 {
    match pars {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => *value,
            other => panic!("a payload of one integer: {other:?}"),
        },
        other => panic!("a payload of one integer: {other:?}"),
    }
}

/// A send of `value` on `channel` of [`SPACE`] with the random state of the send `id`.
fn send_with_id(channel: &Par, value: i64, id: usize) -> ProduceOp {
    let mut op = send_on(SPACE, channel, value, false);
    op.data.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-p5d send {id}").as_bytes()).to_bytes();
    op
}

fn region(byte: u8) -> CostAuthority {
    CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![byte; 32],
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::Ground(vec![byte])),
            }),
            // Added by decision 90 (DR-135): an unpriced region, as outside native
            // execution.
            price: 0,
            caller: None,
        }],
    }
}

// ── The merge indices of two branches (the harness of reified_merge.rs) ──────────────────────

/// The merge index of a branch log over the pre-state `root` of `space`, with the pre-state
/// closures of `block_index.rs::create_event_log_index`.
fn merge_index(space: &ParRSpace, root: &Blake2b256Hash, log: Vec<Event>) -> EventLogIndex {
    let reader = space
        .get_history_repository()
        .get_history_reader(root)
        .expect("read the history at the pre-state root");
    let produce_exists_in_pre_state = |p: &rspace_plus_plus::rspace::trace::event::Produce| {
        reader
            .get_data(&p.channel_hash)
            .is_ok_and(|data| data.iter().any(|d| d.source == *p))
    };
    let produce_touches_pre_state_join = |p: &rspace_plus_plus::rspace::trace::event::Produce| {
        reader
            .get_joins(&p.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|j| j.len() > 1))
    };
    EventLogIndex::new(
        log,
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        NumberChannelsDiff::new(),
    )
}

/// The channels on which two branch logs conflict.
fn conflicting(
    space: &ParRSpace,
    root: &Blake2b256Hash,
    left: &[Event],
    right: &[Event],
) -> BTreeSet<Blake2b256Hash> {
    conflicts(
        &merge_index(space, root, left.to_vec()),
        &merge_index(space, root, right.to_vec()),
    )
    .0
    .into_iter()
    .collect()
}

/// `log` without the events on `channel`: the log of an allocator that the branches never share.
fn without_channel(log: &[Event], channel: &Blake2b256Hash) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash != *channel,
            Event::IoEvent(IOEvent::Consume(consume)) => !consume.channel_hashes.contains(channel),
            Event::Comm(comm) => !comm.consume.channel_hashes.contains(channel),
        })
        .cloned()
        .collect()
}

/// One branch: the allocations `counts` from the checkpoint `root`, with the jobs from `first`.
/// It returns the log and the indices of each allocation that succeeded.
async fn branch(
    fixture: &Fixture,
    root: &Blake2b256Hash,
    first: u64,
    counts: &[u64],
) -> (Vec<Event>, Vec<u64>) {
    fixture
        .space
        .reset(root)
        .await
        .expect("reset to the pre-state root");
    let mut indices = Vec::new();
    for (job, count) in (first..).zip(counts) {
        if let Ok(allocated) = fixture.allocate(job, *count).await {
            indices.extend(allocated);
        }
    }
    (fixture.space.take_event_log().await, indices)
}

// ── The creation and the allocation ───────────────────────────────────────────────────────────

#[tokio::test]
async fn space_array_creation_stages_the_allocator() {
    for store in [Store::Fixed(3), Store::Cyclic(2), Store::Vector] {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let authority = region(4);
        let made = SpaceCreation {
            authority: authority.clone(),
            ..creation(SPACE, store.config(), store.family())
        };
        create(store.agent(), &env, made.clone()).await;
        let log = space.take_event_log().await;
        assert_eq!(
            allocator_labels(&log),
            vec!["other", "other", "produce A", "other"],
            "{store:?}: the config, the space counter, the allocator and the handle"
        );
        let records = space.get_data(&allocator::allocator_channel(SPACE)).await;
        let [allocator_record] = records.as_slice() else {
            panic!("{store:?}: one allocator record: {records:?}");
        };
        assert!(!allocator_record.persist, "{store:?}");
        assert_eq!(
            allocator_record.a,
            record::counter_record(
                0,
                &RandomPurpose::Allocator.derive(&made.random).split_byte(1),
                Some(authority)
            ),
            "{store:?}: A(sid) = 0 with the allocator purpose and the authority of the caller"
        );
        // A second creation of the space fails and stages no second allocator.
        let again = store.agent().prepare_create(&env, made).await;
        assert_eq!(again.err(), Some(SpaceError::SpaceAlreadyExists));
        assert_eq!(allocator_values(&space).await, vec![0]);
    }
}

#[tokio::test]
async fn space_array_gensym_indices_sequential() {
    let fixture = fixture(Store::Fixed(8)).await;
    assert_eq!(fixture.allocate(1, 3).await, Ok(vec![0, 1, 2]));
    assert_eq!(
        allocator_labels(&fixture.space.take_event_log().await),
        vec!["consume A", "comm A", "produce A"],
        "one allocation takes the allocator and puts its next value"
    );
    assert_eq!(fixture.allocate(2, 1).await, Ok(vec![3]));
    assert_eq!(fixture.allocate(3, 0).await, Ok(vec![]));
    assert_eq!(
        allocator_labels(&fixture.space.take_event_log().await),
        vec![
            "consume A",
            "comm A",
            "produce A",
            "consume A",
            "comm A",
            "produce A"
        ],
        "an allocation of no names still takes and puts the allocator"
    );
    assert_eq!(fixture.allocate(4, 2).await, Ok(vec![4, 5]));
    assert_eq!(fixture.allocator().await, vec![6]);
    // The names are the index names of the space.
    for index in 0..6 {
        assert_eq!(
            namespace::reified_id_of(&name(index)),
            Some(namespace::ReifiedId::Name(SPACE, NameTail::Index(index)))
        );
    }
}

#[tokio::test]
async fn space_array_fixed_out_of_names() {
    let four = fixture(Store::Fixed(4)).await;
    assert_eq!(four.allocate(1, 3).await, Ok(vec![0, 1, 2]));
    four.space.take_event_log().await;
    // An allocation that does not fit allocates nothing and stages nothing.
    assert_eq!(four.allocate(2, 2).await, Err(SpaceError::OutOfNames));
    assert!(four.space.take_event_log().await.is_empty());
    assert_eq!(four.allocator().await, vec![3]);
    assert_eq!(four.allocate(3, 1).await, Ok(vec![3]));
    assert_eq!(four.allocate(4, 1).await, Err(SpaceError::OutOfNames));
    assert_eq!(four.allocate(5, 0).await, Ok(vec![]));
    assert_eq!(four.allocator().await, vec![4]);
    // A space of one name allocates it once.
    let single = fixture(Store::Fixed(1)).await;
    assert_eq!(single.allocate(1, 1).await, Ok(vec![0]));
    assert_eq!(single.allocate(2, 1).await, Err(SpaceError::OutOfNames));
}

#[tokio::test]
async fn space_array_cyclic_modulo_wraps() {
    let fixture = fixture(Store::Cyclic(3)).await;
    assert_eq!(fixture.allocate(1, 2).await, Ok(vec![0, 1]));
    assert_eq!(fixture.allocator().await, vec![2]);
    assert_eq!(fixture.allocate(2, 2).await, Ok(vec![2, 0]));
    assert_eq!(fixture.allocator().await, vec![1]);
    assert_eq!(fixture.allocate(3, 7).await, Ok(vec![1, 2, 0, 1, 2, 0, 1]));
    assert_eq!(fixture.allocator().await, vec![2]);
    // A cyclic space never runs out of names: two holders of one name share its collection.
    fixture
        .run(SpaceOp::Produce(send_on(SPACE, &name(1), 5, false)))
        .await
        .expect("the send on a reused name commits");
    assert_eq!(
        fired(
            &fixture
                .run(SpaceOp::Consume(receive_on(SPACE, &name(1), "w", any())))
                .await
                .expect("the receive commits")
        ),
        vec![("w".to_string(), vec![int(5)])]
    );
}

#[tokio::test]
async fn space_array_cyclic_allocator_identities_unique() {
    // A cyclic space of one name takes and puts the value 0 in every allocation. The random
    // state of each job still gives each take and each put an identity of its own (S6).
    let fixture = fixture(Store::Cyclic(1)).await;
    let mut seen = HashSet::new();
    let created = fixture
        .space
        .get_data(&allocator::allocator_channel(SPACE))
        .await;
    assert!(seen.insert(("produce", created[0].source.hash.clone())));
    for job in 1..=5 {
        assert_eq!(fixture.allocate(job, 1).await, Ok(vec![0]));
        assert_eq!(fixture.allocator().await, vec![0], "the value repeats");
        for identity in identities(&fixture.space.take_event_log().await) {
            assert!(
                seen.insert(identity.clone()),
                "a repeated identity: {identity:?}"
            );
        }
    }
    assert_eq!(seen.len(), 1 + 5 * 2);
}

// ── The key check (F8) and the operations on index names ──────────────────────────────────────

#[tokio::test]
async fn space_array_forged_channel_rejected() {
    let fixture = fixture(Store::Fixed(3)).await;
    assert_eq!(fixture.allocate(1, 2).await, Ok(vec![0, 1]));
    fixture.space.take_event_log().await;
    let ephemeral = NativeSpace {
        durability: Durability::Ephemeral,
        ..SPACE
    };
    let forged = [
        string("a forged channel"),
        namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([5; 32]))),
        allocator::index_name(OTHER, 0),
        allocator::index_name(ephemeral, 0),
        name(3),
        name(u64::MAX),
    ];
    for channel in &forged {
        for op in [
            SpaceOp::Produce(send_on(SPACE, channel, 1, false)),
            SpaceOp::Consume(receive_on(SPACE, channel, "w", any())),
        ] {
            assert_eq!(
                fixture.run(op).await,
                Err(SpaceError::NotAnIndexName),
                "{channel:?}"
            );
        }
    }
    assert!(
        fixture.space.take_event_log().await.is_empty(),
        "a rejected channel stages nothing"
    );
    assert_eq!(
        support::agent::counter(
            &fixture.space,
            &support::agent::space_counter_channel(SPACE)
        )
        .await,
        vec![0],
        "no collection was opened"
    );
    // The store checks the shape of a name, not its allocation: an index below the size that
    // the allocator has not handed out yet is accepted. A program cannot forge it, because a
    // program cannot make a GPrivate id.
    fixture
        .run(SpaceOp::Produce(send_on(SPACE, &name(2), 1, false)))
        .await
        .expect("an index below the size is a collection of the space");
}

#[tokio::test]
async fn space_array_queue_operations_on_index_names() {
    let fixture = fixture(Store::Fixed(2)).await;
    let indices = fixture.allocate(1, 2).await.expect("two names");
    let [a, b] = [name(indices[0]), name(indices[1])];
    for value in [1, 2] {
        fixture
            .run(SpaceOp::Produce(send_with_id(
                &a,
                value,
                usize::try_from(value).expect("an id"),
            )))
            .await
            .expect("the send commits");
    }
    // The queue of a is FIFO, and its records live on the data channel of the HashMap key.
    assert_eq!(
        records_on(&fixture.space, &data_channel(SPACE, &a)).await,
        vec![(0, vec![int(1)], false), (1, vec![int(2)], false)]
    );
    let outcome = fixture
        .run(SpaceOp::Consume(receive_on(SPACE, &a, "first", any())))
        .await
        .expect("the receive commits");
    assert_eq!(fired(&outcome), vec![("first".to_string(), vec![int(1)])]);
    // The head blocks: a receive for 9 parks behind the record 2.
    fixture
        .run(SpaceOp::Consume(receive_on(SPACE, &a, "nine", exactly(9))))
        .await
        .expect("the receive parks");
    assert_eq!(
        waiters_on(&fixture.space, &data_channel(SPACE, &a)).await,
        vec![(3, false, false)]
    );
    // b is a collection of its own.
    assert!(records_on(&fixture.space, &data_channel(SPACE, &b))
        .await
        .is_empty());
    fixture
        .run(SpaceOp::Produce(send_with_id(&b, 7, 7)))
        .await
        .expect("the send on b commits");
    assert_eq!(
        records_on(&fixture.space, &data_channel(SPACE, &b)).await,
        vec![(0, vec![int(7)], false)]
    );
    assert_eq!(
        support::agent::counter(&fixture.space, &support::agent::counter_channel(SPACE, &a)).await,
        vec![4],
        "every operation on a bumps its counter"
    );
}

/// What one discipline does with the sends of 1 and then 2 on one index name, followed by a
/// receive of any value.
struct DisciplineCase {
    /// The name of the discipline.
    inner: &'static str,
    /// The value that the receive fires, or `None` when the DR-75 order of the bag chooses.
    fires: Option<i64>,
    /// The error of the second send, or `None` when the collection stores it.
    second_send: Option<SpaceError>,
}

/// Every discipline that the family dispatch wires runs on the index names of a fixed Array, a
/// cyclic Array and a Vector space. The family agent creates each space, allocates two of its
/// names and runs the discipline on the first one, so the test covers the dispatch arms of the
/// Array and Vector families, not only their agents. The queue fires the oldest record, the stack
/// the youngest, the bag and the set one record in their DR-75 order, the priority queue the
/// oldest record of its unprioritized level, and the cell refuses a second record. A name of
/// another space fails with `NotAnIndexName` before any staging (F8).
#[tokio::test]
async fn space_array_all_disciplines_supported() {
    let cases = [
        DisciplineCase {
            inner: BagDiscipline::NAME,
            fires: None,
            second_send: None,
        },
        DisciplineCase {
            inner: QueueDiscipline::NAME,
            fires: Some(1),
            second_send: None,
        },
        DisciplineCase {
            inner: StackDiscipline::NAME,
            fires: Some(2),
            second_send: None,
        },
        DisciplineCase {
            inner: SetDiscipline::NAME,
            fires: None,
            second_send: None,
        },
        DisciplineCase {
            inner: CellDiscipline::NAME,
            fires: Some(1),
            second_send: Some(SpaceError::CellOccupied),
        },
        DisciplineCase {
            inner: PriorityQueueDiscipline::NAME,
            fires: Some(1),
            second_send: None,
        },
    ];
    for store in [Store::Fixed(4), Store::Cyclic(4), Store::Vector] {
        for case in &cases {
            let label = format!("{store:?} {}", case.inner);
            let space = play().await;
            let env = env_over(space.clone(), &budget());
            let mut config = store.space_config(SPACE);
            config.inner = case.inner.to_string();
            let family = SpaceFactoryUrn {
                outer: store.family().outer,
                inner: SpaceInner::parse(case.inner).expect("a wired inner collection"),
            };
            create(&FamilyAgent, &env, creation(SPACE, config.to_par(), family)).await;
            let indices = allocate_with(&FamilyAgent, &env, alloc_op(1, 2))
                .await
                .expect("the family agent allocates two names");
            assert_eq!(indices, vec![0, 1], "{label}");
            let channel = name(indices[0]);
            run(
                &FamilyAgent,
                &env,
                SpaceOp::Produce(send_on(SPACE, &channel, 1, false)),
            )
            .await
            .expect("the first send commits");
            let second = run(
                &FamilyAgent,
                &env,
                SpaceOp::Produce(send_on(SPACE, &channel, 2, false)),
            )
            .await;
            assert_eq!(second.err(), case.second_send, "{label}");
            let outcome = run(
                &FamilyAgent,
                &env,
                SpaceOp::Consume(receive_on(SPACE, &channel, "any", any())),
            )
            .await
            .expect("the receive commits");
            let [(body, values)] = fired(&outcome).try_into().expect("one fire");
            assert_eq!(body, "any", "{label}");
            let [value] = values.as_slice() else {
                panic!("{label}: the receive binds {} values", values.len());
            };
            let fired_value = value_of(std::slice::from_ref(value));
            let stored: Vec<i64> = records_on(&space, &data_channel(SPACE, &channel))
                .await
                .into_iter()
                .map(|(_, pars, _)| value_of(&pars))
                .collect();
            match (case.fires, &case.second_send) {
                (Some(expected), Some(_)) => {
                    assert_eq!(fired_value, expected, "{label}");
                    assert!(stored.is_empty(), "{label}: the cell is empty");
                }
                (Some(expected), None) => {
                    assert_eq!(fired_value, expected, "{label}");
                    assert_eq!(
                        stored,
                        vec![3 - expected],
                        "{label}: the other record stays"
                    );
                }
                (None, _) => {
                    assert!([1, 2].contains(&fired_value), "{label}");
                    assert_eq!(
                        stored,
                        vec![3 - fired_value],
                        "{label}: the other record stays"
                    );
                }
            }
            for op in [
                SpaceOp::Produce(send_on(SPACE, &allocator::index_name(OTHER, 0), 1, false)),
                SpaceOp::Consume(receive_on(
                    SPACE,
                    &allocator::index_name(OTHER, 0),
                    "w",
                    any(),
                )),
            ] {
                assert_eq!(
                    run(&FamilyAgent, &env, op).await.err(),
                    Some(SpaceError::NotAnIndexName),
                    "{label}"
                );
            }
        }
    }
}

// ── The allocation jobs of Rholang deploys (F11) ─────────────────────────────────────────────

/// The elements of a list `Par`.
fn list_elements(list: &Par) -> Vec<Par> {
    match list.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(elements)),
        }] => elements.ps.clone(),
        other => panic!("a list: {other:?}"),
    }
}

/// A fixed Array space hands out its index names in allocation order until its size. The setup
/// deploy takes the names 0 and 1 with the handle method `gensym`, and a `new` of two binders in
/// a `use` block of the space takes the names 2 and 3 in one allocation job. A send on such a
/// name reaches the collection of its index. A forged channel in the `use` block routes to the
/// space (R5), so it fails with `NotAnIndexName` (F8). A `new` that needs more names than the
/// space has left fails with `OutOfNames`. Neither failure leaves space data. A Vector space
/// keeps allocating.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_array_new_in_use_allocates_indices() {
    let mut session = support::session::Session::new().await;
    let space = session
        .setup("rho:space:array:queue", "\"default\"; size=5")
        .await;
    assert_eq!(space.a, allocator::index_name(space.space, 0));
    assert_eq!(space.b, allocator::index_name(space.space, 1));
    session
        .run(
            &support::session::with_space(
                r#"new out in { use space { new x, y in { out!([*x, *y]) | x!(7) } } | for (@names <- out) { @"rrs-indices"!(names) } }"#,
            ),
            "rrs-p5-array-use-new",
        )
        .await;
    let indices = list_elements(&session.sole("rrs-indices").await);
    assert_eq!(indices, vec![
        allocator::index_name(space.space, 2),
        allocator::index_name(space.space, 3)
    ]);
    assert_eq!(session.records(space.space, &indices[0]).await, vec![(
        0,
        vec![int(7)],
        false
    )]);
    session
        .fail_with(
            &support::session::with_space(r#"use space { @"rrs-forged"!(1) }"#),
            "rrs-p5-array-use-forged",
            &support::session::Injections::new(),
            &["NotAnIndexName"],
        )
        .await;
    session
        .fail_with(
            &support::session::with_space(r#"use space { new u, v in { Nil } }"#),
            "rrs-p5-array-use-new-full",
            &support::session::Injections::new(),
            &["OutOfNames"],
        )
        .await;

    let mut session = support::session::Session::new().await;
    let space = session.setup("rho:space:vector:queue", "\"default\"").await;
    session
        .run(
            &support::session::with_space(
                r#"new out in { use space { new x, y, z in { out!([*x, *y, *z]) } } | for (@names <- out) { @"rrs-indices"!(names) } }"#,
            ),
            "rrs-p5-vector-use-new",
        )
        .await;
    assert_eq!(
        list_elements(&session.sole("rrs-indices").await),
        (2..5)
            .map(|index| allocator::index_name(space.space, index))
            .collect::<Vec<_>>()
    );
}

/// A `new` with space annotations allocates the binders that name an Array or a Vector space from
/// that space, in binder order, in one allocation job for the space. A binder without an
/// annotation keeps its plain unforgeable name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_array_newspace_annotation_allocates() {
    let mut session = support::session::Session::new().await;
    let space = session
        .setup("rho:space:array:queue", "\"default\"; size=8")
        .await;
    session
        .run(
            &support::session::with_space(
                r#"new out in { new x : space, y, z : space in { out!([*x, *y, *z]) } | for (@names <- out) { @"rrs-annotated"!(names) } }"#,
            ),
            "rrs-p5-array-newspace",
        )
        .await;
    let names = list_elements(&session.sole("rrs-annotated").await);
    assert_eq!(names.len(), 3);
    assert_eq!(names[0], allocator::index_name(space.space, 2));
    assert_eq!(classify_channel(&names[1]), ChannelClass::Unforgeable);
    assert_eq!(names[2], allocator::index_name(space.space, 3));
}

#[tokio::test]
async fn space_array_corrupt_shape_is_inconsistent() {
    let no_size = SpaceError::Inconsistent("the config of an Array space has no size");
    let bad_size = SpaceError::Inconsistent("the size of an Array space is not a positive integer");
    let bad_flag = SpaceError::Inconsistent("the cyclic flag of an Array space is not a Boolean");
    let cases = [
        (Vec::new(), no_size),
        (vec![(SIZE_PARAM, int(0))], bad_size.clone()),
        (vec![(SIZE_PARAM, int(-2))], bad_size.clone()),
        (vec![(SIZE_PARAM, string("2"))], bad_size),
        (vec![(SIZE_PARAM, int(2)), (CYCLIC_PARAM, int(1))], bad_flag),
    ];
    for (params, expected) in cases {
        // The factory validates the params. A stored config without a valid shape cannot come
        // from it, so the agent fails closed on every use.
        let mut config = SpaceConfig::new(
            SPACE.sid,
            ArrayOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        );
        for (key, value) in &params {
            config.params.insert(key.to_string(), value.clone());
        }
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(
            &ARRAY_QUEUE,
            &env,
            creation(SPACE, config.to_par(), Store::Fixed(1).family()),
        )
        .await;
        space.take_event_log().await;
        assert_eq!(
            allocate_with(&ARRAY_QUEUE, &env, alloc_op(1, 1)).await,
            Err(expected.clone()),
            "{params:?}"
        );
        assert_eq!(
            run(
                &ARRAY_QUEUE,
                &env,
                SpaceOp::Produce(send_on(SPACE, &name(0), 1, false))
            )
            .await,
            Err(expected),
            "{params:?}"
        );
        assert!(space.take_event_log().await.is_empty(), "{params:?}");
        assert_eq!(allocator_values(&space).await, vec![0], "{params:?}");
    }
}

#[tokio::test]
async fn space_outer_stores_share_the_hashmap_key() {
    // A5: the three stores key a collection by the canonical bytes of its channel. So the family
    // agent opens every collection with the channels of the HashMap store.
    for channel in [name(0), name(41), string("forged")] {
        let key = HashMapOuter.key(&channel).expect("a key");
        assert_eq!(ArrayOuter.key(&channel), Ok(key.clone()));
        assert_eq!(VectorOuter.key(&channel), Ok(key.clone()));
        let hashmap = CollectionChannels::new(&HashMapOuter, SPACE, &key);
        assert_eq!(CollectionChannels::new(&ArrayOuter, SPACE, &key), hashmap);
        assert_eq!(CollectionChannels::new(&VectorOuter, SPACE, &key), hashmap);
        assert_eq!(*hashmap.data(), data_channel(SPACE, &channel));
    }
    // An index name is a name of its space, so it routes to the space by its own bytes (R3).
    assert_eq!(classify_channel(&name(3)), ChannelClass::SpaceName(SPACE));
    assert_eq!(
        classify_channel(&allocator::allocator_channel(SPACE)),
        ChannelClass::Storage,
        "the allocator channel is storage, so a program never reaches it"
    );
}

#[tokio::test]
async fn space_array_restart() {
    // The allocator is a record of the history trie, not a value in memory, so a node that
    // restarts at the last root continues where it stopped.
    let mut manager = InMemoryStoreManager::new();
    let first = ParRSpace::create(
        manager.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    let env = env_over(first.clone(), &budget());
    create_store(Store::Fixed(5), &first, &env).await;
    assert_eq!(
        allocate_with(&ARRAY_QUEUE, &env, alloc_op(1, 2)).await,
        Ok(vec![0, 1])
    );
    run(
        &ARRAY_QUEUE,
        &env,
        SpaceOp::Produce(send_on(SPACE, &name(1), 9, false)),
    )
    .await
    .expect("the send commits");
    let root = first.create_checkpoint().await.expect("a checkpoint").root;
    drop(env);
    drop(first);
    // A new space over the same stores, reset to the root, stands for the restarted node.
    let second = ParRSpace::create(
        manager
            .r_space_stores()
            .await
            .expect("the same in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    second.reset(&root).await.expect("reset to the last root");
    let env = env_over(second.clone(), &budget());
    assert_eq!(allocator_values(&second).await, vec![2]);
    assert_eq!(
        allocate_with(&ARRAY_QUEUE, &env, alloc_op(2, 2)).await,
        Ok(vec![2, 3])
    );
    assert_eq!(
        allocate_with(&ARRAY_QUEUE, &env, alloc_op(3, 2)).await,
        Err(SpaceError::OutOfNames)
    );
    assert_eq!(
        records_on(&second, &data_channel(SPACE, &name(1))).await,
        vec![(0, vec![int(9)], false)]
    );
}

// ── The Vector store ──────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_vector_gensym_grows() {
    let fixture = fixture(Store::Vector).await;
    let mut allocated = Vec::new();
    for (job, count) in [(1, 1), (2, 3), (3, 0), (4, 2), (5, 1)] {
        allocated.push(
            fixture
                .allocate(job, count)
                .await
                .expect("a vector never runs out"),
        );
    }
    assert_eq!(allocated, vec![
        vec![0],
        vec![1, 2, 3],
        vec![],
        vec![4, 5],
        vec![6]
    ]);
    assert_eq!(fixture.allocator().await, vec![7]);
}

/// A Vector store whose creation stages the allocator near `i64::MAX`, so that a test reaches
/// the bound of the store with a few allocations. It is the Vector store in every other way.
#[derive(Clone, Copy, Debug, Default)]
struct NearFullVector;

/// The allocator value of a new [`NearFullVector`] space.
const NEAR_FULL: i64 = i64::MAX - 2;

impl OuterStore for NearFullVector {
    const NAME: &'static str = "vector";
    const ALLOCATOR: bool = true;

    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> { VectorOuter.key(channel) }

    fn check_key(
        &self,
        config: &SpaceConfig,
        channel: &Par,
        key: &CollectionKey,
    ) -> Result<(), SpaceError> {
        VectorOuter.check_key(config, channel, key)
    }

    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        VectorOuter.data_channel(space, key)
    }

    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        VectorOuter.counter_channel(space, key)
    }

    fn creation_records(
        &self,
        space: NativeSpace,
        random: &Blake2b512Random,
        _authority: &CostAuthority,
    ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
        Ok(vec![(
            allocator::allocator_channel(space),
            record::counter_record(
                NEAR_FULL,
                &RandomPurpose::Allocator.derive(random).split_byte(1),
                None,
            ),
        )])
    }

    fn allocate(
        &self,
        cx: &mut AllocCx<'_>,
        config: &SpaceConfig,
        op: &AllocOp,
    ) -> Result<Vec<u64>, SpaceError> {
        VectorOuter.allocate(cx, config, op)
    }

    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        VectorOuter.gensym(space, random)
    }
}

#[tokio::test]
async fn space_vector_unbounded_indices() {
    let agent = GenericSpace::<NearFullVector, QueueDiscipline>::default();
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create(
        &agent,
        &env,
        creation(SPACE, Store::Vector.config(), Store::Vector.family()),
    )
    .await;
    let near = u64::try_from(NEAR_FULL).expect("a natural value");
    assert_eq!(
        allocate_with(&agent, &env, alloc_op(1, 1)).await,
        Ok(vec![near])
    );
    assert_eq!(
        allocate_with(&agent, &env, alloc_op(2, 1)).await,
        Ok(vec![near + 1])
    );
    space.take_event_log().await;
    // The allocator holds i64::MAX: one more name would pass the bound of the record.
    assert_eq!(allocator_values(&space).await, vec![i64::MAX]);
    assert_eq!(
        allocate_with(&agent, &env, alloc_op(3, 1)).await,
        Err(SpaceError::OutOfNames)
    );
    assert!(space.take_event_log().await.is_empty());
    // A Vector space accepts every index name of its own space, the greatest included.
    for index in [near + 1, u64::MAX] {
        run(
            &VECTOR_QUEUE,
            &env,
            SpaceOp::Produce(send_on(SPACE, &name(index), 3, false)),
        )
        .await
        .expect("an index name of the space");
        assert_eq!(
            fired(
                &run(
                    &VECTOR_QUEUE,
                    &env,
                    SpaceOp::Consume(receive_on(SPACE, &name(index), "w", any()))
                )
                .await
                .expect("the receive commits")
            ),
            vec![("w".to_string(), vec![int(3)])]
        );
    }
}

#[tokio::test]
async fn space_vector_forged_channel_rejected() {
    let fixture = fixture(Store::Vector).await;
    for channel in [
        string("a forged channel"),
        int(0),
        namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([2; 32]))),
        allocator::index_name(OTHER, 0),
        allocator::allocator_channel(SPACE),
    ] {
        for op in [
            SpaceOp::Produce(send_on(SPACE, &channel, 1, false)),
            SpaceOp::Consume(receive_on(SPACE, &channel, "w", any())),
        ] {
            assert_eq!(
                fixture.run(op).await,
                Err(SpaceError::NotAnIndexName),
                "{channel:?}"
            );
        }
    }
    assert!(fixture.space.take_event_log().await.is_empty());
}

// Retired by decision 90 (C6, step D): `space_vector_replay_equal` and its step helpers ran through
// the rigged replay modes, which retire (decision 125). Native replay: the family matrix
// `space_native_replay_equal_{hashmap,pathmap,array,vector}`. Its negative controls:
// `space_native_forged_predecessor_fails_closed`,
// `native_space_replay_rejects_a_tampered_candidate`,
// `native_space_replay_rejects_an_outcome_that_does_not_fit`. The text before:
// ```
// /// The script of the replay test: allocations and operations on the allocated names.
// #[derive(Clone, Debug, PartialEq)]
// enum Step {
//     Allocate(u64),
//     Send(u64, i64),
//     Receive(u64),
// }
//
// /// The result of one step: the indices of an allocation, or the fires of an operation. The
// /// outcome of an operation is large, so it is boxed.
// #[derive(Clone, Debug, PartialEq)]
// enum Ran {
//     Allocated(Result<Vec<u64>, SpaceError>),
//     Operated(Box<Result<SpaceOutcome, SpaceError>>),
// }
//
// async fn run_steps(store: Store, env: &SpaceEnv, steps: &[Step]) -> Vec<Ran> {
//     let mut ran = Vec::with_capacity(steps.len());
//     for (job, step) in (1u64..).zip(steps) {
//         ran.push(match step {
//             Step::Allocate(count) => {
//                 Ran::Allocated(allocate_with(store.agent(), env, alloc_op(job, *count)).await)
//             }
//             Step::Send(index, value) => Ran::Operated(Box::new(
//                 run(
//                     store.agent(),
//                     env,
//                     SpaceOp::Produce(send_with_id(
//                         &name(*index),
//                         *value,
//                         usize::try_from(job).expect("an id"),
//                     )),
//                 )
//                 .await,
//             )),
//             Step::Receive(index) => Ran::Operated(Box::new(
//                 run(
//                     store.agent(),
//                     env,
//                     SpaceOp::Consume(receive_on(SPACE, &name(*index), "w", any())),
//                 )
//                 .await,
//             )),
//         });
//     }
//     ran
// }
//
// #[tokio::test]
// async fn space_vector_replay_equal() {
//     let steps = [
//         Step::Allocate(1),
//         Step::Allocate(2),
//         Step::Send(0, 5),
//         Step::Receive(1),
//         Step::Send(1, 6),
//         Step::Allocate(1),
//         Step::Receive(0),
//     ];
//     for store in [Store::Vector, Store::Cyclic(2), Store::Fixed(4)] {
//         let (play_space, replay_space): (ParRSpace, ParReplay) = play_and_replay().await;
//         let play_env = env_over(play_space.clone(), &budget());
//         create(
//             store.agent(),
//             &play_env,
//             creation(SPACE, store.config(), store.family()),
//         )
//         .await;
//         let start = play_space
//             .create_checkpoint()
//             .await
//             .expect("a checkpoint after the creation")
//             .root;
//         let played = run_steps(store, &play_env, &steps).await;
//         assert!(
//             played.iter().all(|ran| match ran {
//                 Ran::Allocated(result) => result.is_ok(),
//                 Ran::Operated(result) => result.is_ok(),
//             }),
//             "{store:?}: {played:?}"
//         );
//         let checkpoint = play_space.create_checkpoint().await.expect("a checkpoint");
//         replay_space
//             .rig_and_reset(start.clone(), checkpoint.log)
//             .await
//             .expect("the replay rigs the play log");
//         let replay_env = env_over(replay_space.clone(), &budget());
//         let replayed = run_steps(store, &replay_env, &steps).await;
//         assert_eq!(replayed, played, "{store:?}");
//         replay_space
//             .check_replay_data()
//             .await
//             .expect("every recorded COMM is paired");
//         assert_eq!(
//             replay_space
//                 .create_checkpoint()
//                 .await
//                 .expect("a checkpoint")
//                 .root,
//             checkpoint.root,
//             "{store:?}"
//         );
//         // Negative control: a replay without the recorded COMMs diverges at the allocator.
//         replay_space
//             .rig_and_reset(start, Vec::new())
//             .await
//             .expect("the replay rigs an empty log");
//         match allocate_with(store.agent(), &replay_env, alloc_op(1, 1)).await {
//             Err(SpaceError::ReplayDivergence(_)) => {}
//             other => panic!("{store:?}: a log without the COMMs diverges: {other:?}"),
//         }
//     }
// }
// ```

// ── The allocation job ────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_allocation_job_dangling_aborts() {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    for agent in [&ARRAY_QUEUE as &dyn SpaceAgent, &VECTOR_QUEUE, &FamilyAgent] {
        assert!(matches!(
            agent.allocate(&env, alloc_op(1, 1)).await,
            Err(SpaceError::DanglingHandle { .. })
        ));
    }
    assert!(space.take_event_log().await.is_empty());
    // A space of another family fails the family check of the Array agent and stages nothing.
    let hashmap = SpaceConfig::new(
        SPACE.sid,
        HashMapOuter::NAME,
        QueueDiscipline::NAME,
        SpaceQualifier::Default,
    );
    create(
        &HASHMAP_QUEUE,
        &env,
        creation(SPACE, hashmap.to_par(), SpaceFactoryUrn::HASHMAP_QUEUE),
    )
    .await;
    space.take_event_log().await;
    assert!(matches!(
        ARRAY_QUEUE.allocate(&env, alloc_op(2, 1)).await,
        Err(SpaceError::FamilyUnavailable { outer, inner }) if outer == "hashmap" && inner == "queue"
    ));
    assert!(matches!(
        HASHMAP_QUEUE.allocate(&env, alloc_op(3, 1)).await,
        Ok(Allocation::Baseline)
    ));
    assert!(space.take_event_log().await.is_empty());
}

// Disabled by decision 90 (DR-132): the deterministic scheduler and its `root` are gone, and space
// jobs run inline in the counted task of their caller. `space_allocation_job_runs_inline` below
// runs the same jobs and checks the same results.
// #[tokio::test]
// async fn space_allocation_job_runs_under_the_scheduler() {
//     let host = play().await;
//     let session: RhoISpace = Arc::new(Box::new(host.clone()));
//     let budget = budget();
//     let reified = runtime::for_host(host.clone(), budget.clone());
//     let array: Arc<dyn SpaceAgent> =
//         Arc::new(GenericSpace::<ArrayOuter, QueueDiscipline>::default());
//     let hashmap: Arc<dyn SpaceAgent> =
//         Arc::new(GenericSpace::<HashMapOuter, QueueDiscipline>::default());
//     let hashmap_space = NativeSpace {
//         sid: [0x5F; SID_LEN],
//         durability: Durability::Durable,
//     };
//     let hashmap_config = SpaceConfig::new(
//         hashmap_space.sid,
//         HashMapOuter::NAME,
//         QueueDiscipline::NAME,
//         SpaceQualifier::Default,
//     );
//     let results = root(session, budget, ReductionCoordinator::default(), async {
//         reified
//             .create_space(
//                 Arc::clone(&array),
//                 creation(SPACE, Store::Fixed(3).config(), Store::Fixed(3).family()),
//             )
//             .await?;
//         reified
//             .create_space(
//                 Arc::clone(&hashmap),
//                 creation(
//                     hashmap_space,
//                     hashmap_config.to_par(),
//                     SpaceFactoryUrn::HASHMAP_QUEUE,
//                 ),
//             )
//             .await?;
//         let first = reified.allocate(Arc::clone(&array), alloc_op(1, 2)).await?;
//         let second = reified.allocate(Arc::clone(&array), alloc_op(2, 1)).await?;
//         let baseline = reified
//             .allocate(Arc::clone(&hashmap), AllocOp {
//                 space: hashmap_space,
//                 ..alloc_op(3, 2)
//             })
//             .await?;
//         let exhausted = reified.allocate(Arc::clone(&array), alloc_op(4, 1)).await;
//         Ok::<_, InterpreterError>((first, second, baseline, exhausted))
//     })
//     .await
//     .expect("the space jobs run");
//     let (first, second, baseline, exhausted) = results;
//     assert_eq!(first, Some(vec![0, 1]));
//     assert_eq!(second, Some(vec![2]));
//     assert_eq!(baseline, None, "a HashMap space keeps the baseline draws");
//     match exhausted {
//         Err(InterpreterError::ReduceError(message)) => {
//             assert!(message.starts_with("reified:OutOfNames: "), "{message}")
//         }
//         other => panic!("a full Array space aborts the job: {other:?}"),
//     }
//     assert_eq!(allocator_values(&host).await, vec![3]);
// }

/// Added by decision 90 (DR-132): the allocation jobs of an Array space run inline, as every space
/// job does: they allocate consecutive index names, a HashMap space keeps the baseline draws, and
/// a full Array space aborts the job with `OutOfNames`.
#[tokio::test]
async fn space_allocation_job_runs_inline() {
    let host = play().await;
    // Changed by decision 90 (C6, decision 69): a host runtime runs space jobs only when its
    // budget runs a native deploy. The legacy budget follows.
    // let budget = budget();
    let budget = native_budget();
    let reified = runtime::for_host(host.clone(), budget.clone());
    let array: Arc<dyn SpaceAgent> =
        Arc::new(GenericSpace::<ArrayOuter, QueueDiscipline>::default());
    let hashmap: Arc<dyn SpaceAgent> =
        Arc::new(GenericSpace::<HashMapOuter, QueueDiscipline>::default());
    let hashmap_space = NativeSpace {
        sid: [0x5F; SID_LEN],
        durability: Durability::Durable,
    };
    let hashmap_config = SpaceConfig::new(
        hashmap_space.sid,
        HashMapOuter::NAME,
        QueueDiscipline::NAME,
        SpaceQualifier::Default,
    );
    let results = async {
        reified
            .create_space(
                Arc::clone(&array),
                creation(SPACE, Store::Fixed(3).config(), Store::Fixed(3).family()),
            )
            .await?;
        reified
            .create_space(
                Arc::clone(&hashmap),
                creation(
                    hashmap_space,
                    hashmap_config.to_par(),
                    SpaceFactoryUrn::HASHMAP_QUEUE,
                ),
            )
            .await?;
        let first = reified.allocate(Arc::clone(&array), alloc_op(1, 2)).await?;
        let second = reified.allocate(Arc::clone(&array), alloc_op(2, 1)).await?;
        let baseline = reified
            .allocate(Arc::clone(&hashmap), AllocOp {
                space: hashmap_space,
                ..alloc_op(3, 2)
            })
            .await?;
        let exhausted = reified.allocate(Arc::clone(&array), alloc_op(4, 1)).await;
        Ok::<_, InterpreterError>((first, second, baseline, exhausted))
    }
    .await
    .expect("the space jobs run");
    let (first, second, baseline, exhausted) = results;
    assert_eq!(first, Some(vec![0, 1]));
    assert_eq!(second, Some(vec![2]));
    assert_eq!(baseline, None, "a HashMap space keeps the baseline draws");
    match exhausted {
        // Changed by Reified RSpaces (user decision of 2026-10-25, as DR-128): `OutOfNames` is a
        // user failure, so it is a `SpaceUserError` with the text of its reduce error.
        // Err(InterpreterError::ReduceError(message)) => {
        Err(InterpreterError::SpaceUserError(message)) => {
            assert!(message.starts_with("reified:OutOfNames: "), "{message}")
        }
        other => panic!("a full Array space aborts the job: {other:?}"),
    }
    assert_eq!(allocator_values(&host).await, vec![3]);
}

/// The phlo schedule of a native deploy, as `reified_native_fail_closed.rs` makes it.
fn native_schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([0, 1, 1, 0])
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: 3,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// The native runtime config of a deploy with the phlo limit 1,000,000.
fn native_config() -> NativeRuntimeConfig {
    let descriptor = native_schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [3];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 1_000_000,
            price_ceiling: 3,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        1_000_000,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
            .expect("the test contract builds");
    NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4096,
        regions: NativePhloRegionLimits {
            regions: 1000,
            encoded_authority_bytes: 10_000_000,
        },
    })
}

/// Added by decision 90 (C6, decision 69): a budget in native execution, with a funded deploy
/// signature and the phlo limit of [`native_config`]. A host runtime runs space jobs only when
/// its budget runs a native deploy.
fn native_budget() -> RuntimeBudget {
    let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs-p5d native allocation"));
    native.set_deploy_signature_funded(
        b"rrs-p5d-native",
        funding_sig_compound(&[&1u64.to_be_bytes()]),
    );
    native
        .reset_for_native_execution(native_config())
        .expect("the budget enters native execution");
    assert!(native.native_execution_active());
    native
}

// Disabled by decision 47: native execution runs the allocation job, so it does not fail with
// `SpacesUnsupportedInNativeExecution`. Its successor is
// `allocation_native_play_allocates_as_legacy`.
// Changed by decision 90 (C6): the successor is now
// `allocation_native_play_allocates_and_legacy_fails_closed`.
//
// ```
// #[tokio::test]
// async fn allocation_native_fails_closed() {
//     // Decision 46: native play and native replay fail before any space work, so an allocation
//     // job reads no config and stages nothing.
//     let space = play().await;
//     let env = env_over(space.clone(), &budget());
//     create_store(Store::Fixed(4), &space, &env).await;
//     let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs-p5d native allocation"));
//     native.set_deploy_signature_funded(
//         b"rrs-p5d-native",
//         funding_sig_compound(&[&1u64.to_be_bytes()]),
//     );
//     native
//         .reset_for_native_execution(native_config())
//         .expect("the budget enters native execution");
//     assert!(native.native_execution_active());
//     let array: Arc<dyn SpaceAgent> =
//         Arc::new(GenericSpace::<ArrayOuter, QueueDiscipline>::default());
//     for reified in [
//         runtime::for_host(space.clone(), native),
//         runtime::native_replay(),
//     ] {
//         match reified.allocate(Arc::clone(&array), alloc_op(1, 1)).await {
//             Err(InterpreterError::ReduceError(message)) => assert!(
//                 message.starts_with("reified:SpacesUnsupportedInNativeExecution: "),
//                 "{message}"
//             ),
//             other => panic!("an allocation ran in native execution: {other:?}"),
//         }
//     }
//     assert!(space.take_event_log().await.is_empty());
//     assert_eq!(allocator_values(&space).await, vec![0]);
// }
// ```

// Changed by decision 90 (C6, decision 69): spaces have one execution model, so a host runtime
// outside native execution fails closed, and the comparison of native play with legacy execution
// has no legacy side. The test that replaces it follows the legacy test, as
// `space_joins_native_play_commits_and_legacy_fails_closed` replaces its legacy test.
//
// #[tokio::test]
// async fn allocation_native_play_allocates_as_legacy() {
//     // Decision 47: native play runs the allocation job of an Array space as legacy execution
//     // runs it: the same indices, the same events and the same allocator value. A native replay
//     // runtime that is not bound yet fails the job closed with `RuntimeUnavailable`, so it reads
//     // no config and stages nothing.
//     let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs-p5d native allocation"));
//     native.set_deploy_signature_funded(
//         b"rrs-p5d-native",
//         funding_sig_compound(&[&1u64.to_be_bytes()]),
//     );
//     native
//         .reset_for_native_execution(native_config())
//         .expect("the budget enters native execution");
//     assert!(native.native_execution_active());
//     let array: Arc<dyn SpaceAgent> =
//         Arc::new(GenericSpace::<ArrayOuter, QueueDiscipline>::default());
//     let mut runs = Vec::with_capacity(2);
//     for deploy in [budget(), native] {
//         let space = play().await;
//         let env = env_over(space.clone(), &budget());
//         create_store(Store::Fixed(4), &space, &env).await;
//         space.take_event_log().await;
//         let indices = runtime::for_host(space.clone(), deploy)
//             .allocate(Arc::clone(&array), alloc_op(1, 1))
//             .await
//             .expect("the allocation commits");
//         runs.push((
//             indices,
//             space.take_event_log().await,
//             allocator_values(&space).await,
//         ));
//     }
//     assert_eq!(runs[0].0, Some(vec![0]));
//     assert_eq!(runs[1], runs[0]);
//
//     let space = play().await;
//     let env = env_over(space.clone(), &budget());
//     create_store(Store::Fixed(4), &space, &env).await;
//     space.take_event_log().await;
//     match runtime::native_replay()
//         .allocate(Arc::clone(&array), alloc_op(1, 1))
//         .await
//     {
//         Err(InterpreterError::ReduceError(message)) => assert!(
//             message.starts_with("reified:RuntimeUnavailable: "),
//             "{message}"
//         ),
//         other => panic!("an unbound native replay runtime allocated: {other:?}"),
//     }
//     assert!(space.take_event_log().await.is_empty());
//     assert_eq!(allocator_values(&space).await, vec![0]);
// }

/// Decision 47: native play runs the allocation job of an Array space as an allocation job runs
/// it (`space_array_gensym_indices_sequential`). It takes the allocator, puts its next value and
/// allocates the first index. Decision 90 (C6): a host runtime whose deploy is not in native
/// execution and a native replay runtime that is not bound yet fail the job closed with
/// `RuntimeUnavailable`. They read no config, stage nothing and keep the allocator at 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allocation_native_play_allocates_and_legacy_fails_closed() {
    let array: Arc<dyn SpaceAgent> =
        Arc::new(GenericSpace::<ArrayOuter, QueueDiscipline>::default());
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create_store(Store::Fixed(4), &space, &env).await;
    space.take_event_log().await;
    let indices = runtime::for_host(space.clone(), native_budget())
        .allocate(Arc::clone(&array), alloc_op(1, 1))
        .await
        .expect("the allocation commits");
    assert_eq!(indices, Some(vec![0]));
    assert_eq!(
        allocator_labels(&space.take_event_log().await),
        vec!["consume A", "comm A", "produce A"],
        "native play takes the allocator and puts its next value"
    );
    assert_eq!(allocator_values(&space).await, vec![1]);

    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create_store(Store::Fixed(4), &space, &env).await;
    space.take_event_log().await;
    for reified in [
        runtime::for_host(space.clone(), budget()),
        runtime::native_replay(),
    ] {
        match reified.allocate(Arc::clone(&array), alloc_op(1, 1)).await {
            Err(InterpreterError::ReduceError(message)) => assert!(
                message.starts_with("reified:RuntimeUnavailable: "),
                "{message}"
            ),
            other => panic!("a runtime outside native execution allocated: {other:?}"),
        }
    }
    assert!(space.take_event_log().await.is_empty());
    assert_eq!(allocator_values(&space).await, vec![0]);
}

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
//
// #[tokio::test]
// async fn allocation_legacy_charges_equal_host() {
//     // Decision 45, Option P: in a legacy deploy, the take and the put of the allocator pay what
//     // ChargingRSpace charges for the equivalent host calls, in the order of the host program.
//     let authority = region(9);
//     for store in [Store::Fixed(4), Store::Cyclic(2), Store::Vector] {
//         let space = play().await;
//         let agent_budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs-p5d agent"));
//         let _agent_legacy = agent_budget.enter_legacy_scope();
//         let env = env_over(space.clone(), &agent_budget);
//         create_store(store, &space, &env).await;
//         let stored = space
//             .get_data(&allocator::allocator_channel(SPACE))
//             .await
//             .remove(0)
//             .a;
//         let before = agent_budget.get_log().len();
//         let op = AllocOp {
//             authority: authority.clone(),
//             ..alloc_op(1, 3)
//         };
//         let indices = allocate_with(store.agent(), &env, op)
//             .await
//             .expect("an allocation");
//         let charged = agent_budget.get_log()[before..].to_vec();
//         let (_, next) = model(store, 0, 3).expect("the allocation fits");
//         assert_eq!(indices.len(), 3);
//
//         let host_budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs-p5d host"));
//         let _host_legacy = host_budget.enter_legacy_scope();
//         let host = charging_host(&host_budget).await;
//         let channel = allocator::allocator_channel(SPACE);
//         host_produce(&host, &host_budget, channel.clone(), stored, false).await;
//         let random = job_random(1);
//         let take = host_consume(
//             &host,
//             &host_budget,
//             vec![channel.clone()],
//             vec![counter_pattern()],
//             counter_continuation(
//                 0,
//                 &RandomPurpose::Allocator.derive(&random).split_byte(0),
//                 Some(authority.clone()),
//             ),
//             false,
//             false,
//         )
//         .await;
//         assert!(take.fired.is_some(), "{store:?}: the host take fires on A");
//         let put = host_produce(
//             &host,
//             &host_budget,
//             channel,
//             record::counter_record(
//                 next,
//                 &RandomPurpose::Allocator.derive(&random).split_byte(1),
//                 Some(authority.clone()),
//             ),
//             false,
//         )
//         .await;
//         assert!(put.fired.is_none(), "{store:?}: the host put stores A");
//         let expected: Vec<Cost> = take.entries.into_iter().chain(put.entries).collect();
//         assert!(!expected.is_empty());
//         assert_eq!(
//             charged, expected,
//             "{store:?}: the legacy log of the host program"
//         );
//     }
// }

// ── The merge of allocations (CounterConflict.tla, NoDuplicateIndex) ────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_allocator_race_conflict() {
    // Two branches of one block allocate from one space. Both consume the pre-state produce of
    // A(sid), so merge check #1 sees their race, and the merge rejects one of them.
    for store in [Store::Fixed(4), Store::Cyclic(4), Store::Vector] {
        let fixture = fixture(store).await;
        let root = fixture
            .space
            .create_checkpoint()
            .await
            .expect("the pre-state checkpoint")
            .root;
        let (left, left_indices) = branch(&fixture, &root, 1, &[1]).await;
        let (right, right_indices) = branch(&fixture, &root, 2, &[2]).await;
        assert_eq!(left_indices, vec![0], "{store:?}");
        assert_eq!(right_indices, vec![0, 1], "{store:?}");
        let allocator = hash(&allocator::allocator_channel(SPACE));
        assert_eq!(
            conflicting(&fixture.space, &root, &left, &right),
            BTreeSet::from([allocator.clone()]),
            "{store:?}: the branches race on the allocator"
        );
        let reason = conflict_reason(
            &merge_index(&fixture.space, &root, left.clone()),
            &merge_index(&fixture.space, &root, right.clone()),
        );
        assert!(
            reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
            "{store:?}: merge check #1 finds the race: {reason:?}"
        );
        // Negative control: without the events of the allocator, the branches merge, and both
        // hold the index 0. So the shared allocator record is necessary (LazyAllocator).
        assert!(
            conflicting(
                &fixture.space,
                &root,
                &without_channel(&left, &allocator),
                &without_channel(&right, &allocator)
            )
            .is_empty(),
            "{store:?}"
        );
        assert!(left_indices
            .iter()
            .any(|index| right_indices.contains(index)));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_allocator_same_block_one_chain() {
    // The deploys of one block run in order on one state. A later allocation consumes the
    // allocator record that an earlier one produced, so the allocations form one chain, and a
    // deploy that does not allocate stays out of it.
    let fixture = fixture(Store::Vector).await;
    let names = fixture.allocate(1, 2).await.expect("two names");
    let root = fixture
        .space
        .create_checkpoint()
        .await
        .expect("the pre-state checkpoint")
        .root;
    let mut logs = Vec::with_capacity(3);
    assert_eq!(fixture.allocate(2, 1).await, Ok(vec![2]));
    logs.push(fixture.space.take_event_log().await);
    fixture
        .run(SpaceOp::Produce(send_with_id(&name(names[0]), 4, 4)))
        .await
        .expect("a send on an allocated name");
    logs.push(fixture.space.take_event_log().await);
    assert_eq!(fixture.allocate(3, 2).await, Ok(vec![3, 4]));
    logs.push(fixture.space.take_event_log().await);
    let indices: Vec<EventLogIndex> = logs
        .into_iter()
        .map(|log| merge_index(&fixture.space, &root, log))
        .collect();
    assert!(
        depends(&indices[2], &indices[0]),
        "the second allocation follows the first"
    );
    assert!(!depends(&indices[0], &indices[2]));
    for other in [0, 2] {
        assert!(
            !depends(&indices[1], &indices[other]),
            "the send is independent"
        );
        assert!(!depends(&indices[other], &indices[1]));
    }
    let items = HashableSet((0..indices.len()).collect::<HashSet<usize>>());
    let chains: BTreeSet<BTreeSet<usize>> = compute_related_sets(&items, |left, right| {
        depends(&indices[*left], &indices[*right])
    })
    .0
    .into_iter()
    .map(|chain| chain.0.into_iter().collect())
    .collect();
    assert_eq!(
        chains,
        BTreeSet::from([BTreeSet::from([0, 2]), BTreeSet::from([1])]),
        "the allocations of one space form one chain"
    );
}

// ── Properties: OuterStorage.v ─────────────────────────────────────────────────────────────────

/// The counts of a run of allocations.
fn arb_counts() -> impl Strategy<Value = Vec<u64>> { vec(0u64..4, 1..8) }

/// Runs `counts` on a fresh space of `store`, and returns the result, the log and the allocator
/// value before and after each allocation.
async fn run_counts(
    store: Store,
    counts: &[u64],
) -> Vec<(i64, u64, Result<Vec<u64>, SpaceError>, Vec<Event>, i64)> {
    let fixture = fixture(store).await;
    let mut steps = Vec::with_capacity(counts.len());
    for (job, count) in (1u64..).zip(counts) {
        let [before] =
            <[i64; 1]>::try_from(fixture.allocator().await).expect("one allocator record");
        let result = fixture.allocate(job, *count).await;
        let log = fixture.space.take_event_log().await;
        let [after] =
            <[i64; 1]>::try_from(fixture.allocator().await).expect("one allocator record");
        steps.push((before, *count, result, log, after));
    }
    steps
}

#[test]
fn prop_fixed_array_allocates_at_most_size() {
    check(pure_config(), (1i64..6, arb_counts()), |(size, counts)| {
        tokio_runtime().block_on(async {
            let store = Store::Fixed(size);
            let config = store.space_config(SPACE);
            let mut total = 0u64;
            for (_, _, result, _, after) in run_counts(store, &counts).await {
                prop_assert!(after <= size, "the allocator never passes the size");
                if let Ok(indices) = result {
                    total += u64::try_from(indices.len()).expect("a small count");
                    for index in indices {
                        prop_assert!(i64::try_from(index).is_ok_and(|index| index < size));
                        let channel = name(index);
                        let key = ArrayOuter.key(&channel).expect("a key");
                        prop_assert_eq!(
                            ArrayOuter.check_key(&config, &channel, &key),
                            Ok(()),
                            "an allocated name passes the key check of its space"
                        );
                    }
                }
            }
            prop_assert!(i64::try_from(total).is_ok_and(|total| total <= size));
            Ok(())
        })
    });
}

#[test]
fn prop_fixed_array_out_of_names_exact() {
    check(pure_config(), (1i64..6, arb_counts()), |(size, counts)| {
        tokio_runtime().block_on(async {
            for (before, count, result, log, after) in run_counts(Store::Fixed(size), &counts).await
            {
                let fits = u64::try_from(size - before).is_ok_and(|free| count <= free);
                match result {
                    Ok(indices) => {
                        prop_assert!(fits, "an allocation succeeds only when it fits");
                        prop_assert_eq!(indices.len(), usize::try_from(count).expect("a count"));
                    }
                    Err(error) => {
                        prop_assert_eq!(error, SpaceError::OutOfNames);
                        prop_assert!(!fits, "an allocation fails only when it does not fit");
                        prop_assert!(log.is_empty(), "nothing is allocated");
                        prop_assert_eq!(after, before);
                    }
                }
            }
            Ok(())
        })
    });
}

#[test]
fn prop_cyclic_array_index_mod_size() {
    check(pure_config(), (1i64..5, arb_counts()), |(size, counts)| {
        tokio_runtime().block_on(async {
            let store = Store::Cyclic(size);
            for (before, count, result, _, after) in run_counts(store, &counts).await {
                let (indices, next) = model(store, before, count).expect("a cyclic space");
                prop_assert_eq!(result, Ok(indices));
                prop_assert_eq!(after, next);
                prop_assert!(after < size);
            }
            Ok(())
        })
    });
}

/// The stores of the properties that hold for every store.
fn arb_store() -> impl Strategy<Value = Store> {
    prop_oneof![
        (1i64..6).prop_map(Store::Fixed),
        (1i64..5).prop_map(Store::Cyclic),
        Just(Store::Vector),
    ]
}

#[test]
fn prop_allocation_batch_atomic() {
    check(
        pure_config(),
        (arb_store(), arb_counts()),
        |(store, counts)| {
            tokio_runtime().block_on(async {
                for (before, count, result, log, after) in run_counts(store, &counts).await {
                    match (result, model(store, before, count)) {
                        (Ok(indices), Some((expected, next))) => {
                            prop_assert_eq!(indices, expected);
                            prop_assert_eq!(after, next);
                            prop_assert_eq!(
                                allocator_labels(&log),
                                vec!["consume A", "comm A", "produce A"],
                                "one take and one put"
                            );
                        }
                        (Err(SpaceError::OutOfNames), None) => {
                            prop_assert!(log.is_empty());
                            prop_assert_eq!(after, before);
                        }
                        (result, expected) => {
                            return Err(TestCaseError::fail(format!(
                                "{store:?}: {result:?} against the model {expected:?}"
                            )))
                        }
                    }
                }
                Ok(())
            })
        },
    );
}

#[test]
fn prop_fixed_array_indices_sequential() {
    check(pure_config(), (1i64..8, arb_counts()), |(size, counts)| {
        tokio_runtime().block_on(async {
            let mut expected_start = 0u64;
            let mut all = Vec::new();
            for (_, _, result, _, _) in run_counts(Store::Fixed(size), &counts).await {
                if let Ok(indices) = result {
                    let count = u64::try_from(indices.len()).expect("a count");
                    prop_assert_eq!(
                        &indices,
                        &(expected_start..expected_start + count).collect::<Vec<u64>>()
                    );
                    expected_start += count;
                    all.extend(indices);
                }
            }
            prop_assert_eq!(all, (0..expected_start).collect::<Vec<u64>>());
            Ok(())
        })
    });
}

#[test]
fn prop_allocator_indices_distinct() {
    let monotone = prop_oneof![(1i64..6).prop_map(Store::Fixed), Just(Store::Vector)];
    check(
        pure_config(),
        (monotone, arb_counts()),
        |(store, counts)| {
            tokio_runtime().block_on(async {
                let mut seen = BTreeSet::new();
                for (_, _, result, _, _) in run_counts(store, &counts).await {
                    for index in result.unwrap_or_default() {
                        prop_assert!(seen.insert(index), "{store:?}: the index {index} repeats");
                    }
                }
                Ok(())
            })
        },
    );
}

#[test]
fn prop_allocator_identities_unique() {
    check(
        pure_config(),
        (arb_store(), arb_counts()),
        |(store, counts)| {
            tokio_runtime().block_on(async {
                let mut seen = HashSet::new();
                for (_, _, _, log, _) in run_counts(store, &counts).await {
                    for identity in identities(&log) {
                        prop_assert!(
                            seen.insert(identity.clone()),
                            "{store:?}: the identity {identity:?} repeats"
                        );
                    }
                }
                Ok(())
            })
        },
    );
}

// ── Properties: VectorStore.v ──────────────────────────────────────────────────────────────────

#[test]
fn prop_vector_index_is_counter() {
    check(pure_config(), arb_counts(), |counts| {
        tokio_runtime().block_on(async {
            let mut counter = 0u64;
            for (before, count, result, _, after) in run_counts(Store::Vector, &counts).await {
                prop_assert_eq!(u64::try_from(before).expect("a natural value"), counter);
                prop_assert_eq!(result, Ok((counter..counter + count).collect::<Vec<u64>>()));
                counter += count;
                prop_assert_eq!(u64::try_from(after).expect("a natural value"), counter);
            }
            Ok(())
        })
    });
}

#[test]
fn prop_vector_gensym_monotonic() {
    check(pure_config(), 1usize..9, |calls| {
        tokio_runtime().block_on(async {
            let steps = run_counts(Store::Vector, &vec![1; calls]).await;
            let indices: Vec<u64> = steps
                .into_iter()
                .map(|(_, _, result, _, _)| match result.as_deref() {
                    Ok([index]) => *index,
                    other => panic!("one name for each gensym: {other:?}"),
                })
                .collect();
            for pair in indices.windows(2) {
                prop_assert_eq!(pair[1], pair[0] + 1, "each gensym returns the next index");
            }
            prop_assert_eq!(indices.first().copied(), Some(0));
            Ok(())
        })
    });
}

#[test]
fn prop_vector_gensym_distinct() {
    check(pure_config(), arb_counts(), |counts| {
        tokio_runtime().block_on(async {
            let mut seen = BTreeSet::new();
            for (_, _, result, _, _) in run_counts(Store::Vector, &counts).await {
                for index in result.expect("a vector never runs out") {
                    prop_assert!(seen.insert(index), "the index {index} repeats");
                }
            }
            Ok(())
        })
    });
}

// ── Properties: HashMapStore.v ─────────────────────────────────────────────────────────────────

/// One operation of the map-law properties: a send of a value, or a receive, on the name with
/// the index `name` of a space of three names.
#[derive(Clone, Copy, Debug)]
enum MapOp {
    Send { id: usize, name: u64, value: i64 },
    Receive { id: usize, name: u64 },
}

impl MapOp {
    fn name(self) -> u64 {
        match self {
            MapOp::Send { name, .. } | MapOp::Receive { name, .. } => name,
        }
    }

    fn op(self) -> SpaceOp {
        match self {
            MapOp::Send {
                id,
                name: index,
                value,
            } => SpaceOp::Produce(send_with_id(&name(index), value, id)),
            MapOp::Receive { id, name: index } => {
                SpaceOp::Consume(receive_on(SPACE, &name(index), &format!("w{id}"), any()))
            }
        }
    }
}

/// A space of three allocated names.
async fn three_names() -> Fixture {
    let fixture = fixture(Store::Fixed(3)).await;
    assert_eq!(fixture.allocate(1, 3).await, Ok(vec![0, 1, 2]));
    fixture
}

/// The payloads of each collection of the space of [`three_names`], each list sorted: the view
/// of the store that does not depend on the list order of the store.
async fn payloads(fixture: &Fixture) -> BTreeMap<u64, Vec<i64>> {
    let mut view = BTreeMap::new();
    for index in 0..3 {
        let mut values: Vec<i64> = records_on(&fixture.space, &data_channel(SPACE, &name(index)))
            .await
            .into_iter()
            .map(|(_, pars, _)| value_of(&pars))
            .collect();
        values.sort_unstable();
        if !values.is_empty() {
            view.insert(index, values);
        }
    }
    view
}

#[test]
fn prop_hashmap_iteration_permutation_invariant() {
    let sends = vec((0u64..3, 0i64..5), 1..8).prop_map(|sends| {
        sends
            .into_iter()
            .enumerate()
            .map(|(id, (name, value))| MapOp::Send { id, name, value })
            .collect::<Vec<MapOp>>()
    });
    let case = sends.prop_flat_map(|ops| (Just(ops.clone()), Just(ops).prop_shuffle()));
    check(pure_config(), case, |(ops, permuted)| {
        tokio_runtime().block_on(async {
            let mut views = Vec::with_capacity(2);
            for order in [&ops, &permuted] {
                let fixture = three_names().await;
                for op in order.iter() {
                    fixture.run(op.op()).await.expect("a send commits");
                }
                views.push(payloads(&fixture).await);
            }
            prop_assert_eq!(
                &views[0],
                &views[1],
                "the view does not depend on the order"
            );
            Ok(())
        })
    });
}

#[test]
fn prop_hashmap_get_put_other() {
    let op = prop_oneof![
        3 => (0u64..3, 0i64..5).prop_map(|(name, value)| (name, Some(value))),
        1 => (0u64..3).prop_map(|name| (name, None)),
    ];
    let case = vec(op, 1..8).prop_map(|ops| {
        ops.into_iter()
            .enumerate()
            .map(|(id, (name, value))| match value {
                Some(value) => MapOp::Send { id, name, value },
                None => MapOp::Receive { id, name },
            })
            .collect::<Vec<MapOp>>()
    });
    check(pure_config(), case, |ops| {
        tokio_runtime().block_on(async {
            let fixture = three_names().await;
            for op in ops {
                let mut others = Vec::with_capacity(2);
                for index in (0..3).filter(|index| *index != op.name()) {
                    let data = data_channel(SPACE, &name(index));
                    others.push((
                        index,
                        records_on(&fixture.space, &data).await,
                        waiters_on(&fixture.space, &data).await,
                    ));
                }
                fixture.run(op.op()).await.expect("the operation commits");
                for (index, records, waiters) in others {
                    let data = data_channel(SPACE, &name(index));
                    prop_assert_eq!(records_on(&fixture.space, &data).await, records);
                    prop_assert_eq!(waiters_on(&fixture.space, &data).await, waiters);
                }
            }
            Ok(())
        })
    });
}

// ── Property: CounterConflict.tla NoDuplicateIndex ──────────────────────────────────────────────

/// Two branches of one block from one pre-state: the logs of the branches, whether check #1
/// finds a race between them, and the indices that each branch allocated.
struct Branches {
    left: Vec<Event>,
    right: Vec<Event>,
    race: bool,
    left_indices: Vec<u64>,
    right_indices: Vec<u64>,
}

/// Runs the allocations `prefix` on a fresh space of `store`, checkpoints the pre-state, and runs
/// the branches `left_counts` and `right_counts` from it.
async fn two_branches(
    store: Store,
    prefix: &[u64],
    left_counts: &[u64],
    right_counts: &[u64],
) -> Branches {
    let fixture = fixture(store).await;
    for (job, count) in (100u64..).zip(prefix) {
        // The prefix sets up the allocator. An allocation that does not fit changes nothing.
        let _ = fixture.allocate(job, *count).await;
    }
    let root = fixture
        .space
        .create_checkpoint()
        .await
        .expect("the pre-state checkpoint")
        .root;
    let (left, left_indices) = branch(&fixture, &root, 1, left_counts).await;
    let (right, right_indices) = branch(&fixture, &root, 10, right_counts).await;
    let race = !conflicting(&fixture.space, &root, &left, &right).is_empty();
    Branches {
        left,
        right,
        race,
        left_indices,
        right_indices,
    }
}

/// A store, a prefix of allocations, and the allocations of two branches.
fn arb_branches() -> impl Strategy<Value = (Store, Vec<u64>, Vec<u64>, Vec<u64>)> {
    (
        arb_store(),
        vec(0u64..3, 0..2),
        vec(1u64..3, 0..3),
        vec(1u64..3, 0..3),
    )
}

#[test]
fn prop_no_duplicate_index() {
    check(
        pure_config(),
        arb_branches(),
        |(store, prefix, left_counts, right_counts)| {
            tokio_runtime().block_on(async {
                let branches = two_branches(store, &prefix, &left_counts, &right_counts).await;
                let left: BTreeSet<u64> = branches.left_indices.iter().copied().collect();
                let shared = branches
                    .right_indices
                    .iter()
                    .any(|index| left.contains(index));
                // NoDuplicateIndex: a merge without a race never hands one index to both
                // branches.
                prop_assert!(
                    branches.race || !shared,
                    "{store:?}: the branches share an index without a race"
                );
                Ok(())
            })
        },
    );
}

#[test]
fn prop_allocations_race() {
    check(
        pure_config(),
        arb_branches(),
        |(store, prefix, left_counts, right_counts)| {
            tokio_runtime().block_on(async {
                let branches = two_branches(store, &prefix, &left_counts, &right_counts).await;
                // AllocationsRace: two branches that both allocate race on the allocator.
                let allocates = |log: &[Event]| allocator_labels(log).contains(&"comm A");
                if allocates(&branches.left) && allocates(&branches.right) {
                    prop_assert!(branches.race, "{store:?}: two allocating branches race");
                }
                Ok(())
            })
        },
    );
}

// ── The params and the rules of the stores ────────────────────────────────────────────────────

#[test]
fn space_array_params_and_rules_are_pinned() {
    // The names of the Array params, as the factory schema of FACTORY spells them (F7).
    assert_eq!(SIZE_PARAM, "size");
    assert_eq!(CYCLIC_PARAM, "cyclic");
    assert_eq!(Store::Fixed(3).rule(), AllocationRule::Fixed { size: 3 });
    assert_eq!(Store::Cyclic(3).rule(), AllocationRule::Cyclic { size: 3 });
    assert_eq!(Store::Vector.rule(), AllocationRule::Unbounded);
    // The model and the rule of the store plan the same allocations.
    for store in [Store::Fixed(3), Store::Cyclic(3), Store::Vector] {
        for value in 0..3 {
            for count in 0..5 {
                let planned = store
                    .rule()
                    .plan(value, count)
                    .map(|plan| (plan.indices, plan.next));
                match model(store, value, count) {
                    Some(expected) => assert_eq!(planned, Ok(expected), "{store:?}"),
                    None => assert_eq!(planned, Err(SpaceError::OutOfNames), "{store:?}"),
                }
            }
        }
    }
}
