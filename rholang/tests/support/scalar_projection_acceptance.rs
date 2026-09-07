#[derive(Clone, Copy, Debug)]
enum ScalarProjectionRequest {
    Single,
    Integer,
    Boolean,
}

impl ScalarProjectionRequest {
    fn literal(self) -> Expr {
        Expr {
            expr_instance: Some(match self {
                Self::Single | Self::Integer => ExprInstance::GInt(7),
                Self::Boolean => ExprInstance::GBool(true),
            }),
        }
    }

    fn arity_error(self) -> &'static str {
        match self {
            Self::Integer => "Error: Integer expected, or unimplemented expression.",
            Self::Single | Self::Boolean => "Error: Multiple expressions given.",
        }
    }

    fn evaluate(
        self,
        reducer: &DebruijnInterpreter,
        par: &Par,
        env: &Env<Par>,
        recursive: bool,
    ) -> Result<Vec<u8>, String> {
        let expression = match (self, recursive) {
            (Self::Single, false) => reducer.eval_single_expr(par, env),
            (Self::Single, true) => reducer.eval_single_expr_recursive(par, env),
            (Self::Integer, false) => reducer.eval_to_i64(par, env).map(|value| Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }),
            (Self::Integer, true) => reducer.eval_to_i64_recursive(par, env).map(|value| Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }),
            (Self::Boolean, false) => reducer.eval_to_bool(par, env).map(|value| Expr {
                expr_instance: Some(ExprInstance::GBool(value)),
            }),
            (Self::Boolean, true) => reducer.eval_to_bool_recursive(par, env).map(|value| Expr {
                expr_instance: Some(ExprInstance::GBool(value)),
            }),
        };
        expression
            .map(|value| value.encode_to_vec())
            .map_err(|error| format!("{error:?}"))
    }
}

fn scalar_projection_foreign_families() -> [Par; 8] {
    [
        Par::default().with_sends(vec![Send::default()]),
        Par::default().with_receives(vec![Receive::default()]),
        Par::default().with_news(vec![New::default()]),
        Par::default().with_matches(vec![Match::default()]),
        Par::default().with_unforgeables(vec![GUnforgeable::default()]),
        Par::default().with_bundles(vec![Bundle::default()]),
        Par::default().with_connectives(vec![Connective::default()]),
        Par::default().with_conditionals(vec![If::default()]),
    ]
}

fn scalar_projection_carrier(expression: &Expr, count: u64, mask: u8) -> Par {
    use rspace_plus_plus::rspace::bag::Multiplicity;

    let mut par = Par::default();
    if count != 0 {
        par.insert_component_many(
            expression,
            Multiplicity::new(count).expect("nonzero count"),
            Vec::new(),
            false,
        )
        .expect("one compressed expression endpoint");
    }
    for (index, foreign) in scalar_projection_foreign_families().into_iter().enumerate() {
        if mask & (1 << index) != 0 {
            par = par.append(foreign);
        }
    }
    assert_eq!(par.family_cardinality::<Expr>(), count);
    assert_eq!(par.all_components_are::<Expr>(), mask == 0);
    assert_eq!(
        par.distinct_len(),
        usize::from(count != 0) + mask.count_ones() as usize
    );
    par
}

