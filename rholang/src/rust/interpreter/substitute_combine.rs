//! # The substitution SCC's arm table — split and rebuild, single-sourced
//!
//! Leg-2 replaces substitution's call stack with an explicit heap worklist. A
//! worklist driver and a recursive oracle twin must agree on **every** arm of a
//! 36-variant schema; writing the arms twice guarantees they eventually will
//! not. So each arm is factored into two pure functions:
//!
//! * `split_*` — take an owned node apart into `(arm descriptor, child Pars)`.
//!   The descriptor carries everything that is **not** a child: flags, cached
//!   bitsets, method names, remainders, counts.
//! * `rebuild_*` — put a node back together from `(arm descriptor, substituted
//!   child Pars, env shift)`.
//!
//! The recursive twin recurses between the two; the driver worklists between
//! the two. Neither owns the arm semantics, so an arm cannot diverge between
//! them — the failure mode that put an `EMinus → EPlus` rewrite in the dead
//! `SubstituteTrait<Expr>::substitute` while its live twin rebuilt `EMinus`
//! correctly.
//!
//! ## The round-trip guard
//!
//! Because both paths share these functions, a bug *inside* them is invisible
//! to a differential between the paths. `rebuild_expr_instance(split(e), the
//! same children, shift = 0)` must therefore equal `e` for every arm, and
//! `expr_instance_round_trip_is_the_identity` asserts exactly that over the
//! constructed corpus. That is the test which catches the `EMinus → EPlus`
//! class, and it is the reason a shared table is safer than two copies rather
//! than merely shorter.
//!
//! ## What is deliberately NOT descended into
//!
//! `EPathmapBody` and `EZipperBody` carry `Par` payloads and are returned
//! untouched, because that is what `SubstituteTrait<Expr>::substitute_no_sort`
//! does today (they fall to its catch-all arm). Descending into them would
//! change substituted bytes, hence signed bytes, hence consensus. They appear
//! here as [`ExprArm::NoDescent`], which is a **record of current behaviour**,
//! and `models::rust::rholang::par_children::substitute_descends_into` carries
//! the same statement in checkable form.
//!
//! See `docs/design/audits/theta-depth-traversals-2026-07-26.md`.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMatches,
    EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus,
    EPlusPlus, ETuple, Expr, If, Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
};
use models::rust::bundle_ops::BundleOps;
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{concatenate_pars, single_bundle};
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;

use super::errors::InterpreterError;
use super::util::{prepend_connective, prepend_expr};

/// `locally_free` truncation at the environment shift. Moved here verbatim from
/// `substitute.rs` so that both the driver and the oracle read one copy.
pub(crate) fn set_bits_until(bits: Vec<u8>, until: i32) -> Vec<u8> {
    if until <= 0 {
        return Vec::new();
    }
    // Truncate the bitvector at `until` positions, preserving bit positions.
    // Matches Scala's BitSet.until(n).
    bits.into_iter().take(until as usize).collect()
}

// ===========================================================================
// Expr
// ===========================================================================

/// The unary `ExprInstance` arms: one child `Par`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnaryArm {
    ENot,
    ENeg,
}

/// The binary `ExprInstance` arms: two child `Par`s and no other data. Every
/// one of these is rebuilt as **its own** variant — see the round-trip guard.
///
/// ⚠ `clippy::enum_variant_names` objects that every variant starts with `E`.
/// That prefix is not a stutter to be trimmed: these names are a **1:1 mirror**
/// of the `ExprInstance` variants they split from and rebuild into
/// (`ExprInstance::EMultBody` -> `BinaryArm::EMult` -> `ExprInstance::EMultBody`),
/// and the round-trip guard is readable only because the correspondence is
/// spelled the same on both sides. Dropping the `E` would rename one half of a
/// pairing whose whole point is that the halves match.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryArm {
    EMult,
    EDiv,
    EMod,
    EPercentPercent,
    EPlus,
    EMinus,
    EPlusPlus,
    EMinusMinus,
    ELt,
    ELte,
    EGt,
    EGte,
    EEq,
    ENeq,
    EAnd,
    EOr,
    EMatches,
}

