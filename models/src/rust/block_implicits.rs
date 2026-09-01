// See models/src/test/scala/coop/rchain/models/blockImplicits.scala

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use prost::bytes::Bytes as ByteString;
use rand::prelude::*;

use super::block::state_hash::{self, StateHash};
use super::block_hash::{self, BlockHash, BlockHashSerde};
use super::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION;
use super::bond_generation::BondGeneration;
use super::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, FinalizationCertificate, Header,
    Justification, ProcessedDeploy, ProcessedSystemDeploy, ValidatorBondGeneration,
};
use super::validator::{self, Validator, ValidatorSerde};
use crate::rhoapi::PCost;

const CERTIFIED_VALIDATOR_INCARNATION_PROTOCOL_VERSION: i64 = 5;

fn ensure_certified_floor_commitment(block: &mut BlockMessage) {
    if block.header.version < CERTIFIED_ADMISSION_PROTOCOL_VERSION
        || block.body.state.block_number == 0
        || block.header.finalized_floor.is_some()
    {
        return;
    }
    let floor_hash = block
        .header
        .parents_hash_list
        .first()
        .cloned()
        .unwrap_or_else(|| block.block_hash.clone());
    let floor_post_state_hash = block.body.state.pre_state_hash.clone();
    let mut authority_preimage = b"f1r3fly-test-certified-consensus-context-v1".to_vec();
    authority_preimage.extend_from_slice(&floor_hash);
    authority_preimage.extend_from_slice(&floor_post_state_hash);
    let authority_context_digest: ByteString = Blake2b256::hash(authority_preimage).into();
    let manifest = std::collections::BTreeSet::from([BlockHashSerde(floor_hash.clone())]);
    let certificate = FinalizationCertificate {
        schema_version: FinalizationCertificate::SCHEMA_VERSION,
        protocol_version: block.header.version,
        shard_id: block.shard_id.clone(),
        genesis_hash: BlockHashSerde(floor_hash.clone()),
        predecessor_floor_hash: BlockHashSerde(floor_hash.clone()),
        predecessor_certificate_digest: BlockHashSerde(ByteString::from(vec![
            0;
            block_hash::LENGTH
        ])),
        predecessor_certificate_block_hash: BlockHashSerde(ByteString::from(vec![
            0;
            block_hash::LENGTH
        ])),
        target_floor_hash: BlockHashSerde(floor_hash),
        target_post_state_hash: BlockHashSerde(floor_post_state_hash),
        target_block_number: block.body.state.block_number.saturating_sub(1),
        fault_tolerance_numerator: 0,
        fault_tolerance_denominator: 1,
        exact_latest_messages: std::collections::BTreeMap::from([(
            ValidatorSerde(block.sender.clone()),
            BlockHashSerde(block.block_hash.clone()),
        )]),
        authority_context_digest: BlockHashSerde(authority_context_digest.clone()),
        supporting_manifest_digest: FinalizationCertificate::supporting_digest(&manifest),
        finalized_manifest_digest: FinalizationCertificate::finalized_digest(&manifest),
        supporting_block_count: 1,
        finalized_block_count: 1,
    };
    block.header.finalized_floor = Some(certificate.commitment(authority_context_digest));
    block.finalized_floor_certificate = Some(certificate);
}

pub fn block_hash_gen() -> impl Strategy<Value = BlockHash> {
    prop::collection::vec(any::<u8>(), block_hash::LENGTH)
        .prop_map(|byte_vec| prost::bytes::Bytes::from(byte_vec))
}

pub fn state_hash_gen() -> impl Strategy<Value = StateHash> {
    prop::collection::vec(any::<u8>(), state_hash::LENGTH)
        .prop_map(|byte_vec| prost::bytes::Bytes::from(byte_vec))
}

pub fn validator_gen() -> impl Strategy<Value = Validator> {
    prop::collection::vec(any::<u8>(), validator::LENGTH)
        .prop_map(|byte_vec| prost::bytes::Bytes::from(byte_vec))
}

