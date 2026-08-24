use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rspace_plus_plus::rspace::bag::{BagDelta, Multiplicity};
use rspace_plus_plus::rspace::data_index::{DataIndexError, RSpaceDataIndex, RSpaceDataStore};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::{Datum, DatumAddress};
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::serializers::serializers::encode_datum;
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::{Deserialize, Serialize};

static COUNTING_VALUE_DECODES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CountingValue(String);

impl StableHashSerialize for CountingValue {}

impl RhoPathEncode for CountingValue {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.as_bytes().to_vec() }
}

impl ColdStoreDecode for CountingValue {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        COUNTING_VALUE_DECODES.fetch_add(1, Ordering::Relaxed);
        legacy_prefix(bytes)
    }
}

fn datum(value: &str, persist: bool, discriminator: u8) -> Datum<String> {
    Datum {
        a: Arc::new(value.to_string()),
        persist,
        source: Produce::new(
            Blake2b256Hash::new(&[discriminator]),
            Blake2b256Hash::new(&[discriminator, 1]),
            persist,
        ),
    }
}

#[test]
fn identical_records_are_one_trie_key_with_multiplicity() {
    let mut index = RSpaceDataIndex::new();
    let value = datum("payload", false, 1);
    let (path, first) = index.insert(&"channel".to_string(), &value).unwrap();
    let (_, second) = index.insert(&"channel".to_string(), &value).unwrap();
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);
    assert_eq!(index.distinct_len(), 1);
    assert_eq!(index.cardinality(), 2);
    assert_eq!(index.count_path(&path), 2);
    assert_eq!(index.remove_path(&path).unwrap().get(), 1);
    assert_eq!(index.remove_path(&path), None);
    assert!(index.is_empty());
}

#[test]
fn exact_channels_are_isolated_by_their_framed_identity() {
    let mut index = RSpaceDataIndex::new();
    index
        .insert(&"a".to_string(), &datum("left", false, 1))
        .unwrap();
    index
        .insert(&"ab".to_string(), &datum("right", false, 2))
        .unwrap();
    let a = index
        .channel_entries::<_, String>(&"a".to_string())
        .unwrap();
    let ab = index
        .channel_entries::<_, String>(&"ab".to_string())
        .unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].datum.a.as_str(), "left");
    assert_eq!(ab.len(), 1);
    assert_eq!(ab[0].datum.a.as_str(), "right");
}

#[test]
fn structural_prefix_queries_prune_before_record_decode() {
    let mut index = RSpaceDataIndex::new();
    let channel = "channel".to_string();
    index
        .insert(&channel, &datum("shared/left", false, 1))
        .unwrap();
    index
        .insert(&channel, &datum("shared/right", false, 2))
        .unwrap();
    index.insert(&channel, &datum("other", false, 3)).unwrap();
    let entries = index
        .structure_prefix_entries::<_, String>(&channel, b"shared/")
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].datum.a.as_str(), "shared/left");
    assert_eq!(entries[1].datum.a.as_str(), "shared/right");
}

#[test]
fn clone_is_a_copy_on_write_residual_bag() {
    let mut original = RSpaceDataIndex::new();
    let channel = "channel".to_string();
    let value = datum("payload", false, 1);
    let (path, _) = original.insert(&channel, &value).unwrap();
    let mut residual = original.clone();
    residual.remove_path(&path);
    assert_eq!(original.count_path(&path), 1);
    assert_eq!(residual.count_path(&path), 0);
}

#[test]
fn channel_iteration_decodes_once_per_distinct_record() {
    let mut index = RSpaceDataIndex::new();
    let channel = "channel".to_string();
    let value = datum("payload", true, 7);
    for _ in 0..10 {
        index.insert(&channel, &value).unwrap();
    }
    let entries = index.channel_entries::<_, String>(&channel).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].multiplicity.get(), 10);
    assert_eq!(entries[0].datum, value);
}

