use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::Arc;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, publication_limits,
};
use super::support::PersistenceTrialDir;
use crate::rspace::bag::{BagDelta, Multiplicity};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::{
    ClosedDurableHistoryReader, DurableHotStoreBootstrapLimits, HotStoreInstances,
};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableAuthorityViewV1, DurableViewReaderV1, expansion_total,
    pack_file_name, recover_read_authority,
};
use crate::rspace::persistence::{
    CanonicalFormatError, ClosureError, EndpointValue, LogPackDecodeLimits, ObjectDigest, PackId,
    SemanticRoots,
};
use crate::rspace::rho_path::RhoPathV1;

type View = DurableAuthorityViewV1<Profile>;
type Reader = DurableViewReaderV1<Profile>;
type Continuation = WaitingContinuation<String, String>;
type Layout = &'static [(&'static [&'static str], &'static str)];

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 3] = ["v0", "v1", "v2"];
const BODIES: [&str; 2] = ["k0", "k1"];
const STORED_GROUPS: [&[&str]; 6] =
    [&["a"], &["b"], &["a", "b"], &["b", "a"], &["a", "a"], &["c", "a", "b"]];
const UNSTORED_GROUPS: [&[&str]; 3] = [&["c"], &["c", "a"], &["a", "b", "c"]];
const GENERATED_CASES: u32 = 64;

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn index_of(names: &[&str], value: &str) -> usize {
    names
        .iter()
        .position(|name| *name == value)
        .expect("an enumerated value belongs to the test universe")
}

fn stored_group<T: AsRef<str>>(channels: &[T]) -> usize {
    STORED_GROUPS
        .iter()
        .position(|names| {
            names.len() == channels.len() &&
                names
                    .iter()
                    .zip(channels)
                    .all(|(name, stored)| *name == stored.as_ref())
        })
        .expect("the group is one of the stored groups")
}

fn probed_groups() -> impl Iterator<Item = &'static [&'static str]> {
    STORED_GROUPS.into_iter().chain(UNSTORED_GROUPS)
}

fn occurrences(multiplicity: NonZeroU64) -> usize {
    usize::try_from(multiplicity.get()).expect("a test multiplicity fits usize")
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
    .expect("recover a durable view of the published state")
}

fn published_view(populate_state: impl FnOnce(&Store)) -> (PersistenceTrialDir, View) {
    let (scratch, hot, gate, mut owner) = fixture();
    populate_state(&hot);
    poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("publish and acknowledge the populated state");
    drop(owner);
    let view = recover(&scratch);
    (scratch, view)
}

fn roots_of(view: &View) -> SemanticRoots {
    view.semantic_roots()
        .expect("read the view's semantic roots")
}

fn reader_at(view: &View, roots: SemanticRoots) -> Reader {
    Reader::new(
        view.read_handle()
            .expect("take a read handle on the view's generation"),
        roots,
    )
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    data: Vec<(String, Vec<Datum<String>>)>,
    continuations: Vec<(Vec<String>, Vec<Continuation>)>,
    joins: Vec<(String, Vec<Vec<String>>)>,
}

fn observe(reader: &dyn HistoryReaderBase<String, String, String, String>) -> Observation {
    Observation {
        data: CHANNELS
            .into_iter()
            .map(|name| (channel(name), reader.get_data_canonical(&channel(name))))
            .collect(),
        continuations: probed_groups()
            .map(|names| (group(names), reader.get_continuations_canonical(&group(names))))
            .collect(),
        joins: CHANNELS
            .into_iter()
            .map(|name| (channel(name), reader.get_joins_canonical(&channel(name))))
            .collect(),
    }
}

