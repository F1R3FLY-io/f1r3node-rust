// See casper/src/main/scala/coop/rchain/casper/protocol/package.scala
// See models/src/main/scala/coop/rchain/casper/protocol/PacketTypeTag.scala

use comm::rust::rp::protocol_helper;
use models::casper::{
    ApprovedBlockProto, ApprovedBlockRequestProto, BlockApprovalProto, BlockHashMessageProto,
    BlockMessageProto, BlockRequestProto, FloorCacheRequestProto, FloorCacheResponseProto,
    ForkChoiceTipRequestProto, GetSnapshotChunkRequestProto, GetWalPayloadRequestProto,
    HasBlockProto, HasBlockRequestProto, HasSnapshotProto, HasSnapshotRequestProto,
    HasWalPayloadProto, HasWalPayloadRequestProto, MergeableEntryRequestProto,
    MergeableEntryResponseProto, NoApprovedBlockAvailableProto, SnapshotChunkResponseProto,
    StoreItemsMessageProto, StoreItemsMessageRequestProto, UnapprovedBlockProto,
    WalPayloadResponseProto,
};
use models::routing::{Packet, Protocol};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::CasperMessage;
use prost::Message;

/// Result type for packet parsing operations
#[derive(Debug, Clone, PartialEq)]
pub enum PacketParseResult<T> {
    Success(T),
    Failure(String),
    IllegalPacket(String),
}

impl<T> PacketParseResult<T> {
    pub fn is_success(&self) -> bool { matches!(self, PacketParseResult::Success(_)) }

    pub fn get(self) -> Result<T, String> {
        match self {
            PacketParseResult::Success(value) => Ok(value),
            PacketParseResult::Failure(msg) | PacketParseResult::IllegalPacket(msg) => Err(msg),
        }
    }

    pub fn map<U, F>(self, f: F) -> PacketParseResult<U>
    where F: FnOnce(T) -> U {
        match self {
            PacketParseResult::Success(value) => PacketParseResult::Success(f(value)),
            PacketParseResult::Failure(msg) => PacketParseResult::Failure(msg),
            PacketParseResult::IllegalPacket(msg) => PacketParseResult::IllegalPacket(msg),
        }
    }

    pub fn fold<U, F1, F2>(self, on_success: F1, on_failure: F2) -> U
    where
        F1: FnOnce(T) -> U,
        F2: FnOnce(String) -> U,
    {
        match self {
            PacketParseResult::Success(value) => on_success(value),
            PacketParseResult::Failure(msg) | PacketParseResult::IllegalPacket(msg) => {
                on_failure(msg)
            }
        }
    }
}

/// Enum representing all possible Casper message types
#[derive(Debug, Clone, PartialEq)]
pub enum CasperMessageProto {
    BlockHashMessage(BlockHashMessageProto),
    BlockMessage(BlockMessageProto),
    ApprovedBlock(ApprovedBlockProto),
    ApprovedBlockRequest(ApprovedBlockRequestProto),
    BlockRequest(BlockRequestProto),
    HasBlock(HasBlockProto),
    HasBlockRequest(HasBlockRequestProto),
    ForkChoiceTipRequest(ForkChoiceTipRequestProto),
    BlockApproval(BlockApprovalProto),
    UnapprovedBlock(UnapprovedBlockProto),
    NoApprovedBlockAvailable(NoApprovedBlockAvailableProto),
    StoreItemsMessageRequest(StoreItemsMessageRequestProto),
    StoreItemsMessage(StoreItemsMessageProto),
    MergeableEntryRequest(MergeableEntryRequestProto),
    MergeableEntryResponse(MergeableEntryResponseProto),
    FloorCacheRequest(FloorCacheRequestProto),
    FloorCacheResponse(FloorCacheResponseProto),
    // Snapshot chunk-fetch (Wave 3, Phase 7b-1).
    GetSnapshotChunkRequest(GetSnapshotChunkRequestProto),
    SnapshotChunkResponse(SnapshotChunkResponseProto),
    HasSnapshotRequest(HasSnapshotRequestProto),
    HasSnapshot(HasSnapshotProto),
    // Between-snapshot WAL payload fetch (Wave 3, Phase 7b-2).
    GetWalPayloadRequest(GetWalPayloadRequestProto),
    WalPayloadResponse(WalPayloadResponseProto),
    HasWalPayloadRequest(HasWalPayloadRequestProto),
    HasWalPayload(HasWalPayloadProto),
}

