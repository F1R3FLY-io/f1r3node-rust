//! EPathMap obligation returns are enabled wherever production's accept
//! succeeds.
//!
//! `live_runtime_epm_raw_par_return_step` adds production's fatal pre-checks
//! to the linked child return as guards. This module shows that the guards are
//! not stricter than production: from a prestate that satisfies
//! `epm_raw_par_return_source_ready`, the step exists, and its only successor
//! is the state that `epm_obligation_return_live_after` builds around the
//! parent frame resolved by `epm_obligation_resolved_frame`.
//!
//! Production accepts the return in the obligation arm of the EPathMap frame's
//! `accept`. `PathObligationWait::resolution` turns the child's Par summary
//! into `RawPar { canonical_path_stable }`, and the path's `resolve_obligation`
//! runs `private_preflight` and then `resolve_leaf_obligation` on the focused
//! top. An unstable raw Par bubbles the root segment as unstable; a stable one
//! kills the path (EscapeOfStablePar) and still returns Ok. Before the call,
//! `matches_popped` requires the obligation arm of `before_accept`: the
//! parent's `active_obligation` is the awaited one, awaiting resolution, which
//! also means that the path is not dead (`dead_error`).
//!
//! `epm_raw_par_return_source_ready` has three parts:
//! - production's own success conditions: the guards of the step, which are
//!   the checks above, the child cursor's End, and a derived Par return;
//! - `epm_child_return_ready`: the before-side conjuncts of
//!   `linked_epm_child_return`, model invariants that tie the child, the
//!   parent's admission frame and the parent's EPathMap frame together;
//! - `epm_obligation_parser_ready`: two facts about the parent's parser that
//!   production does not check. The resolved root's completed segment count
//!   fits `usize`, and a RawParEscape obligation is the only frame of the root
//!   path, with no Region owner. Reachable parser states must supply them.
//!
//! Production's `canonical_path_resolutions` counter is not part of the live
//! product. The checked accepts below model its overflow check for nested and
//! raw-Par returns.
//!
//! A NestedEpm1 EPathMap child ends and returns into its EPathMap parent in
//! one driver iteration. `epathmap_terminal_step` ends the child: it runs the
//! path's `finish`, commits the read, traversed and event counters through
//! checked adds, and returns the child's own mode and entries stability. The
//! child's `step_into` returns OtherComplete, `finish_other_complete` runs
//! `process_admission_completion`, and `PathObligationWait::resolution` turns
//! the child's summary into `NestedEpm1 { mode, entries_stable }`.
//! `resolve_leaf_obligation` bubbles a stable Map and kills the path for any
//! other child (NestedEpmNotMap, NestedEpmNotStable), and returns Ok in all
//! three cases. `live_runtime_epm_nested_return_step` performs the child's End
//! itself, so its prestate holds the child before its End.
//!
//! `epm_nested_return_enabled` shows that this step exists wherever
//! `epm_nested_return_source_ready` holds and `epm_terminal_complete_from_source`
//! accepts the production End observation of the child. The observation
//! supplies the End event: `epm_terminal_source_success_is_exact` proves it with
//! admission id 0 on `epm_terminal_end_frame`, which is `nested_end_frame`, and
//! the End arm of `event_step` does not read the admission id. So
//! `epm_nested_return_source_ready` lists the before-side conjuncts of the
//! return without the End (`epm_nested_child_return_ready`), and the parent is
//! resolved with the observed mode and entries stability. The observation is
//! accepted only when the terminal counters do not overflow, so production's
//! checked stats are part of the premise.
//!
//! Live EpmEnd ends an EPathMap child whose parent accepts it in a later step,
//! as a CanonicalPath parent (kind 4) does, and it takes the observation as its
//! argument. `epm_nested_return_terminal_agrees` shows that on one child frame
//! the observation gives Live EpmEnd and the nested return the same after-frame
//! and the same `Complete` action, so one source extraction of the End serves
//! both actions.
//!
//! Production's accept of a nested return resolves the awaited obligation with
//! `resolve_obligation` and then increments `canonical_path_resolutions` with
//! `checked_add(1)`, which fails with SizeOverflow after the reducer call has
//! been applied. `rpb3_epm_nested_accept_checked_stats_refinement` models that
//! accept on a source-shaped prestate, and `epm_nested_accept_source_of` builds
//! it from the nested return's prestate: the parent's EPathMap and admission
//! frames, the child's EPathMap frame before and after its End, the child's
//! admission frame, the child's snapshot as the retained child view, the next
//! obligation id, any counter value, and a reducer call that is applied exactly
//! when the parent passes `epm_obligation_before_accept`.
//! `epm_nested_return_checked_accept_agrees` shows that this accept agrees with
//! every nested return: it resolves the awaited obligation with the mode and
//! entries stability of the step's parser resolution, counts one more
//! resolution below `usize::MAX`, and at `usize::MAX` reports the overflow after
//! the reducer call with the counter unchanged.
//!
//! Production's accept of a raw-Par return takes the resolution from the
//! child's summary, `RawPar { canonical_path_stable }`, resolves the awaited
//! obligation with `resolve_obligation`, and then increments
//! `canonical_path_resolutions` with `checked_add(1)`, which fails with
//! SizeOverflow after the reducer call has been applied. The completion then
//! checks the effect: `matches_effect` requires the stability of the returned
//! Par to equal the stability of the resolution, or the completion fails after
//! the increment. `RawParEpmAcceptSource` and `raw_par_epm_accept_from_source`
//! model that accept in the shape of the checked-stats module's nested accept,
//! and `epm_raw_par_accept_source_of` builds the source from the raw-Par
//! return's prestate: the parent's EPathMap and admission frames, the child's
//! admission frame, the child's indexed region as its body and as the retained
//! child view, the child's Par summary with the child's own stability, any
//! counter value, and a reducer call that is applied exactly when the parent
//! passes `epm_obligation_before_accept`.
//! `epm_raw_par_return_checked_accept_agrees` shows that this accept agrees with
//! every raw-Par return: it resolves the awaited obligation with the stability
//! of the step's parser resolution, passes the effect check, counts one more
//! resolution below `usize::MAX`, and at `usize::MAX` reports the overflow after
//! the reducer call with the counter unchanged.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
    rpb3_epm_nested_accept_checked_stats_refinement as nested_accept,
    rpb3_epm_nested_source_refinement as nested,
    rpb3_epm_obligation_return_live as obligation_return, rpb3_map_frame_refinement as map,
    rpb3_path_parser_refinement as parser, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as score_bridge, rpb3_wire_cursor_source_refinement as wire,
    LiveRuntimeProductState, LiveSourceProductState, ProductScoreState,
};

