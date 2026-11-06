use std::sync::Arc;

use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::transport::stream_observable::StreamObservable;
use comm::rust::transport::transport_layer::Blob;
use models::routing::Packet;
use prost::bytes::Bytes;

fn peer(id: &'static [u8]) -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: Bytes::from_static(id),
        },
        endpoint: Endpoint::new("host".to_string(), 40400, 40404),
    }
}

fn blob(sender: &PeerNode, content: &'static [u8]) -> Blob {
    Blob {
        sender: sender.clone(),
        packet: Packet {
            type_id: "test".to_string(),
            content: Bytes::from_static(content),
        },
    }
}

#[tokio::test]
async fn stopping_one_peer_discards_only_its_pending_packets() {
    let cache = Arc::new(dashmap::DashMap::new());
    let sender = peer(b"sender");
    let first = StreamObservable::new(peer(b"first"), 4, cache.clone());
    let second = StreamObservable::new(peer(b"second"), 4, cache.clone());

    first.enque(&blob(&sender, b"one")).await.unwrap();
    first.enque(&blob(&sender, b"two")).await.unwrap();
    second.enque(&blob(&sender, b"three")).await.unwrap();
    assert_eq!(cache.len(), 3);

    assert_eq!(first.stop_and_discard(), 2);
    assert!(first.is_complete());
    assert_eq!(cache.len(), 1);
    assert!(matches!(
        first.enque(&blob(&sender, b"four")).await,
        Err(CommError::PeerUnavailable(_))
    ));
    assert_eq!(cache.len(), 1);
    assert!(!second.is_complete());
}

#[tokio::test]
async fn concurrent_enqueue_and_stop_leave_no_cached_packets() {
    let cache = Arc::new(dashmap::DashMap::new());
    let sender = peer(b"sender");
    let queue = Arc::new(StreamObservable::new(peer(b"receiver"), 128, cache.clone()));
    let producer = {
        let queue = queue.clone();
        tokio::spawn(async move {
            for _ in 0..128 {
                let _ = queue.enque(&blob(&sender, b"pending")).await;
                tokio::task::yield_now().await;
            }
        })
    };

    tokio::task::yield_now().await;
    queue.stop_and_discard();
    producer.await.unwrap();
    assert!(queue.is_complete());
    assert!(cache.is_empty());
}
