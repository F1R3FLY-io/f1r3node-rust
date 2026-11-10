//! Open obligation 6 of `docs/design/pathmap/rpb3-admission-source-traceability.md`:
//! source refinement of the general Plain parser relation.
//!
//! The optimized transitions of the model use closed-form jumps rather than
//! byte steps: `forward_parser_payload` for the fresh and coalesced Forward
//! batches, every expanded restore checkpoint, and the CanonicalPath bulk skip;
//! `restore_parser_checkpoint` for Leave; and saved-handle source images for
//! historical checkpoints. This module derives each of them from the scalar
//! byte transition `feed_parser_byte` and the checkpoints pushed by scalar
//! Edges. It does not prove that the production Rust executes these modeled
//! transitions; the remaining Rust projection premises are restated at each
//! theorem.

use vstd::prelude::*;

use crate::{
    rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_historical_checkpoint_source as historical,
    rpb3_epathmap_event_refinement as epm, rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_restore_run_expansion as restore, rpb3_path_parser_refinement as parser,
};

verus! {

// ---------------------------------------------------------------------------
// 1. Scalar byte runs of the general Plain parser relation
// ---------------------------------------------------------------------------

/// Feed `bytes` one at a time through the scalar relation. Byte `i` is fed
/// with path length `depth + i + 1`, as every scalar Edge does: EPM
/// `edge_return` passes `path_bytes.len() + 1`, CanonicalPath
/// `canonical_path_internal_edge_next` passes `at + 1`.
pub open spec fn plain_parser_byte_run(
    image: parser::ParserImage,
    bytes: Seq<u8>,
    sources: Seq<parser::ParserSource>,
    depth: int,
) -> parser::ParserImage
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        image
    } else {
        parser::feed_parser_byte(
            plain_parser_byte_run(image, bytes.drop_last(), sources.drop_last(), depth),
            bytes.last(),
            sources.last(),
            (depth + bytes.len()) as usize,
        ).image
    }
}

/// The event returned by the scalar relation for byte `index` of a run.
pub open spec fn plain_parser_byte_event(
    image: parser::ParserImage,
    bytes: Seq<u8>,
    sources: Seq<parser::ParserSource>,
    depth: int,
    index: int,
) -> parser::ParserEvent {
    parser::feed_parser_byte(
        plain_parser_byte_run(image, bytes.subrange(0, index), sources.subrange(0, index), depth),
        bytes[index],
        sources[index],
        (depth + index + 1) as usize,
    ).event
}

/// Identifier of the focused path's top frame when it is an obligation still
/// receiving payload bytes.
pub open spec fn parser_payload_obligation_id(image: parser::ParserImage) -> Option<u64> {
    if image.focus.stack.len() == 0 {
        None
    } else {
        match image.focus.stack.last() {
            parser::ParserFrame::Obligation {
                id, status: parser::ParserObligationStatus::Payload, ..
            } => Some(id),
            _ => None,
        }
    }
}

pub open spec fn parser_sources(positions: Seq<epm::SourceAnchor>) -> Seq<parser::ParserSource> {
    Seq::new(positions.len(), |i: int| parser::ParserSource {
        source: positions[i].source,
        offset: positions[i].offset,
    })
}

// ---------------------------------------------------------------------------
// 2. The Forward jump is the scalar reusable-payload byte run
// ---------------------------------------------------------------------------

/// `forward_parser_payload` changes only the stamp and the active Region
/// residuals, so the root path, the root gate, the focus, and the global
/// obligation counter are those of the original image.
pub proof fn forward_payload_root_projection(image: parser::ParserImage, bytes: Seq<u8>)
    ensures
        ({
            let after = parser::forward_parser_payload(image, bytes);
            &&& after.focus == image.focus
            &&& after.fault == image.fault
            &&& after.next_obligation == image.next_obligation
            &&& after.owners.len() == image.owners.len()
            &&& after.stamp == (image.stamp as int + bytes.len()) as usize
            &&& parser::parser_root(after) == parser::parser_root(image)
            &&& parser::parser_root_gate(after) == parser::parser_root_gate(image)
            &&& parser_payload_obligation_id(after) == parser_payload_obligation_id(image)
        }),
{
    let after = parser::forward_parser_payload(image, bytes);
    if image.owners.len() > 0 {
        assert(after.owners[0].parent == image.owners[0].parent);
    }
}

pub proof fn forward_payload_without_bytes_is_identity(image: parser::ParserImage)
    ensures parser::forward_parser_payload(image, Seq::<u8>::empty()) == image,
{
    let after = parser::forward_parser_payload(image, Seq::<u8>::empty());
    assert forall|i: int| 0 <= i < image.owners.len() implies
        #[trigger] after.owners[i] == image.owners[i] by {
        assert(after.owners[i].child_bytes =~= image.owners[i].child_bytes);
    }
    assert(after.owners =~= image.owners);
}

pub proof fn forward_capacity_narrows_to_shorter_width(
    image: parser::ParserImage,
    wide: int,
    narrow: int,
)
    requires
        parser::parser_forward_capacity(image, wide),
        0 < narrow <= wide,
    ensures
        parser::parser_forward_capacity(image, narrow),
{
    assert forall|i: int| 0 <= i < image.owners.len() implies
        #[trigger] image.owners[i].remaining as int > narrow
            && image.owners[i].path_remaining as int > narrow by {
        assert(image.owners[i].remaining as int > wide);
        assert(image.owners[i].path_remaining as int > wide);
    }
}

/// One scalar byte inside a forwardable payload interior. After `index`
/// bytes have been forwarded, the next scalar byte takes the reusable-payload
/// branch of `feed_parser_byte` and lands exactly on the jump of `index + 1`
/// bytes. The byte's provenance is irrelevant on this branch.
pub proof fn forward_capacity_step_feeds_reusable_byte(
    image: parser::ParserImage,
    bytes: Seq<u8>,
    index: int,
    source: parser::ParserSource,
)
    requires
        parser::parser_forward_capacity(image, bytes.len() as int),
        0 <= index < bytes.len(),
    ensures
        parser_payload_obligation_id(image).is_some(),
        parser::feed_parser_byte(
            parser::forward_parser_payload(image, bytes.subrange(0, index)),
            bytes[index],
            source,
            (image.stamp as int + index + 1) as usize,
        ) == (parser::ParserStep {
            image: parser::forward_parser_payload(image, bytes.subrange(0, index + 1)),
            event: parser::ParserEvent::ObligationByte {
                id: parser_payload_obligation_id(image).unwrap(),
                last: false,
            },
        }),
{
    hide(parser::feed_parser_focus);
    hide(parser::settle_parser_owners);
    let width = bytes.len() as int;
    let done = bytes.subrange(0, index);
    let prior = parser::forward_parser_payload(image, done);
    let next = parser::forward_parser_payload(image, bytes.subrange(0, index + 1));
    let path_len = (image.stamp as int + index + 1) as usize;
    forward_payload_root_projection(image, done);
    assert(done.len() == index);
    assert(!parser::parser_root_gate(prior).dead);
    assert(!prior.fault);
    assert(prior.stamp as int == image.stamp as int + index);
    assert(prior.stamp as int + 1 == path_len as int);
    assert forall|k: int| 0 <= k < prior.owners.len() implies
        #[trigger] prior.owners[k].remaining > 1 && prior.owners[k].path_remaining > 1 by {
        assert(image.owners[k].remaining as int > width);
        assert(image.owners[k].path_remaining as int > width);
        assert(prior.owners[k].remaining as int == image.owners[k].remaining as int - index);
        assert(prior.owners[k].path_remaining as int
            == image.owners[k].path_remaining as int - index);
    }
    assert(parser::parser_owner_has_room(prior));
    let id = parser_payload_obligation_id(image).unwrap();
    assert(parser::reusable_parser_payload(prior, path_len) == Some(id));
    let consumed = parser::consume_parser_owner_byte(prior, bytes[index]);
    let stamped = parser::ParserImage { stamp: path_len, ..consumed };
    assert forall|k: int| 0 <= k < image.owners.len() implies
        #[trigger] stamped.owners[k] == next.owners[k] by {
        assert(prior.owners[k].child_bytes.push(bytes[index])
            =~= image.owners[k].child_bytes + bytes.subrange(0, index + 1));
        assert(prior.owners[k].remaining as int == image.owners[k].remaining as int - index);
        assert(prior.owners[k].path_remaining as int
            == image.owners[k].path_remaining as int - index);
    }
    assert(stamped.owners =~= next.owners);
    assert(stamped == next);
}

/// Induction over the forwarded prefix length.
pub proof fn forward_payload_prefix_matches_byte_run(
    image: parser::ParserImage,
    bytes: Seq<u8>,
    sources: Seq<parser::ParserSource>,
    count: int,
)
    requires
        parser::parser_forward_capacity(image, bytes.len() as int),
        sources.len() == bytes.len(),
        0 <= count <= bytes.len(),
    ensures
        plain_parser_byte_run(
            image, bytes.subrange(0, count), sources.subrange(0, count), image.stamp as int,
        ) == parser::forward_parser_payload(image, bytes.subrange(0, count)),
    decreases count,
{
    hide(parser::feed_parser_byte);
    if count == 0 {
        forward_payload_without_bytes_is_identity(image);
        assert(bytes.subrange(0, 0) =~= Seq::<u8>::empty());
    } else {
        forward_payload_prefix_matches_byte_run(image, bytes, sources, count - 1);
        let prefix = bytes.subrange(0, count);
        let prefix_sources = sources.subrange(0, count);
        assert(prefix.drop_last() =~= bytes.subrange(0, count - 1));
        assert(prefix_sources.drop_last() =~= sources.subrange(0, count - 1));
        assert(prefix.last() == bytes[count - 1]);
        forward_capacity_step_feeds_reusable_byte(image, bytes, count - 1, prefix_sources.last());
    }
}

/// Parser core of obligation 6. Under the capacity accepted by every
/// optimized transition, the closed-form jump equals the scalar byte run
/// for every provenance sequence, every scalar event is a non-final payload
/// byte of the focused obligation, and the root gate and global obligation
/// counter are unchanged.
pub proof fn forward_payload_equals_plain_byte_run(
    image: parser::ParserImage,
    bytes: Seq<u8>,
    sources: Seq<parser::ParserSource>,
)
    requires
        parser::parser_forward_capacity(image, bytes.len() as int),
        sources.len() == bytes.len(),
    ensures
        plain_parser_byte_run(image, bytes, sources, image.stamp as int)
            == parser::forward_parser_payload(image, bytes),
        parser_payload_obligation_id(image).is_some(),
        forall|index: int| 0 <= index < bytes.len() ==>
            #[trigger] plain_parser_byte_event(image, bytes, sources, image.stamp as int, index)
                == (parser::ParserEvent::ObligationByte {
                    id: parser_payload_obligation_id(image).unwrap(),
                    last: false,
                }),
        parser::parser_root_gate(parser::forward_parser_payload(image, bytes))
            == parser::parser_root_gate(image),
        parser::forward_parser_payload(image, bytes).next_obligation == image.next_obligation,
{
    hide(parser::feed_parser_byte);
    forward_payload_prefix_matches_byte_run(image, bytes, sources, bytes.len() as int);
    assert(bytes.subrange(0, bytes.len() as int) =~= bytes);
    assert(sources.subrange(0, sources.len() as int) =~= sources);
    forward_capacity_step_feeds_reusable_byte(image, bytes, 0, sources[0]);
    assert forall|index: int| 0 <= index < bytes.len() implies
        #[trigger] plain_parser_byte_event(image, bytes, sources, image.stamp as int, index)
            == (parser::ParserEvent::ObligationByte {
                id: parser_payload_obligation_id(image).unwrap(),
                last: false,
            }) by {
        forward_payload_prefix_matches_byte_run(image, bytes, sources, index);
        forward_capacity_step_feeds_reusable_byte(image, bytes, index, sources[index]);
    }
    forward_payload_root_projection(image, bytes);
}

