//! # `clone_equivalence_corpus` — the GENERATED `Clone` against the DERIVE, per axis
//!
//! Stage F-4 replaced `<Par as Clone>::clone` — a `#[derive(Clone)]` expansion —
//! with an explicit-worklist traversal over the shared `drive_with` trampoline
//! (`models/codegen/schema_codegen.rs` §7, emitted into
//! `OUT_DIR/rhoapi_term_ops.rs`). `Clone` is a structural copy, so the expected
//! answer is **byte-identical, no consensus axis moves**.
//!
//! ⚠ *Expected* is not *proved*. This file proves it.
//!
//! ## ★★ Why the oracle is a GENERATED FUNCTION and not the derive
//!
//! Every other conversion in this campaign could leave the derive compiled beside
//! its replacement, because the replacement was a new *function*:
//! `bincode_encoder::encode` sits next to a still-derived `Serialize`, and
//! `rholang/tests/stack_depth_gate.rs` carries both as `bincode_ser` and
//! `bincode_ser_derived`.
//!
//! `Clone` is a **trait impl**. Converting it means the derive is gone — and with
//! it the differential's reference. So the generator re-emits the derive's own
//! body, from the same resolved-field vector the wire tables come from, as a
//! family of free functions (`term_ops::oracle_clone_*`) that recurse into *each
//! other* rather than through `<Par as Clone>::clone`. `oracle_clone_par` is
//! therefore a faithful Θ(depth) reproduction of what rustc emitted, and it is
//! what every assertion below compares against.
//!
//! ⚠ Being Θ(depth) is the *point*, and it is also a constraint: the corpus here
//! is deliberately SHALLOW. Depth independence is
//! `rholang/tests/stack_depth_gate.rs`'s subject, on a sized thread.
//!
//! ## The axes, and why each one is separately necessary
//!
//! | axis | what it would catch that the others would not |
//! |---|---|
//! | `PartialEq` | a structural difference in the fields `eq` compares |
//! | **bincode bytes** | the COLD STORE's fixed point — the cold store already holds byte strings written by this encoding |
//! | `bincode_encoder` bytes | the production encoder, driven by a different table than serde's derive |
//! | **prost bytes** | the protobuf wire, whose field ORDER is ascending-tag and therefore differs from serde's declaration order for `Par` and `TaggedContinuation` — and, ★ measured below, the ONLY byte axis that can see `locally_free` at all |
//! | Blake2b-256 of the bincode bytes | the channel / post-state hash's input, stated separately because that is the value consensus actually compares |
//! | `Debug` | field order *and* `locally_free`, in a form a human can read in the failure |
//! | `Hash` | the hand-written digest, whose field set mirrors `PartialEq`'s |
//! | `Ord` | the derived comparison, which unlike `eq` DOES compare `locally_free` |
//!
//! ### ⚠★ A refuted premise, measured
//!
//! The plan's argument for the byte axes was *"`eq` ignores `locally_free`, so
//! bincode catches what `eq` misses"*. The **second half is false**:
//! `models/build.rs` injects `serialize_with = serialize_as_empty_bytes` on all
//! twelve `locally_free` declarations, so bincode writes eight zero bytes
//! whatever the field holds — bincode is blind to it too, and so is the
//! Blake2b-256 taken over bincode. `dropping_a_field_from_the_rebuild_is_caught_and_names_the_axis`
//! asserts both halves of that: bincode is *required* to be blind, and the
//! **prost** axis is the one that fires. A corpus carrying `eq` + bincode alone
//! would pass a clone that dropped `locally_free`.
//!
//! ★ Each is asserted twice: clone-vs-oracle **and** clone-vs-original. The second
//! is what makes the first non-vacuous — two identically-wrong copies would agree
//! with each other.
//!
//! ## ⚠★ The corpus is ENUMERATED, never hand-listed
//!
//! In the idiom of `models/tests/variant_exhaustiveness_gate.rs`: the constructed
//! variant names are checked, in order, against the GENERATED
//! `bincode_schema_tables::*_VARIANTS` tables. All **36** `ExprInstance` arms, all **9**
//! `ConnectiveInstance` arms, all **4** `UnfInstance` arms, all **3**
//! `VarInstance` arms and both `TaggedCont` arms are covered by construction — a
//! 37th arm added to the `.proto` fails this file rather than silently escaping
//! it. That is not hypothetical: a hand-maintained corpus is exactly how the
//! campaign's driver list came to miss `Hash` entirely.
//!
//! ## Shapes covered beyond the arm enumeration
//!
//! * every collection shape — `EList`, `ETuple`, `ESet`, `EMap`, `EPathMap` —
//!   because the driver walks a `Vec<Par>` element by element and a `BTreeMap`
//!   through its values, and those are different emitted code paths;
//! * `New.injections`, the schema's **only** `map<string, Par>`, including
//!   **duplicate-key overwrite** at construction: a `BTreeMap` cannot hold two
//!   equal keys, so the surviving value is the second, and the clone must
//!   reproduce that map and not the insertion history;
//! * every **zigzag-signed** field carrying a NEGATIVE value (`Var::BoundVar`,
//!   `Var::FreeVar`, `New.bind_count`, `ExprInstance::GInt`, `VarRef::index`,
//!   `VarRef::depth`). ⚠ The plan asked for "the multi-`Signed` shape"; there is
//!   **no `Signed` message in `rhoapi`** (checked: `RhoTypes.proto` and
//!   `CasperMessage.proto`), so the nearest real thing is the six `sint32`/`sint64`
//!   fields, whose bincode rendering is a plain integer and whose prost rendering
//!   is zigzag — two encodings of one value, which is exactly the kind of
//!   asymmetry a clone bug would show up in differently per axis;
//! * `Par` with **all nine** recursive collections populated at once, so the
//!   declaration-order rebuild is exercised where tag order and declaration order
//!   actually differ (`Par`: …7, 11, 8, 12, 9, 10);
//! * `TaggedContinuation`, the other message whose two orders differ;
//! * a **wide** node (512 siblings) and a **shallow-but-broad** node, because the
//!   driver's value stack is sized by the frontier and not by the depth.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte,
    EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr,
    EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational, GDeployId,
    GDeployerId, GFixedPoint, GPrivate, GSysAuthToken, GUnforgeable, If, KeyValuePair,
    ListBindPatterns, ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom, Receive,
    ReceiveBind, Send, TaggedContinuation, Var, VarRef,
};
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::bincode_encoder::encode;
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::term_ops::{
    oracle_clone_par, CLONE_CUT_SET, CLONE_DESCEND_SET, CLONE_EXTERN_BOUNDED,
    CLONE_RESIDUAL_HEIGHT, EMITTED_TRAVERSALS,
};

