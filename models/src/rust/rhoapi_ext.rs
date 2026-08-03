//! PathMap-native implementation of the externally generated `EPathMap` type.
//!
//! `models/build.rs` maps `.rhoapi.EPathMap` to this module, so generated
//! expression and zipper fields resolve to one representation throughout the
//! program. Entry storage is homogeneous and prefix-compressed:
//!
//! - `Empty` is mode-neutral;
//! - set membership uses `PathMap<()>`;
//! - key/value membership uses `PathMap<Par>`.
//!
//! The first insertion selects set or map mode, mixed membership is rejected,
//! and deleting the final member restores neutral empty. Canonical path bytes
//! are the only keys; insertion order, duplicates, and retained `Vec<Par>`
//! projections are not representable.
//!
//! Bincode and protobuf serialize the same versioned EPM1 snapshot: PathMap's
//! compact ACTree03 arena followed by a generated, stack-safe protobuf value
//! table in map mode. `trie_snapshot` caches the completed byte string across a
//! clone family; cold construction is linear in trie nodes plus encoded values,
//! while warm access is O(1) before the caller copies the slice. `epm_layout`
//! caches only the value-independent topology prefix so nested map values can be
//! streamed without retaining every suffix of a nested chain.
//!
//! Equality, hashing, ordering, lookup, algebraic operations, and zipper walks
//! operate on PathMap directly and do not force either serialization cache.
//! Mutation installs fresh cache cells on the modified value. Set members are
//! decoded to `Par` only at explicit compatibility boundaries; map keys remain
//! canonical bytes and map values remain borrowed `Par`s on PDA worklists.

use std::cmp::Ordering;
use std::fmt;
#[cfg(test)]
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::DecodeError;

use super::canonical_path::{decode_trie_path, encode_trie_path, encode_trie_path_with_stability};
use super::epathmap_trie_codec::{self, EPathMapMode, EPathMapRepr};
use super::pathmap_crate_type_mapper::{eval_stable_par, PathFrameError, PathFrames};
use super::pathmap_integration::{
    cursor_entry_key as encode_cursor_entry_key, entry_key_at as encode_entry_key_at, par_to_path,
    segments_to_key, CursorKind, RholangMapPathMap, RholangSetPathMap,
};
use super::pathmap_native_query::{
    collect_child_segments, next_value_key, path_prefix_exists, subtrie_value_count,
};
use super::pathmap_zipper::decode_cursor;
use crate::rhoapi::{Par, Var};

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE ENTRY TRIE — the stored form of an `EPathMap`'s entries
// ─────────────────────────────────────────────────────────────────────────────

/// ★ **The entries of an `EPathMap`, stored as the trie they always were.**
///
/// Set mode stores canonical paths in `PathMap<()>`; map mode stores canonical
/// paths and associated values in `PathMap<Par>`. `Empty` remains mode-neutral.
/// No insertion-order collection or decoded-entry cache is retained.
///
/// [`Self::insert_entry`], [`Self::extend_entries`], and
/// [`Self::remove_greatest_entry`] mutate the trie and invalidate serialization
/// caches. `len`, `entries_stable`, `union_locally_free`, and
/// `any_connective_used` are maintained folds, avoiding a decode traversal for
/// common metadata queries.
pub struct EntryTrie {
    /// THE STORE. Keys are `encode_trie_path(entry)` (capless, injective,
    /// prefix-free, and **total** over every `Par` via the `0x0F` escape arm —
    /// `canonical_path.rs`), so a non-ground entry is as storable as a ground
    /// one and there is no arm in which the `Vec` has to come back.
    repr: EPathMapRepr<Par>,
    /// Number of DISTINCT entries. Maintained at insert/remove because
    /// `PathMap::val_count` is documented O(N) ("This is not a cheap method",
    /// `pathmap-0.2.2/src/trie_map.rs:499`).
    len: usize,
    /// `ps.iter().all(eval_stable_par)` — the entry half of the GROUND predicate.
    ///
    /// This no longer selects a protobuf representation; all EPathMaps use the
    /// EPM1 snapshot. It remains exact for canonical-path consumers.
    entries_stable: bool,
    /// Union of the entries' `locally_free` bitsets — the value
    /// `create_set_pathmap_from_elements` used to compute on every conversion.
    union_locally_free: Vec<u8>,
    /// OR of the entries' `connective_used` flags (the map's own `remainder`
    /// is folded in by the caller, not here — it is metadata, not an entry).
    any_connective_used: bool,
    /// Canonical `EPM1` snapshot of the prefix-compressed trie. The `OnceLock`
    /// itself is shared by a clone family, so cloning a cold map and then
    /// serializing several siblings still constructs exactly one snapshot.
    /// Mutation detaches only the mutated sibling by installing a fresh cell.
    /// This is a serialization cache only: equality, hashing, ordering and
    /// lookup walk the PathMap directly and never force it.
    trie_snapshot: Arc<OnceLock<Vec<u8>>>,
    /// Value-independent EPM1 prefix: mode, ACTree03 topology, and value count.
    /// The generated protobuf PDA streams `PathMap<Par>` values after this
    /// prefix, so nested maps share compressed topology without retaining a
    /// complete snapshot for every suffix of a nested chain.
    epm_layout: Arc<OnceLock<epathmap_trie_codec::EpmLayout>>,
}

/// Decode set members in trie order with a read-zipper walk and no sort.
/// This is an explicit compatibility-boundary conversion, never retained in [`EntryTrie`].
/// Set mode intentionally stores only `PathMap<()>` keys; callers that need
/// semantic `Par` values pay one stack-safe decode per key and own the result.
fn decode_set_entries(map: &RholangSetPathMap) -> Vec<Par> {
    use pathmap::zipper::{ZipperIteration, ZipperMoving};
    let mut ps = Vec::new();
    let mut rz = map.read_zipper();
    while rz.to_next_val() {
        ps.push(decode_trie_path(rz.path()).expect(
            "EntryTrie keys are canonical_path encodings; construction and EPM1 decode validate this",
        ));
    }
    ps
}

/// Visit value-bearing paths in descending byte-lexicographic order.
///
/// PathMap exposes forward value iteration and both directions of sibling
/// movement, but no reverse-value convenience iterator. The PDA consumers
/// that push children onto a LIFO worklist need the reverse order; collecting
/// the forward iterator into a temporary vector would allocate one pointer per
/// set member and two per map binding. This walk composes the existing zipper
/// primitives instead and retains only the zipper's path buffer.
fn for_each_raw_value_reverse<'trie, V>(
    map: &'trie pathmap::PathMap<V>,
    mut visit: impl FnMut(&[u8], &'trie V),
) where
    V: Clone + Send + Sync + Unpin + 'static,
{
    use pathmap::zipper::{ZipperMoving, ZipperReadOnlyValues};

    let mut zipper = map.read_zipper();
    while zipper.descend_last_byte() {}

    loop {
        if let Some(value) = zipper.get_val() {
            visit(zipper.path(), value);
        }
        if zipper.at_root() {
            break;
        }
        if zipper.to_prev_sibling_byte() {
            while zipper.descend_last_byte() {}
        } else {
            let ascended = zipper.ascend_byte();
            debug_assert!(ascended, "a non-root zipper must be able to ascend");
        }
    }
}

fn empty_set_trie() -> &'static RholangSetPathMap {
    static EMPTY: OnceLock<RholangSetPathMap> = OnceLock::new();
    EMPTY.get_or_init(RholangSetPathMap::new)
}

fn remove_subtrie_native<V>(map: &mut pathmap::PathMap<V>, prefix: &[u8])
where V: Clone + Send + Sync + Unpin {
    if prefix.is_empty() {
        *map = pathmap::PathMap::new();
        return;
    }
    map.remove_branches_at(prefix, true);
    drop(map.remove(prefix));
}

/// Recreate only the terminating topology paths below a prefixed destination.
/// Value positions are installed separately so set members can retain the
/// historical setSubtrie composition rule and map values remain associated.
fn compose_topology<DestinationValue, SourceValue>(
    destination: &mut pathmap::PathMap<DestinationValue>,
    prefix: &[u8],
    source: &pathmap::PathMap<SourceValue>,
) where
    DestinationValue: Clone + Send + Sync + Unpin,
    SourceValue: Clone + Send + Sync + Unpin,
{
    for relative in source.read_zipper().into_path_iter() {
        let mut absolute = Vec::with_capacity(prefix.len() + relative.len());
        absolute.extend_from_slice(prefix);
        absolute.extend_from_slice(&relative);
        if !absolute.is_empty() {
            destination.create_path(absolute);
        }
    }
}

/// Re-type a PathMap's terminating topology as a membership trie. Values are
/// deliberately excluded: this mask exists only where a structural
/// algebraic operation must be applied to `PathMap<Par>` without inventing an
/// unlawful lattice for `Par`.
fn topology_mask<V>(source: &pathmap::PathMap<V>) -> RholangSetPathMap
where V: Clone + Send + Sync + Unpin {
    let mut mask = RholangSetPathMap::new();
    for leaf in source.read_zipper().into_path_iter() {
        mask.insert(leaf, ());
    }
    mask
}

fn composed_subtrie_member_key(prefix: &[u8], source_key: &[u8]) -> Vec<u8> {
    let (segments, _) = decode_cursor(source_key);
    let segment_bytes = segments.iter().map(Vec::len).sum::<usize>();
    let mut absolute = Vec::with_capacity(prefix.len() + segment_bytes + 1);
    absolute.extend_from_slice(prefix);
    for segment in segments {
        absolute.extend_from_slice(&segment);
    }
    absolute.push(super::canonical_path::tag::TERM);
    absolute
}

fn key_after_dropping_segments(key: &[u8], count: usize) -> Option<Vec<u8>> {
    let (segments, _) = decode_cursor(key);
    if segments.len() <= count {
        return None;
    }
    Some(segments_to_key(&segments[count..], true))
}

fn copy_dropped_topology<V>(
    destination: &mut pathmap::PathMap<V>,
    source: &pathmap::PathMap<V>,
    count: usize,
) where
    V: Clone + Send + Sync + Unpin,
{
    for source_leaf in source.read_zipper().into_path_iter() {
        if let Some(destination_leaf) = key_after_dropping_segments(&source_leaf, count) {
            destination.create_path(destination_leaf);
        }
    }
}

#[allow(deprecated)]
fn validate_canonical_key(kind: &str, key: &[u8]) -> Result<(), DecodeError> {
    let decoded = decode_trie_path(key)
        .map_err(|error| DecodeError::new(format!("EPathMap {kind} key: {error:?}")))?;
    let canonical = encode_trie_path(&decoded);
    crate::rust::rholang::par_children::dismantle(decoded);
    if canonical == key {
        Ok(())
    } else {
        Err(DecodeError::new(format!(
            "EPathMap {kind} key is not canonical"
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EPathMapModeError {
    expected: EPathMapMode,
    actual: EPathMapMode,
}

impl fmt::Display for EPathMapModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EPathMap is in {:?} mode; {:?} operation would mix set and map membership",
            self.actual, self.expected
        )
    }
}

impl std::error::Error for EPathMapModeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EPathMapEmptyModeError;

impl fmt::Display for EPathMapEmptyModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "cannot create value-free EPathMap topology from neutral empty storage; insert a set member or map entry first to select PathMap<()> or PathMap<Par>",
        )
    }
}

impl std::error::Error for EPathMapEmptyModeError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EPathMapAlgebraError {
    ModeMismatch {
        left: EPathMapMode,
        right: EPathMapMode,
    },
    ValueConflict {
        operation: &'static str,
        encoded_key: Vec<u8>,
    },
    AmbiguousEmpty {
        operation: &'static str,
    },
}

impl fmt::Display for EPathMapAlgebraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModeMismatch { left, right } => write!(
                f,
                "EPathMap algebra cannot combine {:?} and {:?} storage",
                left, right
            ),
            Self::ValueConflict {
                operation,
                encoded_key,
            } => write!(
                f,
                "EPathMap {operation} has unequal values at canonical key 0x{}",
                hex::encode(encoded_key)
            ),
            Self::AmbiguousEmpty { operation } => write!(
                f,
                "EPathMap {operation} cannot choose PathMap<()> or PathMap<Par> from two neutral empty operands"
            ),
        }
    }
}

impl std::error::Error for EPathMapAlgebraError {}

