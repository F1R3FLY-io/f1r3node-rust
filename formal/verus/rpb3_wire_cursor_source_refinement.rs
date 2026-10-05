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
