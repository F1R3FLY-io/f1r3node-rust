use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
#[cfg(test)]
use models::rhoapi::Receive;
use models::rhoapi::{
    Bundle, CostSignature, CostSignedTerm, CostStack, EVar, Expr, GPrivate, GUnforgeable, If,
    Match, MatchCase, New, Par, ReceiveBind, Send, Var,
};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::utils::union;
use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};
use shared::rust::collection_backing::hash_backing;

use super::costs::Cost;
use super::RuntimeBudget;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::host_work::HostWorkBudget;
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::rho_type::{RhoExpression, RhoUnforgeable};
use crate::rust::interpreter::substitute::Substitute;
#[cfg(test)]
use crate::rust::interpreter::substitute::SubstituteTrait;
use crate::rust::interpreter::util::evaluation_random;
#[cfg(test)]
use crate::rust::interpreter::util::{allocate_new_bindings, evaluation_terms};

/// Resolves, for the funding analysis, the names that a deploy binds with
/// `new` where the reducer allocates them from the deploy's own randomness.
/// The resolved names replace the bound levels of the cost signatures that the
/// analyzer reads. This entry is not metered. The pre-execution funding check
/// uses [`resolve_lexical_names_for_funding_metered`].
pub fn resolve_lexical_names_for_funding(
    program: &Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
) -> Result<Par, InterpreterError> {
    // Changed by G1-2 (DR-67): the recursive resolver below substituted the
    // whole body of each `new` again and copied each level to count its terms.
    // Both costs grow with the square of the nesting depth, and the recursion
    // grows the stack with the depth. It stays as the `#[cfg(test)]` oracle.
    // let substitute = Substitute {
    //     metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    // };
    // resolve_par(program.clone(), rand, urn_map, &substitute)
    FundingResolver::new(urn_map, None).run(program, rand)
}

/// G1-2 (DR-67): the funding resolver of the pre-execution funding check. It
/// runs on every incoming deploy before any payment, so it charges `host` for
/// each unit of its work before it does the work, and it returns
/// [`InterpreterError::HostWorkRejected`] when the budget is exhausted.
///
/// The resolver visits each node once, keeps the bindings in one scope stack
/// and resolves each cost signature once, so its work is linear in the size of
/// the program. Its walk uses a heap work stack, so its stack depth does not
/// grow with the nesting depth of the program.
pub fn resolve_lexical_names_for_funding_metered(
    program: &Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    host: &HostWorkBudget,
) -> Result<Par, InterpreterError> {
    FundingResolver::new(urn_map, Some(host)).run(program, rand)
}

/// The randomness of a position. `Some` marks a position where the reducer
/// evaluates the term with the deploy's own randomness, so the resolver
/// allocates the names of a `new` there. `None` marks a position that the
/// resolver does not resolve: send data, receive bodies (G1-3, DR-121), and
/// every term below them.
type Position = Option<Blake2b512Random>;

/// One task of the resolver's work stack. The resolver pops one task at a time.
enum Task<'a> {
    /// Check the term count of a node, then schedule its children and its
    /// rebuild in the reducer's term order.
    Visit(&'a Par, Position),
    /// Resolve the signatures of a receive's binds in the receive's scope.
    BindSignatures(&'a [ReceiveBind]),
    /// Resolve the signature of a signed term in its scope.
    Signature(&'a CostSignature),
    /// Resolve the cells of a token stack in its scope.
    Cells(&'a CostStack),
    /// Push binders that the resolver leaves unresolved: the variables of a
    /// receive pattern or a match case.
    Holes(usize),
    /// Bind the names of a `new` (allocated in a resolving position, or holes
    /// elsewhere) and schedule its body.
    Enter(&'a New, Position),
    /// Pop the binders of the innermost scope.
    Pop { count: usize, resolved: bool },
    /// Rebuild a node from a copy of the fields that the resolver does not walk
    /// and from its rebuilt children.
    Build(&'a Par),
}

/// The meter of copies and signature work: the reducer's mapping of
/// operations, scanned bytes and backing bytes to the host-work dimensions.
struct HostBacking<'h>(Option<&'h HostWorkBudget>);

impl BackingMeter for HostBacking<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        let Some(host) = self.0 else {
            return Ok(());
        };
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, operations),
            (HostWorkDimension::VerificationBytes, scanned),
            (HostWorkDimension::SearchStateBytes, backing),
        ] {
            let amount = u64::try_from(amount).map_err(|_| BackingError::Overflow)?;
            host.reserve(dimension, HostWorkUnits::new(amount))
                .map_err(|_| BackingError::Rejected)?;
        }
        Ok(())
    }
}

fn host_rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

/// The number of evaluation terms of a node, as the reducer counts them
/// (`util::evaluation_terms`). It reads the node without copying it.
fn term_count(par: &Par) -> usize {
    let dequotations = par
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.expr_instance,
                Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
            )
        })
        .count();
    par.sends.len()
        + par.receives.len()
        + par.news.len()
        + par.matches.len()
        + par.conditionals.len()
        + par.bundles.len()
        + dequotations
        + par.cost_signed_terms.len()
        + par.cost_stacks.len()
}

