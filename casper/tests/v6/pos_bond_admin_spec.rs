use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::system_deploy_util::generate_slash_deploy_random_seed;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::genesis_wallets::{read_bond_balance, read_wallet_balance};
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use models::rhoapi::Par;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
    owner: WalletOwner,
}

fn party() -> Party {
    let (sk, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    Party { sk, pk, owner }
}

async fn shard(
    funded: &[&Party],
    epoch_length: i32,
    quarantine_length: i32,
) -> (Arc<RuntimeManager>, StateHash, Vec<PublicKey>) {
    let rm = mk_runtime_manager("v6-bond-admin", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.proof_of_stake.epoch_length = epoch_length;
    genesis.proof_of_stake.quarantine_length = quarantine_length;
    genesis.vaults = funded
        .iter()
        .map(|p| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: 1_000_000,
        })
        .collect();
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let validators = genesis
        .proof_of_stake
        .validators
        .iter()
        .map(|v| v.pk.clone())
        .collect();
    (Arc::new(rm), block.body.state.post_state_hash.clone(), validators)
}

fn envelope(term: &str, ts: i64, who: &Party) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(
            term.to_string(),
            ts,
            Some(200_000),
            Some(1),
            Some(who.sk.clone()),
            None,
            Some("root".to_string()),
        )
        .unwrap(),
    )
}

fn block(number: i64, deploys: Vec<ProcessedDeploy>, proposer: &Party) -> BlockMessage {
    BlockMessage {
        block_hash: Bytes::from(format!("v6-bond-admin-{number}").into_bytes()),
        header: Header {
            parents_hash_list: vec![],
            timestamp: number,
            version: 1,
            extra_bytes: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
                bonds: vec![],
                block_number: number,
            },
            deploys,
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
            v6: None,
        },
        justifications: vec![],
        sender: proposer.pk.bytes.clone(),
        seq_num: number as i32,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

async fn pos_bond(rm: &RuntimeManager, root: &StateHash, pk: &PublicKey) -> Option<i64> {
    let query = format!(
        r#"new ret, rl(`rho:registry:lookup`), posCh, bondsCh in {{
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{
               @PoS!("getBonds", *bondsCh) |
               for (@bonds <- bondsCh) {{ ret!(bonds.getOrElse("{}".hexToBytes(), -1)) }}
             }}
           }}"#,
        hex::encode(&pk.bytes)
    );
    let (answer, _) = rm.play_exploratory_deploy(query, root, None).await.unwrap();
    RhoNumber::unapply(&answer[0]).filter(|b| *b >= 0)
}

async fn run_record(executor: &BlockExecutor, root: &StateHash, block: &BlockMessage) -> BlockRecord {
    let record = executor.execute(root, block).await.expect("the block executes");
    for (i, entry) in record.executed().iter().enumerate() {
        assert_eq!(
            entry.outcome,
            EnvelopeOutcome::Completed,
            "{:?}",
            record.gate().get(i).map(|g| g.verdict.clone())
        );
    }
    let replayed = executor.replay(&record, block).await.unwrap();
    assert_eq!(&replayed, record.post_root());
    record
}

async fn run(executor: &BlockExecutor, root: &StateHash, block: &BlockMessage) -> StateHash {
    run_record(executor, root, block).await.post_root().clone()
}

async fn close_charge(rm: &RuntimeManager, pre: &StateHash, record: &BlockRecord) -> u64 {
    let compute = compute_wallet_owner();
    let before = read_wallet_balance(rm, pre, &compute).await.unwrap();
    let after = read_wallet_balance(rm, record.post_root(), &compute).await.unwrap();
    after - before - record.settlement().unwrap().compute_credit
}

