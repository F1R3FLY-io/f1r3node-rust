use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use proptest::prelude::*;

use super::canonical_rebuild_oracle::{Entries, admit_node, rebuild};
use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::{PersistenceTrialDir, admit, manifest, nonzero};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::difference::{
    StateDifferenceLimits, StateDifferenceReport, visit_state_difference,
};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableAuthorityViewV1, DurableStateRootV1, recover_read_authority,
};
use crate::rspace::persistence::semantic::{
    decode_continuation_endpoint_with_channels, decode_data_endpoint_with_channel,
    decode_join_endpoint_with_channel,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalDagNodeView, CanonicalFormatError, CanonicalObjectStore,
    ClosureError, DagChild, EndpointValue, ImmutableObjectStore, LogPackDecodeLimits, ObjectDigest,
    ObjectRefV1, SemanticNamespace, SemanticRoots,
};
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};

type View = DurableAuthorityViewV1<Profile>;
type Continuation = WaitingContinuation<String, String>;
type EndpointKey = (SemanticNamespace, Vec<u8>);
type Endpoints = BTreeMap<EndpointKey, EndpointValue>;
type Difference = BTreeMap<EndpointKey, (EndpointValue, EndpointValue)>;
type Visit = (SemanticNamespace, Vec<u8>, EndpointValue, EndpointValue);
type Occurrences = BTreeSet<(SemanticNamespace, Vec<u8>, ObjectRefV1)>;
type Schedule = Vec<Vec<Edit>>;

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 3] = ["v0", "v1", "v2"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[&str]; 6] =
    [&["a"], &["b"], &["a", "b"], &["b", "a"], &["a", "a"], &["c", "a", "b"]];
const DURABLE_CASES: u32 = 32;
const IN_MEMORY_CASES: u32 = 256;
const CHAIN_DEPTH: usize = 20_000;
const MAX_PAYLOAD_BYTES: u64 = 1 << 20;
const AMPLE: StateDifferenceLimits = StateDifferenceLimits {
    max_object_reads: 1 << 20,
    max_object_read_bytes: 1 << 30,
    max_worklist: 1 << 12,
    max_path_bytes: 1 << 16,
};
const DIFFERENCE_CASES: [&str; 11] = [
    "pairs",
    "pairs without a difference",
    "data differences",
    "continuation differences",
    "join differences",
    "endpoints only in from",
    "endpoints only in to",
    "endpoints with changed values",
    "from node ends inside a to prefix",
    "to node ends inside a from prefix",
    "divergences inside both prefixes",
];
const READ_CASES: [&str; 5] = [
    "pairs",
    "pairs of a state with itself",
    "pairs that read objects",
    "pairs that skip an equal namespace",
    "pairs that skip a shared subtree below differing roots",
];
const DAG_CASES: [&str; 10] = [
    "pairs",
    "endpoints only in from",
    "endpoints only in to",
    "endpoints with changed values",
    "from node ends inside a to prefix",
    "to node ends inside a from prefix",
    "divergences inside both prefixes",
    "differences at keys with descendants",
    "empty key differences",
    "pairs that skip a shared subtree below differing roots",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Datum { channel: usize, value: usize },
    Continuation { group: usize, body: usize },
}

#[derive(Clone, Copy, Debug)]
enum Edit {
    Put(Item),
    Remove(Item),
    RemoveAny(usize),
}

#[derive(Clone, Debug, Default)]
struct Model {
    data: BTreeMap<(usize, usize), u64>,
    continuations: BTreeSet<(usize, usize)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Contents {
    data: BTreeMap<String, BTreeMap<String, u64>>,
    continuations: BTreeMap<Vec<String>, BTreeSet<String>>,
    joins: BTreeMap<String, BTreeMap<Vec<String>, u64>>,
}

struct RetainedState {
    roots: SemanticRoots,
    endpoints: Endpoints,
    contents: Contents,
}

struct Published {
    view: View,
    states: Vec<RetainedState>,
    _scratch: PersistenceTrialDir,
}

#[derive(Debug, Default)]
struct Coverage(BTreeMap<&'static str, u64>);

struct KeyShape {
    node_ends: BTreeSet<Vec<u8>>,
    interior: BTreeMap<Vec<u8>, u8>,
    extended: BTreeSet<Vec<u8>>,
}

struct CountingStore<'store, S>
where S: ImmutableObjectStore
{
    inner: &'store S,
    reads: AtomicU64,
    withheld: Option<ObjectDigest>,
    failing: Option<(ObjectDigest, fn() -> S::Error)>,
}

impl<'store, S: ImmutableObjectStore> CountingStore<'store, S> {
    fn new(inner: &'store S) -> Self {
        Self {
            inner,
            reads: AtomicU64::new(0),
            withheld: None,
            failing: None,
        }
    }

    fn reads(&self) -> u64 { self.reads.load(Ordering::Relaxed) }
}

impl<S: ImmutableObjectStore> ImmutableObjectStore for CountingStore<'_, S> {
    type Error = S::Error;
    type Object = S::Object;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        match (self.failing, self.withheld) {
            (Some((failing, error)), _) if failing == *digest => Err(error()),
            (_, Some(withheld)) if withheld == *digest => Ok(None),
            _ => self.inner.get_bounded(digest, max_encoded_len),
        }
    }
}

impl Coverage {
    fn count(&mut self, case: &'static str, occurrences: usize) {
        *self.0.entry(case).or_default() +=
            u64::try_from(occurrences).expect("a case count fits u64");
    }

    fn merge(&mut self, other: Self) {
        for (case, occurrences) in other.0 {
            *self.0.entry(case).or_default() += occurrences;
        }
    }

    fn occurrences(&self, case: &str) -> u64 { self.0.get(case).copied().unwrap_or(0) }

