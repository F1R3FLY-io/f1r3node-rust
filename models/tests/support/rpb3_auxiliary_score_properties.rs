fn auxiliary_new_score_case(
    bind_count: i32,
    value: i64,
    nested_connective: bool,
    zipper_connective: bool,
    include_map_injection: bool,
) -> (Vec<u8>, Vec<u8>) {
    let map = EPathMap::new_map(
        [(gint(value), gint(value + 1))],
        Vec::new(),
        nested_connective,
        None,
    );
    let map_bytes = protobuf_encoder::encode_to_vec(&map);
    let snapshot = independent_epathmap_snapshot_field(&map_bytes);
    assert_eq!(snapshot, map.trie_snapshot());
    let map_expr = Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
    };
    let map_par = Par::default().with_exprs(vec![map_expr]);
    let zipper = crate::rhoapi::EZipper {
        pathmap: Some(map),
        current_path: Vec::new(),
        is_write_zipper: false,
        locally_free: Vec::new(),
        connective_used: zipper_connective,
        cursor_kind: 0,
    };
    let zipper_expr = Expr {
        expr_instance: Some(ExprInstance::EZipperBody(zipper)),
    };
    let zipper_par = Par::default().with_exprs(vec![zipper_expr]);
    let first_key = "a\0path".to_owned();
    let second_key = "a\u{1}path".to_owned();
    let first_uri = "rho:\0path".to_owned();
    let second_uri = "rho:\u{1}path".to_owned();
    assert!(first_key.contains('\0') && first_uri.contains('\0'));
    let mut injections = std::collections::BTreeMap::new();
    if include_map_injection {
        injections.insert(first_key.clone(), map_par.clone());
    }
    injections.insert(second_key.clone(), zipper_par.clone());
    let new = New {
        bind_count,
        p: Some(gint(value + 2)),
        uri: vec![first_uri.clone(), second_uri.clone()],
        injections,
        ..New::default()
    };
    let map_score = authoritative_constructor(Score::EPATHMAP, vec![
        authoritative_bytes_atom(&snapshot),
        authoritative_integer_atom(i64::from(nested_connective)),
    ]);
    let zipper_score = authoritative_constructor(Score::EPATHMAP + 1, vec![
        authoritative_bytes_atom(&snapshot),
        authoritative_integer_atom(i64::from(zipper_connective)),
    ]);
    let map_par_score =
        authoritative_constructor(999, vec![map_score, authoritative_integer_atom(0)]);
    let zipper_par_score =
        authoritative_constructor(999, vec![zipper_score, authoritative_integer_atom(0)]);
    let mut children = vec![
        authoritative_integer_atom(i64::from(bind_count)),
        authoritative_string_atom(&first_uri),
        authoritative_string_atom(&second_uri),
    ];
    if include_map_injection {
        children.push(authoritative_node([
            authoritative_string_atom(&first_key),
            map_par_score,
        ]));
    }
    children.push(authoritative_node([
        authoritative_string_atom(&second_key),
        zipper_par_score.clone(),
    ]));
    children.push(property_par_gint_score(value + 2));
    let expected = authoritative_constructor(Score::NEW, children);
    let trace = property_component_source(&new, &NEW_RHO_MATCH_PROGRAM, &expected);
    assert_eq!(trace.new_finishes.len(), 1);
    let NewFinishInput::Complete {
        bind_count: observed_count,
        uris,
        injections: observed_injections,
        ..
    } = &trace.new_finishes[0].input
    else {
        panic!("complete New returned an incomplete trace");
    };
    assert_eq!(*observed_count, bind_count);
    assert_eq!(uris, &vec![first_uri.into_bytes(), second_uri.into_bytes()]);
    assert_eq!(observed_injections.len(), new.injections.len());
    let zipper_body = protobuf_encoder::encode_to_vec(&zipper_par);
    let zipper_children = trace
        .children
        .iter()
        .filter(|child| {
            std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM) && child.body == zipper_body
        })
        .collect::<Vec<_>>();
    assert_eq!(zipper_children.len(), 1);
    assert_eq!(
        zipper_children[0].summary.score_bytes.as_deref(),
        Some(zipper_par_score.as_slice()),
    );
    assert!(trace
        .epathmap_scores
        .iter()
        .any(|score| score.snapshot == snapshot));
    (expected, zipper_par_score)
}

