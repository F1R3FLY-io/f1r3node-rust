//! Frozen recursive oracle for the pre-PDA accounting-demand semantics.
//!
//! This code intentionally uses decoded `Par` values and recursion. It is test
//! only, bounded by its callers, and structurally independent of the production
//! byte cursor. Its event order is the historical contract that the iterative
//! implementation must refine.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Bundle, Expr, If, Match, New, Par, Receive, Send};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemandOracleEvent {
    Comm(Option<Par>),
    Unknown,
}

pub fn trace_recursive(par: &Par, out: &mut Vec<DemandOracleEvent>) {
    for send in par.decode_occurrences::<Send>() {
        out.push(DemandOracleEvent::Comm(send.chan));
    }

    for receive in par.decode_occurrences::<Receive>() {
        let channel = receive.binds.first().and_then(|bind| bind.source.clone());
        out.push(DemandOracleEvent::Comm(channel));
        if let Some(body) = &receive.body {
            trace_recursive(body, out);
        }
    }

    for new in par.decode_occurrences::<New>() {
        if let Some(body) = &new.p {
            trace_recursive(body, out);
        }
    }

    for mat in par.decode_occurrences::<Match>() {
        for case in &mat.cases {
            if let Some(source) = &case.source {
                trace_recursive(source, out);
            }
        }
    }

    for conditional in par.decode_occurrences::<If>() {
        if let Some(if_true) = &conditional.if_true {
            trace_recursive(if_true, out);
        }
        if let Some(if_false) = &conditional.if_false {
            trace_recursive(if_false, out);
        }
    }

    for bundle in par.decode_occurrences::<Bundle>() {
        if let Some(body) = &bundle.body {
            trace_recursive(body, out);
        }
    }

    for expr in par.decode_occurrences::<Expr>() {
        if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
            if matches!(
                evar.v.as_ref().and_then(|var| var.var_instance.as_ref()),
                Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_))
            ) {
                out.push(DemandOracleEvent::Unknown);
            }
        }
    }
}

pub fn trace(par: &Par) -> Vec<DemandOracleEvent> {
    let mut out = Vec::new();
    trace_recursive(par, &mut out);
    out
}
