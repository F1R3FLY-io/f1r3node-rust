use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::manifest::{
    SemanticNamespace, SemanticProfileId, SemanticRoots, SemanticStateManifestView,
};
use super::object::{
    CanonicalBlobKind, CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView,
    EndpointValue,
};
use super::retention::v2_reader::RetentionMetadataInventoryV2;
use super::rewrite::{ExactPath, InheritedBoundaryKind, InheritedObjectRef, RewriteProvenance};
use super::store::{ImmutableObjectStore, StagingObjectStore, StagingStoreError};

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "task 7927 wires the trusted successor to these trials")
)]
pub(crate) mod ledger_trial;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StructuralClosureReport {
    pub distinct_objects: u64,
    pub node_objects: u64,
    pub payload_objects: u64,
    pub canonical_bytes: u64,
    pub references: u64,
    pub maximum_worklist: u64,
}

/// Resource ceilings for structural object-graph admission.
///
/// No field limits DAG depth.  The verifier remains an explicit worklist PDA;
/// these budgets bound total hostile-input work and storage instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructuralValidationLimits {
    pub max_distinct_objects: u64,
    pub max_canonical_bytes: u64,
    pub max_references: u64,
    pub max_worklist: u64,
}

impl StructuralValidationLimits {
    pub const fn unbounded() -> Self {
        Self {
            max_distinct_objects: u64::MAX,
            max_canonical_bytes: u64::MAX,
            max_references: u64::MAX,
            max_worklist: u64::MAX,
        }
    }
}

impl Default for StructuralValidationLimits {
    fn default() -> Self { Self::unbounded() }
}

/// Resource ceilings specific to exact-path incremental closure validation.
/// Logical visits include repeated visits to a shared staged object at
/// different absolute paths; such visits cannot be memoized by digest alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementalClosureLimits {
    pub structural: StructuralValidationLimits,
    pub max_logical_visits: u64,
    pub max_exact_path_chunks: u64,
}

impl IncrementalClosureLimits {
    pub const UNBOUNDED: Self = Self {
        structural: StructuralValidationLimits::unbounded(),
        max_logical_visits: u64::MAX,
        max_exact_path_chunks: u64::MAX,
    };
}

impl Default for IncrementalClosureLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Debug)]
pub enum ClosureError<E> {
    Format(CanonicalFormatError),
    Store(E),
}

impl<E: Display> Display for ClosureError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(error) => write!(formatter, "persistent DAG format error: {error}"),
            Self::Store(error) => write!(formatter, "persistent object-store error: {error}"),
        }
    }
}

