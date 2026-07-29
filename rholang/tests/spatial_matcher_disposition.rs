//! # The per-variant disposition of `SpatialMatcher`, made enumerable
//!
//! `SpatialMatcher<Expr, Expr>::spatial_match`
//! (`rholang/src/rust/interpreter/matcher/spatial_matcher.rs`) is a `match` over
//! **pairs** of `ExprInstance`, terminated by a `_ => None` catch-all. A
//! catch-all in that position is not a default — it is a *decision*, taken
//! silently, for every variant nobody wrote an arm for: the pattern matches
//! nothing, the `COMM` never fires, the receive rests forever, the node exits
//! `0`, and no diagnostic is produced anywhere.
//!
//! `spatial_match` decides `COMM` firing, so that decision is **consensus
//! visible**. This file makes it *enumerable* instead of silent.
//!
//! ## The probe
//!
//! For every `ExprInstance` variant that carries at least one child `Par`, build
//!
//! * a **pattern** whose child slot 0 is a free variable at level `0`, and
//! * a **target** whose child slot 0 is the ground term `42`,
//!
//! with every remaining slot filled identically on both sides, and run the
//! production entry `SpatialMatcher<Par, Par>::spatial_match` over the pair. The
//! variant either binds `0 ↦ 42` — it descends — or it does not, in which case
//! it must be **declared** as not descending by
//! [`models::rust::rholang::par_children::spatial_match_descends_into`].
//!
//! There is no third outcome. A variant added to `RhoTypes.proto` cannot be
//! quietly omitted from consideration, because the iteration set is the
//! GENERATED variant table (`EXPR_INSTANCE_VARIANT_COUNT` is
//! `EXPR_INSTANCE_VARIANTS.len()`, not a literal), and
//! [`variant_probes_cover_every_generated_expr_instance_variant`] fails until
//! the new variant has a representative here.
//!
//! ## Why an audit that greps `ExprInstance::` misses this file's subject
//!
//! `spatial_matcher.rs` and `has_locally_free.rs` import every variant
//! **unqualified** through `use super::exports::*`
//! (`rholang/src/rust/interpreter/matcher/exports.rs`), so they contain no
//! occurrence of the string `ExprInstance::` at all. A sweep that greps the
//! qualified path reports both files as clean. This test does not grep; it
//! runs the matcher.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus, EMinusMinus,
    EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar,
    EZipper, Expr, GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken,
    GUnforgeable, KeyValuePair, Par, Var,
};
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::par_children::{
    expr_instance_child_pars, expr_instance_variant_index, spatial_match_descends_into,
    EXPR_INSTANCE_VARIANT_COUNT,
};
use models::rust::rholang::wire::{Descent, FieldKind, WireNode, WireOneof};
use models::rust::rholang::wire_schema::{
    EXPR_INSTANCE_VARIANTS, UNF_INSTANCE_VARIANTS, UNF_INSTANCE_VARIANT_COUNT,
};
use models::rust::utils::{new_freevar_par, new_gint_par};
use prost::Message;
use rholang::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::util::prepend_expr;

// ===========================================================================
// The probe corpus
// ===========================================================================

/// The ground term the target carries in child slot 0; the value the pattern's
/// free variable must be bound to when the variant descends.
const BOUND: i64 = 42;

/// The ground term every slot OTHER than slot 0 carries, identically on both
/// sides, so that a failure can only be attributable to slot 0.
const FILLER: i64 = 7;

/// The pattern's slot 0: a free variable at level `0`.
fn hole() -> Par { new_freevar_par(0, Vec::new()) }

/// The target's slot 0.
fn bound() -> Par { new_gint_par(BOUND, Vec::new(), false) }

/// Every other slot, on both sides.
fn filler() -> Par { new_gint_par(FILLER, Vec::new(), false) }

fn pathmap(ps: Vec<Par>, connective_used: bool) -> EPathMap {
    // `EPathMap` carries a private intern cell, so it is built through its
    // constructor rather than a struct literal.
    EPathMap::new(ps, Vec::new(), connective_used, None)
}

/// One variant of `ExprInstance`, in both its pattern and its target form.
struct Probe {
    /// The variant with child slot 0 replaced by [`hole`].
    pattern: ExprInstance,
    /// The same variant with child slot 0 replaced by [`bound`].
    target: ExprInstance,
}

impl Probe {
    /// A variant with no child `Par` at all — a ground, or `EVarBody`, whose
    /// payload is a `Var` rather than a `Par`. There is nothing to descend
    /// into, so pattern and target are the same term and the probe is skipped
    /// by [`child_bearing_probes`]. It is present so that the corpus still
    /// covers every generated variant.
    fn childless(instance: ExprInstance) -> Self {
        Probe {
            pattern: instance.clone(),
            target: instance,
        }
    }

    fn new(pattern: ExprInstance, target: ExprInstance) -> Self { Probe { pattern, target } }

    /// The GENERATED name of this probe's variant. Looked up by serde index in
    /// the wire-schema table rather than written out, so a renamed or
    /// re-ordered variant cannot leave a stale label in a failure message.
    fn name(&self) -> &'static str {
        let index = expr_instance_variant_index(&self.pattern);
        EXPR_INSTANCE_VARIANTS
            .iter()
            .find(|variant| variant.serde_index == index)
            .expect("every ExprInstance variant index is in the generated table")
            .name
    }

    fn has_child_slot(&self) -> bool {
        let mut children: Vec<&Par> = Vec::new();
        expr_instance_child_pars(&self.pattern, &mut children);
        !children.is_empty()
    }
}

