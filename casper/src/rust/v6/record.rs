use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use super::body::StateHash;

pub const RECORD_DIGEST_DOMAIN: &[u8] = b"f1r3node:v6-execution-record:v1";

pub type RecordDigest = [u8; 32];

pub(crate) fn domain_digest(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(domain.len() + bytes.len());
    input.extend_from_slice(domain);
    input.extend_from_slice(bytes);
    Blake2b256::hash(input)
        .try_into()
        .expect("Blake2b256 emits 32 bytes")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Footprint {
    pub channels: Vec<[u8; 32]>,
}

pub const RESERVED_CLOSE_UNCOVERED: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VoidReason {
    SlashRecords,
    UnreadableTerms,
    CloseFailed,
}

impl VoidReason {
    pub fn tag(self) -> u32 {
        match self {
            VoidReason::SlashRecords => 2,
            VoidReason::UnreadableTerms => 3,
            VoidReason::CloseFailed => 4,
        }
    }

    pub fn from_tag(tag: u32) -> Result<Option<Self>, String> {
        match tag {
            0 => Ok(None),
            // Tag 1 is void reason (a), the uncovered close. It is reserved for B6 (Decision 119); do not reuse it.
            RESERVED_CLOSE_UNCOVERED => Err("void reason 1, (a), is reserved for B6".to_string()),
            2 => Ok(Some(VoidReason::SlashRecords)),
            3 => Ok(Some(VoidReason::UnreadableTerms)),
            4 => Ok(Some(VoidReason::CloseFailed)),
            other => Err(format!("unknown void reason {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VoidReason::SlashRecords => "b",
            VoidReason::UnreadableTerms => "c",
            VoidReason::CloseFailed => "d",
        }
    }

    pub fn slashable(self) -> bool {
        matches!(self, VoidReason::SlashRecords | VoidReason::UnreadableTerms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriorStatus {
    Included,
    Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriorEntry {
    pub block_number: i64,
    pub position: u64,
    pub status: PriorStatus,
}

pub type Prior = std::collections::HashMap<Bytes, PriorStatus>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixMarks {
    pub void: Option<(Bytes, VoidReason)>,
    pub statuses: Vec<(Bytes, PriorEntry)>,
}

pub trait ExecutionRecord: Clone + std::fmt::Debug + PartialEq + Send + Sync + 'static {
    fn target(&self) -> &BlockHash;

    fn pre_root(&self) -> &StateHash;

    fn post_root(&self) -> &StateHash;

    fn envelope_ids(&self) -> &[Bytes];

    fn encode(&self) -> Bytes;

    fn decode(bytes: &[u8]) -> Result<Self, String>;

    fn encoded_len(&self) -> usize { self.encode().len() }

    fn footprint(&self) -> Option<Footprint> { None }

    fn void_reason(&self) -> Option<VoidReason> { None }

    fn ran(&self) -> Vec<bool> { vec![true; self.envelope_ids().len()] }

    fn digest(&self) -> RecordDigest {
        domain_digest(RECORD_DIGEST_DOMAIN, &self.encode())
    }
}
