//! # DR-131 P2b: the differential of the cold-store decode machine
//!
//! Ported from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/tests/bincode_decoder_differential.rs`) to this branch's
//! machine, `models::rust::rholang::record_decoder`.
//!
//! ## What is proved
//!
//! The machine must accept the same language as the derived decoder. For every
//! byte string `b`,
//!
//! ```text
//!     T::cold_decode(b)   and   bincode::deserialize::<T>(b)
//! ```
//!
//! give the same value, or both fail. This file covers the `Ok` half on
//! well-formed inputs. `record_decoder_malformed.rs` covers the rejection set,
//! which is the half that consensus sees.
//!
//! ## The oracle
//!
//! Every `rhoapi` type still derives `Deserialize`. The production read path
//! no longer calls it, so the compiler-generated decoder is the oracle, and it
//! cannot drift from the format.
//!
//! ## Value equality
//!
//! The hand-written `PartialEq` of the `rhoapi` types (`models/src/lib.rs`)
//! skips `locally_free`. So every comparison here also compares the `Debug`
//! text, which holds every field. A decoder that read the right length of a
//! `locally_free` field but kept the wrong bytes fails that comparison.
//!
//! ## Why `decode(encode(x)) == x` is not the main property
//!
//! The serializer writes the 13 `locally_free` fields as empty bytes, so a
//! round trip loses them, for the derived decoder too. The round trip is
//! asserted only with `PartialEq`, which skips those fields. Everything else is
//! asserted as a differential, which holds whatever the encoder does.
//!
//! ## Changes from the F1R3Lang file
//!
//! - The trie-snapshot test is not ported. This branch's `EPathMap` is a plain
//!   message with derived serde and no trie snapshot.
//! - The random corpus is `record_corpus::random_par`, because the workspace
//!   generator is vacuous on this branch (see the corpus module).
//! - The alphabet checks also cover the six cost-signature arms.
//! - The deep cost-signature test uses 100,000 levels, releases the value with
//!   `dismantle_term`, and also rejects a truncated row that holds the deep
//!   value.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use models::rhoapi::cost_signature::Value as CostValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostSignature, CostSignatureCompound, CostStack, ETuple, EZipper, Expr,
    ListBindPatterns, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::rholang::dismantle::dismantle_term;
use models::rust::rholang::record_decoder::{
    cold_decode_list_bind_patterns_for_test, cold_decode_par_with_random_for_test,
    connective_instance_variant_index, cost_value_variant_index, expr_instance_variant_index,
    CS_COMPOUND, CS_UNIT,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::de::DeserializeOwned;
use serde::Serialize;

mod record_corpus;
use record_corpus as corpus;

/// Equality of every field: `PartialEq` and the `Debug` text.
fn same_value<T: PartialEq + Debug>(left: &T, right: &T) -> bool {
    left == right && format!("{left:?}") == format!("{right:?}")
}

/// The verdict of the differential, as a pure function of one observation.
///
/// It is separate from [`agree`] so that `the_differential_can_go_red` can
/// run it on the output of a drifted decoder and show that it rejects, and
/// which of its two clauses rejects.
///
/// The extent clause matters: a native history row holds the root as a
/// prefix, so a decoder that read the right value but reported the wrong
/// extent would read the following fields from the wrong place.
fn differential_verdict<T: PartialEq + Debug>(
    label: &str,
    machine: &T,
    consumed: usize,
    oracle: &T,
    exact_len: usize,
) -> Result<(), String> {
    if !same_value(machine, oracle) {
        return Err(format!(
            "{label}: the machine and the derived oracle produced DIFFERENT values"
        ));
    }
    if consumed != exact_len {
        return Err(format!(
            "{label}: the machine reported the wrong byte extent ({consumed}, want \
             {exact_len}). A row holds the root as a PREFIX, so a wrong extent reads the \
             fields that follow it from the wrong place"
        ));
    }
    Ok(())
}

/// The machine and the derived oracle agree on the encoding of `value`, and
/// the machine reports the exact length of the encoding.
fn agree<T>(label: &str, value: &T)
where T: ColdStoreDecode + Serialize + DeserializeOwned + PartialEq + Debug {
    let bytes = bincode::serialize(value).expect("oracle: serialize");
    let oracle: T = bincode::deserialize(&bytes).expect("oracle: deserialize");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: the machine rejected a well-formed encoding: {e}"));
    if let Err(why) = differential_verdict(label, &machine, consumed, &oracle, bytes.len()) {
        panic!("{why}");
    }
}

/// `allow_trailing_bytes()` is part of the options of the reader. A decoder
/// that rejected trailing bytes would accept a smaller language, which is
/// also a fork.
fn agree_with_trailing<T>(label: &str, value: &T)
where T: ColdStoreDecode + Serialize + DeserializeOwned + PartialEq + Debug {
    let mut bytes = bincode::serialize(value).expect("oracle: serialize");
    let exact_len = bytes.len();
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let oracle: T = bincode::deserialize(&bytes)
        .expect("oracle: trailing bytes must be ACCEPTED (allow_trailing_bytes)");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: the machine rejected trailing bytes: {e}"));
    assert!(
        same_value(&machine, &oracle),
        "{label}: disagreement with trailing bytes"
    );
    assert_eq!(
        consumed, exact_len,
        "{label}: the extent must exclude the trailing bytes"
    );
}

// ===========================================================================
// Each corpus reaches the whole alphabet that it is credited with
// ===========================================================================
//
// A test that asserts agreement arm by arm cannot falsify a claim about an
// arm that its corpus never reaches, however many cases the corpus has. On the
// F1R3Lang line a one-field drift in the `ETuple` arm turned six tests of that
// file red and left five green, all three generator proptests among the green,
// because the generator cannot produce an `ETuple`. So each test below checks
// that its corpus reaches every `ExprInstance`, `ConnectiveInstance` and
// cost-signature arm.

/// The `ExprInstance` alphabet, keyed by serde variant index.
fn expr_alphabet() -> BTreeMap<u32, &'static str> {
    corpus::every_expr_instance()
        .into_iter()
        .map(|(name, instance)| (expr_instance_variant_index(&instance), name))
        .collect()
}

