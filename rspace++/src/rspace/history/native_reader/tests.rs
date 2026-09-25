use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use crate::rspace::history::radix_tree::{Item, decode, empty_node, encode};
use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

struct Meter {
    remaining: AtomicUsize,
    calls: AtomicUsize,
    backing: AtomicUsize,
}

impl Meter {
    fn new(remaining: usize) -> Self {
        Self {
            remaining: AtomicUsize::new(remaining),
            calls: AtomicUsize::new(0),
            backing: AtomicUsize::new(0),
        }
    }
}

impl NativeReadMeter for Meter {
    type Error = &'static str;
    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        self.remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .map_err(|_| "exhausted")?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.backing
            .fetch_add(charge.backing_bytes, Ordering::Relaxed);
        Ok(())
    }
}

fn frame(kind: NativeLeafKind, payload: &[u8], trailing: &[u8]) -> ([u8; 32], Vec<u8>) {
    let tag = match kind {
        NativeLeafKind::Joins => 0_u32,
        NativeLeafKind::Data => 1,
        NativeLeafKind::Continuations => 2,
    };
    let mut leaf = bincode::serialize(&payload).unwrap();
    let hash = digest(&leaf);
    let mut bytes = tag.to_le_bytes().to_vec();
    bytes.append(&mut leaf);
    bytes.extend_from_slice(trailing);
    (hash, bytes)
}

fn install(
    nodes: &dyn KeyValueStore,
    leaves: &dyn KeyValueStore,
    kind: NativeLeafKind,
    hash: [u8; 32],
    leaf: Vec<u8>,
    serialized: bool,
) -> [u8; 32] {
    let key = if serialized {
        bincode::serialize(&hash.to_vec()).unwrap()
    } else {
        hash.to_vec()
    };
    leaves.put_one(key, leaf).unwrap();
    let mut node = empty_node();
    node[kind.prefix() as usize] = Item::Leaf {
        prefix: vec![7; 32],
        value: hash.to_vec(),
    };
    let bytes = encode(&node);
    let root = digest(&bytes);
    nodes.put_one(root.to_vec(), bytes).unwrap();
    root
}

fn read(
    reader: &NativeHistoryReader<'_>,
    kind: NativeLeafKind,
    meter: &Meter,
) -> Result<Option<Vec<Vec<u8>>>, NativeReadError<&'static str>> {
    reader.with_records(kind, &[7; 32], meter, |rows| Ok(rows.iter().map(<[u8]>::to_vec).collect()))
}

#[test]
fn all_leaf_kinds_and_both_key_encodings_match_legacy_framing() {
    for kind in [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins] {
        for serialized in [false, true] {
            let nodes = Arc::new(InMemoryKeyValueStore::new());
            let leaves = Arc::new(InMemoryKeyValueStore::new());
            let rows = vec![vec![], vec![1, 2, 3], vec![4; 300]];
            let mut payload = bincode::serialize(&rows).unwrap();
            payload.extend_from_slice(&[31, 32]);
            let (hash, bytes) = frame(kind, &payload, &[91, 92, 93]);
            let root = install(nodes.as_ref(), leaves.as_ref(), kind, hash, bytes, serialized);
            let reader = NativeHistoryReader::new(root, nodes.as_ref(), leaves.as_ref());
            assert_eq!(read(&reader, kind, &Meter::new(usize::MAX)).unwrap(), Some(rows.clone()));
            let history = RadixHistory::create(Blake2b256Hash(root.to_vec()), nodes).unwrap();
            let legacy = RSpaceHistoryReaderImpl::<String, String, String, String>::new(
                Box::new(history),
                leaves,
            );
            let key = Blake2b256Hash(vec![7; 32]);
            let actual = match kind {
                NativeLeafKind::Data => legacy.get_data_proj_binary(&key),
                NativeLeafKind::Continuations => legacy.get_continuations_proj_binary(&key),
                NativeLeafKind::Joins => legacy.get_joins_proj_binary(&key),
            }
            .unwrap();
            assert_eq!(actual, rows);
        }
    }
}

#[test]
fn every_reservation_rejection_prevents_the_consumer() {
    for serialized in [false, true] {
        let nodes = InMemoryKeyValueStore::new();
        let leaves = InMemoryKeyValueStore::new();
        let (hash, bytes) =
            frame(NativeLeafKind::Data, &bincode::serialize(&vec![vec![1_u8]]).unwrap(), &[]);
        let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes, serialized);
        let reader = NativeHistoryReader::new(root, &nodes, &leaves);
        let all = Meter::new(usize::MAX);
        read(&reader, NativeLeafKind::Data, &all).unwrap();
        for allowed in 0..all.calls.load(Ordering::Relaxed) {
            let calls = Cell::new(0);
            let result =
                reader.with_records(NativeLeafKind::Data, &[7; 32], &Meter::new(allowed), |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                });
            assert!(matches!(result, Err(NativeReadError::Host("exhausted"))));
            assert_eq!(calls.get(), 0);
        }
    }
}

