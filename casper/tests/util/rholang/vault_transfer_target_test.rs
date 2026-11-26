use casper::rust::util::{construct_deploy, proto_util};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::utils::new_gstring_par;
use num_bigint::BigInt;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn single_transfer_source(register_target: bool) -> String {
    let transfer = r#"@SystemVault!("findOrCreate", me, *myVaultCh) |
        @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
        for (@(true, myVault) <- myVaultCh & key <- keyCh) {
          @myVault!("transfer", target, 1, *key, *transferRet) |
          for (@transferResult <- transferRet) {
            @SystemVault!("findOrCreate", target, *targetVaultCh) |
            for (@(true, targetVault) <- targetVaultCh) {
              @targetVault!("balance", *balCh) |
              for (@bal <- balCh) {
                return!([transferResult, bal])
              }
            }
          }
        }"#;
    let body = if register_target {
        format!(
            r#"@SystemVault!("findOrCreate", target, *preTargetCh) |
        for (@(true, _) <- preTargetCh) {{ {transfer} }}"#
        )
    } else {
        transfer.to_string()
    };
    format!(
        r#"
new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
    VaultAddress(`rho:vault:address`), seed, svCh, targetCh, meCh, myVaultCh,
    preTargetCh, targetVaultCh, keyCh, transferRet, balCh in {{
  rl!(`rho:vault:system`, *svCh) |
  VaultAddress!("fromUnforgeable", *seed, *targetCh) |
  VaultAddress!("fromDeployerId", *deployerId, *meCh) |
  for (@(_, SystemVault) <- svCh & @target <- targetCh & @me <- meCh) {{
    {body}
  }}
}}
"#
    )
}

fn two_transfers_source() -> String {
    r#"
new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
    VaultAddress(`rho:vault:address`), seed, svCh, targetCh, meCh, myVaultCh,
    targetVaultCh, keyCh, firstRet, secondRet, balCh in {
  rl!(`rho:vault:system`, *svCh) |
  VaultAddress!("fromUnforgeable", *seed, *targetCh) |
  VaultAddress!("fromDeployerId", *deployerId, *meCh) |
  for (@(_, SystemVault) <- svCh & @target <- targetCh & @me <- meCh) {
    @SystemVault!("findOrCreate", me, *myVaultCh) |
    @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
    for (@(true, myVault) <- myVaultCh & key <- keyCh) {
      @myVault!("transfer", target, 1, *key, *firstRet) |
      @myVault!("transfer", target, 2, *key, *secondRet) |
      for (@firstResult <- firstRet; @secondResult <- secondRet) {
        @SystemVault!("findOrCreate", target, *targetVaultCh) |
        for (@(true, targetVault) <- targetVaultCh) {
          @targetVault!("balance", *balCh) |
          for (@bal <- balCh) {
            return!([firstResult, secondResult, bal])
          }
        }
      }
    }
  }
}
"#
    .to_string()
}

async fn run_deploy(source: String) -> Vec<Par> {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();
    let deploy = construct_deploy::source_deploy_now_full(
        source,
        Some(5_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(node.genesis.shard_id.clone()),
    )
    .unwrap();
    node.runtime_manager
        .capture_results(&proto_util::post_state_hash(&node.genesis), &deploy)
        .await
        .unwrap()
}

fn is_true_nil_tuple(par: &Par) -> bool {
    match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
        Some(ExprInstance::ETupleBody(tuple)) if tuple.ps.len() == 2 => {
            let first_true = matches!(
                tuple.ps[0]
                    .exprs
                    .first()
                    .and_then(|e| e.expr_instance.as_ref()),
                Some(ExprInstance::GBool(true))
            );
            let second_nil = tuple.ps[1].exprs.is_empty();
            first_true && second_nil
        }
        _ => false,
    }
}

fn big_int(par: &Par) -> BigInt {
    match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
        Some(ExprInstance::GBigInt(bytes)) => BigInt::from_signed_bytes_be(bytes),
        other => panic!("expected BigInt, got {other:?}"),
    }
}

fn list_items(par: &Par) -> &[Par] {
    match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
        Some(ExprInstance::EListBody(list)) => &list.ps,
        other => panic!("expected list, got {other:?}"),
    }
}