/// The child slots of an arm that are **pattern positions**: substituted at
/// `depth + 1` rather than at the enclosing depth, exactly as
/// `ReceiveBind::patterns` and `MatchCase::pattern` are in
/// `substitute_drive`'s `descend_bind` / `descend_case`.
///
/// ★ Single-sourced for the same reason `split_expr_instance` and
/// `rebuild_expr_instance` are: the worklist driver and the recursive oracle
/// both consult this, so a slot's depth cannot diverge between them.
///
/// The match on [`BinaryArm`] is written out rather than wildcarded, so adding
/// a two-slot arm does not silently inherit "no pattern slots" — the author
/// has to say which of the two positions binds.
pub(crate) fn expr_arm_pattern_slots(arm: &ExprArm) -> &'static [usize] {
    /// `EMatches`'s second slot, `pattern`.
    const SLOT_1: &[usize] = &[1];
    const NONE: &[usize] = &[];

    match arm {
        ExprArm::Binary(binary) => match binary {
            // `target matches pattern`. `p_matches_normalizer`'s
            // `combine_p_matches` normalizes the pattern under
            // `bound_map_chain.push()` and in a fresh `FreeMap`, so it is a
            // NESTED PATTERN one binding level deeper. The only construct that
            // can name an outer binder from inside it is `=x`, which
            // `BoundMapChain::find` emits as `VarRef { depth }` carrying the
            // chain distance — and `maybe_substitute_var_ref` fires only when
            // that `depth` equals the traversal's. Visiting the pattern at the
            // enclosing depth therefore strands exactly those `VarRef`s.
            //
            // The asymmetry with `has_locally_free`, which reads this node's
            // `locally_free`/`connective_used` from the TARGET alone, is not a
            // disagreement: a plain `x` in the pattern goes through
            // `BoundMapChain::get` (current scope only), so it is a fresh
            // binding occurrence and contributes nothing to the enclosing
            // scope's free variables. See `matcher::spatial_matcher`'s
            // `EMatchesBody` arm and `par_children::substitute_descends_into`.
            BinaryArm::EMatches => SLOT_1,

            BinaryArm::EMult
            | BinaryArm::EDiv
            | BinaryArm::EMod
            | BinaryArm::EPercentPercent
            | BinaryArm::EPlus
            | BinaryArm::EMinus
            | BinaryArm::EPlusPlus
            | BinaryArm::EMinusMinus
            | BinaryArm::ELt
            | BinaryArm::ELte
            | BinaryArm::EGt
            | BinaryArm::EGte
            | BinaryArm::EEq
            | BinaryArm::ENeq
            | BinaryArm::EAnd
            | BinaryArm::EOr => NONE,
        },

        // Operands, elements, entries, receivers and arguments — all ordinary
        // term positions at the enclosing depth.
        ExprArm::Unary(_)
        | ExprArm::EList { .. }
        | ExprArm::ETuple { .. }
        | ExprArm::ESet { .. }
        | ExprArm::EMap { .. }
        | ExprArm::EMethod { .. }
        | ExprArm::NoDescent(_) => NONE,
    }
}

/// An `ExprInstance` with its child `Par`s removed.
pub(crate) enum ExprArm {
    Unary(UnaryArm),
    Binary(BinaryArm),
    EList {
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<Var>,
    },
    ETuple {
        locally_free: Vec<u8>,
        connective_used: bool,
    },
    ESet {
        connective_used: bool,
        locally_free: Vec<u8>,
        remainder: Option<Var>,
    },
    /// Children are the flattened `(key, value)` pairs of the mapped
    /// `ParMap`'s `sorted_list`, in that order — so `children.len()` is even.
    EMap {
        connective_used: bool,
        locally_free: Vec<u8>,
        remainder: Option<Var>,
    },
    EMethod {
        method_name: String,
        locally_free: Vec<u8>,
        connective_used: bool,
    },
    /// Returned verbatim: grounds, `EVarBody` (handled by the caller before it
    /// ever reaches here in the `sub_exp` fold), and the two path-map arms the
    /// recursive form does not descend into.
    NoDescent(ExprInstance),
}

