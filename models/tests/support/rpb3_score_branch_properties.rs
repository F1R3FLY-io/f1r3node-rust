fn property_component_source<T: ParBagComponent>(
    value: &T,
    program: &'static RhoMatchProgram,
    expected_score: &[u8],
) -> rpb3_root_finish_trace::RootFinishTrace {
    let bytes = protobuf_encoder::encode_to_vec(value);
    let (trace, transcripts) = checked_property_finish(value, &bytes, program, true);
    let returned = trace.returned.as_ref().expect("component returned");
    let handle = returned
        .score_handle
        .expect("component has a score")
        .index();
    assert_eq!(transcripts[handle], expected_score);
    assert_eq!(returned.score_bytes.as_deref(), Some(expected_score));
    assert_eq!(handle + 1, transcripts.len());
    assert_eq!(trace.remaining_frames, Some(0));
    trace
}

fn property_gint_score(value: i64) -> Vec<u8> {
    authoritative_constructor(2, vec![authoritative_integer_atom(value)])
}

fn property_par_gint_score(value: i64) -> Vec<u8> {
    authoritative_constructor(999, vec![
        property_gint_score(value),
        authoritative_integer_atom(0),
    ])
}

fn property_rejected_without_root(bytes: &[u8], program: &'static RhoMatchProgram) {
    rpb3_root_finish_trace::start();
    assert!(validate_canonical_protobuf(ProtobufRegion::copy_from_slice(bytes), program).is_err());
    let trace = rpb3_root_finish_trace::take();
    assert!(trace.returned.is_none());
}

