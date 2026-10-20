//! G1-4a (DR-122): the recursive oracle of the estimate's walks. Pass 1 runs
//! as a recursion that calls the same steps of `Estimator` in the same order,
//! and pass 2 folds the term recursively with sums and maxima (design §5.5).
//! The parity tests compare both with the work-stack machines.

// Added by the merge of feature/cost-accounted-rho (finding R1-29): the
// oracle walks the parallel tails of a token stack.
use models::rhoapi::CostStack;

use super::*;

/// The result of one analysis: the estimate, the participants and the first
/// failed rule of pass 1, the vector before assembly and the program.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Analysis {
    pub(super) estimate: CostEstimate,
    pub(super) participants: Vec<Participant>,
    pub(super) reason: Option<IncompleteReason>,
    pub(super) vector: BTreeMap<SigKey, [u64; 4]>,
}

/// Pass 1 as a recursion.
fn walk(
    estimator: &mut Estimator<'_>,
    par: &Par,
    scope: Option<usize>,
    resolving: bool,
) -> Result<(), InterpreterError> {
    estimator.enter_visit(par)?;
    for send in &par.sends {
        estimator.send(send, scope)?;
    }
    for receive in &par.receives {
        estimator.receive(receive, scope)?;
        if let Some(body) = receive.body.as_ref() {
            let count = usize::try_from(receive.bind_count).unwrap_or(0);
            estimator.push_holes(count, BinderKind::ReceivePattern)?;
            walk(estimator, body, None, false)?;
            estimator.pop(count)?;
        }
    }
    for new in &par.news {
        let count = estimator.push_new(new, resolving)?;
        if let Some(body) = new.p.as_ref() {
            walk(estimator, body, scope, resolving)?;
        }
        estimator.pop(count)?;
    }
    for matched in &par.matches {
        estimator.check_match(matched);
        for case in &matched.cases {
            estimator.check_case(case);
            if let Some(source) = case.source.as_ref() {
                let count = usize::try_from(case.free_count).unwrap_or(0);
                estimator.push_holes(count, BinderKind::MatchPattern)?;
                walk(estimator, source, scope, resolving)?;
                estimator.pop(count)?;
            }
        }
    }
    for conditional in &par.conditionals {
        estimator.check_conditional(conditional);
        for branch in conditional
            .if_true
            .iter()
            .chain(conditional.if_false.iter())
        {
            walk(estimator, branch, scope, resolving)?;
        }
    }
    for bundle in &par.bundles {
        if let Some(body) = bundle.body.as_ref() {
            walk(estimator, body, scope, resolving)?;
        }
    }
    estimator.check_exprs(par)?;
    for term in &par.cost_signed_terms {
        let opened = estimator.open_scope(term, scope)?;
        if let Some(body) = term.body.as_ref() {
            walk(estimator, body, Some(opened), resolving)?;
        }
    }
    estimator.check_stacks(par);
    Ok(())
}

fn add(left: Vector, right: Vector) -> Vector { merge_vectors(left, right, u64::saturating_add) }

fn max(left: Vector, right: Vector) -> Vector { merge_vectors(left, right, u64::max) }