fn borrowed_admission_score_entry(
    entry: crate::rust::rholang::canonical_score_bytes::CanonicalScoreEntryView<'_, '_>,
) -> CanonicalScoreSourceEntry {
    let kind = match entry.kind {
        CanonicalScoreNodeView::Integer(value) => CanonicalScoreSourceKind::Integer(value),
        CanonicalScoreNodeView::Node(children) => {
            CanonicalScoreSourceKind::Node(children.iter().map(|child| child.index()).collect())
        }
        CanonicalScoreNodeView::String(bytes) => match bytes {
            CanonicalScoreBytesView::Owned(value) => {
                CanonicalScoreSourceKind::StringOwned(value.to_vec())
            }
            CanonicalScoreBytesView::Shared(value) => {
                CanonicalScoreSourceKind::StringShared(value.to_vec())
            }
            CanonicalScoreBytesView::Region(value) => {
                CanonicalScoreSourceKind::StringRegion(value.as_bytes().to_vec())
            }
            CanonicalScoreBytesView::Indexed(value) => {
                CanonicalScoreSourceKind::StringIndexed(value.bytes().collect())
            }
            CanonicalScoreBytesView::BorrowedIndexed(value) => {
                CanonicalScoreSourceKind::StringIndexed(value.as_slice().to_vec())
            }
        },
        CanonicalScoreNodeView::Bytes(bytes) => match bytes {
            CanonicalScoreBytesView::Owned(value) => {
                CanonicalScoreSourceKind::BytesOwned(value.to_vec())
            }
            CanonicalScoreBytesView::Shared(value) => {
                CanonicalScoreSourceKind::BytesShared(value.to_vec())
            }
            CanonicalScoreBytesView::Region(value) => {
                CanonicalScoreSourceKind::BytesRegion(value.as_bytes().to_vec())
            }
            CanonicalScoreBytesView::Indexed(value) => {
                CanonicalScoreSourceKind::BytesIndexed(value.bytes().collect())
            }
            CanonicalScoreBytesView::BorrowedIndexed(value) => {
                CanonicalScoreSourceKind::BytesIndexed(value.as_slice().to_vec())
            }
        },
    };
    CanonicalScoreSourceEntry {
        kind,
        encoded_len: entry.encoded_len,
    }
}

fn independent_admission_score_atom(prefix: u8, bytes: &[u8]) -> Vec<u8> {
    let mut encoded = vec![1, prefix];
    for byte in bytes {
        encoded.push(*byte);
        if *byte == 0 {
            encoded.push(0xff);
        }
    }
    encoded.extend([0, 0]);
    encoded
}