#[test]
fn invalid_final_record_is_rejected_before_any_consumer() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let mut payload = bincode::serialize(&vec![vec![1_u8], vec![2_u8]]).unwrap();
    payload.pop();
    let (hash, bytes) = frame(NativeLeafKind::Data, &payload, &[]);
    let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes, false);
    let reader = NativeHistoryReader::new(root, &nodes, &leaves);
    let calls = Cell::new(0);
    let result =
        reader.with_records(NativeLeafKind::Data, &[7; 32], &Meter::new(usize::MAX), |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });
    assert!(matches!(result, Err(NativeReadError::Invalid(NativeReadFault::Truncated))));
    assert_eq!(calls.get(), 0);
}

#[test]
fn overflowing_lengths_and_empty_rows_obey_the_framing_bounds() {
    let payload = bincode::serialize(&vec![Vec::<u8>::new(); 3]).unwrap();
    let (_, valid) = frame(NativeLeafKind::Data, &payload, &[]);
    let (rows, _) = NativeRecords::parse(&valid, NativeLeafKind::Data).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().len(), 3);
    assert!(rows.iter().all(<[u8]>::is_empty));
    for offset in [4, 12, 20] {
        let mut malformed = valid.clone();
        malformed[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(NativeRecords::parse(&malformed, NativeLeafKind::Data).is_err());
    }
    let (_, empty) = frame(NativeLeafKind::Data, &0_u64.to_le_bytes(), &[]);
    let (rows, _) = NativeRecords::parse(&empty, NativeLeafKind::Data).unwrap();
    assert!(rows.is_empty());
    assert_eq!(rows.iter().next(), None);
}

#[test]
fn consumer_failure_is_not_a_host_or_storage_failure() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let (hash, bytes) = frame(NativeLeafKind::Data, &0_u64.to_le_bytes(), &[]);
    let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes, false);
    let reader = NativeHistoryReader::new(root, &nodes, &leaves);
    let calls = Cell::new(0);
    let result: Result<Option<()>, _> =
        reader.with_records(NativeLeafKind::Data, &[7; 32], &Meter::new(usize::MAX), |_| {
            calls.set(calls.get() + 1);
            Err("consumer refused")
        });
    assert!(matches!(result, Err(NativeReadError::Consumer("consumer refused"))));
    assert_eq!(calls.get(), 1);
}

#[test]
fn malformed_raw_leaf_does_not_fall_back_to_a_valid_serialized_key() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let (hash, bytes) =
        frame(NativeLeafKind::Data, &bincode::serialize(&Vec::<Vec<u8>>::new()).unwrap(), &[]);
    let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes, true);
    leaves.put_one(hash.to_vec(), vec![255]).unwrap();
    let reader = NativeHistoryReader::new(root, &nodes, &leaves);
    assert!(matches!(
        read(&reader, NativeLeafKind::Data, &Meter::new(usize::MAX)),
        Err(NativeReadError::Invalid(NativeReadFault::Truncated))
    ));
}

#[test]
fn missing_and_corrupt_storage_are_errors_not_absence() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let meter = Meter::new(usize::MAX);
    assert_eq!(
        read(&NativeHistoryReader::new(digest(&[]), &nodes, &leaves), NativeLeafKind::Data, &meter)
            .unwrap(),
        None
    );
    assert!(matches!(
        read(&NativeHistoryReader::new([9; 32], &nodes, &leaves), NativeLeafKind::Data, &meter),
        Err(NativeReadError::Invalid(NativeReadFault::MissingNode))
    ));
    let (hash, bytes) =
        frame(NativeLeafKind::Data, &bincode::serialize(&Vec::<Vec<u8>>::new()).unwrap(), &[]);
    let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes.clone(), false);
    leaves.delete(vec![hash.to_vec()]).unwrap();
    let reader = NativeHistoryReader::new(root, &nodes, &leaves);
    assert!(matches!(
        read(&reader, NativeLeafKind::Data, &meter),
        Err(NativeReadError::Invalid(NativeReadFault::MissingLeaf))
    ));
    let mut wrong_kind = bytes.clone();
    wrong_kind[0] = 0;
    leaves.put_one(hash.to_vec(), wrong_kind).unwrap();
    assert!(matches!(
        read(&reader, NativeLeafKind::Data, &meter),
        Err(NativeReadError::Invalid(NativeReadFault::LeafKind))
    ));
    let (_, wrong_hash) =
        frame(NativeLeafKind::Data, &bincode::serialize(&vec![vec![4_u8]]).unwrap(), &[]);
    leaves.put_one(hash.to_vec(), wrong_hash).unwrap();
    assert!(matches!(
        read(&reader, NativeLeafKind::Data, &meter),
        Err(NativeReadError::Invalid(NativeReadFault::LeafHash))
    ));
    nodes.put_one(root.to_vec(), vec![]).unwrap();
    assert!(matches!(
        read(&reader, NativeLeafKind::Data, &meter),
        Err(NativeReadError::Invalid(NativeReadFault::NodeHash))
    ));
}