// ---------------------------------------------------------------------------
// 3. Accepted Forward batches are runs of scalar Payload Edges
// ---------------------------------------------------------------------------

/// The checkpoint every scalar Edge pushes: the complete pre-edge path state
/// (`plain_edge_path_shape`, `start_obligation`, `consume_obligation_byte`).
pub open spec fn scalar_edge_checkpoint(before: epm::PathTopology) -> epm::PathCheckpoint {
    epm::PathCheckpoint {
        pending: before.pending,
        gate: before.gate,
        path_len: before.path_bytes.len() as usize,
        parser: before.parser,
    }
}

/// Path successor of one scalar Payload Edge. Every field is the one fixed by
/// `edge_return` with effect `Payload`: `consume_obligation_byte` fixes the
/// obligation, counter, root flag, bytes, anchors, restore stack and gate, and
/// the parser-step clause fixes the parser to the scalar `feed_parser_byte`
/// image with path length `|path| + 1`.
pub open spec fn scalar_payload_edge_path(
    before: epm::PathTopology,
    byte: u8,
    source: epm::SourceAnchor,
) -> epm::PathTopology {
    let old = before.pending.unwrap();
    epm::PathTopology {
        next_obligation: before.next_obligation,
        pending: Some(epm::PathObligation {
            payload_end: source,
            awaiting_resolution: before.path_bytes.len() + 1 - old.payload_start
                == old.payload_len,
            ..old
        }),
        root_left: before.root_left,
        restore: before.restore.push(scalar_edge_checkpoint(before)),
        path_bytes: before.path_bytes.push(byte),
        path_positions: before.path_positions.push(source),
        gate: before.gate,
        parser: parser::feed_parser_byte(
            before.parser,
            byte,
            parser::ParserSource { source: source.source, offset: source.offset },
            (before.path_bytes.len() + 1) as usize,
        ).image,
    }
}

/// Scalar Payload Edges over `bytes`, fed in order with their anchors.
pub open spec fn scalar_payload_edge_run(
    before: epm::PathTopology,
    bytes: Seq<u8>,
    sources: Seq<epm::SourceAnchor>,
) -> epm::PathTopology
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        before
    } else {
        scalar_payload_edge_path(
            scalar_payload_edge_run(before, bytes.drop_last(), sources.drop_last()),
            bytes.last(),
            sources.last(),
        )
    }
}

/// The path-level clauses of `edge_return` with effect `Payload` for a byte
/// that does not complete its obligation. Omitted are only the frame clauses
/// (cursor event, layout, trie position, origin and byte of the edge, frame
/// source, mode). The child-request clause appears as `!obligation_ready`,
/// which makes `request_for_obligation(after, _)` None
/// (`unready_obligation_requests_no_child`), so the Edge returns Continue.
pub open spec fn payload_edge_path_clauses(
    before: epm::PathTopology,
    after: epm::PathTopology,
    byte: u8,
    source: epm::SourceAnchor,
) -> bool {
    let step = parser::feed_parser_byte(
        before.parser,
        byte,
        parser::ParserSource { source: source.source, offset: source.offset },
        (before.path_bytes.len() + 1) as usize,
    );
    &&& !before.root_left
    &&& before.path_bytes.len() < usize::MAX as int
    &&& before.next_obligation == before.parser.next_obligation
    &&& epm::parser_gate_matches(before.gate, before.parser)
    &&& after.parser == step.image
    &&& after.next_obligation == step.image.next_obligation
    &&& epm::parser_gate_matches(after.gate, after.parser)
    &&& match (step.event, before.pending) {
        (parser::ParserEvent::ObligationByte { id, .. }, Some(obligation)) => id == obligation.id,
        _ => false,
    }
    &&& epm::consume_obligation_byte(before, after, byte, source)
    &&& !epm::obligation_ready(after)
}

pub proof fn unready_obligation_requests_no_child(path: epm::PathTopology, return_id: u64)
    requires
        !epm::obligation_ready(path),
    ensures
        epm::request_for_obligation(path, return_id).is_none(),
{
}

/// Bytes `[from, to)` of a restore run, counted from its first depth.
pub open spec fn run_bytes(run: restore::RestoreRunImage, from: int, to: int) -> Seq<u8> {
    run.current.bytes.subrange(run.first_depth as int + from, run.first_depth as int + to)
}

pub open spec fn run_anchors(
    run: restore::RestoreRunImage,
    from: int,
    to: int,
) -> Seq<epm::SourceAnchor> {
    run.current.positions.subrange(run.first_depth as int + from, run.first_depth as int + to)
}

/// Closed-form path after the first `count` bytes of a restore run. The
/// counter and root flag come from `template`; `prefix` is the restore stack
/// below the run. Count 0 is the pre-batch path and count `run_width` is the
/// published path; each count in between is a scalar intermediate state.
pub open spec fn forward_virtual_path(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
) -> epm::PathTopology {
    let depth = run.first_depth as int + count;
    epm::PathTopology {
        next_obligation: template.next_obligation,
        pending: Some(restore::pending_at_depth(run, depth)),
        root_left: template.root_left,
        restore: prefix + restore::expanded_run(run).subrange(0, count),
        path_bytes: run.current.bytes.subrange(0, depth),
        path_positions: run.current.positions.subrange(0, depth),
        gate: run.gate,
        parser: parser::forward_parser_payload(
            run.base_parser,
            run.current.bytes.subrange(run.first_depth as int, depth),
        ),
    }
}

/// Step `count` of a restore run meets the path clauses of a scalar Payload Edge.
pub open spec fn forward_virtual_step_clauses(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
) -> bool {
    let depth = run.first_depth as int + count;
    payload_edge_path_clauses(
        forward_virtual_path(template, prefix, run, count),
        forward_virtual_path(template, prefix, run, count + 1),
        run.current.bytes[depth],
        run.current.positions[depth],
    )
}

/// One scalar Payload Edge maps the virtual path at `count` to the virtual
/// path at `count + 1`: the pushed checkpoint is literally
/// `run_checkpoint(run, count)`, and the parser image is the reusable-payload
/// scalar step of section 2.
pub proof fn forward_virtual_step_is_scalar_payload_edge(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
)
    requires
        restore::run_wf(run),
        0 <= count < restore::run_width(run),
    ensures
        forward_virtual_path(template, prefix, run, count + 1) == scalar_payload_edge_path(
            forward_virtual_path(template, prefix, run, count),
            run.current.bytes[run.first_depth as int + count],
            run.current.positions[run.first_depth as int + count],
        ),
        parser::feed_parser_byte(
            forward_virtual_path(template, prefix, run, count).parser,
            run.current.bytes[run.first_depth as int + count],
            parser::ParserSource {
                source: run.current.positions[run.first_depth as int + count].source,
                offset: run.current.positions[run.first_depth as int + count].offset,
            },
            (forward_virtual_path(template, prefix, run, count).path_bytes.len() + 1) as usize,
        ).event == (parser::ParserEvent::ObligationByte {
            id: parser_payload_obligation_id(run.base_parser).unwrap(),
            last: false,
        }),
{
    hide(parser::feed_parser_byte);
    let first = run.first_depth as int;
    let depth = first + count;
    let width = restore::run_width(run);
    let all = run_bytes(run, 0, width);
    let byte = run.current.bytes[depth];
    let anchor = run.current.positions[depth];
    let source = parser::ParserSource { source: anchor.source, offset: anchor.offset };
    assert(all.len() == width);
    assert(all.subrange(0, count) =~= run.current.bytes.subrange(first, depth));
    assert(all.subrange(0, count + 1) =~= run.current.bytes.subrange(first, depth + 1));
    assert(all[count] == byte);
    forward_capacity_step_feeds_reusable_byte(run.base_parser, all, count, source);
    let before = forward_virtual_path(template, prefix, run, count);
    let after = forward_virtual_path(template, prefix, run, count + 1);
    let expected = scalar_payload_edge_path(before, byte, anchor);
    assert(before.path_bytes.len() == depth);
    assert(after.parser == expected.parser);
    assert(after.pending == expected.pending);
    assert(restore::expanded_run(run)[count] == scalar_edge_checkpoint(before));
    assert(after.restore =~= expected.restore);
    assert(after.path_bytes =~= expected.path_bytes);
    assert(after.path_positions =~= expected.path_positions);
}

/// The step also meets every path clause of `edge_return` with effect
/// Payload, given the two agreements that the batch guards do not carry: the
/// path counter equals the parser counter, and the parser's focused Payload
/// obligation is the pending obligation.
pub proof fn forward_virtual_step_meets_payload_edge_clauses(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
)
    requires
        restore::run_wf(run),
        0 <= count < restore::run_width(run),
        !template.root_left,
        template.next_obligation == run.base_parser.next_obligation,
        parser_payload_obligation_id(run.base_parser) == Some(run.base_pending.id),
    ensures
        forward_virtual_step_clauses(template, prefix, run, count),
{
    hide(parser::feed_parser_byte);
    let first = run.first_depth as int;
    let depth = first + count;
    forward_virtual_step_is_scalar_payload_edge(template, prefix, run, count);
    forward_payload_root_projection(run.base_parser, run.current.bytes.subrange(first, depth));
    forward_payload_root_projection(run.base_parser, run.current.bytes.subrange(first, depth + 1));
    let before = forward_virtual_path(template, prefix, run, count);
    let after = forward_virtual_path(template, prefix, run, count + 1);
    assert(before.path_bytes.len() == depth);
    assert(epm::consume_obligation_byte(
        before, after, run.current.bytes[depth], run.current.positions[depth],
    ));
}

/// Induction: `count` scalar Payload Edges from the virtual path at `from`.
pub proof fn forward_virtual_suffix_is_scalar_payload_edges(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    from: int,
    count: int,
)
    requires
        restore::run_wf(run),
        0 <= from,
        0 <= count,
        from + count <= restore::run_width(run),
    ensures
        forward_virtual_path(template, prefix, run, from + count) == scalar_payload_edge_run(
            forward_virtual_path(template, prefix, run, from),
            run_bytes(run, from, from + count),
            run_anchors(run, from, from + count),
        ),
    decreases count,
{
    hide(parser::feed_parser_byte);
    if count == 0 {
        assert(run_bytes(run, from, from).len() == 0);
    } else {
        forward_virtual_suffix_is_scalar_payload_edges(template, prefix, run, from, count - 1);
        forward_virtual_step_is_scalar_payload_edge(template, prefix, run, from + count - 1);
        let bytes = run_bytes(run, from, from + count);
        let anchors = run_anchors(run, from, from + count);
        assert(bytes.drop_last() =~= run_bytes(run, from, from + count - 1));
        assert(anchors.drop_last() =~= run_anchors(run, from, from + count - 1));
    }
}

/// Every expanded checkpoint parser (the saved image of each depth of a
/// payload run) is the scalar byte run over the run prefix.
pub proof fn payload_checkpoint_parser_is_plain_byte_prefix(
    run: restore::RestoreRunImage,
    index: int,
)
    requires
        restore::run_wf(run),
        0 <= index <= restore::run_width(run),
    ensures
        parser::forward_parser_payload(run.base_parser, run_bytes(run, 0, index))
            == plain_parser_byte_run(
                run.base_parser,
                run_bytes(run, 0, index),
                parser_sources(run_anchors(run, 0, index)),
                run.first_depth as int,
            ),
        index < restore::run_width(run) ==> restore::run_checkpoint(run, index).parser
            == plain_parser_byte_run(
                run.base_parser,
                run_bytes(run, 0, index),
                parser_sources(run_anchors(run, 0, index)),
                run.first_depth as int,
            ),
{
    hide(parser::feed_parser_byte);
    let width = restore::run_width(run);
    let all = run_bytes(run, 0, width);
    let sources = parser_sources(run_anchors(run, 0, width));
    assert(all.len() == width);
    assert(sources.len() == width);
    forward_payload_prefix_matches_byte_run(run.base_parser, all, sources, index);
    assert(all.subrange(0, index) =~= run_bytes(run, 0, index));
    assert(sources.subrange(0, index) =~= parser_sources(run_anchors(run, 0, index)));
}

