use std::collections::HashSet;
use std::sync::{Arc, Barrier};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::casperbuffer::pending_request_policy::PendingRequestPolicy;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use proptest::prelude::*;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

mod request_ownership {
    include!("../src/rust/engine/block_retriever/request_ownership.rs");

    impl<D> RequestOwners<D> {
        pub fn publish_pending(
            &self,
            owner: &Arc<RequestOwner<D>>,
            blocks: HashSet<BlockHashSerde>,
            certificates: HashSet<BlockHashSerde>,
        ) -> Result<(), KvStoreError> {
            self.ensure_owner(owner)?;
            let mut registry = self.registry.lock();
            self.publish_locked(&mut registry, owner, blocks, certificates, false)
        }

        pub fn reserve_retry(
            &self,
            owner: &Arc<RequestOwner<D>>,
            peer: bool,
            budget: u32,
            now: u64,
            quarantine_ms: u64,
        ) -> Result<Option<RetryOperation<D>>, KvStoreError>
        where
            D: Clone,
        {
            Ok(
                match self.reserve_retry_with(owner, budget, now, quarantine_ms, |_, _, _| {
                    RetrySelection::Dispatch(peer, ())
                })? {
                    PreparedRetry::Dispatch(operation, ()) => Some(operation),
                    _ => None,
                },
            )
        }
    }
}
use request_ownership::RequestOwners;

#[path = "pending_request_owners_spec/retirement.rs"]
mod retirement;

async fn last_peer_operation(
    persistent: bool,
) -> (
    RequestOwners<()>,
    Arc<request_ownership::RequestOwner<()>>,
    request_ownership::RetryOperation<()>,
) {
    let owners = owners(1, 2).await;
    let mut initial = policy();
    initial.retry_attempts = 31;
    initial.peer_requery_attempts = 2;
    initial.peer_requery_cursor = 7;
    initial.dependency_recovery_last_request = Some(3);
    initial.broadcast_retry_last_request = Some(4);
    initial.peer_requery_last_request = Some(5);
    let owner = owners.activate(hash(1), initial, |_| ()).unwrap().unwrap();
    if persistent {
        owners
            .publish_pending(&owner, HashSet::new(), HashSet::new())
            .unwrap();
        owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    }
    let operation = owners
        .reserve_retry(&owner, true, 32, 6, 10)
        .unwrap()
        .unwrap();
    (owners, owner, operation)
}

#[tokio::test]
async fn quarantine_entry_waits_for_the_last_reserved_peer_operation() {
    for persistent in [false, true] {
        let (owners, owner, operation) = last_peer_operation(persistent).await;
        let before = owner.policy();
        assert!(owners
            .reserve_retry(&owner, false, 32, 7, 10)
            .unwrap()
            .is_none());
        assert_eq!(owner.policy(), before);
        assert_eq!(owners.available_operations(), 1);
        operation.complete_action().unwrap();
        assert_eq!(owner.policy().retry_attempts, 32);
        assert_eq!(owner.policy().peer_requery_attempts, 3);
        assert_eq!(owners.available_operations(), 2);
    }
}

async fn assert_quarantine_entry_clears_peer_schedule(persistent: bool) {
    let (owners, owner, operation) = last_peer_operation(persistent).await;
    operation.complete_action().unwrap();
    let before = owner.policy();
    assert!(owners
        .reserve_retry(&owner, false, 32, 7, 10)
        .unwrap()
        .is_none());
    let after = owner.policy();
    assert_eq!(after.retry_attempts, 32);
    assert_eq!(after.retry_budget_quarantine_until, Some(17));
    assert_eq!(after.initial_timestamp, before.initial_timestamp);
    assert_eq!(
        after.requested_as_dependency,
        persistent && before.requested_as_dependency
    );
    assert_eq!(
        (
            after.peer_requery_attempts,
            after.peer_requery_cursor,
            after.dependency_recovery_last_request,
            after.broadcast_retry_last_request,
            after.peer_requery_last_request,
        ),
        (0, 0, None, None, None),
        "upstream quarantine entry retires peer state without resetting the spent total"
    );
}

