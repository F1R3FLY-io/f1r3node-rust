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

        $(impl $type {
            pub const PACKET_TYPE_ID: &'static str = $tag;
        }

        impl ToPacket for $type {
            fn content(&self) -> prost::bytes::Bytes { self.encode_to_vec().into() }

            fn mk_packet(&self) -> Packet {
                Packet {
                    type_id: Self::PACKET_TYPE_ID.to_string(),
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

/// The packet of a block whose `BlockMessageProto` encoding is already in hand.
pub fn encoded_block_message_packet(content: prost::bytes::Bytes) -> Packet {
    Packet {
        type_id: BlockMessageProto::PACKET_TYPE_ID.to_string(),
        content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_block_packet_equals_the_packet_of_the_block() {
        let block = BlockMessageProto {
            block_hash: vec![7; 32].into(),
            sender: vec![3; 65].into(),
            seq_num: 11,
            ..BlockMessageProto::default()
        };
        assert_eq!(
            encoded_block_message_packet(block.encode_to_vec().into()),
            block.mk_packet()
        );
    }
}