/// A representative of **every** `ExprInstance` variant.
///
/// Ordered as the generated table orders them (serde declaration order), purely
/// for readability — [`variant_probes_cover_every_generated_expr_instance_variant`]
/// checks the set, not the order.
fn probes() -> Vec<Probe> {
    vec![
        // ---- grounds: no child `Par` ----
        Probe::childless(ExprInstance::GBool(true)),
        Probe::childless(ExprInstance::GInt(1)),
        Probe::childless(ExprInstance::GString("s".into())),
        Probe::childless(ExprInstance::GUri("rho:id:x".into())),
        Probe::childless(ExprInstance::GByteArray(vec![7])),
        Probe::childless(ExprInstance::GDouble(1.0f64.to_bits())),
        Probe::childless(ExprInstance::GBigInt(vec![1])),
        Probe::childless(ExprInstance::GBigRat(GBigRational {
            numerator: vec![1],
            denominator: vec![2],
        })),
        Probe::childless(ExprInstance::GFixedPoint(GFixedPoint {
            unscaled: vec![1],
            scale: 2,
        })),
        // ---- a variable is a `Var`, not a `Par` ----
        Probe::childless(ExprInstance::EVarBody(EVar {
            v: Some(Var::default()),
        })),
        // ---- unary ----
        Probe::new(
            ExprInstance::ENotBody(ENot { p: Some(hole()) }),
            ExprInstance::ENotBody(ENot { p: Some(bound()) }),
        ),
        Probe::new(
            ExprInstance::ENegBody(ENeg { p: Some(hole()) }),
            ExprInstance::ENegBody(ENeg { p: Some(bound()) }),
        ),
        // ---- binary arithmetic ----
        Probe::new(
            ExprInstance::EMultBody(EMult {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EMultBody(EMult {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EDivBody(EDiv {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EDivBody(EDiv {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EModBody(EMod {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EModBody(EMod {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EPlusBody(EPlus {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EPlusBody(EPlus {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EMinusBody(EMinus {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EMinusBody(EMinus {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EPlusPlusBody(EPlusPlus {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EPlusPlusBody(EPlusPlus {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EMinusMinusBody(EMinusMinus {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EMinusMinusBody(EMinusMinus {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        // ---- binary comparison ----
        Probe::new(
            ExprInstance::ELtBody(ELt {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::ELtBody(ELt {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::ELteBody(ELte {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::ELteBody(ELte {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EGtBody(EGt {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EGtBody(EGt {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EGteBody(EGte {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EGteBody(EGte {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EEqBody(EEq {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EEqBody(EEq {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::ENeqBody(ENeq {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::ENeqBody(ENeq {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        // ---- binary boolean ----
        Probe::new(
            ExprInstance::EAndBody(EAnd {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EAndBody(EAnd {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        Probe::new(
            ExprInstance::EOrBody(EOr {
                p1: Some(hole()),
                p2: Some(filler()),
            }),
            ExprInstance::EOrBody(EOr {
                p1: Some(bound()),
                p2: Some(filler()),
            }),
        ),
        // ---- `matches`: slot 0 is the TARGET; the `pattern` slot is a nested
        //      pattern at a different binding depth and is identical on both
        //      sides here, so this probe measures the target slot alone. ----
        Probe::new(
            ExprInstance::EMatchesBody(EMatches {
                target: Some(hole()),
                pattern: Some(filler()),
            }),
            ExprInstance::EMatchesBody(EMatches {
                target: Some(bound()),
                pattern: Some(filler()),
            }),
        ),
        // ---- collections ----
        Probe::new(
            ExprInstance::EListBody(EList {
                ps: vec![hole()],
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::EListBody(EList {
                ps: vec![bound()],
                ..Default::default()
            }),
        ),
        Probe::new(
            ExprInstance::ETupleBody(ETuple {
                ps: vec![hole()],
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::ETupleBody(ETuple {
                ps: vec![bound()],
                ..Default::default()
            }),
        ),
        Probe::new(
            ExprInstance::ESetBody(ESet {
                ps: vec![hole()],
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::ESetBody(ESet {
                ps: vec![bound()],
                ..Default::default()
            }),
        ),
        Probe::new(
            ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(hole()),
                    value: Some(filler()),
                }],
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(bound()),
                    value: Some(filler()),
                }],
                ..Default::default()
            }),
        ),
        Probe::new(
            ExprInstance::EPathmapBody(pathmap(vec![hole()], true)),
            ExprInstance::EPathmapBody(pathmap(vec![bound()], false)),
        ),
        Probe::new(
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(pathmap(vec![hole()], true)),
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(pathmap(vec![bound()], false)),
                ..Default::default()
            }),
        ),
        // ---- method: slot 0 is the receiver ----
        Probe::new(
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(hole()),
                arguments: vec![filler()],
                connective_used: true,
                ..Default::default()
            }),
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: Some(bound()),
                arguments: vec![filler()],
                ..Default::default()
            }),
        ),
    ]
}

fn child_bearing_probes() -> Vec<Probe> {
    probes().into_iter().filter(Probe::has_child_slot).collect()
}

/// Wrap an `ExprInstance` in the `Par` the matcher is actually entered with.
///
/// `prepend_expr` recomputes `locally_free` and `connective_used` exactly as the
/// normalizer does, so the probe `Par` is shaped like a normalized one rather
/// than hand-flagged.
fn par_of(instance: ExprInstance) -> Par {
    prepend_expr(
        Par::default(),
        Expr {
            expr_instance: Some(instance),
        },
        0,
    )
}

/// Run the production entry over one probe and report whether the pattern's
/// free variable came back bound to the target's ground term.
fn binds(probe: &Probe) -> bool {
    let target = par_of(probe.target.clone());
    let pattern = par_of(probe.pattern.clone());

    let mut context = SpatialMatcherContext::new();
    match context.spatial_match(target, pattern) {
        None => false,
        Some(()) => context.free_map.get(&0) == Some(&bound()),
    }
}

// ===========================================================================
// The gates
// ===========================================================================

/// The iteration set is the GENERATED table, so this test cannot be satisfied
/// vacuously: a 37th `ExprInstance` arm fails here until it has a probe, and
/// therefore until somebody has decided what the matcher does with it.
#[test]
fn variant_probes_cover_every_generated_expr_instance_variant() {
    let mut indices: Vec<u32> = probes()
        .iter()
        .map(|probe| expr_instance_variant_index(&probe.pattern))
        .collect();
    indices.sort_unstable();

    assert_eq!(
        indices,
        (0..EXPR_INSTANCE_VARIANT_COUNT as u32).collect::<Vec<u32>>(),
        "the spatial-matcher probe corpus no longer covers every ExprInstance variant \
         the wire-schema table generates. `EXPR_INSTANCE_VARIANT_COUNT` is \
         `EXPR_INSTANCE_VARIANTS.len()`, so this fires the moment a variant is added to \
         `RhoTypes.proto` — and it stays red until somebody decides whether \
         `spatial_match` descends into it."
    );

    for probe in probes() {
        assert_eq!(
            expr_instance_variant_index(&probe.pattern),
            expr_instance_variant_index(&probe.target),
            "the pattern and target halves of the {} probe are different variants",
            probe.name()
        );
    }
}

/// ★ THE GUARD.
///
/// Every child-bearing variant either descends — a free variable in child slot
/// 0 comes back bound — or is **declared** not to. There is no silent third
/// case, which is what `_ => None` used to provide.
#[test]
fn every_child_bearing_variant_descends_or_is_declared_not_to() {
    let mut undeclared_inert: Vec<&'static str> = Vec::new();
    let mut declared_but_descends: Vec<&'static str> = Vec::new();

    for probe in child_bearing_probes() {
        let descends = binds(&probe);
        let declared = spatial_match_descends_into(&probe.pattern);

        match (descends, declared) {
            (false, true) => undeclared_inert.push(probe.name()),
            (true, false) => declared_but_descends.push(probe.name()),
            _ => {}
        }
    }

    assert!(
        undeclared_inert.is_empty(),
        "these ExprInstance variants carry child `Par`s but `spatial_match` binds NOTHING \
         through them, and they are NOT declared as non-descending by \
         `spatial_match_descends_into`. A pattern containing one of them is INERT: the \
         COMM never fires, the receive rests forever, the node exits 0 and emits no \
         diagnostic. Implement the arm, or declare the variant excluded WITH ITS REASON. \
         Undeclared-inert variants: {:?}",
        undeclared_inert
    );

    assert!(
        declared_but_descends.is_empty(),
        "these ExprInstance variants ARE descended into by `spatial_match`, but \
         `spatial_match_descends_into` declares that they are not. The declaration is \
         consensus-visible documentation of which patterns can fire a COMM; it must not \
         understate the matcher. Mis-declared variants: {:?}",
        declared_but_descends
    );
}

/// The excluded set, pinned exactly — modelled on
/// `substitute_disposition_excludes_exactly_the_two_pathmap_arms`.
///
/// Growing this set makes more programs inert; shrinking it makes programs that
/// rest today fire. Either direction is a protocol change, so neither may
/// happen as a side effect of an unrelated edit.
#[test]
fn spatial_match_disposition_excludes_exactly_the_declared_arms() {
    let excluded: Vec<&'static str> = child_bearing_probes()
        .iter()
        .filter(|probe| !spatial_match_descends_into(&probe.pattern))
        .map(Probe::name)
        .collect();

    assert_eq!(
        excluded,
        vec!["EPathmapBody", "EZipperBody"],
        "the set of child-bearing ExprInstance variants that `spatial_match` does NOT \
         descend into has changed. That set decides which patterns can fire a COMM, so \
         it is consensus-visible. See `spatial_match_descends_into`."
    );
}

/// The flagship regression, stated as the surface program it corresponds to.
///
/// `EMult`, `EDiv`, `EMod`, `EPlus`, `EPlusPlus`, `EMinusMinus` and
/// `EPercentPercent` all descended before this suite existed; `EMinus` alone did
/// not. No design produces that asymmetry — it is a dropped line, and this is
/// the test that says so in one assertion.
#[test]
fn subtraction_in_a_pattern_binds_exactly_as_addition_does() {
    let subtraction = Probe::new(
        ExprInstance::EMinusBody(EMinus {
            p1: Some(hole()),
            p2: Some(filler()),
        }),
        ExprInstance::EMinusBody(EMinus {
            p1: Some(bound()),
            p2: Some(filler()),
        }),
    );
    let addition = Probe::new(
        ExprInstance::EPlusBody(EPlus {
            p1: Some(hole()),
            p2: Some(filler()),
        }),
        ExprInstance::EPlusBody(EPlus {
            p1: Some(bound()),
            p2: Some(filler()),
        }),
    );

    assert!(
        binds(&addition),
        "the control failed: `a + b` no longer binds in a pattern, so this test can say \
         nothing about `a - b`"
    );
    assert!(
        binds(&subtraction),
        "`a - b` in a pattern matches NOTHING while `a + b`, `a * b`, `a / b` and `a % b` \
         all match. A receive whose pattern contains a subtraction rests forever, with no \
         diagnostic."
    );
}

/// A variant that descends must not descend where `Substitute` refuses to.
///
/// `Substitute` rewrites a term; `spatial_match` reads one and binds out of it.
/// They are different operations, but they run over the same installed pattern:
/// a `for` bind is substituted before it is stored, and matched after. If the
/// matcher descended into a sub-term substitution had left untouched, it would
/// bind out of **stale bytes** — a `VarRef` that was never resolved, a
/// `BoundVar` that was never shifted. So the matcher's frontier must be a
/// subset of substitution's.
#[test]
fn spatial_match_never_descends_where_substitute_does_not() {
    use models::rust::rholang::par_children::substitute_descends_into;

    let outrunning: Vec<&'static str> = child_bearing_probes()
        .iter()
        .filter(|probe| {
            spatial_match_descends_into(&probe.pattern) && !substitute_descends_into(&probe.pattern)
        })
        .map(Probe::name)
        .collect();

    assert!(
        outrunning.is_empty(),
        "these variants are descended into by `spatial_match` but NOT by `Substitute`, so \
         the matcher would bind out of a sub-term substitution never rewrote — an \
         unresolved `VarRef` or an unshifted `BoundVar`. Fix substitution first, or stop \
         descending here. Variants: {:?}",
        outrunning
    );
}

// ===========================================================================
// The per-variant semantics the classification actually asserts
//
// "Slot 0 binds" is necessary but not sufficient: an arm that accepted
// everything would pass the guard above too. These tests pin what each newly
// implemented arm REFUSES, which is the other half of the claim.
// ===========================================================================

/// A binary arm must compare BOTH operands. An arm that matched slot 0 and
/// ignored slot 1 would satisfy the disposition guard while accepting terms it
/// must reject.
#[test]
fn every_binary_arm_refuses_a_mismatched_second_operand() {
    let differing_second_operand = new_gint_par(FILLER + 1, Vec::new(), false);
    let mut exercised: Vec<&'static str> = Vec::new();

    for probe in child_bearing_probes() {
        // Only the two-slot arms carry a second operand to corrupt.
        let mut children: Vec<&Par> = Vec::new();
        expr_instance_child_pars(&probe.target, &mut children);
        if children.len() != 2 || !spatial_match_descends_into(&probe.pattern) {
            continue;
        }
        // `EMapBody`'s two slots are a key and a value, and `ETupleBody`'s /
        // `EListBody`'s are two elements — all of them are already covered by
        // the generic path below, so no arm is skipped here; only the shapes
        // that cannot express "same variant, different second slot" are.
        let corrupted = replace_second_child(&probe.target, differing_second_operand.clone());
        let Some(corrupted) = corrupted else { continue };

        let mut context = SpatialMatcherContext::new();
        assert!(
            context
                .spatial_match(par_of(corrupted), par_of(probe.pattern.clone()))
                .is_none(),
            "{} matched a target whose SECOND child differs from the pattern's. The arm is \
             ignoring a slot.",
            probe.name()
        );
        exercised.push(probe.name());
    }

    // ★ POSITIVE CONTROL. Every assertion above is inside a loop with two
    // `continue`s, so an empty or truncated iteration would report success
    // having tested nothing. Name the variants this run actually exercised and
    // require every two-slot arm to be among them.
    let expected = vec![
        "EMultBody",
        "EDivBody",
        "EModBody",
        "EPlusBody",
        "EMinusBody",
        "EPlusPlusBody",
        "EMinusMinusBody",
        "EPercentPercentBody",
        "ELtBody",
        "ELteBody",
        "EGtBody",
        "EGteBody",
        "EEqBody",
        "ENeqBody",
        "EAndBody",
        "EOrBody",
        "EMatchesBody",
        "EMapBody",
        "EMethodBody",
    ];
    let mut exercised_sorted = exercised.clone();
    exercised_sorted.sort_unstable();
    let mut expected_sorted = expected.clone();
    expected_sorted.sort_unstable();
    assert_eq!(
        exercised_sorted, expected_sorted,
        "the second-operand refusal loop did not exercise the arms it is supposed to. \
         Exercised: {:?}",
        exercised
    );
}

/// Rebuild `instance` with its second child slot replaced, for the binary
/// shapes where "second child" is well defined. Returns `None` for shapes this
/// helper does not construct, so the caller skips them explicitly rather than
/// silently passing.
fn replace_second_child(instance: &ExprInstance, replacement: Par) -> Option<ExprInstance> {
    let first = Some(bound());
    let second = Some(replacement);
    Some(match instance {
        ExprInstance::EMultBody(_) => ExprInstance::EMultBody(EMult {
            p1: first,
            p2: second,
        }),
        ExprInstance::EDivBody(_) => ExprInstance::EDivBody(EDiv {
            p1: first,
            p2: second,
        }),
        ExprInstance::EModBody(_) => ExprInstance::EModBody(EMod {
            p1: first,
            p2: second,
        }),
        ExprInstance::EPlusBody(_) => ExprInstance::EPlusBody(EPlus {
            p1: first,
            p2: second,
        }),
        ExprInstance::EMinusBody(_) => ExprInstance::EMinusBody(EMinus {
            p1: first,
            p2: second,
        }),
        ExprInstance::EPlusPlusBody(_) => ExprInstance::EPlusPlusBody(EPlusPlus {
            p1: first,
            p2: second,
        }),
        ExprInstance::EMinusMinusBody(_) => ExprInstance::EMinusMinusBody(EMinusMinus {
            p1: first,
            p2: second,
        }),
        ExprInstance::EPercentPercentBody(_) => {
            ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: first,
                p2: second,
            })
        }
        ExprInstance::ELtBody(_) => ExprInstance::ELtBody(ELt {
            p1: first,
            p2: second,
        }),
        ExprInstance::ELteBody(_) => ExprInstance::ELteBody(ELte {
            p1: first,
            p2: second,
        }),
        ExprInstance::EGtBody(_) => ExprInstance::EGtBody(EGt {
            p1: first,
            p2: second,
        }),
        ExprInstance::EGteBody(_) => ExprInstance::EGteBody(EGte {
            p1: first,
            p2: second,
        }),
        ExprInstance::EEqBody(_) => ExprInstance::EEqBody(EEq {
            p1: first,
            p2: second,
        }),
        ExprInstance::ENeqBody(_) => ExprInstance::ENeqBody(ENeq {
            p1: first,
            p2: second,
        }),
        ExprInstance::EAndBody(_) => ExprInstance::EAndBody(EAnd {
            p1: first,
            p2: second,
        }),
        ExprInstance::EOrBody(_) => ExprInstance::EOrBody(EOr {
            p1: first,
            p2: second,
        }),
        ExprInstance::EMatchesBody(_) => ExprInstance::EMatchesBody(EMatches {
            target: first,
            pattern: second,
        }),
        ExprInstance::EMethodBody(_) => ExprInstance::EMethodBody(EMethod {
            method_name: "nth".to_string(),
            target: first,
            arguments: vec![second.expect("replacement is Some")],
            ..Default::default()
        }),
        ExprInstance::EListBody(_) => ExprInstance::EListBody(EList {
            ps: vec![first.expect("bound is Some"), second.expect("Some")],
            ..Default::default()
        }),
        ExprInstance::ETupleBody(_) => ExprInstance::ETupleBody(ETuple {
            ps: vec![first.expect("bound is Some"), second.expect("Some")],
            ..Default::default()
        }),
        ExprInstance::EMapBody(_) => ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: first,
                value: second,
            }],
            ..Default::default()
        }),
        _ => return None,
    })
}

/// A method call's NAME is part of its identity. `x.nth(0)` is not a candidate
/// match for `x.length(0)` no matter what the receiver binds to.
#[test]
fn a_method_pattern_refuses_a_different_method_name() {
    let pattern = ExprInstance::EMethodBody(EMethod {
        method_name: "nth".to_string(),
        target: Some(hole()),
        arguments: vec![filler()],
        connective_used: true,
        ..Default::default()
    });
    let same_name = ExprInstance::EMethodBody(EMethod {
        method_name: "nth".to_string(),
        target: Some(bound()),
        arguments: vec![filler()],
        ..Default::default()
    });
    let other_name = ExprInstance::EMethodBody(EMethod {
        method_name: "length".to_string(),
        target: Some(bound()),
        arguments: vec![filler()],
        ..Default::default()
    });

    assert!(
        binds(&Probe::new(pattern.clone(), same_name)),
        "the control failed: a method pattern no longer binds its receiver"
    );
    assert!(
        !binds(&Probe::new(pattern, other_name)),
        "a method pattern matched a call to a DIFFERENT method"
    );
}

/// A method's argument list is positional and exact-arity — `fold_match` with
/// no remainder — so a call of a different arity is a different call.
#[test]
fn a_method_pattern_refuses_a_different_arity() {
    let pattern = ExprInstance::EMethodBody(EMethod {
        method_name: "slice".to_string(),
        target: Some(hole()),
        arguments: vec![filler()],
        connective_used: true,
        ..Default::default()
    });
    let two_arguments = ExprInstance::EMethodBody(EMethod {
        method_name: "slice".to_string(),
        target: Some(bound()),
        arguments: vec![filler(), filler()],
        ..Default::default()
    });
    let no_arguments = ExprInstance::EMethodBody(EMethod {
        method_name: "slice".to_string(),
        target: Some(bound()),
        arguments: Vec::new(),
        ..Default::default()
    });

    assert!(
        !binds(&Probe::new(pattern.clone(), two_arguments)),
        "a 1-argument method pattern matched a 2-argument call"
    );
    assert!(
        !binds(&Probe::new(pattern, no_arguments)),
        "a 1-argument method pattern matched a 0-argument call"
    );
}

/// ★ The claim that `EMatches`'s right-hand side must be compared by EQUALITY
/// rather than descended into, demonstrated on the case that decides it.
///
/// `@{x matches Int}` against `@{5 matches Int}` must bind `x ↦ 5`. The two
/// right-hand sides are `ConnInt` CONNECTIVE `Par`s, and
/// `SpatialMatcher<Par, Connective>`'s `ConnInt` arm demands a `GInt`
/// EXPRESSION on the target side — so a `spatial_match` of the right-hand sides
/// against each other returns `None` and the whole pattern would go inert. This
/// is the concrete reason the arm guards that slot with `==`.
#[test]
fn a_matches_pattern_compares_its_right_hand_side_by_equality() {
    let int_connective = || {
        let mut par = Par::default();
        par.connectives = vec![models::rhoapi::Connective {
            connective_instance: Some(models::rhoapi::connective::ConnectiveInstance::ConnInt(
                true,
            )),
        }];
        par.connective_used = true;
        par
    };
    let string_connective = || {
        let mut par = Par::default();
        par.connectives = vec![models::rhoapi::Connective {
            connective_instance: Some(models::rhoapi::connective::ConnectiveInstance::ConnString(
                true,
            )),
        }];
        par.connective_used = true;
        par
    };

    let pattern = ExprInstance::EMatchesBody(EMatches {
        target: Some(hole()),
        pattern: Some(int_connective()),
    });

    assert!(
        binds(&Probe::new(
            pattern.clone(),
            ExprInstance::EMatchesBody(EMatches {
                target: Some(bound()),
                pattern: Some(int_connective()),
            }),
        )),
        "`x matches Int` failed to bind against `42 matches Int`. Descending into the \
         right-hand side instead of comparing it is exactly what breaks this case: the \
         `ConnInt` arm of `SpatialMatcher<Par, Connective>` requires a GInt EXPRESSION on \
         the target side, and here the target side is itself a ConnInt CONNECTIVE."
    );
    assert!(
        !binds(&Probe::new(
            pattern,
            ExprInstance::EMatchesBody(EMatches {
                target: Some(bound()),
                pattern: Some(string_connective()),
            }),
        )),
        "`x matches Int` matched `42 matches String`"
    );
}

/// The same pin, taken at the seam RSpace actually calls.
///
/// `Matcher::get` is the `rspace_plus_plus::rspace::r#match::Match` impl the
/// tuple space consults to decide whether a produce and a consume may COMM. A
/// pattern that this function declines is a receive that rests forever. Running
/// the flagship case here — rather than only through
/// `SpatialMatcher<Par, Par>` — is what makes "consensus visible" a measurement
/// instead of an argument.
#[test]
fn the_rspace_comm_seam_now_fires_on_a_subtraction_pattern() {
    use models::rhoapi::{BindPattern, ListParWithRandom};
    use rspace_plus_plus::rspace::r#match::Match;

    let matcher = rholang::rust::interpreter::matcher::r#match::Matcher;

    let bind = |instance: ExprInstance| BindPattern {
        patterns: vec![par_of(instance)],
        remainder: None,
        free_count: 1,
    };
    let datum = |instance: ExprInstance| ListParWithRandom {
        pars: vec![par_of(instance)],
        random_state: Vec::new(),
    };

    let subtraction_pattern = bind(ExprInstance::EMinusBody(EMinus {
        p1: Some(hole()),
        p2: Some(filler()),
    }));
    let subtraction_datum = datum(ExprInstance::EMinusBody(EMinus {
        p1: Some(bound()),
        p2: Some(filler()),
    }));

    let addition_pattern = bind(ExprInstance::EPlusBody(EPlus {
        p1: Some(hole()),
        p2: Some(filler()),
    }));
    let addition_datum = datum(ExprInstance::EPlusBody(EPlus {
        p1: Some(bound()),
        p2: Some(filler()),
    }));

    let control = matcher.get(&addition_pattern, &addition_datum);
    assert_eq!(
        control.map(|result| result.pars),
        Some(vec![bound()]),
        "the control failed: an addition pattern no longer produces a COMM at the RSpace \
         seam, so this test can say nothing about subtraction"
    );

    let subject = matcher.get(&subtraction_pattern, &subtraction_datum);
    assert_eq!(
        subject.map(|result| result.pars),
        Some(vec![bound()]),
        "RSpace still declines to COMM a subtraction pattern against a matching datum. \
         The receive rests forever, the node exits 0, and nothing is reported."
    );
}

// ===========================================================================
// `GUnforgeable` — the same question, asked of the other oneof
// ===========================================================================

/// A representative of **every** `UnfInstance` variant, with a second payload
/// per variant so that inequality can be exercised too.
fn unf_probes() -> Vec<(UnfInstance, UnfInstance)> {
    vec![
        (
            UnfInstance::GPrivateBody(GPrivate { id: vec![1] }),
            UnfInstance::GPrivateBody(GPrivate { id: vec![2] }),
        ),
        (
            UnfInstance::GDeployIdBody(GDeployId { sig: vec![1] }),
            UnfInstance::GDeployIdBody(GDeployId { sig: vec![2] }),
        ),
        (
            UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![1],
            }),
            UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![2],
            }),
        ),
        (
            UnfInstance::GSysAuthTokenBody(GSysAuthToken {}),
            // `GSysAuthToken` is the empty message: it has exactly one value, so
            // the "different payload" half is necessarily the same value. The
            // inequality leg below therefore skips it by construction rather
            // than by exception — see `unforgeables_of_different_variants_do_not_match`.
            UnfInstance::GSysAuthTokenBody(GSysAuthToken {}),
        ),
    ]
}

fn unf(instance: UnfInstance) -> GUnforgeable {
    GUnforgeable {
        unf_instance: Some(instance),
    }
}

#[test]
fn unf_probes_cover_every_generated_unf_instance_variant() {
    assert_eq!(
        unf_probes().len(),
        UNF_INSTANCE_VARIANT_COUNT,
        "the GUnforgeable probe corpus no longer covers every UnfInstance variant the \
         wire-schema table generates ({:?})",
        UNF_INSTANCE_VARIANTS
            .iter()
            .map(|variant| variant.name)
            .collect::<Vec<_>>()
    );
}

/// Spatial matching of a `GUnforgeable` is EQUALITY — for every variant. An
/// unforgeable name is an opaque byte string with no sub-`Par` and no binder, so
/// there is nothing else it could be.
#[test]
fn every_unforgeable_variant_matches_itself() {
    for (a, _) in unf_probes() {
        let name = &format!("{:?}", std::mem::discriminant(&a));
        let mut context = SpatialMatcherContext::new();
        assert!(
            context.spatial_match(unf(a.clone()), unf(a)).is_some(),
            "a GUnforgeable failed to match ITSELF: {}. Two of the four variants were \
             handled and the rest fell into a `_ => None`.",
            name
        );
    }
}

#[test]
fn unforgeables_of_different_variants_do_not_match() {
    let all: Vec<UnfInstance> = unf_probes().into_iter().map(|(a, _)| a).collect();
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i == j {
                continue;
            }
            let mut context = SpatialMatcherContext::new();
            assert!(
                context
                    .spatial_match(unf(a.clone()), unf(b.clone()))
                    .is_none(),
                "two DIFFERENT GUnforgeable variants matched each other"
            );
        }
    }
    for (a, b) in unf_probes() {
        if a == b {
            // `GSysAuthToken` is the empty message — one inhabitant, so it has
            // no unequal sibling to test.
            continue;
        }
        let mut context = SpatialMatcherContext::new();
        assert!(
            context.spatial_match(unf(a), unf(b)).is_none(),
            "two GUnforgeables of the same variant but different payloads matched"
        );
    }
}

/// Why the `GUnforgeable` matcher is reached only from a direct call.
///
/// `list_match::match_function` consults `spatial_match` **only** when the
/// pattern reports `connective_used`, and `HasLocallyFree<GUnforgeable>` is a
/// constant `false`. Every unforgeable pattern therefore takes the
/// `guard(t == p)` branch, which is why the impl's header has always said the
/// code is "never reached". That is a property of `connective_used`, not of the
/// matcher — so it is pinned here, next to the arms it justifies, rather than
/// left as a comment.
#[test]
fn no_unforgeable_pattern_reports_connective_used() {
    let context = SpatialMatcherContext::new();
    for (a, _) in unf_probes() {
        assert!(
            !HasLocallyFree::<GUnforgeable>::connective_used(&context, unf(a)),
            "a GUnforgeable reported `connective_used`, which routes it into \
             `SpatialMatcher<GUnforgeable, GUnforgeable>`. That impl's arms are no longer \
             unreachable, and its disposition needs re-deciding."
        );
    }
}

// ===========================================================================
// ★★ #127/#136 — the SHAPE axis: an absent required child
// ===========================================================================
//
// Everything above asks whether the matcher DESCENDS into a variant. This
// section asks what it does when the child it descends to **is not there**.
//
// `EMinus { Par p1 = 1; }` is proto3: the field is optional ON THE WIRE, and
// `prost` returns `None` for it without error. So an absent required child is
// not something `Par::decode` refuses — it is something `Par::decode`
// **returns**, and every consumer downstream of a decode can meet one.
//
// ★ THE DISPOSITION, and it settles #127 and #136 with one rule:
//
//   An absent required child is an INTERNAL INVARIANT VIOLATION, not a
//   decidable negative.
//
// Three independent reasons, none of them a preference:
//
//   1. The codebase already named it. `unwrap_option_safe`
//      (`interpreter/mod.rs`) raises
//      `InterpreterError::UndefinedRequiredProtobufFieldError` and is used at
//      41 sites. The field is *required*; an absent one is an *error*.
//   2. No Rholang source denotes it. The grammar has no production for a
//      subtraction with one operand, so "does this pattern match it?" has no
//      negative answer — the object is not a term.
//   3. `spatial_match` RETURNS `Option<()>`, whose two inhabitants are
//      *matched* and *did not match*. Answering `None` would make a malformed
//      term indistinguishable from a well-formed non-matching one — silently
//      answering `false` to a question that cannot be decided, which is
//      exactly what #73 ("undecidable guards now refuse loudly") refused.
//
// ⇒ The panic is an ASSERTION, and the fix belongs at the boundary that admits
// foreign bytes rather than at 149 call sites. `.expect(...)` is the right
// spelling for an assertion and satisfies the standing `unwrap`→`expect` rule;
// it is the FLOOR here, and this section is what keeps it from being mistaken
// for the ceiling.
//
// ⚠ Turning these arms into a `None` return would WIDEN nothing and NARROW
// nothing for well-formed terms — but it would change what the node does with
// malformed ones, from "stop" to "silently answer no-match". That is the
// #73-forbidden direction, so it is not taken here.
//
// Reachability — which decides severity — is measured in
// `rholang/tests/absent_required_child_reachability.rs` (consensus path:
// delivered but not matched, with a positive control) and
// `rspace++/libs/rspace_rhotypes/tests/ffi_absent_required_child.rs` (FFI:
// SIGABRT on ten bytes).

/// `par_of` WITHOUT the normalizer pass.
///
/// ⚠ `prepend_expr` recomputes `locally_free`/`connective_used` and therefore
/// READS the child slots — it panics on an absent one (measured as `m0` in
/// `absent_required_child_reachability.rs`). The absent form must be built
/// without it, which is also what the wire does: both fields are proto fields,
/// decoded verbatim, and nothing recomputes them on the read path.
fn par_of_raw(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

/// Read a protobuf varint at `at`, returning `(value, byte_length)`.
fn varint_at(bytes: &[u8], at: usize) -> (usize, usize) {
    let mut value = 0usize;
    let mut shift = 0u32;
    let mut i = at;
    loop {
        let byte = bytes[i];
        value |= ((byte & 0x7f) as usize) << shift;
        i += 1;
        if byte & 0x80 == 0 {
            return (value, i - at);
        }
        shift += 7;
    }
}

/// Rewrite the varint at `at` (`width` bytes) to `value`, which must still fit
/// in `width` bytes — true here because the new value is strictly smaller.
fn put_varint(bytes: &mut Vec<u8>, at: usize, width: usize, value: usize) {
    let mut encoded = Vec::with_capacity(width);
    let mut v = value;
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            encoded.push(byte);
            break;
        }
        encoded.push(byte | 0x80);
    }
    assert!(
        encoded.len() <= width,
        "the repaired length needs {} varint bytes but only {width} are available",
        encoded.len()
    );
    while encoded.len() < width {
        // Pad with a redundant continuation so the framing width is unchanged.
        let last = encoded.len() - 1;
        encoded[last] |= 0x80;
        encoded.push(0);
    }
    bytes[at..at + width].copy_from_slice(&encoded);
}

/// Decrement every length prefix that ENCLOSES `hole` by `removed`.
///
/// Walks the message tree from the top, and at each level finds the single
/// length-delimited field whose content span contains `hole`, repairs it, and
/// descends. Stops when no enclosing field remains — i.e. when `hole` is at the
/// current level. Non-length-delimited fields are skipped by wire type, so the
/// walk does not assume the schema's field order.
fn repair_enclosing_lengths(bytes: &mut Vec<u8>, hole: usize, removed: usize) {
    let mut start = 0usize;
    let mut end = bytes.len();
    let mut repaired = 0usize;
    loop {
        let mut cursor = start;
        let mut descended = false;
        while cursor < end {
            let (tag, tag_w) = varint_at(bytes, cursor);
            let field_start = cursor;
            cursor += tag_w;
            match tag & 0x7 {
                // length-delimited
                2 => {
                    let (len, len_w) = varint_at(bytes, cursor);
                    let content = cursor + len_w;
                    if hole >= field_start && hole < content + len {
                        if hole == field_start {
                            // `hole` IS this field — nothing encloses it here.
                            return;
                        }
                        put_varint(bytes, cursor, len_w, len - removed);
                        repaired += 1;
                        start = content;
                        end = content + len - removed;
                        descended = true;
                        break;
                    }
                    cursor = content + len;
                }
                0 => {
                    let (_, w) = varint_at(bytes, cursor);
                    cursor += w;
                }
                5 => cursor += 4,
                1 => cursor += 8,
                other => panic!("★ unsupported protobuf wire type {other} while repairing"),
            }
        }
        if !descended {
            assert!(
                repaired > 0,
                "★ no enclosing length prefix was repaired — the excision left the framing \
                 inconsistent and the decode below would fail for the wrong reason"
            );
            return;
        }
    }
}

/// ★★ Remove child slot 0 from `instance` **on the wire bytes**, and hand back
/// what `Par::decode` makes of the result.
///
/// ### Why byte surgery, and not a hand-written "absent" twin per variant
///
/// A third form written out for each of the thirty-six variants would be
/// exactly the hand-maintained denominator this file exists to remove — the
/// `EXPR_INSTANCE_VARIANT_COUNT = 36` mistake in a new costume. `par_children`'s
/// by-move stripper is private, so the remaining general construction is the
/// wire itself, which is also the construction the finding is *about*.
///
/// ### Why it is unambiguous
///
/// Slot 0 carries [`bound()`]; every other slot carries [`filler()`]; `BOUND !=
/// FILLER`. So slot 0's sub-encoding occurs **exactly once** in the target's
/// protobuf, and it can be excised without naming the variant. The framing is
/// `Par.exprs` ++ `Expr.<arm>` ++ payload — two nested length prefixes, both
/// repaired by varint arithmetic rather than assumed to be one byte wide.
///
/// ### Why it is not trusted
///
/// The result is checked against the GENERATED child table: it must decode, the
/// bytes must actually have changed (a mutation that does not apply is a false
/// zero — #129's harness lost a whole cell to exactly that), and
/// `expr_instance_child_pars` must report exactly one child fewer.
fn strip_first_optional_child(instance: ExprInstance) -> Option<ExprInstance> {
    let mut children_before: Vec<&Par> = Vec::new();
    expr_instance_child_pars(&instance, &mut children_before);
    let expected = children_before.len() - 1;

    let whole = par_of_raw(instance.clone()).encode_to_vec();
    let slot0 = bound().encode_to_vec();

    let occurrences = whole
        .windows(slot0.len())
        .filter(|w| *w == slot0.as_slice())
        .count();
    if occurrences == 0 {
        // ★ The variant's children are NOT nested prost sub-messages. `EPathMap`
        // is `extern_path`'d (`models/build.rs`) and encodes its entries to
        // proto field 8 `serialized_paths` — a stream of `encode_trie_path`
        // keys — so there is no `Par` sub-message to excise and no "absent
        // required child" to construct. Its read side is bounded and
        // `Err`-returning rather than `Option`-returning, which is the #130 /
        // #135 axis, not this one.
        return None;
    }
    assert_eq!(
        occurrences, 1,
        "★ slot 0's encoding occurs {occurrences} times, not once, so the excision below \
         would remove the wrong field. BOUND and FILLER must stay distinct and BOUND must \
         not appear in any other slot."
    );
    let payload_at = whole
        .windows(slot0.len())
        .position(|w| w == slot0.as_slice())
        .expect("the occurrence just counted is findable");

    // `<tag><len>` immediately precede the payload. Both are varints; find the
    // length varint by walking back until its decoded value is the payload's.
    let mut len_at = payload_at;
    let len_width = loop {
        assert!(
            len_at > 0,
            "★ no length prefix found before slot 0's payload"
        );
        len_at -= 1;
        let (value, width) = varint_at(&whole, len_at);
        if value == slot0.len() && len_at + width == payload_at {
            break width;
        }
    };
    // The field tag is the varint ending where the length begins.
    let mut tag_at = len_at;
    let tag_width = loop {
        assert!(tag_at > 0, "★ no field tag found before slot 0's length");
        tag_at -= 1;
        let (_, width) = varint_at(&whole, tag_at);
        if tag_at + width == len_at {
            break width;
        }
    };
    let removed = tag_width + len_width + slot0.len();

    let mut cut = Vec::with_capacity(whole.len() - removed);
    cut.extend_from_slice(&whole[..tag_at]);
    cut.extend_from_slice(&whole[payload_at + slot0.len()..]);

    // ★ Repair EVERY enclosing length prefix, however deep. `EMap` nests three
    // (`Par.exprs` ++ `Expr.<arm>` ++ `EMap.kvs` ++ `KeyValuePair.key`) where the
    // scalar pairs nest two, so the chain is WALKED rather than counted — a
    // fixed count silently corrupts the deeper variants, which is how this was
    // caught.
    repair_enclosing_lengths(&mut cut, tag_at, removed);

    assert_ne!(
        cut, whole,
        "★ the strip did not change the encoding — THE MUTATION DID NOT APPLY, and any \
         green result below would be a false zero"
    );

    let decoded = Par::decode(&cut[..]).unwrap_or_else(|e| {
        panic!(
            "★ the absent-child encoding did not decode: {e}. The premise of this whole \
             section is that `prost` ACCEPTS an absent required child; if it now refuses, \
             the shape axis is closed at the decoder and these dispositions are stale."
        )
    });
    let stripped = decoded.exprs[0]
        .expr_instance
        .clone()
        .expect("the stripped Par keeps its expr_instance");
    let mut children_after: Vec<&Par> = Vec::new();
    expr_instance_child_pars(&stripped, &mut children_after);
    assert_eq!(
        children_after.len(),
        expected,
        "★ stripping slot 0 left {} children, not {expected}. The generated child table \
         says the surgery removed the wrong thing.",
        children_after.len()
    );
    Some(stripped)
}

/// How many elements this variant's payload carries in its `Seq`/`Map` fields,
/// at its own level.
///
/// ★ Driven by the generated emission table: `WireNode::wire_emit` reports a
/// `Descent::Seq { len, .. }` for every repeated field, so this is the schema's
/// own answer rather than a per-variant match written here.
///
/// It is what separates "child slot 0 is an `Option` field" from "child slot 0
/// is a sequence ELEMENT". Removing an element from `EList.ps` yields a
/// SHORTER LIST, which is a perfectly good term and must not match — removing
/// `EMinus.p1` yields something that is not a term at all. Both reduce the
/// child count by one, so the child count cannot tell them apart and this can.
fn top_level_sequence_elements(instance: &ExprInstance) -> usize {
    let mut sink: Vec<u8> = Vec::new();
    let Some(node) = instance.wire_emit(&mut sink) else {
        return 0;
    };
    let mut total = 0usize;
    let mut from = 0usize;
    loop {
        match node.wire_emit(from, &mut sink) {
            Descent::Done => return total,
            Descent::Node { resume, .. } => from = resume as usize,
            Descent::Seq { resume, len, .. } => {
                total += len;
                from = resume as usize;
            }
            Descent::Map { resume, map } => {
                total += map.len();
                from = resume as usize;
            }
        }
    }
}

/// How many `Option<Message>` slots this variant's payload declares.
///
/// ★ Read from the GENERATED wire-schema program, not from a list here:
/// `WireOneof::wire_emit` hands back the payload as a `&dyn WireNode` and
/// `WireNode::wire_program()` is the descriptor-derived field table. A 37th
/// variant is therefore counted correctly the moment the generator runs, and
/// `FieldKind::Opt` is what distinguishes a *required child* from a `Seq`
/// (an empty `EList` is a perfectly good term; an absent `EMinus.p1` is not).
fn declared_optional_child_slots(instance: &ExprInstance) -> usize {
    let mut sink: Vec<u8> = Vec::new();
    match instance.wire_emit(&mut sink) {
        Some(node) => node
            .wire_program()
            .iter()
            .filter(|kind| **kind == FieldKind::Opt)
            .count(),
        None => 0,
    }
}

/// What the matcher does when child slot 0 of the TARGET is absent.
#[derive(Debug, PartialEq, Eq)]
enum AbsentChildOutcome {
    /// The arm asserted its invariant and stopped. ★ The declared disposition
    /// for every variant that descends into an `Option<Par>` slot.
    Asserted,
    /// The pair reached no arm that reads the absent slot.
    NoMatch,
    /// It matched anyway — the absent slot was never read.
    Matched,
}

/// Replace child slot 0 of a probe's target with **nothing**, the way the wire
/// delivers it, and report what the production matcher does.
///
/// ⚠ The target is round-tripped through `prost` first, so what the matcher
/// receives is a value that CAME OFF THE WIRE rather than one built in Rust.
/// A hand-constructed `None` would prove the panic exists without proving
/// anything about what a decoder can hand over.
fn absent_child_outcome(probe: &Probe) -> (AbsentChildOutcome, String) {
    let target_instance = strip_first_optional_child(probe.target.clone())
        .expect("the caller checked that slot 0 is a nested prost sub-message");
    let bytes = par_of_raw(target_instance).encode_to_vec();
    let target = Par::decode(&bytes[..]).unwrap_or_else(|e| {
        panic!(
            "★ `Par::decode` REFUSED the absent-child encoding of {}: {e}. The premise of \
             this whole section is that prost accepts it; if that has changed, the shape \
             axis is closed at the decoder and these dispositions are stale.",
            probe.name()
        )
    });
    let pattern = par_of(probe.pattern.clone());

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut context = SpatialMatcherContext::new();
        context.spatial_match(target, pattern)
    }));
    match outcome {
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string());
            (AbsentChildOutcome::Asserted, message)
        }
        Ok(None) => (AbsentChildOutcome::NoMatch, String::new()),
        Ok(Some(())) => (AbsentChildOutcome::Matched, String::new()),
    }
}

/// ★★★ THE SHAPE GATE.
///
/// Iterates the GENERATED variant table, so a 37th `ExprInstance` arm fails
/// here until somebody has decided what happens when its child is absent.
///
/// It is not satisfiable vacuously: the assertion below names the variants it
/// exercised and requires the set to be non-empty *and* to contain every
/// variant the generated program says has an `Option<Message>` slot.
#[test]
fn every_variant_with_an_optional_child_slot_asserts_rather_than_answers_no_match() {
    let mut with_optional_slots: Vec<&'static str> = Vec::new();
    let mut asserted: Vec<(&'static str, String)> = Vec::new();
    let mut silent: Vec<(&'static str, AbsentChildOutcome)> = Vec::new();
    // Variants where child slot 0 is a sequence element rather than an
    // `Option` field. Recorded, not skipped silently — an empty set here would
    // mean the discriminator stopped working.
    let mut sequence_shortening: Vec<&'static str> = Vec::new();
    // Variants whose children are not nested prost sub-messages at all —
    // `EPathMap`'s trie-key stream. Recorded rather than skipped.
    let mut not_prost_nested: Vec<&'static str> = Vec::new();

    for probe in child_bearing_probes() {
        if declared_optional_child_slots(&probe.target) == 0 {
            // No `Option<Message>` field anywhere in the payload — there is no
            // absent required child to construct.
            continue;
        }
        let Some(stripped) = strip_first_optional_child(probe.target.clone()) else {
            not_prost_nested.push(probe.name());
            continue;
        };
        if top_level_sequence_elements(&stripped) != top_level_sequence_elements(&probe.target) {
            // ★ Slot 0 was a sequence ELEMENT, not an `Option` field. Removing
            // it yields a SHORTER COLLECTION — a well-formed term — so "does
            // not match" is the correct and decidable answer, and this variant
            // is not a member of the class.
            sequence_shortening.push(probe.name());
            continue;
        }
        with_optional_slots.push(probe.name());
        if !spatial_match_descends_into(&probe.pattern) {
            // Declared not to descend above; it cannot read the absent slot.
            continue;
        }
        match absent_child_outcome(&probe) {
            (AbsentChildOutcome::Asserted, message) => asserted.push((probe.name(), message)),
            (other, _) => silent.push((probe.name(), other)),
        }
    }

    assert!(
        !with_optional_slots.is_empty(),
        "★ no variant in the generated table declares an `Option<Message>` child slot. \
         `EMinus`, `ENot` and fourteen others do, so this is a table or a probe-set \
         failure — and it would make every assertion below vacuous."
    );

    assert!(
        silent.is_empty(),
        "★★ variant(s) whose absent child produced a SILENT answer instead of an \
         assertion:\n  {:?}\n\n\
         A `None` return here means the matcher answered \"does not match\" to a value \
         that is not a term. That is the #73-forbidden direction — a guard that cannot \
         decide must refuse loudly, not answer `false`. If this is now deliberate, it \
         is a change to what the node does with malformed input and belongs in the \
         registry's disposition, not in a silent arm.",
        silent
    );

    // ★ Name what was exercised, so the gate reports an enumeration rather than
    // a tally and a probe that quietly stopped running is visible.
    assert!(
        !sequence_shortening.is_empty(),
        "★ no variant was classified as sequence-shortening. `EList`, `ESet` and `ETuple` \
         all carry their child slot 0 inside a repeated field, so an empty set means \
         `top_level_sequence_elements` stopped discriminating and collection variants are \
         now being held to the wrong disposition."
    );
    println!(
        "  {} variants encode their children OUTSIDE the prost sub-message tree (trie-key \
         streams; the #130/#135 axis, not this one): {not_prost_nested:?}",
        not_prost_nested.len()
    );
    println!(
        "  {} variants carry child slot 0 in a repeated field (shortening is a TERM, so \
         no-match is correct): {sequence_shortening:?}",
        sequence_shortening.len()
    );
    println!(
        "  {} variants carry child slot 0 in an Option<Message> field; {} of them descend \
         and ASSERT on an absent one:",
        with_optional_slots.len(),
        asserted.len()
    );
    for (name, message) in &asserted {
        println!("    ★ {name:24} -> {message}");
    }
    assert!(
        asserted.len() >= 16,
        "★ only {} variants were shown to assert. The pre-existing binary arms alone are \
         seven, #118 added nine more; a number below that means probes stopped reaching \
         the arms and this gate has gone quiet.",
        asserted.len()
    );
}
