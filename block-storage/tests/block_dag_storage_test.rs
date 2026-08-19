// See block-storage/src/test/scala/coop/rchain/blockstorage/dag/BlockDagStorageTest.scala
// See block-storage/src/test/scala/coop/rchain/blockstorage/dag/BlockDagKeyValueStorageTest.scala

use std::collections::{BTreeSet, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::{
    block_element_gen, block_elements_with_parents_gen, block_hash_gen, block_with_new_hashes_gen,
    get_random_block, validator_gen,
};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::equivocation_record::EquivocationRecord;
use models::rust::validator::Validator;
use once_cell::sync::Lazy;
use proptest::prelude::ProptestConfig;
use proptest::proptest;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use tokio::runtime::Runtime;

fn init_logger() { shared::rust::tracing_init::init_for_tests(); }

fn genesis_block() -> BlockMessage {
    get_random_block(
        Some(0),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
    )
}

async fn create_dag_storage(genesis: &BlockMessage) -> BlockDagKeyValueStorage {
    let mut kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    dag_storage
        .insert(
            genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
        )
        .unwrap();
    dag_storage
}

fn proptest_config() -> ProptestConfig {
    init_logger();

    ProptestConfig {
        cases: 5,
        max_shrink_iters: 5,
        ..ProptestConfig::default()
    }
}

static RUNTIME: Lazy<Runtime> = Lazy::new(|| Runtime::new().unwrap());

struct BlockLookup {
    block_metadata: Option<BlockMetadata>,
    latest_message_hash: Option<BlockHash>,
    latest_message: Option<BlockMetadata>,
    children: Option<imbl::HashSet<BlockHash>>,
    contains: bool,
}

struct LookupResult {
    list: Vec<BlockLookup>,
    latest_message_hashes: imbl::HashMap<Validator, BlockHash>,
    latest_messages: HashMap<Validator, BlockMetadata>,
    topo_sort: Vec<Vec<BlockHash>>,
    latest_block_number: i64,
}

fn lookup_elements(
    block_elements: &[BlockMessage],
    dag_storage: &BlockDagKeyValueStorage,
    topo_sort_start_block_number: Option<i64>,
) -> LookupResult {
    let topo_sort_start_block_number = topo_sort_start_block_number.unwrap_or(0);
    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    let list: Vec<BlockLookup> = block_elements
        .iter()
        .map(|block_element| {
            let block_metadata = dag.lookup(&block_element.block_hash).unwrap();
            let latest_message_hash = dag.latest_message_hash(&block_element.sender);
            let latest_message = dag.latest_message(&block_element.sender).unwrap();
            let children = dag.children(&block_element.block_hash);
            let contains = dag.contains(&block_element.block_hash);
            BlockLookup {
                block_metadata,
                latest_message_hash,
                latest_message,
                children,
                contains,
            }
        })
        .collect();

    let latest_message_hashes = dag.latest_message_hashes();
    let latest_messages = dag.latest_messages().unwrap();
    let topo_sort = dag.topo_sort(topo_sort_start_block_number, None).unwrap();
    let latest_block_number = dag.latest_block_number();
    LookupResult {
        list,
        latest_message_hashes,
        latest_messages,
        topo_sort,
        latest_block_number,
    }
}

fn test_lookup_elements_result(
    lookup_result: &LookupResult,
    block_elements: &[BlockMessage],
    genesis: &BlockMessage,
) {
    let LookupResult {
        list,
        latest_message_hashes,
        latest_messages,
        topo_sort,
        latest_block_number,
    } = lookup_result;

    let real_latest_messages =
        block_elements
            .iter()
            .fold(HashMap::new(), |mut acc, block_element| {
                if !block_element.sender.is_empty() {
                    acc.insert(
                        block_element.sender.clone(),
                        BlockMetadata::from_block(block_element, false, None, None),
                    );
                }
                acc
            });

    list.iter()
        .zip(block_elements.iter())
        .for_each(|(block_lookup, block_element)| {
            let BlockLookup {
                block_metadata,
                latest_message_hash,
                latest_message,
                children,
                contains,
            } = block_lookup;
            assert_eq!(
                *block_metadata,
                Some(BlockMetadata::from_block(block_element, false, None, None))
            );

            assert_eq!(
                *latest_message_hash,
                real_latest_messages
                    .get(&block_element.sender)
                    .map(|metadata| metadata.block_hash.clone())
            );

            assert_eq!(
                *latest_message,
                real_latest_messages.get(&block_element.sender).cloned()
            );

            let children_set = children.as_ref().map(|dash_set| {
                let mut set = HashSet::new();
                for item in dash_set.iter() {
                    set.insert(item.clone());
                }
                set
            });

            let expected_children: HashSet<BlockHash> = block_elements
                .iter()
                .filter(|b| {
                    b.header
                        .parents_hash_list
                        .contains(&block_element.block_hash)
                })
                .map(|b| b.block_hash.clone())
                .collect();

            assert_eq!(children_set, Some(expected_children));
            assert!(*contains);
        });

    let filtered_latest_message_hashes: HashMap<_, _> = latest_message_hashes
        .iter()
        .filter(|(_, hash)| **hash != genesis.block_hash)
        .map(|(validator, hash)| (validator.clone(), hash.clone()))
        .collect();

    let expected_latest_message_hashes: HashMap<_, _> = real_latest_messages
        .iter()
        .map(|(validator, metadata)| (validator.clone(), metadata.block_hash.clone()))
        .collect();

    assert_eq!(
        filtered_latest_message_hashes,
        expected_latest_message_hashes
    );

    let filtered_latest_messages: HashMap<_, _> = latest_messages
        .iter()
        .filter(|(_, metadata)| metadata.block_hash != genesis.block_hash)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    assert_eq!(filtered_latest_messages, real_latest_messages);

    // Verify topo sort
    fn normalize(topo_sort: &[Vec<BlockHash>]) -> Vec<Vec<BlockHash>> {
        if topo_sort.len() == 1 && topo_sort[0].is_empty() {
            Vec::new()
        } else {
            topo_sort.to_vec()
        }
    }

    let real_topo_sort = normalize(&[block_elements
        .iter()
        .map(|b| b.block_hash.clone())
        .collect::<Vec<_>>()]);

    assert_eq!(topo_sort.len(), real_topo_sort.len());

    for (topo_sort_level, real_topo_sort_level) in topo_sort.iter().zip(real_topo_sort.iter()) {
        let topo_sort_set: HashSet<BlockHash> = topo_sort_level
            .iter()
            .filter(|&hash| *hash != genesis.block_hash)
            .cloned()
            .collect();

        let real_topo_sort_set: HashSet<BlockHash> = real_topo_sort_level.iter().cloned().collect();

        assert_eq!(topo_sort_set, real_topo_sort_set);
    }

    assert_eq!(*latest_block_number, topo_sort.len() as i64);
}

#[test]
fn dag_storage_should_be_able_to_lookup_a_stored_block() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

      for block_element in &block_elements {
        dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
      }

      let dag = dag_storage.get_representation().expect("dag representation");

      let block_element_lookups = block_elements.iter().map(|block_element| {
        let block_metadata = dag.lookup(&block_element.block_hash).unwrap();
        let latest_message_hash = dag.latest_message_hash(&block_element.sender);
        let latest_message = dag.latest_message(&block_element.sender).unwrap();
        (block_metadata, latest_message_hash, latest_message)
      });

      let latest_message_hashes = dag.latest_message_hashes();
      let latest_messages = dag.latest_messages().unwrap();

      block_element_lookups.zip(block_elements.clone()).for_each(|((block_metadata, latest_message_hash, latest_message), block_element)| {
        assert_eq!(block_metadata, Some(BlockMetadata::from_block(&block_element, false, None, None)));
        assert_eq!(latest_message_hash, Some(block_element.block_hash.clone()));
        assert_eq!(latest_message, Some(BlockMetadata::from_block(&block_element, false, None, None)));
      });

      assert_eq!(latest_message_hashes.len(), block_elements.len() + 1);
      assert_eq!(latest_messages.len(), block_elements.len() + 1);
    });
}