verus! {

/// The EPathMap path gate that mirrors a parser root gate. The completed
/// segment count is narrowed from `u64` to `usize`.
pub open spec fn epm_gate_image(gate: parser::ParserGate) -> epm::PathGate {
    epm::PathGate {
        dead: gate.dead,
        stack_empty: gate.stack_empty,
        split_complete: gate.split_complete,
        completed_segments: gate.completed_segments as usize,
        first_is_list: gate.first_is_list,
        stable: gate.stable,
        escape_allowed: gate.escape_allowed,
    }
}

/// The parent EPathMap frame after its obligation `id` is resolved with
/// `resolution`: the parser is resolved as `resolve_leaf_obligation` does,
/// the gate is the image of the resolved root gate, and the frame neither
/// waits nor holds a pending obligation. Everything else is kept.
pub open spec fn epm_obligation_resolved_frame(
    parent: epm::EpmFrame,
    id: u64,
    resolution: parser::ParserResolution,
) -> epm::EpmFrame {
    let resolved = parser::resolve_parser_obligation(parent.path.parser, id, resolution);
    epm::EpmFrame {
        path: epm::PathTopology {
            pending: None,
            parser: resolved,
            gate: epm_gate_image(parser::parser_root_gate(resolved)),
            ..parent.path
        },
        wait: None,
        ..parent
    }
}

/// The product after the child returns into its resolved parent. The child's
/// admission frame, map frame, EPathMap entry and indexed region are popped;
/// the parent's admission frame becomes `epm_parent_resolved`, one more frame
/// is completed, and the parent's EPathMap entry becomes `resolved`. The
/// receipts are carried from a provisional state, as
/// `epm_push_source_product_after` does.
pub open spec fn epm_obligation_return_product_after(
    before: ProductScoreState,
    resolved: epm::EpmFrame,
) -> ProductScoreState {
    let admission_before = before.bridge.admission;
    let count = admission_before.frames.len();
    let admission_after = admission::AdmissionState {
        frames: admission_before.frames.subrange(0, count - 1).update(
            count - 2, epm::epm_parent_resolved(admission_before.frames[count - 2]),
        ),
        completed: (admission_before.completed as int + 1) as usize,
        ..admission_before
    };
    let provisional = ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge {
            admission: admission_after,
            transcript: before.bridge.transcript,
        },
        map: map::IntegratedAdmissionState {
            admission: admission_after,
            map_frames: before.map.map_frames.drop_last(),
            transcript: before.map.transcript,
        },
        epm_frames: before.epm_frames.drop_last().update(count - 2, Some(resolved)),
        indexed_regions: before.indexed_regions.drop_last(),
        accepted_epm_sources: before.accepted_epm_sources,
    };
    ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(before, provisional),
        ..provisional
    }
}

/// The live state after the return: the product of
/// `epm_obligation_return_product_after`, the runtime image after the
/// admission step, the same arena, and the child's wire reader and cursor
/// dropped.
pub open spec fn epm_obligation_return_live_after(
    before: LiveRuntimeProductState,
    resolved: epm::EpmFrame,
) -> LiveRuntimeProductState {
    let product = epm_obligation_return_product_after(before.source.product, resolved);
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product: product,
            runtime: runtime::runtime_after_admission_step(
                before.source.runtime, product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire.drop_last(),
        },
        cursors: before.cursors.drop_last(),
    }
}

/// The before-side conjuncts of `linked_epm_child_return` for the return
/// that `source_derived_epm_return` derives from `prior` and `frames`: the
/// admission state is well formed and can count one more completed frame;
/// the parent is an EPathMap frame whose snapshot is its region; the child is
/// finished, well formed, admissible, and its summary agrees with the arena;
/// the child's return id links it to the parent's admission wait and to the
/// parent's obligation wait, whose pending obligation is ready; the child's
/// summary is a valid resolution of that obligation; and the parent's gate
/// matches its parser. A nested child also needs its End event.
pub open spec fn epm_child_return_ready(
    prior: epm::LinkedEpmState,
    frames: Seq<Option<epm::EpmFrame>>,
) -> bool {
    match nested::source_derived_epm_return(prior, frames) {
        Some(derived) => {
            let count = prior.admission.frames.len();
            let parent = prior.admission.frames[count - 2];
            let child = prior.admission.frames.last();
            &&& admission::admission_state_wf(prior.admission)
            &&& prior.admission.completed < usize::MAX
            &&& prior.epm.snapshot == prior.indexed_regions[prior.indexed_regions.len() - 2]
            &&& parent.frame_kind == 3
            &&& child.cursor == child.region_end
            &&& child.waiting_id.is_none()
            &&& child.nested_started == child.nested_resolved
            &&& admission::admission_frame_wf(child)
            &&& admission::admission_family_admissible(child)
            &&& admission::admission_child_summary_agrees(
                child, derived.admission_summary, prior.admission.arena,
            )
            &&& match (
                derived.summary, derived.admission_summary,
                derived.nested_before, derived.nested_after,
            ) {
                (
                    epm::ChildSummary::Par { .. },
                    admission::AdmissionReturnedKind::Par { .. },
                    None,
                    None,
                ) => child.frame_kind == 1,
                (
                    epm::ChildSummary::Epm1 { mode, entries_stable },
                    admission::AdmissionReturnedKind::Other,
                    Some(child_before),
                    Some(child_after),
                ) => {
                    &&& child.frame_kind == 3
                    &&& child_before.snapshot == prior.indexed_regions.last()
                    &&& epm::event_step(
                        child_before, child_after, epm::EpmEvent::End,
                        prior.admission.next_obligation_id,
                        epm::EpmAction::Complete { mode, entries_stable },
                    )
                    &&& child_after.cursor.done
                },
                _ => false,
            }
            &&& match child.return_id {
                Some(return_id) => {
                    &&& parent.waiting_id == Some(return_id)
                    &&& parent.waiting_kind == child.frame_kind
                    &&& (child.frame_kind != 3 || parent.nested_resolved < u64::MAX)
                    &&& match prior.epm.wait {
                        Some(epm::EpmWait::Obligation { id, kind, return_id: expected_id }) => {
                            &&& expected_id == return_id
                            &&& epm::valid_child_resolution(kind, derived.summary)
                            &&& match prior.epm.path.pending {
                                Some(obligation) => {
                                    &&& obligation.id == id
                                    &&& obligation.kind == kind
                                    &&& epm::obligation_ready(prior.epm.path)
                                },
                                None => false,
                            }
                            &&& epm::parser_gate_matches(
                                prior.epm.path.gate, prior.epm.path.parser,
                            )
                        },
                        _ => false,
                    }
                },
                None => false,
            }
        },
        None => false,
    }
}

/// Premises on the parent's parser that production does not check. The
/// resolved root's completed segment count fits `usize`, so the gate image
/// keeps it. A RawParEscape obligation is the only frame of the root path,
/// with no Region owner, so an unstable resolution reaches the root.
pub open spec fn epm_obligation_parser_ready(
    parent: epm::EpmFrame,
    kind: epm::PathObligationKind,
    summary: epm::ChildSummary,
) -> bool {
    match parent.wait {
        Some(epm::EpmWait::Obligation { id, .. }) => {
            let resolved = parser::resolve_parser_obligation(
                parent.path.parser, id, epm::parser_resolution(summary),
            );
            &&& parser::parser_root_gate(resolved).completed_segments as int <= usize::MAX as int
            &&& (kind == epm::PathObligationKind::RawParEscape
                ==> parent.path.parser.owners.len() == 0
                    && parent.path.parser.focus.stack.len() == 1)
        },
        _ => false,
    }
}