/// Take an `ExprInstance` apart. The returned children are in the order the
/// recursive form substitutes them, which is the order the driver must push
/// them and the order `rebuild_expr_instance` expects them back.
/// ⚠ The child list is `Vec<Option<Par>>`, not `Vec<Par>`.
///
/// The recursive form calls `unwrap_option_safe` on each `Option<Par>` operand
/// **immediately before descending into it**, so a term with a good `p1` and a
/// missing `p2` reports whatever `p1`'s substitution reports first, and only
/// then the missing-field error. Dropping `None`s here — or checking arity up
/// front — would move the error. Keeping the `Option` in position lets both the
/// driver and the oracle raise it exactly where the recursive form did.
pub(crate) fn split_expr_instance(instance: ExprInstance) -> (ExprArm, Vec<Option<Par>>) {
    /// Both operands of a binary arm, in `(p1, p2)` order, positions preserved.
    fn two(p1: Option<Par>, p2: Option<Par>) -> Vec<Option<Par>> { vec![p1, p2] }
    /// A list slot: every element is present by construction.
    fn present(ps: Vec<Par>) -> Vec<Option<Par>> { ps.into_iter().map(Some).collect() }

    match instance {
        ExprInstance::ENotBody(ENot { p }) => (ExprArm::Unary(UnaryArm::ENot), vec![p]),
        ExprInstance::ENegBody(ENeg { p }) => (ExprArm::Unary(UnaryArm::ENeg), vec![p]),

        ExprInstance::EMultBody(EMult { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EMult), two(p1, p2))
        }
        ExprInstance::EDivBody(EDiv { p1, p2 }) => (ExprArm::Binary(BinaryArm::EDiv), two(p1, p2)),
        ExprInstance::EModBody(EMod { p1, p2 }) => (ExprArm::Binary(BinaryArm::EMod), two(p1, p2)),
        ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EPercentPercent), two(p1, p2))
        }
        ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EPlus), two(p1, p2))
        }
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EMinus), two(p1, p2))
        }
        ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EPlusPlus), two(p1, p2))
        }
        ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
            (ExprArm::Binary(BinaryArm::EMinusMinus), two(p1, p2))
        }
        ExprInstance::ELtBody(ELt { p1, p2 }) => (ExprArm::Binary(BinaryArm::ELt), two(p1, p2)),
        ExprInstance::ELteBody(ELte { p1, p2 }) => (ExprArm::Binary(BinaryArm::ELte), two(p1, p2)),
        ExprInstance::EGtBody(EGt { p1, p2 }) => (ExprArm::Binary(BinaryArm::EGt), two(p1, p2)),
        ExprInstance::EGteBody(EGte { p1, p2 }) => (ExprArm::Binary(BinaryArm::EGte), two(p1, p2)),
        ExprInstance::EEqBody(EEq { p1, p2 }) => (ExprArm::Binary(BinaryArm::EEq), two(p1, p2)),
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => (ExprArm::Binary(BinaryArm::ENeq), two(p1, p2)),
        ExprInstance::EAndBody(EAnd { p1, p2 }) => (ExprArm::Binary(BinaryArm::EAnd), two(p1, p2)),
        ExprInstance::EOrBody(EOr { p1, p2 }) => (ExprArm::Binary(BinaryArm::EOr), two(p1, p2)),
        ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
            (ExprArm::Binary(BinaryArm::EMatches), two(target, pattern))
        }

        ExprInstance::EListBody(EList {
            ps,
            locally_free,
            connective_used,
            remainder,
        }) => (
            ExprArm::EList {
                locally_free,
                connective_used,
                remainder,
            },
            present(ps),
        ),
        ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free,
            connective_used,
        }) => (
            ExprArm::ETuple {
                locally_free,
                connective_used,
            },
            present(ps),
        ),
        ExprInstance::ESetBody(eset) => {
            // The recursive form maps through `ParSet` and substitutes the
            // SORTED elements, then re-sorts on rebuild. Reproduced exactly.
            let par_set = ParSetTypeMapper::eset_to_par_set(eset);
            (
                ExprArm::ESet {
                    connective_used: par_set.connective_used,
                    locally_free: par_set.locally_free,
                    remainder: par_set.remainder,
                },
                present(par_set.ps.sorted_pars),
            )
        }
        ExprInstance::EMapBody(emap) => {
            let par_map = ParMapTypeMapper::emap_to_par_map(emap);
            let mut children = Vec::with_capacity(par_map.ps.sorted_list.len() * 2);
            for (k, v) in par_map.ps.sorted_list {
                children.push(Some(k));
                children.push(Some(v));
            }
            (
                ExprArm::EMap {
                    connective_used: par_map.connective_used,
                    locally_free: par_map.locally_free,
                    remainder: par_map.remainder,
                },
                children,
            )
        }
        ExprInstance::EMethodBody(EMethod {
            method_name,
            target,
            arguments,
            locally_free,
            connective_used,
        }) => {
            let mut children = Vec::with_capacity(1 + arguments.len());
            children.push(target);
            children.extend(arguments.into_iter().map(Some));
            (
                ExprArm::EMethod {
                    method_name,
                    locally_free,
                    connective_used,
                },
                children,
            )
        }

        // ---- returned verbatim ----
        other @ (ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::EVarBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_)) => (ExprArm::NoDescent(other), Vec::new()),
    }
}

