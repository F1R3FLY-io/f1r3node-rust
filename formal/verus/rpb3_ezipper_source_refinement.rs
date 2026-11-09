use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_generated_descriptor_projection;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;
pub mod rpb3_path_parser_refinement;
mod rpb3_epathmap_event_refinement;
mod rpb3_epm_source_accept_refinement;
mod rpb3_epm_nested_source_refinement;

use {
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as nested,
    rpb3_epm_source_accept_refinement as source_accept,
    rpb3_generated_descriptor_projection as generated, rpb3_score_admission_bridge as bridge,
    rpb3_score_transcript as score,
};

verus! {

pub open spec fn ezipper_indexed_receipt(
    epm_before: epm::LinkedEpmState,
    scored_before: bridge::ScoreAdmissionBridge,
) -> bool {
    &&& bridge::bridge_well_formed(scored_before)
    &&& epm::linked_indexed_sources(epm_before)
    &&& epm_before.admission.frames.len() >= 4
    &&& epm_before.epm.snapshot == epm_before.indexed_regions.last()
    &&& source_accept::epm_source_finish_accept(epm_before)
        == Some(scored_before.admission)
    &&& epm_before.admission.frames[epm_before.admission.frames.len() - 4]
        .program_type_id == 24
    &&& epm_before.admission.frames[epm_before.admission.frames.len() - 4]
        .family == admission::AdmissionFamily::Expr
    &&& epm_before.admission.frames[epm_before.admission.frames.len() - 3]
        .program_type_id == 32
    &&& epm_before.admission.frames[epm_before.admission.frames.len() - 2]
        .program_type_id == 31
    &&& generated::generated_descriptor(32, 1).is_some()
    &&& match generated::generated_descriptor(32, 5) {
        Some(descriptor) => descriptor.slot == 4
            && descriptor.wire == 0 && descriptor.scalar_kind == 1,
        None => false,
    }
    &&& generated::generated_descriptor(24, 33).is_some()
}

pub open spec fn ezipper_returned_pathmap_source(
    epm_before: epm::LinkedEpmState,
    after_pathmap_pop: bridge::ScoreAdmissionBridge,
) -> bool {
    &&& epm::linked_indexed_sources(epm_before)
    &&& epm_before.admission.frames.len() >= 4
    &&& epm_before.epm.snapshot == epm_before.indexed_regions.last()
    &&& source_accept::epm_source_finish_accept(epm_before).is_some()
    &&& after_pathmap_pop.admission.frames.len() + 2
        == epm_before.admission.frames.len()
    &&& after_pathmap_pop.admission.frames.last().program_type_id == 32
    &&& after_pathmap_pop.admission.frames.last().returns.len() == 1
    &&& after_pathmap_pop.admission.frames.last().returns[0].slot == 0
    &&& after_pathmap_pop.admission.frames.last().returns[0].tag == 1
    &&& after_pathmap_pop.admission.frames.last().returns[0].body_bytes
        == epm_before.admission.frames[epm_before.admission.frames.len() - 2]
            .source_bytes
    &&& after_pathmap_pop.admission.frames.last().returns[0].body_id
        == epm_before.admission.frames[epm_before.admission.frames.len() - 2]
            .body_id
    &&& match after_pathmap_pop.admission.frames.last().returns[0].kind {
        admission::AdmissionReturnedKind::ExprPayload {
            score_present, canonical_fixed_point, ..
        } => score_present && canonical_fixed_point,
        _ => false,
    }
}

pub open spec fn ezipper_pathmap_connective(
    frame: admission::AdmissionFrameState,
) -> Option<bool> {
    if frame.program_type_id != 31 || frame.frame_kind != 1 {
        None
    } else if !frame.seen.contains(1) {
        if admission::admission_scalar_slot_absent(frame, 1) { Some(false) }
        else { None }
    } else {
        match admission::admission_unique_scalar_occurrence(frame, 1, 4, 1) {
            Some(occurrence) if occurrence.varint_value == 1 => Some(true),
            _ => None,
        }
    }
}

pub open spec fn ezipper_connective(
    frame: admission::AdmissionFrameState,
) -> Option<bool> {
    if frame.program_type_id != 32 || frame.frame_kind != 1 {
        None
    } else if !frame.seen.contains(4) {
        if admission::admission_scalar_slot_absent(frame, 4) { Some(false) }
        else { None }
    } else {
        match admission::admission_unique_scalar_occurrence(frame, 4, 5, 1) {
            Some(occurrence) if occurrence.varint_value == 1 => Some(true),
            _ => None,
        }
    }
}

pub open spec fn ezipper_score_bytes(
    snapshot: Seq<u8>, connective_used: bool,
) -> Seq<u8> {
    seq![2u8] + score::integer_transcript(14)
        + score::bytes_transcript(snapshot)
        + score::integer_transcript(if connective_used { 1 } else { 0 })
        + seq![0u8]
}

pub struct EZipperSourceRoute {
    pub after_pathmap_score: bridge::ScoreAdmissionBridge,
    pub after_pathmap_pop: bridge::ScoreAdmissionBridge,
    pub after_zipper_score: bridge::ScoreAdmissionBridge,
    pub after_expr_pop: bridge::ScoreAdmissionBridge,
    pub pathmap_score_handle: usize,
    pub zipper_score_handle: usize,
    pub ghost snapshot_bytes: Seq<u8>,
}

pub open spec fn ezipper_pathmap_score_step(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
) -> Option<(bridge::ScoreAdmissionBridge, usize)> {
    if !ezipper_indexed_receipt(epm_before, before)
        || before.admission.frames.last().score_handle.is_some()
        || admission::admission_epathmap_path_stability(
            before.admission.frames.last(),
        ).is_none()
    {
        None
    } else {
        let snapshot = epm_before.epm.snapshot.bytes;
        match ezipper_pathmap_connective(before.admission.frames.last()) {
            None => None,
            Some(connective_used) => match bridge::bridge_append_unattached(
                before, score::ScoreNode::BytesAtom(snapshot),
            ) {
                None => None,
                Some((with_snapshot, snapshot_handle)) =>
                    match bridge::bridge_append_unattached(
                        with_snapshot,
                        score::ScoreNode::Integer(
                            if connective_used { 1 } else { 0 },
                        ),
                    ) {
                        None => None,
                        Some((with_flag, flag_handle)) =>
                            match bridge::bridge_constructor_step(
                                with_flag, 13,
                                seq![snapshot_handle, flag_handle],
                            ) {
                                None => None,
                                Some((after, _, score_handle)) =>
                                    Some((after, score_handle)),
                            },
                    },
            },
        }
    }
}

pub open spec fn ezipper_pathmap_accept_step(
    epm_before: epm::LinkedEpmState,
    scored: bridge::ScoreAdmissionBridge,
) -> Option<bridge::ScoreAdmissionBridge> {
    if !bridge::bridge_well_formed(scored)
        || scored.admission.frames.len() < 3
        || scored.admission.frames.last().program_type_id != 31
        || scored.admission.frames[scored.admission.frames.len() - 2]
            .program_type_id != 32
        || scored.admission.frames.last().score_handle.is_none()
        || scored.admission.frames.last().return_id.is_none()
        || scored.admission.frames.last().returns.len() == 0
        || scored.admission.frames.last().returns.last().body_bytes
            != epm_before.epm.snapshot.bytes
    {
        None
    } else {
        let child = scored.admission.frames.last();
        match admission::admission_epathmap_path_stability(child) {
            None => None,
            Some(canonical_path_stable) =>
                match admission::admission_complete_child_step(
                    scored.admission, child.return_id.unwrap(), 1,
                    admission::AdmissionReturnedKind::ExprPayload {
                        score_present: true,
                        canonical_fixed_point: true,
                        canonical_path_stable,
                    },
                ) {
                    None => None,
                    Some(after) => Some(bridge::ScoreAdmissionBridge {
                        admission: after,
                        transcript: scored.transcript,
                    }),
                },
        }
    }
}

pub open spec fn ezipper_score_step(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
) -> Option<(bridge::ScoreAdmissionBridge, usize)> {
    if !bridge::bridge_well_formed(before)
        || !ezipper_returned_pathmap_source(epm_before, before)
        || before.admission.frames.len() < 2
        || before.admission.frames.last().program_type_id != 32
        || before.admission.frames.last().frame_kind != 1
        || before.admission.frames.last().score_handle.is_some()
        || before.admission.frames.last().returns.len() != 1
        || before.admission.frames.last().returns[0].slot != 0
        || before.admission.frames.last().returns[0].tag != 1
    {
        None
    } else {
        let snapshot = epm_before.epm.snapshot.bytes;
        match ezipper_connective(before.admission.frames.last()) {
            None => None,
            Some(connective_used) => match bridge::bridge_append_unattached(
                before, score::ScoreNode::BytesAtom(snapshot),
            ) {
                None => None,
                Some((with_snapshot, snapshot_handle)) =>
                    match bridge::bridge_append_unattached(
                        with_snapshot,
                        score::ScoreNode::Integer(
                            if connective_used { 1 } else { 0 },
                        ),
                    ) {
                        None => None,
                        Some((with_flag, flag_handle)) =>
                            match bridge::bridge_constructor_step(
                                with_flag, 14,
                                seq![snapshot_handle, flag_handle],
                            ) {
                                None => None,
                                Some((after, _, score_handle)) =>
                                    Some((after, score_handle)),
                            },
                    },
            },
        }
    }
}

pub open spec fn ezipper_expr_accept_step(
    scored: bridge::ScoreAdmissionBridge,
) -> Option<bridge::ScoreAdmissionBridge> {
    if !bridge::bridge_well_formed(scored)
        || scored.admission.frames.len() < 2
        || scored.admission.frames.last().program_type_id != 32
        || scored.admission.frames.last().score_handle.is_none()
        || scored.admission.frames.last().return_id.is_none()
        || scored.admission.frames[scored.admission.frames.len() - 2]
            .program_type_id != 24
        || scored.admission.frames[scored.admission.frames.len() - 2]
            .waiting_tag != Some(33)
    {
        None
    } else {
        let child = scored.admission.frames.last();
        match admission::admission_complete_child_step(
            scored.admission, child.return_id.unwrap(), 1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: false,
            },
        ) {
            None => None,
            Some(after) => Some(bridge::ScoreAdmissionBridge {
                admission: after,
                transcript: scored.transcript,
            }),
        }
    }
}

