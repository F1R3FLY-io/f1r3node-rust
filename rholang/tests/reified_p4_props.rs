//! The property tests of the P4 formal invariants of Reified RSpaces (P4 design E.3, readiness
//! review §4 and §6): the first working space, `hashmap:queue`.
//!
//! Each test extracts one headline theorem or one invariant of the P4 formal models as a
//! property-based test. The invariant map (`formal/reified-rspaces-invariant-map.json`, phase
//! `P4`) names each test with its formal entries.
//!
//! | Group       | Formal source                                               | Tests |
//! |-------------|-------------------------------------------------------------|-------|
//! | Namespace   | `Namespace.v`                                               | 11    |
//! | Envelope    | `RecordEnvelope.v`                                          | 8     |
//! | Counter     | `CounterConflict.v`                                         | 9     |
//! | Queue       | `HeadOnlyDisciplines.v` (the queue part)                    | 9     |
//! | Refs        | `SpaceRefs.v`                                               | 9     |
//! | Factory     | `SpaceFactory.v` (readiness review §4 restates one headline) | 8     |
//! | TLA+        | `HeadOnlyDiscipline`, `CounterConflict`, `NativeHandles`    | 8     |
//! | Leaf extras | the P4 leaves                                               | 4     |
//!
//! The tests drive the real code:
//!
//! * The pure properties call the namespace, record, routing, factory and merger functions.
//!   They run 64 cases.
//! * The queue properties run the real agent `GenericSpace<HashMapOuter, QueueDiscipline>` over
//!   an in-memory RSpace. They run 64 cases.
//! * The properties that need a Rholang runtime evaluate deploys on the play and replay
//!   runtimes of `create_runtimes`. The merge properties also run the merger of HEAD: check #1
//!   to #3, `depends`, `StateChange` and the trie actions. These properties run 16 cases.
//!
//! A deploy names its Reified ids through the normalizer environment. The program binds a name
//! with ``new x(`uri`)``, and the environment maps the URI to the id. A program cannot forge an
//! unforgeable name, so a test hands a space name or a handle to a later deploy this way.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, CostAuthority, CostRegion, CostSignature, EList, Expr, GDeployId,
    GDeployerId, GUnforgeable, ListParWithRandom, New, Par, ParWithRandom, TaggedContinuation, Var,
};
use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage};
use models::rust::reified::encoding::{
    decode_process_tuple, encode_free, encode_params, Marker, ProcessConstruct, SendParams,
    UseBlock,
};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::space_factory_urn::{
    SpaceFactoryConstructor, SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier,
    FACTORY_FAMILY_COUNT, HASHMAP_BAG_ALIAS, PATHMAP_BAG_ALIAS, SPACE_URN_PREFIX,
};
use models::rust::utils::{new_bundle_par, new_freevar_par, new_gbytearray_par};
use proptest::array::uniform32;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{RngSeed, TestCaseError, TestRunner};
use reified_strategies::{arb_ground_par, arb_par, gint, gstring, list, tuple};
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{RuntimeBudget, Sig};
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::reified::costs::{ReissueCause, ScanMeter, FIRE_ROUND_CAP};
use rholang::rust::interpreter::reified::errors::ReifiedReduceError;
use rholang::rust::interpreter::reified::factory::{handle_continuation, handle_pattern};
use rholang::rust::interpreter::reified::handle::typed_error;
use rholang::rust::interpreter::reified::route::{classify_channel, route, ChannelClass, Route};
use rholang::rust::interpreter::reified::scope::{
    classify_space_ref, wrap_body, SpaceCtx, SpaceRef, SpaceRefError, UseTarget,
};
use rholang::rust::interpreter::reified::system_channels::{
    body_ref, check_persistent_send, handle_of, op_of, HASHMAP_QUEUE_FACTORY, P4_SYSTEM_CHANNELS,
};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::agent::{
    run_op, ConsumeOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
    SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::errors::{
    SpaceError, SpaceReplyCode, SPACE_ERROR_REPLY_TAG,
};
use rholang::rust::interpreter::spaces::factory::constructor::{Execution, FactoryConstructor};
use rholang::rust::interpreter::spaces::factory::defaults::{constructor_schema, required_params};
use rholang::rust::interpreter::spaces::factory::urn::{factory_capability, factory_channel};
use rholang::rust::interpreter::spaces::generic::{
    CollectionChannels, Discipline, GenericSpace, Ordered, OuterStore, Side,
};
use rholang::rust::interpreter::spaces::namespace::{
    self, canonical_inner_channel_bytes, decode_reified_id, gprivate, reified_id_of,
    sole_gprivate_id, CollectionKey, Durability, Kind, NameTail, NativeSpace, RandomPurpose,
    ReifiedId, SpaceId, AGENT_TAG, ANN, ASSERT_EQUAL, EMBED, EPHEMERAL_BIT, FACTORY_LAST,
    GET_SPACE_AGENT, HANDLE_OP, MATCHER_OP, NATIVE_ENGINE_FACTORY, REIFIED_TAG, SID_LEN, SINK_OP,
    SPACE_ID_PREFIX_LEN, VECTOR,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record::{
    config_map, config_record, continuation_envelope, continuation_ord, counter_record,
    counter_value, datum_envelope, datum_ord, unwrap_continuation, unwrap_datum, wrap_continuation,
    wrap_datum, ContinuationRecord, DatumRecord, MalformedRecord, RecordKind, RECORD_TAG_MARK,
    RECORD_VERSION,
};
use rholang::rust::interpreter::spaces::theory::builtin::BUILTIN_NAMES;
use rholang::rust::interpreter::spaces::theory::identity::TheoryIdentity;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::id::derive_space_id;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier as ConfigQualifier;
use rholang::rust::interpreter::system_processes::byte_name;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::allocate_new_bindings;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash as stable_hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    are_conflicting, conflict_reason, conflicts, depends, MergeType, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger::compute_trie_actions;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce, SpaceStoreHandle,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};

// ─── Configuration ───────────────────────────────────────────────────────────────────────────

/// The fixed seed of every property (P4 batch 4E-T1).
const PROPERTY_SEED: u64 = 0x0E0D_2026_1005_4E71;

/// The cases of a pure property and of a property over the in-memory agent.
const PURE_CASES: u32 = 64;

/// The cases of a property that evaluates deploys on a Rholang runtime.
const RUNTIME_CASES: u32 = 16;

fn pure_config() -> ProptestConfig {
    ProptestConfig {
        cases: PURE_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn runtime_config() -> ProptestConfig {
    ProptestConfig {
        cases: RUNTIME_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    }
}

/// Runs the property `test` over `strategy` with `config`. A failure reports the reason and the
/// minimal failing input, as the `proptest!` macro does.
fn check<S>(
    config: ProptestConfig,
    strategy: S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    TestRunner::new(config)
        .run(&strategy, test)
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// The tokio runtime of one case. The Rholang runtime spawns tasks, so it needs worker threads.
fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build the tokio runtime of a case")
}

// ─── Shared values ───────────────────────────────────────────────────────────────────────────

fn gbool(value: bool) -> Par { reified_strategies::gbool(value) }

fn bytes_par(bytes: &[u8]) -> Par {
    reified_strategies::expr_par(ExprInstance::GByteArray(bytes.to_vec()), Vec::new(), false)
}

/// A cost authority with one region, as the accounting layer stamps it.
fn region(byte: u8) -> CostAuthority {
    CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![byte; 32],
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::Ground(vec![byte])),
            }),
        }],
    }
}

fn arb_authority() -> BoxedStrategy<Option<CostAuthority>> {
    prop_oneof![Just(None), any::<u8>().prop_map(|byte| Some(region(byte)))].boxed()
}

/// A real random state, as the reducer derives it.
fn arb_random() -> BoxedStrategy<Blake2b512Random> {
    vec(any::<u8>(), 0..12)
        .prop_map(|seed| Blake2b512Random::create_from_bytes(&seed))
        .boxed()
}

fn sorted(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

/// The same `Par` with each component list reversed. It sorts to the value of `par`.
fn reversed_components(par: &Par) -> Par {
    let mut reversed = par.clone();
    reversed.sends.reverse();
    reversed.receives.reverse();
    reversed.news.reverse();
    reversed.exprs.reverse();
    reversed.matches.reverse();
    reversed.unforgeables.reverse();
    reversed.bundles.reverse();
    reversed.connectives.reverse();
    reversed.conditionals.reverse();
    reversed
}

// ─── Namespace (Namespace.v) ─────────────────────────────────────────────────────────────────

const DURABILITIES: [Durability; 2] = [Durability::Durable, Durability::Ephemeral];

/// The uppercase KIND letters of the namespace table.
const KIND_LETTERS: [u8; 11] = *b"DCIASHNXBMT";

fn arb_durability() -> BoxedStrategy<Durability> { select(DURABILITIES.to_vec()).boxed() }

fn arb_kind() -> BoxedStrategy<Kind> { select(Kind::ALL.to_vec()).boxed() }

/// A space id from a pool of three, so that equal ids occur, or any id.
fn arb_sid() -> BoxedStrategy<SpaceId> {
    prop_oneof![
        (0u8..3).prop_map(|seed| [seed; SID_LEN]),
        uniform32(any::<u8>())
    ]
    .boxed()
}

fn arb_space() -> BoxedStrategy<NativeSpace> {
    (arb_sid(), arb_durability())
        .prop_map(|(sid, durability)| NativeSpace { sid, durability })
        .boxed()
}

/// The bytes of a collection key from a pool of three, so that equal keys occur, or any key.
fn arb_key_bytes() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![
        (1u8..4).prop_map(|byte| vec![byte]),
        vec(any::<u8>(), 1..48)
    ]
    .boxed()
}

fn collection_key(bytes: &[u8]) -> CollectionKey {
    CollectionKey::from_bytes(bytes.to_vec()).expect("a generated key is not empty")
}

fn arb_tail() -> BoxedStrategy<NameTail> {
    prop_oneof![
        uniform32(any::<u8>()).prop_map(NameTail::Random),
        any::<u64>().prop_map(NameTail::Index),
    ]
    .boxed()
}

/// One Reified id of each form of the namespace table. The test encodes it with the
/// constructors of `namespace.rs`. The kinds that P4 has no constructor for (path index,
/// allocator, matcher, body and the reserved kinds) use the documented layout `E0‖KIND‖payload`.
#[derive(Clone, Debug, PartialEq)]
enum IdSpec {
    Op(u8),
    Config(NativeSpace),
    SpaceCounter(NativeSpace),
    CollectionCounter(NativeSpace, Vec<u8>),
    Data(NativeSpace, Vec<u8>),
    PathIndex(NativeSpace, Vec<u8>),
    Allocator(NativeSpace),
    Handle(NativeSpace),
    Name(NativeSpace, NameTail),
    Matcher(Durability, Vec<u8>),
    Body(Durability, [u8; SID_LEN]),
    Reserved(Kind, Vec<u8>),
}

fn kind_id(kind: Kind, durability: Durability, payload: &[u8]) -> Vec<u8> {
    let mut id = Vec::with_capacity(2 + payload.len());
    id.push(REIFIED_TAG);
    id.push(kind.byte(durability).expect("the kind has this durability"));
    id.extend_from_slice(payload);
    id
}

impl IdSpec {
    fn encode(&self) -> Vec<u8> {
        match self {
            IdSpec::Op(op) => namespace::op_channel(*op),
            IdSpec::Config(space) => namespace::config_channel(*space),
            IdSpec::SpaceCounter(space) => namespace::space_counter(*space),
            IdSpec::CollectionCounter(space, key) => {
                namespace::collection_counter(*space, &collection_key(key))
            }
            IdSpec::Data(space, key) => namespace::data_channel(*space, &collection_key(key)),
            IdSpec::PathIndex(space, prefix) => kind_id(
                Kind::PathIndex,
                space.durability,
                &[&space.sid[..], prefix].concat(),
            ),
            IdSpec::Allocator(space) => kind_id(Kind::Allocator, space.durability, &space.sid),
            IdSpec::Handle(space) => namespace::handle_channel(*space),
            IdSpec::Name(space, tail) => namespace::name_id(*space, *tail),
            IdSpec::Matcher(durability, state) => kind_id(Kind::Matcher, *durability, state),
            IdSpec::Body(durability, tag) => kind_id(Kind::Body, *durability, tag),
            IdSpec::Reserved(kind, payload) => kind_id(*kind, Durability::Durable, payload),
        }
    }

    /// The value that `decode_reified_id` must return for the encoding.
    fn decoded(&self) -> ReifiedId<'_> {
        match self {
            IdSpec::Op(op) => ReifiedId::Op(*op),
            IdSpec::Config(space) => ReifiedId::Config(*space),
            IdSpec::SpaceCounter(space) => ReifiedId::SpaceCounter(*space),
            IdSpec::CollectionCounter(space, key) => ReifiedId::CollectionCounter(*space, key),
            IdSpec::Data(space, key) => ReifiedId::Data(*space, key),
            IdSpec::PathIndex(space, prefix) => ReifiedId::PathIndex(*space, prefix),
            IdSpec::Allocator(space) => ReifiedId::Allocator(*space),
            IdSpec::Handle(space) => ReifiedId::Handle(*space),
            IdSpec::Name(space, tail) => ReifiedId::Name(*space, *tail),
            IdSpec::Matcher(durability, state) => ReifiedId::Matcher(*durability, state),
            IdSpec::Body(durability, tag) => ReifiedId::Body(*durability, &tag[..]),
            IdSpec::Reserved(kind, payload) => ReifiedId::Reserved(*kind, payload),
        }
    }

    fn of_decoded(id: &ReifiedId<'_>) -> IdSpec {
        match *id {
            ReifiedId::Op(op) => IdSpec::Op(op),
            ReifiedId::Config(space) => IdSpec::Config(space),
            ReifiedId::SpaceCounter(space) => IdSpec::SpaceCounter(space),
            ReifiedId::CollectionCounter(space, key) => {
                IdSpec::CollectionCounter(space, key.to_vec())
            }
            ReifiedId::Data(space, key) => IdSpec::Data(space, key.to_vec()),
            ReifiedId::PathIndex(space, prefix) => IdSpec::PathIndex(space, prefix.to_vec()),
            ReifiedId::Allocator(space) => IdSpec::Allocator(space),
            ReifiedId::Handle(space) => IdSpec::Handle(space),
            ReifiedId::Name(space, tail) => IdSpec::Name(space, tail),
            ReifiedId::Matcher(durability, state) => IdSpec::Matcher(durability, state.to_vec()),
            ReifiedId::Body(durability, tag) => IdSpec::Body(
                durability,
                <[u8; SID_LEN]>::try_from(tag).expect("a decoded body tag has 32 bytes"),
            ),
            ReifiedId::Reserved(kind, payload) => IdSpec::Reserved(kind, payload.to_vec()),
        }
    }

    /// The durability of the KIND byte, or `None` for an op id.
    fn durability(&self) -> Option<Durability> {
        match self {
            IdSpec::Op(_) => None,
            IdSpec::Config(space)
            | IdSpec::SpaceCounter(space)
            | IdSpec::CollectionCounter(space, _)
            | IdSpec::Data(space, _)
            | IdSpec::PathIndex(space, _)
            | IdSpec::Allocator(space)
            | IdSpec::Handle(space)
            | IdSpec::Name(space, _) => Some(space.durability),
            IdSpec::Matcher(durability, _) | IdSpec::Body(durability, _) => Some(*durability),
            IdSpec::Reserved(..) => Some(Durability::Durable),
        }
    }

    /// The same id with the durability `durability`. An op id and a reserved id keep theirs.
    fn with_durability(self, durability: Durability) -> IdSpec {
        let moved = |space: NativeSpace| NativeSpace {
            durability,
            ..space
        };
        match self {
            IdSpec::Op(_) | IdSpec::Reserved(..) => self,
            IdSpec::Config(space) => IdSpec::Config(moved(space)),
            IdSpec::SpaceCounter(space) => IdSpec::SpaceCounter(moved(space)),
            IdSpec::CollectionCounter(space, key) => IdSpec::CollectionCounter(moved(space), key),
            IdSpec::Data(space, key) => IdSpec::Data(moved(space), key),
            IdSpec::PathIndex(space, prefix) => IdSpec::PathIndex(moved(space), prefix),
            IdSpec::Allocator(space) => IdSpec::Allocator(moved(space)),
            IdSpec::Handle(space) => IdSpec::Handle(moved(space)),
            IdSpec::Name(space, tail) => IdSpec::Name(moved(space), tail),
            IdSpec::Matcher(_, state) => IdSpec::Matcher(durability, state),
            IdSpec::Body(_, tag) => IdSpec::Body(durability, tag),
        }
    }
}

fn arb_id_spec() -> BoxedStrategy<IdSpec> {
    prop_oneof![
        any::<u8>().prop_map(IdSpec::Op),
        arb_space().prop_map(IdSpec::Config),
        arb_space().prop_map(IdSpec::SpaceCounter),
        (arb_space(), arb_key_bytes())
            .prop_map(|(space, key)| IdSpec::CollectionCounter(space, key)),
        (arb_space(), arb_key_bytes()).prop_map(|(space, key)| IdSpec::Data(space, key)),
        (arb_space(), vec(any::<u8>(), 0..40))
            .prop_map(|(space, prefix)| IdSpec::PathIndex(space, prefix)),
        arb_space().prop_map(IdSpec::Allocator),
        arb_space().prop_map(IdSpec::Handle),
        (arb_space(), arb_tail()).prop_map(|(space, tail)| IdSpec::Name(space, tail)),
        (arb_durability(), vec(any::<u8>(), 32..72))
            .prop_map(|(durability, state)| IdSpec::Matcher(durability, state)),
        (arb_durability(), uniform32(any::<u8>()))
            .prop_map(|(durability, tag)| IdSpec::Body(durability, tag)),
        (
            select(vec![Kind::Engine, Kind::Theory]),
            vec(any::<u8>(), 32..72)
        )
            .prop_map(|(kind, payload)| IdSpec::Reserved(kind, payload)),
    ]
    .boxed()
}

/// Byte strings near the accepted ids. Each is a tag, a KIND byte and a payload of an
/// interesting length, a short string, or an encoded id with one edit.
fn arb_raw_id() -> BoxedStrategy<Vec<u8>> {
    let kind_byte = prop_oneof![
        3 => select(KIND_LETTERS.to_vec()),
        3 => select(KIND_LETTERS.to_vec()).prop_map(|letter| letter | EPHEMERAL_BIT),
        1 => any::<u8>(),
    ];
    let tag = prop_oneof![5 => Just(REIFIED_TAG), 1 => any::<u8>()];
    let payload_len = prop_oneof![
        Just(0usize),
        Just(30usize),
        Just(31usize),
        Just(32usize),
        Just(33usize),
        Just(40usize),
        Just(64usize),
        0usize..72,
    ];
    prop_oneof![
        3 => (tag, kind_byte, payload_len).prop_flat_map(|(tag, kind, len)| {
            vec(any::<u8>(), len).prop_map(move |payload| {
                let mut id = Vec::with_capacity(2 + payload.len());
                id.push(tag);
                id.push(kind);
                id.extend(payload);
                id
            })
        }),
        1 => vec(any::<u8>(), 0..4),
        2 => (arb_id_spec(), any::<u8>(), 0u8..3).prop_map(|(spec, byte, edit)| {
            let mut id = spec.encode();
            match edit {
                0 => {
                    id.pop();
                }
                1 => id.push(byte),
                _ => {
                    if let Some(last) = id.last_mut() {
                        *last ^= byte;
                    }
                }
            }
            id
        }),
    ]
    .boxed()
}

/// Every op index that the plan names. The list holds the 32 factories, the engine factory,
/// getSpaceAgent, assertEqual and vector. It also holds the three internal ops, the five syntax
/// markers and the agent tag.
fn named_ops() -> Vec<u8> {
    let mut ops: Vec<u8> = (0..=FACTORY_LAST).collect();
    ops.extend([
        NATIVE_ENGINE_FACTORY,
        GET_SPACE_AGENT,
        ASSERT_EQUAL,
        VECTOR,
        EMBED,
        ANN,
        HANDLE_OP,
        MATCHER_OP,
        SINK_OP,
    ]);
    ops.extend(Marker::ALL.iter().map(|marker| *marker as u8));
    ops.push(AGENT_TAG);
    ops
}

fn use_ctx(space: NativeSpace) -> SpaceCtx {
    Some(Arc::new(UseTarget {
        space: gprivate(namespace::handle_channel(space)),
        native: space,
    }))
}

fn arb_ctx() -> BoxedStrategy<SpaceCtx> {
    prop_oneof![Just(None), arb_space().prop_map(use_ctx)].boxed()
}

/// `kind_byte_injective`: each kind and durability has one KIND byte. The ephemeral byte is
/// the durable byte with the bit 0x20, and `M` and `T` have no ephemeral form.
/// `Kind::from_byte` inverts `Kind::byte` on every byte.
#[test]
fn prop_kind_byte_injective() {
    check(
        pure_config(),
        (
            arb_kind(),
            arb_durability(),
            arb_kind(),
            arb_durability(),
            any::<u8>(),
        ),
        |(first, first_durability, second, second_durability, byte)| {
            if let (Some(a), Some(b)) =
                (first.byte(first_durability), second.byte(second_durability))
            {
                prop_assert_eq!(
                    a == b,
                    (first, first_durability) == (second, second_durability)
                );
            }
            if let Some(a) = first.byte(first_durability) {
                prop_assert_eq!(Kind::from_byte(a), Some((first, first_durability)));
            }
            prop_assert_eq!(
                first.byte(Durability::Ephemeral).is_none(),
                matches!(first, Kind::Engine | Kind::Theory)
            );
            let durable = first
                .byte(Durability::Durable)
                .expect("every kind has a durable byte");
            prop_assert!(durable.is_ascii_uppercase());
            if let Some(ephemeral) = first.byte(Durability::Ephemeral) {
                prop_assert_eq!(ephemeral, durable | EPHEMERAL_BIT);
            }
            if let Some((kind, durability)) = Kind::from_byte(byte) {
                prop_assert_eq!(kind.byte(durability), Some(byte));
            }
            let mut table = BTreeMap::new();
            for kind in Kind::ALL {
                for durability in DURABILITIES {
                    if let Some(encoded) = kind.byte(durability) {
                        prop_assert!(table.insert(encoded, (kind, durability)).is_none());
                    }
                }
            }
            prop_assert_eq!(table.len(), 2 * Kind::ALL.len() - 2);
            for candidate in 0..=u8::MAX {
                prop_assert_eq!(Kind::from_byte(candidate), table.get(&candidate).copied());
            }
            Ok(())
        },
    );
}

/// `storage_id_injective`: the data channel and the counters are injective in their space
/// and key, and the three storage forms never share an id.
#[test]
fn prop_storage_id_injective() {
    check(
        pure_config(),
        (arb_space(), arb_space(), arb_key_bytes(), arb_key_bytes()),
        |(first_space, second_space, first_key, second_key)| {
            let (first, second) = (collection_key(&first_key), collection_key(&second_key));
            let same = first_space == second_space && first == second;
            prop_assert_eq!(
                namespace::data_channel(first_space, &first)
                    == namespace::data_channel(second_space, &second),
                same
            );
            prop_assert_eq!(
                namespace::collection_counter(first_space, &first)
                    == namespace::collection_counter(second_space, &second),
                same
            );
            prop_assert_eq!(
                namespace::space_counter(first_space) == namespace::space_counter(second_space),
                first_space == second_space
            );
            prop_assert_ne!(
                namespace::data_channel(first_space, &first),
                namespace::collection_counter(second_space, &second)
            );
            prop_assert_ne!(
                namespace::data_channel(first_space, &first),
                namespace::space_counter(second_space)
            );
            prop_assert_ne!(
                namespace::space_counter(first_space),
                namespace::collection_counter(second_space, &second)
            );
            Ok(())
        },
    );
}

/// `handle_id_injective`: `E0‖H‖sid` is injective in the space, differs from every other
/// constructor, decodes to its space, and is the only id that `handle_of` accepts.
#[test]
fn prop_handle_id_injective() {
    check(
        pure_config(),
        (
            arb_space(),
            arb_space(),
            arb_key_bytes(),
            arb_tail(),
            any::<u8>(),
        ),
        |(first, second, key, tail, op)| {
            let handle = namespace::handle_channel(first);
            let key = collection_key(&key);
            prop_assert_eq!(handle == namespace::handle_channel(second), first == second);
            prop_assert_ne!(&handle, &namespace::config_channel(second));
            prop_assert_ne!(&handle, &namespace::space_counter(second));
            prop_assert_ne!(&handle, &namespace::collection_counter(second, &key));
            prop_assert_ne!(&handle, &namespace::data_channel(second, &key));
            prop_assert_ne!(&handle, &namespace::name_id(second, tail));
            prop_assert_ne!(&handle, &namespace::op_channel(op));
            prop_assert_eq!(decode_reified_id(&handle), Some(ReifiedId::Handle(first)));
            prop_assert_eq!(handle_of(&gprivate(handle.clone())), Some(first));
            prop_assert_eq!(handle_of(&gprivate(namespace::config_channel(first))), None);
            Ok(())
        },
    );
}

/// `ephemeral_durable_disjoint`: an ephemeral id never equals a durable id, and the bit
/// 0x20 of the KIND byte is set exactly for an ephemeral id.
#[test]
fn prop_ephemeral_durable_disjoint() {
    check(
        pure_config(),
        (arb_id_spec(), arb_id_spec()),
        |(first, second)| {
            let ephemeral = first.with_durability(Durability::Ephemeral);
            let durable = second.with_durability(Durability::Durable);
            if let (Some(Durability::Ephemeral), Some(Durability::Durable)) =
                (ephemeral.durability(), durable.durability())
            {
                prop_assert_ne!(ephemeral.encode(), durable.encode());
            }
            for spec in [&ephemeral, &durable] {
                if let Some(durability) = spec.durability() {
                    let id = spec.encode();
                    let kind_byte = id[1];
                    prop_assert_eq!(
                        kind_byte & EPHEMERAL_BIT != 0,
                        durability == Durability::Ephemeral
                    );
                    prop_assert_eq!(
                        Kind::from_byte(kind_byte).map(|(_, read)| read),
                        Some(durability)
                    );
                    prop_assert_eq!(
                        kind_byte.is_ascii_lowercase(),
                        durability == Durability::Ephemeral
                    );
                }
            }
            Ok(())
        },
    );
}

/// `reified_ids_disjoint_from_new_ids`: a Reified id has 2 bytes or at least 34. So a name of
/// `new` (32 bytes from `allocate_new_bindings`) never decodes as a Reified id.
#[test]
fn prop_reified_ids_disjoint_from_new_ids() {
    check(
        pure_config(),
        (arb_id_spec(), vec(any::<u8>(), 0..32), 1usize..4),
        |(spec, seed, binders)| {
            let id = spec.encode();
            prop_assert!(id.len() == 2 || id.len() >= SPACE_ID_PREFIX_LEN);
            let new = New {
                bind_count: i32::try_from(binders).expect("a few binders"),
                p: Some(Par::default()),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            };
            let mut rand = Blake2b512Random::create_from_bytes(&seed);
            let bound = allocate_new_bindings(&new, &Env::new(), &mut rand, &HashMap::new())
                .expect("the names of a new are allocated");
            for level in 0..binders {
                let name = bound
                    .env_map
                    .get(&i32::try_from(level).expect("a few binders"))
                    .expect("every binder has a name");
                let new_id = sole_gprivate_id(name).expect("a new name is one GPrivate");
                prop_assert_eq!(new_id.len(), 32);
                prop_assert_ne!(new_id, id.as_slice());
                prop_assert_eq!(decode_reified_id(new_id), None);
                prop_assert_eq!(classify_channel(name), ChannelClass::Unforgeable);
            }
            Ok(())
        },
    );
}

/// `reified_ids_disjoint_from_byte_names_and_mettail`: a Reified id starts with 0xE0 and has
/// more than one byte. So it is never a byte name of the system processes (1 byte). It is
/// also never a MeTTaIL id (first byte 0xF0 to 0xF9).
#[test]
fn prop_reified_ids_disjoint_from_byte_names_and_mettail() {
    check(
        pure_config(),
        (
            arb_id_spec(),
            any::<u8>(),
            0xF0u8..=0xF9,
            vec(any::<u8>(), 33),
        ),
        |(spec, byte, mettail_tag, payload)| {
            let id = spec.encode();
            prop_assert_ne!(id.len(), 1);
            prop_assert_eq!(id.first().copied(), Some(REIFIED_TAG));
            let name = byte_name(byte);
            prop_assert_eq!(sole_gprivate_id(&name), Some(&[byte][..]));
            prop_assert_ne!(&name, &gprivate(id.clone()));
            prop_assert_eq!(reified_id_of(&name), None);
            prop_assert_eq!(classify_channel(&name), ChannelClass::System);
            let mut mettail = Vec::with_capacity(1 + payload.len());
            mettail.push(mettail_tag);
            mettail.extend(payload);
            prop_assert_eq!(mettail.len(), SPACE_ID_PREFIX_LEN);
            prop_assert_ne!(&mettail, &id);
            prop_assert_eq!(decode_reified_id(&mettail), None);
            prop_assert_eq!(
                classify_channel(&gprivate(mettail)),
                ChannelClass::Unforgeable
            );
            Ok(())
        },
    );
}

/// `op_ids_pairwise_distinct`: the 45 named op indices are distinct, and `op_channel` is
/// injective. An op id differs from every id of a space, and the decoder reads it as `Op`.
/// The P4 system channels have distinct ops and URNs. Their body refs are `200 + op`, apart
/// from the HEAD body refs.
#[test]
fn prop_op_ids_pairwise_distinct() {
    check(
        pure_config(),
        (any::<u8>(), any::<u8>(), arb_id_spec()),
        |(first, second, spec)| {
            let named = named_ops();
            prop_assert_eq!(named.len(), 47);
            prop_assert_eq!(named.iter().collect::<BTreeSet<_>>().len(), 47);
            prop_assert_eq!(
                namespace::op_channel(first) == namespace::op_channel(second),
                first == second
            );
            let op_id = namespace::op_channel(first);
            prop_assert_eq!(decode_reified_id(&op_id), Some(ReifiedId::Op(first)));
            prop_assert_eq!(op_of(&gprivate(op_id.clone())), Some(first));
            if !matches!(spec, IdSpec::Op(_)) {
                prop_assert_ne!(namespace::op_channel(first), spec.encode());
            }
            for marker in Marker::ALL {
                prop_assert_eq!(marker.id(), [REIFIED_TAG, marker as u8]);
                prop_assert!(named.contains(&(marker as u8)));
                prop_assert_eq!(op_of(&marker.par()), Some(marker as u8));
            }
            let ops: BTreeSet<u8> = P4_SYSTEM_CHANNELS
                .iter()
                .map(|channel| channel.op)
                .collect();
            let urns: BTreeSet<&str> = P4_SYSTEM_CHANNELS
                .iter()
                .map(|channel| channel.urn)
                .collect();
            prop_assert_eq!(ops.len(), P4_SYSTEM_CHANNELS.len());
            prop_assert_eq!(urns.len(), P4_SYSTEM_CHANNELS.len());
            for channel in P4_SYSTEM_CHANNELS {
                prop_assert!(named.contains(&channel.op));
                prop_assert_eq!(channel.body_ref(), 200 + i64::from(channel.op));
                prop_assert!(!(0..=36).contains(&channel.body_ref()));
                prop_assert!(!(101..=108).contains(&channel.body_ref()));
                prop_assert_eq!(op_of(&channel.channel()), Some(channel.op));
            }
            Ok(())
        },
    );
}

/// `decode_encode_reified_id` (with `decode_sound`): the decoder inverts every constructor,
/// and every id that it accepts is the encoding of its result.
#[test]
fn prop_decode_reified_id_roundtrip_and_sound() {
    check(
        pure_config(),
        (arb_id_spec(), arb_raw_id()),
        |(spec, raw)| {
            let encoded = spec.encode();
            prop_assert_eq!(decode_reified_id(&encoded), Some(spec.decoded()));
            if let Some(decoded) = decode_reified_id(&raw) {
                prop_assert_eq!(IdSpec::of_decoded(&decoded).encode(), raw.clone());
            }
            let name = gprivate(raw.clone());
            prop_assert_eq!(reified_id_of(&name), decode_reified_id(&raw));
            let mut with_expr = name;
            with_expr.exprs.push(Expr {
                expr_instance: Some(ExprInstance::GInt(0)),
            });
            prop_assert_eq!(reified_id_of(&with_expr), None);
            Ok(())
        },
    );
}

/// `space_and_collection_counters_distinct`: the space counter `S(sid)` has 34 bytes, and a
/// collection counter `S(sid‖key)` has more. They never share an id, and the decoder tells
/// them apart. Both route as storage to the default space.
#[test]
fn prop_space_and_collection_counters_distinct() {
    check(
        pure_config(),
        (arb_space(), arb_space(), arb_key_bytes(), arb_ctx()),
        |(first, second, key, ctx)| {
            let key = collection_key(&key);
            let space_counter = namespace::space_counter(first);
            let collection_counter = namespace::collection_counter(second, &key);
            prop_assert_ne!(&space_counter, &collection_counter);
            prop_assert_eq!(space_counter.len(), SPACE_ID_PREFIX_LEN);
            prop_assert!(collection_counter.len() > SPACE_ID_PREFIX_LEN);
            prop_assert_eq!(
                decode_reified_id(&space_counter),
                Some(ReifiedId::SpaceCounter(first))
            );
            prop_assert_eq!(
                decode_reified_id(&collection_counter),
                Some(ReifiedId::CollectionCounter(second, key.as_bytes()))
            );
            for id in [space_counter, collection_counter] {
                let channel = gprivate(id);
                prop_assert_eq!(classify_channel(&channel), ChannelClass::Storage);
                prop_assert_eq!(route(&ctx, &channel), Ok(Route::Default));
            }
            Ok(())
        },
    );
}

/// `name_id_routes_to_sid`: a name id decodes to its space and tail and has 66 or 42 bytes.
/// It classifies as a space name and routes to its space under every context. `gensym`
/// makes such a name from the `GENSYM` draw of the call.
#[test]
fn prop_name_id_routes_to_sid() {
    check(
        pure_config(),
        (arb_space(), arb_tail(), arb_ctx(), vec(any::<u8>(), 0..16)),
        |(space, tail, ctx, seed)| {
            let id = namespace::name_id(space, tail);
            prop_assert_eq!(decode_reified_id(&id), Some(ReifiedId::Name(space, tail)));
            let expected_len = match tail {
                NameTail::Random(_) => 66,
                NameTail::Index(_) => 42,
            };
            prop_assert_eq!(id.len(), expected_len);
            let name = gprivate(id);
            prop_assert_eq!(classify_channel(&name), ChannelClass::SpaceName(space));
            let key = CollectionKey::of_channel(&name).expect("a name has a key");
            prop_assert_eq!(
                route(&ctx, &name),
                Ok(Route::Space {
                    space,
                    key: key.clone()
                })
            );
            prop_assert_eq!(route(&None, &name), Ok(Route::Space { space, key }));
            let random = Blake2b512Random::create_from_bytes(&seed);
            let made = HashMapOuter
                .gensym(space, &random)
                .expect("gensym makes a name");
            let mut draw = RandomPurpose::Gensym.derive(&random);
            let mut expected_tail = [0u8; 32];
            for (byte, value) in expected_tail.iter_mut().zip(draw.next()) {
                *byte = value as u8;
            }
            prop_assert_eq!(
                made,
                gprivate(namespace::name_id(space, NameTail::Random(expected_tail)))
            );
            Ok(())
        },
    );
}

/// `canon_respects_par_eq`: the key of a channel is a function of its sorted shape. Equal
/// `Par` values share one key whatever their `locally_free` bits. Two channels share a key
/// exactly when they sort to one value. No key is empty, so a data channel is longer than a
/// space id.
#[test]
fn prop_canonical_inner_bytes_respects_par_eq() {
    check(
        pure_config(),
        (
            arb_par(),
            arb_par(),
            any::<bool>(),
            vec(any::<u8>(), 0..3),
            arb_space(),
        ),
        |(first, other, same_shape, bits, space)| {
            let canon = |par: &Par| canonical_inner_channel_bytes(par).expect("a Par serializes");
            let second = match same_shape {
                true => reversed_components(&first),
                false => other,
            };
            let mut with_bits = first.clone();
            with_bits.locally_free = bits;
            prop_assert_eq!(&with_bits, &first);
            prop_assert_eq!(canon(&with_bits), canon(&first));
            prop_assert_eq!(
                canon(&concatenate_pars(first.clone(), second.clone())),
                canon(&concatenate_pars(second.clone(), first.clone()))
            );
            let sorted_equal = sorted(&first) == sorted(&second);
            prop_assert_eq!(canon(&first) == canon(&second), sorted_equal);
            prop_assert!(!canon(&first).is_empty());
            let first_key = CollectionKey::of_channel(&first).expect("a key");
            let second_key = CollectionKey::of_channel(&second).expect("a key");
            prop_assert_eq!(
                namespace::data_channel(space, &first_key)
                    == namespace::data_channel(space, &second_key),
                sorted_equal
            );
            prop_assert!(namespace::data_channel(space, &first_key).len() > SPACE_ID_PREFIX_LEN);
            Ok(())
        },
    );
}

