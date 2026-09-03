use std::sync::Arc;

use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::transport::chunker::Chunker;
use comm::rust::transport::payload_budget::PayloadBudget;
use comm::rust::transport::stream_handler::{Circuit, CircuitBreaker, StreamError, StreamHandler};
use comm::rust::transport::transport_layer::Blob;
use futures::stream;
use models::routing::chunk::Content;
use models::routing::{Chunk, ChunkData, Packet};
use proptest::prelude::*;
use prost::bytes::Bytes;
use shared::rust::shared::compression::Compression;
use tokio_stream::Stream;

const NETWORK_ID: &str = "test";
const MAX_PAYLOAD: usize = 2 * 1024 * 1024;

fn peer_node(name: &str) -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: Bytes::copy_from_slice(name.as_bytes()),
        },
        endpoint: Endpoint::new("127.0.0.1".to_string(), 40400, 40400),
    }
}

fn blob(content: Vec<u8>) -> Blob {
    Blob {
        sender: peer_node("sender"),
        packet: Packet {
            type_id: "BlockMessageTest".to_string(),
            content: Bytes::from(content),
        },
    }
}

fn budget(items: usize) -> Arc<PayloadBudget> {
    let wire = Compression::max_compressed_allocation(MAX_PAYLOAD).unwrap();
    PayloadBudget::new("test", MAX_PAYLOAD + wire, items).unwrap()
}

fn never_break(_: &comm::rust::transport::stream_handler::Streamed) -> Circuit { Circuit::Closed }

async fn handle<S>(
    source: S,
    payload_budget: &Arc<PayloadBudget>,
) -> comm::rust::transport::messages::StreamMessage
where
    S: Stream<Item = Chunk> + Unpin,
{
    StreamHandler::handle_stream(source, never_break, payload_budget, MAX_PAYLOAD)
        .await
        .unwrap()
}

async fn handle_error<S>(
    source: S,
    breaker: CircuitBreaker,
    payload_budget: &Arc<PayloadBudget>,
) -> StreamError
where
    S: Stream<Item = Chunk> + Unpin,
{
    StreamHandler::handle_stream(source, breaker, payload_budget, MAX_PAYLOAD)
        .await
        .unwrap_err()
}

fn chunks(content: Vec<u8>, chunk_size: usize) -> Vec<Chunk> {
    Chunker::chunk_it(NETWORK_ID, &blob(content), chunk_size).unwrap()
}

#[tokio::test]
async fn metadata_and_uncompressed_payload_round_trip_without_cache() {
    let payload_budget = budget(1);
    let content = b"Hello, World!".to_vec();
    let message = handle(stream::iter(chunks(content.clone(), 4096)), &payload_budget).await;
    assert_eq!(message.sender, peer_node("sender"));
    assert_eq!(message.type_id, "BlockMessageTest");
    assert_eq!(message.content_length, content.len() as i32);
    assert!(!message.compressed);
    assert_eq!(payload_budget.used_bytes(), content.len());
    let (restored, reservation) = StreamHandler::restore(message).await.unwrap();
    assert_eq!(restored.packet.content.as_ref(), content);
    assert_eq!(reservation.bytes(), content.len());
    drop(reservation);
    assert_eq!(payload_budget.used_bytes(), 0);
    assert_eq!(payload_budget.active_items(), 0);
}

#[tokio::test]
async fn compressed_payload_reserves_declared_and_wire_bytes_and_round_trips() {
    let payload_budget = budget(1);
    let content = vec![42; 600 * 1024];
    let message = handle(stream::iter(chunks(content.clone(), 4096)), &payload_budget).await;
    assert!(message.compressed);
    assert!(message.reserved_bytes() > content.len());
    assert_eq!(payload_budget.used_bytes(), message.reserved_bytes());
    let (restored, reservation) = StreamHandler::restore(message).await.unwrap();
    assert_eq!(restored.packet.content.as_ref(), content);
    drop(reservation);
    assert_eq!(payload_budget.used_bytes(), 0);
}

#[tokio::test]
async fn every_rejected_stream_releases_its_reservation() {
    let cases = vec![
        Vec::<Chunk>::new(),
        chunks(vec![1; 4096], 4096).into_iter().skip(1).collect(),
        vec![Chunk { content: None }],
    ];
    for case in cases {
        let payload_budget = budget(1);
        let error = handle_error(stream::iter(case), never_break, &payload_budget).await;
        assert!(matches!(error, StreamError::NotFullMessage { .. }));
        assert_eq!(payload_budget.used_bytes(), 0);
        assert_eq!(payload_budget.active_items(), 0);
    }
}

#[tokio::test]
async fn incomplete_duplicate_and_data_before_header_are_rejected() {
    let complete = chunks(vec![7; 20_000], 4096);
    let incomplete: Vec<_> = complete
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, chunk)| chunk.clone())
        .collect();
    let duplicate = [vec![complete[0].clone()], complete.clone()].concat();
    let before_header = vec![
        Chunk {
            content: Some(Content::Data(ChunkData {
                content_data: Bytes::from_static(b"early"),
            })),
        },
        complete[0].clone(),
    ];
    for case in [incomplete, duplicate, before_header] {
        let payload_budget = budget(1);
        let error = handle_error(stream::iter(case), never_break, &payload_budget).await;
        assert!(matches!(error, StreamError::NotFullMessage { .. }));
        assert_eq!(payload_budget.used_bytes(), 0);
    }
}

