// Unit tests for PathMap zipper query methods: hasVal, atPath, pathExists —
// plus the native-descent identity suite (`native_query_identity_tests`), the
// trie-memo suite (`trie_memo_tests`), and the full-runtime result/cost suite
// (`native_query_runtime_tests`) guarding the EPathMap trie-cache +
// native-prefix-descent fix.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, EZipper, Expr, Par};
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::par_to_path;

#[cfg(test)]
mod zipper_query_tests {
    use super::*;

    fn create_test_pathmap() -> EPathMap {
        // Create PathMap with entries: ["a", "value1"], ["a", "b", "value2"], ["c", "value3"]
        let entries = vec![
            create_path_par(vec!["a".to_string()], "value1"),
            create_path_par(vec!["a".to_string(), "b".to_string()], "value2"),
            create_path_par(vec!["c".to_string()], "value3"),
        ];

        // EPathMap fix P3 (PM-2): constructor instead of a struct literal
        // (the wrapper's shadow cell is private).
        EPathMap::new(entries, vec![], false, None)
    }

    fn create_path_par(path: Vec<String>, _value: &str) -> Par {
        // In Rholang pathmaps, each Par in EPathMap.ps represents a path.
        // The path is extracted using par_to_path, and the entire Par is stored as the value.
        // The test expects to find a value at path ["a"] when we create an entry.
        // However, par_to_path treats ALL list elements as path segments.
        //
        // The test creates entries like ["a", "value1"] and expects to find a value at path ["a"].
        // But if we create EListBody(["a", "value1"]), par_to_path extracts ["a", "value1"] as the path,
        // not ["a"]. So the test structure is incorrect.
        //
        // The fix: create a Par that represents only the path segments. So for path ["a"],
        // we create EListBody(["a"]), and the value stored will be that Par. The test checks
        // if there's a value at path ["a"], which will be true.
        let path_elements = path
            .iter()
            .map(|s| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString(s.clone())),
                }])
            })
            .collect::<Vec<_>>();

        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: path_elements,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn create_path_list(path: Vec<String>) -> Par {
        let path_elements = path
            .iter()
            .map(|s| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString(s.clone())),
                }])
            })
            .collect::<Vec<_>>();

        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: path_elements,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    #[test]
    fn test_has_val_on_zipper_with_value() {
        let pathmap = create_test_pathmap();

        // Create zipper at path ["a"] which has a value
        let zipper = EZipper {
            pathmap: Some(pathmap),
            current_path: vec![b"a".to_vec()],
            is_write_zipper: false,
            locally_free: vec![],
            connective_used: false,
        };

        // Test hasVal implementation logic
        let pathmap_result =
            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(zipper.pathmap.as_ref().unwrap());
        let rholang_pathmap = pathmap_result.map;

        // Builds key from current_path segments using the same encoding as par_to_path
        let key: Vec<u8> = zipper
            .current_path
            .iter()
            .flat_map(|seg| {
                // Creates a Par from the segment bytes to encode it properly
                let par = Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString(
                        String::from_utf8(seg.clone()).unwrap(),
                    )),
                }]);
                let segments = par_to_path(&par);
                segments.into_iter().flat_map(|mut seg| {
                    seg.push(0xFF);
                    seg
                })
            })
            .collect();

        let has_val = rholang_pathmap.get(&key).is_some();
        assert!(has_val, "Expected value at path ['a']");
    }

    #[test]
    fn test_has_val_on_zipper_without_value() {
        let pathmap = create_test_pathmap();

        // Create zipper at path ["x"] which does not exist
        let zipper = EZipper {
            pathmap: Some(pathmap),
            current_path: vec![b"x".to_vec()],
            is_write_zipper: false,
            locally_free: vec![],
            connective_used: false,
        };

        let pathmap_result =
            PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(zipper.pathmap.as_ref().unwrap());
        let rholang_pathmap = pathmap_result.map;

        // Builds key from current_path segments using the same encoding as par_to_path
        let key: Vec<u8> = zipper
            .current_path
            .iter()
            .flat_map(|seg| {
                // Creates a Par from the segment bytes to encode it properly
                let par = Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString(
                        String::from_utf8(seg.clone()).unwrap(),
                    )),
                }]);
                let segments = par_to_path(&par);
                segments.into_iter().flat_map(|mut seg| {
                    seg.push(0xFF);
                    seg
                })
            })
            .collect();

        let has_val = rholang_pathmap.get(&key).is_some();
        assert!(!has_val, "Expected no value at path ['x']");
    }

    #[test]
    fn test_at_path_from_zipper() {
        let pathmap = create_test_pathmap();

        // Test getting value at path ["a"]
        let pathmap_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
        let rholang_pathmap = pathmap_result.map;

        // Builds key from path ["a"] using par_to_path encoding
        let path_par = create_path_list(vec!["a".to_string()]);
        let segments = par_to_path(&path_par);
        let key: Vec<u8> = segments
            .into_iter()
            .flat_map(|mut seg| {
                seg.push(0xFF);
                seg
            })
            .collect();

        let value = rholang_pathmap.get(&key);

        assert!(value.is_some(), "Expected to find value at path ['a']");
    }

    #[test]
    fn test_at_path_nonexistent() {
        let pathmap = create_test_pathmap();

        let pathmap_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
        let rholang_pathmap = pathmap_result.map;

        let key: Vec<u8> = vec![
            b'n', b'o', b'n', b'e', b'x', b'i', b's', b't', b'e', b'n', b't', 0xFF,
        ];
        let value = rholang_pathmap.get(&key);

        assert!(value.is_none(), "Expected no value at nonexistent path");
    }

    #[test]
    fn test_path_exists_at_root() {
        let pathmap = create_test_pathmap();

        // Root exists if PathMap is not empty
        let exists = !pathmap.ps.is_empty();
        assert!(exists, "Root path should exist for non-empty PathMap");
    }

    #[test]
    fn test_path_exists_for_intermediate_node() {
        let pathmap = create_test_pathmap();

        // Path ["a"] exists because there are children under it
        let pathmap_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
        let rholang_pathmap = pathmap_result.map;

        // Builds prefix key from path ["a"] using par_to_path encoding
        let path_par = create_path_list(vec!["a".to_string()]);
        let segments = par_to_path(&path_par);
        let prefix_key: Vec<u8> = segments
            .into_iter()
            .flat_map(|mut seg| {
                seg.push(0xFF);
                seg
            })
            .collect();

        let exists = rholang_pathmap
            .iter()
            .any(|(k, _)| k.starts_with(&prefix_key));

        assert!(exists, "Path ['a'] should exist as it has children");
    }

    #[test]
    fn test_path_exists_for_nonexistent_path() {
        let pathmap = create_test_pathmap();

        let pathmap_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&pathmap);
        let rholang_pathmap = pathmap_result.map;

        let prefix_key: Vec<u8> = vec![b'z', 0xFF];
        let exists = rholang_pathmap
            .iter()
            .any(|(k, _)| k.starts_with(&prefix_key));

        assert!(!exists, "Path ['z'] should not exist");
    }

    #[test]
    fn test_path_exists_empty_pathmap() {
        let empty_pathmap = EPathMap::new(vec![], vec![], false, None);

        let exists = !empty_pathmap.ps.is_empty();
        assert!(!exists, "Empty PathMap should not have existing paths");
    }
}

