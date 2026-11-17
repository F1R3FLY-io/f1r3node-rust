//! Canonical, normalized RSpace state transitions.
//!
//! Arithmetic and first-wins operations are the resident producer semantics.
//! The durable form is deliberately stronger: every changed path carries its
//! exact before and after state. That makes stale-base replay observable,
//! makes same-base recompilation deterministic, and gives the later
//! manifest/WAL recovery layer enough evidence to recognize a wholly
//! committed target without accepting mixed endpoint states. The live CoW and
//! persistent-DAG applicators therefore consume one transition language.

use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, FORMAT_VERSION_V1, OBJECT_DIGEST_LEN,
    OBJECT_DOMAIN_LEN, OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor,
    checked_len, checked_sum, parse_envelope, write_envelope_header, write_u64,
};
use super::manifest::SemanticNamespace;
use super::object::{CanonicalBlob, CanonicalBlobKind, EndpointValue};
use crate::rspace::bag::{Bag, BagDelta};
use crate::rspace::rho_path::{RhoPathError, RhoPathStreamValidatorV1, RhoPathV1};

const MUTATION_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Mut\0\0\0";
const MUTATION_SCHEMA_VERSION_V1: u64 = 1;
const MUTATION_SECTION_COUNT: u64 = 3;
const OBJECT_REF_LEN: u64 = 32 + 8;

/// Maximum caller-owned scratch admitted by the bounded mutation scanner.
///
/// The allocation is split into equal primary and prior-key-reread lanes.
/// Fixed-width fields remain in small stack arrays, so retained heap does not
/// depend on mutation, key, or entry cardinality.
pub(crate) const MUTATION_STREAM_MAX_SCRATCH_BYTES_V1: usize = 64 * 1024;

/// Positional byte source used by the bounded mutation scanner.
///
/// Production implementations must keep the addressed image stable for the
/// scanner's lifetime. The scanner nevertheless re-hashes every prior key it
/// rereads, so mutation of ordering evidence is detected before acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MutationReadClassV1 {
    /// A byte in the mutation's exact, contiguous authenticated extent.
    Primary,
    /// A byte reread solely to prove the previous key's ordering identity.
    PriorKeyReread,
}

pub(crate) trait MutationBatchReadAtV1 {
    type Error;

    fn read_exact_at(
        &mut self,
        class: MutationReadClassV1,
        offset: u64,
        destination: &mut [u8],
    ) -> Result<(), Self::Error>;
}

/// Exact, owned result of one accepted streamed mutation image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamedMutationBatchV1 {
    payload_digest: ObjectDigest,
    base: StateVersionV1,
    target_generation: u64,
    entry_count: u64,
    key_bytes: u64,
    primary_read_bytes: u64,
    prior_key_reread_bytes: u64,
    physical_read_bytes: u64,
}

impl StreamedMutationBatchV1 {
    pub(crate) const fn payload_digest(self) -> ObjectDigest { self.payload_digest }

    pub(crate) const fn base(self) -> StateVersionV1 { self.base }

    pub(crate) const fn target_generation(self) -> u64 { self.target_generation }

    pub(crate) const fn entry_count(self) -> u64 { self.entry_count }

    pub(crate) const fn key_bytes(self) -> u64 { self.key_bytes }

    /// Exact bytes read from the mutation's contiguous primary extent.
    pub(crate) const fn primary_read_bytes(self) -> u64 { self.primary_read_bytes }

    /// Exact bytes reread to validate prior-key identity and ordering.
    pub(crate) const fn prior_key_reread_bytes(self) -> u64 { self.prior_key_reread_bytes }

    /// Successful primary bytes plus exact prior-key reread bytes.
    pub(crate) const fn physical_read_bytes(self) -> u64 { self.physical_read_bytes }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MutationStreamErrorV1<E> {
    Read(E),
    Mutation(MutationError),
    ScratchBytes {
        maximum: usize,
        actual: usize,
    },
    PhysicalReadLimit {
        limit: u64,
        attempted: u64,
    },
    SourceChanged {
        offset: u64,
        length: u64,
        expected: ObjectDigest,
        actual: ObjectDigest,
    },
}

impl<E> From<MutationError> for MutationStreamErrorV1<E> {
    fn from(error: MutationError) -> Self { Self::Mutation(error) }
}

/// A failed scan reports exactly where the primary cursor stopped and how
/// much physical work completed. Semantic failures normally report
/// `source_cursor == mutation_len` because the scanner raw-drains the declared
/// extent; only I/O or resource refusal may stop it earlier.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MutationStreamFailureV1<E> {
    error: MutationStreamErrorV1<E>,
    source_cursor: u64,
    primary_read_bytes: u64,
    prior_key_reread_bytes: u64,
    physical_read_bytes: u64,
}

impl<E> MutationStreamFailureV1<E> {
    pub(crate) const fn error(&self) -> &MutationStreamErrorV1<E> { &self.error }

    pub(crate) const fn source_cursor(&self) -> u64 { self.source_cursor }

    pub(crate) const fn primary_read_bytes(&self) -> u64 { self.primary_read_bytes }

    pub(crate) const fn prior_key_reread_bytes(&self) -> u64 { self.prior_key_reread_bytes }

    pub(crate) const fn physical_read_bytes(&self) -> u64 { self.physical_read_bytes }

    pub(crate) fn into_error(self) -> MutationStreamErrorV1<E> { self.error }
}

#[derive(Clone, Copy)]
struct StreamedKeyIdentityV1 {
    offset: u64,
    len: u64,
    digest: ObjectDigest,
}

struct MutationStreamCursorV1<'source, 'observer, R, F> {
    source: &'source mut R,
    source_offset: u64,
    mutation_len: u64,
    cursor: u64,
    primary_read_bytes: u64,
    reread_read_bytes: u64,
    reread_admitted_bytes: u64,
    max_physical_read_bytes: u64,
    payload_hasher: Blake2b<U32>,
    observe_primary: &'observer mut F,
}

