use casper::rust::util::proto_util;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Evaluate `term` starting from `root`, then return the number of new
/// persistent and linear continuations installed on any channel that
/// had no continuations at `root`, together with the resulting root so
/// a follow-up deploy can resume from it.
async fn retained_continuations_from(
    node: &TestNode,
    root: Blake2b256Hash,
    term: &str,
    completion_marker: &str,
) -> (usize, usize, Blake2b256Hash) {
    let repository = node.runtime_manager.get_history_repo();
    let before = repository.get_history_reader(&root).unwrap();
    let mut runtime = node.runtime_manager.spawn_runtime().await.unwrap();
    runtime.reset(&root).await.unwrap();

    let result = runtime.evaluate_with_term(term).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let completed = new_gstring_par(completion_marker.to_owned(), vec![], false);
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
    (persistent, linear, checkpoint.root)
}

/// Build a term that resolves a vault address from a given raw-bytes
/// `addressBytes` and then makes `requests` parallel
/// `unforgeableAuthKey` requests against that address (all to the same
/// derived vault address, so they all share one cache entry).
fn auth_key_term(address_bytes_hex: &str, requests: usize, marker: &str) -> String {
    let returns: String = (0..requests)
        .map(|i| format!("keyCh{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sends: String = (0..requests)
        .map(|i| format!(r#"@SystemVault!("unforgeableAuthKey", *seed, *keyCh{i})"#))
        .collect::<Vec<_>>()
        .join(" |\n    ");
    let joins: String = (0..requests)
        .map(|i| format!("_ <- keyCh{i}"))
        .collect::<Vec<_>>()
        .join(" & ");
    format!(
        r#"
new lookup(`rho:registry:lookup`), svCh, seed, {returns} in {{
  lookup!(`rho:vault:system`, *svCh) |
  @"{address_bytes_hex}-pin"!(*seed) |
  for (@(_, SystemVault) <- svCh) {{
    {sends} |
    for ({joins}) {{
      @"{marker}"!(true)
    }}
  }}
}}
"#
    )
}

/// `unforgeableAuthKey` derives the vault address from the given
/// unforgeable seed; calling it again in a later deploy with the SAME
/// unforgeable seed must return the cached bundle instead of installing
/// a new `authKey("challenge", ...)` continuation. The `seed` channel
/// is pinned on a shared forgeable name across both deploys so the
/// second deploy targets the same vault address as the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_deploy_reuses_the_cached_unforgeable_auth_key() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();
    let genesis_root =
        Blake2b256Hash::from_bytes_prost(&proto_util::post_state_hash(&node.genesis));

    // First deploy: install the seed-pinning send and request the
    // AuthKey. This should install one persistent `authKey` continuation.
    let first_term = r#"
new lookup(`rho:registry:lookup`), svCh, seed, keyCh in {
  lookup!(`rho:vault:system`, *svCh) |
  @"issue1156-seed-pin"!(*seed) |
  for (@(_, SystemVault) <- svCh) {
    @SystemVault!("unforgeableAuthKey", *seed, *keyCh) |
    for (_ <- keyCh) {
      @"issue1156-first-done"!(true)
    }
  }
}
"#;
    let (first_persistent, _, after_first) =
        retained_continuations_from(&node, genesis_root, first_term, "issue1156-first-done").await;
    assert!(
        first_persistent >= 1,
        "the first unforgeableAuthKey request must install the authKey continuation (got {first_persistent})"
    );

    // Second deploy: read back the pinned seed from the shared channel
    // and make another AuthKey request for the same shape. With the
    // cache this must install no new continuations at all.
    let second_term = r#"
new lookup(`rho:registry:lookup`), svCh, keyCh in {
  lookup!(`rho:vault:system`, *svCh) |
  for (seed <<- @"issue1156-seed-pin") {
    for (@(_, SystemVault) <- svCh) {
      @SystemVault!("unforgeableAuthKey", *seed, *keyCh) |
      for (_ <- keyCh) {
        @"issue1156-second-done"!(true)
      }
    }
  }
}
"#;
    let (second_persistent, second_linear, _) =
        retained_continuations_from(&node, after_first, second_term, "issue1156-second-done").await;
    assert_eq!(
        (second_persistent, second_linear),
        (0, 0),
        "a cached unforgeableAuthKey must not install new continuations"
    );
}

/// Each transfer deploy makes two `AuthKey` requests for the same
/// vault address: one inside PoS's `deposit` during pre-charge, and
/// one inside the user term. With the cache those two calls share a
/// single `authKey` continuation; without the cache each would install
/// a fresh one. This test does `N` parallel requests inside one deploy
/// and asserts that each additional request past the first adds no
/// persistent continuations.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_auth_key_requests_in_one_deploy_share_one_continuation() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();
    let genesis_root =
        Blake2b256Hash::from_bytes_prost(&proto_util::post_state_hash(&node.genesis));

    let (one_request_persistent, _, _) = retained_continuations_from(
        &node,
        genesis_root.clone(),
        &auth_key_term("single", 1, "issue1156-one-done"),
        "issue1156-one-done",
    )
    .await;

    let (two_requests_persistent, _, _) = retained_continuations_from(
        &node,
        genesis_root.clone(),
        &auth_key_term("pair", 2, "issue1156-two-done"),
        "issue1156-two-done",
    )
    .await;

    let (three_requests_persistent, _, _) = retained_continuations_from(
        &node,
        genesis_root,
        &auth_key_term("triple", 3, "issue1156-three-done"),
        "issue1156-three-done",
    )
    .await;

    assert_eq!(
        two_requests_persistent, one_request_persistent,
        "two parallel AuthKey requests must install the same persistent continuations as one"
    );
    assert_eq!(
        three_requests_persistent, one_request_persistent,
        "three parallel AuthKey requests must install the same persistent continuations as one"
    );
}
