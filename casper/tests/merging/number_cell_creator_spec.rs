use std::collections::HashMap;

use casper::rust::errors::CasperError;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::hash::keccak256::Keccak256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;

use super::support::{block_hash, dag_with_parents, index_block, merge_blocks, IndexedBlock};
use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::with_runtime_manager;

const MAP: &str = "number-cell-creator-map";
const CONFLICT: &str = "number-cell-creator-conflict";

fn deploy(source: String, sec: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        source,
        Some(5_000_000),
        None,
        Some(sec.clone()),
        None,
        None,
    )
    .unwrap()
}

/// The two TreeHashMap path nybbles of a string key at depth 1: the root's
/// child bit, then the bit in that child node.
fn nybbles(key: &str) -> (u8, u8) {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(key.to_string())),
    }]);
    let hash = Keccak256::hash(par.encode_to_vec());
    (hash[0] % 16, hash[0] / 16)
}

/// Keys whose paths share one interior node: the first creates it, the others
/// only set further bits in it.
fn keys_through_one_node() -> (String, String, String) {
    let k1 = "creator-key-0".to_string();
    let (n0, n1) = nybbles(&k1);
    let sibling = |skip: u8| {
        (1..10_000)
            .map(|i| format!("creator-key-{i}"))
            .find(|k| {
                let (a, b) = nybbles(k);
                a == n0 && b != n1 && b != skip
            })
            .expect("a key through the same node")
    };
    let k2 = sibling(n1);
    let k3 = sibling(nybbles(&k2).1);
    (k1, k2, k3)
}

fn init_map() -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), thmCh, mapCh in {{
  rl!(`rho:lang:treeHashMap`, *thmCh) |
  for (TreeHashMap <- thmCh) {{
    TreeHashMap!("init", 1, *mapCh) |
    for (@map <- mapCh) {{ @"{MAP}"!(map) }}
  }}
}}"#
    )
}

fn insert(key: &str, conflict: bool) -> String {
    let race = if conflict {
        format!(r#" | @"{CONFLICT}"!(0)"#)
    } else {
        String::new()
    };
    format!(
        r#"new rl(`rho:registry:lookup`), thmCh, ret in {{
  rl!(`rho:lang:treeHashMap`, *thmCh) |
  for (TreeHashMap <- thmCh; @map <<- @"{MAP}") {{
    TreeHashMap!("set", map, "{key}", "v", *ret)
  }}{race}
}}"#
    )
}

async fn execute(
    runtime_manager: &RuntimeManager,
    state: &Bytes,
    deploy: Signed<DeployData>,
    number: i64,
    sender: &PublicKey,
) -> Bytes {
    let block_data = BlockData {
        time_stamp: deploy.data.time_stamp,
        seq_num: 1,
        block_number: number,
        sender: sender.clone(),
    };
    let (post_state, processed, _) = runtime_manager
        .compute_state(
            state,
            vec![deploy],
            vec![],
            block_data,
            Some(HashMap::new()),
        )
        .await
        .expect("execution");
    assert!(
        processed.iter().all(|d| !d.is_failed),
        "the deploy must execute"
    );
    post_state
}