fn bond_term(bonds: &[(u64, u64, &str)]) -> String {
    let registrations = bonds
        .iter()
        .map(|(native, amount, stash)| {
            format!(
                r#"new bonded, registered, handleCh in {{
                     rb!("bond", (*deployerId, {native}), *bonded) |
                     for (@(true, receipt) <- bonded) {{
                       @PoS!("bond", receipt, {amount}, *registered, *handleCh)
                     }} |
                     for (@r <- registered) {{ @"registered-{stash}"!(r) }} |
                     for (@h <- handleCh) {{ @"{stash}"!(h) }}
                   }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        r#"new rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`), rl(`rho:registry:lookup`), posCh in {{
             @"published-id"!(*deployerId) |
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{ {registrations} }}
           }}"#
    )
}

fn with_handle(call: &str, answer: &str) -> String {
    format!(
        r#"new rb(`rho:rev:bond`), d(`rho:system:deployerId`), rl(`rho:registry:lookup`), posCh, handleCh, out in {{
             rb!("handle", *d, *handleCh) |
             for (@(true, handle) <- handleCh) {{
               rl!(`rho:system:pos`, *posCh) |
               for (@(_, PoS) <- posCh) {{ {call} | for (@r <- out) {{ @"{answer}"!(r) }} }}
             }}
           }}"#
    )
}

async fn data(rm: &RuntimeManager, root: &StateHash, channel: &str) -> Vec<Par> {
    rm.get_data(root.clone(), &RhoString::create_par(channel.to_string()))
        .await
        .unwrap()
}

async fn admitted(rm: &RuntimeManager, root: &StateHash, stash: &str) -> bool {
    let answers = data(rm, root, stash).await;
    assert_eq!(answers.len(), 1, "{stash}");
    let (ok, _) = RhoTuple2::unapply(&answers[0]).unwrap();
    RhoBoolean::unapply(&ok).unwrap()
}

async fn answers(rm: &RuntimeManager, root: &StateHash, answer: &str) -> Vec<(bool, Par)> {
    data(rm, root, answer)
        .await
        .iter()
        .map(|par| match RhoTuple2::unapply(par) {
            Some((ok, value)) => (RhoBoolean::unapply(&ok).unwrap(), value),
            None => (RhoBoolean::unapply(par).unwrap(), Par::default()),
        })
        .collect()
}

#[tokio::test]
async fn a_withdrawn_bond_is_released_to_its_validator_after_quarantine() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis, _) = shard(&[&alice], 1, 1).await;
    let executor = BlockExecutor::new(rm.clone(), 2);

    let bond = bond_term(&[(1_000, 1_000, "alice-handle")]);
    let bonded = run(&executor, &genesis, &block(1, vec![envelope(&bond, 1, &alice)], &proposer)).await;
    assert!(admitted(&rm, &bonded, "alice-handle").await);
    assert_eq!(read_bond_balance(&rm, &bonded, &alice.pk.bytes).await.unwrap(), 1_000);
    assert_eq!(pos_bond(&rm, &bonded, &alice.pk).await, Some(1_000));

    let withdraw = with_handle(r#"@PoS!("withdraw", handle, *out)"#, "answer");
    let withdrawn = run(&executor, &bonded, &block(2, vec![envelope(&withdraw, 2, &alice)], &proposer)).await;
    assert_eq!(answers(&rm, &withdrawn, "answer").await, vec![(true, Par::default())]);
    assert_eq!(pos_bond(&rm, &withdrawn, &alice.pk).await, None);
    assert_eq!(read_bond_balance(&rm, &withdrawn, &alice.pk.bytes).await.unwrap(), 1_000);

    let quarantined = run(&executor, &withdrawn, &block(3, vec![], &proposer)).await;
    assert_eq!(read_bond_balance(&rm, &quarantined, &alice.pk.bytes).await.unwrap(), 1_000);

    let before = read_wallet_balance(&rm, &quarantined, &alice.owner).await.unwrap();
    let released = run(&executor, &quarantined, &block(4, vec![], &proposer)).await;
    assert_eq!(read_bond_balance(&rm, &released, &alice.pk.bytes).await.unwrap(), 0);
    assert_eq!(
        read_wallet_balance(&rm, &released, &alice.owner).await.unwrap(),
        before + 1_000
    );
}

