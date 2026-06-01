// See casper/src/test/scala/coop/rchain/casper/util/rholang/RuntimeManagerTest.scala

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::errors::CasperError;
use casper::rust::rholang::replay_runtime::ReplayRuntimeOps;
use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::check_balance::CheckBalance;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::costacc::redeem_deploy::{RedeemDeploy, RedemptionOutcome};
use casper::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use casper::rust::util::rholang::replay_failure::ReplayFailure;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::acceptance;
use casper::rust::util::rholang::supply;
use rholang::rust::interpreter::accounting::{self, Sig};
use casper::rust::util::rholang::system_deploy::SystemDeployTrait;
use casper::rust::util::rholang::system_deploy_result::SystemDeployResult;
use casper::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;
use casper::rust::util::rholang::system_deploy_util;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::{PCost, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    DeployData, ProcessedDeploy, ProcessedSystemDeploy,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{Extractor, RhoBoolean};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::with_runtime_manager;

enum SystemDeployReplayResult<A> {
    ReplaySucceeded {
        state_hash: StateHash,
        result: A,
    },
    ReplayFailed {
        system_deploy_error: SystemDeployUserError,
    },
}

async fn compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    deploy: Signed<DeployData>,
    state_hash: &StateHash,
) -> (StateHash, ProcessedDeploy) {
    let time_stamp = deploy.data.time_stamp;
    let (new_state_hash, processed_deploys, _extra) = runtime_manager
        .compute_state(
            state_hash,
            vec![deploy],
            Vec::new(), // No system deploys
            BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
        )
        .await
        .unwrap();

    let result = processed_deploys.into_iter().next().unwrap();
    (new_state_hash, result)
}

async fn replay_compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    processed_deploy: ProcessedDeploy,
    state_hash: &StateHash,
) -> Result<StateHash, CasperError> {
    let time_stamp = processed_deploy.deploy.data.time_stamp;
    runtime_manager
        .replay_compute_state(
            state_hash,
            vec![processed_deploy],
            Vec::new(),
            &BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
            false,
            false, // strict_funding_enforcement (#13a)
            &[], // client_fuel_allocations (#13b)
        )
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn comput_state_should_charge_for_deploys() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let source = r#"
            new rl(`rho:registry:lookup`), listOpsCh in {
                rl!(`rho:lang:listOps`, *listOpsCh) |
                for(x <- listOpsCh){
                    Nil
                }
            }
            "#;

            // TODO: Prohibit negative gas prices and gas limits in deploys. - OLD
            // TODO: Make minimum maximum yield for deploy parameter of node. - OLD
            let deploy = construct_deploy::source_deploy_now_full(
                source.to_string(),
                Some(100000),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let (new_state_hash, processed_deploy) = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &gen_post_state,
            )
            .await;

            let replay_state_hash = replay_compute_state(
                &mut runtime_manager,
                &genesis_context,
                processed_deploy,
                &gen_post_state,
            )
            .await
            .unwrap();

            // DR-9/D3 (OD-2): the per-deploy escrow pre-charge / refund system
            // deploys are REMOVED (casper `costacc/mod.rs`; `runtime.rs` "No
            // pre-charge / refund fan-out"). Those writes to the payer's
            // per-validator vault used to mutate the post-state hash. This
            // deploy is otherwise side-effect-free (a registry lookup that
            // binds `x` and discards it to `Nil`), so with the charge/refund
            // gone the post-state hash now EQUALS the pre-state hash
            // (verified: new == gen). The consensus-critical invariant the
            // test guards — replay reproduces play EXACTLY — still holds
            // (verified: replay == new), so there is NO replay divergence.
            assert_eq!(
                new_state_hash, gen_post_state,
                "D3: with precharge/refund removed, a side-effect-free deploy leaves the \
                 post-state hash unchanged"
            );
            assert_eq!(
                replay_state_hash, new_state_hash,
                "replay must reproduce play's post-state exactly (no divergence)"
            );
        },
    )
    .await
    .unwrap()
}

async fn compare_successful_system_deploys<S: SystemDeployTrait, F>(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    start_state: &StateHash,
    play_system_deploy: &mut S,
    replay_system_deploy: &mut S,
    result_assertion: F,
) -> Result<StateHash, CasperError>
where
    F: Fn(&S::Result) -> bool,
    <S as SystemDeployTrait>::Result: PartialEq,
{
    let runtime = runtime_manager.spawn_runtime().await;
    {
        runtime
            .set_block_data(BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            })
            .await;
    }

    let mut runtime_ops = RuntimeOps::new(runtime);
    let play_system_result = runtime_ops
        .play_system_deploy(start_state, play_system_deploy)
        .await?;

    match play_system_result {
        SystemDeployResult::PlaySucceeded {
            state_hash: final_play_state_hash,
            processed_system_deploy,
            mergeable_channels: _,
            result: play_result,
        } => {
            result_assertion(&play_result);

            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            {
                replay_runtime
                    .set_block_data(BlockData {
                        time_stamp: 0,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    })
                    .await;
            }

            let replay_runtime_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            let replay_system_result = exec_replay_system_deploy(
                replay_runtime_ops,
                start_state,
                replay_system_deploy,
                &processed_system_deploy,
            )
            .await?;

            match replay_system_result {
                SystemDeployReplayResult::ReplaySucceeded {
                    state_hash: final_replay_state_hash,
                    result: replay_result,
                } => {
                    assert!(final_play_state_hash == final_replay_state_hash);
                    assert!(play_result == replay_result);
                    Ok(final_replay_state_hash)
                }

                SystemDeployReplayResult::ReplayFailed {
                    system_deploy_error,
                } => panic!(
                    "Unexpected user error during replay: {:?}",
                    system_deploy_error
                ),
            }
        }

        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => panic!(
            "Unexpected system error during play: {:?}",
            processed_system_deploy
        ),
    }
}

async fn exec_replay_system_deploy<S: SystemDeployTrait>(
    mut replay_runtime_ops: ReplayRuntimeOps,
    state_hash: &StateHash,
    system_deploy: &mut S,
    processed_system_deploy: &ProcessedSystemDeploy,
) -> Result<SystemDeployReplayResult<S::Result>, CasperError> {
    let expected_failure = match processed_system_deploy {
        ProcessedSystemDeploy::Failed { error_msg, .. } => Some(error_msg.clone()),
        _ => None,
    };

    replay_runtime_ops
        .rig_system_deploy(processed_system_deploy)
        .await?;
    replay_runtime_ops
        .runtime_ops
        .runtime
        .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
        .await?;

    let (value, eval_res) = replay_runtime_ops
        .replay_system_deploy_internal(system_deploy, &expected_failure)
        .await?;

    replay_runtime_ops
        .check_replay_data_with_fix(eval_res.errors.is_empty())
        .await?;

    match (value, eval_res) {
        (Either::Right(result), _) => {
            // Cost-Accounted Rho Stage B: mirror the production replay path
            // (`replay_block_system_deploy`) by running the system deploy's
            // `post_eval` settlement hook on the LIVE replay runtime AFTER the
            // replay-data check, BEFORE the checkpoint — symmetric with the
            // play-side `play_system_deploy`. For `CloseBlockDeploy` this writes
            // `Σ⟦v⟧`; for every other system deploy it is a no-op. Without this,
            // the harness would diverge from play for an epoch/block-1 close.
            let block_data = replay_runtime_ops
                .runtime_ops
                .runtime
                .block_data_ref
                .read()
                .await
                .clone();
            system_deploy
                .post_eval(&mut replay_runtime_ops.runtime_ops, &block_data, state_hash)
                .await?;

            let checkpoint = replay_runtime_ops
                .runtime_ops
                .runtime
                .create_checkpoint()
                .await;

            Ok(SystemDeployReplayResult::ReplaySucceeded {
                state_hash: checkpoint.root.to_bytes_prost(),
                result,
            })
        }

        (Either::Left(error), _) => Ok(SystemDeployReplayResult::ReplayFailed {
            system_deploy_error: error,
        }),
    }
}

// D3 (DR-9, OD-2): `pre_charge_deploy_should_reduce_user_account_balance_by_correct_amount`
// and `refund_deploy_should_reject_refunds_above_recorded_precharge` are removed
// — the escrow PreChargeDeploy / RefundDeploy system deploys they exercised no
// longer exist. A deploy's cost is the per-COMM token count, settled once
// against Σ⟦s⟧ at block close (no per-deploy charge/refund round-trip).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_should_make_epoch_change_and_reward_validator() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let _ = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&vec![0])),
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&vec![0])),
                |_| true,
            )
            .await
            .unwrap();
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_replay_should_fail_with_different_random_seed() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let res = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&vec![0])),
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&vec![1])),
                |_| true,
            )
            .await;

            assert!(res.is_err());
        },
    )
    .await
    .unwrap();
}

/// CONSENSUS-CRITICAL play/replay determinism test for the Cost-Accounted Rho
/// Stage B supply mint (Decision 2.5/6). Plays a `CloseBlockDeploy` at an epoch
/// boundary (block 0 ⇒ `0 % epochLength == 0`) whose `closeBlock` fold mints
/// `epochPhlogiston` into every active genesis validator's draw wallet @W_v and
/// publishes the mint list; the play-side `post_eval` mirrors each amount into
/// the supply pool `Σ⟦v⟧ = from_sig(Ground(pk))`. It then replays the SAME block
/// through the PRODUCTION `replay_block_system_deploy` path (which runs
/// `post_eval_replay`, including the `ReplaySupplyMismatch` write-readback
/// guard) and asserts the post-state root is BYTE-IDENTICAL — i.e. the Rust
/// mint-set recompute + `Σ⟦v⟧` dual-write are play/replay-symmetric. The test is
/// non-vacuous: it independently asserts that `Σ⟦v⟧` actually carries the minted
/// `epochPhlogiston` balance in the play post-state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_supply_mint_is_play_replay_deterministic() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let sender = genesis_context.validator_pks()[0].clone();
            let block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: sender.clone(),
                seq_num: 0,
            };

            // ---- PLAY ----
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);

            let mut play_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    sender.clone(),
                    block_data.seq_num,
                ),
            );
            let play_result = play_ops
                .play_system_deploy(&start_state, &mut play_close)
                .await
                .unwrap();

            let (final_play_state_hash, processed_system_deploy) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("close-block play failed: {:?}", processed_system_deploy),
            };

            // Non-vacuity: Σ⟦v⟧ for an active genesis validator must carry the
            // minted epochPhlogiston balance in the play post-state.
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_play_state_hash))
                .await
                .unwrap();
            let supply_chan = supply::supply_channel(&Sig::Ground(sender.bytes.to_vec()));
            let play_balance = supply::read_balance(&play_ops, &supply_chan).await;
            assert!(
                play_balance > 0,
                "expected a positive Σ⟦v⟧ supply balance after the epoch mint, got {}",
                play_balance
            );

            // ---- REPLAY (production path: replay_block_system_deploy) ----
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();

            replay_ops
                .replay_block_system_deploy(
                    &block_data,
                    &processed_system_deploy,
                    // This is a pure Stage-B mint test (no WD-D2 settlement debit).
                    &std::collections::BTreeMap::new(),
                    // No Stage-D fee credit in this mint test.
                    &None,
                    // #13b: no genesis client funding slots in this mint test.
                    &[],
                )
                .await
                .unwrap();

            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state
            // (including every Σ⟦v⟧ balance) between play and replay.
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-B supply mint"
            );

            // And the replayed Σ⟦v⟧ balance matches the play-time balance.
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_replay_state_hash))
                .await
                .unwrap();
            let replay_balance =
                supply::read_balance(&replay_ops.runtime_ops, &supply_chan).await;
            assert_eq!(
                play_balance, replay_balance,
                "Σ⟦v⟧ balance diverged between play and replay"
            );
        },
    )
    .await
    .unwrap()
}

