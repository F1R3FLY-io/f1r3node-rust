//! Trie ENUMERATION over an EPathMap: `getPath` / `toNextLeaf` / `leafCount`.
//!
//! These three make walking an EPathMap TOTAL — descend to a leaf, read its key
//! AND its value, advance to the next leaf, and know the count in advance:
//!
//! ```text
//!     z = m.readZipper(); n = z.leafCount();
//!     n times: z = z.toNextLeaf(); use z.getPath(), z.getLeaf()
//! ```
//!
//! Each SURFACES capability the `pathmap` crate already has — `ZipperMoving::path()`
//! (read here out of `EZipper.current_path`, which is already wire data),
//! `ZipperIteration::to_next_val()`, and `ZipperMoving::val_count()`.
//!
//! # ★ CROSS-ENDPOINT CONTRACT with mettail's rhocalc — read before landing C1
//!
//! The two runtimes report an exhausted / failed navigation DIFFERENTLY, and each
//! is correct in its own house style:
//!
//! | runtime            | exhausted `toNextLeaf` | how it reads in a program |
//! |--------------------|------------------------|---------------------------|
//! | f1r3node (here)    | `Ok(Par::default())`   | **`Nil`**                 |
//! | mettail rhocalc    | `Err(())`              | the term stays **stuck**  |
//!
//! `Nil` is this reducer's established convention for "no answer" — the same one
//! `descendIndexedBranch` out-of-range and `toNextSibling`-at-the-last-child
//! already use (see `zipper_query_methods_spec.rs`, `idxOut` / `nextSiblingLast`).
//! mettail's stuck form is *its* established convention (user decision
//! 2026-06-30). Neither is being changed; the mismatch is deliberate and is
//! recorded here so it is impossible to miss.
//!
//! ⚠ **REQUIRED TRANSLATION.** C1 — the seam that routes rhocalc collection
//! methods into this reducer's method table — MUST map the `Nil` returned here
//! on exhaustion back to rhocalc's STUCK form. It must not surface `Nil` as a
//! zipper, and it must not let a walk continue on it.
//!
//! Why this is worth two live assertions rather than a paragraph: `to_next_val()`
//! does not merely report failure at the end of a walk, it also **RESETS THE
//! ZIPPER TO THE ROOT** (`pathmap/src/zipper.rs:546`). The position it leaves
//! behind is a perfectly valid root zipper. So a seam that mistranslates
//! exhaustion into anything the walk can keep consuming does not raise an error
//! anywhere — the counted walk SILENTLY RESTARTS and loops forever. That is a
//! defect that surfaces late and expensively, which is why both endpoints assert
//! it now, before the seam exists.
//!
//! The mettail twin of `to_next_leaf_returns_nil_when_exhausted` is
//! `languages/src/rhocalc/zipper.rs::exhausted_walk_is_stuck_here_and_nil_on_the_reducer`
//! (plus the surface-level
//! `languages/tests/rhocalc_tests.rs::zipper_leaf_walk_exhaustion_stays_stuck`).

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

async fn eval_ok(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime
        .evaluate_with_term(term)
        .await
        .expect("evaluation must not error");
    assert!(
        res.errors.is_empty(),
        "evaluation raised interpreter errors: {:?}",
        res.errors
    );
}

async fn read_single_expr(runtime: &RhoRuntimeImpl, channel_name: &str) -> ExprInstance {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(channel_name.to_string())),
    }]);
    let data = runtime.get_data(&channel).await;
    assert_eq!(
        data.len(),
        1,
        "expected exactly one datum at @\"{}\"",
        channel_name
    );
    let pars = &data[0].a.pars;
    assert_eq!(
        pars.len(),
        1,
        "expected a single Par at @\"{}\"",
        channel_name
    );
    pars[0]
        .exprs
        .first()
        .and_then(|e| e.expr_instance.clone())
        .unwrap_or_else(|| panic!("no expr at @\"{}\"", channel_name))
}

async fn assert_bool(runtime: &RhoRuntimeImpl, channel_name: &str) {
    match read_single_expr(runtime, channel_name).await {
        ExprInstance::GBool(true) => {}
        other => panic!("@\"{}\" expected GBool(true), got {:?}", channel_name, other),
    }
}

async fn assert_int(runtime: &RhoRuntimeImpl, channel_name: &str, expected: i64) {
    match read_single_expr(runtime, channel_name).await {
        ExprInstance::GInt(n) if n == expected => {}
        other => panic!(
            "@\"{}\" expected GInt({}), got {:?}",
            channel_name, expected, other
        ),
    }
}

