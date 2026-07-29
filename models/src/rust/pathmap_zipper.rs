//! Zipper wrapper types for integrating PathMap zippers with Rholang Par types.
//!
//! This module provides wrapper types that bridge PathMap's zipper API with Rholang's process-oriented
//! data model. Operations work on Par values as the unit of operation rather than raw bytes.
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
//! ★ A replacement cursor move is not forbidden — it is *re-typed*. It must
//! take `(cursor_segments, path_par, map)` and call `entry_key_at`, which is a
//! DIFFERENT signature from the deleted `(&mut self, path: &Par)`. The
//! deletion therefore corrects the capability rather than removing it: a
//! caller cannot re-acquire the old answer by re-adding the old name.
//!
//! `RholangWriteZipper` and `RholangZipperHead` went with them. Both had
//! zero users of any method, and both existed only to host the deleted move;
//! keeping constructors that no code calls, for a type whose only positioning
//! API was the retired rule, is how the rule survived its own refutation the
//! first time.

use pathmap::zipper::ReadZipperUntracked;

use super::pathmap_integration::{entry_key_at, CursorKind, RholangPathMap};
use crate::rhoapi::{EPathMap, Par};

/// Wrapper for PathMap ReadZipper that maintains Rholang context
pub struct RholangReadZipper<'a, 'path> {
    pub(crate) zipper: ReadZipperUntracked<'a, 'path, Par>,
    pub(crate) connective_used: bool,
    pub(crate) locally_free: Vec<u8>,
}

impl<'a, 'path> RholangReadZipper<'a, 'path> {
    /// Create a new read zipper from a PathMap at root
    pub fn new(map: &'a RholangPathMap, connective_used: bool, locally_free: Vec<u8>) -> Self {
        RholangReadZipper {
            zipper: map.read_zipper(),
            connective_used,
            locally_free,
        }
    }

    /// Create a new read zipper at a specific path
    ///
    /// `path` is the WHOLE path, so its key is the codec's own
    /// [`entry_key_at`] at the root — bit for bit the key
    /// `create_pathmap_from_elements` inserted the entry under, whichever arm
    /// it took. Reconstructing it from the path's per-element segments and
    /// appending the split terminator unconditionally is the retired rule this
    /// module's documentation names; for a bare (non-list) path it addresses
    /// the SINGLETON LIST instead.
    ///
    /// ★ **Retained deliberately with no caller.** It is this module's only
    /// exemplar of the corrected law, and it is the sanctioned replacement for
    /// the deleted `descend_to` at the root. Deleting it would leave the module
    /// with no positioning API at all — which is the state in which the retired
    /// rule was written the first time. Being `pub` in a library crate, it
    /// costs no `dead_code` diagnostic to keep.
    pub fn new_at_path(
        map: &'a RholangPathMap,
        path: &Par,
        connective_used: bool,
        locally_free: Vec<u8>,
    ) -> Result<RholangReadZipper<'a, 'static>, String> {
        let key = entry_key_at(&[], path, map);
        // Use the owned version since we can't return a reference to local key
        Ok(RholangReadZipper {
            zipper: map.read_zipper_at_path(key),
            connective_used,
            locally_free,
        })
    }

    /// Get the value at the current position
    pub fn get_val(&self) -> Option<&Par> {
        use pathmap::zipper::ZipperValues;
        self.zipper.val()
    }

    /// Check if there's a value at current position
    pub fn has_val(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.is_val()
    }

    /// Check if the current path exists
    pub fn path_exists(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.path_exists()
    }

    /// Convert zipper to Par representation
    /// This creates a special Par that represents the zipper state
    pub fn to_par(&self) -> Par {
        // For now, we'll represent the zipper as a special PathMap
        // In a full implementation, we'd need a custom Expr type for zippers
        // We'll create an empty PathMap as a placeholder since we can't easily
        // extract the underlying PathMap from the zipper
        // EPathMap fix P3 (PM-2): constructor instead of a struct literal
        // (the wrapper's shadow cell is private).
        let empty_pathmap = EPathMap::new(
            vec![],
            self.locally_free.clone(),
            self.connective_used,
            None,
        );

        // Create a special Par that represents a read zipper
        // We'll use a special marker to identify it as a zipper
        Par::default().with_exprs(vec![crate::rhoapi::Expr {
            expr_instance: Some(crate::rhoapi::expr::ExprInstance::EPathmapBody(
                empty_pathmap,
            )),
        }])
    }
}

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
pub(crate) fn unflatten_segments(flattened: &[u8]) -> Vec<Vec<u8>> {
    decode_cursor(flattened).0
}

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
