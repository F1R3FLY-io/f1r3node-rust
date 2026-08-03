//! Differential for the production event-hash byte entry points.
//!
//! Datum, bind-pattern, and continuation bytes are built by the generated
//! explicit-worklist bincode encoder. This suite compares those production
//! entry points with the legacy derived-serde oracle over every reachable
//! container position, canonical PathMap permutations and duplicates, promoted
//! regressions, and bounded generated trees. Any mismatch changes consensus
//! event-hash preimages.

mod fixtures;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par, epathmap_remainder_connective,
    ezipper_value, gstring_par, nested_epathmap_value,
};
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, EList, EMap, EMatches, EMethod, EPathMap,
    EPlus, ESet, ETuple, EZipper, Expr, If, KeyValuePair, ListParWithRandom, Match, MatchCase, New,
    Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var,
};
use models::rust::event_hash_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::pathmap_crate_type_mapper::eval_stable_epathmap;
use proptest::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn sample_map() -> EPathMap { e6a_index_epathmap() }

fn datum(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: vec![0xAA; 32],
    }
}

fn assert_event_hash_eq_legacy_datum(label: &str, value: &ListParWithRandom) {
    let legacy = bincode::serialize(value).expect("legacy bincode");
    let actual = event_hash_bytes_list_par_with_random(value);
    assert_eq!(
        actual, legacy,
        "{label}: datum event-hash bytes must equal legacy bincode bytes"
    );
}

fn assert_event_hash_eq_legacy_pattern(label: &str, value: &BindPattern) {
    let legacy = bincode::serialize(value).expect("legacy bincode");
    let actual = event_hash_bytes_bind_pattern(value);
    assert_eq!(
        actual, legacy,
        "{label}: pattern event-hash bytes must equal legacy bincode bytes"
    );
}

fn assert_event_hash_eq_legacy_continuation(label: &str, value: &TaggedContinuation) {
    let legacy = bincode::serialize(value).expect("legacy bincode");
    let actual = event_hash_bytes_tagged_continuation(value);
    assert_eq!(
        actual, legacy,
        "{label}: continuation event-hash bytes must equal legacy bincode bytes"
    );
}

