use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epathmap_event_refinement.rs"]
pub mod epm;
#[cfg(not(rpb3_integrated))]
use epm::admission;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_score_transcript.rs"]
pub mod score;
#[cfg(rpb3_integrated)]
use crate::rpb3_score_transcript as score;

verus! {

pub struct DerivedEpmReturn {
    pub summary: epm::ChildSummary,
    pub admission_summary: admission::AdmissionReturnedKind,
    pub nested_before: Option<epm::EpmFrame>,
    pub nested_after: Option<epm::EpmFrame>,
}

pub enum SourceEpmResolution {
    MapValue { canonical_path_stable: bool },
    RawParEscape { canonical_path_stable: bool },
    NestedEpm1 { mode: epm::EpmMode, entries_stable: bool },
}

pub open spec fn nested_end_frame(before: epm::EpmFrame) -> epm::EpmFrame {
    epm::EpmFrame {
        path: before.path,
        cursor: epm::EpmCursorImage {
            act_plan: before.cursor.act_plan,
            validated_plan: before.cursor.validated_plan,
            at: before.cursor.at,
            declared_value_count: before.cursor.declared_value_count,
            map_values_seen: before.cursor.map_values_seen,
            value_table_offset: before.cursor.value_table_offset,
            done: true,
        },
        wait: before.wait,
        mode: before.mode,
        entries_stable: before.entries_stable,
        last_source: before.last_source,
        snapshot: before.snapshot,
        backing: before.backing,
        trie_start: before.trie_start,
        trie_end: before.trie_end,
        value_table_start: before.value_table_start,
        mode_offset: before.mode_offset,
    }
}

pub open spec fn epm_return_stack_source(
    before: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
) -> bool {
    &&& epm::linked_indexed_sources(before)
    &&& epm_frames.len() == before.admission.frames.len()
    &&& epm_frames.len() > 1
    &&& epm_frames[epm_frames.len() - 2] == Some(before.epm)
    &&& forall|i: int| 0 <= i < epm_frames.len() ==> match #[trigger] epm_frames[i] {
        None => true,
        Some(frame) => before.admission.frames[i].frame_kind == 3
            && frame.snapshot == before.indexed_regions[i]
            && epm::frame_layout_wf(frame),
    }
}

pub open spec fn source_derived_epm_return(
    before: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
) -> Option<DerivedEpmReturn> {
    if !epm_return_stack_source(before, epm_frames) { None } else {
        let child = before.admission.frames.last();
        let parent = before.admission.frames[before.admission.frames.len() - 2];
        match epm_frames.last() {
            None if child.frame_kind == 1 && child.program_type_id == 1 => {
                match admission::concrete_derived_summary(parent, child) {
                    Some(admission_summary) => match admission_summary {
                        admission::AdmissionReturnedKind::Par {
                            canonical_path_stable, ..
                        } => Some(DerivedEpmReturn {
                            summary: epm::ChildSummary::Par { canonical_path_stable },
                            admission_summary,
                            nested_before: None,
                            nested_after: None,
                        }),
                        _ => None,
                    },
                    None => None,
                }
            },
            Some(nested) if child.frame_kind == 3 => Some(DerivedEpmReturn {
                summary: epm::ChildSummary::Epm1 {
                    mode: nested.mode,
                    entries_stable: nested.entries_stable,
                },
                admission_summary: admission::AdmissionReturnedKind::Other,
                nested_before: Some(nested),
                nested_after: Some(nested_end_frame(nested)),
            }),
            _ => None,
        }
    }
}

