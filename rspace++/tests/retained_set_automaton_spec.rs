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
