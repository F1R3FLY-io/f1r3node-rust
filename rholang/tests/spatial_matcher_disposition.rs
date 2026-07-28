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
use models::rust::rholang::wire_schema::{
    EXPR_INSTANCE_VARIANTS, UNF_INSTANCE_VARIANTS, UNF_INSTANCE_VARIANT_COUNT,
};
use models::rust::utils::{new_freevar_par, new_gint_par};
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
