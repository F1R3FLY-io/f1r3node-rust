// See models/src/test/scala/coop/rchain/models/rholang/SortTest.scala - ScoredTermSpec

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EMethod, Expr, New, Par, Var};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::sorter::bundle_sort_matcher::BundleSortMatcher;
use models::rust::rholang::sorter::connective_sort_matcher::ConnectiveSortMatcher;
use models::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use models::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
use models::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use models::rust::rholang::sorter::score_tree::{compare_score, ScoreAtom, ScoredTerm, Tree};
use models::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::rholang::sorter::var_sort_matcher::VarSortMatcher;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::test_utils::test_utils::{
    generate_bundle, generate_connective, generate_expr, generate_match, generate_new,
    generate_par, generate_receive, generate_send, generate_var,
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn global_proptest_config() -> ProptestConfig {
    ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    }
}

#[test]
fn scored_term_should_sort_so_that_shorter_nodes_come_first() {
    let mut unsorted_terms = vec![
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2, 2, 3]),
        },
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2, 2]),
        },
    ];

    let sorted_terms = vec![
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2, 2]),
        },
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2, 2, 3]),
        },
    ];

    ScoredTerm::sort_vec(&mut unsorted_terms);
    assert_eq!(unsorted_terms, sorted_terms);
}

#[test]
fn scored_term_should_sort_so_that_smaller_leaves_stay_first() {
    let mut unsorted_terms = vec![
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
        },
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(2),
        },
    ];

    let sorted_terms = vec![
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
        },
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(2),
        },
    ];

    ScoredTerm::sort_vec(&mut unsorted_terms);
    assert_eq!(unsorted_terms, sorted_terms);
}

#[test]
fn scored_term_should_sort_so_that_smaller_leaves_are_put_first() {
    let mut unsorted_terms = vec![
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(2),
        },
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
        },
    ];

    let sorted_terms = vec![
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
        },
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(2),
        },
    ];

    ScoredTerm::sort_vec(&mut unsorted_terms);
    assert_eq!(unsorted_terms, sorted_terms);
}

#[test]
fn scored_term_should_sort_so_that_smaller_nodes_are_put_first() {
    let mut unsorted_terms = vec![
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::Node(vec![
                Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2]),
                Tree::<ScoreAtom>::create_node_from_i64s(vec![2, 2]),
            ]),
        },
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::Node(vec![
                Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 1]),
                Tree::<ScoreAtom>::create_node_from_i64s(vec![2, 2]),
            ]),
        },
    ];

    let sorted_terms = vec![
        ScoredTerm {
            term: "bar".to_string(),
            score: Tree::Node(vec![
                Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 1]),
                Tree::<ScoreAtom>::create_node_from_i64s(vec![2, 2]),
            ]),
        },
        ScoredTerm {
            term: "foo".to_string(),
            score: Tree::Node(vec![
                Tree::<ScoreAtom>::create_node_from_i64s(vec![1, 2]),
                Tree::<ScoreAtom>::create_node_from_i64s(vec![2, 2]),
            ]),
        },
    ];

    ScoredTerm::sort_vec(&mut unsorted_terms);
    assert_eq!(unsorted_terms, sorted_terms);
}

