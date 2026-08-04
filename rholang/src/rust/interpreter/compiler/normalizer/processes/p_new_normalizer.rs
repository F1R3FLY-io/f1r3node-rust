use std::collections::{BTreeMap, HashMap};

use models::rhoapi::{New, Par};
use rholang_parser::ast::{AnnProc, NameDecl};
use rholang_parser::SourcePos;

use crate::rust::interpreter::compiler::exports::{
    BoundMapChain, IdContextPos, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::{filter_and_adjust_bitset, prepend_new};

/// `new x, y in { P }`, descend half.
///
/// This is the machine's **binder-scoping** discipline in its simplest form:
/// the declarations are sorted and pushed onto the `bound_map_chain`, the body
/// descends under the extended chain, and the scope is left behind simply by
/// *not* carrying it in the continuation — the enclosing frames still hold
/// their own chains, so there is nothing to pop. Where the recursive form
/// relied on stack unwinding to restore scope, the machine relies on each
/// continuation owning the chain it will resume with.
#[inline(never)]
pub(crate) fn descend_p_new<'ast>(
    decls: &[NameDecl<'ast>],
    proc: AnnProc<'ast>,
    input: ProcVisitInputs,
) -> Result<Step<'ast>, InterpreterError> {
    // TODO: bindings within a single new shouldn't have overlapping names. - OLD
    let new_tagged_bindings: Vec<(Option<String>, String, VarSort, usize, usize)> = decls
        .iter()
        .map(|decl| match decl {
            NameDecl { id, uri: None } => Ok((
                None,
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            )),
            NameDecl { id, uri: Some(urn) } => Ok((
                Some((**urn).to_string()), // Dereference Uri to get the inner &str
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            )),
        })
        .collect::<Result<Vec<_>, InterpreterError>>()?;

    // Sort bindings: None's first, then URI's lexicographically
    let mut sorted_bindings: Vec<(Option<String>, String, VarSort, usize, usize)> =
        new_tagged_bindings;
    sorted_bindings.sort_by(|a, b| a.0.cmp(&b.0));

    let new_bindings: Vec<IdContextPos<VarSort>> = sorted_bindings
        .iter()
        .map(|row| {
            (row.1.clone(), row.2.clone(), SourcePos {
                line: row.3,
                col: row.4,
            })
        })
        .collect();

    let uris: Vec<String> = sorted_bindings
        .iter()
        .filter_map(|row| row.0.clone())
        .collect();

    let new_env: BoundMapChain<VarSort> = input.bound_map_chain.put_all_pos(new_bindings);
    let new_count: usize = new_env.get_count() - input.bound_map_chain.get_count();

    Ok(Step::Descend {
        kont: NormKont::New {
            new_count,
            uris,
            input_par: input.par,
        },
        work: NormWork::Proc {
            proc,
            input: ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: new_env,
                free_map: input.free_map,
            },
        },
    })
}

