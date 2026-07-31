//! # `Par`-typed cold-store BYTE GOLDENS — blessed on the DERIVED encoder
//!
//! ## Why this file exists, and why it is in `models/` rather than `rspace++/`
//!
//! `rspace++/tests/serializer_byte_goldens.rs` pins the cold-store leaf bytes
//! for the rspace++ *test-double* instantiation (`Datum<String>`,
//! `WaitingContinuation<String, String>`) and concedes, in its own doc comment,
//! that the models-typed path is not pinned there. It cannot be: `models`
//! depends on `rspace_plus_plus` (`models/Cargo.toml`), so `rspace++` cannot
//! name `Par`, `ListParWithRandom`, `BindPattern` or `TaggedContinuation`
//! without a dependency cycle. The `Par`-typed goldens therefore live on the
//! `models` side of that edge, where both halves of the pair are nameable, and
//! call the very same `rspace_plus_plus::…::serializers` entry points.
//!
//! ## What these goldens are for
//!
//! They are the **pre-change baseline** for the iterative cold-store decoder
//! (`models/src/rust/rholang/par_codec.rs`). That decoder's obligation is
//! *language identity* — for every byte string `b`,
//! `T::cold_decode(b)` and `bincode::deserialize::<T>(b)` agree — and the
//! encoder is deliberately untouched. Blessing these constants BEFORE the
//! decoder exists is what makes that statement checkable: a golden captured
//! afterwards would pin whatever the new code does, which is not evidence of
//! anything.
//!
//! The bytes themselves are a consensus surface. `encode_datums` /
//! `encode_continuations` produce the leaves of the history trie, so the
//! checkpoint root — and hence the block hash — is a function of them.
//!
//! ## The four wire shapes a hand-written codec drifts on
//!
//! A hand-written decoder cannot be reviewed against "the derive"; it has to be
//! reviewed against the four places where the derive does something a reader
//! would not guess. Every fixture below carries all four, and each has a named
//! assertion of its own in `models/tests/par_codec_wire_shapes.rs`:
//!
//! | # | shape | what surprises |
//! |---|-------|----------------|
//! | 1 | `New.injections: BTreeMap<String, Par>` | the ONLY `btree_map` field in `RhoTypes.proto`; serde emits a **map** (`u64` count, then key/value pairs), so a decoder must call `deserialize_map`, not `deserialize_seq` |
//! | 2 | the 12 `serialize_with = serialize_as_empty_bytes` sites | WRITTEN as `serialize_bytes(&[])` (8 zero bytes), READ back as `Vec<u8>` through `deserialize_seq`. The asymmetry is deliberate (`models/src/rust/serde_helpers.rs`), and a decoder must read the stream's REAL length rather than assume zero |
//! | 3 | `EPathMap` (`models/src/rust/rhoapi_ext.rs`) | **4** serde fields, not 5 — `intern` is `#[serde(skip)]`; ★ `ps` is a **two-element tuple** (`u64 \|U(m)\| ‖ U(m)`, then `u64 n ‖ n × Par`) that bincode writes positionally with no framing of its own; `locally_free` is blanked on serialize ONLY, and the retained derived `Deserialize` reads the real bytes |
//! | 4 | the three oneofs (`ExprInstance` 36, `ConnectiveInstance` 9, `TaggedCont` 2) | an `Option` tag (1 byte) **then** a variant index (`u32`, 4 bytes fixint-LE) — two separate reads, not one |
//!
//! ## Blessing procedure
//!
//! ```text
//! PAR_GOLDEN_BLESS=1 cargo test -p models --test serializer_par_byte_goldens -- --nocapture
//! ```
//!
//! prints the constants; the assert mode then pins them forever. Re-blessing is
//! a **consensus change** and must be justified as one — never as a test fix.

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, EList, EMap, EMethod, ENot, ESet, ETuple,
    EVar, EZipper, Expr, GDeployId, GFixedPoint, GPrivate, GUnforgeable, If, KeyValuePair,
    ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom, Receive, ReceiveBind, Send,
    TaggedContinuation, Var, VarRef,
};
use models::rust::rhoapi_ext::EPathMap;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    encode_continuations, encode_datum, encode_datums,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