/// Extract a Packet from a Protocol message
pub fn extract_packet_from_protocol(protocol: &Protocol) -> Result<Packet, String> {
    protocol_helper::to_packet(protocol).map_err(|e| format!("Failed to extract packet: {:?}", e))
}

/// Convert a Packet to a CasperMessageProto based on type ID
pub fn to_casper_message_proto(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    match packet.type_id.as_str() {
        "BlockHashMessage" => convert_block_hash_message(packet),
        "BlockMessage" => convert_block_message(packet),
        "ApprovedBlock" => convert_approved_block(packet),
        "ApprovedBlockRequest" => convert_approved_block_request(packet),
        "BlockRequest" => convert_block_request(packet),
        "HasBlock" => convert_has_block(packet),
        "HasBlockRequest" => convert_has_block_request(packet),
        "ForkChoiceTipRequest" => convert_fork_choice_tip_request(packet),
        "BlockApproval" => convert_block_approval(packet),
        "UnapprovedBlock" => convert_unapproved_block(packet),
        "NoApprovedBlockAvailable" => convert_no_approved_block_available(packet),
        "StoreItemsMessageRequest" => convert_store_items_message_request(packet),
        "StoreItemsMessage" => convert_store_items_message(packet),
        "MergeableEntryRequest" => convert_mergeable_entry_request(packet),
        "MergeableEntryResponse" => convert_mergeable_entry_response(packet),
        "FloorCacheRequest" => parse_packet::<FloorCacheRequestProto>(packet)
            .map(CasperMessageProto::FloorCacheRequest),
        "FloorCacheResponse" => parse_packet::<FloorCacheResponseProto>(packet)
            .map(CasperMessageProto::FloorCacheResponse),
        "GetSnapshotChunkRequest" => parse_packet::<GetSnapshotChunkRequestProto>(packet)
            .map(CasperMessageProto::GetSnapshotChunkRequest),
        "SnapshotChunkResponse" => parse_packet::<SnapshotChunkResponseProto>(packet)
            .map(CasperMessageProto::SnapshotChunkResponse),
        "HasSnapshotRequest" => parse_packet::<HasSnapshotRequestProto>(packet)
            .map(CasperMessageProto::HasSnapshotRequest),
        "HasSnapshot" => {
            parse_packet::<HasSnapshotProto>(packet).map(CasperMessageProto::HasSnapshot)
        }
        "GetWalPayloadRequest" => parse_packet::<GetWalPayloadRequestProto>(packet)
            .map(CasperMessageProto::GetWalPayloadRequest),
        "WalPayloadResponse" => parse_packet::<WalPayloadResponseProto>(packet)
            .map(CasperMessageProto::WalPayloadResponse),
        "HasWalPayloadRequest" => parse_packet::<HasWalPayloadRequestProto>(packet)
            .map(CasperMessageProto::HasWalPayloadRequest),
        "HasWalPayload" => {
            parse_packet::<HasWalPayloadProto>(packet).map(CasperMessageProto::HasWalPayload)
        }
        _ => PacketParseResult::IllegalPacket(format!("Unrecognized typeId: {}", packet.type_id)),
    }
}

