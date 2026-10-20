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
use models::rust::utils::union;

use super::costs::Cost;
use super::RuntimeBudget;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
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
/// analyzer reads.
///
/// G1-2 (DR-67): the resolver visits each node once, keeps the bindings in one
/// scope stack and resolves each cost signature once, so its work is linear in
/// the size of the program. Its walk uses a heap work stack, so its stack depth
/// does not grow with the nesting depth of the program.
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
    FundingResolver::new(urn_map).run(program, rand)
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
    // Changed by the G1-4a fix (review H-1): each signature task also tells
    // whether its scope is a resolving position. See `resolve_signature`.
    // /// Resolve the signatures of a receive's binds in the receive's scope.
    // BindSignatures(&'a [ReceiveBind]),
    // /// Resolve the signature of a signed term in its scope.
    // Signature(&'a CostSignature),
    // /// Resolve the cells of a token stack in its scope.
    // Cells(&'a CostStack),
    /// Resolve the signatures of a receive's binds in the receive's scope.
    BindSignatures(&'a [ReceiveBind], bool),
    /// Resolve the signature of a signed term in its scope.
    Signature(&'a CostSignature, bool),
    /// Resolve the cells of a token stack in its scope.
    Cells(&'a CostStack, bool),
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

/// Drops rebuilt terms without recursion. The resolver calls it on its partial
/// results when it stops with an error. The work is in the one complete
/// teardown, `dismantle_terms`.
pub(crate) fn dismantle(pars: Vec<Par>) { super::delta_sigma::estimate::dismantle_terms(pars); }

// Disabled by the G1-4a fix (review cleanup: keep one complete teardown).
// This walk moved only the children that the resolver walks. It missed send
// channels, receive patterns, sources, conditions and bind signatures, match
// targets, patterns and guards, `if` conditions, `new` injections,
// expressions, connectives and signatures, so a deep term in one of those
// positions still dropped recursively. `dismantle_terms` covers them all.
// pub(crate) fn dismantle(pars: Vec<Par>) {
//     let mut pending = pars;
//     while let Some(mut par) = pending.pop() {
//         for term in par.cost_signed_terms.drain(..) {
//             pending.extend(term.body);
//         }
//         for mut send in par.sends.drain(..) {
//             pending.append(&mut send.data);
//         }
//         for mut receive in par.receives.drain(..) {
//             pending.extend(receive.body.take());
//         }
//         for mut new in par.news.drain(..) {
//             pending.extend(new.p.take());
//         }
//         for mut matched in par.matches.drain(..) {
//             for case in matched.cases.drain(..) {
//                 pending.extend(case.source);
//             }
//         }
//         for mut conditional in par.conditionals.drain(..) {
//             pending.extend(conditional.if_true.take());
//             pending.extend(conditional.if_false.take());
//         }
//         for mut bundle in par.bundles.drain(..) {
//             pending.extend(bundle.body.take());
//         }
//     }
// }

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
struct FundingResolver<'u> {
    urn_map: &'u HashMap<String, Par>,
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
    /// Added by G1-4a (DR-122), test only: the address of each visited node
    /// and the number of binders in scope there.
    #[cfg(test)]
    levels: Vec<(usize, usize)>,
}

impl<'u> FundingResolver<'u> {
    fn new(urn_map: &'u HashMap<String, Par>) -> Self {
        Self {
            urn_map,
            substitute: Substitute {
                metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
            },
            bindings: Vec::new(),
            resolved_news: 0,
            pars: Vec::new(),
            signatures: Vec::new(),
            #[cfg(test)]
            levels: Vec::new(),
        }
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
        match task {
            Task::Visit(par, position) => self.visit(par, position, work),
            Task::BindSignatures(binds, resolving) => {
                for bind in binds {
                    if let Some(signature) = bind.cost_signature.as_ref() {
                        let resolved = self.resolve_signature(signature, resolving)?;
                        self.signatures.push(resolved);
                    }
                }
                Ok(())
            }
            Task::Signature(signature, resolving) => {
                let resolved = self.resolve_signature(signature, resolving)?;
                self.signatures.push(resolved);
                Ok(())
            }
            Task::Cells(stack, resolving) => {
                self.signatures.reserve(stack.cells.len());
                for cell in &stack.cells {
                    let resolved = self.resolve_signature(cell, resolving)?;
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
        #[cfg(test)]
        self.levels
            .push((par as *const Par as usize, self.bindings.len()));
        let mut ordered = Vec::with_capacity(scheduled_tasks(par));
        let terms = match position.as_ref() {
            Some(_) => {
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
        let resolving = position.is_some();

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
            ordered.push(Task::BindSignatures(&receive.binds, resolving));
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
                ordered.push(Task::Signature(signature, resolving));
            }
            if let Some(body) = signed.body.as_ref() {
                ordered.push(Task::Visit(body, body_position));
            }
            index += 1;
        }
        for stack in &par.cost_stacks {
            ordered.push(Task::Cells(stack, resolving));
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
        let mut values = Vec::with_capacity(bind_count);
        for _ in 0..simple_count {
            let name = Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: rand.next().into_iter().map(|byte| byte as u8).collect(),
                })),
            }]);
            values.push(name);
        }
        for urn in &new.uri {
            let value = match self.urn_map.get(urn) {
                Some(value) => value.clone(),
                None => match new.injections.get(urn) {
                    Some(value) => {
                        if let Some(unforgeable) = RhoUnforgeable::unapply(value) {
                            let instance = unforgeable.unf_instance.ok_or_else(|| {
                                InterpreterError::BugFoundError(
                                    "unf_instance field is None".to_string(),
                                )
                            })?;
                            Par::default().with_unforgeables(vec![GUnforgeable {
                                unf_instance: Some(instance),
                            }])
                        } else if let Some(expression) = RhoExpression::unapply(value) {
                            let instance = expression.expr_instance.ok_or_else(|| {
                                InterpreterError::BugFoundError(
                                    "expr_instance field is None".to_string(),
                                )
                            })?;
                            Par::default().with_exprs(vec![Expr {
                                expr_instance: Some(instance),
                            }])
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
    ///
    /// Changed by the G1-4a fix (review H-1): outside a resolving position, a
    /// signature whose quoted or named process has a free level stays
    /// unchanged. The reducer substitutes a receive body or a datum before it
    /// evaluates the signatures in it. Under the binders of that term, the
    /// substitution keeps the first `shift` entries of each `locally_free`.
    /// This substitution runs at shift 0 and clears them, so the stored bytes
    /// would differ from the reducer's. The unchanged signature still holds
    /// its variable, and the analysis treats its lane as dynamic.
    fn resolve_signature(
        &self,
        signature: &CostSignature,
        resolving: bool,
    ) -> Result<CostSignature, InterpreterError> {
        if self.resolved_news == 0 {
            return Ok(signature.clone());
        }
        if !resolving && self.quotes_a_free_level(signature)? {
            return Ok(signature.clone());
        }
        let env = self.signature_env(signature)?;
        self.substitute
            .substitute_cost_signature(signature.clone(), 0, &env)
    }

    /// Added by the G1-4a fix (review H-1): whether a quoted or named process
    /// of the signature has a free level. The scan uses a heap work list and
    /// charges one structural item for each signature node and each scanned
    /// `locally_free` entry, as `signature_env` does.
    fn quotes_a_free_level(&self, signature: &CostSignature) -> Result<bool, InterpreterError> {
        let mut pending = vec![signature];
        while let Some(current) = pending.pop() {
            match current.value.as_ref() {
                Some(CostSignatureValue::Compound(compound)) => {
                    pending.extend(compound.elements.iter());
                }
                Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                    if par.locally_free.iter().any(|free| *free != 0) {
                        return Ok(true);
                    }
                }
                Some(
                    CostSignatureValue::Ground(_)
                    | CostSignatureValue::BoundLevel(_)
                    | CostSignatureValue::Unit(_),
                )
                | None => {}
            }
        }
        Ok(false)
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
        let level = i32::try_from(depth).map_err(|_| {
            InterpreterError::BugFoundError(
                "funding resolver scope depth does not fit in i32".to_string(),
            )
        })?;
        let mut env = Env {
            env_map: HashMap::new(),
            level,
            shift: 0,
        };
        env.env_map.reserve(referenced.len());
        for level in referenced {
            let Some(binder) = depth.checked_sub(level + 1) else {
                continue;
            };
            if let Some(Some(value)) = self.bindings.get(binder) {
                let position = i32::try_from(binder).map_err(|_| {
                    InterpreterError::BugFoundError(
                        "funding resolver binder position does not fit in i32".to_string(),
                    )
                })?;
                env.env_map.insert(position, value.clone());
            }
        }
        Ok(env)
    }

    /// Rebuilds a node: a copy of the fields that the resolver does not walk,
    /// with the rebuilt children and the resolved signatures in the order in
    /// which the visit scheduled them.
    fn build(&mut self, par: &Par) -> Result<(), InterpreterError> {
        let (child_count, signature_count) = rebuilt_parts(par);
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
                chan: send.chan.clone(),
                data,
                persistent: send.persistent,
                locally_free: send.locally_free.clone(),
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
                    patterns: bind.patterns.clone(),
                    source: bind.source.clone(),
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
                locally_free: receive.locally_free.clone(),
                connective_used: receive.connective_used,
                condition: receive.condition.clone(),
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
                uri: new.uri.clone(),
                injections: new.injections.clone(),
                locally_free: new.locally_free.clone(),
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
                    pattern: case.pattern.clone(),
                    source,
                    free_count: case.free_count,
                    guard: case.guard.clone(),
                });
            }
            matches.push(Match {
                target: matched.target.clone(),
                cases,
                locally_free: matched.locally_free.clone(),
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
                condition: conditional.condition.clone(),
                if_true,
                if_false,
                locally_free: conditional.locally_free.clone(),
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
            unforgeables: par.unforgeables.clone(),
            bundles,
            connectives: par.connectives.clone(),
            conditionals,
            locally_free: par.locally_free.clone(),
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
                Some(value) => dequoted.push(value.clone()),
                None => kept.push(expr.clone()),
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

/// Added by G1-4a (DR-122), test only: the resolver's binder count at each node
/// that it visits, keyed by the node's address in `program`. The level test of
/// the estimate compares it with the binder kinds of pass 1.
#[cfg(test)]
pub(crate) fn resolver_levels(
    program: &Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
) -> Result<Vec<(usize, usize)>, InterpreterError> {
    let mut resolver = FundingResolver::new(urn_map);
    let mut work = vec![Task::Visit(program, Some(rand))];
    while let Some(task) = work.pop() {
        if let Err(error) = resolver.step(task, &mut work) {
            dismantle(std::mem::take(&mut resolver.pars));
            return Err(error);
        }
    }
    dismantle(std::mem::take(&mut resolver.pars));
    Ok(resolver.levels)
}

/// The recursive resolver of HEAD `17e07307f`, kept as the test oracle. With
/// `resolve_receive_bodies` set it is HEAD's resolver. Without it, it leaves
/// the names that a receive body creates unresolved (DR-121). Changed by the
/// second G1-4a fix (re-review LOW 1): without it, the oracle also follows
/// the rule of `resolve_signature` for a quote with a free level outside a
/// resolving position.
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
    // resolve_par(
    //     program.clone(),
    //     rand,
    //     urn_map,
    //     &substitute,
    //     resolve_receive_bodies,
    // )
    let mut resolved = resolve_par(
        program.clone(),
        rand,
        urn_map,
        &substitute,
        resolve_receive_bodies,
    )?;
    if !resolve_receive_bodies {
        keep_unresolvable_quotes(program, &mut resolved, true);
    }
    Ok(resolved)
}

/// Added by the second G1-4a fix (re-review LOW 1): the oracle form of the
/// rule of `resolve_signature`. Outside a resolving position, a receive body
/// or a datum, a signature whose quoted or named process has a free level
/// keeps the form of the program. The recursion visits `program` and
/// `resolved` in step, because the substitution of resolved names keeps the
/// shape of every walked position.
#[cfg(test)]
fn keep_unresolvable_quotes(program: &Par, resolved: &mut Par, resolving: bool) {
    fn quotes_a_free_level(signature: &CostSignature) -> bool {
        match signature.value.as_ref() {
            Some(CostSignatureValue::Compound(compound)) => {
                compound.elements.iter().any(quotes_a_free_level)
            }
            Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                par.locally_free.iter().any(|free| *free != 0)
            }
            Some(
                CostSignatureValue::Ground(_)
                | CostSignatureValue::BoundLevel(_)
                | CostSignatureValue::Unit(_),
            )
            | None => false,
        }
    }
    let keep = |original: &CostSignature, result: &mut CostSignature| {
        if !resolving && quotes_a_free_level(original) {
            *result = original.clone();
        }
    };
    for (send, target) in program.sends.iter().zip(resolved.sends.iter_mut()) {
        for (datum, datum_target) in send.data.iter().zip(target.data.iter_mut()) {
            keep_unresolvable_quotes(datum, datum_target, false);
        }
    }
    for (receive, target) in program.receives.iter().zip(resolved.receives.iter_mut()) {
        for (bind, bind_target) in receive.binds.iter().zip(target.binds.iter_mut()) {
            if let (Some(original), Some(result)) = (
                bind.cost_signature.as_ref(),
                bind_target.cost_signature.as_mut(),
            ) {
                keep(original, result);
            }
        }
        if let (Some(body), Some(body_target)) = (receive.body.as_ref(), target.body.as_mut()) {
            keep_unresolvable_quotes(body, body_target, false);
        }
    }
    for (new, target) in program.news.iter().zip(resolved.news.iter_mut()) {
        if let (Some(body), Some(body_target)) = (new.p.as_ref(), target.p.as_mut()) {
            keep_unresolvable_quotes(body, body_target, resolving);
        }
    }
    for (matched, target) in program.matches.iter().zip(resolved.matches.iter_mut()) {
        for (case, case_target) in matched.cases.iter().zip(target.cases.iter_mut()) {
            if let (Some(source), Some(source_target)) =
                (case.source.as_ref(), case_target.source.as_mut())
            {
                keep_unresolvable_quotes(source, source_target, resolving);
            }
        }
    }
    for (conditional, target) in program
        .conditionals
        .iter()
        .zip(resolved.conditionals.iter_mut())
    {
        if let (Some(branch), Some(branch_target)) =
            (conditional.if_true.as_ref(), target.if_true.as_mut())
        {
            keep_unresolvable_quotes(branch, branch_target, resolving);
        }
        if let (Some(branch), Some(branch_target)) =
            (conditional.if_false.as_ref(), target.if_false.as_mut())
        {
            keep_unresolvable_quotes(branch, branch_target, resolving);
        }
    }
    for (bundle, target) in program.bundles.iter().zip(resolved.bundles.iter_mut()) {
        if let (Some(body), Some(body_target)) = (bundle.body.as_ref(), target.body.as_mut()) {
            keep_unresolvable_quotes(body, body_target, resolving);
        }
    }
    for (signed, target) in program
        .cost_signed_terms
        .iter()
        .zip(resolved.cost_signed_terms.iter_mut())
    {
        if let (Some(original), Some(result)) =
            (signed.signature.as_ref(), target.signature.as_mut())
        {
            keep(original, result);
        }
        if let (Some(body), Some(body_target)) = (signed.body.as_ref(), target.body.as_mut()) {
            keep_unresolvable_quotes(body, body_target, resolving);
        }
    }
    for (stack, target) in program
        .cost_stacks
        .iter()
        .zip(resolved.cost_stacks.iter_mut())
    {
        for (cell, cell_target) in stack.cells.iter().zip(target.cells.iter_mut()) {
            keep(cell, cell_target);
        }
    }
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

    /// Added by the G1-4a fix (review H-1): the quote of a resolved name
    /// resolves in a resolving position. In a receive body it stays
    /// unchanged, because the reducer substitutes the body under the shift
    /// of the receive's binders before it evaluates the signature.
    #[test]
    fn a_quoted_name_resolves_only_in_a_resolving_position() {
        let program = Compiler::source_to_adt(
            "new x in { {% Nil %}[ #*x ] | for (@n <- x) { {% Nil %}[ #*x ] } }",
        )
        .expect("the program compiles");
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"quoted name"),
            &HashMap::new(),
        )
        .expect("the program resolves");
        let inner_signature = |par: &Par| {
            par.news[0]
                .p
                .as_ref()
                .and_then(|body| body.receives[0].body.as_ref())
                .and_then(|body| body.cost_signed_terms[0].signature.clone())
                .expect("a signature in the receive body")
        };
        let body = resolved.news[0].p.as_ref().expect("the body of the new");
        let outer = body.cost_signed_terms[0]
            .signature
            .clone()
            .expect("a signature in the body of the new");
        match outer.value {
            Some(Value::Quote(quoted)) => {
                assert_eq!(quoted.unforgeables.len(), 1, "the quote of the name");
                assert!(quoted.exprs.is_empty(), "no variable is left");
            }
            other => panic!("the outer signature is a quote: {other:?}"),
        }
        assert_eq!(inner_signature(&resolved), inner_signature(&program));
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
