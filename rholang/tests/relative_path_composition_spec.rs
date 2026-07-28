//! ★ THE RELATIVE-PATH COMPOSITION LAW, and the READ-SIDE IMAGE INVARIANT.
//!
//! # The law
//!
//! A zipper cursor is a pair `(segments, kind)`: `EZipper.current_path` holds
//! the per-element codec segments and `EZipper.cursor_kind` holds the codec ARM
//! (`models::rust::pathmap_integration::CursorKind`). `atPath(p)` and
//! `descendTo(p)` both COMPOSE a relative path `p` onto that cursor, and the
//! question this file pins is which arm the COMPOSED cursor takes:
//!
//! ```math
//! \mathrm{kind}(\mathrm{cursor} \frown p) \;=\;
//!   \begin{cases}
//!     \mathrm{CursorKind::of}(p) & \text{if the cursor is empty (the root),}\\
//!     \mathrm{Split}             & \text{otherwise.}
//!   \end{cases}
//! ```
//!
//! # Why the second arm is forced, and is not a choice
//!
//! `encode_trie_path` emits exactly two shapes (`canonical_path.rs`
//! `push_path_ops`):
//!
//! ```text
//!   split arm   concat(per-element segments) ++ 0x00     — a ground EList
//!   bare arm    ONE segment, no terminator               — everything else
//! ```
//!
//! so **a bare entry's key is exactly one segment long**. A cursor of `n`
//! segments can therefore name a bare entry only when `n == 1`; for `n ≥ 2` the
//! unterminated concatenation `concat(segments)` is in the image of no `Par` at
//! all — `decode_trie_path` rejects it as `UnterminatedMultiSegment` — and a
//! `map.get` with it therefore MISSES ON EVERY MAP, whatever the map holds.
//!
//! Below the root the cursor contributes at least one segment and a non-empty
//! relative path contributes at least one more, so the composed path has `n ≥ 2`
//! and the terminated key is the only key it can have. (A relative `[]`
//! contributes zero segments, and `CursorKind::of([])` is already `Split`, so
//! the two arms agree there.)
//!
//! # The defect this file was written against (#108)
//!
//! `entry_key_at` passed the ARGUMENT's arm as the COMPOSED cursor's arm, so a
//! BARE relative argument below the root produced an unterminated multi-segment
//! key:
//!
//! ```text
//!   {| ["a",5], ["a","x"] |}
//!
//!   readZipper().atPath(["a",5])        04 01 61  03 0a  00     ✓ ["a",5]
//!   readZipperAt(["a"]).atPath(5)       04 01 61  03 0a         ✗ Nil
//!                                                        ↑ no terminator:
//!                                                          in the image of no Par
//!   readZipperAt(["a"]).atPath(["x"])   04 01 61  04 01 78  00  ✓ ["a","x"]
//! ```
//!
//! The middle row is not "a lookup that happened to miss": it is a lookup that
//! CANNOT hit, on any map, ever. All three rows are pinned below, on ONE tree —
//! a repair that pins only the failing row would repeat the mistake the guard
//! `entry_key_below_the_root_still_rebuilds` made, which covered only the list
//! arm of the very function it was written for.
//!
//! # The read-side image invariant
//!
//! The write-side twin of this file is `trie_entry_invariant_spec.rs`
//! (`∀ (k,v) ∈ m . encode_trie_path(v) = k`), which catches a PRODUCER that
//! files a value under a key the value does not encode to. It cannot see this
//! defect, because nothing was produced: the trie was perfect and the READER
//! asked it a question in a language it does not speak. The dual invariant is
//!
//! ```math
//! \forall\ \text{entry keys } k \text{ a reader constructs} .\quad
//!   \mathrm{decode\_trie\_path}(k) \in \mathrm{Ok}
//! ```
//!
//! — every key a reader builds is in the image of `encode_trie_path`. It is
//! checked at `cursor_entry_key`, the single point every entry key passes
//! through, under `#[cfg(debug_assertions)]`; the exhaustive model-level twin is
//! `models/tests/pathmap_integration_tests.rs::every_reader_key_is_in_the_codec_image`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
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

