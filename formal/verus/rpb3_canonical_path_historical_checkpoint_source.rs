//! Fieldwise bridge from retained reducer predecessor observations to the
//! ghost checkpoints used by the EPathMap Unwind composition.
//!
//! `batch_restore_predecessor_source(run_index, depth)` supplies each Rust
//! observation on demand, and `batch_restore_predecessor_parser_source`
//! supplies the frame/owner image of that same saved handle. The proof below
//! does not assume an entire ghost
//! checkpoint equals an observation. It isolates two projections still owed
//! by Rust: the actual checkpoint/owner/frame fields must satisfy
//! `predecessor_matches_saved_state`, and the chosen historical ghost fields
//! must satisfy `checkpoint_matches_predecessor`. Arena prefix provenance
//! must satisfy `predecessor_anchor_matches_prefix`. A nonempty restored
//! prefix already exposes its Payload-end anchor as `last_source`; only a
//! depth-zero Payload checkpoint needs an extra pre-prefix anchor.

use vstd::prelude::*;

use crate::{
    rpb3_canonical_path_persistent_state_source as saved, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_batch_cursor_composition as batch,
};

verus! {

/// Projection of `CanonicalPathBatchPendingSource`. `payload_end_source` is
/// present only for AwaitingResolution; Payload uses the restored prefix's
/// last source whenever that prefix is nonempty.
pub struct ObservedPending {
    pub id: u64,
    pub kind: epm::PathObligationKind,
    pub payload_len: u64,
    pub tag_source: epm::SourceAnchor,
    pub payload_start: usize,
    pub payload_end_exclusive: u128,
    pub remaining: u64,
    pub awaiting_resolution: bool,
    pub payload_end_source: Option<epm::SourceAnchor>,
}

/// Projection of the relevant fields of `CanonicalPathBatchHandleSource`.
/// Its owner boundary is retained as an observation, not inferred here.
pub struct ObservedPredecessor {
    pub state_id: usize,
    pub depth: usize,
    pub last_source: Option<epm::SourceAnchor>,
    pub stamp: usize,
    pub open: bool,
    pub fault_free: bool,
    pub gate: epm::PathGate,
    pub owner_boundary_end: Option<u128>,
    pub pending: Option<ObservedPending>,
    pub ghost parser_source: epm::parser::ParserSourceSnapshot,
}

pub open spec fn saved_run(run: batch::HistoricalRestoreRunImage) -> saved::SourceSavedRun {
    saved::SourceSavedRun {
        state_token: run.saved_state_id,
        first_depth: run.first_depth,
        latest_depth: run.latest_depth,
    }
}

pub open spec fn gate_fields_match(actual: epm::PathGate, model: saved::SourceGate) -> bool {
    &&& actual.dead == model.dead
    &&& actual.stack_empty == model.stack_empty
    &&& actual.split_complete == model.split_complete
    &&& actual.completed_segments as int == model.completed_segments as int
    &&& actual.first_is_list == model.first_is_list
    &&& actual.stable == model.stable
    &&& actual.escape_allowed == model.escape_allowed
}

pub open spec fn formal_gate_fields_match(left: epm::PathGate, right: epm::PathGate) -> bool {
    &&& left.dead == right.dead
    &&& left.stack_empty == right.stack_empty
    &&& left.split_complete == right.split_complete
    &&& left.completed_segments == right.completed_segments
    &&& left.first_is_list == right.first_is_list
    &&& left.stable == right.stable
    &&& left.escape_allowed == right.escape_allowed
}

pub open spec fn anchor_fields_match(actual: epm::SourceAnchor, model: saved::SourceAnchor) -> bool {
    actual.source as int == model.source as int && actual.offset == model.offset
}

/// `SourceObligation.kind` uses the explicit projection codes 0 =
/// RawParEscape, 1 = NestedEpm1. Rust enum-to-code extraction is a premise.
pub open spec fn kind_fields_match(actual: epm::PathObligationKind, model: u8) -> bool {
    (model == 0 && actual == epm::PathObligationKind::RawParEscape)
        || (model == 1 && actual == epm::PathObligationKind::NestedEpm1)
}

pub open spec fn pending_fields_match_saved(
    actual: ObservedPending,
    model: saved::SourceObligation,
    progress: saved::SourceObligationProgress,
) -> bool {
    &&& actual.id == model.id
    &&& actual.id == progress.id
    &&& kind_fields_match(actual.kind, model.kind)
    &&& actual.payload_len == model.payload_len
    &&& anchor_fields_match(actual.tag_source, model.tag_source)
    &&& actual.payload_start == model.payload_start
    &&& actual.payload_end_exclusive as int
        == model.payload_start as int + model.payload_len as int
    &&& actual.remaining == progress.remaining
    &&& actual.awaiting_resolution == progress.awaiting_resolution
    &&& match model.status {
        saved::SourceObligationStatus::Payload => {
            !actual.awaiting_resolution && actual.payload_end_source.is_none()
        },
        saved::SourceObligationStatus::AwaitingResolution { payload_end } => {
            actual.awaiting_resolution
                && actual.payload_end_source.is_some()
                && anchor_fields_match(actual.payload_end_source.unwrap(), payload_end)
        },
    }
}

/// Fieldwise projection of one real `batch_restore_predecessor_source` call
/// onto the independently modeled `PersistentCheckpoints::get` result.
pub open spec fn predecessor_matches_saved_state(
    checkpoints: saved::SourceCheckpoints,
    run: batch::HistoricalRestoreRunImage,
    depth: usize,
    actual: ObservedPredecessor,
) -> bool {
    let selected = saved::saved_run_state_at_depth(checkpoints, saved_run(run), depth);
    let core = saved::batch_restore_predecessor_core(checkpoints, saved_run(run), depth);
    &&& selected.is_some()
    &&& core.is_some()
    &&& actual.state_id == core.unwrap().state_token
    &&& actual.depth == core.unwrap().depth
    &&& actual.stamp == core.unwrap().stamp
    &&& actual.open == core.unwrap().open
    &&& actual.fault_free == core.unwrap().fault_free
    &&& actual.parser_source.stamp == actual.stamp
    &&& actual.parser_source.path_bytes.len() == actual.depth as int
    &&& gate_fields_match(actual.gate, core.unwrap().gate)
    &&& match (
        actual.pending,
        core.unwrap().pending,
        selected.unwrap().current.selected_obligation,
    ) {
        (None, None, None) => true,
        // A dead/faulted saved state can retain an obligation frame, while
        // `batch_handle_source` suppresses pending progress for that state.
        (None, None, Some(_)) => !core.unwrap().fault_free,
        (Some(p), Some(progress), Some(obligation)) =>
            pending_fields_match_saved(p, obligation, progress),
        _ => false,
    }
}

/// The on-demand arena slice has the same prefix provenance as the formal
/// historical source tree. This is a separate Rust arena projection premise.
pub open spec fn predecessor_anchor_matches_prefix(
    run: batch::HistoricalRestoreRunImage,
    depth: usize,
    actual: ObservedPredecessor,
) -> bool {
    if depth == 0 {
        actual.last_source.is_none()
    } else {
        actual.last_source == Some(run.latest.positions[depth as int - 1])
    }
}

pub open spec fn predecessor_parser_bytes_match_prefix(
    run: batch::HistoricalRestoreRunImage,
    depth: usize,
    actual: ObservedPredecessor,
) -> bool {
    &&& actual.parser_source.path_bytes.len() == depth as int
    &&& forall|i: int| 0 <= i < depth as int ==> {
        let located = #[trigger] actual.parser_source.path_bytes[i];
        &&& located.byte == run.latest.bytes[i]
        &&& located.source.source == run.latest.positions[i].source
        &&& located.source.offset == run.latest.positions[i].offset
    }
}

/// The production parser snapshot reads `next_obligation` from the global
/// machine even for a saved handle. A checkpoint retains the counter at its
/// capture time; Leave restores its parser state while keeping the current
/// global counter. All other parser fields must match the saved handle.
pub open spec fn source_parser_matches_saved_checkpoint(
    source: epm::parser::ParserSourceSnapshot,
    checkpoint: epm::parser::ParserImage,
) -> bool {
    &&& checkpoint.next_obligation <= source.next_obligation
    &&& epm::parser::source_parser_matches(source,
        epm::parser::ParserImage { next_obligation: source.next_obligation, ..checkpoint })
}

/// The ghost checkpoint chooses fields from the predecessor observation.
/// Path length is already constrained by `historical_run_wf`, so it is not
/// repeated as a premise. Only a depth-zero Payload anchor needs an extra
/// pre-prefix source: a positive-depth predecessor already has `last_source`.
pub open spec fn checkpoint_matches_predecessor(
    run: batch::HistoricalRestoreRunImage,
    depth: usize,
    actual: ObservedPredecessor,
    checkpoint: epm::PathCheckpoint,
    empty_prefix_payload_end: Option<epm::SourceAnchor>,
) -> bool {
    &&& formal_gate_fields_match(actual.gate, checkpoint.gate)
    &&& source_parser_matches_saved_checkpoint(actual.parser_source, checkpoint.parser)
    &&& epm::parser_gate_matches(checkpoint.gate, checkpoint.parser)
    &&& match (actual.pending, checkpoint.pending) {
        (None, None) => true,
        (Some(observed), Some(formal)) => {
            &&& formal.id == observed.id
            &&& formal.kind == observed.kind
            &&& formal.payload_len as int == observed.payload_len as int
            &&& formal.tag_source == observed.tag_source
            &&& formal.payload_start == observed.payload_start
            &&& formal.awaiting_resolution == observed.awaiting_resolution
            &&& if observed.awaiting_resolution {
                observed.payload_end_source == Some(formal.payload_end)
            } else if depth > 0 {
                actual.last_source == Some(formal.payload_end)
            } else {
                empty_prefix_payload_end == Some(formal.payload_end)
            }
        },
        _ => false,
    }
}

/// One observation per retained prefix depth, in ascending depth order.
/// These are per-field source premises, not a checkpoint-equality premise.
pub open spec fn historical_predecessors_project(
    checkpoints: saved::SourceCheckpoints,
    run: batch::HistoricalRestoreRunImage,
    observations: Seq<ObservedPredecessor>,
    empty_prefix_payload_end: Option<epm::SourceAnchor>,
) -> bool {
    &&& batch::historical_run_wf(run)
    &&& observations.len() == batch::historical_run_width(run)
    &&& forall|i: int| 0 <= i < observations.len() ==> {
        let depth = (run.first_depth as int + i) as usize;
        let observed = #[trigger] observations[i];
        &&& predecessor_matches_saved_state(checkpoints, run, depth, observed)
        &&& predecessor_anchor_matches_prefix(run, depth, observed)
        &&& predecessor_parser_bytes_match_prefix(run, depth, observed)
        &&& checkpoint_matches_predecessor(
            run, depth, observed, run.checkpoints[i], empty_prefix_payload_end,
        )
    }
}

/// For every selected width within a coalesced run, the corresponding scalar
/// Leave selects the same saved token/depth as the on-demand Rust predecessor
/// observation. The historical checkpoint's gate and pending fields then
/// follow from the isolated per-field projection facts above.
pub proof fn historical_predecessors_select_scalar_leaves(
    checkpoints: saved::SourceCheckpoints,
    run: batch::HistoricalRestoreRunImage,
    observations: Seq<ObservedPredecessor>,
    empty_prefix_payload_end: Option<epm::SourceAnchor>,
    width: usize,
)
    requires
        historical_predecessors_project(checkpoints, run, observations, empty_prefix_payload_end),
        0 < width as int <= batch::historical_run_width(run),
    ensures forall|step: int| #![trigger observations[batch::historical_run_width(run) - step]] 1 <= step <= width as int ==> {
        let depth = (run.latest_depth as int - step + 1) as usize;
        let index = batch::historical_run_width(run) - step;
        let observed = observations[index];
        let selected = batch::selected_historical_checkpoint(run, step as usize);
        &&& selected.saved_state_id == observed.state_id
        &&& selected.checkpoint == run.checkpoints[index]
        &&& selected.checkpoint.path_len == observed.depth
        &&& saved::scalar_leave_selected_after(checkpoints, saved_run(run), step as nat)
            == saved::saved_run_state_at_depth(checkpoints, saved_run(run), depth)
        &&& checkpoint_matches_predecessor(
            run, depth, observed, selected.checkpoint, empty_prefix_payload_end,
        )
        &&& predecessor_anchor_matches_prefix(run, depth, observed)
        &&& run.current.positions[depth as int]
            == run.current.positions[run.current.positions.len() - step]
    },
{
    assert forall|step: int| #![trigger observations[batch::historical_run_width(run) - step]] 1 <= step <= width as int implies {
        let depth = (run.latest_depth as int - step + 1) as usize;
        let index = batch::historical_run_width(run) - step;
        let observed = observations[index];
        let selected = batch::selected_historical_checkpoint(run, step as usize);
        &&& selected.saved_state_id == observed.state_id
        &&& selected.checkpoint == run.checkpoints[index]
        &&& selected.checkpoint.path_len == observed.depth
        &&& saved::scalar_leave_selected_after(checkpoints, saved_run(run), step as nat)
            == saved::saved_run_state_at_depth(checkpoints, saved_run(run), depth)
        &&& checkpoint_matches_predecessor(
            run, depth, observed, selected.checkpoint, empty_prefix_payload_end,
        )
        &&& predecessor_anchor_matches_prefix(run, depth, observed)
        &&& run.current.positions[depth as int]
            == run.current.positions[run.current.positions.len() - step]
    } by {
        let index = batch::historical_run_width(run) - step;
        let depth = (run.latest_depth as int - step + 1) as usize;
        assert(0 <= index < observations.len());
        assert(depth as int == run.first_depth as int + index);
        assert(saved::saved_run_wf(saved_run(run)));
        assert(run.first_depth <= depth <= run.latest_depth);
        assert(step as nat == run.latest_depth as int - depth as int + 1);
        assert(predecessor_matches_saved_state(
            checkpoints, run, depth, observations[index],
        ));
        assert(predecessor_anchor_matches_prefix(run, depth, observations[index]));
        assert(checkpoint_matches_predecessor(
            run, depth, observations[index], run.checkpoints[index], empty_prefix_payload_end,
        ));
        assert(observations[index].depth == depth);
        assert(run.checkpoints[index].path_len == depth);
        assert(saved::saved_run_state_at_depth(checkpoints, saved_run(run), depth).is_some());
        saved::scalar_leave_selected_is_saved_predecessor(
            checkpoints, saved_run(run), depth, step as nat,
        );
        assert(run.current.positions.len() == run.latest_depth as int + 1);
        assert(run.current.positions.len() - step == depth as int);
    }
}

