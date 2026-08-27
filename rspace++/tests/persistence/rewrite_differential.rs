use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use super::support::{empty_roots, encode, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore, CanonicalObjectView, DagChild,
    DagRewriteLimits, EndpointValue, ImmutableObjectStore, MutationBatchViewV1, ObjectAdmission,
    ObjectDigest, ObjectRefV1, PreparedDagRewrite, RSpaceMutationBatchV1, RewriteError,
    SemanticRoots, SemanticStateManifestV1, StagingStoreError, StateVersionV1,
    preflight_sensitive_copy_bytes, reset_preflight_sensitive_copy_bytes, rewrite_semantic_roots,
    rewrite_semantic_roots_with_limits,
};
use crate::rspace::rho_path::RhoPathV1;

type Entries = BTreeMap<Vec<u8>, EndpointValue>;

fn datum(identity: &[u8]) -> Vec<u8> { RhoPathV1::datum_key(&7_u64, b"shape", identity) }

fn continuation(identity: &[u8]) -> Vec<u8> {
    let prefix = RhoPathV1::continuation_prefix(&[7_u64, 8_u64]);
    RhoPathV1::collection_record_path(&prefix, identity)
}

fn join(record: &[u8]) -> Vec<u8> {
    let mut path = RhoPathV1::join_prefix(&7_u64);
    path.extend_from_slice(record);
    path
}

fn count(value: u64) -> EndpointValue {
    NonZeroU64::new(value)
        .map(EndpointValue::Multiplicity)
        .unwrap_or(EndpointValue::Absent)
}

fn admit_any(store: &CanonicalObjectStore, node: &CanonicalDagNode) -> ObjectRefV1 {
    let bytes: Arc<[u8]> = encode(node).into();
    let (reference, admission) = store.admit(bytes).expect("oracle node is admitted");
    assert!(matches!(admission, ObjectAdmission::Inserted | ObjectAdmission::AlreadyPresent));
    reference
}

/// Independent canonical rebuild oracle.  It partitions an ordered logical
/// map by the maximal common prefix at each level; it does not call the
/// differential editor or mirror its mutable-node representation.
fn rebuild(store: &CanonicalObjectStore, entries: &Entries) -> ObjectRefV1 {
    if entries.is_empty() {
        return admit_any(store, &CanonicalDagNode::empty());
    }
    let ordered = entries.iter().collect::<Vec<_>>();
    rebuild_partition(store, &ordered, 0)
}

fn rebuild_partition(
    store: &CanonicalObjectStore,
    entries: &[(&Vec<u8>, &EndpointValue)],
    depth: usize,
) -> ObjectRefV1 {
    let first = entries.first().expect("a partition is nonempty").0;
    let last = entries.last().expect("a partition is nonempty").0;
    let mut shared = depth;
    let limit = first.len().min(last.len());
    while shared < limit && first[shared] == last[shared] {
        shared += 1;
    }

    let endpoint = entries
        .iter()
        .find_map(|(key, value)| (key.len() == shared).then_some(**value))
        .unwrap_or(EndpointValue::Absent);
    let mut children = Vec::new();
    let mut start = entries.partition_point(|(key, _)| key.len() == shared);
    while start < entries.len() {
        let edge = entries[start].0[shared];
        let mut end = start + 1;
        while end < entries.len() && entries[end].0.len() > shared && entries[end].0[shared] == edge
        {
            end += 1;
        }
        children.push(DagChild {
            edge,
            target: rebuild_partition(store, &entries[start..end], shared + 1),
        });
        start = end;
    }
    let node =
        CanonicalDagNode::new(first[depth..shared].to_vec(), endpoint, children.into_boxed_slice())
            .expect("oracle partition is canonical");
    admit_any(store, &node)
}

fn rewrite<'a>(
    base: &'a CanonicalObjectStore,
    batch: &RSpaceMutationBatchV1,
) -> PreparedDagRewrite<'a, CanonicalObjectStore> {
    let bytes = encode(&batch.plan().expect("test mutation is nonempty"));
    let view = MutationBatchViewV1::parse(&bytes).expect("test mutation view validates");
    rewrite_semantic_roots(base, view).expect("rewrite succeeds")
}

fn roots_with_data(base: &CanonicalObjectStore, entries: &Entries) -> SemanticRoots {
    let empty = rebuild(base, &Entries::new());
    SemanticRoots {
        data_bag: rebuild(base, entries),
        ..empty_roots(empty)
    }
}

