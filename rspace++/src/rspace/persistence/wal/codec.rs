//! Portable, streaming codecs for one sealed transaction and its commit.

use std::convert::Infallible;
use std::marker::PhantomData;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::disposition::{
    LogPackObjectDispositionV1, LogPackObjectPlanV1, OBJECT_DATA_KIND_V1, OBJECT_REUSE_KIND_V1,
};
use super::error::{LogPackFormatError, LogPackWriteError};
use super::types::{CommitPublicationV1, LogPackAddress, LogPackDecodeLimits, LogPackSequence};
use crate::rspace::persistence::canonical::{
    CanonicalFormatError, CanonicalSink, OBJECT_DIGEST_LEN, OBJECT_DOMAIN_LEN, ObjectRefV1,
    ReadCursor, checked_len, checked_sum, write_u64,
};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, Crc32c, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::manifest::{SEMANTIC_PROFILE_ID_LEN, SemanticProfileId};
use crate::rspace::persistence::mutation::{
    MutationBatchViewV1, MutationDecodeLimits, StateVersionV1,
};
use crate::rspace::persistence::object::CanonicalObjectView;
use crate::rspace::persistence::physical::{
    PHYSICAL_DIGEST_LEN, PhysicalDigest, STORE_ID_LEN, StoreId,
};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::transaction::{SealedDagTransaction, SealedTransitionV1};

const PREPARE_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceWAL.Prep\0\0";
const COMMIT_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceWAL.Comm\0\0";
const LOG_PACK_VERSION_V1: u64 = 1;
const PREPARE_KIND: u64 = 1;
const COMMIT_KIND: u64 = 2;
const OBJECT_REF_LEN: u64 = (OBJECT_DIGEST_LEN + 8) as u64;
const OBJECT_RECORD_HEADER_LEN: u64 = 8 + OBJECT_REF_LEN;
const PREPARE_AUTH_HEADER_LEN: usize = 232;
const PREPARE_HEADER_LEN: u64 = PREPARE_AUTH_HEADER_LEN as u64 + CHECKSUM_FIELD_LEN as u64;
const PREPARE_TRAILER_LEN: u64 = CHECKSUM_FIELD_LEN as u64;
const COMMIT_AUTH_LEN: usize = 376;
pub const TRANSACTION_COMMIT_RECORD_LEN_V1: u64 = (COMMIT_AUTH_LEN + CHECKSUM_FIELD_LEN) as u64;
const COMMIT_RECORD_LEN: usize = TRANSACTION_COMMIT_RECORD_LEN_V1 as usize;

/// Identity returned only after every prepare byte has reached the sink.
/// Physical durability is established later by the publisher's sync phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepareRecordIdentityV1 {
    sequence: LogPackSequence,
    store_id: StoreId,
    receipt: SealedTransitionV1,
    record_len: u64,
    digest: PhysicalDigest,
    crc32c: u32,
}

impl PrepareRecordIdentityV1 {
    /// Reconstitute the write identity from a physically and canonically
    /// validated record image. Recovery must still perform durable-store
    /// collision equality, closure, and semantic-profile admission.
    pub const fn from_record_view(view: TransactionPrepareRecordViewV1<'_>) -> Self {
        Self {
            sequence: view.sequence,
            store_id: view.store_id,
            receipt: view.receipt,
            record_len: view.record_len,
            digest: view.digest,
            crc32c: view.crc32c,
        }
    }

    pub const fn sequence(self) -> LogPackSequence { self.sequence }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn receipt(self) -> SealedTransitionV1 { self.receipt }

    pub const fn record_len(self) -> u64 { self.record_len }

    pub const fn digest(self) -> PhysicalDigest { self.digest }

    pub const fn crc32c(self) -> u32 { self.crc32c }
}

/// Preflighted, zero-copy write plan over a profile-typed sealed transaction.
pub struct PreparedLogPackTransactionV1<'transaction, 'mutation, P: SemanticProfile> {
    transaction: &'transaction SealedDagTransaction<'mutation, P>,
    store_id: StoreId,
    sequence: LogPackSequence,
    record_len: u64,
    mutation_len: u64,
    object_count: u64,
    object_bytes: u64,
    object_plan: LogPackObjectPlanV1,
    _profile: PhantomData<fn() -> P>,
}

