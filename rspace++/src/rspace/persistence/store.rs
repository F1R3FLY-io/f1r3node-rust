use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::object::{CanonicalObjectView, validate_canonical_object_for};
use super::retention::v2_format::RetentionReader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectAdmission {
    Inserted,
    AlreadyPresent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagingStoreLimits {
    pub max_objects: u64,
    pub max_canonical_bytes: u64,
}

impl StagingStoreLimits {
    pub const UNBOUNDED: Self = Self {
        max_objects: u64::MAX,
        max_canonical_bytes: u64::MAX,
    };
}

impl Default for StagingStoreLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagingStoreError<E> {
    Base(E),
    Format(CanonicalFormatError),
}

impl<E: std::fmt::Display> std::fmt::Display for StagingStoreError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base(error) => write!(formatter, "base object-store failure: {error}"),
            Self::Format(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for StagingStoreError<E> {}

impl<E> From<CanonicalFormatError> for StagingStoreError<E> {
    fn from(error: CanonicalFormatError) -> Self { Self::Format(error) }
}

/// Backend-neutral immutable-object operations required by closure validation
/// and later pack/WAL adapters.
pub trait ImmutableObjectStore: Send + Sync {
    type Error;
    type Object: AsRef<[u8]>;

    /// Resolve an object without materializing more than `max_encoded_len`
    /// bytes. Durable backends must inspect record metadata before allocating
    /// or reading the payload.
    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error>;

    fn get(&self, digest: &ObjectDigest) -> Result<Option<Self::Object>, Self::Error> {
        self.get_bounded(digest, u64::MAX)
    }
}

/// Private/in-memory admission capability. Durable object readers deliberately
/// implement only [`ImmutableObjectStore`]; publication is the sole path that
/// may append canonical objects and advance authority.
pub trait MutableObjectStore: ImmutableObjectStore {
    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error>;
}

/// An immutable lookup result borrowed from either the durable base or the
/// transaction-private staging layer.
pub enum LayeredObject<O> {
    Base(O),
    Staged(Arc<[u8]>),
}

impl<O: AsRef<[u8]>> AsRef<[u8]> for LayeredObject<O> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Base(object) => object.as_ref(),
            Self::Staged(object) => object.as_ref(),
        }
    }
}

/// A private content-addressed overlay used while compiling and validating a
/// multi-object DAG transition.
///
/// Staging never calls the base store's admission method.  A failed rewrite
/// can therefore be discarded without exposing a prefix of its object graph.
/// Staged bytes are fully parsed for the overlay's retention reader, which is
/// V1-only unless a retention V2 writer selects the dual reader.
pub struct StagingObjectStore<'a, S: ImmutableObjectStore> {
    base: &'a S,
    limits: StagingStoreLimits,
    reader: RetentionReader,
    state: RwLock<StagingState>,
}

#[derive(Default)]
struct StagingState {
    canonical_bytes: u64,
    objects: BTreeMap<ObjectDigest, Arc<[u8]>>,
}

