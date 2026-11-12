//! EPathMap score snapshot provenance over the integrated admission product.
//!
//! The EZipper score candidate `bridge_ezipper_score_step` reads the score node
//! of the EZipper's PathMap return through `bridge_epathmap_score_snapshot`: the
//! node must have three children, the first `Integer(13)` and the second the
//! snapshot atom. `bridge_epathmap_owner_score_snapshot_readback` shows this for
//! the transcript right after the owner step. This module shows that the node
//! keeps that shape, over the snapshot it was built from, on every state that the
//! integrated dispatcher reaches.
//!
//! The invariant `epathmap_score_nodes_shaped` holds of a bridge when every
//! admission frame satisfies two clauses under the bridge transcript:
//! - Node-shape clause, `epathmap_payload_nodes_shaped`: every EPathMap payload
//!   return, which is the PathMap return of an EZipper (program 32, slot 0, tag 1)
//!   or the EPathMap arm of an Expr (slot 0, tag 32), carries
//!   `ExprPayload { score_present: true, canonical_fixed_point: true, .. }` and a
//!   score handle naming an EPathMap score node: children
//!   `[Integer(13), BytesAtom(s), Integer(flag)]`, read back as `s`.
//! - Program-31 clause, `epathmap_owner_snapshot_provenance`: a scored EPathMap
//!   owner (program 31) waits for no child, its last return is its
//!   `EPathMapSnapshot`, and its handle names the EPathMap score node over that
//!   snapshot.
//!
//! The clauses speak about the handles the EZipper candidate reads, not about
//! every node whose first child is `Integer(13)`: the FixedPoint score of scale
//! 13 builds `Node([h])` with entry `h` equal to `Integer(13)`.
//!
//! The owner gets its handle only from the finish selector on its EOF, where the
//! selector is the owner step (`owner_finish_selects_owner_step`), so the clause
//! holds from the moment the handle exists (`owner_score_step_shapes_owner`). A
//! scored owner never changes again: every action that edits a frame's returns or
//! waiting state needs that frame waiting or unscored. When the owner returns
//! into its EZipper or Expr parent, the parent receives the owner's handle with
//! an `ExprPayload` summary (`complete_child_payload_return_shaped`); this is the
//! one place where the Program-31 clause is used, and without it the
//! MessageChildComplete case fails. Every other action leaves the payload returns
//! in place and only extends the transcript.
//!
//! At the pop, `owner_return_reads_back_owner_snapshot` states what the
//! Program-31 clause buys: the return a scored owner leaves at a payload key
//! carries the owner's handle, which reads back exactly the owner's snapshot
//! bytes under the transcript of the pop and under every extension of it.
//!
//! The EZipper score step appends four entries (the snapshot atom, the flag,
//! the constructor and the node). The node frames the snapshot atom with 22
//! bytes, as the owner's node already in the transcript does, so its length
//! always fits (`epathmap_score_node_snapshot_fits`); the one transcript bound
//! the step can meet is the entry count of `score::checked_append`, which
//! refuses an append to a transcript of `usize::MAX` entries. Completeness is
//! therefore a dichotomy on that bound.
//!
//! Results:
//! - `integrated_step_keeps_epathmap_shaped`: every action of `integrated_step`
//!   preserves the invariant, through `live_runtime_step_keeps_epathmap_shaped`
//!   for the Live actions and one lemma per remaining action family.
//! - `integrated_trace_keeps_epathmap_shaped`: every state of an integrated trace
//!   whose first state satisfies the invariant satisfies it; a first state whose
//!   frames hold no returns and no scores does (`unscored_start_is_shaped`).
//! - `ezipper_eof_score_step_dichotomy`: on a bridge that satisfies the
//!   invariant, an EZipper at EOF whose PathMap return is present and whose
//!   connective flag is readable has `bridge_ezipper_score_step` as its selected
//!   finish when the transcript has room for four more entries
//!   (`ezipper_eof_score_step_is_complete`), and every finish candidate None,
//!   so no selected finish, otherwise (`ezipper_eof_without_room_has_no_score_step`,
//!   which reads the groups 1 to 5 of the selector step by step through
//!   `finish_group_1_empty_has_no_step` to `finish_group_5_empty_has_no_step`).
//! - `reachable_ezipper_eof_has_score_step`: the dichotomy on every active state
//!   of a trace whose first state satisfies the invariant and the connective
//!   flag invariant of `rpb3_ezipper_bool_slot_readable`, whose corollary
//!   `reachable_ezipper_flag_slot_is_readable` supplies the readable flag.
//! - `ezipper_witness_reaches_complete_score`: the EZipper of
//!   `bridge_ezipper_witness_before` over its unscored owner keeps the invariant
//!   through the owner score and the owner's return, and the completeness lemma
//!   then gives the EZipper score step.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission,
    rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_epathmap_event_refinement as epm,
    rpb3_epathmap_terminal_source_observation as epm_terminal,
    rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_batch_cursor_source_extraction as act_source,
    rpb3_epm_live_handoff as handoff,
    rpb3_epm_obligation_return_live as obligation_return,
    rpb3_epm_push_live_action_refinement as push_live,
    rpb3_epm_push_source_action_refinement as push_source,
    rpb3_epm_restore_run_expansion as restore,
    rpb3_epm_source_accept_refinement as epm_source_accept,
    rpb3_epm_unwind_event_lift as unwind,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_ezipper_bool_slot_readable as ezipper_flags,
    rpb3_integrated_dispatcher as dispatcher,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_map_frame_refinement as map,
    rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as score_bridge,
    rpb3_score_transcript as score,
    rpb3_wire_cursor_source_refinement as wire_source,
    LiveRuntimeProductAction, LiveRuntimeProductState, ProductScoreState,
};

verus! {

/// Node `handle` of `transcript` is the EPathMap score node over `snapshot`, as
/// `production_pathmap_score_nodes` builds it: a node whose three children are
/// the constructor `Integer(13)`, the atom `BytesAtom(snapshot)` and an integer
/// connective flag.
pub open spec fn epathmap_score_node(
    transcript: score::ScoreArena,
    handle: usize,
    snapshot: Seq<u8>,
) -> bool {
    let entries = transcript.entries;
    &&& (handle as int) < entries.len()
    &&& match entries[handle as int].kind {
        score::ScoreNode::Node(children) => {
            &&& children.len() == 3
            &&& (children[0] as int) < entries.len()
            &&& (children[1] as int) < entries.len()
            &&& (children[2] as int) < entries.len()
            &&& entries[children[0] as int].kind == score::ScoreNode::Integer(13i64)
            &&& entries[children[1] as int].kind == score::ScoreNode::BytesAtom(snapshot)
            &&& entries[children[2] as int].kind is Integer
        },
        _ => false,
    }
}

/// `handle` reads back as a snapshot, and names the EPathMap score node over it.
pub open spec fn epathmap_score_handle_shaped(
    transcript: score::ScoreArena,
    handle: usize,
) -> bool {
    match score_bridge::bridge_epathmap_score_snapshot(transcript, handle) {
        Some(snapshot) => epathmap_score_node(transcript, handle, snapshot),
        None => false,
    }
}

/// The EPathMap owner frame: a Message frame of program 31.
pub open spec fn epathmap_owner_frame(frame: admission::AdmissionFrameState) -> bool {
    &&& frame.frame_kind == 1
    &&& frame.family == admission::AdmissionFamily::Other
    &&& frame.program_type_id == 31
}

/// Program-31 clause: a scored owner waits for no child, its last return is its
/// snapshot, and its handle names the EPathMap score node over that snapshot.
pub open spec fn epathmap_owner_snapshot_provenance(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
) -> bool {
    epathmap_owner_frame(frame) ==> match frame.score_handle {
        None => true,
        Some(handle) => {
            &&& frame.waiting_id.is_none()
            &&& frame.returns.len() > 0
            &&& frame.returns.last().kind is EPathMapSnapshot
            &&& epathmap_score_node(transcript, handle, frame.returns.last().body_bytes)
        },
    }
}

/// The returns whose score node the EZipper candidate reads: the PathMap return
/// of an EZipper (program 32) at slot 0, tag 1, and the EPathMap arm of an Expr
/// at slot 0, tag 32.
pub open spec fn epathmap_payload_return(
    frame: admission::AdmissionFrameState,
    returned: admission::AdmissionReturnedSummary,
) -> bool {
    &&& frame.frame_kind == 1
    &&& returned.slot == 0
    &&& ((frame.family == admission::AdmissionFamily::Other
            && frame.program_type_id == 32 && returned.tag == 1)
        || (frame.family == admission::AdmissionFamily::Expr && returned.tag == 32))
}

/// An EPathMap payload return carries a scored, canonical `ExprPayload` and a
/// handle naming an EPathMap score node.
pub open spec fn epathmap_payload_return_shaped(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
    returned: admission::AdmissionReturnedSummary,
) -> bool {
    epathmap_payload_return(frame, returned) ==> {
        &&& match returned.kind {
            admission::AdmissionReturnedKind::ExprPayload {
                score_present, canonical_fixed_point, ..
            } => score_present && canonical_fixed_point,
            _ => false,
        }
        &&& match returned.score_handle {
            Some(handle) => epathmap_score_handle_shaped(transcript, handle),
            None => false,
        }
    }
}

/// Node-shape clause: every EPathMap payload return of `frame` is shaped.
pub open spec fn epathmap_payload_nodes_shaped(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
) -> bool {
    forall|i: int| 0 <= i < frame.returns.len()
        ==> #[trigger] epathmap_payload_return_shaped(transcript, frame, frame.returns[i])
}

/// Both clauses for one frame.
pub open spec fn epathmap_frame_shaped(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
) -> bool {
    &&& epathmap_payload_nodes_shaped(transcript, frame)
    &&& epathmap_owner_snapshot_provenance(transcript, frame)
}

/// Both clauses for every frame of a stack.
pub open spec fn epathmap_frames_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
) -> bool {
    forall|i: int| 0 <= i < frames.len()
        ==> #[trigger] epathmap_frame_shaped(transcript, frames[i])
}

/// The invariant on one bridge: every admission frame is shaped under the
/// bridge transcript.
pub open spec fn epathmap_score_nodes_shaped(
    bridge: score_bridge::ScoreAdmissionBridge,
) -> bool {
    epathmap_frames_shaped(bridge.transcript, bridge.admission.frames)
}

/// The invariant on the live runtime product.
pub open spec fn live_epathmap_score_nodes_shaped(state: LiveRuntimeProductState) -> bool {
    epathmap_score_nodes_shaped(state.source.product.bridge)
}

/// The invariant on an integrated state. An accepted state names the product
/// that produced it, as `integrated_accepted_wf` does.
pub open spec fn integrated_epathmap_score_nodes_shaped(
    state: dispatcher::IntegratedState,
) -> bool {
    match state {
        dispatcher::IntegratedState::Active { product } =>
            live_epathmap_score_nodes_shaped(product),
        dispatcher::IntegratedState::Accepted { accepted } => match accepted {
            dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. } =>
                live_epathmap_score_nodes_shaped(post_root_score),
            dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. } =>
                live_epathmap_score_nodes_shaped(pre_complete),
            dispatcher::IntegratedAccepted::CanonicalPathRoot { pre_complete, .. } =>
                live_epathmap_score_nodes_shaped(pre_complete),
        },
    }
}

/// `after` appends entries to `before` and keeps every existing entry.
pub open spec fn epathmap_transcript_extends(
    before: score::ScoreArena,
    after: score::ScoreArena,
) -> bool {
    &&& before.entries.len() <= after.entries.len()
    &&& after.entries.take(before.entries.len() as int) == before.entries
}

/// `after` keeps every field of `before` that the two clauses read, and stops
/// waiting if `before` was a scored frame that did not wait.
pub open spec fn epathmap_frame_view_kept(
    before: admission::AdmissionFrameState,
    after: admission::AdmissionFrameState,
) -> bool {
    &&& after.frame_kind == before.frame_kind
    &&& after.family == before.family
    &&& after.program_type_id == before.program_type_id
    &&& after.returns == before.returns
    &&& after.score_handle == before.score_handle
    &&& (after.waiting_id.is_none() || before.waiting_id.is_some()
        || before.score_handle.is_none())
}

/// A frame that no clause constrains: it holds no EPathMap payload returns, and
/// it is not a scored owner.
pub open spec fn epathmap_frame_inert(frame: admission::AdmissionFrameState) -> bool {
    &&& (frame.frame_kind != 1
        || (frame.family != admission::AdmissionFamily::Expr
            && (frame.family != admission::AdmissionFamily::Other
                || frame.program_type_id != 32)))
    &&& (!epathmap_owner_frame(frame) || frame.score_handle.is_none())
}