/// The first retained Payload checkpoint needs no separate base anchor when
/// its restored prefix is nonempty: its end is that prefix's observed last
/// source. `checkpoint_matches_predecessor` still states the fieldwise ghost
/// projection; this theorem does not prove that Rust chose the ghost fields.
pub proof fn positive_first_payload_end_is_restored_prefix_anchor(
    checkpoints: saved::SourceCheckpoints,
    run: batch::HistoricalRestoreRunImage,
    observations: Seq<ObservedPredecessor>,
)
    requires
        historical_predecessors_project(checkpoints, run, observations, None),
        run.first_depth > 0,
        observations[0].pending.is_some(),
        !observations[0].pending.unwrap().awaiting_resolution,
    ensures
        observations[0].last_source.is_some(),
        run.checkpoints[0].pending.is_some(),
        run.checkpoints[0].pending.unwrap().payload_end
            == observations[0].last_source.unwrap(),
        observations[0].last_source
            == Some(run.latest.positions[run.first_depth as int - 1]),
{
    assert(observations.len() == batch::historical_run_width(run));
    assert(observations.len() > 0);
    assert(predecessor_anchor_matches_prefix(run, run.first_depth, observations[0]));
    assert(checkpoint_matches_predecessor(
        run, run.first_depth, observations[0], run.checkpoints[0], None,
    ));
}