impl<E> std::error::Error for ClosureError<E>
where E: std::error::Error + 'static
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Store(error) => Some(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum EndpointPolicy {
    Multiplicity,
    Content(CanonicalBlobKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Expectation {
    Manifest,
    Node {
        namespace: SemanticNamespace,
        policy: EndpointPolicy,
        root: bool,
    },
    Blob {
        namespace: SemanticNamespace,
        kind: CanonicalBlobKind,
    },
}

impl Expectation {
    /// Compact, exact memo key for the finite structural-expectation grammar.
    /// Keeping this in the digest ledger removes one allocation per verified
    /// expectation without conflating namespace, endpoint policy, or root
    /// normalization.
    const fn bit(self) -> u32 {
        let index = match self {
            Self::Manifest => 0,
            Self::Node {
                namespace,
                policy,
                root,
            } => {
                let namespace = match namespace {
                    SemanticNamespace::DataBag => 0,
                    SemanticNamespace::ContinuationMap => 1,
                    SemanticNamespace::JoinBag => 2,
                };
                let policy = match policy {
                    EndpointPolicy::Multiplicity => 0,
                    EndpointPolicy::Content(CanonicalBlobKind::ContinuationSourceV1) => 1,
                };
                1 + namespace * 4 + policy * 2 + root as usize
            }
            Self::Blob {
                namespace,
                kind: CanonicalBlobKind::ContinuationSourceV1,
            } => {
                let namespace = match namespace {
                    SemanticNamespace::DataBag => 0,
                    SemanticNamespace::ContinuationMap => 1,
                    SemanticNamespace::JoinBag => 2,
                };
                13 + namespace
            }
        };
        1u32 << index
    }
}

#[derive(Clone, Copy, Debug)]
enum Work {
    Enter(ObjectRefV1, Expectation, ClosureCreditV1),
    Exit(ObjectDigest, Expectation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct IncrementalVisitKey {
    reference: ObjectRefV1,
    expectation: Expectation,
    path: ExactPath,
}

#[derive(Clone, Copy, Debug)]
enum IncrementalWork {
    Enter(IncrementalVisitKey),
    Exit(IncrementalVisitKey),
}

/// The ledger's independent reachability domains. The two semantic domains
/// count state-DAG objects; the metadata domain counts the retention V2
/// anchor and the radix nodes reachable from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClosureDomainV1 {
    Current,
    Retained,
    Metadata,
}

impl ClosureDomainV1 {
    pub(crate) const ALL: [Self; 3] = [Self::Current, Self::Retained, Self::Metadata];
    /// The two domains a semantic closure credits.
    #[cfg(test)]
    pub(crate) const SEMANTIC: [Self; 2] = [Self::Current, Self::Retained];

    const fn index(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Retained => 1,
            Self::Metadata => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClosureCreditV1(u8);

impl ClosureCreditV1 {
    pub(crate) const CURRENT_AND_RETAINED: Self = Self(0b11);
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const RETAINED: Self = Self(0b10);

    pub(crate) const fn contains(self, domain: ClosureDomainV1) -> bool {
        self.0 & (1 << domain.index()) != 0
    }

    pub(crate) const fn is_empty(self) -> bool { self.0 == 0 }

    const fn with(self, domain: ClosureDomainV1) -> Self { Self(self.0 | (1 << domain.index())) }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClosureInventoryV1 {
    pub(crate) distinct_objects: u64,
    pub(crate) canonical_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
struct DigestState {
    declared_len: u64,
    visit_epoch: u64,
    active_epoch: u64,
    verified_expectations: u32,
    counts: [u64; 3],
}

impl DigestState {
    const fn fresh(declared_len: u64) -> Self {
        Self {
            declared_len,
            visit_epoch: 0,
            active_epoch: 0,
            verified_expectations: 0,
            counts: [0; 3],
        }
    }

    fn credit<E>(&mut self, credit: ClosureCreditV1) -> Result<ClosureCreditV1, ClosureError<E>> {
        let mut crossed = ClosureCreditV1::NONE;
        for domain in ClosureDomainV1::ALL {
            let count = &mut self.counts[domain.index()];
            match (credit.contains(domain), *count) {
                (false, _) => {}
                (true, 0) => {
                    *count = 1;
                    crossed = crossed.with(domain);
                }
                (true, positive) => {
                    *count = positive.checked_add(1).ok_or_else(|| {
                        format_error(CanonicalFormatError::CounterOverflow(
                            "closure reference counts",
                        ))
                    })?;
                }
            }
        }
        Ok(crossed)
    }
}

/// One digest-keyed ledger shared by every state named by a retention
/// manifest.  Per domain it holds each object's checked reference count (its
/// root multiplicity plus one per outgoing reference occurrence of an active
/// object), the root multiplicities, and the inventory of the objects whose
/// count is positive.  Epochs deduplicate one state's walk, while the counts
/// deduplicate the union across states.  The ledger is itself a PathMap, so no
/// flattened object list, sort, or surrogate identifier is introduced.
#[derive(Clone)]
pub(crate) struct StructuralClosureLedgerV1 {
    states: PathMap<DigestState>,
    roots: PathMap<[u64; 3]>,
    next_epoch: u64,
    inventories: [ClosureInventoryV1; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RetainedUnionClosureReportV1 {
    pub(crate) distinct_objects: u64,
    pub(crate) canonical_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetainedUnionClosureLimitsV1 {
    pub(crate) max_distinct_objects: u64,
    pub(crate) max_canonical_bytes: u64,
}

impl RetainedUnionClosureLimitsV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        max_distinct_objects: u64::MAX,
        max_canonical_bytes: u64::MAX,
    };
}

/// Non-forgeable evidence that one exact state manifest has completed the
/// structural closure PDA under the supplied immutable store.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StructurallyAdmittedStateV1 {
    manifest: ObjectRefV1,
    profile_id: SemanticProfileId,
    roots: SemanticRoots,
    report: StructuralClosureReport,
}

impl StructurallyAdmittedStateV1 {
    pub(crate) const fn manifest(self) -> ObjectRefV1 { self.manifest }

    pub(crate) const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub(crate) const fn roots(self) -> SemanticRoots { self.roots }

    pub(crate) const fn report(self) -> StructuralClosureReport { self.report }
}

impl StructuralClosureLedgerV1 {
    pub(crate) fn new() -> Self {
        Self {
            states: PathMap::new(),
            roots: PathMap::new(),
            next_epoch: 1,
            inventories: [ClosureInventoryV1::default(); 3],
        }
    }

    pub(crate) const fn retained_union_report(&self) -> RetainedUnionClosureReportV1 {
        let inventory = self.inventories[ClosureDomainV1::Retained.index()];
        RetainedUnionClosureReportV1 {
            distinct_objects: inventory.distinct_objects,
            canonical_bytes: inventory.canonical_bytes,
        }
    }

    /// The inventory of the retention V2 metadata closure: empty for a V1
    /// retention source.
    pub(crate) const fn metadata_inventory(&self) -> ClosureInventoryV1 {
        self.inventories[ClosureDomainV1::Metadata.index()]
    }

    #[cfg(test)]
    pub(crate) const fn inventory(&self, domain: ClosureDomainV1) -> ClosureInventoryV1 {
        self.inventories[domain.index()]
    }

    #[cfg(test)]
    pub(crate) fn count(&self, domain: ClosureDomainV1, digest: ObjectDigest) -> u64 {
        self.states
            .get_val_at(digest.as_bytes())
            .map_or(0, |state| state.counts[domain.index()])
    }

    #[cfg(test)]
    pub(crate) fn root_count(&self, domain: ClosureDomainV1, digest: ObjectDigest) -> u64 {
        self.roots
            .get_val_at(digest.as_bytes())
            .map_or(0, |counts| counts[domain.index()])
    }

    #[cfg(test)]
    pub(crate) fn counted_objects(&self) -> Vec<(ObjectRefV1, [u64; 3])> {
        let mut objects = Vec::new();
        let mut cursor = self.states.read_zipper_at_borrowed_path(&[]);
        if let Some(state) = cursor.val() {
            objects.push((object_ref_from_ledger_entry(cursor.path(), *state), state.counts));
        }
        while let Some(state) = cursor.to_next_get_val() {
            objects.push((object_ref_from_ledger_entry(cursor.path(), *state), state.counts));
        }
        objects
    }

    #[cfg(test)]
    pub(crate) fn root_multiplicities(&self) -> Vec<(ObjectDigest, [u64; 3])> {
        let mut roots = Vec::new();
        let mut cursor = self.roots.read_zipper_at_borrowed_path(&[]);
        let digest = |path: &[u8]| {
            ObjectDigest::from_slice(path).expect("closure roots are keyed by object digests")
        };
        if let Some(counts) = cursor.val() {
            roots.push((digest(cursor.path()), *counts));
        }
        while let Some(counts) = cursor.to_next_get_val() {
            roots.push((digest(cursor.path()), *counts));
        }
        roots
    }

    #[cfg(test)]
    pub(crate) fn from_test_retained_union(references: &[ObjectRefV1]) -> Self {
        let mut ledger = Self::new();
        let retained = ClosureDomainV1::Retained.index();
        let mut previous = None;
        for reference in references {
            if let Some(previous) = previous {
                assert!(
                    previous < reference.digest(),
                    "test retained-union references must be strictly digest ordered"
                );
            }
            let mut state = DigestState::fresh(reference.encoded_len());
            state.counts[retained] = 1;
            let replaced = ledger.states.insert(reference.digest().as_bytes(), state);
            assert!(replaced.is_none(), "strict test references cannot replace a digest");
            let inventory = &mut ledger.inventories[retained];
            inventory.distinct_objects = inventory
                .distinct_objects
                .checked_add(1)
                .expect("bounded property fixture object count");
            inventory.canonical_bytes = inventory
                .canonical_bytes
                .checked_add(reference.encoded_len())
                .expect("bounded property fixture canonical bytes");
            previous = Some(reference.digest());
        }
        ledger
    }

    /// Seed the metadata domain from an admitted retention V2 closure. The
    /// anchor is the domain's only root, and each object's count is its root
    /// multiplicity plus its occurrences as a child of a distinct metadata
    /// object: exactly the ledger that activating the anchor from an empty
    /// metadata domain builds, obtained without reading any object again.
    pub(crate) fn seed_retention_metadata(
        &mut self,
        anchor: ObjectRefV1,
        metadata: &RetentionMetadataInventoryV2,
    ) -> Result<(), CanonicalFormatError> {
        let domain = ClosureDomainV1::Metadata.index();
        if self.inventories[domain] != ClosureInventoryV1::default() {
            return Err(CanonicalFormatError::SemanticCoherence {
                detail: "retention metadata was seeded more than once".to_owned(),
            });
        }
        for (reference, &count) in metadata.objects().iter().zip(metadata.occurrences()) {
            let key = reference.digest();
            let mut state = match self.states.get_val_at(key.as_bytes()).copied() {
                Some(state) if state.declared_len != reference.encoded_len() => {
                    return Err(CanonicalFormatError::ObjectLengthMismatch {
                        expected: state.declared_len,
                        actual: reference.encoded_len(),
                    });
                }
                Some(state) => state,
                None => DigestState::fresh(reference.encoded_len()),
            };
            state.counts[domain] = count;
            self.states.insert(key.as_bytes(), state);
        }
        let mut roots = self
            .roots
            .get_val_at(anchor.digest().as_bytes())
            .copied()
            .unwrap_or([0; 3]);
        roots[domain] = 1;
        self.roots.insert(anchor.digest().as_bytes(), roots);
        self.inventories[domain] = ClosureInventoryV1 {
            distinct_objects: metadata.object_count(),
            canonical_bytes: metadata.canonical_bytes(),
        };
        Ok(())
    }

    /// Visit the retained closure directly in the digest order maintained by
    /// the PathMap.  The callback receives exact content references rebuilt
    /// from the trie key and its admitted declared length; no flattened list,
    /// secondary sort, or surrogate identifier is introduced.
    pub(crate) fn try_visit_retained_union<E>(
        &self,
        mut visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), E> {
        let retained = ClosureDomainV1::Retained.index();
        let mut cursor = self.states.read_zipper_at_borrowed_path(&[]);
        if let Some(state) = cursor.val() {
            if state.counts[retained] > 0 {
                visitor(object_ref_from_ledger_entry(cursor.path(), *state))?;
            }
        }
        while let Some(state) = cursor.to_next_get_val() {
            if state.counts[retained] > 0 {
                visitor(object_ref_from_ledger_entry(cursor.path(), *state))?;
            }
        }
        Ok(())
    }

    /// Visit the retained union together with the retention V2 metadata
    /// closure in one digest-ordered traversal: the DATA objects of a V2
    /// generation checkpoint. An object live in both domains is visited once.
    pub(crate) fn try_visit_retained_union_and_metadata<E>(
        &self,
        mut visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), E> {
        let retained = ClosureDomainV1::Retained.index();
        let metadata = ClosureDomainV1::Metadata.index();
        let mut cursor = self.states.read_zipper_at_borrowed_path(&[]);
        if let Some(state) = cursor.val() {
            match (state.counts[retained], state.counts[metadata]) {
                (0, 0) => {}
                _ => visitor(object_ref_from_ledger_entry(cursor.path(), *state))?,
            }
        }
        while let Some(state) = cursor.to_next_get_val() {
            match (state.counts[retained], state.counts[metadata]) {
                (0, 0) => {}
                _ => visitor(object_ref_from_ledger_entry(cursor.path(), *state))?,
            }
        }
        Ok(())
    }

    fn begin_epoch<E>(&mut self) -> Result<u64, ClosureError<E>> {
        let epoch = self.next_epoch;
        self.next_epoch = self.next_epoch.checked_add(1).ok_or_else(|| {
            format_error(CanonicalFormatError::CounterOverflow("structural closure epochs"))
        })?;
        Ok(epoch)
    }

    fn credit_root<E>(
        &mut self,
        manifest: ObjectDigest,
        credit: ClosureCreditV1,
    ) -> Result<(), ClosureError<E>> {
        if credit.is_empty() {
            return Ok(());
        }
        let mut counts = self
            .roots
            .get_val_at(manifest.as_bytes())
            .copied()
            .unwrap_or([0; 3]);
        for domain in ClosureDomainV1::ALL {
            if credit.contains(domain) {
                counts[domain.index()] =
                    counts[domain.index()].checked_add(1).ok_or_else(|| {
                        format_error(CanonicalFormatError::CounterOverflow(
                            "closure root multiplicities",
                        ))
                    })?;
            }
        }
        self.roots.insert(manifest.as_bytes(), counts);
        Ok(())
    }
}

fn object_ref_from_ledger_entry(path: &[u8], state: DigestState) -> ObjectRefV1 {
    let digest = ObjectDigest::from_slice(path)
        .expect("the structural ledger is keyed only by fixed-width object digests");
    ObjectRefV1::new(digest, state.declared_len)
        .expect("the structural verifier admits only complete canonical object lengths")
}

impl Default for StructuralClosureLedgerV1 {
    fn default() -> Self { Self::new() }
}

/// Verify canonical framing, identities, graph closure, branch normalization,
/// and namespace endpoint specialization for a semantic manifest.
///
/// This is intentionally a *structural* verifier. Publication as RSpace
/// authority additionally requires the semantic key/payload and cross-root
/// coherence verifier; structurally valid arbitrary byte paths are not RSpace
/// state. Traversal uses an explicit heap worklist, has no recursion or depth
/// ceiling, and validates every declared length before a memoized fast path.
pub fn verify_structural_closure<S>(
    store: &S,
    manifest: ObjectRefV1,
) -> Result<StructuralClosureReport, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    verify_structural_closure_with_limits(store, manifest, StructuralValidationLimits::unbounded())
}

pub fn verify_structural_closure_with_limits<S>(
    store: &S,
    manifest: ObjectRefV1,
    limits: StructuralValidationLimits,
) -> Result<StructuralClosureReport, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut ledger = StructuralClosureLedgerV1::new();
    verify_structural_closure_in_ledger_with_limits(
        store,
        manifest,
        limits,
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .map(StructurallyAdmittedStateV1::report)
}

pub(crate) fn verify_structural_closure_in_ledger_with_limits<S>(
    store: &S,
    manifest: ObjectRefV1,
    limits: StructuralValidationLimits,
    ledger: &mut StructuralClosureLedgerV1,
    union_limits: RetainedUnionClosureLimitsV1,
) -> Result<StructurallyAdmittedStateV1, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    verify_structural_closure_in_ledger_with_credit(
        store,
        manifest,
        limits,
        ledger,
        union_limits,
        ClosureCreditV1::RETAINED,
    )
}

pub(crate) fn verify_structural_closure_in_ledger_with_credit<S>(
    store: &S,
    manifest: ObjectRefV1,
    limits: StructuralValidationLimits,
    ledger: &mut StructuralClosureLedgerV1,
    union_limits: RetainedUnionClosureLimitsV1,
    credit: ClosureCreditV1,
) -> Result<StructurallyAdmittedStateV1, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let epoch = ledger.begin_epoch()?;
    ledger.credit_root(manifest.digest(), credit)?;
    let mut report = StructuralClosureReport::default();
    let mut active_objects = 0u64;
    let mut admitted_manifest = None;
    let mut work = Vec::new();
    push_work(
        &mut work,
        Work::Enter(manifest, Expectation::Manifest, credit),
        limits,
        &mut report,
    )?;

    while let Some(next) = work.pop() {
        match next {
            Work::Exit(digest, expectation) => {
                let mut state = ledger
                    .states
                    .get_val_at(digest.as_bytes())
                    .copied()
                    .expect("an exit follows an entered object");
                debug_assert_eq!(state.active_epoch, epoch);
                state.active_epoch = 0;
                state.verified_expectations |= expectation.bit();
                let replaced = ledger.states.insert(digest.as_bytes(), state);
                debug_assert!(replaced.is_some());
                active_objects -= 1;
            }
            Work::Enter(reference, expectation, credit) => {
                let digest = reference.digest();
                let existing = ledger.states.get_val_at(digest.as_bytes()).copied();
                let first_visit = existing.is_none_or(|state| state.visit_epoch != epoch);
                let admitted_distinct_objects = if first_visit {
                    let attempted = report.distinct_objects.checked_add(1).ok_or_else(|| {
                        format_error(CanonicalFormatError::CounterOverflow("distinct objects"))
                    })?;
                    check_limit("distinct objects", limits.max_distinct_objects, attempted)?;
                    Some(attempted)
                } else {
                    None
                };
                let admitted_canonical_bytes = if first_visit {
                    let attempted = report
                        .canonical_bytes
                        .checked_add(reference.encoded_len())
                        .ok_or_else(|| {
                            format_error(CanonicalFormatError::CounterOverflow("canonical bytes"))
                        })?;
                    check_limit("canonical bytes", limits.max_canonical_bytes, attempted)?;
                    Some(attempted)
                } else {
                    None
                };
                let mut state = existing.unwrap_or(DigestState::fresh(reference.encoded_len()));
                if state.declared_len != reference.encoded_len() {
                    return Err(format_error(CanonicalFormatError::ObjectLengthMismatch {
                        expected: state.declared_len,
                        actual: reference.encoded_len(),
                    }));
                }
                if first_visit {
                    state.visit_epoch = epoch;
                    state.active_epoch = 0;
                    state.verified_expectations = 0;
                }
                let crossed = state.credit(credit)?;
                let expectation_bit = expectation.bit();
                if state.verified_expectations & expectation_bit != 0 {
                    if !crossed.is_empty() {
                        return Err(format_error(CanonicalFormatError::UnexpandedClosureCrossing(
                            digest,
                        )));
                    }
                    if !credit.is_empty() {
                        ledger.states.insert(digest.as_bytes(), state);
                    }
                    continue;
                }
                if state.active_epoch == epoch {
                    return Err(format_error(CanonicalFormatError::ObjectCycle(
                        reference.digest(),
                    )));
                }
                state.active_epoch = epoch;

                let admitted_union = if crossed.contains(ClosureDomainV1::Retained) {
                    let union = ledger.inventories[ClosureDomainV1::Retained.index()];
                    let objects = union.distinct_objects.checked_add(1).ok_or_else(|| {
                        format_error(CanonicalFormatError::CounterOverflow(
                            "retained union objects",
                        ))
                    })?;
                    check_limit(
                        "retained union objects",
                        union_limits.max_distinct_objects,
                        objects,
                    )?;
                    let bytes = union
                        .canonical_bytes
                        .checked_add(reference.encoded_len())
                        .ok_or_else(|| {
                            format_error(CanonicalFormatError::CounterOverflow(
                                "retained union canonical bytes",
                            ))
                        })?;
                    check_limit(
                        "retained union canonical bytes",
                        union_limits.max_canonical_bytes,
                        bytes,
                    )?;
                    Some(ClosureInventoryV1 {
                        distinct_objects: objects,
                        canonical_bytes: bytes,
                    })
                } else {
                    None
                };
                let admitted_current = if crossed.contains(ClosureDomainV1::Current) {
                    let current = ledger.inventories[ClosureDomainV1::Current.index()];
                    Some(ClosureInventoryV1 {
                        distinct_objects: current.distinct_objects.checked_add(1).ok_or_else(
                            || {
                                format_error(CanonicalFormatError::CounterOverflow(
                                    "current closure objects",
                                ))
                            },
                        )?,
                        canonical_bytes: current
                            .canonical_bytes
                            .checked_add(reference.encoded_len())
                            .ok_or_else(|| {
                                format_error(CanonicalFormatError::CounterOverflow(
                                    "current closure canonical bytes",
                                ))
                            })?,
                    })
                } else {
                    None
                };

                let replaced = ledger.states.insert(digest.as_bytes(), state);
                debug_assert_eq!(replaced.is_some(), existing.is_some());
                active_objects = active_objects.checked_add(1).ok_or_else(|| {
                    format_error(CanonicalFormatError::CounterOverflow("active closure objects"))
                })?;
                let bytes = load_reference(store, reference)?;
                let bytes = bytes.as_ref();
                if first_visit {
                    report.distinct_objects =
                        admitted_distinct_objects.expect("first visits preflight distinct objects");
                    report.canonical_bytes =
                        admitted_canonical_bytes.expect("first visits preflight canonical bytes");
                }
                if let Some(union) = admitted_union {
                    ledger.inventories[ClosureDomainV1::Retained.index()] = union;
                }
                if let Some(current) = admitted_current {
                    ledger.inventories[ClosureDomainV1::Current.index()] = current;
                }
                let object = CanonicalObjectView::parse(bytes).map_err(format_error)?;
                push_work(
                    &mut work,
                    Work::Exit(reference.digest(), expectation),
                    limits,
                    &mut report,
                )?;
                match expectation {
                    Expectation::Manifest => {
                        let CanonicalObjectView::SemanticManifest(manifest) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::SemanticManifest,
                                object.kind(),
                            )));
                        };
                        admitted_manifest = Some((manifest.profile_id(), manifest.roots()));
                        schedule_manifest_roots(manifest, &mut work, limits, &mut report, crossed)?;
                    }
                    Expectation::Node {
                        namespace,
                        policy,
                        root,
                    } => {
                        let CanonicalObjectView::DagNode(node) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::DagNode,
                                object.kind(),
                            )));
                        };
                        if first_visit {
                            increment(&mut report.node_objects, 1, "node objects")?;
                        }
                        validate_node_context(node, namespace, policy, root)
                            .map_err(format_error)?;
                        if let EndpointValue::Content(content) = node.endpoint() {
                            let EndpointPolicy::Content(kind) = policy else {
                                return Err(format_error(CanonicalFormatError::EndpointPolicy {
                                    namespace: namespace.name(),
                                    endpoint: node.endpoint().name(),
                                }));
                            };
                            increment(&mut report.references, 1, "object references")?;
                            check_limit(
                                "object references",
                                limits.max_references,
                                report.references,
                            )?;
                            push_work(
                                &mut work,
                                Work::Enter(
                                    content,
                                    Expectation::Blob { namespace, kind },
                                    crossed,
                                ),
                                limits,
                                &mut report,
                            )?;
                        }
                        for child in node.children().iter() {
                            increment(&mut report.references, 1, "object references")?;
                            check_limit(
                                "object references",
                                limits.max_references,
                                report.references,
                            )?;
                            push_work(
                                &mut work,
                                Work::Enter(
                                    child.target,
                                    Expectation::Node {
                                        namespace,
                                        policy,
                                        root: false,
                                    },
                                    crossed,
                                ),
                                limits,
                                &mut report,
                            )?;
                        }
                    }
                    Expectation::Blob { namespace, kind } => {
                        let CanonicalObjectView::Blob(blob) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::Blob,
                                object.kind(),
                            )));
                        };
                        if blob.kind() != kind {
                            return Err(format_error(CanonicalFormatError::PayloadKind {
                                namespace: namespace.name(),
                                expected: kind.name(),
                                actual: blob.kind().name(),
                            }));
                        }
                        if first_visit {
                            increment(&mut report.payload_objects, 1, "payload objects")?;
                        }
                    }
                }
            }
        }
    }
    debug_assert_eq!(active_objects, 0);
    let (profile_id, roots) = admitted_manifest.expect("manifest expectation admits one manifest");
    Ok(StructurallyAdmittedStateV1 {
        manifest,
        profile_id,
        roots,
        report,
    })
}