impl<'a, S: ImmutableObjectStore> StagingObjectStore<'a, S> {
    pub fn new(base: &'a S) -> Self { Self::with_limits(base, StagingStoreLimits::UNBOUNDED) }

    pub fn with_limits(base: &'a S, limits: StagingStoreLimits) -> Self {
        Self {
            base,
            limits,
            reader: RetentionReader::V1Only,
            state: RwLock::new(StagingState::default()),
        }
    }

    /// Admit the retention metadata domains a reader with this capability
    /// accepts. Only the retention V2 writer selects the dual reader.
    pub(crate) const fn with_retention_reader(mut self, reader: RetentionReader) -> Self {
        self.reader = reader;
        self
    }

    pub fn stage(
        &self,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<(ObjectRefV1, ObjectAdmission), StagingStoreError<S::Error>> {
        validate_canonical_object_for(&canonical_bytes, self.reader)?;
        self.stage_prevalidated(canonical_bytes)
    }

    pub(super) fn stage_prevalidated(
        &self,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<(ObjectRefV1, ObjectAdmission), StagingStoreError<S::Error>> {
        let digest = ObjectDigest::of_canonical_bytes(&canonical_bytes);
        let encoded_len = checked_len(canonical_bytes.len(), "canonical object")?;
        let reference = ObjectRefV1::new(digest, encoded_len)?;
        let admission = self.insert_or_compare(reference, canonical_bytes)?;
        Ok((reference, admission))
    }

    pub fn len(&self) -> Result<usize, StagingStoreError<S::Error>> {
        Ok(self
            .state
            .read()
            .map_err(|_| StagingStoreError::Format(CanonicalFormatError::ObjectStorePoisoned))?
            .objects
            .len())
    }

    pub(super) fn staged_references(
        &self,
    ) -> Result<Box<[ObjectRefV1]>, StagingStoreError<S::Error>> {
        let state = self
            .state
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let requested = checked_len(state.objects.len(), "staged object references")?;
        let mut references = Vec::new();
        references
            .try_reserve_exact(state.objects.len())
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested })?;
        for (digest, bytes) in &state.objects {
            references
                .push(ObjectRefV1::new(*digest, checked_len(bytes.len(), "canonical object")?)?);
        }
        Ok(references.into_boxed_slice())
    }

    /// Test inspection owns Arc handles and releases the staging lock before
    /// returning.  Production code consumes the overlay with `into_objects`;
    /// it never invokes a sink or callback while a lock guard is live.
    #[cfg(test)]
    pub(crate) fn staged_snapshot(
        &self,
    ) -> Result<Box<[(ObjectRefV1, Arc<[u8]>)]>, StagingStoreError<S::Error>> {
        let state = self
            .state
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let requested = checked_len(state.objects.len(), "staged snapshot objects")?;
        let mut snapshot = Vec::new();
        snapshot
            .try_reserve_exact(state.objects.len())
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested })?;
        for (digest, bytes) in &state.objects {
            let encoded_len = checked_len(bytes.len(), "canonical object")?;
            snapshot.push((ObjectRefV1::new(*digest, encoded_len)?, Arc::clone(bytes)));
        }
        Ok(snapshot.into_boxed_slice())
    }

    /// Consume the mutable overlay into an immutable digest-ordered batch.
    /// No lock, durable-store reference, or visitor callback escapes sealing.
    pub(super) fn into_objects<T>(
        self,
        mut map: impl FnMut(ObjectRefV1, Arc<[u8]>) -> T,
    ) -> Result<Box<[T]>, StagingStoreError<S::Error>> {
        let state = self
            .state
            .into_inner()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let mut objects = Vec::new();
        let requested = checked_len(state.objects.len(), "staged objects")?;
        objects
            .try_reserve_exact(state.objects.len())
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested })?;
        for (digest, bytes) in state.objects {
            let encoded_len = checked_len(bytes.len(), "canonical object")?;
            objects.push(map(ObjectRefV1::new(digest, encoded_len)?, bytes));
        }
        Ok(objects.into_boxed_slice())
    }

    fn insert_or_compare(
        &self,
        reference: ObjectRefV1,
        bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, StagingStoreError<S::Error>> {
        if let Some(existing) =
            self.get_staged_bounded(&reference.digest(), reference.encoded_len())?
        {
            return if existing.as_ref() == bytes.as_ref() {
                Ok(ObjectAdmission::AlreadyPresent)
            } else {
                Err(CanonicalFormatError::ContentCollision(reference.digest()).into())
            };
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        match state.objects.get(&reference.digest()) {
            Some(existing) if existing.as_ref() == bytes.as_ref() => {
                Ok(ObjectAdmission::AlreadyPresent)
            }
            Some(_) => Err(CanonicalFormatError::ContentCollision(reference.digest()).into()),
            None => {
                let object_count = checked_len(state.objects.len(), "staged objects")?
                    .checked_add(1)
                    .ok_or(CanonicalFormatError::CounterOverflow("staged objects"))?;
                require_limit("staged objects", self.limits.max_objects, object_count)?;
                let canonical_bytes = state
                    .canonical_bytes
                    .checked_add(reference.encoded_len())
                    .ok_or(CanonicalFormatError::CounterOverflow("staged canonical bytes"))?;
                require_limit(
                    "staged canonical bytes",
                    self.limits.max_canonical_bytes,
                    canonical_bytes,
                )?;
                state.objects.insert(reference.digest(), bytes);
                state.canonical_bytes = canonical_bytes;
                Ok(ObjectAdmission::Inserted)
            }
        }
    }

    pub(super) fn get_staged_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Arc<[u8]>>, StagingStoreError<S::Error>> {
        let state = self
            .state
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let Some(object) = state.objects.get(digest) else {
            return Ok(None);
        };
        let attempted = checked_len(object.len(), "staged object")?;
        if attempted > max_encoded_len {
            return Err(CanonicalFormatError::ResourceLimit {
                resource: "materialized object bytes",
                limit: max_encoded_len,
                attempted,
            }
            .into());
        }
        Ok(Some(Arc::clone(object)))
    }
}

fn require_limit(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), CanonicalFormatError> {
    if attempted > limit {
        Err(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        })
    } else {
        Ok(())
    }
}