    fn assert_complete(&self, label: &str, cases: &[&str]) {
        let missing = cases
            .iter()
            .filter(|case| self.occurrences(case) == 0)
            .collect::<Vec<_>>();
        assert!(missing.is_empty(), "{label}: the cases {missing:?} never occurred: {self:?}");
    }
}

impl KeyShape {
    fn of(keys: &BTreeSet<Vec<u8>>) -> Self {
        let mut continuations = BTreeMap::<Vec<u8>, BTreeSet<u8>>::new();
        for key in keys {
            for (length, byte) in key.iter().enumerate() {
                continuations
                    .entry(key[..length].to_vec())
                    .or_default()
                    .insert(*byte);
            }
        }
        let mut node_ends = keys.clone();
        if keys.is_empty() {
            node_ends.insert(Vec::new());
        }
        let extended = continuations.keys().cloned().collect();
        let mut interior = BTreeMap::new();
        for (position, next) in continuations {
            match next.into_iter().collect::<Vec<_>>().as_slice() {
                [only] if !keys.contains(&position) => {
                    interior.insert(position, *only);
                }
                [_] => {}
                _ => {
                    node_ends.insert(position);
                }
            }
        }
        Self {
            node_ends,
            interior,
            extended,
        }
    }
}

impl Model {
    fn present(&self) -> Vec<Item> {
        self.data
            .keys()
            .map(|&(channel, value)| Item::Datum { channel, value })
            .chain(
                self.continuations
                    .iter()
                    .map(|&(group, body)| Item::Continuation { group, body }),
            )
            .collect()
    }

    fn apply(&mut self, hot: &Store, edit: Edit) {
        match edit {
            Edit::Put(Item::Datum { channel, value }) => {
                put_value(hot, CHANNELS[channel], VALUES[value]);
                *self.data.entry((channel, value)).or_default() += 1;
            }
            Edit::Put(Item::Continuation { group, body }) => {
                put_continuation_with_joins(hot, GROUPS[group], BODIES[body]);
                self.continuations.insert((group, body));
            }
            Edit::Remove(item) => self.remove(hot, item),
            Edit::RemoveAny(selector) => {
                let present = self.present();
                if let Some(item) = present.get(selector % present.len().max(1)).copied() {
                    self.remove(hot, item);
                }
            }
        }
    }

    fn remove(&mut self, hot: &Store, item: Item) {
        match item {
            Item::Datum { channel, value } => {
                remove_value(hot, CHANNELS[channel], VALUES[value]);
                match self.data.get(&(channel, value)).copied() {
                    Some(1) => {
                        self.data.remove(&(channel, value));
                    }
                    Some(count) => {
                        self.data.insert((channel, value), count - 1);
                    }
                    None => panic!("the schedule removes {item:?}, which the model does not hold"),
                }
            }
            Item::Continuation { group, body } => {
                remove_continuation_with_joins(hot, GROUPS[group], BODIES[body]);
                assert!(
                    self.continuations.remove(&(group, body)),
                    "the schedule removes {item:?}, which the model does not hold"
                );
            }
        }
    }

    fn contents(&self) -> Contents {
        let mut contents = Contents::default();
        for (&(channel_index, value_index), &count) in &self.data {
            *contents
                .data
                .entry(channel(CHANNELS[channel_index]))
                .or_default()
                .entry(VALUES[value_index].to_owned())
                .or_default() += count;
        }
        for &(group_index, body_index) in &self.continuations {
            let names = GROUPS[group_index];
            contents
                .continuations
                .entry(group(names))
                .or_default()
                .insert(BODIES[body_index].to_owned());
            for member in names.iter().collect::<BTreeSet<_>>() {
                contents
                    .joins
                    .entry(channel(member))
                    .or_default()
                    .insert(group(names), 1);
            }
        }
        contents
    }
}

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn index_of(names: &[&str], name: &str) -> usize {
    names
        .iter()
        .position(|candidate| *candidate == name)
        .expect("a named value belongs to the test universe")
}

fn group_index(names: &[&str]) -> usize {
    GROUPS
        .iter()
        .position(|candidate| *candidate == names)
        .expect("the group is one of the test groups")
}

fn datum_item(channel_name: &str, value: &str) -> Item {
    Item::Datum {
        channel: index_of(&CHANNELS, channel_name),
        value: index_of(&VALUES, value),
    }
}

fn continuation_item(names: &[&str], body: &str) -> Item {
    Item::Continuation {
        group: group_index(names),
        body: index_of(&BODIES, body),
    }
}

fn put_value(hot: &Store, channel_name: &str, value: &str) {
    hot.put_datum(&channel(channel_name), datum(channel_name, value));
}

fn remove_value(hot: &Store, channel_name: &str, value: &str) {
    let key = channel(channel_name);
    let index = hot
        .get_data(&key)
        .iter()
        .position(|stored| stored.a.as_str() == value)
        .expect("the datum to remove is stored");
    hot.remove_datum(&key, i32::try_from(index).expect("a datum index fits i32"))
        .expect("remove the stored datum");
}

fn put_continuation_with_joins(hot: &Store, names: &[&str], body: &str) {
    let channels = group(names);
    hot.put_continuation(&channels, continuation(names, body))
        .expect("the hot store takes the continuation");
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.put_join(member, &channels)
            .expect("the hot store takes the join");
    }
}

fn remove_continuation_with_joins(hot: &Store, names: &[&str], body: &str) {
    let channels = group(names);
    let index = hot
        .get_continuations(&channels)
        .iter()
        .position(|stored| stored.continuation.as_str() == body)
        .expect("the continuation to remove is stored");
    hot.remove_continuation(
        &channels,
        i32::try_from(index).expect("a continuation index fits i32"),
    )
    .expect("remove the stored continuation");
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.remove_join(member, &channels)
            .expect("remove the join of the removed continuation");
    }
}

