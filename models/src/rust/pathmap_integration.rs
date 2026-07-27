//! Integration layer between Rholang Par types and the PathMap crate.
//!
//! EPathMap wire W2b-1 (the global trie re-key): trie keys are now the
//! CANONICAL PATH CODEC bytes (`canonical_path::encode_trie_path`), not the
//! former `ParToSExpr` + `SExpr::encode` segments joined with a `0xFF`
//! separator. The codec is capless, injective, and prefix-free, with the
//! `0x0F` escape arm for non-`eval_stable` trie entries (R3F-2) and a depth
//! limit of 32 collection levels (R3F-7). Consequences of the flip
//! (design §3.1-§3.2): trie keys re-key globally, formerly-colliding paths
//! (`"(expr)"`/`"Nil"` degenerate keys) SEPARATE (a D-2 disclosed fix), and
//! the `0xFF`-separator machinery (this file's flatten, the
//! `pathmap_native_query` separator special-case, `SExpr::decode`
//! reconstruction) retires.
//!
//! `par_to_path` now returns the PER-ELEMENT codec segments (each element's
//! `encode_trie_segment`); full trie keys are built with [`segments_to_key`]
//! (concatenation + the split-list `0x00` terminator) or, for a whole entry
//! Par, directly with `canonical_path::encode_trie_path`. Callers that store
//! `current_path` segments are unaffected by the return type; only the
//! key-BUILD sites (the former `0xFF` flatten) change to the codec
//! concatenation.

use pathmap::PathMap;

use crate::rhoapi::{Par, Var};
use crate::rust::canonical_path::{
    encode_trie_path, encode_trie_segment, split_carrier_list, tag, takes_split_arm,
};

/// Type alias for our standard use case: PathMap from bytes to Rholang Par.
pub type RholangPathMap = PathMap<Par>;

/// WHICH ENTRY an `EZipper` cursor addresses — the split/bare discriminator
/// that `current_path` alone cannot carry.
///
/// The canonical path codec has TWO top-level arms (`canonical_path.rs` §1.3):
/// a ground list splits into per-element segments plus a `0x00` terminator,
/// and anything else is one bare segment with NO terminator. `current_path`
/// stores the per-element segments of either arm, so the bare element `1` and
/// the singleton list `[1]` give the SAME segment vector — while being
/// DIFFERENT entries, under keys `03 02` and `03 02 00`, which one map may
/// hold at the same time. The cursor is `(segments, kind)`, and
/// [`cursor_entry_key`] is the key it names:
///
/// ```text
/// key(cursor) = concat(segments) ++ (0x00 iff the cursor is a split frame)
/// ```
///
/// This mirrors `EZipper.cursor_kind` (`RhoTypes.proto`), whose wire values are
/// pinned by [`CursorKind::from_wire`] / [`CursorKind::to_wire`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum CursorKind {
    /// A SPLIT path frame of `segments.len()` elements: the key is
    /// `concat(segments) ++ 0x00`.
    ///
    /// The root cursor is this with zero segments — key `0x00`, the empty
    /// list — and so is the cursor of `readZipperAt`/`descendTo` given a
    /// ground-LIST argument. It is the DEFAULT because it is proto value 0,
    /// so an `EZipper` serialized before `cursor_kind` existed decodes to
    /// exactly the behaviour that preceded it.
    #[default]
    Split,
    /// A BARE element: the key is `concat(segments)`, with no terminator.
    /// From `readZipperAt`/`descendTo` given a non-list argument, and from an
    /// enumeration step that landed on a bare entry.
    Bare,
    /// An element-PREFIX at which the cursor has NOT chosen between the bare
    /// entry (`concat(segments)`) and the split entry
    /// (`concat(segments) ++ 0x00`).
    ///
    /// Produced by every move that advances by one CHILD SEGMENT
    /// (`descendFirst`, `descendIndexedBranch`, `toNextSibling`,
    /// `toPrevSibling`, `ascendOne`, `ascend`): a segment move lands on an
    /// element boundary and carries no information about which arm the entry
    /// there took. [`cursor_entry_key`] resolves it to the SHORTEST key
    /// PRESENT in the map — bare first, split otherwise — which is
    /// indistinguishable from [`CursorKind::Split`] on any map holding no bare
    /// entries, and is why navigation does not move for the ground-list corpus.
    Prefix,
}

