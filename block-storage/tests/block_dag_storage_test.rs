// See block-storage/src/test/scala/coop/rchain/blockstorage/dag/BlockDagStorageTest.scala
// See block-storage/src/test/scala/coop/rchain/blockstorage/dag/BlockDagKeyValueStorageTest.scala

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::{
    block_element_gen, block_elements_with_parents_gen, block_hash_gen, block_with_new_hashes_gen,
    get_random_block as random_block, validator_gen,
};
use models::rust::block_metadata::{
    AdmissionRejectionReason, BlockMetadata, CertifiedAdmissionOutcome, CertifiedSenderAuthority,
    CERTIFIED_ADMISSION_PROTOCOL_VERSION,
};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, Justification, ProcessedDeploy, ProcessedSystemDeploy, StateEffectId,
};
use models::rust::equivocation_record::EquivocationRecord;
use models::rust::validator::Validator;
use once_cell::sync::Lazy;
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use tokio::runtime::Runtime;

fn init_logger() { shared::rust::tracing_init::init_for_tests(); }

#[allow(clippy::too_many_arguments)]
fn get_random_block(
    set_block_number: Option<i64>,
    set_seq_number: Option<i32>,
    set_pre_state_hash: Option<StateHash>,
    set_post_state_hash: Option<StateHash>,
    set_validator: Option<Validator>,
    set_version: Option<i64>,
    set_timestamp: Option<i64>,
    set_parents_hash_list: Option<Vec<BlockHash>>,
    set_justifications: Option<Vec<Justification>>,
    set_deploys: Option<Vec<ProcessedDeploy>>,
    set_sys_deploys: Option<Vec<ProcessedSystemDeploy>>,
    set_bonds: Option<Vec<Bond>>,
    set_shard_id: Option<String>,
    hash_f: Option<Box<dyn Fn(BlockMessage) -> BlockHash>>,
) -> BlockMessage {
    let mut block = random_block(
        set_block_number,
        set_seq_number,
        set_pre_state_hash,
        set_post_state_hash,
        set_validator,
        set_version,
        set_timestamp,
        set_parents_hash_list,
        set_justifications,
        set_deploys,
        set_sys_deploys,
        set_bonds,
        set_shard_id,
        hash_f,
    );
    block.header.sender_bond_generation = Some(BondGeneration::GENESIS);
    block.body.state.bond_generations.clear();
    block
}

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
        Some(vec![]),
        None,
        None,
        Some(vec![]),
        None,
        None,
    )
}

fn certified_sender_authority(block: &BlockMessage) -> CertifiedSenderAuthority {
    let generation = block
        .header
        .sender_bond_generation
        .expect("test block sender bond generation");
    let stake = block
        .body
        .state
        .bonds
        .iter()
        .find(|bond| bond.validator == block.sender && bond.stake > 0)
        .map(|bond| bond.stake)
        .unwrap_or(1);
    let authority_floor_hash = block
        .header
        .parents_hash_list
        .first()
        .cloned()
        .unwrap_or_else(|| block.block_hash.clone());
    let authority_floor_post_state_hash = block.body.state.pre_state_hash.clone();
    let mut preimage = b"f1r3fly-test-certified-consensus-context-v1".to_vec();
    preimage.extend_from_slice(&authority_floor_hash);
    preimage.extend_from_slice(&authority_floor_post_state_hash);
    let context_digest = Blake2b256::hash(preimage).into();
    CertifiedSenderAuthority::new(
        block,
        authority_floor_hash,
        authority_floor_post_state_hash,
        context_digest,
        generation,
        stake,
    )
    .expect("test block sender authority")
}

fn expected_metadata(block: &BlockMessage, intrinsically_invalid: bool) -> BlockMetadata {
    let authority = certified_sender_authority(block);
    let outcome = if intrinsically_invalid {
        CertifiedAdmissionOutcome::rejected(
            block,
            &authority,
            AdmissionRejectionReason::InvalidTransaction,
        )
    } else {
        CertifiedAdmissionOutcome::accepted(block, &authority)
    }
    .expect("test admission outcome");
    BlockMetadata::from_certified_block(block, None, None, &authority, &outcome)
        .expect("certified test block metadata")
}

