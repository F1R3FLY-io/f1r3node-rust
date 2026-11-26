#![allow(clippy::mutable_key_type)]
use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;

use super::support::{
    block_hash, channel_values, dag_with_children, dag_with_parents, index_block, merge_blocks,
    IndexedBlock,
};
use crate::util::rholang::resources::with_runtime_manager;

const SHARED_CHANNEL: &str = "issue-873-shared";
const CONFLICT_CHANNEL: &str = "issue-873-conflict";

fn deploy(source: &str, sec: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        source.to_string(),
        None,
        None,
        Some(sec.clone()),
        None,
        None,
    )
    .unwrap()
}

struct MergeOutcome {
    merged_state: Bytes,
    shared_values: Vec<String>,
    rejected: HashSet<Bytes>,
    applied: HashSet<Bytes>,
}

async fn merge_on_genesis(
    runtime_manager: &RuntimeManager,
    genesis: &BlockHash,
    base_state: &Bytes,
    blocks: Vec<IndexedBlock>,
    expensive_sig: Option<Bytes>,
) -> MergeOutcome {
    let dag = dag_with_children(genesis, &blocks.iter().collect::<Vec<_>>());
    merge_in_dag(
        runtime_manager,
        dag,
        genesis,
        base_state,
        blocks,
        expensive_sig,
    )
    .await
}

async fn merge_in_dag(
    runtime_manager: &RuntimeManager,
    dag: KeyValueDagRepresentation,
    genesis: &BlockHash,
    base_state: &Bytes,
    blocks: Vec<IndexedBlock>,
    expensive_sig: Option<Bytes>,
) -> MergeOutcome {
    let merged = merge_blocks(
        runtime_manager,
        &dag,
        genesis,
        base_state,
        &blocks.iter().collect::<Vec<_>>(),
        expensive_sig,
    );
    let shared_values = channel_values(runtime_manager, &merged.state, SHARED_CHANNEL).await;
    MergeOutcome {
        merged_state: merged.state,
        shared_values,
        rejected: merged.rejected,
        applied: merged.applied,
    }
}

fn assert_independent_chains(block: &IndexedBlock, first: &Bytes, second: &Bytes) {
    let chain_of = |sig: &Bytes| {
        block
            .chains
            .iter()
            .position(|chain| {
                chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .any(|d| d.deploy_id == *sig)
            })
            .unwrap()
    };
    assert_ne!(
        chain_of(first),
        chain_of(second),
        "the two deploys must be independent deploy chains of one block"
    );
}

/// Issue #873: block B holds two independent deploys that both send to one
/// non-mergeable channel. D2 also sends to a channel on which a deploy of a
/// sibling block waits, so the merge rejects the chain of D2. The chain of D1
/// carries the block diff of the shared channel, which holds the datum of D2,
/// so the merge must reject it too: the merged state never holds the datum of
/// a rejected deploy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_sibling_chain_does_not_apply_the_datum_of_a_rejected_sibling() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;

            let d1 = deploy(&format!(r#"@"{SHARED_CHANNEL}"!("d1")"#), &vaults[0].0);
            let d2 = deploy(
                &format!(r#"@"{SHARED_CHANNEL}"!("d2") | @"{CONFLICT_CHANNEL}"!(0)"#),
                &vaults[1].0,
            );
            let waiter = deploy(
                &format!(r#"for (_ <- @"{CONFLICT_CHANNEL}") {{ Nil }}"#),
                &vaults[2].0,
            );
            let (d1_sig, d2_sig, waiter_sig) = (d1.sig.clone(), d2.sig.clone(), waiter.sig.clone());

            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![d1, d2],
                &validators[0].1,
                block_hash(0xB1),
                1,
            )
            .await;
            assert_independent_chains(&block_b, &d1_sig, &d2_sig);
            let block_c = index_block(
                &runtime_manager,
                &base_state,
                vec![waiter],
                &validators[1].1,
                block_hash(0xC1),
                1,
            )
            .await;

            let outcome = merge_on_genesis(
                &runtime_manager,
                &genesis,
                &base_state,
                vec![block_b, block_c],
                Some(waiter_sig.clone()),
            )
            .await;

            assert!(
                outcome.rejected.contains(&d2_sig),
                "the merge must reject D2, which races the waiter of block C"
            );
            assert!(
                outcome.applied.contains(&waiter_sig),
                "the merge must keep the waiter of block C"
            );
            assert_eq!(
                outcome.shared_values,
                Vec::<String>::new(),
                "the shared channel must hold no datum of a rejected deploy"
            );
            assert!(
            outcome.rejected.contains(&d1_sig),
            "D1 shares a non-mergeable channel with the rejected D2, so it must be rejected too"
        );
        },
    )
    .await
    .unwrap()
}