/// An EZipper at EOF whose PathMap return is present: the guards of
/// `bridge_ezipper_score_step` that read neither the transcript nor the
/// connective flag. The flag, which `bridge_pathmap_score_append` reads as
/// `bridge_bool_slot(frame, 4, 5)`, is readable on every reachable EZipper by
/// `rpb3_ezipper_bool_slot_readable`.
pub open spec fn epathmap_ezipper_eof_present(frame: admission::AdmissionFrameState) -> bool {
    &&& frame.family == admission::AdmissionFamily::Other
    &&& frame.frame_kind == 1
    &&& frame.program_type_id == 32
    &&& frame.slot_count == 6
    &&& frame.waiting_id.is_none()
    &&& frame.score_handle.is_none()
    &&& frame.cursor == frame.region_end
    &&& frame.seen.contains(0)
    &&& frame.returns.len() == 1
    &&& frame.returns[0].slot == 0
    &&& frame.returns[0].tag == 1
}

/// The transcript has room for the four entries of a PathMap score append:
/// `score::checked_append` refuses an append to a transcript that already
/// holds `usize::MAX` entries. Production's `ScoreStore::append_entry` refuses
/// an entry with `ScoreStoreError::LengthOverflow` when `try_reserve(1)`
/// fails, which happens no later than this bound. The other bound of the
/// append, the length of the appended node, always holds on a shaped bridge
/// (`epathmap_score_node_snapshot_fits`).
pub open spec fn epathmap_score_append_room(transcript: score::ScoreArena) -> bool {
    transcript.entries.len() + 4 <= usize::MAX
}

/// Every step whose result finish group 1 of `bridge_score_finish_selected`
/// folds is None: the raw Par and the cost frames.
pub open spec fn epathmap_group_1_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_raw_par_step(before).is_none()
    &&& score_bridge::bridge_cost_atom_absent_step(before).is_none()
    &&& score_bridge::bridge_cost_atom_bytes_step(before).is_none()
    &&& score_bridge::bridge_cost_bound_step(before).is_none()
    &&& score_bridge::bridge_cost_atom_bound_forward_step(before).is_none()
    &&& score_bridge::bridge_cost_signature_step(before).is_none()
    &&& score_bridge::bridge_cost_region_step(before).is_none()
    &&& score_bridge::bridge_cost_token_stack_step(before).is_none()
}

/// Every step whose result finish group 2 folds is None: Bundle, If, Send,
/// Match, the absent oneofs, receive binds and match cases.
pub open spec fn epathmap_group_2_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_bundle_step(before).is_none()
    &&& score_bridge::bridge_if_step(before).is_none()
    &&& score_bridge::bridge_send_step(before).is_none()
    &&& score_bridge::bridge_match_step(before).is_none()
    &&& score_bridge::bridge_absent_specialized_constructor_step(before).is_none()
    &&& score_bridge::bridge_absent_connective_step(before).is_none()
    &&& score_bridge::bridge_receive_bind_step(before).is_none()
    &&& score_bridge::bridge_match_case_step(before).is_none()
}

/// Every step whose result finish group 3 folds is None: the scalar Expr and
/// Connective arms, BigRational, FixedPoint, the system auth token, and New
/// without URIs and with at most one injection.
pub open spec fn epathmap_group_3_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_bool_oneof_step(before).is_none()
    &&& score_bridge::bridge_expr_atom_step(before).is_none()
    &&& score_bridge::bridge_expr_numeric_step(before).is_none()
    &&& score_bridge::bridge_expr_big_rational_step(before).is_none()
    &&& score_bridge::bridge_expr_fixed_point_step(before).is_none()
    &&& score_bridge::bridge_sys_auth_token_step(before).is_none()
    &&& score_bridge::bridge_new_empty_collections_step(before).is_none()
    &&& score_bridge::bridge_new_one_injection_step(before).is_none()
}

/// Every step whose result finish group 4 folds is None: the other New
/// shapes, Expr payload forwarding, structural Expr messages, EVar, ordered
/// collections, EMethod and EMap.
pub open spec fn epathmap_group_4_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_new_one_absent_injection_step(before).is_none()
    &&& score_bridge::bridge_new_one_uri_step(before).is_none()
    &&& score_bridge::bridge_new_collections_step(before).is_none()
    &&& score_bridge::bridge_expr_payload_forward_step(before).is_none()
    &&& score_bridge::bridge_expr_structural_step(before).is_none()
    &&& score_bridge::bridge_evar_step(before).is_none()
    &&& score_bridge::bridge_ordered_sequence_step(before).is_none()
    &&& score_bridge::bridge_emethod_step(before).is_none()
    &&& score_bridge::bridge_emap_finish_step(before).is_none()
}

/// Every step whose result finish group 5 folds is None: the Connective
/// message arms, unforgeable bytes, complete Receive, and the nonsemantic
/// finish.
pub open spec fn epathmap_group_5_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_connective_not_step(before).is_none()
    &&& score_bridge::bridge_connective_varref_step(before).is_none()
    &&& score_bridge::bridge_connective_collection_step(before).is_none()
    &&& score_bridge::bridge_unforgeable_bytes_step(before).is_none()
    &&& score_bridge::bridge_receive_present_condition_step(before).is_none()
    &&& score_bridge::bridge_receive_absent_condition_step(before).is_none()
    &&& score_bridge::bridge_nonsemantic_step(before).is_none()
}

/// Every step whose result finish group 6 folds is None: Var, the EPathMap
/// owner and the EZipper.
pub open spec fn epathmap_group_6_steps_none(before: score_bridge::ScoreAdmissionBridge) -> bool {
    &&& score_bridge::bridge_var_score_step(before).is_none()
    &&& score_bridge::bridge_epathmap_owner_score_step(before).is_none()
    &&& score_bridge::bridge_ezipper_score_step(before).is_none()
}

/// Every finish candidate of `before` is None: each of the 43 score steps whose
/// results the six groups of `bridge_score_finish_selected` fold.
pub open spec fn epathmap_finish_candidates_none(
    before: score_bridge::ScoreAdmissionBridge,
) -> bool {
    &&& epathmap_group_1_steps_none(before)
    &&& epathmap_group_2_steps_none(before)
    &&& epathmap_group_3_steps_none(before)
    &&& epathmap_group_4_steps_none(before)
    &&& epathmap_group_5_steps_none(before)
    &&& epathmap_group_6_steps_none(before)
}

/// The completeness dichotomy of an EZipper at EOF with its PathMap present:
/// with room for four more transcript entries, the EZipper score step is Some
/// and the finish selector picks it; without room, every finish candidate, the
/// EZipper step among them, is None and the finish selector picks nothing.
pub open spec fn epathmap_ezipper_score_dichotomy(
    before: score_bridge::ScoreAdmissionBridge,
) -> bool {
    if epathmap_score_append_room(before.transcript) {
        &&& score_bridge::bridge_ezipper_score_step(before).is_some()
        &&& score_bridge::bridge_score_finish_selected(before)
            == score_bridge::bridge_ezipper_score_step(before)
    } else {
        &&& epathmap_finish_candidates_none(before)
        &&& score_bridge::bridge_score_finish_selected(before).is_none()
    }
}

/// An EPathMap score node reads back as its snapshot.
pub proof fn epathmap_score_node_reads_back(
    transcript: score::ScoreArena,
    handle: usize,
    snapshot: Seq<u8>,
)
    requires epathmap_score_node(transcript, handle, snapshot),
    ensures
        score_bridge::bridge_epathmap_score_snapshot(transcript, handle) == Some(snapshot),
        epathmap_score_handle_shaped(transcript, handle),
{
}

/// An EPathMap score node survives every transcript extension.
pub proof fn epathmap_score_node_extends(
    before: score::ScoreArena,
    after: score::ScoreArena,
    handle: usize,
    snapshot: Seq<u8>,
)
    requires
        epathmap_score_node(before, handle, snapshot),
        epathmap_transcript_extends(before, after),
    ensures epathmap_score_node(after, handle, snapshot),
{
    let count = before.entries.len() as int;
    assert(after.entries.take(count)[handle as int] == after.entries[handle as int]);
    match before.entries[handle as int].kind {
        score::ScoreNode::Node(children) => {
            assert(after.entries.take(count)[children[0] as int]
                == after.entries[children[0] as int]);
            assert(after.entries.take(count)[children[1] as int]
                == after.entries[children[1] as int]);
            assert(after.entries.take(count)[children[2] as int]
                == after.entries[children[2] as int]);
        },
        _ => {},
    }
}

/// A shaped handle stays shaped under every transcript extension.
pub proof fn epathmap_handle_shaped_extends(
    before: score::ScoreArena,
    after: score::ScoreArena,
    handle: usize,
)
    requires
        epathmap_score_handle_shaped(before, handle),
        epathmap_transcript_extends(before, after),
    ensures epathmap_score_handle_shaped(after, handle),
{
    let snapshot = score_bridge::bridge_epathmap_score_snapshot(before, handle).unwrap();
    epathmap_score_node_extends(before, after, handle, snapshot);
    epathmap_score_node_reads_back(after, handle, snapshot);
}

/// The snapshot atom of an EPathMap score node fits a `usize` length together
/// with the 22 bytes that frame it in a node of three integer and atom
/// children. The node itself is such a frame: on a well-formed transcript its
/// encoded length, a `usize`, is its transcript length, the two node bytes plus
/// the transcripts of `Integer(13)` (10 bytes), of the atom, and of the integer
/// flag (10 bytes).
pub proof fn epathmap_score_node_snapshot_fits(
    transcript: score::ScoreArena,
    handle: usize,
    snapshot: Seq<u8>,
)
    requires
        score::arena_well_formed(transcript),
        epathmap_score_node(transcript, handle, snapshot),
    ensures score::bytes_transcript(snapshot).len() + 22 <= usize::MAX,
{
    let entries = transcript.entries;
    let node = handle as int;
    let prefix = entries.take(node);
    assert(score::entry_well_formed(entries, node));
    match entries[node].kind {
        score::ScoreNode::Node(children) => {
            let constructor = children[0] as int;
            let atom = children[1] as int;
            let flag = children[2] as int;
            assert(score::child_handles_valid(prefix, children));
            assert(constructor < node && atom < node && flag < node) by {
                assert((children[0] as int) < prefix.len());
                assert((children[1] as int) < prefix.len());
                assert((children[2] as int) < prefix.len());
            }
            assert(score::entry_well_formed(entries, constructor));
            assert(score::entry_well_formed(entries, atom));
            assert(score::entry_well_formed(entries, flag));
            score::integer_transcript_exact(13i64);
            if let score::ScoreNode::Integer(value) = entries[flag].kind {
                score::integer_transcript_exact(value);
            }
            assert forall|i: int| 0 <= i < prefix.len() implies
                prefix[i].encoded_len as int == prefix[i].transcript.len() by {
                assert(prefix[i] == entries[i]);
                assert(score::entry_well_formed(entries, i));
            }
            score::node_transcript_exact(prefix, children);
            let rest = children.drop_first();
            let last = rest.drop_first();
            let none = last.drop_first();
            assert(rest[0] == children[1] && last[0] == children[2]);
            assert(none.len() == 0);
            assert(score::children_encoded_len(prefix, none) == 0);
            assert(prefix[constructor] == entries[constructor]);
            assert(prefix[atom] == entries[atom]);
            assert(prefix[flag] == entries[flag]);
            assert(score::children_encoded_len(prefix, last) == 10);
            assert(score::children_encoded_len(prefix, rest)
                == score::bytes_transcript(snapshot).len() + 10);
            assert(score::children_encoded_len(prefix, children)
                == score::bytes_transcript(snapshot).len() + 20);
            assert(entries[node].encoded_len as int
                == score::bytes_transcript(snapshot).len() + 22);
        },
        _ => {},
    }
}

/// A shaped frame stays shaped under every transcript extension.
pub proof fn epathmap_frame_shaped_extends(
    before: score::ScoreArena,
    after: score::ScoreArena,
    frame: admission::AdmissionFrameState,
)
    requires
        epathmap_frame_shaped(before, frame),
        epathmap_transcript_extends(before, after),
    ensures epathmap_frame_shaped(after, frame),
{
    assert forall|i: int| 0 <= i < frame.returns.len() implies
        #[trigger] epathmap_payload_return_shaped(after, frame, frame.returns[i]) by {
        assert(epathmap_payload_return_shaped(before, frame, frame.returns[i]));
        if epathmap_payload_return(frame, frame.returns[i]) {
            epathmap_handle_shaped_extends(
                before, after, frame.returns[i].score_handle.unwrap(),
            );
        }
    }
    if epathmap_owner_frame(frame) {
        if let Some(handle) = frame.score_handle {
            epathmap_score_node_extends(
                before, after, handle, frame.returns.last().body_bytes,
            );
        }
    }
}

/// A frame that keeps the view of a shaped frame is shaped.
pub proof fn epathmap_view_kept_frame_shaped(
    transcript: score::ScoreArena,
    before: admission::AdmissionFrameState,
    after: admission::AdmissionFrameState,
)
    requires
        epathmap_frame_shaped(transcript, before),
        epathmap_frame_view_kept(before, after),
    ensures epathmap_frame_shaped(transcript, after),
{
    assert forall|i: int| 0 <= i < after.returns.len() implies
        #[trigger] epathmap_payload_return_shaped(transcript, after, after.returns[i]) by {
        assert(epathmap_payload_return_shaped(transcript, before, before.returns[i]));
    }
}

