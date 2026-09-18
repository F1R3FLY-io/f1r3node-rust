use models::rust::rholang::protobuf_encoder;
use proptest::prelude::*;

use super::*;

#[test]
fn map_and_pattern_slots_preserve_pairing_and_pattern_depth() {
    use models::rhoapi::{EMap, EMatches, EPlus, KeyValuePair};
    let ctx = SubCtx {
        depth: 3,
        shift_delta: 5,
    };
    for preserve in [false, true] {
        for mask in 0..4 {
            let map = EMap {
                kvs: vec![
                    KeyValuePair {
                        key: (mask & 1 != 0).then(|| scalar(1)),
                        value: (mask & 2 != 0).then(|| scalar(2)),
                    },
                    KeyValuePair {
                        key: Some(scalar(3)),
                        value: Some(scalar(4)),
                    },
                ],
                ..EMap::default()
            };
            let jobs: Vec<_> = map
                .kvs
                .iter()
                .flat_map(|pair| [pair.key.as_ref(), pair.value.as_ref()])
                .map(|value| required_label(value, ctx, preserve))
                .collect();
            let input = Expr {
                expr_instance: Some(ExprInstance::EMapBody(map)),
            };
            drive_labelled_component(
                &input,
                EncodedFamily::Expr,
                ctx,
                preserve,
                jobs.clone(),
                Vec::new(),
                jobs,
                |result| {
                    let (expr, _) = result.first::<Expr>().unwrap().unwrap();
                    let Some(ExprInstance::EMapBody(map)) = expr.expr_instance else {
                        panic!("map result")
                    };
                    map.kvs
                        .iter()
                        .flat_map(|pair| {
                            [
                                pair.key.as_ref().map_or(0, scalar_label),
                                pair.value.as_ref().map_or(0, scalar_label),
                            ]
                        })
                        .collect()
                },
            );
        }
        for pattern in [false, true] {
            let input = Expr {
                expr_instance: Some(if pattern {
                    ExprInstance::EMatchesBody(EMatches {
                        target: Some(scalar(1)),
                        pattern: Some(scalar(2)),
                    })
                } else {
                    ExprInstance::EPlusBody(EPlus {
                        p1: Some(scalar(1)),
                        p2: Some(scalar(2)),
                    })
                }),
            };
            let jobs = vec![
                required_label(Some(&scalar(1)), ctx, preserve),
                required_label(
                    Some(&scalar(2)),
                    SubCtx {
                        depth: if pattern { 4 } else { 3 },
                        shift_delta: 5,
                    },
                    preserve,
                ),
            ];
            drive_labelled_component(
                &input,
                EncodedFamily::Expr,
                ctx,
                preserve,
                jobs.clone(),
                Vec::new(),
                jobs,
                |result| {
                    let (expr, _) = result.first::<Expr>().unwrap().unwrap();
                    let (left, right) = match expr.expr_instance.unwrap() {
                        ExprInstance::EMatchesBody(value) => (value.target, value.pattern),
                        ExprInstance::EPlusBody(value) => (value.p1, value.p2),
                        _ => panic!("binary result"),
                    };
                    vec![
                        left.as_ref().map_or(0, scalar_label),
                        right.as_ref().map_or(0, scalar_label),
                    ]
                },
            );
        }
    }
}

