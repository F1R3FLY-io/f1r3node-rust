// Property tests of the checkpoint model of Reified RSpaces (P10 step S7, plan
// decision 85). Each property is the executable form of headlines of
// formal/rocq/reified_rspaces/theories/Checkpoint.v on the host RSpace:
// C1 and C8 (a reset at any root of the history gives back the store that the
// checkpoint wrote), C2 (a persistent datum keeps its flag across the
// checkpoint and the reset, and a later COMM keeps it), C3 and C4 (two hot
// stores whose record lists are permutations of each other checkpoint to one
// root, also after the same operations), and C7 (the revert of a soft
// checkpoint restores the store of the snapshot, whatever prefix of a deploy
// ran), C5 (a replay rigged with the log of play reaches the root of play, and
// a shorter replay does not) and C6 (a reset after the volatile sweep holds no
// record of a swept channel). The mapping lives in formal/
// reified-rspaces-invariant-map.json (phase P10). Runs are deterministic (fixed
// seed) and never write to the source tree (no failure persistence).

#[path = "reified_space_store_handle/support.rs"]
mod support;

use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
use support::*;

const SEED: u64 = 0x0E0_C4EC_2026_1012;
const CHANNELS: [&str; 3] = ["a", "b", "c"];

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a test runtime")
}

/// One generated step. A produce stores a fresh value, so every datum of a run
/// is distinct. A consume names a value that an earlier or a later produce may
/// store, or any value.
#[derive(Clone, Debug)]
enum Step {
    Produce {
        channel: usize,
        persist: bool,
    },
    Consume {
        channels: Vec<usize>,
        exact: Option<usize>,
        persist: bool,
    },
}

fn step(exact_only: bool) -> impl Strategy<Value = Step> {
    let produce = (0..CHANNELS.len(), any::<bool>())
        .prop_map(|(channel, persist)| Step::Produce { channel, persist });
    let consume = (
        prop_oneof![
            (0..CHANNELS.len()).prop_map(|channel| vec![channel]),
            (0..CHANNELS.len(), 0..CHANNELS.len())
                .prop_filter("two distinct channels", |(first, second)| first < second)
                .prop_map(|(first, second)| vec![first, second]),
        ],
        (0usize..12, prop::bool::weighted(0.7)),
        any::<bool>(),
    )
        .prop_map(move |(channels, (value, named), persist)| Step::Consume {
            channels,
            exact: (exact_only || named).then_some(value),
            persist,
        });
    prop_oneof![3 => produce, 2 => consume]
}

/// The host operations of `steps` on the channels `CHANNELS`. The n-th produce
/// stores the value `v<n + offset>`.
fn ops(steps: &[Step], offset: usize) -> Vec<Op> { ops_on(steps, offset, &CHANNELS) }

/// The host operations of `steps` on the channels `names`.
fn ops_on(steps: &[Step], offset: usize, names_of: &[&str; 3]) -> Vec<Op> {
    let mut produced = offset;
    let mut result = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Produce { channel, persist } => {
                result.push(Op::produce(names_of[*channel], &format!("v{produced}"), *persist));
                produced += 1;
            }
            Step::Consume {
                channels,
                exact,
                persist,
            } => {
                let names: Vec<&str> = channels.iter().map(|channel| names_of[*channel]).collect();
                let patterns = names
                    .iter()
                    .map(|_| match exact {
                        Some(value) => Pat::Exact(format!("v{}", value + offset)),
                        None => Pat::Any,
                    })
                    .collect();
                result.push(Op::consume(&names, patterns, &format!("k{index}"), *persist));
            }
        }
    }
    result
}

async fn run_all(space: &Space, ops: &[Op]) {
    for op in ops {
        run_host(space, op).await.expect("run a host operation");
    }
}

/// The view of every channel and group, with each record list sorted: a reset
/// reads the records of a key in the order of their encodings.
fn sorted(view: ChannelView) -> ChannelView {
    let (mut data, mut continuations, mut joins) = view;
    data.iter_mut().for_each(|records| records.sort());
    continuations.iter_mut().for_each(|records| records.sort());
    joins.iter_mut().for_each(|groups| groups.sort());
    (data, continuations, joins)
}

async fn view(space: &Space, groups: &[Vec<String>]) -> ChannelView {
    channel_view(space, &universe(), groups).await
}

/// C1 and C8: a chain of blocks, each one checkpointed. A reset at the root of
/// each block, the last one first, reads back the store of that block.
async fn reset_restores_every_root(blocks: Vec<Vec<Step>>) -> Result<(), TestCaseError> {
    let space = play_space().await;
    let mut recorded = Vec::with_capacity(blocks.len());
    let mut offset = 0;
    for block in &blocks {
        run_all(&space, &ops(block, offset)).await;
        offset += block.len();
        let groups = groups_of(&[&space]).await;
        let before = sorted(view(&space, &groups).await);
        let root = checkpoint(&space).await.root;
        recorded.push((root, groups, before));
    }
    for (root, groups, before) in recorded.iter().rev() {
        space.reset(root).await.expect("reset to a recorded root");
        let after = sorted(view(&space, groups).await);
        prop_assert_eq!(&after, before);
    }
    Ok(())
}

