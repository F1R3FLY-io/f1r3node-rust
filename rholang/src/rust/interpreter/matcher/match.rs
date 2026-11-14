use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance::{FreeVar, Wildcard};
use models::rhoapi::{Expr, If};
use models::rust::par_bag::EncodedParComponent;
use models::rust::rholang::implicits::vector_par;
use models::rust::rholang::match_path::{
    append_match_path_event, encode_par_match_atom, is_single_expr_match_carrier,
    RHO_MATCH_PATH_VERSION,
};
use models::rust::rholang::match_region_path::{
    append_region_event, classify_admitted_expr, encode_admitted_expr_match_path,
    AdmittedExprRegion, AdmittedExprRegionClass, AdmittedParRegionClass,
    CanonicalRhoPathRegionCursor, CanonicalRhoPathRegionEvent,
};
use models::rust::rholang::match_schema::{RhoMatchProgram, RhoPathCursor, RhoPathEvent};
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::{new_elist_expr, to_vec};
use pathmap::PathMap;
use rho_pure_eval::{Env as PureEnv, EvalError, SpatialMatch};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rho_path::{
    RhoMatchQuery, RhoMatchQueryPart, RhoPathCaptureId, RhoPathEncode, RhoPathQuery,
    RhoPathQueryAtom, StructuralSequenceEncoder,
};

use super::exports::*;
use super::fold_match::FoldMatch;
use super::projection_rules::{declines, projection_rule, ProjectionRule};
use super::spatial_matcher::SpatialMatcherContext;

#[derive(Clone, Default)]
pub struct Matcher;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/package.scala - matchListPar
//
// `get` borrows the pattern and the
// candidate datum — the space no longer clones either per match attempt.
// The interior pair walk (`fold_match` over borrowed slices) compares
// non-binding pairs by reference and clones only binding/connective pairs
// into the owned spatial lattice; the returned `ListParWithRandom` owns
// exactly the BOUND subterms (free_map values) plus a copy of the datum's
// `random_state`.
impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for Matcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let mut spatial_matcher = SpatialMatcherContext::new();

        let fold_match_result =
            spatial_matcher.fold_match(&data.pars, &pattern.patterns, pattern.remainder.clone());
        let match_result = match fold_match_result {
            Some(pars) => Some((spatial_matcher.free_map, pars)),
            None => None,
        };

        match match_result {
            Some((mut free_map, caught_rem)) => {
                let remainder_map = match pattern.remainder {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => {
                        free_map.insert(
                            level,
                            vector_par(Vec::new(), false).with_exprs(vec![new_elist_expr(
                                caught_rem,
                                Vec::new(),
                                false,
                                None,
                            )]),
                        );
                        free_map
                    }
                    _ => free_map,
                };

                let bound_pars = to_vec(remainder_map, pattern.free_count);

                Some(ListParWithRandom {
                    pars: bound_pars,
                    random_state: data.random_state.clone(),
                    cost_region: data.cost_region.clone(),
                })
            }
            None => None,
        }
    }

    fn retained_pattern_path(&self, pattern: &BindPattern) -> Option<Vec<u8>> {
        if pattern.remainder.is_some()
            || pattern.free_count != 0
            || pattern.patterns.iter().any(|par| {
                par.connective_used()
                    || !par.locally_free().is_empty()
                    || !par_has_safe_exact_expr_path(par)
            })
        {
            return None;
        }
        let mut output = StructuralSequenceEncoder::with_capacity(pattern.patterns.len());
        for par in &pattern.patterns {
            output.push(&encode_par_match_atom(par));
        }
        Some(output.finish())
    }

    fn retained_datum_path(&self, datum: &ListParWithRandom) -> Option<Vec<u8>> {
        Some(datum.rho_path_structure())
    }

    fn rho_path_query(&self, pattern: &BindPattern) -> Option<RhoPathQuery> {
        let accepts_remainder = match pattern
            .remainder
            .as_ref()
            .and_then(|remainder| remainder.var_instance.as_ref())
        {
            None => false,
            Some(FreeVar(_) | Wildcard(_)) => true,
            Some(_) => return None,
        };
        let mut captures = PathMap::new();
        let atoms = pattern
            .patterns
            .iter()
            .map(|pattern| rho_path_query_atom(pattern, &mut captures))
            .collect::<Option<Vec<_>>>()?;
        Some(RhoPathQuery::structural_sequence(atoms, accepts_remainder))
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }

    /// Cross-channel `where`-clause guard. Called by the matcher
    /// coordinator after every spatial bind has produced a
    /// `ListParWithRandom`. The bound variables of every bind are
    /// concatenated in receive-bind order (matching the De Bruijn
    /// indices the parser assigned), then the guard expression is
    /// evaluated via rho-pure-eval. Returns true iff it reduces to
    /// `GBool(true)`. Anything else (false, non-bool, error) means
    /// guard-fail and the consume stays uncommitted. See plan §7.12.
    fn check_commit(&self, k: &TaggedContinuation, matched: &[&ListParWithRandom]) -> bool {
        let Some(guard) = k.guard.as_ref() else {
            return true;
        };
        if is_empty_par(guard) {
            return true;
        }
        let mut combined: Vec<Par> = Vec::new();
        for m in matched {
            combined.extend_from_slice(&m.pars);
        }
        guard_passes(guard, &combined)
    }
}

