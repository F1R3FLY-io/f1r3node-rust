//! A finite, successful `epathmap_step` invocation from internal work to its
//! first returned Push or Complete. `EpmAction::Continue` below is an internal
//! reducer transition; EPathMap never returns `FrameAction::Continue`.
//!
//! Each tick corresponds to one pass through the Rust loop. An accepted batch
//! commits and starts another pass. A scalar tick follows a declined batch and
//! starts another pass only for Leave, an Edge without a completed obligation,
//! or a Value without a body. The final pass has a declined batch and either
//! returns Push from Edge/Value or reaches EOF and returns Complete.
//!
//! Rust extraction remains open: production has no retained finite loop trace
//! binding each `try_epathmap_batch` result, ACT event, reducer result, and
//! final returned request/summary to these ghost images. In particular, the
//! Forward scalar projection and every ordinary scalar `event_step` below are
//! explicit premises, not claimed consequences of the Rust source checks.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_epathmap_end_source_selector as end_source, rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
    rpb3_epm_batch_cursor_composition as batch, rpb3_epm_push_live_action_refinement as push_live,
    rpb3_epm_push_source_action_refinement as push_source,
    rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore, rpb3_epm_unwind_product_lift as unwind_product,
};

verus! {

pub enum EpmLoopInternalTick {
    /// `try_epathmap_batch` returned false; `next_event` returned this event;
    /// the reducer returned no child request, so the Rust loop continued.
    ScalarAfterDeclinedBatch { event: epm::EpmEvent },
    /// `try_epathmap_batch` accepted a Forward prefix and committed it. The
    /// virtual scalar sequence is still an explicit Rust extraction premise.
    AcceptedForward {
        start: usize,
        width: usize,
        source: AcceptedForwardSource,
        expanded_events: Seq<epm::EpmEvent>,
        expanded_states: Seq<integrated::LiveRuntimeProductState>,
    },
    /// The accepted Unwind successor is constructed by the separately proved
    /// arbitrary-width virtual run expansion, rather than supplied as a poststate.
    AcceptedUnwind {
        source: epm::LinkedEpmState,
        run: batch::HistoricalRestoreRunImage,
        prefix: Seq<epm::PathCheckpoint>,
        start: usize,
        width: usize,
        facts: restore::UnwindCapacityFacts,
    },
}

pub enum AcceptedForwardSource {
    Fresh {
        pending: epm::PathObligation,
        state_id: usize,
        latest_restore_state_id: Option<usize>,
        facts: restore::ForwardCapacityFacts,
    },
    Coalesced {
        run: restore::RestoreRunImage,
        prefix: Seq<epm::PathCheckpoint>,
        current_state_id: usize,
        current_pending: epm::PathObligation,
        facts: restore::ForwardCapacityFacts,
    },
}

pub open spec fn accepted_forward_source_guard(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    source: AcceptedForwardSource,
) -> bool {
    match source {
        AcceptedForwardSource::Fresh {
            pending, state_id, latest_restore_state_id, facts,
        } => batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
        AcceptedForwardSource::Coalesced {
            run, prefix, current_state_id, current_pending, facts,
        } => batch::coalesced_forward_batch_source_guard(
            before, run, prefix, current_state_id, current_pending, start, width, facts,
        ),
    }
}

/// The scalar transition relation describes an internal loop iteration, not
/// a returned driver action. Its Rust projection must establish the ACT event
/// and the selected `path.edge`/`path.leave`/`path.value` reducer result.
pub open spec fn scalar_loop_continues(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
) -> bool {
    &&& unwind_product::live_runtime_internal_epm_unwind_step(before, after, event)
    &&& match (integrated::product_linked_top(before.source.product),
        integrated::product_linked_top(after.source.product)) {
        (Some(prior), Some(next)) => match event {
            epm::EpmEvent::Leave { .. } => true,
            epm::EpmEvent::Edge { .. } => epm::request_for_obligation(
                next.epm.path, prior.admission.next_obligation_id,
            ).is_none(),
            epm::EpmEvent::Value { value_body: None, .. } => true,
            _ => false,
        },
        _ => false,
    }
}

pub open spec fn forward_scalar_projection(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    start: usize,
    width: usize,
    source: AcceptedForwardSource,
    events: Seq<epm::EpmEvent>,
    states: Seq<integrated::LiveRuntimeProductState>,
) -> bool {
    &&& width > 0
    &&& width as int * 2 <= usize::MAX as int
    &&& events == transport::epm_push_chunk_events(
        transport::EpmPushTransportChunk::Forward { start, width },
    )
    &&& events.len() == width as int * 2
    &&& states.len() == events.len() + 1
    &&& states[0] == before
    &&& states.last() == after
    &&& match integrated::product_linked_top(before.source.product) {
        Some(prior) => accepted_forward_source_guard(
            prior.epm, start, width, source,
        ),
        None => false,
    }
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger]
        unwind_product::live_runtime_internal_epm_unwind_step(
            states[i], states[i + 1], events[i],
        )
}