#[tokio::test]
async fn two_pos_bonds_on_one_native_bond_admit_the_first_and_release_the_rest() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis, _) = shard(&[&alice], 1, 1).await;
    let executor = BlockExecutor::new(rm.clone(), 2);

    let bond = bond_term(&[(1_000, 1_000, "first"), (500, 1_000, "second")]);
    let record = run_record(&executor, &genesis, &block(1, vec![envelope(&bond, 1, &alice)], &proposer)).await;
    let post = record.post_root().clone();
    let charge = record.settlement().unwrap().debited();

    let first = admitted(&rm, &post, "first").await;
    let second = admitted(&rm, &post, "second").await;
    assert!(first != second, "exactly one is admitted");
    assert_eq!(pos_bond(&rm, &post, &alice.pk).await, Some(1_000));
    assert_eq!(read_bond_balance(&rm, &post, &alice.pk.bytes).await.unwrap(), 1_000);
    assert_eq!(
        read_wallet_balance(&rm, &post, &alice.owner).await.unwrap(),
        1_000_000 - 1_500 - charge + 500 - close_charge(&rm, &genesis, &record).await
    );
}

#[tokio::test]
async fn a_pending_bond_backed_only_by_a_withdrawn_bond_is_dropped() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis, _) = shard(&[&alice], 1, 10).await;
    let executor = BlockExecutor::new(rm.clone(), 2);

    let bond = bond_term(&[(1_000, 1_000, "alice-handle")]);
    let bonded = run(&executor, &genesis, &block(1, vec![envelope(&bond, 1, &alice)], &proposer)).await;
    let withdraw = with_handle(r#"@PoS!("withdraw", handle, *out)"#, "answer");
    let withdrawn = run(&executor, &bonded, &block(2, vec![envelope(&withdraw, 2, &alice)], &proposer)).await;
    assert_eq!(pos_bond(&rm, &withdrawn, &alice.pk).await, None);

    let again = r#"new rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`), rl(`rho:registry:lookup`),
            posCh, bonded, registered, handleCh in {
          rb!("bond", (*deployerId, 1), *bonded) |
          for (@(true, receipt) <- bonded) {
            rl!(`rho:system:pos`, *posCh) |
            for (@(_, PoS) <- posCh) { @PoS!("bond", receipt, 1000, *registered, *handleCh) }
          } |
          for (@r <- registered) { @"registered-again"!(r) } |
          for (@h <- handleCh) { @"again"!(h) }
        }"#.to_string();
    let before = read_wallet_balance(&rm, &withdrawn, &alice.owner).await.unwrap();
    let record = run_record(&executor, &withdrawn, &block(3, vec![envelope(&again, 3, &alice)], &proposer)).await;
    let rebonded = record.post_root().clone();
    assert!(!admitted(&rm, &rebonded, "again").await);
    assert_eq!(pos_bond(&rm, &rebonded, &alice.pk).await, None);
    assert_eq!(read_bond_balance(&rm, &rebonded, &alice.pk.bytes).await.unwrap(), 1_000);
    assert_eq!(
        read_wallet_balance(&rm, &rebonded, &alice.owner).await.unwrap(),
        before - record.settlement().unwrap().debited() - close_charge(&rm, &withdrawn, &record).await
    );
}