pub open spec fn ezipper_complete_source_route(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
) -> Option<EZipperSourceRoute> {
    match ezipper_pathmap_score_step(epm_before, before) {
        None => None,
        Some((after_pathmap_score, pathmap_score_handle)) =>
            match ezipper_pathmap_accept_step(epm_before, after_pathmap_score) {
                None => None,
                Some(after_pathmap_pop) =>
                    match ezipper_score_step(epm_before, after_pathmap_pop) {
                        None => None,
                        Some((after_zipper_score, zipper_score_handle)) =>
                            match ezipper_expr_accept_step(after_zipper_score) {
                                None => None,
                                Some(after_expr_pop) => Some(EZipperSourceRoute {
                                    after_pathmap_score,
                                    after_pathmap_pop,
                                    after_zipper_score,
                                    after_expr_pop,
                                    pathmap_score_handle,
                                    zipper_score_handle,
                                    snapshot_bytes: epm_before.epm.snapshot.bytes,
                                }),
                            },
                    },
            },
    }
}

pub open spec fn ezipper_modeled_empty_snapshot() -> Seq<u8> {
    seq![0x45u8, 0x50u8, 0x4du8, 0x31u8, 1u8, 0u8, 0u8, 0u8]
}

pub open spec fn ezipper_modeled_expr_bytes() -> Seq<u8> {
    seq![0x8au8, 0x02u8, 0x0cu8, 0x0au8, 0x0au8, 0x4au8, 0x08u8]
        + ezipper_modeled_empty_snapshot()
}

pub open spec fn ezipper_modeled_backing(
    start: usize, len: usize,
) -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: ezipper_modeled_expr_bytes(),
        source: 0,
        absolute_start: 0,
        start,
        len,
        end_override: None,
    }
}

