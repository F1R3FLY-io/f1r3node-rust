// See comm/src/main/scala/coop/rchain/comm/rp/Connect.scala

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use futures::future::join_all;
use prost::bytes::Bytes;
use rand::seq::SliceRandom;
use tracing::{info, warn};

use crate::rust::discovery::node_discovery::NodeDiscovery;
use crate::rust::errors::CommError;
use crate::rust::metrics_constants::{
    CONNECT_METRIC, CONNECT_TIME_METRIC, RP_CONNECT_METRICS_SOURCE,
};
use crate::rust::peer_node::PeerNode;
use crate::rust::rp::protocol_helper;
use crate::rust::rp::rp_conf::RPConf;
use crate::rust::transport::transport_layer::TransportLayer;

pub type Connection = PeerNode;

/// Outcome of recording one failed heartbeat against a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatFailure {
    /// The streak is short of the threshold, so the peer keeps its connection.
    Retained { streak: usize, threshold: usize },
    /// The streak reached the threshold; the peer is evicted and its count cleared.
    Evicted,
}

/// Consecutive heartbeat failures per peer, and the streak length that evicts.
///
/// A peer survives until it fails `threshold` heartbeats in a row; any success
/// clears its count. The caller owns one tracker for the lifetime of its cleanup
/// loop — a tracker rebuilt each pass would evict on first failure.
#[derive(Debug)]
pub struct PeerLivenessTracker {
    streaks: HashMap<Bytes, usize>,
    threshold: usize,
}

impl PeerLivenessTracker {
    /// Rejects a zero threshold: it would name a streak no peer can reach, and
    /// silently clamping it hides a misconfiguration behind eviction behaviour
    /// nobody asked for.
    pub fn new(threshold: u32) -> Result<Self, CommError> {
        if threshold == 0 {
            return Err(CommError::ConfigError(
                "heartbeat failure threshold must be at least 1".to_string(),
            ));
        }

        Ok(Self {
            streaks: HashMap::new(),
            threshold: threshold as usize,
        })
    }

    pub fn threshold(&self) -> usize { self.threshold }

    /// Current consecutive-failure count for a peer; zero when it has none.
    pub fn streak(&self, peer_id: &Bytes) -> usize {
        self.streaks.get(peer_id).copied().unwrap_or(0)
    }

    pub fn record_success(&mut self, peer_id: &Bytes) { self.streaks.remove(peer_id); }

    pub fn record_failure(&mut self, peer_id: &Bytes) -> HeartbeatFailure {
        let streak = self.streaks.entry(peer_id.clone()).or_insert(0);
        *streak += 1;

        if *streak >= self.threshold {
            self.streaks.remove(peer_id);
            HeartbeatFailure::Evicted
        } else {
            HeartbeatFailure::Retained {
                streak: *streak,
                threshold: self.threshold,
            }
        }
    }

    /// Drop counts for peers that are no longer connected, bounding the map to
    /// the live connection set.
    pub fn retain_connected(&mut self, connected: &HashSet<Bytes>) {
        self.streaks
            .retain(|peer_id, _| connected.contains(peer_id));
    }
}

#[derive(Debug, Clone)]
pub struct Connections(pub Vec<Connection>);

impl Connections {
    pub fn empty() -> Self { Self(Vec::new()) }

    pub fn from_vec(connections: Vec<Connection>) -> Self { Self(connections) }

    pub fn into_vec(self) -> Vec<Connection> { self.0 }

    pub fn as_slice(&self) -> &[Connection] { &self.0 }

    pub fn len(&self) -> usize { self.0.len() }

    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    pub fn iter(&self) -> std::slice::Iter<'_, Connection> { self.0.iter() }

    pub fn take(&self, n: usize) -> Connections {
        Connections(self.0.iter().take(n).cloned().collect())
    }

    pub fn to_set(&self) -> HashSet<PeerNode> { self.0.iter().cloned().collect() }

    pub fn add_conn_and_report(&self, connection: Connection) -> Result<Connections, CommError> {
        let new_connections = self.add_conn(connection)?;
        new_connections.report_conn()
    }

    pub fn add_conn(&self, connection: Connection) -> Result<Connections, CommError> {
        self.add_conns(vec![connection])
    }

    pub fn add_conns(&self, to_be_added: Vec<Connection>) -> Result<Connections, CommError> {
        let ids_to_add: Vec<_> = to_be_added.iter().map(|peer| &peer.id.key).collect();

        // Remove any existing connections with the same IDs
        let existing_without_duplicates: Vec<Connection> = self
            .0
            .iter()
            .filter(|peer| !ids_to_add.contains(&&peer.id.key))
            .cloned()
            .collect();

        // Add the new connections
        let mut new_connections = existing_without_duplicates;
        new_connections.extend(to_be_added);

        Ok(Connections(new_connections))
    }

