//! Phase-ordered local transactions over a fully admitted RSpace DAG base.
//!
//! Full graph admission is a trust-boundary operation. Ordinary checkpoints
//! consume a profile-typed VerifiedDagState and validate only changed semantic
//! records, new canonical objects, and affected cross-root laws. Every phase
//! owns its private overlay; only SealedDagTransaction can cross into the
//! physical WAL/publication layer.

use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::sync::Arc;

use pathmap::PathMap;

use super::canonical::{CanonicalEncode, CanonicalFormatError, ObjectDigest, ObjectRefV1};
use super::closure::{
    ClosureError, IncrementalClosureLimits, StructuralClosureReport, verify_incremental_closure,
};
use super::manifest::{SemanticProfileId, SemanticStateManifestV1};
use super::mutation::{ContentTransition, MutationBatchViewV1, StateVersionV1};
use super::object::{CanonicalBlobKind, CanonicalObjectKind, CanonicalObjectView};
use super::retention::RetentionManifestV1;
use super::rewrite::{
    DagRewriteLimits, DagRewriteProduct, DagRewriteReport, RewriteError,
    rewrite_semantic_roots_in_store,
};
use super::semantic::{
    IncrementalSemanticLimits, IncrementalSemanticReport, PreparedSemanticDelta, SemanticProfile,
    SemanticValidationLimits, SemanticValidationReport, SemanticallyAdmittedStateV1,
    complete_semantic_delta, prepare_semantic_delta, verify_rspace_semantics,
};
use super::store::{
    ImmutableObjectStore, ObjectAdmission, StagingObjectStore, StagingStoreError,
    StagingStoreLimits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagTransactionLimits {
    pub staging: StagingStoreLimits,
    pub rewrite: DagRewriteLimits,
    pub closure: IncrementalClosureLimits,
    pub semantic: IncrementalSemanticLimits,
}

impl DagTransactionLimits {
    pub const UNBOUNDED: Self = Self {
        staging: StagingStoreLimits::UNBOUNDED,
        rewrite: DagRewriteLimits::UNBOUNDED,
        closure: IncrementalClosureLimits::UNBOUNDED,
        semantic: IncrementalSemanticLimits::UNBOUNDED,
    };
}

impl Default for DagTransactionLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

impl DagTransactionLimits {
    /// Rewrite and closure retain identities in the same exact-path arena, so
    /// their independently useful low-level limits become one aggregate
    /// transaction ceiling. The stricter caller value governs both phases.
    fn with_aggregate_exact_path_limit(mut self) -> Self {
        let aggregate = self
            .rewrite
            .max_exact_path_chunks
            .min(self.closure.max_exact_path_chunks);
        self.rewrite.max_exact_path_chunks = aggregate;
        self.closure.max_exact_path_chunks = aggregate;
        self
    }
}

#[derive(Debug)]
pub enum DagTransactionError<E> {
    BaseVersion {
        verified: StateVersionV1,
        mutation: StateVersionV1,
    },
    Stage(Box<StagingStoreError<E>>),
    Rewrite(Box<RewriteError<StagingStoreError<E>>>),
    Validation(Box<ClosureError<StagingStoreError<E>>>),
    Format(Box<CanonicalFormatError>),
}

impl<E: Display> Display for DagTransactionError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BaseVersion { verified, mutation } => write!(
                formatter,
                "mutation base {mutation:?} does not equal verified base {verified:?}"
            ),
            Self::Stage(error) => write!(formatter, "transaction staging failed: {error}"),
            Self::Rewrite(error) => write!(formatter, "transaction rewrite failed: {error}"),
            Self::Validation(error) => write!(formatter, "target validation failed: {error}"),
            Self::Format(error) => Display::fmt(error.as_ref(), formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DagTransactionError<E> {}

impl<E> From<CanonicalFormatError> for DagTransactionError<E> {
    fn from(error: CanonicalFormatError) -> Self { Self::Format(Box::new(error)) }
}

/// A non-forgeable binding between one immutable store, one exact catalog
/// version, and one authenticated semantic profile.
pub struct VerifiedDagState<'store, S: ImmutableObjectStore, P: SemanticProfile> {
    store: &'store S,
    version: StateVersionV1,
    admission: SemanticValidationReport,
    profile: PhantomData<fn() -> P>,
}

impl<'store, S: ImmutableObjectStore, P: SemanticProfile> Copy for VerifiedDagState<'store, S, P> {}

impl<'store, S: ImmutableObjectStore, P: SemanticProfile> Clone for VerifiedDagState<'store, S, P> {
    fn clone(&self) -> Self { *self }
}

impl<'store, S: ImmutableObjectStore, P: SemanticProfile> VerifiedDagState<'store, S, P> {
    /// Fully admit an untrusted or previously unknown graph. Local successor
    /// transactions use the resulting token and never repeat this whole-state
    /// traversal on their hot path.
    pub fn admit(
        store: &'store S,
        version: StateVersionV1,
        limits: SemanticValidationLimits,
    ) -> Result<Self, ClosureError<S::Error>> {
        let admission = verify_rspace_semantics::<P, S>(store, version.manifest(), limits)?;
        Ok(Self {
            store,
            version,
            admission,
            profile: PhantomData,
        })
    }

    /// Construct transaction authority from a completed trust-boundary
    /// admission without repeating the whole-state walk. The semantic token's
    /// constructor is private to the verifier, and the exact manifest binding
    /// is checked again here before authority is exposed.
    pub(crate) fn from_completed_admission(
        store: &'store S,
        version: StateVersionV1,
        admission: SemanticallyAdmittedStateV1,
    ) -> Result<Self, CanonicalFormatError> {
        if admission.manifest() != version.manifest() {
            return Err(CanonicalFormatError::SemanticCoherence {
                detail: "semantic admission token does not name the requested state manifest"
                    .to_owned(),
            });
        }
        Ok(Self {
            store,
            version,
            admission: admission.report(),
            profile: PhantomData,
        })
    }

    pub const fn version(self) -> StateVersionV1 { self.version }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn admission_report(self) -> SemanticValidationReport { self.admission }

    /// Borrow the exact object store sealed by this admission token.  The
    /// returned reference cannot outlive the token, so callers cannot detach
    /// typed projections from the authenticated catalog generation.
    pub(crate) fn object_store(&self) -> &'store S { self.store }

    /// Perform a bounded, stack-safe lookup in one authenticated semantic
    /// trie.  The caller supplies the root and canonical byte key; no trie is
    /// flattened or reconstructed.  This is the primitive used by read-only
    /// durable projections before a legacy hash-key bridge is selected.
    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this method yet"
    )]
    pub(crate) fn lookup_endpoint(
        &self,
        root: ObjectRefV1,
        key: &[u8],
        max_steps: u64,
    ) -> Result<
        Option<crate::rspace::persistence::EndpointValue>,
        crate::rspace::persistence::ClosureError<S::Error>,
    > {
        let mut steps = 0;
        crate::rspace::persistence::semantic::lookup_endpoint_counted(
            self.store,
            root,
            key,
            max_steps,
            &mut steps,
            "durable semantic lookup steps",
        )
    }

    /// Load one authenticated canonical object named by this state's object
    /// graph.  Length and digest are checked before the bytes are returned.
    pub(crate) fn load_object(
        &self,
        reference: ObjectRefV1,
    ) -> Result<S::Object, crate::rspace::persistence::ClosureError<S::Error>> {
        crate::rspace::persistence::semantic::load_reference(self.store, reference)
    }

    /// Parse the semantic manifest named by this exact state version.  The
    /// manifest is loaded through the same authenticated object path as trie
    /// nodes, so its roots cannot be supplied independently by a caller.
    pub(crate) fn semantic_manifest(
        &self,
    ) -> Result<
        crate::rspace::persistence::SemanticStateManifestView,
        crate::rspace::persistence::ClosureError<S::Error>,
    > {
        let bytes = self.load_object(self.version.manifest())?;
        crate::rspace::persistence::SemanticStateManifestView::parse(bytes.as_ref())
            .map_err(crate::rspace::persistence::ClosureError::Format)
    }

    /// Project authenticated endpoints under one canonical byte prefix with
    /// explicit work and result budgets.  The traversal is iterative and
    /// preserves the trie paths exactly as encoded by the durable DAG.
    pub(crate) fn collect_endpoints_under_prefix(
        &self,
        root: ObjectRefV1,
        prefix: &[u8],
        max_steps: u64,
        max_endpoints: u64,
    ) -> Result<
        Vec<(Vec<u8>, crate::rspace::persistence::EndpointValue)>,
        crate::rspace::persistence::ClosureError<S::Error>,
    > {
        crate::rspace::persistence::semantic::collect_endpoints_under_prefix(
            self.store,
            root,
            prefix,
            max_steps,
            max_endpoints,
        )
    }

    /// Visit authenticated endpoints without materializing the whole
    /// projection. The callback runs in traversal order and may decode or
    /// stream each canonical endpoint immediately.
    pub(crate) fn visit_endpoints_under_prefix<F>(
        &self,
        root: ObjectRefV1,
        prefix: &[u8],
        max_steps: u64,
        max_endpoints: u64,
        visit: F,
    ) -> Result<(), crate::rspace::persistence::ClosureError<S::Error>>
    where
        F: FnMut(
            &[u8],
            crate::rspace::persistence::EndpointValue,
        ) -> Result<(), crate::rspace::persistence::ClosureError<S::Error>>,
    {
        crate::rspace::persistence::semantic::visit_endpoints_under_prefix(
            self.store,
            root,
            prefix,
            max_steps,
            max_endpoints,
            visit,
        )
    }
}