/// `new x, y in { P }`, combine half.
#[inline(never)]
pub(crate) fn combine_p_new<'ast>(
    new_count: usize,
    uris: Vec<String>,
    input_par: Par,
    value: NormVal,
    env: &HashMap<String, Par>,
) -> Result<Step<'ast>, InterpreterError> {
    let mut body_result = value.into_proc();

    // TODO: we should build btree_map with real values, not a copied references from env: ref &HashMap
    let btree_map: BTreeMap<String, Par> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    // Transfer the cache to `New::locally_free`, matching the recursive
    // implementation. The nested body must not retain a duplicate cache.
    let body_locally_free = std::mem::take(&mut body_result.par.locally_free);
    let result_new = New {
        bind_count: new_count as i32,
        p: Some(body_result.par),
        uri: uris,
        injections: btree_map,
        locally_free: filter_and_adjust_bitset(body_locally_free, new_count),
    };

    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: prepend_new(input_par, result_new),
        free_map: body_result.free_map,
    })))
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::create_bit_vector;
    use models::rhoapi::{New, Par};
    use models::rust::utils::{new_boundvar_par, new_gint_par, new_send};

    use crate::rust::interpreter::test_utils::utils::proc_visit_inputs_and_env;
    use crate::rust::interpreter::util::prepend_new;

    #[test]
    fn p_new_should_bind_new_variables() {
        use rholang_parser::ast::{Id, NameDecl, SendType};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create declarations: x, y, z
        let decls = vec![
            NameDecl {
                id: Id {
                    name: "x",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
            NameDecl {
                id: Id {
                    name: "y",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
            NameDecl {
                id: Id {
                    name: "z",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
        ];

        // Create sends: x!(7), y!(8), z!(9)
        let x_channel = ParBuilderUtil::create_ast_name_var("x");
        let send_x = ParBuilderUtil::create_ast_send(
            x_channel,
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(7, &parser)],
            &parser,
        );

        let y_channel = ParBuilderUtil::create_ast_name_var("y");
        let send_y = ParBuilderUtil::create_ast_send(
            y_channel,
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(8, &parser)],
            &parser,
        );

        let z_channel = ParBuilderUtil::create_ast_name_var("z");
        let send_z = ParBuilderUtil::create_ast_send(
            z_channel,
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(9, &parser)],
            &parser,
        );

        // Create Par: (x!(7) | y!(8)) | z!(9)
        let par_xy = ParBuilderUtil::create_ast_par(send_x, send_y, &parser);
        let par_xyz = ParBuilderUtil::create_ast_par(par_xy, send_z, &parser);

        // Create new x, y, z in { ... }
        let p_new = ParBuilderUtil::create_ast_new_with_decls(decls, par_xyz, &parser);

        let result = normalize_ann_proc(
            &p_new,
            proc_visit_inputs_and_env().0,
            &proc_visit_inputs_and_env().1,
            &parser,
        );
        assert!(result.is_ok());

        let expected_result = prepend_new(Par::default(), New {
            bind_count: 3,
            p: Some(
                Par::default()
                    .prepend_send(new_send(
                        new_boundvar_par(2, create_bit_vector(&vec![2]), false),
                        vec![new_gint_par(7, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![2]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                        vec![new_gint_par(8, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![1]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                        vec![new_gint_par(9, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![0]),
                        false,
                    )),
            ),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.unwrap().free_map,
            proc_visit_inputs_and_env().0.free_map
        );
    }

    #[test]
    fn p_new_should_sort_uris_and_place_them_at_the_end() {
        use rholang_parser::ast::{Id, NameDecl, SendType, Uri};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        // Create declarations: x, y, r (with URI), out (with URI), z
        let decls = vec![
            NameDecl {
                id: Id {
                    name: "x",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
            NameDecl {
                id: Id {
                    name: "y",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
            NameDecl {
                id: Id {
                    name: "r",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: Some(Uri::from("rho:registry")),
            },
            NameDecl {
                id: Id {
                    name: "out",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: Some(Uri::from("rho:stdout")),
            },
            NameDecl {
                id: Id {
                    name: "z",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: None,
            },
        ];

        // Create sends: x!(7), y!(8), r!(9), out!(10), z!(11)
        let send_x = ParBuilderUtil::create_ast_send(
            ParBuilderUtil::create_ast_name_var("x"),
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(7, &parser)],
            &parser,
        );

        let send_y = ParBuilderUtil::create_ast_send(
            ParBuilderUtil::create_ast_name_var("y"),
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(8, &parser)],
            &parser,
        );

        let send_r = ParBuilderUtil::create_ast_send(
            ParBuilderUtil::create_ast_name_var("r"),
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(9, &parser)],
            &parser,
        );

        let send_out = ParBuilderUtil::create_ast_send(
            ParBuilderUtil::create_ast_name_var("out"),
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(10, &parser)],
            &parser,
        );

        let send_z = ParBuilderUtil::create_ast_send(
            ParBuilderUtil::create_ast_name_var("z"),
            SendType::Single,
            vec![ParBuilderUtil::create_ast_long_literal(11, &parser)],
            &parser,
        );

        // Create nested Par: ((((x!(7) | y!(8)) | r!(9)) | out!(10)) | z!(11))
        let par_xy = ParBuilderUtil::create_ast_par(send_x, send_y, &parser);
        let par_xyr = ParBuilderUtil::create_ast_par(par_xy, send_r, &parser);
        let par_xyro = ParBuilderUtil::create_ast_par(par_xyr, send_out, &parser);
        let par_xyroz = ParBuilderUtil::create_ast_par(par_xyro, send_z, &parser);

        // Create new x, y, r, out, z in { ... }
        let p_new = ParBuilderUtil::create_ast_new_with_decls(decls, par_xyroz, &parser);

        let result = normalize_ann_proc(
            &p_new,
            proc_visit_inputs_and_env().0,
            &proc_visit_inputs_and_env().1,
            &parser,
        );
        assert!(result.is_ok());

        let expected_result = prepend_new(Par::default(), New {
            bind_count: 5,
            p: Some(
                Par::default()
                    .prepend_send(new_send(
                        new_boundvar_par(4, create_bit_vector(&vec![4]), false),
                        vec![new_gint_par(7, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![4]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(3, create_bit_vector(&vec![3]), false),
                        vec![new_gint_par(8, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![3]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                        vec![new_gint_par(9, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![1]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(0, create_bit_vector(&vec![0]), false),
                        vec![new_gint_par(10, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![0]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(2, create_bit_vector(&vec![2]), false),
                        vec![new_gint_par(11, Vec::new(), false)],
                        false,
                        create_bit_vector(&vec![2]),
                        false,
                    )),
            ),
            uri: vec!["rho:registry".to_string(), "rho:stdout".to_string()],
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(
            result.clone().unwrap().par.news[0]
                .p
                .clone()
                .unwrap()
                .sends
                .iter()
                .map(|x| x.locally_free.clone())
                .collect::<Vec<Vec<u8>>>(),
            vec![
                create_bit_vector(&vec![2]),
                create_bit_vector(&vec![0]),
                create_bit_vector(&vec![1]),
                create_bit_vector(&vec![3]),
                create_bit_vector(&vec![4])
            ]
        );
        // The child aggregate is transferred to `New::locally_free` and then
        // adjusted for the five newly bound names. Individual sends retain
        // their own caches; the nested `Par` must not duplicate the aggregate.
        assert_eq!(
            result.unwrap().par.news[0].p.clone().unwrap().locally_free,
            Vec::<u8>::new()
        );
    }
}
