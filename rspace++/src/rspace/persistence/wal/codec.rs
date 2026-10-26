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
    CanonicalFormatError, CanonicalSink, OBJECT_DIGEST_LEN, OBJECT_DOMAIN_LEN, ObjectDigest,
    ObjectRefV1, ReadCursor, checked_len, checked_sum, write_u64,
};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, Crc32c, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::durable::PACK_HEADER_LEN_V1;
use crate::rspace::persistence::manifest::{SEMANTIC_PROFILE_ID_LEN, SemanticProfileId};
use crate::rspace::persistence::mutation::{
    MutationBatchViewV1, MutationDecodeLimits, StateVersionV1,
};
use crate::rspace::persistence::object::{
    CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView,
};
use crate::rspace::persistence::physical::{
    PHYSICAL_DIGEST_LEN, PhysicalDigest, STORE_ID_LEN, StoreId,
};
use crate::rspace::persistence::retention::v2_format::{
    RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionReader,
};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::transaction::{
    SealedDagTransaction, SealedObject, SealedTransitionV1,
};

pub(super) const PREPARE_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceWAL.Prep\0\0";
pub(super) const COMMIT_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceWAL.Comm\0\0";
pub(super) const LOG_PACK_VERSION_V1: u64 = 1;
pub(super) const TRANSACTION_PREPARE_KIND_V1: u64 = 1;
pub(super) const GENERATION_CHECKPOINT_PREPARE_KIND_V1: u64 = 3;
pub(super) const TRANSACTION_COMMIT_KIND_V1: u64 = 2;
pub(super) const GENERATION_CHECKPOINT_COMMIT_KIND_V1: u64 = 4;
pub const PREPARE_DISCRIMINATOR_LEN_V1: u64 = (OBJECT_DOMAIN_LEN + 8 + 8) as u64;
pub const COMMIT_DISCRIMINATOR_LEN_V1: u64 = (OBJECT_DOMAIN_LEN + 8 + 8) as u64;
const OBJECT_REF_LEN: u64 = (OBJECT_DIGEST_LEN + 8) as u64;
pub(crate) const OBJECT_RECORD_HEADER_LEN_V1: u64 = 8 + OBJECT_REF_LEN;
const PREPARE_AUTH_HEADER_LEN: usize = 232;
pub const TRANSACTION_PREPARE_HEADER_LEN_V1: u64 =
    PREPARE_AUTH_HEADER_LEN as u64 + CHECKSUM_FIELD_LEN as u64;
const PREPARE_HEADER_LEN: u64 = TRANSACTION_PREPARE_HEADER_LEN_V1;
const PREPARE_TRAILER_LEN: u64 = CHECKSUM_FIELD_LEN as u64;
const COMMIT_AUTH_LEN: usize = 376;
pub const TRANSACTION_COMMIT_RECORD_LEN_V1: u64 = (COMMIT_AUTH_LEN + CHECKSUM_FIELD_LEN) as u64;
const COMMIT_RECORD_LEN: usize = TRANSACTION_COMMIT_RECORD_LEN_V1 as usize;

/// An unauthenticated prepare-record discriminator.
///
/// This value is permitted to select only one of the two small, fixed header
/// widths. The selected header must then pass its kind-specific checksum and
/// resource preflight before any declared variable extent is trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareRecordKindV1 {
    Incremental,
    GenerationCheckpoint,
}

impl PrepareRecordKindV1 {
    pub const fn fixed_header_len(self) -> u64 {
        match self {
            Self::Incremental => TRANSACTION_PREPARE_HEADER_LEN_V1,
            Self::GenerationCheckpoint => {
                super::checkpoint::GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1
            }
        }
    }
}