/// The `ConnectiveInstance` alphabet.
fn connective_alphabet() -> BTreeMap<u32, &'static str> {
    corpus::every_connective_instance()
        .into_iter()
        .map(|(name, instance)| (connective_instance_variant_index(&instance), name))
        .collect()
}

/// The cost-signature alphabet. The absent value has no arm.
fn cost_alphabet() -> BTreeMap<u32, &'static str> {
    corpus::every_cost_signature()
        .into_iter()
        .filter_map(|(name, signature)| {
            signature
                .value
                .as_ref()
                .map(|value| (cost_value_variant_index(value), name))
        })
        .collect()
}

fn missing(alphabet: &BTreeMap<u32, &'static str>, reached: &BTreeSet<u32>) -> Vec<&'static str> {
    alphabet
        .iter()
        .filter(|(index, _)| !reached.contains(*index))
        .map(|(_, name)| *name)
        .collect()
}

/// The verdict of the alphabet check, pure over the reached sets. The error
/// names every missing arm.
fn alphabet_verdict(label: &str, reached: &corpus::ReachedArms) -> Result<(), String> {
    let missing_exprs = missing(&expr_alphabet(), &reached.exprs);
    let missing_connectives = missing(&connective_alphabet(), &reached.connectives);
    let missing_costs = missing(&cost_alphabet(), &reached.cost_values);
    if missing_exprs.is_empty() && missing_connectives.is_empty() && missing_costs.is_empty() {
        return Ok(());
    }
    Err(format!(
        "ALPHABET COVERAGE FAILED for `{label}`: the corpus never reaches ExprInstance arm(s) \
         [{}], ConnectiveInstance arm(s) [{}] or cost-signature arm(s) [{}]. A differential \
         cannot falsify a claim about an arm that its corpus cannot express.",
        missing_exprs.join(", "),
        missing_connectives.join(", "),
        missing_costs.join(", ")
    ))
}

fn assert_alphabet_complete<'a>(label: &str, roots: impl IntoIterator<Item = &'a Par>) {
    if let Err(why) = alphabet_verdict(label, &corpus::reached_arms(roots)) {
        panic!("{why}");
    }
}

/// Every `Par` that a `TaggedContinuation` holds directly.
fn tagged_continuation_pars(v: &TaggedContinuation) -> Vec<&Par> {
    let mut out: Vec<&Par> = v.guard.iter().collect();
    if let Some(TaggedCont::ParBody(with_random)) = v.tagged_cont.as_ref() {
        out.extend(with_random.body.iter());
    }
    out
}

// ===========================================================================
// The constructed corpus
// ===========================================================================