/// Four entries; a PathMap element is both the key and the value it stores, so
/// `getLeaf()` at a leaf returns that same list. Byte-lex order over the first
/// segment is "a" < "b" < "c", so the depth-first LEAF order is
/// `["a","x"]`, `["a","y"]`, `["b"]`, `["c","z"]`.
const MAP: &str = r#"{| ["a", "x"], ["a", "y"], ["b"], ["c", "z"] |}"#;

/// `leafCount()` is the map's cardinality at the root and the branch's result
/// count at a prefix — the DECIDABLE BOUND that terminates a walk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn leaf_count_is_the_walk_bound() {
    with_runtime("zipper-enum-count-", |mut runtime| async move {
        let program = format!(
            r#"
            @"countRoot"!( {m}.readZipper().leafCount() ) |
            @"countA"!( {m}.readZipperAt(["a"]).leafCount() ) |
            @"countLeaf"!( {m}.readZipperAt(["b"]).leafCount() ) |
            @"countMissing"!( {m}.readZipperAt(["zz"]).leafCount() )
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        assert_int(&runtime, "countRoot", 4).await;
        assert_int(&runtime, "countA", 2).await;
        assert_int(&runtime, "countLeaf", 1).await;
        assert_int(&runtime, "countMissing", 0).await;
        runtime
    })
    .await;
}

/// A `leafCount()`-bounded walk visits every entry exactly once in depth-first
/// order, and BOTH `getPath()` and `getLeaf()` answer at every stop — the
/// guarantee that makes a separate "is there a value here?" predicate
/// unnecessary.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn leaf_walk_visits_every_entry_in_order() {
    with_runtime("zipper-enum-walk-", |mut runtime| async move {
        let program = format!(
            r#"
            @"p1"!( {m}.readZipper().toNextLeaf().getPath() == ["a", "x"] ) |
            @"v1"!( {m}.readZipper().toNextLeaf().getLeaf() == ["a", "x"] ) |
            @"p2"!( {m}.readZipper().toNextLeaf().toNextLeaf().getPath() == ["a", "y"] ) |
            @"v2"!( {m}.readZipper().toNextLeaf().toNextLeaf().getLeaf() == ["a", "y"] ) |
            @"p3"!( {m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().getPath() == ["b"] ) |
            @"p4"!( {m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf().getPath() == ["c", "z"] )
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        for channel in ["p1", "v1", "p2", "v2", "p3", "p4"] {
            assert_bool(&runtime, channel).await;
        }
        runtime
    })
    .await;
}

/// ★ **THE CROSS-ENDPOINT PIN.** The step past the last leaf yields **`Nil`**
/// here, where mettail's rhocalc leaves the term **stuck**.
///
/// C1 must translate this `Nil` into rhocalc's stuck form. If it instead lets a
/// walk continue on it, the counted-walk idiom silently RESTARTS — `to_next_val()`
/// resets the zipper to the root on exhaustion — and loops forever with no error
/// raised anywhere.
///
/// Twin: `languages/src/rhocalc/zipper.rs::exhausted_walk_is_stuck_here_and_nil_on_the_reducer`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn to_next_leaf_returns_nil_when_exhausted() {
    with_runtime("zipper-enum-exhaust-", |mut runtime| async move {
        let program = format!(
            r#"
            @"exhausted"!(
              {m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf() == Nil
            ) |
            @"notExhaustedYet"!(
              ({m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf() == Nil) == false
            ) |
            @"emptyMapIsImmediatelyExhausted"!( {{| |}}.readZipper().toNextLeaf() == Nil )
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        // The 5th step on a 4-entry map is exhaustion: Nil, NOT a zipper that
        // has silently wrapped back to the first leaf.
        assert_bool(&runtime, "exhausted").await;
        // The 4th step is still a live zipper — this is what makes the
        // assertion above about exhaustion rather than about `toNextLeaf`
        // returning Nil unconditionally.
        assert_bool(&runtime, "notExhaustedYet").await;
        assert_bool(&runtime, "emptyMapIsImmediatelyExhausted").await;
        runtime
    })
    .await;
}

/// The cursor key round-trips: the reported path re-addresses the very entry the
/// cursor is focused on. `getPath()` decodes `EZipper.current_path` — a field
/// that is ALREADY wire data (`RhoTypes.proto:352`), which is why this surface
/// needs no proto change and no new state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_path_round_trips_through_the_map() {
    with_runtime("zipper-enum-roundtrip-", |mut runtime| async move {
        let program = format!(
            r#"
            @"roundTrip"!(
              {m}.readZipperAt( {m}.readZipper().toNextLeaf().getPath() ).getLeaf() == ["a", "x"]
            ) |
            @"pathAtRootIsEmpty"!( {m}.readZipper().getPath() == [] ) |
            @"pathIsIndexable"!( {m}.readZipper().toNextLeaf().getPath().nth(0) == "a" ) |
            @"pathHasLength"!( {m}.readZipper().toNextLeaf().getPath().length() == 2 )
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        assert_bool(&runtime, "roundTrip").await;
        assert_bool(&runtime, "pathAtRootIsEmpty").await;
        // `getPath()` yields a LIST, so a trace is indexable — which is what
        // `trace.nth(…)` / `trace.length()` in the FIPS lookahead examples need.
        assert_bool(&runtime, "pathIsIndexable").await;
        assert_bool(&runtime, "pathHasLength").await;
        runtime
    })
    .await;
}

/// ★ THE FIPS LOOKAHEAD IDIOM, executed. The three methods are only useful if
/// the COUNTED WALK they are designed for actually runs in Rholang, so this
/// drives the real loop — a persistent receive carrying `(zipper, remaining,
/// accumulator)`, terminating on the `leafCount()` bound rather than on a
/// failed step — and checks it collects every trace exactly once.
///
/// This is the shape that replaces `for (@{| trace, ..._ |}, _ <- x)` in the
/// Lookahead FIPS: that pattern peels ONE arbitrary entry (and does not parse),
/// whereas the confinement use case needs ALL of them ("Bob can see all the
/// outgoing messages") and beam search needs to rank them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn counted_walk_collects_every_trace() {
    with_runtime("zipper-enum-fips-", |mut runtime| async move {
        let program = format!(
            r#"
            new walk in {{
              walk!({m}.readZipper(), {m}.readZipper().leafCount(), []) |
              for (@z, @remaining, @acc <= walk) {{
                match remaining {{
                  0 => @"traces"!(acc)
                  _ => match z.toNextLeaf() {{
                         next => walk!(next, remaining - 1, acc ++ [next.getPath()])
                       }}
                }}
              }}
            }}
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        let channel = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("traces".to_string())),
        }]);
        let data = runtime.get_data(&channel).await;
        assert_eq!(data.len(), 1, "the walk must terminate and report once");
        let collected = &data[0].a.pars[0];
        match collected
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::EListBody(list)) => assert_eq!(
                list.ps.len(),
                4,
                "every entry visited exactly once, terminating on the leafCount \
                 bound (not on a failed step): {:?}",
                list.ps
            ),
            other => panic!("@\"traces\" expected a list of traces, got {:?}", other),
        }
        runtime
    })
    .await;
}