/// Issue #873, recovery path: D2 of block B is re-proposed in block B2, so the
/// merge drops the stale chain of D2 in B as a duplicate. The sibling chain of
/// D1 in B still carries the block diff of the shared channel with the stale
/// datum of D2. The merge must reject that sibling, so the shared channel holds
/// only the datum of the fresh copy of D2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_sibling_chain_does_not_apply_the_stale_datum_of_a_deduplicated_sibling() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;

            let d1 = deploy(&format!(r#"@"{SHARED_CHANNEL}"!("d1")"#), &vaults[0].0);
            let d2 = deploy(
                &format!(
                    r#"new bd(`rho:block:data`), bdCh in {{
                     bd!(*bdCh) |
                     for (@number, @_, @_ <- bdCh) {{
                       @"{SHARED_CHANNEL}"!("d2@${{n}}" %% {{"n": number}})
                     }}
                   }}"#
                ),
                &vaults[1].0,
            );
            let (d1_sig, d2_sig) = (d1.sig.clone(), d2.sig.clone());

            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![d1, d2.clone()],
                &validators[0].1,
                block_hash(0xB1),
                1,
            )
            .await;
            assert_independent_chains(&block_b, &d1_sig, &d2_sig);
            let block_b2 = index_block(
                &runtime_manager,
                &base_state,
                vec![d2],
                &validators[1].1,
                block_hash(0xB2),
                2,
            )
            .await;

            let outcome = merge_on_genesis(
                &runtime_manager,
                &genesis,
                &base_state,
                vec![block_b, block_b2],
                None,
            )
            .await;

            assert!(
                outcome.applied.contains(&d2_sig),
                "the merge must apply the fresh copy of D2 from block B2"
            );
            assert_eq!(
                outcome.shared_values,
                vec!["d2@2".to_string()],
                "the shared channel must hold only the datum of the fresh copy of D2"
            );
            assert!(
            outcome.rejected.contains(&d1_sig),
            "D1 shares a non-mergeable channel with the dropped stale D2, so it must be rejected"
        );
        },
    )
    .await
    .unwrap()
}

fn chain_with(block: &IndexedBlock, sig: &Bytes) -> DeployChainIndex {
    block
        .chains
        .iter()
        .find(|chain| {
            chain
                .deploys_with_cost
                .0
                .iter()
                .any(|d| d.deploy_id == *sig)
        })
        .cloned()
        .unwrap()
}