#[test]
fn canonical_candidate_order_is_independent_of_insertion_order() {
    let channel = "channel".to_string();
    let values = [datum("alpha", false, 1), datum("beta", false, 2), datum("gamma", false, 3)];
    let mut forward = RSpaceDataIndex::new();
    let mut reverse = RSpaceDataIndex::new();
    for value in &values {
        forward.insert(&channel, value).unwrap();
    }
    for value in values.iter().rev() {
        reverse.insert(&channel, value).unwrap();
    }
    let forward_paths = forward
        .ordered_channel_entries::<_, String>(&channel)
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    let reverse_paths = reverse
        .ordered_channel_entries::<_, String>(&channel)
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    assert_eq!(forward_paths, reverse_paths);
}

#[test]
fn candidate_pool_uses_identity_bytes_for_order_and_decodes_on_first_access() {
    let channel = "channel".to_string();
    let values = [datum("alpha", false, 1), datum("beta", false, 2), datum("gamma", false, 3)];
    let mut string_index = RSpaceDataIndex::new();
    for value in &values {
        string_index.insert(&channel, value).unwrap();
    }
    let expected = string_index
        .ordered_channel_entries::<_, String>(&channel)
        .unwrap()
        .into_iter()
        .map(|entry| entry.datum)
        .collect::<Vec<_>>();
    let pool = string_index.candidate_pool::<_, String>(&channel).unwrap();
    let actual = (0..pool.len())
        .map(|position| pool.candidate(position).unwrap().0.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);

    let counted = Datum {
        a: Arc::new(CountingValue("payload".to_string())),
        persist: false,
        source: Produce::new(Blake2b256Hash::new(&[8]), Blake2b256Hash::new(&[8, 1]), false),
    };
    let mut counted_store = RSpaceDataStore::new();
    counted_store
        .set_channel(channel.clone(), [counted.clone()])
        .unwrap();
    COUNTING_VALUE_DECODES.store(0, Ordering::Relaxed);
    let counted_pool = counted_store.channel_pool(&channel).unwrap().unwrap();
    assert_eq!(COUNTING_VALUE_DECODES.load(Ordering::Relaxed), 0);
    assert_eq!(counted_pool.candidate(0).unwrap().0, &counted);
    assert_eq!(COUNTING_VALUE_DECODES.load(Ordering::Relaxed), 1);
    assert_eq!(counted_pool.clone().candidate(0).unwrap().0, &counted);
    assert_eq!(COUNTING_VALUE_DECODES.load(Ordering::Relaxed), 1);
    assert_eq!(
        counted_store
            .channel_pool(&channel)
            .unwrap()
            .unwrap()
            .candidate(0)
            .unwrap()
            .0,
        &counted
    );
    assert_eq!(COUNTING_VALUE_DECODES.load(Ordering::Relaxed), 1);

    let (_, address) = counted_pool.candidate(0).unwrap();
    let DatumAddress::Stored(path) = address else {
        panic!("resting data must have a PathMap address");
    };
    assert!(counted_store.remove_path(&path));
    counted_store.insert(&channel, &counted).unwrap();
    assert_eq!(
        counted_store
            .channel_pool(&channel)
            .unwrap()
            .unwrap()
            .candidate(0)
            .unwrap()
            .0,
        &counted
    );
    assert_eq!(COUNTING_VALUE_DECODES.load(Ordering::Relaxed), 1);

    assert!(counted_store.remove_path(&path));
    assert!(
        counted_store
            .channel_pool(&channel)
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn decoded_entries_and_encoded_pool_have_the_same_exact_order_and_addresses() {
    let channel = "channel".to_string();
    let values = [
        datum("alpha", false, 1),
        datum("beta", true, 2),
        datum("gamma", false, 3),
        datum("delta", true, 4),
    ];
    let mut index = RSpaceDataIndex::new();
    for value in &values {
        index.insert(&channel, value).unwrap();
    }

    let decoded = index
        .ordered_channel_entries::<_, String>(&channel)
        .expect("the decoded public view must use the stored-identity order");
    let pool = index
        .candidate_pool::<_, String>(&channel)
        .expect("the encoded candidate catalog must build");

    assert_eq!(pool.len(), decoded.len());
    for (position, expected) in decoded.iter().enumerate() {
        let (actual, address) = pool
            .candidate(position)
            .expect("every distinct encoded entry must occupy one catalog position");
        let DatumAddress::Stored(path) = address else {
            panic!("an index-backed candidate cannot have a fresh address");
        };
        assert_eq!(path.as_ref(), expected.path.as_slice());
        assert_eq!(actual, &expected.datum);

        let view = rspace_plus_plus::rspace::rho_path::RhoPathV1::parse_datum(path.as_ref())
            .expect("an inserted path must remain a valid datum path");
        assert_eq!(
            view.datum_identity(),
            encode_datum(actual),
            "the decoded datum must be the exact identity bytes that ordered the encoded catalog"
        );
    }
}

#[test]
fn repeated_exact_identity_is_one_ordered_position_with_reservable_multiplicity() {
    let channel = "channel".to_string();
    let repeated = datum("repeated", false, 9);
    let mut index = RSpaceDataIndex::new();
    for _ in 0..3 {
        index.insert(&channel, &repeated).unwrap();
    }

    let entries = index
        .ordered_channel_entries::<_, String>(&channel)
        .expect("one exact identity is valid regardless of multiplicity");
    assert_eq!(entries.len(), 1, "multiplicity must not expand the ordered catalog");
    assert_eq!(entries[0].multiplicity.get(), 3);

    let mut pool = index.candidate_pool::<_, String>(&channel).unwrap();
    assert_eq!(pool.len(), 1, "the pool has one position per exact identity");
    let (_, original_address) = pool
        .candidate(0)
        .expect("the repeated candidate is available");

    assert!(pool.reserve(0));
    assert!(pool.candidate(0).is_some(), "two copies remain after one reservation");
    assert!(pool.reserve(0));
    assert!(pool.candidate(0).is_some(), "one copy remains after two reservations");
    assert!(pool.reserve(0));
    assert!(pool.candidate(0).is_none(), "all three copies are now reserved");
    assert!(!pool.reserve(0), "a fourth reservation must fail");

    assert!(pool.release(0));
    let (_, restored_address) = pool.candidate(0).expect("release restores one copy");
    assert_eq!(restored_address, original_address, "multiplicity shares one exact removal address");
}

#[test]
fn public_index_refuses_one_identity_stored_under_two_derived_paths() {
    use rspace_plus_plus::rspace::bag::Bag;
    use rspace_plus_plus::rspace::rho_path::RhoPathV1;

    let channel = "channel".to_string();
    let value = datum("same datum", false, 7);
    let identity = encode_datum(&value);
    let left = RhoPathV1::datum_key(&channel, b"a-derived-structure", &identity);
    let right = RhoPathV1::datum_key(&channel, b"z-derived-structure", &identity);
    assert_ne!(left, right, "the malformed fixture needs two exact trie paths");

    let mut bag = Bag::new();
    bag.insert(&left).unwrap();
    bag.insert(&right).unwrap();
    let index = RSpaceDataIndex::from_bag(bag);
    assert_eq!(index.distinct_len(), 2, "the public Bag constructor makes the edge case real");

    assert_eq!(
        index.ordered_channel_entries::<_, String>(&channel),
        Err(DataIndexError::DuplicateDatumIdentity),
        "the decoded ordering boundary must reject a non-total identity catalog"
    );
    assert!(matches!(
        index.candidate_pool::<_, String>(&channel),
        Err(DataIndexError::DuplicateDatumIdentity)
    ));
}

#[test]
fn malformed_index_keys_fail_locally() {
    use rspace_plus_plus::rspace::bag::Bag;

    let channel = "channel".to_string();
    let mut bag = Bag::new();
    let mut malformed = rspace_plus_plus::rspace::rho_path::RhoPathV1::datum_prefix(&channel);
    malformed.extend_from_slice(&[0, 2]);
    bag.insert(&malformed).unwrap();
    let index = RSpaceDataIndex::from_bag(bag);
    assert!(matches!(index.channel_entries::<_, String>(&channel), Err(DataIndexError::Path(_))));
}

#[test]
fn data_store_distinguishes_unloaded_from_loaded_empty_channels() {
    let channel = "channel".to_string();
    let mut store = RSpaceDataStore::<String, String>::new();
    assert_eq!(store.channel_data(&channel).unwrap(), None);
    store.set_channel(channel.clone(), []).unwrap();
    assert_eq!(store.channel_data(&channel).unwrap(), Some(Vec::new()));
    assert_eq!(store.loaded_channel_count(), 1);
    assert_eq!(store.loaded_records().unwrap(), vec![(channel, Vec::new())]);
}

#[test]
fn channel_replacement_is_transactional_and_removes_the_old_subtrie() {
    let channel = "channel".to_string();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [datum("old", false, 1), datum("old", false, 1)])
        .unwrap();
    store
        .set_channel(channel.clone(), [datum("new", true, 2)])
        .unwrap();
    let values = store.channel_data(&channel).unwrap().unwrap();
    assert_eq!(values, vec![datum("new", true, 2)]);
    assert_eq!(store.distinct_len(), 1);
    assert_eq!(store.cardinality(), 1);
}

#[test]
fn removing_an_expanded_index_decrements_only_its_multiplicity() {
    let channel = "channel".to_string();
    let repeated = datum("repeated", false, 1);
    let other = datum("other", false, 2);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [repeated.clone(), other.clone(), repeated.clone()])
        .unwrap();
    let before = store.channel_data(&channel).unwrap().unwrap();
    let repeated_index = before.iter().position(|datum| datum == &repeated).unwrap();
    store.remove_index(&channel, repeated_index as i32).unwrap();
    assert_eq!(store.index().count(&channel, &repeated), 1);
    assert_eq!(store.index().count(&channel, &other), 1);
    assert_eq!(store.cardinality(), 2);
}

