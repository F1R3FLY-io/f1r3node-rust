use proptest::prelude::*;
use rholang::rust::interpreter::rho_type::RhoList;

use super::*;

async fn map_query(manager: &RuntimeManager, state: &StateHash, depth: u8, body: &str) -> Vec<Par> {
    let source = format!(
        r#"new return, rl(`rho:registry:lookup`), treeCh, mapCh in {{
          rl!(`rho:lang:treeHashMap`, *treeCh) |
          for (tree <- treeCh) {{
            tree!("init", {depth}, *mapCh) |
            for (@map <- mapCh) {{ {body} }}
          }}
        }}"#
    );
    let (values, _) = manager
        .play_exploratory_deploy(source, state, None)
        .await
        .unwrap();
    assert_eq!(
        values.len(),
        1,
        "the map operation must return exactly one result"
    );
    RhoList::unapply(&values[0]).unwrap()
}

fn increment_body(operations: &[u8]) -> String {
    let acks = (0..operations.len())
        .map(|i| format!("ack{i}"))
        .collect::<Vec<_>>();
    let sends = operations
        .iter()
        .enumerate()
        .map(|(i, key)| format!("tree!(\"updateOrInsert\", map, {key}, *update, *ack{i})"))
        .collect::<Vec<_>>()
        .join(" | ");
    let waits = acks
        .iter()
        .map(|ack| format!("_ <- {ack}"))
        .collect::<Vec<_>>()
        .join(" & ");
    let queries = (0..4)
        .map(|key| {
            format!(
                r#"new result in {{
          tree!("get", map, {key}, *result) |
          for (@value <- result) {{
            if (value == Nil) {{ r{key}!(0) }} else {{ r{key}!(value) }}
          }}
        }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        r#"new update, {acks}, r0, r1, r2, r3 in {{
          contract update(@value, reply) = {{
            if (value == Nil) {{ reply!(1) }} else {{ reply!(value + 1) }}
          }} |
          {sends} |
          for ({waits}) {{
            {queries} |
            for (@v0 <- r0 & @v1 <- r1 & @v2 <- r2 & @v3 <- r3) {{
              return!([v0, v1, v2, v3])
            }}
          }}
        }}"#,
        acks = acks.join(", "),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn atomic_trie_upsert_initializes_once_and_preserves_nil_and_legacy_operations() {
    with_runtime_manager(|manager, _, genesis| async move {
        let state = genesis.body.state.post_state_hash;
        for depth in [0, 1, 3] {
            let values =
                map_query(&manager, &state, depth, &increment_body(&[0, 0, 1, 2, 1])).await;
            assert_eq!(
                values
                    .iter()
                    .map(|p| RhoNumber::unapply(p).unwrap())
                    .collect::<Vec<_>>(),
                vec![2, 2, 1, 0]
            );
        }
        let values = map_query(
            &manager,
            &state,
            0,
            r#"
          new setAck, updateAck, nilAck, finalAck, keepCh, nilCh, presentCh, legacyCh,
              nilUpdate, legacyUpdate, replace in {
            contract nilUpdate(@value, reply) = { reply!(Nil) } |
            contract legacyUpdate(@value, reply) = { reply!(value + 1) } |
            contract replace(@value, reply) = {
              if (value == Nil) { reply!(7) } else { reply!(-1) }
            } |
            tree!("set", map, "keep", 9, *setAck) |
            for (_ <- setAck) {
              tree!("updateOrInsert", map, "nil", *nilUpdate, *nilAck) |
              for (_ <- nilAck) {
                tree!("contains", map, "nil", *presentCh) |
                tree!("get", map, "nil", *nilCh) |
                for (@present <- presentCh & @nil <- nilCh) {
                  tree!("updateOrInsert", map, "nil", *replace, *updateAck) |
                  for (_ <- updateAck) {
                    tree!("update", map, "nil", *legacyUpdate, *finalAck) |
                    for (_ <- finalAck) {
                      tree!("get", map, "keep", *keepCh) |
                      tree!("get", map, "nil", *legacyCh) |
                      for (@keep <- keepCh & @legacy <- legacyCh) {
                        return!([present, nil == Nil, keep == 9, legacy == 8])
                      }
                    }
                  }
                }
              }
            }
          }
        "#,
        )
        .await;
        assert_eq!(
            values.iter().map(RhoBoolean::unapply).collect::<Vec<_>>(),
            vec![Some(true); 4]
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn atomic_trie_upsert_holds_the_leaf_until_the_callback_returns() {
    with_runtime_manager(|manager, _, genesis| async move {
        let mut runtime = manager.spawn_runtime().await;
        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(
                &genesis.body.state.post_state_hash,
            ))
            .await
            .unwrap();
        let setup = r#"
          new rl(`rho:registry:lookup`), treeCh, mapCh, first, second, started, ack1, ack2 in {
            rl!(`rho:lang:treeHashMap`, *treeCh) |
            for (tree <- treeCh) {
              tree!("init", 0, *mapCh) |
              for (@map <- mapCh) {
                contract first(@value, reply) = {
                  @"trie-first-entered"!(value == Nil) |
                  started!(Nil) |
                  for (_ <- @"trie-release") { reply!(1) }
                } |
                contract second(@value, reply) = {
                  @"trie-second-entered"!(value) |
                  reply!(value + 1)
                } |
                tree!("updateOrInsert", map, "key", *first, *ack1) |
                for (_ <- started) {
                  tree!("updateOrInsert", map, "key", *second, *ack2)
                } |
                for (_ <- ack1 & _ <- ack2) {
                  tree!("get", map, "key", "trie-final")
                }
              }
            }
          }
        "#;
        assert!(runtime
            .evaluate_with_term(setup)
            .await
            .unwrap()
            .errors
            .is_empty());
        let held = runtime.create_checkpoint().await.root.to_bytes_prost();
        assert_eq!(
            manager
                .get_data(
                    held.clone(),
                    &RhoString::create_par("trie-first-entered".to_string())
                )
                .await
                .unwrap(),
            vec![RhoBoolean::create_par(true)]
        );
        assert!(manager
            .get_data(
                held,
                &RhoString::create_par("trie-second-entered".to_string())
            )
            .await
            .unwrap()
            .is_empty());
        assert!(runtime
            .evaluate_with_term(r#"@"trie-release"!(Nil)"#)
            .await
            .unwrap()
            .errors
            .is_empty());
        let finished = runtime.create_checkpoint().await.root.to_bytes_prost();
        assert_eq!(
            manager
                .get_data(
                    finished.clone(),
                    &RhoString::create_par("trie-second-entered".to_string())
                )
                .await
                .unwrap(),
            vec![RhoNumber::create_par(1)]
        );
        assert_eq!(
            manager
                .get_data(finished, &RhoString::create_par("trie-final".to_string()))
                .await
                .unwrap(),
            vec![RhoNumber::create_par(2)]
        );
    })
    .await
    .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 8, max_shrink_iters: 32, ..ProptestConfig::default() })]

    #[test]
    fn atomic_trie_upsert_generated_parallel_updates_preserve_every_key(
        operations in prop::collection::vec(0_u8..4, 1..9), depth in 0_u8..4,
    ) {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(
            with_runtime_manager(|manager, _, genesis| async move {
                let values = map_query(&manager, &genesis.body.state.post_state_hash, depth, &increment_body(&operations)).await;
                let mut expected = vec![0_i64; 4];
                for key in operations { expected[usize::from(key)] += 1; }
                assert_eq!(values.iter().map(|p| RhoNumber::unapply(p).unwrap()).collect::<Vec<_>>(), expected);
            })
        ).unwrap();
    }
}
