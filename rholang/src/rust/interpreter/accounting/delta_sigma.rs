use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Expr, New, Par, Receive, Send, Var};
use prost::Message;

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

    pub fn of_source(term: &Par, source_len: u64) -> Self {
        let comm = comm_bound(term);
        DeltaEstimate {
            comm,
            compute: comm.and_then(|_| compute_bound(term, source_len, &ScheduleBound::default())),
        }
    }

    pub fn bound(&self) -> Option<u64> {
        self.comm?.checked_add(self.compute?)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleBound {
    pub node_base: u64,
    pub per_bind: u64,
    pub size_factor: u64,
    pub name_expansion: u64,
    pub data_slack: u64,
}

impl Default for ScheduleBound {
    fn default() -> Self {
        ScheduleBound {
            node_base: 256,
            per_bind: 128,
            size_factor: 4,
            name_expansion: 8,
            data_slack: 64,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Name,
    Data(u32),
}

struct Estimate<'a> {
    schedule: &'a ScheduleBound,
    data: u64,
    scope: Vec<Slot>,
    total: u64,
}

pub fn compute_bound(term: &Par, source_len: u64, schedule: &ScheduleBound) -> Option<u64> {
    let size = term.encoded_len() as u64;
    let data = schedule
        .name_expansion
        .checked_mul(size)?
        .checked_add(schedule.data_slack)?;
    let mut estimate = Estimate {
        schedule,
        data,
        scope: Vec::new(),
        total: source_len,
    };
    estimate.par(term)?;
    Some(estimate.total)
}

impl Estimate<'_> {
    fn charge(&mut self, node_len: usize, binds: usize) -> Option<()> {
        let s = self.schedule;
        let size = s
            .name_expansion
            .checked_mul(node_len as u64)?
            .checked_add(if self.receives_data() { self.data } else { 0 })?;
        let node = s
            .node_base
            .checked_add(s.per_bind.checked_mul(binds as u64)?)?
            .checked_add(s.size_factor.checked_mul(size)?)?;
        self.total = self.total.checked_add(node)?;
        Some(())
    }

    fn receives_data(&self) -> bool {
        self.scope.iter().any(|slot| matches!(slot, Slot::Data(_)))
    }

    fn bound<T>(&mut self, slots: Vec<Slot>, inner: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let depth = self.scope.len();
        self.scope.extend(slots);
        let result = inner(self);
        let affine = self.scope[depth..]
            .iter()
            .all(|slot| !matches!(slot, Slot::Data(n) if *n > 1));
        self.scope.truncate(depth);
        if affine {
            result
        } else {
            None
        }
    }

    fn par(&mut self, par: &Par) -> Option<()> {
        if !par.cost_stacks.is_empty() {
            return None;
        }
        for send in &par.sends {
            self.charge(send.encoded_len(), send.data.len())?;
            self.par(send.chan.as_ref()?)?;
            send.data.iter().try_for_each(|d| self.par(d))?;
        }
        for receive in &par.receives {
            if receive.condition.is_some() {
                return None;
            }
            self.charge(receive.encoded_len(), receive.binds.len())?;
            for bind in &receive.binds {
                self.par(bind.source.as_ref()?)?;
                bind.patterns.iter().try_for_each(|p| self.par(p))?;
                self.var(bind.remainder.as_ref())?;
            }
            let count = usize::try_from(receive.bind_count).ok()?;
            let body = receive.body.as_ref()?;
            self.bound(vec![Slot::Data(0); count], |e| e.par(body))?;
        }
        for new in &par.news {
            if !new.injections.is_empty() {
                return None;
            }
            self.charge(new.encoded_len(), 0)?;
            let count = usize::try_from(new.bind_count).ok()?;
            let body = new.p.as_ref()?;
            self.bound(vec![Slot::Name; count], |e| e.par(body))?;
        }
        for matched in &par.matches {
            self.charge(matched.encoded_len(), matched.cases.len())?;
            self.par(matched.target.as_ref()?)?;
            for case in &matched.cases {
                if case.guard.is_some() {
                    return None;
                }
                self.par(case.pattern.as_ref()?)?;
                let count = usize::try_from(case.free_count).ok()?;
                let body = case.source.as_ref()?;
                self.bound(vec![Slot::Data(0); count], |e| e.par(body))?;
            }
        }
        for conditional in &par.conditionals {
            self.charge(conditional.encoded_len(), 0)?;
            self.par(conditional.condition.as_ref()?)?;
            for branch in [&conditional.if_true, &conditional.if_false].into_iter().flatten() {
                self.par(branch)?;
            }
        }
        for bundle in &par.bundles {
            self.charge(bundle.encoded_len(), 0)?;
            self.par(bundle.body.as_ref()?)?;
        }
        for term in &par.cost_signed_terms {
            self.charge(term.encoded_len(), 0)?;
            self.par(term.body.as_ref()?)?;
        }
        for expr in &par.exprs {
            self.expr(expr)?;
        }
        Some(())
    }

    fn var(&mut self, var: Option<&Var>) -> Option<()> {
        if let Some(VarInstance::BoundVar(index)) = var.and_then(|v| v.var_instance.as_ref()) {
            let index = usize::try_from(*index).ok()?;
            let position = self.scope.len().checked_sub(index.checked_add(1)?)?;
            if let Slot::Data(n) = &mut self.scope[position] {
                *n = n.saturating_add(1);
            }
        }
        Some(())
    }

    fn expr(&mut self, expr: &Expr) -> Option<()> {
        use ExprInstance::*;
        self.charge(expr.encoded_len(), 0)?;
        match expr.expr_instance.as_ref()? {
            GBool(_) | GInt(_) | GString(_) | GUri(_) | GByteArray(_) => Some(()),
            EVarBody(var) => self.var(var.v.as_ref()),
            ENotBody(e) => self.par(e.p.as_ref()?),
            ENegBody(e) => self.par(e.p.as_ref()?),
            EMultBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EDivBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EModBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EPlusBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EMinusBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            ELtBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            ELteBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EGtBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EGteBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EEqBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            ENeqBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EAndBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EOrBody(e) => self.pair(e.p1.as_ref(), e.p2.as_ref()),
            EListBody(e) => {
                e.ps.iter().try_for_each(|p| self.par(p))?;
                self.var(e.remainder.as_ref())
            }
            ETupleBody(e) => e.ps.iter().try_for_each(|p| self.par(p)),
            ESetBody(e) => {
                e.ps.iter().try_for_each(|p| self.par(p))?;
                self.var(e.remainder.as_ref())
            }
            EMapBody(e) => {
                for kv in &e.kvs {
                    self.par(kv.key.as_ref()?)?;
                    self.par(kv.value.as_ref()?)?;
                }
                self.var(e.remainder.as_ref())
            }
            _ => None,
        }
    }

    fn pair(&mut self, p1: Option<&Par>, p2: Option<&Par>) -> Option<()> {
        self.par(p1?)?;
        self.par(p2?)
    }
}