pub fn bond_gen() -> impl Strategy<Value = Bond> {
    let validator_gen = prop::collection::vec(any::<u8>(), validator::LENGTH)
        .prop_map(|byte_vec| prost::bytes::Bytes::from(byte_vec));
    let stake_gen = 1i64..=1024i64;
    (validator_gen, stake_gen).prop_map(|(validator, stake)| Bond { validator, stake })
}

pub fn justification_gen() -> impl Strategy<Value = Justification> {
    let validator_gen = prop::collection::vec(any::<u8>(), validator::LENGTH)
        .prop_map(|byte_vec| prost::bytes::Bytes::from(byte_vec));
    let block_hash_gen = block_hash_gen();
    (validator_gen, block_hash_gen).prop_map(|(validator, latest_block_hash)| Justification {
        validator: validator.into(),
        latest_block_hash: latest_block_hash.into(),
    })
}

fn alpha_num_char() -> impl Strategy<Value = char> {
    prop::char::ranges(vec!['a'..='z', 'A'..='Z', '0'..='9'].into_iter().collect())
}

fn deploy_data_gen() -> impl Strategy<Value = DeployData> {
    let term_length = 32..=1024;
    let term = prop::collection::vec(alpha_num_char(), term_length)
        .prop_map(|chars| chars.into_iter().collect::<String>());

    (any::<i64>(), term, any::<String>()).prop_map(|(timestamp, term, shard_id)| DeployData {
        time_stamp: timestamp,
        valid_after_block_number: 1,
        term,
        language: "rholang".to_string(),
        shard_id,
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    })
}

pub fn signed_deploy_data_gen() -> impl Strategy<Value = Signed<DeployData>> {
    deploy_data_gen().prop_map(|deploy_data| {
        let secp256k1 = Secp256k1;
        let (sec, _) = secp256k1.new_key_pair();

        Signed::create(deploy_data, Box::new(secp256k1), sec)
            .expect("Failed to create signed deploy data")
    })
}

pub fn processed_deploy_gen() -> impl Strategy<Value = ProcessedDeploy> {
    let deploy_data_gen = signed_deploy_data_gen();
    deploy_data_gen.prop_map(|deploy_data| ProcessedDeploy {
        deploy: deploy_data,
        envelope_commitment: ByteString::new(),
        cost: PCost { cost: 0 },
        deploy_log: Vec::new(),
        is_failed: false,
        system_deploy_error: None,
        cosigners: Vec::new(),
        cosigner_threshold: 0,
        pre_state_hash: ByteString::new(),
        post_state_hash: ByteString::new(),
        authority_funding_certificate: None,
        authority_cost_witness: None,
        admission_status: Default::default(),
    })
}

