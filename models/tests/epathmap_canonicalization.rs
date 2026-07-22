//! EPathMap canonicalization → order-insensitive matching.
//!
//! A GROUND map normalizes to TRIE order (a PathMap zipper walk, NO sort), so
//! permuted / duplicated constructions become STRUCTURALLY EQUAL after
//! normalization — the property that makes the spatial matcher (and hence a
//! COMM) fire order-insensitively, and that makes the serde event-hash
//! preimage a pure function of the entry multiset. Non-ground maps keep the
//! pre-wire (order-preserving) normalization.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EPathMap, Expr, Par, Var};
use models::rust::pathmap_crate_type_mapper::canonicalize_ground_epathmap;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

fn gint(v: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }],
        ..Default::default()
    }
}
fn gstr(s: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}
fn map_expr(entries: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new(
                entries,
                Vec::new(),
                false,
                None,
            ))),
        }],
        ..Default::default()
    }
}
fn nested_map(entries: Vec<Par>) -> Par {
    // A nested map, as an ENTRY of an outer map.
    map_expr(entries)
}

/// Normalize a Par exactly as the compiler / runtime does (the sorter).
fn normalize(par: &Par) -> Par {
    ParSortMatcher::sort_match(par).term
}

#[test]
fn permuted_ground_maps_normalize_equal() {
    let forward = map_expr(vec![gstr("a"), gint(1), gstr("b")]);
    let backward = map_expr(vec![gstr("b"), gstr("a"), gint(1)]);
    // Different construction order ⇒ IDENTICAL normalized form ⇒ the spatial
    // matcher (structural ==) fires ⇒ COMM.
    assert_eq!(
        normalize(&forward),
        normalize(&backward),
        "permuted ground maps must normalize to the same term"
    );
}

#[test]
fn duplicated_entries_collapse_under_normalization() {
    let with_dup = map_expr(vec![gint(1), gint(2), gint(1), gint(2)]);
    let deduped = map_expr(vec![gint(1), gint(2)]);
    assert_eq!(normalize(&with_dup), normalize(&deduped));
}

#[test]
fn nested_map_of_map_normalizes_equal() {
    // {|{|1,2|}|} == {|{|2,1|}|}: canonicalization recurses bottom-up through
    // the zipper walk at every level.
    let forward = map_expr(vec![nested_map(vec![gint(1), gint(2)])]);
    let backward = map_expr(vec![nested_map(vec![gint(2), gint(1)])]);
    assert_eq!(normalize(&forward), normalize(&backward));
}

#[test]
fn permuted_ground_maps_have_identical_event_hash_preimage() {
    // The serde bytes (the event-hash preimage) of a ground map are a pure
    // function of the entry multiset — consensus-safe, with NO dependence on
    // construction order and NO pre-normalization required. The hand-written
    // `EPathMap::serialize` canonicalizes a ground map's `ps` to trie order AT
    // THE SERIALIZER, so the RAW (non-normalized) permutations already agree —
    // the strengthened property the old derived Serialize could only provide
    // AFTER an explicit `normalize()` (which a raw producer never applies).
    let forward = map_expr(vec![gstr("a"), gint(1), gstr("b")]);
    let backward = map_expr(vec![gstr("b"), gint(1), gstr("a")]);
    assert_eq!(
        bincode::serialize(&forward).expect("serialize"),
        bincode::serialize(&backward).expect("serialize"),
        "raw (non-normalized) permuted ground maps must have identical serde (event-hash) bytes"
    );
    // And the serializer's canonicalization AGREES with the sorter's
    // normalization (the two canonical forms coincide).
    assert_eq!(
        bincode::serialize(&forward).expect("serialize"),
        bincode::serialize(&normalize(&forward)).expect("serialize"),
        "serializer canonicalization must agree with sorter normalization"
    );
}

#[test]
fn canonicalize_is_idempotent() {
    let m = EPathMap::new(vec![gstr("z"), gint(3), gstr("a")], Vec::new(), false, None);
    let once = canonicalize_ground_epathmap(&m);
    let twice = canonicalize_ground_epathmap(&once);
    assert_eq!(once.ps.as_slice(), twice.ps.as_slice(), "canonicalization is idempotent");
    // And a canonicalized map equals a permuted-then-canonicalized one.
    let permuted = EPathMap::new(vec![gint(3), gstr("a"), gstr("z")], Vec::new(), false, None);
    assert_eq!(once.ps.as_slice(), canonicalize_ground_epathmap(&permuted).ps.as_slice());
}

#[test]
fn non_ground_map_is_not_canonicalized() {
    // A pattern map (a free-var entry, connective_used) is NOT eval_stable, so
    // it is returned UNCHANGED — its entry order is preserved (pre-wire
    // behavior; binder patterns still match structurally with remainder bind).
    let free = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            })),
        }],
        connective_used: true,
        ..Default::default()
    };
    let m = EPathMap::new(vec![gstr("z"), free, gstr("a")], Vec::new(), true, None);
    let canonical = canonicalize_ground_epathmap(&m);
    assert_eq!(
        canonical.ps.as_slice(),
        m.ps.as_slice(),
        "a non-ground map is returned unchanged (order preserved)"
    );
}

#[test]
fn empty_and_singleton_are_stable() {
    let empty = map_expr(vec![]);
    assert_eq!(normalize(&empty), normalize(&map_expr(vec![])));
    let single = map_expr(vec![gint(7)]);
    assert_eq!(normalize(&single), normalize(&map_expr(vec![gint(7)])));
}