    pub fn remove_conn_and_report(&self, connection: Connection) -> Result<Connections, CommError> {
        let new_connections = self.remove_conn(connection)?;
        new_connections.report_conn()
    }

    pub fn remove_conn(&self, connection: Connection) -> Result<Connections, CommError> {
        self.remove_conns(vec![connection])
    }

    pub fn remove_conns(&self, to_be_removed: Vec<Connection>) -> Result<Connections, CommError> {
        let ids_to_remove: Vec<_> = to_be_removed.iter().map(|peer| &peer.id.key).collect();

        // Keep only connections whose IDs are not in the removal list
        let remaining_connections: Vec<Connection> = self
            .0
            .iter()
            .filter(|peer| !ids_to_remove.contains(&&peer.id.key))
            .cloned()
            .collect();

        Ok(Connections(remaining_connections))
    }

    pub fn refresh_conn(&self, connection: Connection) -> Result<Connections, CommError> {
        let mut new_connections: Vec<Connection> = self
            .0
            .iter()
            .filter(|peer| peer.id.key != connection.id.key)
            .cloned()
            .collect();

        // If the connection existed in the original list, add it to the end
        if self.0.iter().any(|peer| peer.id.key == connection.id.key) {
            new_connections.push(connection);
        }

        Ok(Connections(new_connections))
    }

    pub fn report_conn(&self) -> Result<Connections, CommError> {
        let size = self.0.len();
        info!("Peers: {}", size);
        metrics::gauge!("peers", "source" => RP_CONNECT_METRICS_SOURCE).set(size as f64);
        metrics::counter!(CONNECT_METRIC, "source" => RP_CONNECT_METRICS_SOURCE).increment(1);
        Ok(self.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionsCell {
    pub peers: Arc<Mutex<Connections>>,
}

impl ConnectionsCell {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(Connections::empty())),
        }
    }

    pub fn random(&self, max: usize) -> Result<Connections, CommError> {
        let peers = self.peers.lock().map_err(|_| {
            CommError::InternalCommunicationError("ConnectionsCell lock poisoned".to_string())
        })?;

        let mut rng = rand::rng();
        let mut shuffled = peers.0.clone();
        shuffled.shuffle(&mut rng);

        Ok(Connections(shuffled.into_iter().take(max).collect()))
    }

    pub fn read(&self) -> Result<Connections, CommError> {
        let peers = self.peers.lock().map_err(|_| {
            CommError::InternalCommunicationError("ConnectionsCell lock poisoned".to_string())
        })?;
        Ok(peers.clone())
    }

    pub fn flat_modify<F>(&self, f: F) -> Result<Connections, CommError>
    where F: FnOnce(Connections) -> Result<Connections, CommError> {
        let mut peers = self.peers.lock().map_err(|_| {
            CommError::InternalCommunicationError("ConnectionsCell lock poisoned".to_string())
        })?;

        let current_peers = peers.clone();
        let new_peers = f(current_peers)?;
        *peers = new_peers.clone();

        Ok(new_peers)
    }
}