pub open spec fn loop_internal_tick(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    tick: EpmLoopInternalTick,
) -> bool {
    match tick {
        EpmLoopInternalTick::ScalarAfterDeclinedBatch { event } =>
            scalar_loop_continues(before, after, event),
        EpmLoopInternalTick::AcceptedForward {
            start, width, source, expanded_events, expanded_states,
        } => forward_scalar_projection(
            before, after, start, width, source, expanded_events, expanded_states,
        ),
        EpmLoopInternalTick::AcceptedUnwind {
            source, run, prefix, start, width, facts,
        } => {
            &&& width > 0
            &&& unwind_product::live_runtime_unwind_source_guard(
                before, source, run, prefix, start, width, facts,
            )
            &&& after == unwind_product::live_runtime_unwind_virtual_state(
                before, source, run, prefix, width,
            )
        },
    }
}

/// `attempts.len()` is the finite termination measure. The terminal pass is
/// separate and is not silently counted as an internal Continue transition.
pub open spec fn finite_loop_internal_trace(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
) -> bool {
    &&& states.len() == attempts.len() + 1
    &&& integrated::live_runtime_product_wf(states[0])
    &&& forall|i: int| 0 <= i < attempts.len() ==> #[trigger]
        loop_internal_tick(states[i], states[i + 1], attempts[i])
}

pub open spec fn same_internal_score_and_source_image(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
) -> bool {
    &&& after.source.arena == before.source.arena
    &&& after.source.wire == before.source.wire
    &&& after.cursors == before.cursors
    &&& after.source.product.bridge.transcript
        == before.source.product.bridge.transcript
    &&& after.source.product.indexed_regions
        == before.source.product.indexed_regions
}

proof fn forward_scalar_prefix_preserves_image(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    start: usize,
    width: usize,
    source: AcceptedForwardSource,
    events: Seq<epm::EpmEvent>,
    states: Seq<integrated::LiveRuntimeProductState>,
    upto: int,
)
    requires
        forward_scalar_projection(before, after, start, width, source, events, states),
        0 <= upto <= events.len(),
    ensures same_internal_score_and_source_image(before, states[upto]),
    decreases upto,
{
    if upto > 0 {
        forward_scalar_prefix_preserves_image(
            before, after, start, width, source, events, states, upto - 1,
        );
        let index = upto - 1;
        assert(0 <= index < events.len());
        assert(forward_scalar_projection(
            before, after, start, width, source, events, states,
        ));
        assert(forall|i: int| 0 <= i < events.len() ==> #[trigger]
            unwind_product::live_runtime_internal_epm_unwind_step(
                states[i], states[i + 1], events[i],
            ));
        assert(unwind_product::live_runtime_internal_epm_unwind_step(
            states[index], states[index + 1], events[index],
        ));
        assert(same_internal_score_and_source_image(
            states[upto - 1], states[upto],
        ));
    }
}

