use casper::rust::util::proto_util;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const DONE: &str = "issue1156-auth-key-residue-done";

async fn new_continuations(node: &TestNode, term: &str) -> (usize, usize) {
    let root = Blake2b256Hash::from_bytes_prost(&proto_util::post_state_hash(&node.genesis));
    let repository = node.runtime_manager.get_history_repo();
    let before = repository.get_history_reader(&root).unwrap();
    let mut runtime = node.runtime_manager.spawn_runtime().await.unwrap();
    runtime.reset(&root).await.unwrap();

    let result = runtime.evaluate_with_term(term).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let done = new_gstring_par(DONE.to_owned(), vec![], false);
    assert_eq!(
        runtime.get_data(&done).await.len(),
        1,
        "the term did not finish"
    );

    let channels: Vec<_> = runtime
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
    channels
        .iter()
        .flat_map(|channels| after.get_continuations_proj_generic(channels))
        .fold((0, 0), |(persistent, linear), entry| {
            if entry.persist {
                (persistent + 1, linear)
            } else {
                (persistent, linear + 1)
            }
        })
}

async fn node() -> TestNode {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    TestNode::standalone(genesis).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_counter_sees_a_new_persistent_contract() {
    let node = node().await;
    let term = format!(
        r#"
new c in {{
  contract c(@x) = {{ Nil }} |
  @"{DONE}"!(true)
}}
"#
    );
    assert_eq!(new_continuations(&node, &term).await, (1, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn made_and_checked_auth_keys_leave_no_continuations() {
    let node = node().await;
    let term = format!(
        r#"
new lookup(`rho:registry:lookup`), authKeyCh, shapeA, shapeB, k1, k2, k3, c1, c2, c3, c4 in {{
  lookup!(`rho:system:authKey`, *authKeyCh) |
  for (@(_, AuthKey) <- authKeyCh) {{
    @AuthKey!("make", *shapeA, *k1) |
    @AuthKey!("make", *shapeA, *k2) |
    @AuthKey!("make", *shapeB, *k3) |
    for (@key1 <- k1 & @key2 <- k2 & @key3 <- k3) {{
      @AuthKey!("check", key1, *shapeA, *c1) |
      @AuthKey!("check", key2, *shapeA, *c2) |
      @AuthKey!("check", key3, *shapeB, *c3) |
      @AuthKey!("check", key3, *shapeA, *c4) |
      for (@true <- c1 & @true <- c2 & @true <- c3 & @false <- c4) {{
        if (key1 == key2 and key1 != key3) {{ @"{DONE}"!(true) }}
      }}
    }}
  }}
}}
"#
    );
    assert_eq!(new_continuations(&node, &term).await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_system_vault_auth_key_leaves_no_continuations() {
    let node = node().await;
    let term = format!(
        r#"
new lookup(`rho:registry:lookup`), vaultCh, unf, keyCh in {{
  lookup!(`rho:vault:system`, *vaultCh) |
  for (@(_, SystemVault) <- vaultCh) {{
    @SystemVault!("unforgeableAuthKey", *unf, *keyCh) |
    for (_ <- keyCh) {{ @"{DONE}"!(true) }}
  }}
}}
"#
    );
    assert_eq!(new_continuations(&node, &term).await, (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_built_without_the_private_name_fails_the_check() {
    let node = node().await;
    let term = format!(
        r#"
new lookup(`rho:registry:lookup`), authKeyCh, shape, fakeName, c1, c2 in {{
  lookup!(`rho:system:authKey`, *authKeyCh) |
  for (@(_, AuthKey) <- authKeyCh) {{
    @AuthKey!("check", bundle+{{ (*fakeName, *shape) }}, *shape, *c1) |
    @AuthKey!("check", *shape, *shape, *c2) |
    for (@false <- c1 & @false <- c2) {{ @"{DONE}"!(true) }}
  }}
}}
"#
    );
    assert_eq!(new_continuations(&node, &term).await, (0, 0));
}
