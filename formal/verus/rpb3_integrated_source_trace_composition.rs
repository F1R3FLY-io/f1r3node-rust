//! Induction over the single live admission transition vocabulary.
//!
//! A successful Rust driver trace still has to be projected into
//! `live_runtime_product_step` at every transition. This module composes those
//! checked local transitions once that source extraction has been established;
//! it does not treat a passing local proof as a Rust-to-model projection.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_epathmap_end_source_selector as end_source,
    rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
    rpb3_epm_loop_continuation_source as epm_loop,
    rpb3_epm_source_accept_refinement as epm_accept,
    rpb3_epm_unwind_event_lift as unwind,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_map_frame_refinement as map,
    rpb3_score_admission_bridge as score_bridge,
    rpb3_runtime_frame_source_refinement as runtime_source,
};

verus! {

/// One root-to-return trace uses the same product state at each adjacent
/// transition. In particular, it cannot splice independently verified local
/// witnesses with incompatible frame, score, source, or cursor images.
pub open spec fn integrated_source_trace(
    states: Seq<integrated::LiveRuntimeProductState>,
    actions: Seq<integrated::LiveRuntimeProductAction>,
) -> bool {
    &&& states.len() == actions.len() + 1
    &&& integrated::live_runtime_product_wf(states[0])
    &&& forall|i: int| 0 <= i < actions.len() ==> #[trigger]
        integrated::live_runtime_product_step(
            states[i], states[i + 1], actions[i],
        )
}

pub proof fn integrated_source_trace_prefix_preserves_wf(
    states: Seq<integrated::LiveRuntimeProductState>,
    actions: Seq<integrated::LiveRuntimeProductAction>,
    upto: int,
)
    requires
        integrated_source_trace(states, actions),
        0 <= upto <= actions.len(),
    ensures integrated::live_runtime_product_wf(states[upto]),
    decreases upto,
{
    if upto > 0 {
        integrated_source_trace_prefix_preserves_wf(states, actions, upto - 1);
        let index = upto - 1;
        assert(0 <= index < actions.len());
        assert(forall|i: int| 0 <= i < actions.len() ==> #[trigger]
            integrated::live_runtime_product_step(
                states[i], states[i + 1], actions[i],
            ));
        assert(integrated::live_runtime_product_step(
            states[index], states[index + 1], actions[index],
        ));
        integrated::live_runtime_product_step_preserves_wf(
            states[index], states[index + 1], actions[index],
        );
    }
}

/// The only final action admitted by this theorem is the actual root EOF
/// return. The existing root-return theorem then proves exact frame
/// exhaustion and score/summary agreement for this trace's final state.
pub proof fn integrated_source_trace_root_return_exact(
    states: Seq<integrated::LiveRuntimeProductState>,
    actions: Seq<integrated::LiveRuntimeProductAction>,
    claimed_score: Seq<u8>,
)
    requires
        integrated_source_trace(states, actions),
        actions.len() > 0,
        match actions.last() {
            integrated::LiveRuntimeProductAction::RootComplete { .. } => true,
            _ => false,
        },
    ensures
        integrated::live_runtime_product_wf(states.last()),
        match actions.last() {
            integrated::LiveRuntimeProductAction::RootComplete { returned } => {
                &&& returned.remaining_frames == 0
                &&& returned.pre_pop == states.last().source.runtime
                &&& returned.score_bytes == states.last().source.product.bridge.admission.arena[
                    returned.summary.score_handle as int
                ].transcript_bytes
                &&& integrated::rpb3_score_admission_bridge::bridge_root_first_difference(
                    states.last().source.product.bridge, claimed_score,
                ).is_none() == integrated::rpb3_score_transcript::claimed_score_matches(
                    states.last().source.product.bridge.transcript,
                    returned.summary.score_handle,
                    claimed_score,
                )
            },
            _ => false,
        },
{
    let last = actions.len() as int - 1;
    integrated_source_trace_prefix_preserves_wf(states, actions, last);
    assert(forall|i: int| 0 <= i < actions.len() ==> #[trigger]
        integrated::live_runtime_product_step(
            states[i], states[i + 1], actions[i],
        ));
    assert(integrated::live_runtime_product_step(
        states[last], states[last + 1], actions[last],
    ));
    match actions[last] {
        integrated::LiveRuntimeProductAction::RootComplete { returned } => {
            integrated::live_runtime_product_root_complete_is_exact(
                states[last], states[last + 1], returned, claimed_score,
            );
        },
        _ => {},
    }
    assert(states[last + 1] == states.last());
    assert(actions[last] == actions.last());
}

pub enum CheckedOtherCompleteOutcomeImage {
    Root,
    AcceptedMessage,
    AcceptedOther,
}

pub enum CheckedOtherCompleteEffectImage {
    None,
    Stored,
    EPathMapValue,
    ResolvedObligation,
}

pub struct CheckedOtherEpmCompleteSourceImage {
    pub ghost terminal: terminal::EpmTerminalSourceObservation,
    pub stack_len_before: usize,
    pub completed_kind: u8,
    pub score_len_before: usize,
    pub score_len_after_finish: usize,
    pub stack_len_after: usize,
    pub outcome: CheckedOtherCompleteOutcomeImage,
    pub effect: CheckedOtherCompleteEffectImage,
    pub resolutions_before_accept: usize,
    pub resolutions_after_accept: usize,
}

pub open spec fn checked_other_epm_complete_source_ready(
    before: integrated::LiveRuntimeProductState,
    observed: CheckedOtherEpmCompleteSourceImage,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || before.cursors.len() <= 1
        || before.cursors.last().is_some()
        || integrated::product_linked_top(before.source.product).is_none()
    { false } else {
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        &&& observed.stack_len_before as int
            == before.source.product.bridge.admission.frames.len()
        &&& observed.completed_kind == 3
        &&& before.source.product.bridge.admission.frames.last().frame_kind == 3
        &&& observed.score_len_before as int
            == before.source.product.bridge.transcript.entries.len()
        &&& observed.score_len_after_finish == observed.score_len_before
        &&& observed.stack_len_after as int + 1
            == observed.stack_len_before as int
        &&& observed.outcome is AcceptedMessage
        &&& observed.effect is Stored
        &&& observed.resolutions_after_accept
            == observed.resolutions_before_accept
        &&& terminal::epm_terminal_complete_from_source(
            prior.epm, observed.terminal,
        ).is_some()
        &&& epm_accept::epm_source_finish_accept(prior).is_some()
        &&& epm_accept::epm_source_finish_accept(prior).unwrap().frames.len() > 0
    }
}

pub open spec fn checked_other_epm_complete_product_after(
    before: integrated::LiveRuntimeProductState,
) -> integrated::ProductScoreState
    recommends integrated::product_linked_top(before.source.product).is_some(),
        epm_accept::epm_source_finish_accept(
            integrated::product_linked_top(before.source.product).unwrap(),
        ).is_some(),
        epm_accept::epm_source_finish_accept(
            integrated::product_linked_top(before.source.product).unwrap(),
        ).unwrap().frames.len() > 0,
{
    let product = before.source.product;
    let prior = integrated::product_linked_top(product).unwrap();
    let accepted = epm_accept::epm_source_finish_accept(prior).unwrap();
    let provisional = integrated::ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge {
            admission: accepted,
            transcript: product.bridge.transcript,
        },
        map: map::IntegratedAdmissionState {
            admission: accepted,
            map_frames: product.map.map_frames.drop_last(),
            transcript: product.map.transcript,
        },
        epm_frames: product.epm_frames.drop_last(),
        indexed_regions: product.indexed_regions.drop_last(),
        accepted_epm_sources: Seq::empty(),
    };
    integrated::ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(
            product, provisional,
        ).update(accepted.frames.len() - 1, Some(prior)),
        ..provisional
    }
}