#[tokio::test]
async fn quarantine_entry_volatile_clears_peer_schedule() {
    assert_quarantine_entry_clears_peer_schedule(false).await;
}

#[tokio::test]
async fn quarantine_entry_durable_clears_peer_schedule() {
    assert_quarantine_entry_clears_peer_schedule(true).await;
}

#[tokio::test]
async fn suppressed_selection_commits_only_clock_and_cursor_and_releases_permit() {
    for persistent in [false, true] {
        let owners = owners(1, 1).await;
        let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
        if persistent {
            owners
                .publish_pending(&owner, HashSet::new(), HashSet::new())
                .unwrap();
            owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
        }
        let before = owner.policy();
        let result = owners
            .reserve_retry_with(&owner, 32, 5, 10, |candidate, _, _| {
                candidate.last_request_timestamp = 5;
                candidate.peer_requery_cursor = 1;
                candidate.retry_attempts = 17;
                candidate.peer_requery_last_request = Some(5);
                request_ownership::RetrySelection::Suppressed(())
            })
            .unwrap();
        assert!(matches!(
            result,
            request_ownership::PreparedRetry::Suppressed(())
        ));
        let after = owner.policy();
        assert_eq!(after.last_request_timestamp, 5);
        assert_eq!(after.peer_requery_cursor, 1);
        assert_eq!(after.retry_attempts, before.retry_attempts);
        assert_eq!(
            after.peer_requery_last_request,
            before.peer_requery_last_request
        );
        assert_eq!(owners.available_operations(), 1);
        if persistent {
            assert_eq!(
                owners
                    .buffer()
                    .pending_request_policy(&BlockHashSerde(hash(1)))
                    .unwrap()
                    .unwrap(),
                after
            );
        }
    }
}

#[tokio::test]
async fn no_selection_discards_candidate_changes() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let before = owner.policy();
    let result = owners
        .reserve_retry_with(&owner, 32, 5, 10, |candidate, _, _| {
            candidate.last_request_timestamp = 5;
            candidate.peer_requery_cursor = 1;
            request_ownership::RetrySelection::<()>::None
        })
        .unwrap();
    assert!(matches!(result, request_ownership::PreparedRetry::None));
    assert_eq!(owner.policy(), before);
    assert_eq!(owners.available_operations(), 1);
}

fn hash(value: u8) -> BlockHash { vec![value; 32].into() }

fn policy() -> PendingRequestPolicy {
    PendingRequestPolicy {
        revision: 1,
        initial_timestamp: 1,
        last_request_timestamp: 2,
        requested_as_dependency: true,
        retry_attempts: 0,
        peer_requery_attempts: 0,
        peer_requery_cursor: 0,
        retry_budget_quarantine_until: None,
        dependency_recovery_last_request: None,
        broadcast_retry_last_request: None,
        peer_requery_last_request: None,
    }
}

async fn owners(active: usize, operations: usize) -> RequestOwners<()> {
    let mut manager = InMemoryStoreManager::new();
    RequestOwners::new(
        CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap(),
        active,
        operations,
        |_| (),
    )
}

#[tokio::test]
async fn publish_by_hash_preserves_capacity_and_existing_owner() {
    let owners = owners(1, 2).await;
    let active = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&active, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending_for(hash(2), policy(), |_| (), HashSet::new(), HashSet::new())
        .unwrap();
    assert_eq!(owners.active_count(), 1);
    assert!(Arc::ptr_eq(&owners.active_owners()[0], &active));
    assert!(owners
        .buffer()
        .pending_request_policy(&BlockHashSerde(hash(2)))
        .unwrap()
        .is_some());
    owners
        .publish_pending_for(hash(1), policy(), |_| (), HashSet::new(), HashSet::new())
        .unwrap();
    assert_eq!(owners.active_count(), 0);
    let loaded = owners.lookup(&hash(1), |_| ()).unwrap().unwrap();
    assert!(Arc::ptr_eq(&active, &loaded));
    operation.complete_action().unwrap();
    assert_eq!(loaded.policy().retry_attempts, 1);
    assert_eq!(loaded.policy().peer_requery_attempts, 1);
}

