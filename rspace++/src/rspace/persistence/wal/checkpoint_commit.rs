//! Typed terminal COMMIT for one self-contained generation checkpoint.
//!
//! Kind 4 deliberately does not reinterpret the kind-2 incremental receipt.
//! It embeds the exact canonical checkpoint descriptor and only the three
//! physical publication coordinates not already carried by that descriptor.

use super::checkpoint::{
    GenerationCheckpointPrepareIdentityV1, GenerationCheckpointPrepareRecordViewV1,
};
use super::codec::{
    ArraySink, COMMIT_DOMAIN, GENERATION_CHECKPOINT_COMMIT_KIND_V1, LOG_PACK_VERSION_V1,
    read_physical_digest, read_store_id, require_domain, require_version_and_kind,
};
use super::error::LogPackFormatError;
use super::types::{CommitPublicationV1, LogPackAddress, LogPackSequence};
use crate::rspace::persistence::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, ReadCursor, checked_len, checked_sum,
    write_u64,
};
use crate::rspace::persistence::checkpoint::{
    GENERATION_CHECKPOINT_CANONICAL_LEN, GenerationCheckpointDescriptorViewV1,
};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::durable::{
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1,
};
use crate::rspace::persistence::physical::{PhysicalDigest, StoreId};
use crate::rspace::persistence::retention::v2_format::RetentionReader;

const CHECKPOINT_COMMIT_AUTH_LEN: usize = 392;
pub(crate) const GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1: u64 =
    (CHECKPOINT_COMMIT_AUTH_LEN + CHECKSUM_FIELD_LEN) as u64;
const CHECKPOINT_COMMIT_RECORD_LEN: usize = GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 as usize;

#[derive(Clone)]
pub(crate) struct GenerationCheckpointCommitRecordV1 {
    bytes: [u8; CHECKPOINT_COMMIT_RECORD_LEN],
    digest: PhysicalDigest,
}

impl std::fmt::Debug for GenerationCheckpointCommitRecordV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GenerationCheckpointCommitRecordV1")
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

