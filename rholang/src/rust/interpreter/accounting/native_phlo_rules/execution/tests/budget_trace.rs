use super::*;
use crate::rust::interpreter::accounting::native_runtime::{NativePathTrie, PathId};

/// The node of the one-segment path [(id, 0)] in a fixture trie (C7b,
/// DR-86).
fn fixture_path(id: u64) -> PathId {
    static PATHS: std::sync::LazyLock<NativePathTrie> = std::sync::LazyLock::new(|| {
        let mut paths = NativePathTrie::new(128);
        for id in 0..64 {
            paths
                .intern(PathId::ROOT, &[(id, 0)])
                .expect("fixture path fits");
        }
        paths
    });
    PATHS
        .find(&[(id, 0)])
        .expect("fixture paths cover ids below 64")
}

fn trace_limits() -> NativeBudgetTraceLimits {
    NativeBudgetTraceLimits {
        attempts: 1024,
        path_segments: 128,
        regions: limits(),
    }
}

fn attempt(id: u64, charge: u64, granted: bool) -> NativeBudgetAttempt {
    NativeBudgetAttempt {
        occurrence: NativeBudgetOccurrence {
            session: [1; 32],
            path: fixture_path(id),
            stage: NativeAttemptStage::ProduceIntroduction,
        },
        observation: row(1, 1, false, [charge, 0, 0]),
        granted,
        // Added by G3 (DR-135, S4).
        owner_refused: false,
        // Added by G3 (DR-135, S4 review).
        restorations: Arc::from([]),
    }
}

#[test]
fn denied_attempt_keeps_its_original_prefix_decision_after_reordering() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![
            attempt(0, 4, true),
            attempt(1, 2, false),
            attempt(2, 1, true),
        ]
        .into();
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        assert_eq!(checked.total(), 5);
        assert!(matches!(
            checked.finish(),
            Err(NativeBudgetTraceError::Incomplete)
        ));
        for index in [1, 2, 0] {
            let entry = &rows[index];
            let decision = checked
                .consume(&entry.occurrence, &entry.observation)
                .unwrap();
            assert_eq!(
                matches!(decision, NativeBudgetReplayDecision::Accepted { .. }),
                entry.granted
            );
        }
        assert_eq!(checked.finish().unwrap(), 5);
        assert_eq!(checked.remaining(), 0);
    });
}

#[test]
fn forged_denials_and_duplicate_occurrences_are_rejected() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        for charge in [0, 1, 5] {
            let rows = vec![attempt(0, charge, false)].into();
            assert!(matches!(
                contract.check_budget_trace([1; 32], rows, trace_limits()),
                Err(NativeBudgetTraceError::Decision)
            ));
        }
        let entry = attempt(0, 1, true);
        let rows = vec![entry.clone(), entry].into();
        assert!(matches!(
            contract.check_budget_trace([1; 32], rows, trace_limits()),
            Err(NativeBudgetTraceError::Duplicate)
        ));
    });
}

#[test]
fn exact_observation_and_session_are_required_without_consuming_on_error() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        assert!(matches!(
            contract.check_budget_trace([2; 32], rows.clone(), trace_limits()),
            Err(NativeBudgetTraceError::Session)
        ));
        let mut wrong_stage = rows[0].clone();
        wrong_stage.occurrence.stage = NativeAttemptStage::Comm;
        assert!(matches!(
            contract.check_budget_trace([1; 32], vec![wrong_stage].into(), trace_limits()),
            Err(NativeBudgetTraceError::Stage)
        ));
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        let mut altered = rows[0].observation.as_ref().clone();
        altered.event_id[0] ^= 1;
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &altered),
            Err(NativeBudgetTraceError::Observation)
        ));
        let mut missing = rows[0].occurrence.clone();
        missing.path = fixture_path(63);
        assert!(matches!(
            checked.consume(&missing, &rows[0].observation),
            Err(NativeBudgetTraceError::Unknown)
        ));
        assert_eq!(checked.remaining(), 1);
        checked
            .consume(&rows[0].occurrence, &rows[0].observation)
            .unwrap();
        assert!(matches!(
            checked.consume(&rows[0].occurrence, &rows[0].observation),
            Err(NativeBudgetTraceError::Consumed)
        ));
        assert_eq!(checked.finish().unwrap(), 1);
    });
}

