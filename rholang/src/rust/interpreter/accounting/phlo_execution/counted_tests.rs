use super::*;

fn counted<'a>(parts: &'a [Vec<PhloResourceAmount<'a>>; 5]) -> CountedPhloExecutionWitness<'a> {
    CountedPhloExecutionWitness {
        available: &parts[0],
        required: &parts[1],
        used: &parts[2],
        unused: &parts[3],
        fresh: &parts[4],
    }
}

fn fresh_counted<'a>(entries: &'a [PhloResourceAmount<'a>]) -> CountedPhloExecutionWitness<'a> {
    CountedPhloExecutionWitness {
        available: &[],
        required: entries,
        used: &[],
        unused: &[],
        fresh: entries,
    }
}

#[test]
fn maximum_quantity_needs_two_entries_and_keeps_exact_usage() {
    let authority = Sig::Ground(vec![1]);
    let schedules = [PhloSchedule {
        actual_price: 0,
        ..SCHEDULES[0]
    }];
    let controls = configured(&schedules, u64::MAX, u64::MAX);
    let entries = [PhloResourceAmount {
        resource: resource(&authority),
        quantity: u64::MAX,
    }];
    let limits = PhloExecutionLimits {
        resource_entries: 2,
        authority_nodes: 2,
        key_bytes: 2 * (b"slot".len() + b"acquired".len() + 1),
    };
    let checked = check_counted_phlo_execution(controls, fresh_counted(&entries), limits).unwrap();
    assert_eq!(checked.usage(), u64::MAX);
    assert_eq!(checked.fresh_usage(), u64::MAX);
    assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&[])), 1);
    assert!(checked.witness().occurrences().is_none());
    let obligations = project_phlo_obligations(
        checked,
        PhloOutcome::Accepted(&[]),
        std::num::NonZeroUsize::new(2).unwrap(),
    )
    .unwrap();
    assert_eq!(obligations.amounts(), &[1, 0]);
    assert_eq!(
        check_counted_phlo_execution(controls, fresh_counted(&entries), PhloExecutionLimits {
            resource_entries: 1,
            ..limits
        }),
        Err(PhloExecutionError::TooManyResourceEntries)
    );
}

