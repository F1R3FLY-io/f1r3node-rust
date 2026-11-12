use std::collections::HashMap;

use casper::rust::rholang::replay_runtime::ReplayRuntimeOps;
use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::util::rholang::resources::mk_runtime_manager;

#[derive(Clone, Copy)]
enum CheckpointOrder {
    FirstThenSecond,
    SecondThenFirst,
}

struct Workload {
    genesis: Signed<DeployData>,
    successor: Signed<DeployData>,
    at_root_a: Signed<DeployData>,
    at_root_b: Signed<DeployData>,
}

impl Workload {
    fn new() -> Self {
        Self {
            genesis: send_on_fresh_channel(1),
            successor: send_on_fresh_channel(2),
            at_root_a: send_on_fresh_channel(3),
            at_root_b: send_on_fresh_channel(4),
        }
    }
}

fn send_on_fresh_channel(ordinal: i64) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        format!("new fresh in {{ fresh!({ordinal}) }}"),
        ordinal,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("the default key signs a send on a fresh channel")
}

fn block_data() -> BlockData {
    BlockData {
        time_stamp: 0,
        block_number: 1,
        sender: construct_deploy::DEFAULT_PUB.clone(),
        seq_num: 1,
    }
}

async fn manager_with_two_roots(workload: &Workload) -> (RuntimeManager, StateHash, StateHash) {
    let manager = mk_runtime_manager("concurrent-runtime-views-test", None).await;
    let (_, root_a, _) = manager
        .compute_genesis(vec![workload.genesis.clone()], 0, 0)
        .await
        .expect("the genesis deploy produces root A");
    let (root_b, _, _) = manager
        .compute_state(
            &root_a,
            vec![workload.successor.clone()],
            Vec::new(),
            block_data(),
            None,
        )
        .await
        .expect("the successor deploy produces root B from root A");
    assert_ne!(root_a, root_b, "CW-01 needs two different roots");
    (manager, root_a, root_b)
}

async fn with_block_context(runtime: RhoRuntimeImpl) -> RhoRuntimeImpl {
    runtime.set_block_data(block_data()).await;
    runtime.set_invalid_blocks(HashMap::new()).await;
    runtime
}

async fn play_runtime(manager: &RuntimeManager) -> RuntimeOps {
    RuntimeOps::new(with_block_context(manager.spawn_runtime().await).await)
}

async fn replay_runtime(manager: &RuntimeManager) -> ReplayRuntimeOps {
    ReplayRuntimeOps::new_from_runtime(
        with_block_context(manager.spawn_replay_runtime().await).await,
    )
}

async fn reset(runtime: &mut RhoRuntimeImpl, root: &StateHash) {
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(root))
        .await
        .expect("a runtime resets to a root its manager has published");
}

async fn play(runtime_ops: &mut RuntimeOps, deploy: &Signed<DeployData>) -> ProcessedDeploy {
    let (recorded, _) = runtime_ops
        .play_deploy_with_cost_accounting(deploy.clone())
        .await
        .expect("a send on a fresh channel plays");
    assert!(!recorded.is_failed, "a send on a fresh channel succeeds");
    assert!(
        !recorded.deploy_log.is_empty(),
        "a play records its send in the deploy log"
    );
    recorded
}

async fn replay(replay_ops: &mut ReplayRuntimeOps, recorded: &ProcessedDeploy) {
    replay_ops
        .replay_deploy_e(true, recorded)
        .await
        .expect("a recorded send replays at the root it was recorded at");
}

async fn checkpoint(runtime: &mut RhoRuntimeImpl) -> StateHash {
    runtime.create_checkpoint().await.root.to_bytes_prost()
}

async fn checkpoint_both(
    first: &mut RhoRuntimeImpl,
    second: &mut RhoRuntimeImpl,
    order: CheckpointOrder,
) -> (StateHash, StateHash) {
    match order {
        CheckpointOrder::FirstThenSecond => {
            let after_first = checkpoint(first).await;
            let after_second = checkpoint(second).await;
            (after_first, after_second)
        }
        CheckpointOrder::SecondThenFirst => {
            let after_second = checkpoint(second).await;
            let after_first = checkpoint(first).await;
            (after_first, after_second)
        }
    }
}

