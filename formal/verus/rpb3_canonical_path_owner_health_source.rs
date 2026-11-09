//! Reachable captured-owner health for the canonical-path Forward guard.
//!
//! `focus` enters `PersistentRegionOwners::capture` only with no state fault
//! and an Open current path. `capture` appends a clone of that path, records
//! the parent's outermost ID (or its own ID), and does not modify prior owner
//! entries. `select`, settlement, rejection, and resolution either retain or
//! discard owner references; they never rewrite a captured owner snapshot.
//! The source-to-model premises still owed are the actual frame-history
//! RegionPath candidates, checkpoint/owner-ID correspondence, and the mapping
//! of each Rust branch into the modeled trace actions below.

use vstd::prelude::*;

use crate::{
    rpb3_canonical_path_forward_capacity_source as capacity, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_restore_run_expansion as restore,
};

verus! {

pub enum SourcePathStatus {
    Open,
    SplitComplete,
    Dead,
}

pub struct SourceFocusState {
    pub current_status: SourcePathStatus,
    /// Identity of the entire saved `InnerPath` image, beyond its status.
    pub current_image: usize,
    pub owner_id: Option<usize>,
    pub fault: bool,
}

pub struct SourceRegionPathCandidate {
    /// The child held in the selected RegionPath frame.
    pub child_status: SourcePathStatus,
    pub child_image: usize,
}

pub struct SourceOwner {
    /// `current.clone()` at capture time; it is immutable thereafter.
    pub owner_status: SourcePathStatus,
    pub owner_image: usize,
    pub parent: Option<usize>,
    pub outermost: usize,
}

pub struct SourceOwnerMachine {
    pub ghost owners: Seq<SourceOwner>,
    pub state: SourceFocusState,
}

#[verifier::opaque]
pub open spec fn owner_store_wf(owners: Seq<SourceOwner>) -> bool {
    &&& owners.len() <= usize::MAX as int
    &&& forall|i: int| 0 <= i < owners.len() ==> {
        let entry = #[trigger] owners[i];
        &&& entry.owner_status == SourcePathStatus::Open
        &&& entry.outermost as int <= i
        &&& match entry.parent {
            None => entry.outermost as int == i,
            Some(parent) => {
                (parent as int) < i
                    && entry.outermost == owners[parent as int].outermost
            },
        }
    }
}

pub open spec fn focus_reference_wf(machine: SourceOwnerMachine) -> bool {
    match machine.state.owner_id {
        None => true,
        Some(id) => (id as int) < machine.owners.len(),
    }
}

pub open spec fn owner_machine_wf(machine: SourceOwnerMachine) -> bool {
    owner_store_wf(machine.owners) && focus_reference_wf(machine)
}

pub open spec fn focus_guard(state: SourceFocusState) -> bool {
    !state.fault && state.current_status == SourcePathStatus::Open
}

pub open spec fn captured_owner_for_focus(before: SourceOwnerMachine) -> SourceOwner
    recommends focus_reference_wf(before), before.owners.len() < usize::MAX as int,
{
    let id = before.owners.len() as usize;
    SourceOwner {
        owner_status: before.state.current_status,
        owner_image: before.state.current_image,
        parent: before.state.owner_id,
        outermost: match before.state.owner_id {
            None => id,
            Some(parent) => before.owners[parent as int].outermost,
        },
    }
}

/// One `focus` loop iteration. `top` represents the inspected frame slot:
/// None means it is absent or not `Region { phase: Path { .. } }`.
pub open spec fn focus_step(
    before: SourceOwnerMachine,
    top: Option<SourceRegionPathCandidate>,
) -> SourceOwnerMachine
    recommends owner_machine_wf(before),
        !focus_guard(before.state) || top.is_none()
            || before.owners.len() < usize::MAX as int,
{
    if !focus_guard(before.state) || top.is_none() {
        before
    } else {
        let id = before.owners.len() as usize;
        let captured = captured_owner_for_focus(before);
        SourceOwnerMachine {
            owners: before.owners.push(captured),
            state: SourceFocusState {
                current_status: top.unwrap().child_status,
                current_image: top.unwrap().child_image,
                owner_id: Some(id),
                fault: before.state.fault,
            },
        }
    }
}

/// An inspected sequence of RegionPath frames. Once a guard fails or no
/// RegionPath frame is present, the production loop stops; subsequent supplied
/// observations are ignored.
pub open spec fn focus_run(
    before: SourceOwnerMachine,
    tops: Seq<Option<SourceRegionPathCandidate>>,
) -> SourceOwnerMachine
    decreases tops.len()
{
    if tops.len() == 0 || !focus_guard(before.state) || tops[0].is_none()
        || before.owners.len() >= usize::MAX as int
    {
        before
    } else {
        focus_run(
            focus_step(before, tops[0]),
            tops.subrange(1, tops.len() as int),
        )
    }
}

/// `select` returns `before.clone()` with a fault on a dead child inside a
/// captured owner. Other candidates pass to `focus` unchanged.
pub open spec fn select_before_focus(
    before: SourceFocusState,
    candidate: SourceFocusState,
) -> SourceFocusState {
    if candidate.current_status == SourcePathStatus::Dead && before.owner_id.is_some() {
        SourceFocusState {
            current_status: before.current_status,
            current_image: before.current_image,
            owner_id: before.owner_id,
            fault: true,
        }
    } else {
        candidate
    }
}

/// One completed `settle` pop: the new current path is computed from the
/// immutable saved owner and child, while the owner reference becomes parent.
pub open spec fn settle_parent(
    before: SourceOwnerMachine,
    current_status: SourcePathStatus,
    current_image: usize,
) -> SourceOwnerMachine
    recommends focus_reference_wf(before), before.state.owner_id.is_some(),
{
    let id = before.state.owner_id.unwrap();
    SourceOwnerMachine {
        owners: before.owners,
        state: SourceFocusState {
            current_status,
            current_image,
            owner_id: before.owners[id as int].parent,
            fault: before.state.fault,
        },
    }
}

/// The dead-error resolution branch materializes its owner chain, resolves
/// into a new root, clears owner/fault references, then calls `focus`.
pub open spec fn resolved_materialized_root(
    before: SourceOwnerMachine,
    current_status: SourcePathStatus,
    current_image: usize,
) -> SourceOwnerMachine {
    SourceOwnerMachine {
        owners: before.owners,
        state: SourceFocusState {
            current_status,
            current_image,
            owner_id: None,
            fault: false,
        },
    }
}

pub enum SourceOwnerAction {
    Focus { top: Option<SourceRegionPathCandidate> },
    Select { candidate: SourceFocusState },
    SettleParent { current_status: SourcePathStatus, current_image: usize },
    ResolveMaterialized { current_status: SourcePathStatus, current_image: usize },
    Reject,
}

pub open spec fn action_wf(before: SourceOwnerMachine, action: SourceOwnerAction) -> bool {
    match action {
        SourceOwnerAction::Focus { top } => {
            !focus_guard(before.state) || top.is_none()
                || before.owners.len() < usize::MAX as int
        },
        SourceOwnerAction::Select { candidate } => match candidate.owner_id {
            None => true,
            Some(id) => (id as int) < before.owners.len(),
        },
        SourceOwnerAction::SettleParent { .. } => before.state.owner_id.is_some(),
        SourceOwnerAction::ResolveMaterialized { .. } => true,
        SourceOwnerAction::Reject => true,
    }
}

pub open spec fn owner_source_step(
    before: SourceOwnerMachine,
    action: SourceOwnerAction,
) -> SourceOwnerMachine {
    match action {
        SourceOwnerAction::Focus { top } => focus_step(before, top),
        SourceOwnerAction::Select { candidate } => SourceOwnerMachine {
            owners: before.owners,
            state: select_before_focus(before.state, candidate),
        },
        SourceOwnerAction::SettleParent { current_status, current_image } =>
            settle_parent(before, current_status, current_image),
        SourceOwnerAction::ResolveMaterialized { current_status, current_image } =>
            resolved_materialized_root(before, current_status, current_image),
        SourceOwnerAction::Reject => SourceOwnerMachine {
            owners: before.owners,
            state: SourceFocusState {
                current_status: before.state.current_status,
                current_image: before.state.current_image,
                owner_id: before.state.owner_id,
                fault: true,
            },
        },
    }
}

pub open spec fn owner_source_trace(
    before: SourceOwnerMachine,
    actions: Seq<SourceOwnerAction>,
) -> SourceOwnerMachine
    decreases actions.len()
{
    if actions.len() == 0 {
        before
    } else {
        owner_source_trace(
            owner_source_step(before, actions[0]),
            actions.subrange(1, actions.len() as int),
        )
    }
}

pub open spec fn owner_source_trace_wf(
    before: SourceOwnerMachine,
    actions: Seq<SourceOwnerAction>,
) -> bool
    decreases actions.len()
{
    if actions.len() == 0 {
        true
    } else {
        &&& action_wf(before, actions[0])
        &&& owner_source_trace_wf(
            owner_source_step(before, actions[0]),
            actions.subrange(1, actions.len() as int),
        )
    }
}

pub open spec fn root_path_open(machine: SourceOwnerMachine) -> bool {
    match machine.state.owner_id {
        None => machine.state.current_status == SourcePathStatus::Open,
        Some(id) => {
            let outermost = machine.owners[id as int].outermost;
            machine.owners[outermost as int].owner_status == SourcePathStatus::Open
        },
    }
}

pub open spec fn observed_fault_free(machine: SourceOwnerMachine) -> bool {
    !machine.state.fault
        && machine.state.current_status != SourcePathStatus::Dead
        && root_path_open(machine)
}

/// Open the quantified store invariant only for the one entry a caller needs.
pub proof fn captured_owner_entry_facts(owners: Seq<SourceOwner>, i: int)
    requires owner_store_wf(owners), 0 <= i < owners.len(),
    ensures
        owners[i].owner_status == SourcePathStatus::Open,
        owners[i].outermost as int <= i,
{
    reveal(owner_store_wf);
}

/// A push does not change an old entry or the earlier entry named by its
/// parent. Prove this at one index before introducing the new-store quantifier.
pub proof fn old_captured_owner_entry_survives_push(
    owners: Seq<SourceOwner>,
    captured: SourceOwner,
    i: int,
)
    requires
        owner_store_wf(owners),
        0 <= i < owners.len(),
    ensures
        owners.push(captured)[i] == owners[i],
        {
            let pushed = owners.push(captured);
            let entry = pushed[i];
            &&& entry.owner_status == SourcePathStatus::Open
            &&& entry.outermost as int <= i
            &&& match entry.parent {
                None => entry.outermost as int == i,
                Some(parent) => {
                    (parent as int) < i
                        && entry.outermost == pushed[parent as int].outermost
                },
            }
        },
{
    reveal(owner_store_wf);
    let pushed = owners.push(captured);
    assert(pushed[i] == owners[i]);
    let entry = owners[i];
    assert(entry.owner_status == SourcePathStatus::Open);
    assert(entry.outermost as int <= i);
    match entry.parent {
        None => {
            assert(entry.outermost as int == i);
        },
        Some(parent) => {
            assert((parent as int) < i);
            assert(entry.outermost == owners[parent as int].outermost);
            assert(pushed[parent as int] == owners[parent as int]);
        },
    }
}

/// The appended entry is Open and either its own outermost root or linked to
/// a strictly earlier, already bounded parent.
pub proof fn new_captured_owner_entry_is_well_formed(
    owners: Seq<SourceOwner>,
    captured: SourceOwner,
)
    requires
        owner_store_wf(owners),
        captured.owner_status == SourcePathStatus::Open,
        match captured.parent {
            None => captured.outermost as int == owners.len(),
            Some(parent) => {
                (parent as int) < owners.len()
                    && captured.outermost == owners[parent as int].outermost
            },
        },
    ensures {
        let pushed = owners.push(captured);
        let i = owners.len() as int;
        let entry = pushed[i];
        &&& entry.owner_status == SourcePathStatus::Open
        &&& entry.outermost as int <= i
        &&& match entry.parent {
            None => entry.outermost as int == i,
            Some(parent) => {
                (parent as int) < i
                    && entry.outermost == pushed[parent as int].outermost
            },
        }
    },
{
    reveal(owner_store_wf);
    let pushed = owners.push(captured);
    let i = owners.len() as int;
    assert(pushed[i] == captured);
    match captured.parent {
        None => {
            assert(captured.outermost as int == i);
        },
        Some(parent) => {
            assert((parent as int) < i);
            assert(pushed[parent as int] == owners[parent as int]);
            assert(owners[parent as int].outermost as int <= parent as int);
            assert(captured.outermost as int <= i);
        },
    }
}

/// Combine the two pointwise lemmas into the original quantified invariant.
pub proof fn append_captured_open_owner_preserves_store(
    owners: Seq<SourceOwner>,
    captured: SourceOwner,
)
    requires
        owner_store_wf(owners),
        owners.len() < usize::MAX as int,
        captured.owner_status == SourcePathStatus::Open,
        match captured.parent {
            None => captured.outermost as int == owners.len(),
            Some(parent) => {
                (parent as int) < owners.len()
                    && captured.outermost == owners[parent as int].outermost
            },
        },
    ensures
        owner_store_wf(owners.push(captured)),
        forall|i: int| 0 <= i < owners.len() ==>
            #[trigger] owners.push(captured)[i] == owners[i],
{
    let pushed = owners.push(captured);
    assert(pushed.len() == owners.len() + 1);
    assert(owner_store_wf(pushed)) by {
        reveal(owner_store_wf);
        assert forall|i: int| 0 <= i < pushed.len() implies {
            let entry = #[trigger] pushed[i];
            &&& entry.owner_status == SourcePathStatus::Open
            &&& entry.outermost as int <= i
            &&& match entry.parent {
                None => entry.outermost as int == i,
                Some(parent) => {
                    (parent as int) < i
                        && entry.outermost == pushed[parent as int].outermost
                },
            }
        } by {
            if i < owners.len() {
                old_captured_owner_entry_survives_push(owners, captured, i);
            } else {
                assert(i == owners.len());
                new_captured_owner_entry_is_well_formed(owners, captured);
            }
        }
    }
    assert forall|i: int| 0 <= i < owners.len() implies
        #[trigger] pushed[i] == owners[i] by {
        old_captured_owner_entry_survives_push(owners, captured, i);
    }
}

