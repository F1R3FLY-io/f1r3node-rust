//! # The sorter's canonical form, pinned byte-for-byte
//!
//! ## Why this file exists
//!
//! `ParSortMatcher::sort_match` decides the **canonical form**, and
//! `rholang/src/rust/interpreter/accounting/cost_accounting/sig.rs` computes
//! `ParSortMatcher::sort_match(&par).term.encode_to_vec()` — *the bytes that
//! get signed*. A one-element reordering is a consensus fork, not a
//! performance regression.
//!
//! Leg-2 Stage C-2 converts the sorter from mutual recursion to an explicit
//! heap worklist (`sorter/sort_drive.rs`) over a **single-sourced** per-arm
//! assembly table (`sorter/sort_combine.rs`). A differential against a
//! recursive oracle that shares that same table proves the two *traversal
//! shapes* agree — but it cannot catch an error transcribed **into the shared
//! table itself**, because both sides would then be wrong together.
//!
//! This file closes that hole. It pins the sorter's observable output —
//! the encoded term and a canonical rendering of the score tree — against a
//! golden fixture captured from the **pre-conversion** implementation, so a
//! transcription error in the arm table is a loud, diffable failure naming the
//! corpus entry that moved.
//!
//! ```text
//!               corpus entry
//!                     │
//!         ┌───────────┴────────────┐
//!         ▼                        ▼
//!   sort_match(x).term      sort_match(x).score
//!         │                        │
//!   encode_to_vec()          render_score()
//!         │                        │
//!         └────────► line ◄────────┘
//!                     │
//!                     ▼
//!        tests/golden/sorter_canonical_forms.txt
//! ```
//!
//! ## Regenerating
//!
//! ```bash
//! SORTER_GOLDEN_BLESS=1 cargo test -p models --test models_tests -- sorter_canonical
//! ```
//!
//! ⚠ **Re-blessing is a consensus-visible act.** The golden file is the
//! canonical form. Regenerate it only when the canonical form is *intended* to
//! change, and say so in the commit message.
//!
//! ### ★ Blessing log — `EPathMap` entries move into the trie (2 lines)
//!
//! `expr/EZipperBody` and `par-of-expr/EZipperBody` moved, and the shape of the
//! move is worth recording because it is the opposite of alarming:
//!
//! ```text
//! - (i14 (i999 (i2 i9) i0) (i999 (i2 i3) i0) i0)     score: 9 then 3
//! + (i14 (i999 (i2 i3) i0) (i999 (i2 i9) i0) i0)     score: 3 then 9
//! ```
//!
//! **The hex column — `sort_match(&par).term.encode_to_vec()`, the byte string
//! `cost_accounting/sig.rs` signs — is BYTE-IDENTICAL on both sides**
//! (`…420c020000000306020000000312`). Only the SCORE tree moved.
//!
//! And it moved into agreement with those bytes. `42 0c` is proto field 8, and
//! its two length-framed keys are `03 06` then `03 12` — zigzag 6 and 18, i.e.
//! **3 then 9**: the emitted term has been in trie order all along. The score
//! tree read the zipper's `ps` in the order a producer wrote it, so the sorter
//! was scoring one order while signing another. Making the entries live in the
//! trie removed the producer's order, and the two now read the same sequence.
//!
//! So this bless does not move a signature; it removes a disagreement between a
//! signature and the score that was supposed to explain it.
//!
//! ## What the corpus must reach, and why
//!
//! * **Every `ExprInstance` variant** (all 36) — an arm that is never exercised
//!   is an arm whose transcription is unchecked. [`corpus_covers_every_expr_instance_variant`]
//!   fails if the schema grows past what is covered here.
//! * **Multi-sibling terms.** A `Combine` that pops its children in the wrong
//!   order still produces a same-multiset result, so *permutation* errors are
//!   invisible on single-child terms. Only multi-sibling entries can catch
//!   them, which is why every collection entry carries at least two elements
//!   with **distinct** scores.
//! * **Every top-level `Sortable`** — `Par`, `Expr`, `Send`, `Receive`,
//!   `ReceiveBind`, `New`, `Match`, `If`, `Bundle`, `Connective`,
//!   `GUnforgeable`, `Var` — because each has its own entry point and its own
//!   assembly arm.
//! * **Nesting**, so that a driver whose descent order differs from the
//!   recursive form's shows up.
//!
//! ## ⚠ Determinism: why sets and maps carry distinct-scoring elements only
//!
//! `SortedParHashSet` / `SortedParMap` route their elements through a
//! `std::collections::HashSet` / `HashMap`, whose iteration order comes from a
//! per-instance `RandomState` seed. `ScoredTerm::sort_vec` is a **stable**
//! sort, so two elements with *equal* scores would keep that (randomised)
//! relative order and this fixture would flake. Every `ESet` / `EMap` /
//! `EPathMap` entry below therefore uses elements whose scores are pairwise
//! distinct, which makes the sorted order total and reproducible. That is a
//! statement about the fixture, not a claim about the sorter: the underlying
//! order-dependence is pre-existing and is documented here rather than hidden.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap,
    EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap,
    EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GDeployId,
    GBigRational, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken, GUnforgeable, If, KeyValuePair, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send, Var, VarRef,
};
use models::rust::rholang::sorter::bundle_sort_matcher::BundleSortMatcher;
use models::rust::rholang::sorter::connective_sort_matcher::ConnectiveSortMatcher;
use models::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use models::rust::rholang::sorter::if_sort_matcher::IfSortMatcher;
use models::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
use models::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use models::rust::rholang::sorter::score_tree::{ScoreAtom, ScoredTerm, Tree};
use models::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::rholang::sorter::unforgeable_sort_matcher::UnforgeableSortMatcher;
use models::rust::rholang::sorter::var_sort_matcher::VarSortMatcher;
use prost::Message;
use std::collections::BTreeMap;