// ===========================================================================
// ★ THE SORTER IS A NORMALIZER, AND A NORMALIZER IS NOT INJECTIVE
// ===========================================================================
//
// Three tests used to live here. Each asserted, in different words, the
// **iff**
//
//     sort_match(x).term == sort_match(y).term   ⟺   x == y
//
// and each was driven by five draws from `generate_*(2)`. The forward
// direction is true and is exactly what consensus needs. **The reverse
// direction is false**, and directly so:
//
//     Par { exprs: [GInt 1, GInt 2] }  ≠  Par { exprs: [GInt 2, GInt 1] }
//
// — `Par`'s derived `PartialEq` compares `exprs` as an *ordered* `Vec` — yet
// the two sort to **equal terms with equal scores**, because putting siblings
// into canonical order is the sorter's entire job. That is not a defect; it is
// the definition of a canonical form, and `reduce.rs` relies on it (two
// permuted `Par`s must COMM-match interchangeably).
//
// The old tests passed only because two independent draws are unlikely to be
// permutations of one another — and after Leg-2 Stage A fixed
// `generate_par`'s vacuity (`vec(…, 0..1)` meant *exactly zero elements,
// always*, so the sub-generators never ran at all) those draws finally
// populate collections, which makes a permutation collision reachable. A test
// that asserts a false property and survives on sampling luck cannot gate a
// change to the sorter, so the three are replaced here by the properties that
// are actually true, actually needed, and actually strong enough:
//
// | property | statement | why it matters |
// |---|---|---|
// | **functionality** | `x == y ⟹ sort(x) == sort(y)` | a validator must reach the same canonical form as the proposer |
// | **idempotence** | `sort(sort(x)) == sort(x)` | the canonical form is a fixed point, so re-normalising a stored term cannot move it |
// | **score/term agreement** | equal sorted terms ⟹ equal scores, and conversely | the score is what orders siblings; a disagreement would make the order depend on which of the two was compared |
// | **permutation collapse** | permuted siblings share one canonical form | the witness that the iff is false, pinned so it cannot be "restored" |
//
// See `docs/design/audits/theta-depth-traversals-2026-07-26.md` §11.3 (E24)
// for the measurement, and `models/tests/sorter_canonical_golden.rs` for the
// byte-level pin on the canonical form itself.

/// **Functionality.** Equal inputs must produce equal canonical forms *and*
/// equal scores. This is the direction consensus depends on: a validator
/// re-normalising the proposer's term must land on the same bytes.
#[test]
fn sorting_is_a_function_equal_terms_have_equal_canonical_forms_and_scores() {
    fn check<T, F>(generator: impl Strategy<Value = Vec<T>>, sort_fn: F)
    where
        T: Clone + PartialEq + std::fmt::Debug,
        F: Fn(&T) -> ScoredTerm<T>,
    {
        proptest!(global_proptest_config(), |(values in generator)| {
            for x in &values {
                for y in &values {
                    if x == y {
                        let sx = sort_fn(x);
                        let sy = sort_fn(y);
                        assert!(
                            sx.term == sy.term,
                            "EQUAL terms produced DIFFERENT canonical forms — sorting is not a \
                             function of its input, so a validator could not reproduce the \
                             proposer's bytes:\n  x = {:?}\n  y = {:?}",
                            x, y
                        );
                        assert!(
                            sx.score == sy.score,
                            "EQUAL terms produced DIFFERENT scores:\n  x = {:?}\n  y = {:?}",
                            x, y
                        );
                    }
                }
            }
        });
    }

    check(prop::collection::vec(generate_bundle(2), 5), |x| {
        BundleSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_connective(2), 5), |x| {
        ConnectiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_expr(2), 5), |x| {
        ExprSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_match(2), 5), |x| {
        MatchSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_new(2), 5), |x| {
        NewSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_par(2), 5), |x| {
        ParSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_receive(2), 5), |x| {
        ReceiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_send(2), 5), |x| {
        SendSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_var(2), 5), |x| {
        VarSortMatcher::sort_match(x)
    });
}