/// The Par a program sent to `@"out"` — the value as a Rholang program observes
/// it, having crossed the tuplespace.
async fn out_par(prefix: &str, program: &str) -> Par {
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
        pars[0].clone()
    })
    .await
}

/// The fixture every row of the matrix reads: ONE tree holding a split entry
/// whose LAST element is bare-shaped (`5`) and one whose last element is a
/// string. Both are ground lists, so both are split-arm entries — the
/// bare/split question below is about the ARGUMENT, not about the tree.
const TREE: &str = r#"{| ["a", 5], ["a", "x"] |}"#;

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE MATRIX — all three rows, one tree
// ─────────────────────────────────────────────────────────────────────────────

/// ROW 1 — the ROOT arm with a whole-path argument. `entry_key_at` asks the
/// codec directly here, so this row has always been right and is the byte
/// stability anchor: if a repair moves it, the repair moved the root.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn row1_root_cursor_with_the_whole_path_finds_the_entry() {
    let value = out_par(
        "relpath-row1-",
        &format!(r#"@"out"!( {TREE}.readZipper().atPath(["a", 5]) )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gint(5)]),
        "readZipper().atPath([\"a\",5]) must find [\"a\",5]"
    );
}

/// ★ ROW 2 — **THE DEFECT.** A BARE relative argument below the root. The
/// composed path is `["a", 5]`, which is exactly the entry row 1 found from the
/// root; the only difference is where the cursor was standing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn row2_bare_relative_argument_below_the_root_finds_the_entry() {
    let value = out_par(
        "relpath-row2-",
        &format!(r#"@"out"!( {TREE}.readZipperAt(["a"]).atPath(5) )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gint(5)]),
        "readZipperAt([\"a\"]).atPath(5) must find [\"a\",5] — the same entry \
         row 1 finds from the root. A bare relative argument below the root \
         built an UNTERMINATED multi-segment key, which is in the image of no \
         Par, so the lookup could not hit on any map"
    );
}

