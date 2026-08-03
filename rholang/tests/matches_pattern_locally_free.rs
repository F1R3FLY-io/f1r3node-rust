//! `EMatches::pattern` contributes to the node's `locally_free`.
//!
//! `HasLocallyFree::locally_free`'s contract is *"a bitset representing which
//! variables are locally free if the term is located at depth `depth`"* — a
//! question about **scope**. `=x` inside a `matches` pattern resolves through
//! `BoundMapChain::find`, which walks the whole chain, and is emitted as a
//! `VarRef` carrying the chain distance: it names an outer binder from inside
//! the pattern. Reading the node's bitset from the `target` alone dropped that
//! bit, and it never reappeared.
//!
//! Rholang has exactly three pattern positions. `MatchCase::pattern`
//! (`p_match_normalizer`) and `ReceiveBind::patterns` (`p_input_normalizer`)
//! both union their pattern's bitset into the parent. `EMatches::pattern`
//! (`p_matches_normalizer`, whose `combine_p_matches` pushes
//! `bound_map_chain` exactly as those two do) did not. These tests assert the
//! three agree, on the **bitset value** — never on the absence of an error.
//!
//! ⚠ `connective_used` is the OTHER half of the same trait and is deliberately
//! NOT symmetric: its contract is about *concreteness*, a `matches` pattern is
//! allowed to be non-concrete, and `1 matches ~1` is a concrete term. The last
//! test here pins that asymmetry so a future "let us make these consistent"
//! sweep fails instead of changing what programs are accepted.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMatches, Par};
use rholang::rust::interpreter::matcher::has_locally_free::ematches_locally_free;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