pub open spec fn fresh_forward_run(
    before: epm::EpmFrame,
    pending: epm::PathObligation,
    state_id: usize,
    start: usize,
    width: usize,
) -> restore::RestoreRunImage {
    restore::publish_forward_run(
        batch::path_source(before),
        pending,
        before.path.gate,
        before.path.parser,
        state_id,
        batch::trie_source(before),
        start,
        width,
    )
}

/// The run an accepted fresh Forward batch publishes extends the pre-batch
/// path by the batch's trie bytes and anchors, and its virtual path at count
/// 0 is the pre-batch path.
pub proof fn fresh_forward_run_starts_at_pre_batch_path(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires
        batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
    ensures
        ({
            let run = fresh_forward_run(before, pending, state_id, start, width);
            let source = batch::trie_source(before);
            &&& restore::run_wf(run)
            &&& restore::run_width(run) == width as int
            &&& run.base_parser == before.path.parser
            &&& run.first_depth as int == before.path.path_bytes.len()
            &&& run.current.bytes == before.path.path_bytes
                + source.bytes.subrange(start as int, start as int + width as int)
            &&& run.current.positions == before.path.path_positions
                + source.positions.subrange(start as int, start as int + width as int)
            &&& forward_virtual_path(before.path, before.path.restore, run, 0) == before.path
        }),
{
    hide(parser::feed_parser_byte);
    let base = batch::path_source(before);
    let source = batch::trie_source(before);
    restore::accepted_forward_publishes_exact_virtual_checkpoints(
        base, pending, before.path.gate, before.path.parser, state_id, source, start, width, facts,
    );
    let run = fresh_forward_run(before, pending, state_id, start, width);
    let first = run.first_depth as int;
    let initial = forward_virtual_path(before.path, before.path.restore, run, 0);
    assert(restore::pending_at_depth(run, first) == pending);
    assert(run.current.bytes.subrange(first, first) =~= Seq::<u8>::empty());
    forward_payload_without_bytes_is_identity(before.path.parser);
    assert(initial.restore =~= before.path.restore);
    assert(initial.path_bytes =~= before.path.path_bytes);
    assert(initial.path_positions =~= before.path.path_positions);
    assert(initial == before.path);
}

/// The virtual path of the published run at count `width` is the path of the
/// frame `forward_batch_after` publishes.
pub proof fn fresh_forward_run_ends_at_published_path(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires
        batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
    ensures
        forward_virtual_path(
            before.path,
            before.path.restore,
            fresh_forward_run(before, pending, state_id, start, width),
            width as int,
        ) == batch::forward_batch_after(before, start, width, pending, state_id).path,
{
    hide(parser::feed_parser_byte);
    let base = batch::path_source(before);
    let source = batch::trie_source(before);
    restore::accepted_forward_publishes_exact_virtual_checkpoints(
        base, pending, before.path.gate, before.path.parser, state_id, source, start, width, facts,
    );
    let run = fresh_forward_run(before, pending, state_id, start, width);
    let after = batch::forward_batch_after(before, start, width, pending, state_id);
    let first = run.first_depth as int;
    let w = width as int;
    let published = forward_virtual_path(before.path, before.path.restore, run, w);
    assert(run.current.positions.len() == first + w);
    assert(restore::pending_at_depth(run, first + w) == restore::run_current_pending(run));
    assert(published.restore =~= after.path.restore);
    assert(published.path_bytes =~= after.path.path_bytes);
    assert(published.path_positions =~= after.path.path_positions);
    assert(published == after.path);
}

/// E, fresh batch. The accepted fresh Forward batch publishes exactly the
/// path reached by `width` scalar Payload Edges over the batch's trie bytes
/// and anchors. The virtual path at count 0 is the pre-batch path and at
/// count `width` the published path; with the two agreements, every step
/// meets the path clauses of `edge_return` with effect Payload.
pub proof fn fresh_forward_batch_is_scalar_payload_edges(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires
        batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
    ensures
        ({
            let run = fresh_forward_run(before, pending, state_id, start, width);
            let after = batch::forward_batch_after(before, start, width, pending, state_id);
            let source = batch::trie_source(before);
            &&& restore::run_wf(run)
            &&& restore::run_width(run) == width as int
            &&& run.base_parser == before.path.parser
            &&& forward_virtual_path(before.path, before.path.restore, run, 0) == before.path
            &&& forward_virtual_path(before.path, before.path.restore, run, width as int)
                == after.path
            &&& after.path == scalar_payload_edge_run(
                before.path,
                source.bytes.subrange(start as int, start as int + width as int),
                source.positions.subrange(start as int, start as int + width as int),
            )
            &&& (before.path.next_obligation == before.path.parser.next_obligation
                && parser_payload_obligation_id(before.path.parser) == Some(pending.id)
                ==> forall|count: int| 0 <= count < width as int
                    ==> #[trigger] forward_virtual_step_clauses(
                        before.path, before.path.restore, run, count,
                    ))
        }),
{
    hide(parser::feed_parser_byte);
    hide(batch::forward_batch_after);
    fresh_forward_run_starts_at_pre_batch_path(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    );
    fresh_forward_run_ends_at_published_path(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    );
    let source = batch::trie_source(before);
    let run = fresh_forward_run(before, pending, state_id, start, width);
    let w = width as int;
    forward_virtual_suffix_is_scalar_payload_edges(before.path, before.path.restore, run, 0, w);
    assert(run_bytes(run, 0, w) =~= source.bytes.subrange(start as int, start as int + w));
    assert(run_anchors(run, 0, w) =~= source.positions.subrange(start as int, start as int + w));
    if before.path.next_obligation == before.path.parser.next_obligation
        && parser_payload_obligation_id(before.path.parser) == Some(pending.id) {
        assert forall|count: int| 0 <= count < w implies #[trigger] forward_virtual_step_clauses(
            before.path, before.path.restore, run, count,
        ) by {
            forward_virtual_step_meets_payload_edge_clauses(
                before.path, before.path.restore, run, count,
            );
        }
    }
}

/// E, coalesced batch. Relative to the extended run, the pre-batch path is
/// the virtual path at the old run width (the old checkpoints are a prefix of
/// the extended expansion) and the published path is the virtual path at the
/// extended width, so the batch is again `width` scalar Payload Edges.
pub proof fn coalesced_forward_batch_is_scalar_payload_edges(
    before: epm::EpmFrame,
    run: restore::RestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    current_state_id: usize,
    current_pending: epm::PathObligation,
    start: usize,
    width: usize,
    facts: restore::ForwardCapacityFacts,
)
    requires
        batch::coalesced_forward_batch_source_guard(
            before, run, prefix, current_state_id, current_pending, start, width, facts,
        ),
    ensures
        ({
            let source = batch::trie_source(before);
            let extended = restore::extend_coalesced_forward_run(run, source, start, width);
            let after = batch::coalesced_forward_batch_after(before, run, prefix, start, width);
            let old_width = restore::run_width(run);
            &&& restore::run_wf(extended)
            &&& restore::run_width(extended) == old_width + width as int
            &&& forward_virtual_path(before.path, prefix, extended, old_width) == before.path
            &&& forward_virtual_path(before.path, prefix, extended, old_width + width as int)
                == after.path
            &&& after.path == scalar_payload_edge_run(
                before.path,
                source.bytes.subrange(start as int, start as int + width as int),
                source.positions.subrange(start as int, start as int + width as int),
            )
            &&& (before.path.next_obligation == before.path.parser.next_obligation
                && parser_payload_obligation_id(before.path.parser) == Some(current_pending.id)
                ==> forall|count: int| old_width <= count < old_width + width as int
                    ==> #[trigger] forward_virtual_step_clauses(
                        before.path, prefix, extended, count,
                    ))
        }),
{
    hide(parser::feed_parser_byte);
    let source = batch::trie_source(before);
    restore::accepted_coalesced_forward_extends_virtual_checkpoints(
        run, current_state_id, current_pending, source, start, width, facts,
    );
    let extended = restore::extend_coalesced_forward_run(run, source, start, width);
    let after = batch::coalesced_forward_batch_after(before, run, prefix, start, width);
    let first = run.first_depth as int;
    let old_width = restore::run_width(run);
    let w = width as int;
    let resumed = forward_virtual_path(before.path, prefix, extended, old_width);
    let published = forward_virtual_path(before.path, prefix, extended, old_width + w);
    assert(run.current.positions.len() == first + old_width);
    assert(restore::pending_at_depth(extended, first + old_width) == current_pending);
    assert(restore::expanded_run(extended).subrange(0, old_width)
        =~= restore::expanded_run(run));
    assert(resumed.restore =~= before.path.restore);
    assert(resumed.path_bytes =~= before.path.path_bytes);
    assert(resumed.path_positions =~= before.path.path_positions);
    assert(extended.current.bytes.subrange(first, first + old_width)
        =~= run.current.bytes.subrange(first, run.current.bytes.len() as int));
    assert(resumed == before.path);
    assert(extended.current.positions.len() == first + old_width + w);
    assert(restore::pending_at_depth(extended, first + old_width + w)
        == restore::run_current_pending(extended));
    assert(published.restore =~= after.path.restore);
    assert(published.path_bytes =~= after.path.path_bytes);
    assert(published.path_positions =~= after.path.path_positions);
    assert(published == after.path);
    forward_virtual_suffix_is_scalar_payload_edges(before.path, prefix, extended, old_width, w);
    assert(run_bytes(extended, old_width, old_width + w)
        =~= source.bytes.subrange(start as int, start as int + w));
    assert(run_anchors(extended, old_width, old_width + w)
        =~= source.positions.subrange(start as int, start as int + w));
    if before.path.next_obligation == before.path.parser.next_obligation
        && parser_payload_obligation_id(before.path.parser) == Some(current_pending.id) {
        forward_payload_root_projection(
            run.base_parser,
            run.current.bytes.subrange(first, run.current.bytes.len() as int),
        );
        assert forall|count: int| old_width <= count < old_width + w implies
            #[trigger] forward_virtual_step_clauses(before.path, prefix, extended, count) by {
            forward_virtual_step_meets_payload_edge_clauses(before.path, prefix, extended, count);
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Accepted Leave (Unwind) batches select scalar prefixes
// ---------------------------------------------------------------------------

/// Projections of the virtual path that an Unwind reads: the counter and
/// root flag are the template's, the parser keeps the run's base counter, and
/// the restore stack is the prefix followed by the first `count` checkpoints.
pub proof fn forward_virtual_path_projection(
    template: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
)
    ensures
        ({
            let path = forward_virtual_path(template, prefix, run, count);
            &&& path.next_obligation == template.next_obligation
            &&& path.root_left == template.root_left
            &&& path.parser.next_obligation == run.base_parser.next_obligation
            &&& path.restore == prefix + restore::expanded_run(run).subrange(0, count)
        }),
{
    forward_payload_root_projection(
        run.base_parser,
        run.current.bytes.subrange(run.first_depth as int, run.first_depth as int + count),
    );
}

/// The virtual path reads its template only through the counter and the
/// root flag.
pub proof fn forward_virtual_path_template_swap(
    first: epm::PathTopology,
    second: epm::PathTopology,
    prefix: Seq<epm::PathCheckpoint>,
    run: restore::RestoreRunImage,
    count: int,
)
    requires
        first.next_obligation == second.next_obligation,
        first.root_left == second.root_left,
    ensures
        forward_virtual_path(first, prefix, run, count)
            == forward_virtual_path(second, prefix, run, count),
{
}

/// L. An Unwind of `width` over the historical image of a payload run selects
/// the virtual path at `run_width - width`, which is the scalar Payload-edge
/// run over the first `run_width - width` run bytes. The Unwind keeps the
/// current global counter; it equals the saved counter because the Forward
/// jump never changes it.
pub proof fn payload_run_unwind_selects_scalar_prefix(
    before: epm::EpmFrame,
    run: restore::RestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    width: usize,
)
    requires
        restore::run_wf(run),
        0 < width as int <= restore::run_width(run),
        !before.path.root_left,
        before.path.parser.next_obligation == run.base_parser.next_obligation,
    ensures
        ({
            let kept = restore::run_width(run) - width as int;
            let after = batch::unwind_batch_after(
                before, batch::historical_run_from_payload(run), prefix, width,
            );
            &&& after.path == forward_virtual_path(before.path, prefix, run, kept)
            &&& after.path == scalar_payload_edge_run(
                forward_virtual_path(before.path, prefix, run, 0),
                run_bytes(run, 0, kept),
                run_anchors(run, 0, kept),
            )
        }),
{
    hide(parser::feed_parser_byte);
    let first = run.first_depth as int;
    let historical = batch::historical_run_from_payload(run);
    let kept = restore::run_width(run) - width as int;
    let after = batch::unwind_batch_after(before, historical, prefix, width);
    let selected = forward_virtual_path(before.path, prefix, run, kept);
    assert(batch::historical_run_width(historical) == restore::run_width(run));
    assert(historical.checkpoints[kept] == restore::run_checkpoint(run, kept));
    forward_payload_root_projection(run.base_parser, run.current.bytes.subrange(first, first + kept));
    assert(run.current.bytes.len() - width as int == first + kept);
    assert(after.path.parser == selected.parser);
    assert(after.path == selected);
    forward_virtual_suffix_is_scalar_payload_edges(before.path, prefix, run, 0, kept);
}

/// L composed with E. After an accepted fresh Forward batch, every Unwind of
/// `unwind_width <= width` over its published run yields the scalar
/// Payload-edge run over the first `width - unwind_width` batch bytes; a full
/// Unwind restores the pre-batch path exactly. Each intermediate Unwind frame
/// (each scalar Leave of `accepted_unwind_batch_has_scalar_restore_steps`) is
/// the virtual path of the corresponding scalar prefix.
pub proof fn fresh_forward_then_unwind_restores_scalar_prefix(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
    unwind_width: usize,
)
    requires
        batch::forward_batch_source_guard(
            before, start, width, pending, state_id, latest_restore_state_id, facts,
        ),
        0 < unwind_width <= width,
    ensures
        ({
            let run = fresh_forward_run(before, pending, state_id, start, width);
            let forwarded = batch::forward_batch_after(before, start, width, pending, state_id);
            let historical = batch::historical_run_from_payload(run);
            let kept = width as int - unwind_width as int;
            let source = batch::trie_source(before);
            let unwound = batch::unwind_batch_after(
                forwarded, historical, before.path.restore, unwind_width,
            );
            &&& forwarded.path.restore == before.path.restore + historical.checkpoints
            &&& unwound.path == scalar_payload_edge_run(
                before.path,
                source.bytes.subrange(start as int, start as int + kept),
                source.positions.subrange(start as int, start as int + kept),
            )
            &&& (unwind_width == width ==> unwound.path == before.path)
            &&& forall|step: usize| 1 <= step <= unwind_width ==>
                #[trigger] batch::unwind_virtual_frame(
                    forwarded, historical, before.path.restore, step,
                ).path == forward_virtual_path(
                    before.path, before.path.restore, run, width as int - step as int,
                )
        }),
{
    hide(parser::feed_parser_byte);
    hide(batch::forward_batch_after);
    hide(batch::unwind_batch_after);
    hide(forward_virtual_path);
    hide(scalar_payload_edge_run);
    fresh_forward_run_starts_at_pre_batch_path(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    );
    fresh_forward_run_ends_at_published_path(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    );
    let run = fresh_forward_run(before, pending, state_id, start, width);
    let forwarded = batch::forward_batch_after(before, start, width, pending, state_id);
    let historical = batch::historical_run_from_payload(run);
    let kept = width as int - unwind_width as int;
    let source = batch::trie_source(before);
    let w = width as int;
    let prefix = before.path.restore;
    forward_virtual_path_projection(before.path, prefix, run, w);
    assert(restore::expanded_run(run).subrange(0, w) =~= restore::expanded_run(run));
    assert(forwarded.path.restore == prefix + historical.checkpoints);
    assert(restore::run_width(run) - unwind_width as int == kept);
    forward_virtual_path_template_swap(forwarded.path, before.path, prefix, run, 0);
    forward_virtual_path_template_swap(forwarded.path, before.path, prefix, run, kept);
    payload_run_unwind_selects_scalar_prefix(forwarded, run, prefix, unwind_width);
    assert(run_bytes(run, 0, kept) =~= source.bytes.subrange(start as int, start as int + kept));
    assert(run_anchors(run, 0, kept)
        =~= source.positions.subrange(start as int, start as int + kept));
    assert forall|step: usize| 1 <= step <= unwind_width implies
        #[trigger] batch::unwind_virtual_frame(forwarded, historical, prefix, step).path
            == forward_virtual_path(before.path, prefix, run, width as int - step as int) by {
        payload_run_unwind_selects_scalar_prefix(forwarded, run, prefix, step);
        assert(restore::run_width(run) - step as int == width as int - step as int);
        forward_virtual_path_template_swap(
            forwarded.path, before.path, prefix, run, width as int - step as int,
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Saved-handle parser images
// ---------------------------------------------------------------------------

/// Arena bytes with their provenance, as `parser_source_image_for` lists them.
pub open spec fn located_path_bytes(
    bytes: Seq<u8>,
    anchors: Seq<epm::SourceAnchor>,
) -> Seq<parser::ParserLocated> {
    Seq::new(bytes.len(), |i: int| parser::ParserLocated {
        byte: bytes[i],
        source: parser::ParserSource { source: anchors[i].source, offset: anchors[i].offset },
    })
}

/// Image of a saved handle that shares the persistent state of a base handle.
/// Rust `parser_source_image_for(handle)` reads `state(handle)` =
/// `PersistentCheckpoints::get(handle.state, handle.bytes.len())`, which moves
/// the stamp of a fault-free, non-dead state to the handle depth; its path
/// bytes are the handle's arena slice; frames and the owner chain (absolute
/// boundaries) are those of the shared state; the counter is the global one.
pub open spec fn rebased_parser_source(
    base: parser::ParserSourceSnapshot,
    extension: Seq<parser::ParserLocated>,
    next_obligation: u64,
) -> parser::ParserSourceSnapshot {
    parser::ParserSourceSnapshot {
        next_obligation,
        stamp: (base.stamp + extension.len()) as usize,
        path_bytes: base.path_bytes + extension,
        ..base
    }
}

/// S(i). With the snapshot counter equal to the current global counter, the
/// historical saved-image relation is exactly the full source match of the
/// parser image that Leave restores from the checkpoint.
pub proof fn saved_image_matches_restored_parser_iff(
    source: parser::ParserSourceSnapshot,
    saved: parser::ParserImage,
    current: parser::ParserImage,
)
    requires
        source.next_obligation == current.next_obligation,
    ensures
        historical::source_parser_matches_saved_checkpoint(source, saved) <==> (
        saved.next_obligation <= current.next_obligation && parser::source_parser_matches(
            source,
            parser::restore_parser_checkpoint(saved, current),
        )),
{
    assert(parser::restore_parser_checkpoint(saved, current) == parser::ParserImage {
        next_obligation: source.next_obligation,
        ..saved
    });
}

/// Re-reading the global counter changes no other conjunct of the match.
pub proof fn counter_reread_preserves_parser_source_match(
    source: parser::ParserSourceSnapshot,
    image: parser::ParserImage,
    next_obligation: u64,
)
    requires
        parser::source_parser_matches(source, image),
    ensures
        parser::source_parser_matches(
            parser::ParserSourceSnapshot { next_obligation, ..source },
            parser::ParserImage { next_obligation, ..image },
        ),
{
    let reread = parser::ParserSourceSnapshot { next_obligation, ..source };
    let restored = parser::ParserImage { next_obligation, ..image };
    assert(parser::parser_root(restored) == parser::parser_root(image));
    assert(parser::parser_root_gate(restored) == parser::parser_root_gate(image));
    assert forall|i: int| 0 <= i < reread.owner_chain.len() implies
        #[trigger] parser::source_owner_matches(reread, i, restored.owners[i]) by {
        assert(parser::source_owner_matches(source, i, image.owners[i]));
    }
}

/// S(ii). The rebased image of a matching base handle matches the Forward
/// jump over the extension bytes (which section 2 equates with the scalar
/// byte run): owner residuals are absolute boundaries minus the stamp, and
/// owner child bytes are the arena slice from the owner start to the stamp.
pub proof fn rebased_saved_image_matches_forward_payload(
    base: parser::ParserSourceSnapshot,
    image: parser::ParserImage,
    extension: Seq<parser::ParserLocated>,
)
    requires
        parser::source_parser_matches(base, image),
        base.path_bytes.len() == base.stamp,
        parser::parser_forward_capacity(image, extension.len() as int),
    ensures
        parser::source_parser_matches(
            rebased_parser_source(base, extension, base.next_obligation),
            parser::forward_parser_payload(image, parser::located_values(extension)),
        ),
{
    let width = extension.len() as int;
    let values = parser::located_values(extension);
    let rebased = rebased_parser_source(base, extension, base.next_obligation);
    let after = parser::forward_parser_payload(image, values);
    assert(values.len() == width);
    forward_payload_root_projection(image, values);
    assert(rebased.stamp as int == base.stamp as int + width);
    assert(rebased.path_bytes.len() == rebased.stamp as int);
    assert forall|i: int| 0 <= i < rebased.owner_chain.len() implies
        #[trigger] parser::source_owner_matches(rebased, i, after.owners[i]) by {
        assert(parser::source_owner_matches(base, i, image.owners[i]));
        let modeled = image.owners[i];
        let owner = base.owner_chain[i];
        assert(modeled.remaining as int > width);
        assert(modeled.path_remaining as int > width);
        assert(after.owners[i].remaining as int == modeled.remaining as int - width);
        assert(after.owners[i].path_remaining as int == modeled.path_remaining as int - width);
        assert(owner.region_end as int == base.stamp as int + modeled.remaining as int);
        assert(owner.path_end as int == base.stamp as int + modeled.path_remaining as int);
        assert(rebased.path_bytes.subrange(modeled.start_depth as int, rebased.stamp as int)
            =~= base.path_bytes.subrange(modeled.start_depth as int, base.stamp as int)
            + extension);
        assert(parser::located_values(
            rebased.path_bytes.subrange(modeled.start_depth as int, rebased.stamp as int),
        ) =~= parser::located_values(
            base.path_bytes.subrange(modeled.start_depth as int, base.stamp as int),
        ) + values);
        assert(after.owners[i].child_bytes == modeled.child_bytes + values);
    }
}

/// S(iii). Saved-handle images inside a payload run. Rust premises: the base
/// handle's image matches the run's base parser exactly, with the arena prefix
/// as path bytes; every handle of the run shares that persistent state, so its
/// image is the rebase by the run bytes with the re-read counter. Then the
/// image at every run depth satisfies the historical module's parser premise
/// (`source_parser_matches_saved_checkpoint`) against the formal checkpoint,
/// whose parser is the scalar byte run of the restored prefix, and its path
/// bytes are the arena prefix of that depth.
pub proof fn payload_run_rebased_images_match_checkpoints(
    run: restore::RestoreRunImage,
    base: parser::ParserSourceSnapshot,
    counter: u64,
    index: int,
)
    requires
        restore::run_wf(run),
        0 <= index < restore::run_width(run),
        parser::source_parser_matches(base, run.base_parser),
        base.path_bytes == located_path_bytes(
            run.current.bytes.subrange(0, run.first_depth as int),
            run.current.positions.subrange(0, run.first_depth as int),
        ),
        run.base_parser.next_obligation <= counter,
    ensures
        ({
            let image = rebased_parser_source(
                base,
                located_path_bytes(run_bytes(run, 0, index), run_anchors(run, 0, index)),
                counter,
            );
            &&& historical::source_parser_matches_saved_checkpoint(
                image,
                restore::run_checkpoint(run, index).parser,
            )
            &&& image.stamp as int == run.first_depth as int + index
            &&& image.path_bytes == located_path_bytes(
                run.current.bytes.subrange(0, run.first_depth as int + index),
                run.current.positions.subrange(0, run.first_depth as int + index),
            )
            &&& restore::run_checkpoint(run, index).parser == plain_parser_byte_run(
                run.base_parser,
                run_bytes(run, 0, index),
                parser_sources(run_anchors(run, 0, index)),
                run.first_depth as int,
            )
        }),
{
    hide(parser::feed_parser_byte);
    let first = run.first_depth as int;
    let width = restore::run_width(run);
    let extension = located_path_bytes(run_bytes(run, 0, index), run_anchors(run, 0, index));
    let values = parser::located_values(extension);
    let checkpoint = restore::run_checkpoint(run, index).parser;
    let image = rebased_parser_source(base, extension, counter);
    assert(values =~= run_bytes(run, 0, index));
    assert(run_bytes(run, 0, index) =~= run.current.bytes.subrange(first, first + index));
    assert(checkpoint == parser::forward_parser_payload(run.base_parser, values));
    assert(base.path_bytes.len() == base.stamp);
    if index == 0 {
        assert(values =~= Seq::<u8>::empty());
        forward_payload_without_bytes_is_identity(run.base_parser);
        assert(image.path_bytes =~= base.path_bytes);
        assert(image == parser::ParserSourceSnapshot { next_obligation: counter, ..base });
        counter_reread_preserves_parser_source_match(base, run.base_parser, counter);
    } else {
        forward_capacity_narrows_to_shorter_width(run.base_parser, width, index);
        rebased_saved_image_matches_forward_payload(base, run.base_parser, extension);
        let exact = rebased_parser_source(base, extension, base.next_obligation);
        counter_reread_preserves_parser_source_match(exact, checkpoint, counter);
        assert(image == parser::ParserSourceSnapshot { next_obligation: counter, ..exact });
    }
    forward_payload_root_projection(run.base_parser, values);
    assert(image.path_bytes =~= located_path_bytes(
        run.current.bytes.subrange(0, first + index),
        run.current.positions.subrange(0, first + index),
    ));
    payload_checkpoint_parser_is_plain_byte_prefix(run, index);
}

// ---------------------------------------------------------------------------
// 6. Scalar Edge and Leave: one byte transition plus one checkpoint
// ---------------------------------------------------------------------------

/// One scalar Edge on the path, whatever its effect: one `feed_parser_byte`
/// step at path length `|path| + 1`, and one push of the pre-edge checkpoint.
pub open spec fn scalar_edge_path_step(
    before: epm::PathTopology,
    after: epm::PathTopology,
    byte: u8,
    source: epm::SourceAnchor,
) -> bool {
    let step = parser::feed_parser_byte(
        before.parser,
        byte,
        parser::ParserSource { source: source.source, offset: source.offset },
        (before.path_bytes.len() + 1) as usize,
    );
    &&& !before.root_left
    &&& before.path_bytes.len() < usize::MAX as int
    &&& before.next_obligation == before.parser.next_obligation
    &&& epm::parser_gate_matches(before.gate, before.parser)
    &&& after.parser == step.image
    &&& after.next_obligation == step.image.next_obligation
    &&& epm::parser_gate_matches(after.gate, after.parser)
    &&& after.root_left == before.root_left
    &&& after.path_bytes == before.path_bytes.push(byte)
    &&& after.path_positions == before.path_positions.push(source)
    &&& after.restore == before.restore.push(scalar_edge_checkpoint(before))
}

/// The Leave successor fixed by `leave_return` when the restore stack is
/// nonempty: pop the top checkpoint, restore its path prefix, obligation,
/// gate and parser, and keep the current global counter.
pub open spec fn scalar_leave_path(before: epm::PathTopology) -> epm::PathTopology {
    let checkpoint = before.restore.last();
    epm::PathTopology {
        next_obligation: before.next_obligation,
        pending: checkpoint.pending,
        root_left: false,
        restore: before.restore.drop_last(),
        path_bytes: before.path_bytes.subrange(0, checkpoint.path_len as int),
        path_positions: before.path_positions.subrange(0, checkpoint.path_len as int),
        gate: checkpoint.gate,
        parser: parser::restore_parser_checkpoint(checkpoint.parser, before.parser),
    }
}

/// D. Every scalar Edge is one byte transition plus one checkpoint push. A
/// Payload Edge's successor is `scalar_payload_edge_path`; Plain and Payload
/// keep the counter and Start advances it by one.
pub proof fn edge_return_is_scalar_edge_path_step(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    byte: u8,
    source_offset: usize,
    source: epm::SourceAnchor,
    origin: epm::EdgeOrigin,
    effect: epm::PathEdgeEffect,
    admission_next_id: u64,
    action: epm::EpmAction,
)
    requires
        epm::edge_return(
            before, after, byte, source_offset, source, origin, effect, admission_next_id, action,
        ),
    ensures
        scalar_edge_path_step(before.path, after.path, byte, source),
        effect is Payload ==> after.path == scalar_payload_edge_path(before.path, byte, source),
        effect is Start ==> after.path.next_obligation == before.path.next_obligation + 1,
        !(effect is Start) ==> after.path.next_obligation == before.path.next_obligation,
{
    hide(parser::feed_parser_byte);
    match effect {
        epm::PathEdgeEffect::Payload => {
            let expected = scalar_payload_edge_path(before.path, byte, source);
            assert(after.path.pending == expected.pending);
            assert(after.path == expected);
        },
        _ => {},
    }
}

pub proof fn leave_return_selects_scalar_leave_path(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    source: epm::SourceAnchor,
)
    requires
        epm::leave_return(before, after, source),
        before.path.restore.len() > 0,
    ensures
        after.path == scalar_leave_path(before.path),
{
}

/// The Leave that follows a scalar Edge pops exactly the pushed checkpoint:
/// it restores the pre-edge path except for the global counter, and restores
/// it exactly when the Edge kept the counter. The checkpoint records only the
/// byte length and Leave cuts the anchors at it, so the anchors come back
/// exactly under the layout invariant `|path_bytes| = |path_positions|`.
pub proof fn scalar_edge_then_leave_restores_checkpoint(
    before: epm::PathTopology,
    after: epm::PathTopology,
    byte: u8,
    source: epm::SourceAnchor,
)
    requires
        scalar_edge_path_step(before, after, byte, source),
        before.path_bytes.len() == before.path_positions.len(),
    ensures
        scalar_leave_path(after) == (epm::PathTopology {
            next_obligation: after.next_obligation,
            parser: parser::restore_parser_checkpoint(before.parser, after.parser),
            ..before
        }),
        after.next_obligation == before.next_obligation ==> scalar_leave_path(after) == before,
{
    hide(parser::feed_parser_byte);
    let restored = scalar_leave_path(after);
    assert(after.restore.last() == scalar_edge_checkpoint(before));
    assert(restored.restore =~= before.restore);
    assert(restored.path_bytes =~= before.path_bytes);
    assert(restored.path_positions =~= before.path_positions);
    if after.next_obligation == before.next_obligation {
        assert(restored.parser == before.parser);
    }
}

/// D, frame level: an `edge_return` followed by a `leave_return` restores the
/// pre-edge path, exactly for Plain and Payload Edges and up to the global
/// counter for Start.
pub proof fn edge_return_then_leave_return_round_trip(
    before: epm::EpmFrame,
    middle: epm::EpmFrame,
    after: epm::EpmFrame,
    byte: u8,
    source_offset: usize,
    source: epm::SourceAnchor,
    origin: epm::EdgeOrigin,
    effect: epm::PathEdgeEffect,
    admission_next_id: u64,
    action: epm::EpmAction,
    leave_source: epm::SourceAnchor,
)
    requires
        epm::edge_return(
            before, middle, byte, source_offset, source, origin, effect, admission_next_id, action,
        ),
        epm::leave_return(middle, after, leave_source),
    ensures
        after.path == (epm::PathTopology {
            next_obligation: middle.path.next_obligation,
            parser: parser::restore_parser_checkpoint(before.path.parser, middle.path.parser),
            ..before.path
        }),
        !(effect is Start) ==> after.path == before.path,
{
    hide(parser::feed_parser_byte);
    edge_return_is_scalar_edge_path_step(
        before, middle, byte, source_offset, source, origin, effect, admission_next_id, action,
    );
    leave_return_selects_scalar_leave_path(middle, after, leave_source);
    assert(before.path.path_bytes.len() == before.path.path_positions.len());
    scalar_edge_then_leave_restores_checkpoint(before.path, middle.path, byte, source);
}

/// The checkpoint pushed by a scalar Edge has a saved-handle image: if the
/// Rust snapshot of the pre-edge handle matches the pre-edge parser, the same
/// handle re-read at any later global counter satisfies the historical
/// saved-image relation against the checkpoint's parser.
pub proof fn scalar_edge_checkpoint_has_saved_image(
    before: epm::PathTopology,
    source: parser::ParserSourceSnapshot,
    counter: u64,
)
    requires
        parser::source_parser_matches(source, before.parser),
        before.parser.next_obligation <= counter,
    ensures
        historical::source_parser_matches_saved_checkpoint(
            parser::ParserSourceSnapshot { next_obligation: counter, ..source },
            scalar_edge_checkpoint(before).parser,
        ),
{
    counter_reread_preserves_parser_source_match(source, before.parser, counter);
}

// ---------------------------------------------------------------------------
// 7. The optimized CanonicalPath bulk skip is a scan of scalar Edges
// ---------------------------------------------------------------------------

/// Edge actions on one frame, as consumed by `canonical_path_driver_scan_fold`.
pub open spec fn canonical_edge_actions(
    index: int,
    count: nat,
) -> Seq<common::CanonicalPathInternalAction> {
    Seq::new(count, |i: int| common::CanonicalPathInternalAction::Edge { index })
}

/// The first `count` interior bytes after the selected (post-first-Edge) slot.
pub open spec fn canonical_skip_bytes(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    count: int,
) -> Seq<u8> {
    let selected = edge.parser_frames[top].unwrap();
    let frame = edge.live.path_frames[top].unwrap();
    frame.input.bytes.subrange(selected.bytes.len() as int, selected.bytes.len() + count)
}

pub open spec fn canonical_skip_sources(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    count: int,
) -> Seq<parser::ParserSource> {
    let selected = edge.parser_frames[top].unwrap();
    let frame = edge.live.path_frames[top].unwrap();
    Seq::new(count as nat, |i: int| {
        let anchor = frame.input.positions[selected.positions.len() + i];
        parser::ParserSource { source: anchor.source, offset: anchor.offset }
    })
}

/// State after the first scalar Edge of a bulk loop event and `count`
/// further scalar Edges over the opaque interior, in closed form.
pub open spec fn canonical_skip_virtual_state(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    count: int,
) -> common::LiveCanonicalPathCommonState {
    let selected = edge.parser_frames[top].unwrap();
    common::LiveCanonicalPathCommonState {
        parser_frames: edge.parser_frames.update(top, Some(common::CanonicalParserSlot {
            parser: parser::forward_parser_payload(
                selected.parser,
                canonical_skip_bytes(edge, top, count),
            ),
            bytes: selected.bytes + canonical_skip_bytes(edge, top, count),
            positions: selected.positions + canonical_skip_sources(edge, top, count),
            cursor_traversed: (selected.cursor_traversed + count) as usize,
        })),
        ..edge
    }
}

/// The same state with the top slot's traversed-count replaced.
pub open spec fn canonical_slot_with_traversed(
    state: common::LiveCanonicalPathCommonState,
    top: int,
    traversed: usize,
) -> common::LiveCanonicalPathCommonState {
    common::LiveCanonicalPathCommonState {
        parser_frames: state.parser_frames.update(top, Some(common::CanonicalParserSlot {
            cursor_traversed: traversed,
            ..state.parser_frames[top].unwrap()
        })),
        ..state
    }
}

/// Facts of an accepted bulk event about the state after its first Edge.
pub open spec fn canonical_skip_premises(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    skip: int,
) -> bool {
    let selected = edge.parser_frames[top].unwrap();
    let frame = edge.live.path_frames[top].unwrap();
    &&& 0 <= top < edge.parser_frames.len()
    &&& edge.parser_frames[top].is_some()
    &&& edge.live.path_frames[top].is_some()
    &&& edge.live.outstanding[top].is_none()
    &&& edge.selected_values[top].is_none()
    &&& parser::parser_forward_capacity(selected.parser, skip)
    &&& selected.parser.stamp as int == selected.bytes.len()
    &&& selected.positions.len() == selected.bytes.len()
    &&& selected.cursor_traversed as int <= selected.bytes.len()
    &&& selected.bytes.len() + skip <= frame.input.bytes.len()
}

/// One scalar CanonicalPath Edge maps the closed form at `count` to the
/// closed form at `count + 1`.
pub proof fn canonical_skip_virtual_step_is_internal_edge(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    skip: int,
    count: int,
)
    requires
        common::live_canonical_path_common_wf(canonical_skip_virtual_state(edge, top, count)),
        canonical_skip_premises(edge, top, skip),
        0 <= count < skip,
    ensures
        common::canonical_path_internal_edge_next(canonical_skip_virtual_state(edge, top, count), top)
            == Some(canonical_skip_virtual_state(edge, top, count + 1)),
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
    let selected = edge.parser_frames[top].unwrap();
    let frame = edge.live.path_frames[top].unwrap();
    let from = selected.bytes.len() as int;
    let all = canonical_skip_bytes(edge, top, skip);
    let current = canonical_skip_virtual_state(edge, top, count);
    let next = canonical_skip_virtual_state(edge, top, count + 1);
    let slot = current.parser_frames[top].unwrap();
    let next_slot = next.parser_frames[top].unwrap();
    let at = from + count;
    assert(slot.bytes.len() == at);
    assert(slot.positions.len() == at);
    let anchor = frame.input.positions[at];
    let source = parser::ParserSource { source: anchor.source, offset: anchor.offset };
    let byte = frame.input.bytes[at];
    assert(all.len() == skip);
    assert(all.subrange(0, count) =~= canonical_skip_bytes(edge, top, count));
    assert(all.subrange(0, count + 1) =~= canonical_skip_bytes(edge, top, count + 1));
    assert(all[count] == byte);
    forward_capacity_step_feeds_reusable_byte(selected.parser, all, count, source);
    forward_payload_root_projection(selected.parser, canonical_skip_bytes(edge, top, count));
    forward_payload_root_projection(selected.parser, canonical_skip_bytes(edge, top, count + 1));
    assert((at + 1) as usize == (selected.parser.stamp as int + count + 1) as usize);
    assert(!parser::parser_waiting_obligation(slot.parser.focus));
    let step = parser::feed_parser_byte(slot.parser, byte, source, (at + 1) as usize);
    assert(step.image == next_slot.parser);
    assert(!parser::parser_root_gate(step.image).dead);
    let expected_slot = common::CanonicalParserSlot {
        parser: step.image,
        bytes: slot.bytes.push(byte),
        positions: slot.positions.push(source),
        cursor_traversed: (slot.cursor_traversed + 1) as usize,
    };
    assert(expected_slot.bytes =~= next_slot.bytes);
    assert(canonical_skip_sources(edge, top, count + 1)
        =~= canonical_skip_sources(edge, top, count).push(source));
    assert(expected_slot.positions =~= next_slot.positions);
    assert(expected_slot == next_slot);
    assert(current.parser_frames.update(top, Some(expected_slot)) =~= next.parser_frames);
}

/// Forward induction: every closed form up to the skip width is well formed,
/// and the closed form at 0 is the state after the first Edge.
pub proof fn canonical_skip_virtual_prefix_is_wf(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    skip: int,
    count: int,
)
    requires
        common::live_canonical_path_common_wf(edge),
        canonical_skip_premises(edge, top, skip),
        0 <= count <= skip,
    ensures
        common::live_canonical_path_common_wf(canonical_skip_virtual_state(edge, top, count)),
        canonical_skip_virtual_state(edge, top, 0) == edge,
    decreases count,
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
    let selected = edge.parser_frames[top].unwrap();
    let initial = canonical_skip_virtual_state(edge, top, 0);
    assert(canonical_skip_bytes(edge, top, 0) =~= Seq::<u8>::empty());
    forward_payload_without_bytes_is_identity(selected.parser);
    assert(initial.parser_frames[top].unwrap().bytes =~= selected.bytes);
    assert(initial.parser_frames[top].unwrap().positions =~= selected.positions);
    assert(initial.parser_frames[top].unwrap() == selected);
    assert(initial.parser_frames =~= edge.parser_frames);
    if count > 0 {
        canonical_skip_virtual_prefix_is_wf(edge, top, skip, count - 1);
        canonical_skip_virtual_step_is_internal_edge(edge, top, skip, count - 1);
        common::canonical_path_internal_edge_preserves_wf(
            canonical_skip_virtual_state(edge, top, count - 1),
            top,
        );
    }
}

