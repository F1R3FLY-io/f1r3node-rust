//! Semantic validation for canonical RSpace DAG images.
//!
//! Structural closure validation proves that the immutable object graph is
//! canonical and complete.  This module supplies the separate language-level
//! proof required before a manifest can become authoritative: every trie
//! endpoint must be an exact frozen RhoPathV1 record, continuation source
//! payloads must round-trip exactly, and every continuation must have the
//! corresponding JoinBag memberships.  Traversal and lookup are explicit
//! pushdown machines; neither term depth nor trie depth consumes native stack.

use std::collections::{BTreeSet, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration, ZipperValues};
use serde::Serialize;

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::closure::{
    ClosureError, RetainedUnionClosureLimitsV1, StructuralClosureLedgerV1, StructuralClosureReport,
    StructuralValidationLimits, StructurallyAdmittedStateV1,
    verify_structural_closure_in_ledger_with_limits,
};
use super::manifest::{SemanticNamespace, SemanticProfileId, SemanticRoots};
use super::mutation::MutationBatchViewV1;
use super::object::{CanonicalBlobKind, CanonicalObjectKind, CanonicalObjectView, EndpointValue};
use super::store::ImmutableObjectStore;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, cold_decode_vec_prefix, legacy_prefix,
};
use crate::rspace::serializers::serializers::{
    decode_datum, encode_continuation_identity_into, encode_continuation_source_into, encode_datum,
    encode_join_record_into,
};
use crate::rspace::trace::event::Consume;

/// Closed registry hook for semantic codec profiles.  Persistence entry
/// points accept a profile type, never four independently chosen decoders.
pub mod semantic_profile_seal {
    pub trait Sealed {}
}

/// One stable interpretation of the canonical RSpace key and payload bytes.
///
/// The identifier is embedded in every semantic manifest and later WAL
/// authority record.  Associated types therefore cannot be erased between
/// full admission, local incremental transitions, recovery, and publication.
pub trait SemanticProfile: semantic_profile_seal::Sealed + Send + Sync + 'static {
    type Channel: ColdStoreDecode + RhoPathEncode;
    type Pattern: Clone + ColdStoreDecode + StableHashSerialize;
    type Datum: Clone + ColdStoreDecode + RhoPathEncode + Serialize;
    type Continuation: Clone + ColdStoreDecode + StableHashSerialize;

    const ID: SemanticProfileId;
}

/// The authenticated semantic profile used by a concrete RSpace type family.
///
/// The persistence layer is intentionally generic over RSpace's four value
/// types, but production admission must still bind all four codecs as one
/// inseparable interpretation.  This profile supplies that binding without
/// introducing a second representation or requiring the durable backend to
/// know about a particular application-level type alias.  The profile ID is
/// part of every semantic manifest, so a manifest cannot be admitted under a
/// different codec family by accident.
pub struct RSpaceSemanticProfile<C, P, A, K>(PhantomData<fn() -> (C, P, A, K)>);

const RSPACE_SEMANTIC_PROFILE_ID: SemanticProfileId =
    SemanticProfileId::from_array(*b"RSpace.Rust.V1\0\0");

impl<C, P, A, K> semantic_profile_seal::Sealed for RSpaceSemanticProfile<C, P, A, K> {}

impl<C, P, A, K> SemanticProfile for RSpaceSemanticProfile<C, P, A, K>
where
    C: ColdStoreDecode + RhoPathEncode + Send + Sync + 'static,
    P: Clone + ColdStoreDecode + StableHashSerialize + Send + Sync + 'static,
    A: Clone + ColdStoreDecode + RhoPathEncode + Serialize + Send + Sync + 'static,
    K: Clone + ColdStoreDecode + StableHashSerialize + Send + Sync + 'static,
{
    type Channel = C;
    type Pattern = P;
    type Datum = A;
    type Continuation = K;

    const ID: SemanticProfileId = RSPACE_SEMANTIC_PROFILE_ID;
}

/// Caller-selected admission budgets.
///
/// These are resource ceilings, not traversal-depth limits.  Trie and term
/// depth remain unbounded by the algorithm; a caller can select budgets that
/// reflect the available memory and the size advertised by its pack catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticValidationLimits {
    pub structural: StructuralValidationLimits,
    pub max_logical_nodes: u64,
    pub max_endpoints: u64,
    pub max_logical_path_bytes: u64,
    pub max_payload_bytes: u64,
    pub max_join_lookups: u64,
    pub max_join_lookup_steps: u64,
    pub max_worklist: u64,
}

impl SemanticValidationLimits {
    pub const fn unbounded() -> Self {
        Self {
            structural: StructuralValidationLimits::unbounded(),
            max_logical_nodes: u64::MAX,
            max_endpoints: u64::MAX,
            max_logical_path_bytes: u64::MAX,
            max_payload_bytes: u64::MAX,
            max_join_lookups: u64::MAX,
            max_join_lookup_steps: u64::MAX,
            max_worklist: u64::MAX,
        }
    }
}

impl Default for SemanticValidationLimits {
    fn default() -> Self { Self::unbounded() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemanticValidationReport {
    pub structural: StructuralClosureReport,
    pub logical_nodes: u64,
    pub endpoints: u64,
    pub data_entries: u64,
    pub continuation_entries: u64,
    pub join_entries: u64,
    pub source_payload_bytes: u64,
    pub join_lookups: u64,
    pub join_lookup_steps: u64,
    pub maximum_logical_path_bytes: u64,
    pub maximum_worklist: u64,
}

/// Non-forgeable completion token for one exact manifest.  The report remains
/// available for diagnostics, while transaction authority can be constructed
/// only from this token rather than from caller-filled counters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SemanticallyAdmittedStateV1 {
    manifest: ObjectRefV1,
    report: SemanticValidationReport,
}

impl SemanticallyAdmittedStateV1 {
    pub(crate) const fn manifest(self) -> ObjectRefV1 { self.manifest }