fn recover(scratch: &PersistenceTrialDir) -> View {
    recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("recover a durable view of the published states")
}

fn projected_endpoints(view: &View, roots: SemanticRoots) -> Endpoints {
    let mut endpoints = Endpoints::new();
    for (namespace, root) in roots.iter() {
        for (path, value) in view
            .project_prefix(root, &[], u64::MAX, u64::MAX)
            .expect("project a retained namespace")
        {
            assert!(
                endpoints.insert((namespace, path), value).is_none(),
                "an endpoint is projected once"
            );
        }
    }
    endpoints
}

fn decoded<S>(store: &S, endpoints: &Endpoints) -> Contents
where
    S: ImmutableObjectStore,
    S::Error: Debug,
{
    let mut contents = Contents::default();
    for ((namespace, path), value) in endpoints {
        match namespace {
            SemanticNamespace::DataBag => {
                let (key, entry, multiplicity) =
                    decode_data_endpoint_with_channel::<String, String>(path, *value)
                        .expect("decode a data endpoint");
                assert_eq!(entry, datum(&key, &entry.a), "a durable datum is one the test stored");
                *contents
                    .data
                    .entry(key)
                    .or_default()
                    .entry(Arc::unwrap_or_clone(entry.a))
                    .or_default() += multiplicity.get();
            }
            SemanticNamespace::ContinuationMap => {
                let (channels, entry) = decode_continuation_endpoint_with_channels::<
                    String,
                    String,
                    String,
                    _,
                >(store, path, *value, MAX_PAYLOAD_BYTES)
                .expect("decode a continuation endpoint");
                let names = channels.iter().map(String::as_str).collect::<Vec<_>>();
                assert_eq!(
                    entry,
                    continuation(&names, &entry.continuation),
                    "a durable continuation is one the test stored"
                );
                assert!(
                    contents
                        .continuations
                        .entry(channels)
                        .or_default()
                        .insert(Arc::unwrap_or_clone(entry.continuation)),
                    "a continuation is decoded once"
                );
            }
            SemanticNamespace::JoinBag => {
                let (key, joined, multiplicity) =
                    decode_join_endpoint_with_channel::<String>(path, *value)
                        .expect("decode a join endpoint");
                *contents
                    .joins
                    .entry(key)
                    .or_default()
                    .entry(joined)
                    .or_default() += multiplicity.get();
            }
        }
    }
    contents
}

fn publish(schedule: &[Vec<Edit>]) -> Published {
    let (scratch, hot, gate, mut owner) = fixture();
    let mut model = Model::default();
    let mut versions = Vec::with_capacity(schedule.len() + 1);
    let mut models = Vec::with_capacity(schedule.len() + 1);
    versions.push(owner.state(&hot).expect("read the fixture's state"));
    models.push(model.clone());
    for unit in schedule {
        for edit in unit {
            model.apply(&hot, *edit);
        }
        versions.push(
            poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
                .expect("the owner publishes the unit"),
        );
        models.push(model.clone());
    }
    drop(owner);
    let view = recover(&scratch);
    let store = view.verified_state().object_store();
    let states = versions
        .iter()
        .zip(&models)
        .map(|(version, model)| {
            let roots = view
                .resolve(DurableStateRootV1::of_manifest(version.manifest()))
                .expect("every published state is retained and resolves")
                .roots();
            let endpoints = projected_endpoints(&view, roots);
            let contents = decoded(store, &endpoints);
            assert_eq!(
                contents,
                model.contents(),
                "a retained state holds exactly the contents its schedule put in it"
            );
            RetainedState {
                roots,
                endpoints,
                contents,
            }
        })
        .collect();
    Published {
        view,
        states,
        _scratch: scratch,
    }
}

fn walk<S>(
    store: &S,
    from: SemanticRoots,
    to: SemanticRoots,
    limits: StateDifferenceLimits,
) -> Result<(StateDifferenceReport, Vec<Visit>), ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut visits = Vec::new();
    let report = visit_state_difference(
        store,
        from,
        to,
        limits,
        |namespace, path, from_value, to_value| {
            visits.push((namespace, path.to_vec(), from_value, to_value));
            Ok(())
        },
    )?;
    Ok((report, visits))
}

fn visited_difference(visits: Vec<Visit>) -> Result<Difference, TestCaseError> {
    let mut visited = Difference::new();
    for (namespace, path, from_value, to_value) in visits {
        prop_assert_ne!(from_value, to_value, "an endpoint with equal values was visited");
        let key = (namespace, path);
        prop_assert!(
            visited
                .insert(key.clone(), (from_value, to_value))
                .is_none(),
            "the endpoint {key:?} was visited twice"
        );
    }
    Ok(visited)
}

fn expected_difference(from: &Endpoints, to: &Endpoints) -> Difference {
    let mut difference = Difference::new();
    for (key, before) in from {
        let after = to.get(key).copied().unwrap_or(EndpointValue::Absent);
        if after != *before {
            difference.insert(key.clone(), (*before, after));
        }
    }
    for (key, after) in to {
        if !from.contains_key(key) {
            difference.insert(key.clone(), (EndpointValue::Absent, *after));
        }
    }
    difference
}

fn node_occurrences<S>(store: &S, roots: SemanticRoots) -> Occurrences
where
    S: ImmutableObjectStore,
    S::Error: Debug,
{
    let mut occurrences = Occurrences::new();
    for (namespace, root) in roots.iter() {
        let mut pending = vec![(Vec::new(), root)];
        while let Some((start, reference)) = pending.pop() {
            let bytes = store
                .get_bounded(&reference.digest(), reference.encoded_len())
                .expect("read a retained node")
                .expect("a retained node is present");
            let node = CanonicalDagNodeView::parse(bytes.as_ref())
                .expect("a namespace node is a DAG node");
            let mut end = start.clone();
            end.extend_from_slice(node.prefix());
            for child in node.children().iter() {
                let mut child_start = end.clone();
                child_start.push(child.edge);
                pending.push((child_start, child.target));
            }
            assert!(
                occurrences.insert((namespace, start, reference)),
                "a node occurrence is enumerated once"
            );
        }
    }
    occurrences
}

