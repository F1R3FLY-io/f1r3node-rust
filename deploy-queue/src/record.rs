use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;
use serde::{Deserialize, Serialize};

pub type Sig = Vec<u8>;

/// Where a deploy is in the queue's lifecycle.
///
/// `Queued` deploys wait in the queue; `Submitted` ones are in the node's
/// pool but in no block the queue has seen; `InBlock` ones are in a block and
/// wait for the node's verdict. The four terminal states never change:
/// `Finalized`, `Failed` and `Expired` mirror the node's verdicts (or, for a
/// deploy the queue never delivered, its own `Expired`), and `Rejected` is a
/// deploy the node refused outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueueState {
    Queued,
    Submitted,
    InBlock,
    Finalized,
    Failed,
    Expired,
    Rejected,
}

impl QueueState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            QueueState::Finalized | QueueState::Failed | QueueState::Expired | QueueState::Rejected
        )
    }

    /// The node's deploy-status vocabulary, plus `Rejected`, which only a
    /// queue answers.
    pub fn status_label(self) -> &'static str {
        match self {
            QueueState::Queued | QueueState::Submitted | QueueState::InBlock => "Pending",
            QueueState::Finalized => "Finalized",
            QueueState::Failed => "Failed",
            QueueState::Rejected => "Rejected",
            QueueState::Expired => "Expired",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            QueueState::Queued => "queued",
            QueueState::Submitted => "submitted",
            QueueState::InBlock => "in-block",
            QueueState::Finalized => "finalized",
            QueueState::Failed => "failed",
            QueueState::Expired => "expired",
            QueueState::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRecord {
    pub deployer: Vec<u8>,
    /// The size of the stored signed deploy.
    pub payload_bytes: u64,
    pub valid_after: i64,
    /// Intake order; release order within one deployer follows it.
    pub sequence: u64,
    pub accepted_at_ms: i64,
    pub state: QueueState,
    /// The tip height when the queue released the deploy to the node.
    pub submitted_at_height: Option<i64>,
    /// The tip height when the deploy was last sent or resent to the node.
    pub last_sent_height: Option<i64>,
    pub submissions: u32,
    pub rejection_count: u32,
    pub latest_block_hash: Option<Vec<u8>>,
    /// The tip height when the deploy became terminal.
    pub decided_at_height: Option<i64>,
    /// Why a queue-decided terminal state was reached.
    pub detail: Option<String>,
}

impl QueueRecord {
    pub fn new(
        deploy: &Signed<DeployData>,
        payload_bytes: u64,
        sequence: u64,
        accepted_at_ms: i64,
    ) -> Self {
        Self {
            deployer: deploy.pk.bytes.to_vec(),
            payload_bytes,
            valid_after: deploy.data.valid_after_block_number,
            sequence,
            accepted_at_ms,
            state: QueueState::Queued,
            submitted_at_height: None,
            last_sent_height: None,
            submissions: 0,
            rejection_count: 0,
            latest_block_hash: None,
            decided_at_height: None,
            detail: None,
        }
    }

    pub fn view(&self) -> DeployView {
        DeployView {
            state: self.state,
            rejection_count: self.rejection_count,
            latest_block_hash: self.latest_block_hash.clone(),
            detail: self.detail.clone(),
        }
    }
}

/// The signed deploy as the queue stores it: protobuf-encoded.
pub fn encode_deploy(deploy: &Signed<DeployData>) -> Vec<u8> {
    DeployData::to_proto_ref(deploy).encode_to_vec()
}

/// A stored deploy. Its signature was verified at intake, so it is not
/// verified again.
pub fn decode_deploy(bytes: &[u8]) -> Result<Signed<DeployData>, String> {
    let proto = DeployDataProto::decode(bytes)
        .map_err(|e| format!("stored deploy does not decode: {}", e))?;
    DeployData::from_proto_unverified(proto)
}

/// What a client is told about a deploy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployView {
    pub state: QueueState,
    pub rejection_count: u32,
    pub latest_block_hash: Option<Vec<u8>>,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;

    use super::*;

    #[test]
    fn a_stored_deploy_decodes_to_the_deploy_and_its_record_keeps_its_keys() {
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp: 3,
                phlo_price: 1.into(),
                phlo_limit: 1000,
                valid_after_block_number: 9,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                parameters: Vec::new(),
                cosignatures: Vec::new(),
                required_cosigners: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[2; 32]),
        )
        .unwrap();
        assert_eq!(decode_deploy(&encode_deploy(&deploy)).unwrap(), deploy);
        let record = QueueRecord::new(&deploy, 42, 1, 0);
        assert_eq!(record.payload_bytes, 42);
        assert_eq!(record.valid_after, 9);
        assert_eq!(record.deployer, deploy.pk.bytes.to_vec());
    }

    #[test]
    fn every_state_has_a_node_status_label() {
        for (state, label) in [
            (QueueState::Queued, "Pending"),
            (QueueState::Submitted, "Pending"),
            (QueueState::InBlock, "Pending"),
            (QueueState::Finalized, "Finalized"),
            (QueueState::Failed, "Failed"),
            (QueueState::Rejected, "Rejected"),
            (QueueState::Expired, "Expired"),
        ] {
            assert_eq!(state.status_label(), label);
            assert_eq!(state.is_terminal(), label != "Pending");
        }
    }
}
