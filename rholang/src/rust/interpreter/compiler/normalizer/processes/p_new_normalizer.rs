use std::collections::{BTreeMap, HashMap};

use models::create_bit_vector;
use models::rhoapi::{Bundle, Match, MatchCase, New, Par, Receive, ReceiveBind, Send};
use models::rust::utils::{
    new_boundvar_par, new_freevar_par, new_gstring_par, new_wildcard_par, union,
};
use rholang_parser::ast::{AnnProc, NameDecl};
use rholang_parser::SourcePos;

use crate::rust::interpreter::compiler::exports::{
    BoundMapChain, IdContextPos, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, VarSort};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::registry::versioned_urn::is_wildcard_urn;
use crate::rust::interpreter::system_processes::FixedChannels;
use crate::rust::interpreter::util::{filter_and_adjust_bitset, prepend_new};

pub fn normalize_p_new<'ast>(
    decls: &[NameDecl<'ast>],
    proc: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Partition each binder by whether its URN is a wildcard. Static binders
    // (no URI, or an exact URN) stay on the compile-time urn_map path; wildcard
    // binders get rewritten to a dynamic registry lookup round-trip, since
    // "latest matching" cannot be baked in at genesis.
    let has_any_wildcard = decls
        .iter()
        .any(|d| d.uri.as_ref().map(|u| is_wildcard_urn(u)).unwrap_or(false));

    if !has_any_wildcard {
        return normalize_static_only(decls, proc, input, env, parser);
    }

    normalize_with_wildcards(decls, proc, input, env, parser)
}