// GAP-2 / GAP-4 (FV precondition): `is_dag_ancestor` is the trusted ancestry primitive
// that the FORMALLY-VERIFIED finalized-floor (`casper/finality/floor.rs`) and the safety
// clique oracle (`casper/safety/clique_oracle.rs`) both ASSUME — the Rocq
// (`CliqueOracle.v`/`Selection.v`/`Foundation.v`) models ancestry only ABSTRACTLY as
// `anc_of`. Its block-number prune (`block_number(current) <= stop_height => stop
// descending`) is sound only when block numbers are strictly monotone along parent
// edges (`wf_dag`) — a precondition BLOCK VALIDATION enforces (`block_number = 1 + max
// parent number`, mirrored by GuardBridge's `validated_block`) and that
// `block_elements_with_parents_gen` respects. This test discharges the trusted-primitive
// gap: on random well-formed DAGs, `is_dag_ancestor` (WITH the prune) computes EXACTLY
// the reflexive-transitive closure over parents — the abstract `anc_of` relation the
// proofs reason about. (The demoted height-map contiguity `assert!` at
// `block_metadata_store.rs:388` is a separate, stronger diagnostic — monotonicity, not
// contiguity, is the property the prune relies on, and it is what this test exercises.)
#[test]
fn is_dag_ancestor_matches_reflexive_transitive_closure_over_parents() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
      for be in &block_elements {
        dag_storage.insert(be, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).expect("insert block element");
      }
      let dag = dag_storage.get_representation().expect("dag representation");

      // Ground-truth parent map from the block metadata (genesis has no parents).
      let mut parents: std::collections::HashMap<prost::bytes::Bytes, Vec<prost::bytes::Bytes>> =
          std::collections::HashMap::new();
      parents.insert(genesis.block_hash.clone(), Vec::new());
      for be in &block_elements {
        let meta = BlockMetadata::from_block(be, false, None, None);
        parents.insert(meta.block_hash.clone(), meta.parents.clone());
      }

      // Reference: reflexive-transitive closure over parents, with NO prune.
      let reachable = |anc: &prost::bytes::Bytes, desc: &prost::bytes::Bytes| -> bool {
        if anc == desc { return true; }
        let mut stack = vec![desc.clone()];
        let mut seen: std::collections::HashSet<prost::bytes::Bytes> = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
          if !seen.insert(cur.clone()) { continue; }
          if cur == *anc { return true; }
          if let Some(ps) = parents.get(&cur) { for p in ps { stack.push(p.clone()); } }
        }
        false
      };

      let all: Vec<prost::bytes::Bytes> = std::iter::once(genesis.block_hash.clone())
          .chain(block_elements.iter().map(|b| b.block_hash.clone()))
          .collect();
      for a in &all {
        for b in &all {
          let got = dag.is_dag_ancestor(a, b).expect("is_dag_ancestor");
          let want = reachable(a, b);
          assert_eq!(got, want,
            "is_dag_ancestor mismatch (prune unsound?): a={:?} b={:?} got={} closure={}",
            a, b, got, want);
        }
      }
    });
}

