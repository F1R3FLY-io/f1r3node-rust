//! EPathMap fix P0 — CANONICAL-ENCODING PARITY HARNESS (test-only).
//!
//! Pins the 84a0fbe4 derived truths that every later phase of the principled
//! EPathMap value-handling fix (P1 intern store → P2 chain fusion → P3 L1.5
//! wrapper → P4 transport/spliced event hashing) must preserve
//! BYTE-IDENTICALLY:
//!
//!   * golden PROST encodings (`Message::encode_to_vec`) + `encoded_len` —
//!     the charge/memo canonical encoding (plan §0.B/§0.D);
//!   * golden SERDE encodings (`bincode::serialize` + `serde_json`) — the
//!     event-hash canonical encoding, INCLUDING the `locally_free`-as-empty
//!     serialize-only normalization injected by models/build.rs (§0.D);
//!   * golden produce/consume EVENT HASHES (`Produce::create` /
//!     `Consume::create`) — the full stable_hash_provider composition,
//!     including the per-produce `random_state` placement that makes a
//!     per-Par digest UNABLE to reproduce the hash (only cached per-Par
//!     serde BYTES compose — §0.D);
//!   * ORD fixtures — the derived declaration-order comparison INCLUDING
//!     `locally_free`, versus the AlwaysEqual `==`/`Hash` that IGNORE it
//!     (the documented inconsistency, §0.E: the store must key by full
//!     prost-byte fidelity, never by `==`).
//!
//! GOLDEN CAPTURE: goldens live in `tests/fixtures/goldens/` and are
//! committed. `EPM_P0_BLESS=1 cargo test -p models --test
//! epathmap_canonical_fixtures` regenerates them and prints the pinned
//! scalar constants; the default mode ASSERTS against the committed bytes.
//! The capture procedure ran the bless mode twice in separate processes and
//! byte-compared the outputs before committing (determinism gate — any
//! nondeterminism is a STOP, never normalized away).

mod fixtures;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use models::create_bit_vector;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, EPathMap, ListParWithRandom, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::new_freevar_par;
use prost::Message;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par,
    epathmap_remainder_connective, ezipper_value, gstring_par, ground_list,
    nested_epathmap_value,
};

// ─────────────────────────────────────────────────────────────────────────────
// Pinned scalar constants (captured at 84a0fbe4; printed by the bless mode)
// ─────────────────────────────────────────────────────────────────────────────

mod pinned {
    /// `encoded_len` of each fixture's PROST encoding (== golden byte length).
    pub const E6A_INDEX_ENCODED_LEN: usize = 487;
    pub const NESTED_ENCODED_LEN: usize = 52;
    pub const EZIPPER_ENCODED_LEN: usize = 64;
    pub const LOCALLY_FREE_ENCODED_LEN: usize = 51;
    pub const REMAINDER_CONNECTIVE_ENCODED_LEN: usize = 23;

    /// Blake2b256 of `bincode(channel)` for the E-6a index channel Par
    /// (`stable_hash_provider::hash`, the channel leg of every event hash).
    pub const INDEX_CHANNEL_HASH_HEX: &str =
        "5927a6b63fd2ee3b92b4bfd2b4166f4fa4e2f59f99a38fed853c8e1d5bba6301";

    /// `Produce::create(channel, ListParWithRandom{[index], rs1}, true).hash`.
    pub const PRODUCE_INDEX_RS1_HASH_HEX: &str =
        "ae4e288e4ee137d3e2566aa2ec378e315fb9471bd91a8ecdbb8c0141372bcaa0";

    /// Same datum pars, `random_state` differing in ONE byte — the hash MUST
    /// differ (pins the per-produce random_state placement inside the datum).
    pub const PRODUCE_INDEX_RS2_HASH_HEX: &str =
        "676962a4d99c5ffcd1c63c25a9460a12e7bd85ff2e84a5987d6696271b9230de";