#[test]
fn par_corpus_agrees_with_the_derived_oracle() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let cases = corpus::par_corpus();
    assert!(
        cases.len() >= 60,
        "ANTI-VACUITY: the Par corpus shrank to {} cases",
        cases.len()
    );
    assert_alphabet_complete("par_corpus", cases.iter().map(|(_, par)| par));
    for (label, par) in cases {
        agree(&label, &par);
    }
}

#[test]
fn par_corpus_accepts_trailing_bytes_exactly_as_the_oracle_does() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let cases = corpus::par_corpus();
    assert_alphabet_complete("par_corpus+trailing", cases.iter().map(|(_, par)| par));
    for (label, par) in cases {
        agree_with_trailing(&label, &par);
    }
}

#[test]
fn list_par_with_random_corpus_agrees() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let cases = corpus::list_par_with_random_corpus();
    assert_alphabet_complete(
        "list_par_with_random_corpus",
        cases.iter().flat_map(|(_, v)| v.pars.iter()),
    );
    for (label, v) in cases {
        agree(&label, &v);
        agree_with_trailing(&label, &v);
    }
}

#[test]
fn bind_pattern_corpus_agrees() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let cases = corpus::bind_pattern_corpus();
    assert_alphabet_complete(
        "bind_pattern_corpus",
        cases.iter().flat_map(|(_, v)| v.patterns.iter()),
    );
    for (label, v) in cases {
        agree(&label, &v);
        agree_with_trailing(&label, &v);
    }
}

#[test]
fn tagged_continuation_corpus_agrees() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let cases = corpus::tagged_continuation_corpus();
    assert_alphabet_complete(
        "tagged_continuation_corpus",
        cases.iter().flat_map(|(_, v)| tagged_continuation_pars(v)),
    );
    for (label, v) in cases {
        agree(&label, &v);
        agree_with_trailing(&label, &v);
    }
}

/// `ParWithRandom` and `ListBindPatterns` are on the machine but are not the
/// root of any history row. Their test entry points run their programs, so
/// that no program of the machine stays unrun.
#[test]
fn non_root_machine_types_agree() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let with_randoms = corpus::par_with_random_corpus();
    assert_alphabet_complete(
        "par_with_random_corpus",
        with_randoms.iter().flat_map(|(_, v)| v.body.iter()),
    );
    let bind_lists = corpus::list_bind_patterns_corpus();
    assert_alphabet_complete(
        "list_bind_patterns_corpus",
        bind_lists
            .iter()
            .flat_map(|(_, v)| v.patterns.iter())
            .flat_map(|p| p.patterns.iter()),
    );
    for (label, v) in with_randoms {
        let bytes = bincode::serialize(&v).expect("serialize ParWithRandom");
        let oracle: ParWithRandom = bincode::deserialize(&bytes).expect("oracle ParWithRandom");
        let (machine, consumed) =
            cold_decode_par_with_random_for_test(&bytes).expect("machine ParWithRandom");
        differential_verdict(
            &format!("ParWithRandom::{label}"),
            &machine,
            consumed,
            &oracle,
            bytes.len(),
        )
        .unwrap_or_else(|why| panic!("{why}"));
    }
    for (label, v) in bind_lists {
        let bytes = bincode::serialize(&v).expect("serialize ListBindPatterns");
        let oracle: ListBindPatterns =
            bincode::deserialize(&bytes).expect("oracle ListBindPatterns");
        let (machine, consumed) =
            cold_decode_list_bind_patterns_for_test(&bytes).expect("machine ListBindPatterns");
        differential_verdict(
            &format!("ListBindPatterns::{label}"),
            &machine,
            consumed,
            &oracle,
            bytes.len(),
        )
        .unwrap_or_else(|why| panic!("{why}"));
    }
}

// ===========================================================================
// The differential can go red
// ===========================================================================

/// The production machine with a drift injected in one arm: an `ETuple` comes
/// back with `connective_used` flipped and one element fewer. That is the
/// shape of the smallest realistic defect of a hand-written decoder, an arm
/// whose field list has drifted by one from its type. The drift is injected
/// on the returned value.
fn decode_with_an_etuple_field_drift(bytes: &[u8]) -> (Par, usize) {
    let (mut par, consumed) = Par::cold_decode_prefix(bytes).expect("drifted decoder: machine");
    for expr in par.exprs.iter_mut() {
        if let Some(ExprInstance::ETupleBody(tuple)) = expr.expr_instance.as_mut() {
            tuple.connective_used = !tuple.connective_used;
            tuple.ps.pop();
        }
    }
    (par, consumed)
}

