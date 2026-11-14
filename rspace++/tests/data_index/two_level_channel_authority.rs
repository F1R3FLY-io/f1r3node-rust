//! Representation-level tests for the exact-channel, two-level RSpace data
//! authority.
//!
//! This module is included as a private child of `rspace::data_index` so it can
//! observe test-only guarded-root identities without exposing ownership
//! internals in the production API.  The outer trie owns one exact endpoint
//! per loaded channel; each real endpoint owns an independently guarded,
//! channel-relative `Bag`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;

use proptest::collection::vec;
use proptest::prelude::*;

use super::*;

/// A test channel whose semantic identity is exactly the supplied byte string.
/// This lets the generated cases exercise genuine identity-prefix relations,
/// rather than only prefix relations between higher-level channel values.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct ExactByteChannel(Vec<u8>);

impl crate::rspace::hashing::stable_hash_provider::StableHashSerialize for ExactByteChannel {
    fn stable_hash_bytes(&self) -> Vec<u8> { self.0.clone() }

    fn stable_hash_encode_into(&self, output: &mut Vec<u8>) { output.extend_from_slice(&self.0); }
}

impl RhoPathEncode for ExactByteChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.0.clone() }
}

fn datum(label: u8) -> Datum<String> {
    Datum {
        a: Arc::new(format!("value-{label}")),
        persist: label % 2 != 0,
        source: crate::rspace::trace::event::Produce::new(
            Blake2b256Hash::new(&[label]),
            Blake2b256Hash::new(&[label, 0xA5]),
            label % 2 != 0,
        ),
    }
}

fn marker<C: RhoPathEncode>(channel: &C) -> Vec<u8> { RhoPathV1::datum_prefix(channel) }

fn inner_anchor<C, A: Clone>(store: &RSpaceDataStore<C, A>, marker: &[u8]) -> Option<usize> {
    store.index().channel_bag_anchor_identity(marker)
}

#[test]
fn exact_outer_endpoint_distinguishes_unloaded_from_loaded_empty() {
    let loaded = "loaded-empty".to_owned();
    let unloaded = "unloaded".to_owned();
    let loaded_marker = marker(&loaded);
    let unloaded_marker = marker(&unloaded);
    let mut store = RSpaceDataStore::<String, String>::new();

    assert_eq!(store.channel_data(&loaded).unwrap(), None);
    assert_eq!(inner_anchor(&store, &loaded_marker), None);

    store.set_channel(loaded.clone(), []).unwrap();

    assert!(store.is_loaded(&loaded));
    assert!(!store.is_loaded(&unloaded));
    assert_eq!(store.channel_data(&loaded).unwrap(), Some(Vec::new()));
    assert_eq!(store.channel_data(&unloaded).unwrap(), None);
    assert!(inner_anchor(&store, &loaded_marker).is_some());
    assert_eq!(inner_anchor(&store, &unloaded_marker), None);
    assert_eq!(store.loaded_channel_count(), 1);
    assert_eq!((store.distinct_len(), store.cardinality()), (0, 0));
}

#[test]
fn prefix_related_channel_identities_are_exactly_independent() {
    let channels = ["a".to_owned(), "ab".to_owned(), "a\0b".to_owned()];
    let values = [datum(1), datum(2), datum(3)];
    let mut store = RSpaceDataStore::new();

    for (channel, value) in channels.iter().zip(&values) {
        store.set_channel(channel.clone(), [value.clone()]).unwrap();
    }
    let before = channels
        .iter()
        .map(|channel| inner_anchor(&store, &marker(channel)).unwrap())
        .collect::<Vec<_>>();

    store.set_channel(channels[0].clone(), []).unwrap();

    assert_eq!(store.channel_data(&channels[0]).unwrap(), Some(Vec::new()));
    assert_eq!(store.channel_data(&channels[1]).unwrap(), Some(vec![values[1].clone()]));
    assert_eq!(store.channel_data(&channels[2]).unwrap(), Some(vec![values[2].clone()]));
    assert_ne!(inner_anchor(&store, &marker(&channels[0])).unwrap(), before[0]);
    assert_eq!(inner_anchor(&store, &marker(&channels[1])).unwrap(), before[1]);
    assert_eq!(inner_anchor(&store, &marker(&channels[2])).unwrap(), before[2]);
    assert_eq!(store.loaded_channel_count(), channels.len());
}

#[test]
fn replacing_one_channel_preserves_the_sibling_inner_anchor() {
    let target = "shared\0target".to_owned();
    let sibling = "shared\0sibling".to_owned();
    let target_marker = marker(&target);
    let sibling_marker = marker(&sibling);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(target.clone(), [datum(1), datum(1)])
        .unwrap();
    store.set_channel(sibling.clone(), [datum(9)]).unwrap();
    let old_target = inner_anchor(&store, &target_marker).unwrap();
    let old_sibling = inner_anchor(&store, &sibling_marker).unwrap();

    store.set_channel(target.clone(), [datum(2)]).unwrap();

    assert_ne!(inner_anchor(&store, &target_marker).unwrap(), old_target);
    assert_eq!(inner_anchor(&store, &sibling_marker).unwrap(), old_sibling);
    assert_eq!(store.channel_data(&sibling).unwrap(), Some(vec![datum(9)]));
}

