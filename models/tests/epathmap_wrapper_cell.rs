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
use std::sync::{Arc, Mutex, MutexGuard};

use models::create_bit_vector;
use models::rhoapi::{EPathMap, Par, Var};
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use proptest::prelude::*;
use prost::Message;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par,
    epathmap_remainder_connective, ezipper_value, gstring_par, ground_list, nested_epathmap_value,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes the tests whose assertions cross the process-wide intern store
/// (`Arc::ptr_eq` between DIFFERENT instances relies on the store bucket
/// surviving between the two interns — concurrent eviction-heavy tests could
/// otherwise race it). Instance-local (cell-only) assertions do not need it.
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn store_guard() -> MutexGuard<'static, ()> {
    STORE_LOCK
        .lock()
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
        map.ps.clone(),
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

    // The 56-call-site shim (and P2's fused rendezvous behind it): the
    // conversion's store rendezvous must fill the CALLER's cell so every
    // later touch on this instance/family is O(1).
    let converted = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&map);
    let cell = map
        .shadow_cell_for_test()
        .expect("e_pathmap_to_rholang_pathmap fills the caller's shadow cell");
    assert_eq!(converted.connective_used, cell.connective_used);
    assert_eq!(converted.locally_free, cell.locally_free);
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
        target.ps.len(),
        e6a_index_epathmap().ps.len() + extra.ps.len(),
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
    assert!(map.ps.is_empty());
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

/// The OLD generated type's serde shape, verbatim (what models/build.rs used
/// to emit): same struct name, same field names and order, the SAME
/// `serialize_with` on `locally_free`, no cell. The wrapper must be
/// serialization-indistinguishable from this twin.
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
        // L2: the twin keeps a plain `Vec<Par>` (it IS the pre-L2 layout
        // oracle) — extract an owned copy from the shared payload.
        ps: map.ps.to_vec(),
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

        // Deserialize differential: the wrapper reads the twin's stream to
        // the same field values (locally_free EMPTY on this stream — the
        // twin serialized it as empty).
        let de: EPathMap = bincode::deserialize(&twin_bincode).expect("wrapper de");
        prop_assert_eq!(&de.ps, &map.ps);
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
    for entry in &round.ps {
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
    let plain = e6a_index_epathmap();
    let tagged = EPathMap::new(
        plain.ps.clone(),
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

/// Mutating a filled-cell value while BYPASSING the cell reset violates the
/// cell invariant; the next cached use must trip the debug_assert LOUDLY
/// (this is the continuous police for the whole test fleet — release builds
/// trust the audited invariant: every production `ps` write routes through
/// `ps_make_mut`, which resets the cell).
///
/// L2 makes the bypass NARROWER and LOUDER than the pre-L2 pub-field hazard:
/// `mutated.ps.push(..)` no longer compiles (no `DerefMut` on `SharedPars`);
/// the only remaining bypass is the raw `SharedPars::make_mut` below, which
/// CoW-detaches the payload (the interned original is untouched) but leaves
/// the stale cell in place — exactly what this test pins.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "shadow cell is STALE")]
fn stale_cell_mutation_is_policed_in_debug_builds() {
    let map = e6a_index_epathmap();
    let _ = map.intern();
    let mut mutated = map.clone(); // carries the filled cell (and shares ps)
    mutated
        .ps
        .make_mut() // RAW bypass: no cell reset (ps_make_mut is the sanctioned route)
        .push(ground_list(vec![gstring_par("staleProbe")])); // invariant violation
    let _ = mutated.encoded_len(); // cached path → debug_assert fires
}

/// L2's sanctioned-mutation contract: `ps_make_mut` (a) TAKES the shadow
/// cell (a mutated value re-derives bytes from its real fields — no stale
/// cache can survive the sanctioned route), and (b) CoW-detaches the shared
/// payload (clone siblings and the intern store never observe the write).
#[test]
fn ps_make_mut_resets_the_cell_and_cow_detaches_the_shared_payload() {
    let _guard = store_guard();

    let map = e6a_index_epathmap();
    let interned = map.intern();
    let mut mutated = map.clone();
    assert!(
        mutated.shadow_cell_for_test().is_some(),
        "precondition: the clone carries the filled cell"
    );
    assert!(
        mutated.ps.ptr_eq(&map.ps),
        "precondition: the clone shares the ps payload (L2 O(1) clone)"
    );

    mutated.ps_make_mut().push(ground_list(vec![gstring_par("cowProbe")]));

    // (a) The cell is gone — encoded_len/encode_raw take the field walk and
    // agree with a fresh twin of the mutated fields (no stale-cache panic,
    // no stale bytes).
    assert!(
        mutated.shadow_cell_for_test().is_none(),
        "ps_make_mut must take the cell"
    );
    let twin = fresh_twin(&mutated);
    assert_eq!(mutated.encoded_len(), twin.encoded_len());
    assert_eq!(
        prost::Message::encode_to_vec(&mutated),
        prost::Message::encode_to_vec(&twin),
        "post-mutation bytes must re-derive from the REAL fields"
    );

    // (b) CoW isolation: the original and its interned entry still carry the
    // pre-mutation payload and bytes.
    assert!(
        !mutated.ps.ptr_eq(&map.ps),
        "ps_make_mut must detach the shared payload"
    );
    assert_eq!(map.ps.len() + 1, mutated.ps.len());
    assert_eq!(
        prost::Message::encode_to_vec(&map).as_slice(),
        interned.canonical_prost.as_slice(),
        "the interned original is untouched by the sibling's mutation"
    );
}

/// L2's representation contract: `EPathMap::clone` shares the `ps` payload
/// (one allocation, refcount bump — O(1) at the node) and the shared value
/// is semantically indistinguishable from an owned deep copy (`==`, `Ord`,
/// hash, prost bytes, serde bytes).
#[test]
fn clone_shares_the_ps_payload_and_preserves_value_semantics() {
    for (name, map) in all_fixture_maps() {
        let clone = map.clone();
        assert!(
            clone.ps.ptr_eq(&map.ps),
            "{name}: clone must share the ps payload (Arc bump, not deep copy)"
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
