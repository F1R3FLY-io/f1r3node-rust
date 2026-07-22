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