impl<'source, 'observer, R, F> MutationStreamCursorV1<'source, 'observer, R, F>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    fn canonical_error(error: CanonicalFormatError) -> MutationStreamErrorV1<R::Error> {
        MutationStreamErrorV1::Mutation(error.into())
    }

    fn body_offset(&self, body_start: u64) -> Result<usize, MutationStreamErrorV1<R::Error>> {
        let relative = self.cursor.checked_sub(body_start).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::LengthOverflow {
                field: "mutation body cursor",
            })
        })?;
        usize::try_from(relative).map_err(|_| {
            Self::canonical_error(CanonicalFormatError::LengthOutOfRange {
                field: "mutation body cursor",
                value: relative,
            })
        })
    }

    fn available_usize(&self) -> Result<usize, MutationStreamErrorV1<R::Error>> {
        let available = self.mutation_len.checked_sub(self.cursor).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::LengthOverflow {
                field: "mutation stream cursor",
            })
        })?;
        usize::try_from(available).map_err(|_| {
            Self::canonical_error(CanonicalFormatError::LengthOutOfRange {
                field: "mutation stream available bytes",
                value: available,
            })
        })
    }

    fn require_primary_extent(
        &self,
        len: usize,
        error_base: u64,
    ) -> Result<(), MutationStreamErrorV1<R::Error>> {
        let len_u64 = u64::try_from(len).map_err(|_| {
            Self::canonical_error(CanonicalFormatError::LengthOverflow {
                field: "mutation stream read",
            })
        })?;
        let end = self.cursor.checked_add(len_u64).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::LengthOutOfRange {
                field: "cursor extent",
                value: u64::MAX,
            })
        })?;
        if end > self.mutation_len {
            let offset = self.body_offset(error_base)?;
            return Err(Self::canonical_error(CanonicalFormatError::Truncated {
                offset,
                needed: len,
                available: self.available_usize()?,
            }));
        }
        Ok(())
    }

    fn read_primary_exact(
        &mut self,
        destination: &mut [u8],
        error_base: u64,
    ) -> Result<(), MutationStreamErrorV1<R::Error>> {
        self.require_primary_extent(destination.len(), error_base)?;
        let absolute = self.source_offset.checked_add(self.cursor).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::LengthOverflow {
                field: "mutation source offset",
            })
        })?;
        self.source
            .read_exact_at(MutationReadClassV1::Primary, absolute, destination)
            .map_err(MutationStreamErrorV1::Read)?;
        (self.observe_primary)(destination);
        self.payload_hasher.update(&*destination);
        let read = u64::try_from(destination.len()).expect("slice length fits u64");
        self.cursor = self.cursor.checked_add(read).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::LengthOverflow {
                field: "mutation primary cursor",
            })
        })?;
        self.primary_read_bytes = self.primary_read_bytes.checked_add(read).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::CounterOverflow(
                "mutation primary read bytes",
            ))
        })?;
        Ok(())
    }

    fn read_u64(&mut self, error_base: u64) -> Result<u64, MutationStreamErrorV1<R::Error>> {
        let mut bytes = [0u8; 8];
        self.read_primary_exact(&mut bytes, error_base)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_u8(&mut self, error_base: u64) -> Result<u8, MutationStreamErrorV1<R::Error>> {
        let mut byte = [0u8; 1];
        self.read_primary_exact(&mut byte, error_base)?;
        Ok(byte[0])
    }

    fn read_reference(
        &mut self,
        error_base: u64,
    ) -> Result<ObjectRefV1, MutationStreamErrorV1<R::Error>> {
        let mut bytes = [0u8; OBJECT_REF_LEN as usize];
        self.read_primary_exact(&mut bytes, error_base)?;
        let mut cursor = ReadCursor::new(&bytes);
        let reference = ObjectRefV1::parse(&mut cursor).map_err(Self::canonical_error)?;
        cursor.finish().map_err(Self::canonical_error)?;
        Ok(reference)
    }

    fn admit_reread(&mut self, bytes: u64) -> Result<(), MutationStreamErrorV1<R::Error>> {
        let reread = self
            .reread_admitted_bytes
            .checked_add(bytes)
            .ok_or_else(|| {
                Self::canonical_error(CanonicalFormatError::CounterOverflow(
                    "mutation prior-key reread bytes",
                ))
            })?;
        let attempted = self.mutation_len.checked_add(reread).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::CounterOverflow(
                "mutation physical read bytes",
            ))
        })?;
        if attempted > self.max_physical_read_bytes {
            return Err(MutationStreamErrorV1::PhysicalReadLimit {
                limit: self.max_physical_read_bytes,
                attempted,
            });
        }
        self.reread_admitted_bytes = reread;
        Ok(())
    }

    fn read_reread_exact(
        &mut self,
        relative_offset: u64,
        destination: &mut [u8],
    ) -> Result<(), MutationStreamErrorV1<R::Error>> {
        let absolute = self
            .source_offset
            .checked_add(relative_offset)
            .ok_or_else(|| {
                Self::canonical_error(CanonicalFormatError::LengthOverflow {
                    field: "mutation reread source offset",
                })
            })?;
        self.source
            .read_exact_at(MutationReadClassV1::PriorKeyReread, absolute, destination)
            .map_err(MutationStreamErrorV1::Read)?;
        let read = u64::try_from(destination.len()).expect("slice length fits u64");
        self.reread_read_bytes = self.reread_read_bytes.checked_add(read).ok_or_else(|| {
            Self::canonical_error(CanonicalFormatError::CounterOverflow(
                "mutation prior-key reread bytes",
            ))
        })?;
        Ok(())
    }

    fn raw_drain(
        &mut self,
        primary_lane: &mut [u8],
    ) -> Result<(), MutationStreamErrorV1<R::Error>> {
        while self.cursor < self.mutation_len {
            let remaining = self.mutation_len - self.cursor;
            let take = usize::try_from(remaining.min(primary_lane.len() as u64))
                .expect("bounded primary lane length fits usize");
            self.read_primary_exact(&mut primary_lane[..take], 0)?;
        }
        Ok(())
    }

    fn physical_read_bytes(&self) -> u64 {
        self.primary_read_bytes
            .checked_add(self.reread_read_bytes)
            .expect("physical reads were checked as they accumulated")
    }

    fn primary_read_bytes(&self) -> u64 { self.primary_read_bytes }

    fn prior_key_reread_bytes(&self) -> u64 { self.reread_read_bytes }
}

/// Validate one canonical mutation directly from a positional source.
///
/// Every primary byte is sent to `observe_primary` exactly once, including a
/// raw drain after the first semantic error. Prior-key rereads are excluded
/// from that observer. `max_physical_read_bytes` admits the complete primary
/// extent before the first read and each exact prior-key reread before it is
/// issued.
#[expect(
    clippy::result_large_err,
    reason = "MutationStreamFailureV1 (128+ B) holds the error, stop cursor and 3 read counters"
)]
pub(crate) fn stream_mutation_batch_v1<R, F>(
    source: &mut R,
    source_offset: u64,
    mutation_len: u64,
    limits: MutationDecodeLimits,
    max_physical_read_bytes: u64,
    scratch: &mut [u8],
    mut observe_primary: F,
) -> Result<StreamedMutationBatchV1, MutationStreamFailureV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let invalid_scratch = scratch.len() < 2 || scratch.len() > MUTATION_STREAM_MAX_SCRATCH_BYTES_V1;
    if invalid_scratch {
        return Err(MutationStreamFailureV1 {
            error: MutationStreamErrorV1::ScratchBytes {
                maximum: MUTATION_STREAM_MAX_SCRATCH_BYTES_V1,
                actual: scratch.len(),
            },
            source_cursor: 0,
            primary_read_bytes: 0,
            prior_key_reread_bytes: 0,
            physical_read_bytes: 0,
        });
    }
    if mutation_len > max_physical_read_bytes {
        return Err(MutationStreamFailureV1 {
            error: MutationStreamErrorV1::PhysicalReadLimit {
                limit: max_physical_read_bytes,
                attempted: mutation_len,
            },
            source_cursor: 0,
            primary_read_bytes: 0,
            prior_key_reread_bytes: 0,
            physical_read_bytes: 0,
        });
    }

    let lane_len = scratch.len() / 2;
    let (primary_lane, remainder) = scratch.split_at_mut(lane_len);
    let reread_lane = &mut remainder[..lane_len];
    let mut cursor = MutationStreamCursorV1 {
        source,
        source_offset,
        mutation_len,
        cursor: 0,
        primary_read_bytes: 0,
        reread_read_bytes: 0,
        reread_admitted_bytes: 0,
        max_physical_read_bytes,
        payload_hasher: Blake2b::<U32>::new(),
        observe_primary: &mut observe_primary,
    };

    let parsed = stream_mutation_inner(&mut cursor, limits, primary_lane, reread_lane);
    let parsed = match parsed {
        Ok(parsed) => Ok(parsed),
        Err(
            error @ (MutationStreamErrorV1::Mutation(_) |
            MutationStreamErrorV1::SourceChanged { .. }),
        ) => match cursor.raw_drain(primary_lane) {
            Ok(()) => Err(error),
            Err(drain_error) => Err(drain_error),
        },
        Err(error) => Err(error),
    };

    match parsed {
        Ok((base, target_generation, entry_count, key_bytes)) => {
            debug_assert_eq!(cursor.cursor, mutation_len);
            let primary_read_bytes = cursor.primary_read_bytes();
            let prior_key_reread_bytes = cursor.prior_key_reread_bytes();
            let physical_read_bytes = cursor.physical_read_bytes();
            let payload_digest = finish_stream_digest(cursor.payload_hasher);
            Ok(StreamedMutationBatchV1 {
                payload_digest,
                base,
                target_generation,
                entry_count,
                key_bytes,
                primary_read_bytes,
                prior_key_reread_bytes,
                physical_read_bytes,
            })
        }
        Err(error) => {
            let primary_read_bytes = cursor.primary_read_bytes();
            let prior_key_reread_bytes = cursor.prior_key_reread_bytes();
            Err(MutationStreamFailureV1 {
                error,
                source_cursor: cursor.cursor,
                primary_read_bytes,
                prior_key_reread_bytes,
                physical_read_bytes: cursor.physical_read_bytes(),
            })
        }
    }
}