#[test]
fn candidate_availability_shares_the_channel_bag_anchor() {
    let channel = "catalog-channel".to_owned();
    let channel_marker = marker(&channel);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [datum(1), datum(1), datum(2)])
        .unwrap();
    let channel_anchor = inner_anchor(&store, &channel_marker).unwrap();

    let pool = store.channel_pool(&channel).unwrap().unwrap();

    assert_eq!(pool.stored_availability().anchor_identity(), channel_anchor);
    assert_eq!(pool.stored_availability().distinct_len(), 2);
    assert_eq!(pool.stored_availability().cardinality(), 3);
}

#[test]
fn empty_catalog_is_distinct_and_unrelated_channels_do_not_supply_candidates() {
    let empty = "catalog-empty".to_owned();
    let populated = "catalog-populated".to_owned();
    let value = datum(12);
    let mut store = RSpaceDataStore::new();
    store.set_channel(empty.clone(), []).unwrap();
    store
        .set_channel(populated.clone(), [value.clone()])
        .unwrap();

    let empty_pool = store
        .channel_pool(&empty)
        .unwrap()
        .expect("a loaded empty channel has an explicit candidate catalog");
    assert!(empty_pool.is_empty());
    assert_eq!(empty_pool.stored_availability().distinct_len(), 0);
    assert_eq!(empty_pool.stored_availability().cardinality(), 0);
    assert!(empty_pool.candidate(0).is_none());

    let populated_pool = store
        .channel_pool(&populated)
        .unwrap()
        .expect("the populated channel has a candidate catalog");
    assert_eq!(
        populated_pool
            .stored_availability()
            .count(&RSpaceDataIndex::relative_datum_path(&value)),
        1
    );
    assert_eq!(populated_pool.candidate(0).map(|(datum, _)| datum), Some(&value));
    assert_eq!(store.loaded_channel_count(), 2);
}

#[test]
fn incremental_catalog_updates_retain_the_authoritative_inner_anchor() {
    let channel = "incremental-catalog".to_owned();
    let channel_marker = marker(&channel);
    let value = datum(3);
    let relative = RSpaceDataIndex::relative_datum_path(&value);
    let absolute = RSpaceDataIndex::datum_path(&channel, &value);
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), [value.clone()]).unwrap();
    let _ = store.channel_pool(&channel).unwrap().unwrap();

    store.insert(&channel, &value).unwrap();
    let after_insert = store.channel_pool(&channel).unwrap().unwrap();
    assert_eq!(after_insert.stored_availability().count(&relative), 2);
    assert_eq!(
        after_insert.stored_availability().anchor_identity(),
        inner_anchor(&store, &channel_marker).unwrap(),
    );

    assert!(store.remove_path(&absolute));
    let after_remove = store.channel_pool(&channel).unwrap().unwrap();
    assert_eq!(after_remove.stored_availability().count(&relative), 1);
    assert_eq!(
        after_remove.stored_availability().anchor_identity(),
        inner_anchor(&store, &channel_marker).unwrap(),
    );
}

#[test]
fn cloned_store_splits_only_the_mutated_outer_and_inner_authorities() {
    let target = "target".to_owned();
    let sibling = "sibling".to_owned();
    let target_marker = marker(&target);
    let sibling_marker = marker(&sibling);
    let mut left = RSpaceDataStore::new();
    left.set_channel(target.clone(), [datum(1)]).unwrap();
    left.set_channel(sibling.clone(), [datum(9)]).unwrap();
    let right = left.clone();
    let shared_outer = left.index().outer_anchor_identity();
    let shared_target = inner_anchor(&left, &target_marker).unwrap();
    let shared_sibling = inner_anchor(&left, &sibling_marker).unwrap();
    assert_eq!(right.index().outer_anchor_identity(), shared_outer);
    assert_eq!(inner_anchor(&right, &target_marker), Some(shared_target));
    assert_eq!(inner_anchor(&right, &sibling_marker), Some(shared_sibling));

    left.insert(&target, &datum(2)).unwrap();

    assert_ne!(left.index().outer_anchor_identity(), right.index().outer_anchor_identity());
    assert_ne!(inner_anchor(&left, &target_marker), Some(shared_target));
    assert_eq!(inner_anchor(&right, &target_marker), Some(shared_target));
    assert_eq!(inner_anchor(&left, &sibling_marker), Some(shared_sibling));
    assert_eq!(inner_anchor(&right, &sibling_marker), Some(shared_sibling));
    assert_eq!(right.channel_data(&target).unwrap(), Some(vec![datum(1)]));
    assert_eq!(left.channel_data(&target).unwrap().unwrap().len(), 2);
}

