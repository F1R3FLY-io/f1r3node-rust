//! DR-131 P2b: a native history row whose payload is nested 100,000 levels
//! deep decodes on a thread with a small stack.
//!
//! The derived bincode decoder recurses once for each nesting level, at about
//! 12,894 bytes of stack per level in release builds (measured on
//! `feature/mettail`, `5108d8543:rspace++/src/rspace/serializers/cold_store_decode.rs:8-10`).
//! The explicit-stack machine of P2b must decode the same row without that
//! recursion.
//!
//! The deep encoding is built without a recursive encoder. Bincode is
//! positional, so a `Par` that holds one list with one child encodes as a
//! fixed prefix, the child, and a fixed suffix. Repeating the prefix and the
//! suffix around a marker gives the encoding of the nested value. The decoded
//! value is checked and dismantled one level at a time, because the derived
//! `PartialEq` and `Drop` would recurse once for each level.

// Changed by the merge of feature/cost-accounted-rho (finding R3-16):
// F1R3Lang's `Par` has a generated iterative `Drop`, so struct-update syntax
// (`Par { field, ..Default::default() }`) cannot move the other fields out of
// the base (E0509). Each such `Par` builder in this file uses
// `models::par_from_default!`, which evaluates the base once and then assigns
// each named field.
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, ListParWithRandom, Par};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::native_reader::decode_datum;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::serializers::serializers::encode_datum;
use rspace_plus_plus::rspace::trace::event::Produce;

/// The nesting depth of the deep row.
const DEEP_NESTING: usize = 100_000;

/// The stack of the thread that decodes the deep row.
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn marker() -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("DR-131 deep marker".to_string())),
        }],
        ..Default::default()
    }
}

fn wrapped(child: Par) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![child],
                ..Default::default()
            })),
        }],
        ..Default::default()
    }
}

/// Splits `outer` around the only occurrence of `inner`.
fn split_around<'a>(outer: &'a [u8], inner: &[u8]) -> (&'a [u8], &'a [u8]) {
    let positions: Vec<usize> = outer
        .windows(inner.len())
        .enumerate()
        .filter(|(_, window)| *window == inner)
        .map(|(position, _)| position)
        .collect();
    assert_eq!(positions.len(), 1, "the marker occurs exactly once");
    (&outer[..positions[0]], &outer[positions[0] + inner.len()..])
}

fn datum_with(par: Par) -> Datum<ListParWithRandom> {
    Datum {
        // Changed by the merge of feature/cost-accounted-rho (finding R3-10):
        // F1R3Lang's `Datum` shares its payload by `Arc`.
        // a: ListParWithRandom {
        //     pars: vec![par],
        //     random_state: vec![1, 2, 3],
        //     ..Default::default()
        // },
        a: Arc::new(ListParWithRandom {
            pars: vec![par],
            random_state: vec![1, 2, 3],
            ..Default::default()
        }),
        persist: false,
        source: Produce {
            channel_hash: Blake2b256Hash::new(b"deep channel"),
            hash: Blake2b256Hash::new(b"deep produce"),
            ..Default::default()
        },
    }
}

/// The encoding of a datum whose payload is `marker` inside `depth` lists.
fn deep_row(depth: usize) -> Vec<u8> {
    let leaf = bincode::serialize(&marker()).expect("serialize the marker");
    let once = bincode::serialize(&wrapped(marker())).expect("serialize one level");
    let (prefix, suffix) = split_around(&once, &leaf);
    // Changed by the merge of feature/cost-accounted-rho (finding R3-10):
    // F1R3Lang's `Datum` has no serde derives. `encode_datum` writes the
    // payload, `persist` and `source` in the order of the derived encoder, so
    // the bytes are the same.
    // let row = bincode::serialize(&datum_with(marker())).expect("serialize the shallow row");
    let row = encode_datum(&datum_with(marker()));
    let (row_prefix, row_suffix) = split_around(&row, &leaf);
    let mut bytes = Vec::with_capacity(
        row_prefix.len() + depth * (prefix.len() + suffix.len()) + leaf.len() + row_suffix.len(),
    );
    bytes.extend_from_slice(row_prefix);
    for _ in 0..depth {
        bytes.extend_from_slice(prefix);
    }
    bytes.extend_from_slice(&leaf);
    for _ in 0..depth {
        bytes.extend_from_slice(suffix);
    }
    bytes.extend_from_slice(row_suffix);
    bytes
}

/// Checks that `par` is `marker` inside `depth` single-element lists, and
/// drops it one level at a time.
fn check_and_dismantle(mut par: Par, depth: usize) {
    for level in 0..depth {
        let mut exprs = std::mem::take(&mut par.exprs);
        assert_eq!(par, Par::default(), "level {level} holds only one list");
        assert_eq!(exprs.len(), 1, "level {level} holds one expression");
        let Some(ExprInstance::EListBody(mut list)) =
            exprs.pop().and_then(|expr| expr.expr_instance)
        else {
            panic!("level {level} holds a list");
        };
        assert_eq!(
            list.ps.len(),
            1,
            "the list at level {level} holds one element"
        );
        let child = list.ps.pop().expect("one list element");
        assert_eq!(
            list,
            EList::default(),
            "the list at level {level} has no other field"
        );
        par = child;
    }
    assert_eq!(par, marker());
}

#[test]
fn a_row_nested_one_hundred_thousand_deep_decodes_on_a_small_stack() {
    let bytes = deep_row(DEEP_NESTING);
    let decoded = std::thread::Builder::new()
        .name("cold-store-decode-small-stack".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || decode_datum::<ListParWithRandom>(&bytes).map_err(|error| error.to_string()))
        .expect("spawn the small-stack thread")
        .join()
        .expect("the decode stays within a small stack")
        .expect("the deep row decodes");
    assert!(!decoded.persist);
    // Changed by the merge of feature/cost-accounted-rho (finding R3-10):
    // F1R3Lang's `Datum` shares its payload by `Arc`. The decoder made the
    // only reference, so the payload is unwrapped without a copy.
    // let mut pars = decoded.a.pars;
    let mut payload =
        Arc::try_unwrap(decoded.a).expect("the decoded datum is the only owner of its payload");
    let mut pars = std::mem::take(&mut payload.pars);
    assert_eq!(pars.len(), 1);
    check_and_dismantle(pars.pop().expect("one payload"), DEEP_NESTING);
}

#[test]
fn the_deep_row_builder_matches_the_encoder_at_a_shallow_depth() {
    let mut nested = marker();
    for _ in 0..5 {
        nested = wrapped(nested);
    }
    // Changed by the merge of feature/cost-accounted-rho (finding R3-10):
    // F1R3Lang's `Datum` has no serde derives (see `deep_row`).
    // assert_eq!(
    //     deep_row(5),
    //     bincode::serialize(&datum_with(nested)).expect("serialize the shallow nested row")
    // );
    assert_eq!(deep_row(5), encode_datum(&datum_with(nested)));
}