pub open spec fn source_epm_resolution(
    wait: Option<epm::EpmWait>, summary: epm::ChildSummary,
) -> Option<SourceEpmResolution> {
    match (wait, summary) {
        (
            Some(epm::EpmWait::MapValue { .. }),
            epm::ChildSummary::Par { canonical_path_stable },
        ) => Some(SourceEpmResolution::MapValue { canonical_path_stable }),
        (
            Some(epm::EpmWait::Obligation {
                kind: epm::PathObligationKind::RawParEscape, ..
            }),
            epm::ChildSummary::Par { canonical_path_stable },
        ) => Some(SourceEpmResolution::RawParEscape { canonical_path_stable }),
        (
            Some(epm::EpmWait::Obligation {
                kind: epm::PathObligationKind::NestedEpm1, ..
            }),
            epm::ChildSummary::Epm1 { mode, entries_stable },
        ) => Some(SourceEpmResolution::NestedEpm1 { mode, entries_stable }),
        _ => None,
    }
}

pub open spec fn source_linked_epm_child_return(
    before: epm::LinkedEpmState,
    after: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
) -> bool {
    match source_derived_epm_return(before, epm_frames) {
        None => false,
        Some(derived) => {
            &&& source_epm_resolution(before.epm.wait, derived.summary).is_some()
            &&& epm::linked_epm_child_return(
                before, after, derived.summary, derived.admission_summary,
                derived.nested_before, derived.nested_after,
            )
        },
    }
}

pub proof fn source_epm_return_has_derived_summary(
    before: epm::LinkedEpmState,
    after: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
)
    requires source_linked_epm_child_return(before, after, epm_frames),
    ensures
        source_derived_epm_return(before, epm_frames).is_some(),
        match source_derived_epm_return(before, epm_frames).unwrap() {
            DerivedEpmReturn {
                summary: epm::ChildSummary::Par { canonical_path_stable },
                admission_summary: admission::AdmissionReturnedKind::Par {
                    canonical_path_stable: admitted_stable, ..
                },
                nested_before: None,
                nested_after: None,
            } => canonical_path_stable == admitted_stable
                && admission::admission_raw_par_path_stability(
                    before.admission.frames.last(),
                ) == Some(canonical_path_stable),
            DerivedEpmReturn {
                summary: epm::ChildSummary::Epm1 { mode, entries_stable },
                admission_summary: admission::AdmissionReturnedKind::Other,
                nested_before: Some(nested),
                nested_after: Some(finished),
            } => mode == nested.mode && entries_stable == nested.entries_stable
                && epm::event_step(
                    nested, finished, epm::EpmEvent::End,
                    before.admission.next_obligation_id,
                    epm::EpmAction::Complete { mode, entries_stable },
                ),
            _ => false,
        },
{}

