fn recursive_score_oracle(stack: &CostTokenStack) -> Vec<u8> {
    authoritative_constructor(
        Score::COST_TOKEN_STACK,
        stack
            .layers
            .iter()
            .map(authoritative_cost_signature_score)
            .collect(),
    )
}

fn assert_canonical_stack_at_admission_source(stack: &CostTokenStack) {
    let body = protobuf_encoder::encode_to_vec(stack);
    let recursive = CostTokenStack::decode(body.as_slice()).expect("recursive Prost stack decode");
    assert_eq!(&recursive, stack);
    assert_eq!(recursive.encode_to_vec(), body);

    let expected_score = recursive_score_oracle(&recursive);
    with_component_key(&recursive, |key| {
        let trailer = key.len() - std::mem::size_of::<u64>();
        let score_len = usize::try_from(u64::from_be_bytes(key[trailer..].try_into().unwrap()))
            .expect("test score length fits usize");
        let score_start = PAR_BAG_KEY_DOMAIN.len() + 1;
        let score_end = score_start + score_len;
        assert_eq!(&key[score_start..score_end], expected_score.as_slice());
        assert_eq!(&key[score_end..trailer], body.as_slice());
    });

    rpb3_root_finish_trace::start();
    let admission = validate_canonical_protobuf(
        ProtobufRegion::copy_from_slice(&body),
        &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
    )
    .expect("generated canonical CostTokenStack admits");
    let trace = rpb3_root_finish_trace::take();
    rpb3_root_finish_trace::assert_driver_step_trace(&trace, &COSTTOKENSTACK_RHO_MATCH_PROGRAM);
    assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
    assert_eq!(
        admission.component(),
        Some(CanonicalComponentAdmission {
            family: ParComponentTag::CostTokenStack,
            canonical_path_stable: false,
            complete: true,
        })
    );
    assert_eq!(admission.stats().typed_decodes, 0);
    assert_eq!(admission.stats().byte_native_validations, 1);
    assert_eq!(trace.remaining_frames, Some(0));
    assert_eq!(trace.complete, trace.pre_pop);
    assert_eq!(trace.pre_pop, trace.returned);

    let entries = trace
        .score_arena
        .as_ref()
        .expect("CostTokenStack root return captures its score source");
    let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
    let returned = trace
        .returned
        .as_ref()
        .expect("CostTokenStack root returns");
    rpb3_root_finish_trace::assert_summary_score_source(
        returned,
        &transcripts,
        "CostTokenStack root",
    );
    assert_eq!(
        returned.score_bytes.as_deref(),
        Some(expected_score.as_slice())
    );
    assert_eq!(
        returned
            .score_handle
            .expect("CostTokenStack root score")
            .index()
            + 1,
        entries.len()
    );
    let layer_children: Vec<_> = trace
        .children
        .iter()
        .filter(|child| std::ptr::eq(child.program, &COSTSIGNATURE_RHO_MATCH_PROGRAM))
        .collect();
    assert_eq!(layer_children.len(), recursive.layers.len());
    for (child, layer) in layer_children.into_iter().zip(&recursive.layers) {
        assert_eq!(child.body, layer.encode_to_vec());
        rpb3_root_finish_trace::assert_summary_score_source(
            &child.summary,
            &transcripts,
            "CostTokenStack signature child",
        );
        assert_eq!(
            child.summary.score_bytes.as_deref(),
            Some(authoritative_cost_signature_score(layer).as_slice())
        );
    }

    assert_eq!(
        validate_canonical_component_protobuf(
            ProtobufRegion::copy_from_slice(&body),
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
            ParComponentTag::CostTokenStack,
            &expected_score,
        )
        .expect("canonical component admission accepts its recursive score"),
        admission.component().unwrap()
    );

    let par = Par::default().with_cost_token_stacks(vec![recursive]);
    let par_body = protobuf_encoder::encode_to_vec(&par);
    let recursive_par = Par::decode(par_body.as_slice()).expect("recursive Prost Par decode");
    assert_eq!(recursive_par.encode_to_vec(), par_body);
    let expected_par_score = authoritative_constructor(Score::PAR, vec![
        expected_score.clone(),
        authoritative_integer_atom(0),
    ]);
    assert_eq!(authoritative_par_score(&recursive_par), expected_par_score);
    rpb3_root_finish_trace::start();
    let par_admission = validate_canonical_protobuf(
        ProtobufRegion::copy_from_slice(&par_body),
        &PAR_PROTOBUF_REGION_PROGRAM,
    )
    .expect("Par containing the tenth family admits");
    let par_trace = rpb3_root_finish_trace::take();
    rpb3_root_finish_trace::assert_driver_step_trace(&par_trace, &PAR_PROTOBUF_REGION_PROGRAM);
    assert_eq!(
        par_admission.score_bytes(),
        Some(expected_par_score.as_slice())
    );
    let stack_children: Vec<_> = par_trace
        .children
        .iter()
        .filter(|child| std::ptr::eq(child.program, &COSTTOKENSTACK_RHO_MATCH_PROGRAM))
        .collect();
    assert_eq!(stack_children.len(), 1);
    assert_eq!(stack_children[0].body, body);
    assert_eq!(
        stack_children[0].summary.score_bytes.as_deref(),
        Some(expected_score.as_slice())
    );

    let mut wrong_score = expected_score.clone();
    let last = wrong_score.len() - 1;
    wrong_score[last] ^= 1;
    assert!(matches!(
        validate_canonical_component_protobuf(
            ProtobufRegion::copy_from_slice(&body),
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
            ParComponentTag::CostTokenStack,
            &wrong_score,
        ),
        Err(CanonicalProtobufAdmissionError::ScoreMismatch { .. })
    ));
}