impl EntryTrie {
    #[allow(deprecated)]
    fn from_repr(repr: EPathMapRepr<Par>) -> Result<Self, DecodeError> {
        let mut len = 0usize;
        match &repr {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(map) => {
                for (key, ()) in map.iter() {
                    validate_canonical_key("set", &key)?;
                    len += 1;
                }
            }
            EPathMapRepr::Map(map) => {
                for (key, _) in map.iter() {
                    validate_canonical_key("map", &key)?;
                    len += 1;
                }
            }
        }

        let mut built = EntryTrie {
            repr,
            len,
            entries_stable: true,
            union_locally_free: Vec::new(),
            any_connective_used: false,
            trie_snapshot: Arc::new(OnceLock::new()),
            epm_layout: Arc::new(OnceLock::new()),
        };
        built.recompute_folds();
        Ok(built)
    }

    /// Decode set members into an owned vector in canonical trie order.
    ///
    /// This is deliberately explicit and uncached: `EntryTrie` remains a
    /// prefix-compressed `PathMap<()>`, never a trie plus a retained `Vec<Par>`.
    /// Map mode has distinct key/value APIs and cannot be flattened through
    /// this set-only surface.
    pub fn entries_owned(&self) -> Vec<Par> {
        match &self.repr {
            EPathMapRepr::Empty => Vec::new(),
            EPathMapRepr::Set(map) => decode_set_entries(map),
            EPathMapRepr::Map(_) => {
                panic!("set-only entry decoding used on map-mode EPathMap")
            }
        }
    }

    /// The canonical, versioned PathMap arena used by every new serialization
    /// surface.
    ///
    /// Cold construction is O(trie nodes + encoded map-value bytes). The clone
    /// family shares that construction even when cloned while cold. Warm reads
    /// are one `Arc`/`OnceLock` indirection and return the cached slice without
    /// allocation; the enclosing serializer then copies the slice to its output.
    pub fn trie_snapshot(&self) -> &[u8] {
        self.trie_snapshot
            .get_or_init(|| epathmap_trie_codec::encode_with_layout(&self.repr, self.epm_layout()))
            .as_slice()
    }

    #[inline]
    pub(crate) fn epm_layout(&self) -> &epathmap_trie_codec::EpmLayout {
        self.epm_layout
            .get_or_init(|| epathmap_trie_codec::layout(&self.repr))
    }

    #[cfg(test)]
    fn snapshot_is_forced(&self) -> bool { self.trie_snapshot.get().is_some() }

    /// The set trie itself, handed out by O(1) root clone.
    pub fn set_trie(&self) -> &RholangSetPathMap {
        match &self.repr {
            EPathMapRepr::Empty => empty_set_trie(),
            EPathMapRepr::Set(map) => map,
            EPathMapRepr::Map(_) => panic!(
                "set-only EPathMap API used on map-mode storage; use map_trie()/map lookup APIs"
            ),
        }
    }

    pub fn map_trie(&self) -> Option<&RholangMapPathMap> { self.repr.as_map() }