/// An inert frame is shaped.
pub proof fn epathmap_inert_frame_shaped(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
)
    requires epathmap_frame_inert(frame),
    ensures epathmap_frame_shaped(transcript, frame),
{
    assert forall|i: int| 0 <= i < frame.returns.len() implies
        #[trigger] epathmap_payload_return_shaped(transcript, frame, frame.returns[i]) by {}
}

/// A frame with no returns and no score is shaped.
pub proof fn epathmap_fresh_frame_shaped(
    transcript: score::ScoreArena,
    frame: admission::AdmissionFrameState,
)
    requires
        frame.returns.len() == 0,
        frame.score_handle.is_none(),
    ensures epathmap_frame_shaped(transcript, frame),
{
}

/// A stack whose prefix below `limit` is kept from a shaped stack, under an
/// extended transcript, and whose other frames are shaped, is shaped.
pub proof fn epathmap_frames_assemble(
    before_transcript: score::ScoreArena,
    after_transcript: score::ScoreArena,
    before_frames: Seq<admission::AdmissionFrameState>,
    after_frames: Seq<admission::AdmissionFrameState>,
    limit: int,
)
    requires
        epathmap_frames_shaped(before_transcript, before_frames),
        epathmap_transcript_extends(before_transcript, after_transcript),
        0 <= limit <= before_frames.len(),
        limit <= after_frames.len(),
        forall|i: int| 0 <= i < limit ==> after_frames[i] == before_frames[i],
        forall|i: int| limit <= i < after_frames.len()
            ==> #[trigger] epathmap_frame_shaped(after_transcript, after_frames[i]),
    ensures epathmap_frames_shaped(after_transcript, after_frames),
{
    assert forall|i: int| 0 <= i < after_frames.len() implies
        #[trigger] epathmap_frame_shaped(after_transcript, after_frames[i]) by {
        if i < limit {
            assert(after_frames[i] == before_frames[i]);
            assert(epathmap_frame_shaped(before_transcript, before_frames[i]));
            epathmap_frame_shaped_extends(before_transcript, after_transcript, before_frames[i]);
        }
    }
}

/// Replacing the top frame by a shaped frame keeps the invariant.
pub proof fn epathmap_top_replaced_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
    top: admission::AdmissionFrameState,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 0,
        epathmap_frame_shaped(transcript, top),
    ensures epathmap_frames_shaped(transcript, frames.update(frames.len() - 1, top)),
{
    let after = frames.update(frames.len() - 1, top);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] epathmap_frame_shaped(transcript, after[i]) by {
        if i < frames.len() - 1 {
            assert(after[i] == frames[i]);
        }
    }
}

/// Replacing the top frame and pushing a child, both shaped, keeps the
/// invariant.
pub proof fn epathmap_child_pushed_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
    top: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 0,
        epathmap_frame_shaped(transcript, top),
        epathmap_frame_shaped(transcript, child),
    ensures
        epathmap_frames_shaped(transcript, frames.update(frames.len() - 1, top).push(child)),
{
    let after = frames.update(frames.len() - 1, top).push(child);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] epathmap_frame_shaped(transcript, after[i]) by {
        if i < frames.len() - 1 {
            assert(after[i] == frames[i]);
        } else if i == frames.len() - 1 {
            assert(after[i] == top);
        } else {
            assert(after[i] == child);
        }
    }
}

/// Popping the top frame and replacing its parent by a shaped frame keeps the
/// invariant.
pub proof fn epathmap_parent_replaced_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
    parent: admission::AdmissionFrameState,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 1,
        epathmap_frame_shaped(transcript, parent),
    ensures
        epathmap_frames_shaped(
            transcript,
            frames.subrange(0, frames.len() - 1).update(frames.len() - 2, parent),
        ),
{
    let after = frames.subrange(0, frames.len() - 1).update(frames.len() - 2, parent);
    assert forall|i: int| 0 <= i < after.len() implies
        #[trigger] epathmap_frame_shaped(transcript, after[i]) by {
        if i < frames.len() - 2 {
            assert(after[i] == frames[i]);
        } else {
            assert(after[i] == parent);
        }
    }
}

/// The EPathMap byte cursor advance of the top frame keeps the invariant.
pub proof fn epathmap_cursor_advanced_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
    consumed: usize,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 0,
    ensures
        epathmap_frames_shaped(
            transcript,
            frames.update(
                frames.len() - 1, epm::epm_advanced_admission_cursor(frames.last(), consumed),
            ),
        ),
{
    let top = frames.last();
    let advanced = epm::epm_advanced_admission_cursor(top, consumed);
    assert(epathmap_frame_shaped(transcript, frames[frames.len() - 1]));
    epathmap_view_kept_frame_shaped(transcript, top, advanced);
    epathmap_top_replaced_shaped(transcript, frames, advanced);
}

/// Every successful `admission_continue_step` keeps the invariant: it rewrites
/// only the top frame's cursor and slot bookkeeping.
pub proof fn admission_continue_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        forall|next_cursor: usize, slot: usize, repeated: bool, field_tag: u32|
            match #[trigger] admission::admission_continue_step(
                state, next_cursor, slot, repeated, field_tag,
            ) {
                Some(next) => epathmap_frames_shaped(transcript, next.frames),
                None => true,
            },
{
    assert forall|next_cursor: usize, slot: usize, repeated: bool, field_tag: u32|
        match #[trigger] admission::admission_continue_step(
            state, next_cursor, slot, repeated, field_tag,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_continue_step(
            state, next_cursor, slot, repeated, field_tag,
        ) {
            let top = state.frames.last();
            let updated = next.frames[state.frames.len() - 1];
            assert(next.frames == state.frames.update(state.frames.len() - 1, updated));
            assert(epathmap_frame_shaped(transcript, state.frames[state.frames.len() - 1]));
            epathmap_view_kept_frame_shaped(transcript, top, updated);
            epathmap_top_replaced_shaped(transcript, state.frames, updated);
        }
    }
}

/// Every successful scalar record keeps the invariant: a continue step, then a
/// scalar occurrence appended to the top frame.
pub proof fn scalar_record_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        forall|field: admission::IndexedCanonicalFieldModel, slot: usize, repeated: bool,
            scalar_kind: u8, physical_source: usize, physical_start: int|
            match #[trigger] admission::admission_record_scalar_occurrence_at_position_step(
                state, field, slot, repeated, scalar_kind, physical_source, physical_start,
            ) {
                Some(next) => epathmap_frames_shaped(transcript, next.frames),
                None => true,
            },
{
    admission_continue_keeps_epathmap_shaped(transcript, state);
    assert forall|field: admission::IndexedCanonicalFieldModel, slot: usize, repeated: bool,
        scalar_kind: u8, physical_source: usize, physical_start: int|
        match #[trigger] admission::admission_record_scalar_occurrence_at_position_step(
            state, field, slot, repeated, scalar_kind, physical_source, physical_start,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_record_scalar_occurrence_at_position_step(
            state, field, slot, repeated, scalar_kind, physical_source, physical_start,
        ) {
            let continued = admission::admission_continue_step(
                state, field.encoded_end, slot, repeated, field.number,
            ).unwrap();
            let index = continued.frames.len() - 1;
            let top = continued.frames[index];
            let updated = next.frames[index];
            assert(next.frames == continued.frames.update(index, updated));
            assert(epathmap_frames_shaped(transcript, continued.frames));
            assert(epathmap_frame_shaped(transcript, continued.frames[index]));
            epathmap_view_kept_frame_shaped(transcript, top, updated);
            epathmap_top_replaced_shaped(transcript, continued.frames, updated);
        }
    }
}

/// Every successful `admission_push_step` keeps the invariant: the unscored
/// parent starts waiting, and the pushed child has no returns and no score.
pub proof fn admission_push_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        forall|parent_next: usize, child: admission::AdmissionFrameState, slot: usize,
            repeated: bool, field_tag: u32, frame_bytes: usize|
            match #[trigger] admission::admission_push_step(
                state, parent_next, child, slot, repeated, field_tag, frame_bytes,
            ) {
                Some(next) => epathmap_frames_shaped(transcript, next.frames),
                None => true,
            },
{
    assert forall|parent_next: usize, child: admission::AdmissionFrameState, slot: usize,
        repeated: bool, field_tag: u32, frame_bytes: usize|
        match #[trigger] admission::admission_push_step(
            state, parent_next, child, slot, repeated, field_tag, frame_bytes,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_push_step(
            state, parent_next, child, slot, repeated, field_tag, frame_bytes,
        ) {
            let index = state.frames.len() - 1;
            let parent = state.frames[index];
            let waiting = next.frames[index];
            let pushed = next.frames[index + 1];
            assert(next.frames == state.frames.update(index, waiting).push(pushed));
            assert(epathmap_frame_shaped(transcript, state.frames[index]));
            assert(parent == state.frames.last());
            epathmap_view_kept_frame_shaped(transcript, parent, waiting);
            epathmap_fresh_frame_shaped(transcript, pushed);
            epathmap_child_pushed_shaped(transcript, state.frames, waiting, pushed);
        }
    }
}

/// The return that a successful child Complete pushes onto its parent is
/// shaped when the child satisfies the Program-31 clause. At the two EPathMap
/// payload keys, the arm agreement forces the child to be a scored program-31
/// owner and the summary to be a scored, canonical `ExprPayload`; the pushed
/// return carries the owner's handle, which the Program-31 clause shapes.
pub proof fn complete_child_payload_return_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
    returned_id: u64,
    returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires
        admission::admission_complete_child_step(
            state, returned_id, returned_kind, summary,
        ).is_some(),
        epathmap_owner_snapshot_provenance(transcript, state.frames.last()),
    ensures
        ({
            let next = admission::admission_complete_child_step(
                state, returned_id, returned_kind, summary,
            ).unwrap();
            let parent = next.frames[state.frames.len() - 2];
            epathmap_payload_return_shaped(transcript, parent, parent.returns.last())
        }),
{
    let count = state.frames.len();
    let child = state.frames[count - 1];
    let parent = state.frames[count - 2];
    let next = admission::admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ).unwrap();
    let accepted = next.frames[count - 2];
    let returned = accepted.returns.last();
    assert(child == state.frames.last());
    assert(returned.slot == parent.waiting_slot.unwrap());
    assert(returned.tag == parent.waiting_tag.unwrap());
    assert(returned.score_handle == child.score_handle);
    assert(returned.kind == summary);
    if epathmap_payload_return(accepted, returned) {
        assert(admission::admission_arm_child_summary_agrees(
            parent, child, summary, state.arena,
        ));
        assert(admission::admission_child_summary_agrees(child, summary, state.arena));
        if parent.family == admission::AdmissionFamily::Other {
            assert(admission::admission_ezipper_pathmap_payload(parent, child) == Some(summary));
        } else {
            assert(admission::admission_expr_structural_payload_with_arena(
                parent, child, state.arena,
            ) == Some(summary));
            assert(admission::admission_expr_structural_payload(parent, child) == Some(summary));
        }
        assert(epathmap_owner_frame(child));
        assert(child.score_handle.is_some());
        let handle = child.score_handle.unwrap();
        epathmap_score_node_reads_back(transcript, handle, child.returns.last().body_bytes);
    }
}

/// Every successful child Complete keeps the invariant. The parent waited for
/// the child, so a parent that is an owner is unscored and stays so; the
/// parent's old returns keep their shape, and the new return is shaped by
/// `complete_child_payload_return_shaped`.
pub proof fn complete_child_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
    returned_id: u64,
    returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        match admission::admission_complete_child_step(
            state, returned_id, returned_kind, summary,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        },
{
    if let Some(next) = admission::admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ) {
        let count = state.frames.len();
        let child = state.frames[count - 1];
        let parent = state.frames[count - 2];
        let accepted = next.frames[count - 2];
        assert(child == state.frames.last());
        assert(epathmap_frame_shaped(transcript, state.frames[count - 1]));
        assert(epathmap_frame_shaped(transcript, state.frames[count - 2]));
        complete_child_payload_return_shaped(
            transcript, state, returned_id, returned_kind, summary,
        );
        assert(next.frames == state.frames.subrange(0, count - 1).update(count - 2, accepted));
        assert(accepted.returns == parent.returns.push(accepted.returns.last()));
        assert forall|i: int| 0 <= i < accepted.returns.len() implies
            #[trigger] epathmap_payload_return_shaped(transcript, accepted, accepted.returns[i]) by {
            if i < parent.returns.len() {
                assert(accepted.returns[i] == parent.returns[i]);
                assert(epathmap_payload_return_shaped(transcript, parent, parent.returns[i]));
            } else {
                assert(accepted.returns[i] == accepted.returns.last());
            }
        }
        assert(epathmap_frame_shaped(transcript, accepted));
        epathmap_parent_replaced_shaped(transcript, state.frames, accepted);
    }
}