fn unshared_occurrences(from: &Occurrences, to: &Occurrences) -> u64 {
    u64::try_from(from.symmetric_difference(to).count()).expect("an occurrence count fits u64")
}

fn shares_a_subtree_below_differing_roots(
    from: SemanticRoots,
    to: SemanticRoots,
    from_occurrences: &Occurrences,
    to_occurrences: &Occurrences,
) -> bool {
    from.iter()
        .into_iter()
        .zip(to.iter())
        .any(|((namespace, from_root), (_, to_root))| {
            from_root != to_root &&
                from_occurrences.iter().any(|occurrence| {
                    occurrence.0 == namespace && to_occurrences.contains(occurrence)
                })
        })
}

fn namespace_keys(endpoints: &Endpoints, namespace: SemanticNamespace) -> BTreeSet<Vec<u8>> {
    endpoints
        .keys()
        .filter(|(candidate, _)| *candidate == namespace)
        .map(|(_, path)| path.clone())
        .collect()
}

fn alignment_cases(from: &KeyShape, to: &KeyShape) -> [usize; 3] {
    let ends_inside = |ends: &BTreeSet<Vec<u8>>, other: &KeyShape| {
        ends.iter()
            .filter(|position| other.interior.contains_key(*position))
            .count()
    };
    let divergences = from
        .interior
        .iter()
        .filter(|(position, byte)| {
            to.interior
                .get(*position)
                .is_some_and(|other| other != *byte)
        })
        .count();
    [ends_inside(&from.node_ends, to), ends_inside(&to.node_ends, from), divergences]
}

fn difference_coverage(from: &Endpoints, to: &Endpoints) -> Coverage {
    let difference = expected_difference(from, to);
    let mut coverage = Coverage::default();
    coverage.count("pairs", 1);
    coverage.count("pairs without a difference", usize::from(difference.is_empty()));
    for ((namespace, _), values) in &difference {
        coverage.count(
            match namespace {
                SemanticNamespace::DataBag => "data differences",
                SemanticNamespace::ContinuationMap => "continuation differences",
                SemanticNamespace::JoinBag => "join differences",
            },
            1,
        );
        coverage.count(
            match values {
                (_, EndpointValue::Absent) => "endpoints only in from",
                (EndpointValue::Absent, _) => "endpoints only in to",
                _ => "endpoints with changed values",
            },
            1,
        );
    }
    for namespace in SemanticNamespace::ALL {
        let [from_ends, to_ends, divergences] = alignment_cases(
            &KeyShape::of(&namespace_keys(from, namespace)),
            &KeyShape::of(&namespace_keys(to, namespace)),
        );
        coverage.count("from node ends inside a to prefix", from_ends);
        coverage.count("to node ends inside a from prefix", to_ends);
        coverage.count("divergences inside both prefixes", divergences);
    }
    coverage
}

fn seed_schedules() -> [Schedule; 2] {
    [
        vec![
            vec![Edit::Put(datum_item("a", "v0")), Edit::Put(datum_item("b", "v2"))],
            vec![Edit::Put(datum_item("a", "v1"))],
            vec![
                Edit::Remove(datum_item("a", "v0")),
                Edit::Remove(datum_item("a", "v1")),
                Edit::Put(datum_item("b", "v2")),
            ],
            vec![
                Edit::Put(continuation_item(&["a", "b"], "k0")),
                Edit::Put(continuation_item(&["a"], "k1")),
            ],
            vec![
                Edit::Remove(continuation_item(&["a", "b"], "k0")),
                Edit::Put(continuation_item(&["b", "a"], "k0")),
            ],
        ],
        vec![
            vec![Edit::Put(datum_item("c", "v0"))],
            vec![Edit::Remove(datum_item("c", "v0")), Edit::Put(datum_item("a", "v2"))],
            vec![
                Edit::Put(continuation_item(&["a", "a"], "k0")),
                Edit::Put(continuation_item(&["c", "a", "b"], "k1")),
            ],
            vec![
                Edit::Put(continuation_item(&["c", "a", "b"], "k0")),
                Edit::Remove(continuation_item(&["a", "a"], "k0")),
            ],
        ],
    ]
}

fn generated_schedules() -> impl Strategy<Value = Schedule> {
    let item = prop_oneof![
        (0..CHANNELS.len(), 0..VALUES.len())
            .prop_map(|(channel, value)| Item::Datum { channel, value }),
        (0..GROUPS.len(), 0..BODIES.len())
            .prop_map(|(group, body)| Item::Continuation { group, body }),
    ];
    let edit = prop_oneof![
        2 => item.prop_map(Edit::Put),
        1 => any::<usize>().prop_map(Edit::RemoveAny),
    ];
    prop::collection::vec(prop::collection::vec(edit, 1..=3), 2..=4)
}

