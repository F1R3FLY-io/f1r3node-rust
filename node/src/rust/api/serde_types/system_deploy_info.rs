//! JSON serialization/deserialization for SystemDeployInfoWithEventData and related types
//!
//! This module provides custom JSON serialization for protobuf types that don't have serde derives by default.

use casper::rust::util::proto_util;
use models::casper::{
    BlockCommitmentProto, BondInfo, CloseBlockSystemDeployDataProto, HeaderProto,
    JustificationInfo, PeekProto, RejectedDeployInfo, ReportCommProto, ReportConsumeProto,
    ReportProduceProto, ReportProto, SingleReport, SlashSystemDeployDataProto,
    SystemDeployDataProto, SystemDeployInfoWithEventData,
};
use models::rhoapi::{BindPattern, ListParWithRandom, Par};
use models::rust::casper::protocol::casper_message::BlockCommitment;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::base64_bytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondInfoSerde {
    pub validator: String,
    pub stake: String,
}

impl From<BondInfo> for BondInfoSerde {
    fn from(data: BondInfo) -> Self {
        Self {
            validator: data.validator,
            stake: data.stake,
        }
    }
}

impl From<BondInfoSerde> for BondInfo {
    fn from(data: BondInfoSerde) -> Self {
        Self {
            validator: data.validator,
            stake: data.stake,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JustificationInfoSerde {
    pub validator: String,
    #[serde(rename = "latestBlockHash")]
    pub latest_block_hash: String,
}

impl From<JustificationInfo> for JustificationInfoSerde {
    fn from(data: JustificationInfo) -> Self {
        Self {
            validator: data.validator,
            latest_block_hash: data.latest_block_hash,
        }
    }
}

impl From<JustificationInfoSerde> for JustificationInfo {
    fn from(data: JustificationInfoSerde) -> Self {
        Self {
            validator: data.validator,
            latest_block_hash: data.latest_block_hash,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedDeployInfoSerde {
    pub sig: String,
}

impl From<RejectedDeployInfo> for RejectedDeployInfoSerde {
    fn from(data: RejectedDeployInfo) -> Self { Self { sig: data.sig } }
}

impl From<RejectedDeployInfoSerde> for RejectedDeployInfo {
    fn from(data: RejectedDeployInfoSerde) -> Self { Self { sig: data.sig } }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HeaderSerde {
    #[serde(rename = "parentsHashList", with = "base64_bytes::list")]
    pub parents_hash_list: Vec<Vec<u8>>,
    pub timestamp: i64,
    pub version: i64,
    #[serde(rename = "extraBytes", with = "base64_bytes")]
    pub extra_bytes: Vec<u8>,
    #[serde(rename = "chainId", with = "base64_bytes")]
    pub chain_id: Vec<u8>,
}

impl From<HeaderProto> for HeaderSerde {
    fn from(data: HeaderProto) -> Self {
        Self {
            parents_hash_list: data.parents_hash_list.iter().map(|h| h.to_vec()).collect(),
            timestamp: data.timestamp,
            version: data.version,
            extra_bytes: data.extra_bytes.to_vec(),
            chain_id: data.chain_id.to_vec(),
        }
    }
}

impl From<HeaderSerde> for HeaderProto {
    fn from(data: HeaderSerde) -> Self {
        Self {
            parents_hash_list: data.parents_hash_list.into_iter().map(Into::into).collect(),
            timestamp: data.timestamp,
            version: data.version,
            extra_bytes: data.extra_bytes.into(),
            chain_id: data.chain_id.into(),
        }
    }
}

/// A block as its hash and signature commit to it, with the hash.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BlockCommitmentSerde {
    #[serde(rename = "blockHash", with = "base64_bytes")]
    pub block_hash: Vec<u8>,
    pub header: HeaderSerde,
    #[serde(rename = "preStateHash", with = "base64_bytes")]
    pub pre_state_hash: Vec<u8>,
    #[serde(rename = "postStateHash", with = "base64_bytes")]
    pub post_state_hash: Vec<u8>,
    #[serde(rename = "bondsRoot", with = "base64_bytes")]
    pub bonds_root: Vec<u8>,
    #[serde(rename = "justificationsRoot", with = "base64_bytes")]
    pub justifications_root: Vec<u8>,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    #[serde(rename = "rejectedRoot", with = "base64_bytes")]
    pub rejected_root: Vec<u8>,
    #[serde(rename = "appliedFromScopeRoot", with = "base64_bytes")]
    pub applied_from_scope_root: Vec<u8>,
    #[serde(rename = "mergeBase", with = "base64_bytes")]
    pub merge_base: Vec<u8>,
    #[serde(rename = "bodyExtraBytes", with = "base64_bytes")]
    pub body_extra_bytes: Vec<u8>,
    #[serde(with = "base64_bytes")]
    pub sender: Vec<u8>,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    #[serde(rename = "seqNum")]
    pub seq_num: i32,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    #[serde(rename = "extraBytes", with = "base64_bytes")]
    pub extra_bytes: Vec<u8>,
    #[serde(rename = "deploysRoot", with = "base64_bytes")]
    pub deploys_root: Vec<u8>,
    #[serde(with = "base64_bytes")]
    pub sig: Vec<u8>,
}

impl From<BlockCommitmentProto> for BlockCommitmentSerde {
    fn from(data: BlockCommitmentProto) -> Self {
        let commitment = BlockCommitment::from_proto(data)
            .expect("a reported commitment is built from a block, so it has a header");
        Self {
            block_hash: proto_util::commitment_hash(&commitment).to_vec(),
            header: commitment.header.to_proto().into(),
            pre_state_hash: commitment.pre_state_hash.to_vec(),
            post_state_hash: commitment.post_state_hash.to_vec(),
            bonds_root: commitment.bonds_root.to_vec(),
            justifications_root: commitment.justifications_root.to_vec(),
            block_number: commitment.block_number,
            rejected_root: commitment.rejected_root.to_vec(),
            applied_from_scope_root: commitment.applied_from_scope_root.to_vec(),
            merge_base: commitment.merge_base.to_vec(),
            body_extra_bytes: commitment.body_extra_bytes.to_vec(),
            sender: commitment.sender.to_vec(),
            sig_algorithm: commitment.sig_algorithm,
            seq_num: commitment.seq_num,
            shard_id: commitment.shard_id,
            extra_bytes: commitment.extra_bytes.to_vec(),
            deploys_root: commitment.deploys_root.to_vec(),
            sig: commitment.sig.to_vec(),
        }
    }
}

// `block_hash` is derived from the other fields, so it is not carried back.
impl From<BlockCommitmentSerde> for BlockCommitmentProto {
    fn from(data: BlockCommitmentSerde) -> Self {
        Self {
            header: Some(data.header.into()),
            pre_state_hash: data.pre_state_hash.into(),
            post_state_hash: data.post_state_hash.into(),
            bonds_root: data.bonds_root.into(),
            justifications_root: data.justifications_root.into(),
            block_number: data.block_number,
            rejected_root: data.rejected_root.into(),
            applied_from_scope_root: data.applied_from_scope_root.into(),
            merge_base: data.merge_base.into(),
            body_extra_bytes: data.body_extra_bytes.into(),
            sender: data.sender.into(),
            sig_algorithm: data.sig_algorithm,
            seq_num: data.seq_num,
            shard_id: data.shard_id,
            extra_bytes: data.extra_bytes.into(),
            deploys_root: data.deploys_root.into(),
            sig: data.sig.into(),
        }
    }
}

/// A slash and its evidence: two blocks one sender signed at one sequence
/// number, ordered by block hash.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SlashSystemDeployDataSerde {
    #[serde(rename = "issuerPublicKey", with = "base64_bytes")]
    pub issuer_public_key: Vec<u8>,
    pub first: BlockCommitmentSerde,
    pub second: BlockCommitmentSerde,
}

impl From<SlashSystemDeployDataProto> for SlashSystemDeployDataSerde {
    fn from(data: SlashSystemDeployDataProto) -> Self {
        Self {
            issuer_public_key: data.issuer_public_key.to_vec(),
            first: data
                .first
                .expect("a reported slash carries both blocks of its evidence")
                .into(),
            second: data
                .second
                .expect("a reported slash carries both blocks of its evidence")
                .into(),
        }
    }
}

impl From<SlashSystemDeployDataSerde> for SlashSystemDeployDataProto {
    fn from(data: SlashSystemDeployDataSerde) -> Self {
        Self {
            issuer_public_key: data.issuer_public_key.into(),
            first: Some(data.first.into()),
            second: Some(data.second.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CloseBlockSystemDeployDataSerde {}

impl From<CloseBlockSystemDeployDataProto> for CloseBlockSystemDeployDataSerde {
    fn from(_data: CloseBlockSystemDeployDataProto) -> Self { Self {} }
}

impl From<CloseBlockSystemDeployDataSerde> for CloseBlockSystemDeployDataProto {
    fn from(_data: CloseBlockSystemDeployDataSerde) -> Self { Self {} }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum SystemDeployDataSerde {
    SlashSystemDeploy(SlashSystemDeployDataSerde),
    CloseBlockSystemDeploy(CloseBlockSystemDeployDataSerde),
}

impl From<SystemDeployDataProto> for SystemDeployDataSerde {
    fn from(data: SystemDeployDataProto) -> Self {
        match data.system_deploy {
            Some(models::casper::system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                slash,
            )) => Self::SlashSystemDeploy(slash.into()),
            Some(
                models::casper::system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(
                    close,
                ),
            ) => Self::CloseBlockSystemDeploy(close.into()),
            None => Self::CloseBlockSystemDeploy(CloseBlockSystemDeployDataSerde {}),
        }
    }
}

impl From<SystemDeployDataSerde> for SystemDeployDataProto {
    fn from(data: SystemDeployDataSerde) -> Self {
        let system_deploy = match data {
            SystemDeployDataSerde::SlashSystemDeploy(slash) => Some(
                models::casper::system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                    slash.into(),
                ),
            ),
            SystemDeployDataSerde::CloseBlockSystemDeploy(close) => Some(
                models::casper::system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(
                    close.into(),
                ),
            ),
        };
        Self { system_deploy }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PeekProtoSerde {
    #[serde(rename = "channelIndex")]
    pub channel_index: i32,
}

impl From<PeekProto> for PeekProtoSerde {
    fn from(data: PeekProto) -> Self {
        Self {
            channel_index: data.channel_index,
        }
    }
}

impl From<PeekProtoSerde> for PeekProto {
    fn from(data: PeekProtoSerde) -> Self {
        Self {
            channel_index: data.channel_index,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportProduceSerde {
    pub channel: Option<Par>,
    pub data: Option<ListParWithRandom>,
}

impl From<ReportProduceProto> for ReportProduceSerde {
    fn from(data: ReportProduceProto) -> Self {
        Self {
            channel: data.channel,
            data: data.data,
        }
    }
}

impl From<ReportProduceSerde> for ReportProduceProto {
    fn from(data: ReportProduceSerde) -> Self {
        Self {
            channel: data.channel,
            data: data.data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportConsumeSerde {
    pub channels: Vec<Par>,
    pub patterns: Vec<BindPattern>,
    pub peeks: Vec<PeekProtoSerde>,
}

impl From<ReportConsumeProto> for ReportConsumeSerde {
    fn from(data: ReportConsumeProto) -> Self {
        Self {
            channels: data.channels,
            patterns: data.patterns,
            peeks: data.peeks.into_iter().map(|p| p.into()).collect(),
        }
    }
}

impl From<ReportConsumeSerde> for ReportConsumeProto {
    fn from(data: ReportConsumeSerde) -> Self {
        Self {
            channels: data.channels,
            patterns: data.patterns,
            peeks: data.peeks.into_iter().map(|p| p.into()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportCommSerde {
    pub consume: Option<ReportConsumeSerde>,
    pub produces: Vec<ReportProduceSerde>,
}

impl From<ReportCommProto> for ReportCommSerde {
    fn from(data: ReportCommProto) -> Self {
        Self {
            consume: data.consume.map(|c| c.into()),
            produces: data.produces.into_iter().map(|p| p.into()).collect(),
        }
    }
}

impl From<ReportCommSerde> for ReportCommProto {
    fn from(data: ReportCommSerde) -> Self {
        Self {
            consume: data.consume.map(|c| c.into()),
            produces: data.produces.into_iter().map(|p| p.into()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum ReportProtoSerde {
    Produce(ReportProduceSerde),
    Consume(ReportConsumeSerde),
    Comm(ReportCommSerde),
}

impl From<ReportProto> for ReportProtoSerde {
    fn from(data: ReportProto) -> Self {
        match data.report {
            Some(models::casper::report_proto::Report::Produce(produce)) => {
                Self::Produce(produce.into())
            }
            Some(models::casper::report_proto::Report::Consume(consume)) => {
                Self::Consume(consume.into())
            }
            Some(models::casper::report_proto::Report::Comm(comm)) => Self::Comm(comm.into()),
            None => Self::Produce(ReportProduceSerde {
                channel: None,
                data: None,
            }),
        }
    }
}

impl From<ReportProtoSerde> for ReportProto {
    fn from(data: ReportProtoSerde) -> Self {
        let report = match data {
            ReportProtoSerde::Produce(produce) => Some(
                models::casper::report_proto::Report::Produce(produce.into()),
            ),
            ReportProtoSerde::Consume(consume) => Some(
                models::casper::report_proto::Report::Consume(consume.into()),
            ),
            ReportProtoSerde::Comm(comm) => {
                Some(models::casper::report_proto::Report::Comm(comm.into()))
            }
        };
        Self { report }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    ToSchema,
    Default
)]
pub enum ReportPhaseSerde {
    #[default]
    #[serde(rename = "REPORT_PHASE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "REPORT_PHASE_PRECHARGE")]
    Precharge,
    #[serde(rename = "REPORT_PHASE_USER")]
    User,
    #[serde(rename = "REPORT_PHASE_REFUND")]
    Refund,
}

/// Proto `phase` field (`i32`) -> serde mirror. An unknown discriminant
/// decodes as `Unspecified` so a future-added phase never breaks the
/// JSON consumer.
impl From<i32> for ReportPhaseSerde {
    fn from(value: i32) -> Self {
        use models::casper::ReportPhase as P;
        match value {
            v if v == P::Precharge as i32 => Self::Precharge,
            v if v == P::User as i32 => Self::User,
            v if v == P::Refund as i32 => Self::Refund,
            _ => Self::Unspecified,
        }
    }
}

/// Serde mirror -> proto `phase` field (`i32`).
impl From<ReportPhaseSerde> for i32 {
    fn from(phase: ReportPhaseSerde) -> Self {
        use models::casper::ReportPhase as P;
        match phase {
            ReportPhaseSerde::Unspecified => P::Unspecified as i32,
            ReportPhaseSerde::Precharge => P::Precharge as i32,
            ReportPhaseSerde::User => P::User as i32,
            ReportPhaseSerde::Refund => P::Refund as i32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SingleReportSerde {
    pub events: Vec<ReportProtoSerde>,
    /// The execution phase this segment belongs to. `Unspecified` for
    /// reports produced before the marker was introduced, and for
    /// system-deploy segments. Consumers must treat `Unspecified` as
    /// "fall back to the positional path".
    #[serde(default)]
    pub phase: ReportPhaseSerde,
}

impl From<SingleReport> for SingleReportSerde {
    fn from(data: SingleReport) -> Self {
        Self {
            events: data.events.into_iter().map(|e| e.into()).collect(),
            phase: data.phase.into(),
        }
    }
}

impl From<SingleReportSerde> for SingleReport {
    fn from(data: SingleReportSerde) -> Self {
        Self {
            events: data.events.into_iter().map(|e| e.into()).collect(),
            phase: data.phase.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SystemDeployInfoWithEventSerde {
    #[serde(rename = "systemDeploy")]
    pub system_deploy: Option<SystemDeployDataSerde>,
    pub report: Vec<SingleReportSerde>,
}

impl From<SystemDeployInfoWithEventData> for SystemDeployInfoWithEventSerde {
    fn from(data: SystemDeployInfoWithEventData) -> Self {
        Self {
            system_deploy: data.system_deploy.map(|s| s.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
        }
    }
}

impl From<SystemDeployInfoWithEventSerde> for SystemDeployInfoWithEventData {
    fn from(data: SystemDeployInfoWithEventSerde) -> Self {
        Self {
            system_deploy: data.system_deploy.map(|s| s.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commitment(seed: u8) -> BlockCommitmentProto {
        BlockCommitmentProto {
            header: Some(HeaderProto {
                parents_hash_list: vec![vec![seed; 32].into(), vec![seed + 1; 32].into()],
                timestamp: i64::from(seed),
                version: 1,
                extra_bytes: vec![seed].into(),
                chain_id: vec![seed + 3; 32].into(),
            }),
            pre_state_hash: vec![seed; 32].into(),
            post_state_hash: vec![seed + 2; 32].into(),
            bonds_root: vec![seed + 3; 32].into(),
            justifications_root: vec![seed + 11; 32].into(),
            block_number: i64::from(seed) + 10,
            rejected_root: vec![seed + 4; 32].into(),
            applied_from_scope_root: vec![seed + 5; 32].into(),
            merge_base: vec![seed + 6; 32].into(),
            body_extra_bytes: vec![seed + 7].into(),
            sender: vec![0x04; 65].into(),
            sig_algorithm: "secp256k1".to_string(),
            seq_num: 3,
            shard_id: "root".to_string(),
            extra_bytes: vec![seed + 8].into(),
            deploys_root: vec![seed + 9; 32].into(),
            sig: vec![seed + 10; 70].into(),
        }
    }

    fn slash(issuer: u8) -> SlashSystemDeployDataProto {
        SlashSystemDeployDataProto {
            issuer_public_key: vec![issuer; 65].into(),
            first: Some(commitment(1)),
            second: Some(commitment(20)),
        }
    }

    #[test]
    fn slash_evidence_round_trips_through_json() {
        let proto = slash(9);

        let json = serde_json::to_string(&SlashSystemDeployDataSerde::from(proto.clone())).unwrap();
        let back: SlashSystemDeployDataProto =
            serde_json::from_str::<SlashSystemDeployDataSerde>(&json)
                .unwrap()
                .into();

        assert_eq!(back, proto);
    }

    #[test]
    fn each_evidence_block_reports_its_hash() {
        let serde = SlashSystemDeployDataSerde::from(slash(9));
        for (reported, seed) in [(&serde.first, 1), (&serde.second, 20)] {
            let expected = proto_util::commitment_hash(
                &BlockCommitment::from_proto(commitment(seed)).unwrap(),
            );
            assert_eq!(reported.block_hash, expected.to_vec());
        }
    }

    fn sample_par() -> Par {
        Par {
            exprs: vec![models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(7)),
            }],
            ..Default::default()
        }
    }

    fn sample_produce() -> ReportProduceProto {
        ReportProduceProto {
            channel: Some(sample_par()),
            data: Some(ListParWithRandom {
                pars: vec![sample_par()],
                random_state: vec![9, 9].into(),
            }),
        }
    }

    fn sample_consume() -> ReportConsumeProto {
        ReportConsumeProto {
            channels: vec![sample_par()],
            patterns: vec![BindPattern::default()],
            peeks: vec![PeekProto { channel_index: 3 }],
        }
    }

    #[test]
    fn bond_info_roundtrips() {
        let proto = BondInfo {
            stake: "500".to_string(),
            validator: "validator-key".to_string(),
        };
        let serde: BondInfoSerde = proto.clone().into();
        assert_eq!(serde.stake, "500");
        let back: BondInfo = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn justification_info_roundtrips() {
        let proto = JustificationInfo {
            validator: "validator-key".to_string(),
            latest_block_hash: "abcd".to_string(),
        };
        let serde: JustificationInfoSerde = proto.clone().into();
        let json = serde_json::to_value(&serde).unwrap();
        assert_eq!(json["latestBlockHash"], "abcd");
        let back: JustificationInfo = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn rejected_deploy_info_roundtrips() {
        let proto = RejectedDeployInfo {
            sig: "cafe".to_string(),
        };
        let serde: RejectedDeployInfoSerde = proto.clone().into();
        let back: RejectedDeployInfo = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn system_deploy_data_slash_variant_roundtrips() {
        let proto = SystemDeployDataProto {
            system_deploy: Some(
                models::casper::system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(slash(2)),
            ),
        };
        let serde: SystemDeployDataSerde = proto.clone().into();
        assert!(matches!(serde, SystemDeployDataSerde::SlashSystemDeploy(_)));
        let back: SystemDeployDataProto = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn system_deploy_data_close_block_variant_roundtrips() {
        let proto = SystemDeployDataProto {
            system_deploy: Some(
                models::casper::system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(
                    CloseBlockSystemDeployDataProto {},
                ),
            ),
        };
        let serde: SystemDeployDataSerde = proto.clone().into();
        assert!(matches!(
            serde,
            SystemDeployDataSerde::CloseBlockSystemDeploy(_)
        ));
        let back: SystemDeployDataProto = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn system_deploy_data_none_falls_back_to_close_block() {
        let proto = SystemDeployDataProto {
            system_deploy: None,
        };
        let serde: SystemDeployDataSerde = proto.into();
        assert!(matches!(
            serde,
            SystemDeployDataSerde::CloseBlockSystemDeploy(_)
        ));
    }

    #[test]
    fn report_proto_produce_variant_roundtrips() {
        let proto = ReportProto {
            report: Some(models::casper::report_proto::Report::Produce(
                sample_produce(),
            )),
        };
        let serde: ReportProtoSerde = proto.clone().into();
        assert!(matches!(serde, ReportProtoSerde::Produce(_)));
        let back: ReportProto = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn report_proto_consume_variant_roundtrips() {
        let proto = ReportProto {
            report: Some(models::casper::report_proto::Report::Consume(
                sample_consume(),
            )),
        };
        let serde: ReportProtoSerde = proto.clone().into();
        match &serde {
            ReportProtoSerde::Consume(consume) => {
                assert_eq!(consume.peeks.len(), 1);
                assert_eq!(consume.peeks[0].channel_index, 3);
            }
            other => panic!("expected Consume variant, got {:?}", other),
        }
        let back: ReportProto = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn report_proto_comm_variant_roundtrips() {
        let proto = ReportProto {
            report: Some(models::casper::report_proto::Report::Comm(
                ReportCommProto {
                    consume: Some(sample_consume()),
                    produces: vec![sample_produce()],
                },
            )),
        };
        let serde: ReportProtoSerde = proto.clone().into();
        assert!(matches!(serde, ReportProtoSerde::Comm(_)));
        let back: ReportProto = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn report_proto_none_falls_back_to_empty_produce() {
        let proto = ReportProto { report: None };
        let serde: ReportProtoSerde = proto.into();
        match serde {
            ReportProtoSerde::Produce(produce) => {
                assert!(produce.channel.is_none());
                assert!(produce.data.is_none());
            }
            other => panic!("expected Produce fallback, got {:?}", other),
        }
    }

    #[test]
    fn report_phase_roundtrips_through_i32() {
        use models::casper::ReportPhase as P;
        for (phase, raw) in [
            (ReportPhaseSerde::Unspecified, P::Unspecified as i32),
            (ReportPhaseSerde::Precharge, P::Precharge as i32),
            (ReportPhaseSerde::User, P::User as i32),
            (ReportPhaseSerde::Refund, P::Refund as i32),
        ] {
            assert_eq!(ReportPhaseSerde::from(raw), phase);
            assert_eq!(i32::from(phase), raw);
        }
    }

    #[test]
    fn report_phase_unknown_discriminant_decodes_as_unspecified() {
        assert_eq!(ReportPhaseSerde::from(9999), ReportPhaseSerde::Unspecified);
    }

    #[test]
    fn report_phase_json_names_are_stable() {
        assert_eq!(
            serde_json::to_string(&ReportPhaseSerde::Precharge).unwrap(),
            "\"REPORT_PHASE_PRECHARGE\""
        );
        let parsed: ReportPhaseSerde = serde_json::from_str("\"REPORT_PHASE_REFUND\"").unwrap();
        assert_eq!(parsed, ReportPhaseSerde::Refund);
    }

    #[test]
    fn single_report_missing_phase_defaults_to_unspecified() {
        let json = r#"{"events":[]}"#;
        let serde: SingleReportSerde = serde_json::from_str(json).unwrap();
        assert_eq!(serde.phase, ReportPhaseSerde::Unspecified);
    }

    #[test]
    fn system_deploy_info_with_event_roundtrips() {
        let proto = SystemDeployInfoWithEventData {
            system_deploy: Some(SystemDeployDataProto {
                system_deploy: Some(
                    models::casper::system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                        slash(8),
                    ),
                ),
            }),
            report: vec![SingleReport {
                events: vec![ReportProto {
                    report: Some(models::casper::report_proto::Report::Produce(
                        sample_produce(),
                    )),
                }],
                phase: models::casper::ReportPhase::User as i32,
            }],
        };

        let serde: SystemDeployInfoWithEventSerde = proto.clone().into();
        assert_eq!(serde.report.len(), 1);
        assert_eq!(serde.report[0].phase, ReportPhaseSerde::User);

        let json = serde_json::to_string(&serde).unwrap();
        let parsed: SystemDeployInfoWithEventSerde = serde_json::from_str(&json).unwrap();
        let back: SystemDeployInfoWithEventData = parsed.into();
        assert_eq!(back, proto);
    }
}