fn stream_mutation_inner<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    limits: MutationDecodeLimits,
    primary_lane: &mut [u8],
    reread_lane: &mut [u8],
) -> Result<(StateVersionV1, u64, u64, u64), MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let mut domain = [0u8; OBJECT_DOMAIN_LEN];
    cursor.read_primary_exact(&mut domain, 0)?;
    if &domain != MUTATION_DOMAIN {
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::InvalidDomain {
                expected: MUTATION_DOMAIN,
            },
        ));
    }
    let version = cursor.read_u64(0)?;
    if version != FORMAT_VERSION_V1 {
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace mutation batch",
                version,
            },
        ));
    }
    let body_len = cursor.read_u64(0)?;
    let body_len_usize = usize::try_from(body_len).map_err(|_| {
        MutationStreamCursorV1::<R, F>::canonical_error(CanonicalFormatError::LengthOutOfRange {
            field: "object body",
            value: body_len,
        })
    })?;
    let body_start = OBJECT_ENVELOPE_HEADER_LEN;
    let expected_end = body_start.checked_add(body_len).ok_or_else(|| {
        MutationStreamCursorV1::<R, F>::canonical_error(CanonicalFormatError::LengthOutOfRange {
            field: "cursor extent",
            value: u64::MAX,
        })
    })?;
    if expected_end > cursor.mutation_len {
        let available =
            usize::try_from(cursor.mutation_len.saturating_sub(body_start)).map_err(|_| {
                MutationStreamCursorV1::<R, F>::canonical_error(
                    CanonicalFormatError::LengthOutOfRange {
                        field: "mutation stream available bytes",
                        value: cursor.mutation_len.saturating_sub(body_start),
                    },
                )
            })?;
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::Truncated {
                offset: body_start as usize,
                needed: body_len_usize,
                available,
            },
        ));
    }
    if expected_end < cursor.mutation_len {
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::TrailingBytes {
                offset: usize::try_from(expected_end).map_err(|_| {
                    MutationStreamCursorV1::<R, F>::canonical_error(
                        CanonicalFormatError::LengthOutOfRange {
                            field: "mutation envelope end",
                            value: expected_end,
                        },
                    )
                })?,
                trailing: usize::try_from(cursor.mutation_len - expected_end).map_err(|_| {
                    MutationStreamCursorV1::<R, F>::canonical_error(
                        CanonicalFormatError::LengthOutOfRange {
                            field: "mutation trailing bytes",
                            value: cursor.mutation_len - expected_end,
                        },
                    )
                })?,
            },
        ));
    }

    let schema_version = cursor.read_u64(body_start)?;
    if schema_version != MUTATION_SCHEMA_VERSION_V1 {
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace mutation schema",
                version: schema_version,
            },
        ));
    }
    let base_generation = cursor.read_u64(body_start)?;
    let target_generation = cursor.read_u64(body_start)?;
    let base_manifest = cursor.read_reference(body_start)?;
    let section_count = cursor.read_u64(body_start)?;
    if section_count != MUTATION_SECTION_COUNT {
        return Err(MutationError::SectionCount(section_count).into());
    }
    let base = StateVersionV1::new(base_generation, base_manifest);
    require_contiguous_generation(base_generation, target_generation)?;

    let mut total_entries = 0u64;
    let mut total_key_bytes = 0u64;
    stream_count_section(
        cursor,
        body_start,
        SemanticNamespace::DataBag,
        limits,
        &mut total_entries,
        &mut total_key_bytes,
        primary_lane,
        reread_lane,
    )?;
    stream_content_section(
        cursor,
        body_start,
        limits,
        &mut total_entries,
        &mut total_key_bytes,
        primary_lane,
        reread_lane,
    )?;
    stream_count_section(
        cursor,
        body_start,
        SemanticNamespace::JoinBag,
        limits,
        &mut total_entries,
        &mut total_key_bytes,
        primary_lane,
        reread_lane,
    )?;

    if cursor.cursor != expected_end {
        let offset = cursor.body_offset(body_start)?;
        let trailing = usize::try_from(expected_end - cursor.cursor).map_err(|_| {
            MutationStreamCursorV1::<R, F>::canonical_error(
                CanonicalFormatError::LengthOutOfRange {
                    field: "mutation body trailing bytes",
                    value: expected_end - cursor.cursor,
                },
            )
        })?;
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::TrailingBytes { offset, trailing },
        ));
    }
    if total_entries == 0 {
        return Err(MutationError::EmptyBatch.into());
    }
    Ok((base, target_generation, total_entries, total_key_bytes))
}

#[allow(clippy::too_many_arguments)]
fn stream_count_section<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    body_start: u64,
    namespace: SemanticNamespace,
    limits: MutationDecodeLimits,
    total_entries: &mut u64,
    total_key_bytes: &mut u64,
    primary_lane: &mut [u8],
    reread_lane: &mut [u8],
) -> Result<(), MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let entry_count = stream_section_header(cursor, body_start, namespace, limits, total_entries)?;
    let mut previous = None;
    for _ in 0..entry_count {
        let current = stream_key(
            cursor,
            body_start,
            namespace,
            limits,
            total_key_bytes,
            previous,
            primary_lane,
            reread_lane,
        )?;
        previous = Some(current);
        CountTransition::new(cursor.read_u64(body_start)?, cursor.read_u64(body_start)?)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stream_content_section<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    body_start: u64,
    limits: MutationDecodeLimits,
    total_entries: &mut u64,
    total_key_bytes: &mut u64,
    primary_lane: &mut [u8],
    reread_lane: &mut [u8],
) -> Result<(), MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let namespace = SemanticNamespace::ContinuationMap;
    let entry_count = stream_section_header(cursor, body_start, namespace, limits, total_entries)?;
    let mut previous = None;
    for _ in 0..entry_count {
        let current = stream_key(
            cursor,
            body_start,
            namespace,
            limits,
            total_key_bytes,
            previous,
            primary_lane,
            reread_lane,
        )?;
        previous = Some(current);
        ContentTransition::new(
            stream_optional_reference(cursor, body_start)?,
            stream_optional_reference(cursor, body_start)?,
        )?;
    }
    Ok(())
}

fn stream_section_header<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    body_start: u64,
    namespace: SemanticNamespace,
    limits: MutationDecodeLimits,
    total_entries: &mut u64,
) -> Result<u64, MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let actual = cursor.read_u64(body_start)?;
    if actual != namespace as u64 {
        return Err(MutationError::SectionOrder {
            expected: namespace as u64,
            actual,
        }
        .into());
    }
    let entry_count = cursor.read_u64(body_start)?;
    account_entries(entry_count, limits, total_entries)?;
    Ok(entry_count)
}

