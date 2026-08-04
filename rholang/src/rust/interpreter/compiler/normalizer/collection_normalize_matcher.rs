use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, ETuple, Expr, Par, Var};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Collection, KeyValuePair};

use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::exports::{FreeMap, ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{
    CollectKind, NormKont, NormVal, NormWork, Step,
};
use crate::rust::interpreter::compiler::normalizer::remainder_normalizer_matcher::normalize_remainder;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use crate::rust::interpreter::util::prepend_expr;

// ===========================================================================
// ★ THE REPRODUCER'S OWN PATH
// ===========================================================================
//
// `normalize_ann_proc → normalize_p_collect → normalize_collection → fold_match
//  → normalize_ann_proc` is the four-frame cycle a `gdb` backtrace showed at the
// SIGSEGV of `[`×288 `0` `]`×288, at a dead-constant 43,520 bytes per level
// (audit §12.2). All four frames are now one `Step`: `normalize_p_collect`'s
// `prepend_expr`, `normalize_collection`'s constructor selection and
// `fold_match`'s accumulators are FUSED into a single continuation, so a
// bracket level costs one heap `NormKont` and no native stack at all.

/// The four collection constructors, as a function of the data
/// [`CollectKind`] carries.
///
/// The recursive form built these as closures capturing `optional_remainder`;
/// a closure cannot travel on a work stack, so the capture became a field and
/// the bodies moved here **unchanged**, including the `connective_used |=
/// remainder.is_some()` fixups each one applies after construction.
#[inline(never)]
pub(crate) fn build_collection_expr(
    kind: &CollectKind,
    ps: Vec<Par>,
    locally_free: Vec<u8>,
    connective_used: bool,
) -> Expr {
    match kind {
        CollectKind::List { remainder } => {
            let mut tmp_e_list = EList {
                ps,
                locally_free,
                connective_used,
                remainder: remainder.clone(),
            };

            tmp_e_list.connective_used = tmp_e_list.connective_used || remainder.is_some();
            Expr {
                expr_instance: Some(ExprInstance::EListBody(tmp_e_list)),
            }
        }

        CollectKind::Tuple => {
            let tmp_tuple = ETuple {
                ps,
                locally_free,
                connective_used,
            };

            Expr {
                expr_instance: Some(ExprInstance::ETupleBody(tmp_tuple)),
            }
        }

        CollectKind::Set { remainder } => {
            let mut tmp_par_set = ParSet {
                ps: SortedParHashSet::create_from_vec(ps),
                locally_free,
                connective_used,
                remainder: remainder.clone(),
            };

            tmp_par_set.connective_used = tmp_par_set.connective_used || remainder.is_some();

            let eset = ParSetTypeMapper::par_set_to_eset(tmp_par_set);

            Expr {
                expr_instance: Some(ExprInstance::ESetBody(eset)),
            }
        }

        CollectKind::PathMap { remainder } => {
            // EPathMap fix P3 (PM-2): constructor instead of a
            // struct literal (private shadow cell). The value is
            // FRESH (never interned), so the field write below stays
            // sound under the shadow-cell invariant.
            let mut tmp_e_pathmap =
                EPathMap::new(ps, locally_free, connective_used, remainder.clone());

            tmp_e_pathmap.connective_used = tmp_e_pathmap.connective_used || remainder.is_some();
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(tmp_e_pathmap)),
            }
        }
    }
}