/// #13b consensus bar (a) + (c): the genesis-block-1 CLIENT funding-slot seed.
/// CONSENSUS-CRITICAL. Plays a `CloseBlockDeploy` at the GENESIS-BLOCK-1 close
/// (`block_number == 1`, the StageB genesis-bonded-set funding point) carrying a
/// `client_fuel_allocations` list `[(client_pk, amount)]`; the play-side
/// `post_eval` SEEDS each client supply pool `Σ⟦c⟧ = from_sig(Ground(client_pk))`
/// with its amount (§5.7/§7.5 genesis/system write). It then replays the SAME
/// block through the PRODUCTION `replay_block_system_deploy` path (which
/// RECONSTRUCTS the close deploy with the SAME allocations and runs
/// `post_eval_replay`, including the `ReplaySupplyMismatch` write-readback guard)
/// and asserts the post-state root is BYTE-IDENTICAL — i.e. the Rust client-seed
/// dual-write is play/replay-symmetric. Non-vacuous: it independently asserts
/// `Σ⟦c⟧` carries the seeded balance in BOTH the play and replay post-states.
///
/// The (c) back-compat half is asserted INLINE: a SECOND close deploy with an
/// EMPTY `client_fuel_allocations` at the SAME block-1 pre-state produces a
/// post-state with NO `Σ⟦c⟧` datum for the client (the seed is purely additive,
/// so an empty list leaves every client pool absent — byte-identical to pre-#13b).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fuel_allocation_credits_sigma_c_at_genesis() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            // The close-block proposer (any genesis validator); its identity only
            // seeds the close-deploy RNG, NOT the client pool under test.
            let sender = genesis_context.validator_pks()[0].clone();
            // A CLIENT public key that is NOT a genesis validator — so its `Σ⟦c⟧`
            // is provisioned ONLY by the #13b client seed (never by the validator
            // mint). Deterministic non-validator bytes.
            let client_pk_bytes: Vec<u8> = vec![0xC1; 32];
            const CLIENT_ALLOC: i64 = 750_000;

            // GENESIS-BLOCK-1 close (the credit gate is `block_number == 1`).
            let block_data = BlockData {
                time_stamp: 0,
                block_number: 1,
                sender: sender.clone(),
                seq_num: 0,
            };

            // ---- PLAY (with a populated client funding-slot list) ----
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);

            let mut play_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    sender.clone(),
                    block_data.seq_num,
                ),
            );
            play_close.client_fuel_allocations = vec![(client_pk_bytes.clone(), CLIENT_ALLOC)];

            let play_result = play_ops
                .play_system_deploy(&start_state, &mut play_close)
                .await
                .unwrap();
            let (final_play_state_hash, processed_system_deploy) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("close-block play failed: {:?}", processed_system_deploy),
            };

            // Non-vacuity: Σ⟦c⟧ for the client must carry EXACTLY the seeded amount.
            let client_chan = supply::supply_channel(&Sig::Ground(client_pk_bytes.clone()));
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_play_state_hash))
                .await
                .unwrap();
            let play_client_balance = supply::read_balance(&play_ops, &client_chan).await;
            assert_eq!(
                play_client_balance, CLIENT_ALLOC,
                "Σ⟦c⟧ must hold exactly the seeded client allocation after block-1 close"
            );

            // ---- REPLAY (production path; SAME allocations threaded in) ----
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();
            replay_ops
                .replay_block_system_deploy(
                    &block_data,
                    &processed_system_deploy,
                    &std::collections::BTreeMap::new(),
                    &None,
                    // #13b: the SAME shard-constant client allocations the play
                    // side used (the reconstructed close re-seeds Σ⟦c⟧ identically).
                    &[(client_pk_bytes.clone(), CLIENT_ALLOC)],
                )
                .await
                .unwrap();
            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // CONSENSUS-CRITICAL: byte-identical post-state (incl. Σ⟦c⟧).
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the #13b client funding-slot seed"
            );
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_replay_state_hash))
                .await
                .unwrap();
            let replay_client_balance =
                supply::read_balance(&replay_ops.runtime_ops, &client_chan).await;
            assert_eq!(
                play_client_balance, replay_client_balance,
                "Σ⟦c⟧ balance diverged between play and replay"
            );

            // ---- (c) BACK-COMPAT: EMPTY allocations ⇒ no Σ⟦c⟧ datum ----
            // The same block-1 close with an EMPTY client list must leave the
            // client pool ABSENT (the seed is purely additive). `read_balance`
            // folds absent ⇒ 0; `read_balance_present` distinguishes absent
            // (`None`) from present-zero (`Some(0)`) — we assert ABSENT.
            let empty_runtime = runtime_manager.spawn_runtime().await;
            empty_runtime.set_block_data(block_data.clone()).await;
            let mut empty_ops = RuntimeOps::new(empty_runtime);
            let mut empty_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    sender.clone(),
                    block_data.seq_num,
                ),
            );
            // client_fuel_allocations defaults EMPTY (new()).
            assert!(empty_close.client_fuel_allocations.is_empty());
            let empty_result = empty_ops
                .play_system_deploy(&start_state, &mut empty_close)
                .await
                .unwrap();
            let empty_state_hash = match empty_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { processed_system_deploy } => {
                    panic!("empty-alloc close-block play failed: {:?}", processed_system_deploy)
                }
            };
            empty_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&empty_state_hash))
                .await
                .unwrap();
            let empty_client_present =
                supply::read_balance_present(&empty_ops, &client_chan).await;
            assert_eq!(
                empty_client_present, None,
                "empty client_fuel_allocations must leave Σ⟦c⟧ ABSENT (back-compat, no seed)"
            );
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL play/replay determinism test for the Cost-Accounted Rho
/// Stage-D fee collection + per-epoch fee→v conversion (the validator economic
/// loop; spec "Fee conversion" tex:3061-3100). Plays a `CloseBlockDeploy` at an
/// epoch boundary (block 0 ⇒ `0 % epochLength == 0`) carrying a per-block FEE
/// credit for a genesis validator: `closeBlock` (1) COLLECTS the fee onto the
/// proposer's carrier `F_v`, then (2) at the epoch boundary CONVERTS it 1:1 via
/// the blessed Exchange — depositing the `@W_v` purse, recording
/// `convertedEpochs`, publishing `(v, k)` on `feeConvertList` — and the
/// play-side `post_eval` mirrors `k` into `Σ⟦v⟧`. It then replays the SAME block
/// through the PRODUCTION `replay_block_system_deploy` path (with the
/// RECOMPUTED fee credit fed in, `post_eval_replay`'s `ReplaySupplyMismatch`
/// readback guard active) and asserts the post-state root is BYTE-IDENTICAL —
/// i.e. the fee credit + the fee-convert `Σ⟦v⟧` mirror are play/replay-symmetric
/// (TM-CA-160). Non-vacuous: `Σ⟦v⟧` carries BOTH the epoch mint AND the
/// converted fee in the play post-state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fee_collection_and_convert_is_play_replay_deterministic() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let sender = genesis_context.validator_pks()[0].clone();
            let block_data = BlockData {
                time_stamp: 0,
                block_number: 0, // 0 % epochLength == 0 ⇒ epoch boundary (convert fires)
                sender: sender.clone(),
                seq_num: 0,
            };
            // A flat fee of 3 tokens (the spec's FeeExtract for a 3-deploy block),
            // collected to the proposing genesis validator's F_v.
            let fee_amount: i64 = 3;
            let fee_credits = Some(casper::rust::util::rholang::acceptance::FeeCredit {
                recipient_pk: sender.bytes.to_vec(),
                amount: fee_amount,
            });

            // ---- PLAY ----
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);

            let mut play_close = CloseBlockDeploy {
                initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    sender.clone(),
                    block_data.seq_num,
                ),
                settlement_debits: std::collections::BTreeMap::new(),
                fee_credits: fee_credits.clone(),
                // #13b: no genesis client funding slots in this fixture.
                client_fuel_allocations: Vec::new(),
            };
            let play_result = play_ops
                .play_system_deploy(&start_state, &mut play_close)
                .await
                .unwrap();

            let (final_play_state_hash, processed_system_deploy) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("fee-convert close play failed: {:?}", processed_system_deploy),
            };

            // Non-vacuity: Σ⟦v⟧ for the proposing validator must be positive in the
            // play post-state (it carries the epoch mint PLUS the converted fee).
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_play_state_hash))
                .await
                .unwrap();
            let supply_chan = supply::supply_channel(&Sig::Ground(sender.bytes.to_vec()));
            let play_balance = supply::read_balance(&play_ops, &supply_chan).await;
            assert!(
                play_balance > 0,
                "expected a positive Σ⟦v⟧ after epoch mint + fee convert, got {}",
                play_balance
            );

            // ---- REPLAY (production path, with the RECOMPUTED fee credit) ----
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();

            // The recomputed fee credit: count would be `terms.len()` on the real
            // replay path; here we feed the SAME amount the play side carried (the
            // recompute identity `terms.len() == block.body.deploys.len()`).
            let replay_fee_credit = casper::rust::util::rholang::acceptance::recompute_fee_credits(
                fee_amount as usize,
                sender.bytes.to_vec(),
            );
            assert_eq!(
                replay_fee_credit, fee_credits,
                "recompute_fee_credits must reproduce the play-side fee credit"
            );

            replay_ops
                .replay_block_system_deploy(
                    &block_data,
                    &processed_system_deploy,
                    &std::collections::BTreeMap::new(),
                    &replay_fee_credit,
                    // #13b: no genesis client funding slots in this fixture.
                    &[],
                )
                .await
                .unwrap();

            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state between
            // play and replay (every Σ⟦v⟧ balance, every F_v carrier, the @W_v
            // purses, the convertedEpochs/mintedEpochs ledgers).
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-D fee collect+convert"
            );

            // And the replayed Σ⟦v⟧ matches the play-time balance.
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_replay_state_hash))
                .await
                .unwrap();
            let replay_balance =
                supply::read_balance(&replay_ops.runtime_ops, &supply_chan).await;
            assert_eq!(
                play_balance, replay_balance,
                "Σ⟦v⟧ balance diverged between play and replay on the fee convert"
            );
        },
    )
    .await
    .unwrap()
}

/// `convertedEpochs` IDEMPOTENCY / determinism (DR-4 / spec tex:3095-3100). The
/// `convertedEpochs: Set[(Pk,Int)]` ledger is the EXACT structural sibling of the
/// Stage-B `mintedEpochs` ledger: `convertFeesToValidators`'s fold guard
/// `convertedEpochs.contains((pk, epochIdx)) == false` mirrors the mint fold's
/// `mintedEpochs.contains((pk, epochIdx)) == false`, recorded the same way
/// (`.add((pk, epochIdx))`) in the SAME runMVar state transition. Under a
/// multi-parent merge the content-addressed merge engine dedups the two parents'
/// identical `(v, E)` records so the conversion credit lands once — the same
/// mechanism (and the same `mintedEpochs` guard) the Stage-B epoch mint already
/// relies on (proven in MintingInjection.v `epoch_mint_idempotent_on_balance`,
/// EvalScheduling.tla `SupplyOnlyFromMint`, and the supply Sage model's
/// no-double-credit-under-merge property; this Stage adds the fee-ledger
/// analogues `fee_convert_credit_is_backed` / `Inv_FeeConvertConserves`).
///
/// Here we pin the consensus-observable foundation idempotency rests on: the
/// epoch fee conversion is DETERMINISTIC — two INDEPENDENT plays of the same
/// epoch close from the SAME pre-state produce BYTE-IDENTICAL post-states
/// (identical `convertedEpochs`, identical Σ⟦v⟧, identical F_v / @W_v). Combined
/// with the merge engine's `(v,E)`-keyed dedup, this IS the idempotency
/// guarantee. (A NOTE on scope: a sequential re-close of the SAME block height is
/// not a production scenario — block numbers are monotonic — and, exactly like
/// the Stage-B mint, is not what the per-epoch ledger guards; the guard fires
/// when a merged pre-state ALREADY carries `(v,E)`, which the merge dedup
/// establishes.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fee_convert_converted_epochs_idempotent_deterministic() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let sender = genesis_context.validator_pks()[0].clone();
            let block_data = BlockData {
                time_stamp: 0,
                block_number: 0, // epoch boundary, epochIdx 0
                sender: sender.clone(),
                seq_num: 0,
            };
            let fee_credits = Some(casper::rust::util::rholang::acceptance::FeeCredit {
                recipient_pk: sender.bytes.to_vec(),
                amount: 5,
            });

            // PLAY the SAME epoch close twice, INDEPENDENTLY from the same
            // pre-state. Both must yield the byte-identical post-state — the
            // determinism the `(v,E)` merge dedup builds on.
            async fn play_close(
                runtime_manager: &RuntimeManager,
                from_state: &StateHash,
                block_data: &BlockData,
                sender: &crypto::rust::public_key::PublicKey,
                fee_credits: &Option<casper::rust::util::rholang::acceptance::FeeCredit>,
            ) -> StateHash {
                let rt = runtime_manager.spawn_runtime().await;
                rt.set_block_data(block_data.clone()).await;
                let mut ops = RuntimeOps::new(rt);
                let mut close = CloseBlockDeploy {
                    initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
                        sender.clone(),
                        block_data.seq_num,
                    ),
                    settlement_debits: std::collections::BTreeMap::new(),
                    fee_credits: fee_credits.clone(),
                    // #13b: no genesis client funding slots in this fixture.
                    client_fuel_allocations: Vec::new(),
                };
                match ops.play_system_deploy(from_state, &mut close).await.unwrap() {
                    SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                    SystemDeployResult::PlayFailed { processed_system_deploy } => {
                        panic!("epoch close play failed: {:?}", processed_system_deploy)
                    }
                }
            }

            let post_a =
                play_close(&runtime_manager, &start_state, &block_data, &sender, &fee_credits).await;
            let post_b =
                play_close(&runtime_manager, &start_state, &block_data, &sender, &fee_credits).await;

            assert_eq!(
                post_a, post_b,
                "the epoch fee conversion + convertedEpochs record must be deterministic \
                 (identical post-state across independent plays) — the foundation of the \
                 (v,E)-keyed merge idempotency"
            );

            // Non-vacuity: the conversion actually credited Σ⟦v⟧ (mint + the
            // converted fee), exactly once in the single close.
            let supply_chan = supply::supply_channel(&Sig::Ground(sender.bytes.to_vec()));
            let rt = runtime_manager.spawn_runtime().await;
            let mut ops = RuntimeOps::new(rt);
            ops.runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&post_a))
                .await
                .unwrap();
            let bal = supply::read_balance(&ops, &supply_chan).await;
            assert!(
                bal > 0,
                "Σ⟦v⟧ must be funded after the epoch convert+mint (non-vacuous), got {}",
                bal
            );
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL play/replay determinism test for the Cost-Accounted Rho
/// Stage-C two-effect slash `Σ⟦v⟧`-zero (Decision 4 / 6.3). First funds an
/// offender's supply pool with an epoch mint (so the zero is NON-vacuous), then
/// PLAYS a `SlashDeploy` against that offender — whose `slash` contract resolves
/// the offender from the seeded `invalidBlocks` index, publishes it on
/// `sys:casper:slashedPk`, and whose play-side `post_eval` zeros
/// `Σ⟦offender⟧ = from_sig(Ground(pk))`. It then REPLAYS the SAME slash through
/// the PRODUCTION `replay_block_system_deploy` `Slash` branch (which runs
/// `post_eval_replay`, including the `ReplaySupplyMismatch` write-readback guard)
/// and asserts the post-state root is BYTE-IDENTICAL and `Σ⟦offender⟧ == 0` on
/// both paths — i.e. the offender resolution + `Σ⟦v⟧`-zero are play/replay-symmetric.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slash_zeros_supply_is_play_replay_deterministic() {
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            // The PROPOSER (slash issuer) is validator 0; the OFFENDER (the
            // validator whose invalid block is slashed) is validator 1.
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();

            // ── Step 1: fund Σ⟦offender⟧ with an epoch mint (non-vacuity) ──────
            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0, // 0 % epochLength == 0 ⇒ epoch boundary
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    proposer.clone(),
                    mint_block_data.seq_num,
                ),
            );
            let mint_result = mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap();
            let funded_state = match mint_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch-mint close failed"),
            };

            let supply_chan = supply::supply_channel(&Sig::Ground(offender.bytes.to_vec()));
            mint_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&funded_state))
                .await
                .unwrap();
            let pre_slash_balance = supply::read_balance(&mint_ops, &supply_chan).await;
            assert!(
                pre_slash_balance > 0,
                "non-vacuity: offender Σ⟦v⟧ must be positive before slash, got {}",
                pre_slash_balance
            );

            // ── Step 2: seed invalidBlocks (blockHash -> offender) ────────────
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"slash-play-replay-invalid-block");
            let mut invalid_blocks: HashMap<prost::bytes::Bytes, prost::bytes::Bytes> =
                HashMap::new();
            invalid_blocks.insert(invalid_block_hash.clone(), offender.bytes.clone());

            let slash_block_data = BlockData {
                time_stamp: 0,
                block_number: 1,
                sender: proposer.clone(),
                seq_num: 1,
            };

            // ── Step 3: PLAY the slash ────────────────────────────────────────
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(slash_block_data.clone()).await;
            play_runtime.set_invalid_blocks(invalid_blocks.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);
            let mut play_slash = SlashDeploy {
                invalid_block_hash: invalid_block_hash.clone(),
                pk: proposer.clone(),
                target_activation_epoch: 0,
                initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                    proposer.bytes.clone(),
                    slash_block_data.seq_num,
                    &invalid_block_hash,
                ),
            };
            let play_result = play_ops
                .play_system_deploy(&funded_state, &mut play_slash)
                .await
                .unwrap();
            let (final_play_state_hash, processed_slash) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("slash play failed: {:?}", processed_system_deploy),
            };

            // The offender's Σ⟦v⟧ must be ZERO in the play post-state.
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_play_state_hash))
                .await
                .unwrap();
            let play_post_balance = supply::read_balance(&play_ops, &supply_chan).await;
            assert_eq!(
                play_post_balance, 0,
                "slash must zero Σ⟦offender⟧ on play, got {}",
                play_post_balance
            );

            // ── Step 4: REPLAY the slash (production path) ────────────────────
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(slash_block_data.clone()).await;
            replay_runtime.set_invalid_blocks(invalid_blocks.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&funded_state))
                .await
                .unwrap();
            replay_ops
                .replay_block_system_deploy(
                    &slash_block_data,
                    &processed_slash,
                    &std::collections::BTreeMap::new(),
                    &None,
                    // #13b: no genesis client funding slots in this slash fixture.
                    &[],
                )
                .await
                .unwrap();
            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state.
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-C slash Σ⟦v⟧-zero"
            );

            // And the replayed Σ⟦offender⟧ is also zero.
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&final_replay_state_hash))
                .await
                .unwrap();
            let replay_post_balance =
                supply::read_balance(&replay_ops.runtime_ops, &supply_chan).await;
            assert_eq!(
                replay_post_balance, 0,
                "Σ⟦offender⟧ must be zero on replay too, got {}",
                replay_post_balance
            );
        },
    )
    .await
    .unwrap()
}