#[test]
fn dag_storage_should_be_able_to_handle_checking_if_contains_a_block_with_empty_hash() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    let contains = dag.contains(&prost::bytes::Bytes::new());
    assert!(!contains);
}

#[test]
fn dag_storage_should_be_able_to_restore_state_on_startup() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

      for block_element in &block_elements {
        dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
      }

      let result = lookup_elements(&block_elements, &dag_storage, None);
      test_lookup_elements_result(&result, &block_elements, &genesis);
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_latest_messages_with_genesis_with_empty_sender_field() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

      let mut block_elements_with_genesis = block_elements.clone();
      if let Some(first) = block_elements_with_genesis.first_mut() {
          first.sender = prost::bytes::Bytes::new();
      }

      for block_element in &block_elements_with_genesis {
        dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
      }

      let result = lookup_elements(&block_elements_with_genesis, &dag_storage, None);
      test_lookup_elements_result(&result, &block_elements_with_genesis, &genesis);
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_state_from_the_previous_two_instances() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(first_block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10),
      second_block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

        for block_element in &first_block_elements {
            dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
        }

        for block_element in &second_block_elements {
            dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
        }

        let mut all_block_elements = first_block_elements.clone();
        all_block_elements.extend(second_block_elements.clone());
        let result = lookup_elements(&all_block_elements, &dag_storage, None);
        test_lookup_elements_result(&result, &all_block_elements, &genesis);
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_after_squashing_latest_messages() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
        proptest!(proptest_config(), |(
            second_block_elements in block_with_new_hashes_gen(block_elements.clone()),
            third_block_elements in block_with_new_hashes_gen(block_elements.clone())
        )| {
            let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

            for block_element in &block_elements {
                dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
            }

            for block_element in &second_block_elements {
                dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
            }

            for block_element in &third_block_elements {
                dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
            }

            let mut all_block_elements = block_elements.clone();
            all_block_elements.extend(second_block_elements.clone());
            all_block_elements.extend(third_block_elements.clone());

            let result = lookup_elements(&block_elements, &dag_storage, None);
            test_lookup_elements_result(&result, &all_block_elements, &genesis);
        });
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_equivocations_tracker_on_startup() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10),
      equivocator in validator_gen(), block_hash in block_hash_gen())| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

        for block_element in &block_elements {
            dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
        }

        let equivocation_record = EquivocationRecord::new(equivocator, 0, BTreeSet::from([block_hash]));
        dag_storage.insert_equivocation_record(equivocation_record.clone()).unwrap();

        let records = dag_storage.equivocation_records().unwrap();
        assert_eq!(records, HashSet::from([equivocation_record]));

        let result = lookup_elements(&block_elements, &dag_storage, None);
        test_lookup_elements_result(&result, &block_elements, &genesis);

    });
}

