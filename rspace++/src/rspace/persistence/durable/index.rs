//! Rebuildable canonical-object index format.
//!
//! The index is a physical accelerator, never semantic authority. A missing or
//! corrupt generation can be reconstructed deterministically from committed
//! DATA records in live packs.

use std::fmt::{Display, Formatter};

use pathmap::PathMap;

use super::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1};
use crate::rspace::persistence::canonical::{
    CanonicalFormatError, ObjectDigest, ObjectRefV1, ReadCursor,
};
use crate::rspace::persistence::checkpoint::GenerationCheckpointErrorV1;
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::physical::{STORE_ID_LEN, StoreId};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::wal::{
    CommitPublicationV1, GenerationCheckpointLayoutVisitErrorV1,
    GenerationCheckpointObjectLayoutV1, GenerationCheckpointSourceV1, LogPackAddress,
    LogPackFormatError, LogPackObjectDispositionV1, LogPackObjectView, LogPackSequence, PackId,
    PreparedGenerationCheckpointV1, PreparedLogPackTransactionV1, PreparedObjectLayoutV1,
};

#[cfg(test)]
thread_local! {
    static COMMITTED_INDEX_CONSTRUCTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

const INDEX_DOMAIN: &[u8; 16] = b"RSpaceIndex.V1\0\0";
const INDEX_VERSION: u64 = 1;
const INDEX_HEADER_AUTH_LEN: usize = INDEX_HEADER_LEN_V1 as usize - CHECKSUM_FIELD_LEN;
const INDEX_RECORD_AUTH_LEN: usize = INDEX_RECORD_LEN_V1 as usize - CHECKSUM_FIELD_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexGenerationV1(u64);

impl IndexGenerationV1 {
    pub const FIRST: Self = Self(1);

    pub fn new(value: u64) -> Result<Self, IndexFormatError> {
        if value == 0 {
            return Err(IndexFormatError::ZeroGeneration);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 { self.0 }

    pub fn successor(self) -> Result<Self, IndexFormatError> {
        let value = self
            .0
            .checked_add(1)
            .ok_or(IndexFormatError::GenerationOverflow(self.0))?;
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIndexHeaderV1 {
    store_id: StoreId,
    generation: IndexGenerationV1,
}

impl ObjectIndexHeaderV1 {
    pub const fn new(store_id: StoreId, generation: IndexGenerationV1) -> Self {
        Self {
            store_id,
            generation,
        }
    }

    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn generation(self) -> IndexGenerationV1 { self.generation }

    pub fn encode(self) -> [u8; INDEX_HEADER_LEN_V1 as usize] {
        let mut bytes = [0u8; INDEX_HEADER_LEN_V1 as usize];
        bytes[..16].copy_from_slice(INDEX_DOMAIN);
        put_u64(&mut bytes, 16, INDEX_VERSION);
        put_u64(&mut bytes, 24, INDEX_HEADER_LEN_V1);
        bytes[32..48].copy_from_slice(self.store_id.as_bytes());
        put_u64(&mut bytes, 48, self.generation.get());
        let checksum = crc32c(&bytes[..INDEX_HEADER_AUTH_LEN]);
        bytes[INDEX_HEADER_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub fn parse_exact(bytes: &[u8]) -> Result<Self, IndexFormatError> {
        require_exact_len(bytes, INDEX_HEADER_LEN_V1, "index header")?;
        let expected = decode_crc32c(
            bytes[INDEX_HEADER_AUTH_LEN..]
                .try_into()
                .expect("fixed index-header checksum extent"),
        )
        .map_err(|encoded| IndexFormatError::ChecksumOutOfRange {
            field: "index header",
            encoded,
        })?;
        let actual = crc32c(&bytes[..INDEX_HEADER_AUTH_LEN]);
        if expected != actual {
            return Err(IndexFormatError::Checksum {
                field: "index header",
                expected,
                actual,
            });
        }
        if &bytes[..16] != INDEX_DOMAIN {
            return Err(IndexFormatError::InvalidDomain);
        }
        let version = get_u64(bytes, 16);
        if version != INDEX_VERSION {
            return Err(IndexFormatError::UnsupportedVersion(version));
        }
        let declared_len = get_u64(bytes, 24);
        if declared_len != INDEX_HEADER_LEN_V1 {
            return Err(IndexFormatError::DeclaredHeaderLength(declared_len));
        }
        let store_id = StoreId::from_array(
            bytes[32..48]
                .try_into()
                .expect("fixed index store ID extent"),
        );
        let generation = IndexGenerationV1::new(get_u64(bytes, 48))?;
        Ok(Self {
            store_id,
            generation,
        })
    }

    pub fn verify_identity(
        self,
        expected_store: StoreId,
        expected_generation: IndexGenerationV1,
    ) -> Result<(), IndexFormatError> {
        if self.store_id != expected_store {
            return Err(IndexFormatError::Store {
                expected: expected_store,
                actual: self.store_id,
            });
        }
        if self.generation != expected_generation {
            return Err(IndexFormatError::Generation {
                expected: expected_generation,
                actual: self.generation,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectLocationV1 {
    pack_id: PackId,
    canonical_offset: u64,
    sequence: LogPackSequence,
}

impl ObjectLocationV1 {
    pub fn new(
        pack_id: PackId,
        canonical_offset: u64,
        sequence: LogPackSequence,
    ) -> Result<Self, IndexFormatError> {
        if canonical_offset < PACK_HEADER_LEN_V1 {
            return Err(IndexFormatError::CanonicalOffsetBeforePackHeader(canonical_offset));
        }
        Ok(Self {
            pack_id,
            canonical_offset,
            sequence,
        })
    }

    pub const fn pack_id(self) -> PackId { self.pack_id }

    pub const fn canonical_offset(self) -> u64 { self.canonical_offset }

    pub const fn sequence(self) -> LogPackSequence { self.sequence }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIndexRecordV1 {
    reference: ObjectRefV1,
    location: ObjectLocationV1,
}

/// Copy-on-write, prefix-compressed lookup image reconstructed from the
/// catalog-appointed index prefix.
///
/// Digests are already fixed 32-byte paths, so this structure introduces no
/// symbol-table or platform-sized surrogate IDs. Values remain small `Copy`
/// records; wrapping them in `Arc` would only add allocation and refcount
/// traffic. The map is an accelerator and is never semantic authority.
#[derive(Clone)]
pub(crate) struct CommittedObjectIndexV1 {
    records: PathMap<ObjectIndexRecordV1>,
    record_count: u64,
}

impl CommittedObjectIndexV1 {
    pub(crate) fn new() -> Self {
        #[cfg(test)]
        COMMITTED_INDEX_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));
        Self {
            records: PathMap::new(),
            record_count: 0,
        }
    }

    pub(crate) const fn record_count(&self) -> u64 { self.record_count }

    pub(crate) fn get(&self, digest: ObjectDigest) -> Option<ObjectIndexRecordV1> {
        self.records.get_val_at(digest.as_bytes()).copied()
    }

    /// Admit one committed DATA record. A second DATA record for the same
    /// digest is non-canonical even when its bytes are equal; subsequent
    /// transactions must encode REUSE instead. Rejecting it gives recovery a
    /// unique, byte-identical index rebuild order.
    pub(crate) fn validate_insert(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), IndexFormatError> {
        let digest = record.reference().digest();
        if let Some(existing) = self.get(digest) {
            if existing.reference() == record.reference() {
                return Err(IndexFormatError::DuplicateData(record.reference()));
            }
            return Err(IndexFormatError::DigestReferenceConflict {
                digest,
                existing: existing.reference(),
                candidate: record.reference(),
            });
        }
        self.record_count
            .checked_add(1)
            .ok_or(IndexFormatError::RecordCountOverflow)?;
        Ok(())
    }

    pub(crate) fn insert(&mut self, record: ObjectIndexRecordV1) -> Result<(), IndexFormatError> {
        self.validate_insert(record)?;
        let digest = record.reference().digest();
        self.record_count = self
            .record_count
            .checked_add(1)
            .expect("validated index record count has a successor");
        let replaced = self.records.insert(digest.as_bytes(), record);
        debug_assert!(replaced.is_none());
        Ok(())
    }
}

impl Default for CommittedObjectIndexV1 {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
pub(crate) fn reset_committed_index_constructions() {
    COMMITTED_INDEX_CONSTRUCTIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn committed_index_constructions() -> u64 {
    COMMITTED_INDEX_CONSTRUCTIONS.with(std::cell::Cell::get)
}

impl ObjectIndexRecordV1 {
    pub fn new(
        reference: ObjectRefV1,
        location: ObjectLocationV1,
    ) -> Result<Self, IndexFormatError> {
        location
            .canonical_offset
            .checked_add(reference.encoded_len())
            .ok_or(IndexFormatError::ObjectEndOverflow {
                offset: location.canonical_offset,
                length: reference.encoded_len(),
            })?;
        Ok(Self {
            reference,
            location,
        })
    }

    pub(crate) fn from_inline(
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
        layout: PreparedObjectLayoutV1,
    ) -> Result<Self, IndexFormatError> {
        if layout.disposition() != LogPackObjectDispositionV1::Data {
            return Err(IndexFormatError::ReuseHasNoIndexLocation(layout.reference()));
        }
        let relative = layout
            .canonical_offset()
            .ok_or(IndexFormatError::InlineObjectMissingOffset(layout.reference()))?;
        Self::from_relative(layout.reference(), relative, pack_id, prepare_offset, sequence)
    }

    pub(crate) fn from_checkpoint_layout(
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
        layout: GenerationCheckpointObjectLayoutV1,
    ) -> Result<Self, IndexFormatError> {
        Self::from_relative(
            layout.reference(),
            layout.canonical_offset(),
            pack_id,
            prepare_offset,
            sequence,
        )
    }

    pub(crate) fn from_data_view(
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
        object: LogPackObjectView<'_>,
    ) -> Result<Self, IndexFormatError> {
        if object.disposition() != LogPackObjectDispositionV1::Data {
            return Err(IndexFormatError::ReuseHasNoIndexLocation(object.reference()));
        }
        let relative = object
            .canonical_offset()
            .ok_or(IndexFormatError::InlineObjectMissingOffset(object.reference()))?;
        Self::from_relative(object.reference(), relative, pack_id, prepare_offset, sequence)
    }

    pub(crate) fn from_streamed_data(
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
        reference: ObjectRefV1,
        relative_canonical_offset: u64,
    ) -> Result<Self, IndexFormatError> {
        Self::from_relative(reference, relative_canonical_offset, pack_id, prepare_offset, sequence)
    }

    pub(crate) fn from_checkpoint_reference(
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
        reference: ObjectRefV1,
        relative_canonical_offset: u64,
    ) -> Result<Self, IndexFormatError> {
        Self::from_relative(reference, relative_canonical_offset, pack_id, prepare_offset, sequence)
    }

    fn from_relative(
        reference: ObjectRefV1,
        relative: u64,
        pack_id: PackId,
        prepare_offset: u64,
        sequence: LogPackSequence,
    ) -> Result<Self, IndexFormatError> {
        let canonical_offset = prepare_offset.checked_add(relative).ok_or(
            IndexFormatError::AbsoluteOffsetOverflow {
                prepare_offset,
                relative_offset: relative,
            },
        )?;
        Self::new(reference, ObjectLocationV1::new(pack_id, canonical_offset, sequence)?)
    }

    pub const fn reference(self) -> ObjectRefV1 { self.reference }

    pub const fn location(self) -> ObjectLocationV1 { self.location }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this bound check yet"
    )]
    pub fn checked_object_end(self) -> Result<u64, IndexFormatError> {
        self.location
            .canonical_offset
            .checked_add(self.reference.encoded_len())
            .ok_or(IndexFormatError::ObjectEndOverflow {
                offset: self.location.canonical_offset,
                length: self.reference.encoded_len(),
            })
    }

    pub fn encode(self) -> [u8; INDEX_RECORD_LEN_V1 as usize] {
        let mut bytes = [0u8; INDEX_RECORD_LEN_V1 as usize];
        bytes[..32].copy_from_slice(self.reference.digest().as_bytes());
        put_u64(&mut bytes, 32, self.reference.encoded_len());
        put_u64(&mut bytes, 40, self.location.pack_id.get());
        put_u64(&mut bytes, 48, self.location.canonical_offset);
        put_u64(&mut bytes, 56, self.location.sequence.get());
        let checksum = crc32c(&bytes[..INDEX_RECORD_AUTH_LEN]);
        bytes[INDEX_RECORD_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub fn parse_exact(bytes: &[u8]) -> Result<Self, IndexFormatError> {
        require_exact_len(bytes, INDEX_RECORD_LEN_V1, "index record")?;
        let expected = decode_crc32c(
            bytes[INDEX_RECORD_AUTH_LEN..]
                .try_into()
                .expect("fixed index-record checksum extent"),
        )
        .map_err(|encoded| IndexFormatError::ChecksumOutOfRange {
            field: "index record",
            encoded,
        })?;
        let actual = crc32c(&bytes[..INDEX_RECORD_AUTH_LEN]);
        if expected != actual {
            return Err(IndexFormatError::Checksum {
                field: "index record",
                expected,
                actual,
            });
        }
        let mut reference_cursor = ReadCursor::new(&bytes[..40]);
        let reference = ObjectRefV1::parse(&mut reference_cursor)?;
        reference_cursor.finish()?;
        let pack_id = PackId::new(get_u64(bytes, 40))?;
        let canonical_offset = get_u64(bytes, 48);
        let sequence = LogPackSequence::new(get_u64(bytes, 56))?;
        Self::new(reference, ObjectLocationV1::new(pack_id, canonical_offset, sequence)?)
    }
}

/// Borrowed authoritative prefix of one rebuildable index file.
///
/// Bytes after `durable_tail` are deliberately ignored; only the catalog can
/// appoint an index frontier. Record checksums remain fallible and are
/// validated as the iterator advances, so callers can rebuild instead of
/// treating accelerator damage as semantic-state loss.
#[derive(Clone, Copy, Debug)]
pub struct ObjectIndexFileViewV1<'a> {
    header: ObjectIndexHeaderV1,
    records: &'a [u8],
    durable_tail: u64,
    ignored_tail_bytes: u64,
}

impl<'a> ObjectIndexFileViewV1<'a> {
    pub fn parse_exact(bytes: &'a [u8]) -> Result<Self, IndexFormatError> {
        let tail = u64::try_from(bytes.len()).map_err(|_| IndexFormatError::LengthOutOfRange)?;
        Self::parse_durable_prefix(bytes, tail)
    }

    pub fn parse_durable_prefix(
        bytes: &'a [u8],
        durable_tail: u64,
    ) -> Result<Self, IndexFormatError> {
        let available =
            u64::try_from(bytes.len()).map_err(|_| IndexFormatError::LengthOutOfRange)?;
        if durable_tail > available {
            return Err(IndexFormatError::DurableTailBeyondFile {
                durable_tail,
                available,
            });
        }
        if durable_tail < INDEX_HEADER_LEN_V1 ||
            !(durable_tail - INDEX_HEADER_LEN_V1).is_multiple_of(INDEX_RECORD_LEN_V1)
        {
            return Err(IndexFormatError::UnalignedDurableTail(durable_tail));
        }
        let durable_tail_usize =
            usize::try_from(durable_tail).map_err(|_| IndexFormatError::LengthOutOfRange)?;
        let header = ObjectIndexHeaderV1::parse_exact(&bytes[..INDEX_HEADER_LEN_V1 as usize])?;
        Ok(Self {
            header,
            records: &bytes[INDEX_HEADER_LEN_V1 as usize..durable_tail_usize],
            durable_tail,
            ignored_tail_bytes: available - durable_tail,
        })
    }

    pub const fn header(self) -> ObjectIndexHeaderV1 { self.header }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this tail accessor yet"
    )]
    pub const fn durable_tail(self) -> u64 { self.durable_tail }

    pub const fn ignored_tail_bytes(self) -> u64 { self.ignored_tail_bytes }

    pub fn record_count(self) -> u64 {
        u64::try_from(self.records.len()).expect("durable index prefix fits u64") /
            INDEX_RECORD_LEN_V1
    }

    pub fn records(self) -> ObjectIndexRecordIter<'a> {
        ObjectIndexRecordIter {
            records: self.records.chunks_exact(INDEX_RECORD_LEN_V1 as usize),
        }
    }
}

pub struct ObjectIndexRecordIter<'a> {
    records: std::slice::ChunksExact<'a, u8>,
}

impl Iterator for ObjectIndexRecordIter<'_> {
    type Item = Result<ObjectIndexRecordV1, IndexFormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.records.next().map(ObjectIndexRecordV1::parse_exact)
    }

    fn size_hint(&self) -> (usize, Option<usize>) { self.records.size_hint() }
}

impl ExactSizeIterator for ObjectIndexRecordIter<'_> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalIndexRecordImageV1 {
    offset: u64,
    bytes: [u8; INDEX_RECORD_LEN_V1 as usize],
}

impl CanonicalIndexRecordImageV1 {
    pub(crate) const fn offset(self) -> u64 { self.offset }

    pub(crate) const fn bytes(&self) -> &[u8; INDEX_RECORD_LEN_V1 as usize] { &self.bytes }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalIndexStreamV1 {
    header: ObjectIndexHeaderV1,
    next_offset: u64,
    record_count: u64,
}

impl CanonicalIndexStreamV1 {
    pub(crate) const fn new(header: ObjectIndexHeaderV1) -> Self {
        Self {
            header,
            next_offset: INDEX_HEADER_LEN_V1,
            record_count: 0,
        }
    }

    pub(crate) fn header_image(self) -> [u8; INDEX_HEADER_LEN_V1 as usize] { self.header.encode() }

    pub(crate) const fn durable_tail(self) -> u64 { self.next_offset }

    pub(crate) fn next_record(
        &mut self,
        record: ObjectIndexRecordV1,
    ) -> Result<CanonicalIndexRecordImageV1, IndexFormatError> {
        let offset = self.next_offset;
        let next_offset = self.next_offset.checked_add(INDEX_RECORD_LEN_V1).ok_or(
            IndexFormatError::CanonicalStreamTailOverflow {
                offset: self.next_offset,
            },
        )?;
        let record_count = self
            .record_count
            .checked_add(1)
            .ok_or(IndexFormatError::RecordCountOverflow)?;
        self.next_offset = next_offset;
        self.record_count = record_count;
        Ok(CanonicalIndexRecordImageV1 {
            offset,
            bytes: record.encode(),
        })
    }

    pub(crate) fn verify_appointed_tail(self, appointed_tail: u64) -> Result<(), IndexFormatError> {
        if self.next_offset != appointed_tail {
            return Err(IndexFormatError::CanonicalStreamTail {
                expected: self.next_offset,
                appointed: appointed_tail,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjectIndexPrefixVerifierV1 {
    stream: CanonicalIndexStreamV1,
    appointed_tail: u64,
    header_verified: bool,
}

impl ObjectIndexPrefixVerifierV1 {
    pub(crate) fn new(
        header: ObjectIndexHeaderV1,
        appointed_tail: u64,
        available: u64,
    ) -> Result<Self, IndexFormatError> {
        if appointed_tail < INDEX_HEADER_LEN_V1 ||
            !(appointed_tail - INDEX_HEADER_LEN_V1).is_multiple_of(INDEX_RECORD_LEN_V1)
        {
            return Err(IndexFormatError::UnalignedDurableTail(appointed_tail));
        }
        if appointed_tail > available {
            return Err(IndexFormatError::DurableTailBeyondFile {
                durable_tail: appointed_tail,
                available,
            });
        }
        Ok(Self {
            stream: CanonicalIndexStreamV1::new(header),
            appointed_tail,
            header_verified: false,
        })
    }

    pub(crate) fn verify_header(&mut self, bytes: &[u8]) -> Result<(), IndexFormatError> {
        let actual = ObjectIndexHeaderV1::parse_exact(bytes)?;
        actual.verify_identity(self.stream.header.store_id(), self.stream.header.generation())?;
        self.header_verified = true;
        Ok(())
    }

    pub(crate) const fn next_record_offset(&self) -> u64 { self.stream.durable_tail() }

    pub(crate) fn verify_next_record(
        &mut self,
        expected: ObjectIndexRecordV1,
        bytes: &[u8],
    ) -> Result<(), IndexFormatError> {
        let actual = ObjectIndexRecordV1::parse_exact(bytes)?;
        if actual != expected {
            return Err(IndexFormatError::RecordMismatch {
                offset: self.stream.durable_tail(),
                expected,
                actual,
            });
        }
        self.accept_next_record(actual)
    }

    pub(crate) fn accept_next_record(
        &mut self,
        record: ObjectIndexRecordV1,
    ) -> Result<(), IndexFormatError> {
        if !self.header_verified {
            return Err(IndexFormatError::IndexHeaderNotVerified);
        }
        let image = self.stream.next_record(record)?;
        if self.stream.durable_tail() > self.appointed_tail {
            return Err(IndexFormatError::DerivedRecordBeyondAppointedTail {
                record_offset: image.offset(),
                appointed_tail: self.appointed_tail,
            });
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), IndexFormatError> {
        if !self.header_verified {
            return Err(IndexFormatError::IndexHeaderNotVerified);
        }
        self.stream.verify_appointed_tail(self.appointed_tail)
    }
}

/// Preflighted append of every new DATA body in one prepared transaction.
///
/// The append plan borrows the WAL plan and derives locations from its checked
/// layout iterator. It therefore cannot drift from the bytes emitted to the
/// pack, does not reparse them, and never emits an index record for REUSE.
pub(crate) struct PreparedIndexAppendV1<'prepared, 'transaction, 'mutation, P: SemanticProfile> {
    prepared: &'prepared PreparedLogPackTransactionV1<'transaction, 'mutation, P>,
    pack_id: PackId,
    prepare_offset: u64,
    old_tail: u64,
    new_tail: u64,
}

impl<'prepared, 'transaction, 'mutation, P: SemanticProfile>
    PreparedIndexAppendV1<'prepared, 'transaction, 'mutation, P>
{
    pub(crate) fn new(
        header: ObjectIndexHeaderV1,
        old_tail: u64,
        pack_id: PackId,
        prepare_offset: u64,
        prepared: &'prepared PreparedLogPackTransactionV1<'transaction, 'mutation, P>,
    ) -> Result<Self, IndexFormatError> {
        if header.store_id() != prepared.store_id() {
            return Err(IndexFormatError::PrepareStore {
                expected: header.store_id(),
                actual: prepared.store_id(),
            });
        }
        if old_tail < INDEX_HEADER_LEN_V1 ||
            !(old_tail - INDEX_HEADER_LEN_V1).is_multiple_of(INDEX_RECORD_LEN_V1)
        {
            return Err(IndexFormatError::UnalignedDurableTail(old_tail));
        }
        if prepare_offset < PACK_HEADER_LEN_V1 {
            return Err(IndexFormatError::PrepareOffsetBeforePackHeader(prepare_offset));
        }
        prepare_offset.checked_add(prepared.record_len()).ok_or(
            IndexFormatError::PrepareEndOverflow {
                offset: prepare_offset,
                length: prepared.record_len(),
            },
        )?;
        let append_bytes = prepared
            .inline_count()
            .checked_mul(INDEX_RECORD_LEN_V1)
            .ok_or(IndexFormatError::IndexTailOverflow {
                old_tail,
                record_count: prepared.inline_count(),
            })?;
        let new_tail =
            old_tail
                .checked_add(append_bytes)
                .ok_or(IndexFormatError::IndexTailOverflow {
                    old_tail,
                    record_count: prepared.inline_count(),
                })?;
        Ok(Self {
            prepared,
            pack_id,
            prepare_offset,
            old_tail,
            new_tail,
        })
    }

    pub(crate) const fn old_tail(&self) -> u64 { self.old_tail }

    pub(crate) const fn new_tail(&self) -> u64 { self.new_tail }

    pub(crate) fn record_count(&self) -> u64 { self.prepared.inline_count() }

    pub(crate) fn records(
        &self,
    ) -> impl Iterator<Item = Result<ObjectIndexRecordV1, IndexFormatError>> + '_ {
        let sequence = self.prepared.sequence();
        self.prepared.object_layouts().filter_map(move |layout| {
            if layout.disposition() == LogPackObjectDispositionV1::Reuse {
                None
            } else {
                Some(ObjectIndexRecordV1::from_inline(
                    self.pack_id,
                    self.prepare_offset,
                    sequence,
                    layout,
                ))
            }
        })
    }
}

/// Complete fresh-index image derived from the exact checkpoint PREPARE
/// layout. The plan borrows the already-admitted source, emits each record
/// through a caller callback, and never materializes a record collection.
pub(crate) struct PreparedGenerationCheckpointIndexV1<'prepared, 'source, P, Source>
where
    P: SemanticProfile,
    Source: GenerationCheckpointSourceV1,
{
    prepared: &'prepared PreparedGenerationCheckpointV1<'source, P, Source>,
    header: ObjectIndexHeaderV1,
    prepare_address: LogPackAddress,
    exact_tail: u64,
    publication: CommitPublicationV1,
}

impl<'prepared, 'source, P, Source>
    PreparedGenerationCheckpointIndexV1<'prepared, 'source, P, Source>
where
    P: SemanticProfile,
    Source: GenerationCheckpointSourceV1,
{
    pub(crate) fn new(
        header: ObjectIndexHeaderV1,
        prepare_address: LogPackAddress,
        prepared: &'prepared PreparedGenerationCheckpointV1<'source, P, Source>,
    ) -> Result<Self, IndexFormatError> {
        if header.store_id() != prepared.store_id() {
            return Err(IndexFormatError::PrepareStore {
                expected: header.store_id(),
                actual: prepared.store_id(),
            });
        }
        if prepare_address.length() != prepared.record_len() {
            return Err(IndexFormatError::Length {
                field: "checkpoint PREPARE address",
                expected: prepared.record_len(),
                actual: prepare_address.length(),
            });
        }
        if prepare_address.offset() != PACK_HEADER_LEN_V1 {
            return Err(IndexFormatError::CheckpointPrepareOffset {
                expected: PACK_HEADER_LEN_V1,
                actual: prepare_address.offset(),
            });
        }
        prepare_address
            .offset()
            .checked_add(prepared.record_len())
            .ok_or(IndexFormatError::PrepareEndOverflow {
                offset: prepare_address.offset(),
                length: prepared.record_len(),
            })?;

        let descriptor = prepared.descriptor();
        let record_count = descriptor.inventory().data_objects();
        let record_bytes = record_count.checked_mul(INDEX_RECORD_LEN_V1).ok_or(
            IndexFormatError::IndexTailOverflow {
                old_tail: INDEX_HEADER_LEN_V1,
                record_count,
            },
        )?;
        let exact_tail = INDEX_HEADER_LEN_V1.checked_add(record_bytes).ok_or(
            IndexFormatError::IndexTailOverflow {
                old_tail: INDEX_HEADER_LEN_V1,
                record_count,
            },
        )?;
        let publication = CommitPublicationV1::new(
            prepare_address.segment(),
            header.generation().get(),
            exact_tail,
            descriptor.retention_manifest(),
            descriptor.current_state_canonical_bytes().get(),
            descriptor
                .inventory()
                .retained_union_canonical_bytes()
                .get(),
        )?;
        Ok(Self {
            prepared,
            header,
            prepare_address,
            exact_tail,
            publication,
        })
    }

    pub(crate) const fn header(&self) -> ObjectIndexHeaderV1 { self.header }

    pub(crate) const fn prepare_address(&self) -> LogPackAddress { self.prepare_address }

    pub(crate) fn record_count(&self) -> u64 {
        self.prepared.descriptor().inventory().data_objects()
    }

    pub(crate) const fn exact_tail(&self) -> u64 { self.exact_tail }

    pub(crate) const fn publication(&self) -> CommitPublicationV1 { self.publication }

    pub(crate) fn try_visit_records<E>(
        &self,
        mut visitor: impl FnMut(ObjectIndexRecordV1) -> Result<(), E>,
    ) -> Result<(), PreparedGenerationCheckpointIndexVisitErrorV1<E>> {
        let pack_id = self.prepare_address.pack_id();
        let prepare_offset = self.prepare_address.offset();
        let sequence = self.prepared.sequence();
        self.prepared
            .try_visit_object_layouts(|layout| {
                let record = ObjectIndexRecordV1::from_checkpoint_layout(
                    pack_id,
                    prepare_offset,
                    sequence,
                    layout,
                )
                .map_err(CheckpointIndexVisitorErrorV1::Index)?;
                visitor(record).map_err(CheckpointIndexVisitorErrorV1::Visitor)
            })
            .map_err(|error| match error {
                GenerationCheckpointLayoutVisitErrorV1::Checkpoint(error) => {
                    PreparedGenerationCheckpointIndexVisitErrorV1::Checkpoint(error)
                }
                GenerationCheckpointLayoutVisitErrorV1::Invariant(error) => {
                    PreparedGenerationCheckpointIndexVisitErrorV1::Invariant(error)
                }
                GenerationCheckpointLayoutVisitErrorV1::Visitor(
                    CheckpointIndexVisitorErrorV1::Index(error),
                ) => PreparedGenerationCheckpointIndexVisitErrorV1::Index(error),
                GenerationCheckpointLayoutVisitErrorV1::Visitor(
                    CheckpointIndexVisitorErrorV1::Visitor(error),
                ) => PreparedGenerationCheckpointIndexVisitErrorV1::Visitor(error),
            })
    }
}

enum CheckpointIndexVisitorErrorV1<E> {
    Index(IndexFormatError),
    Visitor(E),
}

#[derive(Debug)]
pub(crate) enum PreparedGenerationCheckpointIndexVisitErrorV1<E> {
    Checkpoint(GenerationCheckpointErrorV1),
    Invariant(LogPackFormatError),
    Index(IndexFormatError),
    Visitor(E),
}

impl<E: Display> Display for PreparedGenerationCheckpointIndexVisitErrorV1<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checkpoint(error) => Display::fmt(error, formatter),
            Self::Invariant(error) => write!(formatter, "checkpoint layout invariant: {error}"),
            Self::Index(error) => Display::fmt(error, formatter),
            Self::Visitor(error) => write!(formatter, "checkpoint index visitor failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error
    for PreparedGenerationCheckpointIndexVisitErrorV1<E>
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Checkpoint(error) => Some(error),
            Self::Invariant(error) => Some(error),
            Self::Index(error) => Some(error),
            Self::Visitor(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexFormatError {
    Canonical(CanonicalFormatError),
    LogPack(LogPackFormatError),
    Length {
        field: &'static str,
        expected: u64,
        actual: u64,
    },
    LengthOutOfRange,
    InvalidDomain,
    UnsupportedVersion(u64),
    DeclaredHeaderLength(u64),
    Checksum {
        field: &'static str,
        expected: u32,
        actual: u32,
    },
    ChecksumOutOfRange {
        field: &'static str,
        encoded: u64,
    },
    ZeroGeneration,
    GenerationOverflow(u64),
    Store {
        expected: StoreId,
        actual: StoreId,
    },
    PrepareStore {
        expected: StoreId,
        actual: StoreId,
    },
    Generation {
        expected: IndexGenerationV1,
        actual: IndexGenerationV1,
    },
    CanonicalOffsetBeforePackHeader(u64),
    AbsoluteOffsetOverflow {
        prepare_offset: u64,
        relative_offset: u64,
    },
    PrepareOffsetBeforePackHeader(u64),
    CheckpointPrepareOffset {
        expected: u64,
        actual: u64,
    },
    PrepareEndOverflow {
        offset: u64,
        length: u64,
    },
    IndexTailOverflow {
        old_tail: u64,
        record_count: u64,
    },
    ObjectEndOverflow {
        offset: u64,
        length: u64,
    },
    ReuseHasNoIndexLocation(ObjectRefV1),
    InlineObjectMissingOffset(ObjectRefV1),
    DuplicateData(ObjectRefV1),
    DigestReferenceConflict {
        digest: ObjectDigest,
        existing: ObjectRefV1,
        candidate: ObjectRefV1,
    },
    RecordCountOverflow,
    IndexHeaderNotVerified,
    CanonicalStreamTailOverflow {
        offset: u64,
    },
    CanonicalStreamTail {
        expected: u64,
        appointed: u64,
    },
    DerivedRecordBeyondAppointedTail {
        record_offset: u64,
        appointed_tail: u64,
    },
    RecordMismatch {
        offset: u64,
        expected: ObjectIndexRecordV1,
        actual: ObjectIndexRecordV1,
    },
    DurableTailBeyondFile {
        durable_tail: u64,
        available: u64,
    },
    UnalignedDurableTail(u64),
}

impl From<CanonicalFormatError> for IndexFormatError {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<LogPackFormatError> for IndexFormatError {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(error) }
}

impl Display for IndexFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::LogPack(error) => Display::fmt(error, formatter),
            Self::Length {
                field,
                expected,
                actual,
            } => write!(formatter, "{field} length {actual} does not equal {expected}"),
            Self::LengthOutOfRange => {
                formatter.write_str("index byte length does not fit the portable u64 domain")
            }
            Self::InvalidDomain => formatter.write_str("invalid RSpace index domain"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported RSpace index version {version}")
            }
            Self::DeclaredHeaderLength(length) => {
                write!(formatter, "RSpace index declares header length {length}")
            }
            Self::Checksum {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "{field} CRC32C mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
            Self::ChecksumOutOfRange { field, encoded } => {
                write!(formatter, "{field} checksum {encoded:#018x} is not a zero-extended CRC32C")
            }
            Self::ZeroGeneration => formatter.write_str("index generation zero is reserved"),
            Self::GenerationOverflow(generation) => {
                write!(formatter, "index generation {generation} has no successor")
            }
            Self::Store { expected, actual } => {
                write!(formatter, "expected RSpace index store {expected}, got {actual}")
            }
            Self::PrepareStore { expected, actual } => write!(
                formatter,
                "index store {expected} does not match prepared WAL record store {actual}"
            ),
            Self::Generation { expected, actual } => write!(
                formatter,
                "expected RSpace index generation {}, got {}",
                expected.get(),
                actual.get()
            ),
            Self::CanonicalOffsetBeforePackHeader(offset) => write!(
                formatter,
                "index canonical-body offset {offset} lies inside the pack header"
            ),
            Self::AbsoluteOffsetOverflow {
                prepare_offset,
                relative_offset,
            } => write!(
                formatter,
                "prepare offset {prepare_offset} plus object offset {relative_offset} overflows \
                 u64"
            ),
            Self::PrepareOffsetBeforePackHeader(offset) => {
                write!(formatter, "prepare offset {offset} lies inside the pack header")
            }
            Self::CheckpointPrepareOffset { expected, actual } => write!(
                formatter,
                "checkpoint PREPARE offset {actual} does not equal required fresh-pack offset \
                 {expected}"
            ),
            Self::PrepareEndOverflow { offset, length } => write!(
                formatter,
                "prepare offset {offset} plus record length {length} overflows u64"
            ),
            Self::IndexTailOverflow {
                old_tail,
                record_count,
            } => write!(
                formatter,
                "index tail {old_tail} plus {record_count} 72-byte records overflows u64"
            ),
            Self::ObjectEndOverflow { offset, length } => write!(
                formatter,
                "indexed object offset {offset} plus length {length} overflows u64"
            ),
            Self::ReuseHasNoIndexLocation(reference) => write!(
                formatter,
                "reused object {} has no new physical index location",
                reference.digest()
            ),
            Self::InlineObjectMissingOffset(reference) => write!(
                formatter,
                "inline object {} is missing its prepared body offset",
                reference.digest()
            ),
            Self::DuplicateData(reference) => write!(
                formatter,
                "object {} is encoded as DATA more than once in one live index generation",
                reference.digest()
            ),
            Self::DigestReferenceConflict {
                digest,
                existing,
                candidate,
            } => write!(
                formatter,
                "object digest {digest} has conflicting references {existing:?} and {candidate:?}"
            ),
            Self::RecordCountOverflow => {
                formatter.write_str("committed object index record count overflows u64")
            }
            Self::IndexHeaderNotVerified => {
                formatter.write_str("physical index header was not verified before its records")
            }
            Self::CanonicalStreamTailOverflow { offset } => write!(
                formatter,
                "canonical index stream cannot advance its tail from offset {offset}"
            ),
            Self::CanonicalStreamTail {
                expected,
                appointed,
            } => write!(
                formatter,
                "WAL-derived canonical index tail {expected} does not equal catalog-appointed \
                 tail {appointed}"
            ),
            Self::DerivedRecordBeyondAppointedTail {
                record_offset,
                appointed_tail,
            } => write!(
                formatter,
                "WAL-derived index record at offset {record_offset} extends beyond \
                 catalog-appointed tail {appointed_tail}"
            ),
            Self::RecordMismatch {
                offset,
                expected,
                actual,
            } => write!(
                formatter,
                "physical index record at offset {offset} is {actual:?}, but committed WAL \
                 requires {expected:?}"
            ),
            Self::DurableTailBeyondFile {
                durable_tail,
                available,
            } => write!(
                formatter,
                "durable index tail {durable_tail} exceeds available file bytes {available}"
            ),
            Self::UnalignedDurableTail(tail) => write!(
                formatter,
                "durable index tail {tail} is not 64 plus a whole number of 72-byte records"
            ),
        }
    }
}

impl std::error::Error for IndexFormatError {}

fn require_exact_len(
    bytes: &[u8],
    expected: u64,
    field: &'static str,
) -> Result<(), IndexFormatError> {
    let actual = u64::try_from(bytes.len()).map_err(|_| IndexFormatError::LengthOutOfRange)?;
    if actual != expected {
        return Err(IndexFormatError::Length {
            field,
            expected,
            actual,
        });
    }
    Ok(())
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed index integer extent"),
    )
}

const _: () = {
    assert!(STORE_ID_LEN == 16);
    assert!(INDEX_HEADER_AUTH_LEN == 56);
    assert!(INDEX_RECORD_AUTH_LEN == 64);
};
