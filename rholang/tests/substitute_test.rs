// See rholang/src/test/scala/coop/rchain/rholang/interpreter/SubstituteTest.scala

use std::collections::BTreeMap;

use models::create_bit_vector;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EMatches, EMinusMinus, EPercentPercent, EPlusPlus, Expr,
    Match, MatchCase, New, Par, Send, Var, VarRef,
};
use models::rust::rholang::implicits::GPrivateBuilder;
use models::rust::utils::{new_boundvar_par, new_freevar_var, new_gstring_par};
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use rholang::rust::interpreter::util::prepend_connective;
use rspace_plus_plus::rspace::history::Either;

const DEPTH: i32 = 0;

fn env() -> Env<Par> { Env::new() }

fn substitute_instance() -> Substitute {
    let cost = Cost::create(0, "substitute_test".to_string());
    let budget = RuntimeBudget::new(cost);
    let substitute = Substitute {
        metering: MeteredMachine::new(budget),
    };
    substitute
}

fn generate_random_subsequence<T: Clone>(items: &[T]) -> Vec<T> {
    let mut rng = thread_rng();
    let subset: Vec<T> = items
        .choose_multiple(&mut rng.clone(), rng.gen_range(0..=items.len()))
        .cloned()
        .collect();
    // subset.sort();
    subset
}

#[test]
fn substitute_should_retain_all_non_empty_par_ed_connectives() {
    let sample_connectives: Vec<Connective> = vec![
        Connective {
            connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                index: 0,
                depth: 0,
            })),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![],
            })),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![],
            })),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnNotBody(Par::default())),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnBool(false)),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnInt(false)),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnString(true)),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnUri(true)),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnByteArray(true)),
        },
        Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![],
            })),
        },
    ];

    let doubled_connectives: Vec<Connective> = sample_connectives
        .clone()
        .into_iter()
        .chain(sample_connectives.into_iter())
        .collect();

    for _ in 0..10 {
        let random_connectives: Vec<Connective> = generate_random_subsequence(&doubled_connectives);

        let par = Par::default();
        for connective in &random_connectives {
            prepend_connective(par.clone(), connective.clone(), DEPTH);
        }

        let substitution = substitute_instance().substitute(par.clone(), DEPTH, &env());
        assert!(substitution.is_ok());
        assert_eq!(substitution.unwrap().connectives, par.connectives);
    }
}

#[test]
fn free_var_should_throw_an_error() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source);
    let substitution = substitute_instance().maybe_substitute_var(new_freevar_var(0), DEPTH, &env);
    assert!(substitution.is_err());
}

#[test]
fn bound_var_should_be_substituted_for_process() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());
    let substitution = substitute_instance().maybe_substitute_var(
        Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        },
        DEPTH,
        &env,
    );
    assert!(substitution.is_ok());
    assert_eq!(substitution.unwrap(), Either::Right(source))
}

#[test]
fn bound_var_should_be_substituted_with_expression() {
    let new_sends = vec![Send {
        chan: Some(new_boundvar_par(0, Vec::new(), false)),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    }];
    let source = Par::default().with_sends(new_sends);
    let mut env = Env::new();
    env = env.put(source.clone());
    let substitution = substitute_instance().maybe_substitute_var(
        Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        },
        DEPTH,
        &env,
    );
    assert!(substitution.is_ok());
    assert_eq!(substitution.unwrap(), Either::Right(source))
}

#[test]
fn bound_var_should_be_left_unchanged() {
    let mut env = Env::new();
    env = env.put(GPrivateBuilder::new_par());
    env = env.put(GPrivateBuilder::new_par());
    env = env.shift(1);
    let substitution = substitute_instance().maybe_substitute_var(
        Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        },
        DEPTH,
        &env,
    );
    assert!(substitution.is_ok());
    assert_eq!(
        substitution.unwrap(),
        Either::Left(Var {
            var_instance: Some(VarInstance::BoundVar(0)),
        })
    )
}

