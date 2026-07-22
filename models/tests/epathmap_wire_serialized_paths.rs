//! EPathMap native byte-array wire (U(m), proto field 8 `serialized_paths`):
//! the VALUE arm of a GROUND map serializes as the uncompressed, trie-ordered,
//! length-framed key stream produced by a PathMap zipper walk — canonical
//! (permutation/duplication-insensitive), injective, and never deflated. These
//! tests pin the WIRE + DIGEST canonicity that Part A guarantees on its own
//! (structural eq / live COMM matching are pinned by the interpreter tests once
//! the normalization canonicalizer lands).

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EPathMap, Expr, Par, Var};
use prost::Message;

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
fn gmap(entries: Vec<Par>) -> Par {
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
/// A GROUND EPathMap (no remainder / connective / free vars).
fn ground(entries: Vec<Par>) -> EPathMap {
    EPathMap::new(entries, Vec::new(), false, None)
}

// proto field 8, wire type 2 (length-delimited) = (8 << 3) | 2 = 0x42.
const FIELD8_KEY: u8 = 0x42;
// proto field 1, wire type 2 = (1 << 3) | 2 = 0x0A.
const FIELD1_KEY: u8 = 0x0A;

#[test]
fn ground_map_serializes_as_field8_u_m() {
    let m = ground(vec![gint(1), gint(2)]);
    let bytes = m.encode_to_vec();
    assert_eq!(
        bytes[0], FIELD8_KEY,
        "a non-empty ground map is the VALUE arm (field 8 serialized_paths)"
    );
    assert!(!m.intern().path_stream.is_empty(), "U(m) is non-empty");
}

#[test]
fn permuted_construction_yields_identical_wire_and_digest() {
    let forward = ground(vec![gstr("a"), gint(1), gstr("b")]);
    let backward = ground(vec![gstr("b"), gstr("a"), gint(1)]);
    // Same entry multiset, different construction order ⇒ identical U(m) bytes
    // (the trie/zipper walk is insertion-order-independent) and identical
    // Blake2b digest.
    assert_eq!(forward.encode_to_vec(), backward.encode_to_vec());
    assert_eq!(forward.intern().path_stream, backward.intern().path_stream);
    assert_eq!(forward.intern().digest, backward.intern().digest);
    // ...and they intern to the SAME shared entry.
    assert!(std::sync::Arc::ptr_eq(&forward.intern(), &backward.intern()));
}

#[test]
fn duplicated_entries_dedup_in_the_wire() {
    let with_dup = ground(vec![gint(1), gint(2), gint(1), gint(2)]);
    let deduped = ground(vec![gint(1), gint(2)]);
    // Idempotent trie insertion dedups ⇒ identical U(m) + digest.
    assert_eq!(with_dup.encode_to_vec(), deduped.encode_to_vec());
    assert_eq!(with_dup.intern().digest, deduped.intern().digest);
}

#[test]
fn nested_map_of_map_wire_is_canonical() {
    // The outer map holds one entry — a nested map. Permuting the NESTED map's
    // entries never changes the outer U(m) (encode_trie_path canonicalizes the
    // nested region via its own zipper walk).
    let forward = ground(vec![gmap(vec![gint(1), gint(2)])]);
    let backward = ground(vec![gmap(vec![gint(2), gint(1)])]);
    assert_eq!(forward.encode_to_vec(), backward.encode_to_vec());
    assert_eq!(forward.intern().digest, backward.intern().digest);
}

#[test]
fn byte_round_trip_is_stable_and_reconstructs_canonically() {
    let m = ground(vec![gstr("zebra"), gint(-7), gstr("apple"), gint(3)]);
    let bytes = m.encode_to_vec();
    let decoded = EPathMap::decode(&bytes[..]).expect("decode field-8 value arm");
    // The decoded map re-encodes byte-identically (canonical fixed point) and
    // carries every entry (in trie order).
    assert_eq!(decoded.encode_to_vec(), bytes);
    assert_eq!(decoded.ps.len(), 4);
    // The decoded (trie-ordered) map is the canonical form: encoding it and the
    // permuted original agree.
    let permuted = ground(vec![gint(3), gstr("apple"), gint(-7), gstr("zebra")]);
    assert_eq!(permuted.encode_to_vec(), bytes);
}

#[test]
fn injective_distinct_maps_have_distinct_digests() {
    let maps = vec![
        ground(vec![gint(1)]),
        ground(vec![gint(2)]),
        ground(vec![gint(1), gint(2)]),
        ground(vec![gint(1), gint(3)]),
        ground(vec![gstr("1")]),
        ground(vec![gmap(vec![gint(1)])]),
    ];
    let mut digests: Vec<[u8; 32]> = maps.iter().map(|m| m.intern().digest).collect();
    let count = digests.len();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), count, "distinct ground maps ⇒ distinct digests");
}

#[test]
fn large_map_round_trips() {
    let entries: Vec<Par> = (0..2000i64).map(gint).collect();
    let m = ground(entries);
    let bytes = m.encode_to_vec();
    assert_eq!(bytes[0], FIELD8_KEY);
    let decoded = EPathMap::decode(&bytes[..]).expect("decode large map");
    assert_eq!(decoded.ps.len(), 2000);
    assert_eq!(decoded.encode_to_vec(), bytes);
    // Reverse construction ⇒ identical wire (order-insensitive at scale).
    let reversed: Vec<Par> = (0..2000i64).rev().map(gint).collect();
    assert_eq!(ground(reversed).encode_to_vec(), bytes);
}

#[test]
fn non_ground_map_stays_on_the_term_arm() {
    // A map carrying a remainder is NOT eval_stable ⇒ the TERM arm (field 1
    // `ps`), byte-identical to pre-wire behavior.
    let remainder = Some(Var {
        var_instance: Some(VarInstance::FreeVar(0)),
    });
    let m = EPathMap::new(vec![gint(1)], Vec::new(), true, remainder);
    let bytes = m.encode_to_vec();
    assert_eq!(
        bytes[0], FIELD1_KEY,
        "a non-ground map serializes on the term arm (field 1 ps), never field 8"
    );
    assert!(m.intern().path_stream.is_empty(), "no U(m) for a non-ground map");
    let decoded = EPathMap::decode(&bytes[..]).expect("decode term arm");
    assert_eq!(decoded.encode_to_vec(), bytes);
}

#[test]
fn empty_map_is_all_defaults_not_field8() {
    let m = ground(vec![]);
    let bytes = m.encode_to_vec();
    assert!(bytes.is_empty(), "the canonical empty map is the all-defaults message");
    assert!(m.intern().path_stream.is_empty());
    let decoded = EPathMap::decode(&bytes[..]).expect("decode empty");
    assert!(decoded.ps.is_empty());
}