// ─── Envelope (RecordEnvelope.v) ─────────────────────────────────────────────────────────────

fn arb_ord() -> BoxedStrategy<i64> { prop_oneof![0i64..64, Just(i64::MAX), 0..=i64::MAX].boxed() }

fn arb_payload() -> BoxedStrategy<Vec<Par>> {
    prop_oneof![
        Just(Vec::new()),
        (0i64..8).prop_map(|value| vec![gint(value)]),
        vec(arb_par(), 0..4),
    ]
    .boxed()
}

fn arb_datum() -> BoxedStrategy<ListParWithRandom> {
    (arb_payload(), arb_random(), arb_authority(), any::<bool>())
        .prop_map(
            |(pars, random, cost_authority, stacked)| ListParWithRandom {
                pars,
                random_state: random.to_bytes(),
                cost_authority,
                cost_stack: stacked.then(Default::default),
            },
        )
        .boxed()
}

fn parked(
    body: Option<Par>,
    random: &Blake2b512Random,
    guard: Option<Par>,
    authority: Option<CostAuthority>,
) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body,
            random_state: random.to_bytes(),
        })),
        guard,
        cost_authority: authority,
    }
}

/// A continuation of any form: a body, a `ParBody` without a body, a native body ref or none.
fn arb_continuation() -> BoxedStrategy<TaggedContinuation> {
    let guard = prop_oneof![
        Just(None),
        any::<bool>().prop_map(|value| Some(gbool(value)))
    ];
    prop_oneof![
        6 => (arb_par(), arb_random(), guard, arb_authority()).prop_map(
            |(body, random, guard, authority)| parked(Some(body), &random, guard, authority)
        ),
        1 => arb_random().prop_map(|random| parked(None, &random, None, None)),
        1 => (200i64..260).prop_map(|body_ref| TaggedContinuation {
            tagged_cont: Some(TaggedCont::ScalaBodyRef(body_ref)),
            guard: None,
            cost_authority: None,
        }),
        1 => Just(TaggedContinuation::default()),
    ]
    .boxed()
}

/// The edits of an envelope list. An edit adds a remainder, flips the list flag or the `Par`
/// flag, puts a send next to the list, or sets other `locally_free` bits.
#[derive(Clone, Copy, Debug)]
enum EnvelopeEdit {
    None,
    Remainder,
    ListFlag,
    ParFlag,
    ExtraSend,
    Bits,
}

fn edited_list(ps: Vec<Par>, edit: EnvelopeEdit) -> Par {
    let mut envelope = list(ps);
    match edit {
        EnvelopeEdit::None => {}
        EnvelopeEdit::Remainder => {
            if let Some(Expr {
                expr_instance: Some(ExprInstance::EListBody(elist)),
            }) = envelope.exprs.first_mut()
            {
                elist.remainder = Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                });
            }
        }
        EnvelopeEdit::ListFlag => {
            if let Some(Expr {
                expr_instance: Some(ExprInstance::EListBody(elist)),
            }) = envelope.exprs.first_mut()
            {
                elist.connective_used = !elist.connective_used;
            }
        }
        EnvelopeEdit::ParFlag => envelope.connective_used = !envelope.connective_used,
        EnvelopeEdit::ExtraSend => envelope.sends.push(Default::default()),
        EnvelopeEdit::Bits => envelope.locally_free = vec![0b1010],
    }
    envelope
}

/// The edits that keep a record valid. A strict decoder ignores only the `locally_free` bits.
const VALID_EDITS: u8 = 5;

/// A tag of `kind` with one byte changed: the version, the kind or the mark.
fn edited_tag(kind: RecordKind, edit: u8) -> Par {
    let tag = kind.tag_bytes();
    match edit {
        0 => bytes_par(&[tag[0], tag[1], tag[2], RECORD_VERSION + 1]),
        1 => bytes_par(&[tag[0], tag[1], tag[2] ^ 0x01, tag[3]]),
        _ => bytes_par(&[tag[0], tag[1] ^ 0x01, tag[2], tag[3]]),
    }
}

/// The datum record of `datum` with the order `ord` and the edit `edit`. Edits below
/// `VALID_EDITS` keep the record valid. Each other edit breaks one check of `unwrap_datum`.
fn datum_candidate(
    ord: i64,
    datum: &ListParWithRandom,
    edit: u8,
    filler: Par,
) -> ListParWithRandom {
    let mut slots = vec![
        RecordKind::Datum.tag(),
        gint(ord),
        Par::default(),
        Par::default(),
    ];
    let mut list_edit = EnvelopeEdit::None;
    let mut first = None;
    let mut keep_first = true;
    match edit {
        0..=3 => {}
        4 => list_edit = EnvelopeEdit::Bits,
        5..=7 => slots[0] = edited_tag(RecordKind::Datum, edit - 5),
        8 => slots[1] = gint(-1 - ord % 7),
        9 => slots[1] = gstring("1"),
        10 => {
            slots[1] = Par {
                connective_used: true,
                ..gint(ord)
            }
        }
        11 => slots[2] = gint(0),
        12 => slots[3] = gint(0),
        13 => {
            slots.pop();
        }
        14 => slots.push(Par::default()),
        15 => list_edit = EnvelopeEdit::Remainder,
        16 => list_edit = EnvelopeEdit::ListFlag,
        17 => list_edit = EnvelopeEdit::ParFlag,
        18 => list_edit = EnvelopeEdit::ExtraSend,
        19 => first = Some(continuation_envelope(ord, filler)),
        20 => first = Some(gint(ord)),
        _ => keep_first = false,
    }
    let envelope = first.unwrap_or_else(|| edited_list(slots, list_edit));
    let mut pars = Vec::with_capacity(1 + datum.pars.len());
    if keep_first {
        pars.push(envelope);
    }
    pars.extend(datum.pars.iter().cloned());
    ListParWithRandom {
        pars,
        ..datum.clone()
    }
}

const DATUM_EDITS: u8 = 22;

/// The parked continuation of `continuation` with the order `ord` and the edit `edit`.
/// Edits below `VALID_EDITS` keep the record valid.
fn continuation_candidate(
    ord: i64,
    continuation: &TaggedContinuation,
    edit: u8,
) -> TaggedContinuation {
    let Some(TaggedCont::ParBody(ParWithRandom {
        body: Some(original),
        random_state,
    })) = &continuation.tagged_cont
    else {
        return continuation.clone();
    };
    let mut slots = vec![
        RecordKind::Continuation.tag(),
        gint(ord),
        Par::default(),
        original.clone(),
    ];
    let mut list_edit = EnvelopeEdit::None;
    let mut body = None;
    match edit {
        0..=3 => {}
        4 => list_edit = EnvelopeEdit::Bits,
        5..=7 => slots[0] = edited_tag(RecordKind::Continuation, edit - 5),
        8 => slots[1] = gint(-1 - ord % 7),
        9 => slots[1] = gstring("1"),
        10 => {
            slots[1] = Par {
                connective_used: true,
                ..gint(ord)
            }
        }
        11 => slots[2] = gint(0),
        12 => {
            slots.pop();
        }
        13 => slots.push(Par::default()),
        14 => list_edit = EnvelopeEdit::Remainder,
        15 => list_edit = EnvelopeEdit::ListFlag,
        16 => list_edit = EnvelopeEdit::ParFlag,
        17 => list_edit = EnvelopeEdit::ExtraSend,
        18 => body = Some(Some(datum_envelope(ord))),
        19 => body = Some(None),
        20 => {
            return TaggedContinuation {
                tagged_cont: Some(TaggedCont::ScalaBodyRef(201)),
                ..continuation.clone()
            }
        }
        _ => {
            return TaggedContinuation {
                tagged_cont: None,
                ..continuation.clone()
            }
        }
    }
    let body = body.unwrap_or_else(|| Some(edited_list(slots, list_edit)));
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body,
            random_state: random_state.clone(),
        })),
        ..continuation.clone()
    }
}

const CONTINUATION_EDITS: u8 = 22;

/// The counter record with the value `value` and the edit `edit`. Edits below `VALID_EDITS`
/// keep the record valid.
fn counter_candidate(
    value: i64,
    random: &Blake2b512Random,
    authority: Option<CostAuthority>,
    edit: u8,
) -> ListParWithRandom {
    let mut envelope = vec![RecordKind::Counter.tag()];
    let mut list_edit = EnvelopeEdit::None;
    let mut value_par = gint(value);
    let mut extra = false;
    let mut missing = false;
    let mut cost_stack = None;
    match edit {
        0..=3 => {}
        4 => list_edit = EnvelopeEdit::Bits,
        5..=7 => envelope[0] = edited_tag(RecordKind::Counter, edit - 5),
        8 => value_par = gint(-1 - value % 7),
        9 => value_par = gstring("3"),
        10 => {
            value_par = Par {
                connective_used: true,
                ..gint(value)
            }
        }
        11 => envelope.push(Par::default()),
        12 => extra = true,
        13 => missing = true,
        14 => cost_stack = Some(Default::default()),
        15 => list_edit = EnvelopeEdit::Remainder,
        16 => list_edit = EnvelopeEdit::ListFlag,
        17 => list_edit = EnvelopeEdit::ParFlag,
        _ => list_edit = EnvelopeEdit::ExtraSend,
    }
    let mut pars = vec![edited_list(envelope, list_edit)];
    if !missing {
        pars.push(value_par);
    }
    if extra {
        pars.push(Par::default());
    }
    ListParWithRandom {
        pars,
        random_state: random.to_bytes(),
        cost_authority: authority,
        cost_stack,
    }
}

const COUNTER_EDITS: u8 = 19;

/// The config record of `map` with the edit `edit`. Edits below `VALID_EDITS` keep the record
/// valid.
fn config_record_candidate(map: Par, random: &Blake2b512Random, edit: u8) -> ListParWithRandom {
    let mut envelope = vec![RecordKind::Config.tag()];
    let mut list_edit = EnvelopeEdit::None;
    let mut extra = false;
    let mut missing = false;
    let mut cost_authority = None;
    let mut cost_stack = None;
    match edit {
        0..=3 => {}
        4 => list_edit = EnvelopeEdit::Bits,
        5..=7 => envelope[0] = edited_tag(RecordKind::Config, edit - 5),
        8 => envelope.push(Par::default()),
        9 => extra = true,
        10 => missing = true,
        11 => cost_authority = Some(region(1)),
        12 => cost_stack = Some(Default::default()),
        13 => list_edit = EnvelopeEdit::Remainder,
        14 => list_edit = EnvelopeEdit::ListFlag,
        15 => list_edit = EnvelopeEdit::ParFlag,
        _ => list_edit = EnvelopeEdit::ExtraSend,
    }
    let mut pars = vec![edited_list(envelope, list_edit)];
    if !missing {
        pars.push(map);
    }
    if extra {
        pars.push(Par::default());
    }
    ListParWithRandom {
        pars,
        random_state: random.to_bytes(),
        cost_authority,
        cost_stack,
    }
}

const CONFIG_RECORD_EDITS: u8 = 17;

fn arb_config_qualifier() -> BoxedStrategy<ConfigQualifier> {
    select(vec![
        ConfigQualifier::Default,
        ConfigQualifier::Temp,
        ConfigQualifier::Seq,
    ])
    .boxed()
}

fn arb_space_config() -> BoxedStrategy<SpaceConfig> {
    let name = prop_oneof![
        select(vec!["hashmap", "pathmap", "queue", "bag", "stack"]).prop_map(String::from),
        "[a-z]{0,6}",
    ];
    // Changed by P7 step 4 (design v3 TH-2): the config holds the identity of its theory or
    // `None`, so the codec property covers both forms of the theory slot.
    let theory = prop_oneof![
        Just(None),
        select(BUILTIN_NAMES.to_vec()).prop_map(|name| Some(TheoryIdentity::builtin(name))),
    ];
    (
        uniform32(any::<u8>()),
        name.clone(),
        name,
        arb_config_qualifier(),
        proptest::collection::btree_map("[a-z]{1,4}", arb_ground_par(), 0..3),
        theory,
    )
        .prop_map(
            |(space_id, outer, inner, qualifier, params, theory)| SpaceConfig {
                space_id,
                outer,
                inner,
                qualifier,
                params,
                theory,
            },
        )
        .boxed()
}

/// The entries of the config map of I-17 as (key, value) pairs.
fn config_entries(config: &SpaceConfig) -> Vec<(String, Par)> {
    vec![
        ("spaceId".to_string(), bytes_par(&config.space_id)),
        ("outer".to_string(), gstring(&config.outer)),
        ("inner".to_string(), gstring(&config.inner)),
        ("qualifier".to_string(), gstring(config.qualifier.as_str())),
        (
            "params".to_string(),
            reified_strategies::map(config.params.clone().into_iter().collect()),
        ),
        // Changed by P7 step 4 (design v3 TH-2, decision 74): the slot holds `Nil` or the
        // canonical bytes of the identity.
        // ("theory".to_string(), Par::default()),
        (
            "theory".to_string(),
            config
                .theory
                .as_ref()
                .map_or_else(Par::default, |identity| bytes_par(&identity.to_bytes())),
        ),
    ]
}

/// The config map of `config` with the edit `edit`. Edits below 4 keep the map valid. Each
/// other edit breaks one check of `SpaceConfig::from_par`. It removes or adds an entry,
/// shortens the space id, names an unknown qualifier or sets a theory. Other edits put a
/// number where a string belongs, flip the flag of the map, or replace the params map.
fn config_map_candidate(config: &SpaceConfig, edit: u8, index: usize, value: Par) -> Par {
    let mut entries = config_entries(config);
    match edit {
        0..=3 => {}
        4 => {
            entries.remove(index % entries.len());
        }
        5 => entries.push(("extra".to_string(), value)),
        6 => entries[0].1 = bytes_par(&config.space_id[1..]),
        7 => entries[3].1 = gstring("sometimes"),
        8 => entries[5].1 = gstring("Nat"),
        9 => entries[1].1 = gint(1),
        10 => {}
        _ => entries[4].1 = gstring("params"),
    }
    let mut map = reified_strategies::map(entries);
    if edit == 10 {
        map.connective_used = !map.connective_used;
    }
    map
}

const CONFIG_MAP_VALID_EDITS: u8 = 4;
const CONFIG_MAP_EDITS: u8 = 12;

/// A datum record as the store holds it, and the merger number predicate on its bytes.
fn merger_reads_a_number(record: &ListParWithRandom) -> bool {
    let stored = Datum {
        a: record.clone(),
        persist: false,
        source: Produce::create(&gstring("rrs-number-channel"), record, false),
    };
    let bytes = bincode::serialize(&stored).expect("a datum serializes");
    RholangMergingLogic::encoded_datum_is_number(&bytes)
}

fn first_is_list(record: &ListParWithRandom) -> bool {
    match record.pars.first() {
        Some(first) => matches!(first.exprs.as_slice(), [Expr {
            expr_instance: Some(ExprInstance::EListBody(EList { .. }))
        }]),
        None => false,
    }
}

/// `datum_unwrap_wrap`: `unwrap_datum` inverts `wrap_datum`. The order comes back, and the
/// payload, the random state, the cost authority and the cost stack stay.
#[test]
fn prop_datum_envelope_roundtrip() {
    check(pure_config(), (arb_ord(), arb_datum()), |(ord, datum)| {
        let record = wrap_datum(ord, &datum);
        prop_assert_eq!(record.pars.len(), datum.pars.len() + 1);
        prop_assert_eq!(&record.pars[0], &datum_envelope(ord));
        prop_assert_eq!(&record.pars[1..], datum.pars.as_slice());
        prop_assert_eq!(&record.random_state, &datum.random_state);
        prop_assert_eq!(&record.cost_authority, &datum.cost_authority);
        prop_assert_eq!(&record.cost_stack, &datum.cost_stack);
        prop_assert_eq!(datum_ord(&record), Ok(ord));
        prop_assert_eq!(unwrap_datum(&record), Ok(DatumRecord { ord, datum }));
        Ok(())
    });
}

/// `datum_wrap_injective`: two datum records are equal exactly when their orders and their
/// datums are equal.
#[test]
fn prop_datum_envelope_injective() {
    check(
        pure_config(),
        (
            vec(arb_datum(), 3),
            (0usize..3, 0i64..3),
            (0usize..3, 0i64..3),
        ),
        |(datums, first, second)| {
            let (first_datum, first_ord) = (&datums[first.0], first.1);
            let (second_datum, second_ord) = (&datums[second.0], second.1);
            prop_assert_eq!(
                wrap_datum(first_ord, first_datum) == wrap_datum(second_ord, second_datum),
                first_ord == second_ord && first_datum == second_datum
            );
            Ok(())
        },
    );
}

/// `datum_unwrap_sound`: `unwrap_datum` accepts exactly the image of `wrap_datum` with a
/// natural order. Every record that it accepts is the wrapping of its result. Each single
/// edit of a check (tag, order, slots, arity, list form, flags) is rejected.
#[test]
fn prop_datum_envelope_decoder_sound() {
    check(
        pure_config(),
        (arb_ord(), arb_datum(), 0..DATUM_EDITS, arb_par()),
        |(ord, datum, edit, filler)| {
            let candidate = datum_candidate(ord, &datum, edit, filler);
            match unwrap_datum(&candidate) {
                Ok(DatumRecord {
                    ord: read,
                    datum: payload,
                }) => {
                    prop_assert!(edit < VALID_EDITS, "edit {} was accepted", edit);
                    prop_assert!(read >= 0);
                    prop_assert_eq!(read, ord);
                    prop_assert_eq!(datum_ord(&candidate), Ok(read));
                    prop_assert_eq!(wrap_datum(read, &payload), candidate);
                }
                Err(error) => {
                    prop_assert!(edit >= VALID_EDITS, "valid edit {} was rejected", edit);
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Datum));
                    prop_assert_eq!(
                        datum_ord(&candidate),
                        Err(MalformedRecord(RecordKind::Datum))
                    );
                }
            }
            Ok(())
        },
    );
}

/// `continuation_unwrap_wrap`: a `ParBody` with a body wraps, `unwrap_continuation` inverts
/// `wrap_continuation` and accepts only its image, and every other continuation has no
/// continuation record.
#[test]
fn prop_continuation_envelope_roundtrip() {
    check(
        pure_config(),
        (arb_ord(), arb_continuation(), 0..CONTINUATION_EDITS),
        |(ord, continuation, edit)| {
            let has_body = matches!(
                &continuation.tagged_cont,
                Some(TaggedCont::ParBody(ParWithRandom { body: Some(_), .. }))
            );
            match wrap_continuation(ord, &continuation) {
                Ok(wrapped) => {
                    prop_assert!(has_body);
                    prop_assert_eq!(continuation_ord(&wrapped), Ok(ord));
                    prop_assert_eq!(
                        unwrap_continuation(&wrapped),
                        Ok(ContinuationRecord {
                            ord,
                            continuation: continuation.clone()
                        })
                    );
                }
                Err(error) => {
                    prop_assert!(!has_body);
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Continuation));
                }
            }
            let candidate = continuation_candidate(ord, &continuation, edit);
            match unwrap_continuation(&candidate) {
                Ok(ContinuationRecord {
                    ord: read,
                    continuation: original,
                }) => {
                    prop_assert!(has_body && edit < VALID_EDITS, "edit {} was accepted", edit);
                    prop_assert!(read >= 0);
                    prop_assert_eq!(read, ord);
                    prop_assert_eq!(&original, &continuation);
                    prop_assert_eq!(continuation_ord(&candidate), Ok(read));
                    prop_assert_eq!(wrap_continuation(read, &original), Ok(candidate));
                }
                Err(error) => {
                    prop_assert!(
                        !has_body || edit >= VALID_EDITS,
                        "valid edit {} was rejected",
                        edit
                    );
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Continuation));
                }
            }
            Ok(())
        },
    );
}

/// `counter_and_config_roundtrip`: the counter record and the config record are exact
/// codecs. Each decoder inverts its encoder and accepts only its image. So does the config
/// map of I-17 (`SpaceConfig::from_par` and `to_par`).
#[test]
fn prop_counter_and_config_records_roundtrip() {
    check(
        pure_config(),
        (
            arb_ord(),
            arb_random(),
            arb_authority(),
            arb_space_config(),
            0..COUNTER_EDITS,
            0..CONFIG_RECORD_EDITS,
            (0..CONFIG_MAP_EDITS, 0usize..6, arb_ground_par()),
        ),
        |(value, random, authority, config, counter_edit, record_edit, map_edit)| {
            let counter = counter_record(value, &random, authority.clone());
            prop_assert_eq!(counter_value(&counter), Ok(value));
            prop_assert_eq!(&counter.cost_authority, &authority);
            let candidate = counter_candidate(value, &random, authority.clone(), counter_edit);
            match counter_value(&candidate) {
                Ok(read) => {
                    prop_assert!(
                        counter_edit < VALID_EDITS,
                        "counter edit {} was accepted",
                        counter_edit
                    );
                    prop_assert_eq!(read, value);
                    let random = Blake2b512Random::from_bytes(&candidate.random_state);
                    prop_assert_eq!(
                        counter_record(read, &random, candidate.cost_authority.clone()),
                        candidate
                    );
                }
                Err(error) => {
                    prop_assert!(
                        counter_edit >= VALID_EDITS,
                        "valid counter edit {} was rejected",
                        counter_edit
                    );
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Counter));
                }
            }
            let map = config.to_par();
            let record = config_record(map.clone(), &random);
            prop_assert_eq!(config_map(&record), Ok(&map));
            prop_assert_eq!(&record.cost_authority, &None);
            let candidate = config_record_candidate(map.clone(), &random, record_edit);
            match config_map(&candidate) {
                Ok(read) => {
                    prop_assert!(
                        record_edit < VALID_EDITS,
                        "config edit {} was accepted",
                        record_edit
                    );
                    let random = Blake2b512Random::from_bytes(&candidate.random_state);
                    prop_assert_eq!(config_record(read.clone(), &random), candidate.clone());
                }
                Err(error) => {
                    prop_assert!(
                        record_edit >= VALID_EDITS,
                        "valid config edit {} was rejected",
                        record_edit
                    );
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Config));
                }
            }
            prop_assert_eq!(SpaceConfig::from_par(&map), Ok(config.clone()));
            prop_assert_eq!(&map, &reified_strategies::map(config_entries(&config)));
            let (edit, index, value) = map_edit;
            let candidate = config_map_candidate(&config, edit, index, value);
            match SpaceConfig::from_par(&candidate) {
                Ok(read) => {
                    prop_assert!(
                        edit < CONFIG_MAP_VALID_EDITS,
                        "map edit {} was accepted",
                        edit
                    );
                    prop_assert_eq!(&read, &config);
                    prop_assert_eq!(read.to_par(), candidate);
                }
                Err(error) => {
                    prop_assert!(
                        edit >= CONFIG_MAP_VALID_EDITS,
                        "valid map edit {} was rejected",
                        edit
                    );
                    prop_assert_eq!(error, MalformedRecord(RecordKind::Config));
                }
            }
            Ok(())
        },
    );
}

/// `envelope_kinds_disjoint`: the four tags differ, and no decoder accepts a record that
/// the encoder of another kind made.
#[test]
fn prop_envelope_kinds_disjoint() {
    check(
        pure_config(),
        (
            arb_ord(),
            arb_ord(),
            arb_datum(),
            arb_random(),
            arb_ground_par(),
            arb_par(),
        ),
        |(ord, value, datum, random, map, body)| {
            let tags: BTreeSet<[u8; 4]> = RecordKind::ALL
                .iter()
                .map(|kind| kind.tag_bytes())
                .collect();
            prop_assert_eq!(tags.len(), RecordKind::ALL.len());
            for kind in RecordKind::ALL {
                prop_assert_eq!(kind.tag_bytes(), [
                    REIFIED_TAG,
                    RECORD_TAG_MARK,
                    kind as u8,
                    RECORD_VERSION
                ]);
            }
            let datum_record = wrap_datum(ord, &datum);
            let counter = counter_record(value, &random, None);
            let config = config_record(map, &random);
            prop_assert_eq!(datum_ord(&counter), Err(MalformedRecord(RecordKind::Datum)));
            prop_assert_eq!(datum_ord(&config), Err(MalformedRecord(RecordKind::Datum)));
            prop_assert_eq!(
                counter_value(&datum_record),
                Err(MalformedRecord(RecordKind::Counter))
            );
            prop_assert_eq!(
                counter_value(&config),
                Err(MalformedRecord(RecordKind::Counter))
            );
            prop_assert_eq!(
                config_map(&datum_record),
                Err(MalformedRecord(RecordKind::Config))
            );
            prop_assert_eq!(
                config_map(&counter),
                Err(MalformedRecord(RecordKind::Config))
            );
            let datum_as_continuation = parked(Some(datum_envelope(ord)), &random, None, None);
            prop_assert_eq!(
                continuation_ord(&datum_as_continuation),
                Err(MalformedRecord(RecordKind::Continuation))
            );
            let continuation_as_datum = ListParWithRandom {
                pars: vec![continuation_envelope(ord, body)],
                ..datum
            };
            prop_assert_eq!(
                datum_ord(&continuation_as_datum),
                Err(MalformedRecord(RecordKind::Datum))
            );
            Ok(())
        },
    );
}

/// `no_reified_datum_is_single_number`: the first par of every Reified record is a list. So
/// the merger never reads a datum, counter or config record as a number cell. This holds
/// also for a datum with an empty payload or a number payload (A11). A bare number is a
/// number cell, so the predicate is not vacuous.
#[test]
fn prop_no_reified_datum_is_single_number() {
    check(
        pure_config(),
        (
            arb_ord(),
            arb_datum(),
            arb_ord(),
            arb_random(),
            arb_ground_par(),
            any::<i64>(),
        ),
        |(ord, datum, value, random, map, number)| {
            let records = [
                wrap_datum(ord, &datum),
                counter_record(value, &random, None),
                config_record(map, &random),
            ];
            for record in &records {
                prop_assert!(first_is_list(record));
                prop_assert!(RholangMergingLogic::try_get_number_with_rnd(record).is_none());
                prop_assert!(!merger_reads_a_number(record));
            }
            let bare = ListParWithRandom {
                pars: vec![gint(number)],
                ..ListParWithRandom::default()
            };
            prop_assert!(RholangMergingLogic::try_get_number_with_rnd(&bare).is_some());
            prop_assert!(merger_reads_a_number(&bare));
            Ok(())
        },
    );
}

/// `unknown_version_rejected`: a tag with another version is the tag of no kind, and every
/// decoder rejects a record that carries it. A tag of version 1 with another kind byte is
/// rejected by the decoder of each other kind.
#[test]
fn prop_envelope_unknown_version_rejected() {
    check(
        pure_config(),
        (
            any::<u8>(),
            any::<u8>().prop_filter("another version", |version| *version != RECORD_VERSION),
            arb_ord(),
            arb_par(),
            arb_ground_par(),
            arb_random(),
        ),
        |(kind_byte, version, ord, body, map, random)| {
            let tag = [REIFIED_TAG, RECORD_TAG_MARK, kind_byte, version];
            for kind in RecordKind::ALL {
                prop_assert_ne!(tag, kind.tag_bytes());
            }
            for (tag, accepted) in [
                (tag, None),
                (
                    [REIFIED_TAG, RECORD_TAG_MARK, kind_byte, RECORD_VERSION],
                    Some(kind_byte),
                ),
            ] {
                let tag = bytes_par(&tag);
                let datum = ListParWithRandom {
                    pars: vec![
                        list(vec![tag.clone(), gint(ord), Par::default(), Par::default()]),
                        gint(1),
                    ],
                    ..ListParWithRandom::default()
                };
                prop_assert_eq!(
                    datum_ord(&datum).is_ok(),
                    accepted == Some(RecordKind::Datum as u8)
                );
                let continuation = parked(
                    Some(list(vec![
                        tag.clone(),
                        gint(ord),
                        Par::default(),
                        body.clone(),
                    ])),
                    &random,
                    None,
                    None,
                );
                prop_assert_eq!(
                    continuation_ord(&continuation).is_ok(),
                    accepted == Some(RecordKind::Continuation as u8)
                );
                let counter = ListParWithRandom {
                    pars: vec![list(vec![tag.clone()]), gint(ord)],
                    random_state: random.to_bytes(),
                    ..ListParWithRandom::default()
                };
                prop_assert_eq!(
                    counter_value(&counter).is_ok(),
                    accepted == Some(RecordKind::Counter as u8)
                );
                let config = ListParWithRandom {
                    pars: vec![list(vec![tag]), map.clone()],
                    random_state: random.to_bytes(),
                    ..ListParWithRandom::default()
                };
                prop_assert_eq!(
                    config_map(&config).is_ok(),
                    accepted == Some(RecordKind::Config as u8)
                );
            }
            Ok(())
        },
    );
}

// ─── Counter: check #1 (CounterConflict.v §2 and §3) ─────────────────────────────────────────

/// The channels of the synthetic event logs.
const CHECK1_CHANNELS: usize = 4;

fn check1_channel(index: usize) -> Blake2b256Hash {
    Blake2b256Hash::new(format!("rrs-p4-check1-channel-{index}").as_bytes())
}

/// A produce of a synthetic log. Its hash is a function of its channel, its identity and its
/// persistence, as `Produce::create` hashes them. So two produces are equal exactly when the
/// three agree.
fn check1_produce((channel, ident, persistent): (usize, u8, bool)) -> Produce {
    Produce::new(
        check1_channel(channel),
        Blake2b256Hash::new(
            format!("rrs-p4-check1-produce-{channel}-{ident}-{persistent}").as_bytes(),
        ),
        persistent,
    )
}

fn check1_consume((channels, ident, persistent): (Vec<usize>, u8, bool)) -> Consume {
    Consume {
        channel_hashes: channels
            .iter()
            .map(|channel| check1_channel(*channel))
            .collect(),
        hash: Blake2b256Hash::new(
            format!("rrs-p4-check1-consume-{channels:?}-{ident}-{persistent}").as_bytes(),
        ),
        persistent,
    }
}

fn arb_check1_produce() -> BoxedStrategy<(usize, u8, bool)> {
    (0..CHECK1_CHANNELS, 0u8..3, any::<bool>()).boxed()
}

fn arb_check1_consume() -> BoxedStrategy<(Vec<usize>, u8, bool)> {
    (vec(0..CHECK1_CHANNELS, 1..3), 0u8..3, any::<bool>()).boxed()
}

/// Two event logs over four channels with few identities. Each log takes some events of a
/// shared pool and adds its own, so the logs often share a COMM, a produce or a consume.
fn arb_check1_logs() -> BoxedStrategy<(Vec<Event>, Vec<Event>)> {
    (
        vec(arb_check1_event(), 1..6),
        vec(arb_check1_event(), 0..4),
        vec(arb_check1_event(), 0..4),
    )
        .prop_flat_map(|(pool, own_first, own_second)| {
            let size = pool.len();
            (
                Just((pool, own_first, own_second)),
                vec(any::<bool>(), size),
                vec(any::<bool>(), size),
            )
        })
        .prop_map(|((pool, own_first, own_second), take_first, take_second)| {
            let pick = |take: &[bool], own: Vec<Event>| -> Vec<Event> {
                pool.iter()
                    .zip(take)
                    .filter(|(_, taken)| **taken)
                    .map(|(event, _)| event.clone())
                    .chain(own)
                    .collect()
            };
            (pick(&take_first, own_first), pick(&take_second, own_second))
        })
        .boxed()
}

/// One event over four channels with few identities.
fn arb_check1_event() -> BoxedStrategy<Event> {
    prop_oneof![
        arb_check1_produce()
            .prop_map(|produce| Event::IoEvent(IOEvent::Produce(check1_produce(produce)))),
        arb_check1_consume()
            .prop_map(|consume| Event::IoEvent(IOEvent::Consume(check1_consume(consume)))),
        (
            arb_check1_consume(),
            vec(arb_check1_produce(), 1..3),
            any::<bool>()
        )
            .prop_map(|(consume, produces, peek)| {
                Event::Comm(COMM {
                    consume: check1_consume(consume),
                    produces: produces.into_iter().map(check1_produce).collect(),
                    peeks: match peek {
                        true => BTreeSet::from([0]),
                        false => BTreeSet::new(),
                    },
                    times_repeated: BTreeMap::new(),
                })
            }),
    ]
    .boxed()
}

/// The sets of an event log as `CounterConflict.v` defines them (§2), computed from the raw
/// events. Membership compares produces by hash, as the merger does.
struct RestatedIndex {
    linear: Vec<Produce>,
    persistent: Vec<Produce>,
    consumed: Vec<Produce>,
    peeked: Vec<Produce>,
    copied_by_peek: Vec<Produce>,
    touching_joins: Vec<Produce>,
    consumes_linear: Vec<Consume>,
    consumes_persistent: Vec<Consume>,
    consumes_produced: Vec<Consume>,
    mergeable: BTreeSet<Blake2b256Hash>,
}

impl RestatedIndex {
    fn new(
        log: &[Event],
        in_pre_state: &dyn Fn(&Produce) -> bool,
        touches_join: &dyn Fn(&Produce) -> bool,
        mergeable: &BTreeSet<Blake2b256Hash>,
    ) -> Self {
        let mut index = RestatedIndex {
            linear: Vec::new(),
            persistent: Vec::new(),
            consumed: Vec::new(),
            peeked: Vec::new(),
            copied_by_peek: Vec::new(),
            touching_joins: Vec::new(),
            consumes_linear: Vec::new(),
            consumes_persistent: Vec::new(),
            consumes_produced: Vec::new(),
            mergeable: mergeable.clone(),
        };
        for event in log {
            match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    match produce.persistent {
                        true => index.persistent.push(produce.clone()),
                        false => index.linear.push(produce.clone()),
                    }
                    if in_pre_state(produce) {
                        index.copied_by_peek.push(produce.clone());
                    }
                    if touches_join(produce) {
                        index.touching_joins.push(produce.clone());
                    }
                }
                Event::IoEvent(IOEvent::Consume(consume)) => match consume.persistent {
                    true => index.consumes_persistent.push(consume.clone()),
                    false => index.consumes_linear.push(consume.clone()),
                },
                Event::Comm(comm) => {
                    index.consumes_produced.push(comm.consume.clone());
                    match comm.peeks.is_empty() {
                        true => index.consumed.extend(comm.produces.iter().cloned()),
                        false => index.peeked.extend(comm.produces.iter().cloned()),
                    }
                }
            }
        }
        index
    }

    fn produce_mergeable(&self, produce: &Produce) -> bool {
        self.mergeable.contains(&produce.channel_hash)
            && [&self.linear, &self.persistent, &self.consumed, &self.peeked]
                .iter()
                .any(|set| set.contains(produce))
    }

    fn consume_mergeable(&self, consume: &Consume) -> bool {
        consume
            .channel_hashes
            .iter()
            .any(|channel| self.mergeable.contains(channel))
            && [
                &self.consumes_linear,
                &self.consumes_persistent,
                &self.consumes_produced,
            ]
            .iter()
            .any(|set| set.contains(consume))
    }

    /// The produces that the log made and did not destroy. These are the linear produces that no
    /// COMM consumed and the persistent produces, minus the copies of the pre-state.
    fn produces_created_and_not_destroyed(&self) -> Vec<&Produce> {
        self.linear
            .iter()
            .filter(|produce| !self.consumed.contains(produce))
            .chain(self.persistent.iter())
            .filter(|produce| !self.copied_by_peek.contains(produce))
            .collect()
    }

    fn consumes_created_and_not_destroyed(&self) -> Vec<&Consume> {
        self.consumes_linear
            .iter()
            .filter(|consume| !self.consumes_produced.contains(consume))
            .chain(self.consumes_persistent.iter())
            .collect()
    }
}

/// Check #1 (`check1`). It finds the shared non-persistent produces that both logs consume
/// without a peek, and the shared non-persistent consumes that both logs COMM. It removes the
/// events that are mergeable in both logs and returns the channels of the rest.
fn restated_check1(a: &RestatedIndex, b: &RestatedIndex) -> BTreeSet<Blake2b256Hash> {
    let mut channels = BTreeSet::new();
    for produce in &a.consumed {
        if b.consumed.contains(produce)
            && !(a.produce_mergeable(produce) && b.produce_mergeable(produce))
            && !produce.persistent
        {
            channels.insert(produce.channel_hash.clone());
        }
    }
    for consume in &a.consumes_produced {
        if b.consumes_produced.contains(consume)
            && !(a.consume_mergeable(consume) && b.consume_mergeable(consume))
            && !consume.persistent
        {
            channels.extend(consume.channel_hashes.iter().cloned());
        }
    }
    channels
}

/// Check #2. One log created a produce and did not destroy it. The other log created a consume
/// on its channel and did not destroy it.
fn restated_check2(a: &RestatedIndex, b: &RestatedIndex) -> BTreeSet<Blake2b256Hash> {
    let mut channels = BTreeSet::new();
    for (left, right) in [(a, b), (b, a)] {
        for produce in left.produces_created_and_not_destroyed() {
            for consume in right.consumes_created_and_not_destroyed() {
                if consume.channel_hashes.contains(&produce.channel_hash) {
                    channels.insert(produce.channel_hash.clone());
                }
            }
        }
    }
    channels
}

