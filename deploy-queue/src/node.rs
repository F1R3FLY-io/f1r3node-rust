use std::time::Duration;

use models::rust::deploy_request::DeployRequest;
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use shared::rust::token_amount::TokenAmount;

/// A request to the node that takes longer than this has failed; the queue
/// retries it.
const NODE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("node address {0:?} is not a valid http(s) URL")]
    InvalidAddress(String),
    #[error("node request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("node answered {status}: {body}")]
    Status { status: StatusCode, body: String },
}

/// The shard values the queue paces by, read from the node's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardValues {
    pub shard_id: String,
    pub min_phlo_price: TokenAmount,
    pub deploy_lifespan: i64,
    pub max_user_deploys_per_block: u32,
    pub max_parent_depth: i32,
}

/// The node's chain position and shard values at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSnapshot {
    pub shard: ShardValues,
    pub tip: i64,
    pub lfb: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeStatus {
    #[serde(rename = "shardId")]
    pub shard_id: String,
    #[serde(rename = "minPhloPrice")]
    pub min_phlo_price: TokenAmount,
    #[serde(rename = "lastFinalizedBlockNumber")]
    pub last_finalized_block_number: i64,
    #[serde(rename = "latestBlockNumber")]
    pub latest_block_number: Option<i64>,
    #[serde(rename = "deployLifespan")]
    pub deploy_lifespan: Option<i64>,
    #[serde(rename = "maxUserDeploysPerBlock")]
    pub max_user_deploys_per_block: Option<u32>,
    #[serde(rename = "maxParentDepth")]
    pub max_parent_depth: Option<i32>,
}

impl NodeStatus {
    /// The snapshot, once casper is up and has finalized at least genesis.
    /// `Err` for values no running shard can hold.
    pub fn chain(&self) -> Result<Option<ChainSnapshot>, String> {
        let (
            Some(tip),
            Some(deploy_lifespan),
            Some(max_user_deploys_per_block),
            Some(max_parent_depth),
        ) = (
            self.latest_block_number,
            self.deploy_lifespan,
            self.max_user_deploys_per_block,
            self.max_parent_depth,
        )
        else {
            return Ok(None);
        };
        if self.last_finalized_block_number < 0 {
            return Ok(None);
        }
        if deploy_lifespan < 0 {
            return Err(format!(
                "node reports a negative deploy lifespan: {}",
                deploy_lifespan
            ));
        }
        if max_parent_depth < 0 {
            return Err(format!(
                "node reports a negative max parent depth: {}",
                max_parent_depth
            ));
        }
        Ok(Some(ChainSnapshot {
            shard: ShardValues {
                shard_id: self.shard_id.clone(),
                min_phlo_price: self.min_phlo_price.clone(),
                deploy_lifespan,
                max_user_deploys_per_block,
                max_parent_depth,
            },
            tip,
            lfb: self.last_finalized_block_number,
        }))
    }
}

/// The node's answer for one deploy, in its HTTP shape.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NodeDeployStatus {
    pub state: String,
    pub rejection_count: u32,
    pub latest_block_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
    message: String,
}

/// How the node answered one submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    /// The node already holds the deploy.
    Duplicate,
    /// The deploy's window or expiration has closed.
    Expired(String),
    /// The node refused the deploy for a reason resubmitting cannot change.
    Rejected {
        kind: String,
        message: String,
    },
    /// The node could not take the deploy now.
    Retry {
        reason: String,
        retry_after: Option<Duration>,
    },
}

#[derive(Clone)]
pub struct NodeHttp {
    client: reqwest::Client,
    base: Url,
}

impl NodeHttp {
    pub fn new(base: &str) -> Result<Self, NodeError> {
        let base = Url::parse(base).map_err(|_| NodeError::InvalidAddress(base.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") || base.host().is_none() {
            return Err(NodeError::InvalidAddress(base.to_string()));
        }
        let client = reqwest::Client::builder()
            .timeout(NODE_REQUEST_TIMEOUT)
            .build()?;
        Ok(Self { client, base })
    }

    pub fn client(&self) -> &reqwest::Client { &self.client }

    /// The node URL for a path and query of the queue's own API.
    pub fn url(&self, path_and_query: &str) -> Url {
        let mut url = self.base.clone();
        let (path, query) = match path_and_query.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (path_and_query, None),
        };
        url.set_path(path);
        url.set_query(query);
        url
    }

    pub fn ws_events_url(&self) -> String {
        let mut url = self.url("/ws/events");
        let scheme = if self.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        url.set_scheme(scheme)
            .expect("http and https URLs take the ws schemes");
        url.to_string()
    }