pub open spec fn checked_other_epm_complete_live_after(
    before: integrated::LiveRuntimeProductState,
) -> integrated::LiveRuntimeProductState
    recommends integrated::product_linked_top(before.source.product).is_some(),
        epm_accept::epm_source_finish_accept(
            integrated::product_linked_top(before.source.product).unwrap(),
        ).is_some(),
        epm_accept::epm_source_finish_accept(
            integrated::product_linked_top(before.source.product).unwrap(),
        ).unwrap().frames.len() > 0,
{
    let product = checked_other_epm_complete_product_after(before);
    integrated::LiveRuntimeProductState {
        source: integrated::LiveSourceProductState {
            product,
            runtime: runtime_source::runtime_after_admission_step(
                before.source.runtime, product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire.drop_last(),
        },
        cursors: before.cursors.drop_last(),
    }
}

pub proof fn checked_other_epm_complete_selects_live_action(
    before: integrated::LiveRuntimeProductState,
    observed: CheckedOtherEpmCompleteSourceImage,
)
    requires checked_other_epm_complete_source_ready(before, observed),
    ensures {
        let after = checked_other_epm_complete_live_after(before);
        &&& integrated::live_runtime_product_step(
            before, after, integrated::LiveRuntimeProductAction::EpmFinishAccept,
        )
        &&& integrated::live_runtime_product_wf(after)
        &&& after.source.product.bridge.admission.frames.len()
            == observed.stack_len_after as int
        &&& after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1
        &&& after.source.product.bridge.transcript.entries.len()
            == observed.score_len_after_finish as int
        &&& after.source.product.accepted_epm_sources.last()
            == integrated::product_linked_top(before.source.product)
    },
{
    let product = before.source.product;
    let prior = integrated::product_linked_top(product).unwrap();
    let accepted = epm_accept::epm_source_finish_accept(prior).unwrap();
    let after = checked_other_epm_complete_live_after(before);
    let finished = epm_accept::epm_end_linked(prior);
    assert(epm_accept::epm_source_accepts_into_program31(
        prior, finished, accepted,
    ));
    epm_accept::epm_source_accept_carries_end_summary(prior, finished, accepted);
    assert(after.source.product.bridge.admission == accepted);
    assert(after.source.product.accepted_epm_sources.len()
        == accepted.frames.len());
    assert(after.source.product.accepted_epm_sources.last() == Some(prior));
    assert(integrated::product_epm_finish_accept_step(product, after.source.product));
    assert(integrated::live_runtime_product_epm_finish_accept_step(before, after));
    integrated::live_runtime_product_epm_finish_accept_preserves_wf(before, after);
    assert(integrated::live_runtime_product_step(
        before, after, integrated::LiveRuntimeProductAction::EpmFinishAccept,
    ));
}

/// The terminal selector and the accepted parent return construct the same
/// End frame from the same pre-End EPathMap frame. The selector's End is a
/// proof witness; it is not a preceding checked driver transition.
pub proof fn checked_other_epm_terminal_end_matches_accept_end(
    before: integrated::rpb3_epathmap_event_refinement::EpmFrame,
)
    ensures terminal::epm_terminal_end_frame(before)
        == epm_accept::epm_end_frame(before),
{
    assert(terminal::epm_terminal_end_frame(before)
        == epm_accept::epm_end_frame(before));
}

/// A finite internal loop selects EOF Complete at its final pre-End state.
/// The checked driver then performs one atomic End, pop, and program-31
/// acceptance from that *same* state, carrying its source into the receipt.
pub proof fn checked_other_epm_complete_composes_terminal_selector(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<epm_loop::EpmLoopInternalTick>,
    observed: CheckedOtherEpmCompleteSourceImage,
)
    requires
        epm_loop::finite_loop_to_return(
            states, attempts,
            epm_loop::EpmLoopStop::Complete { observed: observed.terminal },
        ),
        checked_other_epm_complete_source_ready(states.last(), observed),
    ensures {
        let before = states.last();
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let selected = terminal::epm_terminal_complete_from_source(
            prior.epm, observed.terminal,
        ).unwrap();
        let after = checked_other_epm_complete_live_after(before);
        &&& epm_loop::loop_selected_action(
            before,
            epm_loop::EpmLoopStop::Complete { observed: observed.terminal },
        ) == Some(selected.action)
        &&& selected.after == epm_accept::epm_end_frame(prior.epm)
        &&& integrated::live_runtime_product_step(
            before, after, integrated::LiveRuntimeProductAction::EpmFinishAccept,
        )
        &&& integrated::live_runtime_product_wf(after)
        &&& after.source.product.accepted_epm_sources.last() == Some(prior)
        &&& after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1
        &&& after.source.product.bridge.admission.frames.len()
            == observed.stack_len_after as int
    },
{
    epm_loop::finite_loop_complete_dispatches_exact_image(
        states, attempts, observed.terminal,
    );
    checked_other_epm_complete_selects_live_action(states.last(), observed);
    let prior = integrated::product_linked_top(states.last().source.product).unwrap();
    checked_other_epm_terminal_end_matches_accept_end(prior.epm);
    assert(terminal::epm_terminal_complete_from_source(
        prior.epm, observed.terminal,
    ).unwrap().after == terminal::epm_terminal_end_frame(prior.epm));
}

/// Expand one successful internal EPathMap loop tick into its actual scalar
/// product states. Forward uses the checked scalar projection; Unwind uses
/// the arbitrary-width virtual states proved by the unwind lift.
pub open spec fn epm_tick_scalar_states(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    tick: epm_loop::EpmLoopInternalTick,
) -> Seq<integrated::LiveRuntimeProductState> {
    match tick {
        epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch { .. } =>
            seq![before, after],
        epm_loop::EpmLoopInternalTick::AcceptedForward {
            expanded_states, ..
        } => expanded_states,
        epm_loop::EpmLoopInternalTick::AcceptedUnwind {
            source, run, prefix, width, ..
        } => Seq::new(width as nat + 1, |i: int|
            unwind_product::live_runtime_unwind_virtual_state(
                before, source, run, prefix, i as usize,
            )),
    }
}

pub open spec fn epm_continue_actions(
    events: Seq<epm::EpmEvent>,
) -> Seq<integrated::LiveRuntimeProductAction> {
    Seq::new(events.len(), |i: int|
        integrated::LiveRuntimeProductAction::EpmContinue { event: events[i] })
}

pub open spec fn epm_tick_scalar_actions(
    tick: epm_loop::EpmLoopInternalTick,
) -> Seq<integrated::LiveRuntimeProductAction> {
    match tick {
        epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch { event } =>
            epm_continue_actions(seq![event]),
        epm_loop::EpmLoopInternalTick::AcceptedForward {
            expanded_events, ..
        } => epm_continue_actions(expanded_events),
        epm_loop::EpmLoopInternalTick::AcceptedUnwind {
            start, width, ..
        } => epm_continue_actions(Seq::new(width as nat, |i: int|
            unwind::unwind_event(start, width, i))),
    }
}

pub open spec fn epm_actions_are_only_continue(
    actions: Seq<integrated::LiveRuntimeProductAction>,
) -> bool {
    forall|i: int| 0 <= i < actions.len() ==> match #[trigger] actions[i] {
        integrated::LiveRuntimeProductAction::EpmContinue { .. } => true,
        _ => false,
    }
}