/// Convert CasperMessageProto enum to CasperMessage
pub fn casper_message_from_proto(proto: CasperMessageProto) -> Result<CasperMessage, String> {
    match proto {
        CasperMessageProto::BlockHashMessage(proto) => {
            Ok(CasperMessage::from_block_hash_message(proto))
        }
        CasperMessageProto::BlockMessage(proto) => CasperMessage::from_block_message(proto),
        CasperMessageProto::ApprovedBlock(proto) => CasperMessage::from_approved_block(proto),
        CasperMessageProto::ApprovedBlockRequest(proto) => {
            Ok(CasperMessage::from_approved_block_request(proto))
        }
        CasperMessageProto::BlockRequest(proto) => Ok(CasperMessage::from_block_request(proto)),
        CasperMessageProto::HasBlock(proto) => Ok(CasperMessage::from_has_block(proto)),
        CasperMessageProto::HasBlockRequest(proto) => {
            Ok(CasperMessage::from_has_block_request(proto))
        }
        CasperMessageProto::ForkChoiceTipRequest(proto) => {
            Ok(CasperMessage::from_fork_choice_tip_request(proto))
        }
        CasperMessageProto::BlockApproval(proto) => CasperMessage::from_block_approval(proto),
        CasperMessageProto::UnapprovedBlock(proto) => CasperMessage::from_unapproved_block(proto),
        CasperMessageProto::NoApprovedBlockAvailable(proto) => {
            Ok(CasperMessage::from_no_approved_block_available(proto))
        }
        CasperMessageProto::StoreItemsMessageRequest(proto) => {
            Ok(CasperMessage::from_store_items_message_request(proto))
        }
        CasperMessageProto::StoreItemsMessage(proto) => {
            Ok(CasperMessage::from_store_items_message(proto))
        }
        CasperMessageProto::MergeableEntryRequest(proto) => {
            Ok(CasperMessage::from_mergeable_entry_request(proto))
        }
        CasperMessageProto::FloorCacheRequest(proto) => {
            Ok(CasperMessage::from_floor_cache_request(proto))
        }
        CasperMessageProto::FloorCacheResponse(proto) => {
            Ok(CasperMessage::from_floor_cache_response(proto))
        }
        CasperMessageProto::MergeableEntryResponse(proto) => {
            Ok(CasperMessage::from_mergeable_entry_response(proto))
        }
        CasperMessageProto::GetSnapshotChunkRequest(proto) => {
            Ok(CasperMessage::from_get_snapshot_chunk_request(proto))
        }
        CasperMessageProto::SnapshotChunkResponse(proto) => {
            Ok(CasperMessage::from_snapshot_chunk_response(proto))
        }
        CasperMessageProto::HasSnapshotRequest(proto) => {
            Ok(CasperMessage::from_has_snapshot_request(proto))
        }
        CasperMessageProto::HasSnapshot(proto) => Ok(CasperMessage::from_has_snapshot(proto)),
        CasperMessageProto::GetWalPayloadRequest(proto) => {
            Ok(CasperMessage::from_get_wal_payload_request(proto))
        }
        CasperMessageProto::WalPayloadResponse(proto) => {
            Ok(CasperMessage::from_wal_payload_response(proto))
        }
        CasperMessageProto::HasWalPayloadRequest(proto) => {
            Ok(CasperMessage::from_has_wal_payload_request(proto))
        }
        CasperMessageProto::HasWalPayload(proto) => Ok(CasperMessage::from_has_wal_payload(proto)),
    }
}

// Individual conversion functions for each message type
fn convert_block_hash_message(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<BlockHashMessageProto>(packet).map(CasperMessageProto::BlockHashMessage)
}

fn convert_block_message(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<BlockMessageProto>(packet).map(CasperMessageProto::BlockMessage)
}

fn convert_approved_block(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<ApprovedBlockProto>(packet).map(CasperMessageProto::ApprovedBlock)
}

fn convert_approved_block_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<ApprovedBlockRequestProto>(packet).map(CasperMessageProto::ApprovedBlockRequest)
}

fn convert_block_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<BlockRequestProto>(packet).map(CasperMessageProto::BlockRequest)
}

fn convert_has_block(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasBlockProto>(packet).map(CasperMessageProto::HasBlock)
}

fn convert_has_block_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasBlockRequestProto>(packet).map(CasperMessageProto::HasBlockRequest)
}

fn convert_fork_choice_tip_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<ForkChoiceTipRequestProto>(packet).map(CasperMessageProto::ForkChoiceTipRequest)
}

fn convert_block_approval(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<BlockApprovalProto>(packet).map(CasperMessageProto::BlockApproval)
}

fn convert_unapproved_block(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<UnapprovedBlockProto>(packet).map(CasperMessageProto::UnapprovedBlock)
}

fn convert_no_approved_block_available(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<NoApprovedBlockAvailableProto>(packet)
        .map(CasperMessageProto::NoApprovedBlockAvailable)
}