/// The number of rebuilt children and resolved signatures that the rebuild of
/// `par` takes from the value stacks.
fn rebuilt_parts(par: &Par) -> (usize, usize) {
    let mut children = 0usize;
    let mut signatures = 0usize;
    for send in &par.sends {
        children += send.data.len();
    }
    for receive in &par.receives {
        signatures += receive
            .binds
            .iter()
            .filter(|bind| bind.cost_signature.is_some())
            .count();
        children += usize::from(receive.body.is_some());
    }
    for new in &par.news {
        children += usize::from(new.p.is_some());
    }
    for matched in &par.matches {
        children += matched
            .cases
            .iter()
            .filter(|case| case.source.is_some())
            .count();
    }
    for conditional in &par.conditionals {
        children += usize::from(conditional.if_true.is_some())
            + usize::from(conditional.if_false.is_some());
    }
    for bundle in &par.bundles {
        children += usize::from(bundle.body.is_some());
    }
    for signed in &par.cost_signed_terms {
        children += usize::from(signed.body.is_some());
        signatures += usize::from(signed.signature.is_some());
    }
    for stack in &par.cost_stacks {
        signatures += stack.cells.len();
    }
    (children, signatures)
}

/// The number of tasks that the visit of `par` schedules.
fn scheduled_tasks(par: &Par) -> usize {
    let mut tasks = 1usize;
    for send in &par.sends {
        tasks += send.data.len();
    }
    for receive in &par.receives {
        tasks += 3 + usize::from(receive.body.is_some());
    }
    tasks += par.news.len();
    for matched in &par.matches {
        for case in &matched.cases {
            tasks += 2 + usize::from(case.source.is_some());
        }
    }
    for conditional in &par.conditionals {
        tasks += usize::from(conditional.if_true.is_some())
            + usize::from(conditional.if_false.is_some());
    }
    for bundle in &par.bundles {
        tasks += usize::from(bundle.body.is_some());
    }
    for signed in &par.cost_signed_terms {
        tasks += usize::from(signed.signature.is_some()) + usize::from(signed.body.is_some());
    }
    tasks + par.cost_stacks.len()
}

/// Merges the value of a resolved dequotation into a rebuilt node, field by
/// field, as `concatenate_pars` does. Each merge moves the value's fields once,
/// so a node with many dequotations costs linear work.
fn merge_into(rebuilt: &mut Par, value: Par) {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free,
        connective_used,
        cost_signed_terms,
        cost_stacks,
    } = value;
    rebuilt.sends.extend(sends);
    rebuilt.receives.extend(receives);
    rebuilt.news.extend(news);
    rebuilt.exprs.extend(exprs);
    rebuilt.matches.extend(matches);
    rebuilt.unforgeables.extend(unforgeables);
    rebuilt.bundles.extend(bundles);
    rebuilt.connectives.extend(connectives);
    rebuilt.conditionals.extend(conditionals);
    rebuilt.cost_signed_terms.extend(cost_signed_terms);
    rebuilt.cost_stacks.extend(cost_stacks);
    if !locally_free.is_empty() {
        rebuilt.locally_free = union(std::mem::take(&mut rebuilt.locally_free), locally_free);
    }
    rebuilt.connective_used |= connective_used;
}

/// Drops rebuilt terms without recursion. The derived drop of prost recurses
/// once per nesting level, so each node first moves the children that the
/// resolver walks to a heap work list. The resolver calls it on its partial
/// results when it stops with an error.
pub(crate) fn dismantle(pars: Vec<Par>) {
    let mut pending = pars;
    while let Some(mut par) = pending.pop() {
        for term in par.cost_signed_terms.drain(..) {
            pending.extend(term.body);
        }
        for mut send in par.sends.drain(..) {
            pending.append(&mut send.data);
        }
        for mut receive in par.receives.drain(..) {
            pending.extend(receive.body.take());
        }
        for mut new in par.news.drain(..) {
            pending.extend(new.p.take());
        }
        for mut matched in par.matches.drain(..) {
            for case in matched.cases.drain(..) {
                pending.extend(case.source);
            }
        }
        for mut conditional in par.conditionals.drain(..) {
            pending.extend(conditional.if_true.take());
            pending.extend(conditional.if_false.take());
        }
        for mut bundle in par.bundles.drain(..) {
            pending.extend(bundle.body.take());
        }
    }
}

/// G1-2 (DR-67): the funding resolver as a specialized stack-safe machine.
///
/// The machine walks the positions that the funding analysis reads: every
/// process position, send data, and the bodies of receives, `new`, `match`,
/// `if`, bundles and signed terms. It keeps a stack of binders. A `new` in a
/// resolving position binds the names that the reducer allocates. Every other
/// binder is a hole, so its references stay bound levels and the analyzer
/// treats them as dynamic authority. A receive body is not a resolving
/// position (DR-121): the reducer runs it with randomness that depends on the
/// matched datum. The machine copies the fields that it does not walk and
/// rewrites only cost signatures.
struct FundingResolver<'u, 'h> {
    urn_map: &'u HashMap<String, Par>,
    host: Option<&'h HostWorkBudget>,
    substitute: Substitute,
    /// The binders in scope, outermost first. `Some` holds the value that the
    /// reducer binds, and `None` is a hole.
    bindings: Vec<Option<Par>>,
    /// The number of enclosing `new` terms whose names the resolver allocated.
    /// The recursive resolver substituted every signature inside the body of
    /// such a `new`, and this resolver does the same.
    resolved_news: usize,
    /// Rebuilt children that wait for the rebuild of their parent.
    pars: Vec<Par>,
    /// Resolved signatures that wait for the rebuild of their node.
    signatures: Vec<CostSignature>,
}