/// A prestate at which production accepts a raw-Par return: the live state is
/// well formed with the child's cursor above its parent's, the top frame is
/// the returning Message child (kind 1) with no EPathMap entry, the parent
/// passes production's pre-checks for a RawParEscape wait, the child's cursor
/// reaches End, the derived return is ready, and the parent's parser meets
/// `epm_obligation_parser_ready`.
pub open spec fn epm_raw_par_return_source_ready(before: LiveRuntimeProductState) -> bool {
    let product = before.source.product;
    &&& integrated::live_runtime_product_wf(before)
    &&& before.cursors.len() > 1
    &&& product.bridge.admission.frames.last().frame_kind == 1
    &&& product.epm_frames.last().is_none()
    &&& obligation_return::epm_obligation_parent_ready(
        product, epm::PathObligationKind::RawParEscape,
    )
    &&& match before.cursors.last() {
        Some(cursor) => wire::wire_runtime_next_field(cursor)
            == Some(wire::WireConcreteTransition::End {
                state: wire::wire_runtime_derive_cursor(cursor).unwrap(),
            }),
        None => false,
    }
    &&& match (
        integrated::product_linked_parent(product),
        obligation_return::epm_obligation_derived(product),
    ) {
        (Some(prior), Some(derived)) => {
            &&& epm_child_return_ready(prior, product.epm_frames)
            &&& epm_obligation_parser_ready(
                prior.epm, epm::PathObligationKind::RawParEscape, derived.summary,
            )
        },
        _ => false,
    }
}

/// When the root's completed segment count fits `usize`, the gate image of
/// the root gate matches the parser.
pub proof fn epm_gate_image_matches(image: parser::ParserImage)
    requires
        parser::parser_root_gate(image).completed_segments as int <= usize::MAX as int,
    ensures
        epm::parser_gate_matches(epm_gate_image(parser::parser_root_gate(image)), image),
{
    let gate = parser::parser_root_gate(image);
    assert(gate.completed_segments as usize as int == gate.completed_segments as int);
}

/// An unstable raw-Par resolution of the awaited obligation when it is the
/// only frame of a live root path, as `resolve_leaf_obligation` resolves it:
/// the stack empties, one more segment completes, the root becomes unstable,
/// and the path stays live without a fault.
pub proof fn epm_raw_par_root_resolution(image: parser::ParserImage, id: u64)
    requires
        image.owners.len() == 0,
        image.focus.stack.len() == 1,
        match image.focus.stack.last() {
            parser::ParserFrame::Obligation {
                id: active,
                kind: parser::ParserObligationKind::RawParEscape,
                status: parser::ParserObligationStatus::AwaitingResolution { .. },
                ..
            } => active == id,
            _ => false,
        },
        !parser::parser_root_gate(image).dead,
    ensures
        ({
            let resolved = parser::resolve_parser_obligation(
                image, id, parser::ParserResolution::RawPar { canonical_path_stable: false },
            );
            &&& resolved.owners.len() == 0
            &&& resolved.focus.stack.len() == 0
            &&& resolved.focus.completed_segments
                == (image.focus.completed_segments + 1) as u64
            &&& !resolved.focus.stable
            &&& resolved.focus.status == image.focus.status
            &&& !resolved.fault
            &&& !parser::parser_root_gate(resolved).dead
            &&& parser::parser_root_gate(resolved).stack_empty
            &&& !parser::parser_root_gate(resolved).stable
        }),
{
    let path = image.focus;
    let popped = parser::pop_parser_top(path);
    assert(popped.stack.len() == 0);
    let bubbled = parser::bubble_parser(popped, false);
    assert(bubbled.stack.len() == 0);
    assert(bubbled.completed_segments == (path.completed_segments + 1) as u64);
    assert(!bubbled.stable);
    assert(bubbled.status == path.status);
    let candidate = parser::ParserImage { focus: bubbled, ..image };
    assert(parser::settle_parser_owners(candidate) == candidate);
    assert(parser::resolve_parser_obligation(
        image, id, parser::ParserResolution::RawPar { canonical_path_stable: false },
    ) == candidate);
}

/// A child that the reducer rejects (`child_reducer_dead`) leaves the resolved
/// path dead: every such resolution kills the focused path, and with Region
/// owners the parser records a fault instead.
pub proof fn epm_resolution_dead_settles(
    image: parser::ParserImage,
    id: u64,
    kind: epm::PathObligationKind,
    summary: epm::ChildSummary,
)
    requires
        epm::valid_child_resolution(kind, summary),
        epm::child_reducer_dead(kind, summary),
    ensures
        parser::parser_root_gate(parser::resolve_parser_obligation(
            image, id, epm::parser_resolution(summary),
        )).dead,
{
    let resolution = epm::parser_resolution(summary);
    assert(resolution != parser::ParserResolution::RawPar { canonical_path_stable: false });
    assert(resolution
        != parser::ParserResolution::NestedEpm1 { is_map: true, entries_stable: true });
    let candidate = parser::ParserImage { focus: parser::parser_dead(image.focus), ..image };
    assert(parser::settle_parser_owners(candidate) == candidate);
    let resolved = parser::resolve_parser_obligation(image, id, resolution);
    if image.owners.len() == 0 {
        assert(resolved == candidate);
        assert(parser::parser_root(resolved) == parser::parser_dead(image.focus));
    } else {
        assert(resolved.fault);
    }
}