fn compile(src: &str) -> Par {
    ParBuilderUtil::mk_term(src).unwrap_or_else(|e| panic!("{src} must compile, got {e:?}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Navigation. Each step `expect`s, so a program that stops having the shape the
// test is about fails by NAME instead of by a silently-vacuous `[]` comparison.
// ─────────────────────────────────────────────────────────────────────────────

fn only_receive_body(par: &Par) -> &Par {
    let r = par
        .receives
        .first()
        .expect("the program's top level is a single `for`");
    r.body.as_ref().expect("Receive.body")
}

fn only_receive(par: &Par) -> &models::rhoapi::Receive {
    par.receives.first().expect("a single `for` at this level")
}

/// The single datum of the single send at this level.
fn only_sent_datum(par: &Par) -> &Par {
    let s = par.sends.first().expect("a single send at this level");
    assert_eq!(s.data.len(), 1, "the send carries exactly one datum");
    &s.data[0]
}

/// `(target, pattern)` of the single `EMatches` carried by `par`.
fn ematches_slots(par: &Par) -> (&Par, &Par) {
    let expr = par
        .exprs
        .first()
        .expect("this Par must carry exactly one Expr");
    match expr
        .expr_instance
        .as_ref()
        .expect("Expr.expr_instance must be populated")
    {
        ExprInstance::EMatchesBody(EMatches { target, pattern }) => (
            target.as_ref().expect("EMatches.target"),
            pattern.as_ref().expect("EMatches.pattern"),
        ),
        other => panic!("expected an EMatchesBody, got {other:?}"),
    }
}

/// The single case pattern of the single `match` carried by `par`.
fn match_case_pattern(par: &Par) -> &Par {
    let m = par.matches.first().expect("this Par must carry one Match");
    let case = m.cases.first().expect("Match must have one case");
    case.pattern.as_ref().expect("MatchCase.pattern")
}

fn only_match(par: &Par) -> &models::rhoapi::Match {
    par.matches.first().expect("this Par must carry one Match")
}

// `create_bit_vector` is one BYTE per index, not packed bits: index `i` set is
// `[0; i]` followed by `1`. Spelling the literals out keeps the tests readable
// as "which index", and keeps them failing if that representation ever changes.
/// Index 0 free, nothing else.
const IDX_0: &[u8] = &[1];
/// Index 1 free, index 0 not.
const IDX_1: &[u8] = &[0, 1];
/// Closed.
const CLOSED: &[u8] = &[];
/// What survives a one-name binder: index 1 escapes as index 0, so the answer is
/// [`IDX_0`].
///
/// ⚠ This constant used to be `&[0]` — "index 0 is NOT free", i.e. `∅` with a
/// trailing zero — because `filter_and_adjust_bitset` discarded the bit and
/// emitted the shifted *position* as the *value*. The rows below deliberately
/// pinned that value while its repair was carried as a separate,
/// consensus-visible change; the repair has landed (see
/// `rholang/tests/locally_free_binder_shift.rs` and
/// `interpreter::util::binder_shift_law`), so the alias now names the
/// well-formed answer. It is kept as an alias rather than inlined because the
/// rows read as "the value that survives ONE binder", which is the property
/// under test, and because the identity `ADJUSTED_PAST_ONE_BINDER == IDX_0` is
/// itself the fix's statement.
const ADJUSTED_PAST_ONE_BINDER: &[u8] = IDX_0;

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE DIFFERENTIAL — `matches` versus its two siblings, same content
// ─────────────────────────────────────────────────────────────────────────────

/// RED before the fix: `pattern.locally_free` was `[1]` and the enclosing
/// `Par`/`Send`/body all read `[]`.
#[test]
fn a_matches_pattern_var_ref_reaches_the_enclosing_par() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(10 matches =x) }"#);
    let body = only_receive_body(&par);
    let datum = only_sent_datum(body);
    let (target, pattern) = ematches_slots(datum);

    // The two slots, so the test says WHERE the bit comes from.
    assert_eq!(target.locally_free, CLOSED, "`10` is closed");
    assert_eq!(
        pattern.locally_free, IDX_0,
        "`=x` names `x`, bound at index 0 by the enclosing `for`"
    );

    // ★ The value under test: the node, and every ancestor inside the binder.
    assert_eq!(
        datum.locally_free, IDX_0,
        "the Par carrying `10 matches =x` must report index 0 free"
    );
    assert_eq!(
        body.sends[0].locally_free, IDX_0,
        "the Send carrying it must report index 0 free"
    );
    assert_eq!(
        body.locally_free, IDX_0,
        "the `for` body must report index 0 free"
    );

    // The binder that owns index 0 is where it is discharged, not before.
    assert_eq!(
        only_receive(&par).locally_free,
        CLOSED,
        "the `for` binds `x`, so the `for` itself is closed"
    );
    assert_eq!(par.locally_free, CLOSED, "the whole program is closed");
}

/// The `MatchCase::pattern` sibling, on identical content. This one was already
/// correct; it is the oracle the `matches` row is compared against.
#[test]
fn b_match_case_pattern_var_ref_reaches_the_enclosing_par() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(match 10 { =x => Nil }) }"#);
    let body = only_receive_body(&par);
    let datum = only_sent_datum(body);

    assert_eq!(match_case_pattern(datum).locally_free, IDX_0);
    assert_eq!(only_match(datum).locally_free, IDX_0);
    assert_eq!(datum.locally_free, IDX_0);
    assert_eq!(body.locally_free, IDX_0);
    assert_eq!(only_receive(&par).locally_free, CLOSED);
}

