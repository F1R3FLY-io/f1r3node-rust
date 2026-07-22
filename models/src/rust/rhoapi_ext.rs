//! EPathMap fix P3 — the hand-maintained `EPathMap` wrapper (full T1, stage
//! L1.5), extended by stage L2 — the shared-`ps` representation
//! ([`SharedPars`], USER decision D2 approved 2026-07-20 on the E-6d #2
//! evidence: clone-class 29.76% flat at ≈44.8 ms/inj across e6d1→e6d2, the
//! attributed residual being the `Expr::to_vec` deep copies of `ps`).
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
//!   first-touch digest walk is paid once per family, not per copy. Stage
//!   L2: `ps` is a [`SharedPars`] (`Arc<Vec<Par>>`), so the former L1.5
//!   deep-copy is now an `Arc` bump too — `EPathMap::clone` is O(1) AT THE
//!   NODE (both the `ps` payload and the intern handle are refcount bumps;
//!   only `locally_free`/`remainder` still copy, both small).
//! * `Default`/`Debug` — prost-derive generates both alongside `Message`;
//!   replicated here (Debug prints the four proto fields in declaration
//!   order and omits the cell, matching the old derived output).
//! * serde `Serialize` — HAND-WRITTEN (see the `impl serde::Serialize` below);
//!   `Deserialize` — DERIVED, with `#[serde(skip)]` on the cell. The
//!   Serialize impl emits `serialize_struct("EPathMap", 4)` + the four fields
//!   in declaration order, `locally_free` ALWAYS empty (an inline `EmptyBytes`
//!   wrapper = `serialize_bytes(&[])`, byte-identical to the dropped
//!   `serialize_with = serialize_as_empty_bytes` attribute). It differs from a
//!   pure derive in ONE way: a GROUND map (`eval_stable_epathmap`) serializes
//!   its `ps` in CANONICAL TRIE ORDER (`ground_canonical_ps`) so the event-hash
//!   preimage is a pure function of the entry set (producer-independent);
//!   non-ground/empty maps stay byte-identical to the P3 derived layout. The
//!   asymmetry is serialize-ONLY: the derived `Deserialize` reads real
//!   `locally_free` bytes from the stream (no `deserialize_with`), exactly as
//!   before. Gated by the P0 serde goldens + the canonical-twin proptest
//!   differential.
//! * `PartialEq`/`Hash` — the AlwaysEqual impls MOVED from
//!   `models/src/lib.rs:613-627`: `ps`/`connective_used`/`remainder` only,
//!   `locally_free` IGNORED (scalapb `AlwaysEqual[BitSet]` parity).
//! * `Eq`/`Ord`/`PartialOrd` — replicate the derived declaration-order
//!   comparison `ps → locally_free → connective_used → remainder`,
//!   INCLUDING `locally_free`. This is deliberately INCONSISTENT with the
//!   AlwaysEqual `==` (two maps can be `==` yet `cmp` `Less`) — the wart is
//!   load-bearing 84a0fbe4 behavior, pinned by the P0 Ord fixtures and the
//!   wrapper-suite wart test; do NOT "fix" it.
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
//! MUTATION DISCIPLINE (the shadow-cell invariant, tightened by L2): the
//! proto fields stay `pub` (the ~56 read sites and the struct patterns rely
//! on it), but `ps` is now a [`SharedPars`] with NO `DerefMut` — the old
//! silent hazard `map.ps.push(..)` no longer compiles. Every `ps` mutation
//! is loud: the SANCTIONED route is [`EPathMap::ps_make_mut`], which TAKES
//! the cell before handing out `&mut Vec<Par>` (exactly the
//! `merge_field`/`clear` reset discipline — a mutated value re-derives its
//! bytes from the real fields at the next encode), and the raw bypass
//! `map.ps.make_mut()` (no cell reset) remains policed by the
//! `debug_assert`s in `encoded_len`/`encode_raw`, which re-verify the
//! fields against the cached bytes on every cached use — the entire test
//! fleet (debug assertions on) polices the invariant continuously
//! (`stale_cell_mutation_is_policed_in_debug_builds` pins the bypass →
//! panic path; `ps_make_mut_resets_the_cell` pins the sanctioned path);
//! the P0 goldens/differentials gate release behavior. The cell fills ONLY
//! at the interpreter's intern rendezvous, and every `&mut`-path through
//! `Message` (`merge_field`/`clear`) resets it, as in L1.5.
//!
//! L2 SHARING SEMANTICS: sharing is an invisible representation choice —
//! `==`/`Hash`/`Ord`/serde/prost all read THROUGH the `Arc` to the same
//! `Vec<Par>` values as before (identity semantics preserved; wire and
//! serde bytes byte-identical by the P0 goldens). Aliasing is safe by
//! construction: the only `&mut Vec<Par>` escape is `Arc::make_mut`
//! (copy-on-write — a shared payload is cloned before mutation, so no
//! clone sibling can observe a write).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::DecodeError;