pub proof fn focus_step_preserves_captured_owner_health(
    before: SourceOwnerMachine,
    top: Option<SourceRegionPathCandidate>,
)
    requires
        owner_machine_wf(before),
        before.owners.len() < usize::MAX as int,
    ensures
        owner_machine_wf(focus_step(before, top)),
        forall|i: int| 0 <= i < before.owners.len() ==>
            #[trigger] focus_step(before, top).owners[i] == before.owners[i],
{
    if focus_guard(before.state) && top.is_some() {
        let after = focus_step(before, top);
        let captured = captured_owner_for_focus(before);
        assert(after.owners == before.owners.push(captured));
        assert(after.owners.len() == before.owners.len() + 1);
        assert(captured.owner_status == SourcePathStatus::Open);
        match before.state.owner_id {
            None => {
                assert(captured.outermost as int == before.owners.len());
            },
            Some(parent) => {
                assert((parent as int) < before.owners.len());
                assert(captured.outermost == before.owners[parent as int].outermost);
            },
        }
        append_captured_open_owner_preserves_store(before.owners, captured);
        assert(after.owners[before.owners.len() as int].owner_status == SourcePathStatus::Open);
        assert(after.owners[before.owners.len() as int].owner_image == before.state.current_image);
        assert(owner_store_wf(after.owners));
        assert(focus_reference_wf(after));
        assert forall|i: int| 0 <= i < before.owners.len() implies
            #[trigger] after.owners[i] == before.owners[i] by {
            assert(after.owners[i] == before.owners[i]);
        }
    } else {
        assert(focus_step(before, top) == before);
    }
}