/// The `ReceiveBind::patterns` sibling — the third and last pattern position.
#[test]
fn c_receive_bind_pattern_var_ref_reaches_the_enclosing_par() {
    let par = compile(r#"for (@x <- @"c") { for (@{=x} <- @"d") { Nil } }"#);
    let body = only_receive_body(&par);
    let inner = only_receive(body);

    assert_eq!(
        inner.binds[0].patterns[0].locally_free, IDX_0,
        "`=x` in a `for` pattern names index 0"
    );
    assert_eq!(inner.locally_free, IDX_0);
    assert_eq!(body.locally_free, IDX_0);
    assert_eq!(only_receive(&par).locally_free, CLOSED);
}

/// ★ The three pattern positions must agree. This is the assertion the fix is
/// FOR: it is the one that fails no matter which of the three drifts.
#[test]
fn d_all_three_pattern_positions_agree() {
    let rows: [(&str, &str); 3] = [
        (
            "EMatches::pattern",
            r#"for (@x <- @"c") { @"o"!(10 matches =x) }"#,
        ),
        (
            "MatchCase::pattern",
            r#"for (@x <- @"c") { @"o"!(match 10 { =x => Nil }) }"#,
        ),
        (
            "ReceiveBind::patterns",
            r#"for (@x <- @"c") { for (@{=x} <- @"d") { Nil } }"#,
        ),
    ];
    for (position, src) in rows {
        let par = compile(src);
        assert_eq!(
            only_receive_body(&par).locally_free,
            IDX_0,
            "{position}: a `VarRef` naming index 0 from inside a pattern must be \
             reported by the enclosing scope"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE BIT MUST ESCAPE AN INTERVENING BINDER
//
// One `for` cannot discriminate a reader that drops the bit from a reader that
// keeps it *and then discharges it too early*: both end at `[]`. Nesting a
// second `for` separates them — `filter_and_adjust_bitset` must carry index 1
// out past the inner binder as index 0, and only the OUTER `for` may clear it.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn e_matches_pattern_var_ref_escapes_an_intervening_binder() {
    let par = compile(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(10 matches =x) } }"#);
    let outer_body = only_receive_body(&par);
    let inner = only_receive(outer_body);
    let inner_body = inner.body.as_ref().expect("inner Receive.body");
    let (_, pattern) = ematches_slots(only_sent_datum(inner_body));

    assert_eq!(
        pattern.locally_free, IDX_1,
        "inside two binders, `x` is index 1 and `y` is index 0"
    );
    assert_eq!(inner_body.locally_free, IDX_1, "the inner `for` body");
    assert_eq!(
        inner.locally_free, ADJUSTED_PAST_ONE_BINDER,
        "★ the inner `for` binds `y` only, so `x` SURVIVES it — pre-fix this was \
         `[]`, i.e. the inner `for` claimed to be closed"
    );
    assert_eq!(
        outer_body.locally_free, ADJUSTED_PAST_ONE_BINDER,
        "and reaches the outer `for` body"
    );
    assert_eq!(
        only_receive(&par).locally_free,
        CLOSED,
        "the outer `for` binds `x`, and THAT is where it is discharged"
    );
}

/// ★ The whole ancestor chain of the two escaping programs must be BYTE-EQUAL.
///
/// This is the strongest form of the differential and the one that does not
/// depend on `filter_and_adjust_bitset` being right: whatever it computes, the
/// `matches` spelling and the `match` spelling of the same content must compute
/// it identically. Pre-fix the `matches` chain was `[]` at every level from the
/// `EMatches` node up.
#[test]
fn e_f_the_matches_and_match_chains_are_byte_equal() {
    /// `(EMatches-or-Match carrier, inner body, inner Receive, outer body,
    /// outer Receive)`, as bitsets.
    fn chain(src: &str) -> Vec<Vec<u8>> {
        let par = compile(src);
        let outer_body = only_receive_body(&par);
        let inner = only_receive(outer_body);
        let inner_body = inner.body.as_ref().expect("inner Receive.body");
        vec![
            only_sent_datum(inner_body).locally_free.clone(),
            inner_body.locally_free.clone(),
            inner.locally_free.clone(),
            outer_body.locally_free.clone(),
            only_receive(&par).locally_free.clone(),
        ]
    }

    let via_matches = chain(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(10 matches =x) } }"#);
    let via_match =
        chain(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(match 10 { =x => Nil }) } }"#);

    assert_eq!(
        via_matches, via_match,
        "`matches` and `match` must report the same locally_free at every level"
    );
    // ⚠ And pinned to the VALUE, so "both empty" cannot pass this row.
    assert_eq!(
        via_matches,
        vec![
            IDX_1.to_vec(),
            IDX_1.to_vec(),
            ADJUSTED_PAST_ONE_BINDER.to_vec(),
            ADJUSTED_PAST_ONE_BINDER.to_vec(),
            CLOSED.to_vec(),
        ],
        "the shared chain, spelled out"
    );
}