/// Verify the complete logical transaction delta while stopping only at
/// namespace-bound references inherited by the private rewrite from its exact
/// verified base.
///
/// Every other target object must be present in the staging overlay, even when
/// equal bytes happen to exist elsewhere in the ambient content store.  This
/// proves both directions of portable delta closure without traversing an
/// untouched base subtree.
pub(crate) fn verify_incremental_closure<S>(
    store: &StagingObjectStore<'_, S>,
    manifest: ObjectRefV1,
    provenance: &mut RewriteProvenance,
    limits: IncrementalClosureLimits,
) -> Result<StructuralClosureReport, ClosureError<StagingStoreError<S::Error>>>
where
    S: ImmutableObjectStore,
{
    let mut report = StructuralClosureReport::default();
    let mut work = Vec::new();
    push_incremental_work(
        &mut work,
        IncrementalWork::Enter(IncrementalVisitKey {
            reference: manifest,
            expectation: Expectation::Manifest,
            path: ExactPath::empty(),
        }),
        limits.structural,
        &mut report,
    )?;
    let mut declared_lengths = HashMap::<ObjectDigest, u64>::new();
    let mut active = HashSet::<ObjectDigest>::new();
    let mut completed = HashSet::<IncrementalVisitKey>::new();
    let mut logical_visits = 0_u64;

    while let Some(next) = work.pop() {
        match next {
            IncrementalWork::Exit(key) => {
                active.remove(&key.reference.digest());
                insert_incremental_visit(&mut completed, key).map_err(format_error)?;
            }
            IncrementalWork::Enter(key) => {
                if completed.contains(&key) {
                    continue;
                }
                increment(&mut logical_visits, 1, "incremental logical visits")?;
                check_limit(
                    "incremental logical visits",
                    limits.max_logical_visits,
                    logical_visits,
                )?;
                let reference = key.reference;
                let expectation = key.expectation;
                let staged = store
                    .get_staged_bounded(&reference.digest(), reference.encoded_len())
                    .map_err(ClosureError::Store)?;
                let Some(bytes) = staged else {
                    let Some((namespace, kind)) = expectation_boundary(expectation) else {
                        return Err(format_error(CanonicalFormatError::MissingObject(
                            reference.digest(),
                        )));
                    };
                    if !provenance.contains(&InheritedObjectRef {
                        namespace,
                        kind,
                        path: key.path,
                        reference,
                    }) {
                        return Err(format_error(CanonicalFormatError::MissingObject(
                            reference.digest(),
                        )));
                    }
                    insert_incremental_visit(&mut completed, key).map_err(format_error)?;
                    continue;
                };

                let first_visit = !declared_lengths.contains_key(&reference.digest());
                let admitted_distinct_objects = if first_visit {
                    let attempted = report.distinct_objects.checked_add(1).ok_or_else(|| {
                        format_error(CanonicalFormatError::CounterOverflow("incremental objects"))
                    })?;
                    check_limit(
                        "incremental objects",
                        limits.structural.max_distinct_objects,
                        attempted,
                    )?;
                    Some(attempted)
                } else {
                    None
                };
                let admitted_canonical_bytes = if first_visit {
                    let attempted = report
                        .canonical_bytes
                        .checked_add(reference.encoded_len())
                        .ok_or_else(|| {
                            format_error(CanonicalFormatError::CounterOverflow(
                                "incremental canonical bytes",
                            ))
                        })?;
                    check_limit(
                        "incremental canonical bytes",
                        limits.structural.max_canonical_bytes,
                        attempted,
                    )?;
                    Some(attempted)
                } else {
                    None
                };
                if let Some(declared_len) = declared_lengths.get(&reference.digest()).copied() {
                    if declared_len != reference.encoded_len() {
                        return Err(format_error(CanonicalFormatError::ObjectLengthMismatch {
                            expected: declared_len,
                            actual: reference.encoded_len(),
                        }));
                    }
                }
                if active.contains(&reference.digest()) {
                    return Err(format_error(CanonicalFormatError::ObjectCycle(
                        reference.digest(),
                    )));
                }
                active.try_reserve(1).map_err(|_| {
                    format_error(CanonicalFormatError::AllocationFailure { requested: 1 })
                })?;
                active.insert(reference.digest());
                if first_visit {
                    declared_lengths.try_reserve(1).map_err(|_| {
                        format_error(CanonicalFormatError::AllocationFailure { requested: 1 })
                    })?;
                    declared_lengths.insert(reference.digest(), reference.encoded_len());
                }

                let canonical = bytes.as_ref();
                if first_visit {
                    let actual_len = checked_len(canonical.len(), "incremental closure object")
                        .map_err(format_error)?;
                    if actual_len != reference.encoded_len() {
                        return Err(format_error(CanonicalFormatError::ObjectLengthMismatch {
                            expected: reference.encoded_len(),
                            actual: actual_len,
                        }));
                    }
                    let actual = ObjectDigest::of_canonical_bytes(canonical);
                    if actual != reference.digest() {
                        return Err(format_error(CanonicalFormatError::DigestMismatch {
                            expected: reference.digest(),
                            actual,
                        }));
                    }
                    report.distinct_objects = admitted_distinct_objects
                        .expect("first visits preflight incremental objects");
                    report.canonical_bytes = admitted_canonical_bytes
                        .expect("first visits preflight incremental canonical bytes");
                }

                let object = CanonicalObjectView::parse(canonical).map_err(format_error)?;
                push_incremental_work(
                    &mut work,
                    IncrementalWork::Exit(key),
                    limits.structural,
                    &mut report,
                )?;
                match expectation {
                    Expectation::Manifest => {
                        let CanonicalObjectView::SemanticManifest(manifest) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::SemanticManifest,
                                object.kind(),
                            )));
                        };
                        schedule_incremental_manifest_roots(
                            manifest,
                            &mut work,
                            limits.structural,
                            &mut report,
                        )?;
                    }
                    Expectation::Node {
                        namespace,
                        policy,
                        root,
                    } => {
                        let CanonicalObjectView::DagNode(node) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::DagNode,
                                object.kind(),
                            )));
                        };
                        if first_visit {
                            increment(&mut report.node_objects, 1, "incremental node objects")?;
                        }
                        validate_node_context(node, namespace, policy, root)
                            .map_err(format_error)?;
                        let mut node_path = key.path;
                        provenance
                            .paths
                            .extend(&mut node_path, node.prefix(), limits.max_exact_path_chunks)
                            .map_err(format_error)?;
                        if let EndpointValue::Content(content) = node.endpoint() {
                            let EndpointPolicy::Content(kind) = policy else {
                                return Err(format_error(CanonicalFormatError::EndpointPolicy {
                                    namespace: namespace.name(),
                                    endpoint: node.endpoint().name(),
                                }));
                            };
                            increment(&mut report.references, 1, "incremental references")?;
                            check_limit(
                                "incremental references",
                                limits.structural.max_references,
                                report.references,
                            )?;
                            push_incremental_work(
                                &mut work,
                                IncrementalWork::Enter(IncrementalVisitKey {
                                    reference: content,
                                    expectation: Expectation::Blob { namespace, kind },
                                    path: node_path,
                                }),
                                limits.structural,
                                &mut report,
                            )?;
                        }
                        for child in node.children().iter() {
                            increment(&mut report.references, 1, "incremental references")?;
                            check_limit(
                                "incremental references",
                                limits.structural.max_references,
                                report.references,
                            )?;
                            let mut child_path = node_path;
                            provenance
                                .paths
                                .extend(
                                    &mut child_path,
                                    &[child.edge],
                                    limits.max_exact_path_chunks,
                                )
                                .map_err(format_error)?;
                            push_incremental_work(
                                &mut work,
                                IncrementalWork::Enter(IncrementalVisitKey {
                                    reference: child.target,
                                    expectation: Expectation::Node {
                                        namespace,
                                        policy,
                                        root: false,
                                    },
                                    path: child_path,
                                }),
                                limits.structural,
                                &mut report,
                            )?;
                        }
                    }
                    Expectation::Blob { namespace, kind } => {
                        let CanonicalObjectView::Blob(blob) = object else {
                            return Err(format_error(unexpected(
                                reference.digest(),
                                CanonicalObjectKind::Blob,
                                object.kind(),
                            )));
                        };
                        if blob.kind() != kind {
                            return Err(format_error(CanonicalFormatError::PayloadKind {
                                namespace: namespace.name(),
                                expected: kind.name(),
                                actual: blob.kind().name(),
                            }));
                        }
                        if first_visit {
                            increment(
                                &mut report.payload_objects,
                                1,
                                "incremental payload objects",
                            )?;
                        }
                    }
                }
            }
        }
    }

    for reference in store.staged_references().map_err(ClosureError::Store)? {
        let reachable = declared_lengths
            .get(&reference.digest())
            .is_some_and(|declared_len| *declared_len == reference.encoded_len());
        if !reachable {
            return Err(format_error(CanonicalFormatError::SemanticCoherence {
                detail: "transaction staged an object outside the target delta closure".to_owned(),
            }));
        }
    }
    Ok(report)
}