async fn play_alone(
    workload: &Workload,
    base: &StateHash,
    deploy: &Signed<DeployData>,
) -> (StateHash, ProcessedDeploy) {
    let (manager, _, _) = manager_with_two_roots(workload).await;
    let mut runtime_ops = play_runtime(&manager).await;
    reset(&mut runtime_ops.runtime, base).await;
    let recorded = play(&mut runtime_ops, deploy).await;
    (checkpoint(&mut runtime_ops.runtime).await, recorded)
}

async fn replay_alone(
    workload: &Workload,
    base: &StateHash,
    recorded: &ProcessedDeploy,
) -> StateHash {
    let (manager, _, _) = manager_with_two_roots(workload).await;
    let mut replay_ops = replay_runtime(&manager).await;
    reset(&mut replay_ops.runtime_ops.runtime, base).await;
    replay(&mut replay_ops, recorded).await;
    checkpoint(&mut replay_ops.runtime_ops.runtime).await
}

fn assert_each_operation_is_isolated(
    bases: (&StateHash, &StateHash),
    interleaved: (&StateHash, &StateHash),
    alone: (&StateHash, &StateHash),
) {
    assert_ne!(
        interleaved.0, bases.0,
        "CW-01: the operation at root A publishes a successor of root A"
    );
    assert_ne!(
        interleaved.1, bases.1,
        "CW-01: the operation at root B publishes a successor of root B"
    );
    assert_eq!(
        interleaved, alone,
        "CW-01: each interleaved operation returns the root it returns alone on a fresh manager"
    );
}

async fn interleaved_plays_both_checkpoint(order: CheckpointOrder) {
    let workload = Workload::new();
    let (manager, root_a, root_b) = manager_with_two_roots(&workload).await;

    let mut first = play_runtime(&manager).await;
    let mut second = play_runtime(&manager).await;
    reset(&mut first.runtime, &root_a).await;
    reset(&mut second.runtime, &root_b).await;
    play(&mut first, &workload.at_root_a).await;
    play(&mut second, &workload.at_root_b).await;
    let (after_a, after_b) = checkpoint_both(&mut first.runtime, &mut second.runtime, order).await;

    let (alone_after_a, _) = play_alone(&workload, &root_a, &workload.at_root_a).await;
    let (alone_after_b, _) = play_alone(&workload, &root_b, &workload.at_root_b).await;
    assert_each_operation_is_isolated(
        (&root_a, &root_b),
        (&after_a, &after_b),
        (&alone_after_a, &alone_after_b),
    );
}

async fn interleaved_replays_both_checkpoint(order: CheckpointOrder) {
    let workload = Workload::new();
    let (manager, root_a, root_b) = manager_with_two_roots(&workload).await;
    let (played_after_a, recorded_at_a) = play_alone(&workload, &root_a, &workload.at_root_a).await;
    let (played_after_b, recorded_at_b) = play_alone(&workload, &root_b, &workload.at_root_b).await;

    let mut first = replay_runtime(&manager).await;
    let mut second = replay_runtime(&manager).await;
    reset(&mut first.runtime_ops.runtime, &root_a).await;
    reset(&mut second.runtime_ops.runtime, &root_b).await;
    replay(&mut first, &recorded_at_a).await;
    replay(&mut second, &recorded_at_b).await;
    let (after_a, after_b) = checkpoint_both(
        &mut first.runtime_ops.runtime,
        &mut second.runtime_ops.runtime,
        order,
    )
    .await;

    let alone_after_a = replay_alone(&workload, &root_a, &recorded_at_a).await;
    let alone_after_b = replay_alone(&workload, &root_b, &recorded_at_b).await;
    assert_each_operation_is_isolated(
        (&root_a, &root_b),
        (&after_a, &after_b),
        (&alone_after_a, &alone_after_b),
    );
    assert_eq!(
        (&alone_after_a, &alone_after_b),
        (&played_after_a, &played_after_b),
        "a replay alone returns the root of the play that recorded its log"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_play_runtimes_at_different_roots_checkpoint_first_then_second() {
    interleaved_plays_both_checkpoint(CheckpointOrder::FirstThenSecond).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_play_runtimes_at_different_roots_checkpoint_second_then_first() {
    interleaved_plays_both_checkpoint(CheckpointOrder::SecondThenFirst).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_replay_runtimes_at_different_roots_checkpoint_first_then_second() {
    interleaved_replays_both_checkpoint(CheckpointOrder::FirstThenSecond).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_replay_runtimes_at_different_roots_checkpoint_second_then_first() {
    interleaved_replays_both_checkpoint(CheckpointOrder::SecondThenFirst).await;
}