/// Result-identity tests for the native trie-descent query helpers
/// (`models::rust::pathmap_native_query`) against verbatim transcriptions of
/// the retired whole-map scans they replaced in `reduce.rs`. Every comparison
/// is on full result VECTORS, so ordering is pinned exactly — including the
/// byte-lex orders the E-6a experiment locked
/// (`descendFirst`/`descendIndexedBranch`/`childCount` sort+dedup order,
/// `getSubtrie` trie-DFS collection order).
#[cfg(test)]
mod native_query_identity_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, EPathMap, Expr, Par};
    use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
    use models::rust::pathmap_integration::RholangPathMap;
    use models::rust::pathmap_native_query::{
        collect_child_segments, collect_subtrie_values, path_prefix_exists,
    };

    // ---- Oracles: the retired reduce.rs scan algorithms, verbatim ----

    /// The retired childCount/descendFirst/descendIndexedBranch/toNextSibling/
    /// toPrevSibling scan: whole-map iteration, first-segment extraction,
    /// sort()+dedup(). (With `prefix = &[]` this is also the retired
    /// childCount EPathmapBody-arm scan: `starts_with(&[])` is always true and
    /// keys are never empty.)
    fn oracle_child_segments(map: &RholangPathMap, prefix: &[u8]) -> Vec<Vec<u8>> {
        let mut children: Vec<Vec<u8>> = Vec::new();
        for (key, _) in map.iter() {
            if key.starts_with(prefix) && key.len() > prefix.len() {
                let remaining = &key[prefix.len()..];
                if let Some(pos) = remaining.iter().position(|&b| b == 0xFF) {
                    let segment = remaining[..pos].to_vec();
                    children.push(segment);
                }
            }
        }
        children.sort();
        children.dedup();
        children
    }

    /// The retired getSubtrie scan: whole-map iteration filtered by prefix, in
    /// map-iteration (trie-DFS byte-lex) order.
    fn oracle_subtrie_values(map: &RholangPathMap, prefix: &[u8]) -> Vec<Par> {
        let mut subtrie_elements = Vec::new();
        for (key, value) in map.iter() {
            if key.starts_with(prefix) {
                subtrie_elements.push(value.clone());
            }
        }
        subtrie_elements
    }

    /// The retired pathExists scan.
    fn oracle_prefix_exists(map: &RholangPathMap, key: &[u8]) -> bool {
        map.iter().any(|(k, _)| k.starts_with(key))
    }

    // ---- Fixtures ----

    fn gint_par(n: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }])
    }

    fn string_list_par(path: &[&str]) -> Par {
        let path_elements = path
            .iter()
            .map(|s| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString((*s).to_string())),
                }])
            })
            .collect::<Vec<_>>();
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: path_elements,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    /// A realistic element-built map (through the production conversion,
    /// memo included): shared prefixes, multi-segment depth, single-segment
    /// entries.
    fn element_built_map() -> RholangPathMap {
        let e_pathmap = EPathMap::new(
            vec![
                string_list_par(&["op", "site0", "left"]),
                string_list_par(&["op", "site0", "right"]),
                string_list_par(&["op", "site1"]),
                string_list_par(&["v", "c0"]),
                string_list_par(&["v", "c1", "deep"]),
                string_list_par(&["w"]),
            ],
            vec![],
            false,
            None,
        );
        PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&e_pathmap).map
    }

    /// The adversarial RAW trie exercising cases the SExpr encoding cannot
    /// produce (segments that are byte-prefixes of sibling segments; an empty
    /// segment) plus a value exactly at an interior separator boundary. The
    /// native helpers are defined at the raw-bytes level, so their identity
    /// with the scans must hold here too.
    fn raw_adversarial_map() -> RholangPathMap {
        let mut map = RholangPathMap::new();
        map.insert(b"ab\xFF".to_vec(), gint_par(1)); // value exactly at segment end
        map.insert(b"ab\xFFq\xFF".to_vec(), gint_par(2));
        map.insert(b"abc\xFFq\xFF".to_vec(), gint_par(3)); // sibling segment "abc" extends "ab"
        map.insert(b"b\xFFq\xFF".to_vec(), gint_par(4));
        map.insert(b"\xFFz\xFF".to_vec(), gint_par(5)); // EMPTY first segment
        map
    }

    // ---- collect_child_segments ----

    #[test]
    fn child_segments_match_scan_on_element_built_map() {
        let map = element_built_map();
        let seg_op: Vec<u8> = {
            // Flattened one-segment prefix for "op" (encoded segment + 0xFF),
            // recovered from the map itself so the test does not re-implement
            // the encoder: it is the shortest child segment at root that leads
            // to three grandchildren.
            let top = oracle_child_segments(&map, &[]);
            let mut k = top
                .into_iter()
                .find(|seg| {
                    let mut p = seg.clone();
                    p.push(0xFF);
                    oracle_child_segments(&map, &p).len() == 2
                })
                .expect("an 'op'-like top segment with two children must exist");
            k.push(0xFF);
            k
        };

        for prefix in [&[][..], &seg_op[..]] {
            let expected = oracle_child_segments(&map, prefix);
            let actual = collect_child_segments(&map, prefix, None);
            assert_eq!(
                actual, expected,
                "child segments (values AND order) must match the retired scan at prefix {:?}",
                prefix
            );
            assert!(!expected.is_empty(), "fixture must exercise a non-trivial prefix");
        }

        // Nonexistent prefix → empty in both forms.
        let bogus = b"\x01bogus\xFF".to_vec();
        assert_eq!(
            collect_child_segments(&map, &bogus, None),
            oracle_child_segments(&map, &bogus)
        );
        assert!(collect_child_segments(&map, &bogus, None).is_empty());
    }

    #[test]
    fn child_segments_match_scan_on_raw_adversarial_map() {
        let map = raw_adversarial_map();

        // Root: the scan's sort() puts "" first and "ab" BEFORE "abc" (raw
        // byte-lex on segments), even though the separator byte 0xFF sorts
        // AFTER 'c' inside the trie — the exact order-inversion hazard the
        // terminator-first DFS exists to neutralize.
        let expected_root = vec![
            b"".to_vec(),
            b"ab".to_vec(),
            b"abc".to_vec(),
            b"b".to_vec(),
        ];
        assert_eq!(oracle_child_segments(&map, &[]), expected_root);
        assert_eq!(collect_child_segments(&map, &[], None), expected_root);

        // Interior prefix.
        assert_eq!(
            collect_child_segments(&map, b"ab\xFF", None),
            oracle_child_segments(&map, b"ab\xFF")
        );
        assert_eq!(collect_child_segments(&map, b"ab\xFF", None), vec![b"q".to_vec()]);

        // A leaf path (value present, no children) → no child segments.
        assert_eq!(
            collect_child_segments(&map, b"b\xFFq\xFF", None),
            oracle_child_segments(&map, b"b\xFFq\xFF")
        );
        assert!(collect_child_segments(&map, b"b\xFFq\xFF", None).is_empty());
    }

    #[test]
    fn child_segments_limit_is_a_prefix_of_the_full_order() {
        let map = raw_adversarial_map();
        let full = collect_child_segments(&map, &[], None);
        assert_eq!(full.len(), 4);
        for k in 0..=full.len() + 1 {
            let limited = collect_child_segments(&map, &[], Some(k));
            assert_eq!(
                limited,
                full[..k.min(full.len())].to_vec(),
                "limit={} must truncate the full sorted order",
                k
            );
        }
    }

    // ---- collect_subtrie_values ----

    #[test]
    fn subtrie_values_match_scan_including_order() {
        let element_map = element_built_map();
        let raw_map = raw_adversarial_map();

        // Element-built map: whole map (empty prefix) and every top-level
        // child subtrie.
        let mut prefixes: Vec<Vec<u8>> = vec![Vec::new()];
        for seg in oracle_child_segments(&element_map, &[]) {
            let mut p = seg;
            p.push(0xFF);
            prefixes.push(p);
        }
        for prefix in &prefixes {
            assert_eq!(
                collect_subtrie_values(&element_map, prefix),
                oracle_subtrie_values(&element_map, prefix),
                "subtrie values (and order) must match the retired scan at prefix {:?}",
                prefix
            );
        }

        // Raw map: the prefix with a value AT the prefix itself ("ab\xFF")
        // must yield that value FIRST, exactly as the scan encountered it.
        let at_prefix = collect_subtrie_values(&raw_map, b"ab\xFF");
        assert_eq!(at_prefix, oracle_subtrie_values(&raw_map, b"ab\xFF"));
        assert_eq!(at_prefix, vec![gint_par(1), gint_par(2)]);

        // Empty-segment branch.
        assert_eq!(
            collect_subtrie_values(&raw_map, b"\xFF"),
            oracle_subtrie_values(&raw_map, b"\xFF")
        );

        // Nonexistent prefix → empty.
        assert_eq!(
            collect_subtrie_values(&raw_map, b"zz\xFF"),
            oracle_subtrie_values(&raw_map, b"zz\xFF")
        );
        assert!(collect_subtrie_values(&raw_map, b"zz\xFF").is_empty());
    }

    // ---- path_prefix_exists ----

    #[test]
    fn path_prefix_exists_matches_scan() {
        let map = raw_adversarial_map();
        let cases: Vec<(&[u8], bool)> = vec![
            (b"ab", true),            // partial segment
            (b"abc", true),           // partial sibling segment
            (b"abd", false),          // diverges inside a segment
            (b"ab\xFF", true),        // exact key
            (b"ab\xFFq\xFF", true),   // exact deeper key
            (b"ab\xFFq\xFFx", false), // beyond a leaf
            (b"\xFF", true),          // empty-segment branch
            (b"c", false),            // absent top byte
        ];
        for (key, expected) in cases {
            assert_eq!(
                oracle_prefix_exists(&map, key),
                expected,
                "oracle disagrees with the pinned expectation for {:?}",
                key
            );
            assert_eq!(
                path_prefix_exists(&map, key),
                expected,
                "native path_prefix_exists must match the retired scan for {:?}",
                key
            );
        }

        let element_map = element_built_map();
        for seg in oracle_child_segments(&element_map, &[]) {
            let mut p = seg;
            p.push(0xFF);
            assert!(path_prefix_exists(&element_map, &p));
            assert!(oracle_prefix_exists(&element_map, &p));
        }
    }
}