/// Scoping an enumeration is ALGEBRAIC, not a walk parameter: `getSubtrie()`
/// yields a PathMap of just that branch, whose `readZipper()` walks exactly it.
/// Walking from a zipper parked at a strict prefix also stays inside the branch,
/// because prefix-sharing keys are contiguous in depth-first order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scoped_enumeration_is_algebraic() {
    with_runtime("zipper-enum-scope-", |mut runtime| async move {
        let program = format!(
            r#"
            @"subtrieCount"!( {m}.readZipperAt(["a"]).getSubtrie().readZipper().leafCount() ) |
            @"prefixWalkFirst"!(
              {m}.readZipperAt(["a"]).toNextLeaf().getPath() == ["a", "x"]
            ) |
            @"prefixWalkSecond"!(
              {m}.readZipperAt(["a"]).toNextLeaf().toNextLeaf().getPath() == ["a", "y"]
            )
            "#,
            m = MAP
        );
        eval_ok(&mut runtime, &program).await;
        assert_int(&runtime, "subtrieCount", 2).await;
        // `["a"]` is a strict prefix and NOT itself an entry, so the walk's
        // first stop is the branch's FIRST entry — `to_next_val` advances to
        // the next value, and the prefix position carries none. (An earlier
        // draft of this test expected `["a","y"]`, reasoning as if the prefix
        // itself had been consumed as a leaf; it had not.)
        assert_bool(&runtime, "prefixWalkFirst").await;
        // `leafCount()` at `["a"]` is 2, and those two steps stay in the
        // branch — prefix-sharing keys are contiguous in depth-first order.
        assert_bool(&runtime, "prefixWalkSecond").await;
        runtime
    })
    .await;
}

