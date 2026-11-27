use std::sync::Barrier;

use proptest::prelude::*;
use rand::{RngCore, SeedableRng};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use shared::rust::store::memory_key_value_store::MemoryKeyValueStore;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData};

fn fixture(kind: u8, bytes: Vec<u8>, serialized_key: bool) -> (ByteBuffer, ByteBuffer) {
    let leaf = bincode::serialize(&DataLeaf {
        bytes: bytes.clone(),
    })
    .unwrap();
    let hash = Blake2b256Hash::new(&leaf);
    let key = if serialized_key {
        bincode::serialize(&hash).unwrap()
    } else {
        hash.bytes()
    };
    let data = match kind {
        0 => PersistedData::Joins(JoinsLeaf { bytes }),
        1 => PersistedData::Data(DataLeaf { bytes }),
        2 => PersistedData::Continuations(ContinuationsLeaf { bytes }),
        _ => unreachable!(),
    };
    (key, bincode::serialize(&data).unwrap())
}

fn codec(enabled: bool) -> ColdStoreCodec {
    ColdStoreCodec {
        inner: Arc::new(MemoryKeyValueStore::new()),
        compress_writes: enabled,
        budget: Arc::new(CodecBudget::new(MAX_CODEC_OPERATIONS, MAX_SCRATCH_BYTES)),
    }
}

fn frame(value: &[u8]) -> Vec<u8> {
    let payload = lz4_flex::block::compress(value);
    let mut result = bincode::serialize(&Header {
        magic: MAGIC,
        version: 1,
        codec: 1,
        flags: 0,
        canonical_len: value.len() as u64,
        payload_len: payload.len() as u64,
        digest: Blake2b::<U32>::digest(value).into(),
    })
    .unwrap();
    assert_eq!(result.len(), HEADER_LEN);
    result.extend_from_slice(&payload);
    result
}

fn change_header(value: &mut [u8], update: impl FnOnce(&mut Header)) {
    let mut header: Header = bincode::deserialize(&value[..HEADER_LEN]).unwrap();
    update(&mut header);
    bincode::serialize_into(&mut value[..HEADER_LEN], &header).unwrap();
}

#[test]
fn legacy_variants_and_both_key_formats_remain_exact() {
    for kind in 0..3 {
        for serialized_key in [false, true] {
            for payload in [vec![], vec![3, 4, 4], vec![17; 8192]] {
                let (key, canonical) = fixture(kind, payload, serialized_key);
                assert!(!canonical.starts_with(&MAGIC));
                let store = codec(false);
                store.put_one(key.clone(), canonical.clone()).unwrap();
                assert_eq!(store.inner.get_one(&key).unwrap(), Some(canonical.clone()));
                assert_eq!(store.get_one(&key).unwrap(), Some(canonical.clone()));
                assert_eq!(store.decode(&key, frame(&canonical)).unwrap(), canonical);
            }
        }
    }
}

#[test]
fn raw_reads_do_not_rehash_leaf_payloads() {
    for enabled in [false, true] {
        for kind in 0..3 {
            for serialized_key in [false, true] {
                let store = codec(enabled);
                let (key, canonical) = fixture(kind, vec![11; 8192], serialized_key);
                store.inner.put_one(key.clone(), canonical.clone()).unwrap();
                LEAF_HASH_CHECKS.set(0);
                assert_eq!(store.get_one(&key).unwrap(), Some(canonical.clone()));
                assert_eq!(store.contains(&vec![key.clone()]).unwrap(), vec![true]);
                assert_eq!(store.scan_page(None, 1).unwrap().rows, vec![(
                    key.clone(),
                    canonical.clone()
                )]);
                assert_eq!(store.to_map().unwrap(), BTreeMap::from([(key, canonical)]));
                assert_eq!(LEAF_HASH_CHECKS.get(), 0);
            }
        }
    }
}

#[test]
fn conditional_batches_validate_each_input_only_once() {
    for enabled in [false, true] {
        let store = codec(enabled);
        let pairs: Vec<_> = (0..3)
            .map(|kind| fixture(kind, vec![kind; 8192], true))
            .collect();
        LEAF_HASH_CHECKS.set(0);
        store.put_if_absent(pairs.clone()).unwrap();
        assert_eq!(LEAF_HASH_CHECKS.get(), 3);
        LEAF_HASH_CHECKS.set(0);
        store.put_if_absent(pairs).unwrap();
        assert_eq!(LEAF_HASH_CHECKS.get(), 3 + usize::from(enabled));
    }
}