/// The production machine with an extent one byte short.
fn decode_with_an_extent_drift(bytes: &[u8]) -> (Par, usize) {
    let (par, consumed) = Par::cold_decode_prefix(bytes).expect("drifted decoder: machine");
    (par, consumed.saturating_sub(1))
}

/// One named case of the constructed corpus. A test that skipped a renamed
/// case without a failure would hide exactly the defect it looks for.
fn corpus_case(label: &str) -> Par {
    corpus::par_corpus()
        .into_iter()
        .find(|(name, _)| name == label)
        .map(|(_, par)| par)
        .unwrap_or_else(|| panic!("the constructed corpus no longer has a case labelled `{label}`"))
}

fn etuple_index() -> u32 {
    expr_instance_variant_index(&ExprInstance::ETupleBody(ETuple::default()))
}

fn reaches_an_etuple(par: &Par) -> bool {
    corpus::reached_arms(std::iter::once(par))
        .exprs
        .contains(&etuple_index())
}

/// Both clauses of [`differential_verdict`] run on data that each excludes:
///
/// | decoder                               | reaches `ETuple`         | does not       |
/// |---------------------------------------|--------------------------|----------------|
/// | production                            | accepts                  | accepts        |
/// | [`decode_with_an_etuple_field_drift`] | rejects (value clause)   | accepts        |
/// | [`decode_with_an_extent_drift`]       | rejects (extent clause)  | rejects        |
///
/// The second row shows that a defect local to one arm is invisible to a
/// corpus that never reaches the arm. The third row shows that an extent
/// defect is not local, so every corpus catches it.
#[test]
fn the_differential_can_go_red() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let risky = corpus_case("expr::ETupleBody");
    let control = corpus_case("deep_list::7");
    assert!(
        reaches_an_etuple(&risky),
        "the risky shape must reach the ETuple arm"
    );
    assert!(
        !reaches_an_etuple(&control),
        "the control must NOT reach the ETuple arm"
    );

    for (label, par, expect_drifted_red) in [("risky", &risky, true), ("control", &control, false)]
    {
        let bytes = bincode::serialize(par).expect("serialize");
        let oracle: Par = bincode::deserialize(&bytes).expect("oracle");

        let (machine, consumed) = Par::cold_decode_prefix(&bytes).expect("machine");
        differential_verdict(label, &machine, consumed, &oracle, bytes.len())
            .expect("the PRODUCTION decoder must agree with the oracle on both shapes");

        let (drifted, drifted_consumed) = decode_with_an_etuple_field_drift(&bytes);
        let verdict = differential_verdict(label, &drifted, drifted_consumed, &oracle, bytes.len());
        if expect_drifted_red {
            let why = verdict
                .expect_err("an ETuple field drift MUST be caught on a shape with an ETuple");
            assert!(
                why.contains("DIFFERENT values"),
                "the rejection must come from the VALUE clause; got: {why}"
            );
            assert!(
                drifted != machine,
                "the drift left the value unchanged, so the rejection is not its effect"
            );
        } else {
            verdict.expect("an ETuple defect must be INVISIBLE to a shape without an ETuple");
        }

        let (same_value_drift, short_extent) = decode_with_an_extent_drift(&bytes);
        let why =
            differential_verdict(label, &same_value_drift, short_extent, &oracle, bytes.len())
                .expect_err("an extent one byte short MUST be caught");
        assert!(
            why.contains("wrong byte extent"),
            "the rejection must come from the EXTENT clause; got: {why}"
        );
    }
}

/// How many `random_par(3)` draws the alphabet measurement takes.
const GENERATOR_ALPHABET_DRAWS: u32 = 512;

