//! ★ THE TRIE ENTRY INVARIANT, over the tries the INTERPRETER produces.
//!
//! ```math
//! \forall (k, v) \in m .\quad \mathrm{encode\_trie\_path}(v) = k
//! ```
//!
//! # What the invariant is, and why the whole read side rests on it
//!
//! A `RholangPathMap` is not a general map. It is a *set of Par entries indexed
//! by their own codec path*: `create_pathmap_from_elements`
//! (`models/src/rust/pathmap_integration.rs`) writes
//! `map.insert(encode_trie_path(par), par.clone())`, so **the value is a
//! redundant mirror of the key** — as `reduce.rs` states it at the `setSubtrie`
//! step-3b insert, *"A PathMap entry is both its own key and its own value."*
//!
//! That redundancy is spent in two OPPOSITE directions by two readers:
//!
//! ```text
//!                          ┌────────────────────────────────┐
//!                          │   RholangPathMap  (k ↦ v)      │
//!                          └───────────┬────────────────────┘
//!            reads the VALUE           │          reads the KEYS
//!            AT ONE KEY                │      (to_next_val + decode_trie_path)
//!         (PathMap::get)               │                 ▼
//!                    ▼                 │       canonical_ps_from_trie
//!   getLeaf (reduce.rs), the fused     │       → EVERY EPathMap the reducer
//!   chain's get, values_with_prefix    │         hands back to a program, AND
//!   (pathmap_native_query.rs)          │         the serde / EVENT-HASH preimage
//! ```
//!
//! The two agree on a map **exactly when this invariant holds**, and there is
//! no other reason for them to agree. A producer that files a value under a key
//! the value does not encode to therefore makes a program's `getLeaf` disagree
//! with the same program's enumeration of the same map.
//!
//! # ⚠ What changed, and why this file is still load-bearing
//!
//! The left-hand side of that diagram used to be
//! `rholang_pathmap_to_e_pathmap`, the BULK converter behind every
//! `EPathMap`-returning method in the reducer. It walked `PathMap::iter()` and
//! dropped the keys. It now walks the keys, so the two BULK readers have become
//! one and **cannot disagree — there is no second bulk reader to be wrong**
//! (defects #89 and #91 are unrepresentable, not fixed).
//!
//! What survives on the value side is the **point lookup**: `getLeaf`, the fused
//! chain's `get`, and `values_with_prefix` all return `map.get(key)`. Those
//! answers equal `decode_trie_path(key)` only while this invariant holds, so the
//! invariant is not vacuous and this file is not obsolete — its subject has
//! narrowed from "two bulk traversals" to "the point lookups versus the bulk
//! one".
//!
//! # Why the failure is entry LOSS, not merely disagreement
//!
//! A value read is lossy under re-insertion. Two DISTINCT keys carrying the SAME
//! value yield two identical entries — the cardinality still looks right — and
//! the very next `e_pathmap_to_rholang_pathmap` re-keys both to
//! `encode_trie_path(v)` and merges them:
//!
//! ```text
//!   k₁ ↦ v          ps = [v, v]        encode_trie_path(v) ↦ v
//!   k₂ ↦ v   ────▶  (len 2 — looks    ────▶   ONE entry.
//!                    correct!)                 ENTRIES ARE LOST.
//! ```
//!
//! This is why every cardinality assertion on `ps` is blind to the defect, and
//! why the assertions in this file are on ENTRIES.
//!
//! # How this file checks "every trie the interpreter produces"
//!
//! Two layers, both required:
//!
//! 1. **The guard.** `rholang_pathmap_to_e_pathmap` is the single point every
//!    trie in the system passes through on its way back to a value, and it
//!    asserts the invariant under `#[cfg(debug_assertions)]` (release builds —
//!    consensus nodes — compile it out). Every test in the workspace therefore
//!    exercises the invariant on every map it touches, and a producer bug is
//!    named at the first moment it is visible rather than at the distant point
//!    where entries go missing. The guard SURVIVED the converter's move to the
//!    key side — deleting it would have been tidy and wrong, because the point
//!    lookups it now protects still read values.
//! 2. **The matrix below.** The guard proves nothing about a case no test
//!    drives, and until this file existed *no test drove a `setSubtrie` whose
//!    source held a non-list entry* — `rholang/tests/setsubtrie_spec.rs` has
//!    five tests, every one of which asserts only that evaluation raised no
//!    errors, and every source entry in all of them is a ground list. The
//!    matrix drives BOTH codec arms on both sides of the composition and
//!    asserts the resulting ENTRIES.
//!
//! The model-level twin (the invariant over every trie the `models` crate can
//! build, exhaustively) is
//! `models/tests/pathmap_integration_tests.rs::every_trie_this_crate_builds_upholds_the_entry_invariant`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::{
    create_pathmap_from_elements, render_trie_entry_divergences, trie_entry_divergences,
};
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

