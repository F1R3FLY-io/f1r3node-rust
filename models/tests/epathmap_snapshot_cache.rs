//! EPathMap representation, serde, and snapshot-cache contracts.
//!
//! The suite checks the hand-maintained external type against its serde twin,
//! pins the legacy `locally_free` asymmetry and comparison semantics, proves
//! Debug does not force serialization, and verifies that clones share immutable
//! PathMap/snapshot state while mutation detaches only the modified sibling.

mod fixtures;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par, epathmap_remainder_connective,
    ezipper_value, ground_list, gstring_par, nested_epathmap_value,
};
use models::create_bit_vector;
use models::rhoapi::{EPathMap, Par, Var};
use proptest::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn std_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
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
                    entries.push(ground_list(vec![gstring_par("nest"), epathmap_par(inner)]));
                }
                EPathMap::new(
                    entries,
                    locally_free_bit
                        .map(|bit| create_bit_vector(&[bit]))
                        .unwrap_or_default(),
                    connective_used,
                    with_remainder.then(|| Var {
                        var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
                    }),
                )
            },
        )
}

/// Serde oracle with the same field names/order. Its `ps` field is the direct
/// EPM1 byte array emitted by `EntryTrie::serialize`.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "EPathMap")]
struct DerivedTwin {
    ps: Vec<u8>,
    #[serde(serialize_with = "models::rust::serde_helpers::serialize_as_empty_bytes")]
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
}

fn derived_twin(map: &EPathMap) -> DerivedTwin {
    DerivedTwin {
        // The twin keeps plain owned values (it IS the layout oracle): the key
        // stream of the entries THIS SURFACE WRITES, and a copy of the map's
        // canonical projection.
        //
        // ⚠ `bincode_path_stream()`, not `path_stream()` (CBR-043). The `Vec<Par>`
        // beside it is serialized with every `locally_free` blanked, so the
        // oracle's key half must be the keys of the blanked entries — otherwise
        // the ORACLE would be asserting the defect and the wrapper would be
        // failing for being correct.
        ps: map.trie_snapshot().to_vec(),
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
    ps: Vec<u8>,
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
}

proptest! {
    /// Bincode + JSON differential against the derived schema twin.
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

        // Decode reconstructs the homogeneous PathMap directly from EPM1.
        let de: EPathMap = bincode::deserialize(&twin_bincode).expect("wrapper de");
        prop_assert_eq!(de.trie_snapshot(), map.trie_snapshot());
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
    assert_eq!(round.trie_snapshot(), tagged.trie_snapshot());

    // Deserialize half: real bytes in the stream are read VERBATIM (no
    // deserialize-side normalization exists — the asymmetry is
    // serialize-only).
    //
    let raw = RawBytesTwin {
        ps: EPathMap::default().trie_snapshot().to_vec(),
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

#[test]
fn always_equal_vs_derived_ord_wart_survives_the_wrapper() {
    let plain = e6a_index_epathmap();
    let tagged = EPathMap::new(
        plain.entry_trie().entries_owned(),
        create_bit_vector(&[0]), // [] vs [0x01]
        plain.connective_used,
        plain.remainder.clone(),
    );

    // AlwaysEqual: equal + same hash (locally_free invisible).
    assert_eq!(plain, tagged, "== must ignore locally_free");
    assert_eq!(
        std_hash(&plain),
        std_hash(&tagged),
        "Hash must ignore locally_free"
    );
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
}

#[test]
fn debug_is_constant_time_and_does_not_force_the_snapshot() {
    let map = epathmap_remainder_connective();
    let debug = format!("{map:?}");
    assert!(
        debug.starts_with("EPathMap { ps: EntryTrie { mode:")
            && debug.contains("snapshot_cached: false"),
        "trie-native struct debug: {debug}"
    );
    for field in ["ps", "locally_free", "connective_used", "remainder"] {
        assert!(debug.contains(field), "missing field {field} in {debug}");
    }
    let _ = map.trie_snapshot();
    assert!(
        format!("{map:?}").contains("snapshot_cached: true"),
        "Debug must report but never force the serialization cache"
    );
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
    let map = e6a_index_epathmap();
    let before = map.trie_snapshot().to_vec();
    let before_bytes = prost::Message::encode_to_vec(&map);

    let mut mutated = map.clone();
    assert_eq!(
        mutated.trie_snapshot().as_ptr(),
        map.trie_snapshot().as_ptr(),
        "precondition: the clone shares the EPM1 snapshot cell (O(1) clone)"
    );

    mutated.insert_entry(ground_list(vec![gstring_par("isolationProbe")]));

    assert_eq!(
        map.trie_snapshot(),
        before.as_slice(),
        "the original's entries are untouched by the sibling's insert"
    );
    assert_eq!(
        map.len() + 1,
        mutated.len(),
        "…and the sibling gained exactly the one entry"
    );
    assert_ne!(
        mutated.trie_snapshot().as_ptr(),
        map.trie_snapshot().as_ptr(),
        "mutation detaches the derived snapshot cell"
    );
    assert_eq!(
        prost::Message::encode_to_vec(&map).as_slice(),
        before_bytes.as_slice(),
        "the original is untouched by the sibling's mutation"
    );
}

/// The representation contract: `EPathMap::clone` is O(1) at the node — the
/// trie clone is a refcount bump and the snapshot cell is an `Arc` bump —
/// and the shared value is semantically indistinguishable from an owned deep
/// copy (`==`, `Ord`, hash, prost bytes, serde bytes).
#[test]
fn clone_shares_the_trie_snapshot_and_preserves_value_semantics() {
    for (name, map) in all_fixture_maps() {
        // Force the direct trie snapshot first: every clone shares the same
        // `Arc<OnceLock<Vec<u8>>>`, cold or warm.
        let snapshot_ptr = map.trie_snapshot().as_ptr();
        let clone = map.clone();
        assert_eq!(
            clone.trie_snapshot().as_ptr(),
            snapshot_ptr,
            "{name}: clone must share the memoized EPM1 bytes (Arc bump, not a trie walk)"
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