#[tokio::test]
async fn a_published_deployer_id_does_not_act_for_its_validator() {
    let (alice, mallory, proposer) = (party(), party(), party());
    let (rm, genesis, _) = shard(&[&alice, &mallory], 1, 10).await;
    let executor = BlockExecutor::new(rm.clone(), 2);

    let bond = bond_term(&[(1_000, 1_000, "alice-handle")]);
    let bonded = run(&executor, &genesis, &block(1, vec![envelope(&bond, 1, &alice)], &proposer)).await;
    assert!(admitted(&rm, &bonded, "alice-handle").await);

    let impersonate = r#"new rl(`rho:registry:lookup`), rb(`rho:rev:bond`), keccak(`rho:crypto:keccak256Hash`), posCh, imageCh, out in {
        for (@aliceId <<- @"published-id") {
          rl!(`rho:system:pos`, *posCh) |
          keccak!("secret".toByteArray(), *imageCh) |
          for (@(_, PoS) <- posCh & @image <- imageCh) {
            @PoS!("withdraw", aliceId, *out) |
            @PoS!("commitRandomImage", aliceId, image, *out) |
            @PoS!("revealRandom", aliceId, "secret".toByteArray(), *out) |
            rb!("handle", aliceId, *out) |
            new taken in {
              @PoS!("bond", aliceId, 1000, *out, *taken) |
              for (@h <- taken) { @"mallory-handle"!(h) }
            } |
            for (@r <= out) { @"answer"!(r) }
          }
        }
      }"#;
    let refused = run(&executor, &bonded, &block(2, vec![envelope(impersonate, 2, &mallory)], &proposer)).await;
    let refusals = answers(&rm, &refused, "answer").await;
    assert_eq!(refusals.len(), 5);
    let mut messages: Vec<_> = refusals
        .iter()
        .map(|(ok, message)| {
            assert!(!ok);
            RhoString::unapply(message).unwrap()
        })
        .collect();
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "Unknown validator handle".to_string(),
            "Unknown validator handle".to_string(),
            "Unknown validator handle".to_string(),
            "expected a bond receipt from rho:rev:bond".to_string(),
            "the validator must hold the enclosing seal".to_string(),
        ]
    );
    assert!(data(&rm, &refused, "mallory-handle").await.is_empty(), "no handle for a published id");
    assert_eq!(pos_bond(&rm, &refused, &alice.pk).await, Some(1_000));

    let own = with_handle(
        r#"new keccak(`rho:crypto:keccak256Hash`), imageCh, committed in {
             keccak!("secret".toByteArray(), *imageCh) |
             for (@image <- imageCh) {
               @PoS!("commitRandomImage", handle, image, *committed) |
               for (@c <- committed) { @PoS!("revealRandom", handle, "secret".toByteArray(), *out) | @"own-answer"!(c) }
             }
           }"#,
        "own-answer",
    );
    let acted = run(&executor, &refused, &block(3, vec![envelope(&own, 3, &alice)], &proposer)).await;
    let mut own_answers = answers(&rm, &acted, "own-answer").await;
    own_answers.sort_by_key(|(ok, _)| *ok);
    assert_eq!(own_answers, vec![(true, Par::default()), (true, Par::default())]);
}

#[tokio::test]
async fn a_slash_moves_the_offenders_bond_to_the_compute_wallet() {
    let proposer = party();
    let (rm, genesis, validators) = shard(&[], 1000, 50_000).await;
    let offender = validators[0].clone();
    let stake = read_bond_balance(&rm, &genesis, &offender.bytes).await.unwrap();
    assert!(stake > 0);
    assert_eq!(pos_bond(&rm, &genesis, &offender).await, Some(stake as i64));
    let compute = read_wallet_balance(&rm, &genesis, &compute_wallet_owner()).await.unwrap();

    let invalid = Bytes::from_static(b"an-equivocation-of-the-offender");
    let slash = SystemDeployEnum::Slash(SlashDeploy {
        invalid_block_hash: invalid.clone(),
        pk: proposer.pk.clone(),
        target_activation_epoch: 0,
        initial_rand: generate_slash_deploy_random_seed(proposer.pk.bytes.clone(), 1, &invalid),
    });
    let data = BlockData {
        time_stamp: 1,
        block_number: 1,
        sender: proposer.pk.clone(),
        seq_num: 1,
    };
    let invalid_blocks = HashMap::from([(invalid.clone(), offender.bytes.clone())]);
    let (slashed, _, system) = rm
        .compute_state(&genesis, vec![], vec![slash], data.clone(), Some(invalid_blocks.clone()))
        .await
        .expect("the slash runs");
    assert_eq!(system.len(), 1);
    assert!(!system[0].clone().failed(), "{:?}", system[0]);

    assert_eq!(read_bond_balance(&rm, &slashed, &offender.bytes).await.unwrap(), 0);
    assert_eq!(pos_bond(&rm, &slashed, &offender).await, Some(0));
    assert_eq!(
        read_wallet_balance(&rm, &slashed, &compute_wallet_owner()).await.unwrap(),
        compute + stake
    );

    let replayed = rm
        .replay_compute_state(&genesis, vec![], system, &data, Some(invalid_blocks), false)
        .await
        .expect("the slash replays");
    assert_eq!(replayed, slashed);
}

