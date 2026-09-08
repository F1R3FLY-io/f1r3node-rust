use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, Expr, GUnforgeable, If, Match, New, Par, Receive, Send,
    VarRef,
};
use models::rust::par_bag::Multiplicity;
use models::rust::utils::{new_freevar_expr, new_wildcard_expr};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use rholang::rust::interpreter::matcher::par_count::{CountBound, ParCount};

type ReferenceCounts = [Option<u128>; 8];
type ReferenceInterval = (ReferenceCounts, ReferenceCounts);

#[derive(Clone, Debug, Default)]
struct ParSpec {
    concrete: [u8; 8],
    free: u8,
    wildcard: u8,
    connectives: Vec<(ConnectiveSpec, u8)>,
}

#[derive(Clone, Debug)]
enum ConnectiveSpec {
    And(Vec<ParSpec>),
    Or(Vec<ParSpec>),
    Not(Box<ParSpec>),
    Scalar(u8, bool),
    VarRef,
    Missing,
}

fn reference_min(left: Option<u128>, right: Option<u128>) -> Option<u128> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left < right { left } else { right }),
        (None, other) | (other, None) => other,
    }
}

fn reference_max(left: Option<u128>, right: Option<u128>) -> Option<u128> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left > right { left } else { right }),
        _ => None,
    }
}

fn reference_sum(left: Option<u128>, right: Option<u128>) -> Option<u128> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        _ => None,
    }
}

fn reference_scale(value: Option<u128>, multiplicity: u8) -> Option<u128> {
    if multiplicity == 0 {
        Some(0)
    } else {
        value.map(|value| value * u128::from(multiplicity))
    }
}

fn reference_par(spec: &ParSpec) -> ReferenceInterval {
    let concrete = spec.concrete.map(|count| Some(u128::from(count)));
    let mut result = (
        concrete,
        if spec.free != 0 || spec.wildcard != 0 {
            [None; 8]
        } else {
            concrete
        },
    );
    for (connective, multiplicity) in &spec.connectives {
        let child = reference_connective(connective);
        for index in 0..8 {
            result.0[index] = reference_sum(
                result.0[index],
                reference_scale(child.0[index], *multiplicity),
            );
            result.1[index] = reference_sum(
                result.1[index],
                reference_scale(child.1[index], *multiplicity),
            );
        }
    }
    result
}

fn reference_connective(spec: &ConnectiveSpec) -> ReferenceInterval {
    match spec {
        ConnectiveSpec::And(children) => {
            let mut result = ([Some(0); 8], [None; 8]);
            for child in children {
                let child = reference_par(child);
                for index in 0..8 {
                    result.0[index] = reference_max(result.0[index], child.0[index]);
                    result.1[index] = reference_min(result.1[index], child.1[index]);
                }
            }
            result
        }
        ConnectiveSpec::Or(children) => {
            let mut result = ([None; 8], [Some(0); 8]);
            for child in children {
                let child = reference_par(child);
                for index in 0..8 {
                    result.0[index] = reference_min(result.0[index], child.0[index]);
                    result.1[index] = reference_max(result.1[index], child.1[index]);
                }
            }
            result
        }
        ConnectiveSpec::Not(_) => ([Some(0); 8], [None; 8]),
        ConnectiveSpec::Scalar(_, _) => {
            let mut counts = [Some(0); 8];
            counts[3] = Some(1);
            (counts, counts)
        }
        ConnectiveSpec::VarRef | ConnectiveSpec::Missing => ([Some(0); 8], [Some(0); 8]),
    }
}

fn build_par(spec: &ParSpec) -> Par {
    let mut par = Par::default();
    macro_rules! insert {
        ($component:expr, $count:expr, $active:expr) => {
            if $count != 0 {
                let multiplicity = Multiplicity::new(u64::from($count))
                    .expect("positive small fixture multiplicity");
                par.insert_component_many(&$component, multiplicity, Vec::new(), $active)
                    .expect("small typed fixture component");
            }
        };
    }
    insert!(Send::default(), spec.concrete[0], false);
    insert!(Receive::default(), spec.concrete[1], false);
    insert!(New::default(), spec.concrete[2], false);
    insert!(
        Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        },
        spec.concrete[3],
        false
    );
    insert!(Match::default(), spec.concrete[4], false);
    insert!(GUnforgeable::default(), spec.concrete[5], false);
    insert!(Bundle::default(), spec.concrete[6], false);
    insert!(If::default(), spec.concrete[7], false);
    insert!(new_freevar_expr(0), spec.free, true);
    insert!(new_wildcard_expr(), spec.wildcard, true);
    for (connective, multiplicity) in &spec.connectives {
        insert!(build_connective(connective), *multiplicity, true);
    }
    par
}

fn build_connective(spec: &ConnectiveSpec) -> Connective {
    let connective_instance = match spec {
        ConnectiveSpec::And(children) => Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: children.iter().map(build_par).collect(),
        })),
        ConnectiveSpec::Or(children) => Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
            ps: children.iter().map(build_par).collect(),
        })),
        ConnectiveSpec::Not(child) => Some(ConnectiveInstance::ConnNotBody(build_par(child))),
        ConnectiveSpec::Scalar(kind, value) => Some(match kind {
            0 => ConnectiveInstance::ConnBool(*value),
            1 => ConnectiveInstance::ConnInt(*value),
            2 => ConnectiveInstance::ConnString(*value),
            3 => ConnectiveInstance::ConnUri(*value),
            4 => ConnectiveInstance::ConnByteArray(*value),
            _ => unreachable!("the fixture has exactly five scalar connective families"),
        }),
        ConnectiveSpec::VarRef => Some(ConnectiveInstance::VarRefBody(VarRef {
            index: 2,
            depth: 1,
        })),
        ConnectiveSpec::Missing => None,
    };
    Connective {
        connective_instance,
    }
}

