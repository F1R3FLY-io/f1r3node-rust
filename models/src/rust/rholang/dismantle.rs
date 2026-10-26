//! The iterative teardown of `Par` terms and cost signatures.
//!
//! Moved by DR-131 P2b from
//! `rholang/src/rust/interpreter/accounting/delta_sigma/estimate.rs`, without
//! change to the functions, so that the cold-store decoder in this crate
//! (`record_decoder.rs`) releases a partly decoded value without recursion. The
//! estimate re-exports both public functions, so their callers keep their
//! paths.

use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::cost_signature::Value as CostSignatureValue;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{CostSignature, Expr, Par};

/// Drops a term without recursion. The work is in `dismantle_terms`.
pub fn dismantle_term(par: Par) { dismantle_terms(vec![par]); }

/// Drops terms without recursion. The derived drop of prost recurses once
/// per nesting level, so each node first moves every nested `Par` and
/// `CostSignature` to a heap work list: process positions, expressions,
/// patterns, conditions, connectives, injections and signatures. It is the
/// one teardown of the crate's accounting walks (review cleanup), and the
/// funding resolver uses it too.
pub fn dismantle_terms(pars: Vec<Par>) {
    let mut pars = pars;
    let mut signatures: Vec<CostSignature> = Vec::new();
    loop {
        if let Some(mut par) = pars.pop() {
            for mut send in par.sends.drain(..) {
                pars.extend(send.chan.take());
                pars.append(&mut send.data);
            }
            for mut receive in par.receives.drain(..) {
                for mut bind in receive.binds.drain(..) {
                    pars.append(&mut bind.patterns);
                    pars.extend(bind.source.take());
                    signatures.extend(bind.cost_signature.take());
                }
                pars.extend(receive.body.take());
                pars.extend(receive.condition.take());
            }
            for mut new in par.news.drain(..) {
                pars.extend(new.p.take());
                pars.extend(std::mem::take(&mut new.injections).into_values());
            }
            for expr in par.exprs.drain(..) {
                take_expr_pars(expr, &mut pars);
            }
            for mut matched in par.matches.drain(..) {
                pars.extend(matched.target.take());
                for case in matched.cases.drain(..) {
                    pars.extend(case.pattern);
                    pars.extend(case.source);
                    pars.extend(case.guard);
                }
            }
            for mut bundle in par.bundles.drain(..) {
                pars.extend(bundle.body.take());
            }
            for connective in par.connectives.drain(..) {
                match connective.connective_instance {
                    Some(ConnectiveInstance::ConnAndBody(body))
                    | Some(ConnectiveInstance::ConnOrBody(body)) => pars.extend(body.ps),
                    Some(ConnectiveInstance::ConnNotBody(body)) => pars.push(body),
                    Some(
                        ConnectiveInstance::VarRefBody(_)
                        | ConnectiveInstance::ConnBool(_)
                        | ConnectiveInstance::ConnInt(_)
                        | ConnectiveInstance::ConnString(_)
                        | ConnectiveInstance::ConnUri(_)
                        | ConnectiveInstance::ConnByteArray(_),
                    )
                    | None => {}
                }
            }
            for mut conditional in par.conditionals.drain(..) {
                pars.extend(conditional.condition.take());
                pars.extend(conditional.if_true.take());
                pars.extend(conditional.if_false.take());
            }
            for mut signed in par.cost_signed_terms.drain(..) {
                pars.extend(signed.body.take());
                signatures.extend(signed.signature.take());
            }
            for mut stack in par.cost_stacks.drain(..) {
                signatures.append(&mut stack.cells);
            }
        } else if let Some(signature) = signatures.pop() {
            match signature.value {
                Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                    pars.push(par)
                }
                Some(CostSignatureValue::Compound(compound)) => {
                    signatures.extend(compound.elements)
                }
                Some(
                    CostSignatureValue::Ground(_)
                    | CostSignatureValue::BoundLevel(_)
                    | CostSignatureValue::Unit(_),
                )
                | None => {}
            }
        } else {
            break;
        }
    }
}

/// Moves the operands of one owned expression to the teardown work list. The
/// match is exhaustive, like `expr_operands`, so a new form reaches both.
fn take_expr_pars(expr: Expr, pars: &mut Vec<Par>) {
    let mut both = |left: Option<Par>, right: Option<Par>| {
        pars.extend(left);
        pars.extend(right);
    };
    match expr.expr_instance {
        Some(ExprInstance::ENotBody(body)) => both(body.p, None),
        Some(ExprInstance::ENegBody(body)) => both(body.p, None),
        Some(ExprInstance::EMultBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EDivBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EModBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EPlusBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EMinusBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::ELtBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::ELteBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EGtBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EGteBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EEqBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::ENeqBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EAndBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EOrBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EPercentPercentBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EPlusPlusBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EMinusMinusBody(body)) => both(body.p1, body.p2),
        Some(ExprInstance::EMatchesBody(body)) => both(body.target, body.pattern),
        Some(ExprInstance::EListBody(list)) => pars.extend(list.ps),
        Some(ExprInstance::ETupleBody(tuple)) => pars.extend(tuple.ps),
        Some(ExprInstance::ESetBody(set)) => pars.extend(set.ps),
        Some(ExprInstance::EMapBody(map)) => {
            for pair in map.kvs {
                pars.extend(pair.key);
                pars.extend(pair.value);
            }
        }
        Some(ExprInstance::EMethodBody(method)) => {
            pars.extend(method.target);
            pars.extend(method.arguments);
        }
        Some(ExprInstance::EPathmapBody(pathmap)) => pars.extend(pathmap.ps),
        Some(ExprInstance::EZipperBody(zipper)) => {
            pars.extend(zipper.pathmap.into_iter().flat_map(|pathmap| pathmap.ps))
        }
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
            | ExprInstance::GUint8(_)
            | ExprInstance::EVarBody(_),
        )
        | None => {}
    }
}