impl GenerationCheckpointCommitRecordV1 {
    pub(crate) fn new(
        store_id: StoreId,
        previous_commit: PhysicalDigest,
        prepare_address: LogPackAddress,
        prepare: GenerationCheckpointPrepareIdentityV1,
        publication: CommitPublicationV1,
    ) -> Result<Self, LogPackFormatError> {
        if prepare.store_id() != store_id {
            return Err(LogPackFormatError::Store {
                expected: store_id,
                actual: prepare.store_id(),
            });
        }
        if prepare_address.length() != prepare.record_len() {
            return Err(LogPackFormatError::AddressLength {
                address: prepare_address,
                declared: prepare.record_len(),
            });
        }
        let descriptor = prepare.descriptor();
        validate_checkpoint_publication(descriptor.as_view(), prepare_address, publication)?;

        let mut bytes = [0u8; CHECKPOINT_COMMIT_RECORD_LEN];
        {
            let mut sink = ArraySink::new(&mut bytes[..CHECKPOINT_COMMIT_AUTH_LEN]);
            sink.write_all(COMMIT_DOMAIN).expect("fixed COMMIT domain");
            write_u64(&mut sink, LOG_PACK_VERSION_V1).expect("fixed COMMIT version");
            write_u64(&mut sink, GENERATION_CHECKPOINT_COMMIT_KIND_V1)
                .expect("fixed checkpoint COMMIT kind");
            write_u64(&mut sink, GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1)
                .expect("fixed checkpoint COMMIT length");
            write_u64(&mut sink, prepare.sequence().get()).expect("fixed checkpoint sequence");
            sink.write_all(store_id.as_bytes()).expect("fixed store ID");
            sink.write_all(previous_commit.as_bytes())
                .expect("fixed previous COMMIT digest");
            write_u64(&mut sink, prepare_address.segment()).expect("fixed PREPARE pack");
            write_u64(&mut sink, prepare_address.offset()).expect("fixed PREPARE offset");
            write_u64(&mut sink, prepare_address.length()).expect("fixed PREPARE length");
            sink.write_all(prepare.digest().as_bytes())
                .expect("fixed PREPARE digest");
            descriptor
                .write_canonical(&mut sink)
                .expect("validated checkpoint descriptor has a fixed canonical image");
            write_u64(&mut sink, publication.durable_index_tail())
                .expect("fixed durable index tail");
            write_u64(&mut sink, publication.first_live_pack()).expect("fixed first-live pack");
            write_u64(&mut sink, publication.index_generation()).expect("fixed index generation");
            debug_assert_eq!(sink.written(), CHECKPOINT_COMMIT_AUTH_LEN);
        }
        let checksum = crc32c(&bytes[..CHECKPOINT_COMMIT_AUTH_LEN]);
        bytes[CHECKPOINT_COMMIT_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
        let digest = PhysicalDigest::of_bytes(&bytes[..CHECKPOINT_COMMIT_AUTH_LEN]);
        Ok(Self { bytes, digest })
    }

    pub(crate) const fn canonical_bytes(&self) -> &[u8; CHECKPOINT_COMMIT_RECORD_LEN] {
        &self.bytes
    }

    pub(crate) const fn digest(&self) -> PhysicalDigest { self.digest }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this method yet"
    )]
    pub(crate) fn write_to<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        sink.write_all(&self.bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GenerationCheckpointCommitViewV1 {
    sequence: LogPackSequence,
    store_id: StoreId,
    previous_commit: PhysicalDigest,
    prepare_address: LogPackAddress,
    prepare_digest: PhysicalDigest,
    descriptor: GenerationCheckpointDescriptorViewV1,
    publication: CommitPublicationV1,
    digest: PhysicalDigest,
}

impl GenerationCheckpointCommitViewV1 {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, LogPackFormatError> {
        Self::parse_for(bytes, RetentionReader::V1Only)
    }

    pub(crate) fn parse_for(
        bytes: &[u8],
        reader: RetentionReader,
    ) -> Result<Self, LogPackFormatError> {
        let actual = checked_len(bytes.len(), "checkpoint COMMIT record")?;
        if actual != GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 {
            return Err(LogPackFormatError::LengthMismatch {
                declared: GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
                actual,
            });
        }
        let declared_crc = decode_crc32c(
            bytes[CHECKPOINT_COMMIT_AUTH_LEN..]
                .try_into()
                .expect("fixed checkpoint COMMIT CRC extent"),
        )
        .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint COMMIT record",
            encoded,
        })?;
        let actual_crc = crc32c(&bytes[..CHECKPOINT_COMMIT_AUTH_LEN]);
        if declared_crc != actual_crc {
            return Err(LogPackFormatError::RecordChecksum {
                expected: declared_crc,
                actual: actual_crc,
            });
        }

        let mut cursor = ReadCursor::new(&bytes[..CHECKPOINT_COMMIT_AUTH_LEN]);
        require_domain(&mut cursor, COMMIT_DOMAIN)?;
        require_version_and_kind(&mut cursor, GENERATION_CHECKPOINT_COMMIT_KIND_V1)?;
        let declared_len = cursor.read_u64()?;
        if declared_len != GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 {
            return Err(LogPackFormatError::LengthMismatch {
                declared: declared_len,
                actual,
            });
        }
        let sequence = LogPackSequence::new(cursor.read_u64()?)?;
        let store_id = read_store_id(&mut cursor)?;
        let previous_commit = read_physical_digest(&mut cursor)?;
        let prepare_address =
            LogPackAddress::new(cursor.read_u64()?, cursor.read_u64()?, cursor.read_u64()?)?;
        let prepare_digest = read_physical_digest(&mut cursor)?;
        let descriptor = GenerationCheckpointDescriptorViewV1::parse_for(
            cursor.read_exact(GENERATION_CHECKPOINT_CANONICAL_LEN as usize)?,
            reader,
        )?;
        let durable_index_tail = cursor.read_u64()?;
        let first_live_pack = cursor.read_u64()?;
        let index_generation = cursor.read_u64()?;
        cursor.finish()?;
        let publication = CommitPublicationV1::new(
            first_live_pack,
            index_generation,
            durable_index_tail,
            descriptor.retention_manifest(),
            descriptor.current_state_canonical_bytes().get(),
            descriptor.retained_union_canonical_bytes().get(),
        )?;
        validate_checkpoint_publication(descriptor, prepare_address, publication)?;
        Ok(Self {
            sequence,
            store_id,
            previous_commit,
            prepare_address,
            prepare_digest,
            descriptor,
            publication,
            digest: PhysicalDigest::of_bytes(&bytes[..CHECKPOINT_COMMIT_AUTH_LEN]),
        })
    }

    pub(crate) const fn sequence(self) -> LogPackSequence { self.sequence }

    pub(crate) const fn store_id(self) -> StoreId { self.store_id }

    pub(crate) const fn previous_commit(self) -> PhysicalDigest { self.previous_commit }

    pub(crate) const fn prepare_address(self) -> LogPackAddress { self.prepare_address }

    pub(crate) const fn prepare_digest(self) -> PhysicalDigest { self.prepare_digest }

    pub(crate) const fn descriptor(self) -> GenerationCheckpointDescriptorViewV1 { self.descriptor }

    pub(crate) const fn publication(self) -> CommitPublicationV1 { self.publication }

    pub(crate) const fn digest(self) -> PhysicalDigest { self.digest }

    pub(crate) fn verify_prepare(
        self,
        prepare: GenerationCheckpointPrepareIdentityV1,
    ) -> Result<(), LogPackFormatError> {
        if prepare.sequence() != self.sequence {
            return Err(LogPackFormatError::Sequence {
                expected: self.sequence.get(),
                actual: prepare.sequence().get(),
            });
        }
        if prepare.store_id() != self.store_id {
            return Err(LogPackFormatError::Store {
                expected: self.store_id,
                actual: prepare.store_id(),
            });
        }
        if prepare.digest() != self.prepare_digest {
            return Err(LogPackFormatError::RecordDigest {
                expected: self.prepare_digest,
                actual: prepare.digest(),
            });
        }
        if prepare.record_len() != self.prepare_address.length() {
            return Err(LogPackFormatError::AddressLength {
                address: self.prepare_address,
                declared: prepare.record_len(),
            });
        }
        if prepare.descriptor().as_view() != self.descriptor {
            return Err(LogPackFormatError::CheckpointDescriptorMismatch);
        }
        Ok(())
    }

    pub(crate) fn verify_record_view(
        self,
        prepare: GenerationCheckpointPrepareRecordViewV1<'_>,
    ) -> Result<(), LogPackFormatError> {
        self.verify_prepare(GenerationCheckpointPrepareIdentityV1::from_record_view(prepare))
    }
}

