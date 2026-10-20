use casper::rust::util::{construct_deploy, proto_util};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn transfer_to_fresh_address(register_target: bool) -> String {
    let transfer = r#"@SystemVault!("findOrCreate", me, *myVaultCh) |
        @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
        for (@(true, myVault) <- myVaultCh & key <- keyCh) {
          @myVault!("transfer", target, 1, *key, *return)
        }"#;
    let body = if register_target {
        format!(
            r#"@SystemVault!("findOrCreate", target, *targetVaultCh) |
        for (@(true, _) <- targetVaultCh) {{ {transfer} }}"#
        )
    } else {
        transfer.to_string()
    };
    format!(
        r#"
new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
    VaultAddress(`rho:vault:address`), seed, svCh, targetCh, meCh, myVaultCh,
    targetVaultCh, keyCh in {{
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

async fn transfer_results(register_target: bool) -> Vec<Par> {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();
    let deploy = construct_deploy::source_deploy_now_full(
        transfer_to_fresh_address(register_target),
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

/// A transfer sends `_deposit` to the target vault. That contract exists only
/// after `findOrCreate` created the vault, so a transfer to a fresh address
/// never answers. Bridge-v2.rho calls `findOrCreate` on its own vault for this reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_to_an_unregistered_vault_never_answers() {
    assert!(transfer_results(false).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transfer_to_a_registered_vault_succeeds() {
    let results = transfer_results(true).await;
    assert_eq!(results.len(), 1);
    let ok = match &results[0].exprs[0].expr_instance {
        Some(ExprInstance::ETupleBody(tuple)) => {
            matches!(
                tuple.ps[0].exprs[0].expr_instance,
                Some(ExprInstance::GBool(true))
            )
        }
        _ => false,
    };
    assert!(ok, "{:?}", results[0]);
}