/// ROW 3 — a LIST relative argument below the root: the arm that already
/// worked, and the arm the pre-existing guard covered. It must stay green on
/// the same tree that makes row 2 red.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn row3_list_relative_argument_below_the_root_finds_the_entry() {
    let value = out_par(
        "relpath-row3-",
        &format!(r#"@"out"!( {TREE}.readZipperAt(["a"]).atPath(["x"]) )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gstring("x")]),
        "readZipperAt([\"a\"]).atPath([\"x\"]) must find [\"a\",\"x\"]"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The same law reached by the OTHER composer: descendTo
// ─────────────────────────────────────────────────────────────────────────────

/// `descendTo` writes the composed cursor's arm into `EZipper.cursor_kind`, so
/// it carries the same law and had the same copy of it. Two descents — a list
/// then a BARE element — must name the entry a single `atPath` of the whole
/// path names.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn descend_to_a_bare_element_below_the_root_names_the_same_entry() {
    let value = out_par(
        "relpath-descend-bare-",
        &format!(r#"@"out"!( {TREE}.readZipper().descendTo(["a"]).descendTo(5).getLeaf() )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gint(5)]),
        "readZipper().descendTo([\"a\"]).descendTo(5).getLeaf() must find [\"a\",5]"
    );
}

/// The mixed composer: a cursor from `readZipperAt`, a descent by a bare
/// element, read with `getLeaf`. `readZipperAt(p)` and
/// `readZipper().descendTo(p)` must name the same entry, so this and the test
/// above must agree.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_zipper_at_then_descend_to_a_bare_element_names_the_same_entry() {
    let value = out_par(
        "relpath-at-descend-bare-",
        &format!(r#"@"out"!( {TREE}.readZipperAt(["a"]).descendTo(5).getLeaf() )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gint(5)]),
        "readZipperAt([\"a\"]).descendTo(5).getLeaf() must find [\"a\",5]"
    );
}

/// `descendTo` by a LIST below the root — the arm that already worked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn descend_to_a_list_below_the_root_names_the_same_entry() {
    let value = out_par(
        "relpath-descend-list-",
        &format!(r#"@"out"!( {TREE}.readZipper().descendTo(["a"]).descendTo(["x"]).getLeaf() )"#),
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a"), gstring("x")]),
        "readZipper().descendTo([\"a\"]).descendTo([\"x\"]).getLeaf() must find [\"a\",\"x\"]"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ⚠ THE ROW THAT MUST NOT MOVE — why `cursor_entry_key` keeps its `CursorKind`
// ─────────────────────────────────────────────────────────────────────────────
//
// A depth-1 cursor CAN name a bare entry: `readZipperAt(5)` stands on the bare
// entry `5`, whose key is one segment with no terminator, and the singleton
// list `[5]` — key `03 0a 00` — is a DIFFERENT entry that the same map may hold
// at the same time. Removing the discriminator from `cursor_entry_key`, or
// terminating unconditionally at the caller, would merge these two.

/// `readZipperAt(5)` and `readZipperAt([5])` name DIFFERENT entries of one map.
/// This is the live behaviour that forces `cursor_entry_key` to keep taking a
/// `CursorKind`: at depth 1 the arm is real information, and only at depth ≥ 2
/// is `Bare` uninhabited.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_depth_one_cursor_still_tells_the_bare_entry_from_the_singleton_list() {
    let bare = out_par(
        "relpath-depth1-bare-",
        r#"@"out"!( {| 5, [5] |}.readZipperAt(5).getLeaf() )"#,
    )
    .await;
    assert_eq!(bare, gint(5), "readZipperAt(5) names the BARE entry 5");

    let singleton = out_par(
        "relpath-depth1-list-",
        r#"@"out"!( {| 5, [5] |}.readZipperAt([5]).getLeaf() )"#,
    )
    .await;
    assert_eq!(
        singleton,
        list(vec![gint(5)]),
        "readZipperAt([5]) names the SINGLETON LIST [5]"
    );
}

/// The root arm of `atPath` tells the same two entries apart — the whole-path
/// case, where `CursorKind::of(argument)` IS the composed arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_root_arm_still_tells_the_bare_entry_from_the_singleton_list() {
    let bare = out_par("relpath-root-bare-", r#"@"out"!( {| 5, [5] |}.atPath(5) )"#).await;
    assert_eq!(
        bare,
        gint(5),
        "atPath(5) at the root names the BARE entry 5"
    );

    let singleton = out_par(
        "relpath-root-list-",
        r#"@"out"!( {| 5, [5] |}.atPath([5]) )"#,
    )
    .await;
    assert_eq!(
        singleton,
        list(vec![gint(5)]),
        "atPath([5]) at the root names the SINGLETON LIST [5]"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The composed path is a LIST — stated, not discovered
// ─────────────────────────────────────────────────────────────────────────────

/// Below a BARE cursor, composing by a bare element still yields a LIST entry:
/// `{| 5, [5, "x"] |}.readZipperAt(5).atPath("x")` is `[5,"x"]`. The cursor
/// stood on the bare entry `5`, and descending from it produced a two-element
/// path, which has no bare form.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn composing_below_a_bare_cursor_yields_the_list_entry() {
    let value = out_par(
        "relpath-below-bare-cursor-",
        r#"@"out"!( {| 5, [5, "x"] |}.readZipperAt(5).atPath("x") )"#,
    )
    .await;
    assert_eq!(
        value,
        list(vec![gint(5), gstring("x")]),
        "readZipperAt(5).atPath(\"x\") must find [5,\"x\"]"
    );
}

/// An EMPTY relative path below the root names the cursor's own path AS A LIST:
/// `readZipperAt(["a"]).atPath([])` is `["a"]`. This is the one composition
/// where the argument's own arm and the composed arm already agreed (both
/// `Split`), and it pins that the repair did not disturb it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_relative_path_names_the_cursor_path_itself() {
    let value = out_par(
        "relpath-empty-",
        r#"@"out"!( {| ["a"], ["a", 5] |}.readZipperAt(["a"]).atPath([]) )"#,
    )
    .await;
    assert_eq!(
        value,
        list(vec![gstring("a")]),
        "readZipperAt([\"a\"]).atPath([]) must find [\"a\"]"
    );
}
