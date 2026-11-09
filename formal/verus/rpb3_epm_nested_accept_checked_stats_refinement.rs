//! Source-shaped nested EPathMap obligation acceptance with checked stats.
//!
//! After a nested EPathMap child completes, the driver pops it and calls its
//! suspended EPathMap parent's `accept`. That method takes the waiting state
//! and child view, derives a `NestedEpm1` resolution from the returned child
//! mode/stability, calls `path.resolve_obligation`, and only then increments
//! `canonical_path_resolutions` with `checked_add(1)`. If the addition
//! overflows, the method returns an error after the reducer call succeeded;
//! it does not emit `FrameAcceptEffect::ResolvedObligation` or roll the
//! reducer mutation back.
//!
//! This selector derives the resolution and counter outcome from source-shaped
//! prestate and call result. It does not construct a parent poststate. Rust
//! extraction must still bind the indexed images, retained `same_view`,
//! terminal child event, reducer call result, and actual driver pop.

use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
mod rpb3_epathmap_event_refinement;
#[cfg(not(rpb3_integrated))]
use epm::admission;
#[cfg(not(rpb3_integrated))]
use rpb3_epathmap_event_refinement as epm;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub enum NestedReducerCallResult {
    Failed,
    Applied,
}

pub struct NestedEpmAcceptSource {
    pub parent: epm::EpmFrame,
    pub parent_admission: admission::AdmissionFrameState,
    pub child_before_end: epm::EpmFrame,
    pub child_after_end: epm::EpmFrame,
    pub child_admission: admission::AdmissionFrameState,
    pub ghost retained_child: epm::IndexedRegionImage,
    pub next_obligation_id: u64,
    pub resolutions_before: usize,
    pub reducer_call: NestedReducerCallResult,
}

pub enum NestedEpmAcceptOutcome {
    ReducerError { resolutions_after: usize },
    OverflowAfterReducerApplied {
        id: u64,
        mode: epm::EpmMode,
        entries_stable: bool,
        resolutions_after: usize,
    },
    Resolved {
        id: u64,
        mode: epm::EpmMode,
        entries_stable: bool,
        resolutions_after: usize,
    },
}

/// The production `CompleteParentSource::before` also checks that the
/// retained waiting-child view and the active child's snapshot are the same
/// Rust view. Equality of these indexed images is the modeled counterpart;
/// deriving it from concrete `same_view` is still a Rust extraction task.
pub open spec fn nested_epm_accept_source_ready(source: NestedEpmAcceptSource) -> bool {
    &&& epm::frame_layout_wf(source.parent)
    &&& epm::frame_layout_wf(source.child_before_end)
    &&& epm::indexed_image_wf(source.retained_child)
    &&& source.retained_child == source.child_before_end.snapshot
    &&& source.parent_admission.frame_kind == 3
    &&& source.parent_admission.source_bytes == source.parent.snapshot.bytes
    &&& source.child_admission.frame_kind == 3
    &&& source.child_admission.score_handle.is_none()
    &&& source.child_admission.source_bytes == source.child_before_end.snapshot.bytes
    &&& source.child_admission.region_start == 0
    &&& source.child_admission.region_end as int
        == source.child_before_end.snapshot.bytes.len()
    &&& source.child_admission.physical_source
        == epm::indexed_position_at(source.child_before_end.snapshot, 0).source
    &&& source.child_admission.physical_start
        == epm::indexed_position_at(source.child_before_end.snapshot, 0).offset
    &&& epm::event_step(
        source.child_before_end,
        source.child_after_end,
        epm::EpmEvent::End,
        source.next_obligation_id,
        epm::EpmAction::Complete {
            mode: source.child_before_end.mode,
            entries_stable: source.child_before_end.entries_stable,
        },
    )
    &&& match source.parent.wait {
        Some(epm::EpmWait::Obligation {
            id,
            kind: epm::PathObligationKind::NestedEpm1,
            return_id,
        }) => {
            &&& source.parent_admission.waiting_id == Some(return_id)
            &&& source.parent_admission.waiting_kind == 3
            &&& source.child_admission.return_id == Some(return_id)
            &&& match source.parent.path.pending {
                Some(pending) => {
                    &&& pending.id == id
                    &&& pending.kind == epm::PathObligationKind::NestedEpm1
                    &&& pending.awaiting_resolution
                },
                None => false,
            }
        },
        _ => false,
    }
}