fn expr_par(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Deterministic shape coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn event_hash_bytes_match_legacy_bincode_for_every_container_position() {
    let map_par = || epathmap_par(sample_map());

    // Free-variable and wildcard vars for pattern shapes.
    let free_var = Var {
        var_instance: Some(VarInstance::FreeVar(0)),
    };

    let shapes: Vec<(&str, Par)> = vec![
        ("datum-root map", map_par()),
        (
            "locally-free entry map",
            epathmap_par(epathmap_locally_free_entries()),
        ),
        (
            "remainder/connective map",
            epathmap_par(epathmap_remainder_connective()),
        ),
        (
            "ezipper fixture",
            expr_par(ExprInstance::EZipperBody(ezipper_value())),
        ),
        (
            "elist element",
            expr_par(ExprInstance::EListBody(EList {
                ps: vec![gstring_par("head"), map_par()],
                locally_free: vec![1], // exercised: must serialize as EMPTY
                connective_used: false,
                remainder: Some(free_var),
            })),
        ),
        (
            "etuple element",
            expr_par(ExprInstance::ETupleBody(ETuple {
                ps: vec![map_par(), gstring_par("tail")],
                locally_free: Vec::new(),
                connective_used: true,
            })),
        ),
        (
            "eset element",
            expr_par(ExprInstance::ESetBody(ESet {
                ps: vec![map_par()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        ),
        (
            "emap value",
            expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(gstring_par("k")),
                    value: Some(map_par()),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        ),
        (
            "emethod target+argument",
            expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "readZipperAt".to_string(),
                target: Some(map_par()),
                arguments: vec![map_par(), gstring_par("arg")],
                locally_free: vec![2],
                connective_used: false,
            })),
        ),
        (
            "binary-op operand",
            expr_par(ExprInstance::EPlusBody(EPlus {
                p1: Some(map_par()),
                p2: Some(gstring_par("rhs")),
            })),
        ),
        (
            "ematches target",
            expr_par(ExprInstance::EMatchesBody(EMatches {
                target: Some(map_par()),
                pattern: Some(gstring_par("pat")),
            })),
        ),
        (
            "ezipper pathmap",
            expr_par(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(sample_map()),
                current_path: vec![vec![1, 2, 0xFF]],
                is_write_zipper: false,
                locally_free: vec![1],
                connective_used: false,
                // 0 = SPLIT, prost's default: the field is omitted from the
                // wire entirely, so this event-hash pin does not move.
                cursor_kind: 0,
            })),
        ),
        (
            "nested map",
            expr_par(ExprInstance::EPathmapBody(EPathMap::new(
                vec![map_par()],
                Vec::new(),
                false,
                None,
            ))),
        ),
        ("send data+chan", {
            let mut par = Par::default();
            par.sends = vec![Send {
                chan: Some(map_par()),
                data: vec![map_par(), gstring_par("d")],
                persistent: true,
                locally_free: vec![4],
                connective_used: false,
            }];
            par
        }),
        ("receive bind+body+condition", {
            let mut par = Par::default();
            par.receives = vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![map_par()],
                    source: Some(map_par()),
                    remainder: Some(free_var),
                    free_count: 1,
                }],
                body: Some(map_par()),
                persistent: false,
                peek: true,
                bind_count: 1,
                locally_free: vec![8],
                connective_used: false,
                condition: Some(map_par()),
            }];
            par
        }),
        ("new body+injection", {
            let mut par = Par::default();
            let mut injections = std::collections::BTreeMap::new();
            injections.insert("rho:injection".to_string(), map_par());
            par.news = vec![New {
                bind_count: 2,
                p: Some(map_par()),
                uri: vec!["rho:io:stdout".to_string()],
                injections,
                locally_free: vec![16],
            }];
            par
        }),
        ("match target+case+guard", {
            let mut par = Par::default();
            par.matches = vec![Match {
                target: Some(map_par()),
                cases: vec![MatchCase {
                    pattern: Some(gstring_par("p")),
                    source: Some(map_par()),
                    free_count: 0,
                    guard: Some(map_par()),
                }],
                locally_free: Vec::new(),
                connective_used: true,
            }];
            par
        }),
        ("if all three arms", {
            let mut par = Par::default();
            par.conditionals = vec![If {
                condition: Some(map_par()),
                if_true: Some(map_par()),
                if_false: Some(map_par()),
                locally_free: vec![32],
                connective_used: false,
            }];
            par
        }),
        ("bundle body", {
            let mut par = Par::default();
            par.bundles = vec![Bundle {
                body: Some(map_par()),
                write_flag: true,
                read_flag: false,
            }];
            par
        }),
        ("connective and/or/not", {
            let mut par = Par::default();
            par.connectives = vec![
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                        ps: vec![map_par()],
                    })),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(map_par())),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnBool(true)),
                },
            ];
            par.connective_used = true;
            par
        }),
    ];

    for (label, par) in shapes {
        assert_event_hash_eq_legacy_datum(label, &datum(vec![par.clone(), gstring_par("sibling")]));
        assert_event_hash_eq_legacy_pattern(label, &BindPattern {
            patterns: vec![par.clone()],
            remainder: Some(free_var),
            free_count: 1,
        });
        assert_event_hash_eq_legacy_continuation(label, &TaggedContinuation {
            guard: Some(par.clone()),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(par),
                random_state: vec![7; 16],
            })),
        });
    }
}

#[test]
fn nested_pathmaps_match_legacy_bincode() {
    let plain = datum(vec![epathmap_par(nested_epathmap_value())]);
    assert_event_hash_eq_legacy_datum("nested-pathmaps", &plain);
}

/// ScalaBodyRef continuations (no Par at all) plus a guarded one.
#[test]
fn scala_body_ref_matches_legacy_bincode() {
    assert_event_hash_eq_legacy_continuation("scala-body-ref", &TaggedContinuation {
        guard: Some(epathmap_par(sample_map())),
        tagged_cont: Some(TaggedCont::ScalaBodyRef(42)),
    });
    assert_event_hash_eq_legacy_continuation("none-cont", &TaggedContinuation {
        guard: None,
        tagged_cont: None,
    });
}

#[test]
fn cloned_pathmaps_have_identical_event_hash_bytes() {
    let original = sample_map();
    let clone = original.clone();
    let d1 = datum(vec![epathmap_par(original)]);
    let d2 = datum(vec![epathmap_par(clone)]);
    assert_event_hash_eq_legacy_datum("clone-original", &d1);
    assert_event_hash_eq_legacy_datum("clone", &d2);
    assert_eq!(
        event_hash_bytes_list_par_with_random(&d1),
        event_hash_bytes_list_par_with_random(&d2),
        "clone-family datums must hash to the same bytes"
    );
}

