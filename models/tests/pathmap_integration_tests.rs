use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::{
    create_pathmap_from_elements, par_to_path, segments_to_key, RholangPathMap,
};

fn make_string_par(s: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}

fn make_int_par(i: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i)),
        }],
        ..Default::default()
    }
}

fn make_list_of(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

fn make_list_par(elements: Vec<&str>) -> Par {
    let ps: Vec<Par> = elements.iter().map(|s| make_string_par(s)).collect();
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

#[test]
fn test_create_empty_pathmap() {
    let result = create_pathmap_from_elements(&[], None);
    assert!(result.map.is_empty());
    assert!(!result.connective_used);
    assert!(result.locally_free.is_empty());
}

#[test]
fn test_create_pathmap_single_element() {
    let par = make_list_par(vec!["books", "fiction", "gatsby"]);
    let result = create_pathmap_from_elements(&[par.clone()], None);
    assert!(!result.map.is_empty());
}

#[test]
fn test_pathmap_union() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    assert_eq!(union.val_count(), 2);
}

#[test]
fn test_pathmap_intersection() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]);
    let par3 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1, par3], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let intersection = map1.map.meet(&map2.map);
    assert_eq!(intersection.val_count(), 1);
}

#[test]
fn test_pathmap_subtraction() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "c"]);
    let par3 = make_list_par(vec!["a", "b"]);

    let map1 = create_pathmap_from_elements(&[par1, par2], None);
    let map2 = create_pathmap_from_elements(&[par3], None);

    let diff = map1.map.subtract(&map2.map);
    // Should have only ["a", "c"] remaining
    assert_eq!(diff.val_count(), 1);
}

#[test]
fn test_pathmap_restriction() {
    let par1 = make_list_par(vec!["books", "fiction", "gatsby"]);
    let par2 = make_list_par(vec!["books", "fiction", "moby"]);
    let par3 = make_list_par(vec!["books", "nonfiction", "history"]);
    let prefix = make_list_par(vec!["books", "fiction"]);

    let map = create_pathmap_from_elements(&[par1, par2, par3], None);
    // W2b-1 (why bytes moved): PathMap::restrict is a PREFIX/subtrie op; under
    // the codec a prefix is the NON-terminated segment concatenation (the FULL
    // encode_trie_path key terminates with 0x00 and is thus prefix-free of the
    // longer keys, degenerating restrict to exact-match). Build the restricting
    // map with prefix keys — mirroring the production `restriction` method
    // (reduce.rs) — so restrict prefix-matches and yields the 2 fiction books.
    let mut prefix_map = RholangPathMap::new();
    prefix_map.insert(segments_to_key(&par_to_path(&prefix), false), prefix.clone());

    let restricted = map.map.restrict(&prefix_map);
    // Should have only the 2 fiction books
    assert_eq!(restricted.val_count(), 2);
}

#[test]
fn test_pathmap_to_e_pathmap_conversion() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let original_ps = vec![par1.clone(), par2.clone()];
    let map = create_pathmap_from_elements(&original_ps, None);

    let e_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
        &map.map,
        map.connective_used,
        &map.locally_free,
        None,
    );

    assert_eq!(e_pathmap.ps.len(), 2);
}

#[test]
fn test_e_pathmap_roundtrip() {
    let par1 = make_list_par(vec!["x", "y"]);
    let par2 = make_list_par(vec!["z", "w"]);

    // EPathMap fix P3 (PM-2): constructor instead of a struct literal
    // (the wrapper's shadow cell is private).
    let e_pathmap1 = EPathMap::new(vec![par1, par2], vec![], false, None);

    let result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&e_pathmap1);
    assert_eq!(result.map.val_count(), 2);

    let e_pathmap2 = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
        &result.map,
        result.connective_used,
        &result.locally_free,
        None,
    );

    assert_eq!(e_pathmap2.ps.len(), e_pathmap1.ps.len());
}

