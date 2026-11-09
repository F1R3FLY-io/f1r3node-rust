//! One modeled batch combines indexed cursor events with a compressed restore
//! run. Its source and capacity premises still require extraction from Rust.

use vstd::prelude::*;

use crate::{
    rpb3_epathmap_event_refinement as epm, rpb3_epm_push_transport_source_refinement as transport,
    rpb3_epm_restore_run_expansion as restore,
};

verus! {

pub open spec fn path_source(frame: epm::EpmFrame) -> epm::SourceTreeImage {
    epm::SourceTreeImage {
        bytes: frame.path.path_bytes,
        positions: frame.path.path_positions,
    }
}

pub open spec fn trie_source(frame: epm::EpmFrame) -> epm::IndexedRegionImage
    recommends epm::frame_layout_wf(frame),
        frame.trie_start <= frame.trie_end,
        frame.trie_end as int <= frame.snapshot.bytes.len(),
{
    epm::indexed_subview(frame.snapshot, epm::ByteSpan {
        start: frame.trie_start,
        end: frame.trie_end,
    })
}

pub open spec fn batch_cursor_source_aligned(
    before: epm::EpmFrame,
    chunk: transport::EpmPushTransportChunk,
) -> bool {
    let events = transport::epm_push_chunk_events(chunk);
    &&& epm::frame_layout_wf(before)
    &&& !before.cursor.done
    &&& before.cursor.at as int + events.len()
        <= epm::cursor_event_count(before) as int
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger]
        epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
            == epm::wire_event(events[i])
}

pub open spec fn advanced_cursor(
    before: epm::EpmFrame, event_count: int,
) -> epm::EpmCursorImage
    recommends before.cursor.at as int + event_count <= usize::MAX as int,
{
    epm::EpmCursorImage {
        act_plan: before.cursor.act_plan,
        validated_plan: before.cursor.validated_plan,
        at: (before.cursor.at as int + event_count) as usize,
        declared_value_count: before.cursor.declared_value_count,
        map_values_seen: before.cursor.map_values_seen,
        value_table_offset: before.cursor.value_table_offset,
        done: false,
    }
}

pub open spec fn forward_batch_source_guard(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
) -> bool {
    let chunk = transport::EpmPushTransportChunk::Forward { start, width };
    &&& batch_cursor_source_aligned(before, chunk)
    &&& before.wait.is_none()
    &&& before.path.pending == Some(pending)
    &&& !before.path.root_left
    &&& latest_restore_state_id != Some(state_id)
    &&& start as int + width as int <= before.trie_end - before.trie_start
    &&& before.path.parser.stamp as int == before.path.path_bytes.len()
    &&& epm::parser_gate_matches(before.path.gate, before.path.parser)
    &&& epm::parser::parser_forward_capacity(before.path.parser, width as int)
    &&& restore::forward_capacity_facts_match(
        path_source(before), pending, trie_source(before), start, width, facts,
    )
}

pub open spec fn forward_batch_after(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
) -> epm::EpmFrame
    recommends epm::frame_layout_wf(before), width > 0,
        start as int + width as int <= before.trie_end - before.trie_start,
        before.path.path_bytes.len() + width as int <= usize::MAX as int,
        restore::run_wf(restore::publish_forward_run(
            path_source(before), pending, before.path.gate, before.path.parser, state_id,
            trie_source(before), start, width,
        )),
{
    let run = restore::publish_forward_run(
        path_source(before), pending, before.path.gate, before.path.parser, state_id,
        trie_source(before), start, width,
    );
    epm::EpmFrame {
        path: epm::PathTopology {
            next_obligation: before.path.next_obligation,
            pending: Some(restore::run_current_pending(run)),
            root_left: before.path.root_left,
            restore: before.path.restore + restore::expanded_run(run),
            path_bytes: run.current.bytes,
            path_positions: run.current.positions,
            gate: before.path.gate,
            parser: epm::parser::forward_parser_payload(run.base_parser,
                run.current.bytes.subrange(run.first_depth as int, run.current.bytes.len() as int)),
        },
        cursor: advanced_cursor(before, 2 * width as int),
        wait: before.wait,
        mode: before.mode,
        entries_stable: before.entries_stable,
        last_source: run.current.positions[run.current.positions.len() - 1],
        snapshot: before.snapshot,
        backing: before.backing,
        trie_start: before.trie_start,
        trie_end: before.trie_end,
        value_table_start: before.value_table_start,
        mode_offset: before.mode_offset,
    }
}

pub proof fn accepted_forward_batch_composes_cursor_and_restore_run(
    before: epm::EpmFrame,
    start: usize,
    width: usize,
    pending: epm::PathObligation,
    state_id: usize,
    latest_restore_state_id: Option<usize>,
    facts: restore::ForwardCapacityFacts,
)
    requires forward_batch_source_guard(
        before, start, width, pending, state_id, latest_restore_state_id, facts,
    ),
    ensures {
        let chunk = transport::EpmPushTransportChunk::Forward { start, width };
        let after = forward_batch_after(before, start, width, pending, state_id);
        &&& after.cursor.at as int
            == before.cursor.at as int + transport::epm_push_chunk_len(chunk) as int
        &&& after.path.path_bytes.len()
            == before.path.path_bytes.len() + width as int
        &&& after.path.restore.len()
            == before.path.restore.len() + width as int
        &&& after.path.path_bytes == path_source(before).bytes
            + trie_source(before).bytes.subrange(start as int, start as int + width as int)
        &&& after.path.path_positions == path_source(before).positions
            + trie_source(before).positions.subrange(start as int, start as int + width as int)
        &&& after.last_source
            == epm::trie_position(before, (start as int + width as int - 1) as usize)
        &&& epm::same_frame_source(before, after)
        &&& after.cursor.act_plan == before.cursor.act_plan
        &&& after.cursor.validated_plan == before.cursor.validated_plan
        &&& forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) ==>
            #[trigger] epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
                == epm::wire_event(transport::epm_push_chunk_event(chunk, i))
    },
{
    let chunk = transport::EpmPushTransportChunk::Forward { start, width };
    let source = trie_source(before);
    let base = path_source(before);
    restore::accepted_forward_publishes_exact_virtual_checkpoints(
        base, pending, before.path.gate, before.path.parser,
        state_id, source, start, width, facts,
    );
    transport::epm_push_batch_commit_matches_last_scalar_source(chunk);
    let run = restore::publish_forward_run(
        base, pending, before.path.gate, before.path.parser,
        state_id, source, start, width,
    );
    let after = forward_batch_after(before, start, width, pending, state_id);
    assert(restore::run_wf(run));
    assert(run.current.positions.len() > 0);
    assert(after.last_source == source.positions[start as int + width as int - 1]);
    assert(source.positions[start as int + width as int - 1]
        == epm::trie_position(before, (start as int + width as int - 1) as usize));
}