#[test]
fn removing_an_invalid_expanded_index_is_non_mutating() {
    let channel = "channel".to_string();
    let value = datum("value", false, 1);
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), [value]).unwrap();
    let before = store.clone();
    assert!(matches!(
        store.remove_index(&channel, -1),
        Err(DataIndexError::IndexOutOfBounds {
            index: -1,
            cardinality: 1
        })
    ));
    assert_eq!(store, before);
}

#[test]
fn candidate_pool_keeps_multiplicity_in_the_subtrie() {
    let channel = "channel".to_string();
    let repeated = datum("repeated", false, 1);
    let other = datum("other", false, 2);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [repeated.clone(), repeated.clone(), repeated.clone(), other])
        .unwrap();
    let mut pool = store.channel_pool(&channel).unwrap().unwrap();
    assert_eq!(pool.len(), 2);
    let repeated_position = (0..pool.len())
        .find(|position| {
            pool.candidate(*position)
                .is_some_and(|(value, _)| value == &repeated)
        })
        .unwrap();
    assert!(pool.reserve(repeated_position));
    assert_eq!(pool.next_available(repeated_position), Some(repeated_position));
    assert!(pool.reserve(repeated_position));
    assert_eq!(pool.next_available(repeated_position), Some(repeated_position));
    assert!(pool.reserve(repeated_position));
    assert_ne!(pool.next_available(repeated_position), Some(repeated_position));
    assert!(!pool.reserve(repeated_position));
    assert!(pool.release(repeated_position));
    assert!(pool.release(repeated_position));
    assert!(pool.release(repeated_position));
    assert_eq!(pool.next_available(repeated_position), Some(repeated_position));
}