pub proof fn epm_internal_edges_prefix_wf(
    states: Seq<integrated::LiveRuntimeProductState>,
    events: Seq<epm::EpmEvent>,
    upto: int,
)
    requires
        states.len() == events.len() + 1,
        integrated::live_runtime_product_wf(states[0]),
        forall|i: int| 0 <= i < events.len() ==> #[trigger]
            unwind_product::live_runtime_internal_epm_unwind_step(
                states[i], states[i + 1], events[i],
            ),
        0 <= upto <= events.len(),
    ensures integrated::live_runtime_product_wf(states[upto]),
    decreases upto,
{
    if upto > 0 {
        epm_internal_edges_prefix_wf(states, events, upto - 1);
        let index = upto - 1;
        assert(unwind_product::live_runtime_internal_epm_unwind_step(
            states[index], states[index + 1], events[index],
        ));
        unwind_product::live_runtime_internal_epm_continue_preserves_wf(
            states[index], states[index + 1], events[index],
        );
    }
}

/// Lift an already checked scalar internal sequence into the shared driver
/// vocabulary. Source-specific Forward and Unwind guards stay outside this
/// lemma, so its proof depends only on adjacent EPathMap product steps.
pub proof fn epm_internal_edges_to_integrated_trace(
    states: Seq<integrated::LiveRuntimeProductState>,
    events: Seq<epm::EpmEvent>,
)
    requires
        states.len() == events.len() + 1,
        integrated::live_runtime_product_wf(states[0]),
        forall|i: int| 0 <= i < events.len() ==> #[trigger]
            unwind_product::live_runtime_internal_epm_unwind_step(
                states[i], states[i + 1], events[i],
            ),
    ensures
        integrated_source_trace(states, epm_continue_actions(events)),
        epm_actions_are_only_continue(epm_continue_actions(events)),
{
    let actions = epm_continue_actions(events);
    assert forall|i: int| 0 <= i < events.len() implies
        #[trigger] integrated::live_runtime_product_step(
            states[i], states[i + 1], actions[i],
        ) by {
        epm_internal_edges_prefix_wf(states, events, i);
        assert(unwind_product::live_runtime_internal_epm_unwind_step(
            states[i], states[i + 1], events[i],
        ));
        assert(actions[i] == integrated::LiveRuntimeProductAction::EpmContinue {
            event: events[i],
        });
    }
    assert forall|i: int| 0 <= i < actions.len() implies
        match #[trigger] actions[i] {
            integrated::LiveRuntimeProductAction::EpmContinue { .. } => true,
            _ => false,
        } by {
        assert(actions[i] == integrated::LiveRuntimeProductAction::EpmContinue {
            event: events[i],
        });
    }
}

pub proof fn epm_scalar_tick_flattens(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
)
    requires
        integrated::live_runtime_product_wf(before),
        epm_loop::scalar_loop_continues(before, after, event),
    ensures
        integrated_source_trace(
            seq![before, after], epm_continue_actions(seq![event]),
        ),
        epm_actions_are_only_continue(epm_continue_actions(seq![event])),
{
    assert(unwind_product::live_runtime_internal_epm_unwind_step(
        before, after, event,
    ));
    epm_internal_edges_to_integrated_trace(seq![before, after], seq![event]);
}

