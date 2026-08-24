use vstd::prelude::*;

verus! {

pub const RCI2_DOMAIN_LEN: usize = 4;
pub const U64_WORD_LEN: usize = 8;
pub const FRAME_END_LEN: usize = 2;
pub const ESCAPE: u8 = 0;
pub const ESCAPED_ZERO: u8 = 0xff;
pub const FRAME_END: u8 = 0;

/// The arithmetic image of `usize::checked_add` used by the production RCI2
/// cursor.  Expressing the success boundary over mathematical integers makes
/// the absence of modular wraparound explicit.
pub fn checked_cursor_add(cursor: usize, width: usize) -> (result: Option<usize>)
    ensures
        result.is_some()
            == (cursor as int + width as int <= usize::MAX as int),
        match result {
            Some(next) => next as int == cursor as int + width as int,
            None => cursor as int + width as int > usize::MAX as int,
        },
{
    if width > usize::MAX - cursor {
        None
    } else {
        Some(cursor + width)
    }
}

/// A checked addition followed by the slice-bound test performed by
/// `ChannelImageV2`.  Every successful cursor movement stays inside the input;
/// a positive-width movement also makes strict progress.
pub fn checked_bounded_advance(
    cursor: usize,
    end: usize,
    width: usize,
) -> (result: Option<usize>)
    ensures
        result.is_some() == (cursor as int + width as int <= end as int),
        match result {
            Some(next) => {
                &&& next as int == cursor as int + width as int
                &&& cursor <= next
                &&& next <= end
                &&& (0 < width ==> cursor < next)
            },
            None => cursor as int + width as int > end as int,
        },
{
    match checked_cursor_add(cursor, width) {
        None => None,
        Some(next) => {
            if next > end {
                None
            } else {
                Some(next)
            }
        },
    }
}

/// The one-byte flag increment at the end of the RCI2 parser cannot wrap and
/// is strictly monotone after the preceding bounds check has found a byte.
pub fn checked_cursor_increment(cursor: usize, end: usize) -> (next: usize)
    requires cursor < end,
    ensures
        next as int == cursor as int + 1,
        cursor < next,
        next <= end,
{
    match checked_bounded_advance(cursor, end, 1) {
        Some(next) => next,
        None => {
            assert(false);
            cursor
        },
    }
}

/// Mathematical value of one fixed-width big-endian word.
pub open spec fn u64_be_spec(
    b0: u8,
    b1: u8,
    b2: u8,
    b3: u8,
    b4: u8,
    b5: u8,
    b6: u8,
    b7: u8,
) -> int {
    b0 as int * 72_057_594_037_927_936
        + b1 as int * 281_474_976_710_656
        + b2 as int * 1_099_511_627_776
        + b3 as int * 4_294_967_296
        + b4 as int * 16_777_216
        + b5 as int * 65_536
        + b6 as int * 256
        + b7 as int
}

/// Executable counterpart of `u64::from_be_bytes` for the exact eight-byte
/// word copied by the production parser.
pub fn executable_u64_from_be_bytes(
    b0: u8,
    b1: u8,
    b2: u8,
    b3: u8,
    b4: u8,
    b5: u8,
    b6: u8,
    b7: u8,
) -> (result: u64)
    ensures result as int == u64_be_spec(b0, b1, b2, b3, b4, b5, b6, b7),
{
    let a0 = b0 as u64;
    let a1 = a0 * 256 + b1 as u64;
    let a2 = a1 * 256 + b2 as u64;
    let a3 = a2 * 256 + b3 as u64;
    let a4 = a3 * 256 + b4 as u64;
    let a5 = a4 * 256 + b5 as u64;
    let a6 = a5 * 256 + b6 as u64;
    a6 * 256 + b7 as u64
}

pub struct FixedU64Read {
    pub value: u64,
    pub next: usize,
}

/// Bind the fixed word conversion to the checked `cursor + 8` slice shape.
/// The caller supplies the eight bytes only after the Rust slice operation has
/// established this same success condition.
pub fn checked_fixed_u64_read(
    cursor: usize,
    end: usize,
    b0: u8,
    b1: u8,
    b2: u8,
    b3: u8,
    b4: u8,
    b5: u8,
    b6: u8,
    b7: u8,
) -> (result: Option<FixedU64Read>)
    ensures
        result.is_some() == (cursor as int + U64_WORD_LEN as int <= end as int),
        match result {
            Some(read) => {
                &&& read.next as int == cursor as int + U64_WORD_LEN as int
                &&& cursor < read.next
                &&& read.next <= end
                &&& read.value as int
                    == u64_be_spec(b0, b1, b2, b3, b4, b5, b6, b7)
            },
            None => cursor as int + U64_WORD_LEN as int > end as int,
        },
{
    match checked_bounded_advance(cursor, end, U64_WORD_LEN) {
        None => None,
        Some(next) => {
            let value = executable_u64_from_be_bytes(b0, b1, b2, b3, b4, b5, b6, b7);
            Some(FixedU64Read { value, next })
        },
    }
}

pub const ESCAPE_STEP_COPY: u8 = 0;
pub const ESCAPE_STEP_ZERO: u8 = 1;
pub const ESCAPE_STEP_FINISH: u8 = 2;
pub const ESCAPE_STEP_INVALID: u8 = 3;

pub struct EscapedFrameStep {
    pub next: usize,
    pub kind: u8,
}

/// One scalar iteration of `take_escaped_frame`. A nonzero byte consumes one
/// input byte. Every zero consumes a complete two-byte escape pair, classified
/// as escaped zero, endpoint terminator, or invalid escape. A truncated pair
/// is the sole failure after the first byte has already been observed.
pub fn checked_escaped_frame_step(
    cursor: usize,
    end: usize,
    first: u8,
    second: u8,
) -> (result: Option<EscapedFrameStep>)
    requires cursor < end,
    ensures
        result.is_some()
            == (first != ESCAPE || cursor as int + 2 <= end as int),
        match result {
            Some(step) => {
                &&& cursor < step.next
                &&& step.next <= end
                &&& end - step.next < end - cursor
                &&& (first != ESCAPE ==> {
                    &&& step.next as int == cursor as int + 1
                    &&& step.kind == ESCAPE_STEP_COPY
                })
                &&& (first == ESCAPE ==> {
                    &&& step.next as int == cursor as int + 2
                    &&& step.kind == if second == ESCAPED_ZERO {
                        ESCAPE_STEP_ZERO
                    } else if second == FRAME_END {
                        ESCAPE_STEP_FINISH
                    } else {
                        ESCAPE_STEP_INVALID
                    }
                })
            },
            None => first == ESCAPE && cursor as int + 2 > end as int,
        },
{
    if first != ESCAPE {
        let next = checked_cursor_increment(cursor, end);
        Some(EscapedFrameStep { next, kind: ESCAPE_STEP_COPY })
    } else {
        match checked_bounded_advance(cursor, end, 2) {
            None => None,
            Some(next) => {
                let kind = if second == ESCAPED_ZERO {
                    ESCAPE_STEP_ZERO
                } else if second == FRAME_END {
                    ESCAPE_STEP_FINISH
                } else {
                    ESCAPE_STEP_INVALID
                };
                Some(EscapedFrameStep { next, kind })
            },
        }
    }
}

/// The parser's observed-distinct counter accepts exactly the mathematical
/// successor values representable by `u64`.
pub fn checked_distinct_increment(observed: u64) -> (result: Option<u64>)
    ensures
        result.is_some() == (observed < u64::MAX),
        match result {
            Some(next) => next as int == observed as int + 1,
            None => observed == u64::MAX,
        },
{
    if observed == u64::MAX {
        None
    } else {
        Some(observed + 1)
    }
}

/// The sequence trailer is accepted only when the fixed-width declared count
/// is exactly the checked number of endpoint frames already consumed.
pub fn exact_distinct_count(declared: u64, observed: u64) -> (accepted: bool)
    ensures accepted == (declared == observed),
{
    declared == observed
}

/// RPB3 admission has already checked the complete canonical key (domain,
/// family, score, protobuf body, score-length trailer). RCI2 strips only the
/// common four-byte domain. The remaining relative key is nonempty and starts
/// with its nonzero family tag.
pub open spec fn admitted_rpb3_relative_key(key: Seq<u8>) -> bool {
    key.len() > 0 && key[0] != ESCAPE
}

/// Lexicographic byte order used by both PathMap endpoints and RCI2's strict
/// ascending-endpoint validation.
pub open spec fn byte_lex_less(left: Seq<u8>, right: Seq<u8>) -> bool
    decreases left.len() + right.len(),
{
    if left.len() == 0 {
        right.len() > 0
    } else if right.len() == 0 {
        false
    } else if left[0] < right[0] {
        true
    } else if left[0] > right[0] {
        false
    } else {
        byte_lex_less(left.drop_first(), right.drop_first())
    }
}

pub struct SemanticEndpoint {
    pub ghost key: Seq<u8>,
    pub multiplicity: u64,
}

pub struct EncodedEndpoint {
    pub ghost escaped_key: Seq<u8>,
    pub multiplicity: u64,
}

pub open spec fn endpoint_admitted(endpoint: SemanticEndpoint) -> bool {
    admitted_rpb3_relative_key(endpoint.key) && endpoint.multiplicity > 0
}

pub open spec fn endpoints_strictly_ascending(
    endpoints: Seq<SemanticEndpoint>,
) -> bool
    decreases endpoints.len(),
{
    if endpoints.len() <= 1 {
        true
    } else {
        byte_lex_less(endpoints[0].key, endpoints[1].key)
            && endpoints_strictly_ascending(endpoints.drop_first())
    }
}

/// Exact byte-stuffing operation used before each `00 00` endpoint terminator.
pub open spec fn escape_rpb3_key(key: Seq<u8>) -> Seq<u8>
    decreases key.len(),
{
    if key.len() == 0 {
        Seq::empty()
    } else if key[0] == ESCAPE {
        seq![ESCAPE, ESCAPED_ZERO] + escape_rpb3_key(key.drop_first())
    } else {
        seq![key[0]] + escape_rpb3_key(key.drop_first())
    }
}

/// Inverse for one already-delimited escaped endpoint payload. The outer RCI2
/// cursor removes `00 00` before invoking this logical inverse.
pub open spec fn unescape_rpb3_key(encoded: Seq<u8>) -> Option<Seq<u8>>
    decreases encoded.len(),
{
    if encoded.len() == 0 {
        Some(Seq::empty())
    } else if encoded[0] != ESCAPE {
        match unescape_rpb3_key(encoded.drop_first()) {
            Some(tail) => Some(seq![encoded[0]] + tail),
            None => None,
        }
    } else if encoded.len() < 2 || encoded[1] != ESCAPED_ZERO {
        None
    } else {
        match unescape_rpb3_key(encoded.drop_first().drop_first()) {
            Some(tail) => Some(seq![ESCAPE] + tail),
            None => None,
        }
    }
}

proof fn unescape_nonzero_prefix(
    head: u8,
    encoded_tail: Seq<u8>,
    decoded_tail: Seq<u8>,
)
    requires
        head != ESCAPE,
        unescape_rpb3_key(encoded_tail) == Some(decoded_tail),
    ensures
        unescape_rpb3_key(seq![head] + encoded_tail)
            == Some(seq![head] + decoded_tail),
{
    let prefixed = seq![head] + encoded_tail;
    assert(prefixed.len() > 0);
    assert(prefixed[0] == head);
    assert(prefixed.drop_first() =~= encoded_tail);
    assert(prefixed.drop_first() == encoded_tail);
}

proof fn unescape_zero_prefix(
    encoded_tail: Seq<u8>,
    decoded_tail: Seq<u8>,
)
    requires unescape_rpb3_key(encoded_tail) == Some(decoded_tail),
    ensures
        unescape_rpb3_key(seq![ESCAPE, ESCAPED_ZERO] + encoded_tail)
            == Some(seq![ESCAPE] + decoded_tail),
{
    let prefixed = seq![ESCAPE, ESCAPED_ZERO] + encoded_tail;
    assert(prefixed.len() >= 2);
    assert(prefixed[0] == ESCAPE);
    assert(prefixed[1] == ESCAPED_ZERO);
    assert(prefixed.drop_first().drop_first() =~= encoded_tail);
    assert(prefixed.drop_first().drop_first() == encoded_tail);
}

proof fn escaped_key_round_trip(key: Seq<u8>)
    ensures unescape_rpb3_key(escape_rpb3_key(key)) == Some(key),
    decreases key.len(),
{
    if key.len() > 0 {
        let tail = key.drop_first();
        escaped_key_round_trip(tail);
        assert(seq![key[0]] + tail =~= key);
        assert(seq![key[0]] + tail == key);
        if key[0] == ESCAPE {
            assert(escape_rpb3_key(key)
                == seq![ESCAPE, ESCAPED_ZERO] + escape_rpb3_key(tail));
            unescape_zero_prefix(escape_rpb3_key(tail), tail);
        } else {
            assert(escape_rpb3_key(key)
                == seq![key[0]] + escape_rpb3_key(tail));
            unescape_nonzero_prefix(key[0], escape_rpb3_key(tail), tail);
        }
    }
}

pub open spec fn encode_endpoint(endpoint: SemanticEndpoint) -> EncodedEndpoint {
    EncodedEndpoint {
        escaped_key: escape_rpb3_key(endpoint.key),
        multiplicity: endpoint.multiplicity,
    }
}

pub open spec fn decode_endpoint(frame: EncodedEndpoint) -> Option<SemanticEndpoint> {
    match unescape_rpb3_key(frame.escaped_key) {
        Some(key) => {
            let endpoint = SemanticEndpoint {
                key,
                multiplicity: frame.multiplicity,
            };
            if endpoint_admitted(endpoint) {
                Some(endpoint)
            } else {
                None
            }
        },
        None => None,
    }
}

/// Per-frame refinement begins precisely after the production RPB3 key
/// validator has established `endpoint_admitted`.
proof fn admitted_endpoint_encoder_decoder_refinement(endpoint: SemanticEndpoint)
    requires endpoint_admitted(endpoint),
    ensures decode_endpoint(encode_endpoint(endpoint)) == Some(endpoint),
{
    escaped_key_round_trip(endpoint.key);
}

pub open spec fn encode_endpoints(
    endpoints: Seq<SemanticEndpoint>,
) -> Seq<EncodedEndpoint>
    decreases endpoints.len(),
{
    if endpoints.len() == 0 {
        Seq::empty()
    } else {
        seq![encode_endpoint(endpoints[0])]
            + encode_endpoints(endpoints.drop_first())
    }
}

pub open spec fn decode_endpoints(
    frames: Seq<EncodedEndpoint>,
) -> Option<Seq<SemanticEndpoint>>
    decreases frames.len(),
{
    if frames.len() == 0 {
        Some(Seq::empty())
    } else {
        match decode_endpoint(frames[0]) {
            None => None,
            Some(endpoint) => match decode_endpoints(frames.drop_first()) {
                None => None,
                Some(tail) => Some(seq![endpoint] + tail),
            },
        }
    }
}

proof fn encode_endpoints_preserves_length(endpoints: Seq<SemanticEndpoint>)
    ensures encode_endpoints(endpoints).len() == endpoints.len(),
    decreases endpoints.len(),
{
    if endpoints.len() > 0 {
        encode_endpoints_preserves_length(endpoints.drop_first());
    }
}

proof fn decode_encoded_endpoint_prefix(
    head: SemanticEndpoint,
    tail: Seq<SemanticEndpoint>,
)
    requires
        endpoint_admitted(head),
        decode_endpoints(encode_endpoints(tail)) == Some(tail),
    ensures
        decode_endpoints(seq![encode_endpoint(head)] + encode_endpoints(tail))
            == Some(seq![head] + tail),
{
    admitted_endpoint_encoder_decoder_refinement(head);
    let prefixed = seq![encode_endpoint(head)] + encode_endpoints(tail);
    assert(prefixed.len() > 0);
    assert(prefixed[0] == encode_endpoint(head));
    assert(prefixed.drop_first() =~= encode_endpoints(tail));
    assert(prefixed.drop_first() == encode_endpoints(tail));
}

proof fn admitted_endpoints_encoder_decoder_refinement(
    endpoints: Seq<SemanticEndpoint>,
)
    requires
        forall|index: int| 0 <= index < endpoints.len()
            ==> #[trigger] endpoint_admitted(endpoints[index]),
    ensures decode_endpoints(encode_endpoints(endpoints)) == Some(endpoints),
    decreases endpoints.len(),
{
    if endpoints.len() > 0 {
        admitted_endpoint_encoder_decoder_refinement(endpoints[0]);
        assert forall|index: int| 0 <= index < endpoints.drop_first().len() implies
            #[trigger] endpoint_admitted(endpoints.drop_first()[index]) by {
            assert(endpoints.drop_first()[index] == endpoints[index + 1]);
        }
        admitted_endpoints_encoder_decoder_refinement(endpoints.drop_first());
        decode_encoded_endpoint_prefix(endpoints[0], endpoints.drop_first());
        assert(seq![endpoints[0]] + endpoints.drop_first() =~= endpoints);
        assert(seq![endpoints[0]] + endpoints.drop_first() == endpoints);
    }
}