pub open spec fn ezipper_modeled_source() -> epm::LinkedEpmState {
    let wire = ezipper_modeled_expr_bytes();
    let snapshot = ezipper_modeled_empty_snapshot();
    let pathmap_bytes = wire.subrange(5, 15);
    let zipper_bytes = wire.subrange(3, 15);
    let root_image = epm::indexed_backing_project(
        ezipper_modeled_backing(0, 15),
    ).unwrap();
    let zipper_image = epm::indexed_backing_project(
        ezipper_modeled_backing(3, 12),
    ).unwrap();
    let pathmap_image = epm::indexed_backing_project(
        ezipper_modeled_backing(5, 10),
    ).unwrap();
    let snapshot_backing = ezipper_modeled_backing(7, 8);
    let snapshot_image = epm::indexed_backing_project(snapshot_backing).unwrap();
    let epm_child = admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 7,
        program_type_id: 0,
        region_start: 0,
        region_end: 8,
        source_bytes: snapshot,
        cursor: 8,
        slot_count: 0,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: Some(2),
        frame_kind: 3,
        body_id: 3,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    };
    let pathmap = admission::AdmissionFrameState {
        physical_start: 5,
        program_type_id: 31,
        region_end: 10,
        source_bytes: pathmap_bytes,
        cursor: 10,
        slot_count: 4,
        last_slot: Some(3),
        seen: Set::empty().insert(3),
        required: Set::empty().insert(3),
        waiting_id: Some(2),
        waiting_kind: 3,
        waiting_slot: Some(3),
        waiting_tag: Some(9),
        return_id: Some(1),
        frame_kind: 1,
        body_id: 2,
        nested_started: 1,
        ..epm_child
    };
    let zipper = admission::AdmissionFrameState {
        physical_start: 3,
        program_type_id: 32,
        region_end: 12,
        source_bytes: zipper_bytes,
        cursor: 12,
        slot_count: 6,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        required: Set::empty(),
        waiting_id: Some(1),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(1),
        return_id: Some(0),
        body_id: 1,
        nested_started: 0,
        ..pathmap
    };
    let expr = admission::AdmissionFrameState {
        physical_start: 0,
        program_type_id: 24,
        region_end: 15,
        source_bytes: wire,
        cursor: 15,
        slot_count: 1,
        family: admission::AdmissionFamily::Expr,
        waiting_id: Some(0),
        waiting_tag: Some(33),
        return_id: None,
        body_id: 0,
        ..zipper
    };
    let admission = admission::AdmissionState {
        frames: seq![expr, zipper, pathmap, epm_child],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 4,
        completed: 0,
        next_obligation_id: 3,
    };
    let prefix = epm::epm_prefix_from_bytes(snapshot).unwrap();
    let initial = epm::initial_epm_candidate(
        snapshot_backing, snapshot_image, prefix, None, None,
    );
    let finished_path = epm::PathTopology { root_left: true, ..initial.path };
    let end_cursor = epm::EpmCursorImage { at: 2, ..initial.cursor };
    let epm_frame = epm::EpmFrame {
        path: finished_path,
        cursor: end_cursor,
        ..initial
    };
    epm::LinkedEpmState {
        epm: epm_frame,
        admission,
        indexed_regions: seq![
            root_image, zipper_image, pathmap_image, snapshot_image,
        ],
    }
}

pub open spec fn ezipper_modeled_before() -> bridge::ScoreAdmissionBridge {
    bridge::ScoreAdmissionBridge {
        admission: source_accept::epm_source_finish_accept(
            ezipper_modeled_source(),
        ).unwrap(),
        transcript: score::empty_arena(),
    }
}

pub struct EZipperAbsentSource {
    pub admission: admission::AdmissionState,
    pub indexed_regions: Seq<epm::IndexedRegionImage>,
}

pub open spec fn ezipper_absent_wire() -> Seq<u8> {
    seq![0x8au8, 0x02u8, 0u8]
}

pub open spec fn ezipper_absent_source() -> EZipperAbsentSource {
    let wire = ezipper_absent_wire();
    let zipper = admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 3,
        program_type_id: 32,
        region_start: 0,
        region_end: 0,
        source_bytes: Seq::empty(),
        cursor: 0,
        slot_count: 6,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: Some(0),
        frame_kind: 1,
        body_id: 1,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    };
    let expr = admission::AdmissionFrameState {
        physical_start: 0,
        program_type_id: 24,
        region_end: 3,
        source_bytes: wire,
        cursor: 3,
        slot_count: 1,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        family: admission::AdmissionFamily::Expr,
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(33),
        return_id: None,
        body_id: 0,
        ..zipper
    };
    let root_image = epm::indexed_backing_project(epm::IndexedBackingImage::Borrowed {
        bytes: wire,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 3,
        end_override: None,
    }).unwrap();
    let zipper_image = epm::indexed_backing_project(epm::IndexedBackingImage::Borrowed {
        bytes: wire,
        source: 0,
        absolute_start: 0,
        start: 3,
        len: 0,
        end_override: None,
    }).unwrap();
    EZipperAbsentSource {
        admission: admission::AdmissionState {
            frames: seq![expr, zipper],
            arena: Seq::empty(),
            root_end: 3,
            pushed: 2,
            completed: 0,
            next_obligation_id: 1,
        },
        indexed_regions: seq![root_image, zipper_image],
    }
}

pub open spec fn ezipper_absent_source_wf(source: EZipperAbsentSource) -> bool {
    &&& admission::admission_state_wf(source.admission)
    &&& source.indexed_regions.len() == source.admission.frames.len()
    &&& forall|i: int| 0 <= i < source.indexed_regions.len() ==> {
        &&& epm::indexed_image_wf(source.indexed_regions[i])
        &&& source.indexed_regions[i].bytes == source.admission.frames[i].source_bytes
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).source
            == source.admission.frames[i].physical_source
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).offset
            == source.admission.frames[i].physical_start
    }
}

pub open spec fn ezipper_absent_pop_step(
    source: EZipperAbsentSource,
) -> Option<admission::AdmissionState> {
    if !ezipper_absent_source_wf(source)
        || source.admission.frames.len() != 2
        || source.admission.frames.last().program_type_id != 32
        || source.admission.frames.last().returns.len() != 0
        || source.admission.frames.last().score_handle.is_some()
    {
        None
    } else {
        admission::admission_complete_child_step(
            source.admission, 0, 1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: false,
                canonical_fixed_point: true,
                canonical_path_stable: false,
            },
        )
    }
}

pub proof fn ezipper_absent_modeled_pop_is_source_bound()
    ensures {
        let source = ezipper_absent_source();
        &&& ezipper_absent_source_wf(source)
        &&& source.indexed_regions[1].bytes.len() == 0
        &&& source.indexed_regions[1].end.source == 0
        &&& source.indexed_regions[1].end.offset == 3
        &&& ezipper_absent_pop_step(source).is_some()
        &&& ezipper_absent_pop_step(source).unwrap().arena.len() == 0
        &&& ezipper_absent_pop_step(source).unwrap().frames.last().returns.last().score_handle.is_none()
        &&& ezipper_absent_pop_step(source).unwrap().frames.last().returns.last().body_bytes.len() == 0
    },
{
    let source = ezipper_absent_source();
    let wire = ezipper_absent_wire();
    assert(wire.len() == 3);
    assert(wire.subrange(3, 3) == Seq::<u8>::empty());
    assert(source.admission.frames.len() == 2);
    assert(source.admission.frames[0].source_bytes == wire);
    assert(source.admission.frames[1].source_bytes == wire.subrange(3, 3));
    assert(source.admission.frames[0].physical_source == source.admission.frames[1].physical_source);
    assert(source.admission.frames[1].source_bytes.len() == 0);
    assert(source.admission.frames[1].physical_start == 3);
    assert(admission::admission_frame_wf(source.admission.frames[0]));
    assert(admission::admission_frame_wf(source.admission.frames[1]));
    assert(admission::admission_child_physical_contained(
        source.admission.frames[0], source.admission.frames[1],
    ));
    assert(admission::admission_state_wf(source.admission));
    assert(source.indexed_regions.len() == 2);
    assert forall|i: int| 0 <= i < source.indexed_regions.len() implies {
        &&& epm::indexed_image_wf(source.indexed_regions[i])
        &&& source.indexed_regions[i].bytes == source.admission.frames[i].source_bytes
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).source
            == source.admission.frames[i].physical_source
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).offset
            == source.admission.frames[i].physical_start
    } by {
        if i == 0 {
            assert(source.indexed_regions[i] == source.indexed_regions[0]);
        } else {
            assert(i == 1);
            assert(source.indexed_regions[i] == source.indexed_regions[1]);
        }
    }
    assert(ezipper_absent_source_wf(source));
    let parent = source.admission.frames[0];
    let child = source.admission.frames[1];
    let summary = admission::AdmissionReturnedKind::ExprPayload {
        score_present: false,
        canonical_fixed_point: true,
        canonical_path_stable: false,
    };
    assert(admission::admission_ezipper_expr_payload(child) == Some(summary));
    assert(admission::admission_child_summary_agrees(child, summary, source.admission.arena));
    assert(admission::admission_arm_child_summary_agrees(parent, child, summary, source.admission.arena));
    assert(admission::admission_complete_child_step(source.admission, 0, 1, summary).is_some());
    assert(ezipper_absent_pop_step(source).is_some());
}