#[test]
fn full_width_addition_and_preparation_overflow_are_denials() {
    with_contract([0, 1, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let rows: Arc<[_]> = vec![
            attempt(0, u64::MAX, true),
            attempt(1, 1, false),
            attempt(2, 0, true),
        ]
        .into();
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        for index in [2, 1, 0] {
            checked
                .consume(&rows[index].occurrence, &rows[index].observation)
                .unwrap();
        }
        assert_eq!(checked.finish().unwrap(), u64::MAX);
    });
    with_contract([0, 2, 0, 0], u64::MAX, u64::MAX, 0, |contract| {
        let entry = attempt(0, u64::MAX, false);
        let mut checked = contract
            .check_budget_trace([1; 32], vec![entry.clone()].into(), trace_limits())
            .unwrap();
        assert_eq!(
            checked
                .consume(&entry.occurrence, &entry.observation)
                .unwrap(),
            NativeBudgetReplayDecision::Denied
        );
        assert_eq!(checked.finish().unwrap(), 0);
        let forged = NativeBudgetAttempt {
            granted: true,
            ..entry
        };
        assert!(matches!(
            contract.check_budget_trace([1; 32], vec![forged].into(), trace_limits()),
            Err(NativeBudgetTraceError::Decision)
        ));
    });
}

#[test]
fn trace_limits_apply_before_replay() {
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        for restricted in [
            NativeBudgetTraceLimits {
                attempts: 0,
                ..trace_limits()
            },
            NativeBudgetTraceLimits {
                path_segments: 0,
                ..trace_limits()
            },
        ] {
            assert!(matches!(
                contract.check_budget_trace([1; 32], rows.clone(), restricted),
                Err(NativeBudgetTraceError::Limit)
            ));
        }
    });
}

proptest! {
    #[test]
    fn checked_prefix_properties_survive_arbitrary_replay_permutations(
        limit in any::<u64>(),
        inputs in prop::collection::vec((any::<u64>(), any::<u64>()), 0..48),
    ) {
        with_contract([0, 1, 0, 0], limit, limit, 0, |contract| {
            let mut expected = 0_u128;
            let rows: Arc<[_]> = inputs.iter().enumerate().map(|(id, (charge, _))| {
                let next = expected + u128::from(*charge);
                let granted = next <= u128::from(limit);
                if granted { expected = next; }
                attempt(id as u64, *charge, granted)
            }).collect::<Vec<_>>().into();
            let mut checked = contract.check_budget_trace([1; 32], rows.clone(), trace_limits()).unwrap();
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            order.sort_by_key(|index| (inputs[*index].1, *index));
            for index in order {
                let entry = &rows[index];
                let decision = checked.consume(&entry.occurrence, &entry.observation).unwrap();
                assert_eq!(matches!(decision, NativeBudgetReplayDecision::Accepted { .. }), entry.granted);
            }
            assert_eq!(u128::from(checked.finish().unwrap()), expected);
            for index in 0..rows.len() {
                let mut corrupted = rows.to_vec();
                corrupted[index].granted = !corrupted[index].granted;
                assert!(matches!(contract.check_budget_trace([1; 32], corrupted.into(), trace_limits()), Err(NativeBudgetTraceError::Decision)));
            }
        });
    }
}

