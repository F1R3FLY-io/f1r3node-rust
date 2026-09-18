use proptest::prelude::*;

use super::{checked_add, delimited_len, finish_node, EmitMode, Node, Piece, ProtobufRegionError};

#[path = "../par_corpus/mod.rs"]
mod par_corpus;

use std::cell::RefCell;

use super::{Builder, ProtobufCutPlan, ProtobufCutStats, ProtobufRegion};
use crate::rust::rholang::match_schema::RhoMatchProgram;
use crate::rust::rholang::match_schema_tables::PAR_PROTOBUF_REGION_PROGRAM;
use crate::rust::rholang::protobuf_encoder;

#[cfg(target_pointer_width = "64")]
#[test]
fn native_occurrence_counter_crosses_u32_boundary_and_resets() {
    use super::{Frame, FrameSchema};
    let mut frame = Frame::new(
        ProtobufRegion::from(Vec::new()), FrameSchema::Message(&PAR_PROTOBUF_REGION_PROGRAM),
        None, None, true,
    );
    frame.last_tag = 1;
    frame.occurrence = u32::MAX as usize - 1;
    assert_eq!(frame.next_occurrence(1), u32::MAX as usize);
    assert_eq!(frame.next_occurrence(1), u32::MAX as usize + 1);
    assert_eq!(frame.next_occurrence(2), 0);
    assert_eq!(frame.next_occurrence(1), 0);
}

#[test]
fn synthetic_invalid_counter_retains_checked_overflow_behavior() {
    use super::{Frame, FrameSchema};
    let mut frame = Frame::new(
        ProtobufRegion::from(Vec::new()), FrameSchema::Message(&PAR_PROTOBUF_REGION_PROGRAM),
        None, None, true,
    );
    frame.last_tag = 1;
    frame.occurrence = usize::MAX;
    assert_eq!(frame.next_occurrence(2), 0);
    frame.last_tag = 1;
    frame.occurrence = usize::MAX;
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frame.next_occurrence(1))).is_err());
}

#[test]
fn native_par_cut_paths_preserve_machine_sized_occurrences() {
    use super::{Frame, FrameSchema, ProtobufRegionStepKind};
    let mut builder = Builder::new();
    let mut frames = vec![Frame::new(
        ProtobufRegion::from(Vec::new()), FrameSchema::Message(&PAR_PROTOBUF_REGION_PROGRAM),
        None, None, true,
    )];
    let ordinals = [0, 1, u32::MAX as usize, usize::MAX / 2, usize::MAX];
    for occurrence in ordinals {
        let index = builder.cuts.len();
        builder.add_child(
            &mut frames, ProtobufRegion::from(vec![0x20, 1]),
            &PAR_PROTOBUF_REGION_PROGRAM, 1, 1, occurrence,
            ProtobufRegionStepKind::Message, PAR_PROTOBUF_REGION_PROGRAM.type_id, None, true,
        ).unwrap();
        let path = &builder.paths[builder.cuts[index].path_leaf];
        assert_eq!(path.step.occurrence, occurrence);
        assert!(matches!(frames[0].pieces.last(), Some(Piece::Par { tag: 1, cut }) if *cut == index));
    }
}

proptest! {
    #[test]
    fn occurrence_transition_matches_independent_tag_run_length(tags in prop::collection::vec(1u32..128, 0..96)) {
        use super::{Frame, FrameSchema};
        let mut frame = Frame::new(
            ProtobufRegion::from(Vec::new()), FrameSchema::Message(&PAR_PROTOBUF_REGION_PROGRAM),
            None, None, true,
        );
        for (index, tag) in tags.iter().copied().enumerate() {
            let expected = tags[..index].iter().rev().take_while(|previous| **previous == tag).count();
            prop_assert_eq!(frame.next_occurrence(tag), expected);
            prop_assert_eq!(frame.last_tag, tag);
        }
    }
}

struct PublishedNode {
    source: ProtobufRegion,
    schema: super::FrameSchema,
    path_leaf: Option<usize>,
    output: [prost::bytes::Bytes; 3],
    tables: RegisteredTables,
}

#[derive(Clone)]
struct RegisteredTables {
    paths: Vec<super::PathNode>,
    cuts: Vec<super::ProtobufParCut>,
    groups: Vec<super::ProtobufEPathMapRegion>,
}

fn assert_same_region(before: &ProtobufRegion, after: &ProtobufRegion) {
    assert_eq!(before.absolute_range(), after.absolute_range());
    assert_eq!(before.as_bytes(), after.as_bytes());
    assert_eq!(before.as_bytes().as_ptr(), after.as_bytes().as_ptr());
}

