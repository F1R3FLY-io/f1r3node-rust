use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Expr, New, Par, Receive, Send};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeltaEstimate {
    pub comm: Option<u64>,
    pub compute: Option<u64>,
}

impl DeltaEstimate {
    pub fn of(term: &Par) -> Self {
        DeltaEstimate {
            comm: comm_bound(term),
            compute: None,
        }
    }

    pub fn delta(&self, phlo_limit: u64) -> u64 {
        match (self.comm, self.compute) {
            (Some(comm), Some(compute)) => comm.saturating_add(compute).min(phlo_limit),
            _ => phlo_limit,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binder {
    Private,
    Other,
}

pub fn comm_bound(term: &Par) -> Option<u64> {
    let mut scope = Vec::new();
    par_bound(term, &mut scope)
}

fn par_bound(par: &Par, scope: &mut Vec<Binder>) -> Option<u64> {
    if !par.cost_stacks.is_empty() {
        return None;
    }
    if par.exprs.iter().any(evaluates_a_process) {
        return None;
    }
    let mut total: u64 = 0;
    for send in &par.sends {
        send_bound(send, scope)?;
    }
    for receive in &par.receives {
        total = total.checked_add(receive_bound(receive, scope)?)?;
    }
    for new in &par.news {
        total = total.checked_add(new_bound(new, scope)?)?;
    }
    for matched in &par.matches {
        let mut most: u64 = 0;
        for case in &matched.cases {
            let bound = usize::try_from(case.free_count).ok()?;
            let body = case.source.as_ref()?;
            most = most.max(scoped(scope, vec![Binder::Other; bound], body)?);
        }
        total = total.checked_add(most)?;
    }
    for conditional in &par.conditionals {
        let branch = |p: &Option<Par>, scope: &mut Vec<Binder>| match p {
            Some(p) => par_bound(p, scope),
            None => Some(0),
        };
        let most = branch(&conditional.if_true, scope)?.max(branch(&conditional.if_false, scope)?);
        total = total.checked_add(most)?;
    }
    for bundle in &par.bundles {
        total = total.checked_add(par_bound(bundle.body.as_ref()?, scope)?)?;
    }
    for term in &par.cost_signed_terms {
        total = total.checked_add(par_bound(term.body.as_ref()?, scope)?)?;
    }
    Some(total)
}

fn scoped(scope: &mut Vec<Binder>, binders: Vec<Binder>, body: &Par) -> Option<u64> {
    let depth = scope.len();
    scope.extend(binders);
    let bound = par_bound(body, scope);
    scope.truncate(depth);
    bound
}

fn new_bound(new: &New, scope: &mut Vec<Binder>) -> Option<u64> {
    let count = usize::try_from(new.bind_count).ok()?;
    let fresh = count.checked_sub(new.uri.len())?;
    let binders = std::iter::repeat_n(Binder::Private, fresh)
        .chain(std::iter::repeat_n(Binder::Other, new.uri.len()))
        .collect();
    scoped(scope, binders, new.p.as_ref()?)
}

fn send_bound(send: &Send, scope: &[Binder]) -> Option<()> {
    if send.persistent || !is_private(send.chan.as_ref()?, scope) {
        return None;
    }
    Some(())
}

fn receive_bound(receive: &Receive, scope: &mut Vec<Binder>) -> Option<u64> {
    if receive.persistent {
        return None;
    }
    for bind in &receive.binds {
        if !is_private(bind.source.as_ref()?, scope) {
            return None;
        }
    }
    let bound = usize::try_from(receive.bind_count).ok()?;
    let body = scoped(scope, vec![Binder::Other; bound], receive.body.as_ref()?)?;
    body.checked_add(1)
}

fn is_private(channel: &Par, scope: &[Binder]) -> bool {
    let only_a_variable = channel.sends.is_empty()
        && channel.receives.is_empty()
        && channel.news.is_empty()
        && channel.matches.is_empty()
        && channel.unforgeables.is_empty()
        && channel.bundles.is_empty()
        && channel.connectives.is_empty()
        && channel.conditionals.is_empty()
        && channel.cost_signed_terms.is_empty()
        && channel.cost_stacks.is_empty();
    match (only_a_variable, channel.exprs.as_slice()) {
        (true, [expr]) => match &expr.expr_instance {
            Some(ExprInstance::EVarBody(var)) => {
                match var.v.as_ref().and_then(|v| v.var_instance.as_ref()) {
                    Some(VarInstance::BoundVar(index)) => usize::try_from(*index)
                        .ok()
                        .and_then(|index| scope.len().checked_sub(index + 1))
                        .is_some_and(|position| scope[position] == Binder::Private),
                    _ => false,
                }
            }
            _ => false,
        },
        _ => false,
    }
}

fn evaluates_a_process(expr: &Expr) -> bool {
    matches!(
        expr.expr_instance,
        Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
    )
}