pub proof fn source_epm_map_value_return_pop_shape(
    before: epm::LinkedEpmState,
    after: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
)
    requires
        source_linked_epm_child_return(before, after, epm_frames),
        match before.epm.wait {
            Some(epm::EpmWait::MapValue { .. }) => true,
            _ => false,
        },
    ensures
        before.admission.frames.len() > 1,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 1,
        epm_frames.last().is_none(),
        before.admission.frames[before.admission.frames.len() - 2].frame_kind == 3,
        after.admission.frames.len() + 1 == before.admission.frames.len(),
        after.admission.completed as int == before.admission.completed as int + 1,
        after.admission.pushed == before.admission.pushed,
        after.admission.arena == before.admission.arena,
        after.admission.root_end == before.admission.root_end,
        after.admission.next_obligation_id == before.admission.next_obligation_id,
        after.indexed_regions == before.indexed_regions.drop_last(),
        forall|i: int| 0 <= i < after.admission.frames.len() - 1
            ==> after.admission.frames[i] == before.admission.frames[i],
        after.admission.frames.last().physical_start
            == before.admission.frames[before.admission.frames.len() - 2].physical_start,
        after.admission.frames.last().frame_kind
            == before.admission.frames[before.admission.frames.len() - 2].frame_kind,
        after.admission.frames.last().region_start
            == before.admission.frames[before.admission.frames.len() - 2].region_start,
        after.admission.frames.last().region_end
            == before.admission.frames[before.admission.frames.len() - 2].region_end,
        after.admission.frames.last().source_bytes
            == before.admission.frames[before.admission.frames.len() - 2].source_bytes,
        after.admission.frames.last().cursor
            == before.admission.frames[before.admission.frames.len() - 2].cursor,
        after.admission.frames.last().waiting_id.is_none(),
        after.epm.wait.is_none(),
        after.epm.path == before.epm.path,
        after.epm.cursor == before.epm.cursor,
        match source_derived_epm_return(before, epm_frames) {
            Some(DerivedEpmReturn {
                summary: epm::ChildSummary::Par { canonical_path_stable },
                admission_summary: admission::AdmissionReturnedKind::Par {
                    canonical_path_stable: admitted_stable, ..
                },
                nested_before: None,
                nested_after: None,
            }) => canonical_path_stable == admitted_stable
                && admission::admission_raw_par_path_stability(
                    before.admission.frames.last(),
                ) == Some(canonical_path_stable)
                && after.epm.entries_stable
                    == (before.epm.entries_stable && canonical_path_stable),
            _ => false,
        },
{
    source_epm_return_has_derived_summary(before, after, epm_frames);
    let old_count = before.admission.frames.len();
    assert(old_count > 1);
    let parent_index = old_count - 2;
    assert(after.admission.frames == before.admission.frames.subrange(
        0, old_count - 1,
    ).update(
        parent_index,
        epm::epm_parent_resolved(before.admission.frames[parent_index]),
    ));
    assert(after.admission.frames.len() == old_count - 1);
    assert forall|i: int| 0 <= i < after.admission.frames.len() - 1
        implies after.admission.frames[i] == before.admission.frames[i] by {
        assert(i < parent_index);
    }
    assert(parent_index == after.admission.frames.len() - 1);
    let parent = before.admission.frames[parent_index];
    let next_parent = after.admission.frames[parent_index];
    assert(next_parent == epm::epm_parent_resolved(parent));
    assert(next_parent.physical_start == parent.physical_start);
    assert(next_parent.frame_kind == parent.frame_kind);
    assert(next_parent.region_start == parent.region_start);
    assert(next_parent.region_end == parent.region_end);
    assert(next_parent.source_bytes == parent.source_bytes);
    assert(next_parent.cursor == parent.cursor);
    let derived = source_derived_epm_return(before, epm_frames).unwrap();
    assert(epm::linked_epm_child_return(
        before, after, derived.summary, derived.admission_summary,
        derived.nested_before, derived.nested_after,
    ));
    let returned_id = before.admission.frames.last().return_id.unwrap();
    assert(epm::accept_child(
        before.epm, after.epm, Some(returned_id), derived.summary,
    ));
    match derived {
        DerivedEpmReturn {
            summary: epm::ChildSummary::Par { canonical_path_stable },
            admission_summary: admission::AdmissionReturnedKind::Par {
                canonical_path_stable: admitted_stable, ..
            },
            nested_before: None,
            nested_after: None,
        } => {
            assert(before.admission.frames.last().frame_kind == 1);
            assert(before.admission.frames.last().program_type_id == 1);
            assert(epm_frames.last().is_none());
            assert(after.epm.entries_stable
                == (before.epm.entries_stable && canonical_path_stable));
            assert(canonical_path_stable == admitted_stable);
        },
        _ => assert(false),
    }
}

pub open spec fn program31_score_bytes(
    snapshot: Seq<u8>, connective_used: bool,
) -> Seq<u8> {
    seq![2u8]
        + score::integer_transcript(13)
        + score::bytes_transcript(snapshot)
        + score::integer_transcript(if connective_used { 1 } else { 0 })
        + seq![0u8]
}

