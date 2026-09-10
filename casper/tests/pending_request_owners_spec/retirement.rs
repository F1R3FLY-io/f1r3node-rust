use super::*;

async fn retired(pending: bool) -> (RequestOwners<()>, Arc<request_ownership::RequestOwner<()>>) {
    let owners = owners(1, 2).await;
    let mut initial = policy();
    initial.retry_attempts = 2;
    let owner = owners
        .activate(hash(1), initial, |_| ())
        .expect("activate")
        .expect("owner");
    if pending {
        owners
            .publish_pending(&owner, HashSet::new(), HashSet::new())
            .expect("publish");
        owners
            .activate(hash(1), policy(), |_| ())
            .expect("reactivate")
            .expect("owner");
    }
    assert!(owners
        .reserve_retry(&owner, false, 2, 5, 10)
        .expect("retire")
        .is_none());
    assert_eq!(owners.active_count(), 0);
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("budget"),
        (2, Some(15))
    );
    (owners, owner)
}

#[tokio::test]
async fn final_completion_and_retirement_preserve_the_last_charge() {
    for persistent in [false, true] {
        let (owners, owner, operation) = last_peer_operation(persistent).await;
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            let completion = scope.spawn(|| {
                barrier.wait();
                operation.complete_action().expect("completion");
            });
            let retirement = scope.spawn(|| {
                barrier.wait();
                assert!(owners
                    .reserve_retry(&owner, false, 32, 7, 10)
                    .unwrap()
                    .is_none());
            });
            completion.join().unwrap();
            retirement.join().unwrap();
        });
        assert_eq!(owners.retry_budget(&hash(1)).unwrap().0, 32);
        assert_eq!(owners.available_operations(), 2);
        assert!(owners
            .reserve_retry(&owner, false, 32, 8, 10)
            .unwrap()
            .is_none());
        assert_eq!(owners.active_count(), 0);
        assert_eq!(owners.retry_budget(&hash(1)).unwrap().0, 32);
        owners.renew_expired(18).await.expect("renew");
        assert_eq!(owners.retry_budget(&hash(1)).unwrap(), (0, None));
        assert_eq!(owners.deferred_count(), 0);
        if persistent {
            assert!(
                owners
                    .lookup(&hash(1), |_| ())
                    .unwrap()
                    .unwrap()
                    .policy()
                    .requested_as_dependency
            );
        }
    }
}

#[tokio::test]
async fn deferred_final_completion_survives_retirement_and_renewal_without_recharge() {
    for drain_before_renewal in [false, true] {
        let (owners, owner, operation) = last_peer_operation(true).await;
        let committed = owner.policy();
        owners.buffer().remove(BlockHashSerde(hash(1))).unwrap();
        assert!(operation.complete_action().is_err());
        assert_eq!(owner.policy().retry_attempts, 32);
        assert_eq!(owners.deferred_count(), 1);
        assert_eq!(owners.available_operations(), 1);
        owners
            .buffer()
            .publish_pending_request(
                BlockHashSerde(hash(1)),
                HashSet::new(),
                HashSet::new(),
                None,
                &committed,
            )
            .expect("restore confirmed predecessor");
        assert!(owners
            .reserve_retry(&owner, false, 32, 7, 10)
            .unwrap()
            .is_none());
        assert_eq!(owners.active_count(), 0);
        let retired = owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap()
            .unwrap();
        assert_eq!(retired.retry_attempts, 32);
        assert_eq!(retired.retry_budget_quarantine_until, Some(17));
        if drain_before_renewal {
            assert_eq!(owners.drain_completions(2).unwrap(), 1);
        }
        owners.renew_expired(17).await.expect("renew");
        assert_eq!(
            owners.drain_completions(2).unwrap(),
            usize::from(!drain_before_renewal)
        );
        assert_eq!(owners.drain_completions(2).unwrap(), 0);
        assert_eq!(owners.available_operations(), 2);
        assert_eq!(owners.deferred_count(), 0);
        let actual = owner.policy();
        assert_eq!(
            (actual.retry_attempts, actual.retry_budget_quarantine_until),
            (0, None)
        );
        assert!(actual.requested_as_dependency);
        assert_eq!(
            owners
                .buffer()
                .pending_request_policy(&BlockHashSerde(hash(1)))
                .unwrap(),
            Some(actual)
        );
    }
}

