// See rholang/src/main/scala/coop/rchain/rholang/interpreter/compiler/Compiler.scala

use std::collections::HashMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::Par;
use models::rust::rholang::par_children::dismantle;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

use super::free_map::FreeMap;
use super::normalize::{normalize_ann_proc, VarSort};
use crate::rust::interpreter::compiler::exports::ProcVisitInputs;
use crate::rust::interpreter::errors::InterpreterError;

pub struct Compiler;

impl Compiler {
    pub fn source_to_adt(source: &str) -> Result<Par, InterpreterError> {
        Self::source_to_adt_with_normalizer_env(source, HashMap::new())
    }

    pub fn source_to_adt_with_normalizer_env(
        source: &str,
        normalizer_env: HashMap<String, Par>,
    ) -> Result<Par, InterpreterError> {
        let parser = rholang_parser::RholangParser::new();
        let result = parser.parse(source);

        match result {
            validated::Validated::Good(procs) => {
                if procs.len() == 1 {
                    let proc = procs.into_iter().next().unwrap();
                    Self::normalize_term(proc, normalizer_env, &parser)
                } else {
                    Err(InterpreterError::ParserError(format!(
                        "Expected single process, got {}",
                        procs.len()
                    )))
                }
            }
            validated::Validated::Fail(failures) => {
                // Convert parsing failures to InterpreterError
                let error_messages: Vec<String> = failures
                    .iter()
                    .flat_map(|failure| {
                        failure
                            .errors
                            .iter()
                            .map(|error| format!("{:?} at {:?}", error.error, error.span))
                    })
                    .collect();
                Err(InterpreterError::ParserError(format!(
                    "Parse failed: {}",
                    error_messages.join(", ")
                )))
            }
        }
    }

    fn normalize_term<'a>(
        ast: rholang_parser::ast::AnnProc<'a>,
        normalizer_env: HashMap<String, Par>,
        parser: &'a rholang_parser::RholangParser<'a>,
    ) -> Result<Par, InterpreterError> {
        let normalized_result =
            normalize_ann_proc(&ast, ProcVisitInputs::new(), &normalizer_env, parser)?;

        // ★ Stage E-1, applied to the normalizer.
        //
        // `normalize_term` is `normalize_ann_proc` followed by
        // `ParSortMatcher::sort_match`, and the sorter READS its input and
        // BUILDS a fresh term — so the un-sorted intermediate falls out of scope
        // here. `<Par as Drop>` is a *derived* Θ(depth) recursive traversal
        // (measured at 434 B/level debug on this very subject, once the machine
        // had removed the 43,542), which would have left the deploy path with a
        // depth ceiling after the traversal itself was heap-bounded. Both exits
        // therefore hand the intermediate to `par_children::dismantle`, which
        // tears it down with an explicit worklist.
        //
        // This is the same residual and the same fix as `b98fa20a` for
        // `substitute`; see audit §12.1 row 3 and §12.5 vacuity instance #4.
        if normalized_result.free_map.count() > 0 {
            dismantle(normalized_result.par);
            return Err(Self::top_level_error(normalized_result.free_map));
        }

        let sorted_par = ParSortMatcher::sort_match(&normalized_result.par);
        dismantle(normalized_result.par);
        Ok(sorted_par.term)
    }

    /// The three top-level rejections — a non-empty free map is connectives,
    /// wildcards, or level bindings, checked in that order.
    ///
    /// Split out of [`Compiler::normalize_term`] so the un-sorted intermediate
    /// can be dismantled *before* the message is built; the message depends only
    /// on the free map, never on the term.
    fn top_level_error(free_map: FreeMap<VarSort>) -> InterpreterError {
        if !free_map.connectives.is_empty() {
            fn connective_instance_to_string(conn: ConnectiveInstance) -> String {
                match conn {
                    ConnectiveInstance::ConnAndBody(_) => String::from("/\\ (conjunction)"),
                    ConnectiveInstance::ConnOrBody(_) => String::from("\\/ (disjunction)"),
                    ConnectiveInstance::ConnNotBody(_) => String::from("~ (negation)"),
                    _ => format!("{:?}", conn),
                }
            }

            let connectives: Vec<String> = free_map
                .connectives
                .into_iter()
                .map(|(conn_type, source_position)| {
                    format!(
                        "{} at {}",
                        connective_instance_to_string(conn_type),
                        source_position
                    )
                })
                .collect();

            InterpreterError::TopLevelLogicalConnectivesNotAllowedError(connectives.join(", "))
        } else if !free_map.wildcards.is_empty() {
            let top_level_wildcard_list: Vec<String> = free_map
                .wildcards
                .into_iter()
                .map(|source_position| format!("_ (wildcard) at {}", source_position))
                .collect();

            InterpreterError::TopLevelWildcardsNotAllowedError(top_level_wildcard_list.join(", "))
        } else {
            let free_variable_list: Vec<String> = free_map
                .level_bindings
                .into_iter()
                .map(|(var_name, var_sort)| format!("{} at {:?}", var_name, var_sort.source_span))
                .collect();

            InterpreterError::TopLevelFreeVariablesNotAllowedError(free_variable_list.join(", "))
        }
    }
}