#[test]
fn equal_scalar_cost_does_not_allow_observation_substitution() {
    with_contract([0, 0, 0, 0], 0, 0, 0, |contract| {
        let original = attempt(0, 7, true);
        let mut checked = contract
            .check_budget_trace([1; 32], vec![original.clone()].into(), trace_limits())
            .unwrap();
        let observation = original.observation.as_ref();
        let mut mutations = vec![observation.clone(); 6];
        mutations[0].event_id[0] ^= 1;
        mutations[1].kind = AuthorityByteEventKind::ConsumeIntroduction;
        mutations[2]
            .measurement
            .as_mut()
            .unwrap()
            .introduction_bytes += 1;
        mutations[3].authority.regions[0].instance_id[0] ^= 1;
        mutations[4].authority.regions[0]
            .signature
            .as_mut()
            .unwrap()
            .value = Some(Value::Ground(vec![8]));
        mutations[5].legacy_amount = Some(0);
        for altered in mutations {
            assert_eq!(
                contract
                    .prepare(Arc::new(altered.clone()), limits())
                    .unwrap()
                    .usage(),
                0
            );
            assert!(matches!(
                checked.consume(&original.occurrence, &altered),
                Err(NativeBudgetTraceError::Observation)
            ));
            assert_eq!(checked.remaining(), 1);
        }
        checked.consume(&original.occurrence, observation).unwrap();
        assert_eq!(checked.finish().unwrap(), 0);
    });
}

