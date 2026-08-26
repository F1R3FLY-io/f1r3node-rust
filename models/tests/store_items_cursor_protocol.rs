use models::casper::{StoreItemsMessageProto, StoreItemsMessageRequestProto, StoreNodeKeyProto};
use models::rust::casper::protocol::casper_message::{
    StoreItemsCursorProtocol, StoreItemsMessage, StoreItemsMessageRequest, StoreNodeKey,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

fn hash(seed: u8) -> Blake2b256Hash {
    let mut bytes = vec![0; 32];
    bytes[0] = seed;
    Blake2b256Hash::from_bytes(bytes)
}

fn path(seed: u8) -> Vec<(Blake2b256Hash, Option<u8>)> { vec![(hash(seed), None)] }

fn response(
    protocol: StoreItemsCursorProtocol,
    start_path: Vec<(Blake2b256Hash, Option<u8>)>,
    last_path: Vec<(Blake2b256Hash, Option<u8>)>,
    terminal: bool,
) -> StoreItemsMessage {
    StoreItemsMessage {
        start_path,
        last_path,
        history_items: Vec::new(),
        data_items: Vec::new(),
        cursor_protocol: protocol,
        terminal,
    }
}

#[test]
fn only_an_explicit_legacy_epoch_normalizes_the_empty_terminal_cursor() {
    let start_path = path(1);
    let legacy = response(
        StoreItemsCursorProtocol::LegacyEmptyTerminal,
        start_path.clone(),
        Vec::new(),
        true,
    );
    assert_eq!(
        legacy
            .canonical_advertised_next_path(StoreItemsCursorProtocol::LegacyEmptyTerminal)
            .unwrap(),
        start_path
    );

    let unspecified = response(
        StoreItemsCursorProtocol::Unspecified,
        path(2),
        Vec::new(),
        true,
    );
    assert!(unspecified
        .canonical_advertised_next_path(StoreItemsCursorProtocol::AuthenticatedTerminal)
        .unwrap_err()
        .contains("selected explicitly"));
}

#[test]
fn mixed_epochs_and_forged_terminal_shapes_are_rejected() {
    let start_path = path(3);
    let next_path = vec![(hash(3), None), (hash(4), Some(7))];

    let mixed = response(
        StoreItemsCursorProtocol::LegacyEmptyTerminal,
        start_path.clone(),
        Vec::new(),
        true,
    );
    assert!(mixed
        .canonical_advertised_next_path(StoreItemsCursorProtocol::AuthenticatedTerminal)
        .unwrap_err()
        .contains("protocol mismatch"));

    let forged_terminal = response(
        StoreItemsCursorProtocol::AuthenticatedTerminal,
        start_path.clone(),
        next_path,
        true,
    );
    assert!(forged_terminal
        .canonical_advertised_next_path(StoreItemsCursorProtocol::AuthenticatedTerminal)
        .unwrap_err()
        .contains("must repeat startPath"));

    let forged_continuation = response(
        StoreItemsCursorProtocol::AuthenticatedTerminal,
        start_path.clone(),
        start_path,
        false,
    );
    assert!(forged_continuation
        .canonical_advertised_next_path(StoreItemsCursorProtocol::AuthenticatedTerminal)
        .unwrap_err()
        .contains("distinct continuation"));
}

#[test]
fn cursor_indexes_are_canonical_and_never_truncated() {
    for invalid in [-1, 257, i32::MAX] {
        let error = StoreNodeKey::from_proto(StoreNodeKeyProto {
            hash: Bytes::from(vec![0; 32]),
            index: invalid,
        })
        .unwrap_err();
        assert!(error.contains("0..=255 or sentinel 256"));
    }

    assert_eq!(
        StoreNodeKey::from_proto(StoreNodeKeyProto {
            hash: Bytes::from(vec![0; 32]),
            index: 256,
        })
        .unwrap()
        .1,
        None
    );
}

#[test]
fn malformed_cursor_hash_width_propagates_through_request_and_response_decode() {
    for width in [31, 33] {
        let request = StoreItemsMessageRequestProto {
            start_path: vec![StoreNodeKeyProto {
                hash: Bytes::from(vec![0; width]),
                index: 256,
            }],
            skip: 0,
            take: 1,
            cursor_protocol: StoreItemsCursorProtocol::AuthenticatedTerminal.to_proto(),
        };
        assert!(StoreItemsMessageRequest::from_proto(request)
            .unwrap_err()
            .contains("exactly 32 bytes"));
    }

    let response = StoreItemsMessageProto {
        start_path: vec![StoreNodeKeyProto {
            hash: Bytes::from(vec![0; 32]),
            index: 256,
        }],
        last_path: vec![StoreNodeKeyProto {
            hash: Bytes::from(vec![0; 33]),
            index: 256,
        }],
        history_items: Vec::new(),
        data_items: Vec::new(),
        cursor_protocol: StoreItemsCursorProtocol::AuthenticatedTerminal.to_proto(),
        terminal: true,
    };
    assert!(StoreItemsMessage::from_proto(response)
        .unwrap_err()
        .contains("exactly 32 bytes"));
}