#[test]
fn send_should_leave_variables_not_in_the_environment_alone() {
    let mut env = Env::new();
    env = env.put(GPrivateBuilder::new_par());
    env = env.put(GPrivateBuilder::new_par());
    env = env.shift(1);

    let send = Send {
        chan: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    };

    let substitution = substitute_instance().substitute(send.clone(), DEPTH, &env);
    assert!(substitution.is_ok());
    assert_eq!(substitution.unwrap(), send)
}

#[test]
fn send_should_substitute_bound_vars_for_values() {
    let source0 = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source0.clone());
    env = env.put(GPrivateBuilder::new_par());

    let send = Send {
        chan: Some(Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![1]),
            connective_used: false,
        }])),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![1]),
        connective_used: false,
    };

    let substitution = substitute_instance().substitute(send, DEPTH, &env);
    assert!(substitution.is_ok());
    assert_eq!(substitution.unwrap(), Send {
        chan: Some(Par::default().with_sends(vec![Send {
            chan: Some(source0),
            data: vec![Par::default()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }])),
        data: vec![Par::default()],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    })
}

#[test]
fn send_should_substitute_all_bound_vars_for_values() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Send {
        chan: Some(new_boundvar_par(0, Vec::new(), false)),
        data: vec![Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(0, Vec::new(), false)),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
        }])],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    };

    let expected_result = Send {
        chan: Some(source.clone()),
        data: vec![Par::default().with_sends(vec![Send {
            chan: Some(source),
            data: vec![Par::default()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }])],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());
    assert_eq!(substitution.unwrap(), expected_result)
}

#[test]
fn send_should_substitute_all_bound_vars_for_values_in_environment() {
    let chan0 = new_boundvar_par(0, Vec::new(), false);
    let source = Par::default().with_news(vec![New {
        bind_count: 1,
        p: Some(Par::default().with_sends(vec![Send {
            chan: Some(chan0.clone()),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
        }])),
        uri: vec![],
        injections: BTreeMap::new(),
        locally_free: vec![],
    }]);
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Send {
        chan: Some(chan0.clone()),
        data: vec![Par::default().with_sends(vec![Send {
            chan: Some(chan0.clone()),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
        }])],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    let p = Some(Par::default().with_sends(vec![Send {
        chan: Some(chan0.clone()),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    }]));
    assert_eq!(substitution.unwrap(), Send {
        chan: Some(Par::default().with_news(vec![New {
            bind_count: 1,
            p: p.clone(),
            uri: vec![],
            injections: BTreeMap::new(),
            locally_free: vec![]
        }])),
        data: vec![Par::default().with_sends(vec![Send {
            chan: Some(Par::default().with_news(vec![New {
                bind_count: 1,
                p,
                uri: vec![],
                injections: BTreeMap::new(),
                locally_free: vec![]
            }])),
            data: vec![Par::default()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }])],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    })
}

#[test]
fn new_should_only_substitute_body_of_expression() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = New {
        bind_count: 1,
        p: Some(Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![1]),
            connective_used: false,
        }])),
        uri: vec![],
        injections: BTreeMap::new(),
        locally_free: create_bit_vector(&vec![0]),
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), New {
        bind_count: 1,
        p: Some(Par::default().with_sends(vec![Send {
            chan: Some(source),
            data: vec![Par::default()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false
        }])),
        uri: vec![],
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    })
}

#[test]
fn new_should_only_substitute_all_variables_in_body_of_expression() {
    let source0 = GPrivateBuilder::new_par();
    let source1 = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source0.clone());
    env = env.put(source1.clone());

    let target = New {
        bind_count: 2,
        p: Some(Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(3, Vec::new(), false)),
            data: vec![Par::default().with_sends(vec![Send {
                chan: Some(new_boundvar_par(2, Vec::new(), false)),
                data: vec![Par::default()],
                persistent: false,
                locally_free: create_bit_vector(&vec![2]),
                connective_used: false,
            }])],
            persistent: false,
            locally_free: create_bit_vector(&vec![2, 3]),
            connective_used: false,
        }])),
        uri: vec![],
        injections: BTreeMap::new(),
        locally_free: create_bit_vector(&vec![0, 1]),
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), New {
        bind_count: 2,
        p: Some(Par::default().with_sends(vec![Send {
            chan: Some(source0),
            data: vec![Par::default().with_sends(vec![Send {
                chan: Some(source1),
                data: vec![Par::default()],
                persistent: false,
                locally_free: vec![],
                connective_used: false
            }])],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false
        }])),
        uri: vec![],
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    })
}