    pub(crate) const fn report(self) -> SemanticValidationReport { self.report }
}

/// Work ceilings for one locally produced, verified-base transition.
///
/// These counters scale with changed keys, new payloads, and affected
/// cross-root probes.  They deliberately contain no whole-state count and no
/// depth ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementalSemanticLimits {
    pub max_changed_endpoints: u64,
    pub max_key_bytes: u64,
    pub max_payload_bytes: u64,
    pub max_join_lookups: u64,
    pub max_join_lookup_steps: u64,
    pub max_continuation_prefix_probes: u64,
    pub max_continuation_prefix_steps: u64,
}

impl IncrementalSemanticLimits {
    pub const UNBOUNDED: Self = Self {
        max_changed_endpoints: u64::MAX,
        max_key_bytes: u64::MAX,
        max_payload_bytes: u64::MAX,
        max_join_lookups: u64::MAX,
        max_join_lookup_steps: u64::MAX,
        max_continuation_prefix_probes: u64::MAX,
        max_continuation_prefix_steps: u64::MAX,
    };
}

impl Default for IncrementalSemanticLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncrementalSemanticReport {
    pub changed_endpoints: u64,
    pub changed_data: u64,
    pub changed_continuations: u64,
    pub changed_joins: u64,
    pub key_bytes: u64,
    pub source_payload_bytes: u64,
    pub join_lookups: u64,
    pub join_lookup_steps: u64,
    pub continuation_prefix_probes: u64,
    pub continuation_prefix_steps: u64,
}

pub(crate) struct PreparedSemanticDelta {
    required_joins: PathMap<()>,
    vacated_join_groups: PathMap<()>,
    report: IncrementalSemanticReport,
}

pub(crate) fn prepare_semantic_delta<P, S>(
    store: &S,
    mutation: MutationBatchViewV1<'_>,
    staged_sources: &HashSet<ObjectRefV1>,
    limits: IncrementalSemanticLimits,
) -> Result<PreparedSemanticDelta, ClosureError<S::Error>>
where
    P: SemanticProfile,
    S: ImmutableObjectStore,
{
    let mut report = IncrementalSemanticReport::default();
    let mut required_joins = PathMap::<()>::new();
    let mut vacated_join_groups = PathMap::<()>::new();
    let mut required_sources = HashSet::<ObjectRefV1>::new();

    for entry in mutation.data() {
        charge_delta_entry(entry.key, SemanticNamespace::DataBag, &mut report, limits)?;
        if entry.transition.after() != 0 {
            validate_data_key::<P::Channel, P::Datum, S::Error>(entry.key)?;
        }
    }

    for entry in mutation.continuations() {
        charge_delta_entry(entry.key, SemanticNamespace::ContinuationMap, &mut report, limits)?;
        let Some(source_ref) = entry.transition.after() else {
            continue;
        };
        if !required_sources.contains(&source_ref) {
            required_sources.try_reserve(1).map_err(|_| {
                format_error(CanonicalFormatError::AllocationFailure { requested: 1 })
            })?;
            required_sources.insert(source_ref);
        }
        if !staged_sources.contains(&source_ref) {
            return Err(format_error(CanonicalFormatError::SemanticCoherence {
                detail: "continuation target source was not supplied to the exact transaction"
                    .to_owned(),
            }));
        }
        let validated = validate_continuation_record::<P::Channel, P::Pattern, P::Continuation, S>(
            store,
            entry.key,
            source_ref,
            limits.max_payload_bytes,
            &mut report.source_payload_bytes,
        )?;
        for channel in &validated.channels {
            let mut key = RhoPathV1::join_prefix(channel);
            key.extend_from_slice(&validated.join_record);
            required_joins.insert(&key, ());
        }
    }

    for entry in mutation.joins() {
        charge_delta_entry(entry.key, SemanticNamespace::JoinBag, &mut report, limits)?;
        let channels = validate_join_key::<P::Channel, S::Error>(entry.key)?;
        if entry.transition.after() == 0 {
            let prefix = RhoPathV1::continuation_prefix(&channels);
            vacated_join_groups.insert(&prefix, ());
        }
    }

    if required_sources != *staged_sources {
        return Err(format_error(CanonicalFormatError::SemanticCoherence {
            detail: "transaction contains an unreferenced continuation-source object".to_owned(),
        }));
    }

    Ok(PreparedSemanticDelta {
        required_joins,
        vacated_join_groups,
        report,
    })
}

pub(crate) fn complete_semantic_delta<S>(
    store: &S,
    roots: SemanticRoots,
    mut prepared: PreparedSemanticDelta,
    limits: IncrementalSemanticLimits,
) -> Result<IncrementalSemanticReport, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    try_visit_paths(&prepared.required_joins, |key| {
        increment(&mut prepared.report.join_lookups, 1, "incremental join lookups")?;
        check_limit(
            "incremental join lookups",
            limits.max_join_lookups,
            prepared.report.join_lookups,
        )?;
        let found = lookup_endpoint_counted(
            store,
            roots.join_bag,
            key,
            limits.max_join_lookup_steps,
            &mut prepared.report.join_lookup_steps,
            "incremental join lookup steps",
        )?;
        if !matches!(found, Some(EndpointValue::Multiplicity(_))) {
            return Err(format_error(CanonicalFormatError::SemanticCoherence {
                detail: "changed continuation has no target JoinBag membership for one of its \
                         distinct channels"
                    .to_owned(),
            }));
        }
        Ok(())
    })?;

    try_visit_paths(&prepared.vacated_join_groups, |prefix| {
        increment(
            &mut prepared.report.continuation_prefix_probes,
            1,
            "continuation prefix probes",
        )?;
        check_limit(
            "continuation prefix probes",
            limits.max_continuation_prefix_probes,
            prepared.report.continuation_prefix_probes,
        )?;
        if subtree_has_endpoint_counted(
            store,
            roots.continuation_map,
            prefix,
            limits.max_continuation_prefix_steps,
            &mut prepared.report.continuation_prefix_steps,
        )? {
            return Err(format_error(CanonicalFormatError::SemanticCoherence {
                detail: "removed JoinBag membership is still required by a target continuation"
                    .to_owned(),
            }));
        }
        Ok(())
    })?;

    Ok(prepared.report)
}

