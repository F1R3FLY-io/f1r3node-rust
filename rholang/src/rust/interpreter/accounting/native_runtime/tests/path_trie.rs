use std::collections::BTreeSet;

use proptest::prelude::*;
use rspace_plus_plus::rspace::operation_context::CausalPath;

use super::*;

fn paths() -> impl Strategy<Value = Vec<Vec<PathSegment>>> {
    prop::collection::vec(prop::collection::vec((0_u64..3, 0_u64..3), 0..7), 0..24)
}

fn common_prefix(left: &[PathSegment], right: &[PathSegment]) -> usize {
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

/// Interns `paths` as the delta decoder does: each path keeps its longest
/// common prefix with the previous path and interns the rest below the
/// ancestor at that depth.
fn intern_chain(trie: &mut NativePathTrie, paths: &[Vec<PathSegment>]) -> (Vec<PathId>, usize) {
    let mut previous: &[PathSegment] = &[];
    let mut previous_id = PathId::ROOT;
    let mut suffixes = 0;
    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        let prefix = common_prefix(previous, path);
        suffixes += path.len() - prefix;
        assert!(trie.admit(path.len() - prefix, usize::MAX));
        let base = trie.ancestor(previous_id, prefix).expect("ancestor exists");
        let id = trie
            .intern(base, &path[prefix..])
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
        let mut from_root = NativePathTrie::new(8);
        let mut interned = BTreeSet::new();
        for path in &paths {
            interned.insert(
                from_root
                    .intern(PathId::ROOT, path)
                    .expect("path fits the trie"),
            );
        }
        let mut chained = NativePathTrie::new(8);
        intern_chain(&mut chained, &paths);
        prop_assert_eq!(&chained, &from_root);

        let walked = from_root
            .preorder()
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
        let mut trie = NativePathTrie::new(8);
        let (ids, suffixes) = intern_chain(&mut trie, &paths);
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
        let mut trie = NativePathTrie::new(16);
        let ids = paths
            .iter()
            .map(|path| trie.intern(PathId::ROOT, path).expect("path fits the trie"))
            .collect::<Vec<_>>();
        for (path, id) in paths.iter().zip(&ids) {
            for depth in 0..=path.len() {
                let ancestor = trie.ancestor(*id, depth).expect("ancestor fits");
                prop_assert_eq!(trie.segments(ancestor), path[..depth].to_vec());
                prop_assert_eq!(
                    trie.suffix(*id, depth).expect("suffix fits"),
                    path[depth..].to_vec()
                );
            }
        }
        for (left, left_id) in paths.iter().zip(&ids) {
            for (right, right_id) in paths.iter().zip(&ids) {
                prop_assert_eq!(
                    trie.lcp_depth(*left_id, *right_id).expect("prefix fits"),
                    common_prefix(left, right)
                );
            }
        }
    }
}

#[test]
fn depth_and_suffix_limits_reject_without_change() {
    let mut trie = NativePathTrie::new(2);
    let leaf = trie
        .intern(PathId::ROOT, &[(1, 0), (2, 0)])
        .expect("two segments fit");
    let before = trie.clone();
    assert!(trie.child(leaf, (3, 0)).is_err());
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