    /// `Consume::create([channel], [freevar bind], ParBody continuation,
    /// false).hash` — the discovery-receive shape.
    pub const CONSUME_DISCOVERY_HASH_HEX: &str =
        "8f1c2de113d80b62535a19d4bdf6b7ee9cc9e5b8f653d39de724519c5e531c4c";
}

// ─────────────────────────────────────────────────────────────────────────────
// Golden-file plumbing
// ─────────────────────────────────────────────────────────────────────────────

fn goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("goldens")
}

fn bless_enabled() -> bool { std::env::var_os("EPM_P0_BLESS").is_some() }

/// In bless mode: write `actual` to the golden file. In assert mode: read the
/// committed golden and assert byte equality.
fn check_golden(name: &str, actual: &[u8]) {
    let path = goldens_dir().join(name);
    if bless_enabled() {
        std::fs::create_dir_all(goldens_dir()).expect("create goldens dir");
        std::fs::write(&path, actual).expect("write golden");
        println!("BLESSED {} ({} bytes)", name, actual.len());
    } else {
        let expected = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("missing golden {} — run EPM_P0_BLESS=1 first: {e}", name));
        assert_eq!(
            actual, &expected[..],
            "golden {} drifted from the committed 84a0fbe4 bytes",
            name
        );
    }
}

fn check_pinned_len(label: &str, pinned: usize, actual: usize) {
    if bless_enabled() {
        println!("PIN {label} = {actual};");
    } else {
        assert_eq!(actual, pinned, "pinned {label} drifted");
    }
}

fn check_pinned_hex(label: &str, pinned: &str, actual: &str) {
    if bless_enabled() {
        println!("PIN {label} = \"{actual}\";");
    } else {
        assert_eq!(actual, pinned, "pinned {label} drifted");
    }
}

