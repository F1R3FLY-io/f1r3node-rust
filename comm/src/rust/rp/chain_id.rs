use std::sync::{Arc, OnceLock};

use models::rust::casper::protocol::packet_type_tag::packet_message_class;
pub use models::rust::casper::protocol::packet_type_tag::MessageClass;
use prost::bytes::Bytes;

use crate::rust::errors::{wrong_chain_id, CommError};
use crate::rust::peer_node::PeerNode;

/// How much of a chain identity to show in a log line or an error message.
const DISPLAY_BYTES: usize = 8;

fn short_id(id: &[u8]) -> String {
    if id.is_empty() {
        return "<unknown>".to_string();
    }
    let shown = &id[..id.len().min(DISPLAY_BYTES)];
    if shown.len() == id.len() {
        hex::encode(shown)
    } else {
        format!("{}...", hex::encode(shown))
    }
}

/// Classify a message by its packet type id.
///
/// `None` is a control-plane message with no packet payload (handshake,
/// heartbeat, disconnect). Those are always `Pull`: the handshake is how two
/// nodes learn each other's chain identity, so refusing it would make the
/// identity unknowable.
pub fn classify(type_id: Option<&str>) -> MessageClass {
    match type_id {
        None => MessageClass::Pull,
        Some(type_id) => packet_message_class(type_id),
    }
}

/// This node's chain identity: the hash of its shard's genesis block.
///
/// `networkId` cannot express chain identity. It is a configuration string,
/// and relaunching a network deliberately reuses it, so a node that kept the
/// state of the previous chain passes a `networkId` check and can then push
/// blocks of a chain that no longer exists. The genesis hash differs between
/// the two launches and so separates them.
///
/// The value is learned at different times depending on how the node starts:
/// a node restarted on existing data has it immediately, a node that joins
/// learns it during bootstrap. The cell is therefore shared and filled in
/// late. Every `RPConf` clone holds the same `Arc`, so a value published after
/// the transport was built is visible to the transport.
#[derive(Clone, Debug, Default)]
pub struct ChainIdCell {
    inner: Arc<OnceLock<Bytes>>,
}

impl ChainIdCell {
    /// An empty cell, for a node that does not know its genesis yet.
    pub fn unknown() -> Self { Self::default() }

    /// A cell already holding `genesis`.
    pub fn known(genesis: Bytes) -> Self {
        let cell = Self::unknown();
        let _ = cell.inner.set(genesis);
        cell
    }

    /// This node's chain identity, or `None` while it is still unknown.
    pub fn get(&self) -> Option<&Bytes> { self.inner.get() }

    /// The value to put on the wire: the identity, or empty when unknown.
    pub fn to_wire(&self) -> Bytes { self.inner.get().cloned().unwrap_or_default() }

