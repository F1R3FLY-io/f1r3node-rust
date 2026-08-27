//! Semantic validation for canonical RSpace DAG images.
//!
//! Structural closure validation proves that the immutable object graph is
//! canonical and complete.  This module supplies the separate language-level
//! proof required before a manifest can become authoritative: every trie
//! endpoint must be an exact frozen RhoPathV1 record, continuation source
//! payloads must round-trip exactly, and every continuation must have the
//! corresponding JoinBag memberships.  Traversal and lookup are explicit
//! pushdown machines; neither term depth nor trie depth consumes native stack.

use std::collections::BTreeSet;
use std::sync::Arc;

use pathmap::PathMap;
use serde::Serialize;

use super::canonical::{CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len};
use super::closure::{
    ClosureError, StructuralClosureReport, StructuralValidationLimits,
    verify_structural_closure_with_limits,
};
use super::manifest::SemanticNamespace;
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

/// Validate a complete canonical RSpace image without projecting any trie to
/// a `Vec`, sorting endpoints, or recursively descending either the DAG or a
/// decoded Rholang value.
pub fn verify_rspace_semantics<C, P, A, K, S>(
    store: &S,
    manifest_ref: ObjectRefV1,
    limits: SemanticValidationLimits,
) -> Result<SemanticValidationReport, ClosureError<S::Error>>
where
    C: ColdStoreDecode + RhoPathEncode,
    P: Clone + ColdStoreDecode + StableHashSerialize,
    A: Clone + ColdStoreDecode + RhoPathEncode + Serialize,
    K: Clone + ColdStoreDecode + StableHashSerialize,
    S: ImmutableObjectStore,
{
    let structural = verify_structural_closure_with_limits(store, manifest_ref, limits.structural)?;
    let manifest_bytes = load_reference(store, manifest_ref)?;
    let manifest =
        match CanonicalObjectView::parse(manifest_bytes.as_ref()).map_err(format_error)? {
            CanonicalObjectView::SemanticManifest(manifest) => manifest,
            object => {
                return Err(format_error(CanonicalFormatError::UnexpectedObjectKind {
                    digest: manifest_ref.digest(),
                    expected: CanonicalObjectKind::SemanticManifest.name(),
                    actual: object.kind().name(),
                }));
            }
        };
    let roots = manifest.roots();
    let mut report = SemanticValidationReport {
        structural,
        ..SemanticValidationReport::default()
    };

    walk_trie(store, roots.data_bag, limits, &mut report, |path, endpoint, report| {
        validate_data_endpoint::<C, A, S::Error>(path, endpoint)?;
        increment(&mut report.data_entries, 1, "data entries")
    })?;

    walk_trie(store, roots.join_bag, limits, &mut report, |path, endpoint, report| {
        validate_join_endpoint::<C, S::Error>(path, endpoint)?;
        increment(&mut report.join_entries, 1, "join entries")
    })?;

    walk_trie(store, roots.continuation_map, limits, &mut report, |path, endpoint, report| {
        validate_continuation_endpoint::<C, P, K, S>(
            store,
            roots.join_bag,
            path,
            endpoint,
            limits,
            report,
        )?;
        increment(&mut report.continuation_entries, 1, "continuation entries")
    })?;

    Ok(report)
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
    Ok(())
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
    increment(&mut report.source_payload_bytes, payload_len, "continuation source payload bytes")?;
    check_limit(
        "continuation source payload bytes",
        limits.max_payload_bytes,
        report.source_payload_bytes,
    )?;
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
    let mut distinct_channels = PathMap::<()>::new();
    for channel in &channels {
        let identity = channel.rho_path_identity();
        if distinct_channels.insert(&identity, ()).is_some() {
            continue;
        }
        let mut join_key = RhoPathV1::join_prefix(channel);
        join_key.extend_from_slice(&join_record);
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
    let mut reference = root;
    let mut offset = 0usize;
    loop {
        increment(&mut report.join_lookup_steps, 1, "join lookup steps")?;
        check_limit("join lookup steps", limits.max_join_lookup_steps, report.join_lookup_steps)?;
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
