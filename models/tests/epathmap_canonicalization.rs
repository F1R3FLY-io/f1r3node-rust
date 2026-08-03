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
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

fn gint(v: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }],
        ..Default::default()
    }
}
fn gstr(s: &str) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}
fn map_expr(entries: Vec<Par>) -> Par {
    models::par_from_default! {
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
fn normalize(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

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

/// ★ **THE CONSTRUCTOR IS THE CANONICALIZER**, and it is idempotent.
///
/// `canonicalize_ground_epathmap` is deleted. It took a map, built a trie from
/// its `ps`, and read the entries back out in trie order — which is precisely
/// what `EPathMap::new` does now, because the map it builds STORES that trie.
/// A separate canonicalizer could only have been the identity, so keeping it as
/// a no-op would have been keeping a second route to the same answer, which is
/// the shape of defect this campaign has been removing.
#[test]
fn the_constructor_canonicalizes_and_is_idempotent() {
    let m = EPathMap::new(vec![gstr("z"), gint(3), gstr("a")], Vec::new(), false, None);
    let round_tripped = EPathMap::new(m.entry_trie().entries_owned(), Vec::new(), false, None);
    assert_eq!(
        m.trie_snapshot(),
        round_tripped.trie_snapshot(),
        "re-filing a canonical projection reproduces it"
    );

    let permuted = EPathMap::new(vec![gint(3), gstr("a"), gstr("z")], Vec::new(), false, None);
    assert_eq!(
        m.trie_snapshot(),
        permuted.trie_snapshot(),
        "…and a permuted construction of the same entry set lands on it too"
    );
}

/// ★ **A NON-GROUND MAP IS CANONICALIZED TOO** — this test used to assert the
/// opposite, and it is rewritten rather than deleted because the reversal IS
/// the change.
///
/// It read *"a non-ground map is returned unchanged (order preserved)"*, which
/// was true while only ground maps had a trie to be read back off. Every map
/// stores its entries in a trie now — `encode_trie_path` is total over every
/// `Par` via the `0x0F` escape arm — so a pattern map's entries come back in
/// trie order like anyone else's.
///
/// ⚠ **This moves consensus bytes.** A non-ground map's only prost encoding is
/// tag 1, `repeated Par`, written in `ps` order, so re-ordering `ps` re-orders
/// the wire. That is stated in the commit; the network version constant is not
/// touched here, because bumping it is a network-coordination act.
///
/// ⚠ And a limit, so it is not over-claimed: this is a canonical **SYNTACTIC**
/// identity — byte-lex over the entries' escape-arm encodings — and **not a
/// semantic one**. Two non-ground entries can be different terms that match the
/// same things, and no representation collapses those, because pattern
/// equivalence is undecidable in general. What is gained is order-insensitivity
/// and deduplication, which is real.
#[test]
fn a_non_ground_map_is_canonicalized_as_well() {
    let free = models::par_from_default! {
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
    let forward = EPathMap::new(
        vec![gstr("z"), free.clone(), gstr("a")],
        Vec::new(),
        true,
        None,
    );
    let permuted = EPathMap::new(vec![free, gstr("a"), gstr("z")], Vec::new(), true, None);

    assert_eq!(
        forward.trie_snapshot(),
        permuted.trie_snapshot(),
        "two orders of one non-ground entry set are one value"
    );
    assert_eq!(
        forward, permuted,
        "…and the comparators agree, as they must with the wire"
    );
    assert_eq!(
        prost::Message::encode_to_vec(&forward),
        prost::Message::encode_to_vec(&permuted),
        "…and so does the wire itself"
    );
}

#[test]
fn empty_and_singleton_are_stable() {
    let empty = map_expr(vec![]);
    assert_eq!(normalize(&empty), normalize(&map_expr(vec![])));
    let single = map_expr(vec![gint(7)]);
    assert_eq!(normalize(&single), normalize(&map_expr(vec![gint(7)])));
}
