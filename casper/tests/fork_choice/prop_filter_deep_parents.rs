// C12 proptests — the concrete `Estimator::filter_deep_parents` conforms to the
// abstract `within_depth`/`prop_filter` model of the fork-choice GuardBridge.
//
// The Rocq module `formal/rocq/fork_choice/theories/GuardBridge.v`:165-189 models
// the depth filter abstractly:
//     Definition within_depth (maxn mpd pn : nat) : bool := Nat.leb (maxn - pn) mpd.
//     Definition prop_filter (maxn mpd : nat) (nums : list nat) : list nat :=
//                    filter (within_depth maxn mpd) nums.
//     Theorem honest_forkchoice_parents_validate : ... parents_ok maxn (mpd+buf)
//                    (prop_filter maxn mpd nums) = true.
// but with no mechanical tie to the concrete Rust that actually filters parents.
// This file is that tie.
//
// The concrete function under test is
// `casper/src/rust/estimator.rs`:133-182 (`Estimator::filter_deep_parents`), whose
// `Some(max_parent_depth)` branch is:
//     let Some((main_hash, secondary_hashes)) = ranked_latest_hashes.split_first() ...;   // :156
//     let max_block_number = dag.lookup_unsafe(main_hash)?.block_number;                    // :164
//     let secondary_parents = secondary_hashes.iter().map(|h| dag.lookup_unsafe(h))...;     // :166-169
//     let shallow_parents = secondary_parents.into_iter()
//         .filter(|p| max_block_number - p.block_number <= max_parent_depth as i64)...;     // :171-174
//     Ok(once(main_hash).chain(shallow_parents.map(|p| p.block_hash)).collect())            // :176-178
//
// `filter_deep_parents` is a PRIVATE `async` method that reads block numbers out of
// a live LMDB-backed `KeyValueDagRepresentation` (heavy runtime scaffolding), and
// editing consensus source to expose it is out of scope. So — exactly as the sibling
// `finalized_floor/prop_bonds_from_floor.rs` transcribes its two consensus sites —
// we transcribe the `Some(..)` branch as a pure function with the `dag.lookup_unsafe`
// block-number lookups already RESOLVED (that is the precise state of the real code
// at :171-174, where the DAG has been consulted and only the integer arithmetic
// remains). `max_block_number` is, per :164, the MAIN parent's block number (the head
// of the ranked list), not a global maximum — the model preserves that exactly.
//
// Properties checked (the concrete realization of the GuardBridge model):
//   (a) SOUNDNESS    — every RETAINED secondary parent satisfies within_depth
//                      (`max_block_number - pn <= depth`); nothing too deep survives.
//   (b) MAIN-KEPT    — the main parent is ALWAYS retained, and retained FIRST
//                      (the real code chains `once(main_hash)` ahead of the filter).
//   (c) COMPLETENESS — nothing satisfying within_depth is dropped.
//   (d) EXACT SET    — capstone: the retained set equals {main} ∪ prop_filter(secondaries),
//                      tying the concrete output to the abstract spec set exactly.
//
// The arithmetic is modeled over SIGNED i64 (as the Rust performs it), not Rocq's
// truncated `nat` monus; the two agree on the retained set — when `pn > maxn` the
// i64 difference is negative (`<= depth`, retained) and Rocq's `maxn - pn` monus is
// `0 <= mpd` (also retained). No Rocq refinement lemma is added (the Rust is not
// extracted from Rocq; a lemma would restate `prop_filter` against itself).
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
/// within depth `depth` of the main parent's height `max_block_number` iff
/// `max_block_number - pn <= depth`.
fn within_depth(max_block_number: i64, pn: i64, depth: i64) -> bool {
    max_block_number - pn <= depth
}

/// Pure transcription of `Estimator::filter_deep_parents`'s `Some(max_parent_depth)`
/// branch (estimator.rs:156-178) with the `dag.lookup_unsafe(..).block_number`
/// lookups resolved. `ranked` is the ranked tip list (head = main parent). Returns
/// the retained hashes: the main parent first, then the within-depth secondaries in
/// input order — byte-for-byte the real code's `once(main).chain(filter(..))`.
fn filter_deep_parents(max_parent_depth: i64, ranked: &[Parent]) -> Vec<u64> {
    // estimator.rs:156 — head is the main parent, tail the secondaries. The real
    // code returns Err(InvalidArgument) on an empty tip list (:157-162, the P2-8
    // fix); the model surfaces "no tips" as an empty Vec. The properties below are
    // quantified over the non-empty domain (scenario() always yields >= 1 parent),
    // which is the real consensus invariant: rank_forkchoices returns >= 1 tip.
    let Some((main, secondary)) = ranked.split_first() else {
        return Vec::new();
    };
    // estimator.rs:164 — `max_block_number` is the MAIN parent's block number.
    let max_block_number = main.block_number;
    // estimator.rs:171-178 — retain within-depth secondaries; main parent first.
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
// secondaries, and (since a secondary's height may exceed the main's) exercises the
// negative-difference `pn > maxn` case that must always be retained.
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
                within_depth(main.block_number, pn, depth),
                "retained secondary hash {} at height {} violates within_depth \
                 (main height {}, depth {})",
                h, pn, main.block_number, depth
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
        let main = ranked[0];
        let retained: BTreeSet<u64> =
            filter_deep_parents(depth, &ranked).into_iter().collect();
        for p in ranked.iter().skip(1) {
            if within_depth(main.block_number, p.block_number, depth) {
                prop_assert!(
                    retained.contains(&p.hash),
                    "secondary hash {} at height {} satisfies within_depth \
                     (main height {}, depth {}) but was DROPPED",
                    p.hash, p.block_number, main.block_number, depth
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
        let expected: BTreeSet<u64> = std::iter::once(main.hash)
            .chain(
                ranked
                    .iter()
                    .skip(1)
                    .filter(|p| within_depth(main.block_number, p.block_number, depth))
                    .map(|p| p.hash),
            )
            .collect();
        let got: BTreeSet<u64> =
            filter_deep_parents(depth, &ranked).into_iter().collect();
        prop_assert_eq!(got, expected);
    }
}