/// A two-Leave witness with no obligation: a separately projected current
/// handle has a different, SplitComplete state token, yet both selected ghost
/// checkpoints come from the saved token's depth-specific predecessors.
pub proof fn changed_current_token_two_leave_witness() {
    let first_anchor = epm::SourceAnchor { source: 4, offset: 20 };
    let second_anchor = epm::SourceAnchor { source: 4, offset: 21 };
    let gate = epm::PathGate {
        dead: false,
        stack_empty: true,
        split_complete: false,
        completed_segments: 0,
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    };
    let saved_gate = saved::SourceGate {
        dead: false,
        stack_empty: true,
        split_complete: false,
        completed_segments: 0,
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    };
    let saved_inner = saved::SourceInnerPath {
        status: saved::SourcePathStatus::Open,
        gate: saved_gate,
        selected_obligation: None,
        frame_history_id: 1,
    };
    let changed_inner = saved::SourceInnerPath {
        status: saved::SourcePathStatus::SplitComplete,
        gate: saved_gate,
        selected_obligation: None,
        frame_history_id: 2,
    };
    let checkpoints = saved::SourceCheckpoints {
        direct: seq![saved_inner, changed_inner],
        linked: Seq::empty(),
    };
    let run = batch::HistoricalRestoreRunImage {
        first_depth: 0,
        latest_depth: 1,
        saved_state_id: 0,
        current: epm::SourceTreeImage {
            bytes: seq![1u8, 2u8],
            positions: seq![first_anchor, second_anchor],
        },
        latest: epm::SourceTreeImage {
            bytes: seq![1u8],
            positions: seq![first_anchor],
        },
        checkpoints: seq![
            epm::PathCheckpoint {
                pending: None, gate, path_len: 0,
                parser: epm::parser::initial_parser(),
            },
            epm::PathCheckpoint {
                pending: None, gate, path_len: 1,
                parser: epm::parser::ParserImage {
                    stamp: 1, ..epm::parser::initial_parser()
                },
            },
        ],
    };
    let first_parser = epm::parser::initial_parser_source_snapshot();
    let second_parser = epm::parser::ParserSourceSnapshot {
        stamp: 1,
        path_bytes: seq![epm::parser::ParserLocated {
            byte: 1,
            source: epm::parser::ParserSource {
                source: first_anchor.source, offset: first_anchor.offset,
            },
        }],
        ..first_parser
    };
    let first = ObservedPredecessor {
        state_id: 0,
        depth: 0,
        last_source: None,
        stamp: 0,
        open: true,
        fault_free: true,
        gate,
        owner_boundary_end: None,
        pending: None,
        parser_source: first_parser,
    };
    let second = ObservedPredecessor {
        state_id: 0,
        depth: 1,
        last_source: Some(first_anchor),
        stamp: 1,
        open: true,
        fault_free: true,
        gate,
        owner_boundary_end: None,
        pending: None,
        parser_source: second_parser,
    };
    let observations = seq![first, second];
    let current_state_token = 2usize;
    assert(current_state_token != run.saved_state_id);
    assert(run.current.bytes.len() == 2);
    assert(saved::checkpoint_get(checkpoints, current_state_token, 2).is_some());
    assert(saved::checkpoint_get(checkpoints, current_state_token, 2)
        .unwrap().current == changed_inner);
    assert(changed_inner.frame_history_id != saved_inner.frame_history_id);
    assert(run.first_depth <= run.latest_depth);
    assert(run.latest_depth < usize::MAX);
    assert(run.current.bytes.len() == run.current.positions.len());
    assert(run.latest.bytes.len() == run.latest.positions.len());
    assert(run.current.bytes.len() == run.latest_depth as int + 1);
    assert(run.latest.bytes == run.current.bytes.subrange(0, run.latest_depth as int));
    assert(run.latest.positions == run.current.positions.subrange(0, run.latest_depth as int));
    assert(batch::historical_run_width(run) == 2);
    assert(run.checkpoints.len() == batch::historical_run_width(run));
    assert forall|i: int| 0 <= i < run.checkpoints.len() implies
        #[trigger] run.checkpoints[i].path_len as int == run.first_depth as int + i by {
        if i == 0 {
            assert(run.checkpoints[i].path_len == 0);
        } else {
            assert(i == 1);
            assert(run.checkpoints[i].path_len == 1);
        }
    }
    assert(batch::historical_run_wf(run));
    assert(predecessor_matches_saved_state(checkpoints, run, 0, first));
    assert(predecessor_matches_saved_state(checkpoints, run, 1, second));
    assert(predecessor_anchor_matches_prefix(run, 0, first));
    assert(predecessor_anchor_matches_prefix(run, 1, second));
    assert(checkpoint_matches_predecessor(
        run, 0, first, run.checkpoints[0], None,
    ));
    assert(checkpoint_matches_predecessor(
        run, 1, second, run.checkpoints[1], None,
    ));
    assert forall|i: int| 0 <= i < observations.len() implies {
        let depth = (run.first_depth as int + i) as usize;
        let observed = #[trigger] observations[i];
        &&& predecessor_matches_saved_state(checkpoints, run, depth, observed)
        &&& predecessor_anchor_matches_prefix(run, depth, observed)
        &&& checkpoint_matches_predecessor(
            run, depth, observed, run.checkpoints[i], None,
        )
    } by {
        if i == 0 {
            assert(observations[i] == first);
        } else {
            assert(i == 1);
            assert(observations[i] == second);
        }
    }
    historical_predecessors_select_scalar_leaves(
        checkpoints, run, observations, None, 2,
    );
    assert(batch::selected_historical_checkpoint(run, 2).saved_state_id == 0);
    assert(batch::selected_historical_checkpoint(run, 2).checkpoint.path_len == 0);
}

}

fn main() {}