#[test]
fn untrusted_writes_reject_wrong_hashes_before_committing() {
    for enabled in [false, true] {
        for serialized_key in [false, true] {
            let store = codec(enabled);
            let valid = fixture(1, vec![1; 8192], serialized_key);
            let mut wrong = fixture(2, vec![2; 8192], serialized_key);
            *wrong.0.last_mut().unwrap() ^= 1;
            assert!(store.put(vec![valid.clone(), wrong.clone()]).is_err());
            assert!(!store.non_empty().unwrap());
            assert!(store.put_if_absent(vec![valid, wrong.clone()]).is_err());
            assert!(!store.non_empty().unwrap());
            assert!(store.put_one_if_absent(wrong.0, wrong.1).is_err());
            assert!(!store.non_empty().unwrap());
        }
    }
}

#[test]
fn disabled_writes_still_verify_compressed_reads() {
    let store = codec(false);
    let (key, canonical) = fixture(2, vec![1; 8192], true);
    let physical = frame(&canonical);
    store.inner.put_one(key.clone(), physical.clone()).unwrap();
    LEAF_HASH_CHECKS.set(0);
    assert_eq!(store.get_one(&key).unwrap(), Some(canonical.clone()));
    assert_eq!(LEAF_HASH_CHECKS.get(), 1);
    let mut damaged = physical.clone();
    change_header(&mut damaged, |header| header.digest[0] ^= 1);
    store.inner.put_one(key.clone(), damaged).unwrap();
    assert!(store.get_one(&key).is_err());
    assert!(store.contains(&vec![key.clone()]).is_err());
    assert!(store.put_if_absent(vec![(key.clone(), canonical)]).is_err());
    let mut wrong_key = key;
    *wrong_key.last_mut().unwrap() ^= 1;
    store.inner.put_one(wrong_key.clone(), physical).unwrap();
    assert!(store.get_one(&wrong_key).is_err());
}

