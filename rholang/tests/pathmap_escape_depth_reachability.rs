//! # Is the escape arm's decode ceiling REACHABLE from ordinary Rholang?
//!
//! `decode_trie_path` is total in the trie grammar — `COLLECTION_DEPTH_LIMIT`
//! and `SCANNER_STACK_CEILING` are retired (`models/src/rust/canonical_path.rs`,
//! module notes) — with exactly one residual partiality: the `0x0F` **escape
//! arm**, which stores a ¬`eval_stable` entry as its canonical prost bytes and
//! reads them back with `Par::decode`. prost caps decode recursion at 100
//! message levels and caps encode at nothing, so past some term depth an entry
//! encodes to a key that will not decode.
//!
//! That boundary is about to matter in a way it did not before. Moving every
//! serialization surface onto the trie's own byte array `U(m)` makes
//! `decode_trie_path` the **reader** on surfaces that previously carried entries
//! as nested `Par`s. This file measures which of those moves is permissive and
//! which is restrictive, and — the question the register entry turns on —
//! whether the restrictive class is reachable from a Rholang deploy at all.
//!
//! ## ★ The two directions are NOT symmetric, and that is the whole finding
//!
//! ```text
//!   PROST    tag 1 → tag 8    PERMISSIVE   an entry that arrives via tag 1 sits
//!                                          W ≥ 3 levels below the decode root, so
//!                                          the outer decode already proved
//!                                          W + L(e) ≤ 100. The escape arm
//!                                          re-decodes it with a FRESH budget of
//!                                          100 and spends L(e) ≤ 97.
//!                                          ⇒ strictly more headroom, and the
//!                                            headroom GROWS with nesting depth,
//!                                            because a `bytes` field costs prost
//!                                            zero message levels.
//!
//!   BINCODE  Vec<Par> → U(m)  RESTRICTIVE  the cold store is iterative and
//!                                          depth-unlimited in BOTH directions
//!                                          today. Reading keys instead of terms
//!                                          puts prost's decoder back in the path
//!                                          for ¬eval_stable entries.
//! ```
//!
//! ## ★★ Anti-vacuity
//!
//! Every ceiling here is **searched**, never transcribed. A test that asserts a
//! constant someone typed measures the typist. [`escape_arm_ceiling`] walks the
//! depth axis until the codec actually refuses and returns the last accepting
//! depth, so if the ceiling moves, this file reports the new one instead of
//! going quietly green against a stale number.
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
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn gint(value: i64) -> Par {
    expr_carrier(ExprInstance::GInt(value))
}

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

