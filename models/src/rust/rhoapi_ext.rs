//! The hand-maintained `EPathMap` wrapper, whose entries are an [`EntryTrie`].
//!
//! Lineage: the P3 shadow-cell wrapper (full T1, stage L1.5), then stage L2's
//! shared-`ps` representation (`SharedPars = Arc<Vec<Par>>`, USER decision D2 of
//! 2026-07-20 on the E-6d #2 evidence — clone-class 29.76% flat at ≈44.8 ms/inj
//! across e6d1→e6d2, attributed to the `Expr::to_vec` deep copies of `ps`), and
//! now the inversion described under ★★ below: the `Vec` that L2 was sharing is
//! gone, and the trie it was a source for is the field.
//!
//! `models/build.rs` declares `.rhoapi.EPathMap` as an EXTERN type
//! (`tonic_prost_build::configure().extern_path(".rhoapi.EPathMap",
//! "crate::rust::rhoapi_ext::EPathMap")` — tonic-prost-build 0.14.6
//! `src/lib.rs:464`, forwarded verbatim to `prost_build::Config`), so prost
//! no longer generates the struct: every generated reference
//! (`expr::ExprInstance::EPathmapBody`, `EZipper.pathmap`) resolves to THIS
//! type, and `models/src/lib.rs` re-exports it from `crate::rhoapi` so all
//! existing import paths keep working. There is exactly ONE `EPathMap` type
//! in the program (plan v1 risk R1: no unwrapped twin).
//!
//! WHY: the post-P2 E-6d profile's #1 residual is the P1 store rendezvous
//! itself — every `interned_epathmap` call re-walks the map through the
//! streaming digest (prost `encoded_len`/`encode` frames grew 3.34% → 20.12%
//! of the 4.31×-smaller wall; `Par::encoded_len` alone 10.11%). The wrapper
//! adds a private SHADOW CELL (`intern: OnceLock<Arc<InternedEPathMap>>`)
//! that pins the interned entry on the instance: the store rendezvous
//! becomes an O(1) cell read once ANY clone ancestor interned (the cell
//! travels with `Clone`), and the cached canonical bytes serve
//! `Message::encoded_len` (O(1)) and `Message::encode_raw` (one `memcpy`)
//! for every consumer holding an interned instance — substitution charges,
//! `to_byte_array`, nested encodes — with THE SAME numbers and THE SAME
//! bytes (`InternedEPathMap.encoded_len == canonical_prost.len()` by
//! construction, gated by the P0 goldens).
//!
//! EVERY impl here is manual (or a derive proven layout-identical) because
//! the extra non-prost field derails the stock derives:
//!
//! * `prost::Message` — prost-derive cannot skip a non-annotated field, so
//!   the impl replicates the prost-derive 0.14.3 expansion for the proto
//!   shape (`RhoTypes.proto:321-328`: `ps` tag 1 repeated message,
//!   `locally_free` tag 3 bytes, `connective_used` tag 4 bool, `remainder`
//!   tag 5 optional message) field-for-field, PLUS the cached fast path.
//!   `merge_field`/`clear` RESET the cell before mutating (a decoded/cleared
//!   value must never carry a stale handle).
//! * `Clone` — propagates the filled cell (`OnceLock::clone` clones the
//!   inner `Arc`): the handle travels with the clone family, so the
//!   first-touch digest walk is paid once per family, not per copy. `ps` is an
//!   [`EntryTrie`], whose clone is a refcount bump on the trie root plus an
//!   `Arc` bump on the memoized projection — `EPathMap::clone` is O(1) AT THE
//!   NODE (only `locally_free`/`remainder` still copy, both small).
//! * `Default`/`Debug` — prost-derive generates both alongside `Message`;
//!   replicated here (Debug prints the four proto fields in declaration
//!   order and omits the cell, matching the old derived output).
//! * serde `Serialize` — HAND-WRITTEN (see the `impl serde::Serialize` below);
//!   `Deserialize` — DERIVED, with `#[serde(skip)]` on the cell. The
//!   Serialize impl emits `serialize_struct("EPathMap", 4)` + the four fields
//!   in declaration order, `locally_free` ALWAYS empty (an inline `EmptyBytes`
//!   wrapper = `serialize_bytes(&[])`, byte-identical to the dropped
//!   `serialize_with = serialize_as_empty_bytes` attribute). It differs from a
//!   pure derive in ONE way and that way is now the ONLY way: the asymmetry is
//!   serialize-ONLY — the derived `Deserialize` reads real `locally_free` bytes
//!   from the stream (no `deserialize_with`). The ground/non-ground `ps` fork
//!   that used to live here is deleted; `ps` is the entry projection for every
//!   map, so the event-hash preimage is a pure function of the entry set by
//!   construction. Gated by the P0 serde goldens + the canonical-twin proptest
//!   differential.
//! * `PartialEq`/`Hash` — the AlwaysEqual impls MOVED from
//!   `models/src/lib.rs:613-627`: `ps`/`connective_used`/`remainder` only,
//!   `locally_free` IGNORED (scalapb `AlwaysEqual[BitSet]` parity).
//! * `Eq`/`Ord`/`PartialOrd` — replicate the derived declaration-order
//!   comparison `ps → locally_free → connective_used → remainder`,
//!   INCLUDING `locally_free`. This is deliberately INCONSISTENT with the
//!   AlwaysEqual `==` (two maps can be `==` yet `cmp` `Less`) — the wart is
//!   load-bearing 84a0fbe4 behavior, pinned by the P0 Ord fixtures and the
//!   wrapper-suite wart test; do NOT "fix" it. ★ It is now the ONLY thing
//!   `Ord` and `==` disagree about: both read the same entry projection, so the
//!   second, accidental disagreement — `cmp` reading a producer's order while
//!   `==` read `U(m)` — is gone. See `EPathMap::cmp`.
//! * `utoipa::ToSchema` — derived over the four visible fields
//!   (`#[schema(ignore)]` + `#[serde(skip)]` hide the cell), matching the
//!   old generated schema.
//!
//! `#[repr(C)]` is intentionally DROPPED (the generated struct carried it
//! via the blanket `message_attribute(".rhoapi", "#[repr(C)]")`): the sweep
//! recorded in the P3 change (grep for `transmute`/`from_raw`/raw-pointer
//! casts × `EPathMap` across models/rholang/rspace++/casper/node/comm —
//! zero hits; `EPathMap` is not referenced AT ALL outside models, rholang,
//! and mettail's `rholang-runtime`) found no FFI or layout-dependent
//! consumer, and the `OnceLock` field would make a C layout meaningless
//! anyway. Amendment PM-5(4).
//!
//! ★★ THE INVERSION: `ps` IS THE TRIE
//!
//! `EPathMap.ps` was a `Vec<Par>` (latterly an `Arc<Vec<Par>>`) with a trie
//! built from it on demand and cached in the intern store. It is now an
//! [`EntryTrie`] — the trie itself — with the `Vec<Par>` derived from it on
//! demand and memoized. Nothing about a pathmap's meaning changed; what
//! changed is which of the two representations is authoritative.
//!
//! That single move deletes, rather than fixes, four separate mechanisms:
//!
//! * the MUTATION DISCIPLINE this comment used to describe. `ps_make_mut`
//!   handed out `&mut Vec<Par>` after taking the shadow cell, and the raw
//!   `map.ps.make_mut()` bypass was policed by `debug_assert`s in
//!   `encode_raw`/`encoded_len` that re-streamed the fields against the cached
//!   bytes on every cached use. **All of it is gone**: there is no
//!   `&mut Vec<Par>` to hand out, because an entry SET has no positions to
//!   write at. The three mutators that remain ([`EPathMap::insert_entry`],
//!   [`EPathMap::extend_entries`], [`EPathMap::remove_greatest_entry`]) name an
//!   entry rather than a slot, and each takes the cell before touching the trie.
//! * the STALE-CELL invariant. It said *"the cached bytes still equal the
//!   fields' bytes"*, and it was a real question only while the cache and the
//!   fields were two things. The cached thing is now derived from the stored
//!   thing, so `cached_bytes_still_valid` and `fields_match_canonical_prost` are
//!   deleted with the assertions that called them.
//! * the REBUILD. `ground_path_stream(&ps)` built a throwaway trie on every
//!   pre-intern encode; `path_stream_of` now walks the stored one.
//! * the CANONICALISATION FORK. Four consumers — `Serialize`,
//!   `spliced_event_bytes::emit_epathmap`, `wire::pathmap_ps`, and
//!   `sort_combine::combine_epathmap` — each carried a *"if this map is ground,
//!   read the entries off a trie instead"* branch. The projection is that same
//!   trie read, for every map, so all four branches collapse to one expression.
//!
//! ⚠ CONSENSUS-VISIBLE. For NON-ground maps the stored order was the producer's
//! and is now the trie's, which moves prost bytes (tag 1 `repeated Par`), serde
//! bytes, event-hash preimages, and sort order. Ground maps are unaffected —
//! they encoded as proto field 8 (the trie's own key stream) already. See the
//! commit message; the network version constant is deliberately NOT touched
//! here, because bumping it is a network-coordination act rather than a code
//! act.
//!
//! SHARING SEMANTICS: an `EPathMap` clone is O(1) at the node — the trie clone
//! is a refcount bump on the root `TrieNodeODRc` and the memoized projection is
//! an `Arc` bump. Aliasing is safe without any copy-on-write discipline,
//! because no `&mut` path to shared state exists to begin with.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::DecodeError;

