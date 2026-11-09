//! Source-shaped CanonicalPath root `step` and no-parent Complete boundary.
//!
//! `CanonicalPathFrame::step` calls `reducer.value(true, input.end_position())`
//! only after its indexed cursor is exhausted. Two checked stats additions
//! follow a successful value before it can wrap that value as
//! `FrameAction::Complete(FrameSummary::CanonicalPath(summary))`. The driver
//! pops its sole frame and returns that same summary. This selector branches
//! on the observed call and checked-stats results; it neither presumes success nor takes
//! a caller-supplied Complete action or terminal poststate. Binding each
//! observation to arbitrary Rust execution remains an extraction obligation.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_root_source_refinement as root_source,
    rpb3_epathmap_event_refinement as epm,
    rpb3_live_canonical_path_root_terminal_trace as capstone,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_terminal_refinement as terminal,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

/// An event at the actual reducer call site, including the unsuccessful arms
/// of `Option<Result<CanonicalPathSummary, _>>`. The summary is a value read
/// from that return, never an independent expected FrameAction value.
pub enum CanonicalPathValueCallEvent {
    NotCalled,
    ReturnedError,
    ReturnedSummary { summary: root_source::CanonicalPathSummaryImage },
}

/// The two production `checked_add` calls execute in this order. A traversed
/// overflow leaves the earlier read counter updated but emits no Complete.
pub enum CanonicalPathTerminalStatsEvent {
    NotReached,
    ReadOverflow,
    TraversedOverflow { read_after: usize },
    Success { read_after: usize, traversed_after: usize },
}

/// Every scalar field occurs in the production EOF or root Complete path.
/// `waiting_child_absent` is separate because the live path model currently
/// tracks `waiting` but does not represent the retained Rust child view.
pub struct CanonicalPathTerminalDriverObservation {
    pub stack_len_before: usize,
    pub top_kind: u8,
    pub cursor_exhausted: bool,
    pub cursor_bytes_yielded: usize,
    pub cursor_bytes_traversed: usize,
    pub waiting_absent: bool,
    pub waiting_child_absent: bool,
    pub active_obligation: bool,
    pub call_has_value: bool,
    pub call_at: epm::SourceAnchor,
    pub value_event: CanonicalPathValueCallEvent,
    pub stats_read_before: usize,
    pub stats_traversed_before: usize,
    pub stats_event: CanonicalPathTerminalStatsEvent,
    pub score_entries_before: usize,
    pub score_entries_after: usize,
}

pub open spec fn canonical_path_terminal_checked_stats_event(
    observed: CanonicalPathTerminalDriverObservation,
) -> CanonicalPathTerminalStatsEvent {
    let read = observed.stats_read_before as int + observed.cursor_bytes_yielded as int;
    let traversed = observed.stats_traversed_before as int
        + observed.cursor_bytes_traversed as int;
    if !match observed.value_event {
        CanonicalPathValueCallEvent::ReturnedSummary { .. } => true,
        _ => false,
    } {
        CanonicalPathTerminalStatsEvent::NotReached
    } else if read > usize::MAX as int {
        CanonicalPathTerminalStatsEvent::ReadOverflow
    } else if traversed > usize::MAX as int {
        CanonicalPathTerminalStatsEvent::TraversedOverflow {
            read_after: read as usize,
        }
    } else {
        CanonicalPathTerminalStatsEvent::Success {
            read_after: read as usize,
            traversed_after: traversed as usize,
        }
    }
}

pub struct CanonicalPathObservedTerminalOutcome {
    pub action_summary: root_source::CanonicalPathSummaryImage,
    pub reducer: root_source::CanonicalPathReducerEofObservation,
    pub terminal: terminal::LiveCanonicalPathTerminalOutcome,
}

/// Source scalar checks corresponding to the root `step` and
/// `DriverCompleteSource` no-parent branch. The trace supplies the retained
/// indexed image and score bridge; a count alone does not establish score
/// transcript equality from Rust.
pub open spec fn canonical_path_terminal_driver_source_ready(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    observed: CanonicalPathTerminalDriverObservation,
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
            &&& observed.stack_len_before == 1
            &&& observed.top_kind == 4
            &&& observed.cursor_exhausted
            &&& observed.cursor_bytes_yielded == frame.cursor
            &&& observed.cursor_bytes_yielded as int == frame.input.bytes.len()
            &&& observed.waiting_absent
            &&& observed.waiting_child_absent
            &&& observed.call_has_value
            &&& observed.call_at == frame.input.end
            &&& observed.stats_event == canonical_path_terminal_checked_stats_event(observed)
            &&& observed.score_entries_before as int
                == before.base.source.product.bridge.transcript.entries.len()
            &&& observed.score_entries_after == observed.score_entries_before
        }
    }
}