const GOLDEN_PATH: &str = "tests/golden/sorter_canonical_forms.txt";

/// The number of `ExprInstance` variants in `RhoTypes.proto`, mirrored from
/// [`models::rust::rholang::par_children::EXPR_INSTANCE_VARIANT_COUNT`] so that
/// this file fails to compile-and-pass if the schema grows without the corpus
/// growing with it.
const EXPR_INSTANCE_VARIANT_COUNT: usize =
    models::rust::rholang::par_children::EXPR_INSTANCE_VARIANT_COUNT;

// ---------------------------------------------------------------------------
// rendering — the observable
// ---------------------------------------------------------------------------

/// A compact, total rendering of a score tree.
///
/// `Debug` would work but is verbose and would make a diff unreadable; this
/// renders `Node[Leaf(2), Node[Leaf(7)]]` as `(2 (7))`, with the three
/// `TaggedAtom` kinds distinguished by prefix so that `IntAtom(1)`,
/// `StringAtom("1")` and `BytesAtom([0x31])` cannot collide.
fn render_score(t: &Tree<ScoreAtom>) -> String {
    // Iterative, because the whole point of Stage C-1 is that nothing in this
    // family walks a score tree with the native stack.
    enum Step<'a> {
        Visit(&'a Tree<ScoreAtom>),
        Close,
    }
    let mut out = String::new();
    let mut work: Vec<Step<'_>> = vec![Step::Visit(t)];
    let mut need_space = false;
    while let Some(step) = work.pop() {
        match step {
            Step::Visit(Tree::Leaf(atom)) => {
                if need_space {
                    out.push(' ');
                }
                out.push_str(&render_atom(atom));
                need_space = true;
            }
            Step::Visit(Tree::Node(children)) => {
                if need_space {
                    out.push(' ');
                }
                out.push('(');
                need_space = false;
                work.push(Step::Close);
                for child in children.iter().rev() {
                    work.push(Step::Visit(child));
                }
            }
            Step::Close => {
                out.push(')');
                need_space = true;
            }
        }
    }
    out
}