fn state(base: &CanonicalObjectStore, roots: SemanticRoots) -> StateVersionV1 {
    let manifest = SemanticStateManifestV1::new(1, roots, None).unwrap();
    let bytes: Arc<[u8]> = encode(&manifest).into();
    let (reference, _) = base.admit(bytes).unwrap();
    StateVersionV1::new(9, reference)
}

#[derive(Default)]
struct TracingStore {
    inner: CanonicalObjectStore,
    reads: Mutex<BTreeMap<ObjectDigest, u64>>,
}

impl TracingStore {
    fn clear_reads(&self) { self.reads.lock().unwrap().clear(); }

    fn read_count(&self, digest: ObjectDigest) -> u64 {
        self.reads
            .lock()
            .unwrap()
            .get(&digest)
            .copied()
            .unwrap_or(0)
    }
}

impl ImmutableObjectStore for TracingStore {
    type Error = CanonicalFormatError;
    type Object = Arc<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        let mut reads = self.reads.lock().unwrap();
        *reads.entry(*digest).or_default() += 1;
        drop(reads);
        self.inner.get_bounded(digest, max_encoded_len)
    }

    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error> {
        self.inner
            .put_if_absent_or_equal(reference, canonical_bytes)
    }
}

#[test]
fn insertion_update_and_delete_equal_an_independent_full_rebuild() {
    let base = CanonicalObjectStore::new();
    let mut before = Entries::from([
        (datum(b"alpha"), count(2)),
        (datum(b"alphabet"), count(4)),
        (datum(b"zeta"), count(6)),
    ]);
    let roots = roots_with_data(&base, &before);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"alpha"), 2, 9).unwrap();
    batch.record_data(&datum(b"alphabet"), 4, 0).unwrap();
    batch.record_data(&datum(b"alpine"), 0, 5).unwrap();
    let prepared = rewrite(&base, &batch);
    let rewritten = prepared.roots();

    before.insert(datum(b"alpha"), count(9));
    before.remove(&datum(b"alphabet"));
    before.insert(datum(b"alpine"), count(5));
    let expected = rebuild(&CanonicalObjectStore::new(), &before);
    assert_eq!(rewritten.data_bag, expected);
}

#[test]
fn prefix_splits_cover_key_prefix_existing_prefix_and_mid_prefix_divergence() {
    for (existing, inserted) in [
        (b"alphabet".as_slice(), b"alpha".as_slice()),
        (b"alpha".as_slice(), b"alphabet".as_slice()),
        (b"alphabet".as_slice(), b"alpine".as_slice()),
    ] {
        let base = CanonicalObjectStore::new();
        let mut expected = Entries::from([(datum(existing), count(1))]);
        let roots = roots_with_data(&base, &expected);
        let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
        batch.record_data(&datum(inserted), 0, 2).unwrap();
        let prepared = rewrite(&base, &batch);
        let rewritten = prepared.roots();
        expected.insert(datum(inserted), count(2));
        assert_eq!(
            rewritten.data_bag,
            rebuild(&CanonicalObjectStore::new(), &expected),
            "existing={existing:?}, inserted={inserted:?}"
        );
    }
}

#[test]
fn deleting_a_branch_collapses_every_newly_unary_ancestor() {
    let base = CanonicalObjectStore::new();
    let mut expected = Entries::from([
        (datum(b"shared-left-a"), count(1)),
        (datum(b"shared-left-b"), count(2)),
        (datum(b"shared-right"), count(3)),
    ]);
    let roots = roots_with_data(&base, &expected);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"shared-left-a"), 1, 0).unwrap();
    batch.record_data(&datum(b"shared-left-b"), 2, 0).unwrap();
    let prepared = rewrite(&base, &batch);
    let rewritten = prepared.roots();
    expected.remove(&datum(b"shared-left-a"));
    expected.remove(&datum(b"shared-left-b"));
    assert_eq!(rewritten.data_bag, rebuild(&CanonicalObjectStore::new(), &expected));
}