pub proof fn owner_source_step_preserves_health(
    before: SourceOwnerMachine,
    action: SourceOwnerAction,
)
    requires owner_machine_wf(before), action_wf(before, action),
    ensures owner_machine_wf(owner_source_step(before, action)),
{
    match action {
        SourceOwnerAction::Focus { top } => {
            if focus_guard(before.state) && top.is_some() {
                focus_step_preserves_captured_owner_health(before, top);
            }
        },
        SourceOwnerAction::Select { candidate } => {
            let after = owner_source_step(before, action);
            assert(after.owners == before.owners);
            if candidate.current_status == SourcePathStatus::Dead
                && before.state.owner_id.is_some()
            {
                assert(after.state.owner_id == before.state.owner_id);
                assert(focus_reference_wf(before));
            } else {
                assert(after.state.owner_id == candidate.owner_id);
                match candidate.owner_id {
                    None => {},
                    Some(id) => {
                        assert((id as int) < before.owners.len());
                    },
                }
            }
            assert(focus_reference_wf(after));
        },
        SourceOwnerAction::SettleParent { .. } => {
            reveal(owner_store_wf);
            let id = before.state.owner_id.unwrap();
            assert((id as int) < before.owners.len());
            assert(match before.owners[id as int].parent {
                None => true,
                Some(parent) => (parent as int) < id as int,
            });
            assert(focus_reference_wf(owner_source_step(before, action)));
        },
        SourceOwnerAction::ResolveMaterialized { .. } => {},
        SourceOwnerAction::Reject => {},
    }
}

