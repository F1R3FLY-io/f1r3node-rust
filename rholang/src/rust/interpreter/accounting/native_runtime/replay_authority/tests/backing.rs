use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUnits};

use super::*;

fn configured(memory: u64) -> (RuntimeBudget, HostWorkBudget) {
    let mut configuration = config(1_000_000, [0, 1, 1, 0]);
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
    limits.set(
        HostWorkDimension::SearchStateBytes,
        HostWorkLimit::new(memory),
    );
    configuration.host_work = HostWorkBudget::new(limits);
    let host = configuration.host_work();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.reset_for_native_execution(configuration).unwrap();
    (budget, host)
}

#[test]
fn unpaid_authority_preparation_cannot_publish_any_state() {
    let (budget, host) = configured(0);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    assert!(matches!(
        binding.prepare([None, Some(row(0, 3, true, false))]),
        Err(InterpreterError::HostWorkRejected)
    ));
    let state = budget.authority_state.lock().unwrap();
    assert!(state.events.is_empty());
    assert!(state.reserved.0.is_empty());
    assert!(state.realized.0.is_empty());
    assert!(state.pending_replay_events.is_empty());
    assert_eq!(state.pending_replay_rows, 0);
    assert!(state.byte_observations.rows().is_empty());
    assert!(host.is_rejected());
}

#[test]
fn retry_comparison_exhaustion_preserves_published_authority() {
    let (baseline_budget, baseline_host) = configured(100_000_000);
    let baseline_binding =
        ReplayAuthorityBinding::new(baseline_budget.clone(), baseline_budget.deploy_id()).unwrap();
    let _baseline_scope = baseline_budget.enter_comm_accounting_scope();
    baseline_binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    let owner_work = baseline_host
        .usage(HostWorkDimension::VerificationOperations)
        .get();

    let mut configuration = config(1_000_000, [0, 1, 1, 0]);
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
    let lookup_work = (u64::from(usize::BITS) + 1) * 11 * 3;
    limits.set(
        HostWorkDimension::VerificationOperations,
        HostWorkLimit::new(owner_work + lookup_work),
    );
    configuration.host_work = HostWorkBudget::new(limits);
    let host = configuration.host_work();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.reset_for_native_execution(configuration).unwrap();
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    let realized = budget.authority_realized();
    assert!(matches!(
        binding.prepare([None, Some(row(0, 3, true, true))]),
        Err(InterpreterError::HostWorkRejected)
    ));
    assert!(host.is_rejected());
    assert_eq!(budget.authority_realized(), realized);
    assert_eq!(budget.authority_events().len(), 1);
    assert_eq!(budget.byte_observations().rows.len(), 1);
    assert!(budget
        .authority_state
        .lock()
        .unwrap()
        .pending_replay_events
        .is_empty());
}

#[test]
fn prepaid_publication_and_cancellation_survive_later_host_rejection() {
    let (budget, host) = configured(100_000_000);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let mut kept = binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap();
    let cancelled = binding
        .prepare([None, Some(row(1, 3, true, false))])
        .unwrap();
    assert!(host
        .reserve(
            HostWorkDimension::SearchStateBytes,
            HostWorkUnits::new(u64::MAX)
        )
        .is_err());
    let before = host.usages();
    kept.publish();
    drop(cancelled);
    assert_eq!(before, host.usages());
    let expected = authority::authority_demand(&authority(3)).unwrap();
    let state = budget.authority_state.lock().unwrap();
    assert_eq!(state.realized, expected);
    assert_eq!(state.reserved, expected);
    assert_eq!(state.events.len(), 1);
    assert_eq!(state.byte_observations.rows().len(), 1);
    assert!(state.pending_replay_events.is_empty());
    assert_eq!(state.pending_replay_rows, 0);
}

fn populated(memory: u64) -> (RuntimeBudget, HostWorkBudget) {
    let (budget, host) = configured(memory);
    let binding = ReplayAuthorityBinding::new(budget.clone(), budget.deploy_id()).unwrap();
    let scope = budget.enter_comm_accounting_scope();
    binding
        .prepare([None, Some(row(0, 3, true, false))])
        .unwrap()
        .publish();
    drop(scope);
    (budget, host)
}

#[test]
fn checkpoint_and_result_reject_before_unpaid_copies_and_preserve_live_state() {
    let (_, baseline) = populated(100_000_000);
    let exhausted_limit = baseline.usage(HostWorkDimension::SearchStateBytes).get();
    for checkpoint in [true, false] {
        let (budget, host) = populated(exhausted_limit);
        let before = budget.authority_events();
        let realized = budget.authority_realized();
        let result = if checkpoint {
            budget.native_authority_checkpoint().map(|_| ())
        } else {
            budget.reserve_native_result_backing()
        };
        assert!(matches!(result, Err(InterpreterError::HostWorkRejected)));
        assert!(host.is_rejected());
        assert_eq!(budget.authority_events(), before);
        assert_eq!(budget.authority_realized(), realized);
        assert_eq!(budget.byte_observations().rows.len(), 1);
    }
}

#[test]
fn authority_checkpoint_prepays_cloned_state_cleanup() {
    let (budget, host) = populated(100_000_000);
    let before = host.usage(HostWorkDimension::VerificationOperations).get();
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    let prepaid = host.usage(HostWorkDimension::VerificationOperations).get() - before;
    let before = host.usage(HostWorkDimension::VerificationOperations).get();
    let introductions = budget.introduction_authorities.lock().unwrap();
    let state = budget.authority_state.lock().unwrap();
    clone_backing::reserve(&state.events, &host).unwrap();
    clone_backing::reserve_slice(state.byte_observations.rows(), &host).unwrap();
    clone_backing::reserve(&state.realized, &host).unwrap();
    clone_backing::reserve(&state.reserved, &host).unwrap();
    clone_backing::reserve(&state.frontier, &host).unwrap();
    clone_backing::reserve(&state.stack_births, &host).unwrap();
    clone_backing::reserve(&*introductions, &host).unwrap();
    let clone_only = host.usage(HostWorkDimension::VerificationOperations).get() - before;
    assert!(clone_only > 0);
    assert!(prepaid >= clone_only * 2);
    drop((state, introductions, checkpoint));
}

#[test]
fn restored_observation_capacity_must_be_paid_again_before_growth() {
    let (budget, host) = populated(100_000_000);
    let checkpoint = budget.native_authority_checkpoint().unwrap();
    budget.restore_native_authority(checkpoint).unwrap();
    let mut state = budget.authority_state.lock().unwrap();
    let before = host.usage(HostWorkDimension::SearchStateBytes).get();
    state.byte_observations.reserve_native(1, &host).unwrap();
    let after = host.usage(HostWorkDimension::SearchStateBytes).get();
    assert!(after > before);
    assert_eq!(state.byte_observations.rows().len(), 1);
}
