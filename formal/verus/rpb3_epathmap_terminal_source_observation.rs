//! Source-shaped EPathMap terminal End and ordered checked stats.
//!
//! `epathmap_step` reaches `next_event() == None`, calls `path.finish`,
//! commits read, then traversed, then event counters through checked adds,
//! and only then returns the frame's own mode, stability, and indexed
//! snapshot. The observed EOF, reducer result, cursor counts, and indexed
//! image still require extraction from the Rust implementation.

use vstd::prelude::*;

use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub enum EpmTerminalStatsOutcome {
    ReadOverflow,
    TraversedOverflow { read_after: usize },
    EventsOverflow { read_after: usize, traversed_after: usize },
    Success {
        read_after: usize,
        traversed_after: usize,
        events_after: usize,
    },
}

pub struct EpmTerminalSourceObservation {
    pub ghost snapshot: epm::IndexedRegionImage,
    pub end: epm::SourceAnchor,
    pub region_start: epm::SourceAnchor,
    pub last_source: epm::SourceAnchor,
    pub cursor_at: usize,
    pub value_table_offset: usize,
    pub path_finish_succeeded: bool,
    pub mode: epm::EpmMode,
    pub entries_stable: bool,
    pub read_before: usize,
    pub prefix_read: usize,
    pub value_table_read: usize,
    pub traversed_before: usize,
    pub prefix_traversed: usize,
    pub value_table_traversed: usize,
    pub events_before: usize,
    pub topology_events: usize,
    pub stats_outcome: EpmTerminalStatsOutcome,
}

/// The two additions within each byte counter are one assignment in Rust.
/// A traversed overflow can follow a committed read update; an event overflow
/// can follow both committed byte-counter updates.
pub open spec fn epm_terminal_checked_stats(
    observed: EpmTerminalSourceObservation,
) -> EpmTerminalStatsOutcome {
    let read = observed.read_before as int + observed.prefix_read as int
        + observed.value_table_read as int;
    let traversed = observed.traversed_before as int
        + observed.prefix_traversed as int
        + observed.value_table_traversed as int;
    let events = observed.events_before as int + observed.topology_events as int;
    if read > usize::MAX as int {
        EpmTerminalStatsOutcome::ReadOverflow
    } else if traversed > usize::MAX as int {
        EpmTerminalStatsOutcome::TraversedOverflow {
            read_after: read as usize,
        }
    } else if events > usize::MAX as int {
        EpmTerminalStatsOutcome::EventsOverflow {
            read_after: read as usize,
            traversed_after: traversed as usize,
        }
    } else {
        EpmTerminalStatsOutcome::Success {
            read_after: read as usize,
            traversed_after: traversed as usize,
            events_after: events as usize,
        }
    }
}

/// Deterministic End successor: only the terminal flag changes in the
/// EPathMap frame. The driver pop and parent acceptance are separate actions.
pub open spec fn epm_terminal_end_frame(before: epm::EpmFrame) -> epm::EpmFrame {
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

pub proof fn epm_terminal_end_preserves_layout(before: epm::EpmFrame)
    requires epm::frame_layout_wf(before),
    ensures epm::frame_layout_wf(epm_terminal_end_frame(before)),
{}

pub open spec fn epm_terminal_source_ready(
    before: epm::EpmFrame,
    observed: EpmTerminalSourceObservation,
) -> bool {
    &&& epm::frame_layout_wf(before)
    &&& !before.cursor.done
    &&& before.wait.is_none()
    &&& observed.snapshot == before.snapshot
    &&& observed.end == before.snapshot.end
    &&& observed.region_start == epm::indexed_position_at(before.snapshot, 0)
    &&& observed.last_source == before.last_source
    &&& observed.cursor_at == before.cursor.at
    &&& before.cursor.at == epm::cursor_event_count(before)
    &&& observed.value_table_offset == before.cursor.value_table_offset
    &&& before.cursor.value_table_offset as int == before.snapshot.bytes.len()
    &&& (before.mode != epm::EpmMode::Map
        || before.cursor.map_values_seen == before.cursor.declared_value_count)
    &&& observed.path_finish_succeeded
    &&& epm::path_finish_ok(before.path)
    &&& observed.mode == before.mode
    &&& observed.entries_stable == before.entries_stable
    &&& observed.stats_outcome == epm_terminal_checked_stats(observed)
}

pub struct EpmTerminalSourceComplete {
    pub ghost after: epm::EpmFrame,
    pub action: epm::EpmAction,
    pub read_after: usize,
    pub traversed_after: usize,
    pub events_after: usize,
}

pub open spec fn epm_terminal_complete_from_source(
    before: epm::EpmFrame,
    observed: EpmTerminalSourceObservation,
) -> Option<EpmTerminalSourceComplete> {
    if !epm_terminal_source_ready(before, observed) { None }
    else {
        match observed.stats_outcome {
            EpmTerminalStatsOutcome::Success {
                read_after, traversed_after, events_after,
            } => Some(EpmTerminalSourceComplete {
                after: epm_terminal_end_frame(before),
                action: epm::EpmAction::Complete {
                    mode: observed.mode,
                    entries_stable: observed.entries_stable,
                },
                read_after,
                traversed_after,
                events_after,
            }),
            _ => None,
        }
    }
}

pub proof fn epm_terminal_source_success_enabled(
    before: epm::EpmFrame,
    observed: EpmTerminalSourceObservation,
)
    requires
        epm_terminal_source_ready(before, observed),
        observed.stats_outcome is Success,
    ensures epm_terminal_complete_from_source(before, observed).is_some(),
{}

pub proof fn epm_terminal_source_success_is_exact(
    before: epm::EpmFrame,
    observed: EpmTerminalSourceObservation,
)
    requires epm_terminal_complete_from_source(before, observed).is_some(),
    ensures {
        let complete = epm_terminal_complete_from_source(before, observed).unwrap();
        &&& epm::event_step(
            before, complete.after, epm::EpmEvent::End, 0, complete.action,
        )
        &&& complete.after.snapshot == observed.snapshot
        &&& complete.after.cursor.done
        &&& complete.after.wait == before.wait
        &&& complete.after.path == before.path
        &&& match complete.action {
            epm::EpmAction::Complete { mode, entries_stable } => {
                mode == observed.mode && entries_stable == observed.entries_stable
            },
            _ => false,
        }
        &&& complete.read_after as int == observed.read_before as int
            + observed.prefix_read as int + observed.value_table_read as int
        &&& complete.traversed_after as int == observed.traversed_before as int
            + observed.prefix_traversed as int + observed.value_table_traversed as int
        &&& complete.events_after as int == observed.events_before as int
            + observed.topology_events as int
    },
{
    epm_terminal_end_preserves_layout(before);
}

pub proof fn epm_terminal_source_overflow_has_no_complete(
    before: epm::EpmFrame,
    observed: EpmTerminalSourceObservation,
)
    requires epm_terminal_source_ready(before, observed),
    ensures match observed.stats_outcome {
        EpmTerminalStatsOutcome::ReadOverflow
        | EpmTerminalStatsOutcome::TraversedOverflow { .. }
        | EpmTerminalStatsOutcome::EventsOverflow { .. } =>
            epm_terminal_complete_from_source(before, observed).is_none(),
        _ => true,
    },
{}

} // verus!