// ---------------------------------------------------------------------------
// The axes
// ---------------------------------------------------------------------------

/// Every consensus-visible projection of one `Par`, taken together so a failure
/// reports which axis moved rather than only that something did.
#[derive(PartialEq, Eq)]
struct Axes {
    bincode: Vec<u8>,
    wire: Vec<u8>,
    prost: Vec<u8>,
    blake: [u8; 32],
    debug: String,
    hash: u64,
}

impl Axes {
    fn of(p: &Par) -> Axes {
        use prost::Message as _;
        let bincode =
            bincode::serialize(p).expect("bincode: the serde derive cannot fail on `Par`");
        let mut hasher = DefaultHasher::new();
        p.hash(&mut hasher);
        Axes {
            blake: blake2b256(&bincode),
            bincode,
            wire: encode(p).to_vec(),
            prost: p.encode_to_vec(),
            debug: format!("{p:?}"),
            hash: hasher.finish(),
        }
    }

    /// The first axis on which two projections differ, named.
    fn first_difference(&self, other: &Axes) -> Option<&'static str> {
        if self.bincode != other.bincode {
            return Some("bincode bytes (THE COLD STORE'S FIXED POINT)");
        }
        if self.wire != other.wire {
            return Some("bincode_encoder bytes (the production encoder)");
        }
        if self.prost != other.prost {
            return Some("prost bytes (the protobuf wire)");
        }
        if self.blake != other.blake {
            return Some("Blake2b-256 of the bincode bytes (the channel/post-state hash input)");
        }
        if self.debug != other.debug {
            return Some("Debug (field order, and `locally_free`, which `eq` ignores)");
        }
        if self.hash != other.hash {
            return Some("Hash (the hand-written digest)");
        }
        None
    }
}

/// Blake2b-256, the digest RSpace channels and post-state roots are built from.
fn blake2b256(bytes: &[u8]) -> [u8; 32] {
    use blake2::digest::{Update, VariableOutput};
    use blake2::Blake2bVar;
    let mut hasher = Blake2bVar::new(32).expect("blake2b: 32 is a valid output length");
    hasher.update(bytes);
    let mut out = [0u8; 32];
    hasher
        .finalize_variable(&mut out)
        .expect("blake2b: the output buffer is exactly 32 bytes");
    out
}

/// ★ THE PROPERTY, on one corpus term.
///
/// Asserted against the oracle **and** against the original. The second is what
/// stops the first from being satisfiable by two identically-wrong copies.
fn axes_agree(label: &str, original: &Par) {
    let driven = original.clone();
    let oracle = oracle_clone_par(original);

    let source = Axes::of(original);
    let a = Axes::of(&driven);
    let b = Axes::of(&oracle);

    if let Some(axis) = a.first_difference(&b) {
        panic!(
            "★ CLONE EQUIVALENCE BROKEN at `{label}`: the DRIVEN `<Par as Clone>::clone` and the \
             retained derive oracle `term_ops::oracle_clone_par` disagree on {axis}.\n\
             \n\
             driven Debug: {}\n\
             oracle Debug: {}\n\
             \n\
             `Clone` is a structural copy, so no axis may move. A difference here is a \
             GENERATED-CODE defect in `models/codegen/schema_codegen.rs` §7 — most likely a \
             `clone_rebuild_*` that places a field the matching `clone_push_children_*` never \
             pushed, or the two disagreeing about DECLARATION order.",
            a.debug, b.debug
        );
    }
    if let Some(axis) = a.first_difference(&source) {
        panic!(
            "★ CLONE EQUIVALENCE BROKEN at `{label}`: the DRIVEN clone differs from its OWN \
             SOURCE on {axis}.\n\
             \n\
             source Debug: {}\n\
             clone  Debug: {}\n\
             \n\
             This leg is what makes the clone-vs-oracle leg above non-vacuous: two copies that \
             were wrong in the same way would agree with each other and disagree with this.",
            source.debug, a.debug
        );
    }
    assert!(
        driven == *original,
        "★ `{label}`: the driven clone does not compare EQUAL to its source under the \
         hand-written `<Par as PartialEq>::eq`. Every byte-level axis above agreed, so this is \
         `eq`'s own field set — see `models/src/lib.rs`."
    );
    assert!(
        driven == oracle,
        "★ `{label}`: the driven clone and the oracle clone do not compare EQUAL"
    );
    assert_eq!(
        driven.cmp(original),
        std::cmp::Ordering::Equal,
        "★ `{label}`: `Ord` orders the driven clone away from its source. ⚠ Unlike `eq`, the \
         derived `Ord` DOES compare `locally_free`, so this axis is not implied by the others."
    );
    assert_eq!(
        driven.cmp(&oracle),
        std::cmp::Ordering::Equal,
        "★ `{label}`: `Ord` orders the driven clone away from the oracle clone"
    );

    // ⚠ ANTI-VACUITY. A `Par::default()` projects to a handful of bytes on every
    // axis, and a corpus of empty terms would satisfy everything above while
    // exercising no descent at all.
    assert!(
        !a.bincode.is_empty() && !a.debug.is_empty(),
        "VACUOUS: `{label}` projects to nothing"
    );
}