pub open spec fn coalesced_forward_batch_source_guard(
    before: epm::EpmFrame,
    run: restore::RestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    current_state_id: usize,
    current_pending: epm::PathObligation,
    start: usize,
    width: usize,
    facts: restore::ForwardCapacityFacts,
) -> bool {
    let chunk = transport::EpmPushTransportChunk::Forward { start, width };
    &&& batch_cursor_source_aligned(before, chunk)
    &&& before.wait.is_none()
    &&& !before.path.root_left
    &&& start as int + width as int <= before.trie_end - before.trie_start
    &&& before.path.restore == prefix + restore::expanded_run(run)
    &&& before.path.path_bytes == run.current.bytes
    &&& before.path.path_positions == run.current.positions
    &&& before.path.pending == Some(current_pending)
    &&& before.path.gate == run.gate
    &&& before.path.parser == epm::parser::forward_parser_payload(run.base_parser,
        run.current.bytes.subrange(run.first_depth as int, run.current.bytes.len() as int))
    &&& restore::coalesced_forward_facts_match(
        run, current_state_id, current_pending,
        trie_source(before), start, width, facts,
    )
}

pub open spec fn coalesced_forward_batch_after(
    before: epm::EpmFrame,
    run: restore::RestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
) -> epm::EpmFrame
    recommends restore::run_wf(run), width > 0,
        epm::frame_layout_wf(before),
        start as int + width as int <= before.trie_end - before.trie_start,
        run.current.bytes.len() + width as int <= usize::MAX as int,
        restore::run_wf(restore::extend_coalesced_forward_run(
            run, trie_source(before), start, width,
        )),
{
    let extended = restore::extend_coalesced_forward_run(
        run, trie_source(before), start, width,
    );
    epm::EpmFrame {
        path: epm::PathTopology {
            next_obligation: before.path.next_obligation,
            pending: Some(restore::run_current_pending(extended)),
            root_left: false,
            restore: prefix + restore::expanded_run(extended),
            path_bytes: extended.current.bytes,
            path_positions: extended.current.positions,
            gate: before.path.gate,
            parser: epm::parser::forward_parser_payload(extended.base_parser,
                extended.current.bytes.subrange(extended.first_depth as int,
                    extended.current.bytes.len() as int)),
        },
        cursor: advanced_cursor(before, 2 * width as int),
        wait: before.wait,
        mode: before.mode,
        entries_stable: before.entries_stable,
        last_source: extended.current.positions[extended.current.positions.len() - 1],
        snapshot: before.snapshot,
        backing: before.backing,
        trie_start: before.trie_start,
        trie_end: before.trie_end,
        value_table_start: before.value_table_start,
        mode_offset: before.mode_offset,
    }
}

pub proof fn accepted_coalesced_forward_batch_composes_cursor_and_restore_run(
    before: epm::EpmFrame,
    run: restore::RestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    current_state_id: usize,
    current_pending: epm::PathObligation,
    start: usize,
    width: usize,
    facts: restore::ForwardCapacityFacts,
)
    requires coalesced_forward_batch_source_guard(
        before, run, prefix, current_state_id, current_pending,
        start, width, facts,
    ),
    ensures {
        let chunk = transport::EpmPushTransportChunk::Forward { start, width };
        let after = coalesced_forward_batch_after(before, run, prefix, start, width);
        &&& after.cursor.at as int
            == before.cursor.at as int + transport::epm_push_chunk_len(chunk) as int
        &&& after.path.path_bytes.len()
            == before.path.path_bytes.len() + width as int
        &&& after.path.restore.len()
            == before.path.restore.len() + width as int
        &&& after.path.path_bytes == before.path.path_bytes
            + trie_source(before).bytes.subrange(start as int, start as int + width as int)
        &&& after.path.path_positions == before.path.path_positions
            + trie_source(before).positions.subrange(start as int, start as int + width as int)
        &&& after.last_source
            == epm::trie_position(before, (start as int + width as int - 1) as usize)
        &&& epm::same_frame_source(before, after)
        &&& forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) ==>
            #[trigger] epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
                == epm::wire_event(transport::epm_push_chunk_event(chunk, i))
    },
{
    let chunk = transport::EpmPushTransportChunk::Forward { start, width };
    let source = trie_source(before);
    restore::accepted_coalesced_forward_extends_virtual_checkpoints(
        run, current_state_id, current_pending, source, start, width, facts,
    );
    transport::epm_push_batch_commit_matches_last_scalar_source(chunk);
    let extended = restore::extend_coalesced_forward_run(run, source, start, width);
    let after = coalesced_forward_batch_after(before, run, prefix, start, width);
    assert(restore::run_wf(extended));
    assert(extended.current.positions.len() > 0);
    assert(after.last_source == source.positions[start as int + width as int - 1]);
    assert(source.positions[start as int + width as int - 1]
        == epm::trie_position(before, (start as int + width as int - 1) as usize));
}