/// `_transferTemplate` calls `findOrCreate` on the target before `_deposit`, so
/// a transfer to a fresh address registers that address and completes inside
/// the sender's deploy. The target's balance after the transfer must equal the
/// amount sent — asserting only the reply tuple hides the race where the
/// deposit lands on a vault that `findOrCreate` doesn't return.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_to_an_unregistered_vault_registers_it_and_credits_the_target() {
    let results = run_deploy(single_transfer_source(false)).await;
    assert_eq!(results.len(), 1);
    let items = list_items(&results[0]);
    assert_eq!(items.len(), 2);
    assert!(
        is_true_nil_tuple(&items[0]),
        "transfer reply: {:?}",
        items[0]
    );
    assert_eq!(big_int(&items[1]), BigInt::from(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_to_a_registered_vault_credits_the_target() {
    let results = run_deploy(single_transfer_source(true)).await;
    assert_eq!(results.len(), 1);
    let items = list_items(&results[0]);
    assert_eq!(items.len(), 2);
    assert!(
        is_true_nil_tuple(&items[0]),
        "transfer reply: {:?}",
        items[0]
    );
    assert_eq!(big_int(&items[1]), BigInt::from(1));
}

/// Two parallel transfers to the same fresh address must share one vault. Before
/// `findOrCreate` became atomic, both `_transferTemplate` paths read Nil from the
/// vault map, both created a vault and installed a `_deposit` listener, and only
/// the last `set` stayed in the map — deposits to the loser's purse were
/// stranded and the read-back balance was 0, 1, or 2 instead of 3.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_transfers_to_the_same_fresh_address_accumulate() {
    let results = run_deploy(two_transfers_source()).await;
    assert_eq!(results.len(), 1);
    let items = list_items(&results[0]);
    assert_eq!(items.len(), 3);
    assert!(is_true_nil_tuple(&items[0]), "first reply: {:?}", items[0]);
    assert!(is_true_nil_tuple(&items[1]), "second reply: {:?}", items[1]);
    assert_eq!(big_int(&items[2]), BigInt::from(3));
}

async fn retained_continuations(node: &TestNode, term: &str) -> (usize, usize) {
    let root = Blake2b256Hash::from_bytes_prost(&proto_util::post_state_hash(&node.genesis));
    let repository = node.runtime_manager.get_history_repo();
    let before = repository.get_history_reader(&root).unwrap();
    let mut runtime = node.runtime_manager.spawn_runtime().await.unwrap();
    runtime.reset(&root).await.unwrap();

    let result = runtime.evaluate_with_term(term).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let completed = new_gstring_par("issue1101-lookups-completed".to_owned(), vec![], false);
    assert_eq!(runtime.get_data(&completed).await.len(), 1);

    let new_channels: Vec<_> = runtime
        .get_hot_changes()
        .await
        .into_iter()
        .filter(|(channels, row)| {
            !row.wks.is_empty() && before.get_continuations_proj_generic(channels).is_empty()
        })
        .map(|(channels, _)| channels)
        .collect();
    let checkpoint = runtime.create_checkpoint().await;
    let after = repository.get_history_reader(&checkpoint.root).unwrap();
    let mut persistent = 0;
    let mut linear = 0;
    for entry in new_channels
        .iter()
        .flat_map(|channels| after.get_continuations_proj_generic(channels))
    {
        if entry.persist {
            persistent += 1;
        } else {
            linear += 1;
        }
    }
    (persistent, linear)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lookups_of_existing_vaults_do_not_persist_new_factory_continuations() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let address = |index: usize| {
        VaultAddress::from_public_key(&genesis.genesis_vaults[index].1)
            .unwrap()
            .to_base58()
    };
    let sender = address(0);
    let recipient = address(1);
    let node = TestNode::standalone(genesis).await.unwrap();
    let term = format!(
        r#"
new lookup(`rho:registry:lookup`), vaultCh, first, second, third in {{
  lookup!(`rho:vault:system`, *vaultCh) |
  for (@(_, vault) <- vaultCh) {{
    @vault!("findOrCreate", "{sender}", *first) |
    @vault!("findOrCreate", "{recipient}", *second) |
    @vault!("findOrCreate", "{recipient}", *third) |
    for (@(true, _) <- first & @(true, _) <- second & @(true, _) <- third) {{
      @"issue1101-lookups-completed"!(true)
    }}
  }}
}}
"#
    );
    assert_eq!(
        retained_continuations(&node, &term).await,
        (0, 0),
        "existing vault lookups leave persistent or linear continuations"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_lookups_of_a_fresh_vault_only_keep_one_vaults_continuations() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();
    let source = |lookups| {
        format!(
            r#"
new lookup(`rho:registry:lookup`), address(`rho:vault:address`), vaultCh, targetCh,
    seed, first, second in {{
  lookup!(`rho:vault:system`, *vaultCh) |
  address!("fromUnforgeable", *seed, *targetCh) |
  for (@(_, vault) <- vaultCh & @target <- targetCh) {{
    {lookups}
  }}
}}
"#
        )
    };
    let single = source(
        r#"
    @vault!("findOrCreate", target, *first) |
    for (@(true, _) <- first) { @"issue1101-lookups-completed"!(true) }
"#,
    );
    let concurrent = source(
        r#"
    @vault!("findOrCreate", target, *first) |
    @vault!("findOrCreate", target, *second) |
    for (@(true, firstVault) <- first & @(true, secondVault) <- second) {
      if (firstVault == secondVault) { @"issue1101-lookups-completed"!(true) }
    }
"#,
    );
    let single_retained = retained_continuations(&node, &single).await;
    assert!(
        single_retained.0 > 0,
        "the new vault must install its own handlers"
    );
    assert_eq!(
        single_retained.1, 0,
        "creating one vault leaves a linear continuation"
    );
    assert_eq!(
        retained_continuations(&node, &concurrent).await,
        single_retained,
        "concurrent lookups leave more continuations than one vault creation"
    );
}
