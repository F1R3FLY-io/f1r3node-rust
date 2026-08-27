// See models/src/main/scala/coop/rchain/casper/protocol/PacketTypeTag.scala

use prost::Message;

use crate::casper::{
    ApprovedBlockProto, ApprovedBlockRequestProto, BlockApprovalProto, BlockHashMessageProto,
    BlockMessageProto, BlockRequestProto, ForkChoiceTipRequestProto, GetSnapshotChunkRequestProto,
    GetWalPayloadRequestProto, HasBlockProto, HasBlockRequestProto, HasSnapshotProto,
    HasSnapshotRequestProto, HasWalPayloadProto, HasWalPayloadRequestProto,
    MergeableEntryRequestProto, MergeableEntryResponseProto, NoApprovedBlockAvailableProto,
    SnapshotChunkResponseProto, StoreItemsMessageProto, StoreItemsMessageRequestProto,
    UnapprovedBlockProto, WalPayloadResponseProto,
};
use crate::routing::Packet;

// Trait for converting to packets
pub trait ToPacket {
    fn content(&self) -> prost::bytes::Bytes;

    fn mk_packet(&self) -> Packet;
}

// Macro to implement both traits
#[macro_export]
macro_rules! impl_packet {
    ($type:ty, $tag:expr) => {
        impl ToPacket for $type {
            fn content(&self) -> prost::bytes::Bytes { self.encode_to_vec().into() }

            fn mk_packet(&self) -> Packet {
                Packet {
                    type_id: $tag.to_string(),
                    content: self.content(),
                }
            }
        }
    };
}

// Implement for all message types
impl_packet!(BlockMessageProto, "BlockMessage");
impl_packet!(BlockHashMessageProto, "BlockHashMessage");
impl_packet!(ApprovedBlockProto, "ApprovedBlock");
impl_packet!(UnapprovedBlockProto, "UnapprovedBlock");
impl_packet!(BlockApprovalProto, "BlockApproval");
impl_packet!(NoApprovedBlockAvailableProto, "NoApprovedBlockAvailable");
impl_packet!(BlockRequestProto, "BlockRequest");
impl_packet!(ApprovedBlockRequestProto, "ApprovedBlockRequest");
impl_packet!(HasBlockRequestProto, "HasBlockRequest");
impl_packet!(HasBlockProto, "HasBlock");
impl_packet!(ForkChoiceTipRequestProto, "ForkChoiceTipRequest");
impl_packet!(StoreItemsMessageRequestProto, "StoreItemsMessageRequest");
impl_packet!(StoreItemsMessageProto, "StoreItemsMessage");
impl_packet!(MergeableEntryRequestProto, "MergeableEntryRequest");
impl_packet!(MergeableEntryResponseProto, "MergeableEntryResponse");
// Phase 7b-1 snapshot chunk-fetch (2026-08-27).
impl_packet!(GetSnapshotChunkRequestProto, "GetSnapshotChunkRequest");
impl_packet!(SnapshotChunkResponseProto, "SnapshotChunkResponse");
impl_packet!(HasSnapshotRequestProto, "HasSnapshotRequest");
impl_packet!(HasSnapshotProto, "HasSnapshot");
// Phase 7b-2 between-snapshot WAL payload fetch (2026-08-27).
impl_packet!(GetWalPayloadRequestProto, "GetWalPayloadRequest");
impl_packet!(WalPayloadResponseProto, "WalPayloadResponse");
impl_packet!(HasWalPayloadRequestProto, "HasWalPayloadRequest");
impl_packet!(HasWalPayloadProto, "HasWalPayload");
