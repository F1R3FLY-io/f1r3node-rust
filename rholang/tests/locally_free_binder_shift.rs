//! ★ A binder's body reports the indices that ESCAPE it, renumbered — and the
//! bitset that says so is on the protobuf byte path.
//!
//! # The law, and where it lives
//!
//! Every binding form in the language answers one question about its body: *of
//! the de Bruijn indices the body names, which ones does this binder not own?*
//! `New`, `Receive` (both the `for` and the `contract` spelling) and `MatchCase`
//! all answer it by calling **one** function,
//! `interpreter::util::filter_and_adjust_bitset`, which is this port's rendering
//! of Scala's
//!
//! ```scala
//! bodyResult.par.locallyFree.from(boundCount).map(x => x - boundCount)
//! ```
//!
//! `from(n)` keeps the MEMBERS `>= n` of a `BitSet`; `map(_ - n)` renumbers them
//! into the parent's index space. This port represents the bitset as **one byte
//! per index** (`models::create_bit_vector` is `vec![0; max + 1]` then
//! `bit_vector[index] = 1`), so a member's identity IS its position and the two
//! Scala steps collapse into "drop the first `n` bytes".
//!
//! # What these tests assert, and how they avoid being circular
//!
//! The unit rows in `interpreter::util::binder_shift_law` check the function
//! against an index-space oracle. These rows check the **compiler**: for a
//! binding node `N` with `n` binders and body `B`, they derive the body's
//! pre-binding member set from the source fixture, apply
//! `from(n).map(_ - n)` **in index space**, re-render, and require
//! `N.locally_free` to equal it byte for byte. This is source-derived because
//! `New` and `Receive` deliberately transfer the body cache into the binding
//! node and clear the nested duplicate; `MatchCase` retains its nested cache.
//!
//! The decode (`members_of`) and the shift (`escapes`) are the only two things
//! the test computes, and neither mentions `filter_and_adjust_bitset`. The
//! fixtures additionally pin whether the nested cache is transferred or
//! retained. Thus the expectation and the value under test cannot move with
//! either the shift implementation or the cache-ownership implementation.
//!
//! ⚠ Every row additionally asserts that the node's **other** contributions —
//! the channel/source it listens on, the patterns it binds, a `match`'s target —
//! are closed, so the union the normalizer computes has exactly one non-empty
//! term and the equality is exact rather than slack. A program that stops having
//! that shape fails by name.
//!
//! # The byte-visibility row is separate on purpose
//!
//! `locally_free` is blanked on the **bincode** path (`models/build.rs` injects
//! `serialize_with = serialize_as_empty_bytes`, and `bincode_schema.rs` gives it a
//! dedicated `FieldKind::EmptyBytes`), and RETAINED on the **protobuf** path
//! (`protobuf_schema.rs` §B.1). Those are two different consensus lanes with two
//! different answers, and the last two tests here measure each rather than
//! asserting either.

use std::collections::BTreeSet;

use models::rhoapi::{Match, New, Par, Receive};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

