//! JSON serialization for the `/blocks/{start}/{end}` response.
//!
//! The by-heights HTTP route returns an envelope — `blocks` (served
//! bodies), `pruned` (hashes `topo_sort` named at a height where the
//! retention sweep has already advanced past — permanent gap),
//! `unavailable` (hashes where the sweep has not yet reached the
//! height but the body is absent — retry eligible), and `pruneFloor`
//! (the effective-anchor at the moment of the walk). See issue #1087
//! and PR #1052's single-hash precedent for the `block_pruned` split.

use casper::rust::api::block_api::{BlocksByHeightsResponse, MissingBlockEntry};
use models::casper::{BlockInfo, LightBlockInfo};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::block_info::BlockInfoSerde;
use super::light_block_info::LightBlockInfoSerde;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MissingBlockInfoSerde {
    /// Hex-encoded block hash.
    pub hash: String,
    #[serde(rename = "blockHeight")]
    pub block_height: i64,
}

impl From<MissingBlockEntry> for MissingBlockInfoSerde {
    fn from(entry: MissingBlockEntry) -> Self {
        Self {
            hash: hex::encode(entry.hash),
            block_height: entry.block_height,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BlocksByHeightsResponseSerde {
    pub blocks: Vec<BlockInfoSerde>,
    /// Hashes whose block_height <= pruneFloor — permanent on this
    /// node. Retry on this node cannot succeed; try an archive.
    pub pruned: Vec<MissingBlockInfoSerde>,
    /// Hashes whose block_height > pruneFloor — the sweep has not
    /// reached this height yet but the body is missing. Retry may
    /// succeed.
    pub unavailable: Vec<MissingBlockInfoSerde>,
    #[serde(rename = "pruneFloor")]
    pub prune_floor: i64,
}

impl BlocksByHeightsResponseSerde {
    pub fn from_full(response: BlocksByHeightsResponse<BlockInfo>) -> eyre::Result<Self> {
        let blocks = response
            .served
            .into_iter()
            .map(BlockInfoSerde::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            blocks,
            pruned: response.pruned.into_iter().map(Into::into).collect(),
            unavailable: response.unavailable.into_iter().map(Into::into).collect(),
            prune_floor: response.prune_floor,
        })
    }

    pub fn from_light(response: BlocksByHeightsResponse<LightBlockInfo>) -> Self {
        let blocks = response
            .served
            .into_iter()
            .map(|b| BlockInfoSerde::from_light(LightBlockInfoSerde::from(b)))
            .collect();
        Self {
            blocks,
            pruned: response.pruned.into_iter().map(Into::into).collect(),
            unavailable: response.unavailable.into_iter().map(Into::into).collect(),
            prune_floor: response.prune_floor,
        }
    }
}

/// Union of the two shapes `/blocks/{start}/{end}` can return.
/// `include_missing=false` (default) yields `BareArray`; `=true`
/// yields `Envelope`. Serialized untagged so the on-wire shape
/// exactly matches each variant, and declared as the response body
/// so clients generated from the OpenAPI schema see a `oneOf` and
/// decode either shape. Without this a client that didn't opt in
/// would try to decode the envelope schema against a bare array and
/// fail on a successful request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum BlocksByHeightsHttpResponse {
    BareArray(Vec<BlockInfoSerde>),
    Envelope(BlocksByHeightsResponseSerde),
}