// ===========================================================================
// FIXTURES
// ===========================================================================

/// A `Par` carrying a distinguishing `locally_free` tag, so a golden that
/// pinned the WRONG sub-term (rather than none) is still caught.
fn tagged(tag: u8) -> Par {
    Par {
        locally_free: vec![tag],
        ..Default::default()
    }
}

fn gint(n: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    }
}

/// A NON-ground `EPathMap`: `connective_used = true` defeats
/// `eval_stable_epathmap`, so the hand-written `Serialize` takes its
/// "byte-identical to the P3 derived layout" arm and emits `ps` in
/// CONSTRUCTION order. Shape 3, arm A.
fn nonground_pathmap() -> EPathMap {
    EPathMap::new(
        vec![gint(11), gint(12)],
        vec![0x5A, 0x5B, 0x5C],
        true,
        Some(Var {
            var_instance: Some(VarInstance::FreeVar(3)),
        }),
    )
}

/// A GROUND `EPathMap`: no connective, no remainder, empty `locally_free`, and
/// a non-empty `ps` — so the hand-written `Serialize` takes its CANONICAL
/// TRIE ORDER arm (`ground_canonical_ps`). Shape 3, arm B.
///
/// Deliberately built OUT of canonical order, so that a golden which silently
/// lost the reordering would move. Note this arm makes
/// `decode(encode(x)) == x` false for `x` — by design, and identically for the
/// derived decoder; see the round-trip test's documentation in
/// `models/tests/par_codec_differential.rs`.
fn ground_pathmap() -> EPathMap {
    EPathMap::new(vec![gint(9), gint(2), gint(5)], Vec::new(), false, None)
}

