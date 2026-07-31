//! # The ENCODER differential — four properties, and why round-trip is not one
//!
//! ⚠ **Round-trip is not the property.** A codec that encodes differently but
//! decodes its own output round-trips perfectly and forks consensus on the
//! first block. What must hold is agreement with the *derived* path, in both
//! directions and on both verdicts:
//!
//! ```text
//!   ∀t. new_encode(t)             == old_encode(t)              WRITE      (§1)
//!   ∀t. new_decode(old_encode(t)) == old_decode(old_encode(t))  READ       (§2)
//!   ∀t. new_decode(new_encode(t)) == t                          necessary, (§3)
//!                                                               INSUFFICIENT
//!   ∀b. new_decode(b).is_err()    == old_decode(b).is_err()     REJECTION  (§4)
//! ```
//!
//! ★ The derived `Serialize` therefore **stays compiled and callable**. It is
//! the encode oracle for exactly the reason `bincode_decoder_differential` keeps the
//! derived `Deserialize` as the decode oracle: it is compiler-generated from
//! the same struct definitions the table is generated from, and is therefore
//! *undriftable* in a way a second hand-written implementation could never be.
//!
//! ## ★★ Anti-vacuity
//!
//! A green differential between two functions that are secretly the same
//! function is indistinguishable from a green differential between two
//! functions that agree. [`the_encode_differential_can_go_red`] settles that
//! by construction: it runs the very verdict this file asserts against
//! deliberately perturbed byte strings — one with two field emissions swapped,
//! one with a variant index changed — and requires the verdict to REJECT,
//! naming the clause. Without it, a pass here would carry no information.
//!
//! ## Corpus
//!
//! Three layers, all deterministic except the last, and all shared with the
//! decoder differential so neither direction can be tested on a corpus the
//! other never sees:
//!
//! * **exhaustive** — every `ExprInstance` arm, every `ConnectiveInstance`
//!   arm, every `Par` field, every root type, both `EPathMap` serialize arms.
//!   ★ Coverage is asserted against `EXPR_INSTANCE_VARIANT_COUNT`, which is
//!   `EXPR_INSTANCE_VARIANTS.len()` — **generated**, so a 37th arm moves the
//!   bar automatically instead of leaving every assertion passing.
//! * **structural** — the generated variant set crossed with arity 0/1/many
//!   and the awkward field combinations, enumerated rather than sampled.
//! * **proptest** — depth and breadth beyond what enumeration reaches.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::bincode_encoder::{encode, ColdStoreEncode};
use models::rust::rholang::wire::WireNode;
use models::rust::rholang::wire_schema::{
    CONNECTIVE_INSTANCE_VARIANT_COUNT, EXPR_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANT_COUNT,
};
use models::rust::test_utils::test_utils::generate_par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::{Deserialize, Serialize};

mod par_corpus;
use par_corpus as corpus;

// ===========================================================================
// §0  The VERDICT, separated from the subject
// ===========================================================================

/// **The write-side verdict**, as a pure function of two observations.
///
/// Separating it from the encoder is what makes
/// [`the_encode_differential_can_go_red`] possible at all: a verdict welded to
/// the production encoder can only ever be handed bytes that encoder produced,
/// and every such observation is — by construction — one it agrees with. With
/// the judgement apart from the subject, the judge can be shown a *wrong*
/// answer and required to reject it.
fn byte_identity_verdict(label: &str, machine: &[u8], oracle: &[u8]) -> Result<(), String> {
    if machine == oracle {
        return Ok(());
    }
    if machine.len() != oracle.len() {
        return Err(format!(
            "WRITE DIFFERENTIAL FAILED for `{label}`: length {} vs oracle {}. \
             A length difference means a field was emitted, omitted, or given the wrong \
             prefix — not merely mis-ordered.",
            machine.len(),
            oracle.len()
        ));
    }
    let at = machine
        .iter()
        .zip(oracle)
        .position(|(a, b)| a != b)
        .expect("equal length and unequal contents implies a differing index");
    Err(format!(
        "WRITE DIFFERENTIAL FAILED for `{label}`: first difference at byte {at} \
         (machine 0x{:02x}, oracle 0x{:02x}); both are {} bytes. The single-walk emitter \
         and the derived `Serialize` disagree, which is a CONSENSUS FORK.",
        machine[at],
        oracle[at],
        machine.len()
    ))
}