/// Popping the returned child and resolving its parent, as
/// `linked_epm_child_return` does, keeps `admission_state_wf` and
/// `linked_indexed_sources`.
pub proof fn epm_parent_resolved_pop_wf(prior: epm::LinkedEpmState, next: epm::LinkedEpmState)
    requires
        admission::admission_state_wf(prior.admission),
        epm::linked_indexed_sources(prior),
        prior.admission.frames.len() > 1,
        prior.admission.completed < usize::MAX,
        next.admission == (admission::AdmissionState {
            frames: prior.admission.frames.subrange(0, prior.admission.frames.len() - 1).update(
                prior.admission.frames.len() - 2,
                epm::epm_parent_resolved(
                    prior.admission.frames[prior.admission.frames.len() - 2],
                ),
            ),
            arena: prior.admission.arena,
            root_end: prior.admission.root_end,
            pushed: prior.admission.pushed,
            completed: (prior.admission.completed as int + 1) as usize,
            next_obligation_id: prior.admission.next_obligation_id,
        }),
        next.indexed_regions == prior.indexed_regions.drop_last(),
    ensures
        admission::admission_state_wf(next.admission),
        epm::linked_indexed_sources(next),
{
    let state = prior.admission;
    let popped = next.admission;
    let parent_index = state.frames.len() - 2;
    let parent = state.frames[parent_index];
    let resolved = popped.frames[parent_index];
    assert(popped.frames.len() == state.frames.len() - 1);
    assert(resolved == epm::epm_parent_resolved(parent));
    assert(admission::admission_frame_wf(parent));
    assert(resolved.program_type_id == parent.program_type_id);
    assert(resolved.required == parent.required);
    assert(resolved.slot_count == parent.slot_count);
    assert(resolved.frame_kind == parent.frame_kind);
    assert(resolved.seen == parent.seen);
    assert(resolved.scalar_slots == parent.scalar_slots);
    assert(resolved.scalar_occurrences == parent.scalar_occurrences);
    assert(resolved.returns == parent.returns);
    assert(admission::admission_required_slots_generated(resolved));
    assert(resolved.nested_started as int == resolved.nested_resolved as int);
    assert(admission::admission_frame_wf(resolved));
    assert forall|i: int| 0 <= i < popped.frames.len()
        implies admission::admission_frame_wf(popped.frames[i]) by {
        if i < parent_index {
            assert(popped.frames[i] == state.frames[i]);
        } else {
            assert(i == parent_index);
        }
    }
    assert forall|frame_index: int, return_index: int|
        0 <= frame_index < popped.frames.len()
            && 0 <= return_index < popped.frames[frame_index].returns.len()
        implies popped.frames[frame_index].returns[return_index].score_handle.is_some()
            == popped.frames[frame_index].returns[return_index].score_origin_body_id.is_some()
            && match popped.frames[frame_index].returns[return_index].score_handle {
                Some(handle) => (handle as int) < popped.arena.len()
                    && popped.arena[handle as int].body_id
                        == popped.frames[frame_index].returns[return_index]
                            .score_origin_body_id.unwrap(),
                None => true,
            } by {
        if frame_index < parent_index {
            assert(popped.frames[frame_index] == state.frames[frame_index]);
        } else {
            assert(frame_index == parent_index);
            assert(popped.frames[frame_index].returns == parent.returns);
        }
    }
    assert forall|frame_index: int| 1 <= frame_index < popped.frames.len()
        implies ((popped.frames[frame_index - 1].frame_kind != 1
            && popped.frames[frame_index - 1].frame_kind != 2)
            || #[trigger] admission::admission_child_physical_contained(
                popped.frames[frame_index - 1], popped.frames[frame_index],
            )) by {
        assert(popped.frames[frame_index - 1] == state.frames[frame_index - 1]);
        if state.frames[frame_index - 1].frame_kind == 1
            || state.frames[frame_index - 1].frame_kind == 2
        {
            assert(admission::admission_child_physical_contained(
                state.frames[frame_index - 1], state.frames[frame_index],
            ));
        }
        if frame_index < parent_index {
            assert(popped.frames[frame_index] == state.frames[frame_index]);
        } else {
            assert(frame_index == parent_index);
            assert(resolved.physical_source == parent.physical_source);
            assert(resolved.physical_start == parent.physical_start);
            assert(resolved.region_end == parent.region_end);
            assert(resolved.source_bytes == parent.source_bytes);
        }
    }
    assert(admission::admission_state_wf(popped));
    assert forall|i: int| 0 <= i < popped.frames.len()
        implies epm::indexed_image_wf(next.indexed_regions[i])
            && next.indexed_regions[i].bytes == popped.frames[i].source_bytes
            && epm::indexed_position_at(next.indexed_regions[i], 0).source
                == popped.frames[i].physical_source
            && epm::indexed_position_at(next.indexed_regions[i], 0).offset
                == popped.frames[i].physical_start by {
        assert(next.indexed_regions[i] == prior.indexed_regions[i]);
        if i < parent_index {
            assert(popped.frames[i] == state.frames[i]);
        } else {
            assert(i == parent_index);
        }
    }
    assert(epm::linked_indexed_sources(next));
}

/// Wherever production accepts a raw-Par return, the raw-Par step exists: it
/// holds at the state that `epm_obligation_return_live_after` builds around
/// `epm_obligation_resolved_frame`, and every raw-Par step from the prestate
/// reaches that state.
pub proof fn epm_raw_par_return_enabled(before: LiveRuntimeProductState)
    requires epm_raw_par_return_source_ready(before),
    ensures
        ({
            let product = before.source.product;
            let prior = integrated::product_linked_parent(product).unwrap();
            let derived = obligation_return::epm_obligation_derived(product).unwrap();
            match prior.epm.wait {
                Some(epm::EpmWait::Obligation { id, .. }) => {
                    let after = epm_obligation_return_live_after(
                        before,
                        epm_obligation_resolved_frame(
                            prior.epm, id, epm::parser_resolution(derived.summary),
                        ),
                    );
                    &&& obligation_return::live_runtime_epm_raw_par_return_step(before, after)
                    &&& forall|other: LiveRuntimeProductState|
                        #[trigger] obligation_return::live_runtime_epm_raw_par_return_step(
                            before, other,
                        ) ==> other == after
                },
                _ => false,
            }
        }),
{
    let product = before.source.product;
    let prior = integrated::product_linked_parent(product).unwrap();
    let derived = obligation_return::epm_obligation_derived(product).unwrap();
    let parent = prior.epm;
    if let Some(epm::EpmWait::Obligation { id, kind, return_id: expected_id }) = parent.wait {
        let resolution = epm::parser_resolution(derived.summary);
        let resolved = epm_obligation_resolved_frame(parent, id, resolution);
        let after = epm_obligation_return_live_after(before, resolved);
        let product_after = after.source.product;
        let next = integrated::product_linked_top(product_after).unwrap();
        let child = product.bridge.admission.frames.last();
        let stable = match derived.summary {
            epm::ChildSummary::Par { canonical_path_stable } => canonical_path_stable,
            _ => false,
        };
        assert(derived.summary == epm::ChildSummary::Par { canonical_path_stable: stable });
        assert(derived.nested_before.is_none() && derived.nested_after.is_none());
        assert(kind == epm::PathObligationKind::RawParEscape);
        assert(child.return_id == Some(expected_id));
        let parser_after = parser::resolve_parser_obligation(parent.path.parser, id, resolution);
        epm_gate_image_matches(parser_after);
        if stable {
            epm_resolution_dead_settles(parent.path.parser, id, kind, derived.summary);
        } else {
            epm_raw_par_root_resolution(parent.path.parser, id);
        }
        assert(epm::accept_child(parent, resolved, Some(expected_id), derived.summary));
        assert(next.epm == resolved);
        epm_parent_resolved_pop_wf(prior, next);
        assert(epm::linked_epm_child_return(
            prior, next, derived.summary, derived.admission_summary,
            derived.nested_before, derived.nested_after,
        ));
        assert(integrated::product_epm_source_child_binding(
            product, prior, next, derived.summary, derived.admission_summary, None,
        )) by {
            reveal(integrated::product_epm_source_child_binding);
        }
        assert(integrated::product_epm_child_return_step(
            product, product_after, derived.summary, derived.admission_summary, None,
        ));
        assert(obligation_return::epm_obligation_return_pop(before, after));
        assert(obligation_return::live_runtime_epm_raw_par_return_step(before, after)) by {
            reveal(obligation_return::live_runtime_epm_raw_par_return_step);
        }
        assert forall|other: LiveRuntimeProductState|
            #[trigger] obligation_return::live_runtime_epm_raw_par_return_step(before, other)
            implies other == after by {
            obligation_return::live_runtime_epm_raw_par_return_is_functional(
                before, after, other,
            );
        }
    }
}