// ---------------------------------------------------------------------------
// The enumerated arms
// ---------------------------------------------------------------------------

fn gint(n: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    }
}

fn gstr(s: &str) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}

/// A `Par` with a non-empty `locally_free` and `connective_used` set, so the two
/// bounded fields of the hottest type are never at their defaults.
fn marked(mut p: Par) -> Par {
    p.locally_free = vec![0xa5, 0x5a, 0x0f];
    p.connective_used = true;
    p
}

/// Two child `Par`s that differ, so a rebuild that swapped them would be visible.
fn two_children() -> (Par, Par) { (gint(-7), gstr("second")) }

/// One value of every [`ExprInstance`] arm, with **payloads that carry children**
/// wherever the arm has room for them.
///
/// ⚠ `Default::default()` payloads would type-check and cover every arm name
/// while descending into NOTHING — the driver would never push a child and the
/// rebuild would never take one. Every message-payload arm below therefore
/// carries at least one `Par`.
fn expr_instance_values() -> Vec<(&'static str, ExprInstance)> {
    let (a, b) = two_children();
    let binary = |p1: Par, p2: Par| (Some(p1), Some(p2));
    let (l, r) = binary(a.clone(), b.clone());
    vec![
        ("GBool", ExprInstance::GBool(true)),
        // ⚠ NEGATIVE: `g_int` is `sint64`, i.e. ZIGZAG on the protobuf wire and a
        // plain `i64` in bincode. Two encodings of one value.
        ("GInt", ExprInstance::GInt(-9_007_199_254_740_993)),
        ("GString", ExprInstance::GString("a string".to_string())),
        ("GUri", ExprInstance::GUri("rho:io:stdout".to_string())),
        ("GByteArray", ExprInstance::GByteArray(vec![1, 2, 3, 0xff])),
        (
            "ENotBody",
            ExprInstance::ENotBody(ENot { p: Some(a.clone()) }),
        ),
        (
            "ENegBody",
            ExprInstance::ENegBody(ENeg { p: Some(b.clone()) }),
        ),
        (
            "EMultBody",
            ExprInstance::EMultBody(EMult {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EDivBody",
            ExprInstance::EDivBody(EDiv {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EPlusBody",
            ExprInstance::EPlusBody(EPlus {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EMinusBody",
            ExprInstance::EMinusBody(EMinus {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "ELtBody",
            ExprInstance::ELtBody(ELt {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "ELteBody",
            ExprInstance::ELteBody(ELte {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EGtBody",
            ExprInstance::EGtBody(EGt {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EGteBody",
            ExprInstance::EGteBody(EGte {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EEqBody",
            ExprInstance::EEqBody(EEq {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "ENeqBody",
            ExprInstance::ENeqBody(ENeq {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EAndBody",
            ExprInstance::EAndBody(EAnd {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EOrBody",
            ExprInstance::EOrBody(EOr {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        // ⚠ NEGATIVE `sint32` inside a `Copy` payload — `EVar` is one of the seven
        // items that KEPT its derive, so this arm proves the two families meet.
        (
            "EVarBody",
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(-3)),
                }),
            }),
        ),
        (
            "EListBody",
            ExprInstance::EListBody(EList {
                ps: vec![a.clone(), b.clone(), gint(3)],
                locally_free: vec![7],
                connective_used: true,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(-1)),
                }),
            }),
        ),
        (
            "ETupleBody",
            ExprInstance::ETupleBody(ETuple {
                ps: vec![a.clone(), b.clone()],
                locally_free: vec![8],
                connective_used: false,
            }),
        ),
        (
            "ESetBody",
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(1), gint(2), gint(3)],
                locally_free: vec![9],
                connective_used: true,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            }),
        ),
        (
            "EMapBody",
            ExprInstance::EMapBody(EMap {
                // Two pairs, so the `KeyValuePair` walk takes FOUR children in
                // key/value/key/value order — a rebuild that read them
                // key/key/value/value would produce a valid term with the wrong
                // associations and no length change.
                kvs: vec![
                    KeyValuePair {
                        key: Some(gstr("k1")),
                        value: Some(gint(1)),
                    },
                    KeyValuePair {
                        key: Some(gstr("k2")),
                        value: Some(gint(2)),
                    },
                ],
                locally_free: vec![10],
                connective_used: false,
                remainder: None,
            }),
        ),
        (
            "EMethodBody",
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(a.clone()),
                arguments: vec![gint(0), gint(1)],
                locally_free: vec![11],
                connective_used: true,
            }),
        ),
        // ⚠★ The EXTERN type, treated as BOUNDED because its hand-written `Clone`
        // is O(1) at the node. Non-empty so the `EntryTrie` and the shadow cell
        // are both live rather than at their defaults.
        (
            "EPathmapBody",
            ExprInstance::EPathmapBody(EPathMap::new(
                vec![gstr("path"), gint(42)],
                vec![12],
                true,
                None,
            )),
        ),
        (
            "EZipperBody",
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new(vec![gstr("z")], vec![], false, None)),
                current_path: vec![vec![1], vec![2, 3]],
                is_write_zipper: true,
                locally_free: vec![13],
                connective_used: false,
                cursor_kind: 2,
            }),
        ),
        (
            "EMatchesBody",
            ExprInstance::EMatchesBody(EMatches {
                target: Some(a.clone()),
                pattern: Some(b.clone()),
            }),
        ),
        (
            "EPercentPercentBody",
            ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EPlusPlusBody",
            ExprInstance::EPlusPlusBody(EPlusPlus {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        (
            "EMinusMinusBody",
            ExprInstance::EMinusMinusBody(EMinusMinus {
                p1: l.clone(),
                p2: r.clone(),
            }),
        ),
        ("EModBody", ExprInstance::EModBody(EMod { p1: l, p2: r })),
        // The IEEE-754 BIT PATTERN as `u64`, not `f64` — see the same note in
        // `variant_exhaustiveness_gate.rs`: `f64::NAN != f64::NAN` would break
        // `x == x` for a reason unrelated to cloning.
        ("GDouble", ExprInstance::GDouble(0x4009_21fb_5444_2d18)),
        // Two's-complement big-endian, NEGATIVE (leading 0xff).
        ("GBigInt", ExprInstance::GBigInt(vec![0xff, 0x9c])),
        (
            "GBigRat",
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![0xff, 0xfd],
                denominator: vec![0x07],
            }),
        ),
        (
            "GFixedPoint",
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![0xff, 0x85],
                scale: 3,
            }),
        ),
    ]
}

/// One value of every [`ConnectiveInstance`] arm.
///
/// ⚠ `ConnNotBody`'s payload is a bare `Par` — the schema's only place where a
/// **cut-set member is a direct oneof payload**, so it is the one arm that
/// exercises `clone_rebuild_connective_instance`'s `children.par()` branch.
fn connective_instance_values() -> Vec<(&'static str, ConnectiveInstance)> {
    let (a, b) = two_children();
    vec![
        (
            "ConnAndBody",
            ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![a.clone(), b.clone()],
            }),
        ),
        (
            "ConnOrBody",
            ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![b.clone(), a.clone(), gint(0)],
            }),
        ),
        ("ConnNotBody", ConnectiveInstance::ConnNotBody(a)),
        (
            "VarRefBody",
            // ⚠ Both fields NEGATIVE `sint32`.
            ConnectiveInstance::VarRefBody(VarRef {
                index: -5,
                depth: -6,
            }),
        ),
        ("ConnBool", ConnectiveInstance::ConnBool(true)),
        ("ConnInt", ConnectiveInstance::ConnInt(true)),
        ("ConnString", ConnectiveInstance::ConnString(true)),
        ("ConnUri", ConnectiveInstance::ConnUri(true)),
        ("ConnByteArray", ConnectiveInstance::ConnByteArray(true)),
    ]
}

/// One value of every [`UnfInstance`] arm.
///
/// ⚠ `GUnforgeable` is **BOUNDED** for the clone driver — no arm can reach a
/// `Par` — so `Par.unforgeables` is one of the two places the generator emits a
/// whole-`Vec` clone. These values are here to prove that treatment is faithful,
/// which is the only way "the bounded case is correct" is more than an argument.
fn unf_instance_values() -> Vec<(&'static str, UnfInstance)> {
    vec![
        (
            "GPrivateBody",
            UnfInstance::GPrivateBody(GPrivate {
                id: vec![0xde, 0xad, 0xbe, 0xef],
            }),
        ),
        (
            "GDeployIdBody",
            UnfInstance::GDeployIdBody(GDeployId { sig: vec![1, 2, 3] }),
        ),
        (
            "GDeployerIdBody",
            UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![4, 5, 6],
            }),
        ),
        (
            "GSysAuthTokenBody",
            UnfInstance::GSysAuthTokenBody(GSysAuthToken {}),
        ),
    ]
}

/// One value of every [`VarInstance`] arm — all with NEGATIVE `sint32` payloads.
fn var_instance_values() -> Vec<(&'static str, VarInstance)> {
    vec![
        ("BoundVar", VarInstance::BoundVar(-1)),
        ("FreeVar", VarInstance::FreeVar(-2)),
        ("Wildcard", VarInstance::Wildcard(WildcardMsg {})),
    ]
}

/// One value of every [`TaggedCont`] arm.
fn tagged_cont_values() -> Vec<(&'static str, TaggedCont)> {
    vec![
        (
            "ParBody",
            TaggedCont::ParBody(ParWithRandom {
                body: Some(gint(1)),
                random_state: vec![0xa5; 32],
            }),
        ),
        ("ScalaBodyRef", TaggedCont::ScalaBodyRef(-42)),
    ]
}

fn names_of(table: &[models::rust::rholang::bincode_schema::VariantProgram]) -> Vec<&str> {
    table.iter().map(|v| v.name).collect()
}

/// ★ The enumeration check: the constructed arm names, in order, against the
/// GENERATED table.
fn assert_enumerated<T>(enum_name: &str, table: Vec<&str>, values: &[(&'static str, T)]) {
    let constructed: Vec<&str> = values.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        constructed, table,
        "★ `{enum_name}`'s corpus no longer matches the GENERATED `bincode_schema_tables` variant table. \
         The schema grew or shrank and this file's coverage silently stopped being exhaustive — \
         which is precisely how a hand-maintained list of \"the traversals that matter\" came to \
         miss `Hash` entirely. Regenerate the corpus, do not adjust the table."
    );
    assert!(
        !table.is_empty(),
        "★ `{enum_name}`'s generated variant table is EMPTY, so the comparison above passed \
         vacuously"
    );
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// Every corpus term, as `(label, Par)`.
fn corpus() -> Vec<(String, Par)> {
    let mut out: Vec<(String, Par)> = Vec::with_capacity(96);
    let (a, b) = two_children();

    // ── the enumerated oneof arms, each hung under a `Par` ──
    let exprs = expr_instance_values();
    assert_enumerated("ExprInstance", names_of(EXPR_INSTANCE_VARIANTS), &exprs);
    for (name, instance) in exprs {
        out.push((
            format!("ExprInstance::{name}"),
            marked(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(instance),
                }],
                ..Default::default()
            }),
        ));
    }

    let connectives = connective_instance_values();
    assert_enumerated(
        "ConnectiveInstance",
        names_of(CONNECTIVE_INSTANCE_VARIANTS),
        &connectives,
    );
    for (name, instance) in connectives {
        out.push((
            format!("ConnectiveInstance::{name}"),
            marked(models::par_from_default! {
                connectives: vec![Connective {
                    connective_instance: Some(instance),
                }],
                ..Default::default()
            }),
        ));
    }

    let unfs = unf_instance_values();
    assert_enumerated("UnfInstance", names_of(UNF_INSTANCE_VARIANTS), &unfs);
    for (name, instance) in unfs {
        out.push((
            format!("UnfInstance::{name}"),
            marked(models::par_from_default! {
                unforgeables: vec![GUnforgeable {
                    unf_instance: Some(instance),
                }],
                ..Default::default()
            }),
        ));
    }

    let vars = var_instance_values();
    assert_enumerated("VarInstance", names_of(VAR_INSTANCE_VARIANTS), &vars);
    for (name, instance) in vars {
        // Reached through `EList.remainder`, which is `Option<Var>` — a `Copy`
        // message, i.e. one of the seven that kept its derive.
        out.push((
            format!("VarInstance::{name}"),
            marked(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![a.clone()],
                        locally_free: vec![1],
                        connective_used: false,
                        remainder: Some(Var {
                            var_instance: Some(instance),
                        }),
                    })),
                }],
                ..Default::default()
            }),
        ));
    }

    // ── `TaggedCont`: reached through `TaggedContinuation`, not through `Par` ──
    //
    // ⚠ `TaggedContinuation` is NOT reachable from `Par`, so the driver never
    // enters it — its `Clone` is field-wise. It is in the corpus all the same,
    // through its `guard: Option<Par>`, because it is the OTHER message whose
    // declaration order and tag order differ.
    let conts = tagged_cont_values();
    assert_enumerated("TaggedCont", names_of(TAGGED_CONT_VARIANTS), &conts);
    for (name, tagged) in conts {
        let cont = TaggedContinuation {
            tagged_cont: Some(tagged),
            guard: Some(marked(gstr("guard"))),
        };
        let driven = cont.clone();
        assert_eq!(
            format!("{cont:?}"),
            format!("{driven:?}"),
            "★ `TaggedCont::{name}`: the generated field-wise `Clone` for \
             `TaggedContinuation` — the message whose declaration order and tag order differ — \
             does not reproduce its source"
        );
        assert_eq!(
            bincode::serialize(&cont).expect("bincode"),
            bincode::serialize(&driven).expect("bincode"),
            "★ `TaggedCont::{name}`: `TaggedContinuation`'s bincode bytes moved. This is the \
             message whose serde order once cost this campaign a 95-byte encoding with its \
             halves exchanged."
        );
        // Its `guard` is a `Par`, so the axes below apply to that.
        out.push((
            format!("TaggedCont::{name} (guard)"),
            cont.guard.expect("the guard was just set"),
        ));
    }

    // ── the nine recursive collections of `Par`, ALL populated at once ──
    //
    // ★ This is the term that exercises DECLARATION ORDER where it actually
    // differs from tag order (`Par`: 1, 2, 4, 5, 6, 7, **11**, 8, **12**, 9, 10).
    // A `clone_rebuild_par` in tag order would hand the cloned `bundles` to
    // `connectives` here and nowhere else.
    out.push((
        "Par with all nine recursive collections".to_string(),
        marked(Par {
            sends: vec![Send {
                chan: Some(gstr("chan")),
                data: vec![a.clone(), b.clone()],
                persistent: true,
                locally_free: vec![1],
                connective_used: false,
            }],
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![gstr("p1"), gstr("p2")],
                    source: Some(gstr("src")),
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(-1)),
                    }),
                    free_count: 2,
                }],
                body: Some(gint(1)),
                persistent: false,
                peek: true,
                bind_count: 2,
                locally_free: vec![2],
                connective_used: true,
                condition: Some(gint(2)),
            }],
            news: vec![New {
                // ⚠ NEGATIVE `sint32`.
                bind_count: -3,
                p: Some(gint(3)),
                uri: vec![
                    "rho:io:stdout".to_string(),
                    "rho:rchain:deployId".to_string(),
                ],
                injections: injections_with_duplicate_key(),
                locally_free: vec![3],
            }],
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(-4)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![gint(5), gint(6)],
                        locally_free: vec![4],
                        connective_used: false,
                        remainder: None,
                    })),
                },
            ],
            matches: vec![Match {
                target: Some(gstr("target")),
                cases: vec![
                    MatchCase {
                        pattern: Some(gstr("pat1")),
                        source: Some(gint(7)),
                        free_count: 1,
                        guard: Some(gint(8)),
                    },
                    MatchCase {
                        pattern: Some(gstr("pat2")),
                        source: Some(gint(9)),
                        free_count: 0,
                        guard: None,
                    },
                ],
                locally_free: vec![5],
                connective_used: false,
            }],
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: vec![0xca, 0xfe],
                })),
            }],
            bundles: vec![Bundle {
                body: Some(gstr("bundled")),
                write_flag: true,
                read_flag: false,
            }],
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![gint(10), gint(11)],
                })),
            }],
            conditionals: vec![If {
                condition: Some(gint(12)),
                if_true: Some(gstr("then")),
                if_false: Some(gstr("else")),
                locally_free: vec![6],
                connective_used: true,
            }],
            locally_free: vec![0xff, 0x00, 0x7f],
            connective_used: true,
        }),
    ));

    // ── `New.injections`: the schema's ONLY map, with a duplicate-key overwrite ──
    out.push((
        "New.injections (BTreeMap, duplicate-key overwrite)".to_string(),
        marked(models::par_from_default! {
            news: vec![New {
                bind_count: 2,
                p: Some(gint(1)),
                uri: vec![],
                injections: injections_with_duplicate_key(),
                locally_free: vec![],
            }],
            ..Default::default()
        }),
    ));

    // ── `If` on its own: the collection the audit's ladder never touches ──
    //
    // ⚠ `conditionals` is `Par`'s LAST recursive collection in declaration order
    // and its HIGHEST tag (12), so it is the field a truncated rebuild loses
    // first. It is the field the RED probe for this file drops.
    out.push((
        "Par.conditionals only".to_string(),
        marked(models::par_from_default! {
            conditionals: vec![
                If {
                    condition: Some(gint(1)),
                    if_true: Some(gint(2)),
                    if_false: Some(gint(3)),
                    locally_free: vec![1],
                    connective_used: false,
                },
                If {
                    condition: Some(gstr("c")),
                    if_true: Some(gstr("t")),
                    if_false: None,
                    locally_free: vec![2],
                    connective_used: true,
                },
            ],
            ..Default::default()
        }),
    ));

    // ── WIDE: the value stack is sized by the FRONTIER, not by the depth ──
    out.push((
        "wide: 512 siblings under one EList".to_string(),
        marked(models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: (0..512).map(|i| gint(i as i64 - 256)).collect(),
                    locally_free: vec![1],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        }),
    ));

    // ── SHALLOW BUT BROAD: 64 sends, each with 4 data — the production shape ──
    out.push((
        "broad: 64 sends x 4 data".to_string(),
        marked(models::par_from_default! {
            sends: (0..64)
                .map(|i| Send {
                    chan: Some(gstr(&format!("c{i}"))),
                    data: (0..4).map(|j| gint(i * 4 + j)).collect(),
                    persistent: i % 2 == 0,
                    locally_free: vec![i as u8],
                    connective_used: i % 3 == 0,
                })
                .collect(),
            ..Default::default()
        }),
    ));

    // ── the SHALLOW ladder, at the measured production depths ──
    for depth in 1..=6usize {
        let mut p = marked(gint(depth as i64));
        for level in 0..depth {
            p = marked(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(if level % 2 == 0 {
                        ExprInstance::EListBody(EList {
                            ps: vec![p, gstr("sibling")],
                            locally_free: vec![level as u8],
                            connective_used: false,
                            remainder: None,
                        })
                    } else {
                        ExprInstance::ETupleBody(ETuple {
                            ps: vec![p, gint(level as i64)],
                            locally_free: vec![level as u8],
                            connective_used: true,
                        })
                    }),
                }],
                ..Default::default()
            });
        }
        out.push((format!("measured-distribution depth {depth}"), p));
    }

    // ── the `Vec`-nested ladder, through `sends[0].chan` ──
    //
    // ★ A DIFFERENT `Vec` field from the `exprs`/`EList.ps` pair above, and the
    // one the gate subject `clone_send_chain` uses.
    {
        let mut p = marked(gint(0));
        for level in 0..8usize {
            p = marked(models::par_from_default! {
                sends: vec![Send {
                    chan: Some(p),
                    data: vec![gint(level as i64)],
                    persistent: false,
                    locally_free: vec![level as u8],
                    connective_used: false,
                }],
                ..Default::default()
            });
        }
        out.push(("Vec-nested: sends[0].chan x 8".to_string(), p));
    }

    // ── `Par::default()`: the Nil process, and the LEAF case of the driver ──
    out.push(("Par::default (Nil)".to_string(), Par::default()));

    out
}

