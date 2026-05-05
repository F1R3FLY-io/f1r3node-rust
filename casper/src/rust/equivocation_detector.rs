use std::collections::HashMap;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::equivocation_record::{EquivocationDiscoveryStatus, EquivocationRecord};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::dag::dag_ops;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::util::proto_util;
use crate::rust::ValidBlockProcessing;

/// Equivocation detection logic for blockchain consensus
pub struct EquivocationDetector;

impl EquivocationDetector {
    pub async fn check_equivocations(
        requested_as_dependency: bool,
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        tracing::info!("Calculate checkEquivocations.");

        let maybe_latest_message_of_creator_hash = dag.latest_message_hash(&block.sender);
        let maybe_creator_justification = Self::creator_justification_hash(block);
        let is_not_equivocation =
            maybe_creator_justification == maybe_latest_message_of_creator_hash;

        if is_not_equivocation {
            Ok(Either::Right(ValidBlock::Valid))
        } else if requested_as_dependency {
            Ok(Either::Left(BlockError::Invalid(
                InvalidBlock::AdmissibleEquivocation,
            )))
        } else {
            let sender = PrettyPrinter::build_string_no_limit(&block.sender);
            let creator_justification_hash = PrettyPrinter::build_string_no_limit(
                &maybe_creator_justification.unwrap_or_default(),
            );
            let latest_message_of_creator = PrettyPrinter::build_string_no_limit(
                &maybe_latest_message_of_creator_hash.unwrap_or_default(),
            );

            tracing::warn!(
                "Ignorable equivocation: sender is {}, creator justification is {}, latest message of creator is {}",
                sender,
                creator_justification_hash,
                latest_message_of_creator
            );

            Ok(Either::Left(BlockError::Invalid(
                InvalidBlock::IgnorableEquivocation,
            )))
        }
    }

    pub fn creator_justification_hash(block: &BlockMessage) -> Option<BlockHash> {
        proto_util::creator_justification_block_message(block)
            .map(|justification| justification.latest_block_hash)
    }

    pub async fn check_neglected_equivocations_with_update(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        genesis: &BlockMessage,
        block_dag_storage: &BlockDagKeyValueStorage,
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        tracing::info!("Calculate checkNeglectedEquivocationsWithUpdate");

        let neglected_equivocation_detected = Self::is_neglected_equivocation_detected_with_update(
            block,
            dag,
            block_store,
            genesis,
            block_dag_storage,
        )
        .await?;

        let status = if neglected_equivocation_detected {
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
        } else {
            Either::Right(ValidBlock::Valid)
        };

        Ok(status)
    }

    async fn is_neglected_equivocation_detected_with_update(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        genesis: &BlockMessage,
        block_dag_storage: &BlockDagKeyValueStorage,
    ) -> Result<bool, KvStoreError> {
        // Atomic read-modify-write on the equivocation tracker: read all
        // records, classify each against the new block, and update any
        // records whose witness set should grow — all under the global
        // lock so concurrent detectors do not interleave their updates.
        // See docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.2.
        block_dag_storage.access_equivocations_tracker(|tracker| {
            let equivocations = tracker.data()?;
            for equivocation_record in equivocations {
                let status = Self::get_equivocation_discovery_status(
                    block,
                    dag,
                    block_store,
                    &equivocation_record,
                    genesis,
                )?;
                match status {
                    EquivocationDiscoveryStatus::EquivocationNeglected => {
                        return Ok(true);
                    }
                    EquivocationDiscoveryStatus::EquivocationDetected => {
                        let mut updated = equivocation_record.clone();
                        updated
                            .equivocation_detected_block_hashes
                            .insert(block.block_hash.clone());
                        tracker.add(updated)?;
                        tracing::info!(
                            "Equivocation detected and tracker updated for block {}",
                            PrettyPrinter::build_string_no_limit(&block.block_hash)
                        );
                    }
                    EquivocationDiscoveryStatus::EquivocationOblivious => {}
                }
            }
            Ok(false)
        })
    }

    fn get_equivocation_discovery_status(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        genesis: &BlockMessage,
    ) -> Result<EquivocationDiscoveryStatus, KvStoreError> {
        let equivocating_validator = &equivocation_record.equivocator;
        let latest_messages = Self::to_latest_message_hashes(&block.justifications);
        let bonds = proto_util::bonds(block);

        // Find the bond for the equivocating validator
        let maybe_equivocating_validator_bond = bonds
            .iter()
            .find(|bond| bond.validator == equivocating_validator);

        match maybe_equivocating_validator_bond {
            Some(bond) => Self::get_equivocation_discovery_status_for_bonded_validator(
                dag,
                block_store,
                equivocation_record,
                &latest_messages,
                bond.stake,
                genesis,
            ),
            None => {
                /*
                 * Since block has dropped equivocatingValidator from the bonds, it has acknowledged the equivocation.
                 * The combination of Validate.transactions and Validate.bondsCache ensure that you can only drop
                 * validators through transactions to the proof of stake contract.
                 */
                Ok(EquivocationDiscoveryStatus::EquivocationDetected)
            }
        }
    }