/// Check #3: a produce of either log on a channel with a join of the pre-state.
fn restated_check3(a: &RestatedIndex, b: &RestatedIndex) -> BTreeSet<Blake2b256Hash> {
    a.touching_joins
        .iter()
        .chain(b.touching_joins.iter())
        .map(|produce| produce.channel_hash.clone())
        .collect()
}

/// `check1_race_iff`: the restatement of check #1 over raw event logs (CounterConflict.v)
/// agrees with the merger of HEAD. `conflict_reason` reports a race for one IO event exactly
/// when the restated check #1 finds a channel. `conflicts` is the union of the restated
/// checks #1, #2 and #3, and check #1 is symmetric.
#[test]
fn prop_check1_restatement_matches_merger() {
    check(
        pure_config(),
        (
            arb_check1_logs(),
            vec(arb_check1_produce(), 0..4),
            vec(0..CHECK1_CHANNELS, 0..2),
            vec(0..CHECK1_CHANNELS, 0..3),
        ),
        |(logs, pre_state, join_channels, mergeable_channels)| {
            let (first, second) = logs;
            let pre_state: Vec<Produce> = pre_state.into_iter().map(check1_produce).collect();
            let joins: BTreeSet<Blake2b256Hash> =
                join_channels.into_iter().map(check1_channel).collect();
            let mergeable: BTreeSet<Blake2b256Hash> =
                mergeable_channels.into_iter().map(check1_channel).collect();
            let number_channels: NumberChannelsDiff = mergeable
                .iter()
                .map(|channel| (channel.clone(), (0, MergeType::IntegerAdd)))
                .collect();
            let in_pre_state = |produce: &Produce| pre_state.contains(produce);
            let touches_join = |produce: &Produce| joins.contains(&produce.channel_hash);
            let index = |log: &Vec<Event>| {
                EventLogIndex::new(
                    log.clone(),
                    in_pre_state,
                    touches_join,
                    number_channels.clone(),
                )
            };
            let (real_first, real_second) = (index(&first), index(&second));
            let restated_first =
                RestatedIndex::new(&first, &in_pre_state, &touches_join, &mergeable);
            let restated_second =
                RestatedIndex::new(&second, &in_pre_state, &touches_join, &mergeable);
            let check1 = restated_check1(&restated_first, &restated_second);
            prop_assert_eq!(&check1, &restated_check1(&restated_second, &restated_first));
            let reason = conflict_reason(&real_first, &real_second);
            prop_assert_eq!(
                reason
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
                !check1.is_empty(),
                "conflict_reason {:?} and the restated check #1 {:?} disagree",
                reason,
                check1
            );
            let real: BTreeSet<Blake2b256Hash> =
                conflicts(&real_first, &real_second).0.into_iter().collect();
            prop_assert!(check1.is_subset(&real));
            let mut expected = check1;
            expected.extend(restated_check2(&restated_first, &restated_second));
            expected.extend(restated_check3(&restated_first, &restated_second));
            prop_assert_eq!(&real, &expected);
            prop_assert_eq!(
                are_conflicting(&real_first, &real_second),
                !expected.is_empty()
            );
            Ok(())
        },
    );
}

// ─── Queue (HeadOnlyDisciplines.v, the queue part) ───────────────────────────────────────────

type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type QueueAgent = GenericSpace<HashMapOuter, QueueDiscipline>;

/// The space of the agent rig.
const RIG_SPACE: NativeSpace = NativeSpace {
    sid: [0x5A; SID_LEN],
    durability: Durability::Durable,
};

/// The phlo of the budget of the agent rig.
const RIG_PHLO: i64 = 1_000_000_000;

/// A pattern of a single-bind receive: a free variable, or one number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Any,
    Lit(i64),
}

impl Pat {
    fn fits(self, value: i64) -> bool {
        match self {
            Pat::Any => true,
            Pat::Lit(expected) => expected == value,
        }
    }

    fn bind_pattern(self) -> BindPattern {
        match self {
            Pat::Any => BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            Pat::Lit(value) => BindPattern {
                patterns: vec![gint(value)],
                remainder: None,
                free_count: 0,
            },
        }
    }

    /// The pattern of a stored bind: a free variable or one number. Other patterns are not
    /// generated by these tests.
    fn of_bind_pattern(pattern: &BindPattern) -> Pat {
        match pattern.patterns.as_slice() {
            [single] => match single.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::GInt(value)),
                }] => Pat::Lit(*value),
                [Expr {
                    expr_instance: Some(ExprInstance::EVarBody(_)),
                }] => Pat::Any,
                other => panic!("a stored pattern is not generated by these tests: {other:?}"),
            },
            other => panic!("a stored bind has {} patterns", other.len()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecordSpec {
    ord: i64,
    value: i64,
    persist: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WaiterSpec {
    ord: i64,
    pat: Pat,
    persist: bool,
    peek: bool,
    guard: Option<bool>,
}

impl WaiterSpec {
    /// The match of the waiter against a record of `value`: its pattern and its guard.
    fn fits(&self, value: i64) -> bool { self.pat.fits(value) && self.guard != Some(false) }
}

/// A well-formed queue collection: distinct orders on both sides, all below the counter. A
/// collection without a counter (`None`) is fresh and holds nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueueState {
    counter: Option<i64>,
    records: Vec<RecordSpec>,
    waiters: Vec<WaiterSpec>,
}

impl QueueState {
    fn head_record(&self) -> Option<&RecordSpec> {
        self.records.iter().min_by_key(|record| record.ord)
    }

    fn head_waiter(&self) -> Option<&WaiterSpec> {
        self.waiters.iter().min_by_key(|waiter| waiter.ord)
    }

    /// The ord that the next operation takes: the counter, or 0 on a fresh collection.
    fn next_ord(&self) -> i64 { self.counter.unwrap_or(0) }

    /// `INV`: one side is empty, or the head waiter does not match the head record.
    fn no_pending_match(&self) -> bool {
        match (self.head_record(), self.head_waiter()) {
            (Some(record), Some(waiter)) => !waiter.fits(record.value),
            _ => true,
        }
    }

    /// Distinct orders on both sides, each below the counter.
    fn well_formed(&self) -> bool {
        let ords: BTreeSet<i64> = self
            .records
            .iter()
            .map(|record| record.ord)
            .chain(self.waiters.iter().map(|waiter| waiter.ord))
            .collect();
        ords.len() == self.records.len() + self.waiters.len()
            && match self.counter {
                Some(counter) => ords.iter().all(|ord| (0..counter).contains(ord)),
                None => ords.is_empty(),
            }
    }

    /// The same collection with the head waiter changed so that it does not match the head
    /// record.
    fn with_no_pending_match(mut self) -> QueueState {
        let head_value = self.head_record().map(|record| record.value);
        let head_ord = self.head_waiter().map(|waiter| waiter.ord);
        if let (Some(value), Some(ord)) = (head_value, head_ord) {
            for waiter in &mut self.waiters {
                if waiter.ord == ord && waiter.fits(value) {
                    waiter.pat = Pat::Lit(value + 100);
                }
            }
        }
        self
    }
}

/// One operation on the collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueOp {
    Produce {
        value: i64,
        persist: bool,
    },
    Consume {
        pat: Pat,
        persist: bool,
        peek: bool,
        guard: Option<bool>,
    },
}

fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![1 => Just(Pat::Any), 2 => (0i64..4).prop_map(Pat::Lit)].boxed()
}

fn arb_guard() -> BoxedStrategy<Option<bool>> {
    prop_oneof![8 => Just(None), 1 => Just(Some(true)), 1 => Just(Some(false))].boxed()
}

fn arb_record_fields() -> BoxedStrategy<(i64, bool)> {
    (0i64..4, prop_oneof![4 => Just(false), 1 => Just(true)]).boxed()
}

/// A waiter: plain, persistent (`<=`) or peek (`<<-`).
fn arb_waiter_fields() -> BoxedStrategy<(Pat, bool, bool, Option<bool>)> {
    (arb_pat(), 0u8..5, arb_guard())
        .prop_map(|(pat, kind, guard)| (pat, kind == 3, kind == 4, guard))
        .boxed()
}

/// A well-formed collection with at most `max` entries on each side. Most collections hold
/// records only or waiters only, the shapes that the operations reach. The orders are a random
/// injection into `0..counter`.
fn arb_queue_state(max: usize) -> BoxedStrategy<QueueState> {
    let sides = prop_oneof![
        2 => (1usize..=max).prop_map(|records| (records, 0usize)),
        2 => (1usize..=max).prop_map(|waiters| (0usize, waiters)),
        1 => (0usize..=max, 0usize..=max),
    ];
    (sides, 0i64..3, any::<bool>())
        .prop_flat_map(|((records, waiters), slack, fresh)| {
            let counter = i64::try_from(records + waiters).expect("few entries") + slack;
            (
                Just((0..counter).collect::<Vec<i64>>()).prop_shuffle(),
                vec(arb_record_fields(), records),
                vec(arb_waiter_fields(), waiters),
                Just((counter, fresh && records + waiters == 0)),
            )
        })
        .prop_map(|(ords, records, waiters, (counter, fresh))| {
            let mut ords = ords.into_iter();
            let records = records
                .into_iter()
                .map(|(value, persist)| RecordSpec {
                    ord: ords.next().expect("enough orders"),
                    value,
                    persist,
                })
                .collect();
            let waiters = waiters
                .into_iter()
                .map(|(pat, persist, peek, guard)| WaiterSpec {
                    ord: ords.next().expect("enough orders"),
                    pat,
                    persist,
                    peek,
                    guard,
                })
                .collect();
            QueueState {
                counter: match fresh {
                    true => None,
                    false => Some(counter),
                },
                records,
                waiters,
            }
        })
        .boxed()
}

fn arb_queue_op() -> BoxedStrategy<QueueOp> {
    prop_oneof![
        (0i64..4, prop_oneof![3 => Just(false), 1 => Just(true)])
            .prop_map(|(value, persist)| QueueOp::Produce { value, persist }),
        (arb_pat(), 0u8..4, arb_guard()).prop_map(|(pat, kind, guard)| QueueOp::Consume {
            pat,
            persist: kind == 1,
            peek: kind == 2,
            guard,
        }),
    ]
    .boxed()
}

/// An operation that consumes heads more often than `arb_queue_op`: most receives match every
/// value, and some are persistent or peek.
fn arb_fifo_op() -> BoxedStrategy<QueueOp> {
    prop_oneof![
        3 => (0i64..4, prop_oneof![5 => Just(false), 1 => Just(true)])
            .prop_map(|(value, persist)| QueueOp::Produce { value, persist }),
        3 => Just(QueueOp::Consume { pat: Pat::Any, persist: false, peek: false, guard: None }),
        1 => (0i64..4).prop_map(|value| QueueOp::Consume {
            pat: Pat::Lit(value),
            persist: false,
            peek: false,
            guard: None,
        }),
        1 => Just(QueueOp::Consume { pat: Pat::Any, persist: true, peek: false, guard: None }),
        1 => Just(QueueOp::Consume { pat: Pat::Any, persist: false, peek: true, guard: None }),
    ]
    .boxed()
}

/// `op` turned toward the side of `state` that holds entries: a produce meets the waiters and a
/// consume meets the records.
fn aimed(state: &QueueState, op: QueueOp) -> QueueOp {
    match (op, state.records.is_empty(), state.waiters.is_empty()) {
        (QueueOp::Produce { value, .. }, false, true) => QueueOp::Consume {
            pat: match value {
                0 | 1 => Pat::Any,
                _ => Pat::Lit(value),
            },
            persist: false,
            peek: false,
            guard: None,
        },
        (QueueOp::Consume { pat, persist, .. }, true, false) => QueueOp::Produce {
            value: match pat {
                Pat::Any => 0,
                Pat::Lit(value) => value,
            },
            persist,
        },
        (op, _, _) => op,
    }
}

fn random_of(parts: &[&[u8]]) -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(&parts.concat())
}

fn record_datum(record: &RecordSpec) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![gint(record.value)],
        random_state: random_of(&[b"rrs-p4-record", &record.ord.to_be_bytes()]).to_bytes(),
        cost_authority: None,
        cost_stack: None,
    }
}

fn waiter_label(ord: i64) -> String { format!("waiter-{ord}") }

fn waiter_continuation(waiter: &WaiterSpec) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(gstring(&waiter_label(waiter.ord))),
            random_state: random_of(&[b"rrs-p4-waiter", &waiter.ord.to_be_bytes()]).to_bytes(),
        })),
        guard: waiter.guard.map(gbool),
        cost_authority: None,
    }
}

/// The single number of a payload.
fn number_of(pars: &[Par]) -> i64 {
    match pars {
        [single] => match single.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => *value,
            other => panic!("a payload of these tests is one number: {other:?}"),
        },
        other => panic!("a payload of these tests has one par, not {}", other.len()),
    }
}

fn guard_of(guard: &Option<Par>) -> Option<bool> {
    guard.as_ref().map(|guard| match guard.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GBool(value)),
        }] => *value,
        other => panic!("a guard of these tests is one Boolean: {other:?}"),
    })
}

fn string_of(par: &Par) -> Option<&str> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GString(value)),
        }] => Some(value),
        _ => None,
    }
}

// Disabled by decision 42: the agent rig, its only caller, injects a counting matcher through
// `in_memory_rspace_with`, which this helper became.
// async fn in_memory_rspace() -> ParRSpace {
//     let mut manager = InMemoryStoreManager::new();
//     ParRSpace::create(
//         manager
//             .r_space_stores()
//             .await
//             .expect("the in-memory stores"),
//         Arc::new(Box::new(Matcher)),
//     )
//     .expect("an in-memory RSpace")
// }

/// An in-memory host with the injected matcher `matcher`.
async fn in_memory_rspace_with(
    matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>>,
) -> ParRSpace {
    let mut manager = InMemoryStoreManager::new();
    ParRSpace::create(
        manager
            .r_space_stores()
            .await
            .expect("the in-memory stores"),
        matcher,
    )
    .expect("an in-memory RSpace")
}

/// The rholang matcher with a count of its calls: every spatial match and every guard check,
/// with a meter or without one. It delegates each call unchanged, and it records the
/// reservations that the metered calls make.
struct CountingMatcher {
    calls: Arc<AtomicUsize>,
    reserved: Arc<[AtomicUsize; 3]>,
}

/// A meter that forwards each reservation of the matcher and records it.
struct ForwardingMeter<'m> {
    inner: &'m (dyn SourceMeter + Send + Sync),
    recorded: &'m [AtomicUsize; 3],
}

impl SourceMeter for ForwardingMeter<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        self.inner.reserve(operations, scanned, backing)?;
        for (recorded, amount) in self.recorded.iter().zip([operations, scanned, backing]) {
            recorded.fetch_add(amount, Ordering::Relaxed);
        }
        Ok(())
    }
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for CountingMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Matcher.get(pattern, data)
    }

    fn get_metered(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<ListParWithRandom>, RSpaceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let forwarding = ForwardingMeter {
            inner: meter,
            recorded: &self.reserved,
        };
        Matcher.get_metered(pattern, data, &forwarding)
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Matcher.check_commit(continuation, matched)
    }

    fn check_commit_metered(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool, RSpaceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let forwarding = ForwardingMeter {
            inner: meter,
            recorded: &self.reserved,
        };
        Matcher.check_commit_metered(continuation, matched, &forwarding)
    }
}

/// The real queue agent over a fresh in-memory host, with one created space and one collection.
struct AgentRig {
    host: ParRSpace,
    env: SpaceEnv,
    channel: Par,
    key: CollectionKey,
    channels: CollectionChannels,
    /// The matcher calls since the rig was staged.
    matcher_calls: Arc<AtomicUsize>,
    /// The reservations of the metered matcher calls since the rig was staged: operations,
    /// scanned bytes and backing bytes.
    matcher_reserved: Arc<[AtomicUsize; 3]>,
}

/// A padding par of `bytes` bytes.
fn padding(bytes: usize) -> Par { gstring(&"p".repeat(bytes)) }

impl AgentRig {
    /// Creates the space with the agent, then stages `state` on the collection in one raw
    /// transaction. `order` is the staging order of the entries: records first, then waiters,
    /// as indices into one list.
    async fn new(
        state: &QueueState,
        order: &[usize],
        host_work: Option<HostWorkBudget>,
    ) -> AgentRig {
        AgentRig::padded(state, order, host_work, &|_, _| 0).await
    }

    /// [`AgentRig::new`] with padding: `pad(side, ord)` bytes are appended to the payload of
    /// the record with the order `ord`, or to the body of the waiter with the order `ord`.
    async fn padded(
        state: &QueueState,
        order: &[usize],
        host_work: Option<HostWorkBudget>,
        pad: &dyn Fn(Side, i64) -> usize,
    ) -> AgentRig {
        let matcher_calls = Arc::new(AtomicUsize::new(0));
        let matcher_reserved: Arc<[AtomicUsize; 3]> = Arc::new(Default::default());
        let host = in_memory_rspace_with(Arc::new(Box::new(CountingMatcher {
            calls: Arc::clone(&matcher_calls),
            reserved: Arc::clone(&matcher_reserved),
        })))
        .await;
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(RIG_PHLO, "rrs-p4-props agent rig")),
            meter: ScanMeter::new(None),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let call = random_of(&[b"rrs-p4-props factory call"]);
        let creation = SpaceCreation {
            space: RIG_SPACE,
            config: SpaceConfig::new(RIG_SPACE.sid, "hashmap", "queue", ConfigQualifier::Default)
                .to_par(),
            handle_pattern: handle_pattern(),
            handle_continuation: handle_continuation(RIG_SPACE, &call),
            random: call,
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
        };
        QueueAgent::default()
            .prepare_create(&env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        let channel = gstring("rrs-p4-queue");
        let key = CollectionKey::of_channel(&channel).expect("the channel has a key");
        let channels = CollectionChannels::new(&HashMapOuter, RIG_SPACE, &key);
        if let Some(counter) = state.counter {
            let mut txn = env
                .store
                .begin(channels.lock_keys())
                .await
                .expect("a staging transaction");
            txn.produce(SpaceProduce {
                channel: channels.counter().clone(),
                data: counter_record(counter, &random_of(&[b"rrs-p4-counter"]), None),
                persist: false,
                outcome: ProduceOutcome::Store,
                before_observe: None,
            })
            .expect("the counter stages");
            for index in order {
                match state.records.get(*index) {
                    Some(record) => {
                        let mut datum = record_datum(record);
                        match pad(Side::Data, record.ord) {
                            0 => {}
                            bytes => datum.pars.push(padding(bytes)),
                        }
                        txn.produce(SpaceProduce {
                            channel: channels.data().clone(),
                            data: wrap_datum(record.ord, &datum),
                            persist: record.persist,
                            outcome: ProduceOutcome::Store,
                            before_observe: None,
                        })
                        .expect("a record stages");
                    }
                    None => {
                        let waiter = &state.waiters[*index - state.records.len()];
                        let mut continuation = waiter_continuation(waiter);
                        if let (
                            bytes @ 1..,
                            Some(TaggedCont::ParBody(ParWithRandom {
                                body: Some(body), ..
                            })),
                        ) = (
                            pad(Side::Waiters, waiter.ord),
                            &mut continuation.tagged_cont,
                        ) {
                            *body = concatenate_pars(body.clone(), padding(bytes));
                        }
                        txn.consume(SpaceConsume {
                            channels: vec![channels.data().clone()],
                            patterns: vec![waiter.pat.bind_pattern()],
                            continuation: wrap_continuation(waiter.ord, &continuation)
                                .expect("a waiter has a body"),
                            persist: waiter.persist,
                            peeks: match waiter.peek {
                                true => BTreeSet::from([0]),
                                false => BTreeSet::new(),
                            },
                            outcome: ConsumeOutcome::Park,
                            before_observe: None,
                        })
                        .expect("a waiter stages");
                    }
                }
            }
            txn.commit().await.expect("the staged collection commits");
        }
        host.take_event_log().await;
        let env = SpaceEnv {
            meter: ScanMeter::new(host_work),
            ..env
        };
        matcher_calls.store(0, Ordering::Relaxed);
        for amount in matcher_reserved.iter() {
            amount.store(0, Ordering::Relaxed);
        }
        AgentRig {
            host,
            env,
            channel,
            key,
            channels,
            matcher_calls,
            matcher_reserved,
        }
    }

    /// The matcher calls since the rig was staged.
    fn matcher_calls(&self) -> usize { self.matcher_calls.load(Ordering::Relaxed) }

    /// The reservations of the metered matcher calls since the rig was staged.
    fn matcher_reserved(&self) -> [u64; 3] {
        self.matcher_reserved.each_ref().map(|amount| {
            u64::try_from(amount.load(Ordering::Relaxed)).expect("a reservation fits u64")
        })
    }

    /// A rig with the entries staged in their natural order.
    async fn of(state: &QueueState) -> AgentRig {
        let order: Vec<usize> = (0..state.records.len() + state.waiters.len()).collect();
        AgentRig::new(state, &order, None).await
    }

    fn space_op(&self, op: &QueueOp, salt: u64) -> SpaceOp {
        match *op {
            QueueOp::Produce { value, persist } => SpaceOp::Produce(ProduceOp {
                space: RIG_SPACE,
                key: self.key.clone(),
                channel: self.channel.clone(),
                data: ListParWithRandom {
                    pars: vec![gint(value)],
                    random_state: random_of(&[b"rrs-p4-produce", &salt.to_be_bytes()]).to_bytes(),
                    cost_authority: None,
                    cost_stack: None,
                },
                persistent: persist,
                params: BTreeMap::new(),
                authority: CostAuthority::default(),
                reissued_ord: None,
            }),
            QueueOp::Consume {
                pat,
                persist,
                peek,
                guard,
            } => SpaceOp::Consume(ConsumeOp {
                space: RIG_SPACE,
                key: self.key.clone(),
                channel: self.channel.clone(),
                pattern: pat.bind_pattern(),
                body: ParWithRandom {
                    body: Some(gstring(&format!("op-{salt}"))),
                    random_state: random_of(&[b"rrs-p4-consume", &salt.to_be_bytes()]).to_bytes(),
                },
                persistent: persist,
                peek,
                guard: guard.map(gbool),
                query: None,
                authority: CostAuthority::default(),
                introduction_authority: CostAuthority::default(),
                reissued_ord: None,
            }),
        }
    }

    async fn run(&self, op: &QueueOp, salt: u64) -> Result<SpaceOutcome, SpaceError> {
        run_op(&QueueAgent::default(), &self.env, self.space_op(op, salt)).await
    }

    async fn view(&self) -> QueueState {
        let mut records: Vec<RecordSpec> = self
            .host
            .get_data(self.channels.data())
            .await
            .into_iter()
            .map(|datum| {
                let record = unwrap_datum(&datum.a).expect("a stored record has an envelope");
                RecordSpec {
                    ord: record.ord,
                    value: number_of(&record.datum.pars),
                    persist: datum.persist,
                }
            })
            .collect();
        records.sort_by_key(|record| record.ord);
        let mut waiters: Vec<WaiterSpec> = self
            .host
            .get_waiting_continuations(vec![self.channels.data().clone()])
            .await
            .into_iter()
            .map(|waiter| {
                let parked = unwrap_continuation(&waiter.continuation)
                    .expect("a parked waiter has an envelope");
                let [pattern] = waiter.patterns.as_slice() else {
                    panic!("a waiter of a queue has one bind");
                };
                WaiterSpec {
                    ord: parked.ord,
                    pat: Pat::of_bind_pattern(pattern),
                    persist: waiter.persist,
                    peek: !waiter.peeks.is_empty(),
                    guard: guard_of(&parked.continuation.guard),
                }
            })
            .collect();
        waiters.sort_by_key(|waiter| waiter.ord);
        let counter = match self.host.get_data(self.channels.counter()).await.as_slice() {
            [] => None,
            [datum] => Some(counter_value(&datum.a).expect("a counter record")),
            more => panic!("a collection has {} counter records", more.len()),
        };
        QueueState {
            counter,
            records,
            waiters,
        }
    }

    async fn space_counter(&self) -> i64 {
        match self
            .host
            .get_data(self.channels.space_counter())
            .await
            .as_slice()
        {
            [datum] => counter_value(&datum.a).expect("a space counter record"),
            more => panic!("a space has {} space counters", more.len()),
        }
    }
}

/// The state with its records and waiters in order.
fn normalized(mut state: QueueState) -> QueueState {
    state.records.sort_by_key(|record| record.ord);
    state.waiters.sort_by_key(|waiter| waiter.ord);
    state
}

/// The fired partners of an operation: for a produce the orders of the fired waiters, for a
/// consume the orders of the fired records.
fn fired_partners(op: &QueueOp, outcome: &SpaceOutcome) -> Vec<i64> {
    outcome
        .fires
        .iter()
        .map(|fire| match op {
            QueueOp::Produce { .. } => {
                let label = match &fire.continuation.tagged_cont {
                    Some(TaggedCont::ParBody(ParWithRandom {
                        body: Some(body), ..
                    })) => string_of(body)
                        .expect("a waiter body is its label")
                        .to_string(),
                    other => panic!("a fired waiter has a body: {other:?}"),
                };
                label
                    .strip_prefix("waiter-")
                    .and_then(|ord| ord.parse().ok())
                    .expect("a waiter label names its order")
            }
            QueueOp::Consume { .. } => {
                let [result] = fire.results.as_slice() else {
                    panic!("a queue fire binds one record");
                };
                datum_ord(&result.removed_datum).expect("a fired record has an envelope")
            }
        })
        .collect()
}

/// The ords of the other side of `op` in `state`, in head order.
fn other_side(state: &QueueState, op: &QueueOp) -> Vec<i64> {
    let mut ords: Vec<i64> = match op {
        QueueOp::Produce { .. } => state.waiters.iter().map(|waiter| waiter.ord).collect(),
        QueueOp::Consume { .. } => state.records.iter().map(|record| record.ord).collect(),
    };
    ords.sort_unstable();
    ords
}

/// The state that the head-only plan reaches when `op` stores or parks without a fire.
fn stored_or_parked(state: &QueueState, op: &QueueOp) -> QueueState {
    let mut expected = state.clone();
    let ord = state.next_ord();
    expected.counter = Some(ord + 1);
    match *op {
        QueueOp::Produce { value, persist } => expected.records.push(RecordSpec {
            ord,
            value,
            persist,
        }),
        QueueOp::Consume {
            pat,
            persist,
            peek,
            guard,
        } => expected.waiters.push(WaiterSpec {
            ord,
            pat,
            persist,
            peek,
            guard,
        }),
    }
    normalized(expected)
}

/// The legacy waiter scan (the legacy `GenericRSpace` scanned every waiter): the first waiter in
/// order that matches `value`.
fn legacy_scan(state: &QueueState, value: i64) -> Option<i64> {
    let mut waiters = state.waiters.clone();
    waiters.sort_by_key(|waiter| waiter.ord);
    waiters
        .iter()
        .find(|waiter| waiter.fits(value))
        .map(|waiter| waiter.ord)
}

/// A long collection for the round cap. It holds `waiters` plain waiters that match every
/// value, or `records` plain records. A persistent operation then fires more than
/// `FIRE_ROUND_CAP` times.
fn long_state(records: usize, waiters: usize) -> QueueState {
    let records: Vec<RecordSpec> = (0..records)
        .map(|index| RecordSpec {
            ord: i64::try_from(index).expect("few entries"),
            value: 1,
            persist: false,
        })
        .collect();
    let offset = records.len();
    let waiters: Vec<WaiterSpec> = (0..waiters)
        .map(|index| WaiterSpec {
            ord: i64::try_from(offset + index).expect("few entries"),
            pat: Pat::Any,
            persist: false,
            peek: false,
            guard: None,
        })
        .collect();
    QueueState {
        counter: Some(i64::try_from(offset + waiters.len()).expect("few entries")),
        records,
        waiters,
    }
}

/// A divergent pair at the head: a persistent waiter that matches every value, or a persistent
/// record that every receive matches, behind no other entry.
fn divergent_state(waiter_side: bool, rest: usize) -> QueueState {
    let mut state = long_state(
        usize::from(!waiter_side) * (rest + 1),
        usize::from(waiter_side) * (rest + 1),
    );
    match waiter_side {
        true => state.waiters[0].persist = true,
        false => state.records[0].persist = true,
    }
    state
}

/// A state and an operation for the loop bound. The case is a short collection and any
/// operation, a divergent pair, or a long collection with a persistent operation.
fn arb_loop_case() -> BoxedStrategy<(QueueState, QueueOp)> {
    let cap = FIRE_ROUND_CAP;
    prop_oneof![
        4 => (arb_queue_state(5), arb_queue_op()),
        1 => (0usize..3).prop_map(|rest| {
            (divergent_state(true, rest), QueueOp::Produce { value: 1, persist: true })
        }),
        1 => (0usize..3, any::<bool>()).prop_map(|(rest, peek)| {
            (
                divergent_state(false, rest),
                QueueOp::Consume { pat: Pat::Any, persist: true, peek, guard: None },
            )
        }),
        1 => (cap - 2..cap + 6).prop_map(|waiters| {
            (long_state(0, waiters), QueueOp::Produce { value: 1, persist: true })
        }),
        1 => (cap - 2..cap + 6).prop_map(|records| {
            (
                long_state(records, 0),
                QueueOp::Consume { pat: Pat::Any, persist: true, peek: false, guard: None },
            )
        }),
    ]
    .boxed()
}

/// `queue_head_is_min_ord`: the head of each side is an entry of that side with the least
/// `(ord, bytes)`. A side has a head exactly when it is not empty, and the list order does
/// not change the head. In the agent, an operation fires a prefix of the other side in head
/// order. It makes at most two matcher calls for each round, whatever the length of the
/// collection, so the candidates are the heads only.
#[test]
fn prop_queue_candidates_head_only() {
    check(
        pure_config(),
        (
            vec((0i64..6, "[a-c]{0,2}"), 0..8),
            arb_queue_state(6),
            arb_queue_op(),
        ),
        |(entries, state, op)| {
            tokio_runtime().block_on(queue_candidates_head_only_case(entries, state, op))
        },
    );
}

/// One case of `prop_queue_candidates_head_only`.
async fn queue_candidates_head_only_case(
    entries: Vec<(i64, String)>,
    state: QueueState,
    op: QueueOp,
) -> Result<(), TestCaseError> {
    let ordered: Vec<Ordered<'_, String>> = entries
        .iter()
        .map(|(ord, entry)| Ordered { ord: *ord, entry })
        .collect();
    for side in [Side::Data, Side::Waiters] {
        let head = QueueDiscipline
            .head(side, &ordered)
            .expect("the head is computed");
        prop_assert_eq!(head.is_none(), entries.is_empty());
        if let Some(index) = head {
            let key = |(ord, entry): &(i64, String)| {
                (
                    *ord,
                    bincode::serialize(entry).expect("an entry serializes"),
                )
            };
            let least = entries.iter().map(key).min().expect("a non-empty side");
            prop_assert_eq!(key(&entries[index]), least);
            let mut reversed = ordered.clone();
            reversed.reverse();
            let other = QueueDiscipline
                .head(side, &reversed)
                .expect("the head is computed");
            prop_assert_eq!(
                other.map(|index| reversed[index].entry),
                Some(&entries[index].1)
            );
        }
    }
    let op = aimed(&state, op);
    let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX / 4)));
    let order: Vec<usize> = (0..state.records.len() + state.waiters.len()).collect();
    let rig = AgentRig::new(&state, &order, Some(host_work.clone())).await;
    let outcome = rig.run(&op, 1).await.expect("the operation commits");
    let fired = fired_partners(&op, &outcome);
    let heads = other_side(&state, &op);
    prop_assert!(fired.len() <= heads.len());
    prop_assert_eq!(&fired[..], &heads[..fired.len()]);
    let calls = rig.matcher_calls();
    let bound = 1 + 2 * (fired.len() + 1);
    prop_assert!(
        calls <= bound,
        "{} matcher calls for {} fires of a collection of {} entries",
        calls,
        fired.len(),
        heads.len()
    );
    prop_assert_eq!(
        host_work.usage(HostWorkDimension::SearchCandidates),
        HostWorkUsage::ZERO,
        "the branch charges candidates only in its funding search"
    );
    Ok(())
}

/// `queue_produce_preserves_inv`: from a well-formed collection without a pending match,
/// a produce reaches a well-formed collection without a pending match.
#[test]
fn prop_queue_produce_preserves_invariant() {
    check(
        pure_config(),
        (arb_queue_state(5), 0i64..4, any::<bool>()),
        |(state, value, persist)| {
            tokio_runtime().block_on(queue_produce_preserves_invariant_case(
                state, value, persist,
            ))
        },
    );
}

/// One case of `prop_queue_produce_preserves_invariant`.
async fn queue_produce_preserves_invariant_case(
    state: QueueState,
    value: i64,
    persist: bool,
) -> Result<(), TestCaseError> {
    let state = state.with_no_pending_match();
    prop_assert!(state.well_formed() && state.no_pending_match());
    let rig = AgentRig::of(&state).await;
    prop_assert_eq!(rig.view().await, normalized(state.clone()));
    rig.run(&QueueOp::Produce { value, persist }, 1)
        .await
        .expect("the produce commits");
    let after = rig.view().await;
    prop_assert!(after.well_formed(), "{:?}", after);
    prop_assert!(
        after.no_pending_match(),
        "a produce left a pending match: {:?}",
        after
    );
    Ok(())
}

/// `queue_consume_preserves_inv`: from a well-formed collection without a pending match,
/// a consume reaches a well-formed collection without a pending match.
#[test]
fn prop_queue_consume_preserves_invariant() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op()),
        |(state, op)| tokio_runtime().block_on(queue_consume_preserves_invariant_case(state, op)),
    );
}

/// One case of `prop_queue_consume_preserves_invariant`.
async fn queue_consume_preserves_invariant_case(
    state: QueueState,
    op: QueueOp,
) -> Result<(), TestCaseError> {
    let state = state.with_no_pending_match();
    let op = match op {
        QueueOp::Produce { value, .. } => QueueOp::Consume {
            pat: Pat::Lit(value),
            persist: false,
            peek: false,
            guard: None,
        },
        consume => consume,
    };
    let rig = AgentRig::of(&state).await;
    rig.run(&op, 1).await.expect("the consume commits");
    let after = rig.view().await;
    prop_assert!(after.well_formed(), "{:?}", after);
    prop_assert!(
        after.no_pending_match(),
        "a consume left a pending match: {:?}",
        after
    );
    Ok(())
}

/// `queue_fifo`: over a run of operations from a well-formed collection, the records leave
/// in strictly increasing order. Every record that leaves is older than every record that
/// stays. A record that leaves was in the collection at the start or arrived during the run.
#[test]
fn prop_queue_fifo() {
    check(
        pure_config(),
        (
            arb_queue_state(5),
            proptest::bool::weighted(0.85),
            vec(arb_fifo_op(), 2..12),
        ),
        |(state, records_only, ops)| {
            tokio_runtime().block_on(queue_fifo_case(state, records_only, ops))
        },
    );
}

/// One case of `prop_queue_fifo`.
async fn queue_fifo_case(
    state: QueueState,
    records_only: bool,
    ops: Vec<QueueOp>,
) -> Result<(), TestCaseError> {
    let mut state = state;
    if records_only {
        state.waiters.clear();
    }
    let rig = AgentRig::of(&state).await;
    let start = state.next_ord();
    let mut removed = Vec::new();
    for (salt, op) in (1u64..).zip(&ops) {
        let outcome = rig.run(op, salt).await.expect("the operation commits");
        if let QueueOp::Consume { peek: false, .. } = op {
            for fire in &outcome.fires {
                let [result] = fire.results.as_slice() else {
                    panic!("a queue fire binds one record");
                };
                if !result.persistent {
                    removed.push(datum_ord(&result.removed_datum).expect("an envelope"));
                }
            }
        }
    }
    let after = rig.view().await;
    prop_assert!(
        removed.windows(2).all(|pair| pair[0] < pair[1]),
        "{:?}",
        removed
    );
    for gone in &removed {
        prop_assert!(after.records.iter().all(|record| *gone < record.ord));
        prop_assert!(
            state.records.iter().any(|record| record.ord == *gone) || *gone >= start,
            "the record {} came from nowhere",
            gone
        );
    }
    prop_assert!(after.next_ord() >= start);
    Ok(())
}

/// `queue_head_blocks`: only the heads interact. A datum that meets a head waiter that it
/// does not match is stored, also when a later waiter matches. A receive that meets a head
/// record that it does not match parks, also when a later record matches. An element whose
/// side is not empty joins the tail. The first fire of a plan is on the head.
#[test]
fn prop_queue_head_blocks() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op(), any::<bool>()),
        |(state, op, block)| tokio_runtime().block_on(queue_head_blocks_case(state, op, block)),
    );
}