#[test]
fn dag_storage_should_be_able_to_modify_equivocation_records() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(equivocator in validator_gen(), block_hash1 in block_hash_gen(),
      block_hash2 in block_hash_gen())| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

        let equivocation_record = EquivocationRecord::new(equivocator.clone(), 0, BTreeSet::from([block_hash1.clone()]));
        dag_storage.insert_equivocation_record(equivocation_record.clone()).unwrap();

        dag_storage.update_equivocation_record(equivocation_record, block_hash2.clone()).unwrap();

        let updated_equivocation_record = EquivocationRecord::new(equivocator, 0, BTreeSet::from([block_hash1, block_hash2]));
        let records = dag_storage.equivocation_records().unwrap();
        assert_eq!(records, HashSet::from([updated_equivocation_record]));
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_invalid_blocks_on_startup() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

      for block_element in &block_elements {
        dag_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid).unwrap();
      }

      let dag = dag_storage.get_representation().expect("dag representation");
      let invalid_blocks = dag.invalid_blocks();
      let invalid_blocks_set: HashSet<_> = invalid_blocks.iter().cloned().collect();
      assert_eq!(invalid_blocks_set, block_elements.into_iter().map(|b| BlockMetadata::from_block(&b, true, None, None)).collect::<HashSet<_>>());
    });
}

#[test]
fn dag_storage_should_advance_latest_message_to_invalid_block_from_same_sender() {
    // Inserting an invalid block with an advancing sequence number updates the
    // sender's latest message. Required for equivocation detection via
    // `invalid_latest_messages` to fire on validators that have a prior valid
    // block.
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

    let valid_block = get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &valid_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let invalid_block = get_random_block(
        Some(2),
        Some(valid_block.seq_num + 1),
        None,
        None,
        Some(valid_block.sender.clone()),
        None,
        None,
        Some(vec![valid_block.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &invalid_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
        )
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(
        dag.latest_message_hash(&valid_block.sender),
        Some(invalid_block.block_hash.clone())
    );

    let invalid_latest_messages = dag.invalid_latest_messages().unwrap();
    assert_eq!(
        invalid_latest_messages.get(&valid_block.sender),
        Some(&invalid_block.block_hash)
    );
}

/// Inserting an OLDER block by a sender must not move that sender's latest
/// message backward. Settled-history admission inserts old blocks at runtime
/// — a straggler a joiner's restore missed, arriving while the sender's real
/// latest message is a hundred blocks ahead — so the sequence-monotone guard
/// on the latest-message update is what keeps admission from rewriting any
/// validator's position. (Insertion ORDER is an optimization, not what this
/// depends on.)
#[test]
fn dag_storage_keeps_latest_message_when_an_older_block_arrives() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

    let newer = get_random_block(
        Some(5),
        Some(5),
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &newer,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let older = get_random_block(
        Some(2),
        Some(2),
        None,
        None,
        Some(newer.sender.clone()),
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &older,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(
        dag.latest_message_hash(&newer.sender),
        Some(newer.block_hash.clone()),
        "an old block arriving late must not regress its sender's latest message"
    );
}

/// Every deploy in a VALID inserted body resolves to its carrier; invalid
/// bodies are not canonical history and resolve to nothing.
#[test]
fn deploy_appearance_resolves_valid_bodies_and_ignores_invalid_ones() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      for mode in [
          block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
          block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
      ] {
          let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));

          for block_element in &block_elements {
            dag_storage.insert(block_element, mode).unwrap();
          }

          let dag = dag_storage.get_representation().expect("dag representation");
          let mut deploy_sigs = Vec::new();
          let mut block_hashes = Vec::new();

          for block in &block_elements {
              for deploy in &block.body.deploys {
                  deploy_sigs.push(deploy.deploy.sig.clone());
                  block_hashes.push(block.block_hash.clone());
              }
          }

          let deploy_lookups: Vec<Option<BlockHash>> = deploy_sigs
              .iter()
              .map(|sig| dag.deploy_canonical_appearance(sig).unwrap())
              .collect();

          let expected: Vec<Option<BlockHash>> = match mode {
              block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid =>
                  vec![None; deploy_sigs.len()],
              _ => block_hashes.iter().map(|h| Some(h.clone())).collect(),
          };
          assert_eq!(deploy_lookups, expected, "mode {:?}", mode);
      }
    });
}