/// `New.injections` built by inserting one key TWICE.
///
/// ⚠ A `BTreeMap` cannot hold two equal keys, so the surviving value is the
/// SECOND. The clone must reproduce the map, not the insertion history — and the
/// driver's `clone_push_children_new` walks `injections.values()` while
/// `clone_rebuild_new` re-associates through `injections.keys()`, two iterations
/// of one `BTreeMap` that must stay in step.
fn injections_with_duplicate_key() -> BTreeMap<String, Par> {
    let mut map = BTreeMap::new();
    map.insert("dup".to_string(), gint(111));
    let overwritten = map.insert("dup".to_string(), gint(222));
    assert!(
        overwritten.is_some(),
        "VACUOUS: the duplicate-key insert did not overwrite, so this fixture is not the shape \
         it claims to be"
    );
    map.insert("aaa".to_string(), gstr("first-by-key-order"));
    map.insert("zzz".to_string(), gstr("last-by-key-order"));
    map
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// ★★ **THE PROPERTY.** Every corpus term's driven clone agrees with the retained
/// derive oracle, and with its own source, on every consensus-visible axis.
#[test]
fn every_enumerated_shape_clones_identically_on_every_axis() {
    let corpus = corpus();
    assert!(
        corpus.len() >= 36 + 9 + 4 + 3 + 2 + 6,
        "the corpus is only {} terms; the enumerated arms alone are 36 `ExprInstance` + 9 \
         `ConnectiveInstance` + 4 `UnfInstance` + 3 `VarInstance` + 2 `TaggedCont`, so a \
         shorter corpus means the enumeration stopped running",
        corpus.len()
    );
    for (label, term) in &corpus {
        axes_agree(label, term);
    }
    println!(
        "  {} corpus terms; each clone identical to its oracle and its source on bincode, \
         bincode_encoder, prost, Blake2b-256, Debug, Hash, PartialEq and Ord",
        corpus.len()
    );
}

/// ★ **The differential can go RED.** A property that no perturbation breaks is
/// not evidence.
///
/// The perturbation is applied to the ORACLE rather than to the generated file,
/// because the generated file is not editable from a test — and it is the exact
/// perturbation the plan named: drop `conditionals`. `Par.conditionals` is the
/// LAST recursive collection in declaration order and the HIGHEST tag, i.e. the
/// field a truncated rebuild loses first.
///
/// ⚠ This also demonstrates that the assertion NAMES THE AXIS: dropping
/// `conditionals` changes the bincode bytes, so the failure reports the cold
/// store's fixed point rather than "the terms differ".
#[test]
fn dropping_a_field_from_the_rebuild_is_caught_and_names_the_axis() {
    let term = marked(models::par_from_default! {
        conditionals: vec![If {
            condition: Some(gint(1)),
            if_true: Some(gint(2)),
            if_false: Some(gint(3)),
            locally_free: vec![1],
            connective_used: false,
        }],
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(9)),
        }],
        ..Default::default()
    });

    // The correct clone agrees with the oracle on every axis.
    let good = Axes::of(&term.clone());
    let reference = Axes::of(&oracle_clone_par(&term));
    assert_eq!(
        good.first_difference(&reference),
        None,
        "the unperturbed pair must agree, or the perturbation below proves nothing"
    );

    // ★ THE PERTURBATION: one field dropped, exactly as a `clone_rebuild_par`
    // that forgot `conditionals` would produce.
    let mut perturbed = oracle_clone_par(&term);
    perturbed.conditionals.clear();
    let bad = Axes::of(&perturbed);

    let axis = good.first_difference(&bad).expect(
        "★ RED-PROBE FAILURE: dropping `Par.conditionals` produced a term this file's axes \
         cannot distinguish from the original. Every assertion in \
         `every_enumerated_shape_clones_identically_on_every_axis` would then be satisfiable by \
         a rebuild that silently loses that field — which is the whole defect class this corpus \
         exists to refuse.",
    );
    assert_eq!(
        axis, "bincode bytes (THE COLD STORE'S FIXED POINT)",
        "the FIRST axis a dropped `conditionals` moves must be the bincode bytes — that is the \
         cold store's fixed point and the reason `PartialEq` alone is not enough"
    );

    // ── ⚠★ THE SECOND LEG, and a MEASURED REFUTATION of the design's premise ──
    //
    // The plan's argument for carrying byte axes at all was: "`eq` ignores
    // `locally_free`, so bincode catches what `eq` misses". Measured, that is
    // **wrong in its second half**. `models/build.rs` rewrites every
    // `locally_free` declaration with `serialize_with =
    // serialize_as_empty_bytes`, so bincode writes EIGHT ZERO BYTES whatever the
    // field holds — the bincode axis is blind to `locally_free` too, and so is
    // the Blake2b-256 taken over it.
    //
    // What actually sees a `locally_free`-only difference is the **prost** axis
    // (protobuf RETAINS the field), plus `Debug` and `Ord`. So the reason to
    // carry prost separately is not "a second opinion on the same bytes" — it is
    // the ONLY byte axis that covers this field at all. That is a stronger
    // justification than the one the plan gave, and it is asserted rather than
    // argued.
    let mut lf_only = oracle_clone_par(&term);
    lf_only.locally_free = vec![0x01];
    assert!(
        lf_only == term,
        "`<Par as PartialEq>::eq` is documented to IGNORE `locally_free`; if it stopped doing \
         so, the argument below would need restating rather than assuming"
    );
    assert_eq!(
        bincode::serialize(&lf_only).expect("bincode"),
        bincode::serialize(&term).expect("bincode"),
        "⚠ bincode must be BLIND to `locally_free`: `models/build.rs` injects \
         `serialize_with = serialize_as_empty_bytes` on every one of the 12 declarations, so the \
         field serializes as eight zero bytes regardless of content. If this ever fails, the \
         serialize-only normalization has changed — which is a consensus-visible event in its \
         own right, and the cold store already holds bytes written under the old rule."
    );
    let lf_axis = good
        .first_difference(&Axes::of(&lf_only))
        .expect("a `locally_free` change must move at least one axis");
    assert_eq!(
        lf_axis, "prost bytes (the protobuf wire)",
        "⚠★ A `locally_free`-only difference is invisible to `PartialEq` AND to bincode AND to \
         the Blake2b-256 over bincode. The PROTOBUF axis is the only byte axis that retains the \
         field, so it is the only one that can catch a clone which dropped it. A corpus carrying \
         `eq` + bincode alone — which is what the plan proposed — would pass such a clone."
    );

    println!(
        "  RED probe: dropping `Par.conditionals` moves `{axis}`; a `locally_free`-only change \
         moves `{lf_axis}` ONLY (invisible to `PartialEq`, to bincode, and to Blake2b over it)"
    );
}

