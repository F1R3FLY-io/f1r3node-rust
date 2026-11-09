//! Source-shaped restore-run accounting. The capacity facts and state token
//! still require a proved projection from the production reducer.

use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epathmap_event_refinement.rs"]
mod epm;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub struct ForwardCapacityFacts {
    pub source_present: bool,
    pub root_left: bool,
    pub state_open: bool,
    pub fault_free: bool,
    pub stamp: usize,
    pub owner_boundary_remaining: usize,
    pub edges_before: usize,
}

pub struct UnwindCapacityFacts {
    pub root_left: bool,
    pub available: usize,
    pub selected_slice_available: bool,
    pub remaining_slice_available: bool,
}

pub struct RestoreRunImage {
    pub first_depth: usize,
    pub latest_depth: usize,
    pub state_id: usize,
    pub base_pending: epm::PathObligation,
    pub gate: epm::PathGate,
    pub ghost base_parser: epm::parser::ParserImage,
    pub ghost current: epm::SourceTreeImage,
    pub ghost latest: epm::SourceTreeImage,
}

pub open spec fn source_tree_wf(source: epm::SourceTreeImage) -> bool {
    source.bytes.len() == source.positions.len()
}

pub open spec fn run_width(run: RestoreRunImage) -> int {
    run.latest_depth as int - run.first_depth as int + 1
}

pub open spec fn source_prefix(source: epm::SourceTreeImage, len: int)
    -> epm::SourceTreeImage
    recommends 0 <= len <= source.bytes.len(), source_tree_wf(source),
{
    epm::SourceTreeImage {
        bytes: source.bytes.subrange(0, len),
        positions: source.positions.subrange(0, len),
    }
}

pub open spec fn pending_at_depth(
    run: RestoreRunImage, depth: int,
) -> epm::PathObligation
    recommends run_wf(run), run.first_depth as int <= depth <= run.latest_depth as int,
{
    let base = run.base_pending;
    epm::PathObligation {
        id: base.id,
        kind: base.kind,
        payload_len: base.payload_len,
        tag_source: base.tag_source,
        payload_start: base.payload_start,
        payload_end: if depth == run.first_depth as int {
            base.payload_end
        } else {
            run.current.positions[depth - 1]
        },
        awaiting_resolution: false,
    }
}

pub open spec fn run_current_pending(run: RestoreRunImage)
    -> epm::PathObligation
    recommends run_wf(run),
{
    let base = run.base_pending;
    epm::PathObligation {
        id: base.id,
        kind: base.kind,
        payload_len: base.payload_len,
        tag_source: base.tag_source,
        payload_start: base.payload_start,
        payload_end: run.current.positions[run.current.positions.len() - 1],
        awaiting_resolution: false,
    }
}

pub open spec fn run_checkpoint(run: RestoreRunImage, index: int)
    -> epm::PathCheckpoint
    recommends run_wf(run), 0 <= index < run_width(run),
{
    let depth = run.first_depth as int + index;
    epm::PathCheckpoint {
        pending: Some(pending_at_depth(run, depth)),
        gate: run.gate,
        path_len: depth as usize,
        parser: epm::parser::forward_parser_payload(run.base_parser,
            run.current.bytes.subrange(run.first_depth as int, depth)),
    }
}

pub open spec fn expanded_run(run: RestoreRunImage) -> Seq<epm::PathCheckpoint>
    recommends run_wf(run),
{
    Seq::new(run_width(run) as nat, |i: int| run_checkpoint(run, i))
}

pub open spec fn run_wf(run: RestoreRunImage) -> bool {
    &&& run.first_depth <= run.latest_depth
    &&& run.latest_depth < usize::MAX
    &&& source_tree_wf(run.current)
    &&& source_tree_wf(run.latest)
    &&& run.current.bytes.len() == run.latest_depth as int + 1
    &&& run.current.positions.len() > 0
    &&& run.latest.bytes == run.current.bytes.subrange(0, run.latest_depth as int)
    &&& run.latest.positions
        == run.current.positions.subrange(0, run.latest_depth as int)
    &&& run.base_pending.payload_start <= run.first_depth
    &&& !run.base_pending.awaiting_resolution
    &&& run.base_parser.stamp == run.first_depth
    &&& epm::parser_gate_matches(run.gate, run.base_parser)
    &&& epm::parser::parser_forward_capacity(run.base_parser,
        run.current.bytes.len() - run.first_depth as int)
    &&& run.current.bytes.len() - run.base_pending.payload_start
        < run.base_pending.payload_len
}

