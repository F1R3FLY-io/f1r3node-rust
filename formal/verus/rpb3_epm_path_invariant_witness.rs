//! Non-vacuity of the return and handoff cases of the EPathMap path invariant.
//!
//! `integrated_epm_raw_par_return_keeps_epm_paths_wf`,
//! `integrated_epm_nested_return_keeps_epm_paths_wf` and
//! `integrated_epm_handoff_keeps_epm_paths_wf` assume `integrated_epm_paths_wf`
//! of the prestate and an `integrated_step` of their action, and their live
//! counterparts assume `product_epm_paths_wf` and the live step. Each theorem
//! here exhibits a concrete well-formed integrated state that satisfies both
//! premises of both levels, so none of the three cases holds vacuously: the
//! raw-Par return witness (`epm_raw_par_witness_scored`), the nested return
//! witness whose child is not a Map (`epm_nested_dead_witness_before`), and
//! the handoff witness (`epm_handoff_witness_before`). Each theorem also states
//! the invariant of the poststate, which the integrated case theorem gives.
//!
//! The parent EPathMap frame of each return witness is the modeled empty
//! snapshot frame at cursor position 2 with a two-byte path: the obligation
//! tag and an empty payload length. Its parser's only frame is the pending
//! obligation, so the root potential is 1, and its restore holds one initial
//! checkpoint per path byte (`epm_witness_two_byte_restore`). The nested
//! witness's child is the modeled empty snapshot frame itself, and the handoff
//! witness's prestate holds no EPathMap frame.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_epm_frame_path_invariant as frame_path, rpb3_epm_live_handoff as handoff,
    rpb3_epm_obligation_return_live as obligation_return,
    rpb3_epm_obligation_return_witness as witness, rpb3_epm_parser_escape_invariant as escape,
    rpb3_epm_path_invariant_integrated as paths, rpb3_integrated_dispatcher as dispatcher,
    rpb3_path_parser_refinement as parser,
};