pub(super) fn rho_path_query_atom(
    pattern: &Par,
    captures: &mut PathMap<()>,
) -> Option<RhoPathQueryAtom> {
    let exact = !pattern.connective_used() && pattern.locally_free().is_empty();
    if !is_single_expr_match_carrier(pattern) {
        return rho_path_query_cursor_atom(ParCursor::from(pattern), exact, captures);
    }
    let mut answer = None;
    pattern.visit_encoded::<models::rhoapi::Expr>(|component| {
        answer = Some(match classify_admitted_expr(&component) {
            AdmittedExprRegionClass::ExactFreeVar(level) => {
                u32::try_from(level).ok().and_then(|level| {
                    let id = RhoPathCaptureId::new(level);
                    record_linear_capture(captures, &RhoMatchQueryPart::Capture(id))
                        .then_some(RhoPathQueryAtom::Capture(id))
                })
            }
            AdmittedExprRegionClass::ExactWildcard => Some(RhoPathQueryAtom::Wildcard),
            AdmittedExprRegionClass::Structural(region) => {
                rho_path_query_region_atom(component, region, exact, captures)
            }
        });
    });
    answer.flatten()
}

/// The authoritative Par equality ignores locally_free on every nested Par.
/// An Expr-only exact path is therefore safe only when its cursor contains no
/// opaque ParBag or PathMap token that can carry a metadata-sensitive key.
fn admitted_expr_has_safe_exact_path(region: AdmittedExprRegion<'_>) -> bool {
    CanonicalRhoPathRegionCursor::admitted_nested_expr(region).all(|event| {
        matches!(
            event,
            Ok(event)
                if !matches!(
                    event,
                    CanonicalRhoPathRegionEvent::ParBag(_)
                        | CanonicalRhoPathRegionEvent::PathMap(_)
                )
        )
    })
}

fn par_has_safe_exact_expr_path(pattern: &Par) -> bool {
    if !is_single_expr_match_carrier(pattern) {
        return false;
    }
    let mut safe = false;
    pattern.visit_encoded::<Expr>(|component| {
        safe = match classify_admitted_expr(&component) {
            AdmittedExprRegionClass::Structural(region) => {
                admitted_expr_has_safe_exact_path(region)
            }
            AdmittedExprRegionClass::ExactFreeVar(_) | AdmittedExprRegionClass::ExactWildcard => {
                false
            }
        };
    });
    safe
}

fn rho_path_query_region_atom(
    component: EncodedParComponent<'_, Expr>,
    region: AdmittedExprRegion<'_>,
    exact: bool,
    captures: &mut PathMap<()>,
) -> Option<RhoPathQueryAtom> {
    let mut derived_captures = captures.clone();
    if let Some(query) = compile_ordered_region_query(region, &mut derived_captures) {
        *captures = derived_captures;
        return Some(RhoPathQueryAtom::Match(query));
    }
    (exact && admitted_expr_has_safe_exact_path(region)).then(|| {
        RhoPathQueryAtom::Exact(
            encode_admitted_expr_match_path(component)
                .expect("an admitted Expr has canonical protobuf regions"),
        )
    })
}

