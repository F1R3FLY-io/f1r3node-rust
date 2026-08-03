//! # The escape arm is stack-safe and reachable from ordinary Rholang
//!
//! `decode_trie_path` is total in the trie grammar: `COLLECTION_DEPTH_LIMIT`,
//! `SCANNER_STACK_CEILING`, and the former Prost `DecodeContext` ceiling are
//! retired. The `0x0F` **escape arm** stores a ¬`eval_stable` entry as its
//! canonical protobuf bytes and reads it with the generated heap-stack decoder.
//!
//! This matters because moving serialization onto the trie's byte-array form
//! makes `decode_trie_path` the reader. A `PathMap<()>` set can therefore recover
//! entries from keys without retaining a redundant mirrored `Par`, while a
//! `PathMap<Par>` map can reserve its value slot for the associated value.
//!
//! ## ★★ Anti-vacuity
//!
//! The tests walk every depth through a declared ladder and assert the escape
//! tag before decoding. The same deep subject also crosses the public
//! stack-safe `Message` surface, without turning a depth sample into a new
//! artificial maximum.
//!
//! The Rholang fixture carries the matching control: it asserts the entry it
//! built is genuinely ¬`eval_stable` (i.e. really does take the escape arm)
//! before drawing any conclusion from its depth. A fixture that fell into the
//! stable alphabet would measure the split arm and prove nothing about escapes.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ESet, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path, tag};
use models::rust::pathmap_crate_type_mapper::eval_stable_par_for_test;
use rholang::rust::interpreter::compiler::compiler::Compiler;

// ─────────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────────

fn expr_carrier(instance: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn gint(value: i64) -> Par { expr_carrier(ExprInstance::GInt(value)) }

fn glist(ps: Vec<Par>) -> Par {
    expr_carrier(ExprInstance::EListBody(EList {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

/// A ¬`eval_stable` entry nested `depth` levels deep — the same shape
/// `canonical_path`'s own partiality measurement uses, rebuilt here from the
/// public API. `ESet` is outside the stable alphabet, so the whole term takes
/// the ESCAPE arm; the `EList` spine supplies the depth.
fn escaped_nest(depth: usize) -> Par {
    let mut inner = gint(1);
    for _ in 0..depth {
        inner = glist(vec![inner]);
    }
    expr_carrier(ExprInstance::ESetBody(ESet {
        ps: vec![inner],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

/// Prove `decode_trie_path ∘ encode_trie_path = id` throughout a depth ladder.
/// `limit` is a test sample, not a runtime bound.
fn assert_escape_arm_roundtrips_through(limit: usize) {
    for depth in 0..=limit {
        let par = escaped_nest(depth);
        let key = encode_trie_path(&par);
        assert_eq!(
            key.first(),
            Some(&tag::ESCAPE),
            "control: depth {depth} must take the ESCAPE arm, or this measures the split arm"
        );
        let round_tripped = decode_trie_path(&key).unwrap_or_else(|error| {
            panic!("generated escape decode failed at depth {depth}: {error:?}")
        });
        assert_eq!(
            round_tripped, par,
            "escape round-trip differs at depth {depth}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. The former codec boundary is gone
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_generated_escape_reader_has_no_recursive_depth_ceiling() {
    const TEST_DEPTH: usize = 400;
    assert_escape_arm_roundtrips_through(TEST_DEPTH);

    use prost::Message;
    let control = escaped_nest(TEST_DEPTH);
    let control_bytes = control.encode_to_vec();
    let decoded = Par::decode(control_bytes.as_slice())
        .expect("the public Message surface also uses the generated stack-safe machine");
    assert_eq!(decoded, control);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. The domain theorem, executable
// ─────────────────────────────────────────────────────────────────────────────

/// A field-9 EPM1 envelope adds no recursive decoder depth. Every sampled entry
/// round-trips both standalone and inside `EPathMap`; the payload is PathMap's
/// compact trie region and the generated decoder owns the nested traversal.
#[test]
fn a_map_envelope_costs_the_reader_no_depth_at_all() {
    use models::rhoapi::EPathMap;
    use prost::Message;

    const TEST_DEPTH: usize = 400;
    for depth in 0..=TEST_DEPTH {
        let entry = escaped_nest(depth);
        let map = EPathMap::new(vec![entry.clone()], Vec::new(), false, None);
        let bytes = <EPathMap as Message>::encode_to_vec(&map);
        let decoded = <EPathMap as Message>::decode(bytes.as_slice())
            .unwrap_or_else(|error| panic!("map decode failed at depth {depth}: {error:?}"));
        assert_eq!(decoded.len(), 1);
        assert!(decoded
            .entry_trie()
            .set_trie()
            .get(encode_trie_path(&entry))
            .is_some());
    }

    // ANTI-VACUITY: the fixture must take field 9 EPM1. Length-delimited field
    // 9 is 0x4a; the retired list arm was tag 1 (0x0a).
    let probe = EPathMap::new(vec![escaped_nest(1)], Vec::new(), false, None);
    let probe_bytes = <EPathMap as Message>::encode_to_vec(&probe);
    assert_eq!(
        probe_bytes.first(),
        Some(&0x4au8),
        "control: the map must emit EPM1 at field 9 — if a list arm ever returns, \
         this measurement is about a different envelope and proves nothing"
    );

    assert_escape_arm_roundtrips_through(TEST_DEPTH);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Reachability from a deploy
// ─────────────────────────────────────────────────────────────────────────────

/// Build `{| Set([[…1…]]) |}` — a pathmap over one ¬`eval_stable` entry nested
/// `depth` levels — as Rholang source.
fn deep_pathmap_source(depth: usize) -> String {
    let mut source = String::with_capacity(depth * 2 + 24);
    source.push_str("{| Set(");
    for _ in 0..depth {
        source.push('[');
    }
    source.push('1');
    for _ in 0..depth {
        source.push(']');
    }
    source.push_str(") |}");
    source
}

/// Ordinary Rholang reaches deeply escaped keys and the generated reader accepts
/// them. This guards the end-to-end route used by PathMap set projections.
#[test]
fn ordinary_rholang_reaches_the_stack_safe_escape_reader() {
    let probe_depth = 96;

    let source = deep_pathmap_source(probe_depth);
    let normalized = Compiler::source_to_adt(&source);

    let par = normalized.expect("the compiler accepts the deep pathmap fixture");

    // Dig out the map's single entry.
    let entry = par
        .exprs
        .iter()
        .find_map(|expr| match expr.expr_instance.as_ref() {
            Some(ExprInstance::EPathmapBody(map)) => map.entry_trie().find_entry(|_| true),
            _ => None,
        })
        .expect("the fixture must normalize to an EPathMap with one entry");

    // ── ANTI-VACUITY: the entry must really take the escape arm. ─────────────
    assert!(
        !eval_stable_par_for_test(&entry),
        "control: the fixture must be ¬eval_stable, or it takes the SPLIT arm and \
         measures nothing about escapes"
    );
    let key = encode_trie_path(&entry);
    assert_eq!(
        key.first(),
        Some(&tag::ESCAPE),
        "control: the compiled entry must take the ESCAPE arm"
    );

    assert_eq!(
        decode_trie_path(&key).expect("the generated reader accepts the compiled key"),
        entry
    );
}
