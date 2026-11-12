use std::fmt::{Display, Formatter};

use super::admission::DurableAuthorityCoreV1;
use crate::rspace::errors::RootError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::persistence::canonical::OBJECT_DIGEST_LEN;
use crate::rspace::persistence::closure::StructuralClosureLedgerV1;
use crate::rspace::persistence::manifest::SEMANTIC_MANIFEST_MAX_CANONICAL_LEN;
use crate::rspace::persistence::{
    GenerationLeaseError, ImmutableObjectStore, ObjectDigest, ObjectRefV1, SemanticProfile,
    SemanticRoots, SemanticStateManifestView,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DurableStateRootV1(ObjectDigest);

impl DurableStateRootV1 {
    pub(crate) const fn of_manifest(manifest: ObjectRefV1) -> Self { Self(manifest.digest()) }

    pub(crate) const fn digest(self) -> ObjectDigest { self.0 }
}

impl Display for DurableStateRootV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl From<DurableStateRootV1> for Blake2b256Hash {
    fn from(root: DurableStateRootV1) -> Self {
        Blake2b256Hash::from_bytes(root.0.as_bytes().to_vec())
    }
}

impl TryFrom<&Blake2b256Hash> for DurableStateRootV1 {
    type Error = RootError;

    fn try_from(hash: &Blake2b256Hash) -> Result<Self, Self::Error> {
        ObjectDigest::from_slice(&hash.0).map(Self).map_err(|_| {
            RootError::InvalidRootHash(format!(
                "expected {OBJECT_DIGEST_LEN} bytes, found {}",
                hash.0.len()
            ))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedDurableStateV1 {
    manifest: ObjectRefV1,
    roots: SemanticRoots,
}

impl ResolvedDurableStateV1 {
    pub(crate) const fn root(self) -> DurableStateRootV1 {
        DurableStateRootV1::of_manifest(self.manifest)
    }

    pub(crate) const fn manifest(self) -> ObjectRefV1 { self.manifest }

    pub(crate) const fn roots(self) -> SemanticRoots { self.roots }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DurableStateReadHandleErrorV1 {
    Root(RootError),
    GenerationLease(GenerationLeaseError),
}

impl Display for DurableStateReadHandleErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root(error) => write!(formatter, "durable state is not retained here: {error}"),
            Self::GenerationLease(error) => {
                write!(formatter, "durable generation lease was refused: {error}")
            }
        }
    }
}

impl std::error::Error for DurableStateReadHandleErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Root(_) => None,
            Self::GenerationLease(error) => Some(error),
        }
    }
}

impl From<GenerationLeaseError> for DurableStateReadHandleErrorV1 {
    fn from(error: GenerationLeaseError) -> Self { Self::GenerationLease(error) }
}

impl<P: SemanticProfile> DurableAuthorityCoreV1<P> {
    pub(crate) fn resolve(
        &self,
        root: DurableStateRootV1,
    ) -> Result<ResolvedDurableStateV1, RootError> {
        resolve_retained_state::<P, _>(
            self.retained_closure(),
            self.verified_state().object_store(),
            root,
        )
    }

    pub(crate) fn require_retained(
        &self,
        resolved: &ResolvedDurableStateV1,
    ) -> Result<(), RootError> {
        require_counted_root(self.retained_closure(), resolved.root())
    }
}

pub(crate) fn resolve_retained_state<P, S>(
    ledger: &StructuralClosureLedgerV1,
    store: &S,
    root: DurableStateRootV1,
) -> Result<ResolvedDurableStateV1, RootError>
where
    P: SemanticProfile,
    S: ImmutableObjectStore,
    S::Error: Display,
{
    require_counted_root(ledger, root)?;
    let bytes = store
        .get_bounded(&root.digest(), SEMANTIC_MANIFEST_MAX_CANONICAL_LEN)
        .map_err(|error| RootError::KvStoreError(format!("durable state root {root}: {error}")))?
        .ok_or_else(|| {
            RootError::CorruptRootCatalog(format!(
                "retained durable state root {root} is absent from the object store"
            ))
        })?;
    let manifest = SemanticStateManifestView::parse(bytes.as_ref()).map_err(|error| {
        RootError::CorruptRootCatalog(format!(
            "retained durable state root {root} is not a semantic manifest: {error}"
        ))
    })?;
    if manifest.object_ref().digest() != root.digest() {
        return Err(RootError::CorruptRootCatalog(format!(
            "the object store returned other bytes for retained durable state root {root}"
        )));
    }
    match (manifest.legacy_history_root(), manifest.profile_id()) {
        (Some(_), _) => Err(RootError::UnknownRootError(format!(
            "durable state root {root} names a manifest with a legacy history provenance root"
        ))),
        (None, profile) if profile != P::ID => Err(RootError::UnknownRootError(format!(
            "durable state root {root} names a manifest of another semantic profile"
        ))),
        (None, _) => Ok(ResolvedDurableStateV1 {
            manifest: manifest.object_ref(),
            roots: manifest.roots(),
        }),
    }
}

fn require_counted_root(
    ledger: &StructuralClosureLedgerV1,
    root: DurableStateRootV1,
) -> Result<(), RootError> {
    if ledger.counts_semantic_root(root.digest()) {
        Ok(())
    } else {
        Err(RootError::UnknownRootError(format!(
            "durable state root {root} is not a current or retained state of this store"
        )))
    }
}