#[tokio::test]
async fn publication_merges_captured_provenance_into_existing_owner() {
    for persistent in [false, true] {
        let owners = owners(1, 1).await;
        let mut initial = policy();
        initial.requested_as_dependency = false;
        let owner = owners.activate(hash(1), initial, |_| ()).unwrap().unwrap();
        if persistent {
            owners
                .publish_pending(&owner, HashSet::new(), HashSet::new())
                .unwrap();
        }
        owners
            .publish_pending_for(
                hash(1),
                policy(),
                |_| (),
                HashSet::from([BlockHashSerde(hash(2))]),
                HashSet::new(),
            )
            .unwrap();
        assert!(owner.policy().requested_as_dependency);
        assert!(
            owners
                .buffer()
                .pending_request_policy(&BlockHashSerde(hash(1)))
                .unwrap()
                .unwrap()
                .requested_as_dependency
        );
        assert_eq!(
            owners.buffer().get_parents(&BlockHashSerde(hash(1))),
            Some(HashSet::from([BlockHashSerde(hash(2))]))
        );
    }
}

#[tokio::test]
async fn publication_conflict_does_not_partially_promote_captured_provenance() {
    let owners = owners(1, 1).await;
    let mut initial = policy();
    initial.requested_as_dependency = false;
    let owner = owners.activate(hash(1), initial, |_| ()).unwrap().unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    let before = owner.policy();
    let mut competing = before.clone();
    competing.advance_revision().unwrap();
    competing.last_request_timestamp += 1;
    owners
        .buffer()
        .update_pending_request_policy(&BlockHashSerde(hash(1)), &before, &competing)
        .unwrap();
    assert!(owners
        .publish_pending_for(
            hash(1),
            policy(),
            |_| (),
            HashSet::from([BlockHashSerde(hash(2))]),
            HashSet::new(),
        )
        .is_err());
    assert_eq!(owner.policy(), before);
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap(),
        Some(competing)
    );
    assert!(owners
        .buffer()
        .contains_durable_row(&BlockHashSerde(hash(1)))
        .unwrap());
    assert!(owners.buffer().is_pendant(&BlockHashSerde(hash(1))));
}