use super::canonical_path::decode_trie_path;
use super::pathmap_crate_type_mapper::{
    encode_ground_field8, eval_stable_epathmap, fields_match_canonical_prost, ground_canonical_ps,
    ground_field8_len, ground_path_stream, intern_epathmap_via_store, InternedEPathMap,
};
use crate::rhoapi::{Par, Var};

// ─────────────────────────────────────────────────────────────────────────────
// Stage L2: SharedPars — the Arc-backed `ps` payload
// ─────────────────────────────────────────────────────────────────────────────

/// The stage-L2 representation of `EPathMap.ps`: an `Arc<Vec<Par>>`-backed
/// newtype. Cloning is an `Arc` refcount bump (O(1) — THE clone-class kill
/// the E-6d #2 profile motivates), reading is transparent
/// (`Deref<Target = Vec<Par>>` + `IntoIterator for &SharedPars`, so
/// `map.ps.iter()`, `map.ps.len()`, `for p in &map.ps`, and `&map.ps` →
/// `&[Par]` coercions all compile unchanged), and writing is LOUD: there is
/// deliberately NO `DerefMut` — every mutation must go through
/// [`SharedPars::make_mut`] (`Arc::make_mut` copy-on-write) or, at the
/// `EPathMap` level, [`EPathMap::ps_make_mut`] (which also resets the
/// shadow cell). Value semantics are the `Vec`'s throughout: `==`, `Hash`,
/// `Ord`, `Debug`, serde, and prost all delegate to the payload, so two
/// `SharedPars` are equal/ordered/hashed/serialized exactly as their
/// vectors were before L2 — sharing is representation, never meaning.
pub struct SharedPars(Arc<Vec<Par>>);

impl SharedPars {
    /// Copy-on-write mutable access: `Arc::make_mut` — O(1) when this is
    /// the only holder, one `Vec<Par>` deep-clone when shared (the clone
    /// pays exactly the copy that EVERY clone paid before L2, and only on
    /// actual mutation).
    ///
    /// NOTE: this does NOT touch any enclosing `EPathMap`'s shadow cell —
    /// production mutation of a map's `ps` goes through
    /// [`EPathMap::ps_make_mut`], which takes the cell first. A direct
    /// `map.ps.make_mut()` write after an intern is the (test-exercised)
    /// bypass the cached-path `debug_assert`s police.
    pub fn make_mut(&mut self) -> &mut Vec<Par> {
        Arc::make_mut(&mut self.0)
    }

    /// Consume into an owned `Vec<Par>`: the payload is MOVED out when this
    /// is the only holder (O(1)), cloned otherwise (copy-on-extract — the
    /// by-value census sites, e.g. `graft`'s `extend(source.ps)`, paid this
    /// copy implicitly before L2 when the source map itself was cloned).
    pub fn into_vec(self) -> Vec<Par> {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone())
    }

    /// `true` iff `self` and `other` share one payload allocation — the
    /// L2 test seam for asserting O(1) clone sharing and CoW detachment.
    /// Representation-only: never part of value semantics.
    pub fn ptr_eq(&self, other: &SharedPars) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl From<Vec<Par>> for SharedPars {
    /// The construct-then-freeze entry: build a `Vec<Par>`, wrap it once.
    /// Keeps every `EPathMap::new(vec…)` call site compiling unchanged
    /// (`impl Into<SharedPars>` on the constructor).
    fn from(ps: Vec<Par>) -> Self {
        SharedPars(Arc::new(ps))
    }
}

impl std::ops::Deref for SharedPars {
    type Target = Vec<Par>;
    /// Transparent reads: every `&self` `Vec` API (`len`, `iter`, `first`,
    /// indexing, `as_slice`, …) and `&SharedPars` → `&Vec<Par>` → `&[Par]`
    /// coercion works as before L2. No `DerefMut` counterpart — mutation
    /// is exclusively [`SharedPars::make_mut`] (loud, CoW).
    fn deref(&self) -> &Vec<Par> {
        &self.0
    }
}

