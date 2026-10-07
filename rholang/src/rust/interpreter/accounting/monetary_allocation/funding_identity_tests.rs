use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits(n: usize, m: usize) -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(n.max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(m.max(1)).unwrap(),
    }
}

fn permutations(values: &mut [usize], start: usize, result: &mut Vec<Vec<usize>>) {
    if start == values.len() {
        result.push(values.to_vec());
        return;
    }
    for index in start..values.len() {
        values.swap(start, index);
        permutations(values, start + 1, result);
        values.swap(start, index);
    }
}

#[test]
fn every_source_and_obligation_permutation_selects_identical_named_results() {
    let source_keys: [&[u8]; 3] = [b"wallet:a", b"wallet:b", b"wallet:c"];
    let obligation_keys: [&[u8]; 3] = [b"resource:a", b"resource:b", b"resource:c"];
    let capacities = [2, 3, 2];
    let obligations = [1, 2, 1];
    let eligible = [vec![true, false, true], vec![true; 3], vec![
        false, true, true,
    ]];
    let original = FundingMinimaxProblem {
        capacities: &capacities,
        obligations: &obligations,
        eligible: &eligible,
    };
    let baseline = canonicalize_funding_problem(
        original,
        &source_keys,
        &obligation_keys,
        limits(3, 3),
        &work(),
    )
    .unwrap();
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2], 0, &mut orders);
    for rows in &orders {
        for columns in &orders {
            let capacity: Vec<_> = rows.iter().map(|i| capacities[*i]).collect();
            let demand: Vec<_> = columns.iter().map(|j| obligations[*j]).collect();
            let edges: Vec<Vec<_>> = rows
                .iter()
                .map(|i| columns.iter().map(|j| eligible[*i][*j]).collect())
                .collect();
            let sources: Vec<_> = rows.iter().map(|i| source_keys[*i]).collect();
            let targets: Vec<_> = columns.iter().map(|j| obligation_keys[*j]).collect();
            let problem = FundingMinimaxProblem {
                capacities: &capacity,
                obligations: &demand,
                eligible: &edges,
            };
            let canonical =
                canonicalize_funding_problem(problem, &sources, &targets, limits(3, 3), &work())
                    .unwrap();
            assert_eq!(canonical.source_keys(), baseline.source_keys());
            assert_eq!(canonical.obligation_keys(), baseline.obligation_keys());
            for cursor in 0..3 {
                let actual = canonical.select(cursor, limits(3, 3), &work()).unwrap();
                assert_eq!(
                    actual,
                    baseline.select(cursor, limits(3, 3), &work()).unwrap()
                );
                let FundingPolicyResult::Selected(selection) = actual else {
                    panic!("the captured graph is feasible");
                };
                let restored = canonical
                    .restore_assignment(selection.assignment(), &work())
                    .unwrap();
                assert!(canonical
                    .verify_assignment(
                        cursor,
                        &restored,
                        selection.next_cursor(),
                        limits(3, 3),
                        &work(),
                    )
                    .unwrap());
                let checked = problem
                    .check_assignment(&restored, limits(3, 3), &work())
                    .unwrap();
                for (i, identity) in sources.iter().enumerate() {
                    let selected = canonical
                        .source_keys()
                        .iter()
                        .position(|key| key == identity)
                        .unwrap();
                    assert_eq!(
                        checked.source_debits()[i],
                        selection.totals().source_debits()[selected]
                    );
                }
            }
        }
    }
}