#[test]
fn adaptive_branch_boundaries_are_canonical_after_insertions_and_deletions() {
    for cardinality in [4_u16, 5, 16, 17, 48, 49, 255, 256] {
        let base = CanonicalObjectStore::new();
        let roots = roots_with_data(&base, &Entries::new());
        let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
        let mut expected = Entries::new();
        for edge in 0..cardinality {
            let identity = [u8::try_from(edge).unwrap()];
            let key = datum(&identity);
            batch.record_data(&key, 0, u64::from(edge) + 1).unwrap();
            expected.insert(key, count(u64::from(edge) + 1));
        }
        let prepared = rewrite(&base, &batch);
        let rewritten = prepared.roots();
        assert_eq!(rewritten.data_bag, rebuild(&CanonicalObjectStore::new(), &expected));
    }

    for remaining in [48_u16, 16, 4] {
        let base = CanonicalObjectStore::new();
        let mut expected = Entries::new();
        for edge in 0..=remaining {
            let identity = [u8::try_from(edge).unwrap()];
            expected.insert(datum(&identity), count(u64::from(edge) + 1));
        }
        let roots = roots_with_data(&base, &expected);
        let removed = [u8::try_from(remaining).unwrap()];
        let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
        batch
            .record_data(&datum(&removed), u64::from(remaining) + 1, 0)
            .unwrap();
        let prepared = rewrite(&base, &batch);
        let rewritten = prepared.roots();
        expected.remove(&datum(&removed));
        assert_eq!(rewritten.data_bag, rebuild(&CanonicalObjectStore::new(), &expected));
    }
}

#[test]
fn all_three_semantic_roots_are_rewritten_by_one_borrowed_payload() {
    let base = CanonicalObjectStore::new();
    let empty = rebuild(&base, &Entries::new());
    let roots = empty_roots(empty);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    let data_key = datum(b"datum");
    let continuation_key = continuation(b"receive");
    let join_key = join(b"record");
    let content = synthetic_ref(13);
    batch.record_data(&data_key, 0, 7).unwrap();
    batch
        .record_continuation(&continuation_key, None, Some(content))
        .unwrap();
    batch.record_join(&join_key, 0, 11).unwrap();
    let prepared = rewrite(&base, &batch);
    let rewritten = prepared.roots();

    let expected_store = CanonicalObjectStore::new();
    assert_eq!(
        rewritten.data_bag,
        rebuild(&expected_store, &Entries::from([(data_key, count(7))]))
    );
    assert_eq!(
        rewritten.continuation_map,
        rebuild(
            &expected_store,
            &Entries::from([(continuation_key, EndpointValue::Content(content))])
        )
    );
    assert_eq!(
        rewritten.join_bag,
        rebuild(&expected_store, &Entries::from([(join_key, count(11))]))
    );
}

#[test]
fn stale_compare_and_swap_never_changes_the_base_store() {
    let base = CanonicalObjectStore::new();
    let entries = Entries::from([(datum(b"cas"), count(4))]);
    let roots = roots_with_data(&base, &entries);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    let base_len = base.len().unwrap();
    batch.record_data(&datum(b"cas"), 3, 8).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    assert!(matches!(rewrite_semantic_roots(&base, view), Err(RewriteError::CompareAndSwap(_))));
    assert_eq!(base.len().unwrap(), base_len);
}

#[test]
fn an_after_state_in_the_declared_base_is_rejected_as_inconsistent() {
    let base = CanonicalObjectStore::new();
    let entries = Entries::from([(datum(b"replayed"), count(8))]);
    let roots = roots_with_data(&base, &entries);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"replayed"), 3, 8).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    assert!(matches!(rewrite_semantic_roots(&base, view), Err(RewriteError::CompareAndSwap(_))));
}

#[test]
fn recompiling_the_same_batch_from_the_immutable_base_is_idempotent() {
    let base = CanonicalObjectStore::new();
    let expected = Entries::from([(datum(b"pending"), count(3))]);
    let roots = roots_with_data(&base, &expected);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"pending"), 3, 9).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let first = rewrite_semantic_roots(&base, MutationBatchViewV1::parse(&bytes).unwrap()).unwrap();
    let second =
        rewrite_semantic_roots(&base, MutationBatchViewV1::parse(&bytes).unwrap()).unwrap();
    assert_eq!(first.roots(), second.roots());
    let mut first_staged = BTreeSet::new();
    first
        .staging()
        .try_visit_staged::<()>(|reference, _| {
            first_staged.insert(reference.digest());
            Ok(())
        })
        .unwrap();
    let mut second_staged = BTreeSet::new();
    second
        .staging()
        .try_visit_staged::<()>(|reference, _| {
            second_staged.insert(reference.digest());
            Ok(())
        })
        .unwrap();
    assert_eq!(first_staged, second_staged);
}

