//! G1-4a (DR-122): the recursive oracle of the estimate's walks. Pass 1 runs
//! as a recursion that calls the same steps of `Estimator` in the same order,
//! and pass 2 folds the term recursively with sums and maxima (design §5.5).
//! The parity tests compare both with the work-stack machines.

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
    host: &HostWorkBudget,
) -> Result<Analysis, InterpreterError> {
    let rules = NativePhloRules::resolve(schedule)
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    let core = isolated_evaluator(evaluator);
    let mut estimator = Estimator::new(funding, &core, host)?;
    let counts = estimator.measure(resolved)?;
    estimator.charge_plan(&counts)?;
    let plan = static_authority_plan(resolved, funding);
    walk(&mut estimator, resolved, None, true)?;
    let tables = estimator.tables()?;
    let mut next = 0usize;
    let vector = fold(&estimator, &tables, resolved, &mut next);
    let static_signatures = static_authority_signatures(resolved).ok();
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
    host: &HostWorkBudget,
) -> Result<Analysis, InterpreterError> {
    let rules = NativePhloRules::resolve(schedule)
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    let core = isolated_evaluator(evaluator);
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core, host)?;
        let counts = estimator.measure(resolved)?;
        estimator.charge_plan(&counts)?;
        let plan = static_authority_plan(resolved, funding);
        estimator.walk(resolved)?;
        let tables = estimator.tables()?;
        estimator.charge_fold(&counts)?;
        let vector = estimator.run(&tables)?;
        estimator.charge_signatures(&counts)?;
        let static_signatures = static_authority_signatures(resolved).ok();
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
    let host = HostWorkBudget::new(models::rust::host_work::HostWorkLimits::uniform(
        models::rust::host_work::HostWorkLimit::new(u64::MAX),
    ));
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core, &host)?;
        estimator.measure(program)?;
        estimator.walk(program)?;
        Ok(estimator.levels)
    })
}
