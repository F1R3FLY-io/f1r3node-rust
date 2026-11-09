use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_admission_cursor.rs"]
mod rpb3_admission_cursor;
#[cfg(not(rpb3_integrated))]
use rpb3_admission_cursor as admission;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;

verus! {

pub enum WireVarintResult {
    Value { value: u64, next: usize },
    Truncated { next: usize },
    NonMinimal { next: usize },
    Overflow { next: usize },
}

pub open spec fn wire_varint_loop(
    bytes: Seq<u8>, start: usize, limit: usize, group: usize, accumulator: int,
) -> WireVarintResult
    decreases 10 - group,
{
    if group >= 10 {
        WireVarintResult::Overflow { next: (start as int + group as int) as usize }
    } else if start as int + group as int >= limit as int {
        WireVarintResult::Truncated { next: limit }
    } else {
        let byte = bytes[start as int + group as int];
        if group == 9 && byte > 1 {
            WireVarintResult::Overflow {
                next: (start as int + group as int + 1) as usize,
            }
        } else {
            let next_value = accumulator
                + (byte as int % 128) * admission::varint_multiplier(group as int);
            if byte < 128 {
                if group > 0 && byte == 0 {
                    WireVarintResult::NonMinimal {
                        next: (start as int + group as int + 1) as usize,
                    }
                } else if next_value < 0 || next_value > u64::MAX as int {
                    WireVarintResult::Overflow {
                        next: (start as int + group as int + 1) as usize,
                    }
                } else {
                    WireVarintResult::Value {
                        value: next_value as u64,
                        next: (start as int + group as int + 1) as usize,
                    }
                }
            } else {
                wire_varint_loop(
                    bytes, start, limit, (group as int + 1) as usize, next_value,
                )
            }
        }
    }
}

pub open spec fn wire_varint_read(
    bytes: Seq<u8>, start: usize, limit: usize,
) -> WireVarintResult {
    if start > limit || limit as int > bytes.len() {
        WireVarintResult::Truncated { next: start }
    } else {
        wire_varint_loop(bytes, start, limit, 0, 0)
    }
}

pub open spec fn wire_source_or_loop(
    bytes: Seq<u8>, start: usize, limit: usize, group: usize, accumulator: u64,
) -> WireVarintResult
    decreases 10 - group,
{
    if group >= 10 {
        WireVarintResult::Overflow { next: (start as int + group as int) as usize }
    } else if start as int + group as int >= limit as int {
        WireVarintResult::Truncated { next: limit }
    } else {
        let byte = bytes[start as int + group as int];
        if group == 9 && byte > 1 {
            WireVarintResult::Overflow {
                next: (start as int + group as int + 1) as usize,
            }
        } else {
            let shifted = ((byte & 0x7fu8) as u64) << ((group as int * 7) as u32);
            let next_value = accumulator | shifted;
            if byte & 0x80u8 == 0 {
                if group > 0 && byte == 0 {
                    WireVarintResult::NonMinimal {
                        next: (start as int + group as int + 1) as usize,
                    }
                } else {
                    WireVarintResult::Value {
                        value: next_value,
                        next: (start as int + group as int + 1) as usize,
                    }
                }
            } else {
                wire_source_or_loop(
                    bytes, start, limit, (group as int + 1) as usize, next_value,
                )
            }
        }
    }
}

pub open spec fn wire_group_factor(group: usize) -> int {
    if group == 0 { 1 }
    else if group == 1 { 128 }
    else if group == 2 { 16_384 }
    else if group == 3 { 2_097_152 }
    else if group == 4 { 268_435_456 }
    else if group == 5 { 34_359_738_368 }
    else if group == 6 { 4_398_046_511_104 }
    else if group == 7 { 562_949_953_421_312 }
    else if group == 8 { 72_057_594_037_927_936 }
    else if group == 9 { 9_223_372_036_854_775_808 }
    else { 0 }
}

pub proof fn wire_shift_factor_matches_table(group: usize)
    requires group < 10,
    ensures (1u64 << ((group as int * 7) as u32)) as int
        == wire_group_factor(group),
{
    if group == 0 {
        assert((1u64 << 0u32) as int == 1) by (bit_vector);
    } else if group == 1 {
        assert((1u64 << 7u32) as int == 128) by (bit_vector);
    } else if group == 2 {
        assert((1u64 << 14u32) as int == 16_384) by (bit_vector);
    } else if group == 3 {
        assert((1u64 << 21u32) as int == 2_097_152) by (bit_vector);
    } else if group == 4 {
        assert((1u64 << 28u32) as int == 268_435_456) by (bit_vector);
    } else if group == 5 {
        assert((1u64 << 35u32) as int == 34_359_738_368) by (bit_vector);
    } else if group == 6 {
        assert((1u64 << 42u32) as int == 4_398_046_511_104) by (bit_vector);
    } else if group == 7 {
        assert((1u64 << 49u32) as int == 562_949_953_421_312) by (bit_vector);
    } else if group == 8 {
        assert((1u64 << 56u32) as int == 72_057_594_037_927_936) by (bit_vector);
    } else {
        assert(group == 9);
        assert((1u64 << 63u32) as int == 9_223_372_036_854_775_808)
            by (bit_vector);
    }
}

pub proof fn wire_multiplier_matches_shift(group: usize)
    requires group < 10,
    ensures admission::varint_multiplier(group as int)
        == (1u64 << ((group as int * 7) as u32)) as int,
{
    assert(admission::varint_multiplier(0) == 1);
    assert(admission::varint_multiplier(1) == 128);
    assert(admission::varint_multiplier(2) == 16_384);
    assert(admission::varint_multiplier(3) == 2_097_152);
    assert(admission::varint_multiplier(4) == 268_435_456);
    assert(admission::varint_multiplier(5) == 34_359_738_368);
    assert(admission::varint_multiplier(6) == 4_398_046_511_104);
    assert(admission::varint_multiplier(7) == 562_949_953_421_312);
    assert(admission::varint_multiplier(8) == 72_057_594_037_927_936);
    assert(admission::varint_multiplier(9) == 9_223_372_036_854_775_808);
    wire_shift_factor_matches_table(group);
    if group == 0 {} else if group == 1 {} else if group == 2 {}
    else if group == 3 {} else if group == 4 {} else if group == 5 {}
    else if group == 6 {} else if group == 7 {} else if group == 8 {}
    else { assert(group == 9); }
}

pub proof fn wire_payload_matches_mod(byte: u8)
    ensures (byte & 0x7fu8) as int == byte as int % 128,
{
    assert((byte & 0x7fu8) as int == byte as int % 128) by (bit_vector);
}

pub proof fn wire_continuation_matches(byte: u8)
    ensures ((byte & 0x80u8) == 0u8) == (byte < 128u8),
{
    assert(((byte & 0x80u8) == 0u8) == (byte < 128u8)) by (bit_vector);
}

pub proof fn wire_or_shift_is_sum(accumulator: u64, payload: u64, group: usize)
    requires
        group < 10,
        accumulator < (1u64 << ((group as int * 7) as u32)),
        payload < 128,
        group == 9 ==> payload <= 1,
    ensures
        (accumulator | (payload << ((group as int * 7) as u32))) as int
            == accumulator as int
                + payload as int * ((1u64 << ((group as int * 7) as u32)) as int),
{
    let shift = (group as int * 7) as u32;
    wire_shift_factor_matches_table(group);
    if group == 0 {
        assert(shift == 0);
        assert(wire_group_factor(group) == 1);
        assert((1u64 << shift) as int == 1);
        assert(accumulator < 1u64);
        assert((accumulator | (payload << 0u32)) as int
            == accumulator as int + payload as int * 1)
            by (bit_vector)
            requires accumulator < 1u64, payload < 128;
    }
    else if group == 1 {
        assert(shift == 7);
        assert(wire_group_factor(group) == 128);
        assert((1u64 << shift) as int == 128);
        assert(accumulator < 128u64);
        assert((accumulator | (payload << 7u32)) as int
            == accumulator as int + payload as int * 128)
            by (bit_vector)
            requires accumulator < 128u64, payload < 128;
    }
    else if group == 2 {
        assert(shift == 14);
        assert(wire_group_factor(group) == 16384);
        assert((1u64 << shift) as int == 16384);
        assert(accumulator < 16384u64);
        assert((accumulator | (payload << 14u32)) as int
            == accumulator as int + payload as int * 16384)
            by (bit_vector)
            requires accumulator < 16384u64, payload < 128;
    }
    else if group == 3 {
        assert(shift == 21);
        assert(wire_group_factor(group) == 2097152);
        assert((1u64 << shift) as int == 2097152);
        assert(accumulator < 2097152u64);
        assert((accumulator | (payload << 21u32)) as int
            == accumulator as int + payload as int * 2097152)
            by (bit_vector)
            requires accumulator < 2097152u64, payload < 128;
    }
    else if group == 4 {
        assert(shift == 28);
        assert(wire_group_factor(group) == 268435456);
        assert((1u64 << shift) as int == 268435456);
        assert(accumulator < 268435456u64);
        assert((accumulator | (payload << 28u32)) as int
            == accumulator as int + payload as int * 268435456)
            by (bit_vector)
            requires accumulator < 268435456u64, payload < 128;
    }
    else if group == 5 {
        assert(shift == 35);
        assert(wire_group_factor(group) == 34359738368);
        assert((1u64 << shift) as int == 34359738368);
        assert(accumulator < 34359738368u64);
        assert((accumulator | (payload << 35u32)) as int
            == accumulator as int + payload as int * 34359738368)
            by (bit_vector)
            requires accumulator < 34359738368u64, payload < 128;
    }
    else if group == 6 {
        assert(shift == 42);
        assert(wire_group_factor(group) == 4398046511104);
        assert((1u64 << shift) as int == 4398046511104);
        assert(accumulator < 4398046511104u64);
        assert((accumulator | (payload << 42u32)) as int
            == accumulator as int + payload as int * 4398046511104)
            by (bit_vector)
            requires accumulator < 4398046511104u64, payload < 128;
    }
    else if group == 7 {
        assert(shift == 49);
        assert(wire_group_factor(group) == 562949953421312);
        assert((1u64 << shift) as int == 562949953421312);
        assert(accumulator < 562949953421312u64);
        assert((accumulator | (payload << 49u32)) as int
            == accumulator as int + payload as int * 562949953421312)
            by (bit_vector)
            requires accumulator < 562949953421312u64, payload < 128;
    }
    else if group == 8 {
        assert(shift == 56);
        assert(wire_group_factor(group) == 72057594037927936);
        assert((1u64 << shift) as int == 72057594037927936);
        assert(accumulator < 72057594037927936u64);
        assert((accumulator | (payload << 56u32)) as int
            == accumulator as int + payload as int * 72057594037927936)
            by (bit_vector)
            requires accumulator < 72057594037927936u64, payload < 128;
    }
    else if group == 9 {
        assert(shift == 63);
        assert(wire_group_factor(group) == 9223372036854775808);
        assert((1u64 << shift) as int == 9223372036854775808);
        assert(accumulator < 9223372036854775808u64);
        assert((accumulator | (payload << 63u32)) as int
            == accumulator as int + payload as int * 9223372036854775808)
            by (bit_vector)
            requires accumulator < 9223372036854775808u64, payload <= 1;
    } else {
        assert(false);
    }
}

pub proof fn wire_or_group_matches_sum(
    accumulator: u64, byte: u8, group: usize,
)
    requires
        group < 10,
        (accumulator as int) < admission::varint_multiplier(group as int),
        group == 9 ==> byte <= 1,
    ensures
        (accumulator | (((byte & 0x7fu8) as u64)
            << ((group as int * 7) as u32))) as int
            == accumulator as int
                + (byte as int % 128) * admission::varint_multiplier(group as int),
{
    wire_multiplier_matches_shift(group);
    let shift = (group as int * 7) as u32;
    let payload = (byte & 0x7fu8) as u64;
    let factor = 1u64 << shift;
    wire_payload_matches_mod(byte);
    assert(factor as int == admission::varint_multiplier(group as int));
    assert(payload < 128);
    if group == 9 {
        assert(shift == 63);
        assert(payload <= 1);
    }
    assert(accumulator < factor);
    wire_or_shift_is_sum(accumulator, payload, group);
}

pub proof fn wire_accumulation_bound(previous: int, digit: int, multiplier: int)
    requires
        0 <= previous < multiplier,
        0 <= digit < 128,
        multiplier > 0,
    ensures previous + digit * multiplier < 128 * multiplier,
{
    assert(previous + digit * multiplier < 128 * multiplier)
        by (nonlinear_arith)
        requires
            0 <= previous < multiplier,
            0 <= digit < 128,
            multiplier > 0;
}

pub proof fn wire_source_or_loop_matches(
    bytes: Seq<u8>, start: usize, limit: usize, group: usize,
    source_accumulator: u64, mathematical_accumulator: int,
)
    requires
        start <= limit,
        limit as int <= bytes.len(),
        group < 10,
        start as int + group as int <= limit as int,
        source_accumulator as int == mathematical_accumulator,
        mathematical_accumulator < admission::varint_multiplier(group as int),
    ensures wire_source_or_loop(
        bytes, start, limit, group, source_accumulator,
    ) == wire_varint_loop(
        bytes, start, limit, group, mathematical_accumulator,
    ),
    decreases 10 - group,
{
    if (start as int + group as int) < (limit as int) {
        let byte = bytes[start as int + group as int];
        if !(group == 9 && byte > 1) {
            wire_or_group_matches_sum(source_accumulator, byte, group);
            wire_continuation_matches(byte);
            let shifted = ((byte & 0x7fu8) as u64) << ((group as int * 7) as u32);
            let source_next = source_accumulator | shifted;
            let math_next = mathematical_accumulator
                + (byte as int % 128) * admission::varint_multiplier(group as int);
            assert(source_next as int == math_next);
            if byte < 128 {
                assert(0 <= math_next <= u64::MAX as int);
            } else {
                assert(group < 9);
                let multiplier = admission::varint_multiplier(group as int);
                assert(multiplier > 0);
                assert(admission::varint_multiplier((group + 1) as int)
                    == 128 * multiplier);
                assert(0 <= byte as int % 128 <= 127);
                assert(0 <= mathematical_accumulator < multiplier);
                wire_accumulation_bound(
                    mathematical_accumulator, byte as int % 128, multiplier,
                );
                assert(math_next < 128 * multiplier);
                wire_source_or_loop_matches(
                    bytes, start, limit, (group as int + 1) as usize,
                    source_next, math_next,
                );
            }
        }
    }
}

pub proof fn wire_source_or_read_matches(
    bytes: Seq<u8>, start: usize, limit: usize,
)
    requires start <= limit, limit as int <= bytes.len(),
    ensures wire_source_or_loop(bytes, start, limit, 0, 0)
        == wire_varint_read(bytes, start, limit),
{
    wire_source_or_loop_matches(bytes, start, limit, 0, 0, 0);
}

pub proof fn wire_tenth_group_overflow(
    bytes: Seq<u8>, start: usize, limit: usize,
    source_accumulator: u64, mathematical_accumulator: int,
)
    requires
        start <= limit,
        limit as int <= bytes.len(),
        start as int + 10 <= limit as int,
        bytes[start as int + 9] > 1,
        source_accumulator as int == mathematical_accumulator,
    ensures
        wire_source_or_loop(bytes, start, limit, 9, source_accumulator)
            == (WireVarintResult::Overflow {
                next: (start as int + 10) as usize,
            }),
        wire_varint_loop(bytes, start, limit, 9, mathematical_accumulator)
            == (WireVarintResult::Overflow {
                next: (start as int + 10) as usize,
            }),
{}

pub proof fn wire_prefix_same_value(
    left: Seq<u8>, right: Seq<u8>, groups: int,
)
    requires
        0 <= groups <= left.len(),
        groups <= right.len(),
        left.take(groups) == right.take(groups),
    ensures
        admission::decoded_varint_prefix(left, groups)
            == admission::decoded_varint_prefix(right, groups),
    decreases groups,
{
    if groups > 0 {
        assert(left.take(groups - 1) =~= right.take(groups - 1)) by {
            assert forall|i: int| #![trigger left.take(groups - 1)[i]]
                0 <= i < groups - 1
                implies left.take(groups - 1)[i] == right.take(groups - 1)[i] by {
                assert(left.take(groups - 1)[i] == left.take(groups)[i]);
                assert(right.take(groups - 1)[i] == right.take(groups)[i]);
            }
        }
        wire_prefix_same_value(left, right, groups - 1);
        assert(left.take(groups)[groups - 1] == left[groups - 1]);
        assert(right.take(groups)[groups - 1] == right[groups - 1]);
        assert(left[groups - 1] == right[groups - 1]);
    }
}

pub proof fn wire_subrange_prefix(
    bytes: Seq<u8>, start: int, middle: int, end: int,
)
    requires 0 <= start <= middle <= end <= bytes.len(),
    ensures bytes.subrange(start, end).take(middle - start)
        == bytes.subrange(start, middle),
{
    assert(bytes.subrange(start, end).take(middle - start)
        =~= bytes.subrange(start, middle)) by {
        assert forall|i: int|
            #![trigger bytes.subrange(start, end).take(middle - start)[i]]
            0 <= i < middle - start
            implies bytes.subrange(start, end).take(middle - start)[i]
                == bytes.subrange(start, middle)[i] by {
            assert(bytes.subrange(start, end)[i] == bytes[start + i]);
            assert(bytes.subrange(start, middle)[i] == bytes[start + i]);
        }
    }
}

pub proof fn wire_varint_loop_refines(
    bytes: Seq<u8>, start: usize, limit: usize, group: usize, accumulator: int,
)
    requires
        start <= limit,
        limit as int <= bytes.len(),
        group <= 10,
        start as int + group as int <= limit as int,
        accumulator == admission::decoded_varint_prefix(
            bytes.subrange(start as int, start as int + group as int),
            group as int,
        ),
        forall|i: int| #![trigger bytes[start as int + i]] 0 <= i < group as int
            ==> bytes[start as int + i] >= 128u8,
    ensures match wire_varint_loop(bytes, start, limit, group, accumulator) {
        WireVarintResult::Value { value, next } => {
            &&& start < next <= limit
            &&& next as int - start as int <= 10
            &&& admission::canonical_varint_bytes(
                bytes.subrange(start as int, next as int), value,
            )
        },
        _ => true,
    },
    decreases 10 - group,
{
    if group < 10 && (start as int + group as int) < (limit as int) {
        let byte = bytes[start as int + group as int];
        if !(group == 9 && byte > 1) {
            let next_value = accumulator
                + (byte as int % 128) * admission::varint_multiplier(group as int);
            let old_prefix = bytes.subrange(
                start as int, start as int + group as int,
            );
            let new_prefix = bytes.subrange(
                start as int, start as int + group as int + 1,
            );
            wire_subrange_prefix(
                bytes, start as int, start as int + group as int,
                start as int + group as int + 1,
            );
            assert(old_prefix.len() == group as int);
            assert(old_prefix.take(group as int) =~= old_prefix);
            assert(new_prefix.take(group as int) == old_prefix.take(group as int));
            wire_prefix_same_value(new_prefix, old_prefix, group as int);
            assert(admission::decoded_varint_prefix(
                new_prefix, group as int + 1,
            ) == next_value);
            if byte < 128 {
                if !(group > 0 && byte == 0)
                    && 0 <= next_value <= u64::MAX as int
                {
                    let next = (start as int + group as int + 1) as usize;
                    assert(new_prefix == bytes.subrange(start as int, next as int));
                    assert forall|i: int| #![trigger new_prefix[i]]
                        0 <= i < new_prefix.len() - 1
                        implies new_prefix[i] >= 128u8 by {
                        assert(new_prefix[i] == bytes[start as int + i]);
                    }
                }
            } else if group + 1 < 10 {
                assert forall|i: int| #![trigger bytes[start as int + i]]
                    0 <= i < (group + 1) as int
                    implies bytes[start as int + i] >= 128u8 by {
                    if i == group as int {
                        assert(bytes[start as int + i] == byte);
                    }
                }
                wire_varint_loop_refines(
                    bytes, start, limit, (group as int + 1) as usize, next_value,
                );
            }
        }
    }
}

pub proof fn wire_varint_read_refines(
    bytes: Seq<u8>, start: usize, limit: usize,
)
    requires start <= limit, limit as int <= bytes.len(),
    ensures match wire_varint_read(bytes, start, limit) {
        WireVarintResult::Value { value, next } => {
            &&& start < next <= limit
            &&& admission::canonical_varint_bytes(
                bytes.subrange(start as int, next as int), value,
            )
        },
        _ => true,
    },
{
    wire_varint_loop_refines(bytes, start, limit, 0, 0);
}

pub enum WireFieldError {
    InvalidCursor,
    Terminal,
    TruncatedVarint,
    NonMinimalVarint,
    VarintOverflow,
    InvalidNumber,
    DecreasingOrder,
    UnsupportedWire,
    LengthOverflow,
    TruncatedField,
}

pub struct WireLocatedError {
    pub kind: WireFieldError,
    pub reported: usize,
    pub failure: usize,
}

pub enum WireFieldResult {
    End,
    Failure {
        error: WireLocatedError,
        next: usize,
        last_field: u32,
        terminal_error: bool,
    },
    Field {
        field: admission::IndexedCanonicalFieldModel,
        next: usize,
        last_field: u32,
    },
}

pub open spec fn wire_field_failure(
    kind: WireFieldError, reported: usize, failure: usize,
    next: usize, last_field: u32,
    terminal_error: bool,
) -> WireFieldResult {
    WireFieldResult::Failure {
        error: WireLocatedError { kind, reported, failure },
        next, last_field, terminal_error,
    }
}

pub open spec fn wire_varint_failure(value: WireVarintResult) -> WireFieldError {
    match value {
        WireVarintResult::Truncated { .. } => WireFieldError::TruncatedVarint,
        WireVarintResult::NonMinimal { .. } => WireFieldError::NonMinimalVarint,
        _ => WireFieldError::VarintOverflow,
    }
}

pub open spec fn wire_varint_next(value: WireVarintResult) -> usize {
    match value {
        WireVarintResult::Value { next, .. }
        | WireVarintResult::Truncated { next }
        | WireVarintResult::NonMinimal { next }
        | WireVarintResult::Overflow { next } => next,
    }
}

pub open spec fn wire_varint_error_failure(
    value: WireVarintResult, limit: usize,
) -> usize {
    match value {
        WireVarintResult::Truncated { .. } => limit,
        WireVarintResult::NonMinimal { next }
        | WireVarintResult::Overflow { next } => (next as int - 1) as usize,
        WireVarintResult::Value { next, .. } => (next as int - 1) as usize,
    }
}

pub open spec fn wire_cursor_shape(
    cursor: admission::IndexedCanonicalFieldCursorModel,
) -> bool {
    cursor.region_start <= cursor.next <= cursor.region_end
        && cursor.source_bytes.len() == cursor.region_end as int
}

pub open spec fn wire_source_varint_read(
    bytes: Seq<u8>, start: usize, limit: usize,
) -> WireVarintResult {
    if start > limit || limit as int > bytes.len() {
        WireVarintResult::Truncated { next: start }
    } else {
        wire_source_or_loop(bytes, start, limit, 0, 0)
    }
}

pub open spec fn wire_select_varint_read(
    bytes: Seq<u8>, start: usize, limit: usize, source_or: bool,
) -> WireVarintResult {
    if source_or {
        wire_source_varint_read(bytes, start, limit)
    } else {
        wire_varint_read(bytes, start, limit)
    }
}

pub open spec fn wire_field_step_with_reader(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    last_field: u32,
    enforce_numeric_order: bool,
    terminal_error: bool,
    source_or: bool,
) -> WireFieldResult {
    if terminal_error {
        wire_field_failure(
            WireFieldError::Terminal, cursor.next, cursor.next,
            cursor.next, last_field, true,
        )
    } else if !wire_cursor_shape(cursor) {
        wire_field_failure(
            WireFieldError::InvalidCursor, cursor.next, cursor.next,
            cursor.next, last_field, true,
        )
    } else if cursor.next == cursor.region_end {
        WireFieldResult::End
    } else {
        let bytes = cursor.source_bytes;
        let start = cursor.next;
        let limit = cursor.region_end;
        match wire_select_varint_read(bytes, start, limit, source_or) {
            WireVarintResult::Value { value: key, next: key_end } => {
                let number = key / 8;
                let wire = key % 8;
                if number == 0 || number > 0x1fff_ffffu64 {
                    wire_field_failure(
                        WireFieldError::InvalidNumber, start, start,
                        key_end, last_field, true,
                    )
                } else if enforce_numeric_order && number < last_field as u64 {
                    wire_field_failure(
                        WireFieldError::DecreasingOrder, start, start,
                        key_end, last_field, true,
                    )
                } else if wire != 0 && wire != 1 && wire != 2 && wire != 5 {
                    wire_field_failure(
                        WireFieldError::UnsupportedWire, start, start,
                        key_end, number as u32, true,
                    )
                } else if wire == 0 {
                    match wire_select_varint_read(bytes, key_end, limit, source_or) {
                        WireVarintResult::Value { value, next: payload_end } => {
                            let field = admission::IndexedCanonicalFieldModel {
                                number: number as u32,
                                encoded_start: start,
                                encoded_end: payload_end,
                                payload_start: key_end,
                                payload_end,
                                key_width: (key_end as int - start as int) as usize,
                                payload_width: (payload_end as int - key_end as int) as usize,
                                payload_length: 0,
                                wire: 0,
                                decoded_varint: value,
                                payload_bytes: bytes.subrange(
                                    key_end as int, payload_end as int,
                                ),
                            };
                            WireFieldResult::Field {
                                field, next: payload_end, last_field: number as u32,
                            }
                        },
                        failed => wire_field_failure(
                            wire_varint_failure(failed), key_end,
                            wire_varint_error_failure(failed, limit),
                            wire_varint_next(failed), number as u32, true,
                        ),
                    }
                } else if wire == 2 {
                    match wire_select_varint_read(bytes, key_end, limit, source_or) {
                        WireVarintResult::Value { value: length, next: body_start } => {
                            if length > usize::MAX as u64
                                || body_start as int + length as int > usize::MAX as int
                            {
                                wire_field_failure(
                                    WireFieldError::LengthOverflow, key_end,
                                    (body_start as int - 1) as usize,
                                    body_start, number as u32, true,
                                )
                            } else if body_start as int + length as int > limit as int {
                                wire_field_failure(
                                    WireFieldError::TruncatedField, start, limit,
                                    body_start, number as u32, true,
                                )
                            } else {
                                let body_end = (body_start as int + length as int) as usize;
                                let field = admission::IndexedCanonicalFieldModel {
                                    number: number as u32,
                                    encoded_start: start,
                                    encoded_end: body_end,
                                    payload_start: body_start,
                                    payload_end: body_end,
                                    key_width: (key_end as int - start as int) as usize,
                                    payload_width: (body_start as int - key_end as int) as usize,
                                    payload_length: length,
                                    wire: 2,
                                    decoded_varint: 0,
                                    payload_bytes: bytes.subrange(
                                        body_start as int, body_end as int,
                                    ),
                                };
                                WireFieldResult::Field {
                                    field, next: body_end, last_field: number as u32,
                                }
                            }
                        },
                        failed => wire_field_failure(
                            wire_varint_failure(failed), key_end,
                            wire_varint_error_failure(failed, limit),
                            wire_varint_next(failed), number as u32, true,
                        ),
                    }
                } else {
                    let width = if wire == 1 { 8usize } else { 4usize };
                    if key_end as int + width as int > usize::MAX as int
                        || key_end as int + width as int > limit as int
                    {
                        wire_field_failure(
                            WireFieldError::TruncatedField, start, limit,
                            key_end, number as u32, true,
                        )
                    } else {
                        let payload_end = (key_end as int + width as int) as usize;
                        let field = admission::IndexedCanonicalFieldModel {
                            number: number as u32,
                            encoded_start: start,
                            encoded_end: payload_end,
                            payload_start: key_end,
                            payload_end,
                            key_width: (key_end as int - start as int) as usize,
                            payload_width: width,
                            payload_length: 0,
                            wire: wire as u8,
                            decoded_varint: 0,
                            payload_bytes: bytes.subrange(
                                key_end as int, payload_end as int,
                            ),
                        };
                        WireFieldResult::Field {
                            field, next: payload_end, last_field: number as u32,
                        }
                    }
                }
            },
            failed => wire_field_failure(
                wire_varint_failure(failed), start,
                wire_varint_error_failure(failed, limit),
                wire_varint_next(failed), last_field, true,
            ),
        }
    }
}

pub open spec fn wire_field_step(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    last_field: u32,
    enforce_numeric_order: bool,
    terminal_error: bool,
) -> WireFieldResult {
    wire_field_step_with_reader(
        cursor, last_field, enforce_numeric_order, terminal_error, false,
    )
}

pub open spec fn wire_source_field_step(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    last_field: u32,
    enforce_numeric_order: bool,
    terminal_error: bool,
) -> WireFieldResult {
    wire_field_step_with_reader(
        cursor, last_field, enforce_numeric_order, terminal_error, true,
    )
}

pub proof fn wire_source_field_step_matches_model(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    last_field: u32,
    enforce_numeric_order: bool,
    terminal_error: bool,
)
    requires wire_cursor_shape(cursor),
    ensures wire_source_field_step(
        cursor, last_field, enforce_numeric_order, terminal_error,
    ) == wire_field_step(
        cursor, last_field, enforce_numeric_order, terminal_error,
    ),
{
    if !terminal_error && cursor.next < cursor.region_end {
        let bytes = cursor.source_bytes;
        let start = cursor.next;
        let limit = cursor.region_end;
        wire_source_or_read_matches(bytes, start, limit);
        wire_varint_read_refines(bytes, start, limit);
        if let WireVarintResult::Value { value: _, next: key_end } =
            wire_varint_read(bytes, start, limit)
        {
            wire_source_or_read_matches(bytes, key_end, limit);
        }
    }
}

pub proof fn wire_field_step_refines(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    last_field: u32,
    enforce_numeric_order: bool,
    terminal_error: bool,
)
    requires wire_cursor_shape(cursor),
    ensures match wire_field_step(
        cursor, last_field, enforce_numeric_order, terminal_error,
    ) {
        WireFieldResult::End => cursor.next == cursor.region_end && !terminal_error,
        WireFieldResult::Failure { terminal_error: cached, .. } => cached,
        WireFieldResult::Field { field, next, last_field: next_last } => {
            &&& !terminal_error
            &&& next == field.encoded_end
            &&& next_last == field.number
            &&& (!enforce_numeric_order || last_field <= field.number)
            &&& admission::indexed_field_at_cursor(cursor, field)
        },
    },
{
    if !terminal_error && cursor.next < cursor.region_end {
        let bytes = cursor.source_bytes;
        let start = cursor.next;
        let limit = cursor.region_end;
        wire_varint_read_refines(bytes, start, limit);
        if let WireVarintResult::Value { value: key, next: key_end } =
            wire_varint_read(bytes, start, limit)
        {
            let number = key / 8;
            let wire = key % 8;
            if 0 < number <= 0x1fff_ffffu64
                && (!enforce_numeric_order || last_field as u64 <= number)
                && (wire == 0 || wire == 1 || wire == 2 || wire == 5)
            {
                assert(number * 8 + wire == key);
                if wire == 0 || wire == 2 {
                    wire_varint_read_refines(bytes, key_end, limit);
                }
            }
        }
    }
}

pub struct WireSourceState {
    pub cursor: admission::IndexedCanonicalFieldCursorModel,
    pub last_field: u32,
    pub enforce_numeric_order: bool,
    pub terminal_error: Option<WireLocatedError>,
}

pub enum WireSourceTransition {
    End { state: WireSourceState },
    Error { state: WireSourceState, error: WireLocatedError },
    Field {
        state: WireSourceState,
        field: admission::IndexedCanonicalFieldModel,
    },
}

pub open spec fn wire_source_with_cursor(
    old: WireSourceState, next: usize, last_field: u32,
    terminal_error: Option<WireLocatedError>,
) -> WireSourceState {
    WireSourceState {
        cursor: admission::IndexedCanonicalFieldCursorModel {
            region_start: old.cursor.region_start,
            region_end: old.cursor.region_end,
            next,
            source_bytes: old.cursor.source_bytes,
        },
        last_field,
        enforce_numeric_order: old.enforce_numeric_order,
        terminal_error,
    }
}

pub open spec fn wire_source_next_field(
    state: WireSourceState,
) -> WireSourceTransition {
    match state.terminal_error {
        Some(error) => WireSourceTransition::Error { state, error },
        None => match wire_field_step(
            state.cursor, state.last_field, state.enforce_numeric_order, false,
        ) {
            WireFieldResult::End => WireSourceTransition::End { state },
            WireFieldResult::Failure {
                error, next, last_field, terminal_error: _,
            } => WireSourceTransition::Error {
                state: wire_source_with_cursor(
                    state, next, last_field, Some(error),
                ),
                error,
            },
            WireFieldResult::Field { field, next, last_field } =>
                WireSourceTransition::Field {
                    state: wire_source_with_cursor(state, next, last_field, None),
                    field,
                },
        },
    }
}

pub open spec fn wire_source_next_field_or(
    state: WireSourceState,
) -> WireSourceTransition {
    match state.terminal_error {
        Some(error) => WireSourceTransition::Error { state, error },
        None => match wire_source_field_step(
            state.cursor, state.last_field, state.enforce_numeric_order, false,
        ) {
            WireFieldResult::End => WireSourceTransition::End { state },
            WireFieldResult::Failure {
                error, next, last_field, terminal_error: _,
            } => WireSourceTransition::Error {
                state: wire_source_with_cursor(
                    state, next, last_field, Some(error),
                ),
                error,
            },
            WireFieldResult::Field { field, next, last_field } =>
                WireSourceTransition::Field {
                    state: wire_source_with_cursor(state, next, last_field, None),
                    field,
                },
        },
    }
}

pub proof fn wire_source_next_field_or_matches_model(state: WireSourceState)
    requires wire_cursor_shape(state.cursor),
    ensures wire_source_next_field_or(state) == wire_source_next_field(state),
{
    if state.terminal_error.is_none() {
        wire_source_field_step_matches_model(
            state.cursor, state.last_field, state.enforce_numeric_order, false,
        );
    }
}

pub proof fn wire_source_next_field_refines(state: WireSourceState)
    requires wire_cursor_shape(state.cursor),
    ensures match wire_source_next_field(state) {
        WireSourceTransition::End { state: after } => {
            &&& after == state
            &&& state.terminal_error.is_none()
            &&& state.cursor.next == state.cursor.region_end
        },
        WireSourceTransition::Error { state: after, error } => {
            &&& after.terminal_error == Some(error)
            &&& after.cursor.source_bytes == state.cursor.source_bytes
            &&& after.cursor.region_start == state.cursor.region_start
            &&& after.cursor.region_end == state.cursor.region_end
            &&& after.enforce_numeric_order == state.enforce_numeric_order
            &&& (if state.terminal_error.is_some() {
                after == state && Some(error) == state.terminal_error
            } else {
                match wire_field_step(
                    state.cursor, state.last_field,
                    state.enforce_numeric_order, false,
                ) {
                    WireFieldResult::Failure {
                        error: source_error, next, last_field, terminal_error,
                    } => {
                        &&& error == source_error
                        &&& after.cursor.next == next
                        &&& after.last_field == last_field
                        &&& terminal_error
                    },
                    _ => false,
                }
            })
        },
        WireSourceTransition::Field { state: after, field } => {
            &&& state.terminal_error.is_none()
            &&& admission::indexed_field_at_cursor(state.cursor, field)
            &&& after.cursor.next == field.encoded_end
            &&& after.last_field == field.number
            &&& after.terminal_error.is_none()
            &&& after.cursor.source_bytes == state.cursor.source_bytes
        },
    },
{
    if state.terminal_error.is_none() {
        wire_field_step_refines(
            state.cursor, state.last_field, state.enforce_numeric_order, false,
        );
    }
}

pub struct WireSourceAnchor {
    pub source: u32,
    pub offset: usize,
}

pub struct WireLocatedByte {
    pub value: u8,
    pub anchor: WireSourceAnchor,
}

pub struct WireRegionImage {
    pub ghost bytes: Seq<u8>,
    pub ghost anchors: Seq<WireSourceAnchor>,
    pub end: WireSourceAnchor,
}

pub struct WireFrozenPageImage {
    pub ghost bytes: Seq<u8>,
    pub ghost anchors: Seq<WireSourceAnchor>,
}

pub struct WireFrozenLeafImage {
    pub page: usize,
    pub page_start: usize,
    pub len: usize,
    pub logical_start: usize,
}

pub struct WireFrozenImage {
    pub ghost pages: Seq<WireFrozenPageImage>,
    pub ghost leaves: Seq<WireFrozenLeafImage>,
    pub ghost logical_pages: Seq<usize>,
    pub data_len: usize,
    pub start: usize,
    pub len: usize,
}

pub open spec fn wire_frozen_leaf_at(
    image: WireFrozenImage, absolute: usize, leaf: int,
) -> Option<int>
    decreases image.leaves.len() - leaf,
{
    if leaf < 0 || leaf >= image.leaves.len() {
        None
    } else {
        let current = image.leaves[leaf];
        if (current.logical_start as int) <= (absolute as int)
            && (absolute as int) < current.logical_start as int + current.len as int
        {
            Some(leaf)
        } else if current.logical_start as int + current.len as int <= absolute as int {
            wire_frozen_leaf_at(image, absolute, leaf + 1)
        } else {
            None
        }
    }
}

pub open spec fn wire_frozen_at(
    image: WireFrozenImage, offset: usize,
) -> Option<WireLocatedByte> {
    if offset >= image.len || image.start as int + offset as int >= image.data_len as int
        || (image.start as int + offset as int) / 32 >= image.logical_pages.len()
    {
        None
    } else {
        let absolute = (image.start as int + offset as int) as usize;
        let first = image.logical_pages[(absolute as int) / 32];
        match wire_frozen_leaf_at(image, absolute, first as int) {
            Some(index) => {
                let leaf = image.leaves[index];
                let page_offset = leaf.page_start as int
                    + absolute as int - leaf.logical_start as int;
                if leaf.page as int >= image.pages.len()
                    || page_offset < 0
                    || page_offset >= image.pages[leaf.page as int].bytes.len()
                    || page_offset >= image.pages[leaf.page as int].anchors.len()
                {
                    None
                } else {
                    Some(WireLocatedByte {
                        value: image.pages[leaf.page as int].bytes[page_offset],
                        anchor: image.pages[leaf.page as int].anchors[page_offset],
                    })
                }
            },
            None => None,
        }
    }
}

pub enum WireSourcePayloadImage {
    Borrowed {
        ghost bytes: Seq<u8>,
        start: usize,
        len: usize,
        source: u32,
        absolute_start: usize,
    },
    Frozen { value: WireFrozenImage },
    Branch2 { left: usize, right: usize },
    Branch3 { first: usize, second: usize, third: usize },
}

pub struct WireSourceNodeImage {
    pub len: usize,
    pub payload: WireSourcePayloadImage,
}

pub struct WireSourceTreeImage {
    pub ghost nodes: Seq<WireSourceNodeImage>,
    pub root: Option<usize>,
    pub start: usize,
    pub len: usize,
}

pub open spec fn wire_source_node_at(
    nodes: Seq<WireSourceNodeImage>, node_id: usize, offset: usize, fuel: usize,
) -> Option<WireLocatedByte>
    decreases fuel,
{
    if fuel == 0 || node_id as int >= nodes.len() || offset >= nodes[node_id as int].len {
        None
    } else {
        match nodes[node_id as int].payload {
            WireSourcePayloadImage::Borrowed {
                bytes, start, len, source, absolute_start,
            } => {
                if offset >= len || start as int + offset as int >= bytes.len()
                    || absolute_start as int + start as int + offset as int > usize::MAX as int
                {
                    None
                } else {
                    Some(WireLocatedByte {
                        value: bytes[start as int + offset as int],
                        anchor: WireSourceAnchor {
                            source,
                            offset: (absolute_start as int + start as int + offset as int) as usize,
                        },
                    })
                }
            },
            WireSourcePayloadImage::Frozen { value } => wire_frozen_at(value, offset),
            WireSourcePayloadImage::Branch2 { left, right } => {
                if left as int >= nodes.len() || right as int >= nodes.len()
                {
                    None
                } else if offset < nodes[left as int].len {
                    wire_source_node_at(nodes, left, offset, (fuel as int - 1) as usize)
                } else {
                    wire_source_node_at(
                        nodes, right,
                        (offset as int - nodes[left as int].len as int) as usize,
                        (fuel as int - 1) as usize,
                    )
                }
            },
            WireSourcePayloadImage::Branch3 { first, second, third } => {
                if first as int >= nodes.len() || second as int >= nodes.len()
                    || third as int >= nodes.len()
                {
                    None
                } else if offset < nodes[first as int].len {
                    wire_source_node_at(nodes, first, offset, (fuel as int - 1) as usize)
                } else if (offset as int) < nodes[first as int].len as int
                    + nodes[second as int].len as int
                {
                    wire_source_node_at(
                        nodes, second,
                        (offset as int - nodes[first as int].len as int) as usize,
                        (fuel as int - 1) as usize,
                    )
                } else {
                    wire_source_node_at(
                        nodes, third,
                        (offset as int - nodes[first as int].len as int
                            - nodes[second as int].len as int) as usize,
                        (fuel as int - 1) as usize,
                    )
                }
            },
        }
    }
}

pub open spec fn wire_source_tree_at(
    image: WireSourceTreeImage, offset: usize,
) -> Option<WireLocatedByte> {
    if offset >= image.len || image.start as int + offset as int > usize::MAX as int {
        None
    } else {
        match image.root {
            Some(root) => wire_source_node_at(
                image.nodes, root, (image.start as int + offset as int) as usize,
                image.nodes.len() as usize,
            ),
            None => None,
        }
    }
}

pub enum WireBackingImage {
    Borrowed {
        ghost bytes_root: Seq<u8>,
        source: u32,
        absolute_start: usize,
        start: usize,
        len: usize,
        end_override: Option<WireSourceAnchor>,
    },
    Frozen {
        ghost located_root: Seq<WireLocatedByte>,
        start: usize,
        len: usize,
        root_end: WireSourceAnchor,
    },
    Composed {
        ghost located_root: Seq<WireLocatedByte>,
        start: usize,
        len: usize,
        root_end: WireSourceAnchor,
    },
}

pub enum WireRuntimeBackingImage {
    Borrowed {
        ghost bytes: Seq<u8>,
        source: u32,
        absolute_start: usize,
        start: usize,
        len: usize,
        end_override: Option<WireSourceAnchor>,
    },
    Frozen {
        ghost frozen: WireFrozenImage,
        start: usize,
        len: usize,
        root_end: WireSourceAnchor,
    },
    Composed {
        ghost tree: WireSourceTreeImage,
        start: usize,
        len: usize,
        root_end: WireSourceAnchor,
    },
}

pub open spec fn wire_runtime_derive_backing(
    backing: WireRuntimeBackingImage,
) -> Option<WireBackingImage> {
    match backing {
        WireRuntimeBackingImage::Borrowed {
            bytes, source, absolute_start, start, len, end_override,
        } => Some(WireBackingImage::Borrowed {
            bytes_root: bytes, source, absolute_start, start, len, end_override,
        }),
        WireRuntimeBackingImage::Frozen { frozen, start, len, root_end } => {
            if start as int + len as int > frozen.len as int
                || !(forall|i: int| 0 <= i < frozen.len as int
                    ==> (#[trigger] wire_frozen_at(frozen, i as usize)).is_some())
            {
                None
            } else {
                Some(WireBackingImage::Frozen {
                    located_root: Seq::new(frozen.len as nat, |i: int|
                        wire_frozen_at(frozen, i as usize).unwrap()),
                    start, len, root_end,
                })
            }
        },
        WireRuntimeBackingImage::Composed { tree, start, len, root_end } => {
            if start as int + len as int > tree.len as int
                || !(forall|i: int| 0 <= i < tree.len as int
                    ==> (#[trigger] wire_source_tree_at(tree, i as usize)).is_some())
            {
                None
            } else {
                Some(WireBackingImage::Composed {
                    located_root: Seq::new(tree.len as nat, |i: int|
                        wire_source_tree_at(tree, i as usize).unwrap()),
                    start, len, root_end,
                })
            }
        },
    }
}

pub open spec fn wire_runtime_project_region(
    backing: WireRuntimeBackingImage,
) -> Option<WireRegionImage> {
    match wire_runtime_derive_backing(backing) {
        Some(derived) => wire_backing_project(derived),
        None => None,
    }
}

pub open spec fn wire_runtime_subview(
    backing: WireRuntimeBackingImage, sub_start: usize, sub_end: usize,
) -> Option<WireRuntimeBackingImage> {
    if sub_start > sub_end {
        None
    } else {
        match backing {
            WireRuntimeBackingImage::Borrowed {
                bytes, source, absolute_start, start, len, end_override,
            } => {
                if sub_end > len || start as int + sub_end as int > usize::MAX as int {
                    None
                } else {
                    Some(WireRuntimeBackingImage::Borrowed {
                        bytes, source, absolute_start,
                        start: (start as int + sub_start as int) as usize,
                        len: (sub_end as int - sub_start as int) as usize,
                        end_override: if sub_end == len { end_override } else { None },
                    })
                }
            },
            WireRuntimeBackingImage::Frozen { frozen, start, len, root_end } => {
                if sub_end > len || start as int + sub_end as int > usize::MAX as int {
                    None
                } else {
                    Some(WireRuntimeBackingImage::Frozen {
                        frozen,
                        start: (start as int + sub_start as int) as usize,
                        len: (sub_end as int - sub_start as int) as usize,
                        root_end,
                    })
                }
            },
            WireRuntimeBackingImage::Composed { tree, start, len, root_end } => {
                if sub_end > len || start as int + sub_end as int > usize::MAX as int {
                    None
                } else {
                    Some(WireRuntimeBackingImage::Composed {
                        tree,
                        start: (start as int + sub_start as int) as usize,
                        len: (sub_end as int - sub_start as int) as usize,
                        root_end,
                    })
                }
            },
        }
    }
}

pub proof fn wire_runtime_subview_exact(
    backing: WireRuntimeBackingImage, sub_start: usize, sub_end: usize,
)
    requires
        wire_runtime_project_region(backing).is_some(),
        sub_start <= sub_end,
        sub_end as int <= wire_runtime_project_region(backing).unwrap().bytes.len(),
    ensures
        wire_runtime_subview(backing, sub_start, sub_end).is_some(),
        wire_runtime_project_region(
            wire_runtime_subview(backing, sub_start, sub_end).unwrap(),
        ).is_some(),
        wire_runtime_project_region(
            wire_runtime_subview(backing, sub_start, sub_end).unwrap(),
        ).unwrap().bytes =~= wire_runtime_project_region(backing).unwrap().bytes.subrange(
            sub_start as int, sub_end as int,
        ),
        wire_runtime_project_region(
            wire_runtime_subview(backing, sub_start, sub_end).unwrap(),
        ).unwrap().anchors =~= wire_runtime_project_region(backing).unwrap().anchors.subrange(
            sub_start as int, sub_end as int,
        ),
        wire_runtime_project_region(
            wire_runtime_subview(backing, sub_start, sub_end).unwrap(),
        ).unwrap().end == wire_region_position(
            wire_runtime_project_region(backing).unwrap(), sub_end,
        ).unwrap().physical,
{}

pub open spec fn wire_runtime_varint_read(
    backing: WireRuntimeBackingImage, start: usize, limit: usize,
) -> Option<WireVarintResult> {
    match wire_runtime_project_region(backing) {
        Some(region) if start <= limit && limit as int <= region.bytes.len() => {
            Some(wire_source_or_loop(region.bytes, start, limit, 0, 0))
        },
        _ => None,
    }
}

pub proof fn wire_runtime_varint_refines(
    backing: WireRuntimeBackingImage, start: usize, limit: usize,
)
    requires
        wire_runtime_project_region(backing).is_some(),
        start <= limit,
        limit as int <= wire_runtime_project_region(backing).unwrap().bytes.len(),
    ensures wire_runtime_varint_read(backing, start, limit)
        == Some(wire_varint_read(
            wire_runtime_project_region(backing).unwrap().bytes,
            start, limit,
        )),
{
    let region = wire_runtime_project_region(backing).unwrap();
    wire_source_or_read_matches(region.bytes, start, limit);
}

pub open spec fn wire_backing_project(backing: WireBackingImage) -> Option<WireRegionImage> {
    match backing {
        WireBackingImage::Borrowed {
            bytes_root, source, absolute_start, start, len, end_override,
        } => {
            if start as int + len as int > bytes_root.len()
                || absolute_start as int + start as int + len as int > usize::MAX as int
            {
                None
            } else {
                let end = match end_override {
                    Some(anchor) => anchor,
                    None => WireSourceAnchor {
                        source,
                        offset: (absolute_start as int + start as int + len as int) as usize,
                    },
                };
                Some(WireRegionImage {
                    bytes: bytes_root.subrange(start as int, start as int + len as int),
                    anchors: Seq::new(len as nat, |i: int| WireSourceAnchor {
                        source,
                        offset: (absolute_start as int + start as int + i) as usize,
                    }),
                    end,
                })
            }
        },
        WireBackingImage::Frozen { located_root, start, len, root_end }
        | WireBackingImage::Composed { located_root, start, len, root_end } => {
            if start as int + len as int > located_root.len() {
                None
            } else {
                let end_index = start as int + len as int;
                Some(WireRegionImage {
                    bytes: Seq::new(len as nat, |i: int| located_root[start as int + i].value),
                    anchors: Seq::new(len as nat, |i: int| located_root[start as int + i].anchor),
                    end: if end_index == located_root.len() {
                        root_end
                    } else {
                        located_root[end_index].anchor
                    },
                })
            }
        },
    }
}

pub struct WirePhysicalPosition {
    pub logical_offset: usize,
    pub physical: WireSourceAnchor,
    pub end: bool,
}

pub open spec fn wire_region_position(
    region: WireRegionImage, offset: usize,
) -> Option<WirePhysicalPosition> {
    if region.bytes.len() != region.anchors.len() || offset as int > region.bytes.len() {
        None
    } else if offset as int == region.bytes.len() {
        Some(WirePhysicalPosition {
            logical_offset: offset,
            physical: region.end,
            end: true,
        })
    } else {
        Some(WirePhysicalPosition {
            logical_offset: offset,
            physical: region.anchors[offset as int],
            end: false,
        })
    }
}

pub struct WirePhysicalError {
    pub kind: WireFieldError,
    pub reported: WirePhysicalPosition,
    pub failure: WirePhysicalPosition,
}

pub struct WireConcreteCursorImage {
    pub ghost message: WireBackingImage,
    pub ghost reader_view: WireBackingImage,
    pub reader_at: usize,
    pub cursor_yielded: usize,
    pub last_field: u32,
    pub enforce_numeric_order: bool,
    pub fields_read: usize,
    pub terminal_error: Option<WirePhysicalError>,
}

pub open spec fn wire_physical_error_project(error: WirePhysicalError) -> WireLocatedError {
    WireLocatedError {
        kind: error.kind,
        reported: error.reported.logical_offset,
        failure: error.failure.logical_offset,
    }
}

pub open spec fn wire_concrete_cursor_project(
    state: WireConcreteCursorImage,
) -> Option<WireSourceState> {
    match (wire_backing_project(state.message), wire_backing_project(state.reader_view)) {
        (Some(message), Some(view)) => {
            if message.bytes != view.bytes || message.anchors != view.anchors
                || message.end != view.end
                || state.reader_at as int > message.bytes.len()
                || state.cursor_yielded != state.reader_at
            {
                None
            } else {
                Some(WireSourceState {
                    cursor: admission::IndexedCanonicalFieldCursorModel {
                        region_start: 0,
                        region_end: message.bytes.len() as usize,
                        next: state.reader_at,
                        source_bytes: message.bytes,
                    },
                    last_field: state.last_field,
                    enforce_numeric_order: state.enforce_numeric_order,
                    terminal_error: match state.terminal_error {
                        Some(error) => Some(wire_physical_error_project(error)),
                        None => None,
                    },
                })
            }
        },
        _ => None,
    }
}

pub open spec fn wire_concrete_cursor_wf(state: WireConcreteCursorImage) -> bool {
    match (wire_backing_project(state.message), wire_concrete_cursor_project(state)) {
        (Some(region), Some(_)) => {
            &&& region.bytes.len() <= usize::MAX as int
            &&& match state.terminal_error {
                Some(error) => {
                    &&& wire_region_position(region, error.reported.logical_offset)
                        == Some(error.reported)
                    &&& wire_region_position(region, error.failure.logical_offset)
                        == Some(error.failure)
                },
                None => true,
            }
        },
        _ => false,
    }
}

pub struct WireRawByteResult {
    pub offset: usize,
    pub value: u8,
    pub position: WirePhysicalPosition,
    pub next_at: usize,
    pub next_yielded: usize,
}

pub open spec fn wire_concrete_next_raw(
    state: WireConcreteCursorImage,
) -> Option<WireRawByteResult> {
    match wire_backing_project(state.reader_view) {
        Some(region) if (state.reader_at as int) < region.bytes.len()
            && state.cursor_yielded == state.reader_at => {
            let offset = state.reader_at;
            match wire_region_position(region, offset) {
                Some(position) => Some(WireRawByteResult {
                    offset,
                    value: region.bytes[offset as int],
                    position,
                    next_at: (offset as int + 1) as usize,
                    next_yielded: (offset as int + 1) as usize,
                }),
                None => None,
            }
        },
        _ => None,
    }
}

pub proof fn wire_concrete_next_raw_projects(state: WireConcreteCursorImage)
    requires wire_concrete_cursor_wf(state),
    ensures match (wire_backing_project(state.reader_view), wire_concrete_next_raw(state)) {
        (Some(region), Some(result)) => {
            &&& (state.reader_at as int) < region.bytes.len()
            &&& result.offset == state.reader_at
            &&& result.value == region.bytes[state.reader_at as int]
            &&& result.position == wire_region_position(
                region, state.reader_at,
            ).unwrap()
            &&& !result.position.end
            &&& result.next_at as int == state.reader_at as int + 1
            &&& result.next_yielded == result.next_at
        },
        (Some(region), None) => state.reader_at as int == region.bytes.len(),
        _ => false,
    },
{}

pub open spec fn wire_concrete_initial(
    backing: WireBackingImage, enforce_numeric_order: bool,
) -> Option<WireConcreteCursorImage> {
    match wire_backing_project(backing) {
        Some(region) if region.bytes.len() <= usize::MAX as int => {
            Some(WireConcreteCursorImage {
                message: backing,
                reader_view: backing,
                reader_at: 0,
                cursor_yielded: 0,
                last_field: 0,
                enforce_numeric_order,
                fields_read: 0,
                terminal_error: None,
            })
        },
        _ => None,
    }
}

pub proof fn wire_concrete_initial_refines(
    backing: WireBackingImage, enforce_numeric_order: bool,
)
    ensures match wire_concrete_initial(backing, enforce_numeric_order) {
        Some(state) => {
            &&& wire_concrete_cursor_wf(state)
            &&& state.reader_at == 0
            &&& state.cursor_yielded == 0
            &&& state.last_field == 0
            &&& state.fields_read == 0
            &&& state.terminal_error.is_none()
            &&& wire_concrete_cursor_project(state).is_some()
        },
        None => wire_backing_project(backing).is_none()
            || wire_backing_project(backing).unwrap().bytes.len() > usize::MAX as int,
    },
{}

pub enum WireConcreteTransition {
    End { state: WireConcreteCursorImage },
    Error { state: WireConcreteCursorImage, error: WirePhysicalError },
    Field {
        state: WireConcreteCursorImage,
        field: admission::IndexedCanonicalFieldModel,
        start: WirePhysicalPosition,
    },
}

pub open spec fn wire_concrete_advanced(
    old: WireConcreteCursorImage, next: usize, last_field: u32,
    fields_read: usize, terminal_error: Option<WirePhysicalError>,
) -> WireConcreteCursorImage {
    WireConcreteCursorImage {
        message: old.message,
        reader_view: old.reader_view,
        reader_at: next,
        cursor_yielded: next,
        last_field,
        enforce_numeric_order: old.enforce_numeric_order,
        fields_read,
        terminal_error,
    }
}

pub open spec fn wire_concrete_next_field(
    state: WireConcreteCursorImage,
) -> Option<WireConcreteTransition> {
    match (wire_backing_project(state.message), wire_concrete_cursor_project(state)) {
        (Some(region), Some(abstract_state)) => {
            match wire_source_next_field(abstract_state) {
                WireSourceTransition::End { .. } => {
                    Some(WireConcreteTransition::End { state })
                },
                WireSourceTransition::Error { state: after, error } => {
                    match (
                        wire_region_position(region, error.reported),
                        wire_region_position(region, error.failure),
                    ) {
                        (Some(reported), Some(failure)) => {
                            let physical = WirePhysicalError {
                                kind: error.kind, reported, failure,
                            };
                            Some(WireConcreteTransition::Error {
                                state: wire_concrete_advanced(
                                    state, after.cursor.next, after.last_field,
                                    state.fields_read, Some(physical),
                                ),
                                error: physical,
                            })
                        },
                        _ => None,
                    }
                },
                WireSourceTransition::Field { state: after, field } => {
                    match wire_region_position(region, field.encoded_start) {
                        Some(start) => Some(WireConcreteTransition::Field {
                            state: wire_concrete_advanced(
                                state, after.cursor.next, after.last_field,
                                (state.fields_read as int + 1) as usize, None,
                            ),
                            field,
                            start,
                        }),
                        None => None,
                    }
                },
            }
        },
        _ => None,
    }
}

pub open spec fn wire_concrete_next_field_or(
    state: WireConcreteCursorImage,
) -> Option<WireConcreteTransition> {
    match (wire_backing_project(state.message), wire_concrete_cursor_project(state)) {
        (Some(region), Some(abstract_state)) => {
            match wire_source_next_field_or(abstract_state) {
                WireSourceTransition::End { .. } => {
                    Some(WireConcreteTransition::End { state })
                },
                WireSourceTransition::Error { state: after, error } => {
                    match (
                        wire_region_position(region, error.reported),
                        wire_region_position(region, error.failure),
                    ) {
                        (Some(reported), Some(failure)) => {
                            let physical = WirePhysicalError {
                                kind: error.kind, reported, failure,
                            };
                            Some(WireConcreteTransition::Error {
                                state: wire_concrete_advanced(
                                    state, after.cursor.next, after.last_field,
                                    state.fields_read, Some(physical),
                                ),
                                error: physical,
                            })
                        },
                        _ => None,
                    }
                },
                WireSourceTransition::Field { state: after, field } => {
                    match wire_region_position(region, field.encoded_start) {
                        Some(start) => Some(WireConcreteTransition::Field {
                            state: wire_concrete_advanced(
                                state, after.cursor.next, after.last_field,
                                (state.fields_read as int + 1) as usize, None,
                            ),
                            field,
                            start,
                        }),
                        None => None,
                    }
                },
            }
        },
        _ => None,
    }
}

pub proof fn wire_concrete_next_field_or_matches_model(
    state: WireConcreteCursorImage,
)
    requires wire_concrete_cursor_wf(state),
    ensures wire_concrete_next_field_or(state) == wire_concrete_next_field(state),
{
    let abstract_state = wire_concrete_cursor_project(state).unwrap();
    wire_source_next_field_or_matches_model(abstract_state);
}

pub struct WireRuntimeCursorImage {
    pub ghost message: WireRuntimeBackingImage,
    pub ghost reader_view: WireRuntimeBackingImage,
    pub reader_at: usize,
    pub cursor_yielded: usize,
    pub last_field: u32,
    pub enforce_numeric_order: bool,
    pub fields_read: usize,
    pub terminal_error: Option<WirePhysicalError>,
}

pub open spec fn wire_runtime_derive_cursor(
    state: WireRuntimeCursorImage,
) -> Option<WireConcreteCursorImage> {
    match (
        wire_runtime_derive_backing(state.message),
        wire_runtime_derive_backing(state.reader_view),
    ) {
        (Some(message), Some(reader_view)) => Some(WireConcreteCursorImage {
            message,
            reader_view,
            reader_at: state.reader_at,
            cursor_yielded: state.cursor_yielded,
            last_field: state.last_field,
            enforce_numeric_order: state.enforce_numeric_order,
            fields_read: state.fields_read,
            terminal_error: state.terminal_error,
        }),
        _ => None,
    }
}

pub open spec fn wire_runtime_cursor_project(
    state: WireRuntimeCursorImage,
) -> Option<WireSourceState> {
    match wire_runtime_derive_cursor(state) {
        Some(derived) => wire_concrete_cursor_project(derived),
        None => None,
    }
}

pub open spec fn wire_runtime_initial(
    backing: WireRuntimeBackingImage, enforce_numeric_order: bool,
) -> Option<WireRuntimeCursorImage> {
    match wire_runtime_project_region(backing) {
        Some(region) if region.bytes.len() <= usize::MAX as int => {
            Some(WireRuntimeCursorImage {
                message: backing,
                reader_view: backing,
                reader_at: 0,
                cursor_yielded: 0,
                last_field: 0,
                enforce_numeric_order,
                fields_read: 0,
                terminal_error: None,
            })
        },
        _ => None,
    }
}

pub open spec fn wire_runtime_next_field(
    state: WireRuntimeCursorImage,
) -> Option<WireConcreteTransition> {
    match wire_runtime_derive_cursor(state) {
        Some(derived) => wire_concrete_next_field_or(derived),
        None => None,
    }
}

/// Source shape of the production Message cursor before `next_field`.
/// `IndexedCanonicalFieldCursor::in_prost_slot_order` disables numeric wire
/// ordering, and its reader and retained message have one indexed view.
/// The premise that a Rust indexed backing projects to `backing` and its
/// exact bytes/anchors/end is separate from this scalar cursor-state lemma.
pub open spec fn wire_runtime_message_pre_read(
    backing: WireRuntimeBackingImage,
    reader_at: usize,
    last_field: u32,
    fields_read: usize,
) -> WireRuntimeCursorImage {
    WireRuntimeCursorImage {
        message: backing,
        reader_view: backing,
        reader_at,
        cursor_yielded: reader_at,
        last_field,
        enforce_numeric_order: false,
        fields_read,
        terminal_error: None,
    }
}

/// The arguments are the fields of the verified
/// `BorrowedIndexedWindow::image()`. Its `byte_at` and `position_at`
/// postconditions supply the byte and physical-anchor interpretation for
/// every offset, including an optional distinct terminal boundary.
pub open spec fn wire_runtime_borrowed_window(
    bytes: Seq<u8>, source: u32, absolute_start: usize, start: usize, len: usize,
    end_source: Option<u32>, end_offset: usize,
) -> WireRuntimeBackingImage {
    WireRuntimeBackingImage::Borrowed {
        bytes, source, absolute_start, start, len,
        end_override: match end_source {
            Some(source) => Some(WireSourceAnchor { source, offset: end_offset }),
            None => None,
        },
    }
}

pub open spec fn wire_borrowed_window_region(
    bytes: Seq<u8>, source: u32, absolute_start: usize, start: usize, len: usize,
    end_source: Option<u32>, end_offset: usize,
) -> WireRegionImage {
    WireRegionImage {
        bytes: bytes.subrange(start as int, start as int + len as int),
        anchors: Seq::new(len as nat, |i: int| WireSourceAnchor {
            source,
            offset: (absolute_start as int + start as int + i) as usize,
        }),
        end: match end_source {
            Some(source) => WireSourceAnchor { source, offset: end_offset },
            None => WireSourceAnchor {
                source,
                offset: (absolute_start as int + start as int + len as int) as usize,
            },
        },
    }
}

/// A borrowed window's constructor fields determine the entire region image.
/// No pointwise byte/anchor agreement with a separately supplied receipt is
/// assumed. The arithmetic bound is the formal projection's domain.
pub proof fn wire_runtime_borrowed_window_projects(
    bytes: Seq<u8>, source: u32, absolute_start: usize, start: usize, len: usize,
    end_source: Option<u32>, end_offset: usize,
)
    requires
        start as int + len as int <= bytes.len(),
        absolute_start as int + start as int + len as int <= usize::MAX as int,
    ensures
        wire_runtime_project_region(wire_runtime_borrowed_window(
            bytes, source, absolute_start, start, len, end_source, end_offset,
        )) == Some(wire_borrowed_window_region(
            bytes, source, absolute_start, start, len, end_source, end_offset,
        )),
{
}

pub proof fn wire_runtime_message_pre_read_projects(
    backing: WireRuntimeBackingImage,
    observed: WireRegionImage,
    reader_at: usize,
    last_field: u32,
    fields_read: usize,
)
    requires
        // Rust's indexed body must independently project to these exact
        // bytes, physical anchors, and terminal anchor.
        wire_runtime_project_region(backing) == Some(observed),
        observed.bytes.len() <= usize::MAX as int,
        reader_at as int <= observed.bytes.len(),
    ensures {
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(state.message) == Some(observed)
        &&& wire_runtime_project_region(state.reader_view) == Some(observed)
        &&& wire_runtime_derive_cursor(state).is_some()
        &&& wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap())
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: observed.bytes.len() as usize,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    let state = wire_runtime_message_pre_read(
        backing, reader_at, last_field, fields_read,
    );
    let derived = wire_runtime_derive_cursor(state).unwrap();
    assert(wire_backing_project(derived.message) == Some(observed));
    assert(wire_backing_project(derived.reader_view) == Some(observed));
    assert(wire_concrete_cursor_project(derived)
        == Some(WireSourceState {
            cursor: admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: observed.bytes.len() as usize,
                next: reader_at,
                source_bytes: observed.bytes,
            },
            last_field,
            enforce_numeric_order: false,
            terminal_error: None,
        }));
    assert(wire_concrete_cursor_wf(derived));
}

/// For the verified borrowed backing, the pre-read cursor obtains its source
/// bytes, byte anchors, and end anchor from the constructor image itself.
/// Frozen and Composed still require production index/DAG extraction to bind
/// their ordinary Rust accessors to `wire_frozen_at`/`wire_source_tree_at`.
pub proof fn wire_runtime_borrowed_message_pre_read_projects(
    bytes: Seq<u8>, source: u32, absolute_start: usize, start: usize, len: usize,
    end_source: Option<u32>, end_offset: usize,
    reader_at: usize, last_field: u32, fields_read: usize,
)
    requires
        start as int + len as int <= bytes.len(),
        absolute_start as int + start as int + len as int <= usize::MAX as int,
        reader_at <= len,
    ensures {
        let backing = wire_runtime_borrowed_window(
            bytes, source, absolute_start, start, len, end_source, end_offset,
        );
        let observed = wire_borrowed_window_region(
            bytes, source, absolute_start, start, len, end_source, end_offset,
        );
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(state.message) == Some(observed)
        &&& wire_runtime_project_region(state.reader_view) == Some(observed)
        &&& wire_runtime_derive_cursor(state).is_some()
        &&& wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap())
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: len,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    let backing = wire_runtime_borrowed_window(
        bytes, source, absolute_start, start, len, end_source, end_offset,
    );
    let observed = wire_borrowed_window_region(
        bytes, source, absolute_start, start, len, end_source, end_offset,
    );
    wire_runtime_borrowed_window_projects(
        bytes, source, absolute_start, start, len, end_source, end_offset,
    );
    wire_runtime_message_pre_read_projects(
        backing, observed, reader_at, last_field, fields_read,
    );
}

/// The formal frozen image mirrors `PageFrozenBytes`: `wire_frozen_at`
/// selects `logical_pages[absolute / 32]`, scans its leaf records, then reads
/// a byte and provenance from the same page slot. The ordinary Rust
/// `freeze`/`leaf_at` constructor-to-image relation remains the extraction
/// obligation; no receipt-supplied byte or anchor equality is assumed here.
pub open spec fn wire_runtime_frozen_from_index(
    frozen: WireFrozenImage, start: usize, len: usize, root_end: WireSourceAnchor,
) -> WireRuntimeBackingImage {
    WireRuntimeBackingImage::Frozen { frozen, start, len, root_end }
}

pub open spec fn wire_frozen_index_region(
    frozen: WireFrozenImage, start: usize, len: usize, root_end: WireSourceAnchor,
) -> WireRegionImage {
    WireRegionImage {
        bytes: Seq::new(len as nat, |i: int|
            wire_frozen_at(frozen, (start as int + i) as usize).unwrap().value),
        anchors: Seq::new(len as nat, |i: int|
            wire_frozen_at(frozen, (start as int + i) as usize).unwrap().anchor),
        end: if start as int + len as int == frozen.len as int {
            root_end
        } else {
            wire_frozen_at(frozen, (start as int + len as int) as usize).unwrap().anchor
        },
    }
}

/// The quantified premise is precisely in-range accessor totality for the
/// ordinary-Rust frozen index. `freeze` builds its page/leaf table, but its
/// constructor invariant is not yet a co-verified Verus postcondition.
pub proof fn wire_runtime_frozen_index_projects(
    frozen: WireFrozenImage, start: usize, len: usize, root_end: WireSourceAnchor,
)
    requires
        start as int + len as int <= frozen.len as int,
        forall|i: int| 0 <= i < frozen.len as int
            ==> (#[trigger] wire_frozen_at(frozen, i as usize)).is_some(),
    ensures
        wire_runtime_project_region(wire_runtime_frozen_from_index(
            frozen, start, len, root_end,
        )) == Some(wire_frozen_index_region(frozen, start, len, root_end)),
{
    let backing = wire_runtime_frozen_from_index(frozen, start, len, root_end);
    let located_root = Seq::new(frozen.len as nat, |i: int|
        wire_frozen_at(frozen, i as usize).unwrap());
    let derived = WireBackingImage::Frozen { located_root, start, len, root_end };
    assert(wire_runtime_derive_backing(backing) == Some(derived));
    assert(wire_backing_project(derived).is_some());
    let projected = wire_backing_project(derived).unwrap();
    let expected = wire_frozen_index_region(frozen, start, len, root_end);
    assert(projected.bytes =~= expected.bytes) by {
        assert forall|i: int| #![trigger projected.bytes[i]]
            0 <= i < len as int implies projected.bytes[i] == expected.bytes[i] by {
            let absolute = start as int + i;
            assert(0 <= absolute < frozen.len as int);
            assert(wire_frozen_at(frozen, absolute as usize).is_some());
            assert(located_root[absolute] == wire_frozen_at(frozen, absolute as usize).unwrap());
        }
    }
    assert(projected.anchors =~= expected.anchors) by {
        assert forall|i: int| #![trigger projected.anchors[i]]
            0 <= i < len as int implies projected.anchors[i] == expected.anchors[i] by {
            let absolute = start as int + i;
            assert(0 <= absolute < frozen.len as int);
            assert(wire_frozen_at(frozen, absolute as usize).is_some());
            assert(located_root[absolute] == wire_frozen_at(frozen, absolute as usize).unwrap());
        }
    }
    if (start as int + len as int) < frozen.len as int {
        let end_index = start as int + len as int;
        assert(wire_frozen_at(frozen, end_index as usize).is_some());
        assert(located_root[end_index] == wire_frozen_at(frozen, end_index as usize).unwrap());
    }
    assert(projected.end == expected.end);
    assert(projected == expected);
}

/// The corresponding source-tree accessor follows each Branch2/Branch3
/// child length to a Borrowed or Frozen leaf. The Arc DAG-to-`tree` image,
/// including node lengths, child topology, and borrowed/frozen leaf images,
/// is the remaining ordinary-Rust extraction obligation.
pub open spec fn wire_runtime_composed_from_tree(
    tree: WireSourceTreeImage, start: usize, len: usize, root_end: WireSourceAnchor,
) -> WireRuntimeBackingImage {
    WireRuntimeBackingImage::Composed { tree, start, len, root_end }
}

pub open spec fn wire_composed_tree_region(
    tree: WireSourceTreeImage, start: usize, len: usize, root_end: WireSourceAnchor,
) -> WireRegionImage {
    WireRegionImage {
        bytes: Seq::new(len as nat, |i: int|
            wire_source_tree_at(tree, (start as int + i) as usize).unwrap().value),
        anchors: Seq::new(len as nat, |i: int|
            wire_source_tree_at(tree, (start as int + i) as usize).unwrap().anchor),
        end: if start as int + len as int == tree.len as int {
            root_end
        } else {
            wire_source_tree_at(tree, (start as int + len as int) as usize).unwrap().anchor
        },
    }
}

/// The quantified premise is in-range accessor totality for the ordinary-Rust
/// Arc DAG. Its Branch2/Branch3 length and leaf bounds are not yet a
/// co-verified Verus postcondition of `SourceTreeWindow::concat`/`subview`.
pub proof fn wire_runtime_composed_tree_projects(
    tree: WireSourceTreeImage, start: usize, len: usize, root_end: WireSourceAnchor,
)
    requires
        start as int + len as int <= tree.len as int,
        forall|i: int| 0 <= i < tree.len as int
            ==> (#[trigger] wire_source_tree_at(tree, i as usize)).is_some(),
    ensures
        wire_runtime_project_region(wire_runtime_composed_from_tree(
            tree, start, len, root_end,
        )) == Some(wire_composed_tree_region(tree, start, len, root_end)),
{
    let backing = wire_runtime_composed_from_tree(tree, start, len, root_end);
    let located_root = Seq::new(tree.len as nat, |i: int|
        wire_source_tree_at(tree, i as usize).unwrap());
    let derived = WireBackingImage::Composed { located_root, start, len, root_end };
    assert(wire_runtime_derive_backing(backing) == Some(derived));
    assert(wire_backing_project(derived).is_some());
    let projected = wire_backing_project(derived).unwrap();
    let expected = wire_composed_tree_region(tree, start, len, root_end);
    assert(projected.bytes =~= expected.bytes) by {
        assert forall|i: int| #![trigger projected.bytes[i]]
            0 <= i < len as int implies projected.bytes[i] == expected.bytes[i] by {
            let absolute = start as int + i;
            assert(0 <= absolute < tree.len as int);
            assert(wire_source_tree_at(tree, absolute as usize).is_some());
            assert(located_root[absolute] == wire_source_tree_at(tree, absolute as usize).unwrap());
        }
    }
    assert(projected.anchors =~= expected.anchors) by {
        assert forall|i: int| #![trigger projected.anchors[i]]
            0 <= i < len as int implies projected.anchors[i] == expected.anchors[i] by {
            let absolute = start as int + i;
            assert(0 <= absolute < tree.len as int);
            assert(wire_source_tree_at(tree, absolute as usize).is_some());
            assert(located_root[absolute] == wire_source_tree_at(tree, absolute as usize).unwrap());
        }
    }
    if (start as int + len as int) < tree.len as int {
        let end_index = start as int + len as int;
        assert(wire_source_tree_at(tree, end_index as usize).is_some());
        assert(located_root[end_index] == wire_source_tree_at(tree, end_index as usize).unwrap());
    }
    assert(projected.end == expected.end);
    assert(projected == expected);
}

pub proof fn wire_runtime_frozen_message_pre_read_projects(
    frozen: WireFrozenImage, start: usize, len: usize, root_end: WireSourceAnchor,
    reader_at: usize, last_field: u32, fields_read: usize,
)
    requires
        start as int + len as int <= frozen.len as int,
        forall|i: int| 0 <= i < frozen.len as int
            ==> (#[trigger] wire_frozen_at(frozen, i as usize)).is_some(),
        reader_at <= len,
    ensures {
        let backing = wire_runtime_frozen_from_index(frozen, start, len, root_end);
        let observed = wire_frozen_index_region(frozen, start, len, root_end);
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(state.message) == Some(observed)
        &&& wire_runtime_project_region(state.reader_view) == Some(observed)
        &&& wire_runtime_derive_cursor(state).is_some()
        &&& wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap())
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: len,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    let backing = wire_runtime_frozen_from_index(frozen, start, len, root_end);
    let observed = wire_frozen_index_region(frozen, start, len, root_end);
    wire_runtime_frozen_index_projects(frozen, start, len, root_end);
    wire_runtime_message_pre_read_projects(
        backing, observed, reader_at, last_field, fields_read,
    );
}

pub proof fn wire_runtime_composed_message_pre_read_projects(
    tree: WireSourceTreeImage, start: usize, len: usize, root_end: WireSourceAnchor,
    reader_at: usize, last_field: u32, fields_read: usize,
)
    requires
        start as int + len as int <= tree.len as int,
        forall|i: int| 0 <= i < tree.len as int
            ==> (#[trigger] wire_source_tree_at(tree, i as usize)).is_some(),
        reader_at <= len,
    ensures {
        let backing = wire_runtime_composed_from_tree(tree, start, len, root_end);
        let observed = wire_composed_tree_region(tree, start, len, root_end);
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(state.message) == Some(observed)
        &&& wire_runtime_project_region(state.reader_view) == Some(observed)
        &&& wire_runtime_derive_cursor(state).is_some()
        &&& wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap())
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: len,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    let backing = wire_runtime_composed_from_tree(tree, start, len, root_end);
    let observed = wire_composed_tree_region(tree, start, len, root_end);
    wire_runtime_composed_tree_projects(tree, start, len, root_end);
    wire_runtime_message_pre_read_projects(
        backing, observed, reader_at, last_field, fields_read,
    );
}

/// `freeze_owned_segment` exports one retained page pair and one leaf; each
/// logical block starts its leaf search at index zero. This predicate states
/// the index bounds, while the separate Rust-to-ghost representation relation
/// must still identify the actual page vectors and provenance values.
pub open spec fn wire_frozen_one_leaf_wf(image: WireFrozenImage) -> bool {
    if image.pages.len() != 1 || image.leaves.len() != 1 {
        false
    } else {
        let leaf = image.leaves[0];
        &&& image.data_len > 0
        &&& image.start as int + image.len as int <= image.data_len as int
        &&& leaf.page == 0
        &&& leaf.logical_start == 0
        &&& leaf.len == image.data_len
        &&& leaf.page_start as int + image.data_len as int
            <= image.pages[0].bytes.len()
        &&& leaf.page_start as int + image.data_len as int
            <= image.pages[0].anchors.len()
        &&& image.logical_pages.len() * 32 >= image.data_len as int
        &&& forall|block: int| 0 <= block < image.logical_pages.len()
            ==> image.logical_pages[block] == 0
    }
}

/// Exact metadata exported by production `freeze_owned_segment`: one retained
/// page pair, one leaf starting at logical zero, and one zero entry per
/// 32-byte logical block. `page_bytes` and `page_anchors` denote the actual
/// borrowed Arc page slices passed to the verified shared-slot accessor.
pub open spec fn wire_frozen_owned_block_count(used: usize) -> int {
    (used as int) / 32 + (if (used as int) % 32 == 0 { 0int } else { 1int })
}

pub open spec fn wire_frozen_owned_segment_view(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize, view_start: usize, view_len: usize,
) -> WireFrozenImage {
    WireFrozenImage {
        pages: seq![WireFrozenPageImage {
            bytes: page_bytes, anchors: page_anchors,
        }],
        leaves: seq![WireFrozenLeafImage {
            page: 0, page_start, len: used, logical_start: 0,
        }],
        logical_pages: Seq::new(wire_frozen_owned_block_count(used) as nat,
            |block: int| 0usize),
        data_len: used,
        start: view_start,
        len: view_len,
    }
}

pub proof fn wire_frozen_owned_segment_view_wf(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize, view_start: usize, view_len: usize,
)
    requires
        used > 0,
        page_start as int + used as int <= page_bytes.len(),
        page_start as int + used as int <= page_anchors.len(),
        view_start as int + view_len as int <= used as int,
    ensures wire_frozen_one_leaf_wf(wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    )),
{
    let blocks = wire_frozen_owned_block_count(used);
    let value = used as int;
    let quotient = value / 32;
    let remainder = value % 32;
    vstd::arithmetic::div_mod::lemma_fundamental_div_mod(value, 32);
    vstd::arithmetic::div_mod::lemma_mod_pos_bound(value, 32);
    assert(value == 32 * quotient + remainder);
    assert(0 <= remainder < 32);
    assert(quotient >= 0);
    assert(blocks == quotient + (if remainder == 0 { 0int } else { 1int }));
    if remainder == 0 {
        assert(blocks == quotient);
        assert(blocks * 32 == value);
    } else {
        assert(blocks == quotient + 1);
        assert(blocks * 32 == value + 32 - remainder);
    }
    assert(blocks >= 0);
    assert(blocks * 32 >= value);
    let image = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    );
    assert(image.logical_pages.len() == blocks);
    assert forall|block: int| 0 <= block < image.logical_pages.len()
        implies image.logical_pages[block] == 0 by {}
    assert(wire_frozen_one_leaf_wf(image));
}

pub proof fn wire_frozen_owned_segment_subview_preserves_wf(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize, view_start: usize, view_len: usize,
    sub_start: usize, sub_end: usize,
)
    requires
        used > 0,
        page_start as int + used as int <= page_bytes.len(),
        page_start as int + used as int <= page_anchors.len(),
        view_start as int + view_len as int <= used as int,
        sub_start <= sub_end <= view_len,
    ensures {
        let before = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, view_start, view_len,
        );
        let after = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used,
            (view_start as int + sub_start as int) as usize,
            (sub_end as int - sub_start as int) as usize,
        );
        &&& wire_frozen_one_leaf_wf(after)
        &&& after.pages == before.pages
        &&& after.leaves == before.leaves
        &&& after.logical_pages == before.logical_pages
    },
{
    let next_start = (view_start as int + sub_start as int) as usize;
    let next_len = (sub_end as int - sub_start as int) as usize;
    assert(next_start as int + next_len as int <= used as int);
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, next_start, next_len,
    );
}