pub proof fn reachable_captured_owners_remain_open(
    before: SourceOwnerMachine,
    actions: Seq<SourceOwnerAction>,
)
    requires owner_machine_wf(before), owner_source_trace_wf(before, actions),
    ensures owner_machine_wf(owner_source_trace(before, actions)),
    decreases actions.len(),
{
    if actions.len() > 0 {
        owner_source_step_preserves_health(before, actions[0]);
        reachable_captured_owners_remain_open(
            owner_source_step(before, actions[0]),
            actions.subrange(1, actions.len() as int),
        );
    }
}

/// This is the health implication needed by the Forward-capacity source
/// projection. It follows from capture/owner immutability and current Open,
/// without assuming an observed `owner_root_open` or `fault_free` flag.
pub proof fn reachable_positive_capacity_has_open_owner_root(
    before: SourceOwnerMachine,
    actions: Seq<SourceOwnerAction>,
)
    requires
        owner_machine_wf(before),
        owner_source_trace_wf(before, actions),
        owner_source_trace(before, actions).state.current_status == SourcePathStatus::Open,
        !owner_source_trace(before, actions).state.fault,
    ensures
        root_path_open(owner_source_trace(before, actions)),
        observed_fault_free(owner_source_trace(before, actions)),
{
    reachable_captured_owners_remain_open(before, actions);
    let after = owner_source_trace(before, actions);
    if after.state.owner_id.is_some() {
        let id = after.state.owner_id.unwrap();
        assert((id as int) < after.owners.len());
        captured_owner_entry_facts(after.owners, id as int);
        assert((after.owners[id as int].outermost as int) < after.owners.len());
        captured_owner_entry_facts(after.owners, after.owners[id as int].outermost as int);
    }
}

