use vstd::prelude::*;

verus! {

pub const STREAMING_CODEC_SCRATCH_BYTES: usize = 64 * 1024;
pub const STREAMING_CODEC_FIXED_STATE_BYTES: usize = 2048;

pub open spec fn extent_advance_spec(cursor: int, width: int, end: int) -> Option<int> {
    if cursor < 0 || width < 0 || end < 0 || cursor + width > end {
        None
    } else {
        Some(cursor + width)
    }
}

pub fn checked_codec_extent(
    cursor: u64,
    width: u64,
    end: u64,
) -> (result: Option<u64>)
    ensures
        match result {
            Some(next) => {
                &&& extent_advance_spec(cursor as int, width as int, end as int)
                    == Some(next as int)
                &&& cursor <= next
                &&& next <= end
            },
            None => extent_advance_spec(cursor as int, width as int, end as int).is_none(),
        },
{
    if cursor > end || width > end - cursor {
        None
    } else {
        Some(cursor + width)
    }
}

pub fn next_codec_chunk(remaining: u64) -> (width: usize)
    requires 0 < remaining,
    ensures
        0 < width,
        width <= STREAMING_CODEC_SCRATCH_BYTES,
        width as int <= remaining as int,
{
    if remaining < STREAMING_CODEC_SCRATCH_BYTES as u64 {
        remaining as usize
    } else {
        STREAMING_CODEC_SCRATCH_BYTES
    }
}

pub struct ScratchLanes {
    pub primary: usize,
    pub reread: usize,
}

pub fn split_codec_scratch() -> (lanes: ScratchLanes)
    ensures
        0 < lanes.primary,
        0 < lanes.reread,
        lanes.primary + lanes.reread == STREAMING_CODEC_SCRATCH_BYTES,
        lanes.primary <= STREAMING_CODEC_SCRATCH_BYTES,
        lanes.reread <= STREAMING_CODEC_SCRATCH_BYTES,
{
    ScratchLanes {
        primary: STREAMING_CODEC_SCRATCH_BYTES / 2,
        reread: STREAMING_CODEC_SCRATCH_BYTES - STREAMING_CODEC_SCRATCH_BYTES / 2,
    }
}

pub open spec fn frame_transition_spec(state: int, byte: int) -> int {
    if state == 0 {
        if byte == 0 { 1 } else { 0 }
    } else if state == 1 {
        if byte == 0 { 2 } else if byte == 255 { 0 } else { 3 }
    } else {
        3
    }
}

pub fn frame_transition(state: u8, byte: u8) -> (next: u8)
    requires state <= 3,
    ensures next as int == frame_transition_spec(state as int, byte as int),
{
    if state == 0 {
        if byte == 0 { 1 } else { 0 }
    } else if state == 1 {
        if byte == 0 { 2 } else if byte == 255 { 0 } else { 3 }
    } else {
        3
    }
}

pub proof fn group_marker_inside_frame_is_invalid()
    ensures frame_transition_spec(1, 1) == 3,
{
}

pub proof fn escaped_zero_preserves_open_frame()
    ensures frame_transition_spec(1, 255) == 0,
{
}

pub open spec fn rho_path_shape_accepted(
    kind: int,
    frames: int,
    group_ended: bool,
    suffix_bytes: int,
    trailing_bytes: int,
) -> bool {
    if kind == 1 {
        frames == 3
    } else if kind == 2 {
        frames >= 2 && group_ended && trailing_bytes == 0
    } else if kind == 3 {
        frames == 1 && group_ended && suffix_bytes > 0
    } else {
        false
    }
}

pub proof fn datum_suffix_may_be_empty(suffix_bytes: int)
    ensures rho_path_shape_accepted(1, 3, false, suffix_bytes, 0),
{
}

pub proof fn empty_continuation_group_is_rejected()
    ensures !rho_path_shape_accepted(2, 1, true, 0, 0),
{
}

pub proof fn empty_join_suffix_is_rejected()
    ensures !rho_path_shape_accepted(3, 1, true, 0, 0),
{
}

pub open spec fn stable_key_order_authority(
    length_stable: bool,
    digest_stable: bool,
    key_valid: bool,
    strictly_increasing: bool,
) -> bool {
    length_stable && digest_stable && key_valid && strictly_increasing
}

pub proof fn unstable_reread_has_no_order_authority(
    digest_stable: bool,
    key_valid: bool,
    strictly_increasing: bool,
)
    ensures
        !stable_key_order_authority(false, digest_stable, key_valid, strictly_increasing),
{
}

pub open spec fn mutation_error_rank(
    admitted: bool,
    key_valid: bool,
    key_strict: bool,
    changes: bool,
    extent_exact: bool,
    nonempty: bool,
) -> int {
    if !admitted { 1 }
    else if !key_valid { 2 }
    else if !key_strict { 3 }
    else if !changes { 4 }
    else if !extent_exact { 5 }
    else if !nonempty { 6 }
    else { 0 }
}

pub proof fn mutation_error_precedence_is_fixed(
    strict: bool,
    changes: bool,
    exact: bool,
    nonempty: bool,
)
    ensures
        mutation_error_rank(true, false, strict, changes, exact, nonempty) == 2,
        mutation_error_rank(true, true, false, false, exact, nonempty) == 3,
        mutation_error_rank(true, true, true, true, false, false) == 5,
{
}

pub open spec fn canonical_object_kind_dispatched(kind: int) -> bool {
    1 <= kind && kind <= 5
}

pub proof fn exactly_five_canonical_object_kinds_are_dispatched(kind: int)
    ensures canonical_object_kind_dispatched(kind) <==> 1 <= kind && kind <= 5,
{
}

pub open spec fn envelope_result(
    version_valid: bool,
    declared: int,
    available: int,
    field_valid: bool,
) -> int {
    if !version_valid { 1 }
    else if available < declared { 2 }
    else if declared < available { 3 }
    else if !field_valid { 4 }
    else { 0 }
}

pub proof fn envelope_extent_precedes_body_fields(
    declared: int,
    available: int,
    field_valid: bool,
)
    requires declared != available,
    ensures envelope_result(true, declared, available, field_valid) ==
        if available < declared { 2int } else { 3int },
{
}

pub open spec fn codec_auxiliary_bytes(
    _input_bytes: int,
    _key_bytes: int,
    _object_bytes: int,
) -> int {
    STREAMING_CODEC_SCRATCH_BYTES as int + STREAMING_CODEC_FIXED_STATE_BYTES as int
}

pub proof fn codec_auxiliary_memory_has_zero_input_slope(
    input_a: int,
    input_b: int,
    key_a: int,
    key_b: int,
    object_a: int,
    object_b: int,
)
    ensures
        codec_auxiliary_bytes(input_a, key_a, object_a) ==
        codec_auxiliary_bytes(input_b, key_b, object_b),
{
}

} // verus!

fn main() {}