impl RegisteredTables {
    fn capture(builder: &Builder) -> Self {
        Self {
            paths: builder.paths.clone(),
            cuts: builder.cuts.clone(),
            groups: builder.epathmaps.clone(),
        }
    }

    fn assert_preserved(
        &self,
        paths: &[super::PathNode],
        cuts: &[super::ProtobufParCut],
        groups: &[super::ProtobufEPathMapRegion],
    ) {
        assert!(paths.len() >= self.paths.len());
        assert!(cuts.len() >= self.cuts.len());
        assert!(groups.len() >= self.groups.len());
        for (old, new) in self.paths.iter().zip(paths) {
            assert_eq!(old.parent, new.parent);
            assert_eq!(old.step, new.step);
        }
        for (old, new) in self.cuts.iter().zip(cuts) {
            assert_same_region(&old.region, &new.region);
            assert_eq!(old.path_leaf, new.path_leaf);
        }
        for (old, new) in self.groups.iter().zip(groups) {
            assert_same_region(&old.prefix, &new.prefix);
            assert_eq!(old.mode, new.mode);
            assert_eq!(old.cuts, new.cuts);
            assert_eq!(old.path_leaf, new.path_leaf);
        }
    }
}

struct RegisteredGroup {
    index: usize,
    first_cut: usize,
    first_path: usize,
    payload: ProtobufRegion,
    values: Vec<Vec<u8>>,
    mode: crate::rust::epathmap_trie_codec::EPathMapMode,
}

fn verify_registered_group(builder: &Builder, observed: &RegisteredGroup) {
    use super::ProtobufRegionStepKind;
    let group = &builder.epathmaps[observed.index];
    assert_eq!(group.mode, observed.mode);
    assert_eq!(group.cuts, observed.first_cut..observed.first_cut + observed.values.len());
    assert_eq!(group.path_leaf, observed.first_path);
    let path = &builder.paths[group.path_leaf];
    assert_eq!(path.parent, None);
    assert_eq!(path.step.kind, ProtobufRegionStepKind::EPathMapSnapshot);
    assert_eq!(path.step.owner_type_id, PAR_PROTOBUF_REGION_PROGRAM.type_id);
    assert_eq!(path.step.field_tag, 1);
    assert_eq!(path.step.occurrence, observed.index);
    let mut rebuilt = group.prefix.as_bytes().to_vec();
    assert_eq!(group.prefix.absolute_range().start, observed.payload.absolute_range().start);
    assert_eq!(group.prefix.as_bytes().as_ptr(), observed.payload.as_bytes().as_ptr());
    for (ordinal, expected) in observed.values.iter().enumerate() {
        let cut = &builder.cuts[group.cuts.start + ordinal];
        assert_eq!(cut.path_leaf, observed.first_path + 1 + ordinal);
        let path = &builder.paths[cut.path_leaf];
        assert_eq!(path.parent, Some(group.path_leaf));
        assert_eq!(path.step.kind, ProtobufRegionStepKind::EPathMapValue);
        assert_eq!(path.step.owner_type_id, PAR_PROTOBUF_REGION_PROGRAM.type_id);
        assert_eq!(path.step.field_tag, 1);
        assert_eq!(path.step.occurrence, ordinal);
        prost::encoding::encode_varint(expected.len() as u64, &mut rebuilt);
        let start = rebuilt.len();
        let end = start + expected.len();
        assert_eq!(cut.region.as_bytes(), expected);
        assert_eq!(cut.region.absolute_range(), observed.payload.absolute_range().start + start..observed.payload.absolute_range().start + end);
        assert_eq!(cut.region.as_bytes().as_ptr(), observed.payload.as_bytes()[start..].as_ptr());
        rebuilt.extend_from_slice(expected);
    }
    assert_eq!(rebuilt, observed.payload.as_bytes());
}

