//! Generated production checks for the parser image consumed by the Verus
//! Plain Edge and historical checkpoint models.

use proptest::prelude::*;

use super::*;

fn check_reachable_frames(image: &CanonicalPathParserSourceImage) {
    assert_eq!(
        image.root_gate.dead,
        image.fault.is_some()
            || matches!(image.root.status, CanonicalPathParserStatusSource::Dead(_))
    );
    assert_eq!(image.root_gate.stack_empty, image.root.stack.is_none());
    assert_eq!(
        image.root_gate.split_complete,
        image.root.status == CanonicalPathParserStatusSource::SplitComplete
    );
    assert_eq!(
        image.root_gate.completed_segments,
        image.root.completed_segments
    );
    assert_eq!(
        image.root_gate.first_is_list,
        image.root.first_tag.is_some_and(|tag| tag.byte == ELIST)
    );
    assert_eq!(image.root_gate.stable, image.root.stable);
    assert_eq!(image.root_gate.escape_allowed, image.root.escape_allowed);
    let check = |index: Option<usize>| {
        if let Some(index) = index {
            assert!(index < image.frames.len());
        }
    };
    check(image.focused.stack);
    check(image.root.stack);
    for owner in &image.owner_chain {
        check(owner.owner.stack);
        assert!(owner.region_end >= image.stamp as u128);
        assert!(owner.path_end >= image.stamp as u128);
    }
    for frame in &image.frames {
        match frame {
            CanonicalPathParserFrameSource::Tag { next, .. }
            | CanonicalPathParserFrameSource::Varint { next, .. }
            | CanonicalPathParserFrameSource::Bytes { next, .. }
            | CanonicalPathParserFrameSource::Kids { next, .. }
            | CanonicalPathParserFrameSource::Obligation { next, .. } => check(*next),
            CanonicalPathParserFrameSource::Region { phase, next, .. } => {
                check(*next);
                if let CanonicalPathParserRegionPhaseSource::Path { path, .. } = phase {
                    check(path.stack);
                }
            }
        }
    }
}