// ─────────────────────────────────────────────────────────────────────────────
// Driving the real reducer and reading an EPathMap back out
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluate `program`, then return the `EPathMap` sent to `@"out"`.
///
/// The value comes back through the tuplespace, so it has been normalized,
/// sorted, and stored exactly as any program-visible map is — this is the map a
/// Rholang program would observe, not an internal artefact.
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

/// The entries of a map, ordered by their codec key. Duplicates are KEPT — two
/// entries that are the same Par are exactly the symptom being hunted, so
/// collapsing them here would recreate the blindness this file exists to
/// remove.
fn entries_by_key(entries: &[Par]) -> Vec<(Vec<u8>, Par)> {
    let mut keyed: Vec<(Vec<u8>, Par)> = entries
        .iter()
        .map(|par| (encode_trie_path(par), par.clone()))
        .collect();
    keyed.sort_by(|(left, _), (right, _)| left.cmp(right));
    keyed
}

/// Assert that `actual` holds EXACTLY `expected` as an entry set, and that the
/// entries survive a re-insertion (i.e. no two of them are the same entry).
#[track_caller]
fn assert_entries(case: &str, actual: &EPathMap, expected: &[Par]) {
    let actual_entries: Vec<Par> = actual.ps.iter().cloned().collect();

    assert_eq!(
        entries_by_key(&actual_entries),
        entries_by_key(expected),
        "{case}: the map's ENTRIES are wrong\n  actual   = {actual_entries:?}\n  expected = {expected:?}"
    );

    // The re-insertion leg: a map holding two copies of one entry has already
    // lost an entry, and `ps.len()` cannot see it.
    let rebuilt = create_pathmap_from_elements(&actual_entries, None);
    assert_eq!(
        rebuilt.map.val_count(),
        expected.len(),
        "{case}: {} entries came back but only {} survive re-insertion — \
         distinct keys shared a value and the map has lost entries",
        actual_entries.len(),
        rebuilt.map.val_count()
    );

    // …and the rebuilt trie upholds the invariant (it must: it was built by the
    // codec — this is the anchor that makes the comparison above meaningful).
    let divergences = trie_entry_divergences(&rebuilt.map);
    assert!(
        divergences.is_empty(),
        "{case}: {}",
        render_trie_entry_divergences(&divergences)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE MATRIX — both codec arms on both sides of the `setSubtrie` composition
// ─────────────────────────────────────────────────────────────────────────────
//
// `setSubtrie` composes a cursor path with each source entry's RELATIVE path:
//
//     key(entry) = concat(cursor segments) ‖ concat(source segments) ‖ 0x00
//
// The key route asks `par_to_path` — the codec's own split/bare classifier —
// which yields one segment per element for a ground-list carrier and ONE
// segment for anything else. The value stored under that key must therefore be
// the ground list of the cursor's elements followed by the source entry's
// elements, where a BARE source entry contributes ITSELF as a single element.
//
// The composed key always carries the `0x00` terminator, so it always names a
// ground LIST; that is what makes the value a list even when the source entry
// was not one, and it is why the root-cursor case below is a `witness_`.

/// A single-entry source on the BARE arm. The key gains the element's segment;
/// the value must gain the element.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_with_one_bare_source_entry_keeps_the_element() {
    let map = out_pathmap(
        "trie-inv-bare-one-",
        r#"@"out"!( {| ["root", "old"] |}.writeZipperAt(["root"]).setSubtrie({| 5 |}) )"#,
    )
    .await;

    assert_entries("setSubtrie({| 5 |}) at [\"root\"]", &map, &[list(vec![
        gstring("root"),
        gint(5),
    ])]);
}

/// ★ **THE ENTRY-LOSS CASE.** Two BARE source entries produce two DISTINCT
/// keys. If the value route does not follow the key route, both keys receive
/// the SAME value — the cursor path alone — and the two entries collapse into
/// one the moment the map is re-inserted anywhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_with_two_bare_source_entries_keeps_both() {
    let map = out_pathmap(
        "trie-inv-bare-two-",
        r#"@"out"!( {| ["root", "old"] |}.writeZipperAt(["root"]).setSubtrie({| 5, 7 |}) )"#,
    )
    .await;

    assert_entries("setSubtrie({| 5, 7 |}) at [\"root\"]", &map, &[
        list(vec![gstring("root"), gint(5)]),
        list(vec![gstring("root"), gint(7)]),
    ]);
}