/// **Idempotence.** The canonical form is a *fixed point* of the sorter.
///
/// This is the property that makes "canonical" mean anything: a term read back
/// out of RSpace, or spliced into another term and re-sorted, must not move.
/// It is also the property that lets the `ESet` / `EMap` / `EPathMap` arms sort
/// their elements more than once (they do — see
/// `models/src/rust/rholang/sorter/sort_combine.rs`) without the extra rounds
/// changing anything.
#[test]
fn sorting_is_idempotent_the_canonical_form_is_a_fixed_point() {
    fn check<T, F>(generator: impl Strategy<Value = Vec<T>>, sort_fn: F)
    where
        T: Clone + PartialEq + std::fmt::Debug,
        F: Fn(&T) -> ScoredTerm<T>,
    {
        proptest!(global_proptest_config(), |(values in generator)| {
            for x in &values {
                let once = sort_fn(x);
                let twice = sort_fn(&once.term);
                assert!(
                    twice.term == once.term,
                    "SORTING IS NOT IDEMPOTENT — the canonical form is not a fixed point, so \
                     re-normalising a stored term would change the signed bytes:\n  \
                     input       = {:?}\n  sorted once = {:?}\n  sorted twice= {:?}",
                    x, once.term, twice.term
                );
                assert!(
                    twice.score == once.score,
                    "the SCORE is not idempotent, so a re-sorted term would order its \
                     siblings differently:\n  input = {:?}",
                    x
                );
            }
        });
    }

    check(prop::collection::vec(generate_bundle(2), 5), |x| {
        BundleSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_connective(2), 5), |x| {
        ConnectiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_expr(2), 5), |x| {
        ExprSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_match(2), 5), |x| {
        MatchSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_new(2), 5), |x| {
        NewSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_par(2), 5), |x| {
        ParSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_receive(2), 5), |x| {
        ReceiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_send(2), 5), |x| {
        SendSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_var(2), 5), |x| {
        VarSortMatcher::sort_match(x)
    });
}

/// ★★★ **THE PINNED WITNESS — distinct canonical terms CAN share a score, and the consequence
/// is a live consensus fault.**
///
/// This is the counterexample [`equal_canonical_terms_carry_equal_scores`] was weakened for. It
/// is pinned in the shape [`the_sorter_is_a_normalizer_and_is_therefore_not_injective`] uses:
/// a hand-built witness, asserted directly, so no generator has to be lucky.
///
/// # Why the score cannot separate these
///
/// `combine_emap` chains **only** `sorted_key.score` (`sort_combine.rs:1442-1450`); nothing in
/// an `EMap`'s score tree depends on the values. So `{3 → 30}` and `{3 → 90}` are distinct
/// canonical terms whose score trees are identical — both `(999 (9 -1 (999 (2 3) 0) 0) 0)`, in
/// which the key `3` appears and the values appear nowhere.
///
/// # The consequence, and it is the part that matters
///
/// `ScoredTerm::sort_vec` is a **stable** sort, so tied siblings keep their input order. Two
/// independent things then feed that input order, and both are defects:
///
/// | site | input order | consequence |
/// |---|---|---|
/// | `SortedParHashSet::create_from_vec` | `HashSet<Par>` iteration — seeded **per process** | canonical form is a **coin flip**; measured 20/20 across 40 processes |
/// | `combine_par` (`sort_combine.rs:447-455`) | the message's own field order | ⛔ **deterministic**, and two spellings of the SAME process sign differently |
///
/// ⇒ The second is the worse one. `|` is commutative, so `{3:30} | {3:90}` and
/// `{3:90} | {3:30}` denote one process; they must reach one canonical form. Measured, they do
/// not — and unlike the hash case, these are bytes that **are defined today**.
/// `permutation_collapse_survives_nesting` already asserts the property this violates.
///
/// ⚠ **These assertions are written in their CURRENT-STATE polarity and are expected to flip.**
/// The `assert_ne!` below is the statement of a defect, not of a desired property. When the
/// sibling order is made total, it becomes `assert_eq!` in the same commit, and that diff is
/// the fix's RED-to-GREEN pair. ★ Do not "fix" this test by deleting it.
#[test]
fn distinct_canonical_terms_can_share_a_score_and_that_is_a_consensus_fault() {
    use models::rhoapi::{EMap, KeyValuePair};
    use prost::Message;

    fn gint(v: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }])
    }
    fn map1(key: i64, value: i64) -> Expr {
        Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(gint(key)),
                    value: Some(gint(value)),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }
    }

    let a = Par::default().with_exprs(vec![map1(3, 30)]);
    let b = Par::default().with_exprs(vec![map1(3, 90)]);
    let sa = ParSortMatcher::sort_match(&a);
    let sb = ParSortMatcher::sort_match(&b);

    assert_ne!(
        sa.term, sb.term,
        "VACUOUS: {{3 → 30}} and {{3 → 90}} reached the SAME canonical term, so this witness \
         no longer witnesses anything. Rebuild it from a pair that really is distinct."
    );
    assert_eq!(
        sa.score, sb.score,
        "the score now SEPARATES {{3 → 30}} from {{3 → 90}}. If that is deliberate — the value's \
         score is no longer discarded — then this witness is discharged and the `iff` that \
         `equal_canonical_terms_carry_equal_scores` used to assert may be recoverable. Re-derive \
         it rather than deleting this test: the other lossy score paths (`EZipper`'s cursor, \
         `ReceiveBind.free_count`) may still admit ties."
    );

    // ── The deterministic fork. No `HashSet`, no seed, one process. ──
    let ab = Par::default().with_exprs(vec![map1(3, 30), map1(3, 90)]);
    let ba = Par::default().with_exprs(vec![map1(3, 90), map1(3, 30)]);
    let cab = ParSortMatcher::sort_match(&ab).term.encode_to_vec();
    let cba = ParSortMatcher::sort_match(&ba).term.encode_to_vec();

    assert_ne!(
        cab, cba,
        "★ THE DEFECT IS REPAIRED — `{{3:30}} | {{3:90}}` and `{{3:90}} | {{3:30}}` now reach \
         the SAME canonical form.\n\n\
         This assertion is written in its CURRENT-STATE polarity: it asserts the FAULT, because \
         the fault is what is true at the commit that pins it. Flipping it to `assert_eq!` is \
         the repair's deliverable, and that flip belongs in the SAME commit as the fix so the \
         diff carries its own RED-to-GREEN evidence.\n\n\
         Do not delete this test to make the suite green."
    );
}

