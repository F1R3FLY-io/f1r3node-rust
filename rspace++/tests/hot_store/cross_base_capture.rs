use std::cell::Cell;

use super::*;
use crate::rspace::persistence::{
    CatalogPackStateV1, INDEX_HEADER_LEN_V1, LogPackAddress, LogPackSequence, ObjectDigest,
    PACK_HEADER_LEN_V1, PackId, PhysicalDigest, SemanticProfileId, StoreId,
    TRANSACTION_COMMIT_RECORD_LEN_V1,
};

type Store = InMemHotStore<String, String, String, String>;
type Cache = HotStoreState<String, String, String, String>;
type Continuation = WaitingContinuation<String, String>;
type Marker = (Arc<JournalCaptureIdentityV1>, JournalVersionV1, CatalogSlotV1);

const CHANNELS: [&str; 2] = ["a", "b"];
const VALUES: [&str; 3] = ["v0", "v1", "v2"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[&str]; 3] = [&["a"], &["a", "b"], &["b", "a"]];
const STORE_SEED: u8 = 7;
const PROFILE_SEED: u8 = 1;
const ORIGIN_SEED: u8 = 0;
const FOREIGN_ORIGIN_SEED: u8 = 1;
const OTHER_STATE_SEED: u8 = 100;
const SUCCESSOR_SEED: u8 = 200;
const MANIFEST_LEN: u64 = 32;
const SLOT_STRIDE: u64 = 64;
const SUCCESSOR_CASES: u32 = 256;
const REFUSAL_CASES: u32 = 256;
const OBSERVATION_CASES: u32 = 256;

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn manifest(digest_seed: u8, encoded_len: u64) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&[digest_seed]), encoded_len)
        .expect("a synthetic manifest is at least one object envelope header long")
}

fn origin_manifest() -> ObjectRefV1 { manifest(ORIGIN_SEED, MANIFEST_LEN) }

