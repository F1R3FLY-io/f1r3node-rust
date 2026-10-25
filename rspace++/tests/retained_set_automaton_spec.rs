use std::sync::Arc;

use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::RetainedSetAutomaton;
use rspace_plus_plus::rspace::rho_path::{
    RhoPathQuery, RhoPathQueryAtom, RhoPathV1, encode_structural_sequence,
};

#[derive(Clone, Default)]
enum Pattern {
    #[default]
    Any,
    Exact(Vec<u8>),
    SequenceFirst {
        expected: Vec<u8>,
        first: Vec<u8>,
    },
    SequenceSecond {
        expected: Vec<u8>,
        second: Vec<u8>,
    },
}

struct Matcher;

impl Match<Pattern, Vec<u8>, ()> for Matcher {
    fn get(&self, pattern: &Pattern, datum: &Vec<u8>) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => Some(datum.clone()),
            Pattern::Exact(expected) if expected == datum => Some(datum.clone()),
            Pattern::SequenceFirst { expected, .. } | Pattern::SequenceSecond { expected, .. }
                if expected == datum =>
            {
                Some(datum.clone())
            }
            Pattern::Exact(_) | Pattern::SequenceFirst { .. } | Pattern::SequenceSecond { .. } => {
                None
            }
        }
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => None,
            Pattern::Exact(path) => Some(path.clone()),
            Pattern::SequenceFirst { .. } | Pattern::SequenceSecond { .. } => None,
        }
    }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }

    fn rho_path_query(&self, pattern: &Pattern) -> Option<RhoPathQuery> {
        match pattern {
            Pattern::SequenceFirst { first, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Exact(first.clone()), RhoPathQueryAtom::Wildcard],
                false,
            )),
            Pattern::SequenceSecond { second, .. } => Some(RhoPathQuery::structural_sequence(
                [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(second.clone())],
                false,
            )),
            Pattern::Any | Pattern::Exact(_) => None,
        }
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }
}

fn waiting(patterns: Vec<Pattern>) -> WaitingContinuation<Pattern, ()> {
    WaitingContinuation {
        patterns: Arc::new(patterns),
        ..WaitingContinuation::default()
    }
}

#[test]
fn exact_terminals_and_root_fallbacks_are_selected_by_bind_position() {
    let candidates = vec![
        waiting(vec![Pattern::Exact(b"alpha".to_vec())]),
        waiting(vec![Pattern::Exact(b"beta".to_vec())]),
        waiting(vec![Pattern::Any]),
        waiting(vec![Pattern::Exact(b"alpha".to_vec()), Pattern::Exact(b"omega".to_vec())]),
    ];
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);

    assert_eq!(automaton.candidate_count(), candidates.len());
    assert_eq!(automaton.matching_candidates(b"alpha", &[0]), vec![true, false, true, true]);
    assert_eq!(automaton.matching_candidates(b"omega", &[1]), vec![false, false, false, true]);
}

#[test]
fn automaton_selection_is_a_superset_of_direct_matches() {
    let candidates = vec![
        waiting(vec![Pattern::Exact(b"a".to_vec())]),
        waiting(vec![Pattern::Exact(b"ab".to_vec())]),
        waiting(vec![Pattern::Any]),
        waiting(vec![Pattern::Exact(b"z".to_vec()), Pattern::Any]),
    ];
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    for datum in [b"a".as_slice(), b"ab", b"abc", b"z", b"other"] {
        for positions in [vec![0], vec![1], vec![0, 1]] {
            let selected = automaton.matching_candidates(datum, &positions);
            for (candidate_index, candidate) in candidates.iter().enumerate() {
                let directly_matches = positions.iter().any(|position| {
                    candidate
                        .patterns
                        .get(*position)
                        .is_some_and(|pattern| Matcher.get(pattern, &datum.to_vec()).is_some())
                });
                assert!(
                    !directly_matches || selected[candidate_index],
                    "candidate {candidate_index} was hidden for {datum:?} at {positions:?}"
                );
            }
        }
    }
}

#[test]
fn every_candidate_in_a_large_shared_terminal_chain_is_selected() {
    let candidates = (0..4_096)
        .map(|_| waiting(vec![Pattern::Any]))
        .collect::<Vec<_>>();
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    let selected = automaton.matching_candidates(b"datum", &[0]);

    assert_eq!(selected.len(), candidates.len());
    assert!(selected.into_iter().all(|candidate| candidate));
}

