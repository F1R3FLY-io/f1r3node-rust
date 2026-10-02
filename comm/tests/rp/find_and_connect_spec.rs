// See comm/src/test/scala/coop/rchain/comm/rp/FindAndConnectSpec.scala

use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{
    find_and_connect, ConnectFailureTracker, Connections, ConnectionsCell,
};
use comm::rust::test_instances::NodeDiscoveryStub;
use prost::bytes::Bytes;

/// Helper function to create a peer with given name
fn peer(name: &str) -> PeerNode {
    let key = Bytes::from(name.as_bytes().to_vec());
    let id = NodeIdentifier { key };
    let endpoint = Endpoint::new("host".to_string(), 80, 80);
    PeerNode { id, endpoint }
}

/// Helper function to create a ConnectionsCell with given peers
fn mk_connections(peers: &[PeerNode]) -> ConnectionsCell {
    let connections_cell = ConnectionsCell::new();
    let connections = Connections::from_vec(peers.to_vec());
    connections_cell.flat_modify(|_| Ok(connections)).unwrap();
    connections_cell
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    // Test state to track which peers were called and which should succeed
    struct TestState {
        connect_called: Arc<Mutex<Vec<PeerNode>>>,
        will_connect_successfully: Arc<Mutex<Vec<PeerNode>>>,
    }

    impl TestState {
        fn new() -> Self {
            Self {
                connect_called: Arc::new(Mutex::new(Vec::new())),
                will_connect_successfully: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn reset(&self) {
            self.connect_called.lock().unwrap().clear();
            self.will_connect_successfully.lock().unwrap().clear();
        }

        fn set_successful_peers(&self, peers: Vec<PeerNode>) {
            *self.will_connect_successfully.lock().unwrap() = peers;
        }

        fn get_called_peers(&self) -> Vec<PeerNode> { self.connect_called.lock().unwrap().clone() }

        fn create_connect_fn(
            &self,
        ) -> impl Fn(
            &PeerNode,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), CommError>> + Send>,
        > + '_ {
            move |peer: &PeerNode| {
                let peer = peer.clone();
                let connect_called = self.connect_called.clone();
                let will_connect_successfully = self.will_connect_successfully.clone();

                Box::pin(async move {
                    // Record that this peer was called
                    connect_called.lock().unwrap().push(peer.clone());

                    // Check if this peer should succeed
                    let successful_peers = will_connect_successfully.lock().unwrap();
                    if successful_peers.contains(&peer) {
                        Ok(())
                    } else {
                        Err(CommError::TimeOut)
                    }
                })
            }
        }
    }

    mod when_there_are_no_connections_yet {
        use super::*;

        #[tokio::test]
        async fn should_ask_node_discovery_for_list_of_peers_and_try_to_connect_to_it() {
            // given
            let connections = mk_connections(&[]);
            let mut node_discovery = NodeDiscoveryStub::new();
            node_discovery.nodes = vec![peer("A"), peer("B"), peer("C")];
            let test_state = TestState::new();
            test_state.reset();

            // when
            let connect_fn = test_state.create_connect_fn();
            let mut failures = ConnectFailureTracker::new(3).unwrap();
            let result = find_and_connect(
                &connections,
                &node_discovery,
                connect_fn,
                None,
                &mut failures,
            )
            .await;

            // then
            assert!(result.is_ok());
            let connected_peers = result.unwrap();

            // Should have attempted to connect to all 3 peers
            let called_peers = test_state.get_called_peers();
            assert_eq!(called_peers.len(), 3);
            assert!(called_peers.contains(&peer("A")));
            assert!(called_peers.contains(&peer("B")));
            assert!(called_peers.contains(&peer("C")));

            // Since all connections failed, no peers should be in the result
            assert_eq!(connected_peers.len(), 0);
        }

        #[tokio::test]
        async fn should_report_peers_it_connected_to_successfully() {
            // given
            let connections = mk_connections(&[]);
            let mut node_discovery = NodeDiscoveryStub::new();
            node_discovery.nodes = vec![peer("A"), peer("B"), peer("C")];
            let test_state = TestState::new();
            test_state.reset();
            test_state.set_successful_peers(vec![peer("A"), peer("C")]);

            // when
            let connect_fn = test_state.create_connect_fn();
            let mut failures = ConnectFailureTracker::new(3).unwrap();
            let result = find_and_connect(
                &connections,
                &node_discovery,
                connect_fn,
                None,
                &mut failures,
            )
            .await;

            // then
            assert!(result.is_ok());
            let connected_peers = result.unwrap();

            // Should have successfully connected to A and C
            assert_eq!(connected_peers.len(), 2);
            assert!(connected_peers.contains(&peer("A")));
            assert!(connected_peers.contains(&peer("C")));
            assert!(!connected_peers.contains(&peer("B")));
        }
    }

    mod when_there_already_are_some_connections {
        use super::*;

        #[tokio::test]
        async fn should_ask_node_discovery_for_list_of_peers_and_try_to_connect_to_ones_not_connected_yet(
        ) {
            // given
            let connections = mk_connections(&[peer("B")]);
            let mut node_discovery = NodeDiscoveryStub::new();
            node_discovery.nodes = vec![peer("A"), peer("B"), peer("C")];
            let test_state = TestState::new();
            test_state.reset();

            // when
            let connect_fn = test_state.create_connect_fn();
            let mut failures = ConnectFailureTracker::new(3).unwrap();
            let result = find_and_connect(
                &connections,
                &node_discovery,
                connect_fn,
                None,
                &mut failures,
            )
            .await;

            // then
            assert!(result.is_ok());

            // Should have attempted to connect to only A and C (not B, since we're already connected)
            let called_peers = test_state.get_called_peers();
            assert_eq!(called_peers.len(), 2);
            assert!(called_peers.contains(&peer("A")));
            assert!(called_peers.contains(&peer("C")));
            assert!(!called_peers.contains(&peer("B")));
        }

        #[tokio::test]
        async fn should_report_peers_it_connected_to_successfully() {
            // given
            let connections = mk_connections(&[peer("B")]);
            let mut node_discovery = NodeDiscoveryStub::new();
            node_discovery.nodes = vec![peer("A"), peer("B"), peer("C")];
            let test_state = TestState::new();
            test_state.reset();
            test_state.set_successful_peers(vec![peer("A")]);

            // when
            let connect_fn = test_state.create_connect_fn();
            let mut failures = ConnectFailureTracker::new(3).unwrap();
            let result = find_and_connect(
                &connections,
                &node_discovery,
                connect_fn,
                None,
                &mut failures,
            )
            .await;

            // then
            assert!(result.is_ok());
            let connected_peers = result.unwrap();

            // Should have successfully connected to only A
            assert_eq!(connected_peers.len(), 1);
            assert!(connected_peers.contains(&peer("A")));
            assert!(!connected_peers.contains(&peer("B"))); // Already connected
            assert!(!connected_peers.contains(&peer("C"))); // Failed to connect
        }
    }
}

#[cfg(test)]
mod eviction_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use comm::rust::discovery::node_discovery::NodeDiscovery;

    use super::*;

    struct MemoryDiscovery {
        nodes: Mutex<Vec<PeerNode>>,
        removed: Mutex<Vec<PeerNode>>,
    }

    impl MemoryDiscovery {
        fn new(nodes: Vec<PeerNode>) -> Self {
            Self {
                nodes: Mutex::new(nodes),
                removed: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl NodeDiscovery for MemoryDiscovery {
        async fn discover(&self) -> Result<(), CommError> { Ok(()) }

        fn peers(&self) -> Result<Vec<PeerNode>, CommError> {
            Ok(self.nodes.lock().unwrap().clone())
        }

        fn remove_peer(&self, peer: &PeerNode) -> Result<(), CommError> {
            self.nodes.lock().unwrap().retain(|node| node.id != peer.id);
            self.removed.lock().unwrap().push(peer.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn three_failed_dials_evict_a_peer_that_never_connected() {
        let dead = peer("dead");
        let discovery = MemoryDiscovery::new(vec![dead.clone()]);
        let connections = mk_connections(&[]);
        let mut failures = ConnectFailureTracker::new(3).unwrap();
        let attempts = AtomicUsize::new(0);

        for _ in 0..2 {
            find_and_connect(
                &connections,
                &discovery,
                |_| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    async { Err(CommError::TimeOut) }
                },
                None,
                &mut failures,
            )
            .await
            .unwrap();
            assert_eq!(discovery.peers().unwrap(), vec![dead.clone()]);
        }

        find_and_connect(
            &connections,
            &discovery,
            |_| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err(CommError::TimeOut) }
            },
            None,
            &mut failures,
        )
        .await
        .unwrap();
        assert!(discovery.peers().unwrap().is_empty());
        assert_eq!(discovery.removed.lock().unwrap().len(), 1);

        find_and_connect(
            &connections,
            &discovery,
            |_| {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err(CommError::TimeOut) }
            },
            None,
            &mut failures,
        )
        .await
        .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn successful_dial_resets_the_failure_streak() {
        let candidate = peer("candidate");
        let discovery = MemoryDiscovery::new(vec![candidate.clone()]);
        let connections = mk_connections(&[]);
        let mut failures = ConnectFailureTracker::new(3).unwrap();
        let attempts = AtomicUsize::new(0);

        for _ in 0..4 {
            find_and_connect(
                &connections,
                &discovery,
                |_| {
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if attempt == 1 {
                            Ok(())
                        } else {
                            Err(CommError::TimeOut)
                        }
                    }
                },
                None,
                &mut failures,
            )
            .await
            .unwrap();
        }
        assert_eq!(discovery.peers().unwrap(), vec![candidate]);
        assert!(discovery.removed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_bootstrap_dials_never_evict_the_bootstrap_peer() {
        let bootstrap = peer("bootstrap");
        let discovery = MemoryDiscovery::new(vec![bootstrap.clone()]);
        let connections = mk_connections(&[]);
        let mut failures = ConnectFailureTracker::new(3).unwrap();

        for _ in 0..5 {
            find_and_connect(
                &connections,
                &discovery,
                |_| async { Err(CommError::TimeOut) },
                Some(&bootstrap),
                &mut failures,
            )
            .await
            .unwrap();
        }
        assert_eq!(discovery.peers().unwrap(), vec![bootstrap]);
        assert!(discovery.removed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn changed_address_starts_a_new_failure_streak() {
        let original = peer("candidate");
        let mut changed = original.clone();
        changed.endpoint.host = "new-host".to_string();
        let discovery = MemoryDiscovery::new(vec![original]);
        let connections = mk_connections(&[]);
        let mut failures = ConnectFailureTracker::new(2).unwrap();

        find_and_connect(
            &connections,
            &discovery,
            |_| async { Err(CommError::TimeOut) },
            None,
            &mut failures,
        )
        .await
        .unwrap();
        *discovery.nodes.lock().unwrap() = vec![changed.clone()];
        find_and_connect(
            &connections,
            &discovery,
            |_| async { Err(CommError::TimeOut) },
            None,
            &mut failures,
        )
        .await
        .unwrap();
        assert_eq!(discovery.peers().unwrap(), vec![changed]);
    }
}