    pub fn for_each_raw_set_entry(
        &self,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<(), EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(map) => {
                for (key, ()) in map.iter() {
                    visit(&key);
                }
                Ok(())
            }
            EPathMapRepr::Map(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Set,
                actual: EPathMapMode::Map,
            }),
        }
    }

    /// Visit map-mode storage without decoding or cloning the key. The key
    /// slice is valid only for the callback; the associated value borrow has
    /// the EntryTrie's lifetime and can be placed directly on a PDA worklist.
    pub fn for_each_raw_map_entry<'trie>(
        &'trie self,
        mut visit: impl FnMut(&[u8], &'trie Par),
    ) -> Result<(), EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
            EPathMapRepr::Map(map) => {
                for (key, value) in map.iter() {
                    visit(&key, value);
                }
                Ok(())
            }
        }
    }

    /// Visit set members in reverse canonical trie order without materializing
    /// decoded entries or a pointer projection. This is the order a LIFO PDA
    /// needs when it must evaluate the forward canonical order.
    pub fn for_each_raw_set_entry_reverse(
        &self,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<(), EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(map) => {
                for_each_raw_value_reverse(map, |key, ()| visit(key));
                Ok(())
            }
            EPathMapRepr::Map(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Set,
                actual: EPathMapMode::Map,
            }),
        }
    }

    /// Visit map bindings in reverse canonical trie order without cloning keys
    /// or values. Associated values retain the EntryTrie's lifetime and can
    /// be placed directly on a generated or handwritten PDA worklist.
    pub fn for_each_raw_map_entry_reverse<'trie>(
        &'trie self,
        mut visit: impl FnMut(&[u8], &'trie Par),
    ) -> Result<(), EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
            EPathMapRepr::Map(map) => {
                for_each_raw_value_reverse(map, |key, value| visit(key, value));
                Ok(())
            }
        }
    }

    pub fn representation(&self) -> &EPathMapRepr<Par> { &self.repr }

    pub fn mode(&self) -> EPathMapMode { self.repr.mode() }

    fn invalidate_derived(&mut self) {
        self.trie_snapshot = Arc::new(OnceLock::new());
        self.epm_layout = Arc::new(OnceLock::new());
    }

    fn fold_par_metadata(&mut self, par: &Par) {
        self.fold_par_metadata_with_stability(par, eval_stable_par(par));
    }

    fn fold_par_metadata_with_stability(&mut self, par: &Par, stable: bool) {
        self.entries_stable &= stable;
        self.any_connective_used |= par.connective_used;
        self.union_locally_free = crate::rust::utils::union(
            std::mem::take(&mut self.union_locally_free),
            par.locally_free.clone(),
        );
    }

    /// Distinct entry count — O(1) (see the field docs for why it is not
    /// `PathMap::val_count`).
    pub fn len(&self) -> usize { self.len }

    /// `true` iff the map holds neither values nor explicit value-free
    /// topology — O(1) (`PathMap::is_empty` reads the root node's tag).
    pub fn is_empty(&self) -> bool { self.repr.is_empty() }

    /// `true` iff every entry is in the codec's ground domain
    /// (`eval_stable_par`) — the entry half of the GROUND wire predicate. O(1).
    pub fn entries_stable(&self) -> bool { self.entries_stable }

    /// Union of the entries' `locally_free` bitsets.
    pub fn union_locally_free(&self) -> &[u8] { &self.union_locally_free }

    /// OR of the entries' `connective_used` flags.
    pub fn any_connective_used(&self) -> bool { self.any_connective_used }

    /// Add one entry. Idempotent — re-adding an entry already present is a
    /// no-op on the set, which is what makes `setLeaf` and `graft` unable to
    /// create duplicates.
    ///
    /// Invalidates both serialization caches but never hands out a mutable
    /// collection view: the only thing a caller can do is name an entry.
    pub fn insert_entry(&mut self, par: Par) {
        let consumed = self.insert_entry_replacing(par);
        // Set mode stores only the canonical key. The supplied Par can itself
        // be arbitrarily deep, so release it through the iterative teardown.
        crate::rust::rholang::par_children::dismantle(consumed);
    }

    pub fn try_insert_entry(&mut self, par: Par) -> Result<(), EPathMapModeError> {
        if matches!(self.repr, EPathMapRepr::Map(_)) {
            return Err(EPathMapModeError {
                expected: EPathMapMode::Set,
                actual: EPathMapMode::Map,
            });
        }
        self.insert_entry(par);
        Ok(())
    }

    pub(crate) fn try_insert_entry_replacing(
        &mut self,
        par: Par,
    ) -> Result<Par, EPathMapModeError> {
        if matches!(self.repr, EPathMapRepr::Map(_)) {
            return Err(EPathMapModeError {
                expected: EPathMapMode::Set,
                actual: EPathMapMode::Map,
            });
        }
        Ok(self.insert_entry_replacing(par))
    }

    /// The decoder-facing insertion primitive.
    ///
    /// Returns the consumed term instead of dropping it. Set specialization is
    /// `PathMap<()>`, so no `Par` is retained whether the key was new or
    /// already present. A PDA decoder places the returned term on its explicit
    /// teardown worklist.
    pub(crate) fn insert_entry_replacing(&mut self, par: Par) -> Par {
        // ★ ONE walk, not two. `encode_trie_path` opens with
        // `let stable = known_stable || eval_stable_par(par)` — stability is what selects
        // the escape arm — so this used to run `eval_stable_par` a SECOND time over the
        // same entry, and `entries_stable` became a second opinion about something the
        // codec had already decided. Now the codec hands the bit back.
        //
        // The fold remains exact because it is the encoder's own verdict rather
        // than an approximation, and
        // taking it here costs nothing — the alternative is a second walk that could form
        // a second opinion about something the codec has already decided.
        let (key, stable) = encode_trie_path_with_stability(&par);
        self.insert_encoded(key, stable, par)
    }

    /// File `par` under a key the caller has **already** obtained from
    /// [`encode_trie_path_with_stability`], carrying the stability verdict that
    /// came back with it.
    ///
    /// ★ The whole of [`EntryTrie::insert_entry`]'s body below the encode, so
    /// the fold maintenance (`entries_stable`, `any_connective_used`,
    /// `union_locally_free`, `len`) and the two cache invalidations are stated
    /// once. The generated sorter also needs the key in its own hand before it
    /// files the canonical term and must not pay a second
    /// `encode_trie_path` to hand it back.
    ///
    /// ⚠ `key` must be `encode_trie_path(&par)` and `stable` its companion
    /// verdict. It is `pub(crate)` and takes both together — never a bare key —
    /// so the only way to reach it is to have called the encoder.
    pub(crate) fn insert_encoded(&mut self, key: Vec<u8>, stable: bool, par: Par) -> Par {
        if matches!(self.repr, EPathMapRepr::Map(_)) {
            panic!("set insertion attempted on map-mode EPathMap");
        }
        self.entries_stable &= stable;
        self.any_connective_used |= par.connective_used;
        self.union_locally_free = crate::rust::utils::union(
            std::mem::take(&mut self.union_locally_free),
            par.locally_free.clone(),
        );
        if matches!(self.repr, EPathMapRepr::Empty) {
            self.repr = EPathMapRepr::Set(RholangSetPathMap::new());
        }
        let EPathMapRepr::Set(map) = &mut self.repr else {
            unreachable!("map mode was rejected and empty mode was specialized")
        };
        let replaced = map.insert(key, ());
        if replaced.is_none() {
            self.len += 1;
        }
        self.invalidate_derived();
        par
    }

    /// Associate `value` with `key`, specializing neutral empty storage to
    /// `PathMap<Par>` on the first insertion. Set/map mixing is rejected.
    pub fn insert_map_entry(&mut self, key: Par, value: Par) -> Result<(), EPathMapModeError> {
        let replaced = self.insert_map_entry_replacing(key, value)?;
        if let Some(replaced) = replaced {
            crate::rust::rholang::par_children::dismantle(replaced);
        }
        Ok(())
    }

    pub(crate) fn insert_map_entry_replacing(
        &mut self,
        key: Par,
        value: Par,
    ) -> Result<Option<Par>, EPathMapModeError> {
        let (encoded_key, key_stable) = encode_trie_path_with_stability(&key);
        self.insert_encoded_map_entry_replacing(&encoded_key, key_stable, key, value)
    }

    /// Associate a value with an already-encoded canonical key.
    ///
    /// `encoded_key` and `key_stable` must be the pair returned by
    /// [`encode_trie_path_with_stability`] for `key`. Keeping the pair together
    /// lets PathMap-aware callers index companion data and insert the entry
    /// after one canonical-key traversal instead of encoding the key twice.
    pub(crate) fn insert_encoded_map_entry_replacing(
        &mut self,
        encoded_key: &[u8],
        key_stable: bool,
        key: Par,
        value: Par,
    ) -> Result<Option<Par>, EPathMapModeError> {
        if matches!(self.repr, EPathMapRepr::Set(_)) {
            return Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            });
        }

        if matches!(self.repr, EPathMapRepr::Empty) {
            self.repr = EPathMapRepr::Map(pathmap::PathMap::new());
        }

        self.fold_par_metadata_with_stability(&key, key_stable);
        self.fold_par_metadata(&value);
        crate::rust::rholang::par_children::dismantle(key);

        let EPathMapRepr::Map(map) = &mut self.repr else {
            unreachable!("set mode was rejected and empty mode was specialized")
        };
        let replaced = map.insert(encoded_key, value);
        if replaced.is_none() {
            self.len += 1;
        } else {
            // Replacing a value can remove metadata bits, so monotone forward
            // folds are insufficient on this path.
            self.recompute_folds();
        }
        self.invalidate_derived();
        Ok(replaced)
    }

    pub fn get_map_value(&self, key: &Par) -> Result<Option<&Par>, EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(None),
            EPathMapRepr::Map(map) => Ok(map.get(encode_trie_path(key))),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
        }
    }

    pub fn get_map_value_by_encoded_key(
        &self,
        key: &[u8],
    ) -> Result<Option<&Par>, EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(None),
            EPathMapRepr::Map(map) => Ok(map.get(key)),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
        }
    }

    /// Associate a value at an already-canonical cursor key without encoding
    /// that key a second time.  This is the map-mode `setLeaf` primitive.
    pub fn insert_map_value_by_encoded_key(
        &mut self,
        encoded_key: &[u8],
        value: Par,
    ) -> Result<(), EPathMapModeError> {
        if matches!(self.repr, EPathMapRepr::Set(_)) {
            return Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            });
        }
        let key = decode_trie_path(encoded_key)
            .expect("EPathMap cursor keys are in the canonical Par-path image");
        let key_stable = eval_stable_par(&key);
        let replaced =
            self.insert_encoded_map_entry_replacing(encoded_key, key_stable, key, value)?;
        if let Some(replaced) = replaced {
            crate::rust::rholang::par_children::dismantle(replaced);
        }
        Ok(())
    }

    /// Resolve an `EZipper` cursor against whichever homogeneous PathMap
    /// specialization this trie owns.  Prefix cursors consult value presence
    /// in the selected trie; neutral empty uses an empty set trie because both
    /// specializations make the same choice when no value is present.
    pub fn cursor_entry_key(&self, segments: &[Vec<u8>], kind: CursorKind) -> Vec<u8> {
        match &self.repr {
            EPathMapRepr::Empty => encode_cursor_entry_key(segments, kind, empty_set_trie()),
            EPathMapRepr::Set(map) => encode_cursor_entry_key(segments, kind, map),
            EPathMapRepr::Map(map) => encode_cursor_entry_key(segments, kind, map),
        }
    }

    /// Resolve a relative path argument against a cursor without projecting a
    /// map-valued trie through the set-only compatibility boundary.
    pub fn entry_key_at(&self, cursor: &[Vec<u8>], path: &Par) -> Vec<u8> {
        match &self.repr {
            EPathMapRepr::Empty => encode_entry_key_at(cursor, path, empty_set_trie()),
            EPathMapRepr::Set(map) => encode_entry_key_at(cursor, path, map),
            EPathMapRepr::Map(map) => encode_entry_key_at(cursor, path, map),
        }
    }

    /// Read the semantic leaf stored at an encoded key.  Set mode returns the
    /// key decoded as its member; map mode returns the associated value.
    pub fn leaf_at_encoded_key(&self, key: &[u8]) -> Option<Par> {
        match &self.repr {
            EPathMapRepr::Empty => None,
            EPathMapRepr::Set(map) => map.contains(key).then(|| {
                decode_trie_path(key).expect("set-mode EPathMap keys are canonical Par paths")
            }),
            EPathMapRepr::Map(map) => map.get(key).cloned(),
        }
    }

    /// Test exact entry membership without materializing the stored member or
    /// value.  This is deliberately distinct from zipper/path lookup: a list
    /// `Par` is one canonical key here, not a sequence of relative segments.
    pub fn contains_encoded_key(&self, key: &[u8]) -> bool {
        match &self.repr {
            EPathMapRepr::Empty => false,
            EPathMapRepr::Set(map) => map.contains(key),
            EPathMapRepr::Map(map) => map.contains(key),
        }
    }

    /// Test exact set-member/map-key membership with one canonical-key encode
    /// and one PathMap lookup.  No decoded-key or entry-vector projection is
    /// constructed.
    pub fn contains_entry(&self, entry: &Par) -> bool {
        self.contains_encoded_key(&encode_trie_path(entry))
    }

    pub fn path_prefix_exists(&self, prefix: &[u8]) -> bool {
        match &self.repr {
            EPathMapRepr::Empty => false,
            EPathMapRepr::Set(map) => path_prefix_exists(map, prefix),
            EPathMapRepr::Map(map) => path_prefix_exists(map, prefix),
        }
    }

    pub fn collect_child_segments(&self, prefix: &[u8], limit: Option<usize>) -> Vec<Vec<u8>> {
        match &self.repr {
            EPathMapRepr::Empty => Vec::new(),
            EPathMapRepr::Set(map) => collect_child_segments(map, prefix, limit),
            EPathMapRepr::Map(map) => collect_child_segments(map, prefix, limit),
        }
    }

    pub fn subtrie_value_count(&self, prefix: &[u8]) -> usize {
        match &self.repr {
            EPathMapRepr::Empty => 0,
            EPathMapRepr::Set(map) => subtrie_value_count(map, prefix),
            EPathMapRepr::Map(map) => subtrie_value_count(map, prefix),
        }
    }

    pub fn next_value_key(&self, from_key: &[u8]) -> Option<Vec<u8>> {
        match &self.repr {
            EPathMapRepr::Empty => None,
            EPathMapRepr::Set(map) => next_value_key(map, from_key),
            EPathMapRepr::Map(map) => next_value_key(map, from_key),
        }
    }

    /// Keep the trie rooted under `prefix` with PathMap's native restriction
    /// algebra.  Values stay in their specialization and keys stay compressed;
    /// no decoded member or key/value vector is constructed.
    pub fn subtrie(&self, prefix: &[u8]) -> Self {
        let repr = match &self.repr {
            EPathMapRepr::Empty => EPathMapRepr::Empty,
            EPathMapRepr::Set(map) => {
                let restricted = if prefix.is_empty() {
                    map.clone()
                } else {
                    map.restrict(&pathmap::PathMap::single(prefix, ()))
                };
                EPathMapRepr::Set(restricted)
            }
            EPathMapRepr::Map(map) => {
                let restricted = if prefix.is_empty() {
                    map.clone()
                } else {
                    map.restrict(&pathmap::PathMap::single(prefix, Par::default()))
                };
                EPathMapRepr::Map(restricted)
            }
        };
        Self::algebra_result(repr)
    }

    /// Remove the first `count` codec segments from every stored path.  Keys
    /// are rewritten directly from the cursor codec and associated map values
    /// remain in their PathMap value slots; no `Vec<Par>` projection exists on
    /// this path.
    pub fn drop_head(&self, count: usize) -> Self {
        if count == 0 {
            return self.clone();
        }
        let repr = match &self.repr {
            EPathMapRepr::Empty => EPathMapRepr::Empty,
            EPathMapRepr::Set(source) => {
                let mut destination = RholangSetPathMap::new();
                copy_dropped_topology(&mut destination, source, count);
                for (key, ()) in source.iter() {
                    if let Some(key) = key_after_dropping_segments(&key, count) {
                        destination.insert(key, ());
                    }
                }
                EPathMapRepr::Set(destination)
            }
            EPathMapRepr::Map(source) => {
                let mut destination = pathmap::PathMap::<Par>::new();
                copy_dropped_topology(&mut destination, source, count);
                for (key, value) in source.iter() {
                    if let Some(key) = key_after_dropping_segments(&key, count) {
                        destination.insert(key, value.clone());
                    }
                }
                EPathMapRepr::Map(destination)
            }
        };
        Self::algebra_result(repr)
    }

    fn normalize_after_destructive_mutation(&mut self) {
        self.len = match &self.repr {
            EPathMapRepr::Empty => 0,
            EPathMapRepr::Set(map) => map.val_count(),
            EPathMapRepr::Map(map) => map.val_count(),
        };
        if self.repr.is_empty() {
            self.repr = EPathMapRepr::Empty;
            self.len = 0;
        }
        self.recompute_folds();
    }

    /// Remove only the value at `key`, preserving any descendants and
    /// value-free topology below it.
    pub fn remove_encoded_entry(&mut self, key: &[u8]) -> bool {
        let removed = match &mut self.repr {
            EPathMapRepr::Empty => return false,
            EPathMapRepr::Set(map) => map.remove(key).is_some(),
            EPathMapRepr::Map(map) => match map.remove(key) {
                Some(value) => {
                    crate::rust::rholang::par_children::dismantle(value);
                    true
                }
                None => false,
            },
        };
        if removed {
            self.normalize_after_destructive_mutation();
        }
        removed
    }

    /// Remove descendants of `prefix` with PathMap's write-zipper primitive,
    /// retaining a value stored exactly at the prefix.
    pub fn remove_branches_at(&mut self, prefix: &[u8]) -> bool {
        let changed = match &mut self.repr {
            EPathMapRepr::Empty => false,
            EPathMapRepr::Set(map) => map.remove_branches_at(prefix, true),
            EPathMapRepr::Map(map) => map.remove_branches_at(prefix, true),
        };
        if changed {
            self.normalize_after_destructive_mutation();
        }
        changed
    }

    /// Remove the value and every branch at or below `prefix` without a
    /// whole-map key scan.
    pub fn remove_subtrie_at(&mut self, prefix: &[u8]) -> bool {
        if self.repr.is_empty() {
            return false;
        }
        if prefix.is_empty() {
            self.repr = EPathMapRepr::Empty;
            self.len = 0;
            self.recompute_folds();
            return true;
        }
        let changed = match &mut self.repr {
            EPathMapRepr::Empty => false,
            EPathMapRepr::Set(map) => {
                let branches = map.remove_branches_at(prefix, true);
                map.remove(prefix).is_some() || branches
            }
            EPathMapRepr::Map(map) => {
                let branches = map.remove_branches_at(prefix, true);
                let removed = map.remove(prefix);
                if let Some(value) = removed {
                    crate::rust::rholang::par_children::dismantle(value);
                    true
                } else {
                    branches
                }
            }
        };
        if changed {
            self.normalize_after_destructive_mutation();
        }
        changed
    }

    /// Create value-free topology in the already-selected specialization.
    /// Neutral empty cannot choose between `PathMap<()>` and `PathMap<Par>`;
    /// callers must first perform a mode-selecting insertion.
    pub fn create_path(&mut self, path: &[u8]) -> Result<bool, EPathMapEmptyModeError> {
        let changed = match &mut self.repr {
            EPathMapRepr::Empty => return Err(EPathMapEmptyModeError),
            EPathMapRepr::Set(map) => map.create_path(path),
            EPathMapRepr::Map(map) => map.create_path(path),
        };
        if changed {
            self.invalidate_derived();
        }
        Ok(changed)
    }

    /// Replace the branch at `cursor_segments` with `source`, preserving the
    /// homogeneous PathMap specialization.  Source topology is copied with a
    /// native zipper walk; value keys are composed directly from codec bytes,
    /// and map values never leave their associated slots.
    pub fn replace_subtrie(
        &mut self,
        cursor_segments: &[Vec<u8>],
        cursor_kind: CursorKind,
        source: &Self,
    ) -> Result<(), EPathMapAlgebraError> {
        let prefix = segments_to_key(cursor_segments, false);
        let left_mode = self.mode();
        let right_mode = source.mode();

        let repr = match (&self.repr, &source.repr) {
            (EPathMapRepr::Empty, EPathMapRepr::Empty) => {
                if cursor_segments.is_empty() {
                    EPathMapRepr::Empty
                } else {
                    return Err(EPathMapAlgebraError::AmbiguousEmpty {
                        operation: "setSubtrie",
                    });
                }
            }
            (EPathMapRepr::Set(base), EPathMapRepr::Empty) => {
                let mut destination = base.clone();
                remove_subtrie_native(&mut destination, &prefix);
                if !cursor_segments.is_empty() {
                    let key = encode_cursor_entry_key(cursor_segments, cursor_kind, &destination);
                    destination.insert(key, ());
                }
                EPathMapRepr::Set(destination)
            }
            (EPathMapRepr::Map(base), EPathMapRepr::Empty) => {
                let mut destination = base.clone();
                remove_subtrie_native(&mut destination, &prefix);
                if !cursor_segments.is_empty() {
                    let key = encode_cursor_entry_key(cursor_segments, cursor_kind, &destination);
                    destination.create_path(key);
                }
                EPathMapRepr::Map(destination)
            }
            (EPathMapRepr::Empty, EPathMapRepr::Set(source_map)) => {
                let mut destination = RholangSetPathMap::new();
                compose_topology(&mut destination, &prefix, source_map);
                for (source_key, ()) in source_map.iter() {
                    destination.insert(composed_subtrie_member_key(&prefix, &source_key), ());
                }
                EPathMapRepr::Set(destination)
            }
            (EPathMapRepr::Set(base), EPathMapRepr::Set(source_map)) => {
                let mut destination = base.clone();
                remove_subtrie_native(&mut destination, &prefix);
                compose_topology(&mut destination, &prefix, source_map);
                for (source_key, ()) in source_map.iter() {
                    destination.insert(composed_subtrie_member_key(&prefix, &source_key), ());
                }
                EPathMapRepr::Set(destination)
            }
            (EPathMapRepr::Empty, EPathMapRepr::Map(source_map)) => {
                let mut destination = pathmap::PathMap::<Par>::new();
                compose_topology(&mut destination, &prefix, source_map);
                for (source_key, value) in source_map.iter() {
                    destination.insert(
                        composed_subtrie_member_key(&prefix, &source_key),
                        value.clone(),
                    );
                }
                EPathMapRepr::Map(destination)
            }
            (EPathMapRepr::Map(base), EPathMapRepr::Map(source_map)) => {
                let mut destination = base.clone();
                remove_subtrie_native(&mut destination, &prefix);
                compose_topology(&mut destination, &prefix, source_map);
                for (source_key, value) in source_map.iter() {
                    destination.insert(
                        composed_subtrie_member_key(&prefix, &source_key),
                        value.clone(),
                    );
                }
                EPathMapRepr::Map(destination)
            }
            (EPathMapRepr::Set(_), EPathMapRepr::Map(_))
            | (EPathMapRepr::Map(_), EPathMapRepr::Set(_)) => {
                return Err(EPathMapAlgebraError::ModeMismatch {
                    left: left_mode,
                    right: right_mode,
                });
            }
        };

        *self = Self::algebra_result(repr);
        Ok(())
    }

    pub fn for_each_map_entry(
        &self,
        mut visit: impl FnMut(&Par, &Par),
    ) -> Result<(), EPathMapModeError> {
        match &self.repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
            EPathMapRepr::Map(map) => {
                for (key, value) in map.iter() {
                    let key = decode_trie_path(&key)
                        .expect("map-mode EPathMap keys are canonical_path encodings");
                    visit(&key, value);
                    crate::rust::rholang::par_children::dismantle(key);
                }
                Ok(())
            }
        }
    }

    fn algebra_result(repr: EPathMapRepr<Par>) -> Self {
        if repr.is_empty() {
            Self::default()
        } else {
            Self::from_repr(repr).expect(
                "PathMap algebra preserves canonical EPathMap keys produced by construction",
            )
        }
    }

    fn joined_result(repr: EPathMapRepr<Par>, len: usize, left: &Self, right: &Self) -> Self {
        if repr.is_empty() {
            return Self::default();
        }
        Self {
            repr,
            len,
            entries_stable: left.entries_stable && right.entries_stable,
            union_locally_free: crate::rust::utils::union(
                left.union_locally_free.clone(),
                right.union_locally_free.clone(),
            ),
            any_connective_used: left.any_connective_used || right.any_connective_used,
            trie_snapshot: Arc::new(OnceLock::new()),
            epm_layout: Arc::new(OnceLock::new()),
        }
    }

    #[inline]
    fn exact_map_value_eq(left: &Par, right: &Par) -> bool { left.cmp(right) == Ordering::Equal }

    fn mode_mismatch(&self, other: &Self) -> EPathMapAlgebraError {
        EPathMapAlgebraError::ModeMismatch {
            left: self.mode(),
            right: other.mode(),
        }
    }

    pub fn try_join(&self, other: &Self) -> Result<Self, EPathMapAlgebraError> {
        match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, _) => Ok(other.clone()),
            (_, EPathMapRepr::Empty) => Ok(self.clone()),
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {
                let joined = left.join(right);
                let len = joined.val_count();
                Ok(Self::joined_result(
                    EPathMapRepr::Set(joined),
                    len,
                    self,
                    other,
                ))
            }
            (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {
                let mut joined = left.clone();
                compose_topology(&mut joined, &[], right);
                let mut overlap = 0usize;
                for (key, right_value) in right.iter() {
                    if let Some(left_value) = joined.get(&key) {
                        overlap += 1;
                        if !Self::exact_map_value_eq(left_value, right_value) {
                            return Err(EPathMapAlgebraError::ValueConflict {
                                operation: "join",
                                encoded_key: key,
                            });
                        }
                    } else {
                        joined.insert(&key, right_value.clone());
                    }
                }
                Ok(Self::joined_result(
                    EPathMapRepr::Map(joined),
                    self.len + other.len - overlap,
                    self,
                    other,
                ))
            }
            _ => Err(self.mode_mismatch(other)),
        }
    }

    pub fn try_meet(&self, other: &Self) -> Result<Self, EPathMapAlgebraError> {
        match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, _) | (_, EPathMapRepr::Empty) => Ok(Self::default()),
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {
                Ok(Self::algebra_result(EPathMapRepr::Set(left.meet(right))))
            }
            (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {
                let mut intersection = pathmap::PathMap::new();
                let topology = topology_mask(left).meet(&topology_mask(right));
                compose_topology(&mut intersection, &[], &topology);
                let keys = if self.len <= other.len { left } else { right };
                for (key, _) in keys.iter() {
                    let (Some(left_value), Some(right_value)) = (left.get(&key), right.get(&key))
                    else {
                        continue;
                    };
                    if !Self::exact_map_value_eq(left_value, right_value) {
                        return Err(EPathMapAlgebraError::ValueConflict {
                            operation: "meet",
                            encoded_key: key,
                        });
                    }
                    intersection.insert(&key, left_value.clone());
                }
                Ok(Self::algebra_result(EPathMapRepr::Map(intersection)))
            }
            _ => Err(self.mode_mismatch(other)),
        }
    }

    pub fn try_subtract(&self, other: &Self) -> Result<Self, EPathMapAlgebraError> {
        match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, _) => Ok(Self::default()),
            (_, EPathMapRepr::Empty) => Ok(self.clone()),
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => Ok(Self::algebra_result(
                EPathMapRepr::Set(left.subtract(right)),
            )),
            (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {
                let mut difference = left.clone();
                for (key, _) in right.iter() {
                    let removed = difference.remove(&key);
                    if let Some(removed) = removed {
                        crate::rust::rholang::par_children::dismantle(removed);
                    }
                }
                Ok(Self::algebra_result(EPathMapRepr::Map(difference)))
            }
            _ => Err(self.mode_mismatch(other)),
        }
    }

    pub fn try_restrict(&self, other: &Self) -> Result<Self, EPathMapAlgebraError> {
        match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, _) | (_, EPathMapRepr::Empty) => Ok(Self::default()),
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => Ok(Self::algebra_result(
                EPathMapRepr::Set(left.restrict(right)),
            )),
            (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => Ok(Self::algebra_result(
                EPathMapRepr::Map(left.restrict(right)),
            )),
            _ => Err(self.mode_mismatch(other)),
        }
    }

    pub fn try_restrict_member_prefixes(&self, other: &Self) -> Result<Self, EPathMapAlgebraError> {
        match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, _) | (_, EPathMapRepr::Empty) => Ok(Self::default()),
            (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {
                let mut prefixes = RholangSetPathMap::new();
                for (key, ()) in right.iter() {
                    let entry = decode_trie_path(&key)
                        .expect("set-mode EPathMap keys are canonical_path encodings");
                    prefixes.insert(segments_to_key(&par_to_path(&entry), false), ());
                    crate::rust::rholang::par_children::dismantle(entry);
                }
                Ok(Self::algebra_result(EPathMapRepr::Set(
                    left.restrict(&prefixes),
                )))
            }
            (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {
                let mut prefixes = pathmap::PathMap::new();
                for (key, value) in right.iter() {
                    let entry = decode_trie_path(&key)
                        .expect("map-mode EPathMap keys are canonical_path encodings");
                    prefixes.insert(segments_to_key(&par_to_path(&entry), false), value.clone());
                    crate::rust::rholang::par_children::dismantle(entry);
                }
                Ok(Self::algebra_result(EPathMapRepr::Map(
                    left.restrict(&prefixes),
                )))
            }
            _ => Err(self.mode_mismatch(other)),
        }
    }

    /// Add every entry of `other` — the set union that `graft` performs.
    ///
    /// ★ **A trie-to-trie walk, not a materialise-and-reinsert.** This used to be
    /// `for par in other.view().iter() { self.insert_entry(par.clone()) }`, which did two
    /// avoidable things per entry:
    ///
    /// 1. **Forced `other`'s memo**, deep-cloning its whole entry set into a `Vec<Par>` — the
    ///    flat shadow of a prefix-compressed trie, materialised only to be walked once.
    /// 2. **Re-derived a key `other` already holds.** `insert_entry` opens with
    ///    `encode_trie_path(&par)`, and that byte string *is* the key the entry is stored under
    ///    in `other`. Recomputing it is a full canonical encode per entry — and for a nested
    ///    `EPathMap`, a hereditary one.
    ///
    /// ⇒ Reading `other`'s keys directly is strictly less work and keeps a trie operation a
    /// trie operation end to end. The pair comes straight off the read zipper, in the same walk
    /// order [`entries_in_trie_order`] uses.
    ///
    /// ⚠ **Byte-neutral by injectivity, not by inspection.** `encode_trie_path` is injective
    /// (`canonical_path.rs` — capless, prefix-free, total), so `rz.path()` equals what
    /// `encode_trie_path(rz.val())` would have produced. That invariant is checked in release
    /// builds by [`EntryTrie::adopt_trie`], which re-files on divergence rather than trusting
    /// it. Equal keys therefore imply equal entries, which is what makes keep-vs-replace
    /// immaterial here.
    ///
    /// ⚠ The three metadata folds are maintained inline rather than delegated, because
    /// `insert_entry` is no longer on this path. Each is monotone and O(1) per entry — the same
    /// three folds, in the same order.
    pub fn extend_entries(&mut self, other: &EntryTrie) {
        if matches!(self.repr, EPathMapRepr::Map(_)) {
            panic!("set union attempted on map-mode EPathMap");
        }
        let other_map = match &other.repr {
            EPathMapRepr::Empty => return,
            EPathMapRepr::Set(map) => map,
            EPathMapRepr::Map(_) => panic!("cannot mix map-mode entries into a set EPathMap"),
        };
        if matches!(self.repr, EPathMapRepr::Empty) {
            self.repr = EPathMapRepr::Set(RholangSetPathMap::new());
        }
        let EPathMapRepr::Set(self_map) = &mut self.repr else {
            unreachable!("map mode was rejected and empty mode was specialized")
        };

        // PathMap's lattice join operates on whole compressed subtries and can
        // preserve shared nodes. Counting the result is one node cata; it is
        // still cheaper than reinserting every full key through the root.
        *self_map = self_map.join(other_map);
        self.len = self_map.val_count();

        // ★★ The three metadata folds COMBINE from `other`'s aggregates — O(1) each, not
        // O(Σ entries). They used to be re-derived per entry inside the loop above, which meant
        // `eval_stable_par(par)` — a full walk of the entry — on every element, while the source
        // trie was already holding the answer as a maintained fold.
        //
        // ⚠ Exactness under OVERLAP is the property to check, and it holds because all three are
        // **monotone and idempotent**: `&&`, `||` and bitset union each absorb a repeated
        // operand (`x ∧ x = x`, `x ∨ x = x`, `S ∪ S = S`). An entry present in both tries
        // therefore contributes the same value whether folded once or twice — which is what
        // per-entry folding relied on as well; it simply paid to rediscover it.
        //
        // ⚠ `len` is the ONE fold that cannot combine this way, and is left in the loop above:
        // the union may overlap, so `self.len + other.len` would over-count. Only the insert's
        // return value distinguishes a new key from a replaced one.
        self.entries_stable &= other.entries_stable;
        self.any_connective_used |= other.any_connective_used;
        self.union_locally_free = crate::rust::utils::union(
            std::mem::take(&mut self.union_locally_free),
            other.union_locally_free.clone(),
        );

        self.invalidate_derived();
    }

    /// Append the `Par`s this representation actually owns. Set-mode keys are
    /// canonical bytes in `PathMap<()>`, so they contribute no borrowed value;
    /// map mode contributes only its associated `Par` values.
    pub fn extend_owned_par_refs<'trie>(&'trie self, out: &mut Vec<&'trie Par>) {
        if let EPathMapRepr::Map(map) = &self.repr {
            out.extend(map.iter().map(|(_, value)| value));
        }
    }

    /// The FALLIBLE walk: like [`EntryTrie::for_each_entry`], but the visitor may fail
    /// and the failure short-circuits.
    ///
    /// ★ Exists because `for_each_entry` cannot carry a `?`. A caller that evaluates
    /// each entry — and evaluation can fail — otherwise has no fallible streaming
    /// surface and is tempted to materialize an owned compatibility projection.
    pub fn try_for_each_entry<E>(
        &self,
        mut visit: impl FnMut(&Par) -> Result<(), E>,
    ) -> Result<(), E> {
        let map = match &self.repr {
            EPathMapRepr::Empty => return Ok(()),
            EPathMapRepr::Set(map) => map,
            EPathMapRepr::Map(_) => {
                panic!("set-only entry visitor used on map-mode EPathMap")
            }
        };
        for (key, ()) in map.iter() {
            let par = decode_trie_path(&key)
                .expect("set-mode EPathMap keys are canonical_path encodings");
            let result = visit(&par);
            crate::rust::rholang::par_children::dismantle(par);
            result?;
        }
        Ok(())
    }

    /// The EARLY-EXIT walk: the first entry satisfying `pred`, in trie order.
    ///
    /// ★ Exists because `for_each_entry` cannot `break`. A search expressed through it
    /// would visit every entry after the answer was already known. Returning the
    /// decoded hit by value keeps the trie compressed and avoids retaining a decoded
    /// shadow collection.
    pub fn find_entry(&self, mut pred: impl FnMut(&Par) -> bool) -> Option<Par> {
        let map = match &self.repr {
            EPathMapRepr::Empty => return None,
            EPathMapRepr::Set(map) => map,
            EPathMapRepr::Map(_) => {
                panic!("set-only entry search used on map-mode EPathMap")
            }
        };
        for (key, ()) in map.iter() {
            let par = decode_trie_path(&key)
                .expect("set-mode EPathMap keys are canonical_path encodings");
            if pred(&par) {
                return Some(par);
            }
            crate::rust::rholang::par_children::dismantle(par);
        }
        None
    }

    pub fn for_each_entry(&self, mut visit: impl FnMut(&Par)) {
        let map = match &self.repr {
            EPathMapRepr::Empty => return,
            EPathMapRepr::Set(map) => map,
            EPathMapRepr::Map(_) => {
                panic!("set-only entry visitor used on map-mode EPathMap")
            }
        };
        for (key, ()) in map.iter() {
            let par = decode_trie_path(&key)
                .expect("set-mode EPathMap keys are canonical_path encodings");
            visit(&par);
            crate::rust::rholang::par_children::dismantle(par);
        }
    }

    /// Hand every `Par` this value owns to `out`, **BY MOVE**, leaving none behind.
    ///
    /// ★ For iterative teardown ([`crate::rust::rholang::par_children::dismantle`]). `Drop` for
    /// the `Par` family is itself a Θ(depth) recursive traversal, so a deep term must be taken
    /// apart with an explicit worklist rather than dropped — and the worklist can only do that
    /// if it is handed the `Par`s themselves, not copies of them.
    ///
    /// ⚠ **Exhaustively destructured, with no `..`.** That is the point: a field added to
    /// `EntryTrie` that can hold a `Par` becomes a COMPILE ERROR here rather than a silent leak
    /// back onto the recursive destructor. The wrong form is unspellable.
    ///
    /// There is exactly one `Par` retainer to drain: map-mode PathMap values.
    /// The snapshot/layout caches contain bytes only, and set mode stores unit
    /// values, so neither can reintroduce recursive `Par` destruction.
    /// `PathMap`'s by-move `IntoIterator` is a true move on a uniquely-owned trie and
    /// clones nothing. On a shared root the crate copies-on-write, one
    ///   `<Par as Clone>::clone` per value — but that clone is **converted and stack-flat**
    ///   (`CONVERTED_DEPTH`), so it cannot overflow, and it is exactly what the `.cloned()` this
    ///   replaces paid *unconditionally*. ⇒ never a regression, and strictly better whenever the
    ///   trie is unique.
    pub(crate) fn drain_owned_pars(self, out: &mut Vec<Par>) {
        // ⚠ NO `..` — see the doc above.
        let EntryTrie {
            repr,
            len: _,
            entries_stable: _,
            union_locally_free: _,
            any_connective_used: _,
            trie_snapshot: _,
            epm_layout: _,
        } = self;
        match repr {
            EPathMapRepr::Empty | EPathMapRepr::Set(_) => {}
            EPathMapRepr::Map(map) => out.extend(map.into_iter().map(|(_, value)| value)),
        }
    }

    fn into_raw_set_entries(self, mut visit: impl FnMut(Vec<u8>)) -> Result<(), EPathMapModeError> {
        let EntryTrie {
            repr,
            len: _,
            entries_stable: _,
            union_locally_free: _,
            any_connective_used: _,
            trie_snapshot: _,
            epm_layout: _,
        } = self;
        match repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(map) => {
                for (key, ()) in map {
                    visit(key);
                }
                Ok(())
            }
            EPathMapRepr::Map(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Set,
                actual: EPathMapMode::Map,
            }),
        }
    }

    fn into_raw_map_entries(
        self,
        mut visit: impl FnMut(Vec<u8>, Par),
    ) -> Result<(), EPathMapModeError> {
        let EntryTrie {
            repr,
            len: _,
            entries_stable: _,
            union_locally_free: _,
            any_connective_used: _,
            trie_snapshot: _,
            epm_layout: _,
        } = self;
        match repr {
            EPathMapRepr::Empty => Ok(()),
            EPathMapRepr::Set(_) => Err(EPathMapModeError {
                expected: EPathMapMode::Map,
                actual: EPathMapMode::Set,
            }),
            EPathMapRepr::Map(map) => {
                for (key, value) in map {
                    visit(key, value);
                }
                Ok(())
            }
        }
    }

    /// Remove the entry with the GREATEST key in trie order, and return it.
    ///
    /// ⚠ This is the honest replacement for `ps.pop()`. A `Vec` has a last
    /// element because it has positions; an entry SET does not, so "the last
    /// one" has to be re-derived from the only order that exists — the trie's.
    /// The result is deterministic and construction-order-independent, which
    /// `pop()` was not: `{|a, b|}.pop()` and `{|b, a|}.pop()` used to remove
    /// different entries from what is the same map.
    pub fn remove_greatest_entry(&mut self) -> Option<Par> {
        let last_key = {
            use pathmap::zipper::{ZipperIteration, ZipperMoving};
            let map = match &self.repr {
                EPathMapRepr::Empty => return None,
                EPathMapRepr::Set(map) => map,
                EPathMapRepr::Map(_) => panic!("set removal attempted on map-mode EPathMap"),
            };
            let mut rz = map.read_zipper();
            let mut last: Option<Vec<u8>> = None;
            while rz.to_next_val() {
                last = Some(rz.path().to_vec());
            }
            last?
        };
        let EPathMapRepr::Set(map) = &mut self.repr else {
            unreachable!("the mode was checked above")
        };
        let removed = map.remove(&last_key);
        if removed.is_some() {
            self.len -= 1;
            if map.is_empty() {
                self.repr = EPathMapRepr::Empty;
            }
            self.invalidate_derived();
            // The folds are not invertible, so they are recomputed rather than
            // decremented. `entries_stable` remains exact for canonical-path
            // classification.
            self.recompute_folds();
        }
        removed.map(|()| {
            decode_trie_path(&last_key)
                .expect("EntryTrie keys are canonical_path encodings; insertion validates this")
        })
    }

    pub fn remove_greatest_map_entry(&mut self) -> Result<Option<(Par, Par)>, EPathMapModeError> {
        let last_key = match &self.repr {
            EPathMapRepr::Empty => return Ok(None),
            EPathMapRepr::Set(_) => {
                return Err(EPathMapModeError {
                    expected: EPathMapMode::Map,
                    actual: EPathMapMode::Set,
                });
            }
            EPathMapRepr::Map(map) => map.iter().map(|(key, _)| key).last(),
        };
        let Some(last_key) = last_key else {
            return Ok(None);
        };
        let EPathMapRepr::Map(map) = &mut self.repr else {
            unreachable!("the mode was checked above")
        };
        let value = map
            .remove(&last_key)
            .expect("the selected greatest map key still exists");
        let key = decode_trie_path(&last_key)
            .expect("map-mode EPathMap keys are canonical_path encodings");
        self.len -= 1;
        if map.is_empty() {
            self.repr = EPathMapRepr::Empty;
        }
        self.recompute_folds();
        self.invalidate_derived();
        Ok(Some((key, value)))
    }

    /// Re-derive the entry folds from the (post-removal) trie. Only the removal
    /// path needs this — insertion folds forward.
    fn recompute_folds(&mut self) {
        let mut entries_stable = true;
        let mut any_connective_used = false;
        let mut union_locally_free = Vec::new();
        match &self.repr {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(map) => {
                for (key, ()) in map.iter() {
                    let par = decode_trie_path(&key)
                        .expect("set-mode EPathMap keys are canonical_path encodings");
                    entries_stable &= eval_stable_par(&par);
                    any_connective_used |= par.connective_used;
                    union_locally_free =
                        crate::rust::utils::union(union_locally_free, par.locally_free.clone());
                    crate::rust::rholang::par_children::dismantle(par);
                }
            }
            EPathMapRepr::Map(map) => {
                for (key, value) in map.iter() {
                    let key = decode_trie_path(&key)
                        .expect("map-mode EPathMap keys are canonical_path encodings");
                    for par in [&key, value] {
                        entries_stable &= eval_stable_par(par);
                        any_connective_used |= par.connective_used;
                        union_locally_free =
                            crate::rust::utils::union(union_locally_free, par.locally_free.clone());
                    }
                    crate::rust::rholang::par_children::dismantle(key);
                }
            }
        }
        self.entries_stable = entries_stable;
        self.any_connective_used = any_connective_used;
        self.union_locally_free = union_locally_free;
        self.trie_snapshot = Arc::new(OnceLock::new());
        self.epm_layout = Arc::new(OnceLock::new());
    }
}