pub struct SemanticChannel {
    pub ghost endpoints: Seq<SemanticEndpoint>,
    pub connective_used: bool,
}

pub struct EncodedChannel {
    pub ghost frames: Seq<EncodedEndpoint>,
    pub declared_distinct: u64,
    pub connective_used: bool,
}

pub open spec fn semantic_channel_admitted(channel: SemanticChannel) -> bool {
    channel.endpoints.len() <= u64::MAX as int
        && endpoints_strictly_ascending(channel.endpoints)
        && forall|index: int| 0 <= index < channel.endpoints.len()
            ==> #[trigger] endpoint_admitted(channel.endpoints[index])
}

pub open spec fn encode_channel(channel: SemanticChannel) -> EncodedChannel
    recommends semantic_channel_admitted(channel),
{
    EncodedChannel {
        frames: encode_endpoints(channel.endpoints),
        declared_distinct: channel.endpoints.len() as u64,
        connective_used: channel.connective_used,
    }
}

pub open spec fn decode_channel(encoded: EncodedChannel) -> Option<SemanticChannel> {
    if encoded.declared_distinct as int != encoded.frames.len() {
        None
    } else {
        match decode_endpoints(encoded.frames) {
            None => None,
            Some(endpoints) => {
                if endpoints_strictly_ascending(endpoints) {
                    Some(SemanticChannel {
                        endpoints,
                        connective_used: encoded.connective_used,
                    })
                } else {
                    None
                }
            },
        }
    }
}