/// `leaf_window` and `FrozenCanonicalBytes::subview` retain the same page Arc
/// and change only the checked view range. Every selected byte keeps its
/// original paired page entry and physical provenance.
pub proof fn wire_frozen_owned_segment_subview_preserves_lookup(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize, view_start: usize, view_len: usize,
    sub_start: usize, sub_end: usize,
)
    requires
        used > 0,
        page_start as int + used as int <= page_bytes.len(),
        page_start as int + used as int <= page_anchors.len(),
        view_start as int + view_len as int <= used as int,
        sub_start <= sub_end <= view_len,
    ensures {
        let before = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, view_start, view_len,
        );
        let after = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used,
            (view_start as int + sub_start as int) as usize,
            (sub_end as int - sub_start as int) as usize,
        );
        &&& wire_frozen_one_leaf_wf(after)
        &&& forall|i: int| 0 <= i < after.len as int
            ==> (#[trigger] wire_frozen_at(after, i as usize))
                == wire_frozen_at(before, (sub_start as int + i) as usize)
    },
{
    let before = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    );
    let after = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used,
        (view_start as int + sub_start as int) as usize,
        (sub_end as int - sub_start as int) as usize,
    );
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    );
    wire_frozen_owned_segment_subview_preserves_wf(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
        sub_start, sub_end,
    );
    assert forall|i: int| 0 <= i < after.len as int
        implies (#[trigger] wire_frozen_at(after, i as usize))
            == wire_frozen_at(before, (sub_start as int + i) as usize) by {
        let offset = i as usize;
        let before_offset = (sub_start as int + i) as usize;
        assert(before_offset < before.len);
        wire_frozen_one_leaf_at_matches_paired_slot(after, offset);
        wire_frozen_one_leaf_at_matches_paired_slot(before, before_offset);
        assert(after.leaves[0].page_start as int + after.start as int + i
            == before.leaves[0].page_start as int + before.start as int + sub_start as int + i);
    }
}

