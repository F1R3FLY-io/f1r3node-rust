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







// ─────────────────────────────────────────────────────────────────────────────
// ⛔ Section 2 ("cached == computed") is GONE with the intern cell. Every test in it
// compared a cached value against a recomputed one; with no cache both sides are the
// same expression, so the assertions are not weakened but TAUTOLOGICAL.
// ─────────────────────────────────────────────────────────────────────────────


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

    // ⛔ Was: intern both, then re-assert every comparison to pin that a filled cell is
    // compare-invisible. With no cell there is nothing to be invisible; the comparisons
    // themselves are asserted above and stay.
}

/// Debug parity: the four proto fields in declaration order, no cell.
#[test]
fn debug_output_shows_the_four_proto_fields_and_no_cell() {
    let _guard = interning_guard();
    let map = epathmap_remainder_connective();
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
    let before = map.ps().clone();
    // ★ Was `interned.canonical_prost` — the cached bytes. With no cache, capture the
    // original's OWN encoding here, before any sibling mutates. The property under test
    // is unchanged: a sibling's mutation must not touch this value's bytes.
    let before_bytes = prost::Message::encode_to_vec(&map);

    let mut mutated = map.clone();
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
        before_bytes.as_slice(),
        "the original is untouched by the sibling's mutation"
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