impl<'transaction, 'mutation, P: SemanticProfile>
    PreparedLogPackTransactionV1<'transaction, 'mutation, P>
{
    pub fn new(
        transaction: &'transaction SealedDagTransaction<'mutation, P>,
        object_plan: LogPackObjectPlanV1,
        store_id: StoreId,
        sequence: LogPackSequence,
        limits: LogPackDecodeLimits,
    ) -> Result<Self, LogPackFormatError> {
        // Bind the type family even if this structure is assembled through a
        // future internal constructor rather than the ordinary sealing path.
        let receipt = transaction.receipt();
        if receipt.profile_id() != P::ID {
            return Err(CanonicalFormatError::SemanticProfileMismatch {
                expected: *P::ID.as_bytes(),
                actual: *receipt.profile_id().as_bytes(),
            }
            .into());
        }
        let mutation_len = checked_len(transaction.mutation_bytes().len(), "WAL mutation")?;
        require_limit("mutation bytes", limits.max_mutation_bytes, mutation_len)?;
        let mutation = MutationBatchViewV1::parse_with_limits(
            transaction.mutation_bytes(),
            MutationDecodeLimits {
                max_entries: limits.max_mutation_entries,
                max_key_bytes: limits.max_mutation_key_bytes,
            },
        )?;
        validate_mutation_receipt(mutation, receipt)?;

        let object_count = checked_len(transaction.objects().len(), "WAL objects")?;
        require_limit("objects", limits.max_objects, object_count)?;
        let plan_count = checked_len(object_plan.len(), "WAL object disposition plan")?;
        if plan_count != object_count {
            return Err(LogPackFormatError::ObjectPlanCount {
                expected: object_count,
                actual: plan_count,
            });
        }
        let mut object_bytes = 0u64;
        let mut inline_object_bytes = 0u64;
        let mut inline_count = 0u64;
        let mut previous = None;
        for (object, disposition) in transaction.objects().iter().zip(object_plan.dispositions()) {
            let reference = object.reference();
            if let Some(previous) = previous {
                if previous >= reference.digest() {
                    return Err(LogPackFormatError::ObjectsNotStrict {
                        previous,
                        current: reference.digest(),
                    });
                }
            }
            validate_object(reference, object.canonical_bytes())?;
            object_bytes =
                checked_sum(object_bytes, reference.encoded_len(), "WAL canonical object bytes")?;
            if *disposition == LogPackObjectDispositionV1::Data {
                inline_count =
                    inline_count
                        .checked_add(1)
                        .ok_or(CanonicalFormatError::LengthOverflow {
                            field: "WAL inline object count",
                        })?;
                inline_object_bytes = checked_sum(
                    inline_object_bytes,
                    reference.encoded_len(),
                    "WAL inline canonical object bytes",
                )?;
            }
            previous = Some(reference.digest());
        }
        if object_plan.logical_object_bytes() != object_bytes ||
            object_plan.inline_object_bytes() != inline_object_bytes ||
            object_plan.inline_count() != inline_count
        {
            return Err(LogPackFormatError::ObjectPlanInventory);
        }
        require_limit("canonical object bytes", limits.max_object_bytes, object_bytes)?;
        let object_record_bytes = checked_sum(
            object_count.checked_mul(OBJECT_RECORD_HEADER_LEN).ok_or(
                CanonicalFormatError::LengthOverflow {
                    field: "WAL object record headers",
                },
            )?,
            inline_object_bytes,
            "WAL object records",
        )?;
        let record_len = checked_sum(
            checked_sum(PREPARE_HEADER_LEN, mutation_len, "WAL prepare record")?,
            checked_sum(object_record_bytes, PREPARE_TRAILER_LEN, "WAL prepare record")?,
            "WAL prepare record",
        )?;
        require_limit("record bytes", limits.max_record_bytes, record_len)?;
        Ok(Self {
            transaction,
            store_id,
            sequence,
            record_len,
            mutation_len,
            object_count,
            object_bytes,
            object_plan,
            _profile: PhantomData,
        })
    }

    pub const fn record_len(&self) -> u64 { self.record_len }

    pub fn write_to<S: CanonicalSink>(
        &self,
        sink: &mut S,
    ) -> Result<PrepareRecordIdentityV1, LogPackWriteError<S::Error>> {
        let receipt = self.transaction.receipt();
        let mutation = self.transaction.mutation_bytes();
        let mut header = [0u8; PREPARE_AUTH_HEADER_LEN];
        {
            let mut header_sink = ArraySink::new(&mut header);
            write_prepare_header(
                &mut header_sink,
                self.record_len,
                self.sequence,
                self.store_id,
                receipt,
                self.mutation_len,
                self.object_count,
                self.object_bytes,
            )
            .expect("the fixed prepare header length was precomputed");
            debug_assert_eq!(header_sink.written(), PREPARE_AUTH_HEADER_LEN);
        }
        let header_crc = crc32c(&header);
        let mut record = AuthenticatedSink::new(sink);
        record.write_all(&header).map_err(LogPackWriteError::Sink)?;
        record
            .write_all(&encode_crc32c(header_crc))
            .map_err(LogPackWriteError::Sink)?;
        record
            .write_all(mutation)
            .map_err(LogPackWriteError::Sink)?;
        for (object, disposition) in self
            .transaction
            .objects()
            .iter()
            .zip(self.object_plan.dispositions())
        {
            write_u64(&mut record, disposition.kind()).map_err(LogPackWriteError::Sink)?;
            write_reference(&mut record, object.reference()).map_err(LogPackWriteError::Sink)?;
            if *disposition == LogPackObjectDispositionV1::Data {
                record
                    .write_all(object.canonical_bytes())
                    .map_err(LogPackWriteError::Sink)?;
            }
        }
        let (record_crc, digest, written) = record.finish();
        let actual_record_len = written.checked_add(PREPARE_TRAILER_LEN).ok_or_else(|| {
            LogPackWriteError::Invariant(
                CanonicalFormatError::LengthOverflow {
                    field: "WAL emitted prepare record",
                }
                .into(),
            )
        })?;
        if actual_record_len != self.record_len {
            return Err(LogPackWriteError::Invariant(LogPackFormatError::LengthMismatch {
                declared: self.record_len,
                actual: actual_record_len,
            }));
        }
        sink.write_all(&encode_crc32c(record_crc))
            .map_err(LogPackWriteError::Sink)?;
        Ok(PrepareRecordIdentityV1 {
            sequence: self.sequence,
            store_id: self.store_id,
            receipt,
            record_len: self.record_len,
            digest,
            crc32c: record_crc,
        })
    }
}

