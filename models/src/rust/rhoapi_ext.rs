//! EPathMap fix P3 — the hand-maintained `EPathMap` wrapper (full T1, stage
//! L1.5).
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
//!   first-touch digest walk is paid once per family, not per copy. The
//!   `ps` deep-copy REMAINS — the stated L1.5 limitation (stage L2, shared
//!   `ps`, is user decision D2).
//! * `Default`/`Debug` — prost-derive generates both alongside `Message`;
//!   replicated here (Debug prints the four proto fields in declaration
//!   order and omits the cell, matching the old derived output).
//! * serde `Serialize`/`Deserialize` — DERIVED, with `#[serde(skip)]` on the
//!   cell: serde_derive emits `serialize_struct("EPathMap", 4)` + the four
//!   fields in declaration order, with `locally_free` routed through
//!   `serialize_as_empty_bytes` — the SAME macro expansion the generated
//!   struct had (`models/build.rs` injects the identical field attribute),
//!   so the layout is identical BY CONSTRUCTION. The asymmetry is
//!   serialize-ONLY: deserialize reads real `locally_free` bytes from the
//!   stream (no `deserialize_with`), exactly as before. Gated by the P0
//!   serde goldens + the derived-twin proptest differential.
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
//! MUTATION DISCIPLINE (the shadow-cell invariant): the proto fields stay
//! `pub` (the ~56 read sites and the struct patterns rely on it), so Rust
//! cannot forbid `map.ps.push(..)` after the cell fills. The invariant —
//! "a filled cell certifies the fields still encode to `canonical_prost`" —
//! holds because the cell fills ONLY at the interpreter's intern rendezvous
//! (evaluated values treated as immutable thereafter; the workspace survey
//! found exactly one production field-write, the normalizer's fresh
//! `tmp_e_pathmap`, which can never have been interned), and every
//! `&mut`-path through `Message` (`merge_field`/`clear`) resets the cell.
//! `debug_assert`s in `encoded_len`/`encode_raw` re-verify the fields
//! against the cached bytes on every cached use, so the entire test fleet
//! (debug assertions on) polices the invariant continuously; the P0
//! goldens/differentials gate release behavior.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::DecodeError;

use super::pathmap_crate_type_mapper::{
    fields_match_canonical_prost, intern_epathmap_via_store, InternedEPathMap,
};
use crate::rhoapi::{Par, Var};

/// The hand-maintained mirror of `message EPathMap` (`RhoTypes.proto:321`),
/// extended with the P3 shadow cell. Field order and types are EXACTLY the
/// generated struct's (`ps`, `locally_free`, `connective_used`, `remainder`)
/// — serde layout, `Ord`, and `Debug` all depend on that order.
///
/// Construction: out-of-module struct literals are impossible (the cell is
/// private) — use [`EPathMap::new`] or [`Default`] (amendment PM-2; every
/// former literal site is migrated). Struct PATTERNS with `..` keep working.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct EPathMap {
    /// Path entries (proto tag 1, repeated `Par`).
    pub ps: Vec<Par>,
    /// Free-variable bitset (proto tag 3, bytes). Serde serializes this as
    /// EMPTY bytes (serialize-only normalization, `models/build.rs` parity);
    /// prost bytes RETAIN it; `==`/`Hash` ignore it; `Ord` compares it.
    #[serde(serialize_with = "crate::rust::serde_helpers::serialize_as_empty_bytes")]
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

impl EPathMap {
    /// The PM-2 constructor — the replacement for every former struct
    /// literal (the private cell makes out-of-module literals impossible).
    /// The cell starts EMPTY: a newly built value has no interned handle
    /// until its first rendezvous.
    pub fn new(
        ps: Vec<Par>,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<Var>,
    ) -> Self {
        EPathMap {
            ps,
            locally_free,
            connective_used,
            remainder,
            intern: OnceLock::new(),
        }
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
        encoding::message::encoded_len_repeated(1u32, &self.ps)
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
        fields_match_canonical_prost(self, &interned.canonical_prost)
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
    /// clone family. `ps` remains a deep copy (the stated L1.5 limitation;
    /// stage L2 shared-`ps` is user decision D2).
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
            // (canonical_prost IS this value's field-walk encoding), gated
            // by the P0 prost goldens.
            buf.put_slice(&interned.canonical_prost);
            return;
        }
        self.encode_raw_fields(buf);
    }

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
                let value = &mut self.ps;
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
            // == the field-walk encoded_len of this value (P1 computed it
            // from these very fields; the debug_assert re-certifies).
            return interned.encoded_len;
        }
        self.encoded_len_fields()
    }

    fn clear(&mut self) {
        // prost-derive parity for the proto fields, plus the cell reset.
        self.intern.take();
        self.ps.clear();
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
