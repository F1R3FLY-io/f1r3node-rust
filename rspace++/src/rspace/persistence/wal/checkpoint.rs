//! DATA-only WAL records for one self-contained fresh pack generation.

use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use super::codec::{
    ArraySink, AuthenticatedSink, GENERATION_CHECKPOINT_PREPARE_KIND_V1, LOG_PACK_VERSION_V1,
    PREPARE_DOMAIN, require_limit, validate_object_for, write_reference,
};
use super::error::LogPackFormatError;
use super::types::{LogPackDecodeLimits, LogPackSequence};
use crate::rspace::persistence::CanonicalObjectKind;
use crate::rspace::persistence::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DIGEST_LEN, OBJECT_DOMAIN_LEN,
    ObjectRefV1, ReadCursor, checked_len, checked_sum, write_u64,
};
use crate::rspace::persistence::checkpoint::{
    CheckpointReferenceVisitErrorV1, GENERATION_CHECKPOINT_CANONICAL_LEN,
    GenerationCheckpointDescriptorV1, GenerationCheckpointDescriptorViewV1,
    GenerationCheckpointErrorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::durable::{
    AdmittedCheckpointSourceV1, DurableObjectCopyErrorV1, DurableObjectReadErrorV1,
};
use crate::rspace::persistence::physical::{PhysicalDigest, STORE_ID_LEN, StoreId};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::semantic::SemanticProfile;

const CHECKPOINT_AUTH_HEADER_LEN: usize =
    OBJECT_DOMAIN_LEN + 8 + 8 + 8 + 8 + STORE_ID_LEN + GENERATION_CHECKPOINT_CANONICAL_LEN as usize;
pub(crate) const GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1: u64 =
    (CHECKPOINT_AUTH_HEADER_LEN + CHECKSUM_FIELD_LEN) as u64;
const CHECKPOINT_OBJECT_HEADER_LEN: u64 = (OBJECT_DIGEST_LEN + 8) as u64;
const CHECKPOINT_TRAILER_LEN: u64 = CHECKSUM_FIELD_LEN as u64;

#[derive(Debug)]
pub(crate) enum GenerationCheckpointSourceCopyErrorV1<R, W> {
    Source(R),
    Sink(W),
}

impl<R: Display, W: Display> Display for GenerationCheckpointSourceCopyErrorV1<R, W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "checkpoint object source failed: {error}"),
            Self::Sink(error) => write!(formatter, "checkpoint object sink failed: {error}"),
        }
    }
}

impl<R, W> std::error::Error for GenerationCheckpointSourceCopyErrorV1<R, W>
where
    R: std::error::Error + 'static,
    W: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Sink(error) => Some(error),
        }
    }
}

mod source_seal {
    pub(crate) trait Sealed {}
}

#[cfg(test)]
pub(crate) use source_seal::Sealed as GenerationCheckpointSourceTestSealV1;

/// Immutable, semantically admitted source for a complete fresh generation.
///
/// The private supertrait intentionally prevents an arbitrary internal reader
/// from claiming this contract in production. In particular,
/// `copy_checkpoint_object` must revalidate every referenced body while it is
/// copied, and all descriptor/reference visits must observe the same pinned
/// authority snapshot.
pub(crate) trait GenerationCheckpointSourceV1: source_seal::Sealed {
    type ReadError;

    fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1>;

    fn try_visit_checkpoint_references<E>(
        &self,
        visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), CheckpointReferenceVisitErrorV1<E>>;

    fn copy_checkpoint_object<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        scratch: &mut Vec<u8>,
    ) -> Result<(), GenerationCheckpointSourceCopyErrorV1<Self::ReadError, S::Error>>;
}

impl<P: SemanticProfile> source_seal::Sealed for AdmittedCheckpointSourceV1<P> {}

impl<P: SemanticProfile> GenerationCheckpointSourceV1 for AdmittedCheckpointSourceV1<P> {
    type ReadError = DurableObjectReadErrorV1;

    fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1> {
        AdmittedCheckpointSourceV1::checkpoint_descriptor(self)
    }

