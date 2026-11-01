use std::collections::BTreeSet;
use std::convert::Infallible;
use std::sync::Arc;

use proptest::prelude::*;
use serde::Serialize;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::ordered_receive::PreparedContinuationRecord;
use crate::rspace::persistence::ContentTransition;
use crate::rspace::record_index::{CanonicalRecordGroup, RSpaceContinuationStore};
use crate::rspace::serializers::serializers::encode_continuation_identity;
use crate::rspace::trace::event::Consume;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct RawChannel(Vec<u8>);

impl StableHashSerialize for RawChannel {}

impl RhoPathEncode for RawChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.0.clone() }
}

type TestStore = RSpaceContinuationStore<RawChannel, String, String>;

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];

const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];

const GROUPS: [&[usize]; 10] =
    [&[0], &[1], &[2], &[3], &[4], &[2, 3], &[3, 2], &[2, 4, 2], &[4, 3, 1], &[0, 1, 2, 3, 4]];

fn channels() -> Vec<RawChannel> {
    IMAGES
        .iter()
        .map(|image| RawChannel(image.to_vec()))
        .collect()
}

fn absent_channel() -> RawChannel { RawChannel(ABSENT_IMAGE.to_vec()) }

fn probe_channels() -> Vec<RawChannel> {
    let mut probes = Vec::with_capacity(IMAGES.len().saturating_add(1));
    probes.extend(channels());
    probes.push(absent_channel());
    probes
}

fn universe() -> Vec<Vec<RawChannel>> {
    let channels = channels();
    GROUPS
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|position| channels[*position].clone())
                .collect()
        })
        .collect()
}

fn framed(image: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(image.len().saturating_mul(2).saturating_add(2));
    for byte in image {
        match byte {
            0x00 => frame.extend_from_slice(&[0x00, 0xFF]),
            _ => frame.push(*byte),
        }
    }
    frame.extend_from_slice(&[0x00, 0x00]);
    frame
}

fn group_prefix(group: &[RawChannel]) -> Vec<u8> {
    let capacity = group.iter().fold(8_usize, |total, channel| {
        total
            .saturating_add(channel.0.len().saturating_mul(2))
            .saturating_add(2)
    });
    let mut prefix = Vec::with_capacity(capacity);
    prefix.extend_from_slice(b"RHP\x01\x02\x01");
    for channel in group {
        prefix.extend_from_slice(&framed(&channel.0));
    }
    prefix.extend_from_slice(&[0x00, 0x01]);
    prefix
}

fn expected_keys<'g>(supported: impl IntoIterator<Item = &'g Vec<RawChannel>>) -> Vec<Vec<u8>> {
    let mut keys = BTreeSet::new();
    for group in supported {
        let prefix = group_prefix(group);
        for channel in group {
            let mut key = framed(&channel.0);
            key.extend_from_slice(&prefix);
            keys.insert(key);
        }
    }
    keys.into_iter().collect()
}

fn expected_groups_of(channel: &RawChannel, supported: &[&Vec<RawChannel>]) -> Vec<Vec<u8>> {
    supported
        .iter()
        .filter(|group| group.contains(channel))
        .map(|group| group_prefix(group))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn visited_groups(index: &ReceiveOccurrenceIndex, channel: &RawChannel) -> Vec<Vec<u8>> {
    let mut visited = Vec::new();
    let visit = index.try_visit_groups_of(&channel_frame(channel), |prefix| {
        visited.push(prefix.to_vec());
        Ok::<(), Infallible>(())
    });
    match visit {
        Ok(()) => visited,
        Err(error) => match error {},
    }
}

fn continuation(name: &str, source: u8, persist: bool) -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: Arc::new(vec![name.to_owned()]),
        continuation: Arc::new(format!("k-{name}")),
        persist,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![source; 32]),
            persistent: persist,
        },
    }
}

fn put(
    store: &mut TestStore,
    group: &[RawChannel],
    value: WaitingContinuation<String, String>,
) -> bool {
    let prepared = PreparedContinuationRecord::encode_waiting(&value);
    store.put_prepared(group, value, prepared)
}

fn remove_slot(store: &mut TestStore, group: &[RawChannel], slot: usize) -> bool {
    let path = store
        .resting_records(group)
        .and_then(|records| records.get(slot).map(|record| Arc::clone(record.path())));
    match path {
        Some(path) => store.remove_path(group, &path).is_some(),
        None => false,
    }
}