/// The source facts independent of owner health. No `owner_root_open`,
/// `fault_free`, or final `forward_capacity_facts_match` condition appears
/// here; the positive branch retains the exact capacity and Payload arithmetic
/// plus the indexed byte-view projection required by the existing theorem.
pub open spec fn forward_arithmetic_payload_view_projection(
    observed: capacity::ObservedForwardCapacity,
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
) -> bool {
    &&& capacity::test_bulk_mode_wf(observed)
    &&& observed.capacity == capacity::exact_forward_capacity(observed)
    &&& restore::source_tree_wf(base)
    &&& epm::indexed_image_wf(source)
    &&& base.bytes.len() == observed.depth as int
    &&& start as int + observed.available as int <= source.bytes.len()
    &&& if observed.capacity > 0 {
        &&& observed.stamp == observed.depth
        &&& observed.pending.is_some()
        &&& observed.top_payload_remaining.is_some()
        &&& !observed.pending.unwrap().awaiting_resolution
        &&& !pending.awaiting_resolution
        &&& observed.pending.unwrap().payload_start == pending.payload_start
        &&& pending.payload_start as int <= base.bytes.len()
        &&& observed.pending.unwrap().remaining == observed.top_payload_remaining.unwrap()
        &&& observed.pending.unwrap().remaining as int
            == pending.payload_len as int
                - (base.bytes.len() - pending.payload_start as int)
    } else {
        true
    }
}