/// Clear connections by sending heartbeats and removing failed peers.
///
/// Performs the full cleanup cycle matching Scala's `clearConnections`:
/// 1. Sends heartbeats to the first N peers
/// 2. Removes failed peers from ConnectionsCell
/// 3. Removes failed peers from KademliaStore (via node_discovery), EXCEPT the
///    bootstrap peer which is pinned to prevent a discovery death spiral
/// 4. Disconnects gRPC channels for ALL failed peers (including bootstrap)
///
/// The bootstrap peer is kept in the routing table so `findAndConnect` can
/// re-establish the connection on the next discovery cycle. Removing it is
/// irreversible and strands the node if no other peers are known.
///
/// A peer is removed only after `liveness` records enough consecutive failures;
/// the tracker must outlive the call for that streak to accumulate.
///
/// Returns tuple of (number of failed peers, list of failed peers).
pub async fn clear_connections<T>(
    connections_cell: &ConnectionsCell,
    conf: &RPConf,
    transport: &T,
    node_discovery: &dyn crate::rust::discovery::node_discovery::NodeDiscovery,
    liveness: &mut PeerLivenessTracker,
) -> Result<(usize, Vec<PeerNode>), CommError>
where
    T: TransportLayer + Sync,
{
    let connections = connections_cell.read()?;
    let num_to_ping = conf.clear_connections.num_of_connections_pinged;
    // Only the first `num_to_ping` peers are probed, and both successful and
    // retained peers are appended to the back below, so the list rotates. Beyond
    // that many connections a streak spans rotations rather than consecutive
    // cleanup intervals.
    let to_ping = connections.take(num_to_ping);
    let connected_ids: HashSet<Bytes> =
        connections.iter().map(|peer| peer.id.key.clone()).collect();
    liveness.retain_connected(&connected_ids);

    let results = join_all(to_ping.iter().cloned().map(|peer| {
        let heartbeat_msg = protocol_helper::heartbeat(&conf.local, &conf.network_id);
        async move {
            let result = transport.send(&peer, &heartbeat_msg).await;
            (peer, result)
        }
    }))
    .await;

    let mut retained_peers = Vec::new();
    let mut failed_peers = Vec::new();

    for (peer, result) in results {
        match result {
            Ok(()) => {
                liveness.record_success(&peer.id.key);
                retained_peers.push(peer);
            }
            Err(error) => match liveness.record_failure(&peer.id.key) {
                HeartbeatFailure::Evicted => failed_peers.push(peer),
                HeartbeatFailure::Retained { streak, threshold } => {
                    warn!(
                        "Heartbeat to {} failed ({}/{}); retaining connection: {}",
                        peer, streak, threshold, error
                    );
                    retained_peers.push(peer);
                }
            },
        }
    }

    // Bootstrap peer is pinned in KademliaStore so the node can always
    // rediscover it via findAndConnect.  Removing it from the routing
    // table is irreversible and strands the node if no other peers are
    // known.  The bootstrap is still removed from ConnectionsCell and
    // its gRPC channel is disconnected (the TCP connection IS broken).
    let bootstrap_key = conf.bootstrap.as_ref().map(|b| &b.id.key);
    let removable_peers: Vec<&PeerNode> = failed_peers
        .iter()
        .filter(|p| bootstrap_key != Some(&p.id.key))
        .collect();

    if failed_peers.len() > removable_peers.len() {
        tracing::debug!("Heartbeat to bootstrap peer failed, retaining in routing table");
    }

    // Log removal of failed peers
    for peer in &failed_peers {
        info!("Removing peer {} from connections", peer);
    }

    // Remove non-bootstrap failed peers from Kademlia routing table
    for peer in &removable_peers {
        if let Err(e) = node_discovery.remove_peer(peer) {
            warn!("Failed to remove peer {} from Kademlia: {}", peer, e);
        }
    }

    // Disconnect gRPC channels for ALL failed peers (including bootstrap)
    for peer in &failed_peers {
        if let Err(e) = transport.disconnect(peer).await {
            warn!("Failed to disconnect peer {}: {}", peer, e);
        }
    }

    // Update connections: remove all pinged peers, then add back successful ones
    let failed_count = failed_peers.len();
    connections_cell.flat_modify(|conns| {
        let updated = conns.remove_conns(to_ping.into_vec())?;
        updated.add_conns(retained_peers)
    })?;

    // Report connections if any were cleared
    if failed_count > 0 {
        let updated_connections = connections_cell.read()?;
        updated_connections.report_conn()?;
    }

    Ok((failed_count, failed_peers))
}

/// Reset connections by removing all current connections
pub fn reset_connections(connections_cell: &ConnectionsCell) -> Result<(), CommError> {
    connections_cell.flat_modify(|conns| conns.remove_conns(conns.clone().into_vec()))?;
    Ok(())
}

/// Find new peers and attempt to connect to them
pub async fn find_and_connect<N: NodeDiscovery + ?Sized, F, Fut>(
    connections_cell: &ConnectionsCell,
    node_discovery: &N,
    connect_fn: F,
) -> Result<Vec<PeerNode>, CommError>
where
    F: Fn(&PeerNode) -> Fut,
    Fut: std::future::Future<Output = Result<(), CommError>>,
{
    let current_connections = connections_cell.read()?.to_set();
    let all_peers = node_discovery.peers()?;

    // Filter out peers we're already connected to
    let new_peers: Vec<PeerNode> = all_peers
        .into_iter()
        .filter(|peer| !current_connections.contains(peer))
        .collect();

    let mut successful_connections = Vec::new();

    // Attempt to connect to each new peer
    for peer in new_peers {
        match connect_fn(&peer).await {
            Ok(()) => {
                successful_connections.push(peer);
            }
            Err(CommError::WrongNetwork(peer_addr, msg)) => {
                warn!("Can't connect to peer {}. {}", peer_addr, msg);
            }
            Err(_) => {
                warn!(
                    "An error occurred while trying to connect to peer: {:?}",
                    peer
                );
            }
        }
    }

    Ok(successful_connections)
}

