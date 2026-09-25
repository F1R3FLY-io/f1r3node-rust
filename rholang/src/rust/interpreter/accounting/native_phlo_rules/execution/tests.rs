use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostRegion, CostSignatureCompound};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::native_phlo_rules::tests::schedule;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, SignedPhloControls,
};

mod budget_trace;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> NativePhloRegionLimits {
    NativePhloRegionLimits {
        regions: 4096,
        encoded_authority_bytes: 10_000_000,
    }
}

fn with_contract<R>(
    weights: [u64; 4],
    limit: u64,
    bound: u64,
    price: u64,
    test: impl FnOnce(NativePhloExecutionContract<'_>) -> R,
) -> R {
    let mut descriptor = schedule();
    descriptor.actual_price = price;
    for (class, weight) in descriptor.classes.iter_mut().zip(weights) {
        class.weight = weight;
    }
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let selected = binding.schedule();
    let permitted = [selected];
    let owners = [price];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: price,
            required_owner_ceilings: &owners,
            permitted_schedules: &permitted,
        },
        selected,
        bound,
    )
    .unwrap();
    test(NativePhloExecutionContract::new(controls, &binding).unwrap())
}

fn row(owners: usize, regions: usize, comm: bool, raw: [u64; 3]) -> Arc<ByteObservation> {
    let atom = CostSignature {
        value: Some(Value::Ground(vec![7])),
    };
    let signature = match owners {
        0 => CostSignature {
            value: Some(Value::Unit(true)),
        },
        1 => atom,
        _ => {
            sort_signature(&CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![atom; owners],
                })),
            })
            .term
        }
    };
    let mut authority = CostAuthority {
        regions: (0..regions)
            .map(|index| {
                let mut instance_id = vec![0; 32];
                instance_id[24..].copy_from_slice(&(index as u64).to_be_bytes());
                CostRegion {
                    instance_id,
                    signature: Some(signature.clone()),
                }
            })
            .collect(),
    };
    authority
        .regions
        .sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    Arc::new(ByteObservation {
        event_id: [4; 32],
        kind: if comm {
            AuthorityByteEventKind::Comm
        } else {
            AuthorityByteEventKind::ProduceIntroduction
        },
        authority,
        measurement: Some(ByteCharge {
            introduction_bytes: raw[0],
            transfer_bytes: raw[1],
            trace_bytes: raw[2],
        }),
        legacy_amount: None,
    })
}

#[test]
fn arbitrary_authorities_and_distinct_regions_keep_every_occurrence() {
    with_contract([2, 3, 5, 7], u64::MAX, u64::MAX, 0, |contract| {
        let mut meter = contract.reservation();
        let mut expected_total = 0;
        for owners in [0, 1, 2, 3, 65, 1024] {
            for regions in [1, 2, 3] {
                let observation = row(owners, regions, true, [11, 13, 17]);
                let prepared = meter
                    .prepare(Arc::clone(&observation), limits(), &budget())
                    .unwrap();
                assert_eq!(
                    prepared.usage(),
                    (2 + 3 * 11 + 5 * 13 + 7 * 17) * owners as u64 * regions as u64
                );
                assert!(Arc::ptr_eq(prepared.observation(), &observation));
                meter.reserve(&prepared).unwrap();
                expected_total += prepared.usage();
                assert_eq!(meter.used(), expected_total);
            }
        }
    });
}

#[test]
fn full_width_quantities_zero_prices_zero_valuations_and_limits_are_exact() {
    with_contract([0, 1, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let maximum = contract
            .prepare(row(1, 1, false, [u64::MAX, 0, 0]), limits(), &budget())
            .unwrap();
        let zero = contract
            .prepare(row(0, 1, true, [u64::MAX; 3]), limits(), &budget())
            .unwrap();
        let one = contract
            .prepare(row(1, 1, false, [1, 0, 0]), limits(), &budget())
            .unwrap();
        let mut meter = contract.reservation();
        meter.reserve(&maximum).unwrap();
        meter.reserve(&zero).unwrap();
        assert_eq!(meter.used(), u64::MAX);
        assert!(matches!(
            meter.reserve(&one),
            Err(NativePhloExecutionError::Overflow)
        ));
        assert_eq!(meter.used(), u64::MAX);
    });
    with_contract([u64::MAX; 4], 0, 0, 0, |contract| {
        let zero = contract
            .prepare(row(0, 1, true, [u64::MAX; 3]), limits(), &budget())
            .unwrap();
        let mut meter = contract.reservation();
        meter.reserve(&zero).unwrap();
        assert!(matches!(
            meter.prepare(row(2, 1, true, [0; 3]), limits(), &budget()),
            Err(NativePhloExecutionError::Overflow)
        ));
    });
    with_contract([1; 4], 100, 3, 1, |contract| {
        let first = contract
            .prepare(row(1, 1, true, [2, 0, 0]), limits(), &budget())
            .unwrap();
        let mut meter = contract.reservation();
        meter.reserve(&first).unwrap();
        assert!(matches!(
            meter.reserve(&first),
            Err(NativePhloExecutionError::BoundExceeded)
        ));
        assert_eq!(meter.used(), 3);
    });
}