/// A validated borrowed canonical-object record within a prepare image.
#[derive(Clone, Copy, Debug)]
pub struct LogPackObjectView<'a> {
    disposition: LogPackObjectDispositionV1,
    reference: ObjectRefV1,
    canonical_bytes: Option<&'a [u8]>,
    record_offset: u64,
    canonical_offset: Option<u64>,
}

impl<'a> LogPackObjectView<'a> {
    pub const fn disposition(self) -> LogPackObjectDispositionV1 { self.disposition }

    pub const fn reference(self) -> ObjectRefV1 { self.reference }

    pub const fn canonical_bytes(self) -> Option<&'a [u8]> { self.canonical_bytes }

    /// Offset of the object's disposition field from the prepare start.
    pub const fn record_offset(self) -> u64 { self.record_offset }

    /// Offset of an inline canonical body from the prepare start. Reuse
    /// records deliberately contain no body or physical address.
    pub const fn canonical_offset(self) -> Option<u64> { self.canonical_offset }
}

pub struct LogPackObjectIter<'a> {
    cursor: ReadCursor<'a>,
    remaining: u64,
    base_record_offset: u64,
}

impl<'a> Iterator for LogPackObjectIter<'a> {
    type Item = LogPackObjectView<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            debug_assert_eq!(self.cursor.remaining(), 0);
            return None;
        }
        self.remaining -= 1;
        let record_offset = self
            .base_record_offset
            .checked_add(self.cursor.offset() as u64)
            .expect("validated prepare record offsets fit u64");
        let disposition = read_object_disposition(&mut self.cursor)
            .expect("the prepare view validated each object disposition");
        let reference = ObjectRefV1::parse(&mut self.cursor)
            .expect("the prepare view validated each object reference");
        let (canonical_offset, canonical_bytes) = match disposition {
            LogPackObjectDispositionV1::Data => {
                let canonical_offset = self
                    .base_record_offset
                    .checked_add(self.cursor.offset() as u64)
                    .expect("validated prepare record offsets fit u64");
                let len = usize::try_from(reference.encoded_len())
                    .expect("the prepare view bounded each object length by its byte slice");
                let canonical_bytes = self
                    .cursor
                    .read_exact(len)
                    .expect("the prepare view validated each object extent");
                (Some(canonical_offset), Some(canonical_bytes))
            }
            LogPackObjectDispositionV1::Reuse => (None, None),
        };
        Some(LogPackObjectView {
            disposition,
            reference,
            canonical_bytes,
            record_offset,
            canonical_offset,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.remaining)
            .expect("validated object count is bounded by the record byte slice");
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for LogPackObjectIter<'_> {}

/// Physically and canonically validated, zero-copy view of one prepare record.
///
/// This does not establish durable-store collision equality, target-manifest
/// closure, or semantic-profile admission. Recovery must establish all three
/// before this record can contribute to authority.
#[derive(Clone, Copy, Debug)]
pub struct TransactionPrepareRecordViewV1<'a> {
    sequence: LogPackSequence,
    store_id: StoreId,
    receipt: SealedTransitionV1,
    mutation: MutationBatchViewV1<'a>,
    objects: &'a [u8],
    objects_offset: u64,
    object_count: u64,
    digest: PhysicalDigest,
    crc32c: u32,
    record_len: u64,
}