#[tokio::test]
async fn pending_handoff_preserves_inflight_completion_and_cold_provenance() {
    let owners = owners(1, 2).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert_eq!(owner.hash(), &hash(1));
    assert_eq!(owner.inspect(|policy, _| policy.initial_timestamp), 1);
    let operation = owners
        .reserve_retry(&owner, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    assert_eq!(owners.active_count(), 0);
    drop(owner);
    operation.complete_action().unwrap();
    let recovered = owners.lookup(&hash(1), |_| ()).unwrap().unwrap();
    assert_eq!(
        (
            recovered.policy().retry_attempts,
            recovered.policy().peer_requery_attempts
        ),
        (1, 1)
    );
    assert!(recovered.policy().requested_as_dependency);
    assert_eq!(owners.active_count(), 0);
    let restarted = RequestOwners::new(owners.buffer().clone(), 1, 2, |_| ());
    let cold = restarted.lookup(&hash(1), |_| ()).unwrap().unwrap();
    assert_eq!(cold.policy(), recovered.policy());
}

#[tokio::test]
async fn old_completion_and_cleanup_do_not_change_replacement() {
    let owners = owners(1, 2).await;
    let old = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&old, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners.terminate(&old).unwrap();
    let mut replacement_policy = policy();
    replacement_policy.retry_attempts = 7;
    replacement_policy.peer_requery_attempts = 3;
    let replacement = owners
        .activate(hash(1), replacement_policy.clone(), |_| ())
        .unwrap()
        .unwrap();
    operation.complete_action().unwrap();
    owners.terminate(&old).unwrap();
    assert_eq!(replacement.policy(), replacement_policy);
    assert_eq!(owners.active_count(), 1);
    assert!(Arc::ptr_eq(&owners.active_owners()[0], &replacement));
}

#[tokio::test]
async fn failed_completion_write_retains_owner_and_permit_until_repaired() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&owner, false, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    owners.buffer().remove(BlockHashSerde(hash(1))).unwrap();
    let original = Arc::downgrade(&owner);
    drop(owner);
    assert!(operation.complete_action().is_err());
    assert_eq!(owners.deferred_count(), 1);
    assert_eq!(owners.available_operations(), 0);
    let restored = owners.lookup(&hash(1), |_| ()).unwrap().unwrap();
    assert!(Arc::ptr_eq(&restored, &original.upgrade().unwrap()));
    assert_eq!(restored.policy().retry_attempts, 1);
    for _ in 0..3 {
        assert!(owners.drain_completions(1).is_err());
        assert_eq!(owners.deferred_count(), 1);
        assert_eq!(restored.policy().retry_attempts, 1);
    }
    owners
        .buffer()
        .publish_pending_request(
            BlockHashSerde(hash(1)),
            HashSet::new(),
            HashSet::new(),
            None,
            &policy(),
        )
        .unwrap();
    assert_eq!(owners.drain_completions(1).unwrap(), 1);
    assert_eq!(owners.deferred_count(), 0);
    assert_eq!(owners.available_operations(), 1);
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap()
            .unwrap()
            .retry_attempts,
        1
    );
}

#[tokio::test]
async fn later_completion_commits_prior_deferred_charge_exactly_once() {
    let owners = owners(1, 2).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let first = owners
        .reserve_retry(&owner, false, 32, 3, 10)
        .unwrap()
        .unwrap();
    let second = owners
        .reserve_retry(&owner, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    owners.buffer().remove(BlockHashSerde(hash(1))).unwrap();
    assert!(first.complete_action().is_err());
    owners
        .buffer()
        .publish_pending_request(
            BlockHashSerde(hash(1)),
            HashSet::new(),
            HashSet::new(),
            None,
            &policy(),
        )
        .unwrap();
    second.complete_action().unwrap();
    let committed = owners
        .buffer()
        .pending_request_policy(&BlockHashSerde(hash(1)))
        .unwrap()
        .unwrap();
    assert_eq!(
        (committed.retry_attempts, committed.peer_requery_attempts),
        (2, 1)
    );
    assert_eq!(owners.drain_completions(2).unwrap(), 1);
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap(),
        Some(committed)
    );
    assert_eq!(owners.available_operations(), 2);
}

#[tokio::test]
async fn cancelled_transport_releases_allowance_without_charging() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&owner, true, 1, 3, 10)
        .unwrap()
        .unwrap();
    assert!(owners
        .reserve_retry(&owner, false, 1, 3, 10)
        .unwrap()
        .is_none());
    drop(operation);
    assert_eq!(owner.policy(), policy());
    assert_eq!(owners.available_operations(), 1);
    owners
        .reserve_retry(&owner, false, 1, 3, 10)
        .unwrap()
        .unwrap()
        .complete_action()
        .unwrap();
    assert_eq!(owner.policy().retry_attempts, 1);
}

#[tokio::test]
async fn terminal_disposal_releases_deferred_completion_without_replacement_charge() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&owner, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    owners.buffer().remove(BlockHashSerde(hash(1))).unwrap();
    assert!(operation.complete_action().is_err());
    owners.terminate(&owner).unwrap();
    let replacement = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert_eq!(owners.drain_completions(1).unwrap(), 1);
    assert_eq!(replacement.policy(), policy());
    assert_eq!(owners.available_operations(), 1);
}

