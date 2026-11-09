//! Source-shaped model of `PersistentCheckpoints::get(id, stamp)` in
//! `canonical_path_admission.rs`. A saved restore-run token is looked up at
//! each prefix depth; a changed current token does not change that lookup.
//!
//! These proofs start from projected checkpoint records. Their source
//! premises are: the tagged token and checkpoint sequences denote the real
//! `PathStateId` and `PersistentCheckpoints` entries; `selected_obligation`,
//! `frame_history_id`, and gate fields denote the real saved frame-history
//! selection; `owner_root_dead` denotes the owner-table observation at this
//! lookup; and each requested depth denotes the real restored byte prefix.
//! This module does not establish any of those Rust-to-model projections.

use vstd::prelude::*;

verus! {

pub enum SourcePathStatus {
    Open,
    SplitComplete,
    Dead,
}

pub struct SourceAnchor {
    pub source: u32,
    pub offset: usize,
}

/// Projected gate facts about the saved path and its frame/owner context.
/// Their correspondence to the real frame history is a source premise.
pub struct SourceGate {
    pub dead: bool,
    pub stack_empty: bool,
    pub split_complete: bool,
    pub completed_segments: u64,
    pub first_is_list: bool,
    pub stable: bool,
    pub escape_allowed: bool,
}

pub enum SourceObligationStatus {
    Payload,
    AwaitingResolution { payload_end: SourceAnchor },
}

pub struct SourceObligation {
    pub id: u64,
    pub kind: u8,
    pub payload_len: u64,
    pub tag_source: SourceAnchor,
    pub payload_start: usize,
    pub status: SourceObligationStatus,
}

pub struct SourceInnerPath {
    pub status: SourcePathStatus,
    pub gate: SourceGate,
    pub selected_obligation: Option<SourceObligation>,
    /// Identifies the frame-history image used to select the obligation.
    pub frame_history_id: usize,
}

pub struct SourceFocusedPath {
    pub current: SourceInnerPath,
    pub owner_id: Option<usize>,
    pub fault_id: Option<usize>,
    /// Observation of `root_path(state).status` at this source boundary.
    pub owner_root_dead: bool,
    pub stamp: usize,
}

pub struct SourceCheckpoints {
    pub ghost direct: Seq<SourceInnerPath>,
    pub ghost linked: Seq<SourceFocusedPath>,
}

pub struct SourceSavedRun {
    /// The tagged `run.latest.state` token, not the current handle's token.
    pub state_token: usize,
    pub first_depth: usize,
    pub latest_depth: usize,
}

pub struct SourceObligationProgress {
    pub id: u64,
    pub kind: u8,
    pub remaining: u64,
    pub awaiting_resolution: bool,
}

/// The modeled fields of `CanonicalPathBatchHandleSource`. Its source anchor,
/// owner boundary, and byte-handle identity still need arena/owner projection.
pub struct SourcePredecessorCore {
    pub state_token: usize,
    pub depth: usize,
    pub stamp: usize,
    pub open: bool,
    pub fault_free: bool,
    pub gate: SourceGate,
    pub pending: Option<SourceObligationProgress>,
}

pub open spec fn token_is_linked(token: usize) -> bool {
    token % 2 == 1
}

pub open spec fn token_slot(token: usize) -> usize {
    token / 2
}

/// Mirrors `PersistentCheckpoints::get`: direct entries use the requested
/// stamp and have no owner or fault; linked entries retain the saved stamp
/// when faulted or when their current path is dead. SplitComplete is not
/// Dead, so a fault-free linked SplitComplete entry receives the new stamp.
pub open spec fn checkpoint_get(
    checkpoints: SourceCheckpoints,
    token: usize,
    requested_stamp: usize,
) -> Option<SourceFocusedPath> {
    let slot = token_slot(token);
    if token_is_linked(token) {
        if slot >= checkpoints.linked.len() {
            None
        } else {
            let saved = checkpoints.linked[slot as int];
            Some(SourceFocusedPath {
                current: saved.current,
                owner_id: saved.owner_id,
                fault_id: saved.fault_id,
                owner_root_dead: saved.owner_root_dead,
                stamp: if saved.fault_id.is_none()
                    && saved.current.status != SourcePathStatus::Dead {
                    requested_stamp
                } else {
                    saved.stamp
                },
            })
        }
    } else if slot >= checkpoints.direct.len() {
        None
    } else {
        let current = checkpoints.direct[slot as int];
        Some(SourceFocusedPath {
            current,
            owner_id: None,
            fault_id: None,
            owner_root_dead: current.status == SourcePathStatus::Dead,
            stamp: requested_stamp,
        })
    }
}

pub open spec fn saved_run_wf(run: SourceSavedRun) -> bool {
    run.first_depth <= run.latest_depth
}

/// A saved run's prefix at `depth` reuses its saved state ID with a new byte
/// handle of that depth. This is the state lookup made by both scalar Leave
/// and `try_leave_run`; the byte-handle prefix itself is not proved here.
pub open spec fn saved_run_state_at_depth(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    depth: usize,
) -> Option<SourceFocusedPath> {
    if !saved_run_wf(run) || depth < run.first_depth || depth > run.latest_depth {
        None
    } else {
        checkpoint_get(checkpoints, run.state_token, depth)
    }
}

/// `ObligationStatus::Payload.remaining` is `declared - (stamp - start)`.
/// Invalid source projections yield None instead of relying on Rust's
/// `expect` in `active_obligation_in`.
pub open spec fn obligation_progress(
    state: SourceFocusedPath,
) -> Option<SourceObligationProgress> {
    if state.fault_id.is_some()
        || state.current.status == SourcePathStatus::Dead
        || state.owner_root_dead
    {
        None
    } else {
        match state.current.selected_obligation {
            None => None,
            Some(obligation) => match obligation.status {
                SourceObligationStatus::AwaitingResolution { .. } =>
                    Some(SourceObligationProgress {
                        id: obligation.id,
                        kind: obligation.kind,
                        remaining: 0,
                        awaiting_resolution: true,
                    }),
                SourceObligationStatus::Payload => {
                    let consumed = state.stamp as int - obligation.payload_start as int;
                    if consumed < 0 || consumed > obligation.payload_len as int {
                        None
                    } else {
                        Some(SourceObligationProgress {
                            id: obligation.id,
                            kind: obligation.kind,
                            remaining: (obligation.payload_len as int - consumed) as u64,
                            awaiting_resolution: false,
                        })
                    }
                },
            },
        }
    }
}

pub open spec fn source_fault_free(state: SourceFocusedPath) -> bool {
    state.fault_id.is_none()
        && state.current.status != SourcePathStatus::Dead
        && !state.owner_root_dead
}

/// `batch_handle_source` computes `gate.dead` from `dead_error`; all other
/// fields are read from the selected current path.
pub open spec fn observed_gate(state: SourceFocusedPath) -> SourceGate {
    SourceGate {
        dead: !source_fault_free(state),
        stack_empty: state.current.gate.stack_empty,
        split_complete: state.current.gate.split_complete,
        completed_segments: state.current.gate.completed_segments,
        first_is_list: state.current.gate.first_is_list,
        stable: state.current.gate.stable,
        escape_allowed: state.current.gate.escape_allowed,
    }
}

/// The core of `batch_restore_predecessor_source(run_index, depth)`: the
/// production method slices `run.latest.bytes` to `depth`, keeps its saved
/// state token, and calls `batch_handle_source` on that handle.
pub open spec fn batch_restore_predecessor_core(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    depth: usize,
) -> Option<SourcePredecessorCore> {
    match saved_run_state_at_depth(checkpoints, run, depth) {
        None => None,
        Some(state) => Some(SourcePredecessorCore {
            state_token: run.state_token,
            depth,
            stamp: state.stamp,
            open: state.current.status == SourcePathStatus::Open,
            fault_free: source_fault_free(state),
            gate: observed_gate(state),
            pending: obligation_progress(state),
        }),
    }
}

/// Repeated scalar `leave` while the top restore run remains active. Each
/// call selects the old `run.latest` and shortens that run by one depth;
/// after its first-depth entry is selected, the run is popped.
pub open spec fn scalar_leave_selected_after(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    leaves: nat,
) -> Option<SourceFocusedPath>
    decreases leaves
{
    if !saved_run_wf(run) || leaves == 0 {
        None
    } else if leaves == 1 {
        saved_run_state_at_depth(checkpoints, run, run.latest_depth)
    } else if run.latest_depth == run.first_depth {
        None
    } else {
        scalar_leave_selected_after(
            checkpoints,
            SourceSavedRun {
                state_token: run.state_token,
                first_depth: run.first_depth,
                latest_depth: (run.latest_depth - 1) as usize,
            },
            (leaves - 1) as nat,
        )
    }
}

/// The `leaves`-th scalar Leave inside this saved run selects exactly the
/// same checkpoint and requested stamp as the on-demand predecessor source
/// at `depth`. This does not assume the current handle has the saved token.
pub proof fn scalar_leave_selected_is_saved_predecessor(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    depth: usize,
    leaves: nat,
)
    requires
        saved_run_wf(run),
        run.first_depth <= depth <= run.latest_depth,
        leaves == run.latest_depth as int - depth as int + 1,
        saved_run_state_at_depth(checkpoints, run, depth).is_some(),
    ensures
        scalar_leave_selected_after(checkpoints, run, leaves)
            == saved_run_state_at_depth(checkpoints, run, depth),
    decreases leaves,
{
    assert(leaves >= 1);
    if leaves > 1 {
        assert(run.latest_depth > run.first_depth);
        let shorter = SourceSavedRun {
            state_token: run.state_token,
            first_depth: run.first_depth,
            latest_depth: (run.latest_depth - 1) as usize,
        };
        assert(saved_run_wf(shorter));
        assert(run.first_depth <= depth <= shorter.latest_depth);
        assert(leaves - 1 == shorter.latest_depth as int - depth as int + 1);
        assert(saved_run_state_at_depth(checkpoints, shorter, depth)
            == saved_run_state_at_depth(checkpoints, run, depth));
        scalar_leave_selected_is_saved_predecessor(checkpoints, shorter, depth, (leaves - 1) as nat);
    }
}

/// Consequently the scalar Leave predecessor and exact source observer agree
/// on state ID, depth, stamp, gate, and obligation progress. The observer's
/// owner boundary and last source anchor require separate projections.
pub proof fn scalar_leave_predecessor_matches_batch_source_core(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    depth: usize,
    leaves: nat,
)
    requires
        saved_run_wf(run),
        run.first_depth <= depth <= run.latest_depth,
        leaves == run.latest_depth as int - depth as int + 1,
        saved_run_state_at_depth(checkpoints, run, depth).is_some(),
    ensures {
        let selected = scalar_leave_selected_after(checkpoints, run, leaves).unwrap();
        let source = batch_restore_predecessor_core(checkpoints, run, depth).unwrap();
        &&& scalar_leave_selected_after(checkpoints, run, leaves).is_some()
        &&& batch_restore_predecessor_core(checkpoints, run, depth).is_some()
        &&& source.state_token == run.state_token
        &&& source.depth == depth
        &&& source.stamp == selected.stamp
        &&& source.open == (selected.current.status == SourcePathStatus::Open)
        &&& source.fault_free == source_fault_free(selected)
        &&& source.gate == observed_gate(selected)
        &&& source.pending == obligation_progress(selected)
    },
{
    scalar_leave_selected_is_saved_predecessor(checkpoints, run, depth, leaves);
    assert(scalar_leave_selected_after(checkpoints, run, leaves).is_some());
    assert(batch_restore_predecessor_core(checkpoints, run, depth).is_some());
    assert(scalar_leave_selected_after(checkpoints, run, leaves).unwrap()
        == saved_run_state_at_depth(checkpoints, run, depth).unwrap());
}

pub proof fn checkpoint_get_preserves_saved_state(
    checkpoints: SourceCheckpoints,
    token: usize,
    stamp: usize,
)
    requires checkpoint_get(checkpoints, token, stamp).is_some(),
    ensures {
        let selected = checkpoint_get(checkpoints, token, stamp).unwrap();
        let slot = token_slot(token);
        if token_is_linked(token) {
            let saved = checkpoints.linked[slot as int];
            &&& selected.current == saved.current
            &&& selected.owner_id == saved.owner_id
            &&& selected.fault_id == saved.fault_id
            &&& selected.owner_root_dead == saved.owner_root_dead
            &&& selected.stamp == if saved.fault_id.is_none()
                && saved.current.status != SourcePathStatus::Dead {
                stamp
            } else {
                saved.stamp
            }
        } else {
            &&& selected.current == checkpoints.direct[slot as int]
            &&& selected.owner_id.is_none()
            &&& selected.fault_id.is_none()
            &&& selected.stamp == stamp
        }
    },
{
    if token_is_linked(token) {
        assert(token_slot(token) < checkpoints.linked.len());
    } else {
        assert(token_slot(token) < checkpoints.direct.len());
    }
}

pub proof fn saved_run_prefix_states_keep_identity(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    earlier: usize,
    later: usize,
)
    requires
        saved_run_wf(run),
        run.first_depth <= earlier <= later <= run.latest_depth,
        saved_run_state_at_depth(checkpoints, run, earlier).is_some(),
    ensures {
        let first = saved_run_state_at_depth(checkpoints, run, earlier).unwrap();
        let second = saved_run_state_at_depth(checkpoints, run, later).unwrap();
        &&& saved_run_state_at_depth(checkpoints, run, later).is_some()
        &&& first.current == second.current
        &&& first.current.gate == second.current.gate
        &&& first.current.selected_obligation == second.current.selected_obligation
        &&& first.current.frame_history_id == second.current.frame_history_id
        &&& first.owner_id == second.owner_id
        &&& first.fault_id == second.fault_id
        &&& first.owner_root_dead == second.owner_root_dead
        &&& if token_is_linked(run.state_token) {
            let saved = checkpoints.linked[token_slot(run.state_token) as int];
            if saved.fault_id.is_none() && saved.current.status != SourcePathStatus::Dead {
                first.stamp == earlier && second.stamp == later
            } else {
                first.stamp == saved.stamp && second.stamp == saved.stamp
            }
        } else {
            first.stamp == earlier && second.stamp == later
        }
    },
{
    assert(checkpoint_get(checkpoints, run.state_token, later).is_some());
    checkpoint_get_preserves_saved_state(checkpoints, run.state_token, earlier);
    checkpoint_get_preserves_saved_state(checkpoints, run.state_token, later);
}

pub proof fn saved_payload_progress_changes_with_depth(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    earlier: usize,
    later: usize,
    obligation: SourceObligation,
)
    requires
        saved_run_wf(run),
        run.first_depth <= earlier <= later <= run.latest_depth,
        saved_run_state_at_depth(checkpoints, run, earlier).is_some(),
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .current.selected_obligation == Some(obligation),
        obligation.status == SourceObligationStatus::Payload,
        obligation.payload_start <= earlier,
        later as int - obligation.payload_start as int <= obligation.payload_len as int,
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .fault_id.is_none(),
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .current.status != SourcePathStatus::Dead,
        !saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .owner_root_dead,
    ensures {
        let first = saved_run_state_at_depth(checkpoints, run, earlier).unwrap();
        let second = saved_run_state_at_depth(checkpoints, run, later).unwrap();
        &&& obligation_progress(first).is_some()
        &&& obligation_progress(second).is_some()
        &&& !obligation_progress(first).unwrap().awaiting_resolution
        &&& !obligation_progress(second).unwrap().awaiting_resolution
        &&& obligation_progress(first).unwrap().id == obligation.id
        &&& obligation_progress(second).unwrap().id == obligation.id
        &&& obligation_progress(second).unwrap().remaining as int
            + later as int - earlier as int
            == obligation_progress(first).unwrap().remaining as int
    },
{
    saved_run_prefix_states_keep_identity(checkpoints, run, earlier, later);
}

pub proof fn saved_non_obligation_stays_absent(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    earlier: usize,
    later: usize,
)
    requires
        saved_run_wf(run),
        run.first_depth <= earlier <= later <= run.latest_depth,
        saved_run_state_at_depth(checkpoints, run, earlier).is_some(),
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .current.selected_obligation.is_none(),
    ensures
        saved_run_state_at_depth(checkpoints, run, later).is_some(),
        obligation_progress(
            saved_run_state_at_depth(checkpoints, run, earlier).unwrap(),
        ).is_none(),
        obligation_progress(
            saved_run_state_at_depth(checkpoints, run, later).unwrap(),
        ).is_none(),
{
    saved_run_prefix_states_keep_identity(checkpoints, run, earlier, later);
}

/// An unresolved obligation remains at zero remaining bytes across restored
/// prefixes; its identity and resolution anchor are preserved by the lookup.
pub proof fn saved_awaiting_resolution_stays_zero(
    checkpoints: SourceCheckpoints,
    run: SourceSavedRun,
    earlier: usize,
    later: usize,
    obligation: SourceObligation,
)
    requires
        saved_run_wf(run),
        run.first_depth <= earlier <= later <= run.latest_depth,
        saved_run_state_at_depth(checkpoints, run, earlier).is_some(),
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .current.selected_obligation == Some(obligation),
        match obligation.status {
            SourceObligationStatus::AwaitingResolution { .. } => true,
            _ => false,
        },
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .fault_id.is_none(),
        saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .current.status != SourcePathStatus::Dead,
        !saved_run_state_at_depth(checkpoints, run, earlier).unwrap()
            .owner_root_dead,
    ensures {
        let first = saved_run_state_at_depth(checkpoints, run, earlier).unwrap();
        let second = saved_run_state_at_depth(checkpoints, run, later).unwrap();
        &&& saved_run_state_at_depth(checkpoints, run, later).is_some()
        &&& second.current.selected_obligation == Some(obligation)
        &&& obligation_progress(first).is_some()
        &&& obligation_progress(second).is_some()
        &&& obligation_progress(first).unwrap().remaining == 0
        &&& obligation_progress(second).unwrap().remaining == 0
        &&& obligation_progress(first).unwrap().awaiting_resolution
        &&& obligation_progress(second).unwrap().awaiting_resolution
    },
{
    saved_run_prefix_states_keep_identity(checkpoints, run, earlier, later);
}

pub proof fn linked_fault_or_dead_retains_saved_stamp(
    checkpoints: SourceCheckpoints,
    token: usize,
    requested: usize,
)
    requires
        token_is_linked(token),
        token_slot(token) < checkpoints.linked.len(),
        checkpoints.linked[token_slot(token) as int].fault_id.is_some()
            || checkpoints.linked[token_slot(token) as int].current.status
                == SourcePathStatus::Dead,
    ensures {
        let saved = checkpoints.linked[token_slot(token) as int];
        let selected = checkpoint_get(checkpoints, token, requested).unwrap();
        &&& selected.stamp == saved.stamp
        &&& selected.current == saved.current
        &&& selected.owner_id == saved.owner_id
        &&& selected.fault_id == saved.fault_id
    },
{
    checkpoint_get_preserves_saved_state(checkpoints, token, requested);
}

pub open spec fn example_gate() -> SourceGate {
    SourceGate {
        dead: false,
        stack_empty: false,
        split_complete: false,
        completed_segments: 0,
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    }
}

/// Concrete linked payload witness: the saved state and owner stay fixed,
/// while a three-byte restored prefix consumes three declared payload bytes.
pub proof fn linked_payload_prefix_witness() {
    let obligation = SourceObligation {
        id: 7,
        kind: 1,
        payload_len: 10,
        tag_source: SourceAnchor { source: 2, offset: 40 },
        payload_start: 0,
        status: SourceObligationStatus::Payload,
    };
    let inner = SourceInnerPath {
        status: SourcePathStatus::Open,
        gate: example_gate(),
        selected_obligation: Some(obligation),
        frame_history_id: 5,
    };
    let stored = SourceFocusedPath {
        current: inner,
        owner_id: Some(3),
        fault_id: None,
        owner_root_dead: false,
        stamp: 0,
    };
    let checkpoints = SourceCheckpoints {
        direct: Seq::empty(),
        linked: seq![stored],
    };
    let run = SourceSavedRun { state_token: 1, first_depth: 1, latest_depth: 3 };
    assert(saved_run_state_at_depth(checkpoints, run, 1).is_some());
    scalar_leave_predecessor_matches_batch_source_core(checkpoints, run, 1, 3);
    saved_payload_progress_changes_with_depth(checkpoints, run, 1, 3, obligation);
    assert(obligation_progress(saved_run_state_at_depth(checkpoints, run, 1).unwrap())
        .unwrap().remaining == 9);
    assert(obligation_progress(saved_run_state_at_depth(checkpoints, run, 3).unwrap())
        .unwrap().remaining == 7);
}

/// A direct, non-obligation run restores its saved token even when the
/// currently focused path has a different state token and status.
pub proof fn changed_current_state_non_obligation_witness() {
    let saved = SourceInnerPath {
        status: SourcePathStatus::Open,
        gate: example_gate(),
        selected_obligation: None,
        frame_history_id: 1,
    };
    let current = SourceInnerPath {
        status: SourcePathStatus::SplitComplete,
        gate: example_gate(),
        selected_obligation: None,
        frame_history_id: 2,
    };
    let checkpoints = SourceCheckpoints {
        direct: seq![saved, current],
        linked: Seq::empty(),
    };
    let run = SourceSavedRun { state_token: 0, first_depth: 0, latest_depth: 1 };
    let current_token = 2usize;
    assert(current_token != run.state_token);
    assert(checkpoint_get(checkpoints, current_token, 2).unwrap().current == current);
    assert(saved_run_state_at_depth(checkpoints, run, 1).unwrap().current == saved);
    scalar_leave_predecessor_matches_batch_source_core(checkpoints, run, 0, 2);
    saved_non_obligation_stays_absent(checkpoints, run, 0, 1);
}

}

fn main() {}