/// Validate a complete canonical RSpace image without projecting any trie to
/// a `Vec`, sorting endpoints, or recursively descending either the DAG or a
/// decoded Rholang value.
pub fn verify_rspace_semantics<P, S>(
    store: &S,
    manifest_ref: ObjectRefV1,
    limits: SemanticValidationLimits,
) -> Result<SemanticValidationReport, ClosureError<S::Error>>
where
    P: SemanticProfile,
    S: ImmutableObjectStore,
{
    let mut ledger = StructuralClosureLedgerV1::new();
    let structural = verify_structural_closure_in_ledger_with_limits(
        store,
        manifest_ref,
        limits.structural,
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )?;
    verify_rspace_semantics_after_structural::<P, S>(store, structural, limits)
        .map(SemanticallyAdmittedStateV1::report)
}

/// Complete semantic admission from the exact structural token produced by
/// the shared closure PDA. Durable recovery uses this transition so retained
/// inventory derivation and current-state semantics never repeat the
/// structural graph walk.
pub(crate) fn verify_rspace_semantics_after_structural<P, S>(
    store: &S,
    structural: StructurallyAdmittedStateV1,
    limits: SemanticValidationLimits,
) -> Result<SemanticallyAdmittedStateV1, ClosureError<S::Error>>
where
    P: SemanticProfile,
    S: ImmutableObjectStore,
{
    if structural.profile_id() != P::ID {
        return Err(format_error(CanonicalFormatError::SemanticProfileMismatch {
            expected: *P::ID.as_bytes(),
            actual: *structural.profile_id().as_bytes(),
        }));
    }
    let roots = structural.roots();
    let mut report = SemanticValidationReport {
        structural: structural.report(),
        ..SemanticValidationReport::default()
    };

    walk_trie(store, roots.data_bag, limits, &mut report, |path, endpoint, report| {
        validate_data_endpoint::<P::Channel, P::Datum, S::Error>(path, endpoint)?;
        increment(&mut report.data_entries, 1, "data entries")
    })?;

    walk_trie(store, roots.join_bag, limits, &mut report, |path, endpoint, report| {
        validate_join_endpoint::<P::Channel, S::Error>(path, endpoint)?;
        increment(&mut report.join_entries, 1, "join entries")
    })?;

    walk_trie(store, roots.continuation_map, limits, &mut report, |path, endpoint, report| {
        validate_continuation_endpoint::<P::Channel, P::Pattern, P::Continuation, S>(
            store,
            roots.join_bag,
            path,
            endpoint,
            limits,
            report,
        )?;
        increment(&mut report.continuation_entries, 1, "continuation entries")
    })?;

    Ok(SemanticallyAdmittedStateV1 {
        manifest: structural.manifest(),
        report,
    })
}

#[derive(Clone, Copy, Debug)]
struct TrieVisit {
    reference: ObjectRefV1,
    parent_path_len: usize,
    edge: Option<u8>,
}

fn walk_trie<S, F>(
    store: &S,
    root: ObjectRefV1,
    limits: SemanticValidationLimits,
    report: &mut SemanticValidationReport,
    mut validate_endpoint: F,
) -> Result<(), ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
    F: FnMut(
        &[u8],
        EndpointValue,
        &mut SemanticValidationReport,
    ) -> Result<(), ClosureError<S::Error>>,
{
    let mut path = Vec::<u8>::new();
    let mut work = vec![TrieVisit {
        reference: root,
        parent_path_len: 0,
        edge: None,
    }];

    while let Some(visit) = work.pop() {
        observe_max(
            &mut report.maximum_worklist,
            checked_len(
                work.len().checked_add(1).ok_or_else(|| {
                    format_error(CanonicalFormatError::CounterOverflow("semantic worklist"))
                })?,
                "semantic worklist",
            )
            .map_err(format_error)?,
        );
        check_limit("semantic worklist", limits.max_worklist, report.maximum_worklist)?;
        increment(&mut report.logical_nodes, 1, "logical nodes")?;
        check_limit("logical nodes", limits.max_logical_nodes, report.logical_nodes)?;

        path.truncate(visit.parent_path_len);
        let bytes = load_reference(store, visit.reference)?;
        let node = match CanonicalObjectView::parse(bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::DagNode(node) => node,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: visit.reference.digest(),
                    expected: CanonicalObjectKind::DagNode.name(),
                    actual: object.kind().name(),
                }));
            }
        };
        let additional = node
            .prefix()
            .len()
            .checked_add(usize::from(visit.edge.is_some()))
            .ok_or_else(|| format_error(CanonicalFormatError::CounterOverflow("semantic path")))?;
        let requested_path = path
            .len()
            .checked_add(additional)
            .ok_or_else(|| format_error(CanonicalFormatError::CounterOverflow("semantic path")))?;
        let requested_path = checked_len(requested_path, "semantic path").map_err(format_error)?;
        path.try_reserve(additional).map_err(|_| {
            format_error(CanonicalFormatError::AllocationFailure {
                requested: requested_path,
            })
        })?;
        if let Some(edge) = visit.edge {
            path.push(edge);
        }
        path.extend_from_slice(node.prefix());
        let path_len = checked_len(path.len(), "semantic path").map_err(format_error)?;
        observe_max(&mut report.maximum_logical_path_bytes, path_len);
        check_limit(
            "logical path bytes",
            limits.max_logical_path_bytes,
            report.maximum_logical_path_bytes,
        )?;

        if !matches!(node.endpoint(), EndpointValue::Absent) {
            increment(&mut report.endpoints, 1, "semantic endpoints")?;
            check_limit("semantic endpoints", limits.max_endpoints, report.endpoints)?;
            validate_endpoint(&path, node.endpoint(), report)?;
        }

        // Order is irrelevant to validation.  Pushing directly from the
        // borrowed adaptive-node iterator avoids allocating a child Vec; the
        // LIFO worklist merely visits canonical siblings in reverse order.
        let attempted_worklist_len =
            work.len()
                .checked_add(node.children().len())
                .ok_or_else(|| {
                    format_error(CanonicalFormatError::CounterOverflow("semantic worklist"))
                })?;
        let attempted_worklist =
            checked_len(attempted_worklist_len, "semantic worklist").map_err(format_error)?;
        check_limit("semantic worklist", limits.max_worklist, attempted_worklist)?;
        work.try_reserve(node.children().len()).map_err(|_| {
            format_error(CanonicalFormatError::AllocationFailure {
                requested: attempted_worklist,
            })
        })?;
        for child in node.children().iter() {
            work.push(TrieVisit {
                reference: child.target,
                parent_path_len: path.len(),
                edge: Some(child.edge),
            });
        }
    }
    Ok(())
}