/// Property §1 — byte-identical WRITE.
fn assert_writes_identically<T>(label: &str, value: &T)
where
    T: WireNode + Serialize,
{
    let oracle = bincode::serialize(value).expect("oracle: serialize");
    let machine = encode(value);
    if let Err(why) = byte_identity_verdict(label, &machine, &oracle) {
        panic!("{why}");
    }
}

/// Properties §1 + §2 + §3 together, for a root type.
fn assert_all_properties<T>(label: &str, value: &T)
where
    T: WireNode + Serialize + for<'de> Deserialize<'de> + ColdStoreDecode + PartialEq + std::fmt::Debug,
{
    // §1 WRITE: byte-identical to the derived encoder.
    let oracle_bytes = bincode::serialize(value).expect("oracle: serialize");
    let machine_bytes = encode(value);
    if let Err(why) = byte_identity_verdict(label, &machine_bytes, &oracle_bytes) {
        panic!("{why}");
    }

    // §2 READ: the trampolined decoder agrees with the derived decoder on the
    // ORACLE's bytes — the bytes already in every node's cold store.
    let oracle_value: T = bincode::deserialize(&oracle_bytes).expect("oracle: deserialize");
    let machine_value = T::cold_decode(&oracle_bytes).expect("machine: decode oracle bytes");
    assert!(
        machine_value == oracle_value,
        "READ DIFFERENTIAL FAILED for `{label}`: the machine and the derived decoder \
         disagree about the ORACLE's bytes"
    );

    // §3 round-trip through the new encoder. Necessary, and on its own
    // insufficient — it is asserted third precisely so it cannot be mistaken
    // for the property.
    let round = T::cold_decode(&machine_bytes).expect("machine: decode its own bytes");
    assert!(
        round == oracle_value,
        "ROUND-TRIP FAILED for `{label}` (necessary, not sufficient — §1 is the property)"
    );
}

// ===========================================================================
// §1  Byte-identical WRITE over the exhaustive corpus
// ===========================================================================

#[test]
fn every_expr_instance_writes_identically() {
    let arms = corpus::every_expr_instance();
    // ★ Coverage against the GENERATED variant set, never a hand list. A 37th
    // arm added to the `.proto` moves this bar by itself.
    assert_eq!(
        arms.len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        "the corpus must carry one representative of EVERY generated ExprInstance arm; \
         the generated table has {} and the corpus has {}",
        EXPR_INSTANCE_VARIANT_COUNT,
        arms.len()
    );
    for (label, instance) in arms {
        let expr = Expr {
            expr_instance: Some(instance),
        };
        assert_writes_identically(&format!("Expr::{label}"), &expr);
    }
}

#[test]
fn every_connective_instance_writes_identically() {
    let arms = corpus::every_connective_instance();
    assert_eq!(
        arms.len(),
        CONNECTIVE_INSTANCE_VARIANT_COUNT,
        "the corpus must carry one representative of EVERY generated ConnectiveInstance arm"
    );
    for (label, instance) in arms {
        let connective = models::rhoapi::Connective {
            connective_instance: Some(instance),
        };
        assert_writes_identically(&format!("Connective::{label}"), &connective);
    }
}

#[test]
fn every_unforgeable_and_opt_var_writes_identically() {
    for (i, unf) in corpus::every_unforgeable().into_iter().enumerate() {
        assert_writes_identically(&format!("GUnforgeable[{i}]"), &unf);
    }
    for (i, var) in corpus::every_opt_var().into_iter().enumerate() {
        // `Option<Var>` is reached as a FIELD, so exercise it in place.
        let bp = BindPattern {
            patterns: vec![corpus::gint(i as i64)],
            remainder: var,
            free_count: i as i32,
        };
        assert_writes_identically(&format!("BindPattern.remainder[{i}]"), &bp);
    }
}