/// `ScoreAtom`'s payload is private, so its kind is recovered from `Debug`.
/// The three kinds are rendered with distinct prefixes (`i`, `s`, `b`).
fn render_atom(atom: &ScoreAtom) -> String {
    let d = format!("{:?}", atom);
    // `ScoreAtom { value: IntAtom(7) }`
    let inner = d
        .split_once("value: ")
        .map(|(_, rest)| rest.trim_end_matches(" }").to_string())
        .unwrap_or(d);
    if let Some(rest) = inner.strip_prefix("IntAtom(") {
        format!("i{}", rest.trim_end_matches(')'))
    } else if let Some(rest) = inner.strip_prefix("StringAtom(") {
        format!("s{}", rest.trim_end_matches(')'))
    } else if let Some(rest) = inner.strip_prefix("BytesAtom(") {
        format!("b{}", rest.trim_end_matches(')').replace(' ', ""))
    } else {
        format!("?{}", inner)
    }
}

fn line<T: Message>(name: &str, scored: ScoredTerm<T>) -> String {
    format!(
        "{}\t{}\t{}",
        name,
        hex::encode(scored.term.encode_to_vec()),
        render_score(&scored.score)
    )
}

/// `Var` is a `prost::Message`, but `ReceiveBind` needs a wrapper to be encoded
/// on its own; both go through the same shape so the fixture is uniform.
fn line_debug<T: std::fmt::Debug>(name: &str, scored: ScoredTerm<T>) -> String {
    format!(
        "{}\t{:?}\t{}",
        name,
        scored.term,
        render_score(&scored.score)
    )
}

// ---------------------------------------------------------------------------
// corpus builders
// ---------------------------------------------------------------------------

fn gint(v: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }],
        ..Default::default()
    }
}

fn gstring(v: &str) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(v.to_string())),
        }],
        ..Default::default()
    }
}

fn expr_par(ei: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ei),
        }],
        ..Default::default()
    }
}

fn a() -> Option<Par> {
    Some(gint(1))
}
fn b() -> Option<Par> {
    Some(gint(2))
}

/// `[[[…[0]…]]]` — the nesting axis, so a driver whose descent order differs
/// from the recursive form's is visible.
fn nested_list(depth: usize) -> Par {
    let mut p = gint(0);
    for i in 0..depth {
        p = expr_par(ExprInstance::EListBody(EList {
            ps: vec![p, gint(i as i64 + 10)],
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        }));
    }
    p
}

fn pathmap_of(ps: Vec<Par>) -> EPathMap {
    EPathMap::new(ps, Vec::new(), false, None)
}

/// A set of two distinct scalars — the innermost rung of the nesting fixtures.
fn set_of(ps: Vec<Par>) -> Par {
    expr_par(ExprInstance::ESetBody(ESet {
        ps,
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }))
}

fn kv(key: Par, value: Par) -> KeyValuePair {
    KeyValuePair {
        key: Some(key),
        value: Some(value),
    }
}

/// ★★ **The depth-≥2 discriminator: a set inside a map inside a set.**
///
/// ⚠ Why this exists. `combine_eset` / `combine_emap` / `combine_epathmap` are the
/// three arms that re-enter [`ParSortMatcher::sort_match`] **once per nesting
/// level** — they are precisely what stack-safety Phase 3b converts. Until this
/// fixture the corpus never made them re-enter **even once**: every collection in it
/// was depth 1 and scalar-only (`ESet [9,3,"m"]`, `EMap [9→90, 3→30]`,
/// `EPathmap [9,3]`, none of whose elements is itself a collection).
///
/// ⇒ A conversion of those arms could have been wrong at every level below the first
/// and this golden would have been byte- **and** score-identical. A one-element — or
/// one-level — collection has exactly one permutation, so it cannot separate any
/// ordering hypothesis from any other.
///
/// ★ The middle map nests on **both** sides: one pair carries a collection as its
/// KEY, the other carries one as its VALUE, so the key path and the value path are
/// each exercised. Every collection here carries ≥ 2 elements with distinct scores,
/// per this module's determinism rule.
fn nested_set_in_map_in_set() -> ExprInstance {
    let middle = expr_par(ExprInstance::EMapBody(EMap {
        kvs: vec![
            kv(set_of(vec![gint(1), gint(2)]), gint(5)),
            kv(gint(7), set_of(vec![gint(4), gint(6)])),
        ],
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }));
    ExprInstance::ESetBody(ESet {
        ps: vec![middle, gint(3)],
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    })
}