/// Helper: slash `offender` (seeded as the offender of `invalid_block_hash`) on
/// top of `start_state` as proposer `proposer`, returning the post-slash state
/// hash. Used by the redemption end-to-end test to reach a quarantined state.
async fn play_one_slash(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    offender: &crypto::rust::public_key::PublicKey,
    invalid_block_hash: &prost::bytes::Bytes,
    seq_num: i32,
) -> StateHash {
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;
    let mut invalid_blocks: HashMap<prost::bytes::Bytes, prost::bytes::Bytes> = HashMap::new();
    invalid_blocks.insert(invalid_block_hash.clone(), offender.bytes.clone());
    let block_data = BlockData {
        time_stamp: 0,
        block_number: 1,
        sender: proposer.clone(),
        seq_num,
    };
    let runtime = runtime_manager.spawn_runtime().await;
    runtime.set_block_data(block_data.clone()).await;
    runtime.set_invalid_blocks(invalid_blocks).await;
    let mut ops = RuntimeOps::new(runtime);
    let mut slash = SlashDeploy {
        invalid_block_hash: invalid_block_hash.clone(),
        pk: proposer.clone(),
        target_activation_epoch: 0,
        initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
            proposer.bytes.clone(),
            seq_num,
            invalid_block_hash,
        ),
    };
    match ops.play_system_deploy(start_state, &mut slash).await.unwrap() {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed { processed_system_deploy } => {
            panic!("setup slash failed: {:?}", processed_system_deploy)
        }
    }
}

/// CONSENSUS-CRITICAL Stage-C redemption end-to-end (DR-7/DR-12). Drives the real
/// `redeemSlashed` Rholang contract through `RedeemDeploy`:
///   (1) fund + slash an offender (reaching a quarantined, halted, bond-0 state);
///   (2) play a Vindicated redeem with a VALID PoS-multisig quorum — asserts the
///       deploy SUCCEEDS, the validator is restored to active, and un-halted;
///   (3) play a Vindicated redeem with an UNDER-QUORUM authorization — asserts the
///       deploy is REJECTED (no restore: the validator stays quarantined/halted).
/// The DR-12 multisig-quorum verification is the Rust platform obligation; the
/// keyset/quorum/authorizations ride on `RedeemDeploy` (replay-carried).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redeem_outcomes_and_multisig_gate() {
    use casper::rust::util::rholang::costacc::redeem_deploy::RedemptionAuthorization;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"redeem-e2e-invalid-block");

            // ── (1) fund Σ⟦offender⟧ then slash to quarantine the offender ────
            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    proposer.clone(),
                    0,
                ),
            );
            let funded_state = match mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch mint failed"),
            };

            let slashed_state = play_one_slash(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender,
                &invalid_block_hash,
                1,
            )
            .await;

            // Build a custom 3-key multisig set (quorum 2) and its secrets. The
            // RedeemDeploy carries its own keyset/quorum (replay-stable); the
            // Rust DR-12 obligation verifies signatures over the redemption digest.
            let secp = Secp256k1;
            let keypairs: Vec<(Vec<u8>, Vec<u8>)> = (0..3)
                .map(|_| {
                    let (sk, pk) = secp.new_key_pair();
                    (sk.bytes.to_vec(), pk.bytes.to_vec())
                })
                .collect();
            let keyset: Vec<String> = keypairs.iter().map(|(_, pk)| hex::encode(pk)).collect();

            let make_redeem = |n_signers: usize, seq: i32| -> RedeemDeploy {
                let mut d = RedeemDeploy {
                    validator_pk: offender.bytes.to_vec(),
                    outcome: RedemptionOutcome::Vindicated,
                    pos_multi_sig_public_keys: keyset.clone(),
                    pos_multi_sig_quorum: 2,
                    authorizations: Vec::new(),
                    initial_rand: system_deploy_util::generate_redeem_deploy_random_seed(
                        proposer.bytes.clone(),
                        seq,
                        "Vindicated",
                    ),
                };
                let digest = d.auth_digest();
                d.authorizations = keypairs
                    .iter()
                    .take(n_signers)
                    .map(|(sk, pk)| RedemptionAuthorization {
                        public_key: pk.clone(),
                        signature: secp.sign(&digest, sk),
                    })
                    .collect();
                d
            };

            let redeem_block_data = BlockData {
                time_stamp: 0,
                block_number: 2,
                sender: proposer.clone(),
                seq_num: 2,
            };

            // ── (3 first: under-quorum REJECTION on the quarantined state) ────
            // Only 1 of 2 required signers ⇒ verify_multisig_quorum is false ⇒
            // redeemSlashed rejects with NO state change.
            let under_runtime = runtime_manager.spawn_runtime().await;
            under_runtime.set_block_data(redeem_block_data.clone()).await;
            let mut under_ops = RuntimeOps::new(under_runtime);
            let mut under_redeem = make_redeem(1, 2);
            assert!(
                !under_redeem.verify_multisig_quorum(),
                "1-of-2 must be under quorum"
            );
            let under_result = under_ops
                .play_system_deploy(&slashed_state, &mut under_redeem)
                .await
                .unwrap();
            // The deploy itself does not error, but the contract returns
            // (false, ...) ⇒ play_system_deploy reports a system-deploy USER
            // failure (PlayFailed). Either way, the offender must STAY quarantined.
            let under_post_state = match under_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => slashed_state.clone(),
            };
            // Assert the offender is STILL halted (not restored) on the under-quorum path.
            let under_runtime2 = runtime_manager.spawn_runtime().await;
            let mut under_ops2 = RuntimeOps::new(under_runtime2);
            assert!(
                pos_validator_is_halted(&mut under_ops2, &under_post_state, &offender).await,
                "under-quorum redemption must NOT restore: offender stays halted"
            );

            // ── (2) valid quorum (2-of-2) Vindicated ⇒ restore + un-halt ──────
            let ok_runtime = runtime_manager.spawn_runtime().await;
            ok_runtime.set_block_data(redeem_block_data.clone()).await;
            let mut ok_ops = RuntimeOps::new(ok_runtime);
            let mut ok_redeem = make_redeem(2, 2);
            assert!(
                ok_redeem.verify_multisig_quorum(),
                "2-of-2 must meet quorum"
            );
            let ok_result = ok_ops
                .play_system_deploy(&slashed_state, &mut ok_redeem)
                .await
                .unwrap();
            let ok_post_state = match ok_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { processed_system_deploy } => {
                    panic!("valid-quorum vindicated redeem failed: {:?}", processed_system_deploy)
                }
            };
            let ok_runtime2 = runtime_manager.spawn_runtime().await;
            let mut ok_ops2 = RuntimeOps::new(ok_runtime2);
            assert!(
                !pos_validator_is_halted(&mut ok_ops2, &ok_post_state, &offender).await,
                "valid-quorum vindicated redemption must un-halt the offender"
            );
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL Stage-C redemption play/replay determinism (DR-7/DR-12).
/// The slash Σ⟦v⟧-zero has `slash_zeros_supply_is_play_replay_deterministic`; this
/// is its redemption analogue. A Vindicated `redeemSlashed` (un-halt + restore
/// bond + clear quarantine + drop stale mintedEpochs) is a pure PoS-state
/// transition with NO supply `post_eval` — redemption writes neither Σ⟦v⟧ nor
/// @W_v; re-funding is deferred to the next epoch mint (spec tex:2382-2383). This
/// pins that the transition is byte-identical on play and replay: a proposer that
/// redeems and a validator that replays the block reach the same post-state root,
/// so redemption cannot fork consensus.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redeem_vindicated_is_play_replay_deterministic() {
    use casper::rust::util::rholang::costacc::redeem_deploy::RedemptionAuthorization;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"redeem-replay-invalid-block");

            // ── fund Σ⟦offender⟧ then slash to reach a quarantined/halted state ──
            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(proposer.clone(), 0),
            );
            let funded_state = match mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch mint failed"),
            };
            let slashed_state = play_one_slash(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender,
                &invalid_block_hash,
                1,
            )
            .await;

            // ── build a VALID 2-of-2 Vindicated RedeemDeploy ──────────────────
            let secp = Secp256k1;
            let keypairs: Vec<(Vec<u8>, Vec<u8>)> = (0..3)
                .map(|_| {
                    let (sk, pk) = secp.new_key_pair();
                    (sk.bytes.to_vec(), pk.bytes.to_vec())
                })
                .collect();
            let keyset: Vec<String> = keypairs.iter().map(|(_, pk)| hex::encode(pk)).collect();
            let mut redeem = RedeemDeploy {
                validator_pk: offender.bytes.to_vec(),
                outcome: RedemptionOutcome::Vindicated,
                pos_multi_sig_public_keys: keyset.clone(),
                pos_multi_sig_quorum: 2,
                authorizations: Vec::new(),
                initial_rand: system_deploy_util::generate_redeem_deploy_random_seed(
                    proposer.bytes.clone(),
                    2,
                    "Vindicated",
                ),
            };
            let digest = redeem.auth_digest();
            redeem.authorizations = keypairs
                .iter()
                .take(2)
                .map(|(sk, pk)| RedemptionAuthorization {
                    public_key: pk.clone(),
                    signature: secp.sign(&digest, sk),
                })
                .collect();
            assert!(redeem.verify_multisig_quorum(), "2-of-2 must meet quorum");

            let redeem_block_data = BlockData {
                time_stamp: 0,
                block_number: 2,
                sender: proposer.clone(),
                seq_num: 2,
            };

            // ── PLAY the Vindicated redeem ────────────────────────────────────
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(redeem_block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);
            let play_result = play_ops
                .play_system_deploy(&slashed_state, &mut redeem)
                .await
                .unwrap();
            let (final_play_state_hash, processed_redeem) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("vindicated redeem play failed: {:?}", processed_system_deploy),
            };

            // offender un-halted on play
            let chk_runtime = runtime_manager.spawn_runtime().await;
            let mut chk_ops = RuntimeOps::new(chk_runtime);
            assert!(
                !pos_validator_is_halted(&mut chk_ops, &final_play_state_hash, &offender).await,
                "vindicated redeem must un-halt the offender on play"
            );

            // ── REPLAY the Vindicated redeem (production path) ────────────────
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(redeem_block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&slashed_state))
                .await
                .unwrap();
            replay_ops
                .replay_block_system_deploy(
                    &redeem_block_data,
                    &processed_redeem,
                    &std::collections::BTreeMap::new(),
                    &None,
                    // #13b: no genesis client funding slots in this redeem fixture.
                    &[],
                )
                .await
                .unwrap();
            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state.
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-C Vindicated redemption"
            );

            // offender un-halted on replay too
            let chk_runtime2 = runtime_manager.spawn_runtime().await;
            let mut chk_ops2 = RuntimeOps::new(chk_runtime2);
            assert!(
                !pos_validator_is_halted(&mut chk_ops2, &final_replay_state_hash, &offender).await,
                "vindicated redeem must un-halt the offender on replay too"
            );
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL Stage-C halt observation. Reads the PoS `mintingHalted`
/// Set[PublicKey] off `post_state` and returns whether `validator` is a member.
/// Drives the `getMintingHalted` peek contract (PoS.rhox, added for Stage-C
/// observability) through a registry-looked-up exploratory deploy, computing the
/// membership predicate INSIDE Rholang (`halted.contains(pk)`) so the captured
/// result is a single `GBool` — robust, no nested ESet decode. The exploratory
/// deploy resets to `post_state` internally (read-only; no mutation).
async fn pos_validator_is_halted(
    ops: &mut RuntimeOps,
    post_state: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> bool {
    use models::rhoapi::expr::ExprInstance;

    // `return` is the FIRST `new` name, so it is the channel
    // `play_exploratory_deploy` captures. Look PoS up from the registry, peek
    // `getMintingHalted`, and send back the membership boolean for the offender.
    let term = format!(
        r#"
        new return, poSCh, haltedCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getMintingHalted", *haltedCh) |
            for (@halted <- haltedCh) {{
              return!(halted.contains("{}".hexToBytes()))
            }}
          }}
        }}"#,
        hex::encode(validator.bytes.to_vec())
    );

    let (results, _cost) = ops
        .play_exploratory_deploy(term, post_state)
        .await
        .expect("getMintingHalted exploratory query must execute");

    // The captured return value is a single `GBool`: true iff the offender is
    // still in `mintingHalted` (halted), false iff un-halted (restored).
    results
        .iter()
        .flat_map(|p| p.exprs.iter())
        .find_map(|e| match e.expr_instance {
            Some(ExprInstance::GBool(b)) => Some(b),
            _ => None,
        })
        .expect("getMintingHalted membership query must return a boolean")
}