impl CursorKind {
    /// Decode the `EZipper.cursor_kind` wire value.
    ///
    /// Returns `None` for an unrecognized value rather than coercing it: a
    /// cursor whose kind cannot be read names no entry, and silently treating
    /// it as `Split` is precisely the guess this type exists to remove.
    pub fn from_wire(value: u32) -> Option<CursorKind> {
        match value {
            0 => Some(CursorKind::Split),
            1 => Some(CursorKind::Bare),
            2 => Some(CursorKind::Prefix),
            _ => None,
        }
    }

    /// The `EZipper.cursor_kind` wire value.
    pub fn to_wire(self) -> u32 {
        match self {
            CursorKind::Split => 0,
            CursorKind::Bare => 1,
            CursorKind::Prefix => 2,
        }
    }

    /// The kind of a cursor built from the WHOLE path `par` — the arm the
    /// codec itself takes for it, so that
    /// `cursor_entry_key(par_to_path(par), CursorKind::of(par), map)` is
    /// `encode_trie_path(par)` for EVERY Par (the cursor round-trip law).
    pub fn of(par: &Par) -> CursorKind {
        match takes_split_arm(par) {
            true => CursorKind::Split,
            false => CursorKind::Bare,
        }
    }
}

/// The per-element codec SEGMENTS of a path Par (W2b-1).
///
/// - A split-arm entry (a ground `EList` carrier, [`takes_split_arm`]) yields
///   one segment per element (`encode_trie_segment` of each), so a
///   `current_path` of these segments keeps the segment-count ==
///   element-count invariant the zipper relies on
///   (`existing_list.ps[..current_path.len()]` indexing, `starts_with`
///   prefix matching).
/// - Any other Par yields a single segment (`encode_trie_segment(par)`).
///
/// The FULL trie key of the entry is [`cursor_entry_key`] over these segments
/// at [`CursorKind::of`] the Par — equivalently `encode_trie_path` of it.
///
/// ⚠ The split test is [`takes_split_arm`], i.e. the codec's OWN
/// `split_carrier_list`, and not a local re-derivation. An earlier local test
/// inspected only `exprs` and the `EList`'s own metadata, so a list carrier
/// that ALSO carried (say) a send split here into per-element segments while
/// the codec gave it the `0x0F` escape arm — under which the cursor's segments
/// do not concatenate to the key under ANY kind, and the round-trip law cannot
/// hold. Pinned by
/// `canonical_path::tests::par_to_path_agrees_with_the_codec_about_which_pars_split`.
pub fn par_to_path(par: &Par) -> Vec<Vec<u8>> {
    match split_carrier_list(par) {
        Some(list) => list.ps.iter().map(encode_trie_segment).collect(),
        None => vec![encode_trie_segment(par)],
    }
}

/// The ENTRY key a cursor `(segments, kind)` addresses in `map`.
///
/// This is THE cursor→key rule, and the only place the split/bare
/// discriminator is spent. `map` is consulted for [`CursorKind::Prefix`] only.
///
/// ```text
/// Split   concat(segments) ++ 0x00
/// Bare    concat(segments)
/// Prefix  concat(segments)          if the map holds a value there
///         concat(segments) ++ 0x00  otherwise
/// ```
///
/// PREFIX queries — `leafCount`, `childCount`, `getSubtrie`, `pathExists`,
/// `descendFirst`, `restriction`, `prunePath`, `removeBranches` — do NOT use
/// this. They ask about the BRANCH at the cursor, which is
/// `segments_to_key(segments, false)` under every kind, so they are unaffected
/// by the discriminator and their bytes never move. See `pathmap_native_query`
/// for the branch-vs-entry semantics that follow from this split.
pub fn cursor_entry_key(
    segments: &[Vec<u8>],
    kind: CursorKind,
    map: &RholangPathMap,
) -> Vec<u8> {
    let bare = segments_to_key(segments, false);
    match kind {
        CursorKind::Bare => bare,
        CursorKind::Split => {
            let mut split = bare;
            split.push(tag::TERM);
            split
        }
        // Shortest key PRESENT. A proper prefix sorts before what it prefixes,
        // so "shortest present" is also "first in trie order" — the same tie
        // -break the enumeration walk would make.
        CursorKind::Prefix => match map.get(&bare) {
            Some(_) => bare,
            None => {
                let mut split = bare;
                split.push(tag::TERM);
                split
            }
        },
    }
}