/// At the pop: when a child Complete pushes a return at an EPathMap payload
/// key, the child is a scored program-31 owner and the return carries the
/// owner's handle. The Program-31 clause of the child names the EPathMap score
/// node over the owner's snapshot, the bytes the owner score step scored, so
/// the handle reads back exactly those bytes under the transcript of the pop
/// and, since the transcript only grows, under every transcript that extends
/// it.
pub proof fn owner_return_reads_back_owner_snapshot(
    transcript: score::ScoreArena,
    later: score::ScoreArena,
    state: admission::AdmissionState,
    returned_id: u64,
    returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires
        admission::admission_complete_child_step(
            state, returned_id, returned_kind, summary,
        ).is_some(),
        epathmap_owner_snapshot_provenance(transcript, state.frames.last()),
        epathmap_transcript_extends(transcript, later),
    ensures
        ({
            let next = admission::admission_complete_child_step(
                state, returned_id, returned_kind, summary,
            ).unwrap();
            let owner = state.frames.last();
            let parent = next.frames[state.frames.len() - 2];
            let returned = parent.returns.last();
            epathmap_payload_return(parent, returned) ==> {
                &&& epathmap_owner_frame(owner)
                &&& owner.score_handle.is_some()
                &&& returned.score_handle == owner.score_handle
                &&& score_bridge::bridge_epathmap_score_snapshot(
                    later, owner.score_handle.unwrap(),
                ) == Some(owner.returns.last().body_bytes)
            }
        }),
{
    let count = state.frames.len();
    let owner = state.frames[count - 1];
    let parent = state.frames[count - 2];
    let next = admission::admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ).unwrap();
    let accepted = next.frames[count - 2];
    let returned = accepted.returns.last();
    assert(owner == state.frames.last());
    assert(returned.slot == parent.waiting_slot.unwrap());
    assert(returned.tag == parent.waiting_tag.unwrap());
    assert(returned.score_handle == owner.score_handle);
    assert(returned.kind == summary);
    if epathmap_payload_return(accepted, returned) {
        assert(admission::admission_arm_child_summary_agrees(
            parent, owner, summary, state.arena,
        ));
        assert(admission::admission_child_summary_agrees(owner, summary, state.arena));
        if parent.family == admission::AdmissionFamily::Other {
            assert(admission::admission_ezipper_pathmap_payload(parent, owner) == Some(summary));
        } else {
            assert(admission::admission_expr_structural_payload_with_arena(
                parent, owner, state.arena,
            ) == Some(summary));
            assert(admission::admission_expr_structural_payload(parent, owner) == Some(summary));
        }
        assert(epathmap_owner_frame(owner));
        assert(owner.score_handle.is_some());
        let handle = owner.score_handle.unwrap();
        let snapshot = owner.returns.last().body_bytes;
        epathmap_score_node_extends(transcript, later, handle, snapshot);
        epathmap_score_node_reads_back(later, handle, snapshot);
    }
}

/// Every successful CanonicalPath push keeps the invariant: the slotless
/// kind-4 parent is inert, and the pushed child has no returns and no score.
pub proof fn path_push_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        forall|source: admission::AdmissionCanonicalPathPushSource,
            child: admission::AdmissionFrameState,
            storage: admission::AdmissionCanonicalPathPushStorage|
            match #[trigger] admission::admission_canonical_path_push_indexed_step(
                state, source, child, storage,
            ) {
                Some(next) => epathmap_frames_shaped(transcript, next.frames),
                None => true,
            },
{
    assert forall|source: admission::AdmissionCanonicalPathPushSource,
        child: admission::AdmissionFrameState,
        storage: admission::AdmissionCanonicalPathPushStorage|
        match #[trigger] admission::admission_canonical_path_push_indexed_step(
            state, source, child, storage,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_canonical_path_push_indexed_step(
            state, source, child, storage,
        ) {
            let index = state.frames.len() - 1;
            let waiting = next.frames[index];
            let pushed = next.frames[index + 1];
            assert(next.frames == state.frames.update(index, waiting).push(pushed));
            epathmap_inert_frame_shaped(transcript, waiting);
            epathmap_fresh_frame_shaped(transcript, pushed);
            epathmap_child_pushed_shaped(transcript, state.frames, waiting, pushed);
        }
    }
}

/// Every successful CanonicalPath child return keeps the invariant: the child
/// is dropped and the kind-4 parent stops waiting.
pub proof fn path_return_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    state: admission::AdmissionState,
)
    requires epathmap_frames_shaped(transcript, state.frames),
    ensures
        forall|source: admission::AdmissionCanonicalPathReturnSource,
            summary: admission::AdmissionReturnedKind|
            match #[trigger] admission::admission_canonical_path_return_indexed_step(
                state, source, summary,
            ) {
                Some(next) => epathmap_frames_shaped(transcript, next.frames),
                None => true,
            },
{
    assert forall|source: admission::AdmissionCanonicalPathReturnSource,
        summary: admission::AdmissionReturnedKind|
        match #[trigger] admission::admission_canonical_path_return_indexed_step(
            state, source, summary,
        ) {
            Some(next) => epathmap_frames_shaped(transcript, next.frames),
            None => true,
        } by {
        if let Some(next) = admission::admission_canonical_path_return_indexed_step(
            state, source, summary,
        ) {
            let count = state.frames.len();
            let resolved = next.frames[count - 2];
            assert(next.frames == state.frames.subrange(0, count - 1).update(count - 2, resolved));
            epathmap_inert_frame_shaped(transcript, resolved);
            epathmap_parent_replaced_shaped(transcript, state.frames, resolved);
        }
    }
}

/// On a program-31 top, the finish selector is the owner step: groups 1 to 5
/// propose nothing, and group 6 proposes only the owner candidate.
pub proof fn owner_finish_selects_owner_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        epathmap_owner_frame(before.admission.frames.last()),
    ensures
        score_bridge::bridge_score_finish_selected(before)
            == score_bridge::bridge_epathmap_owner_score_step(before),
{
    score_bridge::bridge_score_finish_selection_group_6_earlier_groups_empty(before);
    assert(score_bridge::bridge_var_score_step(before).is_none());
    assert(score_bridge::bridge_ezipper_score_step(before).is_none());
    reveal(score_bridge::bridge_score_finish_selection_group_6);
    reveal(score_bridge::bridge_score_finish_selected);
}

/// The owner step scores the top owner with the EPathMap score node over its
/// snapshot, extends the transcript, and keeps every other frame field.
pub proof fn owner_score_step_shapes_owner(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures
        match score_bridge::bridge_epathmap_owner_score_step(before) {
            Some(next) => {
                &&& epathmap_transcript_extends(before.transcript, next.transcript)
                &&& next.admission.frames.last() == admission::AdmissionFrameState {
                    score_handle: next.admission.frames.last().score_handle,
                    ..before.admission.frames.last()
                }
                &&& epathmap_owner_snapshot_provenance(
                    next.transcript, next.admission.frames.last(),
                )
            },
            None => true,
        },
{
    score_bridge::bridge_epathmap_owner_score_step_exact(before);
    if let Some(next) = score_bridge::bridge_epathmap_owner_score_step(before) {
        let top = before.admission.frames.last();
        let snapshot = top.returns.last().body_bytes;
        score_bridge::bridge_pathmap_score_append_exact(before, snapshot, 13i32, 1usize, 4u32);
        let base = before.transcript.entries.len() as int;
        let handle_base = before.transcript.entries.len() as usize;
        let used = score_bridge::bridge_bool_slot(top, 1, 4).unwrap();
        let entries = next.transcript.entries;
        let suffix = score_bridge::bridge_transcript_suffix_kinds(next.transcript, base);
        let children = seq![(handle_base + 2) as usize, handle_base, (handle_base + 1) as usize];
        assert(seq![(handle_base + 2) as usize] + seq![handle_base, (handle_base + 1) as usize]
            =~= children);
        assert(suffix == score_bridge::production_pathmap_score_nodes(
            false, snapshot, used, handle_base,
        ));
        assert(suffix[0] == score::ScoreNode::BytesAtom(snapshot));
        assert(suffix[1] == score::ScoreNode::Integer(if used { 1i64 } else { 0i64 }));
        assert(suffix[2] == score::ScoreNode::Integer(13i64));
        assert(suffix[3] == score::ScoreNode::Node(children));
        assert(entries[base].kind == suffix[0]);
        assert(entries[base + 1].kind == suffix[1]);
        assert(entries[base + 2].kind == suffix[2]);
        assert(entries[base + 3].kind == suffix[3]);
        assert(children[0] as int == base + 2);
        assert(children[1] as int == base);
        assert(children[2] as int == base + 1);
        assert(epathmap_score_node(next.transcript, (base + 3) as usize, snapshot));
        assert(next.admission.frames.last().returns == top.returns);
    }
}

/// Every finish-selected score step keeps the invariant. Frames below the top
/// are kept and the transcript is extended. A program-31 top is scored by the
/// owner step, which shapes it; any other top keeps its returns and is not an
/// owner.
pub proof fn score_finish_keeps_epathmap_shaped(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        epathmap_score_nodes_shaped(before),
    ensures
        match score_bridge::bridge_score_finish_selected(before) {
            Some(after) => epathmap_score_nodes_shaped(after),
            None => true,
        },
{
    score_bridge::bridge_score_finish_selected_preserves_checked(before);
    if let Some(after) = score_bridge::bridge_score_finish_selected(before) {
        let frames = before.admission.frames;
        let count = frames.len();
        let top = frames[count - 1];
        let scored = after.admission.frames[count - 1];
        assert(frames.last() == top);
        assert(after.admission.frames.last() == scored);
        assert(epathmap_frame_shaped(before.transcript, frames[count - 1]));
        epathmap_frame_shaped_extends(before.transcript, after.transcript, top);
        if epathmap_owner_frame(top) {
            owner_finish_selects_owner_step(before);
            owner_score_step_shapes_owner(before);
        }
        assert forall|i: int| 0 <= i < scored.returns.len() implies
            #[trigger] epathmap_payload_return_shaped(after.transcript, scored, scored.returns[i]) by {
            assert(epathmap_payload_return_shaped(after.transcript, top, top.returns[i]));
        }
        assert(epathmap_frame_shaped(after.transcript, scored));
        assert forall|i: int| count - 1 <= i < after.admission.frames.len() implies
            #[trigger] epathmap_frame_shaped(after.transcript, after.admission.frames[i]) by {
            assert(i == count - 1);
        }
        epathmap_frames_assemble(
            before.transcript, after.transcript, frames, after.admission.frames, count - 1,
        );
    }
}

/// The EPathMap child return pops the child and resolves its parent, which
/// keeps the parent's view.
pub proof fn epm_resolved_parent_keeps_epathmap_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 1,
    ensures
        epathmap_frames_shaped(
            transcript,
            frames.subrange(0, frames.len() - 1).update(
                frames.len() - 2, epm::epm_parent_resolved(frames[frames.len() - 2]),
            ),
        ),
{
    let parent = frames[frames.len() - 2];
    let resolved = epm::epm_parent_resolved(parent);
    assert(epathmap_frame_shaped(transcript, frames[frames.len() - 2]));
    epathmap_view_kept_frame_shaped(transcript, parent, resolved);
    epathmap_parent_replaced_shaped(transcript, frames, resolved);
}

/// The EPathMap push keeps the invariant: its kind-3 parent is inert, and the
/// pushed child has no returns and no score.
pub proof fn epm_push_frames_keep_epathmap_shaped(
    transcript: score::ScoreArena,
    frames: Seq<admission::AdmissionFrameState>,
    consumed: usize,
    return_id: u64,
    kind: u8,
    child: admission::AdmissionFrameState,
)
    requires
        epathmap_frames_shaped(transcript, frames),
        frames.len() > 0,
        frames.last().frame_kind == 3,
        child.returns.len() == 0,
        child.score_handle.is_none(),
    ensures
        epathmap_frames_shaped(
            transcript,
            frames.update(
                frames.len() - 1,
                epm::epm_parent_waiting(
                    epm::epm_advanced_admission_cursor(frames.last(), consumed),
                    return_id, kind,
                ),
            ).push(epm::epm_assigned_child(child, return_id)),
        ),
{
    let waiting = epm::epm_parent_waiting(
        epm::epm_advanced_admission_cursor(frames.last(), consumed), return_id, kind,
    );
    let assigned = epm::epm_assigned_child(child, return_id);
    epathmap_inert_frame_shaped(transcript, waiting);
    epathmap_fresh_frame_shaped(transcript, assigned);
    epathmap_child_pushed_shaped(transcript, frames, waiting, assigned);
}