#[test]
fn dag_storage_should_be_able_to_handle_blocks_with_invalid_numbers() {
    proptest!(proptest_config(), |(genesis in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None),
      block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None))| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
        let mut invalid_block = block.clone();
        invalid_block.body.state.block_number = 1000;
        dag_storage.insert(&genesis, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
        dag_storage.insert(&invalid_block, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid).unwrap();
    });
}

#[tokio::test]
async fn recording_of_new_directly_finalized_block_should_record_finalized_all_non_finalized_ancestors_of_lfb(
) {
    let genesis = genesis_block();
    let dag_storage = create_dag_storage(&genesis).await;
    dag_storage
        .insert(
            &genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
        )
        .unwrap();

    let b1 = get_random_block(
        Some(1),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &b1,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let b2 = get_random_block(
        Some(2),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![b1.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &b2,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let b3 = get_random_block(
        Some(3),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![b2.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &b3,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let b4 = get_random_block(
        Some(4),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![b3.block_hash.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let dag = dag_storage
        .insert(
            &b4,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    assert!(dag.lookup_unsafe(&genesis.block_hash).unwrap().finalized);
    assert!(dag.is_finalized(&genesis.block_hash));

    assert!(!dag.lookup_unsafe(&b1.block_hash).unwrap().finalized);
    assert!(!dag.is_finalized(&b1.block_hash));

    assert!(!dag.lookup_unsafe(&b2.block_hash).unwrap().finalized);
    assert!(!dag.is_finalized(&b2.block_hash));

    assert!(!dag.lookup_unsafe(&b3.block_hash).unwrap().finalized);
    assert!(!dag.is_finalized(&b3.block_hash));

    assert!(!dag.lookup_unsafe(&b4.block_hash).unwrap().finalized);
    assert!(!dag.is_finalized(&b4.block_hash));

    let effects = std::sync::Arc::new(std::sync::Mutex::new(HashSet::new()));
    let effects_clone = effects.clone();
    dag_storage
        .record_directly_finalized(
            b3.block_hash.clone(),
            1.0,
            move |blocks: &HashSet<BlockHash>| {
                let blocks = blocks.clone();
                let effects_clone = effects_clone.clone();
                Box::pin(async move {
                    let mut effects_guard = effects_clone.lock().unwrap();
                    effects_guard.extend(blocks.iter().cloned());
                    Ok(())
                })
            },
        )
        .await
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(dag.last_finalized_block(), b3.block_hash);
    assert!(dag.is_finalized(&b1.block_hash));
    assert!(dag.is_finalized(&b2.block_hash));
    assert!(dag.is_finalized(&b3.block_hash));
    assert!(!dag.is_finalized(&b4.block_hash));

    let b1_meta = dag.lookup_unsafe(&b1.block_hash).unwrap();
    assert!(b1_meta.finalized);
    assert!(!b1_meta.directly_finalized);

    let b2_meta = dag.lookup_unsafe(&b2.block_hash).unwrap();
    assert!(b2_meta.finalized);
    assert!(!b2_meta.directly_finalized);

    let b3_meta = dag.lookup_unsafe(&b3.block_hash).unwrap();
    assert!(b3_meta.finalized);
    assert!(b3_meta.directly_finalized);

    let b4_meta = dag.lookup_unsafe(&b4.block_hash).unwrap();
    assert!(!b4_meta.finalized);
    assert!(!b4_meta.directly_finalized);

    // Check that all finalized blocks were captured in the effects
    let finalized_effects = effects.lock().unwrap();
    let expected_effects = HashSet::from([
        b1.block_hash.clone(),
        b2.block_hash.clone(),
        b3.block_hash.clone(),
    ]);
    assert_eq!(*finalized_effects, expected_effects);
}

#[test]
fn find_returns_some_for_valid_even_length_truncated_hash() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        let full_hex = hex::encode(&*genesis.block_hash);
        let prefix = &full_hex[..6];
        match dag.find(prefix) {
            Ok(Some(found)) => assert_eq!(found, genesis.block_hash),
            Ok(None) => panic!("find() returned None for known prefix {prefix}"),
            Err(e) => panic!("find() returned Err for valid hex prefix: {e:?}"),
        }
    });
}

#[test]
fn find_returns_some_for_valid_odd_length_truncated_hash() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        let full_hex = hex::encode(&*genesis.block_hash);
        let prefix = &full_hex[..5];
        match dag.find(prefix) {
            Ok(Some(found)) => assert_eq!(found, genesis.block_hash),
            Ok(None) => panic!("find() returned None for known odd prefix {prefix}"),
            Err(e) => panic!("find() returned Err for valid odd hex prefix: {e:?}"),
        }
    });
}

#[test]
fn find_returns_err_for_invalid_hex_input() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        match dag.find("zzzz") {
            Err(_) => {}
            Ok(other) => panic!("find() should return Err for non-hex input, got {other:?}"),
        }
        match dag.find("zzzzz") {
            Err(_) => {}
            Ok(other) => {
                panic!("find() should return Err for odd-length non-hex input, got {other:?}")
            }
        }
    });
}