#[tokio::test]
async fn concurrent_last_allowance_does_not_create_expired_budget_probes() {
    let owners = owners(1, 2).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let barrier = Barrier::new(3);
    let operations = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    owners.reserve_retry(&owner, false, 1, 3, 10).unwrap()
                })
            })
            .collect();
        barrier.wait();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(operations.len(), 1);
    operations
        .into_iter()
        .next()
        .unwrap()
        .complete_action()
        .unwrap();
    assert!(owners
        .reserve_retry(&owner, false, 1, 4, 10)
        .unwrap()
        .is_none());
    let probes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    owners.reserve_retry(&owner, false, 1, 14, 10).unwrap()
                })
            })
            .collect();
        barrier.wait();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(probes.is_empty());
    drop(probes);
    assert_eq!(owner.policy().retry_attempts, 1);
    assert_eq!(owner.policy().retry_budget_quarantine_until, Some(14));
    assert_eq!(owners.active_count(), 0);
    owners.renew_expired(14).await.unwrap();
    assert_eq!(owners.retry_budget(&hash(1)).unwrap(), (0, None));
}

#[tokio::test]
async fn capacity_and_foreign_owner_fail_without_mutation() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert!(owners
        .activate(hash(2), policy(), |_| ())
        .unwrap()
        .is_none());
    let foreign: RequestOwners<()> = RequestOwners::new(owners.buffer().clone(), 1, 1, |_| ());
    assert!(foreign.terminate(&owner).is_err());
    assert!(foreign
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .is_err());
    assert!(foreign.reserve_retry(&owner, false, 32, 3, 10).is_err());
    assert!(foreign
        .update_policy(&owner, |policy, _| policy.requested_as_dependency = false)
        .is_err());
    assert_eq!(owner.policy(), policy());
}

#[tokio::test]
async fn concurrent_cold_lookup_uses_one_owner() {
    let owners = owners(2, 2).await;
    owners
        .buffer()
        .publish_pending_request(
            BlockHashSerde(hash(1)),
            HashSet::new(),
            HashSet::new(),
            None,
            &policy(),
        )
        .unwrap();
    let barrier = Barrier::new(3);
    let loaded = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    owners.lookup(&hash(1), |_| ()).unwrap().unwrap()
                })
            })
            .collect();
        barrier.wait();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(Arc::ptr_eq(&loaded[0], &loaded[1]));
    assert_eq!(owners.active_count(), 0);
    let activated = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert!(Arc::ptr_eq(&loaded[0], &activated));
}

#[tokio::test]
async fn concurrent_publication_and_completion_preserve_committed_charge() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&owner, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let completion = scope.spawn(|| {
            barrier.wait();
            operation.complete_action().unwrap();
        });
        barrier.wait();
        owners
            .publish_pending(&owner, HashSet::new(), HashSet::new())
            .unwrap();
        completion.join().unwrap();
    });
    let committed = owners
        .buffer()
        .pending_request_policy(&BlockHashSerde(hash(1)))
        .unwrap()
        .unwrap();
    assert_eq!(
        (committed.retry_attempts, committed.peer_requery_attempts),
        (1, 1)
    );
    assert_eq!(committed, owner.policy());
    assert_eq!(owners.available_operations(), 1);
    assert_eq!(owners.active_count(), 0);
}

#[tokio::test]
async fn rejected_mutations_preserve_policy_and_transient_data() {
    let mut manager = InMemoryStoreManager::new();
    let owners = RequestOwners::new(
        CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap(),
        1,
        1,
        |_| 0u32,
    );
    let mut initial = policy();
    initial.retry_attempts = 1;
    let owner = owners
        .activate(hash(1), initial, |_| 17u32)
        .unwrap()
        .unwrap();
    for pending in [false, true] {
        if pending {
            owners
                .publish_pending(&owner, HashSet::new(), HashSet::new())
                .unwrap();
        }
        for change in 0..3 {
            let before = owner.policy();
            assert!(owners
                .update_policy(&owner, |policy, data| {
                    *data = 99;
                    match change {
                        0 => policy.initial_timestamp += 1,
                        1 => policy.requested_as_dependency = false,
                        _ => policy.retry_attempts = 0,
                    }
                })
                .is_err());
            assert_eq!(owner.policy(), before);
            assert_eq!(owner.inspect(|_, data| *data), 17);
        }
    }
}