fn validate_data_endpoint<C, A, E>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(), ClosureError<E>>
where
    C: ColdStoreDecode + RhoPathEncode,
    A: Clone + ColdStoreDecode + RhoPathEncode + Serialize,
{
    let EndpointValue::Multiplicity(_) = endpoint else {
        return Err(format_error(CanonicalFormatError::EndpointPolicy {
            namespace: SemanticNamespace::DataBag.name(),
            endpoint: endpoint.name(),
        }));
    };
    validate_data_key::<C, A, E>(path)
}

fn validate_data_key<C, A, E>(path: &[u8]) -> Result<(), ClosureError<E>>
where
    C: ColdStoreDecode + RhoPathEncode,
    A: Clone + ColdStoreDecode + RhoPathEncode + Serialize,
{
    let view = RhoPathV1::parse_datum(path)
        .map_err(|error| semantic_key(SemanticNamespace::DataBag, error.to_string()))?;
    let channel_identity = view.channel_identity();
    let channel = decode_exact::<C>(&channel_identity, "channel")
        .map_err(|detail| semantic_key(SemanticNamespace::DataBag, detail))?;
    if channel.rho_path_identity() != channel_identity {
        return Err(semantic_key(
            SemanticNamespace::DataBag,
            "channel identity does not re-encode exactly",
        ));
    }
    if channel.rho_path_structure() != view.channel_structure() {
        return Err(semantic_key(
            SemanticNamespace::DataBag,
            "channel structure disagrees with decoded channel",
        ));
    }

    let datum = decode_datum::<A>(view.datum_identity())
        .map_err(|error| semantic_key(SemanticNamespace::DataBag, error.to_string()))?;
    if encode_datum(&datum) != view.datum_identity() {
        return Err(semantic_key(
            SemanticNamespace::DataBag,
            "datum identity does not decode and re-encode exactly",
        ));
    }
    if datum.a.rho_path_structure() != view.datum_structure() {
        return Err(semantic_key(
            SemanticNamespace::DataBag,
            "datum structure disagrees with decoded datum",
        ));
    }
    Ok(())
}

fn validate_join_endpoint<C, E>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(), ClosureError<E>>
where
    C: ColdStoreDecode + RhoPathEncode,
{
    let EndpointValue::Multiplicity(_) = endpoint else {
        return Err(format_error(CanonicalFormatError::EndpointPolicy {
            namespace: SemanticNamespace::JoinBag.name(),
            endpoint: endpoint.name(),
        }));
    };
    validate_join_key::<C, E>(path).map(|_| ())
}

fn validate_join_key<C, E>(path: &[u8]) -> Result<Vec<C>, ClosureError<E>>
where C: ColdStoreDecode + RhoPathEncode {
    let view = RhoPathV1::parse_join(path)
        .map_err(|error| semantic_key(SemanticNamespace::JoinBag, error.to_string()))?;
    let indexed_identity = view.indexed_channel_identity();
    let indexed = decode_exact::<C>(&indexed_identity, "indexed channel")
        .map_err(|detail| semantic_key(SemanticNamespace::JoinBag, detail))?;
    if indexed.rho_path_identity() != indexed_identity {
        return Err(semantic_key(
            SemanticNamespace::JoinBag,
            "indexed channel does not re-encode exactly",
        ));
    }
    let (channels, consumed) = cold_decode_vec_prefix::<C>(view.join_identity())
        .map_err(|error| semantic_key(SemanticNamespace::JoinBag, error.to_string()))?;
    if consumed != view.join_identity().len() || channels.is_empty() {
        return Err(semantic_key(
            SemanticNamespace::JoinBag,
            "join record is empty or has trailing bytes",
        ));
    }
    let mut reencoded = Vec::new();
    encode_join_record_into(&channels, &mut reencoded);
    if reencoded != view.join_identity() {
        return Err(semantic_key(
            SemanticNamespace::JoinBag,
            "join record does not re-encode exactly",
        ));
    }
    if !channels
        .iter()
        .any(|channel| channel.rho_path_identity() == indexed_identity)
    {
        return Err(semantic_key(
            SemanticNamespace::JoinBag,
            "indexed channel is not a member of its join group",
        ));
    }
    Ok(channels)
}

