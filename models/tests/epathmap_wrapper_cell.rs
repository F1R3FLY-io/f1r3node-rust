//! EPathMap fix P3 — WRAPPER + SHADOW-CELL suite.
//!
//! Pins the hand-maintained `EPathMap` wrapper's contract (the extern_path
//! type in `models/src/rust/rhoapi_ext.rs`):
//!
//!   1. CELL PROPAGATION — `Clone` carries the filled handle (clones made
//!      BEFORE the fill stay empty — `OnceLock` state is copied, not
//!      shared); the first `intern()` fills; the P1 shim
//!      (`e_pathmap_to_rholang_pathmap`) fills the CALLER's cell;
//!      `Message::merge`/`clear` RESET the cell.
//!   2. CACHED == COMPUTED — property tests: `encoded_len` with the cell
//!      filled equals the field-walk `encoded_len` (and the byte length);
//!      `encode_raw` with the cell filled emits the field-walk bytes.
//!      Includes the nested case (an interned INNER map serving cached
//!      bytes inside an outer field walk).
//!   3. SERDE — the derived-twin differential (bincode + JSON) against a
//!      struct carrying the OLD generated type's exact serde shape; the
//!      serialize-ONLY `locally_free` asymmetry (round-trips come back
//!      empty; hand-crafted streams with real bytes deserialize verbatim).
//!   4. THE ORD/ALWAYSEQUAL WART — `a == b` yet `a.cmp(&b) == Less` when
//!      only `locally_free` differs, before AND after interning.
//!   5. STALE-CELL POLICING — mutating a filled-cell value while bypassing
//!      the cell reset (the raw `SharedPars::make_mut`; the pre-L2 pub-field
//!      route no longer compiles) trips the wrapper's `debug_assert` on the
//!      next cached use (debug builds; the invariant's continuous test-fleet
//!      police).
//!   6. L2 SHARED-`ps` — `Clone` shares the payload (`SharedPars::ptr_eq`,
//!      O(1) at the node) with unchanged value semantics; the sanctioned
//!      mutator `ps_make_mut` takes the cell AND CoW-detaches, isolating
//!      clone siblings and the intern store from the write.
//!
//! The byte-level truth is separately gated by the P0 goldens
//! (`epathmap_canonical_fixtures.rs` — unchanged, still asserting the
//! committed 84a0fbe4 bytes).

mod fixtures;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use models::create_bit_vector;
use models::rhoapi::{EPathMap, Par, Var};
use models::rust::pathmap_crate_type_mapper::{
    intern_store_touches_for_test, PathMapCrateTypeMapper,
};
use proptest::prelude::*;
use prost::Message;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par,
    epathmap_remainder_connective, ezipper_value, gstring_par, ground_list, nested_epathmap_value,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Governs this binary's access to the PROCESS-WIDE intern store.
///
/// # ★ Why a reader/writer split, and why EVERY interning test must hold one
///
/// `cargo test` runs a binary's tests as threads in ONE process, so the intern
/// store is shared by every test in this file. (`cargo nextest` forks a process
/// per test and shares nothing — which is why a defect here reads green there
/// and red under `cargo test`; that divergence was the whole bug.)
///
/// Two different assertions need two different amounts of exclusion:
///
/// * **EXCLUSIVE ([`store_guard`])** — a test that asserts something about the
///   store *as a whole*: `Arc::ptr_eq` between DIFFERENT instances (which needs
///   the bucket to survive between the two interns, so an eviction-heavy
///   neighbour must not run), or a measurement of the store-touch counter
///   (which is only attributable to the call under test if nothing else interns
///   during the window).
/// * **SHARED ([`interning_guard`])** — a test that merely *interns* and then
///   asserts something instance-local (bytes, lengths, serde layout). It has no
///   stake in the store's global state, so these may run concurrently with each
///   other; they need only be excluded from the exclusive holders.
///
/// ⚠ The obligation is TOTAL: a store-touching test that holds neither guard
/// silently invalidates every exclusive measurement in the file. That is the
/// defect this file carried — six interning tests held nothing, so the
/// "…must not touch the global store at all" assertion was reading a number
/// its neighbours had written.
static STORE_LOCK: RwLock<()> = RwLock::new(());