/// The EPathMap finish accept keeps the invariant: it pops the EPathMap child
/// into its program-31 parent, which waited for it and so is unscored, and
/// rewrites that parent's last return into its snapshot. The parent is an owner
/// without a score and holds no EPathMap payload returns.
pub proof fn epm_accept_frames_keep_epathmap_shaped(
    transcript: score::ScoreArena,
    before: epm::LinkedEpmState,
)
    requires epathmap_frames_shaped(transcript, before.admission.frames),
    ensures
        match epm_source_accept::epm_source_finish_accept(before) {
            Some(after) => epathmap_frames_shaped(transcript, after.frames),
            None => true,
        },
{
    if let Some(after) = epm_source_accept::epm_source_finish_accept(before) {
        let finished = epm_source_accept::epm_end_linked(before);
        assert(epm_source_accept::epm_source_accept(before, finished) == Some(after));
        let count = before.admission.frames.len();
        let child = before.admission.frames[count - 1];
        let parent = before.admission.frames[count - 2];
        assert(child == before.admission.frames.last());
        let base = admission::admission_complete_child_step(
            before.admission, child.return_id.unwrap(), 3,
            admission::AdmissionReturnedKind::Other,
        ).unwrap();
        let accepted = after.frames[count - 2];
        assert(base.frames.len() == count - 1);
        assert(after.frames == base.frames.update(count - 2, accepted));
        assert(base.frames == before.admission.frames.subrange(0, count - 1).update(
            count - 2, base.frames[count - 2],
        ));
        assert(after.frames =~= before.admission.frames.subrange(0, count - 1).update(
            count - 2, accepted,
        ));
        assert(parent.waiting_id == Some(child.return_id.unwrap()));
        assert(epathmap_frame_shaped(transcript, before.admission.frames[count - 2]));
        assert(accepted.score_handle == parent.score_handle);
        epathmap_inert_frame_shaped(transcript, accepted);
        epathmap_parent_replaced_shaped(transcript, before.admission.frames, accepted);
    }
}

/// The admission root of `admission_initial_state` satisfies the invariant
/// under every transcript.
pub proof fn initial_root_frames_shaped(
    transcript: score::ScoreArena,
    root: admission::AdmissionFrameState,
)
    requires admission::admission_root_initial(root),
    ensures epathmap_frames_shaped(transcript, admission::admission_initial_state(root).frames),
{
    let frames = admission::admission_initial_state(root).frames;
    epathmap_fresh_frame_shaped(transcript, root);
    assert forall|i: int| 0 <= i < frames.len() implies
        #[trigger] epathmap_frame_shaped(transcript, frames[i]) by {
        assert(frames[i] == root);
    }
}

/// An append to a transcript that already holds `usize::MAX` entries is
/// refused: `score::checked_append` has no handle left to give.
pub proof fn full_transcript_refuses_append(
    bridge: score_bridge::ScoreAdmissionBridge,
    kind: score::ScoreNode,
)
    requires bridge.transcript.entries.len() >= usize::MAX,
    ensures score_bridge::bridge_append_unattached(bridge, kind).is_none(),
{
}

/// A successful unattached append adds exactly one transcript entry.
pub proof fn unattached_append_adds_one_entry(
    bridge: score_bridge::ScoreAdmissionBridge,
    kind: score::ScoreNode,
)
    ensures
        match score_bridge::bridge_append_unattached(bridge, kind) {
            Some((next, _)) =>
                next.transcript.entries.len() == bridge.transcript.entries.len() + 1,
            None => true,
        },
{
}

/// The constructor step appends the constructor integer and then the node, so
/// a transcript without room for two more entries refuses it.
pub proof fn constructor_step_needs_two_entries(
    bridge: score_bridge::ScoreAdmissionBridge,
    constructor: i32,
    children: Seq<usize>,
)
    requires bridge.transcript.entries.len() + 2 > usize::MAX,
    ensures score_bridge::bridge_constructor_step(bridge, constructor, children).is_none(),
{
    let constructor_kind = score::ScoreNode::Integer(constructor as i64);
    unattached_append_adds_one_entry(bridge, constructor_kind);
    if bridge.transcript.entries.len() >= usize::MAX {
        full_transcript_refuses_append(bridge, constructor_kind);
    } else if let Some((after_constructor, constructor_handle)) =
        score_bridge::bridge_append_unattached(bridge, constructor_kind)
    {
        full_transcript_refuses_append(
            after_constructor, score::ScoreNode::Node(seq![constructor_handle] + children),
        );
    }
}

/// The PathMap score append appends the snapshot atom, the flag integer and,
/// through the constructor step, the constructor integer and the node, so a
/// transcript without room for four more entries refuses it, whatever the
/// flag reads.
pub proof fn pathmap_append_needs_four_entries(
    before: score_bridge::ScoreAdmissionBridge,
    snapshot: Seq<u8>,
    constructor: i32,
    flag_slot: usize,
    flag_tag: u32,
)
    requires before.transcript.entries.len() + 4 > usize::MAX,
    ensures
        score_bridge::bridge_pathmap_score_append(
            before, snapshot, constructor, flag_slot, flag_tag,
        ).is_none(),
{
    if let Some(used) = score_bridge::bridge_bool_slot(
        before.admission.frames.last(), flag_slot, flag_tag,
    ) {
        let bytes_kind = score::ScoreNode::BytesAtom(snapshot);
        unattached_append_adds_one_entry(before, bytes_kind);
        if before.transcript.entries.len() >= usize::MAX {
            full_transcript_refuses_append(before, bytes_kind);
        } else if let Some((after_bytes, bytes_handle)) =
            score_bridge::bridge_append_unattached(before, bytes_kind)
        {
            let flag_kind = score::ScoreNode::Integer(if used { 1i64 } else { 0i64 });
            unattached_append_adds_one_entry(after_bytes, flag_kind);
            if after_bytes.transcript.entries.len() >= usize::MAX {
                full_transcript_refuses_append(after_bytes, flag_kind);
            } else if let Some((after_flag, flag_handle)) =
                score_bridge::bridge_append_unattached(after_bytes, flag_kind)
            {
                constructor_step_needs_two_entries(
                    after_flag, constructor, seq![bytes_handle, flag_handle],
                );
            }
        }
    }
}

