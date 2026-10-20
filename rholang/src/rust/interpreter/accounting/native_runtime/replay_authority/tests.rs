use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::native_runtime::tests::{authority, config};

mod sparse_ledger;

fn budget() -> RuntimeBudget {
    budget_for_epoch(EventCodecEpoch::Legacy)
}

fn budget_for_epoch(epoch: EventCodecEpoch) -> RuntimeBudget {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(
            config(1_000_000, [0, 1, 1, 0])
                .with_event_codec_epoch(epoch),
        )
        .expect("valid native configuration");
    budget
}

fn execution_epochs() -> [EventCodecEpoch; 3] {
    [EventCodecEpoch::Legacy, EventCodecEpoch::CostV1, EventCodecEpoch::CostV2]
}

fn row(id: u8, owners: usize, granted: bool, retry: bool) -> ReplayAuthorityObservation {
    ReplayAuthorityObservation {
        observation: Arc::new(ByteObservation {
            event_id: [id; 32],
            kind: AuthorityByteEventKind::Comm,
            authority: authority(owners),
            measurement: Some(ByteCharge::default()),
            legacy_amount: None,
        }),
        granted,
        retry,
    }
}

#[test]
fn replay_authority_checkpoint_preserves_the_codec_epoch() {
    for epoch in execution_epochs() {
        let budget = budget_for_epoch(epoch);
        let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id())
            .expect("fresh runtime binding");
        let checkpoint = budget.native_authority_checkpoint().expect("idle checkpoint");
        let scope = budget.enter_comm_accounting_scope();
        binding.prepare([None, Some(row(1, 2, true, false))])
            .expect("valid publication").publish();
        assert_eq!(budget.authority_events().len(), 1);
        assert_eq!(budget.byte_observations().rows.len(), 1);
        drop(scope);
        budget.restore_native_authority(checkpoint).expect("same-generation restore");
        assert_eq!(budget.native_event_codec_epoch(), Some(epoch));
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_realized().0.is_empty());
        assert!(budget.byte_observations().rows.is_empty());
        let _scope = budget.enter_comm_accounting_scope();
        binding.prepare([None, Some(row(2, 3, true, false))])
            .expect("restored binding remains valid").publish();
        assert_eq!(budget.native_event_codec_epoch(), Some(epoch));
        assert_eq!(budget.authority_events().len(), 1);
        assert_eq!(budget.authority_realized(), authority::authority_demand(&authority(3))
            .expect("valid authority demand"));
    }
}

#[test]
fn replay_authority_reset_invalidates_all_prior_context_checkpoints_and_bindings() {
    for old_epoch in execution_epochs() {
        for new_epoch in execution_epochs() {
            let budget = budget_for_epoch(old_epoch);
            let old_binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id())
                .expect("fresh old binding");
            let scope = budget.enter_comm_accounting_scope();
            old_binding.prepare([None, Some(row(1, 2, true, false))])
                .expect("valid old publication").publish();
            drop(scope);
            let old_checkpoint = budget.native_authority_checkpoint().expect("old checkpoint");
            budget.reset_for_native_execution(
                config(1_000_000, [0, 1, 1, 0])
                    .with_event_codec_epoch(new_epoch),
            ).expect("idle context replacement");
            assert_eq!(budget.native_event_codec_epoch(), Some(new_epoch));
            let current_binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id())
                .expect("fresh replacement binding");
            let scope = budget.enter_comm_accounting_scope();
            current_binding.prepare([None, Some(row(2, 1, true, false))])
                .expect("valid replacement publication").publish();
            drop(scope);
            let before = (
                budget.native_event_codec_epoch(),
                budget.authority_realized(),
                budget.byte_observations(),
            );
            assert!(budget.restore_native_authority(old_checkpoint).is_err());
            assert_eq!(
                (budget.native_event_codec_epoch(), budget.authority_realized(), budget.byte_observations()),
                before
            );
            let _scope = budget.enter_comm_accounting_scope();
            let before_attempt = budget.byte_observations();
            assert!(old_binding.prepare([None, Some(row(3, 3, true, false))]).is_err());
            assert_eq!(budget.byte_observations(), before_attempt);
            current_binding.prepare([None, Some(row(3, 1, true, false))])
                .expect("replacement binding remains valid").publish();
            assert_eq!(budget.authority_events().len(), 2);
            assert_eq!(budget.native_event_codec_epoch(), Some(new_epoch));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn replay_authority_publication_and_cancellation_preserve_each_purse(
        owners in 1_usize..33,
        decisions in prop::collection::vec((any::<bool>(), any::<bool>()), 1..33),
        reverse in any::<bool>(),
    ) {
        let budget = budget();
        let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
        let checkpoint = budget.native_authority_checkpoint().unwrap();
        let scope = budget.enter_comm_accounting_scope();
        let demand = authority::authority_demand(&authority(owners)).unwrap();
        let mut pending = Vec::new();
        let mut granted_count = 0;
        for (id, (granted, publish)) in decisions.iter().enumerate() {
            pending.push((binding.prepare([None, Some(row(id as u8, owners, *granted, false))]).unwrap(), *granted, *publish));
            granted_count += usize::from(*granted);
        }
        prop_assert!(budget.authority_events().is_empty());
        for (purse, amount) in &demand.0 {
            prop_assert_eq!(budget.authority_state.lock().unwrap().reserved.get(purse), *amount * granted_count as u64);
        }
        if reverse { pending.reverse(); }
        let mut published = 0;
        for (mut reservation, granted, publish) in pending {
            if publish {
                reservation.publish();
                published += usize::from(granted);
            }
            drop(reservation);
        }
        prop_assert_eq!(budget.authority_events().len(), published);
        prop_assert_eq!(budget.byte_observations().rows.len(), published);
        let state = budget.authority_state.lock().unwrap();
        for (purse, amount) in &demand.0 {
            prop_assert_eq!(state.realized.get(purse), *amount * published as u64);
            prop_assert_eq!(state.reserved.get(purse), *amount * published as u64);
        }
        prop_assert_eq!(state.pending_replay_rows, 0);
        prop_assert!(state.pending_replay_events.is_empty());
        drop(state);
        drop(scope);
        budget.restore_native_authority(checkpoint).unwrap();
        prop_assert!(budget.authority_events().is_empty());
        prop_assert!(budget.authority_realized().0.is_empty());
        prop_assert!(budget.authority_frontier().is_empty());
        prop_assert!(budget.byte_observations().rows.is_empty());
    }
}