/// Head-first induction: from the closed form at `count`, the driver scan of
/// the remaining interior Edges reaches the closed form at the skip width.
pub proof fn canonical_skip_virtual_suffix_scans_to_end(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    skip: int,
    count: int,
)
    requires
        common::live_canonical_path_common_wf(edge),
        canonical_skip_premises(edge, top, skip),
        top == edge.live.path_frames.len() - 1,
        0 <= count <= skip,
    ensures
        common::canonical_path_driver_scan_fold(
            canonical_skip_virtual_state(edge, top, count),
            canonical_edge_actions(top, (skip - count) as nat),
        ) == Some(canonical_skip_virtual_state(edge, top, skip)),
    decreases skip - count,
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
    if count < skip {
        canonical_skip_virtual_suffix_scans_to_end(edge, top, skip, count + 1);
        canonical_skip_virtual_prefix_is_wf(edge, top, skip, count);
        canonical_skip_virtual_step_is_internal_edge(edge, top, skip, count);
        let actions = canonical_edge_actions(top, (skip - count) as nat);
        assert(actions.subrange(1, actions.len() as int)
            =~= canonical_edge_actions(top, (skip - (count + 1)) as nat));
    } else {
        assert(canonical_edge_actions(top, (skip - count) as nat).len() == 0);
    }
}