pub proof fn loop_internal_tick_preserves_image(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    tick: EpmLoopInternalTick,
)
    requires loop_internal_tick(before, after, tick),
    ensures same_internal_score_and_source_image(before, after),
{
    match tick {
        EpmLoopInternalTick::ScalarAfterDeclinedBatch { event } => {
            assert(unwind_product::live_runtime_internal_epm_unwind_step(
                before, after, event,
            ));
        },
        EpmLoopInternalTick::AcceptedForward {
            start, width, source, expanded_events, expanded_states,
        } => {
            forward_scalar_prefix_preserves_image(
                before, after, start, width, source, expanded_events,
                expanded_states, expanded_events.len() as int,
            );
            assert(expanded_states.last() == after);
        },
        EpmLoopInternalTick::AcceptedUnwind {
            source, run, prefix, start, width, facts,
        } => {
            unwind_product::unwind_product_virtual_matches_linked_source(
                before.source.product, source, run, prefix, start, width,
                facts, width,
            );
            assert(after == unwind_product::live_runtime_unwind_virtual_state(
                before, source, run, prefix, width,
            ));
        },
    }
}

pub proof fn finite_loop_prefix_preserves_image(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
    upto: int,
)
    requires
        finite_loop_internal_trace(states, attempts),
        0 <= upto <= attempts.len(),
    ensures same_internal_score_and_source_image(states[0], states[upto]),
    decreases upto,
{
    if upto > 0 {
        finite_loop_prefix_preserves_image(states, attempts, upto - 1);
        let index = upto - 1;
        assert(0 <= index < attempts.len());
        assert(finite_loop_internal_trace(states, attempts));
        assert(forall|i: int| 0 <= i < attempts.len() ==> #[trigger]
            loop_internal_tick(states[i], states[i + 1], attempts[i]));
        assert(loop_internal_tick(
            states[index], states[index + 1], attempts[index],
        ));
        loop_internal_tick_preserves_image(
            states[upto - 1], states[upto], attempts[upto - 1],
        );
    }
}

pub enum EpmLoopStop {
    /// Batch declined; scalar Edge/Value produced a retained child request.
    Push { observed: push_source::EpmPushLiveObservation },
    /// Batch declined; `next_event()` returned None; `path.finish` succeeded.
    Complete { observed: terminal::EpmTerminalSourceObservation },
}

pub open spec fn loop_stop_ready(
    before: integrated::LiveRuntimeProductState,
    stop: EpmLoopStop,
) -> bool {
    match stop {
        EpmLoopStop::Push { observed } =>
            push_live::epm_push_live_ready(before, observed),
        EpmLoopStop::Complete { observed } =>
            end_source::epm_end_source_ready(before, observed),
    }
}

pub open spec fn loop_selected_action(
    before: integrated::LiveRuntimeProductState,
    stop: EpmLoopStop,
) -> Option<epm::EpmAction> {
    if !loop_stop_ready(before, stop) { None } else {
        match stop {
            EpmLoopStop::Push { observed } => {
                let prior = integrated::product_linked_top(before.source.product).unwrap();
                let request = push_source::epm_push_source_request(prior, observed.push).unwrap();
                Some(epm::EpmAction::Push(request))
            },
            EpmLoopStop::Complete { observed } => {
                let prior = integrated::product_linked_top(before.source.product).unwrap();
                let completed = terminal::epm_terminal_complete_from_source(
                    prior.epm, observed,
                ).unwrap();
                Some(completed.action)
            },
        }
    }
}

pub open spec fn finite_loop_to_return(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
    stop: EpmLoopStop,
) -> bool {
    &&& finite_loop_internal_trace(states, attempts)
    &&& loop_stop_ready(states.last(), stop)
}

/// This proves the finite trace has exactly one terminal selector, at its
/// boundary. There is no `FrameAction::Continue` result for an EPathMap frame.
pub proof fn finite_loop_selects_push_or_complete(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
    stop: EpmLoopStop,
)
    requires finite_loop_to_return(states, attempts, stop),
    ensures match (stop, loop_selected_action(states.last(), stop)) {
        (EpmLoopStop::Push { .. }, Some(epm::EpmAction::Push(_))) => true,
        (EpmLoopStop::Complete { .. }, Some(epm::EpmAction::Complete { .. })) => true,
        _ => false,
    },
{
    match stop {
        EpmLoopStop::Push { observed } => {
            assert(push_live::epm_push_live_ready(states.last(), observed));
            assert(push_source::epm_push_source_ready(states.last(), observed.push));
        },
        EpmLoopStop::Complete { observed } => {
            let prior = integrated::product_linked_top(
                states.last().source.product,
            ).unwrap();
            let completed = terminal::epm_terminal_complete_from_source(
                prior.epm, observed,
            ).unwrap();
            assert(completed.action is Complete);
        },
    }
}