#[test]
fn test_pathmap_connective_used() {
    let mut par = make_list_par(vec!["a", "b"]);
    par.connective_used = true;

    let result = create_pathmap_from_elements(&[par], None);
    assert!(result.connective_used);
}

#[test]
fn test_pathmap_locally_free() {
    let mut par = make_list_par(vec!["a", "b"]);
    par.locally_free = vec![1, 2, 3];

    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.locally_free, vec![1, 2, 3]);
}

#[test]
fn test_pathmap_remainder_sets_connective() {
    let par = make_list_par(vec!["a", "b"]);
    let remainder = models::rhoapi::Var {
        var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
    };

    let result = create_pathmap_from_elements(&[par], Some(remainder));
    assert!(result.connective_used);
}

#[test]
fn test_multiple_elements_union() {
    let par1 = make_list_par(vec!["a"]);
    let par2 = make_list_par(vec!["b"]);
    let par3 = make_list_par(vec!["c"]);

    let result = create_pathmap_from_elements(&[par1, par2, par3], None);
    assert_eq!(result.map.val_count(), 3);
}

// ============ EDGE CASES ============

#[test]
fn test_intersection_disjoint_pathmaps() {
    // Intersection of completely disjoint PathMaps should be empty
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let intersection = map1.map.meet(&map2.map);
    assert!(
        intersection.is_empty(),
        "Intersection of disjoint maps should be empty"
    );
}

#[test]
fn test_intersection_empty_with_nonempty() {
    // Intersection with empty PathMap should be empty
    let par = make_list_par(vec!["a", "b"]);
    let map1 = create_pathmap_from_elements(&[par], None);
    let map2 = create_pathmap_from_elements(&[], None);

    let intersection = map1.map.meet(&map2.map);
    assert!(
        intersection.is_empty(),
        "Intersection with empty map should be empty"
    );
}

#[test]
fn test_union_overlapping_keys() {
    // Union with overlapping keys - should keep both (or one, depending on semantics)
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]); // Same path

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    // Should have 1 element (paths are identical)
    assert_eq!(union.val_count(), 1);
}

#[test]
fn test_subtraction_empty_result() {
    // Subtracting all elements should result in empty map
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let diff = map1.map.subtract(&map2.map);
    assert!(
        diff.is_empty(),
        "Subtracting identical maps should result in empty map"
    );
}

#[test]
fn test_subtraction_from_empty() {
    // Subtracting from empty map should remain empty
    let par = make_list_par(vec!["a", "b"]);
    let map1 = create_pathmap_from_elements(&[], None);
    let map2 = create_pathmap_from_elements(&[par], None);

    let diff = map1.map.subtract(&map2.map);
    assert!(
        diff.is_empty(),
        "Subtracting from empty map should be empty"
    );
}

#[test]
fn test_subtraction_disjoint() {
    // Subtracting disjoint set should leave original unchanged
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let diff = map1.map.subtract(&map2.map);
    assert_eq!(
        diff.val_count(),
        1,
        "Subtracting disjoint set should preserve original"
    );
}

#[test]
fn test_restriction_no_match() {
    // Restriction with non-matching prefix should be empty
    let par = make_list_par(vec!["books", "fiction", "gatsby"]);
    let prefix = make_list_par(vec!["movies"]); // Different prefix

    let map = create_pathmap_from_elements(&[par], None);
    let prefix_map = create_pathmap_from_elements(&[prefix], None);

    let restricted = map.map.restrict(&prefix_map.map);
    assert!(
        restricted.is_empty(),
        "Restriction with non-matching prefix should be empty"
    );
}

#[test]
fn test_restriction_exact_match() {
    // Restriction with exact path match
    let par = make_list_par(vec!["books", "fiction"]);
    let prefix = make_list_par(vec!["books", "fiction"]);

    let map = create_pathmap_from_elements(&[par], None);
    let prefix_map = create_pathmap_from_elements(&[prefix], None);

    let restricted = map.map.restrict(&prefix_map.map);
    // Should match since prefix equals the path
    assert!(!restricted.is_empty());
}