/// Decode the fixed domain/version/kind discriminator used to choose an
/// authenticated prepare-header parser.
///
/// The result is not record authentication and carries no authority over a
/// record length, object count, or semantic field.
pub fn decode_prepare_kind_v1(prefix: &[u8]) -> Result<PrepareRecordKindV1, LogPackFormatError> {
    let actual = checked_len(prefix.len(), "WAL prepare discriminator")?;
    if actual != PREPARE_DISCRIMINATOR_LEN_V1 {
        return Err(LogPackFormatError::LengthMismatch {
            declared: PREPARE_DISCRIMINATOR_LEN_V1,
            actual,
        });
    }
    let mut cursor = ReadCursor::new(prefix);
    require_domain(&mut cursor, PREPARE_DOMAIN)?;
    let version = cursor.read_u64()?;
    if version != LOG_PACK_VERSION_V1 {
        return Err(LogPackFormatError::UnsupportedVersion(version));
    }
    let kind = cursor.read_u64()?;
    cursor.finish()?;
    match kind {
        TRANSACTION_PREPARE_KIND_V1 => Ok(PrepareRecordKindV1::Incremental),
        GENERATION_CHECKPOINT_PREPARE_KIND_V1 => Ok(PrepareRecordKindV1::GenerationCheckpoint),
        _ => Err(LogPackFormatError::UnsupportedPrepareKind(kind)),
    }
}

/// An unauthenticated COMMIT discriminator used only to select a fixed record
/// width. The selected record must still pass its complete kind-specific CRC
/// and canonical parser before any field becomes authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitRecordKindV1 {
    Incremental,
    GenerationCheckpoint,
}

impl CommitRecordKindV1 {
    pub const fn fixed_record_len(self) -> u64 {
        match self {
            Self::Incremental => TRANSACTION_COMMIT_RECORD_LEN_V1,
            Self::GenerationCheckpoint => {
                super::checkpoint_commit::GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1
            }
        }
    }
}

/// Decode only the domain/version/kind discriminator of a COMMIT record.
///
/// This unauthenticated result carries no authority over any variable or
/// semantic field and may select only one of two small fixed record widths.
pub fn decode_commit_kind_v1(prefix: &[u8]) -> Result<CommitRecordKindV1, LogPackFormatError> {
    let actual = checked_len(prefix.len(), "WAL commit discriminator")?;
    if actual != COMMIT_DISCRIMINATOR_LEN_V1 {
        return Err(LogPackFormatError::LengthMismatch {
            declared: COMMIT_DISCRIMINATOR_LEN_V1,
            actual,
        });
    }
    let mut cursor = ReadCursor::new(prefix);
    require_domain(&mut cursor, COMMIT_DOMAIN)?;
    let version = cursor.read_u64()?;
    if version != LOG_PACK_VERSION_V1 {
        return Err(LogPackFormatError::UnsupportedVersion(version));
    }
    let kind = cursor.read_u64()?;
    cursor.finish()?;
    match kind {
        TRANSACTION_COMMIT_KIND_V1 => Ok(CommitRecordKindV1::Incremental),
        GENERATION_CHECKPOINT_COMMIT_KIND_V1 => Ok(CommitRecordKindV1::GenerationCheckpoint),
        _ => Err(LogPackFormatError::UnsupportedCommitKind(kind)),
    }
}

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

    pub const fn from_streamed(
        preflight: PrepareRecordPreflightV1,
        digest: PhysicalDigest,
        crc32c: u32,
    ) -> Self {
        Self {
            sequence: preflight.sequence,
            store_id: preflight.store_id,
            receipt: preflight.receipt(),
            record_len: preflight.record_len,
            digest,
            crc32c,
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

/// Checked physical layout of one sealed object in a prepared record.
///
/// This is the publisher/index bridge. It carries no copied body and prevents
/// index construction from reparsing bytes just emitted or independently
/// reimplementing WAL offset arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreparedObjectLayoutV1 {
    disposition: LogPackObjectDispositionV1,
    reference: ObjectRefV1,
    record_offset: u64,
    canonical_offset: Option<u64>,
}

impl PreparedObjectLayoutV1 {
    pub(crate) const fn disposition(self) -> LogPackObjectDispositionV1 { self.disposition }

    pub(crate) const fn reference(self) -> ObjectRefV1 { self.reference }

    pub(crate) const fn record_offset(self) -> u64 { self.record_offset }

    pub(crate) const fn canonical_offset(self) -> Option<u64> { self.canonical_offset }
}

pub(crate) struct PreparedObjectLayoutIter<'a> {
    objects: std::slice::Iter<'a, SealedObject>,
    dispositions: std::slice::Iter<'a, LogPackObjectDispositionV1>,
    next_record_offset: u64,
    expected_payload_end: u64,
}