#[tokio::test]
async fn negative_and_oversized_declared_lengths_are_rejected_before_allocation() {
    for declared in [-1, (MAX_PAYLOAD + 1) as i32] {
        let mut input = chunks(Vec::new(), 4096);
        let Some(Content::Header(header)) = input[0].content.as_mut() else {
            unreachable!()
        };
        header.content_length = declared;
        let payload_budget = budget(1);
        let error = handle_error(stream::iter(input), never_break, &payload_budget).await;
        assert!(matches!(
            error,
            StreamError::Unexpected { .. } | StreamError::MaxSizeReached
        ));
        assert_eq!(payload_budget.used_bytes(), 0);
    }
}

#[tokio::test]
async fn compressed_wire_length_has_a_checked_derived_ceiling() {
    let declared = 8usize;
    let max_wire = Compression::max_compressed_allocation(MAX_PAYLOAD).unwrap();
    let mut input = chunks(Vec::new(), 4096);
    let Some(Content::Header(header)) = input[0].content.as_mut() else {
        unreachable!()
    };
    header.compressed = true;
    header.content_length = declared as i32;
    input.push(Chunk {
        content: Some(Content::Data(ChunkData {
            content_data: Bytes::from(vec![0; max_wire + 1]),
        })),
    });
    let payload_budget = PayloadBudget::new("test", max_wire + declared + 1, 1).unwrap();
    let error = handle_error(stream::iter(input), never_break, &payload_budget).await;
    assert!(matches!(error, StreamError::MaxSizeReached));
    assert_eq!(payload_budget.used_bytes(), 0);
}

#[tokio::test]
async fn circuit_and_network_rejection_release_reservations() {
    fn reject(_: &comm::rust::transport::stream_handler::Streamed) -> Circuit {
        Circuit::opened(StreamError::MaxSizeReached)
    }
    fn network(streamed: &comm::rust::transport::stream_handler::Streamed) -> Circuit {
        match &streamed.header {
            Some(header) if header.network_id != NETWORK_ID => {
                Circuit::opened(StreamError::wrong_network_id())
            }
            _ => Circuit::Closed,
        }
    }
    let payload_budget = budget(1);
    let error = handle_error(
        stream::iter(chunks(vec![1; 4096], 4096)),
        reject,
        &payload_budget,
    )
    .await;
    assert!(matches!(error, StreamError::MaxSizeReached));
    assert_eq!(payload_budget.used_bytes(), 0);

    let mut wrong = chunks(vec![1; 32], 4096);
    let Some(Content::Header(header)) = wrong[0].content.as_mut() else {
        unreachable!()
    };
    header.network_id = "wrong".to_string();
    let error = handle_error(stream::iter(wrong), network, &payload_budget).await;
    assert!(matches!(error, StreamError::WrongNetworkId));
    assert_eq!(payload_budget.used_bytes(), 0);
}

#[tokio::test]
async fn item_capacity_rejects_a_second_live_stream_and_recovers_after_drop() {
    let payload_budget = budget(1);
    let first = handle(stream::iter(chunks(vec![1; 32], 4096)), &payload_budget).await;
    let error = handle_error(
        stream::iter(chunks(vec![2; 32], 4096)),
        never_break,
        &payload_budget,
    )
    .await;
    assert!(matches!(error, StreamError::ResourceExhausted { .. }));
    drop(first);
    let second = handle(stream::iter(chunks(vec![3; 32], 4096)), &payload_budget).await;
    drop(second);
    assert_eq!(payload_budget.active_items(), 0);
}

#[tokio::test]
async fn cancellation_releases_header_reservation() {
    use futures::StreamExt;

    let payload_budget = budget(1);
    let header = chunks(vec![1; 1024], 4096).remove(0);
    let source = stream::iter(vec![header]).chain(stream::pending::<Chunk>());
    let task_budget = payload_budget.clone();
    let task = tokio::spawn(async move {
        StreamHandler::handle_stream(source, never_break, &task_budget, MAX_PAYLOAD).await
    });
    tokio::task::yield_now().await;
    assert_eq!(payload_budget.active_items(), 1);
    task.abort();
    let _ = task.await;
    assert_eq!(payload_budget.used_bytes(), 0);
    assert_eq!(payload_budget.active_items(), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn arbitrary_uncompressed_payloads_round_trip(
        content in prop::collection::vec(any::<u8>(), 0..128 * 1024),
        chunk_size in 2049usize..32 * 1024,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let payload_budget = budget(1);
            let message = handle(stream::iter(chunks(content.clone(), chunk_size)), &payload_budget).await;
            let (restored, reservation) = StreamHandler::restore(message).await.unwrap();
            prop_assert_eq!(restored.packet.content.as_ref(), content.as_slice());
            drop(reservation);
            prop_assert_eq!(payload_budget.used_bytes(), 0);
            Ok(())
        })?;
    }
}
