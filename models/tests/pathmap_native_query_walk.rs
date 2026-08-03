//! `pathmap_native_query::next_value_path` — the trie-enumeration STEP.
//!
//! `models/src/rust/pathmap_native_query.rs` is the whole of the EPathMap
//! native-query surface and it carried NO tests at all; this file is its
//! first. It exists because of one specific question, asked below as an
//! executable experiment.
//!
//! # ★ The falsification experiment (why this file was written first)
//!
//! `{| 1, 2, 3 |}` — an EPathMap of BARE (non-list) elements — makes
//! `z.toNextLeaf()` a FIXED POINT: `getPath()` reports `[1]` at every step and
//! the walk never terminates. Two rival explanations were on the table, and
//! they are distinguished by ONE measurement:
//!
//! | `next_value_path(bare {1,2,3}, [03 02 00])` | verdict |
//! |---|---|
//! | `Some([[03 04]])` — advances | the from_key mismatch alone causes it; there is ONE defect (key termination) |
//! | `Some([[03 02]])` — restarts | the fixed point is INSIDE `next_value_path`; a SECOND, INDEPENDENT defect |
//!
//! The reason a measurement was needed rather than an argument: the recorded
//! explanation ("a dangling from_key makes the zipper restart") is REFUTED by
//! [`dangling_from_key_below_a_two_entry_node_advances_correctly`] — a
//! from_key that dangles below an existing leaf advances perfectly well there.
//! So "dangling ⇒ restart" cannot be the mechanism, and the true mechanism has
//! to be read off the data.
//!
//! # ★ The measured answer: `Some([[03 02]])` — RESTART
//!
//! There is a second, independent defect, and it is a LIVENESS defect: fixing
//! key termination removes only one of its triggers. The others survive —
//! `readZipperAt` on a miss, any `descendTo` into empty space.
//!
//! ## The mechanism, read off the crate
//!
//! `next_value_path` positions a root-rooted read zipper with
//! `move_to_path(from_key)` and calls `to_next_val()`.
//! `ReadZipperCore::to_next_get_val` (`pathmap-0.2.2` `src/zipper.rs:2377`)
//! opens iteration with
//!
//! ```text
//! self.focus_iter_token = self.focus_node.iter_token_for_path(self.node_key());
//! ```
//!
//! where `node_key()` is the part of the focus path that lies inside the
//! focus NODE. For a dangling focus that is a MULTI-byte remainder, and
//! `DenseByteNode::iter_token_for_path` (`src/dense_byte_node.rs:930`) is
//!
//! ```text
//! if key.len() != 1 { self.new_iter_token() } else { ...bits above key[0]... }
//! ```
//!
//! `new_iter_token()` is the node's FULL child mask — the START of the node.
//! So a focus dangling two or more bytes below a dense node silently rewinds
//! iteration to that node's FIRST child instead of resuming after the focus.
//! `LineListNode::iter_token_for_path` (`src/line_list_node.rs:1910`) instead
//! compares the whole key lexicographically, which is why a two-entry node
//! handles the very same dangling shape correctly — the refutation above.
//!
//! Consequences of the rewind, both pinned below:
//!
//!   * a walk that steps with a dangling from_key never advances past the
//!     node's first child — `toNextLeaf` is a fixed point; and
//!   * a walk PAST THE LAST entry restarts from the FIRST — `to_next_val`
//!     never reports exhaustion, so the counted-walk idiom cannot terminate
//!     even by accident.
//!
//! # ★ STATUS: FIXED (stage 2)
//!
//! `next_value_key` / `next_value_path` no longer ask the crate to iterate
//! from a dangling focus; see their doc comments for the specification and
//! the algorithm. The four `witness_`-prefixed tests this file was committed
//! with have been replaced by the positive twins each of them named:
//!
//! | witness (the defect)                            | positive twin (the specification)                     |
//! |-------------------------------------------------|-------------------------------------------------------|
//! | `witness_dangling_from_key_rewinds_to_the_first_child` | `walk_from_a_dangling_from_key_advances_to_the_next_entry` |
//! | `witness_walk_past_the_last_key_restarts_from_the_first` | `walk_from_past_the_last_key_is_exhausted`         |
//! | `witness_bounded_walk_over_bare_elements_never_advances` | `bounded_walk_over_bare_elements_visits_every_entry_once` |
//!
//! ⚠ What stage 2 does NOT fix: the walk now VISITS every bare entry exactly
//! once and terminates, but `EZipper.current_path` is still lossy, so
//! `getPath()` still reports the singleton `[1]` for a bare `1` and
//! `getLeaf()` still answers `Nil`. Stage 2 converts a LIVENESS failure into a
//! WRONG-ANSWER failure — which is progress precisely because wrong answers
//! are assertable.