/// EXCLUSIVE access — see [`STORE_LOCK`].
fn store_guard() -> RwLockWriteGuard<'static, ()> {
    STORE_LOCK
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// SHARED access — see [`STORE_LOCK`]. Held by every test that interns without
/// asserting anything about the store's global state.
fn interning_guard() -> RwLockReadGuard<'static, ()> {
    STORE_LOCK
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn std_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// A structurally-equal FRESH twin (empty cell) — field-walk encodes.
fn fresh_twin(map: &EPathMap) -> EPathMap {
    EPathMap::new(
        map.ps().clone(),
        map.locally_free.clone(),
        map.connective_used,
        map.remainder.clone(),
    )
}

fn all_fixture_maps() -> Vec<(&'static str, EPathMap)> {
    vec![
        ("e6a_index", e6a_index_epathmap()),
        ("nested", nested_epathmap_value()),
        (
            "ezipper_inner",
            ezipper_value()
                .pathmap
                .expect("the EZipper fixture carries a map"),
        ),
        ("locally_free", epathmap_locally_free_entries()),
        ("remainder_connective", epathmap_remainder_connective()),
    ]
}

/// Small generator over the wrapper's full field envelope: ground-string
/// list entries, optionally a NESTED EPathMap entry, optional non-empty
/// `locally_free`, optional remainder, either `connective_used`.
fn arb_epathmap() -> impl Strategy<Value = EPathMap> {
    (
        proptest::collection::vec("[a-z]{1,6}", 0..4),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        proptest::option::of(0usize..3),
    )
        .prop_map(
            |(labels, nested, with_remainder, connective_used, locally_free_bit)| {
                let mut entries: Vec<Par> = labels
                    .iter()
                    .map(|label| ground_list(vec![gstring_par(label), gstring_par("v")]))
                    .collect();
                if nested {
                    let inner = EPathMap::new(
                        vec![ground_list(vec![gstring_par("inner"), gstring_par("leaf")])],
                        Vec::new(),
                        false,
                        None,
                    );
                    entries.push(ground_list(vec![
                        gstring_par("nest"),
                        epathmap_par(inner),
                    ]));
                }
                EPathMap::new(
                    entries,
                    locally_free_bit
                        .map(|bit| create_bit_vector(&[bit]))
                        .unwrap_or_default(),
                    connective_used,
                    with_remainder.then(|| Var {
                        var_instance: Some(
                            models::rhoapi::var::VarInstance::FreeVar(0),
                        ),
                    }),
                )
            },
        )
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Cell propagation / fill / reset
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn first_touch_fills_and_clone_carries_the_handle() {
    let _guard = store_guard();
    let map = e6a_index_epathmap();
    assert!(
        map.shadow_cell_for_test().is_none(),
        "a freshly built value has an EMPTY cell"
    );

    let pre_fill_clone = map.clone();

    let interned = map.intern();
    assert!(
        map.shadow_cell_for_test().is_some(),
        "the first intern() fills the instance's cell"
    );
    assert!(
        pre_fill_clone.shadow_cell_for_test().is_none(),
        "clones made BEFORE the fill copied an empty cell (no retroactive sharing)"
    );

    let post_fill_clone = map.clone();
    let carried = post_fill_clone
        .shadow_cell_for_test()
        .expect("the handle travels with clones made AFTER the fill");
    assert!(
        Arc::ptr_eq(carried, &interned),
        "the clone carries the SAME interned Arc"
    );

    // A cell hit is the O(1) rendezvous: same Arc, no rebuild.
    let via_clone = post_fill_clone.intern();
    assert!(Arc::ptr_eq(&via_clone, &interned));
    let again = map.intern();
    assert!(Arc::ptr_eq(&again, &interned), "intern() is idempotent");

    // The pre-fill clone rendezvouses through the STORE and dedups to the
    // same entry (content addressing).
    let via_store = pre_fill_clone.intern();
    assert!(Arc::ptr_eq(&via_store, &interned));
}

#[test]
fn the_p1_shim_fills_the_callers_cell() {
    let _guard = store_guard();
    let map = nested_epathmap_value();
    assert!(map.shadow_cell_for_test().is_none());

    // ★ **THE SHIM NO LONGER INTERNS**, and this test is rewritten to pin that
    // rather than deleted.
    //
    // It used to assert that `e_pathmap_to_rholang_pathmap` filled the caller's
    // shadow cell, because the conversion's whole job was to BUILD A TRIE from
    // `ps` and the store existed so the build happened once per content. The
    // rendezvous cost a streamed Blake2b digest walk, a global mutex, and a K2
    // full-prost byte verify — worth caching, hence the cell.
    //
    // The map holds the trie. The conversion is an O(1) handle clone plus two
    // folds the `EntryTrie` maintains as entries arrive, so there is nothing
    // left to amortize and reaching for the store would be pure cost. The
    // property to pin is therefore the ABSENCE: a conversion must not intern.
    // ⚠ The store observation is a DELTA on the touch counter, taken inside the
    // exclusive guard — NOT an absolute, and NOT a delta on the bucket count.
    //
    // `assert_eq!(intern_store_len_for_test(), 0)` — what this used to say —
    // asserted that NOTHING IN THE PROCESS had ever interned. Under
    // `cargo nextest` (a process per test) that happened to hold; under
    // `cargo test` (threads in one process) the neighbours below had already
    // filled the store, and the observed value moved run to run (5, 27, 35, 64)
    // because it was reporting the schedule rather than this conversion.
    //
    // A delta on `intern_store_len_for_test` would not fix it: at
    // INTERN_CAPACITY every insert evicts one bucket and adds one, so the
    // length delta is ZERO for a call that did touch the store — and a full
    // binary drives the store to capacity routinely (64 was one of the observed
    // absolutes). The counter below is monotone and is bumped under the store
    // mutex on every hit and every insert, so eviction cannot mask it.
    let touches_before = intern_store_touches_for_test();
    let converted = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&map);
    let touches_after = intern_store_touches_for_test();
    assert!(
        map.shadow_cell_for_test().is_none(),
        "a trie conversion must not force an intern any more — the map already \
         holds the trie, so the rendezvous would be cost with no benefit"
    );
    // ★ NOT redundant with the cell assertion above. An empty cell says only
    // that `map.intern()` was not called on THIS instance; a conversion could
    // still reach the store by interning the fixture's NESTED inner map, or a
    // temporary clone, or by calling `intern_epathmap_via_store` directly —
    // none of which fills `map`'s cell. This assertion is the one that covers
    // those, and `nested_epathmap_value()` was chosen precisely because it
    // carries a nested map for the first of them to be reachable.
    assert_eq!(
        touches_after - touches_before,
        0,
        "…and it must not touch the global store at all"
    );

    // …and it still answers with the same metadata the store used to compute.
    let interned = map.intern();
    assert_eq!(converted.connective_used, interned.connective_used);
    assert_eq!(converted.locally_free, interned.locally_free);
}