#[test]
fn cloned_store_delta_splits_only_the_mutated_outer_and_inner_authorities() {
    let target = "target".to_owned();
    let sibling = "sibling".to_owned();
    let target_marker = marker(&target);
    let sibling_marker = marker(&sibling);
    let addition = RSpaceDataIndex::datum_path(&target, &datum(2));
    for drop_mutated_first in [false, true] {
        let mut left = RSpaceDataStore::new();
        left.set_channel(target.clone(), [datum(1)])
            .expect("the target channel loads");
        left.set_channel(sibling.clone(), [datum(9)])
            .expect("the sibling channel loads");
        let right = left.clone();
        let shared_outer = left.index().outer_anchor_identity();
        let shared_target =
            inner_anchor(&left, &target_marker).expect("the target channel is loaded");
        let shared_sibling =
            inner_anchor(&left, &sibling_marker).expect("the sibling channel is loaded");
        assert_eq!(right.index().outer_anchor_identity(), shared_outer);
        assert_eq!(inner_anchor(&right, &target_marker), Some(shared_target));
        assert_eq!(inner_anchor(&right, &sibling_marker), Some(shared_sibling));
        let mut delta = BagDelta::new();
        delta
            .record_addition(&addition, Multiplicity::ONE)
            .expect("the delta records one addition");

        left.apply_delta(&delta)
            .expect("the delta applies to the mutated version");

        assert_ne!(left.index().outer_anchor_identity(), shared_outer);
        assert_eq!(right.index().outer_anchor_identity(), shared_outer);
        assert_ne!(inner_anchor(&left, &target_marker), Some(shared_target));
        assert_eq!(inner_anchor(&right, &target_marker), Some(shared_target));
        assert_eq!(inner_anchor(&left, &sibling_marker), Some(shared_sibling));
        assert_eq!(inner_anchor(&right, &sibling_marker), Some(shared_sibling));
        let (survivor, survivor_keeps_shared_outer, target_counts) = match drop_mutated_first {
            true => {
                drop(left);
                (right, true, (1, 0))
            }
            false => {
                drop(right);
                (left, false, (1, 1))
            }
        };
        assert_eq!(
            survivor.index().outer_anchor_identity() == shared_outer,
            survivor_keeps_shared_outer
        );
        assert_eq!(
            (
                survivor.index().count(&target, &datum(1)),
                survivor.index().count(&target, &datum(2)),
            ),
            target_counts
        );
        assert_eq!(
            survivor
                .channel_data(&sibling)
                .expect("the sibling channel decodes"),
            Some(vec![datum(9)])
        );
        assert_eq!(inner_anchor(&survivor, &sibling_marker), Some(shared_sibling));
        assert_eq!(survivor.loaded_channel_count(), 2);
    }
}

#[test]
fn removing_the_last_datum_preserves_loaded_empty_authority() {
    let channel = "last-datum".to_owned();
    let value = datum(4);
    let path = RSpaceDataIndex::datum_path(&channel, &value);
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), [value]).unwrap();

    assert!(store.remove_path(&path));

    assert!(store.is_loaded(&channel));
    assert_eq!(store.channel_data(&channel).unwrap(), Some(Vec::new()));
    assert!(inner_anchor(&store, &marker(&channel)).is_some());
    assert_eq!(store.loaded_channel_count(), 1);
    assert_eq!((store.distinct_len(), store.cardinality()), (0, 0));
}

#[test]
fn absolute_count_and_remove_split_at_the_exact_channel_boundary() {
    let left = "a".to_owned();
    let right = "ab".to_owned();
    let shared = datum(7);
    let left_path = RSpaceDataIndex::datum_path(&left, &shared);
    let right_path = RSpaceDataIndex::datum_path(&right, &shared);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(left.clone(), [shared.clone(), shared.clone()])
        .unwrap();
    store.set_channel(right.clone(), [shared.clone()]).unwrap();

    assert_eq!(store.index().count_path(&left_path), 2);
    assert_eq!(store.index().count_path(&right_path), 1);
    assert!(store.remove_path(&left_path));
    assert_eq!(store.index().count_path(&left_path), 1);
    assert_eq!(store.index().count_path(&right_path), 1);
    assert_eq!(store.channel_data(&right).unwrap(), Some(vec![shared]));
}