    fn try_visit_checkpoint_references<E>(
        &self,
        visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), CheckpointReferenceVisitErrorV1<E>> {
        AdmittedCheckpointSourceV1::try_visit_checkpoint_references(self, visitor)
    }

    fn copy_checkpoint_object<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        scratch: &mut Vec<u8>,
    ) -> Result<(), GenerationCheckpointSourceCopyErrorV1<Self::ReadError, S::Error>> {
        match AdmittedCheckpointSourceV1::copy_checkpoint_object(self, reference, sink, scratch) {
            Ok(()) => Ok(()),
            Err(DurableObjectCopyErrorV1::Object(error)) => {
                Err(GenerationCheckpointSourceCopyErrorV1::Source(error))
            }
            Err(DurableObjectCopyErrorV1::Sink(error)) => {
                Err(GenerationCheckpointSourceCopyErrorV1::Sink(error))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointPrepareIdentityV1 {
    sequence: LogPackSequence,
    store_id: StoreId,
    descriptor: GenerationCheckpointDescriptorV1,
    record_len: u64,
    digest: PhysicalDigest,
    crc32c: u32,
}

impl GenerationCheckpointPrepareIdentityV1 {
    pub(crate) const fn from_record_view(
        view: GenerationCheckpointPrepareRecordViewV1<'_>,
    ) -> Self {
        Self {
            sequence: view.preflight.sequence,
            store_id: view.preflight.store_id,
            descriptor: GenerationCheckpointDescriptorV1::from_view(view.preflight.descriptor),
            record_len: view.preflight.record_len,
            digest: view.digest,
            crc32c: view.crc32c,
        }
    }

    /// Reconstitute the same compact identity after a bounded positional
    /// scanner has authenticated the complete record without materializing it.
    pub(crate) const fn from_streamed(
        preflight: GenerationCheckpointPreparePreflightV1,
        digest: PhysicalDigest,
        crc32c: u32,
    ) -> Self {
        Self {
            sequence: preflight.sequence,
            store_id: preflight.store_id,
            descriptor: GenerationCheckpointDescriptorV1::from_view(preflight.descriptor),
            record_len: preflight.record_len,
            digest,
            crc32c,
        }
    }

    pub(crate) const fn sequence(self) -> LogPackSequence { self.sequence }

    pub(crate) const fn store_id(self) -> StoreId { self.store_id }

    pub(crate) const fn descriptor(self) -> GenerationCheckpointDescriptorV1 { self.descriptor }

    pub(crate) const fn record_len(self) -> u64 { self.record_len }

    pub(crate) const fn digest(self) -> PhysicalDigest { self.digest }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub(crate) const fn crc32c(self) -> u32 { self.crc32c }
}

pub(crate) struct PreparedGenerationCheckpointV1<'source, P, Source>
where
    P: SemanticProfile,
    Source: GenerationCheckpointSourceV1,
{
    source: &'source Source,
    descriptor: GenerationCheckpointDescriptorV1,
    store_id: StoreId,
    sequence: LogPackSequence,
    record_len: u64,
    _profile: PhantomData<fn() -> P>,
}

impl<'source, P, Source> PreparedGenerationCheckpointV1<'source, P, Source>
where
    P: SemanticProfile,
    Source: GenerationCheckpointSourceV1,
{
    pub(crate) fn new(
        source: &'source Source,
        store_id: StoreId,
        sequence: LogPackSequence,
        limits: LogPackDecodeLimits,
    ) -> Result<Self, LogPackFormatError> {
        let descriptor = source
            .checkpoint_descriptor()
            .map_err(LogPackFormatError::from)?;
        if descriptor.profile_id() != P::ID {
            return Err(CanonicalFormatError::SemanticProfileMismatch {
                expected: *P::ID.as_bytes(),
                actual: *descriptor.profile_id().as_bytes(),
            }
            .into());
        }
        let inventory = descriptor.inventory();
        require_limit("objects", limits.max_objects, inventory.data_objects())?;
        require_limit(
            "canonical object bytes",
            limits.max_object_bytes,
            inventory.data_canonical_bytes().get(),
        )?;
        let mut observed = OrderedObjectInventoryBuilderV1::new(
            inventory.data_objects(),
            inventory.data_canonical_bytes().get(),
        );
        source
            .try_visit_checkpoint_references(|reference| {
                require_limit(
                    "single canonical object bytes",
                    limits.max_single_object_bytes,
                    reference.encoded_len(),
                )?;
                observed.push(reference).map_err(LogPackFormatError::from)
            })
            .map_err(|error| match error {
                CheckpointReferenceVisitErrorV1::Checkpoint(error) => error.into(),
                CheckpointReferenceVisitErrorV1::Visitor(error) => error,
            })?;
        let observed = observed.finish().map_err(LogPackFormatError::from)?;
        if observed.digest() != inventory.ordered_digest() {
            return Err(LogPackFormatError::CheckpointInventoryDigest {
                expected: *inventory.ordered_digest().as_bytes(),
                actual: *observed.digest().as_bytes(),
            });
        }
        let object_headers = inventory
            .data_objects()
            .checked_mul(CHECKPOINT_OBJECT_HEADER_LEN)
            .ok_or(CanonicalFormatError::LengthOverflow {
                field: "checkpoint object headers",
            })?;
        let record_len = checked_sum(
            GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
            checked_sum(
                checked_sum(
                    object_headers,
                    inventory.data_canonical_bytes().get(),
                    "checkpoint object records",
                )?,
                CHECKPOINT_TRAILER_LEN,
                "checkpoint prepare record",
            )?,
            "checkpoint prepare record",
        )?;
        require_limit("record bytes", limits.max_record_bytes, record_len)?;
        Ok(Self {
            source,
            descriptor,
            store_id,
            sequence,
            record_len,
            _profile: PhantomData,
        })
    }

    pub(crate) const fn record_len(&self) -> u64 { self.record_len }

    pub(crate) const fn descriptor(&self) -> GenerationCheckpointDescriptorV1 { self.descriptor }

    pub(crate) const fn store_id(&self) -> StoreId { self.store_id }

    pub(crate) const fn sequence(&self) -> LogPackSequence { self.sequence }

    pub(crate) fn try_visit_object_layouts<E>(
        &self,
        mut visitor: impl FnMut(GenerationCheckpointObjectLayoutV1) -> Result<(), E>,
    ) -> Result<(), GenerationCheckpointLayoutVisitErrorV1<E>> {
        let inventory = self.descriptor.inventory();
        let mut observed = OrderedObjectInventoryBuilderV1::new(
            inventory.data_objects(),
            inventory.data_canonical_bytes().get(),
        );
        let mut next = GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1;
        self.source
            .try_visit_checkpoint_references(|reference| {
                observed
                    .push(reference)
                    .map_err(GenerationCheckpointLayoutVisitErrorV1::Checkpoint)?;
                let record_offset = next;
                let canonical_offset = checked_sum(
                    record_offset,
                    CHECKPOINT_OBJECT_HEADER_LEN,
                    "checkpoint object body offset",
                )
                .map_err(|error| GenerationCheckpointLayoutVisitErrorV1::Invariant(error.into()))?;
                next = checked_sum(
                    canonical_offset,
                    reference.encoded_len(),
                    "checkpoint object extent",
                )
                .map_err(|error| GenerationCheckpointLayoutVisitErrorV1::Invariant(error.into()))?;
                visitor(GenerationCheckpointObjectLayoutV1 {
                    reference,
                    record_offset,
                    canonical_offset,
                })
                .map_err(GenerationCheckpointLayoutVisitErrorV1::Visitor)
            })
            .map_err(|error| match error {
                CheckpointReferenceVisitErrorV1::Checkpoint(error) => {
                    GenerationCheckpointLayoutVisitErrorV1::Checkpoint(error)
                }
                CheckpointReferenceVisitErrorV1::Visitor(error) => error,
            })?;
        let observed = observed
            .finish()
            .map_err(GenerationCheckpointLayoutVisitErrorV1::Checkpoint)?;
        if observed.digest() != inventory.ordered_digest() {
            return Err(GenerationCheckpointLayoutVisitErrorV1::Invariant(
                LogPackFormatError::CheckpointInventoryDigest {
                    expected: *inventory.ordered_digest().as_bytes(),
                    actual: *observed.digest().as_bytes(),
                },
            ));
        }
        let actual = checked_sum(next, CHECKPOINT_TRAILER_LEN, "checkpoint prepare layout")
            .map_err(|error| GenerationCheckpointLayoutVisitErrorV1::Invariant(error.into()))?;
        if actual != self.record_len {
            return Err(GenerationCheckpointLayoutVisitErrorV1::Invariant(
                LogPackFormatError::LengthMismatch {
                    declared: self.record_len,
                    actual,
                },
            ));
        }
        Ok(())
    }

    pub(crate) fn write_to<S: CanonicalSink>(
        &self,
        sink: &mut S,
    ) -> Result<
        GenerationCheckpointPrepareIdentityV1,
        GenerationCheckpointPrepareWriteErrorV1<Source::ReadError, S::Error>,
    > {
        let mut header = [0u8; CHECKPOINT_AUTH_HEADER_LEN];
        {
            let mut header_sink = ArraySink::new(&mut header);
            header_sink
                .write_all(PREPARE_DOMAIN)
                .expect("fixed checkpoint domain");
            write_u64(&mut header_sink, LOG_PACK_VERSION_V1).expect("fixed checkpoint version");
            write_u64(&mut header_sink, GENERATION_CHECKPOINT_PREPARE_KIND_V1)
                .expect("fixed checkpoint kind");
            write_u64(&mut header_sink, self.record_len).expect("fixed checkpoint length");
            write_u64(&mut header_sink, self.sequence.get()).expect("fixed checkpoint sequence");
            header_sink
                .write_all(self.store_id.as_bytes())
                .expect("fixed checkpoint store ID");
            self.descriptor
                .write_canonical(&mut header_sink)
                .expect("validated checkpoint descriptor has a fixed canonical image");
            debug_assert_eq!(header_sink.written(), CHECKPOINT_AUTH_HEADER_LEN);
        }
        let header_crc = crc32c(&header);
        let mut record = AuthenticatedSink::new(sink);
        record
            .write_all(&header)
            .map_err(GenerationCheckpointPrepareWriteErrorV1::Sink)?;
        record
            .write_all(&encode_crc32c(header_crc))
            .map_err(GenerationCheckpointPrepareWriteErrorV1::Sink)?;

        let inventory = self.descriptor.inventory();
        let mut observed = OrderedObjectInventoryBuilderV1::new(
            inventory.data_objects(),
            inventory.data_canonical_bytes().get(),
        );
        // One fallible bounded allocation is reused for every object body in
        // the complete checkpoint. No buffer scales with the record length.
        let mut copy_scratch = Vec::new();
        self.source
            .try_visit_checkpoint_references(|reference| {
                observed
                    .push(reference)
                    .map_err(GenerationCheckpointPrepareWriteErrorV1::Checkpoint)?;
                write_reference(&mut record, reference)
                    .map_err(GenerationCheckpointPrepareWriteErrorV1::Sink)?;
                match self
                    .source
                    .copy_checkpoint_object(reference, &mut record, &mut copy_scratch)
                {
                    Ok(()) => Ok(()),
                    Err(GenerationCheckpointSourceCopyErrorV1::Source(error)) => {
                        Err(GenerationCheckpointPrepareWriteErrorV1::Source(error))
                    }
                    Err(GenerationCheckpointSourceCopyErrorV1::Sink(error)) => {
                        Err(GenerationCheckpointPrepareWriteErrorV1::Sink(error))
                    }
                }
            })
            .map_err(|error| match error {
                CheckpointReferenceVisitErrorV1::Checkpoint(error) => {
                    GenerationCheckpointPrepareWriteErrorV1::Checkpoint(error)
                }
                CheckpointReferenceVisitErrorV1::Visitor(error) => error,
            })?;
        let observed = observed
            .finish()
            .map_err(GenerationCheckpointPrepareWriteErrorV1::Checkpoint)?;
        if observed.digest() != inventory.ordered_digest() {
            return Err(GenerationCheckpointPrepareWriteErrorV1::Invariant(
                LogPackFormatError::CheckpointInventoryDigest {
                    expected: *inventory.ordered_digest().as_bytes(),
                    actual: *observed.digest().as_bytes(),
                },
            ));
        }
        let (record_crc, digest, written) = record.finish();
        let actual = written.checked_add(CHECKPOINT_TRAILER_LEN).ok_or_else(|| {
            GenerationCheckpointPrepareWriteErrorV1::Invariant(
                CanonicalFormatError::LengthOverflow {
                    field: "emitted checkpoint prepare record",
                }
                .into(),
            )
        })?;
        if actual != self.record_len {
            return Err(GenerationCheckpointPrepareWriteErrorV1::Invariant(
                LogPackFormatError::LengthMismatch {
                    declared: self.record_len,
                    actual,
                },
            ));
        }
        sink.write_all(&encode_crc32c(record_crc))
            .map_err(GenerationCheckpointPrepareWriteErrorV1::Sink)?;
        Ok(GenerationCheckpointPrepareIdentityV1 {
            sequence: self.sequence,
            store_id: self.store_id,
            descriptor: self.descriptor,
            record_len: self.record_len,
            digest,
            crc32c: record_crc,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointObjectLayoutV1 {
    reference: ObjectRefV1,
    record_offset: u64,
    canonical_offset: u64,
}

impl GenerationCheckpointObjectLayoutV1 {
    pub(crate) const fn reference(self) -> ObjectRefV1 { self.reference }

    pub(crate) const fn record_offset(self) -> u64 { self.record_offset }

    pub(crate) const fn canonical_offset(self) -> u64 { self.canonical_offset }
}

#[derive(Debug)]
pub(crate) enum GenerationCheckpointLayoutVisitErrorV1<E> {
    Checkpoint(GenerationCheckpointErrorV1),
    Visitor(E),
    Invariant(LogPackFormatError),
}

impl<E: Display> Display for GenerationCheckpointLayoutVisitErrorV1<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checkpoint(error) => Display::fmt(error, formatter),
            Self::Visitor(error) => write!(formatter, "checkpoint layout visitor failed: {error}"),
            Self::Invariant(error) => write!(formatter, "checkpoint layout invariant: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error
    for GenerationCheckpointLayoutVisitErrorV1<E>
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Checkpoint(error) => Some(error),
            Self::Visitor(error) => Some(error),
            Self::Invariant(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub(crate) enum GenerationCheckpointPrepareWriteErrorV1<R, W> {
    Source(R),
    Sink(W),
    Checkpoint(GenerationCheckpointErrorV1),
    Invariant(LogPackFormatError),
}

impl<R: Display, W: Display> Display for GenerationCheckpointPrepareWriteErrorV1<R, W> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "checkpoint object source failed: {error}"),
            Self::Sink(error) => write!(formatter, "checkpoint sink failed: {error}"),
            Self::Checkpoint(error) => Display::fmt(error, formatter),
            Self::Invariant(error) => write!(formatter, "checkpoint writer invariant: {error}"),
        }
    }
}

impl<R, W> std::error::Error for GenerationCheckpointPrepareWriteErrorV1<R, W>
where
    R: std::error::Error + 'static,
    W: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Sink(error) => Some(error),
            Self::Checkpoint(error) => Some(error),
            Self::Invariant(error) => Some(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointPreparePreflightV1 {
    record_len: u64,
    sequence: LogPackSequence,
    store_id: StoreId,
    descriptor: GenerationCheckpointDescriptorViewV1,
}

impl GenerationCheckpointPreparePreflightV1 {
    pub(crate) const fn record_len(self) -> u64 { self.record_len }

    pub(crate) const fn sequence(self) -> LogPackSequence { self.sequence }

    pub(crate) const fn store_id(self) -> StoreId { self.store_id }

    pub(crate) const fn descriptor(self) -> GenerationCheckpointDescriptorViewV1 { self.descriptor }
}

pub(crate) fn preflight_generation_checkpoint_prepare_v1(
    prefix: &[u8],
    limits: LogPackDecodeLimits,
) -> Result<GenerationCheckpointPreparePreflightV1, LogPackFormatError> {
    preflight_generation_checkpoint_prepare_for(prefix, limits, RetentionReader::V1Only)
}

/// Preflight a checkpoint prepare header for a reader with the given
/// retention capability. Only the embedded descriptor depends on the reader.
pub(crate) fn preflight_generation_checkpoint_prepare_for(
    prefix: &[u8],
    limits: LogPackDecodeLimits,
    reader: RetentionReader,
) -> Result<GenerationCheckpointPreparePreflightV1, LogPackFormatError> {
    let actual = checked_len(prefix.len(), "checkpoint prepare header")?;
    if actual != GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 {
        return Err(LogPackFormatError::LengthMismatch {
            declared: GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
            actual,
        });
    }
    let authenticated = &prefix[..CHECKPOINT_AUTH_HEADER_LEN];
    let declared_crc = decode_crc32c(
        prefix[CHECKPOINT_AUTH_HEADER_LEN..]
            .try_into()
            .expect("fixed checkpoint header CRC extent"),
    )
    .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
        field: "checkpoint prepare header",
        encoded,
    })?;
    let actual_crc = crc32c(authenticated);
    if declared_crc != actual_crc {
        return Err(LogPackFormatError::HeaderChecksum {
            expected: declared_crc,
            actual: actual_crc,
        });
    }
    let mut cursor = ReadCursor::new(authenticated);
    if cursor.read_exact(OBJECT_DOMAIN_LEN)? != PREPARE_DOMAIN {
        return Err(LogPackFormatError::InvalidDomain);
    }
    let version = cursor.read_u64()?;
    if version != LOG_PACK_VERSION_V1 {
        return Err(LogPackFormatError::UnsupportedVersion(version));
    }
    let kind = cursor.read_u64()?;
    if kind != GENERATION_CHECKPOINT_PREPARE_KIND_V1 {
        return Err(LogPackFormatError::UnexpectedRecordKind {
            expected: GENERATION_CHECKPOINT_PREPARE_KIND_V1,
            actual: kind,
        });
    }
    let record_len = cursor.read_u64()?;
    let sequence = LogPackSequence::new(cursor.read_u64()?)?;
    let store_id = StoreId::from_array(
        cursor
            .read_exact(STORE_ID_LEN)?
            .try_into()
            .expect("fixed checkpoint store ID extent"),
    );
    let descriptor_bytes = cursor.read_exact(GENERATION_CHECKPOINT_CANONICAL_LEN as usize)?;
    cursor.finish()?;
    let descriptor = GenerationCheckpointDescriptorViewV1::parse_for(descriptor_bytes, reader)
        .map_err(LogPackFormatError::from)?;
    require_limit("objects", limits.max_objects, descriptor.data_objects())?;
    require_limit(
        "canonical object bytes",
        limits.max_object_bytes,
        descriptor.data_canonical_bytes().get(),
    )?;
    let expected = expected_record_len(descriptor)?;
    if record_len != expected {
        return Err(LogPackFormatError::LengthMismatch {
            declared: record_len,
            actual: expected,
        });
    }
    require_limit("record bytes", limits.max_record_bytes, record_len)?;
    Ok(GenerationCheckpointPreparePreflightV1 {
        record_len,
        sequence,
        store_id,
        descriptor,
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GenerationCheckpointObjectViewV1<'a> {
    reference: ObjectRefV1,
    canonical_bytes: &'a [u8],
    record_offset: u64,
    canonical_offset: u64,
}

impl<'a> GenerationCheckpointObjectViewV1<'a> {
    pub(crate) const fn reference(self) -> ObjectRefV1 { self.reference }

    pub(crate) const fn canonical_bytes(self) -> &'a [u8] { self.canonical_bytes }

    pub(crate) const fn record_offset(self) -> u64 { self.record_offset }

    pub(crate) const fn canonical_offset(self) -> u64 { self.canonical_offset }
}

pub(crate) struct GenerationCheckpointObjectIterV1<'a> {
    cursor: ReadCursor<'a>,
    remaining: u64,
    base_record_offset: u64,
}

impl<'a> Iterator for GenerationCheckpointObjectIterV1<'a> {
    type Item = GenerationCheckpointObjectViewV1<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let record_offset = self.base_record_offset + self.cursor.offset() as u64;
        let reference = ObjectRefV1::parse(&mut self.cursor)
            .expect("checkpoint view validated each object reference");
        let canonical_offset = self.base_record_offset + self.cursor.offset() as u64;
        let canonical_bytes = self
            .cursor
            .read_exact(reference.encoded_len() as usize)
            .expect("checkpoint view validated each object extent");
        Some(GenerationCheckpointObjectViewV1 {
            reference,
            canonical_bytes,
            record_offset,
            canonical_offset,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for GenerationCheckpointObjectIterV1<'_> {}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GenerationCheckpointPrepareRecordViewV1<'a> {
    preflight: GenerationCheckpointPreparePreflightV1,
    objects: &'a [u8],
    digest: PhysicalDigest,
    crc32c: u32,
}

impl<'a> GenerationCheckpointPrepareRecordViewV1<'a> {
    pub(crate) fn parse(
        bytes: &'a [u8],
        limits: LogPackDecodeLimits,
    ) -> Result<Self, LogPackFormatError> {
        Self::parse_for(bytes, limits, RetentionReader::V1Only)
    }

    pub(crate) fn parse_for(
        bytes: &'a [u8],
        limits: LogPackDecodeLimits,
        reader: RetentionReader,
    ) -> Result<Self, LogPackFormatError> {
        if bytes.len() <
            (GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 + CHECKPOINT_TRAILER_LEN) as usize
        {
            return Err(CanonicalFormatError::Truncated {
                offset: 0,
                needed: (GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 + CHECKPOINT_TRAILER_LEN)
                    as usize,
                available: bytes.len(),
            }
            .into());
        }
        let preflight = preflight_generation_checkpoint_prepare_for(
            &bytes[..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize],
            limits,
            reader,
        )?;
        let actual = checked_len(bytes.len(), "checkpoint prepare record")?;
        if actual != preflight.record_len() {
            return Err(LogPackFormatError::LengthMismatch {
                declared: preflight.record_len(),
                actual,
            });
        }
        let payload_end = bytes.len() - CHECKPOINT_TRAILER_LEN as usize;
        let declared_crc = decode_crc32c(
            bytes[payload_end..]
                .try_into()
                .expect("fixed checkpoint record CRC extent"),
        )
        .map_err(|encoded| LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint prepare record",
            encoded,
        })?;
        let actual_crc = crc32c(&bytes[..payload_end]);
        if declared_crc != actual_crc {
            return Err(LogPackFormatError::RecordChecksum {
                expected: declared_crc,
                actual: actual_crc,
            });
        }
        let descriptor = preflight.descriptor();
        let objects = &bytes[GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize..payload_end];
        let mut cursor = ReadCursor::new(objects);
        let mut inventory = OrderedObjectInventoryBuilderV1::new(
            descriptor.data_objects(),
            descriptor.data_canonical_bytes().get(),
        );
        let state_manifest = descriptor.state().manifest();
        let retention_manifest = descriptor.retention_manifest();
        let (retention_role, retention_kind) = designated_retention_object(descriptor.format());
        let mut saw_state_manifest = false;
        let mut saw_retention_manifest = false;
        for _ in 0..descriptor.data_objects() {
            let reference = ObjectRefV1::parse(&mut cursor)?;
            require_limit(
                "single canonical object bytes",
                limits.max_single_object_bytes,
                reference.encoded_len(),
            )?;
            inventory
                .push(reference)
                .map_err(LogPackFormatError::from)?;
            let length = usize::try_from(reference.encoded_len()).map_err(|_| {
                CanonicalFormatError::LengthOutOfRange {
                    field: "checkpoint canonical object",
                    value: reference.encoded_len(),
                }
            })?;
            let kind = validate_object_for(reference, cursor.read_exact(length)?, reader)?;
            if reference == state_manifest {
                require_designated_kind(
                    "state manifest",
                    reference,
                    kind,
                    CanonicalObjectKind::SemanticManifest,
                )?;
                saw_state_manifest = true;
            }
            if reference == retention_manifest {
                require_designated_kind(retention_role, reference, kind, retention_kind)?;
                saw_retention_manifest = true;
            }
        }
        cursor.finish()?;
        if !saw_state_manifest {
            return Err(LogPackFormatError::CheckpointDesignatedObjectMissing {
                role: "state manifest",
                reference: state_manifest,
            });
        }
        if !saw_retention_manifest {
            return Err(LogPackFormatError::CheckpointDesignatedObjectMissing {
                role: retention_role,
                reference: retention_manifest,
            });
        }
        let inventory = inventory.finish().map_err(LogPackFormatError::from)?;
        if inventory.digest() != descriptor.ordered_digest() {
            return Err(LogPackFormatError::CheckpointInventoryDigest {
                expected: *descriptor.ordered_digest().as_bytes(),
                actual: *inventory.digest().as_bytes(),
            });
        }
        Ok(Self {
            preflight,
            objects,
            digest: PhysicalDigest::of_bytes(&bytes[..payload_end]),
            crc32c: declared_crc,
        })
    }

    pub(crate) const fn preflight(self) -> GenerationCheckpointPreparePreflightV1 { self.preflight }

    pub(crate) fn objects(self) -> GenerationCheckpointObjectIterV1<'a> {
        GenerationCheckpointObjectIterV1 {
            cursor: ReadCursor::new(self.objects),
            remaining: self.preflight.descriptor().data_objects(),
            base_record_offset: GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
        }
    }

    pub(crate) const fn digest(self) -> PhysicalDigest { self.digest }

    pub(crate) const fn crc32c(self) -> u32 { self.crc32c }
}

/// The role name and object kind a checkpoint appoints for its retention
/// metadata: the V1 manifest, or the V2 anchor.
pub(crate) const fn designated_retention_object(
    format: RetentionFormat,
) -> (&'static str, CanonicalObjectKind) {
    match format {
        RetentionFormat::V1 => ("retention manifest", CanonicalObjectKind::RetentionManifest),
        RetentionFormat::V2 => ("retention anchor", CanonicalObjectKind::RetentionAnchor),
    }
}

fn require_designated_kind(
    role: &'static str,
    reference: ObjectRefV1,
    actual: CanonicalObjectKind,
    expected: CanonicalObjectKind,
) -> Result<(), LogPackFormatError> {
    if actual != expected {
        return Err(LogPackFormatError::CheckpointDesignatedObjectKind {
            role,
            reference,
            expected,
            actual,
        });
    }
    Ok(())
}

fn expected_record_len(
    descriptor: GenerationCheckpointDescriptorViewV1,
) -> Result<u64, LogPackFormatError> {
    let headers = descriptor
        .data_objects()
        .checked_mul(CHECKPOINT_OBJECT_HEADER_LEN)
        .ok_or(CanonicalFormatError::LengthOverflow {
            field: "checkpoint object headers",
        })?;
    Ok(checked_sum(
        GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
        checked_sum(
            checked_sum(
                headers,
                descriptor.data_canonical_bytes().get(),
                "checkpoint object records",
            )?,
            CHECKPOINT_TRAILER_LEN,
            "checkpoint prepare record",
        )?,
        "checkpoint prepare record",
    )?)
}

const _: () = assert!(CHECKPOINT_AUTH_HEADER_LEN == 280);
const _: () = assert!(GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 == 288);