#[test]
fn checkpoint_leaves_reuse_generated_hashes() {
    use shared::rust::store::memory_key_value_store::MemoryEnvironment;

    use crate::rspace::history::history_repository::HistoryRepositoryInstances;
    use crate::rspace::hot_store_action::{
        HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
    };
    use crate::rspace::hot_store_trie_action::{
        HotStoreTrieAction, TrieInsertAction, TrieInsertBinaryConsume, TrieInsertBinaryJoins,
        TrieInsertBinaryProduce,
    };
    use crate::rspace::internal::{Datum, WaitingContinuation};

    for enabled in [false, true] {
        let env = Arc::new(MemoryEnvironment::new());
        let backing = Arc::new(env.create_store());
        let store = Arc::new(ColdStoreCodec {
            inner: backing.clone(),
            compress_writes: enabled,
            budget: Arc::new(CodecBudget::new(MAX_CODEC_OPERATIONS, MAX_SCRATCH_BYTES)),
        });
        let repo = HistoryRepositoryInstances::<String, String, String, String>::lmdb_repository(
            Arc::new(env.create_store()),
            Arc::new(env.create_store()),
            store.clone(),
        )
        .unwrap();
        let actions = vec![
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                TrieInsertBinaryProduce {
                    hash: Blake2b256Hash::new(b"data"),
                    data: vec![vec![1; 8192]],
                },
            )),
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
                TrieInsertBinaryConsume {
                    hash: Blake2b256Hash::new(b"continuations"),
                    continuations: vec![vec![2; 8192]],
                },
            )),
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
                TrieInsertBinaryJoins {
                    hash: Blake2b256Hash::new(b"joins"),
                    joins: vec![vec![3; 8192]],
                },
            )),
        ];
        LEAF_HASH_CHECKS.set(0);
        let checkpoint = repo.do_checkpoint(actions.clone());
        assert_eq!(LEAF_HASH_CHECKS.get(), 0);
        let physical = backing.to_map().unwrap();
        assert_eq!(physical.len(), 3);
        assert_eq!(
            physical
                .values()
                .filter(|value| value.starts_with(&MAGIC))
                .count(),
            usize::from(enabled)
        );
        for (key, canonical) in store.to_map().unwrap() {
            validate_canonical(&key, &canonical).unwrap();
        }
        LEAF_HASH_CHECKS.set(0);
        assert_eq!(checkpoint.do_checkpoint(actions).root(), checkpoint.root());
        assert_eq!(LEAF_HASH_CHECKS.get(), usize::from(enabled));
        assert_eq!(backing.to_map().unwrap(), physical);

        let channel = "native-data".to_string();
        let channels = vec!["native-continuation".to_string()];
        let patterns = vec!["pattern".to_string()];
        let native = vec![
            HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                channel: channel.clone(),
                data: vec![Datum::create(&channel, "datum".into(), false)],
            })),
            HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                channels: channels.clone(),
                continuations: vec![WaitingContinuation::create(
                    &channels,
                    &patterns,
                    &"body".repeat(4096),
                    true,
                    Default::default(),
                )],
            })),
            HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                channel: channel.clone(),
                joins: vec![channels],
            })),
        ];
        LEAF_HASH_CHECKS.set(0);
        let native_checkpoint = checkpoint.checkpoint(native.clone());
        assert_eq!(LEAF_HASH_CHECKS.get(), 0);
        let reader = native_checkpoint
            .get_history_reader(&native_checkpoint.root())
            .unwrap();
        assert_eq!(reader.base().get_data(&channel).len(), 1);
        assert_eq!(reader.base().get_joins(&channel).len(), 1);
        LEAF_HASH_CHECKS.set(0);
        assert_eq!(native_checkpoint.checkpoint(native.clone()).root(), native_checkpoint.root());
        assert_eq!(LEAF_HASH_CHECKS.get(), usize::from(enabled));
        for (key, canonical) in store.to_map().unwrap() {
            validate_canonical(&key, &canonical).unwrap();
        }
        let (key, canonical) = store
            .to_map()
            .unwrap()
            .into_iter()
            .find(|(_, value)| {
                matches!(
                    bincode::deserialize::<PersistedData>(value).unwrap(),
                    PersistedData::Continuations(_)
                ) && value.len() > 12000
            })
            .unwrap();
        let mut damaged = frame(&canonical);
        change_header(&mut damaged, |header| header.digest[0] ^= 1);
        backing.put_one(key.clone(), damaged.clone()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            native_checkpoint.checkpoint(native)
        }));
        assert!(result.is_err());
        assert_eq!(backing.get_one(&key).unwrap(), Some(damaged));
    }
}

#[test]
fn only_sufficiently_large_profitable_continuations_are_compressed() {
    let store = codec(true);
    for kind in 0..3 {
        let (key, canonical) = fixture(kind, vec![1; 8192], false);
        let physical = store.encode(&key, canonical.clone()).unwrap();
        assert_eq!(physical.starts_with(&MAGIC), kind == 2);
        assert_eq!(store.decode(&key, physical.clone()).unwrap(), canonical);
        if kind == 2 {
            assert!(physical.len() + MIN_SAVING_BYTES <= canonical.len());
            assert_eq!(physical.capacity(), physical.len());
        }
    }
    let (key, small) = fixture(2, vec![1; 32], false);
    assert_eq!(store.encode(&key, small.clone()).unwrap(), small);
    let mut random = vec![0; 8192];
    rand::rngs::StdRng::seed_from_u64(17).fill_bytes(&mut random);
    let (key, canonical) = fixture(2, random, false);
    assert_eq!(store.encode(&key, canonical.clone()).unwrap(), canonical);
}

#[test]
fn oversized_valid_legacy_values_are_raw_not_rejected() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![4; MAX_CANONICAL_RECORD_BYTES], false);
    assert!(canonical.len() > MAX_CANONICAL_RECORD_BYTES);
    let physical = store.encode(&key, canonical.clone()).unwrap();
    assert_eq!(physical, canonical);
    assert_eq!(store.decode(&key, physical).unwrap(), canonical);
}