/// ★ THE CONTROL for the assertion above: the observable it uses is LIVE.
///
/// An assertion that something did NOT happen is worthless unless the
/// instrument would have noticed if it had. This test drives the store
/// deliberately — a miss (insert) and a hit (the same content again, from a
/// FRESH instance so the shadow cell cannot short-circuit it) — and requires
/// the counter to move for each.
///
/// It is a permanent fixture, not scaffolding: it fails if anyone makes
/// `intern_store_touches_for_test` constant, moves a `next_intern_tick` call
/// out from under a store path, or adds a store path that forgets to tick — any
/// of which would turn "must not touch the global store" into an assertion that
/// cannot fail. It shares the EXCLUSIVE guard for the same reason the
/// measurement does.
#[test]
fn the_store_touch_counter_moves_when_the_store_is_actually_touched() {
    let _guard = store_guard();

    let map = e6a_index_epathmap();

    // MISS: never-before-seen content ⇒ build + insert ⇒ one tick.
    let before_miss = intern_store_touches_for_test();
    let interned = map.intern();
    let after_miss = intern_store_touches_for_test();
    assert!(
        after_miss > before_miss,
        "an intern that reaches the store must advance the touch counter \
         (miss/insert path); the counter is the instrument the \
         'must not touch the store' assertion reads, so a constant here would \
         make that assertion vacuous"
    );

    // HIT: a structurally-equal FRESH instance has an EMPTY cell, so it cannot
    // short-circuit and must rendezvous through the store ⇒ another tick.
    let twin = fresh_twin(&map);
    assert!(
        twin.shadow_cell_for_test().is_none(),
        "precondition: the twin's cell is empty, so it must reach the store"
    );
    let before_hit = intern_store_touches_for_test();
    let via_store = twin.intern();
    let after_hit = intern_store_touches_for_test();
    assert!(
        after_hit > before_hit,
        "a store HIT must advance the touch counter too — otherwise a \
         conversion that only ever hit the store would be invisible"
    );
    assert!(
        Arc::ptr_eq(&via_store, &interned),
        "precondition: the twin really did rendezvous with the same entry \
         (so the tick above was a hit, not a second insert)"
    );
}