#[test]
fn every_root_and_shape_satisfies_all_four_properties() {
    let mut checked = 0usize;
    for (label, par) in corpus::par_corpus() {
        assert_all_properties(&format!("Par::{label}"), &par);
        checked += 1;
    }
    for (label, v) in corpus::list_par_with_random_corpus() {
        assert_all_properties(&format!("ListParWithRandom::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        assert_all_properties(&format!("BindPattern::{label}"), &v);
        checked += 1;
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        assert_all_properties(&format!("TaggedContinuation::{label}"), &v);
        checked += 1;
    }
    assert!(
        checked >= 40,
        "the shared corpus collapsed to {checked} cases — a differential over an empty \
         corpus reports a comfortable pass"
    );
}

#[test]
fn the_awkward_wire_shapes_write_identically() {
    // Every shape the module docs call out as a place a hand-written codec
    // drifts, each asserted by BYTES rather than by value.
    assert_writes_identically("all_par_fields", &corpus::all_par_fields());
    assert_writes_identically("nonground_pathmap", &corpus::nonground_pathmap());
    assert_writes_identically("ground_pathmap", &corpus::ground_pathmap());
    // ★ **THERE IS NO LONGER A SECOND ARM TO DEGRADE INTO**, and this leg is
    // rewritten to say so rather than deleted.
    //
    // It used to assert that a ground map and an otherwise-identical NON-ground
    // twin (one `locally_free` bit, which defeats the ground predicate) write
    // DIFFERENT bytes — because the ground arm re-ordered `ps` into canonical
    // trie order on serialize while the non-ground arm wrote the stored order.
    // The `assert_ne!` was the guard that the ground fixture really was taking
    // the canonical path.
    //
    // An `EPathMap` stores a trie, so the stored order IS the canonical order
    // and both twins now emit the same entry sequence. The property worth
    // pinning is therefore the opposite one: the canonicalization is not
    // conditional on being ground, so the two agree ENTRY-FOR-ENTRY and differ
    // only where the metadata genuinely differs.
    let ground = corpus::ground_pathmap();
    let non_ground_twin = {
        let mut plain = ground.clone();
        plain.locally_free = vec![7];
        plain
    };
    assert_eq!(
        ground.ps(),
        non_ground_twin.ps(),
        "the entry order must not depend on the ground predicate any more — both \
         arms read the same trie"
    );
    // …and the bytes still differ, because `locally_free` reaches the prost
    // wire (it is blanked only on the serde one), so this remains a live case
    // rather than a tautology.
    assert_ne!(
        prost::Message::encode_to_vec(&ground),
        prost::Message::encode_to_vec(&non_ground_twin),
        "prost RETAINS locally_free, so the two twins must still differ on the wire"
    );
    for depth in [1usize, 2, 8, 48] {
        assert_writes_identically(&format!("deep_par({depth})"), &corpus::deep_par(depth));
        assert_writes_identically(
            &format!("deep_mixed_par({depth})"),
            &corpus::deep_mixed_par(depth),
        );
    }
}

// ===========================================================================
// §2  The STRUCTURAL enumeration — variant × arity × awkward combination
// ===========================================================================

/// Every generated variant crossed with arity 0 / 1 / many, and with each of
/// the awkward field combinations, enumerated exhaustively and
/// deterministically. **No seed, no regressions file, no flake.**
#[test]
fn the_structural_cross_product_writes_identically() {
    let arities: [usize; 3] = [0, 1, 3];
    let mut cases = 0usize;
    for (label, instance) in corpus::every_expr_instance() {
        for &arity in &arities {
            for &connective_used in &[false, true] {
                for &with_remainder in &[false, true] {
                    let par = Par {
                        exprs: vec![Expr {
                            expr_instance: Some(instance.clone()),
                        }],
                        sends: (0..arity)
                            .map(|i| models::rhoapi::Send {
                                chan: Some(corpus::gint(i as i64)),
                                data: (0..arity).map(|j| corpus::gint(j as i64)).collect(),
                                persistent: i % 2 == 0,
                                locally_free: vec![i as u8],
                                connective_used,
                            })
                            .collect(),
                        connectives: (0..arity)
                            .map(|_| models::rhoapi::Connective {
                                connective_instance: None,
                            })
                            .collect(),
                        locally_free: vec![1, 2, 3],
                        connective_used,
                        ..Default::default()
                    };
                    let bp = BindPattern {
                        patterns: vec![par.clone()],
                        remainder: if with_remainder {
                            corpus::every_opt_var().into_iter().flatten().next()
                        } else {
                            None
                        },
                        free_count: arity as i32,
                    };
                    assert_writes_identically(
                        &format!("cross[{label}/{arity}/{connective_used}/{with_remainder}]"),
                        &par,
                    );
                    assert_writes_identically(
                        &format!("cross-bp[{label}/{arity}/{connective_used}/{with_remainder}]"),
                        &bp,
                    );
                    cases += 2;
                }
            }
        }
    }
    assert_eq!(
        cases,
        EXPR_INSTANCE_VARIANT_COUNT * arities.len() * 2 * 2 * 2,
        "the cross product must be complete: every generated variant × every arity × \
         every awkward combination"
    );
    assert!(cases >= 800, "the enumeration collapsed to {cases} cases");
}

/// The ground literals whose bit patterns are easy to normalize by accident.
#[test]
fn the_awkward_ground_literals_write_identically() {
    let mut instances: Vec<(String, ExprInstance)> = Vec::new();
    for (name, bits) in [
        ("+0.0", 0.0f64.to_bits()),
        ("-0.0", (-0.0f64).to_bits()),
        ("NaN", f64::NAN.to_bits()),
        ("NaN-payload", 0x7ff8_0000_dead_beefu64),
        ("+inf", f64::INFINITY.to_bits()),
        ("-inf", f64::NEG_INFINITY.to_bits()),
        ("min-subnormal", 1u64),
    ] {
        instances.push((format!("GDouble({name})"), ExprInstance::GDouble(bits)));
    }
    for (name, n) in [
        ("i64::MIN", i64::MIN),
        ("i64::MAX", i64::MAX),
        ("-1", -1i64),
        ("0", 0i64),
    ] {
        instances.push((format!("GInt({name})"), ExprInstance::GInt(n)));
    }
    instances.push((
        "GBigRat".into(),
        ExprInstance::GBigRat(models::rhoapi::GBigRational {
            numerator: vec![0xff, 0x00, 0x80],
            denominator: vec![0x01],
        }),
    ));
    instances.push((
        "GBigRat(empty)".into(),
        ExprInstance::GBigRat(models::rhoapi::GBigRational {
            numerator: vec![],
            denominator: vec![],
        }),
    ));
    instances.push((
        "GFixedPoint".into(),
        ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
            unscaled: vec![0x80, 0x00],
            scale: u32::MAX,
        }),
    ));
    instances.push(("GBigInt(empty)".into(), ExprInstance::GBigInt(vec![])));
    instances.push((
        "GBigInt(high-bit)".into(),
        ExprInstance::GBigInt(vec![0x80, 0xff]),
    ));
    instances.push(("GString(empty)".into(), ExprInstance::GString(String::new())));
    instances.push((
        "GString(multibyte)".into(),
        // Deliberately mixes 2-, 3- and 4-byte UTF-8, so a byte-vs-char length
        // confusion cannot survive.
        ExprInstance::GString("é✓𝄞".to_string()),
    ));
    instances.push((
        "GUri(multibyte)".into(),
        ExprInstance::GUri("rho:é/𝄞".to_string()),
    ));
    instances.push((
        "GByteArray(0x00)".into(),
        ExprInstance::GByteArray(vec![0u8; 3]),
    ));

    for (label, instance) in instances {
        assert_writes_identically(
            &label,
            &Expr {
                expr_instance: Some(instance),
            },
        );
    }
}