use super::canonical_path::{decode_trie_path, encode_trie_path, encode_trie_path_with_stability};
// ⚠ `eval_stable_epathmap` is deliberately NOT imported here any more. It was the
// wire discriminant — the predicate that chose between field 8 and the tag-1 list —
// and with one arm there is nothing left for it to select. It is NOT deleted: it
// remains `canonical_path`'s recursion cut and `entries_stable()`'s own consumer.
// Removing the import is what makes it impossible to reintroduce the fork by
// reflex.
use super::pathmap_crate_type_mapper::{
    encode_ground_field8, eval_stable_par, ground_field8_len, path_stream_of,
};
use super::pathmap_integration::RholangPathMap;
use crate::rhoapi::{Par, Var};

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE ENTRY TRIE — the stored form of an `EPathMap`'s entries
// ─────────────────────────────────────────────────────────────────────────────

/// ★ **The entries of an `EPathMap`, stored as the trie they always were.**
///
/// This type replaces the stage-L2 `SharedPars` (`Arc<Vec<Par>>`). The change is
/// an **inversion, not a rewrite**: the `Vec` was the primary and a trie was
/// built from it on demand; now the trie is the primary and the `Vec` is a
/// derived, read-only projection of it ([`Self::view`]).
///
/// # Why the trie, and why this is subtractive
///
/// A `RholangPathMap` is a *set of `Par` entries indexed by their own codec
/// path* — `create_pathmap_from_elements` files each entry under
/// `encode_trie_path(entry)`. A trie's children are indexed **by byte**, so a
/// read-zipper walk over it **is** byte-lexicographic *by construction*. The
/// trie therefore already carries an order; nothing selects it and nothing could
/// select a different one, which is why `path_stream_of` performs **no sort**.
///
/// The `Vec<Par>` carried a **second, competing order** — the order a producer
/// happened to write the entries in — and that order shadowed the trie's:
/// `encode_raw` emitted a ground map as proto field 8 (the trie's key stream)
/// while `==`, `Hash`, `Ord`, and the non-ground wire arm all read the `Vec`.
/// Deleting the `Vec` does not *impose* canonicity; it **removes the
/// competitor**, leaving exactly one order. That is why the repair is
/// subtractive and why so much machinery disappears with it (below).
///
/// # What became unrepresentable rather than fixed
///
/// * **Insertion order.** A trie has none. Two constructions of one entry set —
///   permuted, duplicated, or both — are the same trie, hence the same value,
///   the same bytes, and the same hash. (Defect #83.)
/// * **Staleness.** The stage-L2 hazard was that `ps` was the primary and the
///   interned canonical bytes were a cache of it, so a write to `ps` could
///   outrun the cache. Two `debug_assert`s and `fields_match_canonical_prost`
///   existed to police exactly that. **The cached thing is now the stored
///   thing**, so there is nothing to police: all three are deleted.
/// * **A rebuild.** `ground_path_stream(&ps)` used to build a throwaway trie on
///   every pre-intern encode. The trie is right here; the rebuild is deleted.
///
/// # Reading
///
/// [`Self::view`] materializes the entries **once** per value and memoizes them
/// behind an `Arc`, so `EPathMap::clone` stays an O(1) refcount bump at the node
/// and repeated reads are free. There is deliberately **no**
/// `&mut Vec<Par>` escape anywhere — the memo cannot diverge from the trie
/// because nothing can write to it.
///
/// # Writing
///
/// [`Self::insert_entry`], [`Self::extend_entries`], and
/// [`Self::remove_greatest_entry`] mutate the **trie** and take the memo. There
/// is no `ps_make_mut`: an entry set has no positions to write at, so the
/// position-shaped mutators (`push`/`pop`/`extend` over a `Vec`) could not be
/// carried over even if we wanted them.
///
/// # The O(1) metadata, and why it is maintained at insert rather than derived
///
/// `entries_stable`, `len`, `union_locally_free`, and `any_connective_used` are
/// all folds over the entries. Deriving them would force [`Self::view`] — a full
/// `decode_trie_path` walk — on the *hottest* path there is: `encode_raw` asks
/// "is this map ground?" to pick its wire arm, and a ground map's encoding needs
/// only the trie's key stream, never the decoded entries. Folding them in as the
/// entries arrive keeps that question O(1) and keeps the decode off the encode
/// path entirely.
pub struct EntryTrie {
    /// THE STORE. Keys are `encode_trie_path(entry)` (capless, injective,
    /// prefix-free, and **total** over every `Par` via the `0x0F` escape arm —
    /// `canonical_path.rs`), so a non-ground entry is as storable as a ground
    /// one and there is no arm in which the `Vec` has to come back.
    trie: RholangPathMap,
    /// Number of DISTINCT entries. Maintained at insert/remove because
    /// `PathMap::val_count` is documented O(N) ("This is not a cheap method",
    /// `pathmap-0.2.2/src/trie_map.rs:499`).
    len: usize,
    /// `ps.iter().all(eval_stable_par)` — the entry half of the GROUND predicate.
    ///
    /// ⚠ NO LONGER A WIRE DISCRIMINANT. This used to select the wire arm: a ground
    /// map emitted proto field 8, everything else took the tag-1 field walk, so a
    /// conservative `false` was a consensus-visible byte change. That fork is
    /// DELETED — every map emits proto field 8 now — and this fold no longer
    /// chooses anything the wire can see.
    ///
    /// It is still computed EXACTLY rather than conservatively, because it is
    /// still the honest answer to "are all entries in the codec's ground domain?"
    /// for `entries_stable()`'s remaining consumers, and because it comes free
    /// from the encoder's own verdict (`encode_trie_path_with_stability`) rather
    /// than from a second walk that could form a second opinion.
    entries_stable: bool,
    /// Union of the entries' `locally_free` bitsets — the value
    /// `create_pathmap_from_elements` used to compute on every conversion.
    union_locally_free: Vec<u8>,
    /// OR of the entries' `connective_used` flags (the map's own `remainder`
    /// is folded in by the caller, not here — it is metadata, not an entry).
    any_connective_used: bool,
    /// The memoized key walk: `canonical_ps_from_trie(&trie)`. A pure function
    /// of `trie` with no mutator, behind an `Arc` so `Clone` stays O(1).
    view: OnceLock<Arc<Vec<Par>>>,
    /// ★ **THE consensus byte string** — `U(m)`, the trie's own length-framed
    /// key stream (`path_stream_of`). A pure function of `trie`, memoized behind
    /// an `Arc` so `Clone` stays O(1) and a warm encode is one `memcpy`.
    ///
    /// This is the trie serialized *as a trie*: `pathmap`'s `.paths` payload
    /// (`u32-LE keylen ++ key`, in zipper order) with the disqualifying
    /// zlib-ng deflate removed. Every serialization surface reads it, so the
    /// entry SET — never a projected sequence — is what reaches the wire.
    ///
    /// ⚠ It walks KEYS and never decodes, which is what makes it total where
    /// [`EntryTrie::view`]'s key-side twin would not be.
    path_stream: OnceLock<Arc<Vec<u8>>>,
}