fn validate_continuation_endpoint<C, P, K, S>(
    store: &S,
    join_root: ObjectRefV1,
    path: &[u8],
    endpoint: EndpointValue,
    limits: SemanticValidationLimits,
    report: &mut SemanticValidationReport,
) -> Result<(), ClosureError<S::Error>>
where
    C: ColdStoreDecode + RhoPathEncode,
    P: Clone + ColdStoreDecode + StableHashSerialize,
    K: Clone + ColdStoreDecode + StableHashSerialize,
    S: ImmutableObjectStore,
{
    let EndpointValue::Content(source_ref) = endpoint else {
        return Err(format_error(CanonicalFormatError::EndpointPolicy {
            namespace: SemanticNamespace::ContinuationMap.name(),
            endpoint: endpoint.name(),
        }));
    };
    let validated = validate_continuation_record::<C, P, K, S>(
        store,
        path,
        source_ref,
        limits.max_payload_bytes,
        &mut report.source_payload_bytes,
    )?;
    let mut distinct_channels = PathMap::<()>::new();
    for channel in &validated.channels {
        let identity = channel.rho_path_identity();
        if distinct_channels.insert(&identity, ()).is_some() {
            continue;
        }
        let mut join_key = RhoPathV1::join_prefix(channel);
        join_key.extend_from_slice(&validated.join_record);
        increment(&mut report.join_lookups, 1, "join lookups")?;
        check_limit("join lookups", limits.max_join_lookups, report.join_lookups)?;
        let found = lookup_endpoint(store, join_root, &join_key, limits, report)?;
        if !matches!(found, Some(EndpointValue::Multiplicity(_))) {
            return Err(format_error(CanonicalFormatError::SemanticCoherence {
                detail: "continuation has no JoinBag membership for one of its distinct channels"
                    .to_owned(),
            }));
        }
    }
    Ok(())
}

struct ValidatedContinuation<C, P: Clone, K: Clone> {
    channels: Vec<C>,
    join_record: Vec<u8>,
    waiting: WaitingContinuation<P, K>,
}

fn validate_continuation_record<C, P, K, S>(
    store: &S,
    path: &[u8],
    source_ref: ObjectRefV1,
    max_payload_bytes: u64,
    source_payload_bytes: &mut u64,
) -> Result<ValidatedContinuation<C, P, K>, ClosureError<S::Error>>
where
    C: ColdStoreDecode + RhoPathEncode,
    P: Clone + ColdStoreDecode + StableHashSerialize,
    K: Clone + ColdStoreDecode + StableHashSerialize,
    S: ImmutableObjectStore,
{
    let view = RhoPathV1::parse_continuation(path).map_err(|error| {
        semantic_key::<S::Error>(SemanticNamespace::ContinuationMap, error.to_string())
    })?;
    let channel_count = view.channel_identities().len();
    let requested_channels =
        checked_len(channel_count, "continuation channel count").map_err(format_error)?;
    let mut channels = Vec::<C>::new();
    channels.try_reserve(channel_count).map_err(|_| {
        format_error(CanonicalFormatError::AllocationFailure {
            requested: requested_channels,
        })
    })?;
    for identity in view.channel_identities() {
        let channel = decode_exact::<C>(&identity, "continuation channel").map_err(|detail| {
            semantic_key::<S::Error>(SemanticNamespace::ContinuationMap, detail)
        })?;
        if channel.rho_path_identity() != identity {
            return Err(semantic_key(
                SemanticNamespace::ContinuationMap,
                "channel identity does not re-encode exactly",
            ));
        }
        channels.push(channel);
    }

    let identity = view.continuation_identity();
    let (patterns, mut consumed) = cold_decode_vec_prefix::<P>(&identity).map_err(|error| {
        semantic_key::<S::Error>(SemanticNamespace::ContinuationMap, error.to_string())
    })?;
    let (continuation, used) = K::cold_decode_prefix(&identity[consumed..]).map_err(|error| {
        semantic_key::<S::Error>(SemanticNamespace::ContinuationMap, error.to_string())
    })?;
    consumed = consumed.checked_add(used).ok_or_else(|| {
        format_error(CanonicalFormatError::CounterOverflow("continuation identity offset"))
    })?;
    let ((persist, peeks), tail_used) =
        legacy_prefix::<(bool, BTreeSet<i32>)>(&identity[consumed..]).map_err(|error| {
            semantic_key::<S::Error>(SemanticNamespace::ContinuationMap, error.to_string())
        })?;
    consumed = consumed.checked_add(tail_used).ok_or_else(|| {
        format_error(CanonicalFormatError::CounterOverflow("continuation identity offset"))
    })?;
    if consumed != identity.len() {
        return Err(semantic_key(
            SemanticNamespace::ContinuationMap,
            "continuation identity has trailing bytes",
        ));
    }
    if patterns.len() != channels.len() {
        return Err(semantic_key(
            SemanticNamespace::ContinuationMap,
            format!(
                "pattern/channel arity differs: {} patterns for {} channels",
                patterns.len(),
                channels.len()
            ),
        ));
    }

    let source_bytes = load_reference(store, source_ref)?;
    let blob = match CanonicalObjectView::parse(source_bytes.as_ref()).map_err(format_error)? {
        CanonicalObjectView::Blob(blob)
            if blob.kind() == CanonicalBlobKind::ContinuationSourceV1 =>
        {
            blob
        }
        CanonicalObjectView::Blob(blob) => {
            return Err(format_error(CanonicalFormatError::PayloadKind {
                namespace: SemanticNamespace::ContinuationMap.name(),
                expected: CanonicalBlobKind::ContinuationSourceV1.name(),
                actual: blob.kind().name(),
            }));
        }
        object => {
            return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                digest: source_ref.digest(),
                expected: CanonicalObjectKind::Blob.name(),
                actual: object.kind().name(),
            }));
        }
    };
    let payload_len =
        checked_len(blob.payload().len(), "continuation source payload").map_err(format_error)?;
    increment(source_payload_bytes, payload_len, "continuation source payload bytes")?;
    check_limit("continuation source payload bytes", max_payload_bytes, *source_payload_bytes)?;
    let (source, source_used) = legacy_prefix::<Consume>(blob.payload()).map_err(|error| {
        semantic_payload::<S::Error>(SemanticNamespace::ContinuationMap, error.to_string())
    })?;
    if source_used != blob.payload().len() {
        return Err(semantic_payload(
            SemanticNamespace::ContinuationMap,
            "continuation source has trailing bytes",
        ));
    }

    let waiting = WaitingContinuation {
        patterns: Arc::new(patterns),
        continuation: Arc::new(continuation),
        persist,
        peeks,
        source,
    };
    let mut encoded_identity = Vec::new();
    encode_continuation_identity_into(&waiting, &mut encoded_identity);
    if encoded_identity != identity {
        return Err(semantic_key(
            SemanticNamespace::ContinuationMap,
            "continuation identity does not re-encode exactly",
        ));
    }
    let mut encoded_source = Vec::new();
    encode_continuation_source_into(&waiting, &mut encoded_source);
    if encoded_source != blob.payload() {
        return Err(semantic_payload(
            SemanticNamespace::ContinuationMap,
            "continuation source does not re-encode exactly",
        ));
    }

    let mut join_record = Vec::new();
    encode_join_record_into(&channels, &mut join_record);
    Ok(ValidatedContinuation {
        channels,
        join_record,
        waiting,
    })
}