#[tokio::test]
async fn failed_retired_budget_publication_preserves_budget_and_competing_row() {
    for captured in [false, true] {
        let (owners, old) = retired(false).await;
        let before = owners.retry_budget(&hash(1)).expect("retired budget");
        let mut initial = policy();
        initial.requested_as_dependency = captured;
        let mut competing = policy();
        competing.requested_as_dependency = false;
        competing.retry_attempts = before.0;
        competing.retry_budget_quarantine_until = before.1;
        let result = owners.publish_pending_for(
            hash(1),
            initial.clone(),
            |pending| {
                assert!(!pending);
                owners
                    .buffer()
                    .publish_pending_request(
                        BlockHashSerde(hash(1)),
                        HashSet::new(),
                        HashSet::new(),
                        None,
                        &competing,
                    )
                    .expect("competing transaction");
            },
            HashSet::new(),
            HashSet::new(),
        );
        assert!(matches!(
            result,
            Err(shared::rust::store::key_value_store::KvStoreError::TransactionConflict(_))
        ));
        assert_eq!(owners.retry_budget(&hash(1)).unwrap(), before);
        assert_eq!(owners.active_count(), 0);
        assert_eq!(owners.available_operations(), 2);
        assert_eq!(
            owners
                .buffer()
                .pending_request_policy(&BlockHashSerde(hash(1)))
                .unwrap(),
            Some(competing)
        );
        assert!(!old.policy().requested_as_dependency);
        owners.buffer().remove(BlockHashSerde(hash(1))).unwrap();
        owners
            .publish_pending_for(hash(1), initial, |_| (), HashSet::new(), HashSet::new())
            .expect("retry transfer");
        let pending = owners.lookup(&hash(1), |_| ()).unwrap().unwrap().policy();
        assert_eq!(
            (
                pending.retry_attempts,
                pending.retry_budget_quarantine_until
            ),
            before
        );
        assert_eq!(pending.requested_as_dependency, captured);
    }
}

#[tokio::test]
async fn retained_ordinary_handle_cannot_resurrect_retired_provenance() {
    let (owners, old) = retired(false).await;
    assert!(owners.lookup(&hash(1), |_| ()).expect("lookup").is_none());
    assert!(!old.policy().requested_as_dependency);
    assert!(owners
        .update_policy(&old, |policy, _| policy.requested_as_dependency = true)
        .is_err());
    assert!(owners
        .publish_pending(&old, HashSet::new(), HashSet::new())
        .is_err());
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("budget"),
        (2, Some(15))
    );
    let mut initial = policy();
    initial.requested_as_dependency = false;
    owners
        .publish_pending_for(hash(1), initial, |_| (), HashSet::new(), HashSet::new())
        .expect("transfer");
    let pending = owners
        .lookup(&hash(1), |_| ())
        .expect("lookup")
        .expect("pending");
    assert!(!pending.policy().requested_as_dependency);
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("budget"),
        (2, Some(15))
    );
    owners.terminate(&old).expect("obsolete terminal cleanup");
    assert_eq!(
        owners
            .lookup(&hash(1), |_| ())
            .expect("lookup")
            .expect("pending")
            .policy(),
        pending.policy()
    );
}

#[tokio::test]
async fn local_receipt_retains_active_quarantine_without_recreating_retired_transport() {
    let (owners, _) = retired(false).await;
    assert!(matches!(
        owners
            .activate_local(hash(1), policy(), 7, |_| ())
            .expect("retired receipt"),
        request_ownership::Activation::Ineligible
    ));
    let mut initial = policy();
    initial.retry_budget_quarantine_until = Some(17);
    let active = owners
        .activate(hash(2), initial, |_| ())
        .expect("seed active")
        .expect("owner");
    let request_ownership::Activation::Active(retained) = owners
        .activate_local(hash(2), policy(), 7, |_| ())
        .expect("active receipt")
    else {
        panic!("active owner must remain available")
    };
    assert!(Arc::ptr_eq(&active, &retained));
    assert_eq!(retained.policy().retry_budget_quarantine_until, Some(17));
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("retired budget"),
        (2, Some(15))
    );
}

#[tokio::test]
async fn quarantine_and_capacity_refusal_preserve_retired_budget() {
    let (owners, _) = retired(false).await;
    assert!(owners
        .activate_eligible(hash(1), policy(), 14, None, |_| ())
        .expect("quarantine")
        .is_none());
    assert_eq!(owners.active_count(), 0);
    let other = owners
        .activate_eligible(hash(2), policy(), 15, None, |_| ())
        .expect("other")
        .expect("owner");
    assert!(owners
        .activate_eligible(hash(1), policy(), 15, None, |_| ())
        .expect("capacity")
        .is_none());
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("budget"),
        (2, Some(15))
    );
    owners.terminate(&other).expect("clear capacity");
    assert!(owners
        .activate_eligible(hash(1), policy(), 15, Some(2), |_| ())
        .expect("absent recovery")
        .is_none());
    assert_eq!(owners.active_count(), 0);
    let recited = owners
        .activate_eligible(hash(1), policy(), 15, None, |_| ())
        .expect("recite")
        .expect("owner");
    assert!(owners
        .activate_eligible(hash(1), policy(), 16, Some(2), |_| ())
        .expect("existing recovery")
        .is_some());
    assert!(owners
        .reserve_retry(&recited, false, 2, 16, 10)
        .expect("new retirement")
        .is_none());
    assert_eq!(owners.active_count(), 0);
    assert_eq!(
        owners.retry_budget(&hash(1)).expect("new deadline"),
        (2, Some(26))
    );
    owners.forget(&hash(1)).expect("forget retired record");
    assert_eq!(owners.retry_budget(&hash(1)).expect("forgotten"), (0, None));
}