/// ★ **THE reader.** The entries of a trie, in trie order (a read-zipper walk,
/// **no sort**), read off the VALUE side.
///
/// `ZipperIteration::to_next_val` stops at exactly the positions that hold a
/// value and skips a value at the empty (root) key, so this walk enumerates the
/// same positions — in the same order — that `path_stream_of` frames into
/// `U(m)`. The reducer's answer and the consensus key stream are one traversal.
///
/// It performs **no decode**, which is what makes it total: see
/// [`EntryTrie::view`] for the measured reason that matters.
fn entries_in_trie_order(map: &RholangPathMap) -> Vec<Par> {
    use pathmap::zipper::{ZipperIteration, ZipperValues};
    let mut ps = Vec::new();
    let mut rz = map.read_zipper();
    while rz.to_next_val() {
        ps.push(
            rz.val()
                .expect("to_next_val stops only at positions holding a value")
                .clone(),
        );
    }
    ps
}

impl EntryTrie {
    /// The materialized entries: trie order (a read-zipper walk, **no sort**)
    /// and deduplicated. Computed once per value and memoized behind an `Arc`,
    /// so every later call — and every call on any clone made afterwards — is a
    /// pointer read.
    ///
    /// # ★ It reads the trie's VALUES, and it must
    ///
    /// The obvious implementation is `canonical_ps_from_trie`: walk the keys and
    /// `decode_trie_path` each one. **That implementation is not total, and the
    /// failure is reachable.** `encode_trie_path`'s escape arm stores a
    /// non-ground entry as its canonical prost bytes, and prost's *decoder* caps
    /// recursion at 100 levels while prost's *encoder* caps nothing — so a term
    /// deeper than that encodes to a key that will not decode
    /// (`DecodeError::RecursionLimitReached`), and the projection panics on a
    /// trie the system itself built. It was measured, not reasoned about: the
    /// `par_codec_differential` corpus contains such a term.
    ///
    /// Reading the values sidesteps it completely, because the codec is then
    /// used **in the encode direction only** on every path that has to be total.
    /// Decode survives exactly where a decode failure is a legitimate answer:
    /// rejecting a peer's tag-8 key stream in `merge_field`.
    ///
    /// ⚠ This is NOT a return to the two-bulk-readers defect that `c705776c`
    /// closed. There is still exactly one reader of *"what does this map
    /// contain?"* — it reads the other side of the same entries. The key walk
    /// survives only as a CHECK ([`EntryTrie::adopt_trie`],
    /// `pathmap_integration::trie_entry_divergences`), never as an answer.
    ///
    /// # Why the result is still recursively canonical
    ///
    /// The old key walk got recursive canonicality from `decode ∘ encode` being
    /// the codec's fixed point. The value walk gets it from **construction**:
    /// every `EPathMap` files its own entries into its own trie, so a nested map
    /// inside an entry was already canonical when the entry was built. Canonical
    /// form is maintained hereditarily rather than re-derived on every read —
    /// which is also why it is now free.
    pub fn view(&self) -> &Vec<Par> {
        self.view
            .get_or_init(|| Arc::new(entries_in_trie_order(&self.trie)))
    }

    /// ★ `U(m)` — the trie's own length-framed key stream, memoized.
    ///
    /// The byte string every serialization surface emits: prost field 8, serde,
    /// and the bincode event-hash preimage all hand *this slice* to their host
    /// format's byte-string primitive. Computed once per value and shared by
    /// every later clone, so the hot encode is a `memcpy` and allocates nothing.
    ///
    /// # Why it is canonical without sorting anything
    ///
    /// A trie indexes children by byte, so a read-zipper walk is
    /// byte-lexicographic *by construction* and `path_stream_of` performs no
    /// sort. Duplicates are impossible — one slot per key. `encode_trie_path`
    /// is injective on its domain. So `U(m)` and the entry set determine each
    /// other, and a producer's insertion order is not merely normalized away:
    /// it is **unrepresentable**.
    pub fn path_stream(&self) -> &[u8] {
        self.path_stream
            .get_or_init(|| Arc::new(path_stream_of(&self.trie)))
    }

    /// The trie itself — the store, handed out for the O(1) `clone()` that
    /// `PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap` returns and for
    /// the `path_stream_of` walk that produces `U(m)`.
    pub fn trie(&self) -> &RholangPathMap {
        &self.trie
    }

    /// Distinct entry count — O(1) (see the field docs for why it is not
    /// `PathMap::val_count`).
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` iff the map holds no entries — O(1) (`PathMap::is_empty` reads
    /// the root node's tag).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` iff every entry is in the codec's ground domain
    /// (`eval_stable_par`) — the entry half of the GROUND wire predicate. O(1).
    pub fn entries_stable(&self) -> bool {
        self.entries_stable
    }

    /// Union of the entries' `locally_free` bitsets.
    pub fn union_locally_free(&self) -> &[u8] {
        &self.union_locally_free
    }

    /// OR of the entries' `connective_used` flags.
    pub fn any_connective_used(&self) -> bool {
        self.any_connective_used
    }

    /// `true` iff `self` and `other` share one memoized view allocation — the
    /// test seam for asserting O(1) clone sharing. Representation-only: never
    /// part of value semantics.
    pub fn view_ptr_eq(&self, other: &EntryTrie) -> bool {
        match (self.view.get(), other.view.get()) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    /// Add one entry. Idempotent — re-adding an entry already present is a
    /// no-op on the set, which is what makes `setLeaf` and `graft` unable to
    /// create duplicates.
    ///
    /// Takes the memo (the projection must be recomputed) but never hands out a
    /// `&mut Vec<Par>`: the only thing a caller can do is name an entry.
    pub fn insert_entry(&mut self, par: Par) {
        // ★ ONE walk, not two. `encode_trie_path` opens with
        // `let stable = known_stable || eval_stable_par(par)` — stability is what selects
        // the escape arm — so this used to run `eval_stable_par` a SECOND time over the
        // same entry, and `entries_stable` became a second opinion about something the
        // codec had already decided. Now the codec hands the bit back.
        //
        // ⚠ Still EXACT, though no longer for the wire's sake: this fold used to select
        // proto field 8 over the tag-1 field walk, and that fork is now deleted. It stays
        // exact because it is the encoder's own verdict rather than an approximation, and
        // taking it here costs nothing — the alternative is a second walk that could form
        // a second opinion about something the codec has already decided.
        let (key, stable) = encode_trie_path_with_stability(&par);
        self.entries_stable &= stable;
        self.any_connective_used |= par.connective_used;
        self.union_locally_free = crate::rust::utils::union(
            std::mem::take(&mut self.union_locally_free),
            par.locally_free.clone(),
        );
        if self.trie.insert(key, par).is_none() {
            self.len += 1;
        }
        self.view.take();
        self.path_stream.take();
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
        use pathmap::zipper::{ZipperIteration, ZipperMoving, ZipperValues};

        let mut rz = other.trie.read_zipper();
        while rz.to_next_val() {
            let par = rz
                .val()
                .expect("to_next_val stops only at positions holding a value");

            // ★ `rz.path()` is the key `other` stores this entry under — no re-encode.
            if self.trie.insert(rz.path(), par.clone()).is_none() {
                self.len += 1;
            }
        }

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

        self.view.take();
        self.path_stream.take();
    }

    /// `true` iff the projection memo has been forced, i.e. a second full copy of the entries
    /// is live. See [`EPathMap::live_retainer_count`].
    #[cfg(test)]
    pub(crate) fn view_is_forced(&self) -> bool {
        self.view.get().is_some()
    }

    /// Visit every entry **by reference**, in trie order, without materialising anything.
    ///
    /// ★ The borrowing counterpart to [`Self::drain_owned_pars`], and the reason it exists:
    /// a caller that only wants to LOOK at each entry had no way to say so. The available
    /// surface was `EPathMap::ps()`, a memoised `Vec<Par>` whose materialisation
    /// **deep-clones every entry** — so a read-only loop paid N clones for the privilege of
    /// borrowing.
    ///
    /// ⚠ A callback rather than an `impl Iterator`: the zipper borrows the trie and would
    /// have to be owned by the iterator, which cannot be expressed without a self-referential
    /// struct. The callback keeps the zipper's lifetime inside this frame, where it is trivial.
    ///
    /// ⚠ It lives here rather than at the call site because `pathmap` is a dependency of
    /// `models` and not of its consumers; exporting the walk is cheaper than exporting the
    /// crate.
    /// Push every entry onto `out` as a borrow with the **TRIE's** lifetime.
    ///
    /// ★ Why this exists alongside [`EntryTrie::for_each_entry`]. That one hands the
    /// visitor a `&Par` borrowed for the duration of the *call*, which is right for a
    /// visitor and useless for a collector: a `Vec<&'a Par>` needs borrows that outlive
    /// the walk. `ZipperReadOnlyIteration::to_next_get_val` returns `&'trie Par` — a
    /// borrow with the trie's lifetime rather than the method call's — so the entries
    /// can be pointed at directly.
    ///
    /// ⇒ a child walk no longer forces [`EntryTrie::view`], whose materialisation
    /// **deep-clones every entry** and then retains a full second copy for the life of
    /// the value. The trie is read where it stands.
    pub fn extend_entry_refs<'trie>(&'trie self, out: &mut Vec<&'trie Par>) {
        use pathmap::zipper::ZipperReadOnlyIteration;
        let mut rz = self.trie.read_zipper();
        while let Some(par) = rz.to_next_get_val() {
            out.push(par);
        }
    }