fn supported_groups<'u>(
    store: &TestStore,
    universe: &'u [Vec<RawChannel>],
) -> Vec<&'u Vec<RawChannel>> {
    universe
        .iter()
        .filter(|group| {
            store.resting_len(group).is_some_and(|len| len != 0) || store.contains_installed(group)
        })
        .collect()
}

fn index_of(store: &TestStore) -> &ReceiveOccurrenceIndex {
    store
        .receive_index()
        .expect("the receive occurrence index is present")
}

fn assert_index_matches_support(store: &TestStore, universe: &[Vec<RawChannel>]) {
    let index = index_of(store);
    let supported = supported_groups(store, universe);
    assert_eq!(index.keys(), expected_keys(supported.iter().copied()));
    for channel in probe_channels() {
        assert_eq!(visited_groups(index, &channel), expected_groups_of(&channel, &supported));
    }
    let rebuilt = store
        .build_receive_index()
        .expect("every stored continuation group prefix parses");
    assert!(index.same_keys(&rebuilt));
}

fn canonical_groups(store: &TestStore) -> Vec<CanonicalRecordGroup> {
    let mut groups = Vec::with_capacity(store.loaded_group_count());
    store
        .try_visit_loaded_canonical_records(|group| {
            groups.push(group);
            Ok(())
        })
        .expect("loaded continuation groups replay canonically");
    groups
}

fn content_journal(store: &TestStore) -> Vec<(Vec<u8>, ContentTransition)> {
    store
        .pending_content()
        .iter()
        .map(|(path, transition)| (path, *transition))
        .collect()
}

fn source_journal(store: &TestStore) -> Vec<(Vec<u8>, Arc<[u8]>)> {
    store
        .pending_source_bytes()
        .iter()
        .map(|(path, bytes)| (path, Arc::clone(bytes)))
        .collect()
}

fn mutate_scripted(store: &mut TestStore, groups: &[Vec<RawChannel>]) {
    store.set_group(groups[0].clone(), vec![continuation("a", 1, false)]);
    assert!(put(store, &groups[5], continuation("b", 2, false)));
    assert!(put(store, &groups[5], continuation("c", 3, true)));
    assert!(!put(store, &groups[5], continuation("b", 7, false)));
    store.install(&groups[7], continuation("d", 4, true));
    assert!(remove_slot(store, &groups[5], 0));
    store.set_group(groups[9].clone(), Vec::new());
    assert!(put(store, &groups[9], continuation("e", 5, false)));
    assert!(put(store, &groups[8], continuation("f", 6, false)));
    assert!(remove_slot(store, &groups[8], 0));
}

#[test]
fn occurrence_keys_are_exact_under_prefix_aliased_images() {
    let groups = universe();
    for channel in probe_channels() {
        assert_eq!(channel_frame(&channel), framed(&channel.0));
    }
    let mut index = ReceiveOccurrenceIndex::new();
    for group in &groups {
        let prefix = RhoPathV1::continuation_prefix(group);
        assert_eq!(prefix, group_prefix(group));
        index
            .insert_group(&prefix)
            .expect("a typed continuation group prefix parses");
    }
    let every_group = groups.iter().collect::<Vec<_>>();
    let keys = index.keys();
    assert_eq!(keys, expected_keys(every_group.iter().copied()));
    let distinct_occurrences = groups
        .iter()
        .map(|group| group.iter().collect::<BTreeSet<_>>().len())
        .sum::<usize>();
    assert_eq!(keys.len(), distinct_occurrences);
    for key in &keys {
        let owners = groups
            .iter()
            .flat_map(|group| group.iter().map(move |channel| (group, channel)))
            .filter(|(group, channel)| {
                key.strip_prefix(framed(&channel.0).as_slice()) ==
                    Some(group_prefix(group).as_slice())
            })
            .map(|(group, _)| group)
            .collect::<BTreeSet<_>>();
        assert_eq!(owners.len(), 1);
    }
    for channel in probe_channels() {
        assert_eq!(visited_groups(&index, &channel), expected_groups_of(&channel, &every_group));
    }
    assert!(visited_groups(&index, &absent_channel()).is_empty());
    let mut visits = 0_usize;
    let stopped = index.try_visit_groups_of(&channel_frame(&channels()[2]), |_| {
        visits = visits.checked_add(1).expect("the visit count fits usize");
        Err(visits)
    });
    assert_eq!(stopped, Err(1));
    assert_eq!(visits, 1);
}