/// Compile the admitted sole-Expr region without constructing `Expr` or any
/// nested `Par`. Nested Pars remain opaque RMP2 tokens. Whole-value holes and
/// metadata-bearing values suppress that token; a safe singleton Expr is
/// scanned only to decide ground versus dynamic, never expanded into the datum
/// grammar in this format version.
fn compile_ordered_region_query(
    root: AdmittedExprRegion<'_>,
    captures: &mut PathMap<()>,
) -> Option<RhoMatchQuery> {
    let mut parts = Vec::new();
    let mut exact = Vec::with_capacity(256);
    exact.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    let mut suppressed_nodes = 0usize;
    let mut par_start = None;
    let mut skip_metadata_scalar = false;
    let mut expect_absent_remainder = false;
    let mut saw_hole = false;

    for event in CanonicalRhoPathRegionCursor::admitted_nested_expr(root) {
        let event = event.ok()?;
        if suppressed_nodes != 0 {
            match event {
                CanonicalRhoPathRegionEvent::EnterNode(_) => suppressed_nodes += 1,
                CanonicalRhoPathRegionEvent::LeaveNode => suppressed_nodes -= 1,
                _ => {}
            }
            continue;
        }
        if expect_absent_remainder {
            expect_absent_remainder = false;
            if !matches!(event, CanonicalRhoPathRegionEvent::Absent) {
                return None;
            }
        }

        match event {
            CanonicalRhoPathRegionEvent::EnterNode(program) => {
                if matches!(
                    program.type_name,
                    "Receive" | "ESet" | "EMap" | "EPathMap" | "Connective" | "Var"
                ) {
                    return None;
                }
                if program.type_name == "Par" {
                    par_start = Some(exact.len());
                }
                append_region_event(&mut exact, event).ok()?;
            }
            CanonicalRhoPathRegionEvent::LeaveNode => {
                append_region_event(&mut exact, event).ok()?;
                par_start = None;
            }
            CanonicalRhoPathRegionEvent::EnterField(field) if field.name == "remainder" => {
                append_region_event(&mut exact, event).ok()?;
                expect_absent_remainder = true;
            }
            CanonicalRhoPathRegionEvent::EnterField(field)
                if matches!(field.name, "locally_free" | "connective_used") =>
            {
                append_region_event(&mut exact, event).ok()?;
                flush_exact_query_part(&mut parts, &mut exact);
                parts.push(RhoMatchQueryPart::Wildcard);
                skip_metadata_scalar = true;
            }
            CanonicalRhoPathRegionEvent::Scalar(_) if skip_metadata_scalar => {
                skip_metadata_scalar = false;
            }
            CanonicalRhoPathRegionEvent::ParBag(region) => {
                let replacement = match region.classify() {
                    AdmittedParRegionClass::ExactFreeVar(level) => {
                        let id = RhoPathCaptureId::new(u32::try_from(level).ok()?);
                        let hole = RhoMatchQueryPart::Capture(id);
                        record_linear_capture(captures, &hole).then_some(hole)?
                    }
                    AdmittedParRegionClass::ExactWildcard => RhoMatchQueryPart::Wildcard,
                    AdmittedParRegionClass::MetadataBearingWholeValue => {
                        RhoMatchQueryPart::Wildcard
                    }
                    AdmittedParRegionClass::SoleExpr(child) => {
                        if !admitted_expr_is_dynamic(child, captures)? {
                            append_region_event(&mut exact, event).ok()?;
                            continue;
                        }
                        RhoMatchQueryPart::Wildcard
                    }
                    AdmittedParRegionClass::Opaque => return None,
                };
                exact.truncate(par_start.take()?);
                flush_exact_query_part(&mut parts, &mut exact);
                parts.push(replacement);
                suppressed_nodes = 1;
                saw_hole = true;
            }
            _ => append_region_event(&mut exact, event).ok()?,
        }
    }
    if suppressed_nodes != 0
        || par_start.is_some()
        || skip_metadata_scalar
        || expect_absent_remainder
        || !saw_hole
    {
        return None;
    }
    flush_exact_query_part(&mut parts, &mut exact);
    Some(RhoMatchQuery::new(parts))
}

fn admitted_expr_is_dynamic(
    root: AdmittedExprRegion<'_>,
    captures: &mut PathMap<()>,
) -> Option<bool> {
    struct Scan<'a> {
        cursor: CanonicalRhoPathRegionCursor<'a>,
        expect_absent_remainder: bool,
    }

    let mut dynamic = false;
    let mut scans = vec![Scan {
        cursor: CanonicalRhoPathRegionCursor::admitted_nested_expr(root),
        expect_absent_remainder: false,
    }];
    while let Some(scan) = scans.last_mut() {
        let Some(event) = scan.cursor.next() else {
            scans.pop();
            continue;
        };
        let event = event.ok()?;
        if scan.expect_absent_remainder {
            scan.expect_absent_remainder = false;
            if !matches!(event, CanonicalRhoPathRegionEvent::Absent) {
                return None;
            }
        }
        match event {
            CanonicalRhoPathRegionEvent::EnterNode(program)
                if matches!(
                    program.type_name,
                    "Receive" | "ESet" | "EMap" | "EPathMap" | "Connective" | "Var"
                ) =>
            {
                return None;
            }
            CanonicalRhoPathRegionEvent::EnterField(field) if field.name == "remainder" => {
                scan.expect_absent_remainder = true;
            }
            CanonicalRhoPathRegionEvent::ParBag(region) => {
                // A nested Par may differ only in locally_free metadata while
                // remaining equal to the authoritative matcher. Mark its
                // enclosing opaque ParBag dynamic, but keep scanning for every
                // emitted capture before choosing a whole-value wildcard.
                dynamic = true;
                match region.classify() {
                    AdmittedParRegionClass::ExactFreeVar(level) => {
                        let id = RhoPathCaptureId::new(u32::try_from(level).ok()?);
                        if !record_linear_capture(captures, &RhoMatchQueryPart::Capture(id)) {
                            return None;
                        }
                        dynamic = true;
                    }
                    AdmittedParRegionClass::ExactWildcard
                    | AdmittedParRegionClass::MetadataBearingWholeValue => dynamic = true,
                    AdmittedParRegionClass::SoleExpr(child) => scans.push(Scan {
                        cursor: CanonicalRhoPathRegionCursor::admitted_nested_expr(child),
                        expect_absent_remainder: false,
                    }),
                    AdmittedParRegionClass::Opaque => return None,
                }
            }
            _ => {}
        }
    }
    Some(dynamic)
}