const fn expectation_boundary(
    expectation: Expectation,
) -> Option<(SemanticNamespace, InheritedBoundaryKind)> {
    match expectation {
        Expectation::Manifest => None,
        Expectation::Node { namespace, .. } => Some((namespace, InheritedBoundaryKind::NodeEntry)),
        Expectation::Blob { namespace, .. } => {
            Some((namespace, InheritedBoundaryKind::ContentAtEndpoint))
        }
    }
}

fn schedule_incremental_manifest_roots<E>(
    manifest: SemanticStateManifestView,
    work: &mut Vec<IncrementalWork>,
    limits: StructuralValidationLimits,
    report: &mut StructuralClosureReport,
) -> Result<(), ClosureError<E>> {
    for (namespace, reference) in manifest.roots().iter().into_iter().rev() {
        increment(&mut report.references, 1, "incremental references")?;
        check_limit("incremental references", limits.max_references, report.references)?;
        push_incremental_work(
            work,
            IncrementalWork::Enter(IncrementalVisitKey {
                reference,
                expectation: Expectation::Node {
                    namespace,
                    policy: endpoint_policy(namespace),
                    root: true,
                },
                path: ExactPath::empty(),
            }),
            limits,
            report,
        )?;
    }
    Ok(())
}

fn schedule_manifest_roots<E>(
    manifest: SemanticStateManifestView,
    work: &mut Vec<Work>,
    limits: StructuralValidationLimits,
    report: &mut StructuralClosureReport,
    credit: ClosureCreditV1,
) -> Result<(), ClosureError<E>> {
    for (namespace, root) in manifest.roots().iter().into_iter().rev() {
        increment(&mut report.references, 1, "object references")?;
        check_limit("object references", limits.max_references, report.references)?;
        push_work(
            work,
            Work::Enter(
                root,
                Expectation::Node {
                    namespace,
                    policy: endpoint_policy(namespace),
                    root: true,
                },
                credit,
            ),
            limits,
            report,
        )?;
    }
    Ok(())
}

