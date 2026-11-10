// See models/src/main/scala/coop/rchain/casper/protocol/PacketTypeTag.scala

use prost::Message;

use crate::casper::{
    ApprovedBlockProto, ApprovedBlockRequestProto, BlockApprovalProto, BlockHashMessageProto,
    BlockMessageProto, BlockRequestProto, FloorCacheRequestProto, FloorCacheResponseProto,
    ForkChoiceTipRequestProto, GetSnapshotChunkRequestProto, GetWalPayloadRequestProto,
    HasBlockProto, HasBlockRequestProto, HasSnapshotProto, HasSnapshotRequestProto,
    HasWalPayloadProto, HasWalPayloadRequestProto, NoApprovedBlockAvailableProto,
    SnapshotChunkResponseProto, StoreItemsMessageProto, StoreItemsMessageRequestProto,
    UnapprovedBlockProto, WalPayloadResponseProto,
};
use crate::routing::Packet;

// Trait for converting to packets
pub trait ToPacket {
    fn content(&self) -> prost::bytes::Bytes;

    fn mk_packet(&self) -> Packet;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    Pull,
    Push,
}

// Macro to implement both traits
#[macro_export]
macro_rules! impl_packet {
    ($(($type:ty, $tag:literal, $class:ident)),+ $(,)?) => {
        pub fn packet_message_class(type_id: &str) -> MessageClass {
            match type_id {
                $($tag => MessageClass::$class,)+
                _ => MessageClass::Push,
            }
        }

        $(impl ToPacket for $type {
            fn content(&self) -> prost::bytes::Bytes { self.encode_to_vec().into() }

            fn mk_packet(&self) -> Packet {
                Packet {
                    type_id: $tag.to_string(),
                    content: self.content(),
                }
            }
        })+
    };
}

// Implement for all message types
impl_packet!(
    (BlockMessageProto, "BlockMessage", Push),
    (BlockHashMessageProto, "BlockHashMessage", Push),
    (ApprovedBlockProto, "ApprovedBlock", Push),
    (UnapprovedBlockProto, "UnapprovedBlock", Push),
    (BlockApprovalProto, "BlockApproval", Push),
    (
        NoApprovedBlockAvailableProto,
        "NoApprovedBlockAvailable",
        Push
    ),
    (BlockRequestProto, "BlockRequest", Pull),
    (ApprovedBlockRequestProto, "ApprovedBlockRequest", Pull),
    (HasBlockRequestProto, "HasBlockRequest", Pull),
    (HasBlockProto, "HasBlock", Push),
    (ForkChoiceTipRequestProto, "ForkChoiceTipRequest", Pull),
    (
        StoreItemsMessageRequestProto,
        "StoreItemsMessageRequest",
        Pull
    ),
    (StoreItemsMessageProto, "StoreItemsMessage", Push),
    (FloorCacheRequestProto, "FloorCacheRequest", Pull),
    (FloorCacheResponseProto, "FloorCacheResponse", Push),
    (
        GetSnapshotChunkRequestProto,
        "GetSnapshotChunkRequest",
        Pull
    ),
    (SnapshotChunkResponseProto, "SnapshotChunkResponse", Push),
    (HasSnapshotRequestProto, "HasSnapshotRequest", Pull),
    (HasSnapshotProto, "HasSnapshot", Push),
    (GetWalPayloadRequestProto, "GetWalPayloadRequest", Pull),
    (WalPayloadResponseProto, "WalPayloadResponse", Push),
    (HasWalPayloadRequestProto, "HasWalPayloadRequest", Pull),
    (HasWalPayloadProto, "HasWalPayload", Push),
);
