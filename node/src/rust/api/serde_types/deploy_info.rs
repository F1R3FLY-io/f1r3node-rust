//! JSON serialization/deserialization for DeployInfo
//!
//! This module provides custom JSON serialization for the DeployInfo protobuf type
//! that doesn't have serde derives by default.

// Changed by DR-134 (DR-70): the offered deploy report.
// use models::casper::{DeployInfo, DeployInfoWithEventData, TransferInfo};
use models::casper::{
    DeployInfo, DeployInfoWithEventData, OfferedDeployReport, OfferedPurseReport,
    OfferedSettlementReport, TransferInfo,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Added by DR-134 (DR-70): the byte encoding of the block report.
use super::base64_bytes;
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
}

impl From<DeployInfo> for DeployInfoSerde {
    fn from(deploy: DeployInfo) -> Self {
        Self {
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
        }
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
        }
    }
}

/// Added by DR-134 (DR-70): one purse row of the settlement receipt of an
/// offered deploy. The REV amounts are decimal strings, because they are
/// 128-bit values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OfferedPurseReportSerde {
    #[serde(with = "base64_bytes")]
    pub address: Vec<u8>,
    #[serde(rename = "resourceRev")]
    pub resource_rev: String,
    #[serde(rename = "feeRev")]
    pub fee_rev: String,
    #[serde(rename = "postBalance")]
    pub post_balance: u64,
}

/// Added by DR-134 (DR-70): the settlement receipt of an offered deploy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OfferedSettlementReportSerde {
    #[serde(rename = "phloUsed")]
    pub phlo_used: u64,
    #[serde(rename = "freshPhlo")]
    pub fresh_phlo: u64,
    #[serde(rename = "retainedPhlo")]
    pub retained_phlo: u64,
    #[serde(rename = "phloLimit")]
    pub phlo_limit: u64,
    #[serde(rename = "phloPrice")]
    pub phlo_price: u64,
    #[serde(rename = "feeRev")]
    pub fee_rev: String,
    #[serde(rename = "revSpent")]
    pub rev_spent: String,
    #[serde(rename = "revCeiling")]
    pub rev_ceiling: String,
    pub purses: Vec<OfferedPurseReportSerde>,
}

/// Added by DR-134 (DR-70): the committed native cost evidence of an offered
/// deploy and its settlement receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OfferedDeployReportSerde {
    #[serde(rename = "nativeCostEvidence", with = "base64_bytes")]
    pub native_cost_evidence: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<OfferedSettlementReportSerde>,
}

impl From<OfferedDeployReport> for OfferedDeployReportSerde {
    fn from(report: OfferedDeployReport) -> Self {
        Self {
            native_cost_evidence: report.native_cost_evidence.to_vec(),
            receipt: report.receipt.map(|receipt| OfferedSettlementReportSerde {
                phlo_used: receipt.phlo_used,
                fresh_phlo: receipt.fresh_phlo,
                retained_phlo: receipt.retained_phlo,
                phlo_limit: receipt.phlo_limit,
                phlo_price: receipt.phlo_price,
                fee_rev: receipt.fee_rev,
                rev_spent: receipt.rev_spent,
                rev_ceiling: receipt.rev_ceiling,
                purses: receipt
                    .purses
                    .into_iter()
                    .map(|purse| OfferedPurseReportSerde {
                        address: purse.address.to_vec(),
                        resource_rev: purse.resource_rev,
                        fee_rev: purse.fee_rev,
                        post_balance: purse.post_balance,
                    })
                    .collect(),
            }),
        }
    }
}

impl From<OfferedDeployReportSerde> for OfferedDeployReport {
    fn from(report: OfferedDeployReportSerde) -> Self {
        Self {
            native_cost_evidence: report.native_cost_evidence.into(),
            receipt: report.receipt.map(|receipt| OfferedSettlementReport {
                phlo_used: receipt.phlo_used,
                fresh_phlo: receipt.fresh_phlo,
                retained_phlo: receipt.retained_phlo,
                phlo_limit: receipt.phlo_limit,
                phlo_price: receipt.phlo_price,
                fee_rev: receipt.fee_rev,
                rev_spent: receipt.rev_spent,
                rev_ceiling: receipt.rev_ceiling,
                purses: receipt
                    .purses
                    .into_iter()
                    .map(|purse| OfferedPurseReport {
                        address: purse.address.into(),
                        resource_rev: purse.resource_rev,
                        fee_rev: purse.fee_rev,
                        post_balance: purse.post_balance,
                    })
                    .collect(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployInfoWithEventDataSerde {
    #[serde(rename = "deployInfo")]
    pub deploy_info: Option<DeployInfoSerde>,
    pub report: Vec<SingleReportSerde>,
    // Added by DR-134 (DR-70): set only for an offered deploy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offered: Option<OfferedDeployReportSerde>,
}

impl From<DeployInfoWithEventData> for DeployInfoWithEventDataSerde {
    fn from(data: DeployInfoWithEventData) -> Self {
        Self {
            deploy_info: data.deploy_info.map(|d| d.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
            // Added by DR-134 (DR-70).
            offered: data.offered.map(|offered| offered.into()),
        }
    }
}

impl From<DeployInfoWithEventDataSerde> for DeployInfoWithEventData {
    fn from(data: DeployInfoWithEventDataSerde) -> Self {
        Self {
            deploy_info: data.deploy_info.map(|d| d.into()),
            report: data.report.into_iter().map(|r| r.into()).collect(),
            // Added by DR-134 (DR-70).
            offered: data.offered.map(|offered| offered.into()),
        }
    }
}
