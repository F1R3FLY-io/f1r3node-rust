use std::result::Result;

use models::rhoapi::{Bundle, Par};
use models::rust::bundle_ops::BundleOps;
use models::rust::rholang::par_children::dismantle;
use rholang_parser::ast::{AnnProc, BundleType};
use rholang_parser::SourceSpan;

use super::exports::*;
use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::util::prepend_bundle;

/// The free-variables-in-a-bundle rejection. Takes only the FREE MAP, never the
/// term, so the caller can dismantle the (deep) body before or after building
/// the message.
fn error(
    free_map: &crate::rust::interpreter::compiler::exports::FreeMap<
        crate::rust::interpreter::compiler::normalize::VarSort,
    >,
) -> Result<Step<'static>, InterpreterError> {
    let err_msg = {
        let at = |variable: &str, source_position: &SourceSpan| {
            format!(
                "{} at line {}, column {}",
                variable, source_position.start.line, source_position.start.col
            )
        };

        let wildcards_positions: Vec<String> = free_map
            .wildcards
            .iter()
            .map(|pos| at("", pos))
            .collect();

        let free_vars_positions: Vec<String> = free_map
            .level_bindings
            .iter()
            .map(|(name, context)| at(&format!("`{}`", name), &context.source_span))
            .collect();

        let err_msg_wildcards = if !wildcards_positions.is_empty() {
            format!(" Wildcards positions: {}", wildcards_positions.join(", "))
        } else {
            String::new()
        };

        let err_msg_free_vars = if !free_vars_positions.is_empty() {
            format!(
                " Free variables positions: {}",
                free_vars_positions.join(", ")
            )
        } else {
            String::new()
        };

        format!(
            "Bundle's content must not have free variables or wildcards.{}{}",
            err_msg_wildcards, err_msg_free_vars
        )
    };

    Err(InterpreterError::UnexpectedBundleContent(format!(
        "Bundle's content must not have free variables or wildcards. {}",
        err_msg
    )))
}

/// `bundle+{ P }`, descend half — one child, the bundled body.
#[inline(never)]
pub(crate) fn descend_p_bundle<'ast>(
    bundle_type: BundleType,
    proc: AnnProc<'ast>,
    input: ProcVisitInputs,
    span: SourceSpan,
) -> Step<'ast> {
    let child_input = ProcVisitInputs {
        par: Par::default(),
        ..input.clone()
    };
    Step::Descend {
        kont: NormKont::Bundle {
            bundle_type,
            span,
            input,
        },
        work: NormWork::Proc {
            proc,
            input: child_input,
        },
    }
}

/// `bundle+{ P }`, combine half — the three rejection conditions, then the
/// bundle merge.
#[inline(never)]
pub(crate) fn combine_p_bundle<'ast>(
    bundle_type: BundleType,
    span: SourceSpan,
    input: ProcVisitInputs,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let target_result = value.into_proc();

    // ★ Leg-1. The recursive form built `outermost_bundle` FIRST, cloning the
    // body into it, and only then tested the three rejection conditions — so
    // every bundle paid a deep `<Par as Clone>::clone` even on the paths that
    // immediately threw it away. The conditions are tested first and the body
    // is MOVED into the bundle on the one path that keeps it.
    let (write_flag, read_flag) = match bundle_type {
        BundleType::BundleReadWrite => (true, true),
        BundleType::BundleRead => (false, true),
        BundleType::BundleWrite => (true, false),
        BundleType::BundleEquiv => (false, false),
    };

    if !target_result.par.connectives.is_empty() {
        // ⚠ The rejected body is deep and `<Par as Drop>` is Θ(depth), so it is
        // torn down with an explicit worklist rather than left to fall out of
        // scope — otherwise a hostile bundle would overflow on the way OUT.
        dismantle(target_result.par);
        Err(InterpreterError::UnexpectedBundleContent(format!(
            "Illegal top-level connective in bundle at line {}, column {}.",
            span.start.line, span.start.col
        )))
    } else if !target_result.free_map.wildcards.is_empty()
        || !target_result.free_map.level_bindings.is_empty()
    {
        // `error` cannot name a caller lifetime, so its `Step<'static>` is
        // re-typed here; it only ever returns `Err`.
        let err = match error(&target_result.free_map) {
            Err(e) => e,
            Ok(_) => unreachable!("p_bundle::error always returns Err"),
        };
        dismantle(target_result.par);
        Err(err)
    } else {
        let single = target_result.par.single_bundle();
        let outermost_bundle = Bundle {
            body: Some(target_result.par),
            write_flag,
            read_flag,
        };
        let new_bundle = match single {
            Some(single) => BundleOps::merge(&outermost_bundle, &single),
            None => outermost_bundle,
        };

        Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
            par: { prepend_bundle(input.par, new_bundle) },
            free_map: input.free_map,
        })))
    }
}