impl EntryTrie {
    /// ★ Take over a trie the caller already has, rather than re-filing its
    /// contents — the route back from `RholangSetPathMap` to `EPathMap` that every
    /// pathmap-returning method in the reducer takes.
    ///
    /// # Why it verifies instead of trusting
    ///
    /// Adopting a trie means adopting its KEYS, and a key is only meaningful
    /// while it is `encode_trie_path` of what it decodes to. That is checkable
    /// for the price of one `encode_trie_path` per entry — which is exactly what
    /// re-filing through `EntryTrie::from` would have cost anyway — so the check
    /// is free relative to the alternative and this performs it **in release
    /// builds too**, where `rholang_set_pathmap_to_set_epathmap`'s
    /// `#[cfg(debug_assertions)]` guard is compiled out.
    ///
    /// A trie that fails the check is not rejected; it is **re-filed**, which
    /// normalizes it. So a non-canonical key cannot propagate into a value
    /// either way, and the fast path is taken by every trie the codec built.
    ///
    /// The `val_count` comparison catches the one asymmetry the two trie walks
    /// have: `PathMap::iter()` yields a value stored at the empty (root) key
    /// while `ZipperIteration::to_next_val()` skips it (pinned in
    /// `pathmap_crate_type_mapper::root_key_divergence`). A trie holding one
    /// would be adopted with an entry its own projection cannot see, so it takes
    /// the re-filing path instead.
    pub(crate) fn adopt_trie(map: &RholangSetPathMap) -> EntryTrie {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};