pub fn protocol_v6_processed_deploy_gen() -> impl Strategy<Value = ProcessedDeploy> {
    let term = prop::collection::vec(alpha_num_char(), 32..=1024)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    let shard_id = prop::collection::vec(alpha_num_char(), 1..=64)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    (0..=i64::MAX, term, shard_id).prop_map(|(timestamp, term, shard_id)| {
        let secp256k1 = Secp256k1;
        let (secret, _) = secp256k1.new_key_pair();
        let envelope = Cosigned::create_single_envelope(
            DeployData {
                time_stamp: timestamp,
                valid_after_block_number: 1,
                term,
                language: "rholang".to_string(),
                shard_id,
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            Box::new(secp256k1),
            secret,
        )
        .expect("Failed to create protocol-v6 deploy envelope");
        ProcessedDeploy::empty_from_cosigned(&envelope)
    })
}

pub fn block_element_gen(
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
) -> impl Strategy<Value = BlockMessage> {
    let version = set_version.unwrap_or(CERTIFIED_ADMISSION_PROTOCOL_VERSION);
    // Generate individual components using existing or provided values
    let pre_state_hash_gen = match set_pre_state_hash {
        Some(hash) => Just(hash).boxed(),
        None => state_hash_gen().boxed(),
    };

    let post_state_hash_gen = match set_post_state_hash {
        Some(hash) => Just(hash).boxed(),
        None => state_hash_gen().boxed(),
    };

    let parents_hash_list_gen = match set_parents_hash_list {
        Some(list) => Just(list).boxed(),
        None => prop::collection::vec(block_hash_gen(), 0..5).boxed(),
    };

    let justifications_gen = match set_justifications {
        Some(list) => Just(list).boxed(),
        None => prop::collection::vec(justification_gen(), 0..5).boxed(),
    };

    let deploys_gen = match set_deploys {
        Some(list) => Just(list).boxed(),
        None if version >= CERTIFIED_ADMISSION_PROTOCOL_VERSION => {
            prop::collection::vec(protocol_v6_processed_deploy_gen(), 0..5).boxed()
        }
        None => prop::collection::vec(processed_deploy_gen(), 0..5).boxed(),
    };

    let bonds_gen = match set_bonds {
        Some(list) => Just(list).boxed(),
        None => prop::collection::vec(bond_gen(), 10).boxed(),
    };

    let validator_gen = match set_validator {
        Some(v) => Just(v).boxed(),
        None => bonds_gen
            .clone()
            .prop_map(|bonds| {
                let mut rng = rand::rng();
                bonds
                    .choose(&mut rng)
                    .map(|bond| bond.validator.clone())
                    .map(|b| b)
                    .unwrap_or_else(|| {
                        validator_gen()
                            .boxed()
                            .new_tree(&mut TestRunner::default())
                            .unwrap()
                            .current()
                    })
            })
            .boxed(),
    };

    let timestamp_gen = match set_timestamp {
        Some(t) => Just(t).boxed(),
        None => any::<i64>().boxed(),
    };
    let shard_id = set_shard_id.unwrap_or_else(|| "root".to_string());
    let block_number = set_block_number.unwrap_or(0);
    let seq_number = set_seq_number.unwrap_or(0);

    (
        pre_state_hash_gen,
        post_state_hash_gen,
        parents_hash_list_gen,
        justifications_gen,
        deploys_gen,
        bonds_gen,
        validator_gen,
        timestamp_gen,
    )
        .prop_map(
            move |(
                pre_state_hash,
                post_state_hash,
                parents_hash_list,
                justifications,
                deploys,
                bonds,
                validator,
                timestamp,
            )| {
                let mut bond_generations =
                    if version >= CERTIFIED_VALIDATOR_INCARNATION_PROTOCOL_VERSION {
                        bonds
                            .iter()
                            .map(|bond| ValidatorBondGeneration {
                                validator: bond.validator.clone(),
                                generation: BondGeneration::GENESIS,
                            })
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                bond_generations.sort_by(|left, right| left.validator.cmp(&right.validator));
                bond_generations.dedup_by(|left, right| left.validator == right.validator);
                let active_validators = bond_generations
                    .iter()
                    .map(|generation| generation.validator.clone())
                    .collect();
                let block = BlockMessage {
                    block_hash: prost::bytes::Bytes::new(),
                    header: Header {
                        parents_hash_list: parents_hash_list.into_iter().map(Into::into).collect(),
                        timestamp,
                        version,
                        extra_bytes: prost::bytes::Bytes::new(),
                        sender_bond_generation: (version
                            >= CERTIFIED_VALIDATOR_INCARNATION_PROTOCOL_VERSION)
                            .then_some(BondGeneration::GENESIS),
                        objective_equivocation_evidence_delta: Vec::new(),
                        finalized_floor: None,
                    },
                    body: Body {
                        state: F1r3flyState {
                            pre_state_hash,
                            post_state_hash,
                            bonds,
                            bond_generations,
                            active_validators,
                            block_number,
                        },
                        deploys,
                        system_deploys: set_sys_deploys.clone().unwrap_or_default(),
                        rejected_deploys: Vec::new(),
                        rejected_state_effects: Vec::new(),
                        extra_bytes: prost::bytes::Bytes::new(),
                        applied_from_scope: Vec::new(),
                        merge_base: prost::bytes::Bytes::new(),
                    },
                    justifications,
                    sender: validator.into(),
                    seq_num: seq_number,
                    sig: prost::bytes::Bytes::new(),
                    sig_algorithm: String::new(),
                    shard_id: shard_id.clone(),
                    extra_bytes: prost::bytes::Bytes::new(),
                    finalized_floor_certificate: None,
                };

                // Apply custom hash function if provided, otherwise generate random hash
                let block_hash = match hash_f.as_ref() {
                    Some(f) => f(block.clone()),
                    None => block_hash_gen()
                        .new_tree(&mut TestRunner::default())
                        .unwrap()
                        .current(),
                };

                let mut block = BlockMessage {
                    block_hash: block_hash.into(),
                    ..block
                };
                ensure_certified_floor_commitment(&mut block);
                block
            },
        )
}

pub fn block_elements_with_parents_gen(
    genesis: BlockMessage,
    min_size: usize,
    max_size: usize,
) -> impl Strategy<Value = Vec<BlockMessage>> {
    let bonds = genesis.body.state.bonds.clone();
    prop::collection::vec(
        (
            block_element_gen(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(bonds),
                None,
                None,
            ),
            any::<u64>(),
        ),
        min_size..max_size,
    )
    .prop_map(move |generated| {
        let mut blocks: Vec<BlockMessage> = Vec::with_capacity(generated.len());
        for (mut block, selector) in generated {
            let mut available = Vec::with_capacity(blocks.len() + 1);
            available.push((genesis.block_hash.clone(), genesis.body.state.block_number));
            available.extend(
                blocks
                    .iter()
                    .map(|parent| (parent.block_hash.clone(), parent.body.state.block_number)),
            );
            let mut parents = available
                .iter()
                .enumerate()
                .filter(|(index, _)| selector & (1_u64 << (index % 64)) != 0)
                .map(|(_, parent)| parent.clone())
                .collect::<Vec<_>>();
            if parents.is_empty() {
                parents.push(available[selector as usize % available.len()].clone());
            }
            block.header.parents_hash_list = parents.iter().map(|(hash, _)| hash.clone()).collect();
            block.body.state.block_number = parents
                .iter()
                .map(|(_, height)| *height)
                .max()
                .unwrap_or(genesis.body.state.block_number)
                + 1;
            block.header.finalized_floor = None;
            block.finalized_floor_certificate = None;
            ensure_certified_floor_commitment(&mut block);
            blocks.push(block);
        }
        blocks
    })
}

pub fn block_with_new_hashes_gen(
    block_elements: Vec<BlockMessage>,
) -> impl Strategy<Value = Vec<BlockMessage>> {
    prop::collection::vec(block_hash_gen(), block_elements.len()).prop_map(move |new_hashes| {
        block_elements
            .iter()
            .zip(new_hashes)
            .map(|(block, hash)| BlockMessage {
                block_hash: hash.into(),
                ..block.clone()
            })
            .collect()
    })
}

pub fn get_random_block(
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
    block_element_gen(
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
    )
    .new_tree(&mut TestRunner::default())
    .unwrap()
    .current()
}

pub fn get_random_block_default() -> BlockMessage {
    get_random_block(
        None, None, None, None, None, None, None, None, None, None, None, None, None, None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_incarnation_fields_persist_in_later_protocol_versions() {
        let validator = ByteString::from(vec![3; validator::LENGTH]);
        let block = get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            Some(validator.clone()),
            Some(6),
            Some(0),
            Some(vec![ByteString::from(vec![4; block_hash::LENGTH])]),
            None,
            None,
            None,
            Some(vec![Bond {
                validator: validator.clone(),
                stake: 1,
            }]),
            Some("root".to_string()),
            None,
        );

        assert_eq!(
            block.header.sender_bond_generation,
            Some(BondGeneration::GENESIS)
        );
        assert_eq!(block.body.state.active_validators, vec![validator.clone()]);
        assert_eq!(block.body.state.bond_generations.len(), 1);
        assert_eq!(block.body.state.bond_generations[0].validator, validator);
        assert_eq!(
            block.body.state.bond_generations[0].generation,
            BondGeneration::GENESIS
        );
    }
}