/// The before-side conjuncts of `linked_epm_child_return` for a nested return,
/// without the child's End: the conjuncts of `epm_child_return_ready` for a
/// derived `Epm1` return, with its End event and its done cursor left out. The
/// production End observation supplies those (`epm_nested_return_enabled`).
pub open spec fn epm_nested_child_return_ready(
    prior: epm::LinkedEpmState,
    frames: Seq<Option<epm::EpmFrame>>,
) -> bool {
    match nested::source_derived_epm_return(prior, frames) {
        Some(derived) => {
            let count = prior.admission.frames.len();
            let parent = prior.admission.frames[count - 2];
            let child = prior.admission.frames.last();
            &&& admission::admission_state_wf(prior.admission)
            &&& prior.admission.completed < usize::MAX
            &&& prior.epm.snapshot == prior.indexed_regions[prior.indexed_regions.len() - 2]
            &&& parent.frame_kind == 3
            &&& child.cursor == child.region_end
            &&& child.waiting_id.is_none()
            &&& child.nested_started == child.nested_resolved
            &&& admission::admission_frame_wf(child)
            &&& admission::admission_family_admissible(child)
            &&& admission::admission_child_summary_agrees(
                child, derived.admission_summary, prior.admission.arena,
            )
            &&& match (
                derived.summary, derived.admission_summary,
                derived.nested_before, derived.nested_after,
            ) {
                (
                    epm::ChildSummary::Epm1 { .. },
                    admission::AdmissionReturnedKind::Other,
                    Some(child_before),
                    Some(_),
                ) => {
                    &&& child.frame_kind == 3
                    &&& child_before.snapshot == prior.indexed_regions.last()
                },
                _ => false,
            }
            &&& match child.return_id {
                Some(return_id) => {
                    &&& parent.waiting_id == Some(return_id)
                    &&& parent.waiting_kind == child.frame_kind
                    &&& (child.frame_kind != 3 || parent.nested_resolved < u64::MAX)
                    &&& match prior.epm.wait {
                        Some(epm::EpmWait::Obligation { id, kind, return_id: expected_id }) => {
                            &&& expected_id == return_id
                            &&& epm::valid_child_resolution(kind, derived.summary)
                            &&& match prior.epm.path.pending {
                                Some(obligation) => {
                                    &&& obligation.id == id
                                    &&& obligation.kind == kind
                                    &&& epm::obligation_ready(prior.epm.path)
                                },
                                None => false,
                            }
                            &&& epm::parser_gate_matches(
                                prior.epm.path.gate, prior.epm.path.parser,
                            )
                        },
                        _ => false,
                    }
                },
                None => false,
            }
        },
        None => false,
    }
}

/// A prestate at which production ends a NestedEpm1 EPathMap child and accepts
/// its return, apart from the child's End: the live state is well formed with
/// the child's cursor above its parent's, the top frame is the returning
/// EPathMap child (kind 3) with its EPathMap entry, the parent passes
/// production's pre-checks for a NestedEpm1 wait, the derived return is ready
/// without the End, and the parent's parser meets `epm_obligation_parser_ready`.
/// The End comes from the production observation that
/// `epm_nested_return_enabled` takes.
pub open spec fn epm_nested_return_source_ready(before: LiveRuntimeProductState) -> bool {
    let product = before.source.product;
    &&& integrated::live_runtime_product_wf(before)
    &&& before.cursors.len() > 1
    &&& product.bridge.admission.frames.last().frame_kind == 3
    &&& product.epm_frames.last().is_some()
    &&& obligation_return::epm_obligation_parent_ready(
        product, epm::PathObligationKind::NestedEpm1,
    )
    &&& match (
        integrated::product_linked_parent(product),
        obligation_return::epm_obligation_derived(product),
    ) {
        (Some(prior), Some(derived)) => {
            &&& epm_nested_child_return_ready(prior, product.epm_frames)
            &&& epm_obligation_parser_ready(
                prior.epm, epm::PathObligationKind::NestedEpm1, derived.summary,
            )
        },
        _ => false,
    }
}

/// The terminal observation's End frame is the nested return's End frame:
/// `epm_terminal_end_frame` and `nested_end_frame` both set the cursor's done
/// flag and keep every other field, field for field.
pub proof fn epm_terminal_end_frame_is_nested_end_frame(frame: epm::EpmFrame)
    ensures terminal::epm_terminal_end_frame(frame) == nested::nested_end_frame(frame),
{
}

/// The End arm of `event_step` does not read the admission id: an End event
/// holds with one admission id exactly when it holds with any other.
/// `epm_terminal_source_success_is_exact` proves the terminal End with id 0, and
/// the nested return needs it with the admission state's next obligation id.
pub proof fn epm_end_admission_id_irrelevant(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    action: epm::EpmAction,
    admission_id: u64,
    other_id: u64,
)
    ensures
        epm::event_step(before, after, epm::EpmEvent::End, admission_id, action)
            == epm::event_step(before, after, epm::EpmEvent::End, other_id, action),
{
}

/// Wherever production ends a NestedEpm1 EPathMap child and accepts its
/// return, the nested return exists. From `epm_nested_return_source_ready` and
/// a production End observation of the child that
/// `epm_terminal_complete_from_source` accepts, the step holds at the state
/// that `epm_obligation_return_live_after` builds around the parent resolved
/// with the observed mode and entries stability, and every nested return from
/// the prestate reaches that state. The observation supplies the child's End
/// event, which completes `epm_child_return_ready`.
pub proof fn epm_nested_return_enabled(
    before: LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires
        epm_nested_return_source_ready(before),
        terminal::epm_terminal_complete_from_source(
            before.source.product.epm_frames.last().unwrap(), observed,
        ).is_some(),
    ensures
        ({
            let product = before.source.product;
            let prior = integrated::product_linked_parent(product).unwrap();
            match prior.epm.wait {
                Some(epm::EpmWait::Obligation { id, .. }) => {
                    let after = epm_obligation_return_live_after(
                        before,
                        epm_obligation_resolved_frame(
                            prior.epm, id,
                            parser::ParserResolution::NestedEpm1 {
                                is_map: observed.mode == epm::EpmMode::Map,
                                entries_stable: observed.entries_stable,
                            },
                        ),
                    );
                    &&& obligation_return::epm_obligation_derived(product).unwrap().summary
                        == epm::ChildSummary::Epm1 {
                            mode: observed.mode,
                            entries_stable: observed.entries_stable,
                        }
                    &&& epm_child_return_ready(prior, product.epm_frames)
                    &&& obligation_return::live_runtime_epm_nested_return_step(before, after)
                    &&& forall|other: LiveRuntimeProductState|
                        #[trigger] obligation_return::live_runtime_epm_nested_return_step(
                            before, other,
                        ) ==> other == after
                },
                _ => false,
            }
        }),
{
    let product = before.source.product;
    let prior = integrated::product_linked_parent(product).unwrap();
    let derived = obligation_return::epm_obligation_derived(product).unwrap();
    let child_epm = product.epm_frames.last().unwrap();
    let parent = prior.epm;
    if let Some(epm::EpmWait::Obligation { id, kind, return_id: expected_id }) = parent.wait {
        let next_id = product.bridge.admission.next_obligation_id;
        terminal::epm_terminal_source_success_is_exact(child_epm, observed);
        let complete = terminal::epm_terminal_complete_from_source(
            child_epm, observed,
        ).unwrap();
        epm_terminal_end_frame_is_nested_end_frame(child_epm);
        epm_end_admission_id_irrelevant(
            child_epm, complete.after, complete.action, 0, next_id,
        );
        let finished = nested::nested_end_frame(child_epm);
        let end_action = epm::EpmAction::Complete {
            mode: child_epm.mode,
            entries_stable: child_epm.entries_stable,
        };
        assert(complete.after == finished);
        assert(complete.action == end_action);
        assert(epm::event_step(child_epm, finished, epm::EpmEvent::End, next_id, end_action));
        assert(derived.summary == epm::ChildSummary::Epm1 {
            mode: child_epm.mode,
            entries_stable: child_epm.entries_stable,
        });
        assert(derived.nested_before == Some(child_epm));
        assert(derived.nested_after == Some(finished));
        assert(epm_child_return_ready(prior, product.epm_frames));
        assert(kind == epm::PathObligationKind::NestedEpm1);
        let resolution = epm::parser_resolution(derived.summary);
        assert(resolution == parser::ParserResolution::NestedEpm1 {
            is_map: observed.mode == epm::EpmMode::Map,
            entries_stable: observed.entries_stable,
        });
        let resolved = epm_obligation_resolved_frame(parent, id, resolution);
        let after = epm_obligation_return_live_after(before, resolved);
        let product_after = after.source.product;
        let next = integrated::product_linked_top(product_after).unwrap();
        let child = product.bridge.admission.frames.last();
        assert(child.return_id == Some(expected_id));
        let parser_after = parser::resolve_parser_obligation(parent.path.parser, id, resolution);
        epm_gate_image_matches(parser_after);
        if epm::child_reducer_dead(kind, derived.summary) {
            epm_resolution_dead_settles(parent.path.parser, id, kind, derived.summary);
        }
        assert(epm::accept_child(parent, resolved, Some(expected_id), derived.summary));
        assert(next.epm == resolved);
        epm_parent_resolved_pop_wf(prior, next);
        assert(epm::linked_epm_child_return(
            prior, next, derived.summary, derived.admission_summary,
            derived.nested_before, derived.nested_after,
        ));
        assert(integrated::product_epm_source_child_binding(
            product, prior, next, derived.summary, derived.admission_summary,
            derived.nested_after,
        )) by {
            reveal(integrated::product_epm_source_child_binding);
        }
        assert(integrated::product_epm_child_return_step(
            product, product_after, derived.summary, derived.admission_summary,
            derived.nested_after,
        ));
        assert(obligation_return::epm_obligation_return_pop(before, after));
        assert(obligation_return::live_runtime_epm_nested_return_step(before, after)) by {
            reveal(obligation_return::live_runtime_epm_nested_return_step);
        }
        assert forall|other: LiveRuntimeProductState|
            #[trigger] obligation_return::live_runtime_epm_nested_return_step(before, other)
            implies other == after by {
            obligation_return::live_runtime_epm_nested_return_is_functional(
                before, after, other,
            );
        }
    }
}

