use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as terminal,
};

verus! {

pub open spec fn epm_end_source_ready(
    before: crate::LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
) -> bool {
    if !crate::live_runtime_product_wf(before) {
        false
    } else {
        match crate::product_linked_top(before.source.product) {
            None => false,
            Some(prior) => {
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
                    &&& terminal::epm_terminal_complete_from_source(
                        prior.epm, observed,
                    ).is_some()
                }
            },
        }
    }
}

pub open spec fn epm_end_source_product_after(
    before: crate::LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
) -> crate::ProductScoreState
    recommends epm_end_source_ready(before, observed),
{
    let product = before.source.product;
    let prior = crate::product_linked_top(product).unwrap();
    let complete = terminal::epm_terminal_complete_from_source(
        prior.epm, observed,
    ).unwrap();
    let provisional = crate::ProductScoreState {
        bridge: product.bridge,
        map: product.map,
        epm_frames: product.epm_frames.update(
            product.epm_frames.len() - 1, Some(complete.after),
        ),
        indexed_regions: product.indexed_regions,
        accepted_epm_sources: product.accepted_epm_sources,
    };
    crate::ProductScoreState {
        accepted_epm_sources: crate::product_epm_receipts_carried(product, provisional),
        ..provisional
    }
}

pub open spec fn epm_end_source_live_after(
    before: crate::LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
) -> crate::LiveRuntimeProductState
    recommends epm_end_source_ready(before, observed),
{
    crate::LiveRuntimeProductState {
        source: crate::LiveSourceProductState {
            product: epm_end_source_product_after(before, observed),
            runtime: before.source.runtime,
            arena: before.source.arena,
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

pub proof fn epm_end_source_selects_live_action(
    before: crate::LiveRuntimeProductState,
    observed: terminal::EpmTerminalSourceObservation,
)
    requires epm_end_source_ready(before, observed),
    ensures {
        let after = epm_end_source_live_after(before, observed);
        &&& crate::live_runtime_product_step(
            before, after, crate::LiveRuntimeProductAction::EpmEnd { observed },
        )
        &&& crate::live_runtime_product_wf(after)
        &&& after.source.product.bridge == before.source.product.bridge
        &&& after.source.product.indexed_regions
            == before.source.product.indexed_regions
        &&& after.source.product.epm_frames.last().is_some()
        &&& after.source.product.epm_frames.last().unwrap().cursor.done
    },
{
    let product = before.source.product;
    let prior = crate::product_linked_top(product).unwrap();
    let complete = terminal::epm_terminal_complete_from_source(
        prior.epm, observed,
    ).unwrap();
    let after = epm_end_source_live_after(before, observed);
    let next_product = after.source.product;
    let next = crate::product_linked_top(next_product).unwrap();
    terminal::epm_terminal_source_success_is_exact(prior.epm, observed);
    assert(next_product.epm_frames.last() == Some(complete.after));
    assert(next.epm == complete.after);
    assert(next.admission == prior.admission);
    assert(next.indexed_regions == prior.indexed_regions);
    assert(epm::linked_indexed_sources(next));
    assert(epm::event_step(
        prior.epm, next.epm, epm::EpmEvent::End,
        prior.admission.next_obligation_id, complete.action,
    ));
    assert(epm::linked_epm_complete(prior, next, complete.action));
    assert(next_product.accepted_epm_sources
        == crate::product_epm_receipts_carried(product, next_product));
    assert(crate::product_epm_complete_step(product, next_product, complete.action));
    assert(crate::live_runtime_product_epm_end_source_step(before, after, observed));
    crate::live_runtime_product_epm_end_source_preserves_wf(before, after, observed);
}

}