#[test]
fn loom_parallel_replay_keeps_the_checked_decisions_and_usage() {
    let (rows, checked) = with_contract([0, 1, 0, 0], 1, 1, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true), attempt(1, 1, false)].into();
        let checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        (rows, checked)
    });
    loom::model(move || {
        let shared = loom::sync::Arc::new(loom::sync::Mutex::new(checked.clone()));
        let workers = rows
            .iter()
            .cloned()
            .map(|entry| {
                let shared = shared.clone();
                loom::thread::spawn(move || {
                    let decision = shared
                        .lock()
                        .unwrap()
                        .consume(&entry.occurrence, &entry.observation)
                        .unwrap();
                    assert_eq!(
                        matches!(decision, NativeBudgetReplayDecision::Accepted { .. }),
                        entry.granted
                    );
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(shared.lock().unwrap().finish().unwrap(), 1);
    });
}

/// Added by G3 (DR-135): replay authenticates an observation by equality
/// (SA1). A region that differs only in its price or its caller is another
/// region, with another hash, so replay rejects the forged observation and
/// still accepts the recorded one.
#[test]
fn observation_equality_sees_the_price() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use models::rhoapi::CostCaller;

    let hash = |region: &CostRegion| {
        let mut hasher = DefaultHasher::new();
        region.hash(&mut hasher);
        hasher.finish()
    };
    with_contract([0, 1, 0, 0], 5, 5, 0, |contract| {
        let rows: Arc<[_]> = vec![attempt(0, 1, true)].into();
        let recorded = rows[0].observation.authority.regions[0].clone();
        assert_eq!(hash(&recorded), hash(&recorded.clone()));
        let mut checked = contract
            .check_budget_trace([1; 32], rows.clone(), trace_limits())
            .unwrap();
        for change in 0..4 {
            let mut altered = rows[0].observation.as_ref().clone();
            let region = &mut altered.authority.regions[0];
            match change {
                0 => region.price += 1,
                1 => region.price = u64::MAX,
                2 => {
                    region.caller = Some(CostCaller {
                        signature: None,
                        price: 0,
                    })
                }
                _ => {
                    region.caller = Some(CostCaller {
                        signature: region.signature.clone(),
                        price: 1,
                    })
                }
            }
            assert_ne!(altered.authority.regions[0], recorded, "change {change}");
            assert_ne!(
                hash(&altered.authority.regions[0]),
                hash(&recorded),
                "change {change}"
            );
            assert!(matches!(
                checked.consume(&rows[0].occurrence, &altered),
                Err(NativeBudgetTraceError::Observation)
            ));
        }
        assert_eq!(checked.remaining(), 1);
        checked
            .consume(&rows[0].occurrence, &rows[0].observation)
            .unwrap();
        assert_eq!(checked.finish().unwrap(), 1);
    });
}

// ---- Added by G3 (DR-135, S4): the owner part of the fold (R4, R8, SA8).
// Each attempt introduces one datum whose region belongs to an owner at
// price 2. The introduction weight is 1, so an attempt of `bytes` bytes uses
// `bytes` phlo and draws `2 * bytes` from the owner's purse.

/// Added by G3 (DR-135, S4).
mod owner_fold {
    use super::*;
    // Changed by G3 (DR-135, S11): the mixed attempts sort their regions.
    // use crate::rust::interpreter::accounting::authority::{
    //     cost_signature_to_sig, priced_cost_region,
    // };
    use crate::rust::interpreter::accounting::authority::{
        canonical_regions, cost_signature_to_sig, priced_cost_region,
    };
    use crate::rust::interpreter::accounting::native_phlo_rules::OwnerFoldInput;
    use crate::rust::interpreter::accounting::owner_funding::{
        OwnerFundingBinding, OwnerPurse, OwnerPurseSet, OwnerTable, OwnerTableEntry,
    };

    const OWNER_PRICE: u64 = 2;

    fn owner() -> CostSignature {
        CostSignature {
            value: Some(Value::Ground(vec![0xB1])),
        }
    }

    fn custody(signature: &CostSignature) -> [u8; 32] {
        cost_signature_to_sig(signature)
            .expect("a canonical signature")
            .lane_hash()
    }

    fn locate(signature: &CostSignature) -> Option<OwnerPurseSet> {
        let custody = cost_signature_to_sig(signature).ok()?.lane_hash();
        Some(OwnerPurseSet::Single(OwnerPurse {
            custody,
            signature: signature.clone(),
        }))
    }

    /// A binding with no client lane whose table lists the owner with
    /// `budget`, or lists nothing, with `room`.
    fn binding(budget: Option<u64>, room: usize) -> OwnerFundingBinding {
        let entries = budget
            .map(|budget| {
                (custody(&owner()), OwnerTableEntry {
                    signature: owner(),
                    budget,
                })
            })
            .into_iter()
            .collect();
        OwnerFundingBinding::new(
            Default::default(),
            locate,
            OwnerTable::new(entries, room).expect("a table within its room"),
        )
    }

    /// An introduction (or a COMM) of `bytes` bytes in a region of the owner.
    fn owned_attempt(
        id: u64,
        bytes: u64,
        granted: bool,
        owner_refused: bool,
    ) -> NativeBudgetAttempt {
        let region = priced_cost_region(&owner(), &id.to_be_bytes(), 0, OWNER_PRICE)
            .expect("a priced owner region");
        let mut attempt = attempt(id, bytes, granted);
        let observation = Arc::make_mut(&mut attempt.observation);
        observation.authority = CostAuthority {
            regions: vec![region],
        };
        attempt.owner_refused = owner_refused;
        attempt
    }

    /// Added by G3 (DR-135, S11): the signer of the client regions of the
    /// mixed attempts.
    fn client() -> CostSignature {
        CostSignature {
            value: Some(Value::Ground(vec![0xC1])),
        }
    }

    /// Added by G3 (DR-135, S11): `binding` with the lane of `client` as its
    /// client lane.
    fn client_binding(budget: Option<u64>, room: usize) -> OwnerFundingBinding {
        OwnerFundingBinding::new(
            [custody(&client())].into_iter().collect(),
            locate,
            binding(budget, room).table().clone(),
        )
    }

    /// Added by G3 (DR-135, S11): an introduction of `bytes` bytes with a
    /// region of the owner at the owner price and a region of `client`. Each
    /// region uses `bytes`, so the attempt uses `2 * bytes`, and its client
    /// part is `bytes`.
    fn mixed_attempt(id: u64, bytes: u64, granted: bool) -> NativeBudgetAttempt {
        let owner_region = priced_cost_region(&owner(), &id.to_be_bytes(), 0, OWNER_PRICE)
            .expect("a priced owner region");
        let client_region = priced_cost_region(&client(), &id.to_be_bytes(), 1, OWNER_PRICE)
            .expect("a priced client region");
        let mut attempt = attempt(id, bytes, granted);
        let observation = Arc::make_mut(&mut attempt.observation);
        observation.authority = canonical_regions([owner_region, client_region].iter())
            .expect("a canonical mixed authority")
            .into_authority();
        attempt
    }

    fn fold(
        contract: &NativePhloExecutionContract<'_>,
        rows: Vec<NativeBudgetAttempt>,
        binding: Option<&OwnerFundingBinding>,
    ) -> Result<CheckedNativeBudgetEvidence, NativeBudgetTraceError> {
        contract.check_budget_evidence(
            [1; 32],
            rows.into(),
            trace_limits(),
            binding.map(|binding| OwnerFoldInput {
                binding,
                table: binding.table().clone(),
                // Changed by G3 (DR-135, S4 review): the fold keeps no restoration map.
                // restorations: Default::default(),
            }),
        )
    }

    /// Added by G3 (DR-135, S4): a grant must fit the remaining budget of
    /// its owner under the strict rule. The fold rejects a grant past the
    /// budget, and accepts the same attempt as an owner refusal. The fold
    /// keeps the budget of the owner before each attempt.
    #[test]
    fn fold_rejects_a_grant_beyond_budget() {
        with_contract([0, 1, 0, 0], 100, 100, OWNER_PRICE, |contract| {
            let listed = binding(Some(10), 1);
            let first = owned_attempt(0, 4, true, false);
            let evidence = fold(&contract, vec![first.clone()], Some(&listed))
                .expect("the first grant draws 8 of 10");
            let owner_fold = evidence.owner().expect("an owner fold");
            assert_eq!(owner_fold.remaining_at(&custody(&owner()), 0), Some(10));
            assert_eq!(owner_fold.remaining_at(&custody(&owner()), 1), Some(2));
            assert_eq!(owner_fold.ledger().debits().get(&custody(&owner())), 8);
            assert_eq!(
                (owner_fold.ledger().usage(), owner_fold.ledger().rev()),
                (4, 8)
            );
            assert!(matches!(
                fold(
                    &contract,
                    vec![first.clone(), owned_attempt(1, 2, true, false)],
                    Some(&listed)
                ),
                Err(NativeBudgetTraceError::Decision)
            ));
            fold(
                &contract,
                vec![first.clone(), owned_attempt(1, 2, false, true)],
                Some(&listed),
            )
            .expect("the attempt past the budget is an owner refusal");
            // A bound denial also needs the owner allocation to succeed.
            // Changed by G3 (DR-135, S11): the bound counts no owner share at
            // a price of 1 or more, so the denied attempt is mixed, and its
            // client part exceeds the bound (item 13). The replaced lines
            // follow.
            // assert!(matches!(
            //     fold(
            //         &contract,
            //         vec![first, owned_attempt(1, 97, false, false)],
            //         Some(&listed)
            //     ),
            //     Err(NativeBudgetTraceError::Decision)
            // ));
            assert!(matches!(
                fold(
                    &contract,
                    vec![first, mixed_attempt(1, 101, false)],
                    Some(&client_binding(Some(10), 1))
                ),
                Err(NativeBudgetTraceError::Decision)
            ));
            // An unlisted purse has budget 0 for a grant.
            assert!(matches!(
                fold(
                    &contract,
                    vec![owned_attempt(0, 1, true, false)],
                    Some(&binding(None, 1))
                ),
                Err(NativeBudgetTraceError::Decision)
            ));
        });
    }

    /// Added by G3 (DR-135, S11): the fold counts no owner share at a price
    /// of 1 or more against the bound (item 13). Under the bound 4, two
    /// introductions of 4 that only the owner pays are granted. The total is
    /// 8, and the client total is 0. A recorded bound denial of such an
    /// introduction is forged.
    #[test]
    fn the_fold_never_counts_an_owner_share_against_the_bound() {
        with_contract([0, 1, 0, 0], 4, 4, OWNER_PRICE, |contract| {
            let listed = binding(Some(100), 1);
            let evidence = fold(
                &contract,
                vec![
                    owned_attempt(0, 4, true, false),
                    owned_attempt(1, 4, true, false),
                ],
                Some(&listed),
            )
            .expect("the owner pays both introductions");
            assert_eq!((evidence.total(), evidence.client_total()), (8, 0));
            assert!(
                matches!(
                    fold(
                        &contract,
                        vec![
                            owned_attempt(0, 4, true, false),
                            owned_attempt(1, 4, false, false)
                        ],
                        Some(&listed)
                    ),
                    Err(NativeBudgetTraceError::Decision)
                ),
                "a bound denial of an introduction that only the owner pays is forged"
            );
        });
    }

    /// Added by G3 (DR-135, S11): a mixed introduction counts only its client
    /// region against the bound (item 13). Under the bound 4, a mixed
    /// introduction of 4 uses 8 and counts 4. A second one exceeds the bound,
    /// so the fold rejects its grant and accepts its denial. A client part of
    /// 5 exceeds the bound alone.
    #[test]
    fn a_mixed_attempt_counts_only_its_client_regions() {
        with_contract([0, 1, 0, 0], 4, 4, OWNER_PRICE, |contract| {
            let listed = client_binding(Some(100), 1);
            let evidence = fold(
                &contract,
                vec![mixed_attempt(0, 4, true), mixed_attempt(1, 4, false)],
                Some(&listed),
            )
            .expect("the second client part exceeds the bound");
            assert_eq!((evidence.total(), evidence.client_total()), (8, 4));
            assert!(matches!(
                fold(
                    &contract,
                    vec![mixed_attempt(0, 4, true), mixed_attempt(1, 4, true)],
                    Some(&listed)
                ),
                Err(NativeBudgetTraceError::Decision)
            ));
            assert!(
                matches!(
                    fold(&contract, vec![mixed_attempt(0, 5, true)], Some(&listed)),
                    Err(NativeBudgetTraceError::Decision)
                ),
                "a client part of 5 exceeds the bound 4"
            );
        });
    }

    /// Added by G3 (DR-135, S4): an owner refusal needs a listed shortfall
    /// under the lenient rule (SA8). The fold rejects a refusal that the
    /// budget covers, a refusal of an unlisted purse while the table has
    /// room, a granted refusal, a refused COMM attempt, and any refusal
    /// without owner funding. With a full table an unlisted purse has budget
    /// 0, so its refusal stands.
    #[test]
    fn fold_rejects_an_owner_refusal_without_shortfall() {
        with_contract([0, 1, 0, 0], 100, 100, OWNER_PRICE, |contract| {
            let refusal = owned_attempt(0, 4, false, true);
            assert!(matches!(
                fold(&contract, vec![refusal.clone()], Some(&binding(Some(8), 1))),
                Err(NativeBudgetTraceError::Decision)
            ));
            fold(&contract, vec![refusal.clone()], Some(&binding(Some(7), 1)))
                .expect("a budget of 7 cannot pay 8");
            assert!(matches!(
                fold(&contract, vec![refusal.clone()], Some(&binding(None, 1))),
                Err(NativeBudgetTraceError::Decision)
            ));
            fold(&contract, vec![refusal.clone()], Some(&binding(None, 0)))
                .expect("a full table gives an unlisted purse 0");
            assert!(matches!(
                fold(
                    &contract,
                    vec![owned_attempt(0, 4, true, true)],
                    Some(&binding(Some(7), 1))
                ),
                Err(NativeBudgetTraceError::Decision)
            ));
            let mut comm = refusal.clone();
            comm.occurrence.stage = NativeAttemptStage::Comm;
            Arc::make_mut(&mut comm.observation).kind = AuthorityByteEventKind::Comm;
            assert!(matches!(
                fold(&contract, vec![comm], Some(&binding(Some(7), 1))),
                Err(NativeBudgetTraceError::Decision)
            ));
            assert!(matches!(
                fold(&contract, vec![refusal], None),
                Err(NativeBudgetTraceError::Decision)
            ));
        });
    }
}
