// See casper/src/main/scala/coop/rchain/casper/LastFinalizedHeightConstraintChecker.scala

use super::blocks::proposer::propose_result::CheckProposeConstraintsResult;
use super::casper::CasperSnapshot;
use super::errors::CasperError;
use super::validator_identity::ValidatorIdentity;

pub fn check(
    snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
) -> Result<CheckProposeConstraintsResult, CasperError> {
    let validator = validator_identity.public_key.bytes.clone();
    let last_finalized_block_hash = snapshot.dag.last_finalized_block();
    let height_constraint_threshold = snapshot
        .on_chain_state
        .shard_conf
        .height_constraint_threshold;

    let last_finalized_block = snapshot.dag.lookup_unsafe(&last_finalized_block_hash)?;
    let latest_message_block_number = match snapshot.dag.latest_message_hash(&validator) {
        Some(hash) if snapshot.dag.canonical_genesis_hash() == Some(&hash) => 0,
        Some(hash) => snapshot.dag.lookup_unsafe(&hash)?.block_number,
        None => {
            tracing::debug!(
                target: "f1r3fly.casper.proposer",
                "Height constraint: proposer has no latest message yet; skipping propose"
            );
            return Ok(CheckProposeConstraintsResult::not_bonded());
        }
    };

    let height_difference = latest_message_block_number - last_finalized_block.block_number;
    let global_height_difference = snapshot.max_block_num - last_finalized_block.block_number;

    tracing::info!(
        "Height constraint check: validator_height_diff={}, global_height_diff={}, threshold={}",
        height_difference,
        global_height_difference,
        height_constraint_threshold
    );

    if height_difference <= height_constraint_threshold {
        Ok(CheckProposeConstraintsResult::success())
    } else {
        Ok(CheckProposeConstraintsResult::too_far_ahead_of_last_finalized())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crypto::rust::private_key::PrivateKey;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::bond_generation::BondGeneration;
    use models::rust::casper::protocol::casper_message::Bond;
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::blocks::proposer::propose_result::CheckProposeConstraintsResult;
    use crate::rust::casper::test_helpers::TestCasperWithSnapshot;
    use crate::rust::errors::CasperError;
    use crate::rust::synchrony_constraint_checker;

    fn hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; models::rust::block_hash::LENGTH]) }

    fn snapshots() -> (ValidatorIdentity, CasperSnapshot, CasperSnapshot) {
        let identity = ValidatorIdentity::new(&PrivateKey::from_bytes(&[17; 32]));
        let validator = identity.public_key.bytes.clone();
        let genesis_hash = hash(1);
        let finalized_hash = hash(2);
        let mut finalized = models::rust::block_implicits::get_random_block_default();
        finalized.block_hash = finalized_hash.clone();
        finalized.body.state.block_number = 5;
        finalized.sender = validator.clone();
        finalized.seq_num = 5;
        finalized.header.version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        finalized.header.sender_bond_generation = Some(BondGeneration::GENESIS);
        let finalized_metadata = crate::rust::test_metadata::certify(
            BlockMetadata {
                block_hash: finalized_hash.clone(),
                post_state_hash: finalized.body.state.post_state_hash.clone(),
                parents: Vec::new(),
                sender: validator.clone(),
                justifications: Vec::new(),
                weight_map: BTreeMap::from([(validator.clone(), 1)]),
                bond_generation_map: BTreeMap::from([(validator.clone(), BondGeneration::GENESIS)]),
                active_validator_set: BTreeSet::from([validator.clone()]),
                block_number: 5,
                sequence_number: 5,
                admission_outcome: None,
                directly_finalized: true,
                finalized: true,
                fault_tolerance_value: 1.0,
                successful_state_effect_indices: BTreeSet::new(),
                rejected_state_effects: BTreeSet::new(),
                applied_state_effects: BTreeSet::new(),
                protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                objective_equivocation_evidence_delta: Vec::new(),
                sender_authority: None,
                finalized_floor_commitment: None,
                admission_schema_version: models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
                approved_genesis: false,
                merge_base: Bytes::new(),
            },
            BondGeneration::GENESIS,
        );
        let mut genesis = models::rust::block_implicits::get_random_block_default();
        genesis.block_hash = genesis_hash.clone();
        genesis.header.parents_hash_list.clear();
        genesis.header.sender_bond_generation = None;
        genesis.body.state.block_number = 0;
        genesis.seq_num = 0;
        genesis.justifications.clear();
        let genesis_metadata = BlockMetadata::from_approved_genesis(&genesis).unwrap();

        let mut full = TestCasperWithSnapshot::create_empty_snapshot();
        full.dag
            .block_metadata_index
            .write()
            .add(genesis_metadata)
            .unwrap();
        full.dag
            .block_metadata_index
            .write()
            .add(finalized_metadata)
            .unwrap();
        full.dag.dag_set.insert(genesis_hash.clone());
        full.dag.dag_set.insert(finalized_hash.clone());
        full.dag.block_number_map.insert(genesis_hash.clone(), 0);
        full.dag.block_number_map.insert(finalized_hash.clone(), 5);
        full.dag.canonical_genesis_hash = Some(genesis_hash.clone());
        full.dag
            .latest_messages_map
            .insert(validator.clone(), genesis_hash.clone());
        full.dag.last_finalized_block_hash = finalized_hash.clone();
        full.last_finalized_block = finalized_hash;
        full.max_block_num = 5;
        full.finalized_floor_bonds = vec![Bond {
            validator: validator.clone(),
            stake: 1,
        }];
        full.on_chain_state.bonds_map.insert(validator.clone(), 1);
        full.on_chain_state
            .bond_generations
            .insert(validator.clone(), BondGeneration::GENESIS);
        full.on_chain_state.active_validators = vec![validator];

        let mut restored = full.clone();
        restored.dag.dag_set.remove(&genesis_hash);
        restored.dag.block_number_map.remove(&genesis_hash);
        (identity, full, restored)
    }

    #[tokio::test]
    async fn first_proposal_constraints_are_invariant_when_genesis_body_is_omitted() {
        let (identity, full, restored) = snapshots();

        assert!(matches!(
            check(&full, &identity),
            Ok(CheckProposeConstraintsResult::Success)
        ));
        assert!(matches!(
            check(&restored, &identity),
            Ok(CheckProposeConstraintsResult::Success)
        ));
        assert!(matches!(
            synchrony_constraint_checker::check(&full, &identity).await,
            Ok(CheckProposeConstraintsResult::Success)
        ));
        assert!(matches!(
            synchrony_constraint_checker::check(&restored, &identity).await,
            Ok(CheckProposeConstraintsResult::Success)
        ));
    }

    #[tokio::test]
    async fn first_proposal_constraints_fail_for_noncanonical_missing_latest_message() {
        let (identity, _, mut restored) = snapshots();
        let missing = hash(3);
        restored
            .dag
            .latest_messages_map
            .insert(identity.public_key.bytes.clone(), missing.clone());

        assert!(matches!(
            check(&restored, &identity),
            Err(CasperError::BlockNotHeld(hash)) if hash == missing
        ));
        assert!(matches!(
            synchrony_constraint_checker::check(&restored, &identity).await,
            Err(CasperError::BlockNotHeld(hash)) if hash == missing
        ));
    }
}