async fn create_dag_storage(genesis: &BlockMessage) -> BlockDagKeyValueStorage {
    let mut kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    dag_storage
        .insert(
            genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
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
                    let candidate = expected_metadata(block_element, false);
                    acc.entry(block_element.sender.clone())
                        .and_modify(|current: &mut BlockMetadata| {
                            if candidate.sequence_number > current.sequence_number
                                || (candidate.sequence_number == current.sequence_number
                                    && candidate.block_hash < current.block_hash)
                            {
                                *current = candidate.clone();
                            }
                        })
                        .or_insert(candidate);
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
                Some(expected_metadata(block_element, false))
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

    let mut expected_by_height = BTreeMap::<i64, HashSet<BlockHash>>::new();
    expected_by_height
        .entry(genesis.body.state.block_number)
        .or_default()
        .insert(genesis.block_hash.clone());
    for block in block_elements {
        expected_by_height
            .entry(block.body.state.block_number)
            .or_default()
            .insert(block.block_hash.clone());
    }
    let real_topo_sort = expected_by_height.into_values().collect::<Vec<_>>();

    assert_eq!(topo_sort.len(), real_topo_sort.len());

    for (topo_sort_level, real_topo_sort_level) in topo_sort.iter().zip(real_topo_sort.iter()) {
        let topo_sort_set: HashSet<BlockHash> = topo_sort_level.iter().cloned().collect();

        assert_eq!(topo_sort_set, *real_topo_sort_level);
    }

    let expected_latest_block_number = block_elements
        .iter()
        .map(|block| block.body.state.block_number)
        .chain(std::iter::once(genesis.body.state.block_number))
        .max()
        .expect("genesis height")
        + 1;
    assert_eq!(*latest_block_number, expected_latest_block_number);
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
        assert_eq!(block_metadata, Some(expected_metadata(&block_element, false)));
        assert_eq!(latest_message_hash, Some(block_element.block_hash.clone()));
        assert_eq!(latest_message, Some(expected_metadata(&block_element, false)));
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
        let meta = BlockMetadata::from_block(be, None, None);
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
fn dag_storage_rejects_non_genesis_block_with_empty_sender() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
      if let Some(mut block) = block_elements.first().cloned() {
        block.sender = prost::bytes::Bytes::new();
        let result = dag_storage.insert(
            &block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        );
        prop_assert!(result.is_err());
        let dag = dag_storage.get_representation().expect("dag representation");
        prop_assert!(!dag.contains(&block.block_hash));
      }
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

        let equivocation_record = EquivocationRecord::new(
            equivocator,
            models::rust::bond_generation::BondGeneration::GENESIS,
            0,
            BTreeSet::from([block_hash]),
        );
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

        let equivocation_record = EquivocationRecord::new(
            equivocator.clone(),
            models::rust::bond_generation::BondGeneration::GENESIS,
            0,
            BTreeSet::from([block_hash1.clone()]),
        );
        dag_storage.insert_equivocation_record(equivocation_record.clone()).unwrap();

        dag_storage.update_equivocation_record(equivocation_record, block_hash2.clone()).unwrap();

        let updated_equivocation_record = EquivocationRecord::new(
            equivocator,
            models::rust::bond_generation::BondGeneration::GENESIS,
            0,
            BTreeSet::from([block_hash1, block_hash2]),
        );
        let records = dag_storage.equivocation_records().unwrap();
        assert_eq!(records, HashSet::from([updated_equivocation_record]));
    });
}

#[test]
fn equivocation_tracker_generation_identity_is_arrival_order_independent() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(
        equivocator in validator_gen(),
        generation_zero_hash in block_hash_gen(),
        generation_one_hash in block_hash_gen(),
        reverse_order in any::<bool>(),
    )| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
        let generation_one = BondGeneration::new(1).unwrap();
        let generation_zero_record = EquivocationRecord::new(
            equivocator.clone(),
            BondGeneration::GENESIS,
            7,
            BTreeSet::from([generation_zero_hash]),
        );
        let generation_one_record = EquivocationRecord::new(
            equivocator,
            generation_one,
            7,
            BTreeSet::from([generation_one_hash]),
        );
        let ordered = if reverse_order {
            [generation_one_record.clone(), generation_zero_record.clone()]
        } else {
            [generation_zero_record.clone(), generation_one_record.clone()]
        };
        for record in ordered {
            dag_storage.insert_equivocation_record(record).unwrap();
        }

        prop_assert_eq!(
            dag_storage.equivocation_records().unwrap(),
            HashSet::from([generation_zero_record, generation_one_record]),
        );
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
      assert_eq!(invalid_blocks_set, block_elements.into_iter().map(|b| expected_metadata(&b, true)).collect::<HashSet<_>>());
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
        Some(vec![]),
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
    let active_generations = HashMap::from([(valid_block.sender.clone(), BondGeneration::GENESIS)]);
    assert!(!dag
        .valid_latest_messages(&active_generations)
        .unwrap()
        .contains_key(&valid_block.sender));
}