fn check_visited_differences(published: &Published) -> Result<Coverage, TestCaseError> {
    let store = published.view.verified_state().object_store();
    let mut coverage = Coverage::default();
    for from in &published.states {
        for to in &published.states {
            let (report, visits) =
                walk(store, from.roots, to.roots, AMPLE).expect("walk two retained states");
            let visited = visited_difference(visits)?;
            prop_assert_eq!(
                &visited,
                &expected_difference(&from.endpoints, &to.endpoints),
                "the visits must be exactly the endpoints whose values differ, with both values"
            );
            prop_assert_eq!(
                report.differences,
                u64::try_from(visited.len()).expect("a visit count fits u64")
            );
            let mut applied = from.endpoints.clone();
            for (key, (_, to_value)) in &visited {
                match to_value {
                    EndpointValue::Absent => applied.remove(key),
                    value => applied.insert(key.clone(), *value),
                };
            }
            prop_assert_eq!(
                &applied,
                &to.endpoints,
                "applying every visited to_value to from must give exactly to"
            );
            prop_assert_eq!(
                &decoded(store, &applied),
                &to.contents,
                "the applied endpoints must decode to to's multisets per channel and per ordered \
                 channel list"
            );
            coverage.merge(difference_coverage(&from.endpoints, &to.endpoints));
        }
    }
    Ok(coverage)
}

fn check_counted_reads(published: &Published) -> Result<Coverage, TestCaseError> {
    let store = published.view.verified_state().object_store();
    let occurrences = published
        .states
        .iter()
        .map(|state| node_occurrences(store, state.roots))
        .collect::<Vec<_>>();
    let mut coverage = Coverage::default();
    for (from_index, (from, from_occurrences)) in
        published.states.iter().zip(&occurrences).enumerate()
    {
        for (to_index, (to, to_occurrences)) in
            published.states.iter().zip(&occurrences).enumerate()
        {
            let counting = CountingStore::new(store);
            let (report, visits) = walk(&counting, from.roots, to.roots, AMPLE)
                .expect("walk two retained states through the counting store");
            let reads = counting.reads();
            let unshared = unshared_occurrences(from_occurrences, to_occurrences);
            prop_assert_eq!(
                &visited_difference(visits)?,
                &expected_difference(&from.endpoints, &to.endpoints),
                "the counted walk must still visit exactly the endpoint difference"
            );
            prop_assert_eq!(reads, report.object_reads, "the report counts every store read");
            prop_assert!(
                reads <= unshared,
                "DF-02: the walk read {reads} objects, but the two states do not share only \
                 {unshared} node occurrences"
            );
            prop_assert_eq!(
                reads,
                unshared,
                "the walk reads each unshared node occurrence exactly once"
            );
            if from.roots == to.roots {
                prop_assert_eq!(reads, 0, "a state against itself reads zero objects");
            }
            let equal_namespace = from
                .roots
                .iter()
                .into_iter()
                .zip(to.roots.iter())
                .any(|((_, from_root), (_, to_root))| from_root == to_root);
            coverage.count("pairs", 1);
            coverage.count("pairs of a state with itself", usize::from(from_index == to_index));
            coverage.count("pairs that read objects", usize::from(reads > 0));
            coverage.count(
                "pairs that skip an equal namespace",
                usize::from(from.roots != to.roots && equal_namespace),
            );
            coverage.count(
                "pairs that skip a shared subtree below differing roots",
                usize::from(shares_a_subtree_below_differing_roots(
                    from.roots,
                    to.roots,
                    from_occurrences,
                    to_occurrences,
                )),
            );
        }
    }
    Ok(coverage)
}

fn run_durable_property(
    label: &'static str,
    cases: &[&str],
    check: impl Fn(&Published) -> Result<Coverage, TestCaseError>,
) {
    let mut seeded = Coverage::default();
    for schedule in seed_schedules() {
        seeded.merge(
            check(&publish(&schedule))
                .unwrap_or_else(|error| panic!("{label}: a seeded schedule fails: {error}")),
        );
    }
    println!("{label} seeds: {seeded:?}");
    seeded.assert_complete(label, cases);
    let generated = RefCell::new(Coverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: DURABLE_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_schedules(), |schedule| {
        let coverage = check(&publish(&schedule))?;
        generated.borrow_mut().merge(coverage);
        Ok(())
    })
    .unwrap_or_else(|error| panic!("{label}: a generated schedule fails: {error}"));
    let generated = generated.into_inner();
    println!("{label} generated: {generated:?}");
    assert!(
        generated.occurrences("pairs") >= u64::from(DURABLE_CASES) * 9,
        "{label}: every generated schedule compares at least nine pairs: {generated:?}"
    );
}

#[test]
fn visited_differences_turn_the_from_state_into_the_to_state() {
    run_durable_property("DF-01", &DIFFERENCE_CASES, check_visited_differences);
}

#[test]
fn a_difference_reads_only_the_nodes_the_two_states_do_not_share() {
    run_durable_property("DF-02", &READ_CASES, check_counted_reads);
}

fn path_group(path: &[u8]) -> Vec<String> {
    RhoPathV1::parse_continuation(path)
        .expect("a visited continuation path parses")
        .channel_identities()
        .map(|identity| {
            CHANNELS
                .into_iter()
                .map(channel)
                .find(|candidate| candidate.rho_path_identity() == identity)
                .expect("a continuation path names channels of the test universe")
        })
        .collect()
}

fn side(from_value: EndpointValue, to_value: EndpointValue) -> &'static str {
    match (from_value, to_value) {
        (_, EndpointValue::Absent) => "only in from",
        (EndpointValue::Absent, _) => "only in to",
        _ => "in both",
    }
}

fn present_value(from_value: EndpointValue, to_value: EndpointValue) -> EndpointValue {
    match (from_value, to_value) {
        (EndpointValue::Absent, value) | (value, _) => value,
    }
}

