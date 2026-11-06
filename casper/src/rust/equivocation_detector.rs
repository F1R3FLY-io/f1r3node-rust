//! Equivocation detection — the equivocation verdict plus the
//! neglected-equivocation RMW path.
//!
//! ## Responsibilities
//!
//! * [`EquivocationDetector::equivocation_of`] — an equivocation is two
//!   blocks signed by one sender at one sequence number; finds the held
//!   sibling of a freshly-arrived block.
//! * [`EquivocationDetector::check_neglected_equivocations`] — a block whose
//!   justifications show both blocks of an equivocation must leave the
//!   equivocator unbonded.
//!
//! Neither mutates validator bonds — that is the PoS contract's job. A
//! stored equivocation and its sibling are the evidence the proposer turns
//! into a `SlashDeploy` (`slashing_authorization::authorized_slash_candidates`).

use std::collections::{BTreeMap, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::slashing_authorization::checked_base_seq;
use crate::rust::ValidBlockProcessing;

/// Equivocation detection logic for blockchain consensus
pub struct EquivocationDetector;

/// Memoizes per-justification canonical-child resolution within a single
/// detection pass. The key is (justification block hash, equivocating
/// validator, equivocation-base seq); the value is the canonical child hash
/// (or `None` if no child exists above the base). Without this cache,
/// `is_equivocation_detectable` would re-walk the self-justification chain
/// O(N×J) times for every iteration of the outer record loop.
type CanonicalChildCache = HashMap<(BlockHash, Validator, i64), Option<BlockHash>>;

impl EquivocationDetector {
    /// The sender's other held block at `block`'s sequence number, the one
    /// with the lowest hash if several are held: the block that makes `block`
    /// an equivocation.
    pub fn equivocation_of(
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
    ) -> Option<BlockHash> {
        dag.blocks_at(&block.sender, block.seq_num)?
            .iter()
            .filter(|hash| **hash != block.block_hash)
            .min()
            .cloned()
    }

    /// The verdict on a block that equivocates: admissible when another block
    /// has requested it as a dependency, ignorable otherwise.
    pub fn equivocation_verdict(
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        requested_as_dependency: bool,
    ) -> ValidBlockProcessing {
        let Some(sibling) = Self::equivocation_of(dag, block) else {
            return Either::Right(ValidBlock::Valid);
        };
        tracing::warn!(
            target: "f1r3fly.slashing",
            sender = %PrettyPrinter::build_string_no_limit(&block.sender),
            seq_num = block.seq_num,
            block = %PrettyPrinter::build_string_no_limit(&block.block_hash),
            sibling = %PrettyPrinter::build_string_no_limit(&sibling),
            requested_as_dependency,
            "equivocation"
        );
        Either::Left(BlockError::Invalid(if requested_as_dependency {
            InvalidBlock::AdmissibleEquivocation
        } else {
            InvalidBlock::IgnorableEquivocation
        }))
    }

    /// A block neglects an equivocation when both of its blocks are in the
    /// block's justifications and the equivocator still holds stake in the
    /// block's post-state; a block that carries the slash owes nothing.
    /// Every pair in a block's justifications is one this node holds, since it
    /// holds the block's whole justification cone and stores the second block
    /// of every pair it holds as the equivocation.
    pub fn check_neglected_equivocations(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        genesis: &BlockMessage,
        post_state_bonds: &[Bond],
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        let latest_messages = Self::to_latest_message_hashes(&block.justifications);
        let mut canonical_child_cache = CanonicalChildCache::new();
        let mut judged = HashSet::new();
        for equivocation in dag.invalid_blocks() {
            let Some(base_seq) = checked_base_seq(equivocation.sequence_number) else {
                continue;
            };
            if !judged.insert((equivocation.sender.clone(), base_seq)) {
                continue;
            }
            let bonded = post_state_bonds
                .iter()
                .any(|bond| bond.validator == equivocation.sender && bond.stake > 0);
            if bonded
                && Self::is_equivocation_detectable(
                    dag,
                    block_store,
                    &latest_messages,
                    &equivocation.sender,
                    i64::from(base_seq),
                    genesis,
                    &mut canonical_child_cache,
                )?
            {
                return Ok(Either::Left(BlockError::Invalid(
                    InvalidBlock::NeglectedEquivocation,
                )));
            }
        }
        Ok(Either::Right(ValidBlock::Valid))
    }

    /// Project a block's justification list into a `validator -> latest-hash`
    /// map. **`BTreeMap` is consensus-critical here** — every node iterates
    /// the map below in `is_equivocation_detectable`, and `HashMap` iteration
    /// order leaks `RandomState` entropy into consensus, leading to divergent
    /// classifications across nodes. Do not switch to `HashMap`.
    fn to_latest_message_hashes(
        justifications: &[models::rust::casper::protocol::casper_message::Justification],
    ) -> BTreeMap<Validator, BlockHash> {
        justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect()
    }

    /// Whether two distinct blocks of `equivocating_validator` above
    /// `equivocation_base_block_seq_num` are visible from `latest_messages`.
    fn is_equivocation_detectable(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        latest_messages: &BTreeMap<Validator, BlockHash>,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        let mut updated_equivocation_children: Vec<BlockMessage> = Vec::new();

        for justification_block_hash in latest_messages.values() {
            let Some(justification_block) = block_store.get(justification_block_hash)? else {
                continue;
            };

            Self::maybe_add_equivocation_child(
                dag,
                block_store,
                &justification_block,
                equivocating_validator,
                equivocation_base_block_seq_num,
                &mut updated_equivocation_children,
                genesis,
                canonical_child_cache,
            )?;

            if updated_equivocation_children.len() > 1 {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// P2-11: mutates `equivocation_children` in place — returns `Ok(true)`
    /// iff a new equivocation child was appended.
    fn maybe_add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &mut Vec<BlockMessage>,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        // Genesis is unconditionally the equivocation root, never a child of
        // it. Returning early keeps the (genesis, validator, seq) cache key
        // out of the cache.
        if justification_block.block_hash == genesis.block_hash {
            return Ok(false);
        }

        if justification_block.sender == *equivocating_validator {
            let justification_seq_num = i64::from(justification_block.seq_num);
            if justification_seq_num > equivocation_base_block_seq_num {
                Self::add_equivocation_child(
                    dag,
                    block_store,
                    justification_block,
                    equivocating_validator,
                    equivocation_base_block_seq_num,
                    equivocation_children,
                    canonical_child_cache,
                )
            } else {
                Ok(false)
            }
        } else {
            let latest_messages =
                Self::to_latest_message_hashes(&justification_block.justifications);

            // A missing latest-message for the equivocating validator (no
            // entry in `latest_messages`, or the referenced hash not in the
            // store) is treated as *obliviousness*, not as a store
            // inconsistency — the prior code returned `Err(KeyNotFound)` here
            // and rejected the block. Per §9.x of the design we now let the
            // detection pass continue: the block simply contributes no
            // equivocation child via this justification.
            match latest_messages.get(equivocating_validator) {
                Some(latest_equivocating_validator_block_hash) => {
                    match block_store.get(latest_equivocating_validator_block_hash)? {
                        Some(latest_equivocating_validator_block) => {
                            let latest_seq_num =
                                i64::from(latest_equivocating_validator_block.seq_num);
                            if latest_seq_num > equivocation_base_block_seq_num {
                                Self::add_equivocation_child(
                                    dag,
                                    block_store,
                                    &latest_equivocating_validator_block,
                                    equivocating_validator,
                                    equivocation_base_block_seq_num,
                                    equivocation_children,
                                    canonical_child_cache,
                                )
                            } else {
                                Ok(false)
                            }
                        }
                        None => Ok(false),
                    }
                }
                None => Ok(false),
            }
        }
    }

    /// P2-11: mutates `equivocation_children` in place — returns `Ok(true)`
    /// iff a new (deduplicated) equivocation child was appended.
    fn add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &mut Vec<BlockMessage>,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        let key = (
            justification_block.block_hash.clone(),
            equivocating_validator.clone(),
            equivocation_base_block_seq_num,
        );
        let maybe_equivocation_child_hash = match canonical_child_cache.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let computed = Self::find_canonical_creator_justification_child_above_seq(
                    dag,
                    justification_block,
                    equivocating_validator,
                    equivocation_base_block_seq_num,
                )?;
                canonical_child_cache.insert(key, computed.clone());
                computed
            }
        };

        match maybe_equivocation_child_hash {
            Some(equivocation_child_hash) => match block_store.get(&equivocation_child_hash)? {
                Some(equivocation_child) => {
                    let already_present = equivocation_children
                        .iter()
                        .any(|child| child.block_hash == equivocation_child.block_hash);
                    if !already_present {
                        equivocation_children.push(equivocation_child);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                None => Ok(false),
            },
            None => Ok(false),
        }
    }

    /// Walk the self-justification chain upward from `block`, returning the
    /// **oldest** ancestor authored by `target_validator` whose sequence
    /// number still exceeds `base_seq_num`. This is the canonical child of
    /// the equivocation base — the block we hold against the validator when
    /// deciding whether an equivocation has been observed by `block`'s
    /// causal cone.
    ///
    /// The `visited` set is a defensive cycle guard against
    /// byzantine-crafted self-justifications, **not** genuine DAG cycles —
    /// honest blocks form a strict DAG. A cycle here would loop forever
    /// without it; treat the first repeat as the end of the walk.
    fn find_canonical_creator_justification_child_above_seq(
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        target_validator: &Validator,
        base_seq_num: i64,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if block.sender != *target_validator || i64::from(block.seq_num) <= base_seq_num {
            return Ok(None);
        }

        let mut candidate_hash = block.block_hash.clone();
        let mut current_hash = block.block_hash.clone();
        let mut visited = HashSet::new();

        loop {
            if !visited.insert(current_hash.clone()) {
                break;
            }

            let Some(parent_hash) = dag.self_justification(&current_hash)? else {
                break;
            };

            match dag.lookup_unsafe(&parent_hash) {
                Ok(parent_metadata)
                    if parent_metadata.sender == *target_validator
                        && i64::from(parent_metadata.sequence_number) > base_seq_num =>
                {
                    candidate_hash = parent_hash.clone();
                    current_hash = parent_hash;
                }
                Ok(_) => break,
                // Storage failure during canonical-child walk: propagate
                // rather than absorbing as a normal walk termination — the
                // previous `_ => break` would silently truncate the search
                // and produce an incorrect canonical child, defeating the
                // detector's totality claim (T-9.11).
                Err(e) => return Err(e),
            }
        }

        Ok(Some(candidate_hash))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_hash;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, Justification,
    };
    use parking_lot::RwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;
    use crate::rust::util::proto_util;

    fn validator(id: u8) -> Validator { Bytes::from(vec![id]) }

    fn hash(id: u8) -> BlockHash { Bytes::from(vec![id; block_hash::LENGTH]) }

    fn block(
        sender: &Validator,
        seq_num: i32,
        block_hash: BlockHash,
        self_parent: Option<BlockHash>,
    ) -> BlockMessage {
        let justifications = self_parent
            .map(|latest_block_hash| {
                vec![Justification {
                    validator: sender.clone(),
                    latest_block_hash,
                }]
            })
            .unwrap_or_default();

        BlockMessage {
            block_hash,
            header: Header {
                parents_hash_list: Vec::new(),
                timestamp: 0,
                version: 0,
                extra_bytes: Bytes::new(),
                chain_id: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: Vec::new(),
                    block_number: i64::from(seq_num),
                },
                deploys: Vec::new(),
                rejected_deploys: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Bytes::new(),
                applied_from_scope: Vec::new(),
                merge_base: Bytes::new(),
            },
            justifications,
            sender: sender.clone(),
            seq_num,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    fn metadata(block: &BlockMessage, block_number: i64) -> BlockMetadata {
        BlockMetadata {
            block_hash: block.block_hash.clone(),
            parents: Vec::new(),
            sender: block.sender.clone(),
            justifications: block.justifications.clone(),
            weight_map: BTreeMap::new(),
            block_number,
            sequence_number: block.seq_num,
            equivocates: None,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            merge_base: Bytes::new(),
        }
    }

    fn dag_with(blocks: &[BlockMessage]) -> KeyValueDagRepresentation {
        let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let block_metadata_index = Arc::new(RwLock::new(BlockMetadataStore::new(metadata_store)));

        let mut dag = KeyValueDagRepresentation {
            dag_set: imbl::HashSet::new(),
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::new(),
            main_parent_map: imbl::HashMap::new(),
            self_justification_map: imbl::HashMap::new(),
            sequence_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: BlockHash::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index,
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
            carrier_index: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
            )),
        };

        for (index, block) in blocks.iter().enumerate() {
            let block_number = index as i64;
            dag.dag_set.insert(block.block_hash.clone());
            dag.block_number_map
                .insert(block.block_hash.clone(), block_number);
            dag.height_map
                .entry(block_number)
                .or_default()
                .insert(block.block_hash.clone());
            dag.sequence_map
                .entry((block.sender.clone(), block.seq_num))
                .or_default()
                .insert(block.block_hash.clone());
            if let Some(self_parent) = proto_util::creator_justification_block_message(block)
                .map(|justification| justification.latest_block_hash)
            {
                dag.self_justification_map
                    .insert(block.block_hash.clone(), self_parent);
            }
            dag.block_metadata_index
                .write()
                .add(metadata(block, block_number))
                .unwrap();
        }

        dag
    }

    fn block_store_with(blocks: &[BlockMessage]) -> KeyValueBlockStore {
        let store = KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        for block in blocks {
            store.put_block_message(block).unwrap();
        }
        store
    }

    /// v signs a1 <- a2 <- a3; its latest message is a3.
    fn chain_of_three() -> (KeyValueDagRepresentation, Validator, [BlockMessage; 3]) {
        let v = validator(1);
        let a1 = block(&v, 1, hash(11), None);
        let a2 = block(&v, 2, hash(12), Some(a1.block_hash.clone()));
        let a3 = block(&v, 3, hash(13), Some(a2.block_hash.clone()));
        let mut dag = dag_with(&[a1.clone(), a2.clone(), a3.clone()]);
        dag.latest_messages_map
            .insert(v.clone(), a3.block_hash.clone());
        (dag, v, [a1, a2, a3])
    }

    #[test]
    fn a_block_at_a_held_sequence_number_equivocates_with_the_held_block() {
        let (dag, v, [a1, a2, a3]) = chain_of_three();
        for (held, seq) in [(&a3, 3), (&a2, 2), (&a1, 1)] {
            let sibling = block(&v, seq, hash(20 + seq as u8), None);
            assert_eq!(
                EquivocationDetector::equivocation_of(&dag, &sibling),
                Some(held.block_hash.clone()),
                "seq {seq}"
            );
        }
    }

    #[test]
    fn a_held_block_a_new_sequence_number_or_another_sender_is_no_equivocation() {
        let (dag, v, [_, a2, a3]) = chain_of_three();
        let next = block(&v, 4, hash(14), Some(a3.block_hash.clone()));
        let other = block(&validator(2), 2, hash(32), None);
        assert_eq!(EquivocationDetector::equivocation_of(&dag, &a2), None);
        assert_eq!(EquivocationDetector::equivocation_of(&dag, &next), None);
        assert_eq!(EquivocationDetector::equivocation_of(&dag, &other), None);
    }

    #[test]
    fn the_verdict_is_admissible_only_when_requested_as_a_dependency() {
        let (dag, v, _) = chain_of_three();
        let sibling = block(&v, 2, hash(22), None);
        for (requested, expected) in [
            (true, InvalidBlock::AdmissibleEquivocation),
            (false, InvalidBlock::IgnorableEquivocation),
        ] {
            let verdict = EquivocationDetector::equivocation_verdict(&dag, &sibling, requested);
            assert_eq!(verdict, Either::Left(BlockError::Invalid(expected)));
        }
    }

    #[test]
    fn latest_messages_are_projected_in_validator_order() {
        let justifications = vec![
            Justification {
                validator: validator(3),
                latest_block_hash: hash(30),
            },
            Justification {
                validator: validator(1),
                latest_block_hash: hash(10),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: hash(20),
            },
        ];

        let latest_messages = EquivocationDetector::to_latest_message_hashes(&justifications);
        let validators: Vec<_> = latest_messages.keys().cloned().collect();

        assert_eq!(validators, vec![validator(1), validator(2), validator(3)]);
    }

    #[test]
    fn iterative_detection_skips_missing_latest_pointer_and_continues() {
        let sender = validator(1);
        let observer = validator(2);
        let missing = validator(3);
        let b0 = block(&sender, 0, hash(10), None);
        let left = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let right = block(&sender, 10, hash(30), Some(b0.block_hash.clone()));
        let mut observer_block = block(&observer, 1, hash(40), None);
        observer_block.justifications = vec![Justification {
            validator: sender.clone(),
            latest_block_hash: right.block_hash.clone(),
        }];

        let dag = dag_with(&[
            b0.clone(),
            left.clone(),
            right.clone(),
            observer_block.clone(),
        ]);
        let block_store = block_store_with(&[left.clone(), right.clone(), observer_block.clone()]);
        let latest_messages = BTreeMap::from([
            (missing, hash(99)),
            (observer, observer_block.block_hash.clone()),
            (sender.clone(), left.block_hash.clone()),
        ]);
        let mut cache = CanonicalChildCache::new();

        let detected = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &sender,
            0,
            &b0,
            &mut cache,
        )
        .unwrap();

        assert!(detected);
    }

    #[test]
    fn canonical_child_returns_oldest_visible_block_above_base() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b2 = block(&sender, 2, hash(20), Some(b0.block_hash.clone()));
        let b100 = block(&sender, 100, hash(30), Some(b2.block_hash.clone()));
        let dag = dag_with(&[b0, b2.clone(), b100.clone()]);

        let found = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b100, &sender, 0,
        )
        .unwrap();

        assert_eq!(found, Some(b2.block_hash));
    }

    #[test]
    fn canonical_child_collapses_same_branch_latest_messages() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b10 = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let b11 = block(&sender, 11, hash(30), Some(b10.block_hash.clone()));
        let dag = dag_with(&[b0, b10.clone(), b11.clone()]);

        let from_10 = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b10, &sender, 0,
        )
        .unwrap();
        let from_11 = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b11, &sender, 0,
        )
        .unwrap();

        assert_eq!(from_10, Some(b10.block_hash.clone()));
        assert_eq!(from_11, Some(b10.block_hash));
    }

    #[test]
    fn canonical_child_distinguishes_two_visible_branches() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let left = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let right = block(&sender, 10, hash(30), Some(b0.block_hash.clone()));
        let dag = dag_with(&[b0, left.clone(), right.clone()]);

        let left_found =
            EquivocationDetector::find_canonical_creator_justification_child_above_seq(
                &dag, &left, &sender, 0,
            )
            .unwrap();
        let right_found =
            EquivocationDetector::find_canonical_creator_justification_child_above_seq(
                &dag, &right, &sender, 0,
            )
            .unwrap();

        assert_eq!(left_found, Some(left.block_hash));
        assert_eq!(right_found, Some(right.block_hash));
        assert_ne!(left_found, right_found);
    }

    #[test]
    fn canonical_child_cycle_guard_terminates() {
        let sender = validator(1);
        let b2 = block(&sender, 2, hash(20), Some(hash(30)));
        let b3 = block(&sender, 3, hash(30), Some(hash(20)));
        let dag = dag_with(&[b2.clone(), b3.clone()]);

        let found = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b3, &sender, 0,
        )
        .unwrap();

        assert!(found.is_some());
    }

    #[test]
    fn canonical_child_cache_is_transparent_for_add_child() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b10 = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let b11 = block(&sender, 11, hash(30), Some(b10.block_hash.clone()));
        let dag = dag_with(&[b0, b10.clone(), b11.clone()]);
        let block_store = block_store_with(&[b10.clone(), b11.clone()]);
        let mut cache = CanonicalChildCache::new();

        // P2-11: helper mutates `children` in place; the boolean return
        // reflects whether anything was appended.
        let mut children: Vec<BlockMessage> = Vec::new();
        let added = EquivocationDetector::add_equivocation_child(
            &dag,
            &block_store,
            &b11,
            &sender,
            0,
            &mut children,
            &mut cache,
        )
        .unwrap();

        assert!(added);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].block_hash, b10.block_hash);
        assert_eq!(
            cache.get(&(b11.block_hash, sender, 0)).cloned(),
            Some(Some(children[0].block_hash.clone()))
        );
    }

    /// v signs `left` and `right` at seq 1 on genesis g and is held with the
    /// pair; `observer` has a block citing `right`. Returns the view and a
    /// block whose justifications cite `left` and, if `both`, the observer's.
    fn neglect_case(
        both: bool,
    ) -> (
        KeyValueDagRepresentation,
        KeyValueBlockStore,
        BlockMessage,
        BlockMessage,
        Validator,
    ) {
        let v = validator(1);
        let observer = validator(2);
        let g = block(&validator(9), 0, hash(10), None);
        let left = block(&v, 1, hash(20), None);
        let right = block(&v, 1, hash(30), None);
        let mut observed = block(&observer, 1, hash(40), None);
        observed.justifications = vec![Justification {
            validator: v.clone(),
            latest_block_hash: right.block_hash.clone(),
        }];
        let mut dag = dag_with(&[g.clone(), left.clone(), right.clone(), observed.clone()]);
        let mut held = metadata(&right, 1);
        held.equivocates = Some(left.block_hash.clone());
        dag.invalid_blocks_set.insert(held);
        let store = block_store_with(&[left.clone(), right.clone(), observed.clone()]);

        let mut judged = block(&validator(3), 1, hash(50), None);
        judged.justifications = vec![Justification {
            validator: v.clone(),
            latest_block_hash: left.block_hash.clone(),
        }];
        if both {
            judged.justifications.push(Justification {
                validator: observer,
                latest_block_hash: observed.block_hash.clone(),
            });
        }
        (dag, store, g, judged, v)
    }

    fn neglect_verdict(both: bool, stake: Option<i64>) -> ValidBlockProcessing {
        let (dag, store, genesis, judged, v) = neglect_case(both);
        let bonds: Vec<Bond> = stake
            .map(|stake| Bond {
                validator: v,
                stake,
            })
            .into_iter()
            .collect();
        EquivocationDetector::check_neglected_equivocations(&judged, &dag, &store, &genesis, &bonds)
            .unwrap()
    }

    #[test]
    fn a_block_that_sees_both_blocks_of_a_pair_must_slash_its_sender() {
        assert_eq!(
            neglect_verdict(true, Some(100)),
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
        );
    }

    #[test]
    fn a_block_that_sees_one_block_of_a_pair_owes_nothing() {
        assert_eq!(
            neglect_verdict(false, Some(100)),
            Either::Right(ValidBlock::Valid)
        );
    }

    #[test]
    fn a_block_that_leaves_the_sender_unbonded_owes_nothing() {
        assert_eq!(
            neglect_verdict(true, Some(0)),
            Either::Right(ValidBlock::Valid)
        );
        assert_eq!(
            neglect_verdict(true, None),
            Either::Right(ValidBlock::Valid)
        );
    }
}