impl<'a> TransactionPrepareRecordViewV1<'a> {
    pub fn parse(bytes: &'a [u8], limits: LogPackDecodeLimits) -> Result<Self, LogPackFormatError> {
        let actual_len = checked_len(bytes.len(), "WAL prepare record")?;
        require_limit("record bytes", limits.max_record_bytes, actual_len)?;
        let minimum = usize::try_from(PREPARE_HEADER_LEN + PREPARE_TRAILER_LEN)
            .expect("fixed WAL lengths fit usize");
        if bytes.len() < minimum {
            return Err(CanonicalFormatError::Truncated {
                offset: 0,
                needed: minimum,
                available: bytes.len(),
            }
            .into());
        }
        let header = &bytes[..PREPARE_AUTH_HEADER_LEN];
        let declared_header_crc = decode_crc32c(
            bytes[PREPARE_AUTH_HEADER_LEN..PREPARE_AUTH_HEADER_LEN + CHECKSUM_FIELD_LEN]
                .try_into()
                .expect("fixed header CRC extent"),
        )
        .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
            field: "prepare header",
            encoded,
        })?;
        let actual_header_crc = crc32c(header);
        if declared_header_crc != actual_header_crc {
            return Err(LogPackFormatError::HeaderChecksum {
                expected: declared_header_crc,
                actual: actual_header_crc,
            });
        }

        let mut cursor = ReadCursor::new(header);
        require_domain(&mut cursor, PREPARE_DOMAIN)?;
        require_version_and_kind(&mut cursor, PREPARE_KIND)?;
        let declared_len = cursor.read_u64()?;
        if declared_len != actual_len {
            return Err(LogPackFormatError::LengthMismatch {
                declared: declared_len,
                actual: actual_len,
            });
        }
        let payload_end = bytes.len() - PREPARE_TRAILER_LEN as usize;
        let declared_crc = decode_crc32c(
            bytes[payload_end..]
                .try_into()
                .expect("fixed prepare CRC extent"),
        )
        .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
            field: "prepare record",
            encoded,
        })?;
        let actual_crc = crc32c(&bytes[..payload_end]);
        if declared_crc != actual_crc {
            return Err(LogPackFormatError::RecordChecksum {
                expected: declared_crc,
                actual: actual_crc,
            });
        }
        let digest = PhysicalDigest::of_bytes(&bytes[..payload_end]);
        let sequence = LogPackSequence::new(cursor.read_u64()?)?;
        let store_id = read_store_id(&mut cursor)?;
        let base = read_state(&mut cursor)?;
        let target = read_state(&mut cursor)?;
        let profile_id = read_profile_id(&mut cursor)?;
        let mutation_digest = cursor.read_digest()?;
        let mutation_len = cursor.read_u64()?;
        let object_count = cursor.read_u64()?;
        let object_bytes = cursor.read_u64()?;
        cursor.finish()?;

        require_limit("mutation bytes", limits.max_mutation_bytes, mutation_len)?;
        require_limit("objects", limits.max_objects, object_count)?;
        require_limit("canonical object bytes", limits.max_object_bytes, object_bytes)?;
        let payload = &bytes[PREPARE_HEADER_LEN as usize..payload_end];
        let mut payload_cursor = ReadCursor::new(payload);
        let mutation_len =
            usize::try_from(mutation_len).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "WAL mutation",
                value: mutation_len,
            })?;
        let mutation_bytes = payload_cursor.read_exact(mutation_len)?;
        let mutation =
            MutationBatchViewV1::parse_with_limits(mutation_bytes, MutationDecodeLimits {
                max_entries: limits.max_mutation_entries,
                max_key_bytes: limits.max_mutation_key_bytes,
            })?;
        let receipt =
            SealedTransitionV1::new_for_persistence(base, target, mutation_digest, profile_id);
        validate_mutation_receipt(mutation, receipt)?;

        let objects_start = payload_cursor.offset();
        let objects_offset = PREPARE_HEADER_LEN.checked_add(objects_start as u64).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "WAL object record offset",
            },
        )?;
        let mut observed_object_bytes = 0u64;
        let mut previous = None;
        for _ in 0..object_count {
            let disposition = read_object_disposition(&mut payload_cursor)?;
            let reference = ObjectRefV1::parse(&mut payload_cursor)?;
            if let Some(previous) = previous {
                if previous >= reference.digest() {
                    return Err(LogPackFormatError::ObjectsNotStrict {
                        previous,
                        current: reference.digest(),
                    });
                }
            }
            if disposition == LogPackObjectDispositionV1::Data {
                let object_len = usize::try_from(reference.encoded_len()).map_err(|_| {
                    CanonicalFormatError::LengthOutOfRange {
                        field: "WAL canonical object",
                        value: reference.encoded_len(),
                    }
                })?;
                let canonical_bytes = payload_cursor.read_exact(object_len)?;
                validate_object(reference, canonical_bytes)?;
            }
            observed_object_bytes = checked_sum(
                observed_object_bytes,
                reference.encoded_len(),
                "WAL canonical object bytes",
            )?;
            previous = Some(reference.digest());
        }
        payload_cursor.finish()?;
        if observed_object_bytes != object_bytes {
            return Err(LogPackFormatError::LengthMismatch {
                declared: object_bytes,
                actual: observed_object_bytes,
            });
        }
        let objects = &payload[objects_start..];
        Ok(Self {
            sequence,
            store_id,
            receipt,
            mutation,
            objects,
            objects_offset,
            object_count,
            digest,
            crc32c: declared_crc,
            record_len: actual_len,
        })
    }

    pub const fn sequence(self) -> LogPackSequence { self.sequence }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn receipt(self) -> SealedTransitionV1 { self.receipt }

    pub const fn mutation(self) -> MutationBatchViewV1<'a> { self.mutation }

    pub fn objects(self) -> LogPackObjectIter<'a> {
        LogPackObjectIter {
            cursor: ReadCursor::new(self.objects),
            remaining: self.object_count,
            base_record_offset: self.objects_offset,
        }
    }

    pub const fn objects_offset(self) -> u64 { self.objects_offset }

    pub const fn digest(self) -> PhysicalDigest { self.digest }

    pub const fn crc32c(self) -> u32 { self.crc32c }

    pub const fn record_len(self) -> u64 { self.record_len }
}

