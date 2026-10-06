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

pub struct KademliaStore<T: KademliaRPC> {
    table: PeerTable<T>,
    suppressed: Mutex<HashMap<Bytes, (Endpoint, Instant)>>,
}

impl<T: KademliaRPC> KademliaStore<T> {
    pub fn new(id: NodeIdentifier, kademlia_rpc: Arc<T>) -> Self {
        Self {
            table: PeerTable::new(id.key, None, None, kademlia_rpc),
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
        suppressed.retain(|_, (_, until)| *until > Instant::now());
        suppressed.insert(
            peer.id.key.clone(),
            (peer.endpoint.clone(), Instant::now() + REDISCOVERY_DELAY),
        );
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

    fn matches_suppression(
        suppressed: &mut HashMap<Bytes, (Endpoint, Instant)>,
        peer: &PeerNode,
    ) -> bool {
        match suppressed.get(&peer.id.key) {
            Some((endpoint, until)) if endpoint == &peer.endpoint && *until > Instant::now() => {
                true
            }
            Some(_) => {
                suppressed.remove(&peer.id.key);
                false
            }
            None => false,
        }
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
