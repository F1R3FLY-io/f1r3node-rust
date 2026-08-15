use std::sync::Arc;

use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::retained_set_automaton::RetainedSetAutomaton;

#[derive(Clone, Default)]
enum Pattern {
    #[default]
    Any,
    Exact(Vec<u8>),
}

struct Matcher;

impl Match<Pattern, Vec<u8>, ()> for Matcher {
    fn get(&self, pattern: &Pattern, datum: &Vec<u8>) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => Some(datum.clone()),
            Pattern::Exact(expected) if expected == datum => Some(datum.clone()),
            Pattern::Exact(_) => None,
        }
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Any => None,
            Pattern::Exact(path) => Some(path.clone()),
        }
    }

    fn retained_datum_path(&self, datum: &Vec<u8>) -> Option<Vec<u8>> { Some(datum.clone()) }
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