#[test]
fn canonical_validation_rejects_wrong_keys_tags_lengths_and_trailing_bytes() {
    let store = codec(false);
    let (key, canonical) = fixture(2, vec![3; 4096], false);
    let mut wrong_key = key.clone();
    wrong_key[0] ^= 1;
    assert!(store.encode(&wrong_key, canonical.clone()).is_err());
    assert!(store.decode(&wrong_key, frame(&canonical)).is_err());
    assert_eq!(store.decode(&wrong_key, canonical.clone()).unwrap(), canonical);
    for bad_key in [vec![], vec![1; 31], vec![255; 40], bincode::serialize(&vec![1u8; 31]).unwrap()]
    {
        assert!(store.encode(&bad_key, canonical.clone()).is_err());
        assert!(store.decode(&bad_key, canonical.clone()).is_err());
    }
    let mut trailing = canonical.clone();
    trailing.push(0);
    let mut bad_tag = canonical.clone();
    bad_tag[0] = 3;
    let mut bad_length = canonical.clone();
    bad_length[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    for invalid in [trailing, bad_tag, bad_length] {
        assert!(store.encode(&key, invalid.clone()).is_err());
        assert!(store.decode(&key, invalid.clone()).is_err());
        assert!(store.decode(&key, frame(&invalid)).is_err());
    }
}

#[test]
fn compressed_headers_fail_closed_before_decode_allocation() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![8; 8192], false);
    let physical = store.encode(&key, canonical).unwrap();
    let mutations: [fn(&mut Header); 8] = [
        |h| h.version = 2,
        |h| h.codec = 2,
        |h| h.flags = 1,
        |h| h.canonical_len = u64::MAX,
        |h| h.canonical_len = 0,
        |h| h.canonical_len = MAX_CANONICAL_RECORD_BYTES as u64 + 1,
        |h| h.payload_len = u64::MAX,
        |h| h.payload_len = 0,
    ];
    for update in mutations {
        let mut damaged = physical.clone();
        change_header(&mut damaged, update);
        assert!(store.decode(&key, damaged).is_err());
        assert_eq!(store.budget.state.lock().unwrap().bytes, 0);
    }
    let mut oversized = vec![0; MAX_COMPRESSED_RECORD_BYTES + 1];
    oversized[..MAGIC.len()].copy_from_slice(&MAGIC);
    assert!(store.decode(&key, oversized).is_err());
}

#[test]
fn truncation_extra_input_and_corruption_never_fall_back_to_raw() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![8; 8192], false);
    let physical = store.encode(&key, canonical).unwrap();
    for end in 0..physical.len() {
        assert!(store.decode(&key, physical[..end].to_vec()).is_err());
    }
    let mut extra = physical.clone();
    extra.push(0);
    assert!(store.decode(&key, extra.clone()).is_err());
    change_header(&mut extra, |h| h.payload_len += 1);
    assert!(store.decode(&key, extra).is_err());
    let mut bad_digest = physical.clone();
    change_header(&mut bad_digest, |h| h.digest[0] ^= 1);
    assert!(store.decode(&key, bad_digest).is_err());
    let mut payload = physical;
    payload[HEADER_LEN] ^= 255;
    assert!(store.decode(&key, payload).is_err());
    assert_eq!(store.budget.state.lock().unwrap().bytes, 0);
}

#[test]
fn short_decode_cannot_succeed_with_zero_padding() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![0; 8192], false);
    let mut short = frame(&canonical[..canonical.len() - 8]);
    change_header(&mut short, |h| {
        h.canonical_len = canonical.len() as u64;
        h.digest = Blake2b::<U32>::digest(&canonical).into();
    });
    assert_eq!(store.decode(&key, short).unwrap_err(), invalid("decoded length mismatch"));
}

#[test]
fn digest_protects_kind_while_leaf_hash_protects_key() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![1; 8192], true);
    let mut other_kind = canonical.clone();
    other_kind[..4].copy_from_slice(&1u32.to_le_bytes());
    let mut physical = frame(&other_kind);
    change_header(&mut physical, |h| h.digest = Blake2b::<U32>::digest(&canonical).into());
    assert_eq!(store.decode(&key, physical).unwrap_err(), invalid("canonical digest mismatch"));
    let mut other_key = key;
    *other_key.last_mut().unwrap() ^= 1;
    assert_eq!(
        store.decode(&other_key, frame(&canonical)).unwrap_err(),
        invalid("leaf hash mismatch")
    );
}