#[test]
fn multichannel_delta_publishes_all_touched_channels_and_preserves_untouched_anchor() {
    let left = "delta-a".to_owned();
    let right = "delta-b".to_owned();
    let untouched = "delta-c".to_owned();
    let added = "delta-new".to_owned();
    let mut store = RSpaceDataStore::new();
    store.set_channel(left.clone(), [datum(1)]).unwrap();
    store.set_channel(right.clone(), [datum(2)]).unwrap();
    store.set_channel(untouched.clone(), [datum(9)]).unwrap();
    let _ = store.channel_pool(&right).unwrap().unwrap();
    let _ = store.channel_pool(&untouched).unwrap().unwrap();
    let untouched_catalog = Arc::clone(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&marker(&untouched))
            .unwrap(),
    );
    let old_outer = store.index().outer_anchor_identity();
    let untouched_anchor = inner_anchor(&store, &marker(&untouched)).unwrap();
    let mut delta = BagDelta::new();
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&left, &datum(1)), Multiplicity::ONE)
        .unwrap();
    delta
        .record_addition(
            &RSpaceDataIndex::datum_path(&right, &datum(3)),
            Multiplicity::new(2).unwrap(),
        )
        .unwrap();
    delta
        .record_addition(&RSpaceDataIndex::datum_path(&added, &datum(4)), Multiplicity::ONE)
        .unwrap();

    store.apply_delta(&delta).unwrap();

    assert_eq!(store.index().outer_anchor_identity(), old_outer);
    assert_eq!(store.channel_data(&left).unwrap(), Some(Vec::new()));
    assert_eq!(store.index().count(&right, &datum(2)), 1);
    assert_eq!(store.index().count(&right, &datum(3)), 2);
    assert_eq!(store.index().count(&added, &datum(4)), 1);
    assert_eq!(inner_anchor(&store, &marker(&untouched)), Some(untouched_anchor));
    assert!(Arc::ptr_eq(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&marker(&untouched))
            .unwrap(),
        &untouched_catalog,
    ));
    let right_pool = store.channel_pool(&right).unwrap().unwrap();
    assert_eq!(
        right_pool.stored_availability().anchor_identity(),
        inner_anchor(&store, &marker(&right)).unwrap(),
    );
    assert_eq!(store.loaded_channel_count(), 4);
    assert_eq!((store.distinct_len(), store.cardinality()), (4, 5));
}

#[test]
fn multichannel_delta_failure_preserves_every_authority_and_catalog() {
    let first = "a-first".to_owned();
    let sibling = "m-sibling".to_owned();
    let missing = "z-missing".to_owned();
    let mut store = RSpaceDataStore::new();
    store.set_channel(first.clone(), [datum(1)]).unwrap();
    store.set_channel(sibling.clone(), [datum(2)]).unwrap();
    let first_pool = store.channel_pool(&first).unwrap().unwrap();
    let sibling_pool = store.channel_pool(&sibling).unwrap().unwrap();
    let first_marker = marker(&first);
    let sibling_marker = marker(&sibling);
    let first_catalog = Arc::clone(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&first_marker)
            .unwrap(),
    );
    let sibling_catalog = Arc::clone(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&sibling_marker)
            .unwrap(),
    );
    let before = store.clone();
    let before_outer = store.index().outer_anchor_identity();
    let before_first = inner_anchor(&store, &first_marker);
    let before_sibling = inner_anchor(&store, &sibling_marker);
    let mut delta = BagDelta::new();
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&first, &datum(1)), Multiplicity::ONE)
        .unwrap();
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&missing, &datum(8)), Multiplicity::ONE)
        .unwrap();

    assert!(store.apply_delta(&delta).is_err());

    assert_eq!(store, before);
    assert_eq!(store.index().outer_anchor_identity(), before_outer);
    assert_eq!(inner_anchor(&store, &first_marker), before_first);
    assert_eq!(inner_anchor(&store, &sibling_marker), before_sibling);
    let catalogs = store.candidate_catalogs.read().unwrap();
    assert!(Arc::ptr_eq(catalogs.get(&first_marker).unwrap(), &first_catalog));
    assert!(Arc::ptr_eq(catalogs.get(&sibling_marker).unwrap(), &sibling_catalog));
    assert_eq!(first_pool.candidate(0).unwrap().0, &datum(1));
    assert_eq!(sibling_pool.candidate(0).unwrap().0, &datum(2));
}

#[test]
fn empty_delta_preserves_outer_and_inner_identities() {
    let channel = "empty-delta".to_owned();
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), [datum(1)]).unwrap();
    let outer = store.index().outer_anchor_identity();
    let inner = inner_anchor(&store, &marker(&channel));

    store.apply_delta(&BagDelta::new()).unwrap();

    assert_eq!(store.index().outer_anchor_identity(), outer);
    assert_eq!(inner_anchor(&store, &marker(&channel)), inner);
}