/// Put an `ExprInstance` back together from its substituted children.
///
/// `shift` is `env.shift` at this node — the value the recursive form reads to
/// truncate `locally_free`.
pub(crate) fn rebuild_expr_instance(
    arm: ExprArm,
    mut children: Vec<Par>,
    shift: i32,
) -> ExprInstance {
    fn take2(children: &mut Vec<Par>) -> (Option<Par>, Option<Par>) {
        let mut it = std::mem::take(children).into_iter();
        (it.next(), it.next())
    }

    match arm {
        ExprArm::Unary(kind) => {
            let p = children.into_iter().next();
            match kind {
                UnaryArm::ENot => ExprInstance::ENotBody(ENot { p }),
                UnaryArm::ENeg => ExprInstance::ENegBody(ENeg { p }),
            }
        }
        ExprArm::Binary(kind) => {
            let (p1, p2) = take2(&mut children);
            match kind {
                BinaryArm::EMult => ExprInstance::EMultBody(EMult { p1, p2 }),
                BinaryArm::EDiv => ExprInstance::EDivBody(EDiv { p1, p2 }),
                BinaryArm::EMod => ExprInstance::EModBody(EMod { p1, p2 }),
                BinaryArm::EPercentPercent => {
                    ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 })
                }
                BinaryArm::EPlus => ExprInstance::EPlusBody(EPlus { p1, p2 }),
                // ⚠ `EMinus` rebuilds as `EMinus`. The dead
                // `SubstituteTrait<Expr>::substitute` rebuilt it as `EPlus`;
                // `expr_instance_round_trip_is_the_identity` is what keeps that
                // from coming back.
                BinaryArm::EMinus => ExprInstance::EMinusBody(EMinus { p1, p2 }),
                BinaryArm::EPlusPlus => ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }),
                BinaryArm::EMinusMinus => ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }),
                BinaryArm::ELt => ExprInstance::ELtBody(ELt { p1, p2 }),
                BinaryArm::ELte => ExprInstance::ELteBody(ELte { p1, p2 }),
                BinaryArm::EGt => ExprInstance::EGtBody(EGt { p1, p2 }),
                BinaryArm::EGte => ExprInstance::EGteBody(EGte { p1, p2 }),
                BinaryArm::EEq => ExprInstance::EEqBody(EEq { p1, p2 }),
                BinaryArm::ENeq => ExprInstance::ENeqBody(ENeq { p1, p2 }),
                BinaryArm::EAnd => ExprInstance::EAndBody(EAnd { p1, p2 }),
                BinaryArm::EOr => ExprInstance::EOrBody(EOr { p1, p2 }),
                BinaryArm::EMatches => ExprInstance::EMatchesBody(EMatches {
                    target: p1,
                    pattern: p2,
                }),
            }
        }
        ExprArm::EList {
            locally_free,
            connective_used,
            remainder,
        } => ExprInstance::EListBody(EList {
            ps: children,
            locally_free: set_bits_until(locally_free, shift),
            connective_used,
            remainder,
        }),
        ExprArm::ETuple {
            locally_free,
            connective_used,
        } => ExprInstance::ETupleBody(ETuple {
            ps: children,
            locally_free: set_bits_until(locally_free, shift),
            connective_used,
        }),
        ExprArm::ESet {
            connective_used,
            locally_free,
            remainder,
        } => ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet {
            ps: SortedParHashSet::create_from_vec(children),
            connective_used,
            locally_free: set_bits_until(locally_free, shift),
            remainder,
        })),
        ExprArm::EMap {
            connective_used,
            locally_free,
            remainder,
        } => {
            debug_assert_eq!(
                children.len() % 2,
                0,
                "rebuild_expr_instance: EMap children must be (key, value) pairs"
            );
            let mut pairs = Vec::with_capacity(children.len() / 2);
            let mut it = children.into_iter();
            while let (Some(k), Some(v)) = (it.next(), it.next()) {
                pairs.push((k, v));
            }
            ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(ParMap {
                ps: SortedParMap::create_from_vec(pairs),
                connective_used,
                locally_free: set_bits_until(locally_free, shift),
                remainder,
            }))
        }
        ExprArm::EMethod {
            method_name,
            locally_free,
            connective_used,
        } => {
            let mut it = children.into_iter();
            let target = it.next();
            ExprInstance::EMethodBody(EMethod {
                method_name,
                target,
                arguments: it.collect(),
                locally_free: set_bits_until(locally_free, shift),
                connective_used,
            })
        }
        // ⚠ verbatim, `locally_free` untouched — matching the recursive form's
        // `other => Ok(Expr { expr_instance: Some(other) })`.
        ExprArm::NoDescent(instance) => instance,
    }
}