/// Finish group 1 maps the empty selection to itself only when each of its
/// steps is None: from `NoCandidate` a Some candidate gives `Unique`, and
/// `bridge_add_score_finish_candidate` never leads from `Unique` or
/// `Ambiguous` back to `NoCandidate`.
pub proof fn finish_group_1_empty_has_no_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_score_finish_selection_group_1(
            before, score_bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == score_bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures epathmap_group_1_steps_none(before),
{
    reveal(score_bridge::bridge_score_finish_selection_group_1);
}

/// Finish group 2 maps the empty selection to itself only when each of its
/// steps is None.
pub proof fn finish_group_2_empty_has_no_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_score_finish_selection_group_2(
            before, score_bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == score_bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures epathmap_group_2_steps_none(before),
{
    reveal(score_bridge::bridge_score_finish_selection_group_2);
}

/// Finish group 3 maps the empty selection to itself only when each of its
/// steps is None.
pub proof fn finish_group_3_empty_has_no_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_score_finish_selection_group_3(
            before, score_bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == score_bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures epathmap_group_3_steps_none(before),
{
    reveal(score_bridge::bridge_score_finish_selection_group_3);
}

/// Finish group 4 maps the empty selection to itself only when each of its
/// steps is None.
pub proof fn finish_group_4_empty_has_no_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_score_finish_selection_group_4(
            before, score_bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == score_bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures epathmap_group_4_steps_none(before),
{
    reveal(score_bridge::bridge_score_finish_selection_group_4);
}

/// Finish group 5 maps the empty selection to itself only when each of its
/// steps is None.
pub proof fn finish_group_5_empty_has_no_step(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_score_finish_selection_group_5(
            before, score_bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == score_bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures epathmap_group_5_steps_none(before),
{
    reveal(score_bridge::bridge_score_finish_selection_group_5);
}

/// Completeness on one bridge when the transcript has room. An EZipper at EOF
/// whose PathMap return is present and whose connective flag is readable, on
/// a bridge that satisfies the invariant and has room for four more entries,
/// has a Some EZipper score step, and the finish selector picks it. The
/// invariant shapes the PathMap return, whose own node bounds the length of the
/// appended node; the frame's well-formedness gives the nested balance, and
/// program 32 has no PathMap-kind field, so no slot is required.
pub proof fn ezipper_eof_score_step_is_complete(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        epathmap_score_nodes_shaped(before),
        before.admission.frames.len() > 0,
        epathmap_ezipper_eof_present(before.admission.frames.last()),
        score_bridge::bridge_bool_slot(before.admission.frames.last(), 4, 5).is_some(),
        epathmap_score_append_room(before.transcript),
    ensures
        score_bridge::bridge_ezipper_score_step(before).is_some(),
        score_bridge::bridge_score_finish_selected(before)
            == score_bridge::bridge_ezipper_score_step(before),
{
    let frames = before.admission.frames;
    let count = frames.len();
    let top = frames[count - 1];
    assert(top == frames.last());
    assert(epathmap_frame_shaped(before.transcript, frames[count - 1]));
    assert(epathmap_payload_return_shaped(before.transcript, top, top.returns[0]));
    assert(epathmap_payload_return(top, top.returns[0]));
    let handle = top.returns[0].score_handle.unwrap();
    let snapshot = score_bridge::bridge_epathmap_score_snapshot(
        before.transcript, handle,
    ).unwrap();
    assert(epathmap_score_node(before.transcript, handle, snapshot));
    epathmap_score_node_snapshot_fits(before.transcript, handle, snapshot);
    assert(admission::admission_frame_wf(frames[count - 1]));
    assert(top.waiting_kind == 0);
    assert(top.nested_started == top.nested_resolved);
    assert forall|slot: int| top.required.contains(slot) implies top.seen.contains(slot) by {
        assert(0 <= slot < top.slot_count as int);
        assert forall|tag: u32| !admission::admission_required_descriptor_slot(
            top.program_type_id, tag, slot,
        ) by {}
    }
    assert(admission::admission_slots_complete(top));
    score_bridge::bridge_pathmap_score_append_succeeds(before, snapshot, 14i32, 4usize, 5u32);
    assert(score_bridge::bridge_ezipper_score_step(before)
        == score_bridge::bridge_pathmap_score_append(before, snapshot, 14i32, 4usize, 5u32));
    score_bridge::bridge_score_finish_selection_group_6_earlier_groups_empty(before);
    assert(score_bridge::bridge_var_score_step(before).is_none());
    assert(score_bridge::bridge_epathmap_owner_score_step(before).is_none());
    reveal(score_bridge::bridge_score_finish_selection_group_6);
    reveal(score_bridge::bridge_score_finish_selected);
}

/// Without room for four more entries, every finish candidate of an EZipper at
/// EOF is None: the EZipper score step runs out of transcript entries before
/// it attaches its node, the Var and owner steps need programs 6 and 31, and
/// groups 1 to 5 map the empty selection to itself on a program-32 top, which
/// leaves each of their steps None. So the finish selector picks nothing.
/// Neither the invariant nor the flag is needed.
pub proof fn ezipper_eof_without_room_has_no_score_step(
    before: score_bridge::ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        epathmap_ezipper_eof_present(before.admission.frames.last()),
        !epathmap_score_append_room(before.transcript),
    ensures
        epathmap_finish_candidates_none(before),
        score_bridge::bridge_score_finish_selected(before).is_none(),
{
    let top = before.admission.frames.last();
    if let Some(handle) = top.returns[0].score_handle {
        if let Some(snapshot) = score_bridge::bridge_epathmap_score_snapshot(
            before.transcript, handle,
        ) {
            pathmap_append_needs_four_entries(before, snapshot, 14i32, 4usize, 5u32);
        }
    }
    assert(score_bridge::bridge_ezipper_score_step(before).is_none());
    assert(score_bridge::bridge_var_score_step(before).is_none());
    assert(score_bridge::bridge_epathmap_owner_score_step(before).is_none());
    score_bridge::bridge_score_finish_selection_group_6_earlier_groups_empty(before);
    finish_group_1_empty_has_no_step(before);
    finish_group_2_empty_has_no_step(before);
    finish_group_3_empty_has_no_step(before);
    finish_group_4_empty_has_no_step(before);
    finish_group_5_empty_has_no_step(before);
    reveal(score_bridge::bridge_score_finish_selection_group_6);
    reveal(score_bridge::bridge_score_finish_selected);
}

/// Completeness on one bridge as a dichotomy with no room premise: an EZipper
/// at EOF whose PathMap return is present and whose connective flag is
/// readable, on a bridge that satisfies the invariant, has the EZipper score
/// step as its selected finish when the transcript has room for four more
/// entries, and every finish candidate None otherwise.
pub proof fn ezipper_eof_score_step_dichotomy(before: score_bridge::ScoreAdmissionBridge)
    requires
        score_bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        epathmap_score_nodes_shaped(before),
        before.admission.frames.len() > 0,
        epathmap_ezipper_eof_present(before.admission.frames.last()),
        score_bridge::bridge_bool_slot(before.admission.frames.last(), 4, 5).is_some(),
    ensures epathmap_ezipper_score_dichotomy(before),
{
    if epathmap_score_append_room(before.transcript) {
        ezipper_eof_score_step_is_complete(before);
    } else {
        ezipper_eof_without_room_has_no_score_step(before);
    }
}

/// The witness EZipper with its unscored owner on top satisfies the invariant.
pub proof fn ezipper_witness_before_is_shaped()
    ensures epathmap_score_nodes_shaped(score_bridge::bridge_ezipper_witness_before()),
{
    let before = score_bridge::bridge_ezipper_witness_before();
    let frames = before.admission.frames;
    let parent = score_bridge::bridge_ezipper_witness_parent();
    let owner = score_bridge::bridge_ezipper_witness_owner_child();
    assert(frames[0] == parent);
    assert(frames[1] == owner);
    epathmap_fresh_frame_shaped(before.transcript, parent);
    epathmap_inert_frame_shaped(before.transcript, owner);
    assert forall|i: int| 0 <= i < frames.len() implies
        #[trigger] epathmap_frame_shaped(before.transcript, frames[i]) by {
        if i == 0 {
            assert(frames[i] == parent);
        } else {
            assert(frames[i] == owner);
        }
    }
}

/// Witness: an EZipper over a scored owner. From the shaped witness bridge,
/// the finish selector scores the owner, the owner returns into the EZipper,
/// both steps keep the invariant, and the completeness lemma gives the EZipper
/// score step on the result, whose PathMap return carries the owner's handle.
pub proof fn ezipper_witness_reaches_complete_score()
    ensures
        ({
            let before = score_bridge::bridge_ezipper_witness_before();
            &&& epathmap_score_nodes_shaped(before)
            &&& match score_bridge::bridge_score_finish_selected(before) {
                Some(scored) => {
                    &&& epathmap_score_nodes_shaped(scored)
                    &&& match admission::admission_complete_child_step(
                        scored.admission, 0, 1, score_bridge::bridge_ezipper_witness_payload(),
                    ) {
                        Some(admission) => {
                            let done = score_bridge::ScoreAdmissionBridge {
                                admission,
                                transcript: scored.transcript,
                            };
                            &&& Some(done) == score_bridge::bridge_ezipper_witness_after_child()
                            &&& epathmap_score_nodes_shaped(done)
                            &&& epathmap_ezipper_eof_present(done.admission.frames.last())
                            &&& done.admission.frames.last().returns[0].score_handle
                                == scored.admission.frames.last().score_handle
                            &&& score_bridge::bridge_ezipper_score_step(done).is_some()
                            &&& score_bridge::bridge_score_finish_selected(done)
                                == score_bridge::bridge_ezipper_score_step(done)
                        },
                        None => false,
                    }
                },
                None => false,
            }
        }),
{
    let before = score_bridge::bridge_ezipper_witness_before();
    score_bridge::bridge_ezipper_witness_before_wf();
    score_bridge::bridge_ezipper_witness_owner_scores();
    score_bridge::bridge_ezipper_witness_child_completes();
    score_bridge::bridge_epathmap_witness_snapshot_score_length();
    ezipper_witness_before_is_shaped();
    owner_finish_selects_owner_step(before);
    score_finish_keeps_epathmap_shaped(before);
    let scored = score_bridge::bridge_epathmap_owner_score_step(before).unwrap();
    let payload = score_bridge::bridge_ezipper_witness_payload();
    complete_child_keeps_epathmap_shaped(scored.transcript, scored.admission, 0, 1, payload);
    let admission_after = admission::admission_complete_child_step(
        scored.admission, 0, 1, payload,
    ).unwrap();
    let done = score_bridge::ScoreAdmissionBridge {
        admission: admission_after,
        transcript: scored.transcript,
    };
    assert(Some(done) == score_bridge::bridge_ezipper_witness_after_child());
    let top = done.admission.frames.last();
    assert(score_bridge::bridge_bool_slot(top, 4, 5) == Some(false));
    assert(epathmap_ezipper_eof_present(top));
    assert(epathmap_score_append_room(done.transcript));
    ezipper_eof_score_step_is_complete(done);
}

/// A Message Continue or ContinueIndexed map step records one scalar
/// occurrence on the top frame under the same transcript.
pub proof fn product_message_continue_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        match action {
            admission::ConcreteFrameActionModel::Continue { .. }
            | admission::ConcreteFrameActionModel::ContinueIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let state = before.map.admission;
    let next = after.map.admission;
    assert(admission::concrete_message_driver_step(state, action)
        == Some(admission::ConcreteFrameOutcomeModel::Continued { state: next }));
    assert(after.bridge.admission == next);
    assert(after.bridge.transcript == before.bridge.transcript);
    scalar_record_keeps_epathmap_shaped(before.bridge.transcript, state);
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.projection == state);
    match action {
        admission::ConcreteFrameActionModel::Continue { field } => {
            admission::concrete_message_continue_simulates(view, field);
            let moved = admission::concrete_message_continue(view, field).unwrap();
            assert(next == moved.projection);
        },
        admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            admission::concrete_message_continue_indexed_simulates(view, field, parent_source);
            let moved = admission::concrete_message_continue_indexed(
                view, field, parent_source,
            ).unwrap();
            assert(next == moved.projection);
        },
        _ => {},
    }
}

/// A Message Push or PushIndexed map step makes the unscored top wait and
/// pushes a fresh child under the same transcript.
pub proof fn product_message_push_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        match action {
            admission::ConcreteFrameActionModel::Push { .. }
            | admission::ConcreteFrameActionModel::PushIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let state = before.map.admission;
    let next = after.map.admission;
    let source_action = map::integrated_source_message_action(state, action).unwrap();
    assert(map::integrated_admission_driver_step(
        before.map, map::IntegratedAdmissionAction::Message { action },
    ) == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(admission::concrete_message_driver_step(state, source_action)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: next }));
    assert(after.bridge.admission == next);
    assert(after.bridge.transcript == before.bridge.transcript);
    admission_push_keeps_epathmap_shaped(before.bridge.transcript, state);
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.projection == state);
    match source_action {
        admission::ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            admission::concrete_message_push_simulates(view, field, child, frame_bytes);
            assert(admission::concrete_message_push(view, field, child, frame_bytes)
                == Some(next));
        },
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            assert(admission::concrete_message_push_indexed(
                view, field, child, parent_source, frame_bytes,
            ) == Some(next));
        },
        _ => {},
    }
}

/// A MapKey map step is the continue step of the key field on the Map top,
/// under the same transcript.
pub proof fn product_map_key_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        integrated::product_map_step(
            before, after, map::IntegratedAdmissionAction::MapKey { field },
        ),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    let continued = map::map_admission_key_continue(current, before.map.admission, field).unwrap();
    assert(after.bridge.admission == continued.1);
    assert(after.bridge.transcript == before.bridge.transcript);
    assert(admission::admission_continue_step(
        before.map.admission, field.encoded_end, 0, false, 1,
    ) == Some(continued.1));
    admission_continue_keeps_epathmap_shaped(before.bridge.transcript, before.map.admission);
}

/// A MapValue or MapValueIndexed map step makes the unscored Map top wait and
/// pushes a fresh value child under the same transcript.
pub proof fn product_map_value_push_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        match action {
            map::IntegratedAdmissionAction::MapValue { .. }
            | map::IntegratedAdmissionAction::MapValueIndexed { .. } => true,
            _ => false,
        },
        integrated::product_map_step(before, after, action),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    assert(map::integrated_admission_driver_step(before.map, action)
        == Some(map::IntegratedAdmissionOutcome::MapValuePushed { state: after.map }));
    assert(after.bridge.admission == after.map.admission);
    assert(after.bridge.transcript == before.bridge.transcript);
    admission_push_keeps_epathmap_shaped(before.bridge.transcript, before.map.admission);
    match action {
        map::IntegratedAdmissionAction::MapValue { field, child, frame_bytes } => {
            let pushed = map::map_admission_value_push(
                current, before.map.admission, field, child, frame_bytes,
            ).unwrap();
            assert(after.map.admission == pushed.1);
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) == Some(pushed.1));
        },
        map::IntegratedAdmissionAction::MapValueIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            let pushed = map::map_admission_value_push_indexed(
                current, before.map.admission, field, child, parent_source, frame_bytes,
            ).unwrap();
            assert(after.map.admission == pushed.1);
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child, 1, false, 2, frame_bytes,
            ) == Some(pushed.1));
        },
        _ => {},
    }
}

/// A Message Complete, MapAccept or MapFinish map step is a child Complete of
/// the admission state, under the same transcript; the Message Complete of an
/// EPathMap owner into its EZipper or Expr parent is the case that needs the
/// Program-31 clause (`complete_child_keeps_epathmap_shaped`).
pub proof fn product_map_pop_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        match action {
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            }
            | map::IntegratedAdmissionAction::MapAccept
            | map::IntegratedAdmissionAction::MapFinish => true,
            _ => false,
        },
        integrated::product_map_step(before, after, action),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let state = before.map.admission;
    let transcript = before.bridge.transcript;
    let count = state.frames.len();
    let next = after.map.admission;
    assert(after.bridge.admission == next);
    assert(after.bridge.transcript == transcript);
    match action {
        map::IntegratedAdmissionAction::Message { .. } => {
            assert(map::integrated_source_message_action(
                state, admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameActionModel::Complete));
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                    state: after.map,
                }));
            assert(admission::concrete_message_driver_step(
                state, admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: next }));
            admission::concrete_message_complete_child_simulates(state);
            let child = state.frames.last();
            let parent = state.frames[count - 2];
            let summary = admission::concrete_derived_summary_with_arena(
                parent, child, state.arena,
            ).unwrap();
            assert(admission::admission_complete_child_step(
                state, child.return_id.unwrap(), child.frame_kind, summary,
            ) == Some(next));
            complete_child_keeps_epathmap_shaped(
                transcript, state, child.return_id.unwrap(), child.frame_kind, summary,
            );
        },
        map::IntegratedAdmissionAction::MapAccept => {
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapValueAccepted {
                    state: after.map,
                }));
            let parent_frame = before.map.map_frames[count - 2].unwrap();
            let accepted = map::map_admission_value_complete(parent_frame, state).unwrap();
            let wait = parent_frame.pending.unwrap();
            let summary = admission::AdmissionReturnedKind::Par {
                semantic_default: false,
                canonical_path_stable: accepted.0.value.unwrap().canonical_path_stable,
            };
            assert(next == accepted.1);
            assert(admission::admission_complete_child_step(state, wait.id, 1, summary)
                == Some(next));
            complete_child_keeps_epathmap_shaped(transcript, state, wait.id, 1, summary);
        },
        map::IntegratedAdmissionAction::MapFinish => {
            let map_frame = before.map.map_frames[count - 1].unwrap();
            let finished = map::map_admission_entry_complete(map_frame, state).unwrap();
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapEntryCompleted {
                    summary: finished.0,
                    state: after.map,
                }));
            let child = state.frames.last();
            let returned = admission::AdmissionReturnedKind::Map {
                key_bytes: finished.0.key_bytes,
                value_score_handle: admission::admission_map_value_score_handle(child),
            };
            assert(next == finished.1);
            assert(admission::admission_complete_child_step(
                state, child.return_id.unwrap(), 2, returned,
            ) == Some(next));
            complete_child_keeps_epathmap_shaped(
                transcript, state, child.return_id.unwrap(), 2, returned,
            );
        },
        _ => {},
    }
}

/// The internal EPathMap continue advances the top frame's byte cursor under
/// the same transcript.
pub proof fn product_epm_continue_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    event: epm::EpmEvent,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        integrated::product_epm_continue_step(before, after, event),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let prior = integrated::product_linked_top(before).unwrap();
    let next = integrated::product_linked_top(after).unwrap();
    assert(epm::linked_epm_continue(prior, next, event));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.update(
        prior.admission.frames.len() - 1,
        epm::epm_advanced_admission_cursor(
            prior.admission.frames.last(), next.epm.cursor.value_table_offset,
        ),
    ));
    assert(before.bridge.admission.frames.len() > 0);
    epathmap_cursor_advanced_shaped(
        before.bridge.transcript, before.bridge.admission.frames,
        next.epm.cursor.value_table_offset,
    );
}