#[tokio::test]
async fn a_continuation_fired_inside_the_close_is_paid_by_its_installer() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis, _) = shard(&[&alice], 1, 10).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let heavy = "new y in { y!(1) | for (_ <- y) { new z in { z!(2) | for (_ <- z) { Nil } } } }";
    let light = bond_term(&[(1_000, 1_000, "light")]);
    let loaded = light.replace(
        r#"for (@h <- handleCh) { @"light"!(h) }"#,
        &format!(r#"for (@h <- handleCh) {{ @"light"!(h) | {heavy} }}"#),
    );
    assert_ne!(loaded, light);
    let compute = compute_wallet_owner();

    let record = run_record(&executor, &genesis, &block(1, vec![envelope(&loaded, 1, &alice)], &proposer)).await;
    let post = record.post_root().clone();
    assert!(admitted(&rm, &post, "light").await);
    let settlement = record.settlement().unwrap().clone();
    let charged_in_run = settlement.debited();
    let expected_without_close = 1_000_000 - 1_000 - charged_in_run;
    let left = read_wallet_balance(&rm, &post, &alice.owner).await.unwrap();
    assert!(left < expected_without_close, "the close charged the installer");
    let in_close = expected_without_close - left;
    let before = read_wallet_balance(&rm, &genesis, &compute).await.unwrap();
    let after = read_wallet_balance(&rm, &post, &compute).await.unwrap();
    assert_eq!(after - before, settlement.compute_credit + in_close);
}

#[tokio::test]
async fn a_bond_receipt_is_spent_by_its_first_registration() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis, _) = shard(&[&alice], 1, 10).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let twice = r#"new rb(`rho:rev:bond`), d(`rho:system:deployerId`), rl(`rho:registry:lookup`),
            posCh, bonded, first, second, h1, h2 in {
          rb!("bond", (*d, 1000), *bonded) |
          for (@(true, receipt) <- bonded) {
            rl!(`rho:system:pos`, *posCh) |
            for (@(_, PoS) <- posCh) {
              @PoS!("bond", receipt, 500, *first, *h1) |
              for (@r <- first) {
                @"first"!(r) |
                @PoS!("bond", receipt, 500, *second, *h2) |
                for (@r2 <- second) { @"second"!(r2) }
              }
            }
          } |
          for (@h <- h1) { @"h1"!(h) } |
          for (@h <- h2) { @"h2"!(h) }
        }"#;
    let post = run(&executor, &genesis, &block(1, vec![envelope(twice, 1, &alice)], &proposer)).await;
    let reply = |channel: &'static str| {
        let (rm, post) = (rm.clone(), post.clone());
        async move {
            let d = data(&rm, &post, channel).await;
            assert_eq!(d.len(), 1, "{channel}");
            let (ok, why) = RhoTuple2::unapply(&d[0]).unwrap();
            (RhoBoolean::unapply(&ok).unwrap(), RhoString::unapply(&why))
        }
    };
    assert_eq!(reply("first").await, (true, None));
    assert_eq!(
        reply("second").await,
        (false, Some("This bond receipt is already spent.".to_string()))
    );
    assert!(admitted(&rm, &post, "h1").await);
    assert!(data(&rm, &post, "h2").await.is_empty());
    assert_eq!(pos_bond(&rm, &post, &alice.pk).await, Some(500));
}

