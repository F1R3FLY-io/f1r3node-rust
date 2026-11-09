//! JSON serialization/deserialization for BlockInfo and related types
//!
//! This module provides custom JSON serialization for protobuf-generated types
//! that don't have serde derives by default.

use models::casper::{BlockInfo, DeployInfo};
use models::rust::deploy_parameters::ParameterError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

use crate::rust::api::serde_types::deploy_info::DeployInfoSerde;
use crate::rust::api::serde_types::light_block_info::LightBlockInfoSerde;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[derive(Default)]
pub struct BlockInfoSerde {
    #[serde(rename = "blockInfo")]
    pub block_info: LightBlockInfoSerde,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploys: Option<Vec<DeployInfoSerde>>,
}

impl TryFrom<BlockInfo> for BlockInfoSerde {
    type Error = ParameterError;

    fn try_from(block: BlockInfo) -> Result<Self, Self::Error> {
        Ok(Self {
            block_info: block.block_info.unwrap_or_default().into(),
            deploys: Some(
                block
                    .deploys
                    .into_iter()
                    .map(DeployInfoSerde::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        })
    }
}

impl From<BlockInfoSerde> for BlockInfo {
    fn from(json: BlockInfoSerde) -> Self {
        BlockInfo {
            block_info: Some(json.block_info.into()),
            deploys: json
                .deploys
                .unwrap_or_default()
                .into_iter()
                .map(DeployInfo::from)
                .collect(),
        }
    }
}

pub fn serialize_block_info<S>(block: BlockInfo, serializer: S) -> Result<S::Ok, S::Error>
where S: Serializer {
    let json_block = BlockInfoSerde::try_from(block).map_err(serde::ser::Error::custom)?;
    json_block.serialize(serializer)
}

pub fn deserialize_block_info<'de, D>(deserializer: D) -> Result<BlockInfo, D::Error>
where D: Deserializer<'de> {
    let json_block = BlockInfoSerde::deserialize(deserializer)?;
    Ok(json_block.into())
}

impl BlockInfoSerde {
    /// Create a summary view (block info only, no deploys).
    pub fn from_light(light: LightBlockInfoSerde) -> Self {
        Self {
            block_info: light,
            deploys: None,
        }
    }
}