const fn endpoint_policy(namespace: SemanticNamespace) -> EndpointPolicy {
    match namespace {
        SemanticNamespace::DataBag | SemanticNamespace::JoinBag => EndpointPolicy::Multiplicity,
        SemanticNamespace::ContinuationMap => {
            EndpointPolicy::Content(CanonicalBlobKind::ContinuationSourceV1)
        }
    }
}

fn validate_node_context(
    node: CanonicalDagNodeView<'_>,
    namespace: SemanticNamespace,
    policy: EndpointPolicy,
    root: bool,
) -> Result<(), CanonicalFormatError> {
    let empty = node.prefix().is_empty() &&
        matches!(node.endpoint(), EndpointValue::Absent) &&
        node.children().is_empty();
    if empty && !root {
        return Err(CanonicalFormatError::EmptyNode);
    }
    let endpoint_valid = match (policy, node.endpoint()) {
        (_, EndpointValue::Absent) => true,
        (EndpointPolicy::Multiplicity, EndpointValue::Multiplicity(_)) |
        (EndpointPolicy::Content(_), EndpointValue::Content(_)) => true,
        _ => false,
    };
    if !endpoint_valid {
        return Err(CanonicalFormatError::EndpointPolicy {
            namespace: namespace.name(),
            endpoint: node.endpoint().name(),
        });
    }
    Ok(())
}

