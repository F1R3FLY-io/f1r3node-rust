use crypto::rust::signatures::signed::Cosigner;

use super::*;

fn joint_envelope(
    data: DeployData,
    keys: &[PrivateKey],
    selected_count: usize,
) -> Cosigned<DeployData> {
    let mut members = keys
        .iter()
        .map(|key| {
            (
                Cosigner {
                    pk: Secp256k1.to_public(key),
                    sig: Vec::new().into(),
                    sig_algorithm: Box::new(Secp256k1),
                },
                key,
            )
        })
        .collect::<Vec<_>>();
    members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
    let mut bitmap = vec![0_u8; members.len().div_ceil(8)];
    for index in 0..selected_count {
        bitmap[index / 8] |= 1 << (index % 8);
    }
    let unsigned = members
        .iter()
        .map(|(signer, _)| signer.clone())
        .collect::<Vec<_>>();
    let threshold = u32::try_from(selected_count).unwrap();
    for (signer, key) in members.iter_mut().take(selected_count) {
        let hash = Cosigned::<DeployData>::envelope_signing_hash_for_presence(
            &data,
            &unsigned,
            threshold,
            &bitmap,
            &signer.sig_algorithm.name(),
        )
        .unwrap();
        signer.sig = signer.sig_algorithm.sign(&hash, &key.bytes).into();
    }
    let signers = members.into_iter().map(|(signer, _)| signer).collect();
    let envelope = Cosigned::from_envelope_signed_data_threshold(data, signers, threshold).unwrap();
    envelope.validate_envelope().unwrap();
    assert_eq!(
        envelope.selected_signers_v61().unwrap().len(),
        selected_count
    );
    envelope
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_bound_funded_joint_and_leaves_rotate_one_fee_across_committed_roots() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let keys = genesis_context.genesis_vaults[..3]
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            let deploy_at = |timestamp| {
                let data = construct_deploy::source_deploy(
                    "Nil".to_string(),
                    timestamp,
                    None,
                    None,
                    Some(keys[0].clone()),
                    None,
                    Some(genesis_block.shard_id.clone()),
                )
                .unwrap()
                .data;
                joint_envelope(data, &keys, 3)
            };
            let first = deploy_at(100);
            let joint =
                vault_payer(&sig_to_cost_signature(&accounting::funding_sig(&first)).unwrap())
                    .unwrap();
            let mut payers = first
                .selected_signers_v61()
                .unwrap()
                .into_iter()
                .map(|signer| {
                    vault_payer(
                        &sig_to_cost_signature(&Sig::Ground(accounting::principal_ground_v61(
                            &signer.pk.bytes,
                        )))
                        .unwrap(),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            payers.push(joint.clone());
            payers.sort_by_key(|payer| payer.custody_key);
            assert!(payers
                .windows(2)
                .all(|pair| pair[0].custody_key != pair[1].custody_key));
            let mut current = protocol_mint_to_vault(
                &runtime_manager,
                &genesis_block.body.state.post_state_hash,
                &joint.address,
                16,
                0x76,
            )
            .await;
            let proposer = genesis_context.validator_pks()[0].clone();
            let recipient = VaultAddress::from_public_key(&proposer).unwrap();
            let mut total_fees = vec![0_u64; payers.len()];
            for round in 0..4 {
                let deploy = deploy_at(100 + round);
                let block_data = BlockData {
                    time_stamp: 200 + round,
                    block_number: 2 + round,
                    sender: proposer.clone(),
                    seq_num: i32::try_from(2 + round).unwrap(),
                };
                let initial_recipient =
                    system_vault_balance(&runtime_manager, &current, &recipient).await;
                let admission = runtime_manager
                    .certify_state_bound_admission(
                        &current,
                        vec![deploy],
                        &block_data,
                        &HashMap::new(),
                    )
                    .await
                    .unwrap();
                assert_eq!(admission.outcome().admitted.len(), 1);
                assert!(admission.outcome().rejected.is_empty());
                assert!(admission.outcome().deferred.is_empty());
                let (post, processed, system, _) = runtime_manager
                    .compute_state_with_bonds_cosigned_admitted(admission, Vec::new())
                    .await
                    .unwrap();
                assert_eq!(processed.len(), 1);
                assert!(!processed[0].is_failed);
                let replay = runtime_manager
                    .replay_compute_state(
                        &current,
                        processed.clone(),
                        system,
                        &block_data,
                        None,
                        false,
                    )
                    .await
                    .unwrap();
                assert_eq!(post, replay, "replay must preserve round {round}");
                let certificate = processed[0].authority_funding_certificate.as_ref().unwrap();
                let fee_plan = certificate.fee_plan.as_ref().unwrap();
                assert_eq!(fee_plan.obligation, 1);
                assert_eq!(fee_plan.expected_revision, round);
                assert_eq!(fee_plan.next_revision, round + 1);
                assert_eq!(fee_plan.expected_position, round % 4);
                assert_eq!(fee_plan.next_position, (round + 1) % 4);
                assert_eq!(
                    fee_plan
                        .payer_custodies
                        .iter()
                        .map(|custody| custody.as_ref())
                        .collect::<Vec<&[u8]>>(),
                    payers
                        .iter()
                        .map(|payer| payer.custody_key.as_slice())
                        .collect::<Vec<_>>()
                );
                let witness = processed[0].authority_cost_witness.as_ref().unwrap();
                assert_eq!(
                    certificate
                        .fee_allocation
                        .iter()
                        .map(|resource| resource.amount)
                        .sum::<u64>(),
                    1
                );
                assert_eq!(
                    system_vault_balance(&runtime_manager, &post, &recipient).await
                        - initial_recipient,
                    1
                );
                let mut observed_fee = 0;
                for (index, payer) in payers.iter().enumerate() {
                    assert_ne!(payer.address, recipient);
                    let declared = |resources: &[models::casper::CostAuthorityResourceProto]| {
                        resources
                            .iter()
                            .filter(|resource| resource.key.as_ref() == payer.lane_key.as_slice())
                            .map(|resource| resource.amount)
                            .sum::<u64>()
                    };
                    let fee = declared(&certificate.fee_allocation);
                    let burn = declared(&witness.settlement) + declared(&witness.byte_settlement);
                    let before =
                        system_vault_balance(&runtime_manager, &current, &payer.address).await;
                    let after = system_vault_balance(&runtime_manager, &post, &payer.address).await;
                    assert_eq!(before - after, i64::try_from(burn + fee).unwrap());
                    total_fees[index] += fee;
                    observed_fee += fee;
                }
                assert_eq!(observed_fee, 1);
                current = post;
            }
            assert_eq!(
                total_fees,
                vec![1; 4],
                "authorized funded purses must share residual fees across committed roots"
            );
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_bound_joint_deploy_pays_one_total_fee_through_vault_and_replay() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            assert!(genesis_context.genesis_vaults.len() >= 3);
            let start = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let recipient = VaultAddress::from_public_key(&proposer).unwrap();
            let initial_recipient =
                system_vault_balance(&runtime_manager, &start, &recipient).await;
            let mut fee_mismatches = Vec::new();
            let mut admission_failures = Vec::new();

            for source in ["Nil", "new x in { x!(0) | for (@0 <- x) { Nil } }"] {
            for (members, selected) in [(1, 1), (2, 2), (3, 3), (3, 2)] {
                let keys = genesis_context.genesis_vaults[..members]
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                let data = construct_deploy::source_deploy(
                    source.to_string(),
                    1,
                    None,
                    None,
                    Some(keys[0].clone()),
                    None,
                    Some(genesis_block.shard_id.clone()),
                )
                .unwrap()
                .data;
                let deploy = joint_envelope(data, &keys, selected);
                for signer in deploy.selected_signers_v61().unwrap() {
                    let address = VaultAddress::from_public_key(&signer.pk).unwrap();
                    assert!(system_vault_balance(&runtime_manager, &start, &address).await > 0);
                }
                let mut tampered = deploy.signers().to_vec();
                let selected_index = tampered.iter().position(|s| !s.sig.is_empty()).unwrap();
                tampered[selected_index].sig = vec![0; 64].into();
                assert!(Cosigned::from_envelope_signed_data_threshold(
                    deploy.data().clone(),
                    tampered,
                    u32::try_from(selected).unwrap(),
                )
                .is_err());

                let compound = vault_payer(
                    &sig_to_cost_signature(&accounting::funding_sig(&deploy)).unwrap(),
                )
                .unwrap();
                if selected > 1 {
                    assert_eq!(
                        system_vault_balance(&runtime_manager, &start, &compound.address).await,
                        0,
                        "the regression must use leaf balances without a funded compound pool"
                    );
                }
                let block_data = BlockData {
                    time_stamp: 2,
                    block_number: 2,
                    sender: proposer.clone(),
                    seq_num: 2,
                };
                let admission = runtime_manager
                    .certify_state_bound_admission(
                        &start,
                        vec![deploy.clone()],
                        &block_data,
                        &HashMap::new(),
                    )
                    .await
                    .unwrap();
                if admission.outcome().admitted.len() != 1 {
                    admission_failures.push((
                        source,
                        members,
                        selected,
                        admission.outcome().rejected.len(),
                        admission.outcome().deferred.len(),
                    ));
                    continue;
                }
                assert!(admission.outcome().rejected.is_empty());
                assert!(admission.outcome().deferred.is_empty());
                let (post, processed, system, _) = runtime_manager
                    .compute_state_with_bonds_cosigned_admitted(admission, Vec::new())
                    .await
                    .unwrap();
                assert_eq!(processed.len(), 1);
                assert!(!processed[0].is_failed);
                let replay = runtime_manager
                    .replay_compute_state(&start, processed.clone(), system, &block_data, None, false)
                    .await
                    .unwrap();
                assert_eq!(post, replay, "replay root for {members}/{selected}");
                let certificate = processed[0].authority_funding_certificate.as_ref().unwrap();
                let witness = processed[0].authority_cost_witness.as_ref().unwrap();
                assert_eq!(certificate.fee_recipient, proposer.bytes);
                let mut observed_total_loss = 0_i64;
                for signer in deploy.signers() {
                    let address = VaultAddress::from_public_key(&signer.pk).unwrap();
                    assert_ne!(address, recipient);
                    let initial = system_vault_balance(&runtime_manager, &start, &address).await;
                    let final_balance = system_vault_balance(&runtime_manager, &post, &address).await;
                    assert_eq!(
                        final_balance,
                        system_vault_balance(&runtime_manager, &replay, &address).await
                    );
                    let lane = Sig::Ground(accounting::principal_ground_v61(&signer.pk.bytes))
                        .lane_hash();
                    let declared = witness.settlement.iter()
                        .chain(&witness.byte_settlement)
                        .chain(&certificate.fee_allocation)
                        .filter(|resource| resource.key.as_ref() == lane.as_slice())
                        .map(|resource| resource.amount)
                        .sum::<u64>();
                    let observed_loss = initial - final_balance;
                    assert_eq!(observed_loss, i64::try_from(declared).unwrap());
                    if signer.sig.is_empty() {
                        assert_eq!(observed_loss, 0, "an unsigned policy member cannot pay");
                    }
                    observed_total_loss += observed_loss;
                }
                let burn = witness.settlement.iter()
                    .chain(&witness.byte_settlement)
                    .map(|resource| resource.amount)
                    .sum::<u64>();
                let recipient_gain =
                    system_vault_balance(&runtime_manager, &post, &recipient).await - initial_recipient;
                assert_eq!(
                    recipient_gain,
                    system_vault_balance(&runtime_manager, &replay, &recipient).await - initial_recipient
                );
                assert_eq!(observed_total_loss, i64::try_from(burn).unwrap() + recipient_gain);
                let declared_fee = certificate.fee_allocation.iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>();
                assert_eq!(recipient_gain, i64::try_from(declared_fee).unwrap());
                if recipient_gain != 1 {
                    fee_mismatches.push((source, members, selected, recipient_gain));
                }
            }
            }
            assert!(
                fee_mismatches.is_empty() && admission_failures.is_empty(),
                "funded deploys must admit and transfer one total monetary fee; (source, members, selected, rejected, deferred): {admission_failures:?}; (source, members, selected, actual fee): {fee_mismatches:?}"
            );
        },
    )
    .await
    .unwrap();
}