/// ★ **The ANTI-MONOTONE map — `3 → 90`, `9 → 30`.**
///
/// ⚠ The corpus's existing `EMapBody` row is **monotone** (`9→90, 3→30`), so
/// key-order and key⊕value-order agree on it. `sort_key_value_pair`
/// (`sort_combine.rs:1442-1450`) keeps only the **key's** score and DISCARDS the
/// value's; a converted arm has both scores on the value stack, where combining them
/// is the natural thing to write and is wrong. On a monotone fixture that defect
/// still produces the same order and is invisible.
///
/// ⇒ Anti-monotone pairing is what separates the two hypotheses by ORDER, rather
/// than relying solely on the recorded score column.
fn anti_monotone_map() -> ExprInstance {
    ExprInstance::EMapBody(EMap {
        kvs: vec![kv(gint(3), gint(90)), kv(gint(9), gint(30))],
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    })
}

/// One representative per `ExprInstance` variant. Collections carry ≥ 2
/// elements with **distinct** scores (see the module docs on determinism).
fn expr_instance_corpus() -> Vec<(&'static str, ExprInstance)> {
    vec![
        ("GBool", ExprInstance::GBool(true)),
        ("GInt", ExprInstance::GInt(-7)),
        ("GString", ExprInstance::GString("zz".to_string())),
        ("GUri", ExprInstance::GUri("rho:io:stdout".to_string())),
        ("GByteArray", ExprInstance::GByteArray(vec![0x00, 0xff])),
        ("GDouble", ExprInstance::GDouble(2.5f64.to_bits())),
        ("GBigInt", ExprInstance::GBigInt(vec![1, 2, 3])),
        (
            "GBigRat",
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![3],
                denominator: vec![4],
            }),
        ),
        (
            "GFixedPoint",
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![9],
                scale: 3,
            }),
        ),
        (
            "EVarBody/bound",
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(3)),
                }),
            }),
        ),
        (
            "EVarBody/free",
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(4)),
                }),
            }),
        ),
        ("ENotBody", ExprInstance::ENotBody(ENot { p: a() })),
        ("ENegBody", ExprInstance::ENegBody(ENeg { p: a() })),
        (
            "EMultBody",
            ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
        ),
        ("EDivBody", ExprInstance::EDivBody(EDiv { p1: a(), p2: b() })),
        ("EModBody", ExprInstance::EModBody(EMod { p1: a(), p2: b() })),
        (
            "EPlusBody",
            ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
        ),
        (
            "EMinusBody",
            ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
        ),
        (
            "EPlusPlusBody",
            ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
        ),
        (
            "EMinusMinusBody",
            ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
        ),
        (
            "EPercentPercentBody",
            ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
        ),
        ("ELtBody", ExprInstance::ELtBody(ELt { p1: a(), p2: b() })),
        ("ELteBody", ExprInstance::ELteBody(ELte { p1: a(), p2: b() })),
        ("EGtBody", ExprInstance::EGtBody(EGt { p1: a(), p2: b() })),
        ("EGteBody", ExprInstance::EGteBody(EGte { p1: a(), p2: b() })),
        ("EEqBody", ExprInstance::EEqBody(EEq { p1: a(), p2: b() })),
        ("ENeqBody", ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() })),
        ("EAndBody", ExprInstance::EAndBody(EAnd { p1: a(), p2: b() })),
        ("EOrBody", ExprInstance::EOrBody(EOr { p1: a(), p2: b() })),
        (
            "EMatchesBody",
            ExprInstance::EMatchesBody(EMatches {
                target: a(),
                pattern: b(),
            }),
        ),
        (
            "EListBody",
            ExprInstance::EListBody(EList {
                // Deliberately DESCENDING, so a list whose elements were
                // reordered would be visible. `EList` preserves order.
                ps: vec![gint(9), gint(3), gstring("m")],
                locally_free: vec![],
                connective_used: true,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            }),
        ),
        (
            "ETupleBody",
            ExprInstance::ETupleBody(ETuple {
                ps: vec![gint(9), gint(3)],
                locally_free: vec![],
                connective_used: false,
            }),
        ),
        (
            "ESetBody",
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(9), gint(3), gstring("m")],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
        ),
        (
            "EMapBody",
            ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(gint(9)),
                        value: Some(gint(90)),
                    },
                    KeyValuePair {
                        key: Some(gint(3)),
                        value: Some(gint(30)),
                    },
                ],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            }),
        ),
        (
            "EPathmapBody",
            ExprInstance::EPathmapBody(pathmap_of(vec![gint(9), gint(3)])),
        ),
        // ★★ The three depth-≥2 rows. See the helpers' docs: the arms these exercise
        // are the ones Phase 3b converts, and before these rows the corpus never made
        // any of them re-enter `sort_match` even once.
        (
            "ESetBody-nested-set-in-map-in-set",
            nested_set_in_map_in_set(),
        ),
        ("EMapBody-anti-monotone", anti_monotone_map()),
        (
            "EPathmapBody-nested",
            ExprInstance::EPathmapBody(pathmap_of(vec![
                set_of(vec![gint(1), gint(2)]),
                gint(8),
            ])),
        ),
        (
            "EZipperBody",
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(pathmap_of(vec![gint(9), gint(3)])),
                ..Default::default()
            }),
        ),
        (
            "EMethodBody",
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(gint(5)),
                arguments: vec![gint(9), gint(3)],
                locally_free: vec![],
                connective_used: true,
            }),
        ),
    ]
}