/// Tests for the bounded process-wide EPathMap→trie memo: hits must be
/// value-indistinguishable from misses, byte-different inputs must never
/// alias, and caller mutation must never corrupt the cached trie (the
/// pathmap crate's copy-on-write clone).
#[cfg(test)]
mod trie_memo_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, EPathMap, Expr, Par};
    use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
    use models::rust::pathmap_integration::RholangPathMap;

    fn string_list_par(path: &[&str]) -> Par {
        let path_elements = path
            .iter()
            .map(|s| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString((*s).to_string())),
                }])
            })
            .collect::<Vec<_>>();
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: path_elements,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn e_pathmap(paths: &[&[&str]]) -> EPathMap {
        EPathMap::new(
            // L2: turbofish — `new` takes `impl Into<SharedPars>`.
            paths.iter().map(|p| string_list_par(p)).collect::<Vec<Par>>(),
            vec![],
            false,
            None,
        )
    }

    fn full_stream(map: &RholangPathMap) -> Vec<(Vec<u8>, Par)> {
        map.iter().map(|(k, v)| (k, v.clone())).collect()
    }

    #[test]
    fn repeated_conversion_is_value_identical() {
        // Two separately-allocated but byte-identical EPathMaps: whichever
        // call misses and whichever hits, the results must be
        // indistinguishable in the value domain.
        let a = e_pathmap(&[
            &["memoA", "x"],
            &["memoA", "y", "deep"],
            &["memoB"],
        ]);
        let b = a.clone();

        let first = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&a);
        let second = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&b);
        let third = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&a);

        let reference = full_stream(&first.map);
        assert_eq!(full_stream(&second.map), reference);
        assert_eq!(full_stream(&third.map), reference);
        assert_eq!(second.connective_used, first.connective_used);
        assert_eq!(third.connective_used, first.connective_used);
        assert_eq!(second.locally_free, first.locally_free);
        assert_eq!(third.locally_free, first.locally_free);
    }

    #[test]
    fn byte_different_maps_never_alias() {
        let small = e_pathmap(&[&["aliasProbe", "x"]]);
        let large = e_pathmap(&[&["aliasProbe", "x"], &["aliasProbe", "y"]]);

        let small_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&small);
        let large_result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&large);

        assert_eq!(full_stream(&small_result.map).len(), 1);
        assert_eq!(full_stream(&large_result.map).len(), 2);
    }

    #[test]
    fn caller_mutation_cannot_corrupt_the_cache() {
        let source = e_pathmap(&[&["cowProbe", "x"], &["cowProbe", "y"]]);

        let mut mutated = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source);
        let sentinel_key = b"zz-sentinel\xFF".to_vec();
        mutated.map.insert(
            sentinel_key.clone(),
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(99)),
            }]),
        );
        assert!(mutated.map.get(&sentinel_key).is_some());

        // A fresh conversion of the same EPathMap must NOT see the caller's
        // mutation: the memoized trie is protected by copy-on-write.
        let fresh = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source);
        assert!(
            fresh.map.get(&sentinel_key).is_none(),
            "caller mutation leaked into the memoized trie"
        );
        assert_eq!(full_stream(&fresh.map).len(), 2);
    }
}