/// Connect to a peer by sending a protocol handshake
pub async fn connect<T: TransportLayer>(
    peer: &PeerNode,
    conf: &RPConf,
    transport: &T,
) -> Result<(), CommError> {
    let start = std::time::Instant::now();
    let handshake_msg = protocol_helper::protocol_handshake(&conf.local, &conf.network_id);
    let result = transport.send(peer, &handshake_msg).await;

    // Record connect-time histogram (matches Scala Connect.scala:L174)
    metrics::histogram!(CONNECT_TIME_METRIC, "source" => RP_CONNECT_METRICS_SOURCE)
        .record(start.elapsed().as_secs_f64());

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier};
    use crate::rust::test_instances::NodeDiscoveryStub;

    fn peer(name: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from(name.as_bytes().to_vec()),
            },
            endpoint: Endpoint::new("host".to_string(), 80, 80),
        }
    }

    fn cell_with(peers: &[PeerNode]) -> ConnectionsCell {
        let cell = ConnectionsCell::new();
        cell.flat_modify(|_| Ok(Connections::from_vec(peers.to_vec())))
            .unwrap();
        cell
    }

    #[test]
    fn liveness_tracker_exposes_threshold() {
        let tracker = PeerLivenessTracker::new(3).unwrap();
        assert_eq!(tracker.threshold(), 3);
    }

    #[test]
    fn connections_is_empty_reflects_content() {
        assert!(Connections::empty().is_empty());
        assert!(!Connections::from_vec(vec![peer("A")]).is_empty());
    }

    #[test]
    fn add_conn_and_report_appends_connection() {
        let connections = Connections::empty()
            .add_conn_and_report(peer("A"))
            .unwrap()
            .add_conn_and_report(peer("B"))
            .unwrap();
        assert_eq!(connections.into_vec(), vec![peer("A"), peer("B")]);
    }

    #[test]
    fn remove_conn_and_report_drops_connection() {
        let connections = Connections::from_vec(vec![peer("A"), peer("B")])
            .remove_conn_and_report(peer("A"))
            .unwrap();
        assert_eq!(connections.into_vec(), vec![peer("B")]);
    }

    #[test]
    fn refresh_conn_moves_existing_connection_to_end() {
        let connections = Connections::from_vec(vec![peer("A"), peer("B"), peer("C")])
            .refresh_conn(peer("A"))
            .unwrap();
        assert_eq!(connections.into_vec(), vec![
            peer("B"),
            peer("C"),
            peer("A")
        ]);
    }

    #[test]
    fn refresh_conn_ignores_unknown_connection() {
        let connections = Connections::from_vec(vec![peer("A"), peer("B")])
            .refresh_conn(peer("X"))
            .unwrap();
        assert_eq!(connections.into_vec(), vec![peer("A"), peer("B")]);
    }

    #[test]
    fn random_returns_at_most_max_known_peers() {
        let all = vec![peer("A"), peer("B"), peer("C"), peer("D")];
        let cell = cell_with(&all);

        let two = cell.random(2).unwrap();
        assert_eq!(two.len(), 2);
        for p in two.iter() {
            assert!(all.contains(p));
        }

        let many = cell.random(10).unwrap();
        assert_eq!(many.to_set(), Connections::from_vec(all).to_set());
    }

    #[test]
    fn reset_connections_empties_the_cell() {
        let cell = cell_with(&[peer("A"), peer("B")]);
        reset_connections(&cell).unwrap();
        assert!(cell.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn find_and_connect_skips_wrong_network_peers() {
        let cell = cell_with(&[]);
        let mut discovery = NodeDiscoveryStub::new();
        discovery.nodes = vec![peer("good"), peer("wrong-net"), peer("broken")];

        let connect_fn = |p: &PeerNode| {
            let p = p.clone();
            async move {
                if p == peer("wrong-net") {
                    Err(CommError::WrongNetwork(
                        "wrong-net".to_string(),
                        "network mismatch".to_string(),
                    ))
                } else if p == peer("broken") {
                    Err(CommError::TimeOut)
                } else {
                    Ok(())
                }
            }
        };

        let connected = find_and_connect(&cell, &discovery, connect_fn)
            .await
            .unwrap();
        assert_eq!(connected, vec![peer("good")]);
    }
}