fn compile(src: &str) -> Par {
    ParBuilderUtil::mk_term(src).unwrap_or_else(|e| panic!("{src} must compile, got {e:?}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// The oracle — index space only. Neither function names a byte position.
// ─────────────────────────────────────────────────────────────────────────────

/// Decode the byte-per-index representation into the member set it denotes.
fn members_of(bitset: &[u8]) -> BTreeSet<usize> {
    bitset
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| match byte {
            0 => None,
            _ => Some(index),
        })
        .collect()
}

/// `BitSet.from(n).map(_ - n)` — the members `>= n`, renumbered.
fn escapes(members: &BTreeSet<usize>, bound_count: usize) -> BTreeSet<usize> {
    members
        .iter()
        .copied()
        .filter(|&index| index >= bound_count)
        .map(|index| index - bound_count)
        .collect()
}

/// Render a member set back into the byte-per-index representation. `∅` is `[]`
/// — the only spelling production uses (see `binder_shift_law::bits`).
fn render(members: &BTreeSet<usize>) -> Vec<u8> {
    match members.iter().next_back() {
        None => Vec::new(),
        Some(&highest) => {
            let mut bytes = vec![0u8; highest + 1];
            for &index in members {
                bytes[index] = 1;
            }
            bytes
        }
    }
}

/// The expectation for a binding node: what its body's bitset becomes in the
/// parent's index space.
fn shifted(body: &[u8], bound_count: usize) -> Vec<u8> {
    render(&escapes(&members_of(body), bound_count))
}

// ─────────────────────────────────────────────────────────────────────────────
// Navigation. Each step `expect`s by name.
// ─────────────────────────────────────────────────────────────────────────────

fn outer_receive_body(par: &Par) -> &Par {
    let receive = par
        .receives
        .first()
        .expect("the program's top level is a single `for`");
    receive
        .body
        .as_ref()
        .expect("the outer Receive must have a body")
}

fn only_new(par: &Par) -> &New {
    assert_eq!(par.news.len(), 1, "exactly one `new` at this level");
    &par.news[0]
}

fn only_receive(par: &Par) -> &Receive {
    assert_eq!(par.receives.len(), 1, "exactly one receive at this level");
    &par.receives[0]
}

fn only_match(par: &Par) -> &Match {
    assert_eq!(par.matches.len(), 1, "exactly one `match` at this level");
    &par.matches[0]
}

/// One checked row: the node's own bitset, its body's bitset, and its arity.
struct Row {
    /// What the program is, for the failure message.
    what: &'static str,
    node: Vec<u8>,
    /// Body cache before the enclosing binder shifts it, derived from the
    /// fixture's two de Bruijn names: the inner binder at 0 and outer `x` at 1.
    body_before_binding: Vec<u8>,
    /// Cache stored on the nested body after the binding node is assembled.
    stored_body: Vec<u8>,
    body_cache_policy: BodyCachePolicy,
    bound_count: usize,
}

#[derive(Clone, Copy, Debug)]
enum BodyCachePolicy {
    /// `New` and `Receive` move the cache to the enclosing node, avoiding a
    /// duplicate allocation on the nested body.
    Transferred,
    /// `MatchCase` keeps the cache on its source as well as using it to derive
    /// the enclosing `Match` cache.
    Retained,
}

impl Row {
    /// ★ The law. `assert` order puts the oracle's answer second so the message
    /// reads "got, expected".
    fn check(&self) {
        let Row {
            what,
            node,
            body_before_binding,
            stored_body,
            body_cache_policy,
            bound_count,
        } = self;
        assert_eq!(
            node,
            &shifted(body_before_binding, *bound_count),
            "{what}: the body reports {:?} (members {:?}); {bound_count} binder(s) are \
             discharged here, so the node must report members {:?} — got members {:?}",
            body_before_binding,
            members_of(body_before_binding),
            escapes(&members_of(body_before_binding), *bound_count),
            members_of(node),
        );
        let expected_stored_body = match body_cache_policy {
            BodyCachePolicy::Transferred => Vec::new(),
            BodyCachePolicy::Retained => body_before_binding.clone(),
        };
        assert_eq!(
            stored_body, &expected_stored_body,
            "{what}: nested body cache ownership must be {body_cache_policy:?}"
        );
    }

    /// Whether this row exercises the SURVIVING case, i.e. whether the body
    /// names an index the node does not own. A suite of rows that all discharge
    /// everything would satisfy the law with `[]` on both sides.
    fn is_non_trivial(&self) -> bool {
        !escapes(&members_of(&self.body_before_binding), self.bound_count).is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE FOUR CALL SITES, each wrapped in an outer binder so its body names an
//   index that must SURVIVE it. One `for` cannot separate "keeps the bit" from
//   "keeps it and discharges it too early": both end `[]`.
// ─────────────────────────────────────────────────────────────────────────────

/// `New` — `p_new_normalizer`. Its `locally_free` is the shift and NOTHING
/// else, so this row needs no closedness precondition.
fn new_row() -> Row {
    let par = compile(r#"for (@x <- @"c") { new y in { @"o"!(x) } } "#);
    let body = outer_receive_body(&par);
    let node = only_new(body);
    let inner = node.p.as_ref().expect("New.p");
    assert_eq!(node.bind_count, 1, "`new y in` declares one name");
    Row {
        what: "`new y in { @\"o\"!(x) }` under `for (@x <- @\"c\")`",
        node: node.locally_free.clone(),
        body_before_binding: vec![0, 1],
        stored_body: inner.locally_free.clone(),
        body_cache_policy: BodyCachePolicy::Transferred,
        bound_count: node.bind_count as usize,
    }
}

/// `Receive` via `for` — `p_input_normalizer`.
fn for_row() -> Row {
    let par = compile(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(x) } }"#);
    let body = outer_receive_body(&par);
    let node = only_receive(body);
    let inner = node.body.as_ref().expect("Receive.body");
    assert_eq!(node.bind_count, 1, "`for (@y <- @\"d\")` binds one name");
    for bind in &node.binds {
        assert!(
            bind.source
                .as_ref()
                .expect("ReceiveBind.source")
                .locally_free
                .is_empty(),
            "the source `@\"d\"` must be closed, or the union has a second term"
        );
        for pattern in &bind.patterns {
            assert!(
                pattern.locally_free.is_empty(),
                "the pattern `@y` is a fresh binder and must contribute nothing"
            );
        }
    }
    Row {
        what: "`for (@y <- @\"d\") { @\"o\"!(x) }` under `for (@x <- @\"c\")`",
        node: node.locally_free.clone(),
        body_before_binding: vec![0, 1],
        stored_body: inner.locally_free.clone(),
        body_cache_policy: BodyCachePolicy::Transferred,
        bound_count: node.bind_count as usize,
    }
}

/// `Receive` via `contract` — `p_contr_normalizer`. A DIFFERENT normalizer
/// reaching the same function, which is why it is a separate row.
fn contract_row() -> Row {
    let par = compile(r#"for (@x <- @"c") { contract @"k"(@y) = { @"o"!(x) } }"#);
    let body = outer_receive_body(&par);
    let node = only_receive(body);
    let inner = node.body.as_ref().expect("Receive.body");
    assert_eq!(node.bind_count, 1, "`contract @\"k\"(@y)` binds one name");
    for bind in &node.binds {
        assert!(
            bind.source
                .as_ref()
                .expect("ReceiveBind.source")
                .locally_free
                .is_empty(),
            "the contract's own channel `@\"k\"` must be closed"
        );
        for pattern in &bind.patterns {
            assert!(
                pattern.locally_free.is_empty(),
                "the formal `@y` is a fresh binder and must contribute nothing"
            );
        }
    }
    Row {
        what: "`contract @\"k\"(@y) = { @\"o\"!(x) }` under `for (@x <- @\"c\")`",
        node: node.locally_free.clone(),
        body_before_binding: vec![0, 1],
        stored_body: inner.locally_free.clone(),
        body_cache_policy: BodyCachePolicy::Transferred,
        bound_count: node.bind_count as usize,
    }
}

/// `MatchCase` — `p_match_normalizer`. The shift is per CASE and unioned with
/// the target's bitset, so the row asserts the target is closed.
fn match_row() -> Row {
    let par = compile(r#"for (@x <- @"c") { match 10 { y => @"o"!(x) } }"#);
    let body = outer_receive_body(&par);
    let node = only_match(body);
    assert!(
        node.target
            .as_ref()
            .expect("Match.target")
            .locally_free
            .is_empty(),
        "the target `10` must be closed, or the union has a second term"
    );
    assert_eq!(node.cases.len(), 1, "exactly one case");
    let case = &node.cases[0];
    assert!(
        case.pattern
            .as_ref()
            .expect("MatchCase.pattern")
            .locally_free
            .is_empty(),
        "the pattern `y` is a fresh binder and must contribute nothing"
    );
    assert_eq!(case.free_count, 1, "the pattern `y` binds one process");
    Row {
        what: "`match 10 { y => @\"o\"!(x) }` under `for (@x <- @\"c\")`",
        node: node.locally_free.clone(),
        body_before_binding: vec![0, 1],
        stored_body: case
            .source
            .as_ref()
            .expect("MatchCase.source")
            .locally_free
            .clone(),
        body_cache_policy: BodyCachePolicy::Retained,
        bound_count: case.free_count as usize,
    }
}

fn every_row() -> Vec<Row> { vec![new_row(), for_row(), contract_row(), match_row()] }

/// ★★ THE GATE. Every binding form, one law.
#[test]
fn every_binding_form_shifts_its_body_bitset_into_its_own_index_space() {
    let rows = every_row();
    for row in &rows {
        row.check();
    }

    // ⚠ Non-vacuity floor. Each of the four rows is constructed so its body
    // names an index the node does NOT own; if any stops doing so — because the
    // program was edited, or because the compiler stopped reporting the bit at
    // all — the law above becomes `[] == []` for that row and proves nothing.
    let non_trivial = rows.iter().filter(|row| row.is_non_trivial()).count();
    assert_eq!(
        non_trivial,
        rows.len(),
        "all {} rows must exercise the SURVIVING case; only {non_trivial} did — the rows that \
         did not are proving nothing",
        rows.len()
    );
}

/// The value, spelled out. `x` is index 1 inside the inner binder (`y` took 0),
/// it survives the inner binder's single name, and it becomes index 0.
///
/// This row is redundant with the law and it is here anyway: it is the one a
/// reader can check by hand, and it pins the concrete bytes the defect report
/// named — pre-fix these all read `[0]`, the empty set with a trailing zero.
#[test]
fn the_surviving_index_is_the_member_not_the_position() {
    for row in every_row() {
        assert_eq!(
            row.body_before_binding,
            vec![0, 1],
            "{}: the body names index 1 (`x`), and index 0 is the node's own name",
            row.what
        );
        assert_eq!(
            row.node,
            vec![1],
            "{}: index 1 survives one binder as index 0, which is `[1]` — NOT `[0]`, which \
             is the empty set with a trailing zero",
            row.what
        );
        row.check();
    }
}

/// The complement: the binder that OWNS the index discharges it, and the whole
/// program is closed. A reader that simply passed its body's bitset through
/// would fail here, so the two rows together bracket the operation.
#[test]
fn the_owning_binder_discharges_the_index() {
    for src in [
        r#"for (@x <- @"c") { new y in { @"o"!(x) } } "#,
        r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(x) } }"#,
        r#"for (@x <- @"c") { contract @"k"(@y) = { @"o"!(x) } }"#,
        r#"for (@x <- @"c") { match 10 { y => @"o"!(x) } }"#,
    ] {
        let par = compile(src);
        assert!(
            par.receives[0].locally_free.is_empty(),
            "{src}: the outer `for` binds `x`, so IT is where the bit is discharged"
        );
        assert!(
            par.locally_free.is_empty(),
            "{src}: and the whole program is closed"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ WHICH CONSENSUS LANE SEES THIS BYTE
//
// Not an assertion about whether the fix is right — a measurement of where its
// output can be observed. `models/build.rs` blanks `locally_free` on the serde
// path only; prost retains it. Both rows below flip one nested `Receive`'s
// bitset from the well-formed `[1]` to the pre-fix `[0]` and ask each encoder
// whether it noticed.
// ─────────────────────────────────────────────────────────────────────────────

/// A copy of `par` in which the single nested `Receive`'s `locally_free` is
/// replaced by `replacement`. Nothing else moves.
fn with_nested_receive_bitset(par: &Par, replacement: Vec<u8>) -> Par {
    let mut mutated = par.clone();
    let outer = mutated
        .receives
        .first_mut()
        .expect("one outer `for`")
        .body
        .as_mut()
        .expect("outer Receive.body");
    let inner = outer.receives.first_mut().expect("one inner `for`");
    assert_eq!(
        inner.locally_free,
        vec![1],
        "the nested Receive must be carrying the well-formed value before it is perturbed"
    );
    inner.locally_free = replacement;
    mutated
}

/// ⚠ Lane P (protobuf / prost) DOES see it. `RhoTypes.proto:123` declares
/// `bytes locallyFree = 5` on `Receive`, and `protobuf_schema.rs` §B.1 records that
/// the field is retained on this lane. `cost_accounting/sig.rs:255,291` signs
/// `sort_match(&par).term.encode_to_vec()` — this encoder, on the canonical
/// form — so the byte is inside a signed preimage.
#[test]
fn the_protobuf_lane_sees_the_bitset() {
    let par = compile(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(x) } }"#);
    let pre_fix = with_nested_receive_bitset(&par, vec![0]);

    let canonical = |p: &Par| ParSortMatcher::sort_match(p).term.encode_to_vec();
    assert_ne!(
        canonical(&par),
        canonical(&pre_fix),
        "a nested Receive's `locally_free` is part of the canonical protobuf preimage, so the \
         two readings of the binder shift produce different signed bytes"
    );
}

/// ★ Lane B (bincode / the RSpace channel-hash preimage) does NOT see it.
/// `models/build.rs:162-170` injects `serialize_with =
/// serialize_as_empty_bytes` on every `locally_free` declaration and asserts its
/// own rewrite count against the schema-code generator's `EmptyBytes` count, so
/// the blanking is total rather than per-message.
#[test]
fn the_bincode_lane_does_not_see_the_bitset() {
    let par = compile(r#"for (@x <- @"c") { for (@y <- @"d") { @"o"!(x) } }"#);
    let pre_fix = with_nested_receive_bitset(&par, vec![0]);

    let stable = |p: &Par| bincode::serialize(p).expect("a Par must bincode-serialize");
    assert_eq!(
        stable(&par),
        stable(&pre_fix),
        "`locally_free` is blanked on the serde path, so the two readings of the binder shift \
         are indistinguishable in an RSpace channel-hash preimage"
    );
}