// Producer-independence regressions. PathMap construction absorbs duplicates
// and canonicalizes order, so equivalent set tries must have identical event-
// hash preimages regardless of producer insertion order.

/// A GROUND EPathMap of `GString` entries (permutable, reorderable): every
/// entry is `eval_stable`, so the map is `eval_stable` ⇒ the canonical-ps arm.
fn ground_string_map(labels: &[&str]) -> EPathMap {
    EPathMap::new(
        labels.iter().map(|l| gstring_par(l)).collect::<Vec<Par>>(),
        Vec::new(),
        false,
        None,
    )
}

#[test]
fn permuted_ground_maps_have_identical_event_hash_bytes() {
    let m1 = ground_string_map(&["a", "b", "c"]);
    let d1 = datum(vec![epathmap_par(m1)]);
    let h1 = event_hash_bytes_list_par_with_random(&d1);
    assert_event_hash_eq_legacy_datum("permuted-m1", &d1);

    let m2 = ground_string_map(&["c", "b", "a"]); // same SET, permuted order
    let d2 = datum(vec![epathmap_par(m2)]);

    assert_event_hash_eq_legacy_datum("permuted-m2", &d2);
    assert_eq!(
        h1,
        event_hash_bytes_list_par_with_random(&d2),
        "permuted ground maps must hash to identical event-hash bytes"
    );
}

#[test]
fn duplicate_entries_collapse_to_the_same_event_hash_bytes() {
    let with_dup = ground_string_map(&["a", "a", "b"]);
    let d_dup = datum(vec![epathmap_par(with_dup)]);
    assert_event_hash_eq_legacy_datum("dup-entries", &d_dup);

    let deduped = ground_string_map(&["a", "b"]);
    let d_dedup = datum(vec![epathmap_par(deduped)]);
    assert_event_hash_eq_legacy_datum("deduped-entries", &d_dedup);

    assert_eq!(
        event_hash_bytes_list_par_with_random(&d_dup),
        event_hash_bytes_list_par_with_random(&d_dedup),
        "[a,a,b] and [a,b] must hash to identical (deduped) event-hash bytes"
    );
}