// ===========================================================================
// Connective (the `sub_conn` fold's per-element arms)
// ===========================================================================

/// A `ConnectiveInstance` with its child `Par`s removed. `VarRefBody` is absent
/// because `sub_conn` resolves it before ever descending (it may substitute the
/// whole connective away).
pub(crate) enum ConnArm {
    ConnAnd,
    ConnOr,
    ConnNot,
    /// `ConnBool` / `ConnInt` / `ConnString` / `ConnUri` / `ConnByteArray` —
    /// rebuilt verbatim, no children.
    Ground(ConnectiveInstance),
}

/// Rebuild a connective from its substituted children.
pub(crate) fn rebuild_connective(arm: ConnArm, children: Vec<Par>) -> Connective {
    let instance = match arm {
        ConnArm::ConnAnd => ConnectiveInstance::ConnAndBody(ConnectiveBody { ps: children }),
        ConnArm::ConnOr => ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: children }),
        ConnArm::ConnNot => ConnectiveInstance::ConnNotBody(
            children
                .into_iter()
                .next()
                .expect("rebuild_connective: ConnNot must have exactly one child"),
        ),
        ConnArm::Ground(instance) => instance,
    };
    Connective {
        connective_instance: Some(instance),
    }
}

// ===========================================================================
// process forms
// ===========================================================================

/// `Send`'s post-order body: `chan` first, then the `data` list.
pub(crate) fn rebuild_send(
    chan: Par,
    data: Vec<Par>,
    persistent: bool,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
) -> Send {
    Send {
        chan: Some(chan),
        data,
        persistent,
        locally_free: set_bits_until(locally_free, shift),
        connective_used,
    }
}