/// The returned Push is constructed from the first stopping Edge/Value and
/// the source-checked child. It preserves the arena/score transcript and has
/// the exact post-push frame, wire, cursor, and storage image from the source
/// selector; no returned-action equality is an input premise.
pub proof fn finite_loop_push_dispatches_exact_image(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
    observed: push_source::EpmPushLiveObservation,
)
    requires finite_loop_to_return(
        states, attempts, EpmLoopStop::Push { observed },
    ),
    ensures match push_live::epm_push_live_after(states.last(), observed) {
        Some(after) => {
            let prior = integrated::product_linked_top(
                states.last().source.product,
            ).unwrap();
            let request = push_source::epm_push_source_request(
                prior, observed.push,
            ).unwrap();
            &&& loop_selected_action(
                states.last(), EpmLoopStop::Push { observed },
            ) == Some(epm::EpmAction::Push(request))
            &&& integrated::live_runtime_product_step(
                states.last(), after,
                integrated::LiveRuntimeProductAction::EpmPush { observed },
            )
            &&& integrated::live_runtime_product_wf(after)
            &&& after.source.product.bridge.transcript
                == states.last().source.product.bridge.transcript
            &&& after.source.product.bridge.transcript
                == states[0].source.product.bridge.transcript
            &&& after.source.arena == states.last().source.arena
            &&& after.source.arena == states[0].source.arena
            &&& after.source.product.indexed_regions.last()
                == epm::request_body(request)
            &&& after.source.wire.drop_last() == states.last().source.wire
            &&& after.source.wire.drop_last() == states[0].source.wire
            &&& after.cursors.drop_last() == states.last().cursors
            &&& after.cursors.drop_last() == states[0].cursors
        },
        None => false,
    },
{
    finite_loop_prefix_preserves_image(states, attempts, attempts.len() as int);
    push_live::epm_push_live_action_is_checked(states.last(), observed);
    push_live::epm_push_live_action_dispatches(states.last(), observed);
}

/// EOF Complete is selected only after the successful `finish` and checked
/// stats observations. This is the EPathMap frame's returned action; the
/// driver's subsequent pop and parent acceptance are separate transitions.
pub proof fn finite_loop_complete_dispatches_exact_image(
    states: Seq<integrated::LiveRuntimeProductState>,
    attempts: Seq<EpmLoopInternalTick>,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires finite_loop_to_return(
        states, attempts, EpmLoopStop::Complete { observed },
    ),
    ensures {
        let before = states.last();
        let after = end_source::epm_end_source_live_after(before, observed);
        let prior = integrated::product_linked_top(before.source.product).unwrap();
        let completed = terminal::epm_terminal_complete_from_source(
            prior.epm, observed,
        ).unwrap();
        &&& loop_selected_action(
            before, EpmLoopStop::Complete { observed },
        ) == Some(completed.action)
        &&& integrated::live_runtime_product_step(
            before, after, integrated::LiveRuntimeProductAction::EpmEnd { observed },
        )
        &&& integrated::live_runtime_product_wf(after)
        &&& after.source.product.bridge == before.source.product.bridge
        &&& after.source.product.bridge.transcript
            == states[0].source.product.bridge.transcript
        &&& after.source.product.epm_frames.last() == Some(completed.after)
        &&& after.source.arena == before.source.arena
        &&& after.source.arena == states[0].source.arena
        &&& after.source.wire == before.source.wire
        &&& after.source.wire == states[0].source.wire
        &&& after.cursors == before.cursors
        &&& after.cursors == states[0].cursors
    },
{
    finite_loop_prefix_preserves_image(states, attempts, attempts.len() as int);
    end_source::epm_end_source_selects_live_action(states.last(), observed);
}

} // verus!