fn build_registration_sequence(values: &[Option<i64>]) -> (Builder, Vec<RegisteredGroup>) {
    use super::{Frame, FrameSchema, ProtobufRegionStepKind};
    use crate::rhoapi::Par;
    use crate::rust::rhoapi_ext::EPathMap;
    use crate::rust::epathmap_trie_codec::EPathMapMode;
    let make_map = || EPathMap::new_map(
        values.iter().enumerate().map(|(key, value)| (
            par_corpus::gint(key as i64),
            value.map(par_corpus::gint).unwrap_or_default(),
        )),
        Vec::new(), false, None,
    );
    let groups = [
        make_map(),
        EPathMap::from_set_iter([par_corpus::gint(99)], Vec::new(), false, None),
        EPathMap::from_set_iter(std::iter::empty::<Par>(), Vec::new(), false, None),
        make_map(),
    ];
    let mut builder = Builder::new();
    let mut frames = vec![Frame::new(
        ProtobufRegion::from(Vec::new()), FrameSchema::Message(&PAR_PROTOBUF_REGION_PROGRAM),
        None, None, true,
    )];
    let mut observations = Vec::new();
    for (index, map) in groups.into_iter().enumerate() {
        let before = RegisteredTables::capture(&builder);
        let ordinary = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&par_corpus::gint(index as i64)));
        builder.add_child(
            &mut frames, ordinary.clone(), &PAR_PROTOBUF_REGION_PROGRAM,
            1, 1, index, ProtobufRegionStepKind::Message,
            PAR_PROTOBUF_REGION_PROGRAM.type_id, None, true,
        ).unwrap();
        before.assert_preserved(&builder.paths, &builder.cuts, &builder.epathmaps);
        assert_eq!(builder.cuts.len(), before.cuts.len() + 1);
        assert_eq!(builder.paths.len(), before.paths.len() + 1);
        assert_same_region(&ordinary, &builder.cuts[before.cuts.len()].region);
        assert_eq!(builder.cuts[before.cuts.len()].path_leaf, before.paths.len());
        assert!(matches!(frames[0].pieces.last(), Some(Piece::Par { tag: 1, cut }) if *cut == before.cuts.len()));

        let mut expected = Vec::new();
        if map.mode() == EPathMapMode::Map {
            map.for_each_map_entry(|_, value| expected.push(protobuf_encoder::encode_to_vec(value))).unwrap();
        }
        let snapshot = map.trie_snapshot();
        let backing = ProtobufRegion::from([&[99, 42][..], snapshot, &[88][..]].concat());
        let payload = backing.subregion(super::CanonicalByteRange { start: 2, end: 2 + snapshot.len() }).unwrap();
        let before = RegisteredTables::capture(&builder);
        let observed = RegisteredGroup {
            index, first_cut: builder.cuts.len(), first_path: builder.paths.len(),
            payload: payload.clone(), values: expected, mode: map.mode(),
        };
        builder.add_epathmap(
            &mut frames, payload, &PAR_PROTOBUF_REGION_PROGRAM, 1, index, None,
        ).unwrap();
        before.assert_preserved(&builder.paths, &builder.cuts, &builder.epathmaps);
        assert_eq!(builder.cuts.len(), observed.first_cut + observed.values.len());
        assert_eq!(builder.paths.len(), observed.first_path + 1 + observed.values.len());
        assert_eq!(builder.epathmaps.len(), index + 1);
        assert!(matches!(frames[0].pieces.last(), Some(Piece::EPathMap { tag: 1, group }) if *group == index));
        verify_registered_group(&builder, &observed);
        for prior in &observations {
            verify_registered_group(&builder, prior);
        }
        observations.push(observed);
    }
    (builder, observations)
}

#[test]
fn registration_preserves_tables_between_map_set_empty_and_ordinary_cuts() {
    let (builder, observations) = build_registration_sequence(&[None, Some(7), Some(7), Some(9)]);
    assert_eq!(observations.iter().map(|g| g.values.len()).collect::<Vec<_>>(), [4, 0, 0, 4]);
    assert_eq!(builder.cuts.len(), 12);
    for observed in &observations {
        verify_registered_group(&builder, observed);
    }
}

#[test]
fn registration_controls_reject_shifted_ranges_and_wrong_value_paths() {
    let (mut builder, observations) = build_registration_sequence(&[None, Some(7), Some(9)]);
    let observed = &observations[0];
    verify_registered_group(&builder, observed);
    builder.epathmaps[0].cuts.start += 1;
    assert!(std::panic::catch_unwind(|| verify_registered_group(&builder, observed)).is_err());
    builder.epathmaps[0].cuts.start -= 1;
    let left = builder.cuts[observed.first_cut].path_leaf;
    let right = builder.cuts[observed.first_cut + 1].path_leaf;
    builder.paths[left].step.occurrence = 1;
    builder.paths[right].step.occurrence = 0;
    assert!(std::panic::catch_unwind(|| verify_registered_group(&builder, observed)).is_err());
    builder.paths[left].step.occurrence = 0;
    builder.paths[right].step.occurrence = 1;
    let parent = builder.paths[left].parent.take();
    assert!(std::panic::catch_unwind(|| verify_registered_group(&builder, observed)).is_err());
    builder.paths[left].parent = parent;
    verify_registered_group(&builder, observed);
}