pub proof fn epm_forward_tick_flattens(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    start: usize,
    width: usize,
    source: epm_loop::AcceptedForwardSource,
    events: Seq<epm::EpmEvent>,
    states: Seq<integrated::LiveRuntimeProductState>,
)
    requires
        integrated::live_runtime_product_wf(before),
        epm_loop::forward_scalar_projection(
            before, after, start, width, source, events, states,
        ),
    ensures
        integrated_source_trace(states, epm_continue_actions(events)),
        epm_actions_are_only_continue(epm_continue_actions(events)),
        states[0] == before,
        states.last() == after,
{
    assert(forall|i: int| 0 <= i < events.len() ==> #[trigger]
        unwind_product::live_runtime_internal_epm_unwind_step(
            states[i], states[i + 1], events[i],
        ));
    epm_internal_edges_to_integrated_trace(states, events);
}

pub proof fn epm_unwind_tick_flattens(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    source: epm::LinkedEpmState,
    run: integrated::rpb3_epm_batch_cursor_composition::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: integrated::rpb3_epm_restore_run_expansion::UnwindCapacityFacts,
)
    requires
        integrated::live_runtime_product_wf(before),
        width > 0,
        unwind_product::live_runtime_unwind_source_guard(
            before, source, run, prefix, start, width, facts,
        ),
        after == unwind_product::live_runtime_unwind_virtual_state(
            before, source, run, prefix, width,
        ),
    ensures {
        let states = Seq::new(width as nat + 1, |i: int|
            unwind_product::live_runtime_unwind_virtual_state(
                before, source, run, prefix, i as usize,
            ));
        let events = Seq::new(width as nat, |i: int|
            unwind::unwind_event(start, width, i));
        &&& integrated_source_trace(states, epm_continue_actions(events))
        &&& epm_actions_are_only_continue(epm_continue_actions(events))
        &&& states[0] == before
        &&& states.last() == after
    },
{
    unwind_product::accepted_unwind_live_runtime_internal_sequence(
        before, source, run, prefix, start, width, facts,
    );
    let states = Seq::new(width as nat + 1, |i: int|
        unwind_product::live_runtime_unwind_virtual_state(
            before, source, run, prefix, i as usize,
        ));
    let events = Seq::new(width as nat, |i: int|
        unwind::unwind_event(start, width, i));
    assert forall|i: int| 0 <= i < events.len() implies
        #[trigger] unwind_product::live_runtime_internal_epm_unwind_step(
            states[i], states[i + 1], events[i],
        ) by {
        assert(states[i] == unwind_product::live_runtime_unwind_virtual_state(
            before, source, run, prefix, i as usize,
        ));
        assert(states[i + 1] == unwind_product::live_runtime_unwind_virtual_state(
            before, source, run, prefix, (i + 1) as usize,
        ));
        assert(events[i] == unwind::unwind_event(start, width, i));
    }
    assert(states[0] == before);
    assert(states.last() == after);
    epm_internal_edges_to_integrated_trace(states, events);
}

/// A batched internal tick has a finite checked scalar trace with the same
/// endpoints. No invented intermediate state is accepted: the Forward states
/// are a premise of its source projection, while Unwind states are constructed
/// by the proved virtual run.
pub proof fn epm_loop_tick_flattens_to_integrated_trace(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    tick: epm_loop::EpmLoopInternalTick,
)
    requires
        integrated::live_runtime_product_wf(before),
        epm_loop::loop_internal_tick(before, after, tick),
    ensures {
        let states = epm_tick_scalar_states(before, after, tick);
        let actions = epm_tick_scalar_actions(tick);
        &&& states.len() == actions.len() + 1
        &&& states[0] == before
        &&& states.last() == after
        &&& integrated_source_trace(states, actions)
        &&& epm_actions_are_only_continue(actions)
    },
{
    match tick {
        epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch { event } => {
            epm_scalar_tick_flattens(before, after, event);
        },
        epm_loop::EpmLoopInternalTick::AcceptedForward {
            start, width, source, expanded_events, expanded_states,
        } => {
            epm_forward_tick_flattens(
                before, after, start, width, source, expanded_events,
                expanded_states,
            );
        },
        epm_loop::EpmLoopInternalTick::AcceptedUnwind {
            source, run, prefix, start, width, facts,
        } => {
            epm_unwind_tick_flattens(
                before, after,
                source, run, prefix, start, width, facts,
            );
        },
    }
}

/// Join checked traces at one identical boundary state. The shared state is
/// kept once, so every adjacent action still reads the state that produced it.
pub proof fn integrated_source_trace_concat(
    left_states: Seq<integrated::LiveRuntimeProductState>,
    left_actions: Seq<integrated::LiveRuntimeProductAction>,
    right_states: Seq<integrated::LiveRuntimeProductState>,
    right_actions: Seq<integrated::LiveRuntimeProductAction>,
)
    requires
        integrated_source_trace(left_states, left_actions),
        integrated_source_trace(right_states, right_actions),
        left_states.last() == right_states[0],
    ensures {
        let states = left_states + right_states.drop_first();
        let actions = left_actions + right_actions;
        &&& integrated_source_trace(states, actions)
        &&& states[0] == left_states[0]
        &&& states.last() == right_states.last()
    },
{
    let states = left_states + right_states.drop_first();
    let actions = left_actions + right_actions;
    assert(states.len() == actions.len() + 1);
    assert(states[0] == left_states[0]);
    assert(states.last() == right_states.last());
    assert forall|i: int| 0 <= i < actions.len() implies
        #[trigger] integrated::live_runtime_product_step(
            states[i], states[i + 1], actions[i],
        ) by {
        if i < left_actions.len() {
            assert(states[i] == left_states[i]);
            assert(states[i + 1] == left_states[i + 1]);
            assert(actions[i] == left_actions[i]);
        } else {
            let j = i - left_actions.len();
            assert(0 <= j < right_actions.len());
            assert(actions[i] == right_actions[j]);
            if j == 0 {
                assert(states[i] == left_states.last());
                assert(states[i] == right_states[0]);
            } else {
                assert(states[i] == right_states[j]);
            }
            assert(states[i + 1] == right_states[j + 1]);
        }
    }
}