/// Compile one generated rhoapi cursor into a conservative RMP2 candidate
/// query. The caller's exact-comparison hint is insufficient for a complete
/// schema path: `Par::eq` ignores nested locally_free metadata. This routine
/// therefore indexes only successfully compiled ordered queries and declines
/// every failed scan.
///
/// The generated cursor lets the AC target Bag use one stack-safe query
/// compiler for `Par`, `Send`, `New`, `Expr`, `Match`, `ReceiveBind`, and the
/// other unordered `Par` field types. The spatial PDA remains the verdict
/// authority: this query may admit false positives, but never reject a value
/// that the PDA could match.
pub(super) fn rho_path_query_cursor_atom(
    root: ParCursor<'_>,
    _exact: bool,
    captures: &mut PathMap<()>,
) -> Option<RhoPathQueryAtom> {
    // Generated complete-schema paths can contain Par or Send metadata that
    // the authoritative matcher ignores. Only a successful conservative
    // ordered query may prune; an unsupported scan leaves the full domain.
    if let ParCursor::Par(pattern) = root {
        if let Some(hole) = rho_match_hole(pattern) {
            if !record_linear_capture(captures, &hole) {
                return None;
            }
            return Some(match hole {
                RhoMatchQueryPart::Capture(id) => RhoPathQueryAtom::Capture(id),
                RhoMatchQueryPart::Wildcard => RhoPathQueryAtom::Wildcard,
                RhoMatchQueryPart::Exact(_) => unreachable!("a pattern hole is never exact"),
            });
        }
    }
    // Query compilation may encounter a valid hole before a later structural
    // arm forces conservative fallback. Keep those speculative captures on a
    // CoW branch and publish them only with a complete query.
    let mut derived_captures = captures.clone();
    if let Some(query) = compile_ordered_nested_query(root, &mut derived_captures) {
        *captures = derived_captures;
        return Some(RhoPathQueryAtom::Match(query));
    }
    None
}

