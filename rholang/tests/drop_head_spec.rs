//! ★ `dropHead(n)` — "remove the first `n` segments from all paths"
//! (`docs/rholang/07-pathmaps-and-zippers.md`), stated over BOTH codec arms.
//!
//! # What a path's LENGTH is
//!
//! `dropHead` needs exactly one thing about each entry: how many elements its
//! path has. The canonical path codec answers that, and there are two arms
//! (`canonical_path.rs` §1.3):
//!
//! ```text
//!   split arm   a ground EList carrier  →  one path element per list element
//!   bare arm    everything else          →  a path of length ONE
//! ```
//!
//! The bare arm covers the plain ground atoms (`5`, `"a"`) AND the `0x0F`
//! escape arm, which is where every entry the codec cannot walk into ends up —
//! including a Par carrying a list ALONGSIDE something else.
//!
//! # The rule, one rule for both arms
//!
//! ```text
//!   |path| ≤ n   the path is exhausted — the entry goes
//!   n = 0        the identity, on both arms
//!   otherwise    the entry becomes the ground list of its path's tail
//! ```
//!
//! The first line is the pre-existing pinned semantics for ground lists —
//! `dropHead(k)` on a k-element path removes it
//! (`models/tests/pathmap_integration_tests.rs::test_drophead_exact_length`) —
//! and applying it to the bare arm is what makes `{| 5 |}.dropHead(1)` empty:
//! `5` is a path of length one, exactly like `["a"]`.
//!
//! The second line is why `dropHead(0)` may not REBUILD an entry: the bare `5`
//! and the singleton list `[5]` are different entries under different keys
//! (`03 0a` and `03 0a 00`), so wrapping `5` in a list would move it.
//!
//! # ⚠ The defect this file was written against (#109 item 1)
//!
//! The loop classified entries with `par.exprs.first()` matching an
//! `EListBody`, which is STRICTLY MORE PERMISSIVE than the codec's
//! `split_carrier_list`. The two therefore disagreed about the length of some
//! paths, and the entry below is the witness — a Par carrying a list AND a
//! send, which a program puts in a map by sending it over a channel:
//!
//! ```text
//!   {| [1,2] | @"d"!(3) |}.dropHead(1)
//!       the loop:  a 2-element path  →  [2] | @"d"!(3)      ✗ wrong answer
//!       the trie:  ONE escaped segment, a 1-element path  →  removed
//! ```
//!
//! It did not drop a path element; it dropped an element from the entry's
//! INTERIOR. This is the same classifier divergence that cost `setSubtrie` its
//! bare source entries (`trie_entry_invariant_spec.rs`), reached by an
//! independent route; the classifier now has ONE definition,
//! `pathmap_integration::path_elements`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

// ─────────────────────────────────────────────────────────────────────────────
// Par constructors — the expectation side, built independently of the reducer
// ─────────────────────────────────────────────────────────────────────────────

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn gstring(value: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value.to_string())),
    }])
}

