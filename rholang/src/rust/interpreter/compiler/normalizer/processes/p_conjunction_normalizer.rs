use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::par_construction::{ParConstruction, PlannedConnective, PlannedPar};
use rholang_parser::ast::{AnnProc, BinaryExpOp, Proc};
use rholang_parser::SourceSpan;

use crate::rust::interpreter::compiler::exports::{
    ProcVisitInputs as PublicProcVisitInputs, ProcVisitOutputs as PublicProcVisitOutputs,
};
use crate::rust::interpreter::compiler::free_map::FreeMapConnective;
use crate::rust::interpreter::compiler::normalize_drive::{
    NormKont, NormVal, NormWork, ProcVisitInputs, ProcVisitOutputs, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `P /\ Q`, descend half — the left conjunct.
///
/// The right conjunct sees the left one's `free_map`, so it cannot be scheduled
/// until the left has produced a value. The span the resulting connective is
/// recorded under spans both operands, and is fixed here while both are in
/// scope.
#[inline(never)]
pub(crate) fn descend_p_conjunction<'ast>(
    left: AnnProc<'ast>,
    right: AnnProc<'ast>,
    input: ProcVisitInputs,
    construction: &mut ParConstruction,
) -> Result<Step<'ast>, InterpreterError> {
    let (operands, spans) = flatten_left_conjunction(left, right);
    let bound_map_chain = input.bound_map_chain.clone();
    let free_map = input.free_map.clone();
    let first = operands[0];
    Ok(Step::Descend {
        kont: NormKont::Conjunction {
            operands,
            spans,
            idx: 0,
            acc_pars: Vec::new(),
            input,
        },
        work: NormWork::Proc {
            proc: first,
            input: ProcVisitInputs {
                par: construction.empty_par()?,
                bound_map_chain,
                free_map,
            },
        },
    })
}

/// Flatten only the left spine. This is the exact historical rule: a finished
/// left operand that is already `ConnAnd` contributes its children, while a
/// right-hand conjunction remains one child. Collecting the source spine up
/// front avoids rebuilding prefixes of length `1..n` for a left-associated
/// chain.
fn flatten_left_conjunction<'ast>(
    left: AnnProc<'ast>,
    right: AnnProc<'ast>,
) -> (Vec<AnnProc<'ast>>, Vec<SourceSpan>) {
    let mut reversed_rights = vec![right];
    let mut reversed_spans = vec![SourceSpan {
        start: left.span.start,
        end: right.span.end,
    }];
    let mut cursor = left;
    loop {
        match cursor.proc {
            Proc::BinaryExp {
                op: BinaryExpOp::Conjunction,
                left,
                right,
            } => {
                reversed_rights.push(*right);
                reversed_spans.push(SourceSpan {
                    start: left.span.start,
                    end: right.span.end,
                });
                cursor = *left;
            }
            _ => break,
        }
    }
    reversed_rights.reverse();
    reversed_spans.reverse();
    let mut operands = Vec::with_capacity(1 + reversed_rights.len());
    operands.push(cursor);
    operands.extend(reversed_rights);
    (operands, reversed_spans)
}

/// `P /\ Q`, combine half.
/// `P conjunction Q` on a **fresh** drive. Only the unit tests enter here; the dispatch
/// pushes [`descend_p_conjunction`]'s `Step` onto the drive it is already
/// on, so the whole traversal stays one loop.
pub fn normalize_p_conjunction<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: PublicProcVisitInputs,
    env: &std::collections::HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<PublicProcVisitOutputs, InterpreterError> {
    use crate::rust::interpreter::compiler::normalize_drive::norm_drive_from;
    let mut construction = ParConstruction::new();
    let planned_input = ProcVisitInputs {
        par: construction.import_par(&input.par)?,
        bound_map_chain: input.bound_map_chain,
        free_map: input.free_map,
    };
    let step = descend_p_conjunction(*left, *right, planned_input, &mut construction)?;
    let result = norm_drive_from(step, &mut construction, env, parser)?.into_proc();
    Ok(PublicProcVisitOutputs {
        par: construction.finish(result.par)?,
        free_map: result.free_map,
    })
}

#[inline(never)]
pub(crate) fn combine_p_conjunction<'ast>(
    operands: Vec<AnnProc<'ast>>,
    spans: Vec<SourceSpan>,
    mut idx: usize,
    mut acc_pars: Vec<PlannedPar>,
    input: ProcVisitInputs,
    value: NormVal,
    construction: &mut ParConstruction,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    if idx == 0 {
        match construction.single_connective(result.par) {
            Some(PlannedConnective::And(children)) => acc_pars.extend_from_slice(children),
            _ => acc_pars.push(result.par),
        }
    } else {
        acc_pars.push(result.par);
    }
    idx += 1;

    let mut free_map = result.free_map;
    if idx >= 2 {
        free_map = free_map.add_connective_kind(FreeMapConnective::And, spans[idx - 2]);
    }

    if idx < operands.len() {
        let next = operands[idx];
        let bound_map_chain = input.bound_map_chain.clone();
        return Ok(Step::Descend {
            kont: NormKont::Conjunction {
                operands,
                spans,
                idx,
                acc_pars,
                input,
            },
            work: NormWork::Proc {
                proc: next,
                input: ProcVisitInputs {
                    par: construction.empty_par()?,
                    bound_map_chain,
                    free_map,
                },
            },
        });
    }
    let result_connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: vec![Par::default(); acc_pars.len()],
        })),
    };
    let component = construction.connective_component(&result_connective, &acc_pars)?;
    let result_par = construction.append_component(input.par, component, Vec::new(), true)?;

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: result_par,
        free_map,
    })))
}

//rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::{Connective, ConnectiveBody};
    use models::rust::utils::new_freevar_par;
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::compiler::normalize::VarSort::ProcSort;
    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;

    #[test]
    fn p_conjunction_should_delegate_and_count_any_free_variables_inside() {
        use rholang_parser::SourcePos;

        use super::normalize_p_conjunction;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();

        let parser = rholang_parser::RholangParser::new();

        let left_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let right_proc = ParBuilderUtil::create_ast_proc_var("y", &parser);

        let result =
            normalize_p_conjunction(&left_proc, &right_proc, inputs.clone(), &env, &parser);
        let expected_result = inputs
            .par
            .with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![
                        new_freevar_par(0, Vec::new()),
                        new_freevar_par(1, Vec::new()),
                    ],
                })),
            }])
            .with_connective_used(true);
        assert_eq!(result.clone().unwrap().par, expected_result);

        let expected_free = inputs.free_map.put_all_pos(vec![
            ("x".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
            ("y".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
        ]);

        assert_eq!(
            result.clone().unwrap().free_map.level_bindings,
            expected_free.level_bindings
        );
        assert_eq!(
            result.unwrap().free_map.next_level,
            expected_free.next_level
        );
    }
}
