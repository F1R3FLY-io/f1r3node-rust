use vstd::prelude::*;

verus! {

/// Executable arithmetic kernel of a length-delimited protobuf cursor.  The
/// parser establishes `width <= end - cursor` before advancing, so the update
/// cannot wrap, cannot escape the enclosing message, and makes strict progress.
pub fn checked_cursor_advance(cursor: usize, end: usize, width: usize) -> (next: usize)
    requires
        cursor < end,
        0 < width,
        width <= end - cursor,
    ensures
        next == cursor + width,
        cursor < next,
        next <= end,
{
    cursor + width
}

/// Mathematical image of the scalar accounting fast path.  Demand is counted
/// once for one distinct PathMap endpoint, scaled by its compressed Bag
/// multiplicity, and capped at the public `i64` accounting boundary.
pub open spec fn saturated_scaled_comms_spec(
    current: int,
    per_endpoint: int,
    multiplicity: int,
) -> int {
    if current + per_endpoint * multiplicity > i64::MAX as int {
        i64::MAX as int
    } else {
        current + per_endpoint * multiplicity
    }
}

pub fn saturating_scaled_comms(
    current: u64,
    per_endpoint: u64,
    multiplicity: u64,
) -> (scaled: u64)
    requires
        current <= i64::MAX as u64,
        per_endpoint <= i64::MAX as u64,
    ensures
        scaled <= i64::MAX as u64,
        scaled as int == saturated_scaled_comms_spec(
            current as int,
            per_endpoint as int,
            multiplicity as int,
        ),
{
    let cap = i64::MAX as u64;
    assert(
        (per_endpoint as int) * (multiplicity as int) <= u128::MAX as int
    ) by (nonlinear_arith);
    let product = (per_endpoint as u128) * (multiplicity as u128);
    if product > (cap - current) as u128 {
        cap
    } else {
        current + product as u64
    }
}

/// Whole-trace replay is the observable contract for a stateful region
/// callback.  A compressed endpoint with trace `[A,B]` and multiplicity two
/// must replay `[A,B,A,B]`; grouping equal instruction positions would instead
/// produce `[A,A,B,B]` and is not equivalent.
pub open spec fn repeat_whole_trace(trace: Seq<u8>, multiplicity: nat) -> Seq<u8>
    decreases multiplicity,
{
    if multiplicity == 0 {
        Seq::empty()
    } else {
        trace + repeat_whole_trace(trace, (multiplicity - 1) as nat)
    }
}

proof fn whole_trace_replay_successor(trace: Seq<u8>, multiplicity: nat)
    ensures
        repeat_whole_trace(trace, multiplicity + 1)
            == trace + repeat_whole_trace(trace, multiplicity),
{
}

proof fn grouped_replay_mutant_is_detected()
    ensures
        repeat_whole_trace(seq![1u8, 2u8], 2)
            != seq![1u8, 1u8, 2u8, 2u8],
{
}

}

fn main() {}
