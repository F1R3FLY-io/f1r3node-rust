// A judged block's hash announce must not stall on a slow peer: one
// unreachable peer otherwise taxes every validated block by the full send
// timeout (the slow-peer finality-freeze mechanism).

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use casper::rust::blocks::ingest::validation_worker::{BlockAnnouncer, ANNOUNCE_MAX_IN_FLIGHT};
use comm::rust::errors::CommError;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::create_rp_conf_ask;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use models::routing::Protocol;
use models::rust::block_implicits::get_random_block_default;

use crate::engine::setup;

const PARK: Duration = Duration::from_secs(5);

/// Transport whose send to one peer parks for PARK, mirroring a dead peer
/// holding its leg of the announce broadcast until the send timeout.
struct ParkedTransport {
    parked: PeerNode,
    parked_sends: AtomicUsize,
    healthy_sends: AtomicUsize,
}

impl ParkedTransport {
    fn new(parked: PeerNode) -> Self {
        Self {
            parked,
            parked_sends: AtomicUsize::new(0),
            healthy_sends: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl TransportLayer for ParkedTransport {
    async fn send(&self, peer: &PeerNode, _msg: &Protocol) -> Result<(), CommError> {
        if *peer == self.parked {
            tokio::time::sleep(PARK).await;
            self.parked_sends.fetch_add(1, Ordering::SeqCst);
        } else {
            self.healthy_sends.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn broadcast(&self, peers: &[PeerNode], msg: &Protocol) -> Result<(), CommError> {
        let sends: Vec<_> = peers.iter().map(|peer| self.send(peer, msg)).collect();
        for result in futures::future::join_all(sends).await {
            result?;
        }
        Ok(())
    }

    async fn stream(&self, _peer: &PeerNode, _blob: &Blob) -> Result<(), CommError> { Ok(()) }

    async fn stream_mult(&self, _peers: &[PeerNode], _blob: &Blob) -> Result<(), CommError> {
        Ok(())
    }

    async fn disconnect(&self, _peer: &PeerNode) -> Result<(), CommError> { Ok(()) }

    async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
        Ok(HashSet::new())
    }
}

#[tokio::test]
async fn a_dead_peer_does_not_stall_the_announce() {
    let local_peer = setup::peer_node("announce-local", 40400);
    let healthy_peer = setup::peer_node("announce-healthy", 40401);
    let parked_peer = setup::peer_node("announce-parked", 40402);

    let transport = Arc::new(ParkedTransport::new(parked_peer.clone()));
    let connections_cell = ConnectionsCell {
        peers: Arc::new(Mutex::new(Connections::from_vec(vec![
            healthy_peer,
            parked_peer,
        ]))),
    };
    let announcer = BlockAnnouncer::new(
        transport.clone(),
        connections_cell,
        create_rp_conf_ask(local_peer, None, None),
    );

    let start = Instant::now();
    announcer.announce(&get_random_block_default());
    let elapsed = start.elapsed();

    assert!(
        elapsed < PARK / 2,
        "the announce stalled {elapsed:?} on the parked peer's leg"
    );

    // The announce must still reach every peer — decoupling, not dropping.
    let deadline = Instant::now() + PARK * 2;
    while Instant::now() < deadline {
        if transport.parked_sends.load(Ordering::SeqCst) >= 1
            && transport.healthy_sends.load(Ordering::SeqCst) >= 1
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "announce never reached both peers (healthy {}, parked {})",
        transport.healthy_sends.load(Ordering::SeqCst),
        transport.parked_sends.load(Ordering::SeqCst)
    );
}

/// Transport whose every send parks indefinitely, counting the sends that
/// STARTED: the observable for how many announce tasks are actually in
/// flight at once.
struct SaturatedTransport {
    started: AtomicUsize,
}

#[async_trait]
impl TransportLayer for SaturatedTransport {
    async fn send(&self, _peer: &PeerNode, _msg: &Protocol) -> Result<(), CommError> {
        self.started.fetch_add(1, Ordering::SeqCst);
        futures::future::pending::<()>().await;
        Ok(())
    }

    async fn broadcast(&self, peers: &[PeerNode], msg: &Protocol) -> Result<(), CommError> {
        let sends: Vec<_> = peers.iter().map(|peer| self.send(peer, msg)).collect();
        for result in futures::future::join_all(sends).await {
            result?;
        }
        Ok(())
    }

    async fn stream(&self, _peer: &PeerNode, _blob: &Blob) -> Result<(), CommError> { Ok(()) }

    async fn stream_mult(&self, _peers: &[PeerNode], _blob: &Blob) -> Result<(), CommError> {
        Ok(())
    }

    async fn disconnect(&self, _peer: &PeerNode) -> Result<(), CommError> { Ok(()) }

    async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
        Ok(HashSet::new())
    }
}

/// During catch-up behind slow peers, detached announces must stay bounded:
/// without a cap, the in-flight task count is the validation rate times the
/// slowest peer's send timeout.
#[tokio::test]
async fn saturated_peers_bound_the_detached_announces() {
    let local_peer = setup::peer_node("announce-bound-local", 40410);
    let slow_peer = setup::peer_node("announce-bound-slow", 40411);

    let transport = Arc::new(SaturatedTransport {
        started: AtomicUsize::new(0),
    });
    let announcer = BlockAnnouncer::new(
        transport.clone(),
        ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![slow_peer]))),
        },
        create_rp_conf_ask(local_peer, None, None),
    );

    let overflow = 16;
    for _ in 0..(ANNOUNCE_MAX_IN_FLIGHT + overflow) {
        announcer.announce(&get_random_block_default());
    }

    // Let the spawned tasks reach their (parked) sends.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && transport.started.load(Ordering::SeqCst) < ANNOUNCE_MAX_IN_FLIGHT
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(
        transport.started.load(Ordering::SeqCst),
        ANNOUNCE_MAX_IN_FLIGHT,
        "announces past the in-flight ceiling must be dropped, not spawned"
    );
}