#[test]
fn eval_should_remove_eval_and_quote_pairs() {
    let mut env = Env::new();
    env = env.put(GPrivateBuilder::new_par_from_string("one".to_string()));
    env = env.put(GPrivateBuilder::new_par_from_string("zero".to_string()));

    let target = new_boundvar_par(1, Vec::new(), false);

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(
        substitution.unwrap(),
        GPrivateBuilder::new_par_from_string("one".to_string())
    )
}

#[test]
fn bundle_should_substitute_within_the_body_of_the_bundle() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Bundle {
        body: Some(Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(0, Vec::new(), false)),
            data: vec![Par::default()],
            persistent: false,
            locally_free: create_bit_vector(&vec![0]),
            connective_used: false,
        }])),
        write_flag: false,
        read_flag: false,
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), Bundle {
        body: Some(Par::default().with_sends(vec![Send {
            chan: Some(source),
            data: vec![Par::default()],
            persistent: false,
            locally_free: vec![],
            connective_used: false
        }])),
        write_flag: false,
        read_flag: false
    })
}

#[test]
fn bundle_should_only_substitute_all_vars_inside_body() {
    let source0 = GPrivateBuilder::new_par();
    let source1 = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source0.clone());
    env = env.put(source1.clone());

    let target = Bundle {
        body: Some(Par::default().with_sends(vec![Send {
            chan: Some(new_boundvar_par(1, Vec::new(), false)),
            data: vec![Par::default().with_sends(vec![Send {
                chan: Some(new_boundvar_par(0, Vec::new(), false)),
                data: vec![Par::default()],
                persistent: false,
                locally_free: create_bit_vector(&vec![0]),
                connective_used: false,
            }])],
            persistent: false,
            locally_free: create_bit_vector(&vec![0, 1]),
            connective_used: false,
        }])),
        write_flag: false,
        read_flag: false,
    };

    let substitution = substitute_instance().substitute(target, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), Bundle {
        body: Some(Par::default().with_sends(vec![Send {
            chan: Some(source0),
            data: vec![Par::default().with_sends(vec![Send {
                chan: Some(source1),
                data: vec![Par::default()],
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
            }])],
            persistent: false,
            locally_free: vec![],
            connective_used: false
        }])),
        write_flag: false,
        read_flag: false
    })
}

#[test]
fn bundle_should_preserve_bundles_polarities_during_substitution() {
    let r = Par::default().with_bundles(vec![Bundle {
        body: Some(new_gstring_par("stdout".to_string(), Vec::new(), false)),
        write_flag: true,
        read_flag: false,
    }]);
    let mut env = Env::new();
    env = env.put(r);

    let bundle = Bundle {
        body: Some(new_boundvar_par(0, Vec::new(), false)),
        write_flag: false,
        read_flag: true,
    };

    let substitution = substitute_instance().substitute(bundle, DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), Bundle {
        body: Some(new_gstring_par("stdout".to_string(), Vec::new(), false)),
        write_flag: false,
        read_flag: false
    })
}

#[test]
fn var_ref_should_be_replaced_at_correct_depth() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Par::default().with_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
            index: 0,
            depth: 1,
        })),
    }]);

    let substitution = substitute_instance().substitute(target, 1, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), source)
}

#[test]
fn var_ref_should_not_be_replaced_at_different_depth() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Par::default().with_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
            index: 0,
            depth: 2,
        })),
    }]);

    let substitution = substitute_instance().substitute(target.clone(), 1, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), target)
}