/// D's insert creates the interior node N of a map; an earlier merge X
/// rejects D. E, built on D's state, inserts a sibling key through N, so its
/// delta on N was computed on a value the base never held. The merge of E's
/// block onto X must reject E; recovery then runs D again, which creates N
/// once, and both E and a later insert through N run on the result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_to_a_number_cell_whose_creator_an_earlier_merge_rejected_is_rejected() {
    with_runtime_manager(
        |rm, ctx: GenesisContext, genesis_block: BlockMessage| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vault = |i: usize| ctx.genesis_vaults[i % ctx.genesis_vaults.len()].0.clone();
            let validator = |i: usize| {
                ctx.validator_key_pairs[i % ctx.validator_key_pairs.len()]
                    .1
                    .clone()
            };
            let (k1, k2, k3) = keys_through_one_node();
            let root = BlockHash::from(genesis_block.block_hash.to_vec());

            let p = index_block(
                &rm,
                &genesis_state,
                vec![deploy(init_map(), &vault(0))],
                &validator(0),
                block_hash(0xA0),
                1,
            )
            .await;
            let d = deploy(insert(&k1, true), &vault(1));
            let c = deploy(format!(r#"for (_ <- @"{CONFLICT}") {{ Nil }}"#), &vault(2));
            let e = deploy(insert(&k2, false), &vault(3));
            let (d_sig, c_sig, e_sig) = (d.sig.clone(), c.sig.clone(), e.sig.clone());

            let a = index_block(
                &rm,
                &p.post_state,
                vec![d.clone()],
                &validator(1),
                block_hash(0xA1),
                2,
            )
            .await;
            let c_block = index_block(
                &rm,
                &p.post_state,
                vec![c],
                &validator(2),
                block_hash(0xC1),
                2,
            )
            .await;
            let b = index_block(
                &rm,
                &a.post_state,
                vec![e.clone()],
                &validator(3),
                block_hash(0xB1),
                3,
            )
            .await;

            let dag_x = dag_with_parents(&root, &[
                (&p, root.clone()),
                (&a, p.hash.clone()),
                (&c_block, p.hash.clone()),
            ]);
            let x_merge = merge_blocks(
                &rm,
                &dag_x,
                &p.hash,
                &p.post_state,
                &[&a, &c_block],
                Some(c_sig),
            );
            assert!(
                x_merge.rejected.contains(&d_sig),
                "the merge of A and C must reject D"
            );

            let x = IndexedBlock {
                hash: block_hash(0xD0),
                number: 3,
                post_state: x_merge.state,
                chains: vec![],
                seeds: HashMap::new(),
            };
            let dag_y = dag_with_parents(&root, &[
                (&p, root.clone()),
                (&a, p.hash.clone()),
                (&c_block, p.hash.clone()),
                (&x, c_block.hash.clone()),
                (&b, a.hash.clone()),
            ]);
            let y_merge = merge_blocks(&rm, &dag_y, &x.hash, &x.post_state, &[&b], None);
            assert!(
                !y_merge.applied.contains(&e_sig),
                "the merge onto X must not apply E"
            );
            assert!(
                y_merge.rejected.contains(&e_sig),
                "E must be recorded for recovery"
            );

            let recovered = execute(&rm, &y_merge.state, d, 5, &validator(1)).await;
            let e_again = execute(&rm, &recovered, e, 6, &validator(3)).await;
            execute(
                &rm,
                &e_again,
                deploy(insert(&k3, false), &vault(4)),
                7,
                &validator(2),
            )
            .await;
            Ok::<(), CasperError>(())
        },
    )
    .await
    .unwrap()
    .unwrap();
}