/// The greatest `depth` for which `decode_trie_path ∘ encode_trie_path` is the
/// identity on [`escaped_nest`] — **searched**, so a moved ceiling is reported
/// rather than silently passed.
fn escape_arm_ceiling(limit: usize) -> usize {
    let mut last_accepting = 0;
    for depth in 0..=limit {
        let par = escaped_nest(depth);
        let key = encode_trie_path(&par);
        assert_eq!(
            key.first(),
            Some(&tag::ESCAPE),
            "control: depth {depth} must take the ESCAPE arm, or this measures the split arm"
        );
        match decode_trie_path(&key) {
            Ok(round_tripped) if round_tripped == par => last_accepting = depth,
            _ => return last_accepting,
        }
    }
    panic!(
        "no refusal up to depth {limit}: the escape arm looks unbounded, which \
         contradicts prost's RECURSION_LIMIT — widen the search or re-derive the claim"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. The codec boundary, searched
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_escape_arm_ceiling_is_searched_not_transcribed() {
    let ceiling = escape_arm_ceiling(400);

    // The negative control: one level past the ceiling must actually refuse,
    // and it must refuse for prost's reason rather than a grammar rejection.
    let past = escaped_nest(ceiling + 1);
    let past_key = encode_trie_path(&past);
    assert!(
        !past_key.is_empty(),
        "encode_trie_path is TOTAL — it produces a key even past the read ceiling"
    );
    assert!(
        decode_trie_path(&past_key).is_err(),
        "depth {} must refuse — it is one past the searched ceiling {ceiling}",
        ceiling + 1
    );

    // The positive control: the ceiling itself round-trips.
    let at = escaped_nest(ceiling);
    assert_eq!(
        decode_trie_path(&encode_trie_path(&at)).expect("the ceiling depth round-trips"),
        at,
        "the searched ceiling must be ACCEPTING, or the search is off by one"
    );

    println!("MEASURED escape-arm ceiling: last accepting depth = {ceiling}");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. The domain theorem, executable
// ─────────────────────────────────────────────────────────────────────────────

/// ★ The prost move is PERMISSIVE, and this is the proof rather than the claim.
///
/// An entry that reaches an `EPathMap` through prost arrives via `ps` (tag 1),
/// which sits `W ≥ 3` levels below the decode root (`Par.exprs→Expr`,
/// `Expr.e_pathmap_body→EPathMap`, `EPathMap.ps→Par`). If that outer decode
/// succeeded then `W + L(e) ≤ 100`, so `L(e) ≤ 97`. The escape arm re-decodes
/// the same entry from its own bytes with a **fresh** budget of 100.
///
/// ⇒ every entry tag 1 can deliver, the escape arm can read — with headroom.
/// The observable consequence is the asymmetry asserted here: the depth at
/// which a bare entry stops decoding *standalone* is strictly greater than the
/// depth at which the same entry stops arriving *inside a map*.
#[test]
fn the_escape_arm_reads_deeper_than_tag_one_can_deliver() {
    use models::rhoapi::EPathMap;
    use prost::Message;

    let standalone_ceiling = escape_arm_ceiling(400);

    // The greatest depth at which an entry survives prost's decode while nested
    // inside an EPathMap's tag-1 field walk — searched the same way.
    let mut ingress_ceiling = 0;
    for depth in 0..=standalone_ceiling {
        let map = EPathMap::new(vec![escaped_nest(depth)], Vec::new(), false, None);
        let bytes = <EPathMap as Message>::encode_to_vec(&map);
        match <EPathMap as Message>::decode(bytes.as_slice()) {
            Ok(_) => ingress_ceiling = depth,
            Err(_) => break,
        }
    }

    assert!(
        ingress_ceiling < standalone_ceiling,
        "★ THE DOMAIN THEOREM FAILED. The escape arm must read STRICTLY deeper \
         than tag 1 can deliver, because it re-decodes with a fresh budget while \
         tag 1 spends W ≥ 3 levels of the outer decode's budget first. Measured \
         ingress = {ingress_ceiling}, standalone = {standalone_ceiling}. If these \
         coincide, the prost move is NOT permissive and the register entry's \
         acceptance axis is wrong."
    );

    println!(
        "MEASURED prost: tag-1 ingress ceiling = {ingress_ceiling}, escape-arm ceiling = \
         {standalone_ceiling}, headroom = {}",
        standalone_ceiling - ingress_ceiling
    );
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

/// ★ The question the register entry turns on: can a **deploy** carry an entry
/// past the escape arm's read ceiling?
///
/// If it can, the bincode move is restrictive on a class that real Rholang
/// reaches, and the entry must say so and name Phase 4 S2 as the closure. If it
/// cannot, the restriction is unreachable through the compiler and the entry
/// says that instead. Either way the answer is MEASURED here rather than
/// assumed, and it is reported on stdout so the number lands in the report.
#[test]
fn ordinary_rholang_reaches_past_the_escape_arm_ceiling() {
    let ceiling = escape_arm_ceiling(400);
    let probe_depth = ceiling + 8;

    let source = deep_pathmap_source(probe_depth);
    let normalized = Compiler::source_to_adt(&source);

    let Ok(par) = normalized else {
        println!(
            "MEASURED reachability: the COMPILER refused depth {probe_depth} \
             (ceiling {ceiling}) — the restrictive class is not reachable through \
             this surface syntax at this depth"
        );
        return;
    };

    // Dig out the map's single entry.
    let entry = par
        .exprs
        .iter()
        .find_map(|expr| match expr.expr_instance.as_ref() {
            Some(ExprInstance::EPathmapBody(map)) => map.ps().first().cloned(),
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

    let decodes = decode_trie_path(&key).is_ok();
    println!(
        "MEASURED reachability: ordinary Rholang compiled a ¬eval_stable pathmap \
         entry at probe depth {probe_depth} (escape-arm ceiling {ceiling}); its \
         trie key decodes = {decodes}"
    );

    assert!(
        !decodes,
        "★ the probe was built {} levels past the measured ceiling {ceiling} and \
         still decoded — the ceiling search and this fixture disagree about depth, \
         so one of them is not measuring term nesting",
        probe_depth - ceiling
    );
}