/// Appending a single checked step is the terminal case of trace
/// concatenation, stated directly so no artificial End state is inserted.
pub proof fn integrated_source_trace_append_step(
    states: Seq<integrated::LiveRuntimeProductState>,
    actions: Seq<integrated::LiveRuntimeProductAction>,
    after: integrated::LiveRuntimeProductState,
    action: integrated::LiveRuntimeProductAction,
)
    requires
        integrated_source_trace(states, actions),
        integrated::live_runtime_product_step(states.last(), after, action),
    ensures integrated_source_trace(states.push(after), actions.push(action)),
{
    let extended_states = states.push(after);
    let extended_actions = actions.push(action);
    assert(extended_states.len() == extended_actions.len() + 1);
    assert forall|i: int| 0 <= i < extended_actions.len() implies
        #[trigger] integrated::live_runtime_product_step(
            extended_states[i], extended_states[i + 1], extended_actions[i],
        ) by {
        if i < actions.len() {
            assert(extended_states[i] == states[i]);
            assert(extended_states[i + 1] == states[i + 1]);
            assert(extended_actions[i] == actions[i]);
        } else {
            assert(i == actions.len());
            assert(extended_states[i] == states.last());
            assert(extended_states[i + 1] == after);
            assert(extended_actions[i] == action);
        }
    }
}

pub open spec fn epm_flatten_prefix_states(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<epm_loop::EpmLoopInternalTick>,
    upto: nat,
) -> Seq<integrated::LiveRuntimeProductState>
    decreases upto,
{
    if states.len() != attempts.len() + 1 || upto > attempts.len()
        || states.len() == 0 {
        Seq::empty()
    } else if upto == 0 {
        seq![states[0]]
    } else {
        let previous = epm_flatten_prefix_states(
            states, attempts, (upto - 1) as nat,
        );
        let tick_states = epm_tick_scalar_states(
            states[upto as int - 1], states[upto as int],
            attempts[upto as int - 1],
        );
        if tick_states.len() > 0 {
            previous + tick_states.drop_first()
        } else {
            previous
        }
    }
}

pub open spec fn epm_flatten_prefix_actions(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<epm_loop::EpmLoopInternalTick>,
    upto: nat,
) -> Seq<integrated::LiveRuntimeProductAction>
    decreases upto,
{
    if states.len() != attempts.len() + 1 || upto > attempts.len()
        || states.len() == 0 {
        Seq::empty()
    } else if upto == 0 {
        Seq::empty()
    } else {
        epm_flatten_prefix_actions(states, attempts, (upto - 1) as nat)
            + epm_tick_scalar_actions(attempts[upto as int - 1])
    }
}

pub proof fn epm_only_continue_concat(
    left: Seq<integrated::LiveRuntimeProductAction>,
    right: Seq<integrated::LiveRuntimeProductAction>,
)
    requires
        epm_actions_are_only_continue(left),
        epm_actions_are_only_continue(right),
    ensures epm_actions_are_only_continue(left + right),
{
    assert forall|i: int| 0 <= i < left.len() + right.len() implies
        match #[trigger] (left + right)[i] {
            integrated::LiveRuntimeProductAction::EpmContinue { .. } => true,
            _ => false,
        } by {
        if i < left.len() {
            assert((left + right)[i] == left[i]);
        } else {
            let j = i - left.len();
            assert(0 <= j < right.len());
            assert((left + right)[i] == right[j]);
        }
    }
}

/// Induction flattens every compressed loop tick. The constructed trace has
/// exactly the original invocation entry and its final pre-End state.
pub proof fn epm_finite_loop_prefix_flattens(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<epm_loop::EpmLoopInternalTick>,
    upto: nat,
)
    requires
        epm_loop::finite_loop_internal_trace(states, attempts),
        upto <= attempts.len(),
    ensures {
        let flat_states = epm_flatten_prefix_states(states, attempts, upto);
        let flat_actions = epm_flatten_prefix_actions(states, attempts, upto);
        &&& integrated_source_trace(flat_states, flat_actions)
        &&& flat_states[0] == states[0]
        &&& flat_states.last() == states[upto as int]
        &&& epm_actions_are_only_continue(flat_actions)
    },
    decreases upto,
{
    if upto == 0 {
        assert(integrated::live_runtime_product_wf(states[0]));
    } else {
        let previous = (upto - 1) as nat;
        let index = upto as int - 1;
        epm_finite_loop_prefix_flattens(states, attempts, previous);
        let prefix_states = epm_flatten_prefix_states(states, attempts, previous);
        let prefix_actions = epm_flatten_prefix_actions(states, attempts, previous);
        assert(prefix_states.len() == prefix_actions.len() + 1);
        assert(prefix_states.len() > 0);
        integrated_source_trace_prefix_preserves_wf(
            prefix_states, prefix_actions, prefix_actions.len() as int,
        );
        assert(prefix_states.last() == states[index]);
        assert(0 <= index < attempts.len());
        assert(forall|i: int| 0 <= i < attempts.len() ==> #[trigger]
            epm_loop::loop_internal_tick(states[i], states[i + 1], attempts[i]));
        assert(epm_loop::loop_internal_tick(
            states[index], states[index + 1], attempts[index],
        ));
        epm_loop_tick_flattens_to_integrated_trace(
            states[index], states[index + 1], attempts[index],
        );
        let tick_states = epm_tick_scalar_states(
            states[index], states[index + 1], attempts[index],
        );
        let tick_actions = epm_tick_scalar_actions(attempts[index]);
        assert(tick_states.len() == tick_actions.len() + 1);
        assert(tick_states.len() > 0);
        assert(prefix_states.last() == tick_states[0]);
        integrated_source_trace_concat(
            prefix_states, prefix_actions, tick_states, tick_actions,
        );
        epm_only_continue_concat(prefix_actions, tick_actions);
    }
}