#[test]
fn both_orders_of_a_receive_group_are_visited_as_distinct_endpoints() {
    let ordered = ["a", "b"];
    let reversed = ["b", "a"];
    let published = publish(&[vec![Edit::Put(continuation_item(&ordered, "k0"))], vec![
        Edit::Remove(continuation_item(&ordered, "k0")),
        Edit::Put(continuation_item(&reversed, "k0")),
    ]]);
    let store = published.view.verified_state().object_store();
    let [_, with_ordered, with_reversed] = published.states.as_slice() else {
        panic!("the schedule publishes two states after the fixture's");
    };
    assert_eq!(
        (
            with_ordered
                .contents
                .continuations
                .keys()
                .collect::<Vec<_>>(),
            with_reversed
                .contents
                .continuations
                .keys()
                .collect::<Vec<_>>()
        ),
        (vec![&group(&ordered)], vec![&group(&reversed)]),
        "one state holds the continuation under (a,b) only and the other under (b,a) only"
    );
    for (from, to, from_names, to_names) in [
        (with_ordered, with_reversed, ordered, reversed),
        (with_reversed, with_ordered, reversed, ordered),
    ] {
        let (report, visits) =
            walk(store, from.roots, to.roots, AMPLE).expect("walk the two retained states");
        let continuation_visits = visits
            .iter()
            .filter(|(namespace, ..)| *namespace == SemanticNamespace::ContinuationMap)
            .collect::<Vec<_>>();
        let mut groups = continuation_visits
            .iter()
            .map(|(_, path, from_value, to_value)| (path_group(path), side(*from_value, *to_value)))
            .collect::<Vec<_>>();
        groups.sort();
        let mut expected_groups =
            vec![(group(&from_names), "only in from"), (group(&to_names), "only in to")];
        expected_groups.sort();
        assert_eq!(
            groups, expected_groups,
            "DF-03: the groups {from_names:?} and {to_names:?} must be visited as two distinct \
             endpoints, each on its own side"
        );
        let distinct_paths = continuation_visits
            .iter()
            .map(|(_, path, ..)| path)
            .collect::<BTreeSet<_>>();
        assert_eq!(distinct_paths.len(), 2, "the two groups are two distinct endpoint paths");
        for (_, path, from_value, to_value) in &continuation_visits {
            let names = path_group(path);
            let names = names.iter().map(String::as_str).collect::<Vec<_>>();
            assert!(
                path.starts_with(&RhoPathV1::continuation_prefix(&group(&names))),
                "a visited continuation lies below its group's literal ordered prefix"
            );
            let (channels, entry) =
                decode_continuation_endpoint_with_channels::<String, String, String, _>(
                    store,
                    path,
                    present_value(*from_value, *to_value),
                    MAX_PAYLOAD_BYTES,
                )
                .expect("decode the visited continuation");
            assert_eq!(
                (channels, entry),
                (group(&names), continuation(&names, "k0")),
                "the visited record is the continuation stored under that group"
            );
        }
        let mut joins = visits
            .iter()
            .filter(|(namespace, ..)| *namespace == SemanticNamespace::JoinBag)
            .map(|(_, path, from_value, to_value)| {
                let (indexed, joined, _) = decode_join_endpoint_with_channel::<String>(
                    path,
                    present_value(*from_value, *to_value),
                )
                .expect("decode the visited join");
                (indexed, joined, side(*from_value, *to_value))
            })
            .collect::<Vec<_>>();
        joins.sort();
        let mut expected_joins = vec![
            (channel("a"), group(&from_names), "only in from"),
            (channel("b"), group(&from_names), "only in from"),
            (channel("a"), group(&to_names), "only in to"),
            (channel("b"), group(&to_names), "only in to"),
        ];
        expected_joins.sort();
        assert_eq!(joins, expected_joins, "the joins of both groups are distinct endpoints");
        assert!(
            visits
                .iter()
                .all(|(namespace, ..)| *namespace != SemanticNamespace::DataBag),
            "no data endpoint differs"
        );
        assert_eq!(report.differences, 6, "two continuation and four join endpoints differ");
    }
}

fn data_endpoints(entries: &Entries) -> Endpoints {
    entries
        .iter()
        .map(|(key, value)| ((SemanticNamespace::DataBag, key.clone()), *value))
        .collect()
}

fn in_memory_roots(store: &CanonicalObjectStore, entries: &Entries) -> SemanticRoots {
    let empty = rebuild(store, &Entries::new());
    SemanticRoots {
        data_bag: rebuild(store, entries),
        continuation_map: empty,
        join_bag: empty,
    }
}

fn entries(pairs: &[(&str, EndpointValue)]) -> Entries {
    pairs
        .iter()
        .map(|(key, value)| (key.as_bytes().to_vec(), *value))
        .collect()
}

fn every_shape_pair() -> (Entries, Entries) {
    let one = EndpointValue::Multiplicity(nonzero(1));
    let two = EndpointValue::Multiplicity(nonzero(2));
    (
        entries(&[
            ("ab", one),
            ("abcd", one),
            ("abx", two),
            ("mno", one),
            ("q", one),
            ("zzz", one),
        ]),
        entries(&[
            ("", EndpointValue::Unit),
            ("ab", two),
            ("abce", one),
            ("abx", two),
            ("mn", one),
            ("mno", one),
            ("qr", one),
            ("zzy", one),
        ]),
    )
}

