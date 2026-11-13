use vstd::prelude::*;

verus! {

/** Executable projection of one public-zipper teardown transition.  The
    caller has already positioned the zipper at the reverse terminal path. */
#[derive(Copy, Clone)]
pub struct ReversePruneState {
    pub remaining_edges: u64,
    pub remaining_payloads: u64,
    pub detached_payloads: u64,
    pub pruned_edges: u64,
    pub written_values: u64,
}

pub struct StepView {
    pub parent_is_root: bool,
    pub parent_has_payload: bool,
    pub left_sibling_edges: u64,
}

pub enum PhaseBStepKind {
    BranchCut,
    OneBytePrune,
}

pub open spec fn parent_stops_the_run(view: StepView) -> bool {
    view.parent_is_root || view.parent_has_payload || view.left_sibling_edges != 0
}

pub open spec fn barrier_free_step_kind(view: StepView) -> PhaseBStepKind {
    if parent_stops_the_run(view) {
        PhaseBStepKind::OneBytePrune
    } else {
        PhaseBStepKind::BranchCut
    }
}

pub open spec fn barrier_free_step_detached_edges(view: StepView) -> int {
    match barrier_free_step_kind(view) {
        PhaseBStepKind::BranchCut => view.left_sibling_edges as int + 1,
        PhaseBStepKind::OneBytePrune => 1int,
    }
}

pub fn executable_one_logical_edge_step(
    state: ReversePruneState,
    view: StepView,
    terminal_has_payload: bool,
) -> (result: (PhaseBStepKind, ReversePruneState))
    requires
        view.left_sibling_edges < state.remaining_edges,
        state.pruned_edges + state.remaining_edges <= u64::MAX,
        terminal_has_payload ==> state.remaining_payloads > 0,
        state.detached_payloads < u64::MAX,
    ensures
        result.1.remaining_edges == state.remaining_edges - 1,
        result.1.pruned_edges == state.pruned_edges + 1,
        result.1.written_values == state.written_values,
        result.0 == barrier_free_step_kind(view),
        terminal_has_payload ==> (
            result.1.remaining_payloads == state.remaining_payloads - 1
            && result.1.detached_payloads == state.detached_payloads + 1
        ),
        !terminal_has_payload ==> (
            result.1.remaining_payloads == state.remaining_payloads
            && result.1.detached_payloads == state.detached_payloads
        ),
        result.1.remaining_payloads + result.1.detached_payloads
            == state.remaining_payloads + state.detached_payloads,
{
    let parent_stops = view.parent_is_root || view.parent_has_payload
        || view.left_sibling_edges != 0;
    let kind = if parent_stops {
        PhaseBStepKind::OneBytePrune
    } else {
        PhaseBStepKind::BranchCut
    };
    let detached_edges = match kind {
        PhaseBStepKind::BranchCut => view.left_sibling_edges + 1,
        PhaseBStepKind::OneBytePrune => 1,
    };
    (
        kind,
        ReversePruneState {
            remaining_edges: state.remaining_edges - detached_edges,
            remaining_payloads: if terminal_has_payload {
                state.remaining_payloads - 1
            } else {
                state.remaining_payloads
            },
            detached_payloads: if terminal_has_payload {
                state.detached_payloads + 1
            } else {
                state.detached_payloads
            },
            pruned_edges: state.pruned_edges + detached_edges,
            written_values: state.written_values,
        },
    )
}

/** Root values do not correspond to a logical edge.  The final root step
    conserves payloads and cannot increment the edge-prune counter. */
pub fn executable_root_payload_step(
    state: ReversePruneState,
    root_has_payload: bool,
) -> (result: ReversePruneState)
    requires
        root_has_payload ==> state.remaining_payloads > 0,
        state.detached_payloads < u64::MAX,
    ensures
        result.remaining_edges == state.remaining_edges,
        result.pruned_edges == state.pruned_edges,
        result.written_values == state.written_values,
        root_has_payload ==> (
            result.remaining_payloads == state.remaining_payloads - 1
            && result.detached_payloads == state.detached_payloads + 1
        ),
        !root_has_payload ==> (
            result.remaining_payloads == state.remaining_payloads
            && result.detached_payloads == state.detached_payloads
        ),
        result.remaining_payloads + result.detached_payloads
            == state.remaining_payloads + state.detached_payloads,
{
    ReversePruneState {
        remaining_edges: state.remaining_edges,
        remaining_payloads: if root_has_payload {
            state.remaining_payloads - 1
        } else {
            state.remaining_payloads
        },
        detached_payloads: if root_has_payload {
            state.detached_payloads + 1
        } else {
            state.detached_payloads
        },
        pruned_edges: state.pruned_edges,
        written_values: state.written_values,
    }
}

/** Raw-header ownership surrounding the reverse-prune machine.  A live raw
    header remains quarantined until both counters reach zero.  A fail-stop
    state has no continuation and never invokes ordinary recursive Drop. */
#[derive(Copy, Clone)]
pub struct QuarantineState {
    pub remaining_edges: u64,
    pub remaining_payloads: u64,
    pub raw_header_quarantined: bool,
    pub explicit_raw_drops: u64,
    pub ordinary_recursive_raw_drops: u64,
    pub live_continuations: u64,
    pub fail_stopped: bool,
    pub allocation_attempts: u64,
}

/** The only executable raw-header release transition. */
pub fn executable_drop_proven_empty_header(
    state: QuarantineState,
) -> (result: QuarantineState)
    requires
        state.raw_header_quarantined,
        state.remaining_edges == 0,
        state.remaining_payloads == 0,
        state.explicit_raw_drops < u64::MAX,
        state.ordinary_recursive_raw_drops == 0,
        !state.fail_stopped,
    ensures
        !result.raw_header_quarantined,
        result.remaining_edges == 0,
        result.remaining_payloads == 0,
        result.explicit_raw_drops == state.explicit_raw_drops + 1,
        result.ordinary_recursive_raw_drops == 0,
        result.live_continuations == state.live_continuations,
        !result.fail_stopped,
        result.allocation_attempts == state.allocation_attempts,
{
    QuarantineState {
        raw_header_quarantined: false,
        explicit_raw_drops: state.explicit_raw_drops + 1,
        ..state
    }
}

/** Unexpected panic is terminal.  The header stays quarantined for operating-
    system reclamation; execution cannot resume and recursive Drop is never
    selected. */
pub fn executable_unexpected_panic_fail_stop(
    state: QuarantineState,
) -> (result: QuarantineState)
    requires
        state.raw_header_quarantined,
        state.ordinary_recursive_raw_drops == 0,
    ensures
        result.fail_stopped,
        result.raw_header_quarantined,
        result.live_continuations == 0,
        result.explicit_raw_drops == state.explicit_raw_drops,
        result.ordinary_recursive_raw_drops == 0,
        result.remaining_edges == state.remaining_edges,
        result.remaining_payloads == state.remaining_payloads,
        result.allocation_attempts == state.allocation_attempts,
{
    QuarantineState {
        fail_stopped: true,
        live_continuations: 0,
        ..state
    }
}

/** Unique-anchor teardown performs no integration allocation. */
pub fn executable_unique_anchor_allocation_free(
    state: QuarantineState,
) -> (result: QuarantineState)
    requires
        state.raw_header_quarantined,
        !state.fail_stopped,
    ensures
        result.allocation_attempts == state.allocation_attempts,
        result.raw_header_quarantined == state.raw_header_quarantined,
        result.remaining_edges == state.remaining_edges,
        result.remaining_payloads == state.remaining_payloads,
        result.explicit_raw_drops == state.explicit_raw_drops,
        result.ordinary_recursive_raw_drops == state.ordinary_recursive_raw_drops,
        result.live_continuations == state.live_continuations,
        result.fail_stopped == state.fail_stopped,
{
    state
}

/** A failed cross-anchor CoW allocation is counted once and transitions
    directly to fail-stop without exposing the raw header to unwinding Drop. */
pub fn executable_cross_anchor_allocation_fail_stop(
    state: QuarantineState,
) -> (result: QuarantineState)
    requires
        state.raw_header_quarantined,
        state.ordinary_recursive_raw_drops == 0,
        state.allocation_attempts < u64::MAX,
    ensures
        result.fail_stopped,
        result.raw_header_quarantined,
        result.live_continuations == 0,
        result.explicit_raw_drops == state.explicit_raw_drops,
        result.ordinary_recursive_raw_drops == 0,
        result.remaining_edges == state.remaining_edges,
        result.remaining_payloads == state.remaining_payloads,
        result.allocation_attempts == state.allocation_attempts + 1,
{
    QuarantineState {
        fail_stopped: true,
        live_continuations: 0,
        allocation_attempts: state.allocation_attempts + 1,
        ..state
    }
}

#[derive(Copy, Clone)]
pub struct ParHandoffState {
    pub detached_pars: u64,
    pub queued_par_pda_jobs: u64,
}

/** Detaching one PathMap-owned Par and publishing its PDA job is one atomic
    ownership handoff: neither a skipped nor duplicated job refines it. */
pub fn executable_par_handoff_exactly_once(
    state: ParHandoffState,
) -> (result: ParHandoffState)
    requires
        state.detached_pars < u64::MAX,
        state.queued_par_pda_jobs < u64::MAX,
        state.detached_pars == state.queued_par_pda_jobs,
    ensures
        result.detached_pars == state.detached_pars + 1,
        result.queued_par_pda_jobs == state.queued_par_pda_jobs + 1,
        result.detached_pars == result.queued_par_pda_jobs,
{
    ParHandoffState {
        detached_pars: state.detached_pars + 1,
        queued_par_pda_jobs: state.queued_par_pda_jobs + 1,
    }
}

pub struct AbstractState {
    pub remaining_edges: int,
    pub remaining_payloads: int,
    pub detached_payloads: int,
    pub pruned_edges: int,
    pub written_values: int,
}

pub open spec fn true_count(flags: Seq<bool>) -> int
    decreases flags.len(),
{
    if flags.len() == 0 {
        0
    } else {
        true_count(flags.drop_last())
            + if flags.last() { 1int } else { 0int }
    }
}

pub open spec fn abstract_edge_step(
    state: AbstractState,
    view: StepView,
    terminal_has_payload: bool,
) -> AbstractState {
    AbstractState {
        remaining_edges: state.remaining_edges - barrier_free_step_detached_edges(view),
        remaining_payloads: state.remaining_payloads
            - if terminal_has_payload { 1int } else { 0int },
        detached_payloads: state.detached_payloads
            + if terminal_has_payload { 1int } else { 0int },
        pruned_edges: state.pruned_edges + barrier_free_step_detached_edges(view),
        written_values: state.written_values,
    }
}

pub open spec fn abstract_reverse_terminal_drive(
    views: Seq<StepView>,
    flags: Seq<bool>,
    state: AbstractState,
) -> AbstractState
    decreases flags.len(),
{
    if flags.len() == 0 {
        state
    } else {
        abstract_reverse_terminal_drive(
            views.drop_last(),
            flags.drop_last(),
            abstract_edge_step(state, views.last(), flags.last()),
        )
    }
}

proof fn reverse_terminal_drive_conserves_edges_and_payloads(
    views: Seq<StepView>,
    flags: Seq<bool>,
    state: AbstractState,
)
    requires
        views.len() == flags.len(),
        state.remaining_edges == flags.len(),
        state.remaining_payloads == true_count(flags),
        state.detached_payloads >= 0,
        state.pruned_edges >= 0,
    ensures
        abstract_reverse_terminal_drive(views, flags, state).remaining_edges == 0,
        abstract_reverse_terminal_drive(views, flags, state).remaining_payloads == 0,
        abstract_reverse_terminal_drive(views, flags, state).detached_payloads
            == state.detached_payloads + true_count(flags),
        abstract_reverse_terminal_drive(views, flags, state).pruned_edges
            == state.pruned_edges + flags.len(),
        abstract_reverse_terminal_drive(views, flags, state).written_values
            == state.written_values,
    decreases flags.len(),
{
    if flags.len() > 0 {
        let prefix_views = views.drop_last();
        let prefix = flags.drop_last();
        let stepped = abstract_edge_step(state, views.last(), flags.last());
        assert(barrier_free_step_detached_edges(views.last()) == 1);
        assert(prefix_views.len() == prefix.len());
        assert(stepped.remaining_edges == prefix.len());
        assert(stepped.remaining_payloads == true_count(prefix));
        reverse_terminal_drive_conserves_edges_and_payloads(prefix_views, prefix, stepped);
    }
}

}

fn main() {}