pub struct HistoricalRestoreRunImage {
    pub first_depth: usize,
    pub latest_depth: usize,
    pub saved_state_id: usize,
    pub ghost current: epm::SourceTreeImage,
    pub ghost latest: epm::SourceTreeImage,
    pub ghost checkpoints: Seq<epm::PathCheckpoint>,
}

pub struct SelectedHistoricalCheckpoint {
    pub saved_state_id: usize,
    pub ghost checkpoint: epm::PathCheckpoint,
}

pub open spec fn historical_run_width(run: HistoricalRestoreRunImage) -> int {
    run.latest_depth as int - run.first_depth as int + 1
}

pub open spec fn historical_run_wf(run: HistoricalRestoreRunImage) -> bool {
    &&& run.first_depth <= run.latest_depth
    &&& run.latest_depth < usize::MAX
    &&& restore::source_tree_wf(run.current)
    &&& restore::source_tree_wf(run.latest)
    &&& run.current.bytes.len() == run.latest_depth as int + 1
    &&& run.latest.bytes == run.current.bytes.subrange(0, run.latest_depth as int)
    &&& run.latest.positions
        == run.current.positions.subrange(0, run.latest_depth as int)
    &&& run.checkpoints.len() == historical_run_width(run)
    &&& forall|i: int| 0 <= i < run.checkpoints.len() ==>
        #[trigger] run.checkpoints[i].path_len as int == run.first_depth as int + i
}

pub open spec fn historical_run_from_payload(run: restore::RestoreRunImage)
    -> HistoricalRestoreRunImage
    recommends restore::run_wf(run),
{
    HistoricalRestoreRunImage {
        first_depth: run.first_depth,
        latest_depth: run.latest_depth,
        saved_state_id: run.state_id,
        current: run.current,
        latest: run.latest,
        checkpoints: restore::expanded_run(run),
    }
}

pub proof fn payload_run_has_historical_checkpoint_projection(run: restore::RestoreRunImage)
    requires restore::run_wf(run),
    ensures historical_run_wf(historical_run_from_payload(run)),
{
    let historical = historical_run_from_payload(run);
    assert forall|i: int| 0 <= i < historical.checkpoints.len() implies
        #[trigger] historical.checkpoints[i].path_len as int
            == historical.first_depth as int + i by {
    }
}

pub open spec fn historical_unwind_accepted(
    run: HistoricalRestoreRunImage,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& historical_run_wf(run)
    &&& !facts.root_left
    &&& width > 0
    &&& width <= facts.available
    &&& width as int <= historical_run_width(run)
    &&& (width == 1 || facts.selected_slice_available)
    &&& (width as int == historical_run_width(run) || facts.remaining_slice_available)
}

pub open spec fn selected_historical_checkpoint(
    run: HistoricalRestoreRunImage, width: usize,
) -> SelectedHistoricalCheckpoint
    recommends historical_run_wf(run), 0 < width as int <= historical_run_width(run),
{
    SelectedHistoricalCheckpoint {
        saved_state_id: run.saved_state_id,
        checkpoint: run.checkpoints[historical_run_width(run) - width as int],
    }
}

pub open spec fn unwind_batch_source_guard(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    let chunk = transport::EpmPushTransportChunk::Unwind { start, width };
    &&& batch_cursor_source_aligned(before, chunk)
    &&& before.wait.is_none()
    &&& !before.path.root_left
    &&& before.path.next_obligation == before.path.parser.next_obligation
    &&& epm::parser_gate_matches(before.path.gate, before.path.parser)
    &&& start as int + width as int <= before.trie_end - before.trie_start
    &&& historical_unwind_accepted(run, width, facts)
    &&& forall|i: int| 0 <= i < run.checkpoints.len() ==>
        #[trigger] epm::parser_gate_matches(run.checkpoints[i].gate,
            run.checkpoints[i].parser)
    &&& before.path.restore == prefix + run.checkpoints
    &&& before.path.path_bytes == run.current.bytes
    &&& before.path.path_positions == run.current.positions
    &&& forall|i: int| 0 <= i < width as int ==> #[trigger]
        run.current.positions[run.current.positions.len() - 1 - i]
            == epm::trie_position(before, (start as int + width as int - 1 - i) as usize)
}

pub open spec fn unwind_batch_after(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    width: usize,
) -> epm::EpmFrame
    recommends historical_run_wf(run), 0 < width as int <= historical_run_width(run),
        before.cursor.at as int + width as int <= usize::MAX as int,
{
    let depth = run.current.bytes.len() - width as int;
    let selected = selected_historical_checkpoint(run, width);
    epm::EpmFrame {
        path: epm::PathTopology {
            next_obligation: before.path.next_obligation,
            pending: selected.checkpoint.pending,
            root_left: false,
            restore: prefix + run.checkpoints.subrange(
                0, historical_run_width(run) - width as int,
            ),
            path_bytes: run.current.bytes.subrange(0, depth),
            path_positions: run.current.positions.subrange(0, depth),
            gate: selected.checkpoint.gate,
            parser: epm::parser::restore_parser_checkpoint(
                selected.checkpoint.parser, before.path.parser),
        },
        cursor: advanced_cursor(before, width as int),
        wait: before.wait,
        mode: before.mode,
        entries_stable: before.entries_stable,
        last_source: run.current.positions[depth],
        snapshot: before.snapshot,
        backing: before.backing,
        trie_start: before.trie_start,
        trie_end: before.trie_end,
        value_table_start: before.value_table_start,
        mode_offset: before.mode_offset,
    }
}