/// Decode one authenticated DataBag endpoint without reconstructing a
/// channel-wide vector.  The canonical datum identity is already the exact
/// cold-store record bytes; this helper only decodes that one record and
/// preserves its endpoint multiplicity for the caller.
pub(crate) fn decode_data_endpoint<A: Clone + ColdStoreDecode>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(crate::rspace::internal::Datum<A>, std::num::NonZeroU64), CanonicalFormatError> {
    let view = RhoPathV1::parse_datum(path).map_err(|error| CanonicalFormatError::SemanticKey {
        namespace: SemanticNamespace::DataBag.name(),
        detail: error.to_string(),
    })?;
    let multiplicity = match endpoint {
        EndpointValue::Multiplicity(value) => value,
        other => {
            return Err(CanonicalFormatError::EndpointPolicy {
                namespace: SemanticNamespace::DataBag.name(),
                endpoint: other.name(),
            });
        }
    };
    let datum = decode_datum::<A>(view.datum_identity()).map_err(|error| {
        CanonicalFormatError::SemanticKey {
            namespace: SemanticNamespace::DataBag.name(),
            detail: error.to_string(),
        }
    })?;
    Ok((datum, multiplicity))
}

/// Decode one authenticated DataBag endpoint together with its structural
/// channel identity. The channel identity is checked for an exact
/// decode/re-encode round trip before the datum is returned.
pub(crate) fn decode_data_endpoint_with_channel<C, A>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(C, crate::rspace::internal::Datum<A>, std::num::NonZeroU64), CanonicalFormatError>
where
    C: ColdStoreDecode + RhoPathEncode,
    A: Clone + ColdStoreDecode,
{
    let view = RhoPathV1::parse_datum(path).map_err(|error| CanonicalFormatError::SemanticKey {
        namespace: SemanticNamespace::DataBag.name(),
        detail: error.to_string(),
    })?;
    let channel_identity = view.channel_identity();
    let channel = decode_exact::<C>(&channel_identity, "data channel").map_err(|detail| {
        CanonicalFormatError::SemanticKey {
            namespace: SemanticNamespace::DataBag.name(),
            detail,
        }
    })?;
    if channel.rho_path_identity() != channel_identity {
        return Err(CanonicalFormatError::SemanticKey {
            namespace: SemanticNamespace::DataBag.name(),
            detail: "data channel identity does not re-encode exactly".to_owned(),
        });
    }
    let (datum, multiplicity) = decode_data_endpoint(path, endpoint)?;
    Ok((channel, datum, multiplicity))
}

/// Decode one authenticated JoinBag endpoint.  The path carries the complete
/// ordered join record, so no legacy leaf projection or sorting is needed.
pub(crate) fn decode_join_endpoint<C: Clone + ColdStoreDecode>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(Vec<C>, std::num::NonZeroU64), CanonicalFormatError> {
    let view = RhoPathV1::parse_join(path).map_err(|error| CanonicalFormatError::SemanticKey {
        namespace: SemanticNamespace::JoinBag.name(),
        detail: error.to_string(),
    })?;
    let multiplicity = match endpoint {
        EndpointValue::Multiplicity(value) => value,
        other => {
            return Err(CanonicalFormatError::EndpointPolicy {
                namespace: SemanticNamespace::JoinBag.name(),
                endpoint: other.name(),
            });
        }
    };
    let join = crate::rspace::serializers::serializers::decode_join(view.join_identity()).map_err(
        |error| CanonicalFormatError::SemanticKey {
            namespace: SemanticNamespace::JoinBag.name(),
            detail: error.to_string(),
        },
    )?;
    Ok((join, multiplicity))
}

/// Decode one authenticated JoinBag endpoint together with its indexed
/// channel identity, checking the channel's exact identity round trip.
pub(crate) fn decode_join_endpoint_with_channel<C>(
    path: &[u8],
    endpoint: EndpointValue,
) -> Result<(C, Vec<C>, std::num::NonZeroU64), CanonicalFormatError>
where
    C: Clone + ColdStoreDecode + RhoPathEncode,
{
    let view = RhoPathV1::parse_join(path).map_err(|error| CanonicalFormatError::SemanticKey {
        namespace: SemanticNamespace::JoinBag.name(),
        detail: error.to_string(),
    })?;
    let indexed_identity = view.indexed_channel_identity();
    let indexed =
        decode_exact::<C>(&indexed_identity, "join indexed channel").map_err(|detail| {
            CanonicalFormatError::SemanticKey {
                namespace: SemanticNamespace::JoinBag.name(),
                detail,
            }
        })?;
    if indexed.rho_path_identity() != indexed_identity {
        return Err(CanonicalFormatError::SemanticKey {
            namespace: SemanticNamespace::JoinBag.name(),
            detail: "join indexed channel identity does not re-encode exactly".to_owned(),
        });
    }
    let (join, multiplicity) = decode_join_endpoint(path, endpoint)?;
    Ok((indexed, join, multiplicity))
}

