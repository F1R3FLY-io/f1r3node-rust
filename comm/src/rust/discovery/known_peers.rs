use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::kademlia_rpc::KademliaRPC;
use super::kademlia_store::KademliaStore;
use crate::rust::errors::CommError;
use crate::rust::peer_node::{NodeIdentifier, PeerNode};

pub struct KnownPeers {
    path: PathBuf,
    peers: Vec<PeerNode>,
}

impl KnownPeers {
    pub fn load(path: PathBuf) -> Self {
        let peers = match std::fs::read_to_string(&path) {
            Ok(content) => parse(&path, &content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                warn!(
                    "Failed to read known peers from {}: {}",
                    path.display(),
                    error
                );
                Vec::new()
            }
        };
        Self { path, peers }
    }

    pub fn peers(&self) -> &[PeerNode] { &self.peers }

    pub fn save(&mut self, peers: Vec<PeerNode>) -> Result<bool, CommError> {
        if peers.is_empty() || addresses(&peers) == addresses(&self.peers) {
            return Ok(false);
        }
        let content: String = addresses(&peers)
            .into_iter()
            .map(|address| address + "\n")
            .collect();
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, content)
            .and_then(|()| std::fs::rename(&tmp, &self.path))
            .map_err(|error| {
                CommError::InternalCommunicationError(format!(
                    "Failed to save known peers to {}: {}",
                    self.path.display(),
                    error
                ))
            })?;
        self.peers = peers;
        Ok(true)
    }

    pub async fn seed<T: KademliaRPC>(
        &self,
        store: &KademliaStore<T>,
        local: &NodeIdentifier,
    ) -> Result<usize, CommError> {
        let mut seeded = 0;
        for peer in self.peers.iter().filter(|peer| &peer.id != local) {
            store.update_last_seen(peer).await?;
            seeded += 1;
        }
        Ok(seeded)
    }
}

fn addresses(peers: &[PeerNode]) -> BTreeSet<String> {
    peers.iter().map(PeerNode::to_address).collect()
}

fn parse(path: &Path, content: &str) -> Vec<PeerNode> {
    let mut peers: Vec<PeerNode> = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match PeerNode::from_address(line) {
            Ok(peer) if !peers.contains(&peer) => peers.push(peer),
            Ok(_) => {}
            Err(error) => warn!(
                "Skipping malformed known peer {:?} in {}: {}",
                line,
                path.display(),
                error
            ),
        }
    }
    peers
}
