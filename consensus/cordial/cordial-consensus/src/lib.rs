use std::collections::HashMap;

use bincode::Options;
use cordial_miners_core::crypto::{
    Blake2b256Hasher, CONTENT_HASH_VERSION, Hasher, hash_content, sign, verify,
};
use cordial_miners_core::{Block, BlockContent, BlockIdentity, NodeId};
use k256::ecdsa::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

mod execution;
mod node;
mod proposal;
mod runtime;
mod store;
pub use execution::{
    Application, CommittedExecution, CommittedExecutor, ExecutionCheckpoint, ExecutionReceipt,
    MAX_RECEIPT_BYTES, MAX_RECEIPT_DEPLOYS, Submission,
};
pub use node::{
    CordialNode, CordialQueries, NodeOptions, PeerNetwork, Progress, SUBMIT_PACKET_KIND,
    SYNC_PACKET_KIND,
};
pub use runtime::{BLOCK_PACKET_KIND, CordialIngressAdapter};
pub use store::{Admission, DurableBlocklace, EquivocationRecord, HistoryPage, StoreConfig};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Validator {
    pub public_key: Vec<u8>,
    pub weight: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChainSpec {
    pub network: String,
    pub shard: String,
    pub execution_genesis: [u8; 32],
    pub validators: Vec<Validator>,
    pub wavelength: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid Cordial execution receipt: {0}")]
    Execution(String),
    #[error("Invalid Cordial configuration: {0}")]
    Configuration(String),
    #[error("Cordial chain context does not match")]
    ChainMismatch,
    #[error("Invalid Cordial packet: {0}")]
    Packet(String),
    #[error("Cordial validator is not configured")]
    UnknownValidator,
    #[error("Cordial signature or content hash is invalid")]
    Authentication,
    #[error("Cordial storage error: {0}")]
    Storage(#[from] heed::Error),
    #[error("Cordial filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cordial store is already in use")]
    InUse,
    #[error("Cordial store is inconsistent: {0}")]
    Corrupt(String),
    #[error("Cordial pending-object capacity exceeded")]
    Capacity,
    #[error("Cordial native admission rejected the object")]
    Native(Vec<cordial_miners_core::consensus::InvalidBlock>),
}

pub const MAX_PACKET_BYTES: usize = 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
pub const PROFILE_ID: &str = "cordial-fixed-committee-ordered-rholang-v1";
const MAX_PREDECESSORS: usize = 1024;

#[derive(Clone)]
pub struct Chain {
    spec: ChainSpec,
    fingerprint: [u8; 32],
    weights: HashMap<NodeId, u64>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    chain: [u8; 32],
    initial: bool,
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Packet {
    version: u32,
    identity: BlockIdentity,
    predecessors: Vec<BlockIdentity>,
    payload: Vec<u8>,
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_PACKET_BYTES as u64)
        .reject_trailing_bytes()
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    codec()
        .serialize(value)
        .map_err(|error| Error::Packet(error.to_string()))
}

fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    if bytes.len() > MAX_PACKET_BYTES {
        return Err(Error::Packet("packet limit exceeded".into()));
    }
    codec()
        .deserialize(bytes)
        .map_err(|error| Error::Packet(error.to_string()))
}

impl Chain {
    pub fn new(mut spec: ChainSpec) -> Result<Self, Error> {
        if spec.network.is_empty()
            || spec.network.len() > 256
            || spec.shard.is_empty()
            || spec.shard.len() > 256
            || spec.validators.is_empty()
            || spec.validators.len() > 256
            || spec.wavelength < 3
            || spec.wavelength > 64
        {
            return Err(Error::Configuration(
                "invalid chain identity, membership size, or wavelength".into(),
            ));
        }
        spec.validators
            .sort_by(|left, right| left.public_key.cmp(&right.public_key));
        let mut weights = HashMap::new();
        for validator in &spec.validators {
            let key = VerifyingKey::from_sec1_bytes(&validator.public_key)
                .map_err(|_| Error::Configuration("invalid validator public key".into()))?;
            if key.to_sec1_bytes().as_ref() != validator.public_key || validator.weight == 0 {
                return Err(Error::Configuration(
                    "validator keys must be compressed and weights positive".into(),
                ));
            }
            if weights
                .insert(NodeId(validator.public_key.clone()), validator.weight)
                .is_some()
            {
                return Err(Error::Configuration("duplicate validator key".into()));
            }
        }
        let mut bytes = b"cordial-miners:chain:v2\0".to_vec();
        bytes.extend(encode(&PROFILE_ID)?);
        bytes.extend(encode(&spec)?);
        let fingerprint = Blake2b256Hasher.hash(&bytes);
        Ok(Self {
            spec,
            fingerprint,
            weights,
        })
    }

    pub fn spec(&self) -> &ChainSpec {
        &self.spec
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
    pub fn weights(&self) -> &HashMap<NodeId, u64> {
        &self.weights
    }

    pub fn build_block(
        &self,
        key: &SigningKey,
        predecessors: Vec<BlockIdentity>,
        data: Vec<u8>,
    ) -> Result<Block, Error> {
        if data.len() > MAX_PAYLOAD_BYTES || predecessors.len() > MAX_PREDECESSORS {
            return Err(Error::Packet("block limit exceeded".into()));
        }
        let creator = NodeId(key.verifying_key().to_sec1_bytes().to_vec());
        if !self.weights.contains_key(&creator) {
            return Err(Error::UnknownValidator);
        }
        let content = BlockContent {
            payload: self.payload_bytes(predecessors.is_empty(), data)?,
            predecessors: predecessors.into_iter().collect(),
        };
        let content_hash = hash_content(&content);
        let block = Block {
            identity: BlockIdentity {
                content_hash,
                creator,
                signature: sign(&content_hash, &key.to_bytes()),
            },
            content,
        };
        self.authenticate(&block)?;
        Ok(block)
    }

    pub(crate) fn payload_bytes(&self, initial: bool, data: Vec<u8>) -> Result<Vec<u8>, Error> {
        if data.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::Packet("block limit exceeded".into()));
        }
        if initial && !data.is_empty() {
            return Err(Error::Packet(
                "initial blocks cannot contain application data".into(),
            ));
        }
        encode(&Payload {
            chain: self.fingerprint,
            initial,
            data,
        })
    }

    pub fn encode_block(&self, block: &Block) -> Result<Vec<u8>, Error> {
        self.authenticate(block)?;
        let mut predecessors: Vec<_> = block.content.predecessors.iter().cloned().collect();
        predecessors.sort();
        encode(&Packet {
            version: CONTENT_HASH_VERSION,
            identity: block.identity.clone(),
            predecessors,
            payload: block.content.payload.clone(),
        })
    }

    pub fn decode_block(&self, bytes: &[u8]) -> Result<Block, Error> {
        let packet: Packet = decode(bytes)?;
        if packet.version != CONTENT_HASH_VERSION || packet.predecessors.len() > MAX_PREDECESSORS {
            return Err(Error::Packet(
                "unsupported version or predecessor limit".into(),
            ));
        }
        if packet
            .predecessors
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::Packet(
                "predecessors must have a canonical distinct order".into(),
            ));
        }
        let block = Block {
            identity: packet.identity,
            content: BlockContent {
                predecessors: packet.predecessors.into_iter().collect(),
                payload: packet.payload,
            },
        };
        self.authenticate(&block)?;
        Ok(block)
    }

    pub fn application_data(&self, block: &Block) -> Result<Vec<u8>, Error> {
        self.authenticate(block)?;
        Ok(decode::<Payload>(&block.content.payload)?.data)
    }

    fn authenticate(&self, block: &Block) -> Result<(), Error> {
        if block.content.payload.len() > MAX_PAYLOAD_BYTES + 64
            || block.content.predecessors.len() > MAX_PREDECESSORS
        {
            return Err(Error::Packet("block limit exceeded".into()));
        }
        if !self.weights.contains_key(&block.identity.creator) {
            return Err(Error::UnknownValidator);
        }
        for id in std::iter::once(&block.identity).chain(block.content.predecessors.iter()) {
            if id.creator.0.len() != 33 || id.signature.len() > 72 || id.signature.is_empty() {
                return Err(Error::Packet("invalid identity encoding".into()));
            }
        }
        if hash_content(&block.content) != block.identity.content_hash
            || !verify(
                &block.identity.content_hash,
                &block.identity.creator.0,
                &block.identity.signature,
            )
        {
            return Err(Error::Authentication);
        }
        let payload: Payload = decode(&block.content.payload)?;
        if payload.chain != self.fingerprint {
            return Err(Error::ChainMismatch);
        }
        if payload.data.len() > MAX_PAYLOAD_BYTES
            || payload.initial != block.content.predecessors.is_empty()
            || (payload.initial && !payload.data.is_empty())
        {
            return Err(Error::Packet(
                "invalid initial block or application payload".into(),
            ));
        }
        Ok(())
    }
}