/// C2: after a checkpoint and a reset, every persistent datum is still
/// persistent, and two matching COMMs keep it.
async fn persistent_data_survive(produces: Vec<(usize, bool)>) -> Result<(), TestCaseError> {
    let space = play_space().await;
    for (index, (channel, persist)) in produces.iter().enumerate() {
        run_host(&space, &Op::produce(CHANNELS[*channel], &format!("v{index}"), *persist))
            .await
            .expect("produce");
    }
    let root = checkpoint(&space).await.root;
    space.reset(&root).await.expect("reset to the root");
    for (index, (channel, persist)) in produces.iter().enumerate() {
        let value = format!("v{index}");
        let stored: Vec<bool> = space
            .get_data(&s(CHANNELS[*channel]))
            .await
            .into_iter()
            .filter(|datum| datum.a == value)
            .map(|datum| datum.persist)
            .collect();
        prop_assert_eq!(stored, vec![*persist], "the datum {} after the reset", value);
    }
    for (index, (channel, persist)) in produces.iter().enumerate() {
        if !persist {
            continue;
        }
        let value = format!("v{index}");
        for round in 0..2 {
            let op = Op::consume(
                &[CHANNELS[*channel]],
                vec![Pat::Exact(value.clone())],
                &format!("take{index}.{round}"),
                false,
            );
            run_host(&space, &op)
                .await
                .expect("consume a persistent datum");
        }
        let kept = space
            .get_data(&s(CHANNELS[*channel]))
            .await
            .into_iter()
            .any(|datum| datum.a == value && datum.persist);
        prop_assert!(kept, "the COMMs removed the persistent datum {}", value);
    }
    Ok(())
}

/// C3 and C4: two spaces store one set of data in two orders. Their checkpoints
/// have one root (C4). Two other spaces store the data in the two orders and
/// then run the same operations before their checkpoint, and their checkpoints
/// have one root too (C3).
async fn permuted_parents_share_roots(
    parent: Vec<(usize, bool)>,
    order: Vec<usize>,
    steps: Vec<Step>,
) -> Result<(), TestCaseError> {
    let produce = |index: usize| {
        let (channel, persist) = parent[index];
        Op::produce(CHANNELS[channel], &format!("p{index}"), persist)
    };
    let in_order: Vec<Op> = (0..parent.len()).map(produce).collect();
    let permuted: Vec<Op> = order.iter().map(|index| produce(*index)).collect();
    let operations = ops(&steps, 0);

    let first = play_space().await;
    let second = play_space().await;
    run_all(&first, &in_order).await;
    run_all(&second, &permuted).await;
    let first_root = checkpoint(&first).await.root;
    let second_root = checkpoint(&second).await.root;
    prop_assert_eq!(first_root, second_root, "the parents checkpoint to one root");

    let third = play_space().await;
    let fourth = play_space().await;
    run_all(&third, &in_order).await;
    run_all(&fourth, &permuted).await;
    run_all(&third, &operations).await;
    run_all(&fourth, &operations).await;
    let third_root = checkpoint(&third).await.root;
    let fourth_root = checkpoint(&fourth).await.root;
    prop_assert_eq!(third_root, fourth_root, "the same operations keep one root");
    Ok(())
}

/// C5: a replay rigged with the log of play and reset to the parent root
/// reaches the root of play. A replay that leaves out the last operation of
/// play either misses a COMM of the log, so its checkpoint fails, or reaches
/// another root.
async fn rig_accepts_the_log_of_play(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let operations = ops(&steps, 0);
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    run_all(&play, &operations).await;
    let played = checkpoint(&play).await;
    rig(&replay, &start.root, &played.log).await;
    for op in &operations {
        run_host(&replay, op)
            .await
            .expect("replay a host operation");
    }
    match replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(&replayed.root, &played.root),
        Err(error) => return Err(TestCaseError::fail(format!("replay checkpoint: {error:?}"))),
    }
    if let Some((_, prefix)) = operations.split_last() {
        rig(&replay, &start.root, &played.log).await;
        for op in prefix {
            run_host(&replay, op)
                .await
                .expect("replay a host operation");
        }
        if let Ok(replayed) = replay.create_checkpoint().await {
            prop_assert_ne!(
                replayed.root,
                played.root,
                "a shorter replay reached the root of play"
            );
        }
    }
    Ok(())
}

const SWEPT: [&str; 3] = ["a", "v1", "v2"];