/// Production Frozen coalescing requires `Arc::ptr_eq` on the retained data
/// and adjacent view ranges. Given that same-backing interpretation, joining
/// the ranges preserves each side's byte and physical provenance exactly.
/// Mapping `Arc::ptr_eq` to equal ghost page/leaf/index sequences remains a
/// separate Rust-to-Verus representation obligation.
pub proof fn wire_frozen_owned_segment_coalesce_preserves_lookup(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize,
    left_start: usize, left_len: usize, right_len: usize,
)
    requires
        used > 0,
        page_start as int + used as int <= page_bytes.len(),
        page_start as int + used as int <= page_anchors.len(),
        left_start as int + left_len as int + right_len as int <= used as int,
    ensures {
        let right_start = (left_start as int + left_len as int) as usize;
        let combined_len = (left_len as int + right_len as int) as usize;
        let left = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, left_start, left_len,
        );
        let right = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, right_start, right_len,
        );
        let joined = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, left_start, combined_len,
        );
        &&& wire_frozen_one_leaf_wf(left)
        &&& wire_frozen_one_leaf_wf(right)
        &&& wire_frozen_one_leaf_wf(joined)
        &&& forall|i: int| 0 <= i < combined_len as int ==> {
            let selected = if i < left_len as int {
                wire_frozen_at(left, i as usize)
            } else {
                wire_frozen_at(right, (i - left_len as int) as usize)
            };
            (#[trigger] wire_frozen_at(joined, i as usize)) == selected
        }
    },
{
    let right_start = (left_start as int + left_len as int) as usize;
    let combined_len = (left_len as int + right_len as int) as usize;
    let left = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, left_start, left_len,
    );
    let right = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, right_start, right_len,
    );
    let joined = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, left_start, combined_len,
    );
    assert(left_start as int + combined_len as int <= used as int);
    assert(right_start as int + right_len as int <= used as int);
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, left_start, left_len,
    );
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, right_start, right_len,
    );
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, left_start, combined_len,
    );
    assert forall|i: int| 0 <= i < combined_len as int implies {
        let selected = if i < left_len as int {
            wire_frozen_at(left, i as usize)
        } else {
            wire_frozen_at(right, (i - left_len as int) as usize)
        };
        (#[trigger] wire_frozen_at(joined, i as usize)) == selected
    } by {
        let offset = i as usize;
        wire_frozen_one_leaf_at_matches_paired_slot(joined, offset);
        if i < left_len as int {
            wire_frozen_one_leaf_at_matches_paired_slot(left, offset);
            assert(joined.leaves[0].page_start as int + joined.start as int + i
                == left.leaves[0].page_start as int + left.start as int + i);
        } else {
            let right_offset = (i - left_len as int) as usize;
            assert(right_offset < right.len);
            wire_frozen_one_leaf_at_matches_paired_slot(right, right_offset);
            assert(joined.leaves[0].page_start as int + joined.start as int + i
                == right.leaves[0].page_start as int + right.start as int
                    + right_offset as int);
        }
    }
}