/// **Score/term agreement — the FORWARD direction only.** Equal canonical terms carry equal
/// scores.
///
/// ⛔★★★ **This test used to assert an `iff`, and the reverse direction is FALSE.** It was
/// weakened to what is true, and the counterexample is pinned in
/// [`distinct_canonical_terms_can_share_a_score_and_that_is_a_consensus_fault`] below rather
/// than left for the next reader to rediscover.
///
/// The `iff` passed only because the generators never drew the witness — the identical failure
/// shape the stack-safety report's §5.7.3 documents for the three tests **this one replaced**.
/// ⇒ A property test whose corpus cannot express the falsifying input is not evidence for the
/// property; it is evidence about the corpus.
///
/// ★ What survives, and it is the direction `sort_vec` actually needs for its `Equal` case:
/// siblings with the same canonical term must compare `Equal`. What does **not** survive is the
/// claim the old docstring made — *"two with different canonical terms must not silently share
/// a score class in a way that makes their order depend on which one the comparator saw
/// first"*. They can, they do, and the consequence is measured in the test below.
#[test]
fn equal_canonical_terms_carry_equal_scores() {
    fn check<T, F>(generator: impl Strategy<Value = Vec<T>>, sort_fn: F)
    where
        T: Clone + PartialEq + std::fmt::Debug,
        F: Fn(&T) -> ScoredTerm<T>,
    {
        proptest!(global_proptest_config(), |(values in generator)| {
            for x in &values {
                for y in &values {
                    let sx = sort_fn(x);
                    let sy = sort_fn(y);
                    // ⚠ FORWARD DIRECTION ONLY. The reverse (`equal scores => equal terms`)
                    // is FALSE — see this test's doc comment and the pinned witness below.
                    // Asserting the `iff` here is what let the fault stay invisible.
                    if sx.term == sy.term {
                        assert_eq!(
                            sx.score, sy.score,
                            "two inputs reached the SAME canonical term but DIFFERENT scores. \
                             Siblings are ordered by score, so equal terms that disagree on \
                             score would order nondeterministically:\n  \
                             x = {:?}\n  y = {:?}\n  sorted = {:?}",
                            x, y, sx.term
                        );
                    }
                }
            }
        });
    }

    check(prop::collection::vec(generate_bundle(2), 5), |x| {
        BundleSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_connective(2), 5), |x| {
        ConnectiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_expr(2), 5), |x| {
        ExprSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_match(2), 5), |x| {
        MatchSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_new(2), 5), |x| {
        NewSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_par(2), 5), |x| {
        ParSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_receive(2), 5), |x| {
        ReceiveSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_send(2), 5), |x| {
        SendSortMatcher::sort_match(x)
    });
    check(prop::collection::vec(generate_var(2), 5), |x| {
        VarSortMatcher::sort_match(x)
    });
}