use models::rhoapi::Par;
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_integration::{create_set_pathmap_from_elements, RholangSetPathMap};
use models::rust::pathmap_native_query::{next_value_key, next_value_path};
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────────

/// `{| 1, 2, 3 |}` — three BARE `GInt` elements. Trie keys `03 02`, `03 04`,
/// `03 06` (tag `0x03` = GInt, payload = zigzag varint), none terminated:
/// `encode_trie_path`'s bare arm emits no `0x00`.
fn bare_ints() -> (Vec<Par>, RholangSetPathMap) {
    let elements: Vec<Par> = (1..=3)
        .map(|i| new_gint_par(i, Vec::new(), false))
        .collect();
    let map = create_set_pathmap_from_elements(&elements, None).map;
    (elements, map)
}

/// `{| ["a"], ["b"] |}` — SPLIT singleton lists. Trie keys `04 01 61 00`,
/// `04 01 62 00`; two entries, so the branch is a `LineListNode`.
fn split_strings() -> RholangSetPathMap {
    let elements: Vec<Par> = ["a", "b"]
        .iter()
        .map(|s| {
            new_elist_par(
                vec![new_gstring_par(s.to_string(), Vec::new(), false)],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )
        })
        .collect();
    create_set_pathmap_from_elements(&elements, None).map
}

