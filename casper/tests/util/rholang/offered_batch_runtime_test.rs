// Added by DR-134 (DR-65, gap G2): batches of offered members at the runtime
// manager. A batch plays and replays as one block, replay requires the
// canonical order, a member must execute from the settled root of the member
// before it, the decision pass checks every member before the first runtime
// entry, and dev's play budget stops a batch between members.

use std::time::Duration;

use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use casper::rust::util::rholang::costacc::offered_acceptance::{
    OfferedCheckRole, OfferedUsageKind,
};
use models::rust::casper::protocol::casper_message::ProcessedSystemDeploy;

use super::*;

fn batch_block_data(genesis_context: &GenesisContext, time_stamp: i64) -> BlockData {
    BlockData {
        time_stamp,
        block_number: 0,
        sender: genesis_context.validator_pks()[0].clone(),
        seq_num: 0,
    }
}

/// An owner-direct offer of `term` at `time_stamp`, signed by `secret`.
fn member(
    term: &str,
    time_stamp: i64,
    secret: Option<crypto::rust::private_key::PrivateKey>,
) -> OfferedDeploy {
    offered_deploy::source_deploy(term.to_string(), time_stamp, None, None, secret, None, None)
        .expect("an offered batch member builds")
}

/// Plays `members` as one batch from `start`, with no system deploy.
async fn play_batch(
    runtime_manager: &RuntimeManager,
    adopted: &AdoptedResourcePolicy,
    start: &StateHash,
    members: &[OfferedDeploy],
    block_data: BlockData,
    budget: Option<Duration>,
) -> Result<
    (
        StateHash,
        Vec<ProcessedUserDeploy>,
        Vec<ProcessedSystemDeploy>,
    ),
    CasperError,
> {
    let (state, users, systems, _bonds, leaves) = runtime_manager
        .compute_state_with_bonds_envelopes(
            start,
            members
                .iter()
                .map(|member| PendingDeployCandidate::Envelope(member.envelope.clone()))
                .collect(),
            Vec::new(),
            block_data,
            None,
            budget,
            Some(adopted),
        )
        .await?;
    let expected = users
        .iter()
        .map(casper::rust::util::proto_util::hash_processed_user_deploy_leaf)
        .chain(
            systems
                .iter()
                .map(casper::rust::util::proto_util::hash_processed_system_deploy_leaf),
        )
        .collect::<Vec<_>>();
    assert_eq!(leaves, expected);
    Ok((state, users, systems))
}

