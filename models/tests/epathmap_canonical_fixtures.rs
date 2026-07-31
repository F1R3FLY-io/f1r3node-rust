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
    ///
    /// EPathMap wire re-pin: the three GROUND fixtures now serialize as the
    /// compact field-8 U(m) key stream (a PathMap zipper walk) instead of the
    /// field-1 `ps` walk — e6a_index 487→335, nested 52→30, ezipper 64→40.
    ///
    /// Producer-independent event-hash hardening re-pin: the hand-written
    /// `EPathMap::serialize` emits a GROUND map's `ps` in CANONICAL trie order,
    /// so the `e6a_index` bincode + serde-json goldens and the two
    /// `PRODUCE_INDEX_*` event hashes MOVED (DFS construction order → trie
    /// order). `nested`/`ezipper` (single-entry / already trie-ordered) and the
    /// non-ground fixtures did NOT move for the SERIALIZATION change; the channel
    /// + consume hashes did NOT move (map-free legs); no prost golden moved for
    /// the serialization change (serde-only).
    ///
    /// `locally_free` fixture REPAIR re-pin: the `locally_free` fixture's tagged
    /// entry was changed from synthetic (a ground list carrying an artificial lf
    /// bit) to a genuine bound-variable `EVar` — non-ground BY CONTENT, see
    /// [`fixtures::epathmap_locally_free_entries`]. That FIXTURE-CONTENT change
    /// (not the serialization) moved `locally_free.{prost.bin,bincode.bin,json}`
    /// and this `LOCALLY_FREE_ENCODED_LEN` (51→36); it is test-only and feeds no
    /// event-hash datum (no `PRODUCE_INDEX`/channel/consume hash moved).
    pub const E6A_INDEX_ENCODED_LEN: usize = 335;
    pub const NESTED_ENCODED_LEN: usize = 30;
    pub const EZIPPER_ENCODED_LEN: usize = 40;
    pub const LOCALLY_FREE_ENCODED_LEN: usize = 36;
    /// ★ CBR-041 re-pin (23→19). Every map now emits `U(m)` at proto field 8; the
    /// tag-1 `ps` list arm is deleted. This fixture carries a non-default
    /// `connective_used` + `remainder`, so it was on the list arm and moved.
    ///
    /// ⚠ EXACTLY TWO prost goldens moved — this one and `locally_free` — and both
    /// are maps with non-default metadata fields, i.e. precisely the class
    /// `eval_stable_epathmap` was excluding from field 8. The three GROUND prost
    /// goldens (`e6a_index`, `nested`, `ezipper`) and every bincode and JSON golden
    /// came back UNMOVED. That is the anti-vacuity control for this re-blessing: a
    /// change that moved everything would mean the emitter had drifted rather than
    /// the fork having been removed.
    ///
    /// `LOCALLY_FREE_ENCODED_LEN` is unchanged at 36 — a coincidence of length, not
    /// of content: its bytes moved (tag-1 pair ⇒ tag 3 + tag 8) at equal size.
    ///
    /// ★★ CBR-042 (FORM ②): **not one of these five moved, and that is the
    /// anti-vacuity control for the re-blessing below.** FORM ② makes the
    /// *bincode* surface trie-native and touches no prost byte; all five prost
    /// goldens came back byte-for-byte identical (verified by SHA-256, not by
    /// length), while all five bincode and all five JSON goldens moved. A change
    /// that had moved these too would mean the prost emitter had drifted.
    pub const REMAINDER_CONNECTIVE_ENCODED_LEN: usize = 19;

    /// Blake2b256 of `bincode(channel)` for the E-6a index channel Par
    /// (`stable_hash_provider::hash`, the channel leg of every event hash).
    ///
    /// ★ **UNMOVED by CBR-042** — the channel is a `GString`, not a map, so a
    /// change to the pathmap wire cannot reach it. It is the second control:
    /// paired with the two moved `PRODUCE_*` hashes it shows the movement is
    /// confined to the leg that actually carries an `EPathMap`.
    pub const INDEX_CHANNEL_HASH_HEX: &str =
        "5927a6b63fd2ee3b92b4bfd2b4166f4fa4e2f59f99a38fed853c8e1d5bba6301";

    /// `Produce::create(channel, ListParWithRandom{[index], rs1}, true).hash`.
    ///
    /// MOVED by the producer-independent event-hash hardening: the E-6a index
    /// is a GROUND map, so its bincode preimage (embedded in this datum) now
    /// serializes `ps` in CANONICAL trie order instead of DFS construction
    /// order. The channel hash (`INDEX_CHANNEL_HASH_HEX`) is UNCHANGED (the
    /// channel is a `GString`, not the map).
    ///
    /// ★★ **CBR-042 re-pin** (`9991c1b0…` → `b94d5aaa…`). The datum embeds the
    /// map's bincode, which now opens with `u64-LE |U(m)| ‖ U(m)` — the entry
    /// trie's own byte array — before its values. The preimage grew by
    /// `8 + |U(m)|` (this fixture: 3,689 → 4,029 B), so the digest moved.
    pub const PRODUCE_INDEX_RS1_HASH_HEX: &str =
        "b94d5aaab4aff470914d1d94b30cb6d56015bccdd4c8a11be0e5e2cb6eb8308f";

    /// Same datum pars, `random_state` differing in ONE byte — the hash MUST
    /// differ (pins the per-produce random_state placement inside the datum).
    /// MOVED for the same reason as `PRODUCE_INDEX_RS1_HASH_HEX`, and re-pinned
    /// again by CBR-042 (`b86bf7a0…` → `c5458866…`).
    pub const PRODUCE_INDEX_RS2_HASH_HEX: &str =
        "c5458866a160a44c63099421e265552bc7e61727853443c309d314f2bcef30d3";

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
    // ★ There is no in-place entry mutator any more, and there cannot be: an
    // entry's `locally_free` is part of its trie KEY (the codec's escape arm
    // files a non-ground entry by its canonical prost bytes), so editing one
    // MOVES the entry. The edit is therefore expressed as what it is — a
    // different entry set — and re-filed by the constructor.
    let entries: Vec<models::rhoapi::Par> = map
        .ps()
        .iter()
        .map(|entry| {
            let mut cleared = entry.clone();
            cleared.locally_free = Vec::new();
            cleared
        })
        .collect();
    EPathMap::new(entries, Vec::new(), map.connective_used, map.remainder.clone())
}