#[test]
fn malformed_group_prefixes_are_refused_without_touching_the_index() {
    let groups = universe();
    let mut index = ReceiveOccurrenceIndex::new();
    let resident = RhoPathV1::continuation_prefix(&groups[5]);
    index
        .insert_group(&resident)
        .expect("a typed continuation group prefix parses");
    let before = index.keys();
    let mut trailing = resident.clone();
    trailing.push(0x07);
    let truncated = resident[..resident
        .len()
        .checked_sub(2)
        .expect("a group prefix ends with its group terminator")]
        .to_vec();
    let cases = [
        (b"RHX\x01\x02\x01\x00\x01".to_vec(), RhoPathError::InvalidMagic),
        (b"RHP\x01\x01\x01\x00\x01".to_vec(), RhoPathError::InvalidKind(1)),
        (b"RHP\x01\x02\x02\x00\x01".to_vec(), RhoPathError::InvalidCollectionVersion(2)),
        (b"RHP\x01\x02\x01\x00\x07\x00\x01".to_vec(), RhoPathError::InvalidEscape(0x07)),
        (trailing, RhoPathError::TrailingBytes),
        (truncated, RhoPathError::TruncatedFrame),
    ];
    for (prefix, error) in &cases {
        assert_eq!(index.insert_group(prefix), Err(*error));
        assert_eq!(index.keys(), before);
        assert_eq!(index.remove_group(prefix), Err(*error));
        assert_eq!(index.keys(), before);
    }
}

#[test]
fn repeated_channels_contribute_one_occurrence_key_per_distinct_channel() {
    let channels = channels();
    let groups = universe();
    let repeated = &groups[7];
    assert_eq!(*repeated, vec![channels[2].clone(), channels[4].clone(), channels[2].clone()]);
    let prefix = RhoPathV1::continuation_prefix(repeated);
    let mut index = ReceiveOccurrenceIndex::new();
    index
        .insert_group(&prefix)
        .expect("a typed continuation group prefix parses");
    assert_eq!(index.keys(), expected_keys([repeated]));
    assert_eq!(index.keys().len(), 2);
    assert_eq!(visited_groups(&index, &channels[2]), vec![prefix.clone()]);
    assert_eq!(visited_groups(&index, &channels[4]), vec![prefix.clone()]);
    assert!(visited_groups(&index, &channels[3]).is_empty());
    index
        .remove_group(&prefix)
        .expect("a typed continuation group prefix parses");
    assert!(index.keys().is_empty());

    let mut store = TestStore::new();
    assert!(store.rebuild_receive_index());
    assert!(put(&mut store, repeated, continuation("repeated", 1, false)));
    assert_eq!(index_of(&store).keys(), expected_keys([repeated]));
    assert_index_matches_support(&store, &groups);
}

#[test]
fn installed_only_groups_stay_indexed_across_resting_changes() {
    let channels = channels();
    let groups = universe();
    let group = &groups[5];
    let prefix = group_prefix(group);
    let mut store = TestStore::new();
    assert!(store.rebuild_receive_index());
    store.install(group, continuation("installed", 1, true));
    assert!(!store.is_loaded(group));
    assert_eq!(visited_groups(index_of(&store), &channels[2]), vec![prefix.clone()]);
    assert_eq!(visited_groups(index_of(&store), &channels[3]), vec![prefix.clone()]);
    assert_index_matches_support(&store, &groups);
    store.install(group, continuation("replacement", 2, true));
    assert_index_matches_support(&store, &groups);
    store.set_group(group.clone(), Vec::new());
    assert!(store.is_loaded(group));
    assert_eq!(visited_groups(index_of(&store), &channels[2]), vec![prefix.clone()]);
    assert!(put(&mut store, group, continuation("resting", 3, false)));
    assert!(remove_slot(&mut store, group, 0));
    assert_eq!(store.resting_len(group), Some(0));
    assert_eq!(visited_groups(index_of(&store), &channels[3]), vec![prefix]);
    assert_index_matches_support(&store, &groups);
}