fn eager(view: &View) -> Observation {
    let data = view
        .data_entries(u64::MAX, u64::MAX)
        .expect("enumerate the durable data");
    let continuations = view
        .continuation_entries(u64::MAX, u64::MAX, u64::MAX)
        .expect("enumerate the durable continuations");
    let joins = view
        .join_entries(u64::MAX, u64::MAX)
        .expect("enumerate the durable joins");
    assert!(
        data.iter()
            .all(|(key, _, _)| CHANNELS.contains(&key.as_str())),
        "a durable datum lies outside the probed channels: {data:?}"
    );
    assert!(
        continuations
            .iter()
            .all(|(channels, _)| probed_groups().any(|names| group(names) == *channels)),
        "a durable continuation lies outside the probed groups: {continuations:?}"
    );
    assert!(
        joins
            .iter()
            .all(|(key, _, _)| CHANNELS.contains(&key.as_str())),
        "a durable join lies outside the probed channels: {joins:?}"
    );
    Observation {
        data: CHANNELS
            .into_iter()
            .map(|name| {
                let values = data
                    .iter()
                    .filter(|(key, _, _)| key == name)
                    .flat_map(|(_, entry, multiplicity)| {
                        std::iter::repeat_n(entry.clone(), occurrences(*multiplicity))
                    })
                    .collect();
                (channel(name), values)
            })
            .collect(),
        continuations: probed_groups()
            .map(|names| {
                let key = group(names);
                let values = continuations
                    .iter()
                    .filter(|(channels, _)| *channels == key)
                    .map(|(_, entry)| entry.clone())
                    .collect();
                (key, values)
            })
            .collect(),
        joins: CHANNELS
            .into_iter()
            .map(|name| {
                let values = joins
                    .iter()
                    .filter(|(key, _, _)| key == name)
                    .flat_map(|(_, joined, multiplicity)| {
                        std::iter::repeat_n(joined.clone(), occurrences(*multiplicity))
                    })
                    .collect();
                (channel(name), values)
            })
            .collect(),
    }
}

#[derive(Clone, Debug)]
struct GeneratedState {
    data: Vec<(usize, usize)>,
    continuations: Vec<(usize, usize)>,
}

fn state(data: &[(&str, &str)], continuations: &[(&[&str], &str)]) -> GeneratedState {
    GeneratedState {
        data: data
            .iter()
            .map(|&(name, value)| (index_of(&CHANNELS, name), index_of(&VALUES, value)))
            .collect(),
        continuations: continuations
            .iter()
            .map(|&(names, body)| (stored_group(names), index_of(&BODIES, body)))
            .collect(),
    }
}

fn seed_states() -> [GeneratedState; 3] {
    [
        state(&[], &[]),
        state(&[("a", "v0"), ("a", "v0"), ("a", "v1"), ("b", "v2")], &[
            (&["a", "b"], "k0"),
            (&["a", "b"], "k1"),
            (&["b", "a"], "k0"),
            (&["a", "a"], "k1"),
            (&["c", "a", "b"], "k0"),
        ]),
        state(&[("c", "v2")], &[(&["b"], "k1")]),
    ]
}

fn generated_states() -> impl Strategy<Value = GeneratedState> {
    (
        prop::collection::vec((0..CHANNELS.len(), 0..VALUES.len()), 0..8),
        prop::collection::vec((0..STORED_GROUPS.len(), 0..BODIES.len()), 0..6),
    )
        .prop_map(|(data, continuations)| GeneratedState {
            data,
            continuations,
        })
}

