use casper::rust::util::{construct_deploy, proto_util};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployId, GUnforgeable, Par};

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const VAULT_REGISTRATION: &str = r#"       new bridgeVaultRegisterCh in {
        @SystemVault!("findOrCreate", bridgeVaultAddr, *bridgeVaultRegisterCh) |
        for (@(true, _) <- bridgeVaultRegisterCh) {
         stdout!(["bridge vault registered", bridgeVaultAddr])
        }
       } |
"#;

fn bridge_source(register_vault: bool) -> String {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge-v2.rho"),
    )
    .unwrap();
    assert_eq!(source.matches(VAULT_REGISTRATION).count(), 1);
    if register_vault {
        source
    } else {
        source.replace(VAULT_REGISTRATION, "")
    }
}

fn lock_uri(deploy_id_data: &[Par]) -> String {
    deploy_id_data
        .iter()
        .find_map(|par| match &par.exprs.first()?.expr_instance {
            Some(ExprInstance::EListBody(list)) if list.ps.len() == 3 => {
                match &list.ps[1].exprs.first()?.expr_instance {
                    Some(ExprInstance::GUri(uri)) => Some(uri.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("the bridge publishes its three contract URIs on deployId")
}

async fn lock_results(register_vault: bool) -> Vec<Par> {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let mut node = TestNode::standalone(genesis).await.unwrap();
    let shard_id = node.genesis.shard_id.clone();

    let bridge = construct_deploy::source_deploy_now_full(
        bridge_source(register_vault),
        Some(5_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let block = node
        .add_block_from_deploys(std::slice::from_ref(&bridge))
        .await
        .unwrap();
    assert!(block.body.deploys.iter().all(|deploy| !deploy.is_failed));
    let post_state = proto_util::post_state_hash(&block);

    let deploy_id = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: bridge.sig.to_vec(),
        })),
    }]);
    let published = node
        .runtime_manager
        .get_data(post_state.clone(), &deploy_id)
        .await
        .unwrap();
    let lock_uri = lock_uri(&published);

    let lock = construct_deploy::source_deploy_now_full(
        format!(
            r#"
new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
    VaultAddress(`rho:vault:address`), lockCh, meCh, svCh, keyCh in {{
  rl!(`{lock_uri}`, *lockCh) |
  rl!(`rho:vault:system`, *svCh) |
  VaultAddress!("fromDeployerId", *deployerId, *meCh) |
  for (lock <- lockCh & @(_, SystemVault) <- svCh & @me <- meCh) {{
    @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
    for (key <- keyCh) {{ lock!(100, "0x0000000000000000000000000000000000000001", me, *key, *return) }}
  }}
}}
"#
        ),
        Some(5_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id),
    )
    .unwrap();
    node.runtime_manager
        .capture_results(&post_state, &lock)
        .await
        .unwrap()
}

/// `lock` transfers into the bridge vault, and a transfer reaches a vault only
/// after `findOrCreate` created it. Bridge-v2.rho does that at init.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bridge_that_registers_its_vault_accepts_a_lock() {
    let results = lock_results(true).await;
    assert_eq!(results.len(), 1, "{results:?}");
    let first = match &results[0].exprs[0].expr_instance {
        Some(ExprInstance::EListBody(list)) => &list.ps[0].exprs[0].expr_instance,
        other => panic!("unexpected lock result {other:?}"),
    };
    assert_eq!(
        *first,
        Some(ExprInstance::GString("bridge lock OK".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bridge_without_its_vault_never_answers_a_lock() {
    assert!(lock_results(false).await.is_empty());
}