fn convert_store_items_message_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<StoreItemsMessageRequestProto>(packet)
        .map(CasperMessageProto::StoreItemsMessageRequest)
}

fn convert_store_items_message(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<StoreItemsMessageProto>(packet).map(CasperMessageProto::StoreItemsMessage)
}

fn convert_mergeable_entry_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<MergeableEntryRequestProto>(packet)
        .map(CasperMessageProto::MergeableEntryRequest)
}

fn convert_mergeable_entry_response(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<MergeableEntryResponseProto>(packet)
        .map(CasperMessageProto::MergeableEntryResponse)
}

/// Generic function to parse a packet into a specific protobuf message type
fn parse_packet<T: Message + Default>(packet: &Packet) -> PacketParseResult<T> {
    match T::decode(packet.content.as_ref()) {
        Ok(parsed) => PacketParseResult::Success(parsed),
        Err(e) => PacketParseResult::Failure(format!("Failed to decode packet: {}", e)),
    }
}

/// Specialized verification functions for common message types

/// Verify that a packet is a HasBlockRequest with the expected hash
pub fn verify_has_block_request(packet: &Packet, expected_hash: &BlockHash) -> Result<(), String> {
    if packet.type_id != "HasBlockRequest" {
        return Err(format!("Expected HasBlockRequest, got {}", packet.type_id));
    }

    let has_block_request = parse_packet::<HasBlockRequestProto>(packet).get()?;

    if has_block_request.hash != *expected_hash {
        return Err(format!(
            "Hash mismatch: expected {:?}, got {:?}",
            expected_hash, has_block_request.hash
        ));
    }

    Ok(())
}

/// Verify that a packet is a BlockRequest with the expected hash
pub fn verify_block_request(packet: &Packet, expected_hash: &BlockHash) -> Result<(), String> {
    if packet.type_id != "BlockRequest" {
        return Err(format!("Expected BlockRequest, got {}", packet.type_id));
    }

    let block_request = parse_packet::<BlockRequestProto>(packet).get()?;

    if block_request.hash != *expected_hash {
        return Err(format!(
            "Hash mismatch: expected {:?}, got {:?}",
            expected_hash, block_request.hash
        ));
    }

    Ok(())
}

/// Extract and verify a HasBlockRequest from a Protocol
pub fn extract_and_verify_has_block_request(
    protocol: &Protocol,
    expected_hash: &BlockHash,
) -> Result<HasBlockRequestProto, String> {
    let packet = extract_packet_from_protocol(protocol)?;
    verify_has_block_request(&packet, expected_hash)?;
    parse_packet::<HasBlockRequestProto>(&packet).get()
}