fn apply(hot: &Store, state: &GeneratedState) {
    for &(channel_index, value_index) in &state.data {
        put_value(hot, CHANNELS[channel_index], VALUES[value_index]);
    }
    for &(group_index, body_index) in &state.continuations {
        put_continuation_with_joins(hot, STORED_GROUPS[group_index], BODIES[body_index]);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Contents {
    data: [[u64; VALUES.len()]; CHANNELS.len()],
    continuations: [[bool; BODIES.len()]; STORED_GROUPS.len()],
    joins: BTreeSet<(usize, usize)>,
}

fn modelled(state: &GeneratedState) -> Contents {
    let mut contents = Contents::default();
    for &(channel_index, value_index) in &state.data {
        contents.data[channel_index][value_index] += 1;
    }
    for &(group_index, body_index) in &state.continuations {
        contents.continuations[group_index][body_index] = true;
        for member in STORED_GROUPS[group_index] {
            contents
                .joins
                .insert((index_of(&CHANNELS, member), group_index));
        }
    }
    contents
}

fn durable_contents(view: &View) -> Contents {
    let mut contents = Contents::default();
    for (key, entry, multiplicity) in view
        .data_entries(u64::MAX, u64::MAX)
        .expect("enumerate the durable data")
    {
        let channel_index = index_of(&CHANNELS, &key);
        let value_index = index_of(&VALUES, &entry.a);
        assert_eq!(entry, datum(CHANNELS[channel_index], VALUES[value_index]));
        contents.data[channel_index][value_index] += multiplicity.get();
    }
    for (channels, entry) in view
        .continuation_entries(u64::MAX, u64::MAX, u64::MAX)
        .expect("enumerate the durable continuations")
    {
        let group_index = stored_group(&channels);
        let body_index = index_of(&BODIES, &entry.continuation);
        assert_eq!(entry, continuation(STORED_GROUPS[group_index], BODIES[body_index]));
        assert!(
            !contents.continuations[group_index][body_index],
            "a durable continuation was enumerated twice: {entry:?}"
        );
        contents.continuations[group_index][body_index] = true;
    }
    for (key, joined, multiplicity) in view
        .join_entries(u64::MAX, u64::MAX)
        .expect("enumerate the durable joins")
    {
        assert_eq!(multiplicity.get(), 1, "a durable join has a multiplicity above one");
        assert!(
            contents
                .joins
                .insert((index_of(&CHANNELS, &key), stored_group(&joined))),
            "a durable join was enumerated twice: {key:?} {joined:?}"
        );
    }
    contents
}

#[derive(Clone, Copy, Debug, Default)]
struct ReadCoverage {
    states: u32,
    empty_states: u32,
    channels_with_two_datum_endpoints: u32,
    repeated_data: u32,
    groups_with_two_continuations: u32,
    both_orders_of_one_pair: u32,
    groups_repeating_a_channel: u32,
    stored_groups_extending_a_probed_group: u32,
    channels_in_only_one_of_data_and_joins: u32,
}

impl ReadCoverage {
    fn of(contents: &Contents) -> Self {
        let stored = |names: &[&str]| contents.continuations[stored_group(names)].contains(&true);
        let datum_endpoints = |channel_index: usize| {
            contents.data[channel_index]
                .iter()
                .filter(|&&count| count > 0)
                .count()
        };
        let joined = |channel_index: usize| {
            contents
                .joins
                .iter()
                .any(|&(member, _)| member == channel_index)
        };
        Self {
            states: 1,
            empty_states: u32::from(*contents == Contents::default()),
            channels_with_two_datum_endpoints: u32::from(
                (0..CHANNELS.len()).any(|index| datum_endpoints(index) >= 2),
            ),
            repeated_data: u32::from(contents.data.iter().flatten().any(|&count| count >= 2)),
            groups_with_two_continuations: u32::from(
                contents
                    .continuations
                    .iter()
                    .any(|bodies| bodies.iter().all(|&present| present)),
            ),
            both_orders_of_one_pair: u32::from(stored(&["a", "b"]) && stored(&["b", "a"])),
            groups_repeating_a_channel: u32::from(stored(&["a", "a"])),
            stored_groups_extending_a_probed_group: u32::from(stored(&["c", "a", "b"])),
            channels_in_only_one_of_data_and_joins: u32::from(
                (0..CHANNELS.len()).any(|index| (datum_endpoints(index) > 0) != joined(index)),
            ),
        }
    }

    fn merge(&mut self, other: Self) {
        let Self {
            states,
            empty_states,
            channels_with_two_datum_endpoints,
            repeated_data,
            groups_with_two_continuations,
            both_orders_of_one_pair,
            groups_repeating_a_channel,
            stored_groups_extending_a_probed_group,
            channels_in_only_one_of_data_and_joins,
        } = other;
        self.states += states;
        self.empty_states += empty_states;
        self.channels_with_two_datum_endpoints += channels_with_two_datum_endpoints;
        self.repeated_data += repeated_data;
        self.groups_with_two_continuations += groups_with_two_continuations;
        self.both_orders_of_one_pair += both_orders_of_one_pair;
        self.groups_repeating_a_channel += groups_repeating_a_channel;
        self.stored_groups_extending_a_probed_group += stored_groups_extending_a_probed_group;
        self.channels_in_only_one_of_data_and_joins += channels_in_only_one_of_data_and_joins;
    }

    fn assert_complete(&self) {
        let Self {
            states,
            empty_states,
            channels_with_two_datum_endpoints,
            repeated_data,
            groups_with_two_continuations,
            both_orders_of_one_pair,
            groups_repeating_a_channel,
            stored_groups_extending_a_probed_group,
            channels_in_only_one_of_data_and_joins,
        } = *self;
        assert!(
            [
                states,
                empty_states,
                channels_with_two_datum_endpoints,
                repeated_data,
                groups_with_two_continuations,
                both_orders_of_one_pair,
                groups_repeating_a_channel,
                stored_groups_extending_a_probed_group,
                channels_in_only_one_of_data_and_joins,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of durable state never occurred: {self:?}"
        );
    }
}

fn check_lazy_reads(state: &GeneratedState) -> Result<ReadCoverage, TestCaseError> {
    let (_scratch, view) = published_view(|hot| apply(hot, state));
    let contents = durable_contents(&view);
    prop_assert_eq!(&contents, &modelled(state));
    let reader = reader_at(&view, roots_of(&view));
    prop_assert_eq!(observe(&reader), eager(&view));
    Ok(ReadCoverage::of(&contents))
}

#[test]
fn lazy_reads_equal_the_eager_enumerations_of_every_key() {
    let mut seeded = ReadCoverage::default();
    for seed in seed_states() {
        seeded.merge(check_lazy_reads(&seed).expect("a seeded durable state satisfies VR-02"));
    }
    println!("durable view reader seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(ReadCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_states(), |state| {
        let mut total = generated.get();
        total.merge(check_lazy_reads(&state)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated durable states satisfy VR-02");
    assert!(generated.get().states >= GENERATED_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("durable view reader: {merged:?}");
    merged.assert_complete();
}

#[test]
fn lazy_reads_return_the_pinned_state_after_a_successor_is_published() {
    let (scratch, view) = published_view(|hot| {
        put_value(hot, "a", "v0");
        put_value(hot, "a", "v0");
        put_value(hot, "a", "v1");
        put_value(hot, "b", "v0");
        put_continuation_with_joins(hot, &["a", "b"], "k0");
        put_continuation_with_joins(hot, &["b", "a"], "k0");
        put_continuation_with_joins(hot, &["a", "a"], "k0");
    });
    let pinned_roots = roots_of(&view);
    let pinned = eager(&view);
    let reader = reader_at(&view, pinned_roots);

    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("bootstrap the hot store from the pinned view"),
    );
    let gate = Arc::new(RwLock::new(()));
    let mut owner = view
        .into_rspace_authority(&hot)
        .unwrap_or_else(|failure| panic!("{:?}", failure.1));
    remove_value(&hot, "a", "v0");
    put_value(&hot, "c", "v2");
    remove_continuation_with_joins(&hot, &["b", "a"], "k0");
    put_continuation_with_joins(&hot, &["a", "b"], "k1");
    put_continuation_with_joins(&hot, &["c", "a", "b"], "k0");
    let successor_state = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("publish and acknowledge the successor");
    let while_the_owner_lives = observe(&reader);
    drop(owner);
    let after_owner_teardown = observe(&reader);

    let successor_view = recover(&scratch);
    assert_eq!(successor_view.state(), successor_state);
    let successor = eager(&successor_view);
    let retained = reader_at(&successor_view, pinned_roots);
    assert_ne!(successor.data, pinned.data);
    assert_ne!(successor.continuations, pinned.continuations);
    assert_ne!(successor.joins, pinned.joins);
    assert_eq!(
        while_the_owner_lives, pinned,
        "a reader taken before into_rspace_authority must read its pinned state while the owner \
         holds the successor"
    );
    assert_eq!(
        after_owner_teardown, pinned,
        "a reader must read its pinned state after the owner is torn down"
    );
    assert_eq!(
        observe(&retained),
        pinned,
        "a reader on the successor's generation must read the retained state its roots name"
    );
}

#[test]
fn a_receive_group_is_read_only_at_its_literal_ordered_prefix() {
    let layouts: [Layout; 3] = [&[(&["a", "b"], "k0")], &[(&["b", "a"], "k1")], &[
        (&["a", "b"], "k0"),
        (&["b", "a"], "k1"),
    ]];
    let probes: [&[&str]; 5] = [&["a", "b"], &["b", "a"], &["a"], &["b"], &["a", "a"]];
    for stored in layouts {
        let (_scratch, view) = published_view(|hot| {
            for &(names, body) in stored {
                put_continuation_with_joins(hot, names, body);
            }
        });
        let reader = reader_at(&view, roots_of(&view));
        for names in probes {
            let expected: Vec<Continuation> = stored
                .iter()
                .filter(|&&(stored_names, _)| stored_names == names)
                .map(|&(stored_names, body)| continuation(stored_names, body))
                .collect();
            assert_eq!(
                reader.get_continuations_canonical(&group(names)),
                expected,
                "reading {names:?} with records under {stored:?}"
            );
        }
    }
}

fn corrupt_in_live_packs(root: &Path, view: &View, canonical: &[u8]) -> Vec<u8> {
    let mut corrupted = canonical.to_vec();
    let last = corrupted
        .last_mut()
        .expect("a canonical object is never empty");
    *last ^= 0x01;
    let flipped = *last;
    let packs = view.catalog().pack();
    let mut rewritten = 0_usize;
    for pack in packs.first_live_pack()..=packs.active_pack() {
        let path = root.join(pack_file_name(PackId::new(pack).expect("a live pack id is nonzero")));
        let bytes = fs::read(&path).expect("read a live pack");
        let offsets: Vec<u64> = bytes
            .windows(canonical.len())
            .enumerate()
            .filter(|(_, window)| *window == canonical)
            .map(|(offset, _)| {
                u64::try_from(offset + canonical.len() - 1).expect("a pack offset fits u64")
            })
            .collect();
        let mut file = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open a live pack for an in-place write");
        for offset in offsets {
            file.seek(SeekFrom::Start(offset))
                .expect("seek to the stored byte");
            file.write_all(&[flipped]).expect("flip the stored byte");
            rewritten += 1;
        }
        file.sync_all().expect("sync the rewritten pack");
    }
    assert!(rewritten > 0, "the continuation source must be stored in a live pack");
    corrupted
}

#[test]
fn a_corrupted_continuation_source_panics_with_its_typed_error_where_the_closed_reader_is_empty() {
    let names: &[&str] = &["a", "b"];
    let (scratch, view) = published_view(|hot| put_continuation_with_joins(hot, names, "k0"));
    let key = group(names);
    let roots = roots_of(&view);
    let reference = match view
        .project_prefix(
            roots.continuation_map,
            &RhoPathV1::continuation_prefix(&key),
            u64::MAX,
            u64::MAX,
        )
        .expect("project the group's continuation records")
        .as_slice()
    {
        [(_, EndpointValue::Content(reference))] => *reference,
        other => panic!("the group must hold one continuation record with a source: {other:?}"),
    };
    let source = view
        .get_object_bounded(&reference.digest(), reference.encoded_len())
        .expect("read the stored continuation source")
        .expect("the continuation source is indexed");
    let corrupted = corrupt_in_live_packs(scratch.path(), &view, &source);

    let closed: &dyn HistoryReaderBase<String, String, String, String> =
        &ClosedDurableHistoryReader;
    assert!(closed.get_continuations_canonical(&key).is_empty());
    let refusal = view
        .continuations_for_channels(&key, u64::MAX, u64::MAX, u64::MAX)
        .expect_err("the view refuses the corrupted continuation source");
    assert!(
        matches!(
            &refusal,
            ClosureError::Format(CanonicalFormatError::DigestMismatch { expected, actual })
                if *expected == reference.digest() &&
                    *actual == ObjectDigest::of_canonical_bytes(&corrupted)
        ),
        "{refusal:?}"
    );
    let reader = reader_at(&view, roots);
    let failure = reader
        .try_get_continuations_canonical(&key)
        .expect_err("the reader fails closed on the corrupted continuation source");
    let message = format!("{failure:?}");
    assert!(
        message == format!("{refusal:?}"),
        "the reader's panic must carry the typed error {refusal:?}, found {message:?}"
    );
}

#[derive(Clone, Copy, Debug)]
enum KeyedRead {
    Data,
    Continuations,
    Joins,
}

#[derive(Clone, Copy, Debug)]
enum ReaderLimit {
    Steps,
    Endpoints,
    PayloadBytes,
    JoinOccurrences,
}

#[derive(Debug, PartialEq, Eq)]
enum ReadResult {
    Data(Vec<Datum<String>>),
    Continuations(Vec<Continuation>),
    Joins(Vec<Vec<String>>),
}

impl ReadResult {
    fn is_empty(&self) -> bool {
        match self {
            Self::Data(values) => values.is_empty(),
            Self::Continuations(values) => values.is_empty(),
            Self::Joins(values) => values.is_empty(),
        }
    }
}

const BOUND_READS: [(KeyedRead, ReaderLimit); 8] = [
    (KeyedRead::Data, ReaderLimit::Steps),
    (KeyedRead::Data, ReaderLimit::Endpoints),
    (KeyedRead::Continuations, ReaderLimit::Steps),
    (KeyedRead::Continuations, ReaderLimit::Endpoints),
    (KeyedRead::Continuations, ReaderLimit::PayloadBytes),
    (KeyedRead::Joins, ReaderLimit::Steps),
    (KeyedRead::Joins, ReaderLimit::Endpoints),
    (KeyedRead::Joins, ReaderLimit::JoinOccurrences),
];

fn bounded(limit: ReaderLimit, value: u64) -> DurableHotStoreBootstrapLimits {
    let unbounded = DurableHotStoreBootstrapLimits::default();
    match limit {
        ReaderLimit::Steps => DurableHotStoreBootstrapLimits {
            max_steps: value,
            ..unbounded
        },
        ReaderLimit::Endpoints => DurableHotStoreBootstrapLimits {
            max_endpoints: value,
            ..unbounded
        },
        ReaderLimit::PayloadBytes => DurableHotStoreBootstrapLimits {
            max_payload_bytes: value,
            ..unbounded
        },
        ReaderLimit::JoinOccurrences => DurableHotStoreBootstrapLimits {
            max_join_occurrences: value,
            ..unbounded
        },
    }
}

fn view_read(
    view: &View,
    read: KeyedRead,
    limits: DurableHotStoreBootstrapLimits,
) -> Result<ReadResult, String> {
    let DurableHotStoreBootstrapLimits {
        max_steps,
        max_endpoints,
        max_payload_bytes,
        max_join_occurrences,
    } = limits;
    let result = match read {
        KeyedRead::Data => view
            .data_for_channel(&channel("a"), max_steps, max_endpoints, u64::MAX)
            .map(ReadResult::Data),
        KeyedRead::Continuations => view
            .continuations_for_channels(
                &group(&["a", "b"]),
                max_steps,
                max_endpoints,
                max_payload_bytes,
            )
            .map(ReadResult::Continuations),
        KeyedRead::Joins => view
            .joins_for_channel(&channel("a"), max_steps, max_endpoints, max_join_occurrences)
            .map(ReadResult::Joins),
    };
    result.map_err(|error| format!("{error:?}"))
}

fn reader_read(reader: &Reader, read: KeyedRead) -> ReadResult {
    match read {
        KeyedRead::Data => ReadResult::Data(reader.get_data_canonical(&channel("a"))),
        KeyedRead::Continuations => {
            ReadResult::Continuations(reader.get_continuations_canonical(&group(&["a", "b"])))
        }
        KeyedRead::Joins => ReadResult::Joins(reader.get_joins_canonical(&channel("a"))),
    }
}

fn try_reader_read(reader: &Reader, read: KeyedRead) -> Result<ReadResult, String> {
    let result = match read {
        KeyedRead::Data => reader
            .try_get_data_canonical(&channel("a"))
            .map(ReadResult::Data),
        KeyedRead::Continuations => reader
            .try_get_continuations_canonical(&group(&["a", "b"]))
            .map(ReadResult::Continuations),
        KeyedRead::Joins => reader
            .try_get_joins_canonical(&channel("a"))
            .map(ReadResult::Joins),
    };
    result.map_err(|error| format!("{error:?}"))
}

fn requirement(view: &View, read: KeyedRead, limit: ReaderLimit) -> u64 {
    let (mut low, mut high) = (0_u64, u64::MAX);
    while low < high {
        let middle = low + (high - low) / 2;
        match view_read(view, read, bounded(limit, middle)) {
            Ok(_) => high = middle,
            Err(_) => low = middle + 1,
        }
    }
    low
}

#[test]
fn every_reader_limit_binds_its_read_and_panics_with_the_typed_error() {
    let (_scratch, view) = published_view(|hot| {
        put_value(hot, "a", "v0");
        put_value(hot, "a", "v0");
        put_value(hot, "a", "v1");
        put_continuation_with_joins(hot, &["a"], "k0");
        put_continuation_with_joins(hot, &["a", "b"], "k0");
        put_continuation_with_joins(hot, &["a", "b"], "k1");
    });
    let roots = roots_of(&view);
    let reader = |limits| reader_at(&view, roots).with_limits(limits);
    for (read, limit) in BOUND_READS {
        let unbounded = view_read(&view, read, DurableHotStoreBootstrapLimits::default())
            .expect("an unbounded read succeeds");
        assert!(!unbounded.is_empty(), "{read:?} must read records");
        let required = requirement(&view, read, limit);
        let below = required
            .checked_sub(1)
            .expect("every bound read needs a positive budget");
        assert_eq!(
            reader_read(&reader(bounded(limit, required)), read),
            unbounded,
            "{read:?} at its {limit:?} requirement {required}"
        );
        let refusal = view_read(&view, read, bounded(limit, below))
            .expect_err("one below the requirement is refused");
        let message = try_reader_read(&reader(bounded(limit, below)), read)
            .expect_err("the reader fails closed one below the requirement");
        assert!(
            message == refusal,
            "{read:?} under {limit:?} {below}: the panic must carry {refusal}, found {message:?}"
        );
    }
}

#[test]
fn a_multiplicity_total_past_u64_max_is_a_typed_limit_error_in_either_visit_order() {
    let (_scratch, view) = published_view(|hot| {
        for (name, small, large) in [("a", "v0", "v1"), ("b", "v1", "v0")] {
            put_value(hot, name, small);
            let mut delta = BagDelta::new();
            delta
                .record_addition(
                    &RSpaceDataIndex::datum_path(&channel(name), &datum(name, large)),
                    Multiplicity::new(u64::MAX).expect("u64::MAX is a nonzero multiplicity"),
                )
                .expect("record the addition of the large datum");
            hot.apply_datum_delta(&delta)
                .expect("the hot store accepts a datum of multiplicity u64::MAX");
        }
    });
    let entries = view
        .data_entries(u64::MAX, u64::MAX)
        .expect("enumerate the durable data");
    let reader = reader_at(&view, roots_of(&view));
    let mut visit_orders = BTreeSet::new();
    for name in ["a", "b"] {
        let multiplicities: Vec<u64> = entries
            .iter()
            .filter(|(key, _, _)| key == name)
            .map(|(_, _, multiplicity)| multiplicity.get())
            .collect();
        let expected = match multiplicities.as_slice() {
            [u64::MAX, 1] => CanonicalFormatError::AllocationFailure {
                requested: u64::MAX,
            },
            [1, u64::MAX] => CanonicalFormatError::ResourceLimit {
                resource: "durable data materialization",
                limit: u64::MAX,
                attempted: u64::MAX,
            },
            other => {
                panic!("channel {name} must hold the multiplicities 1 and u64::MAX: {other:?}")
            }
        };
        let refusal = view
            .data_for_channel(&channel(name), u64::MAX, u64::MAX, u64::MAX)
            .expect_err("the view refuses to expand the channel's multiplicities");
        assert!(
            matches!(&refusal, ClosureError::Format(error) if *error == expected),
            "channel {name} with multiplicities {multiplicities:?}: expected {expected:?}, found \
             {refusal:?}"
        );
        let failure = reader
            .try_get_data_canonical(&channel(name))
            .expect_err("the reader fails closed on the channel's multiplicities");
        let message = format!("{failure:?}");
        assert!(
            message == format!("{refusal:?}"),
            "channel {name}: the reader's panic must carry the typed error {refusal:?}, found \
             {message:?}"
        );
        visit_orders.insert(multiplicities);
    }
    assert_eq!(
        visit_orders,
        BTreeSet::from([vec![1, u64::MAX], vec![u64::MAX, 1]]),
        "one channel must visit its large record first and the other second"
    );
}

#[test]
fn expansion_total_is_exact_at_the_limit_and_typed_past_it() {
    const EXPANDED: usize = 2;
    const BOUNDED_LIMITS: [u64; 2] = [3, u64::MAX - 1];
    let expanded = u64::try_from(EXPANDED).expect("the expanded count fits u64");
    for resource in ["durable data materialization", "durable join materialization"] {
        let refused = |limit: u64, attempted: u64| -> Result<u64, CanonicalFormatError> {
            Err(CanonicalFormatError::ResourceLimit {
                resource,
                limit,
                attempted,
            })
        };
        for limit in BOUNDED_LIMITS.into_iter().chain([u64::MAX]) {
            assert_eq!(
                expansion_total(EXPANDED, limit - expanded, limit, resource),
                Ok(limit),
                "{resource}: a total equal to the limit {limit} is returned"
            );
            assert_eq!(
                expansion_total(EXPANDED, u64::MAX, limit, resource),
                refused(limit, u64::MAX),
                "{resource}: a total past u64::MAX under the limit {limit} attempts u64::MAX"
            );
        }
        for limit in BOUNDED_LIMITS {
            assert_eq!(
                expansion_total(EXPANDED, limit - expanded + 1, limit, resource),
                refused(limit, limit + 1),
                "{resource}: a total of the limit {limit} plus one attempts that total"
            );
        }
        assert_eq!(
            expansion_total(EXPANDED, u64::MAX - expanded + 1, u64::MAX, resource),
            refused(u64::MAX, u64::MAX),
            "{resource}: the least total past u64::MAX attempts u64::MAX under the limit u64::MAX"
        );
    }
}