/// The arms that the random corpus reaches, measured.
///
/// 1. The reached `ExprInstance` arms are a subset of [`corpus::RANDOM_EXPR_ARMS`].
/// 2. They do not contain `ETupleBody`.
/// 3. The generator really sampled: at least two distinct arms.
/// 4. The constructed corpus reaches every arm that the generator reaches, and
///    more. That is why both corpora exist.
///
/// A failure here is not a decoder defect. It means that the generator
/// changed, and the account of what its proptests can see must be derived
/// again.
#[test]
fn the_generator_corpus_cannot_reach_the_arm_the_constructed_one_can() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = corpus::random_par(3);
    let mut drawn: Vec<Par> = Vec::with_capacity(GENERATOR_ALPHABET_DRAWS as usize);
    for _ in 0..GENERATOR_ALPHABET_DRAWS {
        drawn.push(
            strategy
                .new_tree(&mut runner)
                .expect("random_par must produce a value tree")
                .current(),
        );
    }

    let generated = corpus::reached_arms(drawn.iter());
    let alphabet = expr_alphabet();
    let name_of = |index: &u32| *alphabet.get(index).unwrap_or(&"<unknown arm>");
    let reached_names: Vec<&str> = generated.exprs.iter().map(name_of).collect();

    assert!(
        generated.exprs.len() >= 2,
        "the generator reached {} distinct ExprInstance arm(s) in {GENERATOR_ALPHABET_DRAWS} \
         draws [{}]",
        generated.exprs.len(),
        reached_names.join(", ")
    );
    let unexpected: Vec<&str> = reached_names
        .iter()
        .copied()
        .filter(|name| !corpus::RANDOM_EXPR_ARMS.contains(name))
        .collect();
    assert!(
        unexpected.is_empty(),
        "random_par produced arm(s) [{}] outside its documented set [{}]",
        unexpected.join(", "),
        corpus::RANDOM_EXPR_ARMS.join(", ")
    );
    assert!(
        !generated.exprs.contains(&etuple_index()),
        "random_par produced an ETuple in {GENERATOR_ALPHABET_DRAWS} draws"
    );

    let constructed = corpus::reached_arms(
        corpus::par_corpus()
            .iter()
            .map(|(_, par)| par)
            .collect::<Vec<_>>(),
    );
    assert!(
        constructed.exprs.contains(&etuple_index()),
        "the constructed corpus must reach the arm that the generator cannot"
    );
    assert!(
        generated.exprs.is_subset(&constructed.exprs) && generated.exprs != constructed.exprs,
        "the generator's arms must be a STRICT subset of the constructed corpus's arms"
    );
    assert!(
        generated.connectives.is_subset(&constructed.connectives),
        "the generator reached ConnectiveInstance arms that the constructed corpus does not"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The differential over random terms. Its corpus does not reach every
    /// arm (see the generator test above). It explores combinations that the
    /// constructed corpus does not.
    #[test]
    fn generated_pars_agree_with_the_derived_oracle(par in corpus::random_par(3)) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        let bytes = bincode::serialize(&par).expect("serialize");
        let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
        let (machine, consumed) = Par::cold_decode_prefix(&bytes).expect("machine");
        prop_assert!(same_value(&machine, &oracle));
        prop_assert_eq!(consumed, bytes.len());
    }

    /// `cold_decode(encode(x)) == x` with `PartialEq`, which skips the
    /// `locally_free` fields that the serializer blanks.
    #[test]
    fn round_trip_over_random_par(par in corpus::random_par(3)) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        let bytes = bincode::serialize(&par).expect("serialize");
        let decoded = Par::cold_decode(&bytes).expect("machine");
        prop_assert!(decoded == par);
    }

    /// The same, inside each of the other three roots.
    #[test]
    fn round_trip_through_every_root(par in corpus::random_par(2)) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        let list = ListParWithRandom {
            pars: vec![par.clone()],
            random_state: vec![1u8, 2],
            cost_authority: Some(corpus::cost_authority()),
            cost_stack: Some(corpus::cost_stack()),
        };
        prop_assert!(ListParWithRandom::cold_decode(
            &bincode::serialize(&list).expect("serialize")).expect("machine") == list);

        let pattern = BindPattern { patterns: vec![par.clone()], remainder: None, free_count: 3 };
        prop_assert!(BindPattern::cold_decode(
            &bincode::serialize(&pattern).expect("serialize")).expect("machine") == pattern);

        let continuation = TaggedContinuation {
            guard: Some(par),
            cost_authority: None,
            tagged_cont: Some(TaggedCont::ScalaBodyRef(5)),
        };
        prop_assert!(TaggedContinuation::cold_decode(
            &bincode::serialize(&continuation).expect("serialize")).expect("machine")
            == continuation);
    }
}

// ===========================================================================
// The EPathMap shapes
// ===========================================================================