/// The zero step retains the actual current reducer state. Later steps use
/// historical checkpoints; in particular, the current pending/gate need not
/// equal the first checkpoint's pending/gate.
pub open spec fn unwind_virtual_frame(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
) -> epm::EpmFrame
    recommends historical_run_wf(run), step as int <= historical_run_width(run),
        before.cursor.at as int + step as int <= usize::MAX as int,
{
    if step == 0 {
        before
    } else {
        unwind_batch_after(before, run, prefix, step)
    }
}

pub proof fn saved_parser_gate_survives_global_counter_restore(
    gate: epm::PathGate,
    saved: epm::parser::ParserImage,
    current: epm::parser::ParserImage,
)
    requires epm::parser_gate_matches(gate, saved),
    ensures
        epm::parser_gate_matches(gate,
            epm::parser::restore_parser_checkpoint(saved, current)),
        epm::parser::restore_parser_checkpoint(saved, current).next_obligation
            == current.next_obligation,
{
    assert(epm::parser::parser_root_gate(
        epm::parser::restore_parser_checkpoint(saved, current))
        == epm::parser::parser_root_gate(saved));
}

#[verifier::rlimit(100)]
pub proof fn accepted_unwind_batch_one_scalar_restore_step(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        unwind_batch_source_guard(before, run, prefix, start, width, facts),
        0 <= step < width as int,
    ensures
        epm::leave_return(
            unwind_virtual_frame(before, run, prefix, step as usize),
            unwind_virtual_frame(before, run, prefix, (step + 1) as usize),
            run.current.positions[run.current.positions.len() - 1 - step],
        ),
{
    let from = unwind_virtual_frame(before, run, prefix, step as usize);
    let to = unwind_virtual_frame(before, run, prefix, (step + 1) as usize);
    let remaining = historical_run_width(run) - step;
    assert(0 < remaining);
    assert(from.path.restore.len() == prefix.len() + remaining);
    assert(from.path.restore.last() == run.checkpoints[remaining - 1]);
    assert(to.path.restore == from.path.restore.drop_last());
    assert(to.path.pending == from.path.restore.last().pending);
    assert(to.path.gate == from.path.restore.last().gate);
    let selected_index = remaining - 1;
    assert(0 <= selected_index < run.checkpoints.len());
    assert(epm::parser_gate_matches(run.checkpoints[selected_index].gate,
        run.checkpoints[selected_index].parser));
    saved_parser_gate_survives_global_counter_restore(
        run.checkpoints[selected_index].gate,
        run.checkpoints[selected_index].parser,
        from.path.parser,
    );
    if step > 0 {
        let prior_index = historical_run_width(run) - step;
        assert(0 <= prior_index < run.checkpoints.len());
        assert(epm::parser_gate_matches(run.checkpoints[prior_index].gate,
            run.checkpoints[prior_index].parser));
        saved_parser_gate_survives_global_counter_restore(
            run.checkpoints[prior_index].gate,
            run.checkpoints[prior_index].parser,
            before.path.parser,
        );
    }
    assert(from.path.next_obligation == from.path.parser.next_obligation);
    assert(to.path.next_obligation == to.path.parser.next_obligation);
    assert(epm::parser_gate_matches(from.path.gate, from.path.parser));
    assert(epm::parser_gate_matches(to.path.gate, to.path.parser));
    assert(to.path.parser == epm::parser::restore_parser_checkpoint(
        from.path.restore.last().parser, from.path.parser));
    assert(from.path.restore.last().path_len as int
        == run.current.bytes.len() - 1 - step);
    assert(to.path.path_bytes == from.path.path_bytes.subrange(
        0, from.path.restore.last().path_len as int,
    ));
    assert(to.path.path_positions == from.path.path_positions.subrange(
        0, from.path.restore.last().path_len as int,
    ));
}

pub proof fn accepted_unwind_batch_has_scalar_restore_steps(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires unwind_batch_source_guard(before, run, prefix, start, width, facts),
    ensures
        unwind_virtual_frame(before, run, prefix, 0) == before,
        unwind_virtual_frame(before, run, prefix, width)
            == unwind_batch_after(before, run, prefix, width),
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] epm::leave_return(
                unwind_virtual_frame(before, run, prefix, step as usize),
                unwind_virtual_frame(before, run, prefix, (step + 1) as usize),
                run.current.positions[run.current.positions.len() - 1 - step],
            ),
{
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] epm::leave_return(
            unwind_virtual_frame(before, run, prefix, step as usize),
            unwind_virtual_frame(before, run, prefix, (step + 1) as usize),
            run.current.positions[run.current.positions.len() - 1 - step],
        ) by {
        accepted_unwind_batch_one_scalar_restore_step(
            before, run, prefix, start, width, facts, step,
        );
    }
}