#[test]
fn the_mutation_bound_manifest_is_the_only_root_authority() {
    let base = CanonicalObjectStore::new();
    let empty = rebuild(&base, &Entries::new());
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(4, synthetic_ref(77))).unwrap();
    batch.record_data(&datum(b"untrusted-root"), 0, 1).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    assert!(matches!(
        rewrite_semantic_roots(&base, view),
        Err(RewriteError::Format(CanonicalFormatError::MissingObject(_)))
    ));
    assert_eq!(base.len().unwrap(), 1, "only the empty root was admitted");
    assert_eq!(empty, rebuild(&CanonicalObjectStore::new(), &Entries::new()));
}

#[test]
fn explicit_rewrite_budgets_fail_closed_without_partial_staging() {
    let base = CanonicalObjectStore::new();
    let roots = roots_with_data(&base, &Entries::new());
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"budgeted"), 0, 1).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_mutations: 0,
        ..DagRewriteLimits::UNBOUNDED
    };
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "rewrite mutations",
            limit: 0,
            attempted: 1,
        }))
    ));
}

#[test]
fn exhausted_edit_budget_refuses_an_insertion_before_suffix_copy() {
    let base = CanonicalObjectStore::new();
    let roots = roots_with_data(&base, &Entries::new());
    let mut identity = vec![b'x'; 1_000_000];
    identity.push(b'y');
    let key = datum(&identity);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&key, 0, 1).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_edited_nodes: 1,
        ..DagRewriteLimits::UNBOUNDED
    };
    reset_preflight_sensitive_copy_bytes();
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "edited nodes",
            limit: 1,
            attempted: 2,
        }))
    ));
    assert_eq!(preflight_sensitive_copy_bytes(), 0);

    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    reset_preflight_sensitive_copy_bytes();
    rewrite_semantic_roots(&base, view).expect("the observer control rewrite succeeds");
    assert!(
        preflight_sensitive_copy_bytes() > 0,
        "a successful insertion must prove that the copy observer is live"
    );
}

#[test]
fn exhausted_edit_budget_refuses_a_split_before_fragment_copies() {
    let base = CanonicalObjectStore::new();
    let shared = vec![b'x'; 1_000_000];
    let mut first_identity = shared.clone();
    first_identity.push(1);
    let mut second_identity = shared;
    second_identity.push(2);
    let first = datum(&first_identity);
    let second = datum(&second_identity);
    let roots = roots_with_data(&base, &Entries::from([(first, count(1))]));
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&second, 0, 1).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_edited_nodes: 1,
        ..DagRewriteLimits::UNBOUNDED
    };
    reset_preflight_sensitive_copy_bytes();
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "edited nodes",
            limit: 1,
            attempted: 2,
        }))
    ));
    assert_eq!(preflight_sensitive_copy_bytes(), 0);

    // Exercise the distinct child-split arm: the root branches on x/y,
    // opening the x child consumes the second edit slot, and splitting that
    // child's long compressed prefix must refuse before copying fragments.
    let base = CanonicalObjectStore::new();
    let shared = vec![b'x'; 1_000_000];
    let mut first_identity = shared.clone();
    first_identity.push(1);
    let mut second_identity = shared;
    second_identity.push(2);
    let mut other_identity = vec![b'y'; 1_000_000];
    other_identity.push(1);
    let first = datum(&first_identity);
    let second = datum(&second_identity);
    let other = datum(&other_identity);
    let roots = roots_with_data(&base, &Entries::from([(first, count(1)), (other, count(1))]));
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&second, 0, 1).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_edited_nodes: 2,
        ..DagRewriteLimits::UNBOUNDED
    };
    reset_preflight_sensitive_copy_bytes();
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "edited nodes",
            limit: 2,
            attempted: 3,
        }))
    ));
    assert_eq!(preflight_sensitive_copy_bytes(), 0);
}