/// C. An accepted CanonicalPath loop event with an opaque-interior skip of
/// width k > 0 is the driver scan of 1 + k scalar Edges on the top frame
/// (`canonical_path_driver_scan_fold`, each step `canonical_path_internal_edge_next`),
/// field for field except the top slot's `cursor_traversed`, which the bulk
/// event takes from the observed cursor. It equals the scan exactly when the
/// observed count is the selected count + k, and the slot parser is the
/// scalar byte run of section 2 over the skipped bytes.
pub proof fn canonical_bulk_skip_is_scalar_edge_scan(
    before: common::LiveCanonicalPathCommonState,
    observed: common::CanonicalPathLoopSourceEvent,
)
    requires
        common::canonical_path_loop_event_next(before, observed).is_some(),
        observed.skipped > 0,
    ensures
        ({
            let top = before.parser_frames.len() - 1;
            let edge = common::canonical_path_internal_edge_next(before, top).unwrap();
            let selected = edge.parser_frames[top].unwrap();
            let skip = observed.skipped as int;
            let scalar = canonical_skip_virtual_state(edge, top, skip);
            let bulk = common::canonical_path_loop_event_next(before, observed).unwrap();
            &&& common::canonical_path_internal_edge_next(before, top).is_some()
            &&& common::canonical_path_driver_scan_fold(
                before,
                canonical_edge_actions(top, (skip + 1) as nat),
            ) == Some(scalar)
            &&& common::live_canonical_path_common_wf(scalar)
            &&& bulk == canonical_slot_with_traversed(scalar, top, observed.traversed_after)
            &&& (observed.traversed_after as int == selected.cursor_traversed as int + skip
                ==> bulk == scalar)
            &&& scalar.parser_frames[top].unwrap().parser == plain_parser_byte_run(
                selected.parser,
                canonical_skip_bytes(edge, top, skip),
                canonical_skip_sources(edge, top, skip),
                selected.parser.stamp as int,
            )
        }),
{
    hide(parser::feed_parser_byte);
    let top = before.parser_frames.len() - 1;
    let edge = common::canonical_path_internal_edge_next(before, top).unwrap();
    let selected = edge.parser_frames[top].unwrap();
    let skip = observed.skipped as int;
    let scalar = canonical_skip_virtual_state(edge, top, skip);
    let bulk = common::canonical_path_loop_event_next(before, observed).unwrap();
    assert(common::live_canonical_path_common_wf(before));
    assert(before.parser_frames.len() == before.live.path_frames.len());
    common::canonical_path_internal_edge_preserves_wf(before, top);
    assert(common::parser_slot_matches(edge, top));
    assert(common::parser_slot_matches(before, top));
    assert(canonical_skip_premises(edge, top, skip));
    canonical_skip_virtual_prefix_is_wf(edge, top, skip, skip);
    canonical_skip_virtual_suffix_scans_to_end(edge, top, skip, 0);
    let actions = canonical_edge_actions(top, (skip + 1) as nat);
    assert(actions.subrange(1, actions.len() as int) =~= canonical_edge_actions(top, skip as nat));
    let frame = before.live.path_frames[top].unwrap();
    assert(canonical_skip_sources(edge, top, skip) =~= Seq::new(observed.skipped as nat, |i: int| {
        let anchor = frame.input.positions[selected.positions.len() + i];
        parser::ParserSource { source: anchor.source, offset: anchor.offset }
    }));
    let replaced = canonical_slot_with_traversed(scalar, top, observed.traversed_after);
    assert(replaced.parser_frames =~= bulk.parser_frames);
    assert(bulk == replaced);
    if observed.traversed_after as int == selected.cursor_traversed as int + skip {
        assert(replaced.parser_frames =~= scalar.parser_frames);
    }
    forward_payload_equals_plain_byte_run(
        selected.parser,
        canonical_skip_bytes(edge, top, skip),
        canonical_skip_sources(edge, top, skip),
    );
}