fn feed_and_restore(bytes: &[u8], source: u32) -> Vec<CanonicalPathParserSourceImage> {
    let mut admission = CanonicalPathTopologyAdmission::new();
    let mut images = vec![admission.parser_source_image()];
    for (index, &byte) in bytes.iter().enumerate() {
        let located = LocatedPathByte::in_source(byte, source + index as u32 % 5, 13 + index * 19);
        assert_eq!(
            admission.edge(located).unwrap(),
            CanonicalPathTransition::Continue
        );
        let image = admission.parser_source_image();
        assert_eq!(image.stamp, index + 1);
        assert_eq!(
            image.path_bytes,
            images
                .last()
                .unwrap()
                .path_bytes
                .iter()
                .copied()
                .chain(std::iter::once(located))
                .collect::<Vec<_>>()
        );
        assert_eq!(image.root_gate, admission.value_root_gate_source());
        assert_eq!(image.root.stable, image.root_gate.stable);
        check_reachable_frames(&image);
        images.push(image);
    }
    for depth in (0..bytes.len()).rev() {
        assert!(matches!(
            admission
                .leave(CanonicalPathSource::in_source(source, 100_000 + depth))
                .unwrap(),
            CanonicalPathTransition::Restored { .. }
        ));
        assert_eq!(admission.parser_source_image(), images[depth]);
    }
    images
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn generated_plain_byte_array_parser_image_tracks_varint_body_and_restore(
        payload in prop::collection::vec(any::<u8>(), 0..260),
        source in 1u32..10_000,
    ) {
        let mut bytes = vec![GBYTE_ARRAY];
        if payload.len() < 128 {
            bytes.push(payload.len() as u8);
        } else {
            bytes.push((payload.len() as u8 & 0x7f) | 0x80);
            bytes.push((payload.len() >> 7) as u8);
        }
        let body_start = bytes.len();
        bytes.extend_from_slice(&payload);
        let images = feed_and_restore(&bytes, source);
        assert_eq!(images[0].focused.stack, None);
        let tag = &images[1];
        let Some(top) = tag.focused.stack else { panic!("tag opens a length frame") };
        assert!(matches!(
            tag.frames[top],
            CanonicalPathParserFrameSource::Varint {
                role: CanonicalPathParserVarintRoleSource::Bytes { utf8: false },
                state: CanonicalPathParserVarintSource { value: 0, shift: 0, bytes: 0 },
                next: None,
            }
        ));
        if body_start == 3 {
            let encoded = &images[2];
            let top = encoded.focused.stack.unwrap();
            assert!(matches!(
                encoded.frames[top],
                CanonicalPathParserFrameSource::Varint {
                    role: CanonicalPathParserVarintRoleSource::Bytes { utf8: false },
                    state: CanonicalPathParserVarintSource { shift: 7, bytes: 1, .. },
                    next: None,
                }
            ));
        }
        let length = &images[body_start];
        if payload.is_empty() {
            assert_eq!(length.focused.stack, None);
            assert_eq!(length.root_gate.completed_segments, 1);
        } else {
            let top = length.focused.stack.unwrap();
            assert!(matches!(
                length.frames[top],
                CanonicalPathParserFrameSource::Bytes {
                    remaining,
                    utf8: None,
                    next: None,
                } if remaining == payload.len() as u64
            ));
            for consumed in 1..payload.len() {
                let image = &images[body_start + consumed];
                let top = image.focused.stack.unwrap();
                assert!(matches!(
                    image.frames[top],
                    CanonicalPathParserFrameSource::Bytes {
                        remaining,
                        utf8: None,
                        next: None,
                    } if remaining == (payload.len() - consumed) as u64
                ));
            }
            let completed = images.last().unwrap();
            assert_eq!(completed.focused.stack, None);
            assert_eq!(completed.root_gate.completed_segments, 1);
        }
    }

    #[test]
    fn generated_plain_region_parser_image_preserves_owner_and_prior_path(
        payload in prop::collection::vec(any::<u8>(), 0..60),
        source in 1u32..10_000,
    ) {
        let mut path = vec![GBYTE_ARRAY, payload.len() as u8];
        path.extend_from_slice(&payload);
        let mut bytes = vec![EPATHMAP, (path.len() + 1) as u8, path.len() as u8];
        bytes.extend_from_slice(&path);
        let images = feed_and_restore(&bytes, source);
        let inside = &images[3];
        assert_eq!(inside.owner_chain.len(), 1);
        assert_eq!(inside.owner_chain[0].region_end, bytes.len() as u128);
        assert_eq!(inside.owner_chain[0].path_end, bytes.len() as u128);
        assert_eq!(inside.root_gate.completed_segments, 0);
        assert_eq!(inside.focused.first_tag, None);
        let complete = images.last().unwrap();
        assert!(complete.owner_chain.is_empty());
        assert_eq!(complete.root_gate.completed_segments, 1);
        assert_eq!(complete.root.stable, complete.focused.stable);
    }
}