/// Full-runtime tests: the rewired methods evaluated end-to-end through the
/// interpreter, with results read back from the tuplespace, plus the
/// cost-charge invariance probe (memoization must not change charged costs).
#[cfg(test)]
mod native_query_runtime_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{Expr, Par};
    use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
    use rholang::rust::interpreter::test_utils::resources::with_runtime;

    async fn eval_ok(runtime: &mut RhoRuntimeImpl, term: &str) {
        let res = runtime
            .evaluate_with_term(term)
            .await
            .expect("evaluation must not error");
        assert!(
            res.errors.is_empty(),
            "evaluation raised interpreter errors: {:?}",
            res.errors
        );
    }

    async fn read_single_expr(runtime: &RhoRuntimeImpl, channel_name: &str) -> ExprInstance {
        let channel = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(channel_name.to_string())),
        }]);
        let data = runtime.get_data(&channel).await;
        assert_eq!(
            data.len(),
            1,
            "expected exactly one datum at @\"{}\"",
            channel_name
        );
        let pars = &data[0].a.pars;
        assert_eq!(pars.len(), 1, "expected a single Par at @\"{}\"", channel_name);
        pars[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.clone())
            .unwrap_or_else(|| panic!("no expr at @\"{}\"", channel_name))
    }

    async fn assert_bool(runtime: &RhoRuntimeImpl, channel_name: &str) {
        match read_single_expr(runtime, channel_name).await {
            ExprInstance::GBool(true) => {}
            other => panic!("@\"{}\" expected GBool(true), got {:?}", channel_name, other),
        }
    }

    async fn assert_int(runtime: &RhoRuntimeImpl, channel_name: &str, expected: i64) {
        match read_single_expr(runtime, channel_name).await {
            ExprInstance::GInt(n) if n == expected => {}
            other => panic!(
                "@\"{}\" expected GInt({}), got {:?}",
                channel_name, expected, other
            ),
        }
    }

    /// The representative map used by every runtime assertion below:
    /// three top-level children in byte-lex order "a" < "b" < "c";
    /// "a" has two leaves x < y; "b" is itself a leaf; "c" has one leaf z.
    const MAP: &str = r#"{| ["a", "x"], ["a", "y"], ["b"], ["c", "z"] |}"#;

    fn query_program() -> String {
        format!(
            r#"
            @"countRoot"!( {m}.childCount() ) |
            @"countRootZipper"!( {m}.readZipper().childCount() ) |
            @"countA"!( {m}.readZipperAt(["a"]).childCount() ) |
            @"firstLeaf"!( {m}.readZipperAt(["a"]).descendFirst().getLeaf() == ["a", "x"] ) |
            @"idx0"!( {m}.readZipper().descendIndexedBranch(0).descendFirst().getLeaf() == ["a", "x"] ) |
            @"idx1"!( {m}.readZipper().descendIndexedBranch(1).getLeaf() == ["b"] ) |
            @"idx2"!( {m}.readZipper().descendIndexedBranch(2).descendFirst().getLeaf() == ["c", "z"] ) |
            @"idxOut"!( {m}.readZipper().descendIndexedBranch(3) == Nil ) |
            @"existsA"!( {m}.readZipperAt(["a"]).pathExists() ) |
            @"existsAX"!( {m}.readZipperAt(["a", "x"]).pathExists() ) |
            @"existsNot"!( {m}.readZipperAt(["zz"]).pathExists() == false ) |
            @"subtrieCount"!( {m}.readZipperAt(["a"]).getSubtrie().childCount() ) |
            @"subtrieHasAX"!( {m}.readZipperAt(["a"]).getSubtrie().readZipperAt(["a", "x"]).pathExists() ) |
            @"subtrieDropsB"!( {m}.readZipperAt(["a"]).getSubtrie().readZipperAt(["b"]).pathExists() == false ) |
            @"nextSibling"!( {m}.readZipperAt(["a"]).toNextSibling() == {m}.readZipperAt(["b"]) ) |
            @"nextSiblingLast"!( {m}.readZipperAt(["c"]).toNextSibling() == Nil ) |
            @"prevSibling"!( {m}.readZipperAt(["b"]).toPrevSibling() == {m}.readZipperAt(["a"]) ) |
            @"prevSiblingFirst"!( {m}.readZipperAt(["a"]).toPrevSibling() == Nil )
            "#,
            m = MAP
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_query_methods_end_to_end() {
        with_runtime("native-query-e2e-", |mut runtime| async move {
            eval_ok(&mut runtime, &query_program()).await;

            assert_int(&runtime, "countRoot", 3).await;
            assert_int(&runtime, "countRootZipper", 3).await;
            assert_int(&runtime, "countA", 2).await;
            assert_bool(&runtime, "firstLeaf").await;
            assert_bool(&runtime, "idx0").await;
            assert_bool(&runtime, "idx1").await;
            assert_bool(&runtime, "idx2").await;
            assert_bool(&runtime, "idxOut").await;
            assert_bool(&runtime, "existsA").await;
            assert_bool(&runtime, "existsAX").await;
            assert_bool(&runtime, "existsNot").await;
            assert_int(&runtime, "subtrieCount", 1).await;
            assert_bool(&runtime, "subtrieHasAX").await;
            assert_bool(&runtime, "subtrieDropsB").await;
            assert_bool(&runtime, "nextSibling").await;
            assert_bool(&runtime, "nextSiblingLast").await;
            assert_bool(&runtime, "prevSibling").await;
            assert_bool(&runtime, "prevSiblingFirst").await;
        })
        .await
    }

    /// Cost-charge invariance: the SAME query program on two fresh runtimes
    /// must charge the SAME total cost. The process-wide trie memo is in a
    /// different warmth state for the two evaluations (the first run populates
    /// entries the second run hits), so any cost leak from memoization would
    /// surface as a differential here. The memo removes UNCHARGED host work
    /// only; every reserve_* call in the method layer is untouched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn repeated_query_costs_are_identical() {
        let program = query_program();

        let cost_cold = with_runtime("native-query-cost-a-", {
            let program = program.clone();
            move |mut runtime| async move {
                let res = runtime
                    .evaluate_with_term(&program)
                    .await
                    .expect("evaluation must not error");
                assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
                res.cost.value
            }
        })
        .await;

        let cost_warm = with_runtime("native-query-cost-b-", {
            let program = program.clone();
            move |mut runtime| async move {
                let res = runtime
                    .evaluate_with_term(&program)
                    .await
                    .expect("evaluation must not error");
                assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
                res.cost.value
            }
        })
        .await;

        assert_eq!(
            cost_cold, cost_warm,
            "trie memoization must not change charged costs"
        );
    }
}