fn connective_instance_corpus() -> Vec<(&'static str, ConnectiveInstance)> {
    vec![
        (
            "ConnAndBody",
            ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![gint(9), gint(3)],
            }),
        ),
        (
            "ConnOrBody",
            ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![gint(9), gint(3)],
            }),
        ),
        ("ConnNotBody", ConnectiveInstance::ConnNotBody(gint(4))),
        (
            "VarRefBody",
            ConnectiveInstance::VarRefBody(VarRef { index: 1, depth: 2 }),
        ),
        ("ConnBool", ConnectiveInstance::ConnBool(true)),
        ("ConnInt", ConnectiveInstance::ConnInt(false)),
        ("ConnString", ConnectiveInstance::ConnString(true)),
        ("ConnUri", ConnectiveInstance::ConnUri(false)),
        ("ConnByteArray", ConnectiveInstance::ConnByteArray(true)),
    ]
}

fn send(persistent: bool) -> Send {
    Send {
        chan: Some(gstring("ch")),
        data: vec![gint(9), gint(3), gstring("d")],
        persistent,
        locally_free: vec![1],
        connective_used: true,
    }
}

fn receive_bind(with_remainder: bool) -> ReceiveBind {
    ReceiveBind {
        patterns: vec![gint(9), gint(3)],
        source: Some(gstring("src")),
        remainder: if with_remainder {
            Some(Var {
                var_instance: Some(VarInstance::FreeVar(2)),
            })
        } else {
            None
        },
        free_count: 2,
    }
}

fn receive(with_condition: bool, peek: bool, persistent: bool) -> Receive {
    Receive {
        binds: vec![receive_bind(true), receive_bind(false)],
        body: Some(gint(7)),
        persistent,
        peek,
        bind_count: 4,
        locally_free: vec![2],
        connective_used: false,
        condition: if with_condition { Some(gint(1)) } else { None },
    }
}

