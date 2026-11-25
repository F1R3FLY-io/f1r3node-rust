use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::{new_gbool_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn channel(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

async fn assert_all_true(cases: &[(&str, &str)]) {
    let sends = cases
        .iter()
        .map(|(name, expr)| format!(r#"@"{name}"!({expr})"#))
        .collect::<Vec<_>>()
        .join(" | ");
    let term = format!(r#"@"p"!(1 | 2) | for (@p <- @"p") {{ {sends} }}"#);

    let mut manager = InMemoryStoreManager::new();
    let store = manager.r_space_stores().await.unwrap();
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let result = runtime
        .evaluate(
            &term,
            Cost::unsafe_max(),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(&[0]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    for (name, _) in cases {
        let data: Vec<Par> = runtime
            .get_data(&channel(name))
            .await
            .into_iter()
            .flat_map(|datum| datum.a.pars)
            .collect();
        assert_eq!(data, vec![new_gbool_par(true, Vec::new(), false)], "{name}");
    }
}

// Sending a collection or comparing it with `==` re-sorts it; `toList` shows
// the order it was built in.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_constructed_collection_is_canonical() {
    assert_all_true(&[
        ("mapSet", r#"{"b": 1, "d": 2}.set("c", 3).set("a", 4).set("e", 5).set("c", 6).toList() == [("a", 4), ("b", 1), ("c", 6), ("d", 2), ("e", 5)]"#),
        ("mapDelete", r#"{"b": 1, "a": 2, "c": 3}.delete("b").toList() == [("a", 2), ("c", 3)]"#),
        ("mapUnion", r#"{"b": 1, "d": 2}.union({"a": 3, "c": 4, "d": 5}).toList() == [("a", 3), ("b", 1), ("c", 4), ("d", 5)]"#),
        ("mapAppend", r#"({"b": 1, "d": 2} ++ {"a": 3, "d": 5}).toList() == [("a", 3), ("b", 1), ("d", 5)]"#),
        ("mapDiff", r#"{"b": 1, "a": 2, "c": 3}.diff({"a": 0, "e": 0}).toList() == [("b", 1), ("c", 3)]"#),
        ("mapToMap", r#"[("b", 1), ("a", 2), ("b", 3)].toMap().toList() == [("a", 2), ("b", 3)]"#),
        ("mapToSet", r#"{"b": 1, "a": 2, 3: "c"}.toSet().toList() == [(3, "c"), ("a", 2), ("b", 1)]"#),
        ("mapKeys", r#"{"b": 1, "a": 2, 3: "c"}.keys().toList() == [3, "a", "b"]"#),
        ("mapLiteral", r#"{"b": 1, "a": 2, 3: "c"}.toList() == [(3, "c"), ("a", 2), ("b", 1)]"#),
        ("setAdd", r#"Set("b", "d").add("c").add("a").add("e").add(1).add("c").toList() == [1, "a", "b", "c", "d", "e"]"#),
        ("setDelete", r#"Set("b", "a", "c", 1).delete("a").toList() == [1, "b", "c"]"#),
        ("setUnion", r#"Set("b", "d").union(Set("a", "c", "d", 1)).toList() == [1, "a", "b", "c", "d"]"#),
        ("setAppend", r#"(Set("b", "d") ++ Set("a", "d")).toList() == ["a", "b", "d"]"#),
        ("setDiff", r#"Set("b", "a", "c").diff(Set("a", "e")).toList() == ["b", "c"]"#),
        ("setRemove", r#"(Set("b", "a", "c") -- Set("a", "e")).toList() == ["b", "c"]"#),
        ("setLiteral", r#"Set("b", "a", 1).toList() == [1, "a", "b"]"#),
        ("listToSet", r#"["b", "a", "b", 1].toSet().toList() == [1, "a", "b"]"#),
    ])
    .await;
}

// `(p | 3, 0)` and `(1 | 2 | 3, 0)` are one key written in different orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keys_that_sort_alike_are_one_key() {
    assert_all_true(&[
        ("mapLiteralOneEntry", r#"{(p | 3, 0): "x", (1 | 2 | 3, 0): "y"}.size() == 1"#),
        ("mapKeysOne", r#"{(p | 3, 0): "x", (1 | 2 | 3, 0): "y"}.keys().size() == 1"#),
        ("mapSetReplaces", r#"{(p | 3, 0): "x", (1 | 2 | 3, 0): "y"}.set((3 | 2 | 1, 0), "z").get((1 | 2 | 3, 0)) == "z""#),
        ("mapDeleteRemoves", r#"{(p | 3, 0): "x", (1 | 2 | 3, 0): "y"}.delete((3 | 2 | 1, 0)).size() == 0"#),
        ("setLiteralOneElement", r#"Set((p | 3, 0), (1 | 2 | 3, 0)).size() == 1"#),
        ("setDeleteRemoves", r#"Set((p | 3, 0), (1 | 2 | 3, 0)).delete((3 | 2 | 1, 0)).size() == 0"#),
    ])
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lookups_find_keys_whatever_their_written_order() {
    assert_all_true(&[
        ("mapGet", r#"{p: "a"}.get(1 | 2) == "a""#),
        ("mapGetReordered", r#"{p: "a"}.get(2 | 1) == "a""#),
        (
            "mapContains",
            r#"{p: "a"}.contains(1 | 2) and {p: "a"}.contains(2 | 1)"#,
        ),
        ("mapGetOrElse", r#"{p: "a"}.getOrElse(2 | 1, "z") == "a""#),
        (
            "mapKeyIsMap",
            r#"{{"b": 1, "a": 2}: "x"}.get({"a": 2, "b": 1}) == "x""#,
        ),
        (
            "mapOneEntry",
            r#"{"b": 0}.set(1 | 2, "a").set(2 | 1, "c").set(p, "d").size() == 2"#,
        ),
        (
            "mapLastValue",
            r#"{"b": 0}.set(1 | 2, "a").set(2 | 1, "c").get(p) == "c""#,
        ),
        (
            "mixedMapOneEntry",
            r#"[(p, 1), (1 | 2, 2)].toMap().size() == 1"#,
        ),
        (
            "mixedMapOneKey",
            r#"[(p, 1), (1 | 2, 2)].toMap().keys().size() == 1"#,
        ),
        (
            "setContains",
            r#"Set(p).contains(1 | 2) and Set(p).contains(2 | 1)"#,
        ),
        (
            "setOneElement",
            r#"Set(1 | 2, 2 | 1, p).toList().length() == 1"#,
        ),
        ("setOneSize", r#"Set(1 | 2, 2 | 1, p).size() == 1"#),
        (
            "setDeleteReordered",
            r#"Set(p, "a").delete(2 | 1) == Set("a")"#,
        ),
        (
            "mapDeleteReordered",
            r#"{p: 1, "a": 2}.delete(2 | 1) == {"a": 2}"#,
        ),
    ])
    .await;
}