// ─────────────────────────────────────────────────────────────────────────────
// The keys the experiment is stated in
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn bare_int_trie_keys_are_unterminated() {
    let (elements, map) = bare_ints();
    assert_eq!(encode_trie_path(&elements[0]), vec![0x03, 0x02]);
    assert_eq!(encode_trie_path(&elements[1]), vec![0x03, 0x04]);
    assert_eq!(encode_trie_path(&elements[2]), vec![0x03, 0x06]);
    assert_eq!(map.val_count(), 3, "three distinct entries, no collisions");

    // The DEFECT restated in one line: the key a reader rebuilds from the
    // per-element segments (`segments_to_key(par_to_path(p), true)`) is the
    // key of the SINGLETON LIST `[1]`, not of the bare `1`.
    let singleton = new_elist_par(
        vec![new_gint_par(1, Vec::new(), false)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    );
    assert_eq!(encode_trie_path(&singleton), vec![0x03, 0x02, 0x00]);
    assert!(map.get([0x03u8, 0x02, 0x00]).is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// The experiment
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn walk_from_before_the_first_key_finds_the_first_entry() {
    let (_, map) = bare_ints();
    assert_eq!(
        next_value_path(&map, &[0x00]),
        Some(vec![vec![0x03, 0x02]]),
        "0x00 sorts below every key, so the next value is the first entry"
    );
}

#[test]
fn walk_from_an_existing_key_advances() {
    let (_, map) = bare_ints();
    // from_key EXISTS (and is a value): the crate's own well-covered case.
    assert_eq!(
        next_value_path(&map, &[0x03, 0x02]),
        Some(vec![vec![0x03, 0x04]])
    );
    assert_eq!(
        next_value_path(&map, &[0x03, 0x04]),
        Some(vec![vec![0x03, 0x06]])
    );
    assert_eq!(next_value_path(&map, &[0x03, 0x06]), None, "last entry");
}

/// ★ THE REFUTATION of "a dangling from_key makes the zipper restart".
///
/// `04 01 61 00 00` dangles one byte below the existing value-leaf
/// `04 01 61 00`, exactly the shape [`witness_dangling_from_key_rewinds_to_the_first_child`]
/// uses — and here it advances correctly, because a two-entry branch is a
/// `LineListNode` whose `iter_token_for_path` compares the whole key.
#[test]
fn dangling_from_key_below_a_two_entry_node_advances_correctly() {
    let map = split_strings();
    assert_eq!(
        next_value_path(&map, &[0x04, 0x01, 0x61, 0x00]),
        Some(vec![vec![0x04, 0x01, 0x62]]),
        "the existing-key case"
    );
    assert_eq!(
        next_value_path(&map, &[0x04, 0x01, 0x61, 0x00, 0x00]),
        Some(vec![vec![0x04, 0x01, 0x62]]),
        "★ the SAME dangling shape — and it advances"
    );
}

/// ★★★ THE DISCRIMINATOR, now its positive twin. `03 02 00` is the key a
/// reader rebuilds for the bare entry `1`; it dangles below the existing
/// value-leaf `03 02`.
///
/// MEASURED BEFORE STAGE 2: `Some([[03 02]])` — the walk answered with the
/// entry it started from, making `toNextLeaf` a fixed point *inside*
/// `next_value_path`, independently of how the from_key was built.
///
/// SPECIFICATION: the LEAST value-key strictly greater than `03 02 00` is
/// `03 04` (`03 02` is smaller — a proper prefix sorts before what it
/// prefixes).
#[test]
fn walk_from_a_dangling_from_key_advances_to_the_next_entry() {
    let (_, map) = bare_ints();
    assert_eq!(
        next_value_key(&map, &[0x03, 0x02, 0x00]),
        Some(vec![0x03, 0x04]),
        "the least key strictly above 03 02 00"
    );
    assert_eq!(
        next_value_path(&map, &[0x03, 0x02, 0x00]),
        Some(vec![vec![0x03, 0x04]])
    );
}

/// The LIVENESS half, now its positive twin. A from_key past the LAST entry
/// exhausts the walk; before stage 2 the dense node rewound to its first
/// child, so a counted walk could not terminate even by running off the end.
#[test]
fn walk_from_past_the_last_key_is_exhausted() {
    let (_, map) = bare_ints();
    assert_eq!(next_value_key(&map, &[0x03, 0x06, 0x00]), None);
    assert_eq!(next_value_path(&map, &[0x03, 0x06, 0x00]), None);
}

/// Dangling foci at every depth and on both sides of every key — the general
/// statement of what the two twins above pin at one point each.
#[test]
fn every_dangling_from_key_answers_by_byte_order() {
    let (_, map) = bare_ints(); // keys 03 02, 03 04, 03 06
    for (from_key, expected) in [
        (vec![0x00u8], Some(vec![0x03u8, 0x02])),
        (vec![0x02], Some(vec![0x03, 0x02])),
        (vec![0x03], Some(vec![0x03, 0x02])),
        (vec![0x03, 0x00], Some(vec![0x03, 0x02])),
        (vec![0x03, 0x01], Some(vec![0x03, 0x02])),
        (vec![0x03, 0x02, 0x00], Some(vec![0x03, 0x04])),
        (vec![0x03, 0x02, 0xff], Some(vec![0x03, 0x04])),
        (vec![0x03, 0x03], Some(vec![0x03, 0x04])),
        (vec![0x03, 0x04, 0x00], Some(vec![0x03, 0x06])),
        (vec![0x03, 0x05], Some(vec![0x03, 0x06])),
        (vec![0x03, 0x06, 0x00], None),
        (vec![0x03, 0x07], None),
        (vec![0x03, 0xff, 0xff], None),
        (vec![0x04], None),
        (vec![0xff], None),
    ] {
        assert_eq!(
            next_value_key(&map, &from_key),
            expected,
            "from {:02x?}",
            from_key
        );
    }
}

/// The rewind is a property of the FOCUS NODE, not of the from_key's length:
/// a one-byte dangling focus at the root behaves correctly, because the root
/// of this trie is a list node.
#[test]
fn one_byte_dangling_focus_at_the_root_is_handled() {
    let (_, map) = bare_ints();
    assert_eq!(
        next_value_path(&map, &[0xff]),
        None,
        "0xff sorts above every key"
    );
    assert_eq!(next_value_path(&map, &[]), Some(vec![vec![0x03, 0x02]]));
}

/// The reducer's `toNextLeaf` idiom, run end to end at THIS layer: seed with
/// the terminated key the reducer builds (`segments_to_key(current_path,
/// true)`) and step. BOUNDED by construction — a regression FAILS rather than
/// hangs.
///
/// Before stage 2 this collected entry 1 four times and never exhausted.
///
/// ⚠ Note WHY it works even though the reducer's from_key is still the wrong
/// key: `03 02 00` is not the bare entry's key, but it does sort strictly
/// between `03 02` and `03 04`, so an order-correct step lands on the right
/// next entry anyway. The cursor remains lossy — `getPath()` still reports the
/// singleton — which is stages 3 and 4, not this one.
#[test]
fn bounded_walk_over_bare_elements_visits_every_entry_once() {
    let (_, map) = bare_ints();
    let mut visited: Vec<Vec<Vec<u8>>> = Vec::with_capacity(4);
    // Start where the reducer starts a walk: the root zipper's empty path.
    let mut current: Vec<Vec<u8>> = Vec::new();
    let mut exhausted_at = None;
    for step in 0..4 {
        // The reducer's from_key: `segments_to_key(current_path, true)`.
        let mut from_key: Vec<u8> = current.iter().flatten().copied().collect();
        from_key.push(0x00);
        match next_value_path(&map, &from_key) {
            Some(segments) => {
                current = segments.clone();
                visited.push(segments);
            }
            None => {
                exhausted_at = Some(step);
                break;
            }
        }
    }
    assert_eq!(
        visited,
        vec![vec![vec![0x03u8, 0x02]], vec![vec![0x03u8, 0x04]], vec![
            vec![0x03u8, 0x06]
        ],],
        "every entry exactly once, in order"
    );
    assert_eq!(
        exhausted_at,
        Some(3),
        "step leafCount()+1 reports exhaustion"
    );
}

/// The SPLIT control for the same idiom: every element a ground list, which
/// is the shape the existing `zipper_enumeration_spec.rs` fixture has — and
/// it terminates and enumerates correctly. This is the containment evidence:
/// the defect lives in the bare arm.
#[test]
fn bounded_walk_over_split_elements_visits_every_entry_once() {
    let map = split_strings();
    let mut visited: Vec<Vec<Vec<u8>>> = Vec::with_capacity(3);
    let mut current: Vec<Vec<u8>> = Vec::new();
    for _ in 0..3 {
        let mut from_key: Vec<u8> = current.iter().flatten().copied().collect();
        from_key.push(0x00);
        match next_value_path(&map, &from_key) {
            Some(segments) => {
                current = segments.clone();
                visited.push(segments);
            }
            None => break,
        }
    }
    assert_eq!(
        visited,
        vec![vec![vec![0x04u8, 0x01, 0x61]], vec![vec![
            0x04u8, 0x01, 0x62
        ]]],
        "two entries, each once, then exhausted"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// The ORDER ORACLE — the specification, checked against a full scan
// ═══════════════════════════════════════════════════════════════════════════
//
// `next_value_key(map, k)` is specified as "the LEAST key in `map` strictly
// greater than `k`". That is directly computable by scanning every key, which
// makes the specification itself the test oracle — no reasoning about zipper
// internals, node shapes, or which of `LineListNode` / `DenseByteNode` /
// `BridgeNode` the trie happens to have built at any depth. Random RAW keys
// are used deliberately: they reach node shapes the codec never produces, and
// the dense-node rewind that motivated stage 2 is exactly a node-shape effect.

/// The specification, evaluated by brute force. `Vec<u8>`'s `Ord` is
/// byte-lexicographic with a proper prefix sorting FIRST, which is the trie's
/// depth-first order.
fn reference_next_value_key(map: &RholangSetPathMap, from_key: &[u8]) -> Option<Vec<u8>> {
    map.iter()
        .map(|(key, _)| key)
        .filter(|key| key.as_slice() > from_key)
        .min()
}

fn trie_of_raw_keys(keys: &[Vec<u8>]) -> RholangSetPathMap {
    let mut map = RholangSetPathMap::new();
    for key in keys {
        map.insert(key.clone(), ());
    }
    map
}

/// Probe keys around a trie: every key, every proper prefix, every key
/// extended by a low / high byte, and the empty key. These are precisely the
/// shapes a lossy cursor produces — a key with a spurious `0x00`, a key
/// truncated to an element boundary, a key that names a branch.
fn probe_keys(keys: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut probes: Vec<Vec<u8>> = vec![Vec::new(), vec![0x00], vec![0xff]];
    for key in keys {
        probes.push(key.clone());
        for cut in 0..key.len() {
            probes.push(key[..cut].to_vec());
        }
        for suffix in [0x00u8, 0x01, 0x7f, 0xff] {
            let mut extended = key.clone();
            extended.push(suffix);
            probes.push(extended);
        }
    }
    probes.sort();
    probes.dedup();
    probes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// ★ THE SOUNDNESS GATE. For arbitrary tries and arbitrary from_keys —
    /// existing, dangling, truncated, or absent — the answer is the least key
    /// strictly greater than the from_key.
    #[test]
    fn prop_next_value_key_matches_the_order_oracle(
        keys in proptest::collection::vec(
            proptest::collection::vec(0u8..6, 0..5), 1..12)
    ) {
        let map = trie_of_raw_keys(&keys);
        for probe in probe_keys(&keys) {
            prop_assert_eq!(
                next_value_key(&map, &probe),
                reference_next_value_key(&map, &probe),
                "from {:02x?}", probe
            );
        }
    }

    /// The same gate over a WIDE byte alphabet, which builds dense nodes:
    /// a `DenseByteNode` is what rewinds when asked to iterate from a
    /// multi-byte dangling focus, so the fix has to be exercised there.
    #[test]
    fn prop_next_value_key_matches_the_oracle_on_dense_nodes(
        keys in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 1..4), 8..40)
    ) {
        let map = trie_of_raw_keys(&keys);
        for probe in probe_keys(&keys) {
            prop_assert_eq!(
                next_value_key(&map, &probe),
                reference_next_value_key(&map, &probe),
                "from {:02x?}", probe
            );
        }
    }

    /// ★ TERMINATION, as a BOUNDED property. A walk seeded at the root and
    /// stepped by the key it last returned visits every entry exactly once, in
    /// ascending key order, and step `val_count() + 1` is `None`.
    ///
    /// Bounded by construction: the loop runs `val_count() + 1` times and
    /// asserts afterwards, so a regression FAILS rather than hangs.
    #[test]
    fn prop_bounded_walk_visits_every_entry_exactly_once(
        keys in proptest::collection::vec(
            proptest::collection::vec(0u8..4, 0..5), 1..14)
    ) {
        let map = trie_of_raw_keys(&keys);
        let count = map.val_count();

        let mut expected: Vec<Vec<u8>> = keys.clone();
        expected.sort();
        expected.dedup();
        prop_assert_eq!(expected.len(), count);

        let mut visited: Vec<Vec<u8>> = Vec::with_capacity(count);
        // The walk's seed: strictly below every key, since a key cannot be
        // shorter than the empty key and no key is empty-and-less.
        let mut cursor: Option<Vec<u8>> = None;
        let mut exhausted_at = None;
        for step in 0..=count {
            let from_key = cursor.clone().unwrap_or_default();
            // Seed the FIRST step below the least key rather than at it: an
            // empty from_key would skip an entry stored at the empty key.
            let answer = match step {
                0 => match map.get::<&[u8]>(&[]) {
                    Some(_) => Some(Vec::new()),
                    None => next_value_key(&map, &from_key),
                },
                _ => next_value_key(&map, &from_key),
            };
            match answer {
                Some(key) => { visited.push(key.clone()); cursor = Some(key); }
                None => { exhausted_at = Some(step); break; }
            }
        }
        prop_assert_eq!(&visited, &expected, "every entry exactly once, in order");
        prop_assert_eq!(exhausted_at, Some(count), "step count+1 is exhaustion");
    }
}
