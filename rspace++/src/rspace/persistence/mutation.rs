//! Canonical, normalized RSpace state transitions.
//!
//! Arithmetic and first-wins operations are the resident producer semantics.
//! The durable form is deliberately stronger: every changed path carries its
//! exact before and after state.  That makes stale-base replay observable,
//! already-applied replay idempotent, and the live CoW and persistent-DAG
//! applicators consumers of one transition language.

use std::fmt::{Display, Formatter};

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::manifest::SemanticNamespace;
use crate::rspace::rho_path::{RhoPathError, RhoPathV1};

const MUTATION_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Mut\0\0\0";
const MUTATION_SCHEMA_VERSION_V1: u64 = 1;
const MUTATION_SECTION_COUNT: u64 = 3;
const OBJECT_REF_LEN: u64 = 32 + 8;

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
            base,
            target_generation,
            data,
            continuations,
            joins,
        })
    }

    pub const fn base(self) -> StateVersionV1 { self.base }

    pub const fn target_generation(self) -> u64 { self.target_generation }

    pub fn data(self) -> CountMutationIter<'a> { self.data.iter() }

    pub fn continuations(self) -> ContentMutationIter<'a> { self.continuations.iter() }

    pub fn joins(self) -> CountMutationIter<'a> { self.joins.iter() }
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
    pub const fn base(&self) -> StateVersionV1 { self.base }

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