verus! {

/// EP of the raw-Par return witness (`epm_raw_par_witness_parent_epm`)
/// satisfies `epm_frame_path_wf`. Its parser's only frame is the RawParEscape
/// obligation at the bottom of the escape-allowing root path, which is EP's
/// pending and waited obligation; the root potential 1 and the two path bytes
/// are within cursor position 2; each restore checkpoint holds the initial
/// parser; and the gate is the parser's image
/// (`epm_raw_par_witness_resolves_root`).
pub proof fn raw_par_witness_parent_frame_path_wf()
    ensures
        frame_path::epm_frame_path_wf(witness::epm_raw_par_witness_parent_epm()),
{
    let frame = witness::epm_raw_par_witness_parent_epm();
    let path = frame.path;
    witness::epm_raw_par_witness_resolves_root();
    escape::parser_escape_wf_initial();
    assert(escape::parser_image_escape_wf(path.parser));
    assert forall|i: int| 0 <= i < path.restore.len() implies {
        &&& escape::parser_image_escape_wf((#[trigger] path.restore[i]).parser)
        &&& frame_path::epm_checkpoint_wf(path.restore[i], i)
    } by {
        assert(path.restore[i] == witness::epm_witness_initial_checkpoint(i as usize));
        assert(path.restore[i].parser == parser::initial_parser());
    }
    assert(frame_path::epm_path_escape_wf(path));
    assert(frame_path::epm_restore_wf(path));
    assert(frame_path::epm_path_length_bounds(frame));
}

/// The prestate of the raw-Par return witness satisfies
/// `product_epm_paths_wf`: its EPathMap frames are none, none, EP and none
/// (`epm_raw_par_witness_scored_shape`).
pub proof fn raw_par_witness_scored_paths_wf()
    ensures
        paths::product_epm_paths_wf(witness::epm_raw_par_witness_scored()),
{
    witness::epm_raw_par_witness_scored_live_shape();
    witness::epm_raw_par_witness_scored_shape();
    raw_par_witness_parent_frame_path_wf();
    let frames = witness::epm_raw_par_witness_scored().source.product.epm_frames;
    assert forall|i: int| 0 <= i < frames.len()
        implies paths::epm_slot_path_wf(#[trigger] frames[i]) by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
}

/// The EpmRawParReturn case is not vacuous: the prestate of the raw-Par return
/// witness is a well-formed integrated state that satisfies the path
/// invariant at the live and at the integrated level and takes the live
/// raw-Par return step and the integrated `EpmRawParReturn` step to the
/// returned state, whose invariant
/// `integrated_epm_raw_par_return_keeps_epm_paths_wf` gives.
pub proof fn raw_par_return_case_inhabited()
    ensures
        ({
            let scored = witness::epm_raw_par_witness_scored();
            let returned = witness::epm_raw_par_witness_returned();
            let before = dispatcher::IntegratedState::Active { product: scored };
            let after = dispatcher::IntegratedState::Active { product: returned };
            &&& dispatcher::integrated_wf(before)
            &&& paths::product_epm_paths_wf(scored)
            &&& obligation_return::live_runtime_epm_raw_par_return_step(scored, returned)
            &&& paths::integrated_epm_paths_wf(before)
            &&& dispatcher::integrated_step(
                before,
                after,
                dispatcher::IntegratedAction::EpmRawParReturn,
            )
            &&& paths::integrated_epm_paths_wf(after)
        }),
{
    let scored = witness::epm_raw_par_witness_scored();
    let returned = witness::epm_raw_par_witness_returned();
    let before = dispatcher::IntegratedState::Active { product: scored };
    let after = dispatcher::IntegratedState::Active { product: returned };
    raw_par_witness_scored_paths_wf();
    witness::epm_raw_par_witness_is_step();
    witness::epm_raw_par_witness_is_integrated_step();
    assert(paths::integrated_epm_paths_wf(before));
    paths::integrated_epm_raw_par_return_keeps_epm_paths_wf(
        before,
        after,
        dispatcher::IntegratedAction::EpmRawParReturn,
    );
}

/// EP of the nested return witness (`epm_nested_dead_witness_parent_epm`)
/// satisfies `epm_frame_path_wf`. Its parser's only frame is the NestedEpm1
/// obligation with an empty payload, which is not escape-capable and is EP's
/// pending and waited obligation; the root potential 1 and the two path bytes
/// are within cursor position 2; each restore checkpoint holds the initial
/// parser; and the gate is the parser's image
/// (`epm_nested_dead_witness_resolves_dead`).
pub proof fn nested_dead_witness_parent_frame_path_wf()
    ensures
        frame_path::epm_frame_path_wf(witness::epm_nested_dead_witness_parent_epm()),
{
    let frame = witness::epm_nested_dead_witness_parent_epm();
    let path = frame.path;
    witness::epm_nested_dead_witness_resolves_dead();
    escape::parser_escape_wf_initial();
    assert(escape::parser_image_escape_wf(path.parser));
    assert forall|i: int| 0 <= i < path.restore.len() implies {
        &&& escape::parser_image_escape_wf((#[trigger] path.restore[i]).parser)
        &&& frame_path::epm_checkpoint_wf(path.restore[i], i)
    } by {
        assert(path.restore[i] == witness::epm_witness_initial_checkpoint(i as usize));
        assert(path.restore[i].parser == parser::initial_parser());
    }
    assert(frame_path::epm_path_escape_wf(path));
    assert(frame_path::epm_restore_wf(path));
    assert(frame_path::epm_path_length_bounds(frame));
}

/// C of the nested return witness (`epm_nested_dead_witness_child_epm`) is the
/// modeled empty snapshot frame: the initial path with the root left, so the
/// initial parser, no restore checkpoint and no pending obligation, no wait,
/// and cursor position 2. It satisfies `epm_frame_path_wf` as the initial
/// frame does (`epm_frame_path_wf_initial`).
pub proof fn nested_dead_witness_child_frame_path_wf()
    ensures
        frame_path::epm_frame_path_wf(witness::epm_nested_dead_witness_child_epm()),
{
    let frame = witness::epm_nested_dead_witness_child_epm();
    escape::parser_escape_wf_initial();
    assert(frame.path.parser == parser::initial_parser());
    assert(frame.path.restore.len() == 0);
    assert(frame_path::parser_image_obligation_wf(frame.path.parser));
    assert(frame_path::epm_restore_wf(frame.path));
}

/// The prestate of the nested return witness satisfies
/// `product_epm_paths_wf`: its EPathMap frames are none, none, EP and C
/// (`epm_nested_dead_witness_product_shape`).
pub proof fn nested_dead_witness_before_paths_wf()
    ensures
        paths::product_epm_paths_wf(witness::epm_nested_dead_witness_before()),
{
    witness::epm_nested_dead_witness_before_shape();
    witness::epm_nested_dead_witness_product_shape();
    nested_dead_witness_parent_frame_path_wf();
    nested_dead_witness_child_frame_path_wf();
    let frames = witness::epm_nested_dead_witness_before().source.product.epm_frames;
    assert forall|i: int| 0 <= i < frames.len()
        implies paths::epm_slot_path_wf(#[trigger] frames[i]) by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
}

/// The EpmNestedReturn case is not vacuous: the prestate of the nested return
/// witness is a well-formed integrated state that satisfies the path
/// invariant at the live and at the integrated level and takes the live
/// nested return step and the integrated `EpmNestedReturn` step to the
/// returned state, whose invariant
/// `integrated_epm_nested_return_keeps_epm_paths_wf` gives.
pub proof fn nested_return_case_inhabited()
    ensures
        ({
            let prestate = witness::epm_nested_dead_witness_before();
            let returned = witness::epm_nested_dead_witness_returned();
            let before = dispatcher::IntegratedState::Active { product: prestate };
            let after = dispatcher::IntegratedState::Active { product: returned };
            &&& dispatcher::integrated_wf(before)
            &&& paths::product_epm_paths_wf(prestate)
            &&& obligation_return::live_runtime_epm_nested_return_step(prestate, returned)
            &&& paths::integrated_epm_paths_wf(before)
            &&& dispatcher::integrated_step(
                before,
                after,
                dispatcher::IntegratedAction::EpmNestedReturn,
            )
            &&& paths::integrated_epm_paths_wf(after)
        }),
{
    let prestate = witness::epm_nested_dead_witness_before();
    let returned = witness::epm_nested_dead_witness_returned();
    let before = dispatcher::IntegratedState::Active { product: prestate };
    let after = dispatcher::IntegratedState::Active { product: returned };
    nested_dead_witness_before_paths_wf();
    witness::epm_nested_dead_witness_is_step();
    witness::epm_nested_dead_witness_is_integrated_step();
    assert(paths::integrated_epm_paths_wf(before));
    paths::integrated_epm_nested_return_keeps_epm_paths_wf(
        before,
        after,
        dispatcher::IntegratedAction::EpmNestedReturn,
    );
}

/// The prestate of the handoff witness (`epm_handoff_witness_before`)
/// satisfies `product_epm_paths_wf`: it holds no EPathMap frame.
pub proof fn handoff_witness_before_paths_wf()
    ensures
        paths::product_epm_paths_wf(handoff::epm_handoff_witness_before()),
{
    let frames = handoff::epm_handoff_witness_before().source.product.epm_frames;
    assert forall|i: int| 0 <= i < frames.len()
        implies paths::epm_slot_path_wf(#[trigger] frames[i]) by {
        assert(i == 0 || i == 1 || i == 2);
    }
}

/// The EpmHandoff case is not vacuous: the prestate of the handoff witness is
/// a well-formed integrated state that satisfies the path invariant at the
/// live and at the integrated level and takes the live handoff step and the
/// integrated `EpmHandoff` step, with the modeled-empty backing and no ACT
/// plan, to the state that holds the initial EPathMap frame, whose invariant
/// `integrated_epm_handoff_keeps_epm_paths_wf` gives.
pub proof fn handoff_case_inhabited()
    ensures
        ({
            let prestate = handoff::epm_handoff_witness_before();
            let handed = handoff::epm_handoff_witness_after();
            let backing = integrated::product_epm_modeled_empty_backing();
            let before = dispatcher::IntegratedState::Active { product: prestate };
            let after = dispatcher::IntegratedState::Active { product: handed };
            &&& dispatcher::integrated_wf(before)
            &&& paths::product_epm_paths_wf(prestate)
            &&& handoff::live_runtime_epm_handoff_step(prestate, handed, backing, None, None)
            &&& paths::integrated_epm_paths_wf(before)
            &&& dispatcher::integrated_step(
                before,
                after,
                dispatcher::IntegratedAction::EpmHandoff { backing, plan: None, validated: None },
            )
            &&& paths::integrated_epm_paths_wf(after)
        }),
{
    let prestate = handoff::epm_handoff_witness_before();
    let handed = handoff::epm_handoff_witness_after();
    let backing = integrated::product_epm_modeled_empty_backing();
    let before = dispatcher::IntegratedState::Active { product: prestate };
    let after = dispatcher::IntegratedState::Active { product: handed };
    let action = dispatcher::IntegratedAction::EpmHandoff { backing, plan: None, validated: None };
    handoff_witness_before_paths_wf();
    handoff::epm_handoff_witness_is_step();
    handoff::epm_handoff_witness_is_integrated_step();
    assert(paths::integrated_epm_paths_wf(before));
    paths::integrated_epm_handoff_keeps_epm_paths_wf(before, after, action);
}

}