impl<'u, 'h> FundingResolver<'u, 'h> {
    fn new(urn_map: &'u HashMap<String, Par>, host: Option<&'h HostWorkBudget>) -> Self {
        Self {
            urn_map,
            host,
            substitute: Substitute {
                metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
            },
            bindings: Vec::new(),
            resolved_news: 0,
            pars: Vec::new(),
            signatures: Vec::new(),
        }
    }

    fn charge(&self, dimension: HostWorkDimension, units: usize) -> Result<(), InterpreterError> {
        let Some(host) = self.host else {
            return Ok(());
        };
        let units = u64::try_from(units).map_err(|_| InterpreterError::HostWorkRejected)?;
        host.reserve(dimension, HostWorkUnits::new(units))?;
        Ok(())
    }

    fn backing(&self) -> HostBacking<'h> { HostBacking(self.host) }

    /// Copies a field that the resolver does not walk. The charge prepays the
    /// copy and its release, as the reducer does for its copies.
    fn copy<T: Clone + CloneBacking>(&self, value: &T) -> Result<T, InterpreterError> {
        if self.host.is_some() {
            clone_backing::reserve_blocks_copy_and_cleanup(value, &self.backing())
                .map_err(host_rejected)?;
        }
        Ok(value.clone())
    }

    fn run(mut self, program: &Par, rand: Blake2b512Random) -> Result<Par, InterpreterError> {
        let mut work = vec![Task::Visit(program, Some(rand))];
        while let Some(task) = work.pop() {
            if let Err(error) = self.step(task, &mut work) {
                dismantle(std::mem::take(&mut self.pars));
                return Err(error);
            }
        }
        match (self.pars.pop(), self.pars.is_empty()) {
            (Some(resolved), true) if self.signatures.is_empty() && self.bindings.is_empty() => {
                Ok(resolved)
            }
            (resolved, _) => {
                dismantle(resolved.into_iter().chain(self.pars).collect());
                Err(InterpreterError::BugFoundError(
                    "funding resolver finished with unbalanced stacks".to_string(),
                ))
            }
        }
    }

