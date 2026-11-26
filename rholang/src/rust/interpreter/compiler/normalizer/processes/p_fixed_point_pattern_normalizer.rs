use std::collections::HashMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::{ConnFixedPointBody, Connective, Par};
use models::rust::utils::new_wildcard_par;
use rholang_parser::ast::AnnProc;
use rholang_parser::SourceSpan;

use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::prepend_connective;

pub fn normalize_p_fixed_point_pattern<'ast>(
    scale: &'ast Option<AnnProc<'ast>>,
    pattern_span: SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Bare `FixedPoint` is sugar for `FixedPoint(_)`. Both forms produce a
    // ConnFixedPointBody whose scale pattern accepts any integer.
    let (scale_par, free_map_after) = match scale {
        None => (new_wildcard_par(Vec::new(), true), input.free_map.clone()),
        Some(scale_proc) => {
            let result = normalize_ann_proc(
                scale_proc,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: input.free_map.clone(),
                },
                env,
                parser,
            )?;
            (result.par, result.free_map)
        }
    };

    let body = ConnFixedPointBody {
        scale: Some(scale_par),
    };

    let connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnFixedPointBody(body)),
    };

    let depth = input.bound_map_chain.depth() as i32;
    let mut updated_par = prepend_connective(input.par, connective.clone(), depth);
    updated_par.connective_used = true;

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: free_map_after
            .add_connective(connective.connective_instance.unwrap(), pattern_span),
    })
}