#[test]
fn plain_region_parser_image_preserves_prior_path_bytes_and_nested_owners() {
    let inner = [EPATHMAP, 4, 1, GBOOL_FALSE, 1, GBOOL_TRUE];
    let images = feed_and_restore(&inner, 17);
    let after_first = &images[4];
    let Some(top) = after_first.focused.stack else {
        panic!("the region awaits its next framed path")
    };
    let CanonicalPathParserFrameSource::Region { previous, .. } = &after_first.frames[top] else {
        panic!("the first framed path retains a region continuation")
    };
    assert_eq!(previous.as_ref().unwrap(), &vec![
        LocatedPathByte::in_source(GBOOL_FALSE, 17 + 3, 13 + 3 * 19)
    ]);
    assert_eq!(images.last().unwrap().root_gate.completed_segments, 1);

    let mut nested = vec![EPATHMAP, 7, inner.len() as u8];
    nested.extend_from_slice(&inner);
    let nested_images = feed_and_restore(&nested, 29);
    let two_owners = &nested_images[6];
    assert_eq!(two_owners.owner_chain.len(), 2);
    assert_eq!(two_owners.owner_chain[0].parent, None);
    assert_eq!(
        two_owners.owner_chain[1].parent,
        Some(two_owners.owner_chain[0].id)
    );
    assert_eq!(two_owners.root_gate.completed_segments, 0);
    assert_eq!(
        nested_images.last().unwrap().root_gate.completed_segments,
        1
    );
}

#[test]
fn dead_plain_edges_and_resolved_obligations_have_distinct_parser_images() {
    let mut dead = CanonicalPathTopologyAdmission::new();
    assert_eq!(
        dead.edge(LocatedPathByte::in_source(0xff, 9, 31)).unwrap(),
        CanonicalPathTransition::Continue
    );
    let dead_first = dead.parser_source_image();
    check_reachable_frames(&dead_first);
    assert!(matches!(
        dead_first.focused.status,
        CanonicalPathParserStatusSource::Dead(CanonicalPathAdmissionError {
            kind: CanonicalPathAdmissionErrorKind::ReservedTag(0xff),
            ..
        })
    ));
    assert_eq!(
        dead.edge(LocatedPathByte::in_source(0x06, 10, 47)).unwrap(),
        CanonicalPathTransition::Continue
    );
    let dead_second = dead.parser_source_image();
    check_reachable_frames(&dead_second);
    assert_eq!(dead_second.focused, dead_first.focused);
    assert_eq!(dead_second.path_bytes.len(), 2);
    dead.leave(CanonicalPathSource::new(100)).unwrap();
    assert_eq!(dead.parser_source_image(), dead_first);

    let mut nested_dead = CanonicalPathTopologyAdmission::new();
    for (index, byte) in [EPATHMAP, 2, 1].into_iter().enumerate() {
        assert_eq!(
            nested_dead.edge(LocatedPathByte::new(byte, index)).unwrap(),
            CanonicalPathTransition::Continue
        );
    }
    let healthy_owner = nested_dead.parser_source_image();
    assert_eq!(healthy_owner.owner_chain.len(), 1);
    assert_eq!(healthy_owner.stamp, 3);
    nested_dead.edge(LocatedPathByte::new(0xff, 3)).unwrap();
    let failed_child = nested_dead.parser_source_image();
    check_reachable_frames(&failed_child);
    assert!(failed_child.fault.is_some());
    assert!(failed_child.root_gate.dead);
    assert_eq!(
        failed_child.root.status,
        CanonicalPathParserStatusSource::Open
    );
    assert_eq!(failed_child.focused, healthy_owner.focused);
    assert_eq!(failed_child.owner_chain, healthy_owner.owner_chain);
    assert_eq!(failed_child.stamp, healthy_owner.stamp);
    assert_eq!(
        failed_child.path_bytes.len(),
        healthy_owner.path_bytes.len() + 1
    );
    nested_dead.edge(LocatedPathByte::new(0x06, 4)).unwrap();
    let dead_descendant = nested_dead.parser_source_image();
    assert_eq!(dead_descendant.stamp, healthy_owner.stamp);
    assert_eq!(
        dead_descendant.path_bytes.len(),
        healthy_owner.path_bytes.len() + 2
    );
    assert_eq!(dead_descendant.focused, healthy_owner.focused);
    nested_dead.leave(CanonicalPathSource::new(5)).unwrap();
    assert_eq!(nested_dead.parser_source_image(), failed_child);
    nested_dead.leave(CanonicalPathSource::new(6)).unwrap();
    assert_eq!(nested_dead.parser_source_image(), healthy_owner);

    let mut escaped = CanonicalPathTopologyAdmission::new();
    assert_eq!(
        escaped.edge(LocatedPathByte::new(ESCAPE, 1)).unwrap(),
        CanonicalPathTransition::Continue
    );
    let CanonicalPathTransition::ObligationStarted(obligation) =
        escaped.edge(LocatedPathByte::new(2, 2)).unwrap()
    else {
        panic!("the completed escape length starts the obligation")
    };
    let started = escaped.parser_source_image();
    check_reachable_frames(&started);
    assert_eq!(started.next_obligation, obligation.id.0 + 1);
    assert_eq!(started.focused.first_tag.unwrap().byte, ESCAPE);
    assert_eq!(
        escaped.edge(LocatedPathByte::new(0xaa, 3)).unwrap(),
        CanonicalPathTransition::ObligationByte {
            id: obligation.id,
            byte: LocatedPathByte::new(0xaa, 3),
            last: false,
        }
    );
    let payload = escaped.parser_source_image();
    check_reachable_frames(&payload);
    assert_eq!(payload.focused.stack, started.focused.stack);
    assert_eq!(
        escaped.edge(LocatedPathByte::new(0xbb, 4)).unwrap(),
        CanonicalPathTransition::ObligationByte {
            id: obligation.id,
            byte: LocatedPathByte::new(0xbb, 4),
            last: true,
        }
    );
    let awaiting = escaped.parser_source_image();
    check_reachable_frames(&awaiting);
    let top = awaiting.focused.stack.unwrap();
    assert!(matches!(
        awaiting.frames[top],
        CanonicalPathParserFrameSource::Obligation {
            status: CanonicalPathParserObligationStatusSource::AwaitingResolution { .. },
            ..
        }
    ));
    escaped
        .resolve_obligation(obligation.id, CanonicalPathObligationResolution::RawPar {
            canonical_path_stable: false,
        })
        .unwrap();
    let resolved = escaped.parser_source_image();
    check_reachable_frames(&resolved);
    assert!(resolved.focused.stack.is_none());
    assert!(!resolved.root_gate.stable);
    assert_eq!(resolved.root_gate.completed_segments, 1);
}