pub open spec fn forward_capacity_facts_match(
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
    facts: ForwardCapacityFacts,
) -> bool {
    &&& source_tree_wf(base)
    &&& epm::indexed_image_wf(source)
    &&& width > 0
    &&& start as int + width as int <= source.bytes.len()
    &&& base.bytes.len() + width as int <= usize::MAX as int
    &&& pending.payload_start as int <= base.bytes.len()
    &&& !pending.awaiting_resolution
    &&& (base.bytes.len() + width as int - pending.payload_start as int)
        < (pending.payload_len as int)
    &&& facts.source_present
    &&& !facts.root_left
    &&& facts.state_open
    &&& facts.fault_free
    &&& facts.stamp as int == base.bytes.len()
    &&& width <= facts.owner_boundary_remaining
    &&& width as int + facts.edges_before as int <= usize::MAX as int
}

pub open spec fn forward_published_current(
    base: epm::SourceTreeImage,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
) -> epm::SourceTreeImage
    recommends source_tree_wf(base), epm::indexed_image_wf(source),
        start as int + width as int <= source.bytes.len(),
{
    epm::SourceTreeImage {
        bytes: base.bytes + source.bytes.subrange(start as int, start as int + width as int),
        positions: base.positions
            + source.positions.subrange(start as int, start as int + width as int),
    }
}

pub open spec fn publish_forward_run(
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    gate: epm::PathGate,
    base_parser: epm::parser::ParserImage,
    state_id: usize,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
) -> RestoreRunImage
    recommends source_tree_wf(base), epm::indexed_image_wf(source), width > 0,
        start as int + width as int <= source.bytes.len(),
        base.bytes.len() + width as int <= usize::MAX as int,
{
    let current = forward_published_current(base, source, start, width);
    let latest_depth = base.bytes.len() + width as int - 1;
    RestoreRunImage {
        first_depth: base.bytes.len() as usize,
        latest_depth: latest_depth as usize,
        state_id,
        base_pending: pending,
        gate,
        base_parser,
        current,
        latest: source_prefix(current, latest_depth),
    }
}

pub proof fn accepted_forward_publishes_exact_virtual_checkpoints(
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    gate: epm::PathGate,
    base_parser: epm::parser::ParserImage,
    state_id: usize,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
    facts: ForwardCapacityFacts,
)
    requires
        forward_capacity_facts_match(base, pending, source, start, width, facts),
        base_parser.stamp as int == base.bytes.len(),
        epm::parser_gate_matches(gate, base_parser),
        epm::parser::parser_forward_capacity(base_parser, width as int),
    ensures {
        let run = publish_forward_run(base, pending, gate, base_parser,
            state_id, source, start, width);
        &&& run_wf(run)
        &&& run_width(run) == width as int
        &&& expanded_run(run).len() == width as int
        &&& run.first_depth as int == base.bytes.len()
        &&& run.latest_depth as int == base.bytes.len() + width as int - 1
        &&& run.state_id == state_id
        &&& run_current_pending(run).payload_end
            == source.positions[start as int + width as int - 1]
        &&& forall|i: int| 0 <= i < width as int ==> {
            let checkpoint = #[trigger] expanded_run(run)[i];
            &&& checkpoint.path_len as int == base.bytes.len() + i
            &&& checkpoint.gate == gate
            &&& match checkpoint.pending {
                Some(obligation) => {
                    &&& obligation.id == pending.id
                    &&& obligation.kind == pending.kind
                    &&& !obligation.awaiting_resolution
                    &&& obligation.payload_end == if i == 0 {
                        pending.payload_end
                    } else {
                        source.positions[start as int + i - 1]
                    }
                },
                None => false,
            }
        }
    },
{
    let run = publish_forward_run(base, pending, gate, base_parser,
        state_id, source, start, width);
    assert(source_tree_wf(run.current));
    assert(run.current.bytes.len() == base.bytes.len() + width as int);
    assert(run.latest.bytes == run.current.bytes.subrange(0, run.latest_depth as int));
    assert(run_wf(run));
    assert(run.current.positions.len() > 0);
    assert(run.current.positions[run.current.positions.len() - 1]
        == source.positions[start as int + width as int - 1]);
    assert forall|i: int| 0 <= i < width as int implies {
        let checkpoint = #[trigger] expanded_run(run)[i];
        &&& checkpoint.path_len as int == base.bytes.len() + i
        &&& checkpoint.gate == gate
        &&& match checkpoint.pending {
            Some(obligation) => {
                &&& obligation.id == pending.id
                &&& obligation.kind == pending.kind
                &&& !obligation.awaiting_resolution
                &&& obligation.payload_end == if i == 0 {
                    pending.payload_end
                } else {
                    source.positions[start as int + i - 1]
                }
            },
            None => false,
        }
    } by {
        if i > 0 {
            assert(run.current.positions[base.bytes.len() + i - 1]
                == source.positions[start as int + i - 1]);
        }
    }
}