#[test]
fn merge_resets_the_cell_and_reencodes_from_fields() {
    let _guard = store_guard();
    let mut target = e6a_index_epathmap();
    let _ = target.intern();
    assert!(target.shadow_cell_for_test().is_some());

    // Proto merge semantics: repeated `ps` EXTENDS, scalars overwrite,
    // `remainder` merges. Any merge_field resets the cell.
    let extra = epathmap_remainder_connective();
    let extra_bytes = extra.encode_to_vec();
    target
        .merge(extra_bytes.as_slice())
        .expect("well-formed proto bytes must merge");
    assert!(
        target.shadow_cell_for_test().is_none(),
        "merge must RESET the shadow cell (fields changed)"
    );

    // The merged value re-encodes from its REAL fields.
    let reencoded = target.encode_to_vec();
    assert_eq!(
        reencoded,
        fresh_twin(&target).encode_to_vec(),
        "post-merge encoding must equal the field-walk encoding"
    );
    assert_eq!(
        target.ps().len(),
        e6a_index_epathmap().ps().len() + extra.ps().len(),
        "repeated ps must have EXTENDED under merge"
    );
}

#[test]
fn clear_resets_the_cell_and_the_fields() {
    let _guard = store_guard();
    let mut map = epathmap_locally_free_entries();
    let _ = map.intern();
    assert!(map.shadow_cell_for_test().is_some());

    map.clear();
    assert!(
        map.shadow_cell_for_test().is_none(),
        "clear must RESET the shadow cell"
    );
    assert!(map.ps().is_empty());
    assert!(map.locally_free.is_empty());
    assert!(!map.connective_used);
    assert!(map.remainder.is_none());
    assert_eq!(map.encoded_len(), 0, "a cleared map encodes to nothing");
}

#[test]
fn decode_produces_an_empty_cell() {
    let map = e6a_index_epathmap();
    let bytes = map.encode_to_vec();
    let decoded = EPathMap::decode(bytes.as_slice()).expect("decode the fixture bytes");
    assert!(
        decoded.shadow_cell_for_test().is_none(),
        "decode goes through Default + merge_field — the cell must be empty"
    );
    assert_eq!(
        decoded.encode_to_vec(),
        bytes,
        "decode/encode must round-trip byte-identically"
    );
    assert_eq!(decoded.locally_free, map.locally_free);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Cached == computed (property tests)
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// encoded_len with the cell FILLED == the field-walk encoded_len ==
    /// the encoding's byte length (the substitution-charge invariance
    /// carrier: same numbers whether or not the value was interned).
    #[test]
    fn cached_encoded_len_equals_computed(map in arb_epathmap()) {
        let _guard = interning_guard();
        let computed_len = map.encoded_len(); // cell empty: field walk
        let _ = map.intern();                 // fill
        let cached_len = map.encoded_len();   // cell filled: O(1) read
        prop_assert_eq!(cached_len, computed_len,
            "cached encoded_len must equal the field-walk encoded_len");
        prop_assert_eq!(cached_len, map.encode_to_vec().len(),
            "encoded_len must equal the encoding's byte length");
        // A structurally-equal fresh twin (field walk) agrees too.
        prop_assert_eq!(fresh_twin(&map).encoded_len(), cached_len);
    }

    /// encode_raw with the cell FILLED (the canonical-bytes memcpy) emits
    /// exactly the field-walk bytes.
    #[test]
    fn cached_encode_raw_equals_computed_bytes(map in arb_epathmap()) {
        let _guard = interning_guard();
        let computed = map.encode_to_vec();  // cell empty: field walk
        let _ = map.intern();                // fill
        let cached = map.encode_to_vec();    // cell filled: memcpy
        prop_assert_eq!(&cached, &computed,
            "cached encode bytes must equal the field-walk bytes");
        prop_assert_eq!(fresh_twin(&map).encode_to_vec(), cached);
    }
}