#[test]
fn radix_framing_accepts_unsorted_records_but_rejects_duplicates_and_truncation() {
    let mut bytes = vec![9, 0];
    bytes.extend_from_slice(&[1; 32]);
    bytes.extend_from_slice(&[2, 0]);
    bytes.extend_from_slice(&[2; 32]);
    assert_eq!(node_step(&bytes, &[2]).unwrap(), NodeStep::Leaf([2; 32]));
    let mut duplicate = bytes.clone();
    duplicate[34] = 9;
    assert_eq!(node_step(&duplicate, &[9]), Err(NativeReadFault::DuplicateIndex));
    for length in 1..34 {
        assert_eq!(node_step(&bytes[..length], &[9]), Err(NativeReadFault::Truncated));
    }
    assert_eq!(node_step(&bytes, &[2, 1]).unwrap(), NodeStep::Absent);
    let mut maximum = Vec::new();
    for index in 0..256 {
        maximum.extend_from_slice(&[index as u8, 127]);
        maximum.extend_from_slice(&[3; 159]);
    }
    assert_eq!(maximum.len(), MAX_NODE_BYTES);
    let mut key = vec![255];
    key.extend_from_slice(&[3; 127]);
    assert_eq!(node_step(&maximum, &key).unwrap(), NodeStep::Leaf([3; 32]));
    maximum.push(0);
    assert_eq!(node_step(&maximum, &key), Err(NativeReadFault::NodeSize));
}

#[test]
fn full_depth_traversal_consumes_the_key_and_does_not_read_an_exhausted_child() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let rows = vec![vec![42_u8]];
    let (hash, leaf) = frame(NativeLeafKind::Data, &bincode::serialize(&rows).unwrap(), &[]);
    leaves.put_one(hash.to_vec(), leaf).unwrap();
    let mut lookup = [7; 33];
    lookup[0] = 0;
    let mut pointer = hash;
    for index in (0..33).rev() {
        let mut bytes = vec![lookup[index], if index == 32 { 0 } else { 128 }];
        bytes.extend_from_slice(&pointer);
        pointer = digest(&bytes);
        nodes.put_one(pointer.to_vec(), bytes).unwrap();
    }
    let reader = NativeHistoryReader::new(pointer, &nodes, &leaves);
    let meter = Meter::new(usize::MAX);
    assert_eq!(read(&reader, NativeLeafKind::Data, &meter).unwrap(), Some(rows));
    assert_eq!(meter.calls.load(Ordering::Relaxed), 1 + 2 * 33 + 2);
    let mut node = vec![0, 160];
    node.extend_from_slice(&[7; 32]);
    node.extend_from_slice(&[99; 32]);
    let root = digest(&node);
    nodes.put_one(root.to_vec(), node).unwrap();
    assert_eq!(
        read(
            &NativeHistoryReader::new(root, &nodes, &leaves),
            NativeLeafKind::Data,
            &Meter::new(usize::MAX)
        )
        .unwrap(),
        None
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn generated_leaf_frames_match_bincode(rows in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..200), 0..32), inner in prop::collection::vec(any::<u8>(), 0..16), outer in prop::collection::vec(any::<u8>(), 0..16)) {
        let mut payload = bincode::serialize(&rows).unwrap(); payload.extend_from_slice(&inner);
        let (hash, bytes) = frame(NativeLeafKind::Data, &payload, &outer);
        let (view, hash_bytes) = NativeRecords::parse(&bytes, NativeLeafKind::Data).unwrap();
        prop_assert_eq!(digest(hash_bytes), hash);
        prop_assert_eq!(view.len(), rows.len());
        prop_assert_eq!(view.is_empty(), rows.is_empty());
        prop_assert_eq!(view.iter().map(<[u8]>::to_vec).collect::<Vec<_>>(), rows);
        for end in 0..12 + payload.len() {
            prop_assert!(NativeRecords::parse(&bytes[..end], NativeLeafKind::Data).is_err());
        }
    }

    #[test]
    fn arbitrary_framing_is_bounded_and_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2048), key in prop::collection::vec(any::<u8>(), 0..40)) {
        let _ = node_step(&bytes, &key);
        for kind in [NativeLeafKind::Data, NativeLeafKind::Continuations, NativeLeafKind::Joins] {
            if let Ok((rows, _)) = NativeRecords::parse(&bytes, kind) {
                prop_assert_eq!(rows.iter().count(), rows.len());
                prop_assert!(rows.len() <= bytes.len() / 8);
            }
        }
    }

    #[test]
    fn borrowed_radix_selection_matches_decoded_nodes(entries in prop::collection::btree_map(any::<u8>(), (any::<bool>(), prop::collection::vec(any::<u8>(), 0..8), any::<[u8;32]>()), 0..80), key in prop::collection::vec(any::<u8>(), 0..12)) {
        let mut node = empty_node();
        for (index, (child, prefix, hash)) in entries {
            node[index as usize] = if child { Item::NodePtr { prefix, ptr: hash.to_vec() } } else { Item::Leaf { prefix, value: hash.to_vec() } };
        }
        let bytes = encode(&node);
        let decoded = decode(bytes.clone());
        let expected = match key.split_first() {
            None => NodeStep::Absent,
            Some((index, tail)) => match &decoded[*index as usize] {
                Item::Leaf { prefix, value } if prefix == tail => NodeStep::Leaf(value.as_slice().try_into().unwrap()),
                Item::NodePtr { prefix, ptr } if tail.starts_with(prefix) => NodeStep::Child { hash: ptr.as_slice().try_into().unwrap(), consumed: 1 + prefix.len() },
                _ => NodeStep::Absent,
            },
        };
        prop_assert_eq!(node_step(&bytes, &key).unwrap(), expected);
    }
}