/// Caller-content phase. Only exact continuation-source blobs may enter;
/// nodes and manifests are constructed by later private phases.
pub struct OpenDagTransaction<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile> {
    _verified: VerifiedDagState<'store, S, P>,
    mutation: MutationBatchViewV1<'mutation>,
    limits: DagTransactionLimits,
    staging: StagingObjectStore<'store, S>,
    staged_sources: HashSet<ObjectRefV1>,
}

impl<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile>
    OpenDagTransaction<'store, 'mutation, S, P>
{
    pub fn new(
        verified: VerifiedDagState<'store, S, P>,
        mutation: MutationBatchViewV1<'mutation>,
        limits: DagTransactionLimits,
    ) -> Result<Self, DagTransactionError<S::Error>> {
        if mutation.base() != verified.version {
            return Err(DagTransactionError::BaseVersion {
                verified: verified.version,
                mutation: mutation.base(),
            });
        }
        let limits = limits.with_aggregate_exact_path_limit();
        Ok(Self {
            staging: StagingObjectStore::with_limits(verified.store, limits.staging),
            _verified: verified,
            mutation,
            limits,
            staged_sources: HashSet::new(),
        })
    }

    pub fn stage_continuation_source(
        &mut self,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<(ObjectRefV1, ObjectAdmission), DagTransactionError<S::Error>> {
        let object = CanonicalObjectView::parse(&canonical_bytes)?;
        let source_ref = match object {
            CanonicalObjectView::Blob(blob)
                if blob.kind() == CanonicalBlobKind::ContinuationSourceV1 =>
            {
                blob.object_ref()
            }
            CanonicalObjectView::Blob(blob) => {
                return Err(CanonicalFormatError::PayloadKind {
                    namespace: "transaction-continuation-source",
                    expected: CanonicalBlobKind::ContinuationSourceV1.name(),
                    actual: blob.kind().name(),
                }
                .into());
            }
            actual => {
                return Err(CanonicalFormatError::UnexpectedObjectKind {
                    digest: ObjectDigest::of_canonical_bytes(&canonical_bytes),
                    expected: CanonicalObjectKind::Blob.name(),
                    actual: actual.kind().name(),
                }
                .into());
            }
        };
        let newly_tracked = !self.staged_sources.contains(&source_ref);
        if newly_tracked {
            self.staged_sources
                .try_reserve(1)
                .map_err(|_| CanonicalFormatError::AllocationFailure { requested: 1 })?;
        }
        let admitted = self
            .staging
            .stage_prevalidated(canonical_bytes)
            .map_err(|error| DagTransactionError::Stage(Box::new(error)))?;
        debug_assert_eq!(admitted.0, source_ref);
        if newly_tracked {
            self.staged_sources.insert(source_ref);
        }
        Ok(admitted)
    }

    /// Stage every newly introduced continuation source from the authoritative
    /// transition journal. The resolver supplies the canonical suffix bytes
    /// held by the live PathMap record; this method never decodes a
    /// continuation or reconstructs a typed record. A staging-owned byte
    /// allocation is made only when the transaction admission API requires an
    /// owned `Arc<[u8]>`.
    pub fn stage_continuation_sources<F>(
        &mut self,
        transitions: &PathMap<ContentTransition>,
        mut resolve: F,
    ) -> Result<(), DagTransactionError<S::Error>>
    where
        F: FnMut(&[u8]) -> Option<Arc<[u8]>>,
    {
        for (path, transition) in transitions.iter() {
            let Some(expected) = transition.after() else {
                continue;
            };
            let canonical = resolve(&path)
                .ok_or_else(|| CanonicalFormatError::MissingObject(expected.digest()))?;
            let (actual, _) = self.stage_continuation_source(canonical)?;
            if actual != expected {
                return Err(CanonicalFormatError::DigestMismatch {
                    expected: expected.digest(),
                    actual: actual.digest(),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Stage continuation sources from the revision-local byte journal. This
    /// is the normal production adapter path; the source map is derived from
    /// the authoritative live trie and contains no typed continuation values.
    pub fn stage_continuation_source_journal(
        &mut self,
        transitions: &PathMap<ContentTransition>,
        source_bytes: &PathMap<Arc<[u8]>>,
    ) -> Result<(), DagTransactionError<S::Error>> {
        self.stage_continuation_sources(transitions, |path| source_bytes.get(path).cloned())
    }

    pub fn seal(self) -> Result<SealedDagTransaction<'mutation, P>, DagTransactionError<S::Error>> {
        self.content_ready()?
            .rewrite()?
            .manifest()?
            .validate()?
            .seal()
    }

    fn content_ready(
        self,
    ) -> Result<ContentReadyDagTransaction<'store, 'mutation, S, P>, DagTransactionError<S::Error>>
    {
        // This scans only changed records. Missing, mistyped, malformed, or
        // unreferenced caller content fails before any radix frontier opens.
        let semantic = prepare_semantic_delta::<P, _>(
            &self.staging,
            self.mutation,
            &self.staged_sources,
            self.limits.semantic,
        )
        .map_err(|error| DagTransactionError::Validation(Box::new(error)))?;
        Ok(ContentReadyDagTransaction {
            _verified: self._verified,
            mutation: self.mutation,
            limits: self.limits,
            staging: self.staging,
            semantic,
        })
    }
}

struct ContentReadyDagTransaction<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile> {
    _verified: VerifiedDagState<'store, S, P>,
    mutation: MutationBatchViewV1<'mutation>,
    limits: DagTransactionLimits,
    staging: StagingObjectStore<'store, S>,
    semantic: PreparedSemanticDelta,
}

impl<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile>
    ContentReadyDagTransaction<'store, 'mutation, S, P>
{
    fn rewrite(
        self,
    ) -> Result<RewrittenDagTransaction<'store, 'mutation, S, P>, DagTransactionError<S::Error>>
    {
        let rewritten =
            rewrite_semantic_roots_in_store(&self.staging, self.mutation, self.limits.rewrite)
                .map_err(|error| DagTransactionError::Rewrite(Box::new(error)))?;
        Ok(RewrittenDagTransaction {
            _verified: self._verified,
            mutation: self.mutation,
            limits: self.limits,
            staging: self.staging,
            semantic: self.semantic,
            rewritten,
        })
    }
}

struct RewrittenDagTransaction<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile> {
    _verified: VerifiedDagState<'store, S, P>,
    mutation: MutationBatchViewV1<'mutation>,
    limits: DagTransactionLimits,
    staging: StagingObjectStore<'store, S>,
    semantic: PreparedSemanticDelta,
    rewritten: DagRewriteProduct,
}

impl<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile>
    RewrittenDagTransaction<'store, 'mutation, S, P>
{
    fn manifest(
        self,
    ) -> Result<ManifestedDagTransaction<'store, 'mutation, S, P>, DagTransactionError<S::Error>>
    {
        if self.rewritten.profile_id != P::ID {
            return Err(CanonicalFormatError::SemanticProfileMismatch {
                expected: *P::ID.as_bytes(),
                actual: *self.rewritten.profile_id.as_bytes(),
            }
            .into());
        }
        let manifest = SemanticStateManifestV1::new(
            self.rewritten.schema_epoch,
            self.rewritten.profile_id,
            self.rewritten.roots,
            self.rewritten.legacy_history_root,
        )?;
        let bytes: Arc<[u8]> = manifest.try_canonical_bytes()?.into();
        let (target_manifest, _) = self
            .staging
            .stage_prevalidated(bytes)
            .map_err(|error| DagTransactionError::Stage(Box::new(error)))?;
        Ok(ManifestedDagTransaction {
            _verified: self._verified,
            mutation: self.mutation,
            limits: self.limits,
            staging: self.staging,
            semantic: self.semantic,
            rewritten: self.rewritten,
            target_manifest,
        })
    }
}

struct ManifestedDagTransaction<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile> {
    _verified: VerifiedDagState<'store, S, P>,
    mutation: MutationBatchViewV1<'mutation>,
    limits: DagTransactionLimits,
    staging: StagingObjectStore<'store, S>,
    semantic: PreparedSemanticDelta,
    rewritten: DagRewriteProduct,
    target_manifest: ObjectRefV1,
}

impl<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile>
    ManifestedDagTransaction<'store, 'mutation, S, P>
{
    fn validate(
        mut self,
    ) -> Result<ValidatedDagTransaction<'store, 'mutation, S, P>, DagTransactionError<S::Error>>
    {
        let closure = verify_incremental_closure(
            &self.staging,
            self.target_manifest,
            &mut self.rewritten.provenance,
            self.limits.closure,
        )
        .map_err(|error| DagTransactionError::Validation(Box::new(error)))?;
        let semantic = complete_semantic_delta(
            &self.staging,
            self.rewritten.roots,
            self.semantic,
            self.limits.semantic,
        )
        .map_err(|error| DagTransactionError::Validation(Box::new(error)))?;
        Ok(ValidatedDagTransaction {
            _verified: self._verified,
            mutation: self.mutation,
            staging: self.staging,
            rewritten: self.rewritten,
            target_manifest: self.target_manifest,
            closure,
            semantic,
        })
    }
}

struct ValidatedDagTransaction<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile> {
    _verified: VerifiedDagState<'store, S, P>,
    mutation: MutationBatchViewV1<'mutation>,
    staging: StagingObjectStore<'store, S>,
    rewritten: DagRewriteProduct,
    target_manifest: ObjectRefV1,
    closure: StructuralClosureReport,
    semantic: IncrementalSemanticReport,
}

impl<'store, 'mutation, S: ImmutableObjectStore, P: SemanticProfile>
    ValidatedDagTransaction<'store, 'mutation, S, P>
{
    fn seal(self) -> Result<SealedDagTransaction<'mutation, P>, DagTransactionError<S::Error>> {
        let objects = self
            .staging
            .into_objects(|reference, canonical_bytes| SealedObject {
                reference,
                canonical_bytes,
            })
            .map_err(|error| DagTransactionError::Stage(Box::new(error)))?;
        let target = StateVersionV1::new(self.rewritten.target_generation, self.target_manifest);
        Ok(SealedDagTransaction {
            receipt: SealedTransitionV1 {
                base: self.rewritten.base,
                target,
                mutation_digest: self.mutation.payload_digest(),
                profile_id: P::ID,
            },
            diagnostics: DagTransactionDiagnostics {
                rewrite: self.rewritten.report,
                closure: self.closure,
                semantic: self.semantic,
            },
            mutation_bytes: self.mutation.canonical_bytes(),
            objects,
            retention_manifest: None,
            profile: PhantomData,
        })
    }
}

/// Deterministic authenticated WAL authority. Diagnostic counters are kept
/// in a separate sidecar because physical object presence and instrumentation
/// must never alter transaction identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealedTransitionV1 {
    base: StateVersionV1,
    target: StateVersionV1,
    mutation_digest: ObjectDigest,
    profile_id: SemanticProfileId,
}

impl SealedTransitionV1 {
    pub(crate) const fn new_for_persistence(
        base: StateVersionV1,
        target: StateVersionV1,
        mutation_digest: ObjectDigest,
        profile_id: SemanticProfileId,
    ) -> Self {
        Self {
            base,
            target,
            mutation_digest,
            profile_id,
        }
    }

    pub const fn base(self) -> StateVersionV1 { self.base }

    pub const fn target(self) -> StateVersionV1 { self.target }

    pub const fn mutation_digest(self) -> ObjectDigest { self.mutation_digest }

    pub const fn profile_id(self) -> SemanticProfileId { self.profile_id }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagTransactionDiagnostics {
    pub rewrite: DagRewriteReport,
    pub closure: StructuralClosureReport,
    pub semantic: IncrementalSemanticReport,
}

#[derive(Clone)]
pub struct SealedObject {
    reference: ObjectRefV1,
    canonical_bytes: Arc<[u8]>,
}

impl std::fmt::Debug for SealedObject {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SealedObject")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

impl SealedObject {
    pub const fn reference(&self) -> ObjectRefV1 { self.reference }

    pub fn canonical_bytes(&self) -> &[u8] { &self.canonical_bytes }

    pub(crate) fn canonical_bytes_arc(&self) -> Arc<[u8]> { Arc::clone(&self.canonical_bytes) }
}

/// Immutable, profile-typed WAL input with no base-store lifetime, lock,
/// callback, or mutable collection.
pub struct SealedDagTransaction<'mutation, P: SemanticProfile> {
    receipt: SealedTransitionV1,
    diagnostics: DagTransactionDiagnostics,
    mutation_bytes: &'mutation [u8],
    objects: Box<[SealedObject]>,
    retention_manifest: Option<ObjectRefV1>,
    profile: PhantomData<fn() -> P>,
}

impl<P: SemanticProfile> std::fmt::Debug for SealedDagTransaction<'_, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SealedDagTransaction")
            .field("receipt", &self.receipt)
            .field("mutation_bytes", &self.mutation_bytes.len())
            .field("objects", &self.objects.len())
            .finish()
    }
}

impl<'mutation, P: SemanticProfile> SealedDagTransaction<'mutation, P> {
    pub const fn receipt(&self) -> SealedTransitionV1 { self.receipt }

    pub const fn diagnostics(&self) -> DagTransactionDiagnostics { self.diagnostics }

    pub const fn mutation_bytes(&self) -> &'mutation [u8] { self.mutation_bytes }

    pub fn objects(&self) -> &[SealedObject] { &self.objects }

    pub(crate) fn with_retention_manifest(
        mut self,
        manifest: RetentionManifestV1,
    ) -> Result<Self, CanonicalFormatError> {
        if self.retention_manifest.is_some() {
            return Err(CanonicalFormatError::SemanticCoherence {
                detail: "retention manifest was attached more than once".to_owned(),
            });
        }
        let reference = manifest.object_ref();
        let bytes: Arc<[u8]> = manifest.try_canonical_bytes()?.into();
        let object = SealedObject {
            reference,
            canonical_bytes: bytes,
        };
        let mut objects = self.objects.into_vec();
        match objects
            .binary_search_by_key(&reference.digest(), |candidate| candidate.reference.digest())
        {
            Ok(index) => {
                if objects[index].canonical_bytes() != object.canonical_bytes() {
                    return Err(CanonicalFormatError::ContentCollision(reference.digest()));
                }
            }
            Err(index) => objects.insert(index, object),
        }
        self.objects = objects.into_boxed_slice();
        self.retention_manifest = Some(reference);
        Ok(self)
    }

    pub(crate) const fn retention_manifest(&self) -> Option<ObjectRefV1> { self.retention_manifest }
}