#[test]
fn equal_sequence_latest_message_tie_break_is_arrival_order_independent() {
    let validator = Bytes::from(vec![0xa9; models::rust::validator::LENGTH]);
    let mut genesis = genesis_block();
    genesis.body.state.bonds = vec![Bond {
        validator: validator.clone(),
        stake: 100,
    }];
    let forward = RUNTIME.block_on(create_dag_storage(&genesis));
    let reverse = RUNTIME.block_on(create_dag_storage(&genesis));
    let mut high_hash = get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        Some(validator.clone()),
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    high_hash.block_hash = Bytes::from(vec![0x20; models::rust::block_hash::LENGTH]);
    high_hash.body.state.bonds = genesis.body.state.bonds.clone();
    let mut low_hash = high_hash.clone();
    low_hash.block_hash = Bytes::from(vec![0x10; models::rust::block_hash::LENGTH]);

    for block in [&high_hash, &low_hash] {
        forward
            .insert(
                block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
    }
    for block in [&low_hash, &high_hash] {
        reverse
            .insert(
                block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
    }

    let expected = Some(low_hash.block_hash);
    assert_eq!(
        forward
            .get_representation()
            .unwrap()
            .latest_message_hash(&validator),
        expected
    );
    assert_eq!(
        reverse
            .get_representation()
            .unwrap()
            .latest_message_hash(&validator),
        expected
    );
}

#[test]
fn objective_equivocation_pair_and_vote_exclusion_are_arrival_order_independent() {
    let validator = Bytes::from(vec![0xa8; models::rust::validator::LENGTH]);
    let mut genesis = genesis_block();
    genesis.body.state.bonds = vec![Bond {
        validator: validator.clone(),
        stake: 100,
    }];
    let forward = RUNTIME.block_on(create_dag_storage(&genesis));
    let reverse = RUNTIME.block_on(create_dag_storage(&genesis));
    let mut high_hash = get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        Some(validator.clone()),
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    high_hash.block_hash = Bytes::from(vec![0x42; models::rust::block_hash::LENGTH]);
    high_hash.body.state.bonds = genesis.body.state.bonds.clone();
    let mut low_hash = high_hash.clone();
    low_hash.block_hash = Bytes::from(vec![0x24; models::rust::block_hash::LENGTH]);

    forward
        .insert(
            &high_hash,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();
    forward
        .insert(
            &low_hash,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();
    reverse
        .insert(
            &low_hash,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();
    reverse
        .insert(
            &high_hash,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let mut descendant = high_hash.clone();
    descendant.block_hash = Bytes::from(vec![0x55; models::rust::block_hash::LENGTH]);
    descendant.seq_num = 2;
    for storage in [&forward, &reverse] {
        storage
            .insert(
                &descendant,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
    }

    let forward_dag = forward.get_representation().unwrap();
    let reverse_dag = reverse.get_representation().unwrap();
    let expected_group =
        BTreeSet::from([low_hash.block_hash.clone(), high_hash.block_hash.clone()]);
    assert_eq!(
        forward_dag
            .equivocation_observations()
            .get(&(validator.clone(), BondGeneration::GENESIS, 1))
            .cloned(),
        Some(expected_group.clone())
    );
    assert_eq!(
        reverse_dag
            .equivocation_observations()
            .get(&(validator.clone(), BondGeneration::GENESIS, 1))
            .cloned(),
        Some(expected_group)
    );
    let generation_zero = HashMap::from([(validator.clone(), BondGeneration::GENESIS)]);
    assert!(!forward_dag
        .valid_latest_messages(&generation_zero)
        .unwrap()
        .contains_key(&validator));
    assert!(!reverse_dag
        .valid_latest_messages(&generation_zero)
        .unwrap()
        .contains_key(&validator));

    let mut next_lifetime = descendant.clone();
    next_lifetime.block_hash = Bytes::from(vec![0x66; models::rust::block_hash::LENGTH]);
    next_lifetime.seq_num = 3;
    next_lifetime.body.state.block_number = 10;
    next_lifetime.header.sender_bond_generation = Some(BondGeneration::new(1).unwrap());
    let generation_one = HashMap::from([(validator.clone(), BondGeneration::new(1).unwrap())]);
    for storage in [&forward, &reverse] {
        storage
            .insert(
                &next_lifetime,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
        assert!(storage
            .get_representation()
            .unwrap()
            .valid_latest_messages(&generation_one)
            .unwrap()
            .contains_key(&validator));
    }
}

#[test]
fn objective_equivocation_pair_is_scoped_to_validator_generation_across_block_heights() {
    let validator = Bytes::from(vec![0xa7; models::rust::validator::LENGTH]);
    let mut genesis = genesis_block();
    genesis.body.state.bonds = vec![Bond {
        validator: validator.clone(),
        stake: 100,
    }];
    let storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let mut old = get_random_block(
        Some(9),
        Some(1),
        None,
        None,
        Some(validator.clone()),
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    old.block_hash = Bytes::from(vec![0x10; models::rust::block_hash::LENGTH]);
    old.body.state.bonds = genesis.body.state.bonds.clone();
    let mut current_first = old.clone();
    current_first.block_hash = Bytes::from(vec![0x20; models::rust::block_hash::LENGTH]);
    current_first.body.state.block_number = 10;
    current_first.header.sender_bond_generation = Some(BondGeneration::new(1).unwrap());
    let mut current_second = current_first.clone();
    current_second.block_hash = Bytes::from(vec![0x30; models::rust::block_hash::LENGTH]);
    current_second.body.state.block_number = 11;
    for block in [&old, &current_first, &current_second] {
        storage
            .insert(
                block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
    }

    let dag = storage.get_representation().unwrap();
    assert!(!dag.objective_equivocations().contains_key(&(
        validator.clone(),
        BondGeneration::GENESIS,
        1,
    )));
    assert_eq!(
        dag.objective_equivocations()
            .get(&(validator, BondGeneration::new(1).unwrap(), 1))
            .cloned(),
        Some((current_first.block_hash, current_second.block_hash))
    );
}

#[test]
fn invalid_block_bonds_cannot_register_a_new_validator_slot() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let untrusted_validator = Bytes::from(vec![0xab; models::rust::validator::LENGTH]);
    let mut invalid_block = get_random_block(
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
    invalid_block.body.state.bonds = vec![Bond {
        validator: untrusted_validator.clone(),
        stake: 100,
    }];

    dag_storage
        .insert(
            &invalid_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
        )
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(dag.latest_message_hash(&untrusted_validator), None);
    assert_eq!(dag.latest_message_hash(&invalid_block.sender), None);
}

#[test]
fn accepted_post_state_bonds_register_a_new_validator_slot_before_floor_promotion() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let new_validator = Bytes::from(vec![0xac; models::rust::validator::LENGTH]);
    let mut block = get_random_block(
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
    block.body.state.bonds = vec![Bond {
        validator: new_validator.clone(),
        stake: 100,
    }];

    dag_storage
        .insert(
            &block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(
        dag.latest_message_hash(&new_validator),
        Some(genesis.block_hash)
    );
}

#[test]
fn nonpositive_post_state_bonds_do_not_register_validator_slots() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let zero_validator = Bytes::from(vec![0xad; models::rust::validator::LENGTH]);
    let negative_validator = Bytes::from(vec![0xae; models::rust::validator::LENGTH]);
    let mut block = get_random_block(
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
    block.body.state.bonds = vec![
        Bond {
            validator: zero_validator.clone(),
            stake: 0,
        },
        Bond {
            validator: negative_validator.clone(),
            stake: -1,
        },
    ];

    dag_storage
        .insert(
            &block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    assert_eq!(dag.latest_message_hash(&zero_validator), None);
    assert_eq!(dag.latest_message_hash(&negative_validator), None);
}

#[test]
fn accepted_transition_uses_canonical_genesis_despite_invalid_height_zero_junk() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let new_validator = Bytes::from(vec![0xaf; models::rust::validator::LENGTH]);
    let mut junk = get_random_block(
        Some(0),
        Some(0),
        None,
        None,
        None,
        None,
        None,
        Some(Vec::new()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    junk.block_hash = Bytes::from(vec![0; models::rust::block_hash::LENGTH]);
    dag_storage
        .insert(
            &junk,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
        )
        .unwrap();

    let mut transition = get_random_block(
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
    transition.body.state.bonds = vec![Bond {
        validator: new_validator.clone(),
        stake: 100,
    }];
    dag_storage
        .insert(
            &transition,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();

    assert_eq!(
        dag_storage
            .get_representation()
            .expect("dag representation")
            .latest_message_hash(&new_validator),
        Some(genesis.block_hash)
    );
}

#[test]
fn protocol_v5_metadata_without_finalization_ledger_fails_closed() {
    RUNTIME.block_on(async {
        let genesis = genesis_block();
        let mut kvm = InMemoryStoreManager::new();
        let pre_upgrade = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        pre_upgrade
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
        assert_eq!(pre_upgrade.genesis_hash().unwrap(), None);
        drop(pre_upgrade);

        let error = match BlockDagKeyValueStorage::new(&mut kvm).await {
            Ok(_) => panic!("protocol-v5 metadata without a ledger must fail closed"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("protocol-v5 DAG metadata exists without a finalization ledger"));
    });
}

#[test]
fn conflicting_approved_insert_cannot_replace_canonical_genesis() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let mut conflicting = genesis_block();
    conflicting.block_hash = Bytes::from(vec![0xcc; models::rust::block_hash::LENGTH]);

    let error = match dag_storage.insert(
        &conflicting,
        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
    ) {
        Ok(_) => panic!("conflicting approved root must fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(
        dag_storage.genesis_hash().unwrap(),
        Some(genesis.block_hash)
    );
}

#[tokio::test]
async fn duplicate_approved_genesis_preserves_advanced_finalization_across_restart() {
    let genesis = genesis_block();
    let mut kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    dag_storage
        .insert(
            &genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
        )
        .unwrap();

    let successor = get_random_block(
        Some(1),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    dag_storage
        .insert(
            &successor,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .unwrap();
    dag_storage
        .record_directly_finalized(successor.block_hash.clone(), 1.0, |_| {
            Box::pin(async { Ok(()) })
        })
        .await
        .unwrap();
    let advanced_head = dag_storage.finalization_head().unwrap().unwrap();
    let records = dag_storage.committed_finalization_records().unwrap();

    dag_storage
        .insert(
            &genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
        )
        .unwrap();
    assert_eq!(
        dag_storage.finalization_head().unwrap(),
        Some(advanced_head.clone())
    );
    assert_eq!(
        dag_storage.committed_finalization_records().unwrap(),
        records
    );

    drop(dag_storage);
    let reopened = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    assert_eq!(
        reopened.finalization_head().unwrap(),
        Some(advanced_head.clone())
    );
    assert_eq!(reopened.committed_finalization_records().unwrap(), records);
    reopened
        .insert(
            &genesis,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
        )
        .unwrap();
    assert_eq!(
        reopened.finalization_head().unwrap(),
        Some(advanced_head.clone())
    );
    assert_eq!(reopened.committed_finalization_records().unwrap(), records);

    let mut altered = genesis.clone();
    altered.body.state.post_state_hash = Bytes::from(vec![0x7a; models::rust::block_hash::LENGTH]);
    let error = match reopened.insert(
        &altered,
        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
    ) {
        Ok(_) => panic!("same-hash genesis with altered immutable metadata must fail"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("duplicate approved genesis disagrees with immutable stored metadata"));
    assert_eq!(reopened.finalization_head().unwrap(), Some(advanced_head));
    assert_eq!(reopened.committed_finalization_records().unwrap(), records);
}

#[test]
fn startup_reconciliation_repairs_missing_slots_and_removes_unregistered_slots() {
    RUNTIME.block_on(async {
        let genesis = genesis_block();
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        block_store.put_block_message(&genesis).unwrap();
        dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();

        let new_validator = Bytes::from(vec![0xb1; models::rust::validator::LENGTH]);
        let mut transition = get_random_block(
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
        transition.body.state.bonds = vec![Bond {
            validator: new_validator.clone(),
            stake: 100,
        }];
        block_store.put_block_message(&transition).unwrap();
        dag_storage
            .insert(
                &transition,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
        let unregistered = Bytes::from(vec![0xb2; models::rust::validator::LENGTH]);
        let mut legacy = get_random_block(
            Some(2),
            Some(1),
            None,
            None,
            Some(unregistered.clone()),
            None,
            None,
            Some(vec![transition.block_hash.clone()]),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        legacy.body.state.bonds = transition.body.state.bonds.clone();
        block_store.put_block_message(&legacy).unwrap();
        dag_storage
            .insert(
                &legacy,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .unwrap();
        assert_eq!(
            dag_storage
                .get_representation()
                .unwrap()
                .latest_message_hash(&unregistered),
            Some(legacy.block_hash.clone())
        );
        dag_storage
            .delete_latest_message_for_test(new_validator.clone())
            .unwrap();

        dag_storage.reconcile_latest_messages(&block_store).unwrap();
        let dag = dag_storage.get_representation().unwrap();
        assert_eq!(
            dag.latest_message_hash(&new_validator),
            Some(genesis.block_hash)
        );
        assert_eq!(dag.latest_message_hash(&unregistered), None);
    });
}

#[test]
fn startup_reconciliation_repairs_objective_and_unary_evidence_indexes() {
    RUNTIME.block_on(async {
        let validator = Bytes::from(vec![0xb3; models::rust::validator::LENGTH]);
        let mut genesis = genesis_block();
        genesis.body.state.bonds = vec![Bond {
            validator: validator.clone(),
            stake: 100,
        }];
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        block_store.put_block_message(&genesis).unwrap();
        dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
            )
            .unwrap();

        let mut first = get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            Some(validator.clone()),
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
        first.block_hash = Bytes::from(vec![0x31; models::rust::block_hash::LENGTH]);
        first.body.state.bonds = genesis.body.state.bonds.clone();
        let mut second = first.clone();
        second.block_hash = Bytes::from(vec![0x32; models::rust::block_hash::LENGTH]);
        for (block, mode) in [
            (
                &first,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            ),
            (
                &second,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
            ),
        ] {
            block_store.put_block_message(block).unwrap();
            dag_storage.insert(block, mode).unwrap();
        }

        dag_storage
            .delete_objective_evidence_for_test(validator.clone(), BondGeneration::GENESIS, 1)
            .unwrap();
        dag_storage
            .delete_invalid_evidence_for_test(second.block_hash.clone())
            .unwrap();
        let stale_hash = Bytes::from(vec![0x33; models::rust::block_hash::LENGTH]);
        let mut stale_metadata = dag_storage
            .get_representation()
            .unwrap()
            .lookup(&genesis.block_hash)
            .unwrap()
            .expect("genesis metadata");
        stale_metadata.block_hash = stale_hash.clone();
        dag_storage
            .put_invalid_evidence_for_test(stale_hash.clone(), stale_metadata)
            .unwrap();
        let damaged = dag_storage.get_representation().unwrap();
        assert!(!damaged.equivocation_observations().contains_key(&(
            validator.clone(),
            BondGeneration::GENESIS,
            1
        )));
        assert_eq!(
            damaged
                .invalid_blocks()
                .into_iter()
                .map(|metadata| metadata.block_hash)
                .collect::<Vec<_>>(),
            vec![stale_hash]
        );

        drop(dag_storage);
        let reopened = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        reopened.reconcile_latest_messages(&block_store).unwrap();
        let repaired = reopened.get_representation().unwrap();
        assert_eq!(
            repaired
                .equivocation_observations()
                .get(&(validator, BondGeneration::GENESIS, 1))
                .cloned(),
            Some(BTreeSet::from([
                first.block_hash,
                second.block_hash.clone()
            ]))
        );
        assert_eq!(
            repaired
                .invalid_blocks()
                .into_iter()
                .map(|metadata| metadata.block_hash)
                .collect::<Vec<_>>(),
            vec![second.block_hash]
        );
    });
}

#[test]
fn dag_storage_should_be_able_to_restore_deploy_index_on_startup() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block_elements in block_elements_with_parents_gen(genesis.clone(), 0, 10))| {
      let forward_storage = RUNTIME.block_on(create_dag_storage(&genesis));
      let reverse_storage = RUNTIME.block_on(create_dag_storage(&genesis));

      for block_element in &block_elements {
        forward_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
      }
      for block_element in block_elements.iter().rev() {
        reverse_storage.insert(block_element, block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal).unwrap();
      }

      let forward_dag = forward_storage.get_representation().expect("forward dag representation");
      let reverse_dag = reverse_storage.get_representation().expect("reverse dag representation");
      let mut expected_occurrences: HashMap<Vec<u8>, BTreeSet<BlockHash>> = HashMap::new();
      let mut expected_representatives: HashMap<Vec<u8>, (i64, BlockHash)> = HashMap::new();

      for block in &block_elements {
          for deploy in &block.body.deploys {
              let deploy_id = deploy.deploy.sig.to_vec();
              expected_occurrences
                  .entry(deploy_id.clone())
                  .or_default()
                  .insert(block.block_hash.clone());
              let candidate = (block.body.state.block_number, block.block_hash.clone());
              expected_representatives
                  .entry(deploy_id)
                  .and_modify(|current| {
                      if candidate.0 > current.0 || (candidate.0 == current.0 && candidate.1 < current.1) {
                          *current = candidate.clone();
                      }
                  })
                  .or_insert(candidate);
          }
      }

      for (deploy_id, occurrences) in expected_occurrences {
          assert_eq!(forward_dag.lookup_deploy_occurrences(&deploy_id).unwrap(), occurrences);
          assert_eq!(reverse_dag.lookup_deploy_occurrences(&deploy_id).unwrap(), occurrences);
          let expected = expected_representatives.get(&deploy_id).map(|(_, hash)| hash.clone());
          assert_eq!(forward_dag.lookup_by_deploy_id(&deploy_id).unwrap(), expected);
          assert_eq!(reverse_dag.lookup_by_deploy_id(&deploy_id).unwrap(), expected);
      }
    });
}

#[test]
fn negative_sequence_rejection_persists_without_objective_evidence() {
    let genesis = genesis_block();
    let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
    let invalid_block = get_random_block(
        Some(1),
        Some(-2),
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );

    for _ in 0..2 {
        dag_storage
            .insert(
                &invalid_block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid,
            )
            .unwrap();
    }

    let dag = dag_storage.get_representation().unwrap();
    assert!(dag
        .invalid_blocks()
        .iter()
        .any(|metadata| metadata.block_hash == invalid_block.block_hash));
    assert!(dag
        .equivocation_observations()
        .values()
        .all(|hashes| !hashes.contains(&invalid_block.block_hash)));
}

#[test]
fn objective_evidence_index_matches_signed_sequence_domain() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(sequence in any::<i32>())| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
        let invalid_block = get_random_block(
            Some(1),
            Some(sequence),
            None,
            None,
            None,
            None,
            None,
            Some(vec![genesis.block_hash.clone()]),
            Some(vec![]),
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

        let dag = dag_storage.get_representation().unwrap();
        let indexed = dag
            .equivocation_observations()
            .values()
            .any(|hashes| hashes.contains(&invalid_block.block_hash));
        prop_assert_eq!(indexed, sequence >= 0);
    });
}

#[test]
fn dag_storage_should_be_able_to_handle_blocks_with_invalid_numbers() {
    let genesis = genesis_block();
    proptest!(proptest_config(), |(block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None))| {
        let dag_storage = RUNTIME.block_on(create_dag_storage(&genesis));
        let mut invalid_block = block.clone();
        invalid_block.body.state.block_number = 1000;
        invalid_block.header.parents_hash_list = vec![genesis.block_hash.clone()];
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
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
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
        Some(vec![]),
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
        Some(vec![]),
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
        Some(vec![]),
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
        Some(vec![]),
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

#[tokio::test]
async fn bound_finalization_rejects_stale_certificates_and_dropped_finalized_state() {
    let mut genesis = genesis_block();
    genesis.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    let dag_storage = create_dag_storage(&genesis).await;
    let initial_base = dag_storage
        .capture_finalization_base()
        .expect("initial finalization base");

    let mut finalized_effect_source = get_random_block(
        Some(1),
        None,
        None,
        None,
        None,
        Some(CERTIFIED_ADMISSION_PROTOCOL_VERSION),
        None,
        Some(vec![genesis.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    finalized_effect_source.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    dag_storage
        .insert(
            &finalized_effect_source,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("insert finalized effect source");

    let mut drops_finalized_effect = get_random_block(
        Some(2),
        None,
        None,
        None,
        None,
        Some(CERTIFIED_ADMISSION_PROTOCOL_VERSION),
        None,
        Some(vec![finalized_effect_source.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    drops_finalized_effect.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    dag_storage
        .insert(
            &drops_finalized_effect,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("insert state-dropping descendant");

    let mut preserves_finalized_effect = get_random_block(
        Some(2),
        None,
        None,
        None,
        None,
        Some(CERTIFIED_ADMISSION_PROTOCOL_VERSION),
        None,
        Some(vec![finalized_effect_source.block_hash.clone()]),
        Some(vec![]),
        None,
        None,
        None,
        None,
        None,
    );
    preserves_finalized_effect.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    dag_storage
        .insert(
            &preserves_finalized_effect,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("insert state-preserving descendant");

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    for hash in [
        &genesis.block_hash,
        &finalized_effect_source.block_hash,
        &drops_finalized_effect.block_hash,
        &preserves_finalized_effect.block_hash,
    ] {
        dag.put_cached_floor(hash.clone(), genesis.block_hash.clone())
            .expect("cache finalized floor");
    }
    let effect = StateEffectId {
        source_block_hash: finalized_effect_source.block_hash.clone(),
        execution_index: 0,
    };
    let mut source_metadata = dag
        .lookup_unsafe(&finalized_effect_source.block_hash)
        .expect("source metadata");
    source_metadata.successful_state_effect_indices.insert(0);
    dag.block_metadata_index
        .write()
        .add(source_metadata)
        .expect("record source effect provenance");
    let mut dropping_metadata = dag
        .lookup_unsafe(&drops_finalized_effect.block_hash)
        .expect("dropping metadata");
    dropping_metadata.rejected_state_effects.insert(effect);
    dag.block_metadata_index
        .write()
        .add(dropping_metadata)
        .expect("record rejected effect provenance");

    assert!(
        block_storage::rust::finality::state_preservation::is_state_preserved(
            &dag,
            &genesis.block_hash,
            &drops_finalized_effect.block_hash,
        )
        .expect("old-base preservation")
    );
    assert!(
        !block_storage::rust::finality::state_preservation::is_state_preserved(
            &dag,
            &finalized_effect_source.block_hash,
            &drops_finalized_effect.block_hash,
        )
        .expect("current-base preservation")
    );

    dag_storage
        .record_directly_finalized_atomic(
            &initial_base.head,
            finalized_effect_source.block_hash.clone(),
            1.0,
            |_, _| async { Ok(()) },
        )
        .await
        .expect("commit newer finalized base");

    let stale_effect_invocations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let invocation_counter = stale_effect_invocations.clone();
    let stale = dag_storage
        .record_directly_finalized_atomic(
            &initial_base.head,
            drops_finalized_effect.block_hash.clone(),
            1.0,
            move |_, _| {
                let invocation_counter = invocation_counter.clone();
                async move {
                    invocation_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .await
        .expect_err("stale certificate must not late-bind to the new head");
    assert!(matches!(stale, KvStoreError::StaleFinalization {
        expected_revision: 0,
        actual_revision: 1,
    }));
    assert_eq!(
        stale_effect_invocations.load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    let current_base = dag_storage
        .capture_finalization_base()
        .expect("current finalization base");
    assert_eq!(
        current_base.head.block_hash.0,
        finalized_effect_source.block_hash
    );
    let rejected = dag_storage
        .record_directly_finalized_atomic(
            &current_base.head,
            drops_finalized_effect.block_hash.clone(),
            1.0,
            |_, _| async { Ok(()) },
        )
        .await
        .expect_err("fresh evaluation must reject a finalized-state regression");
    assert!(
        matches!(rejected, KvStoreError::InvalidArgument(message) if message.contains("durable finalized state"))
    );
    assert_eq!(
        dag_storage
            .finalization_head()
            .expect("durable finalization head")
            .expect("initialized finalization head")
            .block_hash
            .0,
        finalized_effect_source.block_hash
    );

    let stale_safe = dag_storage
        .record_directly_finalized_atomic(
            &initial_base.head,
            preserves_finalized_effect.block_hash.clone(),
            1.0,
            |_, _| async { Ok(()) },
        )
        .await
        .expect_err("a safe candidate still requires fresh evaluation after head change");
    assert!(matches!(stale_safe, KvStoreError::StaleFinalization {
        expected_revision: 0,
        actual_revision: 1,
    }));
    dag_storage
        .record_directly_finalized_atomic(
            &current_base.head,
            preserves_finalized_effect.block_hash.clone(),
            1.0,
            |_, _| async { Ok(()) },
        )
        .await
        .expect("fresh state-preserving evaluation may commit");
    assert_eq!(
        dag_storage
            .finalization_head()
            .expect("updated durable finalization head")
            .expect("initialized updated head")
            .block_hash
            .0,
        preserves_finalized_effect.block_hash
    );
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

// -------------------------------------------------------------------
// DD-7b-2 (a) Option 2 (2026-08-29): behavioral tests for the
// payload-source index.  Complement the shape-scan pins in casper /
// node / rholang with real LMDB round-trips that would catch a
// regression in the typed-store key/value bincode encoding, the
// `Vec<u8>` key semantics, or the `PlRwLock`-guarded put/get shape.
// -------------------------------------------------------------------

#[test]
fn payload_source_record_and_lookup_round_trip() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;

        let payload_hash = [0x11u8; 32];
        let deploy_sig: Vec<u8> = vec![0xAA, 0xBB, 0xCC, 0xDD];

        // Before recording: lookup misses cleanly.
        let before = dag_storage
            .lookup_payload_source(&payload_hash)
            .expect("lookup must not error on empty index");
        assert!(
            before.is_none(),
            "lookup on an empty index must return Ok(None), not an error"
        );

        // Record the mapping.
        dag_storage
            .record_payload_source(payload_hash, &deploy_sig)
            .expect("record must succeed on a fresh storage");

        // After recording: lookup returns the exact sig bytes.
        let got = dag_storage
            .lookup_payload_source(&payload_hash)
            .expect("lookup must not error after record");
        assert_eq!(
            got.as_deref(),
            Some(deploy_sig.as_slice()),
            "lookup after record must return the exact deploy_sig bytes"
        );
    });
}

#[test]
fn payload_source_latest_writer_wins_on_collision() {
    // Two deploys produce byte-identical writes → same Blake2b256
    // payload_hash → same index key.  put_one's latest-writer-wins
    // semantics are the design choice: newer blocks are more likely
    // still in block_storage (design decision 6 lazy fork/prune).
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;

        let payload_hash = [0x22u8; 32];
        let sig_first: Vec<u8> = vec![0xF1; 8];
        let sig_second: Vec<u8> = vec![0xF2; 8];

        dag_storage
            .record_payload_source(payload_hash, &sig_first)
            .expect("first record");
        dag_storage
            .record_payload_source(payload_hash, &sig_second)
            .expect("second record");

        let got = dag_storage
            .lookup_payload_source(&payload_hash)
            .expect("lookup")
            .expect("must be present");
        assert_eq!(
            got, sig_second,
            "latest-writer-wins: the second record must overwrite the first \
             (design decision 6 lazy fork/prune assumes newer entries)"
        );
    });
}

#[test]
fn payload_source_lookup_by_deploy_id_available_on_writable_storage() {
    // DD-7b-2 (a) Option 2 wire-in adds a `lookup_by_deploy_id`
    // mirror on `BlockDagKeyValueStorage` (previously only on the
    // DAG representation).  Needed by the boot reducer BEFORE the
    // LFB-guarded DAG rep is available.  The DAG-rep version is
    // already well-covered by upstream tests; this pin verifies
    // the storage-side method returns the same value for a block
    // inserted via the standard path.
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let genesis = genesis_block();
        let dag_storage = create_dag_storage(&genesis).await;

        // Pull the genesis's deploy sig — genesis_block seeds
        // ProcessedDeploys so we have a real key to look up
        // without needing to construct a child block.
        let deploy_id_vec: Vec<u8> = genesis
            .body
            .deploys
            .first()
            .expect("genesis_block seeds at least one processed deploy")
            .deploy
            .sig
            .to_vec();
        let via_storage = dag_storage
            .lookup_by_deploy_id(&deploy_id_vec)
            .expect("storage-side lookup_by_deploy_id must not error");
        let via_dag_rep = dag_storage
            .get_representation()
            .expect("dag representation")
            .lookup_by_deploy_id(&deploy_id_vec)
            .expect("dag-rep lookup_by_deploy_id must not error");
        assert_eq!(
            via_storage, via_dag_rep,
            "the new writable-storage lookup_by_deploy_id (used by the boot \
             reducer's chain step 2) must return the same block_hash as the \
             DAG-rep version — a divergence would let the boot reducer see a \
             different chain state than downstream consumers."
        );
    });
}
