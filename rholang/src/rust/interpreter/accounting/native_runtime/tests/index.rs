use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::native_runtime::recording::RecordedOccurrence;
// Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the seam of the test that requires a panic.
use crate::rust::interpreter::panic_child::PanicChild;

fn height_bound(nodes: usize) -> usize {
    let bits = usize::BITS as usize;
    2 * nodes
        .checked_add(1)
        .map_or(bits + 1, |n| bits - n.leading_zeros() as usize)
}

fn verify<K: Ord + std::fmt::Debug, V: PartialEq + std::fmt::Debug>(
    index: &NativeIndex<K, V>,
    expected: &BTreeMap<K, V>,
) {
    fn walk<K: Ord + std::fmt::Debug, V>(
        index: &NativeIndex<K, V>,
        current: Option<usize>,
        parent: Option<usize>,
        lower: Option<&K>,
        upper: Option<&K>,
        visited: &mut BTreeSet<usize>,
    ) -> usize {
        let Some(id) = current else {
            return 0;
        };
        assert!(visited.insert(id), "index has repeated links");
        let node = &index.nodes[id];
        assert_eq!(node.parent, parent);
        if let Some(lower) = lower {
            assert!(lower < &node.key);
        }
        if let Some(upper) = upper {
            assert!(&node.key < upper);
        }
        let left = walk(index, node.left, Some(id), lower, Some(&node.key), visited);
        let right = walk(index, node.right, Some(id), Some(&node.key), upper, visited);
        assert_eq!(node.height, 1 + left.max(right));
        assert!(left.abs_diff(right) <= 1, "index lost AVL balance");
        node.height
    }
    let mut visited = BTreeSet::new();
    let height = walk(index, index.root, None, None, None, &mut visited);
    assert_eq!(visited.len(), index.nodes.len());
    assert!(height < height_bound(index.len()));
    assert_eq!(index.len(), expected.len());
    assert_eq!(
        index.keys().collect::<BTreeSet<_>>(),
        expected.keys().collect()
    );
    for (key, value) in expected {
        assert_eq!(index.get(key), Some(value));
    }
}

#[test]
fn native_index_preserves_every_small_insertion_permutation() {
    fn permutations(prefix: &mut Vec<u64>, remaining: &mut Vec<u64>) {
        if remaining.is_empty() {
            let mut index = NativeIndex::default();
            let mut expected = BTreeMap::new();
            for key in prefix.iter().copied() {
                let stable: Vec<_> = index.nodes.iter().map(|node| node.key).collect();
                let prepared = index.prepare_insert(key, key + 10).unwrap();
                assert_eq!(index.len(), expected.len());
                assert_eq!(index.commit(prepared), expected.insert(key, key + 10));
                for (position, key) in stable.iter().enumerate() {
                    assert_eq!(&index.nodes[position].key, key);
                }
                verify(&index, &expected);
            }
            return;
        }
        for offset in 0..remaining.len() {
            let key = remaining.remove(offset);
            prefix.push(key);
            permutations(prefix, remaining);
            prefix.pop();
            remaining.insert(offset, key);
        }
    }
    permutations(&mut Vec::new(), &mut (0..6).collect());
}

#[test]
fn native_index_keeps_logarithmic_height_under_sequential_growth() {
    let mut index = NativeIndex::default();
    let prepared = index.prepare_insert(0u64, 7).unwrap();
    index.commit(prepared);
    for key in 1..512 {
        let prepared = index.prepare_insert(key, 0).unwrap();
        index.commit(prepared);
    }
    assert_eq!(index.get(&0), Some(&7));
    assert!(index.height(index.root) < height_bound(512));
}

// Added by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
// policy test): the child of the test below holds its index in this value. The destructor
// runs during the unwind of a refused commit. It reports the revision, verifies the index
// against its state before the commit, and then reports the verification.
struct ReportsStateOnUnwind<'a> {
    child: &'a PanicChild,
    index: NativeIndex<u64, i32>,
}

impl Drop for ReportsStateOnUnwind<'_> {
    fn drop(&mut self) {
        self.child.report("revision", self.index.revision);
        verify(&self.index, &BTreeMap::from([(4u64, 4)]));
        self.child.report("verified", true);
    }
}

