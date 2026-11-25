use std::sync::Arc;
use std::time::Duration;

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

    assert_eq!(first.close_and_discard(), 2);
    assert!(first.is_complete());
    assert_eq!(cache.len(), 1);
    assert!(matches!(
        first.enque(&blob(&sender, b"four")).await,
        Err(CommError::PeerUnavailable(_))
    ));
    assert_eq!(cache.len(), 1);
    assert!(!second.is_complete());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_during_concurrent_enqueue_leaves_no_cached_packets() {
    let cache = Arc::new(dashmap::DashMap::new());
    let sender = peer(b"sender");
    let queue = Arc::new(StreamObservable::new(peer(b"receiver"), 64, cache.clone()));
    let producer = {
        let queue = queue.clone();
        tokio::spawn(async move {
            let mut sent = 0usize;
            loop {
                match queue.enque(&blob(&sender, b"pending")).await {
                    Ok(()) => sent += 1,
                    Err(CommError::PeerUnavailable(_)) => return sent,
                    Err(other) => panic!("unexpected enqueue error: {other}"),
                }
                tokio::task::yield_now().await;
            }
        })
    };

    tokio::time::timeout(Duration::from_secs(5), async {
        while cache.len() < 16 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("producer fills the queue");

    queue.close_and_discard();
    let sent = tokio::time::timeout(Duration::from_secs(5), producer)
        .await
        .expect("producer stops once the queue is closed")
        .unwrap();

    assert!(sent > 0);
    assert!(queue.is_complete());
    assert!(cache.is_empty());
}