// ===========================================================================
// §3  ★★ ANTI-VACUITY — the differential can go RED
// ===========================================================================

/// **The mutation proof.**
///
/// Two byte-visible perturbations of what the encoder emits, each fed to the
/// very verdict [`assert_writes_identically`] uses:
///
/// 1. **two field emissions swapped** — `Send.persistent` and
///    `Send.connective_used` exchanged, which is a *pure permutation*: the
///    length is unchanged, so only a byte-level comparison can see it;
/// 2. **one variant index changed** — `EX_G_INT` (1) written where `EX_G_BOOL`
///    (0) belongs, the exact failure mode of reading a proto tag as a serde
///    index.
///
/// A CONTROL — the unperturbed bytes — must pass the same verdict in the same
/// run, so a verdict that rejected everything would fail here too.
///
/// ⚠ Without this test a green differential is indistinguishable from
/// comparing a function to itself.
#[test]
fn the_encode_differential_can_go_red() {
    let send = models::rhoapi::Send {
        chan: Some(corpus::gint(1)),
        data: vec![corpus::gint(2)],
        // The two flags must DIFFER, or swapping them is a no-op and the
        // mutation would be silently inert.
        persistent: true,
        locally_free: vec![],
        connective_used: false,
    };
    let par = Par {
        sends: vec![send],
        ..Default::default()
    };
    let truth = bincode::serialize(&par).expect("oracle");

    // CONTROL: the real encoder, judged by the real verdict.
    assert!(
        byte_identity_verdict("control", &encode(&par), &truth).is_ok(),
        "the CONTROL must pass, or every rejection below is a statement about the judge \
         rather than about the mutation"
    );

    // MUTATION 1 — swap two adjacent single-byte field emissions.
    //
    // `Send` is `chan, data, persistent, locally_free, connective_used`. The
    // encoder writes `persistent` (1 byte) then `locally_free` (8 bytes of
    // zero) then `connective_used` (1 byte). Exchanging the two flags is a
    // permutation of the SAME multiset of bytes.
    let persistent_at = truth
        .iter()
        .position(|&b| b == 1)
        .expect("the encoding contains the persistent flag");
    let connective_at = truth.len() - 1;
    let mut swapped = truth.clone();
    swapped.swap(persistent_at, connective_at);
    assert_eq!(
        swapped.len(),
        truth.len(),
        "the swap must preserve length, or it would be caught by a length check rather \
         than by byte identity"
    );
    let verdict = byte_identity_verdict("swapped-fields", &swapped, &truth);
    let why = verdict.expect_err("MUTATION 1 must be REJECTED");
    assert!(
        why.contains("first difference at byte"),
        "MUTATION 1 must be rejected by the BYTE-IDENTITY clause (a pure permutation is \
         invisible to a length check); got: {why}"
    );

    // MUTATION 2 — change one variant index.
    let expr = Expr {
        expr_instance: Some(ExprInstance::GBool(true)),
    };
    let expr_truth = bincode::serialize(&expr).expect("oracle");
    // Layout: Option tag (1 byte) then the u32 LE variant index.
    assert_eq!(expr_truth[0], 1, "Some(..) is Option tag 1");
    assert_eq!(
        u32::from_le_bytes([expr_truth[1], expr_truth[2], expr_truth[3], expr_truth[4]]),
        models::rust::rholang::wire_schema::EX_G_BOOL,
        "the fixture must actually be the GBool arm"
    );
    let mut relabelled = expr_truth.clone();
    relabelled[1..5].copy_from_slice(
        &models::rust::rholang::wire_schema::EX_G_INT.to_le_bytes(),
    );
    let why = byte_identity_verdict("relabelled-variant", &relabelled, &expr_truth)
        .expect_err("MUTATION 2 must be REJECTED");
    assert!(
        why.contains("first difference at byte 1"),
        "MUTATION 2 must be rejected at the variant index itself; got: {why}"
    );

    // And the control again, AFTER both mutations, so a verdict that latched
    // into rejecting cannot pass this test.
    assert!(
        byte_identity_verdict("control-after", &encode(&par), &truth).is_ok(),
        "the verdict must still ACCEPT the truth after rejecting both mutations"
    );
}