/// The whole accepted EPathMap invocation is one finite checked driver trace:
/// internal scalar Continue steps followed by one atomic FinishAccept. End is
/// used only by the selector proof and never appears as an additional step.
pub proof fn epm_finite_loop_complete_accepted_trace(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<epm_loop::EpmLoopInternalTick>,
    observed: CheckedOtherEpmCompleteSourceImage,
)
    requires
        epm_loop::finite_loop_to_return(
            states, attempts,
            epm_loop::EpmLoopStop::Complete { observed: observed.terminal },
        ),
        checked_other_epm_complete_source_ready(states.last(), observed),
    ensures {
        let flat_states = epm_flatten_prefix_states(
            states, attempts, attempts.len(),
        );
        let flat_actions = epm_flatten_prefix_actions(
            states, attempts, attempts.len(),
        );
        let after = checked_other_epm_complete_live_after(states.last());
        let accepted_states = flat_states.push(after);
        let accepted_actions = flat_actions.push(
            integrated::LiveRuntimeProductAction::EpmFinishAccept,
        );
        &&& integrated_source_trace(accepted_states, accepted_actions)
        &&& accepted_states[0] == states[0]
        &&& accepted_states.last() == after
        &&& accepted_actions.last()
            == integrated::LiveRuntimeProductAction::EpmFinishAccept
        &&& epm_actions_are_only_continue(flat_actions)
        &&& after.source.product.accepted_epm_sources.last()
            == integrated::product_linked_top(states.last().source.product)
    },
{
    epm_finite_loop_prefix_flattens(states, attempts, attempts.len());
    checked_other_epm_complete_composes_terminal_selector(states, attempts, observed);
    let flat_states = epm_flatten_prefix_states(
        states, attempts, attempts.len(),
    );
    let flat_actions = epm_flatten_prefix_actions(
        states, attempts, attempts.len(),
    );
    let after = checked_other_epm_complete_live_after(states.last());
    assert(flat_states.last() == states.last());
    integrated_source_trace_append_step(
        flat_states, flat_actions, after,
        integrated::LiveRuntimeProductAction::EpmFinishAccept,
    );
}

/// A modeled nested empty EPathMap has a jointly enabled EOF selector and
/// atomic accepted completion. This proves guard satisfiability, not that the
/// modeled preterminal frame is reached by scalar source events. In particular,
/// its `last_source` is the initial indexed position; production empty mode
/// advances that provenance through Value and Leave before EOF.
pub open spec fn epm_modeled_empty_terminal_observation(
) -> terminal::EpmTerminalSourceObservation {
    let source = integrated::product_epm_modeled_empty_nested_source();
    let frame = source.epm;
    let bytes = frame.snapshot.bytes.len() as usize;
    let events = frame.cursor.at;
    terminal::EpmTerminalSourceObservation {
        snapshot: frame.snapshot,
        end: frame.snapshot.end,
        region_start: epm::indexed_position_at(frame.snapshot, 0),
        last_source: frame.last_source,
        cursor_at: frame.cursor.at,
        value_table_offset: frame.cursor.value_table_offset,
        path_finish_succeeded: true,
        mode: frame.mode,
        entries_stable: frame.entries_stable,
        read_before: 0,
        prefix_read: bytes,
        value_table_read: 0,
        traversed_before: 0,
        prefix_traversed: bytes,
        value_table_traversed: 0,
        events_before: 0,
        topology_events: events,
        stats_outcome: terminal::EpmTerminalStatsOutcome::Success {
            read_after: bytes,
            traversed_after: bytes,
            events_after: events,
        },
    }
}

pub open spec fn epm_modeled_empty_complete_source_image(
) -> CheckedOtherEpmCompleteSourceImage {
    let before = integrated::live_epm_modeled_empty_nested_before();
    let stack_len = before.source.product.bridge.admission.frames.len() as usize;
    let score_len = before.source.product.bridge.transcript.entries.len() as usize;
    CheckedOtherEpmCompleteSourceImage {
        terminal: epm_modeled_empty_terminal_observation(),
        stack_len_before: stack_len,
        completed_kind: 3,
        score_len_before: score_len,
        score_len_after_finish: score_len,
        stack_len_after: 2,
        outcome: CheckedOtherCompleteOutcomeImage::AcceptedMessage,
        effect: CheckedOtherCompleteEffectImage::Stored,
        resolutions_before_accept: 0,
        resolutions_after_accept: 0,
    }
}

pub proof fn epm_modeled_empty_complete_joint_guard_satisfiable()
    ensures {
        let before = integrated::live_epm_modeled_empty_nested_before();
        let states = seq![before];
        let attempts = Seq::<epm_loop::EpmLoopInternalTick>::empty();
        let observed = epm_modeled_empty_complete_source_image();
        &&& epm_loop::finite_loop_to_return(
            states, attempts,
            epm_loop::EpmLoopStop::Complete { observed: observed.terminal },
        )
        &&& checked_other_epm_complete_source_ready(before, observed)
        &&& integrated_source_trace(
            epm_flatten_prefix_states(states, attempts, 0).push(
                checked_other_epm_complete_live_after(before),
            ),
            epm_flatten_prefix_actions(states, attempts, 0).push(
                integrated::LiveRuntimeProductAction::EpmFinishAccept,
            ),
        )
    },
{
    integrated::live_epm_modeled_empty_nested_before_wf();
    integrated::product_epm_modeled_empty_nested_source_accepts();
    let before = integrated::live_epm_modeled_empty_nested_before();
    let source = integrated::product_epm_modeled_empty_nested_source();
    let observed = epm_modeled_empty_complete_source_image();
    let terminal_observed = observed.terminal;
    assert(integrated::product_linked_top(before.source.product) == Some(source));
    assert(epm::frame_layout_wf(source.epm));
    assert(source.epm.cursor.at == epm::cursor_event_count(source.epm));
    assert(source.epm.cursor.value_table_offset as int
        == source.epm.snapshot.bytes.len());
    assert(terminal::epm_terminal_source_ready(source.epm, terminal_observed));
    assert(terminal::epm_terminal_complete_from_source(
        source.epm, terminal_observed,
    ).is_some());
    assert(end_source::epm_end_source_ready(before, terminal_observed));
    assert(checked_other_epm_complete_source_ready(before, observed));
    let states = seq![before];
    let attempts = Seq::<epm_loop::EpmLoopInternalTick>::empty();
    assert(epm_loop::finite_loop_to_return(
        states, attempts,
        epm_loop::EpmLoopStop::Complete { observed: terminal_observed },
    ));
    epm_finite_loop_complete_accepted_trace(states, attempts, observed);
}

/// The scalar cursor starts before Value(false), rather than at the modeled
/// preterminal frame used by the simpler guard-satisfiability witness above.
pub open spec fn epm_modeled_empty_scalar_source(at: usize) -> epm::LinkedEpmState {
    let terminal = integrated::product_epm_modeled_empty_nested_source();
    let anchor = epm::indexed_position_at(terminal.epm.snapshot, 5);
    let path = if at == 2 {
        terminal.epm.path
    } else {
        epm::initial_epm_path()
    };
    let frame = epm::EpmFrame {
        path,
        cursor: epm::EpmCursorImage { at, ..terminal.epm.cursor },
        last_source: if at == 0 { terminal.epm.last_source } else { anchor },
        ..terminal.epm
    };
    epm::LinkedEpmState { epm: frame, ..terminal }
}