#[test]
fn var_ref_should_be_replaced_at_a_higher_depth_inside_a_pattern() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());
    env = env.shift(1);

    let target = Par::default().with_matches(vec![Match {
        target: Some(new_boundvar_par(0, Vec::new(), false)),
        cases: vec![MatchCase {
            pattern: Some(Par::default().with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                    index: 1,
                    depth: 2,
                })),
            }])),
            source: Some(Par::default()),
            free_count: 0,
            guard: None,
        }],
        locally_free: Vec::new(),
        connective_used: false,
    }]);

    let substitution = substitute_instance().substitute(target.clone(), 1, &env);
    assert!(substitution.is_ok());

    assert_eq!(
        substitution.unwrap(),
        Par::default().with_matches(vec![Match {
            target: Some(new_boundvar_par(0, Vec::new(), false)),
            cases: vec![MatchCase {
                pattern: Some(source),
                source: Some(Par::default()),
                free_count: 0,
                guard: None,
            }],
            locally_free: Vec::new(),
            connective_used: false,
        }])
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// `EMatches`'s `pattern` is a nested pattern at `depth + 1`
//
// `p_matches_normalizer::combine_p_matches` normalizes the right-hand side of
// `P matches Q` under `input.bound_map_chain.push()` — one binding level
// deeper — in a FRESH `FreeMap`, and keeps the TARGET's free map. Two
// consequences, and they pull in opposite directions:
//
//   * a plain `x` inside the pattern resolves through `BoundMapChain::get`,
//     which reads the CURRENT scope only, so it is a fresh binding occurrence
//     and never a `BoundVar` pointing outside. That is why `has_locally_free`
//     computes this node's `locally_free`/`connective_used` from the TARGET
//     alone, and why doing so is correct rather than a shortcut.
//   * `=x` resolves through `BoundMapChain::find`, which walks the whole
//     chain, and is emitted as `VarRef { depth }` with the chain distance. So
//     the ONE construct that can name an outer binder from inside a `matches`
//     pattern is exactly the one `maybe_substitute_var_ref` gates on depth.
//
// Substitution must therefore descend into `pattern` at `depth + 1`, as it
// already does for `MatchCase::pattern` (the test above) and for
// `ReceiveBind::patterns`.
// ─────────────────────────────────────────────────────────────────────────────

/// The `(target, pattern)` slots of the single `EMatches` a substituted `Par`
/// carries. Reading the slots rather than the whole `Par` keeps the assertions
/// pinned to the values under test instead of to the sorter's cached-field
/// bookkeeping.
fn ematches_slots(par: &Par) -> (Par, Par) {
    let expr = par
        .exprs
        .first()
        .expect("the substituted Par must carry exactly one Expr");
    match expr
        .expr_instance
        .as_ref()
        .expect("Expr.expr_instance must be populated")
    {
        ExprInstance::EMatchesBody(EMatches { target, pattern }) => (
            target.clone().expect("EMatches.target"),
            pattern.clone().expect("EMatches.pattern"),
        ),
        other => panic!("expected an EMatchesBody, got {other:?}"),
    }
}

/// `target matches pattern`, with `target` a bound var that must survive
/// untouched (substitution at depth != 0 leaves `BoundVar`s alone).
fn matches_par(pattern: Par) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
            target: Some(new_boundvar_par(0, Vec::new(), false)),
            pattern: Some(pattern),
        })),
    }])
}

fn var_ref_par(index: i32, depth: i32) -> Par {
    Par::default().with_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef { index, depth })),
    }])
}

/// RED: the pattern carries a `VarRef` at the depth the normalizer emits for
/// it (one deeper than the enclosing term). It must be substituted.
#[test]
fn var_ref_should_be_replaced_at_a_higher_depth_inside_a_matches_pattern() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source.clone());
    env = env.shift(1);

    let substitution = substitute_instance()
        .substitute(matches_par(var_ref_par(1, 2)), 1, &env)
        .expect("substituting an EMatches must succeed");

    let (target, pattern) = ematches_slots(&substitution);
    assert_eq!(
        pattern, source,
        "the `matches` pattern lives at depth + 1, so a VarRef of depth 2 \
         inside a term substituted at depth 1 must be replaced by the env's \
         binding — exactly as MatchCase::pattern is"
    );
    assert_eq!(
        target,
        new_boundvar_par(0, Vec::new(), false),
        "the target slot is at the enclosing depth and must be untouched"
    );
}