/// Review of #895: the sibling rejection runs inside the retry loop, so the
/// branch walk sees the final survivor set. The base holds `5` on an untagged
/// cell. Block A holds X, which feeds R in block B and K in block C, so X, R
/// and K form one branch. R consumes `5`, K writes `7`, and Z, a sibling of R
/// in B, loses to W of block D. R shares a channel with Z, so the merge rejects
/// R. The merge must not fail, and the cell keeps `5` next to the `7` of K.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sibling_rejection_of_a_cell_consumer_keeps_the_merge_consistent() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;
            let vault = |i: usize| &vaults[i % vaults.len()].0;
            let validator = |i: usize| &validators[i % validators.len()].1;

            let p = deploy(r#"@"issue-873-cell"!(5)"#, vault(0));
            let x = deploy(r#"@"issue-873-a"!(0) | @"issue-873-b"!(0)"#, vault(1));
            let r = deploy(
                &format!(
                    r#"for (_ <- @"issue-873-a"; _ <- @"issue-873-cell") {{ @"{SHARED_CHANNEL}"!("r") }} |
                   new pad in {{ for (_ <- pad; _ <- pad; _ <- pad) {{ Nil }} | pad!(1) | pad!(2) | pad!(3) }}"#
                ),
                vault(2),
            );
            let z = deploy(
                &format!(r#"@"{SHARED_CHANNEL}"!("z") | @"{CONFLICT_CHANNEL}"!(0)"#),
                vault(3),
            );
            let k = deploy(r#"for (_ <- @"issue-873-b") { @"issue-873-cell"!(7) }"#, vault(4));
            let w = deploy(
                &format!(r#"for (_ <- @"{CONFLICT_CHANNEL}") {{ Nil }}"#),
                vault(5),
            );
            let (x_sig, r_sig, z_sig, k_sig, w_sig) = (
                x.sig.clone(),
                r.sig.clone(),
                z.sig.clone(),
                k.sig.clone(),
                w.sig.clone(),
            );

            let block_p =
                index_block(&runtime_manager, &genesis_state, vec![p], validator(0), block_hash(0xA0), 0)
                    .await;
            let block_a =
                index_block(&runtime_manager, &block_p.post_state, vec![x], validator(1), block_hash(0xA1), 1)
                    .await;
            let block_b = index_block(
                &runtime_manager,
                &block_a.post_state,
                vec![r, z],
                validator(2),
                block_hash(0xB1),
                2,
            )
            .await;
            assert_independent_chains(&block_b, &r_sig, &z_sig);
            let block_c =
                index_block(&runtime_manager, &block_a.post_state, vec![k], validator(3), block_hash(0xC1), 2)
                    .await;
            let block_d =
                index_block(&runtime_manager, &block_p.post_state, vec![w], validator(0), block_hash(0xD1), 1)
                    .await;
            assert!(
                chain_with(&block_b, &r_sig) < chain_with(&block_c, &k_sig),
                "R must sort before K, as in the review scenario"
            );

            let root = block_p.hash.clone();
            let base_state = block_p.post_state.clone();
            let dag = dag_with_parents(
                &root,
                &[
                    (&block_a, root.clone()),
                    (&block_b, block_a.hash.clone()),
                    (&block_c, block_a.hash.clone()),
                    (&block_d, root.clone()),
                ],
            );
            let outcome = merge_in_dag(
                &runtime_manager,
                dag,
                &root,
                &base_state,
                vec![block_a, block_b, block_c, block_d],
                Some(w_sig.clone()),
            )
            .await;

            assert!(outcome.rejected.contains(&z_sig), "Z must lose to W");
            assert!(outcome.applied.contains(&w_sig), "the merge must keep W");
            assert!(outcome.applied.contains(&x_sig), "the merge must keep X");
            assert!(
                outcome.rejected.contains(&r_sig),
                "R shares a non-mergeable channel with the rejected Z"
            );
            assert!(
                outcome.applied.contains(&k_sig),
                "K writes an untagged cell, so the merge keeps it without R"
            );
            assert_eq!(
                channel_values(&runtime_manager, &outcome.merged_state, "issue-873-cell").await,
                vec!["5".to_string(), "7".to_string()],
                "the cell must keep the base value that the rejected R consumed, next to the 7 of K"
            );
        },
    )
    .await
    .unwrap()
}

/// The lineage expansion runs inside the retry loop, so conflict resolution
/// runs again without the chains it rejects. Q in block B and S in block E
/// both consume the base's one datum on a race channel, and Q costs more, so
/// S loses to Q. B descends from block A2, whose Z loses to W, so the
/// expansion rejects Q. S lost only to Q, so the merge must keep it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_chain_that_lost_only_to_a_lineage_rejected_chain_is_kept() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;
            let vault = |i: usize| &vaults[i % vaults.len()].0;
            let validator = |i: usize| &validators[i % validators.len()].1;
            const RACE: &str = "issue-873-race";
            const WON: &str = "issue-873-won";

            let p = deploy(&format!(r#"@"{RACE}"!(0)"#), vault(0));
            let z = deploy(&format!(r#"@"{CONFLICT_CHANNEL}"!(0)"#), vault(3));
            let q = deploy(
                &format!(
                    r#"for (_ <- @"{RACE}") {{ Nil }} |
                   new pad in {{ for (_ <- pad; _ <- pad; _ <- pad) {{ Nil }} | pad!(1) | pad!(2) | pad!(3) }}"#
                ),
                vault(2),
            );
            let s = deploy(&format!(r#"for (_ <- @"{RACE}") {{ @"{WON}"!(0) }}"#), vault(4));
            let w = deploy(
                &format!(r#"for (_ <- @"{CONFLICT_CHANNEL}") {{ Nil }}"#),
                vault(5),
            );
            let (q_sig, s_sig, z_sig, w_sig) =
                (q.sig.clone(), s.sig.clone(), z.sig.clone(), w.sig.clone());

            let block_p = index_block(&runtime_manager, &genesis_state, vec![p], validator(0), block_hash(0xA0), 0).await;
            let block_a2 = index_block(&runtime_manager, &block_p.post_state, vec![z], validator(2), block_hash(0xA2), 1).await;
            let block_b = index_block(&runtime_manager, &block_a2.post_state, vec![q], validator(2), block_hash(0xB1), 2).await;
            let block_e = index_block(&runtime_manager, &block_p.post_state, vec![s], validator(3), block_hash(0xE1), 1).await;
            let block_d = index_block(&runtime_manager, &block_p.post_state, vec![w], validator(0), block_hash(0xD1), 1).await;
            let cost = |chain: DeployChainIndex| -> u64 {
                chain.deploys_with_cost.0.iter().map(|d| d.cost).sum()
            };
            assert!(
                cost(chain_with(&block_b, &q_sig)) > cost(chain_with(&block_e, &s_sig)),
                "Q must cost more than S, so S loses the race to Q"
            );

            let root = block_p.hash.clone();
            let base_state = block_p.post_state.clone();
            let dag = dag_with_parents(
                &root,
                &[
                    (&block_a2, root.clone()),
                    (&block_b, block_a2.hash.clone()),
                    (&block_e, root.clone()),
                    (&block_d, root.clone()),
                ],
            );
            let outcome = merge_in_dag(
                &runtime_manager,
                dag,
                &root,
                &base_state,
                vec![block_a2, block_b, block_e, block_d],
                Some(w_sig.clone()),
            )
            .await;

            assert!(outcome.rejected.contains(&z_sig), "Z must lose to W");
            assert!(outcome.applied.contains(&w_sig), "the merge must keep W");
            assert!(
                outcome.rejected.contains(&q_sig),
                "block B descends from the rejected block A2"
            );
            assert!(
                outcome.applied.contains(&s_sig),
                "S lost only to Q, which the lineage expansion rejected, so the merge must keep S"
            );
            assert_eq!(
                channel_values(&runtime_manager, &outcome.merged_state, WON).await,
                vec!["0".to_string()],
                "the kept S must take the race datum"
            );
        },
    )
    .await
    .unwrap()
}

/// A seed twin of `y`, signed by the same key at the same timestamp, whose
/// signature sorts before `y`'s, so the merge keeps it and drops `y`.
fn seed_twin_sorting_first(y: &Signed<DeployData>, key: &PrivateKey) -> Signed<DeployData> {
    (0..256)
        .map(|i| {
            construct_deploy::source_deploy(
                r#"@"issue-873-twin"!(0)"#.to_string(),
                y.data.time_stamp,
                Some(y.data.phlo_limit + 1 + i),
                None,
                Some(key.clone()),
                Some(y.data.valid_after_block_number),
                None,
            )
            .unwrap()
        })
        .find(|x| x.sig < y.sig)
        .expect("a seed twin whose signature sorts first")
}

fn send_at(term: &str, timestamp: i64, key: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        term.to_string(),
        timestamp,
        Some(1_000_000),
        None,
        Some(key.clone()),
        None,
        None,
    )
    .unwrap()
}

/// The merge keeps one deploy per name seed and drops a chain carrying another
/// (`NameSeeds::split_reused`). Block B holds Y, a seed twin of X in block A,
/// and S, an independent send to the channel Y sends to. The merge keeps X and
/// drops Y, so S, which carries B's diff of that channel with Y's datum, must
/// go too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_sibling_chain_does_not_apply_the_datum_of_a_dropped_seed_twin() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;

            let y = send_at(&format!(r#"@"{SHARED_CHANNEL}"!("y")"#), 7, &vaults[1].0);
            let x = seed_twin_sorting_first(&y, &vaults[1].0);
            let s = deploy(&format!(r#"@"{SHARED_CHANNEL}"!("s")"#), &vaults[0].0);
            let (x_sig, y_sig, s_sig) = (x.sig.clone(), y.sig.clone(), s.sig.clone());

            let block_a = index_block(
                &runtime_manager,
                &base_state,
                vec![x],
                &validators[0].1,
                block_hash(0xA1),
                1,
            )
            .await;
            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![s, y],
                &validators[1].1,
                block_hash(0xB1),
                1,
            )
            .await;
            assert_independent_chains(&block_b, &s_sig, &y_sig);

            let outcome = merge_on_genesis(
                &runtime_manager,
                &genesis,
                &base_state,
                vec![block_a, block_b],
                None,
            )
            .await;

            assert!(
                outcome.applied.contains(&x_sig),
                "the merge keeps the seed twin X"
            );
            assert!(
                !outcome.applied.contains(&y_sig),
                "the merge drops Y, which reuses X's seed"
            );
            assert_eq!(
                outcome.shared_values,
                Vec::<String>::new(),
                "the shared channel must hold no datum of the dropped seed twin"
            );
            assert!(
                outcome.rejected.contains(&s_sig),
                "S shares a non-mergeable channel with the dropped Y, so it must be rejected"
            );
        },
    )
    .await
    .unwrap()
}

/// The lineage form of the case above: block B holds only Y, a seed twin of X
/// in block A, and block B2, built on B, sends to a channel Y does not touch.
/// The merge drops Y, so B2's chain, computed on a state holding Y's effect,
/// must go too, as for any chain descending from removed work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_descendant_of_a_dropped_seed_twin_does_not_apply_its_stale_diff() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;
            const OUT: &str = "issue-873-seed-out";

            let y = send_at(&format!(r#"@"{SHARED_CHANNEL}"!("y")"#), 7, &vaults[1].0);
            let x = seed_twin_sorting_first(&y, &vaults[1].0);
            let t = deploy(&format!(r#"@"{OUT}"!("t")"#), &vaults[0].0);
            let (x_sig, y_sig, t_sig) = (x.sig.clone(), y.sig.clone(), t.sig.clone());

            let block_a = index_block(
                &runtime_manager,
                &base_state,
                vec![x],
                &validators[0].1,
                block_hash(0xA1),
                1,
            )
            .await;
            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![y],
                &validators[1].1,
                block_hash(0xB1),
                1,
            )
            .await;
            let block_b2 = index_block(
                &runtime_manager,
                &block_b.post_state,
                vec![t],
                &validators[2].1,
                block_hash(0xB2),
                2,
            )
            .await;
            let dag = dag_with_parents(&genesis, &[
                (&block_a, genesis.clone()),
                (&block_b, genesis.clone()),
                (&block_b2, block_b.hash.clone()),
            ]);

            let outcome = merge_in_dag(
                &runtime_manager,
                dag,
                &genesis,
                &base_state,
                vec![block_a, block_b, block_b2],
                None,
            )
            .await;

            assert!(
                outcome.applied.contains(&x_sig),
                "the merge keeps the seed twin X"
            );
            assert!(
                !outcome.applied.contains(&y_sig),
                "the merge drops Y, which reuses X's seed"
            );
            assert_eq!(
                channel_values(&runtime_manager, &outcome.merged_state, OUT).await,
                Vec::<String>::new(),
                "the merged state must hold nothing from a descendant of the dropped seed twin"
            );
            assert!(
                outcome.rejected.contains(&t_sig),
                "block B2 descends from block B, whose Y the merge dropped"
            );
        },
    )
    .await
    .unwrap()
}