        let mut keys_canonical = true;
        let mut len = 0usize;
        let mut entries_stable = true;
        let mut any_connective_used = false;
        let mut union_locally_free: Vec<u8> = Vec::new();
        {
            let mut rz = map.read_zipper();
            while rz.to_next_val() {
                let par = decode_trie_path(rz.path())
                    .expect("RholangSetPathMap keys must be canonical_path encodings");
                // Re-encoding proves the decoded key is in canonical form. Both
                // directions are stack-safe and accept arbitrary finite depth.
                keys_canonical &= encode_trie_path(&par) == rz.path();
                entries_stable &= eval_stable_par(&par);
                any_connective_used |= par.connective_used;
                union_locally_free =
                    crate::rust::utils::union(union_locally_free, par.locally_free.clone());
                len += 1;
                crate::rust::rholang::par_children::dismantle(par);
            }
        }
        if !keys_canonical || map.val_count() != len {
            // Malformed internal tries are normalized only on the exceptional
            // path. The common path below is an O(1) PathMap root clone and
            // never reconstructs an entry collection.
            let mut normalized = EntryTrie::default();
            let mut rz = map.read_zipper();
            while rz.to_next_val() {
                let par = decode_trie_path(rz.path())
                    .expect("RholangSetPathMap keys must decode before normalization");
                normalized.insert_entry(par);
            }
            return normalized;
        }
        EntryTrie {
            repr: if map.is_empty() {
                EPathMapRepr::Empty
            } else {
                EPathMapRepr::Set(map.clone())
            },
            len,
            entries_stable,
            union_locally_free,
            any_connective_used,
            trie_snapshot: Arc::new(OnceLock::new()),
            epm_layout: Arc::new(OnceLock::new()),
        }
    }
}

impl From<Vec<Par>> for EntryTrie {
    /// THE construction entry: file every element under its own codec path.
    ///
    /// This is where insertion order stops existing. Permutations and
    /// duplicates in the input are absorbed by the insert — the trie is a
    /// function of the entry SET — so `EntryTrie::from(v).view()` is generally
    /// **not** `v`, and that is the point rather than a defect.
    fn from(entries: Vec<Par>) -> Self {
        let mut built = EntryTrie::default();
        for par in entries {
            built.insert_entry(par);
        }
        built
    }
}

