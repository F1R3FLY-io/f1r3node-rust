//! Source fields checked at each successful `epathmap_step` loop boundary.
//!
//! `EPathMapLoopStateSource::matches_successor` in production checks these
//! cursor, owner, path-shape, wait, and physical-source facts. The resulting
//! formal tick is conditional on separately named Rust-to-Verus projections:
//! an ordinary scalar reducer `event_step`, a Forward scalar expansion, or
//! the modeled restore history's projection from Rust. Forward and Unwind
//! run witnesses are selected by the observed compressed path source shape;
//! no source predicate below simply assumes `loop_internal_tick` or
//! `loop_stop_ready`.
//!
//! The production restore vector stores compressed runs. Its `restore_len`
//! must not be equated with `EpmFrame.path.restore.len()`, which counts virtual
//! checkpoints. The batch source-shape relations below preserve that split.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_persistent_state_source as persistent,
    rpb3_epathmap_end_source_selector as end_source, rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
    rpb3_epm_batch_cursor_source_extraction as line_source,
    rpb3_epm_loop_continuation_source as loop_model,
    rpb3_epm_push_live_action_refinement as push_live,
    rpb3_epm_push_source_action_refinement as push_source,
    rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore, rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_runtime_frame_source_refinement as runtime,
};

verus! {

pub struct EpmLoopCursorSource {
    pub mode: epm::EpmMode,
    pub declared_value_count: usize,
    pub map_values_seen: usize,
    pub event_index: usize,
    pub value_table_offset: usize,
    pub snapshot_len: usize,
}

pub struct EpmLoopRestoreShape {
    pub state_id: usize,
    pub latest_len: usize,
    pub first_depth: usize,
}

pub struct EpmLoopPathShape {
    pub state_id: usize,
    pub depth: usize,
    pub restore_len: usize,
    pub top: Option<EpmLoopRestoreShape>,
    pub prior: Option<EpmLoopRestoreShape>,
    pub root_left: bool,
    pub source_present: bool,
}

pub enum EpmLoopWaitSource {
    None,
    MapValue,
    Obligation { id: u64, kind: epm::PathObligationKind },
}

pub struct EpmLoopObligationSource {
    pub id: u64,
    pub kind: epm::PathObligationKind,
    pub payload_len: u64,
    pub tag_source: epm::SourceAnchor,
    pub remaining: u64,
    pub awaiting_resolution: bool,
}

/// Owner identity and field are retained as scalar facts even though the
/// current EpmFrame model does not itself carry a Rust owner pointer.
pub struct EpmLoopStateSource {
    pub owner_id: usize,
    pub field: u32,
    pub region_start: epm::SourceAnchor,
    pub snapshot_len: usize,
    pub cursor: EpmLoopCursorSource,
    pub topology: Option<line_source::LineCursorSourcePosition>,
    pub path: EpmLoopPathShape,
    /// The production `value` reducer selects `root_path(state)`; this is its
    /// observed gate, which can differ from the focused current-path gate.
    pub root_gate: persistent::SourceGate,
    pub obligation: Option<EpmLoopObligationSource>,
    pub last_source: epm::SourceAnchor,
    pub waiting: EpmLoopWaitSource,
    pub waiting_child_len: Option<usize>,
    pub entries_stable: bool,
}

pub enum EpmLoopBranch {
    AcceptedForward { start: usize, width: usize },
    AcceptedUnwind { start: usize, width: usize },
    ScalarEdgeNoChild { byte: u8 },
    ScalarLeave,
    ScalarValueNoBody {
        has_value: bool,
        reducer_stable: Option<bool>,
        root_stable: bool,
    },
    PushEdge { byte: u8 },
    PushValue {
        has_value: bool,
        reducer_stable: Option<bool>,
        root_stable: bool,
        body_range: epm::ByteSpan,
    },
    Complete,
}

pub open spec fn source_path_forward(
    before: EpmLoopPathShape, after: EpmLoopPathShape, width: usize,
) -> bool {
    if width == 0 || before.root_left || !before.source_present
        || before.depth as int + width as int > usize::MAX as int
    { false } else {
        let depth = (before.depth as int + width as int) as usize;
        let coalesced = match before.top {
            Some(top) => top.state_id == before.state_id,
            None => false,
        };
        let increment: int = if coalesced { 0 } else { 1 };
        &&& before.restore_len as int + increment <= usize::MAX as int
        &&& after.state_id == before.state_id
        &&& after.depth == depth
        &&& after.restore_len as int == before.restore_len as int + increment
        &&& after.top == Some(EpmLoopRestoreShape {
            state_id: before.state_id,
            latest_len: (depth - 1) as usize,
            first_depth: if coalesced { before.top.unwrap().first_depth }
                         else { before.depth },
        })
        &&& after.prior == if coalesced { before.prior } else { before.top }
        &&& after.root_left == before.root_left
        &&& after.source_present == before.source_present
    }
}

pub open spec fn source_path_unwind(
    before: EpmLoopPathShape, after: EpmLoopPathShape, width: usize,
) -> bool {
    match before.top {
        None => false,
        Some(run) => {
            let run_width = run.latest_len as int - run.first_depth as int + 1;
            if width == 0 || before.root_left || run.latest_len < run.first_depth
                || (width as int) > run_width
                || run.latest_len as int + 1 != before.depth as int
                || width > before.depth
            { false } else {
                let partial = (width as int) < run_width;
                let decrement: int = if partial { 0 } else { 1 };
                &&& before.restore_len as int >= decrement
                &&& after.state_id == run.state_id
                &&& after.depth as int == before.depth as int - width as int
                &&& after.restore_len as int == before.restore_len as int - decrement
                &&& after.top == if partial {
                    Some(EpmLoopRestoreShape {
                        state_id: run.state_id,
                        latest_len: (run.latest_len as int - width as int) as usize,
                        first_depth: run.first_depth,
                    })
                } else { before.prior }
                &&& (!partial || after.prior == before.prior)
                &&& after.root_left == before.root_left
                &&& after.source_present == before.source_present
            }
        },
    }
}

/// Scalar `path.edge` may change the reducer state token and can accept a
/// sourced byte when no bulk source window exists. Its retained restore
/// shape otherwise follows the same one-byte coalescing rule as Forward.
pub open spec fn source_path_scalar_edge(
    before: EpmLoopPathShape, after: EpmLoopPathShape,
) -> bool {
    if before.root_left || before.depth == usize::MAX {
        false
    } else {
        let coalesced = match before.top {
            Some(top) => top.state_id == before.state_id,
            None => false,
        };
        let increment: int = if coalesced { 0 } else { 1 };
        &&& before.restore_len as int + increment <= usize::MAX as int
        &&& after.depth as int == before.depth as int + 1
        &&& after.restore_len as int == before.restore_len as int + increment
        &&& after.top == Some(EpmLoopRestoreShape {
            state_id: before.state_id,
            latest_len: before.depth,
            first_depth: if coalesced { before.top.unwrap().first_depth }
                         else { before.depth },
        })
        &&& after.prior == if coalesced { before.prior } else { before.top }
        &&& after.root_left == before.root_left
        &&& after.source_present == before.source_present
    }
}

pub open spec fn source_path_scalar_leave(
    before: EpmLoopPathShape, after: EpmLoopPathShape,
) -> bool {
    if before.root_left {
        false
    } else {
        match before.top {
            Some(_) => source_path_unwind(before, after, 1),
            None => after == EpmLoopPathShape { root_left: true, ..before },
        }
    }
}

pub open spec fn checked_loop_successor(
    before: EpmLoopStateSource,
    after: EpmLoopStateSource,
    branch: EpmLoopBranch,
    physical_source: epm::SourceAnchor,
) -> bool {
    let events = match branch {
        EpmLoopBranch::AcceptedForward { width, .. } => 2 * width as int,
        EpmLoopBranch::AcceptedUnwind { width, .. } => width as int,
        EpmLoopBranch::Complete => -1,
        _ => 1,
    };
    let map_values: int = match branch {
        EpmLoopBranch::PushValue { has_value, .. } =>
            if has_value && after.cursor.mode == epm::EpmMode::Map { 1 } else { 0 },
        _ => 0,
    };
    &&& events > 0
    &&& before.owner_id == after.owner_id
    &&& before.field == after.field
    &&& before.region_start == after.region_start
    &&& before.snapshot_len == after.snapshot_len
    &&& before.cursor.mode == after.cursor.mode
    &&& before.cursor.declared_value_count == after.cursor.declared_value_count
    &&& before.cursor.snapshot_len == after.cursor.snapshot_len
    &&& after.cursor.snapshot_len == after.snapshot_len
    &&& before.cursor.event_index as int + events <= usize::MAX as int
    &&& after.cursor.event_index as int == before.cursor.event_index as int + events
    &&& before.cursor.map_values_seen as int + map_values <= usize::MAX as int
    &&& after.cursor.map_values_seen as int
        == before.cursor.map_values_seen as int + map_values
    &&& before.cursor.value_table_offset <= after.cursor.value_table_offset
    &&& after.cursor.value_table_offset <= after.cursor.snapshot_len
    &&& before.waiting is None
    &&& before.waiting_child_len.is_none()
    &&& after.last_source == physical_source
    &&& match branch {
        EpmLoopBranch::AcceptedForward { start, width } => {
            &&& source_path_forward(before.path, after.path, width)
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& start as int + width as int <= usize::MAX as int
            &&& after.waiting is None
            &&& after.waiting_child_len.is_none()
            &&& after.entries_stable == before.entries_stable
        },
        EpmLoopBranch::AcceptedUnwind { start, width } => {
            &&& source_path_unwind(before.path, after.path, width)
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& start as int + width as int <= usize::MAX as int
            &&& after.waiting is None
            &&& after.waiting_child_len.is_none()
            &&& after.entries_stable == before.entries_stable
        },
        EpmLoopBranch::ScalarEdgeNoChild { .. } => {
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& source_path_scalar_edge(before.path, after.path)
            &&& after.waiting is None
            &&& after.waiting_child_len.is_none()
            &&& after.entries_stable == before.entries_stable
        },
        EpmLoopBranch::ScalarLeave => {
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& source_path_scalar_leave(before.path, after.path)
            &&& after.waiting is None
            &&& after.waiting_child_len.is_none()
            &&& after.entries_stable == before.entries_stable
        },
        EpmLoopBranch::ScalarValueNoBody {
            has_value, reducer_stable, root_stable,
        } => {
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& before.path == after.path
            &&& before.root_gate == after.root_gate
            &&& root_stable == before.root_gate.stable
            &&& before.obligation == after.obligation
            &&& reducer_stable == if has_value { Some(root_stable) } else { None }
            &&& after.entries_stable == (before.entries_stable && match reducer_stable {
                Some(stable) => stable,
                None => true,
            })
            &&& after.waiting is None
            &&& after.waiting_child_len.is_none()
        },
        EpmLoopBranch::PushEdge { .. } => {
            &&& before.cursor.value_table_offset == after.cursor.value_table_offset
            &&& source_path_scalar_edge(before.path, after.path)
            &&& after.entries_stable == before.entries_stable
            &&& match (after.waiting, after.obligation) {
                (
                    EpmLoopWaitSource::Obligation { id, kind },
                    Some(progress),
                ) => {
                    &&& progress.id == id
                    &&& progress.kind == kind
                    &&& progress.remaining == 0
                    &&& progress.awaiting_resolution
                },
                _ => false,
            }
            &&& after.waiting_child_len.is_some()
        },
        EpmLoopBranch::PushValue {
            has_value, reducer_stable, root_stable, body_range,
        } => {
            &&& before.path == after.path
            &&& before.root_gate == after.root_gate
            &&& root_stable == before.root_gate.stable
            &&& before.obligation == after.obligation
            &&& before.cursor.mode == epm::EpmMode::Map
            &&& has_value
            &&& before.cursor.value_table_offset < body_range.start
            &&& body_range.start <= body_range.end
            &&& body_range.end == after.cursor.value_table_offset
            &&& body_range.end <= before.snapshot_len
            &&& after.waiting_child_len.is_some()
            &&& after.waiting_child_len.unwrap() as int
                == body_range.end as int - body_range.start as int
            &&& reducer_stable == Some(root_stable)
            &&& after.entries_stable == (before.entries_stable && match reducer_stable {
                Some(stable) => stable,
                None => true,
            })
            &&& after.waiting is MapValue
            &&& after.waiting_child_len.is_some()
            &&& after.obligation.is_none()
        },
        EpmLoopBranch::Complete => false,
    }
}

/// The Rust Value reducer returns the observed root gate's stability. The
/// checked successor binds the return to that source fact; `source_frame_matches`
/// binds the fact to the modeled gate under the Rust-to-Verus projection.
pub proof fn checked_scalar_value_source_has_exact_stability(
    before: EpmLoopStateSource,
    after: EpmLoopStateSource,
    before_frame: epm::EpmFrame,
    after_frame: epm::EpmFrame,
    physical_source: epm::SourceAnchor,
    has_value: bool,
    reducer_stable: Option<bool>,
    root_stable: bool,
)
    requires
        source_frame_matches(before, before_frame),
        source_frame_matches(after, after_frame),
        checked_loop_successor(
            before, after,
            EpmLoopBranch::ScalarValueNoBody {
                has_value, reducer_stable, root_stable,
            },
            physical_source,
        ),
    ensures
        after_frame.entries_stable
            == (before_frame.entries_stable
                && (!has_value || before_frame.path.gate.stable)),
{}

/// The checked Map Push consumes the length-prefixed value-table interval and
/// publishes exactly its body length. The ACT attachment computes the selected
/// body from the bytes and the cursor's consumed-event prefix. The remaining
/// source premise projects Rust `next_event` to that modeled cursor event.
pub proof fn checked_push_value_source_binds_selected_body(
    before: EpmLoopStateSource,
    after: EpmLoopStateSource,
    before_frame: epm::EpmFrame,
    after_frame: epm::EpmFrame,
    physical_source: epm::SourceAnchor,
    reducer_stable: Option<bool>,
    root_stable: bool,
    source: line_source::MapValueNextEventSource,
)
    requires
        source_frame_matches(before, before_frame),
        source_frame_matches(after, after_frame),
        checked_loop_successor(
            before, after,
            EpmLoopBranch::PushValue {
                has_value: true, reducer_stable, root_stable,
                body_range: source.body,
            },
            physical_source,
        ),
        line_source::selected_map_value_attachment_source(before_frame, source),
    ensures
        epm::cursor_event_at(before_frame, before_frame.cursor.at)
            == epm::wire_event(line_source::map_value_event_from_source(source)),
        before.cursor.mode == epm::EpmMode::Map,
        before.cursor.value_table_offset < source.body.start,
        source.body.end == after.cursor.value_table_offset,
        after_frame.cursor.at == source.event_index_after,
        after_frame.cursor.map_values_seen == source.map_values_seen_after,
        source.post_value_table_offset == after_frame.cursor.value_table_offset,
        source.body.end as int <= before.snapshot_len as int,
        after.waiting_child_len.is_some(),
        after.waiting_child_len.unwrap() as int
            == source.body.end as int - source.body.start as int,
        after_frame.entries_stable
            == (before_frame.entries_stable && before_frame.path.gate.stable),
{
    line_source::selected_map_value_attachment_binds_body(before_frame, source);
}

/// Production reports compressed restore runs, whereas the EPM model keeps
/// one virtual checkpoint per edge. Bind the observed top run to the exact
/// suffix of virtual checkpoint *depths*, without equating the two lengths or
/// guessing checkpoint payload/gate contents from the compact observation.
pub open spec fn source_restore_history_matches(
    observed: EpmLoopPathShape, path: epm::PathTopology,
) -> bool {
    let virtual_len = path.restore.len();
    &&& observed.depth as int == path.path_bytes.len()
    &&& path.path_positions.len() == path.path_bytes.len()
    &&& virtual_len == path.path_bytes.len()
    &&& observed.restore_len as int <= virtual_len
    &&& (observed.restore_len == 0) == observed.top.is_none()
    &&& (observed.restore_len < 2) == observed.prior.is_none()
    &&& match observed.top {
        None => virtual_len == 0,
        Some(top) => {
            let width = top.latest_len as int - top.first_depth as int + 1;
            &&& top.first_depth <= top.latest_len
            &&& top.latest_len as int + 1 == observed.depth as int
            &&& width <= virtual_len
            &&& forall|i: int| 0 <= i < width ==>
                #[trigger] path.restore[virtual_len - width + i].path_len as int
                    == top.first_depth as int + i
        },
    }
}

pub proof fn observed_top_run_selects_virtual_restore_suffix(
    observed: EpmLoopPathShape,
    path: epm::PathTopology,
)
    requires
        source_restore_history_matches(observed, path),
        observed.top.is_some(),
    ensures {
        let top = observed.top.unwrap();
        let width = top.latest_len as int - top.first_depth as int + 1;
        &&& width > 0
        &&& width <= path.restore.len()
        &&& forall|i: int| 0 <= i < width ==>
            #[trigger] path.restore[path.restore.len() - width + i].path_len as int
                == top.first_depth as int + i
    },
{}

/// A two-edge coalesced run demonstrates that compressed length one and
/// virtual history length two can satisfy this binding simultaneously.
pub proof fn nonempty_restore_history_source_has_witness()
    ensures exists|observed: EpmLoopPathShape, path: epm::PathTopology|
        observed.restore_len == 1
        && path.restore.len() == 2
        && source_restore_history_matches(observed, path),
{
    let initial = epm::initial_epm_path();
    let first = epm::append_plain_path_with_gate(
        initial, 0x02, epm::SourceAnchor { source: 1, offset: 0 },
        epm::PathGate { completed_segments: 1, ..initial.gate },
    );
    let path = epm::append_plain_path_with_gate(
        first, 0x02, epm::SourceAnchor { source: 1, offset: 1 },
        epm::PathGate { completed_segments: 2, ..initial.gate },
    );
    let checkpoint0 = path.restore[0];
    let checkpoint1 = path.restore[1];
    let observed = EpmLoopPathShape {
        state_id: 8, depth: 2, restore_len: 1,
        top: Some(EpmLoopRestoreShape {
            state_id: 7, latest_len: 1, first_depth: 0,
        }),
        prior: None, root_left: false, source_present: false,
    };
    assert forall|i: int| 0 <= i < 2 implies
        #[trigger] path.restore[i].path_len as int == i by {
        if i == 0 { assert(path.restore[i] == checkpoint0); }
        else { assert(i == 1); assert(path.restore[i] == checkpoint1); }
    }
    assert(source_restore_history_matches(observed, path));
}

