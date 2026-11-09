// Resilience check for issue #666: a hostile peer must not be able to make a
// node do unbounded work with one `StoreItemsMessageRequest`.
//
// This drives a real standalone node (`Running` engine, real genesis history
// exporter) through the exact dispatch path the production gRPC receiver uses
// (`CasperPacketHandler::handle_packet` -> `Engine::handle`). A peer the node
// never asked for sends crafted requests with out-of-bounds `skip`/`take`.
//
// Expected, with the fix in `casper/src/rust/engine/running.rs`:
//   * each hostile request is dropped with no outbound response and no panic;
//   * the node stays up and still answers an honest page request afterwards.
//
// Without the bound, a negative or huge `take` exports the whole state trie
// into one response, a huge `skip` walks the trie for nothing, and (0, 0)
// panics inside `sequential_export`.

use casper::rust::engine::lfs_tuple_space_requester::PAGE_SIZE;
use casper::rust::engine::running::STORE_ITEMS_REQUEST_MAX_PAGE;
use casper::rust::protocol::{casper_message_from_proto, to_casper_message_proto};
use comm::rust::p2p::packet_handler::PacketHandler;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use models::routing::{Packet, Protocol};
use models::rust::casper::protocol::casper_message::{CasperMessage, StoreItemsMessageRequest};
use models::rust::casper::protocol::packet_type_tag::ToPacket;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn attacker_peer() -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: Bytes::from_static(b"issue-666-hostile-peer"),
        },
        endpoint: Endpoint::new("attacker.invalid".to_string(), 40400, 40404),
    }
}

fn store_items_request_packet(root: &Blake2b256Hash, skip: i32, take: i32) -> Packet {
    StoreItemsMessageRequest {
        start_path: vec![(root.clone(), None)],
        skip,
        take,
    }
    .to_proto()
    .mk_packet()
}

fn decode_response(protocol: &Protocol) -> CasperMessage {
    let Some(models::routing::protocol::Message::Packet(packet)) = &protocol.message else {
        panic!("expected a packet response from the node");
    };
    let proto = to_casper_message_proto(packet)
        .get()
        .expect("response packet must parse");
    casper_message_from_proto(proto).expect("response must be a casper message")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_store_items_request_is_dropped_without_overloading_the_node() {
    crate::init_logger();

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone_with_state_exporter(genesis)
        .await
        .unwrap();

    let attacker = attacker_peer();
    let network = node.tle.test_network();
    let root = Blake2b256Hash::from_bytes_prost(&node.genesis.body.state.post_state_hash);

    // Baseline: an honest page request (skip = 0, take = 750) is answered with a
    // StoreItemsMessage. This also proves the node really serves this peer.
    node.packet_handler
        .handle_packet(&attacker, &store_items_request_packet(&root, 0, PAGE_SIZE))
        .await
        .unwrap();
    let answered = network.peer_queue(&attacker).unwrap();
    assert_eq!(
        answered.len(),
        1,
        "an honest page request must be answered exactly once"
    );
    assert!(
        matches!(
            decode_response(&answered[0]),
            CasperMessage::StoreItemsMessage(_)
        ),
        "the honest answer must be a StoreItemsMessage"
    );
    network.clear(&attacker).unwrap();

    // Every out-of-bounds request must be dropped: no outbound response, no
    // panic. `over` is one past the honest maximum; i32::MAX / -1 are the
    // unbounded-export values; (0, 0) and (skip, 0) are the panic cases.
    let over = STORE_ITEMS_REQUEST_MAX_PAGE + 1;
    for (skip, take) in [
        (0, 0),
        (5, 0),
        (0, -1),
        (0, over),
        (0, i32::MAX),
        (-1, PAGE_SIZE),
        (i32::MAX, PAGE_SIZE),
    ] {
        node.packet_handler
            .handle_packet(&attacker, &store_items_request_packet(&root, skip, take))
            .await
            .unwrap_or_else(|e| {
                panic!("hostile request skip={skip} take={take} must not error: {e}")
            });
        assert!(
            network.peer_queue(&attacker).unwrap().is_empty(),
            "hostile request skip={skip} take={take} must produce no response or trie export"
        );
    }

    // Liveness: after the flood the node still answers an honest request, so the
    // engine was neither poisoned nor brought down.
    node.packet_handler
        .handle_packet(&attacker, &store_items_request_packet(&root, 0, PAGE_SIZE))
        .await
        .unwrap();
    let after = network.peer_queue(&attacker).unwrap();
    assert_eq!(
        after.len(),
        1,
        "the node must still serve honest requests after the hostile flood"
    );
    assert!(
        matches!(
            decode_response(&after[0]),
            CasperMessage::StoreItemsMessage(_)
        ),
        "the post-flood answer must be a StoreItemsMessage"
    );
}