impl<'a> IntoIterator for &'a SharedPars {
    type Item = &'a Par;
    type IntoIter = std::slice::Iter<'a, Par>;
    /// `for p in &map.ps` parity with the pre-L2 `&Vec<Par>` field.
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Clone for SharedPars {
    /// O(1): an `Arc` refcount bump — the L2 point. The payload is shared,
    /// never copied; a later mutation on either side detaches via CoW.
    fn clone(&self) -> Self {
        SharedPars(Arc::clone(&self.0))
    }
}

impl Default for SharedPars {
    /// An empty payload in a fresh allocation (`Vec::new` itself does not
    /// allocate; only the `Arc` control block is created).
    fn default() -> Self {
        SharedPars(Arc::new(Vec::new()))
    }
}

impl fmt::Debug for SharedPars {
    /// Transparent: prints exactly as the inner `Vec<Par>` did before L2
    /// (the `EPathMap` Debug output is byte-identical).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for SharedPars {
    /// Delegates to `Vec<Par>` equality (the pre-L2 semantics), with an
    /// `Arc::ptr_eq` fast path — sound because `Par: Eq` (equality is
    /// reflexive), so one shared allocation is always equal to itself.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Eq for SharedPars {}

impl Hash for SharedPars {
    /// Delegates to the `Vec<Par>` hash — identical hash input stream to
    /// the pre-L2 field (length prefix + per-element hashes), so every
    /// hash-keyed container sees unchanged keys.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Ord for SharedPars {
    /// Delegates to `Vec<Par>` lexicographic ordering — the pre-L2
    /// semantics `EPathMap`'s derived-order `cmp` chain relies on.
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for SharedPars {
    /// Consistent with [`Ord`].
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl serde::Serialize for SharedPars {
    /// Transparent seq: serializes exactly as the inner `Vec<Par>` (bincode
    /// = u64-LE length + elements; JSON = array) — the `EPathMap` serde
    /// layout is byte-identical to pre-L2 (P0 serde goldens + the
    /// derived-twin differential gate it). Deliberately NOT `Arc`'s serde
    /// (serde's `rc` impls are feature-gated and layout-equivalent anyway);
    /// delegation to the payload keeps the layout obligation explicit.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_slice().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SharedPars {
    /// Reads a plain `Vec<Par>` (the exact pre-L2 wire shape) and wraps it
    /// in a fresh unshared `Arc`.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<Par>::deserialize(deserializer).map(SharedPars::from)
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
    /// Path entries (proto tag 1, repeated `Par`). Stage L2: an Arc-backed
    /// [`SharedPars`] — clone = refcount bump, reads transparent via
    /// `Deref`, writes CoW via [`EPathMap::ps_make_mut`] /
    /// [`SharedPars::make_mut`]. Serde/prost/`==`/`Hash`/`Ord` all read
    /// through to the payload — layout and semantics identical to the
    /// pre-L2 `Vec<Par>` field (P0 goldens). The schema stays `Vec<Par>`
    /// (the wrapper is invisible to OpenAPI, as to every other consumer).
    #[schema(value_type = Vec<Par>)]
    pub ps: SharedPars,
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
    /// The P3 shadow cell: the interned entry for THIS value's canonical
    /// prost bytes, filled at the first intern rendezvous and propagated by
    /// `Clone`. Reset by `merge_field`/`clear`. Never serialized, never
    /// compared, never hashed, never printed.
    #[serde(skip)]
    #[schema(ignore)]
    intern: OnceLock<Arc<InternedEPathMap>>,
}

impl serde::Serialize for EPathMap {
    /// Hand-written (the P3 derive is dropped) so that a GROUND map's serde
    /// bytes — the bincode/JSON EVENT-HASH PREIMAGE — are a PURE FUNCTION OF
    /// THE ENTRY SET: producer-, construction-order-, and multiplicity-
    /// independent. This closes the flaky `spliced_matches_direct_arbitrary`
    /// root cause (the intern entry is content-addressed by the order-
    /// insensitive `U(m)` yet the old derive cached an order-SENSITIVE serde
    /// preimage) and the underlying consensus hazard (two producers building
    /// the same ground map in different orders emitting different event-hash
    /// bytes).
    ///
    /// Layout — the SAME 4-field struct as the P3 derived twin
    /// (`serialize_struct("EPathMap", 4)` over `ps`, `locally_free`,
    /// `connective_used`, `remainder`; the `intern` cell is never serialized):
    ///
    ///   * GROUND (`eval_stable_epathmap(self) && !self.ps.is_empty()`): `ps`
    ///     in CANONICAL trie order (deduped, recursively canonical) via
    ///     [`ground_canonical_ps`] instead of construction order. The metadata
    ///     fields are already at their ground defaults (the predicate forces
    ///     `locally_free` empty, `!connective_used`, `remainder == None`), so
    ///     the uniform tail below emits `false` / `None` verbatim — matching
    ///     the GROUND arm of `spliced_event_bytes::emit_epathmap`.
    ///   * NON-GROUND / empty: BYTE-IDENTICAL to the P3 derived layout — `ps`
    ///     as-is (the `SharedPars` transparent seq), then the same tail.
    ///
    /// `locally_free` is ALWAYS emitted as EMPTY bytes (the serialize-only
    /// normalization the dropped `serialize_with = serialize_as_empty_bytes`
    /// attribute used to provide); the derived `Deserialize` is retained and
    /// still reads the stream's REAL `locally_free` bytes (the documented
    /// serialize-only asymmetry, plan amendment PM-1).
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
        if eval_stable_epathmap(self) && !self.ps.is_empty() {
            // GROUND: canonical trie order (producer-independent). The tail
            // below emits the ground defaults the predicate guarantees.
            state.serialize_field("ps", &ground_canonical_ps(self))?;
        } else {
            // NON-GROUND / empty: the `SharedPars` transparent seq, unchanged.
            state.serialize_field("ps", &self.ps)?;
        }
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
    /// Stage L2: `ps` is `impl Into<SharedPars>`, so every P3-migrated call
    /// site passing a `Vec<Par>` compiles unchanged (`From<Vec<Par>>` —
    /// construct-then-freeze), and callers already holding a [`SharedPars`]
    /// (e.g. rebuilding around an existing payload) pass it through as an
    /// O(1) `Arc` bump (the reflexive `Into`).
    pub fn new(
        ps: impl Into<SharedPars>,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<Var>,
    ) -> Self {
        EPathMap {
            ps: ps.into(),
            locally_free,
            connective_used,
            remainder,
            intern: OnceLock::new(),
        }
    }

    /// Stage L2's SANCTIONED `ps` mutator: takes the shadow cell FIRST
    /// (exactly the `merge_field`/`clear` reset discipline — a mutated
    /// value must re-derive its bytes from the real fields), then hands out
    /// the copy-on-write `&mut Vec<Par>` (`Arc::make_mut`: O(1) when the
    /// payload is unshared, one deep-clone when shared — paid only on
    /// actual mutation, never on clone).
    ///
    /// Every production in-place `ps` write (the zipper write methods:
    /// `setLeaf` push, `removeLeaf` pop, `graft` extend) routes through
    /// here; construct-then-freeze sites use [`EPathMap::new`] instead.
    /// Pinned by `ps_make_mut_resets_the_cell` (sanctioned path) and
    /// `stale_cell_mutation_is_policed_in_debug_builds` (the raw
    /// `map.ps.make_mut()` bypass still trips the cached-path
    /// `debug_assert`s).
    pub fn ps_make_mut(&mut self) -> &mut Vec<Par> {
        self.intern.take();
        self.ps.make_mut()
    }

    /// Intern this EPathMap: return the shared [`InternedEPathMap`] for its
    /// canonical prost bytes.
    ///
    /// First touch (per clone family): one streamed digest walk + store
    /// rendezvous (`intern_epathmap_via_store` — build on miss), then the
    /// cell fills. Every later touch on this instance OR any clone made
    /// after the fill is an O(1) cell read — THE post-P2 digest-pipeline
    /// kill. Concurrent first touches on one instance race benignly inside
    /// `OnceLock::get_or_init` (the store dedups to a single `Arc`; the
    /// nested `intern.get()` calls from `encode_raw` during initialization
    /// see `None` and take the field walk — `OnceLock::get` never blocks).
    ///
    /// NOTE: a cell hit does NOT refresh the store's LRU tick (the store is
    /// not consulted). An evicted bucket costs a rebuild only when a NEW
    /// instance of the same bytes interns; live families keep their handles
    /// through the `Arc`.
    pub fn intern(&self) -> Arc<InternedEPathMap> {
        Arc::clone(
            self.intern
                .get_or_init(|| intern_epathmap_via_store(self)),
        )
    }

    /// P4.3: read-only shadow-cell peek — `Some` iff the intern rendezvous
    /// has filled the cell. The spliced event-hash emitter
    /// (`spliced_event_bytes`) keys its intern-aware path on this WITHOUT
    /// forcing an intern (hashing must never mutate intern-store state; an
    /// unfilled map simply serializes directly).
    pub fn interned_handle(&self) -> Option<&Arc<InternedEPathMap>> { self.intern.get() }

    /// TEST SEAM: the shadow cell's current content (`None` = unfilled).
    /// Integration tests use this to pin cell propagation/reset semantics
    /// without triggering an intern.
    #[doc(hidden)]
    pub fn shadow_cell_for_test(&self) -> Option<&Arc<InternedEPathMap>> {
        self.interned_handle()
    }

    /// The UNCACHED field-by-field prost encode — the prost-derive 0.14.3
    /// expansion for this message shape, verbatim (tag order 1, 3, 4, 5;
    /// scalar fields skipped at their proto defaults). `pub(crate)` so the
    /// store's K2 verify and the stale-cell `debug_assert` can stream the
    /// REAL fields even when the cell is filled.
    pub(crate) fn encode_raw_fields(&self, buf: &mut impl BufMut) {
        for msg in &self.ps {
            encoding::message::encode(1u32, msg, buf);
        }
        // prost-derive emits `if self.locally_free != b"" as &[u8]`.
        if !self.locally_free.is_empty() {
            encoding::bytes::encode(3u32, &self.locally_free, buf);
        }
        // prost-derive emits `if self.connective_used != false`.
        if self.connective_used {
            encoding::bool::encode(4u32, &self.connective_used, buf);
        }
        if let Some(ref msg) = self.remainder {
            encoding::message::encode(5u32, msg, buf);
        }
    }

    /// The UNCACHED field-by-field `encoded_len` (prost-derive expansion,
    /// same skip-at-default structure as [`Self::encode_raw_fields`]).
    pub(crate) fn encoded_len_fields(&self) -> usize {
        // L2: `as_slice()` resolves through the `SharedPars` Deref to the
        // same `&[Par]` the prost-derive expansion passed (a fully concrete
        // coercion — no reliance on inference through the newtype).
        encoding::message::encoded_len_repeated(1u32, self.ps.as_slice())
            + if !self.locally_free.is_empty() {
                encoding::bytes::encoded_len(3u32, &self.locally_free)
            } else {
                0
            }
            + if self.connective_used {
                encoding::bool::encoded_len(4u32, &self.connective_used)
            } else {
                0
            }
            + self
                .remainder
                .as_ref()
                .map_or(0, |msg| encoding::message::encoded_len(5u32, msg))
    }

    /// The stale-cell invariant check backing the cached-path
    /// `debug_assert`s: with the cell filled, the CURRENT fields must still
    /// stream-encode to the cached canonical bytes. Runs the FIELD walk
    /// (never the cached path — the cached path would trivially compare the
    /// cache against itself).
    fn cached_bytes_still_valid(&self, interned: &InternedEPathMap) -> bool {
        // Ground VALUE arm (non-empty `path_stream`): the CURRENT entries must
        // still walk to the cached U(m). Non-ground / empty: the pre-wire field
        // walk must still stream-encode to the cached canonical bytes.
        if !interned.path_stream.is_empty() {
            ground_path_stream(&self.ps) == interned.path_stream
        } else {
            fields_match_canonical_prost(self, &interned.canonical_prost)
        }
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
    /// clone family. Stage L2: `ps` is a [`SharedPars`], so its "clone" is
    /// an `Arc` bump too — the whole clone is O(1) AT THE NODE (the D2
    /// go-decision; only `locally_free` bytes and the small `remainder`
    /// still copy). Mutation after cloning is safe by CoW
    /// ([`EPathMap::ps_make_mut`] detaches the payload before writing).
    fn clone(&self) -> Self {
        EPathMap {
            ps: self.ps.clone(),
            locally_free: self.locally_free.clone(),
            connective_used: self.connective_used,
            remainder: self.remainder.clone(),
            intern: self.intern.clone(),
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
    fn encode_raw(&self, buf: &mut impl BufMut) {
        if let Some(interned) = self.intern.get() {
            debug_assert!(
                self.cached_bytes_still_valid(interned),
                "EPathMap shadow cell is STALE in encode_raw: the fields were mutated after the \
                 intern rendezvous filled the cell (the encode would emit the cached bytes of the \
                 PRE-mutation value). Mutating a possibly-interned EPathMap through its pub fields \
                 violates the P3 cell invariant — rebuild via EPathMap::new instead."
            );
            // THE digest-pipeline kill: one memcpy of the canonical bytes
            // instead of the O(map) field walk. Same bytes by construction
            // (canonical_prost IS this value's canonical encoding — field 8
            // U(m) for a ground map, or the field walk otherwise), gated by
            // the P0 prost goldens.
            buf.put_slice(&interned.canonical_prost);
            return;
        }
        // Empty cell (incl. during the intern rendezvous). A non-empty GROUND
        // map emits the VALUE arm: proto field 8 = U(m). These bytes are
        // IDENTICAL to the interned `canonical_prost`, so the digest is the
        // same whether or not the cell is filled (interning caches U(m); this
        // rare pre-intern path rebuilds the trie once). Non-ground / empty
        // maps take the pre-wire field walk (`ps` at tag 1).
        if eval_stable_epathmap(self) && !self.ps.is_empty() {
            encode_ground_field8(&ground_path_stream(&self.ps), buf);
            return;
        }
        self.encode_raw_fields(buf);
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
        self.intern.take();
        /// prost-derive parity: the error-context struct name pushed onto
        /// `DecodeError` paths.
        const STRUCT_NAME: &str = "EPathMap";
        match tag {
            1u32 => {
                // L2: CoW escape — the cell is already taken above, and a
                // freshly-decoded value's payload is unshared in practice
                // (`Arc::make_mut` is then O(1); a shared payload would be
                // detached, exactly the invariant CoW guarantees).
                let value = self.ps.make_mut();
                encoding::message::merge_repeated(wire_type, value, buf, ctx).map_err(
                    |mut error| {
                        error.push(STRUCT_NAME, "ps");
                        error
                    },
                )
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
                self.ps = entries.into();
                Ok(())
            }
            _ => encoding::skip_field(wire_type, tag, buf, ctx),
        }
    }

    #[inline]
    fn encoded_len(&self) -> usize {
        if let Some(interned) = self.intern.get() {
            debug_assert!(
                self.cached_bytes_still_valid(interned),
                "EPathMap shadow cell is STALE in encoded_len: the fields were mutated after the \
                 intern rendezvous filled the cell. Mutating a possibly-interned EPathMap through \
                 its pub fields violates the P3 cell invariant — rebuild via EPathMap::new instead."
            );
            // O(1): InternedEPathMap.encoded_len == canonical_prost.len()
            // == the canonical encoded_len of this value (P1 computed it; the
            // debug_assert re-certifies).
            return interned.encoded_len;
        }
        // Empty cell: a non-empty GROUND map's length is the field-8 (U(m))
        // length; non-ground / empty maps use the pre-wire field walk.
        if eval_stable_epathmap(self) && !self.ps.is_empty() {
            return ground_field8_len(&ground_path_stream(&self.ps));
        }
        self.encoded_len_fields()
    }

    fn clear(&mut self) {
        // prost-derive parity for the proto fields, plus the cell reset.
        // L2: `ps` DETACHES to a fresh empty payload instead of clearing in
        // place — O(1) even when shared (a `make_mut().clear()` would
        // deep-clone a shared payload only to empty it), and observationally
        // identical (`clear` promises fields-at-defaults, nothing about
        // capacity).
        self.intern.take();
        self.ps = SharedPars::default();
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
    fn eq(&self, other: &Self) -> bool {
        self.ps == other.ps
            && self.connective_used == other.connective_used
            && self.remainder == other.remainder
    }
}

impl Eq for EPathMap {}

impl Hash for EPathMap {
    /// AlwaysEqual semantics: consistent with `==` (`locally_free` and the
    /// cell excluded).
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
    /// `locally_free` IS compared here although `==` ignores it: two maps
    /// can be `==` yet `cmp` to `Less`/`Greater`. That inconsistency is
    /// pinned 84a0fbe4 behavior (P0 Ord fixtures + the wrapper wart test) —
    /// sorted containers and hash containers deliberately disagree on such
    /// pairs, and "fixing" it would move sort orders consensus-visibly.
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