#[test]
fn exhausted_edit_budget_refuses_child_expansion_before_store_read() {
    let tracing = TracingStore::default();
    let entries = Entries::from([(datum(b"alpha"), count(1)), (datum(b"alpine"), count(2))]);
    let data_root = rebuild(&tracing.inner, &entries);
    let root_bytes = tracing.inner.get(&data_root.digest()).unwrap().unwrap();
    let CanonicalObjectView::DagNode(root_node) =
        CanonicalObjectView::parse(root_bytes.as_ref()).unwrap()
    else {
        unreachable!()
    };
    let selected = root_node
        .children()
        .iter()
        .next()
        .expect("the controlled fixture branches")
        .target;
    let empty = rebuild(&tracing.inner, &Entries::new());
    let roots = SemanticRoots {
        data_bag: data_root,
        ..empty_roots(empty)
    };
    let manifest = SemanticStateManifestV1::new(1, roots, None).unwrap();
    let (manifest_ref, _) = tracing.inner.admit(encode(&manifest).into()).unwrap();
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(5, manifest_ref)).unwrap();
    batch.record_data(&datum(b"alpha"), 1, 7).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_edited_nodes: 1,
        ..DagRewriteLimits::UNBOUNDED
    };
    tracing.clear_reads();
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&tracing, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "edited nodes",
            limit: 1,
            attempted: 2,
        }))
    ));
    assert_eq!(tracing.read_count(selected.digest()), 0);
}

fn unary_merge_state(base: &CanonicalObjectStore) -> (StateVersionV1, Vec<u8>, u64, u64) {
    let key = datum(b"unary-merge");
    let child_prefix = vec![b'z'; 64];
    let child = CanonicalDagNode::new(child_prefix.clone(), count(2), Box::<[DagChild]>::default())
        .unwrap();
    let child_ref = admit_any(base, &child);
    let root = CanonicalDagNode::new(
        key.clone(),
        count(1),
        vec![DagChild {
            edge: b'x',
            target: child_ref,
        }]
        .into_boxed_slice(),
    )
    .unwrap();
    let root_ref = admit_any(base, &root);
    let empty = rebuild(base, &Entries::new());
    let roots = SemanticRoots {
        data_bag: root_ref,
        ..empty_roots(empty)
    };
    let retained_before_join = u64::try_from(key.len() + child_prefix.len()).unwrap();
    let merged_len =
        CanonicalDagNode::canonical_len_for_shape(key.len() + 1 + child_prefix.len(), count(2), 0)
            .unwrap();
    (state(base, roots), key, retained_before_join, merged_len)
}

#[test]
fn unary_compression_charges_the_joining_edge_before_prefix_allocation() {
    let base = CanonicalObjectStore::new();
    let (state, key, retained_before_join, _) = unary_merge_state(&base);
    let mut batch = RSpaceMutationBatchV1::new(state).unwrap();
    batch.record_data(&key, 1, 0).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_retained_prefix_bytes: retained_before_join,
        ..DagRewriteLimits::UNBOUNDED
    };
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "retained prefix bytes",
            limit,
            attempted,
        })) if limit == retained_before_join && attempted == retained_before_join + 1
    ));
}

#[test]
fn unary_compression_preflights_the_merged_encoding_before_allocation() {
    let base = CanonicalObjectStore::new();
    let (state, key, _, merged_len) = unary_merge_state(&base);
    let mut batch = RSpaceMutationBatchV1::new(state).unwrap();
    batch.record_data(&key, 1, 0).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limit = merged_len - 1;
    let limits = DagRewriteLimits {
        max_emitted_bytes: limit,
        ..DagRewriteLimits::UNBOUNDED
    };
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "emitted bytes",
            limit: actual_limit,
            attempted,
        })) if actual_limit == limit && attempted == merged_len
    ));
}

#[test]
fn bounded_store_rejects_a_lying_reference_before_materializing_its_object() {
    let base = CanonicalObjectStore::new();
    let key = datum(b"lying-reference");
    let root = CanonicalDagNode::new(key.clone(), count(1), Box::<[DagChild]>::default()).unwrap();
    let actual_ref = admit_any(&base, &root);
    let declared_len = actual_ref.encoded_len() - 1;
    let lying_ref = ObjectRefV1::new(actual_ref.digest(), declared_len).unwrap();
    let empty = rebuild(&base, &Entries::new());
    let manifest = SemanticStateManifestV1::new(
        1,
        SemanticRoots {
            data_bag: lying_ref,
            ..empty_roots(empty)
        },
        None,
    )
    .unwrap();
    let (manifest_ref, _) = base.admit(encode(&manifest).into()).unwrap();
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(3, manifest_ref)).unwrap();
    batch.record_data(&key, 1, 2).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let error = match rewrite_semantic_roots(&base, view) {
        Err(error) => error,
        Ok(_) => panic!("the lying object must be rejected"),
    };
    let RewriteError::Store(error) = error else {
        panic!("expected a bounded store error")
    };
    assert!(matches!(
        error.as_ref(),
        StagingStoreError::Base(CanonicalFormatError::ResourceLimit {
            resource: "materialized object bytes",
            limit,
            attempted,
        }) if *limit == declared_len && *attempted == actual_ref.encoded_len()
    ));
}