pub proof fn ezipper_absent_modeled_rejects_fabricated_summary()
    ensures {
        let source = ezipper_absent_source();
        let parent = source.admission.frames[0];
        let child = source.admission.frames[1];
        &&& ezipper_absent_pop_step(source).is_some()
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: false,
            }, source.admission.arena,
        )
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: false,
                canonical_fixed_point: false,
                canonical_path_stable: false,
            }, source.admission.arena,
        )
    },
{
    ezipper_absent_modeled_pop_is_source_bound();
    let source = ezipper_absent_source();
    let parent = source.admission.frames[0];
    let child = source.admission.frames[1];
    let correct = admission::AdmissionReturnedKind::ExprPayload {
        score_present: false,
        canonical_fixed_point: true,
        canonical_path_stable: false,
    };
    assert(admission::admission_ezipper_expr_payload(child) == Some(correct));
    assert(admission::admission_expr_structural_payload_with_arena(
        parent, child, source.admission.arena,
    ) == Some(correct));
}

pub proof fn ezipper_modeled_empty_snapshot_score_length()
    ensures
        score::zero_count(ezipper_modeled_empty_snapshot()) == 3,
        score::bytes_transcript(ezipper_modeled_empty_snapshot()).len() == 15,
{
    let bytes = ezipper_modeled_empty_snapshot();
    score::string_and_bytes_transcripts_exact(bytes);
    reveal_with_fuel(score::zero_count, 9);
    assert(score::zero_count(bytes) == 3);
}

pub proof fn ezipper_modeled_receipt_is_source_bound()
    ensures
        admission::admission_state_wf(ezipper_modeled_source().admission),
        epm::linked_indexed_sources(ezipper_modeled_source()),
        source_accept::epm_source_finish_accept(ezipper_modeled_source()).is_some(),
        ezipper_indexed_receipt(
            ezipper_modeled_source(), ezipper_modeled_before(),
        ),
{
    let source = ezipper_modeled_source();
    let wire = ezipper_modeled_expr_bytes();
    let snapshot = ezipper_modeled_empty_snapshot();
    assert(wire.len() == 15);
    assert(snapshot.len() == 8);
    assert(wire.subrange(3, 15).len() == 12);
    assert(wire.subrange(5, 15).len() == 10);
    assert(wire.subrange(7, 15) == snapshot);
    assert(epm::indexed_backing_project(ezipper_modeled_backing(0, 15)).is_some());
    assert(epm::indexed_backing_project(ezipper_modeled_backing(3, 12)).is_some());
    assert(epm::indexed_backing_project(ezipper_modeled_backing(5, 10)).is_some());
    assert(epm::indexed_backing_project(ezipper_modeled_backing(7, 8)).is_some());
    assert(source.admission.frames.len() == 4);
    assert(source.admission.frames[0].program_type_id == 24);
    assert(source.admission.frames[0].family == admission::AdmissionFamily::Expr);
    assert(source.admission.frames[1].program_type_id == 32);
    assert(source.admission.frames[2].program_type_id == 31);
    assert(source.admission.frames[3].frame_kind == 3);
    let pathmap = source.admission.frames[2];
    assert(admission::admission_epathmap_snapshot_descriptor());
    assert forall|slot: int| 0 <= slot < 4 implies
        pathmap.required.contains(slot)
            == (exists|tag: u32|
                admission::admission_required_descriptor_slot(31, tag, slot)) by {
        if slot == 3 {
            assert(admission::admission_required_descriptor_slot(31, 9, 3));
        } else {
            assert(!pathmap.required.contains(slot));
            assert forall|tag: u32|
                !admission::admission_required_descriptor_slot(31, tag, slot) by {
                assert(!admission::admission_required_descriptor_slot(31, tag, slot));
            }
        }
    }
    assert(admission::admission_required_slots_generated(pathmap));
    assert(admission::admission_frame_wf(source.admission.frames[0]));
    assert(admission::admission_frame_wf(source.admission.frames[1]));
    assert(admission::admission_frame_wf(source.admission.frames[2]));
    assert(admission::admission_frame_wf(source.admission.frames[3]));
    wire.lemma_slice_of_slice(3, 15, 2, 12);
    wire.lemma_slice_of_slice(5, 15, 2, 10);
    assert(source.admission.frames[0].source_bytes == wire);
    assert(source.admission.frames[1].source_bytes == wire.subrange(3, 15));
    assert(source.admission.frames[2].source_bytes == wire.subrange(5, 15));
    assert(source.admission.frames[3].source_bytes == wire.subrange(7, 15));
    assert(admission::admission_child_physical_contained(
        source.admission.frames[0], source.admission.frames[1],
    ));
    assert(admission::admission_child_physical_contained(
        source.admission.frames[1], source.admission.frames[2],
    ));
    assert(admission::admission_child_physical_contained(
        source.admission.frames[2], source.admission.frames[3],
    ));
    assert(admission::admission_state_wf(source.admission));
    assert forall|i: int| 0 <= i < source.admission.frames.len() implies {
        &&& epm::indexed_image_wf(source.indexed_regions[i])
        &&& source.indexed_regions[i].bytes
            == source.admission.frames[i].source_bytes
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).source
            == source.admission.frames[i].physical_source
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).offset
            == source.admission.frames[i].physical_start
    } by {
        if 0 <= i && i < source.admission.frames.len() {
            assert(i < 4);
            if i == 0 {
                assert(source.indexed_regions[i] == source.indexed_regions[0]);
            } else if i == 1 {
                assert(source.indexed_regions[i] == source.indexed_regions[1]);
            } else if i == 2 {
                assert(source.indexed_regions[i] == source.indexed_regions[2]);
            } else {
                assert(i == 3);
                assert(source.indexed_regions[i] == source.indexed_regions[3]);
            }
        }
    }
    assert(epm::linked_indexed_sources(source));
    assert(source_accept::epm_source_finish_accept(source).is_some());
    assert(ezipper_indexed_receipt(source, ezipper_modeled_before()));
}

