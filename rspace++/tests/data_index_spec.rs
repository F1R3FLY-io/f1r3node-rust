use std::sync::Arc;

use rspace_plus_plus::rspace::data_index::{DataIndexError, RSpaceDataIndex, RSpaceDataStore};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, DatumAddress};
use rspace_plus_plus::rspace::trace::event::Produce;

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
    pool.release(repeated_position);
    pool.release(repeated_position);
    pool.release(repeated_position);
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
fn fresh_candidate_reservation_is_reversible_and_not_stored() {
    let channel = "channel".to_string();
    let mut store = RSpaceDataStore::new();
    store.set_channel(channel.clone(), []).unwrap();
    let mut pool = store.channel_pool(&channel).unwrap().unwrap();
    pool.prepend_fresh(datum("fresh", false, 9));
    assert!(matches!(pool.candidate(0), Some((_, DatumAddress::Fresh))));
    assert!(pool.reserve(0));
    assert!(pool.is_empty());
    assert_eq!(pool.candidate(0), None);
    pool.release(0);
    assert!(matches!(pool.candidate(0), Some((_, DatumAddress::Fresh))));
}