/// THE serialize-only normalization (models/build.rs injects
/// `serialize_as_empty_bytes` on EVERY `.rhoapi` `locally_free`): every
/// `locally_free` **field** is written as empty bytes, so a round trip loses the
/// bits and the *value* half of an encoding never carries them.
///
/// # ⚠⚠ RE-STATED, and NARROWED, by FORM ② — read this before trusting the name
///
/// This test used to assert something strictly stronger: that serde output is
/// byte-**identical** whether the bitsets are populated or empty, hence *"event
/// hashes therefore never see `locally_free`"*. That claim is **now false for an
/// `EPathMap`'s entries**, and it is false for a reason that has nothing to do
/// with the `serialize_with` normalization:
///
/// * an entry is **KEYED** by `encode_trie_path`, whose `0x0F` escape arm files a
///   ¬`eval_stable` entry as its canonical **prost** bytes, and prost RETAINS
///   `locally_free` (`b73af1d2` / C8 named exactly this);
/// * `1b576c90` (CBR-041) put that key stream `U(m)` on the **prost** wire;
/// * FORM ② (CBR-042) puts it on the **bincode** wire.
///
/// ⇒ two maps differing only in an entry's `locally_free` now hold different
/// keys, hence different `U(m)`, hence different bincode. The normalization
/// still governs every `locally_free` FIELD; it never governed the trie KEY, and
/// bincode was simply the last surface that could not see the difference.
///
/// ★ Filed as CBR-042, not discovered here — and the direction is a
/// **convergence**: before FORM ② two `!=` maps produced identical bincode, so
/// the event hash was not injective on the value. It now is.
///
/// Both `tagged` and its FULLY-cleared twin are NON-ground: the tagged entry is
/// a bound-variable `EVar` — non-ground BY CONTENT, not by its lf bits (see
/// [`epathmap_locally_free_entries`]) — so clearing `locally_free` does NOT flip
/// the map to ground, and both take the same serialize path.
#[test]
fn serde_normalizes_locally_free_to_empty() {
    let tagged = epathmap_locally_free_entries();
    let cleared = clear_locally_free(&tagged);

    // ── What the normalization still guarantees: the VALUE half ─────────────
    //
    // Strip the leading `u64-LE |U(m)| ‖ U(m)` and the two encodings are
    // identical — i.e. every `locally_free` FIELD really is written empty.
    let tagged_bytes = bincode::serialize(&tagged).expect("bincode");
    let cleared_bytes = bincode::serialize(&cleared).expect("bincode");
    let tagged_values = after_path_stream(&tagged_bytes);
    let cleared_values = after_path_stream(&cleared_bytes);
    assert_eq!(
        tagged_values, cleared_values,
        "the VALUE half must not see locally_free (serialize_as_empty_bytes): with the \
         trie's key stream stripped, a bit-tagged map and its cleared twin must write \
         identical bytes"
    );

    // ── What FORM ② added: the KEY half DOES carry it ───────────────────────
    assert_ne!(
        tagged.path_stream(),
        cleared.path_stream(),
        "★ THE CONTROL. The two maps must hold DIFFERENT keys — the escape arm files a \
         ¬eval_stable entry as its canonical prost bytes, which retain locally_free. If \
         they were equal, the assertion above would be comparing a map with itself and \
         would be vacuous."
    );
    assert_ne!(
        tagged_bytes, cleared_bytes,
        "⚠ MEASURED AND FILED (CBR-042): once U(m) is on the bincode wire, an entry's \
         locally_free reaches it through the KEY. This is the cost stated as plainly as \
         the gain — not an accident, and not a test that was loosened."
    );

    // JSON: the same split, expressed structurally rather than by byte offset.
    let tagged_json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&tagged).expect("json")).expect("parse");
    let cleared_json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&cleared).expect("json")).expect("parse");
    assert_eq!(
        tagged_json["ps"][1], cleared_json["ps"][1],
        "serde_json's VALUE half must not see locally_free either"
    );
    assert_ne!(
        tagged_json["ps"][0], cleared_json["ps"][0],
        "…and its KEY half must, for the same reason bincode's does"
    );

    // A round trip drops every bitset (map level and entry level) — the
    // property that makes the DECODED VALUE lf-blind, which is unchanged.
    let round: EPathMap = bincode::deserialize(&tagged_bytes).expect("de");
    assert!(
        round.locally_free.is_empty(),
        "map-level locally_free must normalize to empty on serialize"
    );
    for entry in round.ps() {
        assert!(
            entry.locally_free.is_empty(),
            "entry-level locally_free must normalize to empty on serialize"
        );
    }
}