/// D creates N and E writes through N in the same block. E reads the value D
/// left on N, a cell the block's pre-state lacks, so D and E form one chain.
/// A merge that rejects D must reject E too: E's delta on N rests on a value
/// only D's creation explains.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_writer_in_the_creators_block_is_rejected_with_the_creator() {
    with_runtime_manager(
        |rm, ctx: GenesisContext, genesis_block: BlockMessage| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vault = |i: usize| ctx.genesis_vaults[i % ctx.genesis_vaults.len()].0.clone();
            let validator = |i: usize| {
                ctx.validator_key_pairs[i % ctx.validator_key_pairs.len()]
                    .1
                    .clone()
            };
            let (k1, k2, k3) = keys_through_one_node();
            let root = BlockHash::from(genesis_block.block_hash.to_vec());

            let p = index_block(
                &rm,
                &genesis_state,
                vec![deploy(init_map(), &vault(0))],
                &validator(0),
                block_hash(0xA0),
                1,
            )
            .await;
            let d = deploy(insert(&k1, true), &vault(1));
            let e = deploy(insert(&k2, false), &vault(3));
            let c = deploy(format!(r#"for (_ <- @"{CONFLICT}") {{ Nil }}"#), &vault(2));
            let (d_sig, e_sig, c_sig) = (d.sig.clone(), e.sig.clone(), c.sig.clone());

            let a = index_block(
                &rm,
                &p.post_state,
                vec![d.clone(), e.clone()],
                &validator(1),
                block_hash(0xA1),
                2,
            )
            .await;
            assert_eq!(a.chains.len(), 1, "D and E must form one chain");
            let c_block = index_block(
                &rm,
                &p.post_state,
                vec![c],
                &validator(2),
                block_hash(0xC1),
                2,
            )
            .await;

            let dag = dag_with_parents(&root, &[
                (&p, root.clone()),
                (&a, p.hash.clone()),
                (&c_block, p.hash.clone()),
            ]);
            let merged = merge_blocks(
                &rm,
                &dag,
                &p.hash,
                &p.post_state,
                &[&a, &c_block],
                Some(c_sig),
            );
            assert!(
                merged.rejected.contains(&d_sig),
                "the merge of A and C must reject D"
            );
            assert!(
                !merged.applied.contains(&e_sig),
                "the merge must not apply E without D"
            );
            assert!(
                merged.rejected.contains(&e_sig),
                "E must be recorded for recovery"
            );

            let recovered = execute(&rm, &merged.state, d, 5, &validator(1)).await;
            let e_again = execute(&rm, &recovered, e, 6, &validator(3)).await;
            execute(
                &rm,
                &e_again,
                deploy(insert(&k3, false), &vault(4)),
                7,
                &validator(2),
            )
            .await;
            Ok::<(), CasperError>(())
        },
    )
    .await
    .unwrap()
    .unwrap();
}

/// D creates N and E, built on D, writes to it; both land in one merge with
/// nothing rejected. E's delta is backed by D's creation, which the merge
/// keeps, even when the two sit in separate branches, so E must be kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_to_a_number_cell_whose_creator_the_merge_keeps_is_kept() {
    with_runtime_manager(
        |rm, ctx: GenesisContext, genesis_block: BlockMessage| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vault = |i: usize| ctx.genesis_vaults[i % ctx.genesis_vaults.len()].0.clone();
            let validator = |i: usize| {
                ctx.validator_key_pairs[i % ctx.validator_key_pairs.len()]
                    .1
                    .clone()
            };
            let (k1, k2, k3) = keys_through_one_node();
            let root = BlockHash::from(genesis_block.block_hash.to_vec());

            let p = index_block(
                &rm,
                &genesis_state,
                vec![deploy(init_map(), &vault(0))],
                &validator(0),
                block_hash(0xA0),
                1,
            )
            .await;
            let d = deploy(insert(&k1, false), &vault(1));
            let e = deploy(insert(&k2, false), &vault(3));
            let (d_sig, e_sig) = (d.sig.clone(), e.sig.clone());

            let a = index_block(
                &rm,
                &p.post_state,
                vec![d],
                &validator(1),
                block_hash(0xA1),
                2,
            )
            .await;
            let b = index_block(
                &rm,
                &a.post_state,
                vec![e],
                &validator(3),
                block_hash(0xB1),
                3,
            )
            .await;

            let dag_y = dag_with_parents(&root, &[
                (&p, root.clone()),
                (&a, p.hash.clone()),
                (&b, a.hash.clone()),
            ]);
            let y_merge = merge_blocks(&rm, &dag_y, &p.hash, &p.post_state, &[&a, &b], None);
            assert!(
                y_merge.rejected.is_empty(),
                "nothing in the merge conflicts"
            );
            assert!(y_merge.applied.contains(&d_sig) && y_merge.applied.contains(&e_sig));

            execute(
                &rm,
                &y_merge.state,
                deploy(insert(&k3, false), &vault(4)),
                5,
                &validator(2),
            )
            .await;
            Ok::<(), CasperError>(())
        },
    )
    .await
    .unwrap()
    .unwrap();
}
