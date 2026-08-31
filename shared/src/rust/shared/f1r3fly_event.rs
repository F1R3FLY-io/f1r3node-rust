// F1r3flyEvent — node event types for WebSocket streaming.
// Ported from shared/src/main/scala/coop/rchain/shared/RChainEvent.scala

use serde::{Deserialize, Serialize};

/// Transfer event within a deploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TransferEvent {
    pub from_addr: String,
    pub to_addr: String,
    pub amount: i64,
    pub success: bool,
}

/// Deploy event information included in block events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DeployEvent {
    /// Deploy signature ID
    pub id: String,
    /// Deploy execution cost
    pub cost: i64,
    /// Deployer public key
    pub deployer: String,
    /// Whether the deploy execution failed
    pub errored: bool,
    /// Transfers extracted from this deploy.
    /// None on BlockCreated/BlockAdded (not yet available).
    /// Populated on BlockFinalised only when transfers are enriched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<Vec<TransferEvent>>,
}

impl DeployEvent {
    pub fn new(id: String, cost: i64, deployer: String, errored: bool) -> Self {
        Self {
            id,
            cost,
            deployer,
            errored,
            transfers: None,
        }
    }
}

/// Per-deploy transfer data for the TransfersAvailable event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DeployTransfers {
    pub deploy_id: String,
    pub transfers: Vec<TransferEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum F1r3flyEvent {
    BlockCreated(BlockCreated),
    BlockAdded(BlockAdded),
    BlockFinalised(BlockFinalised),
    TransfersAvailable(TransfersAvailable),
    SentUnapprovedBlock(SentUnapprovedBlockData),
    SentApprovedBlock(SentApprovedBlockData),
    BlockApprovalReceived(BlockApprovalReceived),
    ApprovedBlockReceived(ApprovedBlockReceived),
    EnteredRunningState(EnteredRunningState),
    NodeStarted(NodeStarted),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlockCreated {
    pub block_hash: String,
    pub block_number: i64,
    pub timestamp: i64,
    pub parent_hashes: Vec<String>,
    pub justification_hashes: Vec<(String, String)>,
    pub deploys: Vec<DeployEvent>,
    pub creator: String,
    #[serde(rename = "seq-num")]
    pub seq_number: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlockAdded {
    pub block_hash: String,
    pub block_number: i64,
    pub timestamp: i64,
    pub parent_hashes: Vec<String>,
    pub justification_hashes: Vec<(String, String)>,
    pub deploys: Vec<DeployEvent>,
    pub creator: String,
    #[serde(rename = "seq-num")]
    pub seq_number: i32,
}

/// BlockFinalised event with full block metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlockFinalised {
    pub block_hash: String,
    pub block_number: i64,
    pub timestamp: i64,
    pub parent_hashes: Vec<String>,
    pub justification_hashes: Vec<(String, String)>,
    pub deploys: Vec<DeployEvent>,
    pub creator: String,
    #[serde(rename = "seq-num")]
    pub seq_number: i32,
}

/// Emitted after BlockFinalised when transfer extraction completes.
/// Clients that need transfer data listen for this event.
/// Only emitted on readonly nodes (validators cannot extract transfers).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TransfersAvailable {
    pub block_hash: String,
    pub block_number: i64,
    pub deploys: Vec<DeployTransfers>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlockApprovalReceived {
    pub block_hash: String,
    pub sender: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApprovedBlockReceived {
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EnteredRunningState {
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SentUnapprovedBlockData {
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SentApprovedBlockData {
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NodeStarted {
    pub address: String,
}

impl F1r3flyEvent {
    pub fn block_created(
        block_hash: String,
        block_number: i64,
        timestamp: i64,
        parent_hashes: Vec<String>,
        justification_hashes: Vec<(String, String)>,
        deploys: Vec<DeployEvent>,
        creator: String,
        seq_number: i32,
    ) -> Self {
        Self::BlockCreated(BlockCreated {
            block_hash,
            block_number,
            timestamp,
            parent_hashes,
            justification_hashes,
            deploys,
            creator,
            seq_number,
        })
    }

    pub fn block_added(
        block_hash: String,
        block_number: i64,
        timestamp: i64,
        parent_hashes: Vec<String>,
        justification_hashes: Vec<(String, String)>,
        deploys: Vec<DeployEvent>,
        creator: String,
        seq_number: i32,
    ) -> Self {
        Self::BlockAdded(BlockAdded {
            block_hash,
            block_number,
            timestamp,
            parent_hashes,
            justification_hashes,
            deploys,
            creator,
            seq_number,
        })
    }

    pub fn block_finalised(
        block_hash: String,
        block_number: i64,
        timestamp: i64,
        parent_hashes: Vec<String>,
        justification_hashes: Vec<(String, String)>,
        deploys: Vec<DeployEvent>,
        creator: String,
        seq_number: i32,
    ) -> Self {
        Self::BlockFinalised(BlockFinalised {
            block_hash,
            block_number,
            timestamp,
            parent_hashes,
            justification_hashes,
            deploys,
            creator,
            seq_number,
        })
    }

    pub fn transfers_available(
        block_hash: String,
        block_number: i64,
        deploys: Vec<DeployTransfers>,
    ) -> Self {
        Self::TransfersAvailable(TransfersAvailable {
            block_hash,
            block_number,
            deploys,
        })
    }

    pub fn approved_block_received(block_hash: String) -> Self {
        Self::ApprovedBlockReceived(ApprovedBlockReceived { block_hash })
    }

    pub fn entered_running_state(block_hash: String) -> Self {
        Self::EnteredRunningState(EnteredRunningState { block_hash })
    }

    pub fn sent_unapproved_block(block_hash: String) -> Self {
        Self::SentUnapprovedBlock(SentUnapprovedBlockData { block_hash })
    }

    pub fn sent_approved_block(block_hash: String) -> Self {
        Self::SentApprovedBlock(SentApprovedBlockData { block_hash })
    }

    pub fn block_approval_received(block_hash: String, sender: String) -> Self {
        Self::BlockApprovalReceived(BlockApprovalReceived { block_hash, sender })
    }

    pub fn node_started(address: String) -> Self { Self::NodeStarted(NodeStarted { address }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_deploy() -> DeployEvent {
        DeployEvent::new("sig".to_string(), 100, "deployer-pk".to_string(), false)
    }

    type BlockEventCtor = fn(
        String,
        i64,
        i64,
        Vec<String>,
        Vec<(String, String)>,
        Vec<DeployEvent>,
        String,
        i32,
    ) -> F1r3flyEvent;

    fn sample_block_event(ctor: BlockEventCtor) -> F1r3flyEvent {
        ctor(
            "hash".to_string(),
            7,
            1234,
            vec!["parent".to_string()],
            vec![("v1".to_string(), "j1".to_string())],
            vec![sample_deploy()],
            "creator".to_string(),
            3,
        )
    }

    #[test]
    fn deploy_event_new_leaves_transfers_unset_and_omits_them_in_json() {
        let deploy = sample_deploy();
        assert!(deploy.transfers.is_none());

        let json = serde_json::to_value(&deploy).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("transfers"));
        assert_eq!(obj["id"], "sig");
        assert_eq!(obj["cost"], 100);
        assert_eq!(obj["errored"], false);
    }

    #[test]
    fn deploy_event_serializes_transfers_when_present() {
        let mut deploy = sample_deploy();
        deploy.transfers = Some(vec![TransferEvent {
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            amount: 5,
            success: true,
        }]);

        let json = serde_json::to_value(&deploy).unwrap();
        let transfer = &json["transfers"][0];
        assert_eq!(transfer["from-addr"], "from");
        assert_eq!(transfer["to-addr"], "to");
        assert_eq!(transfer["amount"], 5);
        assert_eq!(transfer["success"], true);
    }

    #[test]
    fn block_events_use_kebab_case_tag_and_field_names() {
        for (ctor, tag) in [
            (
                F1r3flyEvent::block_created as fn(_, _, _, _, _, _, _, _) -> _,
                "block-created",
            ),
            (F1r3flyEvent::block_added, "block-added"),
            (F1r3flyEvent::block_finalised, "block-finalised"),
        ] {
            let json = serde_json::to_value(sample_block_event(ctor)).unwrap();
            assert_eq!(json["event"], tag);
            assert_eq!(json["block-hash"], "hash");
            assert_eq!(json["block-number"], 7);
            assert_eq!(json["seq-num"], 3);
            assert_eq!(json["parent-hashes"][0], "parent");
            assert_eq!(json["justification-hashes"][0][0], "v1");
            assert_eq!(json["deploys"][0]["deployer"], "deployer-pk");
        }
    }

    #[test]
    fn block_events_round_trip_through_json() {
        let original = sample_block_event(F1r3flyEvent::block_added);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: F1r3flyEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            F1r3flyEvent::BlockAdded(block) => {
                assert_eq!(block.block_hash, "hash");
                assert_eq!(block.block_number, 7);
                assert_eq!(block.seq_number, 3);
                assert_eq!(block.deploys.len(), 1);
                assert_eq!(block.deploys[0].id, "sig");
            }
            other => panic!("expected BlockAdded, got {other:?}"),
        }
    }

    #[test]
    fn transfers_available_round_trips_through_json() {
        let original =
            F1r3flyEvent::transfers_available("hash".to_string(), 9, vec![DeployTransfers {
                deploy_id: "sig".to_string(),
                transfers: vec![TransferEvent {
                    from_addr: "from".to_string(),
                    to_addr: "to".to_string(),
                    amount: 11,
                    success: false,
                }],
            }]);

        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["event"], "transfers-available");
        assert_eq!(json["deploys"][0]["deploy-id"], "sig");

        let decoded: F1r3flyEvent = serde_json::from_value(json).unwrap();
        match decoded {
            F1r3flyEvent::TransfersAvailable(data) => {
                assert_eq!(data.block_number, 9);
                assert_eq!(data.deploys[0].transfers[0].amount, 11);
            }
            other => panic!("expected TransfersAvailable, got {other:?}"),
        }
    }

    #[test]
    fn simple_events_carry_their_payload_and_kebab_case_tags() {
        let cases = [
            (
                F1r3flyEvent::sent_unapproved_block("h".to_string()),
                "sent-unapproved-block",
            ),
            (
                F1r3flyEvent::sent_approved_block("h".to_string()),
                "sent-approved-block",
            ),
            (
                F1r3flyEvent::approved_block_received("h".to_string()),
                "approved-block-received",
            ),
            (
                F1r3flyEvent::entered_running_state("h".to_string()),
                "entered-running-state",
            ),
        ];
        for (event, tag) in cases {
            let json = serde_json::to_value(&event).unwrap();
            assert_eq!(json["event"], tag, "{event:?}");
            assert_eq!(json["block-hash"], "h", "{event:?}");
        }

        let approval = serde_json::to_value(F1r3flyEvent::block_approval_received(
            "h".to_string(),
            "s".to_string(),
        ))
        .unwrap();
        assert_eq!(approval["event"], "block-approval-received");
        assert_eq!(approval["sender"], "s");

        let started = serde_json::to_value(F1r3flyEvent::node_started("addr".to_string())).unwrap();
        assert_eq!(started["event"], "node-started");
        assert_eq!(started["address"], "addr");
    }
}