fn property_derived_expr_payload(
    bytes: &[u8],
    program: &'static RhoMatchProgram,
) -> (Option<Vec<u8>>, bool, bool, Option<Vec<u8>>) {
    let source = IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(bytes), 0);
    let (summary, scores, _) = drive_admission(
        FrameRequest::Message {
            region: source,
            program,
        },
        bytes.len(),
    )
    .expect("generated child derives a summary from wire");
    let FrameSummary::Message(MessageSummary {
        semantic: SemanticSummary::ExprPayload(payload),
    }) = summary
    else {
        panic!("generated child derives an Expr payload");
    };
    let score = payload.score.map(|handle| scores.cursor(handle).collect());
    let snapshot = payload
        .pathmap_snapshot
        .map(|region| region.bytes().collect());
    (
        score,
        payload.canonical_fixed_point,
        payload.canonical_path_stable,
        snapshot,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_component_complete_and_incomplete_finishes_match_authoritative_keys(
        first in any::<i16>(),
        second in any::<i16>(),
        flag in any::<bool>(),
    ) {
        let child = gint(i64::from(first));
        let other = gint(i64::from(second));

        let complete_send = Send {
            chan: Some(child.clone()),
            data: vec![other.clone()],
            persistent: flag,
            ..Send::default()
        };
        let incomplete_send = Send {
            data: vec![other.clone()],
            persistent: flag,
            ..Send::default()
        };
        assert!(Send::semantic_shape_is_complete(&complete_send));
        assert!(!Send::semantic_shape_is_complete(&incomplete_send));
        assert_component_score_state(&complete_send, &SEND_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_send, &SEND_RHO_MATCH_PROGRAM, false);

        let complete_receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![child.clone()],
                source: Some(other.clone()),
                ..ReceiveBind::default()
            }],
            body: Some(child.clone()),
            bind_count: 1,
            persistent: flag,
            ..Receive::default()
        };
        let incomplete_receive = Receive {
            persistent: flag,
            ..Receive::default()
        };
        assert!(Receive::semantic_shape_is_complete(&complete_receive));
        assert!(!Receive::semantic_shape_is_complete(&incomplete_receive));
        assert_component_score_state(&complete_receive, &RECEIVE_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_receive, &RECEIVE_RHO_MATCH_PROGRAM, false);

        let complete_new = New {
            p: Some(child.clone()),
            ..New::default()
        };
        let incomplete_new = New {
            bind_count: 1,
            ..New::default()
        };
        assert!(New::semantic_shape_is_complete(&complete_new));
        assert!(!New::semantic_shape_is_complete(&incomplete_new));
        assert_component_score_state(&complete_new, &NEW_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_new, &NEW_RHO_MATCH_PROGRAM, false);

        let complete_match = Match {
            target: Some(child.clone()),
            cases: vec![MatchCase {
                pattern: Some(child.clone()),
                source: Some(other.clone()),
                ..MatchCase::default()
            }],
            connective_used: flag,
            ..Match::default()
        };
        let incomplete_match = Match {
            connective_used: flag,
            ..Match::default()
        };
        assert!(Match::semantic_shape_is_complete(&complete_match));
        assert!(!Match::semantic_shape_is_complete(&incomplete_match));
        assert_component_score_state(&complete_match, &MATCH_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_match, &MATCH_RHO_MATCH_PROGRAM, false);

        let complete_bundle = Bundle {
            body: Some(child.clone()),
            read_flag: flag,
            write_flag: !flag,
        };
        let incomplete_bundle = Bundle {
            body: None,
            read_flag: flag,
            write_flag: !flag,
        };
        assert!(Bundle::semantic_shape_is_complete(&complete_bundle));
        assert!(!Bundle::semantic_shape_is_complete(&incomplete_bundle));
        assert_component_score_state(&complete_bundle, &BUNDLE_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_bundle, &BUNDLE_RHO_MATCH_PROGRAM, false);

        let complete_if = If {
            condition: Some(child.clone()),
            if_true: Some(other.clone()),
            if_false: Some(child.clone()),
            connective_used: flag,
            ..If::default()
        };
        let incomplete_if = If {
            if_false: Some(child.clone()),
            connective_used: flag,
            ..If::default()
        };
        assert!(If::semantic_shape_is_complete(&complete_if));
        assert!(!If::semantic_shape_is_complete(&incomplete_if));
        assert_component_score_state(&complete_if, &IF_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_if, &IF_RHO_MATCH_PROGRAM, false);

        let complete_expr = Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(first))),
        };
        let incomplete_expr = Expr {
            expr_instance: Some(ExprInstance::ENegBody(crate::rhoapi::ENeg { p: None })),
        };
        assert!(Expr::semantic_shape_is_complete(&complete_expr));
        assert!(!Expr::semantic_shape_is_complete(&incomplete_expr));
        assert_component_score_state(&complete_expr, &EXPR_RHO_MATCH_PROGRAM, true);
        assert_component_score_state(&incomplete_expr, &EXPR_RHO_MATCH_PROGRAM, false);
    }

    #[test]
    fn nested_bundle_scores_override_protobuf_body_order(
        outer_read in any::<bool>(),
        outer_write in any::<bool>(),
        inner_read in any::<bool>(),
        inner_write in any::<bool>(),
    ) {
        let send_body = Par::default().with_sends(vec![Send::default()]);
        let nested_body = Par::default().with_bundles(vec![Bundle {
            body: Some(Par::default()),
            read_flag: inner_read,
            write_flag: inner_write,
        }]);
        let send_bundle = Bundle {
            body: Some(send_body.clone()),
            read_flag: outer_read,
            write_flag: outer_write,
        };
        let nested_bundle = Bundle {
            body: Some(nested_body.clone()),
            read_flag: outer_read,
            write_flag: outer_write,
        };
        let send_bytes = protobuf_encoder::encode_to_vec(&send_bundle);
        let nested_bytes = protobuf_encoder::encode_to_vec(&nested_bundle);
        assert!(send_bytes < nested_bytes);

        let constructor = semantics::bundle_constructor(outer_read, outer_write);
        let send_score = authoritative_constructor(
            constructor,
            vec![authoritative_par_score(&send_body)],
        );
        let nested_score = authoritative_constructor(
            constructor,
            vec![authoritative_par_score(&nested_body)],
        );
        assert!(send_score > nested_score);
        property_component_source(&send_bundle, &BUNDLE_RHO_MATCH_PROGRAM, &send_score);
        property_component_source(&nested_bundle, &BUNDLE_RHO_MATCH_PROGRAM, &nested_score);

        let mut body_ordered = Vec::new();
        for body in [&send_bytes, &nested_bytes] {
            assert!(body.len() < 128);
            body_ordered.extend([90, body.len() as u8]);
            body_ordered.extend(body);
        }
        property_rejected_without_root(&body_ordered, &PAR_PROTOBUF_REGION_PROGRAM);

        let score_ordered = Par::default().with_bundles(vec![nested_bundle, send_bundle]);
        let score_ordered_bytes = protobuf_encoder::encode_to_vec(&score_ordered);
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&score_ordered_bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        ).expect("score-ordered nested bundles admit");
        let expected_score = authoritative_par_score(&score_ordered);
        assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
    }

    #[test]
    fn complete_bundle_flags_order_by_score_before_body(
        value in any::<i16>(),
        connective_used in any::<bool>(),
    ) {
        let body = gint(i64::from(value)).with_connective_used(connective_used);
        let read = Bundle {
            body: Some(body.clone()),
            read_flag: true,
            write_flag: false,
        };
        let write = Bundle {
            body: Some(body.clone()),
            read_flag: false,
            write_flag: true,
        };
        let body_score = authoritative_par_score(&body);
        let read_score = authoritative_constructor(306, vec![body_score.clone()]);
        let write_score = authoritative_constructor(307, vec![body_score]);
        assert!(read_score < write_score);
        property_component_source(&read, &BUNDLE_RHO_MATCH_PROGRAM, &read_score);
        property_component_source(&write, &BUNDLE_RHO_MATCH_PROGRAM, &write_score);

        let write_bytes = protobuf_encoder::encode_to_vec(&write);
        let read_bytes = protobuf_encoder::encode_to_vec(&read);
        assert!(write_bytes < read_bytes);
        let mut body_ordered = Vec::new();
        for body in [&write_bytes, &read_bytes] {
            assert!(body.len() < 128);
            body_ordered.extend([90, body.len() as u8]);
            body_ordered.extend(body);
        }
        property_rejected_without_root(&body_ordered, &PAR_PROTOBUF_REGION_PROGRAM);

        let score_ordered = Par::default().with_bundles(vec![read, write]);
        let score_ordered_bytes = protobuf_encoder::encode_to_vec(&score_ordered);
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&score_ordered_bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        ).expect("score-ordered flag-distinct bundles admit");
        let expected_score = authoritative_par_score(&score_ordered);
        assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
    }

    #[test]
    fn raw_par_source_branch_preserves_occurrences_and_semantic_flag(
        value in any::<i16>(),
        occurrences in 0usize..4,
        connective_used in any::<bool>(),
        locally_free in prop::option::of(1u8..=255),
    ) {
        let expr = Expr { expr_instance: Some(ExprInstance::GInt(i64::from(value))) };
        let locally_free = locally_free.into_iter().collect::<Vec<_>>();
        let par = Par::default()
            .with_exprs(vec![expr.clone(); occurrences])
            .with_locally_free(locally_free.clone())
            .with_connective_used(connective_used);
        let bytes = protobuf_encoder::encode_to_vec(&par);
        assert_eq!(Par::decode(bytes.as_slice()).unwrap().encode_to_vec(), bytes);
        rpb3_root_finish_trace::start();
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        ).expect("generated raw Par admits");
        let trace = rpb3_root_finish_trace::take();
        let entries = trace.score_arena.as_ref().expect("raw Par source");
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let returned = trace.returned.as_ref().expect("raw Par return");
        assert_eq!(returned.variant, rpb3_root_finish_trace::RootSemanticVariant::Par);
        rpb3_root_finish_trace::assert_summary_score_source(returned, &transcripts, "generated raw Par");
        let root = returned.score_handle.expect("raw Par score").index();
        assert_eq!(root + 1, entries.len());
        assert_eq!(entries[root - 2].kind, CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
        assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(999));
        let children: Vec<_> = trace.children.iter()
            .filter(|child| std::ptr::eq(child.program, &EXPR_RHO_MATCH_PROGRAM))
            .collect();
        assert_eq!(children.len(), occurrences);
        let mut expected_handles = vec![root - 1];
        for child in children {
            assert_eq!(child.body, protobuf_encoder::encode_to_vec(&expr));
            let child_handle = child.summary.score_handle.expect("Expr child score").index();
            assert_eq!(transcripts[child_handle], property_gint_score(i64::from(value)));
            expected_handles.push(child_handle);
        }
        expected_handles.push(root - 2);
        assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(expected_handles));
        let expected_score = authoritative_constructor(
            999,
            std::iter::repeat_with(|| property_gint_score(i64::from(value)))
                .take(occurrences)
                .chain(std::iter::once(authoritative_integer_atom(i64::from(connective_used))))
                .collect(),
        );
        assert_eq!(transcripts[root], expected_score);
        assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
        assert_eq!(admission.semantic_default_par(), Some(occurrences == 0 && !connective_used));
        assert_eq!(
            admission.canonical_path_stable_par(),
            Some(occurrences == 1 && locally_free.is_empty() && !connective_used),
        );
        assert_eq!(trace.remaining_frames, Some(0));
    }

    #[test]
    fn connective_source_branches_select_exact_nested_scores(
        first in any::<i16>(),
        second in any::<i16>(),
        flag in any::<bool>(),
        index in any::<i16>(),
        depth in any::<i16>(),
    ) {
        let absent = Connective::default();
        let trace = property_component_source(
            &absent,
            &CONNECTIVE_RHO_MATCH_PROGRAM,
            &authoritative_integer_atom(0),
        );
        assert_eq!(trace.score_arena.as_ref().unwrap().len(), 1);
        assert_eq!(trace.score_arena.as_ref().unwrap()[0].kind, CanonicalScoreSourceKind::Integer(0));

        let scalar_arms = [
            (ConnectiveInstance::ConnBool(flag), 404),
            (ConnectiveInstance::ConnInt(flag), 405),
            (ConnectiveInstance::ConnString(flag), 406),
            (ConnectiveInstance::ConnUri(flag), 407),
            (ConnectiveInstance::ConnByteArray(flag), 408),
        ];
        for (arm, constructor) in scalar_arms {
            let connective = Connective { connective_instance: Some(arm) };
            let trace = property_component_source(
                &connective,
                &CONNECTIVE_RHO_MATCH_PROGRAM,
                &authoritative_constructor(constructor, vec![authoritative_integer_atom(i64::from(flag))]),
            );
            let entries = trace.score_arena.as_ref().unwrap();
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[0].kind, CanonicalScoreSourceKind::Integer(i64::from(flag)));
            assert_eq!(entries[1].kind, CanonicalScoreSourceKind::Integer(i64::from(constructor)));
            assert_eq!(entries[2].kind, CanonicalScoreSourceKind::Node(vec![1, 0]));
        }

        let var_ref = VarRef { index: i32::from(index), depth: i32::from(depth) };
        let connective = Connective {
            connective_instance: Some(ConnectiveInstance::VarRefBody(var_ref.clone())),
        };
        let trace = property_component_source(
            &connective,
            &CONNECTIVE_RHO_MATCH_PROGRAM,
            &authoritative_constructor(403, vec![
                authoritative_integer_atom(i64::from(index)),
                authoritative_integer_atom(i64::from(depth)),
            ]),
        );
        let entries = trace.score_arena.as_ref().unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, CanonicalScoreSourceKind::Integer(i64::from(index)));
        assert_eq!(entries[1].kind, CanonicalScoreSourceKind::Integer(i64::from(depth)));
        assert_eq!(entries[2].kind, CanonicalScoreSourceKind::Integer(403));
        assert_eq!(entries[3].kind, CanonicalScoreSourceKind::Node(vec![2, 0, 1]));
        let no_score_child = trace.children.iter().filter(|child| std::ptr::eq(child.program, &VARREF_RHO_MATCH_PROGRAM)).collect::<Vec<_>>();
        assert_eq!(no_score_child.len(), 1);
        assert!(no_score_child[0].summary.score_handle.is_none());
        assert_eq!(no_score_child[0].body, protobuf_encoder::encode_to_vec(&var_ref));

        let first_par = gint(i64::from(first));
        let second_par = gint(i64::from(second));
        let not = Connective { connective_instance: Some(ConnectiveInstance::ConnNotBody(first_par.clone())) };
        let trace = property_component_source(
            &not,
            &CONNECTIVE_RHO_MATCH_PROGRAM,
            &authoritative_constructor(400, vec![property_par_gint_score(i64::from(first))]),
        );
        let par_children = trace.children.iter().filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM)).collect::<Vec<_>>();
        assert_eq!(par_children.len(), 1);
        let entries = trace.score_arena.as_ref().unwrap();
        let root = entries.len() - 1;
        assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(400));
        assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(vec![root - 1, par_children[0].summary.score_handle.unwrap().index()]));

        let body = ConnectiveBody { ps: vec![first_par.clone(), second_par.clone()] };
        for (arm, constructor) in [
            (ConnectiveInstance::ConnAndBody(body.clone()), 401),
            (ConnectiveInstance::ConnOrBody(body.clone()), 402),
        ] {
            let connective = Connective { connective_instance: Some(arm) };
            let trace = property_component_source(
                &connective,
                &CONNECTIVE_RHO_MATCH_PROGRAM,
                &authoritative_constructor(constructor, vec![
                    property_par_gint_score(i64::from(first)),
                    property_par_gint_score(i64::from(second)),
                ]),
            );
            let no_score_body = trace.children.iter().filter(|child| std::ptr::eq(child.program, &CONNECTIVEBODY_RHO_MATCH_PROGRAM)).collect::<Vec<_>>();
            assert_eq!(no_score_body.len(), 1);
            assert!(no_score_body[0].summary.score_handle.is_none());
            assert_eq!(no_score_body[0].body, protobuf_encoder::encode_to_vec(&body));
            let par_children = trace.children.iter().filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM)).collect::<Vec<_>>();
            assert_eq!(par_children.len(), 2);
            let entries = trace.score_arena.as_ref().unwrap();
            let root = entries.len() - 1;
            assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(i64::from(constructor)));
            assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(vec![
                root - 1,
                par_children[0].summary.score_handle.unwrap().index(),
                par_children[1].summary.score_handle.unwrap().index(),
            ]));
        }
    }

    #[test]
    fn unforgeable_source_branches_bind_bytes_and_constructor(
        bytes in prop::collection::vec(any::<u8>(), 1..9),
    ) {
        let bytes = std::iter::once(0).chain(bytes).collect::<Vec<_>>();
        let cases = [
            (GUnforgeable::default(), 0, None),
            (GUnforgeable { unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: Vec::new() })) }, 5, Some(Vec::new())),
            (GUnforgeable { unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: bytes.clone() })) }, 5, Some(bytes.clone())),
            (GUnforgeable { unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: bytes.clone() })) }, 11, Some(bytes.clone())),
            (GUnforgeable { unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId { public_key: bytes.clone() })) }, 10, Some(bytes.clone())),
            (GUnforgeable { unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})) }, 12, None),
        ];
        for (value, constructor, payload) in cases {
            let expected = authoritative_constructor(
                constructor,
                payload.as_ref().map(|bytes| authoritative_bytes_atom(bytes)).into_iter().collect(),
            );
            let trace = property_component_source(&value, &GUNFORGEABLE_RHO_MATCH_PROGRAM, &expected);
            let entries = trace.score_arena.as_ref().unwrap();
            if let Some(bytes) = payload {
                assert_eq!(entries.len(), 3);
                assert_eq!(entries[0].kind, CanonicalScoreSourceKind::BytesIndexed(bytes));
                assert_eq!(entries[1].kind, CanonicalScoreSourceKind::Integer(i64::from(constructor)));
                assert_eq!(entries[2].kind, CanonicalScoreSourceKind::Node(vec![1, 0]));
            } else {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].kind, CanonicalScoreSourceKind::Integer(i64::from(constructor)));
                assert_eq!(entries[1].kind, CanonicalScoreSourceKind::Node(vec![0]));
            }
        }
    }

    #[test]
    fn generated_wire_rejects_ambiguous_oneof_and_descending_par_occurrences(
        seed in any::<u8>(),
    ) {
        let bool_arm = Connective { connective_instance: Some(ConnectiveInstance::ConnBool(true)) };
        let int_arm = Connective { connective_instance: Some(ConnectiveInstance::ConnInt(false)) };
        let mut duplicate_connective = protobuf_encoder::encode_to_vec(&bool_arm);
        duplicate_connective.extend(protobuf_encoder::encode_to_vec(&int_arm));
        property_rejected_without_root(&duplicate_connective, &CONNECTIVE_RHO_MATCH_PROGRAM);

        let private = GUnforgeable { unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![seed] })) };
        let deploy = GUnforgeable { unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![seed] })) };
        let mut duplicate_unforgeable = protobuf_encoder::encode_to_vec(&private);
        duplicate_unforgeable.extend(protobuf_encoder::encode_to_vec(&deploy));
        property_rejected_without_root(&duplicate_unforgeable, &GUNFORGEABLE_RHO_MATCH_PROGRAM);

        let high = i64::from(seed) + 1;
        let low = high - 1;
        let mut descending = Vec::new();
        for value in [high, low] {
            let body = protobuf_encoder::encode_to_vec(&Expr { expr_instance: Some(ExprInstance::GInt(value)) });
            assert!(body.len() < 128);
            descending.extend([0x2a, body.len() as u8]);
            descending.extend(body);
        }
        property_rejected_without_root(&descending, &PAR_PROTOBUF_REGION_PROGRAM);
    }

    #[test]
    fn ordered_sequence_source_branch_preserves_remainder_and_order(
        first in any::<i16>(),
        second in any::<i16>(),
        level in any::<i16>(),
        var_kind in any::<u8>(),
        connective_used in any::<bool>(),
    ) {
        let pars = [gint(i64::from(first)), gint(i64::from(second))];
        let par_scores = [property_par_gint_score(i64::from(first)), property_par_gint_score(i64::from(second))];
        let flag = authoritative_integer_atom(i64::from(connective_used));
        for remainder in [None, Some(property_var(level, var_kind))] {
            let list = crate::rhoapi::EList {
                ps: pars.to_vec(),
                remainder: remainder.clone(),
                connective_used,
                ..Default::default()
            };
            let bytes = protobuf_encoder::encode_to_vec(&list);
            let (trace, transcripts) = checked_aux_finish(
                &bytes,
                &crate::rust::rholang::match_schema_tables::ELIST_RHO_MATCH_PROGRAM,
            );
            let entries = trace.score_arena.as_ref().unwrap();
            let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
            let par_children = trace.children.iter()
                .filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM))
                .collect::<Vec<_>>();
            assert_eq!(par_children.len(), pars.len());
            let mut element_handles = Vec::new();
            for (index, child) in par_children.iter().enumerate() {
                assert_eq!(child.body, protobuf_encoder::encode_to_vec(&pars[index]));
                let handle = child.summary.score_handle.unwrap().index();
                assert_eq!(transcripts[handle], par_scores[index]);
                element_handles.push(handle);
            }
            assert_eq!(entries[root - 3].kind, CanonicalScoreSourceKind::Integer(-1));
            assert_eq!(entries[root - 2].kind, CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
            assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::ELIST)));
            let (remainder_handle, remainder_score) = match remainder.as_ref() {
                None => (root - 3, authoritative_integer_atom(-1)),
                Some(value) => {
                    let children = trace.children.iter()
                        .filter(|child| std::ptr::eq(child.program, &VAR_RHO_MATCH_PROGRAM))
                        .collect::<Vec<_>>();
                    assert_eq!(children.len(), 1);
                    assert_eq!(children[0].body, protobuf_encoder::encode_to_vec(value));
                    let handle = children[0].summary.score_handle.unwrap().index();
                    let score = authoritative_var_score(value);
                    assert_eq!(transcripts[handle], score);
                    (handle, score)
                }
            };
            assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(vec![
                root - 1, remainder_handle, element_handles[0], element_handles[1], root - 2,
            ]));
            assert_eq!(transcripts[root], authoritative_constructor(
                Score::ELIST,
                vec![remainder_score, par_scores[0].clone(), par_scores[1].clone(), flag.clone()],
            ));
        }

        let tuple = crate::rhoapi::ETuple {
            ps: pars.to_vec(),
            connective_used,
            ..Default::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&tuple);
        let (trace, transcripts) = checked_aux_finish(
            &bytes,
            &crate::rust::rholang::match_schema_tables::ETUPLE_RHO_MATCH_PROGRAM,
        );
        let entries = trace.score_arena.as_ref().unwrap();
        let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
        let par_children = trace.children.iter()
            .filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM))
            .collect::<Vec<_>>();
        assert_eq!(par_children.len(), pars.len());
        let element_handles = par_children.iter().enumerate().map(|(index, child)| {
            assert_eq!(child.body, protobuf_encoder::encode_to_vec(&pars[index]));
            let handle = child.summary.score_handle.unwrap().index();
            assert_eq!(transcripts[handle], par_scores[index]);
            handle
        }).collect::<Vec<_>>();
        assert_eq!(entries[root - 2].kind, CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
        assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::ETUPLE)));
        assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(vec![
            root - 1, element_handles[0], element_handles[1], root - 2,
        ]));
        assert_eq!(transcripts[root], authoritative_constructor(
            Score::ETUPLE,
            vec![par_scores[0].clone(), par_scores[1].clone(), flag],
        ));
    }

    #[test]
    fn canonical_set_source_branch_preserves_order_identity_and_score(
        first in any::<i16>(),
        second in any::<i16>(),
        level in any::<i16>(),
        var_kind in any::<u8>(),
        connective_used in any::<bool>(),
    ) {
        let first = i64::from(first);
        let second = if i64::from(second) == first {
            first + 1
        } else {
            i64::from(second)
        };
        let mut elements = [first, second]
            .into_iter()
            .map(|value| (gint(value), property_par_gint_score(value)))
            .collect::<Vec<_>>();
        elements.sort_by(|(left_par, left_score), (right_par, right_score)| {
            left_score.cmp(right_score).then_with(|| {
                protobuf_encoder::encode_to_vec(left_par)
                    .cmp(&protobuf_encoder::encode_to_vec(right_par))
            })
        });
        assert!(elements[0].1 < elements[1].1);

        for count in 0..=elements.len() {
            let pars = elements[..count]
                .iter()
                .map(|(par, _)| par.clone())
                .collect::<Vec<_>>();
            let par_scores = elements[..count]
                .iter()
                .map(|(_, score)| score.clone())
                .collect::<Vec<_>>();
            for remainder in [None, Some(property_var(level, var_kind))] {
                let set = crate::rhoapi::ESet {
                    ps: pars.clone(),
                    remainder: remainder.clone(),
                    connective_used,
                    ..Default::default()
                };
                let bytes = protobuf_encoder::encode_to_vec(&set);
                let (trace, transcripts) = checked_aux_finish(
                    &bytes,
                    &crate::rust::rholang::match_schema_tables::ESET_RHO_MATCH_PROGRAM,
                );
                let entries = trace.score_arena.as_ref().unwrap();
                let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
                let par_children = trace.children.iter()
                    .filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM))
                    .collect::<Vec<_>>();
                assert_eq!(par_children.len(), count);
                let mut element_handles = Vec::new();
                for (index, child) in par_children.iter().enumerate() {
                    assert_eq!(child.body, protobuf_encoder::encode_to_vec(&pars[index]));
                    let handle = child.summary.score_handle.unwrap().index();
                    assert_eq!(transcripts[handle], par_scores[index]);
                    element_handles.push(handle);
                }
                let (remainder_handle, remainder_score) = match remainder.as_ref() {
                    None => (root - 3, authoritative_integer_atom(-1)),
                    Some(value) => {
                        let children = trace.children.iter()
                            .filter(|child| std::ptr::eq(child.program, &VAR_RHO_MATCH_PROGRAM))
                            .collect::<Vec<_>>();
                        assert_eq!(children.len(), 1);
                        assert_eq!(children[0].body, protobuf_encoder::encode_to_vec(value));
                        let handle = children[0].summary.score_handle.unwrap().index();
                        let score = authoritative_var_score(value);
                        assert_eq!(transcripts[handle], score);
                        (handle, score)
                    }
                };
                assert_eq!(entries[root - 3].kind, CanonicalScoreSourceKind::Integer(-1));
                assert_eq!(entries[root - 2].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
                assert_eq!(entries[root - 1].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::ESET)));
                let mut node_children = vec![root - 1, remainder_handle];
                node_children.extend(element_handles);
                node_children.push(root - 2);
                assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(node_children));
                let mut score_children = vec![remainder_score];
                score_children.extend(par_scores.iter().cloned());
                score_children.push(authoritative_integer_atom(i64::from(connective_used)));
                let expected = authoritative_constructor(Score::ESET, score_children);
                assert_eq!(transcripts[root], expected);
                let (score, fixed, stable, snapshot) = property_derived_expr_payload(
                    &bytes,
                    &crate::rust::rholang::match_schema_tables::ESET_RHO_MATCH_PROGRAM,
                );
                assert_eq!(score, Some(expected.clone()));
                assert!(fixed);
                assert!(!stable);
                assert!(snapshot.is_none());

                let expr = Expr { expr_instance: Some(ExprInstance::ESetBody(set)) };
                let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
                let (expr_trace, expr_transcripts) = checked_property_finish(
                    &expr, &expr_bytes, &EXPR_RHO_MATCH_PROGRAM, true,
                );
                let expr_root = expr_trace.returned.as_ref().unwrap().score_handle.unwrap().index();
                assert_eq!(expr_transcripts[expr_root], expected);
            }
        }

        let reversed = crate::rhoapi::ESet {
            ps: elements.iter().rev().map(|(par, _)| par.clone()).collect(),
            ..Default::default()
        };
        let reversed_bytes = protobuf_encoder::encode_to_vec(&reversed);
        let (_, reversed_fixed, _, _) = property_derived_expr_payload(
            &reversed_bytes,
            &crate::rust::rholang::match_schema_tables::ESET_RHO_MATCH_PROGRAM,
        );
        assert!(!reversed_fixed);
        property_rejected_without_root(
            &protobuf_encoder::encode_to_vec(&Expr {
                expr_instance: Some(ExprInstance::ESetBody(reversed)),
            }),
            &EXPR_RHO_MATCH_PROGRAM,
        );

        let mut duplicate_identity = [
            gint(first).with_locally_free(vec![1]),
            gint(first).with_locally_free(vec![2]),
        ];
        duplicate_identity.sort_by_key(protobuf_encoder::encode_to_vec);
        assert_ne!(
            protobuf_encoder::encode_to_vec(&duplicate_identity[0]),
            protobuf_encoder::encode_to_vec(&duplicate_identity[1]),
        );
        assert_eq!(
            authoritative_par_score(&duplicate_identity[0]),
            authoritative_par_score(&duplicate_identity[1]),
        );
        let duplicate_set = crate::rhoapi::ESet {
            ps: duplicate_identity.to_vec(),
            ..Default::default()
        };
        let (_, duplicate_fixed, _, _) = property_derived_expr_payload(
            &protobuf_encoder::encode_to_vec(&duplicate_set),
            &crate::rust::rholang::match_schema_tables::ESET_RHO_MATCH_PROGRAM,
        );
        assert!(!duplicate_fixed);
        property_rejected_without_root(
            &protobuf_encoder::encode_to_vec(&Expr {
                expr_instance: Some(ExprInstance::ESetBody(duplicate_set)),
            }),
            &EXPR_RHO_MATCH_PROGRAM,
        );
    }

    #[test]
    fn complete_emethod_source_branch_preserves_target_arguments_and_name(
        target in any::<i16>(),
        first in any::<i16>(),
        second in any::<i16>(),
        argument_count in 0usize..3,
        connective_used in any::<bool>(),
    ) {
        let target_par = gint(i64::from(target));
        let arguments = [gint(i64::from(first)), gint(i64::from(second))]
            .into_iter().take(argument_count).collect::<Vec<_>>();
        let method_name = format!("m\0{target}");
        let method = crate::rhoapi::EMethod {
            method_name: method_name.clone(),
            target: Some(target_par.clone()),
            arguments: arguments.clone(),
            connective_used,
            locally_free: vec![1],
        };
        let mut score_children = vec![
            authoritative_string_atom(&method_name),
            property_par_gint_score(i64::from(target)),
        ];
        score_children.extend([first, second].into_iter().take(argument_count)
            .map(|value| property_par_gint_score(i64::from(value))));
        score_children.push(authoritative_integer_atom(i64::from(connective_used)));
        let expected = authoritative_constructor(Score::EMETHOD, score_children);
        let bytes = protobuf_encoder::encode_to_vec(&method);
        let (trace, transcripts) = checked_aux_finish(
            &bytes,
            &crate::rust::rholang::match_schema_tables::EMETHOD_RHO_MATCH_PROGRAM,
        );
        let entries = trace.score_arena.as_ref().unwrap();
        let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
        let par_children = trace.children.iter()
            .filter(|child| std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM))
            .collect::<Vec<_>>();
        assert_eq!(par_children.len(), argument_count + 1);
        let mut par_handles = Vec::new();
        for (index, child) in par_children.iter().enumerate() {
            let value = if index == 0 { &target_par } else { &arguments[index - 1] };
            assert_eq!(child.body, protobuf_encoder::encode_to_vec(value));
            let handle = child.summary.score_handle.unwrap().index();
            assert_eq!(transcripts[handle], property_par_gint_score(
                if index == 0 { i64::from(target) } else { i64::from([first, second][index - 1]) }
            ));
            par_handles.push(handle);
        }
        assert_eq!(entries[root - 3].kind, CanonicalScoreSourceKind::StringIndexed(method_name.as_bytes().to_vec()));
        assert_eq!(entries[root - 2].kind, CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
        assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::EMETHOD)));
        let mut node_children = vec![root - 1, root - 3];
        node_children.extend(par_handles);
        node_children.push(root - 2);
        assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(node_children));
        assert_eq!(transcripts[root], expected);

        let expr = Expr { expr_instance: Some(ExprInstance::EMethodBody(method)) };
        let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
        let (expr_trace, expr_transcripts) = checked_property_finish(
            &expr, &expr_bytes, &EXPR_RHO_MATCH_PROGRAM, true,
        );
        let expr_root = expr_trace.returned.as_ref().unwrap().score_handle.unwrap().index();
        assert_eq!(expr_transcripts[expr_root], expected);
        let method_children = expr_trace.children.iter()
            .filter(|child| std::ptr::eq(child.program, &crate::rust::rholang::match_schema_tables::EMETHOD_RHO_MATCH_PROGRAM))
            .collect::<Vec<_>>();
        assert_eq!(method_children.len(), 1);
        assert_eq!(method_children[0].summary.score_bytes.as_deref(), Some(expected.as_slice()));
    }

    #[test]
    fn complete_ezipper_source_reuses_authoritative_snapshot(
        key in any::<i16>(),
        value in any::<i16>(),
        mode in 0u8..3,
        connective_used in any::<bool>(),
    ) {
        let map = match mode {
            0 => EPathMap::default(),
            1 => EPathMap::from_set_iter(
                [gint(i64::from(key))], Vec::new(), false, None,
            ),
            _ => EPathMap::new_map(
                [(gint(i64::from(key)), gint(i64::from(value)))],
                Vec::new(), false, None,
            ),
        };
        let map_bytes = protobuf_encoder::encode_to_vec(&map);
        let snapshot = independent_epathmap_snapshot_field(&map_bytes);
        assert_eq!(snapshot, map.trie_snapshot());
        let (derived_map_score, map_fixed, map_stable, derived_snapshot) =
            property_derived_expr_payload(&map_bytes, &EPATHMAP_RHO_MATCH_PROGRAM);
        assert!(map_fixed);
        assert!(map_stable);
        assert_eq!(derived_snapshot, Some(snapshot.clone()));
        let expected = authoritative_constructor(
            Score::EPATHMAP + 1,
            vec![
                authoritative_bytes_atom(&snapshot),
                authoritative_integer_atom(i64::from(connective_used)),
            ],
        );
        for (path_variation, cursor_kind) in [
            (false, 0u32),
            (false, 1),
            (true, 0),
            (true, 1),
        ] {
            let zipper = crate::rhoapi::EZipper {
                pathmap: Some(map.clone()),
                current_path: if path_variation {
                    vec![protobuf_encoder::encode_to_vec(&gint(i64::from(key)))]
                } else {
                    Vec::new()
                },
                is_write_zipper: path_variation,
                locally_free: if path_variation { vec![1] } else { Vec::new() },
                connective_used,
                cursor_kind,
            };
            let bytes = protobuf_encoder::encode_to_vec(&zipper);
            let (trace, transcripts) = checked_aux_finish(
                &bytes,
                &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM,
            );
            assert_eq!(trace.epathmap_accepts.len(), 1);
            assert_eq!(trace.epathmap_accepts[0].returned_snapshot, snapshot);
            assert_eq!(trace.epathmap_accepts[0].stored_snapshot, snapshot);
            assert_eq!(trace.epathmap_scores.len(), 1);
            assert_eq!(trace.epathmap_scores[0].snapshot, snapshot);
            let snapshot_pops = trace.pop_accepts.iter()
                .filter(|pop| pop.child_kind == FrameRequestKind::EPathMap)
                .collect::<Vec<_>>();
            assert_eq!(snapshot_pops.len(), 1);
            assert_eq!(snapshot_pops[0].after_len, Some(snapshot_pops[0].before_len - 1));
            assert_eq!(snapshot_pops[0].epathmap_child_snapshot.as_deref(), Some(snapshot.as_slice()));
            assert_eq!(snapshot_pops[0].epathmap_returned_snapshot.as_deref(), Some(snapshot.as_slice()));
            let child_scores = trace.children.iter()
                .filter(|child| std::ptr::eq(child.program, &EPATHMAP_RHO_MATCH_PROGRAM))
                .collect::<Vec<_>>();
            assert_eq!(child_scores.len(), 1);
            assert_eq!(child_scores[0].body, map_bytes);
            let pathmap_pops = trace.pop_accepts.iter()
                .filter(|pop| pop.child_program.is_some_and(|program|
                    std::ptr::eq(program, &EPATHMAP_RHO_MATCH_PROGRAM)))
                .collect::<Vec<_>>();
            assert_eq!(pathmap_pops.len(), 1);
            let pathmap_pop = pathmap_pops[0];
            assert_eq!(pathmap_pop.after_len, Some(pathmap_pop.before_len - 1));
            assert_eq!(pathmap_pop.summary.as_ref().unwrap().variant,
                rpb3_root_finish_trace::RootSemanticVariant::ExprPayload);
            let map_score = authoritative_constructor(
                Score::EPATHMAP,
                vec![authoritative_bytes_atom(&snapshot), authoritative_integer_atom(0)],
            );
            assert_eq!(derived_map_score.as_deref(), Some(map_score.as_slice()));
            assert_eq!(child_scores[0].summary.score_bytes.as_deref(), Some(map_score.as_slice()));
            let entries = trace.score_arena.as_ref().unwrap();
            let map_source = &trace.epathmap_scores[0];
            let map_before = map_source.source_len_before;
            assert_eq!(map_source.source_len_after, map_before + 4);
            assert_eq!(map_source.snapshot_handle.index(), map_before);
            assert_eq!(map_source.score_handle.index(), map_before + 3);
            assert_eq!(entries[map_before].kind, CanonicalScoreSourceKind::BytesIndexed(snapshot.clone()));
            assert_eq!(entries[map_before + 1].kind, CanonicalScoreSourceKind::Integer(0));
            assert_eq!(entries[map_before + 2].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::EPATHMAP)));
            assert_eq!(entries[map_before + 3].kind, CanonicalScoreSourceKind::Node(vec![
                map_before + 2, map_before, map_before + 1,
            ]));
            assert_eq!(child_scores[0].summary.score_handle, Some(map_source.score_handle));
            assert_eq!(pathmap_pop.source_len_before, map_source.source_len_after);
            assert_eq!(pathmap_pop.source_len_after, Some(map_source.source_len_after));
            let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
            assert_eq!(root - 3, map_source.source_len_after);
            assert_eq!(entries[root - 3].kind, CanonicalScoreSourceKind::BytesIndexed(snapshot.clone()));
            assert_eq!(entries[root - 2].kind, CanonicalScoreSourceKind::Integer(i64::from(connective_used)));
            assert_eq!(entries[root - 1].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::EPATHMAP + 1)));
            assert_eq!(entries[root].kind, CanonicalScoreSourceKind::Node(vec![root - 1, root - 3, root - 2]));
            assert_eq!(transcripts[map_source.score_handle.index()], map_score);
            assert_eq!(transcripts[root], expected);
            let (derived_zipper_score, zipper_fixed, zipper_stable, zipper_snapshot) =
                property_derived_expr_payload(&bytes, &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM);
            assert_eq!(derived_zipper_score.as_deref(), Some(expected.as_slice()));
            assert!(zipper_fixed);
            assert!(!zipper_stable);
            assert!(zipper_snapshot.is_none());

            let expr = Expr { expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())) };
            let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
            let (expr_trace, expr_transcripts) = checked_property_finish(
                &expr, &expr_bytes, &EXPR_RHO_MATCH_PROGRAM, true,
            );
            let expr_root = expr_trace.returned.as_ref().unwrap().score_handle.unwrap().index();
            assert_eq!(expr_transcripts[expr_root], expected);
            let outer_expr_finishes = expr_trace.expr_finishes.iter()
                .filter(|finish| finish.body == expr_bytes && finish.score_handle.index() == expr_root)
                .collect::<Vec<_>>();
            assert_eq!(outer_expr_finishes.len(), 1);
            assert!(outer_expr_finishes[0].complete);
            assert!(!outer_expr_finishes[0].canonical_path_stable);
            let zipper_pops = expr_trace.pop_accepts.iter()
                .filter(|pop| pop.child_program.is_some_and(|program|
                    std::ptr::eq(program,
                        &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM)))
                .collect::<Vec<_>>();
            assert_eq!(zipper_pops.len(), 1);
            assert_eq!(zipper_pops[0].after_len, Some(zipper_pops[0].before_len - 1));
            assert_eq!(zipper_pops[0].summary.as_ref().unwrap().variant,
                rpb3_root_finish_trace::RootSemanticVariant::ExprPayload);
            assert_eq!(zipper_pops[0].summary.as_ref().unwrap().score_bytes.as_deref(),
                Some(expected.as_slice()));
            assert_eq!(zipper_pops[0].summary.as_ref().unwrap().score_handle.map(CanonicalScoreHandle::index),
                Some(expr_root));
            let snapshot_offsets = expr_bytes.windows(snapshot.len())
                .enumerate()
                .filter_map(|(offset, bytes)| (bytes == snapshot.as_slice()).then_some(offset))
                .collect::<Vec<_>>();
            assert_eq!(snapshot_offsets.len(), 1);
            let mut malformed = expr_bytes.clone();
            malformed[snapshot_offsets[0]] ^= 1;
            rpb3_root_finish_trace::start();
            assert!(validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&malformed), &EXPR_RHO_MATCH_PROGRAM,
            ).is_err());
            let refused = rpb3_root_finish_trace::take();
            assert!(refused.returned.is_none());
            assert!(refused.epathmap_accepts.is_empty());
            assert!(refused.epathmap_scores.is_empty());

            let mut absent_zipper = zipper;
            absent_zipper.pathmap = None;
            let absent_bytes = protobuf_encoder::encode_to_vec(&absent_zipper);
            assert_eq!(
                property_derived_expr_payload(&absent_bytes,
                    &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM),
                (None, true, false, None),
            );
            let (absent_trace, _) = checked_aux_finish(
                &absent_bytes,
                &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM,
            );
            assert!(absent_trace.returned.as_ref().unwrap().score_handle.is_none());
            assert!(absent_trace.score_arena.as_ref().unwrap().is_empty());
            assert!(absent_trace.epathmap_accepts.is_empty());
            assert!(absent_trace.epathmap_scores.is_empty());
            let absent_payload_finishes = absent_trace.expr_payload_finishes.iter()
                .filter(|finish| finish.body == absent_bytes)
                .collect::<Vec<_>>();
            assert_eq!(absent_payload_finishes.len(), 1);
            assert!(absent_payload_finishes[0].score_handle.is_none());

            let absent_expr = Expr {
                expr_instance: Some(ExprInstance::EZipperBody(absent_zipper)),
            };
            let absent_expr_bytes = protobuf_encoder::encode_to_vec(&absent_expr);
            let (absent_expr_trace, _) = checked_property_finish(
                &absent_expr, &absent_expr_bytes, &EXPR_RHO_MATCH_PROGRAM, false,
            );
            let absent_zipper_pops = absent_expr_trace.pop_accepts.iter()
                .filter(|pop| pop.child_program.is_some_and(|program|
                    std::ptr::eq(program,
                        &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM)))
                .collect::<Vec<_>>();
            assert_eq!(absent_zipper_pops.len(), 1);
            assert_eq!(absent_zipper_pops[0].after_len,
                Some(absent_zipper_pops[0].before_len - 1));
            assert_eq!(absent_zipper_pops[0].summary.as_ref().unwrap().variant,
                rpb3_root_finish_trace::RootSemanticVariant::ExprPayload);
            assert!(absent_zipper_pops[0].summary.as_ref().unwrap().score_handle.is_none());
            let outer_absent_finishes = absent_expr_trace.expr_finishes.iter()
                .filter(|finish| finish.body == absent_expr_bytes)
                .collect::<Vec<_>>();
            assert_eq!(outer_absent_finishes.len(), 1);
            assert!(!outer_absent_finishes[0].complete);
            assert!(!outer_absent_finishes[0].canonical_path_stable);
        }
    }

    #[test]
    fn fragmented_ezipper_route_preserves_physical_source_anchors(
        key in any::<i16>(),
        value in any::<i16>(),
        mode in 0u8..3,
        connective_used in any::<bool>(),
        source_base in 100u32..300,
    ) {
        let map = match mode {
            0 => EPathMap::default(),
            1 => EPathMap::from_set_iter(
                [gint(i64::from(key))], Vec::new(), false, None,
            ),
            _ => EPathMap::new_map(
                [(gint(i64::from(key)), gint(i64::from(value)))],
                Vec::new(), false, None,
            ),
        };
        let zipper = crate::rhoapi::EZipper {
            pathmap: Some(map.clone()),
            connective_used,
            ..Default::default()
        };
        let expr = Expr { expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())) };
        let bytes = protobuf_encoder::encode_to_vec(&expr);
        let zipper_bytes = protobuf_encoder::encode_to_vec(&zipper);
        let map_bytes = protobuf_encoder::encode_to_vec(&map);
        let snapshot = independent_epathmap_snapshot_field(&map_bytes);
        let only_offset = |needle: &[u8]| {
            let offsets = bytes.windows(needle.len())
                .enumerate()
                .filter_map(|(index, window)| (window == needle).then_some(index))
                .collect::<Vec<_>>();
            assert_eq!(offsets.len(), 1);
            offsets[0]
        };
        let zipper_at = only_offset(&zipper_bytes);
        let map_at = only_offset(&map_bytes);
        let snapshot_at = only_offset(&snapshot);
        assert!(zipper_at < map_at && map_at < snapshot_at);
        let anchors = (0..bytes.len()).map(|index| {
            CanonicalByteProvenance::new(
                source_base + (index % 11) as u32,
                1000 + index * 23,
            )
        }).collect::<Vec<_>>();
        let terminal = CanonicalByteProvenance::new(900, 9000);
        let end_at = |end: usize| anchors.get(end).copied().unwrap_or(terminal);
        let ordinary = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes), &EXPR_RHO_MATCH_PROGRAM,
        ).expect("ordinary generated EZipper admits");
        rpb3_root_finish_trace::start();
        let located = validate_canonical_protobuf_inner(
            generated_located_admission_region(&bytes, &anchors, terminal),
            &EXPR_RHO_MATCH_PROGRAM, None,
        ).expect("fragmented generated EZipper admits");
        let trace = rpb3_root_finish_trace::take();
        assert_eq!(located, ordinary);
        rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
            &trace,
            &snapshot,
            &anchors[snapshot_at..snapshot_at + snapshot.len()],
            end_at(snapshot_at + snapshot.len()),
        );
        if mode != 0 {
            assert!(trace.epathmap_events.iter().any(|event| matches!(
                &event.kind,
                rpb3_root_finish_trace::EPathMapEventKind::Forward { .. }
                    | rpb3_root_finish_trace::EPathMapEventKind::Unwind { .. }
            )));
        }
        assert_eq!(trace.epathmap_accepts.len(), 1);
        let receipt = &trace.epathmap_accepts[0];
        assert_eq!(receipt.returned_snapshot, snapshot);
        assert_eq!(receipt.stored_snapshot, snapshot);
        assert_eq!(receipt.returned_start, anchors[snapshot_at]);
        assert_eq!(receipt.stored_start, anchors[snapshot_at]);
        let epm_pop = trace.pop_accepts.iter().find(|pop|
            pop.child_kind == FrameRequestKind::EPathMap
        ).expect("snapshot return");
        assert_eq!(epm_pop.epathmap_child_snapshot.as_deref(), Some(snapshot.as_slice()));
        assert_eq!(epm_pop.epathmap_returned_snapshot.as_deref(), Some(snapshot.as_slice()));
        assert_eq!(epm_pop.parent_before.as_ref().unwrap().program_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
        assert_eq!(epm_pop.parent_before.as_ref().unwrap().region_start, anchors[map_at]);
        assert_eq!(epm_pop.parent_after.as_ref().unwrap().region_start, anchors[map_at]);
        assert_eq!(epm_pop.parent_after.as_ref().unwrap().region_end, end_at(map_at + map_bytes.len()));
        assert_eq!(epm_pop.source_len_after, Some(epm_pop.source_len_before));

        let map_pop = trace.pop_accepts.iter().find(|pop|
            pop.child_program.is_some_and(|program| std::ptr::eq(program, &EPATHMAP_RHO_MATCH_PROGRAM))
        ).expect("PathMap message return");
        let map_frame = map_pop.child_frame.as_ref().unwrap();
        assert_eq!(map_frame.region_start, anchors[map_at]);
        assert_eq!(map_frame.region_end, end_at(map_at + map_bytes.len()));
        assert_eq!(map_pop.parent_before.as_ref().unwrap().program_type_id,
            crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM.type_id);
        assert_eq!(map_pop.parent_before.as_ref().unwrap().region_start, anchors[zipper_at]);
        assert_eq!(map_pop.parent_after.as_ref().unwrap().region_start, anchors[zipper_at]);
        assert_eq!(map_pop.parent_after.as_ref().unwrap().region_end, end_at(zipper_at + zipper_bytes.len()));
        assert_eq!(map_pop.after_len, Some(map_pop.before_len - 1));
        assert_eq!(map_pop.source_len_after, Some(map_pop.source_len_before));

        let zipper_pop = trace.pop_accepts.iter().find(|pop|
            pop.child_program.is_some_and(|program|
                std::ptr::eq(program, &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM))
        ).expect("EZipper message return");
        let zipper_frame = zipper_pop.child_frame.as_ref().unwrap();
        assert_eq!(zipper_frame.region_start, anchors[zipper_at]);
        assert_eq!(zipper_frame.region_end, end_at(zipper_at + zipper_bytes.len()));
        assert_eq!(zipper_pop.parent_before.as_ref().unwrap().program_type_id, EXPR_RHO_MATCH_PROGRAM.type_id);
        assert_eq!(zipper_pop.parent_before.as_ref().unwrap().region_start, anchors[0]);
        assert_eq!(zipper_pop.parent_after.as_ref().unwrap().region_start, anchors[0]);
        assert_eq!(zipper_pop.parent_after.as_ref().unwrap().region_end, terminal);
        assert_eq!(zipper_pop.after_len, Some(zipper_pop.before_len - 1));
        assert_eq!(zipper_pop.source_len_after, Some(zipper_pop.source_len_before));
        let entries = trace.score_arena.as_ref().unwrap();
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let root = trace.returned.as_ref().unwrap().score_handle.unwrap().index();
        assert_eq!(
            transcripts[root],
            authoritative_constructor(14, vec![
                authoritative_bytes_atom(&snapshot),
                authoritative_integer_atom(i64::from(connective_used)),
            ]),
        );

        let absent = Expr {
            expr_instance: Some(ExprInstance::EZipperBody(crate::rhoapi::EZipper::default())),
        };
        let absent_bytes = protobuf_encoder::encode_to_vec(&absent);
        assert_eq!(absent_bytes, vec![0x8a, 0x02, 0]);
        let absent_anchors = (0..absent_bytes.len()).map(|index|
            CanonicalByteProvenance::new(source_base + index as u32, 2000 + index * 29)
        ).collect::<Vec<_>>();
        let absent_terminal = CanonicalByteProvenance::new(901, 9010);
        rpb3_root_finish_trace::start();
        let absent_admission = validate_canonical_protobuf_inner(
            generated_located_admission_region(&absent_bytes, &absent_anchors, absent_terminal),
            &EXPR_RHO_MATCH_PROGRAM, None,
        ).expect("fragmented absent EZipper admits");
        let absent_trace = rpb3_root_finish_trace::take();
        assert_eq!(absent_admission.component().unwrap().complete, false);
        let absent_pop = absent_trace.pop_accepts.iter().find(|pop|
            pop.child_program.is_some_and(|program|
                std::ptr::eq(program, &crate::rust::rholang::match_schema_tables::EZIPPER_RHO_MATCH_PROGRAM))
        ).expect("empty EZipper return");
        assert_eq!(absent_pop.child_body.as_deref(), Some([].as_slice()));
        assert_eq!(absent_pop.child_frame.as_ref().unwrap().region_start, absent_terminal);
        assert_eq!(absent_pop.child_frame.as_ref().unwrap().region_end, absent_terminal);
        assert!(absent_pop.summary.as_ref().unwrap().score_handle.is_none());
        assert_eq!(absent_pop.source_len_after, Some(absent_pop.source_len_before));
        assert_eq!(absent_pop.parent_before.as_ref().unwrap().region_start, absent_anchors[0]);
        assert_eq!(absent_pop.parent_after.as_ref().unwrap().region_end, absent_terminal);
    }
}