/// An `EPathMap` encoding with its leading `u64-LE |U(m)| ‖ U(m)` removed — the
/// VALUE half alone.
///
/// ⚠ Reads the length prefix rather than taking a fixed offset, so it cannot
/// silently start slicing in the middle of a key stream when a fixture changes.
fn after_path_stream(bytes: &[u8]) -> &[u8] {
    let len = u64::from_le_bytes(bytes[..8].try_into().expect("8-byte length prefix")) as usize;
    &bytes[8 + len..]
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
// 3b. P4.3 — the SAME event-hash pins with FILLED intern cells (the spliced
//     path). The P0 fixtures above run with unfilled cells (the direct
//     path); these twins intern the map first, so `Produce::create` /
//     `Consume::create` route through the intern-aware spliced emitter —
//     which must reproduce the 84a0fbe4 pins byte-identically.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn event_hash_goldens_produce_with_filled_cell_splices_identically() {
    let channel = gstring_par("e6a:idx:site0");
    let rs1 = fixture_random_state();

    let interned_map = e6a_index_epathmap();

    let datum = ListParWithRandom {
        pars: vec![epathmap_par(interned_map)],
        random_state: rs1,
    };
    // Sanity: the spliced path is ACTIVE for this datum.
    assert_ne!(
        bincode::serialize(&datum).expect("direct").len(),
        0,
        "fixture must serialize"
    );
    let produce = Produce::create(&channel, &datum, true);

    check_pinned_hex(
        "PRODUCE_INDEX_RS1_HASH_HEX (spliced)",
        pinned::PRODUCE_INDEX_RS1_HASH_HEX,
        &hex::encode(&produce.hash.bytes()),
    );
    check_pinned_hex(
        "INDEX_CHANNEL_HASH_HEX (spliced)",
        pinned::INDEX_CHANNEL_HASH_HEX,
        &hex::encode(&produce.channel_hash.bytes()),
    );
}

#[test]
fn event_hash_goldens_consume_with_filled_cell_splices_identically() {
    // The discovery consume with a filled-cell map in BOTH the pattern and
    // the continuation body — the consume legs of the spliced emitter.
    let channel = gstring_par("e6a:idx:site0");

    let interned_map = e6a_index_epathmap();

    // First: the EXACT P0 consume (map-free) must still pin — the direct
    // path through the new trait plumbing.
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
    check_pinned_hex(
        "CONSUME_DISCOVERY_HASH_HEX (trait plumbing)",
        pinned::CONSUME_DISCOVERY_HASH_HEX,
        &hex::encode(&consume.hash.bytes()),
    );

    // Second: map-bearing pattern + continuation — spliced and direct
    // Consume hashes must agree (byte-level differential at the hash).
    let map_pattern = vec![BindPattern {
        patterns: vec![epathmap_par(interned_map.clone())],
        remainder: None,
        free_count: 0,
    }];
    let map_continuation = TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(epathmap_par(interned_map)),
            random_state: fixture_random_state(),
        })),
        guard: None,
    };
    let spliced = Consume::create(&vec![channel.clone()], &map_pattern, &map_continuation, false);

    // Rebuild the SAME values with UNFILLED cells (fresh construction —
    // never interned) so Consume::create takes the direct path.
    let direct_pattern = vec![BindPattern {
        patterns: vec![epathmap_par(e6a_index_epathmap())],
        remainder: None,
        free_count: 0,
    }];
    let direct_continuation = TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(epathmap_par(e6a_index_epathmap())),
            random_state: fixture_random_state(),
        })),
        guard: None,
    };
    let direct = Consume::create(&vec![channel], &direct_pattern, &direct_continuation, false);
    assert_eq!(
        spliced.hash, direct.hash,
        "spliced and direct consume hashes must be identical for identical values"
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