pub open spec fn epm_modeled_empty_scalar_event(at: usize) -> epm::EpmEvent {
    if at == 0 {
        epm::EpmEvent::Value {
            has_value: false, value_body: None, source_offset: 5,
        }
    } else {
        epm::EpmEvent::Leave { source_offset: 5 }
    }
}

pub open spec fn epm_modeled_empty_scalar_live_after(
    before: integrated::LiveRuntimeProductState,
    next: epm::LinkedEpmState,
) -> integrated::LiveRuntimeProductState
    recommends before.source.product.epm_frames.len() > 0,
{
    let product = unwind_product::product_after_internal_epm_continue(
        before.source.product, next,
    );
    integrated::LiveRuntimeProductState {
        source: integrated::LiveSourceProductState {
            product,
            runtime: runtime_source::runtime_after_admission_step(
                before.source.runtime, product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

pub open spec fn epm_modeled_empty_scalar_live(at: usize)
    -> integrated::LiveRuntimeProductState {
    let template = integrated::live_epm_modeled_empty_nested_before();
    let product0 = integrated::product_epm_nested_source_witness_before(
        epm_modeled_empty_scalar_source(0),
    );
    let initial = integrated::LiveRuntimeProductState {
        source: integrated::LiveSourceProductState {
            product: product0,
            ..template.source
        },
        cursors: template.cursors,
    };
    if at == 0 {
        initial
    } else {
        let after_value = epm_modeled_empty_scalar_live_after(
            initial, epm_modeled_empty_scalar_source(1),
        );
        if at == 1 {
            after_value
        } else {
            epm_modeled_empty_scalar_live_after(
                after_value, epm_modeled_empty_scalar_source(2),
            )
        }
    }
}

pub open spec fn epm_modeled_empty_scalar_terminal_observation()
    -> terminal::EpmTerminalSourceObservation {
    let source = epm_modeled_empty_scalar_source(2);
    terminal::EpmTerminalSourceObservation {
        last_source: epm::indexed_position_at(source.epm.snapshot, 5),
        ..epm_modeled_empty_terminal_observation()
    }
}

pub open spec fn epm_modeled_empty_scalar_complete_source_image()
    -> CheckedOtherEpmCompleteSourceImage {
    CheckedOtherEpmCompleteSourceImage {
        terminal: epm_modeled_empty_scalar_terminal_observation(),
        ..epm_modeled_empty_complete_source_image()
    }
}

/// The modeled empty topology has two concrete scalar Continue transitions:
/// Value(false) at mode byte 5, then Leave at that same byte. The final state
/// remains before End, so the driver can select Complete and accept it once.
/// This witness establishes model reachability; projecting Rust executions
/// into these source images remains a separate extraction obligation.
pub proof fn epm_modeled_empty_two_scalar_complete_joint_guard_satisfiable()
    ensures {
        let states = seq![
            epm_modeled_empty_scalar_live(0),
            epm_modeled_empty_scalar_live(1),
            epm_modeled_empty_scalar_live(2),
        ];
        let attempts = seq![
            epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch {
                event: epm_modeled_empty_scalar_event(0),
            },
            epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch {
                event: epm_modeled_empty_scalar_event(1),
            },
        ];
        let observed = epm_modeled_empty_scalar_complete_source_image();
        &&& epm::linked_epm_continue(
            epm_modeled_empty_scalar_source(0),
            epm_modeled_empty_scalar_source(1),
            epm_modeled_empty_scalar_event(0),
        )
        &&& epm::linked_epm_continue(
            epm_modeled_empty_scalar_source(1),
            epm_modeled_empty_scalar_source(2),
            epm_modeled_empty_scalar_event(1),
        )
        &&& states[2].source.product.epm_frames.last().unwrap().last_source
            == epm::SourceAnchor { source: 0, offset: 5 }
        &&& epm_loop::finite_loop_to_return(
            states, attempts,
            epm_loop::EpmLoopStop::Complete { observed: observed.terminal },
        )
        &&& checked_other_epm_complete_source_ready(states[2], observed)
    },
{
    integrated::live_epm_modeled_empty_nested_before_wf();
    integrated::product_epm_modeled_empty_nested_source_accepts();
    let source0 = epm_modeled_empty_scalar_source(0);
    let source1 = epm_modeled_empty_scalar_source(1);
    let source2 = epm_modeled_empty_scalar_source(2);
    let event0 = epm_modeled_empty_scalar_event(0);
    let event1 = epm_modeled_empty_scalar_event(1);
    assert(source0.admission == source1.admission);
    assert(source1.admission == source2.admission);
    assert(source0.indexed_regions == source1.indexed_regions);
    assert(source1.indexed_regions == source2.indexed_regions);
    assert(epm::linked_indexed_sources(source0));
    assert(epm::linked_indexed_sources(source1));
    assert(epm::linked_indexed_sources(source2));
    assert(integrated::rpb3_admission_cursor::admission_state_wf(source0.admission));
    let frames = source0.admission.frames;
    assert(frames.len() == 3);
    let child = frames[2];
    assert(child.cursor == 8);
    assert(source0.epm.cursor.value_table_offset == 8);
    assert(source1.epm.cursor.value_table_offset == 8);
    assert(source2.epm.cursor.value_table_offset == 8);
    assert(epm::epm_advanced_admission_cursor(child, 8) == child);
    assert(frames.update(2, child) == frames);
    assert(source1.admission.frames == frames.update(
        2, epm::epm_advanced_admission_cursor(child, source1.epm.cursor.value_table_offset),
    ));
    assert(source2.admission.frames == frames.update(
        2, epm::epm_advanced_admission_cursor(child, source2.epm.cursor.value_table_offset),
    ));
    assert(source0.epm.path == epm::initial_epm_path());
    assert(source1.epm.path == source0.epm.path);
    assert(source2.epm.path.root_left);
    assert(source0.epm.cursor.at == 0);
    assert(source1.epm.cursor.at == 1);
    assert(source2.epm.cursor.at == 2);
    assert(source2.epm.last_source == epm::SourceAnchor { source: 0, offset: 5 });
    assert(epm::frame_layout_wf(source0.epm));
    assert(epm::frame_layout_wf(source1.epm));
    assert(epm::frame_layout_wf(source2.epm));
    assert(epm::event_step(
        source0.epm, source1.epm, event0,
        source0.admission.next_obligation_id, epm::EpmAction::Continue,
    ));
    assert(epm::linked_epm_continue(source0, source1, event0));
    assert(epm::event_step(
        source1.epm, source2.epm, event1,
        source1.admission.next_obligation_id, epm::EpmAction::Continue,
    ));
    assert(epm::linked_epm_continue(source1, source2, event1));

    let live0 = epm_modeled_empty_scalar_live(0);
    let live1 = epm_modeled_empty_scalar_live(1);
    let live2 = epm_modeled_empty_scalar_live(2);
    assert(integrated::product_linked_top(live0.source.product) == Some(source0));
    assert(integrated::product_linked_top(live1.source.product) == Some(source1));
    assert(integrated::product_linked_top(live2.source.product) == Some(source2));
    assert(integrated::live_runtime_product_wf(live0));
    assert(integrated::product_epm_continue_step(
        live0.source.product, live1.source.product, event0,
    ));
    assert(unwind_product::live_runtime_internal_epm_unwind_step(
        live0, live1, event0,
    ));
    unwind_product::live_runtime_internal_epm_continue_preserves_wf(
        live0, live1, event0,
    );
    assert(integrated::product_epm_continue_step(
        live1.source.product, live2.source.product, event1,
    ));
    assert(unwind_product::live_runtime_internal_epm_unwind_step(
        live1, live2, event1,
    ));
    unwind_product::live_runtime_internal_epm_continue_preserves_wf(
        live1, live2, event1,
    );
    let states = seq![live0, live1, live2];
    let attempts = seq![
        epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch { event: event0 },
        epm_loop::EpmLoopInternalTick::ScalarAfterDeclinedBatch { event: event1 },
    ];
    let observed = epm_modeled_empty_scalar_complete_source_image();
    assert(epm_loop::finite_loop_internal_trace(states, attempts));
    assert(terminal::epm_terminal_source_ready(source2.epm, observed.terminal));
    assert(end_source::epm_end_source_ready(live2, observed.terminal));
    assert(epm_accept::epm_source_finish_accept(source2).is_some());
    assert(checked_other_epm_complete_source_ready(live2, observed));
}

pub enum CheckedOtherEpmCompleteSourceMutation {
    StackBefore,
    Kind,
    ScoreBefore,
    ScoreAfter,
    StackAfter,
    Outcome,
    Effect,
    Resolutions,
    TerminalFinish,
    TerminalStats,
}

pub open spec fn checked_other_epm_complete_changed_usize(value: usize) -> usize {
    if value == 0 { 1 } else { 0 }
}

pub open spec fn checked_other_epm_complete_mutate_source(
    observed: CheckedOtherEpmCompleteSourceImage,
    mutation: CheckedOtherEpmCompleteSourceMutation,
) -> CheckedOtherEpmCompleteSourceImage {
    match mutation {
        CheckedOtherEpmCompleteSourceMutation::StackBefore =>
            CheckedOtherEpmCompleteSourceImage {
                stack_len_before: 0, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::Kind =>
            CheckedOtherEpmCompleteSourceImage {
                completed_kind: 0, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::ScoreBefore =>
            CheckedOtherEpmCompleteSourceImage {
                score_len_before: checked_other_epm_complete_changed_usize(
                    observed.score_len_before,
                ), ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::ScoreAfter =>
            CheckedOtherEpmCompleteSourceImage {
                score_len_after_finish: checked_other_epm_complete_changed_usize(
                    observed.score_len_after_finish,
                ), ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::StackAfter =>
            CheckedOtherEpmCompleteSourceImage {
                stack_len_after: 0, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::Outcome =>
            CheckedOtherEpmCompleteSourceImage {
                outcome: CheckedOtherCompleteOutcomeImage::Root, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::Effect =>
            CheckedOtherEpmCompleteSourceImage {
                effect: CheckedOtherCompleteEffectImage::None, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::Resolutions =>
            CheckedOtherEpmCompleteSourceImage {
                resolutions_after_accept: checked_other_epm_complete_changed_usize(
                    observed.resolutions_before_accept,
                ), ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::TerminalFinish =>
            CheckedOtherEpmCompleteSourceImage {
                terminal: terminal::EpmTerminalSourceObservation {
                    path_finish_succeeded: false, ..observed.terminal
                }, ..observed
            },
        CheckedOtherEpmCompleteSourceMutation::TerminalStats =>
            CheckedOtherEpmCompleteSourceImage {
                terminal: terminal::EpmTerminalSourceObservation {
                    stats_outcome: terminal::EpmTerminalStatsOutcome::ReadOverflow,
                    ..observed.terminal
                }, ..observed
            },
    }
}

pub proof fn checked_other_epm_complete_rejects_mutated_source(
    before: integrated::LiveRuntimeProductState,
    observed: CheckedOtherEpmCompleteSourceImage,
    mutation: CheckedOtherEpmCompleteSourceMutation,
)
    requires checked_other_epm_complete_source_ready(before, observed),
    ensures !checked_other_epm_complete_source_ready(
        before, checked_other_epm_complete_mutate_source(observed, mutation),
    ),
{
    match mutation {
        CheckedOtherEpmCompleteSourceMutation::StackBefore => {},
        CheckedOtherEpmCompleteSourceMutation::Kind => {},
        CheckedOtherEpmCompleteSourceMutation::ScoreBefore => {},
        CheckedOtherEpmCompleteSourceMutation::ScoreAfter => {},
        CheckedOtherEpmCompleteSourceMutation::StackAfter => {},
        CheckedOtherEpmCompleteSourceMutation::Outcome => {},
        CheckedOtherEpmCompleteSourceMutation::Effect => {},
        CheckedOtherEpmCompleteSourceMutation::Resolutions => {},
        CheckedOtherEpmCompleteSourceMutation::TerminalFinish => {},
        CheckedOtherEpmCompleteSourceMutation::TerminalStats => {},
    }
}

} // verus!