#[test]
fn find_returns_ok_none_for_unknown_valid_prefix() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        // "deadbeef" is a valid hex string but extremely unlikely to match
        // the genesis hash prefix.
        match dag.find("deadbeef") {
            Ok(None) => {}
            Ok(Some(h)) => {
                // Allow the (cosmologically improbable) case where the genesis
                // hash actually starts with deadbeef.
                assert!(hex::encode(&*h).starts_with("deadbeef"));
            }
            Err(e) => panic!("find() returned Err for valid hex prefix: {e:?}"),
        }
    });
}

/// A re-included sig's canonical appearance is a function of the DAG, not
/// of node-local insertion order: whichever order the two carriers arrive
/// in, the answer is the latest inclusion by (height, hash).
#[test]
fn deploy_appearance_is_insertion_order_independent() {
    use models::rust::block_implicits::processed_deploy_gen;
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    init_logger();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut runner = TestRunner::default();
        let deploy = processed_deploy_gen()
            .new_tree(&mut runner)
            .unwrap()
            .current();

        for reversed in [false, true] {
            let genesis = genesis_block();
            let dag_storage = create_dag_storage(&genesis).await;
            let mk = |height: i64| {
                get_random_block(
                    Some(height),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(vec![genesis.block_hash.clone()]),
                    None,
                    Some(vec![deploy.clone()]),
                    None,
                    Some(vec![]),
                    None,
                    None,
                )
            };
            let early = mk(1);
            let late = mk(2);
            let order = if reversed {
                [&late, &early]
            } else {
                [&early, &late]
            };
            for b in order {
                dag_storage
                    .insert(
                        b,
                        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
                    )
                    .expect("insert carrier");
            }
            let dag = dag_storage
                .get_representation()
                .expect("dag representation");
            assert_eq!(
                dag.deploy_canonical_appearance(&deploy.deploy.sig)
                    .expect("appearance lookup"),
                Some(late.block_hash.clone()),
                "reversed={}: the canonical appearance is the latest \
                 inclusion by (height, hash), independent of which carrier \
                 this node inserted first",
                reversed
            );
        }
    });
}

/// An appearance is a block that CARRIES the deploy. A rejection record
/// event at a greater height than the inclusion must not become the
/// canonical appearance: the record's block does not hold the deploy, and
/// naming it sends every consumer that fetches the block looking for a
/// deploy that is not in its deploy list.
#[test]
fn canonical_appearance_is_the_latest_inclusion_never_a_record_carrier() {
    use models::rust::block_implicits::processed_deploy_gen;
    use models::rust::casper::protocol::casper_message::RejectedDeploy;
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    init_logger();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut runner = TestRunner::default();
        let deploy = processed_deploy_gen()
            .new_tree(&mut runner)
            .unwrap()
            .current();

        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;

        let inclusion = get_random_block(
            Some(1),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![genesis.block_hash.clone()]),
            None,
            Some(vec![deploy.clone()]),
            None,
            Some(vec![]),
            None,
            None,
        );
        let mut rejecting_merge = get_random_block(
            Some(2),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![inclusion.block_hash.clone()]),
            None,
            Some(vec![]),
            None,
            Some(vec![]),
            None,
            None,
        );
        rejecting_merge.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy.deploy.sig.clone(),
            duplicate: false,
            carrier: inclusion.block_hash.clone(),
        }];

        for b in [&inclusion, &rejecting_merge] {
            dag_storage
                .insert(
                    b,
                    block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
                )
                .expect("insert block");
        }

        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        assert_eq!(
            dag.deploy_canonical_appearance(&deploy.deploy.sig)
                .expect("appearance lookup"),
            Some(inclusion.block_hash.clone()),
            "the record event at height 2 outranks the inclusion by height, \
             but its block does not carry the deploy — the appearance must \
             stay on the inclusion carrier"
        );
    });
}

