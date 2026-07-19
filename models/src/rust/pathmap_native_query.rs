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
//! trie is a byte-prefix of some inserted key. Keys are 0xFF-terminated
//! segment concatenations (`seg₀ ∥ 0xFF ∥ seg₁ ∥ 0xFF ∥ …`), which makes any
//! prefix built by the same flattening land on segment boundaries.
//!
//! Ordering: `PathMap::iter()` is the trie's depth-first traversal —
//! `descend_first_byte` takes the smallest child byte and
//! `to_next_sibling_byte` advances via `ByteMask::next_bit` (ascending) — so
//! keys stream in ascending byte-lexicographic order with a node's value
//! emitted before its descendants. The helpers preserve exactly the orders the
//! scans produced (see each helper's proof sketch).

use pathmap::utils::{BitMask, ByteMask, ByteMaskIter};
use pathmap::zipper::{Zipper, ZipperMoving};

use super::pathmap_integration::RholangPathMap;
use crate::rhoapi::Par;

/// The byte that terminates every path segment in a Rholang PathMap key.
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
/// A "child segment" is the byte run strictly between `prefix` and the next
/// `0xFF` separator on some key, i.e. exactly what the retired scans
/// collected:
///
/// ```text
/// for (key, _) in map.iter() {
///     if key.starts_with(prefix) && key.len() > prefix.len() {
///         let remaining = &key[prefix.len()..];
///         if let Some(pos) = remaining.iter().position(|&b| b == 0xFF) {
///             children.push(remaining[..pos].to_vec());
///         }
///     }
/// }
/// children.sort();
/// children.dedup();
/// ```
///
/// Identity with `sort(); dedup()` of that scan:
///
/// * **Membership (bijection).** The scan records segment `s` iff some
///   inserted key has the byte-prefix `prefix ∥ s ∥ 0xFF` (every inserted key
///   is 0xFF-terminated, so `remaining` of any strictly-longer matching key
///   contains a separator, and the first separator delimits `s`). In a
///   pure-insert trie that is equivalent to the PATH `prefix ∥ s ∥ 0xFF`
///   existing — precisely when this DFS, which never walks past a separator,
///   reaches the node for `s` and finds `0xFF` in its child mask.
/// * **Distinctness.** Distinct segments are distinct trie paths; a DFS visits
///   each path once, so no duplicates are emitted (`dedup()` had removed the
///   scan's per-key repeats).
/// * **Order.** Raw byte-lexicographic order on segments is the trie order in
///   which "end of segment" sorts BEFORE every extension byte (`"ab"` before
///   `"abc"`). Visiting the `0xFF` separator branch FIRST at every node and
///   the remaining child bytes in ascending order realizes exactly that order
///   (the standard end-marker-first radix traversal), which is what `sort()`
///   produced. Note the separator's numeric value (0xFF, the LARGEST byte)
///   makes the plain ascending traversal wrong here — the separator visit
///   must be special-cased first, and is.
///
/// The early `limit` stop is valid because emission is already in the final
/// order. Cost: O(|prefix| + Σ|emitted distinct segments|) instead of O(map).
pub fn collect_child_segments(
    map: &RholangPathMap,
    prefix: &[u8],
    limit: Option<usize>,
) -> Vec<Vec<u8>> {
    let mut children: Vec<Vec<u8>> = Vec::new();
    if limit == Some(0) {
        return children;
    }

    let mut zipper = map.read_zipper_at_borrowed_path(prefix);
    // `child_mask` is EMPTY on a leaf or nonexistent focus, so a nonexistent
    // or childless `prefix` falls straight through with no emissions.
    let root_mask = zipper.child_mask();
    if root_mask.test_bit(SEGMENT_SEPARATOR) {
        // An empty segment (`prefix ∥ 0xFF ∥ …` key) sorts before everything.
        children.push(Vec::new());
        if limit == Some(children.len()) {
            return children;
        }
    }

    // Iterative DFS: `masks[d]` iterates the REMAINING unvisited non-separator
    // child bytes of the node at relative depth d; `segment` holds the bytes
    // descended so far (never contains the separator). The zipper cannot
    // ascend above its root (`prefix`), so traversal is contained by
    // construction.
    let mut masks: Vec<ByteMaskIter> = vec![non_separator_iter(root_mask)];
    let mut segment: Vec<u8> = Vec::new();
    while let Some(mask_iter) = masks.last_mut() {
        match mask_iter.next() {
            Some(byte) => {
                debug_assert_ne!(byte, SEGMENT_SEPARATOR);
                zipper.descend_to_byte(byte);
                segment.push(byte);
                let mask = zipper.child_mask();
                if mask.test_bit(SEGMENT_SEPARATOR) {
                    children.push(segment.clone());
                    if limit == Some(children.len()) {
                        return children;
                    }
                }
                masks.push(non_separator_iter(mask));
            }
            None => {
                masks.pop();
                if segment.pop().is_some() {
                    zipper.ascend_byte();
                }
            }
        }
    }

    children
}

/// A `ByteMask` iterator over every child byte EXCEPT the segment separator
/// (which the DFS handles out-of-band, first).
///
/// NOTE: pathmap 0.2.2's `BitMask::clear_bit` is implemented as XOR — a
/// TOGGLE, not a clear — so clearing an unset separator bit would SET it and
/// send the DFS down a phantom branch. The separator is therefore masked out
/// with an AND against a constant that has every bit set except bit 255
/// (`SEGMENT_SEPARATOR` = 0xFF = word 3, bit 63).
fn non_separator_iter(mask: ByteMask) -> ByteMaskIter {
    const _: () = assert!(SEGMENT_SEPARATOR == 0xFF, "mask constant assumes separator 0xFF");
    const ALL_BUT_SEPARATOR: ByteMask = ByteMask([!0u64, !0u64, !0u64, !0u64 >> 1]);
    mask.and(&ALL_BUT_SEPARATOR).iter()
}
