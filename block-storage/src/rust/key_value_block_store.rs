// See block-storage/src/main/scala/coop/rchain/blockstorage/KeyValueBlockStore.scala

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use models::casper::{ApprovedBlockProto, BlockMessageProto, FinalizationCertificateProto};
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, BlockMessage, FinalizationCertificate,
};
use prost::Message;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

#[derive(Clone)]
pub struct KeyValueBlockStore {
    store: Arc<dyn KeyValueStore>,
    store_approved_block: Arc<dyn KeyValueStore>,
    store_finalization_certificates: Option<Arc<dyn KeyValueStore>>,
    verified_finalization_certificates: Arc<Mutex<(HashSet<BlockHash>, VecDeque<BlockHash>)>>,
    approved_block_key: [u8; 1],
}

thread_local! {
    static BLOCK_PROTO_DECOMPRESS_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static DEPLOY_SIG_DECOMPRESS_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

impl KeyValueBlockStore {
    // Keep a small bounded decompression scratch buffer per thread to prevent
    // long-lived memory retention from repeatedly decoding block payloads.
    const DECOMPRESS_BUFFER_RETAIN_BYTES: usize = 65_536;
    const MAX_STORED_BLOCK_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
    const DEPLOY_SIG_CACHE_MAX_ENTRIES: usize = 1_024;
    const MIN_DEPLOY_SIG_BYTES: usize = 32;

    pub fn new(
        store: Arc<dyn KeyValueStore>,
        store_approved_block: Arc<dyn KeyValueStore>,
    ) -> Self {
        Self::new_with_finalization_certificate_store(store, store_approved_block, None)
    }

    pub fn new_with_finalization_certificate_store(
        store: Arc<dyn KeyValueStore>,
        store_approved_block: Arc<dyn KeyValueStore>,
        store_finalization_certificates: Option<Arc<dyn KeyValueStore>>,
    ) -> Self {
        Self {
            store,
            store_approved_block,
            store_finalization_certificates,
            verified_finalization_certificates: Arc::new(Mutex::new((
                HashSet::new(),
                VecDeque::new(),
            ))),
            approved_block_key: [42],
        }
    }

    pub async fn create_from_kvm(kvm: &mut dyn KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let store = kvm.store("blocks".to_string()).await?;
        let store_approved_block = kvm.store("blocks-approved".to_string()).await?;
        let store_finalization_certificates =
            kvm.store("finalization-certificates".to_string()).await?;
        Ok(Self {
            store,
            store_approved_block,
            store_finalization_certificates: Some(store_finalization_certificates),
            verified_finalization_certificates: Arc::new(Mutex::new((
                HashSet::new(),
                VecDeque::new(),
            ))),
            approved_block_key: [42],
        })
    }

    fn error_block(hash: BlockHash, cause: String) -> String {
        format!(
            "Block decoding error, hash {}. Cause: {}",
            PrettyPrinter::build_string_bytes(&hash),
            cause
        )
    }

    pub fn get(&self, block_hash: &BlockHash) -> Result<Option<BlockMessage>, KvStoreError> {
        let Some(mut block) = self.get_detached(block_hash)? else {
            return Ok(None);
        };
        self.reattach_finalization_certificate(&mut block)?;
        Ok(Some(block))
    }

    pub fn get_detached(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockMessage>, KvStoreError> {
        let bytes = self.store.get_one(&block_hash.to_vec())?;
        if bytes.is_none() {
            return Ok(None);
        }
        let bytes = bytes.unwrap();
        let block_proto = Self::bytes_to_block_proto(&bytes)?;
        let block = BlockMessage::from_proto(block_proto);
        match block {
            Ok(block) => Ok(Some(block)),
            Err(err) => Err(KvStoreError::SerializationError(Self::error_block(
                block_hash.clone(),
                err.to_string(),
            ))),
        }
    }

    /**
     * See block-storage/src/main/scala/coop/rchain/blockstorage/BlockStoreSyntax.scala
     *
     * Get block, "unsafe" because method expects block already in the block store.
     */
    pub fn get_unsafe(&self, block_hash: &BlockHash) -> BlockMessage {
        let err_msg = format!(
            "BlockStore is missing hash: {}",
            PrettyPrinter::build_string_bytes(block_hash),
        );
        self.get(block_hash).expect(&err_msg).expect(&err_msg)
    }

    /// Fast path used by repeat-deploy checks to avoid full BlockMessage conversion.
    pub fn has_any_deploy_sig(
        &self,
        block_hash: &BlockHash,
        deploy_sigs: &HashSet<Vec<u8>>,
    ) -> Result<bool, KvStoreError> {
        if deploy_sigs.is_empty() {
            return Ok(false);
        }
        let key = block_hash.to_vec();
        if let Some(has_any) = Self::cached_has_any_deploy_sig(&key, deploy_sigs) {
            return Ok(has_any);
        }

        let bytes = match self.store.get_one(&key)? {
            Some(bytes) => bytes,
            None => return Ok(false),
        };

        let body = Self::decode_block_deploy_sigs(&bytes)?;
        let mut block_deploy_sigs = Vec::with_capacity(body.deploys.len());
        let mut has_any = false;
        for processed_deploy in body.deploys {
            let deploy = processed_deploy.deploy.ok_or_else(|| {
                KvStoreError::SerializationError(Self::error_block(
                    block_hash.clone(),
                    "Missing deploy field".to_string(),
                ))
            })?;
            let sig = deploy.sig;
            if sig.len() < Self::MIN_DEPLOY_SIG_BYTES {
                return Err(KvStoreError::SerializationError(Self::error_block(
                    block_hash.clone(),
                    format!("Invalid deploy signature length: {}", sig.len()),
                )));
            }
            if deploy_sigs.contains(&sig) {
                has_any = true;
            }
            block_deploy_sigs.push(sig);
        }
        Self::cache_deploy_sigs(key, block_deploy_sigs);
        Ok(has_any)
    }

    /// Fetch rejected deploy signatures for a block without decoding a full BlockMessage.
    /// Returns the `body.rejected_deploys[*].sig` values. Most blocks have none; only
    /// multi-parent merge blocks that dropped a conflicting deploy populate this list.
    pub fn rejected_deploy_sigs(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Vec<u8>>>, KvStoreError> {
        let key = block_hash.to_vec();
        let bytes = match self.store.get_one(&key)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        let body = Self::decode_block_deploy_sigs(&bytes)?;
        let sigs = body.rejected_deploys.into_iter().map(|r| r.sig).collect();
        Ok(Some(sigs))
    }

    /// Fetch deploy signatures for a block without decoding a full BlockMessage.
    /// Uses the same bounded shared cache as `has_any_deploy_sig`.
    pub fn deploy_sigs(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Vec<u8>>>, KvStoreError> {
        let key = block_hash.to_vec();
        if let Some(cached) = Self::cached_deploy_sigs(&key) {
            return Ok(Some(cached));
        }

        let bytes = match self.store.get_one(&key)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };

        let body = Self::decode_block_deploy_sigs(&bytes)?;
        let mut block_deploy_sigs = Vec::with_capacity(body.deploys.len());
        for processed_deploy in body.deploys {
            let deploy = processed_deploy.deploy.ok_or_else(|| {
                KvStoreError::SerializationError(Self::error_block(
                    block_hash.clone(),
                    "Missing deploy field".to_string(),
                ))
            })?;
            if deploy.sig.len() < Self::MIN_DEPLOY_SIG_BYTES {
                return Err(KvStoreError::SerializationError(Self::error_block(
                    block_hash.clone(),
                    format!("Invalid deploy signature length: {}", deploy.sig.len()),
                )));
            }
            block_deploy_sigs.push(deploy.sig);
        }

        Self::cache_deploy_sigs(key, block_deploy_sigs.clone());
        Ok(Some(block_deploy_sigs))
    }

    pub fn has_any_deploy_sig_unsafe(
        &self,
        block_hash: &BlockHash,
        deploy_sigs: &HashSet<Vec<u8>>,
    ) -> bool {
        let err_msg = format!(
            "BlockStore is missing hash: {}",
            PrettyPrinter::build_string_bytes(block_hash),
        );
        self.has_any_deploy_sig(block_hash, deploy_sigs)
            .expect(&err_msg)
    }

    pub fn put(&self, block_hash: BlockHash, block: &BlockMessage) -> Result<(), KvStoreError> {
        let mut stored_block = block.clone();
        self.persist_finalization_certificate(&mut stored_block)?;
        let block_proto = stored_block.to_proto();
        let bytes = Self::block_proto_to_bytes(&block_proto);
        self.store.put_one(block_hash.to_vec(), bytes)
    }

    pub fn put_block_message_awaiting_certificate(
        &self,
        block: &BlockMessage,
    ) -> Result<(), KvStoreError> {
        let commitment = block.header.finalized_floor.as_ref().ok_or_else(|| {
            KvStoreError::SerializationError(
                "detached block must carry a finalized-floor commitment".to_string(),
            )
        })?;
        if block.finalized_floor_certificate.is_some() {
            return Err(KvStoreError::SerializationError(
                "detached block must not carry a finalization certificate".to_string(),
            ));
        }
        commitment
            .validate_shape()
            .map_err(KvStoreError::SerializationError)?;
        let bytes = Self::block_proto_to_bytes(&block.to_proto());
        self.store.put_one(block.block_hash.to_vec(), bytes)
    }

    fn persist_finalization_certificate(
        &self,
        block: &mut BlockMessage,
    ) -> Result<(), KvStoreError> {
        match (
            block.header.finalized_floor.as_ref(),
            block.finalized_floor_certificate.as_ref(),
        ) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(KvStoreError::SerializationError(
                "block carries a finalization certificate without a signed floor commitment"
                    .to_string(),
            )),
            (Some(_), None) => Err(KvStoreError::SerializationError(
                "block carries a signed floor commitment without its finalization certificate"
                    .to_string(),
            )),
            (Some(commitment), Some(certificate)) => {
                certificate
                    .validate_commitment(commitment)
                    .map_err(KvStoreError::SerializationError)?;
                if self.store_finalization_certificates.is_some() {
                    let digest = certificate.digest();
                    self.put_finalization_certificate(&digest, certificate)?;
                    block.finalized_floor_certificate = None;
                }
                Ok(())
            }
        }
    }

    fn reattach_finalization_certificate(
        &self,
        block: &mut BlockMessage,
    ) -> Result<(), KvStoreError> {
        let Some(commitment) = block.header.finalized_floor.as_ref() else {
            if block.finalized_floor_certificate.is_some() {
                return Err(KvStoreError::SerializationError(
                    "stored block carries a finalization certificate without a signed floor commitment"
                        .to_string(),
                ));
            }
            return Ok(());
        };

        if block.finalized_floor_certificate.is_none() {
            block.finalized_floor_certificate = Some(
                self.get_finalization_certificate(&commitment.certificate_digest)?
                    .ok_or_else(|| {
                        KvStoreError::SerializationError(
                            "committed finalization certificate is unavailable".to_string(),
                        )
                    })?,
            );
        }

        block
            .finalized_floor_certificate
            .as_ref()
            .expect("certificate was attached")
            .validate_commitment(commitment)
            .map_err(KvStoreError::SerializationError)
    }

    pub fn get_finalization_certificate(
        &self,
        digest: &BlockHash,
    ) -> Result<Option<FinalizationCertificate>, KvStoreError> {
        if digest.len() != models::rust::block_hash::LENGTH {
            return Err(KvStoreError::SerializationError(format!(
                "finalization certificate digest must be {} bytes",
                models::rust::block_hash::LENGTH
            )));
        }
        let Some(store) = &self.store_finalization_certificates else {
            return Ok(None);
        };
        let Some(bytes) = store.get_one(&digest.to_vec())? else {
            return Ok(None);
        };
        if bytes.len() > FinalizationCertificate::MAX_ENCODED_BYTES {
            return Err(KvStoreError::SerializationError(format!(
                "stored finalization certificate exceeds {} encoded bytes",
                FinalizationCertificate::MAX_ENCODED_BYTES
            )));
        }
        let proto = FinalizationCertificateProto::decode(bytes.as_slice()).map_err(|error| {
            KvStoreError::SerializationError(format!(
                "finalization certificate decoding error: {error}"
            ))
        })?;
        let certificate =
            FinalizationCertificate::from_proto(proto).map_err(KvStoreError::SerializationError)?;
        if certificate.digest() != *digest {
            return Err(KvStoreError::SerializationError(
                "content-addressed finalization certificate digest mismatch".to_string(),
            ));
        }
        Ok(Some(certificate))
    }

    pub fn put_finalization_certificate(
        &self,
        digest: &BlockHash,
        certificate: &FinalizationCertificate,
    ) -> Result<(), KvStoreError> {
        if digest.len() != models::rust::block_hash::LENGTH {
            return Err(KvStoreError::SerializationError(format!(
                "finalization certificate digest must be {} bytes",
                models::rust::block_hash::LENGTH
            )));
        }
        certificate
            .validate_shape()
            .map_err(KvStoreError::SerializationError)?;
        if certificate.digest() != *digest {
            return Err(KvStoreError::SerializationError(
                "content-addressed finalization certificate digest mismatch".to_string(),
            ));
        }
        let bytes = certificate.to_proto().encode_to_vec();
        if bytes.len() > FinalizationCertificate::MAX_ENCODED_BYTES {
            return Err(KvStoreError::SerializationError(format!(
                "finalization certificate exceeds {} encoded bytes",
                FinalizationCertificate::MAX_ENCODED_BYTES
            )));
        }
        let Some(store) = &self.store_finalization_certificates else {
            return Err(KvStoreError::InvalidArgument(
                "finalization certificate sidecar storage is unavailable".to_string(),
            ));
        };
        if let Some(existing) = self.get_finalization_certificate(digest)? {
            if existing != *certificate {
                return Err(KvStoreError::SerializationError(
                    "content-addressed finalization certificate collision".to_string(),
                ));
            }
            return Ok(());
        }
        store.put_if_absent(vec![(digest.to_vec(), bytes)])
    }

    pub fn put_block_message(&self, block: &BlockMessage) -> Result<(), KvStoreError> {
        self.put(block.block_hash.clone(), block)
    }

    pub fn contains_stored_block(&self, block_hash: &BlockHash) -> Result<bool, KvStoreError> {
        Ok(self.store.get_one(&block_hash.to_vec())?.is_some())
    }

    pub fn is_finalization_certificate_verified(&self, digest: &BlockHash) -> bool {
        self.verified_finalization_certificates
            .lock()
            .map(|cache| cache.0.contains(digest))
            .unwrap_or(false)
    }

    pub fn mark_finalization_certificate_verified(
        &self,
        digest: BlockHash,
    ) -> Result<(), KvStoreError> {
        const MAX_VERIFIED_CERTIFICATES: usize = 4_096;
        let mut cache = self
            .verified_finalization_certificates
            .lock()
            .map_err(|error| KvStoreError::LockError(error.to_string()))?;
        if cache.0.insert(digest.clone()) {
            cache.1.push_back(digest);
        }
        while cache.1.len() > MAX_VERIFIED_CERTIFICATES {
            if let Some(evicted) = cache.1.pop_front() {
                cache.0.remove(&evicted);
            }
        }
        Ok(())
    }

    pub fn contains(&self, block_hash: &BlockHash) -> Result<bool, KvStoreError> {
        match self.get(block_hash) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn error_approved_block(cause: String) -> String {
        format!("Approved block decoding error. Cause: {}", cause)
    }

    pub fn get_approved_block(&self) -> Result<Option<ApprovedBlock>, KvStoreError> {
        let bytes = self
            .store_approved_block
            .get_one(&self.approved_block_key.to_vec())?;

        if bytes.is_none() {
            return Ok(None);
        }

        let bytes = bytes.unwrap();
        let block_proto = ApprovedBlockProto::decode(&*bytes).map_err(|err| {
            KvStoreError::SerializationError(Self::error_approved_block(err.to_string()))
        })?;
        let block = ApprovedBlock::from_proto(block_proto).map_err(|err| {
            KvStoreError::SerializationError(Self::error_approved_block(err.to_string()))
        })?;
        Ok(Some(block))
    }

    pub fn put_approved_block(&self, block: &ApprovedBlock) -> Result<(), KvStoreError> {
        let block_proto = block.clone().to_proto();
        let bytes = block_proto.encode_to_vec();
        self.store_approved_block
            .put_one(self.approved_block_key.to_vec(), bytes)
    }

    fn bytes_to_block_proto(bytes: &[u8]) -> Result<BlockMessageProto, KvStoreError> {
        use std::io::Cursor;

        use prost::encoding::decode_varint;

        let mut cursor = Cursor::new(bytes);
        let decompressed_length = decode_varint(&mut cursor).map_err(|err| {
            KvStoreError::SerializationError(format!(
                "Failed to decode varint length prefix: {err}"
            ))
        })?;
        let decompressed_length = usize::try_from(decompressed_length).map_err(|_| {
            KvStoreError::SerializationError(
                "Stored block decompressed length does not fit this platform".to_string(),
            )
        })?;
        if decompressed_length > Self::MAX_STORED_BLOCK_DECOMPRESSED_BYTES {
            return Err(KvStoreError::SerializationError(format!(
                "Stored block declares {decompressed_length} decompressed bytes, exceeding the protocol limit {}",
                Self::MAX_STORED_BLOCK_DECOMPRESSED_BYTES
            )));
        }

        let compressed_data = &bytes[cursor.position() as usize..];
        let max_retain_bytes = Self::decode_buffer_retain_bytes();
        BLOCK_PROTO_DECOMPRESS_BUFFER.with(|buffer| {
            let mut output_buf = buffer.borrow_mut();
            if output_buf.len() < decompressed_length {
                output_buf.resize(decompressed_length, 0u8);
            }
            let output = &mut output_buf[..decompressed_length];

            lz4_flex::decompress_into(compressed_data, output).map_err(|err| {
                KvStoreError::SerializationError(format!("Decompress of block failed: {err}"))
            })?;

            let decode_result = BlockMessageProto::decode(&*output)
                .map_err(|err| KvStoreError::SerializationError(err.to_string()));

            // Avoid retaining very large per-thread scratch buffers indefinitely.
            if output_buf.capacity() > max_retain_bytes {
                output_buf.clear();
                output_buf.shrink_to(max_retain_bytes);
            }

            decode_result
        })
    }

    fn decode_block_deploy_sigs(bytes: &[u8]) -> Result<BlockDeploySigsBody, KvStoreError> {
        use std::io::Cursor;

        use prost::encoding::decode_varint;

        let mut cursor = Cursor::new(bytes);
        let decompressed_length = decode_varint(&mut cursor).map_err(|err| {
            KvStoreError::SerializationError(format!(
                "Failed to decode varint length prefix: {err}"
            ))
        })?;
        let decompressed_length = usize::try_from(decompressed_length).map_err(|_| {
            KvStoreError::SerializationError(
                "Stored deploy-signature index decompressed length does not fit this platform"
                    .to_string(),
            )
        })?;
        if decompressed_length > Self::MAX_STORED_BLOCK_DECOMPRESSED_BYTES {
            return Err(KvStoreError::SerializationError(format!(
                "Stored deploy-signature index declares {decompressed_length} decompressed bytes, exceeding the protocol limit {}",
                Self::MAX_STORED_BLOCK_DECOMPRESSED_BYTES
            )));
        }

        let compressed_data = &bytes[cursor.position() as usize..];
        let max_retain_bytes = Self::decode_buffer_retain_bytes();
        DEPLOY_SIG_DECOMPRESS_BUFFER.with(|buffer| {
            let mut output_buf = buffer.borrow_mut();
            if output_buf.len() < decompressed_length {
                output_buf.resize(decompressed_length, 0u8);
            }
            let output = &mut output_buf[..decompressed_length];

            lz4_flex::decompress_into(compressed_data, output).map_err(|err| {
                KvStoreError::SerializationError(format!("Decompress of block failed: {err}"))
            })?;

            let decode_result = BlockMessageDeploySigIndex::decode(&*output)
                .map_err(|err| KvStoreError::SerializationError(err.to_string()))
                .and_then(|proto| {
                    proto.body.ok_or_else(|| {
                        KvStoreError::SerializationError("Missing body field".to_string())
                    })
                });

            if output_buf.capacity() > max_retain_bytes {
                output_buf.clear();
                output_buf.shrink_to(max_retain_bytes);
            }

            decode_result
        })
    }

    fn block_proto_to_bytes(block_proto: &BlockMessageProto) -> Vec<u8> {
        Self::compress_bytes(&block_proto.encode_to_vec())
    }

    fn cached_has_any_deploy_sig(
        block_hash: &[u8],
        deploy_sigs: &HashSet<Vec<u8>>,
    ) -> Option<bool> {
        let cache = Self::deploy_sig_cache().lock().ok()?;
        cache
            .entries
            .get(block_hash)
            .map(|cached_sigs| cached_sigs.iter().any(|sig| deploy_sigs.contains(sig)))
    }

    fn cached_deploy_sigs(block_hash: &[u8]) -> Option<Vec<Vec<u8>>> {
        let cache = Self::deploy_sig_cache().lock().ok()?;
        cache.entries.get(block_hash).cloned()
    }

    fn cache_deploy_sigs(block_hash: Vec<u8>, deploy_sigs: Vec<Vec<u8>>) {
        let max_entries = Self::max_deploy_sig_cache_entries();
        if max_entries == 0 {
            return;
        }
        if let Ok(mut cache) = Self::deploy_sig_cache().lock() {
            if !cache.entries.contains_key(&block_hash) {
                cache.order.push_back(block_hash.clone());
                while cache.order.len() > max_entries {
                    if let Some(oldest) = cache.order.pop_front() {
                        cache.entries.remove(&oldest);
                    }
                }
            }
            cache.entries.insert(block_hash, deploy_sigs);
        }
    }

    fn deploy_sig_cache() -> &'static Mutex<DeploySigCache> {
        static CACHE: OnceLock<Mutex<DeploySigCache>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(DeploySigCache::default()))
    }

    fn decode_buffer_retain_bytes() -> usize { Self::DECOMPRESS_BUFFER_RETAIN_BYTES }

    fn max_deploy_sig_cache_entries() -> usize { Self::DEPLOY_SIG_CACHE_MAX_ENTRIES }

    #[cfg(test)]
    fn block_proto_decode_buffer_capacity_for_test() -> usize {
        BLOCK_PROTO_DECOMPRESS_BUFFER.with(|buffer| buffer.borrow().capacity())
    }

    /// Compress bytes with varint length prefix (compatible with Java LZ4CompressorWithLength)
    fn compress_bytes(bytes: &[u8]) -> Vec<u8> {
        use prost::encoding::encode_varint;

        let compressed = lz4_flex::compress(bytes);
        let mut result = Vec::new();

        // Encode original (decompressed) length as varint to match Java format
        encode_varint(bytes.len() as u64, &mut result);
        result.extend_from_slice(&compressed);
        result
    }
}

#[derive(Default)]
struct DeploySigCache {
    entries: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    order: VecDeque<Vec<u8>>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct BlockMessageDeploySigIndex {
    #[prost(message, optional, tag = "3")]
    body: Option<BlockDeploySigsBody>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct BlockDeploySigsBody {
    #[prost(message, repeated, tag = "2")]
    deploys: Vec<BlockDeploySigsProcessedDeploy>,
    #[prost(message, repeated, tag = "5")]
    rejected_deploys: Vec<BlockDeploySigsRejectedDeploy>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct BlockDeploySigsProcessedDeploy {
    #[prost(message, optional, tag = "1")]
    deploy: Option<BlockDeploySigsDeploy>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct BlockDeploySigsDeploy {
    #[prost(bytes = "vec", tag = "4")]
    sig: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct BlockDeploySigsRejectedDeploy {
    #[prost(bytes = "vec", tag = "1")]
    sig: Vec<u8>,
}

// See block-storage/src/test/scala/coop/rchain/blockstorage/KeyValueBlockStoreSpec.scala

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use models::rust::block_hash::BlockHashSerde;
    use models::rust::block_implicits::{block_element_gen, processed_deploy_gen};
    use models::rust::casper::protocol::casper_message::{
        ApprovedBlockCandidate, FinalizationCertificate,
    };
    use models::rust::validator::ValidatorSerde;
    use proptest::prelude::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use shared::rust::{ByteBuffer, ByteString};

    use super::*;
    use crate::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;

    struct MockKeyValueStore {
        get_result: Option<ByteString>,
        input_keys: Arc<Mutex<Vec<ByteString>>>,
        input_puts: Arc<Mutex<Vec<ByteString>>>,
    }

    impl MockKeyValueStore {
        fn new(get_result: Option<Vec<u8>>) -> Self {
            Self {
                get_result,
                input_keys: Arc::new(Mutex::new(Vec::new())),
                input_puts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn update_input_keys(&self, keys: Vec<ByteString>) {
            self.input_keys.lock().unwrap().extend(keys);
        }
    }

    impl KeyValueStore for MockKeyValueStore {
        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.update_input_keys(keys.to_vec());
            Ok(vec![self.get_result.clone()])
        }

        fn put(
            &self,
            kv_pairs: Vec<(shared::rust::ByteBuffer, shared::rust::ByteBuffer)>,
        ) -> Result<(), KvStoreError> {
            self.input_keys
                .lock()
                .unwrap()
                .extend(kv_pairs.iter().map(|(k, _)| k.clone()));
            self.input_puts
                .lock()
                .unwrap()
                .extend(kv_pairs.iter().map(|(_, v)| v.clone()));
            Ok(())
        }

        fn delete(&self, _keys: Vec<shared::rust::ByteBuffer>) -> Result<usize, KvStoreError> {
            todo!()
        }

        fn iterate(
            &self,
            _f: fn(shared::rust::ByteBuffer, shared::rust::ByteBuffer),
        ) -> Result<(), KvStoreError> {
            todo!()
        }

        fn iterate_while(
            &self,
            _f: &mut dyn FnMut(
                shared::rust::ByteBuffer,
                shared::rust::ByteBuffer,
            ) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            todo!()
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { todo!() }

        fn to_map(
            &self,
        ) -> Result<
            std::collections::BTreeMap<shared::rust::ByteBuffer, shared::rust::ByteBuffer>,
            KvStoreError,
        > {
            todo!()
        }

        fn print_store(&self) -> Result<(), KvStoreError> { Ok(()) }

        fn size_bytes(&self) -> usize { todo!() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { todo!() }
    }

    pub struct NotImplementedKV;

    impl KeyValueStore for NotImplementedKV {
        fn get(&self, _keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            todo!()
        }

        fn put(&self, _kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            todo!()
        }

        fn delete(&self, _keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> { todo!() }

        fn iterate(&self, _f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> { todo!() }

        fn iterate_while(
            &self,
            _f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            todo!()
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { todo!() }

        fn to_map(
            &self,
        ) -> Result<std::collections::BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            todo!()
        }

        fn print_store(&self) -> Result<(), KvStoreError> { todo!() }

        fn size_bytes(&self) -> usize { todo!() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { todo!() }
    }

    fn to_approved_block(block: BlockMessage) -> ApprovedBlock {
        let candidate = ApprovedBlockCandidate {
            block,
            required_sigs: 0,
        };
        ApprovedBlock {
            candidate,
            sigs: vec![],
        }
    }

    fn vm_rss_kb() -> Option<usize> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find(|line| line.starts_with("VmRSS:"))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<usize>().ok())
    }

    fn kb_to_mib(kb: usize) -> f64 { kb as f64 / 1024.0 }

    fn finalization_certificate() -> FinalizationCertificate {
        let target = BlockHashSerde(Bytes::from(vec![3; 32]));
        let latest = BlockHashSerde(Bytes::from(vec![4; 32]));
        let carrier = BlockHashSerde(Bytes::from(vec![5; 32]));
        FinalizationCertificate {
            schema_version: FinalizationCertificate::SCHEMA_VERSION,
            protocol_version: 6,
            shard_id: "root".to_string(),
            genesis_hash: BlockHashSerde(Bytes::from(vec![1; 32])),
            predecessor_floor_hash: BlockHashSerde(Bytes::from(vec![2; 32])),
            predecessor_certificate_digest: BlockHashSerde(Bytes::from(vec![6; 32])),
            predecessor_certificate_block_hash: carrier.clone(),
            target_floor_hash: target.clone(),
            target_post_state_hash: BlockHashSerde(Bytes::from(vec![7; 32])),
            target_block_number: 3,
            fault_tolerance_numerator: 100_000,
            fault_tolerance_denominator: 1_000_000,
            exact_latest_messages: std::collections::BTreeMap::from([(
                ValidatorSerde(Bytes::from(vec![8; 65])),
                latest.clone(),
            )]),
            authority_context_digest: BlockHashSerde(Bytes::from(vec![9; 32])),
            supporting_manifest_digest: FinalizationCertificate::supporting_digest(
                &std::collections::BTreeSet::from([target.clone(), latest, carrier]),
            ),
            finalized_manifest_digest: FinalizationCertificate::finalized_digest(
                &std::collections::BTreeSet::from([target]),
            ),
            supporting_block_count: 3,
            finalized_block_count: 1,
        }
    }

    #[tokio::test]
    async fn finalization_certificates_are_deduplicated_and_reattached() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut runner = TestRunner::default();
        let mut first = block_element_gen(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .new_tree(&mut runner)
        .unwrap()
        .current();
        let certificate = finalization_certificate();
        first.header.finalized_floor =
            Some(certificate.commitment(certificate.authority_context_digest.0.clone()));
        first.finalized_floor_certificate = Some(certificate.clone());
        store.put_block_message(&first).unwrap();

        let mut second = first.clone();
        second.block_hash = Bytes::from(vec![11; 32]);
        store.put_block_message(&second).unwrap();

        assert_eq!(store.get(&first.block_hash).unwrap(), Some(first));
        assert_eq!(store.get(&second.block_hash).unwrap(), Some(second));
        assert_eq!(
            store
                .get_finalization_certificate(&certificate.digest())
                .unwrap(),
            Some(certificate)
        );
        assert_eq!(
            manager
                .store("finalization-certificates".to_string())
                .await
                .unwrap()
                .to_map()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn committed_certificate_storage_fails_closed_on_missing_or_tampered_proof() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut runner = TestRunner::default();
        let mut block = block_element_gen(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .new_tree(&mut runner)
        .unwrap()
        .current();
        let certificate = finalization_certificate();
        block.header.finalized_floor =
            Some(certificate.commitment(certificate.authority_context_digest.0.clone()));
        assert!(store.put_block_message(&block).is_err());

        let mut tampered = certificate;
        tampered.target_post_state_hash = BlockHashSerde(Bytes::from(vec![12; 32]));
        block.finalized_floor_certificate = Some(tampered);
        assert!(store.put_block_message(&block).is_err());
    }

    #[tokio::test]
    async fn content_addressed_certificate_load_rejects_digest_mismatch() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let certificate = finalization_certificate();
        let digest = certificate.digest();
        let mut tampered = certificate;
        tampered.target_block_number += 1;
        manager
            .store("finalization-certificates".to_string())
            .await
            .unwrap()
            .put_one(digest.to_vec(), tampered.to_proto().encode_to_vec())
            .unwrap();
        assert!(store.get_finalization_certificate(&digest).is_err());
    }

    #[tokio::test]
    async fn content_addressed_certificate_load_rejects_oversized_value_before_decode() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let digest = Bytes::from(vec![13; models::rust::block_hash::LENGTH]);
        manager
            .store("finalization-certificates".to_string())
            .await
            .unwrap()
            .put_one(digest.to_vec(), vec![
                0;
                FinalizationCertificate::MAX_ENCODED_BYTES
                    + 1
            ])
            .unwrap();
        assert!(store.get_finalization_certificate(&digest).is_err());
    }

    #[tokio::test]
    async fn detached_block_remains_unavailable_until_its_certificate_is_stored() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut runner = TestRunner::default();
        let mut block = block_element_gen(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .new_tree(&mut runner)
        .unwrap()
        .current();
        let certificate = finalization_certificate();
        let digest = certificate.digest();
        block.header.finalized_floor =
            Some(certificate.commitment(certificate.authority_context_digest.0.clone()));

        store
            .put_block_message_awaiting_certificate(&block)
            .unwrap();
        assert!(store.contains_stored_block(&block.block_hash).unwrap());
        assert_eq!(
            store.get_detached(&block.block_hash).unwrap(),
            Some(block.clone())
        );
        assert!(store.get(&block.block_hash).is_err());

        store
            .put_finalization_certificate(&digest, &certificate)
            .unwrap();
        let mut expected = block;
        expected.finalized_floor_certificate = Some(certificate);
        assert_eq!(store.get(&expected.block_hash).unwrap(), Some(expected));
        assert!(!store.is_finalization_certificate_verified(&digest));
    }

    #[tokio::test]
    async fn detached_block_and_certificate_obligation_survive_store_recreation() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut runner = TestRunner::default();
        let mut block = block_element_gen(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .new_tree(&mut runner)
        .unwrap()
        .current();
        let certificate = finalization_certificate();
        let digest = certificate.digest();
        block.header.finalized_floor =
            Some(certificate.commitment(certificate.authority_context_digest.0.clone()));
        let block_hash = BlockHashSerde(block.block_hash.clone());

        store
            .put_block_message_awaiting_certificate(&block)
            .unwrap();
        buffer
            .add_certificate_relation(BlockHashSerde(digest.clone()), block_hash.clone())
            .unwrap();
        drop(store);
        drop(buffer);

        let restored_store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let restored_buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap();
        assert_eq!(
            restored_buffer.get_missing_certificate_dependencies(),
            HashSet::from([BlockHashSerde(digest.clone())])
        );
        assert!(restored_buffer.is_waiting_on_certificate(&block_hash));
        assert_eq!(
            restored_store.get_detached(&block.block_hash).unwrap(),
            Some(block.clone())
        );
        assert!(restored_store.get(&block.block_hash).is_err());

        restored_store
            .put_finalization_certificate(&digest, &certificate)
            .unwrap();
        restored_buffer
            .resolve_certificate_dependency(BlockHashSerde(digest))
            .unwrap();
        assert_eq!(restored_buffer.get_pendants(), HashSet::from([block_hash]));
        let restored = restored_store
            .get(&block.block_hash)
            .unwrap()
            .expect("restored block");
        assert_eq!(restored.finalized_floor_certificate, Some(certificate));
    }

    #[tokio::test]
    async fn certificate_sidecar_rejects_a_digest_mismatch_without_persisting() {
        let mut manager = InMemoryStoreManager::new();
        let store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let certificate = finalization_certificate();
        let wrong_digest = Bytes::from(vec![14; models::rust::block_hash::LENGTH]);

        assert!(store
            .put_finalization_certificate(&wrong_digest, &certificate)
            .is_err());
        assert_eq!(
            manager
                .store("finalization-certificates".to_string())
                .await
                .unwrap()
                .to_map()
                .unwrap()
                .len(),
            0
        );
    }

    fn delta_kb_to_mib(delta_kb: isize) -> f64 { delta_kb as f64 / 1024.0 }

    fn bytes_to_mib(bytes: usize) -> f64 { bytes as f64 / (1024.0 * 1024.0) }

    proptest! {
        #![proptest_config(ProptestConfig {
          cases: 5,
          failure_persistence: None,
          .. ProptestConfig::default()
      })]

      /**
        * Block store tests.
        */
      #[test]
      fn block_store_should_get_data_from_underlying_key_value_store(block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None),
        key_string in any::<String>()) {
          let block_bytes = KeyValueBlockStore::block_proto_to_bytes(&block.clone().to_proto());
          let kv = MockKeyValueStore::new(Some(block_bytes));
          let input_keys = Arc::clone(&kv.input_keys);
          let bs = KeyValueBlockStore::new(Arc::new(kv), Arc::new(NotImplementedKV));

          let key = key_string.into_bytes();
          let result = bs.get(&key.clone().into());
          assert!(result.is_ok());
          assert_eq!(*input_keys.lock().unwrap(), vec![key]);
          assert_eq!(result.unwrap(), Some(block));
      }

      #[test]
      fn block_store_should_not_get_data_if_not_exists_in_underlying_key_value_store(key_string in any::<String>()) {
          let kv = MockKeyValueStore::new(None);
          let bs = KeyValueBlockStore::new(Arc::new(kv), Arc::new(NotImplementedKV));
          let key = key_string.into_bytes();
          let result = bs.get(&key.into());
          assert!(result.is_ok());
          assert_eq!(result.unwrap(), None);
      }

      #[test]
      fn block_store_should_put_data_to_underlying_key_value_store(block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None)) {
          let block_bytes = KeyValueBlockStore::block_proto_to_bytes(&block.clone().to_proto());
          let kv = MockKeyValueStore::new(Some(block_bytes.clone()));
          let input_keys = Arc::clone(&kv.input_keys);
          let input_puts = Arc::clone(&kv.input_puts);
          let bs = KeyValueBlockStore::new(Arc::new(kv), Arc::new(NotImplementedKV));

          let result = bs.put_block_message(&block);
          assert!(result.is_ok());
          assert_eq!(*input_keys.lock().unwrap(), vec![block.block_hash.to_vec()]);
          assert_eq!(*input_puts.lock().unwrap(), vec![block_bytes]);
      }

      /**
        * Approved block store
        */
      #[test]
      fn block_store_should_get_approved_block_from_underlying_key_value_store(block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None)) {
          let approved_block = to_approved_block(block);
          let approved_block_bytes = approved_block.clone().to_proto().encode_to_vec();
          let kv = MockKeyValueStore::new(Some(approved_block_bytes));
          let input_keys = Arc::clone(&kv.input_keys);
          let bs = KeyValueBlockStore::new(Arc::new(NotImplementedKV), Arc::new(kv));

          let result = bs.get_approved_block();
          assert!(result.is_ok());
          assert_eq!(*input_keys.lock().unwrap(), vec![bs.approved_block_key]);
          assert_eq!(result.unwrap(), Some(approved_block));
      }

      #[test]
      fn block_store_should_not_get_approved_block_if_not_exists_in_underlying_key_value_store(_s in any::<String>()) {
          let kv = MockKeyValueStore::new(None);
          let bs = KeyValueBlockStore::new(Arc::new(NotImplementedKV), Arc::new(kv));
          let result = bs.get_approved_block();
          assert!(result.is_ok());
          assert_eq!(result.unwrap(), None);
      }

      #[test]
      fn block_store_should_put_approved_block_to_underlying_key_value_store(block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None)) {
          let approved_block = to_approved_block(block);
          let approved_block_bytes = approved_block.clone().to_proto().encode_to_vec();
          let kv = MockKeyValueStore::new(Some(approved_block_bytes.clone()));
          let input_keys = Arc::clone(&kv.input_keys);
          let input_puts = Arc::clone(&kv.input_puts);
          let bs = KeyValueBlockStore::new(Arc::new(NotImplementedKV), Arc::new(kv));

          let result = bs.put_approved_block(&approved_block);
          assert!(result.is_ok());
          assert_eq!(*input_keys.lock().unwrap(), vec![bs.approved_block_key]);
          assert_eq!(*input_puts.lock().unwrap(), vec![approved_block_bytes]);
      }
    }

    #[test]
    fn has_any_deploy_sig_returns_true_or_false_and_caches() {
        let deploy = processed_deploy_gen()
            .new_tree(&mut TestRunner::default())
            .unwrap()
            .current();
        let block = block_element_gen(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![deploy.clone()]),
            None,
            None,
            None,
            None,
        )
        .new_tree(&mut TestRunner::default())
        .unwrap()
        .current();

        let block_bytes = KeyValueBlockStore::block_proto_to_bytes(&block.to_proto());
        let kv = MockKeyValueStore::new(Some(block_bytes));
        let input_keys = Arc::clone(&kv.input_keys);
        let bs = KeyValueBlockStore::new(Arc::new(kv), Arc::new(NotImplementedKV));

        let matching_sig = HashSet::from([deploy.deploy.sig.to_vec()]);
        let not_matching_sig = HashSet::from([vec![0u8]]);

        let has_matching = bs.has_any_deploy_sig(&block.block_hash.clone(), &matching_sig);
        assert!(has_matching.is_ok());
        assert!(has_matching.unwrap());

        let has_not_matching = bs.has_any_deploy_sig(&block.block_hash.clone(), &not_matching_sig);
        assert!(has_not_matching.is_ok());
        assert!(!has_not_matching.unwrap());

        let repeated_lookup = bs
            .has_any_deploy_sig(&block.block_hash.clone(), &not_matching_sig)
            .unwrap();
        assert!(!repeated_lookup);
        assert_eq!(*input_keys.lock().unwrap(), vec![block.block_hash.to_vec()]);
    }

    #[test]
    fn bytes_to_block_proto_should_not_retain_oversized_decode_buffers() {
        let mut block = block_element_gen(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .new_tree(&mut TestRunner::default())
        .unwrap()
        .current();

        let oversized_payload_len = KeyValueBlockStore::decode_buffer_retain_bytes()
            .saturating_mul(8)
            .max(256 * 1024);
        block.extra_bytes = vec![0xAB; oversized_payload_len].into();

        let block_bytes = KeyValueBlockStore::block_proto_to_bytes(&block.to_proto());
        let retain_limit = KeyValueBlockStore::decode_buffer_retain_bytes();
        let mut last_rss = vm_rss_kb();
        let baseline_rss = last_rss;
        let baseline_cap = KeyValueBlockStore::block_proto_decode_buffer_capacity_for_test();

        println!(
            "decode baseline: cap={}B ({:.2} MiB), retain_limit={}B ({:.2} MiB), rss={}KB ({:.2} MiB)",
            baseline_cap,
            bytes_to_mib(baseline_cap),
            retain_limit,
            bytes_to_mib(retain_limit),
            baseline_rss.unwrap_or(0),
            baseline_rss.map(kb_to_mib).unwrap_or(0.0),
        );

        for i in 0..16 {
            let decode_result = KeyValueBlockStore::bytes_to_block_proto(&block_bytes);
            assert!(decode_result.is_ok(), "block decode must succeed");

            if matches!(i + 1, 1 | 2 | 4 | 8 | 16) {
                let cap = KeyValueBlockStore::block_proto_decode_buffer_capacity_for_test();
                let rss = vm_rss_kb();

                let cap_delta_from_limit = cap as isize - retain_limit as isize;
                let cap_delta_from_base = cap as isize - baseline_cap as isize;

                let (rss_value, rss_delta_iter, rss_delta_total) =
                    match (rss, last_rss, baseline_rss) {
                        (Some(curr), Some(prev), Some(base)) => (
                            curr,
                            curr as isize - prev as isize,
                            curr as isize - base as isize,
                        ),
                        (Some(curr), _, _) => (curr, 0, 0),
                        _ => (0, 0, 0),
                    };

                println!(
                    "decode iter #{:>2}: cap={}B ({:.2} MiB) delta_base={:+}B delta_limit={:+}B rss={}KB ({:.2} MiB) rss_delta_iter={:+}KB ({:+.2} MiB) rss_delta_total={:+}KB ({:+.2} MiB)",
                    i + 1,
                    cap,
                    bytes_to_mib(cap),
                    cap_delta_from_base,
                    cap_delta_from_limit,
                    rss_value,
                    kb_to_mib(rss_value),
                    rss_delta_iter,
                    delta_kb_to_mib(rss_delta_iter),
                    rss_delta_total,
                    delta_kb_to_mib(rss_delta_total),
                );

                last_rss = rss;
            }
        }

        let retained_capacity = KeyValueBlockStore::block_proto_decode_buffer_capacity_for_test();
        assert!(
            retained_capacity <= retain_limit,
            "decode buffer retained capacity {} > configured retain limit {}",
            retained_capacity,
            retain_limit
        );
    }

    #[test]
    fn stored_block_length_is_rejected_before_decompression_allocation() {
        let capacity_before = KeyValueBlockStore::block_proto_decode_buffer_capacity_for_test();
        let mut bytes = Vec::new();
        prost::encoding::encode_varint(
            (KeyValueBlockStore::MAX_STORED_BLOCK_DECOMPRESSED_BYTES as u64) + 1,
            &mut bytes,
        );
        let error = KeyValueBlockStore::bytes_to_block_proto(&bytes).unwrap_err();
        assert!(error.to_string().contains("exceeding the protocol limit"));
        assert_eq!(
            KeyValueBlockStore::block_proto_decode_buffer_capacity_for_test(),
            capacity_before
        );
    }

    #[test]
    fn deploy_signature_index_length_is_rejected_before_decompression_allocation() {
        let mut bytes = Vec::new();
        prost::encoding::encode_varint(
            (KeyValueBlockStore::MAX_STORED_BLOCK_DECOMPRESSED_BYTES as u64) + 1,
            &mut bytes,
        );
        let error = KeyValueBlockStore::decode_block_deploy_sigs(&bytes).unwrap_err();
        assert!(error.to_string().contains("exceeding the protocol limit"));
    }
}