/// `ReceiveBind`'s post-order body: the source channel first, then the
/// patterns (which the recursive form substitutes at `depth + 1`).
pub(crate) fn rebuild_receive_bind(
    source: Par,
    patterns: Vec<Par>,
    remainder: Option<Var>,
    free_count: i32,
) -> ReceiveBind {
    ReceiveBind {
        patterns,
        source: Some(source),
        remainder,
        free_count,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_receive(
    binds: Vec<ReceiveBind>,
    body: Par,
    condition: Option<Par>,
    persistent: bool,
    peek: bool,
    bind_count: i32,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
) -> Receive {
    Receive {
        binds,
        body: Some(body),
        persistent,
        peek,
        bind_count,
        locally_free: set_bits_until(locally_free, shift),
        connective_used,
        condition,
    }
}

/// `New`'s post-order body.
///
/// ⚠ `injections` are passed through **unsubstituted**, exactly as the
/// recursive form does (`injections: term.injections`). They are URI-keyed
/// system channels resolved at normalization; substituting them would change
/// the term.
pub(crate) fn rebuild_new(
    body: Par,
    bind_count: i32,
    uri: Vec<String>,
    injections: std::collections::BTreeMap<String, Par>,
    locally_free: Vec<u8>,
    shift: i32,
) -> New {
    New {
        bind_count,
        p: Some(body),
        uri,
        injections,
        locally_free: set_bits_until(locally_free, shift),
    }
}

/// `MatchCase`'s post-order body. Child order is `source`, `pattern`, `guard` —
/// the order the recursive form substitutes them in, which matters because a
/// `?` on the first discards the rest.
pub(crate) fn rebuild_match_case(
    source: Par,
    pattern: Par,
    guard: Option<Par>,
    free_count: i32,
) -> MatchCase {
    MatchCase {
        pattern: Some(pattern),
        source: Some(source),
        free_count,
        guard,
    }
}

pub(crate) fn rebuild_match(
    target: Par,
    cases: Vec<MatchCase>,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
) -> Match {
    Match {
        target: Some(target),
        cases,
        locally_free: set_bits_until(locally_free, shift),
        connective_used,
    }
}

pub(crate) fn rebuild_if(
    condition: Par,
    if_true: Par,
    if_false: Par,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
) -> If {
    If {
        condition: Some(condition),
        if_true: Some(if_true),
        if_false: Some(if_false),
        locally_free: set_bits_until(locally_free, shift),
        connective_used,
    }
}

/// `Bundle`'s post-order body — the inner-bundle merge.
///
/// `shell` is the bundle with its `body` already taken out (Leg-1's
/// `Option::take`, which leaves `write_flag` / `read_flag` intact).
pub(crate) fn rebuild_bundle(mut shell: Bundle, sub_bundle: Par) -> Bundle {
    match single_bundle(&sub_bundle) {
        Some(b) => BundleOps::merge(&shell, &b),
        None => {
            shell.body = Some(sub_bundle);
            shell
        }
    }
}

/// `Par`'s post-order body: the two folded `Par`s produced by `sub_exp` and
/// `sub_conn`, concatenated onto the shell carrying every other slot.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_par(
    exprs_par: Par,
    connectives_par: Par,
    sends: Vec<Send>,
    receives: Vec<Receive>,
    news: Vec<New>,
    matches: Vec<Match>,
    bundles: Vec<Bundle>,
    conditionals: Vec<If>,
    unforgeables: Vec<models::rhoapi::GUnforgeable>,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
) -> Par {
    concatenate_pars(
        exprs_par,
        concatenate_pars(connectives_par, Par {
            sends,
            receives,
            news,
            exprs: Vec::new(),
            matches,
            unforgeables,
            bundles,
            connectives: Vec::new(),
            conditionals,
            locally_free: set_bits_until(locally_free, shift),
            connective_used,
        }),
    )
}

// ---------------------------------------------------------------------------
// the two `sub_exp` / `sub_conn` fold steps
// ---------------------------------------------------------------------------

/// One step of `sub_exp`'s left fold, for an element that produced a
/// substituted `Expr`.
pub(crate) fn fold_prepend_expr(acc: Par, e: Expr, depth: i32) -> Par {
    prepend_expr(acc, e, depth)
}

/// One step of `sub_exp`'s left fold, for an `EVar` that the environment
/// replaced with a whole `Par`. ⚠ Argument order is `(new_par, acc)` — the
/// substituted par goes FIRST, as in the recursive form.
pub(crate) fn fold_concatenate_par(acc: Par, substituted: Par) -> Par {
    concatenate_pars(substituted, acc)
}

/// One step of `sub_conn`'s left fold, for an element that produced a
/// substituted `Connective`.
pub(crate) fn fold_prepend_connective(acc: Par, c: Connective, depth: i32) -> Par {
    prepend_connective(acc, c, depth)
}

/// `unwrap_option_safe`'s error, raised without a value to unwrap. Used where
/// the driver has already established that a required field was `None`.
pub(crate) fn missing_required_field<A: Clone + std::fmt::Debug>() -> InterpreterError {
    super::unwrap_option_safe::<A>(None).expect_err("unwrap_option_safe(None) always returns Err")
}

#[cfg(test)]
mod tests {
    use models::rhoapi::KeyValuePair;

    use super::*;
    use crate::rust::interpreter::test_utils::substitution_corpus::{
        every_expr_instance, EXPR_INSTANCE_VARIANT_COUNT,
    };

    /// ⚠ **The guard that a shared table needs.**
    ///
    /// Because the driver and the oracle both go through `split` / `rebuild`, a
    /// bug inside them is invisible to a differential *between* them. Splitting
    /// an arm and rebuilding it with the same children and `shift = 0` must
    /// therefore be the identity.
    ///
    /// `shift = 0` matters: `set_bits_until(bits, 0)` returns an empty vector,
    /// so the comparison is made against a term whose `locally_free` has been
    /// cleared the same way. That is the only field `rebuild` may alter.
    #[test]
    fn expr_instance_round_trip_is_the_identity() {
        let corpus = every_expr_instance();
        assert_eq!(corpus.len(), EXPR_INSTANCE_VARIANT_COUNT);

        for (name, instance) in corpus {
            let expected = clear_locally_free(instance.clone());
            let (arm, children) = split_expr_instance(instance);
            let children: Vec<Par> = children
                .into_iter()
                .map(|c| c.expect("the corpus carries no absent operands"))
                .collect();
            let rebuilt = rebuild_expr_instance(arm, children, 0);
            assert_eq!(
                Expr {
                    expr_instance: Some(rebuilt)
                },
                Expr {
                    expr_instance: Some(expected)
                },
                "split/rebuild is not the identity for the `{}` arm — this is the \
                 `EMinus -> EPlus` failure class",
                name
            );
        }
    }

    /// `rebuild` clears `locally_free` at `shift = 0`; normalise the expected
    /// value the same way so the round trip compares like with like.
    fn clear_locally_free(instance: ExprInstance) -> ExprInstance {
        match instance {
            ExprInstance::EListBody(mut x) => {
                x.locally_free = Vec::new();
                ExprInstance::EListBody(x)
            }
            ExprInstance::ETupleBody(mut x) => {
                x.locally_free = Vec::new();
                ExprInstance::ETupleBody(x)
            }
            ExprInstance::ESetBody(mut x) => {
                x.locally_free = Vec::new();
                // The recursive form re-sorts on rebuild, so normalise the
                // expected value through the same mapper pair.
                let ps = std::mem::take(&mut x.ps);
                ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet {
                    ps: SortedParHashSet::create_from_vec(ps),
                    connective_used: x.connective_used,
                    locally_free: Vec::new(),
                    remainder: x.remainder,
                }))
            }
            ExprInstance::EMapBody(mut x) => {
                x.locally_free = Vec::new();
                let kvs: Vec<(Par, Par)> = std::mem::take(&mut x.kvs)
                    .into_iter()
                    .map(|KeyValuePair { key, value }| {
                        (key.unwrap_or_default(), value.unwrap_or_default())
                    })
                    .collect();
                ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(ParMap {
                    ps: SortedParMap::create_from_vec(kvs),
                    connective_used: x.connective_used,
                    locally_free: Vec::new(),
                    remainder: x.remainder,
                }))
            }
            ExprInstance::EMethodBody(mut x) => {
                x.locally_free = Vec::new();
                ExprInstance::EMethodBody(x)
            }
            other => other,
        }
    }

    /// A fixed-arity arm must report exactly its arity of operand SLOTS, with
    /// absent operands preserved as `None` in position. That is what lets the
    /// driver raise `unwrap_option_safe`'s error where the recursive form did.
    #[test]
    fn fixed_arity_arms_report_their_slots_in_position() {
        use models::rhoapi::EPlus;
        let (arm, children) = split_expr_instance(ExprInstance::EPlusBody(EPlus {
            p1: None,
            p2: Some(Par::default()),
        }));
        assert!(matches!(arm, ExprArm::Binary(BinaryArm::EPlus)));
        assert_eq!(children.len(), 2, "a binary arm must report two slots");
        assert!(
            children[0].is_none(),
            "an absent p1 must stay in position 0"
        );
        assert!(children[1].is_some());
    }
}
