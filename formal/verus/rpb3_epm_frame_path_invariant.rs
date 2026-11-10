//! Path invariant of an EPathMap frame (`rpb3_epathmap_event_refinement`) and
//! its preservation by every EPathMap event and child return.
//!
//! `epm_frame_path_wf` ties the frame's path topology to its CanonicalPath
//! parser image (`rpb3_path_parser_refinement`). Its four named clauses are:
//! - (a) `epm_path_escape_wf`: `parser_image_escape_wf` holds for the parser and
//!   for the parser of every restore checkpoint;
//! - (b) `epm_pending_matches_focus`: a pending obligation is the focused top
//!   frame, an Obligation frame with the same id, a matching kind, the same
//!   payload length and payload start, and AwaitingResolution exactly when the
//!   obligation awaits resolution; with no pending obligation, a live parser
//!   has no Obligation frame on the focused top;
//! - (c) `epm_wait_awaits_pending`: an Obligation wait has a pending obligation
//!   with the waited id and kind that awaits resolution, and the path gate is
//!   not dead;
//! - (d) `epm_path_length_bounds`: the root potential, the root's completed
//!   segment count plus one when the root stack is non-empty, is at most the
//!   path length, which is at most the cursor position. This bounds the root
//!   count by `usize::MAX` without a crate-wide `global size_of usize`.
//!
//! The four clauses alone are not preserved by the events, so the invariant
//! adds the facts that make it inductive:
//! - `parser_image_obligation_wf`: Obligation frames sit only on the focused
//!   top, and every Region owner's parent has a non-empty stack with no
//!   Obligation frame. Without it a byte that completes the frame above an
//!   interior Obligation frame exposes that frame with no pending obligation.
//! - `epm_pending_awaiting_is_waited`: an obligation that awaits resolution is
//!   waited on. Without it a Plain byte on an awaiting obligation kills the
//!   path while the edge return still opens an Obligation wait.
//! - `epm_restore_wf`: checkpoint `i` was pushed at path length `i`, there is
//!   one checkpoint per path byte, and each checkpoint's parser satisfies
//!   (b), the obligation layout and the potential bound for its length, with a
//!   pending obligation that does not await resolution. Leave restores it.
//! - `parser_gate_matches` between the path gate and the parser: (c) speaks
//!   of the path gate, and `epm_obligation_before_accept` of the parser.
//!
//! Clause (b) also matches the payload start, which decides when a payload
//! byte is the last one, and its pending-free direction is stated for a live
//! parser only: a rejected child resolution kills the path but leaves the
//! killed Obligation frame on the focused top (`resolve_leaf_obligation`
//! kills the path; `active_obligation` is None on a dead path).
//!
//! The parser lemmas follow one parser function each: `bubble_parser`,
//! `settle_parser_owners`, `feed_parser_tag`, `feed_parser_bytes`,
//! `finish_parser_varint`, `feed_parser_focus`, `feed_parser_byte` and
//! `resolve_parser_obligation`. Each keeps the obligation layout and bounds
//! the root potential: a byte adds at most one, and a resolution adds none.
//! The frame lemmas cover `initial_epm_candidate`, `event_step` for each
//! edge effect, Leave, Value and End, and both arms of `accept_child`.
//! `epm_frame_path_wf_discharges_return_premises` derives from the invariant
//! the premises `epm_obligation_before_accept` and
//! `epm_obligation_parser_ready` of a waiting frame.

use vstd::prelude::*;

use crate::{
    rpb3_epathmap_event_refinement as epm,
    rpb3_epm_obligation_return_live as obligation_return,
    rpb3_epm_obligation_return_source as return_source,
    rpb3_epm_parser_escape_invariant as escape,
    rpb3_path_parser_refinement as parser,
};

