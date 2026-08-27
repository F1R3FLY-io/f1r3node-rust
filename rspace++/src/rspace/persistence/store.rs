use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::object::CanonicalObjectView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectAdmission {
    Inserted,
    AlreadyPresent,
}

/// Backend-neutral immutable-object operations required by closure validation
/// and later pack/WAL adapters.
pub trait ImmutableObjectStore: Send + Sync {
    type Error;
    type Object: AsRef<[u8]>;

    fn get(&self, digest: &ObjectDigest) -> Result<Option<Self::Object>, Self::Error>;

    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error>;
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

    fn get(&self, digest: &ObjectDigest) -> Result<Option<Arc<[u8]>>, Self::Error> {
        Ok(self
            .objects
            .read()
            .map_err(|_| CanonicalFormatError::ObjectStorePoisoned)?
            .get(digest)
            .cloned())
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