pub proof fn accepted_unwind_batch_composes_cursor_and_restore_run(
    before: epm::EpmFrame,
    run: HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires unwind_batch_source_guard(before, run, prefix, start, width, facts),
    ensures {
        let chunk = transport::EpmPushTransportChunk::Unwind { start, width };
        let after = unwind_batch_after(before, run, prefix, width);
        let selected = selected_historical_checkpoint(run, width);
        &&& after.cursor.at as int
            == before.cursor.at as int + transport::epm_push_chunk_len(chunk) as int
        &&& after.path.path_bytes.len()
            == before.path.path_bytes.len() - width as int
        &&& after.path.restore.len()
            == before.path.restore.len() - width as int
        &&& after.path.path_bytes
            == before.path.path_bytes.subrange(0, before.path.path_bytes.len() - width as int)
        &&& after.path.path_positions
            == before.path.path_positions.subrange(0, before.path.path_positions.len() - width as int)
        &&& after.path.pending == selected.checkpoint.pending
        &&& after.path.gate == selected.checkpoint.gate
        &&& selected.saved_state_id == run.saved_state_id
        &&& selected.checkpoint.path_len as int == after.path.path_bytes.len()
        &&& forall|step: int| 1 <= step <= width as int ==>
            #[trigger] run.checkpoints[historical_run_width(run) - step].path_len as int
                == run.current.bytes.len() - step
        &&& after.last_source == epm::trie_position(before, start)
        &&& epm::same_frame_source(before, after)
        &&& after.cursor.act_plan == before.cursor.act_plan
        &&& after.cursor.validated_plan == before.cursor.validated_plan
        &&& forall|i: int| 0 <= i < transport::epm_push_chunk_len(chunk) ==>
            #[trigger] epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
                == epm::wire_event(transport::epm_push_chunk_event(chunk, i))
    },
{
    let chunk = transport::EpmPushTransportChunk::Unwind { start, width };
    transport::epm_push_batch_commit_matches_last_scalar_source(chunk);
    let selected = selected_historical_checkpoint(run, width);
    let after = unwind_batch_after(before, run, prefix, width);
    let depth = run.current.bytes.len() - width as int;
    assert(selected.checkpoint.path_len as int == depth);
    assert forall|step: int| 1 <= step <= width as int implies
        #[trigger] run.checkpoints[historical_run_width(run) - step].path_len as int
            == run.current.bytes.len() - step by {
    }
    assert(depth < run.current.positions.len());
    assert(after.last_source == run.current.positions[depth]);
    let last = width as int - 1;
    assert(0 <= last < width as int);
    assert(run.current.positions[run.current.positions.len() - 1 - last]
        == epm::trie_position(before, (start as int + width as int - 1 - last) as usize));
    assert(run.current.positions[depth] == epm::trie_position(before, start));
}

/// A one-byte ACT03 line has scalar events Value, Edge, Value, Leave, Leave.
/// The line's byte is at trie offset 17 (snapshot offset 24). The first Leave
/// is therefore a real line Unwind at cursor index 3, rather than an empty-mode
/// synthetic Leave.
pub open spec fn unwind_witness_bytes() -> Seq<u8> {
    seq![0x45, 0x50, 0x4d, 0x31, 1, 1, 29]
        + seq![0x41, 0x43, 0x54, 0x72, 0x65, 0x65, 0x30, 0x33]
        + seq![18, 0, 0, 0, 0, 0, 0, 0]
        + seq![1, 0x2a, 0xc0, 0, 2]
        + seq![0, 0, 0, 0, 0, 0, 0, 0]
        + seq![0]
}

pub open spec fn unwind_witness_anchor(offset: usize) -> epm::SourceAnchor {
    epm::SourceAnchor { source: 1, offset }
}

pub open spec fn unwind_witness_backing() -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: unwind_witness_bytes(),
        source: 1,
        absolute_start: 0,
        start: 0,
        len: 37,
        end_override: None,
    }
}

pub open spec fn unwind_witness_snapshot() -> epm::IndexedRegionImage {
    match epm::indexed_backing_project(unwind_witness_backing()) {
        Some(snapshot) => snapshot,
        None => epm::IndexedRegionImage {
            bytes: Seq::empty(),
            positions: Seq::empty(),
            end: unwind_witness_anchor(0),
        },
    }
}

pub open spec fn unwind_witness_plan() -> epm::ActPlanImage {
    epm::ActPlanImage {
        records: seq![epm::ActRecordImage {
            node_id: 18,
            subtree_end: 1,
            value: Some(0),
            value_source_offset: 18,
            line_start: 17,
            line_len: 1,
            line_encoded_start: 16,
            wrapper_id: None,
            wrapper_len: 0,
            incoming_source_offset: None,
            incoming_origin: None,
            children: Seq::empty(),
        }],
        finish_order: seq![0],
        line_hash: |_bytes: Seq<u8>| 0,
    }
}

pub open spec fn unwind_witness_validated_plan() -> epm::ValidatedActPlanImage {
    epm::ValidatedActPlanImage {
        records: seq![epm::ValidatedTopologyRecordImage {
            node_id: 18,
            value: 0,
            value_source_offset: 18,
            line_start: 17,
            line_len: 1,
            incoming_source_offset: 0,
            child_count: 0,
            incoming_byte: 0,
            flags: 1,
        }],
        empty: false,
        value_count: 1,
        first_nonzero: None,
    }
}

pub open spec fn unwind_witness_gate(stable: bool) -> epm::PathGate {
    epm::PathGate {
        dead: false,
        stack_empty: true,
        split_complete: true,
        completed_segments: 0,
        first_is_list: false,
        stable,
        escape_allowed: true,
    }
}

pub open spec fn unwind_witness_parser(stable: bool, stamp: usize)
    -> epm::parser::ParserImage {
    let initial = epm::parser::initial_parser();
    epm::parser::ParserImage {
        focus: epm::parser::ParserPath {
            status: epm::parser::ParserStatus::SplitComplete,
            stable,
            ..initial.focus
        },
        next_obligation: 1,
        stamp,
        ..initial
    }
}

pub open spec fn unwind_witness_checkpoint(
    pending: Option<epm::PathObligation>, saved_stable: bool,
) -> epm::PathCheckpoint {
    epm::PathCheckpoint {
        pending,
        gate: unwind_witness_gate(saved_stable),
        path_len: 0,
        parser: unwind_witness_parser(saved_stable, 0),
    }
}

