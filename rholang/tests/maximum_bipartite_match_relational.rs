//! Differential and work-bound evidence for the lazy relational edge cache.
//!
//! The oracle below is the pre-cache iterative Kuhn traversal. It is test-only
//! and intentionally shares no relation state with the production matcher.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use proptest::prelude::*;
use rholang::rust::interpreter::matcher::maximum_bipartite_match::MaximumBipartiteMatch;

#[derive(Clone, Copy)]
struct OracleFrame {
    pattern: usize,
    next_target: usize,
    pending_target: Option<usize>,
}

fn oracle_match(
    pattern_count: usize,
    target_count: usize,
    edges: &[bool],
    calls: &mut usize,
) -> Option<Vec<(usize, usize, (usize, usize))>> {
    let edge = |pattern: usize, target: usize| edges[pattern * target_count + target];
    let mut assignments = vec![None; target_count];

    for root_pattern in 0..pattern_count {
        let mut seen_targets = vec![false; target_count];
        let mut stack = vec![OracleFrame {
            pattern: root_pattern,
            next_target: 0,
            pending_target: None,
        }];
        let mut augmented = false;

        loop {
            let Some(frame) = stack.last_mut() else {
                break;
            };
            let mut descended = false;
            while frame.next_target < target_count {
                let target = frame.next_target;
                frame.next_target += 1;
                if seen_targets[target] {
                    continue;
                }
                *calls += 1;
                if !edge(frame.pattern, target) {
                    continue;
                }
                seen_targets[target] = true;
                match assignments[target] {
                    None => {
                        assignments[target] = Some(frame.pattern);
                        stack.pop();
                        while let Some(parent) = stack.pop() {
                            assignments[parent
                                .pending_target
                                .expect("an oracle parent carries its displaced edge")] =
                                Some(parent.pattern);
                        }
                        augmented = true;
                        break;
                    }
                    Some(previous_pattern) => {
                        frame.pending_target = Some(target);
                        stack.push(OracleFrame {
                            pattern: previous_pattern,
                            next_target: 0,
                            pending_target: None,
                        });
                        descended = true;
                        break;
                    }
                }
            }
            if augmented {
                break;
            }
            if descended {
                continue;
            }
            stack.pop();
            if let Some(parent) = stack.last_mut() {
                parent.pending_target = None;
            } else {
                break;
            }
        }
        if !augmented {
            return None;
        }
    }

    Some(
        assignments
            .into_iter()
            .enumerate()
            .filter_map(|(target, pattern)| {
                pattern.map(|pattern| (target, pattern, (pattern, target)))
            })
            .collect(),
    )
}

fn relational_match(
    pattern_count: usize,
    target_count: usize,
    edges: &[bool],
) -> (
    Option<Vec<(usize, usize, (usize, usize))>>,
    usize,
    rholang::rust::interpreter::matcher::maximum_bipartite_match::MaximumBipartiteMatchStats,
) {
    let graph = edges.to_vec();
    let calls = Arc::new(AtomicUsize::new(0));
    let closure_calls = Arc::clone(&calls);
    let mut matcher = MaximumBipartiteMatch::new_with_cache_policy(
        Box::new(move |pattern: &usize, target: &usize| {
            closure_calls.fetch_add(1, Ordering::Relaxed);
            graph[*pattern * target_count + *target].then_some((*pattern, *target))
        }),
        Box::new(|_| true),
    );
    let result = matcher.find_matches((0..pattern_count).collect(), (0..target_count).collect());
    let stats = matcher.stats();
    let observed_calls = calls.load(Ordering::Relaxed);
    assert_eq!(observed_calls, stats.edge_evaluations);
    (result, observed_calls, stats)
}

fn assert_equivalent(pattern_count: usize, target_count: usize, edges: &[bool]) {
    let mut oracle_calls = 0usize;
    let expected = oracle_match(pattern_count, target_count, edges, &mut oracle_calls);
    let (actual, treatment_calls, stats) = relational_match(pattern_count, target_count, edges);
    assert_eq!(actual, expected);
    assert_eq!(treatment_calls, stats.edge_evaluations);
    assert!(treatment_calls <= pattern_count.saturating_mul(target_count));
}

