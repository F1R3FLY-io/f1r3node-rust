use std::cell::RefCell;
use std::sync::Mutex;

use super::*;
use crate::rspace::bag::{Bag, Multiplicity};
use crate::rspace::data_index::record_rebased_counts;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::persistence::{CanonicalBlob, CanonicalBlobKind, CanonicalEncode, ObjectDigest};
use crate::rspace::serializers::serializers::encode_join_record;

type Hot = dyn HotStore<String, String, String, String>;
type Concrete = InMemHotStore<String, String, String, String>;
type State = InMemHotStoreState<String, String, String, String>;
type Cache = HotStoreState<String, String, String, String>;
type Continuation = WaitingContinuation<String, String>;
type Reader = Box<dyn HistoryReaderBase<String, String, String, String>>;

const CHANNELS: [&str; 4] = ["a", "b", "c", "d"];
const VALUES: [&str; 2] = ["v0", "v1"];
const BODIES: [&str; 5] = ["k0", "k1", "k2", "k3", "k4"];
const SOURCES: [u8; 2] = [1, 9];
const GROUPS: [&[&str]; 4] = [&["a"], &["a", "b"], &["b", "a"], &["c"]];
const BOUNDARY_COUNTS: [u64; 5] = [0, 1, 2, u64::MAX - 1, u64::MAX];
const REFINEMENT_CASES: u32 = 256;
const SAME_BASE_CASES: u32 = 256;
const READ_CASES: u32 = 128;
const FFI_CASES: u32 = 128;
const DROPPED_CAPTURED_SOURCES: &str = "captured source bytes that the rebase drops";

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(name), value.to_owned(), false)
}

fn continuation(body: &str, source: u8) -> Continuation {
    WaitingContinuation {
        patterns: Arc::new(vec![format!("pattern-{body}")]),
        continuation: Arc::new(body.to_owned()),
        persist: false,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![source; 32]),
            persistent: false,
        },
    }
}

fn records(
    data: &[(&str, &[&str])],
    continuations: &[(&[&str], &str, u8)],
    joins: &[(&str, &[&str])],
) -> Cache {
    let mut records = Cache::default();
    for &(name, values) in data {
        records
            .data
            .insert(channel(name), values.iter().map(|value| datum(name, value)).collect());
    }
    for &(names, body, source) in continuations {
        records
            .continuations
            .entry(group(names))
            .or_default()
            .push(continuation(body, source));
    }
    for &(name, names) in joins {
        records
            .joins
            .entry(channel(name))
            .or_default()
            .push(group(names));
    }
    records
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BaseRead {
    Data(String),
    Continuations(Vec<String>),
    Joins(String),
}

#[derive(Default)]
struct ReadLedger {
    reads: Mutex<Vec<BaseRead>>,
}

impl ReadLedger {
    fn record(&self, read: BaseRead) { self.reads.lock().expect("read ledger lock").push(read); }

    fn take(&self) -> Vec<BaseRead> {
        let mut reads = std::mem::take(&mut *self.reads.lock().expect("read ledger lock"));
        reads.sort();
        reads
    }
}

struct BaseReader {
    base: Cache,
    ledger: Arc<ReadLedger>,
}

impl HistoryReaderBase<String, String, String, String> for BaseReader {
    fn get_data_canonical(&self, key: &String) -> Vec<Datum<String>> {
        self.ledger.record(BaseRead::Data(key.clone()));
        self.base.data.get(key).cloned().unwrap_or_default()
    }

    fn get_continuations_canonical(&self, key: &[String]) -> Vec<Continuation> {
        self.ledger.record(BaseRead::Continuations(key.to_vec()));
        self.base
            .continuations
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    fn get_joins_canonical(&self, key: &String) -> Vec<Vec<String>> {
        self.ledger.record(BaseRead::Joins(key.clone()));
        self.base.joins.get(key).cloned().unwrap_or_default()
    }
}

fn journal_base() -> DurableJournalBaseV1 {
    DurableJournalBaseV1 {
        manifest: ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&[0]), 32)
            .expect("a synthetic manifest is at least one object envelope header long"),
        registry: GenerationLeaseRegistryV1::default(),
    }
}

fn reader_over(base: &Cache) -> (Reader, Arc<ReadLedger>) {
    let ledger = Arc::new(ReadLedger::default());
    let reader = BaseReader {
        base: base.clone(),
        ledger: Arc::clone(&ledger),
    };
    (Box::new(reader), ledger)
}

fn store_over(base: &Cache, cache: Cache) -> (Box<Hot>, Arc<ReadLedger>) {
    let (reader, ledger) = reader_over(base);
    let store = HotStoreInstances::create_from_durable_reader(reader, cache, journal_base())
        .expect("the captured cache fits the multiplicity model");
    (store, ledger)
}

fn durable_state(cache: Cache) -> State {
    State {
        durable_journal: Some(Box::new(DurableJournalStateV1::detached(journal_base()))),
        ..State::from_snapshot(cache).expect("the cache fits the multiplicity model")
    }
}

fn concrete_store(base: &Cache, state: State) -> (Concrete, Arc<ReadLedger>) {
    let (reader, ledger) = reader_over(base);
    let store = Concrete {
        state: std::sync::RwLock::new(state),
        history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
        history_reader_base: reader,
    };
    (store, ledger)
}