/// Current (pre-wildcard-rewrite) behavior. Covers every binder set with no
/// wildcard URN — i.e., every URN in `urn_map` plus every unresolvable exact
/// URN (which continues to error at `eval_new` with "Unknown urn for new").
fn normalize_static_only<'ast>(
    decls: &[NameDecl<'ast>],
    proc: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
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

    let body_result = normalize_ann_proc(
        proc,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: new_env.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    // TODO: we should build btree_map with real values, not a copied references from env: ref &HashMap
    let btree_map: BTreeMap<String, Par> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    let result_new = New {
        bind_count: new_count as i32,
        p: Some(body_result.par.clone()),
        uri: uris,
        injections: btree_map,
        locally_free: filter_and_adjust_bitset(body_result.par.clone().locally_free, new_count),
    };

    Ok(ProcVisitOutputs {
        par: prepend_new(input.par.clone(), result_new),
        free_map: body_result.free_map.clone(),
    })
}

/// Rewrite each wildcard-URN binder into a dynamic lookup via
/// `rho:internal:registry_lookup`. For `new fresh*, static*, w_0(`URN_0`),
/// …, w_(n-1)(`URN_(n-1)`) in P`, with `n` wildcard binders, produces
///
/// ```text
/// new fresh*, static*, tmpRet_0, …, tmpRet_(n-1) in {
///   registryLookup!(URN_0, *tmpRet_0) | … | registryLookup!(URN_(n-1), *tmpRet_(n-1)) |
///   for (w_0 <- tmpRet_0) { match w_0 { Nil => abort(…); _ =>
///     for (w_1 <- tmpRet_1) { match w_1 { Nil => abort(…); _ =>
///       …
///       for (w_(n-1) <- tmpRet_(n-1)) { match w_(n-1) { Nil => abort(…); _ => P } }
///     } }
///   } }
/// }
/// ```
///
/// Each `match`-on-`Nil` layer halts the deploy via `rho:execution:abort`
/// when the registry returns no matching non-deprecated version.
///
/// DeBruijn indices in the body `P` reflect the full rewritten scope: the
/// outer `new` adds `static_count + wildcard_count` binders, and the
/// nested fors each add one. `P` is normalized once, in that scope, so no
/// index-shifting pass is needed.
fn normalize_with_wildcards<'ast>(
    decls: &[NameDecl<'ast>],
    proc: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Classify.
    let (static_decls, wildcard_decls): (Vec<&NameDecl>, Vec<&NameDecl>) =
        decls.iter().partition(|d| match &d.uri {
            None => true,
            Some(u) => !is_wildcard_urn(u),
        });
    let wild_count = wildcard_decls.len();
    debug_assert!(
        wild_count > 0,
        "normalize_with_wildcards called with no wildcard binders"
    );

    // Sort static binders as the fast path does: None binders first, then
    // exact-URN binders lex. This preserves the compiled-bytecode shape for
    // the static-only portion.
    let mut static_tagged: Vec<(Option<String>, String, VarSort, usize, usize)> = static_decls
        .iter()
        .map(|decl| match decl {
            NameDecl { id, uri: None } => (
                None,
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            ),
            NameDecl { id, uri: Some(urn) } => (
                Some((**urn).to_string()),
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            ),
        })
        .collect();
    static_tagged.sort_by(|a, b| a.0.cmp(&b.0));

    let static_uris: Vec<String> = static_tagged
        .iter()
        .filter_map(|row| row.0.clone())
        .collect();
    let static_count = static_tagged.len();

    // Build the chain seen by P. Order:
    //   1. static binders (sorted: Nones then exact URIs)
    //   2. tmpRet_0 .. tmpRet_(n-1)                      ← outer `new` adds these
    //   3. w_0 .. w_(n-1)                                 ← one per nested for-receive
    //
    // Earliest-added entries sit deepest; the last-added takes DeBruijn
    // depth 0. In P, the innermost for-bound `w_(n-1)` is at depth 0.
    let mut chain_bindings: Vec<IdContextPos<VarSort>> = static_tagged
        .iter()
        .map(|row| {
            (row.1.clone(), row.2.clone(), SourcePos {
                line: row.3,
                col: row.4,
            })
        })
        .collect();
    for i in 0..wild_count {
        chain_bindings.push((
            format!("__urn_lookup_ret_{i}__"),
            VarSort::NameSort,
            SourcePos { line: 0, col: 0 },
        ));
    }
    for decl in &wildcard_decls {
        chain_bindings.push((decl.id.name.to_string(), VarSort::NameSort, SourcePos {
            line: decl.id.pos.line,
            col: decl.id.pos.col,
        }));
    }

    let p_chain: BoundMapChain<VarSort> = input.bound_map_chain.put_all_pos(chain_bindings);
    let body_result = normalize_ann_proc(
        proc,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: p_chain,
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    // URNs in source order; outer-for binds wildcard_decls[0] and receives on
    // tmpRet_0, inner-for binds wildcard_decls[n-1] on tmpRet_(n-1).
    //
    // Note: `Uri::to_string()` goes through `Display`, which wraps in
    // backticks. Deref through the `Uri` newtype to the inner `&str`
    // instead (matches the static path's `(**urn).to_string()`).
    let wild_urns: Vec<String> = wildcard_decls
        .iter()
        .map(|d| match &d.uri {
            Some(urn) => (**urn).to_string(),
            None => unreachable!("wildcard binder must carry a URN"),
        })
        .collect();

    let outer_count = static_count + wild_count;

    // registry_lookup and abort channels — write-only bundles around the
    // system-process fixed channels (same shape as the corresponding
    // urn_map entries). Inlined here so the rewrite doesn't need to steal
    // user-visible binder names.
    let registry_lookup_chan = Par::default().with_bundles(vec![Bundle {
        body: Some(FixedChannels::registry_lookup()),
        write_flag: true,
        read_flag: false,
    }]);
    let abort_chan = Par::default().with_bundles(vec![Bundle {
        body: Some(FixedChannels::abort()),
        write_flag: true,
        read_flag: false,
    }]);

    // Build the body from innermost outward.
    //
    // At every nested-for level, the enclosing scope size minus the
    // tmpRet's chain-index gives a constant depth of `wild_count - 1` for
    // the source channel — each outer wrap adds exactly one new binder
    // (`w_k`) while we also advance from `tmpRet_k` to `tmpRet_(k+1)` in
    // the chain, so the depth to the respective tmpRet stays flat.
    let source_depth = (wild_count - 1) as i32;
    let source_bitset = create_bit_vector(&[wild_count - 1]);

    let mut inner: Par = body_result.par.clone();

    for k in (0..wild_count).rev() {
        // match w_k { Nil => abort!("…"); _ => inner }
        let abort_message = format!(
            "URN `{}` has no matching non-deprecated version in the registry",
            wild_urns[k]
        );
        let abort_send_par = Par::default().with_sends(vec![Send {
            chan: Some(abort_chan.clone()),
            data: vec![new_gstring_par(abort_message, Vec::new(), false)],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }]);

        // match target `w_k` sits at the newly added depth 0 within the
        // for-receive body scope.
        let match_target = new_boundvar_par(0, create_bit_vector(&[0]), false);

        // Nil pattern: an empty Par. Wildcard pattern: a Par with Wildcard.
        // Neither pattern introduces new free vars (free_count = 0 each).
        let nil_case = MatchCase {
            pattern: Some(Par::default()),
            source: Some(abort_send_par.clone()),
            free_count: 0,
            guard: None,
        };
        let default_case = MatchCase {
            pattern: Some(new_wildcard_par(Vec::new(), false)),
            source: Some(inner.clone()),
            free_count: 0,
            guard: None,
        };

        // The match's locally_free: target's lf (bit 0 for `w_k`) unioned
        // with each case source's lf. For the Nil case, abort_send_par has
        // no free vars. For the default case, `inner`'s lf refers to the
        // scope of the for-receive's body, which includes `w_k` at depth 0
        // — but the match itself consumes `w_k` as target, so if `inner`
        // references `w_k` the bit stays visible via the target. The
        // existing `p_match_normalizer` precedent folds target.locally_free
        // into the match's locally_free (see line 115 there).
        let match_lf = union(
            match_target.locally_free.clone(),
            union(
                abort_send_par.locally_free.clone(),
                inner.locally_free.clone(),
            ),
        );
        let match_par = Par::default().with_matches(vec![Match {
            target: Some(match_target),
            cases: vec![nil_case, default_case],
            locally_free: match_lf.clone(),
            connective_used: false,
        }]);

        // for (w_k <- tmpRet_k) { match_par }
        //
        // The receive's locally_free: source's lf unioned with
        // filter_and_adjust_bitset(body.lf, pattern_bound_count=1). This
        // matches `p_input_normalizer`'s pattern.
        let receive_lf = union(
            source_bitset.clone(),
            filter_and_adjust_bitset(match_par.locally_free.clone(), 1),
        );
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![new_freevar_par(0, Vec::new())],
                source: Some(new_boundvar_par(source_depth, source_bitset.clone(), false)),
                remainder: None,
                free_count: 1,
                cost_signature: None,
            }],
            body: Some(match_par),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: receive_lf.clone(),
            connective_used: false,
            condition: None,
        };
        inner = Par::default().with_receives(vec![receive]);
        inner.locally_free = receive_lf;

        // Suppress "unused" warning for k when wild_count == 1.
        let _ = k;
    }

    // Now `inner` is the chain of nested for-receives. Add Sends at the
    // outer-new scope.
    let mut outer_body = inner;
    for k in 0..wild_count {
        let tmp_depth = wild_count - 1 - k;
        let tmp_bits = create_bit_vector(&[tmp_depth]);
        let send = Send {
            chan: Some(registry_lookup_chan.clone()),
            data: vec![
                new_gstring_par(wild_urns[k].clone(), Vec::new(), false),
                new_boundvar_par(tmp_depth as i32, tmp_bits.clone(), false),
            ],
            persistent: false,
            locally_free: tmp_bits.clone(),
            connective_used: false,
        };
        outer_body.sends.push(send);
        outer_body.locally_free = union(outer_body.locally_free, tmp_bits);
    }

    // Finally wrap in the outer New.
    let btree_map: BTreeMap<String, Par> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let outer_locally_free = filter_and_adjust_bitset(outer_body.locally_free.clone(), outer_count);
    let result_new = New {
        bind_count: outer_count as i32,
        p: Some(outer_body),
        uri: static_uris,
        injections: btree_map,
        locally_free: outer_locally_free,
    };

    Ok(ProcVisitOutputs {
        par: prepend_new(input.par.clone(), result_new),
        free_map: body_result.free_map.clone(),
    })
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::create_bit_vector;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{New, Par};
    use models::rust::utils::{new_boundvar_par, new_gint_par, new_send};

    use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
    use crate::rust::interpreter::system_processes::FixedChannels;
    use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
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
                        new_boundvar_par(2, create_bit_vector(&[2]), false),
                        vec![new_gint_par(7, Vec::new(), false)],
                        false,
                        create_bit_vector(&[2]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(1, create_bit_vector(&[1]), false),
                        vec![new_gint_par(8, Vec::new(), false)],
                        false,
                        create_bit_vector(&[1]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(0, create_bit_vector(&[0]), false),
                        vec![new_gint_par(9, Vec::new(), false)],
                        false,
                        create_bit_vector(&[0]),
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
                        new_boundvar_par(4, create_bit_vector(&[4]), false),
                        vec![new_gint_par(7, Vec::new(), false)],
                        false,
                        create_bit_vector(&[4]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(3, create_bit_vector(&[3]), false),
                        vec![new_gint_par(8, Vec::new(), false)],
                        false,
                        create_bit_vector(&[3]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(1, create_bit_vector(&[1]), false),
                        vec![new_gint_par(9, Vec::new(), false)],
                        false,
                        create_bit_vector(&[1]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(0, create_bit_vector(&[0]), false),
                        vec![new_gint_par(10, Vec::new(), false)],
                        false,
                        create_bit_vector(&[0]),
                        false,
                    ))
                    .prepend_send(new_send(
                        new_boundvar_par(2, create_bit_vector(&[2]), false),
                        vec![new_gint_par(11, Vec::new(), false)],
                        false,
                        create_bit_vector(&[2]),
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
                .into_iter()
                .map(|x| x.locally_free)
                .collect::<Vec<Vec<u8>>>(),
            vec![
                create_bit_vector(&[2]),
                create_bit_vector(&[0]),
                create_bit_vector(&[1]),
                create_bit_vector(&[3]),
                create_bit_vector(&[4])
            ]
        );
        assert_eq!(
            result.unwrap().par.news[0].p.clone().unwrap().locally_free,
            create_bit_vector(&[0, 1, 2, 3, 4])
        );
    }

    /// `new foo(`rho:lib:*:abc:proj:*`) in Nil` should rewrite into a
    /// `new tmpRet in { registryLookup!(URN, *tmpRet) | for (foo <- tmpRet) {
    /// match foo { Nil => abort(…) | _ => Nil } } }` shape. We check the
    /// structural fingerprint of that shape without hand-reconstructing it
    /// bit-for-bit.
    #[test]
    fn p_new_wildcard_urn_rewrites_to_dynamic_lookup() {
        use rholang_parser::ast::{Id, NameDecl, Uri};
        use rholang_parser::SourcePos;

        let parser = rholang_parser::RholangParser::new();

        let decls = vec![NameDecl {
            id: Id {
                name: "foo",
                pos: SourcePos { line: 0, col: 0 },
            },
            uri: Some(Uri::from("rho:lib:*:abc:proj:*")),
        }];
        let body = ParBuilderUtil::create_ast_nil(&parser);
        let p_new = ParBuilderUtil::create_ast_new_with_decls(decls, body, &parser);

        let (inputs, env) = proc_visit_inputs_and_env();
        let result = normalize_ann_proc(&p_new, inputs, &env, &parser).expect("normalize");

        // Outermost: a single New that binds just the synthetic tmpRet.
        assert_eq!(result.par.news.len(), 1);
        let outer = &result.par.news[0];
        assert_eq!(outer.bind_count, 1, "one tmpRet binder, no user binders");
        assert!(outer.uri.is_empty(), "no static URI binders");

        // Outer body: parallel Send to registry_lookup + Receive on tmpRet.
        let outer_body = outer.p.as_ref().expect("outer new has a body");
        assert_eq!(outer_body.sends.len(), 1, "one lookup send");
        assert_eq!(outer_body.receives.len(), 1, "one for-receive");

        // The send targets the registry_lookup fixed channel (via a
        // write-only bundle).
        let send = &outer_body.sends[0];
        let send_chan = send.chan.as_ref().expect("send has a chan");
        assert_eq!(send_chan.bundles.len(), 1);
        assert_eq!(
            send_chan.bundles[0].body.as_ref().expect("bundle body"),
            &FixedChannels::registry_lookup()
        );
        assert!(send_chan.bundles[0].write_flag);
        assert!(!send_chan.bundles[0].read_flag);

        // Send data: [URN as GString, BoundVar(0) referring to tmpRet].
        assert_eq!(send.data.len(), 2);
        assert_eq!(send.data[0].exprs.len(), 1);
        match send.data[0].exprs[0].expr_instance.as_ref() {
            Some(ExprInstance::GString(s)) => {
                assert_eq!(s, "rho:lib:*:abc:proj:*", "URN string literal matches")
            }
            other => panic!("expected GString URN, got {other:?}"),
        }

        // The receive binds `foo` from tmpRet (BoundVar(0) in the outer-new
        // scope since there's only one wildcard binder).
        let receive = &outer_body.receives[0];
        assert_eq!(receive.bind_count, 1);
        assert_eq!(receive.binds.len(), 1);
        let bind = &receive.binds[0];
        assert_eq!(bind.free_count, 1);
        let source = bind.source.as_ref().expect("receive source");
        // tmpRet at depth 0 is encoded as BoundVar(0).
        assert_eq!(source.exprs.len(), 1);

        // Receive body must contain a Match with two cases (Nil-abort + wildcard-body).
        let receive_body = receive.body.as_ref().expect("receive body");
        assert_eq!(receive_body.matches.len(), 1, "halt-on-miss match wrapper");
        let m = &receive_body.matches[0];
        assert_eq!(m.cases.len(), 2);

        // First case: Nil pattern → abort send.
        let nil_case = &m.cases[0];
        assert_eq!(
            nil_case.pattern.as_ref().expect("nil case pattern"),
            &Par::default(),
            "Nil case pattern is empty Par"
        );
        let abort_body = nil_case.source.as_ref().expect("nil case body");
        assert_eq!(abort_body.sends.len(), 1, "Nil case aborts");
        let abort_send = &abort_body.sends[0];
        let abort_chan = abort_send.chan.as_ref().expect("abort chan");
        assert_eq!(
            abort_chan.bundles[0]
                .body
                .as_ref()
                .expect("abort bundle body"),
            &FixedChannels::abort()
        );

        // Second case: wildcard pattern → user body (Nil here).
        let default_case = &m.cases[1];
        let pattern = default_case.pattern.as_ref().expect("default case pattern");
        // Wildcard pattern par has a single EVar/Wildcard expr.
        assert_eq!(pattern.exprs.len(), 1);
    }

    /// Mixed static + wildcard binders in one `new`. The static binder
    /// must stay in the outer New's `uri` list; the wildcard must get a
    /// synthetic tmpRet slot in `bind_count`.
    #[test]
    fn p_new_mixed_static_and_wildcard() {
        use rholang_parser::ast::{Id, NameDecl, Uri};
        use rholang_parser::SourcePos;

        let parser = rholang_parser::RholangParser::new();

        let decls = vec![
            NameDecl {
                id: Id {
                    name: "stdout",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: Some(Uri::from("rho:io:stdout")),
            },
            NameDecl {
                id: Id {
                    name: "lib",
                    pos: SourcePos { line: 0, col: 0 },
                },
                uri: Some(Uri::from("rho:lib:*:abc:proj:*")),
            },
        ];
        let body = ParBuilderUtil::create_ast_nil(&parser);
        let p_new = ParBuilderUtil::create_ast_new_with_decls(decls, body, &parser);

        let (inputs, env) = proc_visit_inputs_and_env();
        let result = normalize_ann_proc(&p_new, inputs, &env, &parser).expect("normalize");

        assert_eq!(result.par.news.len(), 1);
        let outer = &result.par.news[0];
        // One static URI + one tmpRet.
        assert_eq!(outer.bind_count, 2);
        assert_eq!(outer.uri, vec!["rho:io:stdout".to_string()]);

        let outer_body = outer.p.as_ref().expect("outer new has a body");
        assert_eq!(outer_body.sends.len(), 1, "one lookup send for `lib`");
        assert_eq!(outer_body.receives.len(), 1, "one for-receive for `lib`");

        // Send references the tmpRet slot at depth 0 (static URI at depth 1).
        let send = &outer_body.sends[0];
        match send.data[0].exprs[0].expr_instance.as_ref() {
            Some(ExprInstance::GString(s)) => {
                assert_eq!(s, "rho:lib:*:abc:proj:*")
            }
            other => panic!("expected GString URN, got {other:?}"),
        }
    }

    /// Static-only (exact URN) must not touch the dynamic rewrite path.
    /// The compiled bytecode shape should be identical to the pre-change
    /// behavior: a single `New` with `uri = ["rho:io:stdout"]`, no sends,
    /// no receives in the outer body.
    #[test]
    fn p_new_exact_versioned_urn_stays_static() {
        use rholang_parser::ast::{Id, NameDecl, Uri};
        use rholang_parser::SourcePos;

        let parser = rholang_parser::RholangParser::new();

        let decls = vec![NameDecl {
            id: Id {
                name: "lib",
                pos: SourcePos { line: 0, col: 0 },
            },
            uri: Some(Uri::from("rho:lib:1.0.0:abc:proj:2.6.3")),
        }];
        let body = ParBuilderUtil::create_ast_nil(&parser);
        let p_new = ParBuilderUtil::create_ast_new_with_decls(decls, body, &parser);

        let (inputs, env) = proc_visit_inputs_and_env();
        let result = normalize_ann_proc(&p_new, inputs, &env, &parser).expect("normalize");

        assert_eq!(result.par.news.len(), 1);
        let outer = &result.par.news[0];
        assert_eq!(outer.bind_count, 1);
        assert_eq!(outer.uri, vec!["rho:lib:1.0.0:abc:proj:2.6.3".to_string()]);
        let outer_body = outer.p.as_ref().expect("outer new has a body");
        assert!(outer_body.sends.is_empty(), "no dynamic send for exact URN");
        assert!(
            outer_body.receives.is_empty(),
            "no dynamic receive for exact URN"
        );
    }
}
