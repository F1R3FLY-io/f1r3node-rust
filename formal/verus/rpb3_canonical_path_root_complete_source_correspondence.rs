//! Retained CanonicalPath terminal source through the no-parent driver pop.
//!
//! The production `CanonicalPathTerminalSource::matches_complete` compares
//! the retained indexed view, EOF anchor, cursor counts, checked stats, and
//! returned summary. `DriverCompleteSource::before_pop` and
//! `matches_root_return` additionally check the single-frame pop, score
//! length, unchanged resolution count, and returned summary. This bridge
//! expresses those observations at the existing live trace boundary. The
//! symbolic indexed-image equality and each observed Rust guard result are
//! explicit extraction premises; this file does not prove the Rust methods.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_root_source_refinement as root_source,
    rpb3_canonical_path_terminal_driver_observation as driver,
    rpb3_epathmap_event_refinement as epm,
    rpb3_live_canonical_path_root_terminal_trace as capstone,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

/// A successful production terminal-source check and the corresponding
/// no-parent `DriverCompleteSource` check. `same_view_observed` records the
/// Rust result; `retained_input` must separately be extracted as the live
/// indexed image. Equal bytes alone cannot establish view identity.
pub struct CanonicalPathRootCompleteSourceObservation {
    pub ghost retained_input: epm::IndexedRegionImage,
    pub same_view_observed: bool,
    pub retained_end: epm::SourceAnchor,
    pub retained_bytes_yielded: usize,
    pub retained_bytes_traversed: usize,
    pub retained_summary: root_source::CanonicalPathSummaryImage,
    pub returned_summary: root_source::CanonicalPathSummaryImage,
    pub stack_len_after_pop: usize,
    pub parent_after_pop: bool,
    pub accept_effect_after_pop: bool,
    pub score_entries_after_pop: usize,
    pub resolutions_before_pop: usize,
    pub resolutions_after_pop: usize,
    pub read_after_pop: usize,
    pub traversed_after_pop: usize,
}

/// Exact scalar projection of the checked production source guards. This is
/// stronger than merely observing a Complete action: the retained view and
/// summary must agree with the pre-pop frame, and the no-parent pop must
/// preserve stats and score length.
pub open spec fn canonical_path_root_complete_source_ready(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    event: driver::CanonicalPathTerminalDriverObservation,
    observed: CanonicalPathRootCompleteSourceObservation,
) -> bool {
    if !trace::live_canonical_path_trace(root, states, actions)
        || states.len() == 0
    { false } else {
        let before = states.last();
        if before.path_frames.len() == 0 || before.path_frames[0].is_none() {
            false
        } else {
            let frame = before.path_frames[0].unwrap();
            &&& before.base.source.product.bridge.admission.frames.len() == 1
            &&& event.stack_len_before == 1
            &&& event.top_kind == 4
            &&& event.cursor_exhausted
            &&& observed.same_view_observed
            &&& observed.retained_input == frame.input
            &&& observed.retained_end == observed.retained_input.end
            &&& observed.retained_end == event.call_at
            &&& observed.retained_bytes_yielded == event.cursor_bytes_yielded
            &&& observed.retained_bytes_yielded == frame.cursor
            &&& observed.retained_bytes_yielded as int == frame.input.bytes.len()
            &&& observed.retained_bytes_traversed == event.cursor_bytes_traversed
            &&& event.waiting_absent
            &&& event.waiting_child_absent
            &&& event.call_has_value
            &&& !event.active_obligation
            &&& match event.value_event {
                driver::CanonicalPathValueCallEvent::ReturnedSummary { summary } =>
                    summary == observed.retained_summary
                        && summary == observed.returned_summary,
                _ => false,
            }
            &&& event.stats_event == driver::canonical_path_terminal_checked_stats_event(event)
            &&& match event.stats_event {
                driver::CanonicalPathTerminalStatsEvent::Success {
                    read_after, traversed_after,
                } => read_after == observed.read_after_pop
                    && traversed_after == observed.traversed_after_pop,
                _ => false,
            }
            &&& event.score_entries_before as int
                == before.base.source.product.bridge.transcript.entries.len()
            &&& event.score_entries_after == event.score_entries_before
            &&& observed.stack_len_after_pop == 0
            &&& !observed.parent_after_pop
            &&& !observed.accept_effect_after_pop
            &&& observed.score_entries_after_pop == event.score_entries_after
            &&& observed.resolutions_after_pop == observed.resolutions_before_pop
        }
    }
}