fn check_in_memory_pair(from: &Entries, to: &Entries) -> Result<Coverage, TestCaseError> {
    let store = CanonicalObjectStore::new();
    let from_roots = in_memory_roots(&store, from);
    let to_roots = in_memory_roots(&store, to);
    let from_occurrences = node_occurrences(&store, from_roots);
    let to_occurrences = node_occurrences(&store, to_roots);
    let mut coverage = Coverage::default();
    for (before, after, before_roots, after_roots, before_occurrences, after_occurrences) in [
        (from, to, from_roots, to_roots, &from_occurrences, &to_occurrences),
        (to, from, to_roots, from_roots, &to_occurrences, &from_occurrences),
    ] {
        let counting = CountingStore::new(&store);
        let (report, visits) = walk(&counting, before_roots, after_roots, AMPLE)
            .expect("walk two in-memory canonical DAGs");
        let visited = visited_difference(visits)?;
        let before_endpoints = data_endpoints(before);
        let after_endpoints = data_endpoints(after);
        prop_assert_eq!(
            &visited,
            &expected_difference(&before_endpoints, &after_endpoints),
            "the visits must be exactly the endpoint difference"
        );
        prop_assert_eq!(counting.reads(), report.object_reads, "the report counts every read");
        prop_assert_eq!(
            counting.reads(),
            unshared_occurrences(before_occurrences, after_occurrences),
            "the walk reads each unshared node occurrence exactly once"
        );
        let before_shape = KeyShape::of(&before.keys().cloned().collect());
        let after_shape = KeyShape::of(&after.keys().cloned().collect());
        let mut pair = difference_coverage(&before_endpoints, &after_endpoints);
        pair.count(
            "differences at keys with descendants",
            visited
                .keys()
                .filter(|(_, path)| {
                    before_shape.extended.contains(path) || after_shape.extended.contains(path)
                })
                .count(),
        );
        pair.count(
            "empty key differences",
            usize::from(visited.contains_key(&(SemanticNamespace::DataBag, Vec::new()))),
        );
        pair.count(
            "pairs that skip a shared subtree below differing roots",
            usize::from(shares_a_subtree_below_differing_roots(
                before_roots,
                after_roots,
                before_occurrences,
                after_occurrences,
            )),
        );
        coverage.merge(pair);
    }
    Ok(coverage)
}

fn generated_entry_pairs() -> impl Strategy<Value = (Entries, Entries)> {
    let key = || prop::collection::vec(0u8..3, 0..6);
    let value = || {
        prop_oneof![
            Just(EndpointValue::Unit),
            Just(EndpointValue::Multiplicity(nonzero(1))),
            Just(EndpointValue::Multiplicity(nonzero(2))),
        ]
    };
    (
        prop::collection::btree_map(key(), value(), 0..12),
        prop::collection::vec((key(), prop::option::of(value())), 1..6),
    )
        .prop_map(|(from, edits)| {
            let mut to = from.clone();
            for (key, value) in edits {
                match value {
                    Some(value) => to.insert(key, value),
                    None => to.remove(&key),
                };
            }
            (from, to)
        })
}

#[test]
fn arbitrary_canonical_pairs_visit_their_exact_difference_and_read_only_unshared_nodes() {
    let (from, to) = every_shape_pair();
    let seeded = check_in_memory_pair(&from, &to).expect("the every-shape pair satisfies the walk");
    println!("in-memory difference seed: {seeded:?}");
    seeded.assert_complete("in-memory seed", &DAG_CASES);
    let generated = RefCell::new(Coverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: IN_MEMORY_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_entry_pairs(), |(from, to)| {
        let coverage = check_in_memory_pair(&from, &to)?;
        generated.borrow_mut().merge(coverage);
        Ok(())
    })
    .expect("generated canonical pairs satisfy the walk");
    let generated = generated.into_inner();
    println!("in-memory difference generated: {generated:?}");
    assert_eq!(
        generated.occurrences("pairs"),
        u64::from(IN_MEMORY_CASES) * 2,
        "every generated pair is walked in both directions"
    );
}

#[test]
fn every_limit_binds_exactly_and_refuses_one_below() {
    let (from_entries, to_entries) = every_shape_pair();
    let store = CanonicalObjectStore::new();
    let from = in_memory_roots(&store, &from_entries);
    let to = in_memory_roots(&store, &to_entries);
    let counting = CountingStore::new(&store);
    let (report, visits) = walk(&counting, from, to, AMPLE).expect("walk with ample limits");
    let visited = visited_difference(visits.clone()).expect("each endpoint is visited once");
    assert_eq!(
        visited,
        expected_difference(&data_endpoints(&from_entries), &data_endpoints(&to_entries)),
        "the walk visits exactly the endpoint difference"
    );
    let unshared =
        unshared_occurrences(&node_occurrences(&store, from), &node_occurrences(&store, to));
    assert_eq!(
        (unshared, counting.reads(), report.object_reads, report.differences),
        (13, 13, 13, 9),
        "the walk reads the 13 unshared node occurrences and visits the 9 differences"
    );
    let exact = StateDifferenceLimits {
        max_object_reads: report.object_reads,
        max_object_read_bytes: report.object_read_bytes,
        max_worklist: report.maximum_worklist,
        max_path_bytes: report.maximum_path_bytes,
    };
    assert_eq!(
        walk(&store, from, to, exact).expect("the consumed amounts suffice as limits"),
        (report, visits),
        "limits equal to the consumed amounts give the same walk"
    );
    let one_below = [
        (
            "state difference object reads",
            StateDifferenceLimits {
                max_object_reads: exact.max_object_reads - 1,
                ..exact
            },
            exact.max_object_reads,
        ),
        (
            "state difference object read bytes",
            StateDifferenceLimits {
                max_object_read_bytes: exact.max_object_read_bytes - 1,
                ..exact
            },
            exact.max_object_read_bytes,
        ),
        (
            "state difference worklist",
            StateDifferenceLimits {
                max_worklist: exact.max_worklist - 1,
                ..exact
            },
            exact.max_worklist,
        ),
        (
            "state difference path bytes",
            StateDifferenceLimits {
                max_path_bytes: exact.max_path_bytes - 1,
                ..exact
            },
            exact.max_path_bytes,
        ),
    ];
    for (resource, limits, consumed) in one_below {
        let refusal =
            walk(&store, from, to, limits).expect_err("one below a consumed amount refuses");
        assert!(
            matches!(
                refusal,
                ClosureError::Format(CanonicalFormatError::ResourceLimit {
                    resource: refused,
                    limit,
                    attempted,
                }) if refused == resource && limit == consumed - 1 && attempted == consumed
            ),
            "{resource} one below {consumed}: {refusal:?}"
        );
    }
    let unchanged = CountingStore::new(&store);
    let nothing = StateDifferenceLimits {
        max_object_reads: 0,
        max_object_read_bytes: 0,
        max_worklist: 0,
        max_path_bytes: 0,
    };
    assert_eq!(
        (
            walk(&unchanged, to, to, nothing).expect("a state against itself needs no budget"),
            unchanged.reads()
        ),
        ((StateDifferenceReport::default(), Vec::new()), 0),
        "a state against itself reserves, reads and visits nothing"
    );
    for limits in [
        StateDifferenceLimits {
            max_worklist: u64::MAX,
            ..AMPLE
        },
        StateDifferenceLimits {
            max_path_bytes: u64::MAX,
            ..AMPLE
        },
    ] {
        let refusal = walk(&store, from, to, limits).expect_err("an unreservable limit refuses");
        assert!(
            matches!(
                refusal,
                ClosureError::Format(CanonicalFormatError::AllocationFailure {
                    requested: u64::MAX
                })
            ),
            "{limits:?}: {refusal:?}"
        );
    }
}