/// Only a successful value-call event produces a Complete action. The
/// complete root result is delegated to the already proved terminal selector.
pub open spec fn canonical_path_terminal_driver_from_observation(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    observed: CanonicalPathTerminalDriverObservation,
) -> Option<CanonicalPathObservedTerminalOutcome> {
    if !canonical_path_terminal_driver_source_ready(root, states, actions, observed) {
        None
    } else {
        match observed.value_event {
            CanonicalPathValueCallEvent::ReturnedSummary { summary }
                if !observed.active_obligation
                    && match observed.stats_event {
                        CanonicalPathTerminalStatsEvent::Success { .. } => true,
                        _ => false,
                    } => {
                let reducer = root_source::CanonicalPathReducerEofObservation {
                    has_value: observed.call_has_value,
                    at: observed.call_at,
                    active_obligation: observed.active_obligation,
                    value_result: Some(summary),
                };
                match terminal::live_canonical_path_terminal_complete_from_source(
                    states.last(), reducer,
                ) {
                    None => None,
                    Some(terminal_result) => Some(CanonicalPathObservedTerminalOutcome {
                        action_summary: summary,
                        reducer,
                        terminal: terminal_result,
                    }),
                }
            },
            _ => None,
        }
    }
}

pub proof fn canonical_path_terminal_driver_success_is_exact(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    observed: CanonicalPathTerminalDriverObservation,
)
    requires canonical_path_terminal_driver_from_observation(
        root, states, actions, observed,
    ).is_some(),
    ensures {
        let result = canonical_path_terminal_driver_from_observation(
            root, states, actions, observed,
        ).unwrap();
        let before = states.last();
        let pre = before.base.source.product.bridge.admission;
        &&& match observed.value_event {
            CanonicalPathValueCallEvent::ReturnedSummary { summary } =>
                result.action_summary == summary,
            _ => false,
        }
        &&& result.reducer.value_result == Some(result.action_summary)
        &&& result.reducer.at == observed.call_at
        &&& match observed.stats_event {
            CanonicalPathTerminalStatsEvent::Success {
                read_after, traversed_after,
            } => {
                &&& read_after as int == observed.stats_read_before as int
                    + observed.cursor_bytes_yielded as int
                &&& traversed_after as int == observed.stats_traversed_before as int
                    + observed.cursor_bytes_traversed as int
            },
            _ => false,
        }
        &&& result.terminal.returned.summary == result.action_summary
        &&& result.terminal.indexed_end == observed.call_at
        &&& result.terminal.indexed_end == path_live::indexed_from_admission(
            root.indexed_input,
        ).end
        &&& result.terminal.remaining_frames.len() == 0
        &&& result.terminal.returned.remaining_frames == 0
        &&& result.terminal.returned.completed as int == pre.completed as int + 1
        &&& result.terminal.returned.arena == pre.arena
        &&& result.terminal.returned.transcript
            == before.base.source.product.bridge.transcript
        &&& observed.score_entries_after == observed.score_entries_before
    },
{
    let before = states.last();
    match observed.value_event {
        CanonicalPathValueCallEvent::ReturnedSummary { summary } => {
            let reducer = root_source::CanonicalPathReducerEofObservation {
                has_value: observed.call_has_value,
                at: observed.call_at,
                active_obligation: observed.active_obligation,
                value_result: Some(summary),
            };
            assert(canonical_path_terminal_driver_source_ready(
                root, states, actions, observed,
            ));
            assert(capstone::canonical_path_trace_terminal_eof_observed(states, reducer));
            capstone::canonical_path_root_trace_to_successful_terminal_is_exact(
                root, states, actions, reducer,
            );
            assert(terminal::live_canonical_path_terminal_complete_from_source(
                before, reducer,
            ).is_some());
        },
        _ => assert(false),
    }
}

pub proof fn canonical_path_terminal_driver_failure_has_no_complete(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    observed: CanonicalPathTerminalDriverObservation,
)
    requires
        observed.value_event == CanonicalPathValueCallEvent::NotCalled
            || observed.value_event == CanonicalPathValueCallEvent::ReturnedError,
    ensures canonical_path_terminal_driver_from_observation(
        root, states, actions, observed,
    ).is_none(),
{}

pub proof fn canonical_path_terminal_driver_stats_overflow_has_no_complete(
    root: admission::AdmissionCanonicalPathRootSource,
    states: Seq<path_live::LiveCanonicalPathState>,
    actions: Seq<trace::LiveCanonicalPathTraceAction>,
    observed: CanonicalPathTerminalDriverObservation,
)
    requires
        observed.stats_read_before as int + observed.cursor_bytes_yielded as int
            > usize::MAX as int
            || observed.stats_traversed_before as int
                + observed.cursor_bytes_traversed as int > usize::MAX as int,
    ensures canonical_path_terminal_driver_from_observation(
        root, states, actions, observed,
    ).is_none(),
{
    if canonical_path_terminal_driver_source_ready(root, states, actions, observed) {
        assert(observed.stats_event == canonical_path_terminal_checked_stats_event(observed));
        assert(match observed.stats_event {
            CanonicalPathTerminalStatsEvent::Success { .. } => false,
            _ => true,
        });
    }
}

}