/// The kitchen-sink `Par`: every `Par`-bearing field populated, all four odd
/// wire shapes present, and every `locally_free` deliberately NON-empty so the
/// serialize-only blanking (shape 2) is observable in the pinned length.
fn odd_shapes_par() -> Par {
    Par {
        sends: vec![Send {
            chan: Some(gint(1)),
            data: vec![gint(2), gint(3)],
            persistent: true,
            // shape 2 — blanked on serialize
            locally_free: vec![0x11, 0x12, 0x13],
            connective_used: true,
        }],
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![gint(4)],
                source: Some(gint(5)),
                remainder: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(models::rhoapi::var::WildcardMsg {})),
                }),
                free_count: -7,
            }],
            body: Some(gint(6)),
            persistent: true,
            peek: true,
            bind_count: 3,
            // shape 2
            locally_free: vec![0x21],
            connective_used: true,
            condition: Some(gint(7)),
        }],
        news: vec![New {
            bind_count: 2,
            p: Some(gint(8)),
            uri: vec!["rho:io:stdout".to_string(), "rho:registry:lookup".to_string()],
            // shape 1 — the ONLY BTreeMap<String, Par> on the wire
            injections: {
                let mut m = BTreeMap::new();
                m.insert("alpha".to_string(), gint(9));
                m.insert("beta".to_string(), gint(10));
                m
            },
            // shape 2
            locally_free: vec![0x31, 0x32],
        }],
        exprs: vec![
            // shape 4 — a HIGH oneof index (35 of 36); a decoder that read the
            // index as a single byte, or that mis-ordered tag vs index, cannot
            // reproduce this.
            Expr {
                expr_instance: Some(ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled: vec![0x01, 0x02, 0x03],
                    scale: 4,
                })),
            },
            // shape 3, arm A
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(nonground_pathmap())),
            },
            // shape 3, arm B — reached through EZipper, whose `pathmap` field
            // is an `Option<EPathMap>` (tag byte, then the 4-field struct).
            Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(ground_pathmap()),
                    current_path: vec![vec![0x03, 0x02], vec![], vec![0xFF]],
                    is_write_zipper: true,
                    // shape 2
                    locally_free: vec![0x41],
                    connective_used: true,
                    cursor_kind: 2,
                })),
            },
            // the collection arms — each with a NON-empty locally_free (shape 2)
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![gint(13), gint(14)],
                    locally_free: vec![0x51],
                    connective_used: true,
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(6)),
                    }),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: vec![gint(15)],
                    locally_free: vec![0x61, 0x62],
                    connective_used: false,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![gint(16)],
                    locally_free: vec![0x71],
                    connective_used: true,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(gint(17)),
                        value: Some(gint(18)),
                    }],
                    locally_free: vec![0x81],
                    connective_used: false,
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(1)),
                    }),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                    method_name: "nth".to_string(),
                    target: Some(gint(19)),
                    arguments: vec![gint(20), gint(21)],
                    locally_free: vec![0x91],
                    connective_used: true,
                })),
            },
            // a unary and a binary arm, and the variable arm
            Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(gint(22)) })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(-2)),
                    }),
                })),
            },
            // a UTF-8 string with a multi-byte code point: the decoder must
            // reject invalid UTF-8 exactly where the derive does.
            Expr {
                expr_instance: Some(ExprInstance::GString("λ→∀ ok".to_string())),
            },
            // an EMPTY oneof slot: `Option<ExprInstance>` = None (tag 0, no index)
            Expr {
                expr_instance: None,
            },
        ],
        matches: vec![Match {
            target: Some(gint(23)),
            cases: vec![MatchCase {
                pattern: Some(gint(24)),
                source: Some(gint(25)),
                free_count: 1,
                guard: Some(gint(26)),
            }],
            // shape 2
            locally_free: vec![0xA1],
            connective_used: true,
        }],
        unforgeables: vec![
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: vec![0xB1, 0xB2],
                })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![0xC1] })),
            },
            GUnforgeable { unf_instance: None },
        ],
        bundles: vec![Bundle {
            body: Some(gint(27)),
            write_flag: true,
            read_flag: false,
        }],
        connectives: vec![
            // shape 4 — the LAST ConnectiveInstance index (8 of 9)
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnByteArray(true)),
            },
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![gint(28), gint(29)],
                })),
            },
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(gint(30))),
            },
            Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                    index: 5,
                    depth: -1,
                })),
            },
            Connective {
                connective_instance: None,
            },
        ],
        conditionals: vec![If {
            condition: Some(gint(31)),
            if_true: Some(gint(32)),
            if_false: None,
            // shape 2
            locally_free: vec![0xD1],
            connective_used: true,
        }],
        // shape 2 — the outermost blanked field
        locally_free: vec![0xE1, 0xE2, 0xE3],
        connective_used: true,
    }
}

fn fixture_datum() -> Datum<ListParWithRandom> {
    Datum {
        a: ListParWithRandom {
            pars: vec![odd_shapes_par(), tagged(0x77)],
            random_state: vec![0xF1, 0xF2, 0xF3, 0xF4],
        }
        .into(),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(&[1, 2, 3]),
            Blake2b256Hash::new(&[4, 5, 6]),
            false,
        ),
    }
}

fn fixture_datum_persist() -> Datum<ListParWithRandom> {
    Datum {
        a: ListParWithRandom {
            pars: vec![],
            random_state: vec![],
        }
        .into(),
        persist: true,
        source: Produce::new(
            Blake2b256Hash::new(&[7, 8, 9]),
            Blake2b256Hash::new(&[10, 11, 12]),
            true,
        ),
    }
}