#[allow(clippy::too_many_arguments)]
fn stream_key<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    body_start: u64,
    namespace: SemanticNamespace,
    limits: MutationDecodeLimits,
    total_key_bytes: &mut u64,
    previous: Option<StreamedKeyIdentityV1>,
    primary_lane: &mut [u8],
    reread_lane: &mut [u8],
) -> Result<StreamedKeyIdentityV1, MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    let len = cursor.read_u64(body_start)?;
    let len_usize = usize::try_from(len).map_err(|_| {
        MutationStreamCursorV1::<R, F>::canonical_error(CanonicalFormatError::LengthOutOfRange {
            field: "mutation key",
            value: len,
        })
    })?;
    let attempted = total_key_bytes.checked_add(len).ok_or_else(|| {
        MutationStreamCursorV1::<R, F>::canonical_error(CanonicalFormatError::CounterOverflow(
            "mutation key bytes",
        ))
    })?;
    if attempted > limits.max_key_bytes {
        return Err(MutationStreamCursorV1::<R, F>::canonical_error(
            CanonicalFormatError::ResourceLimit {
                resource: "mutation key bytes",
                limit: limits.max_key_bytes,
                attempted,
            },
        ));
    }
    cursor.require_primary_extent(len_usize, body_start)?;

    if let Some(previous) = previous {
        cursor.admit_reread(previous.len)?;
    }
    let offset = cursor.cursor;
    let mut validator = match namespace {
        SemanticNamespace::DataBag => RhoPathStreamValidatorV1::datum(),
        SemanticNamespace::ContinuationMap => RhoPathStreamValidatorV1::continuation(),
        SemanticNamespace::JoinBag => RhoPathStreamValidatorV1::join(),
    };
    let mut current_hasher = Blake2b::<U32>::new();
    let mut previous_hasher = Blake2b::<U32>::new();
    let mut current_read = 0u64;
    let mut previous_read = 0u64;
    let mut ordering = Ordering::Equal;

    while current_read < len {
        let take = usize::try_from((len - current_read).min(primary_lane.len() as u64))
            .expect("bounded primary lane length fits usize");
        let current_chunk = &mut primary_lane[..take];
        cursor.read_primary_exact(current_chunk, body_start)?;
        validator.push(current_chunk).map_err(|source| {
            MutationStreamErrorV1::Mutation(MutationError::InvalidKey { namespace, source })
        })?;
        current_hasher.update(&*current_chunk);

        if let Some(previous) = previous {
            let remaining = previous.len.saturating_sub(previous_read);
            let compare_len = take.min(usize::try_from(remaining).unwrap_or(usize::MAX));
            if compare_len != 0 {
                let prior_chunk = &mut reread_lane[..compare_len];
                cursor.read_reread_exact(previous.offset + previous_read, prior_chunk)?;
                previous_hasher.update(&*prior_chunk);
                if ordering == Ordering::Equal {
                    ordering = prior_chunk
                        .iter()
                        .zip(current_chunk.iter())
                        .find_map(|(prior, current)| (prior != current).then(|| prior.cmp(current)))
                        .unwrap_or(Ordering::Equal);
                }
                previous_read += compare_len as u64;
            }
        }
        current_read += take as u64;
    }
    validator.finish().map_err(|source| {
        MutationStreamErrorV1::Mutation(MutationError::InvalidKey { namespace, source })
    })?;

    if let Some(previous) = previous {
        while previous_read < previous.len {
            let take =
                usize::try_from((previous.len - previous_read).min(reread_lane.len() as u64))
                    .expect("bounded reread lane length fits usize");
            let prior_chunk = &mut reread_lane[..take];
            cursor.read_reread_exact(previous.offset + previous_read, prior_chunk)?;
            previous_hasher.update(&*prior_chunk);
            previous_read += take as u64;
        }
        let actual = finish_stream_digest(previous_hasher);
        if actual != previous.digest {
            return Err(MutationStreamErrorV1::SourceChanged {
                offset: previous.offset,
                length: previous.len,
                expected: previous.digest,
                actual,
            });
        }
        if ordering == Ordering::Equal {
            ordering = previous.len.cmp(&len);
        }
        if ordering != Ordering::Less {
            return Err(MutationError::KeysNotStrict { namespace }.into());
        }
    }

    *total_key_bytes = attempted;
    Ok(StreamedKeyIdentityV1 {
        offset,
        len,
        digest: finish_stream_digest(current_hasher),
    })
}

fn stream_optional_reference<R, F>(
    cursor: &mut MutationStreamCursorV1<'_, '_, R, F>,
    body_start: u64,
) -> Result<Option<ObjectRefV1>, MutationStreamErrorV1<R::Error>>
where
    R: MutationBatchReadAtV1,
    F: FnMut(&[u8]),
{
    match cursor.read_u8(body_start)? {
        0 => Ok(None),
        1 => Ok(Some(cursor.read_reference(body_start)?)),
        tag => Err(MutationError::InvalidPresenceTag(tag).into()),
    }
}

fn finish_stream_digest(hasher: Blake2b<U32>) -> ObjectDigest {
    let bytes: [u8; OBJECT_DIGEST_LEN] = hasher.finalize().into();
    ObjectDigest::from_array(bytes)
}

/// A catalog authority at one committed generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateVersionV1 {
    generation: u64,
    manifest: ObjectRefV1,
}

impl StateVersionV1 {
    pub const fn new(generation: u64, manifest: ObjectRefV1) -> Self {
        Self {
            generation,
            manifest,
        }
    }

    pub const fn generation(self) -> u64 { self.generation }

    pub const fn manifest(self) -> ObjectRefV1 { self.manifest }
}

/// An exact multiplicity compare-and-swap.  Zero denotes an absent endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountTransition {
    before: u64,
    after: u64,
}

impl CountTransition {
    pub fn new(before: u64, after: u64) -> Result<Self, MutationError> {
        if before == after {
            return Err(MutationError::NoOpTransition);
        }
        Ok(Self { before, after })
    }

    pub const fn before(self) -> u64 { self.before }

    pub const fn after(self) -> u64 { self.after }
}

/// An exact continuation-content compare-and-swap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentTransition {
    before: Option<ObjectRefV1>,
    after: Option<ObjectRefV1>,
}

impl ContentTransition {
    pub fn new(
        before: Option<ObjectRefV1>,
        after: Option<ObjectRefV1>,
    ) -> Result<Self, MutationError> {
        if before == after {
            return Err(MutationError::NoOpTransition);
        }
        Ok(Self { before, after })
    }

    pub const fn before(self) -> Option<ObjectRefV1> { self.before }

    pub const fn after(self) -> Option<ObjectRefV1> { self.after }
}

/// Total decoder budgets.  These are work/retention limits, never trie-depth
/// or term-depth limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutationDecodeLimits {
    pub max_entries: u64,
    pub max_key_bytes: u64,
}

impl MutationDecodeLimits {
    pub const UNBOUNDED: Self = Self {
        max_entries: u64::MAX,
        max_key_bytes: u64::MAX,
    };
}

impl Default for MutationDecodeLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationError {
    Canonical(CanonicalFormatError),
    GenerationOverflow(u64),
    NonContiguousGeneration {
        base: u64,
        target: u64,
    },
    EmptyBatch,
    PoisonedBatch,
    NoOpTransition,
    DiscontinuousCount {
        expected: u64,
        actual: u64,
    },
    DiscontinuousContent {
        expected: Option<ObjectRefV1>,
        actual: Option<ObjectRefV1>,
    },
    SectionCount(u64),
    SectionOrder {
        expected: u64,
        actual: u64,
    },
    KeysNotStrict {
        namespace: SemanticNamespace,
    },
    InvalidPresenceTag(u8),
    InvalidKey {
        namespace: SemanticNamespace,
        source: RhoPathError,
    },
    DeltaCountOverflow,
    DeltaCountUnderflow,
    MissingCountObservation {
        namespace: SemanticNamespace,
    },
    InvalidContinuationSourceKind,
}

impl Display for MutationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::GenerationOverflow(base) => {
                write!(formatter, "RSpace generation {base} has no contiguous successor")
            }
            Self::NonContiguousGeneration { base, target } => write!(
                formatter,
                "RSpace mutation target generation {target} does not immediately follow {base}"
            ),
            Self::EmptyBatch => {
                formatter.write_str("an empty mutation batch has no new generation")
            }
            Self::PoisonedBatch => formatter
                .write_str("the mutation batch was discarded after an earlier producer error"),
            Self::NoOpTransition => {
                formatter.write_str("equal before and after states are not mutations")
            }
            Self::DiscontinuousCount { expected, actual } => write!(
                formatter,
                "multiplicity transition is discontinuous: expected before {expected}, got \
                 {actual}"
            ),
            Self::DiscontinuousContent { expected, actual } => write!(
                formatter,
                "content transition is discontinuous: expected before {expected:?}, got {actual:?}"
            ),
            Self::SectionCount(count) => {
                write!(formatter, "mutation batch has {count} sections, expected 3")
            }
            Self::SectionOrder { expected, actual } => write!(
                formatter,
                "mutation namespace {actual} is out of order; expected {expected}"
            ),
            Self::KeysNotStrict { namespace } => write!(
                formatter,
                "{} mutation keys are duplicated or not strictly ascending",
                namespace.name()
            ),
            Self::InvalidPresenceTag(tag) => {
                write!(formatter, "invalid mutation content-presence tag {tag}")
            }
            Self::InvalidKey { namespace, source } => {
                write!(formatter, "invalid {} mutation key: {source}", namespace.name())
            }
            Self::DeltaCountOverflow => {
                formatter.write_str("BagDelta addition exceeds the u64 multiplicity domain")
            }
            Self::DeltaCountUnderflow => {
                formatter.write_str("BagDelta removal exceeds the authoritative multiplicity")
            }
            Self::MissingCountObservation { namespace } => write!(
                formatter,
                "{} mutation has no captured post-state count for a changed path",
                namespace.name()
            ),
            Self::InvalidContinuationSourceKind => formatter.write_str(
                "continuation transition requires a ContinuationSourceV1 canonical blob",
            ),
        }
    }
}

impl std::error::Error for MutationError {}