#[test]
fn structural_queries_use_exact_trie_prefixes_and_filter_after_wildcards() {
    let first_patterns = (0..256)
        .map(|index| {
            let first = format!("record/{index:08x}").into_bytes();
            let expected = encode_structural_sequence([first.as_slice(), b"tail"]);
            waiting(vec![Pattern::SequenceFirst { expected, first }])
        })
        .collect::<Vec<_>>();
    let second_patterns = (0..256)
        .map(|index| {
            let second = format!("class/{index:08x}").into_bytes();
            let expected = encode_structural_sequence([b"head", second.as_slice()]);
            waiting(vec![Pattern::SequenceSecond { expected, second }])
        })
        .collect::<Vec<_>>();
    let mut candidates = first_patterns;
    candidates.extend(second_patterns);
    candidates.push(waiting(vec![Pattern::Any]));
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);

    let target_first =
        encode_structural_sequence([b"record/000000a5".as_slice(), b"tail".as_slice()]);
    let selected_first = automaton.matching_candidates(&target_first, &[0]);
    assert_eq!(selected_first.iter().filter(|selected| **selected).count(), 2);
    assert!(selected_first[0xA5]);
    assert!(selected_first[candidates.len() - 1]);

    let target_second =
        encode_structural_sequence([b"head".as_slice(), b"class/0000005a".as_slice()]);
    let selected_second = automaton.matching_candidates(&target_second, &[0]);
    assert_eq!(selected_second.iter().filter(|selected| **selected).count(), 2);
    assert!(selected_second[256 + 0x5A]);
    assert!(selected_second[candidates.len() - 1]);
}

#[test]
fn retained_segments_product_walk_the_authoritative_resting_bag() {
    let candidates = vec![
        waiting(vec![Pattern::Exact(b"resting/alpha".to_vec())]),
        waiting(vec![Pattern::Exact(b"resting/beta".to_vec())]),
        waiting(vec![Pattern::Any]),
    ];
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    let mut resting = Bag::new();
    resting
        .insert(&RhoPathV1::relative_datum_key(b"resting/alpha", b"identity/alpha"))
        .expect("resting multiplicity");

    let selected =
        automaton.matching_candidates_with_resting(b"unrelated/fresh", &[0], Some(&resting));

    assert_eq!(selected, vec![true, false, true]);
}

#[test]
fn compressed_product_runs_visit_prefix_terminals_and_bag_branches() {
    let alpha = b"shared\0branch/alpha".to_vec();
    let alpha_deeper = b"shared\0branch/alpha/deeper".to_vec();
    let beta = b"shared\0branch/beta".to_vec();
    let candidates = vec![
        waiting(vec![Pattern::Exact(alpha.clone())]),
        waiting(vec![Pattern::Exact(alpha_deeper.clone())]),
        waiting(vec![Pattern::Exact(beta.clone())]),
        waiting(vec![Pattern::Exact(b"shared\0branch/absent".to_vec())]),
        waiting(vec![Pattern::Any]),
    ];
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    let mut resting = Bag::new();
    resting
        .insert(&RhoPathV1::relative_datum_key(&alpha_deeper, b"identity/alpha"))
        .expect("alpha occurrence");
    resting
        .insert(&RhoPathV1::relative_datum_key(&beta, b"identity/beta"))
        .expect("beta occurrence");

    let selected =
        automaton.matching_candidates_with_resting(b"unrelated/fresh", &[0], Some(&resting));

    assert_eq!(selected, vec![true, true, true, false, true]);
}