/// Under the constructor shape, the formal accessor reads the byte and origin
/// from the exact shared page slot used by `frozen_single_leaf_at`.
pub proof fn wire_frozen_one_leaf_at_matches_paired_slot(
    image: WireFrozenImage, offset: usize,
)
    requires wire_frozen_one_leaf_wf(image), offset < image.len,
    ensures {
        let slot = image.leaves[0].page_start as int
            + image.start as int + offset as int;
        wire_frozen_at(image, offset) == Some(WireLocatedByte {
            value: image.pages[0].bytes[slot],
            anchor: image.pages[0].anchors[slot],
        })
    },
{
    let absolute = (image.start as int + offset as int) as usize;
    assert((absolute as int) < image.data_len as int);
    assert((absolute as int) / 32 < image.logical_pages.len());
    assert(image.logical_pages[(absolute as int) / 32] == 0);
    assert(wire_frozen_leaf_at(image, absolute, 0) == Some(0));
}

pub proof fn wire_frozen_owned_segment_view_at_matches_pair(
    page_bytes: Seq<u8>, page_anchors: Seq<WireSourceAnchor>,
    page_start: usize, used: usize, view_start: usize, view_len: usize,
    offset: usize,
)
    requires
        used > 0,
        page_start as int + used as int <= page_bytes.len(),
        page_start as int + used as int <= page_anchors.len(),
        view_start as int + view_len as int <= used as int,
        offset < view_len,
    ensures {
        let image = wire_frozen_owned_segment_view(
            page_bytes, page_anchors, page_start, used, view_start, view_len,
        );
        let slot = page_start as int + view_start as int + offset as int;
        wire_frozen_at(image, offset) == Some(WireLocatedByte {
            value: page_bytes[slot], anchor: page_anchors[slot],
        })
    },
{
    let image = wire_frozen_owned_segment_view(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    );
    wire_frozen_owned_segment_view_wf(
        page_bytes, page_anchors, page_start, used, view_start, view_len,
    );
    wire_frozen_one_leaf_at_matches_paired_slot(image, offset);
}

