//! # The `par_codec` DIFFERENTIAL — machine vs. the retained derived oracle
//!
//! ## What is being proved
//!
//! The iterative decoder's obligation is **language identity**: for every byte
//! string `b`,
//!
//! ```text
//!     T::cold_decode(b)   and   bincode::deserialize::<T>(b)
//! ```
//!
//! agree — same `Ok` value, or both `Err`. This file proves the `Ok` half over
//! well-formed inputs; `models/tests/par_codec_malformed.rs` proves the `Err`
//! half, which is the consensus-visible one.
//!
//! ## Why the oracle is the DERIVE itself
//!
//! `Deserialize` is still derived on every `rhoapi` type — the production path
//! simply no longer calls it. That makes the oracle **compiler-generated**,
//! so it cannot drift from what production used to do: there is no
//! hand-maintained twin to fall out of step. (The `Serialize`/`Deserialize`
//! twins in `rspace++/src/rspace/serializers/serializers.rs` are the precedent
//! for the pattern; this conversion is in a strictly better position than that
//! one was, because there is nothing to maintain.)
//!
//! ## ⚠ Why `decode(encode(x)) == x` is NOT the top-level property
//!
//! `EPathMap::serialize` deliberately re-orders `ps` into canonical trie order
//! for GROUND maps (`models/src/rust/rhoapi_ext.rs`), so for such an `x`,
//! `bincode::deserialize(bincode::serialize(&x)) != x` — for the *derived*
//! decoder too. Round-tripping is therefore asserted only over `generate_par`,
//! which builds no `EPathMap`; everything else is asserted by DIFFERENTIAL,
//! which is the stronger statement anyway: it holds whatever the encoder does.

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::par_codec::{
    cold_decode_list_bind_patterns_for_test, cold_decode_par_with_random_for_test,
};
use models::rust::test_utils::test_utils::generate_par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::Deserialize;

mod par_codec_corpus;
use par_codec_corpus as corpus;

/// Assert that the machine and the derived oracle agree on `bytes`, and that
/// the machine reports the exact number of bytes the value occupies.
///
/// The consumed count is not incidental: `Datum<A>` embeds `A` as a *prefix*,
/// so a decoder that read the right value but reported the wrong extent would
/// silently mis-read `persist` and `source` from the middle of `a`.
fn agree<T>(label: &str, value: &T)
where
    T: ColdStoreDecode + serde::Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let bytes = bincode::serialize(value).expect("oracle: serialize");
    let oracle: T = bincode::deserialize(&bytes).expect("oracle: deserialize");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: machine rejected a well-formed encoding: {e}"));
    assert!(
        machine == oracle,
        "{label}: machine and derived oracle produced DIFFERENT values"
    );
    assert_eq!(
        consumed,
        bytes.len(),
        "{label}: machine reported the wrong byte extent — `Datum<A>` reads `A` as a \
         PREFIX, so a wrong extent silently mis-reads the fields that follow it"
    );
}

/// `allow_trailing_bytes()` is part of `bincode::deserialize`'s configuration.
/// Tightening it would NARROW the accepted language, which is itself a fork.
fn agree_with_trailing<T>(label: &str, value: &T)
where
    T: ColdStoreDecode + serde::Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut bytes = bincode::serialize(value).expect("oracle: serialize");
    let exact_len = bytes.len();
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let oracle: T = bincode::deserialize(&bytes)
        .expect("oracle: trailing bytes must be ACCEPTED (allow_trailing_bytes)");
    let (machine, consumed) = T::cold_decode_prefix(&bytes)
        .unwrap_or_else(|e| panic!("{label}: machine rejected trailing bytes: {e}"));
    assert!(machine == oracle, "{label}: disagreement with trailing bytes");
    assert_eq!(consumed, exact_len, "{label}: extent must exclude the trailing bytes");
}

// ===========================================================================
// The constructed corpus
// ===========================================================================

#[test]
fn par_corpus_agrees_with_the_derived_oracle() {
    let cases = corpus::par_corpus();
    assert!(
        cases.len() >= 60,
        "ANTI-VACUITY: the Par corpus shrank to {} cases; it must carry one \
         representative of every ExprInstance and ConnectiveInstance arm",
        cases.len()
    );
    for (label, par) in cases {
        agree(&label, &par);
    }
}

#[test]
fn par_corpus_accepts_trailing_bytes_exactly_as_the_oracle_does() {
    for (label, par) in corpus::par_corpus() {
        agree_with_trailing(&label, &par);
    }
}

#[test]
fn list_par_with_random_corpus_agrees() {
    for (label, v) in corpus::list_par_with_random_corpus() {
        agree(&label, &v);
    }
}

#[test]
fn bind_pattern_corpus_agrees() {
    for (label, v) in corpus::bind_pattern_corpus() {
        agree(&label, &v);
    }
}

#[test]
fn tagged_continuation_corpus_agrees() {
    for (label, v) in corpus::tagged_continuation_corpus() {
        agree(&label, &v);
    }
}

/// The two machine types that are never a cold-store ROOT (`ParWithRandom` is
/// reached through `TaggedCont::ParBody`, `ListBindPatterns` through the gRPC
/// surface). Exercised through their test-only entry points so that no program
/// in the machine is left unrun.
#[test]
fn non_root_machine_types_agree() {
    for (label, v) in corpus::par_with_random_corpus() {
        let bytes = bincode::serialize(&v).expect("serialize ParWithRandom");
        let oracle: models::rhoapi::ParWithRandom =
            bincode::deserialize(&bytes).expect("oracle ParWithRandom");
        let (machine, consumed) =
            cold_decode_par_with_random_for_test(&bytes).expect("machine ParWithRandom");
        assert!(machine == oracle, "ParWithRandom::{label}: disagreement");
        assert_eq!(consumed, bytes.len(), "ParWithRandom::{label}: extent");
    }
    for (label, v) in corpus::list_bind_patterns_corpus() {
        let bytes = bincode::serialize(&v).expect("serialize ListBindPatterns");
        let oracle: models::rhoapi::ListBindPatterns =
            bincode::deserialize(&bytes).expect("oracle ListBindPatterns");
        let (machine, consumed) =
            cold_decode_list_bind_patterns_for_test(&bytes).expect("machine ListBindPatterns");
        assert!(machine == oracle, "ListBindPatterns::{label}: disagreement");
        assert_eq!(consumed, bytes.len(), "ListBindPatterns::{label}: extent");
    }
}