/// Every fixture with its golden-file stem, in one place.
fn all_fixtures() -> Vec<(&'static str, EPathMap, usize)> {
    vec![
        ("e6a_index", e6a_index_epathmap(), pinned::E6A_INDEX_ENCODED_LEN),
        ("nested", nested_epathmap_value(), pinned::NESTED_ENCODED_LEN),
        (
            "locally_free",
            epathmap_locally_free_entries(),
            pinned::LOCALLY_FREE_ENCODED_LEN,
        ),
        (
            "remainder_connective",
            epathmap_remainder_connective(),
            pinned::REMAINDER_CONNECTIVE_ENCODED_LEN,
        ),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Golden PROST encodings + encoded_len
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn prost_goldens_epathmap_fixtures() {
    for (name, fixture, pinned_len) in all_fixtures() {
        let bytes = fixture.encode_to_vec();
        assert_eq!(
            fixture.encoded_len(),
            bytes.len(),
            "{name}: encoded_len must equal the encoding's byte length"
        );
        check_pinned_len(&format!("{name}_ENCODED_LEN"), pinned_len, bytes.len());
        check_golden(&format!("{name}.prost.bin"), &bytes);
    }
}

#[test]
fn prost_golden_ezipper_fixture() {
    let zipper = ezipper_value();
    let bytes = zipper.encode_to_vec();
    assert_eq!(zipper.encoded_len(), bytes.len());
    check_pinned_len(
        "EZIPPER_ENCODED_LEN",
        pinned::EZIPPER_ENCODED_LEN,
        bytes.len(),
    );
    check_golden("ezipper.prost.bin", &bytes);
}

/// PROST bytes RETAIN `locally_free` (field 3): the fixture with non-empty
/// bitsets and its recursively-cleared twin must encode DIFFERENTLY. This is
/// the fidelity half of the §0.D/§0.E dual regime (serde normalizes, prost
/// does not) — the P1 intern store must key by these full-fidelity bytes.
#[test]
fn prost_encoding_retains_locally_free() {
    let tagged = epathmap_locally_free_entries();
    let cleared = clear_locally_free(&tagged);
    assert_ne!(
        tagged.encode_to_vec(),
        cleared.encode_to_vec(),
        "prost bytes must differ when locally_free differs"
    );
    assert!(tagged.encoded_len() > cleared.encoded_len());
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Golden SERDE encodings (bincode + JSON) + the locally_free normalization
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn serde_bincode_goldens_epathmap_fixtures() {
    for (name, fixture, _) in all_fixtures() {
        let bytes = bincode::serialize(&fixture).expect("bincode serialize");
        check_golden(&format!("{name}.bincode.bin"), &bytes);
    }
    let zipper = ezipper_value();
    let bytes = bincode::serialize(&zipper).expect("bincode serialize");
    check_golden("ezipper.bincode.bin", &bytes);
}

#[test]
fn serde_json_goldens_epathmap_fixtures() {
    for (name, fixture, _) in all_fixtures() {
        let json = serde_json::to_string_pretty(&fixture).expect("json serialize");
        check_golden(&format!("{name}.json"), json.as_bytes());
    }
    let zipper = ezipper_value();
    let json = serde_json::to_string_pretty(&zipper).expect("json serialize");
    check_golden("ezipper.json", json.as_bytes());
}

/// Recursively clear every `locally_free` reachable in the fixture (the map's
/// own field and the entry Pars' fields — the fixture puts bits in exactly
/// those two spots).
fn clear_locally_free(map: &EPathMap) -> EPathMap {
    let mut cleared = map.clone();
    cleared.locally_free = Vec::new();
    for entry in &mut cleared.ps {
        entry.locally_free = Vec::new();
    }
    cleared
}

/// THE serialize-only normalization (models/build.rs injects
/// `serialize_as_empty_bytes` on EVERY `.rhoapi` `locally_free`): serde
/// output is IDENTICAL whether the bitsets are populated or empty, for both
/// bincode and JSON. Event hashes therefore never see `locally_free` — the
/// P4 spliced emitter must replicate exactly this asymmetry (serialize
/// normalizes, deserialize reads real bytes; plan amendment PM-1).
#[test]
fn serde_normalizes_locally_free_to_empty() {
    let tagged = epathmap_locally_free_entries();
    let cleared = clear_locally_free(&tagged);

    assert_eq!(
        bincode::serialize(&tagged).expect("bincode"),
        bincode::serialize(&cleared).expect("bincode"),
        "bincode must not see locally_free (serialize_as_empty_bytes)"
    );
    assert_eq!(
        serde_json::to_string(&tagged).expect("json"),
        serde_json::to_string(&cleared).expect("json"),
        "serde_json must not see locally_free (serialize_as_empty_bytes)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Golden produce/consume event hashes
// ─────────────────────────────────────────────────────────────────────────────

/// A fixed, production-shaped `random_state`. NOTE (captured truth):
/// `Blake2b512Random::create_from_length` fills its seed from
/// `rand::thread_rng()` and is NONDETERMINISTIC per process — the golden
/// fixture must instead seed via `create_from_bytes` (deterministic by
/// construction, mirroring how consensus deploys derive their rand from the
/// deploy signature rather than ambient entropy).
fn fixture_random_state() -> Vec<u8> {
    crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&[
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x18, 0x19,
    ])
    .to_bytes()
}

#[test]
fn event_hash_goldens_produce() {
    // The E-6a publish triple: channel `@"e6a:idx:site0"`, datum = the index
    // value wrapped in the production `ListParWithRandom`, persist = true
    // (one persistent produce serves every query COMM).
    let channel = gstring_par("e6a:idx:site0");
    let rs1 = fixture_random_state();
    let datum1 = ListParWithRandom {
        pars: vec![epathmap_par(e6a_index_epathmap())],
        random_state: rs1.clone(),
    };
    let produce1 = Produce::create(&channel, &datum1, true);

    check_pinned_hex(
        "INDEX_CHANNEL_HASH_HEX",
        pinned::INDEX_CHANNEL_HASH_HEX,
        &hex::encode(&produce1.channel_hash.bytes()),
    );
    check_pinned_hex(
        "PRODUCE_INDEX_RS1_HASH_HEX",
        pinned::PRODUCE_INDEX_RS1_HASH_HEX,
        &hex::encode(&produce1.hash.bytes()),
    );

    // Same pars, random_state differing in one byte ⇒ DIFFERENT hash: the
    // per-produce random_state sits INSIDE the hashed datum, which is why a
    // per-Par digest cannot reproduce event hashes and P4 must splice cached
    // per-Par serde BYTES into the enclosing bincode layout (plan §0.D).
    let mut rs2 = rs1.clone();
    rs2[0] ^= 0x01;
    let datum2 = ListParWithRandom {
        pars: datum1.pars.clone(),
        random_state: rs2,
    };
    let produce2 = Produce::create(&channel, &datum2, true);
    assert_ne!(
        produce1.hash, produce2.hash,
        "random_state must participate in the produce hash"
    );
    check_pinned_hex(
        "PRODUCE_INDEX_RS2_HASH_HEX",
        pinned::PRODUCE_INDEX_RS2_HASH_HEX,
        &hex::encode(&produce2.hash.bytes()),
    );

    // Persist participates too (hash_produce's third leg).
    let produce_nonpersist = Produce::create(&channel, &datum1, false);
    assert_ne!(produce1.hash, produce_nonpersist.hash);

    // END-TO-END locally_free invisibility: bit-tagging the datum's Par must
    // NOT change the produce hash (the serde normalization composed through
    // bincode → Blake2b256).
    let mut tagged_par = epathmap_par(e6a_index_epathmap());
    tagged_par.locally_free = create_bit_vector(&[0]);
    let datum_tagged = ListParWithRandom {
        pars: vec![tagged_par],
        random_state: rs1,
    };
    let produce_tagged = Produce::create(&channel, &datum_tagged, true);
    assert_eq!(
        produce1.hash, produce_tagged.hash,
        "locally_free must be invisible to event hashes (serialize_as_empty_bytes)"
    );
}

#[test]
fn event_hash_goldens_consume() {
    // The discovery-receive triple: one channel, one single-freevar bind
    // (`for(@idx <- @"e6a:idx:site0")`), a ParBody continuation.
    let channel = gstring_par("e6a:idx:site0");
    let patterns = vec![BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }];
    let continuation = TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(gstring_par("continuation-body")),
            random_state: fixture_random_state(),
        })),
        guard: None,
    };
    let consume = Consume::create(&vec![channel.clone()], &patterns, &continuation, false);

    assert_eq!(consume.channel_hashes.len(), 1);
    check_pinned_hex(
        "CONSUME_DISCOVERY_HASH_HEX",
        pinned::CONSUME_DISCOVERY_HASH_HEX,
        &hex::encode(&consume.hash.bytes()),
    );
    // The consume's channel leg reuses the same stable channel hash as the
    // produce side (hash_vec over the singleton).
    check_pinned_hex(
        "INDEX_CHANNEL_HASH_HEX (consume leg)",
        pinned::INDEX_CHANNEL_HASH_HEX,
        &hex::encode(&consume.channel_hashes[0].bytes()),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Ord fixtures — derived declaration-order compare vs AlwaysEqual ==
// ─────────────────────────────────────────────────────────────────────────────

fn std_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// THE DOCUMENTED INCONSISTENCY (plan §0.E): `PartialEq`/`Hash` are the
/// hand-written AlwaysEqual impls (models/src/lib.rs:613-627) and IGNORE
/// `locally_free`, while the DERIVED `Ord`/`PartialOrd` compare ALL fields in
/// declaration order (ps, locally_free, connective_used, remainder) and see
/// it. Two maps differing only in `locally_free` are `==` yet NOT
/// `Ordering::Equal` — sorted containers and hash containers disagree on
/// identity for such pairs, which is exactly why the P1 intern store must
/// key by full prost-byte fidelity and never by `==` (a hand-written
/// prost_eq INCLUDING locally_free, or the digest of the canonical bytes).
#[test]
fn ord_sees_locally_free_that_always_equal_ignores() {
    let plain = e6a_index_epathmap();
    let mut tagged = plain.clone();
    tagged.locally_free = create_bit_vector(&[0]); // [] vs [0x01]

    // AlwaysEqual: equal, same hash.
    assert_eq!(plain, tagged, "AlwaysEqual == must ignore locally_free");
    assert_eq!(
        std_hash(&plain),
        std_hash(&tagged),
        "AlwaysEqual Hash must ignore locally_free"
    );

    // Derived Ord: NOT equal — [] < [1] byte-lexicographically.
    assert_eq!(
        plain.cmp(&tagged),
        std::cmp::Ordering::Less,
        "derived Ord must SEE locally_free (declaration-order field 2)"
    );
    assert_ne!(plain.cmp(&tagged), std::cmp::Ordering::Equal);
}

/// Declaration-order dominance: `ps` (field 1) decides before
/// `locally_free` (field 2), which decides before `connective_used`
/// (field 3), which decides before `remainder` (field 4).
#[test]
fn ord_compares_in_declaration_order() {
    let entry_a = ground_list(vec![gstring_par("a")]);
    let entry_b = ground_list(vec![gstring_par("b")]);

    // ps dominates locally_free: shorter-prefix ps < longer ps even when the
    // shorter side's locally_free is larger.
    // EPathMap fix P3 (PM-2): constructors instead of struct literals
    // (the wrapper's shadow cell is private).
    let one_entry_big_lf = EPathMap::new(
        vec![entry_a.clone()],
        create_bit_vector(&[7]),
        true,
        None,
    );
    let two_entries_no_lf = EPathMap::new(
        vec![entry_a.clone(), entry_b.clone()],
        Vec::new(),
        false,
        None,
    );
    assert_eq!(
        one_entry_big_lf.cmp(&two_entries_no_lf),
        std::cmp::Ordering::Less,
        "ps must dominate every later field"
    );

    // locally_free dominates connective_used.
    let lf_small_conn_true = EPathMap::new(vec![entry_a.clone()], Vec::new(), true, None);
    let lf_big_conn_false = EPathMap::new(
        vec![entry_a.clone()],
        create_bit_vector(&[0]),
        false,
        None,
    );
    assert_eq!(
        lf_small_conn_true.cmp(&lf_big_conn_false),
        std::cmp::Ordering::Less,
        "locally_free must dominate connective_used"
    );

    // connective_used dominates remainder (false < true; None < Some).
    let conn_false_some_rem = epathmap_remainder_connective_with(false);
    let conn_true_no_rem = EPathMap::new(
        vec![ground_list(vec![gstring_par("head")])],
        Vec::new(),
        true,
        None,
    );
    assert_eq!(
        conn_false_some_rem.cmp(&conn_true_no_rem),
        std::cmp::Ordering::Less,
        "connective_used must dominate remainder"
    );

    // remainder is the final tiebreak: None < Some.
    let no_rem = conn_true_no_rem.clone();
    let some_rem = epathmap_remainder_connective_with(true);
    assert_eq!(
        no_rem.cmp(&some_rem),
        std::cmp::Ordering::Less,
        "remainder None must sort before Some"
    );
}

fn epathmap_remainder_connective_with(connective_used: bool) -> EPathMap {
    let mut map = epathmap_remainder_connective();
    map.connective_used = connective_used;
    map
}

/// The EZipper mirror of the inconsistency: its `locally_free` (declaration
/// field 4) is likewise Ord-visible and AlwaysEqual-invisible.
#[test]
fn ezipper_ord_sees_locally_free_too() {
    let zipper = ezipper_value(); // locally_free = [0x01]
    let mut cleared = zipper.clone();
    cleared.locally_free = Vec::new();

    assert_eq!(zipper, cleared, "EZipper AlwaysEqual ignores locally_free");
    assert_eq!(
        cleared.cmp(&zipper),
        std::cmp::Ordering::Less,
        "EZipper derived Ord sees locally_free"
    );
}