/// ⚠ **The witness.** The measured counterexample to the deleted iff, pinned
/// so that nobody re-derives the false property from the (true) forward
/// direction and re-adds it.
///
/// Collapsing permuted siblings onto one canonical form is what the sorter is
/// *for*: it is why `@x!(1 | 2)` and `@x!(2 | 1)` are the same process, and
/// why two structurally-permuted `Par`s COMM-match interchangeably.
#[test]
fn the_sorter_is_a_normalizer_and_is_therefore_not_injective() {
    let ascending = Par {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(2)),
            },
        ],
        ..Default::default()
    };
    let descending = Par {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(2)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            },
        ],
        ..Default::default()
    };

    assert_ne!(
        ascending, descending,
        "`Par`'s derived PartialEq compares `exprs` as an ORDERED Vec, so these two must be \
         unequal — the whole point of the witness"
    );

    let a = ParSortMatcher::sort_match(&ascending);
    let d = ParSortMatcher::sort_match(&descending);

    assert!(
        a.term == d.term,
        "two PERMUTATIONS of the same Par must share one canonical form — that is what \
         normalisation means, and `reduce.rs` relies on it for COMM matching"
    );
    assert!(
        a.score == d.score,
        "two permutations that share a canonical term must share its score"
    );

    // The same statement at the level the sorter's own comparator sees: the
    // comparator returns `Equal` for these, which is exactly why `sort_vec`
    // must stay a STABLE sort (`sort_by`, never `sort_unstable_by`).
    assert_eq!(
        compare_score(&a.score, &d.score),
        std::cmp::Ordering::Equal,
        "the comparator must rank two permutations of one term as Equal"
    );
}

/// The same collapse, one level deeper: permuting the elements of a nested
/// `Par` must not change the enclosing canonical form either.
#[test]
fn permutation_collapse_survives_nesting() {
    fn nested(inner: Par) -> Par {
        Par {
            sends: vec![models::rhoapi::Send {
                chan: Some(Par::default()),
                data: vec![inner],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        }
    }
    let gi = |v: i64| Expr {
        expr_instance: Some(ExprInstance::GInt(v)),
    };
    let ascending = nested(Par {
        exprs: vec![gi(1), gi(2), gi(3)],
        ..Default::default()
    });
    let shuffled = nested(Par {
        exprs: vec![gi(3), gi(1), gi(2)],
        ..Default::default()
    });

    assert_ne!(ascending, shuffled);
    let a = ParSortMatcher::sort_match(&ascending);
    let s = ParSortMatcher::sort_match(&shuffled);
    assert!(
        a.term == s.term && a.score == s.score,
        "a permutation NESTED inside a Send did not collapse to one canonical form"
    );
}

#[test]
fn scored_term_should_score_in_different_byte_string_should_be_unequal() {
    let a = Expr {
        expr_instance: Some(ExprInstance::GByteArray(hex::decode("80").unwrap())),
    };

    let b = Expr {
        expr_instance: Some(ExprInstance::GByteArray(hex::decode("D9").unwrap())),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&a).score,
        ExprSortMatcher::sort_match(&b).score,
        "Scores for different ByteStrings should be unequal"
    );
}

#[test]
fn scored_term_should_sort_so_that_unequal_new_have_unequal_scores() {
    // based on new_sort_matcher.rs -> p field on New should be Some
    let new_1 = New {
        p: Some(Par::default()),
        bind_count: 1,
        injections: {
            let mut injections = std::collections::BTreeMap::new();
            injections.insert("key".to_string(), Par {
                bundles: vec![],
                sends: vec![],
                receives: vec![],
                ..Default::default()
            });
            injections
        },
        ..Default::default()
    };

    let new_2 = New {
        p: Some(Par::default()),
        bind_count: 1,
        ..Default::default()
    };
    assert_ne!(new_1, new_2);

    assert_ne!(
        NewSortMatcher::sort_match(&new_1).score,
        NewSortMatcher::sort_match(&new_2).score
    );
}

#[test]
fn scored_term_should_sort_so_that_unequal_emethod_have_unequal_scores() {
    // based on expr_sort_matcher.rs -> target field on EMethod should be Some
    let method_1 = Expr {
        expr_instance: Some(ExprInstance::EMethodBody(EMethod {
            target: Some(Par::default()),
            connective_used: true,
            ..Default::default()
        })),
    };

    let method_2 = Expr {
        expr_instance: Some(ExprInstance::EMethodBody(EMethod {
            target: Some(Par::default()),
            connective_used: false,
            ..Default::default()
        })),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&method_1).score,
        ExprSortMatcher::sort_match(&method_2).score
    );
}