// ===========================================================================
// The `generate_par` proptest corpus
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Differential over the workspace's own generator. Non-vacuous since the
    /// `0..1` → `0..=2` fix (see `models/src/rust/test_utils/test_utils.rs`);
    /// before it, this test would have quantified over a two-element set.
    #[test]
    fn generated_pars_agree_with_the_derived_oracle(par in generate_par(3)) {
        let bytes = bincode::serialize(&par).expect("serialize");
        let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
        let (machine, consumed) = Par::cold_decode_prefix(&bytes).expect("machine");
        prop_assert!(machine == oracle);
        prop_assert_eq!(consumed, bytes.len());
    }

    /// `cold_decode(encode(x)) == x` over `generate_par`.
    ///
    /// Restricted to this generator on purpose: it builds no `EPathMap`, so it
    /// avoids the GROUND-map canonical re-ordering that makes round-tripping
    /// false for the DERIVED decoder as well. See the module header.
    #[test]
    fn round_trip_over_generate_par(par in generate_par(3)) {
        let bytes = bincode::serialize(&par).expect("serialize");
        let decoded = Par::cold_decode(&bytes).expect("machine");
        prop_assert!(decoded == par);
    }

    /// The same, wrapped in each of the other three cold-store root types, so
    /// the roots are exercised on random terms and not only on the constructed
    /// corpus.
    #[test]
    fn round_trip_through_every_root(par in generate_par(2)) {
        let a = ListParWithRandom { pars: vec![par.clone()], random_state: vec![1u8, 2] };
        prop_assert!(ListParWithRandom::cold_decode(
            &bincode::serialize(&a).expect("serialize")).expect("machine") == a);

        let p = BindPattern { patterns: vec![par.clone()], remainder: None, free_count: 3 };
        prop_assert!(BindPattern::cold_decode(
            &bincode::serialize(&p).expect("serialize")).expect("machine") == p);

        let k = TaggedContinuation {
            guard: Some(par),
            tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ScalaBodyRef(5)),
        };
        prop_assert!(TaggedContinuation::cold_decode(
            &bincode::serialize(&k).expect("serialize")).expect("machine") == k);
    }
}

// ===========================================================================
// The `EPathMap` fixtures
// ===========================================================================

/// `models/tests/epathmap_*` pin the wrapper's serde layout. Running the
/// differential over the same shapes ties the decoder to those pins: if the
/// wrapper's field list, its `#[serde(skip)]` cell or its `SharedPars`
/// transparency ever changes, this fails alongside them rather than after them.
#[test]
fn epathmap_shapes_agree() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EZipper, Expr};

    let shapes = vec![
        ("nonground", corpus::nonground_pathmap()),
        ("ground", corpus::ground_pathmap()),
        (
            "empty",
            models::rust::rhoapi_ext::EPathMap::new(Vec::new(), Vec::new(), false, None),
        ),
        (
            "locally_free_only",
            models::rust::rhoapi_ext::EPathMap::new(
                vec![corpus::gint(1)],
                vec![1, 2, 3],
                false,
                None,
            ),
        ),
    ];
    for (label, map) in shapes {
        // Through `EPathmapBody` …
        agree(
            &format!("epathmap::{label}::body"),
            &Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
                }],
                ..Default::default()
            },
        );
        // … and through `EZipper.pathmap`, whose `Option<EPathMap>` adds a tag
        // byte the direct arm does not have.
        agree(
            &format!("epathmap::{label}::zipper"),
            &Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                        pathmap: Some(map),
                        current_path: vec![vec![1], vec![]],
                        is_write_zipper: false,
                        locally_free: vec![9],
                        connective_used: true,
                        cursor_kind: 1,
                    })),
                }],
                ..Default::default()
            },
        );
    }

    // And the absent `Option<EPathMap>`.
    agree(
        "epathmap::absent::zipper",
        &Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: None,
                    current_path: vec![],
                    is_write_zipper: true,
                    locally_free: vec![],
                    connective_used: false,
                    cursor_kind: 0,
                })),
            }],
            ..Default::default()
        },
    );
}

/// A decoded `EPathMap` must carry an EMPTY shadow cell. The derived
/// `Deserialize` leaves `intern` at `OnceLock::default()` because the field is
/// `#[serde(skip)]`; a machine that filled it — or that reused a cell from a
/// sibling — would hand out a stale interned handle for a value it has never
/// interned, which the wrapper's cached-encode `debug_assert`s treat as a
/// defect.
#[test]
fn decoded_epathmaps_carry_no_intern_handle() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;

    let source = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(corpus::nonground_pathmap())),
        }],
        ..Default::default()
    };
    let bytes = bincode::serialize(&source).expect("serialize");
    let decoded = Par::cold_decode(&bytes).expect("machine");
    match decoded.exprs[0].expr_instance.as_ref() {
        Some(ExprInstance::EPathmapBody(map)) => assert!(
            map.shadow_cell_for_test().is_none(),
            "a decoded EPathMap must not carry an intern handle"
        ),
        other => panic!("expected EPathmapBody, got {:?}", other.is_some()),
    }
}