    fn step<'a>(
        &mut self,
        task: Task<'a>,
        work: &mut Vec<Task<'a>>,
    ) -> Result<(), InterpreterError> {
        self.charge(HostWorkDimension::StructuralItems, 1)?;
        match task {
            Task::Visit(par, position) => self.visit(par, position, work),
            Task::BindSignatures(binds) => {
                for bind in binds {
                    if let Some(signature) = bind.cost_signature.as_ref() {
                        let resolved = self.resolve_signature(signature)?;
                        self.signatures.push(resolved);
                    }
                }
                Ok(())
            }
            Task::Signature(signature) => {
                let resolved = self.resolve_signature(signature)?;
                self.signatures.push(resolved);
                Ok(())
            }
            Task::Cells(stack) => {
                self.signatures.reserve(stack.cells.len());
                for cell in &stack.cells {
                    let resolved = self.resolve_signature(cell)?;
                    self.signatures.push(resolved);
                }
                Ok(())
            }
            Task::Holes(count) => self.push_holes(count),
            Task::Enter(new, position) => self.enter(new, position, work),
            Task::Pop { count, resolved } => {
                let keep = self.bindings.len().checked_sub(count).ok_or_else(|| {
                    InterpreterError::BugFoundError(
                        "funding resolver popped more binders than it pushed".to_string(),
                    )
                })?;
                self.bindings.truncate(keep);
                if resolved {
                    self.resolved_news = self.resolved_news.checked_sub(1).ok_or_else(|| {
                        InterpreterError::BugFoundError(
                            "funding resolver closed a scope that it did not open".to_string(),
                        )
                    })?;
                }
                Ok(())
            }
            Task::Build(par) => self.build(par),
        }
    }

    /// Schedules the children of a node in the reducer's term order: sends,
    /// receives, `new`, `match`, `if`, bundles, dequotations, signed terms and
    /// token stacks. In a resolving position each walked term gets the split
    /// of the node's randomness that the reducer gives it
    /// (`util::evaluation_random`).
    fn visit<'a>(
        &mut self,
        par: &'a Par,
        position: Position,
        work: &mut Vec<Task<'a>>,
    ) -> Result<(), InterpreterError> {
        let mut ordered = Vec::with_capacity(scheduled_tasks(par));
        self.charge(HostWorkDimension::StructuralItems, ordered.capacity())?;
        let terms = match position.as_ref() {
            Some(_) => {
                self.charge(HostWorkDimension::StructuralItems, par.exprs.len())?;
                let count = term_count(par);
                if count > i16::MAX as usize {
                    return Err(InterpreterError::ReduceError(format!(
                        "The number of terms in the Par is {}, which exceeds the limit of {}",
                        count,
                        i16::MAX
                    )));
                }
                count
            }
            None => 0,
        };
        let split = |index: usize| -> Result<Position, InterpreterError> {
            match position.as_ref() {
                Some(rand) => evaluation_random(rand, index, terms).map(Some),
                None => Ok(None),
            }
        };

        let mut index = par.sends.len();
        for send in &par.sends {
            for datum in &send.data {
                ordered.push(Task::Visit(datum, None));
            }
        }
        for receive in &par.receives {
            // Changed by G1-3 (DR-121): the reducer runs a receive body with the
            // merge of the continuation's randomness and the randomness of each
            // matched datum (`dispatch.rs`). The names that the body creates
            // depend on the datum, so the resolver leaves them unresolved, and
            // the analyzer treats their bound levels as dynamic authority.
            // let body_position = split(index)?;
            let body_position: Position = None;
            let binders = usize::try_from(receive.bind_count).unwrap_or(0);
            ordered.push(Task::BindSignatures(&receive.binds));
            ordered.push(Task::Holes(binders));
            if let Some(body) = receive.body.as_ref() {
                ordered.push(Task::Visit(body, body_position));
            }
            ordered.push(Task::Pop {
                count: binders,
                resolved: false,
            });
            index += 1;
        }
        for new in &par.news {
            ordered.push(Task::Enter(new, split(index)?));
            index += 1;
        }
        for matched in &par.matches {
            let case_position = split(index)?;
            for case in &matched.cases {
                let binders = usize::try_from(case.free_count).unwrap_or(0);
                ordered.push(Task::Holes(binders));
                if let Some(source) = case.source.as_ref() {
                    ordered.push(Task::Visit(source, case_position.clone()));
                }
                ordered.push(Task::Pop {
                    count: binders,
                    resolved: false,
                });
            }
            index += 1;
        }
        for conditional in &par.conditionals {
            let branch_position = split(index)?;
            if let Some(if_true) = conditional.if_true.as_ref() {
                ordered.push(Task::Visit(if_true, branch_position.clone()));
            }
            if let Some(if_false) = conditional.if_false.as_ref() {
                ordered.push(Task::Visit(if_false, branch_position));
            }
            index += 1;
        }
        for bundle in &par.bundles {
            let body_position = split(index)?;
            if let Some(body) = bundle.body.as_ref() {
                ordered.push(Task::Visit(body, body_position));
            }
            index += 1;
        }
        index += par
            .exprs
            .iter()
            .filter(|expr| {
                matches!(
                    expr.expr_instance,
                    Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
                )
            })
            .count();
        for signed in &par.cost_signed_terms {
            let body_position = split(index)?;
            if let Some(signature) = signed.signature.as_ref() {
                ordered.push(Task::Signature(signature));
            }
            if let Some(body) = signed.body.as_ref() {
                ordered.push(Task::Visit(body, body_position));
            }
            index += 1;
        }
        for stack in &par.cost_stacks {
            ordered.push(Task::Cells(stack));
        }
        index += par.cost_stacks.len();
        if position.is_some() && index != terms {
            return Err(InterpreterError::BugFoundError(
                "funding resolver term schedule disagrees with reducer schedule".to_string(),
            ));
        }
        ordered.push(Task::Build(par));
        work.extend(ordered.into_iter().rev());
        Ok(())
    }

    fn push_holes(&mut self, count: usize) -> Result<(), InterpreterError> {
        self.charge(HostWorkDimension::SubstitutionBindings, count)?;
        self.bindings.reserve(count);
        self.bindings
            .extend(std::iter::repeat_with(|| None).take(count));
        Ok(())
    }

    /// Binds the names of a `new`. In a resolving position these are the
    /// values that the reducer binds (`util::allocate_new_bindings`), and the
    /// body continues with the randomness that the allocation leaves.
    fn enter<'a>(
        &mut self,
        new: &'a New,
        position: Position,
        work: &mut Vec<Task<'a>>,
    ) -> Result<(), InterpreterError> {
        match position {
            Some(mut rand) => {
                let values = self.new_binding_values(new, &mut rand)?;
                let body = new.p.as_ref().ok_or_else(|| {
                    InterpreterError::UndefinedRequiredProtobufFieldError("New.p".to_string())
                })?;
                let count = values.len();
                self.bindings.reserve(count);
                self.bindings.extend(values.into_iter().map(Some));
                self.resolved_news += 1;
                work.push(Task::Pop {
                    count,
                    resolved: true,
                });
                work.push(Task::Visit(body, Some(rand)));
            }
            None => {
                let count = usize::try_from(new.bind_count).unwrap_or(0);
                self.push_holes(count)?;
                work.push(Task::Pop {
                    count,
                    resolved: false,
                });
                if let Some(body) = new.p.as_ref() {
                    work.push(Task::Visit(body, None));
                }
            }
        }
        Ok(())
    }

    /// The values that the reducer binds for a `new`, in the order of
    /// `util::allocate_new_bindings`: the fresh private names, then the URN
    /// values. That function copies its whole environment at each binding, so
    /// a `new` with many names costs quadratic work there. This helper builds
    /// the same values, with the same errors, in linear work.
    fn new_binding_values(
        &self,
        new: &New,
        rand: &mut Blake2b512Random,
    ) -> Result<Vec<Par>, InterpreterError> {
        let bind_count = usize::try_from(new.bind_count).map_err(|_| {
            InterpreterError::ReduceError("new binding count cannot be negative".to_string())
        })?;
        let simple_count = bind_count.checked_sub(new.uri.len()).ok_or_else(|| {
            InterpreterError::ReduceError(
                "new URI binding count exceeds the total binding count".to_string(),
            )
        })?;
        self.charge(HostWorkDimension::SubstitutionBindings, bind_count)?;
        let mut values = Vec::with_capacity(bind_count);
        for _ in 0..simple_count {
            let name = Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: rand.next().into_iter().map(|byte| byte as u8).collect(),
                })),
            }]);
            if self.host.is_some() {
                clone_backing::reserve_blocks_copy_and_cleanup(&name, &self.backing())
                    .map_err(host_rejected)?;
            }
            values.push(name);
        }
        for urn in &new.uri {
            let value = match self.urn_map.get(urn) {
                Some(value) => self.copy(value)?,
                None => match new.injections.get(urn) {
                    Some(value) => {
                        if let Some(unforgeable) = RhoUnforgeable::unapply(value) {
                            let instance = unforgeable.unf_instance.ok_or_else(|| {
                                InterpreterError::BugFoundError(
                                    "unf_instance field is None".to_string(),
                                )
                            })?;
                            self.copy(&Par::default().with_unforgeables(vec![GUnforgeable {
                                unf_instance: Some(instance),
                            }]))?
                        } else if let Some(expression) = RhoExpression::unapply(value) {
                            let instance = expression.expr_instance.ok_or_else(|| {
                                InterpreterError::BugFoundError(
                                    "expr_instance field is None".to_string(),
                                )
                            })?;
                            self.copy(&Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(instance),
                            }]))?
                        } else {
                            return Err(InterpreterError::BugFoundError(
                                "invalid injection".to_string(),
                            ));
                        }
                    }
                    None => {
                        return Err(InterpreterError::BugFoundError(format!(
                            "No value set for {}. This is a bug in the normalizer or on the path from it.",
                            urn
                        )))
                    }
                },
            };
            values.push(value);
        }
        Ok(values)
    }

    /// Resolves one cost signature in the current scope. Inside the body of a
    /// `new` whose names the resolver allocated, the recursive resolver passed
    /// every signature through `substitute_cost_signature`, which also sorts
    /// it. Elsewhere it left the signature unchanged. This resolver does the
    /// same, with an environment that holds only the bindings that the
    /// signature can reference, so a lookup does not depend on the depth.
    fn resolve_signature(
        &self,
        signature: &CostSignature,
    ) -> Result<CostSignature, InterpreterError> {
        if self.resolved_news == 0 {
            return self.copy(signature);
        }
        let env = self.signature_env(signature)?;
        let owned = self.copy(signature)?;
        match self.host {
            Some(_) => {
                self.substitute
                    .substitute_cost_signature_metered(owned, 0, &env, &self.backing())
            }
            None => self.substitute.substitute_cost_signature(owned, 0, &env),
        }
    }

    /// The environment of one signature: the resolved bindings of its bound
    /// levels and of the free variables of its quoted processes. A quoted
    /// process lists its free variables in `locally_free`, one byte per de
    /// Bruijn index. The environment keeps the scope's depth, so an index
    /// selects the same binder as in the reducer's environment.
    fn signature_env(&self, signature: &CostSignature) -> Result<Env<Par>, InterpreterError> {
        let depth = self.bindings.len();
        let mut referenced = Vec::new();
        let mut pending = vec![signature];
        while let Some(current) = pending.pop() {
            self.charge(HostWorkDimension::StructuralItems, 1)?;
            match current.value.as_ref() {
                Some(CostSignatureValue::BoundLevel(level)) => {
                    if let Ok(level) = usize::try_from(*level) {
                        referenced.push(level);
                    }
                }
                Some(CostSignatureValue::Compound(compound)) => {
                    pending.extend(compound.elements.iter());
                }
                Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                    self.charge(HostWorkDimension::StructuralItems, par.locally_free.len())?;
                    referenced.extend(
                        par.locally_free
                            .iter()
                            .enumerate()
                            .filter(|(_, free)| **free != 0)
                            .map(|(level, _)| level),
                    );
                }
                _ => {}
            }
        }
        let level = i32::try_from(depth).map_err(|_| InterpreterError::HostWorkRejected)?;
        let mut env = Env {
            env_map: HashMap::new(),
            level,
            shift: 0,
        };
        if self.host.is_some() {
            let (operations, backing) = hash_backing::<i32, Par>(referenced.len())
                .ok_or(InterpreterError::HostWorkRejected)?;
            self.backing()
                .reserve(
                    operations,
                    referenced.len().saturating_mul(std::mem::size_of::<i32>()),
                    backing,
                )
                .map_err(host_rejected)?;
        }
        env.env_map.reserve(referenced.len());
        for level in referenced {
            let Some(binder) = depth.checked_sub(level + 1) else {
                continue;
            };
            if let Some(Some(value)) = self.bindings.get(binder) {
                let position =
                    i32::try_from(binder).map_err(|_| InterpreterError::HostWorkRejected)?;
                env.env_map.insert(position, self.copy(value)?);
            }
        }
        Ok(env)
    }

    /// Rebuilds a node: a copy of the fields that the resolver does not walk,
    /// with the rebuilt children and the resolved signatures in the order in
    /// which the visit scheduled them.
    fn build(&mut self, par: &Par) -> Result<(), InterpreterError> {
        let (child_count, signature_count) = rebuilt_parts(par);
        self.charge(
            HostWorkDimension::StructuralItems,
            child_count + signature_count,
        )?;
        let child_start = self.pars.len().checked_sub(child_count).ok_or_else(|| {
            InterpreterError::BugFoundError("funding resolver lost a rebuilt child".to_string())
        })?;
        let signature_start = self
            .signatures
            .len()
            .checked_sub(signature_count)
            .ok_or_else(|| {
                InterpreterError::BugFoundError(
                    "funding resolver lost a resolved signature".to_string(),
                )
            })?;
        let mut children = self.pars.split_off(child_start).into_iter();
        let mut signatures = self.signatures.split_off(signature_start).into_iter();
        let missing = || {
            InterpreterError::BugFoundError(
                "funding resolver rebuilt a node from too few parts".to_string(),
            )
        };

        let mut sends = Vec::with_capacity(par.sends.len());
        for send in &par.sends {
            let mut data = Vec::with_capacity(send.data.len());
            for _ in &send.data {
                data.push(children.next().ok_or_else(missing)?);
            }
            sends.push(Send {
                chan: self.copy(&send.chan)?,
                data,
                persistent: send.persistent,
                locally_free: self.copy(&send.locally_free)?,
                connective_used: send.connective_used,
            });
        }
        let mut receives = Vec::with_capacity(par.receives.len());
        for receive in &par.receives {
            let mut binds = Vec::with_capacity(receive.binds.len());
            for bind in &receive.binds {
                let cost_signature = match bind.cost_signature {
                    Some(_) => Some(signatures.next().ok_or_else(missing)?),
                    None => None,
                };
                binds.push(ReceiveBind {
                    patterns: self.copy(&bind.patterns)?,
                    source: self.copy(&bind.source)?,
                    remainder: bind.remainder,
                    free_count: bind.free_count,
                    cost_signature,
                });
            }
            let body = match receive.body {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            receives.push(models::rhoapi::Receive {
                binds,
                body,
                persistent: receive.persistent,
                peek: receive.peek,
                bind_count: receive.bind_count,
                locally_free: self.copy(&receive.locally_free)?,
                connective_used: receive.connective_used,
                condition: self.copy(&receive.condition)?,
            });
        }
        let mut news = Vec::with_capacity(par.news.len());
        for new in &par.news {
            let p = match new.p {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            news.push(New {
                bind_count: new.bind_count,
                p,
                uri: self.copy(&new.uri)?,
                injections: self.copy(&new.injections)?,
                locally_free: self.copy(&new.locally_free)?,
            });
        }
        let mut matches = Vec::with_capacity(par.matches.len());
        for matched in &par.matches {
            let mut cases = Vec::with_capacity(matched.cases.len());
            for case in &matched.cases {
                let source = match case.source {
                    Some(_) => Some(children.next().ok_or_else(missing)?),
                    None => None,
                };
                cases.push(MatchCase {
                    pattern: self.copy(&case.pattern)?,
                    source,
                    free_count: case.free_count,
                    guard: self.copy(&case.guard)?,
                });
            }
            matches.push(Match {
                target: self.copy(&matched.target)?,
                cases,
                locally_free: self.copy(&matched.locally_free)?,
                connective_used: matched.connective_used,
            });
        }
        let mut conditionals = Vec::with_capacity(par.conditionals.len());
        for conditional in &par.conditionals {
            let if_true = match conditional.if_true {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            let if_false = match conditional.if_false {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            conditionals.push(If {
                condition: self.copy(&conditional.condition)?,
                if_true,
                if_false,
                locally_free: self.copy(&conditional.locally_free)?,
                connective_used: conditional.connective_used,
            });
        }
        let mut bundles = Vec::with_capacity(par.bundles.len());
        for bundle in &par.bundles {
            let body = match bundle.body {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            bundles.push(Bundle {
                body,
                write_flag: bundle.write_flag,
                read_flag: bundle.read_flag,
            });
        }
        let mut cost_signed_terms = Vec::with_capacity(par.cost_signed_terms.len());
        for signed in &par.cost_signed_terms {
            let signature = match signed.signature {
                Some(_) => Some(signatures.next().ok_or_else(missing)?),
                None => None,
            };
            let body = match signed.body {
                Some(_) => Some(children.next().ok_or_else(missing)?),
                None => None,
            };
            cost_signed_terms.push(CostSignedTerm { body, signature });
        }
        let mut cost_stacks = Vec::with_capacity(par.cost_stacks.len());
        for stack in &par.cost_stacks {
            let mut cells = Vec::with_capacity(stack.cells.len());
            for _ in &stack.cells {
                cells.push(signatures.next().ok_or_else(missing)?);
            }
            cost_stacks.push(CostStack { cells });
        }
        if children.next().is_some() || signatures.next().is_some() {
            return Err(InterpreterError::BugFoundError(
                "funding resolver rebuilt a node from too many parts".to_string(),
            ));
        }
        let (exprs, dequoted) = self.resolve_dequotations(&par.exprs)?;
        let mut rebuilt = Par {
            sends,
            receives,
            news,
            exprs,
            matches,
            unforgeables: self.copy(&par.unforgeables)?,
            bundles,
            connectives: self.copy(&par.connectives)?,
            conditionals,
            locally_free: self.copy(&par.locally_free)?,
            connective_used: par.connective_used,
            cost_signed_terms,
            cost_stacks,
        };
        for value in dequoted {
            merge_into(&mut rebuilt, value);
        }
        self.pars.push(rebuilt);
        Ok(())
    }

    /// The expressions of a node, with each dequotation `*x` of a resolved
    /// name taken out. The recursive resolver substituted such a dequotation
    /// with the name's value and merged the value into the node, as the
    /// reducer evaluates it. The caller merges the returned values in the same
    /// way. Every other expression is copied unchanged.
    fn resolve_dequotations(
        &self,
        exprs: &[Expr],
    ) -> Result<(Vec<Expr>, Vec<Par>), InterpreterError> {
        self.charge(HostWorkDimension::StructuralItems, exprs.len())?;
        let depth = self.bindings.len();
        let mut kept = Vec::with_capacity(exprs.len());
        let mut dequoted = Vec::new();
        for expr in exprs {
            let resolved = match expr.expr_instance.as_ref() {
                Some(ExprInstance::EVarBody(EVar {
                    v:
                        Some(Var {
                            var_instance: Some(VarInstance::BoundVar(level)),
                        }),
                })) => usize::try_from(*level)
                    .ok()
                    .and_then(|level| depth.checked_sub(level + 1))
                    .and_then(|binder| self.bindings.get(binder))
                    .and_then(Option::as_ref),
                _ => None,
            };
            match resolved {
                Some(value) => dequoted.push(self.copy(value)?),
                None => kept.push(self.copy(expr)?),
            }
        }
        Ok((kept, dequoted))
    }
}

// Disabled in production by G1-2 (DR-67 implementation note): the recursive
// resolver substituted the whole body of each `new` again, copied each level
// to count its terms, and recursed once per nesting level. The machine above
// replaced it. It stays, unchanged, as the oracle of the parity tests.
#[cfg(test)]
fn resolve_receive(
    mut receive: Receive,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    substitute: &Substitute,
    resolve_receive_bodies: bool,
) -> Result<Receive, InterpreterError> {
    if let Some(body) = receive.body.take() {
        receive.body = Some(resolve_par(
            body,
            rand,
            urn_map,
            substitute,
            resolve_receive_bodies,
        )?);
    }
    Ok(receive)
}

// Disabled in production by G1-2 (DR-67 implementation note): see
// `resolve_receive`.
#[cfg(test)]
fn resolve_par(
    mut par: Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    substitute: &Substitute,
    resolve_receive_bodies: bool,
) -> Result<Par, InterpreterError> {
    let term_count = evaluation_terms(&par).len();
    if term_count > i16::MAX as usize {
        return Err(InterpreterError::ReduceError(format!(
            "The number of terms in the Par is {}, which exceeds the limit of {}",
            term_count,
            i16::MAX
        )));
    }
    let mut index = par.sends.len();

    for receive in &mut par.receives {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        // Changed by G1-3 (DR-121): the oracle models HEAD's rule and the rule
        // of DR-121, which leaves the names of a receive body unresolved.
        // *receive = resolve_receive(receive.clone(), term_rand, urn_map, substitute)?;
        if resolve_receive_bodies {
            *receive = resolve_receive(
                receive.clone(),
                term_rand,
                urn_map,
                substitute,
                resolve_receive_bodies,
            )?;
        }
        index += 1;
    }

    for new in &mut par.news {
        let mut term_rand = evaluation_random(&rand, index, term_count)?;
        let env = allocate_new_bindings(new, &Env::new(), &mut term_rand, urn_map)?;
        let body = new.p.take().ok_or_else(|| {
            InterpreterError::UndefinedRequiredProtobufFieldError("New.p".to_string())
        })?;
        let substituted = substitute.substitute_no_sort(body, 0, &env)?;
        new.p = Some(resolve_par(
            substituted,
            term_rand,
            urn_map,
            substitute,
            resolve_receive_bodies,
        )?);
        index += 1;
    }

    for mat in &mut par.matches {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        for case in &mut mat.cases {
            if let Some(source) = case.source.take() {
                case.source = Some(resolve_par(
                    source,
                    term_rand.clone(),
                    urn_map,
                    substitute,
                    resolve_receive_bodies,
                )?);
            }
        }
        index += 1;
    }

    for conditional in &mut par.conditionals {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(if_true) = conditional.if_true.take() {
            conditional.if_true = Some(resolve_par(
                if_true,
                term_rand.clone(),
                urn_map,
                substitute,
                resolve_receive_bodies,
            )?);
        }
        if let Some(if_false) = conditional.if_false.take() {
            conditional.if_false = Some(resolve_par(
                if_false,
                term_rand,
                urn_map,
                substitute,
                resolve_receive_bodies,
            )?);
        }
        index += 1;
    }

    for bundle in &mut par.bundles {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(body) = bundle.body.take() {
            bundle.body = Some(resolve_par(
                body,
                term_rand,
                urn_map,
                substitute,
                resolve_receive_bodies,
            )?);
        }
        index += 1;
    }

    index += par
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.expr_instance,
                Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
            )
        })
        .count();

    for signed in &mut par.cost_signed_terms {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(body) = signed.body.take() {
            signed.body = Some(resolve_par(
                body,
                term_rand,
                urn_map,
                substitute,
                resolve_receive_bodies,
            )?);
        }
        index += 1;
    }

    index += par.cost_stacks.len();
    if index != term_count {
        return Err(InterpreterError::BugFoundError(
            "funding resolver term schedule disagrees with reducer schedule".to_string(),
        ));
    }
    Ok(par)
}

/// The recursive resolver of HEAD `17e07307f`, kept as the test oracle. With
/// `resolve_receive_bodies` set it is HEAD's resolver. Without it, it leaves
/// the names that a receive body creates unresolved (DR-121).
#[cfg(test)]
pub(crate) fn resolve_lexical_names_for_funding_recursive(
    program: &Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    resolve_receive_bodies: bool,
) -> Result<Par, InterpreterError> {
    let substitute = Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    };
    resolve_par(
        program.clone(),
        rand,
        urn_map,
        &substitute,
        resolve_receive_bodies,
    )
}