proptest! {
    #[test]
    fn generated_registration_retains_every_value_and_prior_group(
        values in prop::collection::vec(prop::option::of(any::<i64>()), 0..24),
    ) {
        build_registration_sequence(&values);
    }
}

fn verify_epathmap_value_frame_partition(
    root: &ProtobufRegion,
    plan: &ProtobufCutPlan,
    observations: &[PublishedNode],
) -> usize {
    use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalWireType};
    let mut frame_count = 0;
    for group in &plan.epathmaps {
        let start = group.prefix.absolute_range().start;
        let mut end = group.prefix.absolute_range().end;
        let mut captured_end = None;
        for observation in observations {
            let base = observation.source.absolute_range().start;
            let mut fields = CanonicalFieldCursor::in_prost_slot_order(observation.source.as_bytes());
            while let Some(field) = fields.next_field().unwrap() {
                if field.wire_type == CanonicalWireType::LengthDelimited
                    && base + field.payload_range.start == start
                {
                    assert!(captured_end.replace(base + field.payload_range.end).is_none());
                }
            }
        }
        let mut rebuilt = group.prefix.as_bytes().to_vec();
        for cut in &plan.cuts[group.cuts.clone()] {
            frame_count += 1;
            let body = cut.region.as_bytes();
            let mut header = Vec::new();
            prost::encoding::encode_varint(body.len() as u64, &mut header);
            assert_eq!(end + header.len(), cut.region.absolute_range().start);
            assert_eq!(header, &root.as_bytes()[end..end + header.len()]);
            rebuilt.extend_from_slice(&header);
            rebuilt.extend_from_slice(body);
            end = cut.region.absolute_range().end;
        }
        assert_eq!(Some(end), captured_end);
        assert_eq!(rebuilt, &root.as_bytes()[start..end]);
    }
    frame_count
}

thread_local! {
    static PUBLISHED: RefCell<Option<Vec<PublishedNode>>> = const { RefCell::new(None) };
}

pub(super) fn record_publication(
    index: usize,
    source: &ProtobufRegion,
    schema: super::FrameSchema,
    path_leaf: Option<usize>,
    builder: &Builder,
) {
    PUBLISHED.with(|capture| {
        let mut capture = capture.borrow_mut();
        let Some(observations) = capture.as_mut() else {
            return;
        };
        assert_eq!(index, observations.len());
        let plan = ProtobufCutPlan {
            nodes: builder.nodes.clone(),
            root: index,
            paths: Vec::new(),
            cuts: builder.cuts.clone(),
            epathmaps: builder.epathmaps.clone(),
            stats: ProtobufCutStats::default(),
        };
        observations.push(PublishedNode {
            source: source.clone(),
            schema,
            path_leaf,
            tables: RegisteredTables::capture(builder),
            output: [
                plan.reconstructed_bytes(),
                plan.shallow_bytes(),
                plan.compact_shell_bytes(),
            ],
        });
    });
}

struct Capture;
impl Drop for Capture {
    fn drop(&mut self) {
        PUBLISHED.with(|capture| {
            capture.borrow_mut().take();
        });
    }
}

fn capture_build(
    root: ProtobufRegion,
    schema: &'static RhoMatchProgram,
) -> (
    Result<ProtobufCutPlan, ProtobufRegionError>,
    Vec<PublishedNode>,
) {
    PUBLISHED.with(|capture| {
        assert!(capture.borrow().is_none());
        *capture.borrow_mut() = Some(Vec::new());
    });
    let guard = Capture;
    let result = super::cut_at_par(root, schema);
    let observations = PUBLISHED.with(|capture| capture.borrow_mut().take().unwrap());
    drop(guard);
    (result, observations)
}

fn verify_publication(bytes: Vec<u8>) -> usize {
    verify_publication_with_piece_counts(bytes).0
}