#[test]
fn repeated_obligations_retain_multiplicity_but_cannot_hide_different_terms() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 2],
        obligations: &[1, 1],
        eligible: &[vec![true; 2], vec![true; 2]],
    };
    let keys: [&[u8]; 2] = [b"same-resource", b"same-resource"];
    let canonical =
        canonicalize_funding_problem(problem, &[b"b", b"a"], &keys, limits(2, 2), &work()).unwrap();
    assert_eq!(canonical.obligation_keys().len(), 2);
    assert_eq!(canonical.problem().obligations, &[1, 1]);
    let FundingPolicyResult::Selected(selection) =
        canonical.select(0, limits(2, 2), &work()).unwrap()
    else {
        panic!("two resource occurrences are funded");
    };
    assert_eq!(selection.totals().total(), 2);
    for changed in [
        FundingMinimaxProblem {
            obligations: &[1, 2],
            ..problem
        },
        FundingMinimaxProblem {
            eligible: &[vec![true, false], vec![true; 2]],
            ..problem
        },
    ] {
        assert_eq!(
            canonicalize_funding_problem(changed, &[b"a", b"b"], &keys, limits(2, 2), &work()),
            Err(FundingIdentityError::InconsistentObligation)
        );
    }
}

#[test]
fn custody_aliases_must_be_resolved_before_canonical_allocation() {
    let problem = FundingMinimaxProblem {
        capacities: &[1, 1],
        obligations: &[2],
        eligible: &[vec![true], vec![true]],
    };
    assert_eq!(
        canonicalize_funding_problem(
            problem,
            &[b"same", b"same"],
            &[b"resource"],
            limits(2, 1),
            &work()
        ),
        Err(FundingIdentityError::DuplicateSource)
    );
    assert_eq!(
        canonicalize_funding_problem(problem, &[b"a"], &[b"resource"], limits(2, 1), &work()),
        Err(FundingIdentityError::InvalidIdentityDimensions)
    );
    assert_eq!(
        canonicalize_funding_problem(problem, &[b"", b"b"], &[b"resource"], limits(2, 1), &work()),
        Err(FundingIdentityError::EmptyIdentity)
    );
    assert!(matches!(
        canonicalize_funding_problem(
            problem,
            &[b"a", b"b"],
            &[b"resource"],
            limits(1, 1),
            &work()
        ),
        Err(FundingIdentityError::Search(
            FundingSearchError::InvalidProblem(FundingAssignmentError::TooManySources)
        ))
    ));
}

#[test]
fn location_authority_and_provenance_bytes_remain_distinct() {
    let keys: [&[u8]; 4] = [
        b"slot:a|authority:x|terms:old",
        b"slot:b|authority:x|terms:old",
        b"slot:a|authority:y|terms:old",
        b"slot:a|authority:x|terms:new",
    ];
    let problem = FundingMinimaxProblem {
        capacities: &[4],
        obligations: &[1; 4],
        eligible: &[vec![true; 4]],
    };
    let canonical =
        canonicalize_funding_problem(problem, &[b"wallet"], &keys, limits(1, 4), &work()).unwrap();
    let mut sorted = keys.to_vec();
    sorted.sort();
    assert_eq!(canonical.obligation_keys(), sorted);
    assert_eq!(
        canonical
            .restore_assignment(&[vec![7, 8, 9, 10]], &work())
            .unwrap(),
        vec![keys
            .iter()
            .map(|key| 7 + sorted.iter().position(|value| value == key).unwrap() as u64)
            .collect::<Vec<_>>()]
    );
}

