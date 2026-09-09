use super::*;

async fn check_classification(
    active: bool,
    persistent: bool,
    occupied: bool,
    now: u64,
    deadline: Option<u64>,
    attempts: u32,
) {
    let owners = owners(1, 2).await;
    let mut initial = policy();
    initial.retry_budget_quarantine_until = deadline;
    initial.retry_attempts = attempts;
    if persistent {
        owners
            .publish_pending_for(
                hash(1),
                initial.clone(),
                |_| (),
                HashSet::new(),
                HashSet::new(),
            )
            .unwrap();
    }
    let held = if active {
        owners.activate(hash(1), initial.clone(), |_| ()).unwrap()
    } else {
        None
    };
    if occupied && !active {
        owners.activate(hash(2), policy(), |_| ()).unwrap().unwrap();
    }
    let active_before = owners.active_count();
    let stored_before = owners
        .buffer()
        .pending_request_policy(&BlockHashSerde(hash(1)))
        .unwrap();
    let result = owners
        .activate_local(hash(1), initial.clone(), now, |_| ())
        .unwrap();
    let expected = if active {
        "active"
    } else if deadline.is_some_and(|until| now < until) {
        "quarantine"
    } else if occupied {
        "capacity"
    } else {
        "active"
    };
    let actual = match result {
        request_ownership::Activation::Active(owner) => {
            assert_eq!(owner.policy(), initial);
            if let Some(held) = held {
                assert!(Arc::ptr_eq(&held, &owner));
            }
            assert_eq!(owners.active_count(), 1);
            "active"
        }
        request_ownership::Activation::Ineligible => "quarantine",
        request_ownership::Activation::AtCapacity => "capacity",
    };
    assert_eq!(actual, expected);
    if expected != "active" {
        assert_eq!(owners.active_count(), active_before);
    }
    assert_eq!(owners.available_operations(), 2);
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap(),
        stored_before
    );
}

#[tokio::test]
async fn local_activation_preserves_precedence_at_deadline_boundaries() {
    for active in [false, true] {
        for persistent in [false, true] {
            for occupied in [false, true] {
                for (now, deadline) in [
                    (0, None),
                    (0, Some(1)),
                    (1, Some(1)),
                    (2, Some(1)),
                    (u64::MAX, Some(u64::MAX)),
                ] {
                    check_classification(active, persistent, occupied, now, deadline, 32).await;
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn local_activation_matches_captured_policy_and_capacity(
        active in any::<bool>(), persistent in any::<bool>(), occupied in any::<bool>(),
        now in any::<u64>(), deadline in prop::option::of(any::<u64>()), attempts in any::<u32>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(check_classification(active, persistent, occupied, now, deadline, attempts));
    }
}