    /// The FALLIBLE walk: like [`EntryTrie::for_each_entry`], but the visitor may fail
    /// and the failure short-circuits.
    ///
    /// ★ Exists because `for_each_entry` cannot carry a `?`. A caller that evaluates
    /// each entry — and evaluation can fail — otherwise has no borrowing option at all
    /// and falls back to `ps()`, forcing the deep-clone memo to get a `Vec` it only
    /// wanted in order to iterate it once.
    pub fn try_for_each_entry<E>(
        &self,
        mut visit: impl FnMut(&Par) -> Result<(), E>,
    ) -> Result<(), E> {
        use pathmap::zipper::{ZipperIteration, ZipperValues};
        let mut rz = self.trie.read_zipper();
        while rz.to_next_val() {
            visit(
                rz.val()
                    .expect("to_next_val stops only at positions holding a value"),
            )?;
        }
        Ok(())
    }

    /// The EARLY-EXIT walk: the first entry satisfying `pred`, in trie order.
    ///
    /// ★ Exists because `for_each_entry` cannot `break`. A search expressed through it
    /// would visit every entry after the answer was already known — and a caller who
    /// notices that reaches for `ps().iter().find(..)` instead, forcing the deep-clone
    /// memo. The borrow carries the TRIE's lifetime (`to_next_get_val`), so the hit can
    /// be returned rather than cloned.
    pub fn find_entry<'trie>(
        &'trie self,
        mut pred: impl FnMut(&Par) -> bool,
    ) -> Option<&'trie Par> {
        use pathmap::zipper::ZipperReadOnlyIteration;
        let mut rz = self.trie.read_zipper();
        while let Some(par) = rz.to_next_get_val() {
            if pred(par) {
                return Some(par);
            }
        }
        None
    }

    pub fn for_each_entry(&self, mut visit: impl FnMut(&Par)) {
        use pathmap::zipper::{ZipperIteration, ZipperValues};
        let mut rz = self.trie.read_zipper();
        while rz.to_next_val() {
            visit(
                rz.val()
                    .expect("to_next_val stops only at positions holding a value"),
            );
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
    /// Two retainers, and both are drained:
    ///
    /// * **the memo** — a full second copy of the entries. [`OnceLock::into_inner`] plus
    ///   [`Arc::into_inner`] IS the uniqueness test, and here it is expressible: `Some` means we
    ///   own the `Vec` and move it; `None` means another handle survives, so dropping ours is an
    ///   O(1) refcount decrement and there is nothing to tear down.
    /// * **the store** — `PathMap`'s by-move `IntoIterator`. On a uniquely-owned trie this is a
    ///   true move and clones nothing. On a SHARED root the crate copies-on-write, one
    ///   `<Par as Clone>::clone` per value — but that clone is **converted and stack-flat**
    ///   (`CONVERTED_DEPTH`), so it cannot overflow, and it is exactly what the `.cloned()` this
    ///   replaces paid *unconditionally*. ⇒ never a regression, and strictly better whenever the
    ///   trie is unique.
    pub(crate) fn drain_owned_pars(self, out: &mut Vec<Par>) {
        // ⚠ NO `..` — see the doc above.
        let EntryTrie {
            trie,
            len: _,
            entries_stable: _,
            union_locally_free: _,
            any_connective_used: _,
            view,
            // ★ NOT a retainer. `U(m)` is a flat `Vec<u8>` — it holds no `Par`,
            // so dropping it is a `dealloc` of one buffer and can never reach
            // the recursive destructor this method exists to avoid. It is named
            // rather than elided because the `..`-free form is the guard: a
            // future field that DOES retain entries must fail to compile here.
            path_stream: _,
        } = self;

        if let Some(entries) = view.into_inner().and_then(Arc::into_inner) {
            out.extend(entries);
        }
        for (_key, par) in trie {
            out.push(par);
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
            let mut rz = self.trie.read_zipper();
            let mut last: Option<Vec<u8>> = None;
            while rz.to_next_val() {
                last = Some(rz.path().to_vec());
            }
            last?
        };
        let removed = self.trie.remove(&last_key);
        if removed.is_some() {
            self.len -= 1;
            self.view.take();
            self.path_stream.take();
            // The folds are not invertible, so they are recomputed rather than
            // decremented. `entries_stable` in particular MUST stay exact: a
            // conservative `false` would move a now-ground map off proto field
            // 8, which is a consensus-visible byte change.
            self.recompute_folds();
        }
        removed
    }

    /// Re-derive the entry folds from the (post-removal) trie. Only the removal
    /// path needs this — insertion folds forward.
    fn recompute_folds(&mut self) {
        let entries = entries_in_trie_order(&self.trie);
        self.entries_stable = entries.iter().all(eval_stable_par);
        self.any_connective_used = entries.iter().any(|par| par.connective_used);
        let mut union_locally_free = Vec::new();
        for par in &entries {
            union_locally_free =
                crate::rust::utils::union(union_locally_free, par.locally_free.clone());
        }
        self.union_locally_free = union_locally_free;
        self.view = OnceLock::from(Arc::new(entries));
        // `U(m)` is a function of the trie, which just changed. Dropped rather
        // than recomputed: the folds have to be eager (they are read O(1)), the
        // key stream does not.
        self.path_stream.take();
    }
}

impl EntryTrie {
    /// ★ Take over a trie the caller already has, rather than re-filing its
    /// contents — the route back from `RholangPathMap` to `EPathMap` that every
    /// pathmap-returning method in the reducer takes.
    ///
    /// # Why it verifies instead of trusting
    ///
    /// Adopting a trie means adopting its KEYS, and a key is only meaningful
    /// while it is `encode_trie_path` of what it decodes to. That is checkable
    /// for the price of one `encode_trie_path` per entry — which is exactly what
    /// re-filing through `EntryTrie::from` would have cost anyway — so the check
    /// is free relative to the alternative and this performs it **in release
    /// builds too**, where `rholang_pathmap_to_e_pathmap`'s
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
    pub(crate) fn adopt_trie(map: &RholangPathMap) -> EntryTrie {
        use pathmap::zipper::{ZipperIteration, ZipperMoving, ZipperValues};

        let mut entries: Vec<Par> = Vec::new();
        let mut keys_canonical = true;
        let mut entries_stable = true;
        let mut any_connective_used = false;
        let mut union_locally_free: Vec<u8> = Vec::new();
        {
            let mut rz = map.read_zipper();
            while rz.to_next_val() {
                let par = rz
                    .val()
                    .expect("to_next_val stops only at positions holding a value")
                    .clone();
                // ★ The check runs in the ENCODE direction. `decode_trie_path`
                // would be the natural reading of "is this key canonical?", and
                // it is the wrong one: it is partial on this codec's own image
                // (prost caps decode recursion at 100 levels, encode at
                // nothing), so a deep entry would fail the check for a reason
                // that has nothing to do with the key.
                keys_canonical &= encode_trie_path(&par) == rz.path();
                entries_stable &= eval_stable_par(&par);
                any_connective_used |= par.connective_used;
                union_locally_free =
                    crate::rust::utils::union(union_locally_free, par.locally_free.clone());
                entries.push(par);
            }
        }
        if !keys_canonical || map.val_count() != entries.len() {
            return EntryTrie::from(entries);
        }
        EntryTrie {
            trie: map.clone(),
            len: entries.len(),
            entries_stable,
            union_locally_free,
            any_connective_used,
            view: OnceLock::from(Arc::new(entries)),
            path_stream: OnceLock::new(),
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
    /// `EPathMap::new(other.ps(), …)` takes when a caller wants a map with the
    /// same entries and different metadata.
    fn from(entries: &Vec<Par>) -> Self {
        EntryTrie::from(entries.as_slice())
    }
}

impl Clone for EntryTrie {
    /// O(1) at the node: the trie clone is a refcount bump on the root
    /// `TrieNodeODRc`, and the memoized view is an `Arc` bump. Only the small
    /// `locally_free` bitset copies.
    fn clone(&self) -> Self {
        EntryTrie {
            trie: self.trie.clone(),
            len: self.len,
            entries_stable: self.entries_stable,
            union_locally_free: self.union_locally_free.clone(),
            any_connective_used: self.any_connective_used,
            view: self.view.clone(),
            path_stream: self.path_stream.clone(),
        }
    }
}

impl Default for EntryTrie {
    /// The empty entry set. `entries_stable` starts `true` — it is a
    /// `for all` over no entries.
    fn default() -> Self {
        EntryTrie {
            trie: RholangPathMap::new(),
            len: 0,
            entries_stable: true,
            union_locally_free: Vec::new(),
            any_connective_used: false,
            view: OnceLock::new(),
            path_stream: OnceLock::new(),
        }
    }
}

impl fmt::Debug for EntryTrie {
    /// Prints as the projected `Vec<Par>` — the `EPathMap` Debug output keeps
    /// its `EPathMap { ps: [...] }` shape.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.view().fmt(f)
    }
}

impl PartialEq for EntryTrie {
    /// Entry-set equality, read through the **KEYS** — the relation the wire commits to.
    ///
    /// ★ C8, owner-ruled a REPAIR rather than a semantic change. This used to compare
    /// `self.view() == other.view()`, i.e. the projected entries under `Par`'s
    /// **AlwaysEqual** `==`, which IGNORES `locally_free`. But entries are KEYED by
    /// `encode_trie_path`, whose escape arm is the entry's canonical prost bytes —
    /// which INCLUDE `locally_free`. So two maps could compare EQUAL while holding
    /// different key sets, hence different `U(m)`, hence **different emitted bytes**.
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
    fn eq(&self, other: &Self) -> bool {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        if self.len != other.len {
            return false;
        }
        let mut a = self.trie.read_zipper();
        let mut b = other.trie.read_zipper();
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

impl Eq for EntryTrie {}

impl Hash for EntryTrie {
    /// Consistent with [`PartialEq`]: the same **key** stream, hashed in trie order.
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
    fn hash<H: Hasher>(&self, state: &mut H) {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        let mut rz = self.trie.read_zipper();
        while rz.to_next_val() {
            rz.path().hash(state);
        }
    }
}

impl Ord for EntryTrie {
    /// Lexicographic over the **KEY** stream, in trie order — the same relation
    /// [`PartialEq`] and [`Hash`] read.
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
    /// Comparing `U(m)` byte-for-byte would be O(1) after the memo and would
    /// read literally the wire's bytes — tempting, and **rejected**: `U(m)`
    /// frames each key with a `u32-LE` length *prefix*, so the framing would
    /// outrank the content. Keys `["B"]` and `["AB"]` order one way by key and
    /// the other way by `U(m)`, because `1u32` and `2u32` compare before the
    /// first key byte is ever reached. That is a valid total order but an
    /// arbitrary one — it sorts by an artifact of the framing rather than by the
    /// trie's own byte-lexicographic structure. The walk costs what `eq`
    /// already costs and orders by the thing that actually means something.
    fn cmp(&self, other: &Self) -> Ordering {
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        let mut a = self.trie.read_zipper();
        let mut b = other.trie.read_zipper();
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

impl PartialOrd for EntryTrie {
    /// Consistent with [`Ord`].
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl serde::Serialize for EntryTrie {
    /// Transparent seq over the canonical projection (bincode = u64-LE length +
    /// elements; JSON = array) — the same 1-field-of-4 slot `EPathMap`'s serde
    /// layout has always had, now filled from the trie.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.view().as_slice().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for EntryTrie {
    /// Reads a plain `Vec<Par>` (the unchanged serde wire shape) and files it.
    /// A stream carrying a permuted or duplicated entry set therefore decodes
    /// to the same value as its canonical twin.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<Par>::deserialize(deserializer).map(EntryTrie::from)
    }
}

/// The hand-maintained mirror of `message EPathMap` (`RhoTypes.proto:321`),
/// extended with the P3 shadow cell. Field order and types are EXACTLY the
/// generated struct's (`ps`, `locally_free`, `connective_used`, `remainder`)
/// — serde layout, `Ord`, and `Debug` all depend on that order.
///
/// Construction: out-of-module struct literals are impossible (the cell is
/// private) — use [`EPathMap::new`] or [`Default`] (amendment PM-2; every
/// former literal site is migrated). Struct PATTERNS with `..` keep working.
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct EPathMap {
    /// ★ The entries, stored as the [`EntryTrie`] they are indexed by.
    ///
    /// **PRIVATE**, and that is load-bearing rather than tidy: the field is no
    /// longer a `Vec<Par>`, so a caller reading `map.ps` would be reading a
    /// projection and a caller writing it would be writing past the trie. Reads
    /// go through [`EPathMap::ps`] (the memoized canonical projection); writes
    /// go through [`EPathMap::insert_entry`] / [`EPathMap::extend_entries`] /
    /// [`EPathMap::remove_greatest_entry`], each of which takes the shadow cell.
    /// `ps_make_mut` no longer exists — **the compiler is the fixture** for
    /// anything that used to hand out `&mut Vec<Par>`.
    ///
    /// Serde still sees a `ps` field of `Vec<Par>` (the [`EntryTrie`] serde
    /// impls project and re-file), so the serde layout and the OpenAPI schema
    /// are unchanged.
    #[schema(value_type = Vec<Par>)]
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
    // ⛔ The P3 shadow cell (`intern: OnceLock<Arc<InternedEPathMap>>`) is GONE.
    //
    // It memoised four things for a single production caller, and every one was
    // already an O(1) read off this value: the trie itself, the two entry folds,
    // and the `eval_stable` classification. Obtaining a SHARED entry cost a full
    // streamed digest walk plus a second full `encode_raw` walk to verify the
    // bucket — two walks to avoid one — behind a process-global mutex.
    //
    // ★ Byte-safety was established by SIMULATION before any site was edited:
    // forcing the accessor to `None` failed exactly five tests, every one of them
    // a test OF the mechanism (`spliced_*`, `*_intern_cell_*`, `*_filled_cell_*`),
    // and moved ZERO byte goldens.
}

impl serde::Serialize for EPathMap {
    /// Hand-written (the P3 derive is dropped) because `locally_free` is
    /// serialize-asymmetric: it is ALWAYS written as EMPTY bytes (the
    /// normalization the dropped `serialize_with = serialize_as_empty_bytes`
    /// attribute used to provide) while the derived `Deserialize` still reads
    /// the stream's REAL bytes (plan amendment PM-1).
    ///
    /// # ★ The ground/non-ground branch is GONE
    ///
    /// This impl used to fork: a GROUND map serialized `ground_canonical_ps(self)`
    /// — the entries re-read off a trie, in trie order — while every other map
    /// serialized `self.ps` in the order its producer wrote them. The fork
    /// existed because the stored order was not canonical and only the ground
    /// arm had somewhere canonical to read from.
    ///
    /// Now `self.ps` **is** the trie, so [`EPathMap::ps`] is that same canonical
    /// projection for EVERY map, and the two arms are one expression. The
    /// ground arm's bytes are unchanged (same walk, same trie); the non-ground
    /// arm's bytes move to canonical order, which is the consensus-visible half
    /// of this change and is stated as such in the commit.
    ///
    /// Layout is the unchanged 4-field struct
    /// (`serialize_struct("EPathMap", 4)` over `ps`, `locally_free`,
    /// `connective_used`, `remainder`; the `intern` cell is never serialized).
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
    /// The PM-2 constructor — the replacement for every former struct
    /// literal (the private cell makes out-of-module literals impossible).
    /// The cell starts EMPTY: a newly built value has no interned handle
    /// until its first rendezvous.
    ///
    /// `ps` is `impl Into<EntryTrie>`, so every call site passing a `Vec<Par>`
    /// compiles unchanged and every entry is filed under its own codec path as
    /// it arrives. ⚠ Consequently `EPathMap::new(v, …).ps() != v` whenever `v`
    /// is permuted, duplicated, or holds a non-canonical nested map — the
    /// constructor canonicalizes because the store it writes into has only one
    /// order.
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

    /// ★ The entries, in canonical trie order — deduplicated and recursively
    /// canonical. Memoized (see [`EntryTrie::view`]), so this is a pointer read
    /// after the first call on any member of a clone family.
    ///
    /// This replaces the former `pub ps: SharedPars` field. It is a method
    /// rather than a field because it is a **projection**: the map does not
    /// store a `Vec<Par>`, it stores a trie, and the vector is derived from it.
    /// Hand every `Par` this map owns to `out`, **BY MOVE**.
    ///
    /// ★ Delegates to [`EntryTrie::drain_owned_pars`]; see that method for why the destructure
    /// is exhaustive and how the two retainers are drained.
    ///
    /// ⚠ **`intern` is drained too, and that matters more than it looks.** The interned handle
    /// owns an `InternedEPathMap` whose `map: RholangPathMap` is a second holder of the same
    /// entries. Its usual fate is an O(1) refcount decrement — but the process-global intern
    /// store is **LRU-evicting**, so an evicted entry's handle becomes unique and its drop runs
    /// the recursive destructor at full depth, on an arbitrary thread, inside the store's mutex,
    /// at a moment no caller chose. Draining it here removes this value's contribution to that
    /// path. (The store itself is slated for deletion, which dissolves the rest.)
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

    /// How many distinct owners of this map's entries are LIVE right now.
    ///
    /// ★ Exists so a teardown guard can pin the by-move table's multiplier **from the value**
    /// rather than infer it from the data. That distinction is load-bearing: a guard that
    /// derives the multiplier as `moved.len() / borrowed.len()` cannot detect a retainer that
    /// was never drained, because the ratio simply becomes a smaller whole number and still
    /// looks valid. Measured — removing the memo drain left such a guard GREEN.
    ///
    /// The trie always owns them; the projection memo owns a full second copy once forced; the
    /// interned handle owns a third when this map has been interned.
    #[cfg(test)]
    pub(crate) fn live_retainer_count(&self) -> usize {
        // ★ Was `1 + memo + intern`. The interned handle was a THIRD owner of the same
        // entries; with the store deleted there are two: the trie and the projection memo.
        1 + usize::from(self.ps.view_is_forced())
    }

    pub fn ps(&self) -> &Vec<Par> {
        self.ps.view()
    }

    /// The entry store itself — the trie every consumer used to rebuild.
    /// `clone()`ing it is a refcount bump.
    pub fn entry_trie(&self) -> &EntryTrie {
        &self.ps
    }

    /// Add one entry, taking the shadow cell (the map's canonical bytes must be
    /// re-derived). Idempotent: an entry already present is absorbed.
    ///
    /// This is the replacement for `ps_make_mut().push(..)` at `setLeaf`.
    pub fn insert_entry(&mut self, par: Par) {
        self.ps.insert_entry(par);
    }

    /// Add every entry of `other` — the set union `graft` performs. Replaces
    /// `ps_make_mut().extend(other.ps.into_vec())`.
    pub fn extend_entries(&mut self, other: &EPathMap) {
        self.ps.extend_entries(&other.ps);
    }

    /// Remove the entry with the greatest key in trie order. Replaces
    /// `ps_make_mut().pop()` at `removeLeaf`; see
    /// [`EntryTrie::remove_greatest_entry`] for why "the last one" has to be
    /// re-derived from the trie's order rather than from a position.
    pub fn remove_greatest_entry(&mut self) -> Option<Par> {
        self.ps.remove_greatest_entry()
    }




    // ⛔ `encode_raw_fields` / `encoded_len_fields` are DELETED, not parked.
    //
    // They were the tag-1 field walk — `for msg in self.ps() { encode(1u32, msg) }`
    // and its `encoded_len_repeated(1u32, …)` twin — reached only from the
    // ¬eval_stable side of the fork in `encode_raw` / `encoded_len`. With the fork
    // removed there is no caller and no shape they describe: every map emits `U(m)`
    // at field 8, and the metadata fields 3/4/5 are emitted inline by the two
    // methods that replaced them, where the encode and its length twin can be read
    // side by side. Keeping a second, unreachable copy of that logic would be an
    // invitation to drift between two things prost requires to agree byte for byte.
    //
    // (Their `pub(crate)` was for the intern store's K2 verify, which was itself
    // deleted with the store.)

    /// ★ **`U(m)` — the identity artifact of this map's entries.**
    ///
    /// ```math
    /// U(m) \;=\; \big\Vert_{k \in \mathrm{keys}(m)}
    ///            \big(\mathrm{u32\text{-}LE}(|k|) \,\Vert\, k\big)
    /// ```
    ///
    /// # The order is not chosen here — the trie already has one
    ///
    /// A trie's children are indexed BY BYTE, so a read-zipper walk is
    /// byte-lexicographic **by construction**. Nothing selects that order and
    /// nothing could select a different one, which is why [`path_stream_of`]
    /// performs **no sort**: the structure supplies the order, and a sort would
    /// be a second opinion about something that is not in question.
    ///
    /// This is the artifact proto field 8 (`serialized_paths`) carries, described
    /// there as *"the canonical identity + hash preimage of a ground map"*.
    ///
    /// # ★ There is no rebuild here any more
    ///
    /// This used to call `ground_path_stream(&self.ps)`, which built a THROWAWAY
    /// trie from the `Vec` on every pre-intern encode. The trie is now the field,
    /// so the walk reads the store directly. That deletion is the whole shape of
    /// this change in miniature: the work existed only to reconstruct something
    /// the value already had.
    /// `U(m)` — the length-framed key stream of this map's trie.
    ///
    /// ★ `pub` since the intern store was deleted. The store used to front this, so a
    /// caller wanting `U(m)` reached it as `intern().path_stream`; with no store the
    /// walk itself is the only way to ask — and it is now **memoized on the trie**
    /// ([`EntryTrie::path_stream`]) rather than re-walked per call, because every
    /// serialization surface reads it.
    pub fn path_stream(&self) -> &[u8] {
        self.ps.path_stream()
    }
}

impl Default for EPathMap {
    /// prost-derive parity: all proto fields at their defaults, cell empty.
    fn default() -> Self {
        EPathMap::new(Vec::new(), Vec::new(), false, None)
    }
}

impl Clone for EPathMap {
    /// Clones the proto fields AND propagates the filled shadow cell (an
    /// `Arc` bump via `OnceLock::clone`) — the handle travels with the
    /// clone family. `ps` is an [`EntryTrie`], whose clone is a refcount bump
    /// on the trie root plus an `Arc` bump on the memoized projection — the
    /// whole clone is O(1) AT THE NODE (only `locally_free` bytes and the small
    /// `remainder` still copy). No copy-on-write discipline is needed any more:
    /// there is no `&mut Vec<Par>` to hand out, so no clone sibling can observe
    /// a write.
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
    /// prost-derive parity: the four proto fields in declaration order,
    /// plain `Debug` per field (prost's scalar wrappers are pass-through
    /// for bytes/bool), the cell omitted — byte-identical to the old
    /// derived output.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EPathMap")
            .field("ps", &self.ps)
            .field("locally_free", &self.locally_free)
            .field("connective_used", &self.connective_used)
            .field("remainder", &self.remainder)
            .finish()
    }
}

impl prost::Message for EPathMap {
    /// # ★ The stale-cell `debug_assert` is DELETED, and could not be restated
    ///
    /// It read: *"with the cell filled, the CURRENT fields must still
    /// stream-encode to the cached canonical bytes"*, and it existed because the
    /// `Vec<Par>` was the primary while the interned bytes were a cache of it —
    /// a write to the `Vec` could outrun the cache. **The cached thing is now
    /// derived from the stored thing**, and every mutator
    /// ([`EPathMap::insert_entry`], [`EPathMap::extend_entries`],
    /// [`EPathMap::remove_greatest_entry`]) takes the cell before touching the
    /// trie, with no `&mut` escape past them. There is no state in which the
    /// question the assertion asked has a `false` answer, so
    /// `cached_bytes_still_valid` and `fields_match_canonical_prost` are deleted
    /// with it.
    fn encode_raw(&self, buf: &mut impl BufMut) {
        // ⛔ The interned fast path is GONE. It read
        // `self.intern.get()` and, on a hit, `put_slice(&interned.canonical_prost)`.
        //
        // ★ Removing it is byte-neutral BY CONSTRUCTION, and the deleted comment
        // said so itself: *"canonical_prost IS this value's canonical encoding —
        // field 8 U(m) for a ground map, or the field walk otherwise"*. Those bytes
        // were produced by the two arms below this line, inside
        // `OnceLock::get_or_init` with the cell empty. So the cache's content was
        // never anything but the output of the code that now runs unconditionally.
        //
        // ⚠ What it cost to keep: obtaining the shared entry required a full
        // streamed digest walk PLUS a second full `encode_raw` walk to verify the
        // bucket — two walks to avoid one — behind a process-global mutex, in a
        // 64-entry LRU whose eviction drops a deep `Par` through the recursive
        // destructor inside that lock (pgmcp 4910).
        //
        // ★★ ONE ARM. The trie serializes AS A TRIE — proto field 8 = U(m), read
        // straight off the stored trie — for EVERY map, ground or not. There is no
        // longer a `ps` field walk at tag 1, and so no fork to be on the wrong side of.
        //
        // # What this replaced, and why the fork was the defect
        //
        // Field 8 used to be gated on `eval_stable_epathmap(self)`; every other map
        // emitted `repeated Par` at tag 1 — the trie flattened to a LIST and each entry
        // re-encoded from scratch. That threw away, at the wire boundary, exactly the
        // properties the trie exists to provide: the key order it maintains by
        // construction, the dedup it guarantees by holding one slot per key, and the
        // encoded form it already stores. The list then had to be re-filed entry by
        // entry on the far side, paying `encode_trie_path` per entry to rebuild what
        // the sender had already computed.
        //
        // # Byte movement, stated exactly
        //
        // GROUND maps are byte-IDENTICAL. `eval_stable_epathmap` requires fields 3/4/5
        // at their proto defaults, so a ground map skipped all three and emitted field 8
        // alone — which is still precisely what the ascending-tag walk below produces.
        // The ground goldens must come back UNMOVED, and they are the anti-vacuity
        // control for the non-ground re-blessing.
        //
        // NON-GROUND maps move: tag-1 field walk ⇒ tag 8. That is the consensus-visible
        // half, and it is why this carries a seven-axis register entry.
        //
        // # ★ The move is PERMISSIVE, and it is measured rather than argued
        //
        // Reading tag 8 back means `decode_trie_path` per key, whose escape arm
        // re-decodes a ¬eval_stable entry through prost. That is a *fresh*
        // `DecodeContext` — the full 100-level budget starting at zero — whereas tag 1
        // spent W ≥ 3 levels of the OUTER decode's budget before reaching the entry.
        // `rholang/tests/pathmap_escape_depth_reachability.rs` measures the consequence:
        // escape-arm ceiling 32 vs tag-1 ingress ceiling 31.
        //
        // ⇒ every entry tag 1 could deliver, tag 8 can read. The headroom is ONE level,
        // not the three a `W ≥ 3` argument predicts; the inequality is what the
        // conclusion needs and it holds, but the margin is thin and is recorded as
        // measured rather than derived. No term that decodes today stops decoding.
        //
        // Ascending tag order (3, 4, 5, 8), prost-derive parity on the skip-at-default
        // rule for each scalar — the format's universal rule, not a fork.
        if !self.locally_free.is_empty() {
            encoding::bytes::encode(3u32, &self.locally_free, buf);
        }
        if self.connective_used {
            encoding::bool::encode(4u32, &self.connective_used, buf);
        }
        if let Some(ref msg) = self.remainder {
            encoding::message::encode(5u32, msg, buf);
        }
        // Skipped when the trie is empty — proto3's own "omit at default" rule for a
        // `bytes` field, which is also what keeps an empty map byte-identical.
        if !self.ps.is_empty() {
            encode_ground_field8(self.path_stream(), buf);
        }
    }

    // `DecodeError::new` is prost's only public constructor for a custom
    // decode error (it is `doc(hidden)` + deprecation-warned but not yet
    // removed); the tag-8 validation arm needs it.
    #[allow(deprecated)]
    fn merge_field(
        &mut self,
        tag: u32,
        wire_type: WireType,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        // Decode-merge mutates fields: reset the shadow cell FIRST so a
        // merged-into value can never carry a stale handle (plan §1-P3
        // "merge/clear reset the cell"). Taking on the unknown-tag skip arm
        // too is deliberate — one uniform rule, no field-tracking.
        /// prost-derive parity: the error-context struct name pushed onto
        /// `DecodeError` paths.
        const STRUCT_NAME: &str = "EPathMap";
        match tag {
            1u32 => {
                // prost calls `merge_field` once per occurrence of tag 1, so
                // `decoded` receives exactly one entry per call; each is filed
                // under its own codec path. A stream carrying a permuted or
                // duplicated entry set therefore decodes to the same value as
                // its canonical twin — decode is canonicalizing because the
                // store it writes into has only one order.
                let mut decoded: Vec<Par> = Vec::new();
                encoding::message::merge_repeated(wire_type, &mut decoded, buf, ctx).map_err(
                    |mut error| {
                        error.push(STRUCT_NAME, "ps");
                        error
                    },
                )?;
                for par in decoded {
                    self.ps.insert_entry(par);
                }
                Ok(())
            }
            3u32 => {
                let value = &mut self.locally_free;
                encoding::bytes::merge(wire_type, value, buf, ctx).map_err(|mut error| {
                    error.push(STRUCT_NAME, "locally_free");
                    error
                })
            }
            4u32 => {
                let value = &mut self.connective_used;
                encoding::bool::merge(wire_type, value, buf, ctx).map_err(|mut error| {
                    error.push(STRUCT_NAME, "connective_used");
                    error
                })
            }
            5u32 => {
                let value = &mut self.remainder;
                encoding::message::merge(
                    wire_type,
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
                // VALUE arm: field 8 (serialized_paths, bytes) = U(m). Decode
                // the length-framed key stream and reconstruct `ps` by decoding
                // each trie key. The result is canonical (trie order) by
                // construction, so a decoded ground map compares
                // structurally-equal to any permuted construction of the same
                // entry multiset. (The shadow cell was reset at the top of
                // merge_field, so the map re-interns on its next touch.)
                let mut region: Vec<u8> = Vec::new();
                encoding::bytes::merge(wire_type, &mut region, buf, ctx).map_err(|mut error| {
                    error.push(STRUCT_NAME, "serialized_paths");
                    error
                })?;
                let mut cursor = 0usize;
                let mut entries: Vec<Par> = Vec::new();
                while cursor + 4 <= region.len() {
                    let len = u32::from_le_bytes(
                        region[cursor..cursor + 4].try_into().expect("4-byte length"),
                    ) as usize;
                    cursor += 4;
                    let end = cursor.checked_add(len).ok_or_else(|| {
                        DecodeError::new("EPathMap serialized_paths: key length overflow")
                    })?;
                    if end > region.len() {
                        return Err(DecodeError::new("EPathMap serialized_paths: truncated key"));
                    }
                    let par = decode_trie_path(&region[cursor..end]).map_err(|codec_error| {
                        DecodeError::new(format!("EPathMap serialized_paths key: {codec_error:?}"))
                    })?;
                    entries.push(par);
                    cursor = end;
                }
                if cursor != region.len() {
                    return Err(DecodeError::new(
                        "EPathMap serialized_paths: trailing bytes after the final key",
                    ));
                }
                // Re-file each decoded key through `insert_entry` rather than
                // re-using the incoming key bytes verbatim: a peer's key that
                // is not `encode_trie_path` of its own decoding would otherwise
                // enter the trie un-normalized. Decoding and re-encoding makes
                // tag-8 decode idempotent-canonical.
                for par in entries {
                    self.ps.insert_entry(par);
                }
                Ok(())
            }
            _ => encoding::skip_field(wire_type, tag, buf, ctx),
        }
    }

    /// The `debug_assert` that used to guard this cached read is deleted for
    /// the same reason as [`Self::encode_raw`]'s — see the note there.
    #[inline]
    fn encoded_len(&self) -> usize {
        // ⛔ The interned O(1) arm is GONE, mirroring `encode_raw`. It returned
        // `interned.encoded_len`, which was `canonical_prost.len()` — the length of
        // bytes the two arms below produce. Same value by construction.
        //
        // ⚠ Honest cost, stated rather than buried: on a map whose cell HAD been
        // filled, `Message::encoded_len` drops from O(1) to O(map). That matters on
        // the substitution charge path, which walks `encoded_len` twice per
        // substitution by design. It is bounded to maps that went through the fused
        // chain's intern call — every other map already paid O(map) — and the price
        // of keeping it was two full walks plus a global lock per rendezvous.
        //
        // ★ THE EXACT TWIN of `encode_raw`. Same four conditions, same order, same
        // skip-at-default rule, term for term — prost corrupts the stream if these two
        // ever disagree by a single byte, so they are written to be read side by side.
        //
        // ⚠ This is also a METERING input (`costs.rs`, `substitute.rs`), so the charge
        // moves with the bytes: it is no longer the sum of per-entry prost lengths but
        // the length of the one shared key stream — a figure the memo hands back
        // without re-encoding anything.
        (if !self.locally_free.is_empty() {
            encoding::bytes::encoded_len(3u32, &self.locally_free)
        } else {
            0
        }) + (if self.connective_used {
            encoding::bool::encoded_len(4u32, &self.connective_used)
        } else {
            0
        }) + self
            .remainder
            .as_ref()
            .map_or(0, |msg| encoding::message::encoded_len(5u32, msg))
            + (if !self.ps.is_empty() {
                ground_field8_len(self.path_stream())
            } else {
                0
            })
    }

    fn clear(&mut self) {
        // prost-derive parity for the proto fields, plus the cell reset. `ps`
        // is replaced by a fresh empty trie rather than emptied in place —
        // observationally identical (`clear` promises fields-at-defaults) and
        // O(1) regardless of what the old trie was sharing.
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

impl PartialEq for EPathMap {
    /// AlwaysEqual semantics: `locally_free` is a transient analysis field
    /// and does NOT participate (scalapb `AlwaysEqual[BitSet]` parity). The
    /// shadow cell does not participate either (it is derived state).
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
    /// `self.ps` is now the trie, so the projection [`EPathMap::ps`] returns is
    /// already in trie order, already deduplicated, and already recursively
    /// canonical. **A positional comparison of two canonical projections IS set
    /// comparison** — there is no permutation left to be fooled by — so the
    /// second arm has nothing left to add and `entries_in_ground_domain` is
    /// deleted along with it. Defect #83 is not fixed here; it is
    /// unrepresentable, for every map rather than for ground maps only.
    ///
    /// ⚠ One consequence, stated rather than buried: entries are keyed by
    /// `encode_trie_path`, whose escape arm is the entry's **canonical prost
    /// bytes**, which INCLUDE `locally_free`. Two entries that are AlwaysEqual
    /// but differ in `locally_free` are therefore distinct trie keys. That is
    /// the same discipline the intern store's K2 verify already applies (*"the
    /// generated AlwaysEqual `==`/`Hash` IGNORE `locally_free` and are therefore
    /// UNUSABLE for keying"*), now applied by the map itself. In a well-formed
    /// term `locally_free` is a function of the structure, so the two cannot
    /// differ; the note is here because "cannot" should be written down.
    fn eq(&self, other: &Self) -> bool {
        self.connective_used == other.connective_used
            && self.remainder == other.remainder
            && self.ps == other.ps
    }
}

impl Eq for EPathMap {}

impl Hash for EPathMap {
    /// AlwaysEqual semantics: consistent with `==` (`locally_free` and the
    /// cell excluded). Both now read the same canonical projection, so this is
    /// the plain element-wise `Vec<Par>` hash again — it is a function of the
    /// entry SET not because it does anything clever but because the thing it
    /// reads has no order of its own to leak.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ps.hash(state);
        self.connective_used.hash(state);
        self.remainder.hash(state);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Derived-Ord replica — declaration order INCLUDING locally_free (the wart)
// ─────────────────────────────────────────────────────────────────────────────

impl Ord for EPathMap {
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
    /// consensus-visibly for a reason unrelated to this change.
    ///
    /// What DID move is the other disagreement, the one nobody chose. Before the
    /// trie became the field, `cmp` read `ps` in the producer's order while `==`
    /// read `U(m)`, so `Ord` and `Eq` disagreed **twice**: once about
    /// `locally_free` (deliberately) and once about entry order (accidentally,
    /// and in a way that made two maps the wire calls identical sort apart).
    /// Both now read the same canonical projection, so exactly one deliberate
    /// inconsistency remains and the accidental one is gone.
    ///
    /// ⚠ Consensus consequence, stated plainly: for NON-ground maps this moves
    /// sort order, because the entries `cmp` walks are now in trie order rather
    /// than construction order. That is the same byte-moving change the wire arm
    /// makes, and it is the reason this stage is consensus-visible.
    ///
    /// Note that `Par: Ord` is the DERIVED one and includes each entry's own
    /// `locally_free`; the wart is hereditary, and that too is unchanged.
    fn cmp(&self, other: &Self) -> Ordering {
        self.ps
            .cmp(&other.ps)
            .then_with(|| self.locally_free.cmp(&other.locally_free))
            .then_with(|| self.connective_used.cmp(&other.connective_used))
            .then_with(|| self.remainder.cmp(&other.remainder))
    }
}

impl PartialOrd for EPathMap {
    /// Consistent with [`Ord`] (all four fields are totally ordered, so the
    /// derived field-chaining `partial_cmp` is extensionally `Some(cmp)`).
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