#[test]
fn every_store_read_exposes_canonical_bytes_and_preserves_pages() {
    let store = codec(true);
    let pairs: Vec<_> = (0..9)
        .map(|i| fixture(i % 3, vec![i; 8192], i % 2 == 0))
        .collect();
    store.put(pairs.clone()).unwrap();
    let expected: BTreeMap<_, _> = pairs.iter().cloned().collect();
    assert_eq!(store.to_map().unwrap(), expected);
    assert_eq!(store.clone_box().to_map().unwrap(), expected);
    let keys = pairs.iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(store.contains(&keys).unwrap(), vec![true; pairs.len()]);
    assert!(store.non_empty().unwrap());
    assert!(store.size_bytes() < expected.iter().map(|(k, v)| k.len() + v.len()).sum());
    assert_eq!(store.size_bytes(), store.inner.size_bytes());
    assert!(store.as_any().downcast_ref::<LmdbKeyValueStore>().is_none());
    let mut visited = 0;
    store
        .iterate_while(&mut |key, value| {
            assert_eq!(expected.get(&key), Some(&value));
            visited += 1;
            Ok(false)
        })
        .unwrap();
    assert_eq!(visited, 1);
    store
        .iterate(|key, value| {
            validate_canonical(&key, &value).unwrap();
        })
        .unwrap();
    let mut cursor = None;
    let mut found = BTreeMap::new();
    loop {
        let physical_page = store.inner.scan_page(cursor.as_deref(), 2).unwrap();
        let page = store.scan_page(cursor.as_deref(), 2).unwrap();
        assert_eq!(page.next, physical_page.next);
        found.extend(page.rows);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(found, expected);
    assert!(store.scan_page(None, 0).is_err());
    assert!(store.scan_page(None, 1025).is_err());
    assert_eq!(store.delete(keys).unwrap(), pairs.len());
    assert!(!store.non_empty().unwrap());
}

#[test]
fn corruption_propagates_from_all_logical_reads_and_conditional_deletion() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![1; 8192], false);
    let mut physical = frame(&canonical);
    change_header(&mut physical, |h| h.codec = 255);
    store.inner.put_one(key.clone(), physical).unwrap();
    assert!(store.get_one(&key).is_err());
    assert!(store.contains(&vec![key.clone()]).is_err());
    assert!(store.to_map().is_err());
    assert!(store.scan_page(None, 1).is_err());
    assert!(store.iterate(|_, _| {}).is_err());
    assert!(store.iterate_while(&mut |_, _| Ok(true)).is_err());
    assert!(store.delete_if_value(&key, &canonical).is_err());
    assert!(store.put_if_absent(vec![(key.clone(), canonical)]).is_err());
    assert!(store.inner.get_one(&key).unwrap().is_some());
}

#[test]
fn mixed_writes_and_conditional_deletes_use_canonical_values() {
    let store = codec(true);
    let disabled = ColdStoreCodec::new(Arc::new(store.clone()), false);
    let (key, canonical) = fixture(2, vec![1; 8192], false);
    store.put_one(key.clone(), canonical.clone()).unwrap();
    assert_eq!(disabled.get_one(&key).unwrap(), Some(canonical.clone()));
    let physical = store.inner.get_one(&key).unwrap().unwrap();
    assert!(physical.starts_with(&MAGIC));
    assert!(!store.delete_if_value(&key, &physical).unwrap());
    assert!(
        !disabled
            .put_one_if_absent(key.clone(), canonical.clone())
            .unwrap()
    );
    assert_eq!(store.inner.get_one(&key).unwrap(), Some(physical));
    assert!(disabled.delete_if_value(&key, &canonical).unwrap());
    assert!(!disabled.delete_if_value(&key, &canonical).unwrap());
    disabled.put_one(key.clone(), canonical.clone()).unwrap();
    assert!(store.delete_if_value(&key, &canonical).unwrap());
    store
        .put_if_absent(vec![(key.clone(), canonical.clone()), (key.clone(), canonical.clone())])
        .unwrap();
    assert_eq!(store.get_one(&key).unwrap(), Some(canonical));
}