pub open spec fn program31_score_append(
    arena: score::ScoreArena,
    snapshot: Seq<u8>,
    connective_used: bool,
) -> Option<(score::ScoreArena, usize)> {
    match score::append_bytes(arena, snapshot) {
        None => None,
        Some((with_snapshot, snapshot_handle)) => match score::append_integer(
            with_snapshot, if connective_used { 1 } else { 0 },
        ) {
            None => None,
            Some((with_flag, flag_handle)) => match score::append_integer(with_flag, 13) {
                None => None,
                Some((with_constructor, constructor_handle)) => score::append_node(
                    with_constructor,
                    seq![constructor_handle, snapshot_handle, flag_handle],
                ),
            },
        },
    }
}

pub proof fn program31_score_append_has_source_transcript(
    arena: score::ScoreArena, snapshot: Seq<u8>, connective_used: bool,
)
    requires score::arena_well_formed(arena),
    ensures match program31_score_append(arena, snapshot, connective_used) {
        Some((after, handle)) => {
            &&& score::arena_well_formed(after)
            &&& (handle as int) < after.entries.len()
            &&& after.entries[handle as int].transcript
                == program31_score_bytes(snapshot, connective_used)
        },
        None => true,
    },
{
    if let Some((with_snapshot, snapshot_handle)) = score::append_bytes(arena, snapshot) {
        score::checked_append_exact(arena, score::ScoreNode::BytesAtom(snapshot));
        if let Some((with_flag, flag_handle)) = score::append_integer(
            with_snapshot, if connective_used { 1 } else { 0 },
        ) {
            score::checked_append_exact(
                with_snapshot,
                score::ScoreNode::Integer(if connective_used { 1 } else { 0 }),
            );
            if let Some((with_constructor, constructor_handle)) =
                score::append_integer(with_flag, 13) {
                score::checked_append_exact(with_flag, score::ScoreNode::Integer(13));
                let children = seq![constructor_handle, snapshot_handle, flag_handle];
                score::checked_append_exact(
                    with_constructor, score::ScoreNode::Node(children),
                );
                if let Some((after, handle)) = score::append_node(with_constructor, children) {
                    assert(after.entries[handle as int].transcript
                        == score::kind_transcript(
                            with_constructor.entries, score::ScoreNode::Node(children),
                        ));
                    assert(snapshot_handle as int == arena.entries.len());
                    assert(flag_handle as int == with_snapshot.entries.len());
                    assert(constructor_handle as int == with_flag.entries.len());
                    assert(snapshot_handle < flag_handle && flag_handle < constructor_handle);
                    assert(with_constructor.entries[constructor_handle as int].transcript
                        == score::integer_transcript(13));
                    assert(with_constructor.entries[snapshot_handle as int].transcript
                        == score::bytes_transcript(snapshot));
                    assert(with_constructor.entries[flag_handle as int].transcript
                        == score::integer_transcript(if connective_used { 1 } else { 0 }));
                    assert(children.drop_first() =~= seq![snapshot_handle, flag_handle]);
                    assert(children.drop_first().drop_first() =~= seq![flag_handle]);
                    assert(children.drop_first().drop_first().drop_first()
                        =~= Seq::<usize>::empty());
                    reveal_with_fuel(score::children_transcript, 4);
                    assert(score::children_transcript(
                        with_constructor.entries, seq![flag_handle],
                    ) == score::integer_transcript(if connective_used { 1 } else { 0 }));
                    assert(score::children_transcript(
                        with_constructor.entries, seq![snapshot_handle, flag_handle],
                    ) == score::bytes_transcript(snapshot)
                        + score::integer_transcript(if connective_used { 1 } else { 0 }));
                    vstd::seq_lib::lemma_concat_associative(
                        score::integer_transcript(13),
                        score::bytes_transcript(snapshot),
                        score::integer_transcript(if connective_used { 1 } else { 0 }),
                    );
                    assert(score::children_transcript(with_constructor.entries, children)
                        == score::integer_transcript(13)
                            + score::bytes_transcript(snapshot)
                            + score::integer_transcript(if connective_used { 1 } else { 0 }));
                    assert(after.entries[handle as int].transcript
                        == program31_score_bytes(snapshot, connective_used));
                }
            }
        }
    }
}

}

fn main() {}