fn one_atom_stack_body(atom_body: &[u8]) -> Vec<u8> {
    let mut layer = vec![0x0a];
    prost::encoding::encode_varint(atom_body.len() as u64, &mut layer);
    layer.extend_from_slice(atom_body);
    let mut stack = vec![0x0a];
    prost::encoding::encode_varint(layer.len() as u64, &mut stack);
    stack.extend_from_slice(&layer);
    stack
}

fn assert_malformed_stack_at_admission_source(body: &[u8], baseline_score: &[u8]) {
    let recursive = CostTokenStack::decode(body);
    let score = match &recursive {
        Ok(decoded) => {
            let canonical = decoded.encode_to_vec();
            assert_ne!(
                canonical, body,
                "mutant must change the canonical byte image"
            );
            let score = recursive_score_oracle(decoded);
            assert!(validate_canonical_component_protobuf(
                ProtobufRegion::copy_from_slice(&canonical),
                &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
                ParComponentTag::CostTokenStack,
                &score,
            )
            .is_ok());
            score
        }
        Err(_) => baseline_score.to_vec(),
    };
    assert!(
        validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(body),
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
        )
        .is_err(),
        "production root accepted a body without a recursive fixed point: {body:?}"
    );
    assert!(
        validate_canonical_component_protobuf(
            ProtobufRegion::copy_from_slice(body),
            &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
            ParComponentTag::CostTokenStack,
            &score,
        )
        .is_err(),
        "production claimed-score admission accepted a malformed body: {body:?}"
    );
    let mut par_body = vec![0x6a];
    prost::encoding::encode_varint(body.len() as u64, &mut par_body);
    par_body.extend_from_slice(body);
    let recursive_par_has_fixed_point = Par::decode(par_body.as_slice())
        .map(|decoded| decoded.encode_to_vec() == par_body)
        .unwrap_or(false);
    assert!(!recursive_par_has_fixed_point);
    assert!(
        validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&par_body),
            &PAR_PROTOBUF_REGION_PROGRAM,
        )
        .is_err(),
        "production Par accepted a malformed CostTokenStack child: {body:?}"
    );
}

fn arbitrary_signature_atom() -> impl Strategy<Value = CostSignatureAtom> {
    prop_oneof![
        Just(CostSignatureAtom::default()),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(|value| CostSignatureAtom {
            form: Some(Form::Ground(value))
        }),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(|value| CostSignatureAtom {
            form: Some(Form::Quote(value))
        }),
        any::<i32>().prop_map(|index| CostSignatureAtom {
            form: Some(Form::Bound(CostBoundSignature { index })),
        }),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(|value| CostSignatureAtom {
            form: Some(Form::RuntimeName(value))
        }),
    ]
}