#[test]
fn foreign_contracts_incomplete_measurements_and_bad_authorities_are_rejected() {
    with_contract([1; 4], 100, 100, 1, |contract| {
        let observation = row(3, 1, true, [1; 3]);
        let prepared = contract
            .prepare(Arc::clone(&observation), limits(), &budget())
            .unwrap();
        with_contract([1; 4], 100, 100, 1, |other| {
            let mut meter = other.reservation();
            assert!(matches!(
                meter.reserve(&prepared),
                Err(NativePhloExecutionError::ContractMismatch)
            ));
            assert_eq!(meter.used(), 0);
        });
        for change in 0..5 {
            let mut altered = observation.as_ref().clone();
            match change {
                0 => altered.measurement = None,
                1 => altered.authority.regions.clear(),
                2 => altered.authority.regions[0]
                    .instance_id
                    .pop()
                    .map(|_| ())
                    .unwrap(),
                3 => altered.authority.regions[0].signature = None,
                _ => altered
                    .authority
                    .regions
                    .push(altered.authority.regions[0].clone()),
            }
            assert!(contract
                .prepare(Arc::new(altered), limits(), &budget())
                .is_err());
        }
        assert!(contract
            .prepare(
                Arc::clone(&observation),
                NativePhloRegionLimits {
                    regions: 0,
                    ..limits()
                },
                &budget()
            )
            .is_err());
        assert!(contract
            .prepare(
                Arc::clone(&observation),
                NativePhloRegionLimits {
                    encoded_authority_bytes: 0,
                    ..limits()
                },
                &budget()
            )
            .is_err());
        let exhausted = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        assert!(contract.prepare(observation, limits(), &exhausted).is_err());
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_reservation_histories_preserve_exact_usage_and_failure_atomicity(
        limit in any::<u64>(),
        charges in prop::collection::vec(prop_oneof![Just(0_u64), Just(u64::MAX), any::<u64>(), 0_u64..100], 0..80),
    ) {
        let mut used = 0_u64;
        let mut reference = 0_u128;
        for charge in charges {
            let expected = reference + u128::from(charge);
            let accepted = reserve_native_usage(&mut used, limit, charge).is_ok();
            prop_assert_eq!(accepted, expected <= u128::from(limit));
            if accepted { reference = expected; }
            prop_assert_eq!(u128::from(used), reference);
            prop_assert!(used <= limit);
        }
        let before = used;
        prop_assert!(reserve_native_usage(&mut used, limit, 0).is_ok());
        prop_assert_eq!(used, before);
    }

    #[test]
    fn prepared_usage_and_reservations_match_independent_integer_oracle(
        weights in prop::array::uniform4(0_u64..20),
        raw in prop::array::uniform3(0_u64..100),
        owners in 0_usize..80, regions in 1_usize..5, comm in any::<bool>(),
        limit in 0_u64..100_000, repeats in 0_usize..12,
    ) {
        with_contract(weights, limit, limit, 0, |contract| {
            let charge = contract.prepare(row(owners, regions, comm, raw), limits(), &budget()).unwrap();
            let expected = (u128::from(comm) * u128::from(weights[0]) +
                raw.into_iter().zip(weights[1..].iter()).map(|(n,w)| u128::from(n) * u128::from(*w)).sum::<u128>()) * owners as u128 * regions as u128;
            assert_eq!(u128::from(charge.usage()), expected);
            let mut meter = contract.reservation();
            let mut used = 0_u128;
            for _ in 0..repeats {
                let fits = used + expected <= u128::from(limit);
                assert_eq!(meter.reserve(&charge).is_ok(), fits);
                if fits { used += expected; }
                assert_eq!(u128::from(meter.used()), used);
            }
        });
    }
}

#[test]
fn loom_reservation_and_receipt_publication_share_one_critical_section() {
    let (policy, charges) = with_contract([2, 0, 1, 0], 12, 12, 0, |contract| {
        let charges: Vec<_> = [(0, true, 1), (0, true, 2), (1, false, 1)]
            .into_iter()
            .map(|(identity, comm, transfer)| {
                let mut observation = row(3, 1, comm, [0, transfer, 0]);
                Arc::make_mut(&mut observation).event_id = [identity as u8; 32];
                let charge = contract.prepare(observation, limits(), &budget()).unwrap();
                (identity, charge)
            })
            .collect();
        (contract.reservation().policy, charges)
    });
    loom::model(move || {
        let reservation = NativePhloReservation {
            policy: Arc::clone(&policy),
            used: 0,
        };
        let state = loom::sync::Arc::new(loom::sync::Mutex::new((reservation, [
            None::<PreparedNativePhloCharge>,
            None,
        ])));
        let mut handles = Vec::new();
        for (identity, charge) in charges.iter().cloned() {
            let shared = loom::sync::Arc::clone(&state);
            handles.push(loom::thread::spawn(move || {
                loom::thread::yield_now();
                let mut state = shared.lock().unwrap();
                if state.1[identity].is_none() && state.0.reserve(&charge).is_ok() {
                    state.1[identity] = Some(charge);
                }
                assert_eq!(
                    state.0.used(),
                    state
                        .1
                        .iter()
                        .flatten()
                        .map(PreparedNativePhloCharge::usage)
                        .sum::<u64>()
                );
                assert!(state.0.used() <= 12);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let state = state.lock().unwrap();
        assert_eq!(
            state.0.used(),
            state
                .1
                .iter()
                .flatten()
                .map(PreparedNativePhloCharge::usage)
                .sum::<u64>()
        );
        for (identity, charge) in state.1.iter().enumerate() {
            if let Some(charge) = charge {
                assert_eq!(charge.observation().event_id, [identity as u8; 32]);
            }
        }
    });
}
