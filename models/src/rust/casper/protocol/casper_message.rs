// See models/src/main/scala/coop/rchain/casper/protocol/CasperMessage.scala

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::{Signed, ToMessage};
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporterInstance;
use shared::rust::{Byte, ByteVector};

use crate::casper::system_deploy_data_proto::SystemDeploy;
use crate::casper::*;
use crate::rhoapi::PCost;
use crate::rust::casper::pretty_printer::PrettyPrinter;
use crate::rust::casper::protocol::legacy_checked_deploy::CheckedLegacyDeploy;
use crate::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use crate::rust::cost_deploy_data::AlgebraAtom;
use crate::rust::cost_protocol_limits::offered_funded_v6_limits;
use crate::rust::deploy_envelope::{DeployEnvelopeFormat, DeployEnvelopeLimits};
use crate::rust::phlo_wire::PhloWireLimits;

// TODO: Use type ByteString from models crate
type ByteString = prost::bytes::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub enum CasperMessage {
    BlockHashMessage(BlockHashMessage),
    BlockMessage(BlockMessage),
    ApprovedBlockCandidate(ApprovedBlockCandidate),
    ApprovedBlock(ApprovedBlock),
    ApprovedBlockRequest(ApprovedBlockRequest),
    BlockApproval(BlockApproval),
    BlockRequest(BlockRequest),
    ForkChoiceTipRequest(ForkChoiceTipRequest),
    HasBlock(HasBlock),
    HasBlockRequest(HasBlockRequest),
    NoApprovedBlockAvailable(NoApprovedBlockAvailable),
    UnapprovedBlock(UnapprovedBlock),
    // Last finalized state messages
    StoreItemsMessageRequest(StoreItemsMessageRequest),
    StoreItemsMessage(StoreItemsMessage),
    MergeableEntryRequest(MergeableEntryRequest),
    MergeableEntryResponse(MergeableEntryResponse),
    FloorCacheRequest(FloorCacheRequest),
    FloorCacheResponse(FloorCacheResponse),
    // Snapshot chunk-fetch (Wave 3, Phase 7b-1).
    GetSnapshotChunkRequest(GetSnapshotChunkRequest),
    SnapshotChunkResponse(SnapshotChunkResponse),
    HasSnapshotRequest(HasSnapshotRequest),
    HasSnapshot(HasSnapshot),
    // Between-snapshot WAL payload fetch (Wave 3, Phase 7b-2).
    GetWalPayloadRequest(GetWalPayloadRequest),
    WalPayloadResponse(WalPayloadResponse),
    HasWalPayloadRequest(HasWalPayloadRequest),
    HasWalPayload(HasWalPayload),
}

impl CasperMessage {
    /// Convert from individual proto message types to CasperMessage
    /// This matches the Scala CasperMessage.from method behavior
    pub fn from_block_hash_message(proto: BlockHashMessageProto) -> Self {
        CasperMessage::BlockHashMessage(BlockHashMessage::from_proto(proto))
    }

    pub fn from_block_message(proto: BlockMessageProto) -> Result<Self, String> {
        Ok(CasperMessage::BlockMessage(BlockMessage::from_proto(
            proto,
        )?))
    }

    pub fn from_approved_block_candidate(
        proto: ApprovedBlockCandidateProto,
    ) -> Result<Self, String> {
        Ok(CasperMessage::ApprovedBlockCandidate(
            ApprovedBlockCandidate::from_proto(proto)?,
        ))
    }

    pub fn from_approved_block(proto: ApprovedBlockProto) -> Result<Self, String> {
        Ok(CasperMessage::ApprovedBlock(ApprovedBlock::from_proto(
            proto,
        )?))
    }

    pub fn from_approved_block_request(proto: ApprovedBlockRequestProto) -> Self {
        CasperMessage::ApprovedBlockRequest(ApprovedBlockRequest::from_proto(proto))
    }

    pub fn from_block_approval(proto: BlockApprovalProto) -> Result<Self, String> {
        Ok(CasperMessage::BlockApproval(BlockApproval::from_proto(
            proto,
        )?))
    }

    pub fn from_block_request(proto: BlockRequestProto) -> Self {
        CasperMessage::BlockRequest(BlockRequest::from_proto(proto))
    }

    pub fn from_fork_choice_tip_request(_proto: ForkChoiceTipRequestProto) -> Self {
        CasperMessage::ForkChoiceTipRequest(ForkChoiceTipRequest)
    }

    pub fn from_has_block(proto: HasBlockProto) -> Self {
        CasperMessage::HasBlock(HasBlock::from_proto(proto))
    }

    pub fn from_has_block_request(proto: HasBlockRequestProto) -> Self {
        CasperMessage::HasBlockRequest(HasBlockRequest::from_proto(proto))
    }

    pub fn from_no_approved_block_available(proto: NoApprovedBlockAvailableProto) -> Self {
        CasperMessage::NoApprovedBlockAvailable(NoApprovedBlockAvailable::from_proto(proto))
    }

    pub fn from_unapproved_block(proto: UnapprovedBlockProto) -> Result<Self, String> {
        Ok(CasperMessage::UnapprovedBlock(UnapprovedBlock::from_proto(
            proto,
        )?))
    }

    pub fn from_store_items_message_request(proto: StoreItemsMessageRequestProto) -> Self {
        CasperMessage::StoreItemsMessageRequest(StoreItemsMessageRequest::from_proto(proto))
    }

    pub fn from_store_items_message(proto: StoreItemsMessageProto) -> Self {
        CasperMessage::StoreItemsMessage(StoreItemsMessage::from_proto(proto))
    }

    pub fn from_mergeable_entry_request(proto: MergeableEntryRequestProto) -> Self {
        CasperMessage::MergeableEntryRequest(MergeableEntryRequest::from_proto(proto))
    }

    pub fn from_mergeable_entry_response(proto: MergeableEntryResponseProto) -> Self {
        CasperMessage::MergeableEntryResponse(MergeableEntryResponse::from_proto(proto))
    }

    pub fn from_floor_cache_request(proto: FloorCacheRequestProto) -> Self {
        CasperMessage::FloorCacheRequest(FloorCacheRequest::from_proto(proto))
    }

    pub fn from_floor_cache_response(proto: FloorCacheResponseProto) -> Self {
        CasperMessage::FloorCacheResponse(FloorCacheResponse::from_proto(proto))
    }

    pub fn from_get_snapshot_chunk_request(proto: GetSnapshotChunkRequestProto) -> Self {
        CasperMessage::GetSnapshotChunkRequest(GetSnapshotChunkRequest::from_proto(proto))
    }

    pub fn from_snapshot_chunk_response(proto: SnapshotChunkResponseProto) -> Self {
        CasperMessage::SnapshotChunkResponse(SnapshotChunkResponse::from_proto(proto))
    }

    pub fn from_has_snapshot_request(proto: HasSnapshotRequestProto) -> Self {
        CasperMessage::HasSnapshotRequest(HasSnapshotRequest::from_proto(proto))
    }

    pub fn from_has_snapshot(proto: HasSnapshotProto) -> Self {
        CasperMessage::HasSnapshot(HasSnapshot::from_proto(proto))
    }

    pub fn from_get_wal_payload_request(proto: GetWalPayloadRequestProto) -> Self {
        CasperMessage::GetWalPayloadRequest(GetWalPayloadRequest::from_proto(proto))
    }

    pub fn from_wal_payload_response(proto: WalPayloadResponseProto) -> Self {
        CasperMessage::WalPayloadResponse(WalPayloadResponse::from_proto(proto))
    }

    pub fn from_has_wal_payload_request(proto: HasWalPayloadRequestProto) -> Self {
        CasperMessage::HasWalPayloadRequest(HasWalPayloadRequest::from_proto(proto))
    }

    pub fn from_has_wal_payload(proto: HasWalPayloadProto) -> Self {
        CasperMessage::HasWalPayload(HasWalPayload::from_proto(proto))
    }
}

// TODO: Remove all into() and to_vec() once we have correct ByteString type in the models crate
#[derive(Clone, Debug, PartialEq)]
pub struct HasBlockRequest {
    pub hash: ByteString,
}

