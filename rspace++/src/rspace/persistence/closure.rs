use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::manifest::{SemanticNamespace, SemanticStateManifestView};
use super::object::{
    CanonicalBlobKind, CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView,
    EndpointValue,
};
use super::store::ImmutableObjectStore;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EndpointPolicy {
    Multiplicity,
    Content(CanonicalBlobKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Debug)]
struct DigestState {
    declared_len: u64,
    active: bool,
    counted: bool,
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
                state.verified.push(expectation);
            }
            Work::Enter(reference, expectation) => {
                let state = states
                    .entry(reference.digest())
                    .or_insert_with(|| DigestState {
                        declared_len: reference.encoded_len(),
                        active: false,
                        counted: false,
                        verified: Vec::new(),
                    });
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
                let first_visit = !state.counted;
                state.counted = true;

                let bytes = load_reference(store, reference)?;
                let bytes = bytes.as_ref();
                if first_visit {
                    increment(&mut report.distinct_objects, 1, "distinct objects")?;
                    check_limit(
                        "distinct objects",
                        limits.max_distinct_objects,
                        report.distinct_objects,
                    )?;
                    increment(
                        &mut report.canonical_bytes,
                        checked_len(bytes.len(), "closure object").map_err(format_error)?,
                        "canonical bytes",
                    )?;
                    check_limit(
                        "canonical bytes",
                        limits.max_canonical_bytes,
                        report.canonical_bytes,
                    )?;
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
    Ok(report)
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
        .get(&reference.digest())
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