/// Both arms in ONE source map, including the `5` / `[5]` pair whose keys
/// differ by exactly the terminator. Four source entries must produce four
/// distinct absolute entries.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_with_a_mixed_source_keeps_every_arm() {
    let map = out_pathmap(
        "trie-inv-mixed-",
        r#"@"out"!( {| ["root", "old"] |}.writeZipperAt(["root"]).setSubtrie({| 5, [5], "a", ["a", "x"] |}) )"#,
    )
    .await;

    assert_entries(
        "setSubtrie({| 5, [5], \"a\", [\"a\",\"x\"] |}) at [\"root\"]",
        &map,
        &[
            // bare `5` and split `[5]` compose to the SAME absolute path — the
            // one place the two arms legitimately meet, because the composed
            // key is terminated either way. Four source entries, three
            // absolute entries, and the pair that merges is stated here rather
            // than discovered.
            list(vec![gstring("root"), gint(5)]),
            list(vec![gstring("root"), gstring("a")]),
            list(vec![gstring("root"), gstring("a"), gstring("x")]),
        ],
    );
}

/// A BARE cursor: `writeZipperAt(5)` names the bare entry `5`, and writing
/// below it composes onto that element. The cursor's own arm is recovered from
/// the composed key, so the absolute entry is `[5, "x"]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_below_a_bare_cursor_composes_onto_the_element() {
    let map = out_pathmap(
        "trie-inv-bare-cursor-",
        r#"@"out"!( {| 5, ["other"] |}.writeZipperAt(5).setSubtrie({| ["x"], 9 |}) )"#,
    )
    .await;

    assert_entries("setSubtrie({| [\"x\"], 9 |}) at bare cursor 5", &map, &[
        // `["other"]` is untouched — its key does not carry the `enc(5)`
        // prefix that step 2 removes.
        list(vec![gstring("other")]),
        list(vec![gint(5), gstring("x")]),
        list(vec![gint(5), gint(9)]),
    ]);
}

/// The SPLIT arm, unchanged: this is the shape every pre-existing
/// `setsubtrie_spec.rs` fixture uses, asserted by CONTENT here for the first
/// time. It is the byte-stability check for the fix — if the repair moved the
/// ground-list corpus, this is what says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_with_split_source_entries_is_unchanged() {
    let map = out_pathmap(
        "trie-inv-split-",
        r#"@"out"!(
             {| ["root", "a", "x"], ["root", "a", "y"], ["root", "b", "z"] |}
               .writeZipperAt(["root", "a"])
               .setSubtrie({| ["new1"], ["new2", "deep"] |})
           )"#,
    )
    .await;

    assert_entries("setSubtrie of ground lists at [\"root\",\"a\"]", &map, &[
        list(vec![gstring("root"), gstring("a"), gstring("new1")]),
        list(vec![
            gstring("root"),
            gstring("a"),
            gstring("new2"),
            gstring("deep"),
        ]),
        list(vec![gstring("root"), gstring("b"), gstring("z")]),
    ]);
}