fn validate_checkpoint_publication(
    descriptor: GenerationCheckpointDescriptorViewV1,
    prepare_address: LogPackAddress,
    publication: CommitPublicationV1,
) -> Result<(), LogPackFormatError> {
    if prepare_address.offset() != PACK_HEADER_LEN_V1 {
        return Err(LogPackFormatError::CheckpointPrepareOffset {
            expected: PACK_HEADER_LEN_V1,
            actual: prepare_address.offset(),
        });
    }
    if publication.first_live_pack() != prepare_address.segment() {
        return Err(LogPackFormatError::CheckpointFirstLivePack {
            expected: prepare_address.segment(),
            actual: publication.first_live_pack(),
        });
    }
    if publication.retention_manifest() != descriptor.retention_manifest() {
        return Err(LogPackFormatError::CheckpointPublicationRetention {
            expected: descriptor.retention_manifest(),
            actual: publication.retention_manifest(),
        });
    }
    let current = descriptor.current_state_canonical_bytes().get();
    if publication.current_state_canonical_bytes() != current {
        return Err(LogPackFormatError::CheckpointPublicationCurrentBytes {
            expected: current,
            actual: publication.current_state_canonical_bytes(),
        });
    }
    let retained = descriptor.retained_union_canonical_bytes().get();
    if publication.retained_canonical_bytes() != retained {
        return Err(LogPackFormatError::CheckpointPublicationRetainedBytes {
            expected: retained,
            actual: publication.retained_canonical_bytes(),
        });
    }
    let record_bytes = descriptor
        .data_objects()
        .checked_mul(INDEX_RECORD_LEN_V1)
        .ok_or(CanonicalFormatError::LengthOverflow {
            field: "checkpoint index records",
        })?;
    let expected_tail =
        checked_sum(INDEX_HEADER_LEN_V1, record_bytes, "checkpoint exact index tail")?;
    if publication.durable_index_tail() != expected_tail {
        return Err(LogPackFormatError::CheckpointIndexTail {
            expected: expected_tail,
            actual: publication.durable_index_tail(),
        });
    }
    Ok(())
}