fn load_reference<S>(
    store: &S,
    reference: ObjectRefV1,
) -> Result<S::Object, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let bytes = store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .map_err(ClosureError::Store)?
        .ok_or_else(|| format_error(CanonicalFormatError::MissingObject(reference.digest())))?;
    let canonical = bytes.as_ref();
    let actual_len = checked_len(canonical.len(), "closure object").map_err(format_error)?;
    if actual_len != reference.encoded_len() {
        return Err(format_error(CanonicalFormatError::ObjectLengthMismatch {
            expected: reference.encoded_len(),
            actual: actual_len,
        }));
    }
    let actual = ObjectDigest::of_canonical_bytes(canonical);
    if actual != reference.digest() {
        return Err(format_error(CanonicalFormatError::DigestMismatch {
            expected: reference.digest(),
            actual,
        }));
    }
    Ok(bytes)
}

fn unexpected(
    digest: ObjectDigest,
    expected: CanonicalObjectKind,
    actual: CanonicalObjectKind,
) -> CanonicalFormatError {
    CanonicalFormatError::UnexpectedObjectKind {
        digest,
        expected: expected.name(),
        actual: actual.name(),
    }
}

fn increment<E>(counter: &mut u64, amount: u64, name: &'static str) -> Result<(), ClosureError<E>> {
    *counter = counter
        .checked_add(amount)
        .ok_or_else(|| format_error(CanonicalFormatError::CounterOverflow(name)))?;
    Ok(())
}