    fn get_equivocation_discovery_status_for_bonded_validator(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        latest_messages: &HashMap<Validator, BlockHash>,
        stake: i64,
        genesis: &BlockMessage,
    ) -> Result<EquivocationDiscoveryStatus, KvStoreError> {
        if stake > 0 {
            let equivocation_detectable = Self::is_equivocation_detectable(
                dag,
                block_store,
                latest_messages,
                equivocation_record,
                &Vec::new(),
                genesis,
            )?;

            if equivocation_detectable {
                Ok(EquivocationDiscoveryStatus::EquivocationNeglected)
            } else {
                Ok(EquivocationDiscoveryStatus::EquivocationOblivious)
            }
        } else {
            // Bug #5 (post-fix): the PoS bond contract enforces
            // `amount > 0` (PoS.rhox `bond` arm), so this branch is
            // unreachable from a correctly-bonded validator. We
            // retain the conservative `EquivocationDetected`
            // classification as a defense-in-depth check; the
            // post-fix invariant `active_implies_bonded`
            // (formal/rocq/slashing/theories/BugFixStakeZero.v:36)
            // makes the branch dead code in practice. See
            // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.6.
            Ok(EquivocationDiscoveryStatus::EquivocationDetected)
        }
    }

    fn to_latest_message_hashes(
        justifications: &[models::rust::casper::protocol::casper_message::Justification],
    ) -> HashMap<Validator, BlockHash> {
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

    fn is_equivocation_detectable(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        latest_messages: &HashMap<Validator, BlockHash>,
        equivocation_record: &EquivocationRecord,
        equivocation_children: &Vec<BlockMessage>,
        genesis: &BlockMessage,
    ) -> Result<bool, KvStoreError> {
        let latest_messages_vec: Vec<(Validator, BlockHash)> = latest_messages
            .iter()
            .map(|(v, h)| (v.clone(), h.clone()))
            .collect();

        match latest_messages_vec.split_first() {
            None => Ok(false),
            Some(((_, justification_block_hash), remainder)) => {
                Self::is_equivocation_detectable_after_viewing_block(
                    dag,
                    block_store,
                    justification_block_hash,
                    equivocation_record,
                    equivocation_children,
                    remainder,
                    genesis,
                )
            }
        }
    }

    fn is_equivocation_detectable_after_viewing_block(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block_hash: &BlockHash,
        equivocation_record: &EquivocationRecord,
        equivocation_children: &Vec<BlockMessage>,
        remainder: &[(Validator, BlockHash)],
        genesis: &BlockMessage,
    ) -> Result<bool, KvStoreError> {
        if equivocation_record
            .equivocation_detected_block_hashes
            .contains(justification_block_hash)
        {
            Ok(true)
        } else {
            let justification_block = block_store.get_unsafe(justification_block_hash);
            Self::is_equivocation_detectable_through_children(
                dag,
                block_store,
                equivocation_record,
                equivocation_children,
                remainder,
                &justification_block,
                genesis,
            )
        }
    }

    fn is_equivocation_detectable_through_children(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        equivocation_children: &Vec<BlockMessage>,
        remainder: &[(Validator, BlockHash)],
        justification_block: &BlockMessage,
        genesis: &BlockMessage,
    ) -> Result<bool, KvStoreError> {
        let equivocating_validator = &equivocation_record.equivocator;
        let equivocation_base_block_seq_num = equivocation_record.equivocation_base_block_seq_num;

        let updated_equivocation_children = Self::maybe_add_equivocation_child(
            dag,
            block_store,
            justification_block,
            equivocating_validator,
            equivocation_base_block_seq_num.into(),
            equivocation_children,
            genesis,
        )?;

        if updated_equivocation_children.len() > 1 {
            Ok(true)
        } else {
            let remainder_map: HashMap<Validator, BlockHash> = remainder
                .iter()
                .map(|(v, h)| (v.clone(), h.clone()))
                .collect();

            Self::is_equivocation_detectable(
                dag,
                block_store,
                &remainder_map,
                equivocation_record,
                &updated_equivocation_children,
                genesis,
            )
        }
    }

    fn maybe_add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &Vec<BlockMessage>,
        genesis: &BlockMessage,
    ) -> Result<Vec<BlockMessage>, KvStoreError> {
        // Genesis termination: `block_hash` equality is the canonical
        // genesis-detection predicate, and it is the SAFEST check
        // available. Three reasons:
        //
        // (1) Hash uniqueness. Block hashes are
        //     `BLAKE2b-512(canonicalize(BlockMessage))`. Two distinct
        //     blocks have equal hashes only via cryptographic
        //     collision (negligible). Any field-based predicate
        //     (e.g. `seq_num == 0`, `parents.is_empty()`) is either
        //     implied by hash equality (so equivalent) or admits
        //     spoofing under partition recovery.
        //
        // (2) Genesis singularity. Exactly one `genesis: &BlockMessage`
        //     reference flows into this function from
        //     `MultiParentCasperImpl::genesis_block()`. The block
        //     store is the single source of truth.
        //
        // (3) Equivalence with the BFS termination at
        //     `find_creator_justification_descendant_above_seq`
        //     (lines 416-455). That BFS terminates when
        //     `proto_util::get_creator_justification_as_list_until_goal_in_memory`
        //     returns an empty list — which happens exactly at
        //     genesis (no creator-justification). Both paths terminate
        //     equivalently per Theorem T-9.7
        //     (`t_9_7_finds_descendant_with_gap`,
        //     formal/rocq/slashing/theories/BugFixSeqNumDensity.v:84).
        //
        // See docs/theory/slashing/design/04-detection-and-pipeline.md
        // §4.7 (genesis-termination invariant) for the full proof.
        if justification_block.block_hash == genesis.block_hash {
            return Ok(equivocation_children.clone());
        }

        if justification_block.sender == *equivocating_validator {
            let justification_seq_num = i64::from(justification_block.seq_num);
            if justification_seq_num > equivocation_base_block_seq_num {
                Self::add_equivocation_child(
                    dag,
                    block_store,
                    justification_block,
                    equivocation_base_block_seq_num,
                    equivocation_children,
                )
            } else {
                Ok(equivocation_children.clone())
            }
        } else {
            let latest_messages =
                Self::to_latest_message_hashes(&justification_block.justifications);

            match latest_messages.get(equivocating_validator) {
                Some(latest_equivocating_validator_block_hash) => {
                    let latest_equivocating_validator_block =
                        block_store.get_unsafe(latest_equivocating_validator_block_hash);

                    let latest_seq_num = i64::from(latest_equivocating_validator_block.seq_num);
                    if latest_seq_num > equivocation_base_block_seq_num {
                        Self::add_equivocation_child(
                            dag,
                            block_store,
                            &latest_equivocating_validator_block,
                            equivocation_base_block_seq_num,
                            equivocation_children,
                        )
                    } else {
                        Ok(equivocation_children.clone())
                    }
                }
                None => {
                    Err(KvStoreError::KeyNotFound(
                        "justificationBlock is missing justification pointers to equivocatingValidator even though justificationBlock isn't a part of equivocationDetectedBlockHashes for this equivocation record.".to_string()
                    ))
                }
            }
        }
    }