#[cfg(test)]
mod stack_safety_tests;

#[cfg(test)]
mod native_names_tests;

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{GPrivate, GUnforgeable};

    use super::*;
    use crate::rust::interpreter::accounting::authority::cost_signature_to_sig;
    use crate::rust::interpreter::accounting::delta_sigma::static_authority_plan;
    use crate::rust::interpreter::accounting::Sig;
    use crate::rust::interpreter::compiler::compiler::Compiler;

    fn private_name(id: Vec<u8>) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    #[test]
    fn resolves_new_bound_cost_authorities_to_the_runtime_name() {
        let program = Compiler::source_to_adt(
            r#"new slot in { {% for(_ <- @"x"){ Nil } %}[ slot ] | slot :: slot :: () | @"x"!(0) }"#,
        )
        .unwrap();
        let rand = Blake2b512Random::create_from_bytes(b"lexical slot");
        let mut expected_rand = rand.clone();
        let expected = private_name(
            expected_rand
                .next()
                .into_iter()
                .map(|byte| byte as u8)
                .collect(),
        );
        let resolved = resolve_lexical_names_for_funding(&program, rand, &HashMap::new()).unwrap();
        let body = resolved.news[0].p.as_ref().unwrap();
        assert_eq!(
            body.cost_signed_terms[0].signature.as_ref().unwrap().value,
            Some(Value::Name(expected.clone()))
        );
        assert_eq!(
            body.cost_stacks[0].cells[0].value,
            Some(Value::Name(expected.clone()))
        );
        assert_eq!(
            body.cost_stacks[0].cells[1].value,
            Some(Value::Name(expected))
        );
    }

    #[test]
    fn resolved_slot_supply_reduces_external_reservation_without_erasing_demand() {
        let program = Compiler::source_to_adt(
            r#"new slot in { {% for(_ <- @"x"){ new y in { y!(0) | for(@0 <- y){ Nil } } } %}[ a -o slot ] | a :: () | slot :: slot :: () | @"slot-registry"!(*slot) }"#,
        )
        .unwrap();
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"funding plan"),
            &HashMap::new(),
        )
        .unwrap();
        let deploy = Sig::Ground(b"deployer".to_vec());
        let plan = static_authority_plan(&resolved, &deploy).unwrap();
        let body = resolved.news[0].p.as_ref().unwrap();
        let slot_stack = body
            .cost_stacks
            .iter()
            .find(|stack| stack.cells.len() == 2)
            .expect("two-cell slot stack");
        let slot =
            cost_signature_to_sig(slot_stack.cells.first().expect("slot stack cell")).unwrap();
        assert_eq!(
            plan.guaranteed_program_supply.get(&slot.lane_hash()),
            2,
            "plan={plan:?}, cells={:?}",
            slot_stack.cells
        );
        assert_eq!(plan.demand.get(&slot.lane_hash()), 2);
        assert_eq!(plan.external_reservation.get(&slot.lane_hash()), 0);
    }

    #[test]
    fn uri_bound_authority_uses_the_same_runtime_binding() {
        let program =
            Compiler::source_to_adt(r#"new slot(`rho:test:slot`) in { {% @"x"!(0) %}[ slot ] }"#)
                .unwrap();
        let expected = private_name(vec![9; 32]);
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"uri slot"),
            &HashMap::from([("rho:test:slot".to_string(), expected.clone())]),
        )
        .unwrap();
        let signature = resolved.news[0].p.as_ref().unwrap().cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap();
        assert_eq!(signature.value, Some(Value::Name(expected)));
    }

    #[test]
    fn receive_bound_authority_remains_dynamic() {
        let program =
            Compiler::source_to_adt(r#"for(slot <- @"slots"){ {% @"x"!(0) %}[ slot ] }"#).unwrap();
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"dynamic slot"),
            &HashMap::new(),
        )
        .unwrap();
        let signature = resolved.receives[0]
            .body
            .as_ref()
            .unwrap()
            .cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap();
        assert!(matches!(signature.value, Some(Value::BoundLevel(_))));
    }

    #[test]
    fn resolution_is_idempotent() {
        let program = Compiler::source_to_adt(r#"new slot in { {% @"x"!(0) %}[ slot ] }"#).unwrap();
        let rand = Blake2b512Random::create_from_bytes(b"idempotent slot");
        let once =
            resolve_lexical_names_for_funding(&program, rand.clone(), &HashMap::new()).unwrap();
        let twice = resolve_lexical_names_for_funding(&once, rand, &HashMap::new()).unwrap();
        assert_eq!(once, twice);
    }
}