fn observe_counts(counts: ParCount) -> ReferenceCounts {
    [
        counts.sends,
        counts.receives,
        counts.news,
        counts.exprs,
        counts.matches,
        counts.unforgeables,
        counts.bundles,
        counts.conditionals,
    ]
    .map(|bound| match bound {
        CountBound::Finite(value) => Some(value),
        CountBound::Unbounded => None,
    })
}

fn observe_interval(interval: (ParCount, ParCount)) -> ReferenceInterval {
    (observe_counts(interval.0), observe_counts(interval.1))
}

fn assert_connective(spec: &ConnectiveSpec) {
    let expected = reference_connective(spec);
    let connective = build_connective(spec);
    let counter = ParCount::zero();
    assert_eq!(
        observe_interval(counter.min_max_con_ref(&connective)),
        expected,
        "borrowed connective {spec:?}"
    );
    assert_eq!(
        observe_interval(counter.min_max_con(connective)),
        expected,
        "owned connective {spec:?}"
    );
}

fn assert_par(spec: &ParSpec) {
    let expected = reference_par(spec);
    let par = build_par(spec);
    let retained = par.clone();
    let counter = ParCount::zero();
    assert_eq!(
        observe_interval(counter.min_max_par_ref(&par)),
        expected,
        "borrowed Par {spec:?}"
    );
    assert_eq!(
        observe_interval(counter.min_max_par(par.clone())),
        expected,
        "owned Par {spec:?}"
    );
    assert!(par.components().shares_storage_with(retained.components()));
    for (connective, _) in &spec.connectives {
        assert_connective(connective);
    }
}

#[test]
fn par_count_pda_leaf_and_empty_connective_equations_are_exact() {
    let empty_and = ConnectiveSpec::And(Vec::new());
    let empty_or = ConnectiveSpec::Or(Vec::new());
    assert_eq!(reference_connective(&empty_and), ([Some(0); 8], [None; 8]));
    assert_eq!(reference_connective(&empty_or), ([None; 8], [Some(0); 8]));
    for spec in [
        empty_and,
        empty_or,
        ConnectiveSpec::VarRef,
        ConnectiveSpec::Missing,
        ConnectiveSpec::Not(Box::new(ParSpec {
            concrete: [1, 2, 3, 4, 5, 6, 7, 8],
            free: 2,
            ..ParSpec::default()
        })),
    ] {
        assert_connective(&spec);
    }
    for kind in 0..5 {
        for value in [false, true] {
            assert_connective(&ConnectiveSpec::Scalar(kind, value));
        }
    }
}

#[test]
fn par_count_pda_parallel_nested_and_scaled_corpus_matches_reference() {
    let concrete = ParSpec {
        concrete: [1, 2, 3, 4, 5, 6, 7, 8],
        ..ParSpec::default()
    };
    let free = ParSpec {
        free: 2,
        ..concrete.clone()
    };
    let wildcard = ParSpec {
        wildcard: 3,
        ..ParSpec::default()
    };
    let scaled = ParSpec {
        connectives: vec![
            (ConnectiveSpec::Scalar(1, true), 2),
            (ConnectiveSpec::Scalar(1, true), 3),
            (ConnectiveSpec::Scalar(0, false), 2),
            (ConnectiveSpec::Not(Box::new(concrete.clone())), 0),
        ],
        ..ParSpec::default()
    };
    let (minimum, maximum) = reference_par(&scaled);
    assert_eq!(minimum[3], Some(7));
    assert_eq!(minimum, maximum);
    let nested = ParSpec {
        concrete: [2, 0, 1, 3, 0, 1, 0, 2],
        connectives: vec![
            (
                ConnectiveSpec::And(vec![concrete.clone(), ParSpec {
                    connectives: vec![(
                        ConnectiveSpec::Or(vec![free.clone(), wildcard.clone()]),
                        2,
                    )],
                    ..scaled.clone()
                }]),
                3,
            ),
            (ConnectiveSpec::Scalar(2, true), 1),
        ],
        ..ParSpec::default()
    };
    for spec in [ParSpec::default(), concrete, free, wildcard, scaled, nested] {
        assert_par(&spec);
    }
}

fn leaf_strategy() -> BoxedStrategy<ParSpec> {
    (prop::array::uniform8(0u8..4), 0u8..3, 0u8..3)
        .prop_map(|(concrete, free, wildcard)| ParSpec {
            concrete,
            free,
            wildcard,
            connectives: Vec::new(),
        })
        .boxed()
}

fn par_strategy() -> BoxedStrategy<ParSpec> {
    leaf_strategy()
        .prop_recursive(2, 24, 3, |inner| {
            let connective = prop_oneof![
                prop::collection::vec(inner.clone(), 0..3).prop_map(ConnectiveSpec::And),
                prop::collection::vec(inner.clone(), 0..3).prop_map(ConnectiveSpec::Or),
                inner.prop_map(|par| ConnectiveSpec::Not(Box::new(par))),
                (0u8..5, any::<bool>())
                    .prop_map(|(kind, value)| ConnectiveSpec::Scalar(kind, value)),
                Just(ConnectiveSpec::VarRef),
                Just(ConnectiveSpec::Missing),
            ];
            (
                leaf_strategy(),
                prop::collection::vec((connective, 0u8..4), 0..3),
            )
                .prop_map(|(mut par, connectives)| {
                    par.connectives = connectives;
                    par
                })
        })
        .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 128,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proptest-regressions/matcher_count_folds.txt"
        )))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn par_count_pda_bounded_shallow_folds_match_independent_reference(spec in par_strategy()) {
        assert_par(&spec);
    }
}