/// `TaggedCont::ParBody` — oneof index 0 (shape 4).
fn fixture_continuation_par_body() -> WaitingContinuation<BindPattern, TaggedContinuation> {
    WaitingContinuation {
        patterns: vec![
            BindPattern {
                patterns: vec![odd_shapes_par()],
                remainder: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
                free_count: 2,
            },
            BindPattern {
                patterns: vec![],
                remainder: None,
                free_count: 0,
            },
        ]
        .into(),
        continuation: TaggedContinuation {
            guard: Some(gint(41)),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(gint(42)),
                random_state: vec![0x01, 0x02],
            })),
        }
        .into(),
        persist: true,
        peeks: BTreeSet::from([0, 2, -5]),
        source: Consume {
            channel_hashes: vec![Blake2b256Hash::new(&[13, 14, 15])],
            hash: Blake2b256Hash::new(&[16, 17, 18]),
            persistent: true,
        },
    }
}

/// `TaggedCont::ScalaBodyRef` — oneof index 1, and an ABSENT oneof
/// (`tagged_cont: None`) in the same fixture family (shape 4).
fn fixture_continuation_scala_ref() -> WaitingContinuation<BindPattern, TaggedContinuation> {
    WaitingContinuation {
        patterns: vec![BindPattern {
            patterns: vec![gint(43)],
            remainder: None,
            free_count: 9,
        }]
        .into(),
        continuation: TaggedContinuation {
            guard: None,
            tagged_cont: Some(TaggedCont::ScalaBodyRef(-99)),
        }
        .into(),
        persist: false,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: vec![],
            hash: Blake2b256Hash::new(&[19, 20, 21]),
            persistent: false,
        },
    }
}

// ===========================================================================
// THE PINS
// ===========================================================================

/// Blake2b256 of the encoding — a compact stand-in for embedding multi-kilobyte
/// byte vectors. The length is asserted exactly alongside it, so a collision
/// would additionally have to preserve length.
fn digest_hex(bytes: &[u8]) -> String {
    hex::encode(Blake2b256Hash::new(bytes).bytes())
}

mod pinned {
    //! ★★ **CBR-042 re-pin — the bincode surface became TRIE-NATIVE (FORM ②).**
    //!
    //! An `EPathMap` now writes `u64-LE |U(m)| ‖ U(m)` — the entry trie's own
    //! byte array — ahead of its values, so every fixture carrying a map grew by
    //! `8 + |U(m)|` per map and all three digests moved. The fixture holds two
    //! maps (a bare `EPathmapBody` and one inside an `EZipper`), and all three
    //! encodings grew by exactly **+46 B**: the same two maps, the same twice.
    //!
    //! ⚠ Re-blessing is a **consensus change** and is filed as one (CBR-042).
    //! The anti-vacuity control lives beside it in
    //! `models/tests/epathmap_canonical_fixtures.rs`: **all five prost goldens
    //! came back byte-for-byte UNMOVED** (SHA-256, not merely length) while all
    //! five bincode and all five JSON goldens moved — which is what
    //! distinguishes *"the bincode surface changed"* from *"an emitter drifted"*.

    /// `encode_datum(fixture_datum())` — the models-typed cold-store leaf.
    /// Captured 2026-07-27 at `18419514` on the DERIVED encoder, before any
    /// line of `par_codec.rs` existed; re-pinned 4,153 → 4,199 by CBR-042.
    pub const PAR_DATUM_LEN: usize = 4199;
    pub const PAR_DATUM_DIGEST_HEX: &str =
        "e56abdc5614046cad47458adc5b2a3ac74165c70a09304c86ab77c1545f3f0b9";

    /// `encode_datums([fixture_datum(), fixture_datum_persist()])`.
    /// Re-pinned 4,285 → 4,331 by CBR-042.
    pub const PAR_DATUMS_LEN: usize = 4331;
    pub const PAR_DATUMS_DIGEST_HEX: &str =
        "df32b177bf14f25ead168ca00f71b372c119cc071d12cc6f668b33d0d4194f64";