/// `EPathMap` reaches the machine through `EPathmapBody`, and through
/// `EZipper.pathmap`, whose `Option<EPathMap>` adds a tag byte that the direct
/// arm does not have.
#[test]
fn epathmap_shapes_agree() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    use models::rhoapi::EPathMap;

    let shapes = vec![
        ("with_remainder", corpus::pathmap()),
        ("plain", corpus::plain_pathmap()),
        ("empty", EPathMap::default()),
        ("locally_free_only", EPathMap {
            ps: vec![corpus::gint(1)],
            locally_free: vec![1, 2, 3],
            connective_used: false,
            remainder: None,
        }),
    ];
    for (label, map) in shapes {
        agree(
            &format!("epathmap::{label}::body"),
            &corpus::par_of(ExprInstance::EPathmapBody(map.clone())),
        );
        agree(
            &format!("epathmap::{label}::zipper"),
            &corpus::par_of(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(map),
                current_path: vec![vec![1], vec![]],
                is_write_zipper: false,
                locally_free: vec![9],
                connective_used: true,
            })),
        );
    }

    agree(
        "epathmap::absent::zipper",
        &corpus::par_of(ExprInstance::EZipperBody(EZipper {
            pathmap: None,
            current_path: vec![],
            is_write_zipper: true,
            locally_free: vec![],
            connective_used: false,
        })),
    );
}

// ===========================================================================
// The cost family
// ===========================================================================

#[test]
fn cost_schema_fields_match_derived_bincode_decoder() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{
        CostSignedTerm, GAuthorityId, GPrincipalId, GUnforgeable, Receive, ReceiveBind,
    };

    let compound = CostSignature {
        value: Some(CostValue::Compound(CostSignatureCompound {
            elements: vec![
                CostSignature {
                    value: Some(CostValue::Ground(vec![0, 255])),
                },
                CostSignature {
                    value: Some(CostValue::BoundLevel(-7)),
                },
                CostSignature {
                    value: Some(CostValue::Quote(Par::default())),
                },
                CostSignature {
                    value: Some(CostValue::Name(Par::default())),
                },
                corpus::unit_signature(),
            ],
        })),
    };
    let stack = CostStack {
        cells: vec![
            CostSignature { value: None },
            corpus::unit_signature(),
            compound.clone(),
        ],
    };
    let par = Par {
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(Par::default()),
                remainder: None,
                free_count: 1,
                cost_signature: Some(compound.clone()),
            }],
            ..Default::default()
        }],
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GFloat32(0x7fc0_dead)),
        }],
        unforgeables: vec![
            GUnforgeable {
                unf_instance: Some(UnfInstance::GAuthorityIdBody(GAuthorityId { id: vec![8] })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrincipalIdBody(GPrincipalId {
                    key_family: 2,
                    public_key: vec![9],
                })),
            },
        ],
        cost_signed_terms: vec![CostSignedTerm {
            body: Some(Par::default()),
            signature: Some(compound),
        }],
        cost_stacks: vec![stack.clone()],
        ..Default::default()
    };
    let continuation = TaggedContinuation {
        guard: Some(par.clone()),
        cost_authority: Some(corpus::cost_authority()),
        tagged_cont: None,
    };
    let list = ListParWithRandom {
        pars: vec![par.clone()],
        random_state: vec![4, 5],
        cost_authority: Some(corpus::cost_authority()),
        cost_stack: Some(stack),
    };

    agree("cost Par", &par);
    agree("cost TaggedContinuation", &continuation);
    agree("cost ListParWithRandom", &list);
    agree_with_trailing("cost Par trailing", &par);
    agree_with_trailing("cost TaggedContinuation trailing", &continuation);
    agree_with_trailing("cost ListParWithRandom trailing", &list);

    fn truncations_agree<T>(value: &T)
    where T: ColdStoreDecode + Serialize + DeserializeOwned + PartialEq + Debug {
        let bytes = bincode::serialize(value).expect("serialize");
        for end in 0..bytes.len() {
            let prefix = &bytes[..end];
            assert_eq!(
                T::cold_decode_prefix(prefix).is_ok(),
                bincode::deserialize::<T>(prefix).is_ok(),
                "truncation at {end} of {} bytes",
                bytes.len(),
            );
        }
    }
    truncations_agree(&par);
    truncations_agree(&continuation);
    truncations_agree(&list);
}

/// The nesting depth of the deep compound signature.
const DEEP_COMPOUND_NESTING: usize = 100_000;

/// The stack of the threads that decode the deep compound signature.
const SMALL_STACK_BYTES: usize = 256 * 1024;