#[tokio::test]
async fn a_genesis_validator_takes_its_handle_at_its_own_seal() {
    let proposer = party();
    let rm = mk_runtime_manager("v6-bond-admin", Some(Genesis::default_mergeable_tags_arc())).await;
    let (validator_keys, _, mut genesis) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    let (sk, pk) = validator_keys[0].clone();
    let validator = Party {
        owner: WalletOwner::from_public_key(&pk).unwrap(),
        sk,
        pk,
    };
    genesis.vaults = vec![Vault {
        vault_address: VaultAddress::from_public_key(&validator.pk).unwrap(),
        initial_balance: 1_000_000,
    }];
    let block0 = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let rm = Arc::new(rm);
    let pre = block0.body.state.post_state_hash.clone();
    assert!(pos_bond(&rm, &pre, &validator.pk).await.is_some());
    let executor = BlockExecutor::new(rm.clone(), 2);
    let own = with_handle(
        r#"new keccak(`rho:crypto:keccak256Hash`), imageCh, committed in {
             keccak!("secret".toByteArray(), *imageCh) |
             for (@image <- imageCh) {
               @PoS!("commitRandomImage", handle, image, *committed) |
               for (@c <- committed) { @PoS!("revealRandom", handle, "secret".toByteArray(), *out) | @"genesis-answer"!(c) }
             }
           }"#,
        "genesis-answer",
    );
    let post = run(&executor, &pre, &block(1, vec![envelope(&own, 1, &validator)], &proposer)).await;
    let mut got = answers(&rm, &post, "genesis-answer").await;
    got.sort_by_key(|(ok, _)| *ok);
    assert_eq!(got, vec![(true, Par::default()), (true, Par::default())]);
}

async fn active(rm: &RuntimeManager, root: &StateHash, pk: &PublicKey) -> bool {
    let query = format!(
        r#"new ret, rl(`rho:registry:lookup`), posCh, activeCh in {{
             rl!(`rho:system:pos`, *posCh) |
             for (@(_, PoS) <- posCh) {{
               @PoS!("getActiveValidators", *activeCh) |
               for (@active <- activeCh) {{ ret!(active.contains("{}".hexToBytes())) }}
             }}
           }}"#,
        hex::encode(&pk.bytes)
    );
    let (answer, _) = rm.play_exploratory_deploy(query, root, None).await.unwrap();
    RhoBoolean::unapply(&answer[0]).unwrap()
}

#[tokio::test]
async fn an_epoch_starts_at_the_first_close_at_or_past_its_boundary() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis, _) = shard(&[&alice, &bob], 3, 10).await;
    let executor = BlockExecutor::new(rm.clone(), 2);
    let bond = |stash: &str| bond_term(&[(1_000, 1_000, stash)]);

    let first = run(&executor, &genesis, &block(1, vec![envelope(&bond("a"), 1, &alice)], &proposer)).await;
    assert_eq!(pos_bond(&rm, &first, &alice.pk).await, Some(1_000));
    assert!(!active(&rm, &first, &alice.pk).await, "block 1 is inside epoch 0");

    let past_void = run(&executor, &first, &block(4, vec![], &proposer)).await;
    assert!(
        active(&rm, &past_void, &alice.pk).await,
        "block 3 was void, so the close of block 4 starts epoch 1"
    );

    let same_epoch = run(&executor, &past_void, &block(5, vec![envelope(&bond("b"), 5, &bob)], &proposer)).await;
    assert_eq!(pos_bond(&rm, &same_epoch, &bob.pk).await, Some(1_000));
    assert!(!active(&rm, &same_epoch, &bob.pk).await, "epoch 1 starts only once");

    let next = run(&executor, &same_epoch, &block(6, vec![], &proposer)).await;
    assert!(active(&rm, &next, &bob.pk).await, "block 6 is the boundary of epoch 2");
}
