use vstd::prelude::*;

verus! {

/** Executable projection of one public-zipper teardown transition.  The
    caller has already positioned the zipper at the reverse terminal path and
    has installed an immediate-parent barrier when required. */
#[derive(Copy, Clone)]
pub struct ReversePruneState {
    pub remaining_edges: u64,
    pub remaining_payloads: u64,
    pub detached_payloads: u64,
    pub pruned_edges: u64,
}

pub fn executable_one_logical_edge_step(
    state: ReversePruneState,
    terminal_has_payload: bool,
) -> (result: ReversePruneState)
    requires
        state.remaining_edges > 0,
        terminal_has_payload ==> state.remaining_payloads > 0,
        state.detached_payloads < u64::MAX,
        state.pruned_edges < u64::MAX,
    ensures
        result.remaining_edges == state.remaining_edges - 1,
        result.pruned_edges == state.pruned_edges + 1,
        terminal_has_payload ==> (
            result.remaining_payloads == state.remaining_payloads - 1
            && result.detached_payloads == state.detached_payloads + 1
        ),
        !terminal_has_payload ==> (
            result.remaining_payloads == state.remaining_payloads
            && result.detached_payloads == state.detached_payloads
        ),
        result.remaining_payloads + result.detached_payloads
            == state.remaining_payloads + state.detached_payloads,
{
    ReversePruneState {
        remaining_edges: state.remaining_edges - 1,
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
        pruned_edges: state.pruned_edges + 1,
    }
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
    }
}

pub struct AbstractState {
    pub remaining_edges: int,
    pub remaining_payloads: int,
    pub detached_payloads: int,
    pub pruned_edges: int,
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
    terminal_has_payload: bool,
) -> AbstractState {
    AbstractState {
        remaining_edges: state.remaining_edges - 1,
        remaining_payloads: state.remaining_payloads
            - if terminal_has_payload { 1int } else { 0int },
        detached_payloads: state.detached_payloads
            + if terminal_has_payload { 1int } else { 0int },
        pruned_edges: state.pruned_edges + 1,
    }
}

pub open spec fn abstract_reverse_terminal_drive(
    flags: Seq<bool>,
    state: AbstractState,
) -> AbstractState
    decreases flags.len(),
{
    if flags.len() == 0 {
        state
    } else {
        abstract_reverse_terminal_drive(
            flags.drop_last(),
            abstract_edge_step(state, flags.last()),
        )
    }
}

proof fn reverse_terminal_drive_conserves_edges_and_payloads(
    flags: Seq<bool>,
    state: AbstractState,
)
    requires
        state.remaining_edges == flags.len(),
        state.remaining_payloads == true_count(flags),
        state.detached_payloads >= 0,
        state.pruned_edges >= 0,
    ensures
        abstract_reverse_terminal_drive(flags, state).remaining_edges == 0,
        abstract_reverse_terminal_drive(flags, state).remaining_payloads == 0,
        abstract_reverse_terminal_drive(flags, state).detached_payloads
            == state.detached_payloads + true_count(flags),
        abstract_reverse_terminal_drive(flags, state).pruned_edges
            == state.pruned_edges + flags.len(),
    decreases flags.len(),
{
    if flags.len() > 0 {
        let prefix = flags.drop_last();
        let stepped = abstract_edge_step(state, flags.last());
        assert(stepped.remaining_edges == prefix.len());
        assert(stepped.remaining_payloads == true_count(prefix));
        reverse_terminal_drive_conserves_edges_and_payloads(prefix, stepped);
    }
}

}

fn main() {}