// ---------------------------------------------------------------------------
// 8. Non-vacuity witnesses
// ---------------------------------------------------------------------------

/// Focused Payload frame of a three-byte RawPar escape whose tag is at
/// (1, 0) and whose length byte ends at depth 2 (bytes 0f 03 aa bb cc).
pub open spec fn opaque_payload_frame() -> parser::ParserFrame {
    parser::ParserFrame::Obligation {
        id: 0,
        kind: parser::ParserObligationKind::RawParEscape,
        payload_len: 3,
        tag_source: parser::ParserSource { source: 1, offset: 0 },
        payload_start: 2,
        status: parser::ParserObligationStatus::Payload,
    }
}

pub open spec fn opaque_payload_parser() -> parser::ParserImage {
    parser::ParserImage {
        focus: parser::ParserPath {
            status: parser::ParserStatus::Open,
            completed_segments: 0,
            first_tag: Some((0x0fu8, parser::ParserSource { source: 1, offset: 0 })),
            stable: true,
            escape_allowed: true,
            stack: seq![opaque_payload_frame()],
        },
        owners: Seq::empty(),
        next_obligation: 1,
        stamp: 2,
        fault: false,
    }
}

pub open spec fn opaque_interior_anchors() -> Seq<epm::SourceAnchor> {
    seq![epm::SourceAnchor { source: 1, offset: 2 }, epm::SourceAnchor { source: 1, offset: 3 }]
}