#[test]
fn empty_and_all_atom_forms_bind_the_tenth_family_to_production_admission() {
    assert_canonical_stack_at_admission_source(&CostTokenStack::default());
    assert_canonical_stack_at_admission_source(&CostTokenStack {
        layers: vec![CostSignature::default(), CostSignature {
            atoms: vec![
                CostSignatureAtom::default(),
                CostSignatureAtom {
                    form: Some(Form::Ground(vec![0, 0xff])),
                },
                CostSignatureAtom {
                    form: Some(Form::Quote(vec![0, 7])),
                },
                CostSignatureAtom {
                    form: Some(Form::Bound(CostBoundSignature { index: i32::MIN })),
                },
                CostSignatureAtom {
                    form: Some(Form::RuntimeName(vec![8, 0, 9])),
                },
            ],
        }],
    });
}

#[test]
fn mixed_ten_family_par_with_nested_epathmap_matches_frozen_root_and_child_scores() {
    let send = Send::default();
    let receive = Receive::default();
    let new = New::default();
    let expr = Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
            [(gint(1), gint(2))],
            Vec::new(),
            false,
            None,
        ))),
    };
    let matched = Match::default();
    let unforgeable = GUnforgeable::default();
    let connective = Connective::default();
    let bundle = Bundle::default();
    let conditional = If::default();
    let stack = CostTokenStack {
        layers: vec![CostSignature::default()],
    };
    let par = Par::default()
        .with_sends(vec![send.clone()])
        .with_receives(vec![receive.clone()])
        .with_news(vec![new.clone()])
        .with_exprs(vec![expr.clone()])
        .with_matches(vec![matched.clone()])
        .with_unforgeables(vec![unforgeable.clone()])
        .with_connectives(vec![connective.clone()])
        .with_bundles(vec![bundle.clone()])
        .with_conditionals(vec![conditional.clone()])
        .with_cost_token_stacks(vec![stack.clone()]);
    let body = protobuf_encoder::encode_to_vec(&par);
    let recursive = Par::decode(body.as_slice()).expect("mixed Par recursive decode");
    assert_eq!(recursive.encode_to_vec(), body);
    let expected_score = authoritative_par_score(&recursive);

    rpb3_root_finish_trace::start();
    let admission = validate_canonical_protobuf(
        ProtobufRegion::copy_from_slice(&body),
        &PAR_PROTOBUF_REGION_PROGRAM,
    )
    .expect("mixed ten-family Par admits");
    let trace = rpb3_root_finish_trace::take();
    rpb3_root_finish_trace::assert_driver_step_trace(&trace, &PAR_PROTOBUF_REGION_PROGRAM);
    assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
    assert_eq!(admission.stats().typed_decodes, 0);
    assert_eq!(admission.stats().epathmaps, 1);
    assert_eq!(trace.complete, trace.pre_pop);
    assert_eq!(trace.pre_pop, trace.returned);

    macro_rules! check_component {
        ($value:expr, $program:expr, $label:literal) => {{
            let expected_body = protobuf_encoder::encode_to_vec(&$value);
            let children = trace
                .children
                .iter()
                .filter(|child| {
                    std::ptr::eq(child.program, $program) && child.body == expected_body
                })
                .collect::<Vec<_>>();
            assert_eq!(children.len(), 1, "{} must finish exactly once", $label);
            assert_nested_component_finish(&$value, children[0], $label);
        }};
    }
    check_component!(send, &SEND_RHO_MATCH_PROGRAM, "mixed Par Send");
    check_component!(receive, &RECEIVE_RHO_MATCH_PROGRAM, "mixed Par Receive");
    check_component!(new, &NEW_RHO_MATCH_PROGRAM, "mixed Par New");
    check_component!(expr, &EXPR_RHO_MATCH_PROGRAM, "mixed Par Expr");
    check_component!(matched, &MATCH_RHO_MATCH_PROGRAM, "mixed Par Match");
    check_component!(
        unforgeable,
        &GUNFORGEABLE_RHO_MATCH_PROGRAM,
        "mixed Par GUnforgeable"
    );
    check_component!(
        connective,
        &CONNECTIVE_RHO_MATCH_PROGRAM,
        "mixed Par Connective"
    );
    check_component!(bundle, &BUNDLE_RHO_MATCH_PROGRAM, "mixed Par Bundle");
    check_component!(conditional, &IF_RHO_MATCH_PROGRAM, "mixed Par If");
    check_component!(
        stack,
        &COSTTOKENSTACK_RHO_MATCH_PROGRAM,
        "mixed Par CostTokenStack"
    );
    assert!(trace
        .children
        .iter()
        .any(|child| std::ptr::eq(child.program, &EPATHMAP_RHO_MATCH_PROGRAM)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_complete_layers_match_recursive_semantic_and_root_source_oracles(
        layers in prop::collection::vec(
            prop::collection::vec(arbitrary_signature_atom(), 0..5), 0..5,
        ),
    ) {
        let stack = CostTokenStack {
            layers: layers.into_iter().map(|atoms| CostSignature { atoms }).collect(),
        };
        assert_canonical_stack_at_admission_source(&stack);
    }

    #[test]
    fn generated_malformed_layers_disagree_with_recursive_fixed_point_and_are_rejected(
        ground in prop::collection::vec(any::<u8>(), 0..24),
        quote in prop::collection::vec(any::<u8>(), 0..24),
    ) {
        let atom = CostSignatureAtom { form: Some(Form::Ground(ground)) };
        let canonical = CostTokenStack {
            layers: vec![CostSignature { atoms: vec![atom.clone()] }],
        };
        let canonical_body = canonical.encode_to_vec();
        assert_eq!(canonical_body, one_atom_stack_body(&atom.encode_to_vec()));
        let baseline_score = recursive_score_oracle(&canonical);
        assert_canonical_stack_at_admission_source(&canonical);

        let mut unknown_atom = atom.encode_to_vec();
        unknown_atom.extend_from_slice(&[0x28, 0x01]);
        assert_malformed_stack_at_admission_source(
            &one_atom_stack_body(&unknown_atom), &baseline_score,
        );

        let mut duplicate_oneof = atom.encode_to_vec();
        duplicate_oneof.extend_from_slice(&[0x12, quote.len() as u8]);
        duplicate_oneof.extend_from_slice(&quote);
        assert_malformed_stack_at_admission_source(
            &one_atom_stack_body(&duplicate_oneof), &baseline_score,
        );

        let explicit_bound_default = [0x1a, 0x02, 0x08, 0x00];
        assert_malformed_stack_at_admission_source(
            &one_atom_stack_body(&explicit_bound_default), &baseline_score,
        );

        let mut layer_with_unknown = canonical_body[2..].to_vec();
        layer_with_unknown.extend_from_slice(&[0x18, 0x01]);
        let mut unknown_layer = vec![0x0a, layer_with_unknown.len() as u8];
        unknown_layer.extend_from_slice(&layer_with_unknown);
        assert_malformed_stack_at_admission_source(&unknown_layer, &baseline_score);

        let mut unknown_stack = canonical_body.clone();
        unknown_stack.extend_from_slice(&[0x18, 0x01]);
        assert_malformed_stack_at_admission_source(&unknown_stack, &baseline_score);

        let mut nonminimal_outer_length = vec![0x0a, canonical_body[1] | 0x80, 0x00];
        nonminimal_outer_length.extend_from_slice(&canonical_body[2..]);
        assert_malformed_stack_at_admission_source(&nonminimal_outer_length, &baseline_score);

        let truncated = &canonical_body[..canonical_body.len() - 1];
        assert_malformed_stack_at_admission_source(truncated, &baseline_score);
    }

    #[test]
    fn generated_multilayer_stack_rejects_a_malformed_layer_at_every_position(
        layers in prop::collection::vec(
            prop::collection::vec(arbitrary_signature_atom(), 0..2), 2..5,
        ),
    ) {
        let stack = CostTokenStack {
            layers: layers.into_iter().map(|atoms| CostSignature { atoms }).collect(),
        };
        assert_canonical_stack_at_admission_source(&stack);
        let baseline_score = recursive_score_oracle(&stack);

        for malformed_at in 0..stack.layers.len() {
            let mut body = Vec::new();
            for (index, layer) in stack.layers.iter().enumerate() {
                let mut layer_body = layer.encode_to_vec();
                if index == malformed_at {
                    layer_body.extend_from_slice(&[0x18, 0x01]);
                }
                body.push(0x0a);
                prost::encoding::encode_varint(layer_body.len() as u64, &mut body);
                body.extend_from_slice(&layer_body);
            }
            assert_malformed_stack_at_admission_source(&body, &baseline_score);
        }
    }
}
