use super::*;

fn phlo_request(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    burn: i64,
    fee: i64,
) -> ApplyPhloCostDeploy {
    let count = NonZeroUsize::new(2).unwrap();
    ApplyPhloCostDeploy::new(
        ApplyCostDeploy::new(
            [0xe1; 32],
            vec![VaultAllocation::new(payer.to_base58(), burn + fee + 1).unwrap()],
            vec![VaultSettlement::new(payer.to_base58(), burn, fee).unwrap()],
            recipient.to_base58(),
            Blake2b512Random::create_from_bytes(&[0xe1]),
        )
        .unwrap(),
        (burn > 0).then(|| {
            MonetaryCursorTransition::new([0xe2; 32], MonetaryCursor::INITIAL, 1, count).unwrap()
        }),
        (fee > 0).then(|| {
            MonetaryCursorTransition::new([0xe3; 32], MonetaryCursor::INITIAL, 1, count).unwrap()
        }),
        count,
    )
    .unwrap()
}

struct PhloProbe {
    deploy: ApplyPhloCostDeploy,
    mode: i64,
    resource: Option<Par>,
    fee: Option<Par>,
}

impl SystemDeployTrait for PhloProbe {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"new rl(`rho:registry:lookup`), vaultCh, first, second,
            reservationId(`sys:casper:costReservationId`),
            allocations(`sys:casper:costAllocations`), charges(`sys:casper:costSettlements`),
            feeAddress(`sys:casper:costFeeAddress`), resourceCursor(`sys:casper:costResourceCursor`),
            feeCursor(`sys:casper:costFeeCursor`), mode(`sys:casper:probeMode`),
            auth(`sys:casper:authToken`), return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *vaultCh) |
          for (@(_, vault) <- vaultCh) {
            match (*resourceCursor, *feeCursor) {
              ([resource], [fee]) => {
                @vault!("applyPhloCost", *reservationId, *allocations, *charges, *feeAddress, resource, fee, *auth, *first) |
                if (*mode == 4) {
                  @vault!("applyPhloCost", *reservationId, *allocations, *charges, *feeAddress, resource, fee, *auth, *second)
                } else {
                  if (*mode == 7) {
                    @vault!("applyPhloCost", *reservationId, *allocations, *charges, *feeAddress, fee, resource, *auth, *second)
                  } else { second!((false, Nil)) }
                }
              }
            } |
            for (@a <- first & @(b, _) <- second) {
              if (*mode == 4 or *mode == 7) {
                match a {
                  (ok, _) => { return!(((ok and not b) or (b and not ok), Nil)) }
                }
              } else {
                match a {
                  (true, _) => {
                    match *mode {
                      1 => { return!((false, "rejected after funding settlement")) }
                      2 => { return!(1 / 0) }
                      3 => { return!("malformed after funding settlement") }
                      5 => { Nil }
                      _ => { return!(a) }
                    }
                  }
                  _ => { return!(a) }
                }
              }
            }
          }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, ()> {
        ApplyPhloCostDeploy::process_result(value)
    }
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn rand(&self) -> Blake2b512Random { self.deploy.rand() }
    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = self.deploy.env();
        env.insert(
            "sys:casper:probeMode".to_string(),
            RhoNumber::create_par(self.mode),
        );
        for (key, value) in [
            ("sys:casper:costResourceCursor", &self.resource),
            ("sys:casper:costFeeCursor", &self.fee),
        ] {
            if let Some(value) = value {
                env.insert(key.to_string(), RhoList::create_par(vec![value.clone()]));
            }
        }
        if self.mode == 6 {
            env.insert(
                "sys:casper:authToken".to_string(),
                Par::default().with_unforgeables(vec![models::rhoapi::GUnforgeable {
                    unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                        models::rhoapi::GPrivate { id: vec![0xee; 32] },
                    )),
                }]),
            );
        }
        env
    }
    fn return_channel(&mut self) -> Result<Par, CasperError> { self.deploy.return_channel() }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_optional_roles_replay_without_initializing_unused_scopes() {
    with_runtime_manager(|mut manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe1; 32] });
        let balance = system_vault_balance(&manager, &initial, &payer).await;
        for (burn, fee) in [(0, 0), (2, 0), (0, 1), (2, 1)] {
            let state = compare_successful_system_deploys(
                &mut manager,
                &genesis,
                &initial,
                &mut phlo_request(&payer, &recipient, burn, fee),
                &mut phlo_request(&payer, &recipient, burn, fee),
                |_| true,
            )
            .await
            .unwrap();
            assert_eq!(
                cursor_at(&manager, &state, [0xe2; 32]).await,
                if burn > 0 {
                    (true, 1, 1)
                } else {
                    (false, 0, 0)
                }
            );
            assert_eq!(
                cursor_at(&manager, &state, [0xe3; 32]).await,
                if fee > 0 { (true, 1, 1) } else { (false, 0, 0) }
            );
            assert_eq!(
                system_vault_balance(&manager, &state, &payer).await,
                balance - burn - fee
            );
            assert_eq!(
                system_vault_balance(&manager, &state, &recipient).await,
                fee
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_outer_failures_restore_both_cursors_and_balances() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe1; 32] });
        let balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        for mode in [1, 2, 3, 5, 6] {
            let result = ops
                .play_system_deploy(&initial, &mut PhloProbe {
                    deploy: phlo_request(&payer, &recipient, 2, 1),
                    mode,
                    resource: None,
                    fee: None,
                })
                .await;
            let diagnostic = match &result {
                Err(error) => format!("{error:?}"),
                Ok(SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                }) => format!("{processed_system_deploy:?}"),
                Ok(SystemDeployResult::PlaySucceeded { .. }) => "unexpected success".to_string(),
            };
            if mode == 2 || mode == 5 {
                assert!(result.is_err(), "mode {mode}: {diagnostic}");
            } else {
                assert!(
                    matches!(result, Ok(SystemDeployResult::PlayFailed { .. })),
                    "mode {mode}: {diagnostic}"
                );
            }
            let expected = match mode {
                1 => "rejected after funding settlement",
                2 => "division",
                3 => "malformed after funding settlement",
                5 => "consumefailed",
                6 => "unauthorized cost application",
                _ => unreachable!(),
            };
            assert!(
                diagnostic.to_lowercase().contains(expected),
                "mode {mode}: {diagnostic}"
            );
            let after = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
            assert_eq!(after, initial, "mode {mode}");
            assert_eq!(cursor_at(&manager, &after, [0xe2; 32]).await, (false, 0, 0));
            assert_eq!(cursor_at(&manager, &after, [0xe3; 32]).await, (false, 0, 0));
            assert_eq!(
                system_vault_balance(&manager, &after, &payer).await,
                balance
            );
            assert_eq!(system_vault_balance(&manager, &after, &recipient).await, 0);
        }
        let unfunded =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe4; 32] });
        assert!(matches!(
            ops.play_system_deploy(&initial, &mut phlo_request(&unfunded, &recipient, 2, 1))
                .await
                .unwrap(),
            SystemDeployResult::PlayFailed { .. }
        ));
        assert_eq!(
            ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
            initial
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_concurrent_pair_requests_charge_once() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe1; 32] });
        let balance = system_vault_balance(&manager, &initial, &payer).await;
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        for mode in [4, 7] {
            let state = successful_system_state(
                ops.play_system_deploy(&initial, &mut PhloProbe {
                    deploy: phlo_request(&payer, &recipient, 2, 1),
                    mode,
                    resource: None,
                    fee: None,
                })
                .await
                .unwrap(),
            );
            assert_eq!(cursor_at(&manager, &state, [0xe2; 32]).await, (true, 1, 1));
            assert_eq!(cursor_at(&manager, &state, [0xe3; 32]).await, (true, 1, 1));
            assert_eq!(
                system_vault_balance(&manager, &state, &payer).await,
                balance - 3
            );
            assert_eq!(system_vault_balance(&manager, &state, &recipient).await, 1);
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_mixed_presence_rollback_rejects_each_stale_position() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe1; 32] });
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        for (burn, fee, role) in [
            (2, 0, "sys:casper:costResourceCursor"),
            (0, 1, "sys:casper:costFeeCursor"),
        ] {
            let partial = successful_system_state(
                ops.play_system_deploy(&initial, &mut phlo_request(&payer, &recipient, burn, fee))
                    .await
                    .unwrap(),
            );
            assert_eq!(
                cursor_at(&manager, &partial, [0xe2; 32]).await,
                if burn > 0 {
                    (true, 1, 1)
                } else {
                    (false, 0, 0)
                }
            );
            assert_eq!(
                cursor_at(&manager, &partial, [0xe3; 32]).await,
                if fee > 0 { (true, 1, 1) } else { (false, 0, 0) }
            );
            let mut request = phlo_request(&payer, &recipient, 2, 1);
            assert!(matches!(
                ops.play_system_deploy(&partial, &mut request)
                    .await
                    .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
                partial
            );
            let env = request.env();
            let mut stale_position = RhoList::unapply(&env[role]).unwrap().remove(0);
            let Some(models::rhoapi::expr::ExprInstance::ETupleBody(tuple)) =
                stale_position.exprs[0].expr_instance.as_mut()
            else {
                panic!("expected cursor tuple")
            };
            tuple.ps[2] = RhoNumber::create_par(1);
            tuple.ps[4] = RhoNumber::create_par(2);
            let (resource, fee) = if burn > 0 {
                (Some(stale_position), None)
            } else {
                (None, Some(stale_position))
            };
            assert!(matches!(
                ops.play_system_deploy(&partial, &mut PhloProbe {
                    deploy: request,
                    mode: 0,
                    resource,
                    fee
                })
                .await
                .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
                partial
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phlo_cursor_vault_rejects_aliased_missing_and_malformed_cursor_pairs() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payer = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let recipient =
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xe1; 32] });
        let mut request = phlo_request(&payer, &recipient, 2, 1);
        let env = request.env();
        let resource = RhoList::unapply(&env["sys:casper:costResourceCursor"])
            .unwrap()
            .remove(0);
        let fee = RhoList::unapply(&env["sys:casper:costFeeCursor"])
            .unwrap()
            .remove(0);
        let mut cases = vec![
            (Some(RhoNil::create_par()), None),
            (None, Some(RhoNil::create_par())),
            (None, Some(resource.clone())),
        ];
        for role in 0..2 {
            for (field, value) in [
                (1, 0),
                (1, 3),
                (2, -1),
                (2, i64::MAX),
                (3, 2),
                (4, 0),
                (4, 2),
                (5, -1),
                (5, 2),
            ] {
                let mut malformed = if role == 0 {
                    resource.clone()
                } else {
                    fee.clone()
                };
                let Some(models::rhoapi::expr::ExprInstance::ETupleBody(tuple)) =
                    malformed.exprs[0].expr_instance.as_mut()
                else {
                    panic!("expected cursor tuple")
                };
                tuple.ps[field] = RhoNumber::create_par(value);
                cases.push(if role == 0 {
                    (Some(malformed), None)
                } else {
                    (None, Some(malformed))
                });
            }
        }
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        for (resource, fee) in cases {
            assert!(matches!(
                ops.play_system_deploy(&initial, &mut PhloProbe {
                    deploy: phlo_request(&payer, &recipient, 2, 1),
                    mode: 0,
                    resource,
                    fee,
                })
                .await
                .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
                initial
            );
        }
    })
    .await
    .unwrap();
}