/// Live EpmEnd and the nested return consume the same End observation of the
/// child. Live EpmEnd ends an EPathMap child whose parent accepts it in a later
/// step, as a CanonicalPath parent (kind 4) does; the nested return ends the
/// child of an EPathMap parent (kind 3) inside the return. On the child's
/// frame, an observation that `epm_terminal_complete_from_source` accepts fixes
/// one after-frame and one action: they are the nested return's End frame and
/// the `Complete` action of its derived summary, and every Live EpmEnd step
/// that takes the observation on the same frame leaves that after-frame on top
/// and completes the frame with that action.
pub proof fn epm_nested_return_terminal_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires
        obligation_return::live_runtime_epm_nested_return_step(before, after),
        terminal::epm_terminal_complete_from_source(
            before.source.product.epm_frames.last().unwrap(), observed,
        ).is_some(),
    ensures
        ({
            let product = before.source.product;
            let child_epm = product.epm_frames.last().unwrap();
            let complete = terminal::epm_terminal_complete_from_source(
                child_epm, observed,
            ).unwrap();
            let derived = obligation_return::epm_obligation_derived(product).unwrap();
            &&& derived.nested_before == Some(child_epm)
            &&& derived.nested_after == Some(complete.after)
            &&& derived.summary == epm::ChildSummary::Epm1 {
                mode: observed.mode,
                entries_stable: observed.entries_stable,
            }
            &&& complete.action == epm::EpmAction::Complete {
                mode: observed.mode,
                entries_stable: observed.entries_stable,
            }
            &&& forall|end_before: LiveRuntimeProductState, end_after: LiveRuntimeProductState|
                end_before.source.product.epm_frames.last() == Some(child_epm)
                    && #[trigger] integrated::live_runtime_product_epm_end_source_step(
                        end_before, end_after, observed,
                    )
                    ==> end_after.source.product.epm_frames.last() == derived.nested_after
                    && integrated::product_epm_complete_step(
                        end_before.source.product, end_after.source.product, complete.action,
                    )
        }),
{
    reveal(obligation_return::live_runtime_epm_nested_return_step);
    let product = before.source.product;
    let child_epm = product.epm_frames.last().unwrap();
    let complete = terminal::epm_terminal_complete_from_source(child_epm, observed).unwrap();
    let derived = obligation_return::epm_obligation_derived(product).unwrap();
    epm_terminal_end_frame_is_nested_end_frame(child_epm);
    assert(derived.nested_before == Some(child_epm));
    assert(derived.nested_after == Some(complete.after));
    assert forall|end_before: LiveRuntimeProductState, end_after: LiveRuntimeProductState|
        end_before.source.product.epm_frames.last() == Some(child_epm)
            && #[trigger] integrated::live_runtime_product_epm_end_source_step(
                end_before, end_after, observed,
            )
        implies end_after.source.product.epm_frames.last() == derived.nested_after
            && integrated::product_epm_complete_step(
                end_before.source.product, end_after.source.product, complete.action,
            ) by {
        assert(integrated::product_linked_top(end_before.source.product).unwrap().epm
            == child_epm);
    }
}

/// The source of production's checked nested accept at the prestate of a
/// nested return, with `resolutions_before` as the value of
/// `canonical_path_resolutions`. The parent is the EPathMap frame under the
/// child, with the admission frame under the top one; the child is the top
/// EPathMap frame before its End and `nested_end_frame` of it after, with the
/// top admission frame; the retained child view is the child's snapshot; and
/// the reducer call is applied exactly when the parent passes
/// `epm_obligation_before_accept` for a NestedEpm1 wait.
pub open spec fn epm_nested_accept_source_of(
    before: LiveRuntimeProductState,
    resolutions_before: usize,
) -> nested_accept::NestedEpmAcceptSource {
    let product = before.source.product;
    let frames = product.bridge.admission.frames;
    let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
    let child = product.epm_frames.last().unwrap();
    nested_accept::NestedEpmAcceptSource {
        parent,
        parent_admission: frames[frames.len() - 2],
        child_before_end: child,
        child_after_end: nested::nested_end_frame(child),
        child_admission: frames.last(),
        retained_child: child.snapshot,
        next_obligation_id: product.bridge.admission.next_obligation_id,
        resolutions_before,
        reducer_call: if obligation_return::epm_obligation_before_accept(
            parent, epm::PathObligationKind::NestedEpm1,
        ) {
            nested_accept::NestedReducerCallResult::Applied
        } else {
            nested_accept::NestedReducerCallResult::Failed
        },
    }
}