impl From<&[Par]> for EntryTrie {
    fn from(entries: &[Par]) -> Self {
        let mut built = EntryTrie::default();
        for par in entries {
            built.insert_entry(par.clone());
        }
        built
    }
}

impl From<&Vec<Par>> for EntryTrie {
    /// Rebuild around a borrowed projection — the shape
    /// `EPathMap::new(other.entries_owned(), …)` takes when a caller wants a map with the
    /// same entries and different metadata.
    fn from(entries: &Vec<Par>) -> Self { EntryTrie::from(entries.as_slice()) }
}

impl Clone for EntryTrie {
    /// O(1) at the trie root: PathMap clone is a refcount bump and both
    /// serialization caches are shared by `Arc`. Only the small `locally_free`
    /// bitset copies.
    fn clone(&self) -> Self {
        EntryTrie {
            repr: self.repr.clone(),
            len: self.len,
            entries_stable: self.entries_stable,
            union_locally_free: self.union_locally_free.clone(),
            any_connective_used: self.any_connective_used,
            trie_snapshot: Arc::clone(&self.trie_snapshot),
            epm_layout: Arc::clone(&self.epm_layout),
        }
    }
}

impl Default for EntryTrie {
    /// The empty entry set. `entries_stable` starts `true` — it is a
    /// `for all` over no entries.
    fn default() -> Self {
        EntryTrie {
            repr: EPathMapRepr::Empty,
            len: 0,
            entries_stable: true,
            union_locally_free: Vec::new(),
            any_connective_used: false,
            trie_snapshot: Arc::new(OnceLock::new()),
            epm_layout: Arc::new(OnceLock::new()),
        }
    }
}

impl fmt::Debug for EntryTrie {
    /// O(1) trie-native diagnostics. Debug must not force or dump a potentially
    /// large EPM1 snapshot merely to describe a schema node.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntryTrie")
            .field("mode", &self.mode())
            .field("len", &self.len)
            .field("snapshot_cached", &self.trie_snapshot.get().is_some())
            .finish()
    }
}

#[cfg(test)]
impl EntryTrie {
    /// Entry equality, read directly through the PathMap. Set mode compares
    /// keys; map mode compares each ordered `(key, value)` pair. Neither path
    /// materializes a projection or forces the EPM1 serialization cache.
    ///
    /// ★ C8, owner-ruled a REPAIR rather than a semantic change. This used to compare
    /// `self.view() == other.view()`, i.e. the projected entries under `Par`'s
    /// **AlwaysEqual** `==`, which IGNORES `locally_free`. But entries are KEYED by
    /// `encode_trie_path`, whose escape arm is the entry's canonical protobuf bytes —
    /// which INCLUDE `locally_free`. So two maps could compare EQUAL while holding
    /// different key sets, hence different EPM1 snapshots and emitted bytes.
    /// `==` was strictly coarser than the relation consensus commits to.
    ///
    /// The old doc named this exact hazard and dismissed it: *"In a well-formed term
    /// `locally_free` is a function of the structure, so the two cannot differ; the note
    /// is here because 'cannot' should be written down."* ⚠ That is a claim about who
    /// the callers are, not about what the type permits — the same shape that let
    /// `contains_par`'s memo ship unsound earlier in this campaign. Comparing keys makes
    /// it unrepresentable instead of unlikely.
    ///
    /// ⇒ Now a paired zipper walk over the two key streams. It is also strictly cheaper:
    /// the old form forced BOTH projections, deep-cloning every entry on each side, to
    /// answer a question the tries could answer by walking.
    fn recursive_eq_oracle(&self, other: &Self) -> bool {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        if self.len != other.len {
            return false;
        }
        let (a_map, b_map) = match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, EPathMapRepr::Empty) => return true,
            (EPathMapRepr::Set(a), EPathMapRepr::Set(b)) => (a, b),
            (EPathMapRepr::Map(a), EPathMapRepr::Map(b)) => {
                if !a
                    .read_zipper()
                    .into_path_iter()
                    .eq(b.read_zipper().into_path_iter())
                {
                    return false;
                }
                let mut a = a.iter();
                let mut b = b.iter();
                loop {
                    match (a.next(), b.next()) {
                        (None, None) => return true,
                        (Some((a_key, a_value)), Some((b_key, b_value))) => {
                            if a_key != b_key || a_value != b_value {
                                return false;
                            }
                        }
                        (None, Some(_)) | (Some(_), None) => return false,
                    }
                }
            }
            (EPathMapRepr::Empty, EPathMapRepr::Set(_))
            | (EPathMapRepr::Empty, EPathMapRepr::Map(_))
            | (EPathMapRepr::Set(_), EPathMapRepr::Empty)
            | (EPathMapRepr::Set(_), EPathMapRepr::Map(_))
            | (EPathMapRepr::Map(_), EPathMapRepr::Empty)
            | (EPathMapRepr::Map(_), EPathMapRepr::Set(_)) => return false,
        };
        if !a_map
            .read_zipper()
            .into_path_iter()
            .eq(b_map.read_zipper().into_path_iter())
        {
            return false;
        }
        let mut a = a_map.read_zipper();
        let mut b = b_map.read_zipper();
        loop {
            // ⚠ NO `_` arm. `variant_exhaustiveness_gate` refuses a catch-all in a
            // comparison impl, and it is right to: a catch-all answering `false` makes
            // the match exhaustive to the compiler, which disables the only check that a
            // newly-reachable case gets a deliberate answer. Enumerated, the three
            // not-equal shapes are visibly the three that should be not-equal.
            match (a.to_next_val(), b.to_next_val()) {
                // Both streams ended together: every key agreed.
                (false, false) => return true,
                // Both produced a key: equal iff the keys are.
                (true, true) => {
                    if a.path() != b.path() {
                        return false;
                    }
                    continue;
                }
                // One ended first — different lengths, and `len` should already
                // have caught it above. Kept because "cannot happen" is a claim
                // about callers, not about what the tries permit.
                (false, true) | (true, false) => return false,
            }
        }
    }
}

#[cfg(test)]
impl EntryTrie {
    /// Consistent with [`PartialEq`]: the same key stream (and map values),
    /// hashed in trie order without forcing EPM1 serialization.
    ///
    /// ⚠ `Hash` must agree with `==` or a `HashMap` keyed on this type silently loses
    /// entries, so this moved with `eq` and could not have moved separately.
    ///
    /// ★ Changing it is byte-neutral, and the reason is a repair landed earlier in this
    /// campaign rather than an argument about callers. `Hash` reaches emitted bytes only
    /// through `HashSet<Par>`/`HashMap<Par,Par>` iteration order in
    /// `SortedParHashSet::create_from_vec` and `SortedParMap`, and BOTH funnel into
    /// `ScoredTerm::sort_vec` — which `SS-Y4` made a TOTAL order. A total sort's output
    /// does not depend on its input order, so hash iteration order cannot reach a byte.
    /// Before that repair this change would have needed a seven-axis entry.
    fn recursive_hash_oracle<H: Hasher>(&self, state: &mut H) {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        match &self.repr {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(map) => {
                for path in map.read_zipper().into_path_iter() {
                    path.hash(state);
                }
                let mut rz = map.read_zipper();
                while rz.to_next_val() {
                    rz.path().hash(state);
                }
            }
            EPathMapRepr::Map(map) => {
                for path in map.read_zipper().into_path_iter() {
                    path.hash(state);
                }
                for (key, value) in map.iter() {
                    key.hash(state);
                    value.hash(state);
                }
            }
        }
    }
}

#[cfg(test)]
impl EntryTrie {
    /// Lexicographic over the key stream, then map values, in trie order — the
    /// same relation [`PartialEq`] and [`Hash`] read. This walks PathMap
    /// directly and leaves the EPM1 serialization cache cold.
    ///
    /// ★ This finishes C8. That commit moved `==` and `Hash` off the projected
    /// list and onto the keys, *"the relation the wire commits to"*, and left
    /// `Ord` behind on `self.view().cmp(other.view())`. So equality and hashing
    /// asked the trie while ordering asked a flattened list, and `Ord` reaches
    /// emitted bytes through `ScoredTerm::sort_vec`. Two orders for one value is
    /// exactly the defect C8 was written to kill, surviving in the one impl the
    /// commit did not enumerate.
    ///
    /// ⇒ One relation family, three impls, one byte stream. A map cannot now
    /// compare `Equal` to a map it is not `==` to, because both questions are
    /// answered by the same paired walk.
    ///
    /// # Why the walk and not `path_stream().cmp(...)`
    ///
    /// Comparing the cached snapshot byte-for-byte would be O(1) after a cache
    /// hit and would read literally the wire's bytes — tempting, and rejected:
    /// EPM1 orders first by its header and ACTree03's child-before-parent arena
    /// layout, not by the trie's byte-lexicographic key order. That is a valid
    /// total order but an arbitrary serialization artifact. The walk costs what
    /// equality already costs and orders by the structure that has semantic
    /// meaning.
    fn recursive_cmp_oracle(&self, other: &Self) -> Ordering {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        let (a_map, b_map) = match (&self.repr, &other.repr) {
            (EPathMapRepr::Empty, EPathMapRepr::Empty) => return Ordering::Equal,
            (EPathMapRepr::Set(a), EPathMapRepr::Set(b)) => (a, b),
            (EPathMapRepr::Map(a), EPathMapRepr::Map(b)) => {
                let topology = a
                    .read_zipper()
                    .into_path_iter()
                    .cmp(b.read_zipper().into_path_iter());
                if topology != Ordering::Equal {
                    return topology;
                }
                let mut a = a.iter();
                let mut b = b.iter();
                loop {
                    match (a.next(), b.next()) {
                        (None, None) => return Ordering::Equal,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (Some((a_key, a_value)), Some((b_key, b_value))) => {
                            match a_key.cmp(&b_key) {
                                Ordering::Equal => match a_value.cmp(b_value) {
                                    Ordering::Equal => {}
                                    decided => return decided,
                                },
                                decided => return decided,
                            }
                        }
                    }
                }
            }
            _ => return (self.mode() as u8).cmp(&(other.mode() as u8)),
        };
        let topology = a_map
            .read_zipper()
            .into_path_iter()
            .cmp(b_map.read_zipper().into_path_iter());
        if topology != Ordering::Equal {
            return topology;
        }
        let mut a = a_map.read_zipper();
        let mut b = b_map.read_zipper();
        loop {
            match (a.to_next_val(), b.to_next_val()) {
                // Both exhausted at the same position: every key agreed.
                (false, false) => return Ordering::Equal,
                // A proper prefix of the other's key sequence sorts first.
                (false, true) => return Ordering::Less,
                (true, false) => return Ordering::Greater,
                (true, true) => match a.path().cmp(b.path()) {
                    Ordering::Equal => continue,
                    decided => return decided,
                },
            }
        }
    }
}

impl serde::Serialize for EntryTrie {
    /// Serialize exactly one canonical, versioned EPM1 byte array. EPM1 embeds
    /// PathMap's `ACTree03` arena and, in map mode, a stack-safely encoded value
    /// table. No entry list or key/value shadow representation is constructed.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.trie_snapshot())
    }
}

impl<'de> serde::Deserialize<'de> for EntryTrie {
    /// Decode one canonical EPM1 byte array into its homogeneous PathMap
    /// specialization. Both trie validation and map-value decoding are
    /// iterative and impose no traversal-depth ceiling.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let snapshot = Vec::<u8>::deserialize(deserializer)?;
        let mut map = EPathMap::default();
        map.replace_trie_snapshot(&snapshot)
            .map_err(serde::de::Error::custom)?;
        Ok(map.ps)
    }
}

/// Hand-maintained external type for `message EPathMap`.
///
/// Entries are stored only as a homogeneous, prefix-compressed PathMap:
/// neutral empty, `PathMap<()>` set mode, or `PathMap<Par>` map mode. Use
/// [`EPathMap::new`] or [`EPathMap::new_map`] rather than struct literals.
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct EPathMap {
    /// ★ The entries, stored as the [`EntryTrie`] they are indexed by.
    ///
    /// **PRIVATE**, and that is load-bearing rather than tidy: the field is no
    /// longer a `Vec<Par>`, so callers read the trie through
    /// [`EPathMap::entry_trie`] and mutate it only through the mode-specific
    /// operations. `ps_make_mut` no longer exists — **the compiler is the
    /// fixture** for anything that used to hand out `&mut Vec<Par>`.
    ///
    /// Serde field `ps` is a byte array containing the EPM1 snapshot.
    #[schema(value_type = Vec<u8>)]
    ps: EntryTrie,
    /// Free-variable bitset (proto tag 3, bytes). Serde serializes this as
    /// EMPTY bytes (serialize-only normalization, `models/build.rs` parity);
    /// prost bytes RETAIN it; `==`/`Hash` ignore it; `Ord` compares it. The
    /// serialize-only blanking now lives in the hand-written `Serialize` impl
    /// below (the `serialize_with` attribute was dropped when the derive was);
    /// the derived `Deserialize` still reads REAL bytes from the stream.
    pub locally_free: Vec<u8>,
    /// Whether a connective is used below (proto tag 4, bool).
    pub connective_used: bool,
    /// Pattern remainder (proto tag 5, optional `Var`).
    pub remainder: Option<Var>,
}

