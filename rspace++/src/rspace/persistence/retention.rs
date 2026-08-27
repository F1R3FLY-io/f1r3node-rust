//! Canonical retained-root authority for bounded durable compaction.
//!
//! The manifest names semantic state roots, not pack offsets. Compaction can
//! therefore rewrite their shared Merkle closure into a new pack generation
//! without changing logical state identities. Decoding is borrowed and
//! streaming; no root projection is required.

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::mutation::StateVersionV1;

pub(crate) const RETENTION_MANIFEST_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Retn\0\0";
const STATE_ENTRY_LEN: u64 = 8 + 32 + 8 + 8;

/// One semantic state root and the deterministic size of its reachable,
/// deduplicated canonical closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedStateV1 {
    state: StateVersionV1,
    canonical_bytes: u64,
}

impl RetainedStateV1 {
    pub fn new(state: StateVersionV1, canonical_bytes: u64) -> Result<Self, CanonicalFormatError> {
        if canonical_bytes < state.manifest().encoded_len() {
            return Err(CanonicalFormatError::RetentionStateBytes {
                generation: state.generation(),
                minimum: state.manifest().encoded_len(),
                actual: canonical_bytes,
            });
        }
        Ok(Self {
            state,
            canonical_bytes,
        })
    }

    pub const fn state(self) -> StateVersionV1 { self.state }

    pub const fn canonical_bytes(self) -> u64 { self.canonical_bytes }

    fn write<S: CanonicalSink>(self, sink: &mut S) -> Result<(), S::Error> {
        write_u64(sink, self.state.generation())?;
        self.state.manifest().write(sink)?;
        write_u64(sink, self.canonical_bytes)
    }

    fn parse(cursor: &mut ReadCursor<'_>) -> Result<Self, CanonicalFormatError> {
        let state = StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(cursor)?);
        Self::new(state, cursor.read_u64()?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionManifestV1 {
    current: RetainedStateV1,
    historical: Box<[RetainedStateV1]>,
    body_len: u64,
    canonical_len: u64,
}

impl RetentionManifestV1 {
    pub fn new(
        current: RetainedStateV1,
        historical: impl Into<Box<[RetainedStateV1]>>,
    ) -> Result<Self, CanonicalFormatError> {
        let historical = historical.into();
        validate_historical(current, &historical)?;
        let count = checked_len(historical.len(), "retained historical states")?;
        let entries =
            count
                .checked_mul(STATE_ENTRY_LEN)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "retained historical states",
                })?;
        let body_len = checked_sum(STATE_ENTRY_LEN + 8, entries, "retention manifest body")?;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "retention manifest envelope")?;
        Ok(Self {
            current,
            historical,
            body_len,
            canonical_len,
        })
    }

    pub const fn current(&self) -> RetainedStateV1 { self.current }

    pub fn historical(&self) -> &[RetainedStateV1] { &self.historical }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated retention-manifest length includes its envelope")
    }
}

impl CanonicalEncode for RetentionManifestV1 {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, RETENTION_MANIFEST_DOMAIN, self.body_len)?;
        self.current.write(sink)?;
        write_u64(sink, self.historical.len() as u64)?;
        for retained in &self.historical {
            retained.write(sink)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RetentionManifestView<'a> {
    bytes: &'a [u8],
    current: RetainedStateV1,
    historical: &'a [u8],
    historical_count: usize,
}

impl<'a> RetentionManifestView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        let body = parse_envelope(bytes, RETENTION_MANIFEST_DOMAIN, "RSpace retention manifest")?;
        let mut cursor = ReadCursor::new(body);
        let current = RetainedStateV1::parse(&mut cursor)?;
        let count_u64 = cursor.read_u64()?;
        let count =
            usize::try_from(count_u64).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "retained historical states",
                value: count_u64,
            })?;
        let byte_len = count.checked_mul(STATE_ENTRY_LEN as usize).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "retained historical states",
            },
        )?;
        let historical = cursor.read_exact(byte_len)?;
        cursor.finish()?;

        let mut parsed = RetainedStateIter {
            cursor: ReadCursor::new(historical),
            remaining: count,
        };
        let mut previous = None;
        for retained in &mut parsed {
            let retained = retained?;
            if retained.state() == current.state() {
                return Err(CanonicalFormatError::RetentionContainsCurrent);
            }
            if previous.is_some_and(|previous| !state_precedes(previous, retained.state())) {
                return Err(CanonicalFormatError::RetainedStatesNotStrict);
            }
            previous = Some(retained.state());
        }

        Ok(Self {
            bytes,
            current,
            historical,
            historical_count: count,
        })
    }

    pub const fn current(self) -> RetainedStateV1 { self.current }

    pub fn historical(self) -> RetainedStateIter<'a> {
        RetainedStateIter {
            cursor: ReadCursor::new(self.historical),
            remaining: self.historical_count,
        }
    }

    pub const fn canonical_bytes(self) -> &'a [u8] { self.bytes }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(self.bytes), self.bytes.len() as u64)
            .expect("parsed retention-manifest length includes its envelope")
    }
}

pub struct RetainedStateIter<'a> {
    cursor: ReadCursor<'a>,
    remaining: usize,
}

impl Iterator for RetainedStateIter<'_> {
    type Item = Result<RetainedStateV1, CanonicalFormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            debug_assert_eq!(self.cursor.remaining(), 0);
            return None;
        }
        self.remaining -= 1;
        Some(RetainedStateV1::parse(&mut self.cursor))
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (self.remaining, Some(self.remaining)) }
}

impl ExactSizeIterator for RetainedStateIter<'_> {}

fn validate_historical(
    current: RetainedStateV1,
    historical: &[RetainedStateV1],
) -> Result<(), CanonicalFormatError> {
    let mut previous = None;
    for retained in historical {
        if retained.state() == current.state() {
            return Err(CanonicalFormatError::RetentionContainsCurrent);
        }
        if previous.is_some_and(|previous| !state_precedes(previous, retained.state())) {
            return Err(CanonicalFormatError::RetainedStatesNotStrict);
        }
        previous = Some(retained.state());
    }
    Ok(())
}

fn state_precedes(left: StateVersionV1, right: StateVersionV1) -> bool {
    (left.generation(), left.manifest()) < (right.generation(), right.manifest())
}

const _: () = assert!(STATE_ENTRY_LEN == 56);