#[test]
fn final_resting_removal_without_an_installed_slot_drops_the_group_keys() {
    let channels = channels();
    let groups = universe();
    let group = &groups[8];
    let mut store = TestStore::new();
    assert!(store.rebuild_receive_index());
    assert!(put(&mut store, group, continuation("first", 1, false)));
    assert!(put(&mut store, group, continuation("second", 2, false)));
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    assert!(remove_slot(&mut store, group, 0));
    assert_eq!(store.resting_len(group), Some(1));
    assert_eq!(visited_groups(index_of(&store), &channels[1]), vec![group_prefix(group)]);
    assert!(store.remove_path(group, b"absent record path").is_none());
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    assert!(remove_slot(&mut store, group, 0));
    assert_eq!(store.resting_len(group), Some(0));
    assert!(index_of(&store).keys().is_empty());
    for channel in &channels {
        assert!(visited_groups(index_of(&store), channel).is_empty());
    }
    assert!(!remove_slot(&mut store, group, 0));
    assert!(index_of(&store).keys().is_empty());
    assert_index_matches_support(&store, &groups);
}

#[test]
fn set_group_transitions_toggle_group_support() {
    let groups = universe();
    let group = &groups[9];
    let mut store = TestStore::new();
    assert!(store.rebuild_receive_index());
    store.set_group(group.clone(), vec![continuation("a", 1, false)]);
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    store.set_group(group.clone(), Vec::new());
    assert!(store.is_loaded(group));
    assert!(index_of(&store).keys().is_empty());
    store.set_group(group.clone(), vec![continuation("a", 1, false), continuation("b", 2, true)]);
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    store.set_group(group.clone(), vec![continuation("c", 3, false)]);
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    store.set_group(groups[0].clone(), Vec::new());
    assert!(store.is_loaded(&groups[0]));
    assert_eq!(index_of(&store).keys(), expected_keys([group]));
    assert_index_matches_support(&store, &groups);
}

#[test]
fn clear_leaves_the_occurrence_index_absent_until_rebuilt() {
    let groups = universe();
    let mut store = TestStore::new();
    assert!(store.receive_index().is_none());
    assert!(put(&mut store, &groups[0], continuation("unindexed", 1, false)));
    assert!(store.receive_index().is_none());
    assert!(store.rebuild_receive_index());
    assert_eq!(index_of(&store).keys(), expected_keys([&groups[0]]));
    store.clear();
    assert!(store.receive_index().is_none());
    assert!(put(&mut store, &groups[1], continuation("after-clear", 2, false)));
    store.install(&groups[2], continuation("installed", 3, true));
    store.set_group(groups[3].clone(), vec![continuation("set", 4, false)]);
    assert!(store.receive_index().is_none());
    assert!(store.rebuild_receive_index());
    assert_eq!(index_of(&store).keys(), expected_keys([&groups[1], &groups[2], &groups[3]]));
    assert_index_matches_support(&store, &groups);
}

#[test]
fn alias_and_duplicate_puts_leave_the_occurrence_keys_unchanged() {
    let groups = universe();
    let group = &groups[6];
    let mut store = TestStore::new();
    assert!(store.rebuild_receive_index());
    assert!(put(&mut store, group, continuation("same", 1, false)));
    let resident = index_of(&store).keys();
    assert_eq!(resident, expected_keys([group]));
    assert!(!put(&mut store, group, continuation("same", 9, false)));
    assert_eq!(index_of(&store).keys(), resident);
    assert!(!put(&mut store, group, continuation("same", 1, false)));
    assert_eq!(index_of(&store).keys(), resident);
    assert!(put(&mut store, group, continuation("other", 2, false)));
    assert_eq!(index_of(&store).keys(), resident);
    assert_eq!(store.resting_len(group), Some(2));
    assert_index_matches_support(&store, &groups);
}

#[test]
fn canonical_plans_snapshots_and_journals_are_identical_with_the_index_on_and_off() {
    let groups = universe();
    let mut indexed = TestStore::new();
    assert!(indexed.rebuild_receive_index());
    let mut plain = TestStore::new();
    mutate_scripted(&mut indexed, &groups);
    mutate_scripted(&mut plain, &groups);
    assert!(plain.receive_index().is_none());
    assert_index_matches_support(&indexed, &groups);
    assert_eq!(indexed.loaded_records(), plain.loaded_records());
    assert_eq!(indexed.installed_records(), plain.installed_records());
    assert_eq!(canonical_groups(&indexed), canonical_groups(&plain));
    assert!(indexed.pending_delta() == plain.pending_delta());
    assert_eq!(content_journal(&indexed), content_journal(&plain));
    assert_eq!(source_journal(&indexed), source_journal(&plain));
    assert_eq!(indexed.loaded_group_count(), plain.loaded_group_count());
    assert_eq!(indexed.resting_record_count(), plain.resting_record_count());
    assert_eq!(indexed.installed_count(), plain.installed_count());

    let snapshot = indexed.clone();
    assert!(index_of(&snapshot).same_keys(index_of(&indexed)));
    let frozen = index_of(&snapshot).keys();
    assert!(put(&mut indexed, &groups[4], continuation("g", 8, false)));
    assert_eq!(index_of(&snapshot).keys(), frozen);
    assert!(!index_of(&snapshot).same_keys(index_of(&indexed)));
    assert_index_matches_support(&snapshot, &groups);
    assert_index_matches_support(&indexed, &groups);
}