pub proof fn ezipper_modeled_complete_route_is_reachable()
    ensures ezipper_complete_source_route(
        ezipper_modeled_source(), ezipper_modeled_before(),
    ).is_some(),
{
    ezipper_modeled_receipt_is_source_bound();
    let source = ezipper_modeled_source();
    let before = ezipper_modeled_before();
    let snapshot = source.epm.snapshot.bytes;
    ezipper_modeled_empty_snapshot_score_length();
    assert(snapshot == ezipper_modeled_empty_snapshot());
    assert(score::bytes_transcript(snapshot).len() == 15);
    assert(before.transcript == score::empty_arena());
    assert(before.admission.arena.len() == 0);
    assert(bridge::bridge_well_formed(before));
    source_accept::epm_source_finish_accept_preserves_wf(source);
    assert(before.admission == source_accept::epm_source_finish_accept(source).unwrap());
    assert(admission::admission_state_wf(before.admission));
    assert(before.admission.frames.last().score_handle.is_none());
    assert(admission::admission_epathmap_path_stability(
        before.admission.frames.last(),
    ).is_some());
    assert(ezipper_pathmap_connective(before.admission.frames.last()) == Some(false));
    let bytes_kind = score::ScoreNode::BytesAtom(snapshot);
    assert(score::arena_well_formed(before.transcript));
    assert(score::kind_valid(before.transcript.entries, bytes_kind));
    assert(score::checked_append(before.transcript, bytes_kind).is_some());
    assert(bridge::bridge_append_unattached(before, bytes_kind).is_some());
    let (after_bytes, bytes_handle) = bridge::bridge_append_unattached(before, bytes_kind).unwrap();
    bridge::bridge_append_unattached_exact(before, bytes_kind);
    bridge::bridge_append_unattached_preserves_admission_wf(before, bytes_kind);
    let flag_kind = score::ScoreNode::Integer(0i64);
    score::integer_transcript_exact(0i64);
    assert(bridge::bridge_append_unattached(after_bytes, flag_kind).is_some());
    let (after_flag, flag_handle) = bridge::bridge_append_unattached(after_bytes, flag_kind).unwrap();
    bridge::bridge_append_unattached_exact(after_bytes, flag_kind);
    bridge::bridge_append_unattached_preserves_admission_wf(after_bytes, flag_kind);
    assert(bytes_handle == 0);
    assert(flag_handle == 1);
    assert(admission::admission_slots_complete(after_flag.admission.frames.last()));
    assert(admission::admission_family_admissible(after_flag.admission.frames.last()));
    let constructor_kind = score::ScoreNode::Integer(13i64);
    assert(bridge::bridge_append_unattached(after_flag, constructor_kind).is_some());
    let (after_constructor, constructor_handle) =
        bridge::bridge_append_unattached(after_flag, constructor_kind).unwrap();
    bridge::bridge_append_unattached_exact(after_flag, constructor_kind);
    assert(constructor_handle == 2);
    assert(after_constructor.transcript.entries[0].encoded_len == 15);
    assert(after_constructor.transcript.entries[1].encoded_len == 10);
    assert(after_constructor.transcript.entries[2].encoded_len == 10);
    let children = seq![constructor_handle, bytes_handle, flag_handle];
    assert(score::child_handles_valid(after_constructor.transcript.entries, children));
    score::node_transcript_exact(after_constructor.transcript.entries, children);
    assert(children.drop_first() == seq![bytes_handle, flag_handle]);
    assert(seq![bytes_handle, flag_handle].drop_first() == seq![flag_handle]);
    assert(seq![flag_handle].drop_first() == Seq::<usize>::empty());
    assert(score::children_encoded_len(after_constructor.transcript.entries, Seq::<usize>::empty()) == 0);
    assert(score::children_encoded_len(after_constructor.transcript.entries, seq![flag_handle]) == 10);
    assert(score::children_encoded_len(after_constructor.transcript.entries, seq![bytes_handle, flag_handle]) == 25);
    assert(score::children_encoded_len(after_constructor.transcript.entries, children) == 35);
    assert(score::kind_transcript(after_constructor.transcript.entries, score::ScoreNode::Node(children)).len() == 37);
    assert(bridge::bridge_append_unattached(after_constructor, score::ScoreNode::Node(children)).is_some());
    let (after_node, score_handle) = bridge::bridge_append_unattached(
        after_constructor, score::ScoreNode::Node(children),
    ).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle, before.admission.frames.last().body_id,
    ).is_some());
    assert(seq![constructor_handle] + seq![bytes_handle, flag_handle] == children);
    assert(bridge::bridge_constructor_step(after_flag, 13i32, seq![bytes_handle, flag_handle]).is_some());
    bridge::bridge_constructor_exact(after_flag, 13i32, seq![bytes_handle, flag_handle]);
    assert(ezipper_pathmap_score_step(source, before).is_some());
    let after_pathmap_score = ezipper_pathmap_score_step(source, before).unwrap().0;
    assert(bridge::bridge_well_formed(after_pathmap_score));
    assert(admission::admission_state_wf(after_pathmap_score.admission));
    assert(after_pathmap_score.admission.frames.last().score_handle.is_some());
    assert(after_pathmap_score.admission.frames.last().return_id == Some(1));
    assert(after_pathmap_score.admission.frames.last().returns.len() == 1);
    assert(after_pathmap_score.admission.frames.last().returns.last().body_bytes == snapshot);
    let child = after_pathmap_score.admission.frames.last();
    let parent = after_pathmap_score.admission.frames[after_pathmap_score.admission.frames.len() - 2];
    let stable = admission::admission_epathmap_path_stability(child).unwrap();
    let summary = admission::AdmissionReturnedKind::ExprPayload {
        score_present: true, canonical_fixed_point: true, canonical_path_stable: stable,
    };
    assert(admission::admission_ezipper_pathmap_payload(parent, child) == Some(summary));
    assert(admission::admission_child_summary_agrees(child, summary, after_pathmap_score.admission.arena));
    assert(admission::admission_arm_child_summary_agrees(parent, child, summary, after_pathmap_score.admission.arena));
    assert(admission::admission_complete_child_step(after_pathmap_score.admission, 1, 1, summary).is_some());
    assert(ezipper_pathmap_accept_step(source, after_pathmap_score).is_some());
    let after_pathmap_pop = ezipper_pathmap_accept_step(
        source, after_pathmap_score,
    ).unwrap();
    admission::admission_complete_child_preserves_wf(
        after_pathmap_score.admission, 1, 1, summary,
    );
    assert(admission::admission_state_wf(after_pathmap_pop.admission));
    assert(after_pathmap_pop.transcript == after_pathmap_score.transcript);
    assert(after_pathmap_pop.admission.arena == after_pathmap_score.admission.arena);
    assert(bridge::bridge_well_formed(after_pathmap_pop));
    assert(ezipper_returned_pathmap_source(source, after_pathmap_pop));
    assert(after_pathmap_pop.admission.frames.last().score_handle.is_none());
    assert(after_pathmap_pop.admission.frames.last().returns.len() == 1);
    assert(ezipper_connective(after_pathmap_pop.admission.frames.last()) == Some(false));
    let zipper_bytes_kind = score::ScoreNode::BytesAtom(snapshot);
    assert(score::kind_valid(after_pathmap_pop.transcript.entries, zipper_bytes_kind));
    assert(score::checked_append(after_pathmap_pop.transcript, zipper_bytes_kind).is_some());
    assert(bridge::bridge_append_unattached(after_pathmap_pop, zipper_bytes_kind).is_some());
    let (zipper_after_bytes, zipper_bytes_handle) =
        bridge::bridge_append_unattached(after_pathmap_pop, zipper_bytes_kind).unwrap();
    bridge::bridge_append_unattached_exact(after_pathmap_pop, zipper_bytes_kind);
    bridge::bridge_append_unattached_preserves_admission_wf(after_pathmap_pop, zipper_bytes_kind);
    assert(bridge::bridge_append_unattached(zipper_after_bytes, flag_kind).is_some());
    let (zipper_after_flag, zipper_flag_handle) =
        bridge::bridge_append_unattached(zipper_after_bytes, flag_kind).unwrap();
    bridge::bridge_append_unattached_exact(zipper_after_bytes, flag_kind);
    bridge::bridge_append_unattached_preserves_admission_wf(zipper_after_bytes, flag_kind);
    assert(zipper_bytes_handle == 4);
    assert(zipper_flag_handle == 5);
    assert(admission::admission_slots_complete(zipper_after_flag.admission.frames.last()));
    assert(admission::admission_family_admissible(zipper_after_flag.admission.frames.last()));
    let zipper_constructor_kind = score::ScoreNode::Integer(14i64);
    assert(bridge::bridge_append_unattached(zipper_after_flag, zipper_constructor_kind).is_some());
    let (zipper_after_constructor, zipper_constructor_handle) =
        bridge::bridge_append_unattached(zipper_after_flag, zipper_constructor_kind).unwrap();
    bridge::bridge_append_unattached_exact(zipper_after_flag, zipper_constructor_kind);
    assert(zipper_constructor_handle == 6);
    assert(zipper_after_constructor.transcript.entries[4].encoded_len == 15);
    assert(zipper_after_constructor.transcript.entries[5].encoded_len == 10);
    assert(zipper_after_constructor.transcript.entries[6].encoded_len == 10);
    let zipper_children = seq![zipper_constructor_handle, zipper_bytes_handle, zipper_flag_handle];
    assert(score::child_handles_valid(zipper_after_constructor.transcript.entries, zipper_children));
    score::node_transcript_exact(zipper_after_constructor.transcript.entries, zipper_children);
    assert(zipper_children.drop_first() == seq![zipper_bytes_handle, zipper_flag_handle]);
    assert(seq![zipper_bytes_handle, zipper_flag_handle].drop_first() == seq![zipper_flag_handle]);
    assert(seq![zipper_flag_handle].drop_first() == Seq::<usize>::empty());
    assert(score::children_encoded_len(zipper_after_constructor.transcript.entries, Seq::<usize>::empty()) == 0);
    assert(score::children_encoded_len(zipper_after_constructor.transcript.entries, seq![zipper_flag_handle]) == 10);
    assert(score::children_encoded_len(zipper_after_constructor.transcript.entries, seq![zipper_bytes_handle, zipper_flag_handle]) == 25);
    assert(score::children_encoded_len(zipper_after_constructor.transcript.entries, zipper_children) == 35);
    assert(score::kind_transcript(zipper_after_constructor.transcript.entries, score::ScoreNode::Node(zipper_children)).len() == 37);
    assert(bridge::bridge_append_unattached(zipper_after_constructor, score::ScoreNode::Node(zipper_children)).is_some());
    let (zipper_after_node, zipper_score_handle) = bridge::bridge_append_unattached(
        zipper_after_constructor, score::ScoreNode::Node(zipper_children),
    ).unwrap();
    assert(admission::admission_attach_score_step(
        zipper_after_node.admission, zipper_score_handle,
        after_pathmap_pop.admission.frames.last().body_id,
    ).is_some());
    assert(seq![zipper_constructor_handle] + seq![zipper_bytes_handle, zipper_flag_handle] == zipper_children);
    assert(bridge::bridge_constructor_step(
        zipper_after_flag, 14i32, seq![zipper_bytes_handle, zipper_flag_handle],
    ).is_some());
    bridge::bridge_constructor_exact(
        zipper_after_flag, 14i32, seq![zipper_bytes_handle, zipper_flag_handle],
    );
    assert(ezipper_score_step(source, after_pathmap_pop).is_some());
    let after_zipper_score = ezipper_score_step(
        source, after_pathmap_pop,
    ).unwrap().0;
    assert(bridge::bridge_well_formed(after_zipper_score));
    assert(admission::admission_state_wf(after_zipper_score.admission));
    assert(after_zipper_score.admission.frames.last().score_handle.is_some());
    assert(after_zipper_score.admission.frames.last().return_id == Some(0));
    assert(after_zipper_score.admission.frames[after_zipper_score.admission.frames.len() - 2].waiting_tag == Some(33));
    let zipper_child = after_zipper_score.admission.frames.last();
    let expr_parent = after_zipper_score.admission.frames[after_zipper_score.admission.frames.len() - 2];
    let expr_summary = admission::AdmissionReturnedKind::ExprPayload {
        score_present: true, canonical_fixed_point: true, canonical_path_stable: false,
    };
    assert(admission::admission_ezipper_expr_payload(zipper_child) == Some(expr_summary));
    assert(admission::admission_child_summary_agrees(
        zipper_child, expr_summary, after_zipper_score.admission.arena,
    ));
    assert(admission::admission_arm_child_summary_agrees(
        expr_parent, zipper_child, expr_summary, after_zipper_score.admission.arena,
    ));
    assert(admission::admission_complete_child_step(
        after_zipper_score.admission, 0, 1, expr_summary,
    ).is_some());
    assert(ezipper_expr_accept_step(after_zipper_score).is_some());
}