/// These are field equalities with the traced Rust state, not assumptions
/// that either health flag is true. They remain Rust-to-model projection
/// obligations until the production owner/checkpoint state is extracted.
pub open spec fn forward_observer_matches_owner_trace(
    observed: capacity::ObservedForwardCapacity,
    after: SourceOwnerMachine,
) -> bool {
    if observed.capacity > 0 {
        &&& observed.open == (after.state.current_status == SourcePathStatus::Open)
        &&& observed.state_fault_none == !after.state.fault
        &&& observed.owner_root_open == root_path_open(after)
        &&& observed.fault_free == observed_fault_free(after)
    } else {
        true
    }
}

/// Compose reachable capture/selection health with the already verified
/// source capacity arithmetic for every positive accepted width. The owner
/// flags become consequences of the trace rather than positive premises.
pub proof fn accepted_forward_facts_from_reachable_owner_trace(
    before: SourceOwnerMachine,
    actions: Seq<SourceOwnerAction>,
    observed: capacity::ObservedForwardCapacity,
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
)
    requires
        owner_machine_wf(before),
        owner_source_trace_wf(before, actions),
        forward_arithmetic_payload_view_projection(observed, base, pending, source, start),
        forward_observer_matches_owner_trace(
            observed, owner_source_trace(before, actions),
        ),
        width > 0,
        width <= observed.capacity,
    ensures restore::forward_capacity_facts_match(
        base, pending, source, start, width,
        capacity::forward_facts_from_source(observed),
    ),
{
    capacity::positive_forward_capacity_bounds(observed, width);
    let after = owner_source_trace(before, actions);
    assert(after.state.current_status == SourcePathStatus::Open);
    assert(!after.state.fault);
    reachable_positive_capacity_has_open_owner_root(before, actions);
    assert(root_path_open(after));
    assert(observed_fault_free(after));
    assert(observed.owner_root_open);
    assert(observed.fault_free);
    assert(capacity::forward_source_projection(
        observed, base, pending, source, start,
    ));
    capacity::accepted_width_derives_forward_capacity_facts(
        observed, base, pending, source, start, width,
    );
}

/// Nested captures followed by a dead child select leave the immutable
/// outermost owner Open, even though the selected focused state is faulted.
pub proof fn nested_owner_dead_child_witness() {
    let initial = SourceOwnerMachine {
        owners: Seq::empty(),
        state: SourceFocusState {
            current_status: SourcePathStatus::Open,
            current_image: 1,
            owner_id: None,
            fault: false,
        },
    };
    let first = owner_source_step(initial, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 2,
        }),
    });
    let second = owner_source_step(first, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 3,
        }),
    });
    let selected = owner_source_step(second, SourceOwnerAction::Select {
        candidate: SourceFocusState {
            current_status: SourcePathStatus::Dead,
            current_image: 4,
            owner_id: Some(1),
            fault: false,
        },
    });
    assert(owner_store_wf(initial.owners)) by {
        reveal(owner_store_wf);
    }
    assert(owner_machine_wf(initial));
    assert(action_wf(initial, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 2,
        }),
    }));
    owner_source_step_preserves_health(initial, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 2,
        }),
    });
    assert(action_wf(first, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 3,
        }),
    }));
    owner_source_step_preserves_health(first, SourceOwnerAction::Focus {
        top: Some(SourceRegionPathCandidate {
            child_status: SourcePathStatus::Open,
            child_image: 3,
        }),
    });
    assert(action_wf(second, SourceOwnerAction::Select {
        candidate: SourceFocusState {
            current_status: SourcePathStatus::Dead,
            current_image: 4,
            owner_id: Some(1),
            fault: false,
        },
    }));
    owner_source_step_preserves_health(second, SourceOwnerAction::Select {
        candidate: SourceFocusState {
            current_status: SourcePathStatus::Dead,
            current_image: 4,
            owner_id: Some(1),
            fault: false,
        },
    });
    assert(owner_machine_wf(selected));
    assert(second.owners[1].outermost == 0);
    assert(second.owners[0].owner_status == SourcePathStatus::Open);
    assert(selected.state.fault);
    assert(root_path_open(selected));
}

}

fn main() {}
