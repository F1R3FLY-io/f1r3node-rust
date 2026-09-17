use std::num::NonZeroUsize;

use acceptance::SupplyReader;
use casper::rust::util::rholang::costacc::vault_cost_deploy::ApplyPhloCostDeploy;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorTransition,
};
use rholang::rust::interpreter::rho_type::RhoList;

use super::*;

#[path = "funding_cursor_vault.rs"]
mod funding_cursor_vault;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_publishes_both_contributions_and_rejects_stale_roles() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xd1; 32] });
        let count = NonZeroUsize::new(2).unwrap();
        let resource_scope = [0xd2; 32];
        let fee_scope = [0xd3; 32];
        let make = |resource_revision, fee_revision| {
            ApplyPhloCostDeploy::new(
                ApplyCostDeploy::new(
                    [0xd1; 32],
                    vec![VaultAllocation::new(payer.to_base58(), 5).unwrap()],
                    vec![VaultSettlement::new(payer.to_base58(), 2, 1).unwrap()],
                    recipient.to_base58(),
                    Blake2b512Random::create_from_bytes(&[0xd1]),
                )
                .unwrap(),
                Some(
                    MonetaryCursorTransition::new(
                        resource_scope,
                        MonetaryCursor::new(resource_revision, 0, count).unwrap(),
                        0,
                        count,
                    )
                    .unwrap(),
                ),
                Some(
                    MonetaryCursorTransition::new(
                        fee_scope,
                        MonetaryCursor::new(fee_revision, 0, count).unwrap(),
                        0,
                        count,
                    )
                    .unwrap(),
                ),
                count,
            )
            .unwrap()
        };
        let before = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let first = successful_system_state(
            ops.play_system_deploy(&initial, &mut make(0, 0))
                .await
                .unwrap(),
        );
        assert_eq!(
            cursor_at(&manager, &first, resource_scope).await,
            (true, 1, 0)
        );
        assert_eq!(cursor_at(&manager, &first, fee_scope).await, (true, 1, 0));
        assert_eq!(
            system_vault_balance(&manager, &first, &payer).await,
            before - 3
        );
        assert_eq!(system_vault_balance(&manager, &first, &recipient).await, 1);
        for (resource, fee) in [(0, 1), (1, 0), (0, 0)] {
            assert!(matches!(
                ops.play_system_deploy(&first, &mut make(resource, fee))
                    .await
                    .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
                first
            );
        }
        let second = successful_system_state(
            ops.play_system_deploy(&first, &mut make(1, 1))
                .await
                .unwrap(),
        );
        assert_eq!(
            cursor_at(&manager, &second, resource_scope).await,
            (true, 2, 0)
        );
        assert_eq!(cursor_at(&manager, &second, fee_scope).await, (true, 2, 0));
        assert_eq!(
            system_vault_balance(&manager, &second, &payer).await,
            before - 6
        );
        assert_eq!(system_vault_balance(&manager, &second, &recipient).await, 2);
    })
    .await
    .unwrap();
}

async fn assert_cursor_readers(
    manager: &RuntimeManager,
    state: &StateHash,
    scope: [u8; 32],
    expected: Option<MonetaryCursor>,
) {
    let count = NonZeroUsize::new(2).unwrap();
    let reader = acceptance::RuntimeManagerSupplyReader {
        runtime_manager: manager,
        pre_state_hash: state.clone(),
    };
    assert_eq!(
        reader.read_monetary_cursor(scope, count).await.unwrap(),
        expected
    );
    let mut runtime = manager.spawn_runtime().await;
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(state))
        .await
        .unwrap();
    let mut ops = RuntimeOps::new(runtime);
    let reader = acceptance::RuntimeOpsSupplyReader {
        runtime_ops: &ops,
        pre_state_root: state.as_ref().try_into().unwrap(),
    };
    assert_eq!(
        reader.read_monetary_cursor(scope, count).await.unwrap(),
        expected
    );
    assert_eq!(
        ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
        *state
    );
}