/// `steps` without the joins that mix the durable channel `a` with a volatile
/// channel. The sweep refuses such a group (`SweepCrossesDurable`), because in
/// a node a volatile channel belongs to a temp space, and a join stays in one
/// space.
fn unmixed(steps: Vec<Step>) -> Vec<Step> {
    steps
        .into_iter()
        .filter(|step| match step {
            Step::Consume { channels, .. } => {
                channels.iter().all(|channel| *channel == 0) ||
                    channels.iter().all(|channel| *channel != 0)
            }
            Step::Produce { .. } => true,
        })
        .collect()
}

/// C6: after the volatile sweep of `v1` and `v2` and a checkpoint, a reset
/// holds no data and no joins of the swept channels, and no continuation of a
/// group that held one of them.
async fn sweep_then_reset_holds_no_volatile_record(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let space = play_space().await;
    run_all(&space, &ops_on(&unmixed(steps), 0, &SWEPT)).await;
    let volatile = names(&["v1", "v2"]);
    let mut swept_groups: Vec<Vec<String>> = volatile.iter().map(|c| vec![c.clone()]).collect();
    for channel in &volatile {
        swept_groups.extend(space.get_joins(channel.clone()).await);
    }
    let handle = SpaceStoreHandle::new(space.clone());
    handle
        .sweep_volatile(&volatile)
        .await
        .expect("sweep the volatile channels");
    let root = checkpoint(&space).await.root;
    space.reset(&root).await.expect("reset to the root");
    for channel in &volatile {
        prop_assert!(
            space.get_data(channel).await.is_empty(),
            "data on {} after the reset",
            channel
        );
        prop_assert!(
            space.get_joins(channel.clone()).await.is_empty(),
            "joins of {} after the reset",
            channel
        );
    }
    for group in &swept_groups {
        prop_assert!(
            space
                .get_waiting_continuations(group.clone())
                .await
                .is_empty(),
            "continuations of {:?} after the reset",
            group
        );
    }
    Ok(())
}

/// C7: a soft checkpoint, then any prefix of a failed deploy, then the revert.
/// The store reads as before the deploy, and its checkpoint equals the
/// checkpoint of a space that never ran the deploy.
async fn soft_revert_restores_the_parent(
    parent: Vec<Step>,
    failed: Vec<Step>,
) -> Result<(), TestCaseError> {
    let space = play_space().await;
    let witness = play_space().await;
    let parent_ops = ops(&parent, 0);
    run_all(&space, &parent_ops).await;
    run_all(&witness, &parent_ops).await;
    let mut groups = groups_of(&[&space]).await;
    let soft = space.create_soft_checkpoint().await;
    run_all(&space, &ops(&failed, parent.len())).await;
    groups.extend(groups_of(&[&space]).await);
    groups.sort();
    groups.dedup();
    space
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("revert the soft checkpoint");
    let reverted = view(&space, &groups).await;
    let expected = view(&witness, &groups).await;
    prop_assert_eq!(&reverted, &expected, "the revert restores the parent store");
    let root = checkpoint(&space).await.root;
    let witness_root = checkpoint(&witness).await.root;
    prop_assert_eq!(root, witness_root, "the revert leaves the checkpoint of the parent");
    Ok(())
}

fn parent_and_order() -> impl Strategy<Value = (Vec<(usize, bool)>, Vec<usize>)> {
    prop::collection::vec((0..CHANNELS.len(), any::<bool>()), 1..8).prop_flat_map(|parent| {
        let indices: Vec<usize> = (0..parent.len()).collect();
        (Just(parent), Just(indices).prop_shuffle())
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn prop_checkpoint_reset_restores_every_root(
        blocks in prop::collection::vec(prop::collection::vec(step(false), 0..8), 1..4)
    ) {
        runtime().block_on(reset_restores_every_root(blocks))?;
    }

    #[test]
    fn prop_checkpoint_keeps_persistent_data(
        produces in prop::collection::vec((0..CHANNELS.len(), any::<bool>()), 1..8)
    ) {
        runtime().block_on(persistent_data_survive(produces))?;
    }

    #[test]
    fn prop_permuted_parents_share_roots(
        (parent, order) in parent_and_order(),
        steps in prop::collection::vec(step(true), 0..8)
    ) {
        runtime().block_on(permuted_parents_share_roots(parent, order, steps))?;
    }

    #[test]
    fn prop_soft_revert_restores_the_parent(
        parent in prop::collection::vec(step(false), 0..8),
        failed in prop::collection::vec(step(false), 0..8)
    ) {
        runtime().block_on(soft_revert_restores_the_parent(parent, failed))?;
    }

    #[test]
    fn prop_rig_accepts_the_log_of_play(steps in prop::collection::vec(step(false), 0..10)) {
        runtime().block_on(rig_accepts_the_log_of_play(steps))?;
    }

    #[test]
    fn prop_sweep_then_reset_holds_no_volatile_record(
        steps in prop::collection::vec(step(false), 0..10)
    ) {
        runtime().block_on(sweep_then_reset_holds_no_volatile_record(steps))?;
    }
}