/// Pass 2 as a recursion: parallel parts add, a receive adds its own
/// contribution to the vector of its body, and a branch point takes the
/// componentwise maximum of its branches. `next` numbers the participants in
/// the order of pass 1.
fn fold(estimator: &Estimator<'_>, tables: &Tables, par: &Par, next: &mut usize) -> Vector {
    let mut acc = Vector::new();
    let local = |acc: &mut Vector, next: &mut usize| {
        if let Some(participant) = estimator.participants.get(*next) {
            estimator.contribute(participant, tables, acc);
        }
        *next += 1;
    };
    for _ in &par.sends {
        local(&mut acc, next);
    }
    for receive in &par.receives {
        local(&mut acc, next);
        if let Some(body) = receive.body.as_ref() {
            let child = fold(estimator, tables, body, next);
            acc = add(acc, child);
        }
    }
    for new in &par.news {
        if let Some(body) = new.p.as_ref() {
            let child = fold(estimator, tables, body, next);
            acc = add(acc, child);
        }
    }
    for matched in &par.matches {
        let mut branches: Option<Vector> = None;
        for source in matched.cases.iter().filter_map(|case| case.source.as_ref()) {
            let branch = fold(estimator, tables, source, next);
            branches = Some(match branches {
                Some(earlier) => max(earlier, branch),
                None => branch,
            });
        }
        if let Some(branches) = branches {
            acc = add(acc, branches);
        }
    }
    for conditional in &par.conditionals {
        let branch = |choice: &Option<Par>, next: &mut usize| match choice.as_ref() {
            Some(body) => fold(estimator, tables, body, next),
            None => Vector::new(),
        };
        let if_true = branch(&conditional.if_true, next);
        let if_false = branch(&conditional.if_false, next);
        acc = add(acc, max(if_true, if_false));
    }
    for bundle in &par.bundles {
        if let Some(body) = bundle.body.as_ref() {
            let child = fold(estimator, tables, body, next);
            acc = add(acc, child);
        }
    }
    for term in &par.cost_signed_terms {
        if let Some(body) = term.body.as_ref() {
            let child = fold(estimator, tables, body, next);
            acc = add(acc, child);
        }
    }
    acc
}

/// The estimate with both passes recursive. Callers run it on a thread whose
/// stack grows with the nesting depth of `resolved`.
pub(super) fn analyze(
    resolved: &Par,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<Analysis, InterpreterError> {
    let rules = NativePhloRules::resolve(schedule)
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    let core = isolated_evaluator(evaluator);
    let mut estimator = Estimator::new(funding, &core)?;
    estimator.measure(resolved)?;
    let shallow = estimator.signatures_fit();
    let plan = match shallow {
        true => Some(static_authority_plan(resolved, funding)),
        false => {
            estimator.violate(IncompleteReason::UnsizedValue);
            None
        }
    };
    walk(&mut estimator, resolved, None, true)?;
    let tables = estimator.tables()?;
    let mut next = 0usize;
    let vector = fold(&estimator, &tables, resolved, &mut next);
    let static_signatures = match shallow {
        true => static_authority_signatures(resolved).ok(),
        false => None,
    };
    let ordered = vector.clone().into_iter().collect();
    let estimate = estimator.assemble(plan, vector, static_signatures, &rules, schedule)?;
    Ok(Analysis {
        estimate,
        participants: estimator.participants,
        reason: estimator.reason,
        vector: ordered,
    })
}

/// The estimate with both passes as work-stack machines, with the same
/// observations as `analyze`.
pub(super) fn analyze_iteratively(
    resolved: &Par,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<Analysis, InterpreterError> {
    let rules = NativePhloRules::resolve(schedule)
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    let core = isolated_evaluator(evaluator);
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core)?;
        estimator.measure(resolved)?;
        let shallow = estimator.signatures_fit();
        let plan = match shallow {
            true => Some(static_authority_plan(resolved, funding)),
            false => {
                estimator.violate(IncompleteReason::UnsizedValue);
                None
            }
        };
        estimator.walk(resolved)?;
        let tables = estimator.tables()?;
        let vector = estimator.run(&tables)?;
        let static_signatures = match shallow {
            true => static_authority_signatures(resolved).ok(),
            false => None,
        };
        let ordered = vector.clone().into_iter().collect();
        let estimate = estimator.assemble(plan, vector, static_signatures, &rules, schedule)?;
        Ok(Analysis {
            estimate,
            participants: estimator.participants,
            reason: estimator.reason,
            vector: ordered,
        })
    })
}

/// The level counts of pass 1 on `program`, keyed by node address.
pub(super) fn estimator_levels(
    program: &Par,
    funding: &Sig,
    evaluator: &ReducerCore,
) -> Result<Vec<(usize, usize)>, InterpreterError> {
    let core = isolated_evaluator(evaluator);
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core)?;
        estimator.measure(program)?;
        estimator.walk(program)?;
        Ok(estimator.levels)
    })
}

