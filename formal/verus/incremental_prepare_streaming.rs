use vstd::prelude::*;

verus! {

/// Production kind-1 recovery uses the same fixed scratch ceiling as the
/// checkpoint streaming path.  The proof obligations below make the bound a
/// property of every read request rather than a testing convention.
pub const STREAM_SCRATCH_BYTES: usize = 64 * 1024;
pub const FIXED_STREAM_STATE_BYTES: usize = 1024;

pub open spec fn checked_extent_spec(cursor: int, width: int, end: int) -> Option<int> {
    if cursor < 0 || width < 0 || end < 0 || cursor + width > end {
        None
    } else {
        Some(cursor + width)
    }
}

/// Exact checked cursor movement for authenticated WAL extents.  A successful
/// positive-width transition is monotone, remains within the admitted end,
/// and decreases the finite remaining-byte measure.
pub fn checked_extent_advance(
    cursor: u64,
    width: u64,
    end: u64,
) -> (result: Option<u64>)
    ensures
        match result {
            Some(next) => {
                &&& checked_extent_spec(cursor as int, width as int, end as int)
                    == Some(next as int)
                &&& cursor <= next
                &&& next <= end
                &&& (0 < width ==> cursor < next)
                &&& (0 < width ==> end - next < end - cursor)
            },
            None => checked_extent_spec(cursor as int, width as int, end as int).is_none(),
        },
{
    if cursor > end || width > end - cursor {
        assert(cursor as int + width as int > end as int);
        None
    } else {
        let next = cursor + width;
        assert(next as int == cursor as int + width as int);
        Some(next)
    }
}

/// Select the next physical read without deriving an allocation or request
/// size from the record.  The only scalable quantity is the number of loop
/// iterations; retained scratch is fixed.
pub fn next_stream_read_width(cursor: u64, end: u64) -> (width: usize)
    requires cursor < end,
    ensures
        0 < width,
        width <= STREAM_SCRATCH_BYTES,
        width as int <= end as int - cursor as int,
{
    let remaining = end - cursor;
    if remaining < STREAM_SCRATCH_BYTES as u64 {
        assert(0 < remaining);
        remaining as usize
    } else {
        STREAM_SCRATCH_BYTES
    }
}

/// Admission is exact: the boundary is accepted and every smaller limit is
/// rejected.  This function is shared conceptually by record, mutation,
/// object, entry, and key-byte limits.
pub open spec fn resource_admitted_spec(required: int, limit: int) -> bool {
    required <= limit
}

pub fn resource_admitted(required: u64, limit: u64) -> (accepted: bool)
    ensures accepted == resource_admitted_spec(required as int, limit as int),
{
    required <= limit
}

pub proof fn exact_resource_boundary_is_accepted(required: u64)
    ensures resource_admitted_spec(required as int, required as int),
{
}

pub proof fn required_minus_one_is_rejected(required: u64)
    requires 0 < required,
    ensures !resource_admitted_spec(required as int, required as int - 1),
{
}

pub struct LayoutAdvance {
    pub next_ordinal: u64,
    pub next_offset: u64,
}

pub open spec fn layout_advance_spec(
    ordinal: int,
    offset: int,
    body_len: int,
    end: int,
) -> Option<(int, int)> {
    if ordinal < 0
        || offset < 0
        || body_len < 0
        || end < 0
        || ordinal + 1 > u64::MAX as int
        || offset + body_len > end
    {
        None
    } else {
        Some((ordinal + 1, offset + body_len))
    }
}

/// Arithmetic kernel for the header-only replay transcript.  Each accepted
/// object advances the ordinal once and its body extent exactly; overflow or
/// crossing the authenticated trailer is a typed rejection.
pub fn checked_layout_advance(
    ordinal: u64,
    offset: u64,
    body_len: u64,
    end: u64,
) -> (result: Option<LayoutAdvance>)
    ensures
        match result {
            Some(step) => {
                &&& layout_advance_spec(
                    ordinal as int,
                    offset as int,
                    body_len as int,
                    end as int,
                ) == Some((step.next_ordinal as int, step.next_offset as int))
                &&& step.next_ordinal == ordinal + 1
                &&& step.next_offset as int == offset as int + body_len as int
                &&& step.next_offset <= end
            },
            None => layout_advance_spec(
                ordinal as int,
                offset as int,
                body_len as int,
                end as int,
            ).is_none(),
        },
{
    if ordinal == u64::MAX {
        assert(ordinal as int + 1 > u64::MAX as int);
        None
    } else {
        match checked_extent_advance(offset, body_len, end) {
            None => None,
            Some(next_offset) => {
                let next_ordinal = ordinal + 1;
                assert(next_ordinal as int == ordinal as int + 1);
                Some(LayoutAdvance { next_ordinal, next_offset })
            },
        }
    }
}

/// Accumulate actual physical reread work independently from the authorized
/// pack extent.  This prevents replay/prior-key reads from being hidden or
/// double-charged as logical scan bytes.
pub fn checked_add_physical_work(
    completed: u64,
    requested: u64,
) -> (result: Option<u64>)
    ensures
        match result {
            Some(total) => {
                &&& total as int == completed as int + requested as int
                &&& completed <= total
                &&& (0 < requested ==> completed < total)
            },
            None => completed as int + requested as int > u64::MAX as int,
        },
{
    if requested > u64::MAX - completed {
        None
    } else {
        Some(completed + requested)
    }
}

/// A variable-size read is impossible until the fixed authenticated prefix and
/// every declared resource have been admitted.  Once admitted, the returned
/// request obeys the same fixed scratch bound as `next_stream_read_width`.
pub fn admitted_stream_read_width(
    admitted: bool,
    cursor: u64,
    end: u64,
) -> (result: Option<usize>)
    requires cursor <= end,
    ensures
        !admitted ==> result.is_none(),
        match result {
            Some(width) => {
                &&& admitted
                &&& cursor < end
                &&& 0 < width
                &&& width <= STREAM_SCRATCH_BYTES
                &&& width as int <= end as int - cursor as int
            },
            None => true,
        },
{
    if !admitted || cursor == end {
        None
    } else {
        Some(next_stream_read_width(cursor, end))
    }
}

/// Mathematical auxiliary-space model.  Record, key, and object lengths are
/// quantified but do not occur in the result: retaining any of them would
/// invalidate this theorem and its executable allocation property.
pub open spec fn auxiliary_stream_bytes(
    _record_len: int,
    _key_len: int,
    _object_len: int,
) -> int {
    STREAM_SCRATCH_BYTES as int + FIXED_STREAM_STATE_BYTES as int
}

pub proof fn auxiliary_memory_has_zero_input_slope(
    record_a: int,
    record_b: int,
    key_a: int,
    key_b: int,
    object_a: int,
    object_b: int,
)
    ensures
        auxiliary_stream_bytes(record_a, key_a, object_a)
            == auxiliary_stream_bytes(record_b, key_b, object_b),
{
}

/// The stream accepts only after all seven independent gates pass.  Keeping
/// this Boolean kernel explicit makes early authority/index exposure a direct
/// executable refinement obligation.
pub fn validation_accepted(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    replay_exact: bool,
    source_stable: bool,
) -> (accepted: bool)
    ensures
        accepted == (
            extent_complete
            && crc_valid
            && semantics_valid
            && commit_bound
            && chain_bound
            && replay_exact
            && source_stable
        ),
{
    extent_complete
        && crc_valid
        && semantics_valid
        && commit_bound
        && chain_bound
        && replay_exact
        && source_stable
}

} // verus!

fn main() {}