/// The EPathMap Map-value return pops the child and resolves its EPathMap
/// parent under the same transcript.
pub proof fn product_epm_child_return_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::product_score_wf(before),
        epathmap_score_nodes_shaped(before.bridge),
        integrated::product_epm_child_return_step(
            before, after, summary, admission_summary, None,
        ),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let prior = integrated::product_linked_parent(before).unwrap();
    let next = integrated::product_linked_top(after).unwrap();
    assert(epm::linked_epm_child_return(
        prior, next, summary, admission_summary, before.epm_frames.last(), None,
    ));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.subrange(
        0, prior.admission.frames.len() - 1,
    ).update(
        prior.admission.frames.len() - 2,
        epm::epm_parent_resolved(prior.admission.frames[prior.admission.frames.len() - 2]),
    ));
    assert(before.bridge.admission.frames.len() > 1);
    epm_resolved_parent_keeps_epathmap_shaped(
        before.bridge.transcript, before.bridge.admission.frames,
    );
}

/// The EPathMap finish accept is `epm_source_finish_accept` of the linked top,
/// under the same transcript.
pub proof fn product_epm_finish_accept_keeps_epathmap_shaped(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        epathmap_score_nodes_shaped(before.bridge),
        integrated::product_epm_finish_accept_step(before, after),
    ensures epathmap_score_nodes_shaped(after.bridge),
{
    let prior = integrated::product_linked_top(before).unwrap();
    assert(prior.admission == before.bridge.admission);
    assert(epm_source_accept::epm_source_finish_accept(prior) == Some(after.bridge.admission));
    assert(after.bridge.transcript == before.bridge.transcript);
    epm_accept_frames_keep_epathmap_shaped(before.bridge.transcript, prior);
}

/// Live Continue: the Message Continue map step of the top field.
pub proof fn live_continue_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(integrated::live_source_product_continue_step(before.source, after.source));
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_message_continue_keeps_epathmap_shaped(
                    before.source.product, after.source.product,
                    admission::ConcreteFrameActionModel::Continue { field },
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live ContinueIndexed: the Message ContinueIndexed map step of the top
/// field over the top indexed region.
pub proof fn live_continue_indexed_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_continue_indexed_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(integrated::live_source_product_continue_indexed_step(before.source, after.source));
    let parent_source = integrated::product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_message_continue_keeps_epathmap_shaped(
                    before.source.product, after.source.product,
                    admission::ConcreteFrameActionModel::ContinueIndexed {
                        field, parent_source,
                    },
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live MapKey: the MapKey map step of the top field.
pub proof fn live_map_key_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_key_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(integrated::live_source_product_map_key_step(before.source, after.source));
    match before.source.wire.last() {
        Some(reader) => match wire_source::wire_source_next_field(reader) {
            wire_source::WireSourceTransition::Field { field, .. } =>
                product_map_key_keeps_epathmap_shaped(
                    before.source.product, after.source.product, field,
                ),
            _ => {},
        },
        None => {},
    }
}

/// Live RootScore, RootComplete and MessageChildScore: the finish-selected
/// score step of the bridge.
pub proof fn live_score_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_wf(before),
        score_bridge::bridge_score_finish_selected(before.source.product.bridge)
            == Some(after.source.product.bridge),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    let product = before.source.product;
    assert(integrated::product_score_wf(product));
    assert(admission::admission_state_wf(product.bridge.admission));
    score_finish_keeps_epathmap_shaped(product.bridge);
}

/// Live RootCompleteNoScore: the step keeps the whole product.
pub proof fn live_root_noscore_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    returned: runtime_source::RuntimeNoScoreRootReturnImage,
)
    requires
        integrated::live_runtime_product_root_complete_noscore_step(before, after, returned),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    integrated::live_runtime_product_root_complete_noscore_keeps_state(before, after, returned);
}

/// Live MessagePush: the plain or indexed Message Push map step of the top
/// field, whichever side of the observed step holds.
pub proof fn live_message_push_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_message_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            if integrated::live_runtime_product_message_push_step(
                before, after, request, storage,
            ) {
                product_message_push_keeps_epathmap_shaped(
                    old, new, admission::ConcreteFrameActionModel::Push {
                        field,
                        child: request.child,
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            } else {
                assert(integrated::live_runtime_product_message_push_indexed_step(
                    before, after, request, storage,
                ));
                product_message_push_keeps_epathmap_shaped(
                    old, new, admission::ConcreteFrameActionModel::PushIndexed {
                        field,
                        child: request.child,
                        parent_source: integrated::product_admission_source_image(
                            old.indexed_regions.last(),
                        ),
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            }
        }
    }
}

/// Live MessageEpmPush: the indexed Message Push map step of a PathMap field,
/// whose child is an EPathMap frame.
pub proof fn live_message_epm_push_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_message_epm_push_indexed_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        assert(integrated::live_runtime_product_message_epm_push_indexed_step(
            before, after, request, storage,
        ));
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            product_message_push_keeps_epathmap_shaped(
                old, new, admission::ConcreteFrameActionModel::PushIndexed {
                    field,
                    child: request.child,
                    parent_source: integrated::product_admission_source_image(
                        old.indexed_regions.last(),
                    ),
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            );
        }
    }
}

/// Live MapValuePush: the plain or indexed MapValue map step of the top
/// field, whichever side of the observed step holds.
pub proof fn live_map_value_push_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        integrated::live_runtime_product_map_value_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    let old = before.source.product;
    let new = after.source.product;
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            old.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
            wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
            if integrated::live_runtime_product_map_value_push_step(
                before, after, request, storage,
            ) {
                product_map_value_push_keeps_epathmap_shaped(
                    old, new, map::IntegratedAdmissionAction::MapValue {
                        field,
                        child: request.child,
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            } else {
                assert(integrated::live_runtime_product_map_value_push_indexed_step(
                    before, after, request, storage,
                ));
                product_map_value_push_keeps_epathmap_shaped(
                    old, new, map::IntegratedAdmissionAction::MapValueIndexed {
                        field,
                        child: request.child,
                        parent_source: integrated::product_admission_source_image(
                            old.indexed_regions.last(),
                        ),
                        frame_bytes: before.source.runtime.frame_bytes,
                    },
                );
            }
        }
    }
}

/// Live MessageChildComplete: the Message Complete map step, which pops the
/// finished child into its Message parent.
pub proof fn live_message_child_complete_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_message_child_complete_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    product_map_pop_keeps_epathmap_shaped(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    );
}

/// Live MapValueAccept: the MapAccept map step.
pub proof fn live_map_value_accept_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_value_accept_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    product_map_pop_keeps_epathmap_shaped(
        before.source.product, after.source.product, map::IntegratedAdmissionAction::MapAccept,
    );
}

/// Live MapEntryFinish: the MapFinish map step.
pub proof fn live_map_entry_finish_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_map_entry_finish_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    reveal(integrated::live_runtime_product_map_entry_finish_step);
    product_map_pop_keeps_epathmap_shaped(
        before.source.product, after.source.product, map::IntegratedAdmissionAction::MapFinish,
    );
}

/// Live EpmMapValueAccept: the EPathMap Map-value return.
pub proof fn live_epm_map_value_accept_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        integrated::live_runtime_product_epm_map_value_accept_step(
            before, after, summary, admission_summary,
        ),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    product_epm_child_return_keeps_epathmap_shaped(
        before.source.product, after.source.product, summary, admission_summary,
    );
}

/// Live EpmContinue: the internal EPathMap continue of the product.
pub proof fn live_epm_continue_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    event: epm::EpmEvent,
)
    requires
        integrated::live_runtime_product_wf(before),
        unwind_product::live_runtime_internal_epm_unwind_step(before, after, event),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(unwind_product::live_internal_epm_unwind_step(before.source, after.source, event));
    product_epm_continue_keeps_epathmap_shaped(
        before.source.product, after.source.product, event,
    );
}

/// Live EpmPush: the EPathMap top, a kind-3 frame, waits, and the requested
/// child, with no returns and no score, is pushed under the same transcript.
pub proof fn live_epm_push_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: push_source::EpmPushLiveObservation,
)
    requires
        push_live::epm_push_live_after(before, observed) == Some(after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(push_live::epm_push_live_ready(before, observed));
    assert(push_source::epm_push_source_ready(before, observed.push));
    let old = before.source.product;
    let new = after.source.product;
    let prior = integrated::product_linked_top(old).unwrap();
    let request = push_source::epm_push_source_request(prior, observed.push).unwrap();
    let consumed = observed.push.reducer_after.cursor.value_table_offset;
    let return_id = epm::request_return_id(request);
    let kind = epm::request_frame_kind(request);
    assert(new == push_source::epm_push_source_product_after(old, observed.push, request));
    assert(prior.admission == old.bridge.admission);
    assert(new.bridge.transcript == old.bridge.transcript);
    assert(new.bridge.admission.frames == prior.admission.frames.update(
        prior.admission.frames.len() - 1,
        epm::epm_parent_waiting(
            epm::epm_advanced_admission_cursor(prior.admission.frames.last(), consumed),
            return_id, kind,
        ),
    ).push(epm::epm_assigned_child(observed.push.child, return_id)));
    epm_push_frames_keep_epathmap_shaped(
        old.bridge.transcript, prior.admission.frames, consumed, return_id, kind,
        observed.push.child,
    );
}

/// Live EpmEnd: the EPathMap completion keeps the bridge.
pub proof fn live_epm_end_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: epm_terminal::EpmTerminalSourceObservation,
)
    requires
        integrated::live_runtime_product_epm_end_source_step(before, after, observed),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    integrated::live_runtime_product_epm_end_source_preserves_wf(before, after, observed);
}

/// Live EpmFinishAccept: the EPathMap finish accept of the product.
pub proof fn live_epm_finish_accept_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        integrated::live_runtime_product_epm_finish_accept_step(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    assert(integrated::live_source_product_epm_finish_accept_step(before.source, after.source));
    product_epm_finish_accept_keeps_epathmap_shaped(before.source.product, after.source.product);
}

/// Every Live product action keeps the invariant: one lemma per action.
pub proof fn live_runtime_step_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires
        integrated::live_runtime_product_step(before, after, action),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    match action {
        LiveRuntimeProductAction::Continue =>
            live_continue_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_continue_indexed_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::MapKey =>
            live_map_key_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::RootScore =>
            live_score_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::RootComplete { .. } =>
            live_score_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::RootCompleteNoScore { returned } =>
            live_root_noscore_keeps_epathmap_shaped(before, after, returned),
        LiveRuntimeProductAction::MessageChildScore =>
            live_score_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_message_push_keeps_epathmap_shaped(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_message_epm_push_keeps_epathmap_shaped(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_map_value_push_keeps_epathmap_shaped(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageChildComplete =>
            live_message_child_complete_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::MapValueAccept =>
            live_map_value_accept_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::MapEntryFinish =>
            live_map_entry_finish_keeps_epathmap_shaped(before, after),
        LiveRuntimeProductAction::EpmMapValueAccept { summary, admission_summary } =>
            live_epm_map_value_accept_keeps_epathmap_shaped(
                before, after, summary, admission_summary,
            ),
        LiveRuntimeProductAction::EpmContinue { event } =>
            live_epm_continue_keeps_epathmap_shaped(before, after, event),
        LiveRuntimeProductAction::EpmPush { observed } =>
            live_epm_push_keeps_epathmap_shaped(before, after, observed),
        LiveRuntimeProductAction::EpmEnd { observed } =>
            live_epm_end_keeps_epathmap_shaped(before, after, observed),
        LiveRuntimeProductAction::EpmFinishAccept =>
            live_epm_finish_accept_keeps_epathmap_shaped(before, after),
    }
}

/// The Live arm of the dispatcher at RootCompleteNoScore: the accepted state
/// names the product the step completed, and the step keeps it unchanged.
pub proof fn integrated_noscore_root_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: dispatcher::IntegratedState,
    returned: runtime_source::RuntimeNoScoreRootReturnImage,
    route: dispatcher::IntegratedPushRoute,
)
    requires
        live_epathmap_score_nodes_shaped(before),
        dispatcher::integrated_live_case(
            before, after, LiveRuntimeProductAction::RootCompleteNoScore { returned }, route,
        ),
    ensures integrated_epathmap_score_nodes_shaped(after),
{
    if let dispatcher::IntegratedState::Accepted {
        accepted: dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. },
    } = after {
        live_root_noscore_keeps_epathmap_shaped(before, pre_complete, returned);
    }
}

/// The Live arm of the dispatcher: RootComplete accepts its post-score
/// product, RootCompleteNoScore accepts the product it completed, and every
/// other Live action is a `live_runtime_product_step`.
pub proof fn integrated_live_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: dispatcher::IntegratedState,
    action: LiveRuntimeProductAction,
    route: dispatcher::IntegratedPushRoute,
)
    requires
        live_epathmap_score_nodes_shaped(before),
        dispatcher::integrated_live_case(before, after, action, route),
    ensures integrated_epathmap_score_nodes_shaped(after),
{
    match (action, after) {
        (
            LiveRuntimeProductAction::RootComplete { .. },
            dispatcher::IntegratedState::Accepted {
                accepted: dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. },
            },
        ) => live_runtime_step_keeps_epathmap_shaped(before, post_root_score, action),
        (_, dispatcher::IntegratedState::Active { product: next }) => {
            dispatcher::integrated_live_relation_refines_live_step(before, next, action, route);
            live_runtime_step_keeps_epathmap_shaped(before, next, action);
        },
        (LiveRuntimeProductAction::RootCompleteNoScore { returned }, _) =>
            integrated_noscore_root_keeps_epathmap_shaped(before, after, returned, route),
        _ => {},
    }
}