/// A collection literal, descend half.
///
/// Selects the constructor, normalizes the `...remainder` (which is a leaf — it
/// only consults the free map), and schedules the **first** element. An empty
/// collection has no children and finishes immediately.
#[inline(never)]
pub(crate) fn descend_collection<'ast>(
    proc: &'ast Collection<'ast>,
    input: ProcVisitInputs,
) -> Result<Step<'ast>, InterpreterError> {
    let (kind, known_free, elements): (CollectKind, FreeMap<VarSort>, &'ast [AnnProc<'ast>]) =
        match proc {
            Collection::List {
                elements,
                remainder,
            } => {
                let (optional_remainder, known_free) =
                    normalize_remainder(remainder, input.free_map.clone())?;
                (
                    CollectKind::List {
                        remainder: optional_remainder,
                    },
                    known_free,
                    elements.as_slice(),
                )
            }

            Collection::Tuple(elements) => (
                CollectKind::Tuple,
                input.free_map.clone(),
                elements.as_slice(),
            ),

            Collection::Set {
                elements,
                remainder,
            } => {
                let (optional_remainder, known_free) =
                    normalize_remainder(remainder, input.free_map.clone())?;
                (
                    CollectKind::Set {
                        remainder: optional_remainder,
                    },
                    known_free,
                    elements.as_slice(),
                )
            }

            Collection::PathMap {
                elements,
                remainder,
            } => {
                let (optional_remainder, known_free) =
                    normalize_remainder(remainder, input.free_map.clone())?;
                (
                    CollectKind::PathMap {
                        remainder: optional_remainder,
                    },
                    known_free,
                    elements.as_slice(),
                )
            }

            Collection::Map {
                elements,
                remainder,
            } => {
                let (optional_remainder, known_free) =
                    normalize_remainder(remainder, input.free_map.clone())?;
                return Ok(descend_collect_map(
                    optional_remainder,
                    elements.as_slice(),
                    known_free,
                    input,
                ));
            }
        };

    let input_depth = input.bound_map_chain.depth() as i32;
    let bound_map_chain = input.bound_map_chain;

    let Some(first) = elements.first().copied() else {
        // `fold_match` over an empty element list runs its constructor on empty
        // accumulators and returns the entry free map untouched.
        let expr = build_collection_expr(&kind, Vec::new(), Vec::new(), false);
        return Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
            par: prepend_expr(input.par, expr, input_depth),
            free_map: known_free,
        })));
    };

    Ok(Step::Descend {
        kont: NormKont::Collect {
            kind,
            elements,
            idx: 1,
            acc_pars: Vec::with_capacity(elements.len()),
            locally_free: Vec::new(),
            connective_used: false,
            known_free: known_free.clone(),
            bound_map_chain: bound_map_chain.clone(),
            input_par: input.par,
            input_depth,
        },
        work: NormWork::Proc {
            proc: first,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map: known_free,
            },
        },
    })
}

/// A collection literal, combine half — `fold_match`'s loop body.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn combine_collect<'ast>(
    kind: CollectKind,
    elements: &'ast [AnnProc<'ast>],
    idx: usize,
    mut acc_pars: Vec<Par>,
    locally_free: Vec<u8>,
    connective_used: bool,
    known_free: FreeMap<VarSort>,
    bound_map_chain: BoundMapChain<VarSort>,
    input_par: Par,
    input_depth: i32,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let mut result = value.into_proc();

    // ★ Leg-1. `<Par as Clone>::clone` is itself a Θ(depth) NATIVE-STACK
    // traversal (15,850 B/level debug, 2,852 release — audit §5 row 5), so a
    // single deep clone anywhere in this traversal re-imposes the ceiling the
    // conversion just removed. It did: with the recursion gone the probe read
    // 15,850 B/level, which is that constant exactly. The recursive form cloned
    // the child `Par` into the accumulator and then transferred
    // `locally_free` out of the original. Preserve that transfer while deleting
    // the deep clone: the enclosing collection owns the cache; the nested
    // child must not retain a duplicate.
    let child_locally_free = std::mem::take(&mut result.par.locally_free);
    let child_connective_used = result.par.connective_used;
    acc_pars.push(result.par);
    let result_known_free = result.free_map;
    let locally_free = union(locally_free, child_locally_free);
    let connective_used = connective_used || child_connective_used;

    match elements.get(idx).copied() {
        Some(next) => Ok(Step::Descend {
            kont: NormKont::Collect {
                kind,
                elements,
                idx: idx + 1,
                acc_pars,
                locally_free,
                connective_used,
                known_free: result_known_free.clone(),
                bound_map_chain: bound_map_chain.clone(),
                input_par,
                input_depth,
            },
            work: NormWork::Proc {
                proc: next,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: result_known_free,
                },
            },
        }),
        None => {
            let _ = known_free;
            let expr = build_collection_expr(&kind, acc_pars, locally_free, connective_used);
            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: prepend_expr(input_par, expr, input_depth),
                free_map: result_known_free,
            })))
        }
    }
}