/// Whole-image refinement proves exact endpoint count, strict endpoint order,
/// nonzero multiplicity admission, escaped-key inversion, and preservation of
/// the root `connective_used` identity bit. The RPB3 key validator is the sole
/// lower-layer boundary; no protobuf or PathMap internals are re-modeled here.
proof fn admitted_channel_encoder_decoder_refinement(channel: SemanticChannel)
    requires semantic_channel_admitted(channel),
    ensures decode_channel(encode_channel(channel)) == Some(channel),
{
    admitted_endpoints_encoder_decoder_refinement(channel.endpoints);
    encode_endpoints_preserves_length(channel.endpoints);
    assert(encode_channel(channel).declared_distinct as int
        == encode_channel(channel).frames.len());
    assert(decode_endpoints(encode_channel(channel).frames)
        == Some(channel.endpoints));
}

/// Concrete witness that the admitted-channel precondition and refinement
/// theorem include a nonempty endpoint as well as the empty channel case.
proof fn admitted_channel_boundary_is_non_vacuous()
    ensures {
        let endpoint = SemanticEndpoint { key: seq![1u8], multiplicity: 1 };
        let channel = SemanticChannel {
            endpoints: seq![endpoint],
            connective_used: true,
        };
        semantic_channel_admitted(channel)
            && decode_channel(encode_channel(channel)) == Some(channel)
    },
{
    let endpoint = SemanticEndpoint { key: seq![1u8], multiplicity: 1 };
    let channel = SemanticChannel {
        endpoints: seq![endpoint],
        connective_used: true,
    };
    admitted_channel_encoder_decoder_refinement(channel);
}

}

fn main() {}