/// Fixed-size terminal record written only after the prepare data is durable.
#[derive(Clone)]
pub struct TransactionCommitRecordV1 {
    bytes: [u8; COMMIT_RECORD_LEN],
    digest: PhysicalDigest,
}

impl std::fmt::Debug for TransactionCommitRecordV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransactionCommitRecordV1")
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

impl TransactionCommitRecordV1 {
    pub fn new(
        receipt: SealedTransitionV1,
        store_id: StoreId,
        previous_commit: PhysicalDigest,
        prepare_address: LogPackAddress,
        prepare: PrepareRecordIdentityV1,
        publication: CommitPublicationV1,
    ) -> Result<Self, LogPackFormatError> {
        if prepare.store_id() != store_id {
            return Err(LogPackFormatError::Store {
                expected: store_id,
                actual: prepare.store_id(),
            });
        }
        if prepare.receipt() != receipt {
            return Err(LogPackFormatError::ReceiptMismatch);
        }
        if prepare_address.length() != prepare.record_len() {
            return Err(LogPackFormatError::AddressLength {
                address: prepare_address,
                declared: prepare.record_len(),
            });
        }
        if publication.current_state_canonical_bytes() < receipt.target().manifest().encoded_len() {
            return Err(LogPackFormatError::CurrentStateBytes {
                minimum: receipt.target().manifest().encoded_len(),
                actual: publication.current_state_canonical_bytes(),
            });
        }
        let mut bytes = [0u8; COMMIT_RECORD_LEN];
        {
            let mut sink = ArraySink::new(&mut bytes[..COMMIT_AUTH_LEN]);
            sink.write_all(COMMIT_DOMAIN).expect("fixed commit domain");
            write_u64(&mut sink, LOG_PACK_VERSION_V1).expect("fixed commit version");
            write_u64(&mut sink, COMMIT_KIND).expect("fixed commit kind");
            write_u64(&mut sink, COMMIT_RECORD_LEN as u64).expect("fixed commit length");
            write_u64(&mut sink, prepare.sequence().get()).expect("fixed commit sequence");
            sink.write_all(store_id.as_bytes()).expect("fixed store ID");
            sink.write_all(previous_commit.as_bytes())
                .expect("fixed previous digest");
            write_u64(&mut sink, prepare_address.segment()).expect("fixed prepare segment");
            write_u64(&mut sink, prepare_address.offset()).expect("fixed prepare offset");
            write_u64(&mut sink, prepare_address.length()).expect("fixed prepare length");
            sink.write_all(prepare.digest().as_bytes())
                .expect("fixed prepare digest");
            write_state(&mut sink, receipt.base()).expect("fixed base state");
            write_state(&mut sink, receipt.target()).expect("fixed target state");
            sink.write_all(receipt.profile_id().as_bytes())
                .expect("fixed profile ID");
            sink.write_all(receipt.mutation_digest().as_bytes())
                .expect("fixed mutation digest");
            write_u64(&mut sink, publication.durable_index_tail()).expect("fixed index tail");
            write_u64(&mut sink, publication.first_live_pack()).expect("fixed first-live pack");
            write_u64(&mut sink, publication.index_generation()).expect("fixed index generation");
            write_reference(&mut sink, publication.retention_manifest())
                .expect("fixed retention manifest");
            write_u64(&mut sink, publication.current_state_canonical_bytes())
                .expect("fixed current-state byte inventory");
            write_u64(&mut sink, publication.retained_canonical_bytes())
                .expect("fixed retained byte inventory");
            debug_assert_eq!(sink.written(), COMMIT_AUTH_LEN);
        }
        let crc = crc32c(&bytes[..COMMIT_AUTH_LEN]);
        bytes[COMMIT_AUTH_LEN..].copy_from_slice(&encode_crc32c(crc));
        let digest = PhysicalDigest::of_bytes(&bytes[..COMMIT_AUTH_LEN]);
        Ok(Self { bytes, digest })
    }

