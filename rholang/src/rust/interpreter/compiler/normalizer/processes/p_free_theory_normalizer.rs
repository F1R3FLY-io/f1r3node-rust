//! The normalizer arm of the Reified RSpaces theory reference `free Th(args)`.

use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::reified::builders::{efree, push_process_expr};
use rholang_parser::ast::{AnnProc, Id};
use rholang_parser::RholangParser;

use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalizer::processes::reified_syntax::{
    normalize_procs, reified_syntax_error,
};
use crate::rust::interpreter::errors::InterpreterError;

/// Normalizes `free name(args)` to the value `(FREE, "name", [args…])`, an expression of
/// `Par.exprs`. The arguments are processes in the current scope, in source order. The theory
/// name must be a name that the parser accepts in this position (plan decision 27).
pub fn normalize_p_free_theory<'ast>(
    name: &Id<'ast>,
    args: &[AnnProc<'ast>],
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let (args, free_map) =
        normalize_procs(args, &input.bound_map_chain, input.free_map, env, parser)?;
    let theory = efree(name.name, args).map_err(reified_syntax_error)?;
    Ok(ProcVisitOutputs {
        par: push_process_expr(input.par, theory),
        free_map,
    })
}