fn compile_ordered_nested_query(
    root: ParCursor<'_>,
    captures: &mut PathMap<()>,
) -> Option<RhoMatchQuery> {
    // These generated roots do not use protobuf equality once they become
    // dynamic AC rows:
    //
    // - `Receive.binds` are matched by the unordered list PDA, not in their
    //   encoded sequence order;
    // - `New` ignores `uri` and `injections` while descending through `p`;
    // - `MatchCase` ignores `free_count` and `guard` while `Match` descends
    //   through `target`, `pattern`, and `source`;
    // - `ReceiveBind` ignores `remainder` and `free_count` while comparing
    //   `patterns` and descending through `source`.
    //
    // RMP2 currently has no schema-semantic projection for those equations.
    // Encoding the ignored fields as exact query bytes would reject values
    // which the authoritative PDA accepts. Decline this query; the caller
    // falls back to spatial matching when compilation cannot prove safety.
    if declines(root.program().type_name) {
        return None;
    }

    if let ParCursor::Par(pattern) = root {
        is_single_expr_match_carrier(pattern).then_some(())?;
    }

    let mut parts = Vec::new();
    let mut exact = Vec::with_capacity(256);
    exact.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    let mut suppressed_nodes = 0usize;
    let mut ignored_field_depth = 0usize;
    let mut saw_hole = false;
    let mut owners: Vec<&RhoMatchProgram> = Vec::with_capacity(16);

    for event in RhoPathCursor::new(root) {
        if ignored_field_depth != 0 {
            match event {
                RhoPathEvent::EnterField(_) => ignored_field_depth += 1,
                RhoPathEvent::LeaveField => {
                    ignored_field_depth -= 1;
                    if ignored_field_depth == 0 {
                        append_match_path_event(&mut exact, event);
                    }
                }
                _ => {}
            }
            continue;
        }
        if suppressed_nodes != 0 {
            match event {
                RhoPathEvent::EnterNode { .. } => suppressed_nodes += 1,
                RhoPathEvent::LeaveNode => suppressed_nodes -= 1,
                _ => {}
            }
            continue;
        }

        match event {
            RhoPathEvent::EnterNode { cursor, program } => {
                if let ParCursor::Par(par) = cursor {
                    if let Some(hole) = rho_match_hole(par) {
                        if !record_linear_capture(captures, &hole) {
                            return None;
                        }
                        flush_exact_query_part(&mut parts, &mut exact);
                        parts.push(hole);
                        suppressed_nodes = 1;
                        saw_hole = true;
                        continue;
                    }
                    if !is_single_expr_match_carrier(par) {
                        return None;
                    }
                    // RMP2 deliberately keeps nested Par values opaque. Scan
                    // the admitted singleton Expr only to distinguish a ground
                    // exact token from a dynamic whole-value wildcard; never
                    // project it into a recursively owned Expr/Par graph.
                    let dynamic = if par.connective_used() || !par.locally_free().is_empty() {
                        true
                    } else {
                        let mut classification = None;
                        par.visit_encoded::<Expr>(|component| {
                            classification = Some(match classify_admitted_expr(&component) {
                                AdmittedExprRegionClass::ExactFreeVar(level) => {
                                    u32::try_from(level).ok().and_then(|level| {
                                        let id = RhoPathCaptureId::new(level);
                                        record_linear_capture(
                                            captures,
                                            &RhoMatchQueryPart::Capture(id),
                                        )
                                        .then_some(true)
                                    })
                                }
                                AdmittedExprRegionClass::ExactWildcard => Some(true),
                                AdmittedExprRegionClass::Structural(region) => {
                                    admitted_expr_is_dynamic(region, captures)
                                }
                            });
                        });
                        classification.flatten()?
                    };
                    if dynamic {
                        flush_exact_query_part(&mut parts, &mut exact);
                        parts.push(RhoMatchQueryPart::Wildcard);
                        suppressed_nodes = 1;
                        saw_hole = true;
                        continue;
                    }
                }
                if declines(program.type_name) {
                    // These carriers have associative-commutative matching
                    // semantics. Their dynamic children belong to the AC
                    // query machine, not this ordered cursor program. Decline
                    // the ordered query even for ground instances.
                    return None;
                }
                owners.push(program);
                append_match_path_event(&mut exact, event);
            }
            RhoPathEvent::EnterField(field)
                if matches!(
                    owners
                        .last()
                        .and_then(|owner| projection_rule(owner.type_name, field.name)),
                    Some(ProjectionRule::Ignored)
                ) =>
            {
                append_match_path_event(&mut exact, event);
                flush_exact_query_part(&mut parts, &mut exact);
                parts.push(RhoMatchQueryPart::Wildcard);
                // The matcher ignores Send's cost region. Skip its complete
                // optional-message value, whether absent or deeply nested;
                // scalar metadata uses this same field-boundary rule.
                ignored_field_depth = 1;
            }
            RhoPathEvent::LeaveNode => {
                owners.pop();
                append_match_path_event(&mut exact, event);
            }
            _ => append_match_path_event(&mut exact, event),
        }
    }
    // Send's persistent/channel/data equation is represented by this ordered
    // cursor even when every child is ground. Other ground root types still
    // require their own semantic projection before they may prune AC rows.
    if suppressed_nodes != 0
        || ignored_field_depth != 0
        || (!saw_hole && !matches!(root, ParCursor::Send(_)))
    {
        return None;
    }
    flush_exact_query_part(&mut parts, &mut exact);
    Some(RhoMatchQuery::new(parts))
}

/// Rholang receive binders are linear, while hand-built or persisted nonlinear
/// `BindPattern`s retain the spatial matcher's historical accumulation
/// behavior. A repeated level therefore cannot be reinterpreted as the generic
/// RhoPath query language's equality-constrained capture. Returning `false`
/// makes the entire derived query fall back to the authoritative matcher.
fn record_linear_capture(captures: &mut PathMap<()>, hole: &RhoMatchQueryPart) -> bool {
    match hole {
        RhoMatchQueryPart::Capture(id) => captures.insert(id.get().to_be_bytes(), ()).is_none(),
        RhoMatchQueryPart::Wildcard => true,
        RhoMatchQueryPart::Exact(_) => unreachable!("a pattern hole is never exact"),
    }
}

fn flush_exact_query_part(parts: &mut Vec<RhoMatchQueryPart>, exact: &mut Vec<u8>) {
    if !exact.is_empty() {
        parts.push(RhoMatchQueryPart::Exact(std::mem::take(exact)));
    }
}

