use std::cell::RefCell;
use std::collections::HashSet;

use models::rhoapi::{ElaboratedSeal, Par, SealOrigin};
use rholang_parser::ast::{AnnProc, Bind, Id, Name, Names, Proc, Signature, Var};
use rholang_parser::{RholangParser, SourcePos};

use crate::rust::interpreter::errors::InterpreterError;

pub const DEPLOYMENT_SEAL: &str = "\u{0}deployment";
pub const KIND_DEPLOYMENT: i32 = 1;
pub const KIND_SIGNED: i32 = 2;
pub const KIND_PROCESS: i32 = 3;

#[derive(Clone, Debug)]
pub struct Elaboration {
    pub origin: SealOrigin,
    pub deployer: Par,
    pub pass: bool,
}

thread_local! {
    static ELABORATION: RefCell<Option<Elaboration>> = const { RefCell::new(None) };
}

pub fn within<R>(elaboration: Option<Elaboration>, f: impl FnOnce() -> R) -> R {
    let previous = ELABORATION.with(|cell| cell.replace(elaboration));
    let result = f();
    ELABORATION.with(|cell| cell.replace(previous));
    result
}

pub fn current() -> Option<Elaboration> { ELABORATION.with(|cell| cell.borrow().clone()) }

pub fn deployment_signature<'ast>() -> Signature<'ast> {
    Signature::Ground(Name::NameVar(Var::Id(Id {
        name: DEPLOYMENT_SEAL,
        pos: SourcePos { line: 0, col: 0 },
    })))
}

pub fn is_deployment_signature(sig: &Signature) -> bool {
    matches!(sig, Signature::Ground(Name::NameVar(Var::Id(id))) if id.name == DEPLOYMENT_SEAL)
}

fn only_hashes(sig: &Signature) -> bool {
    match sig {
        Signature::Hash(_) => true,
        Signature::Ground(_) => false,
        Signature::Compound(left, right) | Signature::Transfer(left, right) => {
            only_hashes(left) && only_hashes(right)
        }
    }
}

fn contains_hash(sig: &Signature) -> bool {
    match sig {
        Signature::Hash(_) => true,
        Signature::Ground(_) => false,
        Signature::Compound(left, right) | Signature::Transfer(left, right) => {
            contains_hash(left) || contains_hash(right)
        }
    }
}

pub fn kind_of(sig: &Signature) -> i32 {
    if is_deployment_signature(sig) {
        KIND_DEPLOYMENT
    } else if only_hashes(sig) {
        KIND_PROCESS
    } else {
        KIND_SIGNED
    }
}

pub fn seal_for(sig: &Signature, limit: Option<u64>) -> Option<ElaboratedSeal> {
    current().map(|elaboration| ElaboratedSeal {
        kind: kind_of(sig),
        origin: crate::rust::interpreter::accounting::rev_stack::seal::origin_digest(&elaboration.origin),
        phlo_limit: limit.unwrap_or(elaboration.origin.phlo_limit),
    })
}

pub fn elaborate<'ast>(
    proc: AnnProc<'ast>,
    parser: &'ast RholangParser<'ast>,
) -> Result<AnnProc<'ast>, InterpreterError> {
    let sig = deployment_signature();
    let body = seal(proc, &sig, None, parser)?;
    Ok(parser
        .ast_builder()
        .alloc_signed_term_with_limit(body, sig, None)
        .ann(proc.span))
}

fn sig_ids<'ast>(sig: &Signature<'ast>, out: &mut HashSet<String>, parser: &'ast RholangParser<'ast>) {
    match sig {
        Signature::Ground(Name::NameVar(Var::Id(id))) => {
            out.insert(id.name.to_string());
        }
        Signature::Ground(_) => {}
        Signature::Hash(proc) => out.extend(proc_ids(*proc, parser)),
        Signature::Compound(left, right) | Signature::Transfer(left, right) => {
            sig_ids(left, out, parser);
            sig_ids(right, out, parser);
        }
    }
}

fn proc_ids<'ast>(proc: AnnProc<'ast>, parser: &'ast RholangParser<'ast>) -> Vec<String> {
    let Proc::Collection(rholang_parser::ast::Collection::List { elements, .. }) =
        parser.ast_builder().alloc_list(&[proc])
    else {
        return Vec::new();
    };
    elements[0]
        .iter_vars()
        .filter_map(|var| match var {
            Var::Id(id) => Some(id.name.to_string()),
            Var::Wildcard => None,
        })
        .collect()
}

fn name_ids<'ast>(name: &Name<'ast>, out: &mut HashSet<String>, parser: &'ast RholangParser<'ast>) {
    match name {
        Name::NameVar(Var::Id(id)) => {
            out.insert(id.name.to_string());
        }
        Name::NameVar(Var::Wildcard) => {}
        Name::Quote(proc) => out.extend(proc_ids(*proc, parser)),
    }
}

fn names_ids<'ast>(names: &Names<'ast>, out: &mut HashSet<String>, parser: &'ast RholangParser<'ast>) {
    for name in names.names.iter() {
        name_ids(name, out, parser);
    }
    if let Some(Var::Id(id)) = names.remainder {
        out.insert(id.name.to_string());
    }
}