/// Every nested return agrees with production's checked accept. On the source
/// that `epm_nested_accept_source_of` builds from the step's prestate, for any
/// counter value `resolutions_before`, `nested_epm_accept_from_source` takes
/// the awaited obligation's id and the mode and entries stability of the
/// step's summary, which give the step's parser resolution of the parent.
/// Below `usize::MAX` the outcome is `Resolved` with one more resolution; at
/// `usize::MAX` it is `OverflowAfterReducerApplied` with the counter unchanged.
pub proof fn epm_nested_return_checked_accept_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    resolutions_before: usize,
)
    requires obligation_return::live_runtime_epm_nested_return_step(before, after),
    ensures
        ({
            let product = before.source.product;
            let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
            let outcome = nested_accept::nested_epm_accept_from_source(
                epm_nested_accept_source_of(before, resolutions_before),
            );
            match (
                parent.wait,
                obligation_return::epm_obligation_derived(product).unwrap().summary,
            ) {
                (
                    Some(epm::EpmWait::Obligation { id, .. }),
                    epm::ChildSummary::Epm1 { mode, entries_stable },
                ) => {
                    &&& after.source.product.epm_frames.last().unwrap().path.parser
                        == parser::resolve_parser_obligation(
                            parent.path.parser,
                            id,
                            parser::ParserResolution::NestedEpm1 {
                                is_map: mode == epm::EpmMode::Map,
                                entries_stable,
                            },
                        )
                    &&& (resolutions_before < usize::MAX ==> outcome == Some(
                        nested_accept::NestedEpmAcceptOutcome::Resolved {
                            id,
                            mode,
                            entries_stable,
                            resolutions_after: (resolutions_before + 1) as usize,
                        },
                    ))
                    &&& (resolutions_before == usize::MAX ==> outcome == Some(
                        nested_accept::NestedEpmAcceptOutcome::OverflowAfterReducerApplied {
                            id,
                            mode,
                            entries_stable,
                            resolutions_after: resolutions_before,
                        },
                    ))
                },
                _ => false,
            }
        }),
{
    reveal(obligation_return::live_runtime_epm_nested_return_step);
    let product = before.source.product;
    let product_after = after.source.product;
    let prior = integrated::product_linked_parent(product).unwrap();
    let next = integrated::product_linked_top(product_after).unwrap();
    let derived = obligation_return::epm_obligation_derived(product).unwrap();
    let frames = product.bridge.admission.frames;
    let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
    let child_epm = product.epm_frames.last().unwrap();
    let parent_admission = frames[frames.len() - 2];
    let child = frames.last();
    let source = epm_nested_accept_source_of(before, resolutions_before);
    assert(prior.epm == parent);
    assert(derived.summary == epm::ChildSummary::Epm1 {
        mode: child_epm.mode,
        entries_stable: child_epm.entries_stable,
    });
    assert(derived.nested_after == Some(nested::nested_end_frame(child_epm)));
    assert(epm::linked_epm_child_return(
        prior, next, derived.summary, derived.admission_summary,
        product.epm_frames.last(), derived.nested_after,
    ));
    if let Some(epm::EpmWait::Obligation { id, kind, return_id }) = parent.wait {
        assert(kind == epm::PathObligationKind::NestedEpm1);
        assert(child.return_id == Some(return_id));
        assert(epm::event_step(
            child_epm, nested::nested_end_frame(child_epm), epm::EpmEvent::End,
            product.bridge.admission.next_obligation_id,
            epm::EpmAction::Complete {
                mode: child_epm.mode,
                entries_stable: child_epm.entries_stable,
            },
        ));
        assert(child_epm.snapshot == product.indexed_regions.last());
        assert(parent.snapshot == product.indexed_regions[product.indexed_regions.len() - 2]);
        assert(epm::frame_layout_wf(parent));
        assert(parent_admission.source_bytes == parent.snapshot.bytes);
        assert(child.source_bytes == child_epm.snapshot.bytes);
        assert(child.score_handle.is_none());
        assert(nested_accept::nested_epm_accept_source_ready(source));
        assert(source.reducer_call == nested_accept::NestedReducerCallResult::Applied);
        assert(product_after.epm_frames.last() == Some(next.epm));
    }
}

/// Production's accept of a raw-Par return on a source-shaped prestate: the
/// parent's EPathMap and admission frames, the returning Message child's
/// admission frame and body, the parent's retained child view, the child's
/// summary as the accept takes it, the value of `canonical_path_resolutions`
/// before the accept, and the result of the reducer call, which is the same
/// `resolve_obligation` call as for a nested return.
pub struct RawParEpmAcceptSource {
    pub parent: epm::EpmFrame,
    pub parent_admission: admission::AdmissionFrameState,
    pub child_admission: admission::AdmissionFrameState,
    pub ghost child_body: epm::IndexedRegionImage,
    pub ghost retained_child: epm::IndexedRegionImage,
    pub summary: epm::ChildSummary,
    pub resolutions_before: usize,
    pub reducer_call: nested_accept::NestedReducerCallResult,
}

/// The outcomes of the raw-Par accept, in production's order: the reducer call
/// fails with the counter unchanged; `checked_add(1)` overflows after an
/// applied reducer call, with the counter unchanged; `matches_effect` rejects
/// the effect after the counter was incremented; or the obligation is resolved
/// with one more resolution.
pub enum RawParEpmAcceptOutcome {
    ReducerError { resolutions_after: usize },
    OverflowAfterReducerApplied {
        id: u64,
        canonical_path_stable: bool,
        resolutions_after: usize,
    },
    EffectMismatch { resolutions_after: usize },
    Resolved {
        id: u64,
        canonical_path_stable: bool,
        resolutions_after: usize,
    },
}

/// The checks production makes before the raw-Par accept.
/// `CompleteParentSource::before` requires the retained child view to be the
/// same view as the Message child's body, and the obligation arm of
/// `before_accept` requires the awaited RawParEscape obligation to be pending
/// and awaiting resolution. The child is a finished raw Par (kind 1, program 1)
/// with a score, and its admission frame covers exactly its body.
pub open spec fn raw_par_epm_accept_source_ready(source: RawParEpmAcceptSource) -> bool {
    &&& epm::frame_layout_wf(source.parent)
    &&& epm::indexed_image_wf(source.retained_child)
    &&& source.retained_child == source.child_body
    &&& source.parent_admission.frame_kind == 3
    &&& source.parent_admission.source_bytes == source.parent.snapshot.bytes
    &&& source.child_admission.frame_kind == 1
    &&& source.child_admission.program_type_id == 1
    &&& source.child_admission.score_handle.is_some()
    &&& source.child_admission.cursor == source.child_admission.region_end
    &&& source.child_admission.source_bytes == source.child_body.bytes
    &&& source.child_admission.region_start == 0
    &&& source.child_admission.region_end as int == source.child_body.bytes.len()
    &&& source.child_admission.physical_source
        == epm::indexed_position_at(source.child_body, 0).source
    &&& source.child_admission.physical_start
        == epm::indexed_position_at(source.child_body, 0).offset
    &&& match source.parent.wait {
        Some(epm::EpmWait::Obligation {
            id,
            kind: epm::PathObligationKind::RawParEscape,
            return_id,
        }) => {
            &&& source.parent_admission.waiting_id == Some(return_id)
            &&& source.parent_admission.waiting_kind == 1
            &&& source.child_admission.return_id == Some(return_id)
            &&& match source.parent.path.pending {
                Some(pending) => {
                    &&& pending.id == id
                    &&& pending.kind == epm::PathObligationKind::RawParEscape
                    &&& pending.awaiting_resolution
                },
                None => false,
            }
        },
        _ => false,
    }
}