impl serde::Serialize for EPathMap {
    /// Serialize the EPM1 byte snapshot plus the three metadata fields.
    /// `locally_free` remains serialize-asymmetric and is emitted as empty bytes
    /// for compatibility with the generated schema normalization.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        /// The serialize-only `locally_free` normalization: EMPTY bytes
        /// regardless of content (byte-identical to
        /// `serde_helpers::serialize_as_empty_bytes` — `serialize_bytes(&[])`).
        struct EmptyBytes;
        impl serde::Serialize for EmptyBytes {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(&[])
            }
        }

        let mut state = serializer.serialize_struct("EPathMap", 4)?;
        state.serialize_field("ps", &self.ps)?;
        state.serialize_field("locally_free", &EmptyBytes)?;
        state.serialize_field("connective_used", &self.connective_used)?;
        state.serialize_field("remainder", &self.remainder)?;
        state.end()
    }
}

impl EPathMap {
    /// Replace the entry representation from one canonical `EPM1` snapshot.
    /// This is the sole field-9 interpretation shared by both protobuf readers.
    #[allow(deprecated)]
    pub(crate) fn replace_trie_snapshot(&mut self, region: &[u8]) -> Result<(), DecodeError> {
        let repr = epathmap_trie_codec::decode(region)
            .map_err(|error| DecodeError::new(format!("EPathMap trie_snapshot: {error}")))?;
        self.replace_decoded_representation(repr)
    }

    /// Install a representation already validated by the pausable EPM1
    /// decoder. This is the generated protobuf PDA's non-recursive completion
    /// edge; it deliberately performs no second byte decode.
    #[allow(deprecated)]
    pub(crate) fn replace_decoded_representation(
        &mut self,
        repr: EPathMapRepr<Par>,
    ) -> Result<(), DecodeError> {
        let current = self.mode();
        let incoming = repr.mode();
        if current != EPathMapMode::Empty && incoming != EPathMapMode::Empty && current != incoming
        {
            return Err(DecodeError::new(format!(
                "EPathMap trie_snapshot cannot change {:?} storage to {:?} storage",
                current, incoming
            )));
        }
        self.ps = EntryTrie::from_repr(repr)?;
        Ok(())
    }

    /// Merge the retired protobuf `serialized_paths` compatibility field into
    /// set-mode storage.
    ///
    /// The protobuf reader and the hand-maintained `Message` implementation
    /// share this one interpretation of `U(m)`.  Keeping the field-envelope
    /// read outside this method lets the generated reader consume the bytes on
    /// its own PDA without delegating back to `Message::merge_field`.
    #[allow(deprecated)]
    pub(crate) fn merge_serialized_paths(&mut self, region: &[u8]) -> Result<(), DecodeError> {
        if self.mode() == EPathMapMode::Map {
            return Err(DecodeError::new(
                "EPathMap serialized_paths cannot be merged into map-mode storage",
            ));
        }
        let mut entries = EntryTrie::default();
        for frame in PathFrames::new(region) {
            let key = frame.map_err(|fault| match fault {
                PathFrameError::LengthOverflow { .. } => {
                    DecodeError::new("EPathMap serialized_paths: key length overflow")
                }
                PathFrameError::TruncatedKey { .. } => {
                    DecodeError::new("EPathMap serialized_paths: truncated key")
                }
                PathFrameError::TruncatedLength { .. } => DecodeError::new(
                    "EPathMap serialized_paths: trailing bytes after the final key",
                ),
            })?;
            let par = decode_trie_path(key).map_err(|codec_error| {
                DecodeError::new(format!("EPathMap serialized_paths key: {codec_error:?}"))
            })?;
            entries.insert_entry(par);
        }
        self.ps.extend_entries(&entries);
        Ok(())
    }

    /// Construct set-mode storage. An empty input stays mode-neutral. Entries
    /// are inserted under their canonical byte paths, so order and duplicates
    /// cannot become retained state.
    pub fn new(
        ps: impl Into<EntryTrie>,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<Var>,
    ) -> Self {
        EPathMap {
            ps: ps.into(),
            locally_free,
            connective_used,
            remainder,
        }
    }

    /// Construct the value-bearing specialization. An empty iterator remains
    /// mode-neutral; its first later insertion selects map or set mode.
    pub fn new_map(
        entries: impl IntoIterator<Item = (Par, Par)>,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<Var>,
    ) -> Self {
        let mut ps = EntryTrie::default();
        for (key, value) in entries {
            ps.insert_map_entry(key, value)
                .expect("a fresh map constructor cannot contain set entries");
        }
        EPathMap {
            ps,
            locally_free,
            connective_used,
            remainder,
        }
    }

    /// Hand every `Par` this map owns to `out`, **BY MOVE**.
    ///
    /// ★ Delegates to [`EntryTrie::drain_owned_pars`]; see that method for why the destructure
    /// is exhaustive and how the two retainers are drained.
    pub(crate) fn drain_owned_pars(self, out: &mut Vec<Par>) {
        // ⚠ NO `..` — a field added here that can hold a `Par` must fail to compile.
        let EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        } = self;

        ps.drain_owned_pars(out);
    }

    /// Consume set-mode storage through PathMap's owned zipper, visiting each
    /// canonical byte key in trie order without decoding an entry projection.
    pub fn into_raw_set_entries(self, visit: impl FnMut(Vec<u8>)) -> Result<(), EPathMapModeError> {
        let EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        } = self;
        ps.into_raw_set_entries(visit)
    }

    /// Consume map-mode storage through PathMap's owned zipper, moving each
    /// canonical byte key and associated value in trie order without cloning.
    pub fn into_raw_map_entries(
        self,
        visit: impl FnMut(Vec<u8>, Par),
    ) -> Result<(), EPathMapModeError> {
        let EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        } = self;
        ps.into_raw_map_entries(visit)
    }

    /// Number of PathMap-owned `Par` value retainers used by teardown tests.
    /// Set mode owns byte keys only; map mode owns one `Par` per trie value.
    #[cfg(test)]
    pub(crate) fn live_retainer_count(&self) -> usize {
        match self.ps.representation() {
            EPathMapRepr::Empty => 0,
            EPathMapRepr::Set(_) => 0,
            EPathMapRepr::Map(_) => 1,
        }
    }

    /// The entry store itself — the trie every consumer used to rebuild.
    /// `clone()`ing it is a refcount bump.
    pub fn entry_trie(&self) -> &EntryTrie { &self.ps }

    pub fn representation(&self) -> &EPathMapRepr<Par> { self.ps.representation() }

    pub fn mode(&self) -> EPathMapMode { self.ps.mode() }

    /// Number of distinct trie entries. This is maintained with the PathMap
    /// root and does not materialize the legacy `Vec<Par>` projection.
    pub fn len(&self) -> usize { self.ps.len() }

    /// Whether the neutral/set/map trie contains no entries.
    pub fn is_empty(&self) -> bool { self.ps.is_empty() }

    /// Insert one set member. Idempotent; an existing canonical key is absorbed.
    pub fn insert_entry(&mut self, par: Par) { self.ps.insert_entry(par); }

    pub fn try_insert_entry(&mut self, par: Par) -> Result<(), EPathMapModeError> {
        self.ps.try_insert_entry(par)
    }

    pub fn insert_map_entry(&mut self, key: Par, value: Par) -> Result<(), EPathMapModeError> {
        self.ps.insert_map_entry(key, value)
    }

    pub fn insert_map_value_by_encoded_key(
        &mut self,
        encoded_key: &[u8],
        value: Par,
    ) -> Result<(), EPathMapModeError> {
        self.ps.insert_map_value_by_encoded_key(encoded_key, value)
    }

    pub fn get_map_value(&self, key: &Par) -> Result<Option<&Par>, EPathMapModeError> {
        self.ps.get_map_value(key)
    }

    pub fn for_each_map_entry(
        &self,
        visit: impl FnMut(&Par, &Par),
    ) -> Result<(), EPathMapModeError> {
        self.ps.for_each_map_entry(visit)
    }

    pub fn cursor_entry_key(&self, segments: &[Vec<u8>], kind: CursorKind) -> Vec<u8> {
        self.ps.cursor_entry_key(segments, kind)
    }

    pub fn entry_key_at(&self, cursor: &[Vec<u8>], path: &Par) -> Vec<u8> {
        self.ps.entry_key_at(cursor, path)
    }

    pub fn leaf_at_encoded_key(&self, key: &[u8]) -> Option<Par> {
        self.ps.leaf_at_encoded_key(key)
    }

    pub fn contains_encoded_key(&self, key: &[u8]) -> bool { self.ps.contains_encoded_key(key) }

    pub fn contains_entry(&self, entry: &Par) -> bool { self.ps.contains_entry(entry) }

    pub fn path_prefix_exists(&self, prefix: &[u8]) -> bool { self.ps.path_prefix_exists(prefix) }

    pub fn collect_child_segments(&self, prefix: &[u8], limit: Option<usize>) -> Vec<Vec<u8>> {
        self.ps.collect_child_segments(prefix, limit)
    }

    pub fn subtrie_value_count(&self, prefix: &[u8]) -> usize {
        self.ps.subtrie_value_count(prefix)
    }

    pub fn next_value_key(&self, from_key: &[u8]) -> Option<Vec<u8>> {
        self.ps.next_value_key(from_key)
    }

    pub fn subtrie(&self, prefix: &[u8]) -> Self {
        EPathMap::new(
            self.ps.subtrie(prefix),
            self.locally_free.clone(),
            self.connective_used,
            None,
        )
    }

    pub fn drop_head(&self, count: usize) -> Self {
        EPathMap::new(
            self.ps.drop_head(count),
            self.locally_free.clone(),
            self.connective_used,
            None,
        )
    }

    pub fn remove_encoded_entry(&mut self, key: &[u8]) -> bool { self.ps.remove_encoded_entry(key) }

    pub fn remove_branches_at(&mut self, prefix: &[u8]) -> bool {
        self.ps.remove_branches_at(prefix)
    }

    pub fn remove_subtrie_at(&mut self, prefix: &[u8]) -> bool { self.ps.remove_subtrie_at(prefix) }

    pub fn create_path(&mut self, path: &[u8]) -> Result<bool, EPathMapEmptyModeError> {
        self.ps.create_path(path)
    }

    pub fn replace_subtrie(
        &mut self,
        cursor_segments: &[Vec<u8>],
        cursor_kind: CursorKind,
        source: &EPathMap,
    ) -> Result<(), EPathMapAlgebraError> {
        self.ps
            .replace_subtrie(cursor_segments, cursor_kind, &source.ps)
    }

    #[allow(deprecated)]
    pub(crate) fn try_insert_entry_replacing(&mut self, par: Par) -> Result<Par, DecodeError> {
        self.ps
            .try_insert_entry_replacing(par)
            .map_err(|error| DecodeError::new(format!("EPathMap ps: {error}")))
    }

    /// Add every entry of `other` — the set union `graft` performs. Replaces
    /// `ps_make_mut().extend(other.ps.into_vec())`.
    pub fn extend_entries(&mut self, other: &EPathMap) { self.ps.extend_entries(&other.ps); }

    /// Remove the entry with the greatest key in trie order. Replaces
    /// `ps_make_mut().pop()` at `removeLeaf`; see
    /// [`EntryTrie::remove_greatest_entry`] for why "the last one" has to be
    /// re-derived from the trie's order rather than from a position.
    pub fn remove_greatest_entry(&mut self) -> Option<Par> { self.ps.remove_greatest_entry() }

    pub fn remove_greatest_map_entry(&mut self) -> Result<Option<(Par, Par)>, EPathMapModeError> {
        self.ps.remove_greatest_map_entry()
    }

    /// Canonical EPM1/ACTree03 bytes. Protobuf field 9 and bincode copy this
    /// slice directly; no set-entry or key/value vector is materialized.
    pub fn trie_snapshot(&self) -> &[u8] { self.ps.trie_snapshot() }
}

impl Default for EPathMap {
    /// Neutral empty representation with default metadata.
    fn default() -> Self { EPathMap::new(Vec::new(), Vec::new(), false, None) }
}