/// The lifecycle event ingest rides `insert`'s body pass: a valid block's
/// executions and records project into per-sig rows; an invalid block's
/// body contributes nothing (it is not canonical history).
#[test]
fn insert_projects_lifecycle_events_from_valid_bodies_only() {
    use models::rust::block_implicits::processed_deploy_gen;
    use models::rust::casper::protocol::casper_message::RejectedDeploy;
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    init_logger();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;

        let mut runner = TestRunner::default();
        let executed = processed_deploy_gen()
            .new_tree(&mut runner)
            .unwrap()
            .current();
        let rejected_sig = prost::bytes::Bytes::from(vec![0xAA; 70]);
        let carrier = prost::bytes::Bytes::from(vec![0xBB; 32]);

        let mut valid_block = get_random_block(
            Some(1),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![genesis.block_hash.clone()]),
            None,
            Some(vec![executed.clone()]),
            None,
            Some(vec![]),
            None,
            None,
        );
        valid_block.body.rejected_deploys = vec![RejectedDeploy {
            sig: rejected_sig.clone(),
            duplicate: true,
            carrier: carrier.clone(),
        }];
        dag_storage
            .insert(
                &valid_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .expect("insert valid block");

        let invalid_deploy = processed_deploy_gen()
            .new_tree(&mut runner)
            .unwrap()
            .current();
        let invalid_block = get_random_block(
            Some(1),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![genesis.block_hash.clone()]),
            None,
            Some(vec![invalid_deploy.clone()]),
            None,
            Some(vec![]),
            None,
            None,
        );
        dag_storage
            .insert(
                &invalid_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
            )
            .expect("insert invalid block");

        let dag = dag_storage
            .get_representation()
            .expect("dag representation");

        let included_row = dag
            .deploy_lifecycle_events(&executed.deploy.sig)
            .expect("read row")
            .expect("executed deploy has a row");
        assert_eq!(
            included_row.valid_after,
            Some(executed.deploy.data.valid_after_block_number),
            "the first inclusion records the deploy's window start"
        );
        assert!(
            matches!(
                included_row.events.as_slice(),
                [block_storage::rust::dag::deploy_lifecycle_types::LifecycleEvent {
                    height: 1,
                    kind: block_storage::rust::dag::deploy_lifecycle_types::LifecycleEventKind::Included { is_failed: false },
                    ..
                }]
            ),
            "one Included event at the block's height; got {:?}",
            included_row.events
        );

        let rejected_row = dag
            .deploy_lifecycle_events(&rejected_sig)
            .expect("read row")
            .expect("rejected sig has a row");
        assert!(
            matches!(
                rejected_row.events.as_slice(),
                [block_storage::rust::dag::deploy_lifecycle_types::LifecycleEvent {
                    kind: block_storage::rust::dag::deploy_lifecycle_types::LifecycleEventKind::Rejected { duplicate: true, .. },
                    ..
                }]
            ),
            "one Rejected event carrying the record's duplicate flag; got {:?}",
            rejected_row.events
        );

        assert!(
            dag.deploy_lifecycle_events(&invalid_deploy.deploy.sig)
                .expect("read row")
                .is_none(),
            "an invalid block's body must contribute no lifecycle events"
        );
    });
}