impl HasBlockRequest {
    pub fn from_proto(proto: HasBlockRequestProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> HasBlockRequestProto { HasBlockRequestProto { hash: self.hash } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HasBlock {
    pub hash: ByteString,
}

impl HasBlock {
    pub fn from_proto(proto: HasBlockProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> HasBlockProto { HasBlockProto { hash: self.hash } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockRequest {
    pub hash: ByteString,
}

impl BlockRequest {
    pub fn from_proto(proto: BlockRequestProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> BlockRequestProto { BlockRequestProto { hash: self.hash } }
}

// --- Snapshot chunk-fetch (Wave 3, Phase 7b-1) ---------------
//
// Joiners fetch a finalized snapshot in 4 MiB chunks keyed by
// `(block_hash, chunk_index)`.  The response carries the raw
// chunk bytes, its Blake2b256 hash, the Merkle root anchor over
// all chunk hashes, the total chunk count, and the chunk's
// bottom-up Merkle inclusion proof against the root.  The
// requester rehashes `chunk_bytes` locally and verifies the
// proof against the anchored root before accepting the chunk.

/// One step of a bottom-up Merkle inclusion proof.
/// `is_sibling_right = true` means the sibling is the RIGHT
/// child at this level (concat order: `current || sibling`);
/// `false` means the sibling is the LEFT child (`sibling ||
/// current`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MerkleProofStep {
    pub sibling_hash: ByteString,
    pub is_sibling_right: bool,
}

impl MerkleProofStep {
    pub fn from_proto(proto: MerkleProofStepProto) -> Self {
        Self {
            sibling_hash: proto.sibling_hash,
            is_sibling_right: proto.is_sibling_right,
        }
    }

    pub fn to_proto(self) -> MerkleProofStepProto {
        MerkleProofStepProto {
            sibling_hash: self.sibling_hash,
            is_sibling_right: self.is_sibling_right,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GetSnapshotChunkRequest {
    pub block_hash: ByteString,
    pub chunk_index: u32,
}

impl GetSnapshotChunkRequest {
    pub fn from_proto(proto: GetSnapshotChunkRequestProto) -> Self {
        Self {
            block_hash: proto.block_hash,
            chunk_index: proto.chunk_index,
        }
    }

    pub fn to_proto(self) -> GetSnapshotChunkRequestProto {
        GetSnapshotChunkRequestProto {
            block_hash: self.block_hash,
            chunk_index: self.chunk_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotChunkResponse {
    pub block_hash: ByteString,
    pub chunk_index: u32,
    pub chunk_bytes: ByteString,
    pub chunk_hash: ByteString,
    pub merkle_root: ByteString,
    pub chunk_count: u32,
    pub merkle_proof: Vec<MerkleProofStep>,
}

impl SnapshotChunkResponse {
    pub fn from_proto(proto: SnapshotChunkResponseProto) -> Self {
        Self {
            block_hash: proto.block_hash,
            chunk_index: proto.chunk_index,
            chunk_bytes: proto.chunk_bytes,
            chunk_hash: proto.chunk_hash,
            merkle_root: proto.merkle_root,
            chunk_count: proto.chunk_count,
            merkle_proof: proto
                .merkle_proof
                .into_iter()
                .map(MerkleProofStep::from_proto)
                .collect(),
        }
    }

    pub fn to_proto(self) -> SnapshotChunkResponseProto {
        SnapshotChunkResponseProto {
            block_hash: self.block_hash,
            chunk_index: self.chunk_index,
            chunk_bytes: self.chunk_bytes,
            chunk_hash: self.chunk_hash,
            merkle_root: self.merkle_root,
            chunk_count: self.chunk_count,
            merkle_proof: self
                .merkle_proof
                .into_iter()
                .map(MerkleProofStep::to_proto)
                .collect(),
        }
    }
}

/// Joiner → serving validator: "do you have a serveable
/// snapshot for this block?"  Broadcast + point-to-point;
/// serving nodes reply with [`HasSnapshot`] when they can
/// serve it.  Mirrors the [`HasBlockRequest`] / [`HasBlock`]
/// pattern.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HasSnapshotRequest {
    pub block_hash: ByteString,
}

impl HasSnapshotRequest {
    pub fn from_proto(proto: HasSnapshotRequestProto) -> Self {
        Self {
            block_hash: proto.block_hash,
        }
    }

    pub fn to_proto(self) -> HasSnapshotRequestProto {
        HasSnapshotRequestProto {
            block_hash: self.block_hash,
        }
    }
}

/// Response to [`HasSnapshotRequest`].  Carries `merkle_root`
/// so the joiner can pin an anchor BEFORE issuing per-chunk
/// requests.  Both `merkle_root` and `chunk_count` are
/// ADVISORY — the joiner MUST re-verify `merkle_root` against
/// its locally-anchored root (from block finalization) and
/// expect `chunk_count` to match the actual chunks it later
/// receives via [`GetSnapshotChunkRequest`].  A server
/// reporting an inflated `chunk_count` would be caught by the
/// per-chunk inclusion-proof check during fetch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HasSnapshot {
    pub block_hash: ByteString,
    pub merkle_root: ByteString,
    pub chunk_count: u32,
}

impl HasSnapshot {
    pub fn from_proto(proto: HasSnapshotProto) -> Self {
        Self {
            block_hash: proto.block_hash,
            merkle_root: proto.merkle_root,
            chunk_count: proto.chunk_count,
        }
    }

    pub fn to_proto(self) -> HasSnapshotProto {
        HasSnapshotProto {
            block_hash: self.block_hash,
            merkle_root: self.merkle_root,
            chunk_count: self.chunk_count,
        }
    }
}

/// Joiner → serving validator: "please send the raw bytes for
/// this payload hash."  The requested hash is the content
/// address — the joiner recomputes `Blake2b256(payload_bytes)`
/// against the response and compares with the ORIGINAL
/// request's hash (not the echoed field).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GetWalPayloadRequest {
    pub payload_hash: ByteString,
}

impl GetWalPayloadRequest {
    pub fn from_proto(proto: GetWalPayloadRequestProto) -> Self {
        Self {
            payload_hash: proto.payload_hash,
        }
    }

    pub fn to_proto(self) -> GetWalPayloadRequestProto {
        GetWalPayloadRequestProto {
            payload_hash: self.payload_hash,
        }
    }
}

/// Serving validator → joiner: raw bytes for the requested
/// payload hash.
///
/// # ADVISORY `payload_hash` echo
///
/// The echoed `payload_hash` is a convenience for pipelining
/// (multiple in-flight requests, echo lets the joiner match
/// response → request).  The joiner MUST recompute
/// `Blake2b256(payload_bytes)` and compare against the
/// ORIGINAL request's hash — a server returning mismatched
/// bytes with a correct-looking echoed hash would otherwise
/// skip the real content-integrity check.
///
/// `Hash` is intentionally NOT derived — `payload_bytes` can
/// be up to MAX_PAYLOAD_BYTES (4 MiB) and shouldn't be a
/// HashSet key.  `Eq` IS derived for exact-match comparison
/// in tests + replay-apply correctness checks (where the
/// bytes are already in memory).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalPayloadResponse {
    pub payload_hash: ByteString,
    pub payload_bytes: ByteString,
}

impl WalPayloadResponse {
    pub fn from_proto(proto: WalPayloadResponseProto) -> Self {
        Self {
            payload_hash: proto.payload_hash,
            payload_bytes: proto.payload_bytes,
        }
    }

    pub fn to_proto(self) -> WalPayloadResponseProto {
        WalPayloadResponseProto {
            payload_hash: self.payload_hash,
            payload_bytes: self.payload_bytes,
        }
    }
}

/// Joiner → serving validator: "do you have bytes for this
/// payload hash?"  Broadcast + point-to-point; serving nodes
/// reply with [`HasWalPayload`] when they can serve it.
/// Mirrors the [`HasSnapshotRequest`] / [`HasSnapshot`]
/// pattern on the payload-fetch side.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HasWalPayloadRequest {
    pub payload_hash: ByteString,
}

impl HasWalPayloadRequest {
    pub fn from_proto(proto: HasWalPayloadRequestProto) -> Self {
        Self {
            payload_hash: proto.payload_hash,
        }
    }

    pub fn to_proto(self) -> HasWalPayloadRequestProto {
        HasWalPayloadRequestProto {
            payload_hash: self.payload_hash,
        }
    }
}

/// Response to [`HasWalPayloadRequest`].  Carries `payload_size`
/// so the joiner can budget before issuing the full fetch via
/// [`GetWalPayloadRequest`].
///
/// # ADVISORY `payload_size`
///
/// The joiner MUST cap at `MAX_PAYLOAD_BYTES` (4 MiB) regardless
/// of what the server claims here, and the actual response's
/// byte count is the authoritative truth.  A server over-
/// reporting here would get caught by the comm-layer message-
/// size limit before memory pressure.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HasWalPayload {
    pub payload_hash: ByteString,
    pub payload_size: u32,
}

impl HasWalPayload {
    pub fn from_proto(proto: HasWalPayloadProto) -> Self {
        Self {
            payload_hash: proto.payload_hash,
            payload_size: proto.payload_size,
        }
    }

    pub fn to_proto(self) -> HasWalPayloadProto {
        HasWalPayloadProto {
            payload_hash: self.payload_hash,
            payload_size: self.payload_size,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForkChoiceTipRequest;

impl ForkChoiceTipRequest {
    pub fn to_proto(self) -> ForkChoiceTipRequestProto { ForkChoiceTipRequestProto {} }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedBlockCandidate {
    pub block: BlockMessage,
    pub required_sigs: i32,
}

impl ApprovedBlockCandidate {
    pub fn from_proto(proto: ApprovedBlockCandidateProto) -> Result<Self, String> {
        Ok(Self {
            block: BlockMessage::from_proto(
                proto
                    .block
                    .ok_or_else(|| "Missing block field".to_string())?,
            )?,
            required_sigs: proto.required_sigs,
        })
    }

    pub fn to_proto(self) -> ApprovedBlockCandidateProto {
        ApprovedBlockCandidateProto {
            block: Some(self.block.to_proto()),
            required_sigs: self.required_sigs,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnapprovedBlock {
    pub candidate: ApprovedBlockCandidate,
    pub timestamp: i64,
    pub duration: i64,
}

impl UnapprovedBlock {
    pub fn from_proto(proto: UnapprovedBlockProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            timestamp: proto.timestamp,
            duration: proto.duration,
        })
    }

    pub fn to_proto(self) -> UnapprovedBlockProto {
        UnapprovedBlockProto {
            candidate: Some(self.candidate.to_proto()),
            timestamp: self.timestamp,
            duration: self.duration,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockApproval {
    pub candidate: ApprovedBlockCandidate,
    pub sig: Signature,
}

impl BlockApproval {
    pub fn from_proto(proto: BlockApprovalProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            sig: proto.sig.ok_or_else(|| "Missing sig field".to_string())?,
        })
    }

    pub fn to_proto(self) -> BlockApprovalProto {
        BlockApprovalProto {
            candidate: Some(self.candidate.to_proto()),
            sig: Some(self.sig),
        }
    }
}

/// Ask a peer for its cached finalized-floor values for the named blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheRequest {
    pub hashes: Vec<ByteString>,
}

impl FloorCacheRequest {
    pub fn from_proto(proto: FloorCacheRequestProto) -> Self {
        Self {
            hashes: proto.hashes,
        }
    }

    pub fn to_proto(self) -> FloorCacheRequestProto {
        FloorCacheRequestProto {
            hashes: self.hashes,
        }
    }
}

/// One block's cached floor and frontier, as the responder derived them when
/// it validated the block.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheEntry {
    pub block_hash: ByteString,
    pub floor_hash: ByteString,
    pub frontier_hash: ByteString,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheResponse {
    pub entries: Vec<FloorCacheEntry>,
    /// The shard's genesis block hash; empty when the responder does not
    /// hold it.
    pub genesis_hash: ByteString,
    /// The genesis block itself; absent when the responder does not hold
    /// it. Verified against `genesis_hash` by the receiver before storing.
    pub genesis_block: Option<BlockMessage>,
}

impl FloorCacheResponse {
    pub fn from_proto(proto: FloorCacheResponseProto) -> Self {
        Self {
            entries: proto
                .entries
                .into_iter()
                .map(|entry| FloorCacheEntry {
                    block_hash: entry.block_hash,
                    floor_hash: entry.floor_hash,
                    frontier_hash: entry.frontier_hash,
                })
                .collect(),
            genesis_hash: proto.genesis_hash,
            genesis_block: proto
                .genesis_block
                .and_then(|block| BlockMessage::from_proto(block).ok()),
        }
    }

    pub fn to_proto(self) -> FloorCacheResponseProto {
        FloorCacheResponseProto {
            entries: self
                .entries
                .into_iter()
                .map(|entry| FloorCacheEntryProto {
                    block_hash: entry.block_hash,
                    floor_hash: entry.floor_hash,
                    frontier_hash: entry.frontier_hash,
                })
                .collect(),
            genesis_hash: self.genesis_hash,
            genesis_block: self.genesis_block.map(|block| block.to_proto()),
        }
    }
}

/// The anchor's finalized floor and frontier, carried with the approved block
/// so a restored node can start deriving forward from them.
///
/// Each block is named by hash AND number: the number sizes the receiver's
/// download window, which it must fix before it holds any block to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizedFloorSeed {
    pub floor_hash: ByteString,
    pub floor_number: i64,
    pub frontier_hash: ByteString,
    pub frontier_number: i64,
}

impl FinalizedFloorSeed {
    pub fn from_proto(proto: FinalizedFloorSeedProto) -> Self {
        Self {
            floor_hash: proto.floor_hash,
            floor_number: proto.floor_number,
            frontier_hash: proto.frontier_hash,
            frontier_number: proto.frontier_number,
        }
    }

    pub fn to_proto(self) -> FinalizedFloorSeedProto {
        FinalizedFloorSeedProto {
            floor_hash: self.floor_hash,
            floor_number: self.floor_number,
            frontier_hash: self.frontier_hash,
            frontier_number: self.frontier_number,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedBlock {
    pub candidate: ApprovedBlockCandidate,
    pub sigs: Vec<Signature>,
    /// Absent from peers that predate the seed, and from the non-trim response
    /// (a node syncing from genesis derives its own floors).
    pub floor_seed: Option<FinalizedFloorSeed>,
}

impl ApprovedBlock {
    pub fn from_proto(proto: ApprovedBlockProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            sigs: proto.sigs,
            floor_seed: proto.floor_seed.map(FinalizedFloorSeed::from_proto),
        })
    }

    pub fn to_proto(self) -> ApprovedBlockProto {
        ApprovedBlockProto {
            candidate: Some(self.candidate.to_proto()),
            sigs: self.sigs,
            floor_seed: self.floor_seed.map(FinalizedFloorSeed::to_proto),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoApprovedBlockAvailable {
    pub identifier: String,
    pub node_identifier: String,
}

impl NoApprovedBlockAvailable {
    pub fn from_proto(proto: NoApprovedBlockAvailableProto) -> Self {
        Self {
            identifier: proto.identifier,
            node_identifier: proto.node_identifier,
        }
    }

    pub fn to_proto(self) -> NoApprovedBlockAvailableProto {
        NoApprovedBlockAvailableProto {
            identifier: self.identifier,
            node_identifier: self.node_identifier,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApprovedBlockRequest {
    pub identifier: String,
    pub trim_state: bool,
}

impl ApprovedBlockRequest {
    pub fn from_proto(proto: ApprovedBlockRequestProto) -> Self {
        Self {
            identifier: proto.identifier,
            trim_state: proto.trim_state,
        }
    }

    pub fn to_proto(self) -> ApprovedBlockRequestProto {
        ApprovedBlockRequestProto {
            identifier: self.identifier,
            trim_state: self.trim_state,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockHashMessage {
    pub block_hash: ByteString,
    pub block_creator: ByteString,
}

impl BlockHashMessage {
    pub fn from_proto(proto: BlockHashMessageProto) -> Self {
        Self {
            block_hash: proto.hash,
            block_creator: proto.block_creator,
        }
    }

    pub fn to_proto(self) -> BlockHashMessageProto {
        BlockHashMessageProto {
            hash: self.block_hash,
            block_creator: self.block_creator,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockMessage {
    pub block_hash: ByteString,
    pub header: Header,
    pub body: Body,
    pub justifications: Vec<Justification>,
    pub sender: ByteString,
    pub seq_num: i32,
    pub sig: ByteString,
    pub sig_algorithm: String,
    pub shard_id: String,
    pub extra_bytes: ByteString,
}

impl BlockMessage {
    pub fn from_proto(proto: BlockMessageProto) -> Result<Self, String> {
        Self::from_proto_using(proto, Body::from_proto)
    }

    pub fn from_proto_with_limits(
        proto: BlockMessageProto,
        envelope_limits: DeployEnvelopeLimits,
        evidence_limits: PhloWireLimits,
    ) -> Result<Self, String> {
        Self::from_proto_using(proto, |body| {
            Body::from_proto_with_limits(body, envelope_limits, evidence_limits)
        })
    }

    fn from_proto_using(
        proto: BlockMessageProto,
        decode_body: impl FnOnce(BodyProto) -> Result<Body, String>,
    ) -> Result<Self, String> {
        Ok(Self {
            block_hash: proto.block_hash,
            header: Header::from_proto(
                proto
                    .header
                    .ok_or_else(|| "Missing header field".to_string())?,
            ),
            body: decode_body(proto.body.ok_or_else(|| "Missing body field".to_string())?)?,
            justifications: proto
                .justifications
                .into_iter()
                .map(|j| Justification::from_proto(j))
                .collect(),
            sender: proto.sender,
            seq_num: proto.seq_num,
            sig: proto.sig,
            sig_algorithm: proto.sig_algorithm,
            shard_id: proto.shard_id,
            extra_bytes: proto.extra_bytes,
        })
    }

    pub fn to_proto(&self) -> BlockMessageProto {
        BlockMessageProto {
            block_hash: self.block_hash.clone(),
            header: Some(self.header.to_proto()),
            body: Some(self.body.to_proto()),
            justifications: self
                .justifications
                .clone()
                .into_iter()
                .map(|j| j.to_proto())
                .collect(),
            sender: self.sender.clone(),
            seq_num: self.seq_num,
            sig: self.sig.clone(),
            sig_algorithm: self.sig_algorithm.clone(),
            shard_id: self.shard_id.clone(),
            extra_bytes: self.extra_bytes.clone(),
        }
    }

    pub fn to_string(self) -> String {
        PrettyPrinter::build_string(CasperMessage::BlockMessage(self), false)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub parents_hash_list: Vec<ByteString>,
    pub timestamp: i64,
    pub version: i64,
    pub extra_bytes: ByteString,
}

impl Header {
    pub fn from_proto(proto: HeaderProto) -> Self {
        Self {
            parents_hash_list: proto.parents_hash_list,
            timestamp: proto.timestamp,
            version: proto.version,
            extra_bytes: proto.extra_bytes,
        }
    }

    pub fn to_proto(&self) -> HeaderProto {
        HeaderProto {
            parents_hash_list: self.parents_hash_list.clone(),
            timestamp: self.timestamp,
            version: self.version,
            extra_bytes: self.extra_bytes.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RejectedDeploy {
    pub sig: ByteString,
    /// Set by the merge that formed this record when the sig's effect is
    /// present in that merge's own post-state (a kept copy in the same
    /// merge, or already settled in its base): the record discarded a
    /// redundant copy and does not dispute the sig's standing win.
    pub duplicate: bool,
    /// The block that held the copy this record adjudicated — the rejected
    /// chain's source block, recorded by the forming merge.
    pub carrier: ByteString,
}

impl RejectedDeploy {
    pub fn from_proto(proto: RejectedDeployProto) -> Self {
        Self {
            sig: proto.sig,
            duplicate: proto.duplicate,
            carrier: proto.carrier,
        }
    }

    pub fn to_proto(self) -> RejectedDeployProto {
        RejectedDeployProto {
            sig: self.sig,
            duplicate: self.duplicate,
            carrier: self.carrier,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub state: F1r3flyState,
    pub deploys: Vec<ProcessedUserDeploy>,
    pub rejected_deploys: Vec<RejectedDeploy>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
    pub extra_bytes: ByteString,
    pub applied_from_scope: Vec<ByteString>,
    pub merge_base: ByteString,
}

impl Body {
    pub fn from_proto(proto: BodyProto) -> Result<Self, String> {
        Self::from_proto_using(proto, ProcessedUserDeploy::from_proto)
    }

    pub fn from_proto_with_limits(
        proto: BodyProto,
        envelope_limits: DeployEnvelopeLimits,
        evidence_limits: PhloWireLimits,
    ) -> Result<Self, String> {
        Self::from_proto_using(proto, |deploy| {
            ProcessedUserDeploy::from_proto_with_limits(deploy, envelope_limits, evidence_limits)
        })
    }

    fn from_proto_using(
        proto: BodyProto,
        decode: impl FnMut(ProcessedDeployProto) -> Result<ProcessedUserDeploy, String>,
    ) -> Result<Self, String> {
        Ok(Self {
            state: F1r3flyState::from_proto(
                proto
                    .state
                    .ok_or_else(|| "Missing state field".to_string())?,
            ),
            deploys: proto
                .deploys
                .into_iter()
                .map(decode)
                .collect::<Result<Vec<_>, String>>()?,
            rejected_deploys: proto
                .rejected_deploys
                .into_iter()
                .map(|r| RejectedDeploy::from_proto(r))
                .collect(),
            system_deploys: proto
                .system_deploys
                .into_iter()
                .map(|s| ProcessedSystemDeploy::from_proto(s))
                .collect::<Result<Vec<ProcessedSystemDeploy>, String>>()?,
            extra_bytes: proto.extra_bytes,
            applied_from_scope: proto.applied_from_scope,
            merge_base: proto.merge_base,
        })
    }

    pub fn to_proto(&self) -> BodyProto {
        BodyProto {
            state: Some(self.state.to_proto()),
            deploys: self
                .deploys
                .clone()
                .into_iter()
                .map(|d| d.to_proto())
                .collect(),
            rejected_deploys: self
                .rejected_deploys
                .clone()
                .into_iter()
                .map(|r| r.to_proto())
                .collect(),
            system_deploys: self
                .system_deploys
                .clone()
                .into_iter()
                .map(|s| s.to_proto())
                .collect(),
            extra_bytes: self.extra_bytes.clone(),
            applied_from_scope: self.applied_from_scope.clone(),
            merge_base: self.merge_base.clone(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Eq,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Hash
)]
pub struct Justification {
    #[serde(with = "shared::rust::serde_bytes")]
    pub validator: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub latest_block_hash: ByteString,
}

impl Justification {
    pub fn from_proto(proto: JustificationProto) -> Self {
        Self {
            validator: proto.validator,
            latest_block_hash: proto.latest_block_hash,
        }
    }

    pub fn to_proto(&self) -> JustificationProto {
        JustificationProto {
            validator: self.validator.clone(),
            latest_block_hash: self.latest_block_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct F1r3flyState {
    pub pre_state_hash: ByteString,
    pub post_state_hash: ByteString,
    pub bonds: Vec<Bond>,
    pub block_number: i64,
}

impl F1r3flyState {
    pub fn from_proto(proto: RChainStateProto) -> Self {
        Self {
            pre_state_hash: proto.pre_state_hash,
            post_state_hash: proto.post_state_hash,
            bonds: proto
                .bonds
                .into_iter()
                .map(|b| Bond::from_proto(b))
                .collect(),
            block_number: proto.block_number,
        }
    }

    pub fn to_proto(&self) -> RChainStateProto {
        RChainStateProto {
            pre_state_hash: self.pre_state_hash.clone(),
            post_state_hash: self.post_state_hash.clone(),
            bonds: self
                .bonds
                .clone()
                .into_iter()
                .map(|b| Bond::to_proto(b))
                .collect(),
            block_number: self.block_number,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessedDeploy {
    envelope: CheckedLegacyDeploy,
    pub cost: PCost,
    pub deploy_log: Vec<Event>,
    pub is_failed: bool,
    pub system_deploy_error: Option<String>,
}

impl ProcessedDeploy {
    pub fn refund_amount(&self) -> i64 {
        (self.body().phlo_limit - self.cost.cost as i64).max(0) * self.body().phlo_price
    }

    pub fn from_checked_envelope(envelope: CheckedLegacyDeploy) -> Self {
        Self {
            envelope,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
        }
    }

    pub fn empty(deploy: Signed<DeployData>) -> Result<Self, String> {
        CheckedLegacyDeploy::from_signed(deploy).map(Self::from_checked_envelope)
    }

    pub fn body(&self) -> &DeployData { self.envelope.body() }

    pub fn primary(&self) -> &crypto::rust::signatures::signed::Cosigner { self.envelope.primary() }

    pub fn signers(&self) -> &[crypto::rust::signatures::signed::Cosigner] {
        self.envelope.signers()
    }

    pub fn threshold(&self) -> u32 { self.envelope.threshold() }

    pub fn wire_threshold(&self) -> i32 { self.envelope.wire_threshold() }

    pub fn identity_bytes(&self) -> &[u8] { self.envelope.identity_bytes() }

    pub fn checked_envelope(&self) -> &CheckedLegacyDeploy { &self.envelope }

    /// A legacy execution-body view, only available after the complete
    /// processed deploy's flat signer list has been checked.
    pub fn legacy_execution_projection(&self) -> Signed<DeployData> {
        self.envelope.execution_projection()
    }

    /// Restore the complete checked authority after historical execution,
    /// which accepts only the primary `Signed` body context. Reject a receipt
    /// that does not describe exactly that authorized body and primary signer.
    pub fn with_checked_envelope(mut self, envelope: CheckedLegacyDeploy) -> Result<Self, String> {
        if self.body() != envelope.body() || self.primary() != envelope.primary() {
            return Err("legacy execution receipt does not match checked envelope".to_string());
        }
        self.envelope = envelope;
        Ok(self)
    }

    pub fn to_deploy_info(self) -> DeployInfo {
        DeployInfo {
            deployer: PrettyPrinter::build_string_no_limit(&self.primary().pk.bytes),
            term: self.body().term.clone(),
            timestamp: self.body().time_stamp,
            sig: PrettyPrinter::build_string_no_limit(&self.primary().sig),
            sig_algorithm: self.primary().sig_algorithm.name(),
            phlo_price: self.body().phlo_price,
            phlo_limit: self.body().phlo_limit,
            valid_after_block_number: self.body().valid_after_block_number,
            cost: self.cost.cost,
            errored: self.is_failed,
            system_deploy_error: self.system_deploy_error.unwrap_or_default(),
            transfers: Vec::new(),
            transfers_available: false,
        }
    }

    pub fn from_proto(proto: ProcessedDeployProto) -> Result<Self, String> {
        if proto.native_cost_evidence.is_some() {
            return Err(
                "native cost evidence requires the funded processed-deploy decoder".to_string(),
            );
        }
        if proto.deploy_log.iter().any(|event| matches!(
            event.event_instance.as_ref(),
            Some(event_proto::EventInstance::RestingComm(_))
        )) {
            return Err("legacy processed deploy cannot contain a CostV2 resting event".to_string());
        }
        Ok(Self {
            envelope: CheckedLegacyDeploy::from_processed_proto(
                proto
                    .deploy
                    .ok_or_else(|| "Missing deploy field".to_string())?,
            )?,
            cost: proto.cost.ok_or_else(|| "Missing cost field".to_string())?,
            deploy_log: proto
                .deploy_log
                .into_iter()
                .map(|e| Event::from_proto(e))
                .collect::<Result<Vec<Event>, String>>()?,
            is_failed: proto.errored,
            system_deploy_error: {
                if proto.system_deploy_error.is_empty() {
                    None
                } else {
                    Some(proto.system_deploy_error)
                }
            },
        })
    }

    pub fn to_proto(self) -> ProcessedDeployProto {
        ProcessedDeployProto {
            deploy: Some(self.envelope.to_proto()),
            cost: Some(self.cost),
            deploy_log: self.deploy_log.into_iter().map(|e| e.to_proto()).collect(),
            errored: self.is_failed,
            system_deploy_error: self.system_deploy_error.unwrap_or_default(),
            native_cost_evidence: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProcessedUserDeploy {
    Legacy(ProcessedDeploy),
    Offered(OfferedProcessedDeploy),
}

impl ProcessedUserDeploy {
    pub fn from_proto(proto: ProcessedDeployProto) -> Result<Self, String> {
        let version = proto
            .deploy
            .as_ref()
            .and_then(|deploy| deploy.authorization_v61.as_ref())
            .map(|authorization| authorization.format_version);
        match DeployEnvelopeFormat::from_authorization_version(version)? {
            DeployEnvelopeFormat::Legacy => ProcessedDeploy::from_proto(proto).map(Self::Legacy),
            DeployEnvelopeFormat::OfferedFunded => {
                let limits = offered_funded_v6_limits();
                OfferedProcessedDeploy::from_proto(proto, limits.envelope, limits.evidence)
                    .map(Self::Offered)
            }
            _ => Err("processed deploy authorization format is not active".to_string()),
        }
    }

    pub fn from_proto_with_limits(
        proto: ProcessedDeployProto,
        envelope_limits: DeployEnvelopeLimits,
        evidence_limits: PhloWireLimits,
    ) -> Result<Self, String> {
        let version = proto
            .deploy
            .as_ref()
            .and_then(|deploy| deploy.authorization_v61.as_ref())
            .map(|authorization| authorization.format_version);
        match DeployEnvelopeFormat::from_authorization_version(version)? {
            DeployEnvelopeFormat::Legacy => ProcessedDeploy::from_proto(proto).map(Self::Legacy),
            DeployEnvelopeFormat::OfferedFunded => {
                OfferedProcessedDeploy::from_proto(proto, envelope_limits, evidence_limits)
                    .map(Self::Offered)
            }
            _ => Err("processed deploy authorization format is not active".to_string()),
        }
    }

    pub fn to_proto(&self) -> ProcessedDeployProto {
        match self {
            Self::Legacy(deploy) => deploy.clone().to_proto(),
            Self::Offered(deploy) => deploy.to_proto_verified(),
        }
    }

    pub fn identity_bytes(&self) -> &[u8] {
        match self {
            Self::Legacy(deploy) => deploy.identity_bytes(),
            Self::Offered(deploy) => deploy.identity_bytes(),
        }
    }

    pub fn valid_after_block_number(&self) -> i64 {
        match self {
            Self::Legacy(deploy) => deploy.body().valid_after_block_number,
            Self::Offered(deploy) => deploy.envelope().body().valid_after_block_number,
        }
    }

    pub fn shard_id(&self) -> &str {
        match self {
            Self::Legacy(deploy) => &deploy.body().shard_id,
            Self::Offered(deploy) => &deploy.envelope().body().shard_id,
        }
    }

    pub fn phlo_price(&self) -> i64 {
        match self {
            Self::Legacy(deploy) => deploy.body().phlo_price,
            Self::Offered(deploy) => {
                let crate::rust::deploy_envelope::DeployEnvelopeRef::OfferedFunded(signed) =
                    deploy.envelope().view()
                else {
                    unreachable!("verified offered processed deploy")
                };
                signed.data.phlo_price()
            }
        }
    }

    pub fn phlo_limit(&self) -> i64 {
        match self {
            Self::Legacy(deploy) => deploy.body().phlo_limit,
            Self::Offered(deploy) => {
                let crate::rust::deploy_envelope::DeployEnvelopeRef::OfferedFunded(signed) =
                    deploy.envelope().view()
                else {
                    unreachable!("verified offered processed deploy")
                };
                signed.data.phlo_limit()
            }
        }
    }

    pub fn term(&self) -> &str {
        match self {
            Self::Legacy(deploy) => &deploy.body().term,
            Self::Offered(deploy) => &deploy.envelope().body().term,
        }
    }

    pub fn expiration_timestamp(&self) -> Option<i64> {
        match self {
            Self::Legacy(deploy) => deploy.body().expiration_timestamp,
            Self::Offered(deploy) => deploy.envelope().body().expiration_timestamp,
        }
    }

    pub fn timestamp(&self) -> i64 {
        match self {
            Self::Legacy(deploy) => deploy.body().time_stamp,
            Self::Offered(deploy) => deploy.envelope().body().time_stamp,
        }
    }

    pub fn deployer_bytes(&self) -> &[u8] {
        match self {
            Self::Legacy(deploy) => &deploy.primary().pk.bytes,
            Self::Offered(deploy) => &deploy.envelope().primary().pk.bytes,
        }
    }

    pub fn to_deploy_info(&self) -> DeployInfo {
        match self {
            Self::Legacy(deploy) => deploy.clone().to_deploy_info(),
            Self::Offered(deploy) => {
                let body = deploy.envelope().body();
                DeployInfo {
                    deployer: PrettyPrinter::build_string_no_limit(self.deployer_bytes()),
                    term: body.term.clone(),
                    timestamp: body.time_stamp,
                    sig: PrettyPrinter::build_string_no_limit(self.identity_bytes()),
                    sig_algorithm: deploy.envelope().primary().sig_algorithm.name(),
                    phlo_price: self.phlo_price(),
                    phlo_limit: self.phlo_limit(),
                    valid_after_block_number: body.valid_after_block_number,
                    cost: deploy.cost().cost,
                    errored: deploy.is_failed(),
                    system_deploy_error: String::new(),
                    transfers: Vec::new(),
                    transfers_available: false,
                }
            }
        }
    }

    pub fn deploy_log(&self) -> &[Event] {
        match self {
            Self::Legacy(deploy) => &deploy.deploy_log,
            Self::Offered(deploy) => deploy.deploy_log(),
        }
    }

    pub fn cost(&self) -> &PCost {
        match self {
            Self::Legacy(deploy) => &deploy.cost,
            Self::Offered(deploy) => deploy.cost(),
        }
    }

    pub fn is_failed(&self) -> bool {
        match self {
            Self::Legacy(deploy) => deploy.is_failed,
            Self::Offered(deploy) => deploy.is_failed(),
        }
    }

    pub fn as_legacy(&self) -> Option<&ProcessedDeploy> {
        match self {
            Self::Legacy(deploy) => Some(deploy),
            Self::Offered(_) => None,
        }
    }

    pub fn as_offered(&self) -> Option<&OfferedProcessedDeploy> {
        match self {
            Self::Legacy(_) => None,
            Self::Offered(deploy) => Some(deploy),
        }
    }
}

impl From<ProcessedDeploy> for ProcessedUserDeploy {
    fn from(value: ProcessedDeploy) -> Self { Self::Legacy(value) }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemDeployData {
    Slash {
        invalid_block_hash: ByteString,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
    },
    CloseBlockSystemDeployData,
    Empty,
}

impl SystemDeployData {
    pub fn create_slash(
        invalid_block_hash: ByteString,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
    ) -> Self {
        Self::Slash {
            invalid_block_hash,
            issuer_public_key,
            target_activation_epoch,
        }
    }

    pub fn create_close() -> Self { Self::CloseBlockSystemDeployData }

    pub fn from_proto(proto: SystemDeployDataProto) -> Result<Self, String> {
        match proto
            .system_deploy
            .ok_or_else(|| "Missing system deploy field".to_string())?
        {
            system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                slash_system_deploy_data_proto,
            ) => Ok(Self::Slash {
                invalid_block_hash: slash_system_deploy_data_proto.invalid_block_hash,
                issuer_public_key: PublicKey::from_bytes(
                    &slash_system_deploy_data_proto.issuer_public_key,
                ),
                target_activation_epoch: slash_system_deploy_data_proto.target_activation_epoch,
            }),
            system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(_) => {
                Ok(Self::CloseBlockSystemDeployData)
            }
        }
    }

    pub fn to_proto(sdd: SystemDeployData) -> SystemDeployDataProto {
        match sdd {
            Self::Slash {
                invalid_block_hash,
                issuer_public_key,
                target_activation_epoch,
            } => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::SlashSystemDeploy(
                    SlashSystemDeployDataProto {
                        invalid_block_hash,
                        issuer_public_key: issuer_public_key.bytes.into(),
                        target_activation_epoch,
                    },
                )),
            },
            Self::CloseBlockSystemDeployData => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::CloseBlockSystemDeploy(
                    CloseBlockSystemDeployDataProto {},
                )),
            },
            Self::Empty => SystemDeployDataProto {
                system_deploy: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessedSystemDeploy {
    Succeeded {
        event_list: Vec<Event>,
        system_deploy: SystemDeployData,
    },
    Failed {
        event_list: Vec<Event>,
        error_msg: String,
    },
}

impl ProcessedSystemDeploy {
    pub fn failed(self) -> bool { matches!(self, ProcessedSystemDeploy::Failed { .. }) }

    pub fn fold<A, F, G>(self, if_succeeded: F, if_failed: G) -> A
    where
        F: Fn(Vec<Event>) -> A,
        G: Fn(Vec<Event>, String) -> A,
    {
        match self {
            ProcessedSystemDeploy::Succeeded { event_list, .. } => if_succeeded(event_list),
            ProcessedSystemDeploy::Failed {
                event_list,
                error_msg,
            } => if_failed(event_list, error_msg),
        }
    }

    pub fn from_proto(psd: ProcessedSystemDeployProto) -> Result<Self, String> {
        let deploy_log: Result<Vec<Event>, String> =
            psd.deploy_log.into_iter().map(Event::from_proto).collect();

        match deploy_log {
            Ok(deploy_log) => {
                if psd.error_msg.is_empty() {
                    Ok(ProcessedSystemDeploy::Succeeded {
                        event_list: deploy_log,
                        system_deploy: SystemDeployData::from_proto(
                            psd.system_deploy
                                .ok_or_else(|| "Missing system deploy field".to_string())?,
                        )?,
                    })
                } else {
                    Ok(ProcessedSystemDeploy::Failed {
                        event_list: deploy_log,
                        error_msg: psd.error_msg,
                    })
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn to_proto(self) -> ProcessedSystemDeployProto {
        match self {
            ProcessedSystemDeploy::Succeeded {
                event_list,
                system_deploy,
            } => ProcessedSystemDeployProto {
                system_deploy: Some(SystemDeployData::to_proto(system_deploy)),
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg: "".to_string(),
            },
            ProcessedSystemDeploy::Failed {
                event_list,
                error_msg,
            } => ProcessedSystemDeployProto {
                system_deploy: None,
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg,
            },
        }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Eq,
    Hash,
    utoipa::ToSchema
)]
pub struct DeployData {
    pub term: String,
    #[serde(rename = "timestamp")]
    pub time_stamp: i64,
    #[serde(rename = "phloPrice")]
    pub phlo_price: i64,
    #[serde(rename = "phloLimit")]
    pub phlo_limit: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    /// Optional millisecond timestamp after which deploy is invalid (None = no expiration)
    pub expiration_timestamp: Option<i64>,
}

impl ToMessage for DeployData {
    type Type = DeployDataProto;
    fn to_message(&self) -> Self::Type { DeployData::_to_proto(self.clone()) }
    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> {
        Err("legacy Casper deploys do not support protocol-v6 envelopes".to_string())
    }
}

/// Detach the recursive children of one signature-algebra connective and move
/// them onto the explicit teardown stack.
fn take_sig_compound_children(
    connective: crate::casper::sig_compound::Connective,
    work: &mut Vec<crate::casper::SigCompound>,
) {
    use crate::casper::sig_compound::Connective;
    match connective {
        Connective::Atom(_) => {}
        Connective::Tensor(mut pair) | Connective::With(mut pair) => {
            work.extend(pair.left.take().map(|child| *child));
            work.extend(pair.right.take().map(|child| *child));
        }
        Connective::Plus(mut plus) => {
            work.extend(plus.left.take().map(|child| *child));
            work.extend(plus.right.take().map(|child| *child));
        }
        Connective::Bang(mut bang) => {
            work.extend(bang.inner.take().map(|child| *child));
        }
        Connective::Whynot(child) => work.push(*child),
        Connective::Lolly(mut lolly) => {
            work.extend(lolly.from.take().map(|child| *child));
            work.extend(lolly.to.take().map(|child| *child));
        }
        Connective::Threshold(mut threshold) => work.append(&mut threshold.members),
    }
}

/// The generated protobuf shape is recursively boxed, so rustc's default drop
/// glue consumes one native frame per algebra level. Detaching every child
/// before its shell drops turns teardown into the same O(depth) heap worklist
/// used by the ingress analyzer and keeps native stack use O(1).
impl Drop for crate::casper::SigCompound {
    fn drop(&mut self) {
        let mut work = Vec::new();
        if let Some(connective) = self.connective.take() {
            take_sig_compound_children(connective, &mut work);
        }
        while let Some(mut compound) = work.pop() {
            if let Some(connective) = compound.connective.take() {
                take_sig_compound_children(connective, &mut work);
            }
            // `compound` now has no recursive child; its own `Drop` is bounded.
        }
    }
}

impl DeployData {
    pub fn total_phlo_charge(&self) -> i64 { self.phlo_limit * self.phlo_price }

    /// Returns true if this deploy has a time-based expiration set
    pub fn has_expiration(&self) -> bool {
        self.expiration_timestamp
            .map(|exp| exp > 0)
            .unwrap_or(false)
    }

    /// Returns true if this deploy has expired at the given time
    pub fn is_expired_at(&self, current_time_millis: i64) -> bool {
        self.expiration_timestamp
            .map(|exp| current_time_millis > exp)
            .unwrap_or(false)
    }

    pub fn encode(a: DeployData) -> ByteVector { DeployData::_to_proto(a).encode_to_vec() }

    pub fn decode(a: ByteVector) -> Result<DeployData, String> {
        let proto = DeployDataProto::decode(&a[..])
            .map_err(|e| format!("Failed to decode DeployData: {}", e))?;
        Self::reject_nonlegacy_fields(&proto)?;
        Ok(DeployData::_from_proto(proto))
    }

    fn reject_nonlegacy_fields(proto: &DeployDataProto) -> Result<(), String> {
        if !proto.cosigners.is_empty()
            || proto.cosigner_threshold != 0
            || proto.sig_algebra.is_some()
            || !proto.authority_presentations.is_empty()
            || !proto.deploy_id.is_empty()
            || proto.authorization_v61.is_some()
            || proto.funding_intent.is_some()
        {
            return Err(
                "legacy Casper cannot decode cost-accounted deploy authorization".to_string(),
            );
        }
        Ok(())
    }

    fn _from_proto(proto: DeployDataProto) -> Self {
        Self {
            term: proto.term,
            time_stamp: proto.timestamp,
            phlo_price: proto.phlo_price,
            phlo_limit: proto.phlo_limit,
            valid_after_block_number: proto.valid_after_block_number,
            shard_id: proto.shard_id,
            // 0 in protobuf means not set, convert to None
            expiration_timestamp: if proto.expiration_timestamp == 0 {
                None
            } else {
                Some(proto.expiration_timestamp)
            },
        }
    }

    pub fn from_proto(proto: DeployDataProto) -> Result<Signed<DeployData>, String> {
        Self::reject_nonlegacy_fields(&proto)?;
        let algorithm = SignaturesAlgFactory::apply(&proto.sig_algorithm)
            .ok_or_else(|| format!("Unknown signature algorithm: {}", proto.sig_algorithm))?;

        let sig = proto.sig.clone();
        let pk = PublicKey::from_bytes(&proto.deployer);
        let signed = Signed::from_signed_data(DeployData::_from_proto(proto), pk, sig, algorithm)?;

        match signed {
            Some(signed) => Ok(signed),
            None => Err("Invalid signature".to_string()),
        }
    }

    /// Multi-signature aware decode. Returns a [`Cosigned<DeployData>`]
    /// envelope covering both legacy single-sig wire deploys (`cosigners`
    /// empty → one-element envelope) and multi-sig wire deploys (`cosigners`
    /// non-empty → N-element envelope).
    ///
    /// Invariants enforced by `Cosigned::from_signed_data` at construction:
    /// 1. All signers' signatures verify against the canonical message hash.
    /// 2. Canonical pk-ascending sort; no duplicates.
    ///
    /// D3 (DR-9): there is no per-signer `phlo_share` and no share-sum
    /// invariant — fuel is the per-signature supply pool Σ⟦s⟧.
    pub fn from_proto_cosigned(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        // Resolve the primary signer's algorithm.
        let primary_alg = SignaturesAlgFactory::apply(&proto.sig_algorithm).ok_or_else(|| {
            format!(
                "Unknown primary signature algorithm: {}",
                proto.sig_algorithm
            )
        })?;

        // Phase 3 dispatch: sig_algebra (LL-rich algebra) OVERRIDES
        // the flat cosigners[] + cosigner_threshold path. Take it out
        // of proto upfront so the later _from_proto consumes the rest.
        let sig_algebra = proto.sig_algebra.clone();

        let is_multi_sig = !proto.cosigners.is_empty();
        let cosigner_threshold = proto.cosigner_threshold;
        let total_signers = 1 + proto.cosigners.len() as i32;
        // Validate threshold range at the wire boundary so we can return
        // a clear protocol-level error before signer construction.
        if cosigner_threshold < 0 || cosigner_threshold > total_signers {
            return Err(format!(
                "Invalid cosigner_threshold {}: must satisfy 0 ≤ threshold ≤ {} (total signers)",
                cosigner_threshold, total_signers
            ));
        }

        // Build the canonical signer list. Primary first (will be sorted
        // canonically by Cosigned::from_signed_data). D3 (DR-9): no per-signer
        // phlo_share — funding is the per-signature supply pool Σ⟦s⟧.
        let mut signers = Vec::with_capacity(1 + proto.cosigners.len());
        signers.push(Cosigner {
            pk: PublicKey::from_bytes(&proto.deployer),
            sig: proto.sig.clone(),
            sig_algorithm: primary_alg,
        });
        for cs in &proto.cosigners {
            let alg = SignaturesAlgFactory::apply(&cs.sig_algorithm).ok_or_else(|| {
                format!(
                    "Unknown cosigner signature algorithm: {} for cosigner pk={}",
                    cs.sig_algorithm,
                    hex::encode(&cs.pk)
                )
            })?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&cs.pk),
                sig: cs.sig.clone(),
                sig_algorithm: alg,
            });
        }

        let data = DeployData::_from_proto(proto);
        // Phase 3 dispatch: when sig_algebra is set, the flat cosigners
        // path is ignored; verification walks the algebra.
        if let Some(algebra) = sig_algebra {
            return Self::from_proto_cosigned_with_sig_algebra(data, &algebra);
        }
        // Phase 2 dispatch: cosigner_threshold == 0 → N-of-N (Phase 1
        // semantics; every signer must verify); cosigner_threshold > 0 →
        // M-of-N (at least `threshold` valid signatures required;
        // placeholder signers with empty sig are admitted).
        if cosigner_threshold == 0 {
            Cosigned::from_signed_data(data, signers).map_err(|e| {
                format!(
                    "Cosigned envelope validation failed (is_multi_sig={}): {}",
                    is_multi_sig, e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, cosigner_threshold as u32)
                .map_err(|e| {
                    format!(
                        "Cosigned threshold envelope validation failed (threshold={}, total_signers={}): {}",
                        cosigner_threshold, total_signers, e
                    )
                })
        }
    }

    /// Phase 3 LL-rich algebra dispatch. Validates a deploy against
    /// the given `SigCompound` algebraic expression:
    ///
    /// - Tensor(left, right): both branches must verify (recursive).
    /// - Plus(left, right, chosen_branch): only the chosen branch verifies.
    /// - With(left, right): both branches' atoms must be presented and
    ///   verify (the verifier picks which to consume at evaluation).
    /// - Bang(inner): inner atom must verify (replicable at evaluation).
    /// - WhyNot(inner): if inner has a non-empty sig, it must verify;
    ///   empty sig is permitted (optional / zero-or-more uses).
    /// - Lolly(from, to, handle): both branches must verify (the
    ///   transformer runs at evaluation via the capability registry).
    /// - Threshold{threshold, members}: at least `threshold` members
    ///   must verify (Phase 2 quorum semantics, lifted into the algebra).
    /// - Atom: the leaf signer; signature must verify.
    ///
    /// The collected atoms are folded into a canonical Cosigner list
    /// (sorted ascending by pk; placeholders for non-required atoms get
    /// empty sigs and zero shares) and `Cosigned::from_signed_data_threshold`
    /// (with `threshold = required_signers.len()`) finalizes the envelope.
    pub fn from_proto_cosigned_with_sig_algebra(
        data: DeployData,
        sig_algebra: &crate::casper::SigCompound,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        // F-A funding/capability separation — INGRESS REJECT (c), THE
        // load-bearing guard (`docs/theory/cost-accounting-impl/
        // f-a-funding-vs-capability-separation.md` §3/§6, red-team item 4).
        //
        // BEFORE lowering the wire algebra to a flat `Cosigned`, reject any
        // `sig_algebra` whose tree contains a VALUE/CAPABILITY type-logic
        // connective (`Plus` ⊕ / `With` & / `Bang` ! / `WhyNot` ? / `Lolly` ⊸).
        // These are NOT funding-signature formers (cost-accounted-rho §App-A:
        // the funding grammar is `s ::= g | #P | s ∘ s` — atoms folded by the
        // tensor only); they belong to the capability/type layer
        // (`typed_value.tex`, `rho:system:capabilities` + W2). `Atom`, `Tensor`
        // (the funding `And` ∘), and `Threshold` (a k-of-N admission-boundary
        // quorum, F-A Threshold=(A) — lowered to a flat `Cosigned` + scalar
        // `cosigner_threshold`) are KEPT.
        //
        // This is what actually stops a malformed ingress algebra: the analysis below
        // would otherwise flatten
        // a `⊕/&/!/?/⊸`-formed algebra and fold it into a funding envelope.
        Self::reject_capability_connectives(sig_algebra)?;

        // One stack-safe analysis pass collects EVERY atom, computes the
        // required signature count, and remembers whether the accepted funding
        // grammar is pure N-of-N. Threshold descendants are still collected,
        // but an outer threshold contributes its scalar exactly once.
        let (atoms, min_required, all_required) = Self::analyze_funding_algebra(sig_algebra)?;

        if atoms.is_empty() {
            return Err(
                "Sig algebra contains no atomic signers — at least one Atom is required"
                    .to_string(),
            );
        }

        let mut signers: Vec<Cosigner> = Vec::with_capacity(atoms.len());
        for atom in atoms.into_iter() {
            let alg = SignaturesAlgFactory::apply(&atom.sig_algorithm)
                .ok_or_else(|| format!("Unknown signature algorithm: {}", atom.sig_algorithm))?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&atom.pk),
                sig: atom.sig,
                sig_algorithm: alg,
            });
        }

        // Dispatch:
        //   - All atoms required (min_required == signers.len(), no Plus
        //     branch dropped, no WhyNot absent, no Threshold) →
        //     from_signed_data (canonical N-of-N).
        //   - Otherwise → from_signed_data_threshold with min_required.
        let total = signers.len() as u32;
        if min_required == total && all_required {
            Cosigned::from_signed_data(data, signers)
                .map_err(|e| format!("Cosigned sig_algebra validation failed: {}", e))
        } else if min_required == 0 {
            let presented: Vec<Cosigner> = signers
                .into_iter()
                .filter(|signer| !signer.sig.is_empty())
                .collect();
            if presented.is_empty() {
                return Err(
                    "Sig algebra requires zero valid signatures and presents no signer; a Cosigned envelope requires at least one"
                        .to_string(),
                );
            }
            Cosigned::from_signed_data(data, presented).map_err(|e| {
                format!(
                    "Cosigned sig_algebra optional-present validation failed: {}",
                    e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, min_required).map_err(|e| {
                format!(
                    "Cosigned sig_algebra threshold validation failed (min_required={}): {}",
                    min_required, e
                )
            })
        }
    }

    /// F-A funding/capability separation — INGRESS REJECT (c). Iteratively
    /// reject a `SigCompound` deploy algebra that contains ANY value/capability
    /// type-logic connective (`Plus` ⊕ / `With` & / `Bang` ! / `WhyNot` ? /
    /// `Lolly` ⊸ — the five formers that are NOT in the funding grammar
    /// `g|#P|s∘s`, cost-accounted-rho §App-A). `Atom`, `Tensor` (the funding
    /// `And` ∘), and `Threshold` (a k-of-N admission-boundary quorum, F-A
    /// Threshold=(A)) are accepted; the walk recurses THROUGH `Tensor`'s sides
    /// and `Threshold`'s members so a connective nested anywhere in the tree is
    /// still caught.
    ///
    /// Returns a crisp protocol error naming the offending connective. This is
    /// the load-bearing wire-boundary guard: it refuses a malicious/ill-formed
    /// deploy at decode time, BEFORE the algebra is lowered into a funding
    /// `Cosigned`.
    fn reject_capability_connectives(sig: &crate::casper::SigCompound) -> Result<(), String> {
        use crate::casper::sig_compound::Connective;
        const NOT_A_FUNDING_FORMER: &str =
            "is not a funding-signature former (cost-accounted-rho §App-A: \
             funding signatures are g | #P | s∘s — ground/quote atoms folded by \
             the tensor ∘; value/capability connectives ⊕/&/!/?/⊸ are \
             capability-layer only)";
        let mut work = vec![sig];
        while let Some(sig) = work.pop() {
            let connective = sig
                .connective
                .as_ref()
                .ok_or_else(|| "SigCompound.connective missing".to_string())?;
            match connective {
                Connective::Atom(_) => {}
                Connective::Tensor(pair) => {
                    let left = pair
                        .left
                        .as_deref()
                        .ok_or_else(|| "SigPair.left missing".to_string())?;
                    let right = pair
                        .right
                        .as_deref()
                        .ok_or_else(|| "SigPair.right missing".to_string())?;
                    work.push(right);
                    work.push(left);
                }
                Connective::Threshold(threshold) => {
                    work.extend(threshold.members.iter().rev());
                }
                Connective::Plus(_) => {
                    return Err(format!(
                        "value/capability connective ⊕ (Plus) {NOT_A_FUNDING_FORMER}"
                    ));
                }
                Connective::With(_) => {
                    return Err(format!(
                        "value/capability connective & (With) {NOT_A_FUNDING_FORMER}"
                    ));
                }
                Connective::Bang(_) => {
                    return Err(format!(
                        "value/capability connective ! (Bang) {NOT_A_FUNDING_FORMER}"
                    ));
                }
                Connective::Whynot(_) => {
                    return Err(format!(
                        "value/capability connective ? (WhyNot) {NOT_A_FUNDING_FORMER}"
                    ));
                }
                Connective::Lolly(_) => {
                    return Err(format!(
                        "value/capability connective ⊸ (Lolly) {NOT_A_FUNDING_FORMER}"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Analyze an already funding-validated algebra in one explicit-worklist
    /// pass. The boolean on each job says whether this subtree contributes to
    /// the outer minimum; members below `Threshold(k, …)` are still collected as
    /// signers but do not add their individual minima on top of `k`.
    fn analyze_funding_algebra(
        sig: &crate::casper::SigCompound,
    ) -> Result<(Vec<AlgebraAtom>, u32, bool), String> {
        use crate::casper::sig_compound::Connective;

        let mut atoms = Vec::new();
        let mut min_required = 0u32;
        let mut all_required = true;
        let mut work = vec![(sig, true)];

        while let Some((sig, contributes)) = work.pop() {
            let connective = sig
                .connective
                .as_ref()
                .ok_or_else(|| "SigCompound.connective missing".to_string())?;
            match connective {
                Connective::Atom(atom) => {
                    atoms.push(AlgebraAtom::from_proto(atom));
                    if contributes {
                        min_required += 1;
                    }
                }
                Connective::Tensor(pair) => {
                    let left = pair
                        .left
                        .as_deref()
                        .ok_or_else(|| "SigPair.left missing".to_string())?;
                    let right = pair
                        .right
                        .as_deref()
                        .ok_or_else(|| "SigPair.right missing".to_string())?;
                    work.push((right, contributes));
                    work.push((left, contributes));
                }
                Connective::Threshold(threshold) => {
                    if threshold.threshold < 1
                        || (threshold.threshold as usize) > threshold.members.len()
                    {
                        return Err(format!(
                            "SigThreshold.threshold must satisfy 1 ≤ threshold ≤ members.len() ({}), got {}",
                            threshold.members.len(),
                            threshold.threshold
                        ));
                    }
                    all_required = false;
                    if contributes {
                        min_required += threshold.threshold as u32;
                    }
                    work.extend(threshold.members.iter().rev().map(|member| (member, false)));
                }
                Connective::Plus(_)
                | Connective::With(_)
                | Connective::Bang(_)
                | Connective::Whynot(_)
                | Connective::Lolly(_) => {
                    return Err(
                        "capability connective reached funding analysis after ingress validation"
                            .to_string(),
                    );
                }
            }
        }

        Ok((atoms, min_required, all_required))
    }

    fn _to_proto(dd: DeployData) -> DeployDataProto {
        DeployDataProto {
            term: dd.term,
            timestamp: dd.time_stamp,
            phlo_price: dd.phlo_price,
            phlo_limit: dd.phlo_limit,
            valid_after_block_number: dd.valid_after_block_number,
            shard_id: dd.shard_id,
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.expiration_timestamp.unwrap_or(0),
            ..Default::default()
        }
    }

    pub fn to_proto(dd: Signed<DeployData>) -> DeployDataProto { Self::to_proto_ref(&dd) }

    pub fn to_proto_ref(dd: &Signed<DeployData>) -> DeployDataProto {
        DeployDataProto {
            term: dd.data.term.clone(),
            timestamp: dd.data.time_stamp,
            phlo_price: dd.data.phlo_price,
            phlo_limit: dd.data.phlo_limit,
            valid_after_block_number: dd.data.valid_after_block_number,
            shard_id: dd.data.shard_id.clone(),
            deployer: dd.pk.bytes.clone().into(),
            sig: dd.sig.clone().into(),
            sig_algorithm: dd.sig_algorithm.name(),
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.data.expiration_timestamp.unwrap_or(0),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peek {
    pub channel_index: i32,
}

impl Peek {
    pub fn from_proto(proto: PeekProto) -> Self {
        Self {
            channel_index: proto.channel_index,
        }
    }

    pub fn to_proto(self) -> PeekProto {
        PeekProto {
            channel_index: self.channel_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Produce(ProduceEvent),
    Consume(ConsumeEvent),
    Comm(CommEvent),
    RestingComm(RestingCommEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProduceEvent {
    pub channels_hash: ByteString,
    pub hash: ByteString,
    pub persistent: bool,
    pub times_repeated: i32,
    pub is_deterministic: bool,
    pub output_value: Vec<ByteString>,
    /// Indicates whether this produce event represents a failed non-deterministic process.
    pub failed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsumeEvent {
    pub channels_hashes: Vec<ByteString>,
    pub hash: ByteString,
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommEvent {
    pub consume: ConsumeEvent,
    pub produces: Vec<ProduceEvent>,
    pub peeks: Vec<Peek>,
}

/// A CostV2 resting rendezvous. The two ordered vectors address the same
/// stored data in bind order; neither may be sorted or deduplicated.
#[derive(Clone, Debug, PartialEq)]
pub struct RestingCommEvent {
    pub consume: ConsumeEvent,
    pub continuation_index: u64,
    pub ordered_produces: Vec<ProduceEvent>,
    pub ordered_data_indices: Vec<u64>,
    pub peeks: Vec<Peek>,
}

impl RestingCommEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.consume.channels_hashes.len() != self.ordered_produces.len()
            || self.ordered_produces.len() != self.ordered_data_indices.len()
        {
            return Err("resting COMM bind-order arity mismatch".to_string());
        }
        if self.continuation_index > i32::MAX as u64
            || self.ordered_data_indices.iter().any(|index| *index > i32::MAX as u64)
        {
            return Err("resting COMM store index exceeds supported address range".to_string());
        }
        if self.consume.hash.len() != 32
            || self.consume.channels_hashes.iter().any(|hash| hash.len() != 32)
            || self.ordered_produces.iter().any(|produce| {
                produce.hash.len() != 32
                    || produce.channels_hash.len() != 32
                    || produce.times_repeated != 0
            })
        {
            return Err("resting COMM source has invalid hash or repetition".to_string());
        }
        let mut previous = None;
        for peek in &self.peeks {
            if peek.channel_index < 0
                || usize::try_from(peek.channel_index)
                    .ok()
                    .is_none_or(|index| index >= self.ordered_produces.len())
                || previous.is_some_and(|index| peek.channel_index <= index)
            {
                return Err("resting COMM peeks are not canonical bind indices".to_string());
            }
            previous = Some(peek.channel_index);
        }
        Ok(())
    }

    pub fn from_proto(proto: RestingCommEventProto) -> Result<Self, String> {
        let event = Self {
            consume: ConsumeEvent::from_proto(
                proto.consume.ok_or_else(|| "Missing resting consume field".to_string())?,
            ),
            continuation_index: proto.continuation_index,
            ordered_produces: proto
                .ordered_produces
                .into_iter()
                .map(ProduceEvent::from_proto)
                .collect(),
            ordered_data_indices: proto.ordered_data_indices,
            peeks: proto.peeks.into_iter().map(Peek::from_proto).collect(),
        };
        event.validate()?;
        Ok(event)
    }

    pub fn to_proto(&self) -> RestingCommEventProto {
        RestingCommEventProto {
            consume: Some(self.consume.clone().to_proto()),
            continuation_index: self.continuation_index,
            ordered_produces: self.ordered_produces.iter().cloned().map(ProduceEvent::to_proto).collect(),
            ordered_data_indices: self.ordered_data_indices.clone(),
            peeks: self.peeks.iter().cloned().map(Peek::to_proto).collect(),
        }
    }
}

impl Event {
    pub fn from_proto(proto: EventProto) -> Result<Event, String> {
        match proto.event_instance {
            Some(event_proto::EventInstance::Produce(pe)) => {
                Ok(Event::Produce(ProduceEvent::from_proto(pe)))
            }
            Some(event_proto::EventInstance::Consume(ce)) => {
                Ok(Event::Consume(ConsumeEvent::from_proto(ce)))
            }
            Some(event_proto::EventInstance::Comm(CommEventProto {
                consume,
                produces,
                peeks,
            })) => Ok(Event::Comm(CommEvent {
                consume: ConsumeEvent::from_proto(
                    consume.ok_or_else(|| "Missing consume field".to_string())?,
                ),
                produces: produces.into_iter().map(ProduceEvent::from_proto).collect(),
                peeks: peeks.into_iter().map(Peek::from_proto).collect(),
            })),
            Some(event_proto::EventInstance::RestingComm(resting)) => {
                Ok(Event::RestingComm(RestingCommEvent::from_proto(resting)?))
            }

            _ => Err("Received malformed Event: None".to_string()),
        }
    }

    pub fn to_proto(&self) -> EventProto {
        match self {
            Event::Produce(pe) => EventProto {
                event_instance: Some(event_proto::EventInstance::Produce(pe.clone().to_proto())),
            },
            Event::Consume(ce) => EventProto {
                event_instance: Some(event_proto::EventInstance::Consume(ce.clone().to_proto())),
            },
            Event::Comm(cme) => EventProto {
                event_instance: Some(event_proto::EventInstance::Comm(cme.clone().to_proto())),
            },
            Event::RestingComm(resting) => EventProto {
                event_instance: Some(event_proto::EventInstance::RestingComm(resting.to_proto())),
            },
        }
    }
}

impl ProduceEvent {
    pub fn to_proto(self) -> ProduceEventProto {
        ProduceEventProto {
            channels_hash: self.channels_hash,
            hash: self.hash,
            persistent: self.persistent,
            times_repeated: self.times_repeated,
            is_deterministic: self.is_deterministic,
            output_value: self.output_value,
            failed: self.failed,
        }
    }

    pub fn from_proto(proto: ProduceEventProto) -> Self {
        ProduceEvent {
            channels_hash: proto.channels_hash,
            hash: proto.hash,
            persistent: proto.persistent,
            times_repeated: proto.times_repeated,
            is_deterministic: proto.is_deterministic,
            output_value: proto.output_value,
            failed: proto.failed,
        }
    }
}

impl ConsumeEvent {
    pub fn to_proto(self) -> ConsumeEventProto {
        ConsumeEventProto {
            channels_hashes: self.channels_hashes,
            hash: self.hash,
            persistent: self.persistent,
        }
    }

    pub fn from_proto(proto: ConsumeEventProto) -> Self {
        ConsumeEvent {
            channels_hashes: proto.channels_hashes,
            hash: proto.hash,
            persistent: proto.persistent,
        }
    }
}

impl CommEvent {
    pub fn to_proto(self) -> CommEventProto {
        CommEventProto {
            consume: Some(self.consume.to_proto()),
            produces: self.produces.into_iter().map(|pe| pe.to_proto()).collect(),
            peeks: self.peeks.into_iter().map(|pk| pk.to_proto()).collect(),
        }
    }

    pub fn from_proto(
        consume: ConsumeEventProto,
        produces: Vec<ProduceEventProto>,
        peeks: Vec<PeekProto>,
    ) -> Self {
        CommEvent {
            consume: ConsumeEvent::from_proto(consume),
            produces: produces.into_iter().map(ProduceEvent::from_proto).collect(),
            peeks: peeks.into_iter().map(Peek::from_proto).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bond {
    pub validator: ByteString,
    pub stake: i64,
}

impl Bond {
    pub fn from_proto(proto: BondProto) -> Self {
        Self {
            validator: proto.validator,
            stake: proto.stake,
        }
    }

    pub fn to_proto(self) -> BondProto {
        BondProto {
            validator: self.validator,
            stake: self.stake,
        }
    }
}

// Last finalized state

pub struct StoreNodeKey {
    pub hash: Blake2b256Hash,
    pub index: Option<Byte>,
}

impl StoreNodeKey {
    // Encoding of non-existent index for store node (Skip or Leaf node)
    const NONE_INDEX: i32 = 0x100;

    pub fn from_proto(proto: StoreNodeKeyProto) -> (Blake2b256Hash, Option<Byte>) {
        // Key hash
        let hash_bytes = Blake2b256Hash::from_bytes(proto.hash.to_vec());

        // Relative branch index / max 8-bit
        let idx = if proto.index == Self::NONE_INDEX {
            None
        } else {
            Some(proto.index as u8)
        };

        (hash_bytes, idx)
    }

    pub fn to_proto(s: &(Blake2b256Hash, Option<Byte>)) -> StoreNodeKeyProto {
        StoreNodeKeyProto {
            hash: s.0.bytes().into(),
            index: s.1.map(|b| b as i32).unwrap_or(Self::NONE_INDEX),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreItemsMessageRequest {
    pub start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub skip: i32,
    pub take: i32,
}

impl StoreItemsMessageRequest {
    pub fn from_proto(proto: StoreItemsMessageRequestProto) -> Self {
        Self {
            start_path: proto
                .start_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            skip: proto.skip,
            take: proto.take,
        }
    }

    pub fn to_proto(self) -> StoreItemsMessageRequestProto {
        StoreItemsMessageRequestProto {
            start_path: self.start_path.iter().map(StoreNodeKey::to_proto).collect(),
            skip: self.skip,
            take: self.take,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreItemsMessage {
    pub start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub last_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub history_items: Vec<(Blake2b256Hash, ByteString)>,
    pub data_items: Vec<(Blake2b256Hash, ByteString)>,
}

impl StoreItemsMessage {
    pub fn pretty(self) -> String {
        let start: String = self
            .start_path
            .iter()
            .map(RSpaceExporterInstance::path_pretty)
            .collect();

        let last: String = self
            .last_path
            .iter()
            .map(RSpaceExporterInstance::path_pretty)
            .collect();

        let history_size = self.history_items.len();
        let data_size = self.data_items.len();

        format!(
            "StoreItemsMessage(history: {:?}, data: {:?}, start: {:?}, last: {:?})",
            history_size, data_size, start, last
        )
    }

    pub fn from_proto(proto: StoreItemsMessageProto) -> Self {
        Self {
            start_path: proto
                .start_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            last_path: proto
                .last_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            history_items: proto
                .history_items
                .into_iter()
                .map(|store_item_proto| {
                    (
                        Blake2b256Hash::from_bytes(store_item_proto.key.to_vec()),
                        store_item_proto.value,
                    )
                })
                .collect(),
            data_items: proto
                .data_items
                .into_iter()
                .map(|store_item_proto| {
                    (
                        Blake2b256Hash::from_bytes(store_item_proto.key.to_vec()),
                        store_item_proto.value,
                    )
                })
                .collect(),
        }
    }

    pub fn to_proto(self) -> StoreItemsMessageProto {
        StoreItemsMessageProto {
            start_path: self.start_path.iter().map(StoreNodeKey::to_proto).collect(),
            last_path: self.last_path.iter().map(StoreNodeKey::to_proto).collect(),
            history_items: self
                .history_items
                .into_iter()
                .map(|(key, value)| StoreItemProto {
                    key: key.bytes().into(),
                    value,
                })
                .collect(),
            data_items: self
                .data_items
                .into_iter()
                .map(|(key, value)| StoreItemProto {
                    key: key.bytes().into(),
                    value,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergeableEntryRequest {
    pub block_hash: ByteString,
}

impl MergeableEntryRequest {
    pub fn from_proto(proto: MergeableEntryRequestProto) -> Self {
        Self {
            block_hash: proto.block_hash,
        }
    }

    pub fn to_proto(self) -> MergeableEntryRequestProto {
        MergeableEntryRequestProto {
            block_hash: self.block_hash,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergeableEntryResponse {
    pub block_hash: ByteString,
    /// Bincode of `Vec<DeployMergeableData>`. Empty bytes = peer has the block
    /// but no entry for it.
    pub serialized_entry: ByteString,
}

impl MergeableEntryResponse {
    pub fn from_proto(proto: MergeableEntryResponseProto) -> Self {
        Self {
            block_hash: proto.block_hash,
            serialized_entry: proto.serialized_entry,
        }
    }

    pub fn to_proto(self) -> MergeableEntryResponseProto {
        MergeableEntryResponseProto {
            block_hash: self.block_hash,
            serialized_entry: self.serialized_entry,
        }
    }
}

#[cfg(test)]
mod approved_block_tests {
    use prost::bytes::Bytes;

    use super::*;

    fn candidate() -> ApprovedBlockCandidate {
        ApprovedBlockCandidate {
            block: BlockMessage {
                block_hash: Bytes::from_static(b"anchor"),
                header: Header {
                    parents_hash_list: vec![],
                    timestamp: 0,
                    version: 0,
                    extra_bytes: Bytes::new(),
                },
                body: Body {
                    state: F1r3flyState {
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                        bonds: vec![],
                        block_number: 87,
                    },
                    deploys: vec![],
                    rejected_deploys: vec![],
                    system_deploys: vec![],
                    extra_bytes: Bytes::new(),
                    applied_from_scope: vec![],
                    merge_base: Bytes::new(),
                },
                justifications: vec![],
                sender: Bytes::new(),
                seq_num: 0,
                sig: Bytes::new(),
                sig_algorithm: String::new(),
                shard_id: "root".to_string(),
                extra_bytes: Bytes::new(),
            },
            required_sigs: 0,
        }
    }

    /// The finalized-floor cache travels with the LFS window. A restored node
    /// cannot derive floors for blocks below its anchor — the derivation
    /// recurses through history it deliberately does not keep — and without
    /// them every sibling-branch validation crawls gap-by-gap toward genesis.
    /// The responder computed these values when it validated the blocks; the
    /// numbers are hashes only, a few KB for a window whose size is constant
    /// in chain height.
    #[test]
    fn the_floor_cache_survives_the_wire() {
        let entry = FloorCacheEntry {
            block_hash: Bytes::from_static(b"window-block"),
            floor_hash: Bytes::from_static(b"its-floor"),
            frontier_hash: Bytes::from_static(b"its-frontier"),
        };
        let request = FloorCacheRequest {
            hashes: vec![Bytes::from_static(b"window-block")],
        };
        let response = FloorCacheResponse {
            entries: vec![entry],
            genesis_hash: Bytes::from_static(b"the-genesis"),
            genesis_block: Some(crate::rust::block_implicits::get_random_block_default()),
        };

        assert_eq!(
            FloorCacheRequest::from_proto(request.clone().to_proto()),
            request,
            "the requested hash set must survive the wire"
        );
        assert_eq!(
            FloorCacheResponse::from_proto(response.clone().to_proto()),
            response,
            "every entry must survive intact: the receiver writes these into the \
             same caches its own validation would have filled"
        );
    }

    /// The seed rides on the ApprovedBlock, never inside its candidate: the
    /// candidate's serialized bytes are what the genesis ceremony signs and
    /// what `Validate::approved_block` re-derives to verify, so a field added
    /// there would put unsigned peer-supplied data inside the signed envelope
    /// and make two ceremony participants disagree on the digest.
    #[test]
    fn the_floor_seed_survives_the_wire_and_the_candidate_digest_does_not_move() {
        let seed = FinalizedFloorSeed {
            floor_hash: Bytes::from_static(b"floor"),
            floor_number: 37,
            frontier_hash: Bytes::from_static(b"frontier"),
            frontier_number: 41,
        };
        let seeded = ApprovedBlock {
            candidate: candidate(),
            sigs: vec![],
            floor_seed: Some(seed.clone()),
        };
        let bare = ApprovedBlock {
            candidate: candidate(),
            sigs: vec![],
            floor_seed: None,
        };

        assert_eq!(
            ApprovedBlock::from_proto(seeded.clone().to_proto()).expect("round trip"),
            seeded,
            "the seed must survive the wire intact: the receiver sizes its download \
             window from these numbers before it requests a single block"
        );
        assert_eq!(
            ApprovedBlock::from_proto(bare.clone().to_proto()).expect("round trip"),
            bare,
            "a peer that sends no seed must decode as no seed, not as a zero floor"
        );

        assert_eq!(
            seeded
                .to_proto()
                .candidate
                .expect("candidate")
                .encode_to_vec(),
            bare.to_proto()
                .candidate
                .expect("candidate")
                .encode_to_vec(),
            "seeding must not shift one byte of the candidate: those bytes are the \
             ceremony's signed payload"
        );
    }
}

#[cfg(test)]
mod tests {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    use super::*;
    use crate::rust::block_implicits::{get_random_block_default, signed_deploy_data_gen};

    type Bytes = prost::bytes::Bytes;

    fn signed_deploy() -> Signed<DeployData> {
        signed_deploy_data_gen()
            .new_tree(&mut TestRunner::default())
            .unwrap()
            .current()
    }

    #[test]
    fn processed_user_deploy_keeps_legacy_wire_and_rejects_malformed_offered_decode() {
        let legacy = ProcessedDeploy::empty(signed_deploy()).unwrap();
        let legacy_wire = legacy.clone().to_proto();
        let decoded = ProcessedUserDeploy::from_proto(legacy_wire.clone()).unwrap();
        assert_eq!(decoded.to_proto(), legacy_wire);
        assert_eq!(decoded.as_legacy(), Some(&legacy));

        let mut offered_wire = legacy_wire;
        offered_wire.deploy.as_mut().unwrap().authorization_v61 = Some(DeployAuthorizationV61 {
            format_version:
                crate::rust::signed_phlo_deploy::OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION,
            ..Default::default()
        });
        assert!(ProcessedUserDeploy::from_proto(offered_wire).is_err());
    }

    fn produce_event() -> ProduceEvent {
        ProduceEvent {
            channels_hash: Bytes::from_static(b"chan"),
            hash: Bytes::from_static(b"produce"),
            persistent: true,
            times_repeated: 2,
            is_deterministic: false,
            output_value: vec![Bytes::from_static(b"out")],
            failed: true,
        }
    }

    fn consume_event() -> ConsumeEvent {
        ConsumeEvent {
            channels_hashes: vec![Bytes::from_static(b"c1"), Bytes::from_static(b"c2")],
            hash: Bytes::from_static(b"consume"),
            persistent: false,
        }
    }

    fn hash32(fill: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![fill; 32]) }

    #[test]
    fn rejected_deploy_decodes_legacy_wire_format() {
        let legacy = [0x0a, 0x03, b's', b'i', b'g'];
        let decoded = RejectedDeployProto::decode(legacy.as_slice()).unwrap();
        let record = RejectedDeploy::from_proto(decoded);

        assert_eq!(record.sig, ByteString::from_static(b"sig"));
        assert!(!record.duplicate);
        assert!(record.carrier.is_empty());
    }

    #[test]
    fn hash_addressed_messages_round_trip() {
        let has_block = HasBlock {
            hash: Bytes::from_static(b"h1"),
        };
        assert_eq!(
            HasBlock::from_proto(has_block.clone().to_proto()),
            has_block
        );

        let has_block_request = HasBlockRequest {
            hash: Bytes::from_static(b"h2"),
        };
        assert_eq!(
            HasBlockRequest::from_proto(has_block_request.clone().to_proto()),
            has_block_request
        );

        let block_request = BlockRequest {
            hash: Bytes::from_static(b"h3"),
        };
        assert_eq!(
            BlockRequest::from_proto(block_request.clone().to_proto()),
            block_request
        );

        let mergeable_request = MergeableEntryRequest {
            block_hash: Bytes::from_static(b"h4"),
        };
        assert_eq!(
            MergeableEntryRequest::from_proto(mergeable_request.clone().to_proto()),
            mergeable_request
        );

        let mergeable_response = MergeableEntryResponse {
            block_hash: Bytes::from_static(b"h5"),
            serialized_entry: Bytes::from_static(b"entry"),
        };
        assert_eq!(
            MergeableEntryResponse::from_proto(mergeable_response.clone().to_proto()),
            mergeable_response
        );
    }

    #[test]
    fn identifier_messages_round_trip() {
        let block_hash_message = BlockHashMessage {
            block_hash: Bytes::from_static(b"hash"),
            block_creator: Bytes::from_static(b"creator"),
        };
        assert_eq!(
            BlockHashMessage::from_proto(block_hash_message.clone().to_proto()),
            block_hash_message
        );

        let no_approved = NoApprovedBlockAvailable {
            identifier: "id".to_string(),
            node_identifier: "node".to_string(),
        };
        assert_eq!(
            NoApprovedBlockAvailable::from_proto(no_approved.clone().to_proto()),
            no_approved
        );

        let approved_request = ApprovedBlockRequest {
            identifier: "id".to_string(),
            trim_state: true,
        };
        assert_eq!(
            ApprovedBlockRequest::from_proto(approved_request.clone().to_proto()),
            approved_request
        );

        assert_eq!(
            ForkChoiceTipRequest.to_proto(),
            ForkChoiceTipRequestProto {}
        );
    }

    #[test]
    fn block_message_round_trips_through_proto() {
        let block = get_random_block_default();
        let round_tripped = BlockMessage::from_proto(block.to_proto()).unwrap();
        assert_eq!(round_tripped, block);
    }

    #[test]
    fn block_message_from_proto_requires_header_and_body() {
        let block = get_random_block_default();

        let mut missing_header = block.to_proto();
        missing_header.header = None;
        assert_eq!(
            BlockMessage::from_proto(missing_header),
            Err("Missing header field".to_string())
        );

        let mut missing_body = block.to_proto();
        missing_body.body = None;
        assert_eq!(
            BlockMessage::from_proto(missing_body),
            Err("Missing body field".to_string())
        );
    }

    #[test]
    fn block_message_to_string_pretty_prints() {
        let block = get_random_block_default();
        let rendered = block.clone().to_string();
        assert!(rendered.contains(&format!("#{}", block.body.state.block_number)));
    }

    #[test]
    fn unapproved_block_and_block_approval_round_trip() {
        let block = get_random_block_default();
        let candidate = ApprovedBlockCandidate {
            block,
            required_sigs: 3,
        };

        let unapproved = UnapprovedBlock {
            candidate: candidate.clone(),
            timestamp: 11,
            duration: 22,
        };
        assert_eq!(
            UnapprovedBlock::from_proto(unapproved.clone().to_proto()).unwrap(),
            unapproved
        );

        let approval = BlockApproval {
            candidate: candidate.clone(),
            sig: Signature {
                public_key: Bytes::from_static(b"pk"),
                algorithm: "secp256k1".to_string(),
                sig: Bytes::from_static(b"sig"),
            },
        };
        assert_eq!(
            BlockApproval::from_proto(approval.clone().to_proto()).unwrap(),
            approval
        );

        assert_eq!(
            BlockApproval::from_proto(BlockApprovalProto {
                candidate: None,
                sig: None,
            }),
            Err("Missing candidate field".to_string())
        );
        assert_eq!(
            UnapprovedBlock::from_proto(UnapprovedBlockProto {
                candidate: None,
                timestamp: 0,
                duration: 0,
            }),
            Err("Missing candidate field".to_string())
        );
    }

    #[test]
    fn events_round_trip_through_proto() {
        let produce = Event::Produce(produce_event());
        assert_eq!(Event::from_proto(produce.to_proto()).unwrap(), produce);

        let consume = Event::Consume(consume_event());
        assert_eq!(Event::from_proto(consume.to_proto()).unwrap(), consume);

        let comm = Event::Comm(CommEvent {
            consume: consume_event(),
            produces: vec![produce_event()],
            peeks: vec![Peek { channel_index: 4 }],
        });
        assert_eq!(Event::from_proto(comm.to_proto()).unwrap(), comm);
    }

    fn resting_event(indices: [u64; 2]) -> RestingCommEvent {
        RestingCommEvent {
            consume: ConsumeEvent {
                channels_hashes: vec![Bytes::from(vec![1; 32]), Bytes::from(vec![1; 32])],
                hash: Bytes::from(vec![2; 32]),
                persistent: true,
            },
            continuation_index: 3,
            ordered_produces: vec![
                ProduceEvent {
                    channels_hash: Bytes::from(vec![1; 32]),
                    hash: Bytes::from(vec![4; 32]),
                    persistent: false,
                    times_repeated: 0,
                    is_deterministic: true,
                    output_value: Vec::new(),
                    failed: false,
                },
                ProduceEvent {
                    channels_hash: Bytes::from(vec![1; 32]),
                    hash: Bytes::from(vec![4; 32]),
                    persistent: false,
                    times_repeated: 0,
                    is_deterministic: true,
                    output_value: Vec::new(),
                    failed: false,
                },
            ],
            ordered_data_indices: indices.to_vec(),
            peeks: vec![Peek { channel_index: 1 }],
        }
    }

    #[test]
    fn resting_event_roundtrip_preserves_repeated_channel_bind_order() {
        let forward = Event::RestingComm(resting_event([1, 2]));
        let reversed = Event::RestingComm(resting_event([2, 1]));
        assert_eq!(Event::from_proto(forward.to_proto()).unwrap(), forward);
        assert_ne!(forward.to_proto().encode_to_vec(), reversed.to_proto().encode_to_vec());
    }

    #[test]
    fn legacy_produce_event_wire_is_unchanged_by_resting_arm() {
        let event = Event::Produce(ProduceEvent {
            channels_hash: Bytes::from_static(&[1]),
            hash: Bytes::from_static(&[2]),
            persistent: false,
            times_repeated: 0,
            is_deterministic: false,
            output_value: Vec::new(),
            failed: false,
        });
        let expected = [0x0a, 0x06, 0x0a, 0x01, 0x01, 0x12, 0x01, 0x02];
        assert_eq!(event.to_proto().encode_to_vec(), expected);
        assert_eq!(Event::from_proto(EventProto::decode(expected.as_slice()).unwrap()).unwrap(), event);
    }

    #[test]
    fn resting_event_refuses_malformed_ordered_address() {
        let mut wire = resting_event([1, 2]).to_proto();
        wire.ordered_data_indices.pop();
        assert!(RestingCommEvent::from_proto(wire).is_err());

        let mut wire = resting_event([1, 2]).to_proto();
        wire.continuation_index = i32::MAX as u64 + 1;
        assert!(RestingCommEvent::from_proto(wire).is_err());

        let mut wire = resting_event([1, 2]).to_proto();
        wire.peeks = vec![PeekProto { channel_index: 1 }, PeekProto { channel_index: 1 }];
        assert!(RestingCommEvent::from_proto(wire).is_err());

        let mut wire = resting_event([1, 2]).to_proto();
        wire.ordered_produces[0].times_repeated = 1;
        assert!(RestingCommEvent::from_proto(wire).is_err());
    }

    #[test]
    fn legacy_processed_deploy_refuses_cost_v2_resting_event() {
        let mut wire = ProcessedDeploy::empty(signed_deploy()).unwrap().to_proto();
        wire.deploy_log.push(Event::RestingComm(resting_event([1, 2])).to_proto());
        assert!(ProcessedDeploy::from_proto(wire).is_err());
    }

    proptest::proptest! {
        #[test]
        fn resting_event_order_is_injective_for_distinct_indices(left in 0u64..4096, right in 0u64..4096) {
            proptest::prop_assume!(left != right);
            let forward = Event::RestingComm(resting_event([left, right]));
            let reversed = Event::RestingComm(resting_event([right, left]));
            proptest::prop_assert_eq!(Event::from_proto(forward.to_proto()).unwrap(), forward.clone());
            proptest::prop_assert_ne!(forward.to_proto().encode_to_vec(), reversed.to_proto().encode_to_vec());
        }
    }

    #[test]
    fn empty_event_proto_is_rejected() {
        assert_eq!(
            Event::from_proto(EventProto {
                event_instance: None,
            }),
            Err("Received malformed Event: None".to_string())
        );
    }

    #[test]
    fn processed_deploy_round_trips_and_derives_values() {
        let deploy = signed_deploy();
        let mut processed = ProcessedDeploy::empty(deploy.clone()).unwrap();
        processed.cost = PCost { cost: 100 };
        processed.deploy_log = vec![Event::Produce(produce_event())];
        processed.is_failed = true;
        processed.system_deploy_error = Some("boom".to_string());
        assert_eq!(
            ProcessedDeploy::from_proto(processed.clone().to_proto()).unwrap(),
            processed
        );

        let mut unsupported = processed.clone().to_proto();
        unsupported.native_cost_evidence = Some(vec![1, 2, 3].into());
        assert!(ProcessedDeploy::from_proto(unsupported).is_err());

        let empty = ProcessedDeploy::empty(deploy.clone()).unwrap();
        assert_eq!(empty.cost, PCost { cost: 0 });
        assert!(!empty.is_failed);
        assert_eq!(empty.system_deploy_error, None);
        assert_eq!(
            ProcessedDeploy::from_proto(empty.clone().to_proto()).unwrap(),
            empty
        );

        let expected_refund = (deploy.data.phlo_limit - 100).max(0) * deploy.data.phlo_price;
        assert_eq!(processed.refund_amount(), expected_refund);

        let info = processed.clone().to_deploy_info();
        assert_eq!(info.term, deploy.data.term);
        assert_eq!(info.cost, 100);
        assert!(info.errored);
        assert_eq!(info.system_deploy_error, "boom".to_string());
        assert_eq!(info.sig, hex::encode(&deploy.sig));
    }

    #[test]
    fn system_deploy_data_round_trips_all_variants() {
        let slash = SystemDeployData::create_slash(
            Bytes::from_static(b"invalid-block"),
            PublicKey::from_bytes(b"issuer"),
            5,
        );
        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(slash.clone())).unwrap(),
            slash
        );

        let close = SystemDeployData::create_close();
        assert_eq!(close, SystemDeployData::CloseBlockSystemDeployData);
        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(close.clone())).unwrap(),
            close
        );

        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(SystemDeployData::Empty)),
            Err("Missing system deploy field".to_string())
        );
    }

    #[test]
    fn processed_system_deploy_round_trips_and_folds() {
        let succeeded = ProcessedSystemDeploy::Succeeded {
            event_list: vec![Event::Consume(consume_event())],
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
        };
        assert_eq!(
            ProcessedSystemDeploy::from_proto(succeeded.clone().to_proto()).unwrap(),
            succeeded
        );
        assert!(!succeeded.clone().failed());
        assert_eq!(succeeded.fold(|events| events.len(), |_, _| 999), 1);

        let failed = ProcessedSystemDeploy::Failed {
            event_list: vec![],
            error_msg: "went wrong".to_string(),
        };
        assert_eq!(
            ProcessedSystemDeploy::from_proto(failed.clone().to_proto()).unwrap(),
            failed
        );
        assert!(failed.clone().failed());
        assert_eq!(
            failed.fold(|_| "ok".to_string(), |_, msg| msg),
            "went wrong".to_string()
        );
    }

    #[test]
    fn deploy_data_encode_decode_round_trips_expiration() {
        let with_expiration = DeployData {
            term: "Nil".to_string(),
            time_stamp: 1,
            phlo_price: 2,
            phlo_limit: 3,
            valid_after_block_number: 4,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(500),
        };
        assert_eq!(
            DeployData::decode(DeployData::encode(with_expiration.clone())).unwrap(),
            with_expiration
        );

        let without_expiration = DeployData {
            expiration_timestamp: None,
            ..with_expiration.clone()
        };
        assert_eq!(
            DeployData::decode(DeployData::encode(without_expiration.clone())).unwrap(),
            without_expiration
        );

        assert!(DeployData::decode(vec![0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn deploy_data_expiration_and_charge_helpers() {
        let mut deploy = DeployData {
            term: "Nil".to_string(),
            time_stamp: 1,
            phlo_price: 7,
            phlo_limit: 11,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        };
        assert_eq!(deploy.total_phlo_charge(), 77);
        assert!(!deploy.has_expiration());
        assert!(!deploy.is_expired_at(i64::MAX));

        deploy.expiration_timestamp = Some(100);
        assert!(deploy.has_expiration());
        assert!(!deploy.is_expired_at(100));
        assert!(deploy.is_expired_at(101));
    }

    #[test]
    fn signed_deploy_data_survives_proto_round_trip() {
        let signed = signed_deploy();
        let round_tripped = DeployData::from_proto(DeployData::to_proto_ref(&signed)).unwrap();
        assert_eq!(round_tripped.data, signed.data);
        assert_eq!(round_tripped.sig, signed.sig);
        assert_eq!(round_tripped.pk, signed.pk);
    }

    #[test]
    fn deploy_data_from_proto_rejects_unknown_algorithm_and_bad_signature() {
        let signed = signed_deploy();

        let mut unknown_alg = DeployData::to_proto_ref(&signed);
        unknown_alg.sig_algorithm = "no-such-alg".to_string();
        assert!(DeployData::from_proto(unknown_alg)
            .unwrap_err()
            .contains("Unknown signature algorithm"));

        let mut tampered = DeployData::to_proto_ref(&signed);
        tampered.term = format!("{} ", tampered.term);
        assert!(DeployData::from_proto(tampered).is_err());
    }

    #[test]
    fn store_node_key_round_trips_with_and_without_index() {
        let with_index = (hash32(1), Some(7u8));
        assert_eq!(
            StoreNodeKey::from_proto(StoreNodeKey::to_proto(&with_index)),
            with_index
        );

        let without_index = (hash32(2), None);
        assert_eq!(
            StoreNodeKey::from_proto(StoreNodeKey::to_proto(&without_index)),
            without_index
        );
    }

    #[test]
    fn store_items_messages_round_trip() {
        let request = StoreItemsMessageRequest {
            start_path: vec![(hash32(1), Some(0)), (hash32(2), None)],
            skip: 5,
            take: 10,
        };
        assert_eq!(
            StoreItemsMessageRequest::from_proto(request.clone().to_proto()),
            request
        );

        let message = StoreItemsMessage {
            start_path: vec![(hash32(1), None)],
            last_path: vec![(hash32(2), Some(3))],
            history_items: vec![(hash32(3), Bytes::from_static(b"history"))],
            data_items: vec![(hash32(4), Bytes::from_static(b"data"))],
        };
        assert_eq!(
            StoreItemsMessage::from_proto(message.clone().to_proto()),
            message
        );

        let pretty = message.pretty();
        assert!(pretty.starts_with("StoreItemsMessage(history: 1, data: 1"));
    }

    #[test]
    fn casper_message_wrappers_tag_the_right_variant() {
        let hash = Bytes::from_static(b"h");
        assert_eq!(
            CasperMessage::from_has_block(HasBlockProto { hash: hash.clone() }),
            CasperMessage::HasBlock(HasBlock { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_has_block_request(HasBlockRequestProto { hash: hash.clone() }),
            CasperMessage::HasBlockRequest(HasBlockRequest { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_block_request(BlockRequestProto { hash: hash.clone() }),
            CasperMessage::BlockRequest(BlockRequest { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_fork_choice_tip_request(ForkChoiceTipRequestProto {}),
            CasperMessage::ForkChoiceTipRequest(ForkChoiceTipRequest)
        );

        let block = get_random_block_default();
        assert_eq!(
            CasperMessage::from_block_message(block.to_proto()).unwrap(),
            CasperMessage::BlockMessage(block)
        );

        let request = FloorCacheRequest {
            hashes: vec![hash.clone()],
        };
        assert_eq!(
            CasperMessage::from_floor_cache_request(request.clone().to_proto()),
            CasperMessage::FloorCacheRequest(request)
        );
    }

    // --- Snapshot chunk-fetch roundtrips (Wave 3, Phase 7b-1) ---

    #[test]
    fn merkle_proof_step_proto_roundtrip() {
        for is_right in [false, true] {
            let step = MerkleProofStep {
                sibling_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
                is_sibling_right: is_right,
            };
            let roundtripped = MerkleProofStep::from_proto(step.clone().to_proto());
            assert_eq!(roundtripped, step);
        }
    }

    #[test]
    fn get_snapshot_chunk_request_proto_roundtrip() {
        let req = GetSnapshotChunkRequest {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            chunk_index: 42,
        };
        let roundtripped = GetSnapshotChunkRequest::from_proto(req.clone().to_proto());
        assert_eq!(roundtripped, req);
    }

    #[test]
    fn snapshot_chunk_response_proto_roundtrip() {
        let resp = SnapshotChunkResponse {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            chunk_index: 7,
            chunk_bytes: prost::bytes::Bytes::from(vec![0x22; 1024]),
            chunk_hash: prost::bytes::Bytes::from(vec![0x33; 32]),
            merkle_root: prost::bytes::Bytes::from(vec![0x44; 32]),
            chunk_count: 16,
            merkle_proof: vec![
                MerkleProofStep {
                    sibling_hash: prost::bytes::Bytes::from(vec![0x55; 32]),
                    is_sibling_right: true,
                },
                MerkleProofStep {
                    sibling_hash: prost::bytes::Bytes::from(vec![0x66; 32]),
                    is_sibling_right: false,
                },
            ],
        };
        let roundtripped = SnapshotChunkResponse::from_proto(resp.clone().to_proto());
        assert_eq!(roundtripped, resp);
    }

    #[test]
    fn casper_message_from_get_snapshot_chunk_request() {
        let proto = GetSnapshotChunkRequestProto {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            chunk_index: 3,
        };
        let msg = CasperMessage::from_get_snapshot_chunk_request(proto.clone());
        match msg {
            CasperMessage::GetSnapshotChunkRequest(inner) => {
                assert_eq!(inner.block_hash, proto.block_hash);
                assert_eq!(inner.chunk_index, proto.chunk_index);
            }
            _ => panic!("expected GetSnapshotChunkRequest variant"),
        }
    }

    #[test]
    fn casper_message_from_snapshot_chunk_response() {
        let proto = SnapshotChunkResponseProto {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            chunk_index: 0,
            chunk_bytes: prost::bytes::Bytes::new(),
            chunk_hash: prost::bytes::Bytes::new(),
            merkle_root: prost::bytes::Bytes::new(),
            chunk_count: 1,
            merkle_proof: Vec::new(),
        };
        let msg = CasperMessage::from_snapshot_chunk_response(proto.clone());
        assert!(matches!(msg, CasperMessage::SnapshotChunkResponse(_)));
    }

    // --- HasSnapshot lookup pair roundtrips (Wave 3, PR 3.2) ---

    #[test]
    fn has_snapshot_request_proto_roundtrip() {
        let req = HasSnapshotRequest {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
        };
        let roundtripped = HasSnapshotRequest::from_proto(req.clone().to_proto());
        assert_eq!(roundtripped, req);
    }

    #[test]
    fn has_snapshot_proto_roundtrip() {
        let resp = HasSnapshot {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            merkle_root: prost::bytes::Bytes::from(vec![0x22; 32]),
            chunk_count: 5,
        };
        let roundtripped = HasSnapshot::from_proto(resp.clone().to_proto());
        assert_eq!(roundtripped, resp);
    }

    #[test]
    fn casper_message_from_has_snapshot_request() {
        let proto = HasSnapshotRequestProto {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
        };
        let msg = CasperMessage::from_has_snapshot_request(proto.clone());
        match msg {
            CasperMessage::HasSnapshotRequest(inner) => {
                assert_eq!(inner.block_hash, proto.block_hash);
            }
            _ => panic!("expected HasSnapshotRequest variant"),
        }
    }

    #[test]
    fn casper_message_from_has_snapshot() {
        let proto = HasSnapshotProto {
            block_hash: prost::bytes::Bytes::from(vec![0x11; 32]),
            merkle_root: prost::bytes::Bytes::from(vec![0x22; 32]),
            chunk_count: 7,
        };
        let msg = CasperMessage::from_has_snapshot(proto.clone());
        match msg {
            CasperMessage::HasSnapshot(inner) => {
                assert_eq!(inner.block_hash, proto.block_hash);
                assert_eq!(inner.merkle_root, proto.merkle_root);
                assert_eq!(inner.chunk_count, proto.chunk_count);
            }
            _ => panic!("expected HasSnapshot variant"),
        }
    }

    // --- WAL payload fetch roundtrips (Wave 3, PR 3.3) ---------

    #[test]
    fn get_wal_payload_request_proto_roundtrip() {
        let req = GetWalPayloadRequest {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let roundtripped = GetWalPayloadRequest::from_proto(req.clone().to_proto());
        assert_eq!(roundtripped, req);
    }

    #[test]
    fn wal_payload_response_proto_roundtrip() {
        let resp = WalPayloadResponse {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_bytes: prost::bytes::Bytes::from(vec![0xCD; 1024]),
        };
        let roundtripped = WalPayloadResponse::from_proto(resp.clone().to_proto());
        assert_eq!(roundtripped, resp);
    }

    #[test]
    fn casper_message_from_get_wal_payload_request() {
        let proto = GetWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let msg = CasperMessage::from_get_wal_payload_request(proto.clone());
        match msg {
            CasperMessage::GetWalPayloadRequest(inner) => {
                assert_eq!(inner.payload_hash, proto.payload_hash);
            }
            _ => panic!("expected GetWalPayloadRequest variant"),
        }
    }

    #[test]
    fn casper_message_from_wal_payload_response() {
        let proto = WalPayloadResponseProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_bytes: prost::bytes::Bytes::from(vec![0xCD; 256]),
        };
        let msg = CasperMessage::from_wal_payload_response(proto.clone());
        match msg {
            CasperMessage::WalPayloadResponse(inner) => {
                assert_eq!(inner.payload_hash, proto.payload_hash);
                assert_eq!(inner.payload_bytes, proto.payload_bytes);
            }
            _ => panic!("expected WalPayloadResponse variant"),
        }
    }

    // --- HasWalPayload lookup pair roundtrips (Wave 3, PR 3.4) ---

    #[test]
    fn has_wal_payload_request_proto_roundtrip() {
        let req = HasWalPayloadRequest {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let roundtripped = HasWalPayloadRequest::from_proto(req.clone().to_proto());
        assert_eq!(roundtripped, req);
    }

    #[test]
    fn has_wal_payload_proto_roundtrip() {
        let resp = HasWalPayload {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_size: 2048,
        };
        let roundtripped = HasWalPayload::from_proto(resp.clone().to_proto());
        assert_eq!(roundtripped, resp);
    }

    #[test]
    fn casper_message_from_has_wal_payload_request() {
        let proto = HasWalPayloadRequestProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
        };
        let msg = CasperMessage::from_has_wal_payload_request(proto.clone());
        match msg {
            CasperMessage::HasWalPayloadRequest(inner) => {
                assert_eq!(inner.payload_hash, proto.payload_hash);
            }
            _ => panic!("expected HasWalPayloadRequest variant"),
        }
    }

    #[test]
    fn casper_message_from_has_wal_payload() {
        let proto = HasWalPayloadProto {
            payload_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            payload_size: 7,
        };
        let msg = CasperMessage::from_has_wal_payload(proto.clone());
        match msg {
            CasperMessage::HasWalPayload(inner) => {
                assert_eq!(inner.payload_hash, proto.payload_hash);
                assert_eq!(inner.payload_size, proto.payload_size);
            }
            _ => panic!("expected HasWalPayload variant"),
        }
    }
}

#[cfg(test)]
mod cosigned_tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;

    use super::*;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            time_stamp: 0,
            phlo_price: 0,
            phlo_limit: 0,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        }
    }

    fn signed_deploy(data: DeployData) -> Signed<DeployData> {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (sk, _) = alg.new_key_pair();
        Signed::create(data, alg, sk).expect("signed deploy")
    }

    /// Consensus-fork guard for the Workstream-B ground-`g` / quote-`#P`
    /// signature-atom split. The split adds `SigAtom.atom_kind` and the
    /// `Sig::Ground`/`Sig::Quote` runtime variants, but NONE of that may
    /// enter the deploy-signature preimage — otherwise every legacy
    /// single-signature deploy on chain would re-hash to a different
    /// `deploy_id` and the network would hard-fork.
    ///
    /// The preimage is `DeployData::_to_proto(..).encode_to_vec()`, and the
    /// signing digest is `Signed::signature_hash(alg, preimage)`. Both the
    /// preimage and the digest below were captured from the PRE-split code
    /// (the `_to_proto` body emits only term/timestamp/phlo_price/phlo_limit/
    /// valid_after_block_number/shard_id/expiration_timestamp and never a
    /// `SigAtom`/`atom_kind`/`sig_algebra`). If this assertion ever fails,
    /// the preimage changed — STOP: that is a consensus fork, and the fix is
    /// to exclude the offending field from `_to_proto`, never to update the
    /// pinned digest.
    #[test]
    fn deploy_signature_hash_excludes_retired_phlo_fields() {
        use prost::Message;

        // D3 (DR-9, fresh-genesis): the deploy-signature preimage NO LONGER
        // carries phloPrice (tag 7) / phloLimit (tag 8) — those tags are
        // reserved and `_to_proto` never emits them. This re-pins the preimage
        // and digest for the post-D3 single-sig wire shape. The retired tag
        // bytes (`3802...40...` for tags 7/8) MUST be absent.
        //
        // Fixed legacy single-sig deploy: term="Nil", timestamp=0,
        // valid_after_block_number=0, shard_id="root", no expiration.
        let data = deploy_data();

        let preimage = DeployData::_to_proto(data.clone()).encode_to_vec();
        // Post-D3 preimage. Field tags: 2=term("Nil"), 11=shardId("root").
        // timestamp/valid_after_block_number default to 0 (omitted). No tag 7
        // (phloPrice) or tag 8 (phloLimit), no SigAtom/atom_kind/sig_algebra.
        const PINNED_PREIMAGE_HEX: &str = "12034e696c5a04726f6f74";
        assert_eq!(
            hex::encode(&preimage),
            PINNED_PREIMAGE_HEX,
            "deploy-signature preimage changed — consensus fork risk; \
             do NOT update the pin, exclude the offending field from _to_proto"
        );
        // The retired phloPrice/phloLimit tag-7/8 bytes must NOT appear.
        assert!(
            !hex::encode(&preimage).contains("3802")
                && !preimage.windows(2).any(|w| w == [0x40, 0x05]),
            "retired phloPrice/phloLimit bytes must be absent from the D3 preimage"
        );

        // Blake2b256 digest of the post-D3 preimage (secp256k1 path).
        const PINNED_GOLDEN_DIGEST_HEX: &str =
            "c2ac266875edd634b52a2c7272ea7e1e06d5a33a1864ad90a471d56aa89b45df";
        let digest = Signed::<DeployData>::signature_hash(&Secp256k1::name(), preimage);
        assert_eq!(
            hex::encode(&digest),
            PINNED_GOLDEN_DIGEST_HEX,
            "post-D3 deploy signature_hash changed — re-pin only if the wire \
             shape intentionally changed (fresh-genesis)"
        );
    }

    /// A single-signature deploy serialized via the legacy `to_proto` path
    /// must NOT carry the multi-sig/algebra fields: `sig_algebra` is `None`
    /// and `cosigners` is empty. This is the structural complement to the
    /// golden-vector pin — it asserts the split did not start emitting a
    /// `SigCompound`/`SigAtom` onto the legacy single-sig wire shape.
    #[test]
    fn single_sig_to_proto_omits_sig_algebra_and_cosigners() {
        let signed = signed_deploy(deploy_data());
        let proto = DeployData::to_proto(signed);
        assert!(
            proto.sig_algebra.is_none(),
            "single-sig deploy must not emit sig_algebra"
        );
        assert!(
            proto.cosigners.is_empty(),
            "single-sig deploy must not emit cosigners"
        );
    }

    // D3 (DR-9): the escrow unit tests (`checked_total_phlo_charge_*`,
    // `refund_amount_is_bounded_by_valid_escrow`,
    // `settlement_edge_cases_are_total_and_deterministic`) and the
    // `refund_amount_property_is_bounded_by_valid_escrow` proptest are removed
    // with the escrow arithmetic they exercised. A deploy's cost is the
    // per-COMM token count (validated by the runtime/replay equivalence in the
    // `casper`/`rholang` crates), debited once from Σ⟦s⟧ — there is no per-deploy
    // charge/refund to bound.

    fn fresh_atom_signing(payload: &DeployData) -> (crate::casper::SigAtom, Vec<u8>) {
        let secp = Secp256k1;
        let (sk, pk) = secp.new_key_pair();
        let serialized = DeployData::_to_proto(payload.clone()).encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let sig = secp.sign(&hash, &sk.bytes);
        let pk_bytes_vec: Vec<u8> = pk.bytes.to_vec();
        (
            crate::casper::SigAtom {
                pk: pk.bytes.clone().into(),
                sig: prost::bytes::Bytes::from(sig),
                sig_algorithm: Secp256k1::name(),
                ..Default::default()
            },
            pk_bytes_vec,
        )
    }

    fn empty_atom() -> crate::casper::SigAtom {
        let secp = Secp256k1;
        let (_, pk) = secp.new_key_pair();
        crate::casper::SigAtom {
            pk: pk.bytes.into(),
            sig: prost::bytes::Bytes::new(),
            sig_algorithm: Secp256k1::name(),
            ..Default::default()
        }
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_tensor_validates_both_branches() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                    })),
                },
            ))),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Tensor with two valid signers must verify");
        assert_eq!(cosigned.signers().len(), 2);
    }

    /// F-A ingress reject (c): a `Plus` (⊕) `sig_algebra` is a VALUE/CAPABILITY
    /// connective, NOT a funding-signature former — it is now REJECTED at the
    /// deploy-decode boundary BEFORE any branch-witness validation (previously
    /// this test asserted the Plus was processed; F-A reclassifies ⊕ to the
    /// capability layer).
    #[test]
    fn from_proto_cosigned_sig_algebra_plus_rejected_at_ingress() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let atom_b_unsigned = empty_atom(); // not chosen, sig is empty
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Plus(Box::new(
                crate::casper::SigPlus {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(
                            atom_b_unsigned,
                        )),
                    })),
                    chosen_branch: 0, // left
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Plus (⊕) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Plus") && err.contains("not a funding-signature former"),
            "error must name the rejected ⊕/Plus connective: {}",
            err
        );
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_threshold_2_of_3_satisfied() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let atom_c_unsigned = empty_atom();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 2,
                    members: vec![
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(
                                atom_c_unsigned,
                            )),
                        },
                    ],
                },
            )),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Threshold 2-of-3 with 2 valid sigs must verify");
        assert_eq!(cosigned.signers().len(), 3);
    }

    /// F-A ingress reject (c): a `WhyNot` (?) `sig_algebra` is a
    /// VALUE/CAPABILITY connective and is REJECTED at the deploy-decode boundary
    /// — regardless of whether its wrapped atom is present, absent, valid, or
    /// invalid. Previously the absent / present-valid / present-invalid WhyNot
    /// cases each took a different decode branch; F-A reclassifies `?` to the
    /// capability layer, so all of them now fail at ingress with the SAME
    /// connective-rejection error (verified here for the absent and the
    /// present-invalid cases).
    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_rejected_at_ingress() {
        // (i) absent (empty) wrapped atom.
        let payload = deploy_data();
        let absent_atom = empty_atom();
        let algebra_absent = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(absent_atom)),
                },
            ))),
        };
        let err_absent = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra_absent)
            .expect_err("WhyNot (?) is a capability connective, rejected at ingress");
        assert!(
            err_absent.contains("WhyNot") && err_absent.contains("not a funding-signature former"),
            "error must name the rejected ?/WhyNot connective: {}",
            err_absent
        );

        // (ii) present-but-wrong-payload wrapped atom — still rejected at the
        // connective boundary BEFORE any signature verification runs.
        let payload2 = deploy_data();
        let other_payload = DeployData {
            term: "other-payload".to_string(),
            ..deploy_data()
        };
        let (atom_invalid, _) = fresh_atom_signing(&other_payload);
        let algebra_invalid = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom_invalid)),
                },
            ))),
        };
        let err_invalid =
            DeployData::from_proto_cosigned_with_sig_algebra(payload2, &algebra_invalid)
                .expect_err("present-invalid WhyNot is still rejected at the connective boundary");
        assert!(
            err_invalid.contains("WhyNot")
                && !err_invalid.contains("failed signature verification"),
            "ingress reject must fire BEFORE signature verification: {}",
            err_invalid
        );
    }

    /// F-A ingress reject (c): a `Plus` (⊕) `sig_algebra` is rejected at the
    /// connective boundary BEFORE the `chosen_branch` range check — so even a
    /// structurally invalid `chosen_branch` surfaces the connective-rejection,
    /// not the branch error (previously this asserted the `chosen_branch`
    /// message). The funding grammar has no ⊕ former at all.
    #[test]
    fn from_proto_cosigned_sig_algebra_plus_rejected_before_chosen_branch_check() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Plus(Box::new(
                crate::casper::SigPlus {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                    })),
                    chosen_branch: 2, // invalid — but the connective is rejected first
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Plus (⊕) rejected at ingress before chosen_branch validation");
        assert!(
            err.contains("Plus") && err.contains("not a funding-signature former"),
            "ingress reject must fire before the chosen_branch check: {}",
            err
        );
    }

    #[test]
    fn historical_single_signer_threshold_is_wire_only_not_authority() {
        let mut wire = ProcessedDeploy::empty(signed_deploy(deploy_data()))
            .unwrap()
            .to_proto();
        wire.deploy.as_mut().unwrap().cosigner_threshold = 2;
        let decoded = ProcessedDeploy::from_proto(wire.clone()).unwrap();
        assert_eq!(decoded.wire_threshold(), 2);
        assert_eq!(decoded.threshold(), 0);
        assert_eq!(decoded.to_proto(), wire);
    }

    // =================================================================
    // F-A funding/capability separation — INGRESS REJECT (c) tests.
    //
    // `docs/theory/cost-accounting-impl/f-a-funding-vs-capability-separation.md`
    // §3/§6: the deploy-decode path
    // (`from_proto_cosigned_with_sig_algebra`) REJECTS the five value/capability
    // type-logic connectives (`Plus` ⊕ / `With` & / `Bang` ! / `WhyNot` ? /
    // `Lolly` ⊸) — they are NOT funding-signature formers (§App-A `g|#P|s∘s`).
    // `Atom`, `Tensor` (the funding `And` ∘) and `Threshold` (a k-of-N
    // admission-boundary quorum, F-A Threshold=(A)) are KEPT. Plus + WhyNot
    // rejection are covered by the two repaired tests above; the remaining three
    // connectives (`Bang`/`Lolly`/`With`) + nesting + the Threshold-kept and
    // flat-N-of-N no-regression cases are pinned here.
    // =================================================================

    fn atom_compound(payload: &DeployData) -> crate::casper::SigCompound {
        let (atom, _) = fresh_atom_signing(payload);
        crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
        }
    }

    /// `Bang` (!) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_bang_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Bang(Box::new(
                crate::casper::SigBang {
                    inner: Some(Box::new(atom_compound(&payload))),
                    uses_bound: 0,
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Bang (!) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Bang") && err.contains("not a funding-signature former"),
            "error must name the rejected !/Bang connective: {}",
            err
        );
    }

    /// `Lolly` (⊸) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_lolly_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Lolly(Box::new(
                crate::casper::SigLolly {
                    from: Some(Box::new(atom_compound(&payload))),
                    to: Some(Box::new(atom_compound(&payload))),
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Lolly (⊸) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Lolly") && err.contains("not a funding-signature former"),
            "error must name the rejected ⊸/Lolly connective: {}",
            err
        );
    }

    /// `With` (&) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_with_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::With(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(atom_compound(&payload))),
                    right: Some(Box::new(atom_compound(&payload))),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("With (&) is a capability connective, rejected at ingress");
        assert!(
            err.contains("With") && err.contains("not a funding-signature former"),
            "error must name the rejected &/With connective: {}",
            err
        );
    }

    /// A capability connective NESTED inside an otherwise-funding `Tensor`
    /// (`Atom ∘ Bang`) is still caught — the reject walk recurses through the
    /// tensor's sides.
    #[test]
    fn from_proto_cosigned_sig_algebra_connective_nested_in_tensor_rejected() {
        let payload = deploy_data();
        let bang = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Bang(Box::new(
                crate::casper::SigBang {
                    inner: Some(Box::new(atom_compound(&payload))),
                    uses_bound: 0,
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(atom_compound(&payload))),
                    right: Some(Box::new(bang)),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("a Bang nested in a Tensor must still be rejected");
        assert!(
            err.contains("Bang") && err.contains("not a funding-signature former"),
            "nested-connective reject must name the offending connective: {}",
            err
        );
    }

    /// A capability connective NESTED inside a `Threshold` member is still
    /// caught — the reject walk recurses through threshold members (Threshold
    /// itself is kept, but its members must be funding-grammar).
    #[test]
    fn from_proto_cosigned_sig_algebra_connective_nested_in_threshold_member_rejected() {
        let payload = deploy_data();
        let lolly = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Lolly(Box::new(
                crate::casper::SigLolly {
                    from: Some(Box::new(atom_compound(&payload))),
                    to: Some(Box::new(atom_compound(&payload))),
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 1,
                    members: vec![atom_compound(&payload), lolly],
                },
            )),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("a Lolly nested in a Threshold member must still be rejected");
        assert!(
            err.contains("Lolly") && err.contains("not a funding-signature former"),
            "nested-in-threshold reject must name the offending connective: {}",
            err
        );
    }

    /// F-A Threshold=(A): a `Threshold` k-of-N (with funding-grammar atom
    /// members) is STILL ACCEPTED — it is an admission-boundary quorum, lowered
    /// to a flat `Cosigned` + scalar `cosigner_threshold`, NOT rejected by the
    /// ingress guard. (Mirrors the kept Tensor case which the existing
    /// `..._tensor_validates_both_branches` test already covers.)
    #[test]
    fn from_proto_cosigned_sig_algebra_threshold_still_accepted_post_f_a() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let atom_c_unsigned = empty_atom();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 2,
                    members: vec![
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(
                                atom_c_unsigned,
                            )),
                        },
                    ],
                },
            )),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Threshold 2-of-3 (funding atoms) must still verify post-F-A");
        assert_eq!(cosigned.signers().len(), 3);
        assert_eq!(cosigned.cosigner_threshold(), 2);
    }

    /// No-regression: a plain flat N-of-N deploy (`cosigners[]`, NO
    /// `sig_algebra`) decodes through the legacy `from_proto_cosigned` path
    /// exactly as before F-A (the ingress reject only touches the `sig_algebra`
    /// branch). Pairs with `single_sig_to_proto_omits_sig_algebra_and_cosigners`
    /// (single-sig wire shape) and the `deploy_signature_hash_excludes_*` golden
    /// pin (single-sig byte identity).
    #[test]
    fn flat_n_of_n_without_sig_algebra_still_decodes_post_f_a() {
        use prost::Message;
        let payload = deploy_data();
        let serialized = DeployData::_to_proto(payload.clone()).encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let secp = Secp256k1;

        let (sk_primary, pk_primary) = secp.new_key_pair();
        let (sk_co, pk_co) = secp.new_key_pair();
        // Start from the exact signed legacy payload, including client phlo
        // price/limit, then attach only the signatures and cosigner metadata.
        let mut proto = DeployData::_to_proto(payload.clone());
        proto.deployer = pk_primary.bytes.clone().into();
        proto.sig = prost::bytes::Bytes::from(secp.sign(&hash, &sk_primary.bytes));
        proto.sig_algorithm = Secp256k1::name();
        proto.cosigners = vec![CompoundSigner {
            pk: pk_co.bytes.clone().into(),
            sig: prost::bytes::Bytes::from(secp.sign(&hash, &sk_co.bytes)),
            sig_algorithm: Secp256k1::name(),
        }];
        proto.cosigner_threshold = 0; // N-of-N
        let cosigned = DeployData::from_proto_cosigned(proto)
            .expect("flat N-of-N (no sig_algebra) must decode unchanged post-F-A");
        assert_eq!(cosigned.signers().len(), 2);
        assert!(cosigned.is_compound());
    }

    #[test]
    fn sig_compound_analysis_and_drop_are_stack_safe_at_depth_4096() {
        std::thread::Builder::new()
            .name("sig-compound-pda".to_owned())
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut algebra = crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(empty_atom())),
                };
                for _ in 0..4_096 {
                    algebra = crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Threshold(
                            crate::casper::SigThreshold {
                                threshold: 1,
                                members: vec![algebra],
                            },
                        )),
                    };
                }

                DeployData::reject_capability_connectives(&algebra)
                    .expect("a threshold/atom chain is in the funding grammar");
                let (atoms, required, all_required) = DeployData::analyze_funding_algebra(&algebra)
                    .expect("the explicit-worklist analyzer accepts the chain");
                assert_eq!(atoms.len(), 1);
                assert_eq!(required, 1);
                assert!(!all_required);

                // `algebra` drops here on the same fixed-small-stack thread;
                // the generated recursive Box shape must use the custom PDA.
            })
            .expect("spawn fixed-stack signature-algebra probe")
            .join()
            .expect("signature-algebra analysis/drop must not grow native stack with depth");
    }
}