/// Decode one authenticated ContinuationMap endpoint, including its exact
/// source blob.  The existing semantic validator performs all identity,
/// payload, and round-trip checks; this projection reuses that same machine
/// rather than maintaining a second decoder.
pub(crate) fn decode_continuation_endpoint<C, P, K, S>(
    store: &S,
    path: &[u8],
    endpoint: EndpointValue,
    max_payload_bytes: u64,
) -> Result<WaitingContinuation<P, K>, ClosureError<S::Error>>
where
    C: ColdStoreDecode + RhoPathEncode,
    P: Clone + ColdStoreDecode + StableHashSerialize,
    K: Clone + ColdStoreDecode + StableHashSerialize,
    S: ImmutableObjectStore,
{
    let source = match endpoint {
        EndpointValue::Content(reference) => reference,
        other => {
            return Err(format_error(CanonicalFormatError::EndpointPolicy {
                namespace: SemanticNamespace::ContinuationMap.name(),
                endpoint: other.name(),
            }));
        }
    };
    let mut source_payload_bytes = 0;
    Ok(validate_continuation_record::<C, P, K, S>(
        store,
        path,
        source,
        max_payload_bytes,
        &mut source_payload_bytes,
    )?
    .waiting)
}

/// Decode one authenticated continuation endpoint and retain its exact
/// ordered channel group for startup/state projections.
pub(crate) fn decode_continuation_endpoint_with_channels<C, P, K, S>(
    store: &S,
    path: &[u8],
    endpoint: EndpointValue,
    max_payload_bytes: u64,
) -> Result<(Vec<C>, WaitingContinuation<P, K>), ClosureError<S::Error>>
where
    C: ColdStoreDecode + RhoPathEncode,
    P: Clone + ColdStoreDecode + StableHashSerialize,
    K: Clone + ColdStoreDecode + StableHashSerialize,
    S: ImmutableObjectStore,
{
    let source = match endpoint {
        EndpointValue::Content(reference) => reference,
        other => {
            return Err(format_error(CanonicalFormatError::EndpointPolicy {
                namespace: SemanticNamespace::ContinuationMap.name(),
                endpoint: other.name(),
            }));
        }
    };
    let mut source_payload_bytes = 0;
    let validated = validate_continuation_record::<C, P, K, S>(
        store,
        path,
        source,
        max_payload_bytes,
        &mut source_payload_bytes,
    )?;
    Ok((validated.channels, validated.waiting))
}

fn lookup_endpoint<S>(
    store: &S,
    root: ObjectRefV1,
    key: &[u8],
    limits: SemanticValidationLimits,
    report: &mut SemanticValidationReport,
) -> Result<Option<EndpointValue>, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    lookup_endpoint_counted(
        store,
        root,
        key,
        limits.max_join_lookup_steps,
        &mut report.join_lookup_steps,
        "join lookup steps",
    )
}

pub(crate) fn lookup_endpoint_counted<S>(
    store: &S,
    root: ObjectRefV1,
    key: &[u8],
    max_steps: u64,
    steps: &mut u64,
    resource: &'static str,
) -> Result<Option<EndpointValue>, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut reference = root;
    let mut offset = 0usize;
    loop {
        increment(steps, 1, resource)?;
        check_limit(resource, max_steps, *steps)?;
        let bytes = load_reference(store, reference)?;
        let node = match CanonicalObjectView::parse(bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::DagNode(node) => node,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: reference.digest(),
                    expected: CanonicalObjectKind::DagNode.name(),
                    actual: object.kind().name(),
                }));
            }
        };
        let remaining = &key[offset..];
        if !remaining.starts_with(node.prefix()) {
            return Ok(None);
        }
        offset += node.prefix().len();
        if offset == key.len() {
            return Ok(
                (!matches!(node.endpoint(), EndpointValue::Absent)).then_some(node.endpoint())
            );
        }
        let edge = key[offset];
        offset += 1;
        let Some(next) = node.children().get(edge) else {
            return Ok(None);
        };
        reference = next;
    }
}

/// Visit endpoint paths below one canonical prefix using an explicit work
/// list. This is the allocation-bounded projection primitive: callers can
/// decode or stream each endpoint immediately instead of first materializing
/// a channel-wide `Vec`. The authoritative representation remains the
/// compressed DAG and no sorted intermediate collection is used to perform
/// the lookup.
pub(crate) fn visit_endpoints_under_prefix<S, F>(
    store: &S,
    root: ObjectRefV1,
    prefix: &[u8],
    max_steps: u64,
    max_endpoints: u64,
    mut visit: F,
) -> Result<(), ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
    F: FnMut(&[u8], EndpointValue) -> Result<(), ClosureError<S::Error>>,
{
    let mut reference = root;
    let mut offset = 0usize;
    let mut path = Vec::new();
    let mut steps = 0u64;
    loop {
        if prefix.is_empty() {
            break;
        }
        increment(&mut steps, 1, "durable prefix lookup steps")?;
        check_limit("durable prefix lookup steps", max_steps, steps)?;
        let bytes = load_reference(store, reference)?;
        let node = match CanonicalObjectView::parse(bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::DagNode(node) => node,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: reference.digest(),
                    expected: CanonicalObjectKind::DagNode.name(),
                    actual: object.kind().name(),
                }));
            }
        };
        let node_parent_path = path.clone();
        let remaining = &prefix[offset..];
        if node.prefix().starts_with(remaining) {
            // The requested prefix ends inside this compressed line. The
            // complete node subtree is therefore already within the query.
            path = node_parent_path;
            break;
        }
        if !remaining.starts_with(node.prefix()) {
            return Ok(());
        }
        path.extend_from_slice(node.prefix());
        offset += node.prefix().len();
        if offset == prefix.len() {
            path = node_parent_path;
            break;
        }
        let edge = prefix[offset];
        offset += 1;
        let Some(next) = node.children().get(edge) else {
            return Ok(());
        };
        path.push(edge);
        reference = next;
    }

    let mut work = vec![(reference, path)];
    let mut endpoint_count = 0u64;
    while let Some((reference, parent_path)) = work.pop() {
        increment(&mut steps, 1, "durable prefix traversal steps")?;
        check_limit("durable prefix traversal steps", max_steps, steps)?;
        let bytes = load_reference(store, reference)?;
        let node = match CanonicalObjectView::parse(bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::DagNode(node) => node,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: reference.digest(),
                    expected: CanonicalObjectKind::DagNode.name(),
                    actual: object.kind().name(),
                }));
            }
        };
        let mut node_path = parent_path;
        node_path.extend_from_slice(node.prefix());
        if !matches!(node.endpoint(), EndpointValue::Absent) {
            increment(&mut endpoint_count, 1, "durable prefix endpoints")?;
            check_limit("durable prefix endpoints", max_endpoints, endpoint_count)?;
            visit(&node_path, node.endpoint())?;
        }
        for child in node.children().iter() {
            let mut child_parent = node_path.clone();
            child_parent.push(child.edge);
            work.push((child.target, child_parent));
        }
    }
    Ok(())
}