pub proof fn wire_frozen_one_leaf_total(image: WireFrozenImage)
    requires wire_frozen_one_leaf_wf(image),
    ensures forall|i: int| 0 <= i < image.len as int
        ==> (#[trigger] wire_frozen_at(image, i as usize)).is_some(),
{
    assert forall|i: int| 0 <= i < image.len as int
        implies (#[trigger] wire_frozen_at(image, i as usize)).is_some() by {
        let absolute = image.start as int + i;
        assert(0 <= absolute < image.data_len as int);
        assert(absolute / 32 < image.logical_pages.len());
        assert(image.logical_pages[absolute / 32] == 0);
        assert(wire_frozen_leaf_at(image, absolute as usize, 0) == Some(0));
        assert(wire_frozen_at(image, i as usize).is_some());
    }
}

/// Each source node is created from an already valid leaf or from checked
/// child-length sums (`node2`/`node3`). A postorder ghost numbering gives
/// every child a smaller index than its parent, so the modeled lookup fuel
/// is sufficient even when the production Arc tree shares subtrees.
pub open spec fn wire_source_node_postorder_shape(
    nodes: Seq<WireSourceNodeImage>, id: usize,
) -> bool {
    if id as int >= nodes.len() || nodes[id as int].len == 0 {
        false
    } else {
        let node = nodes[id as int];
        match node.payload {
            WireSourcePayloadImage::Borrowed {
                bytes, start, len, source: _, absolute_start,
            } => {
                &&& node.len == len
                &&& start as int + len as int <= bytes.len()
                &&& absolute_start as int + start as int + len as int - 1
                    <= usize::MAX as int
            },
            WireSourcePayloadImage::Frozen { value } => {
                &&& node.len == value.len
                &&& wire_frozen_one_leaf_wf(value)
            },
            WireSourcePayloadImage::Branch2 { left, right } => {
                &&& left < id && right < id
                &&& node.len as int == nodes[left as int].len as int
                    + nodes[right as int].len as int
            },
            WireSourcePayloadImage::Branch3 { first, second, third } => {
                &&& first < id && second < id && third < id
                &&& node.len as int == nodes[first as int].len as int
                    + nodes[second as int].len as int
                    + nodes[third as int].len as int
            },
        }
    }
}

pub open spec fn wire_source_nodes_postorder_wf(nodes: Seq<WireSourceNodeImage>) -> bool {
    &&& nodes.len() <= usize::MAX as int
    &&& forall|id: int| 0 <= id < nodes.len()
        ==> (#[trigger] wire_source_node_postorder_shape(nodes, id as usize))
}

pub open spec fn wire_source_tree_postorder_wf(tree: WireSourceTreeImage) -> bool {
    &&& wire_source_nodes_postorder_wf(tree.nodes)
    &&& match tree.root {
        Some(root) => {
            &&& (root as int) < tree.nodes.len()
            &&& tree.start as int + tree.len as int
                <= tree.nodes[root as int].len as int
        },
        None => tree.len == 0,
    }
}

pub proof fn wire_source_node_postorder_total(
    nodes: Seq<WireSourceNodeImage>, id: usize, offset: usize, fuel: usize,
)
    requires
        wire_source_nodes_postorder_wf(nodes),
        (id as int) < nodes.len(),
        offset < nodes[id as int].len,
        id < fuel,
    ensures wire_source_node_at(nodes, id, offset, fuel).is_some(),
    decreases id,
{
    assert(0 <= (id as int) < nodes.len());
    assert(((id as int) as usize) == id);
    assert(wire_source_node_postorder_shape(nodes, (id as int) as usize));
    assert(wire_source_node_postorder_shape(nodes, id));
    assert(fuel > 0);
    let child_fuel = (fuel as int - 1) as usize;
    match nodes[id as int].payload {
        WireSourcePayloadImage::Borrowed {
            bytes: _, start: _, len: _, source: _, absolute_start: _,
        } => {
            assert(wire_source_node_at(nodes, id, offset, fuel).is_some());
        },
        WireSourcePayloadImage::Frozen { value } => {
            wire_frozen_one_leaf_total(value);
            assert(wire_frozen_at(value, offset).is_some());
        },
        WireSourcePayloadImage::Branch2 { left, right } => {
            if offset < nodes[left as int].len {
                wire_source_node_postorder_total(nodes, left, offset, child_fuel);
            } else {
                assert(offset >= nodes[left as int].len);
                let child_offset = (offset as int - nodes[left as int].len as int) as usize;
                assert(child_offset < nodes[right as int].len);
                wire_source_node_postorder_total(nodes, right, child_offset, child_fuel);
            }
        },
        WireSourcePayloadImage::Branch3 { first, second, third } => {
            let first_len = nodes[first as int].len;
            let second_len = nodes[second as int].len;
            if offset < first_len {
                wire_source_node_postorder_total(nodes, first, offset, child_fuel);
            } else if (offset as int) < first_len as int + second_len as int {
                assert(offset >= first_len);
                let child_offset = (offset as int - first_len as int) as usize;
                assert(child_offset < second_len);
                wire_source_node_postorder_total(nodes, second, child_offset, child_fuel);
            } else {
                assert(offset as int >= first_len as int + second_len as int);
                let child_offset = (offset as int - first_len as int - second_len as int) as usize;
                assert(child_offset < nodes[third as int].len);
                wire_source_node_postorder_total(nodes, third, child_offset, child_fuel);
            }
        },
    }
    assert(wire_source_node_at(nodes, id, offset, fuel).is_some());
}

pub proof fn wire_source_tree_postorder_total(tree: WireSourceTreeImage)
    requires wire_source_tree_postorder_wf(tree),
    ensures forall|i: int| 0 <= i < tree.len as int
        ==> (#[trigger] wire_source_tree_at(tree, i as usize)).is_some(),
{
    assert forall|i: int| 0 <= i < tree.len as int
        implies (#[trigger] wire_source_tree_at(tree, i as usize)).is_some() by {
        let root = tree.root.unwrap();
        let absolute = (tree.start as int + i) as usize;
        assert(absolute < tree.nodes[root as int].len);
        wire_source_node_postorder_total(
            tree.nodes, root, absolute, tree.nodes.len() as usize,
        );
    }
}

pub proof fn wire_source_tree_postorder_at_total(
    tree: WireSourceTreeImage, offset: usize,
)
    requires wire_source_tree_postorder_wf(tree), offset < tree.len,
    ensures wire_source_tree_at(tree, offset).is_some(),
{
    assert(tree.root.is_some());
    let root = tree.root.unwrap();
    let absolute = (tree.start as int + offset as int) as usize;
    assert(absolute < tree.nodes[root as int].len);
    wire_source_node_postorder_total(
        tree.nodes, root, absolute, tree.nodes.len() as usize,
    );
}

pub struct WireSourceLiveViewImage {
    pub ghost tree: WireSourceTreeImage,
    pub ghost lineage: nat,
    pub ghost leaf_backing: Option<nat>,
    pub end: WireSourceAnchor,
}

pub open spec fn wire_source_live_view_wf(view: WireSourceLiveViewImage) -> bool {
    &&& wire_source_tree_postorder_wf(view.tree)
    &&& match view.tree.root {
        None => view.lineage == 0 && view.leaf_backing.is_none(),
        Some(_) => view.lineage > 0,
    }
}

pub open spec fn wire_source_live_view_at(
    view: WireSourceLiveViewImage, offset: usize,
) -> Option<WireLocatedByte> {
    wire_source_tree_at(view.tree, offset)
}

pub open spec fn wire_source_live_view_position(
    view: WireSourceLiveViewImage, offset: usize,
) -> Option<WireSourceAnchor> {
    if offset > view.tree.len {
        None
    } else if offset == view.tree.len {
        Some(view.end)
    } else {
        match wire_source_live_view_at(view, offset) {
            Some(located) => Some(located.anchor),
            None => None,
        }
    }
}

pub open spec fn wire_source_live_view_same(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
) -> bool {
    &&& left.lineage == right.lineage
    &&& left.tree.start == right.tree.start
    &&& left.tree.len == right.tree.len
}

pub open spec fn wire_source_live_indexed_same(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
) -> bool {
    wire_source_live_view_same(left, right) && left.end == right.end
}

pub open spec fn wire_source_live_view_clone(
    view: WireSourceLiveViewImage,
) -> WireSourceLiveViewImage {
    WireSourceLiveViewImage {
        tree: view.tree,
        lineage: view.lineage,
        leaf_backing: view.leaf_backing,
        end: view.end,
    }
}

pub proof fn wire_source_live_view_clone_preserves(view: WireSourceLiveViewImage)
    requires wire_source_live_view_wf(view),
    ensures
        wire_source_live_view_wf(wire_source_live_view_clone(view)),
        wire_source_live_view_same(view, wire_source_live_view_clone(view)),
        wire_source_live_indexed_same(view, wire_source_live_view_clone(view)),
        forall|i: int| 0 <= i <= view.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(wire_source_live_view_clone(view), i as usize))
                == wire_source_live_view_at(view, i as usize),
        forall|i: int| 0 <= i <= view.tree.len as int
            ==> (#[trigger] wire_source_live_view_position(wire_source_live_view_clone(view), i as usize))
                == wire_source_live_view_position(view, i as usize),
{
}

pub open spec fn wire_source_live_view_subview(
    view: WireSourceLiveViewImage, from: usize, to: usize,
) -> Option<WireSourceLiveViewImage> {
    if from > to || to > view.tree.len
        || view.tree.start as int + from as int > usize::MAX as int
    {
        None
    } else {
        Some(WireSourceLiveViewImage {
            tree: WireSourceTreeImage {
                nodes: view.tree.nodes,
                root: view.tree.root,
                start: (view.tree.start as int + from as int) as usize,
                len: (to as int - from as int) as usize,
            },
            lineage: view.lineage,
            leaf_backing: view.leaf_backing,
            end: if to == view.tree.len {
                view.end
            } else {
                wire_source_live_view_at(view, to).unwrap().anchor
            },
        })
    }
}

pub proof fn wire_source_live_view_subview_preserves(
    view: WireSourceLiveViewImage, from: usize, to: usize,
)
    requires wire_source_live_view_wf(view), from <= to, to <= view.tree.len,
    ensures {
        let selected = wire_source_live_view_subview(view, from, to).unwrap();
        &&& wire_source_live_view_subview(view, from, to).is_some()
        &&& wire_source_live_view_wf(selected)
        &&& selected.lineage == view.lineage
        &&& selected.leaf_backing == view.leaf_backing
        &&& selected.tree.nodes == view.tree.nodes
        &&& selected.tree.root == view.tree.root
        &&& selected.tree.len as int == to as int - from as int
        &&& wire_source_live_view_position(selected, selected.tree.len)
            == wire_source_live_view_position(view, to)
        &&& forall|i: int| 0 <= i < selected.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(selected, i as usize))
                == wire_source_live_view_at(view, (from as int + i) as usize)
    },
{
    if to < view.tree.len {
        wire_source_tree_postorder_at_total(view.tree, to);
        assert(wire_source_tree_at(view.tree, to).is_some());
        assert(wire_source_live_view_at(view, to).is_some());
    }
    let selected = wire_source_live_view_subview(view, from, to).unwrap();
    assert(wire_source_tree_postorder_wf(selected.tree));
    assert forall|i: int| 0 <= i < selected.tree.len as int
        implies (#[trigger] wire_source_live_view_at(selected, i as usize))
            == wire_source_live_view_at(view, (from as int + i) as usize) by {
        assert(selected.tree.start as int + i == view.tree.start as int + from as int + i);
    }
}

pub struct WireSourceIndexedRegionImage {
    pub ghost root: WireSourceLiveViewImage,
    pub start: usize,
    pub len: usize,
    pub root_end: WireSourceAnchor,
}

pub open spec fn wire_source_indexed_region_wf(view: WireSourceIndexedRegionImage) -> bool {
    &&& wire_source_live_view_wf(view.root)
    &&& view.start as int + view.len as int <= view.root.tree.len as int
}

pub open spec fn wire_source_indexed_at(
    view: WireSourceIndexedRegionImage, offset: usize,
) -> Option<WireLocatedByte> {
    if offset >= view.len {
        None
    } else {
        wire_source_live_view_at(
            view.root, (view.start as int + offset as int) as usize,
        )
    }
}

pub open spec fn wire_source_indexed_position(
    view: WireSourceIndexedRegionImage, offset: usize,
) -> Option<WireSourceAnchor> {
    if offset > view.len {
        None
    } else if view.start as int + offset as int == view.root.tree.len as int {
        Some(view.root_end)
    } else {
        match wire_source_live_view_at(
            view.root, (view.start as int + offset as int) as usize,
        ) {
            Some(located) => Some(located.anchor),
            None => None,
        }
    }
}

pub open spec fn wire_source_indexed_same(
    left: WireSourceIndexedRegionImage, right: WireSourceIndexedRegionImage,
) -> bool {
    &&& wire_source_live_view_same(left.root, right.root)
    &&& left.start == right.start
    &&& left.len == right.len
    &&& left.root_end == right.root_end
}

pub open spec fn wire_source_indexed_clone(
    view: WireSourceIndexedRegionImage,
) -> WireSourceIndexedRegionImage {
    WireSourceIndexedRegionImage {
        root: wire_source_live_view_clone(view.root),
        start: view.start,
        len: view.len,
        root_end: view.root_end,
    }
}

pub proof fn wire_source_indexed_clone_preserves(view: WireSourceIndexedRegionImage)
    requires wire_source_indexed_region_wf(view),
    ensures
        wire_source_indexed_region_wf(wire_source_indexed_clone(view)),
        wire_source_indexed_same(view, wire_source_indexed_clone(view)),
        forall|i: int| 0 <= i <= view.len as int
            ==> (#[trigger] wire_source_indexed_at(wire_source_indexed_clone(view), i as usize))
                == wire_source_indexed_at(view, i as usize),
        forall|i: int| 0 <= i <= view.len as int
            ==> (#[trigger] wire_source_indexed_position(wire_source_indexed_clone(view), i as usize))
                == wire_source_indexed_position(view, i as usize),
{
}

pub open spec fn wire_source_indexed_subview(
    view: WireSourceIndexedRegionImage, from: usize, to: usize,
) -> Option<WireSourceIndexedRegionImage> {
    if from > to || to > view.len
        || view.start as int + from as int > usize::MAX as int
    {
        None
    } else {
        Some(WireSourceIndexedRegionImage {
            root: view.root,
            start: (view.start as int + from as int) as usize,
            len: (to as int - from as int) as usize,
            root_end: view.root_end,
        })
    }
}

pub proof fn wire_source_indexed_subview_preserves(
    view: WireSourceIndexedRegionImage, from: usize, to: usize,
)
    requires wire_source_indexed_region_wf(view), from <= to <= view.len,
    ensures {
        let selected = wire_source_indexed_subview(view, from, to).unwrap();
        &&& wire_source_indexed_subview(view, from, to).is_some()
        &&& wire_source_indexed_region_wf(selected)
        &&& wire_source_live_view_same(selected.root, view.root)
        &&& selected.start as int == view.start as int + from as int
        &&& selected.len as int == to as int - from as int
        &&& selected.root_end == view.root_end
        &&& wire_source_indexed_same(view, selected)
            == (from == 0 && to == view.len)
        &&& wire_source_indexed_position(selected, selected.len)
            == wire_source_indexed_position(view, to)
        &&& forall|i: int| 0 <= i < selected.len as int
            ==> (#[trigger] wire_source_indexed_at(selected, i as usize))
                == wire_source_indexed_at(view, (from as int + i) as usize)
        &&& forall|i: int| 0 <= i < selected.len as int
            ==> (#[trigger] wire_source_indexed_position(selected, i as usize))
                == wire_source_indexed_position(view, (from as int + i) as usize)
    },
{
    let selected = wire_source_indexed_subview(view, from, to).unwrap();
    if (view.start as int + to as int) < view.root.tree.len as int {
        wire_source_tree_postorder_at_total(
            view.root.tree, (view.start as int + to as int) as usize,
        );
    }
    assert forall|i: int| 0 <= i < selected.len as int
        implies (#[trigger] wire_source_indexed_at(selected, i as usize))
            == wire_source_indexed_at(view, (from as int + i) as usize) by {
        assert(selected.start as int + i == view.start as int + from as int + i);
    }
    assert forall|i: int| 0 <= i < selected.len as int
        implies (#[trigger] wire_source_indexed_position(selected, i as usize))
            == wire_source_indexed_position(view, (from as int + i) as usize) by {
        assert(selected.start as int + i == view.start as int + from as int + i);
    }
}

pub open spec fn wire_source_indexed_subview_with_end(
    view: WireSourceIndexedRegionImage, from: usize, to: usize,
    end: WireSourceAnchor,
) -> Option<WireSourceIndexedRegionImage> {
    if from > to || to > view.len {
        None
    } else {
        let root_from = (view.start as int + from as int) as usize;
        let root_to = (view.start as int + to as int) as usize;
        match wire_source_live_view_subview(view.root, root_from, root_to) {
            Some(root) => Some(WireSourceIndexedRegionImage {
                root,
                start: 0,
                len: (to as int - from as int) as usize,
                root_end: end,
            }),
            None => None,
        }
    }
}

pub proof fn wire_source_indexed_subview_with_end_preserves(
    view: WireSourceIndexedRegionImage, from: usize, to: usize,
    end: WireSourceAnchor,
)
    requires wire_source_indexed_region_wf(view), from <= to <= view.len,
    ensures {
        let selected = wire_source_indexed_subview_with_end(view, from, to, end).unwrap();
        &&& wire_source_indexed_subview_with_end(view, from, to, end).is_some()
        &&& wire_source_indexed_region_wf(selected)
        &&& selected.root.lineage == view.root.lineage
        &&& selected.root.leaf_backing == view.root.leaf_backing
        &&& selected.root.tree.nodes == view.root.tree.nodes
        &&& selected.root.tree.start as int
            == view.root.tree.start as int + view.start as int + from as int
        &&& selected.root.tree.len as int == to as int - from as int
        &&& selected.start == 0
        &&& selected.len as int == to as int - from as int
        &&& wire_source_indexed_position(selected, selected.len) == Some(end)
        &&& wire_source_indexed_same(view, selected)
            == (view.start == 0 && view.len == view.root.tree.len
                && from == 0 && to == view.len && end == view.root_end)
        &&& forall|i: int| 0 <= i < selected.len as int
            ==> (#[trigger] wire_source_indexed_at(selected, i as usize))
                == wire_source_indexed_at(view, (from as int + i) as usize)
        &&& forall|i: int| 0 <= i < selected.len as int
            ==> (#[trigger] wire_source_indexed_position(selected, i as usize))
                == wire_source_indexed_position(view, (from as int + i) as usize)
    },
{
    let root_from = (view.start as int + from as int) as usize;
    let root_to = (view.start as int + to as int) as usize;
    wire_source_live_view_subview_preserves(view.root, root_from, root_to);
    let selected = wire_source_indexed_subview_with_end(view, from, to, end).unwrap();
    assert forall|i: int| 0 <= i < selected.len as int
        implies (#[trigger] wire_source_indexed_at(selected, i as usize))
            == wire_source_indexed_at(view, (from as int + i) as usize) by {
        assert(selected.root.tree.start as int + i
            == view.root.tree.start as int + view.start as int + from as int + i);
    }
    assert forall|i: int| 0 <= i < selected.len as int
        implies (#[trigger] wire_source_indexed_position(selected, i as usize))
            == wire_source_indexed_position(view, (from as int + i) as usize) by {
        assert(selected.root.tree.start as int + i
            == view.root.tree.start as int + view.start as int + from as int + i);
    }
}

pub open spec fn wire_source_indexed_runtime(
    view: WireSourceIndexedRegionImage,
) -> WireRuntimeBackingImage {
    wire_runtime_composed_from_tree(view.root.tree, view.start, view.len, view.root_end)
}

pub proof fn wire_source_indexed_projects(view: WireSourceIndexedRegionImage)
    requires wire_source_indexed_region_wf(view),
    ensures wire_runtime_project_region(wire_source_indexed_runtime(view))
        == Some(wire_composed_tree_region(
            view.root.tree, view.start, view.len, view.root_end,
        )),
{
    wire_source_tree_postorder_total(view.root.tree);
    wire_runtime_composed_tree_projects(
        view.root.tree, view.start, view.len, view.root_end,
    );
}

pub open spec fn wire_source_live_view_same_root_coalesce(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
) -> Option<WireSourceLiveViewImage> {
    if left.lineage != right.lineage
        || left.leaf_backing != right.leaf_backing
        || left.tree.nodes != right.tree.nodes
        || left.tree.root != right.tree.root
        || left.tree.start as int + left.tree.len as int != right.tree.start as int
        || left.tree.len as int + right.tree.len as int > usize::MAX as int
    {
        None
    } else {
        Some(WireSourceLiveViewImage {
            tree: WireSourceTreeImage {
                nodes: left.tree.nodes,
                root: left.tree.root,
                start: left.tree.start,
                len: (left.tree.len as int + right.tree.len as int) as usize,
            },
            lineage: left.lineage,
            leaf_backing: left.leaf_backing,
            end: right.end,
        })
    }
}

pub proof fn wire_source_live_view_same_root_coalesce_preserves(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
)
    requires
        wire_source_live_view_wf(left),
        wire_source_live_view_wf(right),
        left.lineage == right.lineage,
        left.leaf_backing == right.leaf_backing,
        left.tree.nodes == right.tree.nodes,
        left.tree.root == right.tree.root,
        left.tree.start as int + left.tree.len as int == right.tree.start as int,
    ensures {
        let joined = wire_source_live_view_same_root_coalesce(left, right).unwrap();
        &&& wire_source_live_view_same_root_coalesce(left, right).is_some()
        &&& wire_source_live_view_wf(joined)
        &&& joined.lineage == left.lineage
        &&& joined.leaf_backing == left.leaf_backing
        &&& joined.tree.len as int == left.tree.len as int + right.tree.len as int
        &&& wire_source_live_view_position(joined, joined.tree.len) == Some(right.end)
        &&& forall|i: int| 0 <= i < joined.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(joined, i as usize))
                == if i < left.tree.len as int {
                    wire_source_live_view_at(left, i as usize)
                } else {
                    wire_source_live_view_at(
                        right, (i - left.tree.len as int) as usize,
                    )
                }
    },
{
    let joined = wire_source_live_view_same_root_coalesce(left, right).unwrap();
    assert(wire_source_tree_postorder_wf(joined.tree));
    assert forall|i: int| 0 <= i < joined.tree.len as int
        implies (#[trigger] wire_source_live_view_at(joined, i as usize))
            == if i < left.tree.len as int {
                wire_source_live_view_at(left, i as usize)
            } else {
                wire_source_live_view_at(
                    right, (i - left.tree.len as int) as usize,
                )
            } by {
        if i < left.tree.len as int {
            assert(joined.tree.start as int + i == left.tree.start as int + i);
        } else {
            assert(joined.tree.start as int + i
                == right.tree.start as int + i - left.tree.len as int);
        }
    }
}

pub proof fn wire_source_live_view_lookup_total(view: WireSourceLiveViewImage)
    requires wire_source_live_view_wf(view),
    ensures
        forall|i: int| 0 <= i < view.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(view, i as usize)).is_some(),
        wire_source_live_view_position(view, view.tree.len) == Some(view.end),
{
    wire_source_tree_postorder_total(view.tree);
}

pub open spec fn wire_source_live_view_runtime(
    view: WireSourceLiveViewImage,
) -> WireRuntimeBackingImage {
    wire_runtime_composed_from_tree(view.tree, 0, view.tree.len, view.end)
}

pub proof fn wire_source_live_view_projects(view: WireSourceLiveViewImage)
    requires wire_source_live_view_wf(view),
    ensures wire_runtime_project_region(wire_source_live_view_runtime(view))
        == Some(wire_composed_tree_region(view.tree, 0, view.tree.len, view.end)),
{
    wire_source_tree_postorder_total(view.tree);
    wire_runtime_composed_tree_projects(view.tree, 0, view.tree.len, view.end);
}

pub open spec fn wire_source_live_borrowed_leaf(
    bytes: Seq<u8>, leaf_start: usize, leaf_len: usize,
    source: u32, absolute_start: usize, backing: nat, lineage: nat,
    view_start: usize, view_len: usize, end: WireSourceAnchor,
) -> WireSourceLiveViewImage {
    WireSourceLiveViewImage {
        tree: WireSourceTreeImage {
            nodes: Seq::empty().push(WireSourceNodeImage {
                len: leaf_len,
                payload: WireSourcePayloadImage::Borrowed {
                    bytes, start: leaf_start, len: leaf_len, source, absolute_start,
                },
            }),
            root: Some(0),
            start: view_start,
            len: view_len,
        },
        lineage,
        leaf_backing: Some(backing),
        end,
    }
}

pub open spec fn wire_source_live_borrowed_original_coalesce(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
    fresh_lineage: nat,
) -> Option<WireSourceLiveViewImage> {
    if left.tree.root != Some(0) || right.tree.root != Some(0)
        || left.tree.nodes.len() != 1 || right.tree.nodes.len() != 1
        || left.leaf_backing.is_none()
        || left.leaf_backing != right.leaf_backing
    {
        None
    } else {
        match (left.tree.nodes[0].payload, right.tree.nodes[0].payload) {
            (
                WireSourcePayloadImage::Borrowed {
                    bytes: left_bytes, start: left_start, len: _,
                    source: left_source, absolute_start: left_absolute,
                },
                WireSourcePayloadImage::Borrowed {
                    bytes: right_bytes, start: right_start, len: _,
                    source: right_source, absolute_start: right_absolute,
                },
            ) => {
                let selected_start = left_start as int + left.tree.start as int;
                let selected_right = right_start as int + right.tree.start as int;
                let total = left.tree.len as int + right.tree.len as int;
                if left_bytes != right_bytes || left_source != right_source
                    || left_absolute != right_absolute
                    || selected_start + left.tree.len as int != selected_right
                    || total > usize::MAX as int
                {
                    None
                } else if total == 0 {
                    Some(WireSourceLiveViewImage {
                        tree: left.tree,
                        lineage: left.lineage,
                        leaf_backing: left.leaf_backing,
                        end: right.end,
                    })
                } else if fresh_lineage == 0
                    || fresh_lineage == left.lineage
                    || fresh_lineage == right.lineage
                    || selected_start > usize::MAX as int
                    || selected_start + total > left_bytes.len()
                    || left_absolute as int + selected_start > usize::MAX as int
                    || left_absolute as int + selected_start + total - 1 > usize::MAX as int
                {
                    None
                } else {
                    Some(wire_source_live_borrowed_leaf(
                        left_bytes, selected_start as usize, total as usize,
                        left_source, left_absolute, left.leaf_backing.unwrap(),
                        fresh_lineage, 0, total as usize, right.end,
                    ))
                }
            },
            _ => None,
        }
    }
}

pub proof fn wire_source_live_borrowed_original_coalesce_preserves(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
    fresh_lineage: nat,
)
    requires
        wire_source_live_view_wf(left),
        wire_source_live_view_wf(right),
        wire_source_live_borrowed_original_coalesce(left, right, fresh_lineage).is_some(),
    ensures {
        let joined = wire_source_live_borrowed_original_coalesce(
            left, right, fresh_lineage,
        ).unwrap();
        &&& wire_source_live_view_wf(joined)
        &&& joined.leaf_backing == left.leaf_backing
        &&& joined.tree.len as int == left.tree.len as int + right.tree.len as int
        &&& wire_source_live_view_position(joined, joined.tree.len) == Some(right.end)
        &&& (if joined.tree.len == 0 {
            wire_source_live_view_same(left, joined)
        } else {
            joined.lineage == fresh_lineage
                && !wire_source_live_view_same(left, joined)
                && !wire_source_live_view_same(right, joined)
        })
        &&& forall|i: int| 0 <= i < joined.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(joined, i as usize))
                == if i < left.tree.len as int {
                    wire_source_live_view_at(left, i as usize)
                } else {
                    wire_source_live_view_at(
                        right, (i - left.tree.len as int) as usize,
                    )
                }
    },
{
    let joined = wire_source_live_borrowed_original_coalesce(
        left, right, fresh_lineage,
    ).unwrap();
    assert(left.tree.root == Some(0) && right.tree.root == Some(0));
    assert(left.tree.nodes.len() == 1 && right.tree.nodes.len() == 1);
    if joined.tree.len == 0 {
        assert(wire_source_live_view_wf(joined));
    } else {
        match (left.tree.nodes[0].payload, right.tree.nodes[0].payload) {
            (
                WireSourcePayloadImage::Borrowed {
                    bytes: left_bytes, start: left_start, len: left_leaf_len,
                    source: left_source, absolute_start: left_absolute,
                },
                WireSourcePayloadImage::Borrowed {
                    bytes: right_bytes, start: right_start, len: right_leaf_len,
                    source: right_source, absolute_start: right_absolute,
                },
            ) => {
                assert(left_bytes == right_bytes);
                assert(left_source == right_source);
                assert(left_absolute == right_absolute);
                let selected_start = left_start as int + left.tree.start as int;
                let selected_right = right_start as int + right.tree.start as int;
                assert(selected_start + left.tree.len as int == selected_right);
                assert(joined.tree.start == 0);
                assert(joined.tree.root == Some(0));
                assert(joined.tree.nodes.len() == 1);
                assert(joined.tree.nodes[0].payload == WireSourcePayloadImage::Borrowed {
                    bytes: left_bytes,
                    start: selected_start as usize,
                    len: joined.tree.len,
                    source: left_source,
                    absolute_start: left_absolute,
                });
                assert(wire_source_live_view_wf(joined));
                assert forall|i: int| 0 <= i < joined.tree.len as int
                    implies (#[trigger] wire_source_live_view_at(joined, i as usize))
                        == if i < left.tree.len as int {
                            wire_source_live_view_at(left, i as usize)
                        } else {
                            wire_source_live_view_at(
                                right, (i - left.tree.len as int) as usize,
                            )
                } by {
                    let physical = selected_start + i;
                    let located = WireLocatedByte {
                        value: left_bytes[physical],
                        anchor: WireSourceAnchor {
                            source: left_source,
                            offset: (left_absolute as int + physical) as usize,
                        },
                    };
                    assert(0 <= physical < left_bytes.len());
                    assert(left_absolute as int + physical <= usize::MAX as int);
                    assert(wire_source_live_view_at(joined, i as usize) == Some(located));
                    if i < left.tree.len as int {
                        let local = (left.tree.start as int + i) as usize;
                        assert(wire_source_node_postorder_shape(left.tree.nodes, 0));
                        assert(left.tree.nodes[0].len == left_leaf_len);
                        assert(local < left.tree.nodes[0].len);
                        assert(local < left_leaf_len);
                        assert(left_start as int + local as int == physical);
                        assert((left_start as int + local as int) < left_bytes.len());
                        assert(left_absolute as int + left_start as int + local as int
                            <= usize::MAX as int);
                        assert(selected_start + i
                            == left_start as int + left.tree.start as int + i);
                        assert(wire_source_node_at(
                            left.tree.nodes, 0, local, 1,
                        ) == Some(located));
                        assert(wire_source_live_view_at(left, i as usize) == Some(located));
                    } else {
                        let right_i = (i - left.tree.len as int) as usize;
                        let local = (right.tree.start as int + right_i as int) as usize;
                        assert(wire_source_node_postorder_shape(right.tree.nodes, 0));
                        assert(right.tree.nodes[0].len == right_leaf_len);
                        assert(right_i < right.tree.len);
                        assert(local < right.tree.nodes[0].len);
                        assert(local < right_leaf_len);
                        assert(right_start as int + local as int == physical);
                        assert((right_start as int + local as int) < right_bytes.len());
                        assert(right_absolute as int + right_start as int + local as int
                            <= usize::MAX as int);
                        assert(selected_start + i
                            == right_start as int + right.tree.start as int
                                + i - left.tree.len as int);
                        assert(wire_source_node_at(
                            right.tree.nodes, 0, local, 1,
                        ) == Some(located));
                        assert(wire_source_live_view_at(
                            right, (i - left.tree.len as int) as usize,
                        ) == Some(located));
                    }
                }
            },
            _ => {
                assert(false);
            },
        }
    }
}

pub open spec fn wire_source_live_frozen_leaf(
    value: WireFrozenImage, backing: nat, lineage: nat, end: WireSourceAnchor,
) -> WireSourceLiveViewImage {
    WireSourceLiveViewImage {
        tree: WireSourceTreeImage {
            nodes: Seq::empty().push(WireSourceNodeImage {
                len: value.len,
                payload: WireSourcePayloadImage::Frozen { value },
            }),
            root: Some(0),
            start: 0,
            len: value.len,
        },
        lineage,
        leaf_backing: Some(backing),
        end,
    }
}

pub open spec fn wire_source_live_frozen_original_coalesce(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
    fresh_lineage: nat,
) -> Option<WireSourceLiveViewImage> {
    if left.tree.root != Some(0) || right.tree.root != Some(0)
        || left.tree.nodes.len() != 1 || right.tree.nodes.len() != 1
        || left.leaf_backing.is_none()
        || left.leaf_backing != right.leaf_backing
    {
        None
    } else {
        match (left.tree.nodes[0].payload, right.tree.nodes[0].payload) {
            (
                WireSourcePayloadImage::Frozen { value: left_value },
                WireSourcePayloadImage::Frozen { value: right_value },
            ) => {
                if !wire_frozen_one_leaf_wf(left_value)
                    || !wire_frozen_one_leaf_wf(right_value)
                {
                    None
                } else {
                    let page_bytes = left_value.pages[0].bytes;
                    let page_anchors = left_value.pages[0].anchors;
                    let page_start = left_value.leaves[0].page_start;
                    let used = left_value.data_len;
                    let selected_start = left_value.start as int + left.tree.start as int;
                    let selected_right = right_value.start as int + right.tree.start as int;
                    let total = left.tree.len as int + right.tree.len as int;
                    if left_value != wire_frozen_owned_segment_view(
                        page_bytes, page_anchors, page_start, used,
                        left_value.start, left_value.len,
                    ) || right_value != wire_frozen_owned_segment_view(
                        page_bytes, page_anchors, page_start, used,
                        right_value.start, right_value.len,
                    ) || selected_start + left.tree.len as int != selected_right
                        || selected_start + total > used as int
                        || total > usize::MAX as int
                    {
                        None
                    } else if total == 0 {
                        Some(WireSourceLiveViewImage {
                            tree: left.tree,
                            lineage: left.lineage,
                            leaf_backing: left.leaf_backing,
                            end: right.end,
                        })
                    } else if fresh_lineage == 0
                        || fresh_lineage == left.lineage
                        || fresh_lineage == right.lineage
                    {
                        None
                    } else {
                        let joined_value = wire_frozen_owned_segment_view(
                            page_bytes, page_anchors, page_start, used,
                            selected_start as usize, total as usize,
                        );
                        Some(wire_source_live_frozen_leaf(
                            joined_value, left.leaf_backing.unwrap(), fresh_lineage,
                            right.end,
                        ))
                    }
                }
            },
            _ => None,
        }
    }
}

pub proof fn wire_source_live_frozen_original_coalesce_preserves(
    left: WireSourceLiveViewImage, right: WireSourceLiveViewImage,
    fresh_lineage: nat,
)
    requires
        wire_source_live_view_wf(left),
        wire_source_live_view_wf(right),
        wire_source_live_frozen_original_coalesce(left, right, fresh_lineage).is_some(),
    ensures {
        let joined = wire_source_live_frozen_original_coalesce(
            left, right, fresh_lineage,
        ).unwrap();
        &&& wire_source_live_view_wf(joined)
        &&& joined.leaf_backing == left.leaf_backing
        &&& joined.tree.len as int == left.tree.len as int + right.tree.len as int
        &&& wire_source_live_view_position(joined, joined.tree.len) == Some(right.end)
        &&& (if joined.tree.len == 0 {
            wire_source_live_view_same(left, joined)
        } else {
            joined.lineage == fresh_lineage
                && !wire_source_live_view_same(left, joined)
                && !wire_source_live_view_same(right, joined)
        })
        &&& forall|i: int| 0 <= i < joined.tree.len as int
            ==> (#[trigger] wire_source_live_view_at(joined, i as usize))
                == if i < left.tree.len as int {
                    wire_source_live_view_at(left, i as usize)
                } else {
                    wire_source_live_view_at(
                        right, (i - left.tree.len as int) as usize,
                    )
                }
    },
{
    let joined = wire_source_live_frozen_original_coalesce(
        left, right, fresh_lineage,
    ).unwrap();
    assert(left.tree.root == Some(0) && right.tree.root == Some(0));
    assert(left.tree.nodes.len() == 1 && right.tree.nodes.len() == 1);
    if joined.tree.len == 0 {
        assert(wire_source_live_view_wf(joined));
    } else {
        match (left.tree.nodes[0].payload, right.tree.nodes[0].payload) {
            (
                WireSourcePayloadImage::Frozen { value: left_value },
                WireSourcePayloadImage::Frozen { value: right_value },
            ) => {
                assert(wire_frozen_one_leaf_wf(left_value));
                assert(wire_frozen_one_leaf_wf(right_value));
                assert(wire_source_node_postorder_shape(left.tree.nodes, 0));
                assert(wire_source_node_postorder_shape(right.tree.nodes, 0));
                assert(left.tree.nodes[0].len == left_value.len);
                assert(right.tree.nodes[0].len == right_value.len);
                let page_bytes = left_value.pages[0].bytes;
                let page_anchors = left_value.pages[0].anchors;
                let page_start = left_value.leaves[0].page_start;
                let used = left_value.data_len;
                let selected_start = left_value.start as int + left.tree.start as int;
                let selected_right = right_value.start as int + right.tree.start as int;
                let total = left.tree.len as int + right.tree.len as int;
                assert(left_value == wire_frozen_owned_segment_view(
                    page_bytes, page_anchors, page_start, used,
                    left_value.start, left_value.len,
                ));
                assert(right_value == wire_frozen_owned_segment_view(
                    page_bytes, page_anchors, page_start, used,
                    right_value.start, right_value.len,
                ));
                assert(selected_start + left.tree.len as int == selected_right);
                assert(selected_start + total <= used as int);
                let left_end = (left.tree.start as int + left.tree.len as int) as usize;
                let right_end = (right.tree.start as int + right.tree.len as int) as usize;
                assert(left_end <= left_value.len && right_end <= right_value.len);
                wire_frozen_owned_segment_subview_preserves_lookup(
                    page_bytes, page_anchors, page_start, used,
                    left_value.start, left_value.len,
                    left.tree.start, left_end,
                );
                wire_frozen_owned_segment_subview_preserves_lookup(
                    page_bytes, page_anchors, page_start, used,
                    right_value.start, right_value.len,
                    right.tree.start, right_end,
                );
                wire_frozen_owned_segment_coalesce_preserves_lookup(
                    page_bytes, page_anchors, page_start, used,
                    selected_start as usize, left.tree.len, right.tree.len,
                );
                let left_selected = wire_frozen_owned_segment_view(
                    page_bytes, page_anchors, page_start, used,
                    selected_start as usize, left.tree.len,
                );
                let right_selected = wire_frozen_owned_segment_view(
                    page_bytes, page_anchors, page_start, used,
                    selected_right as usize, right.tree.len,
                );
                let joined_value = wire_frozen_owned_segment_view(
                    page_bytes, page_anchors, page_start, used,
                    selected_start as usize, total as usize,
                );
                assert(wire_frozen_one_leaf_wf(joined_value));
                assert(joined.tree.nodes[0].payload
                    == WireSourcePayloadImage::Frozen { value: joined_value });
                assert(wire_source_live_view_wf(joined));
                assert forall|i: int| 0 <= i < joined.tree.len as int
                    implies (#[trigger] wire_source_live_view_at(joined, i as usize))
                        == if i < left.tree.len as int {
                            wire_source_live_view_at(left, i as usize)
                        } else {
                            wire_source_live_view_at(
                                right, (i - left.tree.len as int) as usize,
                            )
                        } by {
                    assert(wire_source_live_view_at(joined, i as usize)
                        == wire_frozen_at(joined_value, i as usize));
                    if i < left.tree.len as int {
                        assert(wire_source_live_view_at(left, i as usize)
                            == wire_frozen_at(
                                left_value,
                                (left.tree.start as int + i) as usize,
                            ));
                        assert(wire_frozen_at(left_selected, i as usize)
                            == wire_frozen_at(
                                left_value,
                                (left.tree.start as int + i) as usize,
                            ));
                    } else {
                        let right_i = (i - left.tree.len as int) as usize;
                        assert(right_i < right.tree.len);
                        assert(wire_source_live_view_at(right, right_i)
                            == wire_frozen_at(
                                right_value,
                                (right.tree.start as int + right_i as int) as usize,
                            ));
                        assert(wire_frozen_at(right_selected, right_i)
                            == wire_frozen_at(
                                right_value,
                                (right.tree.start as int + right_i as int) as usize,
                            ));
                    }
                }
            },
            _ => {
                assert(false);
            },
        }
    }
}

/// Appending a fresh node cannot change lookup through an existing postorder
/// ID: every branch reference of an old node points to an earlier old ID.
pub proof fn wire_source_old_lookup_after_append(
    nodes: Seq<WireSourceNodeImage>, appended: WireSourceNodeImage,
    id: usize, offset: usize, fuel: usize,
)
    requires
        wire_source_nodes_postorder_wf(nodes),
        (id as int) < nodes.len(),
    ensures wire_source_node_at(nodes.push(appended), id, offset, fuel)
        == wire_source_node_at(nodes, id, offset, fuel),
    decreases fuel,
{
    let extended = nodes.push(appended);
    assert(extended[id as int] == nodes[id as int]);
    if fuel == 0 || offset >= nodes[id as int].len {
        return;
    }
    assert(0 <= (id as int) < nodes.len());
    assert(((id as int) as usize) == id);
    assert(wire_source_node_postorder_shape(nodes, (id as int) as usize));
    assert(wire_source_node_postorder_shape(nodes, id));
    let child_fuel = (fuel as int - 1) as usize;
    match nodes[id as int].payload {
        WireSourcePayloadImage::Borrowed {
            bytes: _, start: _, len: _, source: _, absolute_start: _,
        } => {},
        WireSourcePayloadImage::Frozen { value: _ } => {},
        WireSourcePayloadImage::Branch2 { left, right } => {
            if offset < nodes[left as int].len {
                wire_source_old_lookup_after_append(nodes, appended, left, offset, child_fuel);
            } else {
                let local = (offset as int - nodes[left as int].len as int) as usize;
                wire_source_old_lookup_after_append(nodes, appended, right, local, child_fuel);
            }
        },
        WireSourcePayloadImage::Branch3 { first, second, third } => {
            let first_len = nodes[first as int].len;
            let second_len = nodes[second as int].len;
            if offset < first_len {
                wire_source_old_lookup_after_append(nodes, appended, first, offset, child_fuel);
            } else if (offset as int) < first_len as int + second_len as int {
                let local = (offset as int - first_len as int) as usize;
                wire_source_old_lookup_after_append(nodes, appended, second, local, child_fuel);
            } else {
                let local = (offset as int - first_len as int - second_len as int) as usize;
                wire_source_old_lookup_after_append(nodes, appended, third, local, child_fuel);
            }
        },
    }
}

/// Ghost counterparts of the production `node2` and `node3` constructors.
/// Their arguments are the postorder IDs assigned to the *actual* Arc children.
/// Establishing that assignment, including Arc::clone alias preservation and
/// Arc::new freshness, is the remaining cross-crate representation obligation.
pub open spec fn wire_source_append_branch2(
    nodes: Seq<WireSourceNodeImage>, left: usize, right: usize,
) -> Seq<WireSourceNodeImage> {
    nodes.push(WireSourceNodeImage {
        len: (nodes[left as int].len as int + nodes[right as int].len as int) as usize,
        payload: WireSourcePayloadImage::Branch2 { left, right },
    })
}

pub open spec fn wire_source_append_branch3(
    nodes: Seq<WireSourceNodeImage>, first: usize, second: usize, third: usize,
) -> Seq<WireSourceNodeImage> {
    nodes.push(WireSourceNodeImage {
        len: (nodes[first as int].len as int + nodes[second as int].len as int
            + nodes[third as int].len as int) as usize,
        payload: WireSourcePayloadImage::Branch3 { first, second, third },
    })
}

/// A fresh Branch2 leaves every existing postorder ID intact and selects the
/// exact child located byte (both value and physical origin) at every offset.
pub proof fn wire_source_append_branch2_projects(
    nodes: Seq<WireSourceNodeImage>, left: usize, right: usize,
)
    requires
        wire_source_nodes_postorder_wf(nodes),
        nodes.len() < usize::MAX as int,
        (left as int) < nodes.len(),
        (right as int) < nodes.len(),
        nodes[left as int].len as int + nodes[right as int].len as int <= usize::MAX as int,
    ensures {
        let extended = wire_source_append_branch2(nodes, left, right);
        let root = nodes.len() as usize;
        let left_len = nodes[left as int].len;
        let total = (nodes[left as int].len as int + nodes[right as int].len as int) as usize;
        &&& extended.len() == nodes.len() + 1
        &&& extended.subrange(0, nodes.len() as int) == nodes
        &&& wire_source_nodes_postorder_wf(extended)
        &&& forall|i: int| 0 <= i < total as int ==> {
            let offset = i as usize;
            let selected = if offset < left_len {
                wire_source_node_at(nodes, left, offset, root)
            } else {
                wire_source_node_at(
                    nodes, right, (i - left_len as int) as usize, root,
                )
            };
            &&& (#[trigger] wire_source_node_at(extended, root, i as usize, (root as int + 1) as usize))
                == selected
            &&& selected.is_some()
        }
    },
{
    let extended = wire_source_append_branch2(nodes, left, right);
    let root = nodes.len() as usize;
    let total = (nodes[left as int].len as int + nodes[right as int].len as int) as usize;
    assert(extended.subrange(0, nodes.len() as int) == nodes);
    assert forall|id: int| 0 <= id < extended.len()
        implies (#[trigger] wire_source_node_postorder_shape(extended, id as usize)) by {
        if id < nodes.len() {
            assert(wire_source_node_postorder_shape(nodes, id as usize));
            assert(extended[id] == nodes[id]);
        } else {
            assert(id == nodes.len());
            assert(wire_source_node_postorder_shape(nodes, (left as int) as usize));
            assert(wire_source_node_postorder_shape(nodes, (right as int) as usize));
            assert(nodes[left as int].len > 0 && nodes[right as int].len > 0);
            assert(left < root && right < root);
            assert(extended[left as int].len == nodes[left as int].len);
            assert(extended[right as int].len == nodes[right as int].len);
            assert(extended[id].len == total);
            assert(total > 0);
            assert(extended[id].payload == WireSourcePayloadImage::Branch2 { left, right });
            assert(wire_source_node_postorder_shape(extended, root));
        }
    }
    assert(wire_source_nodes_postorder_wf(extended));
    assert forall|i: int| 0 <= i < total as int implies {
        let offset = i as usize;
        let selected = if offset < nodes[left as int].len {
            wire_source_node_at(nodes, left, offset, root)
        } else {
            wire_source_node_at(
                nodes, right, (i - nodes[left as int].len as int) as usize, root,
            )
        };
        &&& (#[trigger] wire_source_node_at(extended, root, i as usize, (root as int + 1) as usize))
            == selected
        &&& selected.is_some()
    } by {
        let offset = i as usize;
        assert(offset < extended[root as int].len);
        assert(extended == nodes.push(extended[root as int]));
        if offset < nodes[left as int].len {
            wire_source_old_lookup_after_append(
                nodes, extended[root as int], left, offset, root,
            );
        } else {
            let local = (i - nodes[left as int].len as int) as usize;
            wire_source_old_lookup_after_append(
                nodes, extended[root as int], right, local, root,
            );
        }
        wire_source_node_postorder_total(extended, root, offset, (root as int + 1) as usize);
        assert(wire_source_node_at(extended, root, offset, (root as int + 1) as usize).is_some());
    }
}

/// The three-child constructor has the same prefix and located-byte property.
pub proof fn wire_source_append_branch3_projects(
    nodes: Seq<WireSourceNodeImage>, first: usize, second: usize, third: usize,
)
    requires
        wire_source_nodes_postorder_wf(nodes),
        nodes.len() < usize::MAX as int,
        (first as int) < nodes.len(),
        (second as int) < nodes.len(),
        (third as int) < nodes.len(),
        nodes[first as int].len as int + nodes[second as int].len as int
            + nodes[third as int].len as int <= usize::MAX as int,
    ensures {
        let extended = wire_source_append_branch3(nodes, first, second, third);
        let root = nodes.len() as usize;
        let first_len = nodes[first as int].len;
        let second_len = nodes[second as int].len;
        let total = (first_len as int + second_len as int + nodes[third as int].len as int) as usize;
        &&& extended.len() == nodes.len() + 1
        &&& extended.subrange(0, nodes.len() as int) == nodes
        &&& wire_source_nodes_postorder_wf(extended)
        &&& forall|i: int| 0 <= i < total as int ==> {
            let offset = i as usize;
            let selected = if offset < first_len {
                wire_source_node_at(nodes, first, offset, root)
            } else if i < first_len as int + second_len as int {
                wire_source_node_at(
                    nodes, second, (i - first_len as int) as usize, root,
                )
            } else {
                wire_source_node_at(
                    nodes, third,
                    (i - first_len as int - second_len as int) as usize, root,
                )
            };
            &&& (#[trigger] wire_source_node_at(extended, root, i as usize, (root as int + 1) as usize))
                == selected
            &&& selected.is_some()
        }
    },
{
    let extended = wire_source_append_branch3(nodes, first, second, third);
    let root = nodes.len() as usize;
    let total = (nodes[first as int].len as int + nodes[second as int].len as int
        + nodes[third as int].len as int) as usize;
    assert(extended.subrange(0, nodes.len() as int) == nodes);
    assert forall|id: int| 0 <= id < extended.len()
        implies (#[trigger] wire_source_node_postorder_shape(extended, id as usize)) by {
        if id < nodes.len() {
            assert(wire_source_node_postorder_shape(nodes, id as usize));
            assert(extended[id] == nodes[id]);
        } else {
            assert(id == nodes.len());
            assert(wire_source_node_postorder_shape(nodes, (first as int) as usize));
            assert(wire_source_node_postorder_shape(nodes, (second as int) as usize));
            assert(wire_source_node_postorder_shape(nodes, (third as int) as usize));
            assert(nodes[first as int].len > 0 && nodes[second as int].len > 0
                && nodes[third as int].len > 0);
            assert(first < root && second < root && third < root);
            assert(extended[first as int].len == nodes[first as int].len);
            assert(extended[second as int].len == nodes[second as int].len);
            assert(extended[third as int].len == nodes[third as int].len);
            assert(extended[id].len == total);
            assert(total > 0);
            assert(extended[id].payload == WireSourcePayloadImage::Branch3 {
                first, second, third,
            });
            assert(wire_source_node_postorder_shape(extended, root));
        }
    }
    assert(wire_source_nodes_postorder_wf(extended));
    assert forall|i: int| 0 <= i < total as int implies {
        let offset = i as usize;
        let first_len = nodes[first as int].len;
        let second_len = nodes[second as int].len;
        let selected = if offset < first_len {
            wire_source_node_at(nodes, first, offset, root)
        } else if i < first_len as int + second_len as int {
            wire_source_node_at(nodes, second, (i - first_len as int) as usize, root)
        } else {
            wire_source_node_at(
                nodes, third,
                (i - first_len as int - second_len as int) as usize, root,
            )
        };
        &&& (#[trigger] wire_source_node_at(extended, root, i as usize, (root as int + 1) as usize))
            == selected
        &&& selected.is_some()
    } by {
        let offset = i as usize;
        assert(offset < extended[root as int].len);
        assert(extended == nodes.push(extended[root as int]));
        let first_len = nodes[first as int].len;
        let second_len = nodes[second as int].len;
        if offset < first_len {
            wire_source_old_lookup_after_append(
                nodes, extended[root as int], first, offset, root,
            );
        } else if i < first_len as int + second_len as int {
            let local = (i - first_len as int) as usize;
            wire_source_old_lookup_after_append(
                nodes, extended[root as int], second, local, root,
            );
        } else {
            let local = (i - first_len as int - second_len as int) as usize;
            wire_source_old_lookup_after_append(
                nodes, extended[root as int], third, local, root,
            );
        }
        wire_source_node_postorder_total(extended, root, offset, (root as int + 1) as usize);
        assert(wire_source_node_at(extended, root, offset, (root as int + 1) as usize).is_some());
    }
}

/// These implications are universal for constructor-shaped ghost images.
/// Identifying a live `PageFrozenBytes`/`SourceTreeWindow` with the exact ghost
/// page arrays and Arc DAG is still a cross-crate extraction obligation.
pub proof fn wire_runtime_frozen_one_leaf_pre_read_projects(
    frozen: WireFrozenImage, start: usize, len: usize, root_end: WireSourceAnchor,
    reader_at: usize, last_field: u32, fields_read: usize,
)
    requires
        wire_frozen_one_leaf_wf(frozen),
        start as int + len as int <= frozen.len as int,
        reader_at <= len,
    ensures {
        let backing = wire_runtime_frozen_from_index(frozen, start, len, root_end);
        let observed = wire_frozen_index_region(frozen, start, len, root_end);
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(backing) == Some(observed)
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: len,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    wire_frozen_one_leaf_total(frozen);
    wire_runtime_frozen_message_pre_read_projects(
        frozen, start, len, root_end, reader_at, last_field, fields_read,
    );
}

pub proof fn wire_runtime_composed_postorder_pre_read_projects(
    tree: WireSourceTreeImage, start: usize, len: usize, root_end: WireSourceAnchor,
    reader_at: usize, last_field: u32, fields_read: usize,
)
    requires
        wire_source_tree_postorder_wf(tree),
        start as int + len as int <= tree.len as int,
        reader_at <= len,
    ensures {
        let backing = wire_runtime_composed_from_tree(tree, start, len, root_end);
        let observed = wire_composed_tree_region(tree, start, len, root_end);
        let state = wire_runtime_message_pre_read(
            backing, reader_at, last_field, fields_read,
        );
        &&& wire_runtime_project_region(backing) == Some(observed)
        &&& wire_runtime_cursor_project(state)
            == Some(WireSourceState {
                cursor: admission::IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: len,
                    next: reader_at,
                    source_bytes: observed.bytes,
                },
                last_field,
                enforce_numeric_order: false,
                terminal_error: None,
            })
    },
{
    wire_source_tree_postorder_total(tree);
    wire_runtime_composed_message_pre_read_projects(
        tree, start, len, root_end, reader_at, last_field, fields_read,
    );
}

/// A byte-derived varint field at the live runtime cursor. Unlike a field
/// receipt, this value is constructed from the two canonical reads of the
/// projected indexed body. The projection of the Rust backing and reader is
/// still an explicit premise of the refinement lemma below.
pub open spec fn wire_runtime_varint_continue_read(
    state: WireRuntimeCursorImage,
) -> Option<admission::IndexedCanonicalFieldModel> {
    match wire_runtime_cursor_project(state) {
        Some(source) if source.terminal_error.is_none()
            && wire_cursor_shape(source.cursor)
            && source.cursor.next < source.cursor.region_end => {
            let cursor = source.cursor;
            let bytes = cursor.source_bytes;
            let start = cursor.next;
            let limit = cursor.region_end;
            match wire_varint_read(bytes, start, limit) {
                WireVarintResult::Value { value: key, next: key_end }
                    if 0 < key / 8 <= 0x1fff_ffffu64
                        && key % 8 == 0
                        && (!source.enforce_numeric_order
                            || source.last_field as u64 <= key / 8) => {
                    match wire_varint_read(bytes, key_end, limit) {
                        WireVarintResult::Value { value, next: payload_end } => {
                            Some(admission::IndexedCanonicalFieldModel {
                                number: (key / 8) as u32,
                                encoded_start: start,
                                encoded_end: payload_end,
                                payload_start: key_end,
                                payload_end,
                                key_width: (key_end as int - start as int) as usize,
                                payload_width: (payload_end as int - key_end as int) as usize,
                                payload_length: 0,
                                wire: 0,
                                decoded_varint: value,
                                payload_bytes: bytes.subrange(
                                    key_end as int, payload_end as int,
                                ),
                            })
                        },
                        _ => None,
                    }
                },
                _ => None,
            }
        },
        _ => None,
    }
}

pub proof fn wire_runtime_varint_continue_read_refines(
    state: WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        wire_runtime_derive_cursor(state).is_some(),
        wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap()),
        wire_runtime_varint_continue_read(state) == Some(field),
    ensures
        admission::indexed_field_at_cursor(
            wire_runtime_cursor_project(state).unwrap().cursor, field,
        ),
        exists|after: WireConcreteCursorImage, start: WirePhysicalPosition|
            wire_runtime_next_field(state)
                == Some(WireConcreteTransition::Field { state: after, field, start }),
{
    let derived = wire_runtime_derive_cursor(state).unwrap();
    let source = wire_runtime_cursor_project(state).unwrap();
    let bytes = source.cursor.source_bytes;
    let start = source.cursor.next;
    let limit = source.cursor.region_end;
    assert(source.terminal_error.is_none());
    assert(wire_cursor_shape(source.cursor));
    assert(start < limit);
    if let WireVarintResult::Value { value: key, next: key_end } =
        wire_varint_read(bytes, start, limit)
    {
        if let WireVarintResult::Value { value: _, next: payload_end } =
            wire_varint_read(bytes, key_end, limit)
        {
            assert(wire_field_step(
                source.cursor, source.last_field, source.enforce_numeric_order, false,
            ) == WireFieldResult::Field {
                field, next: payload_end, last_field: field.number,
            });
            wire_source_next_field_refines(source);
            assert(admission::indexed_field_at_cursor(source.cursor, field));
            wire_concrete_next_field_or_matches_model(derived);
            assert(wire_concrete_cursor_project(derived) == Some(source));
            assert(wire_source_next_field(source)
                == WireSourceTransition::Field {
                    state: wire_source_with_cursor(source, payload_end, field.number, None),
                    field,
                });
            let region = wire_backing_project(derived.message).unwrap();
            assert(region.bytes == bytes);
            assert(region.anchors.len() == region.bytes.len());
            assert(wire_region_position(region, start).is_some());
            assert(wire_runtime_next_field(state) == wire_concrete_next_field(derived));
            if let Some(WireConcreteTransition::Field { state: after, field: read, start }) =
                wire_runtime_next_field(state)
            {
                assert(read == field);
            } else {
                assert(false);
            }
        } else {
            assert(false);
        }
    } else {
        assert(false);
    }
}

pub proof fn wire_runtime_next_field_from_source(state: WireRuntimeCursorImage)
    requires
        wire_runtime_derive_cursor(state).is_some(),
        wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap()),
    ensures match wire_runtime_next_field(state) {
        Some(WireConcreteTransition::Field { field, .. }) => {
            match wire_runtime_cursor_project(state) {
                Some(source) => exists|next: WireSourceState|
                    wire_source_next_field(source)
                        == WireSourceTransition::Field { state: next, field },
                None => false,
            }
        },
        _ => true,
    },
{
    let derived = wire_runtime_derive_cursor(state).unwrap();
    wire_concrete_next_field_or_matches_model(derived);
    if let Some(WireConcreteTransition::Field { field, .. }) = wire_runtime_next_field(state) {
        let source = wire_runtime_cursor_project(state).unwrap();
        if let WireSourceTransition::Field { state: next, field: source_field } =
            wire_source_next_field(source)
        {
            assert(source_field == field);
            assert(wire_source_next_field(source)
                == WireSourceTransition::Field { state: next, field });
        }
    }
}

pub proof fn wire_runtime_end_is_exact(state: WireRuntimeCursorImage)
    requires
        wire_runtime_derive_cursor(state).is_some(),
        wire_concrete_cursor_wf(wire_runtime_derive_cursor(state).unwrap()),
        wire_runtime_next_field(state)
            == Some(WireConcreteTransition::End {
                state: wire_runtime_derive_cursor(state).unwrap(),
            }),
    ensures
        wire_runtime_cursor_project(state).is_some(),
        wire_source_next_field(wire_runtime_cursor_project(state).unwrap())
            == (WireSourceTransition::End {
                state: wire_runtime_cursor_project(state).unwrap(),
            }),
        wire_runtime_cursor_project(state).unwrap().cursor.next
            == wire_runtime_cursor_project(state).unwrap().cursor.region_end,
        state.terminal_error.is_none(),
{
    let derived = wire_runtime_derive_cursor(state).unwrap();
    wire_concrete_next_field_or_matches_model(derived);
    let source = wire_runtime_cursor_project(state).unwrap();
    wire_source_next_field_refines(source);
}

pub open spec fn wire_empty_par_field_backing() -> WireRuntimeBackingImage {
    WireRuntimeBackingImage::Borrowed {
        bytes: seq![0x0au8, 0x00u8],
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: None,
    }
}

pub proof fn wire_empty_par_field_is_read()
    ensures exists|cursor: WireRuntimeCursorImage,
        after: WireConcreteCursorImage,
        field: admission::IndexedCanonicalFieldModel,
        start: WirePhysicalPosition| {
        &&& wire_runtime_initial(wire_empty_par_field_backing(), false)
            == Some(cursor)
        &&& wire_runtime_next_field(cursor)
            == Some(WireConcreteTransition::Field { state: after, field, start })
        &&& field.number == 1
        &&& field.wire == 2
        &&& field.payload_start == 2
        &&& field.payload_end == 2
        &&& field.payload_bytes == Seq::<u8>::empty()
    },
{
    assert(wire_runtime_initial(wire_empty_par_field_backing(), false).is_some());
    let cursor = wire_runtime_initial(wire_empty_par_field_backing(), false).unwrap();
    let region = wire_runtime_project_region(wire_empty_par_field_backing()).unwrap();
    assert(region.bytes =~= seq![0x0au8, 0x00u8]);
    assert(wire_varint_read(region.bytes, 0, 2)
        == WireVarintResult::Value { value: 10, next: 1 });
    assert(wire_varint_read(region.bytes, 1, 2)
        == WireVarintResult::Value { value: 0, next: 2 });
    let derived = wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_concrete_cursor_wf(derived));
    wire_concrete_next_field_or_matches_model(derived);
    match wire_runtime_next_field(cursor) {
        Some(WireConcreteTransition::Field { state: after, field, start }) => {
            assert(field.payload_bytes =~= Seq::<u8>::empty());
        },
        _ => assert(false),
    }
}

}

fn main() {}
