use std::collections::BTreeMap;
use std::sync::Arc;

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::executor::{EnvelopeExecutor, ExecutedBlock};
use casper::rust::v6::genesis_wallets::read_wallet_balance;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

struct Party {
    sk: PrivateKey,
    pk: PublicKey,
    owner: WalletOwner,
    address: String,
}

fn party() -> Party {
    let (sk, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Party { sk, pk, owner, address }
}

async fn shard(funded: &[&Party]) -> (Arc<RuntimeManager>, StateHash) {
    let rm = mk_runtime_manager("v6-genesis-records", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = funded
        .iter()
        .map(|p| Vault {
            vault_address: VaultAddress::from_public_key(&p.pk).unwrap(),
            initial_balance: 10_000_000,
        })
        .collect();
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    (Arc::new(rm), block.body.state.post_state_hash.clone())
}

async fn execute(
    rm: &Arc<RuntimeManager>,
    pre: &StateHash,
    term: &str,
    who: &Party,
    proposer: &Party,
) -> ExecutedBlock {
    let envelope = construct_deploy::source_deploy(
        term.to_string(),
        1,
        Some(5_000_000),
        Some(1),
        Some(who.sk.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap();
    let data = BlockData {
        time_stamp: 1,
        block_number: 1,
        sender: proposer.pk.clone(),
        seq_num: 1,
    };
    let executed = EnvelopeExecutor::new(rm.clone(), 2)
        .execute(pre, vec![envelope], data)
        .await
        .unwrap();
    assert_eq!(executed.gate.admitted(), vec![true]);
    assert!(!executed.processed[0].is_failed, "{:?}", executed.processed[0]);
    executed
}

async fn checks(rm: &RuntimeManager, root: &StateHash) -> BTreeMap<String, bool> {
    rm.get_data(root.clone(), &RhoString::create_par("check".to_string()))
        .await
        .unwrap()
        .iter()
        .map(|par| {
            let (label, ok) = RhoTuple2::unapply(par).unwrap();
            (RhoString::unapply(&label).unwrap(), RhoBoolean::unapply(&ok).unwrap())
        })
        .collect()
}

const UTILITIES: &str = r#"
new rl(`rho:registry:lookup`), insert(`rho:registry:insertArbitrary`),
    listOpsCh, eitherCh, numberCh, mintCh, authCh, stackCh, tokenCh, mapCh in {
  rl!(`rho:lang:listOps`, *listOpsCh) |
  for (@(_, ListOps) <- listOpsCh) {
    new sum, folded, add in {
      contract add(@x, @acc, ret) = { ret!(x + acc) } |
      @ListOps!("sum", [1, 2, 3], *sum) |
      @ListOps!("fold", [1, 2, 3], 10, *add, *folded) |
      for (@s <- sum & @f <- folded) { @"check"!(("listOps", s == 6 and f == 16)) }
    }
  } |
  rl!(`rho:lang:either`, *eitherCh) |
  for (@(_, Either) <- eitherCh) {
    new right, left in {
      @Either!("fromBoolean", true, "no", *right) |
      @Either!("fromBoolean", false, "no", *left) |
      for (@r <- right & @l <- left) { @"check"!(("either", r == (true, Nil) and l == (false, "no"))) }
    }
  } |
  rl!(`rho:lang:nonNegativeNumber`, *numberCh) |
  for (@(_, NonNegativeNumber) <- numberCh) {
    new numberRet, added, value in {
      @NonNegativeNumber!(5, *numberRet) |
      for (@number <- numberRet) {
        @number!("add", 3, *added) |
        for (@ok <- added) {
          @number!("value", *value) |
          for (@v <- value) { @"check"!(("nonNegativeNumber", ok and v == 8n)) }
        }
      }
    }
  } |
  rl!(`rho:system:makeMint`, *mintCh) |
  for (@(_, MakeMint) <- mintCh) {
    new mintRet, purseRet, balance in {
      @MakeMint!(*mintRet) |
      for (@mint <- mintRet) {
        @mint!("makePurse", 10, *purseRet) |
        for (@purse <- purseRet) {
          @purse!("getBalance", *balance) |
          for (@b <- balance) { @"check"!(("makeMint", b == 10n)) }
        }
      }
    }
  } |
  rl!(`rho:system:authKey`, *authCh) |
  for (@(_, AuthKey) <- authCh) {
    new keyRet, same, other in {
      @AuthKey!("make", "shape", *keyRet) |
      for (key <- keyRet) {
        @AuthKey!("check", *key, "shape", *same) |
        @AuthKey!("check", *key, "other", *other) |
        for (@s <- same & @o <- other) { @"check"!(("authKey", s and not o)) }
      }
    }
  } |
  rl!(`rho:lang:stack`, *stackCh) |
  for (@(_, Stack) <- stackCh) {
    new stackRet, empty in {
      @Stack!("init", *stackRet) |
      for (@stack <- stackRet) {
        @Stack!("isEmpty", stack, *empty) |
        for (@e <- empty) { @"check"!(("stack", e)) }
      }
    }
  } |
  rl!(`rho:system:tokenMetadata`, *tokenCh) |
  for (@(_, TokenMetadata) <- tokenCh) {
    new all in {
      @TokenMetadata!("all", *all) |
      for (@(name, symbol, decimals) <- all) {
        @"check"!(("tokenMetadata", name != "" and symbol != "" and decimals >= 0))
      }
    }
  } |
  rl!(`rho:lang:treeHashMap`, *mapCh) |
  for (TreeHashMap <- mapCh) {
    new mapRet, set, got in {
      TreeHashMap!("init", 2, *mapRet) |
      for (@map <- mapRet) {
        TreeHashMap!("set", map, "k", 7, *set) |
        for (_ <- set) {
          TreeHashMap!("get", map, "k", *got) |
          for (@v <- got) { @"check"!(("treeHashMap", v == 7)) }
        }
      }
    }
  } |
  new uriRet, looked, target in {
    insert!(bundle+{*target}, *uriRet) |
    for (@uri <- uriRet) {
      rl!(uri, *looked) |
      for (@found <- looked) { @"check"!(("registry", found == bundle+{*target})) }
    }
  }
}
"#;

const VAULT_FACADE: &str = r#"
new rl(`rho:registry:lookup`), d(`rho:system:deployerId`), svCh, found, balance, transfer in {
  rl!(`rho:vault:system`, *svCh) |
  for (@(_, SystemVault) <- svCh) {
    @SystemVault!("find", "__ALICE__", *found) |
    @SystemVault!("balance", "__ALICE__", *balance) |
    @SystemVault!("transfer", "__BOB__", 100, *d, *transfer) |
    for (@(ok, handle) <- found) { @"check"!(("find", ok and handle != Nil)) } |
    for (@(ok, b) <- balance) { @"check"!(("balance", ok and b > 0 and b <= 10000000)) } |
    for (_ <- transfer) { @"check"!(("transfer answered", false)) }
  }
}
"#;

const POS_ENTRY_POINTS: &str = r#"
new rl(`rho:registry:lookup`), d(`rho:system:deployerId`), keccak(`rho:crypto:keccak256Hash`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) {
    new bonds, active, withdrawers, pending, rewards, epochRewards, epoch, quarantine,
        threshold, params, withdraw, slash, close, imageCh, committed, revealed, registered, handleCh in {
      @PoS!("getBonds", *bonds) |
      @PoS!("getActiveValidators", *active) |
      @PoS!("getWithdrawers", *withdrawers) |
      @PoS!("getPendingWithdrawer", *pending) |
      @PoS!("getRewards", *rewards) |
      @PoS!("getCurrentEpochRewards", *epochRewards) |
      @PoS!("getEpochLength", *epoch) |
      @PoS!("getQuarantineLength", *quarantine) |
      @PoS!("getFaultToleranceThresholdPpm", *threshold) |
      @PoS!("getConsensusParameters", *params) |
      for (@b <- bonds) { @"check"!(("getBonds", b.size() == 1)) } |
      for (@a <- active) { @"check"!(("getActiveValidators", a.size() == 1)) } |
      for (@w <- withdrawers) { @"check"!(("getWithdrawers", w == {})) } |
      for (@p <- pending) { @"check"!(("getPendingWithdrawer", p == {})) } |
      for (@r <- rewards) { @"check"!(("getRewards", r == {})) } |
      for (@r <- epochRewards) { @"check"!(("getCurrentEpochRewards", r.size() == 1)) } |
      for (@e <- epoch) { @"check"!(("getEpochLength", e > 0)) } |
      for (@q <- quarantine) { @"check"!(("getQuarantineLength", q >= 0)) } |
      for (@t <- threshold) { @"check"!(("getFaultToleranceThresholdPpm", t >= 0)) } |
      for (@(depth, lifespan, price) <- params) {
        @"check"!(("getConsensusParameters", depth != Nil and lifespan != Nil and price != Nil))
      } |
      @PoS!("withdraw", *d, *withdraw) |
      for (@(ok, why) <- withdraw) { @"check"!(("withdraw", not ok and why == "Unknown validator handle")) } |
      @PoS!("slash", *d, "00".hexToBytes(), Nil, *slash) |
      for (@(ok, _) <- slash) { @"check"!(("slash", not ok)) } |
      @PoS!("closeBlock", Nil, *close) |
      for (@(ok, _) <- close) { @"check"!(("closeBlock", not ok)) } |
      keccak!("secret".toByteArray(), *imageCh) |
      for (@image <- imageCh) {
        @PoS!("commitRandomImage", *d, image, *committed) |
        @PoS!("revealRandom", *d, "secret".toByteArray(), *revealed) |
        for (@(c, why) <- committed) { @"check"!(("commitRandomImage", not c and why == "Unknown validator handle")) } |
        for (@(r, why) <- revealed) { @"check"!(("revealRandom", not r and why == "Unknown validator handle")) }
      } |
      @PoS!("bond", *d, 1000, *registered, *handleCh) |
      for (@b <- registered) { @"check"!(("bond", b == (false, "expected a bond receipt from rho:rev:bond"))) }
    }
  }
}
"#;

#[tokio::test]
async fn the_v6_utility_contracts_answer_under_seal_records() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[&alice]).await;
    let executed = execute(&rm, &genesis, UTILITIES, &alice, &proposer).await;
    let checks = checks(&rm, &executed.mid_root).await;
    let labels = [
        "authKey",
        "either",
        "listOps",
        "makeMint",
        "nonNegativeNumber",
        "registry",
        "stack",
        "tokenMetadata",
        "treeHashMap",
    ];
    assert_eq!(checks.keys().cloned().collect::<Vec<_>>(), labels.map(String::from).to_vec());
    assert!(checks.values().all(|ok| *ok), "{checks:?}");
}

#[tokio::test]
async fn the_v6_vault_facade_finds_and_reads_under_seal_records_and_has_no_transfer() {
    let (alice, bob, proposer) = (party(), party(), party());
    let (rm, genesis) = shard(&[&alice]).await;
    let term = VAULT_FACADE
        .replace("__ALICE__", &alice.address)
        .replace("__BOB__", &bob.address);
    let executed = execute(&rm, &genesis, &term, &alice, &proposer).await;
    let checks = checks(&rm, &executed.mid_root).await;
    assert_eq!(
        checks,
        BTreeMap::from([("balance".to_string(), true), ("find".to_string(), true)])
    );
    assert_eq!(read_wallet_balance(&rm, &executed.settled_root, &bob.owner).await.unwrap(), 0);
    assert_eq!(
        read_wallet_balance(&rm, &executed.settled_root, &alice.owner).await.unwrap(),
        10_000_000 - executed.settlement.debited()
    );
}

#[tokio::test]
async fn every_pos_v6_entry_point_answers_under_seal_records() {
    let (alice, proposer) = (party(), party());
    let (rm, genesis) = shard(&[&alice]).await;
    let executed = execute(&rm, &genesis, POS_ENTRY_POINTS, &alice, &proposer).await;
    let checks = checks(&rm, &executed.mid_root).await;
    let labels = [
        "bond",
        "closeBlock",
        "commitRandomImage",
        "getActiveValidators",
        "getBonds",
        "getConsensusParameters",
        "getCurrentEpochRewards",
        "getEpochLength",
        "getFaultToleranceThresholdPpm",
        "getPendingWithdrawer",
        "getQuarantineLength",
        "getRewards",
        "getWithdrawers",
        "revealRandom",
        "slash",
        "withdraw",
    ];
    assert_eq!(checks.keys().cloned().collect::<Vec<_>>(), labels.map(String::from).to_vec());
    assert!(checks.values().all(|ok| *ok), "{checks:?}");
}