#[test]
fn equality_observes_loaded_empty_and_exact_channel_contents() {
    let channel = "equality".to_owned();
    let unloaded = RSpaceDataStore::<String, String>::new();
    let mut left = RSpaceDataStore::<String, String>::new();
    let mut right = RSpaceDataStore::<String, String>::new();
    left.set_channel(channel.clone(), []).unwrap();
    right.set_channel(channel.clone(), []).unwrap();

    assert_ne!(left, unloaded);
    assert_eq!(left, right);

    right.insert(&channel, &datum(1)).unwrap();
    assert_ne!(left, right);
    left.insert(&channel, &datum(1)).unwrap();
    assert_eq!(left, right);
}

#[test]
// TLRS-PROP-02: aggregate counters refine the exact sum of independently
// guarded channel Bags, including the public u64 saturation boundary.
fn aggregate_metrics_preserve_loaded_distinct_exact_and_saturated_domains() {
    let maximum = Multiplicity::new(u64::MAX).unwrap();
    let first = b"aggregate/a".to_vec();
    let second = b"aggregate/ab".to_vec();
    let loaded_empty = b"aggregate/neighbor".to_vec();
    let first_value = datum(1);
    let second_value = datum(2);
    let first_path = RSpaceDataIndex::datum_path(&first, &first_value);
    let second_path = RSpaceDataIndex::datum_path(&second, &second_value);
    let mut store = RSpaceDataStore::<Vec<u8>, String>::new();
    store.set_channel(loaded_empty.clone(), []).unwrap();
    let mut delta = BagDelta::new();
    delta.record_addition(&first_path, maximum).unwrap();
    delta
        .record_addition(&second_path, Multiplicity::ONE)
        .unwrap();

    store.apply_delta(&delta).unwrap();

    assert_eq!(store.loaded_channel_count(), 3);
    assert_eq!(store.distinct_len(), 2);
    assert_eq!(store.index().exact_cardinality, u128::from(u64::MAX) + 1);
    assert_eq!(store.cardinality(), u64::MAX);
    assert!(store.index().exact_cardinality > u128::from(u64::MAX));
    assert_eq!(store.index().count_path(&first_path), u64::MAX);
    assert_eq!(store.index().count_path(&second_path), 1);
    assert_eq!(store.channel_data(&loaded_empty).unwrap(), Some(Vec::new()));

    let mut remove_one = BagDelta::new();
    remove_one
        .record_removal(&second_path, Multiplicity::ONE)
        .unwrap();
    store.apply_delta(&remove_one).unwrap();

    assert_eq!(store.loaded_channel_count(), 3);
    assert_eq!(store.distinct_len(), 1);
    assert_eq!(store.index().exact_cardinality, u128::from(u64::MAX));
    assert_eq!(store.cardinality(), u64::MAX);
    assert!(!(store.index().exact_cardinality > u128::from(u64::MAX)));
}

#[test]
// TLRS-PROP-04: only a successful nonempty delta publishes a new authority.
fn delta_publication_identity_distinguishes_success_empty_refusal_and_underflow() {
    let target = b"publication/target".to_vec();
    let sibling = b"publication/sibling".to_vec();
    let target_value = datum(1);
    let sibling_value = datum(9);
    let added_value = datum(2);
    let target_marker = marker(&target);
    let sibling_marker = marker(&sibling);
    let mut store = RSpaceDataStore::<Vec<u8>, String>::new();
    store
        .set_channel(target.clone(), [target_value.clone()])
        .unwrap();
    store
        .set_channel(sibling.clone(), [sibling_value.clone()])
        .unwrap();
    let before_success_outer = store.index().outer_anchor_identity();
    let before_success_target = inner_anchor(&store, &target_marker).unwrap();
    let before_success_sibling = inner_anchor(&store, &sibling_marker).unwrap();
    let mut success = BagDelta::new();
    success
        .record_addition(&RSpaceDataIndex::datum_path(&target, &added_value), Multiplicity::ONE)
        .unwrap();

    store.apply_delta(&success).unwrap();

    assert_eq!(store.index().outer_anchor_identity(), before_success_outer);
    assert_ne!(inner_anchor(&store, &target_marker), Some(before_success_target),);
    assert_eq!(inner_anchor(&store, &sibling_marker), Some(before_success_sibling),);

    let alias = store.clone();
    let aliased_value = datum(5);
    let before_aliased_outer = store.index().outer_anchor_identity();
    let before_aliased_target =
        inner_anchor(&store, &target_marker).expect("the target channel is loaded");
    let mut aliased = BagDelta::new();
    aliased
        .record_addition(&RSpaceDataIndex::datum_path(&target, &aliased_value), Multiplicity::ONE)
        .expect("the aliased delta records one addition");
    store
        .apply_delta(&aliased)
        .expect("a delta applies while an alias holds the authority");
    assert_ne!(store.index().outer_anchor_identity(), before_aliased_outer);
    assert_eq!(alias.index().outer_anchor_identity(), before_aliased_outer);
    assert_ne!(inner_anchor(&store, &target_marker), Some(before_aliased_target));
    assert_eq!(inner_anchor(&alias, &target_marker), Some(before_aliased_target));
    assert_eq!(alias.index().count(&target, &aliased_value), 0);
    assert_eq!(store.index().count(&target, &aliased_value), 1);
    drop(alias);

    let before_empty_outer = store.index().outer_anchor_identity();
    let before_empty_target = inner_anchor(&store, &target_marker);
    store.apply_delta(&BagDelta::new()).unwrap();
    assert_eq!(store.index().outer_anchor_identity(), before_empty_outer);
    assert_eq!(inner_anchor(&store, &target_marker), before_empty_target);

    let mut cancelled = BagDelta::new();
    let cancelled_path = RSpaceDataIndex::datum_path(&target, &datum(3));
    cancelled
        .record_addition(&cancelled_path, Multiplicity::ONE)
        .unwrap();
    cancelled
        .record_removal(&cancelled_path, Multiplicity::ONE)
        .unwrap();
    assert!(cancelled.is_empty());
    store.apply_delta(&cancelled).unwrap();
    assert_eq!(store.index().outer_anchor_identity(), before_empty_outer);
    assert_eq!(inner_anchor(&store, &target_marker), before_empty_target);

    let mut refused = BagDelta::new();
    refused
        .record_addition(b"not-a-rhopath", Multiplicity::ONE)
        .unwrap();
    assert!(store.apply_delta(&refused).is_err());
    assert_eq!(store.index().outer_anchor_identity(), before_empty_outer);
    assert_eq!(inner_anchor(&store, &target_marker), before_empty_target);

    let mut underflow = BagDelta::new();
    underflow
        .record_removal(&RSpaceDataIndex::datum_path(&target, &datum(255)), Multiplicity::ONE)
        .unwrap();
    assert!(store.apply_delta(&underflow).is_err());
    assert_eq!(store.index().outer_anchor_identity(), before_empty_outer);
    assert_eq!(inner_anchor(&store, &target_marker), before_empty_target);
    assert_eq!(store.index().count(&target, &target_value), 1);
    assert_eq!(store.index().count(&target, &added_value), 1);
    assert_eq!(store.index().count(&sibling, &sibling_value), 1);
}