    fn add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &Vec<BlockMessage>,
    ) -> Result<Vec<BlockMessage>, KvStoreError> {
        // Walk the creator-justification chain looking for any block whose
        // sequence number is *strictly greater* than the equivocation base.
        // The earlier `baseSeqNum + 1` exact-match assumed seq numbers are
        // dense (never skipped); under partition recovery a validator may
        // legitimately skip a number, breaking that assumption.
        // See docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.7
        // (theorem t_9_7_finds_descendant_with_gap).
        match Self::find_creator_justification_descendant_above_seq(
            dag,
            justification_block,
            equivocation_base_block_seq_num,
        )? {
            Some(equivocation_child_hash) => {
                let equivocation_child = block_store.get_unsafe(&equivocation_child_hash);
                let mut updated_children = equivocation_children.clone();
                updated_children.push(equivocation_child);
                Ok(updated_children)
            }
            None => {
                Err(KvStoreError::KeyNotFound(
                    "creator-justification descendant with sequence number above equivocation base hasn't been added to the blockDAG yet.".to_string()
                ))
            }
        }
    }

    fn find_creator_justification_descendant_above_seq(
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        base_seq_num: i64,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if i64::from(block.seq_num) > base_seq_num {
            return Ok(Some(block.block_hash.clone()));
        }

        let start_nodes = vec![block.block_hash.clone()];

        let neighbors = |block_hash: &BlockHash| -> Vec<BlockHash> {
            proto_util::get_creator_justification_as_list_until_goal_in_memory(
                dag,
                block_hash,
                |_| false,
            )
            .unwrap_or_else(|_| Vec::new())
        };

        let traversal_result = dag_ops::bf_traverse(start_nodes, neighbors);
        let target_validator = &block.sender;

        for candidate_hash in traversal_result {
            match dag.lookup_unsafe(&candidate_hash) {
                Ok(candidate_metadata) => {
                    if i64::from(candidate_metadata.sequence_number) > base_seq_num
                        && candidate_metadata.sender == *target_validator
                    {
                        return Ok(Some(candidate_hash));
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }

        Ok(None)
    }
}