pub open spec fn coalesced_forward_facts_match(
    run: RestoreRunImage,
    current_state_id: usize,
    current_pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
    facts: ForwardCapacityFacts,
) -> bool {
    &&& run_wf(run)
    &&& current_state_id == run.state_id
    &&& current_pending == run_current_pending(run)
    &&& forward_capacity_facts_match(
        run.current, current_pending, source, start, width, facts,
    )
    &&& epm::parser::parser_forward_capacity(run.base_parser,
        run.current.bytes.len() + width as int - run.first_depth as int)
}

pub open spec fn extend_coalesced_forward_run(
    run: RestoreRunImage,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
) -> RestoreRunImage
    recommends run_wf(run), epm::indexed_image_wf(source), width > 0,
        start as int + width as int <= source.bytes.len(),
        run.current.bytes.len() + width as int <= usize::MAX as int,
{
    let current = forward_published_current(run.current, source, start, width);
    let latest_depth = current.bytes.len() - 1;
    RestoreRunImage {
        first_depth: run.first_depth,
        latest_depth: latest_depth as usize,
        state_id: run.state_id,
        base_pending: run.base_pending,
        gate: run.gate,
        base_parser: run.base_parser,
        current,
        latest: source_prefix(current, latest_depth),
    }
}

pub proof fn accepted_coalesced_forward_extends_virtual_checkpoints(
    run: RestoreRunImage,
    current_state_id: usize,
    current_pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
    facts: ForwardCapacityFacts,
)
    requires coalesced_forward_facts_match(
        run, current_state_id, current_pending, source, start, width, facts,
    ),
    ensures {
        let after = extend_coalesced_forward_run(run, source, start, width);
        &&& run_wf(after)
        &&& after.first_depth == run.first_depth
        &&& after.state_id == current_state_id
        &&& run_current_pending(run) == current_pending
        &&& run_current_pending(after).payload_end
            == source.positions[start as int + width as int - 1]
        &&& run_width(after) == run_width(run) + width as int
        &&& after.current.bytes == run.current.bytes
            + source.bytes.subrange(start as int, start as int + width as int)
        &&& after.current.positions == run.current.positions
            + source.positions.subrange(start as int, start as int + width as int)
        &&& forall|i: int| 0 <= i < run_width(run) ==>
            #[trigger] expanded_run(after)[i] == expanded_run(run)[i]
        &&& forall|i: int| 0 <= i < width as int ==> {
            let checkpoint = #[trigger] expanded_run(after)[run_width(run) + i];
            &&& checkpoint.path_len as int == run.current.bytes.len() + i
            &&& match checkpoint.pending {
                Some(obligation) => {
                    &&& obligation.id == run.base_pending.id
                    &&& !obligation.awaiting_resolution
                    &&& obligation.payload_end == if i == 0 {
                        run.current.positions[run.current.positions.len() - 1]
                    } else {
                        source.positions[start as int + i - 1]
                    }
                },
                None => false,
            }
        }
    },
{
    let after = extend_coalesced_forward_run(run, source, start, width);
    assert(run_wf(after));
    assert(run.current.positions.len() > 0);
    assert(after.current.positions[after.current.positions.len() - 1]
        == source.positions[start as int + width as int - 1]);
    assert forall|i: int| 0 <= i < run_width(run) implies
        #[trigger] expanded_run(after)[i] == expanded_run(run)[i] by {
        let depth = run.first_depth as int + i;
        assert(run.first_depth as int <= depth < run.current.bytes.len());
        assert(after.current.bytes.subrange(run.first_depth as int, depth)
            == run.current.bytes.subrange(run.first_depth as int, depth));
        if i > 0 {
            assert(after.current.positions[depth - 1]
                == run.current.positions[depth - 1]);
        }
    }
    assert forall|i: int| 0 <= i < width as int implies {
        let checkpoint = #[trigger] expanded_run(after)[run_width(run) + i];
        &&& checkpoint.path_len as int == run.current.bytes.len() + i
        &&& match checkpoint.pending {
            Some(obligation) => {
                &&& obligation.id == run.base_pending.id
                &&& !obligation.awaiting_resolution
                &&& obligation.payload_end == if i == 0 {
                    run.current.positions[run.current.positions.len() - 1]
                } else {
                    source.positions[start as int + i - 1]
                }
            },
            None => false,
        }
    } by {
        let depth = run.current.bytes.len() + i;
        if i == 0 {
            assert(after.current.positions[depth - 1]
                == run.current.positions[run.current.positions.len() - 1]);
        } else {
            assert(after.current.positions[depth - 1]
                == source.positions[start as int + i - 1]);
        }
    }
}