/// One case of `prop_queue_head_blocks`.
async fn queue_head_blocks_case(
    state: QueueState,
    op: QueueOp,
    block: bool,
) -> Result<(), TestCaseError> {
    let mut state = state;
    if block {
        match op {
            QueueOp::Produce { value, .. } => {
                state.records.clear();
                if let Some(head) = state.waiters.iter_mut().min_by_key(|waiter| waiter.ord) {
                    head.pat = Pat::Lit(value + 100);
                }
            }
            QueueOp::Consume { pat, guard, .. } => {
                state.waiters.clear();
                if let (Some(head), Pat::Lit(expected), false) = (
                    state.records.iter_mut().min_by_key(|record| record.ord),
                    pat,
                    guard == Some(false),
                ) {
                    head.value = expected + 100;
                }
            }
        }
    }
    let rig = AgentRig::of(&state).await;
    let outcome = rig.run(&op, 1).await.expect("the operation commits");
    let after = rig.view().await;
    let fired = fired_partners(&op, &outcome);
    let blocked = match op {
        QueueOp::Produce { value, .. } => {
            !state.records.is_empty() || state.head_waiter().is_some_and(|head| !head.fits(value))
        }
        QueueOp::Consume { pat, guard, .. } => {
            !state.waiters.is_empty()
                || state
                    .head_record()
                    .is_some_and(|head| !(pat.fits(head.value) && guard != Some(false)))
        }
    };
    if blocked {
        prop_assert!(
            outcome.fires.is_empty(),
            "a blocked operation fired: {:?}",
            fired
        );
        prop_assert_eq!(&outcome.reissue, &None);
        prop_assert_eq!(&after, &stored_or_parked(&state, &op));
    }
    if let Some(first) = fired.first() {
        prop_assert_eq!(Some(*first), other_side(&state, &op).first().copied());
    }
    Ok(())
}

/// `queue_loop_bounded_or_reissues`: a plan fires at most `FIRE_ROUND_CAP` times and at most
/// once for each entry of the other side. A plan that stops at the cap fired exactly
/// `FIRE_ROUND_CAP` times. A divergent re-issue (A10) needs a persistent partner and a
/// surviving incoming side. A re-issue carries the order of the first round exactly when the
/// operation is persistent (decision 43).
#[test]
fn prop_queue_loop_bounded() {
    check(pure_config(), arb_loop_case(), |case| {
        tokio_runtime().block_on(queue_loop_bounded_case(case))
    });
}

/// One case of `prop_queue_loop_bounded`.
async fn queue_loop_bounded_case(case: (QueueState, QueueOp)) -> Result<(), TestCaseError> {
    let (state, op) = case;
    let rig = AgentRig::of(&state).await;
    let outcome = rig.run(&op, 1).await.expect("the operation commits");
    let fired = fired_partners(&op, &outcome);
    prop_assert!(fired.len() <= FIRE_ROUND_CAP);
    prop_assert!(fired.len() <= other_side(&state, &op).len());
    match outcome.reissue.as_ref().map(|reissue| reissue.cause) {
        Some(ReissueCause::RoundCap) => prop_assert_eq!(fired.len(), FIRE_ROUND_CAP),
        Some(ReissueCause::DivergentPair) => {
            let last = *fired.last().expect("a divergent pair fired");
            match op {
                QueueOp::Produce { persist, .. } => {
                    let waiter = state
                        .waiters
                        .iter()
                        .find(|waiter| waiter.ord == last)
                        .expect("the fired waiter was stored");
                    prop_assert!(waiter.persist);
                    prop_assert!(persist || waiter.peek);
                }
                QueueOp::Consume { persist, peek, .. } => {
                    let record = state
                        .records
                        .iter()
                        .find(|record| record.ord == last)
                        .expect("the fired record was stored");
                    prop_assert!(persist);
                    prop_assert!(record.persist || peek);
                }
            }
        }
        None => {}
    }
    if fired.len() > FIRE_ROUND_CAP - 1 {
        prop_assert!(outcome.reissue.is_some());
    }
    if let Some(reissue) = &outcome.reissue {
        let persistent = match op {
            QueueOp::Produce { persist, .. } | QueueOp::Consume { persist, .. } => persist,
        };
        prop_assert_eq!(
            kept_place(reissue).map(|(_, ord)| ord),
            persistent.then_some(state.next_ord())
        );
    }
    Ok(())
}

/// `queue_ord_fresh`: the ord of a fresh operation is the value of the collection counter, above
/// the ord of every entry. The operation advances the counter by one and keeps the
/// collection well formed. The first operation takes the ord 0, opens the counter at 1 and
/// advances the space counter.
#[test]
fn prop_queue_ord_fresh() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op()),
        |(state, op)| tokio_runtime().block_on(queue_ord_fresh_case(state, op)),
    );
}

/// One case of `prop_queue_ord_fresh`.
async fn queue_ord_fresh_case(state: QueueState, op: QueueOp) -> Result<(), TestCaseError> {
    prop_assert!(state.well_formed());
    let rig = AgentRig::of(&state).await;
    let space_counter = rig.space_counter().await;
    let ord = state.next_ord();
    rig.run(&op, 1).await.expect("the operation commits");
    let after = rig.view().await;
    prop_assert!(after.well_formed(), "{:?}", after);
    prop_assert_eq!(after.counter, Some(ord + 1));
    let opened = i64::from(state.counter.is_none());
    prop_assert_eq!(rig.space_counter().await, space_counter + opened);
    let before: BTreeSet<i64> = state
        .records
        .iter()
        .map(|record| record.ord)
        .chain(state.waiters.iter().map(|waiter| waiter.ord))
        .collect();
    let new: Vec<i64> = after
        .records
        .iter()
        .map(|record| record.ord)
        .chain(after.waiters.iter().map(|waiter| waiter.ord))
        .filter(|ord| !before.contains(ord))
        .collect();
    prop_assert!(
        new.iter().all(|new_ord| *new_ord == ord),
        "{:?} {}",
        new,
        ord
    );
    prop_assert!(new.len() <= 1);
    prop_assert!(before.iter().all(|old| *old < ord));
    Ok(())
}

/// `queue_plan_permutation_invariant`: the plan depends on the entries of the collection,
/// not on the list order of the store. Two stores with the same entries in other list
/// orders stage the same operations, log the same events and reach the same entries.
#[test]
fn prop_queue_plan_permutation_invariant() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op(), any::<u64>()),
        |(state, op, order_seed)| {
            tokio_runtime().block_on(queue_plan_permutation_invariant_case(state, op, order_seed))
        },
    );
}

/// One case of `prop_queue_plan_permutation_invariant`.
async fn queue_plan_permutation_invariant_case(
    state: QueueState,
    op: QueueOp,
    order_seed: u64,
) -> Result<(), TestCaseError> {
    let entries = state.records.len() + state.waiters.len();
    let natural: Vec<usize> = (0..entries).collect();
    let mut shuffled = natural.clone();
    let mut draw = order_seed;
    for index in (1..shuffled.len()).rev() {
        draw = draw
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let other = usize::try_from(draw >> 33).expect("a small draw") % (index + 1);
        shuffled.swap(index, other);
    }
    let first_work = unlimited_host_work();
    let second_work = unlimited_host_work();
    let first = AgentRig::new(&state, &natural, Some(first_work.clone())).await;
    let second = AgentRig::new(&state, &shuffled, Some(second_work.clone())).await;
    let first_outcome = first.run(&op, 1).await;
    let second_outcome = second.run(&op, 1).await;
    prop_assert_eq!(&first_outcome, &second_outcome);
    prop_assert_eq!(
        first.host.take_event_log().await,
        second.host.take_event_log().await
    );
    prop_assert_eq!(first.view().await, second.view().await);
    prop_assert_eq!(
        first_work.usages(),
        second_work.usages(),
        "the charges do not depend on the list order of the store"
    );
    Ok(())
}

/// `legacy_scan_violates_head_blocks`: the head waiter does not match a datum, and a later
/// waiter does. The legacy waiter scan fires the later waiter, so it breaks head-blocks and
/// the order of the waiters. The head-only plan of P4 stores the datum.
#[test]
fn prop_legacy_scan_breaks_head_blocks() {
    check(
        pure_config(),
        (vec(arb_waiter_fields(), 1..5), 0i64..4),
        |(waiters, value)| {
            tokio_runtime().block_on(legacy_scan_breaks_head_blocks_case(waiters, value))
        },
    );
}

/// One case of `prop_legacy_scan_breaks_head_blocks`.
async fn legacy_scan_breaks_head_blocks_case(
    waiters: Vec<(Pat, bool, bool, Option<bool>)>,
    value: i64,
) -> Result<(), TestCaseError> {
    let mut state = QueueState {
        counter: Some(i64::try_from(waiters.len() + 1).expect("few waiters")),
        records: Vec::new(),
        waiters: (0i64..)
            .zip(waiters)
            .map(|(ord, (pat, persist, peek, guard))| WaiterSpec {
                ord,
                pat,
                persist,
                peek,
                guard,
            })
            .collect(),
    };
    state.waiters[0].pat = Pat::Lit(value + 100);
    state.waiters.push(WaiterSpec {
        ord: i64::try_from(state.waiters.len()).expect("few waiters"),
        pat: Pat::Any,
        persist: false,
        peek: false,
        guard: None,
    });
    let head = state.head_waiter().map(|waiter| waiter.ord);
    let scanned = legacy_scan(&state, value);
    prop_assert!(scanned.is_some());
    prop_assert_ne!(scanned, head);
    prop_assert!(scanned > head);
    let rig = AgentRig::of(&state).await;
    let op = QueueOp::Produce {
        value,
        persist: false,
    };
    let outcome = rig.run(&op, 1).await.expect("the produce commits");
    prop_assert!(
        outcome.fires.is_empty(),
        "the head-only plan fired {:?}",
        fired_partners(&op, &outcome)
    );
    prop_assert_eq!(rig.view().await, stored_or_parked(&state, &op));
    Ok(())
}

// ─── Re-entries (decision 43) ────────────────────────────────────────────────────────────────

/// The side and the kept order of a persistent element in flight. An element that is not
/// persistent keeps no order: it re-enters as a fresh operation (decision 43).
fn kept_place(reissue: &SpaceReissue) -> Option<(Side, i64)> {
    match &reissue.op {
        SpaceOp::Produce(op) => op.reissued_ord.map(|ord| (Side::Data, ord)),
        SpaceOp::Consume(op) => op.reissued_ord.map(|ord| (Side::Waiters, ord)),
        SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
        SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
    }
}

/// `SHADOW(R, W, F)`: when the head waiter matches the head record, a persistent element in
/// flight precedes the head of its side.
fn shadowed(state: &QueueState, flight: &[SpaceReissue]) -> bool {
    match (state.head_record(), state.head_waiter()) {
        (Some(record), Some(waiter)) if waiter.fits(record.value) => flight
            .iter()
            .filter_map(kept_place)
            .any(|(side, ord)| match side {
                Side::Data => ord < record.ord,
                Side::Waiters => ord < waiter.ord,
            }),
        _ => true,
    }
}

/// `wf'`: the collection is well formed, and the kept orders of the persistent elements in
/// flight are distinct, below the counter, and distinct from the orders of the stored entries.
fn well_formed_in_flight(state: &QueueState, flight: &[SpaceReissue]) -> bool {
    let stored: BTreeSet<i64> = state
        .records
        .iter()
        .map(|record| record.ord)
        .chain(state.waiters.iter().map(|waiter| waiter.ord))
        .collect();
    let kept: Vec<i64> = flight
        .iter()
        .filter_map(kept_place)
        .map(|(_, ord)| ord)
        .collect();
    let distinct: BTreeSet<i64> = kept.iter().copied().collect();
    state.well_formed()
        && distinct.len() == kept.len()
        && kept.iter().all(|ord| {
            !stored.contains(ord)
                && state
                    .counter
                    .is_some_and(|counter| (0..counter).contains(ord))
        })
}

/// A collection whose waiter side is long: `peeks` peek waiters, then `plain` plain waiters,
/// then one waiter of the value `literal`. Every waiter but the last takes every value, so a
/// persistent send reaches the cap when `peeks + plain` is at least `FIRE_ROUND_CAP`.
fn capped_waiters(peeks: usize, plain: usize, literal: i64) -> QueueState {
    let mut waiters: Vec<WaiterSpec> = (0..peeks + plain)
        .map(|index| WaiterSpec {
            ord: i64::try_from(index).expect("few entries"),
            pat: Pat::Any,
            persist: false,
            peek: index < peeks,
            guard: None,
        })
        .collect();
    waiters.push(WaiterSpec {
        ord: i64::try_from(peeks + plain).expect("few entries"),
        pat: Pat::Lit(literal),
        persist: false,
        peek: false,
        guard: None,
    });
    QueueState {
        counter: Some(i64::try_from(waiters.len()).expect("few entries")),
        records: Vec::new(),
        waiters,
    }
}

/// A collection for a run with re-entries: a small collection without a pending match, a long
/// waiter side, or a long record side. On the long sides a persistent operation reaches the cap.
fn arb_reentry_state() -> BoxedStrategy<QueueState> {
    let cap = FIRE_ROUND_CAP;
    prop_oneof![
        1 => arb_queue_state(5).prop_map(QueueState::with_no_pending_match),
        2 => (0usize..3, cap - 2..cap + 4, 0i64..4)
            .prop_map(|(peeks, plain, literal)| capped_waiters(peeks, plain, literal)),
        2 => (cap - 2..cap + 4).prop_map(|records| long_state(records, 0)),
    ]
    .boxed()
}

/// One step of a run with re-entries: a fresh operation, or the re-entry of the element in
/// flight with this index, modulo the number of elements in flight.
#[derive(Clone, Copy, Debug)]
enum RunStep {
    Fresh(QueueOp),
    Reenter(usize),
}

fn arb_run_step() -> BoxedStrategy<RunStep> {
    prop_oneof![
        3 => arb_queue_op().prop_map(RunStep::Fresh),
        1 => (0i64..4).prop_map(|value| RunStep::Fresh(QueueOp::Produce { value, persist: true })),
        1 => Just(RunStep::Fresh(QueueOp::Consume {
            pat: Pat::Any,
            persist: true,
            peek: false,
            guard: None,
        })),
        3 => (0usize..4).prop_map(RunStep::Reenter),
    ]
    .boxed()
}

/// A run: a collection, a persistent operation toward its long side, and the steps after it.
fn arb_run() -> BoxedStrategy<(QueueState, Vec<RunStep>)> {
    (arb_reentry_state(), 0i64..4, vec(arb_run_step(), 1..10))
        .prop_map(|(state, value, mut steps)| {
            let opening = match (state.waiters.is_empty(), state.records.is_empty()) {
                (false, _) => QueueOp::Produce {
                    value,
                    persist: true,
                },
                (true, false) => QueueOp::Consume {
                    pat: Pat::Any,
                    persist: true,
                    peek: false,
                    guard: None,
                },
                (true, true) => QueueOp::Produce {
                    value,
                    persist: false,
                },
            };
            steps.insert(0, RunStep::Fresh(opening));
            (state, steps)
        })
        .boxed()
}

/// The steps that the drain of a run may take after its planned steps. A divergent pair
/// re-issues in every round, so the drain needs a bound.
const DRAIN_STEPS: usize = 6;

/// One step of a run as it ran: the operation, its outcome, the order that it took, and the
/// collection and the elements in flight after it.
struct RunRecord {
    /// The operation of the element of the step, as the run first issued it.
    queue_op: QueueOp,
    op: SpaceOp,
    outcome: SpaceOutcome,
    /// The value of the counter before the step: the order of a fresh operation.
    bumped: i64,
    before: QueueState,
    after: QueueState,
    flight: Vec<SpaceReissue>,
    /// The order of each waiter body that the run has issued so far, by its label.
    labels: BTreeMap<String, i64>,
}

impl AgentRig {
    /// Runs one operation, a fresh one or a re-issue.
    async fn run_space_op(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&QueueAgent::default(), &self.env, op).await
    }

    /// Runs `steps`, then lets the elements in flight re-enter, oldest first, for at most
    /// `DRAIN_STEPS` more steps. A re-entry with no element in flight is skipped.
    async fn run_with_reentries(&self, steps: &[RunStep]) -> Vec<RunRecord> {
        let mut flight: Vec<(QueueOp, SpaceReissue)> = Vec::new();
        let mut records = Vec::with_capacity(steps.len() + DRAIN_STEPS);
        let mut labels: BTreeMap<String, i64> = BTreeMap::new();
        let mut before = self.view().await;
        let planned = steps
            .iter()
            .copied()
            .map(Some)
            .chain(std::iter::repeat_n(None, DRAIN_STEPS));
        for (salt, step) in (1u64..).zip(planned) {
            let (queue_op, op) = match (step, flight.len()) {
                (Some(RunStep::Fresh(op)), _) => (op, self.space_op(&op, salt)),
                (Some(RunStep::Reenter(_)), 0) => continue,
                (Some(RunStep::Reenter(index)), len) => {
                    let (queue_op, reissue) = flight.remove(index % len);
                    (queue_op, reissue.op)
                }
                (None, 0) => break,
                (None, _) => {
                    let (queue_op, reissue) = flight.remove(0);
                    (queue_op, reissue.op)
                }
            };
            if let SpaceOp::Consume(ConsumeOp {
                body: ParWithRandom {
                    body: Some(body), ..
                },
                reissued_ord,
                ..
            }) = &op
            {
                let label = string_of(body).expect("a run body is its label");
                labels.insert(label.to_string(), reissued_ord.unwrap_or(before.next_ord()));
            }
            let outcome = self
                .run_space_op(op.clone())
                .await
                .expect("the operation commits");
            if let Some(reissue) = &outcome.reissue {
                flight.push((queue_op, reissue.clone()));
            }
            let after = self.view().await;
            records.push(RunRecord {
                queue_op,
                op,
                outcome,
                bumped: before.next_ord(),
                before,
                after: after.clone(),
                flight: flight.iter().map(|(_, reissue)| reissue.clone()).collect(),
                labels: labels.clone(),
            });
            before = after;
        }
        records
    }
}

/// The order that a run step took: the kept order of a re-entry, or the bump value.
fn step_ord(record: &RunRecord) -> i64 {
    let kept = match &record.op {
        SpaceOp::Produce(op) => op.reissued_ord,
        SpaceOp::Consume(op) => op.reissued_ord,
        SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
        SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
    };
    kept.unwrap_or(record.bumped)
}

/// True when the operation of a step is persistent.
fn persistent_op(op: &SpaceOp) -> bool {
    match op {
        SpaceOp::Produce(op) => op.persistent,
        SpaceOp::Consume(op) => op.persistent,
        SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
        SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
    }
}

/// `queue_reissue_keeps_ord` and `ReissueKeepsOrd` (decision 43): over a run with re-entries
/// between other operations, the re-issue of a persistent operation carries the order of its
/// first round, and the re-issue of an operation that is not persistent carries none. A
/// persistent element that ends stored or parked keeps the order of its first round. Every
/// step, fresh or re-entered, bumps the counter once, and the collection and the elements in
/// flight stay well formed (`wf'`).
#[test]
fn prop_queue_reissue_keeps_first_round_ord() {
    check(pure_config(), arb_run(), |(state, steps)| {
        tokio_runtime().block_on(queue_reissue_keeps_first_round_ord_case(state, steps))
    });
}

/// One case of `prop_queue_reissue_keeps_first_round_ord`.
async fn queue_reissue_keeps_first_round_ord_case(
    state: QueueState,
    steps: Vec<RunStep>,
) -> Result<(), TestCaseError> {
    let rig = AgentRig::of(&state).await;
    let run = rig.run_with_reentries(&steps).await;
    for record in &run {
        let ord = step_ord(record);
        prop_assert_eq!(record.after.counter, Some(record.bumped + 1));
        prop_assert!(
            well_formed_in_flight(&record.after, &record.flight),
            "{:?} {:?}",
            record.after,
            record.flight
        );
        let kept = record.outcome.reissue.as_ref().map(kept_place);
        match (persistent_op(&record.op), kept) {
            (true, Some(place)) => prop_assert_eq!(place.map(|(_, kept)| kept), Some(ord)),
            (false, Some(place)) => prop_assert_eq!(place, None),
            (_, None) => {}
        }
        let stays = persistent_op(&record.op) && record.outcome.reissue.is_none();
        if stays {
            let stored = match &record.op {
                SpaceOp::Produce(_) => record.after.records.iter().any(|entry| entry.ord == ord),
                SpaceOp::Consume(_) => record.after.waiters.iter().any(|entry| entry.ord == ord),
                SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
                SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
            };
            prop_assert!(stored, "the element of order {} is not at its place", ord);
        }
    }
    prop_assert!(run.len() <= steps.len() + DRAIN_STEPS);
    Ok(())
}

/// `queue_reentry_preserves_shadow_inv`, `queue_quiescent_inv`, `ShadowedNoPendingMatch` and
/// `QuiescentNoPendingMatch` (decision 43): from a collection without a pending match, every
/// step of a run with re-entries keeps `SHADOW`: when the heads match, a persistent element in
/// flight precedes the head of its side. A step after which no persistent element is in flight
/// leaves no pending match.
#[test]
fn prop_queue_shadow_invariant_with_inflight() {
    check(pure_config(), arb_run(), |(state, steps)| {
        tokio_runtime().block_on(queue_shadow_invariant_with_inflight_case(state, steps))
    });
}

/// One case of `prop_queue_shadow_invariant_with_inflight`.
async fn queue_shadow_invariant_with_inflight_case(
    state: QueueState,
    steps: Vec<RunStep>,
) -> Result<(), TestCaseError> {
    prop_assert!(state.well_formed() && state.no_pending_match());
    let rig = AgentRig::of(&state).await;
    for record in rig.run_with_reentries(&steps).await {
        prop_assert!(
            shadowed(&record.after, &record.flight),
            "{:?} left an unshadowed pending match: {:?} {:?}",
            record.op,
            record.after,
            record.flight
        );
        if record
            .flight
            .iter()
            .all(|reissue| kept_place(reissue).is_none())
        {
            prop_assert!(
                record.after.no_pending_match(),
                "{:?} left a pending match with no persistent element in flight: {:?}",
                record.op,
                record.after
            );
        }
    }
    Ok(())
}

/// `queue_fifo` with re-entries and `FifoOrder` (decision 43): over a run with re-entries, the
/// records that are not persistent leave in strictly increasing order, each is older than every
/// record that is not persistent and stays, and each was in the collection at the start or
/// arrived during the run.
#[test]
fn prop_queue_fifo_with_reentries() {
    check(pure_config(), arb_run(), |(state, steps)| {
        tokio_runtime().block_on(queue_fifo_with_reentries_case(state, steps))
    });
}

/// One case of `prop_queue_fifo_with_reentries`.
async fn queue_fifo_with_reentries_case(
    state: QueueState,
    steps: Vec<RunStep>,
) -> Result<(), TestCaseError> {
    let rig = AgentRig::of(&state).await;
    let start = state.next_ord();
    let mut removed = Vec::new();
    for record in rig.run_with_reentries(&steps).await {
        let SpaceOp::Consume(consume) = &record.op else {
            continue;
        };
        if consume.peek {
            continue;
        }
        for fire in &record.outcome.fires {
            let [result] = fire.results.as_slice() else {
                panic!("a queue fire binds one record");
            };
            if result.persistent {
                continue;
            }
            let gone = datum_ord(&result.removed_datum).expect("a fired record has an envelope");
            prop_assert!(
                record
                    .after
                    .records
                    .iter()
                    .filter(|kept| !kept.persist)
                    .all(|kept| gone < kept.ord),
                "{} left before an older record",
                gone
            );
            prop_assert!(
                state.records.iter().any(|initial| initial.ord == gone) || gone >= start,
                "the record {} came from nowhere",
                gone
            );
            removed.push(gone);
        }
    }
    prop_assert!(
        removed.windows(2).all(|pair| pair[0] < pair[1]),
        "{:?}",
        removed
    );
    Ok(())
}

/// The re-issue of a send that is not persistent carries no order (decision 43, the negative
/// control C1 of `HeadOnlyDisciplines.v`): a send that fires `FIRE_ROUND_CAP` peek waiters
/// re-issues unchanged, and after any steps between the rounds its re-entry takes the order of
/// its new bump. A re-entry that stores the datum stores it at that order.
#[test]
fn prop_queue_nonpersistent_reissue_fresh_ord() {
    check(
        pure_config(),
        (
            FIRE_ROUND_CAP..FIRE_ROUND_CAP + 3,
            0i64..4,
            0i64..4,
            vec(arb_queue_op().prop_map(transient), 0..4),
        ),
        |(peeks, literal, value, between)| {
            tokio_runtime().block_on(queue_nonpersistent_reissue_fresh_ord_case(
                peeks, literal, value, between,
            ))
        },
    );
}

/// `op` without persistence. The steps between the rounds of
/// `prop_queue_nonpersistent_reissue_fresh_ord` re-issue nothing, so nothing else is in flight.
fn transient(op: QueueOp) -> QueueOp {
    match op {
        QueueOp::Produce { value, .. } => QueueOp::Produce {
            value,
            persist: false,
        },
        QueueOp::Consume {
            pat, peek, guard, ..
        } => QueueOp::Consume {
            pat,
            persist: false,
            peek,
            guard,
        },
    }
}

/// One case of `prop_queue_nonpersistent_reissue_fresh_ord`.
async fn queue_nonpersistent_reissue_fresh_ord_case(
    peeks: usize,
    literal: i64,
    value: i64,
    between: Vec<QueueOp>,
) -> Result<(), TestCaseError> {
    let state = capped_waiters(peeks, 0, literal);
    let rig = AgentRig::of(&state).await;
    let send = rig.space_op(
        &QueueOp::Produce {
            value,
            persist: false,
        },
        1,
    );
    let outcome = rig
        .run_space_op(send.clone())
        .await
        .expect("the send commits");
    prop_assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let reissue = outcome.reissue.expect("the peek waiters reach the cap");
    prop_assert_eq!(&reissue.op, &send, "the re-issue keeps no order");
    for (salt, op) in (2u64..).zip(&between) {
        rig.run(op, salt)
            .await
            .expect("a step between the rounds commits");
    }
    let before = rig.view().await;
    let bumped = before.next_ord();
    rig.run_space_op(reissue.op)
        .await
        .expect("the re-entry commits");
    let after = rig.view().await;
    prop_assert_eq!(after.counter, Some(bumped + 1));
    let fresh = RecordSpec {
        ord: bumped,
        value,
        persist: false,
    };
    let added: Vec<&RecordSpec> = after
        .records
        .iter()
        .filter(|record| !before.records.contains(record))
        .collect();
    prop_assert!(
        added.iter().all(|record| **record == fresh),
        "the re-entry stored {:?}, not at the order {} of its bump",
        added,
        bumped
    );
    prop_assert!(after.well_formed(), "{:?}", after);
    prop_assert!(
        after.no_pending_match(),
        "the re-entry left a pending match with nothing in flight: {:?}",
        after
    );
    Ok(())
}

/// The collection `state` after the element of `op` was stored or parked at the order `ord`
/// with no fire, by an operation that bumped the counter once.
fn placed_at(state: &QueueState, op: &QueueOp, ord: i64) -> QueueState {
    let mut expected = state.clone();
    expected.counter = Some(state.next_ord() + 1);
    match *op {
        QueueOp::Produce { value, persist } => expected.records.push(RecordSpec {
            ord,
            value,
            persist,
        }),
        QueueOp::Consume {
            pat,
            persist,
            peek,
            guard,
        } => expected.waiters.push(WaiterSpec {
            ord,
            pat,
            persist,
            peek,
            guard,
        }),
    }
    normalized(expected)
}

/// The fired partners of a run step, as `fired_partners` gives them: a waiter of the initial
/// collection names its order in its label, and a waiter that the run issued has its order in
/// `record.labels`.
fn run_fired_partners(record: &RunRecord) -> Vec<i64> {
    record
        .outcome
        .fires
        .iter()
        .map(|fire| match record.queue_op {
            QueueOp::Produce { .. } => {
                let label = match &fire.continuation.tagged_cont {
                    Some(TaggedCont::ParBody(ParWithRandom {
                        body: Some(body), ..
                    })) => string_of(body).expect("a waiter body is its label"),
                    other => panic!("a fired waiter has a body: {other:?}"),
                };
                match record.labels.get(label) {
                    Some(ord) => *ord,
                    None => label
                        .strip_prefix("waiter-")
                        .and_then(|ord| ord.parse().ok())
                        .expect("a waiter label names its order"),
                }
            }
            QueueOp::Consume { .. } => {
                let [result] = fire.results.as_slice() else {
                    panic!("a queue fire binds one record");
                };
                datum_ord(&result.removed_datum).expect("a fired record has an envelope")
            }
        })
        .collect()
}

/// `queue_head_blocks_reentry` and `queue_reentry_loop_bounded_or_reissues` (decision 43): in a
/// run with re-entries, a re-entry whose own side holds an older head fires nothing and rests at
/// its kept order behind that head. A re-entry that reaches the head fires first on the head of
/// the other side. Every step fires at most `FIRE_ROUND_CAP` times and at most once for each
/// entry of the other side.
#[test]
fn prop_queue_reentry_head_blocks() {
    check(
        pure_config(),
        prop_oneof![arb_run(), arb_double_flight_run()],
        |(state, steps)| tokio_runtime().block_on(queue_reentry_head_blocks_case(state, steps)),
    );
}

/// A run in which two persistent sends reach the cap one after the other, so both are in
/// flight, and then re-enter, the older one first. The last waiter takes the value of the
/// younger send only. The older send rests at the head of the records, and the younger
/// re-entry then finds that older record at the head of its side, so it must rest behind it and
/// fire nothing, although the last waiter takes its value.
fn arb_double_flight_run() -> BoxedStrategy<(QueueState, Vec<RunStep>)> {
    (0i64..4, 1i64..4, vec(arb_run_step(), 0..4))
        .prop_map(|(younger, offset, rest)| {
            let state = capped_waiters(0, 2 * FIRE_ROUND_CAP, younger);
            let mut steps = vec![
                RunStep::Fresh(QueueOp::Produce {
                    value: younger + offset,
                    persist: true,
                }),
                RunStep::Fresh(QueueOp::Produce {
                    value: younger,
                    persist: true,
                }),
                RunStep::Reenter(0),
                RunStep::Reenter(0),
            ];
            steps.extend(rest);
            (state, steps)
        })
        .boxed()
}

/// One case of `prop_queue_reentry_head_blocks`.
async fn queue_reentry_head_blocks_case(
    state: QueueState,
    steps: Vec<RunStep>,
) -> Result<(), TestCaseError> {
    let rig = AgentRig::of(&state).await;
    for record in rig.run_with_reentries(&steps).await {
        let fired = run_fired_partners(&record);
        let other = other_side(&record.before, &record.queue_op);
        prop_assert!(fired.len() <= FIRE_ROUND_CAP);
        prop_assert!(fired.len() <= other.len());
        if let Some(first) = fired.first() {
            prop_assert_eq!(Some(*first), other.first().copied());
        }
        let kept = match &record.op {
            SpaceOp::Produce(op) => op.reissued_ord,
            SpaceOp::Consume(op) => op.reissued_ord,
            SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
            SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
        };
        let Some(ord) = kept else {
            continue;
        };
        let own_head = match record.queue_op {
            QueueOp::Produce { .. } => record.before.head_record().map(|head| head.ord),
            QueueOp::Consume { .. } => record.before.head_waiter().map(|head| head.ord),
        };
        if own_head.is_some_and(|head| head < ord) {
            prop_assert!(
                record.outcome.fires.is_empty(),
                "a re-entry behind an older head fired: {:?}",
                fired
            );
            prop_assert_eq!(&record.outcome.reissue, &None);
            prop_assert_eq!(
                &record.after,
                &placed_at(&record.before, &record.queue_op, ord)
            );
        }
    }
    Ok(())
}

/// `queue_reentry_permutation_invariant` (decision 43): the re-entry of a persistent element
/// depends on the entries of the collection, not on the list order of the store. Two stores
/// with the same entries in other list orders stage the same operations, log the same events,
/// reach the same entries, and charge the same host work.
#[test]
fn prop_queue_reentry_permutation_invariant() {
    check(
        pure_config(),
        (arb_queue_state(5), any::<bool>(), 0i64..4, any::<u64>()),
        |(state, send, value, order_seed)| {
            tokio_runtime().block_on(queue_reentry_permutation_invariant_case(
                state, send, value, order_seed,
            ))
        },
    );
}

/// One case of `prop_queue_reentry_permutation_invariant`. The re-entry keeps the least order
/// below the counter that no entry holds. A collection with no free order gains one by a larger
/// counter.
async fn queue_reentry_permutation_invariant_case(
    state: QueueState,
    send: bool,
    value: i64,
    order_seed: u64,
) -> Result<(), TestCaseError> {
    let mut state = state;
    let held: BTreeSet<i64> = state
        .records
        .iter()
        .map(|record| record.ord)
        .chain(state.waiters.iter().map(|waiter| waiter.ord))
        .collect();
    let counter = state.next_ord();
    let kept = match (0..counter).find(|ord| !held.contains(ord)) {
        Some(ord) => ord,
        None => {
            state.counter = Some(counter + 1);
            counter
        }
    };
    let op = match send {
        true => QueueOp::Produce {
            value,
            persist: true,
        },
        false => QueueOp::Consume {
            pat: Pat::Any,
            persist: true,
            peek: false,
            guard: None,
        },
    };
    let entries = state.records.len() + state.waiters.len();
    let natural: Vec<usize> = (0..entries).collect();
    let mut shuffled = natural.clone();
    let mut draw = order_seed;
    for index in (1..shuffled.len()).rev() {
        draw = draw
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let other = usize::try_from(draw >> 33).expect("a small draw") % (index + 1);
        shuffled.swap(index, other);
    }
    let first_work = unlimited_host_work();
    let second_work = unlimited_host_work();
    let first = AgentRig::new(&state, &natural, Some(first_work.clone())).await;
    let second = AgentRig::new(&state, &shuffled, Some(second_work.clone())).await;
    let reentry = |rig: &AgentRig| match rig.space_op(&op, 1) {
        SpaceOp::Produce(produce) => SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(kept),
            ..produce
        }),
        SpaceOp::Consume(consume) => SpaceOp::Consume(ConsumeOp {
            reissued_ord: Some(kept),
            ..consume
        }),
        SpaceOp::Settle(_) => unreachable!("the queue discipline never issues a settle"),
        SpaceOp::Join(_) => unreachable!("the queue programs of this suite issue no join"),
    };
    let first_outcome = first.run_space_op(reentry(&first)).await;
    let second_outcome = second.run_space_op(reentry(&second)).await;
    prop_assert!(first_outcome.is_ok(), "{:?}", first_outcome);
    prop_assert_eq!(&first_outcome, &second_outcome);
    prop_assert_eq!(
        first.host.take_event_log().await,
        second.host.take_event_log().await
    );
    prop_assert_eq!(first.view().await, second.view().await);
    prop_assert_eq!(
        first_work.usages(),
        second_work.usages(),
        "the charges do not depend on the list order of the store"
    );
    Ok(())
}

/// `fresh_ord_reissue_loses_first_round_place` (decision 43, the negative control C0 of
/// `HeadOnlyDisciplines.v`): a persistent waiter of `value` makes a persistent send of `value`
/// a divergent pair, and a younger datum is stored while the send is in flight. The re-entry
/// with the first-round order precedes the younger datum and fires the waiter again. The same
/// re-issue with a fresh order would rest behind the younger datum and fire nothing, so it would
/// lose the place of its first round.
#[test]
fn prop_fresh_ord_reissue_loses_first_round_place() {
    check(pure_config(), (0i64..4, 1i64..4), |(value, offset)| {
        tokio_runtime().block_on(fresh_ord_reissue_loses_first_round_place_case(
            value, offset,
        ))
    });
}

/// One case of `prop_fresh_ord_reissue_loses_first_round_place`.
async fn fresh_ord_reissue_loses_first_round_place_case(
    value: i64,
    offset: i64,
) -> Result<(), TestCaseError> {
    let younger = value + offset;
    let state = QueueState {
        counter: Some(1),
        records: Vec::new(),
        waiters: vec![WaiterSpec {
            ord: 0,
            pat: Pat::Lit(value),
            persist: true,
            peek: false,
            guard: None,
        }],
    };
    let send = QueueOp::Produce {
        value,
        persist: true,
    };
    let datum = QueueOp::Produce {
        value: younger,
        persist: false,
    };
    let mut fires = Vec::with_capacity(2);
    for fresh in [false, true] {
        let rig = AgentRig::of(&state).await;
        let first = rig
            .run_space_op(rig.space_op(&send, 1))
            .await
            .expect("the send commits");
        let reissue = first.reissue.expect("the divergent pair re-issues");
        prop_assert_eq!(kept_place(&reissue), Some((Side::Data, 1)));
        rig.run(&datum, 2).await.expect("the younger datum commits");
        let again = match (fresh, reissue.op) {
            (true, SpaceOp::Produce(produce)) => SpaceOp::Produce(ProduceOp {
                reissued_ord: None,
                ..produce
            }),
            (_, op) => op,
        };
        let outcome = rig.run_space_op(again).await.expect("the re-entry commits");
        let after = rig.view().await;
        let head = after.head_record().map(|record| (record.ord, record.value));
        match fresh {
            false => {
                prop_assert_eq!(head, Some((2, younger)));
                prop_assert!(outcome.reissue.is_some(), "the pair stays divergent");
            }
            true => {
                prop_assert_eq!(
                    head,
                    Some((2, younger)),
                    "the fresh send rests behind the younger datum"
                );
                prop_assert!(after
                    .records
                    .iter()
                    .any(|record| record.value == value && record.ord == 3));
                prop_assert_eq!(&outcome.reissue, &None);
            }
        }
        fires.push(outcome.fires.len());
    }
    prop_assert_eq!(
        fires,
        vec![1, 0],
        "only the kept order fires the waiter again"
    );
    Ok(())
}