fn new_node() -> New {
    let mut injections = BTreeMap::new();
    injections.insert("zeta".to_string(), gint(9));
    injections.insert("alpha".to_string(), gint(3));
    New {
        bind_count: 2,
        p: Some(gint(7)),
        uri: vec!["rho:z".to_string(), "rho:a".to_string()],
        injections,
        locally_free: vec![3],
    }
}

fn match_case(with_guard: bool) -> MatchCase {
    MatchCase {
        pattern: Some(gint(9)),
        source: Some(gint(3)),
        free_count: 1,
        guard: if with_guard { Some(gint(5)) } else { None },
    }
}

fn match_node() -> Match {
    Match {
        target: Some(gstring("t")),
        cases: vec![match_case(true), match_case(false)],
        locally_free: vec![4],
        connective_used: true,
    }
}

fn if_node() -> If {
    If {
        condition: Some(gint(1)),
        if_true: Some(gint(2)),
        if_false: Some(gint(3)),
        locally_free: vec![5],
        connective_used: false,
    }
}

fn bundle(write_flag: bool, read_flag: bool) -> Bundle {
    Bundle {
        body: Some(gint(6)),
        write_flag,
        read_flag,
    }
}

fn unforgeable_corpus() -> Vec<(&'static str, GUnforgeable)> {
    vec![
        (
            "GPrivateBody",
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![7, 7] })),
            },
        ),
        (
            "GDeployerIdBody",
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                    public_key: vec![1, 2],
                })),
            },
        ),
        (
            "GDeployIdBody",
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![3, 4] })),
            },
        ),
        (
            "GSysAuthTokenBody",
            GUnforgeable {
                unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
            },
        ),
        ("None", GUnforgeable { unf_instance: None }),
    ]
}

fn var_corpus() -> Vec<(&'static str, Var)> {
    vec![
        (
            "BoundVar",
            Var {
                var_instance: Some(VarInstance::BoundVar(5)),
            },
        ),
        (
            "FreeVar",
            Var {
                var_instance: Some(VarInstance::FreeVar(6)),
            },
        ),
        (
            "Wildcard",
            Var {
                var_instance: Some(VarInstance::Wildcard(models::rhoapi::var::WildcardMsg {})),
            },
        ),
        ("None", Var { var_instance: None }),
    ]
}

/// A `Par` with **every** category populated, each with several members whose
/// scores differ — the entry that catches a `Combine` popping category results
/// in the wrong order.
fn full_par() -> Par {
    Par {
        sends: vec![send(true), send(false)],
        receives: vec![receive(true, false, false), receive(false, true, true)],
        news: vec![new_node()],
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("e".to_string())),
            },
        ],
        matches: vec![match_node()],
        unforgeables: vec![
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![9] })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![3] })),
            },
        ],
        bundles: vec![bundle(true, false), bundle(false, true)],
        connectives: vec![
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnBool(true)),
            },
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnInt(true)),
            },
        ],
        conditionals: vec![if_node()],
        locally_free: vec![1, 2, 3],
        connective_used: true,
    }
}

// ---------------------------------------------------------------------------
// the fixture
// ---------------------------------------------------------------------------