fn verify_node_field_certificates(
    index: usize,
    plan: &ProtobufCutPlan,
    observations: &[PublishedNode],
) -> [usize; 4] {
    use crate::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalWireType};
    use super::{FrameSchema, ProtobufRegionStep, ProtobufRegionStepKind};
    let parent = &observations[index];
    let source = &observations[index].source;
    let base = source.absolute_range().start;
    let mut fields = CanonicalFieldCursor::in_prost_slot_order(source.as_bytes());
    let mut counts = [0; 4];
    let mut end = 0;
    let mut previous_tags = Vec::new();
    for piece in &plan.nodes[index].pieces {
        let field = fields.next_field().unwrap().expect("one source field per piece");
        let occurrence = previous_tags.iter().rev().take_while(|tag| **tag == field.number).count();
        previous_tags.push(field.number);
        let expected_step = match parent.schema {
            FrameSchema::Message(program) => ProtobufRegionStep {
                owner_type_id: program.type_id,
                field_tag: field.number,
                occurrence,
                kind: ProtobufRegionStepKind::Message,
            },
            FrameSchema::MapEntry { owner_type_id, outer_tag, occurrence, .. } => ProtobufRegionStep {
                owner_type_id,
                field_tag: outer_tag,
                occurrence,
                kind: ProtobufRegionStepKind::MapValue,
            },
        };
        let check_path = |leaf: usize, step: ProtobufRegionStep| {
            let path = &plan.paths[leaf];
            assert_eq!(path.parent, parent.path_leaf);
            assert_eq!(path.step, step);
            if let Some(parent) = path.parent {
                assert!(parent < leaf);
            }
        };
        assert_eq!(field.encoded_range.start, end);
        end = field.encoded_range.end;
        let encoded = &source.as_bytes()[field.encoded_range.start..end];
        let payload = &source.as_bytes()[field.payload_range.start..field.payload_range.end];
        let payload_range = base + field.payload_range.start..base + field.payload_range.end;
        if let Piece::Borrowed(region) = piece {
            counts[0] += 1;
            assert_eq!(region.as_bytes(), encoded);
            assert_eq!(region.absolute_range(), base + field.encoded_range.start..base + end);
            continue;
        }
        assert_eq!(field.wire_type, CanonicalWireType::LengthDelimited);
        let (tag, body) = match piece {
            Piece::Child { tag, node, .. } => {
                counts[1] += 1;
                assert!(*node < index);
                let child = &observations[*node].source;
                assert_eq!(child.absolute_range(), payload_range);
                assert_eq!(child.as_bytes().as_ptr(), payload.as_ptr());
                match observations[*node].schema {
                    FrameSchema::MapEntry { owner_type_id, outer_tag, occurrence: entry_ordinal, .. } => {
                        assert_eq!(observations[*node].path_leaf, parent.path_leaf);
                        assert!(matches!(parent.schema, FrameSchema::Message(_)));
                        assert_eq!(owner_type_id, expected_step.owner_type_id);
                        assert_eq!(outer_tag, field.number);
                        assert_eq!(entry_ordinal, occurrence);
                    }
                    FrameSchema::Message(_) => check_path(
                        observations[*node].path_leaf.expect("ordinary descent registers a path"),
                        expected_step,
                    ),
                }
                (*tag, child.as_bytes().to_vec())
            }
            Piece::Par { tag, cut } => {
                counts[2] += 1;
                let cut = &plan.cuts[*cut];
                assert_eq!(cut.region.absolute_range(), payload_range);
                assert_eq!(cut.region.as_bytes().as_ptr(), payload.as_ptr());
                check_path(cut.path_leaf, expected_step);
                (*tag, cut.region.as_bytes().to_vec())
            }
            Piece::EPathMap { tag, group } => {
                counts[3] += 1;
                let group = &plan.epathmaps[*group];
                assert_eq!(group.prefix.absolute_range().start, payload_range.start);
                assert_eq!(group.prefix.as_bytes().as_ptr(), payload.as_ptr());
                check_path(group.path_leaf, ProtobufRegionStep {
                    kind: ProtobufRegionStepKind::EPathMapSnapshot, ..expected_step
                });
                let mut body = group.prefix.as_bytes().to_vec();
                for (ordinal, cut) in plan.cuts[group.cuts.clone()].iter().enumerate() {
                    let path = &plan.paths[cut.path_leaf];
                    assert_eq!(path.parent, Some(group.path_leaf));
                    assert!(group.path_leaf < cut.path_leaf);
                    assert_eq!(path.step, ProtobufRegionStep {
                        occurrence: ordinal,
                        kind: ProtobufRegionStepKind::EPathMapValue,
                        ..expected_step
                    });
                    prost::encoding::encode_varint(cut.region.len() as u64, &mut body);
                    body.extend_from_slice(cut.region.as_bytes());
                }
                (*tag, body)
            }
            Piece::Borrowed(_) => unreachable!(),
        };
        assert_eq!(tag, field.number);
        assert_eq!(body, payload);
        let mut rebuilt = Vec::new();
        prost::encoding::encode_varint(u64::from(tag) << 3 | 2, &mut rebuilt);
        prost::encoding::encode_varint(body.len() as u64, &mut rebuilt);
        rebuilt.extend_from_slice(&body);
        assert_eq!(rebuilt, encoded);
    }
    assert!(fields.next_field().unwrap().is_none());
    assert_eq!(end, source.len());
    counts
}

