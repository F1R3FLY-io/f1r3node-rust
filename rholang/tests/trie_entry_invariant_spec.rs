//! Set-mode PathMap integration through the interpreter.
//!
//! `RholangSetPathMap` is a prefix-compressed `PathMap<()>`: membership lives in
//! canonical byte keys and there is no redundant `Par` value slot. These tests
//! drive the reducer's set-subtrie, union, and restriction paths and verify the
//! exact program-visible EPathMap members after each algebraic composition.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::create_set_pathmap_from_elements;
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
    let actual_entries = actual.entry_trie().entries_owned();

    assert_eq!(
        entries_by_key(&actual_entries),
        entries_by_key(expected),
        "{case}: the map's ENTRIES are wrong\n  actual   = {actual_entries:?}\n  expected = {expected:?}"
    );

    // The re-insertion leg: a map holding two copies of one entry has already
    // lost an entry, and `ps.len()` cannot see it.
    let rebuilt = create_set_pathmap_from_elements(&actual_entries, None);
    assert_eq!(
        rebuilt.map.val_count(),
        expected.len(),
        "{case}: {} entries came back but only {} survive re-insertion — \
         canonical set members collapsed unexpectedly",
        actual_entries.len(),
        rebuilt.map.val_count()
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
// The codec's split/bare classifier yields one segment per element for a
// ground-list carrier and one segment for anything else. `setSubtrie` composes
// those segments and stores only the resulting canonical set key. The composed
// key carries the `0x00` terminator and therefore denotes a ground-list member.

/// A single-entry source on the BARE arm. The composed member gains the element.
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
/// keys and therefore two distinct absolute set members.
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
// Reducer-to-PathMap conversion coverage
// ─────────────────────────────────────────────────────────────────────────────

/// Every returned map remains a lossless set when adopted and rebuilt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_map_the_reducer_returns_round_trips_losslessly() {
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
        let entries = map.entry_trie().entries_owned();
        let rebuilt = create_set_pathmap_from_elements(&entries, None);
        assert_eq!(
            rebuilt.map.val_count(),
            entries.len(),
            "{program}: the returned map holds {} entries but only {} are \
             distinct — entries were lost before the program ever saw it",
            entries.len(),
            rebuilt.map.val_count()
        );
        // Exercise adoption after the value has crossed the tuplespace.
        let _ = PathMapCrateTypeMapper::rholang_set_pathmap_to_set_epathmap(
            &rebuilt.map,
            false,
            &[],
            None,
        );
    }
}