/// The source gate is read from Rust's selected root path. Its correspondence
/// to the EPM model is an explicit, still-unproved Rust-to-Verus projection;
/// the loop check itself only compares two concrete Rust observations.
pub open spec fn source_root_gate_matches(
    observed: persistent::SourceGate, modeled: epm::PathGate,
) -> bool {
    &&& observed.dead == modeled.dead
    &&& observed.stack_empty == modeled.stack_empty
    &&& observed.split_complete == modeled.split_complete
    &&& observed.completed_segments as int == modeled.completed_segments as int
    &&& observed.first_is_list == modeled.first_is_list
    &&& observed.stable == modeled.stable
    &&& observed.escape_allowed == modeled.escape_allowed
}

pub open spec fn source_frame_matches(
    observed: EpmLoopStateSource, frame: epm::EpmFrame,
) -> bool {
    &&& !frame.cursor.done
    &&& observed.cursor.mode == frame.mode
    &&& observed.cursor.declared_value_count == frame.cursor.declared_value_count
    &&& observed.cursor.map_values_seen == frame.cursor.map_values_seen
    &&& observed.cursor.event_index == frame.cursor.at
    &&& observed.cursor.value_table_offset == frame.cursor.value_table_offset
    &&& observed.cursor.snapshot_len as int == frame.snapshot.bytes.len()
    &&& observed.snapshot_len == observed.cursor.snapshot_len
    &&& observed.region_start == epm::indexed_position_at(frame.snapshot, 0)
    &&& source_restore_history_matches(observed.path, frame.path)
    &&& observed.path.root_left == frame.path.root_left
    &&& source_root_gate_matches(observed.root_gate, frame.path.gate)
    &&& observed.last_source == frame.last_source
    &&& observed.entries_stable == frame.entries_stable
    &&& observed.waiting_child_len.is_some() == frame.wait.is_some()
    &&& match (observed.obligation, frame.path.pending) {
        (None, None) => true,
        (Some(source), Some(pending)) => {
            &&& source.id == pending.id
            &&& source.kind == pending.kind
            &&& source.payload_len as int == pending.payload_len as int
            &&& source.tag_source == pending.tag_source
            &&& source.awaiting_resolution == pending.awaiting_resolution
            &&& pending.payload_start as int <= frame.path.path_bytes.len()
            &&& source.remaining as int
                + frame.path.path_bytes.len() - pending.payload_start as int
                == pending.payload_len as int
        },
        _ => false,
    }
    &&& match observed.waiting {
        EpmLoopWaitSource::None => frame.wait.is_none(),
        EpmLoopWaitSource::MapValue => frame.wait is Some
            && frame.wait.unwrap() is MapValue,
        EpmLoopWaitSource::Obligation { id, kind } => match frame.wait {
            Some(epm::EpmWait::Obligation {
                id: formal_id, kind: formal_kind, ..
            }) => id == formal_id && kind == formal_kind,
            _ => false,
        },
    }
}

