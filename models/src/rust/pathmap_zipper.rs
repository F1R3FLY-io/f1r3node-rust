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
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::pathmap_integration::create_pathmap_from_elements;
    use crate::rust::utils::new_gint_par;

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    #[test]
    fn flatten_segments_appends_separator_per_segment() {
        let segments = vec![vec![1u8, 2, 3], vec![4, 5]];
        assert_eq!(flatten_segments(&segments), vec![1, 2, 3, 0xFF, 4, 5, 0xFF]);
    }

    #[test]
    fn unflatten_segments_inverts_flatten() {
        let segments = vec![vec![1u8, 2, 3], vec![4, 5]];
        assert_eq!(unflatten_segments(&flatten_segments(&segments)), segments);
    }

    #[test]
    fn unflatten_segments_drops_empty_segments() {
        assert_eq!(
            unflatten_segments(&[0xFF, 1, 0xFF, 0xFF, 2, 3, 0xFF]),
            vec![vec![1u8], vec![2, 3]]
        );
        assert!(unflatten_segments(&[]).is_empty());
    }

    #[test]
    fn read_zipper_finds_inserted_value_by_par_path() {
        let elements = vec![gint(1), gint(2)];
        let created = create_pathmap_from_elements(&elements, None);

        let mut zipper = RholangReadZipper::new(&created.map, false, Vec::new());
        assert!(!zipper.has_val());
        zipper.descend_to(&gint(1)).unwrap();
        assert!(zipper.path_exists());
        assert!(zipper.has_val());
        assert_eq!(zipper.get_val(), Some(&gint(1)));
    }

    #[test]
    fn read_zipper_reports_missing_path() {
        let created = create_pathmap_from_elements(&[gint(1)], None);
        let mut zipper = RholangReadZipper::new(&created.map, false, Vec::new());
        zipper.descend_to(&gint(99)).unwrap();
        assert!(!zipper.has_val());
        assert_eq!(zipper.get_val(), None);
    }

    #[test]
    fn read_zipper_new_at_path_starts_at_value() {
        let created = create_pathmap_from_elements(&[gint(7)], None);
        let zipper = RholangReadZipper::new_at_path(&created.map, &gint(7), true, vec![1]).unwrap();
        assert!(zipper.has_val());
        assert_eq!(zipper.get_val(), Some(&gint(7)));
    }

    #[test]
    fn read_zipper_to_par_carries_metadata_into_pathmap_expr() {
        let created = create_pathmap_from_elements(&[gint(1)], None);
        let zipper = RholangReadZipper::new(&created.map, true, vec![3]);
        let par = zipper.to_par();
        assert_eq!(par.exprs.len(), 1);
        match &par.exprs[0].expr_instance {
            Some(ExprInstance::EPathmapBody(pathmap)) => {
                assert!(pathmap.ps.is_empty());
                assert!(pathmap.connective_used);
                assert_eq!(pathmap.locally_free, vec![3]);
            }
            other => panic!("expected EPathmapBody, got {:?}", other),
        }
    }

    #[test]
    fn write_zipper_sets_gets_and_removes_values() {
        let mut map = RholangPathMap::new();
        let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
        zipper.descend_to(&gint(7)).unwrap();
        assert!(!zipper.has_val());
        assert_eq!(zipper.set_val(gint(70)), None);
        assert!(zipper.has_val());
        assert!(zipper.path_exists());
        assert_eq!(zipper.get_val(), Some(&gint(70)));
        assert_eq!(zipper.set_val(gint(71)), Some(gint(70)));
        assert_eq!(zipper.remove_val(), Some(gint(71)));
        assert!(!zipper.has_val());
    }

    #[test]
    fn write_zipper_reset_returns_to_root() {
        let mut map = RholangPathMap::new();
        let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
        zipper.descend_to(&gint(1)).unwrap();
        zipper.set_val(gint(10));
        zipper.reset();
        assert!(!zipper.has_val());
        zipper.descend_to(&gint(1)).unwrap();
        assert_eq!(zipper.get_val(), Some(&gint(10)));
    }

    #[test]
    fn write_zipper_new_at_path_lands_on_requested_path() {
        let mut map = RholangPathMap::new();
        {
            let mut zipper =
                RholangWriteZipper::new_at_path(&mut map, &gint(5), false, Vec::new()).unwrap();
            zipper.set_val(gint(50));
        }
        let mut reader = RholangReadZipper::new(&map, false, Vec::new());
        reader.descend_to(&gint(5)).unwrap();
        assert_eq!(reader.get_val(), Some(&gint(50)));
    }

    #[test]
    fn write_zipper_remove_branches_clears_subtrie() {
        let mut map = RholangPathMap::new();
        {
            let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
            zipper.descend_to(&gint(1)).unwrap();
            zipper.set_val(gint(10));
            zipper.reset();
            zipper.descend_to(&gint(2)).unwrap();
            zipper.set_val(gint(20));
            zipper.reset();
            zipper.remove_branches();
        }
        let mut reader = RholangReadZipper::new(&map, false, Vec::new());
        reader.descend_to(&gint(1)).unwrap();
        assert!(!reader.has_val());
    }

    #[test]
    fn graft_copies_source_subtrie_under_current_position() {
        let source = create_pathmap_from_elements(&[gint(1)], None);
        let source_zipper = RholangReadZipper::new(&source.map, false, Vec::new());

        let mut dest = RholangPathMap::new();
        {
            let mut writer = RholangWriteZipper::new(&mut dest, false, Vec::new());
            writer.descend_to(&gint(9)).unwrap();
            writer.graft(&source_zipper);
        }

        let mut reader = RholangReadZipper::new(&dest, false, Vec::new());
        reader.descend_to(&gint(9)).unwrap();
        reader.descend_to(&gint(1)).unwrap();
        assert_eq!(reader.get_val(), Some(&gint(1)));
    }

    #[test]
    fn join_into_unions_source_with_existing_entries() {
        let source = create_pathmap_from_elements(&[gint(1)], None);
        let source_zipper = RholangReadZipper::new(&source.map, false, Vec::new());

        let dest_created = create_pathmap_from_elements(&[gint(2)], None);
        let mut dest = dest_created.map;
        {
            let mut writer = RholangWriteZipper::new(&mut dest, false, Vec::new());
            writer.join_into(&source_zipper);
        }

        let mut reader = RholangReadZipper::new(&dest, false, Vec::new());
        reader.descend_to(&gint(1)).unwrap();
        assert!(reader.has_val());
        let mut reader2 = RholangReadZipper::new(&dest, false, Vec::new());
        reader2.descend_to(&gint(2)).unwrap();
        assert!(reader2.has_val());
    }

    #[test]
    fn zipper_head_construction_keeps_metadata() {
        let mut map = RholangPathMap::new();
        let head = RholangZipperHead::new(&mut map, true, vec![7]);
        assert!(head.connective_used);
        assert_eq!(head.locally_free, vec![7]);
    }
}
