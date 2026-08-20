use crate::rust::interpreter::compiler::free_map::FreeMapConnective;
use crate::rust::interpreter::compiler::normalize::{NameVisitOutputs, ProcVisitInputs};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::errors::InterpreterError::PatternReceiveError;

pub fn fail_on_invalid_connective<P: Clone>(
    input: &ProcVisitInputs<P>,
    name_res: &NameVisitOutputs<P>,
) -> Result<NameVisitOutputs<P>, InterpreterError> {
    if input.bound_map_chain.depth() == 0 {
        name_res
            .free_map
            .connectives
            .iter()
            .find_map(
                |(connective_instance, source_span)| match connective_instance {
                    FreeMapConnective::Or => Some(PatternReceiveError(format!(
                        "\\/ (disjunction) at {:?}",
                        source_span
                    ))),
                    FreeMapConnective::Not => Some(PatternReceiveError(format!(
                        "~ (negation) at {:?}",
                        source_span
                    ))),
                    _ => None,
                },
            )
            .map_or(Ok(name_res.clone()), Err)
    } else {
        Ok(name_res.clone())
    }
}