fn push_work<E>(
    work: &mut Vec<Work>,
    item: Work,
    limits: StructuralValidationLimits,
    report: &mut StructuralClosureReport,
) -> Result<(), ClosureError<E>> {
    let attempted_len = work
        .len()
        .checked_add(1)
        .ok_or_else(|| format_error(CanonicalFormatError::CounterOverflow("closure worklist")))?;
    let attempted = checked_len(attempted_len, "closure worklist").map_err(format_error)?;
    check_limit("closure worklist", limits.max_worklist, attempted)?;
    work.try_reserve(1).map_err(|_| {
        format_error(CanonicalFormatError::AllocationFailure {
            requested: attempted,
        })
    })?;
    work.push(item);
    report.maximum_worklist = report.maximum_worklist.max(attempted);
    Ok(())
}

fn push_incremental_work<E>(
    work: &mut Vec<IncrementalWork>,
    item: IncrementalWork,
    limits: StructuralValidationLimits,
    report: &mut StructuralClosureReport,
) -> Result<(), ClosureError<E>> {
    let attempted_len = work.len().checked_add(1).ok_or_else(|| {
        format_error(CanonicalFormatError::CounterOverflow("incremental closure worklist"))
    })?;
    let attempted =
        checked_len(attempted_len, "incremental closure worklist").map_err(format_error)?;
    check_limit("incremental closure worklist", limits.max_worklist, attempted)?;
    work.try_reserve(1).map_err(|_| {
        format_error(CanonicalFormatError::AllocationFailure {
            requested: attempted,
        })
    })?;
    work.push(item);
    report.maximum_worklist = report.maximum_worklist.max(attempted);
    Ok(())
}

fn insert_incremental_visit(
    completed: &mut HashSet<IncrementalVisitKey>,
    key: IncrementalVisitKey,
) -> Result<(), CanonicalFormatError> {
    if !completed.contains(&key) {
        completed
            .try_reserve(1)
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested: 1 })?;
        completed.insert(key);
    }
    Ok(())
}

fn check_limit<E>(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), ClosureError<E>> {
    if attempted > limit {
        Err(format_error(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        }))
    } else {
        Ok(())
    }
}

fn format_error<E>(error: CanonicalFormatError) -> ClosureError<E> { ClosureError::Format(error) }