// ═══════════════════════════════════════════════════════════════════════════
// ★ THE BARE-ELEMENT ARM — the coverage gap that let the defect survive
// ═══════════════════════════════════════════════════════════════════════════
//
// Every element of `MAP` above is a ground LIST. `encode_trie_path` is a
// TWO-ARM codec (`canonical_path.rs`, §1.3) and the fixture only ever drove
// one of them:
//
//   split arm (ground list `[e₁..e_k]`)  path = enc(e₁) ‖ … ‖ enc(e_k) ‖ 0x00
//   bare arm  (anything else)            path = enc(par)          — NO 0x00
//
// so no test in this file, or anywhere else, had ever asked what any zipper
// method does at a BARE entry. `{| 1, 2, 3 |}` is that question, and the
// answers below are wrong in three independent ways.
//
// # The defect, stated once
//
// Every reader rebuilds its key with `segments_to_key(current_path, true)`
// (`pathmap_integration.rs:66`), which appends `tag::TERM` UNCONDITIONALLY.
// For a bare entry that key is not a miss — it is the valid canonical key of a
// DIFFERENT element:
//
//   bare `1`        key  03 02          (what the map stores)
//   read key        key  03 02 00   ==  encode_trie_path([1])   — the SINGLETON
//
// The insert side is correct and injective and is NOT the bug: appending a
// terminator at insert would make `encode_trie_path(1) == encode_trie_path([1])`
// and merge `{| 5, [5] |}` into one entry. The lossy component is
// `EZipper.current_path` (`RhoTypes.proto:352`, `repeated bytes`), which stores
// the per-element segments but not the split/bare discriminator, so every
// reconstruction must guess — and always guesses "split".
//
// ⚠ The `witness_` tests below assert the DEFECTIVE answers on purpose, and
// each names the positive twin that replaces it. Every walk here is BOUNDED by
// an explicit chain length, never by a termination condition, so a regression
// FAILS rather than hangs — which matters, because `toNextLeaf` on this
// fixture is a fixed point and a `leafCount()`-bounded loop over it collects
// the same entry three times rather than looping forever.

/// Three entries, none of them a list, so all three take the codec's BARE arm.
/// Trie keys `03 02`, `03 04`, `03 06` — tag `0x03` = `GInt`, payload the
/// zigzag varint — and NONE of them carries the `0x00` terminator.
const BARE_MAP: &str = r#"{| 1, 2, 3 |}"#;

/// A map whose elements take BOTH arms, including the pair `5` / `[5]` that
/// makes the insert side's injectivity load-bearing: they are DIFFERENT
/// entries with different keys (`03 0A` and `03 0A 00`) and must stay so.
const MIXED_MAP: &str = r#"{| 5, [5], "a", ["a", "x"] |}"#;

/// `leafCount()` is a PREFIX query — `segments_to_key(current_path, false)` —
/// so it never appends the terminator and is CORRECT on bare entries today.
/// That is what makes the rest of this section a contradiction rather than an
/// absence: the map demonstrably holds three entries that nothing else can read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bare_leaf_count_is_correct() {
    with_runtime("zipper-enum-bare-count-", |mut runtime| async move {
        let program = format!(
            r#"
            @"countRoot"!( {m}.readZipper().leafCount() ) |
            @"mixedCount"!( {x}.readZipper().leafCount() )
            "#,
            m = BARE_MAP,
            x = MIXED_MAP
        );
        eval_ok(&mut runtime, &program).await;
        assert_int(&runtime, "countRoot", 3).await;
        // ★ 4, not 3: `5` and `[5]` are DISTINCT entries. The insert side is
        // injective and must remain so — this is the assertion that would go
        // red if the terminator were ever appended at insert time.
        assert_int(&runtime, "mixedCount", 4).await;
        runtime
    })
    .await;
}

/// ⚠ WITNESS OF A DEFECT — every value read of a bare entry answers `Nil`.
///
/// Positive twin: `bare_leaf_reads_back_the_element`, which asserts that each
/// of the three reads below returns the element itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn witness_bare_leaf_reads_back_as_nil() {
    with_runtime("zipper-enum-bare-read-", |mut runtime| async move {
        let program = format!(
            r#"
            @"getLeafIsNil"!( {m}.readZipperAt(1).getLeaf() == Nil ) |
            @"atPathBareIsNil"!( {m}.atPath(1) == Nil ) |
            @"atPathListIsNil"!( {m}.atPath([1]) == Nil )
            "#,
            m = BARE_MAP
        );
        eval_ok(&mut runtime, &program).await;
        // The cursor is at the bare entry `1`, whose key is `03 02`; the read
        // rebuilds `03 02 00`, the key of the singleton list `[1]`.
        assert_bool(&runtime, "getLeafIsNil").await;
        // `atPath(1)` asks for the bare entry by name and still misses…
        assert_bool(&runtime, "atPathBareIsNil").await;
        // …and `atPath([1])` misses too, because `[1]` really is absent: the
        // map holds `1`. There is NO spelling that reads a bare entry today.
        assert_bool(&runtime, "atPathListIsNil").await;
        runtime
    })
    .await;
}