fn journal_version(store: &Concrete) -> JournalVersionV1 {
    store
        .state
        .read()
        .expect("hot store state read lock")
        .durable_journal
        .as_ref()
        .expect("the store keeps its durable journal")
        .version
        .clone()
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    PutDatum(&'static str, &'static str),
    RemoveDatum(&'static str, usize),
    ReadDatums(&'static str),
    PutContinuation(&'static [&'static str], &'static str, u8),
    RemoveContinuation(&'static [&'static str], usize),
    ReadContinuations(&'static [&'static str]),
    PutJoin(&'static str, &'static [&'static str]),
    RemoveJoin(&'static str, &'static [&'static str]),
    ReadJoins(&'static str),
}

fn removal_index(selector: usize, length: usize) -> Option<i32> {
    selector
        .checked_rem(length)
        .map(|index| i32::try_from(index).expect("a removal index fits i32"))
}

fn apply(store: &Hot, operation: Operation) {
    match operation {
        Operation::PutDatum(name, value) => store.put_datum(&channel(name), datum(name, value)),
        Operation::RemoveDatum(name, selector) => {
            let length = store.get_data(&channel(name)).len();
            if let Some(index) = removal_index(selector, length) {
                store
                    .remove_datum(&channel(name), index)
                    .expect("a datum inside the channel is removed");
            }
        }
        Operation::ReadDatums(name) => {
            store.get_data(&channel(name));
        }
        Operation::PutContinuation(names, body, source) => {
            store
                .put_continuation(&group(names), continuation(body, source))
                .expect("the hot store answers every continuation insertion");
        }
        Operation::RemoveContinuation(names, selector) => {
            let length = store.get_continuations(&group(names)).len();
            if let Some(index) = removal_index(selector, length) {
                store
                    .remove_continuation(&group(names), index)
                    .expect("a continuation inside the group is removed");
            }
        }
        Operation::ReadContinuations(names) => {
            store.get_continuations(&group(names));
        }
        Operation::PutJoin(name, names) => {
            store
                .put_join(&channel(name), &group(names))
                .expect("the hot store takes every join");
        }
        Operation::RemoveJoin(name, names) => {
            store
                .remove_join(&channel(name), &group(names))
                .expect("the hot store answers every join removal");
        }
        Operation::ReadJoins(name) => {
            store.get_joins(&channel(name));
        }
    }
}

struct Journals {
    data: BagDelta,
    continuations: BagDelta,
    content: PathMap<ContentTransition>,
    sources: PathMap<Arc<[u8]>>,
    joins: BagDelta,
}

impl Journals {
    fn empty() -> Self {
        Self {
            data: BagDelta::new(),
            continuations: BagDelta::new(),
            content: PathMap::new(),
            sources: PathMap::new(),
            joins: BagDelta::new(),
        }
    }

    fn of(store: &Hot) -> Self {
        let pending = store
            .pending_deltas()
            .expect("the hot store exposes its journals");
        Self {
            data: pending.data.expect("the hot store keeps a data journal"),
            continuations: pending
                .continuations
                .expect("the hot store keeps a continuation journal"),
            content: pending
                .continuation_content
                .expect("the hot store keeps continuation content transitions"),
            sources: pending
                .continuation_source_bytes
                .expect("the hot store keeps continuation source bytes"),
            joins: pending.joins.expect("the hot store keeps a join journal"),
        }
    }

    fn rebase(&self, store: &Hot) -> Result<(), RSpaceError> {
        store.rebase_pending_deltas(
            &self.data,
            &self.continuations,
            &self.content,
            &self.sources,
            &self.joins,
        )
    }
}

fn entries<V: Clone + Send + Sync + Unpin>(journal: &PathMap<V>) -> BTreeMap<Vec<u8>, V> {
    journal
        .iter()
        .map(|(path, value)| (path, value.clone()))
        .collect()
}

fn same_journals(left: &Journals, right: &Journals) -> bool {
    left.data == right.data &&
        left.continuations == right.continuations &&
        left.joins == right.joins &&
        entries(&left.content) == entries(&right.content) &&
        entries(&left.sources) == entries(&right.sources)
}

fn same_records(left: &Cache, right: &Cache) -> bool {
    left.data == right.data &&
        left.continuations == right.continuations &&
        left.installed_continuations == right.installed_continuations &&
        left.joins == right.joins &&
        left.installed_joins == right.installed_joins
}

struct Payload {
    cache: Cache,
    journals: Journals,
}

fn captured_payload(capture_base: &Cache, operations: &[Operation]) -> Payload {
    let (store, _) = store_over(capture_base, Cache::default());
    for &operation in operations {
        apply(store.as_ref(), operation);
    }
    Payload {
        cache: store.snapshot(),
        journals: Journals::of(store.as_ref()),
    }
}

struct ContinuationKeys {
    record_path: Vec<u8>,
    identity_path: Vec<u8>,
    reference: ObjectRefV1,
    source: Arc<[u8]>,
}

fn continuation_keys(channels: &[String], continuation: &Continuation) -> ContinuationKeys {
    let prefix = RhoPathV1::continuation_prefix(channels);
    let prepared = PreparedContinuationRecord::encode_waiting(continuation);
    let view = prepared.record_view();
    let blob =
        CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, view.source_suffix().to_vec())
            .expect("a continuation source is a canonical blob");
    let mut record_path = Vec::with_capacity(prefix.len() + prepared.canonical().len());
    record_path.extend_from_slice(&prefix);
    record_path.extend_from_slice(prepared.canonical());
    ContinuationKeys {
        record_path,
        identity_path: RhoPathV1::collection_record_path(&prefix, view.identity()),
        reference: blob.object_ref(),
        source: blob
            .try_canonical_bytes()
            .expect("a continuation source blob has canonical bytes")
            .into(),
    }
}

fn join_path(name: &str, join: &[String]) -> Vec<u8> {
    let mut path = RhoPathV1::join_prefix(&channel(name));
    path.extend_from_slice(&encode_join_record(join));
    path
}

#[derive(Default)]
struct Projection {
    data: Bag,
    continuations: Bag,
    joins: Bag,
    content: BTreeMap<Vec<u8>, ObjectRefV1>,
    sources: BTreeMap<Vec<u8>, Arc<[u8]>>,
}

fn projection(records: &Cache) -> Projection {
    let mut projection = Projection::default();
    for (name, datums) in &records.data {
        for datum in datums {
            projection
                .data
                .insert(&RSpaceDataIndex::datum_path(name, datum))
                .expect("a projected datum count fits u64");
        }
    }
    for (channels, continuations) in &records.continuations {
        for continuation in continuations {
            let keys = continuation_keys(channels, continuation);
            projection
                .continuations
                .insert(&keys.record_path)
                .expect("a projected continuation count fits u64");
            projection
                .content
                .insert(keys.identity_path.clone(), keys.reference);
            projection.sources.insert(keys.identity_path, keys.source);
        }
    }
    for (name, joins) in &records.joins {
        for join in joins {
            projection
                .joins
                .insert(&join_path(name, join))
                .expect("a projected join count fits u64");
        }
    }
    projection
}

fn overlay(receiving_base: &Cache, cache: &Cache) -> Cache {
    let mut overlay = receiving_base.clone();
    overlay.data.extend(
        cache
            .data
            .iter()
            .map(|(name, datums)| (name.clone(), datums.clone())),
    );
    overlay.continuations.extend(
        cache
            .continuations
            .iter()
            .map(|(channels, continuations)| (channels.clone(), continuations.clone())),
    );
    overlay.joins.extend(
        cache
            .joins
            .iter()
            .map(|(name, joins)| (name.clone(), joins.clone())),
    );
    overlay
}

fn count_refinement(
    name: &str,
    journal: &BagDelta,
    held: &Bag,
    target: &Bag,
) -> Result<(), String> {
    let mut applied = held.clone();
    journal.apply_to(&mut applied).map_err(|error| {
        format!("the {name} journal does not apply to the receiving base: {error}")
    })?;
    if applied != *target {
        return Err(format!("the {name} journal applied to the receiving base is not the overlay"));
    }
    if journal
        .additions()
        .iter()
        .any(|(path, _)| journal.removals().contains(&path))
    {
        return Err(format!("the {name} journal holds a path on both sides"));
    }
    Ok(())
}

fn refinement(receiving_base: &Cache, cache: &Cache, journals: &Journals) -> Result<(), String> {
    let held = projection(receiving_base);
    let target = projection(&overlay(receiving_base, cache));
    count_refinement("data", &journals.data, &held.data, &target.data)?;
    count_refinement(
        "continuation",
        &journals.continuations,
        &held.continuations,
        &target.continuations,
    )?;
    count_refinement("join", &journals.joins, &held.joins, &target.joins)?;
    let transitions = entries(&journals.content);
    if transitions
        .iter()
        .any(|(path, transition)| transition.before() != held.content.get(path).copied())
    {
        return Err("a content transition does not start at the receiving base".to_owned());
    }
    let mut applied = held.content.clone();
    for (path, transition) in &transitions {
        match transition.after() {
            Some(reference) => {
                applied.insert(path.clone(), reference);
            }
            None => {
                applied.remove(path);
            }
        }
    }
    if applied != target.content {
        return Err(
            "the content transitions applied to the receiving base are not the overlay".to_owned()
        );
    }
    let expected_sources: BTreeMap<Vec<u8>, Arc<[u8]>> = transitions
        .iter()
        .filter(|(_, transition)| transition.after().is_some())
        .filter_map(|(path, _)| {
            target
                .sources
                .get(path)
                .map(|bytes| (path.clone(), Arc::clone(bytes)))
        })
        .collect();
    if entries(&journals.sources) != expected_sources {
        return Err("the source journal does not hold the bytes of the overlay at exactly the \
                    transitions that end in a reference"
            .to_owned());
    }
    Ok(())
}

fn captured_empty<K: Eq + Hash, V>(
    cache: &HashMap<K, Vec<V>>,
    receiving_base: &HashMap<K, Vec<V>>,
) -> bool {
    cache.iter().any(|(key, captured)| {
        captured.is_empty() && receiving_base.get(key).is_some_and(|held| !held.is_empty())
    })
}

fn uncaptured<K: Eq + Hash, V>(
    cache: &HashMap<K, Vec<V>>,
    receiving_base: &HashMap<K, Vec<V>>,
) -> bool {
    receiving_base
        .iter()
        .any(|(key, held)| !held.is_empty() && !cache.contains_key(key))
}

fn shapes(
    receiving_base: &Cache,
    payload: &Payload,
    rebased: &Journals,
) -> [(&'static str, bool); 17] {
    let transitions = entries(&rebased.content);
    [
        ("data additions", !rebased.data.additions().is_empty()),
        ("data removals", !rebased.data.removals().is_empty()),
        ("continuation additions", !rebased.continuations.additions().is_empty()),
        ("continuation removals", !rebased.continuations.removals().is_empty()),
        ("join additions", !rebased.joins.additions().is_empty()),
        ("join removals", !rebased.joins.removals().is_empty()),
        (
            "content insertions",
            transitions
                .values()
                .any(|transition| transition.before().is_none()),
        ),
        (
            "content removals",
            transitions
                .values()
                .any(|transition| transition.after().is_none()),
        ),
        (
            "content replacements",
            transitions
                .values()
                .any(|transition| transition.before().is_some() && transition.after().is_some()),
        ),
        ("source bytes", !rebased.sources.is_empty()),
        (
            DROPPED_CAPTURED_SOURCES,
            payload
                .journals
                .sources
                .iter()
                .any(|(path, _)| rebased.sources.get(path).is_none()),
        ),
        (
            "captured-empty data channels over base data",
            captured_empty(&payload.cache.data, &receiving_base.data),
        ),
        (
            "uncaptured data channels with base data",
            uncaptured(&payload.cache.data, &receiving_base.data),
        ),
        (
            "captured-empty groups over base continuations",
            captured_empty(&payload.cache.continuations, &receiving_base.continuations),
        ),
        (
            "uncaptured groups with base continuations",
            uncaptured(&payload.cache.continuations, &receiving_base.continuations),
        ),
        (
            "captured-empty join channels over base joins",
            captured_empty(&payload.cache.joins, &receiving_base.joins),
        ),
        (
            "uncaptured join channels with base joins",
            uncaptured(&payload.cache.joins, &receiving_base.joins),
        ),
    ]
}

#[derive(Debug, Default)]
struct Coverage {
    cases: u32,
    shapes: BTreeMap<&'static str, u32>,
}

impl Coverage {
    fn record(&mut self, receiving_base: &Cache, payload: &Payload, rebased: &Journals) {
        self.cases += 1;
        for (shape, occurred) in shapes(receiving_base, payload, rebased) {
            *self.shapes.entry(shape).or_default() += u32::from(occurred);
        }
    }

    fn assert_complete(&self) { self.assert_complete_except(&[]); }

    fn assert_complete_except(&self, unreachable: &[&str]) {
        let missing: Vec<&str> = self
            .shapes
            .iter()
            .filter(|(shape, count)| **count == 0 && !unreachable.contains(shape))
            .map(|(shape, _)| *shape)
            .collect();
        assert!(
            self.cases > 0 && missing.is_empty(),
            "a shape of journal rebase never occurred: {missing:?} in {self:?}"
        );
    }
}

fn rebase_payload(payload: &Payload, receiving_base: &Cache) -> Result<Journals, TestCaseError> {
    let (receiver, _) = store_over(receiving_base, payload.cache.clone());
    let before = receiver.snapshot();
    prop_assert_eq!(payload.journals.rebase(receiver.as_ref()), Ok(()));
    prop_assert!(
        same_records(&receiver.snapshot(), &before),
        "a rebase changed the records of the store"
    );
    Ok(Journals::of(receiver.as_ref()))
}

fn check_refinement(
    payload: &Payload,
    receiving_base: &Cache,
    coverage: &mut Coverage,
) -> Result<(), TestCaseError> {
    let rebased = rebase_payload(payload, receiving_base)?;
    refinement(receiving_base, &payload.cache, &rebased).map_err(TestCaseError::fail)?;
    coverage.record(receiving_base, payload, &rebased);
    Ok(())
}

fn check_same_base(
    base: &Cache,
    operations: &[Operation],
    coverage: &mut Coverage,
) -> Result<(), TestCaseError> {
    let payload = captured_payload(base, operations);
    let rebased = rebase_payload(&payload, base)?;
    prop_assert_eq!(
        &rebased.data,
        &payload.journals.data,
        "the rebased data journal is not the captured one"
    );
    prop_assert_eq!(
        &rebased.continuations,
        &payload.journals.continuations,
        "the rebased continuation journal is not the captured one"
    );
    prop_assert_eq!(
        &rebased.joins,
        &payload.journals.joins,
        "the rebased join journal is not the captured one"
    );
    let captured_content = entries(&payload.journals.content);
    prop_assert_eq!(
        &entries(&rebased.content),
        &captured_content,
        "the rebased content transitions are not the captured ones"
    );
    prop_assert_eq!(
        entries(&rebased.sources),
        entries(&payload.journals.sources),
        "the rebased source bytes are not the captured ones"
    );
    coverage.record(base, &payload, &rebased);
    Ok(())
}

fn cross_base_seed() -> (Payload, Cache) {
    let capture_base = records(
        &[("a", &["v0"]), ("b", &["v0"])],
        &[(&["a"], "k0", 1), (&["a", "b"], "k0", 1)],
        &[("a", &["a"]), ("b", &["a", "b"])],
    );
    let mut payload = captured_payload(&capture_base, &[
        Operation::PutDatum("a", "v1"),
        Operation::RemoveDatum("b", 0),
        Operation::ReadDatums("c"),
        Operation::PutContinuation(&["a"], "k1", 1),
        Operation::PutContinuation(&["a"], "k3", 1),
        Operation::PutContinuation(&["a"], "k4", 1),
        Operation::RemoveContinuation(&["a", "b"], 0),
        Operation::ReadJoins("a"),
        Operation::PutJoin("a", &["a", "b"]),
        Operation::RemoveJoin("b", &["a", "b"]),
    ]);
    let absent = continuation_keys(&group(&["a"]), &continuation("k2", 1));
    payload
        .journals
        .sources
        .insert(absent.identity_path, absent.source);
    let uncaptured = continuation_keys(&group(&["b", "a"]), &continuation("k2", 1));
    payload.journals.content.insert(
        uncaptured.identity_path,
        ContentTransition::new(Some(uncaptured.reference), None)
            .expect("a removal is a transition"),
    );
    payload
        .journals
        .data
        .record_removal(
            &RSpaceDataIndex::datum_path(&channel("d"), &datum("d", "v0")),
            Multiplicity::ONE,
        )
        .expect("one removal fits the captured data journal");
    payload
        .journals
        .joins
        .record_removal(&join_path("c", &group(&["c"])), Multiplicity::ONE)
        .expect("one removal fits the captured join journal");
    let receiving_base = records(
        &[("a", &["v0", "v0"]), ("c", &["v1"]), ("d", &["v0"])],
        &[
            (&["a"], "k0", 9),
            (&["a"], "k1", 1),
            (&["a"], "k2", 1),
            (&["a"], "k3", 9),
            (&["a", "b"], "k0", 1),
            (&["b", "a"], "k2", 1),
        ],
        &[("a", &["a"]), ("a", &["a"]), ("b", &["a", "b"]), ("c", &["c"])],
    );
    (payload, receiving_base)
}

fn same_base_seed() -> (Cache, [Operation; 10]) {
    let base = records(
        &[("a", &["v0"]), ("b", &["v0"]), ("c", &["v0"])],
        &[(&["a"], "k0", 1), (&["a", "b"], "k0", 1), (&["b", "a"], "k2", 1), (&["c"], "k0", 1)],
        &[("a", &["a"]), ("b", &["a", "b"]), ("c", &["c"])],
    );
    let operations = [
        Operation::PutDatum("a", "v1"),
        Operation::RemoveDatum("b", 0),
        Operation::RemoveContinuation(&["a"], 0),
        Operation::PutContinuation(&["a"], "k0", 1),
        Operation::PutContinuation(&["a"], "k1", 1),
        Operation::RemoveContinuation(&["a", "b"], 0),
        Operation::RemoveContinuation(&["b", "a"], 0),
        Operation::PutContinuation(&["b", "a"], "k2", 9),
        Operation::PutJoin("a", &["a", "b"]),
        Operation::RemoveJoin("b", &["a", "b"]),
    ];
    (base, operations)
}

fn base_records() -> impl Strategy<Value = Cache> {
    (
        prop::collection::vec((0..CHANNELS.len(), 0..VALUES.len()), 0..6),
        prop::collection::btree_map((0..GROUPS.len(), 0..BODIES.len()), 0..SOURCES.len(), 0..6),
        prop::collection::vec((0..CHANNELS.len(), 0..GROUPS.len()), 0..5),
    )
        .prop_map(|(data, continuations, joins)| {
            let mut base = Cache::default();
            for (channel_index, value_index) in data {
                let name = CHANNELS[channel_index];
                base.data
                    .entry(channel(name))
                    .or_default()
                    .push(datum(name, VALUES[value_index]));
            }
            for ((group_index, body_index), source_index) in continuations {
                base.continuations
                    .entry(group(GROUPS[group_index]))
                    .or_default()
                    .push(continuation(BODIES[body_index], SOURCES[source_index]));
            }
            for (channel_index, group_index) in joins {
                base.joins
                    .entry(channel(CHANNELS[channel_index]))
                    .or_default()
                    .push(group(GROUPS[group_index]));
            }
            base
        })
}

fn any_operation() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (0..CHANNELS.len(), 0..VALUES.len()).prop_map(|(channel_index, value_index)| {
            Operation::PutDatum(CHANNELS[channel_index], VALUES[value_index])
        }),
        (0..CHANNELS.len(), 0..4_usize).prop_map(|(channel_index, selector)| {
            Operation::RemoveDatum(CHANNELS[channel_index], selector)
        }),
        (0..CHANNELS.len())
            .prop_map(|channel_index| Operation::ReadDatums(CHANNELS[channel_index])),
        (0..GROUPS.len(), 0..BODIES.len(), 0..SOURCES.len()).prop_map(
            |(group_index, body_index, source_index)| {
                Operation::PutContinuation(
                    GROUPS[group_index],
                    BODIES[body_index],
                    SOURCES[source_index],
                )
            }
        ),
        (0..GROUPS.len(), 0..4_usize).prop_map(|(group_index, selector)| {
            Operation::RemoveContinuation(GROUPS[group_index], selector)
        }),
        (0..GROUPS.len()).prop_map(|group_index| Operation::ReadContinuations(GROUPS[group_index])),
        (0..CHANNELS.len(), 0..GROUPS.len()).prop_map(|(channel_index, group_index)| {
            Operation::PutJoin(CHANNELS[channel_index], GROUPS[group_index])
        }),
        (0..CHANNELS.len(), 0..GROUPS.len()).prop_map(|(channel_index, group_index)| {
            Operation::RemoveJoin(CHANNELS[channel_index], GROUPS[group_index])
        }),
        (0..CHANNELS.len()).prop_map(|channel_index| Operation::ReadJoins(CHANNELS[channel_index])),
    ]
}

#[derive(Debug)]
struct CrossBaseCase {
    capture_base: Cache,
    operations: Vec<Operation>,
    receiving_base: Cache,
}

fn cross_base_cases() -> impl Strategy<Value = CrossBaseCase> {
    (base_records(), prop::collection::vec(any_operation(), 0..12), base_records()).prop_map(
        |(capture_base, operations, receiving_base)| CrossBaseCase {
            capture_base,
            operations,
            receiving_base,
        },
    )
}

fn runner(cases: u32) -> prop::test_runner::TestRunner {
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

#[test]
fn rebased_journals_applied_to_the_receiving_base_give_the_overlay() {
    let mut seeded = Coverage::default();
    let (payload, receiving_base) = cross_base_seed();
    check_refinement(&payload, &receiving_base, &mut seeded)
        .expect("the seeded cross-base payload satisfies OG-01");
    println!("journal rebase refinement seed: {seeded:?}");
    seeded.assert_complete();
    let coverage = RefCell::new(seeded);
    runner(REFINEMENT_CASES)
        .run(&cross_base_cases(), |case| {
            let payload = captured_payload(&case.capture_base, &case.operations);
            check_refinement(&payload, &case.receiving_base, &mut coverage.borrow_mut())
        })
        .expect("generated cross-base payloads satisfy OG-01");
    let coverage = coverage.into_inner();
    println!("journal rebase refinement: {coverage:?}");
    assert!(coverage.cases > REFINEMENT_CASES);
    coverage.assert_complete();
}

#[test]
fn a_same_base_rebase_reproduces_the_journals_of_the_soft_checkpoint() {
    let mut seeded = Coverage::default();
    let (base, operations) = same_base_seed();
    check_same_base(&base, &operations, &mut seeded)
        .expect("the seeded same-base payload satisfies OG-02");
    println!("same-base journal rebase seed: {seeded:?}");
    seeded.assert_complete_except(&[DROPPED_CAPTURED_SOURCES]);
    let coverage = RefCell::new(seeded);
    runner(SAME_BASE_CASES)
        .run(
            &(base_records(), prop::collection::vec(any_operation(), 0..16)),
            |(base, operations)| check_same_base(&base, &operations, &mut coverage.borrow_mut()),
        )
        .expect("generated same-base payloads satisfy OG-02");
    let coverage = coverage.into_inner();
    println!("same-base journal rebase: {coverage:?}");
    assert!(coverage.cases > SAME_BASE_CASES);
    coverage.assert_complete_except(&[DROPPED_CAPTURED_SOURCES]);
}

fn counted(path: &[u8], count: u64) -> Bag {
    let mut bag = Bag::new();
    if let Ok(amount) = Multiplicity::new(count) {
        bag.insert_many(path, amount)
            .expect("one count fits an empty bag");
    }
    bag
}

#[test]
fn rebased_count_sides_are_disjoint_and_stay_within_u64() {
    let path = b"channel/record";
    for base_count in BOUNDARY_COUNTS {
        for target_count in BOUNDARY_COUNTS {
            let mut journal = BagDelta::new();
            assert_eq!(
                record_rebased_counts(
                    &mut journal,
                    b"channel/",
                    &counted(b"record", base_count),
                    &counted(b"record", target_count),
                ),
                Ok(())
            );
            let addition = journal.additions().count(path);
            let removal = journal.removals().count(path);
            assert_eq!(
                addition,
                target_count.saturating_sub(base_count),
                "the rebased addition from {base_count} to {target_count}"
            );
            assert_eq!(
                removal,
                base_count.saturating_sub(target_count),
                "the rebased removal from {base_count} to {target_count}"
            );
            assert!(
                addition == 0 || removal == 0,
                "the rebase from {base_count} to {target_count} holds one path on both sides"
            );
            assert!(
                removal <= base_count && addition <= target_count,
                "the rebase from {base_count} to {target_count} leaves its bounds"
            );
            assert_eq!(
                base_count
                    .checked_sub(removal)
                    .and_then(|kept| kept.checked_add(addition)),
                Some(target_count),
                "the rebase from {base_count} to {target_count} underflows or overflows"
            );
            let mut applied = counted(path, base_count);
            assert_eq!(journal.apply_to(&mut applied), Ok(()));
            assert!(
                applied == counted(path, target_count),
                "the rebased journal applied to {base_count} is not {target_count}"
            );
        }
    }

    let saturated = Multiplicity::new(u64::MAX).expect("the largest count is a multiplicity");
    let mut state = durable_state(Cache::default());
    state
        .data
        .set_channel_counts(channel("a"), [
            (datum("a", "v0"), saturated),
            (datum("a", "v1"), saturated),
        ])
        .expect("two saturated counts fit one channel");
    let base = records(&[("a", &["v0", "v1", "v1"])], &[], &[]);
    let (store, _) = concrete_store(&base, state);
    assert_eq!(Journals::empty().rebase(&store), Ok(()));
    let rebased = Journals::of(&store);
    let once_in_the_base = RSpaceDataIndex::datum_path(&channel("a"), &datum("a", "v0"));
    let twice_in_the_base = RSpaceDataIndex::datum_path(&channel("a"), &datum("a", "v1"));
    assert_eq!(
        rebased.data.additions().count(&once_in_the_base),
        u64::MAX - 1,
        "the rebased addition of a datum that the overlay holds u64::MAX times and the base once"
    );
    assert_eq!(
        rebased.data.additions().count(&twice_in_the_base),
        u64::MAX - 2,
        "the rebased addition of a datum that the overlay holds u64::MAX times and the base twice"
    );
    assert!(
        rebased.data.removals().is_empty(),
        "a rebase onto a base below the overlay recorded a removal"
    );
    let mut applied = projection(&base).data;
    assert_eq!(rebased.data.apply_to(&mut applied), Ok(()));
    assert_eq!(applied.count(&once_in_the_base), u64::MAX);
    assert_eq!(applied.count(&twice_in_the_base), u64::MAX);
}

fn cache_members(cache: &Cache) -> Vec<BaseRead> {
    let mut members: Vec<BaseRead> = cache
        .data
        .keys()
        .cloned()
        .map(BaseRead::Data)
        .chain(
            cache
                .continuations
                .keys()
                .cloned()
                .map(BaseRead::Continuations),
        )
        .chain(cache.joins.keys().cloned().map(BaseRead::Joins))
        .collect();
    members.sort();
    members
}

fn read_fixture() -> (Cache, Cache) {
    let base = records(
        &[("a", &["v0", "v0"]), ("c", &["v1"])],
        &[(&["a"], "k0", 9), (&["b", "a"], "k2", 1)],
        &[("a", &["a"]), ("d", &["c"])],
    );
    let mut cache = records(&[("a", &["v0"])], &[(&["a"], "k0", 1)], &[("a", &["a"])]);
    cache.data.insert(channel("b"), Vec::new());
    cache.continuations.insert(group(&["a", "b"]), Vec::new());
    (base, cache)
}

#[test]
fn a_rebase_reads_the_receiving_base_once_per_domain_member() {
    let (base, cache) = read_fixture();
    let (store, ledger) = store_over(&base, cache.clone());
    assert_eq!(Journals::empty().rebase(store.as_ref()), Ok(()));
    assert_eq!(
        ledger.take(),
        cache_members(&cache),
        "a rebase without captured keys did not read each channel and group of the cache exactly \
         once"
    );

    let (store, ledger) = store_over(&base, cache);
    assert!(store.get_data(&channel("d")).is_empty());
    assert_eq!(ledger.take(), [BaseRead::Data(channel("d"))]);
    let named_by_content = continuation_keys(&group(&["b", "a"]), &continuation("k2", 1));
    let named_by_source = continuation_keys(&group(&["c"]), &continuation("k0", 1));
    let resident = continuation_keys(&group(&["a", "b"]), &continuation("k0", 1));
    let mut captured = Journals::empty();
    captured
        .data
        .record_addition(
            &RSpaceDataIndex::datum_path(&channel("a"), &datum("a", "v1")),
            Multiplicity::ONE,
        )
        .expect("one addition fits an empty journal");
    captured
        .data
        .record_removal(
            &RSpaceDataIndex::datum_path(&channel("c"), &datum("c", "v1")),
            Multiplicity::ONE,
        )
        .expect("one removal fits the journal");
    captured
        .joins
        .record_addition(&join_path("a", &group(&["a", "b"])), Multiplicity::ONE)
        .expect("one addition fits an empty journal");
    captured
        .joins
        .record_removal(&join_path("d", &group(&["c"])), Multiplicity::ONE)
        .expect("one removal fits the journal");
    captured
        .continuations
        .record_removal(&named_by_content.record_path, Multiplicity::ONE)
        .expect("one removal fits an empty journal");
    captured
        .continuations
        .record_removal(&resident.record_path, Multiplicity::ONE)
        .expect("one removal fits the journal");
    captured.content.insert(
        named_by_content.identity_path,
        ContentTransition::new(Some(named_by_content.reference), None)
            .expect("a removal is a transition"),
    );
    captured
        .sources
        .insert(named_by_source.identity_path, named_by_source.source);
    let mut expected = cache_members(&store.snapshot());
    expected.extend([
        BaseRead::Data(channel("c")),
        BaseRead::Continuations(group(&["b", "a"])),
        BaseRead::Continuations(group(&["c"])),
        BaseRead::Joins(channel("d")),
    ]);
    expected.sort();
    assert_eq!(expected.len(), 10);
    assert_eq!(captured.rebase(store.as_ref()), Ok(()));
    assert_eq!(
        ledger.take(),
        expected,
        "a rebase did not read each member of the cache and of the captured keys exactly once"
    );

    runner(READ_CASES)
        .run(&cross_base_cases(), |case| {
            let payload = captured_payload(&case.capture_base, &case.operations);
            let (receiver, ledger) = store_over(&case.receiving_base, payload.cache.clone());
            prop_assert_eq!(payload.journals.rebase(receiver.as_ref()), Ok(()));
            prop_assert_eq!(
                ledger.take(),
                cache_members(&payload.cache),
                "a rebase of a captured payload did not read each channel and group of its cache \
                 exactly once"
            );
            Ok(())
        })
        .expect("generated payloads satisfy OG-04");
}

#[test]
fn an_ffi_shaped_payload_rebases_to_the_overlay_difference() {
    let base = records(
        &[("a", &["v0", "v0"]), ("b", &["v0"])],
        &[(&["a"], "k0", 9), (&["a"], "k2", 1)],
        &[("a", &["a"]), ("a", &["a", "b"])],
    );
    let mut cache =
        records(&[("a", &["v0", "v1"])], &[(&["a"], "k0", 1), (&["a"], "k1", 1)], &[("a", &["a"])]);
    cache.data.insert(channel("b"), Vec::new());
    let payload = Payload {
        cache,
        journals: Journals::empty(),
    };
    let rebased = rebase_payload(&payload, &base).expect("an FFI-shaped payload is rebased");
    assert_eq!(refinement(&base, &payload.cache, &rebased), Ok(()));
    let data_path =
        |name: &str, value: &str| RSpaceDataIndex::datum_path(&channel(name), &datum(name, value));
    assert_eq!(rebased.data.additions().count(&data_path("a", "v1")), 1);
    assert_eq!(rebased.data.removals().count(&data_path("a", "v0")), 1);
    assert_eq!(rebased.data.removals().count(&data_path("b", "v0")), 1);
    assert_eq!(rebased.continuations.additions().iter().count(), 2);
    assert_eq!(rebased.continuations.removals().iter().count(), 2);
    assert_eq!(entries(&rebased.content).len(), 3);
    assert_eq!(entries(&rebased.sources).len(), 2);
    assert!(rebased.joins.additions().is_empty());
    assert_eq!(
        rebased
            .joins
            .removals()
            .count(&join_path("a", &group(&["a", "b"]))),
        1
    );

    runner(FFI_CASES)
        .run(&cross_base_cases(), |case| {
            let payload = Payload {
                cache: captured_payload(&case.capture_base, &case.operations).cache,
                journals: Journals::empty(),
            };
            let rebased = rebase_payload(&payload, &case.receiving_base)?;
            refinement(&case.receiving_base, &payload.cache, &rebased).map_err(TestCaseError::fail)
        })
        .expect("generated FFI-shaped payloads satisfy OG-05");
}

#[derive(Clone, Copy)]
enum CapturedJournal {
    Data,
    Continuations,
    Content,
    Sources,
    Joins,
}

struct MalformedKey {
    label: &'static str,
    journal: CapturedJournal,
    key: Vec<u8>,
    refusal: &'static str,
}

fn payload_with_key(journal: CapturedJournal, key: &[u8]) -> Journals {
    let mut journals = Journals::empty();
    match journal {
        CapturedJournal::Data => journals
            .data
            .record_addition(key, Multiplicity::ONE)
            .expect("one addition fits an empty journal"),
        CapturedJournal::Continuations => journals
            .continuations
            .record_addition(key, Multiplicity::ONE)
            .expect("one addition fits an empty journal"),
        CapturedJournal::Joins => journals
            .joins
            .record_addition(key, Multiplicity::ONE)
            .expect("one addition fits an empty journal"),
        CapturedJournal::Content => {
            let keys = continuation_keys(&group(&["a"]), &continuation("k0", 1));
            journals.content.insert(
                key,
                ContentTransition::new(None, Some(keys.reference))
                    .expect("an insertion is a transition"),
            );
        }
        CapturedJournal::Sources => {
            journals.sources.insert(key, Arc::from(&b"source"[..]));
        }
    }
    journals
}

fn with_channel_identity(key: &[u8], name: &str, identity: &[u8]) -> Vec<u8> {
    let frame = RhoPathV1::collection_record_path(&[], &channel(name).rho_path_identity());
    let start = key
        .windows(frame.len())
        .position(|window| window == frame.as_slice())
        .expect("the key frames the identity of its channel");
    let replacement = RhoPathV1::collection_record_path(&[], identity);
    let mut reframed = Vec::with_capacity(key.len() - frame.len() + replacement.len());
    reframed.extend_from_slice(&key[..start]);
    reframed.extend_from_slice(&replacement);
    reframed.extend_from_slice(&key[start + frame.len()..]);
    reframed
}

fn malformed_keys() -> [MalformedKey; 13] {
    let stray = b"no canonical path".to_vec();
    let undecodable = 9_u64.to_le_bytes();
    let mut trailing = channel("d").rho_path_identity();
    trailing.push(7);
    let datum_key = RSpaceDataIndex::datum_path(&channel("d"), &datum("d", "v0"));
    let join_key = join_path("d", &group(&["d"]));
    let keys = continuation_keys(&group(&["d"]), &continuation("k0", 1));
    [
        MalformedKey {
            label: "a data key that is no datum path",
            journal: CapturedJournal::Data,
            key: stray.clone(),
            refusal: "captured data journal holds a malformed key",
        },
        MalformedKey {
            label: "a data key whose channel does not decode",
            journal: CapturedJournal::Data,
            key: with_channel_identity(&datum_key, "d", &undecodable),
            refusal: "captured data journal holds a malformed key",
        },
        MalformedKey {
            label: "a data key whose channel carries a trailing byte",
            journal: CapturedJournal::Data,
            key: with_channel_identity(&datum_key, "d", &trailing),
            refusal: "captured data journal holds a key that is not canonical for its channels",
        },
        MalformedKey {
            label: "a join key that is no join path",
            journal: CapturedJournal::Joins,
            key: stray.clone(),
            refusal: "captured join journal holds a malformed key",
        },
        MalformedKey {
            label: "a join key whose channel does not decode",
            journal: CapturedJournal::Joins,
            key: with_channel_identity(&join_key, "d", &undecodable),
            refusal: "captured join journal holds a malformed key",
        },
        MalformedKey {
            label: "a join key whose channel carries a trailing byte",
            journal: CapturedJournal::Joins,
            key: with_channel_identity(&join_key, "d", &trailing),
            refusal: "captured join journal holds a key that is not canonical for its channels",
        },
        MalformedKey {
            label: "a content key that is no continuation path",
            journal: CapturedJournal::Content,
            key: stray.clone(),
            refusal: "captured continuation content journal holds a malformed key",
        },
        MalformedKey {
            label: "a content key whose channel does not decode",
            journal: CapturedJournal::Content,
            key: with_channel_identity(&keys.identity_path, "d", &undecodable),
            refusal: "captured continuation content journal holds a malformed key",
        },
        MalformedKey {
            label: "a content key whose channel carries a trailing byte",
            journal: CapturedJournal::Content,
            key: with_channel_identity(&keys.identity_path, "d", &trailing),
            refusal: "captured continuation content journal holds a key that is not canonical for \
                      its channels",
        },
        MalformedKey {
            label: "a source key that is no continuation path",
            journal: CapturedJournal::Sources,
            key: stray,
            refusal: "captured continuation source journal holds a malformed key",
        },
        MalformedKey {
            label: "a source key whose channel does not decode",
            journal: CapturedJournal::Sources,
            key: with_channel_identity(&keys.identity_path, "d", &undecodable),
            refusal: "captured continuation source journal holds a malformed key",
        },
        MalformedKey {
            label: "a source key whose channel carries a trailing byte",
            journal: CapturedJournal::Sources,
            key: with_channel_identity(&keys.identity_path, "d", &trailing),
            refusal: "captured continuation source journal holds a key that is not canonical for \
                      its channels",
        },
        MalformedKey {
            label: "a continuation count key under no captured group",
            journal: CapturedJournal::Continuations,
            key: keys.record_path,
            refusal: "captured continuation journal holds a key outside every captured \
                      continuation group",
        },
    ]
}

fn live_store() -> (Concrete, Arc<ReadLedger>) {
    let base = records(&[("a", &["v0"])], &[(&["a"], "k0", 1)], &[("a", &["a"])]);
    let (store, ledger) = concrete_store(&base, durable_state(Cache::default()));
    for operation in [
        Operation::PutDatum("a", "v1"),
        Operation::PutContinuation(&["a"], "k1", 1),
        Operation::PutJoin("a", &["a", "b"]),
    ] {
        apply(&store, operation);
    }
    (store, ledger)
}

#[test]
fn a_malformed_captured_key_fails_closed_and_changes_nothing() {
    let (store, ledger) = live_store();
    let journals = Journals::of(&store);
    let records = store.snapshot();
    let version = journal_version(&store);
    assert_eq!(ledger.take().len(), 3);
    for malformed in malformed_keys() {
        let label = malformed.label;
        let refusal = payload_with_key(malformed.journal, &malformed.key).rebase(&store);
        assert!(
            matches!(
                &refusal,
                Err(RSpaceError::JournalConflict(message)) if message.starts_with(malformed.refusal)
            ),
            "{label} was answered with {refusal:?}"
        );
        assert!(ledger.take().is_empty(), "{label} was refused after a read of the base");
        assert!(
            same_journals(&Journals::of(&store), &journals),
            "{label} was refused after a journal changed"
        );
        assert!(
            same_records(&store.snapshot(), &records),
            "{label} was refused after a record changed"
        );
        assert!(
            journal_version(&store).matches(&version),
            "{label} was refused after the journal version advanced"
        );
    }
}

#[test]
fn a_rebase_advances_the_journal_version_once_and_keeps_the_records() {
    let (store, _) = live_store();
    let captured = Journals::of(&store);
    let records = store.snapshot();
    let version = journal_version(&store);
    assert_eq!(captured.rebase(&store), Ok(()));
    let rebased = Journals::of(&store);
    assert!(
        version.next().matches(&journal_version(&store)),
        "a rebase did not advance the journal version by one"
    );
    assert!(same_records(&store.snapshot(), &records), "a rebase changed a record");
    assert_eq!(captured.rebase(&store), Ok(()));
    assert!(
        version.next().next().matches(&journal_version(&store)),
        "a second rebase did not advance the journal version by one"
    );
    assert!(
        same_journals(&Journals::of(&store), &rebased),
        "a second rebase of one payload onto one base changed a journal"
    );
    assert!(same_records(&store.snapshot(), &records), "a second rebase changed a record");

    let (base, cache) = read_fixture();
    let (reader, _) = reader_over(&base);
    let without_journal_version = HotStoreInstances::create_from_hs_and_hr(cache, reader);
    let records = without_journal_version.snapshot();
    assert_eq!(Journals::empty().rebase(without_journal_version.as_ref()), Ok(()));
    assert!(
        same_records(&without_journal_version.snapshot(), &records),
        "a rebase of a store without a journal version changed a record"
    );
}
