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

/// The KEY of the next VALUE strictly after `from_key`, or `None` when no
/// value-key sorts above it.
///
/// # Specification (one line, total, independent of the trie's shape)
///
/// > `next_value_key(map, k)` is the LEAST key in `map` that is strictly
/// > greater than `k` in byte-lexicographic order (a proper prefix sorts
/// > BEFORE what it prefixes), or `None` if there is none.
///
/// Byte-lexicographic order over keys IS the trie's depth-first order, so on
/// an EXISTING `from_key` this is exactly "the next value in a DFS walk", the
/// enumeration step `toNextLeaf` needs. Stating it as an order property rather
/// than as a walk is what makes it total: `from_key` need not exist, need not
/// be a value, and need not even be a prefix of anything in the map.
///
/// ## ⚠ Why this is not simply `move_to_path` + `to_next_val`
///
/// It was, and that was UNSOUND for a `from_key` that does not exist in the
/// trie. `ReadZipperCore::to_next_get_val` (`pathmap-0.2.2`
/// `src/zipper.rs:2377`) opens iteration with
///
/// ```text
/// self.focus_iter_token = self.focus_node.iter_token_for_path(self.node_key());
/// ```
///
/// where `node_key()` is the part of the focus path lying inside the focus
/// NODE. A focus that DANGLES two or more bytes past the deepest existing node
/// makes that a multi-byte key, and `DenseByteNode::iter_token_for_path`
/// (`src/dense_byte_node.rs:930`) is
///
/// ```text
/// if key.len() != 1 { self.new_iter_token() } else { ...bits above key[0]... }
/// ```
///
/// `new_iter_token()` is the node's FULL child mask — the START of the node —
/// so iteration silently REWINDS to that node's first child instead of
/// resuming after the focus. `LineListNode::iter_token_for_path`
/// (`src/line_list_node.rs:1910`) compares the whole key lexicographically and
/// does not, which is why the failure looked shape-dependent and unreproducible
/// in the small.
///
/// Two consequences, both of them liveness failures rather than wrong answers:
/// a walk stepped with a dangling key never advances past that node's first
/// child (`toNextLeaf` becomes a FIXED POINT), and a walk stepped from PAST
/// THE LAST key answers with the FIRST — so exhaustion is never reported and
/// a counted walk cannot terminate even by running off the end. Both are
/// pinned in `models/tests/pathmap_native_query_walk.rs`.
///
/// This is a `pathmap-0.2.2` behaviour, reported upstream separately. The
/// implementation below does not depend on it either way: it NEVER asks the
/// crate to iterate from a dangling focus.
///
/// ## The algorithm
///
/// ```text
/// d ← the length of the longest prefix of from_key that EXISTS in the trie
///
/// if d = |from_key|                       -- the focus exists
///     to_next_val()                       -- the crate's own covered case
///
/// otherwise                               -- from_key diverges at depth d
///     q ← from_key[..d]                   -- exists; focus here
///     floor ← from_key[d]                 -- the byte that does NOT exist
///     loop
///         c ← least child byte of q strictly above floor
///         if c exists
///             descend to q ‖ c            -- exists, so never dangling
///             return the first value at or below it
///         if q is the root: return None
///         floor ← last byte of q ; q ← q[..|q|-1]     -- ascend
/// ```
///
/// Correctness. Write `q = from_key[..d]` and `b = from_key[d]`. No key has
/// `q ‖ b` as a prefix (else `q ‖ b` would exist), so every key splits into:
/// keys not prefixed by `q` (handled by the ascending arm, which resumes the
/// same search one level up); keys equal to or prefixed by `q ‖ c` for `c < b`,
/// and the key `q` itself — all strictly less than `from_key`; and keys
/// prefixed by `q ‖ c` for `c > b` — all strictly greater. The least of those
/// is at or below the least such `c`, which is what `next_bit` returns. Every
/// zipper move the loop makes lands on an EXISTING path, so the rewind above
/// is unreachable by construction.
///
/// Totality. `to_next_val()` at an existing focus explores that subtrie first
/// and then continues forward in DFS order, so even a subtrie holding no value
/// at all (impossible in a pure-insert trie, but not assumed here) yields the
/// correct continuation rather than a wrong answer.
///
/// Cost: O(|from_key| + |answer|) trie steps — the same order as the descent
/// it replaces.
pub fn next_value_key(map: &RholangPathMap, from_key: &[u8]) -> Option<Vec<u8>> {
    use pathmap::zipper::{Zipper, ZipperAbsolutePath, ZipperIteration, ZipperMoving};

    // Rooted at the trie ROOT: the walk must be able to ASCEND out of the
    // current branch to reach the next one, which a zipper rerooted by
    // `read_zipper_at_borrowed_path` cannot do (its ancestor stack is empty).
    // Rooting at the root also makes `origin_path()` report the ABSOLUTE key.
    let mut zipper = map.read_zipper();

    // Fast path: the whole `from_key` exists, which is the steady state of a
    // walk. One optimized descent, then the crate's own covered iteration.
    zipper.descend_to(from_key);
    if zipper.path_exists() {
        return match zipper.to_next_val() {
            true => Some(zipper.origin_path().to_vec()),
            false => None,
        };
    }

    // `from_key` diverges from the trie. Find WHERE, on a fresh zipper: the
    // failed descent above left the focus dangling, and every move from here
    // must start from an existing position.
    let mut zipper = map.read_zipper();
    let matched = zipper.descend_to_existing(from_key);
    debug_assert!(
        matched < from_key.len(),
        "the exact-match case returned above"
    );
    let mut floor = from_key[matched];
    loop {
        match zipper.child_mask().next_bit(floor) {
            Some(byte) => {
                // `byte` is a child of the focus, so this move stays on an
                // existing path and everything at or below it is > from_key.
                zipper.descend_to_byte(byte);
                if zipper.is_val() {
                    return Some(zipper.origin_path().to_vec());
                }
                return match zipper.to_next_val() {
                    true => Some(zipper.origin_path().to_vec()),
                    false => None,
                };
            }
            // Nothing above `floor` here: retry one level up, having excluded
            // everything at or below the byte we came from.
            None => match zipper.path().last().copied() {
                Some(byte) => {
                    zipper.ascend_byte();
                    floor = byte;
                }
                None => return None,
            },
        }
    }
}

/// The path of the next VALUE after `from_key` in depth-first order, as codec
/// segments — or `None` when `from_key` is at or past the last value.
///
/// This is the enumeration step, and it is [`next_value_key`] decoded: see
/// there for the specification, the algorithm, and why it does not use the
/// crate's `move_to_path` + `to_next_val` composition.
///
/// `to_next_val` advances to positions that carry a value, which (unlike a
/// byte- or child-segment move) are always complete, decodable keys, so the
/// caller can always both decode the path and read the value there.
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
/// ⚠ The trailing split-list terminator is not a segment, so the returned
/// vector is exactly the key's ELEMENTS — and a bare (non-list) entry and the
/// singleton list wrapping it therefore return the SAME vector. That is the
/// `EZipper.current_path` shape, and it is lossy; callers that must address
/// the entry again need [`next_value_key`]'s raw key.
pub fn next_value_path(map: &RholangPathMap, from_key: &[u8]) -> Option<Vec<Vec<u8>>> {
    next_value_key(map, from_key)
        .map(|key| super::pathmap_zipper::unflatten_segments(&key))
}