/// Added by the G1-4a fix (review H-1): the recursive form of
/// `signature_holds_variable`. It reads the fields of the messages directly,
/// without `subterm_parts`, and recurses once per nesting level. A variable
/// is an `EVar`, a remainder, a variable reference or a `BoundLevel`.
pub(super) fn signature_holds_variable(signature: &CostSignature) -> bool {
    match signature.value.as_ref() {
        Some(CostSignatureValue::BoundLevel(_)) => true,
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
            par_holds_variable(par)
        }
        Some(CostSignatureValue::Compound(compound)) => {
            compound.elements.iter().any(signature_holds_variable)
        }
        Some(CostSignatureValue::Ground(_)) | Some(CostSignatureValue::Unit(_)) | None => false,
    }
}

fn pars_hold_variable<'a>(pars: impl IntoIterator<Item = &'a Par>) -> bool {
    pars.into_iter().any(par_holds_variable)
}

fn par_holds_variable(par: &Par) -> bool {
    par.sends
        .iter()
        .any(|send| pars_hold_variable(send.chan.iter().chain(&send.data)))
        || par.receives.iter().any(|receive| {
            receive.binds.iter().any(|bind| {
                bind.remainder.is_some()
                    || pars_hold_variable(bind.patterns.iter().chain(&bind.source))
                    || bind.cost_signature.iter().any(signature_holds_variable)
            }) || pars_hold_variable(receive.body.iter().chain(&receive.condition))
        })
        || par
            .news
            .iter()
            .any(|new| pars_hold_variable(new.p.iter().chain(new.injections.values())))
        || par.exprs.iter().any(expr_holds_variable)
        || par.matches.iter().any(|matched| {
            pars_hold_variable(&matched.target)
                || matched.cases.iter().any(|case| {
                    pars_hold_variable(case.pattern.iter().chain(&case.source).chain(&case.guard))
                })
        })
        || par
            .bundles
            .iter()
            .any(|bundle| pars_hold_variable(&bundle.body))
        || par
            .connectives
            .iter()
            .any(|connective| match connective.connective_instance.as_ref() {
                Some(ConnectiveInstance::ConnAndBody(body))
                | Some(ConnectiveInstance::ConnOrBody(body)) => pars_hold_variable(&body.ps),
                Some(ConnectiveInstance::ConnNotBody(body)) => par_holds_variable(body),
                Some(ConnectiveInstance::VarRefBody(_)) => true,
                Some(
                    ConnectiveInstance::ConnBool(_)
                    | ConnectiveInstance::ConnInt(_)
                    | ConnectiveInstance::ConnString(_)
                    | ConnectiveInstance::ConnUri(_)
                    | ConnectiveInstance::ConnByteArray(_),
                )
                | None => false,
            })
        || par.conditionals.iter().any(|conditional| {
            pars_hold_variable(
                conditional
                    .condition
                    .iter()
                    .chain(&conditional.if_true)
                    .chain(&conditional.if_false),
            )
        })
        || par.cost_signed_terms.iter().any(|term| {
            pars_hold_variable(&term.body) || term.signature.iter().any(signature_holds_variable)
        })
        // Changed by the merge of feature/cost-accounted-rho (finding R1-29):
        // F1R3Lang's token stack is a tree, and the work-stack scan also
        // visits the cells of the parallel tails.
        // || par
        //     .cost_stacks
        //     .iter()
        //     .any(|stack| stack.cells.iter().any(signature_holds_variable))
        || par.cost_stacks.iter().any(stack_holds_variable)
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-29): whether
/// a cell of `stack` or of one of its parallel tails holds a variable.
fn stack_holds_variable(stack: &CostStack) -> bool {
    stack.cells.iter().any(signature_holds_variable)
        || stack.parallel_tail.iter().any(stack_holds_variable)
}