#[tokio::test]
async fn failed_publication_preserves_active_owner_and_storage_snapshot() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let mut competing = policy();
    competing.last_request_timestamp = 11;
    owners
        .buffer()
        .publish_pending_request(
            BlockHashSerde(hash(1)),
            HashSet::new(),
            HashSet::new(),
            None,
            &competing,
        )
        .unwrap();
    assert!(owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .is_err());
    assert_eq!(owners.active_count(), 1);
    assert!(Arc::ptr_eq(&owners.active_owners()[0], &owner));
    assert_eq!(owner.policy(), policy());
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap(),
        Some(competing)
    );
}

#[tokio::test]
async fn failed_queue_entry_does_not_prevent_independent_completion() {
    let owners = owners(2, 2).await;
    let mut retained = Vec::new();
    for key in [1, 2] {
        let owner = owners
            .activate(hash(key), policy(), |_| ())
            .unwrap()
            .unwrap();
        let operation = owners
            .reserve_retry(&owner, false, 32, 3, 10)
            .unwrap()
            .unwrap();
        owners
            .publish_pending(&owner, HashSet::new(), HashSet::new())
            .unwrap();
        owners.buffer().remove(BlockHashSerde(hash(key))).unwrap();
        assert!(operation.complete_action().is_err());
        retained.push(owner);
    }
    assert_eq!(owners.deferred_count(), 2);
    assert_eq!(owners.available_operations(), 0);
    let third = owners.activate(hash(3), policy(), |_| ()).unwrap().unwrap();
    assert!(owners
        .reserve_retry(&third, false, 32, 3, 10)
        .unwrap()
        .is_none());
    owners
        .buffer()
        .publish_pending_request(
            BlockHashSerde(hash(2)),
            HashSet::new(),
            HashSet::new(),
            None,
            &policy(),
        )
        .unwrap();
    assert!(owners.drain_completions(2).is_err());
    assert_eq!(owners.deferred_count(), 1);
    assert_eq!(owners.available_operations(), 1);
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(2)))
            .unwrap()
            .unwrap()
            .retry_attempts,
        1
    );
    assert_eq!(retained[0].policy().retry_attempts, 1);
    owners.terminate(&retained[0]).unwrap();
    assert_eq!(owners.drain_completions(2).unwrap(), 1);
    assert_eq!(owners.available_operations(), 2);
}

#[tokio::test]
async fn conflicting_revision_retains_success_without_overwriting_storage() {
    let owners = owners(1, 1).await;
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    let operation = owners
        .reserve_retry(&owner, true, 32, 3, 10)
        .unwrap()
        .unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    let before = owner.policy();
    let mut conflicting = before.clone();
    conflicting.advance_revision().unwrap();
    conflicting.last_request_timestamp = 99;
    owners
        .buffer()
        .update_pending_request_policy(&BlockHashSerde(hash(1)), &before, &conflicting)
        .unwrap();
    assert!(operation.complete_action().is_err());
    assert_eq!(owner.policy().retry_attempts, 1);
    assert_eq!(owner.policy().peer_requery_attempts, 1);
    assert_eq!(owners.deferred_count(), 1);
    assert_eq!(owners.available_operations(), 0);
    assert!(owners.drain_completions(1).is_err());
    assert_eq!(
        owners
            .buffer()
            .pending_request_policy(&BlockHashSerde(hash(1)))
            .unwrap(),
        Some(conflicting)
    );
    let reactivated = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert!(Arc::ptr_eq(&owner, &reactivated));
    assert_eq!(reactivated.policy().retry_attempts, 1);
    owners.terminate(&owner).unwrap();
    assert_eq!(owners.drain_completions(1).unwrap(), 1);
    assert_eq!(owners.available_operations(), 1);
}