    pub const fn canonical_bytes(&self) -> &[u8; COMMIT_RECORD_LEN] { &self.bytes }

    pub const fn digest(&self) -> PhysicalDigest { self.digest }

    pub fn write_to<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        sink.write_all(&self.bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TransactionCommitViewV1 {
    sequence: LogPackSequence,
    store_id: StoreId,
    previous_commit: PhysicalDigest,
    prepare_address: LogPackAddress,
    prepare_digest: PhysicalDigest,
    receipt: SealedTransitionV1,
    publication: CommitPublicationV1,
    digest: PhysicalDigest,
}

impl TransactionCommitViewV1 {
    pub fn parse(bytes: &[u8]) -> Result<Self, LogPackFormatError> {
        let actual = checked_len(bytes.len(), "WAL commit record")?;
        if bytes.len() != COMMIT_RECORD_LEN {
            return Err(LogPackFormatError::LengthMismatch {
                declared: COMMIT_RECORD_LEN as u64,
                actual,
            });
        }
        let declared_crc = decode_crc32c(
            bytes[COMMIT_AUTH_LEN..]
                .try_into()
                .expect("fixed commit CRC extent"),
        )
        .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
            field: "commit record",
            encoded,
        })?;
        let actual_crc = crc32c(&bytes[..COMMIT_AUTH_LEN]);
        if declared_crc != actual_crc {
            return Err(LogPackFormatError::RecordChecksum {
                expected: declared_crc,
                actual: actual_crc,
            });
        }
        let mut cursor = ReadCursor::new(&bytes[..COMMIT_AUTH_LEN]);
        require_domain(&mut cursor, COMMIT_DOMAIN)?;
        require_version_and_kind(&mut cursor, COMMIT_KIND)?;
        let declared_len = cursor.read_u64()?;
        if declared_len != COMMIT_RECORD_LEN as u64 {
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
        let base = read_state(&mut cursor)?;
        let target = read_state(&mut cursor)?;
        let profile_id = read_profile_id(&mut cursor)?;
        let mutation_digest = cursor.read_digest()?;
        let durable_index_tail = cursor.read_u64()?;
        let first_live_pack = cursor.read_u64()?;
        let index_generation = cursor.read_u64()?;
        let retention_manifest = ObjectRefV1::parse(&mut cursor)?;
        let current_state_canonical_bytes = cursor.read_u64()?;
        let retained_canonical_bytes = cursor.read_u64()?;
        cursor.finish()?;
        let publication = CommitPublicationV1::new(
            first_live_pack,
            index_generation,
            durable_index_tail,
            retention_manifest,
            current_state_canonical_bytes,
            retained_canonical_bytes,
        )?;
        if publication.current_state_canonical_bytes() < target.manifest().encoded_len() {
            return Err(LogPackFormatError::CurrentStateBytes {
                minimum: target.manifest().encoded_len(),
                actual: publication.current_state_canonical_bytes(),
            });
        }
        Ok(Self {
            sequence,
            store_id,
            previous_commit,
            prepare_address,
            prepare_digest,
            receipt: SealedTransitionV1::new_for_persistence(
                base,
                target,
                mutation_digest,
                profile_id,
            ),
            publication,
            digest: PhysicalDigest::of_bytes(&bytes[..COMMIT_AUTH_LEN]),
        })
    }

    pub const fn sequence(self) -> LogPackSequence { self.sequence }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn previous_commit(self) -> PhysicalDigest { self.previous_commit }

    pub const fn prepare_address(self) -> LogPackAddress { self.prepare_address }

    pub const fn prepare_digest(self) -> PhysicalDigest { self.prepare_digest }

    pub const fn receipt(self) -> SealedTransitionV1 { self.receipt }

    pub const fn publication(self) -> CommitPublicationV1 { self.publication }

    pub const fn durable_index_tail(self) -> u64 { self.publication.durable_index_tail() }

    pub const fn digest(self) -> PhysicalDigest { self.digest }

    pub fn verify_prepare(
        self,
        prepare: TransactionPrepareRecordViewV1<'_>,
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
        if prepare.receipt() != self.receipt {
            return Err(LogPackFormatError::ReceiptMismatch);
        }
        Ok(())
    }
}

fn validate_mutation_receipt(
    mutation: MutationBatchViewV1<'_>,
    receipt: SealedTransitionV1,
) -> Result<(), LogPackFormatError> {
    if mutation.payload_digest() != receipt.mutation_digest() {
        return Err(LogPackFormatError::MutationDigest {
            expected: receipt.mutation_digest(),
            actual: mutation.payload_digest(),
        });
    }
    if mutation.base() != receipt.base() {
        return Err(LogPackFormatError::MutationReceipt {
            expected: receipt.base(),
            actual: mutation.base(),
        });
    }
    if mutation.target_generation() != receipt.target().generation() {
        return Err(LogPackFormatError::MutationReceipt {
            expected: receipt.target(),
            actual: StateVersionV1::new(mutation.target_generation(), receipt.target().manifest()),
        });
    }
    Ok(())
}

fn validate_object(
    declared: ObjectRefV1,
    canonical_bytes: &[u8],
) -> Result<(), LogPackFormatError> {
    let object = CanonicalObjectView::parse(canonical_bytes)?;
    let actual = match object {
        CanonicalObjectView::DagNode(node) => node.object_ref(),
        CanonicalObjectView::Blob(blob) => blob.object_ref(),
        CanonicalObjectView::SemanticManifest(manifest) => manifest.object_ref(),
        CanonicalObjectView::AcceleratorManifest(manifest) => manifest.object_ref(),
        CanonicalObjectView::RetentionManifest(manifest) => manifest.object_ref(),
    };
    if actual != declared {
        return Err(LogPackFormatError::ObjectReference { declared, actual });
    }
    Ok(())
}

fn read_object_disposition(
    cursor: &mut ReadCursor<'_>,
) -> Result<LogPackObjectDispositionV1, LogPackFormatError> {
    match cursor.read_u64()? {
        OBJECT_DATA_KIND_V1 => Ok(LogPackObjectDispositionV1::Data),
        OBJECT_REUSE_KIND_V1 => Ok(LogPackObjectDispositionV1::Reuse),
        kind => Err(LogPackFormatError::UnsupportedObjectDisposition(kind)),
    }
}

fn require_limit(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), LogPackFormatError> {
    if attempted > limit {
        return Err(LogPackFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        });
    }
    Ok(())
}