pub open spec fn unwind_witness_run(
    pending: Option<epm::PathObligation>, saved_stable: bool,
) -> HistoricalRestoreRunImage {
    let current = epm::SourceTreeImage {
        bytes: seq![0x2a],
        positions: seq![unwind_witness_anchor(24)],
    };
    HistoricalRestoreRunImage {
        first_depth: 0,
        latest_depth: 0,
        saved_state_id: 7,
        current,
        latest: restore::source_prefix(current, 0),
        checkpoints: seq![unwind_witness_checkpoint(pending, saved_stable)],
    }
}

pub open spec fn unwind_witness_before(
    pending: Option<epm::PathObligation>, saved_stable: bool,
) -> epm::EpmFrame {
    epm::EpmFrame {
        path: epm::PathTopology {
            next_obligation: 1,
            pending: None,
            root_left: false,
            restore: seq![unwind_witness_checkpoint(pending, saved_stable)],
            path_bytes: seq![0x2a],
            path_positions: seq![unwind_witness_anchor(24)],
            gate: unwind_witness_gate(true),
            parser: unwind_witness_parser(true, 1),
        },
        cursor: epm::EpmCursorImage {
            act_plan: Some(unwind_witness_plan()),
            validated_plan: Some(unwind_witness_validated_plan()),
            at: 3,
            declared_value_count: 0,
            map_values_seen: 0,
            value_table_offset: 37,
            done: false,
        },
        wait: None,
        mode: epm::EpmMode::Set,
        entries_stable: true,
        last_source: unwind_witness_anchor(24),
        snapshot: unwind_witness_snapshot(),
        backing: unwind_witness_backing(),
        trie_start: 7,
        trie_end: 36,
        value_table_start: 37,
        mode_offset: 5,
    }
}

pub open spec fn unwind_witness_facts() -> restore::UnwindCapacityFacts {
    restore::UnwindCapacityFacts {
        root_left: false,
        available: 1,
        selected_slice_available: true,
        remaining_slice_available: true,
    }
}

pub open spec fn unwind_witness_saved_pending() -> epm::PathObligation {
    epm::PathObligation {
        id: 0,
        kind: epm::PathObligationKind::RawParEscape,
        payload_len: 1,
        tag_source: unwind_witness_anchor(23),
        payload_start: 0,
        payload_end: unwind_witness_anchor(24),
        awaiting_resolution: false,
    }
}

pub proof fn unwind_witness_historical_run_wf(
    pending: Option<epm::PathObligation>, saved_stable: bool,
)
    ensures historical_run_wf(unwind_witness_run(pending, saved_stable)),
{
    let run = unwind_witness_run(pending, saved_stable);
    assert(restore::source_tree_wf(run.current));
    assert(restore::source_tree_wf(run.latest));
    assert(run.latest.bytes == run.current.bytes.subrange(0, 0));
    assert(run.latest.positions == run.current.positions.subrange(0, 0));
    assert forall|i: int| 0 <= i < run.checkpoints.len() implies
        #[trigger] run.checkpoints[i].path_len as int == run.first_depth as int + i by {
        assert(i == 0);
    }
}

pub proof fn unwind_witness_validated_plan_matches()
    ensures epm::act_plan_projects_validated(
        unwind_witness_before(None, true),
        unwind_witness_plan(),
        unwind_witness_validated_plan(),
    ),
{
    let before = unwind_witness_before(None, true);
    let plan = unwind_witness_plan();
    let validated = unwind_witness_validated_plan();
    let trie = unwind_witness_bytes().subrange(7, 36);
    reveal_with_fuel(epm::act_value_count, 2);
    assert(epm::act_value_count(plan.records, 0) == 1);
    assert(epm::act_record_projects_validated(
        trie, plan.records[0], validated.records[0],
    ));
    assert forall|i: int| 0 <= i < plan.records.len() implies
        epm::act_record_projects_validated(trie, plan.records[i], validated.records[i]) by {
        assert(i == 0);
    }
}

pub open spec fn unwind_witness_events() -> Seq<epm::WireTopologyEvent> {
    seq![
        epm::WireTopologyEvent::Value {
            has_value: false, value_body: None, source_offset: 18,
        },
        epm::WireTopologyEvent::Edge {
            source_offset: 17, origin: epm::EdgeOrigin::Direct,
        },
        epm::WireTopologyEvent::Value {
            has_value: true, value_body: None, source_offset: 18,
        },
        epm::WireTopologyEvent::Leave { source_offset: 17 },
        epm::WireTopologyEvent::Leave { source_offset: 18 },
    ]
}

pub proof fn unwind_witness_encoded_prefix()
    ensures {
        let before = unwind_witness_before(None, true);
        &&& unwind_witness_bytes().len() == 37
        &&& epm::indexed_image_wf(before.snapshot)
        &&& epm::indexed_backing_project(before.backing) == Some(before.snapshot)
        &&& before.snapshot.bytes == unwind_witness_bytes()
        &&& epm::epm_prefix_from_bytes(before.snapshot.bytes)
            == Some(epm::EpmPrefixImage {
                mode: epm::EpmMode::Set,
                trie_start: 7,
                trie_end: 36,
                value_count: 0,
                value_table_start: 37,
            })
    },
{
    let before = unwind_witness_before(None, true);
    let bytes = unwind_witness_bytes();
    assert(bytes.len() == 37);
    assert(epm::canonical_varint_at(bytes, 6)
        == Some(epm::CanonicalVarintImage { value: 29, end: 7 })) by {
        reveal_with_fuel(epm::canonical_varint_tail, 2);
        assert(bytes[6] == 29);
    }
    assert(epm::canonical_varint_at(bytes, 36)
        == Some(epm::CanonicalVarintImage { value: 0, end: 37 })) by {
        reveal_with_fuel(epm::canonical_varint_tail, 2);
        assert(bytes[36] == 0);
    }
    assert(epm::indexed_backing_project(unwind_witness_backing()).is_some());
    assert(epm::indexed_backing_project(before.backing) == Some(before.snapshot));
    assert(epm::indexed_image_wf(before.snapshot));
    assert(epm::epm_prefix_from_bytes(bytes) == Some(epm::EpmPrefixImage {
        mode: epm::EpmMode::Set,
        trie_start: 7,
        trie_end: 36,
        value_count: 0,
        value_table_start: 37,
    }));
}