#[test]
fn loads_count_history_loads_and_clears_only() {
    let groups = universe();
    let mut store = TestStore::new();
    assert_eq!(store.loads(), 0);
    assert!(put(&mut store, &groups[0], continuation("a", 1, false)));
    store.install(&groups[1], continuation("b", 2, true));
    assert!(remove_slot(&mut store, &groups[0], 0));
    assert!(store.rebuild_receive_index());
    assert_eq!(store.loads(), 0);
    store.set_group(groups[2].clone(), vec![continuation("c", 3, false)]);
    assert_eq!(store.loads(), 1);
    store.set_group(groups[3].clone(), Vec::new());
    assert_eq!(store.loads(), 2);
    store.clear();
    assert_eq!(store.loads(), 3);
    let snapshot = store.clone();
    store.set_group(groups[4].clone(), Vec::new());
    assert_eq!(snapshot.loads(), 3);
    assert_eq!(store.loads(), 4);
}

#[test]
fn published_snapshot_builds_are_maintained_like_rebuilt_indexes() {
    let groups = universe();
    let mut store = TestStore::new();
    mutate_scripted(&mut store, &groups);
    let detached = store.clone();
    let built = detached
        .build_receive_index()
        .expect("every stored continuation group prefix parses");
    assert!(detached.receive_index().is_none());
    store.publish_receive_index(built);
    assert_index_matches_support(&store, &groups);
    let mut rebuilt = TestStore::new();
    mutate_scripted(&mut rebuilt, &groups);
    assert!(rebuilt.rebuild_receive_index());
    assert!(index_of(&store).same_keys(index_of(&rebuilt)));
    assert!(put(&mut store, &groups[4], continuation("h", 9, false)));
    assert!(remove_slot(&mut store, &groups[0], 0));
    assert_index_matches_support(&store, &groups);
}

#[test]
fn prefix_accessors_read_the_authoritative_tries_by_group_prefix() {
    let groups = universe();
    let mut store = TestStore::new();
    mutate_scripted(&mut store, &groups);
    assert!(put(&mut store, &groups[0], continuation("z", 9, false)));
    assert_eq!(store.resting_len(&groups[0]), Some(2));
    for group in &groups {
        let prefix = RhoPathV1::continuation_prefix(group);
        assert_eq!(store.is_loaded_prefix(&prefix), store.is_loaded(group));
        assert_eq!(
            store
                .loaded_group_channels(&prefix)
                .map(|channels| channels.to_vec()),
            store.is_loaded(group).then(|| group.clone())
        );
        assert_eq!(
            store
                .installed_entry(&prefix)
                .map(|entry| (entry.0.to_vec(), entry.1.clone())),
            store
                .installed(group)
                .map(|continuation| (group.clone(), continuation))
        );
        let ordered = store.ordered_resting_records(&prefix);
        assert_eq!(
            ordered
                .iter()
                .map(|record| record.value().clone())
                .collect::<Vec<_>>(),
            store.resting_values(group).unwrap_or_default()
        );
        assert_eq!(
            ordered
                .iter()
                .map(|record| Arc::clone(record.path()))
                .collect::<Vec<_>>(),
            store
                .resting_records(group)
                .unwrap_or_default()
                .iter()
                .map(|record| Arc::clone(record.path()))
                .collect::<Vec<_>>()
        );
        for record in &ordered {
            let identity_path = record
                .identity_path()
                .expect("a continuation record carries its identity path");
            let identity = encode_continuation_identity(record.value());
            assert_eq!(
                &identity_path[..],
                RhoPathV1::collection_record_path(&prefix, &identity).as_slice()
            );
            let found = store
                .resting_identity_record(&prefix, identity_path)
                .expect("an identity path resolves inside its own group");
            assert!(Arc::ptr_eq(&found, record));
            for other in groups.iter().filter(|other| *other != group) {
                let other_prefix = RhoPathV1::continuation_prefix(other);
                assert!(
                    store
                        .resting_identity_record(&other_prefix, identity_path)
                        .is_none()
                );
            }
        }
        let absent = RhoPathV1::collection_record_path(&prefix, b"absent identity");
        assert!(store.resting_identity_record(&prefix, &absent).is_none());
    }
}

