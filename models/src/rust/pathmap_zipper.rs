//! Lossless conversion between canonical PathMap byte keys and Rholang zipper
//! cursors.
//!
//! # ★ WHAT IS DELIBERATELY ABSENT, AND WHY IT CANNOT BE WRITTEN HERE
//!
//! This module used to export a `descend_to(&Par)` on both the read and the
//! write wrapper, and a `flatten_segments` helper they shared. All three
//! encoded **one retired rule**:
//!
//! > a descended path's entry key is the SPLIT arm — `concat(segments)`
//! > followed by the `0x00` terminator — *unconditionally*.
//!
//! That rule is FALSE, and not merely imprecise. The canonical path codec has
//! two top-level arms, so appending the terminator to a bare (non-list) `p`
//! yields the key of the SINGLETON LIST `[p]` — a well-formed canonical key
//! naming a DIFFERENT element, which one map may hold at the same time. The
//! read is not a miss; it is a wrong answer. Measured on `{| 5, [5] |}`:
//! `descend_to(5)` built `03 0a 00`, the key of `[5]`, and returned `[5]`,
//! where the entry `5` lives at `03 0a`.
//!
//! ⚠ The read-side image invariant in
//! [`cursor_entry_key`](super::pathmap_integration::cursor_entry_key) would
//! NOT have caught it either: a terminated concatenation is always in the
//! codec's image, so the assertion is vacuous on exactly this rule's output.
//! The defect is in the LAW, not in the well-formedness of the bytes, which is
//! why the fix is a deletion and not a guard.
//!
//! The law that replaces it is
//! [`entry_key_at`](super::pathmap_integration::entry_key_at) /
//! [`composed_cursor_kind`](super::pathmap_integration::composed_cursor_kind) /
//! [`cursor_entry_key`](super::pathmap_integration::cursor_entry_key), spelled
//! once. `flatten_segments` was deleted along with its two callers **so that
//! the retired rule is unspellable in this module**: repairing the callers
//! would have left the expression alive for the next author to reach for.
//! `models/tests/pathmap_integration_tests.rs`'s
//! `the_retired_unconditional_terminate_rule_is_unspellable` pins that.
//!
//! The former wrapper objects were removed: their unused `to_par` method
//! returned an empty EPathMap placeholder rather than the represented cursor.
//! Runtime operations retain the real `EPathMap` plus `(segments,
//! CursorKind)` and dispatch directly to its set/map-specialized PathMap APIs.

use super::pathmap_integration::CursorKind;

/// Split a codec trie key back into its per-element segments (W2b-1): the
/// parser-state successor to the retired `split(0xFF)`. Segment boundaries
/// are the codec grammar's `segment_extent`, and the trailing split-list
/// `0x00` terminator is not a segment.
///
/// LIVE since the trie-enumeration surface landed: this is the segments-only
/// half of [`decode_cursor`], which
/// [`pathmap_native_query::next_value_path`](super::pathmap_native_query::next_value_path)
/// needs to hand a walked absolute path back to `EZipper.current_path`, which
/// stores segments. (It carried `#[allow(dead_code)]` until then — the decoder
/// existed but nothing had yet needed to read a path OUT of the trie.)
///
/// ⚠ Its former forward half, `flatten_segments`, is DELETED: it was the
/// retired unconditional-terminate rule, verbatim. The forward direction is
/// [`cursor_entry_key`](super::pathmap_integration::cursor_entry_key), which
/// spends the split/bare discriminator this function recovers.
pub(crate) fn unflatten_segments(flattened: &[u8]) -> Vec<Vec<u8>> { decode_cursor(flattened).0 }

/// Split a codec trie key back into the CURSOR that names it: the per-element
/// segments AND the split/bare discriminator.
///
/// This is the exact inverse of
/// [`cursor_entry_key`](super::pathmap_integration::cursor_entry_key) — for
/// every key `k` produced by `encode_trie_path`,
///
/// ```text
/// let (segments, kind) = decode_cursor(&k);
/// cursor_entry_key(&segments, kind, map) == k
/// ```
///
/// and `kind` is never [`CursorKind::Prefix`]: a key that carries a value
/// names a real entry, and this function is only ever handed such a key (by
/// the enumeration step). The discriminator is read from the PARSE, not from
/// the last byte — a `0x00` can legitimately end a bare segment's payload
/// (`GString("\0")` is `04 01 00`), so testing `k.last() == Some(&TERM)` would
/// misclassify it.
pub fn decode_cursor(flattened: &[u8]) -> (Vec<Vec<u8>>, CursorKind) {
    use super::canonical_path::{segment_extent, tag};
    let mut segments = Vec::new();
    let mut rest = flattened;
    while let Some(&first) = rest.first() {
        // The split-list terminator ends the path and is not a segment.
        if first == tag::TERM {
            // …and it is exactly what makes this a SPLIT frame. Anything after
            // it is not part of a well-formed path, so this is the end either
            // way; `rest.len() == 1` is the well-formed case.
            return (segments, CursorKind::Split);
        }
        match segment_extent(rest) {
            Some(extent) if extent > 0 => {
                segments.push(rest[..extent].to_vec());
                rest = &rest[extent..];
            }
            // Not a well-formed codec segment boundary — stop.
            _ => break,
        }
    }
    // No terminator at a segment boundary: the key IS the concatenation, which
    // is the bare arm. (The empty key also lands here; it names no entry, and
    // no reader builds one.)
    (segments, CursorKind::Bare)
}