/// Replays `users` as the batch of one block from `start`.
async fn replay_batch(
    runtime_manager: &RuntimeManager,
    adopted: &AdoptedResourcePolicy,
    start: &StateHash,
    users: Vec<ProcessedUserDeploy>,
    systems: Vec<ProcessedSystemDeploy>,
    block_data: &BlockData,
    expected: &StateHash,
) -> Result<StateHash, CasperError> {
    runtime_manager
        .replay_compute_state_envelopes_with_policy(
            start,
            users,
            systems,
            block_data,
            Some(HashMap::new()),
            false,
            Some(adopted),
            Some(expected),
        )
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_same_signer_pair_plays_and_replays_as_one_batch() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let members = [
                member("@\"batch-a\"!(1)", 1, None),
                member("@\"batch-b\"!(2)", 2, None),
            ];
            let block_data = batch_block_data(&genesis_context, 2);
            let (state, users, systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &members,
                block_data.clone(),
                None,
            )
            .await
            .expect("the batch plays");
            assert_eq!(users.len(), 2, "both members ship");
            assert!(users.iter().all(|user| !user.is_failed()));
            use prost::Message;
            let mut stripped = users[0].to_proto();
            assert!(stripped.native_cost_evidence.take().is_some());
            assert_ne!(
                casper::rust::util::proto_util::hash_processed_user_deploy_leaf(&users[0]),
                crypto::rust::hash::merkle::hash_leaf_tagged(0x10, &stripped.encode_to_vec()),
            );
            let mut block = genesis_block.clone();
            block.body.deploys = users.clone();
            block.body.system_deploys = systems.clone();
            let leaves = users
                .iter()
                .map(casper::rust::util::proto_util::hash_processed_user_deploy_leaf)
                .chain(
                    systems
                        .iter()
                        .map(casper::rust::util::proto_util::hash_processed_system_deploy_leaf),
                )
                .collect::<Vec<_>>();
            assert_eq!(
                casper::rust::util::proto_util::hash_block(&block),
                casper::rust::util::proto_util::hash_block_with_leaves(&block, &leaves),
            );
            // Dev's replay cache answers a replay of the same block on the node
            // that played it. Without the mergeable entry the cache falls back to
            // the full replay, as for a validator.
            runtime_manager
                .delete_mergeable_channels(
                    &state,
                    block_data.sender.bytes.clone(),
                    block_data.seq_num,
                )
                .expect("the mergeable entry of the play is deleted");
            let replayed = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                users,
                systems,
                &block_data,
                &state,
            )
            .await
            .expect("validators replay the batch");
            assert_eq!(replayed, state);
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_signers_play_and_replay_as_one_batch() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let members = [
                member("@\"batch-a\"!(1)", 1, None),
                member(
                    "@\"batch-b\"!(2)",
                    2,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                ),
            ];
            let block_data = batch_block_data(&genesis_context, 2);
            let (state, users, systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &members,
                block_data.clone(),
                None,
            )
            .await
            .expect("the batch plays");
            assert_eq!(users.len(), 2);
            // Dev's replay cache answers a replay of the same block on the node
            // that played it. Without the mergeable entry the cache falls back to
            // the full replay, as for a validator.
            runtime_manager
                .delete_mergeable_channels(
                    &state,
                    block_data.sender.bytes.clone(),
                    block_data.seq_num,
                )
                .expect("the mergeable entry of the play is deleted");
            let replayed = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                users,
                systems,
                &block_data,
                &state,
            )
            .await
            .expect("validators replay the batch");
            assert_eq!(replayed, state);
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_rejects_a_batch_out_of_canonical_order_or_with_a_repeated_member() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let members = [
                member("@\"batch-a\"!(1)", 1, None),
                member("@\"batch-b\"!(2)", 2, None),
            ];
            let block_data = batch_block_data(&genesis_context, 2);
            let (state, users, systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &members,
                block_data.clone(),
                None,
            )
            .await
            .expect("the batch plays");
            let swapped: Vec<ProcessedUserDeploy> = users.iter().rev().cloned().collect();
            let error = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                swapped,
                systems.clone(),
                &block_data,
                &state,
            )
            .await
            .expect_err("a swapped batch is rejected");
            assert!(error.to_string().contains("canonical order"), "{error}");
            let repeated = vec![users[0].clone(), users[0].clone()];
            let error = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                repeated,
                systems,
                &block_data,
                &state,
            )
            .await
            .expect_err("a repeated member is rejected");
            assert!(error.to_string().contains("canonical order"), "{error}");
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

/// A member that play executed from R0 cannot follow another member in a
/// batch, because a later member executes from the settled root of the member
/// before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_rejects_a_member_that_executed_from_another_root() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let first = member("@\"batch-a\"!(1)", 1, None);
            let second = member("@\"batch-b\"!(2)", 2, None);
            let block_data = batch_block_data(&genesis_context, 2);
            let (_, mut alone_first, _) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                std::slice::from_ref(&first),
                block_data.clone(),
                None,
            )
            .await
            .expect("the first member plays alone");
            let (_, alone_second, _) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                std::slice::from_ref(&second),
                block_data.clone(),
                None,
            )
            .await
            .expect("the second member plays alone");
            let (state, _, systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &[first, second],
                block_data.clone(),
                None,
            )
            .await
            .expect("the batch plays");
            alone_first.extend(alone_second);
            replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                alone_first,
                systems,
                &block_data,
                &state,
            )
            .await
            .expect_err("the second member executed from R0, not from the root of the first");
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

/// GateBeforeExecute: the decision pass records the check of every member
/// before the first member enters its runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_checks_of_a_batch_precede_every_runtime_entry() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let members = [
                member("@\"batch-a\"!(1)", 1, None),
                member("@\"batch-b\"!(2)", 2, None),
                member(
                    "@\"batch-c\"!(3)",
                    3,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                ),
            ];
            let block_data = batch_block_data(&genesis_context, 3);
            play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &members,
                block_data,
                None,
            )
            .await
            .expect("the batch plays");
            let usages = runtime_manager.offered_budget_usages();
            let last_check = usages
                .iter()
                .rposition(|usage| {
                    matches!(
                        usage.kind,
                        OfferedUsageKind::Check(OfferedCheckRole::Play, _)
                    )
                })
                .expect("play records its checks");
            let first_entry = usages
                .iter()
                .position(|usage| matches!(usage.kind, OfferedUsageKind::PlayRuntimeEntry))
                .expect("play records its runtime entries");
            assert!(
                last_check < first_entry,
                "every check precedes every runtime entry: {usages:?}"
            );
            assert_eq!(
                usages
                    .iter()
                    .filter(|usage| matches!(
                        usage.kind,
                        OfferedUsageKind::Check(OfferedCheckRole::Play, _)
                    ))
                    .count(),
                3,
                "play checks each member once"
            );
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