async fn cursor_at(
    manager: &RuntimeManager,
    state: &StateHash,
    scope: [u8; 32],
) -> (bool, i64, i64) {
    let source = format!(
        r#"new return, rl(`rho:registry:lookup`), vaultCh in {{
          rl!(`rho:vault:system`, *vaultCh) |
          for (@(_, vault) <- vaultCh) {{
            @vault!("costCursor", "{}".hexToBytes(), *return)
          }}
        }}"#,
        hex::encode(scope),
    );
    let (values, _) = manager
        .play_exploratory_deploy(source, state, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    let fields = RhoList::unapply(&values[0]).unwrap();
    assert_eq!(fields.len(), 3);
    (
        RhoBoolean::unapply(&fields[0]).unwrap(),
        RhoNumber::unapply(&fields[1]).unwrap(),
        RhoNumber::unapply(&fields[2]).unwrap(),
    )
}

fn guarded(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    scope: [u8; 32],
    revision: i64,
    position: i64,
    next_position: i64,
) -> ApplyCostDeploy {
    let count = NonZeroUsize::new(2).unwrap();
    let cursor = MonetaryCursor::new(revision, position, count).unwrap();
    ApplyCostDeploy::new(
        [0x91; 32],
        vec![VaultAllocation::new(payer.to_base58(), 1).unwrap()],
        vec![VaultSettlement::new(payer.to_base58(), 0, 1).unwrap()],
        recipient.to_base58(),
        Blake2b512Random::create_from_bytes(&[0x91]),
    )
    .unwrap()
    .with_fee_cursor(
        MonetaryCursorTransition::new(scope, cursor, next_position, count).unwrap(),
        count,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_rejects_stale_reuse_and_preserves_other_scopes() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0x91; 32] });
        let scope = [0x92; 32];
        let other = [0x93; 32];
        assert_eq!(cursor_at(&manager, &initial, scope).await, (false, 0, 0));
        let initial_balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let first = successful_system_state(
            ops.play_system_deploy(&initial, &mut guarded(&payer, &recipient, scope, 0, 0, 0))
                .await
                .unwrap(),
        );
        assert_eq!(cursor_at(&manager, &first, scope).await, (true, 1, 0));
        assert_eq!(
            system_vault_balance(&manager, &first, &payer).await,
            initial_balance - 1
        );
        assert_eq!(system_vault_balance(&manager, &first, &recipient).await, 1);

        let rejected = ops
            .play_system_deploy(&first, &mut guarded(&payer, &recipient, scope, 0, 0, 0))
            .await
            .unwrap();
        assert!(matches!(rejected, SystemDeployResult::PlayFailed { .. }));
        let after_rejection = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
        assert_eq!(after_rejection, first);
        assert_eq!(
            cursor_at(&manager, &after_rejection, scope).await,
            (true, 1, 0)
        );
        assert_eq!(
            system_vault_balance(&manager, &after_rejection, &recipient).await,
            1
        );

        let second = successful_system_state(
            ops.play_system_deploy(&first, &mut guarded(&payer, &recipient, scope, 1, 0, 1))
                .await
                .unwrap(),
        );
        assert_eq!(cursor_at(&manager, &second, scope).await, (true, 2, 1));
        let third = successful_system_state(
            ops.play_system_deploy(&second, &mut guarded(&payer, &recipient, other, 0, 0, 1))
                .await
                .unwrap(),
        );
        assert_eq!(cursor_at(&manager, &third, scope).await, (true, 2, 1));
        assert_eq!(cursor_at(&manager, &third, other).await, (true, 1, 1));
        assert_eq!(
            system_vault_balance(&manager, &third, &payer).await,
            initial_balance - 3
        );
        assert_eq!(system_vault_balance(&manager, &third, &recipient).await, 3);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_failed_first_settlement_restores_absence() {
    with_runtime_manager(|manager, _, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0x94; 32] });
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0x95; 32] });
        let scope = [0x96; 32];
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        assert_eq!(cursor_at(&manager, &initial, scope).await, (false, 0, 0));
        let failed = ops
            .play_system_deploy(&initial, &mut guarded(&payer, &recipient, scope, 0, 0, 1))
            .await
            .unwrap();
        assert!(matches!(failed, SystemDeployResult::PlayFailed { .. }));
        let after = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
        assert_eq!(after, initial);
        assert_eq!(cursor_at(&manager, &after, scope).await, (false, 0, 0));
        assert_eq!(system_vault_balance(&manager, &after, &payer).await, 0);
        assert_eq!(system_vault_balance(&manager, &after, &recipient).await, 0);
    })
    .await
    .unwrap();
}