    /// `encode_continuations([par_body, scala_ref])`.
    /// Re-pinned 4,529 → 4,575 by CBR-042.
    pub const PAR_CONTS_LEN: usize = 4575;
    pub const PAR_CONTS_DIGEST_HEX: &str =
        "714df35754a4133ac3b53d96f4ecfcdf1833342b8c4e6205c76e548b15b151f1";
}

fn bless() -> bool {
    std::env::var_os("PAR_GOLDEN_BLESS").is_some()
}

fn check(label: &str, pinned_len: usize, pinned_digest: &str, bytes: &[u8]) {
    if bless() {
        println!(
            "    pub const {label}_LEN: usize = {};\n    pub const {label}_DIGEST_HEX: &str =\n        \"{}\";",
            bytes.len(),
            digest_hex(bytes)
        );
    } else {
        assert_eq!(
            bytes.len(),
            pinned_len,
            "{label}: encoded LENGTH drifted. These are cold-store leaf bytes — the \
             history trie hashes them, so this is a consensus change, not a test failure."
        );
        assert_eq!(
            digest_hex(bytes),
            pinned_digest,
            "{label}: encoded BYTES drifted. These are cold-store leaf bytes — the \
             history trie hashes them, so this is a consensus change, not a test failure."
        );
    }
}

#[test]
fn par_datum_encoding_pinned() {
    check(
        "PAR_DATUM",
        pinned::PAR_DATUM_LEN,
        pinned::PAR_DATUM_DIGEST_HEX,
        &encode_datum(&fixture_datum()),
    );
}

#[test]
fn par_datums_encoding_pinned() {
    check(
        "PAR_DATUMS",
        pinned::PAR_DATUMS_LEN,
        pinned::PAR_DATUMS_DIGEST_HEX,
        &encode_datums(&vec![fixture_datum(), fixture_datum_persist()]),
    );
}

#[test]
fn par_continuations_encoding_pinned() {
    check(
        "PAR_CONTS",
        pinned::PAR_CONTS_LEN,
        pinned::PAR_CONTS_DIGEST_HEX,
        &encode_continuations(&vec![
            fixture_continuation_par_body(),
            fixture_continuation_scala_ref(),
        ]),
    );
}

// ===========================================================================
// ANTI-VACUITY: the fixtures must actually CARRY the four odd shapes
// ===========================================================================
//
// A golden over a fixture that lost its interesting content still passes — it
// just pins the wrong thing. These assertions state, in code, what the pins
// above are claiming to cover.

#[test]
fn fixture_carries_shape_1_a_non_empty_injection_map() {
    let p = odd_shapes_par();
    let injections = &p.news[0].injections;
    assert_eq!(
        injections.len(),
        2,
        "shape 1 (BTreeMap<String, Par>) is no longer exercised by the golden fixture"
    );
    assert!(injections.contains_key("alpha") && injections.contains_key("beta"));
}

#[test]
fn fixture_carries_shape_2_every_blanked_locally_free_site() {
    // The 12 `serialize_with = serialize_as_empty_bytes` sites in the generated
    // `rhoapi.rs`, each with NON-empty content so the blanking is observable.
    let p = odd_shapes_par();
    let mut nonempty: Vec<&str> = Vec::with_capacity(12);
    if !p.locally_free.is_empty() {
        nonempty.push("Par");
    }
    if !p.sends[0].locally_free.is_empty() {
        nonempty.push("Send");
    }
    if !p.receives[0].locally_free.is_empty() {
        nonempty.push("Receive");
    }
    if !p.news[0].locally_free.is_empty() {
        nonempty.push("New");
    }
    if !p.matches[0].locally_free.is_empty() {
        nonempty.push("Match");
    }
    if !p.conditionals[0].locally_free.is_empty() {
        nonempty.push("If");
    }
    for e in &p.exprs {
        match e.expr_instance.as_ref() {
            Some(ExprInstance::EListBody(x)) if !x.locally_free.is_empty() => nonempty.push("EList"),
            Some(ExprInstance::ETupleBody(x)) if !x.locally_free.is_empty() => {
                nonempty.push("ETuple")
            }
            Some(ExprInstance::ESetBody(x)) if !x.locally_free.is_empty() => nonempty.push("ESet"),
            Some(ExprInstance::EMapBody(x)) if !x.locally_free.is_empty() => nonempty.push("EMap"),
            Some(ExprInstance::EMethodBody(x)) if !x.locally_free.is_empty() => {
                nonempty.push("EMethod")
            }
            Some(ExprInstance::EZipperBody(x)) if !x.locally_free.is_empty() => {
                nonempty.push("EZipper")
            }
            _ => {}
        }
    }
    assert_eq!(
        nonempty.len(),
        12,
        "shape 2: the golden fixture must carry a NON-empty `locally_free` at every one of \
         the 12 `serialize_as_empty_bytes` sites; it currently reaches {:?}",
        nonempty
    );
}