#[test]
fn counted_overflow_and_zero_quantities_reject_without_changing_inputs() {
    let authority = Sig::Ground(vec![1]);
    let schedules = [PhloSchedule {
        actual_price: 0,
        ..SCHEDULES[0]
    }];
    let controls = configured(&schedules, u64::MAX, u64::MAX);
    let entries = [
        PhloResourceAmount {
            resource: resource(&authority),
            quantity: u64::MAX,
        },
        PhloResourceAmount {
            resource: resource(&authority),
            quantity: 1,
        },
    ];
    let original = entries;
    assert_eq!(
        check_counted_phlo_execution(controls, fresh_counted(&entries), LIMITS),
        Err(PhloExecutionError::ArithmeticOverflow)
    );
    assert_eq!(entries, original);
    let product = [PhloResourceAmount {
        resource: PhloResource {
            class: 1,
            ..resource(&authority)
        },
        quantity: u64::MAX,
    }];
    assert_eq!(
        check_counted_phlo_execution(controls, fresh_counted(&product), LIMITS),
        Err(PhloExecutionError::ArithmeticOverflow)
    );
    for role in 0..5 {
        let mut parts: [Vec<_>; 5] = std::array::from_fn(|_| Vec::new());
        parts[role].push(PhloResourceAmount {
            resource: resource(&authority),
            quantity: 0,
        });
        assert_eq!(
            check_counted_phlo_execution(controls, counted(&parts), LIMITS),
            Err(PhloExecutionError::ZeroQuantity)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn counted_acceptance_matches_both_partition_equations_and_exhaustion(
        amounts in prop::array::uniform5(0_u64..33),
        class in 0_usize..4,
    ) {
        let authority = Sig::Ground(vec![1]);
        let parts: [Vec<_>; 5] = std::array::from_fn(|index| {
            if amounts[index] == 0 { vec![] } else {
                vec![PhloResourceAmount {
                    resource: PhloResource { class, ..resource(&authority) },
                    quantity: amounts[index],
                }]
            }
        });
        let [available, required, used, unused, fresh] = amounts;
        let valid = available == used + unused && required == used + fresh
            && (unused == 0 || fresh == 0);
        let result = check_counted_phlo_execution(controls(10_000), counted(&parts), LIMITS);
        prop_assert_eq!(result.is_ok(), valid);
        if let Ok(checked) = result {
            prop_assert_eq!(checked.usage(), required * SCHEDULES[0].weights[class]);
            prop_assert_eq!(checked.prepaid_usage(), used * SCHEDULES[0].weights[class]);
            prop_assert_eq!(checked.fresh_usage(), fresh * SCHEDULES[0].weights[class]);
        }
    }

    #[test]
    fn counted_partitions_match_expansion_and_split_quantities(
        entries in prop::collection::vec((0_usize..4, 0_u64..9, 0_u64..9), 0..33),
        reverse in any::<bool>(),
    ) {
        let authorities: Vec<_> = (0..entries.len()).map(|i| Sig::Ground(i.to_le_bytes().to_vec())).collect();
        let mut parts: [Vec<PhloResourceAmount<'_>>; 5] = std::array::from_fn(|_| Vec::new());
        let mut expected_usage = 0;
        let mut expected_new = 0;
        for ((class, supply, demand), authority) in entries.iter().zip(&authorities) {
            let key = PhloResource { class: *class, ..resource(authority) };
            let matched = (*supply).min(*demand);
            for (part, quantity) in parts.iter_mut().zip([
                *supply, *demand, matched, supply - matched, demand - matched,
            ]) {
                if quantity != 0 { part.push(PhloResourceAmount { resource: key, quantity }); }
            }
            expected_usage += demand * SCHEDULES[0].weights[*class];
            expected_new += (demand - matched) * SCHEDULES[0].weights[*class];
        }
        let expanded: [Vec<_>; 5] = std::array::from_fn(|i| parts[i].iter()
            .flat_map(|entry| std::iter::repeat_n(entry.resource, entry.quantity as usize)).collect());
        let compressed = check_counted_phlo_execution(controls(10_000), counted(&parts), LIMITS).unwrap();
        let original = check_phlo_execution(controls(10_000), witness(
            &expanded[0], &expanded[1], &expanded[2], &expanded[3], &expanded[4]), LIMITS).unwrap();
        prop_assert_eq!(compressed.usage(), expected_usage);
        prop_assert_eq!(compressed.fresh_usage(), expected_new);
        prop_assert_eq!(compressed.prepaid_usage(), expected_usage - expected_new);
        prop_assert_eq!(compressed.usage(), original.usage());
        prop_assert_eq!(compressed.retained_charge(PhloOutcome::Accepted(&[])), 2 * expected_new + 1);
        let mut split: [Vec<_>; 5] = std::array::from_fn(|i| parts[i].iter().flat_map(|entry| {
            let left = entry.quantity / 2;
            [left, entry.quantity - left].into_iter().filter(|q| *q > 0)
                .map(|quantity| PhloResourceAmount { quantity, ..*entry })
        }).collect());
        if reverse { for part in &mut split { part.reverse(); } }
        let split_checked = check_counted_phlo_execution(controls(10_000), counted(&split), LIMITS).unwrap();
        prop_assert_eq!(split_checked.usage(), expected_usage);
        prop_assert_eq!(split_checked.fresh_usage(), expected_new);
        for outcome in [PhloOutcome::Accepted(&[]), PhloOutcome::Accepted(&[PhloFailure::User]),
            PhloOutcome::Accepted(&[PhloFailure::Platform]), PhloOutcome::AdmissionRejected] {
            let cap = std::num::NonZeroUsize::new(entries.len() + 1).unwrap();
            let a = project_phlo_obligations(compressed, outcome, cap).unwrap();
            let b = project_phlo_obligations(original, outcome, cap).unwrap();
            prop_assert_eq!(a.amounts(), b.amounts());
            prop_assert_eq!(a.keys(), b.keys());
            let c = project_phlo_obligations(split_checked, outcome, cap).unwrap();
            prop_assert_eq!(a.total(), c.total());
            for (key, amount) in a.keys().iter().zip(a.amounts()) {
                let index = c.keys().iter().position(|other| other == key).unwrap();
                prop_assert_eq!(*amount, c.amounts()[index]);
            }
        }
    }
}