/// `IntroducedOnce` (decision 43): a persistent operation that re-issues stages one record in
/// every round, so its events on the data channel share one identity, and the deploy has one
/// introduction row for that identity, as the host has for a persistent produce that it makes
/// again. The element then rests at the order of its first round.
#[test]
fn prop_queue_persistent_reissue_charged_once() {
    check(
        runtime_config(),
        (
            FIRE_ROUND_CAP - 1..2 * FIRE_ROUND_CAP + 2,
            any::<bool>(),
            vec(any::<u8>(), 4),
        ),
        |(partners, consume_side, seed)| {
            tokio_runtime().block_on(queue_persistent_reissue_charged_once_case(
                partners,
                consume_side,
                seed,
            ))
        },
    );
}

/// One case of `prop_queue_persistent_reissue_charged_once`.
async fn queue_persistent_reissue_charged_once_case(
    partners: usize,
    consume_side: bool,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let space = node.create_space(&[&seed[..], b"-space"].concat()).await;
    let names = vec![space_name(
        space,
        &format!("charged-once-{}", hex::encode(&seed)),
    )];
    let partner_ops: Vec<SlotOp> = (0..partners)
        .map(|index| match consume_side {
            false => SlotOp::Receive {
                slot: 0,
                pat: Pat::Any,
                arrow: Arrow::Consume,
                label: format!("w{index}"),
            },
            true => SlotOp::Send {
                slot: 0,
                value: 1,
                persistent: false,
            },
        })
        .collect();
    let parked = node
        .deploy(&ops_deploy(
            &partner_ops,
            &names,
            OUT,
            &[&seed[..], b"-partners"].concat(),
        ))
        .await;
    prop_assert!(
        parked.result.errors.is_empty(),
        "{:?}",
        parked.result.errors
    );
    let first = node.queue(space, &names[0]).await.next_ord();
    let op = match consume_side {
        false => SlotOp::Send {
            slot: 0,
            value: 1,
            persistent: true,
        },
        true => SlotOp::Receive {
            slot: 0,
            pat: Pat::Any,
            arrow: Arrow::Persistent,
            label: "r".to_string(),
        },
    };
    let deployed = node
        .deploy(&ops_deploy(
            std::slice::from_ref(&op),
            &names,
            OUT,
            &[&seed[..], b"-op"].concat(),
        ))
        .await;
    prop_assert!(
        deployed.result.errors.is_empty(),
        "{:?}",
        deployed.result.errors
    );
    let key = CollectionKey::of_channel(&names[0]).expect("a name has a key");
    let data = stable_hash(&gprivate(namespace::data_channel(space, &key)));
    let (kind, identities): (AuthorityByteEventKind, BTreeSet<[u8; 32]>) = match consume_side {
        false => (
            AuthorityByteEventKind::ProduceIntroduction,
            deployed
                .log
                .iter()
                .filter_map(|event| match event {
                    Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data => {
                        Some(produce_introduction_identity(produce))
                    }
                    _ => None,
                })
                .collect(),
        ),
        true => (
            AuthorityByteEventKind::ConsumeIntroduction,
            deployed
                .log
                .iter()
                .filter_map(|event| match event {
                    Event::IoEvent(IOEvent::Consume(consume))
                        if consume.channel_hashes == std::slice::from_ref(&data) =>
                    {
                        Some(consume_introduction_identity(consume))
                    }
                    _ => None,
                })
                .collect(),
        ),
    };
    prop_assert_eq!(identities.len(), 1, "every round stages one record");
    let identity = *identities.first().expect("one identity");
    let rows = deployed
        .result
        .byte_observations
        .rows
        .iter()
        .filter(|row| row.kind == kind && row.event_id == identity)
        .count();
    prop_assert_eq!(rows, 1, "one introduction row for the one identity");
    let after = node.queue(space, &names[0]).await;
    let rests = match consume_side {
        false => after
            .records
            .iter()
            .any(|record| record.ord == first && record.persist),
        true => after
            .waiters
            .iter()
            .any(|waiter| waiter.ord == first && waiter.persist),
    };
    prop_assert!(
        rests,
        "the element rests at the order {} of its first round: {:?}",
        first,
        after
    );
    Ok(())
}

/// A host-work budget that no queue operation of these tests exhausts.
fn unlimited_host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX / 4)))
}

/// The dimensions that a space job charges (decision 42).
const AGENT_DIMENSIONS: [HostWorkDimension; 3] = [
    HostWorkDimension::VerificationOperations,
    HostWorkDimension::VerificationBytes,
    HostWorkDimension::SearchStateBytes,
];

/// `agent_reservation_cut` (decision 42): a queue operation reserves its work before it makes
/// it. With its exact usage as the limits it commits. With one unit less in any charged
/// dimension it fails with `HostWorkRejected` before the commit, so it logs no event and leaves
/// the collection unchanged.
#[test]
fn prop_agent_reservation_cut_rejects_without_effect() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op()),
        |(state, op)| tokio_runtime().block_on(agent_reservation_cut_case(state, op)),
    );
}

/// One case of `prop_agent_reservation_cut_rejects_without_effect`.
async fn agent_reservation_cut_case(state: QueueState, op: QueueOp) -> Result<(), TestCaseError> {
    let order: Vec<usize> = (0..state.records.len() + state.waiters.len()).collect();
    let measured = unlimited_host_work();
    let rig = AgentRig::new(&state, &order, Some(measured.clone())).await;
    let expected = rig.run(&op, 1).await;
    let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
    for dimension in AGENT_DIMENSIONS {
        exact.set(
            dimension,
            HostWorkLimit::new(measured.usage(dimension).get()),
        );
    }
    let rig = AgentRig::new(&state, &order, Some(HostWorkBudget::new(exact))).await;
    prop_assert_eq!(
        &rig.run(&op, 1).await,
        &expected,
        "the exact usage suffices"
    );
    for dimension in AGENT_DIMENSIONS {
        let usage = measured.usage(dimension).get();
        prop_assert!(usage > 0, "every operation charges {dimension}");
        let mut cut = exact;
        cut.set(dimension, HostWorkLimit::new(usage - 1));
        let rig = AgentRig::new(&state, &order, Some(HostWorkBudget::new(cut))).await;
        let before = rig.view().await;
        prop_assert_eq!(
            rig.run(&op, 1).await,
            Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
            "{}",
            dimension
        );
        prop_assert!(rig.host.take_event_log().await.is_empty(), "{dimension}");
        prop_assert_eq!(rig.view().await, before, "{}", dimension);
    }
    Ok(())
}

/// `agent_charge_independent_of_unread_payloads` (decision 42): a queue operation reads the
/// envelope of each entry of its side and copies only the heads that it meets. So the payloads
/// of the records and the bodies of the waiters that it never meets do not change its charges.
/// An operation meets the heads that it fires and at most one more: the head that blocks it.
#[test]
fn prop_agent_charge_independent_of_unread_payloads() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op(), 1usize..4096),
        |(state, op, bytes)| {
            tokio_runtime().block_on(agent_charge_independent_of_unread_payloads_case(
                state, op, bytes,
            ))
        },
    );
}

/// One case of `prop_agent_charge_independent_of_unread_payloads`.
async fn agent_charge_independent_of_unread_payloads_case(
    state: QueueState,
    op: QueueOp,
    bytes: usize,
) -> Result<(), TestCaseError> {
    let order: Vec<usize> = (0..state.records.len() + state.waiters.len()).collect();
    let plain_work = unlimited_host_work();
    let plain = AgentRig::new(&state, &order, Some(plain_work.clone())).await;
    let plain_outcome = plain.run(&op, 1).await;
    let met = match &plain_outcome {
        Ok(outcome) => outcome.fires.len() + 1,
        Err(_) => 1,
    };
    let (read_side, met_ords): (Side, Vec<i64>) = match op {
        QueueOp::Produce { .. } => (
            Side::Waiters,
            sorted_ords(state.waiters.iter().map(|waiter| waiter.ord), met),
        ),
        QueueOp::Consume { .. } => (
            Side::Data,
            sorted_ords(state.records.iter().map(|record| record.ord), met),
        ),
    };
    let pad = |side: Side, ord: i64| match side == read_side && met_ords.contains(&ord) {
        true => 0,
        false => bytes,
    };
    let padded_work = unlimited_host_work();
    let padded = AgentRig::padded(&state, &order, Some(padded_work.clone()), &pad).await;
    let padded_outcome = padded.run(&op, 1).await;
    prop_assert_eq!(&padded_outcome, &plain_outcome);
    prop_assert_eq!(
        padded_work.usages(),
        plain_work.usages(),
        "unread payloads of {} bytes changed the charges",
        bytes
    );
    Ok(())
}

/// The `count` least orders of `ords`.
fn sorted_ords(ords: impl Iterator<Item = i64>, count: usize) -> Vec<i64> {
    let mut ords: Vec<i64> = ords.collect();
    ords.sort_unstable();
    ords.truncate(count);
    ords
}

/// `p4_scan_covered_only_if_free` (decision 42, negative control): every queue operation
/// reserves work of its own in each charged dimension, beyond the reservations of the matcher.
/// The P4 meter charged only the reads of the matcher, so it left that work unreserved.
#[test]
fn prop_agent_reserves_its_own_work_beyond_the_matcher() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op()),
        |(state, op)| tokio_runtime().block_on(agent_reserves_its_own_work_case(state, op)),
    );
}

/// One case of `prop_agent_reserves_its_own_work_beyond_the_matcher`.
async fn agent_reserves_its_own_work_case(
    state: QueueState,
    op: QueueOp,
) -> Result<(), TestCaseError> {
    let order: Vec<usize> = (0..state.records.len() + state.waiters.len()).collect();
    let host_work = unlimited_host_work();
    let rig = AgentRig::new(&state, &order, Some(host_work.clone())).await;
    rig.run(&op, 1).await.expect("the operation commits");
    for (dimension, matcher) in AGENT_DIMENSIONS.into_iter().zip(rig.matcher_reserved()) {
        let usage = host_work.usage(dimension).get();
        prop_assert!(
            usage > matcher,
            "{}: the agent reserved nothing beyond the matcher ({} of {})",
            dimension,
            matcher,
            usage
        );
    }
    Ok(())
}

// ─── Peek re-store (decision 45) ─────────────────────────────────────────────────────────────

/// An operation that peeks often: sends, plain receives that match every value, and peeks that
/// match every value or one value.
fn arb_peek_op() -> BoxedStrategy<QueueOp> {
    prop_oneof![
        3 => (0i64..4, prop_oneof![3 => Just(false), 1 => Just(true)])
            .prop_map(|(value, persist)| QueueOp::Produce { value, persist }),
        1 => Just(QueueOp::Consume { pat: Pat::Any, persist: false, peek: false, guard: None }),
        3 => Just(QueueOp::Consume { pat: Pat::Any, persist: false, peek: true, guard: None }),
        1 => (0i64..4).prop_map(|value| QueueOp::Consume {
            pat: Pat::Lit(value),
            persist: false,
            peek: true,
            guard: None,
        }),
    ]
    .boxed()
}

/// The events of `log` on the data channel `data`: its produces, its consumes and the COMM
/// events of its consumes.
fn data_events<'a>(log: &'a [Event], data: &Blake2b256Hash) -> Vec<&'a Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == *data,
            Event::IoEvent(IOEvent::Consume(consume)) => {
                consume.channel_hashes == std::slice::from_ref(data)
            }
            Event::Comm(comm) => comm.consume.channel_hashes == std::slice::from_ref(data),
        })
        .collect()
}

/// `queue_restore_realizes_plan` (decision 45, Rule E): over a run of operations that peeks
/// often, the staged operations of the agent realize the logical plan. A peek that fires with
/// the head record leaves the records of the collection as they were. Its receipt keeps the
/// persist flag of the record. For a record that is not persistent, the COMM of the peek
/// removes the record, and the next event on the data channel produces the same record again:
/// the re-store, with the identity of the stored record. A peek of a persistent record removes
/// nothing and produces nothing.
#[test]
fn prop_queue_restore_realizes_plan() {
    check(
        pure_config(),
        (arb_queue_state(5), vec(arb_peek_op(), 2..12)),
        |(state, ops)| tokio_runtime().block_on(queue_restore_realizes_plan_case(state, ops)),
    );
}

/// One case of `prop_queue_restore_realizes_plan`.
async fn queue_restore_realizes_plan_case(
    state: QueueState,
    ops: Vec<QueueOp>,
) -> Result<(), TestCaseError> {
    let mut state = state;
    state.waiters.clear();
    let rig = AgentRig::of(&state).await;
    let data = stable_hash(rig.channels.data());
    for (salt, op) in (1u64..).zip(&ops) {
        let before = rig.view().await;
        let stored: Vec<Datum<ListParWithRandom>> = rig.host.get_data(rig.channels.data()).await;
        let outcome = rig.run(op, salt).await.expect("the operation commits");
        let log = rig.host.take_event_log().await;
        let after = rig.view().await;
        let QueueOp::Consume {
            pat, peek: true, ..
        } = *op
        else {
            continue;
        };
        let head = match before.waiters.is_empty() {
            true => before
                .head_record()
                .copied()
                .filter(|head| pat.fits(head.value)),
            false => None,
        };
        let Some(head) = head else {
            prop_assert!(
                outcome.fires.is_empty(),
                "a peek without a head match fires nothing"
            );
            prop_assert_eq!(&after.records, &before.records);
            continue;
        };
        prop_assert_eq!(
            &after.records,
            &before.records,
            "a peek keeps the records of the plan"
        );
        let [fire] = outcome.fires.as_slice() else {
            panic!("a peek with a matching head fires once");
        };
        let [result] = fire.results.as_slice() else {
            panic!("a queue fire binds one record");
        };
        prop_assert_eq!(
            result.persistent,
            head.persist,
            "the receipt keeps the persist flag of the record"
        );
        let peeked = stored
            .iter()
            .find(|datum| datum_ord(&datum.a) == Ok(head.ord))
            .expect("the head record is stored");
        let events = data_events(&log, &data);
        let [Event::IoEvent(IOEvent::Consume(_)), Event::Comm(comm), rest @ ..] = events.as_slice()
        else {
            panic!("a peek fire logs its consume and its COMM: {events:?}");
        };
        prop_assert_eq!(&comm.peeks, &BTreeSet::from([0]));
        prop_assert_eq!(
            comm.produces.as_slice(),
            std::slice::from_ref(&peeked.source)
        );
        match (head.persist, rest) {
            (true, []) => {}
            (false, [Event::IoEvent(IOEvent::Produce(restored))]) => prop_assert_eq!(
                restored,
                &peeked.source,
                "the re-store has the identity of the record"
            ),
            (persist, other) => {
                return Err(TestCaseError::fail(format!(
                    "a peek of a record with persist = {persist} logged {other:?} after its COMM"
                )));
            }
        }
        let copies = rig
            .host
            .get_data(rig.channels.data())
            .await
            .iter()
            .filter(|datum| datum.source == peeked.source && datum.a == peeked.a)
            .count();
        prop_assert_eq!(copies, 1, "the store holds the peeked record once");
    }
    Ok(())
}

// ─── Runtime harness ─────────────────────────────────────────────────────────────────────────

type RhoHistory = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync
            + 'static,
    >,
>;

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-props-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-props-payer";
const DEPLOY_PHLO: i64 = 100_000_000;
const DEPLOY_LABEL: &str = "rrs-p4-props";

/// The public channel of the deliveries of the receives of a test.
const OUT: &str = "rrs-p4-out";

/// The public channel of the deliveries of the setup deploys.
const SETUP_OUT: &str = "rrs-p4-setup-out";

/// The URI of the collection slot `slot` in the normalizer environment.
fn slot_uri(slot: usize) -> String { format!("rho:rrs:p4:q{slot}") }

/// The URI of a space handle or a Reified id in the normalizer environment.
const HANDLE_URI: &str = "rho:rrs:p4:handle";
const ID_URI: &str = "rho:rrs:p4:id";
const MESSAGE_URI: &str = "rho:rrs:p4:message";

fn injections(pairs: &[(String, Par)]) -> HashMap<String, Par> { pairs.iter().cloned().collect() }

/// A deploy of a test: its program, the ids that it names and its random seed.
#[derive(Clone, Debug)]
struct DeploySpec {
    program: String,
    injected: Vec<(String, Par)>,
    seed: Vec<u8>,
}

impl DeploySpec {
    fn new(program: String, injected: Vec<(String, Par)>, seed: &[u8]) -> DeploySpec {
        DeploySpec {
            program,
            injected,
            seed: seed.to_vec(),
        }
    }
}

/// The result of a deploy, as casper processes it: the evaluation and the event log.
struct Deployed {
    result: EvaluateResult,
    log: Vec<Event>,
}

/// The `<Code>` of each `reified:<Code>:` error of an evaluation, and `other` for every other
/// error.
fn error_codes(errors: &[InterpreterError]) -> Vec<String> {
    errors
        .iter()
        .map(|error| match error {
            InterpreterError::ReduceError(message) => message
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
                .map(str::to_string)
                .unwrap_or_else(|| format!("other: {message}")),
            other => format!("other: {other:?}"),
        })
        .collect()
}

impl Deployed {
    fn codes(&self) -> Vec<String> { error_codes(&self.result.errors) }
}

/// A play runtime and a replay runtime over one in-memory store, as `create_runtimes` makes
/// them, with a funded deploy signature.
struct Node {
    manager: InMemoryStoreManager,
    play: RhoRuntimeImpl,
    replay: RhoRuntimeImpl,
    history: RhoHistory,
}

impl Node {
    async fn new() -> Node {
        let mut manager = InMemoryStoreManager::new();
        let (play, replay, history) = Node::runtimes(&mut manager).await;
        Node {
            manager,
            play,
            replay,
            history,
        }
    }

    async fn runtimes(
        manager: &mut InMemoryStoreManager,
    ) -> (RhoRuntimeImpl, RhoRuntimeImpl, RhoHistory) {
        let stores = manager
            .r_space_stores()
            .await
            .expect("the in-memory RSpace stores");
        let (play, replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
        for runtime in [&play, &replay] {
            runtime.cost.set_deploy_signature_funded(
                DEPLOY_SIGNATURE,
                Sig::Ground(FUNDING_SIGNATURE.to_vec()),
            );
        }
        (play, replay, history)
    }

    /// New runtimes over the same stores, with empty hot stores: a process restart.
    async fn restart(&mut self) {
        let (play, replay, history) = Node::runtimes(&mut self.manager).await;
        self.play = play;
        self.replay = replay;
        self.history = history;
    }

    /// Evaluates a deploy on the play runtime as casper processes a user deploy. It takes a soft
    /// checkpoint, evaluates, takes the event log, and reverts when the deploy failed.
    async fn deploy(&mut self, spec: &DeploySpec) -> Deployed {
        let checkpoint = self.play.create_soft_checkpoint().await;
        let result = self
            .play
            .evaluate(
                &spec.program,
                Cost::create(DEPLOY_PHLO, DEPLOY_LABEL),
                injections(&spec.injected),
                Blake2b512Random::create_from_bytes(&spec.seed),
            )
            .await
            .expect("the evaluation returns a result");
        let log = self.play.take_event_log().await;
        if !result.errors.is_empty() {
            self.play.revert_to_soft_checkpoint(checkpoint).await;
        }
        Deployed { result, log }
    }

    /// Replays a deploy from `root` with its play log on the replay runtime.
    async fn replay(
        &mut self,
        root: &Blake2b256Hash,
        spec: &DeploySpec,
        played: &Deployed,
    ) -> EvaluateResult {
        self.replay
            .reset(root)
            .await
            .expect("reset the replay runtime");
        self.replay
            .rig(played.log.clone())
            .await
            .expect("rig the replay runtime");
        let checkpoint = self.replay.create_soft_checkpoint().await;
        let result = self
            .replay
            .evaluate(
                &spec.program,
                Cost::create(DEPLOY_PHLO, DEPLOY_LABEL),
                injections(&spec.injected),
                Blake2b512Random::create_from_bytes(&spec.seed),
            )
            .await
            .expect("the replay evaluation returns a result");
        self.replay
            .check_replay_data()
            .await
            .expect("the replay consumes the complete play log");
        if !result.errors.is_empty() {
            self.replay.revert_to_soft_checkpoint(checkpoint).await;
        }
        result
    }

    async fn checkpoint(&mut self) -> Blake2b256Hash { self.play.create_checkpoint().await.root }

    async fn reset(&mut self, root: &Blake2b256Hash) {
        self.play.reset(root).await.expect("reset the play runtime");
    }

    /// Creates a `hashmap:queue` space through the factory URN and returns it.
    async fn create_space(&mut self, seed: &[u8]) -> NativeSpace {
        let out = format!("rrs-p4-handle-{}", hex::encode(seed));
        let program = format!(
            "new factory(`rho:space:hashmap:queue`) in {{ \
               for (space <- factory!?(\"default\")) {{ @\"{out}\"!(*space) }} \
             }}"
        );
        let deployed = self
            .deploy(&DeploySpec::new(program, Vec::new(), seed))
            .await;
        assert!(
            deployed.result.errors.is_empty(),
            "the factory call failed: {:?}",
            deployed.result.errors
        );
        let handles = self.values(&out).await;
        let [handle] = handles.as_slice() else {
            panic!("the factory replied {} handles", handles.len());
        };
        match classify_space_ref(handle) {
            Ok(SpaceRef::Native(space)) => space,
            other => panic!("the factory reply is not a native handle: {other:?}"),
        }
    }

    /// The single par of each datum on the public channel `channel`.
    async fn values(&self, channel: &str) -> Vec<Par> {
        self.play
            .get_data(&gstring(channel))
            .await
            .into_iter()
            .map(|datum| match <[Par; 1]>::try_from(datum.a.pars) {
                Ok([value]) => value,
                Err(pars) => panic!("a datum on @{channel:?} has {} pars", pars.len()),
            })
            .collect()
    }

    /// The deliveries on `channel`: the pairs `[label, value]` that the receive bodies sent.
    async fn deliveries(&self, channel: &str) -> Vec<(String, i64)> {
        let mut deliveries: Vec<(String, i64)> = self
            .values(channel)
            .await
            .iter()
            .map(|value| match value.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
                }] => match ps.as_slice() {
                    [label, value] => (
                        string_of(label).expect("a delivery label").to_string(),
                        number_of(std::slice::from_ref(value)),
                    ),
                    other => panic!("a delivery has {} elements", other.len()),
                },
                other => panic!("a delivery is a list: {other:?}"),
            })
            .collect();
        deliveries.sort();
        deliveries
    }

    /// The collection of `name` in `space` as the play runtime holds it.
    async fn queue(&self, space: NativeSpace, name: &Par) -> QueueState {
        let key = CollectionKey::of_channel(name).expect("a name has a key");
        let data = gprivate(namespace::data_channel(space, &key));
        let mut records: Vec<RecordSpec> = self
            .play
            .get_data(&data)
            .await
            .into_iter()
            .map(|datum| {
                let record = unwrap_datum(&datum.a).expect("a stored record has an envelope");
                RecordSpec {
                    ord: record.ord,
                    value: number_of(&record.datum.pars),
                    persist: datum.persist,
                }
            })
            .collect();
        records.sort_by_key(|record| record.ord);
        let mut waiters: Vec<WaiterSpec> = self
            .play
            .get_continuations(vec![data])
            .await
            .into_iter()
            .map(|waiter| {
                let parked = unwrap_continuation(&waiter.continuation)
                    .expect("a parked waiter has an envelope");
                let [pattern] = waiter.patterns.as_slice() else {
                    panic!("a waiter of a queue has one bind");
                };
                WaiterSpec {
                    ord: parked.ord,
                    pat: Pat::of_bind_pattern(pattern),
                    persist: waiter.persist,
                    peek: !waiter.peeks.is_empty(),
                    guard: guard_of(&parked.continuation.guard),
                }
            })
            .collect();
        waiters.sort_by_key(|waiter| waiter.ord);
        let counter = gprivate(namespace::collection_counter(space, &key));
        let counter = match self.play.get_data(&counter).await.as_slice() {
            [] => None,
            [datum] => Some(counter_value(&datum.a).expect("a counter record")),
            more => panic!("a collection has {} counter records", more.len()),
        };
        QueueState {
            counter,
            records,
            waiters,
        }
    }
}

/// A name of `space`, as the handle method `gensym` makes it for a call with the random state of
/// `label`.
fn space_name(space: NativeSpace, label: &str) -> Par {
    HashMapOuter
        .gensym(space, &random_of(&[label.as_bytes()]))
        .expect("gensym makes a name")
}

/// The arrow of a receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrow {
    Consume,
    Peek,
    Persistent,
}

impl Arrow {
    fn symbol(self) -> &'static str {
        match self {
            Arrow::Consume => "<-",
            Arrow::Peek => "<<-",
            Arrow::Persistent => "<=",
        }
    }
}

/// One operation of a deploy on the collection slot `slot`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SlotOp {
    Send {
        slot: usize,
        value: i64,
        persistent: bool,
    },
    Receive {
        slot: usize,
        pat: Pat,
        arrow: Arrow,
        label: String,
    },
}

impl SlotOp {
    fn slot(&self) -> usize {
        match self {
            SlotOp::Send { slot, .. } | SlotOp::Receive { slot, .. } => *slot,
        }
    }

    fn process(&self, out: &str) -> String {
        match self {
            SlotOp::Send {
                slot,
                value,
                persistent,
            } => {
                let bang = if *persistent { "!!" } else { "!" };
                format!("q{slot}{bang}({value})")
            }
            SlotOp::Receive {
                slot,
                pat,
                arrow,
                label,
            } => {
                let (pattern, value) = match pat {
                    Pat::Any => ("x".to_string(), "x".to_string()),
                    Pat::Lit(value) => (value.to_string(), value.to_string()),
                };
                format!(
                    "for (@{pattern} {} q{slot}) {{ @\"{out}\"!([\"{label}\", {value}]) }}",
                    arrow.symbol()
                )
            }
        }
    }
}

/// The deploy that runs `ops` in parallel on the collections `names` (one name for each slot).
fn ops_deploy(ops: &[SlotOp], names: &[Par], out: &str, seed: &[u8]) -> DeploySpec {
    let slots: BTreeSet<usize> = ops.iter().map(SlotOp::slot).collect();
    let binders: Vec<String> = slots
        .iter()
        .map(|slot| format!("q{slot}(`{}`)", slot_uri(*slot)))
        .collect();
    let body: Vec<String> = ops.iter().map(|op| op.process(out)).collect();
    let program = match binders.is_empty() {
        true => body.join(" | "),
        false => format!("new {} in {{ {} }}", binders.join(", "), body.join(" | ")),
    };
    let injected = slots
        .iter()
        .map(|slot| (slot_uri(*slot), names[*slot].clone()))
        .collect();
    DeploySpec::new(program, injected, seed)
}

/// The index of an event log as `block_index.rs` makes it: the pre-state closures read the
/// history at `pre_state`, and no channel is a number channel.
fn pre_state_index(
    history: &RhoHistory,
    pre_state: &Blake2b256Hash,
    log: &[Event],
) -> EventLogIndex {
    let reader = history
        .get_history_reader(pre_state)
        .expect("the pre-state reader");
    let in_pre_state = |produce: &Produce| {
        reader
            .get_data(&produce.channel_hash)
            .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
    };
    let touches_join = |produce: &Produce| {
        reader
            .get_joins(&produce.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
    };
    EventLogIndex::new(log.to_vec(), in_pre_state, touches_join, BTreeMap::new())
}

/// The merged state of branches that the merger accepts, computed as `dag_merger.rs` does. The
/// state change of each branch is combined, turned into trie actions over the base, and applied.
fn merged_root(
    history: &RhoHistory,
    base: &Blake2b256Hash,
    branches: &[(&Blake2b256Hash, &EventLogIndex)],
) -> Blake2b256Hash {
    let mut combined = StateChange::empty();
    let mut writers: HashMap<Blake2b256Hash, usize> = HashMap::new();
    for (post, index) in branches {
        let change = StateChange::new(
            history
                .get_history_reader_struct(base)
                .expect("the base reader"),
            history
                .get_history_reader_struct(post)
                .expect("the post-state reader"),
            index,
        )
        .expect("the state change of a branch");
        for entry in change.datums_changes.iter() {
            if !entry.value().added.is_empty() {
                *writers.entry(entry.key().clone()).or_insert(0) += 1;
            }
        }
        combined = combined.combine(change);
    }
    let multi_writer: BTreeSet<Blake2b256Hash> = writers
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(channel, _)| channel)
        .collect();
    let reader = history.get_history_reader(base).expect("the base reader");
    let actions = compute_trie_actions(&combined, &reader, &BTreeMap::new(), |hash, changes, _| {
        if !changes.added.is_empty() {
            let base_data = reader.get_data(hash)?;
            if !base_data.is_empty() || multi_writer.contains(hash) {
                let base_binary = reader.get_data_proj_binary(hash)?;
                RholangMergingLogic::check_single_value_cell_not_overfilled(
                    hash,
                    &base_data,
                    &base_binary,
                    changes,
                )?;
            }
        }
        Ok(None)
    })
    .expect("the trie actions of the merge");
    history
        .reset(base)
        .expect("reset the history to the base")
        .do_checkpoint(actions)
        .root()
}

/// A space with collections at a checkpointed pre-state, for the merge properties.
struct MergeRig {
    node: Node,
    space: NativeSpace,
    names: Vec<Par>,
    root: Blake2b256Hash,
}

/// A branch from the pre-state: its deploys, their logs and its post-state.
struct BranchRun {
    deployed: Vec<Deployed>,
    post: Blake2b256Hash,
}

impl MergeRig {
    /// A space with `slots` collections. Slot `i` is opened with the records `opened[i]` when
    /// `opened[i]` is `Some`, and is fresh otherwise.
    async fn new(opened: &[Option<Vec<i64>>], seed: &[u8]) -> MergeRig {
        let mut node = Node::new().await;
        let space = node.create_space(&[seed, b"-space"].concat()).await;
        let names: Vec<Par> = (0..opened.len())
            .map(|slot| space_name(space, &format!("{}-{slot}", hex::encode(seed))))
            .collect();
        let mut setup = Vec::new();
        for (slot, records) in opened.iter().enumerate() {
            if let Some(records) = records {
                match records.is_empty() {
                    true => setup.extend([
                        SlotOp::Send {
                            slot,
                            value: 0,
                            persistent: false,
                        },
                        SlotOp::Receive {
                            slot,
                            pat: Pat::Any,
                            arrow: Arrow::Consume,
                            label: format!("setup-{slot}"),
                        },
                    ]),
                    false => setup.extend(records.iter().map(|value| SlotOp::Send {
                        slot,
                        value: *value,
                        persistent: false,
                    })),
                }
            }
        }
        for (index, op) in setup.iter().enumerate() {
            let deploy = ops_deploy(
                std::slice::from_ref(op),
                &names,
                SETUP_OUT,
                &[seed, b"-setup-", &index.to_be_bytes()].concat(),
            );
            let deployed = node.deploy(&deploy).await;
            assert!(
                deployed.result.errors.is_empty(),
                "a setup deploy failed: {:?}",
                deployed.result.errors
            );
        }
        let root = node.checkpoint().await;
        MergeRig {
            node,
            space,
            names,
            root,
        }
    }

    /// Runs `deploys` from the pre-state, in order, and checkpoints.
    async fn branch(&mut self, deploys: &[DeploySpec]) -> BranchRun {
        self.node.reset(&self.root).await;
        let mut deployed = Vec::with_capacity(deploys.len());
        for deploy in deploys {
            deployed.push(self.node.deploy(deploy).await);
        }
        let post = self.node.checkpoint().await;
        BranchRun { deployed, post }
    }

    fn index(&self, log: &[Event]) -> EventLogIndex {
        pre_state_index(&self.node.history, &self.root, log)
    }

    fn counter_hash(&self, slot: usize) -> Blake2b256Hash {
        let key = CollectionKey::of_channel(&self.names[slot]).expect("a name has a key");
        stable_hash(&gprivate(namespace::collection_counter(self.space, &key)))
    }

    fn space_counter_hash(&self) -> Blake2b256Hash {
        stable_hash(&gprivate(namespace::space_counter(self.space)))
    }
}

/// The conflicting channels of two branches of one deploy each.
fn branch_conflicts(
    rig: &MergeRig,
    first: &BranchRun,
    second: &BranchRun,
) -> (BTreeSet<Blake2b256Hash>, Option<String>) {
    let first = rig.index(&first.deployed[0].log);
    let second = rig.index(&second.deployed[0].log);
    (
        conflicts(&first, &second).0.into_iter().collect(),
        conflict_reason(&first, &second),
    )
}

fn arb_value() -> BoxedStrategy<i64> { (0i64..4).boxed() }

/// A write on the slot: a send, or a receive that consumes.
fn arb_write(slot: usize, label: &'static str) -> BoxedStrategy<SlotOp> {
    prop_oneof![
        arb_value().prop_map(move |value| SlotOp::Send {
            slot,
            value,
            persistent: false
        }),
        arb_pat().prop_map(move |pat| SlotOp::Receive {
            slot,
            pat,
            arrow: Arrow::Consume,
            label: label.to_string()
        }),
    ]
    .boxed()
}

/// A receive on the slot `slot` with the pattern `pat`, the arrow `arrow` and the label `label`.
fn receive_op(slot: usize, pat: Pat, arrow: Arrow, label: &str) -> SlotOp {
    SlotOp::Receive {
        slot,
        pat,
        arrow,
        label: label.to_string(),
    }
}

/// Any operation on the slot that terminates in the transaction: a send, a receive, a peek, or
/// a persistent receive that matches one number.
fn arb_slot_op(slot: usize, label: &'static str) -> BoxedStrategy<SlotOp> {
    prop_oneof![
        3 => arb_value().prop_map(move |value| SlotOp::Send { slot, value, persistent: false }),
        2 => arb_pat().prop_map(move |pat| receive_op(slot, pat, Arrow::Consume, label)),
        1 => arb_pat().prop_map(move |pat| receive_op(slot, pat, Arrow::Peek, label)),
        1 => arb_value().prop_map(move |value| SlotOp::Receive {
            slot,
            pat: Pat::Lit(value),
            arrow: Arrow::Persistent,
            label: label.to_string(),
        }),
    ]
    .boxed()
}

/// The records of an opened slot, or `None` for a fresh slot.
fn arb_opened() -> BoxedStrategy<Option<Vec<i64>>> { vec(arb_value(), 0..3).prop_map(Some).boxed() }

/// `ordered_writes_race_on_counter`: two branches from one pre-state each write one queue,
/// with an enqueue or a dequeue. They race on its counter `S(sid,k)` in check #1, whatever
/// else they do.
#[test]
fn prop_queue_writes_race_on_counter() {
    check(
        runtime_config(),
        (
            vec(arb_value(), 0..3),
            vec(prop_oneof![Just(None), arb_opened()], 2),
            (arb_write(0, "a"), vec(arb_slot_op(1, "a-other"), 0..2)),
            (arb_write(0, "b"), vec(arb_slot_op(2, "b-other"), 0..2)),
            vec(any::<u8>(), 4),
        ),
        |(records, others, first, second, seed)| {
            tokio_runtime().block_on(queue_writes_race_on_counter_case(
                records, others, first, second, seed,
            ))
        },
    );
}

/// One case of `prop_queue_writes_race_on_counter`.
async fn queue_writes_race_on_counter_case(
    records: Vec<i64>,
    others: Vec<Option<Vec<i64>>>,
    first: (SlotOp, Vec<SlotOp>),
    second: (SlotOp, Vec<SlotOp>),
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let opened = [Some(records), others[0].clone(), others[1].clone()];
    let mut rig = MergeRig::new(&opened, &seed).await;
    let mut first_ops = vec![first.0];
    first_ops.extend(first.1);
    let mut second_ops = vec![second.0];
    second_ops.extend(second.1);
    let first_run = rig
        .branch(&[ops_deploy(&first_ops, &rig.names, OUT, b"branch-a")])
        .await;
    let second_run = rig
        .branch(&[ops_deploy(&second_ops, &rig.names, OUT, b"branch-b")])
        .await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
        prop_assert!(run.deployed[0].result.mergeable.is_empty());
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(
        conflicting.contains(&rig.counter_hash(0)),
        "no race on the counter: {:?}",
        reason
    );
    prop_assert!(reason.is_some_and(|reason| reason.starts_with("racesForSameIOEvent")));
    Ok(())
}

/// `ordered_reads_race_on_counter` (A2, decision 31): a peek bumps the counter too, so a
/// branch that only reads a queue races with every branch that uses it.
#[test]
fn prop_queue_reads_race_on_counter() {
    check(
        runtime_config(),
        (
            vec(arb_value(), 0..3),
            arb_pat(),
            arb_slot_op(0, "b"),
            vec(any::<u8>(), 4),
        ),
        |(records, peek, other, seed)| {
            tokio_runtime().block_on(queue_reads_race_on_counter_case(records, peek, other, seed))
        },
    );
}