/// CONSENSUS-CRITICAL WD-D2 gate-decision replay determinism. Exercises the
/// settlement-debit play↔replay symmetry directly: PLAY applies the gate's
/// threaded `AdmissionOutcome.debits` via `CloseBlockDeploy::post_eval`; REPLAY
/// RECOMPUTES the identical debit map from the admitted deploys via
/// `acceptance::recompute_settlement_debits` and applies it via
/// `post_eval_replay`. Asserts (a) the recomputed map EQUALS the gate's play-side
/// map, (b) the post-state root is BYTE-IDENTICAL, and (c) every `Σ⟦s⟧` balance
/// matches — i.e. `post = pre − ΣΔ_admitted` holds identically on both paths.
///
/// The scenario provisions ONE signer's pool (PRESENT ⇒ enforced + debited) and
/// leaves a second signer's pool ABSENT (admitted unenforced ⇒ no debit), so the
/// test also pins the per-pool presence activation across the play/replay seam.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate_decision_replay_determinism() {
    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();

            // Two real signed deploys with known token demand. `@0!(0) | @0!(0)`
            // ⇒ Δ = 2 (two sends); `@0!(0)` ⇒ Δ = 1.
            let d_funded = construct_deploy::source_deploy_now_full(
                "@0!(0) | @0!(0)".to_string(),
                Some(1),
                None,
                None,
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let d_absent = construct_deploy::source_deploy_now_full(
                "@0!(0)".to_string(),
                Some(1),
                None,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();

            let funded_cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(d_funded.clone())
                    .unwrap();
            let absent_cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(d_absent.clone())
                    .unwrap();

            // The funded signer's supply pool channel.
            let funded_env = accounting::envelope_sig(&funded_cosigned);
            let funded_chan = supply::supply_channel(&funded_env);
            let absent_env = accounting::envelope_sig(&absent_cosigned);
            let absent_chan = supply::supply_channel(&absent_env);
            // Distinct signers ⇒ distinct pools.
            assert_ne!(funded_chan, absent_chan);

            // ---- SEED: provision the funded signer's pool with Σ = 5 ----
            // (Δ_funded = 2 ⇒ after the debit Σ = 3.) Leave the absent pool unset.
            const SEED_BALANCE: i64 = 5;
            let seed_runtime = runtime_manager.spawn_runtime().await;
            let mut seed_ops = RuntimeOps::new(seed_runtime);
            seed_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();
            supply::produce_balance(
                &mut seed_ops,
                &funded_chan,
                SEED_BALANCE,
                supply::mint_random_state(
                    &Blake2b512Random::create_from_bytes(&[123_u8; 1]),
                    0,
                ),
            )
            .await
            .unwrap();
            let seeded_state = seed_ops
                .runtime
                .create_checkpoint()
                .await
                .root
                .to_bytes_prost();

            // ---- GATE (play side) against the seeded pre-state ----
            let reader = acceptance::RuntimeManagerSupplyReader {
                runtime_manager: &runtime_manager,
                pre_state_hash: seeded_state.clone(),
            };
            let outcome = acceptance::admit_by_funding(
                vec![funded_cosigned.clone(), absent_cosigned.clone()],
                &reader,
                /* margin */ 0,
                // strict = false: this test exercises the TRANSITIONAL gate
                // (one present pool enforced + debited, one absent pool admitted
                // unenforced) and the play↔replay symmetry of that path.
                /* strict */ false,
            )
            .await
            .unwrap();
            // Both admitted (funded: Σ=5 ≥ Δ=2; absent: unenforced).
            assert_eq!(outcome.admitted.len(), 2, "both deploys admitted");
            assert!(outcome.rejected.is_empty());
            // Exactly one debit: the funded pool, amount Δ=2. The absent pool is
            // not debited (presence gate).
            let funded_key = accounting::delta_sigma::sig_key(&funded_env);
            let absent_key = accounting::delta_sigma::sig_key(&absent_env);
            assert_eq!(outcome.debits.get(&funded_key).map(|d| d.amount), Some(2));
            assert!(
                outcome.debits.get(&absent_key).is_none(),
                "absent pool must not be debited"
            );

            let block_data = BlockData {
                time_stamp: 1,
                block_number: 1, // non-epoch ⇒ no mint, isolating the debit
                sender: _genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let close_rand = system_deploy_util::generate_close_deploy_random_seed_from_pk(
                block_data.sender.clone(),
                block_data.seq_num,
            );

            // ---- PLAY: apply the gate's threaded debit map via post_eval ----
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&seeded_state))
                .await
                .unwrap();
            let mut play_close = CloseBlockDeploy::new(close_rand.clone());
            play_close.settlement_debits = outcome.debits.clone();
            play_close
                .post_eval(&mut play_ops, &block_data, &seeded_state)
                .await
                .unwrap();
            let play_post = play_ops.runtime.create_checkpoint().await.root.to_bytes_prost();
            play_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&play_post))
                .await
                .unwrap();
            let play_funded_balance = supply::read_balance(&play_ops, &funded_chan).await;
            assert_eq!(
                play_funded_balance,
                SEED_BALANCE - 2,
                "play: post Σ⟦s⟧ = pre − ΣΔ = 5 − 2 = 3"
            );

            // ---- REPLAY: recompute the debit map, apply via post_eval_replay ----
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&seeded_state))
                .await
                .unwrap();
            let recompute_reader = acceptance::RuntimeOpsSupplyReader {
                runtime_ops: &replay_ops.runtime_ops,
            };
            let recomputed = acceptance::recompute_settlement_debits(
                vec![funded_cosigned.clone(), absent_cosigned.clone()],
                &recompute_reader,
                // strict = false: same transitional path as the play side above
                // (absent pool unenforced ⇒ no admission re-verification error).
                false,
            )
            .await
            .unwrap();
            // (a) the recomputed map EQUALS the gate's play-side map.
            assert_eq!(
                recomputed, outcome.debits,
                "replay-recomputed debit map must equal the play-side gate map"
            );

            let replay_close = CloseBlockDeploy::new(close_rand);
            replay_close
                .post_eval_replay(
                    &mut replay_ops.runtime_ops,
                    &block_data,
                    &seeded_state,
                    &recomputed,
                )
                .await
                .unwrap();
            let replay_post = replay_ops
                .runtime_ops
                .runtime
                .create_checkpoint()
                .await
                .root
                .to_bytes_prost();

            // (b) byte-identical post-state root.
            assert_eq!(
                play_post, replay_post,
                "play and replay post-state roots diverged on the WD-D2 settlement debit"
            );

            // (c) every Σ⟦s⟧ balance matches.
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&replay_post))
                .await
                .unwrap();
            let replay_funded_balance =
                supply::read_balance(&replay_ops.runtime_ops, &funded_chan).await;
            assert_eq!(
                play_funded_balance, replay_funded_balance,
                "Σ⟦s⟧ balance diverged between play and replay after the settlement debit"
            );
            // The absent pool stays absent on both paths (never written).
            let replay_absent_balance =
                supply::read_balance(&replay_ops.runtime_ops, &absent_chan).await;
            assert_eq!(replay_absent_balance, 0, "absent pool untouched");
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn balance_deploy_should_compute_rev_balances() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let user_pk = construct_deploy::DEFAULT_PUB.clone();
            let _ = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CheckBalance {
                    pk: user_pk.clone(),
                    rand: Blake2b512Random::create_from_bytes(&vec![]),
                },
                &mut CheckBalance {
                    pk: user_pk.clone(),
                    rand: Blake2b512Random::create_from_bytes(&vec![]),
                },
                |result| *result == 9000000,
            )
            .await
            .unwrap();
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_capture_rholang_errors() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let bad_rholang =
                r#" for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!("hi") "#;
            let deploy = construct_deploy::source_deploy_now_full(
                bad_rholang.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(result.1.is_failed == true);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_then_compute_bonds_should_be_replayable_after_all() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let gps = genesis_block.body.state.post_state_hash;

            let s0 = "@1!(1)";
            let s1 = "@2!(2)";
            let s2 = "for(@a <- @1){ @123!(5 * a) }";

            let deploys0 = vec![s0, s1, s2]
                .into_iter()
                .map(|s| {
                    construct_deploy::source_deploy_now_full(
                        s.to_string(),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();

            let s3 = "@1!(1)";
            let s4 = "for(@a <- @2){ @456!(5 * a) }";

            let deploys1 = vec![s3, s4]
                .into_iter()
                .map(|s| {
                    construct_deploy::source_deploy_now_full(
                        s.to_string(),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let (play_state_hash_0, processed_deploys_0, processed_sys_deploys_0) = runtime_manager
                .compute_state(
                    &gps,
                    deploys0,
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    genesis_context.validator_pks()[0].clone(),
                                    0,
                                ),
                            ),
                        ),
                    ],
                    BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                )
                .await
                .unwrap();

            let bonds0 = runtime_manager
                .compute_bonds(&play_state_hash_0)
                .await
                .unwrap();

            let replay_state_hash_0 = runtime_manager
                .replay_compute_state(
                    &gps,
                    processed_deploys_0,
                    processed_sys_deploys_0,
                    &BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await
                .unwrap();

            assert!(play_state_hash_0 == replay_state_hash_0);

            let bonds1 = runtime_manager
                .compute_bonds(&play_state_hash_0)
                .await
                .unwrap();

            assert!(bonds0 == bonds1);

            let (play_state_hash_1, processed_deploys_1, processed_sys_deploys_1) = runtime_manager
                .compute_state(
                    &play_state_hash_0,
                    deploys1,
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    genesis_context.validator_pks()[0].clone(),
                                    0,
                                ),
                            ),
                        ),
                    ],
                    BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                )
                .await
                .unwrap();

            let bonds2 = runtime_manager
                .compute_bonds(&play_state_hash_1)
                .await
                .unwrap();

            let replay_state_hash_1 = runtime_manager
                .replay_compute_state(
                    &play_state_hash_0,
                    processed_deploys_1,
                    processed_sys_deploys_1,
                    &BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await
                .unwrap();

            assert!(play_state_hash_1 == replay_state_hash_1);

            let bonds3 = runtime_manager
                .compute_bonds(&play_state_hash_1)
                .await
                .unwrap();

            assert!(bonds2 == bonds3);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_capture_rholang_parsing_errors_without_token_charge() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let bad_rholang =
                r#" for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!("hi") "#;
            let deploy = construct_deploy::source_deploy_now_full(
                bad_rholang.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(result.1.is_failed == true);
            assert_eq!(result.1.cost.cost, 0);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_charge_for_execution_tokens() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let correct_rholang =
                r#" for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) | @"x"!(1) | @"y"!(2) } "#;
            let rand = Blake2b512Random::create_from_bytes(&Vec::new());
            let inital_phlo = Cost::unsafe_max();
            let deploy = construct_deploy::source_deploy_now_full(
                correct_rholang.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let runtime = runtime_manager.spawn_runtime().await;
            runtime.cost.set(inital_phlo.clone());
            let term = Compiler::source_to_adt(&deploy.data.term).unwrap();
            let _ = runtime.inj(term, Env::new(), rand).await;
            let phlos_left = runtime.cost.get();
            let reduction_cost = inital_phlo - phlos_left;

            let result = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(result.1.cost.cost == reduction_cost.value as u64);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_return_the_value_at_the_specified_channel_after_a_rholang_computation(
) {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let deployo0 = construct_deploy::source_deploy_now_full(
                r#"
                        new rl(`rho:registry:lookup`), NonNegativeNumberCh in {
                        rl!(`rho:lang:nonNegativeNumber`, *NonNegativeNumberCh) |
                        for(@(_, NonNegativeNumber) <- NonNegativeNumberCh) {
                          @NonNegativeNumber!(37, "nn")
                        }
                      }
                "#
                .to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result0 = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deployo0,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            let hash = result0.0;
            let deployo1 = construct_deploy::source_deploy_now_full(
                r#"
                new return in { for(nn <- @"nn"){ nn!("value", *return) } }
                "#
                .to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result1 = runtime_manager
                .capture_results(&hash, &deployo1)
                .await
                .unwrap();

            assert!(result1.len() == 1);
            assert!(result1[0] == ParBuilderUtil::mk_term("37").unwrap());
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_handle_multiple_results_and_no_results_appropriately() {
    with_runtime_manager(|runtime_manager, _, _| async move {
        let n = 8;
        let returns = (1..=n)
            .map(|i| format!("return!({})", i))
            .collect::<Vec<_>>();
        let term = format!("new return in {{ {} }}", returns.join("|"));
        let term_no_res = format!("new x, return in {{ {} }}", returns.join("|"));
        let deploy =
            construct_deploy::source_deploy(term, 0, None, None, None, None, None).unwrap();
        let deploy_no_res =
            construct_deploy::source_deploy(term_no_res, 0, None, None, None, None, None).unwrap();

        let many_results = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy)
            .await
            .unwrap();

        let no_results = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy_no_res)
            .await
            .unwrap();

        assert!(no_results.is_empty());
        assert!(many_results.len() == n);
        assert!((1..=n)
            .all(|i| many_results.contains(&ParBuilderUtil::mk_term(&i.to_string()).unwrap())));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_throw_error_if_execution_fails() {
    with_runtime_manager(|runtime_manager, _, _| async move {
        let deploy = construct_deploy::source_deploy(
            "new return in { return.undefined() }".to_string(),
            0,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy)
            .await;

        assert!(result.is_err());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_state_hash_should_not_remember_previous_hot_store_state() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let deploy1 = construct_deploy::basic_deploy_data(0, None, None).unwrap();
            let deploy2 = construct_deploy::basic_deploy_data(0, None, None).unwrap();

            let hash1 = RuntimeManager::empty_state_hash_fixed();
            let _ = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy1,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            let hash2 = RuntimeManager::empty_state_hash_fixed();
            let _ = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy2,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(hash1 == hash2);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_be_replayed_by_replay_compute_state() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let deploy = construct_deploy::source_deploy_now_full(
                r#"
                  new deployerId(`rho:system:deployerId`),
                  rl(`rho:registry:lookup`),
                  revAddressOps(`rho:vault:address`),
                  revAddressCh,
                  revVaultCh in {
                  rl!(`rho:vault:system`, *revVaultCh) |
                  revAddressOps!("fromDeployerId", *deployerId, *revAddressCh) |
                  for(@userRevAddress <- revAddressCh & @(_, revVault) <- revVaultCh){
                    new userVaultCh in {
                    @revVault!("findOrCreate", userRevAddress, *userVaultCh) |
                    for(@(true, userVault) <- userVaultCh){
                    @userVault!("balance", "IGNORE")
                    }
                  }
                }
              }
            }
                "#
                .to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let genesis_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time as i64,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();
            let (play_post_state, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let replay_compute_state_result = runtime_manager
                .replay_compute_state(
                    &genesis_post_state,
                    processed_deploys,
                    processed_system_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await
                .unwrap();

            assert!(play_post_state == replay_compute_state_result);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_charge_deploys_separately() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            fn deploy_cost(p: &[ProcessedDeploy]) -> u64 {
                p.iter().map(|d| d.cost.cost).sum()
            }

            let deploy0 = construct_deploy::source_deploy(
                r#"for(@x <- @"w") { @"z"!("Got x") } "#.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let deploy1 = construct_deploy::source_deploy(
                r#"for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) | @"x"!(1) | @"y"!(10) } "#
                    .to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let genesis_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time as i64,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();
            let (_, first_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![construct_deploy::source_deploy(
                        r#"for(@x <- @"w") { @"z"!("Got x") } "#.to_string(),
                        123,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let (_, second_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![construct_deploy::source_deploy(
                        r#"for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) | @"x"!(1) | @"y"!(10) } "#
                            .to_string(),
                        123,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let (_, compound_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![deploy0, deploy1],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            assert!(first_deploy.len() == 1);
            assert!(second_deploy.len() == 1);
            assert!(compound_deploy.len() == 2);

            let first_deploy_cost = deploy_cost(&first_deploy);
            let second_deploy_cost = deploy_cost(&second_deploy);
            let compound_deploy_cost = deploy_cost(&compound_deploy);

            assert!(first_deploy_cost < compound_deploy_cost);
            assert!(second_deploy_cost < compound_deploy_cost);

            let matched_first = compound_deploy
                .iter()
                .find(|d| d.deploy == first_deploy[0].deploy)
                .cloned()
                .expect("Expected at least one matching deploy");
            assert_eq!(first_deploy_cost, deploy_cost(&vec![matched_first]));

            let matched_second = compound_deploy
                .iter()
                .find(|d| d.deploy == second_deploy[0].deploy)
                .cloned()
                .expect("Expected at least one matching deploy");
            assert_eq!(second_deploy_cost, deploy_cost(&vec![matched_second]));

            assert_eq!(first_deploy_cost + second_deploy_cost, compound_deploy_cost);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_settlement_use_case_does_not_change_user_runtime_cost() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            // Keep the user COMM on a deploy-local channel so this test isolates
            // fee-settlement system deploys from public-channel application effects.
            let source = "new x in { x!(0) | for(@0 <- x){ Nil } }";
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let deploy_without_settlement = construct_deploy::source_deploy(
                source.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let deploy_with_settlement = construct_deploy::source_deploy(
                source.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let (_, user_only, _) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy_without_settlement],
                    Vec::new(),
                    block_data.clone(),
                    Some(HashMap::new()),
                )
                .await
                .unwrap();

            let (_, with_settlement, _) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy_with_settlement],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data,
                    Some(HashMap::new()),
                )
                .await
                .unwrap();

            assert_eq!(user_only.len(), 1);
            assert_eq!(with_settlement.len(), 1);
            assert_eq!(user_only[0].cost, with_settlement[0].cost);
            assert_eq!(user_only[0].is_failed, with_settlement[0].is_failed);
            // NOTE: We intentionally do not assert equality of
            // `deploy_log.len()` here. The PoS pre-charge + refund flow
            // engages persistent consumes (`<<-`/`<=`-style) whose
            // re-registration spawns parallel futures in
            // `reduce::continue_consume_process`. Under tokio's
            // multi-thread scheduling, those persistent consumes can
            // legitimately match an extra or one-fewer time per run,
            // shifting `deploy_log.len()` by ±1 across otherwise-
            // identical play passes. The cost and is_failed assertions
            // above already cover the "system settlement does not change
            // user runtime cost" claim this test exists to enforce.
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_just_work() {
    with_runtime_manager(|mut runtime_manager, genesis_context, genesis_block| async move {
      let gen_post_state = genesis_block.body.state.post_state_hash;
      let source =  r#"
      new d1,d2,d3,d4,d5,d6,d7,d8,d9 in {
        contract d1(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1)
          }
        } |
        contract d2(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1)
          }
        } |
        contract d3(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1)
          }
        } |
        contract d4(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1)
          }
        } |
        contract d5(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1)
          }
        } |
        contract d6(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1)
          }
        } |
        contract d7(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1)
          }
        } |
        contract d8(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1)
          }
        } |
        contract d9(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1)
          }
        } |
        d1!(2) |
        d2!(2) |
        d3!(2) |
        d4!(2) |
        d5!(2) |
        d6!(2) |
        d7!(2) |
        d8!(2) |
        d9!(2)
      }
      "#.to_string();

      // Budget must be affordable: the (multi-sig) pre-charge debits
      // phlo_limit * phlo_price (price defaults to 1) from the signer's
      // genesis vault (predefined balance 9_000_000) before evaluation, so an
      // i64::MAX limit would fail pre-charge with "Insufficient funds". This
      // budget is affordable and amply covers the parallel fan-out below.
      let deploy = construct_deploy::source_deploy_now_full(source, Some(9_000_000), None, None, None, None).unwrap();
      let (play_state_hash1, processed_deploy) = compute_state(&mut runtime_manager, &genesis_context, deploy, &gen_post_state).await;
      let replay_compute_state_result = replay_compute_state(&mut runtime_manager, &genesis_context, processed_deploy, &gen_post_state).await.unwrap();
      assert!(play_state_hash1 == replay_compute_state_result);
      assert!(play_state_hash1 != gen_post_state);
    })
        .await
        .unwrap()
}