#[test]
fn deep_outer_and_inner_tries_clone_and_drop_on_a_small_stack() {
    const DEPTH: usize = 4_096;
    let channel = format!("channel-{}", "c".repeat(DEPTH));
    let value = Datum {
        a: Arc::new(format!("value-{}", "v".repeat(DEPTH))),
        persist: false,
        source: crate::rspace::trace::event::Produce::new(
            Blake2b256Hash::new(b"deep-produce"),
            Blake2b256Hash::new(b"deep-produce-sequence"),
            false,
        ),
    };
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel, [value]).unwrap();

    thread::Builder::new()
        .name("two-level-small-stack-drop".to_owned())
        .stack_size(96 * 1_024)
        .spawn(move || {
            let first = store.clone();
            let second = first.clone();
            drop(first);
            drop(store);
            drop(second);
        })
        .unwrap()
        .join()
        .expect("two guarded trie levels must not recurse with path depth");
}

#[derive(Clone, Debug)]
enum ModelAction {
    Set { channel: u8, labels: Vec<u8> },
    Insert { channel: u8, label: u8 },
    Remove { channel: u8, label: u8 },
    Clear,
}

fn action_strategy() -> impl Strategy<Value = ModelAction> {
    prop_oneof![
        (0u8..3, vec(0u8..8, 0..6))
            .prop_map(|(channel, labels)| ModelAction::Set { channel, labels }),
        (0u8..3, 0u8..8).prop_map(|(channel, label)| ModelAction::Insert { channel, label }),
        (0u8..3, 0u8..8).prop_map(|(channel, label)| ModelAction::Remove { channel, label }),
        Just(ModelAction::Clear),
    ]
}

