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
//! ⚠ The tests marked `witness_` below assert the DEFECTIVE answers on
//! purpose. They are the executable record of the measurement, and each names
//! the positive twin that must replace it once `next_value_path` is made
//! sound for dangling foci. This file is committed with them RED-in-meaning
//! and green-in-CI so the flip is a visible diff.

use models::rhoapi::Par;
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_integration::{create_pathmap_from_elements, RholangPathMap};
use models::rust::pathmap_native_query::next_value_path;
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

// ─────────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────────

/// `{| 1, 2, 3 |}` — three BARE `GInt` elements. Trie keys `03 02`, `03 04`,
/// `03 06` (tag `0x03` = GInt, payload = zigzag varint), none terminated:
/// `encode_trie_path`'s bare arm emits no `0x00`.
fn bare_ints() -> (Vec<Par>, RholangPathMap) {
    let elements: Vec<Par> = (1..=3)
        .map(|i| new_gint_par(i, Vec::new(), false))
        .collect();
    let map = create_pathmap_from_elements(&elements, None).map;
    (elements, map)
}

/// `{| ["a"], ["b"] |}` — SPLIT singleton lists. Trie keys `04 01 61 00`,
/// `04 01 62 00`; two entries, so the branch is a `LineListNode`.
fn split_strings() -> RholangPathMap {
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
    create_pathmap_from_elements(&elements, None).map
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
    assert_eq!(next_value_path(&map, &[0x03, 0x02]), Some(vec![vec![0x03, 0x04]]));
    assert_eq!(next_value_path(&map, &[0x03, 0x04]), Some(vec![vec![0x03, 0x06]]));
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

/// ★★★ THE DISCRIMINATOR. `03 02 00` is the key a reader rebuilds for the
/// bare entry `1`; it dangles below the existing value-leaf `03 02`.
///
/// MEASURED: `Some([[03 02]])` — the walk answers with the entry it started
/// from. `toNextLeaf` is therefore a fixed point *inside* `next_value_path`,
/// independently of how the from_key was built.
///
/// ⚠ WITNESS OF A DEFECT — to be replaced by its positive twin
/// `walk_from_a_dangling_from_key_advances_to_the_next_entry`, which asserts
/// `Some([[03 04]])`: the least value-key strictly greater than `03 02 00`.
#[test]
fn witness_dangling_from_key_rewinds_to_the_first_child() {
    let (_, map) = bare_ints();
    assert_eq!(
        next_value_path(&map, &[0x03, 0x02, 0x00]),
        Some(vec![vec![0x03, 0x02]]),
        "★ RESTART: answered with entry 1, whose key 03 02 is BELOW the from_key"
    );
}

/// ⚠ WITNESS OF A DEFECT — the liveness half. A from_key past the LAST entry
/// must exhaust the walk; instead the dense node rewinds to its first child,
/// so a counted walk can never terminate, not even by running off the end.
///
/// To be replaced by its positive twin
/// `walk_from_past_the_last_key_is_exhausted`, which asserts `None`.
#[test]
fn witness_walk_past_the_last_key_restarts_from_the_first() {
    let (_, map) = bare_ints();
    assert_eq!(
        next_value_path(&map, &[0x03, 0x06, 0x00]),
        Some(vec![vec![0x03, 0x02]]),
        "★ RESTART: past-the-end answered with the FIRST entry"
    );
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
/// ⚠ WITNESS OF A DEFECT — to be replaced by its positive twin
/// `bounded_walk_over_bare_elements_visits_every_entry_once`.
#[test]
fn witness_bounded_walk_over_bare_elements_never_advances() {
    let (_, map) = bare_ints();
    let mut visited: Vec<Vec<Vec<u8>>> = Vec::with_capacity(4);
    // Start where the reducer starts a walk: the root zipper's empty path.
    let mut current: Vec<Vec<u8>> = Vec::new();
    for _ in 0..4 {
        // The reducer's from_key: `segments_to_key(current_path, true)`.
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
        vec![
            vec![vec![0x03u8, 0x02]],
            vec![vec![0x03u8, 0x02]],
            vec![vec![0x03u8, 0x02]],
            vec![vec![0x03u8, 0x02]],
        ],
        "★ FIXED POINT: entry 1 forever; leafCount() is 3 but the walk never \
         reaches entries 2 or 3 and never reports exhaustion"
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
        vec![vec![vec![0x04u8, 0x01, 0x61]], vec![vec![0x04u8, 0x01, 0x62]]],
        "two entries, each once, then exhausted"
    );
}