/// This witness exercises the scalar-only case: a byte from ACT has a
/// physical anchor although there is no contiguous bulk source window, and
/// the reducer state token can change after the edge.
pub proof fn scalar_edge_without_bulk_window_has_source_witness()
    ensures exists|before: EpmLoopPathShape, after: EpmLoopPathShape|
        !before.source_present
        && before.state_id != after.state_id
        && source_path_scalar_edge(before, after)
        && !source_path_forward(before, after, 1),
{
    let before = EpmLoopPathShape {
        state_id: 4, depth: 0, restore_len: 0,
        top: None, prior: None, root_left: false, source_present: false,
    };
    let after = EpmLoopPathShape {
        state_id: 5, depth: 1, restore_len: 1,
        top: Some(EpmLoopRestoreShape {
            state_id: 4, latest_len: 0, first_depth: 0,
        }),
        prior: None, root_left: false, source_present: false,
    };
    assert(source_path_scalar_edge(before, after));
    assert(!source_path_forward(before, after, 1));
}

pub proof fn scalar_edge_source_shape_refines_reducer_path_length(
    before_source: EpmLoopStateSource,
    after_source: EpmLoopStateSource,
    before_frame: epm::EpmFrame,
    after_frame: epm::EpmFrame,
)
    requires
        source_frame_matches(before_source, before_frame),
        source_frame_matches(after_source, after_frame),
        source_path_scalar_edge(before_source.path, after_source.path),
    ensures
        after_frame.path.path_bytes.len()
            == before_frame.path.path_bytes.len() + 1,
        after_source.path.top == Some(EpmLoopRestoreShape {
            state_id: before_source.path.state_id,
            latest_len: before_source.path.depth,
            first_depth: if before_source.path.top.is_some()
                && before_source.path.top.unwrap().state_id == before_source.path.state_id {
                before_source.path.top.unwrap().first_depth
            } else { before_source.path.depth },
        }),
{}