impl<S: ImmutableObjectStore> ImmutableObjectStore for StagingObjectStore<'_, S> {
    type Error = StagingStoreError<S::Error>;
    type Object = LayeredObject<S::Object>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        if let Some(object) = self.get_staged_bounded(digest, max_encoded_len)? {
            return Ok(Some(LayeredObject::Staged(object)));
        }
        self.base
            .get_bounded(digest, max_encoded_len)
            .map(|object| object.map(LayeredObject::Base))
            .map_err(StagingStoreError::Base)
    }
}

impl<S: ImmutableObjectStore> MutableObjectStore for StagingObjectStore<'_, S> {
    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error> {
        let actual_len = checked_len(canonical_bytes.len(), "canonical object")?;
        if actual_len != reference.encoded_len() {
            return Err(CanonicalFormatError::ObjectLengthMismatch {
                expected: reference.encoded_len(),
                actual: actual_len,
            }
            .into());
        }
        validate_canonical_object_for(&canonical_bytes, self.reader)?;
        let actual = ObjectDigest::of_canonical_bytes(&canonical_bytes);
        if actual != reference.digest() {
            return Err(CanonicalFormatError::DigestMismatch {
                expected: reference.digest(),
                actual,
            }
            .into());
        }
        self.insert_or_compare(reference, canonical_bytes)
    }
}

/// In-memory collision-checking implementation used by the pure format layer
/// and its differentials. Durable stores must provide the same full-byte
/// compare before publishing an existing digest.
#[derive(Debug, Default)]
pub struct CanonicalObjectStore {
    objects: RwLock<BTreeMap<ObjectDigest, Arc<[u8]>>>,
}

impl CanonicalObjectStore {
    pub fn new() -> Self { Self::default() }

    pub fn admit(
        &self,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<(ObjectRefV1, ObjectAdmission), CanonicalFormatError> {
        let _ = CanonicalObjectView::parse(&canonical_bytes)?;
        let digest = ObjectDigest::of_canonical_bytes(&canonical_bytes);
        let encoded_len = checked_len(canonical_bytes.len(), "canonical object")?;
        let reference = ObjectRefV1::new(digest, encoded_len)?;
        let admission = self.put_if_absent_or_equal(reference, canonical_bytes)?;
        Ok((reference, admission))
    }

    pub fn len(&self) -> Result<usize, CanonicalFormatError> {
        Ok(self
            .objects
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?
            .len())
    }

    pub fn is_empty(&self) -> Result<bool, CanonicalFormatError> { Ok(self.len()? == 0) }

    #[cfg(test)]
    pub(crate) fn put_prehashed_for_test(
        &self,
        digest: ObjectDigest,
        bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, CanonicalFormatError> {
        self.insert_or_compare(digest, bytes)
    }

    fn insert_or_compare(
        &self,
        digest: ObjectDigest,
        bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, CanonicalFormatError> {
        let mut objects = self
            .objects
            .write()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        match objects.get(&digest) {
            Some(existing) if existing.as_ref() == bytes.as_ref() => {
                Ok(ObjectAdmission::AlreadyPresent)
            }
            Some(_) => Err(CanonicalFormatError::ContentCollision(digest)),
            None => {
                objects.insert(digest, bytes);
                Ok(ObjectAdmission::Inserted)
            }
        }
    }
}

impl ImmutableObjectStore for CanonicalObjectStore {
    type Error = CanonicalFormatError;
    type Object = Arc<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Arc<[u8]>>, Self::Error> {
        let objects = self
            .objects
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let Some(object) = objects.get(digest) else {
            return Ok(None);
        };
        let attempted = checked_len(object.len(), "canonical object")?;
        if attempted > max_encoded_len {
            return Err(CanonicalFormatError::ResourceLimit {
                resource: "materialized object bytes",
                limit: max_encoded_len,
                attempted,
            });
        }
        Ok(Some(Arc::clone(object)))
    }
}

impl MutableObjectStore for CanonicalObjectStore {
    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error> {
        let actual_len = checked_len(canonical_bytes.len(), "canonical object")?;
        if actual_len != reference.encoded_len() {
            return Err(CanonicalFormatError::ObjectLengthMismatch {
                expected: reference.encoded_len(),
                actual: actual_len,
            });
        }
        let _ = CanonicalObjectView::parse(&canonical_bytes)?;
        let actual = ObjectDigest::of_canonical_bytes(&canonical_bytes);
        if actual != reference.digest() {
            return Err(CanonicalFormatError::DigestMismatch {
                expected: reference.digest(),
                actual,
            });
        }
        self.insert_or_compare(reference.digest(), canonical_bytes)
    }
}