/// An EMPTY source list among the source entries composes to the cursor path
/// itself — the step-3 twin of what step 3b does for an entirely empty source.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_subtrie_with_an_empty_source_entry_names_the_cursor_path() {
    let map = out_pathmap(
        "trie-inv-empty-entry-",
        r#"@"out"!( {| ["root", "old"] |}.writeZipperAt(["root"]).setSubtrie({| [], 5 |}) )"#,
    )
    .await;

    assert_entries("setSubtrie({| [], 5 |}) at [\"root\"]", &map, &[
        list(vec![gstring("root")]),
        list(vec![gstring("root"), gint(5)]),
    ]);
}

/// ⚠ WITNESS — at the ROOT cursor the composed key is still terminated, so a
/// BARE source entry is written as its SINGLETON LIST: `setSubtrie({| 5 |})` on
/// a root write-zipper yields `{| [5] |}`, not `{| 5 |}`.
///
/// This is a KEY-side question, not a value-side one, and it is deliberately
/// out of the scope of the entry-invariant repair: the invariant HOLDS here
/// (`encode_trie_path([5])` is exactly the key written), and no entries are
/// lost. Changing it would mean giving the step-3 key route an arm-preserving
/// composition (`entry_key_at`'s root arm), which moves consensus-visible bytes
/// for a second, independent reason and is a separate decision.
///
/// It is pinned so the behaviour is a checked fact rather than an accident, and
/// so that a future arm-preserving composition shows up here as a deliberate
/// change rather than as silence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn witness_set_subtrie_at_the_root_wraps_a_bare_source_entry() {
    let map = out_pathmap(
        "trie-inv-root-",
        r#"@"out"!( {| ["old"] |}.writeZipper().setSubtrie({| 5, ["keep"] |}) )"#,
    )
    .await;

    assert_entries("setSubtrie({| 5, [\"keep\"] |}) at the root", &map, &[
        // ★ `[5]`, not `5` — the composed key carries the terminator.
        list(vec![gint(5)]),
        list(vec![gstring("keep")]),
    ]);
}

// ─────────────────────────────────────────────────────────────────────────────
// The guard's own coverage — that the invariant check is REACHED
// ─────────────────────────────────────────────────────────────────────────────

/// The guard in `rholang_pathmap_to_e_pathmap` is only worth anything if the
/// reducer actually routes through it. A `setSubtrie` result is produced by
/// that converter, so the map returned above passed the check — and this test
/// says so by re-running the check on the value the program observed, which is
/// the same statement from the outside.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_map_the_reducer_returns_upholds_the_invariant() {
    for (prefix, program) in [
        (
            "trie-inv-guard-a-",
            r#"@"out"!( {| ["root", "old"] |}.writeZipperAt(["root"]).setSubtrie({| 5, 7, ["x"] |}) )"#,
        ),
        (
            "trie-inv-guard-b-",
            r#"@"out"!( {| 1, 2, 3 |}.writeZipperAt(1).setSubtrie({| 8, ["y", "z"] |}) )"#,
        ),
        (
            "trie-inv-guard-c-",
            r#"@"out"!( {| ["a"], ["b"] |}.union({| 5, ["a", "deep"] |}) )"#,
        ),
        (
            "trie-inv-guard-d-",
            r#"@"out"!( {| ["a", "x"], ["a", "y"], 5 |}.restriction({| ["a"] |}) )"#,
        ),
    ] {
        let map = out_pathmap(prefix, program).await;
        let entries: Vec<Par> = map.ps.iter().cloned().collect();
        let rebuilt = create_pathmap_from_elements(&entries, None);
        let divergences = trie_entry_divergences(&rebuilt.map);
        assert!(
            divergences.is_empty(),
            "{program}:{}",
            render_trie_entry_divergences(&divergences)
        );
        assert_eq!(
            rebuilt.map.val_count(),
            entries.len(),
            "{program}: the returned map holds {} entries but only {} are \
             distinct — entries were lost before the program ever saw it",
            entries.len(),
            rebuilt.map.val_count()
        );
        // The converter is the guard's home; running the returned map back
        // through it exercises the check on a value that has crossed the
        // tuplespace.
        let _ =
            PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(&rebuilt.map, false, &[], None);
    }
}