pub proof fn ezipper_pathmap_pop_rejects_false_score_and_fixed_point(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires ezipper_complete_source_route(epm_before, before).is_some(),
    ensures {
        let route = ezipper_complete_source_route(epm_before, before).unwrap();
        let state = route.after_pathmap_score.admission;
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames.last();
        let stable = admission::admission_epathmap_path_stability(child).unwrap();
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: false,
                canonical_fixed_point: true,
                canonical_path_stable: stable,
            }, state.arena,
        )
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: false,
                canonical_path_stable: stable,
            }, state.arena,
        )
    },
{
    let route = ezipper_complete_source_route(epm_before, before).unwrap();
    let state = route.after_pathmap_score.admission;
    let parent = state.frames[state.frames.len() - 2];
    let child = state.frames.last();
    assert(admission::admission_ezipper_pathmap_payload(parent, child).is_some());
}

pub proof fn ezipper_expr_pop_rejects_false_score_and_fixed_point(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires ezipper_complete_source_route(epm_before, before).is_some(),
    ensures {
        let route = ezipper_complete_source_route(epm_before, before).unwrap();
        let state = route.after_zipper_score.admission;
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames.last();
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: false,
                canonical_fixed_point: true,
                canonical_path_stable: false,
            }, state.arena,
        )
        &&& !admission::admission_arm_child_summary_agrees(
            parent, child,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: false,
                canonical_path_stable: false,
            }, state.arena,
        )
    },
{
    let route = ezipper_complete_source_route(epm_before, before).unwrap();
    let state = route.after_zipper_score.admission;
    let parent = state.frames[state.frames.len() - 2];
    let child = state.frames.last();
    let correct = admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: false,
    };
    assert(admission::admission_ezipper_expr_payload(child) == Some(correct));
    assert(parent == before.admission.frames[before.admission.frames.len() - 3]);
    assert(before.admission.frames[before.admission.frames.len() - 3]
        == epm_before.admission.frames[epm_before.admission.frames.len() - 4]);
    assert(parent.family == admission::AdmissionFamily::Expr);
    assert(parent.waiting_slot == Some(0));
    assert(parent.waiting_tag == Some(33));
    assert(admission::admission_expr_structural_payload_with_arena(
        parent, child, state.arena,
    ) == Some(correct));
    assert(!admission::admission_arm_child_summary_agrees(
        parent, child,
        admission::AdmissionReturnedKind::ExprPayload {
            score_present: false,
            canonical_fixed_point: true,
            canonical_path_stable: false,
        }, state.arena,
    ));
    assert(!admission::admission_arm_child_summary_agrees(
        parent, child,
        admission::AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: false,
            canonical_path_stable: false,
        }, state.arena,
    ));
}

