use std::collections::BTreeSet;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rspace_plus_plus::rspace::operation_context::CausalPath;

use super::*;

fn host() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1 << 40)))
}

fn paths() -> impl Strategy<Value = Vec<Vec<PathSegment>>> {
    prop::collection::vec(prop::collection::vec((0_u64..3, 0_u64..3), 0..7), 0..24)
}

fn common_prefix(left: &[PathSegment], right: &[PathSegment]) -> usize {
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

/// Interns `paths` as the delta decoder does: each path keeps its longest
/// common prefix with the previous path and interns the rest below the
/// ancestor at that depth.
fn intern_chain(
    trie: &mut NativePathTrie,
    paths: &[Vec<PathSegment>],
    budget: &HostWorkBudget,
) -> (Vec<PathId>, usize) {
    let mut previous: &[PathSegment] = &[];
    let mut previous_id = PathId::ROOT;
    let mut suffixes = 0;
    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        let prefix = common_prefix(previous, path);
        suffixes += path.len() - prefix;
        assert!(trie.admit(path.len() - prefix, usize::MAX));
        let base = trie
            .ancestor(previous_id, prefix, budget)
            .expect("ancestor fits the budget");
        let id = trie
            .intern(base, &path[prefix..], budget)
            .expect("path fits the trie");
        ids.push(id);
        previous = path;
        previous_id = id;
    }
    (ids, suffixes)
}

proptest! {
    /// C7b (DR-86; `NativePathTrie.trie_dfs_is_lex_sort`): the depth-first
    /// walk lists the interned paths in sorted, de-duplicated vector order. A
    /// delta chain and interning from the root build the same nodes in the
    /// same order.
    #[test]
    fn trie_order_equals_vec_sort(paths in paths()) {
        let budget = host();
        let mut from_root = NativePathTrie::new(8, &budget).expect("trie fits the budget");
        let mut interned = BTreeSet::new();
        for path in &paths {
            interned.insert(
                from_root
                    .intern(PathId::ROOT, path, &budget)
                    .expect("path fits the trie"),
            );
        }
        let mut chained = NativePathTrie::new(8, &budget).expect("trie fits the budget");
        intern_chain(&mut chained, &paths, &budget);
        prop_assert_eq!(&chained, &from_root);

        let walked = from_root
            .preorder(&budget)
            .expect("walk fits the budget")
            .into_iter()
            .filter(|id| interned.contains(id))
            .map(|id| from_root.segments(id))
            .collect::<Vec<_>>();
        let mut expected = paths.clone();
        expected.sort();
        expected.dedup();
        expected.retain(|path| !path.is_empty());
        prop_assert_eq!(walked, expected);
    }

    /// C7b (DR-86; `NativePathTrie.node_identity_is_path_equality` and
    /// `trie_nodes_bounded_by_suffixes`): two interned paths have the same id
    /// exactly when they are equal, each node carries the chained digest of
    /// its path (C7a), and the trie holds at most Σ s_i nodes.
    #[test]
    fn ids_equal_iff_paths_equal(paths in paths()) {
        let budget = host();
        let mut trie = NativePathTrie::new(8, &budget).expect("trie fits the budget");
        let (ids, suffixes) = intern_chain(&mut trie, &paths, &budget);
        for (left, left_id) in paths.iter().zip(&ids) {
            prop_assert_eq!(&trie.segments(*left_id), left);
            prop_assert_eq!(trie.digest(*left_id), CausalPath::from(left.clone()).digest());
            prop_assert_eq!(trie.key(*left_id), PathKey::of(&CausalPath::from(left.clone())));
            prop_assert_eq!(trie.find(left), Some(*left_id));
            prop_assert!(trie.contains(*left_id));
            for (right, right_id) in paths.iter().zip(&ids) {
                prop_assert_eq!(left_id == right_id, left == right);
            }
        }
        prop_assert!(trie.node_count() <= suffixes);
    }

    /// The jump table gives every ancestor and every longest common prefix of
    /// the interned paths.
    #[test]
    fn ancestors_and_common_prefixes_match_vectors(
        paths in prop::collection::vec(
            prop::collection::vec((0_u64..2, 0_u64..2), 0..12),
            1..12,
        ),
    ) {
        let budget = host();
        let mut trie = NativePathTrie::new(16, &budget).expect("trie fits the budget");
        let ids = paths
            .iter()
            .map(|path| trie.intern(PathId::ROOT, path, &budget).expect("path fits the trie"))
            .collect::<Vec<_>>();
        for (path, id) in paths.iter().zip(&ids) {
            for depth in 0..=path.len() {
                let ancestor = trie.ancestor(*id, depth, &budget).expect("ancestor fits");
                prop_assert_eq!(trie.segments(ancestor), path[..depth].to_vec());
                prop_assert_eq!(
                    trie.suffix(*id, depth, &budget).expect("suffix fits"),
                    path[depth..].to_vec()
                );
            }
        }
        for (left, left_id) in paths.iter().zip(&ids) {
            for (right, right_id) in paths.iter().zip(&ids) {
                prop_assert_eq!(
                    trie.lcp_depth(*left_id, *right_id, &budget).expect("prefix fits"),
                    common_prefix(left, right)
                );
            }
        }
    }
}

#[test]
fn depth_and_suffix_limits_reject_without_change() {
    let budget = host();
    let mut trie = NativePathTrie::new(2, &budget).expect("trie fits the budget");
    let leaf = trie
        .intern(PathId::ROOT, &[(1, 0), (2, 0)], &budget)
        .expect("two segments fit");
    let before = trie.clone();
    assert!(trie.child(leaf, (3, 0), &budget).is_err());
    assert_eq!(trie, before);
    assert!(trie.admit(3, 4));
    assert!(!trie.admit(2, 4));
    assert!(trie.admit(1, 4));
    assert!(!trie.contains(PathId { index: 5, depth: 1 }));
    assert!(!trie.contains(PathId {
        index: leaf.index,
        depth: 1
    }));
}

#[test]
fn rejected_growth_leaves_the_trie_unchanged() {
    let budget = host();
    let mut trie = NativePathTrie::new(8, &budget).expect("trie fits the budget");
    trie.intern(PathId::ROOT, &[(1, 0)], &budget)
        .expect("one segment fits");
    let before = trie.clone();
    for limit in [1, 64, 256, 1_024] {
        let tight = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)));
        if trie.child(PathId::ROOT, (2, 0), &tight).is_err() {
            assert_eq!(trie, before);
            assert_eq!(trie.children.len(), before.children.len());
        }
    }
    let existing = trie
        .child(PathId::ROOT, (1, 0), &budget)
        .expect("existing child fits");
    assert_eq!(trie.segments(existing), vec![(1, 0)]);
}