fn independently_encode_admission_score_source(
    entries: &[CanonicalScoreSourceEntry],
) -> Option<Vec<Vec<u8>>> {
    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let bytes = match &entry.kind {
            CanonicalScoreSourceKind::Integer(value) => authoritative_integer_atom(*value),
            CanonicalScoreSourceKind::StringOwned(value)
            | CanonicalScoreSourceKind::StringShared(value)
            | CanonicalScoreSourceKind::StringRegion(value)
            | CanonicalScoreSourceKind::StringIndexed(value) => {
                independent_admission_score_atom(2, value)
            }
            CanonicalScoreSourceKind::BytesOwned(value)
            | CanonicalScoreSourceKind::BytesShared(value)
            | CanonicalScoreSourceKind::BytesRegion(value)
            | CanonicalScoreSourceKind::BytesIndexed(value) => {
                independent_admission_score_atom(3, value)
            }
            CanonicalScoreSourceKind::Node(children) => {
                let mut node = vec![2];
                for child in children {
                    if *child >= index {
                        return None;
                    }
                    node.extend_from_slice(encoded.get(*child)?);
                }
                node.push(0);
                node
            }
        };
        if bytes.len() != entry.encoded_len {
            return None;
        }
        encoded.push(bytes);
    }
    Some(encoded)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn generated_send_message_complete_extracts_the_actual_score_arena(
        channel in any::<i16>(),
        first in any::<i16>(),
        second in any::<i16>(),
        persistent in any::<bool>(),
        connective_used in any::<bool>(),
    ) {
        let send = Send {
            chan: Some(gint(i64::from(channel))),
            data: vec![gint(i64::from(first)), gint(i64::from(second))],
            persistent,
            connective_used,
            ..Send::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&send);
        let expected = authoritative_constructor(Score::SEND, vec![
            authoritative_integer_atom(i64::from(persistent)),
            property_par_gint_score(i64::from(channel)),
            property_par_gint_score(i64::from(first)),
            property_par_gint_score(i64::from(second)),
            authoritative_integer_atom(i64::from(connective_used)),
        ]);
        rpb3_root_finish_trace::start_with_message_success_stream();
        let source = IndexedByteRegion::from_protobuf_region(
            ProtobufRegion::copy_from_slice(&bytes), 0,
        );
        let (summary, scores, _) = drive_admission(
            FrameRequest::Message {
                region: source,
                program: &SEND_RHO_MATCH_PROGRAM,
            },
            bytes.len(),
        ).expect("generated Send completes through the production driver");
        let trace = rpb3_root_finish_trace::take();
        let FrameSummary::Message(MessageSummary {
            semantic: SemanticSummary::Component(component),
        }) = summary else {
            panic!("generated Send returns a component summary");
        };
        prop_assert_eq!(component.family, ParComponentTag::Send);
        prop_assert!(component.complete);
        let root = component.score.index();
        prop_assert_eq!(root + 1, scores.source_len());

        let root_events: Vec<_> = trace.checked_events.iter().filter(|event| {
            matches!(event.projection, CheckedAdmissionProjection::MessageComplete {
                scored: true, root: true, ..
            })
        }).collect();
        prop_assert_eq!(root_events.len(), 1);
        let event = root_events[0];
        prop_assert_eq!(event.before_top_kind, FrameRequestKind::Message);
        prop_assert_eq!(event.before_len, 1);
        prop_assert_eq!(event.after_len, 0);
        prop_assert_eq!(event.score_len_after, scores.source_len());
        prop_assert!(event.score_len_before < event.score_len_after);
        let root_eof = trace.message_success.iter().find_map(|step| match step {
            rpb3_root_finish_trace::MessageSuccessState::Eof {
                step_index,
                score_len_before,
                score_len_after,
                component,
                root: true,
                ..
            } => Some((*step_index, *score_len_before, *score_len_after, *component)),
            _ => None,
        }).expect("checked root EOF has its source receipt");
        prop_assert_eq!(root_eof.0, event.step_index);
        prop_assert_eq!(root_eof.1, event.score_len_before);
        prop_assert_eq!(root_eof.2, event.score_len_after);
        let selected = root_eof.3.expect("Send EOF returns component metadata");
        prop_assert_eq!(selected.family, ParComponentTag::Send);
        prop_assert_eq!(selected.score_handle, component.score);
        prop_assert!(selected.complete);
        prop_assert!(scores.source_suffix_valid(event.score_len_before));

        let entries: Vec<_> = (0..scores.source_len())
            .map(|index| borrowed_admission_score_entry(
                scores.source_entry_at(index).expect("live score entry"),
            ))
            .collect();
        prop_assert!(scores.source_entry_at(scores.source_len()).is_none());
        let suffix: Vec<_> = scores.source_suffix_from(event.score_len_before)
            .expect("checked EOF score suffix")
            .map(|(index, view)| (index, borrowed_admission_score_entry(view)))
            .collect();
        prop_assert_eq!(suffix.len(), event.score_len_after - event.score_len_before);
        for (offset, (index, entry)) in suffix.iter().enumerate() {
            prop_assert_eq!(*index, event.score_len_before + offset);
            prop_assert_eq!(entry, &entries[*index]);
        }
        let par_children: Vec<_> = trace.children.iter().filter(|child| {
            std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM)
        }).collect();
        prop_assert_eq!(par_children.len(), 3);
        let expected_bodies = [channel, first, second].map(|value| {
            protobuf_encoder::encode_to_vec(&gint(i64::from(value)))
        });
        let child_handles: Vec<_> = par_children.iter().zip(expected_bodies.iter())
            .map(|(child, body)| {
                assert_eq!(&child.body, body);
                child.summary.score_handle.expect("admitted Send child has a score").index()
            })
            .collect();
        let before = event.score_len_before;
        let expected_suffix = vec![
            CanonicalScoreSourceEntry {
                kind: CanonicalScoreSourceKind::Integer(i64::from(persistent)),
                encoded_len: 10,
            },
            CanonicalScoreSourceEntry {
                kind: CanonicalScoreSourceKind::Integer(i64::from(connective_used)),
                encoded_len: 10,
            },
            CanonicalScoreSourceEntry {
                kind: CanonicalScoreSourceKind::Integer(i64::from(Score::SEND)),
                encoded_len: 10,
            },
            CanonicalScoreSourceEntry {
                kind: CanonicalScoreSourceKind::Node(vec![
                    before + 2, before, child_handles[0], child_handles[1], child_handles[2],
                    before + 1,
                ]),
                encoded_len: expected.len(),
            },
        ];
        prop_assert_eq!(&entries[before..event.score_len_after], expected_suffix.as_slice());
        prop_assert_eq!(trace.score_arena.as_ref(), Some(&entries));
        let encoded = independently_encode_admission_score_source(&entries)
            .expect("borrowed score source has exact lengths and backward handles");
        for (handle, value) in child_handles.iter().zip([channel, first, second]) {
            prop_assert_eq!(&encoded[*handle], &property_par_gint_score(i64::from(value)));
        }
        prop_assert_eq!(encoded[root].as_slice(), expected.as_slice());
        let actual_root: Vec<u8> = scores.cursor(component.score).collect();
        prop_assert_eq!(actual_root.as_slice(), expected.as_slice());

        let mut bad_length = entries.clone();
        bad_length[root].encoded_len += 1;
        prop_assert!(independently_encode_admission_score_source(&bad_length).is_none());

        let mut future_child = entries.clone();
        let CanonicalScoreSourceKind::Node(children) = &mut future_child[root].kind else {
            panic!("Send score root is a constructor node");
        };
        let tag_handle = children[0];
        children[0] = root;
        prop_assert!(independently_encode_admission_score_source(&future_child).is_none());

        let mut wrong_tag = entries.clone();
        let CanonicalScoreSourceKind::Integer(tag) = &mut wrong_tag[tag_handle].kind else {
            panic!("Send score constructor has an integer tag");
        };
        *tag += 1;
        let altered = independently_encode_admission_score_source(&wrong_tag)
            .expect("altering an integer preserves score-source syntax");
        prop_assert_ne!(altered[root].as_slice(), expected.as_slice());
    }

    #[test]
    fn generated_raw_par_preserves_native_bag_multiplicity_and_root_cache_quotient(
        values in prop::collection::vec(any::<i16>(), 1..12),
        locally_free in prop::collection::btree_set(0u8..128, 0..6),
        connective_used in any::<bool>(),
    ) {
        let mut components = values.iter().map(|value| Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(*value))),
        }).collect::<Vec<_>>();
        let par = Par::default().with_exprs(components.clone())
            .with_connective_used(connective_used);
        let cached = par.clone().with_locally_free(locally_free.into_iter().collect());
        let raw_score = admitted_par_score(&par);
        prop_assert_eq!(&raw_score, &authoritative_par_score(&par));
        prop_assert_eq!(&raw_score, &admitted_par_score(&cached));

        components.push(Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(values[0]))),
        });
        let repeated = Par::default().with_exprs(components)
            .with_connective_used(connective_used);
        let repeated_raw_score = admitted_par_score(&repeated);
        prop_assert_eq!(&repeated_raw_score, &authoritative_par_score(&repeated));
        prop_assert_ne!(&raw_score, &repeated_raw_score);
    }

    #[test]
    fn complete_send_score_retains_channel_data_order_and_flags(
        channel in any::<i16>(),
        first in any::<i16>(),
        second in any::<i16>(),
        persistent in any::<bool>(),
        connective_used in any::<bool>(),
        with_cost in any::<bool>(),
        cost_seed in any::<u16>(),
    ) {
        let cost_region = with_cost.then(|| property_cost_region(cost_seed));
        let send = Send {
            chan: Some(gint(i64::from(channel))),
            data: vec![gint(i64::from(first)), gint(i64::from(second))],
            persistent,
            connective_used,
            cost_region: cost_region.clone(),
            ..Send::default()
        };
        let mut children = vec![
            authoritative_integer_atom(i64::from(persistent)),
            property_par_gint_score(i64::from(channel)),
            property_par_gint_score(i64::from(first)),
            property_par_gint_score(i64::from(second)),
            authoritative_integer_atom(i64::from(connective_used)),
        ];
        if let Some(region) = &cost_region {
            children.push(authoritative_cost_region_score(region));
        }
        let expected = authoritative_constructor(Score::SEND, children);
        let trace = property_component_source(&send, &SEND_RHO_MATCH_PROGRAM, &expected);
        prop_assert_eq!(trace.send_finishes.len(), 1);
        prop_assert!(trace.send_finishes[0].complete);
        match &trace.send_finishes[0].input {
            SendFinishInput::Complete { cost, .. } => prop_assert_eq!(cost.is_some(), with_cost),
            SendFinishInput::Incomplete => prop_assert!(false, "complete Send finished incomplete"),
        }
    }

    #[test]
    fn complete_new_nested_pathmap_injections_match_authoritative_score_atoms(
        value in -1000i16..1000,
        random_count in any::<i32>(),
        nested_connective in any::<bool>(),
        zipper_connective in any::<bool>(),
    ) {
        for bind_count in [i32::MIN, -1, 0, 1, i32::MAX, random_count] {
            auxiliary_new_score_case(
                bind_count,
                i64::from(value),
                nested_connective,
                zipper_connective,
                true,
            );
        }
    }

    #[test]
    fn ezipper_injection_score_uses_snapshot_and_own_connective_flag(
        value in -1000i16..1000,
        zipper_connective in any::<bool>(),
    ) {
        let without_nested_connective = auxiliary_new_score_case(
            i32::MIN, i64::from(value), false, zipper_connective, false,
        );
        let with_nested_connective = auxiliary_new_score_case(
            i32::MIN, i64::from(value), true, zipper_connective, false,
        );
        prop_assert_eq!(without_nested_connective, with_nested_connective);
    }

    #[test]
    fn generated_new_map_entries_default_absent_values_and_require_key_order(
        first in b'a'..=b'm',
        second in b'n'..=b'z',
    ) {
        let entry = |key| vec![34, 3, 10, 1, key];
        let mut ordered = vec![18, 0];
        ordered.extend(entry(first));
        ordered.extend(entry(second));
        let empty_par_score = authoritative_constructor(
            Score::PAR,
            vec![authoritative_integer_atom(0)],
        );
        let injection = |key| authoritative_node([
            authoritative_string_atom(&(key as char).to_string()),
            empty_par_score.clone(),
        ]);
        let expected = authoritative_constructor(Score::NEW, vec![
            authoritative_integer_atom(0),
            authoritative_integer_atom(0),
            injection(first),
            injection(second),
            empty_par_score,
        ]);
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&ordered),
            &NEW_RHO_MATCH_PROGRAM,
        ).expect("ordered New map entries with absent values admit");
        prop_assert_eq!(admission.score_bytes(), Some(expected.as_slice()));

        let mut reversed = vec![18, 0];
        reversed.extend(entry(second));
        reversed.extend(entry(first));
        property_rejected_without_root(&reversed, &NEW_RHO_MATCH_PROGRAM);
    }

    #[test]
    fn generated_string_and_bytes_atoms_have_exact_zero_escape_length(
        tail in prop::collection::vec(0u8..=127, 0..24),
        bytes_tail in prop::collection::vec(any::<u8>(), 0..24),
    ) {
        let string = String::from_utf8(std::iter::once(0).chain(tail).collect())
            .expect("ASCII property string");
        let string_atom = authoritative_string_atom(&string);
        let string_len = 2 + string.len()
            + string.as_bytes().iter().filter(|&&byte| byte == 0).count() + 2;
        for (instance, constructor) in [
            (ExprInstance::GString(string.clone()), Score::STRING),
            (ExprInstance::GUri(string.clone()), Score::URI),
        ] {
            let expr = Expr { expr_instance: Some(instance) };
            let expected = authoritative_constructor(constructor, vec![string_atom.clone()]);
            let trace = property_component_source(&expr, &EXPR_RHO_MATCH_PROGRAM, &expected);
            let entries = trace.score_arena.as_ref().unwrap();
            let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
            let atom_handles = entries.iter().enumerate().filter_map(|(index, entry)| {
                matches!(&entry.kind, CanonicalScoreSourceKind::StringIndexed(value)
                    if value.as_slice() == string.as_bytes()).then_some(index)
            }).collect::<Vec<_>>();
            prop_assert_eq!(atom_handles.len(), 1);
            prop_assert_eq!(&transcripts[atom_handles[0]], &string_atom);
            prop_assert_eq!(transcripts[atom_handles[0]].len(), string_len);
        }

        let payload = [0, 0xff].into_iter().chain(bytes_tail).collect::<Vec<_>>();
        let bytes_atom = authoritative_bytes_atom(&payload);
        let bytes_len = 2 + payload.len()
            + payload.iter().filter(|&&byte| byte == 0).count() + 2;
        let expr = Expr {
            expr_instance: Some(ExprInstance::GByteArray(payload.clone())),
        };
        let expected = authoritative_constructor(Score::EBYTEARR, vec![bytes_atom.clone()]);
        let trace = property_component_source(&expr, &EXPR_RHO_MATCH_PROGRAM, &expected);
        let entries = trace.score_arena.as_ref().unwrap();
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let atom_handles = entries.iter().enumerate().filter_map(|(index, entry)| {
            matches!(&entry.kind, CanonicalScoreSourceKind::BytesIndexed(value)
                if value == &payload).then_some(index)
        }).collect::<Vec<_>>();
        prop_assert_eq!(atom_handles.len(), 1);
        prop_assert_eq!(&transcripts[atom_handles[0]], &bytes_atom);
        prop_assert_eq!(transcripts[atom_handles[0]].len(), bytes_len);
    }

    #[test]
    fn generated_new_uri_order_requires_a_complete_body(
        tail in prop::collection::vec(0u8..=127, 0..16),
    ) {
        let left = String::from_utf8(std::iter::once(b'a').chain(tail.clone()).collect())
            .expect("ASCII URI");
        let right = String::from_utf8(std::iter::once(b'z').chain(tail).collect())
            .expect("ASCII URI");
        prop_assert!(left < right);
        let encode_uris = |uris: [&String; 2]| {
            let mut bytes = Vec::new();
            for uri in uris {
                assert!(uri.len() < 128);
                bytes.extend([26, uri.len() as u8]);
                bytes.extend_from_slice(uri.as_bytes());
            }
            bytes
        };
        let complete = New {
            p: Some(Par::default()),
            uri: vec![left.clone(), right.clone()],
            ..New::default()
        };
        let expected = authoritative_constructor(Score::NEW, vec![
            authoritative_integer_atom(0),
            authoritative_string_atom(&left),
            authoritative_string_atom(&right),
            authoritative_integer_atom(0),
            authoritative_constructor(999, vec![authoritative_integer_atom(0)]),
        ]);
        property_component_source(&complete, &NEW_RHO_MATCH_PROGRAM, &expected);
        let mut canonical_wire = vec![18, 0];
        canonical_wire.extend(encode_uris([&left, &right]));
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&complete), canonical_wire);

        let reversed = encode_uris([&right, &left]);
        let mut invalid_complete = vec![18, 0];
        invalid_complete.extend_from_slice(&reversed);
        property_rejected_without_root(&invalid_complete, &NEW_RHO_MATCH_PROGRAM);
        let incomplete = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&reversed), &NEW_RHO_MATCH_PROGRAM,
        ).expect("URI ordering is not a completeness requirement for an absent New body");
        let nonsemantic = authoritative_constructor(i32::MAX, Vec::new());
        prop_assert_eq!(incomplete.score_bytes(), Some(nonsemantic.as_slice()));
    }
}