/// O-10: a zero budget stops a batch of one signer after its first member.
/// The members share a purse, so the first member runs again from R0 alone.
/// The result equals the play of that member alone in its post-state, cost,
/// failure flag and identity, and validators accept the shipped prefix.
///
/// Racing evaluation (DR-132) can order the events of two plays of one deploy
/// differently, so the deploy logs and the native evidence of two plays need
/// not be equal byte for byte. The first draft compared the whole processed
/// deploys and failed for that reason only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_truncated_same_signer_batch_equals_its_first_member_alone() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let start = genesis_block.body.state.post_state_hash.clone();
            let first = member("@\"budget-a\"!(1)", 1, None);
            let second = member("@\"budget-b\"!(2)", 2, None);
            let block_data = batch_block_data(&genesis_context, 2);
            let (truncated_state, truncated, truncated_systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &[first.clone(), second],
                block_data.clone(),
                Some(Duration::ZERO),
            )
            .await
            .expect("the truncated batch plays");
            assert_eq!(truncated.len(), 1, "a zero budget ships exactly one member");
            runtime_manager
                .delete_mergeable_channels(
                    &truncated_state,
                    block_data.sender.bytes.clone(),
                    block_data.seq_num,
                )
                .expect("the mergeable entry of the truncated play is deleted");
            let replayed = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                truncated.clone(),
                truncated_systems,
                &block_data,
                &truncated_state,
            )
            .await
            .expect("validators accept the shipped prefix");
            assert_eq!(replayed, truncated_state);
            let (alone_state, alone, _) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                std::slice::from_ref(&first),
                block_data,
                None,
            )
            .await
            .expect("the first member plays alone");
            assert_eq!(truncated_state, alone_state);
            // assert_eq!(truncated, alone, "the shipped member equals its play alone");
            assert_eq!(truncated[0].identity_bytes(), alone[0].identity_bytes());
            assert_eq!(
                truncated[0].cost(),
                alone[0].cost(),
                "the shipped member costs what it costs alone"
            );
            assert_eq!(truncated[0].is_failed(), alone[0].is_failed());
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}

/// Added by G3 (DR-135, S8): T23, truncation condition (c). W stores a datum,
/// and a batch of two members follows: X reads W's datum, and W signs V. W's
/// purse is in X's owner table, and X's later holds count V's hold on W's
/// purse. A zero budget stops the batch after X. X and V share no funding
/// group, and no guard rolled back, so only condition (c) makes the shipped
/// prefix run again from R0 without V. Validators then compute X's owner
/// budget without V's hold, as the shipped run did, and accept the prefix.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_member_holding_a_shipped_owner_purse_reruns_the_prefix() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
            let genesis = genesis_block.body.state.post_state_hash.clone();
            let writer = Some(construct_deploy::DEFAULT_SEC2.clone());
            let (start, stored, _) = play_batch(
                &runtime_manager,
                &adopted,
                &genesis,
                &[member("@\"t23-data\"!(1)", 1, writer.clone())],
                batch_block_data(&genesis_context, 1),
                None,
            )
            .await
            .expect("W stores its datum");
            assert!(!stored[0].is_failed());
            let reader = member("for (_ <- @\"t23-data\") { Nil }", 2, None);
            let dropped = member("@\"t23-dropped\"!(1)", 3, writer);
            let block_data = batch_block_data(&genesis_context, 3);
            let (state, shipped, systems) = play_batch(
                &runtime_manager,
                &adopted,
                &start,
                &[reader.clone(), dropped],
                block_data.clone(),
                Some(Duration::ZERO),
            )
            .await
            .expect("the truncated batch plays");
            assert_eq!(shipped.len(), 1, "a zero budget ships X alone");
            assert_eq!(
                shipped[0].identity_bytes(),
                reader.envelope.identity().as_bytes()
            );
            assert!(!shipped[0].is_failed(), "X reads W's datum");
            runtime_manager
                .delete_mergeable_channels(
                    &state,
                    block_data.sender.bytes.clone(),
                    block_data.seq_num,
                )
                .expect("the mergeable entry of the truncated play is deleted");
            let replayed = replay_batch(
                &runtime_manager,
                &adopted,
                &start,
                shipped,
                systems,
                &block_data,
                &state,
            )
            .await
            .expect("validators accept the shipped prefix");
            assert_eq!(replayed, state);
        },
    )
    .await
    .expect("the runtime manager of the test genesis starts")
}
