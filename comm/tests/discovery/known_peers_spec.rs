use std::sync::Arc;

use comm::rust::discovery::kademlia_node_discovery::KademliaNodeDiscovery;
use comm::rust::discovery::kademlia_rpc::KademliaRPC;
use comm::rust::discovery::kademlia_store::KademliaStore;
use comm::rust::discovery::known_peers::KnownPeers;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{find_and_connect, ConnectFailureTracker, ConnectionsCell};
use prost::bytes::Bytes;

struct SilentRpc;

#[async_trait::async_trait]
impl KademliaRPC for SilentRpc {
    async fn ping(&self, _peer: &PeerNode) -> Result<bool, CommError> { Ok(true) }

    async fn lookup(&self, _key: &[u8], _peer: &PeerNode) -> Result<Vec<PeerNode>, CommError> {
        Ok(Vec::new())
    }
}

fn peer(id: u8, host: &str) -> PeerNode {
    PeerNode::new(
        NodeIdentifier {
            key: Bytes::from(vec![id; 20]),
        },
        host.to_string(),
        40400,
        40404,
    )
}

#[tokio::test]
async fn restarted_node_reconnects_through_saved_peers_when_bootstrap_is_down() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known-peers");
    let local = peer(1, "validator3");
    let bootstrap = peer(2, "bootstrap");
    let validator1 = peer(3, "validator1");
    let validator4 = peer(4, "validator4");

    let mut before_restart = KnownPeers::load(path.clone());
    assert!(before_restart
        .save(vec![
            bootstrap.clone(),
            validator1.clone(),
            validator4.clone()
        ])
        .unwrap());

    let rpc = Arc::new(SilentRpc);
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));
    store.update_last_seen(&bootstrap).await.unwrap();
    let after_restart = KnownPeers::load(path);
    after_restart.seed(&store, &local.id).await.unwrap();
    let discovery = KademliaNodeDiscovery::new(store, rpc, local.id.clone());

    let connections = ConnectionsCell::new();
    let mut failures = ConnectFailureTracker::new(3).unwrap();
    let dead_bootstrap = bootstrap.clone();
    let connected = find_and_connect(
        &connections,
        &discovery,
        |candidate: &PeerNode| {
            let result = if *candidate == dead_bootstrap {
                Err(CommError::DnsResolutionFailed(
                    candidate.endpoint.host.clone(),
                    "Peer hostname could not be resolved".to_string(),
                ))
            } else {
                Ok(())
            };
            async move { result }
        },
        Some(&bootstrap),
        &mut failures,
    )
    .await
    .unwrap();

    assert_eq!(connected.len(), 2);
    assert!(connected.contains(&validator1));
    assert!(connected.contains(&validator4));
}

#[test]
fn missing_file_gives_no_peers_and_malformed_lines_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known-peers");
    assert!(KnownPeers::load(path.clone()).peers().is_empty());

    let validator1 = peer(3, "validator1");
    let content = format!(
        "{}\n\nnot-an-address\nrnode://zz@host?protocol=1&discovery=2\n{}\n",
        validator1.to_address(),
        validator1.to_address()
    );
    std::fs::write(&path, content).unwrap();

    let loaded = KnownPeers::load(path);
    assert_eq!(loaded.peers(), std::slice::from_ref(&validator1));
    assert_eq!(loaded.peers()[0].endpoint, validator1.endpoint);
}

#[test]
fn save_keeps_the_last_good_set_when_the_node_is_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known-peers");
    let validator1 = peer(3, "validator1");
    let validator4 = peer(4, "validator4");

    let mut known = KnownPeers::load(path.clone());
    assert!(known
        .save(vec![validator1.clone(), validator4.clone()])
        .unwrap());
    assert!(!known.save(Vec::new()).unwrap());
    assert!(!known
        .save(vec![validator4.clone(), validator1.clone()])
        .unwrap());

    let reloaded = KnownPeers::load(path.clone());
    assert_eq!(reloaded.peers().len(), 2);
    assert!(reloaded.peers().contains(&validator1));
    assert!(reloaded.peers().contains(&validator4));

    assert!(known.save(vec![validator4.clone()]).unwrap());
    assert_eq!(KnownPeers::load(path).peers(), [validator4]);
}

#[tokio::test]
async fn seed_skips_the_local_node() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known-peers");
    let local = peer(1, "validator3");
    let validator1 = peer(3, "validator1");

    let mut known = KnownPeers::load(path);
    known.save(vec![local.clone(), validator1.clone()]).unwrap();

    let store = KademliaStore::new(local.id.clone(), Arc::new(SilentRpc));
    assert_eq!(known.seed(&store, &local.id).await.unwrap(), 1);
    assert_eq!(store.peers().unwrap(), vec![validator1]);
}