impl From<CanonicalFormatError> for MutationError {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

/// A normalized change set over the three semantic RSpace roots.
///
/// Values are accumulated in PathMaps, so adjacent mutations share prefixes
/// and the canonical writer obtains strict byte order directly from a zipper.
#[derive(Clone)]
pub struct RSpaceMutationBatchV1 {
    base: StateVersionV1,
    target_generation: u64,
    data: PathMap<CountTransition>,
    continuations: PathMap<ContentTransition>,
    joins: PathMap<CountTransition>,
    poisoned: bool,
}

impl RSpaceMutationBatchV1 {
    pub fn new(base: StateVersionV1) -> Result<Self, MutationError> {
        let target_generation = base
            .generation
            .checked_add(1)
            .ok_or(MutationError::GenerationOverflow(base.generation))?;
        Ok(Self {
            base,
            target_generation,
            data: PathMap::new(),
            continuations: PathMap::new(),
            joins: PathMap::new(),
            poisoned: false,
        })
    }

    fn with_target(base: StateVersionV1, target_generation: u64) -> Result<Self, MutationError> {
        require_contiguous_generation(base.generation, target_generation)?;
        Self::new(base)
    }

    pub const fn base(&self) -> StateVersionV1 { self.base }

    pub const fn target_generation(&self) -> u64 { self.target_generation }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.continuations.is_empty() && self.joins.is_empty()
    }

    pub fn data_transition(&self, path: &[u8]) -> Option<CountTransition> {
        self.data.get(path).copied()
    }

    pub fn continuation_transition(&self, path: &[u8]) -> Option<ContentTransition> {
        self.continuations.get(path).copied()
    }

    pub fn join_transition(&self, path: &[u8]) -> Option<CountTransition> {
        self.joins.get(path).copied()
    }

    pub fn record_data(
        &mut self,
        path: &[u8],
        before: u64,
        after: u64,
    ) -> Result<(), MutationError> {
        if self.poisoned {
            return Err(MutationError::PoisonedBatch);
        }
        let result = validate_key(SemanticNamespace::DataBag, path)
            .and_then(|()| record_count(&mut self.data, path, before, after));
        self.poisoned |= result.is_err();
        result
    }

    /// Record a data-root transition directly from an authoritative Bag and
    /// its normalized PathMap delta.  The Bag remains the source of truth;
    /// this method only emits exact compare-and-swap endpoints into the
    /// mutation trie.  No entry projection or sorting is performed.
    pub fn record_data_delta(&mut self, base: &Bag, delta: &BagDelta) -> Result<(), MutationError> {
        for (path, amount) in delta.additions().iter() {
            let before = base.count(&path);
            let after = before
                .checked_add(amount.get())
                .ok_or(MutationError::DeltaCountOverflow)?;
            self.record_data(&path, before, after)?;
        }
        for (path, amount) in delta.removals().iter() {
            let before = base.count(&path);
            let after = before
                .checked_sub(amount.get())
                .ok_or(MutationError::DeltaCountUnderflow)?;
            self.record_data(&path, before, after)?;
        }
        Ok(())
    }

    /// Record a JoinBag transition directly from an authoritative BagDelta.
    /// Join paths are validated against the join namespace, while endpoint
    /// arithmetic remains identical to the data-root conversion.
    pub fn record_join_delta(&mut self, base: &Bag, delta: &BagDelta) -> Result<(), MutationError> {
        for (path, amount) in delta.additions().iter() {
            let before = base.count(&path);
            let after = before
                .checked_add(amount.get())
                .ok_or(MutationError::DeltaCountOverflow)?;
            self.record_join(&path, before, after)?;
        }
        for (path, amount) in delta.removals().iter() {
            let before = base.count(&path);
            let after = before
                .checked_sub(amount.get())
                .ok_or(MutationError::DeltaCountUnderflow)?;
            self.record_join(&path, before, after)?;
        }
        Ok(())
    }

    pub fn record_join(
        &mut self,
        path: &[u8],
        before: u64,
        after: u64,
    ) -> Result<(), MutationError> {
        if self.poisoned {
            return Err(MutationError::PoisonedBatch);
        }
        let result = validate_key(SemanticNamespace::JoinBag, path)
            .and_then(|()| record_count(&mut self.joins, path, before, after));
        self.poisoned |= result.is_err();
        result
    }

    pub fn record_continuation(
        &mut self,
        path: &[u8],
        before: Option<ObjectRefV1>,
        after: Option<ObjectRefV1>,
    ) -> Result<(), MutationError> {
        if self.poisoned {
            return Err(MutationError::PoisonedBatch);
        }
        let result = validate_key(SemanticNamespace::ContinuationMap, path)
            .and_then(|()| record_content(&mut self.continuations, path, before, after));
        self.poisoned |= result.is_err();
        result
    }

    /// Record a continuation transition from an already-canonical source blob.
    /// The blob remains owned by the caller for staging; this batch stores only
    /// its authenticated object reference and never decodes or re-encodes its
    /// payload.
    pub fn record_continuation_blob(
        &mut self,
        path: &[u8],
        before: Option<ObjectRefV1>,
        after: Option<&CanonicalBlob>,
    ) -> Result<(), MutationError> {
        let after_ref = after
            .filter(|blob| blob.kind() == CanonicalBlobKind::ContinuationSourceV1)
            .map(CanonicalBlob::object_ref);
        if after.is_some() && after_ref.is_none() {
            return Err(MutationError::InvalidContinuationSourceKind);
        }
        self.record_continuation(path, before, after_ref)
    }

    /// Record an already-normalized continuation transition trie. The trie is
    /// the authoritative PathMap journal; only its exact content references
    /// are copied into this batch, with no continuation decode or re-encode.
    pub fn record_continuation_transitions(
        &mut self,
        transitions: &PathMap<ContentTransition>,
    ) -> Result<(), MutationError> {
        for (path, transition) in transitions.iter() {
            self.record_continuation(&path, transition.before(), transition.after())?;
        }
        Ok(())
    }

    pub(crate) fn precede_endpoint(
        &mut self,
        namespace: SemanticNamespace,
        path: &[u8],
        before: EndpointValue,
        after: EndpointValue,
    ) -> Result<(), MutationError> {
        if self.poisoned {
            return Err(MutationError::PoisonedBatch);
        }
        let result = validate_key(namespace, path).and_then(|()| match namespace {
            SemanticNamespace::DataBag => precede_count(
                &mut self.data,
                path,
                endpoint_count(namespace, before)?,
                endpoint_count(namespace, after)?,
            ),
            SemanticNamespace::ContinuationMap => precede_content(
                &mut self.continuations,
                path,
                endpoint_content(namespace, before)?,
                endpoint_content(namespace, after)?,
            ),
            SemanticNamespace::JoinBag => precede_count(
                &mut self.joins,
                path,
                endpoint_count(namespace, before)?,
                endpoint_count(namespace, after)?,
            ),
        });
        self.poisoned |= result.is_err();
        result
    }