/// The generated table is what the assertions are measured against, so a
/// degenerate table would make every coverage assertion above vacuous.
#[test]
fn the_generated_variant_table_is_not_degenerate() {
    assert_eq!(
        EXPR_INSTANCE_VARIANTS.len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        "the count must BE the table's length"
    );
    assert!(
        EXPR_INSTANCE_VARIANT_COUNT >= 36,
        "the schema has at least 36 ExprInstance arms; a smaller table means the generator \
         silently dropped some"
    );
    for (i, v) in EXPR_INSTANCE_VARIANTS.iter().enumerate() {
        assert_eq!(
            v.serde_index, i as u32,
            "generated indices must be DENSE and in declaration order — `{}` claims {} at \
             position {i}",
            v.name, v.serde_index
        );
        assert!(!v.name.is_empty(), "variant {i} has no name");
    }
    // ⚠ The one index the whole design turns on: proto tag 32, serde index 25.
    let pathmap = EXPR_INSTANCE_VARIANTS
        .iter()
        .find(|v| v.name == "EPathmapBody")
        .expect("EPathmapBody must be in the generated table");
    assert_eq!(
        pathmap.serde_index, 25,
        "EPathmapBody is proto TAG 32 and serde INDEX 25. If this ever reads 32, the \
         generator has started reading tags as indices and 12 of 36 arms are mis-labelled."
    );
}