/// The nested composition: an interned INNER map (cached bytes) embedded in
/// an OUTER field walk must produce the same bytes as a fully-fresh tree.
#[test]
fn interned_inner_map_composes_byte_identically_in_an_outer_encode() {
    let _guard = interning_guard();
    let inner = EPathMap::new(
        vec![ground_list(vec![gstring_par("inner"), gstring_par("leaf")])],
        Vec::new(),
        false,
        None,
    );
    let _ = inner.intern(); // inner cell FILLED before embedding
    let outer_with_cached_inner = EPathMap::new(
        vec![ground_list(vec![gstring_par("nest"), epathmap_par(inner)])],
        Vec::new(),
        false,
        None,
    );

    let fully_fresh = nested_epathmap_value(); // same shape, all cells empty
    assert_eq!(
        outer_with_cached_inner.encode_to_vec(),
        fully_fresh.encode_to_vec(),
        "an interned inner map must serve byte-identical cached bytes inside an outer field walk"
    );
    assert_eq!(
        outer_with_cached_inner.encoded_len(),
        fully_fresh.encoded_len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Serde: derived-twin differential + the serialize-only asymmetry
// ─────────────────────────────────────────────────────────────────────────────

/// The CANONICAL serde-oracle twin: same struct name, field names and order,
/// the same `serialize_with` on `locally_free`, no cell.
///
/// ★ The twin's `ps` is simply `map.ps()`. It used to be
/// `canonicalize_ground_epathmap(map).ps` — a ground map's entries re-read off
/// a trie, other maps' entries verbatim — because the wrapper's `Serialize`
/// forked the same way. Neither forks now: the map STORES the trie, so its
/// projection is the canonical order for every map and the oracle is the plain
/// field read again.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "EPathMap")]
struct DerivedTwin {
    ps: Vec<Par>,
    #[serde(serialize_with = "models::rust::serde_helpers::serialize_as_empty_bytes")]
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
}

fn derived_twin(map: &EPathMap) -> DerivedTwin {
    DerivedTwin {
        // The twin keeps a plain `Vec<Par>` (it IS the layout oracle) — an
        // owned copy of the map's canonical projection.
        ps: map.ps().clone(),
        locally_free: map.locally_free.clone(),
        connective_used: map.connective_used,
        remainder: map.remainder.clone(),
    }
}

/// The twin WITHOUT the serialize_with normalization — used to hand-craft
/// streams carrying REAL `locally_free` bytes (which the production
/// serializer can never emit) to pin the deserialize half of the asymmetry.
#[derive(serde::Serialize)]
#[serde(rename = "EPathMap")]
struct RawBytesTwin {
    ps: Vec<Par>,
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
}

proptest! {
    /// bincode + JSON differential vs the derived twin, before AND after
    /// interning (the cell must be serialization-invisible).
    #[test]
    fn serde_layout_matches_the_derived_twin(map in arb_epathmap()) {
        let _guard = interning_guard();
        let twin = derived_twin(&map);
        let twin_bincode = bincode::serialize(&twin).expect("twin bincode");
        let twin_json = serde_json::to_string(&twin).expect("twin json");

        prop_assert_eq!(
            &bincode::serialize(&map).expect("wrapper bincode"), &twin_bincode,
            "wrapper bincode must match the derived twin");
        prop_assert_eq!(
            &serde_json::to_string(&map).expect("wrapper json"), &twin_json,
            "wrapper JSON must match the derived twin");

        let _ = map.intern();
        prop_assert_eq!(
            &bincode::serialize(&map).expect("wrapper bincode interned"), &twin_bincode,
            "a FILLED cell must be bincode-invisible");
        prop_assert_eq!(
            &serde_json::to_string(&map).expect("wrapper json interned"), &twin_json,
            "a FILLED cell must be JSON-invisible");

        // Deserialize differential: the stream carries `ps` in canonical
        // order (that is the only order the serializer can emit), and decode
        // re-files it into a trie, which is idempotent on a canonical stream —
        // so a round trip lands back on `map.ps()`. `locally_free` is EMPTY on
        // this stream (the twin serialized it as empty).
        let de: EPathMap = bincode::deserialize(&twin_bincode).expect("wrapper de");
        prop_assert_eq!(de.ps(), map.ps());
        prop_assert!(de.locally_free.is_empty());
        prop_assert_eq!(de.connective_used, map.connective_used);
        prop_assert_eq!(&de.remainder, &map.remainder);
        prop_assert!(de.shadow_cell_for_test().is_none(),
            "deserialize must produce an EMPTY cell");
    }
}

#[test]
fn fixture_serde_matches_the_derived_twin_exactly() {
    for (name, map) in all_fixture_maps() {
        let twin = derived_twin(&map);
        assert_eq!(
            bincode::serialize(&map).expect("wrapper bincode"),
            bincode::serialize(&twin).expect("twin bincode"),
            "{name}: bincode drift vs the derived layout"
        );
        assert_eq!(
            serde_json::to_string_pretty(&map).expect("wrapper json"),
            serde_json::to_string_pretty(&twin).expect("twin json"),
            "{name}: JSON drift vs the derived layout"
        );
    }
}

/// THE ASYMMETRY, both halves: serialize NORMALIZES `locally_free` to empty
/// (a round trip loses the bits), while deserialize reads REAL bytes when a
/// (hand-crafted) stream carries them.
#[test]
fn serde_locally_free_asymmetry_serialize_normalizes_deserialize_reads() {
    // Serialize half: a bit-tagged map round-trips to EMPTY locally_free.
    let tagged = epathmap_locally_free_entries();
    assert!(!tagged.locally_free.is_empty(), "fixture precondition");
    let round: EPathMap =
        bincode::deserialize(&bincode::serialize(&tagged).expect("ser")).expect("de");
    assert!(
        round.locally_free.is_empty(),
        "serialize_as_empty_bytes must have normalized the map-level bitset"
    );
    for entry in round.ps() {
        assert!(
            entry.locally_free.is_empty(),
            "entry-level bitsets normalize too (every .rhoapi locally_free)"
        );
    }

    // Deserialize half: real bytes in the stream are read VERBATIM (no
    // deserialize-side normalization exists — the asymmetry is
    // serialize-only).
    let raw = RawBytesTwin {
        ps: vec![ground_list(vec![gstring_par("head")])],
        locally_free: create_bit_vector(&[3]),
        connective_used: true,
        remainder: None,
    };
    let crafted: EPathMap =
        bincode::deserialize(&bincode::serialize(&raw).expect("raw ser")).expect("raw de");
    assert_eq!(
        crafted.locally_free,
        create_bit_vector(&[3]),
        "deserialize must read the stream's REAL locally_free bytes"
    );
    assert!(crafted.connective_used);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. The Ord/AlwaysEqual wart survives the wrapper
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn always_equal_vs_derived_ord_wart_survives_the_wrapper() {
    let _guard = interning_guard();
    let plain = e6a_index_epathmap();
    let tagged = EPathMap::new(
        plain.ps().clone(),
        create_bit_vector(&[0]), // [] vs [0x01]
        plain.connective_used,
        plain.remainder.clone(),
    );

    // AlwaysEqual: equal + same hash (locally_free invisible).
    assert_eq!(plain, tagged, "== must ignore locally_free");
    assert_eq!(std_hash(&plain), std_hash(&tagged), "Hash must ignore locally_free");
    // Declaration-order Ord: NOT equal (locally_free visible; [] < [1]).
    assert_eq!(
        plain.cmp(&tagged),
        std::cmp::Ordering::Less,
        "a == b yet a.cmp(&b) == Less — the pinned wart"
    );
    assert_eq!(
        plain.partial_cmp(&tagged),
        Some(std::cmp::Ordering::Less),
        "PartialOrd must agree with Ord"
    );

    // Interning must not perturb any comparison (cells are compare-invisible).
    let _ = plain.intern();
    let _ = tagged.intern();
    assert_eq!(plain, tagged);
    assert_eq!(std_hash(&plain), std_hash(&tagged));
    assert_eq!(plain.cmp(&tagged), std::cmp::Ordering::Less);
}

/// Debug parity: the four proto fields in declaration order, no cell.
#[test]
fn debug_output_shows_the_four_proto_fields_and_no_cell() {
    let _guard = interning_guard();
    let map = epathmap_remainder_connective();
    let _ = map.intern(); // even filled, the cell must not print
    let debug = format!("{map:?}");
    assert!(debug.starts_with("EPathMap { ps: ["), "prost-style struct debug: {debug}");
    for field in ["ps", "locally_free", "connective_used", "remainder"] {
        assert!(debug.contains(field), "missing field {field} in {debug}");
    }
    assert!(!debug.contains("intern"), "the cell must be Debug-invisible: {debug}");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Stale-cell policing (debug builds)
// ─────────────────────────────────────────────────────────────────────────────

/// ★ **THE BYPASS DOES NOT EXIST** — this replaces the `should_panic` test that
/// used to pin its detection.
///
/// The deleted test read: mutate a filled-cell value through the raw
/// `SharedPars::make_mut` (which does not reset the cell), then touch a cached
/// path and watch the `debug_assert` fire. It was the *continuous police* for
/// the whole fleet, and it needed to be, because the stored `Vec<Par>` and the
/// cached canonical bytes were two representations of one value and a write
/// could reach the first without disturbing the second.
///
/// `ps` is the trie now, and the cached bytes are derived from it. There is no
/// `&mut Vec<Par>` to obtain — not from the map, not from the trie — so the
/// mutation that test performed cannot be written. What is checkable is the
/// positive statement that replaced it: **every route that can change a map
/// takes the cell**, so a stale one is never constructed in the first place.
///
/// This test enumerates ALL of them (the three entry mutators plus the two
/// `Message` paths) and, for each, asserts the cell is empty afterwards and the
/// bytes re-derive to those of a freshly built twin.
#[test]
fn every_mutator_takes_the_shadow_cell_so_a_stale_one_is_unconstructible() {
    let _guard = store_guard();

    let probe = ground_list(vec![gstring_par("mutationProbe")]);

    let mutators: Vec<(&str, Box<dyn Fn(&mut EPathMap)>)> = vec![
        (
            "insert_entry",
            Box::new({
                let probe = probe.clone();
                move |map: &mut EPathMap| map.insert_entry(probe.clone())
            }),
        ),
        (
            "extend_entries",
            Box::new({
                let probe = probe.clone();
                move |map: &mut EPathMap| {
                    let other = EPathMap::new(vec![probe.clone()], Vec::new(), false, None);
                    map.extend_entries(&other);
                }
            }),
        ),
        (
            "remove_greatest_entry",
            Box::new(|map: &mut EPathMap| {
                map.remove_greatest_entry();
            }),
        ),
        (
            "Message::clear",
            Box::new(|map: &mut EPathMap| prost::Message::clear(map)),
        ),
        (
            "Message::merge (decode into)",
            Box::new({
                let probe = probe.clone();
                move |map: &mut EPathMap| {
                    let addendum = EPathMap::new(vec![probe.clone()], Vec::new(), false, None);
                    let bytes = prost::Message::encode_to_vec(&addendum);
                    prost::Message::merge(map, bytes.as_slice()).expect("merge");
                }
            }),
        ),
    ];

    for (name, mutate) in mutators {
        let mut map = e6a_index_epathmap();
        let _ = map.intern();
        assert!(
            map.shadow_cell_for_test().is_some(),
            "{name}: precondition — the cell is filled before the mutation"
        );

        mutate(&mut map);

        assert!(
            map.shadow_cell_for_test().is_none(),
            "{name} must take the shadow cell"
        );
        let twin = fresh_twin(&map);
        assert_eq!(
            map.encoded_len(),
            twin.encoded_len(),
            "{name}: post-mutation encoded_len must re-derive from the entries"
        );
        assert_eq!(
            prost::Message::encode_to_vec(&map),
            prost::Message::encode_to_vec(&twin),
            "{name}: post-mutation bytes must re-derive from the entries"
        );
    }
}

/// ★ A mutation is invisible to every OTHER holder of the same entries —
/// without any copy-on-write discipline to get right.
///
/// The predecessor of this test (`ps_make_mut_resets_the_cell_and_cow_detaches_the_shared_payload`)
/// had to check that the sanctioned mutator CoW-detached an `Arc<Vec<Par>>`
/// before writing, because a shared payload was writable through any holder.
/// The trie is a persistent structure whose `insert` produces a new root, so
/// isolation is a property of the data structure rather than of a discipline
/// somebody has to remember at each write site.
#[test]
fn a_mutation_is_invisible_to_every_other_holder() {
    let _guard = store_guard();

    let map = e6a_index_epathmap();
    let interned = map.intern();
    let before = map.ps().clone();

    let mut mutated = map.clone();
    assert!(
        mutated.shadow_cell_for_test().is_some(),
        "precondition: the clone carries the filled cell"
    );
    assert!(
        mutated.entry_trie().view_ptr_eq(map.entry_trie()),
        "precondition: the clone shares the memoized projection (O(1) clone)"
    );

    mutated.insert_entry(ground_list(vec![gstring_par("isolationProbe")]));

    assert_eq!(
        map.ps(),
        &before,
        "the original's entries are untouched by the sibling's insert"
    );
    assert_eq!(
        map.ps().len() + 1,
        mutated.ps().len(),
        "…and the sibling gained exactly the one entry"
    );
    assert!(
        !mutated.entry_trie().view_ptr_eq(map.entry_trie()),
        "the mutated value no longer shares the projection"
    );
    assert_eq!(
        prost::Message::encode_to_vec(&map).as_slice(),
        interned.canonical_prost.as_slice(),
        "the interned original is untouched by the sibling's mutation"
    );
}

/// The representation contract: `EPathMap::clone` is O(1) at the node — the
/// trie clone is a refcount bump and the memoized projection is an `Arc` bump —
/// and the shared value is semantically indistinguishable from an owned deep
/// copy (`==`, `Ord`, hash, prost bytes, serde bytes).
#[test]
fn clone_shares_the_entry_projection_and_preserves_value_semantics() {
    for (name, map) in all_fixture_maps() {
        // Force the projection first: it is memoized lazily, so an unforced
        // source has nothing to share yet.
        let _ = map.ps();
        let clone = map.clone();
        assert!(
            clone.entry_trie().view_ptr_eq(map.entry_trie()),
            "{name}: clone must share the memoized projection (Arc bump, not deep copy)"
        );
        assert_eq!(map, clone, "{name}: shared clone must stay ==");
        assert_eq!(
            std::cmp::Ordering::Equal,
            map.cmp(&clone),
            "{name}: shared clone must compare Equal"
        );
        assert_eq!(std_hash(&map), std_hash(&clone), "{name}: hash parity");
        assert_eq!(
            prost::Message::encode_to_vec(&map),
            prost::Message::encode_to_vec(&clone),
            "{name}: prost byte parity through the shared payload"
        );
        assert_eq!(
            bincode::serialize(&map).expect("bincode map"),
            bincode::serialize(&clone).expect("bincode clone"),
            "{name}: serde byte parity through the shared payload"
        );
    }
}