#[test]
fn rewrite_budgets_are_cumulative_across_all_three_namespaces() {
    let base = CanonicalObjectStore::new();
    let empty = rebuild(&base, &Entries::new());
    let roots = empty_roots(empty);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"first"), 0, 1).unwrap();
    batch.record_join(&join(b"second"), 0, 1).unwrap();
    let base_len = base.len().unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let limits = DagRewriteLimits {
        max_mutations: 1,
        ..DagRewriteLimits::UNBOUNDED
    };
    assert!(matches!(
        rewrite_semantic_roots_with_limits(&base, view, limits),
        Err(RewriteError::Format(CanonicalFormatError::ResourceLimit {
            resource: "rewrite mutations",
            limit: 0,
            attempted: 1,
        }))
    ));
    assert_eq!(base.len().unwrap(), base_len);
}

#[test]
fn a_later_namespace_failure_discards_the_earlier_private_rewrite() {
    let base = CanonicalObjectStore::new();
    let empty = rebuild(&base, &Entries::new());
    let roots = empty_roots(empty);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"would-stage"), 0, 1).unwrap();
    batch
        .record_continuation(
            &continuation(b"stale"),
            Some(synthetic_ref(2)),
            Some(synthetic_ref(3)),
        )
        .unwrap();
    let base_len = base.len().unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    assert!(matches!(rewrite_semantic_roots(&base, view), Err(RewriteError::CompareAndSwap(_))));
    assert_eq!(base.len().unwrap(), base_len);
}

#[test]
fn copy_on_write_never_loads_an_untouched_sibling_subtree() {
    let tracing = TracingStore::default();
    let entries = Entries::from([(datum(b"alpha"), count(1)), (datum(b"omega"), count(2))]);
    let data_root = rebuild(&tracing.inner, &entries);
    let empty = rebuild(&tracing.inner, &Entries::new());
    let roots = SemanticRoots {
        data_bag: data_root,
        ..empty_roots(empty)
    };
    let root_bytes = tracing.inner.get(&data_root.digest()).unwrap().unwrap();
    let CanonicalObjectView::DagNode(root_node) =
        CanonicalObjectView::parse(root_bytes.as_ref()).unwrap()
    else {
        unreachable!()
    };
    let untouched = root_node
        .children()
        .iter()
        .last()
        .expect("the controlled fixture branches")
        .target;
    let manifest = SemanticStateManifestV1::new(1, roots, None).unwrap();
    let (manifest_ref, _) = tracing.inner.admit(encode(&manifest).into()).unwrap();
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(5, manifest_ref)).unwrap();
    batch.record_data(&datum(b"alpha"), 1, 7).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    tracing.clear_reads();
    let prepared = rewrite_semantic_roots(&tracing, view).unwrap();
    let report = prepared.report();
    assert_eq!(tracing.read_count(untouched.digest()), 0);
    assert!(report.reused_child_references >= 1);
}

#[test]
fn every_staged_object_is_reachable_from_the_rewritten_roots() {
    let base = CanonicalObjectStore::new();
    let before = Entries::from([
        (datum(b"alpha"), count(1)),
        (datum(b"beta"), count(2)),
        (datum(b"gamma"), count(3)),
    ]);
    let roots = roots_with_data(&base, &before);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&datum(b"alphabet"), 0, 4).unwrap();
    batch.record_data(&datum(b"beta"), 2, 0).unwrap();
    let prepared = rewrite(&base, &batch);
    let rewritten = prepared.roots();
    let overlay = prepared.staging();

    let mut staged = BTreeSet::new();
    overlay
        .try_visit_staged::<()>(|reference, _| {
            staged.insert(reference.digest());
            Ok(())
        })
        .unwrap();
    let mut reachable = BTreeSet::new();
    let mut work = vec![rewritten.data_bag, rewritten.continuation_map, rewritten.join_bag];
    while let Some(reference) = work.pop() {
        if !reachable.insert(reference.digest()) {
            continue;
        }
        let bytes = overlay.get(&reference.digest()).unwrap().unwrap();
        if let CanonicalObjectView::DagNode(node) =
            CanonicalObjectView::parse(bytes.as_ref()).unwrap()
        {
            work.extend(node.children().iter().map(|child| child.target));
        }
    }
    let reachable_staged = reachable
        .intersection(&staged)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(staged, reachable_staged);
}