#[test]
fn exhaustive_bounded_graphs_preserve_the_exact_nominal_assignment() {
    for pattern_count in 0usize..=4 {
        for target_count in 0usize..=4 {
            let pair_count = pattern_count * target_count;
            for mask in 0u64..(1u64 << pair_count) {
                let edges = (0..pair_count)
                    .map(|bit| mask & (1u64 << bit) != 0)
                    .collect::<Vec<_>>();
                assert_equivalent(pattern_count, target_count, &edges);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_graphs_preserve_the_exact_nominal_assignment(
        pattern_count in 0usize..=8,
        target_count in 0usize..=8,
        raw_edges in prop::collection::vec(any::<bool>(), 0..=64),
    ) {
        let pair_count = pattern_count * target_count;
        let edges = (0..pair_count)
            .map(|index| raw_edges.get(index).copied().unwrap_or(false))
            .collect::<Vec<_>>();
        assert_equivalent(pattern_count, target_count, &edges);
    }
}

#[test]
fn diagonal_control_is_invariant_and_performs_no_relation_reuse() {
    const WIDTH: usize = 128;
    let edges = (0..WIDTH * WIDTH)
        .map(|index| index / WIDTH == index % WIDTH)
        .collect::<Vec<_>>();
    let mut control_calls = 0;
    let expected = oracle_match(WIDTH, WIDTH, &edges, &mut control_calls);
    let (actual, treatment_calls, stats) = relational_match(WIDTH, WIDTH, &edges);

    assert_eq!(actual, expected);
    assert_eq!(treatment_calls, control_calls);
    assert_eq!(stats.relation_row_reuses, 0);
    assert_eq!(stats.cached_edge_visits, 0);
}

#[test]
fn compatibility_constructor_preserves_the_nominal_fnmut_call_schedule() {
    const WIDTH: usize = 32;
    let edges = (0..WIDTH * WIDTH)
        .map(|index| {
            let pattern = index / WIDTH;
            let target = index % WIDTH;
            if pattern == 0 {
                target == 1
            } else {
                target == pattern || target == pattern + 1
            }
        })
        .collect::<Vec<_>>();
    let mut control_calls = 0;
    let expected = oracle_match(WIDTH, WIDTH, &edges, &mut control_calls);

    let graph = edges.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let closure_calls = Arc::clone(&calls);
    let mut matcher =
        MaximumBipartiteMatch::new(Box::new(move |pattern: &usize, target: &usize| {
            closure_calls.fetch_add(1, Ordering::Relaxed);
            graph[*pattern * WIDTH + *target].then_some((*pattern, *target))
        }));
    let actual = matcher.find_matches((0..WIDTH).collect(), (0..WIDTH).collect());
    let stats = matcher.stats();

    assert_eq!(actual, expected);
    assert_eq!(calls.load(Ordering::Relaxed), control_calls);
    assert_eq!(stats.edge_evaluations, control_calls);
    assert_eq!(stats.cached_edge_visits, 0);
    assert_eq!(stats.relation_row_reuses, 0);
}

#[test]
fn rejecting_displacement_chain_reuses_the_relation_and_reduces_edge_evaluations() {
    const WIDTH: usize = 256;
    let edges = (0..WIDTH * WIDTH)
        .map(|index| {
            let pattern = index / WIDTH;
            let target = index % WIDTH;
            if pattern == 0 {
                target == 1
            } else {
                target == pattern || target == pattern + 1
            }
        })
        .collect::<Vec<_>>();
    let mut control_calls = 0;
    let expected = oracle_match(WIDTH, WIDTH, &edges, &mut control_calls);
    let (actual, treatment_calls, stats) = relational_match(WIDTH, WIDTH, &edges);

    assert_eq!(actual, expected);
    assert!(treatment_calls < control_calls);
    assert!(stats.relation_row_reuses > 0);
    assert!(stats.cached_edge_visits > 0);
    assert!(stats.successful_edge_evaluations <= 2 * WIDTH);
}