#[test]
fn candidate_pool_addresses_remove_exact_path_multiplicity() {
    let channel = "channel".to_string();
    let repeated = datum("repeated", false, 1);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [repeated.clone(), repeated])
        .unwrap();
    let pool = store.channel_pool(&channel).unwrap().unwrap();
    let (_, address) = pool.candidate(0).unwrap();
    let DatumAddress::Stored(path) = address else {
        panic!("resting data must have a PathMap address");
    };
    assert!(store.remove_path(&path));
    assert_eq!(store.cardinality(), 1);
    assert!(store.remove_path(&path));
    assert_eq!(store.cardinality(), 0);
    assert!(!store.remove_path(&path));
}

#[test]
fn data_store_delta_commits_repeated_and_distinct_removals_atomically() {
    let channel = "channel".to_string();
    let repeated = datum("repeated", false, 1);
    let other = datum("other", false, 2);
    let repeated_path = RSpaceDataIndex::datum_path(&channel, &repeated);
    let other_path = RSpaceDataIndex::datum_path(&channel, &other);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [
            repeated.clone(),
            repeated.clone(),
            repeated.clone(),
            other.clone(),
        ])
        .unwrap();

    // Populate the derived catalog before the transaction so the test also
    // covers exact before/after cache maintenance.
    assert_eq!(store.channel_pool(&channel).unwrap().unwrap().len(), 2);
    let mut delta = BagDelta::new();
    delta
        .record_removal(&repeated_path, Multiplicity::new(2).unwrap())
        .unwrap();
    delta
        .record_removal(&other_path, Multiplicity::ONE)
        .unwrap();
    store.apply_delta(&delta).unwrap();

    assert_eq!(store.index().count(&channel, &repeated), 1);
    assert_eq!(store.index().count(&channel, &other), 0);
    assert_eq!(store.distinct_len(), 1);
    assert_eq!(store.cardinality(), 1);
    let residual = store.channel_pool(&channel).unwrap().unwrap();
    assert_eq!(residual.len(), 1);
    assert_eq!(residual.candidate(0).unwrap().0, &repeated);

    store.apply_delta(&delta.inverse()).unwrap();
    assert_eq!(store.index().count(&channel, &repeated), 3);
    assert_eq!(store.index().count(&channel, &other), 1);
    assert_eq!(store.distinct_len(), 2);
    assert_eq!(store.cardinality(), 4);
    assert_eq!(store.channel_pool(&channel).unwrap().unwrap().len(), 2);
}