fn verify_publication_with_piece_counts(bytes: Vec<u8>) -> (usize, [usize; 4]) {
    let root = ProtobufRegion::from(bytes);
    let (result, observations) = capture_build(root.clone(), &PAR_PROTOBUF_REGION_PROGRAM);
    let plan = result.unwrap();
    assert_eq!(plan.root + 1, plan.nodes.len());
    assert_eq!(observations.len(), plan.nodes.len());
    for (index, path) in plan.paths.iter().enumerate() {
        if let Some(parent) = path.parent {
            assert!(parent < index);
        }
    }
    for cut in &plan.cuts {
        assert!(cut.path_leaf < plan.paths.len());
        let range = cut.region.absolute_range();
        assert_eq!(cut.region.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            cut.region.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
    }
    for group in &plan.epathmaps {
        assert!(group.path_leaf < plan.paths.len());
        let range = group.prefix.absolute_range();
        assert_eq!(group.prefix.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            group.prefix.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
    }
    let value_frames = verify_epathmap_value_frame_partition(&root, &plan, &observations);
    let mut piece_counts = [0; 4];
    let mut ranks = Vec::new();
    for (index, (node, observed)) in plan.nodes.iter().zip(&observations).enumerate() {
        observed.tables.assert_preserved(&plan.paths, &plan.cuts, &plan.epathmaps);
        for (total, count) in piece_counts.iter_mut().zip(
            verify_node_field_certificates(index, &plan, &observations)
        ) {
            *total += count;
        }
        let range = observed.source.absolute_range();
        assert_eq!(observed.source.as_bytes(), &root.as_bytes()[range.clone()]);
        assert_eq!(
            observed.source.as_bytes().as_ptr(),
            root.as_bytes()[range.start..].as_ptr()
        );
        let mut rank = 1usize;
        for piece in &node.pieces {
            match piece {
                Piece::Child { node, .. } => {
                    assert!(*node < index);
                    rank += 1 + ranks[*node];
                }
                Piece::Par { cut, .. } => {
                    assert!(*cut < plan.cuts.len());
                    rank += 1;
                }
                Piece::EPathMap { group, .. } => {
                    let group = &plan.epathmaps[*group];
                    assert!(group.cuts.start <= group.cuts.end);
                    assert!(group.cuts.end <= plan.cuts.len());
                    rank += 1;
                }
                Piece::Borrowed(region) => {
                    let range = region.absolute_range();
                    assert_eq!(region.as_bytes(), &root.as_bytes()[range.clone()]);
                    assert_eq!(
                        region.as_bytes().as_ptr(),
                        root.as_bytes()[range.start..].as_ptr()
                    );
                    rank += 1;
                }
            }
        }
        ranks.push(rank);
        assert!(rank > 0);
        let mut retained = plan.clone();
        retained.root = index;
        let after = [
            retained.reconstructed_bytes(),
            retained.shallow_bytes(),
            retained.compact_shell_bytes(),
        ];
        assert_eq!(after, observed.output);
        assert_eq!(after[0].as_ref(), observed.source.as_bytes());
        assert_eq!([after[0].len(), after[1].len(), after[2].len()], [
            node.original_len,
            node.shallow_len,
            node.compact_len
        ]);
        retained.nodes.truncate(index + 1);
        assert_eq!(
            [
                retained.reconstructed_bytes(),
                retained.shallow_bytes(),
                retained.compact_shell_bytes()
            ],
            after
        );
    }
    assert_eq!(plan.reconstructed_bytes().as_ref(), root.as_bytes());
    (value_frames, piece_counts)
}

#[test]
fn captured_frame_provenance_covers_dedicated_and_inherited_paths() {
    let mut ordinary = 0;
    let mut inherited = 0;
    let mut map_values = 0;
    for (_, par) in par_corpus::par_corpus() {
        let root = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&par));
        let (result, observations) = capture_build(root, &PAR_PROTOBUF_REGION_PROGRAM);
        let plan = result.unwrap();
        for (index, observed) in observations.iter().enumerate() {
            verify_node_field_certificates(index, &plan, &observations);
            match observed.schema {
                super::FrameSchema::MapEntry { .. } => inherited += 1,
                super::FrameSchema::Message(_) if observed.path_leaf.is_some() => ordinary += 1,
                _ => {}
            }
        }
        map_values += plan.paths.iter()
            .filter(|path| path.step.kind == super::ProtobufRegionStepKind::MapValue).count();
    }
    assert!(ordinary > 0);
    assert!(inherited > 0);
    assert!(map_values > 0);
}