#[test]
fn historical_forward_checkpoints_select_exact_parser_image_and_prefix() {
    let wire = [ESCAPE, 6, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    let source = SourceTreeWindow::from_borrowed(&wire, 23, 100).unwrap();
    let mut admission =
        CanonicalPathTopologyAdmission::new_with_source(Some(source.clone())).unwrap();
    for at in 0..2 {
        admission
            .edge(LocatedPathByte {
                byte: wire[at],
                source: CanonicalPathSource::from_provenance(source.provenance_at(at).unwrap()),
            })
            .unwrap();
    }
    let before = admission.parser_source_image();
    assert_eq!(before.stamp, 2);
    assert_eq!(admission.forward_capacity(3), 3);
    assert!(admission.try_forward_source(2..5).unwrap());
    let run_index = admission.restore.len() - 1;
    let images = (2..=4)
        .map(|depth| {
            let image = admission
                .batch_restore_predecessor_parser_source(run_index, depth)
                .expect("each Forward predecessor has a retained handle");
            check_reachable_frames(&image);
            assert_eq!(image.path_bytes.len(), depth);
            assert_eq!(image.stamp, depth);
            assert_eq!(image.focused, before.focused);
            assert_eq!(image.owner_chain, before.owner_chain);
            assert_eq!(image.root_gate, before.root_gate);
            image
        })
        .collect::<Vec<_>>();
    for depth in (2..=4).rev() {
        admission
            .leave(CanonicalPathSource::new(500 + depth))
            .unwrap();
        assert_eq!(admission.parser_source_image(), images[depth - 2]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_utf8_bytes_match_reference_validator(
        payload in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        let mut wire = vec![GSTRING, payload.len() as u8];
        wire.extend_from_slice(&payload);
        for (at, byte) in wire.into_iter().enumerate() {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, at)).unwrap(),
                CanonicalPathTransition::Continue,
            );
        }
        let image = admission.parser_source_image();
        check_reachable_frames(&image);
        if std::str::from_utf8(&payload).is_ok() {
            prop_assert!(!image.root_gate.dead);
            prop_assert!(image.root_gate.stack_empty);
            prop_assert_eq!(image.root_gate.completed_segments, 1);
        } else {
            prop_assert!(image.root_gate.dead);
        }
    }

    #[test]
    fn generated_multibyte_utf8_boundary_rejection_matches_reference(
        class in 0u8..4,
        second in prop_oneof![0x80u8..=0xbf, Just(0x8f), Just(0x90),
            Just(0x9f), Just(0xa0)],
    ) {
        let (lead, mut tail) = match class {
            0 => (0xe0, vec![0x80]),
            1 => (0xed, vec![0x80]),
            2 => (0xf0, vec![0x80, 0x80]),
            _ => (0xf4, vec![0x80, 0x80]),
        };
        let mut payload = vec![lead, second];
        payload.append(&mut tail);
        let expected_valid = std::str::from_utf8(&payload).is_ok();
        let mut admission = CanonicalPathTopologyAdmission::new();
        for (at, byte) in [GSTRING, payload.len() as u8]
            .into_iter()
            .chain(payload.iter().copied())
            .enumerate()
        {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, at)).unwrap(),
                CanonicalPathTransition::Continue,
            );
        }
        let image = admission.parser_source_image();
        check_reachable_frames(&image);
        prop_assert_eq!(!image.root_gate.dead, expected_valid);
        if expected_valid {
            prop_assert!(image.root_gate.stack_empty);
            prop_assert_eq!(image.root_gate.completed_segments, 1);
        }
    }

    #[test]
    fn generated_scalar_varint_enforces_minimality_and_u64_limit(
        groups in prop::collection::vec(0u8..=127, 1..13),
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        admission.edge(LocatedPathByte::new(GINT, 0)).unwrap();
        for (index, group) in groups.iter().copied().enumerate() {
            let byte = group | if index + 1 < groups.len() { 0x80 } else { 0 };
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, index + 1)).unwrap(),
                CanonicalPathTransition::Continue,
            );
            check_reachable_frames(&admission.parser_source_image());
        }
        let image = admission.parser_source_image();
        let valid = groups.len() <= 10
            && (groups.len() == 1 || *groups.last().unwrap() != 0)
            && (groups.len() < 10 || *groups.last().unwrap() <= 1);
        prop_assert_eq!(!image.root_gate.dead, valid);
        if valid {
            prop_assert!(image.root_gate.stack_empty);
            prop_assert_eq!(image.root_gate.completed_segments, 1);
        }
    }

    #[test]
    fn generated_list_children_bubble_through_exact_kids_continuation(
        children in prop::collection::vec(any::<bool>(), 0..10),
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        admission.edge(LocatedPathByte::new(ELIST, 0)).unwrap();
        admission.edge(LocatedPathByte::new(children.len() as u8, 1)).unwrap();
        for (index, child) in children.iter().copied().enumerate() {
            let byte = if child { GBOOL_TRUE } else { GBOOL_FALSE };
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, index + 2)).unwrap(),
                CanonicalPathTransition::Continue,
            );
            let image = admission.parser_source_image();
            check_reachable_frames(&image);
            if index + 1 < children.len() {
                let top = image.focused.stack.expect("Kids opens the next child tag");
                let CanonicalPathParserFrameSource::Tag {
                    escape_allowed: false, next: Some(kids),
                } = image.frames[top] else {
                    panic!("each unfinished child leaves a Tag over Kids")
                };
                assert!(matches!(
                    image.frames[kids],
                    CanonicalPathParserFrameSource::Kids { remaining, .. }
                        if remaining == (children.len() - index - 1) as u64
                ));
            }
        }
        let image = admission.parser_source_image();
        prop_assert!(!image.root_gate.dead);
        prop_assert!(image.root_gate.stack_empty);
        prop_assert_eq!(image.root_gate.completed_segments, 1);
        prop_assert!(image.root_gate.first_is_list);
    }

    #[test]
    fn generated_nested_single_child_lists_bubble_all_kids_frames(
        depth in 1usize..10,
        child in any::<bool>(),
    ) {
        let mut bytes = Vec::with_capacity(depth * 2 + 1);
        for _ in 0..depth {
            bytes.extend_from_slice(&[ELIST, 1]);
        }
        bytes.push(if child { GBOOL_TRUE } else { GBOOL_FALSE });
        let images = feed_and_restore(&bytes, 101);
        let final_image = images.last().unwrap();
        prop_assert!(!final_image.root_gate.dead);
        prop_assert!(final_image.root_gate.stack_empty);
        prop_assert_eq!(final_image.root_gate.completed_segments, 1);
        prop_assert!(final_image.root_gate.first_is_list);
    }

    #[test]
    fn generated_nested_regions_preserve_outer_to_inner_owner_order(
        depth in 1usize..6,
        leaf in any::<bool>(),
    ) {
        let mut bytes = vec![if leaf { GBOOL_TRUE } else { GBOOL_FALSE }];
        for _ in 0..depth {
            let child_len = bytes.len();
            let mut outer = vec![EPATHMAP, (child_len + 1) as u8, child_len as u8];
            outer.extend_from_slice(&bytes);
            bytes = outer;
        }
        let images = feed_and_restore(&bytes, 223);
        prop_assert_eq!(
            images.iter().map(|image| image.owner_chain.len()).max().unwrap(),
            depth,
        );
        for image in &images {
            for (index, owner) in image.owner_chain.iter().enumerate() {
                if index == 0 {
                    prop_assert_eq!(owner.parent, None);
                } else {
                    prop_assert_eq!(owner.parent, Some(image.owner_chain[index - 1].id));
                }
            }
        }
        let final_image = images.last().unwrap();
        prop_assert!(final_image.owner_chain.is_empty());
        prop_assert!(!final_image.root_gate.dead);
        prop_assert!(final_image.root_gate.stack_empty);
        prop_assert_eq!(final_image.root_gate.completed_segments, 1);
    }

    #[test]
    fn generated_region_keys_enforce_strict_byte_order(
        first in prop_oneof![Just(GBOOL_FALSE), Just(GBOOL_TRUE)],
        second in prop_oneof![Just(GBOOL_FALSE), Just(GBOOL_TRUE)],
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        for (at, byte) in [EPATHMAP, 4, 1, first, 1, second].into_iter().enumerate() {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, at)).unwrap(),
                CanonicalPathTransition::Continue,
            );
        }
        let image = admission.parser_source_image();
        check_reachable_frames(&image);
        if first < second {
            prop_assert!(!image.root_gate.dead);
            prop_assert!(image.root_gate.stack_empty);
            prop_assert_eq!(image.root_gate.completed_segments, 1);
        } else {
            prop_assert!(image.root_gate.dead);
        }
    }

    #[test]
    fn generated_region_byte_array_keys_use_lexicographic_encoded_order(
        first_payload in prop::collection::vec(any::<u8>(), 0..30),
        second_payload in prop::collection::vec(any::<u8>(), 0..30),
    ) {
        let mut first = vec![GBYTE_ARRAY, first_payload.len() as u8];
        first.extend_from_slice(&first_payload);
        let mut second = vec![GBYTE_ARRAY, second_payload.len() as u8];
        second.extend_from_slice(&second_payload);
        let mut wire = vec![EPATHMAP,
            (first.len() + second.len() + 2) as u8,
            first.len() as u8];
        wire.extend_from_slice(&first);
        wire.push(second.len() as u8);
        wire.extend_from_slice(&second);
        let mut admission = CanonicalPathTopologyAdmission::new();
        for (at, byte) in wire.into_iter().enumerate() {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, at)).unwrap(),
                CanonicalPathTransition::Continue,
            );
        }
        let image = admission.parser_source_image();
        check_reachable_frames(&image);
        prop_assert_eq!(!image.root_gate.dead, first < second);
        if first < second {
            prop_assert!(image.root_gate.stack_empty);
            prop_assert_eq!(image.root_gate.completed_segments, 1);
        }
    }

    #[test]
    fn generated_post_fault_plain_edges_preserve_parser_and_stale_stamp(
        bad in 0x11u8..=0xff,
        tail in prop::collection::vec(any::<u8>(), 0..24),
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        for (at, byte) in [EPATHMAP, 2, 1, bad].into_iter().enumerate() {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, at)).unwrap(),
                CanonicalPathTransition::Continue,
            );
        }
        let fault = admission.parser_source_image();
        prop_assert!(fault.fault.is_some());
        prop_assert!(fault.root_gate.dead);
        prop_assert_eq!(fault.stamp, 3);
        for (index, byte) in tail.iter().copied().enumerate() {
            prop_assert_eq!(
                admission.edge(LocatedPathByte::new(byte, index + 4)).unwrap(),
                CanonicalPathTransition::Continue,
            );
            let image = admission.parser_source_image();
            check_reachable_frames(&image);
            prop_assert_eq!(image.stamp, fault.stamp);
            prop_assert_eq!(&image.focused, &fault.focused);
            prop_assert_eq!(&image.owner_chain, &fault.owner_chain);
            prop_assert_eq!(image.next_obligation, fault.next_obligation);
            prop_assert_eq!(image.path_bytes.len(), index + 5);
        }
    }

    #[test]
    fn generated_escape_payload_rebases_stamp_then_resolves_unstable(
        payload in prop::collection::vec(any::<u8>(), 1..32),
    ) {
        let mut admission = CanonicalPathTopologyAdmission::new();
        assert_eq!(
            admission.edge(LocatedPathByte::new(ESCAPE, 0)).unwrap(),
            CanonicalPathTransition::Continue,
        );
        let CanonicalPathTransition::ObligationStarted(obligation) = admission
            .edge(LocatedPathByte::new(payload.len() as u8, 1)).unwrap()
        else { panic!("a completed length starts the escape obligation") };
        for (index, byte) in payload.iter().copied().enumerate() {
            let transition = admission.edge(LocatedPathByte::new(byte, index + 2)).unwrap();
            prop_assert_eq!(transition, CanonicalPathTransition::ObligationByte {
                id: obligation.id,
                byte: LocatedPathByte::new(byte, index + 2),
                last: index + 1 == payload.len(),
            });
            let image = admission.parser_source_image();
            check_reachable_frames(&image);
            prop_assert_eq!(image.stamp, index + 3);
        }
        admission.resolve_obligation(
            obligation.id,
            CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: false,
            },
        ).unwrap();
        let image = admission.parser_source_image();
        prop_assert!(!image.root_gate.dead);
        prop_assert!(image.root_gate.stack_empty);
        prop_assert!(!image.root_gate.stable);
        prop_assert_eq!(image.root_gate.completed_segments, 1);
    }

    #[test]
    fn generated_historical_forward_parser_image_selects_saved_handle_after_live_change(
        payload in prop::collection::vec(any::<u8>(), 3..24),
        nested in any::<bool>(),
        source_id in 1u32..1_000,
    ) {
        let mut wire = if nested {
            vec![EPATHMAP, (payload.len() + 3) as u8, (payload.len() + 2) as u8,
                 EPATHMAP_MAP, payload.len() as u8]
        } else {
            vec![ESCAPE, payload.len() as u8]
        };
        let prefix_len = wire.len();
        wire.extend_from_slice(&payload);
        let source = SourceTreeWindow::from_borrowed(&wire, source_id, 100).unwrap();
        let mut admission = CanonicalPathTopologyAdmission::new_with_source(Some(source.clone()))
            .unwrap();
        let mut obligation = None;
        for at in 0..prefix_len {
            let transition = admission.edge(LocatedPathByte {
                byte: wire[at],
                source: CanonicalPathSource::from_provenance(source.provenance_at(at).unwrap()),
            }).unwrap();
            if at + 1 == prefix_len {
                let CanonicalPathTransition::ObligationStarted(started) = transition else {
                    panic!("escape length must start the obligation")
                };
                obligation = Some(started);
            } else {
                prop_assert_eq!(transition, CanonicalPathTransition::Continue);
            }
        }
        let obligation = obligation.unwrap();
        let before = admission.parser_source_image();
        prop_assert_eq!(before.stamp, prefix_len);
        prop_assert_eq!(before.owner_chain.len(), usize::from(nested));
        let width = payload.len() - 1;
        prop_assert_eq!(admission.forward_capacity(width), width);
        prop_assert!(admission.try_forward_source(prefix_len..prefix_len + width).unwrap());
        let run_index = admission.restore.len() - 1;
        let images = (prefix_len..prefix_len + width).map(|depth| {
            let image = admission.batch_restore_predecessor_parser_source(run_index, depth)
                .expect("Forward retained the exact predecessor handle");
            check_reachable_frames(&image);
            assert_eq!(image.stamp, depth);
            assert_eq!(image.path_bytes.len(), depth);
            assert_eq!(image.focused, before.focused);
            assert_eq!(image.owner_chain, before.owner_chain);
            assert_eq!(image.root_gate, before.root_gate);
            for (index, located) in image.path_bytes.iter().enumerate() {
                assert_eq!(located.byte, wire[index]);
                assert_eq!(
                    located.source,
                    CanonicalPathSource::from_provenance(source.provenance_at(index).unwrap())
                );
            }
            for owner in &image.owner_chain {
                assert_eq!(owner.region_end, wire.len() as u128);
                assert_eq!(owner.path_end, wire.len() as u128);
                assert!(owner.region_end >= image.stamp as u128);
            }
            image
        }).collect::<Vec<_>>();

        let last_at = prefix_len + width;
        prop_assert_eq!(
            admission.edge(LocatedPathByte {
                byte: wire[last_at],
                source: CanonicalPathSource::from_provenance(
                    source.provenance_at(last_at).unwrap()
                ),
            }).unwrap(),
            CanonicalPathTransition::ObligationByte {
                id: obligation.id,
                byte: LocatedPathByte {
                    byte: wire[last_at],
                    source: CanonicalPathSource::from_provenance(
                        source.provenance_at(last_at).unwrap()
                    ),
                },
                last: true,
            }
        );
        admission.resolve_obligation(
            obligation.id,
            if nested {
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                }
            } else {
                CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: false,
                }
            },
        ).unwrap();
        prop_assert_ne!(admission.parser_source_image().root_gate, before.root_gate);
        for (index, expected) in images.iter().enumerate() {
            let selected = admission.batch_restore_predecessor_parser_source(
                run_index, prefix_len + index,
            ).unwrap();
            check_reachable_frames(&selected);
            prop_assert_eq!(&selected, expected);
        }
        admission.leave(CanonicalPathSource::new(9_000)).unwrap();
        for depth in (prefix_len..prefix_len + width).rev() {
            admission.leave(CanonicalPathSource::new(9_001 + depth)).unwrap();
            prop_assert_eq!(&admission.parser_source_image(), &images[depth - prefix_len]);
        }
    }
}