#[test]
fn empty_obligations_and_large_cohorts_preserve_source_identity() {
    for count in [1, 2, 3, 64, 65, 129] {
        let names: Vec<_> = (0..count as u64).rev().map(u64::to_be_bytes).collect();
        let keys: Vec<_> = names.iter().map(|key| key.as_slice()).collect();
        let capacity = vec![u64::MAX; count];
        let edges = vec![vec![]; count];
        let canonical = canonicalize_funding_problem(
            FundingMinimaxProblem {
                capacities: &capacity,
                obligations: &[],
                eligible: &edges,
            },
            &keys,
            &[],
            limits(count, 0),
            &work(),
        )
        .unwrap();
        assert_eq!(canonical.source_keys().len(), count);
        assert!(canonical
            .source_keys()
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        assert_eq!(
            canonical
                .restore_assignment(&vec![vec![]; count], &work())
                .unwrap(),
            vec![Vec::<u64>::new(); count]
        );
        let FundingPolicyResult::Selected(selection) = canonical
            .select(count - 1, limits(count, 0), &work())
            .unwrap()
        else {
            panic!("zero funding is feasible");
        };
        assert_eq!(selection.next_cursor(), None);
        assert!(canonical
            .verify_assignment(
                count - 1,
                &vec![vec![]; count],
                None,
                limits(count, 0),
                &work(),
            )
            .unwrap());
        assert!(!canonical
            .verify_assignment(
                count - 1,
                &vec![vec![]; count],
                Some(0),
                limits(count, 0),
                &work(),
            )
            .unwrap());
    }
}

#[test]
fn every_sort_and_capture_budget_prefix_rejects_incomplete_results() {
    let keys: [&[u8]; 3] = [b"source:c", b"source:a", b"source:b"];
    let problem = FundingMinimaxProblem {
        capacities: &[2, 1, 3],
        obligations: &[1, 2],
        eligible: &[vec![true; 2], vec![false, true], vec![true, false]],
    };
    let measured = work();
    let expected =
        canonicalize_funding_problem(problem, &keys, &[b"y", b"x"], limits(3, 2), &measured)
            .unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let full = measured.usage(dimension).get();
        for prefix in 0..=full {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            bounds.set(dimension, HostWorkLimit::new(prefix));
            let result = canonicalize_funding_problem(
                problem,
                &keys,
                &[b"y", b"x"],
                limits(3, 2),
                &HostWorkBudget::new(bounds),
            );
            if prefix == full {
                assert_eq!(result, Ok(expected.clone()));
            } else {
                assert!(matches!(
                    result,
                    Err(FundingIdentityError::Search(FundingSearchError::HostWork(
                        _
                    )))
                ));
            }
        }
    }
}

#[test]
fn presentation_rejects_feasible_unfair_and_wrong_tie_assignments() {
    let canonical = canonicalize_funding_problem(
        FundingMinimaxProblem {
            capacities: &[4, 4],
            obligations: &[3],
            eligible: &[vec![true], vec![true]],
        },
        &[b"a", b"b"],
        &[b"q"],
        limits(2, 1),
        &work(),
    )
    .unwrap();
    let FundingPolicyResult::Selected(selected) =
        canonical.select(0, limits(2, 1), &work()).unwrap()
    else {
        panic!("feasible fixture");
    };
    assert_eq!(selected.assignment(), &[vec![2], vec![1]]);
    for assignment in [vec![vec![3], vec![0]], vec![vec![1], vec![2]]] {
        canonical
            .problem()
            .check_assignment(&assignment, limits(2, 1), &work())
            .unwrap();
        assert!(!canonical
            .verify_assignment(
                0,
                &assignment,
                selected.next_cursor(),
                limits(2, 1),
                &work()
            )
            .unwrap());
    }
    assert!(!canonical
        .verify_assignment(0, selected.assignment(), None, limits(2, 1), &work())
        .unwrap());
    assert!(!canonical
        .verify_assignment(0, &[vec![2]], selected.next_cursor(), limits(2, 1), &work())
        .unwrap());
    assert!(!canonical
        .verify_assignment(
            0,
            &[vec![2, 0], vec![1]],
            selected.next_cursor(),
            limits(2, 1),
            &work()
        )
        .unwrap());

    let tied = canonicalize_funding_problem(
        FundingMinimaxProblem {
            capacities: &[1, 1],
            obligations: &[1, 1],
            eligible: &[vec![true; 2], vec![true; 2]],
        },
        &[b"a", b"b"],
        &[b"x", b"y"],
        limits(2, 2),
        &work(),
    )
    .unwrap();
    let FundingPolicyResult::Selected(selected) = tied.select(0, limits(2, 2), &work()).unwrap()
    else {
        panic!("feasible fixture");
    };
    assert_eq!(selected.assignment(), &[vec![0, 1], vec![1, 0]]);
    let noncanonical = [vec![1, 0], vec![0, 1]];
    assert_eq!(
        tied.problem()
            .check_assignment(&noncanonical, limits(2, 2), &work())
            .unwrap(),
        *selected.totals()
    );
    assert!(!tied
        .verify_assignment(
            0,
            &noncanonical,
            selected.next_cursor(),
            limits(2, 2),
            &work()
        )
        .unwrap());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_presentations_match_the_recomputed_named_policy(
        capacities in prop::array::uniform2(0_u64..=4),
        obligations in prop::array::uniform2(0_u64..=3),
        edges in prop::array::uniform4(any::<bool>()),
        candidate in prop::array::uniform4(0_u64..=3),
        cursor in 0usize..2,
        next in prop::option::of(0usize..3),
    ) {
        let eligible = [edges[..2].to_vec(), edges[2..].to_vec()];
        let proposed = [candidate[..2].to_vec(), candidate[2..].to_vec()];
        let canonical = canonicalize_funding_problem(
            FundingMinimaxProblem { capacities: &capacities, obligations: &obligations, eligible: &eligible },
            &[b"b", b"a"], &[b"y", b"x"], limits(2, 2), &work(),
        ).unwrap();
        let expected = match canonical.select(cursor, limits(2, 2), &work()).unwrap() {
            FundingPolicyResult::Infeasible { .. } => false,
            FundingPolicyResult::Selected(selection) => {
                let restored = canonical.restore_assignment(selection.assignment(), &work()).unwrap();
                prop_assert!(canonical.verify_assignment(cursor, &restored, selection.next_cursor(), limits(2, 2), &work()).unwrap());
                let agrees = (0..2).all(|i| (0..2).all(|j| proposed[1-i][1-j] == selection.assignment()[i][j]));
                next == selection.next_cursor() && agrees
            }
        };
        prop_assert_eq!(canonical.verify_assignment(cursor, &proposed, next, limits(2, 2), &work()).unwrap(), expected);
    }
}