#[verifier::rlimit(50)]
pub proof fn unwind_witness_encoded_act_plan()
    ensures epm::act_plan_matches_frame(
        unwind_witness_before(None, true), unwind_witness_plan(),
    ),
{
    unwind_witness_encoded_prefix();
    let before = unwind_witness_before(None, true);
    let bytes = unwind_witness_bytes();
    let trie = bytes.subrange(7, 36);
    let plan = unwind_witness_plan();
    assert(epm::act_magic_and_trailer(trie));
    assert(epm::act_semantic_at(trie, 18) == Some(epm::ActSemanticImage {
        value: Some(0),
        value_source_offset: 18,
        line_start: 17,
        line_len: 1,
        line_encoded_start: 16,
        wrapper_id: None,
        wrapper_len: 0,
        first_child: None,
        edges: Seq::empty(),
        encoded_len: 3,
    })) by {
        assert(trie[18] == 0xc0);
        assert(epm::act_value_at(trie, 18) == Some((Some(0), 20)));
        assert(epm::act_varint_at(trie, 20)
            == Some(epm::CanonicalVarintImage { value: 2, end: 21 }));
        assert(epm::act_varint_at(trie, 16)
            == Some(epm::CanonicalVarintImage { value: 1, end: 17 }));
    }
    assert(epm::act_record_matches(trie, plan.records, 0));
    assert(epm::act_producer_matches_frame(trie, plan)) by {
        reveal_with_fuel(epm::act_producer_replay, 2);
        assert(epm::act_finish_order_wf(plan));
    }
    assert(epm::act_plan_matches_frame(before, plan)) by {
        reveal_with_fuel(epm::little_endian_uint, 9);
        assert(epm::little_endian_uint(trie, 8, 8) == 18);
    }
}

#[verifier::rlimit(60)]
pub proof fn unwind_witness_expanded_act_events()
    ensures epm::act_plan_events(
        unwind_witness_before(None, true), unwind_witness_plan(),
    ) == Some(unwind_witness_events()),
{
    unwind_witness_encoded_act_plan();
    let before = unwind_witness_before(None, true);
    let bytes = unwind_witness_bytes();
    let trie = bytes.subrange(7, 36);
    let plan = unwind_witness_plan();
    let events = unwind_witness_events();
    reveal_with_fuel(epm::act_line_forward_work, 2);
    reveal_with_fuel(epm::act_line_leave_work, 2);
    assert(epm::act_record_work(plan, 0).len() == 5);
    assert(epm::act_expand_work(
        plan,
        seq![epm::ActWorkItem::Enter(0)],
        epm::act_work_budget(plan, 0) + 1,
    ) == Some(events)) by {
        reveal_with_fuel(epm::act_line_forward_work, 2);
        reveal_with_fuel(epm::act_line_leave_work, 2);
        reveal_with_fuel(epm::act_work_budget, 2);
        reveal_with_fuel(epm::act_expand_work, 7);
    }
    assert(epm::attach_act_value_bodies(bytes, epm::EpmMode::Set, events, 37)
        == Some(epm::AttachedActEvents {
            events, next_value_offset: 37, map_values_seen: 0,
        })) by {
        reveal_with_fuel(epm::attach_act_value_bodies, 6);
    }
    assert forall|i: int| 0 <= i < events.len() implies
        epm::act_event_source_valid(trie, events[i]) by {
        assert(0 <= i < 5);
        if i == 0 {
            assert(epm::act_event_source_valid(trie, events[0]));
        } else if i == 1 {
            assert(epm::act_event_source_valid(trie, events[1]));
        } else if i == 2 {
            assert(epm::act_event_source_valid(trie, events[2]));
        } else if i == 3 {
            assert(epm::act_event_source_valid(trie, events[3]));
        } else {
            assert(i == 4);
            assert(epm::act_event_source_valid(trie, events[4]));
        }
    }
    assert(before.snapshot.bytes == bytes);
    assert(before.snapshot.bytes.len() == 37);
    assert(before.snapshot.bytes.subrange(
        before.trie_start as int, before.trie_end as int,
    ) == trie);
    assert forall|i: int| 0 <= i < events.len() implies
        epm::act_event_source_valid(
            before.snapshot.bytes.subrange(
                before.trie_start as int, before.trie_end as int,
            ), events[i],
        ) by {
        assert(epm::act_event_source_valid(trie, events[i]));
    }
    assert(epm::act_plan_events(before, plan) == Some(events));
}

/// Evaluate the concrete ACT03 line in small, finite checks. The root value
/// is ACT's set-membership marker; it has no protobuf value-table body.
#[verifier::rlimit(50)]
pub proof fn unwind_witness_concrete_act_layout()
    ensures {
        let before = unwind_witness_before(None, true);
        &&& epm::frame_layout_wf(before)
        &&& epm::act_plan_events(before, unwind_witness_plan())
            == Some(unwind_witness_events())
        &&& epm::cursor_event_at(before, 3)
            == Some(epm::WireTopologyEvent::Leave { source_offset: 17 })
        &&& epm::trie_position(before, 17) == unwind_witness_anchor(24)
    },
{
    let before = unwind_witness_before(None, true);
    unwind_witness_encoded_prefix();
    unwind_witness_encoded_act_plan();
    unwind_witness_validated_plan_matches();
    unwind_witness_expanded_act_events();
    assert(epm::frame_layout_wf(before));
    assert(epm::cursor_event_at(before, 3)
        == Some(epm::WireTopologyEvent::Leave { source_offset: 17 }));
}