/// One case of `prop_queue_reads_race_on_counter`.
async fn queue_reads_race_on_counter_case(
    records: Vec<i64>,
    peek: Pat,
    other: SlotOp,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[Some(records)], &seed).await;
    let reader = SlotOp::Receive {
        slot: 0,
        pat: peek,
        arrow: Arrow::Peek,
        label: "a".to_string(),
    };
    let first_run = rig
        .branch(&[ops_deploy(&[reader], &rig.names, OUT, b"branch-a")])
        .await;
    let second_run = rig
        .branch(&[ops_deploy(&[other], &rig.names, OUT, b"branch-b")])
        .await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(
        conflicting.contains(&rig.counter_hash(0)),
        "no race on the counter: {:?}",
        reason
    );
    prop_assert!(reason.is_some_and(|reason| reason.starts_with("racesForSameIOEvent")));
    Ok(())
}

/// `first_ops_race_on_space_counter` (A25): two branches whose first operations open
/// collections of one space, one collection or two, race on the space counter `S(sid)`.
#[test]
fn prop_first_ops_race_on_space_counter() {
    check(
        runtime_config(),
        (
            any::<bool>(),
            arb_slot_op(0, "a"),
            arb_slot_op(1, "b"),
            vec(any::<u8>(), 4),
        ),
        |(same, first, second, seed)| {
            tokio_runtime().block_on(first_ops_race_on_space_counter_case(
                same, first, second, seed,
            ))
        },
    );
}

/// One case of `prop_first_ops_race_on_space_counter`.
async fn first_ops_race_on_space_counter_case(
    same: bool,
    first: SlotOp,
    second: SlotOp,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[None, None], &seed).await;
    let second = match (same, second) {
        (
            true,
            SlotOp::Send {
                value, persistent, ..
            },
        ) => SlotOp::Send {
            slot: 0,
            value,
            persistent,
        },
        (
            true,
            SlotOp::Receive {
                pat, arrow, label, ..
            },
        ) => SlotOp::Receive {
            slot: 0,
            pat,
            arrow,
            label,
        },
        (false, op) => op,
    };
    let first_run = rig
        .branch(&[ops_deploy(&[first], &rig.names, OUT, b"branch-a")])
        .await;
    let second_run = rig
        .branch(&[ops_deploy(&[second], &rig.names, OUT, b"branch-b")])
        .await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(
        conflicting.contains(&rig.space_counter_hash()),
        "no race on S(sid): {:?}",
        reason
    );
    Ok(())
}

/// `crossed_collections_race`: branch A uses the queues q1 and q2, and branch B uses q2 and
/// q1, in any order. The merge sees a race on both counters.
#[test]
fn prop_crossed_collections_race() {
    check(
        runtime_config(),
        (
            vec(vec(arb_value(), 0..3), 2),
            (arb_slot_op(0, "a1"), arb_slot_op(1, "a2"), any::<bool>()),
            (arb_slot_op(1, "b2"), arb_slot_op(0, "b1"), any::<bool>()),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, seed)| {
            tokio_runtime().block_on(crossed_collections_race_case(records, first, second, seed))
        },
    );
}

/// One case of `prop_crossed_collections_race`.
async fn crossed_collections_race_case(
    records: Vec<Vec<i64>>,
    first: (SlotOp, SlotOp, bool),
    second: (SlotOp, SlotOp, bool),
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[Some(records[0].clone()), Some(records[1].clone())], &seed).await;
    let order = |(left, right, swap): (SlotOp, SlotOp, bool)| match swap {
        true => vec![right, left],
        false => vec![left, right],
    };
    let first_run = rig
        .branch(&[ops_deploy(&order(first), &rig.names, OUT, b"branch-a")])
        .await;
    let second_run = rig
        .branch(&[ops_deploy(&order(second), &rig.names, OUT, b"branch-b")])
        .await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(
        conflicting.contains(&rig.counter_hash(0)),
        "no race on q1: {:?}",
        reason
    );
    prop_assert!(
        conflicting.contains(&rig.counter_hash(1)),
        "no race on q2: {:?}",
        reason
    );
    Ok(())
}

/// `same_block_ops_chain`: two deploys of one block that use one collection form one chain.
/// The second consumes the counter record that the first made, so it depends on the first.
/// A deploy on another opened collection depends on neither.
#[test]
fn prop_same_block_queue_ops_chain() {
    check(
        runtime_config(),
        (
            prop_oneof![Just(None), arb_opened()],
            vec(arb_value(), 0..3),
            arb_slot_op(0, "d1"),
            arb_slot_op(0, "d2"),
            arb_slot_op(1, "d3"),
            vec(any::<u8>(), 4),
        ),
        |(opened, other_records, first, second, third, seed)| {
            tokio_runtime().block_on(same_block_queue_ops_chain_case(
                opened,
                other_records,
                first,
                second,
                third,
                seed,
            ))
        },
    );
}

/// One case of `prop_same_block_queue_ops_chain`.
async fn same_block_queue_ops_chain_case(
    opened: Option<Vec<i64>>,
    other_records: Vec<i64>,
    first: SlotOp,
    second: SlotOp,
    third: SlotOp,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[opened, Some(other_records)], &seed).await;
    let block = rig
        .branch(&[
            ops_deploy(&[first], &rig.names, OUT, b"deploy-1"),
            ops_deploy(&[second], &rig.names, OUT, b"deploy-2"),
            ops_deploy(&[third], &rig.names, OUT, b"deploy-3"),
        ])
        .await;
    for deployed in &block.deployed {
        prop_assert!(
            deployed.result.errors.is_empty(),
            "{:?}",
            deployed.result.errors
        );
    }
    let indices: Vec<EventLogIndex> = block
        .deployed
        .iter()
        .map(|deployed| rig.index(&deployed.log))
        .collect();
    prop_assert!(
        depends(&indices[1], &indices[0]),
        "the second deploy does not chain to the first"
    );
    prop_assert!(!depends(&indices[0], &indices[1]));
    prop_assert!(!depends(&indices[2], &indices[0]));
    prop_assert!(!depends(&indices[2], &indices[1]));
    Ok(())
}

/// `unordered_writes_commute`: writes without a counter (stores on a bag, here the default
/// space) need no counter. Two branches of stores on shared channels do not conflict, and
/// the merged state is the state of both serial orders.
#[test]
fn prop_counter_free_writes_commute() {
    check(
        runtime_config(),
        (
            vec((0usize..2, "[a-c]{1,3}"), 1..4),
            vec((0usize..2, "[a-c]{1,3}"), 1..4),
            vec(any::<u8>(), 4),
        ),
        |(first, second, seed)| {
            tokio_runtime().block_on(counter_free_writes_commute_case(first, second, seed))
        },
    );
}

/// One case of `prop_counter_free_writes_commute`.
async fn counter_free_writes_commute_case(
    first: Vec<(usize, String)>,
    second: Vec<(usize, String)>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[], &seed).await;
    let stores = |writes: &[(usize, String)], tag: &str| -> String {
        writes
            .iter()
            .map(|(bag, value)| format!("@\"rrs-p4-bag-{bag}\"!(\"{tag}-{value}\")"))
            .collect::<Vec<String>>()
            .join(" | ")
    };
    let first = DeploySpec::new(stores(&first, "a"), Vec::new(), b"branch-a");
    let second = DeploySpec::new(stores(&second, "b"), Vec::new(), b"branch-b");
    let first_run = rig.branch(std::slice::from_ref(&first)).await;
    let second_run = rig.branch(std::slice::from_ref(&second)).await;
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(conflicting.is_empty(), "stores conflict: {:?}", reason);
    let first_index = rig.index(&first_run.deployed[0].log);
    let second_index = rig.index(&second_run.deployed[0].log);
    let merged = merged_root(&rig.node.history, &rig.root, &[
        (&first_run.post, &first_index),
        (&second_run.post, &second_index),
    ]);
    let serial = rig.branch(&[first.clone(), second.clone()]).await;
    let reversed = rig.branch(&[second, first]).await;
    prop_assert_eq!(&merged, &serial.post);
    prop_assert_eq!(&merged, &reversed.post);
    Ok(())
}

/// `merge_without_race_is_serializable`: two branches that use disjoint opened queues do
/// not conflict. The merged state is the state of both serial orders of the whole deploys.
#[test]
fn prop_nonracing_queue_branches_serializable() {
    check(
        runtime_config(),
        (
            vec(vec(arb_value(), 0..3), 2),
            vec(arb_slot_op(0, "a"), 1..3),
            vec(arb_slot_op(1, "b"), 1..3),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, seed)| {
            tokio_runtime().block_on(nonracing_queue_branches_serializable_case(
                records, first, second, seed,
            ))
        },
    );
}

/// One case of `prop_nonracing_queue_branches_serializable`.
async fn nonracing_queue_branches_serializable_case(
    records: Vec<Vec<i64>>,
    first: Vec<SlotOp>,
    second: Vec<SlotOp>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[Some(records[0].clone()), Some(records[1].clone())], &seed).await;
    let first = ops_deploy(&first, &rig.names, "rrs-p4-out-a", b"branch-a");
    let second = ops_deploy(&second, &rig.names, "rrs-p4-out-b", b"branch-b");
    let first_run = rig.branch(std::slice::from_ref(&first)).await;
    let second_run = rig.branch(std::slice::from_ref(&second)).await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(
        conflicting.is_empty(),
        "disjoint queues conflict: {:?}",
        reason
    );
    let first_index = rig.index(&first_run.deployed[0].log);
    let second_index = rig.index(&second_run.deployed[0].log);
    let merged = merged_root(&rig.node.history, &rig.root, &[
        (&first_run.post, &first_index),
        (&second_run.post, &second_index),
    ]);
    let serial = rig.branch(&[first.clone(), second.clone()]).await;
    let reversed = rig.branch(&[second, first]).await;
    prop_assert_eq!(&merged, &serial.post);
    prop_assert_eq!(&merged, &reversed.post);
    Ok(())
}

/// `counter_free_peek_admits_write_skew` (the reason for A2). The heads are h1 on q1 and h2
/// on q2. Branch A peeks q1 and dequeues q2, and branch B peeks q2 and dequeues q1. Without
/// the counter events of the peeks, checks #1 to #3 accept the merge. But no serial order
/// gives the merged outcome, in which both peeks saw the old heads. With the counters that
/// the peeks bump, the merger finds the race.
#[test]
fn prop_counter_free_peek_admits_write_skew() {
    check(
        runtime_config(),
        ((arb_value(), arb_value()), vec(any::<u8>(), 4)),
        |(heads, seed)| {
            tokio_runtime().block_on(counter_free_peek_admits_write_skew_case(heads, seed))
        },
    );
}

/// One case of `prop_counter_free_peek_admits_write_skew`.
async fn counter_free_peek_admits_write_skew_case(
    heads: (i64, i64),
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[Some(vec![heads.0]), Some(vec![heads.1])], &seed).await;
    let first = ops_deploy(
        &[
            SlotOp::Receive {
                slot: 0,
                pat: Pat::Any,
                arrow: Arrow::Peek,
                label: "a-peek".to_string(),
            },
            SlotOp::Receive {
                slot: 1,
                pat: Pat::Any,
                arrow: Arrow::Consume,
                label: "a-take".to_string(),
            },
        ],
        &rig.names,
        OUT,
        b"branch-a",
    );
    let second = ops_deploy(
        &[
            SlotOp::Receive {
                slot: 1,
                pat: Pat::Any,
                arrow: Arrow::Peek,
                label: "b-peek".to_string(),
            },
            SlotOp::Receive {
                slot: 0,
                pat: Pat::Any,
                arrow: Arrow::Consume,
                label: "b-take".to_string(),
            },
        ],
        &rig.names,
        OUT,
        b"branch-b",
    );
    let first_run = rig.branch(std::slice::from_ref(&first)).await;
    let first_out = rig.node.deliveries(OUT).await;
    let second_run = rig.branch(std::slice::from_ref(&second)).await;
    let second_out = rig.node.deliveries(OUT).await;
    let mut merged_out = first_out.clone();
    merged_out.extend(second_out.clone());
    merged_out.sort();
    prop_assert!(merged_out.iter().any(|(label, _)| label == "a-peek"));
    prop_assert!(merged_out.iter().any(|(label, _)| label == "b-peek"));
    let (with_counters, _) = branch_conflicts(&rig, &first_run, &second_run);
    prop_assert!(with_counters.contains(&rig.counter_hash(0)));
    prop_assert!(with_counters.contains(&rig.counter_hash(1)));
    let without = |log: &[Event], peeked: &Blake2b256Hash| -> Vec<Event> {
        log.iter()
            .filter(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash != *peeked,
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    !consume.channel_hashes.contains(peeked)
                }
                Event::Comm(comm) => !comm.consume.channel_hashes.contains(peeked),
            })
            .cloned()
            .collect()
    };
    let counter_free_first = rig.index(&without(&first_run.deployed[0].log, &rig.counter_hash(0)));
    let counter_free_second =
        rig.index(&without(&second_run.deployed[0].log, &rig.counter_hash(1)));
    prop_assert!(
        conflicts(&counter_free_first, &counter_free_second)
            .0
            .is_empty(),
        "the counter-free logs conflict: {:?}",
        conflict_reason(&counter_free_first, &counter_free_second)
    );
    let serial = rig.branch(&[first.clone(), second.clone()]).await;
    prop_assert!(serial
        .deployed
        .iter()
        .all(|deployed| deployed.result.errors.is_empty()));
    let serial_out = rig.node.deliveries(OUT).await;
    let reversed = rig.branch(&[second, first]).await;
    prop_assert!(reversed
        .deployed
        .iter()
        .all(|deployed| deployed.result.errors.is_empty()));
    let reversed_out = rig.node.deliveries(OUT).await;
    prop_assert_ne!(&merged_out, &serial_out);
    prop_assert_ne!(&merged_out, &reversed_out);
    Ok(())
}

// ─── Refs (SpaceRefs.v) ──────────────────────────────────────────────────────────────────────

fn other_unforgeable(instance: UnfInstance) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(instance),
    }])
}

fn bundle(body: Par, write: bool, read: bool) -> Par { new_bundle_par(body, write, read) }

/// True when a `Par` has no component. The bit sets do not count.
fn is_empty_par(par: &Par) -> bool {
    par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.exprs.is_empty()
        && par.matches.is_empty()
        && par.unforgeables.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_signed_terms.is_empty()
        && par.cost_stacks.is_empty()
}

/// The class of a Reified id in the routing table of P4 design C.1.
fn id_class(spec: &IdSpec) -> ChannelClass {
    match spec {
        IdSpec::Op(_) => ChannelClass::System,
        IdSpec::Config(_) | IdSpec::Handle(_) | IdSpec::Matcher(..) | IdSpec::Body(..) => {
            ChannelClass::DefaultObject
        }
        IdSpec::Data(..)
        | IdSpec::PathIndex(..)
        | IdSpec::Allocator(_)
        | IdSpec::SpaceCounter(_)
        | IdSpec::CollectionCounter(..) => ChannelClass::Storage,
        IdSpec::Name(space, _) => ChannelClass::SpaceName(*space),
        IdSpec::Reserved(..) => ChannelClass::Unforgeable,
    }
}

/// The deploy id with the signature `sig`.
fn deploy_id(sig: Vec<u8>) -> Par {
    other_unforgeable(UnfInstance::GDeployIdBody(GDeployId { sig }))
}

/// The deployer id with the public key `public_key`.
fn deployer_id(public_key: Vec<u8>) -> Par {
    other_unforgeable(UnfInstance::GDeployerIdBody(GDeployerId { public_key }))
}

/// The agent tag: the op channel `AGENT_TAG` as an unforgeable name.
fn agent_tag() -> Par { gprivate(namespace::op_channel(AGENT_TAG)) }

/// An ordinary channel: a value that a program names without Reified syntax.
fn arb_ordinary_channel() -> BoxedStrategy<Par> {
    let bundled = (arb_ground_par(), any::<bool>(), any::<bool>())
        .prop_map(|(body, write, read)| bundle(body, write, read));
    prop_oneof![
        4 => arb_ground_par(),
        1 => any::<u8>().prop_map(byte_name),
        1 => vec(any::<u8>(), 1..8).prop_map(deploy_id),
        1 => vec(any::<u8>(), 1..8).prop_map(deployer_id),
        1 => bundled,
    ]
    .boxed()
}

/// A channel with the class that the routing table gives it.
fn arb_channel_case() -> BoxedStrategy<(Par, ChannelClass)> {
    let pair = (any::<u8>(), any::<u8>())
        .prop_map(|(first, second)| (gprivate(vec![first, second]), ChannelClass::System));
    let not_tag = any::<u8>().prop_filter("not the tag", |first| *first != REIFIED_TAG);
    let foreign = (not_tag, vec(any::<u8>(), 2..40)).prop_map(|(first, rest)| {
        let mut id = Vec::with_capacity(1 + rest.len());
        id.push(first);
        id.extend(rest);
        (gprivate(id), ChannelClass::Unforgeable)
    });
    let bundled_id =
        (arb_id_spec(), any::<bool>(), any::<bool>()).prop_map(|(spec, write, read)| {
            (
                bundle(gprivate(spec.encode()), write, read),
                ChannelClass::Forged,
            )
        });
    prop_oneof![
        2 => any::<u8>().prop_map(|byte| (byte_name(byte), ChannelClass::System)),
        2 => pair,
        6 => arb_id_spec().prop_map(|spec| (gprivate(spec.encode()), id_class(&spec))),
        2 => foreign,
        1 => vec(any::<u8>(), 1..8).prop_map(|sig| (deploy_id(sig), ChannelClass::Unforgeable)),
        2 => (arb_ground_par(), arb_ground_par()).prop_map(|(agent, raw)| {
            (tuple(vec![agent_tag(), agent, raw]), ChannelClass::AgentTagged)
        }),
        1 => (arb_ground_par(), 0u8..3).prop_map(|(value, shape)| {
            let near = match shape {
                0 => tuple(vec![agent_tag(), value]),
                1 => tuple(vec![agent_tag(), value.clone(), value.clone(), value]),
                _ => tuple(vec![gprivate(namespace::op_channel(HANDLE_OP)), value.clone(), value]),
            };
            (near, ChannelClass::Forged)
        }),
        3 => arb_ground_par().prop_map(|value| {
            let class = match namespace::sole_unforgeable(&value) {
                Some(_) => ChannelClass::Unforgeable,
                None => ChannelClass::Forged,
            };
            (value, class)
        }),
        2 => bundled_id,
        1 => arb_id_spec().prop_map(|spec| {
            let mut channel = gprivate(spec.encode());
            channel.exprs.push(Expr { expr_instance: Some(ExprInstance::GInt(1)) });
            (channel, ChannelClass::Forged)
        }),
        1 => Just((Par::default(), ChannelClass::Forged)),
    ]
    .boxed()
}

/// The route of a channel of class `class` under `ctx` (rules R1 to R5).
fn expected_route(class: ChannelClass, channel: &Par, ctx: &SpaceCtx) -> Route {
    let key = || CollectionKey::of_channel(channel).expect("a channel has a key");
    match (class, ctx) {
        (
            ChannelClass::System
            | ChannelClass::DefaultObject
            | ChannelClass::Unforgeable
            | ChannelClass::Storage,
            _,
        ) => Route::Default,
        (ChannelClass::SpaceName(space), _) => Route::Space { space, key: key() },
        (ChannelClass::AgentTagged, _) => Route::Agent,
        (ChannelClass::Forged, Some(target)) => Route::Space {
            space: target.native,
            key: key(),
        },
        (ChannelClass::Forged, None) => Route::Default,
    }
}

/// A space reference with the result that `classify_space_ref` must give it.
fn arb_space_ref_case() -> BoxedStrategy<(Par, Result<SpaceRef, SpaceRefError>)> {
    let handle = arb_space().prop_map(|space| (space, gprivate(namespace::handle_channel(space))));
    let empty_par =
        (vec(any::<u8>(), 0..2), any::<bool>()).prop_map(|(locally_free, connective_used)| {
            let empty = Par {
                locally_free,
                connective_used,
                ..Par::default()
            };
            (empty, Err(SpaceRefError::EmptySpaceRef))
        });
    let wrapped = (handle.clone(), any::<bool>(), any::<bool>()).prop_map(
        |((space, handle), write, read)| {
            let expected = match write {
                true => Ok(SpaceRef::Native(space)),
                false => Err(SpaceRefError::HandleNotWritable),
            };
            (bundle(handle, write, read), expected)
        },
    );
    let not_handle =
        arb_id_spec().prop_filter("not a handle", |spec| !matches!(spec, IdSpec::Handle(_)));
    let other_id = (not_handle, 0u8..3).prop_map(|(spec, wrapping)| {
        let id = gprivate(spec.encode());
        let reference = match wrapping {
            0 => id,
            1 => bundle(id, true, false),
            _ => bundle(id, false, true),
        };
        (reference, Err(SpaceRefError::NotASpaceRef))
    });
    let empty_bundle = any::<bool>().prop_map(|write| {
        let empty = Par::default().with_bundles(vec![Bundle {
            body: None,
            write_flag: write,
            read_flag: false,
        }]);
        (empty.clone(), Ok(SpaceRef::Agent(empty)))
    });
    prop_oneof![
        1 => empty_par,
        2 => handle.clone().prop_map(|(space, handle)| (handle, Ok(SpaceRef::Native(space)))),
        2 => wrapped,
        2 => other_id,
        1 => handle.prop_map(|(_, handle)| {
            let nested = bundle(bundle(handle, true, false), true, false);
            (nested.clone(), Ok(SpaceRef::Agent(nested)))
        }),
        1 => empty_bundle,
        3 => arb_ordinary_channel().prop_map(|value| {
            let expected = match is_empty_par(&value) {
                true => Err(SpaceRefError::EmptySpaceRef),
                false => Ok(SpaceRef::Agent(value.clone())),
            };
            (value, expected)
        }),
    ]
    .boxed()
}

impl Node {
    /// The payloads of the records of the collection `name` in `space`.
    async fn payloads(&self, space: NativeSpace, name: &Par) -> Vec<Vec<Par>> {
        let key = CollectionKey::of_channel(name).expect("a channel has a key");
        self.play
            .get_data(&gprivate(namespace::data_channel(space, &key)))
            .await
            .into_iter()
            .map(|datum| {
                unwrap_datum(&datum.a)
                    .expect("a record has an envelope")
                    .datum
                    .pars
            })
            .collect()
    }

    /// The pars of the data on the public channel `channel`, without a check of their count.
    async fn data_of(&self, channel: &Par) -> Vec<Vec<Par>> {
        self.play
            .get_data(channel)
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }
}

/// The program of a deploy with the binders `binders` (`name(`uri`)`) and the body `body`.
fn with_binders(binders: &[(&str, &str)], body: &str) -> String {
    match binders.is_empty() {
        true => body.to_string(),
        false => {
            let binders: Vec<String> = binders
                .iter()
                .map(|(name, uri)| format!("{name}(`{uri}`)"))
                .collect();
            format!("new {} in {{ {body} }}", binders.join(", "))
        }
    }
}

/// `classify_space_ref_total_deterministic`: `classify_space_ref` gives every value exactly
/// its documented result: empty, native handle, handle without write flag, other Reified id,
/// or agent. It removes at most one bundle and gives Par-equal values one result.
#[test]
fn prop_classify_space_ref_total() {
    check(
        pure_config(),
        (arb_space_ref_case(), vec(any::<u8>(), 0..2)),
        |(case, bits)| {
            let (reference, expected) = case;
            prop_assert_eq!(classify_space_ref(&reference), expected.clone());
            let mut with_bits = reference.clone();
            with_bits.locally_free = bits;
            prop_assert_eq!(classify_space_ref(&with_bits), expected);
            Ok(())
        },
    );
}

/// `handle_requires_write_flag`: a bare handle names its native space, and so does a handle
/// under a bundle with the write flag. A handle under a bundle without it is
/// `HandleNotWritable`. Only one bundle is removed.
#[test]
fn prop_handle_requires_write_flag() {
    check(
        pure_config(),
        (arb_space(), any::<bool>(), any::<bool>()),
        |(space, write, read)| {
            let handle = gprivate(namespace::handle_channel(space));
            prop_assert_eq!(classify_space_ref(&handle), Ok(SpaceRef::Native(space)));
            let bundled = bundle(handle.clone(), write, read);
            let expected = match write {
                true => Ok(SpaceRef::Native(space)),
                false => Err(SpaceRefError::HandleNotWritable),
            };
            prop_assert_eq!(classify_space_ref(&bundled), expected);
            let nested = bundle(bundled, true, read);
            prop_assert_eq!(
                classify_space_ref(&nested),
                Ok(SpaceRef::Agent(nested.clone()))
            );
            prop_assert_eq!(
                SpaceRefError::HandleNotWritable.reply_code(),
                SpaceReplyCode::HandleNotWritable
            );
            prop_assert_eq!(
                ReifiedReduceError::from(SpaceRefError::HandleNotWritable).code(),
                "HandleNotWritable"
            );
            Ok(())
        },
    );
}

/// `reified_non_handle_not_a_space_ref`: every Reified id that is not a handle, bare or
/// under one bundle, is `NotASpaceRef`, so capabilities and storage ids are not agents.
#[test]
fn prop_reified_non_handle_not_a_space_ref() {
    check(
        pure_config(),
        (
            arb_id_spec().prop_filter("not a handle", |spec| !matches!(spec, IdSpec::Handle(_))),
            any::<bool>(),
            any::<bool>(),
        ),
        |(spec, write, read)| {
            let id = gprivate(spec.encode());
            prop_assert_eq!(classify_space_ref(&id), Err(SpaceRefError::NotASpaceRef));
            prop_assert_eq!(
                classify_space_ref(&bundle(id, write, read)),
                Err(SpaceRefError::NotASpaceRef)
            );
            prop_assert_eq!(
                SpaceRefError::NotASpaceRef.reply_code(),
                SpaceReplyCode::NotASpaceRef
            );
            prop_assert_eq!(
                ReifiedReduceError::from(SpaceRefError::NotASpaceRef).code(),
                "NotASpaceRef"
            );
            Ok(())
        },
    );
}

/// `route_total_deterministic`: `route` gives every channel and every context one route.
/// The route is the same each time and for Par-equal channels, and it agrees with the one
/// class of the channel.
#[test]
fn prop_route_total() {
    check(
        pure_config(),
        (
            prop_oneof![
                arb_par(),
                arb_channel_case().prop_map(|(channel, _)| channel)
            ],
            arb_ctx(),
            vec(any::<u8>(), 0..2),
        ),
        |(channel, ctx, bits)| {
            let routed = route(&ctx, &channel);
            prop_assert!(routed.is_ok(), "{:?}", routed);
            prop_assert_eq!(&route(&ctx, &channel), &routed);
            let mut with_bits = channel.clone();
            with_bits.locally_free = bits;
            prop_assert_eq!(&route(&ctx, &with_bits), &routed);
            let class = classify_channel(&channel);
            prop_assert_eq!(classify_channel(&with_bits), class);
            prop_assert_eq!(routed, Ok(expected_route(class, &channel, &ctx)));
            Ok(())
        },
    );
}

/// `route_rules`: R1 to R5. System channels, default objects, other unforgeable names and
/// storage ids route to the default space. A space name routes to its space, and an
/// agent-tagged name to the agent. A forged channel routes to the context space, or to the
/// default space without a context.
#[test]
fn prop_route_rules() {
    check(
        pure_config(),
        (arb_channel_case(), arb_ctx()),
        |(case, ctx)| {
            let (channel, class) = case;
            prop_assert_eq!(classify_channel(&channel), class);
            prop_assert_eq!(
                route(&ctx, &channel),
                Ok(expected_route(class, &channel, &ctx))
            );
            Ok(())
        },
    );
}

/// `space_name_route_ignores_ctx`: a space name routes to its own space under every
/// context (R3), so it works across `use` blocks, deploys and restarts.
#[test]
fn prop_space_name_route_ignores_ctx() {
    check(
        pure_config(),
        (arb_space(), arb_tail(), arb_ctx(), arb_ctx()),
        |(space, tail, first, second)| {
            let name = gprivate(namespace::name_id(space, tail));
            let key = CollectionKey::of_channel(&name).expect("a name has a key");
            let expected = Ok(Route::Space { space, key });
            prop_assert_eq!(route(&first, &name), expected.clone());
            prop_assert_eq!(route(&second, &name), expected.clone());
            prop_assert_eq!(route(&None, &name), expected);
            Ok(())
        },
    );
}

/// `ordinary_channels_route_default`: a program can name a channel without Reified syntax:
/// a value, a `new` name, a byte name, a deploy id or a bundle. Such a channel routes to the
/// default space outside a `use` block, so ordinary execution does not change.
#[test]
fn prop_ordinary_channels_route_default() {
    check(
        pure_config(),
        (arb_ordinary_channel(), vec(any::<u8>(), 0..8)),
        |(channel, seed)| {
            prop_assert_eq!(route(&None, &channel), Ok(Route::Default));
            prop_assert!(matches!(
                classify_channel(&channel),
                ChannelClass::System | ChannelClass::Unforgeable | ChannelClass::Forged
            ));
            let new = New {
                bind_count: 2,
                p: Some(Par::default()),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            };
            let bound = allocate_new_bindings(
                &new,
                &Env::new(),
                &mut Blake2b512Random::create_from_bytes(&seed),
                &HashMap::new(),
            )
            .expect("the names of a new are allocated");
            for name in bound.env_map.values() {
                prop_assert_eq!(route(&None, name), Ok(Route::Default));
                prop_assert_eq!(classify_channel(name), ChannelClass::Unforgeable);
            }
            Ok(())
        },
    );
}

/// `r7_route_ignores_ctx`: a system process replies through the R7 backend. The backend
/// routes the raw reply channel with no context. So the reply of `assertEqual` lands where
/// `route(&None, ret)` says, also when the call runs in a `use` block. A send of the program
/// in the same block does route to the context space (R5).
#[test]
fn prop_r7_route_ignores_ctx() {
    check(
        runtime_config(),
        (0u8..3, 0u8..3, (0i64..3, 0i64..3), vec(any::<u8>(), 4)),
        |(reply, context, values, seed)| {
            tokio_runtime().block_on(r7_route_ignores_ctx_case(reply, context, values, seed))
        },
    );
}

/// One case of `prop_r7_route_ignores_ctx`.
async fn r7_route_ignores_ctx_case(
    reply: u8,
    context: u8,
    values: (i64, i64),
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let spaces = [
        node.create_space(&[&seed[..], b"-first"].concat()).await,
        node.create_space(&[&seed[..], b"-second"].concat()).await,
    ];
    let names = [
        space_name(spaces[0], "r7-first"),
        space_name(spaces[1], "r7-second"),
    ];
    let mut binders = vec![("assert", "rho:assert:assertEqual")];
    let mut injected = Vec::new();
    let ret = match reply {
        0 => "\"rrs-p4-ret\"".to_string(),
        index => {
            binders.push(("r", ID_URI));
            injected.push((ID_URI.to_string(), names[usize::from(index) - 1].clone()));
            "*r".to_string()
        }
    };
    let calls = format!(
        "assert!({ret}, {}, {}) | @\"rrs-p4-direct\"!(1)",
        values.0, values.1
    );
    let body = match context {
        0 => calls,
        index => {
            binders.push(("h", HANDLE_URI));
            injected.push((
                HANDLE_URI.to_string(),
                gprivate(namespace::handle_channel(spaces[usize::from(index) - 1])),
            ));
            format!("use h {{ {calls} }}")
        }
    };
    let deploy = DeploySpec::new(with_binders(&binders, &body), injected, &seed);
    let deployed = node.deploy(&deploy).await;
    prop_assert!(
        deployed.result.errors.is_empty(),
        "{:?}",
        deployed.result.errors
    );
    let answer = vec![gbool(values.0 == values.1)];
    let ret_channel = match reply {
        0 => gstring("rrs-p4-ret"),
        index => names[usize::from(index) - 1].clone(),
    };
    match route(&None, &ret_channel) {
        Ok(Route::Default) => {
            prop_assert_eq!(node.data_of(&ret_channel).await, vec![answer]);
            for space in spaces {
                prop_assert!(node.payloads(space, &ret_channel).await.is_empty());
            }
        }
        Ok(Route::Space { space, .. }) => {
            prop_assert_eq!(node.payloads(space, &ret_channel).await, vec![answer]);
            prop_assert!(node.data_of(&ret_channel).await.is_empty());
        }
        other => prop_assert!(false, "an unexpected route {:?}", other),
    }
    let direct = gstring("rrs-p4-direct");
    match context {
        0 => prop_assert_eq!(node.data_of(&direct).await, vec![vec![gint(1)]]),
        index => {
            prop_assert_eq!(
                node.payloads(spaces[usize::from(index) - 1], &direct).await,
                vec![vec![gint(1)]]
            );
            prop_assert!(node.data_of(&direct).await.is_empty());
        }
    }
    Ok(())
}

/// `use_wrapper_restores_ctx`: a receive under a use-context stores its body in the USE
/// tuple of the context. So the context returns when the body runs, whoever triggers it. A
/// body that a handle call fires from outside every `use` block still sends to the space of
/// its block.
#[test]
fn prop_use_wrapper_restores_ctx() {
    check(
        runtime_config(),
        (0i64..5, any::<bool>(), arb_par(), vec(any::<u8>(), 4)),
        |(value, through_handle, body, seed)| {
            tokio_runtime().block_on(use_wrapper_restores_ctx_case(
                value,
                through_handle,
                body,
                seed,
            ))
        },
    );
}

/// One case of `prop_use_wrapper_restores_ctx`.
async fn use_wrapper_restores_ctx_case(
    value: i64,
    through_handle: bool,
    body: Par,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let target = UseTarget {
        space: gprivate(namespace::handle_channel(RIG_SPACE)),
        native: RIG_SPACE,
    };
    prop_assert_eq!(wrap_body(&None, body.clone()), body.clone());
    let wrapped = wrap_body(&Some(Arc::new(target.clone())), body.clone());
    prop_assert_eq!(&wrapped.locally_free, &body.locally_free);
    let tuple = match wrapped.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::ETupleBody(tuple)),
        }] => tuple.clone(),
        other => panic!("the wrapper is one tuple: {other:?}"),
    };
    prop_assert_eq!(
        decode_process_tuple(&tuple),
        Ok(ProcessConstruct::Use(UseBlock {
            space: target.space.clone(),
            body
        }))
    );
    let mut node = Node::new().await;
    let space = node.create_space(&[&seed[..], b"-space"].concat()).await;
    let handle = gprivate(namespace::handle_channel(space));
    let injected = vec![(HANDLE_URI.to_string(), handle)];
    let receive = with_binders(
        &[("h", HANDLE_URI)],
        "use h { for (@x <- @\"rrs-p4-in\") { @\"rrs-p4-relay\"!(x) } }",
    );
    let parked = node
        .deploy(&DeploySpec::new(receive, injected.clone(), b"park"))
        .await;
    prop_assert!(
        parked.result.errors.is_empty(),
        "{:?}",
        parked.result.errors
    );
    let input = gstring("rrs-p4-in");
    prop_assert_eq!(node.queue(space, &input).await.waiters.len(), 1);
    let send = match through_handle {
        true => format!(
            "new h(`{HANDLE_URI}`), ret in {{ h!(*ret, \"produce\", \"rrs-p4-in\", [{value}]) }}"
        ),
        false => with_binders(
            &[("h", HANDLE_URI)],
            &format!("use h {{ @\"rrs-p4-in\"!({value}) }}"),
        ),
    };
    let fired = node.deploy(&DeploySpec::new(send, injected, b"fire")).await;
    prop_assert!(fired.result.errors.is_empty(), "{:?}", fired.result.errors);
    prop_assert!(node.queue(space, &input).await.waiters.is_empty());
    let relay = gstring("rrs-p4-relay");
    prop_assert_eq!(node.payloads(space, &relay).await, vec![vec![gint(value)]]);
    prop_assert!(node.data_of(&relay).await.is_empty());
    Ok(())
}

// ─── Factory (SpaceFactory.v) ────────────────────────────────────────────────────────────────

fn arb_family() -> BoxedStrategy<SpaceFactoryUrn> {
    (
        select(SpaceOuter::ALL.to_vec()),
        select(SpaceInner::ALL.to_vec()),
    )
        .prop_map(|(outer, inner)| SpaceFactoryUrn { outer, inner })
        .boxed()
}

fn arb_factory_qualifier() -> BoxedStrategy<SpaceQualifier> {
    select(SpaceQualifier::ALL.to_vec()).boxed()
}