#[test]
fn invalid_batch_does_not_commit_earlier_valid_entries() {
    let store = codec(true);
    let valid = fixture(2, vec![4; 8192], false);
    let invalid = (vec![1; 32], vec![0; 25]);
    assert!(store.put(vec![valid.clone(), invalid.clone()]).is_err());
    assert!(!store.non_empty().unwrap());
    assert!(store.put_if_absent(vec![valid, invalid]).is_err());
    assert!(!store.non_empty().unwrap());
}

#[test]
fn concurrent_conditional_insertion_has_exactly_one_winner() {
    let store = Arc::new(codec(true));
    let barrier = Arc::new(Barrier::new(8));
    let (key, canonical) = fixture(2, vec![9; 8192], false);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    store
                        .put_one_if_absent(key.clone(), canonical.clone())
                        .unwrap()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| h.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
    });
    assert_eq!(store.get_one(&key).unwrap(), Some(canonical));
}

#[test]
fn budget_limits_and_clones_share_reservations() {
    let store = codec(true);
    let clone = store.clone_box();
    let clone = clone.as_any().downcast_ref::<ColdStoreCodec>().unwrap();
    assert!(Arc::ptr_eq(&store.budget, &clone.budget));
    let ensured = ColdStoreCodec::ensure(Arc::new(store.clone()));
    assert!(
        ensured
            .as_any()
            .downcast_ref::<ColdStoreCodec>()
            .unwrap()
            .compress_writes
    );
    let first = store
        .budget
        .acquire(MAX_SCRATCH_BYTES, false)
        .unwrap()
        .unwrap();
    assert!(clone.budget.acquire(1, false).unwrap().is_none());
    let (key, canonical) = fixture(2, vec![7; 8192], false);
    assert_eq!(store.encode(&key, canonical.clone()).unwrap(), canonical);
    drop(first);
    assert_eq!(store.budget.state.lock().unwrap().bytes, 0);
    let a = store.budget.acquire(1, false).unwrap().unwrap();
    let b = store.budget.acquire(1, false).unwrap().unwrap();
    assert!(clone.budget.acquire(1, false).unwrap().is_none());
    drop((a, b));
    assert!(clone.budget.acquire(MAX_SCRATCH_BYTES + 1, false).is_err());
}

#[test]
fn waiting_decoder_resumes_after_reservation_release() {
    let store = codec(true);
    let (key, canonical) = fixture(2, vec![9; 8192], false);
    let physical = frame(&canonical);
    let permit = store
        .budget
        .acquire(MAX_SCRATCH_BYTES, false)
        .unwrap()
        .unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| store.decode(&key, physical).unwrap());
        drop(permit);
        assert_eq!(worker.join().unwrap(), canonical);
    });
    assert_eq!(store.budget.state.lock().unwrap().active, 0);
}

struct ReplacementStore {
    inner: MemoryKeyValueStore,
    replacements: Mutex<Vec<ByteBuffer>>,
}