pub proof fn ezipper_modeled_route_rejects_fabricated_summaries()
    ensures {
        let source = ezipper_modeled_source();
        let before = ezipper_modeled_before();
        ezipper_complete_source_route(source, before).is_some()
    },
{
    ezipper_modeled_complete_route_is_reachable();
    let source = ezipper_modeled_source();
    let before = ezipper_modeled_before();
    ezipper_pathmap_pop_rejects_false_score_and_fixed_point(source, before);
    ezipper_expr_pop_rejects_false_score_and_fixed_point(source, before);
}

pub proof fn ezipper_receipt_uses_indexed_snapshot(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires ezipper_indexed_receipt(epm_before, before),
    ensures
        before.admission.frames.last().returns.last().body_bytes
            == epm_before.epm.snapshot.bytes,
        before.admission.frames.last().physical_source
            == epm_before.admission.frames[epm_before.admission.frames.len() - 2]
                .physical_source,
        before.admission.frames.last().physical_start
            == epm_before.admission.frames[epm_before.admission.frames.len() - 2]
                .physical_start,
{
    source_accept::epm_source_accept_carries_end_summary(
        epm_before, source_accept::epm_end_linked(epm_before),
        before.admission,
    );
    source_accept::epm_source_finish_accept_preserves_wf(epm_before);
}

pub proof fn ezipper_missing_pathmap_has_no_complete_score(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        before.admission.frames.last().program_type_id == 32,
        before.admission.frames.last().returns.len() == 0,
    ensures ezipper_score_step(epm_before, before).is_none(),
{}

pub proof fn ezipper_unlinked_snapshot_has_no_complete_route(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires
        epm_before.indexed_regions.len() > 0,
        epm_before.epm.snapshot != epm_before.indexed_regions.last(),
    ensures ezipper_complete_source_route(epm_before, before).is_none(),
{}

pub proof fn ezipper_corrupt_receipt_has_no_complete_route(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    requires
        source_accept::epm_source_finish_accept(epm_before).is_some(),
        before.admission.frames.len() > 0,
        before.admission.frames.last().returns.len() > 0,
        before.admission.frames.last().returns.last().body_bytes
            != epm_before.epm.snapshot.bytes,
    ensures ezipper_complete_source_route(epm_before, before).is_none(),
{
    if ezipper_indexed_receipt(epm_before, before) {
        ezipper_receipt_uses_indexed_snapshot(epm_before, before);
        assert(false);
    }
}

pub proof fn ezipper_scored_constructor_transcript_exact(
    after_flag: bridge::ScoreAdmissionBridge,
    constructor: i32,
    snapshot_handle: usize,
    flag_handle: usize,
    snapshot: Seq<u8>,
    flag: bool,
)
    requires
        bridge::bridge_well_formed(after_flag),
        admission::admission_state_wf(after_flag.admission),
        score::handle_transcript(after_flag.transcript, snapshot_handle)
            == Some(score::bytes_transcript(snapshot)),
        score::handle_transcript(after_flag.transcript, flag_handle)
            == Some(score::integer_transcript(if flag { 1 } else { 0 })),
        bridge::bridge_constructor_step(
            after_flag, constructor, seq![snapshot_handle, flag_handle],
        ).is_some(),
    ensures {
        let result = bridge::bridge_constructor_step(
            after_flag, constructor, seq![snapshot_handle, flag_handle],
        ).unwrap();
        result.0.admission.arena[result.2 as int].transcript_bytes
            == seq![2u8] + score::integer_transcript(constructor as i64)
                + score::bytes_transcript(snapshot)
                + score::integer_transcript(if flag { 1 } else { 0 })
                + seq![0u8]
    },
{
    bridge::bridge_constructor_exact(
        after_flag, constructor, seq![snapshot_handle, flag_handle],
    );
    let children = seq![snapshot_handle, flag_handle];
    assert(children.len() == 2);
    assert(children[0] == snapshot_handle);
    assert(children.drop_first() == seq![flag_handle]);
    assert(seq![flag_handle].drop_first() == Seq::<usize>::empty());
    assert(score::children_transcript(
        after_flag.transcript.entries, Seq::<usize>::empty(),
    ) == Seq::<u8>::empty());
    assert(score::children_transcript(
        after_flag.transcript.entries, seq![flag_handle],
    ) == score::integer_transcript(if flag { 1 } else { 0 }));
    assert(score::children_transcript(after_flag.transcript.entries, children)
        == score::bytes_transcript(snapshot)
            + score::integer_transcript(if flag { 1 } else { 0 }));
    assert(bridge::bridge_constructor_transcript(after_flag, constructor, children)
        == seq![2u8] + score::integer_transcript(constructor as i64)
            + score::bytes_transcript(snapshot)
            + score::integer_transcript(if flag { 1 } else { 0 })
            + seq![0u8]);
}

pub proof fn ezipper_complete_route_preserves_source_and_score(
    epm_before: epm::LinkedEpmState,
    before: bridge::ScoreAdmissionBridge,
)
    ensures match ezipper_complete_source_route(epm_before, before) {
        None => true,
        Some(route) => {
            let snapshot = epm_before.epm.snapshot.bytes;
            let pathmap_flag = ezipper_pathmap_connective(
                before.admission.frames.last(),
            ).unwrap();
            let zipper_flag = ezipper_connective(
                route.after_pathmap_pop.admission.frames.last(),
            ).unwrap();
            &&& route.snapshot_bytes == snapshot
            &&& route.after_pathmap_score.admission.arena[
                route.pathmap_score_handle as int
            ].transcript_bytes == nested::program31_score_bytes(
                snapshot, pathmap_flag,
            )
            &&& route.after_zipper_score.admission.arena[
                route.zipper_score_handle as int
            ].transcript_bytes == ezipper_score_bytes(snapshot, zipper_flag)
            &&& route.after_pathmap_pop.admission.frames.last().physical_source
                == before.admission.frames[before.admission.frames.len() - 2]
                    .physical_source
            &&& route.after_pathmap_pop.admission.frames.last().physical_start
                == before.admission.frames[before.admission.frames.len() - 2]
                    .physical_start
            &&& route.after_zipper_score.admission.frames.last().physical_source
                == route.after_pathmap_pop.admission.frames.last().physical_source
            &&& route.after_expr_pop.admission.frames.last().physical_source
                == before.admission.frames[before.admission.frames.len() - 3]
                    .physical_source
            &&& route.after_expr_pop.admission.frames.last().returns.last()
                .score_handle == Some(route.zipper_score_handle)
            &&& route.after_expr_pop.admission.frames.last().returns.last()
                .body_bytes
                == route.after_zipper_score.admission.frames.last().source_bytes
        },
    },
{
    if let Some(route) = ezipper_complete_source_route(epm_before, before) {
        ezipper_receipt_uses_indexed_snapshot(epm_before, before);
        let snapshot = epm_before.epm.snapshot.bytes;
        let pathmap_flag = ezipper_pathmap_connective(
            before.admission.frames.last(),
        ).unwrap();
        let zipper_flag = ezipper_connective(
            route.after_pathmap_pop.admission.frames.last(),
        ).unwrap();
        assert(route.after_pathmap_score.admission.frames.last().score_handle
            == Some(route.pathmap_score_handle));
        assert(route.after_zipper_score.admission.frames.last().score_handle
            == Some(route.zipper_score_handle));
        let pathmap_bytes_kind = score::ScoreNode::BytesAtom(snapshot);
        let (pathmap_after_bytes, pathmap_bytes_handle) =
            bridge::bridge_append_unattached(before, pathmap_bytes_kind).unwrap();
        bridge::bridge_append_unattached_exact(before, pathmap_bytes_kind);
        let pathmap_flag_kind = score::ScoreNode::Integer(if pathmap_flag { 1 } else { 0 });
        let (pathmap_after_flag, pathmap_flag_handle) =
            bridge::bridge_append_unattached(pathmap_after_bytes, pathmap_flag_kind).unwrap();
        bridge::bridge_append_unattached_exact(pathmap_after_bytes, pathmap_flag_kind);
        source_accept::epm_source_finish_accept_preserves_wf(epm_before);
        bridge::bridge_append_unattached_preserves_admission_wf(before, pathmap_bytes_kind);
        bridge::bridge_append_unattached_preserves_admission_wf(pathmap_after_bytes, pathmap_flag_kind);
        assert(score::handle_transcript(pathmap_after_flag.transcript, pathmap_bytes_handle)
            == Some(score::bytes_transcript(snapshot)));
        assert(score::handle_transcript(pathmap_after_flag.transcript, pathmap_flag_handle)
            == Some(score::integer_transcript(if pathmap_flag { 1 } else { 0 })));
        bridge::bridge_constructor_exact(
            pathmap_after_flag, 13,
            seq![pathmap_bytes_handle, pathmap_flag_handle],
        );
        assert(admission::admission_state_wf(route.after_pathmap_score.admission));
        ezipper_scored_constructor_transcript_exact(
            pathmap_after_flag, 13, pathmap_bytes_handle, pathmap_flag_handle,
            snapshot, pathmap_flag,
        );
        assert(route.after_pathmap_score.admission.arena[
            route.pathmap_score_handle as int
        ].transcript_bytes == nested::program31_score_bytes(snapshot, pathmap_flag));
        let zipper_before = route.after_pathmap_pop;
        let zipper_bytes_kind = score::ScoreNode::BytesAtom(snapshot);
        let (zipper_after_bytes, zipper_bytes_handle) =
            bridge::bridge_append_unattached(zipper_before, zipper_bytes_kind).unwrap();
        bridge::bridge_append_unattached_exact(zipper_before, zipper_bytes_kind);
        let zipper_flag_kind = score::ScoreNode::Integer(if zipper_flag { 1 } else { 0 });
        let (zipper_after_flag, zipper_flag_handle) =
            bridge::bridge_append_unattached(zipper_after_bytes, zipper_flag_kind).unwrap();
        bridge::bridge_append_unattached_exact(zipper_after_bytes, zipper_flag_kind);
        admission::admission_complete_child_preserves_wf(
            route.after_pathmap_score.admission,
            route.after_pathmap_score.admission.frames.last().return_id.unwrap(),
            1,
            admission::admission_ezipper_pathmap_payload(
                route.after_pathmap_score.admission.frames[
                    route.after_pathmap_score.admission.frames.len() - 2],
                route.after_pathmap_score.admission.frames.last(),
            ).unwrap(),
        );
        bridge::bridge_append_unattached_preserves_admission_wf(zipper_before, zipper_bytes_kind);
        bridge::bridge_append_unattached_preserves_admission_wf(zipper_after_bytes, zipper_flag_kind);
        assert(score::handle_transcript(zipper_after_flag.transcript, zipper_bytes_handle)
            == Some(score::bytes_transcript(snapshot)));
        assert(score::handle_transcript(zipper_after_flag.transcript, zipper_flag_handle)
            == Some(score::integer_transcript(if zipper_flag { 1 } else { 0 })));
        ezipper_scored_constructor_transcript_exact(
            zipper_after_flag, 14, zipper_bytes_handle, zipper_flag_handle,
            snapshot, zipper_flag,
        );
        assert(route.after_zipper_score.admission.arena[
            route.zipper_score_handle as int
        ].transcript_bytes == ezipper_score_bytes(snapshot, zipper_flag));
    }
}

}

fn main() {}