verus! {

/// A parser stack with no Obligation frame.
pub open spec fn parser_stack_obligation_free(stack: Seq<parser::ParserFrame>) -> bool {
    forall|i: int| 0 <= i < stack.len() ==> !((#[trigger] stack[i]) is Obligation)
}

/// The focused top frame of `path` is an Obligation frame.
pub open spec fn parser_top_is_obligation(path: parser::ParserPath) -> bool {
    path.stack.len() > 0 && (path.stack.last() is Obligation)
}

/// No Obligation frame sits below the top of `path`.
pub open spec fn parser_obligation_on_top_only(path: parser::ParserPath) -> bool {
    forall|i: int| 0 <= i < path.stack.len() - 1 ==> !((#[trigger] path.stack[i]) is Obligation)
}

/// A Region owner's parent keeps its RegionActive top, so its stack is
/// non-empty, and it holds no Obligation frame.
pub open spec fn parser_owner_parent_wf(owner: parser::ParserRegionOwner) -> bool {
    &&& owner.parent.stack.len() > 0
    &&& parser_stack_obligation_free(owner.parent.stack)
}

/// Obligation frames sit only on the focused top.
pub open spec fn parser_image_obligation_wf(image: parser::ParserImage) -> bool {
    &&& parser_obligation_on_top_only(image.focus)
    &&& forall|k: int| 0 <= k < image.owners.len() ==> parser_owner_parent_wf(#[trigger] image.owners[k])
}

/// The completed segment count of `path`, plus one when its stack is non-empty.
pub open spec fn parser_path_potential(path: parser::ParserPath) -> int {
    path.completed_segments as int + (if path.stack.len() > 0 { 1int } else { 0int })
}

/// The potential of the root path, which `parser_root_gate` reports.
pub open spec fn parser_root_potential(image: parser::ParserImage) -> int {
    parser_path_potential(parser::parser_root(image))
}

/// The focused top after `ObligationStarted { id, kind, payload_len }` at
/// `path_len`: the new Obligation frame, awaiting resolution exactly when its
/// payload is empty.
pub open spec fn parser_started_top(
    path: parser::ParserPath,
    id: u64,
    kind: parser::ParserObligationKind,
    payload_len: u64,
    path_len: usize,
) -> bool {
    &&& path.stack.len() > 0
    &&& match path.stack.last() {
        parser::ParserFrame::Obligation {
            id: top_id,
            kind: top_kind,
            payload_len: top_len,
            payload_start,
            status,
            ..
        } => {
            &&& top_id == id
            &&& top_kind == kind
            &&& top_len == payload_len
            &&& payload_start == path_len
            &&& (status is AwaitingResolution) == (payload_len == 0)
        },
        _ => false,
    }
}

/// The focused tops around `ObligationByte { id, last }` at `path_len`: the
/// same Obligation frame, in Payload before the byte, and awaiting resolution
/// after it exactly when the byte is the last payload byte.
pub open spec fn parser_payload_byte_tops(
    before: parser::ParserPath,
    after: parser::ParserPath,
    id: u64,
    last: bool,
    path_len: usize,
) -> bool {
    &&& before.stack.len() > 0
    &&& after.stack.len() > 0
    &&& match (before.stack.last(), after.stack.last()) {
        (
            parser::ParserFrame::Obligation {
                id: before_id,
                kind: before_kind,
                payload_len: before_len,
                payload_start: before_start,
                status: parser::ParserObligationStatus::Payload,
                ..
            },
            parser::ParserFrame::Obligation {
                id: after_id,
                kind: after_kind,
                payload_len: after_len,
                payload_start: after_start,
                status: after_status,
                ..
            },
        ) => {
            &&& before_id == id
            &&& after_id == id
            &&& after_kind == before_kind
            &&& after_len == before_len
            &&& after_start == before_start
            &&& (after_status is AwaitingResolution) == last
            &&& last == (path_len as int - before_start as int == before_len as int)
        },
        _ => false,
    }
}

/// The focused tops that an obligation event reports.
pub open spec fn parser_event_tops(
    before: parser::ParserPath,
    after: parser::ParserPath,
    event: parser::ParserEvent,
    path_len: usize,
) -> bool {
    match event {
        parser::ParserEvent::ObligationStarted { id, kind, payload_len } => {
            parser_started_top(after, id, kind, payload_len, path_len)
        },
        parser::ParserEvent::ObligationByte { id, last } => {
            parser_payload_byte_tops(before, after, id, last, path_len)
        },
        _ => true,
    }
}

/// A Continue byte keeps an Obligation frame on the focused top, and adds
/// none where there was none.
pub open spec fn parser_continue_keeps_top(
    before: parser::ParserPath,
    after: parser::ParserPath,
) -> bool {
    &&& parser_top_is_obligation(before) ==> after.stack.len() > 0
        && after.stack.last() == before.stack.last()
    &&& !parser_top_is_obligation(before) ==> !parser_top_is_obligation(after)
}

/// What one `feed_parser_focus` step keeps: the obligation layout, the first
/// owner (a new owner is pushed after the existing ones), the potential of the
/// focused path, the focused top of a Continue byte, the owners of an
/// obligation event, and the tops that an obligation event reports.
pub open spec fn parser_focus_step_frames_hold(
    selected: parser::ParserImage,
    step: parser::ParserStep,
    path_len: usize,
) -> bool {
    &&& parser_image_obligation_wf(step.image)
    &&& selected.owners.len() > 0 ==> step.image.owners.len() > 0
        && step.image.owners[0] == selected.owners[0]
    &&& step.image.owners.len() == selected.owners.len() ==> parser_path_potential(step.image.focus)
        <= parser_path_potential(selected.focus) + 1
    &&& selected.owners.len() == 0 && step.image.owners.len() > 0
        ==> step.image.owners[0].parent.completed_segments as int
        <= parser_path_potential(selected.focus)
    &&& (step.event is Continue) && parser_top_is_obligation(selected.focus)
        ==> step.image.focus.stack == selected.focus.stack
        && step.image.focus.status == parser::ParserStatus::Dead
    &&& (step.event is Continue) && !parser_top_is_obligation(selected.focus)
        ==> parser_stack_obligation_free(step.image.focus.stack)
    &&& !(step.event is Continue) ==> step.image.owners == selected.owners
    &&& parser_event_tops(selected.focus, step.image.focus, step.event, path_len)
}

/// Clause (a): the escape invariant of the parser and of every checkpoint.
pub open spec fn epm_path_escape_wf(path: epm::PathTopology) -> bool {
    &&& escape::parser_image_escape_wf(path.parser)
    &&& forall|i: int| 0 <= i < path.restore.len()
        ==> escape::parser_image_escape_wf((#[trigger] path.restore[i]).parser)
}

/// Clause (b): a pending obligation is the focused top Obligation frame; with
/// none, a live parser has no Obligation frame on the focused top.
pub open spec fn epm_pending_matches_focus(
    pending: Option<epm::PathObligation>,
    image: parser::ParserImage,
) -> bool {
    match pending {
        Some(obligation) => {
            &&& image.focus.stack.len() > 0
            &&& match image.focus.stack.last() {
                parser::ParserFrame::Obligation { id, kind, payload_len, payload_start, status, .. } => {
                    &&& id == obligation.id
                    &&& epm::parser_kind_matches(obligation.kind, kind)
                    &&& payload_len as int == obligation.payload_len as int
                    &&& payload_start == obligation.payload_start
                    &&& (status is AwaitingResolution) == obligation.awaiting_resolution
                },
                _ => false,
            }
        },
        None => !parser::parser_root_gate(image).dead ==> !parser_top_is_obligation(image.focus),
    }
}

/// Clause (c): an Obligation wait has the waited obligation pending, awaiting
/// resolution, on a path whose gate is not dead.
pub open spec fn epm_wait_awaits_pending(frame: epm::EpmFrame) -> bool {
    match frame.wait {
        Some(epm::EpmWait::Obligation { id, kind, .. }) => {
            &&& !frame.path.gate.dead
            &&& match frame.path.pending {
                Some(obligation) => {
                    &&& obligation.awaiting_resolution
                    &&& obligation.id == id
                    &&& obligation.kind == kind
                },
                None => false,
            }
        },
        _ => true,
    }
}

/// Clause (d): root potential <= path length <= cursor position.
pub open spec fn epm_path_length_bounds(frame: epm::EpmFrame) -> bool {
    &&& parser_root_potential(frame.path.parser) <= frame.path.path_bytes.len()
    &&& frame.path.path_bytes.len() <= frame.cursor.at as int
}

/// The frame waits on an obligation.
pub open spec fn epm_waits_on_obligation(frame: epm::EpmFrame) -> bool {
    match frame.wait {
        Some(epm::EpmWait::Obligation { .. }) => true,
        _ => false,
    }
}

/// A pending obligation that awaits resolution is waited on.
pub open spec fn epm_pending_awaiting_is_waited(frame: epm::EpmFrame) -> bool {
    match frame.path.pending {
        Some(obligation) => obligation.awaiting_resolution ==> epm_waits_on_obligation(frame),
        None => true,
    }
}

/// Restore checkpoint `index`: pushed at path length `index`, with a parser
/// that satisfies the obligation layout, clause (b) for its pending obligation
/// and the potential bound for its length, and a pending obligation that does
/// not await resolution.
pub open spec fn epm_checkpoint_wf(checkpoint: epm::PathCheckpoint, index: int) -> bool {
    &&& checkpoint.path_len as int == index
    &&& parser_image_obligation_wf(checkpoint.parser)
    &&& epm_pending_matches_focus(checkpoint.pending, checkpoint.parser)
    &&& match checkpoint.pending {
        Some(obligation) => !obligation.awaiting_resolution,
        None => true,
    }
    &&& parser_root_potential(checkpoint.parser) <= index
}

/// One well-formed checkpoint per path byte.
pub open spec fn epm_restore_wf(path: epm::PathTopology) -> bool {
    &&& path.restore.len() == path.path_bytes.len()
    &&& forall|i: int| 0 <= i < path.restore.len() ==> epm_checkpoint_wf(#[trigger] path.restore[i], i)
}

/// The EPathMap frame path invariant: clauses (a) to (d) and the facts that
/// make them inductive.
pub open spec fn epm_frame_path_wf(frame: epm::EpmFrame) -> bool {
    &&& epm_path_escape_wf(frame.path)
    &&& epm_pending_matches_focus(frame.path.pending, frame.path.parser)
    &&& epm_wait_awaits_pending(frame)
    &&& epm_path_length_bounds(frame)
    &&& parser_image_obligation_wf(frame.path.parser)
    &&& epm_pending_awaiting_is_waited(frame)
    &&& epm_restore_wf(frame.path)
    &&& epm::parser_gate_matches(frame.path.gate, frame.path.parser)
}

/// With no Obligation frame below the top or on it, the stack has none.
pub proof fn parser_top_only_obligation_free(path: parser::ParserPath)
    requires
        parser_obligation_on_top_only(path),
        !parser_top_is_obligation(path),
    ensures
        parser_stack_obligation_free(path.stack),
{
    assert forall|i: int| 0 <= i < path.stack.len()
        implies !((#[trigger] path.stack[i]) is Obligation) by {
        if i == path.stack.len() - 1 {
            assert(path.stack[i] == path.stack.last());
        }
    }
}

/// Popping the top leaves no Obligation frame.
pub proof fn parser_pop_top_frames(path: parser::ParserPath)
    requires
        parser_obligation_on_top_only(path),
        path.stack.len() > 0,
    ensures
        parser_stack_obligation_free(parser::pop_parser_top(path).stack),
        parser::pop_parser_top(path).completed_segments == path.completed_segments,
{
    let popped = parser::pop_parser_top(path);
    assert forall|i: int| 0 <= i < popped.stack.len()
        implies #[trigger] popped.stack[i] == path.stack[i] by {}
}

/// Replacing the top keeps the frames below it, so it keeps the obligation
/// layout, and a replacement that is not an Obligation frame leaves none.
pub proof fn parser_replace_top_frames(path: parser::ParserPath)
    requires
        parser_obligation_on_top_only(path),
        path.stack.len() > 0,
    ensures
        forall|frame: parser::ParserFrame| {
            &&& parser_obligation_on_top_only(#[trigger] parser::replace_parser_top(path, frame))
            &&& parser::replace_parser_top(path, frame).stack.len() == path.stack.len()
            &&& parser::replace_parser_top(path, frame).stack.last() == frame
            &&& parser::replace_parser_top(path, frame).completed_segments
                == path.completed_segments
            &&& !(frame is Obligation) ==> parser_stack_obligation_free(
                parser::replace_parser_top(path, frame).stack,
            )
        },
{
    assert forall|frame: parser::ParserFrame| {
        &&& parser_obligation_on_top_only(#[trigger] parser::replace_parser_top(path, frame))
        &&& parser::replace_parser_top(path, frame).stack.len() == path.stack.len()
        &&& parser::replace_parser_top(path, frame).stack.last() == frame
        &&& parser::replace_parser_top(path, frame).completed_segments == path.completed_segments
        &&& !(frame is Obligation) ==> parser_stack_obligation_free(
            parser::replace_parser_top(path, frame).stack,
        )
    } by {
        let replaced = parser::replace_parser_top(path, frame).stack;
        assert forall|i: int| 0 <= i < replaced.len() - 1
            implies #[trigger] replaced[i] == path.stack[i] by {}
        assert(replaced.last() == frame);
        if !(frame is Obligation) {
            assert forall|i: int| 0 <= i < replaced.len()
                implies !((#[trigger] replaced[i]) is Obligation) by {
                if i < replaced.len() - 1 {
                    assert(replaced[i] == path.stack[i]);
                }
            }
        }
    }
}

/// `bubble_parser` pops finished Kids frames and opens the next List kid with
/// a Tag frame, so it adds no Obligation frame. It completes at most one
/// segment, and only by emptying the stack.
pub proof fn parser_bubble_frames(path: parser::ParserPath, segment_stable: bool)
    requires
        parser_stack_obligation_free(path.stack),
    ensures
        parser_stack_obligation_free(parser::bubble_parser(path, segment_stable).stack),
        parser_path_potential(parser::bubble_parser(path, segment_stable))
            <= path.completed_segments as int + 1,
    decreases path.stack.len(),
{
    if path.stack.len() > 0 {
        match path.stack.last() {
            parser::ParserFrame::Kids { remaining } => {
                let rest = path.stack.drop_last();
                assert forall|i: int| 0 <= i < rest.len()
                    implies #[trigger] rest[i] == path.stack[i] by {}
                if remaining == 1 {
                    let next = parser::ParserPath { stack: rest, ..path };
                    assert(parser_stack_obligation_free(next.stack));
                    parser_bubble_frames(next, segment_stable);
                } else {
                    let pushed = rest.push(parser::ParserFrame::Kids { remaining: (remaining - 1) as u64 })
                        .push(parser::ParserFrame::Tag { escape_allowed: false });
                    assert forall|i: int| 0 <= i < pushed.len()
                        implies !((#[trigger] pushed[i]) is Obligation) by {
                        if i < rest.len() {
                            assert(pushed[i] == rest[i]);
                        }
                    }
                }
            },
            _ => {},
        }
    }
}

/// `settle_parser_owners` keeps the obligation layout and bounds the root
/// potential by the first owner's parent. A focused top that is not an
/// Obligation frame stays so; a focused Obligation frame is kept unless the
/// focus dies; no owner or a dead focus leaves the image unchanged.
pub proof fn parser_settle_frames(image: parser::ParserImage)
    requires
        parser_image_obligation_wf(image),
    ensures
        parser_image_obligation_wf(parser::settle_parser_owners(image)),
        image.owners.len() > 0 ==> parser_root_potential(parser::settle_parser_owners(image))
            <= image.owners[0].parent.completed_segments as int + 1,
        !parser_top_is_obligation(image.focus)
            ==> !parser_top_is_obligation(parser::settle_parser_owners(image).focus),
        parser_top_is_obligation(image.focus) ==> parser::settle_parser_owners(image) == image
            || parser::settle_parser_owners(image).focus.status == parser::ParserStatus::Dead,
        image.owners.len() == 0 ==> parser::settle_parser_owners(image) == image,
        image.focus.status == parser::ParserStatus::Dead
            ==> parser::settle_parser_owners(image) == image,
    decreases image.owners.len(),
{
    hide(parser::parser_bytes_less);
    hide(parser::bubble_parser);
    if image.owners.len() > 0 {
        let last = image.owners.len() - 1;
        let owner = image.owners.last();
        assert(owner == image.owners[last]);
        assert(parser_owner_parent_wf(owner));
        if image.focus.status != parser::ParserStatus::Dead
            && owner.path_remaining == 0
            && !parser::parser_waiting_obligation(image.focus)
        {
            parser_pop_top_frames(owner.parent);
            parser_replace_top_frames(owner.parent);
            let rest = parser::pop_parser_top(owner.parent);
            parser_bubble_frames(rest, true);
            let parent = if image.focus.status == parser::ParserStatus::Dead
                || !parser::parser_valid_finished_path(image.focus)
                || !image.focus.stable
                || owner.child_bytes.len() != owner.declared_len as int
                || match owner.previous {
                    Some(previous) => !parser::parser_bytes_less(previous, owner.child_bytes),
                    None => false,
                } {
                parser::parser_dead(owner.parent)
            } else if owner.remaining == 0 {
                parser::bubble_parser(parser::pop_parser_top(owner.parent), true)
            } else {
                parser::replace_parser_top(
                    owner.parent,
                    parser::ParserFrame::RegionLength {
                        remaining: owner.remaining,
                        previous: Some(owner.child_bytes),
                        state: parser::initial_varint(),
                    },
                )
            };
            assert(parser_stack_obligation_free(parent.stack));
            assert(parser_path_potential(parent) <= owner.parent.completed_segments as int + 1);
            if parser_top_is_obligation(image.focus) {
                assert(!parser::parser_valid_finished_path(image.focus));
                assert(parent.status == parser::ParserStatus::Dead);
            }
            let next = parser::ParserImage {
                focus: parent,
                owners: image.owners.drop_last(),
                ..image
            };
            assert forall|k: int| 0 <= k < next.owners.len()
                implies #[trigger] next.owners[k] == image.owners[k] by {}
            assert(parser_image_obligation_wf(next));
            parser_settle_frames(next);
            assert(parser::settle_parser_owners(image) == parser::settle_parser_owners(next));
            if last == 0 {
                assert(owner == image.owners[0]);
                assert(parser::settle_parser_owners(next) == next);
            } else {
                assert(next.owners[0] == image.owners[0]);
            }
        }
    }
}

/// `feed_parser_tag` on an Obligation-free stack: the new frames are Varint,
/// Bytes, Kids and Tag frames, and a completed scalar bubbles.
pub proof fn parser_tag_step_frames(
    path: parser::ParserPath,
    byte: u8,
    source: parser::ParserSource,
    escape_allowed: bool,
)
    requires
        path.stack.len() > 0,
        parser_stack_obligation_free(path.stack),
    ensures
        parser_stack_obligation_free(
            parser::feed_parser_tag(path, byte, source, escape_allowed).stack,
        ),
        parser_path_potential(parser::feed_parser_tag(path, byte, source, escape_allowed))
            <= path.completed_segments as int + 1,
{
    hide(parser::bubble_parser);
    parser_pop_top_frames(path);
    parser_bubble_frames(parser::pop_parser_top(path), true);
}

/// `feed_parser_bytes` on an Obligation-free stack.
pub proof fn parser_bytes_step_frames(
    path: parser::ParserPath,
    remaining: u64,
    utf8: Option<parser::ParserUtf8>,
    byte: u8,
)
    requires
        path.stack.len() > 0,
        parser_stack_obligation_free(path.stack),
    ensures
        parser_stack_obligation_free(
            parser::feed_parser_bytes(path, remaining, utf8, byte).stack,
        ),
        parser_path_potential(parser::feed_parser_bytes(path, remaining, utf8, byte))
            <= path.completed_segments as int + 1,
{
    hide(parser::feed_parser_utf8);
    hide(parser::bubble_parser);
    parser_pop_top_frames(path);
    parser_replace_top_frames(path);
    parser_bubble_frames(parser::pop_parser_top(path), true);
}

/// `finish_parser_varint` on an Obligation-free focus keeps the owners. An
/// obligation length pushes the new Obligation frame on the focused top and
/// reports it; every other role adds no Obligation frame.
pub proof fn parser_varint_finish_frames(
    image: parser::ParserImage,
    role: parser::ParserVarintRole,
    value: u64,
    source: parser::ParserSource,
    path_len: usize,
)
    requires
        image.focus.stack.len() > 0,
        parser_stack_obligation_free(image.focus.stack),
    ensures
        ({
            let step = parser::finish_parser_varint(image, role, value, source, path_len);
            &&& step.image.owners == image.owners
            &&& parser_obligation_on_top_only(step.image.focus)
            &&& parser_path_potential(step.image.focus) <= image.focus.completed_segments as int + 1
            &&& (step.event is Continue) ==> parser_stack_obligation_free(step.image.focus.stack)
            &&& !(step.event is ObligationByte)
            &&& parser_event_tops(image.focus, step.image.focus, step.event, path_len)
        }),
{
    hide(parser::bubble_parser);
    parser_pop_top_frames(image.focus);
    parser_bubble_frames(parser::pop_parser_top(image.focus), true);
}

/// One `feed_parser_focus` step keeps `parser_focus_step_frames_hold`.
pub proof fn parser_focus_step_frames(
    selected: parser::ParserImage,
    byte: u8,
    source: parser::ParserSource,
    path_len: usize,
)
    requires
        parser_image_obligation_wf(selected),
    ensures
        parser_focus_step_frames_hold(
            selected,
            parser::feed_parser_focus(selected, byte, source, path_len),
            path_len,
        ),
{
    hide(parser::feed_parser_varint);
    hide(parser::feed_parser_tag);
    hide(parser::feed_parser_bytes);
    hide(parser::finish_parser_varint);
    hide(parser::bubble_parser);
    let path = selected.focus;
    let step = parser::feed_parser_focus(selected, byte, source, path_len);
    if !parser_top_is_obligation(path) {
        parser_top_only_obligation_free(path);
    }
    if path.status == parser::ParserStatus::Open {
        let prepared = parser::prepare_parser_tag(path, byte, source);
        if prepared.status == parser::ParserStatus::Open {
            assert(prepared.stack.len() > 0);
            assert(prepared.completed_segments == path.completed_segments);
            assert(path.stack.len() > 0 ==> prepared == path);
            assert(parser_obligation_on_top_only(prepared));
            parser_replace_top_frames(prepared);
            let top = prepared.stack.last();
            if !(top is Obligation) {
                parser_top_only_obligation_free(prepared);
            }
            match top {
                parser::ParserFrame::Tag { escape_allowed } => {
                    parser_tag_step_frames(prepared, byte, source, escape_allowed);
                },
                parser::ParserFrame::Varint { role, state } => {
                    match parser::feed_parser_varint(state, byte) {
                        Some(parser::ParserVarintFeed::Complete { value }) => {
                            parser_varint_finish_frames(
                                parser::ParserImage { focus: prepared, ..selected },
                                role,
                                value,
                                source,
                                path_len,
                            );
                        },
                        _ => {},
                    }
                },
                parser::ParserFrame::Bytes { remaining, utf8 } => {
                    parser_bytes_step_frames(prepared, remaining, utf8, byte);
                },
                parser::ParserFrame::RegionLength { .. } => {
                    let parent = parser::replace_parser_top(
                        prepared,
                        parser::ParserFrame::RegionActive,
                    );
                    assert(parser_stack_obligation_free(parent.stack));
                    assert(parent.stack.len() > 0);
                    assert(parent.completed_segments == path.completed_segments);
                    assert forall|k: int| 0 <= k < step.image.owners.len()
                        implies parser_owner_parent_wf(#[trigger] step.image.owners[k]) by {
                        if k < selected.owners.len() {
                            assert(step.image.owners[k] == selected.owners[k]);
                        } else {
                            assert(step.image.owners[k].parent == parent);
                        }
                    }
                },
                _ => {},
            }
        }
    }
}

/// A reusable interior payload byte keeps the focused Obligation frame, in
/// Payload, and is not its last byte.
pub proof fn parser_reusable_payload_top(image: parser::ParserImage, path_len: usize)
    requires
        parser::reusable_parser_payload(image, path_len).is_some(),
    ensures
        parser_payload_byte_tops(
            image.focus,
            image.focus,
            parser::reusable_parser_payload(image, path_len).unwrap(),
            false,
            path_len,
        ),
{
}

/// `feed_parser_byte` keeps the obligation layout and raises the root
/// potential by at most one. A dead root leaves the image unchanged, a
/// Continue byte keeps the focused top's Obligation frame or adds none, and a
/// live image after an obligation event has the reported focused top.
pub proof fn parser_feed_byte_frames(
    image: parser::ParserImage,
    byte: u8,
    source: parser::ParserSource,
    path_len: usize,
)
    requires
        parser_image_obligation_wf(image),
    ensures
        ({
            let step = parser::feed_parser_byte(image, byte, source, path_len);
            &&& parser_image_obligation_wf(step.image)
            &&& parser_root_potential(step.image) <= parser_root_potential(image) + 1
            &&& parser::parser_root_gate(image).dead ==> step == parser::parser_continue(image)
            &&& (step.event is Continue) ==> parser_continue_keeps_top(image.focus, step.image.focus)
            &&& !parser::parser_root_gate(step.image).dead
                ==> parser_event_tops(image.focus, step.image.focus, step.event, path_len)
        }),
{
    hide(parser::feed_parser_focus);
    hide(parser::settle_parser_owners);
    hide(parser::reusable_parser_payload);
    hide(parser::parser_owner_has_room);
    let selected = parser::consume_parser_owner_byte(image, byte);
    assert forall|k: int| 0 <= k < selected.owners.len()
        implies #[trigger] selected.owners[k].parent == image.owners[k].parent by {}
    assert(selected.focus == image.focus);
    assert(selected.owners.len() == image.owners.len());
    assert(parser_image_obligation_wf(selected));
    if image.owners.len() > 0 {
        assert(selected.owners[0].parent == image.owners[0].parent);
        assert(parser_owner_parent_wf(image.owners[0]));
    }
    let step = parser::feed_parser_byte(image, byte, source, path_len);
    if !parser::parser_root_gate(image).dead {
        if parser::reusable_parser_payload(image, path_len).is_some() {
            parser_reusable_payload_top(image, path_len);
            assert(step.image.focus == image.focus);
        } else if parser::parser_owner_has_room(image) {
            let focused = parser::feed_parser_focus(selected, byte, source, path_len);
            parser_focus_step_frames(selected, byte, source, path_len);
            parser_settle_frames(focused.image);
            let settled = parser::settle_parser_owners(focused.image);
            if image.owners.len() == 0 {
                if focused.image.owners.len() == 0 {
                    assert(settled == focused.image);
                }
            } else {
                assert(focused.image.owners[0] == selected.owners[0]);
            }
            if (focused.event is Continue) && parser_top_is_obligation(image.focus) {
                assert(settled == focused.image);
            }
            if !(focused.event is Continue) && !parser::parser_root_gate(step.image).dead {
                assert(focused.image.owners == selected.owners);
                if !(focused.event is NoSuccessfulEdge) {
                    assert(parser_top_is_obligation(focused.image.focus));
                    assert(settled == focused.image);
                    assert(step.image.focus == focused.image.focus);
                }
            }
        }
    }
}

/// `resolve_parser_obligation` on a non-empty focused stack keeps the
/// obligation layout and does not raise the root potential. A live result has
/// no Obligation frame on the focused top: the resolved frame is popped, or
/// the path dies.
pub proof fn parser_resolve_frames(
    image: parser::ParserImage,
    id: u64,
    resolution: parser::ParserResolution,
)
    requires
        parser_image_obligation_wf(image),
        image.focus.stack.len() > 0,
    ensures
        ({
            let resolved = parser::resolve_parser_obligation(image, id, resolution);
            &&& parser_image_obligation_wf(resolved)
            &&& parser_root_potential(resolved) <= parser_root_potential(image)
            &&& !parser::parser_root_gate(resolved).dead
                ==> !parser_top_is_obligation(resolved.focus)
        }),
{
    hide(parser::settle_parser_owners);
    hide(parser::bubble_parser);
    let path = image.focus;
    parser_pop_top_frames(path);
    let rest = parser::pop_parser_top(path);
    parser_bubble_frames(rest, false);
    parser_bubble_frames(rest, true);
    let focus = match path.stack.last() {
        parser::ParserFrame::Obligation {
            id: active_id,
            kind,
            status: parser::ParserObligationStatus::AwaitingResolution { .. },
            ..
        } if active_id == id => {
            match (kind, resolution) {
                (
                    parser::ParserObligationKind::RawParEscape,
                    parser::ParserResolution::RawPar { canonical_path_stable: false },
                ) => parser::bubble_parser(parser::pop_parser_top(path), false),
                (
                    parser::ParserObligationKind::NestedEpm1,
                    parser::ParserResolution::NestedEpm1 { is_map: true, entries_stable: true },
                ) => parser::bubble_parser(parser::pop_parser_top(path), true),
                _ => parser::parser_dead(path),
            }
        },
        _ => parser::parser_dead(path),
    };
    assert(focus == parser::parser_dead(path) || parser_stack_obligation_free(focus.stack));
    assert(parser_obligation_on_top_only(focus));
    assert(parser_path_potential(focus) <= parser_path_potential(path));
    let entered = parser::ParserImage { focus, ..image };
    assert(parser_image_obligation_wf(entered));
    parser_settle_frames(entered);
    let selected = parser::settle_parser_owners(entered);
    if focus == parser::parser_dead(path) {
        assert(selected == entered);
    } else {
        assert(!parser_top_is_obligation(focus));
    }
    if image.owners.len() > 0 {
        assert(entered.owners[0] == image.owners[0]);
        assert(parser_owner_parent_wf(image.owners[0]));
    } else {
        assert(selected == entered);
    }
}

/// `frame_layout_wf` keeps one source position per path byte.
pub proof fn epm_layout_path_lengths(frame: epm::EpmFrame)
    requires
        epm::frame_layout_wf(frame),
    ensures
        frame.path.path_bytes.len() == frame.path.path_positions.len(),
{
    hide(epm::indexed_image_wf);
    hide(epm::indexed_backing_project);
    hide(epm::epm_prefix_from_bytes);
    hide(epm::act_plan_projects_validated);
    hide(epm::act_plan_events);
    hide(epm::cursor_event_count);
}

/// An edge pushes the checkpoint of a frame that does not wait: it was taken
/// at the current path length and satisfies the checkpoint invariant.
pub proof fn epm_restore_push_wf(before: epm::EpmFrame, after_path: epm::PathTopology)
    requires
        epm_frame_path_wf(before),
        before.wait.is_none(),
        before.path.path_bytes.len() < usize::MAX as int,
        after_path.restore == before.path.restore.push(
            epm::PathCheckpoint {
                pending: before.path.pending,
                gate: before.path.gate,
                path_len: before.path.path_bytes.len() as usize,
                parser: before.path.parser,
            },
        ),
        after_path.path_bytes.len() == before.path.path_bytes.len() + 1,
    ensures
        forall|i: int| 0 <= i < after_path.restore.len()
            ==> escape::parser_image_escape_wf((#[trigger] after_path.restore[i]).parser),
        epm_restore_wf(after_path),
{
    let count = before.path.restore.len();
    let checkpoint = epm::PathCheckpoint {
        pending: before.path.pending,
        gate: before.path.gate,
        path_len: before.path.path_bytes.len() as usize,
        parser: before.path.parser,
    };
    assert(checkpoint.path_len as int == count);
    assert(epm_checkpoint_wf(checkpoint, count as int));
    assert forall|i: int| 0 <= i < after_path.restore.len()
        implies escape::parser_image_escape_wf((#[trigger] after_path.restore[i]).parser) by {
        if i < count {
            assert(after_path.restore[i] == before.path.restore[i]);
        } else {
            assert(after_path.restore[i] == checkpoint);
        }
    }
    assert forall|i: int| 0 <= i < after_path.restore.len()
        implies epm_checkpoint_wf(#[trigger] after_path.restore[i], i) by {
        if i < count {
            assert(after_path.restore[i] == before.path.restore[i]);
        } else {
            assert(after_path.restore[i] == checkpoint);
        }
    }
}

/// The initial EPathMap frame satisfies the invariant.
pub proof fn epm_frame_path_wf_initial(
    backing: epm::IndexedBackingImage,
    snapshot: epm::IndexedRegionImage,
    prefix: epm::EpmPrefixImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    ensures
        epm_frame_path_wf(epm::initial_epm_candidate(backing, snapshot, prefix, plan, validated)),
{
    escape::parser_escape_wf_initial();
    let frame = epm::initial_epm_candidate(backing, snapshot, prefix, plan, validated);
    assert(frame.path.parser == parser::initial_parser());
    assert(parser_image_obligation_wf(frame.path.parser));
    assert(epm_restore_wf(frame.path));
}

/// A Plain edge keeps the invariant: the pending obligation and its focused
/// Obligation frame are kept, and the restore checkpoint of the prestate is
/// pushed.
pub proof fn epm_frame_path_wf_plain_edge(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    source_offset: usize,
    origin: epm::EdgeOrigin,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(
            before,
            after,
            epm::EpmEvent::Edge { source_offset, origin, effect: epm::PathEdgeEffect::Plain },
            admission_next_id,
            action,
        ),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::feed_parser_byte);
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    hide(epm::edge_origin_valid);
    hide(epm::edge_byte);
    hide(epm::trie_position);
    hide(epm::modeled_plain_gate_transition);
    hide(epm::indexed_from_source);
    let byte = epm::edge_byte(before, source_offset, origin);
    let anchor = epm::trie_position(before, source_offset);
    let source = parser::ParserSource { source: anchor.source, offset: anchor.offset };
    let path_len = (before.path.path_bytes.len() + 1) as usize;
    let image = before.path.parser;
    let step = parser::feed_parser_byte(image, byte, source, path_len);
    parser_feed_byte_frames(image, byte, source, path_len);
    escape::parser_escape_wf_feed_byte(image, byte, source, path_len);
    assert(after.path.parser == step.image);
    assert(step.event is Continue);
    assert(before.wait.is_none());
    epm_restore_push_wf(before, after.path);
    match before.path.pending {
        Some(obligation) => {
            assert(!obligation.awaiting_resolution);
            assert(parser_top_is_obligation(image.focus));
            assert(after.path.parser.focus.stack.last() == image.focus.stack.last());
        },
        None => {
            if parser::parser_root_gate(image).dead {
                assert(after.path.parser == image);
            }
        },
    }
    assert(!epm::obligation_ready(after.path));
    assert(after.wait.is_none());
    assert(epm_pending_matches_focus(after.path.pending, after.path.parser));
    assert(epm_path_length_bounds(after));
}

/// A Start edge keeps the invariant: the parser reports the new Obligation
/// frame on the focused top of a live path, and an empty payload is waited on.
pub proof fn epm_frame_path_wf_start_edge(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    source_offset: usize,
    origin: epm::EdgeOrigin,
    kind: epm::PathObligationKind,
    payload_len: usize,
    tag: epm::SourceAnchor,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(
            before,
            after,
            epm::EpmEvent::Edge {
                source_offset,
                origin,
                effect: epm::PathEdgeEffect::Start { kind, payload_len, tag },
            },
            admission_next_id,
            action,
        ),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::feed_parser_byte);
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    hide(epm::edge_origin_valid);
    hide(epm::edge_byte);
    hide(epm::trie_position);
    hide(epm::modeled_plain_gate_transition);
    hide(epm::indexed_from_source);
    let byte = epm::edge_byte(before, source_offset, origin);
    let anchor = epm::trie_position(before, source_offset);
    let source = parser::ParserSource { source: anchor.source, offset: anchor.offset };
    let path_len = (before.path.path_bytes.len() + 1) as usize;
    let image = before.path.parser;
    let step = parser::feed_parser_byte(image, byte, source, path_len);
    parser_feed_byte_frames(image, byte, source, path_len);
    escape::parser_escape_wf_feed_byte(image, byte, source, path_len);
    epm_layout_path_lengths(after);
    assert(after.path.parser == step.image);
    assert(before.wait.is_none());
    epm_restore_push_wf(before, after.path);
    assert(after.path.gate == before.path.gate);
    assert(!before.path.gate.dead);
    assert(!parser::parser_root_gate(step.image).dead);
    assert(path_len as int == after.path.path_bytes.len());
    let obligation = after.path.pending.unwrap();
    match step.event {
        parser::ParserEvent::ObligationStarted { id, kind: parser_kind, payload_len: parser_len } => {
            assert(parser_started_top(step.image.focus, id, parser_kind, parser_len, path_len));
            assert(obligation.id == id);
            assert(epm::parser_kind_matches(obligation.kind, parser_kind));
            assert(obligation.payload_len as int == parser_len as int);
            assert(obligation.payload_start == path_len);
        },
        _ => {},
    }
    assert(epm_pending_matches_focus(after.path.pending, after.path.parser));
    if obligation.awaiting_resolution {
        assert(epm::obligation_ready(after.path));
        assert(epm_waits_on_obligation(after));
    }
    assert(epm_wait_awaits_pending(after));
    assert(epm_path_length_bounds(after));
}

/// A Payload edge keeps the invariant: the parser reports the byte of the
/// focused Obligation frame on a live path, the last byte makes it await
/// resolution, and the awaiting obligation is waited on.
pub proof fn epm_frame_path_wf_payload_edge(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    source_offset: usize,
    origin: epm::EdgeOrigin,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(
            before,
            after,
            epm::EpmEvent::Edge { source_offset, origin, effect: epm::PathEdgeEffect::Payload },
            admission_next_id,
            action,
        ),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::feed_parser_byte);
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    hide(epm::edge_origin_valid);
    hide(epm::edge_byte);
    hide(epm::trie_position);
    hide(epm::modeled_plain_gate_transition);
    hide(epm::indexed_from_source);
    let byte = epm::edge_byte(before, source_offset, origin);
    let anchor = epm::trie_position(before, source_offset);
    let source = parser::ParserSource { source: anchor.source, offset: anchor.offset };
    let path_len = (before.path.path_bytes.len() + 1) as usize;
    let image = before.path.parser;
    let step = parser::feed_parser_byte(image, byte, source, path_len);
    parser_feed_byte_frames(image, byte, source, path_len);
    escape::parser_escape_wf_feed_byte(image, byte, source, path_len);
    epm_layout_path_lengths(after);
    assert(after.path.parser == step.image);
    assert(before.wait.is_none());
    epm_restore_push_wf(before, after.path);
    assert(step.event is ObligationByte);
    assert(!parser::parser_root_gate(image).dead);
    assert(after.path.gate == before.path.gate);
    assert(!parser::parser_root_gate(step.image).dead);
    assert(path_len as int == after.path.path_bytes.len());
    let prior_obligation = before.path.pending.unwrap();
    let obligation = after.path.pending.unwrap();
    match step.event {
        parser::ParserEvent::ObligationByte { id, last } => {
            assert(id == prior_obligation.id);
            assert(parser_payload_byte_tops(image.focus, step.image.focus, id, last, path_len));
            assert(obligation.awaiting_resolution == last);
        },
        _ => {},
    }
    assert(epm_pending_matches_focus(after.path.pending, after.path.parser));
    if obligation.awaiting_resolution {
        assert(epm::obligation_ready(after.path));
        assert(epm_waits_on_obligation(after));
    }
    assert(epm_wait_awaits_pending(after));
    assert(epm_path_length_bounds(after));
}

/// Leave keeps the invariant: it restores the last checkpoint, whose parser,
/// pending obligation and length satisfy the checkpoint invariant, or marks
/// the root left.
pub proof fn epm_frame_path_wf_leave(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    source_offset: usize,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(
            before,
            after,
            epm::EpmEvent::Leave { source_offset },
            admission_next_id,
            action,
        ),
    ensures
        epm_frame_path_wf(after),
{
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    hide(epm::topology_offset_valid);
    hide(epm::topology_position);
    assert(after.wait.is_none());
    assert(after.cursor.at == before.cursor.at + 1);
    if before.path.restore.len() > 0 {
        let count = before.path.restore.len();
        let checkpoint = before.path.restore.last();
        assert(checkpoint == before.path.restore[count - 1]);
        assert(epm_checkpoint_wf(checkpoint, count - 1));
        assert(escape::parser_image_escape_wf(checkpoint.parser));
        escape::parser_escape_wf_restore(checkpoint.parser, before.path.parser);
        let restored = after.path.parser;
        assert(restored == parser::restore_parser_checkpoint(checkpoint.parser, before.path.parser));
        assert(restored.focus == checkpoint.parser.focus);
        assert(restored.owners == checkpoint.parser.owners);
        assert(parser::parser_root(restored) == parser::parser_root(checkpoint.parser));
        assert(parser::parser_root_gate(restored) == parser::parser_root_gate(checkpoint.parser));
        assert(parser_image_obligation_wf(restored));
        assert(epm_pending_matches_focus(after.path.pending, restored));
        assert(after.path.path_bytes.len() == count - 1);
        assert forall|i: int| 0 <= i < after.path.restore.len()
            implies #[trigger] after.path.restore[i] == before.path.restore[i] by {}
        assert(epm_path_escape_wf(after.path));
        assert(epm_restore_wf(after.path));
    } else {
        assert(after.path.parser == before.path.parser);
        assert(after.path.restore == before.path.restore);
    }
    assert(epm_path_length_bounds(after));
}

/// Value keeps the path, opens no Obligation wait, and advances the cursor.
pub proof fn epm_frame_path_wf_value(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    has_value: bool,
    value_body: Option<epm::ByteSpan>,
    source_offset: usize,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(
            before,
            after,
            epm::EpmEvent::Value { has_value, value_body, source_offset },
            admission_next_id,
            action,
        ),
    ensures
        epm_frame_path_wf(after),
{
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    hide(epm::topology_offset_valid);
    hide(epm::topology_position);
    hide(epm::indexed_subview);
    assert(after.path == before.path);
    assert(after.cursor.at == before.cursor.at + 1);
    assert(!epm_waits_on_obligation(after));
    assert(before.wait.is_none());
}

/// End keeps the path, the wait and the cursor position.
pub proof fn epm_frame_path_wf_end(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(before, after, epm::EpmEvent::End, admission_next_id, action),
    ensures
        epm_frame_path_wf(after),
{
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::map_value_body_at);
    assert(after.path == before.path);
    assert(after.wait == before.wait);
    assert(after.cursor.at == before.cursor.at);
}

/// Every `event_step` keeps the invariant.
pub proof fn epm_frame_path_wf_event_step(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    event: epm::EpmEvent,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm_frame_path_wf(before),
        epm::event_step(before, after, event, admission_next_id, action),
    ensures
        epm_frame_path_wf(after),
{
    hide(epm::event_step);
    hide(epm_frame_path_wf);
    match event {
        epm::EpmEvent::Edge { source_offset, origin, effect } => {
            match effect {
                epm::PathEdgeEffect::Plain => {
                    epm_frame_path_wf_plain_edge(
                        before,
                        after,
                        source_offset,
                        origin,
                        admission_next_id,
                        action,
                    );
                },
                epm::PathEdgeEffect::Start { kind, payload_len, tag } => {
                    epm_frame_path_wf_start_edge(
                        before,
                        after,
                        source_offset,
                        origin,
                        kind,
                        payload_len,
                        tag,
                        admission_next_id,
                        action,
                    );
                },
                epm::PathEdgeEffect::Payload => {
                    epm_frame_path_wf_payload_edge(
                        before,
                        after,
                        source_offset,
                        origin,
                        admission_next_id,
                        action,
                    );
                },
            }
        },
        epm::EpmEvent::Leave { source_offset } => {
            epm_frame_path_wf_leave(before, after, source_offset, admission_next_id, action);
        },
        epm::EpmEvent::Value { has_value, value_body, source_offset } => {
            epm_frame_path_wf_value(
                before,
                after,
                has_value,
                value_body,
                source_offset,
                admission_next_id,
                action,
            );
        },
        epm::EpmEvent::End => {
            epm_frame_path_wf_end(before, after, admission_next_id, action);
        },
    }
}

/// The MapValue arm of `accept_child` keeps the path and the cursor and ends
/// the wait; a MapValue wait has no awaiting obligation.
pub proof fn epm_frame_path_wf_map_value_accept(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    return_id: Option<u64>,
    summary: epm::ChildSummary,
)
    requires
        epm_frame_path_wf(before),
        match before.wait {
            Some(epm::EpmWait::MapValue { .. }) => true,
            _ => false,
        },
        epm::accept_child(before, after, return_id, summary),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::resolve_parser_obligation);
    assert(after.path == before.path);
    assert(after.cursor == before.cursor);
    assert(after.wait.is_none());
    assert(!epm_waits_on_obligation(before));
}

/// The obligation arm of `accept_child` keeps the invariant: the resolution
/// does not raise the root potential, and the focused Obligation frame is
/// popped or the path dies, so no pending obligation is left.
pub proof fn epm_frame_path_wf_obligation_accept(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    return_id: Option<u64>,
    summary: epm::ChildSummary,
)
    requires
        epm_frame_path_wf(before),
        epm_waits_on_obligation(before),
        epm::accept_child(before, after, return_id, summary),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::resolve_parser_obligation);
    match before.wait {
        Some(epm::EpmWait::Obligation { id, .. }) => {
            let image = before.path.parser;
            let resolution = epm::parser_resolution(summary);
            assert(before.path.pending.is_some());
            assert(image.focus.stack.len() > 0);
            parser_resolve_frames(image, id, resolution);
            escape::parser_escape_wf_resolve(image, id, resolution);
            assert(after.path.parser == parser::resolve_parser_obligation(image, id, resolution));
            assert(after.path.pending.is_none());
            assert(after.path.restore == before.path.restore);
            assert(after.path.path_bytes == before.path.path_bytes);
            assert(after.cursor == before.cursor);
            assert(after.wait.is_none());
            assert(epm_path_escape_wf(after.path));
            assert(epm_restore_wf(after.path));
            assert(epm_path_length_bounds(after));
        },
        _ => {},
    }
}

/// Both arms of `accept_child` keep the invariant.
pub proof fn epm_frame_path_wf_accept_child(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    return_id: Option<u64>,
    summary: epm::ChildSummary,
)
    requires
        epm_frame_path_wf(before),
        epm::accept_child(before, after, return_id, summary),
    ensures
        epm_frame_path_wf(after),
{
    hide(parser::resolve_parser_obligation);
    hide(epm_frame_path_wf);
    match before.wait {
        Some(epm::EpmWait::MapValue { .. }) => {
            epm_frame_path_wf_map_value_accept(before, after, return_id, summary);
        },
        Some(epm::EpmWait::Obligation { .. }) => {
            epm_frame_path_wf_obligation_accept(before, after, return_id, summary);
        },
        None => {},
    }
}

/// A frame that satisfies the invariant and waits on an obligation of `kind`
/// meets production's pre-checks (`epm_obligation_before_accept`) and the
/// parser premises of the return (`epm_obligation_parser_ready`) for every
/// child summary: (c) and the gate agreement make the parser live, (b) puts
/// the awaited Obligation frame on the focused top, (d) bounds the resolved
/// root count, and (a) makes a RawParEscape frame the whole root stack.
pub proof fn epm_frame_path_wf_discharges_return_premises(
    frame: epm::EpmFrame,
    kind: epm::PathObligationKind,
    summary: epm::ChildSummary,
)
    requires
        epm_frame_path_wf(frame),
        match frame.wait {
            Some(epm::EpmWait::Obligation { kind: waiting, .. }) => waiting == kind,
            _ => false,
        },
    ensures
        obligation_return::epm_obligation_before_accept(frame, kind),
        return_source::epm_obligation_parser_ready(frame, kind, summary),
{
    hide(parser::resolve_parser_obligation);
    match frame.wait {
        Some(epm::EpmWait::Obligation { id, .. }) => {
            let image = frame.path.parser;
            let resolution = epm::parser_resolution(summary);
            assert(!parser::parser_root_gate(image).dead);
            assert(image.focus.stack.len() > 0);
            parser_resolve_frames(image, id, resolution);
            let resolved = parser::resolve_parser_obligation(image, id, resolution);
            assert(parser::parser_root_gate(resolved).completed_segments as int
                <= parser_root_potential(resolved));
            assert(frame.cursor.at as int <= usize::MAX as int);
            if kind == epm::PathObligationKind::RawParEscape {
                escape::parser_escape_raw_par_top_is_root(image);
            }
        },
        _ => {},
    }
}

}