/// A NestedEpm1 payload inside one active Region owner (not claimed
/// reachable): exercises the owner residual and child-byte part of the jump.
pub open spec fn owner_payload_parser() -> parser::ParserImage {
    parser::ParserImage {
        focus: parser::ParserPath {
            status: parser::ParserStatus::Open,
            completed_segments: 0,
            first_tag: Some((0x10u8, parser::ParserSource { source: 1, offset: 3 })),
            stable: true,
            escape_allowed: true,
            stack: seq![parser::ParserFrame::Obligation {
                id: 2,
                kind: parser::ParserObligationKind::NestedEpm1,
                payload_len: 4,
                tag_source: parser::ParserSource { source: 1, offset: 3 },
                payload_start: 5,
                status: parser::ParserObligationStatus::Payload,
            }],
        },
        owners: seq![parser::ParserRegionOwner {
            parent: parser::ParserPath {
                status: parser::ParserStatus::Open,
                completed_segments: 0,
                first_tag: None,
                stable: true,
                escape_allowed: true,
                stack: seq![parser::ParserFrame::RegionActive],
            },
            remaining: 5,
            previous: None,
            declared_len: 6,
            path_remaining: 4,
            start_depth: 3,
            child_bytes: seq![0x10u8, 0x04u8],
        }],
        next_obligation: 3,
        stamp: 5,
        fault: false,
    }
}

/// P on concrete images, with and without an active Region owner.
pub proof fn opaque_and_owner_parser_witness()
    ensures
        parser::parser_forward_capacity(opaque_payload_parser(), 2),
        plain_parser_byte_run(
            opaque_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 2,
        ) == parser::forward_parser_payload(opaque_payload_parser(), seq![0xaau8, 0xbbu8]),
        parser::forward_parser_payload(opaque_payload_parser(), seq![0xaau8, 0xbbu8]).stamp == 4,
        plain_parser_byte_event(
            opaque_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 2, 1,
        ) == (parser::ParserEvent::ObligationByte { id: 0, last: false }),
        parser::parser_forward_capacity(owner_payload_parser(), 2),
        plain_parser_byte_run(
            owner_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 5,
        ) == parser::forward_parser_payload(owner_payload_parser(), seq![0xaau8, 0xbbu8]),
        parser::forward_parser_payload(owner_payload_parser(), seq![0xaau8, 0xbbu8]).owners[0]
            .child_bytes == seq![0x10u8, 0x04u8, 0xaau8, 0xbbu8],
        parser::forward_parser_payload(owner_payload_parser(), seq![0xaau8, 0xbbu8]).owners[0]
            .path_remaining == 2,
        plain_parser_byte_event(
            owner_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 5, 0,
        ) == (parser::ParserEvent::ObligationByte { id: 2, last: false }),
{
    hide(parser::feed_parser_byte);
    let bytes = seq![0xaau8, 0xbbu8];
    let sources = parser_sources(opaque_interior_anchors());
    assert(sources.len() == 2);
    let opaque = opaque_payload_parser();
    let owned = owner_payload_parser();
    assert(parser::parser_root(opaque) == opaque.focus);
    assert(parser::parser_forward_capacity(opaque, 2));
    forward_payload_equals_plain_byte_run(opaque, bytes, sources);
    assert(parser::parser_root(owned) == owned.owners[0].parent);
    assert(parser::parser_forward_capacity(owned, 2));
    forward_payload_equals_plain_byte_run(owned, bytes, sources);
    let after = parser::forward_parser_payload(owned, bytes);
    assert(after.owners[0].child_bytes =~= seq![0x10u8, 0x04u8, 0xaau8, 0xbbu8]);
}

pub open spec fn opaque_payload_pending() -> epm::PathObligation {
    epm::PathObligation {
        id: 0,
        kind: epm::PathObligationKind::RawParEscape,
        payload_len: 3,
        tag_source: epm::SourceAnchor { source: 1, offset: 0 },
        payload_start: 2,
        payload_end: epm::SourceAnchor { source: 1, offset: 1 },
        awaiting_resolution: false,
    }
}

pub open spec fn opaque_payload_gate() -> epm::PathGate {
    epm::PathGate {
        dead: false,
        stack_empty: false,
        split_complete: false,
        completed_segments: 0,
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    }
}

/// EPM path at depth 2 inside the opaque payload (restore prefix left empty;
/// not claimed reachable).
pub open spec fn opaque_payload_template() -> epm::PathTopology {
    epm::PathTopology {
        next_obligation: 1,
        pending: Some(opaque_payload_pending()),
        root_left: false,
        restore: Seq::empty(),
        path_bytes: seq![0x0fu8, 0x03u8],
        path_positions: seq![
            epm::SourceAnchor { source: 1, offset: 0 },
            epm::SourceAnchor { source: 1, offset: 1 },
        ],
        gate: opaque_payload_gate(),
        parser: opaque_payload_parser(),
    }
}

pub open spec fn opaque_interior_source() -> epm::IndexedRegionImage {
    epm::IndexedRegionImage {
        bytes: seq![0xaau8, 0xbbu8],
        positions: opaque_interior_anchors(),
        end: epm::SourceAnchor { source: 1, offset: 4 },
    }
}

/// The run a fresh Forward batch of width 2 publishes from the template.
pub open spec fn opaque_payload_run() -> restore::RestoreRunImage {
    let template = opaque_payload_template();
    restore::publish_forward_run(
        epm::SourceTreeImage { bytes: template.path_bytes, positions: template.path_positions },
        opaque_payload_pending(),
        opaque_payload_gate(),
        opaque_payload_parser(),
        7,
        opaque_interior_source(),
        0,
        2,
    )
}

pub proof fn opaque_payload_run_is_wf()
    ensures
        restore::run_wf(opaque_payload_run()),
        restore::run_width(opaque_payload_run()) == 2,
        run_bytes(opaque_payload_run(), 0, 2) == seq![0xaau8, 0xbbu8],
        run_anchors(opaque_payload_run(), 0, 2) == opaque_interior_anchors(),
{
    let template = opaque_payload_template();
    restore::accepted_forward_publishes_exact_virtual_checkpoints(
        epm::SourceTreeImage { bytes: template.path_bytes, positions: template.path_positions },
        opaque_payload_pending(),
        opaque_payload_gate(),
        opaque_payload_parser(),
        7,
        opaque_interior_source(),
        0,
        2,
        restore::ForwardCapacityFacts {
            source_present: true,
            root_left: false,
            state_open: true,
            fault_free: true,
            stamp: 2,
            owner_boundary_remaining: 2,
            edges_before: 0,
        },
    );
    let run = opaque_payload_run();
    assert(run_bytes(run, 0, 2) =~= seq![0xaau8, 0xbbu8]);
    assert(run_anchors(run, 0, 2) =~= opaque_interior_anchors());
}

/// E and L on a concrete run: the virtual path at 0 is the template, the
/// published path is the scalar Payload-edge run, both steps meet the
/// Payload clauses, and Unwinds of width 1 and 2 select the scalar prefixes.
pub proof fn epm_forward_path_witness()
    ensures
        ({
            let template = opaque_payload_template();
            let run = opaque_payload_run();
            let published = forward_virtual_path(template, template.restore, run, 2);
            let frame = epm::EpmFrame { path: published, ..batch::unwind_witness_before(None, true) };
            let historical = batch::historical_run_from_payload(run);
            &&& forward_virtual_path(template, template.restore, run, 0) == template
            &&& published == scalar_payload_edge_run(
                template, seq![0xaau8, 0xbbu8], opaque_interior_anchors(),
            )
            &&& published.restore.len() == 2
            &&& published.parser.stamp == 4
            &&& forward_virtual_step_clauses(template, template.restore, run, 0)
            &&& forward_virtual_step_clauses(template, template.restore, run, 1)
            &&& batch::unwind_batch_after(frame, historical, template.restore, 2).path == template
            &&& batch::unwind_batch_after(frame, historical, template.restore, 1).path
                == forward_virtual_path(template, template.restore, run, 1)
        }),
{
    hide(parser::feed_parser_byte);
    opaque_payload_run_is_wf();
    let template = opaque_payload_template();
    let run = opaque_payload_run();
    let initial = forward_virtual_path(template, template.restore, run, 0);
    assert(restore::pending_at_depth(run, 2) == opaque_payload_pending());
    assert(run.current.bytes.subrange(2, 2) =~= Seq::<u8>::empty());
    forward_payload_without_bytes_is_identity(opaque_payload_parser());
    assert(initial.restore =~= template.restore);
    assert(initial.path_bytes =~= template.path_bytes);
    assert(initial.path_positions =~= template.path_positions);
    assert(initial == template);
    forward_virtual_suffix_is_scalar_payload_edges(template, template.restore, run, 0, 2);
    assert(parser_payload_obligation_id(opaque_payload_parser()) == Some(0u64));
    forward_virtual_step_meets_payload_edge_clauses(template, template.restore, run, 0);
    forward_virtual_step_meets_payload_edge_clauses(template, template.restore, run, 1);
    let published = forward_virtual_path(template, template.restore, run, 2);
    forward_payload_root_projection(opaque_payload_parser(), run.current.bytes.subrange(2, 4));
    let frame = epm::EpmFrame { path: published, ..batch::unwind_witness_before(None, true) };
    payload_run_unwind_selects_scalar_prefix(frame, run, template.restore, 2);
    payload_run_unwind_selects_scalar_prefix(frame, run, template.restore, 1);
}

pub open spec fn opaque_payload_source_path() -> parser::ParserSourcePath {
    parser::ParserSourcePath {
        status: parser::ParserStatus::Open,
        completed_segments: 0,
        first_tag: Some(parser::ParserLocated {
            byte: 0x0f,
            source: parser::ParserSource { source: 1, offset: 0 },
        }),
        stable: true,
        escape_allowed: true,
        stack: Some(0),
    }
}

/// Read-only source image of the opaque payload handle at depth 2.
pub open spec fn opaque_payload_parser_source() -> parser::ParserSourceSnapshot {
    let template = opaque_payload_template();
    parser::ParserSourceSnapshot {
        next_obligation: 1,
        stamp: 2,
        path_bytes: located_path_bytes(template.path_bytes, template.path_positions),
        focused: opaque_payload_source_path(),
        root: opaque_payload_source_path(),
        owner_chain: Seq::empty(),
        frames: seq![parser::ParserSourceFrame::Obligation {
            id: 0,
            kind: parser::ParserObligationKind::RawParEscape,
            payload_len: 3,
            tag_source: parser::ParserSource { source: 1, offset: 0 },
            payload_start: 2,
            status: parser::ParserObligationStatus::Payload,
            next: None,
        }],
        fault: false,
        root_gate: parser::parser_root_gate(opaque_payload_parser()),
    }
}

/// S on concrete images: the base handle image matches the base parser, and
/// the rebased images at run depths 2 and 3, re-read at a later counter 2,
/// satisfy the historical saved-image relation against the checkpoints.
pub proof fn saved_handle_image_witness()
    ensures
        ({
            let run = opaque_payload_run();
            let base = opaque_payload_parser_source();
            let first_extension = located_path_bytes(
                run_bytes(run, 0, 1), run_anchors(run, 0, 1),
            );
            let empty_extension = located_path_bytes(
                run_bytes(run, 0, 0), run_anchors(run, 0, 0),
            );
            &&& parser::source_parser_matches(base, opaque_payload_parser())
            &&& historical::source_parser_matches_saved_checkpoint(
                rebased_parser_source(base, empty_extension, 2),
                restore::run_checkpoint(run, 0).parser,
            )
            &&& historical::source_parser_matches_saved_checkpoint(
                rebased_parser_source(base, first_extension, 2),
                restore::run_checkpoint(run, 1).parser,
            )
            &&& rebased_parser_source(base, first_extension, 2).stamp == 3
        }),
{
    hide(parser::feed_parser_byte);
    reveal_with_fuel(parser::source_stack_matches, 2);
    opaque_payload_run_is_wf();
    let run = opaque_payload_run();
    let base = opaque_payload_parser_source();
    let image = opaque_payload_parser();
    assert(image.focus.stack.drop_last().len() == 0);
    assert(parser::source_stack_matches(base.frames, None, image.focus.stack.drop_last()));
    assert(parser::source_path_matches(base.frames, base.focused, image.focus));
    assert(parser::parser_root(image) == image.focus);
    assert(parser::source_parser_matches(base, image));
    assert(run.current.bytes.subrange(0, 2) =~= opaque_payload_template().path_bytes);
    assert(run.current.positions.subrange(0, 2) =~= opaque_payload_template().path_positions);
    payload_run_rebased_images_match_checkpoints(run, base, 2, 0);
    payload_run_rebased_images_match_checkpoints(run, base, 2, 1);
}