mod allocations;

#[tokio::test]
async fn native_factory_keeps_the_storage_handles_through_checkpoint_clone_and_reset() {
    use crate::rspace::hashing::stable_hash_provider::hash;
    use crate::rspace::internal::Datum;
    use crate::rspace::r#match::Match;
    use crate::rspace::rspace::RSpace;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    struct Matcher;
    impl Match<String, String, String> for Matcher {
        fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
    }
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let original = play.get_history_repository();
    play.produce("c".into(), "v".into(), false).await.unwrap();
    let checkpoint = play.create_checkpoint().await.unwrap();
    let current = play.get_history_repository();
    let cloned = current.checkpoint(Vec::new());
    let reset = original.reset(&checkpoint.root).unwrap();
    let root: [u8; 32] = checkpoint.root.0.as_slice().try_into().unwrap();
    let channel: [u8; 32] = hash(&"c".to_owned()).0.as_slice().try_into().unwrap();
    for history in [current.as_ref().as_ref(), cloned.as_ref(), reset.as_ref()] {
        let values = history
            .native_history_reader(root)
            .with_records(NativeLeafKind::Data, &channel, &Meter::new(usize::MAX), |rows| {
                Ok(rows
                    .iter()
                    .map(|bytes| bincode::deserialize::<Datum<String>>(bytes).unwrap().a)
                    .collect::<Vec<_>>())
            })
            .unwrap()
            .unwrap();
        assert_eq!(values, ["v"]);
    }
}

#[test]
fn concurrent_readers_share_the_limit_without_publishing_unchecked_records() {
    let nodes = InMemoryKeyValueStore::new();
    let leaves = InMemoryKeyValueStore::new();
    let (hash, bytes) =
        frame(NativeLeafKind::Data, &bincode::serialize(&vec![vec![5_u8]]).unwrap(), &[]);
    let root = install(&nodes, &leaves, NativeLeafKind::Data, hash, bytes, false);
    let reader = NativeHistoryReader::new(root, &nodes, &leaves);
    for _ in 0..32 {
        let meter = Meter::new(5);
        let consumers = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        let result =
                            reader.with_records(NativeLeafKind::Data, &[7; 32], &meter, |rows| {
                                assert_eq!(rows.iter().next().unwrap(), [5]);
                                consumers.fetch_add(1, Ordering::Relaxed);
                                Ok(())
                            });
                        assert!(matches!(
                            result,
                            Ok(Some(())) | Err(NativeReadError::Host("exhausted"))
                        ));
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
        });
        assert_eq!(meter.calls.load(Ordering::Relaxed), 5);
        assert!(consumers.load(Ordering::Relaxed) <= 1);
    }
}