/// Collect endpoint paths below one canonical prefix using an explicit work
/// list. This compatibility wrapper is retained for callers that need a
/// complete projection; new typed readers should use
/// `visit_endpoints_under_prefix` to avoid the intermediate endpoint vector.
pub(crate) fn collect_endpoints_under_prefix<S>(
    store: &S,
    root: ObjectRefV1,
    prefix: &[u8],
    max_steps: u64,
    max_endpoints: u64,
) -> Result<Vec<(Vec<u8>, EndpointValue)>, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut result = Vec::new();
    visit_endpoints_under_prefix(
        store,
        root,
        prefix,
        max_steps,
        max_endpoints,
        |path, endpoint| {
            result.push((path.to_vec(), endpoint));
            Ok(())
        },
    )?;
    Ok(result)
}

fn subtree_has_endpoint_counted<S>(
    store: &S,
    root: ObjectRefV1,
    prefix: &[u8],
    max_steps: u64,
    steps: &mut u64,
) -> Result<bool, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
{
    let mut reference = root;
    let mut offset = 0usize;
    loop {
        increment(steps, 1, "continuation prefix probe steps")?;
        check_limit("continuation prefix probe steps", max_steps, *steps)?;
        let bytes = load_reference(store, reference)?;
        let node = match CanonicalObjectView::parse(bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::DagNode(node) => node,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: reference.digest(),
                    expected: CanonicalObjectKind::DagNode.name(),
                    actual: object.kind().name(),
                }));
            }
        };
        let remaining = &prefix[offset..];
        if remaining.len() <= node.prefix().len() {
            if !node.prefix().starts_with(remaining) {
                return Ok(false);
            }
            return Ok(
                !matches!(node.endpoint(), EndpointValue::Absent) || !node.children().is_empty()
            );
        }
        if !remaining.starts_with(node.prefix()) {
            return Ok(false);
        }
        offset += node.prefix().len();
        let edge = prefix[offset];
        offset += 1;
        let Some(next) = node.children().get(edge) else {
            return Ok(false);
        };
        reference = next;
    }
}

fn charge_delta_entry<E>(
    key: &[u8],
    namespace: SemanticNamespace,
    report: &mut IncrementalSemanticReport,
    limits: IncrementalSemanticLimits,
) -> Result<(), ClosureError<E>> {
    increment(&mut report.changed_endpoints, 1, "changed semantic endpoints")?;
    check_limit(
        "changed semantic endpoints",
        limits.max_changed_endpoints,
        report.changed_endpoints,
    )?;
    match namespace {
        SemanticNamespace::DataBag => {
            increment(&mut report.changed_data, 1, "changed data endpoints")?;
        }
        SemanticNamespace::ContinuationMap => {
            increment(&mut report.changed_continuations, 1, "changed continuation endpoints")?;
        }
        SemanticNamespace::JoinBag => {
            increment(&mut report.changed_joins, 1, "changed join endpoints")?;
        }
    }
    let key_bytes = checked_len(key.len(), "incremental semantic key").map_err(format_error)?;
    increment(&mut report.key_bytes, key_bytes, "incremental semantic key bytes")?;
    check_limit("incremental semantic key bytes", limits.max_key_bytes, report.key_bytes)
}

fn try_visit_paths<E>(
    map: &PathMap<()>,
    mut visitor: impl FnMut(&[u8]) -> Result<(), ClosureError<E>>,
) -> Result<(), ClosureError<E>> {
    let mut cursor = map.read_zipper_at_borrowed_path(&[]);
    if cursor.val().is_some() {
        visitor(cursor.path())?;
    }
    while cursor.to_next_get_val().is_some() {
        visitor(cursor.path())?;
    }
    Ok(())
}

pub(crate) fn load_reference<S>(
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
    let actual_len = checked_len(canonical.len(), "semantic object").map_err(format_error)?;
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

fn decode_exact<T: ColdStoreDecode>(bytes: &[u8], label: &str) -> Result<T, String> {
    let (value, consumed) = T::cold_decode_prefix(bytes).map_err(|error| error.to_string())?;
    if consumed != bytes.len() {
        return Err(format!("{label} has trailing bytes"));
    }
    Ok(value)
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

fn increment<E>(counter: &mut u64, amount: u64, name: &'static str) -> Result<(), ClosureError<E>> {
    *counter = counter
        .checked_add(amount)
        .ok_or_else(|| format_error(CanonicalFormatError::CounterOverflow(name)))?;
    Ok(())
}

fn observe_max(target: &mut u64, observed: u64) { *target = (*target).max(observed); }

fn semantic_key<E>(namespace: SemanticNamespace, detail: impl Into<String>) -> ClosureError<E> {
    format_error(CanonicalFormatError::SemanticKey {
        namespace: namespace.name(),
        detail: detail.into(),
    })
}

fn semantic_payload<E>(namespace: SemanticNamespace, detail: impl Into<String>) -> ClosureError<E> {
    format_error(CanonicalFormatError::SemanticPayload {
        namespace: namespace.name(),
        detail: detail.into(),
    })
}

const fn format_error<E>(error: CanonicalFormatError) -> ClosureError<E> {
    ClosureError::Format(error)
}