#[test]
fn lmdb_adapter_preserves_batches_pages_sizes_and_conditional_operations() {
    let dir = tempfile::tempdir().unwrap();
    let env = Arc::new(unsafe {
        heed::EnvOpenOptions::new()
            .map_size(32 * 1024 * 1024)
            .max_dbs(1)
            .open(dir.path())
            .unwrap()
    });
    let mut txn = env.write_txn().unwrap();
    let db = env.create_database(&mut txn, Some("cold")).unwrap();
    txn.commit().unwrap();
    let underlying = Arc::new(LmdbKeyValueStore::new(env.clone(), db));
    let store = ColdStoreCodec::new(underlying.clone(), true);
    let (key, canonical) = fixture(2, vec![13; 8192], true);
    shared::rust::store::lmdb_key_value_store::batched_put(vec![(&store, vec![(
        key.clone(),
        canonical.clone(),
    )])])
    .unwrap();
    assert!(
        underlying
            .get_one(&key)
            .unwrap()
            .unwrap()
            .starts_with(&MAGIC)
    );
    assert_eq!(store.get_one(&key).unwrap(), Some(canonical.clone()));
    assert_eq!(store.to_map().unwrap(), BTreeMap::from([(key.clone(), canonical.clone())]));
    assert_eq!(store.scan_page(None, 1).unwrap().rows, vec![(key.clone(), canonical.clone())]);
    assert_eq!(store.contains(&vec![key.clone(), vec![0; 32]]).unwrap(), vec![true, false]);
    assert!(
        !store
            .put_one_if_absent(key.clone(), canonical.clone())
            .unwrap()
    );
    assert!(!store.delete_if_value(&key, &vec![]).unwrap());
    {
        let txn = env.read_txn().unwrap();
        let stats = db.stat(&txn).unwrap();
        let expected = (stats.branch_pages + stats.leaf_pages + stats.overflow_pages) *
            stats.page_size as usize;
        drop(txn);
        assert!(expected > 0);
        assert_eq!(store.try_size_bytes().unwrap(), expected);
        assert_eq!(store.size_bytes(), expected);
    }
    store.print_store().unwrap();
    assert!(store.delete_if_value(&key, &canonical).unwrap());
    assert!(!store.delete_if_value(&key, &canonical).unwrap());
    let barrier = Barrier::new(4);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    store
                        .put_one_if_absent(key.clone(), canonical.clone())
                        .unwrap()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|h| h.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
    });
    assert_eq!(store.clone_box().get_one(&key).unwrap(), Some(canonical));
    assert_eq!(store.delete(vec![key]).unwrap(), 1);
    assert!(!store.non_empty().unwrap());
}

impl KeyValueStore for ReplacementStore {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.inner.get(keys)
    }
    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.inner.put(pairs)
    }
    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }
    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }
    fn delete_if_value(
        &self,
        key: &ByteBuffer,
        expected: &ByteBuffer,
    ) -> Result<bool, KvStoreError> {
        if let Some(replacement) = self.replacements.lock().unwrap().pop() {
            self.inner.put_one(key.clone(), replacement)?;
        }
        self.inner.delete_if_value(key, expected)
    }
    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }
    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }
    fn clone_box(&self) -> Box<dyn KeyValueStore> { panic!("unused by replacement-race test") }
    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }
    fn print_store(&self) -> Result<(), KvStoreError> { Ok(()) }
    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }
    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

#[test]
fn conditional_delete_retries_equivalent_representation_and_preserves_other_values() {
    let (key, canonical) = fixture(2, vec![5; 8192], false);
    for equivalent in [false, true] {
        let mut replacement = canonical.clone();
        if !equivalent {
            replacement[..4].copy_from_slice(&1u32.to_le_bytes());
        }
        let underlying = Arc::new(ReplacementStore {
            inner: MemoryKeyValueStore::new(),
            replacements: Mutex::new(vec![replacement.clone()]),
        });
        underlying.put_one(key.clone(), frame(&canonical)).unwrap();
        let store = ColdStoreCodec::new(underlying.clone(), false);
        assert_eq!(store.delete_if_value(&key, &canonical).unwrap(), equivalent);
        assert_eq!(underlying.get_one(&key).unwrap(), (!equivalent).then_some(replacement));
    }
}