/// ⚠ WITNESS OF A DEFECT — `toNextLeaf` is a FIXED POINT on bare entries.
///
/// `getPath()` reports `[1]` at every step: the wrong entry AND the wrong
/// shape (a singleton list where the map holds a bare integer), which is why
/// the `readZipperAt(z.getPath())` round-trip cannot close either.
///
/// BOUNDED BY CONSTRUCTION: four explicit steps, not a termination condition.
///
/// Positive twin: `bare_walk_visits_every_entry_in_order`, which asserts the
/// steps report `1`, `2`, `3` and that the fourth is `Nil`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn witness_bare_walk_never_advances() {
    with_runtime("zipper-enum-bare-walk-", |mut runtime| async move {
        let program = format!(
            r#"
            @"p1"!( {m}.readZipper().toNextLeaf().getPath() == [1] ) |
            @"p2"!( {m}.readZipper().toNextLeaf().toNextLeaf().getPath() == [1] ) |
            @"p3"!( {m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().getPath() == [1] ) |
            @"p4"!( {m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf().getPath() == [1] ) |
            @"v1"!( {m}.readZipper().toNextLeaf().getLeaf() == Nil )
            "#,
            m = BARE_MAP
        );
        eval_ok(&mut runtime, &program).await;
        for channel in ["p1", "p2", "p3", "p4"] {
            assert_bool(&runtime, channel).await;
        }
        // …and the leaf the walk is parked on cannot be read either.
        assert_bool(&runtime, "v1").await;
        runtime
    })
    .await;
}

/// ⚠ WITNESS OF A DEFECT — the LIVENESS half. The step past the last entry
/// must be `Nil`; on a bare map no step ever is, so the `leafCount()`-bounded
/// idiom that `counted_walk_collects_every_trace` proves for lists cannot
/// terminate here on its own.
///
/// Positive twin: `bare_walk_returns_nil_when_exhausted`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn witness_bare_walk_never_exhausts() {
    with_runtime("zipper-enum-bare-exhaust-", |mut runtime| async move {
        let program = format!(
            r#"
            @"stillNotNil"!(
              ({m}.readZipper().toNextLeaf().toNextLeaf().toNextLeaf().toNextLeaf() == Nil) == false
            )
            "#,
            m = BARE_MAP
        );
        eval_ok(&mut runtime, &program).await;
        // The FOURTH step on a THREE-entry map is still a live zipper.
        assert_bool(&runtime, "stillNotNil").await;
        runtime
    })
    .await;
}

/// ⚠ WITNESS OF A DEFECT — the `leafCount()`-bounded walk, run for real on
/// bare entries. It TERMINATES (the bound is the loop's own counter) but
/// collects the SAME trace three times, so the idiom the FIPS lookahead
/// depends on silently returns duplicates instead of the map's contents.
///
/// Positive twin: `bare_counted_walk_collects_every_entry`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn witness_bare_counted_walk_collects_one_entry_three_times() {
    with_runtime("zipper-enum-bare-fips-", |mut runtime| async move {
        let program = format!(
            r#"
            new walk in {{
              walk!({m}.readZipper(), {m}.readZipper().leafCount(), []) |
              for (@z, @remaining, @acc <= walk) {{
                match remaining {{
                  0 => @"bareTraces"!(acc)
                  _ => match z.toNextLeaf() {{
                         next => walk!(next, remaining - 1, acc ++ [next.getPath()])
                       }}
                }}
              }}
            }}
            "#,
            m = BARE_MAP
        );
        eval_ok(&mut runtime, &program).await;
        let channel = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("bareTraces".to_string())),
        }]);
        let data = runtime.get_data(&channel).await;
        assert_eq!(data.len(), 1, "the counted walk must still report once");
        let collected = &data[0].a.pars[0];
        match collected
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::EListBody(list)) => {
                assert_eq!(list.ps.len(), 3, "three steps, as leafCount() bounds");
                assert_eq!(
                    list.ps[0], list.ps[1],
                    "★ steps 1 and 2 report the SAME trace"
                );
                assert_eq!(
                    list.ps[1], list.ps[2],
                    "★ steps 2 and 3 report the SAME trace"
                );
            }
            other => panic!("@\"bareTraces\" expected a list of traces, got {:?}", other),
        }
        runtime
    })
    .await;
}