#[test]
fn generated_resting_bag_product_preserves_direct_matches_and_first_order() {
    let strategy = (
        proptest::collection::vec(
            proptest::collection::vec(proptest::prelude::any::<u8>(), 0..24),
            0..16,
        ),
        proptest::collection::vec(
            proptest::collection::vec(proptest::prelude::any::<u8>(), 0..24),
            0..12,
        ),
        proptest::collection::vec(proptest::prelude::any::<u8>(), 0..24),
        proptest::prelude::any::<bool>(),
        0usize..32,
    );
    let config = proptest::test_runner::Config {
        cases: 96,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    };
    let mut runner = proptest::test_runner::TestRunner::new(config);
    runner
        .run(
            &strategy,
            |(patterns, mut resting_values, fresh, include_fallback, insertion_seed)| {
                let mut candidates = patterns
                    .iter()
                    .cloned()
                    .map(|pattern| waiting(vec![Pattern::Exact(pattern)]))
                    .collect::<Vec<_>>();
                let mut resting_only = fresh.clone();
                resting_only.push(0xff);
                resting_values.push(resting_only.clone());
                let fresh_position = insertion_seed % (candidates.len() + 1);
                candidates.insert(fresh_position, waiting(vec![Pattern::Exact(fresh.clone())]));
                let resting_position = (insertion_seed / 2) % (candidates.len() + 1);
                candidates.insert(resting_position, waiting(vec![Pattern::Exact(resting_only)]));
                if include_fallback {
                    candidates.push(waiting(vec![Pattern::Any]));
                }
                let longest_datum = resting_values
                    .iter()
                    .map(Vec::len)
                    .chain(std::iter::once(fresh.len()))
                    .max()
                    .expect("the generated corpus has a fresh datum");
                let negative_index = candidates.len();
                candidates.push(waiting(vec![Pattern::Exact(vec![0xa5; longest_datum + 1])]));
                let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
                let mut resting = Bag::new();
                for (index, datum) in resting_values.iter().enumerate() {
                    resting
                        .insert(&RhoPathV1::relative_datum_key(datum, &index.to_be_bytes()))
                        .expect("generated resting occurrence");
                }
                let selected =
                    automaton.matching_candidates_with_resting(&fresh, &[0], Some(&resting));
                let direct = candidates
                    .iter()
                    .map(|candidate| {
                        let pattern = &candidate.patterns[0];
                        Matcher.get(pattern, &fresh).is_some() ||
                            resting_values
                                .iter()
                                .any(|datum| Matcher.get(pattern, datum).is_some())
                    })
                    .collect::<Vec<_>>();

                proptest::prop_assert_eq!(selected.len(), direct.len());
                proptest::prop_assert!(direct.iter().filter(|accepted| **accepted).count() >= 2);
                proptest::prop_assert!(!selected[negative_index]);
                for (index, accepted) in direct.iter().enumerate() {
                    proptest::prop_assert!(!*accepted || selected[index]);
                }
                let first_direct = direct.iter().position(|accepted| *accepted);
                let first_selected_direct = direct
                    .iter()
                    .enumerate()
                    .find_map(|(index, accepted)| (selected[index] && *accepted).then_some(index));
                proptest::prop_assert_eq!(first_selected_direct, first_direct);
                proptest::prop_assert!(
                    selected.iter().any(|retained| *retained) || first_direct.is_none()
                );
                Ok(())
            },
        )
        .expect("generated fresh/resting Bag selection preserves spatial matches");
}

#[test]
fn singleton_bag_path_selects_a_terminal_below_diverse_and_shared_branches() {
    for shared_prefix in [false, true] {
        for cardinality in [16usize, 256, 4_096] {
            let values = (0usize..cardinality)
                .map(|index| {
                    if shared_prefix {
                        format!("resting/shared-prefix/record-{index:08x}").into_bytes()
                    } else {
                        format!("{:02x}/{:08x}", index % 251, index.wrapping_mul(0x9E37_79B1))
                            .into_bytes()
                    }
                })
                .collect::<Vec<_>>();
            let target = values.last().expect("nonempty corpus").clone();
            let candidates = values
                .into_iter()
                .map(|value| waiting(vec![Pattern::Exact(value)]))
                .collect::<Vec<_>>();
            let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
            let mut resting = Bag::new();
            resting
                .insert(&RhoPathV1::relative_datum_key(&target, b"stable/resting-identity"))
                .expect("target occurrence");

            let selected = automaton.matching_candidates_with_resting(
                b"fresh/does-not-match-any-retained-pattern",
                &[0],
                Some(&resting),
            );

            assert_eq!(selected.iter().filter(|selected| **selected).count(), 1);
            assert!(selected[candidates.len() - 1]);
        }
    }
}

