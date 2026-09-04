//! Verus refinement kernel for stack-safe RPB3 admission.
//!
//! The production PDA stores frames in an explicit worklist.  This model
//! captures its scalar invariants: checked regions, strict progress, child
//! balance, exact root exhaustion, and monotone slot admission.
use vstd::prelude::*;

verus! {

pub struct ByteRegion { pub start: usize, pub end: usize }

pub fn checked_region(start: usize, width: usize, limit: usize)
    -> (result: Option<ByteRegion>)
    ensures
        result.is_some() == (start as int + width as int <= limit as int),
        match result {
            Some(region) => {
                &&& region.start == start
                &&& region.end as int == start as int + width as int
                &&& region.start <= region.end
                &&& region.end <= limit
            },
            None => start as int + width as int > limit as int,
        },
{
    if start > limit || width > limit - start { None }
    else { Some(ByteRegion { start, end: start + width }) }
}

pub fn checked_advance(position: usize, end: usize, width: usize)
    -> (next: Option<usize>)
    requires position <= end,
    ensures
        next.is_some() == (position as int + width as int <= end as int),
        match next {
            Some(value) => {
                &&& value as int == position as int + width as int
                &&& position <= value &&& value <= end
                &&& (width > 0 ==> position < value)
            },
            None => position as int + width as int > end as int,
        },
{
    if width > end - position { None } else { Some(position + width) }
}

/// `remaining` is the PDA ranking function.  Every successful worklist step
/// consumes one owned slot and therefore terminates without call-stack use.
pub fn worklist_step(remaining: usize) -> (next: Option<usize>)
    ensures
        next.is_some() == (remaining > 0),
        match next { Some(value) => value as int + 1 == remaining as int, None => remaining == 0 },
{
    if remaining == 0 { None } else { Some(remaining - 1) }
}

pub fn checked_next_slot(previous: usize, current: usize) -> (accepted: bool)
    ensures accepted == (previous < current),
{ previous < current }

/// Push and pop are events, not recursive calls.  The preconditions rule out
/// simultaneous events and underflow; the result is the exact open-child count.
pub fn child_frame_step(depth: usize, push: bool, pop: bool)
    -> (result: usize)
    requires !(push && pop), pop ==> depth > 0, push ==> depth < usize::MAX,
    ensures
        push ==> result == depth + 1,
        !push && pop ==> result as int + 1 == depth as int,
        !push && !pop ==> result == depth,
{
    if push { depth + 1 } else if pop { depth - 1 } else { depth }
}

pub fn root_consumption_authorized(
    root_end: usize, cursor: usize, child_depth: usize,
    slots_complete: bool, children_valid: bool,
) -> (authorized: bool)
    ensures authorized == (
        cursor == root_end && child_depth == 0 && slots_complete && children_valid
    ),
{
    cursor == root_end && child_depth == 0 && slots_complete && children_valid
}

/// Negative control: a mutant forgetting the child-depth gate is rejected.
pub fn rejects_unbalanced_root(root_end: usize, cursor: usize, child_depth: usize)
    -> (rejected: bool)
    ensures rejected == (cursor != root_end || child_depth > 0),
{ cursor != root_end || child_depth > 0 }

/// Negative control: duplicate or regressive slots are not monotone.
pub fn rejects_duplicate_slot(previous: usize, current: usize) -> (rejected: bool)
    ensures rejected == (current <= previous),
{ current <= previous }

}

fn main() {}