/// A failed source guard cannot be promoted to a modeled successful return.
pub open spec fn canonical_path_root_complete_from_retained_source(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    event: driver::CanonicalPathTerminalDriverObservation,
    observed: CanonicalPathRootCompleteSourceObservation,
) -> Option<driver::CanonicalPathObservedTerminalOutcome> {
    if !canonical_path_root_complete_source_ready(root, states, actions, event, observed) {
        None
    } else {
        driver::canonical_path_terminal_driver_from_observation(root, states, actions, event)
    }
}

pub proof fn canonical_path_root_complete_source_enables_driver(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    event: driver::CanonicalPathTerminalDriverObservation,
    observed: CanonicalPathRootCompleteSourceObservation,
)
    requires canonical_path_root_complete_source_ready(
        root, states, actions, event, observed,
    ),
    ensures driver::canonical_path_terminal_driver_source_ready(
        root, states, actions, event,
    ),
{
    let before = states.last();
    let frame = before.path_frames[0].unwrap();
    assert(observed.retained_input == frame.input);
    assert(event.call_at == frame.input.end);
    assert(driver::canonical_path_terminal_driver_source_ready(
        root, states, actions, event,
    ));
}

/// The returned value comes from the reducer event and is the very value
/// retained by the terminal source and no-parent driver source. The root pop
/// uses the established live terminal selector; no poststate is supplied.
pub proof fn canonical_path_root_complete_source_corresponds(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    event: driver::CanonicalPathTerminalDriverObservation,
    observed: CanonicalPathRootCompleteSourceObservation,
)
    requires canonical_path_root_complete_source_ready(
        root, states, actions, event, observed,
    ),
    ensures match canonical_path_root_complete_from_retained_source(
        root, states, actions, event, observed,
    ) {
        Some(result) => {
            let before = states.last();
            &&& result.action_summary == observed.retained_summary
            &&& result.action_summary == observed.returned_summary
            &&& result.terminal.returned.summary == observed.returned_summary
            &&& result.terminal.indexed_end == observed.retained_end
            &&& result.terminal.remaining_frames.len() == observed.stack_len_after_pop as int
            &&& result.terminal.returned.remaining_frames == observed.stack_len_after_pop
            &&& result.terminal.returned.arena
                == before.base.source.product.bridge.admission.arena
            &&& result.terminal.returned.transcript
                == before.base.source.product.bridge.transcript
            &&& result.terminal.returned.remaining_capacity
                == before.base.source.runtime.capacity
            &&& observed.score_entries_after_pop == event.score_entries_before
            &&& observed.resolutions_after_pop == observed.resolutions_before_pop
        },
        None => false,
    },
{
    canonical_path_root_complete_source_enables_driver(
        root, states, actions, event, observed,
    );
    let summary = observed.retained_summary;
    let reducer = root_source::CanonicalPathReducerEofObservation {
        has_value: event.call_has_value,
        at: event.call_at,
        active_obligation: event.active_obligation,
        value_result: Some(summary),
    };
    assert(capstone::canonical_path_trace_terminal_eof_observed(states, reducer));
    capstone::canonical_path_root_trace_to_successful_terminal_is_exact(
        root, states, actions, reducer,
    );
    assert(driver::canonical_path_terminal_driver_from_observation(
        root, states, actions, event,
    ).is_some());
    driver::canonical_path_terminal_driver_success_is_exact(
        root, states, actions, event,
    );
}

pub proof fn canonical_path_root_complete_source_rejects_mismatch(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    event: driver::CanonicalPathTerminalDriverObservation,
    observed: CanonicalPathRootCompleteSourceObservation,
)
    requires !canonical_path_root_complete_source_ready(
        root, states, actions, event, observed,
    ),
    ensures canonical_path_root_complete_from_retained_source(
        root, states, actions, event, observed,
    ).is_none(),
{}

}