#[test]
fn repeated_representation_races_return_contention_not_false_or_unconditional_delete() {
    let (key, canonical) = fixture(2, vec![5; 8192], false);
    let physical = frame(&canonical);
    let underlying = Arc::new(ReplacementStore {
        inner: MemoryKeyValueStore::new(),
        replacements: Mutex::new(
            (0..8)
                .rev()
                .map(|i| {
                    if i % 2 == 0 {
                        canonical.clone()
                    } else {
                        physical.clone()
                    }
                })
                .collect(),
        ),
    });
    underlying.put_one(key.clone(), physical).unwrap();
    let store = ColdStoreCodec::new(underlying.clone(), false);
    assert!(matches!(store.delete_if_value(&key, &canonical), Err(KvStoreError::LockError(_))));
    assert_eq!(store.get_one(&key).unwrap(), Some(canonical));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_canonical_leaves_roundtrip_exactly(
        kind in 0u8..3,
        payload in prop::collection::vec(any::<u8>(), 0..16384),
        serialized_key in any::<bool>(),
    ) {
        let store = codec(true);
        let (key, canonical) = fixture(kind, payload, serialized_key);
        let encoded = store.encode(&key, canonical.clone()).unwrap();
        prop_assert_eq!(store.decode(&key, encoded).unwrap(), canonical.clone());
        prop_assert_eq!(store.decode(&key, frame(&canonical)).unwrap(), canonical);
    }

    #[test]
    fn arbitrary_physical_records_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let store = codec(false);
        let _ = store.decode(&[0; 32], bytes);
        prop_assert_eq!(store.budget.state.lock().unwrap().bytes, 0);
    }

    #[test]
    fn malformed_lz4_blocks_never_panic_or_retain_budget(
        payload in prop::collection::vec(any::<u8>(), 0..4096),
        length in 12u64..8192,
    ) {
        let store = codec(false);
        let mut physical = bincode::serialize(&Header {
            magic: MAGIC, version: 1, codec: 1, flags: 0,
            canonical_len: length, payload_len: payload.len() as u64, digest: [0; 32],
        }).unwrap();
        physical.extend_from_slice(&payload);
        prop_assert!(store.decode(&[0; 32], physical).is_err());
        prop_assert_eq!(store.budget.state.lock().unwrap().bytes, 0);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn formal_store_model_agrees_with_operation_traces(
        steps in prop::collection::vec((0u8..5, 0u8..4, any::<bool>()), 1..48),
    ) {
        let backing = Arc::new(MemoryKeyValueStore::new());
        let mut logical = BTreeMap::new();
        for (operation, variant, compress) in steps {
            let store = ColdStoreCodec::new(backing.clone(), compress);
            let (key, value) = fixture(if variant % 2 == 0 { 1 } else { 2 }, vec![variant / 2; 4096], false);
            match operation {
                0 | 1 => {
                    store.put_one(key.clone(), value.clone()).unwrap();
                    logical.insert(key, value);
                }
                2 => {
                    let expected = !logical.contains_key(&key);
                    prop_assert_eq!(store.put_one_if_absent(key.clone(), value.clone()).unwrap(), expected);
                    logical.entry(key).or_insert(value);
                }
                3 => {
                    let expected = logical.get(&key) == Some(&value);
                    prop_assert_eq!(store.delete_if_value(&key, &value).unwrap(), expected);
                    if expected { logical.remove(&key); }
                }
                _ => {
                    let actual = store.get_one(&key).unwrap();
                    prop_assert_eq!(actual.as_ref(), logical.get(&key));
                }
            }
            prop_assert_eq!(store.to_map().unwrap(), logical.clone());
        }
    }

    #[test]
    fn formal_budget_model_agrees_with_reservation_traces(
        steps in prop::collection::vec((any::<bool>(), 0usize..4, 1usize..1537), 1..96),
    ) {
        let budget = CodecBudget::new(2, 1024);
        let mut permits: Vec<Option<CodecPermit<'_>>> = (0..4).map(|_| None).collect();
        let mut sizes = [0; 4];
        for (acquire, index, bytes) in steps {
            if acquire && permits[index].is_none() {
                let active = sizes.iter().filter(|&&n| n > 0).count();
                let total: usize = sizes.iter().sum();
                let result = budget.acquire(bytes, false);
                if bytes > 1024 {
                    prop_assert!(result.is_err());
                } else {
                    let permit = result.unwrap();
                    let admitted = active < 2 && total + bytes <= 1024;
                    prop_assert_eq!(permit.is_some(), admitted);
                    if admitted { sizes[index] = bytes; }
                    permits[index] = permit;
                }
            } else if !acquire {
                permits[index] = None;
                sizes[index] = 0;
            }
            let state = budget.state.lock().unwrap();
            prop_assert_eq!(state.bytes, sizes.iter().sum::<usize>());
            prop_assert_eq!(state.active, sizes.iter().filter(|&&n| n > 0).count());
            prop_assert!(state.bytes <= 1024 && state.active <= 2);
        }
        drop(permits);
        prop_assert_eq!(budget.state.lock().unwrap().bytes, 0);
        prop_assert_eq!(budget.state.lock().unwrap().active, 0);
    }
}