#[test]
fn identity_order_families_preserve_each_positional_result() {
    use models::rhoapi::{EList, ETuple};
    let ctx = SubCtx {
        depth: 3,
        shift_delta: 5,
    };
    for preserve in [false, true] {
        let values: Vec<_> = (1..=3).map(scalar).collect();
        let jobs: Vec<_> = values
            .iter()
            .map(|value| required_label(Some(value), ctx, preserve))
            .collect();
        let send = Send {
            chan: Some(values[0].clone()),
            data: values[1..].to_vec(),
            ..Send::default()
        };
        drive_labelled_component(
            &send,
            EncodedFamily::Send,
            ctx,
            preserve,
            jobs.clone(),
            Vec::new(),
            jobs.clone(),
            |result| {
                let (output, _) = result.first::<Send>().unwrap().unwrap();
                std::iter::once(output.chan.as_ref().map_or(0, scalar_label))
                    .chain(output.data.iter().map(scalar_label))
                    .collect()
            },
        );
        let conditional = If {
            condition: Some(values[0].clone()),
            if_true: Some(values[1].clone()),
            if_false: Some(values[2].clone()),
            ..If::default()
        };
        drive_labelled_component(
            &conditional,
            EncodedFamily::If,
            ctx,
            preserve,
            jobs.clone(),
            Vec::new(),
            jobs.clone(),
            |result| {
                let (output, _) = result.first::<If>().unwrap().unwrap();
                [output.condition, output.if_true, output.if_false]
                    .iter()
                    .map(|value| value.as_ref().map_or(0, scalar_label))
                    .collect()
            },
        );
        let bundle = Bundle {
            body: Some(values[0].clone()),
            ..Bundle::default()
        };
        drive_labelled_component(
            &bundle,
            EncodedFamily::Bundle,
            ctx,
            preserve,
            jobs[..1].to_vec(),
            Vec::new(),
            jobs[..1].to_vec(),
            |result| {
                let (output, _) = result.first::<Bundle>().unwrap().unwrap();
                vec![output.body.as_ref().map_or(0, scalar_label)]
            },
        );
        for tuple in [false, true] {
            let input = Expr {
                expr_instance: Some(if tuple {
                    ExprInstance::ETupleBody(ETuple {
                        ps: values.clone(),
                        ..ETuple::default()
                    })
                } else {
                    ExprInstance::EListBody(EList {
                        ps: values.clone(),
                        ..EList::default()
                    })
                }),
            };
            drive_labelled_component(
                &input,
                EncodedFamily::Expr,
                ctx,
                preserve,
                jobs.clone(),
                Vec::new(),
                jobs.clone(),
                |result| {
                    let (expr, _) = result.first::<Expr>().unwrap().unwrap();
                    let values = match expr.expr_instance.unwrap() {
                        ExprInstance::ETupleBody(value) => value.ps,
                        ExprInstance::EListBody(value) => value.ps,
                        _ => panic!("sequence result"),
                    };
                    values.iter().map(scalar_label).collect()
                },
            );
        }
        for and in [false, true] {
            let input = Connective {
                connective_instance: Some(if and {
                    ConnectiveInstance::ConnAndBody(ConnectiveBody { ps: values.clone() })
                } else {
                    ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: values.clone() })
                }),
            };
            drive_labelled_component(
                &input,
                EncodedFamily::Connective,
                ctx,
                preserve,
                jobs.clone(),
                Vec::new(),
                jobs.clone(),
                |result| {
                    let (output, _) = result.first::<Connective>().unwrap().unwrap();
                    let body = match output.connective_instance.unwrap() {
                        ConnectiveInstance::ConnAndBody(body)
                        | ConnectiveInstance::ConnOrBody(body) => body,
                        _ => panic!("connective result"),
                    };
                    body.ps.iter().map(scalar_label).collect()
                },
            );
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LabelJob {
    Substitute(i64, SubCtx),
    Preserve(i64),
    Missing(SubCtx),
    Empty,
}

fn scalar(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn scalar_label(par: &Par) -> i64 {
    if par.is_empty() {
        return 0;
    }
    let (expr, count) = par.first::<Expr>().unwrap().unwrap();
    assert_eq!(count, Multiplicity::ONE);
    let Some(ExprInstance::GInt(value)) = expr.expr_instance else {
        panic!("scalar witness")
    };
    value
}

fn required_label(value: Option<&Par>, ctx: SubCtx, preserve: bool) -> LabelJob {
    match value {
        Some(value) if preserve => LabelJob::Preserve(scalar_label(value)),
        Some(value) => LabelJob::Substitute(scalar_label(value), ctx),
        None => LabelJob::Missing(ctx),
    }
}

fn optional_label(value: Option<&Par>, ctx: SubCtx, preserve: bool) -> LabelJob {
    value.map_or(LabelJob::Empty, |value| {
        required_label(Some(value), ctx, preserve)
    })
}

fn result_label(job: &LabelJob) -> i64 {
    match job {
        LabelJob::Substitute(value, _) => value + 10_000,
        LabelJob::Preserve(value) => *value,
        LabelJob::Empty => 0,
        LabelJob::Missing(_) => panic!("a missing job cannot produce a successful witness"),
    }
}

fn drive_labelled_component<T: ParBagComponent>(
    input: &T,
    family: EncodedFamily,
    ctx: SubCtx,
    preserve: bool,
    expected: Vec<LabelJob>,
    expected_order: Vec<usize>,
    wire: Vec<LabelJob>,
    inspect: impl FnOnce(Par) -> Vec<i64>,
) {
    let source = ProtobufRegion::from(protobuf_encoder::encode_to_vec(input));
    let mut construction = ParConstruction::new();
    let empty = construction.empty_par().unwrap();
    let count = Multiplicity::new(3).unwrap();
    let mut work = vec![SubWork::PlannedPar(empty)];
    let mut vals = vec![PlannedSubVal::Par(empty)];
    let env = Env::new();
    descend_region_component(
        EncodedOccurrence {
            family,
            region: Some(source.clone()),
            canonical_ezipper: None,
            multiplicity: count,
        },
        ctx,
        preserve,
        EnvView::new(&env),
        empty,
        &mut construction,
        &mut work,
        &mut vals,
    )
    .unwrap();
    if preserve && !T::semantic_shape_is_complete(input) {
        assert_eq!(work.len(), 1);
        assert!(matches!(work.pop(), Some(SubWork::PlannedPar(value)) if value == empty));
        assert_eq!(vals.len(), 2);
        let Some(PlannedSubVal::Par(value)) = vals.pop() else {
            panic!("preserved opaque result")
        };
        let output = construction.finish(value).unwrap();
        let (component, actual_count) = output.first::<T>().unwrap().unwrap();
        assert_eq!(actual_count, count);
        assert_eq!(
            protobuf_encoder::encode_to_vec(&component),
            source.as_bytes()
        );
        return;
    }
    assert_eq!(vals.len(), 1);
    let label = |region: &ProtobufRegion| {
        let range = region.absolute_range();
        assert!(range.end <= source.len());
        assert_eq!(
            region.as_bytes().as_ptr(),
            source.as_bytes()[range.clone()].as_ptr()
        );
        scalar_label(&protobuf_decoder::decode_par(region.as_bytes()).unwrap())
    };
    for wanted in &expected {
        let observed = match work.pop().expect("one work item per expected child") {
            SubWork::ParRegion(region, child_ctx) => {
                LabelJob::Substitute(label(&region), child_ctx)
            }
            SubWork::ImportParRegion(region) => LabelJob::Preserve(label(&region)),
            SubWork::Par(None, child_ctx) => LabelJob::Missing(child_ctx),
            SubWork::PlannedPar(value) => {
                assert_eq!(value, empty);
                LabelJob::Empty
            }
            _ => panic!("unexpected work while evaluating child order"),
        };
        assert_eq!(&observed, wanted);
    }
    let Some(SubWork::Combine(SubKont::RegionComponentK(kont))) = work.pop() else {
        panic!("exactly one component continuation follows its children");
    };
    assert_eq!(kont.child_count, expected.len());
    assert_eq!(kont.wire_order, expected_order);
    assert_eq!(kont.multiplicity, count);
    assert_eq!(kont.preserve, preserve);
    assert_eq!(work.len(), 1);
    if expected
        .iter()
        .any(|job| matches!(job, LabelJob::Missing(_)))
    {
        return;
    }
    for job in &expected {
        let value = result_label(job);
        let planned = if value == 0 {
            empty
        } else {
            construction.import_par(&scalar(value)).unwrap()
        };
        vals.push(PlannedSubVal::Par(planned));
    }
    combine(
        SubKont::RegionComponentK(kont),
        empty,
        &mut construction,
        &mut work,
        &mut vals,
    )
    .unwrap();
    assert_eq!(vals.len(), 2);
    let Some(PlannedSubVal::Par(result)) = vals.pop() else {
        panic!("one component result")
    };
    assert!(matches!(vals.pop(), Some(PlannedSubVal::Par(value)) if value == empty));
    assert!(matches!(work.pop(), Some(SubWork::PlannedPar(value)) if value == empty));
    let result = construction.finish(result).unwrap();
    assert_eq!(result.cardinality(), 3);
    assert_eq!(
        inspect(result),
        wire.iter().map(result_label).collect::<Vec<_>>()
    );
}

fn verify_receive_schedule(widths: &[usize], mask: u8, preserve: bool) {
    let ctx = SubCtx {
        depth: 3,
        shift_delta: 5,
    };
    let mut label = 1i64;
    let binds = widths
        .iter()
        .enumerate()
        .map(|(index, &width)| {
            let patterns = (0..width)
                .map(|_| {
                    let value = scalar(label);
                    label += 1;
                    value
                })
                .collect();
            let source = if mask & 1 != 0 || index != 0 {
                let value = Some(scalar(label));
                label += 1;
                value
            } else {
                None
            };
            ReceiveBind {
                patterns,
                source,
                free_count: index as i32 + 1,
                ..ReceiveBind::default()
            }
        })
        .collect();
    let input = Receive {
        binds,
        body: (mask & 2 != 0).then(|| scalar(101)),
        condition: (mask & 4 != 0).then(|| scalar(102)),
        bind_count: 7,
        ..Receive::default()
    };
    let mut jobs = Vec::new();
    let mut wire = Vec::new();
    let mut order = Vec::new();
    for bind in &input.binds {
        let source = required_label(bind.source.as_ref(), ctx, preserve);
        let patterns: Vec<_> = bind
            .patterns
            .iter()
            .map(|value| {
                required_label(
                    Some(value),
                    SubCtx {
                        depth: 4,
                        shift_delta: 5,
                    },
                    preserve,
                )
            })
            .collect();
        let start = jobs.len();
        jobs.push(source.clone());
        jobs.extend(patterns.clone());
        wire.extend(patterns);
        wire.push(source);
        order.extend((start + 1)..jobs.len());
        order.push(start);
    }
    let shifted = SubCtx {
        depth: 3,
        shift_delta: 12,
    };
    let body = required_label(input.body.as_ref(), shifted, preserve);
    let condition = optional_label(input.condition.as_ref(), shifted, preserve);
    order.extend([jobs.len(), jobs.len() + 1]);
    jobs.extend([body.clone(), condition.clone()]);
    wire.extend([body, condition]);
    drive_labelled_component(
        &input,
        EncodedFamily::Receive,
        ctx,
        preserve,
        jobs,
        order,
        wire,
        |result| {
            let (output, _) = result.first::<Receive>().unwrap().unwrap();
            assert_eq!(output.condition.is_some(), input.condition.is_some());
            let mut labels = Vec::new();
            for bind in output.binds {
                labels.extend(bind.patterns.iter().map(scalar_label));
                labels.push(bind.source.as_ref().map_or(0, scalar_label));
            }
            labels.push(output.body.as_ref().map_or(0, scalar_label));
            labels.push(output.condition.as_ref().map_or(0, scalar_label));
            labels
        },
    );
}

fn verify_match_schedule(masks: &[u8], preserve: bool) {
    let ctx = SubCtx {
        depth: 3,
        shift_delta: 5,
    };
    let cases = masks
        .iter()
        .enumerate()
        .map(|(index, mask)| {
            let base = index as i64 * 3 + 1;
            MatchCase {
                pattern: (mask & 1 != 0).then(|| scalar(base)),
                source: (mask & 2 != 0).then(|| scalar(base + 1)),
                guard: (mask & 4 != 0).then(|| scalar(base + 2)),
                free_count: index as i32 + 2,
            }
        })
        .collect();
    let input = Match {
        target: Some(scalar(100)),
        cases,
        ..Match::default()
    };
    let target = required_label(input.target.as_ref(), ctx, preserve);
    let mut jobs = vec![target.clone()];
    let mut wire = vec![target];
    let mut order = vec![0];
    for case in &input.cases {
        if !preserve && (case.pattern.is_none() || case.source.is_none()) {
            continue;
        }
        let shifted = SubCtx {
            depth: 3,
            shift_delta: 5 + case.free_count,
        };
        let source = required_label(case.source.as_ref(), shifted, preserve);
        let pattern = required_label(
            case.pattern.as_ref(),
            SubCtx {
                depth: 4,
                shift_delta: 5,
            },
            preserve,
        );
        let guard = optional_label(case.guard.as_ref(), shifted, preserve);
        let start = jobs.len();
        jobs.extend([source.clone(), pattern.clone(), guard.clone()]);
        wire.extend([pattern, source, guard]);
        order.extend([start + 1, start, start + 2]);
    }
    drive_labelled_component(
        &input,
        EncodedFamily::Match,
        ctx,
        preserve,
        jobs,
        order,
        wire,
        |result| {
            let (output, _) = result.first::<Match>().unwrap().unwrap();
            let expected_guards: Vec<_> = input
                .cases
                .iter()
                .filter(|case| preserve || (case.pattern.is_some() && case.source.is_some()))
                .map(|case| case.guard.is_some())
                .collect();
            assert_eq!(
                output
                    .cases
                    .iter()
                    .map(|case| case.guard.is_some())
                    .collect::<Vec<_>>(),
                expected_guards
            );
            let mut labels = vec![output.target.as_ref().map_or(0, scalar_label)];
            for case in output.cases {
                labels.extend([
                    case.pattern.as_ref().map_or(0, scalar_label),
                    case.source.as_ref().map_or(0, scalar_label),
                    case.guard.as_ref().map_or(0, scalar_label),
                ]);
            }
            labels
        },
    );
}

#[test]
fn binder_family_dispatch_and_combine_preserve_semantic_and_wire_orders() {
    for preserve in [false, true] {
        for mask in 0..8 {
            verify_receive_schedule(&[2, 1], mask, preserve);
        }
        verify_match_schedule(&[7, 0, 1, 2, 3, 4, 5, 6, 7], preserve);
        verify_match_schedule(&[3, 7], preserve);
        let ctx = SubCtx {
            depth: 3,
            shift_delta: 5,
        };
        let input = New {
            bind_count: 9,
            p: Some(scalar(1)),
            injections: BTreeMap::from([("z".to_owned(), scalar(3)), ("a".to_owned(), scalar(2))]),
            ..New::default()
        };
        let mut jobs = vec![required_label(
            input.p.as_ref(),
            SubCtx {
                depth: 3,
                shift_delta: 14,
            },
            preserve,
        )];
        jobs.extend(
            input
                .injections
                .values()
                .map(|value| LabelJob::Preserve(scalar_label(value))),
        );
        drive_labelled_component(
            &input,
            EncodedFamily::New,
            ctx,
            preserve,
            jobs.clone(),
            Vec::new(),
            jobs,
            |result| {
                let (output, _) = result.first::<New>().unwrap().unwrap();
                assert_eq!(
                    output
                        .injections
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    ["a", "z"]
                );
                std::iter::once(output.p.as_ref().map_or(0, scalar_label))
                    .chain(output.injections.values().map(scalar_label))
                    .collect()
            },
        );
    }
}

#[test]
fn method_constructor_rotation_preserves_target_and_each_argument() {
    use models::rhoapi::EMethod;
    for preserve in [false, true] {
        for width in [0, 1, 4] {
            let method = EMethod {
                method_name: "schedule-witness".to_owned(),
                target: Some(scalar(1)),
                arguments: (0..width).map(|index| scalar(index + 2)).collect(),
                ..EMethod::default()
            };
            let ctx = SubCtx {
                depth: 3,
                shift_delta: 5,
            };
            let jobs: Vec<_> = std::iter::once(method.target.as_ref().unwrap())
                .chain(method.arguments.iter())
                .map(|value| required_label(Some(value), ctx, preserve))
                .collect();
            let input = Expr {
                expr_instance: Some(ExprInstance::EMethodBody(method)),
            };
            drive_labelled_component(
                &input,
                EncodedFamily::Expr,
                ctx,
                preserve,
                jobs.clone(),
                Vec::new(),
                jobs,
                |result| {
                    let (output, _) = result.first::<Expr>().unwrap().unwrap();
                    let Some(ExprInstance::EMethodBody(method)) = output.expr_instance else {
                        panic!("method result")
                    };
                    std::iter::once(method.target.as_ref().map_or(0, scalar_label))
                        .chain(method.arguments.iter().map(scalar_label))
                        .collect()
                },
            );
        }
    }
}

proptest! {
    #[test]
    fn generated_receive_schedules_bind_contexts_and_result_indices(
        widths in prop::collection::vec(0usize..5, 0..5), mask in 0u8..8, preserve in any::<bool>(),
    ) { verify_receive_schedule(&widths, mask, preserve); }

    #[test]
    fn generated_match_schedules_bind_dropped_cases_and_result_indices(
        masks in prop::collection::vec(0u8..8, 0..12), preserve in any::<bool>(),
    ) { verify_match_schedule(&masks, preserve); }
}

#[derive(Debug, PartialEq, Eq)]
enum ObservedJob {
    Substitute(usize, SubCtx),
    Preserve(usize),
    Missing(SubCtx),
    Empty(PlannedPar),
}

fn retained_plan(count: usize) -> ProtobufCutPlan {
    let data = (0..count)
        .map(|index| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(index as i64 + 1)),
            }])
        })
        .collect();
    let send = Send {
        data,
        ..Send::default()
    };
    cut_at_par(
        ProtobufRegion::from(protobuf_encoder::encode_to_vec(&send)),
        &SEND_RHO_MATCH_PROGRAM,
    )
    .unwrap()
}

