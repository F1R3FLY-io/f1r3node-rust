use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use comm::rust::discovery::kademlia_handle_rpc::{handle_lookup, handle_ping};
use comm::rust::discovery::kademlia_node_discovery::KademliaNodeDiscovery;
use comm::rust::discovery::kademlia_rpc::KademliaRPC;
use comm::rust::discovery::kademlia_store::KademliaStore;
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use prost::bytes::Bytes;

struct LookupRpc {
    candidate: Mutex<PeerNode>,
}

#[async_trait::async_trait]
impl KademliaRPC for LookupRpc {
    async fn ping(&self, _peer: &PeerNode) -> Result<bool, CommError> { Ok(true) }

    async fn lookup(&self, _key: &[u8], _peer: &PeerNode) -> Result<Vec<PeerNode>, CommError> {
        Ok(vec![self.candidate.lock().unwrap().clone()])
    }
}

/// Answers this node's pings as `reachable` says, counting them.
struct ContactRpc {
    reachable: bool,
    pings: AtomicUsize,
}

impl ContactRpc {
    fn new(reachable: bool) -> Arc<Self> {
        Arc::new(Self {
            reachable,
            pings: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl KademliaRPC for ContactRpc {
    async fn ping(&self, _peer: &PeerNode) -> Result<bool, CommError> {
        self.pings.fetch_add(1, Ordering::SeqCst);
        Ok(self.reachable)
    }

    async fn lookup(&self, _key: &[u8], _peer: &PeerNode) -> Result<Vec<PeerNode>, CommError> {
        Ok(Vec::new())
    }
}

fn peer(id: u8, host: &str) -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: Bytes::from(vec![id]),
        },
        endpoint: Endpoint::new(host.to_string(), 40400, 40404),
    }
}

#[tokio::test]
async fn removed_peer_is_not_immediately_relearned_from_gossip() {
    let local = peer(1, "local");
    let live = peer(2, "live");
    let dead = peer(3, "dead");
    let rpc = Arc::new(LookupRpc {
        candidate: Mutex::new(dead.clone()),
    });
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));
    store.update_last_seen(&live).await.unwrap();
    store.update_last_seen(&dead).await.unwrap();
    let discovery = KademliaNodeDiscovery::new(store.clone(), rpc.clone(), local.id);

    discovery.evict_unreachable_peer(&dead).unwrap();
    discovery.discover().await.unwrap();
    assert_eq!(store.peers().unwrap(), vec![live.clone()]);

    let changed = peer(3, "new-address");
    *rpc.candidate.lock().unwrap() = changed.clone();
    discovery.discover().await.unwrap();
    assert!(store.peers().unwrap().contains(&changed));
    assert_eq!(
        store.find(&changed.id.key).unwrap().unwrap().endpoint,
        changed.endpoint
    );
}

#[tokio::test]
async fn normal_removal_does_not_delay_rediscovery() {
    let local = peer(1, "local");
    let live = peer(2, "live");
    let dead = peer(3, "dead");
    let rpc = Arc::new(LookupRpc {
        candidate: Mutex::new(dead.clone()),
    });
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));
    store.update_last_seen(&live).await.unwrap();
    store.update_last_seen(&dead).await.unwrap();
    let discovery = KademliaNodeDiscovery::new(store.clone(), rpc, local.id);

    discovery.remove_peer(&dead).unwrap();
    discovery.discover().await.unwrap();
    assert!(store.peers().unwrap().contains(&dead));
}

#[tokio::test]
async fn inbound_rpc_from_an_address_this_node_cannot_reach_does_not_restore_it() {
    let local = peer(1, "local");
    let dead = peer(2, "dead");
    let rpc = ContactRpc::new(false);
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));
    store.update_last_seen(&dead).await.unwrap();
    let discovery = KademliaNodeDiscovery::new(store.clone(), rpc, local.id.clone());

    discovery.evict_unreachable_peer(&dead).unwrap();
    handle_ping(dead.clone(), store.clone(), None)
        .await
        .unwrap();
    handle_lookup(dead.clone(), local.id.key.to_vec(), store.clone(), None)
        .await
        .unwrap();
    assert!(store.find(&dead.id.key).unwrap().is_none());

    let changed = peer(2, "new-address");
    handle_ping(changed.clone(), store.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        store.find(&changed.id.key).unwrap().unwrap().endpoint,
        changed.endpoint
    );
}

#[tokio::test]
async fn an_evicted_peer_reachable_again_is_restored_by_its_own_contact() {
    let local = peer(1, "local");
    let returning = peer(2, "returning");
    let rpc = ContactRpc::new(true);
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));

    store.evict_unreachable_peer(&returning).unwrap();
    handle_ping(returning.clone(), store.clone(), None)
        .await
        .unwrap();
    assert!(store.find(&returning.id.key).unwrap().is_some());

    store.evict_unreachable_peer(&returning).unwrap();
    handle_lookup(
        returning.clone(),
        local.id.key.to_vec(),
        store.clone(),
        None,
    )
    .await
    .unwrap();
    assert!(store.find(&returning.id.key).unwrap().is_some());
}

#[tokio::test(start_paused = true)]
async fn a_suppressed_address_is_probed_at_most_once_per_interval() {
    let local = peer(1, "local");
    let dead = peer(2, "dead");
    let rpc = ContactRpc::new(false);
    let store = Arc::new(KademliaStore::new(local.id.clone(), rpc.clone()));

    store.evict_unreachable_peer(&dead).unwrap();
    for _ in 0..3 {
        handle_ping(dead.clone(), store.clone(), None)
            .await
            .unwrap();
    }
    assert_eq!(rpc.pings.load(Ordering::SeqCst), 1);

    tokio::time::advance(Duration::from_secs(20)).await;
    handle_ping(dead.clone(), store.clone(), None)
        .await
        .unwrap();
    assert_eq!(rpc.pings.load(Ordering::SeqCst), 2);
    assert!(store.find(&dead.id.key).unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn expired_delay_allows_a_peer_to_be_admitted_again() {
    let local = peer(1, "local");
    let dead = peer(2, "dead");
    let rpc = Arc::new(LookupRpc {
        candidate: Mutex::new(dead.clone()),
    });
    let store = KademliaStore::new(local.id, rpc);

    store.evict_unreachable_peer(&dead).unwrap();
    assert!(store.find(&dead.id.key).unwrap().is_none());

    tokio::time::advance(Duration::from_secs(301)).await;
    store.update_last_seen(&dead).await.unwrap();
    assert!(store.find(&dead.id.key).unwrap().is_some());
}