/// The ACT event stream is independent of reducer pending/gate and of the
/// historical checkpoint payload saved in the compressed restore run.
pub proof fn unwind_witness_act_layout(
    pending: Option<epm::PathObligation>, saved_stable: bool,
)
    ensures {
        let before = unwind_witness_before(pending, saved_stable);
        &&& epm::frame_layout_wf(before)
        &&& epm::act_plan_events(before, unwind_witness_plan())
            == Some(unwind_witness_events())
        &&& epm::cursor_event_at(before, 3)
            == Some(epm::WireTopologyEvent::Leave { source_offset: 17 })
        &&& epm::trie_position(before, 17) == unwind_witness_anchor(24)
    },
{
    unwind_witness_concrete_act_layout();
    let base = unwind_witness_before(None, true);
    let before = unwind_witness_before(pending, saved_stable);
    let plan = unwind_witness_plan();
    assert(before.snapshot == base.snapshot);
    assert(before.backing == base.backing);
    assert(before.cursor == base.cursor);
    assert(epm::act_plan_matches_frame(before, plan)
        == epm::act_plan_matches_frame(base, plan));
    assert(epm::act_plan_events(before, plan)
        == epm::act_plan_events(base, plan));
    assert(epm::frame_layout_wf(before));
}

pub proof fn accepted_unwind_has_no_current_pending_witness()
    ensures {
        let run = unwind_witness_run(None, true);
        let before = unwind_witness_before(None, true);
        &&& unwind_batch_source_guard(
            before, run, Seq::empty(), 17, 1, unwind_witness_facts(),
        )
        &&& before.path.pending.is_none()
        &&& epm::cursor_event_at(before, 3)
            == Some(epm::WireTopologyEvent::Leave { source_offset: 17 })
        &&& epm::trie_position(before, 17) == unwind_witness_anchor(24)
    },
{
    let before = unwind_witness_before(None, true);
    let run = unwind_witness_run(None, true);
    unwind_witness_act_layout(None, true);
    unwind_witness_historical_run_wf(None, true);
    assert(historical_unwind_accepted(run, 1, unwind_witness_facts()));
    assert(before.path.restore == Seq::empty() + run.checkpoints);
    assert(before.path.path_bytes == run.current.bytes);
    assert(before.path.path_positions == run.current.positions);
    assert(run.current.positions[0] == epm::trie_position(before, 17));
    let chunk = transport::EpmPushTransportChunk::Unwind { start: 17, width: 1 };
    assert(transport::epm_push_chunk_events(chunk).len() == 1);
    assert(transport::epm_push_chunk_event(chunk, 0)
        == epm::EpmEvent::Leave { source_offset: 17 });
    assert(epm::cursor_event_count(before) == 5);
    assert forall|i: int| 0 <= i < transport::epm_push_chunk_events(chunk).len() implies
        #[trigger] epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
            == epm::wire_event(transport::epm_push_chunk_events(chunk)[i]) by {
        assert(i == 0);
    }
    assert(batch_cursor_source_aligned(before, chunk));
    assert(unwind_batch_source_guard(
        before, run, Seq::empty(), 17, 1, unwind_witness_facts(),
    ));
}

pub proof fn accepted_unwind_has_changed_checkpoint_witness()
    ensures {
        let run = unwind_witness_run(Some(unwind_witness_saved_pending()), false);
        let before = unwind_witness_before(Some(unwind_witness_saved_pending()), false);
        let selected = selected_historical_checkpoint(run, 1);
        &&& unwind_batch_source_guard(
            before, run, Seq::empty(), 17, 1, unwind_witness_facts(),
        )
        &&& before.path.pending != selected.checkpoint.pending
        &&& before.path.gate != selected.checkpoint.gate
        &&& epm::trie_position(before, 17) == unwind_witness_anchor(24)
    },
{
    let before = unwind_witness_before(Some(unwind_witness_saved_pending()), false);
    let run = unwind_witness_run(Some(unwind_witness_saved_pending()), false);
    unwind_witness_act_layout(Some(unwind_witness_saved_pending()), false);
    unwind_witness_historical_run_wf(Some(unwind_witness_saved_pending()), false);
    assert(historical_unwind_accepted(run, 1, unwind_witness_facts()));
    assert(before.path.restore == Seq::empty() + run.checkpoints);
    assert(before.path.path_bytes == run.current.bytes);
    assert(before.path.path_positions == run.current.positions);
    assert(run.current.positions[0] == epm::trie_position(before, 17));
    let chunk = transport::EpmPushTransportChunk::Unwind { start: 17, width: 1 };
    assert(transport::epm_push_chunk_events(chunk).len() == 1);
    assert(epm::cursor_event_count(before) == 5);
    assert forall|i: int| 0 <= i < transport::epm_push_chunk_events(chunk).len() implies
        #[trigger] epm::cursor_event_at(before, (before.cursor.at as int + i) as usize)
            == epm::wire_event(transport::epm_push_chunk_events(chunk)[i]) by {
        assert(i == 0);
    }
    assert(batch_cursor_source_aligned(before, chunk));
    assert(unwind_batch_source_guard(
        before, run, Seq::empty(), 17, 1, unwind_witness_facts(),
    ));
}

}