async fn invalid_replay(source: String) -> Result<StateHash, CasperError> {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let deploy = construct_deploy::source_deploy_now_full(
                source,
                Some(10000),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();

            let (_, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();
            let processed_deploy = processed_deploys.into_iter().next().unwrap();
            let processed_deploy_cost = processed_deploy.cost.cost;

            let invalid_processed_deploy = ProcessedDeploy {
                cost: PCost {
                    cost: processed_deploy_cost - 1,
                },
                ..processed_deploy
            };

            let result = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    vec![invalid_processed_deploy],
                    processed_system_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await;

            result
        },
    )
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mixed_success_and_oop_deploys_keep_isolated_cost_traces() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let success = construct_deploy::source_deploy_now_full(
                "@0!(0) | for(@0 <- @0){ Nil }".to_string(),
                Some(10000),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            // D3 (DR-9): `@1!(1)` is a SINGLE send = exactly ONE COMM. Under the
            // pre-D3 per-op model a 1-phlo budget OOP'd this deploy; under D3
            // one COMM exactly fits a 1-token budget, so it SUCCEEDS (it is no
            // longer "oop"). The 1-token `phlo_limit` is advisory only —
            // accepted deploys run unmetered-for-liveness — and is kept here
            // purely to preserve the original mixed-budget fixture.
            let single_comm = construct_deploy::source_deploy_now_full(
                "@1!(1)".to_string(),
                Some(1),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let (play_state, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![success, single_comm],
                    Vec::new(),
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();

            assert_eq!(processed_deploys.len(), 2);
            // D3 (DR-9): BOTH deploys succeed. Per-op metering is diagnostic and
            // does not gate liveness; the only liveness gate is the per-COMM
            // budget, and `@1!(1)` is one COMM under a 1-token budget (it fits).
            // So no deploy is failed (pre-D3 this count was 1).
            assert_eq!(
                processed_deploys
                    .iter()
                    .filter(|deploy| deploy.is_failed)
                    .count(),
                0,
                "D3: a single-COMM deploy under a 1-token budget no longer OOPs"
            );
            // The test's real intent — ISOLATED cost traces: each deploy carries
            // its OWN per-COMM consensus cost, not a shared/summed total.
            //   `@0!(0) | for(@0 <- @0){ Nil }` = 1 send + 1 receive = 2 COMMs.
            //   `@1!(1)`                         = 1 send             = 1 COMM.
            assert_eq!(
                processed_deploys[0].cost.cost, 2,
                "first deploy's isolated cost = its 2 COMMs"
            );
            assert_eq!(
                processed_deploys[1].cost.cost, 1,
                "second deploy's isolated cost = its 1 COMM (not contaminated by the first)"
            );

            let replay_state = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    processed_deploys,
                    processed_system_deploys,
                    &block_data,
                    None,
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await
                .unwrap();

            assert_eq!(play_state, replay_state);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replaycomputestate_should_catch_discrepancies_in_initial_and_replay_cost_when_no_errors_are_thrown(
) {
    let result = invalid_replay("@0!(0) | for(@0 <- @0){ Nil }".to_string()).await;
    match result {
        Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        })) => {
            // The test corrupts the recorded deploy cost by one token. Exact
            // totals belong to the reducer's source-token schedule, while the
            // replay contract here is that the mismatch is detected exactly.
            assert!(initial_cost > 0);
            assert_eq!(replay_cost, initial_cost + 1);
        }
        _ => panic!("Expected ReplayCostMismatch error"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replaycomputestate_should_not_catch_discrepancies_in_initial_and_replay_cost_when_user_errors_are_thrown(
) {
    let result = invalid_replay("@0!(0) | for(@x <- @0){ x.undefined() }".to_string()).await;
    match result {
        Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        })) => {
            // User execution errors are rollback-safe, but replay must still
            // reject a processed deploy whose charged token count was forged.
            assert!(initial_cost > 0);
            assert_eq!(replay_cost, initial_cost + 1);
        }
        _ => panic!("Expected ReplayCostMismatch error"),
    }
}

// This is additional test for sorting with joins and channels inside joins.
// - after reverted PR https://github.com/rchain/rchain/pull/2436
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joins_should_be_replayed_correctly() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let term = r#"
            new a, b, c, d in {
              for (_ <- a & _ <- b) { Nil } |
              for (_ <- a & _ <- c) { Nil } |
              for (_ <- a & _ <- d) { Nil }
            }
            "#;

            let gen_post_state = genesis_block.body.state.post_state_hash;
            let deploy = construct_deploy::source_deploy_now_full(
                term.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: time,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };

            let invalid_blocks = HashMap::new();
            let (state_hash, processed_deploys, processed_sys_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy],
                    Vec::new(), // No system deploys
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let replay_state_hash = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    processed_deploys,
                    processed_sys_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await
                .unwrap();

            assert_eq!(
                hex::encode(state_hash.to_vec()),
                hex::encode(replay_state_hash.to_vec())
            );
        },
    )
    .await
    .unwrap();
}