#[test]
fn captured_frame_provenance_rejects_another_valid_earlier_parent() {
    for (_, par) in par_corpus::par_corpus() {
        let root = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&par));
        let (result, observations) = capture_build(root, &PAR_PROTOBUF_REGION_PROGRAM);
        let plan = result.unwrap();
        for (index, node) in plan.nodes.iter().enumerate() {
            for piece in &node.pieces {
                let Piece::Par { cut, .. } = piece else { continue };
                let leaf = plan.cuts[*cut].path_leaf;
                let actual = plan.paths[leaf].parent;
                let Some(other) = (0..leaf).find(|candidate| Some(*candidate) != actual) else { continue };
                verify_node_field_certificates(index, &plan, &observations);
                let mut mutant = plan.clone();
                mutant.paths[leaf].parent = Some(other);
                assert!(other < leaf && other < mutant.paths.len());
                assert!(std::panic::catch_unwind(|| {
                    verify_node_field_certificates(index, &mutant, &observations);
                }).is_err());
                return;
            }
        }
    }
    panic!("corpus must expose a cut with a distinct valid earlier parent");
}

proptest! {
    #[test]
    fn generated_injection_entries_preserve_inherited_frame_provenance(
        values in prop::collection::vec(any::<i64>(), 0..24),
    ) {
        use crate::rhoapi::{New, Par};
        let injections = values.into_iter().enumerate()
            .map(|(key, value)| (format!("injection-{key}"), par_corpus::gint(value))).collect();
        let mut par = Par::default();
        par.replace_news(vec![New { injections, ..New::default() }]);
        verify_publication(protobuf_encoder::encode_to_vec(&par));
    }
}

#[test]
fn epathmap_publication_frames_include_empty_and_distinct_map_values() {
    use crate::rhoapi::{Expr, Par};
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::rhoapi_ext::EPathMap;
    let map = EPathMap::new_map(
        [
            (par_corpus::gint(1), Par::default()),
            (par_corpus::gint(2), par_corpus::gint(7)),
            (par_corpus::gint(3), par_corpus::gint(9)),
        ],
        Vec::new(), false, None,
    );
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }]);
    assert_eq!(verify_publication(protobuf_encoder::encode_to_vec(&par)), 3);
}

#[test]
fn every_published_node_preserves_source_caches_indices_and_prior_meaning() {
    let corpus = par_corpus::par_corpus();
    assert!(corpus.len() >= 40);
    let mut piece_counts = [0; 4];
    for (_, par) in corpus {
        for (total, count) in piece_counts.iter_mut().zip(
            verify_publication_with_piece_counts(protobuf_encoder::encode_to_vec(&par)).1
        ) {
            *total += count;
        }
    }
    assert!(piece_counts.iter().all(|count| *count > 0), "all four piece kinds must be exercised");
    verify_publication(protobuf_encoder::encode_to_vec(
        &par_corpus::all_par_fields(),
    ));
    let mut unknown = protobuf_encoder::encode_to_vec(&par_corpus::all_par_fields());
    unknown.extend_from_slice(&[0xfa, 0x07, 0x02, 7, 9]);
    verify_publication(unknown);
}

proptest! {
    #[test]
    fn generated_publications_preserve_actual_frame_bytes(values in prop::collection::vec(any::<i64>(), 0..24)) {
        use crate::rhoapi::{EMap, Expr, KeyValuePair, Par};
        use crate::rhoapi::expr::ExprInstance;
        let entries = values.into_iter().enumerate().map(|(key, value)| KeyValuePair {
            key: Some(par_corpus::gint(key as i64)), value: Some(par_corpus::gint(value)),
        }).chain([KeyValuePair::default()]).collect();
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap { kvs: entries, ..EMap::default() })),
        }]);
        verify_publication(protobuf_encoder::encode_to_vec(&par));
    }
}