/// The raw-Par accept in production's order. The resolution comes from the
/// summary: `PathObligationWait::resolution` gives `RawPar { canonical_path_stable }`
/// for a Par summary and nothing for any other summary. A failed reducer call
/// leaves the counter unchanged; after an applied call, `checked_add(1)`
/// overflows at `usize::MAX`. Below it, the effect check accepts the resolution
/// only when the returned Par has the resolution's stability; the completed
/// child frame fixes that stability as `admission_raw_par_path_stability`, the
/// flag `admission_child_summary_agrees` requires of a returned Par summary.
pub open spec fn raw_par_epm_accept_from_source(
    source: RawParEpmAcceptSource,
) -> Option<RawParEpmAcceptOutcome> {
    if !raw_par_epm_accept_source_ready(source) {
        None
    } else {
        match (source.parent.wait, source.summary) {
            (
                Some(epm::EpmWait::Obligation {
                    id,
                    kind: epm::PathObligationKind::RawParEscape,
                    ..
                }),
                epm::ChildSummary::Par { canonical_path_stable },
            ) => match source.reducer_call {
                nested_accept::NestedReducerCallResult::Failed => Some(
                    RawParEpmAcceptOutcome::ReducerError {
                        resolutions_after: source.resolutions_before,
                    },
                ),
                nested_accept::NestedReducerCallResult::Applied => {
                    let returned = admission::admission_raw_par_path_stability(
                        source.child_admission,
                    );
                    if source.resolutions_before == usize::MAX {
                        Some(RawParEpmAcceptOutcome::OverflowAfterReducerApplied {
                            id,
                            canonical_path_stable,
                            resolutions_after: source.resolutions_before,
                        })
                    } else if returned == Some(canonical_path_stable) {
                        Some(RawParEpmAcceptOutcome::Resolved {
                            id,
                            canonical_path_stable,
                            resolutions_after: (source.resolutions_before as int + 1) as usize,
                        })
                    } else {
                        Some(RawParEpmAcceptOutcome::EffectMismatch {
                            resolutions_after: (source.resolutions_before as int + 1) as usize,
                        })
                    }
                },
            },
            _ => None,
        }
    }
}

/// The source of production's checked raw-Par accept at the prestate of a
/// raw-Par return, with `resolutions_before` as the value of
/// `canonical_path_resolutions`. The parent is the EPathMap frame under the
/// child, with the admission frame under the top one; the child is the top
/// admission frame, whose indexed region is both its body and the retained
/// child view; the summary is the child's Par summary with the child's own
/// stability; and the reducer call is applied exactly when the parent passes
/// `epm_obligation_before_accept` for a RawParEscape wait.
pub open spec fn epm_raw_par_accept_source_of(
    before: LiveRuntimeProductState,
    resolutions_before: usize,
) -> RawParEpmAcceptSource {
    let product = before.source.product;
    let frames = product.bridge.admission.frames;
    let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
    let child = frames.last();
    RawParEpmAcceptSource {
        parent,
        parent_admission: frames[frames.len() - 2],
        child_admission: child,
        child_body: product.indexed_regions.last(),
        retained_child: product.indexed_regions.last(),
        summary: epm::ChildSummary::Par {
            canonical_path_stable: admission::admission_raw_par_path_stability(child).unwrap(),
        },
        resolutions_before,
        reducer_call: if obligation_return::epm_obligation_before_accept(
            parent, epm::PathObligationKind::RawParEscape,
        ) {
            nested_accept::NestedReducerCallResult::Applied
        } else {
            nested_accept::NestedReducerCallResult::Failed
        },
    }
}

/// Every raw-Par return agrees with production's checked accept. On the source
/// that `epm_raw_par_accept_source_of` builds from the step's prestate, for any
/// counter value `resolutions_before`, `raw_par_epm_accept_from_source` takes
/// the awaited obligation's id and the stability of the step's summary, which
/// give the step's parser resolution of the parent, and the effect check
/// passes. Below `usize::MAX` the outcome is `Resolved` with one more
/// resolution; at `usize::MAX` it is `OverflowAfterReducerApplied` with the
/// counter unchanged.
pub proof fn epm_raw_par_return_checked_accept_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    resolutions_before: usize,
)
    requires obligation_return::live_runtime_epm_raw_par_return_step(before, after),
    ensures
        ({
            let product = before.source.product;
            let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
            let outcome = raw_par_epm_accept_from_source(
                epm_raw_par_accept_source_of(before, resolutions_before),
            );
            match (
                parent.wait,
                obligation_return::epm_obligation_derived(product).unwrap().summary,
            ) {
                (
                    Some(epm::EpmWait::Obligation { id, .. }),
                    epm::ChildSummary::Par { canonical_path_stable },
                ) => {
                    &&& after.source.product.epm_frames.last().unwrap().path.parser
                        == parser::resolve_parser_obligation(
                            parent.path.parser,
                            id,
                            parser::ParserResolution::RawPar { canonical_path_stable },
                        )
                    &&& (resolutions_before < usize::MAX ==> outcome == Some(
                        RawParEpmAcceptOutcome::Resolved {
                            id,
                            canonical_path_stable,
                            resolutions_after: (resolutions_before + 1) as usize,
                        },
                    ))
                    &&& (resolutions_before == usize::MAX ==> outcome == Some(
                        RawParEpmAcceptOutcome::OverflowAfterReducerApplied {
                            id,
                            canonical_path_stable,
                            resolutions_after: resolutions_before,
                        },
                    ))
                },
                _ => false,
            }
        }),
{
    reveal(obligation_return::live_runtime_epm_raw_par_return_step);
    obligation_return::epm_raw_par_return_resolution_shape(before, after);
    let product = before.source.product;
    let product_after = after.source.product;
    let prior = integrated::product_linked_parent(product).unwrap();
    let next = integrated::product_linked_top(product_after).unwrap();
    let derived = obligation_return::epm_obligation_derived(product).unwrap();
    let frames = product.bridge.admission.frames;
    let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
    let parent_admission = frames[frames.len() - 2];
    let child = frames.last();
    let body = product.indexed_regions.last();
    let source = epm_raw_par_accept_source_of(before, resolutions_before);
    assert(prior.epm == parent);
    assert(epm::linked_epm_child_return(
        prior, next, derived.summary, derived.admission_summary,
        product.epm_frames.last(), None,
    ));
    if let Some(epm::EpmWait::Obligation { id, kind, return_id }) = parent.wait {
        assert(kind == epm::PathObligationKind::RawParEscape);
        assert(child.return_id == Some(return_id));
        assert(parent.snapshot == product.indexed_regions[product.indexed_regions.len() - 2]);
        assert(epm::frame_layout_wf(parent));
        assert(parent_admission.source_bytes == parent.snapshot.bytes);
        assert(child.source_bytes == body.bytes);
        assert(admission::admission_summary_has_score(derived.admission_summary));
        assert(admission::admission_score_body_agrees(child, product.bridge.admission.arena));
        assert(child.score_handle.is_some());
        assert(raw_par_epm_accept_source_ready(source));
        assert(source.reducer_call == nested_accept::NestedReducerCallResult::Applied);
    }
}

}