/// The result of `SpaceFactoryUrn::parse` as the URN grammar of P4 design A.5 states it.
fn expected_family(value: &str) -> Option<SpaceFactoryUrn> {
    match value {
        HASHMAP_BAG_ALIAS => {
            return Some(SpaceFactoryUrn {
                outer: SpaceOuter::HashMap,
                inner: SpaceInner::Bag,
            })
        }
        PATHMAP_BAG_ALIAS => {
            return Some(SpaceFactoryUrn {
                outer: SpaceOuter::PathMap,
                inner: SpaceInner::Bag,
            })
        }
        _ => {}
    }
    let rest = value.strip_prefix(SPACE_URN_PREFIX)?;
    let parts: Vec<&str> = rest.split(':').collect();
    let [outer, inner] = parts.as_slice() else {
        return None;
    };
    let outer = SpaceOuter::ALL
        .into_iter()
        .find(|candidate| candidate.as_str() == *outer)?;
    let inner = SpaceInner::ALL
        .into_iter()
        .find(|candidate| candidate.as_str() == *inner)?;
    let family = SpaceFactoryUrn { outer, inner };
    let supported = !matches!(
        (outer, inner),
        (
            SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet,
            SpaceInner::VectorDb
        )
    );
    supported.then_some(family)
}

/// A string near a space URN: an alias, a family URN, or a URN with a wrong segment, segment
/// count or prefix.
fn arb_urn_candidate() -> BoxedStrategy<String> {
    let outer_names: Vec<_> = SpaceOuter::ALL.iter().map(|outer| outer.as_str()).collect();
    let inner_names: Vec<_> = SpaceInner::ALL.iter().map(|inner| inner.as_str()).collect();
    let outer = prop_oneof![
        3 => select(outer_names).prop_map(String::from),
        1 => "[a-zA-Z]{0,7}",
    ];
    let inner = prop_oneof![
        3 => select(inner_names).prop_map(String::from),
        1 => "[a-zA-Z]{0,9}",
    ];
    let aliases = vec![
        HASHMAP_BAG_ALIAS,
        PATHMAP_BAG_ALIAS,
        "rho:space:HashMapBag",
        "rho:space:",
    ];
    let other_prefixes = vec!["rho:spaces:", "rho:space", "space:", ""];
    prop_oneof![
        1 => select(aliases).prop_map(String::from),
        4 => (outer.clone(), inner.clone())
            .prop_map(|(outer, inner)| format!("{SPACE_URN_PREFIX}{outer}:{inner}")),
        1 => (outer.clone(), inner.clone(), "[a-z]{0,4}").prop_map(|(outer, inner, extra)| {
            format!("{SPACE_URN_PREFIX}{outer}:{inner}:{extra}")
        }),
        1 => outer.prop_map(|outer| format!("{SPACE_URN_PREFIX}{outer}")),
        1 => (select(other_prefixes), inner)
            .prop_map(|(prefix, inner)| format!("{prefix}hashmap:{inner}")),
    ]
    .boxed()
}

/// A constructor with an optional qualifier, an optional FREE theory and optional parameters.
fn arb_constructor() -> BoxedStrategy<SpaceFactoryConstructor> {
    (
        proptest::option::of(arb_factory_qualifier()),
        proptest::option::of(reified_strategies::arb_free_theory(arb_ground_par())),
        proptest::option::of(reified_strategies::arb_send_params(arb_ground_par())),
    )
        .prop_map(|(qualifier, theory, params)| SpaceFactoryConstructor {
            qualifier,
            theory,
            params,
        })
        .boxed()
}

/// Nonempty `locally_free` bits. A string has no free variables, so a qualifier string with
/// these bits is not the canonical encoding of its qualifier.
fn arb_stray_bits() -> BoxedStrategy<Vec<u8>> { vec(any::<u8>(), 1..3).boxed() }

/// The elements of a constructor list near the canonical form. They are canonical elements,
/// canonical elements whose qualifier string carries stray `locally_free` bits, a permutation,
/// a repeated element, or a list of other pieces. The pieces are other values, qualifier
/// strings (some with stray bits), FREE tuples and PARAMS tuples.
fn arb_constructor_elements() -> BoxedStrategy<Vec<Par>> {
    let canonical =
        arb_constructor().prop_map(|ctor| ctor.to_elements().expect("a constructor encodes"));
    let stray_qualifier = (arb_constructor(), arb_factory_qualifier(), arb_stray_bits()).prop_map(
        |(ctor, qualifier, bits)| {
            let ctor = SpaceFactoryConstructor {
                qualifier: Some(qualifier),
                ..ctor
            };
            let mut elements = ctor.to_elements().expect("a constructor encodes");
            if let Some(first) = elements.first_mut() {
                first.locally_free = bits;
            }
            elements
        },
    );
    let piece = prop_oneof![
        2 => select(vec!["default", "temp", "seq", "durable", ""]).prop_map(gstring),
        1 => (select(vec!["default", "temp", "seq"]), arb_stray_bits())
            .prop_map(|(name, bits)| Par { locally_free: bits, ..gstring(name) }),
        1 => Just(Par { connective_used: true, ..gstring("default") }),
        2 => reified_strategies::arb_free_theory(arb_ground_par())
            .prop_map(|theory| encode_free(&theory).expect("a theory encodes")),
        2 => reified_strategies::arb_send_params(arb_ground_par()).prop_map(|params| {
            encode_params(&params)
                .expect("params encode")
                .expect("params are not empty")
        }),
        2 => arb_ground_par(),
    ];
    prop_oneof![
        2 => canonical.clone(),
        1 => stray_qualifier,
        1 => canonical.clone().prop_shuffle(),
        1 => (canonical, any::<prop::sample::Index>()).prop_map(|(mut elements, index)| {
            if !elements.is_empty() {
                let repeated = elements[index.index(elements.len())].clone();
                elements.push(repeated);
            }
            elements
        }),
        2 => vec(piece, 0..4),
    ]
    .boxed()
}

/// `family_index_bijective`: the family index is a bijection between the 32 supported pairs
/// and `0..32`. `from_family_index` inverts it, and the unsupported pairs have none. The
/// factory channel of family `i` is `[0xE0, i]`, and its body ref is `200 + i`.
#[test]
fn prop_family_index_bijective() {
    check(
        pure_config(),
        (arb_family(), any::<u8>()),
        |(family, index)| {
            match family.family_index() {
                Some(found) => {
                    prop_assert!(family.is_supported_pair());
                    prop_assert!(found < FACTORY_FAMILY_COUNT);
                    prop_assert_eq!(SpaceFactoryUrn::from_family_index(found), Some(family));
                    prop_assert_eq!(
                        factory_channel(family),
                        Some(gprivate(namespace::op_channel(found)))
                    );
                    prop_assert_eq!(body_ref(found), 200 + i64::from(found));
                    prop_assert!(found <= FACTORY_LAST);
                }
                None => {
                    prop_assert!(!family.is_supported_pair());
                    prop_assert_eq!(factory_channel(family), None);
                }
            }
            prop_assert_eq!(
                SpaceFactoryUrn::from_family_index(index).is_some(),
                index < FACTORY_FAMILY_COUNT
            );
            if let Some(found) = SpaceFactoryUrn::from_family_index(index) {
                prop_assert_eq!(found.family_index(), Some(index));
            }
            let indices: Vec<u8> = SpaceFactoryUrn::all()
                .map(|family| {
                    family
                        .family_index()
                        .expect("every listed family has an index")
                })
                .collect();
            prop_assert_eq!(indices, (0..FACTORY_FAMILY_COUNT).collect::<Vec<u8>>());
            prop_assert_eq!(
                SpaceFactoryUrn::HASHMAP_QUEUE.family_index(),
                Some(HASHMAP_QUEUE_FACTORY.op)
            );
            Ok(())
        },
    );
}

/// `aliases_map_to_family`: a URN parses exactly as the outer-first grammar states. The two
/// FIPS aliases name the bag families of HashMap and PathMap with their family indices.
/// Every family parses back from its URN.
#[test]
fn prop_aliases_map_to_family() {
    check(
        pure_config(),
        (arb_urn_candidate(), arb_family()),
        |(candidate, family)| {
            prop_assert_eq!(
                SpaceFactoryUrn::parse(&candidate).ok(),
                expected_family(&candidate)
            );
            for (alias, outer) in [
                (HASHMAP_BAG_ALIAS, SpaceOuter::HashMap),
                (PATHMAP_BAG_ALIAS, SpaceOuter::PathMap),
            ] {
                let canonical = SpaceFactoryUrn {
                    outer,
                    inner: SpaceInner::Bag,
                };
                prop_assert_eq!(SpaceFactoryUrn::parse(alias), Ok(canonical));
                prop_assert_eq!(
                    SpaceFactoryUrn::parse(&canonical.to_string()),
                    Ok(canonical)
                );
                prop_assert!(canonical.family_index().is_some());
            }
            match family.is_supported_pair() {
                true => prop_assert_eq!(SpaceFactoryUrn::parse(&family.to_string()), Ok(family)),
                false => prop_assert!(SpaceFactoryUrn::parse(&family.to_string()).is_err()),
            }
            Ok(())
        },
    );
}

/// `qualifier_matrix`: the HashSet store takes the `seq` qualifier only, every other store
/// takes every qualifier. In native execution every factory with a constructor schema accepts
/// `default`, `temp` and `seq` (P7 steps 1 and 2, design v3 QT-1). Legacy execution refuses `seq`
/// with `QualifierUnavailable` before the params, because the Seq rules need the job order of
/// native execution (QT-5). An Array factory then also needs its `size` (F7, P5 design v3 A.10).
/// Every family without a schema is unavailable. Since P7 the HashSet families have a schema, so
/// a HashSet factory refuses `default` and `temp` with `BadConstructorArgument` (HS-6).
#[test]
fn prop_qualifier_matrix() {
    check(
        pure_config(),
        (arb_family(), arb_factory_qualifier(), any::<bool>()),
        |(family, qualifier, native)| {
            prop_assert_eq!(
                family.supports_qualifier(qualifier),
                family.outer != SpaceOuter::HashSet || qualifier == SpaceQualifier::Seq
            );
            let execution = match native {
                true => Execution::Native,
                false => Execution::Legacy,
            };
            let ctor = list(vec![gstring(qualifier.as_str())]);
            let validated = FactoryConstructor::validate_in(family, &ctor, execution);
            // Changed by P7 step 2 (design v3 QT-5): `seq` passes gate 3 in native execution.
            // let validated = FactoryConstructor::validate(family, &ctor);
            let passes_gate_3 = qualifier != SpaceQualifier::Seq || native;
            // Changed by P7 step 3 (design v3 HS-7): a HashSet family has a schema, so the store
            // check of gate 3 decides before the execution check.
            // match (
            //     constructor_schema(family).is_some(),
            //     required_params(family),
            //     passes_gate_3,
            // ) {
            if constructor_schema(family).is_some() && !family.supports_qualifier(qualifier) {
                prop_assert_eq!(
                    validated,
                    Err(SpaceError::BadConstructorArgument(format!(
                        "the {} outer store does not take the {} qualifier",
                        family.outer.as_str(),
                        qualifier.as_str()
                    )))
                );
                return Ok(());
            }
            match (
                constructor_schema(family).is_some(),
                required_params(family),
                passes_gate_3,
            ) {
                // Changed by P7 step 1 (design v3 QT-1): `temp` is accepted like `default`.
                // (true, [], SpaceQualifier::Default) => {
                //     let validated = validated.expect("a family with a schema accepts default");
                //     prop_assert_eq!(validated.qualifier, SpaceQualifier::Default);
                //     prop_assert_eq!(validated.durability(), Durability::Durable);
                // }
                (true, [], true) => {
                    let validated =
                        validated.expect("a family with a schema accepts the qualifier");
                    prop_assert_eq!(validated.qualifier, qualifier);
                    let durability = match qualifier {
                        SpaceQualifier::Default => Durability::Durable,
                        SpaceQualifier::Temp | SpaceQualifier::Seq => Durability::Ephemeral,
                    };
                    prop_assert_eq!(validated.durability(), durability);
                }
                (true, [required, ..], true) => {
                    prop_assert_eq!(
                        validated,
                        Err(SpaceError::MissingParam {
                            key: (*required).to_string(),
                        })
                    )
                }
                (true, _, false) => prop_assert_eq!(
                    validated,
                    Err(SpaceError::QualifierUnavailable(
                        SpaceQualifier::Seq.as_str().to_string()
                    ))
                ),
                (false, _, _) => prop_assert_eq!(
                    validated,
                    Err(SpaceError::FamilyUnavailable {
                        outer: family.outer.as_str().to_string(),
                        inner: family.inner.as_str().to_string(),
                    })
                ),
            }
            Ok(())
        },
    );
}

/// `constructor_parse_canonical`: parsing inverts the canonical encoding of a constructor
/// `[q?][FREE?][PARAMS?]`, as a list and as its elements.
#[test]
fn prop_constructor_parse_canonical() {
    check(pure_config(), arb_constructor(), |ctor| {
        let elements = ctor.to_elements().expect("a constructor encodes");
        prop_assert_eq!(
            SpaceFactoryConstructor::parse_elements(&elements),
            Ok(ctor.clone())
        );
        prop_assert_eq!(
            SpaceFactoryConstructor::parse(&list(elements)),
            Ok(ctor.clone())
        );
        prop_assert_eq!(
            ctor.effective_qualifier(),
            ctor.qualifier.unwrap_or_default()
        );
        Ok(())
    });
}

/// `constructor_parse_sound`: every element list that the parser accepts is the canonical
/// encoding of its result, so a constructor has one accepted form. `Par` equality ignores
/// `locally_free`, so the property also compares the bits of the qualifier element. The FREE
/// and PARAMS decoders of P2 compare under `Par` equality, as `Encoding.v` models them.
#[test]
fn prop_constructor_parse_sound() {
    check(pure_config(), arb_constructor_elements(), |elements| {
        if let Ok(ctor) = SpaceFactoryConstructor::parse_elements(&elements) {
            let canonical = ctor.to_elements();
            prop_assert_eq!(canonical.as_deref(), Ok(elements.as_slice()));
            if let (Some(_), Ok([qualifier, ..]), [element, ..]) =
                (ctor.qualifier, canonical.as_deref(), elements.as_slice())
            {
                prop_assert_eq!(&qualifier.locally_free, &element.locally_free);
            }
            prop_assert_eq!(SpaceFactoryConstructor::parse(&list(elements)), Ok(ctor));
        }
        Ok(())
    });
}

// ─── Factory: the runtime properties ─────────────────────────────────────────────────────────

/// The program of a factory call that publishes the handle on `out`. The forms are the source
/// forms of a factory call with the default qualifier.
fn factory_program(out: &str, form: u8) -> String {
    let call = match form {
        0 => "factory!?(\"default\")",
        _ => "factory!?()",
    };
    format!(
        "new factory(`rho:space:hashmap:queue`) in {{ \
         for (space <- {call}) {{ @\"{out}\"!(*space) }} }}"
    )
}

/// The native space of the only handle on the public channel `out` of `runtime`.
async fn published_space(runtime: &RhoRuntimeImpl, out: &str) -> Option<NativeSpace> {
    match runtime.get_data(&gstring(out)).await.as_slice() {
        [datum] => match datum.a.pars.as_slice() {
            [handle] => match classify_space_ref(handle) {
                Ok(SpaceRef::Native(space)) => Some(space),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// The op ids that take calls and every other op id, so that both groups occur often.
fn arb_op_id() -> BoxedStrategy<u8> {
    prop_oneof![
        2 => 0u8..=NATIVE_ENGINE_FACTORY,
        2 => select(vec![GET_SPACE_AGENT, ASSERT_EQUAL, VECTOR, HANDLE_OP, MATCHER_OP, SINK_OP]),
        2 => any::<u8>(),
        1 => select(Marker::ALL.iter().map(|marker| *marker as u8).collect::<Vec<u8>>()),
    ]
    .boxed()
}

/// `sid_derivation_deterministic`: the space id is the `SPACE_ID` draw of the random state of
/// the call. The derivation is a pure function, leaves the call random unchanged and differs
/// from the other purposes. Two nodes and the replay of the factory deploy create one space.
#[test]
fn prop_sid_derivation_deterministic() {
    check(
        runtime_config(),
        (
            vec(any::<u8>(), 0..12),
            vec(any::<u8>(), 0..12),
            0u8..2,
            vec(any::<u8>(), 1..8),
        ),
        |(call, other, form, seed)| {
            tokio_runtime().block_on(sid_derivation_deterministic_case(call, other, form, seed))
        },
    );
}

/// One case of `prop_sid_derivation_deterministic`.
async fn sid_derivation_deterministic_case(
    call: Vec<u8>,
    other: Vec<u8>,
    form: u8,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let random = Blake2b512Random::create_from_bytes(&call);
    let sid = derive_space_id(&random);
    prop_assert_eq!(sid, derive_space_id(&random));
    let mut draw = RandomPurpose::SpaceId.derive(&random);
    let expected: Vec<u8> = draw.next().into_iter().map(|byte| byte as u8).collect();
    prop_assert_eq!(&sid[..], &expected[..SID_LEN]);
    prop_assert_eq!(
        random.clone().next(),
        Blake2b512Random::create_from_bytes(&call).next()
    );
    for purpose in [
        RandomPurpose::Gensym,
        RandomPurpose::Config,
        RandomPurpose::Handle,
        RandomPurpose::Counter,
    ] {
        let mut other_draw = purpose.derive(&random);
        let other_bytes: Vec<u8> = other_draw
            .next()
            .into_iter()
            .map(|byte| byte as u8)
            .collect();
        prop_assert_ne!(&sid[..], &other_bytes[..SID_LEN]);
    }
    if other != call {
        prop_assert_ne!(
            sid,
            derive_space_id(&Blake2b512Random::create_from_bytes(&other))
        );
    }
    let out = "rrs-p4-sid-handle";
    let spec = DeploySpec::new(factory_program(out, form), Vec::new(), &seed);
    let mut first = Node::new().await;
    let start = first.play.get_root().await;
    let played = first.deploy(&spec).await;
    prop_assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let created = published_space(&first.play, out)
        .await
        .expect("the factory published a handle");
    let mut second = Node::new().await;
    let again = second.deploy(&spec).await;
    prop_assert!(again.result.errors.is_empty(), "{:?}", again.result.errors);
    prop_assert_eq!(published_space(&second.play, out).await, Some(created));
    let replayed = first.replay(&start, &spec, &played).await;
    prop_assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
    prop_assert_eq!(replayed.cost.value, played.result.cost.value);
    prop_assert_eq!(published_space(&first.replay, out).await, Some(created));
    Ok(())
}

/// `persistent_op_call_aborts` (decision 32, A21, readiness review §4). A persistent send to
/// a factory, to another op that takes calls, or to a handle aborts the deploy once. The
/// abort has its code in play and in replay. A plain send to the same channel does not
/// abort, and a persistent send to an op that takes no calls is an ordinary send.
#[test]
fn prop_persistent_op_call_aborts() {
    check(
        runtime_config(),
        (
            arb_op_id(),
            proptest::option::weighted(0.25, arb_space()),
            vec(any::<u8>(), 1..8),
        ),
        |(op, handle, seed)| {
            tokio_runtime().block_on(persistent_op_call_aborts_case(op, handle, seed))
        },
    );
}

/// One case of `prop_persistent_op_call_aborts`.
async fn persistent_op_call_aborts_case(
    op: u8,
    handle: Option<NativeSpace>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let channel = match handle {
        Some(space) => gprivate(namespace::handle_channel(space)),
        None => gprivate(namespace::op_channel(op)),
    };
    let expected = match (handle, op) {
        (Some(_), _) => Some("PersistentHandleCall"),
        (None, 0..=NATIVE_ENGINE_FACTORY) => Some("PersistentFactoryCall"),
        (None, GET_SPACE_AGENT | ASSERT_EQUAL | VECTOR | HANDLE_OP | MATCHER_OP | SINK_OP) => {
            Some("PersistentOpCall")
        }
        (None, _) => None,
    };
    prop_assert_eq!(
        check_persistent_send(&channel, true)
            .err()
            .map(|error| error.code()),
        expected
    );
    prop_assert_eq!(check_persistent_send(&channel, false), Ok(()));
    let mut node = Node::new().await;
    let start = node.play.get_root().await;
    let injected = vec![(ID_URI.to_string(), channel.clone())];
    let persistent = DeploySpec::new(
        with_binders(&[("x", ID_URI)], "x!!(1)"),
        injected.clone(),
        &seed,
    );
    let played = node.deploy(&persistent).await;
    let codes = played.codes();
    prop_assert_eq!(
        codes.clone(),
        expected
            .map(|code| vec![code.to_string()])
            .unwrap_or_default()
    );
    let replayed = node.replay(&start, &persistent, &played).await;
    prop_assert_eq!(error_codes(&replayed.errors), codes);
    prop_assert_eq!(replayed.cost.value, played.result.cost.value);
    let plain = DeploySpec::new(
        with_binders(&[("x", ID_URI)], "x!(1)"),
        injected,
        &[&seed[..], b"-plain"].concat(),
    );
    let played = node.deploy(&plain).await;
    prop_assert!(
        played
            .codes()
            .iter()
            .all(|code| !code.starts_with("Persistent")),
        "{:?}",
        played.codes()
    );
    Ok(())
}

/// `space_already_exists_is_deterministic_abort` (readiness review §4). A factory call whose
/// space id names an existing space aborts with `SpaceAlreadyExists`. The abort is the same
/// in play, in replay and on another node, with one cost. The space keeps one config record,
/// one space counter and one handle.
#[test]
fn prop_space_already_exists_is_deterministic_abort() {
    check(
        runtime_config(),
        (0u8..2, vec(any::<u8>(), 1..8)),
        |(form, seed)| {
            tokio_runtime().block_on(space_already_exists_is_deterministic_abort_case(form, seed))
        },
    );
}

/// One case of `prop_space_already_exists_is_deterministic_abort`.
async fn space_already_exists_is_deterministic_abort_case(
    form: u8,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let out = "rrs-p4-exists-handle";
    let spec = DeploySpec::new(factory_program(out, form), Vec::new(), &seed);
    let mut outcomes = Vec::with_capacity(2);
    for _ in 0..2 {
        let mut node = Node::new().await;
        let first = node.deploy(&spec).await;
        prop_assert!(first.result.errors.is_empty(), "{:?}", first.result.errors);
        let space = published_space(&node.play, out).await.expect("a handle");
        let root = node.checkpoint().await;
        let second = node.deploy(&spec).await;
        prop_assert_eq!(second.codes(), vec!["SpaceAlreadyExists".to_string()]);
        let replayed = node.replay(&root, &spec, &second).await;
        prop_assert_eq!(error_codes(&replayed.errors), second.codes());
        prop_assert_eq!(replayed.cost.value, second.result.cost.value);
        prop_assert_eq!(published_space(&node.play, out).await, Some(space));
        let config = node
            .play
            .get_data(&gprivate(namespace::config_channel(space)))
            .await;
        prop_assert_eq!(config.len(), 1);
        prop_assert!(config[0].persist);
        let counters = node
            .play
            .get_data(&gprivate(namespace::space_counter(space)))
            .await;
        prop_assert_eq!(counters.len(), 1);
        prop_assert_eq!(counter_value(&counters[0].a), Ok(0));
        let handles = node
            .play
            .get_continuations(vec![gprivate(namespace::handle_channel(space))])
            .await;
        prop_assert_eq!(handles.len(), 1);
        outcomes.push((space, second.codes(), second.result.cost.value));
    }
    prop_assert_eq!(&outcomes[0], &outcomes[1]);
    Ok(())
}

// ─── TLA+ invariants (HeadOnlyDiscipline, CounterConflict, NativeHandles) ────────────────────

/// An operation of a schedule on one collection. The values are 0 and 1, and most receives
/// match every value, so heads often interact. A persistent send sends 9, and a persistent
/// receive matches 0 or 1. So no schedule holds a divergent pair, and every operation ends in
/// its transaction.
fn arb_schedule_op(label: usize) -> BoxedStrategy<SlotOp> {
    let label = format!("op-{label}");
    let pattern = || prop_oneof![2 => Just(Pat::Any), 1 => (0i64..2).prop_map(Pat::Lit)].boxed();
    let receive = |arrow: Arrow, pat: BoxedStrategy<Pat>, label: String| {
        pat.prop_map(move |pat| SlotOp::Receive {
            slot: 0,
            pat,
            arrow,
            label: label.clone(),
        })
    };
    prop_oneof![
        4 => (0i64..2).prop_map(|value| SlotOp::Send { slot: 0, value, persistent: false }),
        1 => Just(SlotOp::Send { slot: 0, value: 9, persistent: true }),
        4 => receive(Arrow::Consume, pattern(), label.clone()),
        1 => receive(Arrow::Peek, pattern(), label.clone()),
        1 => receive(Arrow::Persistent, (0i64..2).prop_map(Pat::Lit).boxed(), label),
    ]
    .boxed()
}

fn arb_schedule() -> BoxedStrategy<Vec<SlotOp>> {
    (4usize..16)
        .prop_flat_map(|length| (0..length).map(arb_schedule_op).collect::<Vec<_>>())
        .boxed()
}

/// A send that does not persist.
fn arb_plain_send() -> BoxedStrategy<SlotOp> {
    (0i64..2)
        .prop_map(|value| SlotOp::Send {
            slot: 0,
            value,
            persistent: false,
        })
        .boxed()
}

/// A plain operation: a send that does not persist, or a consume or peek that matches every
/// value. Such operations never block at the head.
fn arb_flowing_op() -> BoxedStrategy<SlotOp> {
    prop_oneof![
        3 => arb_plain_send(),
        5 => Just(receive_op(0, Pat::Any, Arrow::Consume, "flow")),
        1 => Just(receive_op(0, Pat::Any, Arrow::Peek, "flow")),
    ]
    .boxed()
}

/// A schedule for the FIFO property. Most schedules start with a burst of sends and continue
/// with plain operations, so records leave in many steps. The others are general schedules,
/// which often block at the head.
fn arb_fifo_schedule() -> BoxedStrategy<Vec<SlotOp>> {
    let flowing = (vec(arb_plain_send(), 2..6), vec(arb_flowing_op(), 2..12)).prop_map(
        |(mut schedule, rest)| {
            schedule.extend(rest);
            schedule
        },
    );
    prop_oneof![1 => arb_schedule(), 3 => flowing].boxed()
}

/// The queue operation of a schedule operation.
fn queue_op(op: &SlotOp) -> QueueOp {
    match op {
        SlotOp::Send {
            value, persistent, ..
        } => QueueOp::Produce {
            value: *value,
            persist: *persistent,
        },
        SlotOp::Receive { pat, arrow, .. } => QueueOp::Consume {
            pat: *pat,
            persist: *arrow == Arrow::Persistent,
            peek: *arrow == Arrow::Peek,
            guard: None,
        },
    }
}

/// One step of a schedule: the collection before and after one deploy.
struct ScheduleStep {
    op: SlotOp,
    before: QueueState,
    after: QueueState,
}

/// Runs each operation of `schedule` as its own deploy on a fresh space and records the
/// collection around each deploy.
async fn run_schedule(
    schedule: &[SlotOp],
    seed: &[u8],
) -> Result<Vec<ScheduleStep>, TestCaseError> {
    let mut node = Node::new().await;
    let space = node.create_space(&[seed, b"-space"].concat()).await;
    let names = vec![space_name(
        space,
        &format!("schedule-{}", hex::encode(seed)),
    )];
    let mut steps = Vec::with_capacity(schedule.len());
    for (index, op) in schedule.iter().enumerate() {
        let before = node.queue(space, &names[0]).await;
        let deployed = node
            .deploy(&ops_deploy(
                std::slice::from_ref(op),
                &names,
                OUT,
                &[seed, b"-op-", &index.to_be_bytes()].concat(),
            ))
            .await;
        prop_assert!(
            deployed.result.errors.is_empty(),
            "{:?}: {:?}",
            op,
            deployed.result.errors
        );
        let after = node.queue(space, &names[0]).await;
        steps.push(ScheduleStep {
            op: op.clone(),
            before,
            after,
        });
    }
    Ok(steps)
}

fn ords_of_records(state: &QueueState) -> BTreeSet<i64> {
    state.records.iter().map(|record| record.ord).collect()
}

fn ords_of_waiters(state: &QueueState) -> BTreeSet<i64> {
    state.waiters.iter().map(|waiter| waiter.ord).collect()
}

/// A handle call of a branch: a read-only method, or `produce` on a collection slot.
#[derive(Clone, Debug)]
enum HandleCall {
    Gensym,
    Config,
    Qualifier,
    Produce { slot: usize, value: i64 },
}

fn arb_handle_call(slot: usize) -> BoxedStrategy<HandleCall> {
    prop_oneof![
        Just(HandleCall::Gensym),
        Just(HandleCall::Config),
        Just(HandleCall::Qualifier),
        arb_value().prop_map(move |value| HandleCall::Produce { slot, value }),
    ]
    .boxed()
}

/// The deploy of handle calls on the bare handle of `space`.
fn handle_calls_deploy(
    calls: &[HandleCall],
    space: NativeSpace,
    names: &[Par],
    seed: &[u8],
) -> DeploySpec {
    let mut binders = vec![format!("h(`{HANDLE_URI}`)")];
    let mut injected = vec![(
        HANDLE_URI.to_string(),
        gprivate(namespace::handle_channel(space)),
    )];
    let mut body = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        binders.push(format!("ret{index}"));
        body.push(match call {
            HandleCall::Gensym => format!("h!(*ret{index}, \"gensym\")"),
            HandleCall::Config => format!("h!(*ret{index}, \"config\")"),
            HandleCall::Qualifier => format!("h!(*ret{index}, \"qualifier\")"),
            HandleCall::Produce { slot, value } => {
                let binder = format!("q{slot}(`{}`)", slot_uri(*slot));
                if !binders.contains(&binder) {
                    binders.push(binder);
                    injected.push((slot_uri(*slot), names[*slot].clone()));
                }
                format!("h!(*ret{index}, \"produce\", *q{slot}, [{value}])")
            }
        });
    }
    DeploySpec::new(
        format!("new {} in {{ {} }}", binders.join(", "), body.join(" | ")),
        injected,
        seed,
    )
}

impl MergeRig {
    /// Runs `deploy` on the pre-state and makes its post-state the new pre-state.
    async fn extend_setup(&mut self, deploy: &DeploySpec) {
        self.node.reset(&self.root).await;
        let deployed = self.node.deploy(deploy).await;
        assert!(
            deployed.result.errors.is_empty(),
            "a setup deploy failed: {:?}",
            deployed.result.errors
        );
        self.root = self.node.checkpoint().await;
    }
}

/// `HeadOnlyNoPendingMatch` (HeadOnlyDiscipline): a schedule runs sends, receives, peeks and
/// persistent operations, each in its own deploy. After every step the collection is well
/// formed, and its head waiter does not match its head record.
#[test]
fn prop_queue_no_pending_match_any_schedule() {
    check(
        runtime_config(),
        (arb_schedule(), vec(any::<u8>(), 4)),
        |(schedule, seed)| {
            tokio_runtime().block_on(queue_no_pending_match_any_schedule_case(schedule, seed))
        },
    );
}

/// One case of `prop_queue_no_pending_match_any_schedule`.
async fn queue_no_pending_match_any_schedule_case(
    schedule: Vec<SlotOp>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let steps = run_schedule(&schedule, &seed).await?;
    for step in &steps {
        prop_assert!(
            step.after.well_formed(),
            "{:?} left {:?}",
            step.op,
            step.after
        );
        prop_assert!(
            step.after.no_pending_match(),
            "{:?} left a pending match: {:?}",
            step.op,
            step.after
        );
    }
    Ok(())
}

/// `HeadBlocks` (HeadOnlyDiscipline): take any step of any schedule. The operation blocks if
/// its own side is not empty, or if the other side has no matching head. A blocked operation
/// stores or parks at the tail and leaves the other side unchanged. An operation that fires
/// removes a prefix of the other side in head order.
#[test]
fn prop_queue_head_blocks_any_schedule() {
    check(
        runtime_config(),
        (arb_schedule(), vec(any::<u8>(), 4)),
        |(schedule, seed)| {
            tokio_runtime().block_on(queue_head_blocks_any_schedule_case(schedule, seed))
        },
    );
}

/// One case of `prop_queue_head_blocks_any_schedule`.
async fn queue_head_blocks_any_schedule_case(
    schedule: Vec<SlotOp>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let steps = run_schedule(&schedule, &seed).await?;
    for step in &steps {
        let op = queue_op(&step.op);
        let blocked = match op {
            QueueOp::Produce { value, .. } => {
                !step.before.records.is_empty()
                    || step
                        .before
                        .head_waiter()
                        .is_none_or(|head| !head.fits(value))
            }
            QueueOp::Consume { pat, .. } => {
                !step.before.waiters.is_empty()
                    || step
                        .before
                        .head_record()
                        .is_none_or(|head| !pat.fits(head.value))
            }
        };
        if blocked {
            prop_assert_eq!(
                &step.after,
                &stored_or_parked(&step.before, &op),
                "{:?}",
                step.op
            );
        }
        let (before, after) = match op {
            QueueOp::Produce { .. } => {
                (ords_of_waiters(&step.before), ords_of_waiters(&step.after))
            }
            QueueOp::Consume { .. } => {
                (ords_of_records(&step.before), ords_of_records(&step.after))
            }
        };
        let removed: Vec<i64> = before.difference(&after).copied().collect();
        let prefix: Vec<i64> = before.iter().take(removed.len()).copied().collect();
        prop_assert_eq!(
            removed,
            prefix,
            "{:?} removed entries behind the head",
            step.op
        );
    }
    Ok(())
}

/// `FifoOrder` (HeadOnlyDiscipline): over any schedule, the records leave in the order of
/// their arrival. Each step removes only records older than every record that stays, and
/// later steps remove younger records.
#[test]
fn prop_queue_fifo_any_schedule() {
    check(
        runtime_config(),
        (arb_fifo_schedule(), vec(any::<u8>(), 4)),
        |(schedule, seed)| tokio_runtime().block_on(queue_fifo_any_schedule_case(schedule, seed)),
    );
}

/// One case of `prop_queue_fifo_any_schedule`.
async fn queue_fifo_any_schedule_case(
    schedule: Vec<SlotOp>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let steps = run_schedule(&schedule, &seed).await?;
    let mut last_removed = None;
    for step in &steps {
        let before = ords_of_records(&step.before);
        let after = ords_of_records(&step.after);
        let removed: Vec<i64> = before.difference(&after).copied().collect();
        for gone in &removed {
            prop_assert!(
                after.iter().all(|kept| gone < kept),
                "{:?} removed {} before older records",
                step.op,
                gone
            );
            prop_assert!(
                last_removed < Some(*gone),
                "{:?} removed {} after {:?}",
                step.op,
                gone,
                last_removed
            );
        }
        if let Some(youngest) = removed.last() {
            last_removed = Some(*youngest);
        }
        for record in &step.after.records {
            prop_assert!(before.contains(&record.ord) || record.ord == step.before.next_ord());
        }
    }
    Ok(())
}

/// `NoMergedOrderFabrication` (CounterConflict): two branches use three collections of one
/// space, opened or fresh. They conflict whenever they use one collection or both open a
/// collection. Every merge that the merger accepts is the state of a serial order.
#[test]
fn prop_merged_queue_state_is_serial() {
    check(
        runtime_config(),
        (
            vec(prop_oneof![1 => Just(None), 2 => arb_opened()], 3),
            0u8..3,
            vec((0usize..3, arb_slot_op(0, "a")), 1..3),
            vec((0usize..3, arb_slot_op(0, "b")), 1..3),
            vec(any::<u8>(), 4),
        ),
        |(opened, layout, first, second, seed)| {
            tokio_runtime().block_on(merged_queue_state_is_serial_case(
                opened, layout, first, second, seed,
            ))
        },
    );
}

/// One case of `prop_merged_queue_state_is_serial`.
async fn merged_queue_state_is_serial_case(
    opened: Vec<Option<Vec<i64>>>,
    layout: u8,
    first: Vec<(usize, SlotOp)>,
    second: Vec<(usize, SlotOp)>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let (first, second) = match layout {
        0 => (
            first.into_iter().map(|(_, op)| (0, op)).collect(),
            second.into_iter().map(|(_, op)| (1, op)).collect(),
        ),
        1 => (
            first.into_iter().map(|(slot, op)| (slot % 2, op)).collect(),
            second.into_iter().map(|(_, op)| (2, op)).collect(),
        ),
        _ => (first, second),
    };
    let place = |ops: Vec<(usize, SlotOp)>| -> Vec<SlotOp> {
        ops.into_iter()
            .map(|(slot, op)| match op {
                SlotOp::Send {
                    value, persistent, ..
                } => SlotOp::Send {
                    slot,
                    value,
                    persistent,
                },
                SlotOp::Receive {
                    pat, arrow, label, ..
                } => SlotOp::Receive {
                    slot,
                    pat,
                    arrow,
                    label,
                },
            })
            .collect()
    };
    let (first, second) = (place(first), place(second));
    let slots = |ops: &[SlotOp]| -> BTreeSet<usize> { ops.iter().map(SlotOp::slot).collect() };
    let shared = !slots(&first).is_disjoint(&slots(&second));
    let opens = |ops: &[SlotOp]| slots(ops).iter().any(|slot| opened[*slot].is_none());
    let both_open = opens(&first) && opens(&second);
    let mut rig = MergeRig::new(&opened, &seed).await;
    let first = ops_deploy(&first, &rig.names, "rrs-p4-out-a", b"branch-a");
    let second = ops_deploy(&second, &rig.names, "rrs-p4-out-b", b"branch-b");
    let first_run = rig.branch(std::slice::from_ref(&first)).await;
    let second_run = rig.branch(std::slice::from_ref(&second)).await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    if shared || both_open {
        prop_assert!(
            !conflicting.is_empty(),
            "a shared collection merged: {:?}",
            reason
        );
    }
    if conflicting.is_empty() {
        let first_index = rig.index(&first_run.deployed[0].log);
        let second_index = rig.index(&second_run.deployed[0].log);
        let merged = merged_root(&rig.node.history, &rig.root, &[
            (&first_run.post, &first_index),
            (&second_run.post, &second_index),
        ]);
        let serial = rig.branch(&[first.clone(), second.clone()]).await;
        let reversed = rig.branch(&[second, first]).await;
        prop_assert!(
            merged == serial.post || merged == reversed.post,
            "the merge is not serial"
        );
    }
    Ok(())
}

/// `HandleUnique` (NativeHandles): each factory call creates a space with a fresh id. The
/// space has one persistent handle continuation on `H(sid)`, one config record and the space
/// counter 0, and the call replies `bundle+{H(sid)}`. A call that names an existing space
/// adds nothing.
#[test]
fn prop_handle_unique() {
    check(
        runtime_config(),
        (1usize..4, any::<prop::sample::Index>(), vec(any::<u8>(), 4)),
        |(count, repeat, seed)| tokio_runtime().block_on(handle_unique_case(count, repeat, seed)),
    );
}

/// One case of `prop_handle_unique`.
async fn handle_unique_case(
    count: usize,
    repeat: prop::sample::Index,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let mut spaces = Vec::with_capacity(count);
    for index in 0..count {
        spaces.push(
            node.create_space(&[&seed[..], &index.to_be_bytes()].concat())
                .await,
        );
    }
    let sids: BTreeSet<SpaceId> = spaces.iter().map(|space| space.sid).collect();
    prop_assert_eq!(sids.len(), count);
    let again = repeat.index(count);
    let out = format!(
        "rrs-p4-handle-{}",
        hex::encode([&seed[..], &again.to_be_bytes()].concat())
    );
    let program = factory_program(&out, 0);
    let repeated = node
        .deploy(&DeploySpec::new(
            program,
            Vec::new(),
            &[&seed[..], &again.to_be_bytes()].concat(),
        ))
        .await;
    prop_assert_eq!(repeated.codes(), vec!["SpaceAlreadyExists".to_string()]);
    for space in &spaces {
        let handles = node
            .play
            .get_continuations(vec![gprivate(namespace::handle_channel(*space))])
            .await;
        prop_assert_eq!(handles.len(), 1);
        prop_assert!(handles[0].persist);
        prop_assert!(handles[0].peeks.is_empty());
        prop_assert_eq!(&handles[0].patterns, &vec![handle_pattern()]);
        let config = node
            .play
            .get_data(&gprivate(namespace::config_channel(*space)))
            .await;
        prop_assert_eq!(config.len(), 1);
        let counters = node
            .play
            .get_data(&gprivate(namespace::space_counter(*space)))
            .await;
        prop_assert_eq!(
            counters
                .iter()
                .map(|datum| counter_value(&datum.a))
                .collect::<Vec<_>>(),
            vec![Ok(0)]
        );
        prop_assert_eq!(
            handle_of(&gprivate(namespace::handle_channel(*space))),
            Some(*space)
        );
    }
    Ok(())
}

/// `DanglingIsDeterministicError` (NativeHandles): an operation on a space that does not
/// exist fails with a typed, deterministic error. A send or a receive on a name of the space
/// gets `DanglingName`, and a handle call gets `DanglingHandle`. `getSpaceAgent` replies
/// the typed `DanglingHandle`. Two nodes and the replay agree on the result and the cost,
/// and nothing is stored.
#[test]
fn prop_dangling_deterministic_error() {
    check(
        runtime_config(),
        (
            uniform32(any::<u8>()),
            0u8..4,
            arb_value(),
            vec(any::<u8>(), 1..8),
        ),
        |(sid, kind, value, seed)| {
            tokio_runtime().block_on(dangling_deterministic_error_case(sid, kind, value, seed))
        },
    );
}

/// One case of `prop_dangling_deterministic_error`.
async fn dangling_deterministic_error_case(
    sid: SpaceId,
    kind: u8,
    value: i64,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let space = NativeSpace {
        sid,
        durability: Durability::Durable,
    };
    let name = space_name(space, "dangling");
    let handle = gprivate(namespace::handle_channel(space));
    let (program, injected, expected) = match kind {
        0 => (
            with_binders(&[("q", ID_URI)], &format!("q!({value})")),
            name.clone(),
            Some("DanglingName"),
        ),
        1 => (
            with_binders(
                &[("q", ID_URI)],
                &format!("for (@x <- q) {{ @\"{OUT}\"!([\"dangling\", x]) }}"),
            ),
            name.clone(),
            Some("DanglingName"),
        ),
        2 => (
            format!(
                "new op(`rho:reified:internal:handleOp`), h(`{ID_URI}`), ret in {{ \
                 op!(*h, [*ret, \"gensym\"]) }}"
            ),
            handle.clone(),
            Some("DanglingHandle"),
        ),
        _ => (
            format!(
                "new gsa(`rho:space:getSpaceAgent`), h(`{ID_URI}`), ret in {{ \
                   gsa!(*ret, *h) | for (@reply <- ret) {{ @\"rrs-p4-agent\"!(reply) }} }}"
            ),
            handle.clone(),
            None,
        ),
    };
    let spec = DeploySpec::new(program, vec![(ID_URI.to_string(), injected)], &seed);
    let mut results = Vec::with_capacity(2);
    for _ in 0..2 {
        let mut node = Node::new().await;
        let start = node.play.get_root().await;
        let played = node.deploy(&spec).await;
        prop_assert_eq!(
            played.codes(),
            expected
                .map(|code| vec![code.to_string()])
                .unwrap_or_default()
        );
        let replayed = node.replay(&start, &spec, &played).await;
        prop_assert_eq!(error_codes(&replayed.errors), played.codes());
        prop_assert_eq!(replayed.cost.value, played.result.cost.value);
        let key = CollectionKey::of_channel(&name).expect("a name has a key");
        let data = gprivate(namespace::data_channel(space, &key));
        prop_assert!(node.play.get_data(&data).await.is_empty());
        prop_assert!(node.play.get_continuations(vec![data]).await.is_empty());
        if expected.is_none() {
            // Changed by P7 step 1 (design v3 DG-3): the detail map names the space id.
            // prop_assert_eq!(node.values("rrs-p4-agent").await, vec![typed_error(
            //     SpaceReplyCode::DanglingHandle,
            //     Vec::new()
            // )]);
            prop_assert_eq!(node.values("rrs-p4-agent").await, vec![typed_error(
                SpaceReplyCode::DanglingHandle,
                vec![("spaceId", new_gbytearray_par(sid.to_vec(), Vec::new(), false))]
            )]);
        }
        results.push((played.codes(), played.result.cost.value));
    }
    prop_assert_eq!(&results[0], &results[1]);
    Ok(())
}

/// `DurableSurvivesRestart` (NativeHandles): a checkpoint and a restart over the same store
/// keep the handle continuation and the config record. They also keep the records of a
/// collection and its counter. The handle answers, the records leave in FIFO order, and a
/// new record takes the next ord.
#[test]
fn prop_handle_survives_checkpoint_reset() {
    check(
        runtime_config(),
        (vec(0i64..10, 1..4), 0i64..10, vec(any::<u8>(), 4)),
        |(values, extra, seed)| {
            tokio_runtime().block_on(handle_survives_checkpoint_reset_case(values, extra, seed))
        },
    );
}

/// One case of `prop_handle_survives_checkpoint_reset`.
async fn handle_survives_checkpoint_reset_case(
    values: Vec<i64>,
    extra: i64,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let space = node.create_space(&[&seed[..], b"-space"].concat()).await;
    let names = vec![space_name(space, "restart")];
    for (index, value) in values.iter().enumerate() {
        let send = SlotOp::Send {
            slot: 0,
            value: *value,
            persistent: false,
        };
        let deployed = node
            .deploy(&ops_deploy(
                &[send],
                &names,
                OUT,
                &[&seed[..], b"-send-", &index.to_be_bytes()].concat(),
            ))
            .await;
        prop_assert!(
            deployed.result.errors.is_empty(),
            "{:?}",
            deployed.result.errors
        );
    }
    let root = node.checkpoint().await;
    node.restart().await;
    node.reset(&root).await;
    let handles = node
        .play
        .get_continuations(vec![gprivate(namespace::handle_channel(space))])
        .await;
    prop_assert_eq!(handles.len(), 1);
    prop_assert!(handles[0].persist);
    let config = DeploySpec::new(
        format!(
            "new h(`{HANDLE_URI}`), ret in {{ h!(*ret, \"config\") | \
             for (@config <- ret) {{ @\"rrs-p4-config\"!(config) }} }}"
        ),
        vec![(
            HANDLE_URI.to_string(),
            gprivate(namespace::handle_channel(space)),
        )],
        &[&seed[..], b"-config"].concat(),
    );
    let answered = node.deploy(&config).await;
    prop_assert!(
        answered.result.errors.is_empty(),
        "{:?}",
        answered.result.errors
    );
    let configs = node.values("rrs-p4-config").await;
    prop_assert_eq!(configs.len(), 1);
    prop_assert_eq!(
        SpaceConfig::from_par(&configs[0]),
        Ok(SpaceConfig::new(
            space.sid,
            "hashmap",
            "queue",
            ConfigQualifier::Default
        ))
    );
    let restored = node.queue(space, &names[0]).await;
    let count = i64::try_from(values.len()).expect("few values");
    prop_assert_eq!(restored.counter, Some(count));
    prop_assert_eq!(
        restored
            .records
            .iter()
            .map(|record| record.value)
            .collect::<Vec<_>>(),
        values.clone()
    );
    let send = SlotOp::Send {
        slot: 0,
        value: extra,
        persistent: false,
    };
    let deployed = node
        .deploy(&ops_deploy(
            &[send],
            &names,
            OUT,
            &[&seed[..], b"-extra"].concat(),
        ))
        .await;
    prop_assert!(
        deployed.result.errors.is_empty(),
        "{:?}",
        deployed.result.errors
    );
    let grown = node.queue(space, &names[0]).await;
    prop_assert_eq!(
        grown
            .records
            .last()
            .map(|record| (record.ord, record.value)),
        Some((count, extra))
    );
    let mut expected = values.clone();
    expected.push(extra);
    for (index, value) in expected.iter().enumerate() {
        let label = format!("take-{index}");
        let receive = SlotOp::Receive {
            slot: 0,
            pat: Pat::Any,
            arrow: Arrow::Consume,
            label: label.clone(),
        };
        let deployed = node
            .deploy(&ops_deploy(
                &[receive],
                &names,
                OUT,
                &[&seed[..], b"-take-", &index.to_be_bytes()].concat(),
            ))
            .await;
        prop_assert!(
            deployed.result.errors.is_empty(),
            "{:?}",
            deployed.result.errors
        );
        prop_assert!(node.deliveries(OUT).await.contains(&(label, *value)));
    }
    prop_assert!(node.queue(space, &names[0]).await.records.is_empty());
    Ok(())
}

/// `HandleCallsMergeLikeContracts` (NativeHandles): handle calls on disjoint opened
/// collections, and the read-only methods, merge like contract calls. They do not conflict,
/// and the merged state is the state of both serial orders. With a join on `H(sid)` in the
/// pre-state (the control `JoinDescriptorUnsafe`), check #3 reports every handle call.
#[test]
fn prop_handle_calls_merge_like_contracts() {
    check(
        runtime_config(),
        (
            vec(vec(arb_value(), 0..3), 2),
            vec(arb_handle_call(0), 1..3),
            vec(arb_handle_call(1), 1..3),
            any::<bool>(),
            vec(any::<u8>(), 4),
        ),
        |(records, first, second, join_control, seed)| {
            tokio_runtime().block_on(handle_calls_merge_like_contracts_case(
                records,
                first,
                second,
                join_control,
                seed,
            ))
        },
    );
}

/// One case of `prop_handle_calls_merge_like_contracts`.
async fn handle_calls_merge_like_contracts_case(
    records: Vec<Vec<i64>>,
    first: Vec<HandleCall>,
    second: Vec<HandleCall>,
    join_control: bool,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut rig = MergeRig::new(&[Some(records[0].clone()), Some(records[1].clone())], &seed).await;
    let handle = gprivate(namespace::handle_channel(rig.space));
    if join_control {
        let join = DeploySpec::new(
            with_binders(
                &[("h", HANDLE_URI)],
                "for (@a <= h & @d <= @\"rrs-p4-descriptor\") { Nil }",
            ),
            vec![(HANDLE_URI.to_string(), handle.clone())],
            b"join-control",
        );
        rig.extend_setup(&join).await;
    }
    let first = handle_calls_deploy(&first, rig.space, &rig.names, b"branch-a");
    let second = handle_calls_deploy(&second, rig.space, &rig.names, b"branch-b");
    let first_run = rig.branch(std::slice::from_ref(&first)).await;
    let second_run = rig.branch(std::slice::from_ref(&second)).await;
    for run in [&first_run, &second_run] {
        prop_assert!(
            run.deployed[0].result.errors.is_empty(),
            "{:?}",
            run.deployed[0].result.errors
        );
    }
    let (conflicting, reason) = branch_conflicts(&rig, &first_run, &second_run);
    match join_control {
        true => prop_assert!(
            conflicting.contains(&stable_hash(&handle)),
            "no check #3 conflict: {:?}",
            reason
        ),
        false => {
            prop_assert!(
                conflicting.is_empty(),
                "handle calls conflict: {:?}",
                reason
            );
            let first_index = rig.index(&first_run.deployed[0].log);
            let second_index = rig.index(&second_run.deployed[0].log);
            let merged = merged_root(&rig.node.history, &rig.root, &[
                (&first_run.post, &first_index),
                (&second_run.post, &second_index),
            ]);
            let serial = rig.branch(&[first.clone(), second.clone()]).await;
            let reversed = rig.branch(&[second, first]).await;
            prop_assert_eq!(&merged, &serial.post);
            prop_assert_eq!(&merged, &reversed.post);
        }
    }
    Ok(())
}

// ─── Leaf extras ─────────────────────────────────────────────────────────────────────────────

/// The `this` of a direct call of the handle process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThisKind {
    Handle,
    Bundled,
    Dangling,
    Config,
    Name,
    Op,
}

/// The reply name of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetKind {
    Name,
    WriteBundle,
    ReadBundle,
    Text,
}

/// An argument of a message, by its role in the method table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgKind {
    Channel,
    ReadOnlyChannel,
    Payload,
    Number,
    Arrow,
    BadArrow,
    MatcherPairs,
    ForeignPairs,
    EmptyPairs,
    BodyId,
}