/// The CanonicalPath arm: a Push makes the slotless kind-4 top wait and
/// pushes a fresh child, a child return drops the child and resolves its
/// kind-4 parent, both under the same transcript, and the root Complete
/// accepts the prestate itself.
pub proof fn integrated_path_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: dispatcher::IntegratedState,
    driver_before: path_driver::CanonicalPathDriverProductState,
    action: path_driver::CanonicalPathDriverSourceAction,
)
    requires
        live_epathmap_score_nodes_shaped(before),
        dispatcher::integrated_path_case(before, after, driver_before, action),
    ensures integrated_epathmap_score_nodes_shaped(after),
{
    let common_before = driver_before.common;
    let transcript = before.source.product.bridge.transcript;
    let admission_before = before.source.product.bridge.admission;
    assert(common_before.live.base == before);
    match path_driver::canonical_path_driver_product_step(driver_before, action).unwrap() {
        path_driver::CanonicalPathDriverProductOutcome::Active { state } => {
            assert(after == dispatcher::IntegratedState::Active { product: state.product });
            match action {
                path_driver::CanonicalPathDriverSourceAction::Push {
                    storage_observed, source_before, loop_events, source_after_scan,
                    source, child, storage, child_image, child_reader,
                } => {
                    let selected = common::canonical_path_common_push_macro_from_loop_source(
                        common_before, storage_observed.step_index, source_before,
                        loop_events, source_after_scan, source, child, storage,
                        child_image, child_reader,
                    ).unwrap();
                    assert(state.product == selected.live.base);
                    let scanned = common::canonical_path_loop_source_fold(
                        common_before, loop_events,
                    ).unwrap();
                    assert(scanned.live == common_before.live);
                    assert(common::canonical_path_common_push_from_source(
                        scanned, source, child, storage, child_image, child_reader,
                    ) == Some(selected));
                    assert(path_live::live_canonical_path_push_from_source(
                        scanned.live, source, child, storage, child_image, child_reader,
                    ) == Some(selected.live));
                    assert(selected.live.base.source.product.bridge.admission
                        == admission::admission_canonical_path_push_indexed_step(
                            admission_before, source, child, storage,
                        ).unwrap());
                    assert(selected.live.base.source.product.bridge.transcript == transcript);
                    path_push_keeps_epathmap_shaped(transcript, admission_before);
                },
                path_driver::CanonicalPathDriverSourceAction::ChildReturn { observed, .. } => {
                    let selected = common::canonical_path_common_return_from_source(
                        common_before, observed,
                    ).unwrap();
                    assert(state.product == selected.live.base);
                    let result = path_live::live_canonical_path_return_from_source(
                        common_before.live, observed,
                    ).unwrap();
                    assert(selected.live == result.state);
                    let source = path_live::live_canonical_path_retained_return_source(
                        common_before.live, observed,
                    );
                    assert(result.state.base.source.product.bridge.admission
                        == admission::admission_canonical_path_return_indexed_step(
                            admission_before, source, observed.summary,
                        ).unwrap());
                    assert(result.state.base.source.product.bridge.transcript == transcript);
                    path_return_keeps_epathmap_shaped(transcript, admission_before);
                },
                path_driver::CanonicalPathDriverSourceAction::RootComplete { .. } => {},
            }
        },
        path_driver::CanonicalPathDriverProductOutcome::Terminal { .. } => {},
    }
}

/// The ACT Forward arm: the batch advances the EPathMap top's byte cursor
/// under the same transcript.
pub proof fn act_forward_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    width: usize,
    commit: act_source::AcceptedLineCommitSource,
    pending: epm::PathObligation,
    state_id: usize,
)
    requires
        before.source.product.bridge.admission.frames.len() > 0,
        live_epathmap_score_nodes_shaped(before),
    ensures
        live_epathmap_score_nodes_shaped(
            dispatcher::integrated_act_forward_after(before, width, commit, pending, state_id),
        ),
{
    let product = before.source.product;
    let frame = dispatcher::integrated_act_forward_frame(
        product, width, commit, pending, state_id,
    );
    let consumed = frame.cursor.value_table_offset;
    let after = dispatcher::integrated_act_forward_after(before, width, commit, pending, state_id);
    assert(after.source.product == dispatcher::integrated_act_forward_product(product, frame));
    assert(after.source.product.bridge.admission
        == dispatcher::integrated_epm_cursor_admission(product.bridge.admission, consumed));
    assert(after.source.product.bridge.transcript == product.bridge.transcript);
    epathmap_cursor_advanced_shaped(
        product.bridge.transcript, product.bridge.admission.frames, consumed,
    );
}

/// The ACT Unwind arm: the batch of `width` events leaves the admission state
/// or advances the EPathMap top's byte cursor, under the same transcript.
pub proof fn act_unwind_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires
        dispatcher::integrated_act_unwind_guard(before, run, prefix, start, width, facts),
        live_epathmap_score_nodes_shaped(before),
    ensures
        live_epathmap_score_nodes_shaped(
            dispatcher::integrated_act_unwind_after(before, run, prefix, width),
        ),
{
    let product = before.source.product;
    let source = dispatcher::integrated_act_unwind_source(before);
    let transcript = product.bridge.transcript;
    unwind_product::unwind_product_virtual_matches_linked_source(
        product, source, run, prefix, start, width, facts, width,
    );
    let view = unwind_product::unwind_product_virtual_state(product, source, run, prefix, width);
    let after = dispatcher::integrated_act_unwind_after(before, run, prefix, width);
    assert(after.source.product == view);
    assert(source.admission == product.bridge.admission);
    assert(view.bridge.admission == unwind::linked_unwind_virtual_admission(source, width));
    assert(view.bridge.transcript == transcript);
    if width > 0 {
        assert(product.bridge.admission.frames.len() > 0);
        epathmap_cursor_advanced_shaped(
            transcript, product.bridge.admission.frames, source.epm.cursor.value_table_offset,
        );
    }
}

/// The EPathMap obligation returns pop the returning child and resolve its
/// EPathMap parent under the same transcript.
pub proof fn obligation_return_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        obligation_return::epm_obligation_return_effects(before, after),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    let transcript = before.source.product.bridge.transcript;
    let frames = before.source.product.bridge.admission.frames;
    let count = frames.len();
    let popped = after.source.product.bridge.admission.frames;
    let resolved = epm::epm_parent_resolved(frames[count - 2]);
    epm_resolved_parent_keeps_epathmap_shaped(transcript, frames);
    assert(popped.last() == resolved);
    assert(popped =~= frames.subrange(0, count - 1).update(count - 2, resolved));
}

/// The EPathMap handoff keeps the bridge.
pub proof fn handoff_keeps_epathmap_shaped(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires
        handoff::live_runtime_epm_handoff_step(before, after, backing, plan, validated),
        live_epathmap_score_nodes_shaped(before),
    ensures live_epathmap_score_nodes_shaped(after),
{
    reveal(handoff::live_runtime_epm_handoff_step);
    assert(score_bridge::bridge_same(before.source.product.bridge, after.source.product.bridge));
}

/// Every action of the integrated dispatcher keeps the invariant: one lemma
/// per action family.
pub proof fn integrated_step_keeps_epathmap_shaped(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epathmap_score_nodes_shaped(before),
        dispatcher::integrated_step(before, after, action),
    ensures integrated_epathmap_score_nodes_shaped(after),
{
    match before {
        dispatcher::IntegratedState::Accepted { .. } => {},
        dispatcher::IntegratedState::Active { product } => match action {
            dispatcher::IntegratedAction::Live { live_action, route } =>
                integrated_live_keeps_epathmap_shaped(product, after, live_action, route),
            dispatcher::IntegratedAction::CanonicalPath { driver_before, driver_action } =>
                integrated_path_keeps_epathmap_shaped(product, after, driver_before, driver_action),
            dispatcher::IntegratedAction::ActForward {
                width, commit, pending, state_id, ..
            } => act_forward_keeps_epathmap_shaped(product, width, commit, pending, state_id),
            dispatcher::IntegratedAction::ActUnwind {
                run, prefix, start, width, unwind_facts,
            } => act_unwind_keeps_epathmap_shaped(
                product, run, prefix, start, width, unwind_facts,
            ),
            dispatcher::IntegratedAction::EpmRawParReturn => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_raw_par_return_has_effects(product, next);
                    obligation_return_keeps_epathmap_shaped(product, next);
                }
            },
            dispatcher::IntegratedAction::EpmNestedReturn => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    obligation_return::live_runtime_epm_nested_return_has_effects(product, next);
                    obligation_return_keeps_epathmap_shaped(product, next);
                }
            },
            dispatcher::IntegratedAction::EpmHandoff { backing, plan, validated } => {
                if let dispatcher::IntegratedState::Active { product: next } = after {
                    handoff_keeps_epathmap_shaped(product, next, backing, plan, validated);
                }
            },
        },
    }
}

/// Every state of an integrated trace whose first state satisfies the
/// invariant satisfies it.
pub proof fn integrated_trace_keeps_epathmap_shaped(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    index: int,
)
    requires
        dispatcher::integrated_trace(states, actions),
        integrated_epathmap_score_nodes_shaped(states[0]),
        0 <= index < states.len(),
    ensures integrated_epathmap_score_nodes_shaped(states[index]),
    decreases index,
{
    if index > 0 {
        let previous = index - 1;
        integrated_trace_keeps_epathmap_shaped(states, actions, previous);
        assert(0 <= previous < actions.len());
        assert(dispatcher::integrated_step(
            states[previous], states[previous + 1], actions[previous],
        ));
        assert(previous + 1 == index);
        integrated_step_keeps_epathmap_shaped(
            states[previous], states[previous + 1], actions[previous],
        );
    }
}

/// An active state whose admission frames hold no returns and no scores
/// satisfies the invariant under any transcript; the admission root of
/// `admission_initial_state` has this shape.
pub proof fn unscored_start_is_shaped(product: LiveRuntimeProductState)
    requires
        forall|i: int| 0 <= i < product.source.product.bridge.admission.frames.len() ==> {
            &&& (#[trigger] product.source.product.bridge.admission.frames[i]).returns.len() == 0
            &&& product.source.product.bridge.admission.frames[i].score_handle.is_none()
        },
    ensures
        integrated_epathmap_score_nodes_shaped(dispatcher::IntegratedState::Active { product }),
{
    let bridge = product.source.product.bridge;
    assert forall|i: int| 0 <= i < bridge.admission.frames.len() implies
        #[trigger] epathmap_frame_shaped(bridge.transcript, bridge.admission.frames[i]) by {
        epathmap_fresh_frame_shaped(bridge.transcript, bridge.admission.frames[i]);
    }
}

/// Completeness on every state of an integrated trace whose first state
/// satisfies the invariant and reads the connective flag of every EZipper
/// (`ezipper_flags::integrated_ezipper_flags_readable`): on an active state
/// whose top is an EZipper at EOF with its PathMap return present, the EZipper
/// score step is the selected finish when the transcript has room for four
/// more entries, and every finish candidate is None otherwise. The connective flag
/// of that top is readable by `ezipper_flags::reachable_ezipper_flag_slot_is_readable`,
/// and room is decided, not assumed.
pub proof fn reachable_ezipper_eof_has_score_step(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    index: int,
)
    requires
        dispatcher::integrated_trace(states, actions),
        integrated_epathmap_score_nodes_shaped(states[0]),
        ezipper_flags::integrated_ezipper_flags_readable(states[0]),
        0 <= index < states.len(),
    ensures
        match states[index] {
            dispatcher::IntegratedState::Active { product } => {
                let bridge = product.source.product.bridge;
                bridge.admission.frames.len() > 0
                    && epathmap_ezipper_eof_present(bridge.admission.frames.last())
                    ==> epathmap_ezipper_score_dichotomy(bridge)
            },
            dispatcher::IntegratedState::Accepted { .. } => true,
        },
{
    integrated_trace_keeps_epathmap_shaped(states, actions, index);
    ezipper_flags::reachable_ezipper_flag_slot_is_readable(states, actions, index);
    dispatcher::integrated_trace_wf(states, actions, index);
    if let dispatcher::IntegratedState::Active { product } = states[index] {
        let bridge = product.source.product.bridge;
        if bridge.admission.frames.len() > 0
            && epathmap_ezipper_eof_present(bridge.admission.frames.last())
        {
            assert(integrated::product_score_wf(product.source.product));
            assert(admission::admission_state_wf(bridge.admission));
            assert(ezipper_flags::ezipper_frame(bridge.admission.frames.last()));
            ezipper_eof_score_step_dichotomy(bridge);
        }
    }
}

}