#[test]
fn native_index_rejects_stale_single_and_batch_preparations_before_mutation() {
    let mut index = NativeIndex::default();
    let single = index.prepare_insert(1u64, 1).unwrap();
    let batch = index.prepare_batch(vec![(2u64, 2), (3u64, 3)]).unwrap();
    let newer = index.prepare_insert(4u64, 4).unwrap();
    index.commit(newer);
    let revision = index.revision;
    // Changed by the merge of feature/cost-accounted-rho (plan step 1.9, panic-expectation
    // policy test): each stale commit ran behind an unwind interceptor. A child process now
    // runs each commit, and this process checks the panic and the state that the child
    // reports after the unwind. The final two checks moved into that report, because this
    // process no longer runs a commit.
    // assert!(
    //     std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index.commit(single))).is_err()
    // );
    // assert!(
    //     std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index.commit_batch(batch)))
    //         .is_err()
    // );
    // assert_eq!(index.revision, revision);
    // verify(&index, &BTreeMap::from([(4u64, 4)]));
    let child = PanicChild::new(
        module_path!(),
        "native_index_rejects_stale_single_and_batch_preparations_before_mutation",
    );
    if let Some(case) = child.case() {
        let mut held = ReportsStateOnUnwind {
            child: &child,
            index,
        };
        child.subject_reached();
        match case.as_str() {
            "single" => {
                held.index.commit(single);
            }
            "batch" => held.index.commit_batch(batch),
            other => panic!("no such case: {other}"),
        }
        return;
    }
    for case in ["single", "batch"] {
        let report = child.expect_panic_in_case(case, "native index preparation is stale");
        assert_eq!(
            report.reported("revision"),
            Some(revision.to_string().as_str()),
            "a stale {case} preparation must be refused before the revision changes"
        );
        assert_eq!(
            report.reported("verified"),
            Some("true"),
            "a stale {case} preparation must leave the index as it was"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_index_matches_reference_for_arbitrary_batches(
        updates in prop::collection::vec(prop::collection::vec((0u64..64, any::<u64>()), 0..12), 0..24),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        for batch in updates {
            let stable: Vec<_> = index.nodes.iter().map(|node| node.key).collect();
            let prepared = index.prepare_batch(batch.clone()).unwrap();
            verify(&index, &expected);
            index.commit_batch(prepared);
            expected.extend(batch);
            for (position, key) in stable.iter().enumerate() {
                prop_assert_eq!(&index.nodes[position].key, key);
            }
            verify(&index, &expected);
        }
    }

    #[test]
    fn native_index_matches_reference_after_every_generated_update(
        updates in prop::collection::vec((0u64..128, any::<u64>()), 0..256),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        for (key, value) in updates {
            let prepared = index.prepare_insert(key, value).unwrap();
            prop_assert_eq!(index.commit(prepared), expected.insert(key, value));
            verify(&index, &expected);
        }
    }

    #[test]
    fn native_index_occurrence_keys_preserve_long_paths_and_stages(
        prefix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
        suffixes in prop::collection::vec((any::<u64>(), 0u8..3), 1..32),
    ) {
        let mut index = NativeIndex::default();
        let mut expected = BTreeMap::new();
        for (value, stage) in suffixes {
            let mut path = prefix.clone();
            path.push((value, 0));
            let key = RecordedOccurrence {
                session: [7; 32], path,
                stage: [NativeAttemptStage::ProduceIntroduction, NativeAttemptStage::ConsumeIntroduction, NativeAttemptStage::Comm][stage as usize],
            };
            let prepared = index.prepare_insert(key.clone(), value).unwrap();
            prop_assert_eq!(index.commit(prepared), expected.insert(key, value));
            verify(&index, &expected);
        }
    }
}

fn occurrence_key(occurrence: &RecordedOccurrence) -> OccurrenceKey {
    let path =
        rspace_plus_plus::rspace::operation_context::CausalPath::from(occurrence.path.clone());
    OccurrenceKey {
        session: occurrence.session,
        digest: path.digest(),
        depth: path.len(),
        last: path.last_segment(),
        stage: occurrence.stage,
    }
}

proptest! {
    /// C7a (DR-85; `NativePathTrie.digest_chain_correct`): the digest-keyed
    /// recording index reports a repeated occurrence exactly when the
    /// path-keyed index does. The small session, segment and stage domains
    /// make repeated occurrences frequent.
    #[test]
    fn digest_keyed_occurrences_match_path_keyed_index(
        rows in prop::collection::vec(
            (0_u8..2, prop::collection::vec((0_u64..3, 0_u64..3), 0..6), 0usize..3),
            0..96,
        ),
    ) {
        let stages = [
            NativeAttemptStage::ProduceIntroduction,
            NativeAttemptStage::ConsumeIntroduction,
            NativeAttemptStage::Comm,
        ];
        let mut by_digest = NativeIndex::<OccurrenceKey, ()>::default();
        let mut by_path = NativeIndex::<RecordedOccurrence, ()>::default();
        let mut repeated = 0;
        for (session, path, stage) in rows {
            let occurrence = RecordedOccurrence {
                session: [session; 32],
                path,
                stage: stages[stage],
            };
            let key = occurrence_key(&occurrence);
            let seen_by_digest = by_digest.get(&key).is_some();
            let seen_by_path = by_path.get(&occurrence).is_some();
            prop_assert_eq!(seen_by_digest, seen_by_path);
            if seen_by_path {
                repeated += 1;
                continue;
            }
            let prepared = by_digest.prepare_insert(key, ()).expect("insert fits");
            by_digest.commit(prepared);
            let prepared = by_path.prepare_insert(occurrence, ()).expect("insert fits");
            by_path.commit(prepared);
        }
        prop_assert_eq!(by_digest.len(), by_path.len());
        prop_assert!(repeated <= 96);
    }
}