    /// Produce an immutable, preflighted streaming plan.  No semantic-state
    /// scan, entry projection, or sort occurs.
    pub fn plan(&self) -> Result<MutationBatchPlanV1, MutationError> {
        if self.poisoned {
            return Err(MutationError::PoisonedBatch);
        }
        if self.is_empty() {
            return Err(MutationError::EmptyBatch);
        }

        let (data_entries, data_len) = count_section_metrics(&self.data)?;
        let (continuation_entries, continuation_len) =
            content_section_metrics(&self.continuations)?;
        let (join_entries, join_len) = count_section_metrics(&self.joins)?;
        let mut body_len = 8 + 8 + 8 + OBJECT_REF_LEN + 8;
        body_len = checked_sum(body_len, 16 + data_len, "mutation data section")?;
        body_len = checked_sum(body_len, 16 + continuation_len, "mutation continuation section")?;
        body_len = checked_sum(body_len, 16 + join_len, "mutation join section")?;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "mutation batch envelope")?;
        Ok(MutationBatchPlanV1 {
            base: self.base,
            target_generation: self.target_generation,
            data: self.data.clone(),
            continuations: self.continuations.clone(),
            joins: self.joins.clone(),
            data_entries,
            continuation_entries,
            join_entries,
            body_len,
            canonical_len,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, MutationError> {
        Self::parse_with_limits(bytes, MutationDecodeLimits::UNBOUNDED)
    }

    pub fn parse_with_limits(
        bytes: &[u8],
        limits: MutationDecodeLimits,
    ) -> Result<Self, MutationError> {
        let view = MutationBatchViewV1::parse_with_limits(bytes, limits)?;
        let mut batch = Self::with_target(view.base, view.target_generation)?;
        for entry in view.data() {
            batch.data.insert(entry.key, entry.transition);
        }
        for entry in view.continuations() {
            batch.continuations.insert(entry.key, entry.transition);
        }
        for entry in view.joins() {
            batch.joins.insert(entry.key, entry.transition);
        }
        Ok(batch)
    }
}

fn require_contiguous_generation(base: u64, target: u64) -> Result<(), MutationError> {
    let expected = base
        .checked_add(1)
        .ok_or(MutationError::GenerationOverflow(base))?;
    if target != expected {
        return Err(MutationError::NonContiguousGeneration { base, target });
    }
    Ok(())
}

/// A validated, zero-copy view of one canonical mutation payload.
///
/// Recovery and the persistent-DAG compiler can iterate borrowed key ranges
/// directly from a WAL or memory-mapped pack.  The owned PathMap form remains
/// the producer-side normalization authority.
#[derive(Clone, Copy, Debug)]
pub struct MutationBatchViewV1<'a> {
    canonical_bytes: &'a [u8],
    payload_digest: ObjectDigest,
    base: StateVersionV1,
    target_generation: u64,
    data: CountSectionView<'a>,
    continuations: ContentSectionView<'a>,
    joins: CountSectionView<'a>,
}

impl<'a> MutationBatchViewV1<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, MutationError> {
        Self::parse_with_limits(bytes, MutationDecodeLimits::UNBOUNDED)
    }

    pub fn parse_with_limits(
        bytes: &'a [u8],
        limits: MutationDecodeLimits,
    ) -> Result<Self, MutationError> {
        let body = parse_envelope(bytes, MUTATION_DOMAIN, "RSpace mutation batch")?;
        let mut cursor = ReadCursor::new(body);
        let schema_version = cursor.read_u64()?;
        if schema_version != MUTATION_SCHEMA_VERSION_V1 {
            return Err(CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace mutation schema",
                version: schema_version,
            }
            .into());
        }
        let base_generation = cursor.read_u64()?;
        let target_generation = cursor.read_u64()?;
        let base_manifest = ObjectRefV1::parse(&mut cursor)?;
        let section_count = cursor.read_u64()?;
        if section_count != MUTATION_SECTION_COUNT {
            return Err(MutationError::SectionCount(section_count));
        }
        let base = StateVersionV1::new(base_generation, base_manifest);
        require_contiguous_generation(base_generation, target_generation)?;
        let mut total_entries = 0u64;
        let mut total_key_bytes = 0u64;
        let data = read_count_section(
            body,
            &mut cursor,
            SemanticNamespace::DataBag,
            limits,
            &mut total_entries,
            &mut total_key_bytes,
        )?;
        let continuations = read_content_section(
            body,
            &mut cursor,
            limits,
            &mut total_entries,
            &mut total_key_bytes,
        )?;
        let joins = read_count_section(
            body,
            &mut cursor,
            SemanticNamespace::JoinBag,
            limits,
            &mut total_entries,
            &mut total_key_bytes,
        )?;
        cursor.finish()?;
        if total_entries == 0 {
            return Err(MutationError::EmptyBatch);
        }
        Ok(Self {
            canonical_bytes: bytes,
            payload_digest: ObjectDigest::of_canonical_bytes(bytes),
            base,
            target_generation,
            data,
            continuations,
            joins,
        })
    }

    pub const fn base(self) -> StateVersionV1 { self.base }

    pub const fn target_generation(self) -> u64 { self.target_generation }

    /// Exact validated bytes suitable for direct binary-WAL streaming.
    pub const fn canonical_bytes(self) -> &'a [u8] { self.canonical_bytes }

    /// Domain-separated transaction identity over the exact canonical payload.
    pub const fn payload_digest(self) -> ObjectDigest { self.payload_digest }

    pub fn data(self) -> CountMutationIter<'a> { self.data.iter() }

    pub fn continuations(self) -> ContentMutationIter<'a> { self.continuations.iter() }

    pub fn joins(self) -> CountMutationIter<'a> { self.joins.iter() }

    pub(crate) fn prior_key_reread_bytes(self) -> u64 {
        section_prior_key_reread_bytes(self.data().map(|entry| entry.key)) +
            section_prior_key_reread_bytes(self.continuations().map(|entry| entry.key)) +
            section_prior_key_reread_bytes(self.joins().map(|entry| entry.key))
    }
}

fn section_prior_key_reread_bytes<'a>(keys: impl Iterator<Item = &'a [u8]>) -> u64 {
    let mut reread_bytes = 0u64;
    let mut previous_key_bytes = 0u64;
    for key in keys {
        reread_bytes += previous_key_bytes;
        previous_key_bytes = key.len() as u64;
    }
    reread_bytes
}

#[derive(Clone, Copy, Debug)]
struct CountSectionView<'a> {
    entry_count: u64,
    entries: &'a [u8],
}

impl<'a> CountSectionView<'a> {
    fn iter(self) -> CountMutationIter<'a> {
        CountMutationIter {
            cursor: ReadCursor::new(self.entries),
            remaining: self.entry_count,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ContentSectionView<'a> {
    entry_count: u64,
    entries: &'a [u8],
}

impl<'a> ContentSectionView<'a> {
    fn iter(self) -> ContentMutationIter<'a> {
        ContentMutationIter {
            cursor: ReadCursor::new(self.entries),
            remaining: self.entry_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountMutationEntry<'a> {
    pub key: &'a [u8],
    pub transition: CountTransition,
}

pub struct CountMutationIter<'a> {
    cursor: ReadCursor<'a>,
    remaining: u64,
}

impl<'a> Iterator for CountMutationIter<'a> {
    type Item = CountMutationEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            debug_assert_eq!(self.cursor.remaining(), 0);
            return None;
        }
        self.remaining -= 1;
        let key_len = self
            .cursor
            .read_len("validated mutation key")
            .expect("the mutation view validated every key length");
        let key = self
            .cursor
            .read_exact(key_len)
            .expect("the mutation view validated every key extent");
        let transition = CountTransition::new(
            self.cursor.read_u64().expect("validated count before"),
            self.cursor.read_u64().expect("validated count after"),
        )
        .expect("the mutation view rejected count no-ops");
        Some(CountMutationEntry { key, transition })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.remaining)
            .expect("a validated entry count is bounded by its in-memory byte slice");
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CountMutationIter<'_> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentMutationEntry<'a> {
    pub key: &'a [u8],
    pub transition: ContentTransition,
}

pub struct ContentMutationIter<'a> {
    cursor: ReadCursor<'a>,
    remaining: u64,
}

impl<'a> Iterator for ContentMutationIter<'a> {
    type Item = ContentMutationEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            debug_assert_eq!(self.cursor.remaining(), 0);
            return None;
        }
        self.remaining -= 1;
        let key_len = self
            .cursor
            .read_len("validated mutation key")
            .expect("the mutation view validated every key length");
        let key = self
            .cursor
            .read_exact(key_len)
            .expect("the mutation view validated every key extent");
        let transition = ContentTransition::new(
            read_optional_ref(&mut self.cursor).expect("validated content before"),
            read_optional_ref(&mut self.cursor).expect("validated content after"),
        )
        .expect("the mutation view rejected content no-ops");
        Some(ContentMutationEntry { key, transition })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.remaining)
            .expect("a validated entry count is bounded by its in-memory byte slice");
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ContentMutationIter<'_> {}

/// A stable CoW snapshot whose bytes can be streamed directly to a WAL sink.
#[derive(Clone)]
pub struct MutationBatchPlanV1 {
    base: StateVersionV1,
    target_generation: u64,
    data: PathMap<CountTransition>,
    continuations: PathMap<ContentTransition>,
    joins: PathMap<CountTransition>,
    data_entries: u64,
    continuation_entries: u64,
    join_entries: u64,
    body_len: u64,
    canonical_len: u64,
}

impl MutationBatchPlanV1 {
    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn base(&self) -> StateVersionV1 { self.base }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn target_generation(&self) -> u64 { self.target_generation }