fn write_prepare_header<S: CanonicalSink>(
    sink: &mut S,
    record_len: u64,
    sequence: LogPackSequence,
    store_id: StoreId,
    receipt: SealedTransitionV1,
    mutation_len: u64,
    object_count: u64,
    object_bytes: u64,
) -> Result<(), S::Error> {
    sink.write_all(PREPARE_DOMAIN)?;
    write_u64(sink, LOG_PACK_VERSION_V1)?;
    write_u64(sink, PREPARE_KIND)?;
    write_u64(sink, record_len)?;
    write_u64(sink, sequence.get())?;
    sink.write_all(store_id.as_bytes())?;
    write_state(sink, receipt.base())?;
    write_state(sink, receipt.target())?;
    sink.write_all(receipt.profile_id().as_bytes())?;
    sink.write_all(receipt.mutation_digest().as_bytes())?;
    write_u64(sink, mutation_len)?;
    write_u64(sink, object_count)?;
    write_u64(sink, object_bytes)
}

fn write_state<S: CanonicalSink>(sink: &mut S, state: StateVersionV1) -> Result<(), S::Error> {
    write_u64(sink, state.generation())?;
    write_reference(sink, state.manifest())
}

fn write_reference<S: CanonicalSink>(sink: &mut S, reference: ObjectRefV1) -> Result<(), S::Error> {
    sink.write_all(reference.digest().as_bytes())?;
    write_u64(sink, reference.encoded_len())
}

