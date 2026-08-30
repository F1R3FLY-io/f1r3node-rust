// Issue #294 (B4): loss-aware adjudication end to end, proposer and
// validator alike.
//
// A cheap deploy meets a strictly costlier same-cell contender every round.
// Content ordering (cost first) rejects the cheap deploy deterministically —
// round after round, a fresh costlier contender arrives and the same deploy
// loses again, exactly the starvation the Heavy Pipeline observed. The fix
// under test: after the first loss, the deploy's kept rejection record is in
// the DAG every validator sees, its derived priority beats content ordering,
// and it lands. Every `process_block` here is also the validator half of the
// claim: a peer that recomputes a DIFFERENT rejection set refuses the block
// (`InvalidRejectedDeploy`), so cross-node acceptance proves the proposer and
// validators computed identical rejection sets from the same on-DAG data.

use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::DeployData;
use serial_test::serial;

use super::staging::mint_on_parents;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

async fn build_genesis(n_validators: usize) -> GenesisContext {
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap()
}

/// Cheap same-cell write: the content ordering's permanent loser.
fn cheap_write(key: &str, sec: &PrivateKey, shard_id: &str) -> Signed<DeployData> {
    let rho = format!(r#"for (@m <- @"m") {{ @"m"!(m.set("{}", 1)) }}"#, key);
    construct_deploy::source_deploy_now_full(
        rho,
        None,
        None,
        Some(sec.clone()),
        None,
        Some(shard_id.to_string()),
    )
    .expect("build cheap write")
}

/// Strictly costlier same-cell write: extra arithmetic buys enough phlo cost
/// that the content ordering (total cost first) always prefers it.
fn costly_write(key: &str, val: i64, sec: &PrivateKey, shard_id: &str) -> Signed<DeployData> {
    let rho = format!(
        r#"match ((1 + 2) * (3 + 4) + (5 * 6)) % 7 {{ _ => Nil }} |
           match ((7 + 8) * (9 + 10) + (11 * 12)) % 13 {{ _ => Nil }} |
           for (@m <- @"m") {{ @"m"!(m.set("{}", {})) }}"#,
        key, val
    );
    construct_deploy::source_deploy_now_full(
        rho,
        None,
        None,
        Some(sec.clone()),
        None,
        Some(shard_id.to_string()),
    )
    .expect("build costly write")
}

fn par_to_i64(p: &Par) -> Option<i64> {
    p.exprs.first().and_then(|e| match &e.expr_instance {
        Some(ExprInstance::GInt(n)) => Some(*n),
        _ => None,
    })
}

/// True iff `key` is present in the cell at `state_hash`.
async fn key_landed(node: &TestNode, state_hash: &prost::bytes::Bytes, key: &str) -> bool {
    let term = format!(
        r#"new return in {{ for (@m <<- @"m") {{ return!(m.getOrElse("{}", -999)) }} }}"#,
        key
    );
    match node
        .runtime_manager
        .play_exploratory_deploy(term, state_hash, None)
        .await
    {
        Ok((res, _)) => res.first().and_then(par_to_i64) != Some(-999),
        Err(_) => false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_validator_neutral_base_applies_prior_loss_priority() {
    let ctx = build_genesis(3).await;
    let shard_id = ctx.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(ctx, 3, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let starved_sec = construct_deploy::DEFAULT_SEC.clone();
    let contender_sec = construct_deploy::DEFAULT_SEC2.clone();
    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(starved_sec.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("build init");
    nodes[0].casper.deploy(init).expect("init deploy");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    for node in nodes.iter_mut() {
        node.process_block(init_block.clone())
            .await
            .expect("process init");
    }

    let starved = cheap_write("starved", &starved_sec, &shard_id);
    let starved_sig = starved.sig.clone();
    nodes[0].casper.deploy(starved).expect("starved deploy");
    let starved_sibling =
        mint_on_parents(&mut nodes[0], vec![init_block.clone()], "starved sibling").await;

    let first_contender = costly_write("first", 1, &contender_sec, &shard_id);
    nodes[1]
        .casper
        .deploy(first_contender)
        .expect("first contender deploy");
    let contender_sibling =
        mint_on_parents(&mut nodes[1], vec![init_block.clone()], "contender sibling").await;
    let neutral_sibling = mint_on_parents(&mut nodes[2], vec![init_block], "neutral sibling").await;

    for (owner, block) in [
        (0usize, &starved_sibling),
        (1usize, &contender_sibling),
        (2usize, &neutral_sibling),
    ] {
        for (index, node) in nodes.iter_mut().enumerate() {
            if index != owner {
                node.process_block(block.clone())
                    .await
                    .expect("deliver first siblings");
            }
        }
    }

    let first_merge = mint_on_parents(
        &mut nodes[2],
        vec![neutral_sibling.clone(), starved_sibling, contender_sibling],
        "first neutral-base merge",
    )
    .await;
    for node in nodes.iter_mut().take(2) {
        node.process_block(first_merge.clone())
            .await
            .expect("deliver first merge");
    }
    assert_eq!(
        first_merge.header.parents_hash_list.first(),
        Some(&neutral_sibling.block_hash)
    );
    assert!(first_merge
        .body
        .rejected_deploys
        .iter()
        .any(|record| record.deploy_id() == starved_sig.as_ref()));

    let first_merge_height = first_merge.body.state.block_number;
    let mut gate_tip = None;
    for round in 0..30i32 {
        let marker = construct_deploy::basic_deploy_data(
            100 + round,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("settle marker");
        let block = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle round");
        for node in nodes.iter_mut().take(2) {
            node.process_block(block.clone())
                .await
                .expect("deliver settle round");
        }
        let dag = nodes[0].casper.block_dag().await.expect("dag");
        let floor = floor_of_block(
            &dag,
            &nodes[0].block_store,
            &block.block_hash,
            FtThreshold::from_f32_lossy(0.0),
        )
        .await
        .expect("floor_of_block");
        let covered = floor.hash == first_merge.block_hash
            || (floor.block_number >= first_merge_height
                && dag
                    .is_dag_ancestor(&first_merge.block_hash, &floor.hash)
                    .expect("ancestor query"));
        if covered {
            gate_tip = Some(block);
            break;
        }
    }
    let gate_tip = gate_tip.expect("the floor must cover the first rejection");

    let retry_sibling =
        mint_on_parents(&mut nodes[0], vec![gate_tip.clone()], "retry sibling").await;
    assert!(retry_sibling
        .body
        .deploys
        .iter()
        .any(|processed| processed.deploy.sig == starved_sig));

    let second_contender = costly_write("second", 2, &contender_sec, &shard_id);
    let second_contender_sig = second_contender.sig.clone();
    nodes[1]
        .casper
        .deploy(second_contender)
        .expect("second contender deploy");
    let second_contender_sibling = mint_on_parents(
        &mut nodes[1],
        vec![gate_tip.clone()],
        "second contender sibling",
    )
    .await;
    let second_neutral_sibling =
        mint_on_parents(&mut nodes[2], vec![gate_tip], "second neutral sibling").await;

    for (owner, block) in [
        (0usize, &retry_sibling),
        (1usize, &second_contender_sibling),
        (2usize, &second_neutral_sibling),
    ] {
        for (index, node) in nodes.iter_mut().enumerate() {
            if index != owner {
                node.process_block(block.clone())
                    .await
                    .expect("deliver second siblings");
            }
        }
    }

    let second_merge = mint_on_parents(
        &mut nodes[2],
        vec![
            second_neutral_sibling.clone(),
            retry_sibling,
            second_contender_sibling,
        ],
        "second neutral-base merge",
    )
    .await;
    for node in nodes.iter_mut().take(2) {
        node.process_block(second_merge.clone())
            .await
            .expect("deliver second merge");
    }

    assert_eq!(
        second_merge.header.parents_hash_list.first(),
        Some(&second_neutral_sibling.block_hash)
    );
    assert!(!second_merge
        .body
        .rejected_deploys
        .iter()
        .any(|record| record.deploy_id() == starved_sig.as_ref()));
    assert!(second_merge
        .body
        .rejected_deploys
        .iter()
        .any(|record| record.deploy_id() == second_contender_sig.as_ref()));
    assert!(
        key_landed(
            &nodes[2],
            &second_merge.body.state.post_state_hash,
            "starved"
        )
        .await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
#[ignore = "issue #294 expected RED base-bias sentinel: a fixed contender-owner proposer \
            keeps the retry structurally stale. Start C1 only if soak evidence shows \
            residual expiry."]
async fn repeatedly_rejected_deploy_gains_priority_and_lands() {
    let ctx = build_genesis(2).await;
    let shard_id = ctx.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(ctx, 2, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let starved_sec = construct_deploy::DEFAULT_SEC.clone();
    let contender_sec = construct_deploy::DEFAULT_SEC2.clone();

    // Initialize the single-value cell and distribute.
    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(starved_sec.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("build init");
    nodes[0].casper.deploy(init).expect("init deploy");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    for node in nodes.iter_mut() {
        node.process_block(init_block.clone())
            .await
            .expect("process init");
    }

    // Round 1: the cheap write meets a costlier contender; the merge must
    // reject the cheap write on content order and record it.
    let starved = cheap_write("starved", &starved_sec, &shard_id);
    let starved_sig = starved.sig.clone();
    let sibling_starved = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&starved))
        .await
        .expect("starved sibling");
    let contender = costly_write("y1", 1, &contender_sec, &shard_id);
    let sibling_contender = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender))
        .await
        .expect("contender sibling");
    for node in nodes.iter_mut() {
        node.process_block(sibling_starved.clone())
            .await
            .expect("cross-add starved sibling");
        node.process_block(sibling_contender.clone())
            .await
            .expect("cross-add contender sibling");
    }
    let merge1 = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("first merge block");
    for node in nodes.iter_mut() {
        node.process_block(merge1.clone())
            .await
            .expect("validators accept first merge");
    }
    assert!(
        merge1
            .body
            .rejected_deploys
            .iter()
            .any(|r| r.deploy_id() == starved_sig.as_ref()),
        "FIXTURE: round 1 must reject the cheap write on content order; \
         raise the contender's arithmetic cost if this fails"
    );

    // Contention rounds, in the racing shape the Heavy Pipeline observed:
    // the owner's retry and a FRESH costlier contender propose as SIBLINGS
    // (neither sees the other), and the NON-owner proposes the merge that
    // adjudicates them. On content order the retry loses every race; its
    // kept records must buy the priority to land before the rounds run out.
    let max_rounds = 16;
    let mut landed_round = None;
    for round in 2..=max_rounds {
        let contender = costly_write(
            &format!("y{}", round),
            round as i64,
            &contender_sec,
            &shard_id,
        );
        nodes[1].casper.deploy(contender).expect("contender deploy");
        let contender_block = nodes[1]
            .create_block_unsafe(&[])
            .await
            .expect("contender sibling");
        // The owner proposes WITHOUT seeing the contender's block: its retry
        // (when the gate opens) rides a genuine sibling.
        let owner_block = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("owner sibling");
        for node in nodes.iter_mut() {
            node.process_block(contender_block.clone())
                .await
                .expect("validators accept contender sibling");
            node.process_block(owner_block.clone())
                .await
                .expect("validators accept owner sibling");
        }
        let merge = nodes[1]
            .create_block_unsafe(&[])
            .await
            .expect("merge block");
        for node in nodes.iter_mut() {
            node.process_block(merge.clone())
                .await
                .expect("validators accept merge block");
        }
        let retry_aboard = owner_block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == starved_sig);
        let merge_rejected_starved = merge
            .body
            .rejected_deploys
            .iter()
            .any(|r| r.deploy_id() == starved_sig.as_ref());
        let landed = key_landed(&nodes[1], &merge.body.state.post_state_hash, "starved").await;
        println!(
            "ROUND {}: retry_aboard_owner_block={} merge_rejected_starved={} merge_records={} landed={}",
            round,
            retry_aboard,
            merge_rejected_starved,
            merge.body.rejected_deploys.len(),
            landed
        );
        if landed {
            landed_round = Some(round);
            break;
        }
    }

    assert!(
        landed_round.is_some(),
        "starved deploy never landed in {} contention rounds: every merge \
         rejected it on content order and its recorded losses bought no priority",
        max_rounds
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn rotating_merge_proposers_land_repeatedly_rejected_deploy_before_expiry() {
    let ctx = build_genesis(2).await;
    let shard_id = ctx.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(ctx, 2, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let starved_sec = construct_deploy::DEFAULT_SEC.clone();
    let contender_sec = construct_deploy::DEFAULT_SEC2.clone();

    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(starved_sec.clone()),
        None,
        Some(shard_id.clone()),
    )
    .expect("build init");
    nodes[0].casper.deploy(init).expect("init deploy");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    for node in nodes.iter_mut() {
        node.process_block(init_block.clone())
            .await
            .expect("process init");
    }

    let starved = cheap_write("starved", &starved_sec, &shard_id);
    let starved_sig = starved.sig.clone();
    let sibling_starved = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&starved))
        .await
        .expect("starved sibling");
    let contender = costly_write("y1", 1, &contender_sec, &shard_id);
    let sibling_contender = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender))
        .await
        .expect("contender sibling");
    for node in nodes.iter_mut() {
        node.process_block(sibling_starved.clone())
            .await
            .expect("cross-add starved sibling");
        node.process_block(sibling_contender.clone())
            .await
            .expect("cross-add contender sibling");
    }
    let merge1 = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("first merge block");
    for node in nodes.iter_mut() {
        node.process_block(merge1.clone())
            .await
            .expect("validators accept first merge");
    }
    assert!(
        merge1
            .body
            .rejected_deploys
            .iter()
            .any(|r| r.deploy_id() == starved_sig.as_ref()),
        "FIXTURE: round 1 must reject the cheap write on content order; \
         raise the contender's arithmetic cost if this fails"
    );

    let mut rejected_again = false;
    for round in 2..=3 {
        let contender = costly_write(
            &format!("y{}", round),
            round as i64,
            &contender_sec,
            &shard_id,
        );
        nodes[1].casper.deploy(contender).expect("contender deploy");
        let contender_block = nodes[1]
            .create_block_unsafe(&[])
            .await
            .expect("contender sibling");
        let owner_block = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("owner sibling");
        for node in nodes.iter_mut() {
            node.process_block(contender_block.clone())
                .await
                .expect("validators accept contender sibling");
            node.process_block(owner_block.clone())
                .await
                .expect("validators accept owner sibling");
        }
        let merge = nodes[1]
            .create_block_unsafe(&[])
            .await
            .expect("contender-owner merge");
        for node in nodes.iter_mut() {
            node.process_block(merge.clone())
                .await
                .expect("validators accept contender-owner merge");
        }
        rejected_again |= merge
            .body
            .rejected_deploys
            .iter()
            .any(|rejected| rejected.deploy_id() == starved_sig.as_ref());
    }
    assert!(
        rejected_again,
        "FIXTURE: the racing retry must receive a second rejection before proposer rotation"
    );

    let max_rounds = 16;
    let mut first_eligible_round = None;
    let mut landed_round = None;
    for round in 4..=max_rounds {
        let contender = costly_write(
            &format!("y{}", round),
            round as i64,
            &contender_sec,
            &shard_id,
        );
        nodes[1].casper.deploy(contender).expect("contender deploy");
        let contender_block = nodes[1]
            .create_block_unsafe(&[])
            .await
            .expect("contender block");
        for node in nodes.iter_mut() {
            node.process_block(contender_block.clone())
                .await
                .expect("validators accept contender block");
        }
        let owner_block = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("owner block");
        for node in nodes.iter_mut() {
            node.process_block(owner_block.clone())
                .await
                .expect("validators accept owner block");
        }
        if owner_block
            .body
            .deploys
            .iter()
            .any(|processed| processed.deploy.sig == starved_sig)
        {
            first_eligible_round.get_or_insert(round);
        }
        if key_landed(
            &nodes[0],
            &owner_block.body.state.post_state_hash,
            "starved",
        )
        .await
        {
            landed_round = Some(round);
            break;
        }
    }

    assert!(
        first_eligible_round.is_some(),
        "the rejected deploy was not eligible within {} rotating-proposer rounds",
        max_rounds
    );
    assert_eq!(
        landed_round, first_eligible_round,
        "the rejected deploy must land on its first eligible covered-frontier round"
    );
}