fn model_channel(id: u8) -> String {
    match id {
        0 => "a".to_owned(),
        1 => "ab".to_owned(),
        _ => "a\0b".to_owned(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn operation_sequences_refine_an_exact_channel_multiset_oracle(
        actions in vec(action_strategy(), 0..64),
    ) {
        let mut store = RSpaceDataStore::<String, String>::new();
        let mut oracle = BTreeMap::<u8, BTreeMap<u8, u64>>::new();

        for action in actions {
            match action {
                ModelAction::Set { channel, labels } => {
                    store
                        .set_channel(
                            model_channel(channel),
                            labels.iter().copied().map(datum),
                        )
                        .unwrap();
                    let mut counts = BTreeMap::new();
                    for label in labels {
                        *counts.entry(label).or_insert(0) += 1;
                    }
                    oracle.insert(channel, counts);
                }
                ModelAction::Insert { channel, label } => {
                    let channel_value = model_channel(channel);
                    store.insert(&channel_value, &datum(label)).unwrap();
                    *oracle
                        .entry(channel)
                        .or_default()
                        .entry(label)
                        .or_insert(0) += 1;
                }
                ModelAction::Remove { channel, label } => {
                    let channel_value = model_channel(channel);
                    let path = RSpaceDataIndex::datum_path(&channel_value, &datum(label));
                    let expected = oracle
                        .get(&channel)
                        .and_then(|counts| counts.get(&label))
                        .copied()
                        .unwrap_or(0) != 0;
                    prop_assert_eq!(store.remove_path(&path), expected);
                    if expected {
                        let counts = oracle.get_mut(&channel).unwrap();
                        let count = counts.get_mut(&label).unwrap();
                        *count -= 1;
                        if *count == 0 {
                            counts.remove(&label);
                        }
                    }
                }
                ModelAction::Clear => {
                    store.clear();
                    oracle.clear();
                }
            }

            prop_assert_eq!(store.loaded_channel_count(), oracle.len());
            let expected_distinct = oracle
                .values()
                .map(|counts| counts.len())
                .sum::<usize>();
            let expected_cardinality = oracle
                .values()
                .flat_map(|counts| counts.values())
                .copied()
                .sum::<u64>();
            prop_assert_eq!(store.distinct_len(), expected_distinct);
            prop_assert_eq!(store.cardinality(), expected_cardinality);
            let cloned = store.clone();
            prop_assert_eq!(&cloned, &store);

            for channel_id in 0u8..3 {
                let channel = model_channel(channel_id);
                let expected = oracle.get(&channel_id);
                prop_assert_eq!(store.is_loaded(&channel), expected.is_some());
                prop_assert_eq!(
                    inner_anchor(&store, &marker(&channel)).is_some(),
                    expected.is_some(),
                );
                let actual = store.channel_data(&channel).unwrap();
                prop_assert_eq!(actual.is_some(), expected.is_some());
                if let (Some(actual), Some(expected)) = (actual, expected) {
                    prop_assert_eq!(actual.len() as u64, expected.values().sum::<u64>());
                    for label in 0u8..8 {
                        let expected_count = expected.get(&label).copied().unwrap_or(0);
                        prop_assert_eq!(
                            store.index().count(&channel, &datum(label)),
                            expected_count,
                        );
                        prop_assert_eq!(
                            actual.iter().filter(|value| **value == datum(label)).count() as u64,
                            expected_count,
                        );
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    // TLRS-PROP-01: exact outer endpoints separate every byte relation and
    // distinguish absence from a resident empty inner Bag.
    fn generated_exact_channel_relations_keep_absent_and_loaded_empty_authorities_distinct(
        ancestor in vec(any::<u8>(), 1..16),
        suffix in vec(any::<u8>(), 1..8),
        extension in any::<u8>(),
        loaded_empty_index in 0usize..3,
        absent_offset in 1usize..3,
    ) {
        let mut descendant = ancestor.clone();
        descendant.push(extension);
        descendant.extend_from_slice(&suffix);
        let mut neighbor = ancestor.clone();
        *neighbor.last_mut().unwrap() ^= 0xFF;
        let channels = [
            ExactByteChannel(ancestor.clone()),
            ExactByteChannel(descendant.clone()),
            ExactByteChannel(neighbor.clone()),
        ];
        let absent_index = (loaded_empty_index + absent_offset) % channels.len();
        let populated_index = 3 - loaded_empty_index - absent_index;
        let equal_alias = channels[loaded_empty_index].clone();
        let mut store = RSpaceDataStore::<ExactByteChannel, String>::new();

        prop_assert!(descendant.starts_with(&ancestor));
        prop_assert_ne!(&descendant, &ancestor);
        prop_assert_ne!(&neighbor, &ancestor);
        prop_assert!(!neighbor.starts_with(&ancestor));
        prop_assert_eq!(
            equal_alias.rho_path_identity(),
            channels[loaded_empty_index].rho_path_identity(),
        );
        prop_assert!(channels[1]
            .rho_path_identity()
            .starts_with(&channels[0].rho_path_identity()));
        prop_assert!(!channels[2]
            .rho_path_identity()
            .starts_with(&channels[0].rho_path_identity()));
        prop_assert_eq!(marker(&equal_alias), marker(&channels[loaded_empty_index]));
        prop_assert_ne!(marker(&channels[0]), marker(&channels[1]));
        prop_assert_ne!(marker(&channels[0]), marker(&channels[2]));
        prop_assert_ne!(marker(&channels[1]), marker(&channels[2]));

        store
            .set_channel(channels[loaded_empty_index].clone(), [])
            .unwrap();
        store
            .set_channel(channels[populated_index].clone(), [datum(7)])
            .unwrap();

        prop_assert!(store.is_loaded(&equal_alias));
        prop_assert_eq!(store.channel_data(&equal_alias).unwrap(), Some(Vec::new()));
        prop_assert_eq!(
            store.channel_data(&channels[populated_index]).unwrap(),
            Some(vec![datum(7)]),
        );
        prop_assert!(!store.is_loaded(&channels[absent_index]));
        prop_assert_eq!(store.channel_data(&channels[absent_index]).unwrap(), None);
        prop_assert_eq!(store.loaded_channel_count(), 2);
        prop_assert_eq!((store.distinct_len(), store.cardinality()), (1, 1));
        let empty_anchor = inner_anchor(&store, &marker(&equal_alias)).unwrap();
        let populated_anchor =
            inner_anchor(&store, &marker(&channels[populated_index])).unwrap();

        store
            .set_channel(channels[absent_index].clone(), [])
            .unwrap();

        prop_assert_eq!(store.loaded_channel_count(), 3);
        prop_assert_eq!(
            inner_anchor(&store, &marker(&equal_alias)),
            Some(empty_anchor),
        );
        prop_assert_eq!(
            inner_anchor(&store, &marker(&channels[populated_index])),
            Some(populated_anchor),
        );
        prop_assert_eq!(
            store.channel_data(&channels[absent_index]).unwrap(),
            Some(Vec::new()),
        );
        prop_assert_eq!((store.distinct_len(), store.cardinality()), (1, 1));
    }

    #[test]
    // TLRS-PROP-03: canonical candidate order is independent of construction
    // order and falls through equal digests to collision-free identity bytes.
    fn canonical_order_combines_insertion_permutation_multiplicity_prefixes_and_hash_collision(
        stem in vec(any::<u8>(), 1..12),
        labels in vec(0u8..16, 0..24),
        extension in any::<u8>(),
    ) {
        let ancestor = stem;
        let mut descendant = ancestor.clone();
        descendant.push(extension);
        descendant.push(0xA5);
        let mut neighbor = ancestor.clone();
        *neighbor.last_mut().unwrap() ^= 0xFF;
        let channels = [
            ExactByteChannel(ancestor),
            ExactByteChannel(descendant),
            ExactByteChannel(neighbor),
        ];
        let mut primary = labels;
        primary.extend_from_slice(&[1, 1, 2, 3, 3, 3]);
        let channel_labels = [primary, vec![4, 5, 4, 6], Vec::new()];
        let mut left = RSpaceDataStore::<ExactByteChannel, String>::new();
        let mut right = RSpaceDataStore::<ExactByteChannel, String>::new();

        for (channel, values) in channels.iter().zip(&channel_labels) {
            left.set_channel(
                channel.clone(),
                values.iter().copied().map(datum),
            ).unwrap();
            right.set_channel(
                channel.clone(),
                values.iter().rev().copied().map(datum),
            ).unwrap();
        }

        prop_assert_eq!(left.loaded_channel_count(), channels.len());
        prop_assert_eq!(right.loaded_channel_count(), channels.len());
        prop_assert_eq!(left.distinct_len(), right.distinct_len());
        prop_assert_eq!(left.cardinality(), right.cardinality());
        let forced = crate::rspace::candidate_digest_test_hook::force(
            Blake2b256Hash::from_bytes(vec![0xC7; 32]),
        );
        for channel in &channels {
            let left_entries = left
                .index()
                .ordered_channel_entries::<_, String>(channel)
                .unwrap();
            let right_entries = right
                .index()
                .ordered_channel_entries::<_, String>(channel)
                .unwrap();
            prop_assert_eq!(&left_entries, &right_entries);
            let identities = left_entries
                .iter()
                .map(|entry| {
                    RhoPathV1::parse_datum(&entry.path)
                        .unwrap()
                        .datum_identity()
                        .to_vec()
                })
                .collect::<Vec<_>>();
            prop_assert!(identities.windows(2).all(|pair| pair[0] < pair[1]));
            let observed = left_entries
                .iter()
                .map(|entry| entry.multiplicity.get())
                .sum::<u64>();
            let expected = channel_labels
                .get(channels.iter().position(|candidate| candidate == channel).unwrap())
                .unwrap()
                .len() as u64;
            prop_assert_eq!(observed, expected);

            let pool = left.channel_pool(channel).unwrap().unwrap();
            prop_assert_eq!(pool.len(), left_entries.len());
            for (position, entry) in left_entries.iter().enumerate() {
                let (candidate, address) = pool.candidate(position).unwrap();
                prop_assert_eq!(candidate, &entry.datum);
                match address {
                    DatumAddress::Stored(path) => {
                        prop_assert_eq!(path.as_ref(), entry.path.as_slice());
                    }
                    DatumAddress::Fresh => {
                        prop_assert!(false, "a stored channel catalog cannot produce a fresh address");
                    }
                }
            }
        }
        prop_assert!(forced.hits() > 0);
        prop_assert_eq!(&left, &right);
    }
}
