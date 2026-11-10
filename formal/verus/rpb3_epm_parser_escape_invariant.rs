//! Escape-capability invariant of the CanonicalPath parser model
//! (`rpb3_path_parser_refinement`).
//!
//! The raw-Par escape tag 0x0f is accepted only while the top frame is
//! `Tag { escape_allowed: true }`. A tag frame is pushed on an empty stack as
//! `Tag { escape_allowed: path.escape_allowed }`, List kids are tagged
//! `Tag { escape_allowed: false }`, Region children start from
//! `initial_parser_path(false)`, and the root starts from
//! `initial_parser_path(true)`. `parser_image_escape_wf` states the consequence:
//! an escape-capable frame (the escape-allowing tag, the RawParEscape
//! obligation-length varint, or a RawParEscape obligation) sits only at stack
//! index 0 of the root path, and only when that path allows escapes.
//!
//! The invariant holds for `initial_parser` and is preserved by
//! `feed_parser_byte`, `resolve_parser_obligation` and
//! `restore_parser_checkpoint`. So a RawParEscape obligation on the focused top
//! is the whole root stack, and no Region owner is active.

use vstd::prelude::*;

use crate::rpb3_path_parser_refinement as parser;

verus! {

/// A frame that may exist only at the bottom of an escape-allowing root path.
pub open spec fn parser_escape_frame(f: parser::ParserFrame) -> bool {
    match f {
        parser::ParserFrame::Tag { escape_allowed } => escape_allowed,
        parser::ParserFrame::Varint {
            role: parser::ParserVarintRole::ObligationLength {
                kind: parser::ParserObligationKind::RawParEscape, ..
            }, ..
        } => true,
        parser::ParserFrame::Obligation {
            kind: parser::ParserObligationKind::RawParEscape, ..
        } => true,
        _ => false,
    }
}

/// `root` says whether `path` is the root path of its image.
pub open spec fn parser_path_escape_wf(path: parser::ParserPath, root: bool) -> bool {
    &&& (path.escape_allowed ==> root)
    &&& forall|i: int| 0 <= i < path.stack.len() && #[trigger] parser_escape_frame(path.stack[i])
        ==> path.escape_allowed && i == 0
}

/// The root path is the outermost owner's parent, or the focus when no Region
/// owner is active. Every owner's parent also keeps a non-empty stack (its
/// RegionActive top), because settling an owner pops or replaces that top.
pub open spec fn parser_image_escape_wf(image: parser::ParserImage) -> bool {
    &&& forall|k: int| 0 <= k < image.owners.len()
        ==> parser_path_escape_wf(#[trigger] image.owners[k].parent, k == 0)
    &&& forall|k: int| 0 <= k < image.owners.len()
        ==> (#[trigger] image.owners[k].parent).stack.len() > 0
    &&& parser_path_escape_wf(image.focus, image.owners.len() == 0)
}

pub proof fn parser_escape_wf_initial()
    ensures parser_image_escape_wf(parser::initial_parser()),
{}

/// Bubbling pops finished Kids frames and tags the next List kid
/// `Tag { escape_allowed: false }`; it adds no escape-capable frame.
pub proof fn parser_escape_bubble_keeps_path(
    path: parser::ParserPath,
    segment_stable: bool,
    root: bool,
)
    requires parser_path_escape_wf(path, root),
    ensures parser_path_escape_wf(parser::bubble_parser(path, segment_stable), root),
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
                    assert(parser_path_escape_wf(next, root));
                    parser_escape_bubble_keeps_path(next, segment_stable, root);
                } else {
                    assert(parser_path_escape_wf(
                        parser::bubble_parser(path, segment_stable),
                        root,
                    ));
                }
            },
            _ => {},
        }
    }
}

/// Settling a finished Region child refocuses its owner's parent in the same
/// root position: killed, bubbled after popping its RegionActive top, or
/// reopened at RegionLength.
pub proof fn parser_escape_settle_keeps_image(image: parser::ParserImage)
    requires parser_image_escape_wf(image),
    ensures parser_image_escape_wf(parser::settle_parser_owners(image)),
    decreases image.owners.len(),
{
    hide(parser::parser_bytes_less);
    if image.owners.len() > 0 {
        let last = image.owners.len() - 1;
        let owner = image.owners.last();
        if image.focus.status != parser::ParserStatus::Dead
            && owner.path_remaining == 0
            && !parser::parser_waiting_obligation(image.focus)
        {
            let root = last == 0;
            assert(owner == image.owners[last]);
            assert(parser_path_escape_wf(owner.parent, root));
            assert(owner.parent.stack.len() > 0);
            let rest = parser::pop_parser_top(owner.parent);
            assert forall|i: int| 0 <= i < rest.stack.len()
                implies #[trigger] rest.stack[i] == owner.parent.stack[i] by {}
            assert(parser_path_escape_wf(rest, root));
            parser_escape_bubble_keeps_path(rest, true, root);
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
            assert(parser_path_escape_wf(parent, root));
            let next = parser::ParserImage {
                focus: parent,
                owners: image.owners.drop_last(),
                ..image
            };
            assert forall|k: int| 0 <= k < next.owners.len()
                implies #[trigger] next.owners[k] == image.owners[k] by {}
            assert(parser_image_escape_wf(next));
            parser_escape_settle_keeps_image(next);
            assert(parser::settle_parser_owners(image) == parser::settle_parser_owners(next));
        }
    }
}