/// Build a trie key from per-element codec segments (W2b-1): concatenation,
/// with the split-list terminator (`0x00`) appended iff `terminate`.
///
/// - A FULL entry key (getLeaf/setLeaf/removeLeaf exact addressing) passes
///   `terminate = true` — matching `encode_trie_path` of the list path.
/// - A PREFIX key (getSubtrie/childCount/descend/pathExists) passes
///   `terminate = false`; codec segments are prefix-free, so the raw
///   concatenation is a clean trie prefix boundary.
pub fn segments_to_key(segments: &[Vec<u8>], terminate: bool) -> Vec<u8> {
    let total: usize = segments.iter().map(|s| s.len()).sum::<usize>() + terminate as usize;
    let mut key = Vec::with_capacity(total);
    for segment in segments {
        key.extend_from_slice(segment);
    }
    if terminate {
        key.push(tag::TERM);
    }
    key
}

/// The trie key of the ENTRY named by `path_par`, as reached from a cursor
/// whose per-element segments are `cursor`.
///
/// # Why this exists (the bare-element key defect, read side)
///
/// `segments_to_key(par_to_path(p), true)` appends the split-list terminator
/// UNCONDITIONALLY, so for a bare (non-list) `p` it produces
/// `encode_trie_path([p])` — the key of the SINGLETON LIST, a valid canonical
/// key naming a DIFFERENT element. The read is not a miss; it is a wrong
/// answer. (Pinned by `canonical_path::tests::bare_and_singleton_list_are_distinct_entries`.)
///
/// The information needed to avoid the guess is available whenever the caller
/// holds the whole path Par, and this function is the ONE place that spends it.
///
/// * **At the root** (`cursor` empty) the argument IS the whole path, so the
///   key is [`crate::rust::canonical_path::encode_trie_path`] of it — bit for
///   bit the key `create_pathmap_from_elements` inserted the entry under,
///   split arm or bare arm or `0x0F` escape arm, with NO reconstruction. For a
///   split-form Par this is byte-identical to the old expression (the two
///   agree exactly on the split arm), so the ground-LIST corpus does not move.
///
/// * **Below the root** the argument is a RELATIVE descent: its elements
///   extend the cursor's, and the composed cursor's kind is the ARGUMENT's arm
///   ([`CursorKind::of`]) — descending by `["x"]` lands on a split frame,
///   descending by `1` lands on a bare element. That composition is what makes
///   `readZipperAt(p)` and `readZipper().descendTo(p)` name the same entry.
pub fn entry_key_at(cursor: &[Vec<u8>], path_par: &Par, map: &RholangPathMap) -> Vec<u8> {
    match cursor.is_empty() {
        // ZERO AMBIGUITY: the reader has the Par, so it can ask the codec.
        true => encode_trie_path(path_par),
        false => {
            let relative = par_to_path(path_par);
            let mut segments = Vec::with_capacity(cursor.len() + relative.len());
            segments.extend_from_slice(cursor);
            segments.extend(relative);
            cursor_entry_key(&segments, CursorKind::of(path_par), map)
        }
    }
}

/// Convenience return type—including the constructed map and related Rholang metadata.
pub struct PathMapCreationResult {
    pub map: RholangPathMap,
    pub connective_used: bool,
    pub locally_free: Vec<u8>,
}

/// Construct a RholangPathMap from a list of Par elements and an optional remainder.
/// This mirrors what the normalizer does when producing EPathMap from parsed elements.
///
/// W2b-1: each entry's key is `canonical_path::encode_trie_path(par)` — the
/// capless, injective, prefix-free codec path (split form for ground lists,
/// bare form otherwise, `0x0F` escape for non-`eval_stable` entries). This
/// replaces the former `par_to_path` + `0xFF` flatten; formerly-colliding
/// degenerate keys now separate.
pub fn create_pathmap_from_elements(
    elements: &[Par],
    remainder: Option<Var>,
) -> PathMapCreationResult {
    let mut map = RholangPathMap::new();
    let mut connective_used = false;
    let mut locally_free = Vec::new();

    for par in elements {
        // Update connective metadata
        if par.connective_used {
            connective_used = true;
        }
        locally_free = crate::rust::utils::union(locally_free.clone(), par.locally_free.clone());

        // The codec trie key (capless, injective, prefix-free). No separator
        // — the path is self-delimiting.
        let key = encode_trie_path(par);
        map.insert(key, par.clone());
    }

    if remainder.is_some() {
        connective_used = true;
    }

    PathMapCreationResult {
        map,
        connective_used,
        locally_free,
    }
}