fn region_index(plan: &ProtobufCutPlan, region: &ProtobufRegion) -> usize {
    let index = plan
        .cuts()
        .iter()
        .position(|cut| cut.region().absolute_range() == region.absolute_range())
        .expect("job references a retained cut");
    let original = plan.cuts()[index].region();
    assert_eq!(original.as_bytes(), region.as_bytes());
    assert_eq!(original.as_bytes().as_ptr(), region.as_bytes().as_ptr());
    index
}

fn verify_selection(requests: &[u8], preserve: bool) {
    let consumed = requests.iter().filter(|&&request| request < 3).count();
    let plan = retained_plan(consumed);
    assert_eq!(plan.cuts().len(), consumed);
    let mut cursor = RegionCutCursor::new(&plan);
    let mut construction = ParConstruction::new();
    let empty = construction.empty_par().unwrap();
    let mut children = Vec::new();
    let mut expected = Vec::new();
    let mut ordinal = 0;
    for (index, &request) in requests.iter().enumerate() {
        let ctx = SubCtx {
            depth: index as i32,
            shift_delta: index as i32 + 2,
        };
        match request {
            0 => {
                children.push(cursor.required_mode(true, ctx, preserve).unwrap());
                expected.push(if preserve {
                    ObservedJob::Preserve(ordinal)
                } else {
                    ObservedJob::Substitute(ordinal, ctx)
                });
                ordinal += 1;
            }
            1 => {
                children.push(cursor.preserved().unwrap());
                expected.push(ObservedJob::Preserve(ordinal));
                ordinal += 1;
            }
            2 => {
                assert_eq!(region_index(&plan, &cursor.take().unwrap()), ordinal);
                ordinal += 1;
            }
            3 => {
                children.push(cursor.required_mode(false, ctx, preserve).unwrap());
                expected.push(ObservedJob::Missing(ctx));
            }
            4 => {
                children.push(cursor.optional_mode(false, ctx, empty, preserve).unwrap());
                expected.push(ObservedJob::Empty(empty));
            }
            _ => unreachable!(),
        }
        assert_eq!(cursor.at, ordinal);
    }
    assert_eq!(ordinal, consumed);
    assert!(cursor.take().is_err());
    assert_eq!(cursor.at, consumed);
    cursor.finish().unwrap();
    let mut work = vec![SubWork::PlannedPar(empty)];
    schedule_region_component(
        RegionComponentShell::Send(Send::default()),
        Multiplicity::ONE,
        children,
        Vec::new(),
        preserve,
        &mut work,
    );
    let job_count = expected.len();
    for expected in expected {
        let actual = match work.pop().unwrap() {
            SubWork::ParRegion(region, ctx) => {
                ObservedJob::Substitute(region_index(&plan, &region), ctx)
            }
            SubWork::ImportParRegion(region) => ObservedJob::Preserve(region_index(&plan, &region)),
            SubWork::Par(None, ctx) => ObservedJob::Missing(ctx),
            SubWork::PlannedPar(value) => ObservedJob::Empty(value),
            _ => panic!("continuation or unrelated work ran before the selected child"),
        };
        assert_eq!(actual, expected);
    }
    let Some(SubWork::Combine(SubKont::RegionComponentK(kont))) = work.pop() else {
        panic!("selected jobs must be followed by exactly one component continuation");
    };
    assert_eq!(kont.child_count, job_count);
    assert_eq!(kont.multiplicity, Multiplicity::ONE);
    assert!(kont.wire_order.is_empty());
    assert_eq!(kont.preserve, preserve);
    assert!(matches!(work.pop(), Some(SubWork::PlannedPar(value)) if value == empty));
    assert!(work.is_empty());
}

#[test]
fn dropped_and_synthetic_slots_are_not_a_permutation_of_retained_cuts() {
    verify_selection(&[2, 4, 1], false);
    verify_selection(&[3, 0, 4, 2, 1], false);
    verify_selection(&[3, 0, 4, 2, 1], true);
    verify_selection(&[], false);
}

#[test]
fn region_cut_cursor_rejects_unconsumed_and_exhausted_plans() {
    let plan = retained_plan(2);
    let mut cursor = RegionCutCursor::new(&plan);
    cursor.take().unwrap();
    assert!(cursor.finish().is_err());
    let plan = retained_plan(0);
    let mut cursor = RegionCutCursor::new(&plan);
    assert!(cursor.take().is_err());
    assert_eq!(cursor.at, 0);
    cursor.finish().unwrap();
}

proptest! {
    #[test]
    fn generated_slot_selection_preserves_prefix_identity_and_semantic_order(
        requests in prop::collection::vec(0u8..5, 0..48),
        preserve in any::<bool>(),
    ) {
        verify_selection(&requests, preserve);
    }
}