fn rho_match_hole(pattern: &Par) -> Option<RhoMatchQueryPart> {
    if !is_single_expr_match_carrier(pattern) {
        return None;
    }
    let mut hole = None;
    pattern.visit_encoded::<Expr>(|component| {
        hole = match classify_admitted_expr(&component) {
            AdmittedExprRegionClass::ExactFreeVar(level) => u32::try_from(level)
                .ok()
                .map(RhoPathCaptureId::new)
                .map(RhoMatchQueryPart::Capture),
            AdmittedExprRegionClass::ExactWildcard => Some(RhoMatchQueryPart::Wildcard),
            AdmittedExprRegionClass::Structural(_) => None,
        };
    });
    hole
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// The production spatial-match oracle for guard evaluation (M-1a).
///
/// `rho-pure-eval` needs an answer to "does this target match this
/// pattern?" for `EMatchesBody`, but it cannot ask the matcher directly:
/// `rholang` depends on `rho-pure-eval`, not the other way round. So the
/// matcher is injected here, at the two places a guard is evaluated —
/// [`guard_passes`] below (the `for … where` cross-bind guard) and
/// `Reduce::eval_match` (the `match … where` case guard). Both use THIS
/// type, so the two guard languages decide spatial tests identically.
///
/// ★ Determinism. `SpatialMatcherContext` accumulates bindings in its
/// own `free_map` as it matches, so a shared context would make the
/// verdict depend on call history. A FRESH context is therefore built
/// per question and dropped before returning: no state survives a call,
/// the answer is a pure function of `(target, pattern)`, and the
/// determinism guarantee `rho-pure-eval` publishes ("same input Par +
/// same Env produce the same output Par byte-for-byte on every node")
/// extends unbroken across the seam. This is the same
/// construct-use-discard discipline `Matcher::get` and
/// `Reduce::combine_matches` already follow.
///
/// The bindings the match produces are deliberately dropped. A guard
/// runs after every bind is already resolved and may only answer
/// commit / do-not-commit, so `is_some()` is the whole answer.
pub struct SpatialMatcherOracle;

impl SpatialMatch for SpatialMatcherOracle {
    fn matches(&self, target: &Par, pattern: &Par) -> bool {
        let mut spatial_matcher = SpatialMatcherContext::new();
        spatial_matcher
            .spatial_match_result(target.clone(), pattern.clone())
            .is_some()
    }
}

/// What actually happened when a guard was put to the decider.
///
/// ★ **Why this is not a `bool`.** A guard verdict used to be one, and a
/// single bit cannot hold the distinction the caller most needs: whether the
/// guard was *decided against* or *never decided at all*. Every failure mode
/// collapsed into `false`, so a guard containing a construct the decider has
/// no arm for reported exactly what a refuted guard reports — and the program
/// then compiled, ran, exited 0, admitted nothing, and said nothing.
///
/// The same conflation was diagnosed and removed on the host lane, where the
/// remedy was a three-valued `GuardDisposition::{Fires, Blocks, Declines}`.
/// This is that remedy on the machine lane, with the two *decided-but-failed*
/// cases separated as well, so each variant is one fact:
///
/// | variant | evaluated? | what it means |
/// | --- | --- | --- |
/// | [`Admits`](Self::Admits) | yes | the predicate holds — commit |
/// | [`Refutes`](Self::Refutes) | yes | the predicate does not hold — an ordinary, correct non-commit |
/// | [`NotABoolean`](Self::NotABoolean) | yes | the guard reduced to something that is not a verdict |
/// | [`Failed`](Self::Failed) | attempted | the guard is decidable but errored on *this* datum (type mismatch, `/0`, overflow, unbound index) |
/// | [`Undecidable`](Self::Undecidable) | **no** | ★ the decider has no arm for a node in this guard — it never ran |
///
/// [`Undecidable`](Self::Undecidable) is the one the gate in
/// [`crate::rust::interpreter::guard`] exists to make unreachable, and
/// separating it here is what lets the gate be *checked* rather than assumed.
#[derive(Clone, Debug, PartialEq)]
pub enum GuardDisposition {
    /// Evaluated to `true`.
    Admits,
    /// Evaluated to `false`.
    Refutes,
    /// Evaluated to a value that is not a single `GBool`.
    NotABoolean,
    /// Evaluation ran and failed on this datum. Carries the exact error so a
    /// diagnostic can name it.
    Failed(EvalError),
    /// ★ Evaluation never ran: the guard contains a node kind
    /// `rho-pure-eval` has no arm for. `kind` is the `ExprInstance` variant
    /// name, e.g. `"EMethodBody"`.
    Undecidable { kind: &'static str },
}

impl GuardDisposition {
    /// The commit verdict: `true` only for [`Admits`](Self::Admits).
    ///
    /// ⚠ **Fail-closed, and the asymmetry is deliberate.** Firing a COMM the
    /// reducer would not fire is unsoundness in the worst direction, so every
    /// non-`Admits` disposition declines. What has changed is that declining
    /// is no longer the *only* thing that happens: the caller has the
    /// disposition itself and can refuse, log, or raise on it.
    pub fn commits(&self) -> bool { matches!(self, GuardDisposition::Admits) }
}

/// Decides a guard against the combined cross-bind variables.
///
/// `bound_pars` is the concatenation, in receive-bind order, of every bind's
/// matched data — which *is* the de Bruijn numbering the normalizer assigned,
/// so `BoundVar(k)` reads `bound_pars[bound_pars.len() - 1 - k]`.
///
/// The environment is built by `put`ting each bound par in order, so this
/// function is a pure function of `(condition, bound_pars)`: no state carries
/// between calls, which is what the determinism contract in
/// [`rho_pure_eval`]'s module docs requires of anything the matcher calls.
pub fn guard_disposition(condition: &Par, bound_pars: &[Par]) -> GuardDisposition {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        env = env.put(p.clone());
    }
    guard_disposition_in_env(condition, &env)
}

/// [`guard_disposition`] against an environment the caller has already built.
///
/// `Reduce::eval_match` needs this: a case guard is evaluated in `case_env`,
/// which is the ENCLOSING environment extended with the case's bindings, not
/// the bindings alone. Building a fresh environment from the bindings would
/// silently drop every outer variable the guard mentions and turn it into an
/// `UnboundVariable` failure.
///
/// Both entry points funnel into the same classification, so `for … where`
/// and `match … where` cannot disagree about what a disposition means.
pub fn guard_disposition_in_env(condition: &Par, env: &PureEnv<Par>) -> GuardDisposition {
    match rho_pure_eval::eval_with(condition, env, &SpatialMatcherOracle) {
        Ok(result) => match extract_bool(&result) {
            Some(true) => GuardDisposition::Admits,
            Some(false) => GuardDisposition::Refutes,
            None => {
                tracing::error!(
                    target: "f1r3fly.rholang.guard",
                    result = ?result,
                    "guard did not reduce to a boolean; declining to commit"
                );
                GuardDisposition::NotABoolean
            }
        },
        // ★ THE ARM THE WHOLE FIX TURNS ON. `UnsupportedExpression` is not a
        // verdict about the datum — it is the decider reporting that it never
        // looked. It gets its own disposition and an ERROR-level event naming
        // the node kind, because reaching it means a guard slipped past both
        // layers of `interpreter::guard`'s gate and that is a defect in the
        // node, not in the program.
        Err(EvalError::UnsupportedExpression { kind }) => {
            tracing::error!(
                target: "f1r3fly.rholang.guard",
                kind,
                condition = ?condition,
                "UNDECIDABLE GUARD reached the matcher: `interpreter::guard` \
                 should have refused this at normalize time or in eval_receive. \
                 Declining to commit."
            );
            GuardDisposition::Undecidable { kind }
        }
        Err(error) => {
            tracing::error!(
                target: "f1r3fly.rholang.guard",
                error = %error,
                "guard evaluation failed on this datum; declining to commit"
            );
            GuardDisposition::Failed(error)
        }
    }
}

/// [`guard_disposition`], projected onto the boolean `check_commit` returns.
///
/// ★ **Why the projection stays here rather than becoming fallible.**
/// `Match::check_commit` returns `bool` (`rspace++/src/rspace/match.rs`), and
/// widening it to a `Result` would thread a new failure path through
/// `consume`/`produce` in **both** the play and the replay space — the one
/// place where the comm-event sequence must be reproduced exactly — and would
/// break every out-of-workspace implementor of the trait. The undecidable case
/// is removed at its source instead (see
/// [`crate::rust::interpreter::guard`]), which leaves nothing for a fallible
/// signature to report.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    guard_disposition(condition, bound_pars).commits()
}