fn actual_lines() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(128);

    // --- Expr, one entry per ExprInstance variant, standalone and inside a Par
    for (name, ei) in expr_instance_corpus() {
        let e = Expr {
            expr_instance: Some(ei.clone()),
        };
        out.push(line(&format!("expr/{}", name), ExprSortMatcher::sort_match(&e)));
        let p = expr_par(ei);
        out.push(line(&format!("par-of-expr/{}", name), ParSortMatcher::sort_match(&p)));
    }
    out.push(line(
        "expr/None",
        ExprSortMatcher::sort_match(&Expr {
            expr_instance: None,
        }),
    ));

    // --- Connective
    for (name, ci) in connective_instance_corpus() {
        let c = Connective {
            connective_instance: Some(ci),
        };
        out.push(line(
            &format!("connective/{}", name),
            ConnectiveSortMatcher::sort_match(&c),
        ));
    }
    out.push(line(
        "connective/None",
        ConnectiveSortMatcher::sort_match(&Connective {
            connective_instance: None,
        }),
    ));

    // --- the remaining top-level Sortables
    out.push(line("send/persistent", SendSortMatcher::sort_match(&send(true))));
    out.push(line("send/linear", SendSortMatcher::sort_match(&send(false))));
    out.push(line(
        "receive/cond+linear",
        ReceiveSortMatcher::sort_match(&receive(true, false, false)),
    ));
    out.push(line(
        "receive/nocond+peek+persistent",
        ReceiveSortMatcher::sort_match(&receive(false, true, true)),
    ));
    out.push(line_debug(
        "bind/remainder",
        ReceiveSortMatcher::sort_bind(receive_bind(true)),
    ));
    out.push(line_debug(
        "bind/no-remainder",
        ReceiveSortMatcher::sort_bind(receive_bind(false)),
    ));
    out.push(line("new", NewSortMatcher::sort_match(&new_node())));
    out.push(line("match", MatchSortMatcher::sort_match(&match_node())));
    out.push(line("if", IfSortMatcher::sort_match(&if_node())));
    for (w, r) in [(false, false), (false, true), (true, false), (true, true)] {
        out.push(line(
            &format!("bundle/w{}r{}", w as u8, r as u8),
            BundleSortMatcher::sort_match(&bundle(w, r)),
        ));
    }
    for (name, u) in unforgeable_corpus() {
        out.push(line(
            &format!("unforgeable/{}", name),
            UnforgeableSortMatcher::sort_match(&u),
        ));
    }
    for (name, v) in var_corpus() {
        out.push(line(&format!("var/{}", name), VarSortMatcher::sort_match(&v)));
    }

    // --- whole-Par shapes
    out.push(line("par/empty", ParSortMatcher::sort_match(&Par::default())));
    out.push(line("par/full", ParSortMatcher::sort_match(&full_par())));
    for depth in [1usize, 2, 3, 8] {
        out.push(line(
            &format!("par/nested-list-{}", depth),
            ParSortMatcher::sort_match(&nested_list(depth)),
        ));
    }
    // A Par whose `exprs` are a PERMUTATION of `par/full`'s: the canonical form
    // must be identical, which is the normalizer property the reducer relies on.
    let mut permuted = full_par();
    permuted.exprs.reverse();
    permuted.sends.reverse();
    permuted.bundles.reverse();
    out.push(line("par/full-permuted", ParSortMatcher::sort_match(&permuted)));

    out
}

fn golden_file() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_PATH)
}