#[test]
fn presentation_work_exhaustion_is_an_error_not_a_decision() {
    let canonical = canonicalize_funding_problem(
        FundingMinimaxProblem {
            capacities: &[2],
            obligations: &[1],
            eligible: &[vec![true]],
        },
        &[b"a"],
        &[b"q"],
        limits(1, 1),
        &work(),
    )
    .unwrap();
    let FundingPolicyResult::Selected(selected) =
        canonical.select(0, limits(1, 1), &work()).unwrap()
    else {
        panic!("feasible fixture");
    };
    let measured = work();
    assert!(canonical
        .verify_assignment(
            0,
            selected.assignment(),
            selected.next_cursor(),
            limits(1, 1),
            &measured
        )
        .unwrap());
    let full = measured
        .usage(HostWorkDimension::VerificationOperations)
        .get();
    for prefix in 0..=full {
        let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
        bounds.set(
            HostWorkDimension::VerificationOperations,
            HostWorkLimit::new(prefix),
        );
        let result = canonical.verify_assignment(
            0,
            selected.assignment(),
            selected.next_cursor(),
            limits(1, 1),
            &HostWorkBudget::new(bounds),
        );
        if prefix == full {
            assert!(result.unwrap());
        } else {
            assert!(matches!(result, Err(FundingSearchError::HostWork(_))));
        }
    }
    assert!(matches!(
        canonical.verify_assignment(
            1,
            selected.assignment(),
            selected.next_cursor(),
            limits(1, 1),
            &work()
        ),
        Err(FundingSearchError::InvalidPriorityCursor)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn merge_order_matches_byte_order_and_preserves_every_occurrence(
        keys in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..32), 0..130),
    ) {
        let references: Vec<_> = keys.iter().map(Vec::as_slice).collect();
        let order = key_order(&references, &work()).unwrap();
        let actual: Vec<_> = order.iter().map(|index| references[*index]).collect();
        let mut expected = references.clone(); expected.sort();
        prop_assert_eq!(actual, expected);
        let mut indices = order; indices.sort_unstable();
        prop_assert_eq!(indices, (0..keys.len()).collect::<Vec<_>>());
    }
}