impl Clone for EPathMap {
    /// Clone the PathMap roots and shared snapshot/layout cells by refcount.
    /// Only metadata bytes and the small optional remainder are copied.
    fn clone(&self) -> Self {
        EPathMap {
            ps: self.ps.clone(),
            locally_free: self.locally_free.clone(),
            connective_used: self.connective_used,
            remainder: self.remainder.clone(),
        }
    }
}

impl fmt::Debug for EPathMap {
    /// Trie-native diagnostics.  `ps` delegates to [`EntryTrie`]'s compact
    /// snapshot rendering instead of materialising or recursively formatting a
    /// `Vec<Par>`.  This is an intentional correction to the former generated
    /// shape: an EPathMap is a prefix-compressed set/map, not a list, and its
    /// Debug surface must not restore the discarded list model.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EPathMap {{ ps: {:?}, locally_free: {:?}, connective_used: {:?}, remainder: {:?} }}",
            self.ps, self.locally_free, self.connective_used, self.remainder,
        )
    }
}

impl prost::Message for EPathMap {
    /// Emit protobuf field 9 from EPM1 through the generated stack-safe encoder.
    fn encode_raw(&self, buf: &mut impl BufMut) {
        crate::rust::rholang::protobuf_encoder::encode_into(self, buf);
    }

    // `DecodeError::new` is prost's only public constructor for a custom
    // decode error (it is `doc(hidden)` + deprecation-warned but not yet
    // removed); the tag-8 validation arm needs it.
    #[allow(deprecated)]
    fn merge_field(
        &mut self,
        tag: u32,
        protobuf_wire_type: WireType,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        /// prost-derive parity: the error-context struct name pushed onto
        /// `DecodeError` paths.
        const STRUCT_NAME: &str = "EPathMap";
        match tag {
            1u32 => {
                let mut payload = Vec::new();
                encoding::bytes::merge(protobuf_wire_type, &mut payload, buf, ctx).map_err(
                    |mut error| {
                        error.push(STRUCT_NAME, "ps");
                        error
                    },
                )?;
                let par = crate::rust::rholang::protobuf_decoder::decode_par(payload.as_slice())
                    .map_err(|mut error| {
                        error.push(STRUCT_NAME, "ps");
                        error
                    })?;
                self.ps
                    .try_insert_entry(par)
                    .map_err(|error| DecodeError::new(format!("EPathMap ps: {error}")))?;
                Ok(())
            }
            3u32 => {
                let value = &mut self.locally_free;
                encoding::bytes::merge(protobuf_wire_type, value, buf, ctx).map_err(|mut error| {
                    error.push(STRUCT_NAME, "locally_free");
                    error
                })
            }
            4u32 => {
                let value = &mut self.connective_used;
                encoding::bool::merge(protobuf_wire_type, value, buf, ctx).map_err(|mut error| {
                    error.push(STRUCT_NAME, "connective_used");
                    error
                })
            }
            5u32 => {
                let value = &mut self.remainder;
                encoding::message::merge(
                    protobuf_wire_type,
                    value.get_or_insert_with(Default::default),
                    buf,
                    ctx,
                )
                .map_err(|mut error| {
                    error.push(STRUCT_NAME, "remainder");
                    error
                })
            }
            8u32 => {
                // Legacy set-only key stream retained for backward reads.
                let mut region: Vec<u8> = Vec::new();
                encoding::bytes::merge(protobuf_wire_type, &mut region, buf, ctx).map_err(
                    |mut error| {
                        error.push(STRUCT_NAME, "serialized_paths");
                        error
                    },
                )?;
                self.merge_serialized_paths(&region)
            }
            9u32 => {
                let mut region: Vec<u8> = Vec::new();
                encoding::bytes::merge(protobuf_wire_type, &mut region, buf, ctx).map_err(
                    |mut error| {
                        error.push(STRUCT_NAME, "trie_snapshot");
                        error
                    },
                )?;
                self.replace_trie_snapshot(&region)
            }
            _ => crate::rust::rholang::protobuf_decoder::skip_unknown_field(
                protobuf_wire_type,
                tag,
                buf,
            ),
        }
    }

    #[inline]
    fn encoded_len(&self) -> usize { crate::rust::rholang::protobuf_encoder::encoded_len(self) }

    fn clear(&mut self) {
        // Replace the representation so shared clone-family caches stay immutable.
        self.ps = EntryTrie::default();
        self.locally_free.clear();
        self.connective_used = false;
        self.remainder = None;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AlwaysEqual PartialEq/Hash — MOVED from models/src/lib.rs:613-627
// (see models/src/main/scala/coop/rchain/models/AlwaysEqual.scala)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
impl EPathMap {
    /// AlwaysEqual semantics: `locally_free` is a transient analysis field
    /// and does NOT participate (scalapb `AlwaysEqual[BitSet]` parity). The
    /// serialization caches do not participate either (they are derived state).
    ///
    /// # ★ The two-arm relation is GONE — there is one comparison again
    ///
    /// This impl used to be a disjunction of two equivalences: a positional
    /// `Vec` comparison, then a `U(m)` stream comparison for maps whose entries
    /// were in the codec's ground domain. The second arm existed because the
    /// stored order was a producer's, not the trie's, so `==` had to go and read
    /// the trie's order somewhere else to avoid disagreeing with the wire. The
    /// arm-splitting brought its own hazard — the entry/metadata split in
    /// `entries_in_ground_domain` existed solely to stop the two arms from
    /// making `==` non-transitive.
    ///
    /// `self.ps` is now the trie itself: already construction-order independent,
    /// deduplicated, and recursively canonical. The generated PDA compares that
    /// trie directly, so the second arm has nothing left to add and
    /// `entries_in_ground_domain` is deleted with it. Defect #83 is not merely
    /// checked here; it is unrepresentable for every map.
    ///
    /// ⚠ One consequence, stated rather than buried: entries are keyed by
    /// `encode_trie_path`, whose escape arm is the entry's **canonical protobuf
    /// bytes**, which INCLUDE `locally_free`. Two entries that are AlwaysEqual
    /// but differ in `locally_free` are therefore distinct trie keys. That is
    /// the same discipline the intern store's K2 verify already applies (*"the
    /// generated AlwaysEqual `==`/`Hash` IGNORE `locally_free` and are therefore
    /// UNUSABLE for keying"*), now applied by the map itself. In a well-formed
    /// term `locally_free` is maintained from structure; comparing keys makes
    /// malformed disagreements explicit instead of assuming that invariant.
    fn recursive_eq_oracle(&self, other: &Self) -> bool {
        self.connective_used == other.connective_used
            && self.remainder == other.remainder
            && self.ps.recursive_eq_oracle(&other.ps)
    }
}

#[cfg(test)]
impl EPathMap {
    /// AlwaysEqual semantics: consistent with `==` (`locally_free` and the
    /// caches excluded). Equality and hashing now read the same trie stream, so
    /// the hash is a function of the entry set/map and cannot leak construction
    /// order.
    fn recursive_hash_oracle<H: Hasher>(&self, state: &mut H) {
        self.ps.recursive_hash_oracle(state);
        self.connective_used.hash(state);
        self.remainder.hash(state);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Derived-Ord replica — declaration order INCLUDING locally_free (the wart)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
impl EPathMap {
    /// Replica of the derived `Ord` the generated struct carried
    /// (`#[derive(Eq, Ord, PartialOrd)]` via `models/build.rs`):
    /// lexicographic over the fields IN DECLARATION ORDER —
    /// `ps`, then `locally_free`, then `connective_used`, then `remainder`.
    ///
    /// # ★ The `locally_free` wart is KEPT — and it is now the ONLY disagreement
    ///
    /// `locally_free` IS compared here although `==` ignores it, so two maps can
    /// be `==` yet `cmp` to `Less`. That inconsistency is pinned 84a0fbe4
    /// behavior (the P0 `Ord` fixtures + the wrapper wart test) and it is
    /// deliberately untouched: "fixing" it would move sort orders
    /// in canonical ordering for a reason unrelated to this change.
    ///
    /// What DID move is the other disagreement, the one nobody chose. Before the
    /// trie became the field, `cmp` read `ps` in the producer's order while `==`
    /// read the canonical trie stream, so `Ord` and `Eq` disagreed **twice**: once about
    /// `locally_free` (deliberately) and once about entry order (accidentally,
    /// and in a way that made two maps the wire calls identical sort apart).
    /// Both now read the same canonical trie order, so exactly one deliberate
    /// inconsistency remains and the accidental one is gone.
    ///
    /// ⚠ Consensus consequence, stated plainly: for NON-ground maps this moves
    /// sort order, because the entries `cmp` walks are now in trie order rather
    /// than construction order. This is a consensus-visible ordering change.
    ///
    /// Note that `Par: Ord` is the DERIVED one and includes each entry's own
    /// `locally_free`; the wart is hereditary, and that too is unchanged.
    fn recursive_cmp_oracle(&self, other: &Self) -> Ordering {
        self.ps
            .recursive_cmp_oracle(&other.ps)
            .then_with(|| self.locally_free.cmp(&other.locally_free))
            .then_with(|| self.connective_used.cmp(&other.connective_used))
            .then_with(|| self.remainder.cmp(&other.remainder))
    }
}

#[cfg(test)]
mod pathmap_native_semantics_tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::Expr;

    fn int(value: i64) -> Par {
        let mut par = Par::default();
        par.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        });
        par
    }

    fn hash_of(value: &EPathMap) -> u64 {
        let mut state = DefaultHasher::new();
        value.hash(&mut state);
        state.finish()
    }

    fn oracle_hash_of(value: &EPathMap) -> u64 {
        let mut state = DefaultHasher::new();
        value.recursive_hash_oracle(&mut state);
        state.finish()
    }

    fn epathmap_carrier(map: EPathMap) -> Par {
        let mut par = Par::default();
        par.exprs.push(Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        });
        par
    }

    #[test]
    fn map_semantics_walk_pathmap_without_forcing_the_serialization_snapshot() {
        let left = EPathMap::new_map(
            [(int(1), int(10)), (int(2), int(20))],
            Vec::new(),
            false,
            None,
        );
        let equal = left.clone();
        let different = EPathMap::new_map(
            [(int(1), int(10)), (int(2), int(21))],
            Vec::new(),
            false,
            None,
        );

        assert_eq!(left, equal);
        assert_ne!(left, different);
        assert_eq!(left.cmp(&equal), Ordering::Equal);
        assert_ne!(left.cmp(&different), Ordering::Equal);
        assert_eq!(hash_of(&left), hash_of(&equal));

        assert!(!left.ps.snapshot_is_forced());
        assert!(!equal.ps.snapshot_is_forced());
        assert!(!different.ps.snapshot_is_forced());
    }

    #[test]
    fn generated_epathmap_term_operations_match_the_recursive_oracles() {
        let cases = [
            EPathMap::default(),
            EPathMap::new(vec![int(1), int(2)], Vec::new(), false, None),
            EPathMap::new_map(
                [(int(1), int(10)), (int(2), int(20))],
                Vec::new(),
                false,
                None,
            ),
            EPathMap::new_map(
                [(
                    int(1),
                    epathmap_carrier(EPathMap::new_map(
                        [(int(2), int(30))],
                        Vec::new(),
                        false,
                        None,
                    )),
                )],
                vec![1],
                true,
                None,
            ),
        ];

        for left in &cases {
            assert_eq!(hash_of(left), oracle_hash_of(left));
            for right in &cases {
                assert_eq!(left == right, left.recursive_eq_oracle(right));
                assert_eq!(left.cmp(right), left.recursive_cmp_oracle(right));
            }
        }
    }

    #[test]
    fn forcing_one_nested_map_snapshot_does_not_cache_every_suffix() {
        const DEPTH: usize = 1_024;
        let mut value = int(0);
        for _ in 1..DEPTH {
            value = epathmap_carrier(EPathMap::new_map(
                [(int(1), value)],
                Vec::new(),
                false,
                None,
            ));
        }
        let root = EPathMap::new_map([(int(1), value)], Vec::new(), false, None);

        assert!(!root.ps.snapshot_is_forced());
        let _ = root.trie_snapshot();
        assert!(root.ps.snapshot_is_forced());

        let key = int(1);
        let mut cursor = &root;
        for level in 1..DEPTH {
            let value = cursor
                .get_map_value(&key)
                .expect("the chain remains in map mode")
                .expect("each chain map has one value");
            let Some(ExprInstance::EPathmapBody(nested)) = value
                .exprs
                .first()
                .and_then(|expr| expr.expr_instance.as_ref())
            else {
                panic!("map-value chain ended at level {level} of {DEPTH}")
            };
            assert!(
                !nested.ps.snapshot_is_forced(),
                "streaming the root must not retain the complete snapshot of nested level {level}"
            );
            cursor = nested;
        }
    }
}
