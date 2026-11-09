//! Source-shaped extraction of the bounded ACT line offer and its prepared
//! cursor commit. This module proves the local schedule and event headroom;
//! it does not identify the private validated plan's work queue with the
//! global EPM scalar event stream. That placement remains an explicit premise
//! of `rpb3_epm_batch_cursor_source_extraction`.

use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epm_batch_cursor_source_extraction.rs"]
mod line;
#[cfg(rpb3_integrated)]
use crate::rpb3_epm_batch_cursor_source_extraction as line;

verus! {

/// `CanonicalCompactTrieActiveLineWorkSource`, with its `Range` split into
/// scalar endpoints. The event index is supplied by the enclosing EPM cursor.
pub struct ActiveLineWorkSource {
    pub position: line::LineCursorSourcePosition,
    pub event_index: usize,
    pub direction: line::LineDirection,
    pub start: usize,
    pub end: usize,
    pub remaining_work_events: Option<usize>,
    pub admissible_source_width: usize,
    pub admissible_work_events: usize,
}

pub open spec fn offered_width(work: ActiveLineWorkSource) -> int {
    work.end as int - work.start as int
}

pub open spec fn event_factor(direction: line::LineDirection) -> int {
    match direction {
        line::LineDirection::Forward => 2,
        line::LineDirection::Unwind => 1,
    }
}

/// Transcribes `active_line_work_source`: checked line endpoints, nonempty
/// suffix, optional full work count, and the `usize::MAX - event_index` cap.
pub open spec fn active_line_work_source_shape(work: ActiveLineWorkSource) -> bool {
    let p = work.position;
    let max = usize::MAX as int;
    if p.line_start.is_none() || p.line_end.is_none() {
        false
    } else {
        let line_start = p.line_start.unwrap() as int;
        let line_end = p.line_end.unwrap() as int;
        let full = offered_width(work);
        let factor = event_factor(work.direction);
        let headroom_width = (max - work.event_index as int) / factor;
        let admissible = if full <= headroom_width { full } else { headroom_width };
        &&& line_start <= line_end
        &&& full > 0
        &&& match work.direction {
            line::LineDirection::Forward => {
                &&& p.stage == line::LineStage::LineEdge
                &&& work.start as int == line_start + p.line_index as int
                &&& work.end as int == line_end - 1
            },
            line::LineDirection::Unwind => {
                &&& (p.stage == line::LineStage::Leave
                    || (p.stage == line::LineStage::Children && p.remaining_children == 0))
                &&& work.start as int == line_start
                &&& work.end as int == line_end - p.leave_index as int
            },
        }
        &&& work.remaining_work_events == if factor * full <= max {
            Some((factor * full) as usize)
        } else {
            None
        }
        &&& work.admissible_source_width as int == admissible
        &&& work.admissible_work_events as int == factor * admissible
    }
}

/// Exact mutation made by `CanonicalCompactTriePreparedLineSpan::commit`.
pub open spec fn position_after_commit(
    p: line::LineCursorSourcePosition,
    direction: line::LineDirection,
    width: usize,
) -> line::LineCursorSourcePosition {
    line::LineCursorSourcePosition {
        record: p.record,
        node_id: p.node_id,
        next_record: p.next_record,
        stack_depth: p.stack_depth,
        remaining_children: p.remaining_children,
        stage: match direction {
            line::LineDirection::Forward => p.stage,
            line::LineDirection::Unwind => line::LineStage::Leave,
        },
        line_start: p.line_start,
        line_end: p.line_end,
        line_index: match direction {
            line::LineDirection::Forward => (p.line_index as int + width as int) as usize,
            line::LineDirection::Unwind => p.line_index,
        },
        leave_index: match direction {
            line::LineDirection::Forward => p.leave_index,
            line::LineDirection::Unwind => (p.leave_index as int + width as int) as usize,
        },
    }
}

/// The minimal Rust-to-model facts from `line_span().prepare(width).commit()`.
/// It names only source fields and the post-position, not any expected event.
pub open spec fn prepared_commit_source_projection(
    work: ActiveLineWorkSource,
    width: usize,
    commit: line::AcceptedLineCommitSource,
) -> bool {
    &&& 0 < width as int <= work.admissible_source_width as int
    &&& commit.before == work.position
    &&& commit.after == position_after_commit(work.position, work.direction, width)
    &&& commit.direction == work.direction
    &&& commit.expanded_events as int == event_factor(work.direction) * width as int
    &&& match work.direction {
        line::LineDirection::Forward => {
            &&& commit.start == work.start
            &&& commit.end as int == work.start as int + width as int
        },
        line::LineDirection::Unwind => {
            &&& commit.start as int == work.end as int - width as int
            &&& commit.end == work.end
        },
    }
}

/// Every offered positive prefix fits the event counter, even if the full
/// line's `remaining_work_events` is `None` or exceeds available headroom.
pub proof fn accepted_width_fits_event_headroom(
    work: ActiveLineWorkSource,
    width: usize,
)
    requires
        active_line_work_source_shape(work),
        0 < width as int <= work.admissible_source_width as int,
    ensures
        width as int <= offered_width(work),
        event_factor(work.direction) * width as int
            <= work.admissible_work_events as int,
        work.event_index as int + event_factor(work.direction) * width as int
            <= usize::MAX as int,
{
    let full = offered_width(work);
    let factor = event_factor(work.direction);
    let headroom = usize::MAX as int - work.event_index as int;
    assert(factor == 1 || factor == 2);
    assert(work.admissible_source_width as int
        == if full <= headroom / factor { full } else { headroom / factor });
    assert(width as int <= full);
    assert(width as int <= headroom / factor);
    assert(factor * (width as int) <= headroom);
}

/// For either direction, the observed source offer and the production
/// `prepare().commit()` field projection imply the guard consumed by the
/// existing scalar-event extraction theorem. No scalar event is a premise.
pub proof fn prepared_line_derives_scalar_schedule_guard(
    work: ActiveLineWorkSource,
    width: usize,
    commit: line::AcceptedLineCommitSource,
)
    requires
        active_line_work_source_shape(work),
        prepared_commit_source_projection(work, width, commit),
    ensures
        line::commit_matches_source_positions(commit),
        commit.expanded_events as int <= work.admissible_work_events as int,
        work.event_index as int + commit.expanded_events as int <= usize::MAX as int,
{
    accepted_width_fits_event_headroom(work, width);
    let p = work.position;
    let full = offered_width(work);
    assert(width as int <= full);
    match work.direction {
        line::LineDirection::Forward => {
            assert(commit.end as int <= work.end as int);
            assert((commit.end as int) < p.line_end.unwrap() as int);
            assert(p.line_start.unwrap() as int + p.line_index as int
                == commit.start as int);
            assert(p.line_index as int + width as int <= usize::MAX as int);
        },
        line::LineDirection::Unwind => {
            assert(work.start as int <= commit.start as int);
            assert(p.line_end.unwrap() as int - p.leave_index as int
                == commit.end as int);
            assert(p.leave_index as int + width as int <= usize::MAX as int);
        },
    }
    assert(line::commit_width(commit) == width as int);
    assert(line::commit_matches_source_positions(commit));
}

/// The source offset for each prospective scalar step stays within the
/// committed prefix; the global event ordinal is also representable.
pub proof fn prepared_line_step_has_bounded_source_and_event_index(
    work: ActiveLineWorkSource,
    width: usize,
    commit: line::AcceptedLineCommitSource,
    step: int,
)
    requires
        active_line_work_source_shape(work),
        prepared_commit_source_projection(work, width, commit),
        0 <= step < commit.expanded_events as int,
    ensures
        (commit.start as int) <= (line::line_batch_source_offset(commit, step) as int),
        (line::line_batch_source_offset(commit, step) as int) < (commit.end as int),
        work.event_index as int + step < usize::MAX as int,
{
    prepared_line_derives_scalar_schedule_guard(work, width, commit);
    match work.direction {
        line::LineDirection::Forward => {
            assert(commit.expanded_events as int == 2 * width as int);
            assert(0 <= step / 2 < width as int);
        },
        line::LineDirection::Unwind => {
            assert(commit.expanded_events as int == width as int);
        },
    }
}

pub open spec fn overflow_forward_offer() -> ActiveLineWorkSource {
    ActiveLineWorkSource {
        position: line::LineCursorSourcePosition {
            record: 0, node_id: 0, next_record: 1, stack_depth: 1,
            remaining_children: 0, stage: line::LineStage::LineEdge,
            line_start: Some(10), line_end: Some(16), line_index: 0, leave_index: 0,
        },
        event_index: (usize::MAX as int - 3) as usize,
        direction: line::LineDirection::Forward,
        start: 10,
        end: 15,
        remaining_work_events: Some(10),
        admissible_source_width: 1,
        admissible_work_events: 2,
    }
}

pub open spec fn overflow_unwind_offer() -> ActiveLineWorkSource {
    ActiveLineWorkSource {
        position: line::LineCursorSourcePosition {
            record: 0, node_id: 0, next_record: 1, stack_depth: 1,
            remaining_children: 0, stage: line::LineStage::Leave,
            line_start: Some(10), line_end: Some(16), line_index: 6, leave_index: 0,
        },
        event_index: (usize::MAX as int - 3) as usize,
        direction: line::LineDirection::Unwind,
        start: 10,
        end: 16,
        remaining_work_events: Some(6),
        admissible_source_width: 3,
        admissible_work_events: 3,
    }
}

/// A concrete boundary witness: three remaining event ordinals permit one
/// Forward source byte (two events) but not two; Unwind can use all three.
pub proof fn overflow_limited_prefix_witness()
    ensures
        active_line_work_source_shape(overflow_forward_offer()),
        active_line_work_source_shape(overflow_unwind_offer()),
        overflow_forward_offer().admissible_source_width == 1,
        overflow_unwind_offer().admissible_source_width == 3,
        overflow_forward_offer().event_index as int + 2 <= usize::MAX as int,
        overflow_forward_offer().event_index as int + 4 > usize::MAX as int,
        overflow_unwind_offer().event_index as int + 3 == usize::MAX as int,
{
    assert(active_line_work_source_shape(overflow_forward_offer()));
    assert(active_line_work_source_shape(overflow_unwind_offer()));
    accepted_width_fits_event_headroom(overflow_forward_offer(), 1);
    accepted_width_fits_event_headroom(overflow_unwind_offer(), 3);
}

}