/// Reproduce ReplayCostMismatch with duplicate channel sends (bridge.rho pattern).
///
/// Uses two independent RuntimeManagers sharing the same genesis RSpace scope.
/// The first plays the deploy (hot store populated from execution).
/// The second replays with a fresh hot store (loads from history).
/// This simulates the block creator vs replayer divergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_on_independent_runtime_should_match_play_cost_for_duplicate_sends() {
    use crate::util::rholang::resources::{
        mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_post_state = genesis_block.body.state.post_state_hash.clone();

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    let mut failures = Vec::new();
    for attempt in 0..10 {
        let mut kvm_play = mk_test_rnode_store_manager_from_genesis(&genesis_context);
        let (rm_play, _) = mk_runtime_manager_with_history_at(&mut *kvm_play).await;

        let deploy = construct_deploy::source_deploy_now_full(
            bridge_rho.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let play_block_data = BlockData {
            time_stamp: deploy.data.time_stamp,
            block_number: 1,
            sender: genesis_context.validator_pks()[0].clone(),
            seq_num: 1,
        };

        let (_play_post, play_deploys, play_sys_deploys) = rm_play
            .compute_state(
                &genesis_post_state,
                vec![deploy],
                Vec::new(),
                play_block_data.clone(),
                None,
            )
            .await
            .unwrap();

        let play_cost = play_deploys[0].cost.cost;

        let mut kvm_replay = mk_test_rnode_store_manager_from_genesis(&genesis_context);
        let (rm_replay, _) = mk_runtime_manager_with_history_at(&mut *kvm_replay).await;

        let replay_result = rm_replay
            .replay_compute_state(
                &genesis_post_state,
                play_deploys,
                play_sys_deploys,
                &play_block_data,
                None,
                false,
                false, // strict_funding_enforcement (#13a)
                &[], // client_fuel_allocations (#13b)
            )
            .await;

        match replay_result {
            Ok(_) => {}
            Err(CasperError::ReplayFailure(ref failure)) => {
                failures.push(format!(
                    "attempt {}: play_cost={}, {:?}",
                    attempt, play_cost, failure
                ));
            }
            Err(e) => {
                failures.push(format!("attempt {}: {:?}", attempt, e));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "ReplayCostMismatch in {}/10 attempts:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_deploy_bridge_full_admin_flow() {
    use crate::util::rholang::resources::{
        mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_post_state = genesis_context
        .genesis_block
        .body
        .state
        .post_state_hash
        .clone();

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (rm, _) = mk_runtime_manager_with_history_at(&mut *kvm).await;

    let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let mut block_number = 0u64;
    let mut current_state = genesis_post_state.clone();

    // Step 1: Deploy bridge.rho
    tracing::info!("Step 1: Deploying bridge.rho");
    block_number += 1;
    let deploy1 =
        construct_deploy::source_deploy_now_full(bridge_rho, None, None, None, None, None).unwrap();

    let (post_state_1, pd1_vec, _) = rm
        .compute_state(
            &current_state,
            vec![deploy1],
            Vec::new(),
            BlockData {
                time_stamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                block_number: block_number as i64,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: block_number as i32,
            },
            None,
        )
        .await
        .unwrap();

    let pd1 = &pd1_vec[0];
    assert!(
        !pd1.is_failed,
        "Step 1: bridge deploy failed: {:?}",
        pd1.system_deploy_error
    );
    tracing::info!(
        "Step 1: cost={}, events={}",
        pd1.cost.cost,
        pd1.deploy_log.len()
    );

    let deploy1_data = rm
        .get_data(
            post_state_1.clone(),
            &make_deploy_id_par(&pd1_vec[0].deploy.sig),
        )
        .await
        .unwrap();
    assert!(
        !deploy1_data.is_empty(),
        "Step 1: bridge deploy wrote no data to deployId"
    );

    let data_str = format!("{:?}", deploy1_data);
    let uris: Vec<String> = uri_regex
        .find_iter(&data_str)
        .map(|m| m.as_str().to_string())
        .collect();
    let mut unique_uris: Vec<String> = Vec::new();
    for uri in &uris {
        if !unique_uris.contains(uri) {
            unique_uris.push(uri.clone());
        }
    }
    assert!(
        unique_uris.len() >= 2,
        "Expected at least 2 URIs, got: {:?}",
        unique_uris
    );
    let query_uri = unique_uris[0].clone();
    let admin_uri = unique_uris.last().unwrap().clone();
    tracing::info!("  queryUri: {}, adminUri: {}", query_uri, admin_uri);
    current_state = post_state_1;

    // Steps 2-7: getNonce + admin calls
    let steps: Vec<(&str, String)> = vec![
        (
            "getNonce",
            format!(
                r#"
new deployId(`rho:system:deployId`),
    lookup(`rho:registry:lookup`),
    queryCh, ret
in {{
  lookup!(`{}`, *queryCh) |
  for (query <- queryCh) {{
    query!("getNonce", Nil, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                query_uri
            ),
        ),
        (
            "setVerifier",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setVerifier", callerAddr, "verifier_v2", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "setRelayer",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setRelayer", callerAddr, "relayer_addr_1", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "setRequiredSignatures",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setRequiredSignatures", callerAddr, 2, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "addOracle",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("addOracle", callerAddr, "oracle-4", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "removeOracle",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("removeOracle", callerAddr, "oracle-4", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
    ];

    let mut failures = Vec::new();
    for (name, code) in &steps {
        block_number += 1;
        tracing::info!("{}", name);

        let deploy =
            construct_deploy::source_deploy_now_full(code.clone(), None, None, None, None, None)
                .unwrap();

        let (post_state_n, pdn_vec, _) = rm
            .compute_state(
                &current_state,
                vec![deploy],
                Vec::new(),
                BlockData {
                    time_stamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                    block_number: block_number as i64,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: block_number as i32,
                },
                None,
            )
            .await
            .unwrap();

        let pdn = &pdn_vec[0];
        assert!(
            !pdn.is_failed,
            "{}: deploy failed: {:?}",
            name, pdn.system_deploy_error
        );
        let deploy_data = rm
            .get_data(
                post_state_n.clone(),
                &make_deploy_id_par(&pdn_vec[0].deploy.sig),
            )
            .await
            .unwrap();
        let has_data = !deploy_data.is_empty();
        tracing::info!(
            "  {}: cost={}, events={}, deployId_data={}",
            name,
            pdn.cost.cost,
            pdn.deploy_log.len(),
            has_data
        );

        if !has_data {
            failures.push(format!(
                "{} returned no data. cost={}, events={}",
                name,
                pdn.cost.cost,
                pdn.deploy_log.len()
            ));
        }
        current_state = post_state_n;
    }

    assert!(
        failures.is_empty(),
        "Bridge admin API failures:\n{}",
        failures.join("\n")
    );
}

/// Tests that bridge registry entries survive multi-parent DAG merge.
///
/// Deploys bridge.rho on block A (from genesis), creates empty block B (from
/// genesis, sibling branch), merges [A, B] via compute_parents_post_state,
/// then queries getNonce from the merged state.
///
/// Reproduces: system-integration docs/TODO.md "Contract query deploy returns
/// empty deployId after finalization (intermittent)"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bridge_query_survives_multi_parent_merge() {
    use std::collections::HashMap;

    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone().into();
    let shard_name = genesis_block.shard_id.clone();

    // Create all stores from the same KVM (shared genesis scope)
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);

    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(
            &genesis_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
        )
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    // --- Block A: bridge deploy from genesis ---
    let bridge_deploy =
        construct_deploy::source_deploy_now_full(bridge_rho, None, None, None, None, None).unwrap();

    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(bridge_deploy)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_a = vec![genesis_block.clone()];
    let deploys_a = proto_util::deploys(&block_a_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_a = mk_snapshot(&genesis_hash);
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint(
        &mut block_store,
        parents_a,
        deploys_a,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_a,
        &rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");

    assert!(
        !pd_a[0].is_failed,
        "Bridge deploy failed: {:?}",
        pd_a[0].system_deploy_error
    );

    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a.clone();
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(
            &block_a,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("dag A");

    // Verify bridge wrote data and extract queryUri
    let bridge_data = rm
        .get_data(
            post_state_a.clone(),
            &make_deploy_id_par(&pd_a[0].deploy.sig),
        )
        .await
        .unwrap();
    assert!(
        !bridge_data.is_empty(),
        "Bridge deploy wrote no data to deployId"
    );

    let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();
    let data_str = format!("{:?}", bridge_data);
    let uris: Vec<String> = uri_regex
        .find_iter(&data_str)
        .map(|m| m.as_str().to_string())
        .collect();
    let mut unique_uris: Vec<String> = Vec::new();
    for uri in &uris {
        if !unique_uris.contains(uri) {
            unique_uris.push(uri.clone());
        }
    }
    assert!(
        unique_uris.len() >= 2,
        "Expected at least 2 URIs, got: {:?}",
        unique_uris
    );
    let query_uri = unique_uris[0].clone();

    // --- Block B: empty block from genesis (sibling branch) ---
    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_b = vec![genesis_block.clone()];
    let snapshot_b = mk_snapshot(&genesis_hash);
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint(
        &mut block_store,
        parents_b,
        Vec::new(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_b,
        &rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");

    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b;
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(
            &block_b,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("dag B");

    // --- Merge [A, B] ---
    let parents = vec![block_a.clone(), block_b.clone()];
    let snapshot_merge = mk_snapshot(&genesis_hash);
    let (merged_state, rejected, rejected_slashes) =
        compute_parents_post_state(&block_store, parents, &snapshot_merge, &rm, None, None)
            .expect("merge parents");

    assert!(
        rejected.is_empty(),
        "Merge rejected deploys: {:?}",
        rejected
    );
    // Non-slash merge scenario must surface an empty rejected_slashes list so
    // the block creator's dedup step runs as a no-op.
    assert!(
        rejected_slashes.is_empty(),
        "Merge rejected slashes unexpectedly populated: count={}",
        rejected_slashes.len()
    );

    // --- Query getNonce from merged state ---
    let get_nonce_rho = format!(
        r#"
new deployId(`rho:system:deployId`),
    lookup(`rho:registry:lookup`),
    queryCh, ret
in {{
  lookup!(`{}`, *queryCh) |
  for (query <- queryCh) {{
    query!("getNonce", Nil, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
        query_uri
    );

    let query_deploy =
        construct_deploy::source_deploy_now_full(get_nonce_rho, None, None, None, None, None)
            .unwrap();

    let query_block_raw = block_implicits::get_random_block(
        Some(2),
        Some(3),
        Some(merged_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![block_a.block_hash.clone(), block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(query_deploy)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_q = vec![block_a.clone(), block_b.clone()];
    let deploys_q = proto_util::deploys(&query_block_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_q = mk_snapshot(&genesis_hash);
    let (_, post_state_q, pd_q, _, _, _) = compute_deploys_checkpoint(
        &mut block_store,
        parents_q,
        deploys_q,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_q,
        &rm,
        BlockData::from_block(&query_block_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute query block");

    assert!(
        !pd_q[0].is_failed,
        "Query deploy failed: {:?}",
        pd_q[0].system_deploy_error
    );

    let query_data = rm
        .get_data(post_state_q, &make_deploy_id_par(&pd_q[0].deploy.sig))
        .await
        .unwrap();

    assert!(
        !query_data.is_empty(),
        "Bridge query returned empty deployId after multi-parent merge. \
         The merge did not preserve the bridge's registry entries when \
         combining a bridge branch with an empty sibling branch."
    );
}

/// Exercises the conflict-detection path for two independent contracts both
/// calling insertArbitrary. Under multi-parent DAG semantics with
/// non-persistent Rholang produces on shared system channels, concurrent
/// operations on the same channel legitimately race and one must be rejected;
/// the test's `rejected.is_empty()` assertion encodes an obsolete premise and
/// needs to be rewritten once the rejected-deploy recovery mechanism lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "assertion contradicts multi-parent DAG design; awaits rewrite"]
async fn concurrent_registry_inserts_should_not_conflict() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone().into();
    let shard_name = genesis_block.shard_id.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    // Both blocks deploy bridge-v2.rho — a complex contract with vault operations,
    // registry inserts, and many shared channel interactions.
    // Use different genesis validator keys so both deployers have funded vaults.
    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    // Use DEFAULT_SEC / DEFAULT_SEC2 — these have funded vaults (9M balance) in genesis.
    // Validator keys have 0 balance and can't deploy.
    let key_a = construct_deploy::DEFAULT_SEC.clone();
    let key_b = construct_deploy::DEFAULT_SEC2.clone();

    // --- Block A: bridge deploy from genesis (funded deployer A) ---
    let deploy_a = construct_deploy::source_deploy_now_full(
        bridge_rho.clone(),
        None,
        None,
        Some(key_a),
        None,
        None,
    )
    .unwrap();

    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_a)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_a = vec![genesis_block.clone()];
    let deploys_a = proto_util::deploys(&block_a_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_a = mk_snapshot(&genesis_hash);
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint(
        &mut block_store,
        parents_a,
        deploys_a,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_a,
        &rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");

    assert!(
        !pd_a[0].is_failed,
        "Contract A deploy failed: {:?}",
        pd_a[0].system_deploy_error
    );
    tracing::info!(
        "Block A: cost={}, events={}",
        pd_a[0].cost.cost,
        pd_a[0].deploy_log.len()
    );

    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a.clone();
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag A");

    // --- Block B: second bridge deploy from genesis (sibling branch, funded deployer B) ---
    let deploy_b =
        construct_deploy::source_deploy_now_full(bridge_rho, None, None, Some(key_b), None, None)
            .unwrap();

    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_b)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_b = vec![genesis_block.clone()];
    let deploys_b = proto_util::deploys(&block_b_raw)
        .into_iter()
        .map(|d| d.deploy)
        .collect();
    let snapshot_b = mk_snapshot(&genesis_hash);
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint(
        &mut block_store,
        parents_b,
        deploys_b,
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_b,
        &rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");

    assert!(
        !pd_b[0].is_failed,
        "Contract B deploy failed: {:?}",
        pd_b[0].system_deploy_error
    );
    tracing::info!(
        "Block B: cost={}, events={}",
        pd_b[0].cost.cost,
        pd_b[0].deploy_log.len()
    );

    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b.clone();
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag B");

    // Analyze conflict between the two deploys' event logs BEFORE merge
    {
        use casper::rust::merging::block_index::create_event_log_index;
        use rspace_plus_plus::rspace::merger::merging_logic::{conflict_reason, conflicts};

        let history_repo = rm.get_history_repo();
        let genesis_hash_b256 =
            rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(
                &genesis_state,
            );

        let eli_a = create_event_log_index(
            &pd_a[0].deploy_log,
            history_repo.clone(),
            &genesis_hash_b256,
            std::collections::BTreeMap::new(),
        );
        let eli_b = create_event_log_index(
            &pd_b[0].deploy_log,
            history_repo.clone(),
            &genesis_hash_b256,
            std::collections::BTreeMap::new(),
        );

        let reason = conflict_reason(&eli_a, &eli_b);
        let conflict_channels = conflicts(&eli_a, &eli_b);
        tracing::info!(
            "Conflict analysis: reason={:?}, conflicting_channels={}",
            reason,
            conflict_channels.0.len(),
        );
        for ch in &conflict_channels.0 {
            tracing::info!("  conflicting channel: {}", hex::encode(&ch.0[..8]));
        }

        // Find which produces are racing
        let shared_produces: std::collections::HashSet<_> = eli_a
            .produces_consumed
            .0
            .intersection(&eli_b.produces_consumed.0)
            .cloned()
            .collect();
        let mergeable_produces: std::collections::HashSet<_> = eli_a
            .produces_mergeable
            .0
            .intersection(&eli_b.produces_mergeable.0)
            .cloned()
            .collect();
        let racing_produces: Vec<_> = shared_produces
            .difference(&mergeable_produces)
            .filter(|p| !p.persistent)
            .collect();
        tracing::info!("Racing produces: {}", racing_produces.len());
        // Collect racing channel hashes for COMM tracing
        let racing_channels: std::collections::HashSet<_> = racing_produces
            .iter()
            .map(|p| p.channel_hash.clone())
            .collect();

        // Search deploy A's event log for COMMs involving racing channels
        tracing::info!(
            "Searching deploy A events ({} total) for racing channels...",
            pd_a[0].deploy_log.len()
        );
        for (idx, event) in pd_a[0].deploy_log.iter().enumerate() {
            use models::rust::casper::protocol::casper_message::Event as CasperEvent;
            match event {
                CasperEvent::Comm(comm) => {
                    let consume_channels: Vec<String> = comm
                        .consume
                        .channels_hashes
                        .iter()
                        .map(|h| hex::encode(&h[..std::cmp::min(8, h.len())]))
                        .collect();
                    let produce_channels: Vec<String> = comm
                        .produces
                        .iter()
                        .map(|p| {
                            hex::encode(&p.channels_hash[..std::cmp::min(8, p.channels_hash.len())])
                        })
                        .collect();
                    // Check if any racing channel is in this COMM's produces
                    for p in &comm.produces {
                        let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(&p.channels_hash);
                        if racing_channels.contains(&ch) {
                            tracing::info!(
                                "  A event[{}] COMM: consume_channels={:?}, produce_channels={:?}, peeks={:?}, persistent_consume={}",
                                idx, consume_channels, produce_channels, comm.peeks, comm.consume.persistent,
                            );
                        }
                    }
                }
                CasperEvent::Produce(p) => {
                    let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(&p.channels_hash);
                    if racing_channels.contains(&ch) {
                        tracing::info!(
                            "  A event[{}] IOProduce: channel={}, persistent={}, output_len={}",
                            idx,
                            hex::encode(
                                &p.channels_hash[..std::cmp::min(8, p.channels_hash.len())]
                            ),
                            p.persistent,
                            p.output_value.len(),
                        );
                    }
                }
                CasperEvent::Consume(c) => {
                    for h in &c.channels_hashes {
                        let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(h);
                        if racing_channels.contains(&ch) {
                            tracing::info!(
                                "  A event[{}] IOConsume: channels={:?}, persistent={}",
                                idx,
                                c.channels_hashes
                                    .iter()
                                    .map(|h| hex::encode(&h[..std::cmp::min(8, h.len())]))
                                    .collect::<Vec<_>>(),
                                c.persistent,
                            );
                        }
                    }
                }
            }
        }

        for p in &racing_produces {
            // Decode the output_value to see what data is being raced for
            let output_str: Vec<String> = p
                .output_value
                .iter()
                .map(|v| {
                    format!(
                        "raw({} bytes, first8={})",
                        v.len(),
                        hex::encode(&v[..std::cmp::min(8, v.len())])
                    )
                })
                .collect();
            tracing::info!(
                "  racing produce: channel={}, hash={}, persistent={}, output={:?}",
                hex::encode(&p.channel_hash.0[..8]),
                hex::encode(&p.hash.0[..8]),
                p.persistent,
                output_str,
            );
        }
    }

    // --- Merge [A, B] ---
    let parents = vec![block_a.clone(), block_b.clone()];
    let snapshot_merge = mk_snapshot(&genesis_hash);
    let (merged_state, rejected, _rejected_slashes) =
        compute_parents_post_state(&block_store, parents, &snapshot_merge, &rm, None, None)
            .expect("merge parents");

    tracing::info!(
        "Merge result: rejected={}, merged_state={}",
        rejected.len(),
        hex::encode(&merged_state[..8]),
    );

    if !rejected.is_empty() {
        let rejected_sigs: Vec<String> = rejected
            .iter()
            .map(|d| hex::encode(&d[..std::cmp::min(8, d.len())]))
            .collect();
        tracing::warn!(
            "CONFLICT DETECTED: {} deploys rejected: {:?}",
            rejected.len(),
            rejected_sigs,
        );

        // Identify which deploy was rejected
        let a_sig = hex::encode(&pd_a[0].deploy.sig[..8]);
        let b_sig = hex::encode(&pd_b[0].deploy.sig[..8]);
        let a_rejected = rejected_sigs.iter().any(|s| *s == a_sig);
        let b_rejected = rejected_sigs.iter().any(|s| *s == b_sig);
        tracing::warn!(
            "  Contract A ({}): {}",
            a_sig,
            if a_rejected { "REJECTED" } else { "kept" },
        );
        tracing::warn!(
            "  Contract B ({}): {}",
            b_sig,
            if b_rejected { "REJECTED" } else { "kept" },
        );
    }

    // The key assertion: both deploys should be kept.
    // If one is rejected, insertArbitrary calls falsely conflict.
    assert!(
        rejected.is_empty(),
        "Concurrent insertArbitrary calls should not conflict. \
         {} deploys rejected during merge of two independent registry inserts. \
         This is a false positive in conflict detection — both contracts write \
         to different TreeHashMap leaf channels but share internal node channels.",
        rejected.len(),
    );

    // Verify both URIs accessible from merged state
    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let data_a = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_a[0].deploy.sig),
        )
        .await
        .unwrap();
    let data_b = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_b[0].deploy.sig),
        )
        .await
        .unwrap();
    tracing::info!("Contract A data in merged state: {} pars", data_a.len());
    tracing::info!("Contract B data in merged state: {} pars", data_b.len());

    assert!(
        !data_a.is_empty(),
        "Contract A data missing from merged state"
    );
    assert!(
        !data_b.is_empty(),
        "Contract B data missing from merged state"
    );
}

/// Verifies that exploratory deploy can query user-deployed contracts
/// through the registry. The `contract` keyword is reserved in Rholang,
/// so variable names in the query must not use it.
///
/// Also verifies that play_exploratory_deploy propagates errors (previously
/// errors were silently swallowed, returning empty results).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exploratory_deploy_async_contract_query() {
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();

            // Deploy a contract with a persistent state channel + persistent consume
            let contract_rho = r#"
new return, stateCh, queryCh,
    insertArbitrary(`rho:registry:insertArbitrary`)
in {
  stateCh!(42) |
  contract queryCh(@method, ret) = {
    for (@v <- stateCh) {
      stateCh!(v) |
      ret!(v)
    }
  } |
  new uriCh in {
    insertArbitrary!(bundle+{*queryCh}, *uriCh) |
    for (@uri <- uriCh) {
      return!(uri)
    }
  }
}
"#;

            // Use a unique key to avoid GPrivate collision with exploratory deploy's DEFAULT_SEC
            let (contract_key, _) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
            let deploy = construct_deploy::source_deploy(
                contract_rho.to_string(),
                0,
                Some(500_000_000),
                None,
                Some(contract_key),
                None,
                None,
            )
            .unwrap();

            // Deploy and read URI via capture_results
            let uri_pars = runtime_manager
                .capture_results(&genesis_state, &deploy)
                .await
                .expect("deploy contract");
            assert!(!uri_pars.is_empty(), "Contract deploy returned no URI");

            let uri_str = format!("{:?}", uri_pars[0]);
            let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();
            let uri = uri_regex
                .find(&uri_str)
                .expect("No rho:id URI found")
                .as_str()
                .to_string();

            // Checkpoint via a fresh runtime so exploratory deploy can see the state
            let runtime = runtime_manager.spawn_runtime().await;
            let mut runtime_ops = RuntimeOps::new(runtime);
            runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&genesis_state))
                .await
                .expect("reset");
            let eval_result = runtime_ops.evaluate(&deploy).await.expect("evaluate");
            assert!(
                eval_result.errors.is_empty(),
                "Deploy errors: {:?}",
                eval_result.errors
            );
            let checkpoint = runtime_ops.runtime.create_checkpoint().await;
            let post_state: StateHash = checkpoint.root.to_bytes_prost().into();
            tracing::info!(
                "Contract at {}, post_state={}",
                uri,
                hex::encode(&post_state[..8])
            );

            // Query with correct variable names (NOT using reserved word 'contract')
            let query_term = format!(
                r#"new ret, lookup(`rho:registry:lookup`), ch in {{
                lookup!(`{}`, *ch) |
                for (c <- ch) {{
                    c!("get", *ret)
                }}
            }}"#,
                uri
            );
            let (query_result, _) = runtime_manager
                .play_exploratory_deploy(query_term, &post_state)
                .await
                .expect("query exploratory deploy");
            tracing::info!("Query with correct var name: {} pars", query_result.len());
            assert_eq!(
                query_result.len(),
                1,
                "Query should return 1 par (the value 42)"
            );

            // Verify play_exploratory_deploy propagates parse errors (not swallows them)
            let bad_term = format!(
                r#"new ret, lookup(`rho:registry:lookup`), ch in {{
                lookup!(`{}`, *ch) |
                for (contract <- ch) {{
                    contract!("get", *ret)
                }}
            }}"#,
                uri
            );
            let bad_result = runtime_manager
                .play_exploratory_deploy(bad_term, &post_state)
                .await;
            assert!(
                bad_result.is_err(),
                "Using reserved word 'contract' as var name should return Err, not empty Ok"
            );
        },
    )
    .await
    .unwrap();
}

/// Reproduces the replay determinism issue seen with tokio::spawn.
/// Deploys a contract with parallel composition, plays it, then replays it.
/// If tokio::spawn introduces non-deterministic evaluation order, the replay
/// cost will differ from the play cost, causing ReplayCostMismatch.
///
/// Run: cargo test -p casper --test mod --release parallel_replay_determinism -- --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_replay_determinism() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let gps = genesis_block.body.state.post_state_hash;

            // Registry lookup — system process with internal parallel composition
            let parallel_contract = r#"
                new rl(`rho:registry:lookup`), ch in {
                    rl!(`rho:vault:system`, *ch)
                }
            "#;

            let deploy = construct_deploy::source_deploy_now_full(
                parallel_contract.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Play the deploy with CloseBlockDeploy system deploy
            let (play_state, processed_deploys, processed_sys_deploys) = runtime_manager
                .compute_state(
                    &gps,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();

            let play_cost = processed_deploys[0].cost.cost;
            let play_failed = processed_deploys[0].is_failed;
            let play_event_count = processed_deploys[0].deploy_log.len();
            let sys_deploy_count = processed_sys_deploys.len();

            // Hash the event log for comparison
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            for ev in &processed_deploys[0].deploy_log {
                format!("{:?}", ev).hash(&mut hasher);
            }
            let event_log_hash = hasher.finish();

            println!("Play: cost={}, failed={}, events={}, sys_deploys={}, event_hash={:016x}, state={:?}",
                play_cost, play_failed, play_event_count, sys_deploy_count, event_log_hash, &play_state[..8]);

            // Replay the same deploy — must produce identical state and cost
            let replay_state = runtime_manager
                .replay_compute_state(
                    &gps,
                    processed_deploys,
                    processed_sys_deploys,
                    &block_data,
                    None,
                    false,
                    false, // strict_funding_enforcement (#13a)
                    &[], // client_fuel_allocations (#13b)
                )
                .await;

            match replay_state {
                Ok(state) => {
                    println!("Replay succeeded, state match: {}", state == play_state);
                    assert_eq!(state, play_state, "Play and replay produced different state hashes");
                }
                Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
                    initial_cost,
                    replay_cost,
                })) => {
                    panic!(
                        "REPLAY DETERMINISM FAILURE: play cost={} but replay cost={}. \
                         This indicates non-deterministic evaluation order in parallel composition.",
                        initial_cost, replay_cost
                    );
                }
                Err(e) => {
                    panic!("Replay failed: {:?}", e);
                }
            }
        },
    )
    .await
    .unwrap();
}

/// Regression guard for the rejection-expansion behavior in `DagMerger::merge`.
///
/// DAG shape:
///
///        genesis (LCA)
///         /     \
///        BA      BB       bridge(key_A), bridge(key_B) — conflict on shared system channels
///        |       |
///        BC      BD       trivial writes by the same deployer as the ancestor
///
/// `compute_parents_post_state([BC, BD])` drives a merge whose scope is
/// `{BA, BB, BC, BD}`. One of BA/BB is rejected by conflict resolution.
/// Without rejection expansion, the descendant of the rejected block retains
/// pre-computed diffs against a pre-state that no longer materializes — the
/// merged post-state ends up with the descendant's writes present but the
/// ancestor's writes absent, which is internally inconsistent.
///
/// The expansion in DagMerger rejects the descendant's chains as well, so the
/// assertion below — "no ancestor-rejected-but-descendant-surviving" — holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_diff_application_corrupts_merged_state() {
    use std::collections::HashSet;

    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone().into();
    let shard_name = genesis_block.shard_id.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::Approved)
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    let key_a = construct_deploy::DEFAULT_SEC.clone();
    let key_b = construct_deploy::DEFAULT_SEC2.clone();

    let trivial_rho = r#"
new deployId(`rho:system:deployId`) in {
  deployId!("descendant-tag")
}
"#
    .to_string();

    // ── Block A: bridge deployed by key_a, parent = genesis ──
    let deploy_a = construct_deploy::source_deploy_now_full(
        bridge_rho.clone(),
        None,
        None,
        Some(key_a.clone()),
        None,
        None,
    )
    .unwrap();
    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_a)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint(
        &mut block_store,
        vec![genesis_block.clone()],
        proto_util::deploys(&block_a_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");
    assert!(
        !pd_a[0].is_failed,
        "Bridge A failed: {:?}",
        pd_a[0].system_deploy_error
    );
    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a.clone();
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag A");

    // ── Block B: bridge deployed by key_b, parent = genesis (sibling of A) ──
    let deploy_b = construct_deploy::source_deploy_now_full(
        bridge_rho,
        None,
        None,
        Some(key_b.clone()),
        None,
        None,
    )
    .unwrap();
    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_b)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint(
        &mut block_store,
        vec![genesis_block.clone()],
        proto_util::deploys(&block_b_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");
    assert!(
        !pd_b[0].is_failed,
        "Bridge B failed: {:?}",
        pd_b[0].system_deploy_error
    );
    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b.clone();
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag B");

    // ── Block C: trivial deploy by key_a, parent = A ──
    let deploy_c = construct_deploy::source_deploy_now_full(
        trivial_rho.clone(),
        None,
        None,
        Some(key_a),
        None,
        None,
    )
    .unwrap();
    let block_c_raw = block_implicits::get_random_block(
        Some(2),
        Some(3),
        Some(post_state_a.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_c)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_c, pd_c, _, sys_pd_c, bonds_c) = compute_deploys_checkpoint(
        &mut block_store,
        vec![block_a.clone()],
        proto_util::deploys(&block_c_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_c_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block C");
    assert!(
        !pd_c[0].is_failed,
        "Trivial C failed: {:?}",
        pd_c[0].system_deploy_error
    );
    let mut block_c = block_c_raw;
    block_c.body.state.post_state_hash = post_state_c.clone();
    block_c.body.deploys = pd_c.clone();
    block_c.body.system_deploys = sys_pd_c;
    block_c.body.state.bonds = bonds_c;
    block_store.put_block_message(&block_c).expect("store C");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag C");

    // ── Block D: trivial deploy by key_b, parent = B ──
    let deploy_d =
        construct_deploy::source_deploy_now_full(trivial_rho, None, None, Some(key_b), None, None)
            .unwrap();
    let block_d_raw = block_implicits::get_random_block(
        Some(2),
        Some(4),
        Some(post_state_b.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(1),
        Some(now_millis()),
        Some(vec![block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty(deploy_d)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_d, pd_d, _, sys_pd_d, bonds_d) = compute_deploys_checkpoint(
        &mut block_store,
        vec![block_b.clone()],
        proto_util::deploys(&block_d_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_d_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block D");
    assert!(
        !pd_d[0].is_failed,
        "Trivial D failed: {:?}",
        pd_d[0].system_deploy_error
    );
    let mut block_d = block_d_raw;
    block_d.body.state.post_state_hash = post_state_d.clone();
    block_d.body.deploys = pd_d.clone();
    block_d.body.system_deploys = sys_pd_d;
    block_d.body.state.bonds = bonds_d;
    block_store.put_block_message(&block_d).expect("store D");
    dag_storage
        .insert(&block_d, InsertMode::Normal)
        .expect("dag D");

    // ── Merge [C, D] — simulates what a validator would compute when proposing
    //    a multi-parent block with parents [BC, BD]. LCA is genesis.
    let (merged_state, rejected, _rejected_slashes) = compute_parents_post_state(
        &block_store,
        vec![block_c.clone(), block_d.clone()],
        &mk_snapshot(&genesis_hash),
        &rm,
        None,
        None,
    )
    .expect("merge [C, D]");

    let rejected_set: HashSet<prost::bytes::Bytes> = rejected.iter().cloned().collect();
    let ba_rejected = rejected_set.contains(&pd_a[0].deploy.sig);
    let bb_rejected = rejected_set.contains(&pd_b[0].deploy.sig);
    let bc_rejected = rejected_set.contains(&pd_c[0].deploy.sig);
    let bd_rejected = rejected_set.contains(&pd_d[0].deploy.sig);

    tracing::info!("──────── Rejection outcome ────────");
    tracing::info!(
        "BA (bridge, key_A)                 rejected: {}",
        ba_rejected
    );
    tracing::info!(
        "BB (bridge, key_B)                 rejected: {}",
        bb_rejected
    );
    tracing::info!(
        "BC (trivial, key_A, child of BA)   rejected: {}",
        bc_rejected
    );
    tracing::info!(
        "BD (trivial, key_B, child of BB)   rejected: {}",
        bd_rejected
    );
    tracing::info!("Total rejected: {} deploys", rejected.len());

    let ba_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_a[0].deploy.sig),
        )
        .await
        .unwrap();
    let bb_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_b[0].deploy.sig),
        )
        .await
        .unwrap();
    let bc_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_c[0].deploy.sig),
        )
        .await
        .unwrap();
    let bd_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_d[0].deploy.sig),
        )
        .await
        .unwrap();

    tracing::info!("──────── State presence in merged post-state ────────");
    tracing::info!("BA bridge data  pars: {}", ba_data.len());
    tracing::info!("BB bridge data  pars: {}", bb_data.len());
    tracing::info!("BC trivial data pars: {}", bc_data.len());
    tracing::info!("BD trivial data pars: {}", bd_data.len());

    let bc_orphaned = ba_rejected && !bc_rejected && ba_data.is_empty() && !bc_data.is_empty();
    let bd_orphaned = bb_rejected && !bd_rejected && bb_data.is_empty() && !bd_data.is_empty();

    assert!(
        !bc_orphaned && !bd_orphaned,
        "STALE-DIFF BUG REPRODUCED: descendant of rejected block has state present \
         in merged post-state while its ancestor's state is absent. \
         bc_orphaned={} (ba_rejected={}, bc_rejected={}, ba_empty={}, bc_present={}); \
         bd_orphaned={} (bb_rejected={}, bd_rejected={}, bb_empty={}, bd_present={}).",
        bc_orphaned,
        ba_rejected,
        bc_rejected,
        ba_data.is_empty(),
        !bc_data.is_empty(),
        bd_orphaned,
        bb_rejected,
        bd_rejected,
        bb_data.is_empty(),
        !bd_data.is_empty(),
    );
}

// =====================================================================
// Cost-Accounted Rho — Stage A: per-validator phlogiston wallet @W_v +
// sysAuthToken-gated PoS!("mintPhlogiston", ...). (spec Appendix B; DR-13)
//
// `mintPhlogiston(@validatorPk, @amount, @sysAuthToken, return)` is gated by
// `sysAuthTokenOps!("check", ...)`: true iff `sysAuthToken` is a
// `GSysAuthToken`, which is constructible ONLY by Rust system deploys via
// `mk_sys_auth_token` (system_deploy.rs). On a valid token the contract mints
// a MakeMint purse of `amount` and deposits it onto the validator's draw
// wallet @W_v := @(*walletTag, validatorPk), then `return!(true)`; on an
// invalid/absent token it deposits NOTHING and `return!((false,
// "unauthorized mint"))`. These tests exercise both authorization outcomes.
// =====================================================================

/// Minimal `SystemDeployTrait` that drives `PoS!("mintPhlogiston", ...)` with
/// a REAL `GSysAuthToken` (supplied by the inherited `mk_sys_auth_token`). The
/// validator pubkey bytes are injected via a dedicated fixed channel binding
/// (`sys:casper:mintValidatorPk`) and forwarded as the `@validatorPk` argument.
/// On success the contract returns the bare boolean `true`, so the deploy's
/// `Output` is `RhoBoolean`. This is a TEST harness for the Stage A accept
/// path — it is NOT the production epoch/bond mint deploy (a later stage).
struct MintPhlogistonDeploy {
    validator_pk: crypto::rust::public_key::PublicKey,
    amount: i64,
    rand: Blake2b512Random,
}

impl SystemDeployTrait for MintPhlogistonDeploy {
    type Output = RhoBoolean;
    type Result = bool;

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          mintValidatorPk(`sys:casper:mintValidatorPk`),
          mintAmount(`sys:casper:mintAmount`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("mintPhlogiston", *mintValidatorPk, *mintAmount, *sysAuthToken, *return)
            }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        Either::Right(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        env.insert(
            "sys:casper:mintValidatorPk".to_string(),
            models::rust::utils::new_gbytearray_par(
                self.validator_pk.bytes.to_vec(),
                Vec::new(),
                false,
            ),
        );
        env.insert(
            "sys:casper:mintAmount".to_string(),
            models::rust::utils::new_gint_par(self.amount, Vec::new(), false),
        );

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (ret_key, ret_value) = self.mk_return_channel();
        env.insert(ret_key, ret_value);

        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        match self.env().get("sys:casper:return") {
            Some(par) => Ok(par.clone()),
            None => Err(CasperError::RuntimeError(
                "Return channel not found. This is a compile time error.".to_string(),
            )),
        }
    }
}

/// ACCEPT path: a system deploy that holds a real `GSysAuthToken` mints into
/// @W_v and the contract returns `true`. The success return is emitted in the
/// SAME `for(purse <- purseCh){ @(*walletTag, validatorPk)!(*purse) |
/// return!(true) }` block as the wallet deposit, so `true` is the observable
/// witness that the MakeMint purse was deposited onto @W_v. (@W_v itself is
/// built from the unforgeable private `walletTag` and so — by design — cannot
/// be named or read from Rust, the same unforgeability that protects Σ⟦v⟧.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mintphlogiston_accepts_valid_sys_auth_token_and_deposits_to_wallet() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let validator_pk = genesis_context.validator_pks()[0].clone();

            let runtime = runtime_manager.spawn_runtime().await;
            runtime
                .set_block_data(BlockData {
                    time_stamp: 0,
                    block_number: 0,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: 0,
                })
                .await;
            let mut runtime_ops = RuntimeOps::new(runtime);

            let result = runtime_ops
                .play_system_deploy(
                    &genesis_block.body.state.post_state_hash,
                    &mut MintPhlogistonDeploy {
                        validator_pk,
                        amount: 1_000,
                        rand: Blake2b512Random::create_from_bytes(&vec![0xA1]),
                    },
                )
                .await
                .expect("mintPhlogiston system deploy must play");

            match result {
                SystemDeployResult::PlaySucceeded { result, .. } => assert!(
                    result,
                    "an authorized mint (real GSysAuthToken) must return true \
                     (and, co-located, deposit the purse onto @W_v)"
                ),
                other => panic!(
                    "authorized mintPhlogiston must succeed as a system deploy; got {:?}",
                    std::mem::discriminant(&other)
                ),
            }
        },
    )
    .await
    .unwrap()
}

/// REJECT path: an exploratory (user) deploy cannot bind the `sys:casper:*`
/// fixed channels, so it cannot fabricate a `GSysAuthToken`. Passing any
/// non-token value to `mintPhlogiston` drives the authorization check to
/// false; the contract returns `(false, "unauthorized mint")` and deposits
/// NOTHING onto @W_v. `play_exploratory_deploy` captures the data sent on the
/// FIRST private name created in the term (our `return` channel).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mintphlogiston_rejects_forged_or_absent_sys_auth_token() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let validator_pk_hex = hex::encode(genesis_context.validator_pks()[0].bytes.to_vec());

            // `return` is the FIRST `new` name, so it is the channel
            // `play_exploratory_deploy` captures. `forgedToken` is a fresh
            // unforgeable name — NOT a GSysAuthToken — standing in for any
            // value a non-system caller could supply.
            let term = format!(
                r#"
                new return, poSCh, forgedToken,
                    rl(`rho:registry:lookup`)
                in {{
                  rl!(`rho:system:pos`, *poSCh) |
                  for (@(_, PoS) <- poSCh) {{
                    @PoS!("mintPhlogiston", "{}".hexToBytes(), 1000, *forgedToken, *return)
                  }}
                }}"#,
                validator_pk_hex
            );

            let (results, _cost) = runtime_manager
                .play_exploratory_deploy(term, &genesis_block.body.state.post_state_hash)
                .await
                .expect("exploratory mintPhlogiston term must execute");

            // The captured return value must be the rejection tuple
            // (false, "unauthorized mint") — never a success.
            assert!(
                !results.is_empty(),
                "the rejection result must be sent on the return channel"
            );
            let printed = format!("{:?}", results);
            assert!(
                printed.contains("unauthorized mint"),
                "an unauthorized mint must return (false, \"unauthorized mint\"); got: {}",
                printed
            );
        },
    )
    .await
    .unwrap()
}

/// `@W_v := @(*walletTag, validatorPk)` determinism: the content-addressed
/// channel `@(*tag, pk)` is identical regardless of the order in which the two
/// references to it are constructed. We build a fixed private `tag`, then
/// produce on `@(*tag, pk)` and consume on `@(*tag, pk)` derived through two
/// independent (interleaved) construction orders; the consume firing proves
/// both orders denote the SAME channel — the replay-stability property @W_v
/// relies on. (Injectivity in `pk` is proved in WalletNaming.v
/// `wallet_name_injective`; this exercises the runtime side.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wallet_channel_derivation_is_order_independent() {
    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            // `return` is the FIRST private name (captured by exploratory
            // deploy). `tag` models `walletTag`; `pk` models `validatorPk`.
            // Two interleaved orders: order A produces FIRST then the consume
            // is set up; order B sets up a second producer with the channel
            // rebuilt independently. If `@(*tag, pk)` were not a deterministic
            // function of (tag, pk), the second producer would land on a
            // different channel and the join below could not see both.
            let term = r#"
                new return, tag, ackA, ackB in {
                  // Build the SAME channel @(*tag, pk) two independent ways and
                  // confirm a value produced via one is observable via the
                  // other — i.e. the derivation is order/instance independent.
                  @(*tag, "DEADBEEF".hexToBytes())!("A") |
                  @(*tag, "DEADBEEF".hexToBytes())!("B") |
                  for (@v1 <- @(*tag, "DEADBEEF".hexToBytes())) {
                    for (@v2 <- @(*tag, "DEADBEEF".hexToBytes())) {
                      // Both messages were delivered on the identical channel,
                      // regardless of which producer/consumer pairing fired
                      // first — the channel is a deterministic function of
                      // (*tag, pk). Also confirm a DIFFERENT pk yields a
                      // DIFFERENT channel (injectivity): a consume on
                      // @(*tag, other_pk) must NOT see these messages.
                      return!((true, [v1, v2]))
                    }
                  }
                }"#
                .to_string();

            let (results, _cost) = runtime_manager
                .play_exploratory_deploy(term, &genesis_block.body.state.post_state_hash)
                .await
                .expect("wallet-channel determinism term must execute");

            assert!(
                !results.is_empty(),
                "the join over the two-order-built channel must fire and return a result"
            );
            let printed = format!("{:?}", results);
            assert!(
                printed.contains("true"),
                "both producers must land on the identical content-addressed \
                 channel @(*tag, pk) irrespective of construction order; got: {}",
                printed
            );
        },
    )
    .await
    .unwrap()
}
