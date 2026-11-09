//! A source-observed EPathMap End followed by a CanonicalPath NestedEpm1 return.
//!
//! End is an internal EPathMap frame transition. The subsequent driver pop
//! resolves the retained kind-4 obligation; a MapValue return goes *into* an
//! EPathMap and is not this terminal action. These selectors construct both
//! poststates. Their guards still require extraction of the Rust EOF, checked
//! statistics, linked child state, and reducer wait/return observations.
//! `LiveRuntimeProductAction` does not yet dispatch this combined action.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
    rpb3_live_canonical_path_source_refinement as path,
};

verus! {

/// Only the active EPathMap frame changes at End. The driver stack and score
/// remain intact until the separate checked CanonicalPath return selector.
pub open spec fn terminal_nested_end_product(
    before: crate::ProductScoreState,
    completed: terminal::EpmTerminalSourceComplete,
) -> crate::ProductScoreState {
    crate::ProductScoreState {
        bridge: before.bridge,
        map: before.map,
        epm_frames: before.epm_frames.update(
            before.epm_frames.len() - 1, Some(completed.after),
        ),
        indexed_regions: before.indexed_regions,
        accepted_epm_sources: before.accepted_epm_sources,
    }
}

pub open spec fn terminal_nested_after_end(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
) -> Option<path::LiveCanonicalPathState> {
    let product = before.base.source.product;
    if !path::live_canonical_path_wf(before)
        || product.epm_frames.len() < 2
        || product.epm_frames.last().is_none()
        || !terminal::epm_terminal_complete_from_source(
            product.epm_frames.last().unwrap(), observed,
        ).is_some()
    { None } else {
        let complete = terminal::epm_terminal_complete_from_source(
            product.epm_frames.last().unwrap(), observed,
        ).unwrap();
        let next_product = terminal_nested_end_product(product, complete);
        Some(path::LiveCanonicalPathState {
            base: crate::LiveRuntimeProductState {
                source: crate::LiveSourceProductState {
                    product: next_product,
                    runtime: before.base.source.runtime,
                    arena: before.base.source.arena,
                    wire: before.base.source.wire,
                },
                cursors: before.base.cursors,
            },
            path_frames: before.path_frames,
            outstanding: before.outstanding,
        })
    }
}

/// Build the return observation from the retained Push, not caller-supplied
/// IDs or a caller-supplied summary. Only stable Map satisfies NestedEpm1.
pub open spec fn terminal_nested_return_observation(
    ended: path::LiveCanonicalPathState,
) -> path::LiveCanonicalPathReturnObservation {
    let push = ended.outstanding[ended.outstanding.len() - 2].unwrap();
    path::LiveCanonicalPathReturnObservation {
        reducer_waiting_id: push.reducer_obligation_id,
        reducer_waiting_kind: push.kind,
        stack_return_id: push.stack_return_id,
        epm_entry_stability: seq![true],
        summary: admission::AdmissionReturnedKind::EPathMapSnapshot {
            mode: 2,
            entries_canonical_path_stable: true,
        },
    }
}

/// The linked End guard and the reducer/driver return guard are explicit
/// extraction premises. Neither is inferred from an arbitrary Rust run.
pub open spec fn terminal_nested_source_ready(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
) -> bool {
    match terminal_nested_after_end(before, observed) {
        None => false,
        Some(ended) => {
            let product = before.base.source.product;
            let active = product.epm_frames.last().unwrap();
            let complete = terminal::epm_terminal_complete_from_source(
                active, observed,
            ).unwrap();
            let parent_index = before.outstanding.len() - 2;
            &&& active.mode == epm::EpmMode::Map
            &&& active.entries_stable
            &&& observed.mode == epm::EpmMode::Map
            &&& observed.entries_stable
            &&& before.outstanding[parent_index].is_some()
            &&& before.outstanding[parent_index].unwrap().kind
                == admission::AdmissionCanonicalPathObligationKind::NestedEpm1
            &&& before.path_frames[parent_index].is_some()
            &&& crate::product_linked_top(product).is_some()
            &&& crate::product_linked_top(ended.base.source.product).is_some()
            &&& epm::linked_epm_complete(
                crate::product_linked_top(product).unwrap(),
                crate::product_linked_top(ended.base.source.product).unwrap(),
                complete.action,
            )
            &&& path::live_canonical_path_return_source_ready(
                ended, terminal_nested_return_observation(ended),
            )
        },
    }
}

/// One source-observed EPathMap End and the deterministic checked driver pop.
/// The result is the actual kind-4 parent state selected by the existing
/// indexed return operation, including the typed NestedEpm1 resolution.
pub open spec fn terminal_nested_source_step(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
) -> Option<path::LiveCanonicalPathReturnResult> {
    if !terminal_nested_source_ready(before, observed) { None } else {
        let ended = terminal_nested_after_end(before, observed).unwrap();
        path::live_canonical_path_return_from_source(
            ended, terminal_nested_return_observation(ended),
        )
    }
}

pub proof fn terminal_nested_source_enabled(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires terminal_nested_source_ready(before, observed),
    ensures terminal_nested_source_step(before, observed).is_some(),
{}

pub proof fn terminal_nested_source_corresponds(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires terminal_nested_source_ready(before, observed),
    ensures {
        let ended = terminal_nested_after_end(before, observed).unwrap();
        let returned = terminal_nested_source_step(before, observed).unwrap();
        let product = before.base.source.product;
        let complete = terminal::epm_terminal_complete_from_source(
            product.epm_frames.last().unwrap(), observed,
        ).unwrap();
        let parent_index = before.outstanding.len() - 2;
        &&& crate::product_epm_complete_step(
            product, ended.base.source.product, complete.action,
        )
        &&& epm::event_step(
            product.epm_frames.last().unwrap(), complete.after,
            epm::EpmEvent::End, 0, complete.action,
        )
        &&& complete.after.snapshot == observed.snapshot
        &&& ended.base.source.product.indexed_regions.last() == observed.snapshot
        &&& complete.after.cursor.done
        &&& returned.state.base.source.product.bridge.admission.frames.len() + 1
            == product.bridge.admission.frames.len()
        &&& returned.state.base.source.product.bridge.admission.frames[parent_index]
            .waiting_id.is_none()
        &&& returned.state.base.source.product.indexed_regions[parent_index]
            == product.indexed_regions[parent_index]
        &&& returned.state.base.source.product.bridge.admission.arena
            == product.bridge.admission.arena
        &&& returned.state.base.source.product.bridge.transcript
            == product.bridge.transcript
        &&& returned.resolved.resolved_reducer_id
            == before.outstanding[parent_index].unwrap().reducer_obligation_id
        &&& match returned.resolved.resolution {
            admission::AdmissionCanonicalPathResolution::NestedEpm1 {
                mode, entries_canonical_path_stable,
            } => mode == 2 && entries_canonical_path_stable,
            _ => false,
        }
    },
{
    let ended = terminal_nested_after_end(before, observed).unwrap();
    let product = before.base.source.product;
    let active = product.epm_frames.last().unwrap();
    let complete = terminal::epm_terminal_complete_from_source(active, observed).unwrap();
    terminal::epm_terminal_source_success_is_exact(active, observed);
    assert(ended.base.source.product == terminal_nested_end_product(product, complete));
    assert(ended.base.source.product.accepted_epm_sources
        == crate::product_epm_receipts_carried(product, ended.base.source.product)) by {
        let after = ended.base.source.product;
        assert forall|i: int| 0 <= i < after.accepted_epm_sources.len()
            implies after.accepted_epm_sources[i]
                == crate::product_epm_receipts_carried(product, after)[i] by {}
    }
    assert(crate::product_epm_complete_step(
        product, ended.base.source.product, complete.action,
    ));
    path::live_canonical_path_return_preserves_wf(
        ended, terminal_nested_return_observation(ended),
    );
}

#[cfg(rpb3_live_push)]
pub proof fn terminal_nested_end_is_live_action(
    before: path::LiveCanonicalPathState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires terminal_nested_source_ready(before, observed),
    ensures {
        let ended = terminal_nested_after_end(before, observed).unwrap();
        &&& crate::rpb3_epathmap_end_source_selector::epm_end_source_ready(
            before.base, observed,
        )
        &&& crate::rpb3_epathmap_end_source_selector::epm_end_source_live_after(
            before.base, observed,
        ) == ended.base
        &&& crate::live_runtime_product_step(
            before.base, ended.base,
            crate::LiveRuntimeProductAction::EpmEnd { observed },
        )
        &&& crate::live_runtime_product_wf(ended.base)
    },
{
    terminal_nested_source_corresponds(before, observed);
    let ended = terminal_nested_after_end(before, observed).unwrap();
    let product = before.base.source.product;
    let prior = crate::product_linked_top(product).unwrap();
    let complete = terminal::epm_terminal_complete_from_source(
        prior.epm, observed,
    ).unwrap();
    assert(epm::linked_epm_complete(
        prior, crate::product_linked_top(ended.base.source.product).unwrap(),
        complete.action,
    ));
    assert(crate::rpb3_epathmap_end_source_selector::epm_end_source_ready(
        before.base, observed,
    ));
    let selected = crate::rpb3_epathmap_end_source_selector::epm_end_source_live_after(
        before.base, observed,
    );
    assert(selected.source.product == ended.base.source.product);
    assert(selected == ended.base);
    crate::rpb3_epathmap_end_source_selector::epm_end_source_selects_live_action(
        before.base, observed,
    );
}

} // verus!

fn main() {}