fn successor_manifest(publication: u8) -> ObjectRefV1 {
    manifest(
        SUCCESSOR_SEED
            .checked_add(publication)
            .expect("a case publishes fewer than 56 successors"),
        MANIFEST_LEN,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotShape {
    store: u8,
    profile: u8,
    epoch: u64,
    generation: u64,
    sequence: u64,
    manifest: ObjectRefV1,
}

impl SlotShape {
    fn at(manifest: ObjectRefV1, position: u64) -> Self {
        Self {
            store: STORE_SEED,
            profile: PROFILE_SEED,
            epoch: position + 1,
            generation: position,
            sequence: position + 1,
            manifest,
        }
    }

    fn later(self, gap: u64, manifest: ObjectRefV1) -> Self {
        Self {
            epoch: self.epoch + gap,
            generation: self.generation + gap,
            sequence: self.sequence + gap,
            manifest,
            ..self
        }
    }

    fn successor(self, manifest: ObjectRefV1) -> Self { self.later(1, manifest) }

    fn slot(self) -> CatalogSlotV1 {
        let address = LogPackAddress::from_pack(
            PackId::GENESIS,
            PACK_HEADER_LEN_V1 + SLOT_STRIDE * self.sequence,
            TRANSACTION_COMMIT_RECORD_LEN_V1,
        )
        .expect("a synthetic commit record has an address in the genesis pack");
        let pack = CatalogPackStateV1::new(
            1,
            1,
            address.end(),
            1,
            INDEX_HEADER_LEN_V1,
            self.manifest,
            SLOT_STRIDE,
            SLOT_STRIDE,
        )
        .expect("a synthetic pack state is well formed");
        CatalogSlotV1::new(
            StoreId::from_array([self.store; 16]),
            self.epoch,
            StateVersionV1::new(self.generation, self.manifest),
            SemanticProfileId::from_array([self.profile; 16]),
            PhysicalDigest::of_bytes(&self.sequence.to_le_bytes()),
            LogPackSequence::new(self.sequence).expect("a synthetic commit sequence is nonzero"),
            address,
            pack,
        )
        .expect("a synthetic catalog slot is well formed")
    }
}

#[derive(Clone, Debug)]
struct BaseRecords {
    data: Vec<(usize, usize)>,
    continuations: Vec<(usize, usize)>,
}

fn rich_base() -> BaseRecords {
    BaseRecords {
        data: vec![(0, 0), (0, 0), (1, 1)],
        continuations: vec![(0, 1), (1, 0)],
    }
}

fn empty_base() -> BaseRecords {
    BaseRecords {
        data: Vec::new(),
        continuations: Vec::new(),
    }
}

fn base_cache(records: &BaseRecords) -> Cache {
    let mut cache = Cache {
        continuations: HashMap::with_capacity(GROUPS.len()),
        installed_continuations: HashMap::new(),
        data: HashMap::with_capacity(CHANNELS.len()),
        joins: HashMap::with_capacity(CHANNELS.len()),
        installed_joins: HashMap::new(),
    };
    for &(channel_index, value_index) in &records.data {
        let name = CHANNELS[channel_index];
        cache
            .data
            .entry(channel(name))
            .or_default()
            .push(datum(name, VALUES[value_index]));
    }
    for &(group_index, body_index) in &records.continuations {
        let names = GROUPS[group_index];
        cache
            .continuations
            .entry(group(names))
            .or_default()
            .push(continuation(names, BODIES[body_index]));
    }
    for names in GROUPS {
        if !cache.continuations.contains_key(&group(names)) {
            continue;
        }
        for member in names.iter().collect::<BTreeSet<_>>() {
            cache
                .joins
                .entry(channel(member))
                .or_default()
                .push(group(names));
        }
    }
    cache
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalStart {
    Detached,
    Bound,
}

fn store_over(
    records: &BaseRecords,
    origin: ObjectRefV1,
    registry: &GenerationLeaseRegistryV1,
) -> Store {
    let state = InMemHotStoreState {
        durable_journal: Some(Box::new(DurableJournalStateV1::detached(DurableJournalBaseV1 {
            manifest: origin,
            registry: registry.clone(),
        }))),
        ..InMemHotStoreState::from_snapshot(base_cache(records))
            .expect("the base records fit the multiplicity model")
    };
    InMemHotStore {
        state: std::sync::RwLock::new(state),
        history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
        history_reader_base: Box::new(ClosedDurableHistoryReader),
    }
}

fn start_journal(
    store: &Store,
    start: JournalStart,
    origin_slot: SlotShape,
) -> Result<(), TestCaseError> {
    match start {
        JournalStart::Detached => Ok(()),
        JournalStart::Bound => {
            prop_assert_eq!(store.bind_journal_authority(origin_slot.slot()), Ok(()));
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum JournalOperation {
    PutDatum {
        channel_index: usize,
        value_index: usize,
    },
    RemoveDatum {
        channel_index: usize,
        selector: usize,
    },
    PutContinuation {
        group_index: usize,
        body_index: usize,
    },
    RemoveContinuation {
        group_index: usize,
        selector: usize,
    },
}

#[derive(Debug, PartialEq)]
enum Answer {
    DatumPut,
    DatumRemoved(Result<(), RSpaceError>),
    ContinuationPut(Option<bool>),
    ContinuationRemoved(Option<()>),
}

fn removal_index(store: &Store, operation: JournalOperation) -> i32 {
    let (length, selector) = match operation {
        JournalOperation::RemoveDatum {
            channel_index,
            selector,
        } => (store.get_data(&channel(CHANNELS[channel_index])).len(), selector),
        JournalOperation::RemoveContinuation {
            group_index,
            selector,
        } => (store.get_continuations(&group(GROUPS[group_index])).len(), selector),
        JournalOperation::PutDatum { .. } | JournalOperation::PutContinuation { .. } => (0, 0),
    };
    i32::try_from(selector.checked_rem(length).unwrap_or(0)).expect("a removal index fits i32")
}

fn apply_operation(store: &Store, operation: JournalOperation, index: i32) -> Answer {
    match operation {
        JournalOperation::PutDatum {
            channel_index,
            value_index,
        } => {
            let name = CHANNELS[channel_index];
            store.put_datum(&channel(name), datum(name, VALUES[value_index]));
            Answer::DatumPut
        }
        JournalOperation::RemoveDatum { channel_index, .. } => {
            Answer::DatumRemoved(store.remove_datum(&channel(CHANNELS[channel_index]), index))
        }
        JournalOperation::PutContinuation {
            group_index,
            body_index,
        } => {
            let names = GROUPS[group_index];
            let channels = group(names);
            let inserted =
                store.put_continuation(&channels, continuation(names, BODIES[body_index]));
            for member in channels.iter().collect::<BTreeSet<_>>() {
                store
                    .put_join(member, &channels)
                    .expect("the hot store takes every join");
            }
            Answer::ContinuationPut(inserted)
        }
        JournalOperation::RemoveContinuation { group_index, .. } => {
            let channels = group(GROUPS[group_index]);
            let removed = store.remove_continuation(&channels, index);
            if let Some(()) = removed {
                for member in channels.iter().collect::<BTreeSet<_>>() {
                    store
                        .remove_join(member, &channels)
                        .expect("the hot store answers every join removal");
                }
            }
            Answer::ContinuationRemoved(removed)
        }
    }
}

fn apply_operations(store: &Store, operations: &[JournalOperation]) {
    for &operation in operations {
        let index = removal_index(store, operation);
        apply_operation(store, operation, index);
    }
}

fn same_paths<V: Clone + Send + Sync + Unpin + PartialEq>(
    left: &Option<PathMap<V>>,
    right: &Option<PathMap<V>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.iter().eq(right.iter()),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn same_pending(left: &PendingDeltas, right: &PendingDeltas) -> bool {
    left.data == right.data &&
        left.continuations == right.continuations &&
        left.joins == right.joins &&
        same_paths(&left.continuation_content, &right.continuation_content) &&
        same_paths(&left.continuation_source_bytes, &right.continuation_source_bytes)
}

fn same_observation(left: &PendingMutationObservation, right: &PendingMutationObservation) -> bool {
    same_pending(&left.pending, &right.pending) &&
        left.data_counts.iter().eq(right.data_counts.iter()) &&
        left.join_counts.iter().eq(right.join_counts.iter())
}

fn journal_is_empty(pending: &PendingDeltas) -> bool {
    let bag_is_empty = |delta: &Option<BagDelta>| delta.as_ref().is_some_and(BagDelta::is_empty);
    bag_is_empty(&pending.data) &&
        bag_is_empty(&pending.continuations) &&
        bag_is_empty(&pending.joins) &&
        pending
            .continuation_content
            .as_ref()
            .is_some_and(|content| content.is_empty()) &&
        pending
            .continuation_source_bytes
            .as_ref()
            .is_some_and(|sources| sources.is_empty())
}

fn removes_from(delta: &Option<BagDelta>) -> bool {
    delta
        .as_ref()
        .is_some_and(|delta| !delta.removals().is_empty())
}

fn same_records(left: &Cache, right: &Cache) -> bool {
    left.data == right.data &&
        left.continuations == right.continuations &&
        left.installed_continuations == right.installed_continuations &&
        left.joins == right.joins &&
        left.installed_joins == right.installed_joins
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordedBase {
    Detached(ObjectRefV1),
    Bound(CatalogSlotV1),
    Invalidated,
}

struct JournalCondition {
    base: RecordedBase,
    version: JournalVersionV1,
    marker: Option<Marker>,
    pending: PendingDeltas,
    records: Cache,
}

fn condition_of(store: &Store) -> JournalCondition {
    let pending = store
        .pending_deltas()
        .expect("the hot store exposes its journals");
    let records = store.snapshot();
    let state = store.state.read().expect("hot store state read lock");
    let journal = state
        .durable_journal
        .as_ref()
        .expect("the store keeps its durable journal");
    JournalCondition {
        base: match journal.base {
            JournalBaseV1::Detached(recorded) => RecordedBase::Detached(recorded),
            JournalBaseV1::Bound(slot) => RecordedBase::Bound(slot),
            JournalBaseV1::Invalidated => RecordedBase::Invalidated,
        },
        version: journal.version.clone(),
        marker: journal
            .acknowledged
            .as_ref()
            .map(|marker| (Arc::clone(&marker.identity), marker.version.clone(), marker.terminal)),
        pending,
        records,
    }
}

fn same_marker(left: &Option<Marker>, right: &Option<Marker>) -> bool {
    match (left, right) {
        (
            Some((left_identity, left_version, left_terminal)),
            Some((right_identity, right_version, right_terminal)),
        ) => {
            Arc::ptr_eq(left_identity, right_identity) &&
                left_version.matches(right_version) &&
                left_terminal == right_terminal
        }
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn check_unchanged(
    store: &Store,
    before: &JournalCondition,
    action: &str,
) -> Result<(), TestCaseError> {
    let after = condition_of(store);
    prop_assert_eq!(after.base, before.base, "{} changed the journal's base", action);
    prop_assert!(
        after.version.matches(&before.version),
        "{} changed the journal's version from revision {} to revision {}",
        action,
        before.version.revision,
        after.version.revision
    );
    prop_assert!(
        same_marker(&after.marker, &before.marker),
        "{} changed the acknowledgement marker",
        action
    );
    prop_assert!(
        same_pending(&after.pending, &before.pending),
        "{} changed the pending journals: {:?} against {:?}",
        action,
        after.pending,
        before.pending
    );
    prop_assert!(
        same_records(&after.records, &before.records),
        "{} changed the store's records",
        action
    );
    Ok(())
}

fn is_conflict<T>(outcome: &Result<T, RSpaceError>) -> bool {
    matches!(outcome, Err(RSpaceError::JournalConflict(_)))
}

fn refusal_of<T>(outcome: Result<T, RSpaceError>) -> Result<(), RSpaceError> { outcome.map(|_| ()) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationState {
    Other(u8),
    OfOrigin,
}

impl PublicationState {
    fn manifest(self, origin: ObjectRefV1) -> ObjectRefV1 {
        match self {
            Self::Other(seed) => manifest(OTHER_STATE_SEED + seed, MANIFEST_LEN),
            Self::OfOrigin => origin,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PublicationChoice {
    gap: u64,
    state: PublicationState,
}

fn journal_starts() -> impl Strategy<Value = JournalStart> {
    prop_oneof![Just(JournalStart::Detached), Just(JournalStart::Bound)]
}

fn base_records() -> impl Strategy<Value = BaseRecords> {
    (
        prop::collection::vec((0..CHANNELS.len(), 0..VALUES.len()), 0..5),
        prop::collection::btree_set((0..GROUPS.len(), 0..BODIES.len()), 0..4),
    )
        .prop_map(|(data, continuations)| BaseRecords {
            data,
            continuations: continuations.into_iter().collect(),
        })
}

fn journal_operations() -> impl Strategy<Value = JournalOperation> {
    prop_oneof![
        (0..CHANNELS.len(), 0..VALUES.len()).prop_map(|(channel_index, value_index)| {
            JournalOperation::PutDatum {
                channel_index,
                value_index,
            }
        }),
        (0..CHANNELS.len(), 0..4_usize).prop_map(|(channel_index, selector)| {
            JournalOperation::RemoveDatum {
                channel_index,
                selector,
            }
        }),
        (0..GROUPS.len(), 0..BODIES.len()).prop_map(|(group_index, body_index)| {
            JournalOperation::PutContinuation {
                group_index,
                body_index,
            }
        }),
        (0..GROUPS.len(), 0..4_usize).prop_map(|(group_index, selector)| {
            JournalOperation::RemoveContinuation {
                group_index,
                selector,
            }
        }),
    ]
}

fn publication_choices() -> impl Strategy<Value = PublicationChoice> {
    (1..4_u64, prop_oneof![
        3 => (1..4_u8).prop_map(PublicationState::Other),
        1 => Just(PublicationState::OfOrigin),
    ])
        .prop_map(|(gap, state)| PublicationChoice { gap, state })
}

#[derive(Clone, Debug)]
struct SuccessorCase {
    base: BaseRecords,
    start: JournalStart,
    operations: Vec<JournalOperation>,
    publication: PublicationChoice,
}

fn seed_successor_cases() -> [SuccessorCase; 3] {
    [
        SuccessorCase {
            base: rich_base(),
            start: JournalStart::Detached,
            operations: vec![
                JournalOperation::PutDatum {
                    channel_index: 0,
                    value_index: 2,
                },
                JournalOperation::RemoveDatum {
                    channel_index: 1,
                    selector: 0,
                },
                JournalOperation::PutContinuation {
                    group_index: 2,
                    body_index: 0,
                },
                JournalOperation::RemoveContinuation {
                    group_index: 1,
                    selector: 0,
                },
            ],
            publication: PublicationChoice {
                gap: 1,
                state: PublicationState::Other(1),
            },
        },
        SuccessorCase {
            base: rich_base(),
            start: JournalStart::Bound,
            operations: vec![
                JournalOperation::RemoveDatum {
                    channel_index: 0,
                    selector: 0,
                },
                JournalOperation::PutContinuation {
                    group_index: 0,
                    body_index: 0,
                },
            ],
            publication: PublicationChoice {
                gap: 3,
                state: PublicationState::OfOrigin,
            },
        },
        SuccessorCase {
            base: empty_base(),
            start: JournalStart::Detached,
            operations: Vec::new(),
            publication: PublicationChoice {
                gap: 2,
                state: PublicationState::Other(2),
            },
        },
    ]
}

fn successor_cases() -> impl Strategy<Value = SuccessorCase> {
    (
        base_records(),
        journal_starts(),
        prop::collection::vec(journal_operations(), 0..8),
        publication_choices(),
    )
        .prop_map(|(base, start, operations, publication)| SuccessorCase {
            base,
            start,
            operations,
            publication,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct SuccessorCoverage {
    cases: u32,
    detached: u32,
    bound: u32,
    refusal_sweeps: u32,
    committed: u32,
    empty_journal_refused: u32,
    publication_of_the_origin_state: u32,
    same_base_capture_retired: u32,
}

impl SuccessorCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            detached,
            bound,
            refusal_sweeps,
            committed,
            empty_journal_refused,
            publication_of_the_origin_state,
            same_base_capture_retired,
        } = other;
        self.cases += cases;
        self.detached += detached;
        self.bound += bound;
        self.refusal_sweeps += refusal_sweeps;
        self.committed += committed;
        self.empty_journal_refused += empty_journal_refused;
        self.publication_of_the_origin_state += publication_of_the_origin_state;
        self.same_base_capture_retired += same_base_capture_retired;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            detached,
            bound,
            refusal_sweeps,
            committed,
            empty_journal_refused,
            publication_of_the_origin_state,
            same_base_capture_retired,
        } = *self;
        assert!(
            [
                cases,
                detached,
                bound,
                refusal_sweeps,
                committed,
                empty_journal_refused,
                publication_of_the_origin_state,
                same_base_capture_retired,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of cross-base acknowledgement never occurred: {self:?}"
        );
    }
}

fn wrong_terminals(
    publication: SlotShape,
    origin_slot: SlotShape,
    successor: SlotShape,
) -> [(&'static str, CatalogSlotV1); 11] {
    [
        (
            "other-store",
            SlotShape {
                store: successor.store + 1,
                ..successor
            }
            .slot(),
        ),
        (
            "other-profile",
            SlotShape {
                profile: successor.profile + 1,
                ..successor
            }
            .slot(),
        ),
        (
            "unadvanced-epoch",
            SlotShape {
                epoch: publication.epoch,
                ..successor
            }
            .slot(),
        ),
        (
            "doubly-advanced-epoch",
            SlotShape {
                epoch: publication.epoch + 2,
                ..successor
            }
            .slot(),
        ),
        (
            "unadvanced-generation",
            SlotShape {
                generation: publication.generation,
                ..successor
            }
            .slot(),
        ),
        (
            "doubly-advanced-generation",
            SlotShape {
                generation: publication.generation + 2,
                ..successor
            }
            .slot(),
        ),
        (
            "unadvanced-sequence",
            SlotShape {
                sequence: publication.sequence,
                ..successor
            }
            .slot(),
        ),
        (
            "doubly-advanced-sequence",
            SlotShape {
                sequence: publication.sequence + 2,
                ..successor
            }
            .slot(),
        ),
        ("publication-base", publication.slot()),
        ("origin-slot", origin_slot.slot()),
        ("origin-successor", origin_slot.successor(successor.manifest).slot()),
    ]
}

fn check_refused_everywhere_but_the_successor(
    store: &Store,
    captured: &CapturedHotJournalV1,
    terminals: &[(&'static str, CatalogSlotV1)],
    successor: CatalogSlotV1,
    before: &JournalCondition,
) -> Result<(), TestCaseError> {
    for &(label, wrong) in terminals {
        for (expected, admitted) in [(wrong, wrong), (successor, wrong), (wrong, successor)] {
            let outcome =
                refusal_of(store.acknowledge_pending_journal(captured, expected, admitted));
            prop_assert!(
                is_conflict(&outcome),
                "a cross-base capture was acknowledged at the {} candidate, which is not the \
                 exact successor of its publication base: {:?}",
                label,
                outcome
            );
            check_unchanged(store, before, "a refused acknowledgement")?;
        }
    }
    Ok(())
}

fn check_successor(case: &SuccessorCase) -> Result<SuccessorCoverage, TestCaseError> {
    let registry = GenerationLeaseRegistryV1::default();
    let origin = origin_manifest();
    let origin_slot = SlotShape::at(origin, 0);
    let store = store_over(&case.base, origin, &registry);
    start_journal(&store, case.start, origin_slot)?;
    apply_operations(&store, &case.operations);
    let publication =
        origin_slot.later(case.publication.gap, case.publication.state.manifest(origin));
    let successor = publication.successor(successor_manifest(0));
    let exact = successor.slot();
    let same_base = match case.start {
        JournalStart::Detached => None,
        JournalStart::Bound => Some(store.capture_pending_journal(origin_slot.slot()).map_err(
            |error| {
                TestCaseError::fail(format!("the bound journal refused its capture: {error:?}"))
            },
        )?),
    };
    let captured = store
        .capture_cross_base_journal(origin, publication.slot(), &registry)
        .map_err(|error| {
            TestCaseError::fail(format!("the journal refused its cross-base capture: {error:?}"))
        })?;
    let before = condition_of(&store);
    let terminals = wrong_terminals(publication, origin_slot, successor);
    let mut coverage = SuccessorCoverage {
        cases: 1,
        detached: u32::from(case.start == JournalStart::Detached),
        bound: u32::from(case.start == JournalStart::Bound),
        publication_of_the_origin_state: u32::from(
            case.publication.state == PublicationState::OfOrigin,
        ),
        ..SuccessorCoverage::default()
    };
    check_refused_everywhere_but_the_successor(&store, &captured, &terminals, exact, &before)?;
    coverage.refusal_sweeps += 1;
    prop_assert_eq!(store.validate_pending_journal(&captured), Ok(()));
    if journal_is_empty(&before.pending) {
        let outcome = refusal_of(store.acknowledge_pending_journal(&captured, exact, exact));
        prop_assert!(
            is_conflict(&outcome),
            "an empty cross-base capture was acknowledged at the exact successor: {:?}",
            outcome
        );
        check_unchanged(&store, &before, "the refused acknowledgement of an empty journal")?;
        coverage.empty_journal_refused = 1;
        return Ok(coverage);
    }
    prop_assert_eq!(
        store.acknowledge_pending_journal(&captured, exact, exact),
        Ok(HotJournalAcknowledgementV1::Committed)
    );
    let after = condition_of(&store);
    prop_assert_eq!(after.base, RecordedBase::Bound(exact));
    prop_assert!(
        after.version.matches(&before.version.next()),
        "the acknowledgement moved the journal from revision {} to revision {} instead of the \
         next one",
        before.version.revision,
        after.version.revision
    );
    prop_assert!(journal_is_empty(&after.pending));
    prop_assert!(same_records(&after.records, &before.records));
    prop_assert_eq!(store.validate_journal_acknowledgement(&captured, exact, &registry), Ok(()));
    prop_assert_eq!(
        store.acknowledge_pending_journal(&captured, exact, exact),
        Ok(HotJournalAcknowledgementV1::AlreadyCommitted)
    );
    check_unchanged(&store, &after, "a repeated acknowledgement")?;
    prop_assert!(store.capture_pending_journal(exact).is_ok());
    prop_assert!(
        store
            .capture_cross_base_journal(successor.manifest, publication.slot(), &registry)
            .is_ok()
    );
    prop_assert!(is_conflict(&refusal_of(store.capture_cross_base_journal(
        origin,
        publication.slot(),
        &registry
    ))));
    check_refused_everywhere_but_the_successor(&store, &captured, &terminals, exact, &after)?;
    coverage.refusal_sweeps += 1;
    if let Some(same_base) = same_base {
        let origin_successor = origin_slot.successor(successor.manifest).slot();
        let outcome = refusal_of(store.acknowledge_pending_journal(
            &same_base,
            origin_successor,
            origin_successor,
        ));
        prop_assert!(
            is_conflict(&outcome),
            "a same-base capture of the journal was acknowledged after its cross-base capture: \
             {:?}",
            outcome
        );
        check_unchanged(&store, &after, "a refused same-base acknowledgement")?;
        coverage.same_base_capture_retired = 1;
    }
    coverage.committed = 1;
    Ok(coverage)
}

#[test]
fn a_cross_base_capture_is_acknowledged_only_at_the_successor_of_its_publication_base() {
    let mut seeded = SuccessorCoverage::default();
    for case in seed_successor_cases() {
        seeded.merge(check_successor(&case).expect("a seeded case satisfies CB-01"));
    }
    println!("cross-base acknowledgement seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(SuccessorCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: SUCCESSOR_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&successor_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_successor(&case)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated cases satisfy CB-01");
    assert!(generated.get().cases >= SUCCESSOR_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("cross-base acknowledgement: {merged:?}");
    merged.assert_complete();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalChange {
    PutDatum,
    PutThenRemoveDatum,
    Restore,
    Clear,
    ReplaceState,
    AcknowledgeAnother,
    ReadAndRefuse,
}

#[derive(Clone, Debug)]
struct RefusalCase {
    base: BaseRecords,
    start: JournalStart,
    operations: Vec<JournalOperation>,
    publication: PublicationChoice,
    change: JournalChange,
}

fn seed_refusal_cases() -> [RefusalCase; 7] {
    let journal = vec![
        JournalOperation::PutDatum {
            channel_index: 1,
            value_index: 0,
        },
        JournalOperation::RemoveDatum {
            channel_index: 0,
            selector: 1,
        },
        JournalOperation::RemoveContinuation {
            group_index: 0,
            selector: 0,
        },
    ];
    let other = PublicationChoice {
        gap: 2,
        state: PublicationState::Other(3),
    };
    let of_origin = PublicationChoice {
        gap: 1,
        state: PublicationState::OfOrigin,
    };
    [
        (JournalStart::Detached, other, JournalChange::PutDatum),
        (JournalStart::Bound, other, JournalChange::PutThenRemoveDatum),
        (JournalStart::Detached, of_origin, JournalChange::Restore),
        (JournalStart::Bound, other, JournalChange::Clear),
        (JournalStart::Detached, other, JournalChange::ReplaceState),
        (JournalStart::Bound, of_origin, JournalChange::AcknowledgeAnother),
        (JournalStart::Detached, other, JournalChange::ReadAndRefuse),
    ]
    .map(|(start, publication, change)| RefusalCase {
        base: rich_base(),
        start,
        operations: journal.clone(),
        publication,
        change,
    })
}

fn journal_changes() -> impl Strategy<Value = JournalChange> {
    prop_oneof![
        Just(JournalChange::PutDatum),
        Just(JournalChange::PutThenRemoveDatum),
        Just(JournalChange::Restore),
        Just(JournalChange::Clear),
        Just(JournalChange::ReplaceState),
        Just(JournalChange::AcknowledgeAnother),
        Just(JournalChange::ReadAndRefuse),
    ]
}

fn refusal_cases() -> impl Strategy<Value = RefusalCase> {
    (
        base_records(),
        journal_starts(),
        prop::collection::vec(journal_operations(), 0..8),
        publication_choices(),
        journal_changes(),
    )
        .prop_map(|(base, start, operations, publication, change)| RefusalCase {
            base,
            start,
            operations,
            publication,
            change,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct RefusalCoverage {
    cases: u32,
    nonempty_journals: u32,
    other_digest_refused: u32,
    other_length_refused: u32,
    publication_state_refused: u32,
    foreign_registry_refused: u32,
    altered_cross_base_refused: u32,
    altered_same_base_refused: u32,
    mutation_refused: u32,
    version_only_change_refused: u32,
    invalidation_refused: u32,
    acknowledgement_refused: u32,
    unchanged_journal_kept: u32,
}

impl RefusalCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            nonempty_journals,
            other_digest_refused,
            other_length_refused,
            publication_state_refused,
            foreign_registry_refused,
            altered_cross_base_refused,
            altered_same_base_refused,
            mutation_refused,
            version_only_change_refused,
            invalidation_refused,
            acknowledgement_refused,
            unchanged_journal_kept,
        } = other;
        self.cases += cases;
        self.nonempty_journals += nonempty_journals;
        self.other_digest_refused += other_digest_refused;
        self.other_length_refused += other_length_refused;
        self.publication_state_refused += publication_state_refused;
        self.foreign_registry_refused += foreign_registry_refused;
        self.altered_cross_base_refused += altered_cross_base_refused;
        self.altered_same_base_refused += altered_same_base_refused;
        self.mutation_refused += mutation_refused;
        self.version_only_change_refused += version_only_change_refused;
        self.invalidation_refused += invalidation_refused;
        self.acknowledgement_refused += acknowledgement_refused;
        self.unchanged_journal_kept += unchanged_journal_kept;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            nonempty_journals,
            other_digest_refused,
            other_length_refused,
            publication_state_refused,
            foreign_registry_refused,
            altered_cross_base_refused,
            altered_same_base_refused,
            mutation_refused,
            version_only_change_refused,
            invalidation_refused,
            acknowledgement_refused,
            unchanged_journal_kept,
        } = *self;
        assert!(
            [
                cases,
                nonempty_journals,
                other_digest_refused,
                other_length_refused,
                publication_state_refused,
                foreign_registry_refused,
                altered_cross_base_refused,
                altered_same_base_refused,
                mutation_refused,
                version_only_change_refused,
                invalidation_refused,
                acknowledgement_refused,
                unchanged_journal_kept,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of cross-base refusal never occurred: {self:?}"
        );
    }
}

fn wrong_origins(origin: ObjectRefV1, publication: SlotShape) -> Vec<(&'static str, ObjectRefV1)> {
    let candidates = [
        ("other-digest", manifest(FOREIGN_ORIGIN_SEED, MANIFEST_LEN)),
        ("other-length", manifest(ORIGIN_SEED, MANIFEST_LEN + 1)),
        ("publication-state", publication.manifest),
    ];
    let mut wrong = Vec::with_capacity(candidates.len());
    wrong.extend(
        candidates
            .into_iter()
            .filter(|(_, candidate)| *candidate != origin),
    );
    wrong
}

fn count_wrong_origin(coverage: &mut RefusalCoverage, label: &str) {
    match label {
        "other-digest" => coverage.other_digest_refused += 1,
        "other-length" => coverage.other_length_refused += 1,
        _ => coverage.publication_state_refused += 1,
    }
}

struct RefusalFixture {
    store: Store,
    registry: GenerationLeaseRegistryV1,
    origin: ObjectRefV1,
    origin_slot: SlotShape,
    publication: SlotShape,
}

impl RefusalFixture {
    fn new(case: &RefusalCase) -> Result<Self, TestCaseError> {
        let registry = GenerationLeaseRegistryV1::default();
        let origin = origin_manifest();
        let origin_slot = SlotShape::at(origin, 0);
        let store = store_over(&case.base, origin, &registry);
        start_journal(&store, case.start, origin_slot)?;
        apply_operations(&store, &case.operations);
        Ok(Self {
            store,
            registry,
            origin,
            origin_slot,
            publication: origin_slot
                .later(case.publication.gap, case.publication.state.manifest(origin)),
        })
    }

    fn cross_base_capture(&self) -> Result<CapturedHotJournalV1, TestCaseError> {
        self.store
            .capture_cross_base_journal(self.origin, self.publication.slot(), &self.registry)
            .map_err(|error| {
                TestCaseError::fail(format!(
                    "the journal refused its cross-base capture: {error:?}"
                ))
            })
    }

    fn publication_successor(&self, publication: u8) -> CatalogSlotV1 {
        self.publication
            .successor(successor_manifest(publication))
            .slot()
    }

    fn apply_change(&self, change: JournalChange) -> Result<(), TestCaseError> {
        let store = &self.store;
        match change {
            JournalChange::PutDatum => store.put_datum(&channel("a"), datum("a", "v0")),
            JournalChange::PutThenRemoveDatum => {
                let added = datum("b", "v2");
                let path = RSpaceDataIndex::datum_path(&channel("b"), &added);
                store.put_datum(&channel("b"), added);
                store.remove_datum_path(&path).map_err(|error| {
                    TestCaseError::fail(format!(
                        "the datum just put could not be removed: {error:?}"
                    ))
                })?;
            }
            JournalChange::Restore => {
                let pending = store
                    .pending_deltas()
                    .expect("the hot store exposes its journals");
                match pending {
                    PendingDeltas {
                        data: Some(data),
                        continuations: Some(continuations),
                        continuation_content: Some(content),
                        continuation_source_bytes: Some(sources),
                        joins: Some(joins),
                    } => store
                        .restore_pending_deltas(data, continuations, content, sources, joins)
                        .map_err(|error| {
                            TestCaseError::fail(format!("the journal was not restored: {error:?}"))
                        })?,
                    missing => {
                        return Err(TestCaseError::fail(format!(
                            "the hot store withheld a journal capability: {missing:?}"
                        )));
                    }
                }
            }
            JournalChange::Clear => store.clear(),
            JournalChange::ReplaceState => store.set_state(store.snapshot()),
            JournalChange::AcknowledgeAnother => {
                let another = self.cross_base_capture()?;
                let successor = self.publication_successor(1);
                let outcome = store.acknowledge_pending_journal(&another, successor, successor);
                let empty = journal_is_empty(&another.observation.pending);
                prop_assert_eq!(
                    refusal_of(outcome).is_ok(),
                    !empty,
                    "the second capture's acknowledgement disagrees with its journal's emptiness"
                );
            }
            JournalChange::ReadAndRefuse => {
                store.get_data(&channel("a"));
                store.get_continuations(&group(GROUPS[1]));
                store.get_joins(&channel("b"));
                prop_assert!(store.remove_datum(&channel("a"), 99).is_err());
                prop_assert!(store.remove_continuation(&group(GROUPS[2]), 99).is_none());
            }
        }
        Ok(())
    }

    fn check_altered_origins(
        &self,
        start: JournalStart,
        before: &JournalCondition,
        coverage: &mut RefusalCoverage,
    ) -> Result<(), TestCaseError> {
        let store = &self.store;
        let successor = self.publication_successor(0);
        let origin_successor = self.origin_slot.successor(successor_manifest(0)).slot();
        for (label, wrong) in wrong_origins(self.origin, self.publication) {
            let mut altered = self.cross_base_capture()?;
            altered.origin = wrong;
            let validation = store.validate_pending_journal(&altered);
            prop_assert!(
                is_conflict(&validation),
                "a cross-base capture whose origin is not the journal's recorded state was \
                 validated ({} origin)",
                label
            );
            let outcome =
                refusal_of(store.acknowledge_pending_journal(&altered, successor, successor));
            prop_assert!(
                is_conflict(&outcome),
                "a cross-base capture whose origin is not the journal's recorded state was \
                 acknowledged ({} origin): {:?}",
                label,
                outcome
            );
            check_unchanged(store, before, "a capture with an altered origin")?;
            coverage.altered_cross_base_refused += 1;
            if let JournalStart::Bound = start {
                let mut altered = store
                    .capture_pending_journal(self.origin_slot.slot())
                    .map_err(|error| {
                        TestCaseError::fail(format!(
                            "the bound journal refused its capture: {error:?}"
                        ))
                    })?;
                altered.origin = wrong;
                let validation = store.validate_pending_journal(&altered);
                prop_assert!(
                    is_conflict(&validation),
                    "a same-base capture whose origin is not the journal's recorded state was \
                     validated ({} origin)",
                    label
                );
                let outcome = refusal_of(store.acknowledge_pending_journal(
                    &altered,
                    origin_successor,
                    origin_successor,
                ));
                prop_assert!(
                    is_conflict(&outcome),
                    "a same-base capture whose origin is not the journal's recorded state was \
                     acknowledged ({} origin): {:?}",
                    label,
                    outcome
                );
                check_unchanged(store, before, "a same-base capture with an altered origin")?;
                coverage.altered_same_base_refused += 1;
            }
        }
        Ok(())
    }
}

fn check_refusal(case: &RefusalCase) -> Result<RefusalCoverage, TestCaseError> {
    let fixture = RefusalFixture::new(case)?;
    let store = &fixture.store;
    let foreign = GenerationLeaseRegistryV1::default();
    let publication_base = fixture.publication.slot();
    let before = condition_of(store);
    let mut coverage = RefusalCoverage {
        cases: 1,
        nonempty_journals: u32::from(!journal_is_empty(&before.pending)),
        ..RefusalCoverage::default()
    };
    for (label, wrong) in wrong_origins(fixture.origin, fixture.publication) {
        for authority in [&fixture.registry, &foreign] {
            let refusal =
                refusal_of(store.capture_cross_base_journal(wrong, publication_base, authority));
            prop_assert!(
                is_conflict(&refusal),
                "the journal was captured for the {} origin: {:?}",
                label,
                refusal
            );
            check_unchanged(store, &before, "a capture refused for its origin")?;
        }
        count_wrong_origin(&mut coverage, label);
    }
    let refusal =
        refusal_of(store.capture_cross_base_journal(fixture.origin, publication_base, &foreign));
    prop_assert!(
        is_conflict(&refusal),
        "the journal was captured for another directory authority: {:?}",
        refusal
    );
    check_unchanged(store, &before, "a capture refused for its registry")?;
    coverage.foreign_registry_refused = 1;
    let captured = fixture.cross_base_capture()?;
    check_unchanged(store, &before, "a cross-base capture")?;
    prop_assert_eq!(store.validate_pending_journal(&captured), Ok(()));
    fixture.check_altered_origins(case.start, &before, &mut coverage)?;
    prop_assert_eq!(store.validate_pending_journal(&captured), Ok(()));
    fixture.apply_change(case.change)?;
    let changed = condition_of(store);
    if changed.version.matches(&before.version) {
        prop_assert_eq!(
            store.validate_pending_journal(&captured),
            Ok(()),
            "the {:?} change left the journal and its version unchanged but the capture was \
             refused",
            case.change
        );
        coverage.unchanged_journal_kept = 1;
        return Ok(coverage);
    }
    let validation = store.validate_pending_journal(&captured);
    prop_assert!(
        is_conflict(&validation),
        "a journal changed by {:?} after the capture was validated",
        case.change
    );
    let successor = fixture.publication_successor(0);
    let outcome = refusal_of(store.acknowledge_pending_journal(&captured, successor, successor));
    prop_assert!(
        is_conflict(&outcome),
        "a journal changed by {:?} after the capture was acknowledged: {:?}",
        case.change,
        outcome
    );
    check_unchanged(store, &changed, "the refused acknowledgement of a changed journal")?;
    match (case.change, changed.base) {
        (_, RecordedBase::Invalidated) => {
            for (_, recorded) in [("origin", fixture.origin)]
                .into_iter()
                .chain(wrong_origins(fixture.origin, fixture.publication))
            {
                let refusal = refusal_of(store.capture_cross_base_journal(
                    recorded,
                    publication_base,
                    &fixture.registry,
                ));
                prop_assert!(
                    is_conflict(&refusal),
                    "an invalidated journal was captured: {:?}",
                    refusal
                );
                check_unchanged(store, &changed, "a capture of an invalidated journal")?;
            }
            coverage.invalidation_refused = 1;
        }
        (JournalChange::AcknowledgeAnother, _) => coverage.acknowledgement_refused = 1,
        (JournalChange::Restore | JournalChange::PutThenRemoveDatum, _) => {
            prop_assert!(
                same_pending(&changed.pending, &before.pending),
                "the {:?} change left other pending journals than before",
                case.change
            );
            coverage.version_only_change_refused = 1;
        }
        (_, _) => coverage.mutation_refused = 1,
    }
    Ok(coverage)
}

#[test]
fn a_wrong_origin_or_registry_is_refused_and_a_changed_journal_fails_validation() {
    let plain = HotStoreInstances::create_from_hs_and_hr(
        Cache::default(),
        Box::new(ClosedDurableHistoryReader),
    );
    let registry = GenerationLeaseRegistryV1::default();
    let publication = SlotShape::at(manifest(OTHER_STATE_SEED, MANIFEST_LEN), 1);
    assert!(
        is_conflict(&refusal_of(plain.capture_cross_base_journal(
            origin_manifest(),
            publication.slot(),
            &registry
        ))),
        "a store without a durable journal was captured"
    );
    let mut seeded = RefusalCoverage::default();
    for case in seed_refusal_cases() {
        seeded.merge(check_refusal(&case).expect("a seeded case satisfies CB-02"));
    }
    println!("cross-base refusal seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(RefusalCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: REFUSAL_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&refusal_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_refusal(&case)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated cases satisfy CB-02");
    assert!(generated.get().cases >= REFUSAL_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("cross-base refusal: {merged:?}");
    merged.assert_complete();
}

#[derive(Clone, Copy, Debug)]
enum ObservationStep {
    Operate(JournalOperation),
    Publish(PublicationChoice),
}

#[derive(Clone, Debug)]
struct ObservationCase {
    base: BaseRecords,
    start: JournalStart,
    steps: Vec<ObservationStep>,
    publication: PublicationChoice,
}

fn seed_observation_cases() -> [ObservationCase; 2] {
    [
        ObservationCase {
            base: rich_base(),
            start: JournalStart::Detached,
            steps: vec![
                ObservationStep::Operate(JournalOperation::PutDatum {
                    channel_index: 0,
                    value_index: 2,
                }),
                ObservationStep::Operate(JournalOperation::RemoveDatum {
                    channel_index: 0,
                    selector: 0,
                }),
                ObservationStep::Operate(JournalOperation::PutContinuation {
                    group_index: 2,
                    body_index: 0,
                }),
                ObservationStep::Operate(JournalOperation::RemoveContinuation {
                    group_index: 1,
                    selector: 0,
                }),
                ObservationStep::Publish(PublicationChoice {
                    gap: 1,
                    state: PublicationState::Other(1),
                }),
                ObservationStep::Operate(JournalOperation::PutDatum {
                    channel_index: 1,
                    value_index: 0,
                }),
                ObservationStep::Operate(JournalOperation::RemoveContinuation {
                    group_index: 0,
                    selector: 0,
                }),
                ObservationStep::Publish(PublicationChoice {
                    gap: 2,
                    state: PublicationState::OfOrigin,
                }),
                ObservationStep::Operate(JournalOperation::RemoveDatum {
                    channel_index: 1,
                    selector: 0,
                }),
            ],
            publication: PublicationChoice {
                gap: 1,
                state: PublicationState::Other(2),
            },
        },
        ObservationCase {
            base: rich_base(),
            start: JournalStart::Bound,
            steps: vec![
                ObservationStep::Operate(JournalOperation::RemoveDatum {
                    channel_index: 1,
                    selector: 0,
                }),
                ObservationStep::Operate(JournalOperation::PutContinuation {
                    group_index: 0,
                    body_index: 0,
                }),
            ],
            publication: PublicationChoice {
                gap: 2,
                state: PublicationState::OfOrigin,
            },
        },
    ]
}

fn observation_steps() -> impl Strategy<Value = ObservationStep> {
    prop_oneof![
        4 => journal_operations().prop_map(ObservationStep::Operate),
        1 => publication_choices().prop_map(ObservationStep::Publish),
    ]
}

fn observation_cases() -> impl Strategy<Value = ObservationCase> {
    (
        base_records(),
        journal_starts(),
        prop::collection::vec(observation_steps(), 0..16),
        publication_choices(),
    )
        .prop_map(|(base, start, steps, publication)| ObservationCase {
            base,
            start,
            steps,
            publication,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct ObservationCoverage {
    cases: u32,
    compared: u32,
    nonempty: u32,
    from_detached_journals: u32,
    from_bound_journals: u32,
    after_an_acknowledgement: u32,
    data_removals: u32,
    continuation_removals: u32,
    join_removals: u32,
    publication_of_the_origin_state: u32,
}

impl ObservationCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            compared,
            nonempty,
            from_detached_journals,
            from_bound_journals,
            after_an_acknowledgement,
            data_removals,
            continuation_removals,
            join_removals,
            publication_of_the_origin_state,
        } = other;
        self.cases += cases;
        self.compared += compared;
        self.nonempty += nonempty;
        self.from_detached_journals += from_detached_journals;
        self.from_bound_journals += from_bound_journals;
        self.after_an_acknowledgement += after_an_acknowledgement;
        self.data_removals += data_removals;
        self.continuation_removals += continuation_removals;
        self.join_removals += join_removals;
        self.publication_of_the_origin_state += publication_of_the_origin_state;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            compared,
            nonempty,
            from_detached_journals,
            from_bound_journals,
            after_an_acknowledgement,
            data_removals,
            continuation_removals,
            join_removals,
            publication_of_the_origin_state,
        } = *self;
        assert!(
            [
                cases,
                compared,
                nonempty,
                from_detached_journals,
                from_bound_journals,
                after_an_acknowledgement,
                data_removals,
                continuation_removals,
                join_removals,
                publication_of_the_origin_state,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of cross-base observation never occurred: {self:?}"
        );
    }
}

struct ObservationPair<'store> {
    subject: &'store Store,
    twin: &'store Store,
    registry: &'store GenerationLeaseRegistryV1,
}

impl ObservationPair<'_> {
    fn compare(
        &self,
        origin: ObjectRefV1,
        publication: SlotShape,
        twin_slot: SlotShape,
        publications: u8,
        coverage: &mut ObservationCoverage,
    ) -> Result<(CapturedHotJournalV1, CapturedHotJournalV1), TestCaseError> {
        let before = condition_of(self.subject);
        let cross = self
            .subject
            .capture_cross_base_journal(origin, publication.slot(), self.registry)
            .map_err(|error| {
                TestCaseError::fail(format!(
                    "the subject refused its cross-base capture: {error:?}"
                ))
            })?;
        check_unchanged(self.subject, &before, "a cross-base capture")?;
        let same = self
            .twin
            .capture_pending_journal(twin_slot.slot())
            .map_err(|error| {
                TestCaseError::fail(format!("the twin refused its same-base capture: {error:?}"))
            })?;
        prop_assert_eq!(cross.kind, JournalCaptureKindV1::CrossBase);
        prop_assert_eq!(same.kind, JournalCaptureKindV1::SameBase);
        prop_assert_eq!(cross.base, publication.slot());
        prop_assert_eq!(same.base, twin_slot.slot());
        prop_assert_eq!(cross.origin, origin);
        prop_assert_eq!(same.origin, origin);
        prop_assert_eq!(cross.version.revision, same.version.revision);
        prop_assert!(
            same_observation(&cross.observation, &same.observation),
            "the cross-base capture observed {:?} where the same-base capture at the origin \
             observed {:?}",
            cross.observation,
            same.observation
        );
        prop_assert_eq!(self.subject.validate_pending_journal(&cross), Ok(()));
        prop_assert_eq!(self.twin.validate_pending_journal(&same), Ok(()));
        let pending = &cross.observation.pending;
        coverage.compared += 1;
        coverage.nonempty += u32::from(!journal_is_empty(pending));
        coverage.from_detached_journals +=
            u32::from(matches!(before.base, RecordedBase::Detached(_)));
        coverage.from_bound_journals += u32::from(matches!(before.base, RecordedBase::Bound(_)));
        coverage.after_an_acknowledgement += u32::from(publications > 0);
        coverage.data_removals += u32::from(removes_from(&pending.data));
        coverage.continuation_removals += u32::from(removes_from(&pending.continuations));
        coverage.join_removals += u32::from(removes_from(&pending.joins));
        coverage.publication_of_the_origin_state += u32::from(publication.manifest == origin);
        Ok((cross, same))
    }
}

fn check_observation(case: &ObservationCase) -> Result<ObservationCoverage, TestCaseError> {
    let registry = GenerationLeaseRegistryV1::default();
    let mut origin = origin_manifest();
    let mut subject_slot = SlotShape::at(origin, 0);
    let mut twin_slot = SlotShape::at(origin, 1);
    let subject = store_over(&case.base, origin, &registry);
    let twin = store_over(&case.base, origin, &registry);
    prop_assert_eq!(twin.bind_journal_authority(twin_slot.slot()), Ok(()));
    start_journal(&subject, case.start, subject_slot)?;
    let pair = ObservationPair {
        subject: &subject,
        twin: &twin,
        registry: &registry,
    };
    let mut coverage = ObservationCoverage {
        cases: 1,
        ..ObservationCoverage::default()
    };
    let mut publications = 0_u8;
    for step in &case.steps {
        match *step {
            ObservationStep::Operate(operation) => {
                let index = removal_index(&twin, operation);
                let expected = apply_operation(&twin, operation, index);
                prop_assert_eq!(
                    apply_operation(&subject, operation, index),
                    expected,
                    "the subject and its twin answered {:?} differently",
                    operation
                );
            }
            ObservationStep::Publish(choice) => {
                let publication = subject_slot.later(choice.gap, choice.state.manifest(origin));
                let (cross, same) =
                    pair.compare(origin, publication, twin_slot, publications, &mut coverage)?;
                if journal_is_empty(&cross.observation.pending) {
                    continue;
                }
                publications += 1;
                let next = successor_manifest(publications);
                let subject_successor = publication.successor(next);
                let twin_successor = twin_slot.successor(next);
                prop_assert_eq!(
                    subject.acknowledge_pending_journal(
                        &cross,
                        subject_successor.slot(),
                        subject_successor.slot()
                    ),
                    Ok(HotJournalAcknowledgementV1::Committed)
                );
                prop_assert_eq!(
                    twin.acknowledge_pending_journal(
                        &same,
                        twin_successor.slot(),
                        twin_successor.slot()
                    ),
                    Ok(HotJournalAcknowledgementV1::Committed)
                );
                origin = next;
                subject_slot = subject_successor;
                twin_slot = twin_successor;
            }
        }
    }
    let publication =
        subject_slot.later(case.publication.gap, case.publication.state.manifest(origin));
    let (cross, _) = pair.compare(origin, publication, twin_slot, publications, &mut coverage)?;
    let rebinding = subject_slot.later(1, origin);
    prop_assert_eq!(subject.bind_journal_authority(rebinding.slot()), Ok(()));
    let same = subject
        .capture_pending_journal(rebinding.slot())
        .map_err(|error| {
            TestCaseError::fail(format!("the rebound subject refused its capture: {error:?}"))
        })?;
    prop_assert!(
        cross.version.matches(&same.version),
        "the bind moved the subject's journal from revision {} to revision {}",
        cross.version.revision,
        same.version.revision
    );
    prop_assert!(
        same_observation(&cross.observation, &same.observation),
        "the subject's cross-base capture observed {:?} where its same-base capture at the origin \
         observed {:?}",
        cross.observation,
        same.observation
    );
    prop_assert_eq!(
        subject.validate_pending_journal(&cross),
        Ok(()),
        "a bind to a slot of the origin's state invalidated the cross-base capture"
    );
    Ok(coverage)
}

#[test]
fn a_cross_base_capture_observes_what_a_same_base_capture_at_the_origin_observes() {
    let mut seeded = ObservationCoverage::default();
    for case in seed_observation_cases() {
        seeded.merge(check_observation(&case).expect("a seeded case satisfies CB-03"));
    }
    println!("cross-base observation seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(ObservationCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: OBSERVATION_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&observation_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_observation(&case)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated cases satisfy CB-03");
    assert!(generated.get().cases >= OBSERVATION_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("cross-base observation: {merged:?}");
    merged.assert_complete();
}

#[test]
fn same_base_and_cross_base_captures_of_one_journal_validate_by_their_own_rules() {
    let registry = GenerationLeaseRegistryV1::default();
    let origin = origin_manifest();
    let origin_slot = SlotShape::at(origin, 0);
    let rebound_slot = SlotShape::at(origin, 1);
    let publication = origin_slot.later(2, manifest(OTHER_STATE_SEED + 1, MANIFEST_LEN));
    let next = successor_manifest(0);
    let store = store_over(&rich_base(), origin, &registry);
    assert_eq!(store.bind_journal_authority(origin_slot.slot()), Ok(()));
    apply_operations(&store, &[
        JournalOperation::PutDatum {
            channel_index: 1,
            value_index: 2,
        },
        JournalOperation::RemoveDatum {
            channel_index: 0,
            selector: 0,
        },
        JournalOperation::RemoveContinuation {
            group_index: 1,
            selector: 0,
        },
    ]);
    let same_base = store
        .capture_pending_journal(origin_slot.slot())
        .expect("the bound journal is captured at its slot");
    let cross_base = store
        .capture_cross_base_journal(origin, publication.slot(), &registry)
        .expect("the journal is captured for publication on another state");
    let on_its_own_slot = store
        .capture_cross_base_journal(origin, origin_slot.slot(), &registry)
        .expect("the journal is captured for publication on a slot of its own state");
    for captured in [&same_base, &cross_base, &on_its_own_slot] {
        assert_eq!(store.validate_pending_journal(captured), Ok(()));
    }
    assert!(same_base.version.matches(&cross_base.version));
    assert!(same_observation(&same_base.observation, &cross_base.observation));
    assert_eq!(store.bind_journal_authority(rebound_slot.slot()), Ok(()));
    assert!(
        is_conflict(&store.validate_pending_journal(&same_base)),
        "a same-base capture validated after its journal was bound to another slot of the same \
         state"
    );
    assert_eq!(
        store.validate_pending_journal(&cross_base),
        Ok(()),
        "a bind to another slot of the origin's state invalidated a cross-base capture"
    );
    assert_eq!(
        store.validate_pending_journal(&on_its_own_slot),
        Ok(()),
        "a bind to another slot of the origin's state invalidated a cross-base capture on the \
         previous slot"
    );
    assert!(is_conflict(&refusal_of(store.capture_pending_journal(origin_slot.slot()))));
    let rebound = store
        .capture_pending_journal(rebound_slot.slot())
        .expect("the journal is captured at the slot it is now bound to");
    let before = condition_of(&store);
    let origin_successor = origin_slot.successor(next).slot();
    assert!(
        is_conflict(&refusal_of(store.acknowledge_pending_journal(
            &same_base,
            origin_successor,
            origin_successor
        ))),
        "a same-base capture of a re-bound journal was acknowledged"
    );
    check_unchanged(&store, &before, "a refused same-base acknowledgement")
        .expect("a refused acknowledgement changes nothing");
    let successor = publication.successor(next).slot();
    assert_eq!(
        store.acknowledge_pending_journal(&cross_base, successor, successor),
        Ok(HotJournalAcknowledgementV1::Committed)
    );
    assert_eq!(
        store.acknowledge_pending_journal(&cross_base, successor, successor),
        Ok(HotJournalAcknowledgementV1::AlreadyCommitted)
    );
    assert_eq!(store.validate_journal_acknowledgement(&cross_base, successor, &registry), Ok(()));
    let rebound_successor = rebound_slot.successor(next).slot();
    assert!(is_conflict(&refusal_of(store.acknowledge_pending_journal(
        &rebound,
        rebound_successor,
        rebound_successor
    ))));
    assert!(is_conflict(&refusal_of(store.acknowledge_pending_journal(
        &on_its_own_slot,
        origin_successor,
        origin_successor
    ))));
    assert!(is_conflict(&store.validate_journal_acknowledgement(&rebound, successor, &registry)));
    assert!(is_conflict(&store.validate_journal_acknowledgement(&same_base, successor, &registry)));
}
