use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use super::body::StateHash;

pub const RECORD_DIGEST_DOMAIN: &[u8] = b"f1r3node:v6-execution-record:v1";

pub type RecordDigest = [u8; 32];

pub trait ExecutionRecord: Clone + std::fmt::Debug + PartialEq + Send + Sync + 'static {
    fn target(&self) -> &BlockHash;

    fn pre_root(&self) -> &StateHash;

    fn post_root(&self) -> &StateHash;

    fn envelope_ids(&self) -> &[Bytes];

    fn encode(&self) -> Bytes;

    fn decode(bytes: &[u8]) -> Result<Self, String>;

    fn encoded_len(&self) -> usize { self.encode().len() }

    fn digest(&self) -> RecordDigest {
        let encoded = self.encode();
        let mut input = Vec::with_capacity(RECORD_DIGEST_DOMAIN.len() + encoded.len());
        input.extend_from_slice(RECORD_DIGEST_DOMAIN);
        input.extend_from_slice(&encoded);
        Blake2b256::hash(input)
            .try_into()
            .expect("Blake2b256 emits 32 bytes")
    }
}