fn list(ps: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

/// Evaluate `program`, then return the `EPathMap` sent to `@"out"`.
async fn out_pathmap(prefix: &str, program: &str) -> EPathMap {
    let program = program.to_string();
    with_runtime(prefix, move |mut runtime: RhoRuntimeImpl| async move {
        let result = runtime
            .evaluate_with_term(&program)
            .await
            .expect("evaluation must not fail structurally");
        assert!(
            result.errors.is_empty(),
            "evaluation raised interpreter errors: {:?}",
            result.errors
        );

        let data = runtime.get_data(&gstring("out")).await;
        assert_eq!(data.len(), 1, "expected exactly one datum at @\"out\"");
        let pars = &data[0].a.pars;
        assert_eq!(pars.len(), 1, "expected a single Par at @\"out\"");
        match pars[0].exprs.first().and_then(|e| e.expr_instance.clone()) {
            Some(ExprInstance::EPathmapBody(map)) => map,
            other => panic!("@\"out\" expected an EPathMap, got {other:?}"),
        }
    })
    .await
}

/// Assert the map holds EXACTLY `expected` as an entry set, compared by codec
/// key so the comparison is the trie's own notion of identity.
#[track_caller]
fn assert_entries(case: &str, actual: &EPathMap, expected: &[Par]) {
    let key_of = |par: &Par| encode_trie_path(par);
    let actual_entries = actual.entry_trie().entries_owned();
    let mut actual_keys: Vec<Vec<u8>> = actual_entries.iter().map(key_of).collect();
    let mut expected_keys: Vec<Vec<u8>> = expected.iter().map(key_of).collect();
    actual_keys.sort();
    expected_keys.sort();
    assert_eq!(
        actual_keys, expected_keys,
        "{case}: the map's ENTRIES are wrong\n  actual   = {:?}\n  expected = {expected:?}",
        actual_entries
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The SPLIT arm — the ground-list corpus, which must not move a byte
// ─────────────────────────────────────────────────────────────────────────────

/// The documented example (`docs/rholang/07-pathmaps-and-zippers.md`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_head_shortens_every_ground_list_path() {
    let map = out_pathmap(
        "drophead-lists-",
        r#"@"out"!( {| ["books", "gatsby"], ["books", "moby"] |}.dropHead(1) )"#,
    )
    .await;
    assert_entries("dropHead(1) on two 2-element paths", &map, &[
        list(vec![gstring("gatsby")]),
        list(vec![gstring("moby")]),
    ]);
}

/// Paths of different lengths: the ones at or below `n` are exhausted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_head_removes_paths_it_exhausts() {
    let map = out_pathmap(
        "drophead-mixed-lengths-",
        r#"@"out"!( {| ["a","b","c","d"], ["x","y"], ["p","q","r"] |}.dropHead(2) )"#,
    )
    .await;
    assert_entries("dropHead(2) on paths of length 4, 2 and 3", &map, &[
        list(vec![gstring("c"), gstring("d")]),
        list(vec![gstring("r")]),
    ]);
}

// ─────────────────────────────────────────────────────────────────────────────
// The BARE arm — a path of length one
// ─────────────────────────────────────────────────────────────────────────────

/// `dropHead(0)` is the identity on BOTH arms — and in particular it does NOT
/// wrap a bare entry in a list, which would file it under a different key.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_head_zero_is_the_identity_on_both_arms() {
    let map = out_pathmap(
        "drophead-zero-",
        r#"@"out"!( {| 5, "a", ["a","b"], [] |}.dropHead(0) )"#,
    )
    .await;
    assert_entries("dropHead(0)", &map, &[
        gint(5),
        gstring("a"),
        list(vec![gstring("a"), gstring("b")]),
        list(vec![]),
    ]);
}

/// A bare entry is a path of length ONE, so `dropHead(1)` exhausts it — for the
/// same reason, and by the same rule, that it exhausts the 1-element list
/// `["a"]` beside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bare_entry_is_a_path_of_length_one() {
    let map = out_pathmap(
        "drophead-bare-",
        r#"@"out"!( {| 5, ["a"], ["a","b"] |}.dropHead(1) )"#,
    )
    .await;
    assert_entries(
        "dropHead(1) with a bare entry and a 1-element list beside it",
        &map,
        &[list(vec![gstring("b")])],
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE ESCAPE ARM — the entry the two classifiers disagreed about
// ─────────────────────────────────────────────────────────────────────────────

/// ★ **THE DEFECT.** `[1,2] | @"d"!(3)` carries a list AND a send, so the codec
/// gives it the `0x0F` escape arm: ONE segment, a path of length one.
/// `dropHead(1)` must therefore exhaust it, exactly as it exhausts the bare `5`.
///
/// The `exprs.first()` classifier called it a 2-element path and rewrote it to
/// `[2] | @"d"!(3)` — dropping an element from the entry's interior rather than
/// from its path. The list beside it in the same map is the control: it must
/// shorten in the same evaluation that removes this one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_entry_the_codec_escapes_is_a_path_of_length_one() {
    let map = out_pathmap(
        "drophead-escape-",
        r#"@"c"!([1, 2] | @"d"!(3)) |
           for (@entry <- @"c") {
             @"out"!( {| entry, ["a","b"] |}.dropHead(1) )
           }"#,
    )
    .await;
    assert_entries("dropHead(1) with an escape-arm entry", &map, &[list(vec![
        gstring("b"),
    ])]);
}

/// …and `dropHead(0)` keeps that same entry UNCHANGED — the identity arm has to
/// hold for the escape arm too, which is what says the entry was removed above
/// because its path was exhausted and not because the reducer cannot represent
/// it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_escape_arm_entry_survives_drop_head_zero_unchanged() {
    let map = out_pathmap(
        "drophead-escape-zero-",
        r#"@"c"!([1, 2] | @"d"!(3)) |
           for (@entry <- @"c") {
             @"out"!( {| entry, ["a","b"] |}.dropHead(0) )
           }"#,
    )
    .await;

    assert_eq!(map.len(), 2, "both entries survive dropHead(0)");
    let entries = map.entry_trie().entries_owned();
    let escaped = entries
        .iter()
        .find(|par| !par.sends.is_empty())
        .expect("the escape-arm entry survives");
    assert_eq!(
        escaped.exprs.first().and_then(|e| e.expr_instance.as_ref()),
        Some(&ExprInstance::EListBody(EList {
            ps: vec![gint(1), gint(2)],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
        "★ the entry's interior list is UNTOUCHED — dropHead moves paths, not \
         the insides of entries"
    );
}