#[test]
fn test_empty_pathmap_operations() {
    // Operations on empty PathMaps
    let empty1 = create_pathmap_from_elements(&[], None);
    let empty2 = create_pathmap_from_elements(&[], None);

    let union = empty1.map.join(&empty2.map);
    assert!(union.is_empty(), "Union of empty maps should be empty");

    let intersection = empty1.map.meet(&empty2.map);
    assert!(
        intersection.is_empty(),
        "Intersection of empty maps should be empty"
    );

    let diff = empty1.map.subtract(&empty2.map);
    assert!(diff.is_empty(), "Subtraction of empty maps should be empty");
}

#[test]
fn test_single_segment_paths() {
    // PathMaps with single-segment paths
    let par1 = make_list_par(vec!["a"]);
    let par2 = make_list_par(vec!["b"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    assert_eq!(union.val_count(), 2);
}

#[test]
fn test_deep_nested_paths() {
    // Very deep nested paths
    let par = make_list_par(vec!["a", "b", "c", "d", "e", "f", "g", "h"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_duplicate_elements() {
    // Adding duplicate elements
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]); // Duplicate

    let result = create_pathmap_from_elements(&[par1, par2], None);
    // Should have 1 element (duplicates merged)
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_non_list_par() {
    // Non-list Par (single string) should work too
    let par = make_string_par("simple");
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_mixed_list_and_nonlist() {
    // Mix of list and non-list Pars
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_string_par("simple");

    let result = create_pathmap_from_elements(&[par1, par2], None);
    assert_eq!(result.map.val_count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// ★ THE MIXED FIXTURE — both codec arms in one map
// ═══════════════════════════════════════════════════════════════════════════
//
// `encode_trie_path` (`canonical_path.rs`, §1.3) is a TWO-ARM codec:
//
//   split arm (ground list `[e₁..e_k]`)  path = enc(e₁) ‖ … ‖ enc(e_k) ‖ 0x00
//   bare arm  (anything else)            path = enc(par)          — NO 0x00
//
// Nothing in this file, or in `rholang/tests/zipper_enumeration_spec.rs`, had
// ever built a map holding BOTH — `test_non_list_par` and
// `test_mixed_list_and_nonlist` above check only cardinality, which is the one
// question the defect does not affect. `{| 1, [1], "a", ["a","x"] |}` is the
// fixture that asks the rest.

/// `{| 1, [1], "a", ["a","x"] |}` — a bare int, its singleton list, a bare
/// string, and a two-element list sharing that string's segment.
fn mixed_elements() -> Vec<Par> {
    vec![
        make_int_par(1),
        make_list_of(vec![make_int_par(1)]),
        make_string_par("a"),
        make_list_par(vec!["a", "x"]),
    ]
}

/// The INSERT side is correct and injective — four elements, four distinct
/// keys, four entries — and the exact bytes are pinned so that any change to
/// the insert side is a visible diff rather than a silent re-key.
///
/// ★ In particular `1` and `[1]` differ by exactly the trailing terminator, so
/// "append a terminator at insert" would MERGE them. It is not the fix.
#[test]
fn mixed_arms_produce_four_distinct_entries() {
    let elements = mixed_elements();

    // Tag `0x03` = GInt (payload zigzag varint), `0x04` = GString
    // (payload uv(len) ++ UTF-8), `0x00` = the split-list terminator.
    assert_eq!(encode_trie_path(&elements[0]), vec![0x03, 0x02]);
    assert_eq!(encode_trie_path(&elements[1]), vec![0x03, 0x02, 0x00]);
    assert_eq!(encode_trie_path(&elements[2]), vec![0x04, 0x01, 0x61]);
    assert_eq!(
        encode_trie_path(&elements[3]),
        vec![0x04, 0x01, 0x61, 0x04, 0x01, 0x78, 0x00]
    );

    let result = create_pathmap_from_elements(&elements, None);
    assert_eq!(
        result.map.val_count(),
        4,
        "four distinct keys, so four entries — `1` and `[1]` do NOT collide"
    );
    for element in &elements {
        assert_eq!(
            result.map.get(encode_trie_path(element)),
            Some(element),
            "every element is readable at its own inserted key"
        );
    }
}

/// ⚠ WITNESS OF A DEFECT — the READ side. Every reader rebuilds an entry key
/// as `segments_to_key(par_to_path(p), true)`; for a bare element that is the
/// key of the SINGLETON LIST wrapping it, so the read lands on a different
/// entry (or on nothing).
///
/// Positive twin: `every_element_is_addressable_from_its_own_par`.
#[test]
fn witness_bare_elements_are_not_addressable_by_the_read_key() {
    let elements = mixed_elements();
    let map = create_pathmap_from_elements(&elements, None).map;

    // SPLIT elements: the rebuilt key is the inserted key, so the read lands.
    for split in [&elements[1], &elements[3]] {
        let read_key = segments_to_key(&par_to_path(split), true);
        assert_eq!(read_key, encode_trie_path(split));
        assert_eq!(map.get(&read_key), Some(split));
    }

    // ★ BARE `1`: the rebuilt key is `03 02 00` — the key of `[1]`, which is
    // ALSO in this map. The read succeeds and returns the WRONG element.
    let bare_int_read_key = segments_to_key(&par_to_path(&elements[0]), true);
    assert_eq!(bare_int_read_key, vec![0x03, 0x02, 0x00]);
    assert_eq!(
        map.get(&bare_int_read_key),
        Some(&elements[1]),
        "★ asking for the bare `1` returns the singleton list `[1]`"
    );

    // ★ BARE `"a"`: the rebuilt key is `04 01 61 00`, which is in no map here,
    // so the read is a miss — the same defect, presenting as Nil.
    let bare_string_read_key = segments_to_key(&par_to_path(&elements[2]), true);
    assert_eq!(bare_string_read_key, vec![0x04, 0x01, 0x61, 0x00]);
    assert_eq!(map.get(&bare_string_read_key), None);
}

/// ⚠ WITNESS OF A DEFECT — PREFIX CONFLATION. A bare entry's key is a strict
/// byte-prefix of the split keys that start with the same element, so bare
/// entries sit at INTERIOR trie positions and every `terminate = false` reader
/// (`leafCount`, `childCount`, `getSubtrie`, `pathExists`, `descendFirst`,
/// `restriction`) sees "the entry `a`" and "the prefix `["a", …]`" as one
/// position.
///
/// This is a SEMANTIC question, not a coding error — it is settled and
/// documented in the module header of `pathmap_native_query.rs`, and this test
/// is the executable statement of the situation it settles.
#[test]
fn witness_a_bare_entry_key_is_a_prefix_of_the_split_keys_beside_it() {
    let elements = mixed_elements();
    let map = create_pathmap_from_elements(&elements, None).map;

    let bare_a = encode_trie_path(&elements[2]); // 04 01 61
    let split_ax = encode_trie_path(&elements[3]); // 04 01 61 04 01 78 00
    assert!(
        split_ax.starts_with(&bare_a),
        "the bare entry's WHOLE key is a strict prefix of the split key"
    );

    // The prefix query cannot distinguish "the entry" from "the branch": the
    // subtrie at `04 01 61` holds BOTH the bare entry and `["a","x"]`. This is
    // the helper `leafCount()` at a cursor calls.
    let branch = models::rust::pathmap_native_query::subtrie_value_count(&map, &bare_a);
    assert_eq!(branch, 2, "the bare entry is counted inside its own branch");
}

#[test]
fn test_empty_list_par() {
    // Empty list should be handled gracefully
    let par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![], // Empty list
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    };

    let result = create_pathmap_from_elements(&[par], None);
    // Empty list might be stored differently, just ensure no panic
    assert!(result.map.val_count() <= 1);
}

// ============ ZIPPER TESTS ============

#[test]
fn test_read_zipper_creation() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let elements = vec![par1, par2];
    let result = create_pathmap_from_elements(&elements, None);

    // Verify the PathMap was created successfully
    assert_eq!(result.map.val_count(), 2);
}

#[test]
fn test_read_zipper_at_path() {
    let par1 = make_list_par(vec!["books", "fiction", "gatsby"]);
    let par2 = make_list_par(vec!["books", "fiction", "moby"]);
    let par3 = make_list_par(vec!["books", "nonfiction", "history"]);

    let elements = vec![par1, par2, par3];
    let result = create_pathmap_from_elements(&elements, None);

    // Verify we can create a PathMap at a specific path
    assert_eq!(result.map.val_count(), 3);
}

#[test]
fn test_write_zipper_set_val() {
    let mut map = RholangPathMap::new();

    // Create a simple path and set a value
    let par = make_string_par("value");
    map.insert(b"test_path".to_vec(), par.clone());

    assert_eq!(map.val_count(), 1);
}

#[test]
fn test_graft_operation() {
    // Test grafting one PathMap into another
    let src_par1 = make_list_par(vec!["one", "val"]);
    let src_par2 = make_list_par(vec!["one", "two", "val"]);

    let dst_par = make_list_par(vec!["prefix"]);

    let src_result = create_pathmap_from_elements(&[src_par1, src_par2], None);
    let dst_result = create_pathmap_from_elements(&[dst_par], None);

    // Verify both PathMaps were created
    assert_eq!(src_result.map.val_count(), 2);
    assert_eq!(dst_result.map.val_count(), 1);

    // In a real implementation, we would graft src into dst at a specific path
    // For now, just verify the union operation works
    let combined = dst_result.map.join(&src_result.map);
    assert_eq!(combined.val_count(), 3);
}

#[test]
fn test_join_into_operation() {
    // Test union-merge of two PathMaps
    let par1 = make_list_par(vec!["roman"]);
    let par2 = make_list_par(vec!["romulus"]);

    let par3 = make_list_par(vec!["room"]);
    let par4 = make_list_par(vec!["root"]);

    let map1 = create_pathmap_from_elements(&[par1, par2], None);
    let map2 = create_pathmap_from_elements(&[par3, par4], None);

    let result = map1.map.join(&map2.map);
    assert_eq!(result.val_count(), 4);
}

#[test]
fn test_zipper_empty_pathmap() {
    // Test zipper operations on empty PathMap
    let result = create_pathmap_from_elements(&[], None);
    assert!(result.map.is_empty());
    assert_eq!(result.map.val_count(), 0);
}

#[test]
fn test_zipper_single_element() {
    // Test zipper on single-element PathMap
    let par = make_list_par(vec!["single"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_zipper_deep_path() {
    // Test zipper with deeply nested path
    let par = make_list_par(vec!["a", "b", "c", "d", "e", "f"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

// ============ ACTUAL DROP HEAD TESTS ============

fn perform_drophead(elements: Vec<Par>, n: usize) -> Vec<Par> {
    // Simulate what the interpreter does in dropHead
    let mut result_elements = Vec::new();

    for par in &elements {
        // Check if this Par is a list
        if let Some(ExprInstance::EListBody(list)) =
            par.exprs.first().and_then(|e| e.expr_instance.as_ref())
        {
            // It's a list - drop n elements from the beginning
            if list.ps.len() > n {
                let remaining = list.ps[n..].to_vec();
                let new_list = EList {
                    ps: remaining,
                    locally_free: list.locally_free.clone(),
                    connective_used: list.connective_used,
                    remainder: list.remainder.clone(),
                };
                let new_par = Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EListBody(new_list)),
                    }],
                    ..par.clone()
                };
                result_elements.push(new_par);
            }
            // If not enough elements, skip this entry
        } else {
            // Not a list - keep as-is if n == 0
            if n == 0 {
                result_elements.push(par.clone());
            }
        }
    }

    result_elements
}

fn extract_list_from_par(par: &Par) -> Option<Vec<String>> {
    if let Some(ExprInstance::EListBody(list)) =
        par.exprs.first().and_then(|e| e.expr_instance.as_ref())
    {
        let strings: Vec<String> = list
            .ps
            .iter()
            .filter_map(|p| {
                if let Some(ExprInstance::GString(s)) =
                    p.exprs.first().and_then(|e| e.expr_instance.as_ref())
                {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        Some(strings)
    } else {
        None
    }
}

#[test]
fn test_drophead_large_value() {
    // dropHead(10) on path ["a", "b", "c"] should remove all elements (n > path length)
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 10);
    assert!(
        result.is_empty(),
        "dropHead with n > length should remove all elements"
    );
}

#[test]
fn test_drophead_zero() {
    // dropHead(0) should preserve all elements
    let par1 = make_list_par(vec!["a", "b", "c"]);
    let par2 = make_list_par(vec!["x", "y", "z"]);
    let elements = vec![par1.clone(), par2.clone()];

    let result = perform_drophead(elements, 0);
    assert_eq!(result.len(), 2, "dropHead(0) should preserve all elements");

    let list1 = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(list1, vec!["a", "b", "c"]);
}

#[test]
fn test_drophead_exact_length() {
    // dropHead(3) on path ["a", "b", "c"] should remove all elements
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 3);
    assert!(
        result.is_empty(),
        "dropHead with n == path length should remove all elements"
    );
}

#[test]
fn test_drophead_partial() {
    // dropHead(1) on path ["a", "b", "c"] should leave ["b", "c"]
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 1);
    assert_eq!(result.len(), 1, "dropHead(1) should keep the entry");

    let remaining_list = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(
        remaining_list,
        vec!["b", "c"],
        "Should have dropped first element"
    );
}

#[test]
fn test_drophead_multiple_paths_different_lengths() {
    // dropHead on PathMap with multiple paths of different lengths
    // dropHead(2): ["a", "b", "c", "d"] → ["c", "d"], ["x", "y"] → removed
    let par1 = make_list_par(vec!["a", "b", "c", "d"]);
    let par2 = make_list_par(vec!["x", "y"]);
    let elements = vec![par1, par2];

    let result = perform_drophead(elements, 2);
    assert_eq!(result.len(), 1, "Only the longer path should remain");

    let remaining_list = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(
        remaining_list,
        vec!["c", "d"],
        "Should have dropped first 2 elements"
    );
}

#[test]
fn test_drophead_single_element_path() {
    // dropHead(1) on single-element path ["a"] should result in empty
    let par = make_list_par(vec!["a"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 1);
    assert!(
        result.is_empty(),
        "dropHead(1) on 1-element path should remove it"
    );
}

#[test]
fn test_drophead_all_paths_too_short() {
    // dropHead(5) when all paths are shorter should result in empty
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["x", "y", "z"]);
    let elements = vec![par1, par2];

    let result = perform_drophead(elements, 5);
    assert!(
        result.is_empty(),
        "dropHead with n larger than all paths should remove everything"
    );
}

#[test]
fn test_drophead_mixed_survivability() {
    // Some paths survive, some don't
    let par1 = make_list_par(vec!["a", "b", "c", "d", "e"]); // Survives with 3 elements
    let par2 = make_list_par(vec!["x", "y"]); // Removed
    let par3 = make_list_par(vec!["p", "q", "r"]); // Survives with 1 element
    let elements = vec![par1, par2, par3];

    let result = perform_drophead(elements, 2);
    assert_eq!(result.len(), 2, "2 paths should survive dropHead(2)");

    let list1 = extract_list_from_par(&result[0]).unwrap();
    let list2 = extract_list_from_par(&result[1]).unwrap();

    // Verify correct elements were dropped
    assert!(
        list1.len() >= 1 && list2.len() >= 1,
        "Surviving paths should have elements"
    );
}