#[test]
fn fixture_carries_shape_3_both_epathmap_serialize_arms() {
    let p = odd_shapes_par();
    let mut saw_nonground = false;
    let mut saw_ground = false;
    for e in &p.exprs {
        match e.expr_instance.as_ref() {
            Some(ExprInstance::EPathmapBody(m)) => {
                assert!(!m.ps().is_empty() && !m.locally_free.is_empty() && m.connective_used);
                assert!(m.remainder.is_some());
                saw_nonground = true;
            }
            Some(ExprInstance::EZipperBody(z)) => {
                let m = z.pathmap.as_ref().expect("EZipper must carry a pathmap");
                assert!(!m.ps().is_empty() && m.locally_free.is_empty() && !m.connective_used);
                assert!(m.remainder.is_none());
                saw_ground = true;
            }
            _ => {}
        }
    }
    assert!(
        saw_nonground && saw_ground,
        "shape 3: the golden fixture must carry BOTH EPathMap serialize arms \
         (non-ground = construction order, ground = canonical trie order)"
    );
}

#[test]
fn fixture_carries_shape_4_all_three_oneofs_including_high_indices() {
    let p = odd_shapes_par();

    // ExprInstance: a high index (GFixedPoint = 35 of 36) and an absent oneof.
    let mut saw_high_expr = false;
    let mut saw_absent_expr = false;
    for e in &p.exprs {
        match e.expr_instance.as_ref() {
            Some(ExprInstance::GFixedPoint(_)) => saw_high_expr = true,
            None => saw_absent_expr = true,
            _ => {}
        }
    }
    assert!(saw_high_expr, "shape 4: no high-index ExprInstance in fixture");
    assert!(saw_absent_expr, "shape 4: no absent ExprInstance in fixture");

    // ConnectiveInstance: the last index (ConnByteArray = 8 of 9) and absent.
    let mut saw_high_conn = false;
    let mut saw_absent_conn = false;
    for c in &p.connectives {
        match c.connective_instance.as_ref() {
            Some(ConnectiveInstance::ConnByteArray(_)) => saw_high_conn = true,
            None => saw_absent_conn = true,
            _ => {}
        }
    }
    assert!(saw_high_conn, "shape 4: no high-index ConnectiveInstance in fixture");
    assert!(saw_absent_conn, "shape 4: no absent ConnectiveInstance in fixture");

    // TaggedCont: both variants across the two continuation fixtures.
    assert!(matches!(
        fixture_continuation_par_body().continuation.tagged_cont,
        Some(TaggedCont::ParBody(_))
    ));
    assert!(matches!(
        fixture_continuation_scala_ref().continuation.tagged_cont,
        Some(TaggedCont::ScalaBodyRef(_))
    ));

    // UnfInstance and VarInstance oneofs are present too.
    assert!(p.unforgeables.iter().any(|u| u.unf_instance.is_none()));
    assert!(p
        .unforgeables
        .iter()
        .any(|u| matches!(u.unf_instance, Some(UnfInstance::GDeployIdBody(_)))));
}