#[derive(Clone, Debug)]
enum IndexOperation {
    SetGroup { group: usize, names: Vec<u8> },
    Put { group: usize, name: u8, source: u8 },
    Remove { group: usize, slot: usize },
    Install { group: usize, name: u8 },
    Clear { rebuild: bool },
    Rebuild,
    Publish,
}

fn index_operation() -> impl Strategy<Value = IndexOperation> {
    let group = 0..GROUPS.len();
    prop_oneof![
        2 => (group.clone(), prop::collection::vec(0_u8..4, 0..3))
            .prop_map(|(group, names)| IndexOperation::SetGroup { group, names }),
        5 => (group.clone(), 0_u8..4, 0_u8..2)
            .prop_map(|(group, name, source)| IndexOperation::Put { group, name, source }),
        4 => (group.clone(), 0_usize..3)
            .prop_map(|(group, slot)| IndexOperation::Remove { group, slot }),
        2 => (group, 0_u8..2).prop_map(|(group, name)| IndexOperation::Install { group, name }),
        1 => any::<bool>().prop_map(|rebuild| IndexOperation::Clear { rebuild }),
        1 => Just(IndexOperation::Rebuild),
        1 => Just(IndexOperation::Publish),
    ]
}

fn apply_index_operation(
    store: &mut TestStore,
    groups: &[Vec<RawChannel>],
    operation: &IndexOperation,
    indexed: &mut bool,
) -> u64 {
    match operation {
        IndexOperation::SetGroup { group, names } => {
            let channels = &groups[*group];
            if store.is_loaded(channels) {
                return 0;
            }
            let mut continuations = Vec::with_capacity(names.len());
            continuations.extend(
                names
                    .iter()
                    .map(|name| continuation(&format!("n{name}"), *name, false)),
            );
            store.set_group(channels.clone(), continuations);
            1
        }
        IndexOperation::Put {
            group,
            name,
            source,
        } => {
            put(store, &groups[*group], continuation(&format!("n{name}"), *source, false));
            0
        }
        IndexOperation::Remove { group, slot } => {
            remove_slot(store, &groups[*group], *slot);
            0
        }
        IndexOperation::Install { group, name } => {
            store.install(&groups[*group], continuation(&format!("i{name}"), *name, true));
            0
        }
        IndexOperation::Clear { rebuild } => {
            store.clear();
            *indexed = false;
            if *rebuild {
                assert!(store.rebuild_receive_index());
                *indexed = true;
            }
            1
        }
        IndexOperation::Rebuild => {
            assert!(store.rebuild_receive_index());
            *indexed = true;
            0
        }
        IndexOperation::Publish => {
            let built = store
                .clone()
                .build_receive_index()
                .expect("every stored continuation group prefix parses");
            store.publish_receive_index(built);
            *indexed = true;
            0
        }
    }
}

proptest! {
    #[test]
    fn maintained_index_equals_a_fresh_build_across_generated_mutator_sequences(
        operations in prop::collection::vec(index_operation(), 1..40),
    ) {
        let groups = universe();
        let probes = probe_channels();
        let mut store = TestStore::new();
        prop_assert!(store.rebuild_receive_index());
        let mut indexed = true;
        let mut expected_loads = 0_u64;
        for operation in &operations {
            let loaded = apply_index_operation(&mut store, &groups, operation, &mut indexed);
            expected_loads = expected_loads
                .checked_add(loaded)
                .expect("the generated load count fits u64");
            prop_assert_eq!(store.loads(), expected_loads);
            prop_assert_eq!(store.receive_index().is_some(), indexed);
            if let Some(index) = store.receive_index() {
                let supported = supported_groups(&store, &groups);
                prop_assert_eq!(index.keys(), expected_keys(supported.iter().copied()));
                let rebuilt = store
                    .build_receive_index()
                    .expect("every stored continuation group prefix parses");
                prop_assert!(index.same_keys(&rebuilt));
                for channel in &probes {
                    prop_assert_eq!(
                        visited_groups(index, channel),
                        expected_groups_of(channel, &supported)
                    );
                }
            }
        }
    }
}
