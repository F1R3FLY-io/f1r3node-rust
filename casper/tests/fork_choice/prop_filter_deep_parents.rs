// C12 proptests for the abstract parent-depth policy mirrored by the production
// `retained_parent_indices` helper and the fork-choice GuardBridge.
//
// The Rocq module `formal/rocq/fork_choice/theories/GuardBridge.v` models
// the depth filter abstractly:
//     Definition within_depth (maxn mpd pn : nat) : bool := Nat.leb (maxn - pn) mpd.
//     Definition prop_filter (maxn mpd : nat) (nums : list nat) : list nat :=
//                    filter (within_depth maxn mpd) nums.
//     Theorem honest_forkchoice_parents_validate : ... parents_ok maxn (mpd+buf)
//                    (prop_filter maxn mpd nums) = true.
// but with no mechanical tie to the concrete Rust that actually filters parents.
// This file is that tie.
//
// Production resolves every ranked hash to a block number, finds the maximum height
// in that immutable input, retains index zero unconditionally, and retains exactly
// those tail entries within the depth horizon. In-module proptests in `estimator.rs`
// call that production helper directly; this file cross-checks the corresponding
// abstract set and ordering properties used by Rocq.
//
// Properties checked (the concrete realization of the GuardBridge model):
//   (a) SOUNDNESS    — every RETAINED secondary parent satisfies within_depth
//                      (`max_height - pn <= depth`); nothing too deep survives.
//   (b) MAIN-KEPT    — the main parent is ALWAYS retained, and retained FIRST
//                      (the real code chains `once(main_hash)` ahead of the filter).
//   (c) COMPLETENESS — nothing satisfying within_depth is dropped.
//   (d) EXACT SET    — capstone: the retained set equals {head} ∪ prop_filter(secondaries),
//                      tying the concrete output to the abstract spec set exactly.
//
// The arithmetic is modeled over signed `i64`, as in production. `max_block_number`
// is an actual maximum, so every subtraction is nonnegative on valid block numbers.
//
// LOCAL-ONLY verification (not consensus code). Run under `cargo test -p casper` and
// gated by scripts/check-fork-choice-ALL.sh via the `fork_choice::` filter.

use std::collections::BTreeSet;

use proptest::prelude::*;

/// Faithful to the two `models::rust::block_metadata::BlockMetadata` fields that
/// `filter_deep_parents` reads: `{ block_hash, block_number }`. `u64` stands in for
/// `BlockHash`; distinct integers ⇒ distinct hashes, so membership is unambiguous.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parent {
    hash: u64,
    block_number: i64,
}

/// The abstract GuardBridge predicate `within_depth` (GuardBridge.v:165), over the
/// SIGNED i64 arithmetic the Rust actually performs: a secondary at height `pn` is
/// within depth `depth` of the maximum candidate height iff
/// `max_block_number - pn <= depth`.
fn within_depth(max_block_number: i64, pn: i64, depth: i64) -> bool {
    max_block_number - pn <= depth
}

/// Pure transcription of the production helper after metadata lookups are resolved.
fn filter_deep_parents(max_parent_depth: i64, ranked: &[Parent]) -> Vec<u64> {
    let Some((main, secondary)) = ranked.split_first() else {
        return Vec::new();
    };
    let max_block_number = ranked
        .iter()
        .map(|parent| parent.block_number)
        .max()
        .expect("ranked is nonempty");
    std::iter::once(main.hash)
        .chain(
            secondary
                .iter()
                .filter(|p| max_block_number - p.block_number <= max_parent_depth)
                .map(|p| p.hash),
        )
        .collect()
}

// A ranked tip list of 1..=8 parents with UNIQUE hashes (position index i ⇒ hash i)
// and block numbers in [0, 200], paired with a max_parent_depth in [0, 100]. The
// wide number range vs. the depth range yields a mix of within-depth and too-deep
// secondaries, including cases where a non-head candidate is the tallest.
prop_compose! {
    fn scenario()(
        block_numbers in prop::collection::vec(0i64..=200, 1..=8),
        max_parent_depth in 0i64..=100,
    ) -> (i64, Vec<Parent>) {
        let ranked: Vec<Parent> = block_numbers
            .iter()
            .enumerate()
            .map(|(i, n)| Parent { hash: i as u64, block_number: *n })
            .collect();
        (max_parent_depth, ranked)
    }
}

proptest! {
    // (a) SOUNDNESS: every retained secondary parent is within depth.
    #[test]
    fn retained_secondaries_are_within_depth((depth, ranked) in scenario()) {
        let main = ranked[0];
        let max_block_number = ranked.iter().map(|parent| parent.block_number).max().unwrap();
        let retained = filter_deep_parents(depth, &ranked);
        for &h in &retained {
            if h == main.hash {
                continue; // the main parent is covered by property (b)
            }
            let pn = ranked
                .iter()
                .find(|p| p.hash == h)
                .expect("every retained hash must be an input parent")
                .block_number;
            prop_assert!(
                within_depth(max_block_number, pn, depth),
                "retained secondary hash {} at height {} violates within_depth \
                 (maximum height {}, depth {})",
                h, pn, max_block_number, depth
            );
        }
    }

    // (b) MAIN-KEPT: the main parent is always retained, and retained first.
    #[test]
    fn main_parent_always_retained_first((depth, ranked) in scenario()) {
        let main = ranked[0];
        let retained = filter_deep_parents(depth, &ranked);
        prop_assert!(!retained.is_empty(), "output must be non-empty (main present)");
        prop_assert_eq!(retained[0], main.hash, "main parent must be retained FIRST");
        prop_assert!(retained.contains(&main.hash), "main parent must be retained");
    }

    // (c) COMPLETENESS: nothing satisfying within_depth is dropped.
    #[test]
    fn nothing_within_depth_is_dropped((depth, ranked) in scenario()) {
        let max_block_number = ranked.iter().map(|parent| parent.block_number).max().unwrap();
        let retained: BTreeSet<u64> =
            filter_deep_parents(depth, &ranked).into_iter().collect();
        for p in ranked.iter().skip(1) {
            if within_depth(max_block_number, p.block_number, depth) {
                prop_assert!(
                    retained.contains(&p.hash),
                    "secondary hash {} at height {} satisfies within_depth \
                     (maximum height {}, depth {}) but was DROPPED",
                    p.hash, p.block_number, max_block_number, depth
                );
            }
        }
    }

    // (d) EXACT SET (capstone): the concrete filter's retained set equals the
    // abstract spec set {main} ∪ prop_filter(secondaries) — soundness ∧ completeness
    // ∧ main-retention as one set equality, tying the concrete output to GuardBridge's
    // `prop_filter` directly.
    #[test]
    fn retained_set_equals_spec((depth, ranked) in scenario()) {
        let main = ranked[0];
        let max_block_number = ranked.iter().map(|parent| parent.block_number).max().unwrap();
        let expected: BTreeSet<u64> = std::iter::once(main.hash)
            .chain(
                ranked
                    .iter()
                    .skip(1)
                    .filter(|p| within_depth(max_block_number, p.block_number, depth))
                    .map(|p| p.hash),
            )
            .collect();
        let got: BTreeSet<u64> =
            filter_deep_parents(depth, &ranked).into_iter().collect();
        prop_assert_eq!(got, expected);
    }
}
