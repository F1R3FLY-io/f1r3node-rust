use std::collections::BTreeSet;
use std::io::Write;
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
        write_durably(&self.path, &content).map_err(|error| {
            CommError::InternalCommunicationError(format!(
                "Failed to save known peers to {}: {}",
                self.path.display(),
                error
            ))
        })?;
        self.peers = peers;
        Ok(true)
    }

    /// Adds the saved peers, except the local node and suppressed peers, to `store`.
    /// Returns the number added.
    pub async fn seed<T: KademliaRPC>(
        &self,
        store: &KademliaStore<T>,
        local: &NodeIdentifier,
    ) -> Result<usize, CommError> {
        let mut seeded = 0;
        for peer in self.peers.iter().filter(|peer| &peer.id != local) {
            if store.is_suppressed(peer)? {
                continue;
            }
            store.update_last_seen(peer).await?;
            seeded += 1;
        }
        Ok(seeded)
    }
}

/// Replaces `path` with `content` so that a crash leaves either the old or the new file.
fn write_durably(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
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
