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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::Par;
    use crate::rust::canonical_path::encode_trie_path;
    use crate::rust::pathmap_integration::{
        create_set_pathmap_from_elements, cursor_entry_key, entry_key_at, par_to_path,
        RholangMapPathMap,
    };
    use crate::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn list(elements: Vec<Par>) -> Par {
        new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
    }

    #[test]
    fn split_cursor_roundtrips_the_canonical_key() {
        let element = list(vec![gint(1), gint(2)]);
        let map = create_set_pathmap_from_elements(&[element.clone()], None).map;
        let key = encode_trie_path(&element);
        let (segments, kind) = decode_cursor(&key);
        assert_eq!(kind, CursorKind::Split);
        assert_eq!(segments, par_to_path(&element));
        assert_eq!(cursor_entry_key(&segments, kind, &map), key);
    }

    #[test]
    fn unflatten_segments_returns_exact_split_members() {
        let element = list(vec![gint(1), gint(2)]);
        let key = encode_trie_path(&element);
        assert_eq!(unflatten_segments(&key), par_to_path(&element));
    }

    #[test]
    fn unflatten_segments_handles_empty_input() {
        assert!(unflatten_segments(&[]).is_empty());
    }

    #[test]
    fn current_entry_key_finds_inserted_bare_value() {
        let elements = vec![gint(1), gint(2)];
        let map = create_set_pathmap_from_elements(&elements, None).map;
        let key = entry_key_at(&[], &gint(1), &map);
        assert_eq!(map.get(key), Some(&()));
    }

    #[test]
    fn current_entry_key_reports_missing_bare_value() {
        let map = create_set_pathmap_from_elements(&[gint(1)], None).map;
        assert_eq!(map.get(entry_key_at(&[], &gint(99), &map)), None);
    }

    #[test]
    fn decoded_bare_cursor_starts_at_existing_value() {
        let element = gint(7);
        let map = create_set_pathmap_from_elements(&[element.clone()], None).map;
        let key = encode_trie_path(&element);
        let (segments, kind) = decode_cursor(&key);
        assert_eq!(kind, CursorKind::Bare);
        assert_eq!(map.get(cursor_entry_key(&segments, kind, &map)), Some(&()));
    }

    #[test]
    fn created_pathmap_retains_entry_metadata() {
        let mut element = gint(1);
        element.connective_used = true;
        element.locally_free = vec![3];
        let created = create_set_pathmap_from_elements(&[element], None);
        assert!(created.connective_used);
        assert_eq!(created.locally_free, vec![3]);
    }

    #[test]
    fn canonical_key_sets_gets_and_removes_map_values() {
        let mut map = RholangMapPathMap::new();
        let key = encode_trie_path(&gint(7));
        assert_eq!(map.get(&key), None);
        assert_eq!(map.insert(&key, gint(70)), None);
        assert_eq!(map.get(&key), Some(&gint(70)));
        assert_eq!(map.insert(&key, gint(71)), Some(gint(70)));
        assert_eq!(map.remove(&key), Some(gint(71)));
        assert_eq!(map.get(&key), None);
    }

    #[test]
    fn a_second_cursor_lookup_starts_from_root() {
        let mut map = RholangMapPathMap::new();
        let key = encode_trie_path(&gint(1));
        map.insert(&key, gint(10));
        let (segments, kind) = decode_cursor(&key);
        assert_eq!(
            map.get(cursor_entry_key(&segments, kind, &map)),
            Some(&gint(10))
        );
    }

    #[test]
    fn insertion_at_canonical_path_is_readable_from_decoded_cursor() {
        let mut map = RholangMapPathMap::new();
        let key = encode_trie_path(&gint(5));
        map.insert(&key, gint(50));
        let (segments, kind) = decode_cursor(&key);
        assert_eq!(
            map.get(cursor_entry_key(&segments, kind, &map)),
            Some(&gint(50))
        );
    }

    #[test]
    fn removing_one_canonical_path_keeps_the_other() {
        let mut map = RholangMapPathMap::new();
        let first = encode_trie_path(&gint(1));
        let second = encode_trie_path(&gint(2));
        map.insert(&first, gint(10));
        map.insert(&second, gint(20));
        assert_eq!(map.remove(&first), Some(gint(10)));
        assert_eq!(map.get(&first), None);
        assert_eq!(map.get(&second), Some(&gint(20)));
    }

    #[test]
    fn composed_cursor_key_roundtrips_without_the_retired_terminator_rule() {
        let map = create_set_pathmap_from_elements(&[gint(1)], None).map;
        let cursor = par_to_path(&gint(9));
        let key = entry_key_at(&cursor, &gint(1), &map);
        let (segments, kind) = decode_cursor(&key);
        assert_eq!(cursor_entry_key(&segments, kind, &map), key);
        assert_ne!(key, encode_trie_path(&gint(1)));
    }

    #[test]
    fn native_set_join_unions_distinct_canonical_entries() {
        let source = create_set_pathmap_from_elements(&[gint(1)], None).map;
        let dest = create_set_pathmap_from_elements(&[gint(2)], None).map;
        let joined = dest.join(&source);
        assert_eq!(joined.get(encode_trie_path(&gint(1))), Some(&()));
        assert_eq!(joined.get(encode_trie_path(&gint(2))), Some(&()));
    }

    #[test]
    fn bare_and_singleton_list_entries_never_alias() {
        let bare = gint(5);
        let singleton = list(vec![gint(5)]);
        let map = create_set_pathmap_from_elements(&[bare.clone(), singleton.clone()], None).map;
        let bare_key = entry_key_at(&[], &bare, &map);
        let singleton_key = entry_key_at(&[], &singleton, &map);
        assert_ne!(bare_key, singleton_key);
        assert_eq!(decode_cursor(&bare_key).1, CursorKind::Bare);
        assert_eq!(decode_cursor(&singleton_key).1, CursorKind::Split);
        assert_eq!(map.get(bare_key), Some(&()));
        assert_eq!(map.get(singleton_key), Some(&()));
    }

    #[test]
    fn trailing_zero_in_bare_string_does_not_impersonate_split_terminator() {
        let bare = new_gstring_par("\0".to_string(), Vec::new(), false);
        let key = encode_trie_path(&bare);
        assert_eq!(key.last(), Some(&0));
        assert_eq!(decode_cursor(&key).1, CursorKind::Bare);
    }
}