/// Extract and verify a BlockRequest from a Protocol
pub fn extract_and_verify_block_request(
    protocol: &Protocol,
    expected_hash: &BlockHash,
) -> Result<BlockRequestProto, String> {
    let packet = extract_packet_from_protocol(protocol)?;
    verify_block_request(&packet, expected_hash)?;
    parse_packet::<BlockRequestProto>(&packet).get()
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::packet_type_tag::ToPacket;

    use super::*;

    #[test]
    fn test_parse_has_block_request() {
        let hash = BlockHash::from(b"test_hash".to_vec());
        let has_block_request = HasBlockRequestProto { hash: hash.clone() };
        let packet = has_block_request.mk_packet();

        let result = parse_packet::<HasBlockRequestProto>(&packet);
        assert!(result.is_success());

        let parsed = result.get().unwrap();
        assert_eq!(parsed.hash, hash);
    }

    #[test]
    fn test_parse_block_request() {
        let hash = BlockHash::from(b"test_hash".to_vec());
        let block_request = BlockRequestProto { hash: hash.clone() };
        let packet = block_request.mk_packet();

        let result = parse_packet::<BlockRequestProto>(&packet);
        assert!(result.is_success());

        let parsed = result.get().unwrap();
        assert_eq!(parsed.hash, hash);
    }

    #[test]
    fn test_verify_has_block_request() {
        let hash = BlockHash::from(b"test_hash".to_vec());
        let has_block_request = HasBlockRequestProto { hash: hash.clone() };
        let packet = has_block_request.mk_packet();

        let result = verify_has_block_request(&packet, &hash);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_block_request() {
        let hash = BlockHash::from(b"test_hash".to_vec());
        let block_request = BlockRequestProto { hash: hash.clone() };
        let packet = block_request.mk_packet();

        let result = verify_block_request(&packet, &hash);
        assert!(result.is_ok());
    }

    #[test]
    fn test_to_casper_message_proto() {
        let hash = BlockHash::from(b"test_hash".to_vec());
        let has_block_request = HasBlockRequestProto { hash };
        let packet = has_block_request.mk_packet();

        let result = to_casper_message_proto(&packet);
        assert!(result.is_success());

        match result.get().unwrap() {
            CasperMessageProto::HasBlockRequest(_) => {} // Expected
            _ => panic!("Expected HasBlockRequest variant"),
        }
    }

    // --- Snapshot chunk-fetch (Wave 3, Phase 7b-1) --------------
    //
    // Pins the dispatch surface for the three new proto messages:
    // mk_packet carries the correct type_id, parse_packet decodes
    // the content, and to_casper_message_proto routes to the
    // right CasperMessageProto variant.  A regression in any
    // layer (type_id string, missing dispatch arm, or wrong
    // variant mapping) trips one of these tests.

    #[test]
    fn get_snapshot_chunk_request_parse_roundtrip() {
        let proto = GetSnapshotChunkRequestProto {
            block_hash: BlockHash::from(b"block-hash".to_vec()),
            chunk_index: 42,
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "GetSnapshotChunkRequest");

        let parsed = parse_packet::<GetSnapshotChunkRequestProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.block_hash, proto.block_hash);
        assert_eq!(parsed.chunk_index, proto.chunk_index);
    }

    #[test]
    fn snapshot_chunk_response_parse_roundtrip() {
        let proto = SnapshotChunkResponseProto {
            block_hash: BlockHash::from(b"block-hash".to_vec()),
            chunk_index: 7,
            chunk_bytes: prost::bytes::Bytes::from(vec![0xAB; 1024]),
            chunk_hash: prost::bytes::Bytes::from(vec![0xCD; 32]),
            merkle_root: prost::bytes::Bytes::from(vec![0xEF; 32]),
            chunk_count: 16,
            merkle_proof: vec![
                models::casper::MerkleProofStepProto {
                    sibling_hash: prost::bytes::Bytes::from(vec![0x01; 32]),
                    is_sibling_right: true,
                },
                models::casper::MerkleProofStepProto {
                    sibling_hash: prost::bytes::Bytes::from(vec![0x02; 32]),
                    is_sibling_right: false,
                },
            ],
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "SnapshotChunkResponse");

        let parsed = parse_packet::<SnapshotChunkResponseProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.block_hash, proto.block_hash);
        assert_eq!(parsed.chunk_index, proto.chunk_index);
        assert_eq!(parsed.chunk_bytes, proto.chunk_bytes);
        assert_eq!(parsed.chunk_hash, proto.chunk_hash);
        assert_eq!(parsed.merkle_root, proto.merkle_root);
        assert_eq!(parsed.chunk_count, proto.chunk_count);
        assert_eq!(parsed.merkle_proof.len(), 2);
        assert!(parsed.merkle_proof[0].is_sibling_right);
        assert!(!parsed.merkle_proof[1].is_sibling_right);
    }

    #[test]
    fn to_casper_message_proto_dispatches_get_snapshot_chunk_request() {
        let proto = GetSnapshotChunkRequestProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            chunk_index: 1,
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(
            result,
            CasperMessageProto::GetSnapshotChunkRequest(_)
        ));
    }

    #[test]
    fn to_casper_message_proto_dispatches_snapshot_chunk_response() {
        let proto = SnapshotChunkResponseProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            chunk_index: 0,
            chunk_bytes: prost::bytes::Bytes::new(),
            chunk_hash: prost::bytes::Bytes::new(),
            merkle_root: prost::bytes::Bytes::new(),
            chunk_count: 1,
            merkle_proof: Vec::new(),
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(
            result,
            CasperMessageProto::SnapshotChunkResponse(_)
        ));
    }

    /// LOAD-BEARING compat posture: older peers receiving a
    /// Wave-3 type they don't know about return `IllegalPacket`,
    /// NOT a crash.  Pin that unknown type_ids route to the
    /// graceful-skip arm.
    #[test]
    fn unknown_type_id_returns_illegal_packet_not_panic() {
        let packet = models::routing::Packet {
            type_id: "SomeFutureWave4Message".to_string(),
            content: prost::bytes::Bytes::new(),
        };
        match to_casper_message_proto(&packet) {
            PacketParseResult::IllegalPacket(msg) => {
                assert!(msg.contains("SomeFutureWave4Message"))
            }
            other => panic!("expected IllegalPacket, got {other:?}"),
        }
    }

    /// LOAD-BEARING casper_message_from_proto bridge: converts
    /// CasperMessageProto variants to the domain-model
    /// CasperMessage enum.  Pin both new variants.
    #[test]
    fn casper_message_from_proto_bridges_snapshot_chunk_variants() {
        use models::rust::casper::protocol::casper_message::CasperMessage as DomainMessage;
        let req = GetSnapshotChunkRequestProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            chunk_index: 5,
        };
        let domain = casper_message_from_proto(CasperMessageProto::GetSnapshotChunkRequest(req))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::GetSnapshotChunkRequest(_)));

        let resp = SnapshotChunkResponseProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            chunk_index: 5,
            chunk_bytes: prost::bytes::Bytes::new(),
            chunk_hash: prost::bytes::Bytes::new(),
            merkle_root: prost::bytes::Bytes::new(),
            chunk_count: 1,
            merkle_proof: Vec::new(),
        };
        let domain = casper_message_from_proto(CasperMessageProto::SnapshotChunkResponse(resp))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::SnapshotChunkResponse(_)));
    }

    // --- HasSnapshot lookup pair (Wave 3, PR 3.2) ---------------
    //
    // Parallel to the chunk-fetch tests above: pins type_id,
    // parse roundtrip, dispatch, and domain-model bridge for
    // both HasSnapshotRequest and HasSnapshot messages.

    #[test]
    fn has_snapshot_request_parse_roundtrip() {
        let proto = HasSnapshotRequestProto {
            block_hash: BlockHash::from(b"block-hash".to_vec()),
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "HasSnapshotRequest");

        let parsed = parse_packet::<HasSnapshotRequestProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.block_hash, proto.block_hash);
    }

    #[test]
    fn has_snapshot_parse_roundtrip() {
        let proto = HasSnapshotProto {
            block_hash: BlockHash::from(b"block-hash".to_vec()),
            merkle_root: prost::bytes::Bytes::from(vec![0xEF; 32]),
            chunk_count: 16,
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "HasSnapshot");

        let parsed = parse_packet::<HasSnapshotProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.block_hash, proto.block_hash);
        assert_eq!(parsed.merkle_root, proto.merkle_root);
        assert_eq!(parsed.chunk_count, proto.chunk_count);
    }

    #[test]
    fn to_casper_message_proto_dispatches_has_snapshot_request() {
        let proto = HasSnapshotRequestProto {
            block_hash: BlockHash::from(b"h".to_vec()),
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(result, CasperMessageProto::HasSnapshotRequest(_)));
    }

    #[test]
    fn to_casper_message_proto_dispatches_has_snapshot() {
        let proto = HasSnapshotProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            merkle_root: prost::bytes::Bytes::new(),
            chunk_count: 0,
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(result, CasperMessageProto::HasSnapshot(_)));
    }

    #[test]
    fn casper_message_from_proto_bridges_has_snapshot_variants() {
        use models::rust::casper::protocol::casper_message::CasperMessage as DomainMessage;
        let req = HasSnapshotRequestProto {
            block_hash: BlockHash::from(b"h".to_vec()),
        };
        let domain = casper_message_from_proto(CasperMessageProto::HasSnapshotRequest(req))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::HasSnapshotRequest(_)));

        let resp = HasSnapshotProto {
            block_hash: BlockHash::from(b"h".to_vec()),
            merkle_root: prost::bytes::Bytes::new(),
            chunk_count: 3,
        };
        let domain = casper_message_from_proto(CasperMessageProto::HasSnapshot(resp))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::HasSnapshot(_)));
    }

    // --- WAL payload fetch (Wave 3, PR 3.3) --------------------
    //
    // Parallel to the chunk-fetch tests: pin type_id, parse
    // roundtrip, dispatch, and bridge for both new messages.

    #[test]
    fn get_wal_payload_request_parse_roundtrip() {
        let proto = GetWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "GetWalPayloadRequest");

        let parsed = parse_packet::<GetWalPayloadRequestProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.payload_hash, proto.payload_hash);
    }

    #[test]
    fn wal_payload_response_parse_roundtrip() {
        let proto = WalPayloadResponseProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_bytes: prost::bytes::Bytes::from(vec![0xCD; 1024]),
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "WalPayloadResponse");

        let parsed = parse_packet::<WalPayloadResponseProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.payload_hash, proto.payload_hash);
        assert_eq!(parsed.payload_bytes, proto.payload_bytes);
    }

    #[test]
    fn to_casper_message_proto_dispatches_get_wal_payload_request() {
        let proto = GetWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(
            result,
            CasperMessageProto::GetWalPayloadRequest(_)
        ));
    }

    #[test]
    fn to_casper_message_proto_dispatches_wal_payload_response() {
        let proto = WalPayloadResponseProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_bytes: prost::bytes::Bytes::new(),
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(result, CasperMessageProto::WalPayloadResponse(_)));
    }

    #[test]
    fn casper_message_from_proto_bridges_wal_payload_variants() {
        use models::rust::casper::protocol::casper_message::CasperMessage as DomainMessage;
        let req = GetWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let domain = casper_message_from_proto(CasperMessageProto::GetWalPayloadRequest(req))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::GetWalPayloadRequest(_)));

        let resp = WalPayloadResponseProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_bytes: prost::bytes::Bytes::new(),
        };
        let domain = casper_message_from_proto(CasperMessageProto::WalPayloadResponse(resp))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::WalPayloadResponse(_)));
    }

    // --- HasWalPayload lookup pair (Wave 3, PR 3.4) ------------
    //
    // Mirrors HasSnapshot (PR 3.2) but keyed on payload_hash
    // instead of block_hash.  Last of the proto slices.

    #[test]
    fn has_wal_payload_request_parse_roundtrip() {
        let proto = HasWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "HasWalPayloadRequest");

        let parsed = parse_packet::<HasWalPayloadRequestProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.payload_hash, proto.payload_hash);
    }

    #[test]
    fn has_wal_payload_parse_roundtrip() {
        let proto = HasWalPayloadProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_size: 65536,
        };
        let packet = proto.clone().mk_packet();
        assert_eq!(packet.type_id, "HasWalPayload");

        let parsed = parse_packet::<HasWalPayloadProto>(&packet)
            .get()
            .expect("parse must succeed");
        assert_eq!(parsed.payload_hash, proto.payload_hash);
        assert_eq!(parsed.payload_size, proto.payload_size);
    }

    #[test]
    fn to_casper_message_proto_dispatches_has_wal_payload_request() {
        let proto = HasWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(
            result,
            CasperMessageProto::HasWalPayloadRequest(_)
        ));
    }

    #[test]
    fn to_casper_message_proto_dispatches_has_wal_payload() {
        let proto = HasWalPayloadProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_size: 0,
        };
        let packet = proto.mk_packet();
        let result = to_casper_message_proto(&packet)
            .get()
            .expect("dispatch must succeed");
        assert!(matches!(result, CasperMessageProto::HasWalPayload(_)));
    }

    #[test]
    fn casper_message_from_proto_bridges_has_wal_payload_variants() {
        use models::rust::casper::protocol::casper_message::CasperMessage as DomainMessage;
        let req = HasWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let domain = casper_message_from_proto(CasperMessageProto::HasWalPayloadRequest(req))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::HasWalPayloadRequest(_)));

        let resp = HasWalPayloadProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_size: 1024,
        };
        let domain = casper_message_from_proto(CasperMessageProto::HasWalPayload(resp))
            .expect("bridge must succeed");
        assert!(matches!(domain, DomainMessage::HasWalPayload(_)));
    }
}
