use std::fmt::{Display, Formatter};

use crate::rspace::persistence::canonical::{
    CanonicalFormatError, ObjectRefV1, checked_len, checked_sum,
};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::transaction::SealedDagTransaction;

pub const OBJECT_DATA_KIND_V1: u64 = 1;
pub const OBJECT_REUSE_KIND_V1: u64 = 2;

/// Physical treatment of one logically sealed canonical object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogPackObjectDispositionV1 {
    /// The object is absent from the committed store and its body is inline.
    Data,
    /// An equal canonical body is already committed; only its content identity
    /// is recorded. No physical address is persisted.
    Reuse,
}

impl LogPackObjectDispositionV1 {
    pub const fn kind(self) -> u64 {
        match self {
            Self::Data => OBJECT_DATA_KIND_V1,
            Self::Reuse => OBJECT_REUSE_KIND_V1,
        }
    }
}

/// Result of comparing one sealed body with the committed object named by its
/// digest. `Equal` requires complete length-and-byte equality, never a digest-
/// only shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommittedObjectComparison {
    Missing,
    Equal,
    Collision,
}

/// Fallible committed-object comparison used before the first append.
pub trait CommittedObjectLookup {
    type Error;

    fn compare_committed_object(
        &self,
        reference: ObjectRefV1,
        sealed_canonical_bytes: &[u8],
    ) -> Result<CommittedObjectComparison, Self::Error>;
}

#[derive(Debug)]
pub enum LogPackObjectPlanError<E> {
    Lookup(E),
    Collision(ObjectRefV1),
    Format(CanonicalFormatError),
}

impl<E: Display> Display for LogPackObjectPlanError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lookup(error) => write!(formatter, "committed-object lookup failed: {error}"),
            Self::Collision(reference) => write!(
                formatter,
                "committed object has digest {} but unequal canonical bytes",
                reference.digest()
            ),
            Self::Format(error) => Display::fmt(error, formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for LogPackObjectPlanError<E> {}

impl<E> From<CanonicalFormatError> for LogPackObjectPlanError<E> {
    fn from(error: CanonicalFormatError) -> Self { Self::Format(error) }
}

/// Immutable, digest-order-preserving physical record plan.
///
/// This plan copies no canonical body and performs no semantic projection or
/// sorting. Its byte inventories deliberately distinguish logical sealed bytes
/// from bytes that this particular warm/cold store must physically inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPackObjectPlanV1 {
    dispositions: Box<[LogPackObjectDispositionV1]>,
    inline_count: u64,
    logical_object_bytes: u64,
    inline_object_bytes: u64,
}

impl LogPackObjectPlanV1 {
    pub fn classify<P, L>(
        transaction: &SealedDagTransaction<'_, P>,
        lookup: &L,
    ) -> Result<Self, LogPackObjectPlanError<L::Error>>
    where
        P: SemanticProfile,
        L: CommittedObjectLookup,
    {
        if transaction.receipt().profile_id() != P::ID {
            return Err(CanonicalFormatError::SemanticProfileMismatch {
                expected: *P::ID.as_bytes(),
                actual: *transaction.receipt().profile_id().as_bytes(),
            }
            .into());
        }
        let count = checked_len(transaction.objects().len(), "WAL object disposition plan")?;
        let mut dispositions = Vec::new();
        dispositions
            .try_reserve_exact(transaction.objects().len())
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested: count })?;
        let mut inline_count = 0u64;
        let mut logical_object_bytes = 0u64;
        let mut inline_object_bytes = 0u64;
        for object in transaction.objects() {
            let reference = object.reference();
            logical_object_bytes = checked_sum(
                logical_object_bytes,
                reference.encoded_len(),
                "WAL logical canonical object bytes",
            )?;
            match lookup
                .compare_committed_object(reference, object.canonical_bytes())
                .map_err(LogPackObjectPlanError::Lookup)?
            {
                CommittedObjectComparison::Missing => {
                    dispositions.push(LogPackObjectDispositionV1::Data);
                    inline_count = inline_count.checked_add(1).ok_or(
                        CanonicalFormatError::LengthOverflow {
                            field: "WAL inline object count",
                        },
                    )?;
                    inline_object_bytes = checked_sum(
                        inline_object_bytes,
                        reference.encoded_len(),
                        "WAL inline canonical object bytes",
                    )?;
                }
                CommittedObjectComparison::Equal => {
                    dispositions.push(LogPackObjectDispositionV1::Reuse);
                }
                CommittedObjectComparison::Collision => {
                    return Err(LogPackObjectPlanError::Collision(reference));
                }
            }
        }
        Ok(Self {
            dispositions: dispositions.into_boxed_slice(),
            inline_count,
            logical_object_bytes,
            inline_object_bytes,
        })
    }

    pub const fn len(&self) -> usize { self.dispositions.len() }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this method yet"
    )]
    pub const fn is_empty(&self) -> bool { self.dispositions.is_empty() }

    pub const fn inline_count(&self) -> u64 { self.inline_count }

    pub const fn logical_object_bytes(&self) -> u64 { self.logical_object_bytes }

    pub const fn inline_object_bytes(&self) -> u64 { self.inline_object_bytes }

    pub(crate) fn dispositions(&self) -> &[LogPackObjectDispositionV1] { &self.dispositions }
}
