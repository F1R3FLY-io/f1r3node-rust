// See models/src/main/scala/coop/rchain/models/BlockMetadata.scala

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use prost::bytes::Bytes;
use prost::Message;

use super::casper::protocol::casper_message::{
    BlockMessage, F1r3flyState, Justification, ProcessedSystemDeploy, StateEffectId,
};
use crate::casper::{BlockMetadataInternal, BondProto};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlockMetadata {
    #[serde(with = "shared::rust::serde_bytes")]
    pub block_hash: Bytes,
    #[serde(with = "shared::rust::serde_vec_bytes")]
    pub parents: Vec<Bytes>,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sender: Bytes,
    pub justifications: Vec<Justification>,
    #[serde(with = "shared::rust::serde_btreemap_bytes_i64")]
    pub weight_map: BTreeMap<Bytes, i64>,
    pub block_number: i64,
    pub sequence_number: i32,
    pub invalid: bool,
    pub directly_finalized: bool,
    pub finalized: bool,
    pub fault_tolerance_value: f32,
    pub successful_state_effect_indices: BTreeSet<u32>,
    pub rejected_state_effects: BTreeSet<StateEffectId>,
    pub protocol_version: i64,
}

impl PartialEq for BlockMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.block_hash == other.block_hash
            && self.parents == other.parents
            && self.sender == other.sender
            && self.justifications == other.justifications
            && self.weight_map == other.weight_map
            && self.block_number == other.block_number
            && self.sequence_number == other.sequence_number
            && self.invalid == other.invalid
            && self.directly_finalized == other.directly_finalized
            && self.finalized == other.finalized
            && self.successful_state_effect_indices == other.successful_state_effect_indices
            && self.rejected_state_effects == other.rejected_state_effects
            && self.protocol_version == other.protocol_version
    }
}

impl Eq for BlockMetadata {}

impl std::hash::Hash for BlockMetadata {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.block_hash.hash(state);
        self.parents.hash(state);
        self.sender.hash(state);
        self.justifications.hash(state);
        self.weight_map.iter().for_each(|(k, v)| {
            k.hash(state);
            v.hash(state);
        });
        self.block_number.hash(state);
        self.sequence_number.hash(state);
        self.invalid.hash(state);
        self.directly_finalized.hash(state);
        self.finalized.hash(state);
        self.successful_state_effect_indices.hash(state);
        self.rejected_state_effects.hash(state);
        self.protocol_version.hash(state);
    }
}

impl BlockMetadata {
    pub fn from_proto(proto: BlockMetadataInternal) -> Self {
        BlockMetadata {
            block_hash: proto.block_hash,
            parents: proto.parents,
            sender: proto.sender,
            justifications: proto
                .justifications
                .into_iter()
                .map(|j| Justification::from_proto(j))
                .collect(),
            weight_map: proto
                .bonds
                .into_iter()
                .map(|b| (b.validator.into(), b.stake))
                .collect(),
            block_number: proto.block_num,
            sequence_number: proto.seq_num,
            invalid: proto.invalid,
            directly_finalized: proto.directly_finalized,
            finalized: proto.finalized,
            fault_tolerance_value: proto.fault_tolerance_value,
            successful_state_effect_indices: proto
                .successful_state_effect_indices
                .into_iter()
                .collect(),
            rejected_state_effects: proto
                .rejected_state_effects
                .into_iter()
                .map(StateEffectId::from_proto)
                .collect(),
            protocol_version: proto.protocol_version,
        }
    }