/// The encoding of a `Par` whose one cost stack holds one signature: `depth`
/// compounds of one element each, around `Unit(true)`. It is built by hand,
/// because the derived encoder recurses once for each level.
fn encoded_par_with_compound_depth(depth: usize) -> Vec<u8> {
    let mut bytes = bincode::serialize(&Par::default()).expect("empty Par");
    // The last field of `Par` is `cost_stacks`: its count is the last 8 bytes.
    let stack_count = bytes.len() - 8;
    bytes[stack_count..].copy_from_slice(&1u64.to_le_bytes());
    bytes.reserve(8 + depth * 13 + 6);
    bytes.extend_from_slice(&1u64.to_le_bytes()); // CostStack.cells: one cell
    for _ in 0..depth {
        bytes.push(1); // Some(value)
        bytes.extend_from_slice(&CS_COMPOUND.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes()); // one element
    }
    bytes.push(1); // Some(value)
    bytes.extend_from_slice(&CS_UNIT.to_le_bytes());
    bytes.push(1); // Unit(true)
    bytes
}

/// The number of compound levels of the one signature in `par`, counted
/// without recursion.
fn compound_depth(par: &Par) -> usize {
    let mut depth = 0usize;
    let mut current = par
        .cost_stacks
        .first()
        .and_then(|stack| stack.cells.first())
        .expect("one cost stack with one cell");
    while let Some(CostValue::Compound(compound)) = current.value.as_ref() {
        depth += 1;
        current = compound.elements.first().expect("one element per level");
    }
    assert_eq!(
        current,
        &corpus::unit_signature(),
        "the innermost value is Unit(true)"
    );
    depth
}

#[test]
fn deeply_nested_cost_signature_decodes_and_tears_down_on_small_stack() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let shallow = encoded_par_with_compound_depth(3);
    let oracle: Par = bincode::deserialize(&shallow).expect("derived shallow decode");
    let (machine, consumed) = Par::cold_decode_prefix(&shallow).expect("machine shallow decode");
    assert!(same_value(&machine, &oracle));
    assert_eq!(consumed, shallow.len());
    assert_eq!(compound_depth(&machine), 3);

    let deep = encoded_par_with_compound_depth(DEEP_COMPOUND_NESTING);
    std::thread::Builder::new()
        .name("deep-compound-decode".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || {
            let (value, consumed) = Par::cold_decode_prefix(&deep).expect("deep decode");
            assert_eq!(consumed, deep.len());
            assert_eq!(compound_depth(&value), DEEP_COMPOUND_NESTING);
            dismantle_term(value);
            assert!(Par::cold_decode_prefix(&deep[..deep.len() - 1]).is_err());
        })
        .expect("spawn the small-stack thread")
        .join()
        .expect("the deep compound decodes and tears down within a small stack");
}

/// A row that fails after the deep value is complete. The machine then holds
/// the deep value on its value stacks, and its `Drop` must release it without
/// recursion.
#[test]
fn a_truncated_row_that_holds_a_deep_compound_is_rejected_on_a_small_stack() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let par = encoded_par_with_compound_depth(DEEP_COMPOUND_NESTING);
    let mut row = Vec::with_capacity(8 + par.len() + 8 + 2);
    row.extend_from_slice(&1u64.to_le_bytes()); // ListParWithRandom.pars: one
    row.extend_from_slice(&par);
    row.extend_from_slice(&0u64.to_le_bytes()); // random_state: empty
    row.push(0); // cost_authority: None
    row.push(0); // cost_stack: None
    let whole = row.clone();
    std::thread::Builder::new()
        .name("deep-compound-truncated".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || {
            let (value, consumed) =
                ListParWithRandom::cold_decode_prefix(&whole).expect("the whole row decodes");
            assert_eq!(consumed, whole.len());
            let mut pars = value.pars;
            assert_eq!(pars.len(), 1);
            let deep = pars.pop().expect("one payload");
            assert_eq!(compound_depth(&deep), DEEP_COMPOUND_NESTING);
            dismantle_term(deep);
            // Without the final `cost_stack` tag the decode fails after the
            // whole payload is built.
            assert!(
                ListParWithRandom::cold_decode_prefix(&row[..row.len() - 1]).is_err(),
                "a truncated row must be REJECTED"
            );
        })
        .expect("spawn the small-stack thread")
        .join()
        .expect("the machine overflowed a small stack while TEARING DOWN a rejected row");
}