pub proof fn scalar_leave_source_shape_refines_reducer_path_length(
    before_source: EpmLoopStateSource,
    after_source: EpmLoopStateSource,
    before_frame: epm::EpmFrame,
    after_frame: epm::EpmFrame,
)
    requires
        source_frame_matches(before_source, before_frame),
        source_frame_matches(after_source, after_frame),
        source_path_scalar_leave(before_source.path, after_source.path),
    ensures
        before_source.path.top.is_some() ==>
            after_frame.path.path_bytes.len() + 1
                == before_frame.path.path_bytes.len(),
        before_source.path.top.is_none() ==>
            after_frame.path.path_bytes.len()
                == before_frame.path.path_bytes.len()
                && after_frame.path.root_left,
{}

pub proof fn scalar_value_source_shape_preserves_reducer_path_summary(
    before_source: EpmLoopStateSource,
    after_source: EpmLoopStateSource,
    before_frame: epm::EpmFrame,
    after_frame: epm::EpmFrame,
)
    requires
        source_frame_matches(before_source, before_frame),
        source_frame_matches(after_source, after_frame),
        before_source.path == after_source.path,
        before_source.obligation == after_source.obligation,
    ensures
        before_frame.path.path_bytes.len() == after_frame.path.path_bytes.len(),
        before_frame.path.root_left == after_frame.path.root_left,
        before_frame.path.pending.is_some() == after_frame.path.pending.is_some(),
        match (before_frame.path.pending, after_frame.path.pending) {
            (Some(before_pending), Some(after_pending)) =>
                before_pending.id == after_pending.id
                && before_pending.kind == after_pending.kind,
            _ => true,
        },
{}

pub open spec fn scalar_event_physical_source(
    before: epm::EpmFrame, event: epm::EpmEvent,
) -> epm::SourceAnchor {
    match event {
        epm::EpmEvent::Edge { source_offset, .. } =>
            epm::trie_position(before, source_offset),
        epm::EpmEvent::Leave { source_offset }
        | epm::EpmEvent::Value { source_offset, .. } =>
            epm::topology_position(before, source_offset),
        epm::EpmEvent::End => before.last_source,
    }
}

pub proof fn checked_loop_successor_has_exact_cursor_delta(
    before: EpmLoopStateSource, after: EpmLoopStateSource,
    branch: EpmLoopBranch, physical_source: epm::SourceAnchor,
)
    requires checked_loop_successor(before, after, branch, physical_source),
    ensures
        before.owner_id == after.owner_id,
        before.field == after.field,
        before.region_start == after.region_start,
        after.last_source == physical_source,
        after.cursor.event_index > before.cursor.event_index,
        match branch {
            EpmLoopBranch::AcceptedForward { width, .. } =>
                after.cursor.event_index as int == before.cursor.event_index as int + 2 * width as int,
            EpmLoopBranch::AcceptedUnwind { width, .. } =>
                after.cursor.event_index as int == before.cursor.event_index as int + width as int,
            EpmLoopBranch::Complete => false,
            _ => after.cursor.event_index as int == before.cursor.event_index as int + 1,
        },
{}

/// The checked scalar-source predicate itself is satisfiable; this fixture
/// does not claim extraction of a complete Rust EPathMap invocation.
pub proof fn checked_scalar_leave_source_has_witness()
    ensures exists|before: EpmLoopStateSource, after: EpmLoopStateSource,
        physical: epm::SourceAnchor| checked_loop_successor(
            before, after, EpmLoopBranch::ScalarLeave, physical,
        ),
{
    let physical = epm::SourceAnchor { source: 1, offset: 7 };
    let cursor = EpmLoopCursorSource {
        mode: epm::EpmMode::Set,
        declared_value_count: 0,
        map_values_seen: 0,
        event_index: 0,
        value_table_offset: 2,
        snapshot_len: 10,
    };
    let before = EpmLoopStateSource {
        owner_id: 3,
        field: 0,
        region_start: epm::SourceAnchor { source: 1, offset: 0 },
        snapshot_len: 10,
        cursor,
        topology: None,
        path: EpmLoopPathShape {
            state_id: 0, depth: 1, restore_len: 0,
            top: None, prior: None, root_left: false, source_present: true,
        },
        root_gate: persistent::SourceGate {
            dead: false, stack_empty: true, split_complete: false,
            completed_segments: 1, first_is_list: false,
            stable: true, escape_allowed: false,
        },
        obligation: None,
        last_source: epm::SourceAnchor { source: 1, offset: 6 },
        waiting: EpmLoopWaitSource::None,
        waiting_child_len: None,
        entries_stable: true,
    };
    let after = EpmLoopStateSource {
        cursor: EpmLoopCursorSource { event_index: 1, ..cursor },
        path: EpmLoopPathShape { root_left: true, ..before.path },
        last_source: physical,
        ..before
    };
    assert(checked_loop_successor(
        before, after, EpmLoopBranch::ScalarLeave, physical,
    ));
}