#[test]
fn nested_ground_submap_permutations_have_identical_event_hash_bytes() {
    let inner_fwd = ground_string_map(&["x", "y", "z"]);
    let outer_fwd = EPathMap::new(vec![epathmap_par(inner_fwd)], Vec::new(), true, None);
    assert!(
        !eval_stable_epathmap(&outer_fwd),
        "outer must be NON-ground (connective_used) so it serializes as-is"
    );
    let d_fwd = datum(vec![epathmap_par(outer_fwd)]);
    assert_event_hash_eq_legacy_datum("nested-inner-fwd", &d_fwd);

    let inner_bwd = ground_string_map(&["z", "y", "x"]);
    let outer_bwd = EPathMap::new(vec![epathmap_par(inner_bwd)], Vec::new(), true, None);
    let d_bwd = datum(vec![epathmap_par(outer_bwd)]);
    assert_event_hash_eq_legacy_datum("nested-inner-bwd", &d_bwd);

    assert_eq!(
        event_hash_bytes_list_par_with_random(&d_fwd),
        event_hash_bytes_list_par_with_random(&d_bwd),
        "the inner ground submap canonicalizes regardless of the outer being non-ground"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Proptest over arbitrary bounded trees
// ─────────────────────────────────────────────────────────────────────────────

/// Leaf Pars: ground scalars and EPathMaps.
fn leaf_par_strategy() -> impl Strategy<Value = Par> {
    prop_oneof![
        any::<i64>().prop_map(|i| expr_par(ExprInstance::GInt(i))),
        "[a-z]{0,8}".prop_map(|s| expr_par(ExprInstance::GString(s))),
        any::<bool>().prop_map(|b| expr_par(ExprInstance::GBool(b))),
        proptest::collection::vec(any::<u8>(), 0..8)
            .prop_map(|b| expr_par(ExprInstance::GByteArray(b))),
        proptest::collection::vec("[a-z]{1,4}", 0..3).prop_map(|entries| {
            let map = EPathMap::new(
                entries
                    .into_iter()
                    .map(|s| gstring_par(&s))
                    .collect::<Vec<Par>>(),
                Vec::new(),
                false,
                None,
            );
            epathmap_par(map)
        }),
    ]
}

/// Bounded recursive Par trees over the emitter's container spine.
fn par_strategy() -> impl Strategy<Value = Par> {
    leaf_par_strategy().prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            // EList with an exercised locally_free (must blank in serde).
            (
                proptest::collection::vec(inner.clone(), 0..4),
                any::<bool>()
            )
                .prop_map(|(ps, used)| expr_par(ExprInstance::EListBody(EList {
                    ps,
                    locally_free: vec![1],
                    connective_used: used,
                    remainder: None,
                }))),
            (inner.clone(), inner.clone()).prop_map(|(p1, p2)| expr_par(ExprInstance::EPlusBody(
                EPlus {
                    p1: Some(p1),
                    p2: Some(p2),
                }
            ))),
            (
                inner.clone(),
                proptest::collection::vec(inner.clone(), 0..3)
            )
                .prop_map(|(target, arguments)| expr_par(ExprInstance::EMethodBody(
                    EMethod {
                        method_name: "m".to_string(),
                        target: Some(target),
                        arguments,
                        locally_free: Vec::new(),
                        connective_used: false,
                    }
                ))),
            (
                inner.clone(),
                proptest::collection::vec(inner.clone(), 0..3)
            )
                .prop_map(|(chan, data)| {
                    let mut par = Par::default();
                    par.sends = vec![Send {
                        chan: Some(chan),
                        data,
                        persistent: false,
                        locally_free: vec![2],
                        connective_used: false,
                    }];
                    par
                }),
            proptest::collection::vec(inner.clone(), 1..3).prop_map(|ps| {
                let map = EPathMap::new(ps, Vec::new(), false, None);
                epathmap_par(map)
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Production bytes equal legacy bincode for arbitrary trees on all three
    /// event-hash legs.
    #[test]
    fn event_hash_bytes_match_legacy_bincode_for_arbitrary_trees(
        pars in proptest::collection::vec(par_strategy(), 0..4),
        random_state in proptest::collection::vec(any::<u8>(), 0..48),
        free_count in 0..4i32,
    ) {
        production_gate(&pars, random_state, free_count);
    }
}

#[track_caller]
fn production_gate(pars: &[Par], random_state: Vec<u8>, free_count: i32) {
    let value = ListParWithRandom {
        pars: pars.to_vec(),
        random_state,
    };
    assert_eq!(
        event_hash_bytes_list_par_with_random(&value),
        bincode::serialize(&value).expect("direct"),
        "datum leg diverged"
    );

    let pattern = BindPattern {
        patterns: pars.to_vec(),
        remainder: None,
        free_count,
    };
    assert_eq!(
        event_hash_bytes_bind_pattern(&pattern),
        bincode::serialize(&pattern).expect("direct"),
        "pattern leg diverged"
    );

    let continuation = TaggedContinuation {
        guard: pars.first().cloned(),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: pars.last().cloned(),
            random_state: vec![3; 8],
        })),
    };
    assert_eq!(
        event_hash_bytes_tagged_continuation(&continuation),
        bincode::serialize(&continuation).expect("direct"),
        "continuation leg diverged"
    );
}

// ── the recorded counterexamples, PROMOTED ───────────────────────────────────────────
//
// The two entries of `models/tests/event_hash_bytes_differential.proptest-regressions`. Both
// are a single `Par` holding one `EPathmapBody`, and they differ in exactly one way that is
// the whole reason both are kept:
//
//   entry 0 — the map's `ps` holds `GBool(false)` TWICE;
//   entry 1 — the map's `ps` holds it ONCE.
//
// An `EPathMap`'s `ps` is an `EntryTrie`, so the duplicate is the case where the trie's own
// canonicalisation decides how many entries survive. The generated byte encoder and
// `bincode::serialize` must agree on the result — and they must agree about the COLLAPSE,
// not merely about a list. Entry 1 is the control that shows the encoders agree without a
// duplicate present, which is what makes entry 0's agreement meaningful rather than
// coincidental.

/// A `Par` whose sole expression is `GBool(false)` — the leaf both recorded entries use.
fn recorded_gbool_false_par() -> Par { expr_par(ExprInstance::GBool(false)) }

/// The recorded `pars` of an entry whose map holds `GBool(false)` `multiplicity` times.
fn recorded_epathmap_par(multiplicity: usize) -> Vec<Par> {
    vec![expr_par(ExprInstance::EPathmapBody(EPathMap::new(
        vec![recorded_gbool_false_par(); multiplicity],
        Vec::new(),
        false,
        None,
    )))]
}

/// ★ THE DISPOSITION: entry 0's recorded VALUE is no longer constructible, and the reason
/// is a deliberate design change.
///
/// The corpus records entry 0's `EPathMap.ps` holding `GBool(false)` TWICE. No input to the
/// current API produces that: `ps` is an `EntryTrie`, and `impl From<Vec<Par>> for EntryTrie`
/// says so in its own words — *"duplicates in the input are absorbed by the insert — the
/// trie is a function of the entry SET — so `EntryTrie::from(v).view()` is generally not
/// `v`, and that is the point rather than a defect."* The seed was recorded when `ps` was a
/// plain `Vec<Par>`.
///
/// So this is a Tier-2 FORMAT migration for a layout-A corpus, and unlike the `lex_weight`
/// case it is NOT losslessly invertible: there the absent fields had a documented default
/// that made the old rendering complete, while here the archived value is outside the
/// current type's image altogether. Pretending otherwise would mean writing a test that
/// asserts a `Debug` string the code cannot produce.
///
/// What is asserted instead is the migration itself, so it cannot drift unnoticed:
/// the corpus still says two, the trie still collapses to one, and the offered multiplicity
/// is what the promoted gate tests below feed in. If the trie ever stopped collapsing,
/// entry 0 would become reconstructible again and this test says so.
#[test]
fn the_recorded_epathmap_entries_migrated_from_a_vec_to_a_deduplicating_trie() {
    let corpus = include_str!("event_hash_bytes_differential.proptest-regressions");
    let recorded: Vec<(&str, &str)> = corpus
        .lines()
        .filter_map(|l| l.strip_prefix("cc ")?.split_once(" # shrinks to "))
        .collect();
    assert_eq!(
        recorded.len(),
        2,
        "the corpus no longer holds exactly its two entries"
    );
    assert_eq!(
        recorded[0].0, "11cdd43bc682c5868b68a4b70d85766bd99be3bfe9b19f8d90aa4b3215ef01fb",
        "the corpus entries are no longer the promoted ones"
    );
    assert_eq!(
        recorded[1].0, "9511857581c23949e5f8620affddb87bce7a8edba290c80d3f4a2647a73b0895",
        "the corpus entries are no longer the promoted ones"
    );

    // The archive still describes a map holding the SAME leaf twice.
    let leaf = format!("{:?}", recorded_gbool_false_par());
    assert_eq!(
        recorded[0].1.matches(&leaf).count(),
        2,
        "entry 0 no longer records the DUPLICATE that distinguishes it from entry 1"
    );
    assert_eq!(
        recorded[1].1.matches(&leaf).count(),
        1,
        "entry 1 no longer records the single-entry control"
    );

    // The current type collapses it, which is why entry 0 cannot be rebuilt as recorded.
    let two = EPathMap::new(vec![recorded_gbool_false_par(); 2], Vec::new(), false, None);
    let one = EPathMap::new(vec![recorded_gbool_false_par()], Vec::new(), false, None);
    assert_eq!(
        two.len(),
        1,
        "★ `EntryTrie` no longer absorbs the duplicate. Entry 0's recorded value has become \
         constructible again, so it should be promoted as an exact reconstruction instead \
         of through this migration record."
    );
    assert_eq!(one.len(), 1, "the single-entry control must be unaffected");

    // The single-entry control reconstructs semantically. The archived shrink
    // text used the former Vec debug shape, while the current debug output
    // intentionally exposes EntryTrie mode, length, and EPM1 bytes.
    let mut entries = Vec::new();
    one.entry_trie()
        .for_each_entry(|entry| entries.push(entry.clone()));
    assert_eq!(
        entries.len(),
        1,
        "the single-entry control must enumerate exactly one trie member"
    );
    assert_eq!(
        bincode::serialize(&entries[0]).expect("control entry must encode"),
        bincode::serialize(&recorded_gbool_false_par()).expect("expected entry must encode"),
        "the single-entry control no longer reconstructs the archived GBool(false) member"
    );
}

/// `cc 11cdd43bc682c5868b68a4b70d85766bd99be3bfe9b19f8d90aa4b3215ef01fb` — the DUPLICATE.
#[test]
fn the_recorded_duplicate_epathmap_entry_passes_the_production_gate() {
    production_gate(&recorded_epathmap_par(2), Vec::new(), 0);
}

/// `cc 9511857581c23949e5f8620affddb87bce7a8edba290c80d3f4a2647a73b0895` — the control.
#[test]
fn the_recorded_single_entry_epathmap_passes_the_production_gate() {
    production_gate(&recorded_epathmap_par(1), Vec::new(), 0);
}