#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::{Bundle, Par};
    use models::rust::utils::new_boundvar_par;
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::compiler::exports::ProcVisitInputs;
    use crate::rust::interpreter::compiler::normalize::VarSort;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::test_utils::utils::{
        proc_visit_inputs_and_env, proc_visit_inputs_with_updated_bound_map_chain,
    };

    #[test]
    fn p_bundle_should_normalize_terms_inside() {
        use rholang_parser::ast::BundleType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let bound_inputs =
            proc_visit_inputs_with_updated_bound_map_chain(inputs.clone(), "x", VarSort::ProcSort);

        let parser = rholang_parser::RholangParser::new();

        let inner_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let bundle_proc =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleReadWrite, inner_proc, &parser);

        let result = normalize_ann_proc(&bundle_proc, bound_inputs.clone(), &env, &parser);
        let expected_result = inputs
            .par
            .with_bundles(vec![Bundle {
                body: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                write_flag: true,
                read_flag: true,
            }])
            .with_locally_free(create_bit_vector(&vec![0]));

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.clone().unwrap().free_map, bound_inputs.free_map);
    }

    /** Example:
     * bundle { _ | x }
     */
    #[test]
    fn p_bundle_should_throw_an_error_when_wildcard_or_free_variable_is_found_inside_body_of_bundle(
    ) {
        use rholang_parser::ast::BundleType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();

        let parser = rholang_parser::RholangParser::new();

        let wildcard_proc = ParBuilderUtil::create_ast_wildcard(&parser);
        let var_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let par_proc = ParBuilderUtil::create_ast_par(wildcard_proc, var_proc, &parser);
        let bundle_proc =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleReadWrite, par_proc, &parser);

        let result = normalize_ann_proc(&bundle_proc, inputs.clone(), &env, &parser);
        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedBundleContent { .. })
        ));
    }

    /** Example:
     * bundle { Uri }
     */
    #[test]
    fn p_bundle_should_throw_an_error_when_connective_is_used_at_top_level_of_body_of_bundle() {
        use rholang_parser::ast::{BundleType, SimpleType};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();

        let parser = rholang_parser::RholangParser::new();

        let uri_proc = ParBuilderUtil::create_ast_simple_type(SimpleType::Uri, &parser);
        let bundle_proc =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleReadWrite, uri_proc, &parser);

        let result = normalize_ann_proc(&bundle_proc, inputs.clone(), &env, &parser);

        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedBundleContent { .. })
        ));
    }

    /** Example:
     * bundle { @Nil!(Uri) }
     */
    #[test]
    fn p_bundle_should_not_throw_an_error_when_connective_is_used_outside_of_top_level_of_body_of_bundle(
    ) {
        use rholang_parser::ast::{BundleType, SendType, SimpleType};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();

        let parser = rholang_parser::RholangParser::new();

        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel = ParBuilderUtil::create_ast_quote_name(nil_proc);
        let uri_proc = ParBuilderUtil::create_ast_simple_type(SimpleType::Uri, &parser);
        let send_proc =
            ParBuilderUtil::create_ast_send(channel, SendType::Single, vec![uri_proc], &parser);
        let bundle_proc =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleReadWrite, send_proc, &parser);

        let result = normalize_ann_proc(&bundle_proc, inputs.clone(), &env, &parser);

        assert!(result.is_ok());
    }

    #[test]
    fn p_bundle_should_interpret_bundle_polarization() {
        use rholang_parser::ast::BundleType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let bound_inputs =
            proc_visit_inputs_with_updated_bound_map_chain(inputs.clone(), "x", VarSort::ProcSort);

        let parser = rholang_parser::RholangParser::new();

        fn expected_results(write_flag: bool, read_flag: bool, inputs: &ProcVisitInputs) -> Par {
            inputs
                .clone()
                .par
                .with_bundles(vec![Bundle {
                    body: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                    write_flag,
                    read_flag,
                }])
                .with_locally_free(create_bit_vector(&vec![0]))
        }

        let test = |bundle_type: BundleType, write_flag: bool, read_flag: bool| {
            let inner_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
            let bundle_proc = ParBuilderUtil::create_ast_bundle(bundle_type, inner_proc, &parser);
            let result = normalize_ann_proc(&bundle_proc, bound_inputs.clone(), &env, &parser);
            let expected = expected_results(write_flag, read_flag, &bound_inputs);

            assert_eq!(
                result.clone().unwrap().par,
                expected,
                "Resulting `Par` did not match expected"
            );
            assert_eq!(
                result.unwrap().free_map,
                inputs.free_map,
                "Resulting `FreeMap` did not match expected"
            );
        };

        test(BundleType::BundleReadWrite, true, true);
        test(BundleType::BundleRead, false, true);
        test(BundleType::BundleWrite, true, false);
        test(BundleType::BundleEquiv, false, false);
    }

    #[test]
    fn p_bundle_should_collapse_nested_bundles_merging_their_polarizations() {
        use rholang_parser::ast::BundleType;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (inputs, env) = proc_visit_inputs_and_env();
        let bound_inputs =
            proc_visit_inputs_with_updated_bound_map_chain(inputs.clone(), "x", VarSort::ProcSort);

        let parser = rholang_parser::RholangParser::new();

        let inner_proc = ParBuilderUtil::create_ast_proc_var("x", &parser);
        let inner_bundle =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleRead, inner_proc, &parser);
        let outer_bundle =
            ParBuilderUtil::create_ast_bundle(BundleType::BundleReadWrite, inner_bundle, &parser);

        let expected_result = inputs
            .par
            .with_bundles(vec![Bundle {
                body: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                write_flag: false, // Read-only because ReadWrite AND Read = Read
                read_flag: true,
            }])
            .with_locally_free(create_bit_vector(&vec![0]));

        let result = normalize_ann_proc(&outer_bundle, bound_inputs.clone(), &env, &parser);

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, bound_inputs.free_map);
    }
}
