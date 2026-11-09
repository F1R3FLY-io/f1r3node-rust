//! Source-shaped arithmetic for `CanonicalPathBatchForwardCapacitySource`.
//! This models the real `forward_capacity` and `reusable_payload_prefix`
//! minima and checked failures. The theorem derives the EPathMap forward
//! capacity facts for any accepted width; it does not assume that predicate.
//!
//! The observer now exposes the raw fault, owner-root, top Payload remaining,
//! and test-only bulk-counter fields. Positive Rust capacity implies the
//! formal `fault_free` fact only with a separate reachable owner-root health
//! invariant: the production capacity guard itself checks `state.fault` and
//! current `PathStatus::Open`, while `fault_free` also checks the owner root.

use vstd::prelude::*;

use crate::{rpb3_epathmap_event_refinement as epm, rpb3_epm_restore_run_expansion as restore};

verus! {

pub struct ObservedForwardPending {
    pub payload_start: usize,
    pub remaining: u64,
    pub awaiting_resolution: bool,
}

/// The fields used from `CanonicalPathBatchForwardCapacitySource` and its
/// nested `CanonicalPathBatchHandleSource`.
pub struct ObservedForwardCapacity {
    pub depth: usize,
    pub stamp: usize,
    pub open: bool,
    pub fault_free: bool,
    pub owner_boundary_end: Option<u128>,
    pub pending: Option<ObservedForwardPending>,
    pub available: usize,
    pub capacity: usize,
    pub root_left: bool,
    pub source_present: bool,
    pub edges_before: usize,
    pub state_fault_none: bool,
    pub owner_root_open: bool,
    pub top_is_payload: bool,
    pub top_payload_remaining: Option<u64>,
    pub test_bulk_operations: Option<usize>,
    pub test_bulk_bytes: Option<usize>,
}

/// The production observer always returns both test counters or neither.
pub open spec fn test_bulk_mode_wf(observed: ObservedForwardCapacity) -> bool {
    observed.test_bulk_operations.is_some() == observed.test_bulk_bytes.is_some()
}

pub open spec fn min_usize(a: usize, b: usize) -> usize {
    if a <= b { a } else { b }
}

/// `usize::try_from(value).unwrap_or(usize::MAX)` in the reducer.
pub open spec fn clamp_u128_to_usize(value: u128) -> usize {
    if value as int > usize::MAX as int { usize::MAX } else { value as usize }
}

pub open spec fn clamp_u64_to_usize(value: u64) -> usize {
    if value as int > usize::MAX as int { usize::MAX } else { value as usize }
}

/// The owner check performs `end.checked_sub(stamp)?.checked_sub(1)?`.
pub open spec fn owner_boundary_remaining(
    observed: ObservedForwardCapacity,
) -> Option<usize> {
    match observed.owner_boundary_end {
        None => Some(usize::MAX),
        Some(end) => {
            if end as int <= observed.stamp as int {
                None
            } else {
                Some(clamp_u128_to_usize(
                    (end as int - observed.stamp as int - 1) as u128,
                ))
            }
        },
    }
}

/// Exact non-test `forward_capacity` path, plus its `#[cfg(test)]` cap when
/// both test counters are present. A missing/invalid Payload remaining or checked
/// owner distance produces zero, as `Option::map_or(0, ...)` does in Rust.
pub open spec fn exact_forward_capacity(observed: ObservedForwardCapacity) -> usize {
    if observed.root_left || !observed.source_present
        || !observed.state_fault_none || !observed.open || !observed.top_is_payload
        || observed.top_payload_remaining.is_none()
    {
        0
    } else {
        let remaining = observed.top_payload_remaining.unwrap();
        if remaining == 0 {
            0
        } else {
            let available = min_usize(
                min_usize(
                    observed.available,
                    (usize::MAX as int - observed.depth as int) as usize,
                ),
                (usize::MAX as int - observed.edges_before as int) as usize,
            );
            let available = match (observed.test_bulk_operations, observed.test_bulk_bytes) {
                (None, None) => available,
                (Some(operations), Some(bytes)) => {
                    if operations == usize::MAX {
                        0
                    } else {
                        min_usize(
                            available,
                            (usize::MAX as int - bytes as int) as usize,
                        )
                    }
                },
                _ => 0,
            };
            let limited = min_usize(
                min_usize(available, clamp_u64_to_usize((remaining as int - 1) as u64)),
                (usize::MAX as int - observed.stamp as int) as usize,
            );
            match owner_boundary_remaining(observed) {
                None => 0,
                Some(owner) => min_usize(limited, owner),
            }
        }
    }
}

pub open spec fn forward_facts_from_source(
    observed: ObservedForwardCapacity,
) -> restore::ForwardCapacityFacts {
    restore::ForwardCapacityFacts {
        source_present: observed.source_present,
        root_left: observed.root_left,
        state_open: observed.open,
        fault_free: observed.fault_free,
        stamp: observed.stamp,
        owner_boundary_remaining: match owner_boundary_remaining(observed) {
            Some(distance) => distance,
            None => 0,
        },
        edges_before: observed.edges_before,
    }
}

/// Field and byte-view extraction still owed from Rust. The positive branch
/// relates the observer's pending progress to the formal Payload obligation.
/// `fault_free` is explicit because owner-root death is absent from the
/// production `reusable_payload_prefix` guard.
pub open spec fn forward_source_projection(
    observed: ObservedForwardCapacity,
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
) -> bool {
    &&& test_bulk_mode_wf(observed)
    &&& observed.capacity == exact_forward_capacity(observed)
    &&& restore::source_tree_wf(base)
    &&& epm::indexed_image_wf(source)
    &&& base.bytes.len() == observed.depth as int
    &&& start as int + observed.available as int <= source.bytes.len()
    // Source semantics of `dead_error`: an open, fault-free state with an
    // open root must produce a fault-free handle observation.
    &&& (observed.open && observed.state_fault_none && observed.owner_root_open
        ==> observed.fault_free)
    &&& if observed.capacity > 0 {
        // Reachability: capture saves only Open owners, even across children.
        &&& observed.owner_root_open
        &&& observed.stamp == observed.depth
        &&& observed.pending.is_some()
        &&& observed.top_payload_remaining.is_some()
        &&& !observed.pending.unwrap().awaiting_resolution
        &&& !pending.awaiting_resolution
        &&& observed.pending.unwrap().payload_start == pending.payload_start
        &&& pending.payload_start as int <= base.bytes.len()
        &&& observed.pending.unwrap().remaining == observed.top_payload_remaining.unwrap()
        &&& observed.pending.unwrap().remaining as int
            == pending.payload_len as int
                - (base.bytes.len() - pending.payload_start as int)
    } else {
        true
    }
}

/// Every accepted positive width inherits each minimum's bound, including
/// checked owner distance, `remaining - 1`, depth/stamp and edge headroom.
pub proof fn positive_forward_capacity_bounds(
    observed: ObservedForwardCapacity,
    width: usize,
)
    requires
        width > 0,
        width <= exact_forward_capacity(observed),
    ensures
        observed.source_present,
        !observed.root_left,
        observed.state_fault_none,
        observed.open,
        observed.top_is_payload,
        observed.top_payload_remaining.is_some(),
        observed.top_payload_remaining.unwrap() as int > width as int,
        owner_boundary_remaining(observed).is_some(),
        width <= observed.available,
        width as int + observed.depth as int <= usize::MAX as int,
        width as int + observed.stamp as int <= usize::MAX as int,
        width as int + observed.edges_before as int <= usize::MAX as int,
        width <= owner_boundary_remaining(observed).unwrap(),
{
    assert(observed.top_payload_remaining.unwrap() > 0);
    assert(owner_boundary_remaining(observed).is_some());
    let remaining = observed.top_payload_remaining.unwrap();
    assert(width <= clamp_u64_to_usize((remaining as int - 1) as u64));
    assert(width as int <= remaining as int - 1);
}

/// An arbitrary positive width admitted by the production source capacity
/// satisfies the formal Forward guard once only source-field and byte-view
/// projections above are established. No final guard predicate is assumed.
pub proof fn accepted_width_derives_forward_capacity_facts(
    observed: ObservedForwardCapacity,
    base: epm::SourceTreeImage,
    pending: epm::PathObligation,
    source: epm::IndexedRegionImage,
    start: usize,
    width: usize,
)
    requires
        forward_source_projection(observed, base, pending, source, start),
        width > 0,
        width <= observed.capacity,
    ensures restore::forward_capacity_facts_match(
        base, pending, source, start, width, forward_facts_from_source(observed),
    ),
{
    positive_forward_capacity_bounds(observed, width);
    assert(observed.owner_root_open);
    assert(observed.fault_free);
    assert(start as int + width as int <= source.bytes.len());
    assert(base.bytes.len() + width as int <= usize::MAX as int);
    assert(pending.payload_start as int <= base.bytes.len());
    assert(observed.top_payload_remaining.unwrap() as int > width as int);
    assert((base.bytes.len() + width as int - pending.payload_start as int)
        < pending.payload_len as int);
}

/// The arithmetic has a concrete nonzero case and is not made true only by
/// an impossible source guard. Here the owner boundary is absent.
pub proof fn three_byte_capacity_witness() {
    let observed = ObservedForwardCapacity {
        depth: 2,
        stamp: 2,
        open: true,
        fault_free: true,
        owner_boundary_end: None,
        pending: Some(ObservedForwardPending {
            payload_start: 2,
            remaining: 10,
            awaiting_resolution: false,
        }),
        available: 3,
        capacity: 3,
        root_left: false,
        source_present: true,
        edges_before: 2,
        state_fault_none: true,
        owner_root_open: true,
        top_is_payload: true,
        top_payload_remaining: Some(10),
        test_bulk_operations: None,
        test_bulk_bytes: None,
    };
    assert(exact_forward_capacity(observed) == 3);
    positive_forward_capacity_bounds(observed, 3);
}

}

fn main() {}