/// A map literal, descend half — `fold_match_map`'s prologue.
#[inline(never)]
fn descend_collect_map<'ast>(
    remainder: Option<Var>,
    pairs: &'ast [KeyValuePair<'ast>],
    known_free: FreeMap<VarSort>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    let input_depth = input.bound_map_chain.depth() as i32;
    let bound_map_chain = input.bound_map_chain;

    let Some(first) = pairs.first() else {
        let expr = build_map_expr(&remainder, Vec::new(), Vec::new(), false);
        return Step::Done(NormVal::Proc(ProcVisitOutputs {
            par: prepend_expr(input.par, expr, input_depth),
            free_map: known_free,
        }));
    };

    Step::Descend {
        kont: NormKont::CollectMap {
            remainder,
            pairs,
            idx: 0,
            on_value: false,
            key_par: None,
            acc_pairs: Vec::with_capacity(pairs.len()),
            locally_free: Vec::new(),
            connective_used: false,
            known_free: known_free.clone(),
            bound_map_chain: bound_map_chain.clone(),
            input_par: input.par,
            input_depth,
        },
        work: NormWork::Proc {
            proc: first.0,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain,
                free_map: known_free,
            },
        },
    }
}

/// `fold_match_map`'s terminal expression, extracted so the empty-map case and
/// the general case build it identically.
#[inline(never)]
fn build_map_expr(
    remainder: &Option<Var>,
    acc_pairs: Vec<(Par, Par)>,
    locally_free: Vec<u8>,
    connective_used: bool,
) -> Expr {
    let remainder_connective_used = match remainder {
        Some(var) => var.connective_used(var.clone()),
        None => false,
    };

    let remainder_locally_free = match remainder {
        Some(var) => var.locally_free(var.clone(), 0),
        None => Vec::new(),
    };

    Expr {
        expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap {
                ps: SortedParMap::create_from_vec(acc_pairs.into_iter().rev().collect()),
                connective_used: connective_used || remainder_connective_used,
                locally_free: union(locally_free, remainder_locally_free),
                remainder: remainder.clone(),
            },
        ))),
    }
}

/// A map literal, combine half — two children per pair, key **then** value, with
/// the key's free map threaded into the value's input.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn combine_collect_map<'ast>(
    remainder: Option<Var>,
    pairs: &'ast [KeyValuePair<'ast>],
    idx: usize,
    on_value: bool,
    key_par: Option<Par>,
    mut acc_pairs: Vec<(Par, Par)>,
    locally_free: Vec<u8>,
    connective_used: bool,
    known_free: FreeMap<VarSort>,
    bound_map_chain: BoundMapChain<VarSort>,
    input_par: Par,
    input_depth: i32,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let mut result = value.into_proc();

    if !on_value {
        // The KEY has just finished: schedule this pair's value against the
        // key's free map.
        debug_assert!(
            key_par.is_none(),
            "combine_collect_map: a key slot was already occupied"
        );
        return Ok(Step::Descend {
            kont: NormKont::CollectMap {
                remainder,
                pairs,
                idx,
                on_value: true,
                key_par: Some(result.par),
                acc_pairs,
                locally_free,
                connective_used,
                known_free,
                bound_map_chain: bound_map_chain.clone(),
                input_par,
                input_depth,
            },
            work: NormWork::Proc {
                proc: pairs[idx].1,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: result.free_map,
                },
            },
        });
    }

    // The VALUE has just finished: close the pair.
    let mut key = key_par.expect("combine_collect_map: a value arrived with no key");
    // ★ Leg-1. `<Par as Clone>::clone` is itself a Θ(depth) NATIVE-STACK
    // traversal (15,850 B/level debug, 2,852 release — audit §5 row 5), so a
    // single deep clone anywhere in this traversal re-imposes the ceiling the
    // conversion just removed. It did: with the recursion gone the probe read
    // 15,850 B/level, which is that constant exactly. The recursive form cloned
    // each child `Par` into the accumulator and then transferred
    // `locally_free` out of the originals. Preserve that transfer while
    // deleting the deep clones.
    let key_locally_free = std::mem::take(&mut key.locally_free);
    let key_connective_used = key.connective_used;
    let value_locally_free = std::mem::take(&mut result.par.locally_free);
    let value_connective_used = result.par.connective_used;
    acc_pairs.push((key, result.par));
    let result_known_free = result.free_map;
    let locally_free = union(locally_free, union(key_locally_free, value_locally_free));
    let connective_used = connective_used || key_connective_used || value_connective_used;

    match pairs.get(idx + 1) {
        Some(next) => Ok(Step::Descend {
            kont: NormKont::CollectMap {
                remainder,
                pairs,
                idx: idx + 1,
                on_value: false,
                key_par: None,
                acc_pairs,
                locally_free,
                connective_used,
                known_free: result_known_free.clone(),
                bound_map_chain: bound_map_chain.clone(),
                input_par,
                input_depth,
            },
            work: NormWork::Proc {
                proc: next.0,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: result_known_free,
                },
            },
        }),
        None => {
            let _ = known_free;
            let expr = build_map_expr(&remainder, acc_pairs, locally_free, connective_used);
            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: prepend_expr(input_par, expr, input_depth),
                free_map: result_known_free,
            })))
        }
    }
}

//rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/CollectMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::{KeyValuePair, Par};
    use models::rust::utils::{
        new_boundvar_par, new_elist_expr, new_emap_expr, new_eplus_par, new_eset_expr,
        new_etuple_expr, new_freevar_expr, new_freevar_par, new_freevar_var, new_gint_par,
        new_gstring_par,
    };
    use pretty_assertions::assert_eq;
    use rholang_parser::SourcePos;

    use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
    use crate::rust::interpreter::compiler::normalize::VarSort::{NameSort, ProcSort};
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
    use crate::rust::interpreter::test_utils::utils::collection_proc_visit_inputs_and_env;
    use crate::rust::interpreter::util::prepend_expr;

    fn get_normalized_par(rho: &str) -> Par {
        ParBuilderUtil::mk_term(rho).expect("Compilation failed to normalize Par")
    }

    pub fn assert_equal_normalized(rho1: &str, rho2: &str) {
        assert_eq!(
            get_normalized_par(rho1),
            get_normalized_par(rho2),
            "Normalized Par values are not equal"
        );
    }

    #[test]
    fn list_should_delegate() {
        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create list: [P, *x, 7]
        let proc = ParBuilderUtil::create_ast_list(
            vec![
                ParBuilderUtil::create_ast_proc_var("P", &parser),
                ParBuilderUtil::create_ast_eval_name_var("x", &parser),
                ParBuilderUtil::create_ast_long_literal(7, &parser),
            ],
            None,
            &parser,
        );

        let result = normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let expected_result = prepend_expr(
            inputs.par.clone(),
            new_elist_expr(
                vec![
                    new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                    new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                    new_gint_par(7, Vec::new(), false),
                ],
                create_bit_vector(&vec![0, 1]),
                false,
                None,
            ),
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.clone().unwrap().free_map, inputs.free_map);
    }

    #[test]
    fn list_should_sort_the_insides_of_their_elements() {
        assert_equal_normalized("@0!([{1 | 2}])", "@0!([{2 | 1}])");
    }

    #[test]
    fn list_should_sort_the_insides_of_send_encoded_as_byte_array() {
        let rho1 = r#"
        new x in {
          x!(
            [
              @"a"!(
                @"x"!("abc") |
                @"y"!(1)
              )
            ].toByteArray()
          )
        }
    "#;

        let rho2 = r#"
        new x in {
          x!(
            [
              @"a"!(
                @"y"!(1) |
                @"x"!("abc")
              )
            ].toByteArray()
          )
        }
    "#;
        assert_equal_normalized(&rho1, &rho2);
    }

    #[test]
    fn tuple_should_delegate() {
        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create tuple: (*y, Q)
        let proc = ParBuilderUtil::create_ast_tuple(
            vec![
                ParBuilderUtil::create_ast_eval_name_var("y", &parser),
                ParBuilderUtil::create_ast_proc_var("Q", &parser),
            ],
            &parser,
        );

        let result = normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let expected_result = prepend_expr(
            inputs.par.clone(),
            new_etuple_expr(
                vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                Vec::new(),
                true,
            ),
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.clone().unwrap().free_map,
            inputs.free_map.put_all_pos(vec![
                ("y".to_string(), NameSort, SourcePos { line: 0, col: 0 }),
                ("Q".to_string(), ProcSort, SourcePos { line: 0, col: 0 })
            ])
        )
    }

    #[test]
    fn tuple_should_propagate_free_variables() {
        let (inputs, env) = collection_proc_visit_inputs_and_env();
        let parser = rholang_parser::RholangParser::new();

        // Create elements: 7, (7 | Q), Q
        let elem1 = ParBuilderUtil::create_ast_long_literal(7, &parser);
        let par_left = ParBuilderUtil::create_ast_long_literal(7, &parser);
        let par_right = ParBuilderUtil::create_ast_proc_var("Q", &parser);
        let elem2 = ParBuilderUtil::create_ast_par(par_left, par_right, &parser);
        let elem3 = ParBuilderUtil::create_ast_proc_var("Q", &parser);

        // Create tuple: (7, (7 | Q), Q)
        let proc = ParBuilderUtil::create_ast_tuple(vec![elem1, elem2, elem3], &parser);

        let result = normalize_ann_proc(&proc, inputs.clone(), &env, &parser);

        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedReuseOfProcContextFree { .. })
        ));
    }

    #[test]
    fn tuple_should_sort_the_insides_of_their_elements() {
        assert_equal_normalized("@0!(({1 | 2}))", "@0!(({2 | 1}))");
    }

    #[test]
    fn set_should_delegate() {
        let parser = rholang_parser::RholangParser::new();
        let (inputs, env) = collection_proc_visit_inputs_and_env();

        let proc = ParBuilderUtil::create_ast_set(
            vec![
                ParBuilderUtil::create_ast_add_with_par_of_var("P", "R", &parser),
                ParBuilderUtil::create_ast_int(7, &parser),
                ParBuilderUtil::create_ast_par_with_int_and_var(8, "Q", &parser),
            ],
            Some(ParBuilderUtil::create_ast_var("Z")),
            &parser,
        );

        let parser = rholang_parser::RholangParser::new();
        let result = normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let expected_result = prepend_expr(
            inputs.par.clone(),
            new_eset_expr(
                vec![
                    new_eplus_par(
                        new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                        new_freevar_par(1, Vec::new()),
                    ),
                    new_gint_par(7, Vec::new(), false),
                    prepend_expr(new_gint_par(8, Vec::new(), false), new_freevar_expr(2), 0),
                ],
                create_bit_vector(&vec![1]),
                true,
                Some(new_freevar_var(0)),
            ),
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.unwrap().free_map,
            inputs.free_map.put_all_pos(vec![
                ("Z".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
                ("R".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
                ("Q".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
            ])
        );
    }

    #[test]
    fn set_should_sort_the_insides_of_their_elements() {
        assert_equal_normalized("@0!(Set({1 | 2}))", "@0!(Set({2 | 1}))")
    }

    #[test]
    fn map_should_delegate() {
        let parser = rholang_parser::RholangParser::new();
        let (inputs, env) = collection_proc_visit_inputs_and_env();

        let proc = ParBuilderUtil::create_ast_map(
            vec![
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_int(7, &parser),
                    ParBuilderUtil::create_ast_string("Seven", &parser),
                ),
                ParBuilderUtil::create_ast_key_value_pair(
                    ParBuilderUtil::create_ast_proc_var("P", &parser),
                    ParBuilderUtil::create_ast_eval_name_var("Q", &parser),
                ),
            ],
            Some(ParBuilderUtil::create_ast_var("Z")),
            &parser,
        );

        let parser = rholang_parser::RholangParser::new();
        let result = normalize_ann_proc(&proc, inputs.clone(), &env, &parser);
        let expected_result = prepend_expr(
            inputs.par.clone(),
            new_emap_expr(
                vec![
                    KeyValuePair {
                        key: Some(new_gint_par(7, Vec::new(), false)),
                        value: Some(new_gstring_par("Seven".parse().unwrap(), Vec::new(), false)),
                    },
                    KeyValuePair {
                        key: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
                        value: Some(new_freevar_par(1, Vec::new())),
                    },
                ],
                create_bit_vector(&vec![1]),
                true,
                Some(new_freevar_var(0)),
            ),
            0,
        );

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.unwrap().free_map,
            inputs.free_map.put_all_pos(vec![
                ("Z".to_string(), ProcSort, SourcePos { line: 0, col: 0 }),
                ("Q".to_string(), NameSort, SourcePos { line: 0, col: 0 }),
            ])
        );
    }

    #[test]
    fn map_should_sort_the_insides_of_their_keys() {
        assert_equal_normalized("@0!({{1 | 2} : 0})", "@0!({{2 | 1} : 0})")
    }

    #[test]
    fn map_should_sort_the_insides_of_their_values() {
        assert_equal_normalized("@0!({0 : {1 | 2}})", "@0!({0 : {2 | 1}})")
    }

    #[test]
    fn pathmap_should_parse_empty() {
        let par = get_normalized_par("{||}");
        assert!(par.exprs.len() == 1);
    }

    #[test]
    fn pathmap_should_parse_single_element() {
        let par = get_normalized_par("{| 42 |}");
        assert!(par.exprs.len() == 1);
    }

    #[test]
    fn pathmap_should_parse_multiple_elements() {
        let par = get_normalized_par("{| 1, 2, 3 |}");
        assert!(par.exprs.len() == 1);
    }

    #[test]
    fn pathmap_should_sort_the_insides_of_their_elements() {
        assert_equal_normalized("@0!({|{1 | 2}|})", "@0!({|{2 | 1}|})");
    }
}