#[tokio::test]
async fn restart_preserves_spent_budget_until_explicit_renewal() {
    let owners = owners(1, 1).await;
    let mut initial = policy();
    initial.retry_attempts = u32::MAX;
    initial.peer_requery_attempts = u32::MAX;
    initial.retry_budget_quarantine_until = Some(3);
    let owner = owners.activate(hash(1), initial, |_| ()).unwrap().unwrap();
    owners
        .publish_pending(&owner, HashSet::new(), HashSet::new())
        .unwrap();
    let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
    assert!(owners
        .reserve_retry(&owner, true, u32::MAX, 3, 10)
        .unwrap()
        .is_none());
    let claimed = owner.policy();
    assert_eq!(claimed.retry_attempts, u32::MAX);
    let restarted = RequestOwners::new(owners.buffer().clone(), 1, 1, |_| ());
    let loaded = restarted
        .activate(hash(1), policy(), |_| ())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.policy(), claimed);
    assert!(restarted
        .reserve_retry(&loaded, false, u32::MAX, 12, 10)
        .unwrap()
        .is_none());
    restarted.renew_expired(12).await.unwrap();
    assert_eq!(loaded.policy().retry_attempts, u32::MAX);
    restarted.renew_expired(13).await.unwrap();
    assert_eq!(loaded.policy().retry_attempts, 0);
    assert_eq!(loaded.policy().retry_budget_quarantine_until, None);
    let operation = restarted
        .reserve_retry(&loaded, false, u32::MAX, 13, 10)
        .unwrap()
        .unwrap();
    operation.complete_action().unwrap();
    assert_eq!(loaded.policy().retry_attempts, 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn publication_provenance_histories_preserve_atomic_union(
        initial_dependency in any::<bool>(),
        actions in prop::collection::vec((any::<bool>(), 2u8..8, any::<bool>()), 1..65),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let owners = owners(1, 1).await;
            let mut initial = policy();
            initial.requested_as_dependency = initial_dependency;
            let mut owner = owners.activate(hash(1), initial, |_| ()).unwrap().unwrap();
            let mut expected_dependency = initial_dependency;
            for (captured, parent, conflict) in actions {
                let before = owner.policy();
                let before_parents = owners.buffer().get_parents(&BlockHashSerde(hash(1)));
                if conflict {
                    if let Some(mut competing) = owners.buffer().pending_request_policy(&BlockHashSerde(hash(1))).unwrap() {
                        let expected = competing.clone();
                        competing.advance_revision().unwrap();
                        competing.last_request_timestamp += 1;
                        owners.buffer().update_pending_request_policy(
                            &BlockHashSerde(hash(1)), &expected, &competing,
                        ).unwrap();
                    } else {
                        owners.buffer().publish_pending_request(
                            BlockHashSerde(hash(1)), HashSet::new(), HashSet::new(), None, &before,
                        ).unwrap();
                    }
                }
                let mut seed = policy();
                seed.requested_as_dependency = captured;
                let result = owners.publish_pending_for(
                    hash(1), seed, |_| (), HashSet::from([BlockHashSerde(hash(parent))]), HashSet::new(),
                );
                if conflict {
                    prop_assert!(result.is_err());
                    prop_assert_eq!(owner.policy(), before);
                    prop_assert_eq!(owners.buffer().get_parents(&BlockHashSerde(hash(1))), before_parents);
                    owners.terminate(&owner).unwrap();
                    let mut fresh = policy();
                    fresh.requested_as_dependency = expected_dependency;
                    owner = owners.activate(hash(1), fresh, |_| ()).unwrap().unwrap();
                } else {
                    result.unwrap();
                    expected_dependency |= captured;
                    prop_assert_eq!(owner.policy().requested_as_dependency, expected_dependency);
                    prop_assert_eq!(
                        owners.buffer().pending_request_policy(&BlockHashSerde(hash(1))).unwrap().unwrap(),
                        owner.policy()
                    );
                    let restored = RequestOwners::new(owners.buffer().clone(), 1, 1, |_| ());
                    prop_assert_eq!(
                        restored.lookup(&hash(1), |_| ()).unwrap().unwrap().policy().requested_as_dependency,
                        expected_dependency
                    );
                }
            }
            Ok(())
        })?;
    }

    #[test]
    fn completion_histories_preserve_exact_policy(
        actions in prop::collection::vec((0u8..3, any::<bool>(), any::<bool>()), 1..65),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let owners = owners(1, 2).await;
            let mut expected = (0, 0);
            for (outcome, peer, pending) in actions {
                let owner = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
                let operation = owners.reserve_retry(&owner, peer, u32::MAX, 3, 10).unwrap().unwrap();
                if pending {
                    owners.publish_pending(&owner, HashSet::new(), HashSet::new()).unwrap();
                }
                if outcome < 2 {
                    operation.complete_action().unwrap();
                    expected.0 += 1;
                    expected.1 += u32::from(peer);
                } else { drop(operation); }
                prop_assert_eq!((owner.policy().retry_attempts, owner.policy().peer_requery_attempts), expected);
                prop_assert_eq!(owners.available_operations(), 2);
                owners.update_policy(&owner, |policy, _| policy.last_request_timestamp += 1).unwrap();
                prop_assert!(owner.policy().requested_as_dependency);
            }
            Ok(())
        })?;
    }

    #[test]
    fn interleaved_completion_histories_preserve_incarnation_and_permit_bounds(
        actions in prop::collection::vec((0u8..7, any::<bool>(), 0u8..3), 1..97),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let owners = owners(1, 2).await;
            let mut current = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
            let mut generation = 0u32;
            let mut expected = (0u32, 0u32);
            let mut outstanding = Vec::new();
            for (action, peer, outcome) in actions {
                match action {
                    0 | 1 => {
                        current = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
                        if let Some(operation) = owners.reserve_retry(&current, peer, u32::MAX, 3, 10).unwrap() {
                            outstanding.push((generation, peer, operation));
                        }
                    }
                    2 if !outstanding.is_empty() => {
                        let index = if peer { outstanding.len() - 1 } else { 0 };
                        let (origin, peer, operation) = outstanding.remove(index);
                        if outcome < 2 {
                            operation.complete_action().unwrap();
                            if origin == generation {
                                expected.0 += 1;
                                expected.1 += u32::from(peer);
                            }
                        } else { drop(operation); }
                    }
                    3 => owners.publish_pending(&current, HashSet::new(), HashSet::new()).unwrap(),
                    4 => {
                        owners.terminate(&current).unwrap();
                        generation += 1;
                        expected = (0, 0);
                        current = owners.activate(hash(1), policy(), |_| ()).unwrap().unwrap();
                    }
                    5 => {
                        let loaded = owners.lookup(&hash(1), |_| ()).unwrap().unwrap();
                        prop_assert!(Arc::ptr_eq(&current, &loaded));
                    }
                    _ => {
                        owners.update_policy(&current, |policy, _| policy.last_request_timestamp += 1).unwrap();
                    }
                }
                prop_assert_eq!((current.policy().retry_attempts, current.policy().peer_requery_attempts), expected);
                prop_assert_eq!(owners.available_operations() + outstanding.len(), 2);
                prop_assert_eq!(owners.deferred_count(), 0);
                prop_assert!(owners.active_count() <= 1);
                if let Some(committed) = owners.buffer().pending_request_policy(&BlockHashSerde(hash(1))).unwrap() {
                    prop_assert_eq!(committed, current.policy());
                }
            }
            drop(outstanding);
            prop_assert_eq!(owners.available_operations(), 2);
            Ok(())
        })?;
    }
}