pub open spec fn linked_scalar_after(
    before: integrated::LiveRuntimeProductState,
    reducer_after: epm::EpmFrame,
) -> epm::LinkedEpmState
    recommends integrated::product_linked_top(before.source.product).is_some(),
        before.source.product.bridge.admission.frames.len() > 0,
{
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    let admission_after = admission::AdmissionState {
        frames: prior.admission.frames.update(
            prior.admission.frames.len() - 1,
            epm::epm_advanced_admission_cursor(
                prior.admission.frames.last(), reducer_after.cursor.value_table_offset,
            ),
        ),
        arena: prior.admission.arena,
        root_end: prior.admission.root_end,
        pushed: prior.admission.pushed,
        completed: prior.admission.completed,
        next_obligation_id: prior.admission.next_obligation_id,
    };
    epm::LinkedEpmState {
        epm: reducer_after,
        admission: admission_after,
        indexed_regions: prior.indexed_regions,
    }
}

/// Construct the internal live successor from the reducer image. No caller
/// supplies the product or live poststate as a relation premise.
pub open spec fn scalar_live_after(
    before: integrated::LiveRuntimeProductState,
    reducer_after: epm::EpmFrame,
) -> integrated::LiveRuntimeProductState
    recommends integrated::product_linked_top(before.source.product).is_some(),
        before.source.product.epm_frames.len() > 0,
        before.source.product.bridge.admission.frames.len() > 0,
{
    let linked = linked_scalar_after(before, reducer_after);
    let product = unwind_product::product_after_internal_epm_continue(
        before.source.product, linked,
    );
    integrated::LiveRuntimeProductState {
        source: integrated::LiveSourceProductState {
            product,
            runtime: runtime::runtime_after_admission_step(
                before.source.runtime, linked.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

/// These are the unbound scalar Rust projection facts: ACT selected `event`,
/// `path.edge/leave/value` produced `reducer_after`, and the constructed
/// admission cursor remains well formed. They are narrower than an assumed
/// linked, product, live, or loop transition.
pub open spec fn rust_scalar_reducer_projection(
    before: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
    reducer_after: epm::EpmFrame,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || integrated::product_linked_top(before.source.product).is_none()
        || before.source.product.epm_frames.len() == 0
    { false } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        if prior.admission.frames.len() == 0 { false } else {
            let next = linked_scalar_after(before, reducer_after);
            &&& admission::admission_state_wf(prior.admission)
            &&& epm::linked_indexed_sources(prior)
            &&& prior.admission.frames.last().frame_kind == 3
            &&& prior.admission.frames.last().waiting_id.is_none()
            &&& prior.epm.snapshot == prior.indexed_regions.last()
            &&& prior.admission.frames.last().cursor
                <= prior.epm.cursor.value_table_offset
            &&& epm::event_step(
                prior.epm, reducer_after, event,
                prior.admission.next_obligation_id, epm::EpmAction::Continue,
            )
            &&& admission::admission_state_wf(next.admission)
            &&& epm::linked_indexed_sources(next)
        }
    }
}

pub proof fn scalar_reducer_projection_yields_internal_live_step(
    before: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
    reducer_after: epm::EpmFrame,
)
    requires rust_scalar_reducer_projection(before, event, reducer_after),
    ensures unwind_product::live_runtime_internal_epm_unwind_step(
        before, scalar_live_after(before, reducer_after), event,
    ),
{
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    let linked = linked_scalar_after(before, reducer_after);
    let after = scalar_live_after(before, reducer_after);
    assert(epm::linked_epm_continue(prior, linked, event));
    assert(integrated::product_linked_top(after.source.product) == Some(linked));
    assert(after.source.product.accepted_epm_sources
        == integrated::product_epm_receipts_carried(
            before.source.product, after.source.product,
        ));
    assert(integrated::product_epm_continue_step(
        before.source.product, after.source.product, event,
    ));
}

pub proof fn scalar_reducer_projection_is_live_action(
    before: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
    reducer_after: epm::EpmFrame,
)
    requires rust_scalar_reducer_projection(before, event, reducer_after),
    ensures integrated::live_runtime_product_step(
        before,
        scalar_live_after(before, reducer_after),
        integrated::LiveRuntimeProductAction::EpmContinue { event },
    ),
{
    scalar_reducer_projection_yields_internal_live_step(before, event, reducer_after);
}

pub struct EpmScalarLoopSourceObservation {
    pub before: EpmLoopStateSource,
    pub after: EpmLoopStateSource,
    pub branch: EpmLoopBranch,
    pub event: epm::EpmEvent,
    pub physical_source: epm::SourceAnchor,
    pub batch_declined: bool,
    pub ghost reducer_after: epm::EpmFrame,
}

pub open spec fn scalar_branch_is_internal(
    before: epm::EpmFrame,
    reducer_after: epm::EpmFrame,
    next_id: u64,
    branch: EpmLoopBranch,
    event: epm::EpmEvent,
) -> bool {
    match (branch, event) {
        (
            EpmLoopBranch::ScalarEdgeNoChild { byte },
            epm::EpmEvent::Edge { source_offset, origin, .. },
        ) => {
            &&& epm::edge_byte(before, source_offset, origin) == byte
            &&& epm::request_for_obligation(reducer_after.path, next_id).is_none()
        },
        (EpmLoopBranch::ScalarLeave, epm::EpmEvent::Leave { .. }) => true,
        (
            EpmLoopBranch::ScalarValueNoBody {
                has_value, reducer_stable, root_stable,
            },
            epm::EpmEvent::Value {
                has_value: selected, value_body: None, ..
            },
        ) => has_value == selected,
        _ => false,
    }
}

pub open spec fn scalar_source_observation_checked(
    before: integrated::LiveRuntimeProductState,
    observed: EpmScalarLoopSourceObservation,
) -> bool {
    if integrated::product_linked_top(before.source.product).is_none() {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        &&& source_frame_matches(observed.before, prior.epm)
        &&& source_frame_matches(observed.after, observed.reducer_after)
        &&& observed.physical_source
            == scalar_event_physical_source(prior.epm, observed.event)
        &&& checked_loop_successor(
            observed.before, observed.after, observed.branch,
            observed.physical_source,
        )
        &&& observed.batch_declined
        &&& scalar_branch_is_internal(
            prior.epm, observed.reducer_after,
            prior.admission.next_obligation_id, observed.branch, observed.event,
        )
    }
}

pub struct EpmForwardLoopSourceObservation {
    pub before: EpmLoopStateSource,
    pub after: EpmLoopStateSource,
    pub commit: line_source::AcceptedLineCommitSource,
    pub ghost placement: line_source::ActLineWorkPlacement,
    pub start: usize,
    pub width: usize,
    pub source: loop_model::AcceptedForwardSource,
    pub ghost events: Seq<epm::EpmEvent>,
    pub ghost states: Seq<integrated::LiveRuntimeProductState>,
    pub ghost reducer_afters: Seq<epm::EpmFrame>,
}

/// The reducer token used by the Forward proof must be the token at the
/// production loop boundary. A fresh run uses the observed predecessor as
/// its latest restore token; a coalesced run uses the observed top run.
pub open spec fn selected_forward_run_from_loop_source(
    observed: EpmLoopStateSource,
    frame: epm::EpmFrame,
    source: loop_model::AcceptedForwardSource,
) -> bool {
    let previous_state = match observed.path.top {
        Some(top) => Some(top.state_id),
        None => None,
    };
    match source {
        loop_model::AcceptedForwardSource::Fresh {
            state_id, latest_restore_state_id, ..
        } => {
            &&& state_id == observed.path.state_id
            &&& latest_restore_state_id == previous_state
        },
        loop_model::AcceptedForwardSource::Coalesced {
            run, prefix, current_state_id, ..
        } => match observed.path.top {
            None => false,
            Some(top) => {
                &&& current_state_id == observed.path.state_id
                &&& run.state_id == top.state_id
                &&& run.first_depth == top.first_depth
                &&& run.latest_depth == top.latest_len
                &&& run.current.bytes == frame.path.path_bytes
                &&& run.current.positions == frame.path.path_positions
                &&& prefix + restore::expanded_run(run) == frame.path.restore
            },
        },
    }
}

/// Production's accepted Forward branch checks the cursor publication and
/// compressed path shape. `rust_forward_scalar_projection` below separately
/// names the unbound reducer/event projection; neither is a loop tick premise.
pub open spec fn checked_forward_commit(
    before: integrated::LiveRuntimeProductState,
    observed: EpmForwardLoopSourceObservation,
) -> bool {
    if integrated::product_linked_top(before.source.product).is_none() {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        &&& observed.width > 0
        &&& observed.start == observed.commit.start
        &&& observed.commit.end as int == observed.start as int + observed.width as int
        &&& observed.commit.direction == line_source::LineDirection::Forward
        &&& observed.commit.expanded_events as int == 2 * observed.width as int
        &&& observed.commit.last_source == observed.after.last_source
        &&& observed.before.topology == Some(observed.commit.before)
        &&& observed.after.topology == Some(observed.commit.after)
        &&& source_frame_matches(observed.before, prior.epm)
        &&& checked_loop_successor(
            observed.before, observed.after,
            EpmLoopBranch::AcceptedForward {
                start: observed.start, width: observed.width,
            }, observed.commit.last_source,
        )
        &&& line_source::accepted_indexed_line_source(
            prior.epm, observed.commit, observed.placement,
        )
        &&& selected_forward_run_from_loop_source(
            observed.before, prior.epm, observed.source,
        )
        &&& loop_model::accepted_forward_source_guard(
            prior.epm, observed.start, observed.width, observed.source,
        )
    }
}

/// Unbound Rust projection of each accepted Forward Edge/Value(None) reducer
/// call. The ACT event order is fixed by the transport template and checked
/// indexed-line work; the virtual live successors are constructed one by one.
pub open spec fn rust_forward_scalar_projection(
    before: integrated::LiveRuntimeProductState,
    observed: EpmForwardLoopSourceObservation,
) -> bool {
    &&& observed.events == transport::epm_push_chunk_events(
        transport::EpmPushTransportChunk::Forward {
            start: observed.start, width: observed.width,
        },
    )
    &&& observed.events.len() == 2 * observed.width as int
    &&& observed.states.len() == observed.events.len() + 1
    &&& observed.reducer_afters.len() == observed.events.len()
    &&& observed.states[0] == before
    &&& match integrated::product_linked_top(observed.states.last().source.product) {
        Some(last) => source_frame_matches(observed.after, last.epm),
        None => false,
    }
    &&& forall|i: int| 0 <= i < observed.events.len() ==> {
        &&& rust_scalar_reducer_projection(
            observed.states[i], observed.events[i], observed.reducer_afters[i],
        )
        &&& observed.states[i + 1] == scalar_live_after(
            observed.states[i], observed.reducer_afters[i],
        )
    }
}

pub proof fn checked_forward_source_yields_loop_tick(
    before: integrated::LiveRuntimeProductState,
    observed: EpmForwardLoopSourceObservation,
)
    requires
        checked_forward_commit(before, observed),
        rust_forward_scalar_projection(before, observed),
    ensures loop_model::loop_internal_tick(
        before, observed.states.last(),
        loop_model::EpmLoopInternalTick::AcceptedForward {
            start: observed.start, width: observed.width,
            source: observed.source,
            expanded_events: observed.events,
            expanded_states: observed.states,
        },
    ),
{
    assert forall|i: int| 0 <= i < observed.events.len() implies
        #[trigger] unwind_product::live_runtime_internal_epm_unwind_step(
            observed.states[i], observed.states[i + 1], observed.events[i],
        ) by {
        assert(rust_scalar_reducer_projection(
            observed.states[i], observed.events[i], observed.reducer_afters[i],
        ));
        scalar_reducer_projection_yields_internal_live_step(
            observed.states[i], observed.events[i], observed.reducer_afters[i],
        );
        assert(observed.states[i + 1] == scalar_live_after(
            observed.states[i], observed.reducer_afters[i],
        ));
    }
    assert(loop_model::forward_scalar_projection(
        before, observed.states.last(), observed.start, observed.width,
        observed.source, observed.events, observed.states,
    ));
}

pub struct EpmUnwindLoopSourceObservation {
    pub before: EpmLoopStateSource,
    pub after: EpmLoopStateSource,
    pub commit: line_source::AcceptedLineCommitSource,
    pub ghost placement: line_source::ActLineWorkPlacement,
    pub ghost source: epm::LinkedEpmState,
    pub ghost run: crate::rpb3_epm_batch_cursor_composition::HistoricalRestoreRunImage,
    pub ghost prefix: Seq<epm::PathCheckpoint>,
    pub start: usize,
    pub width: usize,
    pub ghost facts: restore::UnwindCapacityFacts,
}

/// The selected run is the top compressed restore run reported by the Rust
/// source shape, expanded against the *current* modeled reducer history. In
/// particular, an unrelated valid historical run is not a witness for this
/// loop boundary. The remaining Rust-to-model obligation is that the reducer
/// history projects the production reducer's retained restore history.
pub open spec fn selected_unwind_run_from_loop_source(
    observed: EpmLoopStateSource,
    frame: epm::EpmFrame,
    run: crate::rpb3_epm_batch_cursor_composition::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
) -> bool {
    match observed.path.top {
        None => false,
        Some(top) => {
            let run_width = top.latest_len as int - top.first_depth as int + 1;
            let restore_len = frame.path.restore.len();
            &&& top.first_depth <= top.latest_len
            &&& top.latest_len as int + 1 == frame.path.path_bytes.len()
            &&& frame.path.path_positions.len() == frame.path.path_bytes.len()
            &&& run_width <= restore_len
            &&& observed.path.depth as int == frame.path.path_bytes.len()
            &&& run.first_depth == top.first_depth
            &&& run.latest_depth == top.latest_len
            &&& run.saved_state_id == top.state_id
            &&& run.current == epm::SourceTreeImage {
                bytes: frame.path.path_bytes,
                positions: frame.path.path_positions,
            }
            &&& run.latest == epm::SourceTreeImage {
                bytes: frame.path.path_bytes.subrange(0, top.latest_len as int),
                positions: frame.path.path_positions.subrange(0, top.latest_len as int),
            }
            &&& run.checkpoints == frame.path.restore.subrange(
                restore_len as int - run_width, restore_len as int,
            )
            &&& prefix == frame.path.restore.subrange(0, restore_len as int - run_width)
        },
    }
}

pub proof fn selected_unwind_run_excludes_another_restoration(
    observed: EpmLoopStateSource,
    frame: epm::EpmFrame,
    run: crate::rpb3_epm_batch_cursor_composition::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    other: crate::rpb3_epm_batch_cursor_composition::HistoricalRestoreRunImage,
    other_prefix: Seq<epm::PathCheckpoint>,
)
    requires
        selected_unwind_run_from_loop_source(observed, frame, run, prefix),
        selected_unwind_run_from_loop_source(observed, frame, other, other_prefix),
    ensures run == other, prefix == other_prefix,
{}

pub open spec fn checked_unwind_commit(
    before: integrated::LiveRuntimeProductState,
    observed: EpmUnwindLoopSourceObservation,
) -> bool {
    if integrated::product_linked_top(before.source.product).is_none() {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let after = unwind_product::live_runtime_unwind_virtual_state(
            before, observed.source, observed.run, observed.prefix, observed.width,
        );
        &&& observed.width > 0
        &&& observed.start == observed.commit.start
        &&& observed.commit.end as int == observed.start as int + observed.width as int
        &&& observed.commit.direction == line_source::LineDirection::Unwind
        &&& observed.commit.expanded_events == observed.width
        &&& observed.commit.last_source == observed.after.last_source
        &&& observed.before.topology == Some(observed.commit.before)
        &&& observed.after.topology == Some(observed.commit.after)
        &&& source_frame_matches(observed.before, prior.epm)
        &&& selected_unwind_run_from_loop_source(
            observed.before, prior.epm, observed.run, observed.prefix,
        )
        &&& checked_loop_successor(
            observed.before, observed.after,
            EpmLoopBranch::AcceptedUnwind {
                start: observed.start, width: observed.width,
            }, observed.commit.last_source,
        )
        &&& line_source::accepted_indexed_line_source(
            prior.epm, observed.commit, observed.placement,
        )
        &&& unwind_product::live_runtime_unwind_source_guard(
            before, observed.source, observed.run, observed.prefix,
            observed.start, observed.width, observed.facts,
        )
        &&& match integrated::product_linked_top(after.source.product) {
            Some(last) => source_frame_matches(observed.after, last.epm),
            None => false,
        }
    }
}

pub proof fn checked_unwind_source_yields_loop_tick(
    before: integrated::LiveRuntimeProductState,
    observed: EpmUnwindLoopSourceObservation,
)
    requires checked_unwind_commit(before, observed),
    ensures {
        let after = unwind_product::live_runtime_unwind_virtual_state(
            before, observed.source, observed.run, observed.prefix, observed.width,
        );
        &&& loop_model::loop_internal_tick(
            before, after,
            loop_model::EpmLoopInternalTick::AcceptedUnwind {
                source: observed.source, run: observed.run,
                prefix: observed.prefix, start: observed.start,
                width: observed.width, facts: observed.facts,
            },
        )
        &&& integrated::live_runtime_product_wf(after)
    },
{
    unwind_product::live_runtime_unwind_virtual_state_wf(
        before, observed.source, observed.run, observed.prefix,
        observed.start, observed.width, observed.facts, observed.width,
    );
}

pub proof fn checked_scalar_source_yields_loop_tick(
    before: integrated::LiveRuntimeProductState,
    observed: EpmScalarLoopSourceObservation,
)
    requires
        scalar_source_observation_checked(before, observed),
        rust_scalar_reducer_projection(
            before, observed.event, observed.reducer_after,
        ),
    ensures {
        let after = scalar_live_after(before, observed.reducer_after);
        &&& loop_model::loop_internal_tick(
            before, after,
            loop_model::EpmLoopInternalTick::ScalarAfterDeclinedBatch {
                event: observed.event,
            },
        )
        &&& source_frame_matches(
            observed.after,
            integrated::product_linked_top(after.source.product).unwrap().epm,
        )
    },
{
    scalar_reducer_projection_yields_internal_live_step(
        before, observed.event, observed.reducer_after,
    );
    let after = scalar_live_after(before, observed.reducer_after);
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    let linked = integrated::product_linked_top(after.source.product).unwrap();
    assert(linked.epm == observed.reducer_after);
    assert(match observed.event {
        epm::EpmEvent::Leave { .. } => true,
        epm::EpmEvent::Edge { .. } => epm::request_for_obligation(
            linked.epm.path, prior.admission.next_obligation_id,
        ).is_none(),
        epm::EpmEvent::Value { value_body: None, .. } => true,
        _ => false,
    });
    assert(loop_model::scalar_loop_continues(before, after, observed.event));
}

pub struct EpmPushLoopSourceObservation {
    pub before: EpmLoopStateSource,
    pub after: EpmLoopStateSource,
    pub branch: EpmLoopBranch,
    pub physical_source: epm::SourceAnchor,
    pub batch_declined: bool,
    pub ghost live: push_source::EpmPushLiveObservation,
    pub ghost map_value_source: Option<line_source::MapValueNextEventSource>,
}

pub open spec fn checked_push_stop_source(
    before: integrated::LiveRuntimeProductState,
    observed: EpmPushLoopSourceObservation,
) -> bool {
    if integrated::product_linked_top(before.source.product).is_none() {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let event = observed.live.push.terminal_event;
        &&& observed.batch_declined
        &&& source_frame_matches(observed.before, prior.epm)
        &&& source_frame_matches(observed.after, observed.live.push.reducer_after)
        &&& observed.physical_source
            == scalar_event_physical_source(prior.epm, event)
        &&& checked_loop_successor(
            observed.before, observed.after, observed.branch,
            observed.physical_source,
        )
        &&& match (observed.branch, event) {
            (
                EpmLoopBranch::PushEdge { byte },
                epm::EpmEvent::Edge { source_offset, origin, .. },
            ) => {
                &&& epm::edge_byte(prior.epm, source_offset, origin) == byte
                &&& epm::request_for_obligation(
                    observed.live.push.reducer_after.path,
                    prior.admission.next_obligation_id,
                ).is_some()
            },
            (
                EpmLoopBranch::PushValue {
                    has_value, reducer_stable, root_stable, body_range,
                },
                epm::EpmEvent::Value { value_body: Some(_), .. },
            ) => has_value
                && match observed.map_value_source {
                    Some(source) => {
                        &&& line_source::selected_map_value_attachment_source(
                            prior.epm, source,
                        )
                        &&& source.body == body_range
                        &&& source.act_source == observed.physical_source
                        &&& event == line_source::map_value_event_from_source(source)
                    },
                    None => false,
                },
            _ => false,
        }
    }
}

/// Unbound Rust projection of the selected reducer/child and checked driver
/// storage. These are exactly the components of the existing live Push source
/// selector, stated separately from the EPathMap loop's branch/source check.
pub open spec fn rust_push_selector_projection(
    before: integrated::LiveRuntimeProductState,
    observed: push_source::EpmPushLiveObservation,
) -> bool {
    if !push_source::epm_push_source_ready(before, observed.push) {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let request = push_source::epm_push_source_request(
            prior, observed.push,
        ).unwrap();
        &&& before.source.wire.last().is_none()
        &&& before.cursors.last().is_none()
        &&& push_live::epm_push_live_child_ready(before, observed)
        &&& match runtime::source_frame_push_observe_after(
            push_source::epm_push_source_request_kind(request),
            observed.push.actual_top_kind,
            prior.admission.frames.len() as usize,
            before.source.runtime.capacity,
            observed.push.after_len,
            observed.push.after_capacity,
            before.source.runtime.frame_bytes,
            observed.push.stats_frame_variant_bytes,
        ) {
            runtime::SourceFramePushObservationResult::Success { storage } =>
                runtime::source_successful_push_storage(
                    before.source.runtime, storage,
                ),
            _ => false,
        }
    }
}

pub proof fn checked_push_source_yields_stop_readiness(
    before: integrated::LiveRuntimeProductState,
    observed: EpmPushLoopSourceObservation,
)
    requires
        checked_push_stop_source(before, observed),
        rust_push_selector_projection(before, observed.live),
    ensures
        loop_model::loop_stop_ready(
            before, loop_model::EpmLoopStop::Push { observed: observed.live },
        ),
        match (observed.branch, observed.live.push.terminal_event) {
            (
                EpmLoopBranch::PushValue { body_range, .. },
                epm::EpmEvent::Value { value_body: Some(selected_span), .. },
            ) => body_range == selected_span,
            _ => true,
        },
        match loop_model::loop_selected_action(
            before, loop_model::EpmLoopStop::Push { observed: observed.live },
        ) {
            Some(epm::EpmAction::Push(_)) => true,
            _ => false,
        },
{
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    match (observed.branch, observed.live.push.terminal_event) {
        (
            EpmLoopBranch::PushValue {
                has_value, reducer_stable, root_stable, body_range,
            },
            epm::EpmEvent::Value {
                has_value: selected, value_body: Some(selected_span), source_offset,
            },
        ) => {
            let source = observed.map_value_source.unwrap();
            assert(source.body == body_range);
            assert(observed.live.push.terminal_event
                == line_source::map_value_event_from_source(source));
            checked_push_value_source_binds_selected_body(
                observed.before, observed.after, prior.epm,
                observed.live.push.reducer_after, observed.physical_source,
                reducer_stable, root_stable, source,
            );
        },
        _ => {},
    }
    assert(push_live::epm_push_live_ready(before, observed.live));
}

pub struct EpmLoopTerminalCursorSource {
    pub mode: epm::EpmMode,
    pub declared_value_count: usize,
    pub map_values_seen: usize,
    pub event_index: usize,
    pub value_table_offset: usize,
    pub snapshot_len: usize,
    pub terminal_checks: usize,
}

pub struct EpmCompleteLoopSourceObservation {
    pub before: EpmLoopStateSource,
    pub terminal_cursor: EpmLoopTerminalCursorSource,
    pub owner_id_after: usize,
    pub field_after: u32,
    pub region_start_after: epm::SourceAnchor,
    pub snapshot_len_after: usize,
    pub last_source_after: epm::SourceAnchor,
    pub entries_stable_after: bool,
    pub batch_declined: bool,
    pub ghost terminal: terminal::EpmTerminalSourceObservation,
}

/// Mirrors `matches_complete_stop`: EOF does not add a scalar topology event;
/// it changes the cursor to its terminal image after `path.finish` succeeds.
pub open spec fn checked_complete_stop_source(
    before: integrated::LiveRuntimeProductState,
    observed: EpmCompleteLoopSourceObservation,
) -> bool {
    if integrated::product_linked_top(before.source.product).is_none() {
        false
    } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let active = observed.before.cursor;
        let done = observed.terminal_cursor;
        &&& observed.batch_declined
        &&& source_frame_matches(observed.before, prior.epm)
        &&& observed.before.owner_id == observed.owner_id_after
        &&& observed.before.field == observed.field_after
        &&& observed.before.region_start == observed.region_start_after
        &&& observed.before.snapshot_len == observed.snapshot_len_after
        &&& active.mode == done.mode
        &&& active.declared_value_count == done.declared_value_count
        &&& active.map_values_seen == done.map_values_seen
        &&& active.event_index == done.event_index
        &&& active.value_table_offset == done.value_table_offset
        &&& active.snapshot_len == done.snapshot_len
        &&& observed.before.waiting is None
        &&& observed.before.waiting_child_len.is_none()
        &&& observed.before.obligation.is_none()
        &&& observed.before.last_source == observed.last_source_after
        &&& observed.before.entries_stable == observed.entries_stable_after
        &&& done.terminal_checks == 1
        &&& observed.terminal.snapshot == prior.epm.snapshot
        &&& observed.terminal.cursor_at == active.event_index
        &&& observed.terminal.value_table_offset == active.value_table_offset
        &&& observed.terminal.mode == active.mode
        &&& observed.terminal.entries_stable == observed.before.entries_stable
        &&& observed.terminal.last_source == observed.before.last_source
    }
}

/// The remaining EOF projection is `path.finish`, checked stats, and the
/// admission top's finish eligibility; no driver pop or parent accept is
/// claimed by this EPathMap stop theorem.
pub open spec fn rust_complete_selector_projection(
    before: integrated::LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || integrated::product_linked_top(before.source.product).is_none()
    { false } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        if prior.admission.frames.len() == 0 { false } else {
            let top = prior.admission.frames.last();
            &&& epm::linked_indexed_sources(prior)
            &&& admission::admission_state_wf(prior.admission)
            &&& top.frame_kind == 3
            &&& top.waiting_id.is_none()
            &&& top.nested_started == top.nested_resolved
            &&& top.cursor == top.region_end
            &&& admission::admission_frame_wf(top)
            &&& admission::admission_family_admissible(top)
            &&& prior.epm.snapshot == prior.indexed_regions.last()
            &&& terminal::epm_terminal_source_ready(prior.epm, observed)
            &&& observed.stats_outcome is Success
        }
    }
}

pub proof fn checked_complete_source_yields_stop_readiness(
    before: integrated::LiveRuntimeProductState,
    observed: EpmCompleteLoopSourceObservation,
)
    requires
        checked_complete_stop_source(before, observed),
        rust_complete_selector_projection(before, observed.terminal),
    ensures
        loop_model::loop_stop_ready(
            before, loop_model::EpmLoopStop::Complete {
                observed: observed.terminal,
            },
        ),
        match loop_model::loop_selected_action(
            before, loop_model::EpmLoopStop::Complete {
                observed: observed.terminal,
            },
        ) {
            Some(epm::EpmAction::Complete { .. }) => true,
            _ => false,
        },
{
    let prior = integrated::product_linked_top(before.source.product).unwrap();
    terminal::epm_terminal_source_success_enabled(prior.epm, observed.terminal);
    assert(end_source::epm_end_source_ready(before, observed.terminal));
}

} // verus!