    /// Digest used as the WAL transaction identifier.  This is not a
    /// semantic-object reference: the immutable object store intentionally
    /// does not admit mutation payloads as DAG closure objects.
    pub fn payload_digest(&self) -> ObjectDigest { self.canonical_digest() }
}

impl CanonicalEncode for MutationBatchPlanV1 {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, MUTATION_DOMAIN, self.body_len)?;
        write_u64(sink, MUTATION_SCHEMA_VERSION_V1)?;
        write_u64(sink, self.base.generation)?;
        write_u64(sink, self.target_generation)?;
        self.base.manifest.write(sink)?;
        write_u64(sink, MUTATION_SECTION_COUNT)?;
        write_count_section(sink, SemanticNamespace::DataBag, self.data_entries, &self.data)?;
        write_content_section(sink, self.continuation_entries, &self.continuations)?;
        write_count_section(sink, SemanticNamespace::JoinBag, self.join_entries, &self.joins)
    }
}

fn record_count(
    map: &mut PathMap<CountTransition>,
    path: &[u8],
    before: u64,
    after: u64,
) -> Result<(), MutationError> {
    match map.get(path).copied() {
        Some(existing) => {
            if existing.after != before {
                return Err(MutationError::DiscontinuousCount {
                    expected: existing.after,
                    actual: before,
                });
            }
            if existing.before == after {
                map.remove(path);
            } else {
                map.insert(path, CountTransition::new(existing.before, after)?);
            }
        }
        None if before != after => {
            map.insert(path, CountTransition::new(before, after)?);
        }
        None => {}
    }
    Ok(())
}

fn record_content(
    map: &mut PathMap<ContentTransition>,
    path: &[u8],
    before: Option<ObjectRefV1>,
    after: Option<ObjectRefV1>,
) -> Result<(), MutationError> {
    match map.get(path).copied() {
        Some(existing) => {
            if existing.after != before {
                return Err(MutationError::DiscontinuousContent {
                    expected: existing.after,
                    actual: before,
                });
            }
            if existing.before == after {
                map.remove(path);
            } else {
                map.insert(path, ContentTransition::new(existing.before, after)?);
            }
        }
        None if before != after => {
            map.insert(path, ContentTransition::new(before, after)?);
        }
        None => {}
    }
    Ok(())
}

fn precede_count(
    map: &mut PathMap<CountTransition>,
    path: &[u8],
    before: u64,
    after: u64,
) -> Result<(), MutationError> {
    match map.get(path).copied() {
        Some(existing) => {
            if existing.before != after {
                return Err(MutationError::DiscontinuousCount {
                    expected: after,
                    actual: existing.before,
                });
            }
            if existing.after == before {
                map.remove(path);
            } else {
                map.insert(path, CountTransition::new(before, existing.after)?);
            }
        }
        None if before != after => {
            map.insert(path, CountTransition::new(before, after)?);
        }
        None => {}
    }
    Ok(())
}

fn precede_content(
    map: &mut PathMap<ContentTransition>,
    path: &[u8],
    before: Option<ObjectRefV1>,
    after: Option<ObjectRefV1>,
) -> Result<(), MutationError> {
    match map.get(path).copied() {
        Some(existing) => {
            if existing.before != after {
                return Err(MutationError::DiscontinuousContent {
                    expected: after,
                    actual: existing.before,
                });
            }
            if existing.after == before {
                map.remove(path);
            } else {
                map.insert(path, ContentTransition::new(before, existing.after)?);
            }
        }
        None if before != after => {
            map.insert(path, ContentTransition::new(before, after)?);
        }
        None => {}
    }
    Ok(())
}

fn endpoint_count(
    namespace: SemanticNamespace,
    endpoint: EndpointValue,
) -> Result<u64, MutationError> {
    match endpoint {
        EndpointValue::Absent => Ok(0),
        EndpointValue::Multiplicity(count) => Ok(count.get()),
        EndpointValue::ValueFree | EndpointValue::Unit | EndpointValue::Content(_) => {
            Err(CanonicalFormatError::EndpointPolicy {
                namespace: namespace.name(),
                endpoint: endpoint.name(),
            }
            .into())
        }
    }
}

fn endpoint_content(
    namespace: SemanticNamespace,
    endpoint: EndpointValue,
) -> Result<Option<ObjectRefV1>, MutationError> {
    match endpoint {
        EndpointValue::Absent => Ok(None),
        EndpointValue::Content(reference) => Ok(Some(reference)),
        EndpointValue::ValueFree | EndpointValue::Unit | EndpointValue::Multiplicity(_) => {
            Err(CanonicalFormatError::EndpointPolicy {
                namespace: namespace.name(),
                endpoint: endpoint.name(),
            }
            .into())
        }
    }
}

fn validate_key(namespace: SemanticNamespace, path: &[u8]) -> Result<(), MutationError> {
    let result = match namespace {
        SemanticNamespace::DataBag => RhoPathV1::parse_datum(path).map(|_| ()),
        SemanticNamespace::ContinuationMap => RhoPathV1::parse_continuation(path).map(|_| ()),
        SemanticNamespace::JoinBag => RhoPathV1::parse_join(path).map(|_| ()),
    };
    result.map_err(|source| MutationError::InvalidKey { namespace, source })
}

fn try_visit<T, E>(
    map: &PathMap<T>,
    mut visitor: impl FnMut(&[u8], &T) -> Result<(), E>,
) -> Result<(), E>
where
    T: Clone + Send + Sync + Unpin,
{
    let mut cursor = map.read_zipper_at_borrowed_path(&[]);
    if let Some(value) = cursor.val() {
        visitor(cursor.path(), value)?;
    }
    while let Some(value) = cursor.to_next_get_val() {
        visitor(cursor.path(), value)?;
    }
    Ok(())
}

fn count_section_metrics(map: &PathMap<CountTransition>) -> Result<(u64, u64), MutationError> {
    let mut count = 0u64;
    let mut len = 0u64;
    try_visit(map, |path, _| {
        count = count
            .checked_add(1)
            .ok_or(CanonicalFormatError::CounterOverflow("mutation entries"))?;
        let path_len = checked_len(path.len(), "mutation key")?;
        len = checked_sum(len, 8 + 8 + 8, "count mutation entries")?;
        len = checked_sum(len, path_len, "count mutation entries")?;
        Ok::<(), CanonicalFormatError>(())
    })?;
    Ok((count, len))
}

fn content_section_metrics(map: &PathMap<ContentTransition>) -> Result<(u64, u64), MutationError> {
    let mut count = 0u64;
    let mut len = 0u64;
    try_visit(map, |path, transition| {
        count = count
            .checked_add(1)
            .ok_or(CanonicalFormatError::CounterOverflow("mutation entries"))?;
        let path_len = checked_len(path.len(), "mutation key")?;
        let refs = u64::from(transition.before.is_some()) + u64::from(transition.after.is_some());
        len = checked_sum(len, 8 + 1 + 1, "content mutation entries")?;
        len = checked_sum(len, path_len, "content mutation entries")?;
        len = checked_sum(
            len,
            refs.checked_mul(OBJECT_REF_LEN)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "content mutation references",
                })?,
            "content mutation entries",
        )?;
        Ok::<(), CanonicalFormatError>(())
    })?;
    Ok((count, len))
}

fn write_count_section<S: CanonicalSink>(
    sink: &mut S,
    namespace: SemanticNamespace,
    entry_count: u64,
    map: &PathMap<CountTransition>,
) -> Result<(), S::Error> {
    write_u64(sink, namespace as u64)?;
    write_u64(sink, entry_count)?;
    try_visit(map, |path, transition| {
        write_u64(sink, path.len() as u64)?;
        sink.write_all(path)?;
        write_u64(sink, transition.before)?;
        write_u64(sink, transition.after)
    })
}

fn write_content_section<S: CanonicalSink>(
    sink: &mut S,
    entry_count: u64,
    map: &PathMap<ContentTransition>,
) -> Result<(), S::Error> {
    write_u64(sink, SemanticNamespace::ContinuationMap as u64)?;
    write_u64(sink, entry_count)?;
    try_visit(map, |path, transition| {
        write_u64(sink, path.len() as u64)?;
        sink.write_all(path)?;
        write_optional_ref(sink, transition.before)?;
        write_optional_ref(sink, transition.after)
    })
}