struct ConcurrentCursorApply(ApplyCostDeploy);

struct CursorApplyProbe {
    deploy: ApplyCostDeploy,
    failure: i64,
    cursor_override: Option<Par>,
}

impl SystemDeployTrait for CursorApplyProbe {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"new rl(`rho:registry:lookup`), vaultCh, applied,
            reservationId(`sys:casper:costReservationId`),
            allocations(`sys:casper:costAllocations`), charges(`sys:casper:costSettlements`),
            feeAddress(`sys:casper:costFeeAddress`), feeCursor(`sys:casper:costFeeCursor`),
            failure(`sys:casper:cursorProbeFailure`),
            auth(`sys:casper:authToken`), return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *vaultCh) |
          for (@(_, vault) <- vaultCh) {
            match *feeCursor {
              [cursor] => {
                @vault!("applyCost", *reservationId, *allocations, *charges, *feeAddress, cursor, *auth, *applied)
              }
              _ => { applied!((false, "Invalid monetary cursor transport")) }
            } |
            for (@result <- applied) {
              match result {
                (true, _) => {
                  match *failure {
                    1 => { return!((false, "rejected after cursor settlement")) }
                    2 => { return!(1 / 0) }
                    3 => { return!("malformed after cursor settlement") }
                    _ => { return!(result) }
                  }
                }
                _ => { return!(result) }
              }
            }
          }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, ()> {
        ApplyCostDeploy::process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.deploy.rand() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = self.deploy.env();
        env.insert(
            "sys:casper:cursorProbeFailure".to_string(),
            RhoNumber::create_par(self.failure),
        );
        if let Some(cursor) = &self.cursor_override {
            env.insert(
                "sys:casper:costFeeCursor".to_string(),
                RhoList::create_par(vec![cursor.clone()]),
            );
        }
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> { self.deploy.return_channel() }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_outer_failures_restore_money_and_cursor() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xa1; 32] });
        let scope = [0xa2; 32];
        let initial_balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let committed = successful_system_state(
            ops.play_system_deploy(&initial, &mut guarded(&payer, &recipient, scope, 0, 0, 1))
                .await
                .unwrap(),
        );
        for (state, revision, position) in [(&initial, 0, 0), (&committed, 1, 1)] {
            for failure in 1..=3 {
                let result = ops
                    .play_system_deploy(state, &mut CursorApplyProbe {
                        deploy: guarded(&payer, &recipient, scope, revision, position, 0),
                        failure,
                        cursor_override: None,
                    })
                    .await;
                let diagnostic = match &result {
                    Err(error) => format!("{error:?}"),
                    Ok(SystemDeployResult::PlayFailed {
                        processed_system_deploy,
                    }) => {
                        format!("{processed_system_deploy:?}")
                    }
                    Ok(SystemDeployResult::PlaySucceeded { .. }) => {
                        "unexpected success".to_string()
                    }
                };
                match failure {
                    1 => {
                        assert!(matches!(result, Ok(SystemDeployResult::PlayFailed { .. })));
                        assert!(
                            diagnostic.contains("rejected after cursor settlement"),
                            "{diagnostic}"
                        );
                    }
                    2 => {
                        assert!(result.is_err(), "{diagnostic}");
                        assert!(
                            diagnostic.to_lowercase().contains("division"),
                            "{diagnostic}"
                        );
                    }
                    3 => {
                        assert!(
                            matches!(result, Ok(SystemDeployResult::PlayFailed { .. })),
                            "{diagnostic}"
                        );
                        assert!(
                            diagnostic.contains("malformed after cursor settlement"),
                            "{diagnostic}"
                        );
                    }
                    _ => unreachable!(),
                }
                let after = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
                assert_eq!(&after, state);
                assert_eq!(
                    cursor_at(&manager, &after, scope).await,
                    (revision != 0, revision, position)
                );
                assert_eq!(
                    system_vault_balance(&manager, &after, &payer).await,
                    initial_balance - revision
                );
                assert_eq!(
                    system_vault_balance(&manager, &after, &recipient).await,
                    revision
                );
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_malformed_transitions_do_not_initialize_or_charge() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xa3; 32] });
        let scope = [0xa4; 32];
        let balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let malformed = [
            (0, 0, 0, 1, 0),
            (-1, 0, 0, 1, 0),
            (2, -1, 0, 0, 0),
            (2, 0, -1, 1, 0),
            (2, 0, 2, 1, 0),
            (2, 0, 0, 1, -1),
            (2, 0, 0, 1, 2),
            (2, 0, 0, 0, 0),
            (2, 0, 0, 2, 0),
            (2, i64::MAX, 0, i64::MIN, 0),
        ];
        for (count, revision, position, next_revision, next_position) in malformed {
            let mut deploy = guarded(&payer, &recipient, scope, 0, 0, 1);
            let transport = deploy.env().remove("sys:casper:costFeeCursor").unwrap();
            let mut cursor = RhoList::unapply(&transport).unwrap().remove(0);
            let Some(models::rhoapi::expr::ExprInstance::ETupleBody(tuple)) =
                cursor.exprs[0].expr_instance.as_mut()
            else {
                panic!("guarded deploy must contain a cursor tuple");
            };
            for (field, value) in tuple.ps[1..].iter_mut().zip([
                count,
                revision,
                position,
                next_revision,
                next_position,
            ]) {
                *field = RhoNumber::create_par(value);
            }
            let result = ops
                .play_system_deploy(&initial, &mut CursorApplyProbe {
                    deploy,
                    failure: 0,
                    cursor_override: Some(cursor),
                })
                .await
                .unwrap();
            assert!(matches!(result, SystemDeployResult::PlayFailed { .. }));
            let after = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
            assert_eq!(after, initial);
            assert_eq!(cursor_at(&manager, &after, scope).await, (false, 0, 0));
            assert_eq!(
                system_vault_balance(&manager, &after, &payer).await,
                balance
            );
            assert_eq!(system_vault_balance(&manager, &after, &recipient).await, 0);
        }
        let missing = ops
            .play_system_deploy(&initial, &mut CursorApplyProbe {
                deploy: guarded(&payer, &recipient, scope, 0, 0, 1),
                failure: 0,
                cursor_override: Some(RhoNil::create_par()),
            })
            .await
            .unwrap();
        match missing {
            SystemDeployResult::PlayFailed {
                processed_system_deploy: ProcessedSystemDeploy::Failed { error_msg, .. },
            } => {
                assert_eq!(error_msg, "Monetary fee requires a cursor transition");
            }
            _ => panic!("a missing cursor must reject the monetary fee"),
        }
        let after = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
        assert_eq!(after, initial);
        assert_eq!(cursor_at(&manager, &after, scope).await, (false, 0, 0));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_zero_fee_without_cursor_burns_and_replays() {
    with_runtime_manager(|mut manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xb1; 32] });
        let initial_balance = system_vault_balance(&manager, &initial, &payer).await;
        for burn in [0, 1, 4] {
            let make = || {
                ApplyCostDeploy::new(
                    [0xb2; 32],
                    vec![VaultAllocation::new(payer.to_base58(), 4).unwrap()],
                    vec![VaultSettlement::new(payer.to_base58(), burn, 0).unwrap()],
                    recipient.to_base58(),
                    Blake2b512Random::create_from_bytes(&[0xb2]),
                )
                .unwrap()
            };
            let state = compare_successful_system_deploys(
                &mut manager,
                &genesis,
                &initial,
                &mut make(),
                &mut make(),
                |_| true,
            )
            .await
            .unwrap();
            assert_eq!(
                system_vault_balance(&manager, &state, &payer).await,
                initial_balance - burn
            );
            assert_eq!(system_vault_balance(&manager, &state, &recipient).await, 0);
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_initialization_and_update_replay_identically() {
    with_runtime_manager(|mut manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0x99; 32] });
        let scope = [0x9a; 32];
        assert_cursor_readers(&manager, &initial, scope, None).await;
        let first = compare_successful_system_deploys(
            &mut manager,
            &genesis,
            &initial,
            &mut guarded(&payer, &recipient, scope, 0, 0, 0),
            &mut guarded(&payer, &recipient, scope, 0, 0, 0),
            |_| true,
        )
        .await
        .unwrap();
        assert_eq!(cursor_at(&manager, &first, scope).await, (true, 1, 0));
        assert_cursor_readers(
            &manager,
            &first,
            scope,
            Some(MonetaryCursor::new(1, 0, NonZeroUsize::new(2).unwrap()).unwrap()),
        )
        .await;
        let second = compare_successful_system_deploys(
            &mut manager,
            &genesis,
            &first,
            &mut guarded(&payer, &recipient, scope, 1, 0, 1),
            &mut guarded(&payer, &recipient, scope, 1, 0, 1),
            |_| true,
        )
        .await
        .unwrap();
        assert_eq!(cursor_at(&manager, &second, scope).await, (true, 2, 1));
        assert_eq!(system_vault_balance(&manager, &second, &recipient).await, 2);
        assert_cursor_readers(
            &manager,
            &second,
            scope,
            Some(MonetaryCursor::new(2, 1, NonZeroUsize::new(2).unwrap()).unwrap()),
        )
        .await;
        let replay = manager.spawn_replay_runtime().await;
        let replay_ops = ReplayRuntimeOps::new_from_runtime(replay);
        let reader = acceptance::RuntimeOpsSupplyReader {
            runtime_ops: &replay_ops.runtime_ops,
            pre_state_root: second.as_ref().try_into().unwrap(),
        };
        assert!(reader
            .read_monetary_cursor(scope, NonZeroUsize::new(2).unwrap())
            .await
            .is_err());
    })
    .await
    .unwrap();
}