#[test]
fn late_framing_failure_never_publishes_the_root() {
    let root = ProtobufRegion::from(vec![0x0a, 0, 0xff]);
    let (result, observations) = capture_build(root, &PAR_PROTOBUF_REGION_PROGRAM);
    assert!(matches!(result, Err(ProtobufRegionError::Framing(_))));
    assert_eq!(observations.len(), 1);
    assert!(observations[0].source.is_empty());
}


#[test]
fn field_certificate_rejects_swapped_same_tag_par_cuts() {
    let input = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&par_corpus::all_par_fields()));
    let (result, observations) = capture_build(input, &PAR_PROTOBUF_REGION_PROGRAM);
    let plan = result.unwrap();
    let mut mutation = None;
    for (index, node) in plan.nodes.iter().enumerate() {
        for (left, a) in node.pieces.iter().enumerate() {
            for (right, b) in node.pieces.iter().enumerate().skip(left + 1) {
                if let (Piece::Par { tag: a_tag, cut: a_cut }, Piece::Par { tag: b_tag, cut: b_cut }) = (a, b) {
                    if a_tag == b_tag && a_cut != b_cut {
                        mutation = Some((index, left, right));
                        break;
                    }
                }
            }
            if mutation.is_some() { break; }
        }
        if mutation.is_some() { break; }
    }
    let (index, left, right) = mutation.expect("fixture includes distinct same-tag Par cuts");
    verify_node_field_certificates(index, &plan, &observations);
    let mut mutant = plan.clone();
    mutant.nodes[index].pieces.swap(left, right);
    assert!(std::panic::catch_unwind(|| {
        verify_node_field_certificates(index, &mutant, &observations)
    }).is_err());
}

fn widened_delimited_length(tag: u32, body: usize) -> Result<usize, ProtobufRegionError> {
    let key = u64::from(tag) * 8 + 2;
    let varint_len = |value: u64| -> u128 {
        let bits = 64 - value.leading_zeros();
        u128::from(bits.max(1).div_ceil(7))
    };
    u64::try_from(body)
        .ok()
        .and_then(|size| {
            let total = varint_len(key) + varint_len(size) + body as u128;
            usize::try_from(total).ok()
        })
        .ok_or(ProtobufRegionError::SizeOverflow)
}

proptest! {
    #[test]
    fn checked_add_matches_machine_word_arithmetic(left in any::<usize>(), right in any::<usize>()) {
        prop_assert_eq!(checked_add(left, right), left.checked_add(right).ok_or(ProtobufRegionError::SizeOverflow));
    }

    #[test]
    fn delimited_length_matches_widened_arithmetic(tag in 1u32..=536_870_911, body in any::<usize>()) {
        prop_assert_eq!(delimited_len(tag, body), widened_delimited_length(tag, body));
    }
}

#[test]
fn delimited_length_covers_key_and_payload_width_transitions() {
    for tag in [
        1, 15, 16, 2047, 2048, 262143, 262144, 33554431, 33554432, 536870911,
    ] {
        for body in [0, 1, 127, 128, 16383, 16384, usize::MAX - 16, usize::MAX] {
            assert_eq!(
                delimited_len(tag, body),
                widened_delimited_length(tag, body)
            );
        }
    }
}

#[test]
fn overflow_in_each_cached_mode_refuses_finished_node_publication() {
    for mode in [EmitMode::Compact, EmitMode::Shallow, EmitMode::Original] {
        let prior = vec![Node {
            pieces: Vec::new(),
            compact_len: if mode == EmitMode::Compact {
                usize::MAX
            } else {
                0
            },
            shallow_len: if mode == EmitMode::Shallow {
                usize::MAX
            } else {
                0
            },
            original_len: if mode == EmitMode::Original {
                usize::MAX
            } else {
                0
            },
        }];
        let before = (
            prior[0].compact_len,
            prior[0].shallow_len,
            prior[0].original_len,
        );
        let result = finish_node(
            vec![Piece::Child {
                tag: 1,
                node: 0,
                retain_empty_shell: true,
            }],
            &prior,
            &[],
            &[],
        );
        assert!(matches!(result, Err(ProtobufRegionError::SizeOverflow)));
        assert_eq!(prior.len(), 1);
        assert_eq!(
            (
                prior[0].compact_len,
                prior[0].shallow_len,
                prior[0].original_len
            ),
            before
        );
    }
    assert_eq!(
        checked_add(usize::MAX, 1),
        Err(ProtobufRegionError::SizeOverflow)
    );
    assert_eq!(checked_add(usize::MAX, 0), Ok(usize::MAX));
    assert_eq!(
        delimited_len(1, usize::MAX),
        Err(ProtobufRegionError::SizeOverflow)
    );
}