#[test]
fn replay_authority_retries_require_a_published_identical_owner() {
    let budget = budget();
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    assert!(ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).is_err());
    assert!(budget.native_budget_recording().is_err());
    assert_eq!(budget.native_phlo_usage(), None);
    let _scope = budget.enter_comm_accounting_scope();
    assert!(binding
        .prepare([None, Some(row(1, 2, true, true))])
        .is_err());
    let mut owner = binding
        .prepare([None, Some(row(1, 2, true, false))])
        .unwrap();
    assert!(binding
        .prepare([None, Some(row(1, 2, true, true))])
        .is_err());
    owner.publish();
    let before = budget.authority_realized();
    assert!(binding
        .prepare([None, Some(row(1, 3, true, true))])
        .is_err());
    assert!(binding
        .prepare([None, Some(row(1, 2, true, false))])
        .is_err());
    binding
        .prepare([None, Some(row(1, 2, true, true))])
        .unwrap()
        .publish();
    assert_eq!(budget.authority_realized(), before);
    assert_eq!(budget.authority_events().len(), 1);
    assert_eq!(budget.byte_observations().rows.len(), 1);
}

#[test]
fn replay_authority_cannot_overdraw_pending_stack_capacity() {
    let budget = budget();
    let demand = authority::authority_demand(&authority(2)).unwrap();
    budget.install_authority_allocation(demand.clone());
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let cells = vec![authority(1).regions[0].signature.clone().unwrap()];
    let stack = budget
        .prepare_authority_stack_transfer([42; 32], cells, &authority(2))
        .unwrap();
    assert!(binding
        .prepare([None, Some(row(1, 2, true, false))])
        .is_err());
    assert_eq!(budget.authority_state.lock().unwrap().reserved, demand);
    drop(stack);
    binding
        .prepare([None, Some(row(1, 2, true, false))])
        .unwrap()
        .publish();
    assert_eq!(budget.authority_realized(), demand);
}

#[test]
fn replay_authority_rejects_foreign_reset_and_active_checkpoint() {
    let budget = budget();
    assert!(budget.has_exclusive_authority_owner());
    let retained = budget.clone();
    assert!(!budget.has_exclusive_authority_owner());
    drop(retained);
    assert!(budget.has_exclusive_authority_owner());
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    let scope = budget.enter_comm_accounting_scope();
    assert!(budget.native_authority_checkpoint().is_err());
    drop(scope);
    budget
        .reset_for_native_execution(config(1_000_000, [0, 1, 1, 0]))
        .unwrap();
    assert!(budget.restore_native_authority(checkpoint).is_err());
    let _scope = budget.enter_comm_accounting_scope();
    assert!(binding
        .prepare([None, Some(row(1, 1, true, false))])
        .is_err());
    assert!(budget.authority_events().is_empty());
}
