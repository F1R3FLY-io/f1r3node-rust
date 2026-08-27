use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::object::CanonicalObjectView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectAdmission {
    Inserted,
    AlreadyPresent,
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
pub struct StagingObjectStore<'a, S: ImmutableObjectStore> {
    base: &'a S,
    objects: RwLock<BTreeMap<ObjectDigest, Arc<[u8]>>>,
}

impl<'a, S: ImmutableObjectStore> StagingObjectStore<'a, S> {
    pub fn new(base: &'a S) -> Self {
        Self {
            base,
            objects: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn stage(
        &self,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<(ObjectRefV1, ObjectAdmission), StagingStoreError<S::Error>> {
        let _ = CanonicalObjectView::parse(&canonical_bytes)?;
        let digest = ObjectDigest::of_canonical_bytes(&canonical_bytes);
        let encoded_len = checked_len(canonical_bytes.len(), "canonical object")?;
        let reference = ObjectRefV1::new(digest, encoded_len)?;
        let admission = self.put_if_absent_or_equal(reference, canonical_bytes)?;
        Ok((reference, admission))
    }

    pub fn len(&self) -> Result<usize, StagingStoreError<S::Error>> {
        Ok(self
            .objects
            .read()
            .map_err(|_| StagingStoreError::Format(CanonicalFormatError::ObjectStorePoisoned))?
            .len())
    }

    pub fn try_visit_staged<E>(
        &self,
        mut visitor: impl FnMut(ObjectRefV1, &[u8]) -> Result<(), E>,
    ) -> Result<(), VisitStagedError<S::Error, E>> {
        let objects = self.objects.read().map_err(|_| {
            VisitStagedError::Store(StagingStoreError::Format(
                CanonicalFormatError::ObjectStorePoisoned,
            ))
        })?;
        for (digest, bytes) in objects.iter() {
            let encoded_len = checked_len(bytes.len(), "canonical object")
                .map_err(|error| VisitStagedError::Store(StagingStoreError::Format(error)))?;
            let reference = ObjectRefV1::new(*digest, encoded_len)
                .map_err(|error| VisitStagedError::Store(StagingStoreError::Format(error)))?;
            visitor(reference, bytes).map_err(VisitStagedError::Visitor)?;
        }
        Ok(())
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
        if let Some(existing) = self
            .base
            .get_bounded(&reference.digest(), reference.encoded_len())
            .map_err(StagingStoreError::Base)?
        {
            return if existing.as_ref() == bytes.as_ref() {
                Ok(ObjectAdmission::AlreadyPresent)
            } else {
                Err(CanonicalFormatError::ContentCollision(reference.digest()).into())
            };
        }

        let mut objects = self
            .objects
            .write()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        match objects.get(&reference.digest()) {
            Some(existing) if existing.as_ref() == bytes.as_ref() => {
                Ok(ObjectAdmission::AlreadyPresent)
            }
            Some(_) => Err(CanonicalFormatError::ContentCollision(reference.digest()).into()),
            None => {
                objects.insert(reference.digest(), bytes);
                Ok(ObjectAdmission::Inserted)
            }
        }
    }

    fn get_staged_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Arc<[u8]>>, StagingStoreError<S::Error>> {
        let objects = self
            .objects
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?;
        let Some(object) = objects.get(digest) else {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisitStagedError<SE, VE> {
    Store(StagingStoreError<SE>),
    Visitor(VE),
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
        let _ = CanonicalObjectView::parse(&canonical_bytes)?;
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