    pub fn to_proto(&self) -> BlockMetadataInternal {
        BlockMetadataInternal {
            block_hash: self.block_hash.clone(),
            parents: self.parents.clone(),
            sender: self.sender.clone(),
            justifications: self.justifications.iter().map(|j| j.to_proto()).collect(),
            bonds: self
                .weight_map
                .iter()
                .map(|(v, s)| BondProto {
                    validator: v.clone(),
                    stake: *s,
                })
                .collect(),
            block_num: self.block_number,
            seq_num: self.sequence_number,
            invalid: self.invalid,
            directly_finalized: self.directly_finalized,
            finalized: self.finalized,
            fault_tolerance_value: self.fault_tolerance_value,
            successful_state_effect_indices: self
                .successful_state_effect_indices
                .iter()
                .copied()
                .collect(),
            rejected_state_effects: self
                .rejected_state_effects
                .iter()
                .map(StateEffectId::to_proto)
                .collect(),
            protocol_version: self.protocol_version,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> { self.to_proto().encode_to_vec() }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let proto =
            BlockMetadataInternal::decode(bytes).expect("Failed to decode BlockMetadataInternal");
        Self::from_proto(proto)
    }

    fn bytes_ordering(left: &Bytes, right: &Bytes) -> Ordering { left.iter().cmp(right.iter()) }

    pub fn ordering_by_num(left: &BlockMetadata, right: &BlockMetadata) -> Ordering {
        match left.block_number.cmp(&right.block_number) {
            Ordering::Equal => Self::bytes_ordering(&left.block_hash, &right.block_hash),
            other => other,
        }
    }

    fn weight_map(state: &F1r3flyState) -> BTreeMap<Bytes, i64> {
        state
            .bonds
            .iter()
            .map(|b| (b.validator.clone(), b.stake))
            .collect()
    }

    pub fn from_block(
        b: &BlockMessage,
        invalid: bool,
        directly_finalized: Option<bool>,
        finalized: Option<bool>,
    ) -> Self {
        let directly_finalized = directly_finalized.unwrap_or(false);
        let finalized = finalized.unwrap_or(false);
        Self {
            block_hash: b.block_hash.clone(),
            parents: b.header.parents_hash_list.clone(),
            sender: b.sender.clone(),
            justifications: b.justifications.clone(),
            weight_map: Self::weight_map(&b.body.state),
            block_number: b.body.state.block_number,
            sequence_number: b.seq_num,
            invalid,
            // this value is not used anywhere down the call pipeline, so its safe to set it to false
            directly_finalized,
            finalized,
            fault_tolerance_value: 0.0,
            successful_state_effect_indices: b
                .body
                .deploys
                .iter()
                .enumerate()
                .filter(|(_, deploy)| !deploy.is_failed)
                .map(|(index, _)| u32::try_from(index).expect("block deploy index must fit in u32"))
                .chain(
                    b.body
                        .system_deploys
                        .iter()
                        .enumerate()
                        .filter(|(_, deploy)| {
                            matches!(deploy, ProcessedSystemDeploy::Succeeded { .. })
                        })
                        .map(|(index, _)| {
                            u32::try_from(b.body.deploys.len() + index)
                                .expect("block system deploy index must fit in u32")
                        }),
                )
                .collect(),
            rejected_state_effects: b.body.rejected_state_effects.iter().cloned().collect(),
            protocol_version: b.header.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;

    use super::*;
    use crate::rhoapi::PCost;
    use crate::rust::casper::protocol::casper_message::{
        Body, DeployAdmissionStatus, DeployData, Header, ProcessedDeploy, SystemDeployData,
    };

    fn processed_deploy(is_failed: bool) -> ProcessedDeploy {
        let algorithm: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (private_key, _) = algorithm.new_key_pair();
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp: 0,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            algorithm,
            private_key,
        )
        .unwrap();
        ProcessedDeploy {
            deploy,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: Bytes::new(),
            post_state_hash: Bytes::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: DeployAdmissionStatus::Executed,
        }
    }

    #[test]
    fn block_metadata_records_only_successful_execution_effects_and_round_trips() {
        let rejected = StateEffectId {
            source_block_hash: Bytes::from_static(b"source"),
            execution_index: 4,
        };
        let block = BlockMessage {
            block_hash: Bytes::from_static(b"block"),
            header: Header {
                parents_hash_list: vec![Bytes::from_static(b"parent")],
                timestamp: 0,
                version: 3,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::from_static(b"pre"),
                    post_state_hash: Bytes::from_static(b"post"),
                    bonds: Vec::new(),
                    block_number: 9,
                },
                deploys: vec![processed_deploy(false), processed_deploy(true)],
                rejected_deploys: Vec::new(),
                rejected_state_effects: vec![rejected.clone()],
                system_deploys: vec![
                    ProcessedSystemDeploy::Succeeded {
                        event_list: Vec::new(),
                        system_deploy: SystemDeployData::Empty,
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                    },
                    ProcessedSystemDeploy::Failed {
                        event_list: Vec::new(),
                        error_msg: "failed".to_string(),
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                    },
                ],
                extra_bytes: Bytes::new(),
            },
            justifications: Vec::new(),
            sender: Bytes::from_static(b"sender"),
            seq_num: 3,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: "root".to_string(),
            extra_bytes: Bytes::new(),
        };

        let metadata = BlockMetadata::from_block(&block, false, Some(true), Some(true));
        assert_eq!(
            metadata.successful_state_effect_indices,
            BTreeSet::from([0, 2])
        );
        assert_eq!(metadata.rejected_state_effects, BTreeSet::from([rejected]));
        assert_eq!(metadata.protocol_version, 3);
        assert_eq!(BlockMetadata::from_bytes(&metadata.to_bytes()), metadata);
    }
}