#[test]
fn read_and_visitor_errors_stop_the_walk_with_typed_errors() {
    let (from_entries, to_entries) = every_shape_pair();
    let store = CanonicalObjectStore::new();
    let from = in_memory_roots(&store, &from_entries);
    let to = in_memory_roots(&store, &to_entries);
    let target = to.data_bag.digest();

    let withheld = CountingStore {
        withheld: Some(target),
        ..CountingStore::new(&store)
    };
    let missing = walk(&withheld, from, to, AMPLE).expect_err("a withheld node refuses the walk");
    assert!(
        matches!(
            missing,
            ClosureError::Format(CanonicalFormatError::MissingObject(digest)) if digest == target
        ),
        "{missing:?}"
    );

    let failing = CountingStore {
        failing: Some((target, || CanonicalFormatError::ObjectStorePoisoned)),
        ..CountingStore::new(&store)
    };
    let failure = walk(&failing, from, to, AMPLE).expect_err("a failing read refuses the walk");
    assert!(
        matches!(failure, ClosureError::Store(CanonicalFormatError::ObjectStorePoisoned)),
        "{failure:?}"
    );
    assert_eq!(
        (withheld.reads(), failing.reads()),
        (2, 2),
        "the walk stops at the failed read of the second root"
    );

    let mut calls = 0;
    let refusal = visit_state_difference(&store, from, to, AMPLE, |_, _, _, _| {
        calls += 1;
        Err(ClosureError::Format(CanonicalFormatError::SemanticCoherence {
            detail: "the visitor refuses".to_owned(),
        }))
    })
    .expect_err("a refusing visitor stops the walk");
    assert!(
        matches!(
            &refusal,
            ClosureError::Format(CanonicalFormatError::SemanticCoherence { detail })
                if detail == "the visitor refuses"
        ),
        "{refusal:?}"
    );
    assert_eq!(calls, 1, "the walk stops at the visitor's first refusal");

    let not_a_node = admit(&store, &manifest(from));
    let refused = walk(
        &store,
        from,
        SemanticRoots {
            data_bag: not_a_node,
            ..to
        },
        AMPLE,
    )
    .expect_err("a root that is not a DAG node refuses the walk");
    assert!(
        matches!(
            refused,
            ClosureError::Format(CanonicalFormatError::UnexpectedObjectKind {
                digest,
                expected: "DAG-node",
                actual: "semantic-manifest",
            }) if digest == not_a_node.digest()
        ),
        "{refused:?}"
    );
}

fn chain(store: &CanonicalObjectStore, depth: usize, last: EndpointValue) -> ObjectRefV1 {
    let mut reference = admit_node(
        store,
        &CanonicalDagNode::new(Vec::<u8>::new(), last, Vec::<DagChild>::new())
            .expect("a leaf with an endpoint is canonical"),
    );
    for _ in 1..depth {
        reference = admit_node(
            store,
            &CanonicalDagNode::new(Vec::<u8>::new(), EndpointValue::Unit, vec![DagChild {
                edge: b'x',
                target: reference,
            }])
            .expect("a unary node with an endpoint is canonical"),
        );
    }
    reference
}

#[test]
fn a_twenty_thousand_node_deep_difference_is_walked_on_a_small_stack() {
    std::thread::Builder::new()
        .name("state-difference-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let store = CanonicalObjectStore::new();
            let empty = rebuild(&store, &Entries::new());
            let roots = |last| SemanticRoots {
                data_bag: chain(&store, CHAIN_DEPTH, last),
                continuation_map: empty,
                join_bag: empty,
            };
            let from = roots(EndpointValue::Multiplicity(nonzero(1)));
            let to = roots(EndpointValue::Multiplicity(nonzero(2)));
            let depth = u64::try_from(CHAIN_DEPTH).expect("the depth fits u64");
            let limits = StateDifferenceLimits {
                max_path_bytes: depth,
                ..AMPLE
            };
            let (report, visits) = walk(&store, from, to, limits).expect("walk two deep chains");
            assert_eq!(
                visits,
                vec![(
                    SemanticNamespace::DataBag,
                    vec![b'x'; CHAIN_DEPTH - 1],
                    EndpointValue::Multiplicity(nonzero(1)),
                    EndpointValue::Multiplicity(nonzero(2)),
                )],
                "only the deepest endpoint differs"
            );
            assert_eq!(
                (report.object_reads, report.maximum_worklist, report.maximum_path_bytes),
                (2 * depth, 1, depth - 1),
                "the walk reads both chains once with one pending pair at a time"
            );
        })
        .expect("spawn the small-stack walk")
        .join()
        .expect("the walk completes on a 256 KiB stack");
}
