// See casper/src/main/scala/coop/rchain/casper/protocol/package.scala
// See models/src/main/scala/coop/rchain/casper/protocol/PacketTypeTag.scala

use comm::rust::rp::protocol_helper;
use models::casper::{
    ApprovedBlockProto, ApprovedBlockRequestProto, BlockApprovalProto, BlockHashMessageProto,
    BlockMessageProto, BlockRequestProto, ForkChoiceTipRequestProto, GetSnapshotChunkRequestProto,
    GetWalPayloadRequestProto, HasBlockProto, HasBlockRequestProto, HasSnapshotProto,
    HasSnapshotRequestProto, HasWalPayloadProto, HasWalPayloadRequestProto,
    MergeableEntryRequestProto, MergeableEntryResponseProto, NoApprovedBlockAvailableProto,
    SnapshotChunkResponseProto, StoreItemsMessageProto, StoreItemsMessageRequestProto,
    UnapprovedBlockProto, WalPayloadResponseProto,
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
    // Phase 7b-1 snapshot chunk-fetch (2026-08-27).
    GetSnapshotChunkRequest(GetSnapshotChunkRequestProto),
    SnapshotChunkResponse(SnapshotChunkResponseProto),
    HasSnapshotRequest(HasSnapshotRequestProto),
    HasSnapshot(HasSnapshotProto),
    // Phase 7b-2 between-snapshot WAL payload fetch (2026-08-27).
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
        "GetSnapshotChunkRequest" => convert_get_snapshot_chunk_request(packet),
        "SnapshotChunkResponse" => convert_snapshot_chunk_response(packet),
        "HasSnapshotRequest" => convert_has_snapshot_request(packet),
        "HasSnapshot" => convert_has_snapshot(packet),
        "GetWalPayloadRequest" => convert_get_wal_payload_request(packet),
        "WalPayloadResponse" => convert_wal_payload_response(packet),
        "HasWalPayloadRequest" => convert_has_wal_payload_request(packet),
        "HasWalPayload" => convert_has_wal_payload(packet),
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

fn convert_get_snapshot_chunk_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<GetSnapshotChunkRequestProto>(packet)
        .map(CasperMessageProto::GetSnapshotChunkRequest)
}

fn convert_snapshot_chunk_response(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<SnapshotChunkResponseProto>(packet)
        .map(CasperMessageProto::SnapshotChunkResponse)
}

fn convert_has_snapshot_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasSnapshotRequestProto>(packet).map(CasperMessageProto::HasSnapshotRequest)
}

fn convert_has_snapshot(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasSnapshotProto>(packet).map(CasperMessageProto::HasSnapshot)
}

fn convert_get_wal_payload_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<GetWalPayloadRequestProto>(packet).map(CasperMessageProto::GetWalPayloadRequest)
}

fn convert_wal_payload_response(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<WalPayloadResponseProto>(packet).map(CasperMessageProto::WalPayloadResponse)
}

fn convert_has_wal_payload_request(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasWalPayloadRequestProto>(packet).map(CasperMessageProto::HasWalPayloadRequest)
}

fn convert_has_wal_payload(packet: &Packet) -> PacketParseResult<CasperMessageProto> {
    parse_packet::<HasWalPayloadProto>(packet).map(CasperMessageProto::HasWalPayload)
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
}