/// The params of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParamsKind {
    None,
    Persistent(bool),
    PersistentNumber,
    Misspelled,
    Positional,
    Malformed,
}

#[derive(Clone, Debug)]
struct MessageSpec {
    list: bool,
    ret: RetKind,
    method: Option<&'static str>,
    args: Vec<ArgKind>,
    params: ParamsKind,
}

/// The result of a call of the handle process.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DispatchOutcome {
    Dropped,
    Abort(&'static str),
    Typed(&'static str),
    Gensym,
    Qualifier,
    Config,
    Stored,
}

const DISPATCH_CHANNEL: &str = "rrs-p4-dispatch";

fn arg_par(arg: ArgKind) -> Par {
    match arg {
        ArgKind::Channel => gstring(DISPATCH_CHANNEL),
        ArgKind::ReadOnlyChannel => bundle(gstring(DISPATCH_CHANNEL), false, true),
        ArgKind::Payload => list(vec![gint(1)]),
        ArgKind::Number => gint(5),
        ArgKind::Arrow => gstring("<-"),
        ArgKind::BadArrow => gstring("->"),
        ArgKind::MatcherPairs => list(vec![tuple(vec![
            gprivate(kind_id(Kind::Matcher, Durability::Durable, &[7; 32])),
            gstring("c"),
        ])]),
        ArgKind::ForeignPairs => list(vec![tuple(vec![gint(1), gstring("c")])]),
        ArgKind::EmptyPairs => list(Vec::new()),
        ArgKind::BodyId => gprivate(kind_id(Kind::Body, Durability::Durable, &[9; 32])),
    }
}

fn params_par(params: ParamsKind) -> Option<Par> {
    let encoded = |positional: Vec<Par>, named: Vec<(&str, Par)>| {
        encode_params(&SendParams {
            positional,
            named: named
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        })
        .expect("params encode")
    };
    match params {
        ParamsKind::None => None,
        ParamsKind::Persistent(flag) => encoded(Vec::new(), vec![("persistent", gbool(flag))]),
        ParamsKind::PersistentNumber => encoded(Vec::new(), vec![("persistent", gint(1))]),
        ParamsKind::Misspelled => encoded(Vec::new(), vec![("priorty", gint(1))]),
        ParamsKind::Positional => encoded(vec![gint(1)], Vec::new()),
        ParamsKind::Malformed => Some(tuple(vec![Marker::Params.par(), gint(1), gint(2)])),
    }
}

fn ret_par(ret: RetKind, name: &Par) -> Par {
    match ret {
        RetKind::Name => name.clone(),
        RetKind::WriteBundle => bundle(name.clone(), true, false),
        RetKind::ReadBundle => bundle(name.clone(), false, true),
        RetKind::Text => gstring("ret"),
    }
}

fn message_par(message: &MessageSpec, ret: &Par) -> Par {
    if !message.list {
        return gstring("not a message");
    }
    let mut elements = vec![ret.clone(), match message.method {
        Some(method) => gstring(method),
        None => gint(7),
    }];
    elements.extend(message.args.iter().map(|arg| arg_par(*arg)));
    elements.extend(params_par(message.params));
    list(elements)
}

/// The result of a call as the method table of P4 design C.8 states it, in the order of the
/// checks of the handle process.
fn expected_dispatch(this: ThisKind, message: &MessageSpec) -> DispatchOutcome {
    match this {
        ThisKind::Handle => {}
        ThisKind::Dangling => return DispatchOutcome::Abort("DanglingHandle"),
        ThisKind::Bundled | ThisKind::Config | ThisKind::Name | ThisKind::Op => {
            return DispatchOutcome::Dropped
        }
    }
    let Some(method) = message.method.filter(|_| message.list) else {
        return DispatchOutcome::Dropped;
    };
    if !matches!(message.ret, RetKind::Name | RetKind::WriteBundle) {
        return DispatchOutcome::Dropped;
    }
    match message.params {
        ParamsKind::Malformed => return DispatchOutcome::Typed("MalformedMessage"),
        ParamsKind::Positional => return DispatchOutcome::Abort("PositionalParamsUnsupported"),
        _ => {}
    }
    let arity = match method {
        "gensym" | "qualifier" | "config" => 0,
        "produce" => 2,
        "consume" => 3,
        _ => return DispatchOutcome::Typed("UnknownMethod"),
    };
    if message.args.len() != arity {
        return DispatchOutcome::Typed("BadArgument");
    }
    let named: &[&str] = match message.params {
        ParamsKind::Persistent(_) | ParamsKind::PersistentNumber => &["persistent"],
        ParamsKind::Misspelled => &["priorty"],
        _ => &[],
    };
    let schema: &[&str] = match method {
        "produce" => &["persistent"],
        _ => &[],
    };
    if named.iter().any(|key| !schema.contains(key)) {
        return DispatchOutcome::Typed("UnknownParam");
    }
    match (method, message.args.as_slice()) {
        ("gensym", _) => DispatchOutcome::Gensym,
        ("qualifier", _) => DispatchOutcome::Qualifier,
        ("config", _) => DispatchOutcome::Config,
        ("produce", [channel, payload]) => {
            let lists = [
                ArgKind::Payload,
                ArgKind::MatcherPairs,
                ArgKind::ForeignPairs,
                ArgKind::EmptyPairs,
            ];
            match (
                *channel == ArgKind::ReadOnlyChannel,
                lists.contains(payload),
                message.params,
            ) {
                (true, _, _) | (false, false, _) | (false, true, ParamsKind::PersistentNumber) => {
                    DispatchOutcome::Typed("BadArgument")
                }
                (false, true, _) => DispatchOutcome::Stored,
            }
        }
        ("consume", [pairs, arrow, body]) => match (*arrow, *pairs, *body) {
            (ArgKind::Arrow, ArgKind::MatcherPairs | ArgKind::ForeignPairs, ArgKind::BodyId) => {
                match *pairs {
                    ArgKind::MatcherPairs => DispatchOutcome::Abort("AgentSpacesUnavailable"),
                    _ => DispatchOutcome::Typed("ForeignMatcher"),
                }
            }
            (ArgKind::Arrow, _, _) => DispatchOutcome::Typed("BadArgument"),
            _ => DispatchOutcome::Typed("InvalidArrow"),
        },
        _ => DispatchOutcome::Typed("BadArgument"),
    }
}

/// The arguments of a call of `method`. Most calls have the arity of the method and arguments
/// of the right role. Some have another count or a wrong role.
fn arb_args(method: Option<&'static str>) -> BoxedStrategy<Vec<ArgKind>> {
    let any_arg = select(vec![
        ArgKind::Channel,
        ArgKind::ReadOnlyChannel,
        ArgKind::Payload,
        ArgKind::Number,
        ArgKind::Arrow,
        ArgKind::BadArrow,
        ArgKind::MatcherPairs,
        ArgKind::ForeignPairs,
        ArgKind::EmptyPairs,
        ArgKind::BodyId,
    ]);
    let other_count = vec(any_arg, 0..4).boxed();
    match method {
        Some("gensym" | "qualifier" | "config") => {
            prop_oneof![4 => Just(Vec::new()), 1 => other_count].boxed()
        }
        Some("produce") => {
            let channel = prop_oneof![
                4 => Just(ArgKind::Channel),
                1 => Just(ArgKind::ReadOnlyChannel),
                1 => Just(ArgKind::Number),
            ];
            let payload = prop_oneof![
                4 => Just(ArgKind::Payload),
                1 => Just(ArgKind::Number),
                1 => Just(ArgKind::EmptyPairs),
            ];
            let arity = (channel, payload).prop_map(|(channel, payload)| vec![channel, payload]);
            prop_oneof![4 => arity, 1 => other_count].boxed()
        }
        Some("consume") => {
            let pairs = prop_oneof![
                2 => Just(ArgKind::MatcherPairs),
                2 => Just(ArgKind::ForeignPairs),
                1 => Just(ArgKind::EmptyPairs),
                1 => Just(ArgKind::Payload),
            ];
            let arrow = prop_oneof![4 => Just(ArgKind::Arrow), 1 => Just(ArgKind::BadArrow)];
            let body = prop_oneof![4 => Just(ArgKind::BodyId), 1 => Just(ArgKind::Number)];
            let arity =
                (pairs, arrow, body).prop_map(|(pairs, arrow, body)| vec![pairs, arrow, body]);
            prop_oneof![4 => arity, 1 => other_count].boxed()
        }
        _ => other_count,
    }
}

fn arb_message() -> BoxedStrategy<MessageSpec> {
    let ret = prop_oneof![
        8 => Just(RetKind::Name),
        3 => Just(RetKind::WriteBundle),
        1 => Just(RetKind::ReadBundle),
        1 => Just(RetKind::Text),
    ];
    let method = prop_oneof![
        12 => select(vec!["gensym", "qualifier", "config", "produce", "consume"]).prop_map(Some),
        2 => select(vec!["gensm", "producer", "x"]).prop_map(Some),
        1 => Just(None),
    ];
    let params = prop_oneof![
        8 => Just(ParamsKind::None),
        2 => any::<bool>().prop_map(ParamsKind::Persistent),
        1 => Just(ParamsKind::PersistentNumber),
        1 => Just(ParamsKind::Misspelled),
        1 => Just(ParamsKind::Positional),
        1 => Just(ParamsKind::Malformed),
    ];
    (proptest::bool::weighted(0.95), ret, method, params)
        .prop_flat_map(|(is_list, ret, method, params)| {
            arb_args(method).prop_map(move |args| MessageSpec {
                list: is_list,
                ret,
                method,
                args,
                params,
            })
        })
        .boxed()
}

fn arb_this() -> BoxedStrategy<ThisKind> {
    prop_oneof![
        14 => Just(ThisKind::Handle),
        1 => Just(ThisKind::Bundled),
        1 => Just(ThisKind::Dangling),
        1 => Just(ThisKind::Config),
        1 => Just(ThisKind::Name),
        1 => Just(ThisKind::Op),
    ]
    .boxed()
}

/// The tuple elements of a typed error reply: the tag and the code.
fn typed_code(reply: &Par) -> Option<String> {
    match reply.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::ETupleBody(tuple)),
        }] => match tuple.ps.as_slice() {
            [tag, code, _detail] if string_of(tag) == Some(SPACE_ERROR_REPLY_TAG) => {
                string_of(code).map(str::to_string)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The handle process is total (leaf `rrs-agent-native-handle-methods`). Every call of
/// `HANDLE_OP`, whatever its `this` and its message, ends in exactly the result of the
/// method table. The result is a drop, a typed error reply with its code, the value of the
/// method, or an abort with its code. The checks run in the order of P4 design C.8 (A6,
/// A22, A23).
#[test]
fn prop_handle_dispatch_total() {
    check(
        runtime_config(),
        (vec((arb_this(), arb_message()), 6), vec(any::<u8>(), 4)),
        |(calls, seed)| tokio_runtime().block_on(handle_dispatch_total_case(calls, seed)),
    );
}

/// One case of `prop_handle_dispatch_total`.
async fn handle_dispatch_total_case(
    calls: Vec<(ThisKind, MessageSpec)>,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let space = node.create_space(&[&seed[..], b"-space"].concat()).await;
    let dangling = NativeSpace {
        sid: [0xD0; SID_LEN],
        durability: Durability::Durable,
    };
    for (index, (this, message)) in calls.iter().enumerate() {
        let index_byte = u8::try_from(index).expect("few calls");
        let name = gprivate(vec![0x60 + index_byte; SID_LEN]);
        let ret = ret_par(message.ret, &name);
        let target = match this {
            ThisKind::Handle | ThisKind::Bundled => gprivate(namespace::handle_channel(space)),
            ThisKind::Dangling => gprivate(namespace::handle_channel(dangling)),
            ThisKind::Config => gprivate(namespace::config_channel(space)),
            ThisKind::Name => space_name(space, "dispatch"),
            ThisKind::Op => gprivate(namespace::op_channel(0x01)),
        };
        let this_process = match this {
            ThisKind::Bundled => "bundle+{*t}",
            _ => "*t",
        };
        let program = format!(
            "new op(`rho:reified:internal:handleOp`), t(`{ID_URI}`), m(`{MESSAGE_URI}`) in {{ \
             op!({this_process}, *m) }}"
        );
        let injected = vec![
            (ID_URI.to_string(), target),
            (MESSAGE_URI.to_string(), message_par(message, &ret)),
        ];
        let records_before = node.payloads(space, &gstring(DISPATCH_CHANNEL)).await.len();
        let deployed = node
            .deploy(&DeploySpec::new(
                program,
                injected,
                &[&seed[..], b"-call-", &[index_byte]].concat(),
            ))
            .await;
        let replies = node.data_of(&ret).await;
        let expected = expected_dispatch(*this, message);
        let context = format!("{this:?} {message:?}");
        match &expected {
            DispatchOutcome::Abort(code) => {
                prop_assert_eq!(deployed.codes(), vec![code.to_string()], "{}", context);
                prop_assert!(replies.is_empty(), "{}", context);
            }
            DispatchOutcome::Dropped => {
                prop_assert!(
                    deployed.result.errors.is_empty(),
                    "{}: {:?}",
                    context,
                    deployed.result.errors
                );
                prop_assert!(replies.is_empty(), "{}: {:?}", context, replies);
            }
            other => {
                prop_assert!(
                    deployed.result.errors.is_empty(),
                    "{}: {:?}",
                    context,
                    deployed.result.errors
                );
                prop_assert_eq!(replies.len(), 1, "{}", context);
                let [reply] = replies[0].as_slice() else {
                    panic!("a reply has one par: {:?}", replies[0]);
                };
                match other {
                    DispatchOutcome::Typed(code) => {
                        prop_assert_eq!(typed_code(reply), Some(code.to_string()), "{}", context)
                    }
                    DispatchOutcome::Gensym => {
                        prop_assert_eq!(
                            classify_channel(reply),
                            ChannelClass::SpaceName(space),
                            "{}",
                            context
                        )
                    }
                    DispatchOutcome::Qualifier => {
                        prop_assert_eq!(reply, &gstring("default"), "{}", context)
                    }
                    DispatchOutcome::Config => prop_assert_eq!(
                        SpaceConfig::from_par(reply),
                        Ok(SpaceConfig::new(
                            space.sid,
                            "hashmap",
                            "queue",
                            ConfigQualifier::Default
                        )),
                        "{}",
                        context
                    ),
                    DispatchOutcome::Stored => {
                        prop_assert_eq!(reply, &Par::default(), "{}", context);
                        if message.args[0] == ArgKind::Channel {
                            prop_assert_eq!(
                                node.payloads(space, &gstring(DISPATCH_CHANNEL)).await.len(),
                                records_before + 1,
                                "{}",
                                context
                            );
                        }
                    }
                    DispatchOutcome::Dropped | DispatchOutcome::Abort(_) => {}
                }
            }
        }
    }
    Ok(())
}

/// `getSpaceAgent` (leaf `rrs-agent-native-getspaceagent-capability`, plan decision 9.4,
/// A22). For a native handle, bare or under a writable bundle, the reply is the factory
/// capability of the family. It is the same `Par` as the urn map value of
/// `rho:space:hashmap:queue`. Every other reference gets the typed reply of its code.
#[test]
fn prop_get_space_agent_returns_family_capability() {
    check(
        runtime_config(),
        (0u8..8, vec(any::<u8>(), 4)),
        |(kind, seed)| {
            tokio_runtime().block_on(get_space_agent_returns_family_capability_case(kind, seed))
        },
    );
}

/// One case of `prop_get_space_agent_returns_family_capability`.
async fn get_space_agent_returns_family_capability_case(
    kind: u8,
    seed: Vec<u8>,
) -> Result<(), TestCaseError> {
    let mut node = Node::new().await;
    let space = node.create_space(&[&seed[..], b"-space"].concat()).await;
    let dangling = NativeSpace {
        sid: [0xD1; SID_LEN],
        durability: Durability::Durable,
    };
    let (reference, target, expected) = match kind {
        0 => ("*t", gprivate(namespace::handle_channel(space)), None),
        1 => (
            "bundle+{*t}",
            gprivate(namespace::handle_channel(space)),
            None,
        ),
        2 => (
            "bundle-{*t}",
            gprivate(namespace::handle_channel(space)),
            Some(SpaceReplyCode::HandleNotWritable),
        ),
        3 => (
            "bundle0{*t}",
            gprivate(namespace::handle_channel(space)),
            Some(SpaceReplyCode::HandleNotWritable),
        ),
        4 => (
            "*t",
            gprivate(namespace::handle_channel(dangling)),
            Some(SpaceReplyCode::DanglingHandle),
        ),
        5 => (
            "*t",
            gprivate(namespace::config_channel(space)),
            Some(SpaceReplyCode::NotASpaceRef),
        ),
        6 => (
            "\"agent\"",
            gprivate(namespace::handle_channel(space)),
            Some(SpaceReplyCode::NotANativeSpace),
        ),
        _ => (
            "Nil",
            gprivate(namespace::handle_channel(space)),
            Some(SpaceReplyCode::EmptySpaceRef),
        ),
    };
    let program = format!(
        "new gsa(`rho:space:getSpaceAgent`), factory(`rho:space:hashmap:queue`), \
         t(`{ID_URI}`), ret in {{ gsa!(*ret, {reference}) | \
         for (@reply <- ret) {{ @\"rrs-p4-agent\"!([reply, *factory]) }} }}"
    );
    let deployed = node
        .deploy(&DeploySpec::new(
            program,
            vec![(ID_URI.to_string(), target)],
            &seed,
        ))
        .await;
    prop_assert!(
        deployed.result.errors.is_empty(),
        "{:?}",
        deployed.result.errors
    );
    let answers = node.values("rrs-p4-agent").await;
    prop_assert_eq!(answers.len(), 1);
    let (reply, urn_value) = match answers[0].exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
        }] => match ps.as_slice() {
            [reply, urn_value] => (reply.clone(), urn_value.clone()),
            other => panic!("an answer has two elements, not {}", other.len()),
        },
        other => panic!("an answer is a list: {other:?}"),
    };
    let capability =
        factory_capability(SpaceFactoryUrn::HASHMAP_QUEUE).expect("the family has a factory");
    prop_assert_eq!(&urn_value, &capability);
    match expected {
        None => prop_assert_eq!(&reply, &capability),
        Some(code) => prop_assert_eq!(&reply, &typed_error(code, Vec::new())),
    }
    Ok(())
}

/// The `qualifier` round trip (leaf `rrs-spaces-port-core-queue-discipline`, the qualifier
/// type of P4 design B.8). The names of the qualifiers parse back, and no other name parses.
/// The factory and config qualifiers agree, and the properties follow the qualifier table.
#[test]
fn prop_qualifier_parse_roundtrip() {
    check(
        pure_config(),
        prop_oneof![
            "[a-z]{0,8}",
            select(vec!["default", "temp", "seq"]).prop_map(String::from)
        ],
        |name| {
            let config_qualifiers = [
                ConfigQualifier::Default,
                ConfigQualifier::Temp,
                ConfigQualifier::Seq,
            ];
            for (factory, config) in SpaceQualifier::ALL.into_iter().zip(config_qualifiers) {
                prop_assert_eq!(SpaceQualifier::parse(factory.as_str()), Some(factory));
                prop_assert_eq!(ConfigQualifier::parse(config.as_str()), Some(config));
                prop_assert_eq!(factory.as_str(), config.as_str());
                prop_assert_eq!(factory.to_string(), factory.as_str());
                prop_assert_eq!(config.to_string(), config.as_str());
                prop_assert_eq!(config.is_persistent(), config == ConfigQualifier::Default);
                prop_assert_eq!(config.is_mobile(), config != ConfigQualifier::Seq);
                prop_assert_eq!(config.is_concurrent(), config != ConfigQualifier::Seq);
            }
            let known = ["default", "temp", "seq"].contains(&name.as_str());
            prop_assert_eq!(SpaceQualifier::parse(&name).is_some(), known);
            prop_assert_eq!(ConfigQualifier::parse(&name).is_some(), known);
            if let (Some(factory), Some(config)) =
                (SpaceQualifier::parse(&name), ConfigQualifier::parse(&name))
            {
                prop_assert_eq!(factory.as_str(), name.as_str());
                prop_assert_eq!(config.as_str(), name.as_str());
            }
            prop_assert_eq!(SpaceQualifier::default(), SpaceQualifier::Default);
            prop_assert_eq!(ConfigQualifier::default(), ConfigQualifier::Default);
            Ok(())
        },
    );
}

/// `Prepared::commit` is the only mutation (leaf `rrs-spaces-port-core-queue-discipline`, the
/// agent of P4 design B.8). A prepared operation changes nothing while it is open or after it
/// is dropped. Its commit gives the outcome, the events and the collection that `run_op`
/// gives on the same collection.
#[test]
fn prop_prepare_commit_atomic() {
    check(
        pure_config(),
        (arb_queue_state(5), arb_queue_op()),
        |(state, op)| tokio_runtime().block_on(prepare_commit_atomic_case(state, op)),
    );
}

/// One case of `prop_prepare_commit_atomic`.
async fn prepare_commit_atomic_case(state: QueueState, op: QueueOp) -> Result<(), TestCaseError> {
    let op = aimed(&state, op);
    let rig = AgentRig::of(&state).await;
    let before = rig.view().await;
    let prepare = |rig: &AgentRig| {
        let space_op = rig.space_op(&op, 1);
        let env = rig.env.clone();
        async move {
            match space_op {
                SpaceOp::Produce(produce) => {
                    QueueAgent::default().prepare_produce(&env, produce).await
                }
                SpaceOp::Consume(consume) => {
                    QueueAgent::default().prepare_consume(&env, consume).await
                }
                SpaceOp::Settle(settle) => QueueAgent::default().prepare_settle(&env, settle).await,
                SpaceOp::Join(join) => QueueAgent::default().prepare_join(&env, join).await,
            }
        }
    };
    let dropped = prepare(&rig).await.expect("the operation prepares");
    prop_assert_eq!(&rig.view().await, &before);
    drop(dropped);
    prop_assert_eq!(&rig.view().await, &before);
    prop_assert!(rig.host.take_event_log().await.is_empty());
    let committed = prepare(&rig)
        .await
        .expect("the operation prepares")
        .commit()
        .await;
    let committed_view = rig.view().await;
    let committed_log = rig.host.take_event_log().await;
    let other = AgentRig::of(&state).await;
    let ran = other.run(&op, 1).await;
    prop_assert_eq!(&committed, &ran);
    prop_assert_eq!(committed_view, other.view().await);
    prop_assert_eq!(committed_log, other.host.take_event_log().await);
    Ok(())
}
