// A failed network ask must not abort retriever state transitions: a comm
// error propagating out of dependency admission aborts the whole block
// validation upstream, and the validation-error quarantine then freezes the
// node's view of the sender for minutes (the slow-peer finality freeze).

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use async_trait::async_trait;
    use casper::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever, RequestState};
    use comm::rust::errors::CommError;
    use comm::rust::peer_node::PeerNode;
    use comm::rust::rp::connect::{Connections, ConnectionsCell};
    use comm::rust::test_instances::create_rp_conf_ask;
    use comm::rust::transport::transport_layer::{Blob, TransportLayer};
    use models::routing::Protocol;
    use models::rust::block_hash::BlockHash;

    use crate::engine::setup;

    /// Transport whose every send and broadcast fails like a dead peer's
    /// timed-out leg, while still counting the attempts.
    struct FailingTransport {
        attempts: AtomicUsize,
    }

    impl FailingTransport {
        fn new() -> Self {
            Self {
                attempts: AtomicUsize::new(0),
            }
        }

        fn attempts(&self) -> usize { self.attempts.load(Ordering::SeqCst) }

        fn err() -> CommError {
            CommError::ProtocolException("request timed out after 15000ms".to_string())
        }
    }

    #[async_trait]
    impl TransportLayer for FailingTransport {
        async fn send(&self, _peer: &PeerNode, _msg: &Protocol) -> Result<(), CommError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err(Self::err())
        }

        async fn broadcast(&self, _peers: &[PeerNode], _msg: &Protocol) -> Result<(), CommError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err(Self::err())
        }

        async fn stream(&self, _peer: &PeerNode, _blob: &Blob) -> Result<(), CommError> {
            Err(Self::err())
        }

        async fn stream_mult(&self, _peers: &[PeerNode], _blob: &Blob) -> Result<(), CommError> {
            Err(Self::err())
        }

        async fn disconnect(&self, _peer: &PeerNode) -> Result<(), CommError> { Ok(()) }

        async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
            Ok(HashSet::new())
        }
    }

    struct Fixture {
        retriever: BlockRetriever<FailingTransport>,
        transport: Arc<FailingTransport>,
        requested_blocks: Arc<Mutex<HashMap<BlockHash, RequestState>>>,
    }

    fn fixture() -> Fixture {
        let local = setup::peer_node("local", 40400);
        let remote = setup::peer_node("remote", 40401);
        let transport = Arc::new(FailingTransport::new());
        let requested_blocks = Arc::new(Mutex::new(HashMap::new()));
        let retriever = BlockRetriever::new(
            requested_blocks.clone(),
            transport.clone(),
            ConnectionsCell {
                peers: Arc::new(Mutex::new(Connections::from_vec(vec![remote]))),
            },
            create_rp_conf_ask(local, None, None),
        );
        Fixture {
            retriever,
            transport,
            requested_blocks,
        }
    }

    fn timed_out_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 10_000
    }

    #[tokio::test]
    async fn a_failed_ask_does_not_abort_dependency_admission() {
        let f = fixture();
        let hash = BlockHash::from(b"dep-behind-dead-peer".to_vec());

        let result = f
            .retriever
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await;

        assert!(
            result.is_ok(),
            "a failed has-block-request broadcast must not abort admission: {:?}",
            result.err()
        );
        assert!(
            f.transport.attempts() >= 1,
            "the ask must have been attempted"
        );
        assert!(
            f.requested_blocks.lock().unwrap().contains_key(&hash),
            "the admitted hash must stay tracked so the re-request clock owns the retry"
        );
    }

    #[tokio::test]
    async fn a_dead_peer_does_not_abort_the_maintenance_sweep() {
        let f = fixture();
        let waiting = setup::peer_node("waiting", 40402);
        let hashes = [
            BlockHash::from(b"first-unresolved".to_vec()),
            BlockHash::from(b"second-unresolved".to_vec()),
        ];
        for hash in &hashes {
            f.retriever
                .set_request_state_for_test(hash.clone(), RequestState {
                    timestamp: timed_out_ms(),
                    initial_timestamp: timed_out_ms(),
                    peers: HashSet::new(),
                    received: false,
                    in_casper_buffer: false,
                    waiting_list: vec![waiting.clone()],
                    peer_requery_cursor: 0,
                    requested_as_dependency: false,
                })
                .await
                .expect("seed request state");
        }

        let result = f.retriever.request_all(Duration::from_secs(240)).await;

        assert!(
            result.is_ok(),
            "one dead peer's send must not abort the whole sweep: {:?}",
            result.err()
        );
        assert!(
            f.transport.attempts() >= 2,
            "both unresolved entries must be re-asked, got {} attempts",
            f.transport.attempts()
        );
    }
}
