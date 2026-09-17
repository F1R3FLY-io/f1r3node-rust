use std::cell::Cell;

use proptest::prelude::*;

use super::*;
use crate::rhoapi::{Bundle, Connective, ESet, If, Match, New, Receive, Send, Var};
use crate::rust::rholang::protobuf_cursor::CanonicalFieldCursor;

thread_local! {
    static DECODES: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_decode() { DECODES.with(|value| value.set(value.get() + 1)); }

#[test]
#[ignore = "experiment 345: run alone under resource caps after checking idle cores"]
fn split_prefilter_paired_measurement() {
    use std::hint::black_box;
    use std::time::Instant;

    let sample_count: usize = std::env::var("SPLIT_SAMPLES")
        .unwrap_or_else(|_| "7".into())
        .parse()
        .unwrap();
    let warmups = 3;
    assert!(sample_count > 0);
    let mut cases = vec![
        ("empty_list", carrier(list(Vec::new())), 1000usize),
        (
            "flat_list",
            carrier(list(vec![expr_carrier(ExprInstance::GInt(4)); 4])),
            1000,
        ),
        ("scalar", expr_carrier(ExprInstance::GInt(7)), 1000),
    ];
    for (name, depth, batch) in [("list_64", 64, 3), ("list_128", 128, 1)] {
        cases.push((
            name,
            Par::wrap_unary_elist(depth, expr_carrier(ExprInstance::GInt(7))).unwrap(),
            batch,
        ));
    }
    for (name, depth) in [("set_64", 64), ("set_128", 128)] {
        cases.push((
            name,
            expr_carrier(ExprInstance::ESetBody(ESet {
                ps: vec![
                    Par::wrap_unary_elist(depth, expr_carrier(ExprInstance::GInt(7))).unwrap(),
                ],
                ..ESet::default()
            })),
            1,
        ));
    }
    println!("SPLIT_HEADER\tcase\tarm\trole\tsample\tbatch\telapsed_ns");
    for (name, par, batch) in cases {
        let expected = frozen_split_carrier_list(&par).map(|value| value.encode_to_vec());
        for sample in 0..(1 + warmups + sample_count) {
            let role = if sample == 0 {
                "pilot"
            } else if sample <= warmups {
                "warmup"
            } else {
                "retained"
            };
            let arms = if sample % 2 == 0 {
                ["frozen", "prefilter"]
            } else {
                ["prefilter", "frozen"]
            };
            for arm in arms {
                let mut elapsed = 0;
                for _ in 0..batch {
                    let start = Instant::now();
                    let result = black_box(if arm == "frozen" {
                        record_decode();
                        frozen_split_carrier_list(black_box(&par))
                    } else {
                        split_carrier_list(black_box(&par))
                    });
                    elapsed += start.elapsed().as_nanos();
                    assert_eq!(result.as_ref().map(Message::encode_to_vec), expected);
                    drop(result);
                }
                println!("SPLIT_ROW\t{name}\t{arm}\t{role}\t{sample}\t{batch}\t{elapsed}");
            }
        }
    }
}

fn frozen_split_carrier_list(par: &Par) -> Option<EList> {
    if par.family_cardinality::<Send>() != 0
        || par.family_cardinality::<Receive>() != 0
        || par.family_cardinality::<New>() != 0
        || par.family_cardinality::<Match>() != 0
        || par.family_cardinality::<Bundle>() != 0
        || par.family_cardinality::<Connective>() != 0
        || par.family_cardinality::<If>() != 0
        || par.family_cardinality::<GUnforgeable>() != 0
        || !par.locally_free().is_empty()
        || par.connective_used()
        || par.family_cardinality::<Expr>() != 1
    {
        return None;
    }
    let (expr, multiplicity) = par.first::<Expr>().ok()??;
    if multiplicity.get() != 1 {
        return None;
    }
    match expr.expr_instance {
        Some(ExprInstance::EListBody(list))
            if list.remainder.is_none()
                && list.locally_free.is_empty()
                && !list.connective_used =>
        {
            Some(list)
        }
        _ => None,
    }
}

fn frozen_path(par: &Par) -> (Vec<u8>, bool) {
    let mut machine = EncMachine::new();
    let stable = machine.classify_stability(par).unwrap();
    if let Some(list) = frozen_split_carrier_list(par) {
        machine.ops.push(EncOp::EmitTerminator);
        for child in list.ps.into_iter().rev() {
            machine.ops.push(EncOp::Segment {
                par: EncPar::Owned(child),
                known_stable: stable.then_some(true),
                top_level: true,
            });
        }
    } else {
        machine.ops.push(EncOp::Segment {
            par: EncPar::Borrowed(par),
            known_stable: Some(stable),
            top_level: true,
        });
    }
    machine.run().unwrap();
    (machine.root, stable)
}

fn assert_equivalent(par: &Par) {
    let before = par.encode_to_vec();
    let expected = frozen_split_carrier_list(par).map(|list| list.encode_to_vec());
    assert_eq!(
        split_carrier_list(par).map(|list| list.encode_to_vec()),
        expected
    );
    assert_eq!(encode_trie_path_with_stability(par), frozen_path(par));
    assert_eq!(par.encode_to_vec(), before);
}

fn list(ps: Vec<Par>) -> EList {
    EList {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }
}

fn carrier(body: EList) -> Par { expr_carrier(ExprInstance::EListBody(body)) }

#[test]
fn split_prefilter_preserves_all_outer_guards_and_multiplicity() {
    let body = list(vec![expr_carrier(ExprInstance::GInt(7))]);
    let clear = carrier(body.clone());
    assert!(split_carrier_list(&clear).is_some());
    for other in [
        Par::default(),
        clear.with_exprs(vec![Expr::default()]),
        clear.with_exprs(vec![
            Expr {
                expr_instance: Some(ExprInstance::EListBody(body.clone()))
            };
            2
        ]),
        clear.with_sends(vec![Send::default()]),
        clear.with_receives(vec![Receive::default()]),
        clear.with_news(vec![New::default()]),
        clear.with_matches(vec![Match::default()]),
        clear.with_bundles(vec![Bundle::default()]),
        clear.with_connectives(vec![Connective::default()]),
        clear.with_conditionals(vec![If::default()]),
        clear.with_unforgeables(vec![GUnforgeable::default()]),
        clear.with_locally_free(vec![1]),
        clear.with_connective_used(true),
    ] {
        assert!(frozen_split_carrier_list(&other).is_none());
        assert_equivalent(&other);
    }
}

#[test]
fn split_prefilter_retains_list_metadata_and_ordered_duplicate_children() {
    let children = vec![
        expr_carrier(ExprInstance::GInt(4)),
        expr_carrier(ExprInstance::GInt(9)).with_locally_free(vec![1]),
        expr_carrier(ExprInstance::GInt(4)),
    ];
    let body = list(children);
    assert_equivalent(&carrier(body.clone()));
    assert_equivalent(&carrier(list(Vec::new())));
    for changed in [
        EList {
            locally_free: vec![1],
            ..body.clone()
        },
        EList {
            connective_used: true,
            ..body.clone()
        },
        EList {
            remainder: Some(Var::default()),
            ..body.clone()
        },
    ] {
        let par = carrier(changed);
        assert!(split_carrier_list(&par).is_none());
        assert_equivalent(&par);
    }
}

#[test]
fn split_prefilter_decode_boundary_control() {
    let nested = Par::wrap_unary_elist(64, expr_carrier(ExprInstance::GInt(1))).unwrap();
    let nonlist = expr_carrier(ExprInstance::ESetBody(ESet {
        ps: vec![nested],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }));
    DECODES.with(|value| value.set(0));
    assert!(split_carrier_list(&nonlist).is_none());
    assert_eq!(DECODES.with(Cell::get), 0);
    let actual_list = carrier(list(vec![nonlist]));
    DECODES.with(|value| value.set(0));
    assert!(split_carrier_list(&actual_list).is_some());
    assert_eq!(DECODES.with(Cell::get), 1);
}

#[test]
fn split_prefilter_inconclusive_framing_controls() {
    for bytes in [&[0x80][..], &[0xa2, 1, 2, 0][..], &[0][..]] {
        assert!(CanonicalFieldCursor::new(bytes).next_field().is_err());
        assert!(split_header_may_be_list(bytes));
    }
    assert!(split_header_may_be_list(&[0xa2, 1, 0]));
    assert!(split_header_may_be_list(&[0xa0, 1, 0]));
    assert!(!split_header_may_be_list(&[0xaa, 1, 0]));
    assert!(!split_header_may_be_list(&[]));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn split_prefilter_matches_frozen_classifier_and_path(
        values in prop::collection::vec(any::<i64>(), 0..8),
        child_marker in any::<bool>(),
        root_marker in any::<bool>(),
        list_marker in any::<bool>(),
        remainder in any::<bool>(),
        root_lf in prop::collection::vec(any::<u8>(), 0..3),
        list_lf in prop::collection::vec(any::<u8>(), 0..3),
        shape in 0u8..5,
    ) {
        let children = values.into_iter().map(|value|
            expr_carrier(ExprInstance::GInt(value)).with_connective_used(child_marker)
        ).collect();
        let body = EList {
            ps: children, locally_free: list_lf, connective_used: list_marker,
            remainder: remainder.then(Var::default),
        };
        let par = match shape {
            0 => carrier(body),
            1 => expr_carrier(ExprInstance::ESetBody(ESet {
                ps: body.ps, locally_free: body.locally_free,
                connective_used: body.connective_used, remainder: body.remainder,
            })),
            2 => expr_carrier(ExprInstance::ETupleBody(ETuple {
                ps: body.ps, locally_free: body.locally_free, connective_used: body.connective_used,
            })),
            3 => expr_carrier(ExprInstance::GInt(0)),
            _ => Par::default().with_exprs(vec![Expr::default()]),
        }.with_locally_free(root_lf).with_connective_used(root_marker);
        assert_equivalent(&par);
    }
}