/// A successful nested EPathMap child supplies its mode/stability from the
/// completed child frame, never from an independent caller-provided summary.
/// The outcome is the exact order of the Rust `resolve_obligation` call and
/// subsequent fallible `checked_add(1)`.
pub open spec fn nested_epm_accept_from_source(
    source: NestedEpmAcceptSource,
) -> Option<NestedEpmAcceptOutcome> {
    if !nested_epm_accept_source_ready(source) {
        None
    } else {
        match source.parent.wait {
            Some(epm::EpmWait::Obligation {
                id,
                kind: epm::PathObligationKind::NestedEpm1,
                ..
            }) => match source.reducer_call {
                NestedReducerCallResult::Failed => Some(
                    NestedEpmAcceptOutcome::ReducerError {
                        resolutions_after: source.resolutions_before,
                    },
                ),
                NestedReducerCallResult::Applied => {
                    if source.resolutions_before == usize::MAX {
                        Some(NestedEpmAcceptOutcome::OverflowAfterReducerApplied {
                            id,
                            mode: source.child_before_end.mode,
                            entries_stable: source.child_before_end.entries_stable,
                            resolutions_after: source.resolutions_before,
                        })
                    } else {
                        Some(NestedEpmAcceptOutcome::Resolved {
                            id,
                            mode: source.child_before_end.mode,
                            entries_stable: source.child_before_end.entries_stable,
                            resolutions_after: (source.resolutions_before as int + 1) as usize,
                        })
                    }
                },
            },
            _ => None,
        }
    }
}

pub proof fn nested_epm_accept_resolution_comes_from_child(
    source: NestedEpmAcceptSource,
)
    requires nested_epm_accept_source_ready(source),
    ensures
        match source.parent.wait {
            Some(epm::EpmWait::Obligation {
                id,
                kind: epm::PathObligationKind::NestedEpm1,
                ..
            }) => {
                &&& source.child_admission.frame_kind == 3
                &&& source.child_admission.source_bytes
                    == source.child_before_end.snapshot.bytes
                &&& source.retained_child == source.child_before_end.snapshot
                &&& match nested_epm_accept_from_source(source) {
                    Some(NestedEpmAcceptOutcome::Resolved {
                        id: effect_id, mode, entries_stable, ..
                    }) | Some(NestedEpmAcceptOutcome::OverflowAfterReducerApplied {
                        id: effect_id, mode, entries_stable, ..
                    }) => {
                        &&& effect_id == id
                        &&& mode == source.child_before_end.mode
                        &&& entries_stable == source.child_before_end.entries_stable
                    },
                    Some(NestedEpmAcceptOutcome::ReducerError { .. }) => true,
                    _ => false,
                }
            },
            _ => false,
        },
{}

pub proof fn nested_epm_accept_counter_overflow_has_no_effect(
    source: NestedEpmAcceptSource,
)
    requires
        nested_epm_accept_source_ready(source),
        source.reducer_call == NestedReducerCallResult::Applied,
        source.resolutions_before == usize::MAX,
    ensures
        match nested_epm_accept_from_source(source) {
            Some(NestedEpmAcceptOutcome::OverflowAfterReducerApplied {
                resolutions_after, ..
            }) => resolutions_after == source.resolutions_before,
            _ => false,
        },
{}

pub proof fn nested_epm_accept_checked_success_is_exact(
    source: NestedEpmAcceptSource,
)
    requires
        nested_epm_accept_source_ready(source),
        source.reducer_call == NestedReducerCallResult::Applied,
        source.resolutions_before < usize::MAX,
    ensures
        match nested_epm_accept_from_source(source) {
            Some(NestedEpmAcceptOutcome::Resolved {
                id, mode, entries_stable, resolutions_after,
            }) => {
                &&& resolutions_after as int == source.resolutions_before as int + 1
                &&& mode == source.child_before_end.mode
                &&& entries_stable == source.child_before_end.entries_stable
                &&& match source.parent.wait {
                    Some(epm::EpmWait::Obligation { id: waiting_id, .. }) => id == waiting_id,
                    _ => false,
                }
            },
            _ => false,
        },
{}

}

fn main() {}