/// ★ The emitter's own join tables are consistent, and non-vacuous.
///
/// These are the values `models/build.rs` cross-checks the textual derive-strip
/// against. Restating them here as a *runtime* check means a table that went
/// empty (the `String::new()` failure mode) is caught by the test suite as well
/// as by the build.
#[test]
fn the_emitters_join_tables_describe_a_real_conversion() {
    assert!(
        !CLONE_CUT_SET.is_empty(),
        "the CLONE CUT SET is empty, so nothing was converted"
    );
    assert!(
        EMITTED_TRAVERSALS.len() >= 55,
        "`EMITTED_TRAVERSALS` carries {} rows; the schema has 51 non-`Copy` messages and 4 \
         non-`Copy` oneofs, so 55 is the floor",
        EMITTED_TRAVERSALS.len()
    );
    assert!(
        CLONE_DESCEND_SET.len() >= 30,
        "the driver enters only {} items. `Par` alone reaches `Send`, `Receive`, `ReceiveBind`, \
         `New`, `Match`, `MatchCase`, `If`, `Bundle`, `Expr`, `ExprInstance`, `Connective`, \
         `ConnectiveInstance`, `ConnectiveBody`, the fifteen binary `E*` payloads, the four \
         collection payloads and `KeyValuePair` — far more than that. A short descend set means \
         the reachability closure stopped closing, and the types it dropped would be cloned \
         WHOLE, i.e. Θ(depth).",
        CLONE_DESCEND_SET.len()
    );
    assert!(
        CLONE_RESIDUAL_HEIGHT > 0,
        "a residual height of 0 would mean the residual graph has no edges at all, which \
         contradicts `CLONE_DESCEND_SET` being non-empty"
    );
    assert!(
        CLONE_RESIDUAL_HEIGHT < 16,
        "the residual height is {CLONE_RESIDUAL_HEIGHT}. It bounds a generated field-wise \
         `clone`'s NATIVE recursion, so it must be a small constant of the schema; a large value \
         means the cut set is not cutting."
    );
    assert_eq!(
        CLONE_EXTERN_BOUNDED,
        &["EPathMap"],
        "⚠ the set of EXTERN types the clone driver treats as BOUNDED has changed. That \
         treatment is only correct because `EPathMap::clone` is O(1) AT THE NODE (`ps` is an \
         `EntryTrie`: a refcount bump). A new extern type inherits that claim WITHOUT having \
         earned it — give it a `stack_depth_gate` ladder first."
    );

    let driven: Vec<&str> = EMITTED_TRAVERSALS
        .iter()
        .filter(|(_, shape)| *shape == "driven")
        .map(|(ty, _)| *ty)
        .collect();
    assert_eq!(
        driven, CLONE_CUT_SET,
        "the items with a DRIVEN body must be exactly the cut set"
    );
    println!(
        "  cut set {CLONE_CUT_SET:?}, residual height {CLONE_RESIDUAL_HEIGHT}, \
         {} items entered, {} impls emitted",
        CLONE_DESCEND_SET.len(),
        EMITTED_TRAVERSALS.len()
    );
}