fn expr_holds_variable(expr: &Expr) -> bool {
    let pair =
        |left: &Option<Par>, right: &Option<Par>| pars_hold_variable(left.iter().chain(right));
    match expr.expr_instance.as_ref() {
        Some(ExprInstance::EVarBody(_)) => true,
        Some(ExprInstance::ENotBody(body)) => pars_hold_variable(&body.p),
        Some(ExprInstance::ENegBody(body)) => pars_hold_variable(&body.p),
        Some(ExprInstance::EMultBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EDivBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EModBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EPlusBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EMinusBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::ELtBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::ELteBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EGtBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EGteBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EEqBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::ENeqBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EAndBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EOrBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EPercentPercentBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EPlusPlusBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EMinusMinusBody(body)) => pair(&body.p1, &body.p2),
        Some(ExprInstance::EMatchesBody(body)) => pair(&body.target, &body.pattern),
        Some(ExprInstance::EListBody(list)) => {
            list.remainder.is_some() || pars_hold_variable(&list.ps)
        }
        Some(ExprInstance::ETupleBody(tuple)) => pars_hold_variable(&tuple.ps),
        Some(ExprInstance::ESetBody(set)) => set.remainder.is_some() || pars_hold_variable(&set.ps),
        Some(ExprInstance::EMapBody(map)) => {
            map.remainder.is_some()
                || map
                    .kvs
                    .iter()
                    .any(|entry| pars_hold_variable(entry.key.iter().chain(&entry.value)))
        }
        Some(ExprInstance::EMethodBody(method)) => {
            pars_hold_variable(method.target.iter().chain(&method.arguments))
        }
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // F1R3Lang's `EPathMap` has no `ps` vector. `pathmap_holds_variable`
        // decodes the members.
        // Some(ExprInstance::EPathmapBody(pathmap)) => {
        //     pathmap.remainder.is_some() || pars_hold_variable(&pathmap.ps)
        // }
        // Some(ExprInstance::EZipperBody(zipper)) => zipper
        //     .pathmap
        //     .as_ref()
        //     .is_some_and(|pathmap| pathmap.remainder.is_some() || pars_hold_variable(&pathmap.ps)),
        Some(ExprInstance::EPathmapBody(pathmap)) => {
            pathmap.remainder.is_some() || pathmap_holds_variable(pathmap)
        }
        Some(ExprInstance::EZipperBody(zipper)) => zipper
            .pathmap
            .as_ref()
            .is_some_and(|pathmap| pathmap.remainder.is_some() || pathmap_holds_variable(pathmap)),
        Some(
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_)
            | ExprInstance::GUint64(_)
            | ExprInstance::GFloat32(_)
            | ExprInstance::GInt32(_)
            | ExprInstance::GUint32(_)
            | ExprInstance::GUint16(_)
            | ExprInstance::GUint8(_),
        )
        | None => false,
    }
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-16): whether
/// a member of `pathmap` holds a variable. Set members decode through
/// `entries_owned`, and map entries through `for_each_map_entry`. This path
/// is apart from the raw visitors of the work-stack scan.
fn pathmap_holds_variable(pathmap: &EPathMap) -> bool {
    match pathmap.mode() {
        EPathMapMode::Empty => false,
        EPathMapMode::Set => pars_hold_variable(&pathmap.entry_trie().entries_owned()),
        EPathMapMode::Map => {
            let mut holds = false;
            pathmap
                .for_each_map_entry(|key, value| {
                    holds = holds || par_holds_variable(key) || par_holds_variable(value);
                })
                .expect("a map-mode pathmap visits its map entries");
            holds
        }
    }
}

/// Added by the second G1-4a fix (re-review HIGH-1): the peak binder count
/// of the pre-pass on `program`, and the most binders that pass 1 held at
/// once.
pub(super) fn peak_binders(
    program: &Par,
    funding: &Sig,
    evaluator: &ReducerCore,
) -> Result<(usize, usize), InterpreterError> {
    let core = isolated_evaluator(evaluator);
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core)?;
        let counts = estimator.measure(program)?;
        estimator.walk(program)?;
        Ok((counts.peak_binders, estimator.observed_peak))
    })
}