#[test]
fn scored_term_should_sort_so_that_unequal_par_map_have_unequal_scores() {
    let map_1 = Expr {
        expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::new(vec![], true, vec![], None),
        ))),
    };

    let map_2 = Expr {
        expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::new(vec![], false, vec![], None),
        ))),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&map_1).score,
        ExprSortMatcher::sort_match(&map_2).score
    );

    let map_3 = Expr {
        expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::new(vec![], true, vec![], None),
        ))),
    };

    let map_4 = Expr {
        expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::new(vec![], true, vec![], Some(Var::default())),
        ))),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&map_3).score,
        ExprSortMatcher::sort_match(&map_4).score
    );
}

#[test]
fn scored_term_should_sort_so_that_unequal_par_set_have_unequal_scores() {
    let set_1 = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(
            ParSet {
                ps: SortedParHashSet::create_from_empty(),
                connective_used: true,
                locally_free: vec![],
                remainder: None,
            },
        ))),
    };

    let set_2 = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(
            ParSet {
                ps: SortedParHashSet::create_from_empty(),
                connective_used: false,
                locally_free: vec![],
                remainder: None,
            },
        ))),
    };

    assert!(ExprSortMatcher::sort_match(&set_1).score != ExprSortMatcher::sort_match(&set_2).score);

    let set_3 = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(
            ParSet {
                ps: SortedParHashSet::create_from_empty(),
                connective_used: true,
                locally_free: vec![],
                remainder: None,
            },
        ))),
    };

    let set_4 = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(
            ParSet {
                ps: SortedParHashSet::create_from_empty(),
                connective_used: true,
                locally_free: vec![],
                remainder: Some(Var::default()),
            },
        ))),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&set_3).score,
        ExprSortMatcher::sort_match(&set_4).score
    );
}

#[test]
fn scored_term_should_sort_so_that_unequal_list_have_unequal_scores() {
    let list_1 = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![],
            locally_free: vec![],
            connective_used: true,
            remainder: None,
        })),
    };

    let list_2 = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![],
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        })),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&list_1).score,
        ExprSortMatcher::sort_match(&list_2).score
    );

    let list_3 = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![],
            locally_free: vec![],
            connective_used: true,
            remainder: None,
        })),
    };

    let list_4 = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![],
            locally_free: vec![],
            connective_used: true,
            remainder: Some(Var::default()),
        })),
    };

    assert_ne!(
        ExprSortMatcher::sort_match(&list_3).score,
        ExprSortMatcher::sort_match(&list_4).score
    );
}
