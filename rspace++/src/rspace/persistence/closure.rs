use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::manifest::{SemanticNamespace, SemanticStateManifestView};
use super::object::{
    CanonicalBlobKind, CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView,
    EndpointValue,
};
use super::rewrite::{ExactPath, InheritedBoundaryKind, InheritedObjectRef, RewriteProvenance};
use super::store::{ImmutableObjectStore, StagingObjectStore, StagingStoreError};

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

#[derive(Clone, Copy, Debug)]
enum Work {
    Enter(ObjectRefV1, Expectation),
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

#[derive(Debug)]
struct DigestState {
    declared_len: u64,
    active: bool,
    verified: Vec<Expectation>,
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
    verify_structural_closure_report_with_limits(store, manifest, limits)
}

fn verify_structural_closure_report_with_limits<S>(
    store: &S,
    manifest: ObjectRefV1,
    limits: StructuralValidationLimits,
) -> Result<StructuralClosureReport, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut report = StructuralClosureReport::default();
    let mut work = Vec::new();
    push_work(&mut work, Work::Enter(manifest, Expectation::Manifest), limits, &mut report)?;
    let mut states = HashMap::<ObjectDigest, DigestState>::new();

    while let Some(next) = work.pop() {
        match next {
            Work::Exit(digest, expectation) => {
                let state = states
                    .get_mut(&digest)
                    .expect("an exit follows an entered object");
                state.active = false;
                let attempted_len = state.verified.len().checked_add(1).ok_or_else(|| {
                    format_error(CanonicalFormatError::CounterOverflow(
                        "verified object expectations",
                    ))
                })?;
                let attempted = checked_len(attempted_len, "verified object expectations")
                    .map_err(format_error)?;
                state.verified.try_reserve(1).map_err(|_| {
                    format_error(CanonicalFormatError::AllocationFailure {
                        requested: attempted,
                    })
                })?;
                state.verified.push(expectation);
            }
            Work::Enter(reference, expectation) => {
                let digest = reference.digest();
                let first_visit = !states.contains_key(&digest);
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
                if first_visit {
                    states.try_reserve(1).map_err(|_| {
                        format_error(CanonicalFormatError::AllocationFailure {
                            requested: admitted_distinct_objects
                                .expect("first visits preflight distinct objects"),
                        })
                    })?;
                    states.insert(digest, DigestState {
                        declared_len: reference.encoded_len(),
                        active: false,
                        verified: Vec::new(),
                    });
                }
                let state = states
                    .get_mut(&digest)
                    .expect("a digest state is present after first-visit admission");
                if state.declared_len != reference.encoded_len() {
                    return Err(format_error(CanonicalFormatError::ObjectLengthMismatch {
                        expected: state.declared_len,
                        actual: reference.encoded_len(),
                    }));
                }
                if state.verified.contains(&expectation) {
                    continue;
                }
                if state.active {
                    return Err(format_error(CanonicalFormatError::ObjectCycle(
                        reference.digest(),
                    )));
                }
                state.active = true;
                let bytes = load_reference(store, reference)?;
                let bytes = bytes.as_ref();
                if first_visit {
                    report.distinct_objects =
                        admitted_distinct_objects.expect("first visits preflight distinct objects");
                    report.canonical_bytes =
                        admitted_canonical_bytes.expect("first visits preflight canonical bytes");
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
                        schedule_manifest_roots(manifest, &mut work, limits, &mut report)?;
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
                                Work::Enter(content, Expectation::Blob { namespace, kind }),
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
                                Work::Enter(child.target, Expectation::Node {
                                    namespace,
                                    policy,
                                    root: false,
                                }),
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
    debug_assert!(states.values().all(|state| !state.active));
    Ok(report)
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
) -> Result<(), ClosureError<E>> {
    for (namespace, root) in manifest.roots().iter().into_iter().rev() {
        increment(&mut report.references, 1, "object references")?;
        check_limit("object references", limits.max_references, report.references)?;
        push_work(
            work,
            Work::Enter(root, Expectation::Node {
                namespace,
                policy: endpoint_policy(namespace),
                root: true,
            }),
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