/// ★ `ListBindPatterns` / `BindPattern` / `ListParWithRandom` — the three
/// non-`Copy` messages that `Par` cannot reach.
///
/// The driver never enters them, so their `Clone` is generated FIELD-WISE. That
/// is the majority body shape (54 of 55) and it would be untested by a corpus
/// that only walked `Par`.
#[test]
fn the_field_wise_impls_that_par_cannot_reach_are_faithful() {
    let bind = BindPattern {
        patterns: vec![gstr("p"), gint(-1)],
        remainder: Some(Var {
            var_instance: Some(VarInstance::FreeVar(-9)),
        }),
        free_count: 2,
    };
    let list = ListBindPatterns {
        patterns: vec![bind.clone(), BindPattern::default()],
    };
    let datum = ListParWithRandom {
        pars: vec![marked(gint(1)), marked(gstr("two"))],
        random_state: vec![0x5a; 32],
    };

    for (label, before, after) in [
        (
            "BindPattern",
            bincode::serialize(&bind).expect("bincode"),
            bincode::serialize(&bind.clone()).expect("bincode"),
        ),
        (
            "ListBindPatterns",
            bincode::serialize(&list).expect("bincode"),
            bincode::serialize(&list.clone()).expect("bincode"),
        ),
        (
            "ListParWithRandom",
            bincode::serialize(&datum).expect("bincode"),
            bincode::serialize(&datum.clone()).expect("bincode"),
        ),
    ] {
        assert_eq!(
            before, after,
            "★ `{label}`'s GENERATED field-wise `Clone` does not reproduce its source's bincode \
             bytes. It is one of the three non-`Copy` messages `Par` cannot reach, so no \
             `Par`-rooted corpus covers it."
        );
    }
    assert_eq!(
        format!("{list:?}"),
        format!("{:?}", list.clone()),
        "`ListBindPatterns`'s `Debug` moved, i.e. a field order or a `locally_free` changed"
    );
    println!("  BindPattern / ListBindPatterns / ListParWithRandom clone byte-identically");
}