/// The same escape through the `MatchCase::pattern` sibling, so the row above
/// is pinned to a value some other construct in the tree already produces.
#[test]
fn f_match_case_pattern_var_ref_escapes_an_intervening_binder() {
    let par = compile(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(match 10 { =x => Nil }) } }"#);
    let outer_body = only_receive_body(&par);
    let inner = only_receive(outer_body);
    let inner_body = inner.body.as_ref().expect("inner Receive.body");

    assert_eq!(
        match_case_pattern(only_sent_datum(inner_body)).locally_free,
        IDX_1
    );
    assert_eq!(inner_body.locally_free, IDX_1);
    assert_eq!(inner.locally_free, ADJUSTED_PAST_ONE_BINDER);
    assert_eq!(outer_body.locally_free, ADJUSTED_PAST_ONE_BINDER);
    assert_eq!(only_receive(&par).locally_free, CLOSED);
}

// ─────────────────────────────────────────────────────────────────────────────
// ⚠ THE FLOOR — ordinary closed and ordinary open terms
//
// A reader that returned "everything is free" would pass every test above. No
// row may rest on a broken classifier.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn g_a_ground_matches_pattern_stays_closed() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(10 matches 10) }"#);
    let body = only_receive_body(&par);
    let datum = only_sent_datum(body);
    let (target, pattern) = ematches_slots(datum);

    assert_eq!(target.locally_free, CLOSED);
    assert_eq!(pattern.locally_free, CLOSED);
    assert_eq!(
        datum.locally_free, CLOSED,
        "no variable is named, so nothing may be reported"
    );
    assert_eq!(body.locally_free, CLOSED);
}

#[test]
fn h_an_ordinary_bound_reference_is_still_reported() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(x) }"#);
    let body = only_receive_body(&par);
    assert_eq!(
        only_sent_datum(body).locally_free,
        IDX_0,
        "a plain `x` is a BoundVar at index 0 and must be reported"
    );
    assert_eq!(body.locally_free, IDX_0);
    assert_eq!(only_receive(&par).locally_free, CLOSED);
}

/// A plain `x` **inside** the `matches` pattern is a fresh binding occurrence
/// (`BoundMapChain::get`, current scope only), NOT a reference to the outer
/// `x`. So it must contribute nothing — the union must not over-report either.
#[test]
fn i_a_plain_name_inside_a_matches_pattern_contributes_nothing() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(10 matches x) }"#);
    let body = only_receive_body(&par);
    let datum = only_sent_datum(body);
    let (_, pattern) = ematches_slots(datum);

    assert_eq!(
        pattern.locally_free, CLOSED,
        "`x` in pattern position is a FRESH binder, not the outer `x`"
    );
    assert_eq!(
        datum.locally_free, CLOSED,
        "so the enclosing Par reports nothing — the union must not invent a bit"
    );
    assert_eq!(body.locally_free, CLOSED);
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE DEPTH IS PINNED FROM BOTH SIDES
//
// `locally_free` sets a `VarRef`'s bit iff the traversal depth equals the
// `VarRef`'s own `depth`. The pattern of a top-level `matches` is depth 1, so a
// `VarRef` at depth 1 contributes and one at any other depth does not. These
// rows call the arm directly, so an off-by-one in EITHER direction fails.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn j_the_arm_is_a_pure_union_of_the_two_cached_bitsets() {
    use models::create_bit_vector;

    let cases: [(&[usize], &[usize], &[u8]); 5] = [
        // target      pattern       expected union
        (&[], &[], CLOSED),
        (&[], &[0], IDX_0),
        (&[0], &[], IDX_0),
        (&[], &[1], IDX_1),
        (&[2], &[0], &[1, 0, 1]),
    ];
    for (t_bits, p_bits, expected) in cases {
        let target = models::par_from_default! {
            locally_free: if t_bits.is_empty() {
                Vec::new()
            } else {
                create_bit_vector(t_bits)
            },
            ..Default::default()
        };
        let pattern = models::par_from_default! {
            locally_free: if p_bits.is_empty() {
                Vec::new()
            } else {
                create_bit_vector(p_bits)
            },
            ..Default::default()
        };
        assert_eq!(
            ematches_locally_free(&target, &pattern),
            expected,
            "union of target {t_bits:?} and pattern {p_bits:?}"
        );
    }
}