    /// Publish this node's genesis hash.
    ///
    /// Write-once. Publishing the same value again is a no-op; publishing a
    /// different one is an error, because two chain identities on one node is
    /// a bootstrap-integrity violation and never something to paper over.
    /// This is what verifies a value adopted from the bootstrap peer against
    /// the genesis the node eventually records for itself.
    pub fn set_once(&self, genesis: Bytes) -> Result<(), CommError> {
        if let Err(rejected) = self.inner.set(genesis) {
            let held = self.inner.get().expect("set failed, so the cell is filled");
            if *held != rejected {
                return Err(CommError::WrongChainId(
                    "local".to_string(),
                    format!(
                        "chain identity already established as {}; refusing to replace it with {}",
                        short_id(held),
                        short_id(&rejected),
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Decide whether a peer's message may be processed.
    ///
    /// The rules, in order:
    /// 1. This node does not know its own identity yet, so it cannot judge
    ///    anyone. Accept.
    /// 2. The peer names a different chain. Reject, whatever the message is.
    ///    This is the relaunch case: a node of the previous chain names the
    ///    previous genesis.
    /// 3. The peer names no chain. Accept a `Pull`, so a joining node, which
    ///    does not know its genesis yet, can still ask for data. Reject a
    ///    `Push`: a node that holds chain data knows its chain, so one that
    ///    pushes chain data without naming it is either broken or hostile.
    pub fn check(
        &self,
        peer: &str,
        peer_chain_id: &[u8],
        type_id: Option<&str>,
    ) -> Result<(), CommError> {
        let Some(local) = self.inner.get() else {
            return Ok(());
        };

        if !peer_chain_id.is_empty() {
            return if peer_chain_id == local.as_ref() {
                Ok(())
            } else {
                Err(wrong_chain_id(
                    peer.to_string(),
                    format!(
                        "peer is on chain {}, this node is on chain {}",
                        short_id(peer_chain_id),
                        short_id(local),
                    ),
                ))
            };
        }

        match classify(type_id) {
            MessageClass::Pull => Ok(()),
            MessageClass::Push => Err(wrong_chain_id(
                peer.to_string(),
                format!(
                    "peer named no chain, so its {} was refused",
                    type_id.unwrap_or("control message"),
                ),
            )),
        }
    }

    /// Adopt a chain identity from the configured bootstrap peer.
    ///
    /// A joining node cannot verify a chain identity before it holds an
    /// approved block, so trusting one from an arbitrary peer would let an
    /// attacker pin the node onto the wrong chain. The bootstrap address is
    /// already this node's trust root for joining, so only that peer is
    /// believed, and only while this node knows nothing.
    ///
    /// `sender` must be an identity that the transport bound to the peer's TLS
    /// certificate. Only the unary path calls this. A stream sender is bound
    /// too (`verify_stream_sender`), but adoption does not need streams: the
    /// bootstrap's handshake response and heartbeats are unary.
    ///
    /// The adopted value is not final: `set_once` later compares it against
    /// the genesis the node records for itself, and the mismatch fails loudly.
    ///
    /// Returns `true` when a value was adopted.
    pub fn adopt_from_bootstrap(
        &self,
        sender: &PeerNode,
        bootstrap: Option<&PeerNode>,
        peer_chain_id: &[u8],
    ) -> bool {
        if peer_chain_id.is_empty() || self.inner.get().is_some() {
            return false;
        }
        let Some(bootstrap) = bootstrap else {
            return false;
        };
        if bootstrap.id.key != sender.id.key {
            return false;
        }
        if self
            .inner
            .set(Bytes::copy_from_slice(peer_chain_id))
            .is_err()
        {
            return false;
        }
        tracing::info!(
            chain_id = %short_id(peer_chain_id),
            bootstrap = %sender,
            "Adopted chain identity from the configured bootstrap peer; it is \
             verified against this node's own genesis once that is recorded"
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use models::casper::*;
    use models::rust::casper::protocol::packet_type_tag::ToPacket;

    use super::*;

    #[test]
    fn joining_nodes_can_send_every_declared_request() {
        let chain = ChainIdCell::known(Bytes::from_static(b"our-chain"));
        let packets = [
            ApprovedBlockRequestProto::default().mk_packet(),
            BlockRequestProto::default().mk_packet(),
            HasBlockRequestProto::default().mk_packet(),
            ForkChoiceTipRequestProto::default().mk_packet(),
            StoreItemsMessageRequestProto::default().mk_packet(),
            FloorCacheRequestProto::default().mk_packet(),
            GetSnapshotChunkRequestProto::default().mk_packet(),
            HasSnapshotRequestProto::default().mk_packet(),
            GetWalPayloadRequestProto::default().mk_packet(),
            HasWalPayloadRequestProto::default().mk_packet(),
        ];

        assert!(chain.check("peer", b"", None).is_ok());
        for packet in packets {
            assert!(
                chain.check("peer", b"", Some(&packet.type_id)).is_ok(),
                "{} must be accepted from a joining node",
                packet.type_id,
            );
            assert!(matches!(
                chain.check("peer", b"old-chain", Some(&packet.type_id)),
                Err(CommError::WrongChainId(..))
            ));
        }
    }

    #[test]
    fn joining_nodes_cannot_push_data_or_unknown_packets() {
        let chain = ChainIdCell::known(Bytes::from_static(b"our-chain"));
        let packets = [
            BlockMessageProto::default().mk_packet(),
            BlockHashMessageProto::default().mk_packet(),
            ApprovedBlockProto::default().mk_packet(),
            UnapprovedBlockProto::default().mk_packet(),
            BlockApprovalProto::default().mk_packet(),
            NoApprovedBlockAvailableProto::default().mk_packet(),
            HasBlockProto::default().mk_packet(),
            StoreItemsMessageProto::default().mk_packet(),
            FloorCacheResponseProto::default().mk_packet(),
            SnapshotChunkResponseProto::default().mk_packet(),
            HasSnapshotProto::default().mk_packet(),
            WalPayloadResponseProto::default().mk_packet(),
            HasWalPayloadProto::default().mk_packet(),
            models::routing::Packet {
                type_id: "UnknownPacket".to_string(),
                content: Bytes::new(),
            },
        ];

        for packet in packets {
            assert!(
                matches!(
                    chain.check("peer", b"", Some(&packet.type_id)),
                    Err(CommError::WrongChainId(..))
                ),
                "{} must be refused from a joining node",
                packet.type_id,
            );
            assert!(chain
                .check("peer", b"our-chain", Some(&packet.type_id))
                .is_ok());
        }
    }
}