pub open spec fn accepted_unwind(
    run: RestoreRunImage, width: usize, facts: UnwindCapacityFacts,
) -> bool {
    &&& run_wf(run)
    &&& !facts.root_left
    &&& width > 0
    &&& width <= facts.available
    &&& width as int <= run_width(run)
    &&& (width == 1 || facts.selected_slice_available)
    &&& (width as int == run_width(run) || facts.remaining_slice_available)
}

pub open spec fn unwind_selected(
    run: RestoreRunImage, width: usize, facts: UnwindCapacityFacts,
)
    -> epm::SourceTreeImage
    recommends accepted_unwind(run, width, facts),
{
    source_prefix(run.current, run.current.bytes.len() - width as int)
}

pub open spec fn unwind_remaining(
    run: RestoreRunImage, width: usize, facts: UnwindCapacityFacts,
)
    -> Option<RestoreRunImage>
    recommends accepted_unwind(run, width, facts),
{
    if width as int == run_width(run) {
        None
    } else {
        let selected = unwind_selected(run, width, facts);
        let latest_depth = run.latest_depth as int - width as int;
        Some(RestoreRunImage {
            first_depth: run.first_depth,
            latest_depth: latest_depth as usize,
            state_id: run.state_id,
            base_pending: run.base_pending,
            gate: run.gate,
            base_parser: run.base_parser,
            current: selected,
            latest: source_prefix(selected, latest_depth),
        })
    }
}

pub proof fn accepted_unwind_selects_prefix_and_pops_exact_checkpoints(
    run: RestoreRunImage,
    width: usize,
    facts: UnwindCapacityFacts,
)
    requires accepted_unwind(run, width, facts),
    ensures {
        let selected = unwind_selected(run, width, facts);
        let remaining = unwind_remaining(run, width, facts);
        &&& source_tree_wf(selected)
        &&& selected.bytes.len() == run.current.bytes.len() - width as int
        &&& selected.positions.len() == run.current.positions.len() - width as int
        &&& selected.bytes
            == run.current.bytes.subrange(0, run.current.bytes.len() - width as int)
        &&& selected.positions
            == run.current.positions.subrange(0, run.current.positions.len() - width as int)
        &&& match remaining {
            None => width as int == run_width(run),
            Some(rest) => {
                &&& (width as int) < run_width(run)
                &&& run_wf(rest)
                &&& rest.state_id == run.state_id
                &&& run_width(rest) == run_width(run) - width as int
                &&& rest.latest_depth as int == run.latest_depth as int - width as int
                &&& rest.current.bytes == selected.bytes
                &&& rest.current.positions == selected.positions
                &&& forall|i: int| 0 <= i < run_width(rest) ==>
                    #[trigger] expanded_run(rest)[i] == expanded_run(run)[i]
            },
        }
    },
{
    let selected = unwind_selected(run, width, facts);
    let remaining = unwind_remaining(run, width, facts);
    if (width as int) < run_width(run) {
        let rest = remaining.unwrap();
        assert(selected.bytes.len() > 0);
        assert(run_wf(rest));
        assert forall|i: int| 0 <= i < run_width(rest) implies
            #[trigger] expanded_run(rest)[i] == expanded_run(run)[i] by {
            let depth = run.first_depth as int + i;
            assert(depth < selected.bytes.len());
            assert(rest.current.bytes.subrange(run.first_depth as int, depth)
                == run.current.bytes.subrange(run.first_depth as int, depth));
            if i > 0 {
                assert(rest.current.positions[depth - 1]
                    == run.current.positions[depth - 1]);
            }
        }
    }
}

}

fn main() {}