#[test]
fn adjacent_key_cursor_handles_a_large_shared_prefix_without_a_depth_limit() {
    let base = CanonicalObjectStore::new();
    let roots = roots_with_data(&base, &Entries::new());
    let shared = vec![b'x'; 100_000];
    let mut first_identity = shared.clone();
    first_identity.push(1);
    let mut second_identity = shared;
    second_identity.push(2);
    let first = datum(&first_identity);
    let second = datum(&second_identity);
    let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
    batch.record_data(&first, 0, 1).unwrap();
    batch.record_data(&second, 0, 2).unwrap();
    let prepared = rewrite(&base, &batch);
    let rewritten = prepared.roots();
    let expected = Entries::from([(first, count(1)), (second, count(2))]);
    assert_eq!(rewritten.data_bag, rebuild(&CanonicalObjectStore::new(), &expected));
}

#[test]
fn twenty_thousand_endpoint_bearing_nodes_rewrite_on_a_256k_stack() {
    std::thread::Builder::new()
        .name("persistent-radix-rewrite-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            const DEPTH: usize = 20_000;
            let base = CanonicalObjectStore::new();
            let leaf = CanonicalDagNode::new(
                Box::<[u8]>::default(),
                count(1),
                Box::<[DagChild]>::default(),
            )
            .unwrap();
            let mut child = admit_any(&base, &leaf);
            for _ in 1..DEPTH {
                let parent = CanonicalDagNode::new(
                    Box::<[u8]>::default(),
                    count(1),
                    vec![DagChild {
                        edge: b'x',
                        target: child,
                    }]
                    .into_boxed_slice(),
                )
                .unwrap();
                child = admit_any(&base, &parent);
            }
            let root = CanonicalDagNode::new(
                RhoPathV1::join_prefix(&7_u64),
                count(1),
                vec![DagChild {
                    edge: b'x',
                    target: child,
                }]
                .into_boxed_slice(),
            )
            .unwrap();
            let join_root = admit_any(&base, &root);
            let empty = rebuild(&base, &Entries::new());
            let roots = SemanticRoots {
                join_bag: join_root,
                ..empty_roots(empty)
            };
            let mut deepest_key = RhoPathV1::join_prefix(&7_u64);
            deepest_key.extend(std::iter::repeat_n(b'x', DEPTH));
            let mut batch = RSpaceMutationBatchV1::new(state(&base, roots)).unwrap();
            batch.record_join(&deepest_key, 1, 2).unwrap();
            let bytes = encode(&batch.plan().unwrap());
            let view = MutationBatchViewV1::parse(&bytes).unwrap();
            let prepared = rewrite_semantic_roots(&base, view).unwrap();
            let rewritten = prepared.roots();
            let report = prepared.report();
            assert!(report.loaded_nodes >= DEPTH as u64 + 1);
            assert!(report.maximum_finalize_worklist >= DEPTH as u64);

            let expected_store = CanonicalObjectStore::new();
            let changed_leaf = CanonicalDagNode::new(
                Box::<[u8]>::default(),
                count(2),
                Box::<[DagChild]>::default(),
            )
            .unwrap();
            let mut expected = admit_any(&expected_store, &changed_leaf);
            for _ in 1..DEPTH {
                let parent = CanonicalDagNode::new(
                    Box::<[u8]>::default(),
                    count(1),
                    vec![DagChild {
                        edge: b'x',
                        target: expected,
                    }]
                    .into_boxed_slice(),
                )
                .unwrap();
                expected = admit_any(&expected_store, &parent);
            }
            let expected_root = CanonicalDagNode::new(
                RhoPathV1::join_prefix(&7_u64),
                count(1),
                vec![DagChild {
                    edge: b'x',
                    target: expected,
                }]
                .into_boxed_slice(),
            )
            .unwrap();
            assert_eq!(rewritten.join_bag, admit_any(&expected_store, &expected_root));
        })
        .unwrap()
        .join()
        .unwrap();
}
