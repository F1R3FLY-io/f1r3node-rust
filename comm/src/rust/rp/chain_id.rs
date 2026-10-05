use std::sync::{Arc, OnceLock};

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

/// What a peer is trying to do with a message, derived from its packet type.
///
/// A node that has not yet learned its own genesis can only ever *ask* for
/// data, so every message a joining node sends is a `Pull`. The abuse this
/// guards against is the opposite direction: an unsolicited `Push` of chain
/// data from a node that holds the state of a previous chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    /// A request. The responder bounds the cost, so an unidentified peer may
    /// send one: this is what lets a new node bootstrap and an observer sync.
    Pull,
    /// Chain data the sender pushes at us, or a response to something we
    /// asked. Accepting one from an unidentified peer is the resource
    /// exhaustion this module exists to prevent.
    Push,
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
        Some(
            "ApprovedBlockRequest"
            | "BlockRequest"
            | "HasBlockRequest"
            | "ForkChoiceTipRequest"
            | "StoreItemsMessageRequest"
            | "MergeableEntryRequest"
            | "FloorCacheRequest",
        ) => MessageClass::Pull,
        // Default deny: an unrecognized type counts as a push, so a message
        // type added later is guarded before anyone remembers this list.
        Some(_) => MessageClass::Push,
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
    /// 3. The peer names no chain. Accept a `Pull` always, so joining nodes,
    ///    observers and older builds keep working. Accept a `Push` only while
    ///    `require_chain_id` is off, which is the mixed-version window.
    pub fn check(
        &self,
        peer: &str,
        peer_chain_id: &[u8],
        type_id: Option<&str>,
        require_chain_id: bool,
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
            MessageClass::Push if !require_chain_id => Ok(()),
            MessageClass::Push => Err(wrong_chain_id(
                peer.to_string(),
                format!(
                    "peer named no chain and `require-chain-id` is set, so its {} was refused",
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
    /// `sender` must be an identity the transport has bound to the peer's TLS
    /// certificate, as the unary path does. A chunk header's sender is bound
    /// to nothing, so a stream must never call this: any peer could claim to
    /// be the bootstrap.
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