async fn assert_scalar_projection_observation(count: u64, mask: u8, bound: bool) {
    let variable = new_boundvar_par(0, Vec::new(), false)
        .first::<Expr>()
        .expect("canonical variable")
        .expect("one variable")
        .0;
    let par = scalar_projection_carrier(&variable, count, mask);
    let snapshot = par.clone();
    for request in [
        ScalarProjectionRequest::Single,
        ScalarProjectionRequest::Integer,
        ScalarProjectionRequest::Boolean,
    ] {
        let env = if bound {
            Env::new().put(Par::default().with_exprs(vec![request.literal()]))
        } else {
            Env::new()
        };
        let recursive = build().await;
        let driven = build().await;
        let before_recursive = charge_trace(&recursive);
        let before_driven = charge_trace(&driven);
        assert_eq!(before_recursive, before_driven);
        let expected = request.evaluate(&recursive, &par, &env, true);
        let actual = request.evaluate(&driven, &par, &env, false);
        assert_eq!(
            actual, expected,
            "{request:?}: count={count}, mask={mask}, bound={bound}"
        );
        let after_recursive = charge_trace(&recursive);
        let after_driven = charge_trace(&driven);
        assert_eq!(after_driven, after_recursive);

        if mask != 0 || count != 1 {
            let message = if mask != 0 {
                "Error: parallel or non expression found where expression expected."
            } else {
                request.arity_error()
            };
            assert_eq!(
                actual,
                Err(format!(
                    "{:?}",
                    InterpreterError::ReduceError(message.into())
                ))
            );
            assert_eq!(
                after_recursive, before_recursive,
                "refusal evaluated the oracle child"
            );
            assert_eq!(
                after_driven, before_driven,
                "refusal evaluated the PDA child"
            );
        } else {
            assert_ne!(
                after_driven.0, before_driven.0,
                "the admitted child must emit its diagnostic"
            );
            if bound {
                assert_eq!(actual, Ok(request.literal().encode_to_vec()));
            } else {
                assert_eq!(
                    actual,
                    Err(format!(
                        "{:?}",
                        InterpreterError::ReduceError("Unbound variable: 0 in {}".into())
                    ))
                );
            }
        }
        assert!(par.components().shares_storage_with(snapshot.components()));
        assert_eq!(par.locally_free(), snapshot.locally_free());
        assert_eq!(par.connective_used(), snapshot.connective_used());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn scalar_projection_acceptance_matrix_preserves_refusal_trace() {
    for mask in std::iter::once(0).chain((0..8).map(|index| 1 << index)) {
        for count in [0, 1, 2, u64::MAX] {
            assert_scalar_projection_observation(count, mask, true).await;
            assert_scalar_projection_observation(count, mask, false).await;
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 128, ..ProptestConfig::default() })]
    #[test]
    fn scalar_projection_acceptance_properties(
        count in prop_oneof![Just(0u64), Just(1u64), Just(2u64), Just(u64::MAX), any::<u64>()],
        mask in any::<u8>(),
        bound in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        runtime.block_on(assert_scalar_projection_observation(count, mask, bound));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn scalar_projection_recognizers_decline_without_effects() {
    let map = Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::default())),
    };
    let method = models::rhoapi::EMethod {
        method_name: "pathExists".into(),
        target: Some(Par::default().with_exprs(vec![map.clone()])),
        ..Default::default()
    };
    let expression = Expr {
        expr_instance: Some(ExprInstance::EMethodBody(method.clone())),
    };
    let reentry = method_reentry_chain(1);
    let reentry_expression = reentry.first::<Expr>().unwrap().unwrap().0;
    let env = Env::new();
    let control = build().await;
    assert!(control
        .begin_encoded_method_reentry(&reentry)
        .unwrap()
        .is_some());
    let control = build().await;
    assert!(control
        .begin_fused_method_chain(&method, &env)
        .unwrap()
        .is_some());
    let control = build().await;
    assert!(control
        .begin_fused_method_chain_par(&Par::default().with_exprs(vec![expression.clone()]), &env)
        .unwrap()
        .is_some());

    for mask in std::iter::once(0).chain((0..8).map(|index| 1 << index)) {
        for count in [0, 1, 2, u64::MAX] {
            if mask == 0 && count == 1 {
                continue;
            }
            let carrier = scalar_projection_carrier(&expression, count, mask);
            let reentry_carrier = scalar_projection_carrier(&reentry_expression, count, mask);
            let base = scalar_projection_carrier(&map, count, mask);
            let resident = models::rhoapi::EMethod {
                target: Some(base.clone()),
                ..method.clone()
            };
            let reducer = build().await;
            let before = charge_trace(&reducer);
            assert!(reducer
                .begin_fused_method_chain_par(&carrier, &env)
                .unwrap()
                .is_none());
            assert_eq!(charge_trace(&reducer), before);
            assert!(reducer
                .begin_encoded_method_reentry(&reentry_carrier)
                .unwrap()
                .is_none());
            assert_eq!(charge_trace(&reducer), before);
            assert!(reducer
                .begin_fused_method_chain(&resident, &env)
                .unwrap()
                .is_none());
            assert_eq!(charge_trace(&reducer), before);
            assert!(resident
                .target
                .as_ref()
                .unwrap()
                .components()
                .shares_storage_with(base.components()));
        }
    }
}
