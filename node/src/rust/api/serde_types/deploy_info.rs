//! JSON serialization/deserialization for DeployInfo
//!
//! This module provides custom JSON serialization for the DeployInfo protobuf type
//! that doesn't have serde derives by default.

use models::casper::{DeployInfo, DeployInfoWithEventData, TransferInfo};
use models::rust::deploy_parameters::{
    deserialize_parameters, parameters_from_proto, DeployParameter, ParameterError,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rust::api::serde_types::system_deploy_info::SingleReportSerde;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransferInfoSerde {
    #[serde(rename = "fromAddr")]
    pub from_addr: String,
    #[serde(rename = "toAddr")]
    pub to_addr: String,
    pub amount: i64,
    pub success: bool,
    #[serde(rename = "failReason")]
    pub fail_reason: String,
}

impl From<TransferInfo> for TransferInfoSerde {
    fn from(t: TransferInfo) -> Self {
        Self {
            from_addr: t.from_addr,
            to_addr: t.to_addr,
            amount: t.amount,
            success: t.success,
            fail_reason: t.fail_reason,
        }
    }
}

impl From<TransferInfoSerde> for TransferInfo {
    fn from(t: TransferInfoSerde) -> Self {
        TransferInfo {
            from_addr: t.from_addr,
            to_addr: t.to_addr,
            amount: t.amount,
            success: t.success,
            fail_reason: t.fail_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployInfoSerde {
    pub deployer: String,
    pub term: String,
    pub timestamp: i64,
    pub sig: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    #[serde(rename = "phloPrice")]
    pub phlo_price: i64,
    #[serde(rename = "phloLimit")]
    pub phlo_limit: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    pub cost: u64,
    pub errored: bool,
    #[serde(rename = "systemDeployError")]
    pub system_deploy_error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<Vec<TransferInfoSerde>>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_parameters"
    )]
    pub parameters: Vec<DeployParameter>,
}

impl TryFrom<DeployInfo> for DeployInfoSerde {
    type Error = ParameterError;

    fn try_from(deploy: DeployInfo) -> Result<Self, Self::Error> {
        Ok(Self {
            deployer: deploy.deployer,
            term: deploy.term,
            timestamp: deploy.timestamp,
            sig: deploy.sig,
            sig_algorithm: deploy.sig_algorithm,
            phlo_price: deploy.phlo_price,
            phlo_limit: deploy.phlo_limit,
            valid_after_block_number: deploy.valid_after_block_number,
            cost: deploy.cost,
            errored: deploy.errored,
            system_deploy_error: deploy.system_deploy_error,
            transfers: Some(
                deploy
                    .transfers
                    .into_iter()
                    .map(TransferInfoSerde::from)
                    .collect(),
            ),
            parameters: parameters_from_proto(deploy.parameters)?,
        })
    }
}

impl From<DeployInfoSerde> for DeployInfo {
    fn from(json: DeployInfoSerde) -> Self {
        let transfers_available = json.transfers.is_some();
        DeployInfo {
            deployer: json.deployer,
            term: json.term,
            timestamp: json.timestamp,
            sig: json.sig,
            sig_algorithm: json.sig_algorithm,
            phlo_price: json.phlo_price,
            phlo_limit: json.phlo_limit,
            valid_after_block_number: json.valid_after_block_number,
            cost: json.cost,
            errored: json.errored,
            system_deploy_error: json.system_deploy_error,
            transfers: json
                .transfers
                .unwrap_or_default()
                .into_iter()
                .map(TransferInfo::from)
                .collect(),
            transfers_available,
            parameters: json
                .parameters
                .iter()
                .map(DeployParameter::to_proto)
                .collect(),
        }
    }
}

impl Default for DeployInfoSerde {
    fn default() -> Self {
        Self {
            deployer: String::new(),
            term: String::new(),
            timestamp: 0,
            sig: String::new(),
            sig_algorithm: String::new(),
            phlo_price: 0,
            phlo_limit: 0,
            valid_after_block_number: 0,
            cost: 0,
            errored: false,
            system_deploy_error: String::new(),
            transfers: Some(Vec::new()),
            parameters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployInfoWithEventDataSerde {
    #[serde(rename = "deployInfo")]
    pub deploy_info: Option<DeployInfoSerde>,
    pub report: Vec<SingleReportSerde>,
}

impl TryFrom<DeployInfoWithEventData> for DeployInfoWithEventDataSerde {
    type Error = ParameterError;

    fn try_from(data: DeployInfoWithEventData) -> Result<Self, Self::Error> {
        Ok(Self {
            deploy_info: data
                .deploy_info
                .map(DeployInfoSerde::try_from)
                .transpose()?,
            report: data.report.into_iter().map(|r| r.into()).collect(),
        })
    }
}

impl From<DeployInfoWithEventDataSerde> for DeployInfoWithEventData {
    fn from(data: DeployInfoWithEventDataSerde) -> Self {
        Self {
            deploy_info: data.deploy_info.map(|d| d.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use models::casper::{BlockEventInfo, BlockInfo};
    use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
    use models::rust::deploy_parameters::{DeployMapEntry, RholangValue, MAX_PARAMETER_BYTES};
    use prost::Message;
    use serde_json::json;
    use utoipa::OpenApi;

    use super::*;
    use crate::rust::api::serde_types::block_event_info::BlockEventInfoSerde;
    use crate::rust::api::serde_types::block_info::BlockInfoSerde;
    use crate::rust::web::web_api_docs::{AdminApi, PublicApi};

    fn deploy_info(parameters: Vec<DeployParameter>) -> DeployInfo {
        let deploy = DeployData {
            term: "Nil".into(),
            time_stamp: 1,
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: 0,
            shard_id: "root".into(),
            expiration_timestamp: None,
            parameters,
        };
        ProcessedDeploy::empty(
            Signed::create(
                deploy,
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[1; 32]),
            )
            .unwrap(),
        )
        .to_deploy_info()
    }

    fn parameters() -> Vec<DeployParameter> {
        use RholangValue::*;
        vec![
            Bool(true),
            Int(i64::MIN),
            Int(i64::MAX),
            String("text\0λ".into()),
            Bytes(vec![0, 127, 255]),
            Tuple(vec![Int(1), Nil]),
            List(vec![Bool(false), Uri("rho:io:stdout".into())]),
            Set(vec![Int(2), Int(1)]),
            Map(vec![DeployMapEntry {
                key: Tuple(vec![Int(1)]),
                value: List(vec![Bytes(vec![0]), Nil]),
            }]),
            Nil,
            Uri("rho:io:stdout".into()),
            Tuple(vec![]),
            List(vec![]),
            Set(vec![]),
            Map(vec![]),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, value)| DeployParameter {
            name: format!("input_{index}"),
            value,
        })
        .collect()
    }

    #[test]
    fn deploy_parameters_survive_processed_deploy_block_and_report_responses() {
        let parameters = parameters();
        let expected = serde_json::to_value(&parameters).unwrap();
        let proto = deploy_info(parameters.clone());
        let proto = DeployInfo::decode(proto.encode_to_vec().as_slice()).unwrap();
        let deploy = DeployInfoSerde::try_from(proto.clone()).unwrap();
        assert_eq!(deploy.parameters, parameters);
        assert_eq!(
            serde_json::to_value(&deploy).unwrap()["parameters"],
            expected
        );
        assert_eq!(DeployInfo::from(deploy).parameters, proto.parameters);

        let block = BlockInfoSerde::try_from(BlockInfo {
            deploys: vec![proto.clone()],
            ..Default::default()
        })
        .unwrap();
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["deploys"][0]["parameters"], expected);
        let restored: BlockInfoSerde = serde_json::from_value(json).unwrap();
        assert_eq!(
            BlockInfo::from(restored).deploys[0].parameters,
            proto.parameters
        );

        let report = BlockEventInfoSerde::try_from(BlockEventInfo {
            deploys: vec![DeployInfoWithEventData {
                deploy_info: Some(proto.clone()),
                report: vec![],
            }],
            ..Default::default()
        })
        .unwrap();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["deploys"][0]["deployInfo"]["parameters"], expected);
        let restored: BlockEventInfoSerde = serde_json::from_value(json).unwrap();
        assert_eq!(
            BlockEventInfo::from(restored).deploys[0]
                .deploy_info
                .as_ref()
                .unwrap()
                .parameters,
            proto.parameters
        );
    }

    #[test]
    fn deploy_parameters_preserve_legacy_responses_and_summary() {
        let deploy = DeployInfoSerde::try_from(deploy_info(vec![])).unwrap();
        let mut json = serde_json::to_value(&deploy).unwrap();
        assert!(json.get("parameters").is_none());
        let restored: DeployInfoSerde = serde_json::from_value(json.clone()).unwrap();
        assert!(restored.parameters.is_empty());
        json["parameters"] = json!([]);
        let restored: DeployInfoSerde = serde_json::from_value(json).unwrap();
        assert!(restored.parameters.is_empty());
        let summary = BlockInfoSerde::from_light(Default::default());
        assert!(serde_json::to_value(summary)
            .unwrap()
            .get("deploys")
            .is_none());
    }

    #[test]
    fn deploy_parameters_reject_malformed_response_data() {
        let invalid = DeployInfo {
            parameters: vec![models::casper::DeployParameter {
                name: "input".into(),
                value: None,
            }],
            ..Default::default()
        };
        assert!(matches!(
            DeployInfoSerde::try_from(invalid.clone()),
            Err(ParameterError::MissingValue)
        ));
        assert!(BlockInfoSerde::try_from(BlockInfo {
            deploys: vec![invalid.clone()],
            ..Default::default()
        })
        .is_err());
        assert!(BlockEventInfoSerde::try_from(BlockEventInfo {
            deploys: vec![DeployInfoWithEventData {
                deploy_info: Some(invalid),
                report: vec![]
            }],
            ..Default::default()
        })
        .is_err());
        let mut json = serde_json::to_value(DeployInfoSerde::default()).unwrap();
        json["parameters"] = json!([{"name": "input", "value": {"type": "unknown"}}]);
        assert!(serde_json::from_value::<DeployInfoSerde>(json).is_err());
    }

    #[test]
    fn deploy_parameters_preserve_maximum_byte_value_in_response() {
        let parameters = vec![DeployParameter {
            name: "input".into(),
            value: RholangValue::Bytes(vec![255; MAX_PARAMETER_BYTES - 5 - 8]),
        }];
        let deploy = DeployInfoSerde::try_from(deploy_info(parameters.clone())).unwrap();
        let json = serde_json::to_vec(&deploy).unwrap();
        let restored: DeployInfoSerde = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.parameters, parameters);
    }

    #[test]
    fn deploy_parameters_appear_in_public_and_admin_response_schemas() {
        for api in [PublicApi::openapi(), AdminApi::openapi()] {
            let json = serde_json::to_value(api).unwrap();
            let schemas = &json["components"]["schemas"];
            for name in ["DeployInfoSerde", "DeployResponse"] {
                assert_eq!(schemas[name]["properties"]["parameters"]["type"], "array");
                assert!(!schemas[name]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("parameters")));
            }
            for name in ["DeployParameter", "RholangValue", "DeployMapEntry"] {
                assert!(schemas.get(name).is_some(), "{name}");
            }
            assert!(schemas["PendingDeployJson"]["properties"]
                .get("parameters")
                .is_none());
        }
    }
}
