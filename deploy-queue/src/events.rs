use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::engine::Command;
use crate::node::NodeHttp;

const RECONNECT_FLOOR: Duration = Duration::from_secs(1);
const RECONNECT_CEILING: Duration = Duration::from_secs(30);

/// A node event the queue acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEvent {
    /// A block entered the node's DAG (one it created or one it received).
    Block {
        number: i64,
        hash: String,
        deploys: Vec<String>,
    },
    Finalised {
        number: i64,
    },
    /// The verdicts one settle wrote.
    Verdicts(Vec<Verdict>),
    /// The node dropped events this subscriber did not read in time.
    Lagged {
        skipped: u64,
    },
    Running,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerdictState {
    Finalized,
    Expired,
    Failed,
}

#[derive(Deserialize)]
struct Envelope {
    event: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BlockPayload {
    block_hash: String,
    block_number: i64,
    deploys: Vec<DeployPayload>,
}

#[derive(Deserialize)]
struct DeployPayload {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Verdict {
    pub deploy_id: String,
    pub state: VerdictState,
    pub rejection_count: u32,
    pub block_hash: Option<String>,
}

#[derive(Deserialize)]
struct VerdictsPayload {
    verdicts: Vec<Verdict>,
}

#[derive(Deserialize)]
struct LagPayload {
    skipped: u64,
}

pub fn parse_event(text: &str) -> Result<NodeEvent, serde_json::Error> {
    let envelope: Envelope = serde_json::from_str(text)?;
    Ok(match envelope.event.as_str() {
        "block-added" | "block-created" => {
            let block: BlockPayload = serde_json::from_value(envelope.payload)?;
            NodeEvent::Block {
                number: block.block_number,
                hash: block.block_hash,
                deploys: block.deploys.into_iter().map(|d| d.id).collect(),
            }
        }
        "block-finalised" => {
            let block: BlockPayload = serde_json::from_value(envelope.payload)?;
            NodeEvent::Finalised {
                number: block.block_number,
            }
        }
        "deploy-verdicts" => {
            let batch: VerdictsPayload = serde_json::from_value(envelope.payload)?;
            NodeEvent::Verdicts(batch.verdicts)
        }
        "stream-lagged" => {
            let lag: LagPayload = serde_json::from_value(envelope.payload)?;
            NodeEvent::Lagged {
                skipped: lag.skipped,
            }
        }
        "entered-running-state" => NodeEvent::Running,
        _ => NodeEvent::Ignored,
    })
}

#[derive(Debug, thiserror::Error)]
enum LinkError {
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("node event does not parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("node status: {0}")]
    Status(String),
    #[error("deploy queue engine has stopped")]
    EngineStopped,
}

/// Holds one event subscription to the node for the life of the process,
/// reconnecting with backoff. Each connection subscribes first and reads the
/// node's status second, so no event between the two is missed; the engine
/// resynchronizes its in-flight deploys on every connect.
pub async fn run_event_link(node: NodeHttp, commands: mpsc::Sender<Command>) {
    let mut delay = RECONNECT_FLOOR;
    loop {
        match link_once(&node, &commands, &mut delay).await {
            Ok(()) => tracing::warn!(target: "deploy_queue::link", "node closed the event stream"),
            Err(LinkError::EngineStopped) => return,
            Err(e) => {
                tracing::warn!(target: "deploy_queue::link", error = %e, "node event link failed")
            }
        }
        if commands.send(Command::Disconnected).await.is_err() {
            return;
        }
        metrics::counter!("deploy_queue.node_link_reconnects").increment(1);
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RECONNECT_CEILING);
    }
}

async fn link_once(
    node: &NodeHttp,
    commands: &mpsc::Sender<Command>,
    delay: &mut Duration,
) -> Result<(), LinkError> {
    let (mut socket, _) = tokio_tungstenite::connect_async(node.ws_events_url()).await?;
    *delay = RECONNECT_FLOOR;
    let mut connected = announce(node, commands).await?;
    while let Some(message) = socket.next().await {
        let text = match message? {
            Message::Text(text) => text,
            Message::Close(_) => return Ok(()),
            _ => continue,
        };
        let event = parse_event(text.as_str())?;
        match event {
            NodeEvent::Running if !connected => connected = announce(node, commands).await?,
            NodeEvent::Ignored => {}
            event => commands
                .send(Command::Node(event))
                .await
                .map_err(|_| LinkError::EngineStopped)?,
        }
    }
    Ok(())
}

/// Reads the node's status and hands the engine the chain snapshot. False
/// when casper is not up yet; the link retries on `entered-running-state`.
async fn announce(node: &NodeHttp, commands: &mpsc::Sender<Command>) -> Result<bool, LinkError> {
    let status = node
        .status()
        .await
        .map_err(|e| LinkError::Status(e.to_string()))?;
    let Some(chain) = status.chain().map_err(LinkError::Status)? else {
        tracing::info!(target: "deploy_queue::link", "node is not running casper yet; waiting for it");
        return Ok(false);
    };
    commands
        .send(Command::Connected(chain))
        .await
        .map_err(|_| LinkError::EngineStopped)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_events_carry_their_deploy_ids() {
        let event = parse_event(
            r#"{"event":"block-added","schema-version":1,"payload":{"block-hash":"ab",
                "block-number":7,"timestamp":1,"parent-hashes":[],"justification-hashes":[],
                "deploys":[{"id":"01","cost":1,"deployer":"d","errored":false}],
                "creator":"c","seq-num":1}}"#,
        )
        .unwrap();
        assert_eq!(event, NodeEvent::Block {
            number: 7,
            hash: "ab".to_string(),
            deploys: vec!["01".to_string()],
        });
    }

    #[test]
    fn verdicts_lags_and_unknown_events_parse() {
        assert_eq!(
            parse_event(
                r#"{"event":"deploy-verdicts","schema-version":1,"payload":{"verdicts":[
                    {"deploy-id":"01","state":"finalized","rejection-count":0,"block-hash":"ab"},
                    {"deploy-id":"02","state":"expired","rejection-count":1,"block-hash":null}]}}"#
            )
            .unwrap(),
            NodeEvent::Verdicts(vec![
                Verdict {
                    deploy_id: "01".to_string(),
                    state: VerdictState::Finalized,
                    rejection_count: 0,
                    block_hash: Some("ab".to_string()),
                },
                Verdict {
                    deploy_id: "02".to_string(),
                    state: VerdictState::Expired,
                    rejection_count: 1,
                    block_hash: None,
                },
            ])
        );
        assert_eq!(
            parse_event(r#"{"event":"stream-lagged","schema-version":1,"payload":{"skipped":4}}"#)
                .unwrap(),
            NodeEvent::Lagged { skipped: 4 }
        );
        assert_eq!(
            parse_event(r#"{"event":"started","schema-version":1}"#).unwrap(),
            NodeEvent::Ignored
        );
        assert!(parse_event(r#"{"event":"block-added","payload":{}}"#).is_err());
    }
}
