//! JSON serialization/deserialization for SystemDeployInfoWithEventData and related types
//!
//! This module provides custom JSON serialization for protobuf types that don't have serde derives by default.

use models::casper::{
    BondInfo, CloseBlockSystemDeployDataProto, JustificationInfo, PeekProto,
    RedeemSystemDeployDataProto, RedemptionAuthorizationProto, RejectedDeployInfo, ReportCommProto,
    ReportConsumeProto, ReportProduceProto, ReportProto, SingleReport, SlashSystemDeployDataProto,
    SystemDeployDataProto, SystemDeployInfoWithEventData,
};
use models::rhoapi::{BindPattern, ListParWithRandom, Par};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::base64_bytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondInfoSerde {
    pub validator: String,
    pub stake: i64,
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
    #[serde(default, rename = "sourceBlockHash")]
    pub source_block_hash: String,
    #[serde(default)]
    pub reason: String,
}

impl From<RejectedDeployInfo> for RejectedDeployInfoSerde {
    fn from(data: RejectedDeployInfo) -> Self {
        Self {
            sig: data.sig,
            source_block_hash: data.source_block_hash,
            reason: data.reason,
        }
    }
}

impl From<RejectedDeployInfoSerde> for RejectedDeployInfo {
    fn from(data: RejectedDeployInfoSerde) -> Self {
        Self {
            sig: data.sig,
            source_block_hash: data.source_block_hash,
            reason: data.reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SlashSystemDeployDataSerde {
    #[serde(rename = "invalidBlockHash", with = "base64_bytes")]
    pub invalid_block_hash: Vec<u8>,
    #[serde(
        rename = "equivocationBlockHash",
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "base64_bytes"
    )]
    pub equivocation_block_hash: Vec<u8>,
    #[serde(rename = "issuerPublicKey", with = "base64_bytes")]
    pub issuer_public_key: Vec<u8>,
    // `default` preserves API back-compat with pre-§9 clients that don't know
    // about `targetActivationEpoch`. Such payloads deserialize to epoch 0,
    // which the receiver's authorization predicate
    // (`validate_received_slash_deploys`) then rejects against the current
    // epoch — old clients fail cleanly rather than silently slashing.
    #[serde(rename = "targetActivationEpoch", default)]
    pub target_activation_epoch: i64,
    #[serde(rename = "targetBondGeneration", default)]
    pub target_bond_generation: Option<i64>,
}

impl From<SlashSystemDeployDataProto> for SlashSystemDeployDataSerde {
    fn from(data: SlashSystemDeployDataProto) -> Self {
        Self {
            invalid_block_hash: data.invalid_block_hash.to_vec(),
            equivocation_block_hash: data.equivocation_block_hash.to_vec(),
            issuer_public_key: data.issuer_public_key.to_vec(),
            target_activation_epoch: data.target_activation_epoch,
            target_bond_generation: data.target_bond_generation,
        }
    }
}

impl From<SlashSystemDeployDataSerde> for SlashSystemDeployDataProto {
    fn from(data: SlashSystemDeployDataSerde) -> Self {
        Self {
            invalid_block_hash: data.invalid_block_hash.into(),
            equivocation_block_hash: data.equivocation_block_hash.into(),
            issuer_public_key: data.issuer_public_key.into(),
            target_activation_epoch: data.target_activation_epoch,
            target_bond_generation: data.target_bond_generation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RedemptionAuthorizationSerde {
    #[serde(rename = "publicKey", with = "base64_bytes")]
    pub public_key: Vec<u8>,
    #[serde(with = "base64_bytes")]
    pub signature: Vec<u8>,
}

impl From<RedemptionAuthorizationProto> for RedemptionAuthorizationSerde {
    fn from(data: RedemptionAuthorizationProto) -> Self {
        Self {
            public_key: data.public_key.to_vec(),
            signature: data.signature.to_vec(),
        }
    }
}

impl From<RedemptionAuthorizationSerde> for RedemptionAuthorizationProto {
    fn from(data: RedemptionAuthorizationSerde) -> Self {
        Self {
            public_key: data.public_key.into(),
            signature: data.signature.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RedeemSystemDeployDataSerde {
    #[serde(rename = "validatorPk", with = "base64_bytes")]
    pub validator_pk: Vec<u8>,
    #[serde(rename = "targetBondGeneration")]
    pub target_bond_generation: Option<i64>,
    #[serde(rename = "outcomeTag")]
    pub outcome_tag: String,
    pub penalty: i64,
    #[serde(rename = "posMultiSigPublicKeys")]
    pub pos_multi_sig_public_keys: Vec<String>,
    #[serde(rename = "posMultiSigQuorum")]
    pub pos_multi_sig_quorum: u32,
    pub authorizations: Vec<RedemptionAuthorizationSerde>,
}

impl From<RedeemSystemDeployDataProto> for RedeemSystemDeployDataSerde {
    fn from(data: RedeemSystemDeployDataProto) -> Self {
        Self {
            validator_pk: data.validator_pk.to_vec(),
            target_bond_generation: data.target_bond_generation,
            outcome_tag: data.outcome_tag,
            penalty: data.penalty,
            pos_multi_sig_public_keys: data.pos_multi_sig_public_keys,
            pos_multi_sig_quorum: data.pos_multi_sig_quorum,
            authorizations: data.authorizations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RedeemSystemDeployDataSerde> for RedeemSystemDeployDataProto {
    fn from(data: RedeemSystemDeployDataSerde) -> Self {
        Self {
            validator_pk: data.validator_pk.into(),
            target_bond_generation: data.target_bond_generation,
            outcome_tag: data.outcome_tag,
            penalty: data.penalty,
            pos_multi_sig_public_keys: data.pos_multi_sig_public_keys,
            pos_multi_sig_quorum: data.pos_multi_sig_quorum,
            authorizations: data.authorizations.into_iter().map(Into::into).collect(),
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
    RedeemSystemDeploy(RedeemSystemDeployDataSerde),
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
            Some(models::casper::system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(
                redeem,
            )) => Self::RedeemSystemDeploy(redeem.into()),
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
            SystemDeployDataSerde::RedeemSystemDeploy(redeem) => Some(
                models::casper::system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(
                    redeem.into(),
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

    #[test]
    fn slash_system_deploy_preserves_target_activation_epoch() {
        let proto = SlashSystemDeployDataProto {
            invalid_block_hash: vec![1, 2, 3].into(),
            equivocation_block_hash: vec![7, 8, 9].into(),
            issuer_public_key: vec![4, 5, 6].into(),
            target_activation_epoch: 42,
            target_bond_generation: Some(3),
        };

        let serde: SlashSystemDeployDataSerde = proto.clone().into();
        assert_eq!(serde.target_activation_epoch, 42);
        assert_eq!(serde.target_bond_generation, Some(3));
        assert_eq!(serde.equivocation_block_hash, vec![7, 8, 9]);

        let roundtrip: SlashSystemDeployDataProto = serde.into();
        assert_eq!(roundtrip, proto);
    }

    #[test]
    fn slash_system_deploy_json_defaults_missing_target_activation_epoch() {
        // Back-compat: legacy JSON payloads that pre-date the
        // target_activation_epoch field deserialize with default 0. The
        // back-compat default must NOT silently widen the slashable
        // surface — when current_epoch > 0 the receive-side predicate
        // must reject the slash as EpochMismatch. That contract is pinned
        // by `received_stale_slash_deploy_is_rejected_before_replay` in
        // `casper/tests/slashing/slash_authorization_regressions.rs:177-194`,
        // which constructs a slash deploy with target_activation_epoch=0
        // at block_number=11 (current_epoch=1 under epoch_length=10) and
        // verifies the resulting error contains "non-current epoch".
        let json = r#"{"invalidBlockHash":"AQID","issuerPublicKey":"BAUG"}"#;
        let serde: SlashSystemDeployDataSerde = serde_json::from_str(json).unwrap();

        assert_eq!(serde.invalid_block_hash, vec![1, 2, 3]);
        assert!(serde.equivocation_block_hash.is_empty());
        assert_eq!(serde.issuer_public_key, vec![4, 5, 6]);
        assert_eq!(serde.target_activation_epoch, 0);
        assert_eq!(serde.target_bond_generation, None);
    }

    #[test]
    fn redeem_system_deploy_roundtrip_remains_redeem() {
        let proto = RedeemSystemDeployDataProto {
            validator_pk: vec![1, 2, 3].into(),
            outcome_tag: "Guilty".to_string(),
            penalty: 9,
            pos_multi_sig_public_keys: vec!["a".to_string(), "b".to_string()],
            pos_multi_sig_quorum: 2,
            authorizations: vec![RedemptionAuthorizationProto {
                public_key: vec![4, 5].into(),
                signature: vec![6, 7].into(),
            }],
            target_bond_generation: Some(4),
        };
        let wrapped = SystemDeployDataProto {
            system_deploy: Some(
                models::casper::system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(
                    proto.clone(),
                ),
            ),
        };

        let serde: SystemDeployDataSerde = wrapped.clone().into();
        assert!(matches!(
            serde,
            SystemDeployDataSerde::RedeemSystemDeploy(_)
        ));
        let roundtrip: SystemDeployDataProto = serde.into();
        assert_eq!(roundtrip, wrapped);
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
                cost_authority: None,
                cost_stack: None,
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
            validator: "validator-key".to_string(),
            stake: 500,
        };
        let serde: BondInfoSerde = proto.clone().into();
        assert_eq!(serde.stake, 500);
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
            source_block_hash: String::new(),
            reason: String::new(),
        };
        let serde: RejectedDeployInfoSerde = proto.clone().into();
        let back: RejectedDeployInfo = serde.into();
        assert_eq!(back, proto);
    }

    #[test]
    fn system_deploy_data_slash_variant_roundtrips() {
        let proto = SystemDeployDataProto {
            system_deploy: Some(
                models::casper::system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                    SlashSystemDeployDataProto {
                        invalid_block_hash: vec![1].into(),
                        equivocation_block_hash: Vec::new().into(),
                        issuer_public_key: vec![2].into(),
                        target_activation_epoch: 3,
                        target_bond_generation: None,
                    },
                ),
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
                        SlashSystemDeployDataProto {
                            invalid_block_hash: vec![7].into(),
                            equivocation_block_hash: Vec::new().into(),
                            issuer_public_key: vec![8].into(),
                            target_activation_epoch: 1,
                            target_bond_generation: None,
                        },
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