fn write_optional_ref<S: CanonicalSink>(
    sink: &mut S,
    reference: Option<ObjectRefV1>,
) -> Result<(), S::Error> {
    match reference {
        None => sink.write_all(&[0]),
        Some(reference) => {
            sink.write_all(&[1])?;
            reference.write(sink)
        }
    }
}

fn read_count_section<'a>(
    body: &'a [u8],
    cursor: &mut ReadCursor<'a>,
    namespace: SemanticNamespace,
    limits: MutationDecodeLimits,
    total_entries: &mut u64,
    total_key_bytes: &mut u64,
) -> Result<CountSectionView<'a>, MutationError> {
    parse_section_tag(cursor, namespace)?;
    let entry_count = cursor.read_u64()?;
    account_entries(entry_count, limits, total_entries)?;
    let entries_start = cursor.offset();
    let mut previous = None;
    for _ in 0..entry_count {
        let key = read_key(cursor, namespace, limits, total_key_bytes)?;
        require_strict_key(namespace, previous, key)?;
        previous = Some(key);
        CountTransition::new(cursor.read_u64()?, cursor.read_u64()?)?;
    }
    Ok(CountSectionView {
        entry_count,
        entries: &body[entries_start..cursor.offset()],
    })
}

fn read_content_section<'a>(
    body: &'a [u8],
    cursor: &mut ReadCursor<'a>,
    limits: MutationDecodeLimits,
    total_entries: &mut u64,
    total_key_bytes: &mut u64,
) -> Result<ContentSectionView<'a>, MutationError> {
    let namespace = SemanticNamespace::ContinuationMap;
    parse_section_tag(cursor, namespace)?;
    let entry_count = cursor.read_u64()?;
    account_entries(entry_count, limits, total_entries)?;
    let entries_start = cursor.offset();
    let mut previous = None;
    for _ in 0..entry_count {
        let key = read_key(cursor, namespace, limits, total_key_bytes)?;
        require_strict_key(namespace, previous, key)?;
        previous = Some(key);
        ContentTransition::new(read_optional_ref(cursor)?, read_optional_ref(cursor)?)?;
    }
    Ok(ContentSectionView {
        entry_count,
        entries: &body[entries_start..cursor.offset()],
    })
}

fn parse_section_tag(
    cursor: &mut ReadCursor<'_>,
    namespace: SemanticNamespace,
) -> Result<(), MutationError> {
    let actual = cursor.read_u64()?;
    if actual != namespace as u64 {
        return Err(MutationError::SectionOrder {
            expected: namespace as u64,
            actual,
        });
    }
    Ok(())
}

fn account_entries(
    count: u64,
    limits: MutationDecodeLimits,
    total: &mut u64,
) -> Result<(), MutationError> {
    let attempted = total
        .checked_add(count)
        .ok_or(CanonicalFormatError::CounterOverflow("mutation entries"))?;
    if attempted > limits.max_entries {
        return Err(CanonicalFormatError::ResourceLimit {
            resource: "mutation entries",
            limit: limits.max_entries,
            attempted,
        }
        .into());
    }
    *total = attempted;
    Ok(())
}

fn read_key<'a>(
    cursor: &mut ReadCursor<'a>,
    namespace: SemanticNamespace,
    limits: MutationDecodeLimits,
    total_key_bytes: &mut u64,
) -> Result<&'a [u8], MutationError> {
    let len = cursor.read_len("mutation key")?;
    let len_u64 = checked_len(len, "mutation key")?;
    let attempted = total_key_bytes
        .checked_add(len_u64)
        .ok_or(CanonicalFormatError::CounterOverflow("mutation key bytes"))?;
    if attempted > limits.max_key_bytes {
        return Err(CanonicalFormatError::ResourceLimit {
            resource: "mutation key bytes",
            limit: limits.max_key_bytes,
            attempted,
        }
        .into());
    }
    let key = cursor.read_exact(len)?;
    validate_key(namespace, key)?;
    *total_key_bytes = attempted;
    Ok(key)
}

fn require_strict_key(
    namespace: SemanticNamespace,
    previous: Option<&[u8]>,
    current: &[u8],
) -> Result<(), MutationError> {
    if previous.is_some_and(|previous| previous >= current) {
        return Err(MutationError::KeysNotStrict { namespace });
    }
    Ok(())
}

fn read_optional_ref(cursor: &mut ReadCursor<'_>) -> Result<Option<ObjectRefV1>, MutationError> {
    match cursor.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(ObjectRefV1::parse(cursor)?)),
        tag => Err(MutationError::InvalidPresenceTag(tag)),
    }
}

#[cfg(test)]
mod bag_delta_tests {
    use super::*;
    use crate::rspace::bag::Multiplicity;

    fn state() -> StateVersionV1 {
        let digest = ObjectDigest::from_array([7; OBJECT_DIGEST_LEN]);
        let manifest = ObjectRefV1::new(digest, OBJECT_ENVELOPE_HEADER_LEN).unwrap();
        StateVersionV1::new(7, manifest)
    }

    fn key(label: &[u8]) -> Vec<u8> { RhoPathV1::datum_key(&7_u64, b"shape", label) }

    #[test]
    fn conversion_emits_exact_authoritative_endpoints() {
        let path = key(b"delta");
        let mut base = Bag::new();
        base.insert_many(&path, Multiplicity::new(2).unwrap())
            .unwrap();
        let mut delta = BagDelta::new();
        delta
            .record_addition(&path, Multiplicity::new(3).unwrap())
            .unwrap();

        let mut batch = RSpaceMutationBatchV1::new(state()).unwrap();
        batch.record_data_delta(&base, &delta).unwrap();
        let transition = batch.data_transition(&path).unwrap();
        assert_eq!(transition.before(), 2);
        assert_eq!(transition.after(), 5);
    }

    #[test]
    fn conversion_rejects_removal_beyond_authoritative_multiplicity() {
        let path = key(b"underflow");
        let mut delta = BagDelta::new();
        delta
            .record_removal(&path, Multiplicity::new(1).unwrap())
            .unwrap();

        let mut batch = RSpaceMutationBatchV1::new(state()).unwrap();
        assert_eq!(
            batch.record_data_delta(&Bag::new(), &delta),
            Err(MutationError::DeltaCountUnderflow)
        );
    }

    #[test]
    fn continuation_blob_conversion_records_only_the_canonical_reference() {
        let path = RhoPathV1::collection_record_path(
            &RhoPathV1::continuation_prefix(&[7_u64, 8_u64]),
            b"identity",
        );
        let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, b"source".to_vec())
            .unwrap();
        let expected = blob.object_ref();
        let mut batch = RSpaceMutationBatchV1::new(state()).unwrap();
        batch
            .record_continuation_blob(&path, None, Some(&blob))
            .unwrap();
        assert_eq!(batch.continuation_transition(&path).unwrap().after(), Some(expected));
    }

    #[test]
    fn normalized_continuation_trie_conversion_preserves_content_endpoints() {
        let path = RhoPathV1::collection_record_path(
            &RhoPathV1::continuation_prefix(&[7_u64]),
            b"identity",
        );
        let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, b"source".to_vec())
            .unwrap();
        let transition = ContentTransition::new(None, Some(blob.object_ref())).unwrap();
        let mut transitions = PathMap::new();
        transitions.insert(path.clone(), transition);

        let mut batch = RSpaceMutationBatchV1::new(state()).unwrap();
        batch.record_continuation_transitions(&transitions).unwrap();
        assert_eq!(batch.continuation_transition(&path).unwrap(), transition);
    }

    #[test]
    fn join_conversion_uses_the_join_namespace() {
        let mut path = RhoPathV1::join_prefix(&7_u64);
        path.extend_from_slice(&RhoPathV1::collection_record_path(&[], b"join"));
        let mut base = Bag::new();
        base.insert_many(&path, Multiplicity::new(1).unwrap())
            .unwrap();
        let mut delta = BagDelta::new();
        delta
            .record_addition(&path, Multiplicity::new(2).unwrap())
            .unwrap();

        let mut batch = RSpaceMutationBatchV1::new(state()).unwrap();
        batch.record_join_delta(&base, &delta).unwrap();
        assert_eq!(batch.join_transition(&path).unwrap().before(), 1);
        assert_eq!(batch.join_transition(&path).unwrap().after(), 3);
        assert!(batch.data_transition(&path).is_none());
    }
}