// ===========================================================================
// §4  Proptest — depth and breadth past what enumeration reaches
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// §1 over arbitrary generated terms.
    #[test]
    fn generated_pars_write_identically(par in generate_par(3)) {
        let oracle = bincode::serialize(&par).expect("oracle");
        let machine = encode(&par);
        prop_assert!(
            byte_identity_verdict("generated", &machine, &oracle).is_ok(),
            "{}", byte_identity_verdict("generated", &machine, &oracle).unwrap_err()
        );
    }

    /// §1 + §2 + §3 through every cold-store root.
    #[test]
    fn all_properties_through_every_root(par in generate_par(2)) {
        let datum = ListParWithRandom { pars: vec![par.clone()], random_state: vec![1u8, 2, 3] };
        let pattern = BindPattern { patterns: vec![par.clone()], remainder: None, free_count: 3 };
        let cont = TaggedContinuation {
            guard: Some(par.clone()),
            tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ScalaBodyRef(7)),
        };

        for (label, bytes, oracle) in [
            ("Par", encode(&par), bincode::serialize(&par).expect("o")),
            ("ListParWithRandom", encode(&datum), bincode::serialize(&datum).expect("o")),
            ("BindPattern", encode(&pattern), bincode::serialize(&pattern).expect("o")),
            ("TaggedContinuation", encode(&cont), bincode::serialize(&cont).expect("o")),
        ] {
            prop_assert_eq!(&bytes, &oracle, "{} must write identically", label);
        }

        // §2 the machine decoder agrees with the derived one on ORACLE bytes.
        let oracle_bytes = bincode::serialize(&par).expect("o");
        let oracle_value: Par = bincode::deserialize(&oracle_bytes).expect("oracle decode");
        let machine_value = Par::cold_decode(&oracle_bytes).expect("machine decode");
        prop_assert!(machine_value == oracle_value);

        // §3 round-trip (necessary, insufficient).
        prop_assert!(Par::cold_decode(&encode(&par)).expect("round") == oracle_value);
    }

    /// Wide sequences: the counted-repeat path with many siblings.
    #[test]
    fn wide_sequences_write_identically(n in 0usize..64) {
        let par = Par {
            exprs: (0..n).map(|i| Expr { expr_instance: Some(ExprInstance::GInt(i as i64)) }).collect(),
            sends: (0..n).map(|i| models::rhoapi::Send {
                chan: Some(corpus::gint(i as i64)),
                ..Default::default()
            }).collect(),
            ..Default::default()
        };
        prop_assert_eq!(encode(&par), bincode::serialize(&par).expect("oracle"));
    }
}

// ===========================================================================
// §5  Depth — past every old ceiling, on the WRITE side
// ===========================================================================

/// The write side must not be capped, so the depth sweep goes past every
/// ceiling the read side ever had: the derived prost decode `Err` at 34, the
/// **retired** `COLLECTION_DEPTH_LIMIT` of 32 (deleted — the trie reader is now
/// total in depth; `models/tests/epathmap_tag8_read_totality.rs`), and the
/// bincode ENCODE overflow bisected at 9,335.
///
/// ⚠ The ORACLE is the constraint above ~60 in debug (`bincode::serialize` is
/// Θ(depth)), so the oracle leg runs on a large stack while the machine leg
/// runs on the production-faithful one.
#[test]
fn deep_terms_write_identically_past_every_old_ceiling() {
    for depth in [4usize, 32, 33, 34, 48] {
        let par = corpus::deep_par(depth);
        assert_writes_identically(&format!("deep({depth})"), &par);
    }

    // Past the ENCODE ceiling. Building, oracle-encoding and dropping the term
    // are each Θ(depth), so all three run on a stack that never binds; only the
    // machine's encode is measured on the small one.
    for depth in [1_000usize, 9_335, 12_000] {
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                let par = corpus::deep_par(depth);
                let oracle = bincode::serialize(&par).expect("oracle: deep serialize");
                (par, oracle)
            })
            .expect("spawn a big-stack builder");
        let (par, oracle) = handle.join().expect("the oracle leg must survive");

        // ★ The machine leg, on the PRODUCTION-faithful 2 MiB stack — the size
        // `gate_child` sets explicitly, which is the measurement that matters.
        // `RUST_MIN_STACK` is irrelevant to an explicitly sized thread.
        let machine = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let bytes = par.cold_encode();
                // `<Par as Drop>` is itself Θ(depth); releasing a 12,000-deep
                // term on a 2 MiB stack would abort the process and be read as
                // "the encoder overflowed".
                std::mem::forget(par);
                bytes
            })
            .expect("spawn the machine leg")
            .join()
            .expect("the machine must survive a depth no native stack could");

        assert!(
            byte_identity_verdict(&format!("deep({depth})"), &machine, &oracle).is_ok(),
            "{}",
            byte_identity_verdict(&format!("deep({depth})"), &machine, &oracle).unwrap_err()
        );
    }
}
