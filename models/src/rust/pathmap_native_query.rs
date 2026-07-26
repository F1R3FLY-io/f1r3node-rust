//! Native trie-descent implementations of the EPathMap prefix queries.
//!
//! The interpreter's zipper query methods (`pathExists`, `getSubtrie`,
//! `childCount`, `descendFirst`, `descendIndexedBranch`, `toNextSibling`,
//! `toPrevSibling`) previously answered prefix questions by iterating the
//! ENTIRE map (`map.iter()` + `starts_with` filtering) — O(map) host work per
//! query. The helpers here descend the PathMap trie natively so per-query work
//! is O(|prefix| + |answer|) instead.
//!
//! # Semantics contract (provable identity with the retired scans)
//!
//! All helpers assume a PURE-INSERT trie: every map they receive is built by
//! `create_pathmap_from_elements` (inserts only), so every path present in the
//! trie is a byte-prefix of some inserted key.
//!
//! EPathMap wire W2b-1: keys are now the CANONICAL PATH CODEC bytes
//! (`canonical_path::encode_trie_path`) — self-delimiting, prefix-free
//! segments with NO `0xFF` separator (split-list paths end in a single `0x00`
//! terminator). `path_prefix_exists` and `collect_subtrie_values` are
//! byte-generic and carry over unchanged; [`collect_child_segments`] delegates
//! to the parser-state DFS [`canonical_path::collect_child_segments_codec`]
//! (the former `0xFF`-separator special-case and the `BitMask::clear_bit`-XOR
//! workaround retire — the codec's prefix-freeness makes plain ascending
//! traversal correct).
//!
//! Ordering: `PathMap::iter()` is the trie's depth-first traversal —
//! `descend_first_byte` takes the smallest child byte and
//! `to_next_sibling_byte` advances via `ByteMask::next_bit` (ascending) — so
//! keys stream in ascending byte-lexicographic order with a node's value
//! emitted before its descendants. The helpers preserve exactly the orders the
//! scans produced (see each helper's proof sketch).

use super::canonical_path::collect_child_segments_codec;
use super::pathmap_integration::RholangPathMap;
use crate::rhoapi::Par;

/// The byte that terminated every path segment in the RETIRED (pre-W2b-1)
/// Rholang PathMap key format. Kept as documentation-of-record; the codec
/// keys carry no in-band separator.
pub const SEGMENT_SEPARATOR: u8 = 0xFF;

/// Does any key in `map` have `key` as a byte-prefix?
///
/// Replaces `map.iter().any(|(k, _)| k.starts_with(key))`.
///
/// Identity: in a pure-insert trie, the path `key` exists **iff** `key` is a
/// byte-prefix of some inserted key (every trie path is a prefix of an
/// inserted key, and every prefix of an inserted key is a trie path).
/// `PathMap::path_exists_at` checks precisely trie-path existence.
/// Cost: O(|key|) instead of O(Σ|keys|).
pub fn path_prefix_exists(map: &RholangPathMap, key: &[u8]) -> bool {
    map.path_exists_at(key)
}

/// Collect (clones of) all values whose key has `prefix` as a byte-prefix, in
/// map-iteration order.
///
/// Replaces
/// `map.iter().filter(|(k, _)| k.starts_with(prefix)).map(|(_, v)| v.clone())`.
///
/// Identity: keys sharing the byte-prefix `prefix` form a CONTIGUOUS run of
/// the ascending byte-lexicographic key order (any key in the interval
/// `[prefix, succ(prefix))` and no key outside it), and `map.iter()` streams
/// keys in exactly that order. A read zipper rooted at `prefix` performs the
/// same depth-first traversal restricted to the subtrie below `prefix`, so it
/// yields the same values in the same relative order — including a value
/// stored exactly at `prefix` first (the iterator's initial focus-value
/// check), matching where the full scan encounters `key == prefix`.
/// A nonexistent `prefix` yields nothing in both forms (a key with prefix `p`
/// would make `p` an existing path). Cost: O(|prefix| + |subtrie|).
pub fn collect_subtrie_values(map: &RholangPathMap, prefix: &[u8]) -> Vec<Par> {
    map.read_zipper_at_borrowed_path(prefix)
        .into_iter()
        .map(|(_, value)| value.clone())
        .collect()
}