/// D on a concrete scalar Edge with the real parser image: the Plain tag
/// 0x06 from the initial EPM path, its Leave, and the saved image of its
/// checkpoint (initial parser source snapshot).
pub proof fn plain_tag_edge_round_trip_witness()
    ensures
        ({
            let before = epm::initial_epm_path();
            let source = epm::SourceAnchor { source: 1, offset: 7 };
            let after = epm::initial_plain_tag_after(0x06, source);
            &&& scalar_edge_path_step(before, after, 0x06, source)
            &&& scalar_leave_path(after) == before
            &&& historical::source_parser_matches_saved_checkpoint(
                parser::initial_parser_source_snapshot(),
                scalar_edge_checkpoint(before).parser,
            )
        }),
{
    hide(parser::feed_parser_byte);
    epm::initial_plain_tags_refute_unchanged_gate();
    let before = epm::initial_epm_path();
    let source = epm::SourceAnchor { source: 1, offset: 7 };
    let after = epm::initial_plain_tag_after(0x06, source);
    assert(after.path_positions.last() == source);
    assert(after.restore == before.restore.push(scalar_edge_checkpoint(before)));
    assert(scalar_edge_path_step(before, after, 0x06, source));
    scalar_edge_then_leave_restores_checkpoint(before, after, 0x06, source);
    parser::initial_parser_source_snapshot_has_witness();
    scalar_edge_checkpoint_has_saved_image(before, parser::initial_parser_source_snapshot(), 0);
    assert(parser::ParserSourceSnapshot {
        next_obligation: 0,
        ..parser::initial_parser_source_snapshot()
    } == parser::initial_parser_source_snapshot());
}

/// One scalar CanonicalPath Edge keeps the frame count and advances the
/// slot's traversed count by one.
pub proof fn canonical_internal_edge_advances_slot_counts(
    state: common::LiveCanonicalPathCommonState,
    index: int,
)
    requires
        common::canonical_path_internal_edge_next(state, index).is_some(),
    ensures
        common::canonical_path_internal_edge_next(state, index).unwrap().parser_frames.len()
            == state.parser_frames.len(),
        common::canonical_path_internal_edge_next(state, index).unwrap().parser_frames[index]
            .unwrap().cursor_traversed
            == (state.parser_frames[index].unwrap().cursor_traversed + 1) as usize,
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
}

/// The premises of C, and the cursor counts it compares, on the reachable
/// opaque-escape state (root source 0f 03 aa bb cc): after the tag byte the
/// top slot is slot 0 with 1 traversed byte, the next observed loop event
/// skips 2 interior bytes, is accepted, and reports 2 traversed bytes.
pub proof fn opaque_bulk_event_premises_hold()
    ensures
        ({
            let root = common::live_canonical_path_common_root_from_source(
                common::opaque_escape_root_source(),
            ).unwrap();
            let events = common::opaque_escape_loop_events();
            let first = common::canonical_path_loop_event_next(root, events[0]).unwrap();
            &&& common::canonical_path_loop_event_next(first, events[1]).is_some()
            &&& events[1].skipped == 2
            &&& events[1].traversed_after == 2
            &&& first.parser_frames.len() == 1
            &&& first.parser_frames[0].unwrap().cursor_traversed == 1
            &&& common::canonical_path_loop_event_next(first, events[1]).unwrap()
                .parser_frames[0].unwrap().cursor_traversed == 2
        }),
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
    hide(common::live_canonical_path_common_root_from_source);
    hide(common::canonical_path_loop_event_next);
    hide(common::canonical_path_driver_scan_fold);
    hide(common::canonical_path_internal_edge_next);
    hide(crate::rpb3_admission_cursor::admission_canonical_path_root_source_wf);
    common::opaque_escape_first_scalar_witness();
    common::opaque_escape_bulk_prefix_witness();
    common::live_canonical_path_common_root_is_wf(common::opaque_escape_root_source());
    let root = common::live_canonical_path_common_root_from_source(
        common::opaque_escape_root_source(),
    ).unwrap();
    canonical_internal_edge_advances_slot_counts(root, 0);
    assert(common::opaque_escape_loop_events()[1].skipped == 2) by (compute_only);
    assert(common::opaque_escape_loop_events()[1].traversed_after == 2) by (compute_only);
}

/// The closed form's top slot counts the selected slot's traversed bytes
/// plus the number of interior Edges taken.
pub proof fn canonical_skip_virtual_slot_count(
    edge: common::LiveCanonicalPathCommonState,
    top: int,
    count: int,
)
    requires
        0 <= top < edge.parser_frames.len(),
    ensures
        canonical_skip_virtual_state(edge, top, count).parser_frames[top].unwrap().cursor_traversed
            == (edge.parser_frames[top].unwrap().cursor_traversed + count) as usize,
{
    hide(parser::forward_parser_payload);
}

/// C on the reachable opaque-escape state: the accepted bulk event after the
/// tag byte is the scan of 3 scalar Edges up to the traversed count, which
/// the observation gives as 2 where the scalar scan counts 4.
pub proof fn canonical_opaque_bulk_skip_witness()
    ensures
        ({
            let root = common::live_canonical_path_common_root_from_source(
                common::opaque_escape_root_source(),
            ).unwrap();
            let events = common::opaque_escape_loop_events();
            let first = common::canonical_path_loop_event_next(root, events[0]).unwrap();
            let edge = common::canonical_path_internal_edge_next(first, 0).unwrap();
            let scalar = canonical_skip_virtual_state(edge, 0, 2);
            let bulk = common::canonical_path_loop_event_next(first, events[1]).unwrap();
            &&& common::canonical_path_driver_scan_fold(first, canonical_edge_actions(0, 3))
                == Some(scalar)
            &&& bulk == canonical_slot_with_traversed(scalar, 0, 2)
            &&& scalar.parser_frames[0].unwrap().cursor_traversed == 4
            &&& bulk.parser_frames[0].unwrap().cursor_traversed == 2
            &&& bulk != scalar
        }),
{
    hide(parser::feed_parser_byte);
    hide(common::live_canonical_path_common_wf);
    hide(common::live_canonical_path_common_root_from_source);
    hide(common::canonical_path_loop_event_next);
    hide(common::canonical_path_driver_scan_fold);
    hide(common::canonical_path_internal_edge_next);
    hide(crate::rpb3_admission_cursor::admission_canonical_path_root_source_wf);
    hide(common::opaque_escape_root_source);
    hide(common::opaque_escape_loop_events);
    hide(canonical_skip_virtual_state);
    hide(canonical_slot_with_traversed);
    opaque_bulk_event_premises_hold();
    let root = common::live_canonical_path_common_root_from_source(
        common::opaque_escape_root_source(),
    ).unwrap();
    let events = common::opaque_escape_loop_events();
    let first = common::canonical_path_loop_event_next(root, events[0]).unwrap();
    canonical_bulk_skip_is_scalar_edge_scan(first, events[1]);
    canonical_internal_edge_advances_slot_counts(first, 0);
    let edge = common::canonical_path_internal_edge_next(first, 0).unwrap();
    canonical_skip_virtual_slot_count(edge, 0, 2);
    let scalar = canonical_skip_virtual_state(edge, 0, 2);
    let bulk = common::canonical_path_loop_event_next(first, events[1]).unwrap();
    assert(scalar.parser_frames[0].unwrap().cursor_traversed == 4);
    assert(bulk.parser_frames[0] != scalar.parser_frames[0]);
}

// ---------------------------------------------------------------------------
// 9. In-file refutations (closed negations)
// ---------------------------------------------------------------------------

/// A Forward jump that skips the owners' child-byte field.
pub open spec fn forward_payload_skipping_child_bytes(
    image: parser::ParserImage,
    bytes: Seq<u8>,
) -> parser::ParserImage {
    let width = bytes.len();
    parser::ParserImage {
        stamp: (image.stamp as int + width) as usize,
        owners: Seq::new(image.owners.len(), |i: int| {
            let owner = image.owners[i];
            parser::ParserRegionOwner {
                remaining: (owner.remaining as int - width) as u64,
                path_remaining: (owner.path_remaining as int - width) as u64,
                ..owner
            }
        }),
        ..image
    }
}

/// A claimed transition that drops the last byte transition, and a jump that
/// skips a field, both differ from the scalar byte run.
pub proof fn dropped_byte_or_field_claims_are_refuted()
    ensures
        plain_parser_byte_run(
            opaque_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 2,
        ) != parser::forward_parser_payload(opaque_payload_parser(), seq![0xaau8]),
        plain_parser_byte_run(
            owner_payload_parser(), seq![0xaau8, 0xbbu8],
            parser_sources(opaque_interior_anchors()), 5,
        ) != forward_payload_skipping_child_bytes(owner_payload_parser(), seq![0xaau8, 0xbbu8]),
{
    hide(parser::feed_parser_byte);
    opaque_and_owner_parser_witness();
    let skipped = forward_payload_skipping_child_bytes(owner_payload_parser(), seq![0xaau8, 0xbbu8]);
    assert(skipped.owners[0].child_bytes.len() == 2);
    assert(parser::forward_parser_payload(owner_payload_parser(), seq![0xaau8, 0xbbu8]).owners[0]
        .child_bytes.len() == 4);
    assert(parser::forward_parser_payload(opaque_payload_parser(), seq![0xaau8]).stamp == 3);
}

/// Neither agreement premise of E follows from `run_wf`: changing only the
/// pending id, or only the path counter, keeps the run well formed and
/// falsifies the Payload clauses of the first step.
pub proof fn payload_edge_agreements_are_necessary()
    ensures
        ({
            let template = opaque_payload_template();
            let run = opaque_payload_run();
            let other_id = restore::RestoreRunImage {
                base_pending: epm::PathObligation { id: 1, ..run.base_pending },
                ..run
            };
            let other_counter = epm::PathTopology { next_obligation: 5, ..template };
            &&& restore::run_wf(other_id)
            &&& !forward_virtual_step_clauses(template, template.restore, other_id, 0)
            &&& restore::run_wf(run)
            &&& !forward_virtual_step_clauses(other_counter, other_counter.restore, run, 0)
        }),
{
    hide(parser::feed_parser_byte);
    opaque_payload_run_is_wf();
    let template = opaque_payload_template();
    let run = opaque_payload_run();
    let other_id = restore::RestoreRunImage {
        base_pending: epm::PathObligation { id: 1, ..run.base_pending },
        ..run
    };
    let other_counter = epm::PathTopology { next_obligation: 5, ..template };
    assert(restore::run_wf(other_id));
    assert(parser_payload_obligation_id(opaque_payload_parser()) == Some(0u64));
    forward_virtual_step_is_scalar_payload_edge(template, template.restore, other_id, 0);
    assert(forward_virtual_path(template, template.restore, other_id, 0).pending.unwrap().id == 1);
    forward_payload_root_projection(opaque_payload_parser(), run.current.bytes.subrange(2, 2));
    assert(forward_virtual_path(other_counter, other_counter.restore, run, 0).parser.next_obligation
        == 1);
}

} // verus!