fn extract_bool(par: &Par) -> Option<bool> {
    if par.family_cardinality::<Send>() != 0
        || par.family_cardinality::<Receive>() != 0
        || par.family_cardinality::<New>() != 0
        || par.family_cardinality::<models::rhoapi::Match>() != 0
        || par.family_cardinality::<Bundle>() != 0
        || par.family_cardinality::<GUnforgeable>() != 0
        || par.family_cardinality::<Connective>() != 0
        || par.family_cardinality::<If>() != 0
        || par.family_cardinality::<Expr>() != 1
    {
        return None;
    }
    match par.first::<Expr>().ok()??.0.expr_instance? {
        ExprInstance::GBool(b) => Some(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! M-1a: the guard-side of the `SpatialMatch` seam.
    //!
    //! Before the seam, `rho_pure_eval::eval` refused `EMatchesBody`
    //! outright, so EVERY spatial guard failed shut regardless of whether
    //! it was true — the `Err(_) => false` collapse in [`guard_passes`]
    //! swallowed the `UnsupportedExpression`. These tests pin the flip
    //! (a true spatial guard now passes) and pin the two ways a guard
    //! must still fail shut (a genuine mismatch, and an eval error).

    use models::rust::utils::{new_gint_par, new_gstring_par, new_wildcard_par};

    use super::*;

    /// `BoundVar(0) matches <pattern>` — the shape a `for (x <- c) where
    /// (x matches P)` guard lowers to. `BoundVar(0)` is the first bound
    /// variable, which `guard_passes` puts at de Bruijn index 0.
    fn guard_first_bound_matches(pattern: Par) -> Par {
        ematches_guard(
            vector_par(models::create_bit_vector(&[0]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(0)),
                    }),
                })),
            }]),
            pattern,
        )
    }

    fn ematches_guard(target: Par, pattern: Par) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(EMatchesBody(EMatches {
                target: Some(target),
                pattern: Some(pattern),
            })),
        }])
    }

    /// The `Int` type pattern: a connective, so the matcher takes its
    /// connective path rather than structural equality.
    fn int_pattern() -> Par {
        vector_par(Vec::new(), true).with_connectives(vec![Connective {
            connective_instance: Some(ConnInt(true)),
        }])
    }

    #[test]
    fn spatial_guard_passes_when_the_bound_value_matches_a_wildcard() {
        let guard = guard_first_bound_matches(new_wildcard_par(Vec::new(), true));
        assert!(guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_passes_when_the_bound_value_matches_a_type_pattern() {
        let guard = guard_first_bound_matches(int_pattern());
        assert!(guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_a_genuine_mismatch() {
        // A String bound where the pattern demands an Int: the matcher
        // says no, so the guard says no.
        let guard = guard_first_bound_matches(int_pattern());
        assert!(!guard_passes(&guard, &[new_gstring_par(
            "five".to_string(),
            Vec::new(),
            false
        )]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_a_ground_mismatch() {
        let guard = guard_first_bound_matches(new_gint_par(7, Vec::new(), false));
        assert!(!guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
        assert!(guard_passes(&guard, &[new_gint_par(7, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_an_eval_error() {
        // BoundVar(3) with a single bound par is unbound → EvalError →
        // guard-fail. The DEBUG event names it; the verdict is unchanged.
        let unbound_target =
            vector_par(models::create_bit_vector(&[3]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(3)),
                    }),
                })),
            }]);
        let guard = ematches_guard(unbound_target, new_wildcard_par(Vec::new(), true));
        assert!(!guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_when_it_does_not_reduce_to_a_boolean() {
        // A non-boolean guard is guard-fail, not an error and not a pass.
        // This is the branch the new DEBUG event covers.
        assert!(!guard_passes(&new_gint_par(1, Vec::new(), false), &[
            new_gint_par(5, Vec::new(), false)
        ]));
    }

    #[test]
    fn the_seam_is_what_flipped_the_spatial_guard() {
        // Control: the same guard under the default `NoSpatialMatch`
        // oracle still yields `UnsupportedExpression`, which the
        // `Err(_) => false` collapse would turn into guard-fail. So the
        // pass above is attributable to the injected oracle and nothing
        // else — and the `eval` entry point is unchanged for every other
        // caller.
        let guard = guard_first_bound_matches(new_wildcard_par(Vec::new(), true));
        let mut env: PureEnv<Par> = PureEnv::new();
        let env = env.put(new_gint_par(5, Vec::new(), false));
        assert_eq!(
            rho_pure_eval::eval(&guard, &env),
            Err(rho_pure_eval::EvalError::UnsupportedExpression {
                kind: "EMatchesBody"
            })
        );
    }

    #[test]
    fn the_oracle_is_deterministic_across_calls() {
        // A fresh SpatialMatcherContext per question ⇒ no state carries
        // over. Ask a binding-heavy question (a free variable in the
        // pattern, which the matcher records in its free_map) repeatedly
        // and interleave a mismatch, then re-ask: every answer must be
        // the one the pair alone determines.
        let oracle = SpatialMatcherOracle;
        let five = new_gint_par(5, Vec::new(), false);
        let text = new_gstring_par("five".to_string(), Vec::new(), false);
        let binder = models::rust::utils::new_freevar_par(0, Vec::new());

        for _ in 0..8 {
            assert!(oracle.matches(&five, &binder));
            assert!(oracle.matches(&text, &binder));
            assert!(oracle.matches(&five, &int_pattern()));
            assert!(!oracle.matches(&text, &int_pattern()));
        }
    }

    #[test]
    fn cross_bind_spatial_guard_sees_every_bound_variable() {
        // Two binds. `guard_passes` pushes the bound pars onto the env in
        // receive-bind order, and de Bruijn indices count back from the
        // most recent push — so for `for (@x <- a; @y <- b)` it is
        // BoundVar(1) that reads `x` (bind 0) and BoundVar(0) that reads
        // `y` (bind 1). This test therefore pins the index assignment as
        // well as the spatial verdict: a guard on BoundVar(1) must track
        // the FIRST bound par as the pair is swapped.
        let first_bound =
            vector_par(models::create_bit_vector(&[1]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(1)),
                    }),
                })),
            }]);
        let guard = ematches_guard(first_bound, int_pattern());
        assert!(guard_passes(&guard, &[
            new_gint_par(9, Vec::new(), false),
            new_gstring_par("a".to_string(), Vec::new(), false),
        ]));
        assert!(!guard_passes(&guard, &[
            new_gstring_par("a".to_string(), Vec::new(), false),
            new_gint_par(9, Vec::new(), false),
        ]));
    }
}