/// CONTROL — must NOT discriminate. A ground pattern names nothing, so the
/// depth it is visited at cannot change the result.
#[test]
fn a_ground_matches_pattern_is_unchanged_by_substitution() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source);
    env = env.shift(1);

    let ground = new_gstring_par("ground".to_string(), Vec::new(), false);
    let substitution = substitute_instance()
        .substitute(matches_par(ground.clone()), 1, &env)
        .expect("substituting an EMatches must succeed");

    let (target, pattern) = ematches_slots(&substitution);
    assert_eq!(pattern, ground, "a ground pattern is a fixed point");
    assert_eq!(target, new_boundvar_par(0, Vec::new(), false));
}

/// CONTROL — pins the bump to EXACTLY one level. A `VarRef` whose depth does
/// not match the pattern's depth must still be left alone, so the fix cannot
/// degenerate into "substitute every VarRef in a pattern".
#[test]
fn a_matches_pattern_var_ref_at_a_non_matching_depth_is_left_alone() {
    let source = GPrivateBuilder::new_par();
    let mut env = Env::new();
    env = env.put(source);
    env = env.shift(1);

    let stranger = var_ref_par(1, 3);
    let substitution = substitute_instance()
        .substitute(matches_par(stranger.clone()), 1, &env)
        .expect("substituting an EMatches must succeed");

    let (_, pattern) = ematches_slots(&substitution);
    assert_eq!(
        pattern, stranger,
        "depth 3 is neither the enclosing depth (1) nor the pattern's (2)"
    );
}

#[test]
fn e_plus_plus_should_be_substituted_correctly() {
    let mut source = Par::default().with_sends(vec![Send {
        chan: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    }]);
    source.locally_free = create_bit_vector(&vec![0]);
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPlusPlusBody(EPlusPlus {
            p1: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
            p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
        })),
    }]);

    let substitution = substitute_instance().substitute(target.clone(), DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), {
        let mut p = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusPlusBody(EPlusPlus {
                p1: Some(source),
                p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
            })),
        }]);
        p.locally_free = create_bit_vector(&vec![0, 1]);
        p
    })
}

#[test]
fn e_percent_percent_should_be_substituted_correctly() {
    let mut source = Par::default().with_sends(vec![Send {
        chan: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    }]);
    source.locally_free = create_bit_vector(&vec![0]);
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPercentPercentBody(EPercentPercent {
            p1: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
            p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
        })),
    }]);

    let substitution = substitute_instance().substitute(target.clone(), DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), {
        let mut p = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: Some(source),
                p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
            })),
        }]);
        p.locally_free = create_bit_vector(&vec![0, 1]);
        p
    })
}

#[test]
fn e_minus_minus_should_be_substituted_correctly() {
    let mut source = Par::default().with_sends(vec![Send {
        chan: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
        data: vec![Par::default()],
        persistent: false,
        locally_free: create_bit_vector(&vec![0]),
        connective_used: false,
    }]);
    source.locally_free = create_bit_vector(&vec![0]);
    let mut env = Env::new();
    env = env.put(source.clone());

    let target = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMinusMinusBody(EMinusMinus {
            p1: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
            p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
        })),
    }]);

    let substitution = substitute_instance().substitute(target.clone(), DEPTH, &env);
    assert!(substitution.is_ok());

    assert_eq!(substitution.unwrap(), {
        let mut p = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMinusMinusBody(EMinusMinus {
                p1: Some(source),
                p2: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
            })),
        }]);
        p.locally_free = create_bit_vector(&vec![0, 1]);
        p
    })
}