impl SystemDeployTrait for ConcurrentCursorApply {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"new rl(`rho:registry:lookup`), vaultCh, first, second,
            reservationId(`sys:casper:costReservationId`),
            allocations(`sys:casper:costAllocations`), charges(`sys:casper:costSettlements`),
            feeAddress(`sys:casper:costFeeAddress`), feeCursor(`sys:casper:costFeeCursor`),
            auth(`sys:casper:authToken`), return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *vaultCh) |
          for (@(_, vault) <- vaultCh) {
            match *feeCursor {
              [cursor] => {
                @vault!("applyCost", *reservationId, *allocations, *charges, *feeAddress, cursor, *auth, *first) |
                @vault!("applyCost", *reservationId, *allocations, *charges, *feeAddress, cursor, *auth, *second)
              }
              _ => {
                first!((false, "Invalid monetary cursor transport")) |
                second!((false, "Invalid monetary cursor transport"))
              }
            } |
            for (@(a, _) <- first & @(b, _) <- second) {
              if ((a and not b) or (b and not a)) {
                return!((true, Nil))
              } else {
                return!((false, "Exactly one concurrent settlement must succeed"))
              }
            }
          }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, ()> {
        ApplyCostDeploy::process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.0.rand() }

    fn env(&mut self) -> HashMap<String, Par> { self.0.env() }

    fn return_channel(&mut self) -> Result<Par, CasperError> { self.0.return_channel() }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_vault_concurrent_initializers_charge_exactly_once() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0x97; 32] });
        let scope = [0x98; 32];
        let balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let result = ops
            .play_system_deploy(
                &initial,
                &mut ConcurrentCursorApply(guarded(&payer, &recipient, scope, 0, 0, 0)),
            )
            .await
            .unwrap();
        let state = successful_system_state(result);
        assert_eq!(cursor_at(&manager, &state, scope).await, (true, 1, 0));
        assert_eq!(
            system_vault_balance(&manager, &state, &payer).await,
            balance - 1
        );
        assert_eq!(system_vault_balance(&manager, &state, &recipient).await, 1);
    })
    .await
    .unwrap();
}