    pub async fn status(&self) -> Result<NodeStatus, NodeError> {
        let response = self.client.get(self.url("/api/status")).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(NodeError::Status {
                status,
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }

    pub async fn deploy_status(&self, sig: &[u8]) -> Result<NodeDeployStatus, NodeError> {
        let path = format!("/api/deploy-finalization-status/{}", hex::encode(sig));
        let response = self.client.get(self.url(&path)).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(NodeError::Status {
                status,
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }

    pub async fn submit(&self, request: &DeployRequest) -> SubmitOutcome {
        let response = match self
            .client
            .post(self.url("/api/deploy"))
            .json(request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return SubmitOutcome::Retry {
                    reason: format!("node request failed: {}", e),
                    retry_after: None,
                }
            }
        };
        let status = response.status();
        if status.is_success() {
            return SubmitOutcome::Accepted;
        }
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = match response.text().await {
            Ok(body) => body,
            Err(e) => {
                return SubmitOutcome::Retry {
                    reason: format!("node answered {} with an unreadable body: {}", status, e),
                    retry_after,
                }
            }
        };
        classify_submit(status, &body, retry_after)
    }
}

fn classify_submit(status: StatusCode, body: &str, retry_after: Option<Duration>) -> SubmitOutcome {
    let Ok(error) = serde_json::from_str::<ApiError>(body) else {
        return SubmitOutcome::Retry {
            reason: format!("node answered {}: {}", status, body),
            retry_after,
        };
    };
    match (status, error.error.as_str()) {
        (StatusCode::CONFLICT, "duplicate_deploy") => SubmitOutcome::Duplicate,
        (StatusCode::BAD_REQUEST, "deploy_expired") => SubmitOutcome::Expired(error.message),
        (status, _) if status.is_client_error() => SubmitOutcome::Rejected {
            kind: error.error,
            message: error.message,
        },
        _ => SubmitOutcome::Retry {
            reason: format!(
                "node answered {} {}: {}",
                status, error.error, error.message
            ),
            retry_after,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(kind: &str) -> String { format!(r#"{{"error":"{}","message":"m"}}"#, kind) }

    #[test]
    fn submissions_classify_by_status_and_kind() {
        assert_eq!(
            classify_submit(StatusCode::CONFLICT, &body("duplicate_deploy"), None),
            SubmitOutcome::Duplicate
        );
        assert_eq!(
            classify_submit(StatusCode::BAD_REQUEST, &body("deploy_expired"), None),
            SubmitOutcome::Expired("m".to_string())
        );
        assert_eq!(
            classify_submit(StatusCode::BAD_REQUEST, &body("illegal_argument"), None),
            SubmitOutcome::Rejected {
                kind: "illegal_argument".to_string(),
                message: "m".to_string()
            }
        );
        assert!(matches!(
            classify_submit(
                StatusCode::SERVICE_UNAVAILABLE,
                &body("block_not_held"),
                Some(Duration::from_secs(3))
            ),
            SubmitOutcome::Retry {
                retry_after: Some(_),
                ..
            }
        ));
        assert!(matches!(
            classify_submit(StatusCode::INTERNAL_SERVER_ERROR, "not json", None),
            SubmitOutcome::Retry { .. }
        ));
    }

    #[test]
    fn urls_keep_the_node_host_and_take_the_request_path() {
        let node = NodeHttp::new("http://validator:40403").unwrap();
        assert_eq!(
            node.url("/api/blocks/1/2?view=summary").as_str(),
            "http://validator:40403/api/blocks/1/2?view=summary"
        );
        assert_eq!(node.ws_events_url(), "ws://validator:40403/ws/events");
        assert!(NodeHttp::new("validator:40403").is_err());
    }

    #[test]
    fn a_status_without_pacing_values_is_not_a_chain_yet() {
        let status: NodeStatus = serde_json::from_str(
            r#"{"shardId":"root","minPhloPrice":"1","lastFinalizedBlockNumber":-1,
                "latestBlockNumber":null,"deployLifespan":null,
                "maxUserDeploysPerBlock":null,"maxParentDepth":null}"#,
        )
        .unwrap();
        assert_eq!(status.chain(), Ok(None));

        let status: NodeStatus = serde_json::from_str(
            r#"{"shardId":"root","minPhloPrice":"1","lastFinalizedBlockNumber":40,
                "latestBlockNumber":43,"deployLifespan":300,
                "maxUserDeploysPerBlock":4,"maxParentDepth":25}"#,
        )
        .unwrap();
        let chain = status.chain().unwrap().unwrap();
        assert_eq!((chain.tip, chain.lfb), (43, 40));
        assert_eq!(chain.shard.max_parent_depth, 25);
    }
}
