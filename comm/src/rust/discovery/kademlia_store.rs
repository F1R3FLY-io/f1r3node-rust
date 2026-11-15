// See comm/src/main/scala/coop/rchain/comm/discovery/KademliaStore.scala

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use prost::bytes::Bytes;
use tokio::time::Instant;

use super::kademlia_rpc::KademliaRPC;
use super::peer_table::PeerTable;
use crate::rust::errors::CommError;
use crate::rust::metrics_constants::{DISCOVERY_METRICS_SOURCE, PEERS_METRIC};
use crate::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};

const REDISCOVERY_DELAY: Duration = Duration::from_secs(300);
const REPROBE_INTERVAL: Duration = Duration::from_secs(20);

struct Suppression {
    endpoint: Endpoint,
    until: Instant,
    probed_at: Option<Instant>,
}

pub struct KademliaStore<T: KademliaRPC> {
    table: PeerTable<T>,
    rpc: Arc<T>,
    suppressed: Mutex<HashMap<Bytes, Suppression>>,
}

impl<T: KademliaRPC> KademliaStore<T> {
    pub fn new(id: NodeIdentifier, kademlia_rpc: Arc<T>) -> Self {
        Self {
            table: PeerTable::new(id.key, None, None, kademlia_rpc.clone()),
            rpc: kademlia_rpc,
            suppressed: Mutex::new(HashMap::new()),
        }
    }

    pub fn peers(&self) -> Result<Vec<PeerNode>, CommError> {
        let peers = self.table.peers()?;
        metrics::gauge!(PEERS_METRIC, "source" => DISCOVERY_METRICS_SOURCE).set(peers.len() as f64);
        Ok(peers)
    }

    pub fn sparseness(&self) -> Result<Vec<usize>, CommError> { self.table.sparseness() }

    pub fn lookup(&self, key: &Bytes) -> Result<Vec<PeerNode>, CommError> { self.table.lookup(key) }

    pub fn find(&self, key: &Bytes) -> Result<Option<PeerNode>, CommError> { self.table.find(key) }

    pub fn remove(&self, key: &Bytes) -> Result<(), CommError> {
        self.table.remove(key)?;
        let peers = self.peers()?;
        metrics::gauge!(PEERS_METRIC, "source" => DISCOVERY_METRICS_SOURCE).set(peers.len() as f64);
        Ok(())
    }

    pub fn evict_unreachable_peer(&self, peer: &PeerNode) -> Result<(), CommError> {
        let mut suppressed = self.suppressed.lock().map_err(|_| {
            CommError::InternalCommunicationError(
                "Failed to acquire suppressed peer lock".to_string(),
            )
        })?;
        suppressed.retain(|_, suppression| suppression.until > Instant::now());
        suppressed.insert(peer.id.key.clone(), Suppression {
            endpoint: peer.endpoint.clone(),
            until: Instant::now() + REDISCOVERY_DELAY,
            probed_at: None,
        });
        self.table.remove(&peer.id.key)?;
        drop(suppressed);
        self.peers()?;
        Ok(())
    }

    pub fn is_suppressed(&self, peer: &PeerNode) -> Result<bool, CommError> {
        let mut suppressed = self.suppressed.lock().map_err(|_| {
            CommError::InternalCommunicationError(
                "Failed to acquire suppressed peer lock".to_string(),
            )
        })?;
        Ok(Self::matches_suppression(&mut suppressed, peer))
    }

    fn matches_suppression(suppressed: &mut HashMap<Bytes, Suppression>, peer: &PeerNode) -> bool {
        match suppressed.get(&peer.id.key) {
            Some(suppression)
                if suppression.endpoint == peer.endpoint && suppression.until > Instant::now() =>
            {
                true
            }
            Some(_) => {
                suppressed.remove(&peer.id.key);
                false
            }
            None => false,
        }
    }

    /// Records an RPC the peer itself sent. An address suppressed after
    /// eviction is admitted again only if this node can reach it: contact
    /// proves the peer reaches this node, not the reverse. Each suppressed
    /// address is probed at most once per `REPROBE_INTERVAL`, in the
    /// background, so the peer's own RPC is answered without waiting on a
    /// ping that may run to its timeout.
    pub async fn record_contact(self: &Arc<Self>, peer_node: &PeerNode) -> Result<(), CommError>
    where T: Send + Sync + 'static {
        if self.claim_probe(peer_node)? {
            let store = Arc::clone(self);
            let peer = peer_node.clone();
            tokio::spawn(async move {
                if !matches!(store.rpc.ping(&peer).await, Ok(true)) {
                    return;
                }
                let admitted = match store.lift_suppression(&peer) {
                    Ok(()) => store.update_last_seen(&peer).await,
                    Err(error) => Err(error),
                };
                if let Err(error) = admitted {
                    tracing::warn!(
                        target: "f1r3fly.comm.discovery",
                        peer = %peer.to_address(),
                        %error,
                        "a returning peer answered the reachability probe but could not be readmitted"
                    );
                }
            });
        }
        self.update_last_seen(peer_node).await
    }

    fn claim_probe(&self, peer: &PeerNode) -> Result<bool, CommError> {
        let mut suppressed = self.suppressed.lock().map_err(|_| {
            CommError::InternalCommunicationError(
                "Failed to acquire suppressed peer lock".to_string(),
            )
        })?;
        if !Self::matches_suppression(&mut suppressed, peer) {
            return Ok(false);
        }
        let suppression = suppressed
            .get_mut(&peer.id.key)
            .expect("matches_suppression keeps the entry it matched");
        let now = Instant::now();
        if suppression
            .probed_at
            .is_some_and(|probed| now.duration_since(probed) < REPROBE_INTERVAL)
        {
            return Ok(false);
        }
        suppression.probed_at = Some(now);
        Ok(true)
    }

    fn lift_suppression(&self, peer: &PeerNode) -> Result<(), CommError> {
        let mut suppressed = self.suppressed.lock().map_err(|_| {
            CommError::InternalCommunicationError(
                "Failed to acquire suppressed peer lock".to_string(),
            )
        })?;
        if Self::matches_suppression(&mut suppressed, peer) {
            suppressed.remove(&peer.id.key);
        }
        Ok(())
    }

    pub async fn update_last_seen(&self, peer_node: &PeerNode) -> Result<(), CommError> {
        if self.is_suppressed(peer_node)? {
            return Ok(());
        }
        self.table.update_last_seen(peer_node).await?;
        {
            let mut suppressed = self.suppressed.lock().map_err(|_| {
                CommError::InternalCommunicationError(
                    "Failed to acquire suppressed peer lock".to_string(),
                )
            })?;
            if Self::matches_suppression(&mut suppressed, peer_node) {
                self.table.remove(&peer_node.id.key)?;
            }
        }
        self.peers()?;
        Ok(())
    }
}