#[tokio::test]
async fn recitation_before_sweep_preserves_new_schedule_fields() {
    for pending in [false, true] {
        let (owners, old) = retired(pending).await;
        let mut initial = policy();
        initial.initial_timestamp = 16;
        initial.last_request_timestamp = 17;
        initial.requested_as_dependency = false;
        let recited = owners
            .activate_eligible(hash(1), initial, 16, None, |_| ())
            .expect("recite")
            .expect("owner");
        owners
            .update_policy(&recited, |policy, _| {
                policy.peer_requery_cursor = 9;
                policy.last_request_timestamp = 17;
            })
            .expect("new schedule");
        let mut expected = recited.policy();
        expected.retry_attempts = 0;
        expected.retry_budget_quarantine_until = None;
        if pending {
            expected.revision += 1;
        }
        owners.renew_expired(15).await.expect("expiry");
        assert_eq!(recited.policy(), expected);
        assert_eq!(owners.active_count(), 1);
        if !pending {
            assert!(!Arc::ptr_eq(&old, &recited));
        }
    }
}

#[tokio::test]
async fn expiry_scans_cold_pending_rows_without_activating_transport() {
    let mut manager = InMemoryStoreManager::new();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .expect("buffer");
    for key in 1..=129 {
        let owner_set = RequestOwners::new(buffer.clone(), 1, 1, |_| ());
        let mut initial = policy();
        initial.retry_attempts = 2;
        initial.retry_budget_quarantine_until = Some(15);
        owner_set
            .publish_pending_for(hash(key), initial, |_| (), HashSet::new(), HashSet::new())
            .expect("pending");
    }
    let owners = RequestOwners::new(buffer, 1, 1, |_| ());
    owners.renew_expired(15).await.expect("full expiry pass");
    assert_eq!(owners.active_count(), 0);
    for key in 1..=129 {
        let policy = owners
            .lookup(&hash(key), |_| ())
            .expect("lookup")
            .expect("pending")
            .policy();
        assert_eq!(policy.retry_attempts, 0);
        assert_eq!(policy.retry_budget_quarantine_until, None);
        assert!(policy.requested_as_dependency);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn repeated_retirement_transfers_preserve_exact_budget(
        choices in prop::collection::vec((any::<bool>(), any::<bool>(), 1u32..9, 1u8..6), 1..25),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let owners = owners(1, 2).await;
            let mut durable = std::collections::HashMap::new();
            let prefix = [(true, true, 2, 1), (false, false, 3, 1), (true, false, 1, 1)];
            for (index, (publish, captured, budget, address)) in prefix.into_iter().chain(choices).enumerate() {
                let now = index as u64 * 100 + 5;
                let key = hash(address);
                let mut initial = policy();
                initial.requested_as_dependency = false;
                let owner = owners.activate_eligible(key.clone(), initial, now, None, |_| ()).expect("activate").expect("owner");
                for _ in 0..budget {
                    owners.reserve_retry(&owner, false, budget, now, 10).expect("reserve").expect("allowance")
                        .complete_action().expect("complete");
                }
                assert!(owners.reserve_retry(&owner, false, budget, now, 10).expect("retire").is_none());
                assert_eq!(owners.retry_budget(&key).expect("budget"), (budget, Some(now + 10)));
                if publish {
                    let mut supplied = policy();
                    supplied.requested_as_dependency = captured;
                    owners.publish_pending_for(key.clone(), supplied, |_| (), HashSet::new(), HashSet::new()).expect("transfer");
                    let expected = durable.get(&address).copied().unwrap_or(false) || captured;
                    durable.insert(address, expected);
                    assert_eq!(owners.lookup(&key, |_| ()).expect("lookup").expect("pending").policy().requested_as_dependency, expected);
                }
                owners.renew_expired(now + 9).await.expect("not due");
                assert_eq!(owners.retry_budget(&key).expect("preserved"), (budget, Some(now + 10)));
                owners.renew_expired(now + 10).await.expect("due");
                assert_eq!(owners.retry_budget(&key).expect("renewed"), (0, None));
                assert_eq!(owners.active_count(), 0);
                assert_eq!(owners.available_operations(), 2);
            }
        });
    }
}