fn receipts_bound<'ast>(receipts: &rholang_parser::ast::Receipts<'ast>, parser: &'ast RholangParser<'ast>) -> HashSet<String> {
    let mut bound = HashSet::new();
    for bind in receipts.iter().flat_map(|r| r.binds.iter()) {
        match bind {
            Bind::Linear { lhs, .. }
            | Bind::Repeated { lhs, .. }
            | Bind::Peek { lhs, .. }
            | Bind::Signed { lhs, .. } => names_ids(lhs, &mut bound, parser),
        }
    }
    bound
}

fn shadowed<'ast>(sig: &Signature<'ast>, bound: HashSet<String>, parser: &'ast RholangParser<'ast>) -> Result<(), InterpreterError> {
    let mut used = HashSet::new();
    sig_ids(sig, &mut used, parser);
    match used.intersection(&bound).next() {
        None => Ok(()),
        Some(name) => Err(InterpreterError::NormalizerError(format!(
            "cost-accounting: `{name}` is rebound inside a region that its seal names; rename the \
             binder so that the elaborated continuation seal keeps its meaning"
        ))),
    }
}

fn continuation<'ast>(
    body: AnnProc<'ast>,
    sig: &Signature<'ast>,
    limit: Option<u64>,
    parser: &'ast RholangParser<'ast>,
) -> Result<AnnProc<'ast>, InterpreterError> {
    if matches!(body.proc, Proc::SignedTerm { .. }) {
        return seal(body, sig, limit, parser);
    }
    let sealed = seal(body, sig, limit, parser)?;
    Ok(parser
        .ast_builder()
        .alloc_signed_term_with_limit(sealed, sig.clone(), limit)
        .ann(body.span))
}

fn seal<'ast>(
    p: AnnProc<'ast>,
    sig: &Signature<'ast>,
    limit: Option<u64>,
    parser: &'ast RholangParser<'ast>,
) -> Result<AnnProc<'ast>, InterpreterError> {
    let builder = parser.ast_builder();
    let rebuilt = match p.proc {
        Proc::Par { left, right } => {
            builder.alloc_par(seal(*left, sig, limit, parser)?, seal(*right, sig, limit, parser)?)
        }
        Proc::IfThenElse {
            condition,
            if_true,
            if_false,
        } => builder.alloc_if_then_else_opt(
            *condition,
            seal(*if_true, sig, limit, parser)?,
            if_false.map(|p| seal(p, sig, limit, parser)).transpose()?,
        ),
        Proc::New { decls, proc } => {
            shadowed(sig, decls.iter().map(|d| d.id.name.to_string()).collect(), parser)?;
            builder.alloc_new(seal(*proc, sig, limit, parser)?, decls.clone())
        }
        Proc::Let {
            bindings,
            body,
            concurrent,
        } => {
            let mut bound = HashSet::new();
            for binding in bindings.iter() {
                names_ids(&binding.lhs, &mut bound, parser);
            }
            shadowed(sig, bound, parser)?;
            builder.alloc_let(
                bindings.iter().cloned(),
                seal(*body, sig, limit, parser)?,
                *concurrent,
            )
        }
        Proc::Match { expression, cases } => {
            let mut rebuilt = Vec::with_capacity(cases.len());
            for case in cases {
                shadowed(sig, proc_ids(case.pattern, parser).into_iter().collect(), parser)?;
                rebuilt.push((case.pattern, case.guard, seal(case.proc, sig, limit, parser)?));
            }
            builder.alloc_match_with_guards(*expression, rebuilt)
        }
        Proc::ForComprehension { receipts, proc } => {
            shadowed(sig, receipts_bound(receipts, parser), parser)?;
            let body = continuation(*proc, sig, limit, parser)?;
            builder.alloc_for_with_guards(
                receipts.iter().map(|r| (r.binds.iter().cloned(), r.guard)),
                body,
            )
        }
        Proc::Contract {
            name,
            formals,
            body,
        } => {
            let mut bound = HashSet::new();
            names_ids(formals, &mut bound, parser);
            shadowed(sig, bound, parser)?;
            builder.alloc_contract(name.clone(), formals.clone(), continuation(*body, sig, limit, parser)?)
        }
        Proc::SignedTerm {
            proc: inner,
            sig: inner_sig,
            limit: inner_limit,
        } => match (inner_sig, inner.proc) {
            (Signature::Transfer(s1, s2), Proc::ForComprehension { receipts, proc }) => {
                shadowed(s2, receipts_bound(receipts, parser), parser)?;
                let body = seal(
                    builder
                        .alloc_signed_term_with_limit(*proc, (**s2).clone(), *inner_limit)
                        .ann(proc.span),
                    sig,
                    limit,
                    parser,
                )?;
                let comm = builder
                    .alloc_for_with_guards(
                        receipts.iter().map(|r| (r.binds.iter().cloned(), r.guard)),
                        body,
                    )
                    .ann(inner.span);
                builder.alloc_signed_term_with_limit(comm, (**s1).clone(), *inner_limit)
            }
            (Signature::Transfer(_, _), _) => p.proc,
            _ if contains_hash(inner_sig) => p.proc,
            _ => builder.alloc_signed_term_with_limit(
                seal(*inner, inner_sig, *inner_limit, parser)?,
                inner_sig.clone(),
                *inner_limit,
            ),
        },
        _ => p.proc,
    };
    Ok(rebuilt.ann(p.span))
}
