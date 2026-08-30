//! JSON serialization/deserialization for DeployInfo
//!
//! This module provides custom JSON serialization for the DeployInfo protobuf type
//! that doesn't have serde derives by default.

use models::casper::{DeployInfo, DeployInfoWithEventData, TransferInfo};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
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
    #[serde(
        default,
        rename = "deployId",
        deserialize_with = "deserialize_deploy_id"
    )]
    pub deploy_id: String,
    pub deployer: String,
    pub term: String,
    pub timestamp: i64,
    pub sig: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
    // D3 (DR-9): phloPrice / phloLimit removed — a deploy's cost is the per-COMM
    // token count (reported in `cost`); there is no escrow price/limit.
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    pub cost: u64,
    pub errored: bool,
    #[serde(rename = "systemDeployError")]
    pub system_deploy_error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<Vec<TransferInfoSerde>>,
}

fn deserialize_deploy_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where D: Deserializer<'de> {
    let value = String::deserialize(deserializer)?;
    hex::decode(&value).map_err(D::Error::custom)?;
    Ok(value)
}

impl From<DeployInfo> for DeployInfoSerde {
    fn from(deploy: DeployInfo) -> Self {
        Self {
            deploy_id: hex::encode(deploy.deploy_id),
            deployer: deploy.deployer,
            term: deploy.term,
            timestamp: deploy.timestamp,
            sig: deploy.sig,
            sig_algorithm: deploy.sig_algorithm,
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
        }
    }
}

impl TryFrom<DeployInfoSerde> for DeployInfo {
    type Error = hex::FromHexError;

    fn try_from(json: DeployInfoSerde) -> Result<Self, Self::Error> {
        let transfers_available = json.transfers.is_some();
        Ok(DeployInfo {
            deploy_id: hex::decode(&json.deploy_id)?.into(),
            deployer: json.deployer,
            term: json.term,
            timestamp: json.timestamp,
            sig: json.sig,
            sig_algorithm: json.sig_algorithm,
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
            authority_funding_certificate: None,
            authority_cost_witness: None,
            pre_state_hash: Default::default(),
            post_state_hash: Default::default(),
            admission_status: Default::default(),
        })
    }
}

impl Default for DeployInfoSerde {
    fn default() -> Self {
        Self {
            deploy_id: String::new(),
            deployer: String::new(),
            term: String::new(),
            timestamp: 0,
            sig: String::new(),
            sig_algorithm: String::new(),
            valid_after_block_number: 0,
            cost: 0,
            errored: false,
            system_deploy_error: String::new(),
            transfers: Some(Vec::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployInfoWithEventDataSerde {
    #[serde(rename = "deployInfo")]
    pub deploy_info: Option<DeployInfoSerde>,
    pub report: Vec<SingleReportSerde>,
}

impl From<DeployInfoWithEventData> for DeployInfoWithEventDataSerde {
    fn from(data: DeployInfoWithEventData) -> Self {
        Self {
            deploy_info: data.deploy_info.map(|d| d.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
        }
    }
}

impl TryFrom<DeployInfoWithEventDataSerde> for DeployInfoWithEventData {
    type Error = hex::FromHexError;

    fn try_from(data: DeployInfoWithEventDataSerde) -> Result<Self, Self::Error> {
        Ok(Self {
            deploy_info: data.deploy_info.map(DeployInfo::try_from).transpose()?,
            report: data.report.into_iter().map(|r| r.into()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use models::casper::DeployInfo;

    use super::DeployInfoSerde;

    #[test]
    fn deploy_id_json_accepts_canonical_hex() {
        let value = serde_json::json!({
            "deployId": "00aaff",
            "deployer": "",
            "term": "Nil",
            "timestamp": 0,
            "sig": "",
            "sigAlgorithm": "",
            "validAfterBlockNumber": 0,
            "cost": 0,
            "errored": false,
            "systemDeployError": "",
            "transfers": []
        });
        let parsed: DeployInfoSerde = serde_json::from_value(value).expect("valid deploy id");
        assert_eq!(parsed.deploy_id, "00aaff");
    }

    #[test]
    fn deploy_id_json_rejects_non_hex_input() {
        let value = serde_json::json!({
            "deployId": "not-a-deploy-id",
            "deployer": "",
            "term": "Nil",
            "timestamp": 0,
            "sig": "",
            "sigAlgorithm": "",
            "validAfterBlockNumber": 0,
            "cost": 0,
            "errored": false,
            "systemDeployError": "",
            "transfers": []
        });
        assert!(serde_json::from_value::<DeployInfoSerde>(value).is_err());
    }

    #[test]
    fn programmatic_deploy_info_conversion_rejects_non_hex_id() {
        let value = DeployInfoSerde {
            deploy_id: "not-a-deploy-id".to_string(),
            ..DeployInfoSerde::default()
        };
        assert!(DeployInfo::try_from(value).is_err());
    }
}