fn read_state(cursor: &mut ReadCursor<'_>) -> Result<StateVersionV1, CanonicalFormatError> {
    Ok(StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(cursor)?))
}

fn read_store_id(cursor: &mut ReadCursor<'_>) -> Result<StoreId, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(STORE_ID_LEN)?
        .try_into()
        .expect("fixed store ID extent");
    Ok(StoreId::from_array(bytes))
}

fn read_profile_id(cursor: &mut ReadCursor<'_>) -> Result<SemanticProfileId, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(SEMANTIC_PROFILE_ID_LEN)?
        .try_into()
        .expect("fixed profile ID extent");
    Ok(SemanticProfileId::from_array(bytes))
}

fn read_physical_digest(
    cursor: &mut ReadCursor<'_>,
) -> Result<PhysicalDigest, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(PHYSICAL_DIGEST_LEN)?
        .try_into()
        .expect("fixed physical digest extent");
    Ok(PhysicalDigest::from_array(bytes))
}

fn require_domain(
    cursor: &mut ReadCursor<'_>,
    expected: &[u8; OBJECT_DOMAIN_LEN],
) -> Result<(), LogPackFormatError> {
    if cursor.read_exact(OBJECT_DOMAIN_LEN)? != expected {
        return Err(LogPackFormatError::InvalidDomain);
    }
    Ok(())
}

fn require_version_and_kind(
    cursor: &mut ReadCursor<'_>,
    expected_kind: u64,
) -> Result<(), LogPackFormatError> {
    let version = cursor.read_u64()?;
    if version != LOG_PACK_VERSION_V1 {
        return Err(LogPackFormatError::UnsupportedVersion(version));
    }
    let kind = cursor.read_u64()?;
    if kind != expected_kind {
        return Err(LogPackFormatError::InvalidDomain);
    }
    Ok(())
}

struct AuthenticatedSink<'sink, S: CanonicalSink> {
    sink: &'sink mut S,
    crc: Crc32c,
    hasher: Blake2b<U32>,
    written: u64,
}

impl<'sink, S: CanonicalSink> AuthenticatedSink<'sink, S> {
    fn new(sink: &'sink mut S) -> Self {
        Self {
            sink,
            crc: Crc32c::new(),
            hasher: Blake2b::<U32>::new(),
            written: 0,
        }
    }

    fn finish(self) -> (u32, PhysicalDigest, u64) {
        let hash = self.hasher.finalize();
        let mut digest = [0; PHYSICAL_DIGEST_LEN];
        digest.copy_from_slice(&hash);
        (self.crc.finish(), PhysicalDigest::from_array(digest), self.written)
    }
}

impl<S: CanonicalSink> CanonicalSink for AuthenticatedSink<'_, S> {
    type Error = S::Error;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.sink.write_all(bytes)?;
        self.crc.update(bytes);
        self.hasher.update(bytes);
        self.written = self
            .written
            .checked_add(bytes.len() as u64)
            .expect("a prepared WAL record has a validated u64 length");
        Ok(())
    }
}

struct ArraySink<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}

impl<'a> ArraySink<'a> {
    fn new(bytes: &'a mut [u8]) -> Self { Self { bytes, offset: 0 } }

    const fn written(&self) -> usize { self.offset }
}

impl CanonicalSink for ArraySink<'_> {
    type Error = Infallible;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let end = self
            .offset
            .checked_add(bytes.len())
            .expect("fixed record offset cannot overflow");
        assert!(end <= self.bytes.len(), "fixed record layout overflow");
        self.bytes[self.offset..end].copy_from_slice(bytes);
        self.offset = end;
        Ok(())
    }
}