#[test]
fn sorter_canonical_forms_match_the_golden_fixture() {
    let actual = actual_lines();

    if std::env::var("SORTER_GOLDEN_BLESS").is_ok() {
        let path = golden_file();
        std::fs::create_dir_all(path.parent().expect("golden path has a parent"))
            .expect("could not create the golden directory");
        let mut body = String::from(
            "# THE SORTER'S CANONICAL FORM — captured from the PRE-conversion (recursive)\n\
             # implementation and pinned here. `sort_match(&par).term.encode_to_vec()` is what\n\
             # cost_accounting/sig.rs signs, so a diff in this file is a CONSENSUS FORK.\n\
             # Regenerate with SORTER_GOLDEN_BLESS=1 only when the change is intended.\n\
             # columns: name <TAB> hex(term.encode_to_vec()) <TAB> render_score(score)\n",
        );
        for l in &actual {
            body.push_str(l);
            body.push('\n');
        }
        std::fs::write(&path, body).expect("could not write the golden fixture");
        println!("blessed {} ({} entries)", path.display(), actual.len());
        return;
    }

    let raw = std::fs::read_to_string(golden_file()).unwrap_or_else(|e| {
        panic!(
            "could not read {} ({e}). Capture it with \
             SORTER_GOLDEN_BLESS=1 cargo test -p models --test models_tests -- sorter_canonical",
            golden_file().display()
        )
    });
    let expected: Vec<&str> = raw
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();

    assert_eq!(
        actual.len(),
        expected.len(),
        "the corpus changed size: {} entries now, {} in the golden fixture. \
         Adding coverage requires re-blessing; check the diff is ONLY additions.",
        actual.len(),
        expected.len()
    );

    let mut diffs: Vec<String> = Vec::new();
    for (got, want) in actual.iter().zip(expected.iter()) {
        if got != want {
            diffs.push(format!("  - want: {}\n  - got:  {}", want, got));
        }
    }
    assert!(
        diffs.is_empty(),
        "★ THE CANONICAL FORM MOVED. `sort_match(&par).term.encode_to_vec()` is the byte \
         string cost_accounting/sig.rs signs; a difference here is a consensus fork, not a \
         performance regression. {} of {} corpus entries differ:\n{}",
        diffs.len(),
        expected.len(),
        diffs.join("\n")
    );
}

/// An arm that is never exercised is an arm whose transcription is unchecked.
///
/// ★ Asserted as the number of **distinct** `ExprInstance` variants the corpus reaches, not
/// as `len() == COUNT + <allowance>`.
///
/// ⚠ The previous form was `expr_instance_corpus().len() == EXPR_INSTANCE_VARIANT_COUNT + 1`,
/// where the `+ 1` was a hand-written allowance for `EVarBody` appearing twice (bound / free).
/// That is this campaign's most-repeated defect — a transcribed numeral standing beside a
/// table that can compute it — and here it had a second cost: **it punished added coverage.**
/// The three depth-≥2 rows this file gained for stack-safety Phase 3b are deliberate second
/// representatives of `ESetBody` / `EMapBody` / `EPathmapBody`, and under the old form each
/// would have had to be paid for by editing the constant. A coverage guard that must be
/// re-transcribed every time coverage improves is on its way to becoming a coverage *ceiling*.
///
/// ⇒ Counting distinct discriminants states what the test actually means — every variant is
/// reached — and is indifferent to how many representatives each variant has.
#[test]
fn corpus_covers_every_expr_instance_variant() {
    let corpus = expr_instance_corpus();
    let distinct: std::collections::HashSet<std::mem::Discriminant<ExprInstance>> = corpus
        .iter()
        .map(|(_, ei)| std::mem::discriminant(ei))
        .collect();

    assert_eq!(
        distinct.len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        "the ExprInstance corpus reaches {} distinct variants; RhoTypes.proto defines {}. \
         ({} corpus entries in total, so {} are additional representatives of a variant \
         already covered — those are fine and deliberate.)\n\n\
         FEWER than the variant count: a variant was added to the schema but not to this \
         fixture, so its sorter arm is unpinned and a transcription error in it would pass \
         silently.\n\
         MORE than the variant count: impossible by construction — a discriminant set cannot \
         exceed the number of variants — so this direction means \
         `EXPR_INSTANCE_VARIANT_COUNT` is now understated and is itself the stale figure.",
        distinct.len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        corpus.len(),
        corpus.len() - distinct.len()
    );
}

/// The fixture would be worthless if it flaked. Sets and maps route through a
/// randomly-seeded `HashSet`, so this asserts the corpus's determinism claim
/// directly rather than trusting it.
#[test]
fn the_fixture_is_deterministic_across_repeated_runs() {
    let first = actual_lines();
    for round in 1..8 {
        let again = actual_lines();
        assert_eq!(
            first, again,
            "the golden corpus is not deterministic (round {round}). Some entry's order \
             depends on HashSet iteration order — see the module docs: every ESet / EMap / \
             EPathMap entry must carry elements with pairwise DISTINCT scores."
        );
    }
}