/// A restored (truncated) DAG holds a window of blocks whose deepest
/// parent references point below the truncation boundary, and LFS
/// populate marks only the ANCHOR finalized — the window itself, the
/// anchor's own ancestry included, carries no finality marks. Adopting a
/// new LFB above the anchor must therefore walk unmarked window branches,
/// and that walk must treat an unheld parent as the horizon (everything
/// below the boundary is below the anchor's floor, i.e. settled), never
/// as an error: erroring aborts the adoption and wedges the finalizer
/// forever while the chain grows past it.
#[test]
fn truncated_window_finalization_walk_terminates_at_the_horizon() {
    init_logger();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

        // Never inserted: the parent reference below the truncation boundary.
        let below_boundary = get_random_block(
            Some(2),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![]),
            None,
            None,
            None,
            Some(vec![]),
            None,
            None,
        );

        let make = |number: i64, parents: Vec<BlockHash>| {
            get_random_block(
                Some(number),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(parents),
                None,
                None,
                None,
                Some(vec![]),
                None,
                None,
            )
        };
        let b3 = make(3, vec![below_boundary.block_hash.clone()]);
        let b4 = make(4, vec![b3.block_hash.clone()]);
        let anchor = make(5, vec![b4.block_hash.clone()]);
        // Multi-parent: the finalized anchor plus an unmarked window block,
        // so the marking walk cannot stop at the anchor alone.
        let b6 = make(6, vec![anchor.block_hash.clone(), b4.block_hash.clone()]);

        // LFS populate order: anchor (Approved — the only finality mark),
        // then the window, newest to oldest, then post-restore admission.
        let mode_approved =
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved;
        let mode_normal = block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal;
        dag_storage.insert(&anchor, mode_approved).unwrap();
        dag_storage.insert(&b4, mode_normal).unwrap();
        dag_storage.insert(&b3, mode_normal).unwrap();
        dag_storage.insert(&b6, mode_normal).unwrap();

        let result = dag_storage
            .record_directly_finalized(b6.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await;
        assert!(
            result.is_ok(),
            "adopting an LFB above a truncated window must terminate the \
             finalized-ancestry marking walk at the horizon, not abort on the \
             unheld parent: {:?}",
            result.err()
        );

        let dag = dag_storage.get_representation().unwrap();
        assert_eq!(
            dag.last_finalized_block(),
            b6.block_hash,
            "the adoption must land: the LFB pointer moves to the new block"
        );
        for (name, hash) in [
            ("b6", &b6.block_hash),
            ("b4", &b4.block_hash),
            ("b3", &b3.block_hash),
        ] {
            assert!(
                dag.is_finalized(hash),
                "{name} is held ancestry of the adopted LFB and must be marked finalized"
            );
        }
        assert!(
            !dag.contains(&below_boundary.block_hash),
            "the below-boundary reference stays unheld: the walk terminates \
             there without inventing an entry for it"
        );
    });
}

/// The newly-bonded latest-message placeholder must be network-uniform:
/// every node seeds the same slot with the same value, or the joiner's
/// first self-justifying proposal reads as an equivocation on whichever
/// side seeded differently. Ceremony nodes derive genesis from their
/// height-0 block; a truncated node holds no height-0 block and must use
/// the LEARNED genesis hash — never the block that happens to be inserted
/// at seeding time, which is right on no node.
#[test]
fn newly_bonded_placeholder_is_the_learned_genesis_on_a_truncated_dag() {
    init_logger();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

        let make = |number: i64, parents: Vec<BlockHash>| {
            get_random_block(
                Some(number),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(parents),
                None,
                None,
                None,
                Some(vec![]),
                None,
                None,
            )
        };

        // Truncated window: the anchor's own parent is never inserted, and
        // no height-0 block exists anywhere in this DAG.
        let below_boundary = make(4, vec![]);
        let anchor = make(5, vec![below_boundary.block_hash.clone()]);
        dag_storage
            .insert(
                &anchor,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
            )
            .unwrap();

        // The genesis hash this node learned during restore.
        let genesis = make(0, vec![]);
        dag_storage
            .record_genesis_hash(genesis.block_hash.clone())
            .unwrap();

        // A bonding block: its bonds name a validator that has no latest
        // message and appears in no justification — the newly-bonded case.
        let new_validator = Validator::from(vec![7u8; 65]);
        let bonding_block = get_random_block(
            Some(6),
            None,
            None,
            None,
            Some(Validator::from(vec![9u8; 65])),
            None,
            None,
            Some(vec![anchor.block_hash.clone()]),
            Some(vec![]),
            None,
            None,
            Some(vec![models::rust::casper::protocol::casper_message::Bond {
                validator: new_validator.clone(),
                stake: 100,
            }]),
            None,
            None,
        );
        assert_ne!(
            bonding_block.sender, new_validator,
            "fixture: the joiner must not be the inserting block's sender"
        );
        dag_storage
            .insert(
                &bonding_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();

        let dag = dag_storage.get_representation().unwrap();
        assert_eq!(
            dag.latest_message_hash(&new_validator),
            Some(genesis.block_hash.clone()),
            "the newly-bonded slot on a truncated node must be seeded with the \
             learned genesis hash, not with whatever block was being inserted \
             (got {:?}, inserting block was {})",
            dag.latest_message_hash(&new_validator)
                .map(|h| hex::encode(&h)),
            hex::encode(&bonding_block.block_hash),
        );
    });
}