#[test]
fn data_store_delta_underflow_is_non_mutating() {
    let channel = "channel".to_string();
    let present = datum("present", false, 1);
    let absent = datum("absent", false, 2);
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [present.clone()])
        .unwrap();
    let before = store.clone();
    let mut delta = BagDelta::new();
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&channel, &present), Multiplicity::ONE)
        .unwrap();
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&channel, &absent), Multiplicity::ONE)
        .unwrap();

    assert!(matches!(store.apply_delta(&delta), Err(DataIndexError::Bag(_))));
    assert_eq!(store, before);
}

#[test]
fn data_store_delta_rejects_malformed_paths_before_publication() {
    let channel = "channel".to_string();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel, [datum("present", false, 1)])
        .unwrap();
    let before = store.clone();
    let mut delta = BagDelta::new();
    delta
        .record_addition(b"not-a-rho-datum-path", Multiplicity::ONE)
        .unwrap();

    assert!(matches!(store.apply_delta(&delta), Err(DataIndexError::Path(_))));
    assert_eq!(store, before);
}

#[test]
fn fresh_candidate_reservation_is_reversible_and_not_stored() {
    let channel = "channel".to_string();
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), []).unwrap();
    let mut pool = store.channel_pool(&channel).unwrap().unwrap();
    assert!(pool.prepend_fresh(datum("fresh", false, 9)));
    assert!(matches!(pool.candidate(0), Some((_, DatumAddress::Fresh))));
    assert!(pool.reserve(0));
    assert!(pool.is_empty());
    assert_eq!(pool.candidate(0), None);
    assert!(pool.release(0));
    assert!(matches!(pool.candidate(0), Some((_, DatumAddress::Fresh))));
    assert!(!pool.release(0), "a double fresh release must not create capacity");
}