/// Collect the distinct immediate child segments below `prefix`, in ascending
/// raw byte-lexicographic segment order, truncated to the first `limit`
/// segments when `limit` is `Some`.
///
/// EPathMap wire W2b-1: keys are canonical-path-codec bytes, so a "child
/// segment" is one complete CODEC segment (`encode_trie_segment`) below
/// `prefix` — determined by the codec grammar's extent parser, not a `0xFF`
/// separator. This delegates to the parser-state DFS
/// [`collect_child_segments_codec`]; the identity proof (membership /
/// distinctness / order / early-stop) lives there. Because `enc` is
/// prefix-free, plain ascending traversal yields ascending segment order
/// directly — the retired separator-first special case and the
/// `BitMask::clear_bit`-XOR workaround are gone.
///
/// Cost: O(|prefix| + Σ|emitted distinct segments|) instead of O(map).
pub fn collect_child_segments(
    map: &RholangPathMap,
    prefix: &[u8],
    limit: Option<usize>,
) -> Vec<Vec<u8>> {
    collect_child_segments_codec(map, prefix, limit)
}

// ═══════════════════════════════════════════════════════════════════════════
// Trie ENUMERATION support (`leafCount` / `toNextLeaf`)
// ═══════════════════════════════════════════════════════════════════════════
//
// Both SURFACE capability the `pathmap` crate already has — `val_count()` and
// `to_next_val()` — as SEGMENT-level answers, so `reduce.rs` continues to work
// in `EZipper.current_path`'s own vocabulary and never handles raw codec bytes
// for navigation (the same contract `collect_child_segments` already keeps).

/// How many VALUES live at and below `prefix`.
///
/// Replaces `collect_subtrie_values(map, prefix).len()`, which materializes and
/// CLONES every `Par` in the subtrie only to discard them; `val_count()` is the
/// trie's own catamorphism and allocates nothing.
///
/// Identity: `val_count()` counts the values at and below the zipper's focus
/// (including the focus itself), and a zipper rooted at `prefix` covers exactly
/// the keys having `prefix` as a byte-prefix — the same set
/// [`collect_subtrie_values`] streams.
///
/// Cost: O(|prefix| + |subtrie|). This is NOT a constant-time query; it is
/// meant to be read once as a walk bound, not once per step.
pub fn subtrie_value_count(map: &RholangPathMap, prefix: &[u8]) -> usize {
    use pathmap::zipper::ZipperMoving;
    map.read_zipper_at_borrowed_path(prefix).val_count()
}

/// The path of the next VALUE after `from_key` in depth-first order, as codec
/// segments — or `None` when `from_key` is at or past the last value.
///
/// This is the enumeration step. `to_next_val()` advances to positions that
/// carry a value, which (unlike a byte- or child-segment move) are always
/// complete, decodable keys, so the caller can always both decode the path and
/// read the value there.
///
/// ## ⚠ `None` means EXHAUSTED, and the caller must not keep walking
///
/// `to_next_val()` does not merely report `false` at the end — it also RESETS
/// THE ZIPPER TO THE ROOT (`pathmap/src/zipper.rs:546`). The focus it leaves
/// behind is a valid root position, so a caller that treated the reset zipper
/// as a legitimate next step would silently RESTART the walk and loop forever
/// with no error raised anywhere. `None` is therefore load-bearing: it must
/// terminate the walk, never continue it.
///
/// The trailing split-list terminator is not a segment, so the returned vector
/// is exactly the key's elements — the shape `EZipper.current_path` stores.
pub fn next_value_path(map: &RholangPathMap, from_key: &[u8]) -> Option<Vec<Vec<u8>>> {
    use pathmap::zipper::{ZipperAbsolutePath, ZipperIteration, ZipperMoving};

    // Rooted at the trie ROOT with the focus moved to `from_key`: the walk must
    // be able to ASCEND out of the current branch to reach the next one, which
    // a zipper rerooted by `read_zipper_at_borrowed_path` cannot do (its
    // ancestor stack is empty). Rooting at the root also makes `origin_path()`
    // report the ABSOLUTE key, which is what the segments must describe.
    let mut zipper = map.read_zipper();
    zipper.move_to_path(from_key);
    match zipper.to_next_val() {
        true => Some(super::pathmap_zipper::unflatten_segments(
            zipper.origin_path(),
        )),
        false => None,
    }
}