/// The pattern of a top-level `matches` is depth 1, so `=x` there is emitted as
/// `VarRef { depth: 1 }`. A guard at depth and depth+1: the `VarRef` in the
/// pattern must carry depth 1 exactly — 0 would mean the reader never sets its
/// bit, 2 would mean it is set one level out and stranded.
#[test]
fn k_the_pattern_var_ref_depth_is_exactly_one_deeper() {
    use models::rhoapi::connective::ConnectiveInstance;

    let par = compile(r#"for (@x <- @"c") { @"o"!(10 matches =x) }"#);
    let (_, pattern) = ematches_slots(only_sent_datum(only_receive_body(&par)));
    let conn = pattern
        .connectives
        .first()
        .expect("the pattern is a single VarRef connective");
    match conn
        .connective_instance
        .as_ref()
        .expect("Connective.connective_instance")
    {
        ConnectiveInstance::VarRefBody(v) => {
            assert_eq!(v.index, 0, "`x` is index 0");
            assert_eq!(
                v.depth, 1,
                "★ the `matches` pattern is ONE level deeper than the enclosing \
                 term (depth 0), so its VarRef carries depth 1. Depth 0 would \
                 leave the bit unset; depth 2 would strand it."
            );
        }
        other => panic!("expected a VarRefBody, got {other:?}"),
    }

    // The same content in a `for` pattern — also exactly one level deeper.
    let sibling = compile(r#"for (@x <- @"c") { for (@{=x} <- @"d") { Nil } }"#);
    let sib_pattern = &only_receive(only_receive_body(&sibling)).binds[0].patterns[0];
    match sib_pattern.connectives[0]
        .connective_instance
        .as_ref()
        .expect("Connective.connective_instance")
    {
        ConnectiveInstance::VarRefBody(v) => assert_eq!(
            (v.index, v.depth),
            (0, 1),
            "`ReceiveBind::patterns` is also depth 1 — the two agree"
        ),
        other => panic!("expected a VarRefBody, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ⚠ THE ASYMMETRY THAT MUST SURVIVE
// ─────────────────────────────────────────────────────────────────────────────

/// `connective_used` is the other half of `HasLocallyFree` and must NOT be made
/// symmetric with `locally_free`. Its contract is about concreteness, and a
/// `matches` pattern is *allowed* to be non-concrete: that is the construct's
/// whole purpose. `10 matches ~10` is a concrete term that evaluates to a
/// `GBool`, so the enclosing `Par` must report `connective_used == false` even
/// though the pattern reports `true` — otherwise this term stops being sendable
/// and the change becomes one about which programs are ACCEPTED.
#[test]
fn l_connective_used_still_reads_the_target_alone() {
    let par = compile(r#"for (@x <- @"c") { @"o"!(10 matches ~10) }"#);
    let body = only_receive_body(&par);
    let datum = only_sent_datum(body);
    let (target, pattern) = ematches_slots(datum);

    assert!(!target.connective_used, "`10` is concrete");
    assert!(
        pattern.connective_used,
        "`~10` is a connective, so the PATTERN is non-concrete"
    );
    assert!(
        !datum.connective_used,
        "★ and yet the enclosing Par is concrete — `matches` legitimizes a \
         non-concrete pattern. Do not 'fix' this to match `locally_free`."
    );
    assert!(!body.connective_used, "so the `for` body is concrete too");
    assert!(!par.connective_used);
}