impl Iterator for PreparedObjectLayoutIter<'_> {
    type Item = PreparedObjectLayoutV1;

    fn next(&mut self) -> Option<Self::Item> {
        let object = match self.objects.next() {
            Some(object) => object,
            None => {
                debug_assert!(self.dispositions.next().is_none());
                debug_assert_eq!(self.next_record_offset, self.expected_payload_end);
                return None;
            }
        };
        let disposition = *self
            .dispositions
            .next()
            .expect("validated object plan has one disposition per sealed object");
        let record_offset = self.next_record_offset;
        let body_offset = record_offset
            .checked_add(OBJECT_RECORD_HEADER_LEN_V1)
            .expect("preflighted object record offset cannot overflow");
        let canonical_offset = match disposition {
            LogPackObjectDispositionV1::Data => Some(body_offset),
            LogPackObjectDispositionV1::Reuse => None,
        };
        self.next_record_offset = match disposition {
            LogPackObjectDispositionV1::Data => body_offset
                .checked_add(object.reference().encoded_len())
                .expect("preflighted inline object extent cannot overflow"),
            LogPackObjectDispositionV1::Reuse => body_offset,
        };
        Some(PreparedObjectLayoutV1 {
            disposition,
            reference: object.reference(),
            record_offset,
            canonical_offset,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.objects.len();
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for PreparedObjectLayoutIter<'_> {}

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
            require_limit(
                "single referenced object bytes",
                limits.max_single_object_bytes,
                reference.encoded_len(),
            )?;
            if let Some(previous) = previous {
                if previous >= reference.digest() {
                    return Err(LogPackFormatError::ObjectsNotStrict {
                        previous,
                        current: reference.digest(),
                    });
                }
            }
            validate_object_for(
                reference,
                object.canonical_bytes(),
                transaction.retention_reader(),
            )?;
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
            object_count
                .checked_mul(OBJECT_RECORD_HEADER_LEN_V1)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "WAL object record headers",
                })?,
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

    pub const fn receipt(&self) -> SealedTransitionV1 { self.transaction.receipt() }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn incremental_validation_canonical_bytes(&self) -> u64 {
        self.transaction.diagnostics().closure.canonical_bytes
    }

    pub(crate) const fn retention_manifest(&self) -> Option<ObjectRefV1> {
        self.transaction.retention_manifest()
    }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub(crate) fn objects(&self) -> &[crate::rspace::persistence::SealedObject] {
        self.transaction.objects()
    }

    pub(crate) const fn store_id(&self) -> StoreId { self.store_id }

    pub(crate) const fn sequence(&self) -> LogPackSequence { self.sequence }

    pub(crate) const fn inline_count(&self) -> u64 { self.object_plan.inline_count() }

    /// Canonical bytes physically inlined by this prepare record. Reused
    /// objects are excluded from the inventory.
    pub(crate) const fn inline_object_bytes(&self) -> u64 { self.object_plan.inline_object_bytes() }

    pub(crate) fn object_layouts(&self) -> PreparedObjectLayoutIter<'_> {
        let first_record_offset = PREPARE_HEADER_LEN
            .checked_add(self.mutation_len)
            .expect("preflighted prepare payload offset cannot overflow");
        let expected_payload_end = self
            .record_len
            .checked_sub(PREPARE_TRAILER_LEN)
            .expect("preflighted prepare contains its trailer");
        PreparedObjectLayoutIter {
            objects: self.transaction.objects().iter(),
            dispositions: self.object_plan.dispositions().iter(),
            next_record_offset: first_record_offset,
            expected_payload_end,
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LogPackObjectHeaderV1 {
    disposition: LogPackObjectDispositionV1,
    reference: ObjectRefV1,
}

impl LogPackObjectHeaderV1 {
    pub(crate) fn parse_exact(bytes: &[u8]) -> Result<Self, LogPackFormatError> {
        let actual = checked_len(bytes.len(), "WAL object record header")?;
        if actual != OBJECT_RECORD_HEADER_LEN_V1 {
            return Err(LogPackFormatError::LengthMismatch {
                declared: OBJECT_RECORD_HEADER_LEN_V1,
                actual,
            });
        }
        let mut cursor = ReadCursor::new(bytes);
        let disposition = read_object_disposition(&mut cursor)?;
        let reference = ObjectRefV1::parse(&mut cursor)?;
        cursor.finish()?;
        Ok(Self {
            disposition,
            reference,
        })
    }

    pub(crate) const fn disposition(self) -> LogPackObjectDispositionV1 { self.disposition }

    pub(crate) const fn reference(self) -> ObjectRefV1 { self.reference }
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
        let header_bytes = self
            .cursor
            .read_exact(OBJECT_RECORD_HEADER_LEN_V1 as usize)
            .expect("the prepare view validated each object header extent");
        let header = LogPackObjectHeaderV1::parse_exact(header_bytes)
            .expect("the prepare view validated each object header");
        let disposition = header.disposition();
        let reference = header.reference();
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

#[derive(Clone, Copy)]
struct PrepareHeaderV1 {
    declared_len: u64,
    sequence: LogPackSequence,
    store_id: StoreId,
    base: StateVersionV1,
    target: StateVersionV1,
    profile_id: SemanticProfileId,
    mutation_digest: ObjectDigest,
    mutation_len: u64,
    object_count: u64,
    object_bytes: u64,
}

/// Authenticated resource declaration extracted from a fixed prepare prefix.
///
/// Durable recovery uses this descriptor to charge aggregate scan/object work
/// before allocating the record buffer or issuing any variable-size read. The
/// values are trustworthy for admission because the prefix CRC covers every
/// field exposed here; the complete record CRC and canonical parser remain
/// mandatory after admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepareRecordPreflightV1 {
    sequence: LogPackSequence,
    store_id: StoreId,
    base: StateVersionV1,
    target: StateVersionV1,
    profile_id: SemanticProfileId,
    mutation_digest: ObjectDigest,
    record_len: u64,
    mutation_len: u64,
    object_count: u64,
    object_bytes: u64,
}

impl PrepareRecordPreflightV1 {
    pub const fn sequence(self) -> LogPackSequence { self.sequence }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn base(self) -> StateVersionV1 { self.base }

    pub const fn target(self) -> StateVersionV1 { self.target }

    pub const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub const fn mutation_digest(self) -> ObjectDigest { self.mutation_digest }

    pub const fn receipt(self) -> SealedTransitionV1 {
        SealedTransitionV1::new_for_persistence(
            self.base,
            self.target,
            self.mutation_digest,
            self.profile_id,
        )
    }

    pub const fn record_len(self) -> u64 { self.record_len }

    pub const fn mutation_len(self) -> u64 { self.mutation_len }

    pub const fn object_count(self) -> u64 { self.object_count }

    pub const fn object_bytes(self) -> u64 { self.object_bytes }
}

/// Validate the fixed prepare prefix before a durable reader trusts its
/// declared record extent. The checksum covers every length/count field, and
/// explicit limits are applied before allocation or a variable-size read.
pub fn preflight_prepare_record_len_v1(
    prefix: &[u8],
    limits: LogPackDecodeLimits,
) -> Result<u64, LogPackFormatError> {
    Ok(preflight_prepare_record_v1(prefix, limits)?.record_len())
}

/// Authenticate and validate all fixed-prefix resource declarations.
///
/// Unlike [`preflight_prepare_record_len_v1`], this interface lets a caller
/// reserve aggregate object work as well as record bytes before it trusts the
/// variable body.
pub fn preflight_prepare_record_v1(
    prefix: &[u8],
    limits: LogPackDecodeLimits,
) -> Result<PrepareRecordPreflightV1, LogPackFormatError> {
    let header = parse_prepare_header_v1(prefix, limits)?;
    Ok(PrepareRecordPreflightV1 {
        sequence: header.sequence,
        store_id: header.store_id,
        base: header.base,
        target: header.target,
        profile_id: header.profile_id,
        mutation_digest: header.mutation_digest,
        record_len: header.declared_len,
        mutation_len: header.mutation_len,
        object_count: header.object_count,
        object_bytes: header.object_bytes,
    })
}

fn parse_prepare_header_v1(
    prefix: &[u8],
    limits: LogPackDecodeLimits,
) -> Result<PrepareHeaderV1, LogPackFormatError> {
    let actual = checked_len(prefix.len(), "WAL prepare header")?;
    if actual != PREPARE_HEADER_LEN {
        return Err(LogPackFormatError::LengthMismatch {
            declared: PREPARE_HEADER_LEN,
            actual,
        });
    }
    let header = &prefix[..PREPARE_AUTH_HEADER_LEN];
    let declared_header_crc = decode_crc32c(
        prefix[PREPARE_AUTH_HEADER_LEN..]
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
    require_version_and_kind(&mut cursor, TRANSACTION_PREPARE_KIND_V1)?;
    let declared_len = cursor.read_u64()?;
    let minimum = PREPARE_HEADER_LEN.checked_add(PREPARE_TRAILER_LEN).ok_or(
        CanonicalFormatError::LengthOverflow {
            field: "WAL minimum prepare record",
        },
    )?;
    if declared_len < minimum {
        return Err(LogPackFormatError::LengthMismatch {
            declared: declared_len,
            actual: minimum,
        });
    }
    require_limit("record bytes", limits.max_record_bytes, declared_len)?;
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
    Ok(PrepareHeaderV1 {
        declared_len,
        sequence,
        store_id,
        base,
        target,
        profile_id,
        mutation_digest,
        mutation_len,
        object_count,
        object_bytes,
    })
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
        let header = parse_prepare_header_v1(&bytes[..PREPARE_HEADER_LEN as usize], limits)?;
        if header.declared_len != actual_len {
            return Err(LogPackFormatError::LengthMismatch {
                declared: header.declared_len,
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
        let payload = &bytes[PREPARE_HEADER_LEN as usize..payload_end];
        let mut payload_cursor = ReadCursor::new(payload);
        let mutation_len = usize::try_from(header.mutation_len).map_err(|_| {
            CanonicalFormatError::LengthOutOfRange {
                field: "WAL mutation",
                value: header.mutation_len,
            }
        })?;
        let mutation_bytes = payload_cursor.read_exact(mutation_len)?;
        let mutation =
            MutationBatchViewV1::parse_with_limits(mutation_bytes, MutationDecodeLimits {
                max_entries: limits.max_mutation_entries,
                max_key_bytes: limits.max_mutation_key_bytes,
            })?;
        let receipt = SealedTransitionV1::new_for_persistence(
            header.base,
            header.target,
            header.mutation_digest,
            header.profile_id,
        );
        validate_mutation_receipt(mutation, receipt)?;

        let objects_start = payload_cursor.offset();
        let objects_offset = PREPARE_HEADER_LEN.checked_add(objects_start as u64).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "WAL object record offset",
            },
        )?;
        let mut observed_object_bytes = 0u64;
        let mut previous = None;
        for _ in 0..header.object_count {
            let object_header = payload_cursor.read_exact(OBJECT_RECORD_HEADER_LEN_V1 as usize)?;
            let object_header = LogPackObjectHeaderV1::parse_exact(object_header)?;
            let disposition = object_header.disposition();
            let reference = object_header.reference();
            if let Some(previous) = previous {
                if previous >= reference.digest() {
                    return Err(LogPackFormatError::ObjectsNotStrict {
                        previous,
                        current: reference.digest(),
                    });
                }
            }
            require_limit(
                "single referenced object bytes",
                limits.max_single_object_bytes,
                reference.encoded_len(),
            )?;
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
        if observed_object_bytes != header.object_bytes {
            return Err(LogPackFormatError::LengthMismatch {
                declared: header.object_bytes,
                actual: observed_object_bytes,
            });
        }
        let objects = &payload[objects_start..];
        Ok(Self {
            sequence: header.sequence,
            store_id: header.store_id,
            receipt,
            mutation,
            objects,
            objects_offset,
            object_count: header.object_count,
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
        require_record_address(prepare_address, "prepare record")?;
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
            write_u64(&mut sink, TRANSACTION_COMMIT_KIND_V1).expect("fixed commit kind");
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
        require_version_and_kind(&mut cursor, TRANSACTION_COMMIT_KIND_V1)?;
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
        require_record_address(prepare_address, "prepare record")?;
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
        self.verify_prepare_identity(PrepareRecordIdentityV1::from_record_view(prepare))
    }

    pub fn verify_prepare_identity(
        self,
        prepare: PrepareRecordIdentityV1,
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

pub(super) fn require_record_address(
    address: LogPackAddress,
    field: &'static str,
) -> Result<(), LogPackFormatError> {
    if address.offset() < PACK_HEADER_LEN_V1 {
        return Err(LogPackFormatError::AddressBeforePackHeader {
            field,
            offset: address.offset(),
            header_len: PACK_HEADER_LEN_V1,
        });
    }
    Ok(())
}

fn validate_mutation_receipt(
    mutation: MutationBatchViewV1<'_>,
    receipt: SealedTransitionV1,
) -> Result<(), LogPackFormatError> {
    validate_streamed_mutation_receipt_v1(
        mutation.payload_digest(),
        mutation.base(),
        mutation.target_generation(),
        receipt,
    )
}

pub(crate) fn validate_streamed_mutation_receipt_v1(
    payload_digest: ObjectDigest,
    base: StateVersionV1,
    target_generation: u64,
    receipt: SealedTransitionV1,
) -> Result<(), LogPackFormatError> {
    if payload_digest != receipt.mutation_digest() {
        return Err(LogPackFormatError::MutationDigest {
            expected: receipt.mutation_digest(),
            actual: payload_digest,
        });
    }
    if base != receipt.base() {
        return Err(LogPackFormatError::MutationReceipt {
            expected: receipt.base(),
            actual: base,
        });
    }
    if target_generation != receipt.target().generation() {
        return Err(LogPackFormatError::MutationReceipt {
            expected: receipt.target(),
            actual: StateVersionV1::new(target_generation, receipt.target().manifest()),
        });
    }
    Ok(())
}

pub(super) fn validate_object(
    declared: ObjectRefV1,
    canonical_bytes: &[u8],
) -> Result<CanonicalObjectKind, LogPackFormatError> {
    let object = CanonicalObjectView::parse(canonical_bytes)?;
    let kind = object.kind();
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
    Ok(kind)
}

/// Validate one contiguous canonical object for a reader with the given
/// retention capability. A V1-only reader is exactly [`validate_object`].
pub(crate) fn validate_object_for(
    declared: ObjectRefV1,
    canonical_bytes: &[u8],
    reader: RetentionReader,
) -> Result<CanonicalObjectKind, LogPackFormatError> {
    let kind = match CanonicalObjectKind::from_domain_prefix_for(canonical_bytes, reader) {
        Ok(CanonicalObjectKind::RetentionAnchor) => {
            RetentionAnchorV2::parse(canonical_bytes)?;
            CanonicalObjectKind::RetentionAnchor
        }
        Ok(CanonicalObjectKind::RetentionRadixNode) => {
            CanonicalDagNodeView::parse_in_domain(
                canonical_bytes,
                RETENTION_RADIX_DOMAIN,
                "RSpace retention radix node",
            )?;
            CanonicalObjectKind::RetentionRadixNode
        }
        _ => return validate_object(declared, canonical_bytes),
    };
    let actual = ObjectRefV1::new(
        ObjectDigest::of_canonical_bytes(canonical_bytes),
        checked_len(canonical_bytes.len(), "WAL canonical object")?,
    )?;
    if actual != declared {
        return Err(LogPackFormatError::ObjectReference { declared, actual });
    }
    Ok(kind)
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

pub(super) fn require_limit(
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
    write_u64(sink, TRANSACTION_PREPARE_KIND_V1)?;
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

pub(super) fn write_state<S: CanonicalSink>(
    sink: &mut S,
    state: StateVersionV1,
) -> Result<(), S::Error> {
    write_u64(sink, state.generation())?;
    write_reference(sink, state.manifest())
}

pub(super) fn write_reference<S: CanonicalSink>(
    sink: &mut S,
    reference: ObjectRefV1,
) -> Result<(), S::Error> {
    sink.write_all(reference.digest().as_bytes())?;
    write_u64(sink, reference.encoded_len())
}

pub(super) fn read_state(
    cursor: &mut ReadCursor<'_>,
) -> Result<StateVersionV1, CanonicalFormatError> {
    Ok(StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(cursor)?))
}

pub(super) fn read_store_id(cursor: &mut ReadCursor<'_>) -> Result<StoreId, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(STORE_ID_LEN)?
        .try_into()
        .expect("fixed store ID extent");
    Ok(StoreId::from_array(bytes))
}

pub(super) fn read_profile_id(
    cursor: &mut ReadCursor<'_>,
) -> Result<SemanticProfileId, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(SEMANTIC_PROFILE_ID_LEN)?
        .try_into()
        .expect("fixed profile ID extent");
    Ok(SemanticProfileId::from_array(bytes))
}

pub(super) fn read_physical_digest(
    cursor: &mut ReadCursor<'_>,
) -> Result<PhysicalDigest, CanonicalFormatError> {
    let bytes = cursor
        .read_exact(PHYSICAL_DIGEST_LEN)?
        .try_into()
        .expect("fixed physical digest extent");
    Ok(PhysicalDigest::from_array(bytes))
}

pub(super) fn require_domain(
    cursor: &mut ReadCursor<'_>,
    expected: &[u8; OBJECT_DOMAIN_LEN],
) -> Result<(), LogPackFormatError> {
    if cursor.read_exact(OBJECT_DOMAIN_LEN)? != expected {
        return Err(LogPackFormatError::InvalidDomain);
    }
    Ok(())
}

pub(super) fn require_version_and_kind(
    cursor: &mut ReadCursor<'_>,
    expected_kind: u64,
) -> Result<(), LogPackFormatError> {
    let version = cursor.read_u64()?;
    if version != LOG_PACK_VERSION_V1 {
        return Err(LogPackFormatError::UnsupportedVersion(version));
    }
    let kind = cursor.read_u64()?;
    if kind != expected_kind {
        return Err(LogPackFormatError::UnexpectedRecordKind {
            expected: expected_kind,
            actual: kind,
        });
    }
    Ok(())
}

pub(super) struct AuthenticatedSink<'sink, S: CanonicalSink> {
    sink: &'sink mut S,
    crc: Crc32c,
    hasher: Blake2b<U32>,
    written: u64,
}

impl<'sink, S: CanonicalSink> AuthenticatedSink<'sink, S> {
    pub(super) fn new(sink: &'sink mut S) -> Self {
        Self {
            sink,
            crc: Crc32c::new(),
            hasher: Blake2b::<U32>::new(),
            written: 0,
        }
    }

    pub(super) fn finish(self) -> (u32, PhysicalDigest, u64) {
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

pub(super) struct ArraySink<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}

impl<'a> ArraySink<'a> {
    pub(super) fn new(bytes: &'a mut [u8]) -> Self { Self { bytes, offset: 0 } }

    pub(super) const fn written(&self) -> usize { self.offset }
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