#[test]
fn singleton_bag_with_multiple_terminal_depths_uses_the_linear_product() {
    let target = b"shared/prefix/deeper".to_vec();
    let candidates = vec![
        waiting(vec![Pattern::Exact(b"shared".to_vec())]),
        waiting(vec![Pattern::Exact(b"shared/prefix".to_vec())]),
        waiting(vec![Pattern::Exact(target.clone())]),
        waiting(vec![Pattern::Exact(b"shared/absent".to_vec())]),
    ];
    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    let mut resting = Bag::new();
    resting
        .insert(&RhoPathV1::relative_datum_key(&target, b"stable/resting-identity"))
        .expect("target occurrence");

    let selected = automaton.matching_candidates_with_resting(
        b"fresh/does-not-match-any-retained-pattern",
        &[0],
        Some(&resting),
    );

    // Prefix terminals remain a conservative superset; the authoritative
    // matcher rejects the two shorter exact patterns during CandidateScan.
    assert_eq!(selected, vec![true, true, true, false]);
}

#[test]
fn representation_stats_expose_eligibility_and_terminal_sharing() {
    let repeated = Pattern::SequenceFirst {
        expected: encode_structural_sequence([b"record".as_slice(), b"tail".as_slice()]),
        first: b"record".to_vec(),
    };
    let candidates = vec![
        waiting(vec![repeated.clone()]),
        waiting(vec![repeated]),
        waiting(vec![Pattern::SequenceSecond {
            expected: encode_structural_sequence([b"head".as_slice(), b"class".as_slice()]),
            second: b"class".to_vec(),
        }]),
        waiting(vec![Pattern::Exact(b"alpha".to_vec())]),
        waiting(vec![Pattern::Any]),
    ];

    let automaton = RetainedSetAutomaton::compile(&Matcher, &candidates);
    let stats = automaton.stats();

    assert_eq!(stats.candidate_count, 5);
    assert_eq!(stats.indexed_candidate_count, 4);
    assert_eq!(stats.pattern_count, 5);
    assert_eq!(stats.indexed_pattern_count, 4);
    assert_eq!(stats.fallback_pattern_count, 1);
    assert_eq!(stats.terminal_count, 5);
    assert_eq!(stats.distinct_terminal_path_count, 4);
    assert_eq!(stats.shared_terminal_count, 1);
    assert_eq!(stats.live_terminal_count, stats.terminal_count);
    assert_eq!(stats.dead_terminal_count, 0);
    assert!(stats.maximum_terminal_depth > 0);
}

#[test]
fn immutable_rebuilds_never_accumulate_dead_terminals() {
    let corpus = (0usize..512)
        .map(|index| waiting(vec![Pattern::Exact(format!("record/{index:08x}").into_bytes())]))
        .collect::<Vec<_>>();

    for live in [1usize, 16, 128, 256, 511, 512] {
        let automaton = RetainedSetAutomaton::compile(&Matcher, &corpus[..live]);
        let stats = automaton.stats();
        assert_eq!(stats.candidate_count, live);
        assert_eq!(stats.terminal_count, live);
        assert_eq!(stats.live_terminal_count, live);
        assert_eq!(stats.dead_terminal_count, 0);
    }
}

#[test]
fn deep_and_wide_automata_compile_select_and_drop_on_a_tiny_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let deep = vec![0x5a; 1_000_000];
            let deep_candidates = vec![waiting(vec![Pattern::Exact(deep.clone())])];
            let deep_automaton = RetainedSetAutomaton::compile(&Matcher, &deep_candidates);
            assert_eq!(deep_automaton.matching_candidates(&deep, &[0]), vec![true]);
            assert!(deep_automaton.stats().maximum_terminal_depth >= deep.len());
            drop(deep_automaton);

            let wide_candidates = (0..20_000)
                .map(|_| waiting(vec![Pattern::Any]))
                .collect::<Vec<_>>();
            let wide_automaton = RetainedSetAutomaton::compile(&Matcher, &wide_candidates);
            let selected = wide_automaton.matching_candidates(b"datum", &[0]);
            assert_eq!(selected.len(), wide_candidates.len());
            assert!(selected.into_iter().all(|candidate| candidate));
            assert_eq!(wide_automaton.stats().distinct_terminal_path_count, 1);
            assert_eq!(wide_automaton.stats().shared_terminal_count, 19_999);
            drop(wide_automaton);
        })
        .expect("tiny-stack automaton worker")
        .join()
        .expect("deep/wide automaton operations remain stack safe");
}