/// Feeding any byte keeps the invariant: dead or faulted images, reusable
/// payload bytes, every focused frame kind (including a new Region owner), and
/// settling.
pub proof fn parser_escape_wf_feed_byte(
    image: parser::ParserImage,
    byte: u8,
    source: parser::ParserSource,
    path_len: usize,
)
    requires parser_image_escape_wf(image),
    ensures parser_image_escape_wf(parser::feed_parser_byte(image, byte, source, path_len).image),
{
    hide(parser::feed_parser_varint);
    hide(parser::feed_parser_utf8);
    hide(parser::reusable_parser_payload);
    hide(parser::parser_owner_has_room);
    hide(parser::settle_parser_owners);
    let root = image.owners.len() == 0;
    let selected = parser::consume_parser_owner_byte(image, byte);
    assert forall|k: int| 0 <= k < selected.owners.len()
        implies #[trigger] selected.owners[k].parent == image.owners[k].parent by {}
    assert(parser_image_escape_wf(selected));
    let path = selected.focus;
    let step = parser::feed_parser_focus(selected, byte, source, path_len);
    if path.status == parser::ParserStatus::Open {
        let prepared = parser::prepare_parser_tag(path, byte, source);
        if prepared.status == parser::ParserStatus::Open {
            assert(prepared.stack.len() > 0);
            assert(parser_path_escape_wf(prepared, root));
            let top = prepared.stack.last();
            if parser_escape_frame(top) {
                assert(prepared.escape_allowed && prepared.stack.len() == 1);
            }
            let rest = parser::pop_parser_top(prepared);
            assert forall|i: int| 0 <= i < rest.stack.len()
                implies #[trigger] rest.stack[i] == prepared.stack[i] by {}
            assert(parser_path_escape_wf(rest, root));
            parser_escape_bubble_keeps_path(rest, true, root);
            match top {
                parser::ParserFrame::Tag { escape_allowed } => {
                    assert(parser_path_escape_wf(
                        parser::feed_parser_tag(prepared, byte, source, escape_allowed),
                        root,
                    ));
                },
                parser::ParserFrame::Varint { role, state } => {
                    match parser::feed_parser_varint(state, byte) {
                        Some(parser::ParserVarintFeed::More { state: next }) => {
                            assert(parser_path_escape_wf(
                                parser::replace_parser_top(
                                    prepared,
                                    parser::ParserFrame::Varint { role, state: next },
                                ),
                                root,
                            ));
                        },
                        Some(parser::ParserVarintFeed::Complete { value }) => {
                            let finished = parser::finish_parser_varint(
                                parser::ParserImage { focus: prepared, ..selected },
                                role,
                                value,
                                source,
                                path_len,
                            );
                            assert(parser_image_escape_wf(finished.image));
                        },
                        None => {},
                    }
                },
                parser::ParserFrame::Bytes { remaining, utf8 } => {
                    assert(parser_path_escape_wf(
                        parser::feed_parser_bytes(prepared, remaining, utf8, byte),
                        root,
                    ));
                },
                parser::ParserFrame::RegionLength { .. } => {
                    let parent = parser::replace_parser_top(
                        prepared,
                        parser::ParserFrame::RegionActive,
                    );
                    assert(parser_path_escape_wf(parent, root));
                    assert(parent.stack.len() > 0);
                },
                _ => {},
            }
            assert(parser_image_escape_wf(step.image));
        }
    }
    assert(parser_image_escape_wf(step.image));
    parser_escape_settle_keeps_image(step.image);
}

/// The non-empty focused stack is the recommends clause of
/// `resolve_parser_obligation`, which pops the focused top.
pub proof fn parser_escape_wf_resolve(
    image: parser::ParserImage,
    id: u64,
    resolution: parser::ParserResolution,
)
    requires
        parser_image_escape_wf(image),
        image.focus.stack.len() > 0,
    ensures parser_image_escape_wf(parser::resolve_parser_obligation(image, id, resolution)),
{
    hide(parser::settle_parser_owners);
    let root = image.owners.len() == 0;
    let path = image.focus;
    let rest = parser::pop_parser_top(path);
    assert forall|i: int| 0 <= i < rest.stack.len()
        implies #[trigger] rest.stack[i] == path.stack[i] by {}
    assert(parser_path_escape_wf(rest, root));
    parser_escape_bubble_keeps_path(rest, false, root);
    parser_escape_bubble_keeps_path(rest, true, root);
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
    let entered = parser::ParserImage { focus, ..image };
    assert(parser_image_escape_wf(entered));
    parser_escape_settle_keeps_image(entered);
}

/// Restoring a checkpoint keeps the saved focus and owners.
pub proof fn parser_escape_wf_restore(saved: parser::ParserImage, current: parser::ParserImage)
    requires parser_image_escape_wf(saved),
    ensures parser_image_escape_wf(parser::restore_parser_checkpoint(saved, current)),
{}

/// A RawParEscape obligation on the focused top is the whole root stack.
pub proof fn parser_escape_raw_par_top_is_root(image: parser::ParserImage)
    requires
        parser_image_escape_wf(image),
        image.focus.stack.len() > 0,
        match image.focus.stack.last() {
            parser::ParserFrame::Obligation {
                kind: parser::ParserObligationKind::RawParEscape, ..
            } => true,
            _ => false,
        },
    ensures
        image.owners.len() == 0,
        image.focus.stack.len() == 1,
{
    assert(parser_escape_frame(image.focus.stack[image.focus.stack.len() - 1]));
}

}
