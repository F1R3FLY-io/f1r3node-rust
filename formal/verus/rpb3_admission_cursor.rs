//! Verus refinement kernel for stack-safe RPB3 admission.
//!
//! The production PDA stores frames in an explicit worklist.  This model
//! captures its scalar invariants: checked regions, strict progress, child
//! balance, exact root exhaustion, and monotone slot admission.
use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
use rpb3_generated_descriptor_projection as generated;

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

/// Fixed32 and Fixed64 consume exactly the width named by their protobuf wire
/// type. The same checked boundary is used for the encoded range and payload
/// range, so no successful field can extend beyond its containing message.
pub fn checked_fixed_width(position: usize, end: usize, fixed64: bool)
    -> (next: Option<usize>)
    requires position <= end,
    ensures
        match next {
            Some(value) => {
                &&& value as int == position as int + (if fixed64 { 8int } else { 4int })
                &&& position < value
                &&& value <= end
            },
            None => position as int + (if fixed64 { 8int } else { 4int }) > end as int,
        },
{
    checked_advance(position, end, if fixed64 { 8 } else { 4 })
}

/// A length-delimited field first rejects a wire `u64` that cannot address
/// this platform, then checks the entire payload against its parent region.
/// This is the arithmetic in `CanonicalFieldCursor::next_field` after the
/// canonical varint has been consumed.
pub fn checked_payload_end(position: usize, end: usize, length: u64)
    -> (next: Option<usize>)
    requires position <= end,
    ensures
        match next {
            Some(value) => {
                &&& length as int <= usize::MAX as int
                &&& value as int == position as int + length as int
                &&& position <= value &&& value <= end
            },
            None => length as int > usize::MAX as int
                || position as int + length as int > end as int,
        },
{
    assert(usize::MAX as int <= u64::MAX as int);
    if length > usize::MAX as u64 {
        None
    } else {
        checked_advance(position, end, length as usize)
    }
}

/// `IndexedCanonicalFieldCursor::next_field_inner` first consumes the key,
/// then one of the four supported wire payloads. Widths are the byte counts
/// established by `checked_varint_byte`; a length payload adds its own
/// canonical length-prefix width before its checked `u64` body length.
pub fn checked_field_end(
    start: usize, end: usize, key_width: usize, wire: u8,
    payload_width: usize, payload_length: u64,
) -> (next: Option<usize>)
    requires start <= end,
    ensures match next {
        Some(value) => {
            &&& start < value <= end
            &&& 1 <= key_width <= 10
            &&& if wire == 0 {
                1 <= payload_width <= 10
                    && value as int == start as int + key_width as int + payload_width as int
            } else if wire == 1 {
                value as int == start as int + key_width as int + 8
            } else if wire == 5 {
                value as int == start as int + key_width as int + 4
            } else {
                wire == 2 && 1 <= payload_width <= 10
                    && payload_length as int <= usize::MAX as int
                    && value as int == start as int + key_width as int
                        + payload_width as int + payload_length as int
            }
        },
        None => true,
    },
{
    if key_width == 0 || key_width > 10 {
        return None;
    }
    let after_key = match checked_advance(start, end, key_width) {
        Some(value) => value,
        None => return None,
    };
    match wire {
        0 => {
            if payload_width == 0 || payload_width > 10 {
                None
            } else {
                checked_advance(after_key, end, payload_width)
            }
        },
        1 => checked_fixed_width(after_key, end, true),
        5 => checked_fixed_width(after_key, end, false),
        2 => {
            if payload_width == 0 || payload_width > 10 {
                return None;
            }
            match checked_advance(after_key, end, payload_width) {
                Some(after_length) => checked_payload_end(after_length, end, payload_length),
                None => None,
            }
        },
        _ => None,
    }
}

pub struct VarintScan {
    pub cursor: usize,
    pub groups: usize,
    pub value: u64,
}

/// One byte of the production ten-group canonical varint loop. A successful
/// step advances the borrowed cursor exactly once. Group ten accepts only a
/// terminal zero-or-one byte, and a noninitial zero terminal is nonminimal.
pub fn checked_varint_byte(state: VarintScan, end: usize, byte: u8)
    -> (next: Option<(VarintScan, bool)>)
    requires state.cursor < end, state.groups < 10,
    ensures
        match next {
            Some((scan, complete)) => {
                &&& scan.cursor as int == state.cursor as int + 1
                &&& scan.cursor <= end
                &&& scan.groups as int == state.groups as int + 1
                &&& scan.groups <= 10
                &&& scan.value == state.value
                    | (((byte & 0x7f) as u64) << ((state.groups * 7) as u32))
                &&& complete == (byte & 0x80 == 0)
            },
            None => (state.groups == 9 && byte > 1)
                || (state.groups > 0 && byte == 0),
        },
{
    if (state.groups == 9 && byte > 1) || (state.groups > 0 && byte == 0) {
        None
    } else {
        let value = state.value | (((byte & 0x7f) as u64) << ((state.groups * 7) as u32));
        Some((VarintScan {
            cursor: state.cursor + 1,
            groups: state.groups + 1,
            value,
        }, byte & 0x80 == 0))
    }
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

/// Generated Prost order is order of resolved *slots*, not field numbers.
/// Repeated fields may occupy the same slot consecutively; two arms of one
/// singular/oneof slot may not. Once a later slot is seen, an earlier slot
/// cannot reappear, so the frontier is sufficient to detect duplicates.
pub open spec fn slot_admissible(
    previous: Option<usize>, current: usize, slot_count: usize, repeated: bool,
) -> bool {
    current < slot_count && match previous {
        None => true,
        Some(last) => current > last || (current == last && repeated),
    }
}

pub fn checked_next_slot(
    previous: Option<usize>, current: usize, slot_count: usize, repeated: bool,
) -> (accepted: bool)
    ensures accepted == slot_admissible(previous, current, slot_count, repeated),
{
    current < slot_count && match previous {
        None => true,
        Some(last) => current > last || (current == last && repeated),
    }
}

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

/// The affine root frame starts as one push. Every `FrameAction::Push` adds
/// one live frame; every `FrameAction::Complete` consumes one. This is the
/// accounting used by the nonrecursive `drive_admission` loop.
pub struct AdmissionStack {
    pub open: usize,
    pub pushed: usize,
    pub completed: usize,
    pub peak: usize,
}

pub open spec fn stack_balanced(stack: AdmissionStack) -> bool {
    stack.pushed as int == stack.completed as int + stack.open as int
        && stack.open <= stack.peak
        && stack.peak <= stack.pushed
}

pub fn root_stack() -> (stack: AdmissionStack)
    ensures stack_balanced(stack), stack.open == 1, stack.completed == 0,
{
    AdmissionStack { open: 1, pushed: 1, completed: 0, peak: 1 }
}

pub fn push_admission_child(stack: AdmissionStack) -> (next: Option<AdmissionStack>)
    requires stack_balanced(stack), stack.open > 0,
    ensures match next {
        Some(value) => {
            &&& stack_balanced(value)
            &&& value.open as int == stack.open as int + 1
            &&& value.pushed as int == stack.pushed as int + 1
            &&& value.completed == stack.completed
            &&& value.peak >= stack.peak
        },
        None => stack.pushed == usize::MAX,
    },
{
    if stack.pushed == usize::MAX {
        None
    } else {
        let open = stack.open + 1;
        let pushed = stack.pushed + 1;
        Some(AdmissionStack {
            open,
            pushed,
            completed: stack.completed,
            peak: if open > stack.peak { open } else { stack.peak },
        })
    }
}

pub fn complete_admission_frame(stack: AdmissionStack) -> (next: AdmissionStack)
    requires stack_balanced(stack), stack.open > 0,
    ensures
        stack_balanced(next),
        next.open as int + 1 == stack.open as int,
        next.completed as int == stack.completed as int + 1,
        next.pushed == stack.pushed,
        next.peak == stack.peak,
        (next.open == 0) == (next.completed == next.pushed),
{
    AdmissionStack {
        open: stack.open - 1,
        pushed: stack.pushed,
        completed: stack.completed + 1,
        peak: stack.peak,
    }
}

/// `push_admission_frame` records both live and allocated frame storage. The
/// number of bytes is accepted only when the exact multiplication fits usize.
pub fn checked_frame_storage(frame_count: usize, frame_bytes: usize)
    -> (storage: Option<usize>)
    requires frame_bytes > 0,
    ensures match storage {
        Some(bytes) => bytes as int == frame_count as int * frame_bytes as int,
        None => frame_count as int * frame_bytes as int > usize::MAX as int,
    },
{
    frame_count.checked_mul(frame_bytes)
}

/// `CanonicalScoreArena::node` starts at two framing bytes and folds child
/// lengths through checked addition. Each inserted entry is later addressed
/// by the prior arena length, never by a reconstructed score byte offset.
pub fn checked_score_node_child(prefix_len: usize, child_len: usize)
    -> (next: Option<usize>)
    requires prefix_len >= 2,
    ensures match next {
        Some(length) => {
            &&& length as int == prefix_len as int + child_len as int
            &&& length >= prefix_len
        },
        None => prefix_len as int + child_len as int > usize::MAX as int,
    },
{
    prefix_len.checked_add(child_len)
}

pub fn checked_arena_handle(entries: usize) -> (next: Option<(usize, usize)>)
    ensures match next {
        Some((handle, new_entries)) => {
            &&& handle == entries
            &&& new_entries as int == entries as int + 1
            &&& handle < new_entries
        },
        None => entries == usize::MAX,
    },
{
    match entries.checked_add(1) {
        Some(new_entries) => Some((entries, new_entries)),
        None => None,
    }
}

/// Incomplete components retain the nonsemantic score lane. A complete
/// component requires its derived semantic score; score-byte equality alone
/// cannot turn an incomplete component into a complete one.
pub fn select_component_score_lane(
    complete: bool, semantic_score: Option<usize>, nonsemantic_score: usize,
) -> (selected: Option<(bool, usize)>)
    ensures match selected {
        Some((lane_complete, handle)) => {
            &&& lane_complete == complete
            &&& if complete { semantic_score == Some(handle) }
                else { handle == nonsemantic_score }
        },
        None => complete && semantic_score.is_none(),
    },
{
    if complete {
        match semantic_score {
            Some(score) => Some((true, score)),
            None => None,
        }
    } else {
        Some((false, nonsemantic_score))
    }
}

/// The production score cursor and claimed RPB3 score advance together. This
/// borrowed-slice refinement returns the first differing position, including
/// the shorter length when one transcript is a strict prefix of the other.
pub fn first_score_difference(expected: &[u8], claimed: &[u8])
    -> (difference: Option<usize>)
    ensures match difference {
        None => {
            &&& expected.len() == claimed.len()
            &&& forall|j: int| 0 <= j < expected.len() as int
                ==> expected@[j] == claimed@[j]
        },
        Some(at) => {
            &&& at <= expected.len() && at <= claimed.len()
            &&& (at == expected.len() || at == claimed.len()
                || expected@[at as int] != claimed@[at as int])
            &&& forall|j: int| 0 <= j < at as int
                ==> expected@[j] == claimed@[j]
        },
    },
{
    let mut at: usize = 0;
    while at < expected.len() && at < claimed.len()
        invariant
            at <= expected.len(),
            at <= claimed.len(),
            forall|j: int| 0 <= j < at as int ==> expected@[j] == claimed@[j],
        decreases expected.len() - at,
    {
        if expected[at] != claimed[at] {
            return Some(at);
        }
        at += 1;
    }
    if at == expected.len() && at == claimed.len() { None } else { Some(at) }
}

/// CanonicalPath's opaque child continuation is affine: at most one nested
/// EPathMap or raw-Par obligation is outstanding in its owning frame. The
/// child's returned summary must match both the saved ID and semantic kind.
pub struct PathObligationState {
    pub started: u64,
    pub resolved: u64,
    pub waiting_id: Option<u64>,
    pub waiting_nested_epm1: bool,
}

pub open spec fn obligation_balanced(state: PathObligationState) -> bool {
    match state.waiting_id {
        None => state.started == state.resolved,
        Some(_) => state.started as int == state.resolved as int + 1,
    }
}

pub fn initial_path_obligations() -> (state: PathObligationState)
    ensures obligation_balanced(state), state.waiting_id.is_none(),
{
    PathObligationState {
        started: 0,
        resolved: 0,
        waiting_id: None,
        waiting_nested_epm1: false,
    }
}

pub fn start_path_obligation(
    state: PathObligationState, id: u64, nested_epm1: bool,
) -> (next: Option<PathObligationState>)
    requires obligation_balanced(state),
    ensures match next {
        Some(value) => {
            &&& obligation_balanced(value)
            &&& value.started as int == state.started as int + 1
            &&& value.resolved == state.resolved
            &&& value.waiting_id == Some(id)
            &&& value.waiting_nested_epm1 == nested_epm1
        },
        None => state.waiting_id.is_some() || state.started == u64::MAX,
    },
{
    if state.waiting_id.is_some() || state.started == u64::MAX {
        None
    } else {
        Some(PathObligationState {
            started: state.started + 1,
            resolved: state.resolved,
            waiting_id: Some(id),
            waiting_nested_epm1: nested_epm1,
        })
    }
}

pub fn resolve_path_obligation(
    state: PathObligationState, id: u64, returned_nested_epm1: bool,
) -> (next: Option<PathObligationState>)
    requires obligation_balanced(state),
    ensures match next {
        Some(value) => {
            &&& obligation_balanced(value)
            &&& value.started == state.started
            &&& value.resolved as int == state.resolved as int + 1
            &&& value.waiting_id.is_none()
        },
        None => state.waiting_id != Some(id)
            || state.waiting_nested_epm1 != returned_nested_epm1,
    },
{
    if state.waiting_id != Some(id) || state.waiting_nested_epm1 != returned_nested_epm1 {
        None
    } else {
        Some(PathObligationState {
            started: state.started,
            resolved: state.resolved + 1,
            waiting_id: None,
            waiting_nested_epm1: false,
        })
    }
}

pub fn path_completion_authorized(state: PathObligationState) -> (authorized: bool)
    requires obligation_balanced(state),
    ensures authorized == (state.waiting_id.is_none()
        && state.started == state.resolved),
{
    state.waiting_id.is_none() && state.started == state.resolved
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

/// A repeated occurrence stays legal; a duplicate singular/oneof arm and a
/// return to an earlier slot are both rejected by the same generated-slot
/// rule used in `MessageFrame::message_field_step`.
pub fn rejects_duplicate_slot(
    previous: usize, current: usize, slot_count: usize, repeated: bool,
) -> (rejected: bool)
    requires previous < slot_count,
    ensures rejected == !slot_admissible(Some(previous), current, slot_count, repeated),
{
    current >= slot_count || current < previous || (current == previous && !repeated)
}

/// The ten component roots selected by `semantic_kind` and the specialized
/// reducers in `protobuf_admission.rs`; Other covers noncomponent heap frames.
pub enum AdmissionFamily {
    Other, Send, Receive, New, Match, Bundle, If, CostTokenStack,
    Expr, Connective, GUnforgeable,
    ReceiveBindFrame, MatchCaseFrame, CostSignatureFrame, CostRegionFrame,
    BigRationalFrame, FixedPointFrame, EVarFrame,
}

pub open spec fn admission_family_program_id(family: AdmissionFamily) -> Option<u16> {
    match family {
        AdmissionFamily::Other => None,
        AdmissionFamily::Send => Some(15),
        AdmissionFamily::Receive => Some(19),
        AdmissionFamily::New => Some(20),
        AdmissionFamily::Match => Some(22),
        AdmissionFamily::Bundle => Some(8),
        AdmissionFamily::If => Some(23),
        AdmissionFamily::CostTokenStack => Some(13),
        AdmissionFamily::Expr => Some(24),
        AdmissionFamily::Connective => Some(55),
        AdmissionFamily::GUnforgeable => Some(60),
        AdmissionFamily::ReceiveBindFrame => Some(16),
        AdmissionFamily::MatchCaseFrame => Some(21),
        AdmissionFamily::CostSignatureFrame => Some(9),
        AdmissionFamily::CostRegionFrame => Some(12),
        AdmissionFamily::BigRationalFrame => Some(25),
        AdmissionFamily::FixedPointFrame => Some(26),
        AdmissionFamily::EVarFrame => Some(35),
    }
}

/// Facts carried by an accepted `FrameSummary`, not a caller-supplied
/// completeness flag. The fields mirror the corresponding production summary
/// variants used by `reduce_component_*` and the specialized reducers.
pub struct AdmissionParReturnImage {
    pub score_handle: usize,
    pub body_id: u64,
    pub ghost body_bytes: Seq<u8>,
    pub semantic_default: bool,
    pub canonical_path_stable: bool,
}

pub enum AdmissionReturnedKind {
    Par { semantic_default: bool, canonical_path_stable: bool },
    Component { family_tag: u32, canonical_path_stable: bool },
    ReceiveBind { source_present: bool },
    MatchCase { required_present: bool, canonical_fixed_point: bool },
    ExprPayload {
        score_present: bool,
        canonical_fixed_point: bool,
        canonical_path_stable: bool,
    },
    BigRational { numerator: Seq<u8>, denominator: Seq<u8> },
    FixedPoint { unscaled: Seq<u8>, scale: u32 },
    CostScore,
    VarScore,
    ConnectiveBody { par_handles: Seq<usize> },
    VarRef { index: i32, depth: i32 },
    Bytes { value: Seq<u8> },
    Unit,
    KeyValuePair {
        key: Option<AdmissionParReturnImage>,
        value_par: Option<AdmissionParReturnImage>,
    },
    Map { key_bytes: Seq<u8>, value_score_handle: Option<usize> },
    EPathMapSnapshot { mode: u8, entries_canonical_path_stable: bool },
    Other,
}

pub struct AdmissionReturnedSummary {
    pub slot: usize,
    pub tag: u32,
    pub body_id: u64,
    pub ghost body_bytes: Seq<u8>,
    pub score_handle: Option<usize>,
    pub score_origin_body_id: Option<u64>,
    pub kind: AdmissionReturnedKind,
}

pub struct AdmissionScalarOccurrence {
    pub slot: usize,
    pub tag: u32,
    pub scalar_kind: u8,
    pub varint_value: u64,
    pub physical_source: usize,
    pub physical_start: int,
    pub ghost bytes: Seq<u8>,
}

pub struct AdmissionFrameState {
    pub ghost physical_source: usize,
    pub physical_start: usize,
    pub program_type_id: u16,
    pub region_start: usize,
    pub region_end: usize,
    pub ghost source_bytes: Seq<u8>,
    pub cursor: usize,
    pub slot_count: usize,
    pub last_slot: Option<usize>,
    pub ghost seen: Set<int>,
    pub ghost required: Set<int>,
    pub ghost scalar_slots: Set<int>,
    pub ghost scalar_occurrences: Seq<AdmissionScalarOccurrence>,
    pub ghost returns: Seq<AdmissionReturnedSummary>,
    pub ghost family: AdmissionFamily,
    pub waiting_id: Option<u64>,
    pub waiting_kind: u8,
    pub waiting_slot: Option<usize>,
    pub waiting_tag: Option<u32>,
    pub return_id: Option<u64>,
    pub frame_kind: u8,
    pub body_id: u64,
    pub score_handle: Option<usize>,
    pub nested_started: u64,
    pub nested_resolved: u64,
}

pub struct AdmissionScoreBinding {
    pub body_id: u64,
    pub semantic: bool,
    pub encoded_len: usize,
    pub ghost transcript_bytes: Seq<u8>,
}

/// `CanonicalScoreArena::node` starts with two framing bytes and folds the
/// encoded lengths of existing child handles. This models the grammar's node
/// constructor; byte and integer constructors require their own encoding
/// relation and are not derived from this node rule.
pub open spec fn admission_node_children_valid(
    arena: Seq<AdmissionScoreBinding>, children: Seq<usize>,
) -> bool {
    forall|i: int| 0 <= i < children.len()
        ==> (children[i] as int) < arena.len()
}

pub open spec fn admission_node_children_length(
    arena: Seq<AdmissionScoreBinding>, children: Seq<usize>,
) -> int
    decreases children.len(),
{
    if children.len() == 0 {
        0
    } else {
        arena[children[0] as int].encoded_len as int
            + admission_node_children_length(
                arena, children.subrange(1, children.len() as int)
            )
    }
}

pub struct AdmissionState {
    pub ghost frames: Seq<AdmissionFrameState>,
    pub ghost arena: Seq<AdmissionScoreBinding>,
    pub root_end: usize,
    pub pushed: usize,
    pub completed: usize,
    pub next_obligation_id: u64,
}

/// Field counts after generated oneof coalescing and minimum-tag Prost order;
/// source: `RhoTypes.proto` plus `match_schema::resolve_protobuf_field`.
pub open spec fn admission_family_slot_count(family: AdmissionFamily) -> Option<usize> {
    match family {
        AdmissionFamily::Other => None,
        AdmissionFamily::Send => Some(6),
        AdmissionFamily::Receive => Some(9),
        AdmissionFamily::New => Some(5),
        AdmissionFamily::Match => Some(4),
        AdmissionFamily::Bundle => Some(3),
        AdmissionFamily::If => Some(5),
        AdmissionFamily::CostTokenStack | AdmissionFamily::Expr
            | AdmissionFamily::Connective | AdmissionFamily::GUnforgeable => Some(1),
        AdmissionFamily::ReceiveBindFrame => Some(5),
        AdmissionFamily::MatchCaseFrame => Some(4),
        AdmissionFamily::CostSignatureFrame => Some(1),
        AdmissionFamily::CostRegionFrame => Some(7),
        AdmissionFamily::BigRationalFrame | AdmissionFamily::FixedPointFrame => Some(2),
        AdmissionFamily::EVarFrame => Some(1),
    }
}

pub open spec fn admission_is_par(kind: AdmissionReturnedKind) -> bool {
    match kind { AdmissionReturnedKind::Par { .. } => true, _ => false }
}

pub open spec fn admission_return_matches(
    family: AdmissionFamily, slot: usize, kind: AdmissionReturnedKind,
) -> bool {
    match family {
        AdmissionFamily::Send => if slot == 0 || slot == 1 { admission_is_par(kind) }
            else if slot == 5 { match kind { AdmissionReturnedKind::CostScore => true, _ => false } }
            else { true },
        AdmissionFamily::Receive => if slot == 0 {
            match kind { AdmissionReturnedKind::ReceiveBind { .. } => true, _ => false }
        } else if slot == 1 || slot == 7 { admission_is_par(kind) }
            else if slot == 8 { match kind { AdmissionReturnedKind::CostScore => true, _ => false } }
            else { true },
        AdmissionFamily::New => if slot == 1 { admission_is_par(kind) }
            else if slot == 3 { match kind { AdmissionReturnedKind::Map { .. } => true, _ => false } }
            else { true },
        AdmissionFamily::Match => if slot == 0 { admission_is_par(kind) }
            else if slot == 1 {
                match kind { AdmissionReturnedKind::MatchCase { .. } => true, _ => false }
            } else { true },
        AdmissionFamily::Bundle => if slot == 0 { admission_is_par(kind) } else { true },
        AdmissionFamily::If => if slot <= 2 { admission_is_par(kind) } else { true },
        AdmissionFamily::CostTokenStack => if slot == 0 {
            match kind { AdmissionReturnedKind::CostScore => true, _ => false }
        } else { true },
        AdmissionFamily::Expr => if slot == 0 {
            match kind {
                AdmissionReturnedKind::ExprPayload { .. }
                | AdmissionReturnedKind::BigRational { .. }
                | AdmissionReturnedKind::FixedPoint { .. } => true,
                _ => false,
            }
        } else { true },
        AdmissionFamily::Connective => if slot == 0 {
            match kind {
                AdmissionReturnedKind::Par { .. }
                | AdmissionReturnedKind::ConnectiveBody { .. }
                | AdmissionReturnedKind::VarRef { .. } => true,
                _ => false,
            }
        } else { true },
        AdmissionFamily::GUnforgeable => if slot == 0 {
            match kind {
                AdmissionReturnedKind::Bytes { .. } | AdmissionReturnedKind::Unit => true,
                _ => false,
            }
        } else { true },
        AdmissionFamily::ReceiveBindFrame => if slot == 0 || slot == 1 {
            admission_is_par(kind)
        } else if slot == 4 {
            match kind { AdmissionReturnedKind::CostScore => true, _ => false }
        } else { true },
        AdmissionFamily::MatchCaseFrame => if slot == 0 || slot == 1 || slot == 3 {
            admission_is_par(kind)
        } else { true },
        AdmissionFamily::CostSignatureFrame
            | AdmissionFamily::CostRegionFrame
            | AdmissionFamily::BigRationalFrame
            | AdmissionFamily::FixedPointFrame => true,
        AdmissionFamily::EVarFrame => if slot == 0 {
            match kind { AdmissionReturnedKind::VarScore => true, _ => false }
        } else { true },
        AdmissionFamily::Other => true,
    }
}

/// These generated message slots cannot use a scalar `Continue` action.
pub open spec fn admission_slot_requires_child(family: AdmissionFamily, slot: usize) -> bool {
    match family {
        AdmissionFamily::Send => slot == 0 || slot == 1 || slot == 5,
        AdmissionFamily::Receive => slot == 0 || slot == 1 || slot == 7 || slot == 8,
        AdmissionFamily::New => slot == 1 || slot == 3,
        AdmissionFamily::Match => slot == 0 || slot == 1,
        AdmissionFamily::Bundle => slot == 0,
        AdmissionFamily::If => slot <= 2,
        AdmissionFamily::CostTokenStack => slot == 0,
        AdmissionFamily::ReceiveBindFrame => slot == 0 || slot == 1 || slot == 2 || slot == 4,
        AdmissionFamily::MatchCaseFrame => slot == 0 || slot == 1 || slot == 3,
        AdmissionFamily::EVarFrame => slot == 0,
        AdmissionFamily::CostSignatureFrame => slot == 0,
        AdmissionFamily::CostRegionFrame => slot == 5,
        _ => false,
    }
}

/// Generated oneof tag classes: Expr immediate scalars use Continue; its
/// message arms use Push. Connective has scalar tags 5..9, while every
/// GUnforgeable arm is a message. No oneof arm is repeated.
pub open spec fn admission_field_tag_valid(
    family: AdmissionFamily, slot: usize, tag: u32, push: bool, repeated: bool,
) -> bool {
    match family {
        AdmissionFamily::Expr if slot == 0 => !repeated && if push {
            (5 <= tag <= 24) || (27 <= tag <= 33) || tag == 36 || tag == 37
        } else {
            (1 <= tag <= 4) || tag == 25 || tag == 34 || tag == 35
        },
        AdmissionFamily::Connective if slot == 0 => !repeated && if push {
            1 <= tag <= 4
        } else { 5 <= tag <= 9 },
        AdmissionFamily::GUnforgeable if slot == 0 =>
            !repeated && push && 1 <= tag <= 4,
        _ => true,
    }
}

pub open spec fn admission_arm_summary_matches(
    family: AdmissionFamily, slot: usize, tag: u32,
    summary: AdmissionReturnedKind,
) -> bool {
    match family {
        AdmissionFamily::Expr if slot == 0 => match tag {
            36 => match summary { AdmissionReturnedKind::BigRational { .. } => true, _ => false },
            37 => match summary { AdmissionReturnedKind::FixedPoint { .. } => true, _ => false },
            _ => match summary { AdmissionReturnedKind::ExprPayload { .. } => true, _ => false },
        },
        AdmissionFamily::Connective if slot == 0 => match tag {
            1 | 2 => match summary { AdmissionReturnedKind::ConnectiveBody { .. } => true, _ => false },
            3 => admission_is_par(summary),
            4 => match summary { AdmissionReturnedKind::VarRef { .. } => true, _ => false },
            _ => false,
        },
        AdmissionFamily::GUnforgeable if slot == 0 => if tag == 4 {
            match summary { AdmissionReturnedKind::Unit => true, _ => false }
        } else {
            match summary { AdmissionReturnedKind::Bytes { .. } => true, _ => false }
        },
        _ => true,
    }
}

pub open spec fn admission_returned_par(frame: AdmissionFrameState, slot: usize) -> bool {
    exists|i: int| 0 <= i < frame.returns.len()
        && frame.returns[i].slot == slot
        && admission_is_par(frame.returns[i].kind)
}

pub open spec fn admission_returned_var(frame: AdmissionFrameState, slot: usize) -> bool {
    exists|i: int| 0 <= i < frame.returns.len()
        && frame.returns[i].slot == slot
        && match frame.returns[i].kind {
            AdmissionReturnedKind::VarScore => true,
            _ => false,
        }
}

pub open spec fn admission_par_return_image(
    returned: AdmissionReturnedSummary, slot: usize, tag: u32,
) -> Option<AdmissionParReturnImage> {
    if returned.slot != slot || returned.tag != tag
        || returned.score_handle.is_none()
        || returned.score_origin_body_id != Some(returned.body_id)
    {
        None
    } else {
        match returned.kind {
            AdmissionReturnedKind::Par {
                semantic_default, canonical_path_stable,
            } => Some(AdmissionParReturnImage {
                score_handle: returned.score_handle.unwrap(),
                body_id: returned.body_id,
                body_bytes: returned.body_bytes,
                semantic_default,
                canonical_path_stable,
            }),
            _ => None,
        }
    }
}

pub open spec fn admission_par_return_image_bound(
    image: AdmissionParReturnImage, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    (image.score_handle as int) < arena.len()
        && arena[image.score_handle as int].body_id == image.body_id
        && arena[image.score_handle as int].semantic
}

pub open spec fn admission_key_value_pair_fields(
    child: AdmissionFrameState,
) -> Option<(Option<AdmissionParReturnImage>, Option<AdmissionParReturnImage>)> {
    if child.family != AdmissionFamily::Other
        || child.program_type_id != 34
        || child.frame_kind != 1
        || child.slot_count != 2
        || child.cursor != child.region_end
        || child.waiting_id.is_some()
        || child.score_handle.is_some()
        || child.scalar_occurrences.len() != 0
        || child.scalar_slots != Set::empty()
    {
        None
    } else if child.returns.len() == 0 {
        if child.last_slot.is_none()
            && !child.seen.contains(0) && !child.seen.contains(1)
        { Some((None, None)) } else { None }
    } else if child.returns.len() == 1 {
        let returned = child.returns[0];
        if returned.slot == 0 && child.last_slot == Some(0)
            && child.seen.contains(0) && !child.seen.contains(1)
        {
            match admission_par_return_image(returned, 0, 1) {
                Some(key) => Some((Some(key), None)),
                None => None,
            }
        } else if returned.slot == 1 && child.last_slot == Some(1)
            && !child.seen.contains(0) && child.seen.contains(1)
        {
            match admission_par_return_image(returned, 1, 2) {
                Some(value) => Some((None, Some(value))),
                None => None,
            }
        } else { None }
    } else if child.returns.len() == 2
        && child.last_slot == Some(1)
        && child.seen.contains(0) && child.seen.contains(1)
    {
        match (
            admission_par_return_image(child.returns[0], 0, 1),
            admission_par_return_image(child.returns[1], 1, 2),
        ) {
            (Some(key), Some(value)) => Some((Some(key), Some(value))),
            _ => None,
        }
    } else { None }
}

pub open spec fn admission_key_value_pair_bindings(
    key: Option<AdmissionParReturnImage>,
    value: Option<AdmissionParReturnImage>,
    arena: Seq<AdmissionScoreBinding>,
) -> bool {
    (match key {
        None => true,
        Some(image) => admission_par_return_image_bound(image, arena),
    }) && (match value {
        None => true,
        Some(image) => admission_par_return_image_bound(image, arena),
    })
}

pub open spec fn admission_opaque_child_edge(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> bool {
    if parent.family != AdmissionFamily::Other
        || parent.frame_kind != 1
        || child.family != AdmissionFamily::Other
        || child.frame_kind != 1
        || child.score_handle.is_some()
    {
        false
    } else {
        let edge = match (
            parent.program_type_id, parent.waiting_slot,
            parent.waiting_tag, child.program_type_id,
        ) {
            (2, Some(0), Some(1), 3)
            | (2, Some(3), Some(5), 14)
            | (18, Some(0), Some(1), 17) => true,
            _ => false,
        };
        edge && match (parent.waiting_slot, parent.waiting_tag) {
            (Some(slot), Some(tag)) => match generated::generated_descriptor(
                parent.program_type_id, tag,
            ) {
                Some(descriptor) => descriptor.slot == slot
                    && descriptor.child_type_id == Some(child.program_type_id)
                    && descriptor.frame_kind == 1,
                None => false,
            },
            _ => false,
        }
    }
}

pub open spec fn admission_epathmap_child_edge(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> bool {
    parent.family == AdmissionFamily::Other
        && parent.frame_kind == 1
        && parent.program_type_id == 31
        && parent.slot_count == 4
        && parent.waiting_slot == Some(3)
        && parent.waiting_tag == Some(9)
        && parent.waiting_kind == 3
        && child.family == AdmissionFamily::Other
        && child.frame_kind == 3
        && child.score_handle.is_none()
        && admission_epathmap_snapshot_descriptor()
}

pub struct AdmissionEMapPairImage {
    pub key: Option<AdmissionParReturnImage>,
    pub value_par: Option<AdmissionParReturnImage>,
    pub ghost body_bytes: Seq<u8>,
}

pub open spec fn admission_emap_pair_from_return(
    returned: AdmissionReturnedSummary, arena: Seq<AdmissionScoreBinding>,
) -> Option<AdmissionEMapPairImage> {
    if returned.slot != 0 || returned.tag != 1
        || returned.score_handle.is_some() || returned.score_origin_body_id.is_some()
    {
        None
    } else {
        match returned.kind {
            AdmissionReturnedKind::KeyValuePair { key, value_par }
                if admission_key_value_pair_bindings(key, value_par, arena) =>
                Some(AdmissionEMapPairImage { key, value_par, body_bytes: returned.body_bytes }),
            _ => None,
        }
    }
}

pub open spec fn admission_emap_pairs_from_returns(
    summaries: Seq<AdmissionReturnedSummary>, arena: Seq<AdmissionScoreBinding>,
) -> Option<Seq<AdmissionEMapPairImage>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else if summaries[0].slot == 3 {
        if summaries[0].tag != 5 || summaries[0].score_handle.is_none()
            || summaries[0].score_origin_body_id != Some(summaries[0].body_id)
            || (summaries[0].score_handle.unwrap() as int) >= arena.len()
            || arena[summaries[0].score_handle.unwrap() as int].body_id != summaries[0].body_id
            || !arena[summaries[0].score_handle.unwrap() as int].semantic
            || !(match summaries[0].kind {
                AdmissionReturnedKind::VarScore => true,
                _ => false,
            })
        {
            None
        } else {
            admission_emap_pairs_from_returns(summaries.drop_first(), arena)
        }
    } else {
        match admission_emap_pair_from_return(summaries[0], arena) {
            None => None,
            Some(pair) => match admission_emap_pairs_from_returns(summaries.drop_first(), arena) {
                None => None,
                Some(rest) => Some(seq![pair] + rest),
            },
        }
    }
}

pub proof fn admission_emap_returned_pairs_have_source_key_identity(
    summaries: Seq<AdmissionReturnedSummary>, arena: Seq<AdmissionScoreBinding>,
)
    requires forall|i: int| 0 <= i < summaries.len()
        ==> admission_returned_par_identity_exists(summaries[i]),
    ensures match admission_emap_pairs_from_returns(summaries, arena) {
        None => true,
        Some(pairs) => forall|i: int| 0 <= i < pairs.len()
            && pairs[i].key.is_some()
            ==> admission_par_identity_from_source(
                pairs[i].key.unwrap().body_bytes,
            ).is_some(),
    },
    decreases summaries.len(),
{
    if summaries.len() > 0 {
        let rest = summaries.drop_first();
        assert forall|i: int| 0 <= i < rest.len()
            implies admission_returned_par_identity_exists(rest[i]) by {
            assert(rest[i] == summaries[i + 1]);
        }
        admission_emap_returned_pairs_have_source_key_identity(rest, arena);
        if summaries[0].slot != 3 {
            if let Some(pair) = admission_emap_pair_from_return(summaries[0], arena) {
                assert(admission_returned_par_identity_exists(summaries[0]));
                if let Some(key) = pair.key {
                    assert(admission_par_identity_from_source(key.body_bytes).is_some());
                }
            }
        }
    }
}

pub open spec fn admission_emap_pair_complete(pair: AdmissionEMapPairImage) -> bool {
    pair.key.is_some() && pair.value_par.is_some()
}

pub open spec fn admission_emap_complete(pairs: Seq<AdmissionEMapPairImage>) -> bool
    decreases pairs.len(),
{
    pairs.len() == 0 || (admission_emap_pair_complete(pairs[0])
        && admission_emap_complete(pairs.drop_first()))
}

pub open spec fn admission_par_wire_field_sequence_at(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32,
) -> bool
    decreases fields.len(),
{
    if fields.len() == 0 {
        cursor as int == bytes.len()
    } else if bytes.len() > usize::MAX as int {
        false
    } else {
        let field = fields[0];
        indexed_field_at_cursor(IndexedCanonicalFieldCursorModel {
            region_start: 0, region_end: bytes.len() as usize,
            next: cursor, source_bytes: bytes,
        }, field)
            && field.number >= previous_tag
            && (previous_tag != 9 || field.number > previous_tag)
            && (previous_tag != 10 || field.number > previous_tag)
            && (((field.number == 1 || field.number == 2
                || 4 <= field.number <= 8 || 11 <= field.number <= 13)
                && field.wire == 2)
                || (field.number == 9 && field.wire == 2 && field.payload_length > 0)
                || (field.number == 10 && field.wire == 0 && field.decoded_varint == 1))
            && admission_par_wire_field_sequence_at(
                bytes, fields.drop_first(), field.encoded_end, field.number,
            )
    }
}

pub open spec fn admission_par_field_rule(
    previous_tag: u32, field: IndexedCanonicalFieldModel,
) -> bool {
    field.number >= previous_tag
        && (previous_tag != 9 || field.number > previous_tag)
        && (previous_tag != 10 || field.number > previous_tag)
        && (((field.number == 1 || field.number == 2
            || 4 <= field.number <= 8 || 11 <= field.number <= 13)
            && field.wire == 2)
            || (field.number == 9 && field.wire == 2 && field.payload_length > 0)
            || (field.number == 10 && field.wire == 0 && field.decoded_varint == 1))
}

pub open spec fn admission_par_wire_prefix_at(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32, stop: usize,
) -> bool
    decreases fields.len(),
{
    if bytes.len() > usize::MAX as int || stop as int > bytes.len() {
        false
    } else if fields.len() == 0 {
        cursor == stop
    } else {
        let field = fields[0];
        indexed_field_at_cursor(IndexedCanonicalFieldCursorModel {
            region_start: 0, region_end: bytes.len() as usize,
            next: cursor, source_bytes: bytes,
        }, field)
            && field.encoded_end <= stop
            && admission_par_field_rule(previous_tag, field)
            && admission_par_wire_prefix_at(
                bytes, fields.drop_first(), field.encoded_end, field.number, stop,
            )
    }
}

pub open spec fn admission_par_last_tag_after(
    fields: Seq<IndexedCanonicalFieldModel>, previous_tag: u32,
) -> u32 {
    if fields.len() == 0 { previous_tag } else { fields.last().number }
}

pub proof fn admission_par_prefix_append_at(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32, stop: usize,
    field: IndexedCanonicalFieldModel,
)
    requires
        admission_par_wire_prefix_at(bytes, fields, cursor, previous_tag, stop),
        indexed_field_at_cursor(IndexedCanonicalFieldCursorModel {
            region_start: 0, region_end: bytes.len() as usize,
            next: stop, source_bytes: bytes,
        }, field),
        admission_par_field_rule(admission_par_last_tag_after(fields, previous_tag), field),
    ensures admission_par_wire_prefix_at(
        bytes, fields.push(field), cursor, previous_tag, field.encoded_end,
    ),
    decreases fields.len(),
{
    if fields.len() == 0 {
        assert(cursor == stop);
        assert(field.encoded_start == stop);
        assert(fields.push(field)[0] == field);
        assert(fields.push(field).drop_first() == Seq::<IndexedCanonicalFieldModel>::empty());
        assert(admission_par_wire_prefix_at(
            bytes, Seq::empty(), field.encoded_end, field.number, field.encoded_end,
        ));
    } else {
        assert(admission_par_wire_prefix_at(
            bytes, fields.drop_first(), fields[0].encoded_end,
            fields[0].number, stop,
        ));
        assert(admission_par_last_tag_after(fields, previous_tag)
            == admission_par_last_tag_after(
                fields.drop_first(), fields[0].number,
            ));
        admission_par_prefix_append_at(
            bytes, fields.drop_first(), fields[0].encoded_end,
            fields[0].number, stop, field,
        );
        assert(fields.push(field).drop_first() == fields.drop_first().push(field));
        assert(fields.push(field)[0] == fields[0]);
        assert(admission_par_wire_prefix_at(
            bytes, fields.drop_first().push(field), fields[0].encoded_end,
            fields[0].number, field.encoded_end,
        ));
    }
    assert(admission_par_wire_prefix_at(
        bytes, fields.push(field), cursor, previous_tag, field.encoded_end,
    ));
}

pub open spec fn admission_par_tag_slot(tag: u32) -> Option<usize> {
    match tag {
        1 => Some(0), 2 => Some(1), 4 => Some(2), 5 => Some(3),
        6 => Some(4), 7 => Some(5), 8 => Some(6), 9 => Some(7),
        10 => Some(8), 11 => Some(9), 12 => Some(10), 13 => Some(11),
        _ => None,
    }
}

pub open spec fn admission_par_slot_tag(slot: usize) -> u32 {
    if slot < 2 { (slot + 1) as u32 } else { (slot + 2) as u32 }
}

pub proof fn admission_par_tag_slot_inverse(tag: u32, slot: usize)
    requires admission_par_tag_slot(tag) == Some(slot),
    ensures
        slot < 12,
        tag == admission_par_slot_tag(slot),
{
    match tag {
        1 | 2 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 => {},
        _ => {},
    }
}

pub proof fn admission_par_prefix_last_tag_has_slot(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32, stop: usize,
)
    requires admission_par_wire_prefix_at(
        bytes, fields, cursor, previous_tag, stop,
    ),
    ensures fields.len() == 0
        || admission_par_tag_slot(fields.last().number).is_some(),
    decreases fields.len(),
{
    if fields.len() > 0 {
        if fields.len() == 1 {
            let tag = fields[0].number;
            assert(admission_par_field_rule(previous_tag, fields[0]));
            assert(admission_par_tag_slot(tag).is_some());
        } else {
            admission_par_prefix_last_tag_has_slot(
                bytes, fields.drop_first(), fields[0].encoded_end,
                fields[0].number, stop,
            );
            assert(fields.last() == fields.drop_first().last());
        }
    }
}

pub proof fn admission_par_generated_field_rule(
    previous_tag: u32, previous_slot: Option<usize>,
    field: IndexedCanonicalFieldModel,
    descriptor: generated::GeneratedDescriptor,
)
    requires
        previous_slot == admission_par_tag_slot(previous_tag),
        previous_tag == 0 || admission_par_tag_slot(previous_tag).is_some(),
        generated::generated_descriptor(1, field.number) == Some(descriptor),
        field.wire == descriptor.wire,
        field.wire != 2 || field.payload_bytes.len() == field.payload_length as int,
        slot_admissible(previous_slot, descriptor.slot, 12, descriptor.repeated),
        descriptor.frame_kind != 0 || scalar_payload_valid(
            field, descriptor.scalar_kind, descriptor.default_forbidden,
        ),
    ensures
        admission_par_tag_slot(field.number) == Some(descriptor.slot),
        admission_par_field_rule(previous_tag, field),
{
    match field.number {
        1 | 2 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 => {},
        _ => {},
    }
    assert(admission_par_tag_slot(field.number) == Some(descriptor.slot));
    admission_par_tag_slot_inverse(field.number, descriptor.slot);
    if let Some(last_slot) = previous_slot {
        admission_par_tag_slot_inverse(previous_tag, last_slot);
        assert(descriptor.slot >= last_slot);
        if descriptor.slot > last_slot {
            if last_slot < 2 && descriptor.slot < 2 {
                assert(field.number > previous_tag);
            } else if last_slot < 2 {
                assert(field.number > previous_tag);
            } else {
                assert(field.number > previous_tag);
            }
        } else {
            assert(descriptor.repeated);
            assert(field.number == previous_tag);
            assert(previous_tag != 9 && previous_tag != 10);
        }
    } else {
        assert(previous_tag == 0);
        assert(field.number > previous_tag);
    }
    match field.number {
        9 => {
            assert(descriptor.scalar_kind == 16);
            assert(descriptor.default_forbidden);
            assert(field.payload_length > 0);
        },
        10 => {
            assert(descriptor.scalar_kind == 1);
            assert(descriptor.default_forbidden);
            assert(field.decoded_varint == 1);
        },
        _ => {},
    }
}

pub open spec fn admission_par_source_prefix_fields(
    frame: AdmissionFrameState, fields: Seq<IndexedCanonicalFieldModel>,
) -> bool {
    frame.program_type_id == 1 && frame.frame_kind == 1
        && frame.family == AdmissionFamily::Other
        && frame.region_start == 0
        && admission_par_wire_prefix_at(
            frame.source_bytes, fields, 0, 0, frame.cursor,
        )
        && frame.last_slot == (if fields.len() == 0 { None }
            else { admission_par_tag_slot(fields.last().number) })
}

pub proof fn admission_par_source_prefix_after_field(
    before: AdmissionFrameState,
    after: AdmissionFrameState,
    fields: Seq<IndexedCanonicalFieldModel>,
    field: IndexedCanonicalFieldModel,
    descriptor: generated::GeneratedDescriptor,
)
    requires
        admission_par_source_prefix_fields(before, fields),
        before.slot_count == 12,
        indexed_field_at_cursor(IndexedCanonicalFieldCursorModel {
            region_start: 0, region_end: before.region_end,
            next: before.cursor, source_bytes: before.source_bytes,
        }, field),
        generated::generated_descriptor(1, field.number) == Some(descriptor),
        field.wire == descriptor.wire,
        slot_admissible(
            before.last_slot, descriptor.slot, before.slot_count, descriptor.repeated,
        ),
        descriptor.frame_kind != 0 || scalar_payload_valid(
            field, descriptor.scalar_kind, descriptor.default_forbidden,
        ),
        after.program_type_id == before.program_type_id,
        after.frame_kind == before.frame_kind,
        after.family == before.family,
        after.region_start == before.region_start,
        after.region_end == before.region_end,
        after.source_bytes == before.source_bytes,
        after.cursor == field.encoded_end,
        after.last_slot == Some(descriptor.slot),
    ensures admission_par_source_prefix_fields(after, fields.push(field)),
{
    let previous_tag = admission_par_last_tag_after(fields, 0);
    admission_par_prefix_last_tag_has_slot(
        before.source_bytes, fields, 0, 0, before.cursor,
    );
    if fields.len() == 0 {
        assert(previous_tag == 0);
        assert(before.last_slot.is_none());
    } else {
        assert(previous_tag == fields.last().number);
        assert(before.last_slot == admission_par_tag_slot(previous_tag));
    }
    admission_par_generated_field_rule(
        previous_tag, before.last_slot, field, descriptor,
    );
    admission_par_prefix_append_at(
        before.source_bytes, fields, 0, 0, before.cursor, field,
    );
}

pub open spec fn admission_par_source_prefix(frame: AdmissionFrameState) -> bool {
    frame.program_type_id != 1 || frame.frame_kind != 1
        || frame.family != AdmissionFamily::Other
        || exists|fields: Seq<IndexedCanonicalFieldModel>|
            admission_par_source_prefix_fields(frame, fields)
}

pub open spec fn admission_par_source_prefix_state(state: AdmissionState) -> bool {
    forall|i: int| 0 <= i < state.frames.len()
        ==> admission_par_source_prefix(state.frames[i])
}

pub open spec fn admission_returned_par_identity_exists(
    returned: AdmissionReturnedSummary,
) -> bool {
    match returned.kind {
        AdmissionReturnedKind::Par { .. } =>
            admission_par_identity_from_source(returned.body_bytes).is_some(),
        AdmissionReturnedKind::KeyValuePair { key, value_par: _ } => match key {
            None => true,
            Some(image) => admission_par_identity_from_source(image.body_bytes).is_some(),
        },
        _ => true,
    }
}

pub open spec fn admission_source_identity_in_returns(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len()
        ==> admission_returned_par_identity_exists(frame.returns[i])
}

pub open spec fn admission_par_source_trace_invariant(state: AdmissionState) -> bool {
    admission_par_source_prefix_state(state)
        && (forall|i: int| 0 <= i < state.frames.len()
            ==> admission_source_identity_in_returns(state.frames[i]))
}

pub proof fn admission_par_empty_frame_has_source_prefix(frame: AdmissionFrameState)
    requires
        frame.program_type_id == 1,
        frame.frame_kind == 1,
        frame.family == AdmissionFamily::Other,
        frame.region_start == 0,
        frame.cursor == 0,
        frame.last_slot.is_none(),
        frame.source_bytes.len() <= usize::MAX as int,
    ensures admission_par_source_prefix(frame),
{
    let fields = Seq::<IndexedCanonicalFieldModel>::empty();
    assert(admission_par_source_prefix_fields(frame, fields));
}

pub proof fn admission_par_complete_prefix_has_parser(
    frame: AdmissionFrameState, fields: Seq<IndexedCanonicalFieldModel>,
)
    requires
        admission_par_source_prefix_fields(frame, fields),
        frame.cursor == frame.region_end,
        frame.source_bytes.len() == frame.region_end as int,
    ensures admission_par_wire_field_sequence(frame.source_bytes, fields),
    decreases fields.len(),
{
    admission_par_prefix_complete_implies_sequence_at(
        frame.source_bytes, fields, 0, 0, frame.cursor,
    );
}

pub proof fn admission_par_complete_prefix_has_identity(frame: AdmissionFrameState)
    requires
        admission_frame_wf(frame),
        admission_par_source_prefix(frame),
        frame.program_type_id == 1,
        frame.frame_kind == 1,
        frame.family == AdmissionFamily::Other,
        frame.cursor == frame.region_end,
    ensures admission_par_identity_from_source(frame.source_bytes).is_some(),
{
    let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
        admission_par_source_prefix_fields(frame, fields);
    admission_par_complete_prefix_has_parser(frame, fields);
}

pub proof fn admission_par_initial_trace_invariant(root: AdmissionFrameState)
    requires admission_root_initial(root),
    ensures admission_par_source_trace_invariant(admission_initial_state(root)),
{
    if root.program_type_id == 1 {
        assert(root.family == AdmissionFamily::Other);
        admission_par_empty_frame_has_source_prefix(root);
    }
}

pub proof fn admission_key_value_pair_return_key_has_source_identity(
    child: AdmissionFrameState,
    key: Option<AdmissionParReturnImage>,
    value_par: Option<AdmissionParReturnImage>,
)
    requires
        admission_key_value_pair_fields(child) == Some((key, value_par)),
        admission_source_identity_in_returns(child),
    ensures match key {
        None => true,
        Some(image) => admission_par_identity_from_source(image.body_bytes).is_some(),
    },
{
    if let Some(image) = key {
        if child.returns.len() == 1 {
            assert(child.returns[0].body_bytes == image.body_bytes);
            assert(admission_returned_par_identity_exists(child.returns[0]));
        } else {
            assert(child.returns.len() == 2);
            assert(child.returns[0].body_bytes == image.body_bytes);
            assert(admission_returned_par_identity_exists(child.returns[0]));
        }
    }
}

pub proof fn admission_completed_child_summary_has_source_identity(
    state: AdmissionState, summary: AdmissionReturnedKind,
)
    requires
        state.frames.len() > 1,
        admission_par_source_trace_invariant(state),
        admission_frame_wf(state.frames.last()),
        state.frames.last().cursor == state.frames.last().region_end,
        admission_child_summary_agrees(state.frames.last(), summary, state.arena),
    ensures match summary {
        AdmissionReturnedKind::Par { .. } =>
            admission_par_identity_from_source(state.frames.last().source_bytes).is_some(),
        AdmissionReturnedKind::KeyValuePair { key, .. } => match key {
            None => true,
            Some(image) => admission_par_identity_from_source(image.body_bytes).is_some(),
        },
        _ => true,
    },
{
    let child = state.frames.last();
    match summary {
        AdmissionReturnedKind::Par { .. } => {
            admission_par_complete_prefix_has_identity(child);
        },
        AdmissionReturnedKind::KeyValuePair { key, value_par } => {
            admission_key_value_pair_return_key_has_source_identity(
                child, key, value_par,
            );
        },
        _ => {},
    }
}

pub proof fn admission_par_prefix_complete_implies_sequence_at(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32, stop: usize,
)
    requires
        stop as int == bytes.len(),
        admission_par_wire_prefix_at(bytes, fields, cursor, previous_tag, stop),
    ensures admission_par_wire_field_sequence_at(bytes, fields, cursor, previous_tag),
    decreases fields.len(),
{
    if fields.len() > 0 {
        let field = fields[0];
        admission_par_prefix_complete_implies_sequence_at(
            bytes, fields.drop_first(), field.encoded_end, field.number, stop,
        );
    }
}

pub open spec fn admission_par_wire_field_sequence(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
) -> bool {
    admission_par_wire_field_sequence_at(bytes, fields, 0, 0)
}

pub open spec fn admission_par_locally_free_range(
    fields: Seq<IndexedCanonicalFieldModel>,
) -> Option<(usize, usize)>
    decreases fields.len(),
{
    if fields.len() == 0 { None }
    else if fields[0].number == 9 {
        Some((fields[0].encoded_start, fields[0].encoded_end))
    } else { admission_par_locally_free_range(fields.drop_first()) }
}

pub open spec fn admission_par_identity_from_fields(
    bytes: Seq<u8>, fields: Seq<IndexedCanonicalFieldModel>,
) -> Seq<u8> {
    match admission_par_locally_free_range(fields) {
        None => bytes,
        Some((start, end)) => bytes.subrange(0, start as int)
            + bytes.subrange(end as int, bytes.len() as int),
    }
}

pub open spec fn admission_par_identity_from_source(bytes: Seq<u8>) -> Option<Seq<u8>> {
    if exists|fields: Seq<IndexedCanonicalFieldModel>|
        admission_par_wire_field_sequence(bytes, fields)
    {
        let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
            admission_par_wire_field_sequence(bytes, fields);
        Some(admission_par_identity_from_fields(bytes, fields))
    } else { None }
}

pub open spec fn admission_emap_pair_score_order_lt(
    left: AdmissionEMapPairImage, right: AdmissionEMapPairImage,
    arena: Seq<AdmissionScoreBinding>,
) -> bool {
    if !admission_emap_pair_complete(left) || !admission_emap_pair_complete(right)
        || left.key.unwrap().score_handle as int >= arena.len()
        || right.key.unwrap().score_handle as int >= arena.len()
    {
        false
    } else {
        let left_score = arena[left.key.unwrap().score_handle as int].transcript_bytes;
        let right_score = arena[right.key.unwrap().score_handle as int].transcript_bytes;
        admission_bytes_lex_lt(left_score, right_score)
            || (left_score == right_score
                && admission_bytes_lex_lt(left.body_bytes, right.body_bytes))
    }
}

pub open spec fn admission_emap_source_fixed_point(
    pairs: Seq<AdmissionEMapPairImage>, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    (forall|i: int, j: int| 0 <= i < j < pairs.len()
        && admission_emap_pair_complete(pairs[i])
        && admission_emap_pair_complete(pairs[j])
        && (forall|k: int| i < k < j ==> !admission_emap_pair_complete(pairs[k]))
        ==> admission_emap_pair_score_order_lt(pairs[i], pairs[j], arena))
        && (forall|i: int, j: int| 0 <= i < j < pairs.len()
            && admission_emap_pair_complete(pairs[i])
            && admission_emap_pair_complete(pairs[j])
            ==> match (
                admission_par_identity_from_source(pairs[i].key.unwrap().body_bytes),
                admission_par_identity_from_source(pairs[j].key.unwrap().body_bytes),
            ) {
                (Some(left), Some(right)) => left != right,
                _ => false,
            })
}

pub open spec fn admission_emap_source_summary(
    frame: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>,
) -> Option<(bool, bool)> {
    if frame.family != AdmissionFamily::Other || frame.frame_kind != 1
        || frame.program_type_id != 30 || frame.slot_count != 4
        || frame.cursor != frame.region_end || frame.waiting_id.is_some()
    {
        None
    } else {
        match admission_emap_pairs_from_returns(frame.returns, arena) {
            None => None,
            Some(pairs) if frame.seen.contains(0) == (pairs.len() > 0)
                && frame.returns.len() == pairs.len()
                    + (if frame.seen.contains(3) { 1int } else { 0int })
                && (forall|i: int| 0 <= i < frame.returns.len()
                    && frame.returns[i].slot == 3 ==> frame.returns[i].tag == 5)
                && frame.seen.contains(3) == (exists|i: int| 0 <= i < frame.returns.len()
                    && frame.returns[i].slot == 3) => Some((
                        admission_emap_complete(pairs),
                        admission_emap_source_fixed_point(pairs, arena),
                    )),
            _ => None,
        }
    }
}

pub open spec fn admission_eset_return_bound(
    returned: AdmissionReturnedSummary, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    returned.score_handle.is_some()
        && returned.score_origin_body_id == Some(returned.body_id)
        && (returned.score_handle.unwrap() as int) < arena.len()
        && arena[returned.score_handle.unwrap() as int].body_id == returned.body_id
        && arena[returned.score_handle.unwrap() as int].semantic
        && (if returned.slot == 0 {
            returned.tag == 1 && admission_is_par(returned.kind)
        } else if returned.slot == 3 {
            returned.tag == 5
                && match returned.kind {
                    AdmissionReturnedKind::VarScore => true,
                    _ => false,
                }
        } else { false })
}

pub open spec fn admission_eset_source_fixed_point(
    returns: Seq<AdmissionReturnedSummary>, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    (forall|i: int| 0 <= i < returns.len()
        ==> admission_eset_return_bound(returns[i], arena))
        && (forall|i: int, j: int| 0 <= i < j < returns.len()
        && returns[i].slot == 0 && returns[j].slot == 0
        ==> {
            let left_score = arena[returns[i].score_handle.unwrap() as int].transcript_bytes;
            let right_score = arena[returns[j].score_handle.unwrap() as int].transcript_bytes;
            admission_bytes_lex_lt(left_score, right_score)
                || (left_score == right_score
                    && admission_bytes_lex_lt(
                        returns[i].body_bytes, returns[j].body_bytes,
                    ))
        })
        && (forall|i: int, j: int| 0 <= i < j < returns.len()
            && returns[i].slot == 0 && returns[j].slot == 0
            ==> match (
                admission_par_identity_from_source(returns[i].body_bytes),
                admission_par_identity_from_source(returns[j].body_bytes),
            ) {
                (Some(left), Some(right)) => left != right,
                _ => false,
            })
}

pub open spec fn admission_eset_source_summary(
    frame: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>,
) -> Option<bool> {
    if frame.family != AdmissionFamily::Other || frame.frame_kind != 1
        || frame.program_type_id != 29 || frame.slot_count != 4
        || frame.cursor != frame.region_end || frame.waiting_id.is_some()
        || frame.score_handle.is_none()
        || (frame.score_handle.unwrap() as int) >= arena.len()
        || arena[frame.score_handle.unwrap() as int].body_id != frame.body_id
        || !arena[frame.score_handle.unwrap() as int].semantic
        || !(forall|i: int| 0 <= i < frame.returns.len()
            ==> admission_eset_return_bound(frame.returns[i], arena))
        || frame.seen.contains(0) != (exists|i: int| 0 <= i < frame.returns.len()
            && frame.returns[i].slot == 0)
        || frame.seen.contains(3) != (exists|i: int| 0 <= i < frame.returns.len()
            && frame.returns[i].slot == 3)
        || !(forall|i: int| 0 <= i < frame.returns.len()
            && frame.returns[i].slot == 3 ==> i == frame.returns.len() - 1)
    {
        None
    } else {
        Some(admission_eset_source_fixed_point(frame.returns, arena))
    }
}

pub open spec fn admission_ezipper_pathmap_payload(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> Option<AdmissionReturnedKind> {
    if parent.family != AdmissionFamily::Other || parent.frame_kind != 1
        || parent.program_type_id != 32 || parent.slot_count != 6
        || parent.waiting_slot != Some(0) || parent.waiting_tag != Some(1)
        || child.family != AdmissionFamily::Other || child.frame_kind != 1
        || child.program_type_id != 31 || child.score_handle.is_none()
        || !match generated::generated_descriptor(32, 1) {
            Some(descriptor) => descriptor.slot == 0 && descriptor.wire == 2
                && descriptor.child_type_id == Some(31) && descriptor.frame_kind == 1,
            None => false,
        }
    {
        None
    } else {
        match admission_epathmap_path_stability(child) {
            Some(canonical_path_stable) => Some(AdmissionReturnedKind::ExprPayload {
                score_present: true, canonical_fixed_point: true,
                canonical_path_stable,
            }),
            None => None,
        }
    }
}

pub open spec fn admission_ezipper_expr_payload(
    child: AdmissionFrameState,
) -> Option<AdmissionReturnedKind> {
    if child.family != AdmissionFamily::Other || child.frame_kind != 1
        || child.program_type_id != 32 || child.slot_count != 6
        || !match generated::generated_descriptor(24, 33) {
            Some(descriptor) => descriptor.slot == 0 && descriptor.wire == 2
                && descriptor.child_type_id == Some(32) && descriptor.frame_kind == 1,
            None => false,
        }
    {
        None
    } else if !child.seen.contains(0) && child.returns.len() == 0
        && child.score_handle.is_none()
    {
        Some(AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        })
    } else if !child.seen.contains(0) || child.score_handle.is_none()
        || child.returns.len() != 1 || child.returns[0].slot != 0
        || child.returns[0].tag != 1 || child.returns[0].score_handle.is_none()
        || !match child.returns[0].kind {
            AdmissionReturnedKind::ExprPayload {
                score_present, canonical_fixed_point, ..
            } => score_present && canonical_fixed_point,
            _ => false,
        }
    {
        None
    } else {
        Some(AdmissionReturnedKind::ExprPayload {
            score_present: true, canonical_fixed_point: true,
            canonical_path_stable: false,
        })
    }
}

pub open spec fn admission_expr_structural_payload_with_arena(
    parent: AdmissionFrameState, child: AdmissionFrameState,
    arena: Seq<AdmissionScoreBinding>,
) -> Option<AdmissionReturnedKind> {
    if parent.family == AdmissionFamily::Expr && parent.waiting_slot == Some(0)
        && parent.waiting_tag == Some(22) && parent.program_type_id == 24
        && child.family == AdmissionFamily::Other && child.frame_kind == 1
        && child.program_type_id == 29
    {
        match admission_eset_source_summary(child, arena) {
            Some(fixed) => Some(AdmissionReturnedKind::ExprPayload {
                score_present: true, canonical_fixed_point: fixed,
                canonical_path_stable: false,
            }),
            None => None,
        }
    } else if parent.family == AdmissionFamily::Expr && parent.waiting_slot == Some(0)
        && parent.waiting_tag == Some(23) && parent.program_type_id == 24
        && child.family == AdmissionFamily::Other && child.frame_kind == 1
        && child.program_type_id == 30
    {
        match admission_emap_source_summary(child, arena) {
            Some((complete, fixed)) => Some(AdmissionReturnedKind::ExprPayload {
                score_present: complete, canonical_fixed_point: fixed,
                canonical_path_stable: false,
            }),
            None => None,
        }
    } else { admission_expr_structural_payload(parent, child) }
}

pub open spec fn admission_expr_structural_payload(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> Option<AdmissionReturnedKind> {
    if parent.family != AdmissionFamily::Expr
        || parent.waiting_slot != Some(0)
        || child.family != AdmissionFamily::Other
        || child.frame_kind != 1
    {
        None
    } else {
        match parent.waiting_tag {
            None => None,
            Some(tag) => match generated::generated_descriptor(parent.program_type_id, tag) {
                Some(descriptor) if descriptor.frame_kind == 1
                    && descriptor.child_type_id == Some(child.program_type_id) => {
                    if tag == 5 || tag == 6 {
                        Some(AdmissionReturnedKind::ExprPayload {
                            score_present: admission_returned_par(child, 0),
                            canonical_fixed_point: true,
                            canonical_path_stable: false,
                        })
                    } else if (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31) {
                        Some(AdmissionReturnedKind::ExprPayload {
                            score_present: admission_returned_par(child, 0)
                                && admission_returned_par(child, 1),
                            canonical_fixed_point: true,
                            canonical_path_stable: false,
                        })
                    } else if tag == 24 {
                        Some(AdmissionReturnedKind::ExprPayload {
                            score_present: admission_returned_par(child, 1),
                            canonical_fixed_point: true,
                            canonical_path_stable: false,
                        })
                    } else if (tag == 20 && child.program_type_id == 27)
                        || (tag == 21 && child.program_type_id == 28)
                    {
                        match admission_ordered_collection_path_stability(child) {
                            Some(canonical_path_stable) =>
                                Some(AdmissionReturnedKind::ExprPayload {
                                    score_present: true,
                                    canonical_fixed_point: true,
                                    canonical_path_stable,
                                }),
                            None => None,
                        }
                    } else if tag == 32 && child.program_type_id == 31 {
                        match admission_epathmap_path_stability(child) {
                            Some(canonical_path_stable) =>
                                Some(AdmissionReturnedKind::ExprPayload {
                                    score_present: true,
                                    canonical_fixed_point: true,
                                    canonical_path_stable,
                                }),
                            None => None,
                        }
                    } else if tag == 33 && child.program_type_id == 32 {
                        admission_ezipper_expr_payload(child)
                    } else {
                        None
                    }
                },
                _ => None,
            },
        }
    }
}

pub open spec fn admission_map_summary_complete(frame: AdmissionFrameState) -> bool {
    frame.frame_kind == 2
        && frame.family == AdmissionFamily::Other
        && frame.program_type_id == 1
        && frame.slot_count == 2
        && frame.cursor == frame.region_end
        && frame.waiting_id.is_none()
        && frame.seen.contains(0) == frame.scalar_slots.contains(0)
        && (!frame.seen.contains(1) ==> frame.last_slot == (
            if frame.seen.contains(0) { Some(0) } else { None }
        ))
        && frame.returns.len() == (if frame.seen.contains(1) { 1int } else { 0int })
        && exists|key_bytes: Seq<u8>| admission_map_source_matches(frame, key_bytes)
        && (frame.seen.contains(1) ==> {
            &&& frame.last_slot == Some(1)
            &&& frame.returns[0].slot == 1
            &&& frame.returns[0].tag == 2
            &&& frame.returns[0].score_handle.is_some()
            &&& match frame.returns[0].kind {
                AdmissionReturnedKind::Par { semantic_default, .. } => !semantic_default,
                _ => false,
            }
        })
}

pub open spec fn admission_raw_par_has_components(frame: AdmissionFrameState) -> bool {
    exists|i: int| 0 <= i < frame.returns.len()
        && #[trigger] admission_is_component(frame.returns[i].kind)
}

pub open spec fn admission_is_component(kind: AdmissionReturnedKind) -> bool {
    match kind {
        AdmissionReturnedKind::Component { .. } => true,
        _ => false,
    }
}

pub open spec fn admission_raw_par_connective_used(frame: AdmissionFrameState) -> bool {
    exists|i: int| 0 <= i < frame.scalar_occurrences.len()
        && frame.scalar_occurrences[i].slot == 8
        && frame.scalar_occurrences[i].tag == 10
        && frame.scalar_occurrences[i].scalar_kind == 1
        && frame.scalar_occurrences[i].varint_value == 1
}

pub open spec fn admission_raw_par_semantic_default(frame: AdmissionFrameState) -> bool {
    !admission_raw_par_has_components(frame)
        && !admission_raw_par_connective_used(frame)
}

pub open spec fn admission_raw_par_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    if frame.program_type_id != 1 || frame.family != AdmissionFamily::Other {
        None
    } else if frame.returns.len() != 1
        || frame.seen.contains(7)
        || admission_raw_par_connective_used(frame)
    {
        Some(false)
    } else {
        match frame.returns[0].kind {
            AdmissionReturnedKind::Component { canonical_path_stable, .. } =>
                Some(canonical_path_stable),
            _ => None,
        }
    }
}

pub open spec fn admission_ordered_collection_par_returns_typed(
    frame: AdmissionFrameState,
) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 0
        ==> frame.returns[i].tag == 1
            && match frame.returns[i].kind {
                AdmissionReturnedKind::Par { .. } => true,
                _ => false,
            }
}

pub open spec fn admission_ordered_collection_all_par_stable(
    frame: AdmissionFrameState,
) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 0
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::Par { canonical_path_stable, .. } =>
                canonical_path_stable,
            _ => false,
        }
}

pub open spec fn admission_ordered_collection_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    if frame.frame_kind != 1 || frame.family != AdmissionFamily::Other
        || (frame.program_type_id != 27 && frame.program_type_id != 28)
        || generated::generated_slot_count(frame.program_type_id) != Some(frame.slot_count)
        || !admission_ordered_collection_par_returns_typed(frame)
    {
        None
    } else {
        Some(!frame.seen.contains(1)
            && !frame.seen.contains(2)
            && (frame.program_type_id != 27 || !frame.seen.contains(3))
            && admission_ordered_collection_all_par_stable(frame))
    }
}

pub open spec fn admission_epathmap_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    if frame.frame_kind != 1 || frame.family != AdmissionFamily::Other
        || frame.program_type_id != 31 || frame.slot_count != 4
        || !admission_epathmap_snapshot_descriptor()
        || frame.returns.len() != (if frame.seen.contains(2) { 2int } else { 1int })
        || !frame.seen.contains(3)
        || (frame.seen.contains(2) && !(
            frame.returns[0].slot == 2
                && frame.returns[0].tag == 5
                && match frame.returns[0].kind {
                    AdmissionReturnedKind::VarScore => true,
                    _ => false,
                }
        ))
        || frame.returns.last().slot != 3 || frame.returns.last().tag != 9
    {
        None
    } else {
        match frame.returns.last().kind {
            AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            } if mode <= 2 => Some(
                !frame.seen.contains(0)
                    && !frame.seen.contains(1)
                    && !frame.seen.contains(2)
                    && entries_canonical_path_stable,
            ),
            _ => None,
        }
    }
}

pub open spec fn admission_epathmap_snapshot_descriptor() -> bool {
    generated::generated_slot_count(31) == Some(4)
        && match generated::generated_descriptor(31, 9) {
            Some(descriptor) => descriptor.slot == 3
                && !descriptor.repeated
                && descriptor.wire == 2
                && descriptor.scalar_kind == 0
                && descriptor.frame_kind == 3,
            None => false,
        }
}

pub proof fn admission_empty_ordered_collection_path_stable(
    frame: AdmissionFrameState,
)
    requires
        frame.frame_kind == 1,
        frame.family == AdmissionFamily::Other,
        frame.program_type_id == 27 || frame.program_type_id == 28,
        generated::generated_slot_count(frame.program_type_id) == Some(frame.slot_count),
        frame.returns.len() == 0,
        !frame.seen.contains(1),
        !frame.seen.contains(2),
        frame.program_type_id != 27 || !frame.seen.contains(3),
    ensures admission_ordered_collection_path_stability(frame) == Some(true),
{}

pub open spec fn admission_expr_payload_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    if frame.frame_kind != 1 {
        None
    } else if frame.program_type_id == 27 || frame.program_type_id == 28 {
        admission_ordered_collection_path_stability(frame)
    } else if frame.program_type_id == 31 {
        admission_epathmap_path_stability(frame)
    } else if frame.program_type_id == 29
        || frame.program_type_id == 30
        || frame.program_type_id == 32
        || frame.program_type_id == 33
        || (35 <= frame.program_type_id && frame.program_type_id <= 54)
    {
        Some(false)
    } else {
        None
    }
}

pub open spec fn admission_expr_component_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    if frame.family != AdmissionFamily::Expr || frame.program_type_id != 24 {
        None
    } else if frame.returns.len() == 0 {
        if !frame.seen.contains(0) {
            Some(false)
        } else if frame.scalar_occurrences.len() == 1 {
            match frame.scalar_occurrences[0].tag {
                1 | 2 | 3 | 4 | 25 | 34 | 35 => Some(true),
                _ => None,
            }
        } else {
            None
        }
    } else if frame.returns.len() == 1 {
        let returned = frame.returns[0];
        match returned.tag {
            5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17
            | 18 | 19 | 22 | 23 | 24 | 27 | 28 | 29 | 30 | 31 | 33 =>
                match returned.kind {
                    AdmissionReturnedKind::ExprPayload { .. } => Some(false),
                    _ => None,
                },
            36 => match returned.kind {
                AdmissionReturnedKind::BigRational { .. } => Some(true),
                _ => None,
            },
            37 => match returned.kind {
                AdmissionReturnedKind::FixedPoint { .. } => Some(true),
                _ => None,
            },
            20 | 21 | 32 => match returned.kind {
                AdmissionReturnedKind::ExprPayload {
                    score_present,
                    canonical_path_stable,
                    ..
                } => Some(score_present && canonical_path_stable),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    }
}

pub open spec fn admission_component_path_stability(
    frame: AdmissionFrameState,
) -> Option<bool> {
    match frame.family {
        AdmissionFamily::Send | AdmissionFamily::Receive | AdmissionFamily::New
        | AdmissionFamily::Match | AdmissionFamily::Bundle | AdmissionFamily::If
        | AdmissionFamily::CostTokenStack | AdmissionFamily::Connective => Some(false),
        AdmissionFamily::GUnforgeable if frame.program_type_id == 60 => {
            if frame.returns.len() == 0 {
                Some(false)
            } else if frame.returns.len() == 1 {
                match (frame.returns[0].tag, frame.returns[0].kind) {
                    (1, AdmissionReturnedKind::Bytes { .. }) => Some(true),
                    (2 | 3, AdmissionReturnedKind::Bytes { .. }) => Some(false),
                    (4, AdmissionReturnedKind::Unit) => Some(false),
                    _ => None,
                }
            } else {
                None
            }
        },
        AdmissionFamily::Expr => admission_expr_component_path_stability(frame),
        _ => None,
    }
}

pub open spec fn admission_raw_par_component_child(
    tag: u32, child: AdmissionFrameState,
) -> bool {
    match tag {
        1 => child.family == AdmissionFamily::Send && child.program_type_id == 15,
        2 => child.family == AdmissionFamily::Receive && child.program_type_id == 19,
        4 => child.family == AdmissionFamily::New && child.program_type_id == 20,
        5 => child.family == AdmissionFamily::Expr && child.program_type_id == 24,
        6 => child.family == AdmissionFamily::Match && child.program_type_id == 22,
        7 => child.family == AdmissionFamily::GUnforgeable && child.program_type_id == 60,
        8 => child.family == AdmissionFamily::Connective && child.program_type_id == 55,
        11 => child.family == AdmissionFamily::Bundle && child.program_type_id == 8,
        12 => child.family == AdmissionFamily::If && child.program_type_id == 23,
        13 => child.family == AdmissionFamily::CostTokenStack && child.program_type_id == 13,
        _ => false,
    }
}

pub open spec fn admission_all_receive_sources(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 0
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::ReceiveBind { source_present } => source_present,
            _ => false,
        }
}

pub open spec fn admission_all_match_cases_required(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 1
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::MatchCase { required_present, .. } => required_present,
            _ => false,
        }
}

pub open spec fn admission_expr_has_score(frame: AdmissionFrameState) -> bool {
    exists|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 0
        && match frame.returns[i].kind {
            AdmissionReturnedKind::BigRational { .. }
            | AdmissionReturnedKind::FixedPoint { .. } => true,
            AdmissionReturnedKind::ExprPayload { score_present, .. } => score_present,
            _ => false,
        }
}

/// Production completeness is determined by the reducer family and typed
/// summaries. Wire-required fields are checked independently below.
pub open spec fn admission_family_complete(frame: AdmissionFrameState) -> bool {
    match frame.family {
        AdmissionFamily::Send => admission_returned_par(frame, 0),
        AdmissionFamily::Receive => admission_returned_par(frame, 1)
            && admission_all_receive_sources(frame),
        AdmissionFamily::New => admission_returned_par(frame, 1),
        AdmissionFamily::Match => admission_returned_par(frame, 0)
            && admission_all_match_cases_required(frame),
        AdmissionFamily::Bundle => admission_returned_par(frame, 0),
        AdmissionFamily::If => admission_returned_par(frame, 0)
            && admission_returned_par(frame, 1) && admission_returned_par(frame, 2),
        AdmissionFamily::CostTokenStack => true,
        AdmissionFamily::Expr => !frame.seen.contains(0)
            || frame.scalar_slots.contains(0) || admission_expr_has_score(frame),
        AdmissionFamily::Connective | AdmissionFamily::GUnforgeable | AdmissionFamily::Other => true,
        AdmissionFamily::ReceiveBindFrame => admission_returned_par(frame, 1),
        AdmissionFamily::MatchCaseFrame => admission_returned_par(frame, 0)
            && admission_returned_par(frame, 1),
        AdmissionFamily::CostSignatureFrame
            | AdmissionFamily::CostRegionFrame
            | AdmissionFamily::BigRationalFrame
            | AdmissionFamily::FixedPointFrame => true,
        AdmissionFamily::EVarFrame => admission_returned_var(frame, 0),
    }
}

pub open spec fn admission_returned_default_par(
    frame: AdmissionFrameState, slot: usize,
) -> bool {
    exists|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == slot
        && match frame.returns[i].kind {
            AdmissionReturnedKind::Par { semantic_default, .. } => semantic_default,
            _ => false,
        }
}

pub open spec fn admission_summary_has_score(summary: AdmissionReturnedKind) -> bool {
    match summary {
        AdmissionReturnedKind::Par { .. } | AdmissionReturnedKind::Component { .. }
            | AdmissionReturnedKind::CostScore
            | AdmissionReturnedKind::VarScore => true,
        AdmissionReturnedKind::ReceiveBind { source_present } => source_present,
        AdmissionReturnedKind::MatchCase { required_present, .. } => required_present,
        AdmissionReturnedKind::ExprPayload { score_present, .. } => score_present,
        _ => false,
    }
}

pub open spec fn admission_bytes_leaf_payload(
    child: AdmissionFrameState,
) -> Option<Seq<u8>> {
    if child.frame_kind != 1
        || (child.program_type_id != 61 && child.program_type_id != 62
            && child.program_type_id != 63)
        || child.slot_count != 1
    {
        None
    } else if child.scalar_occurrences.len() == 0
        && !child.seen.contains(0)
        && !child.scalar_slots.contains(0)
    {
        Some(Seq::empty())
    } else if child.scalar_occurrences.len() == 1
        && child.seen.contains(0)
        && child.scalar_slots.contains(0)
        && child.scalar_occurrences[0].slot == 0
        && child.scalar_occurrences[0].tag == 1
        && child.scalar_occurrences[0].scalar_kind == 16
        && child.scalar_occurrences[0].bytes.len() > 0
    {
        Some(child.scalar_occurrences[0].bytes)
    } else {
        None
    }
}

pub open spec fn admission_sint32_value(encoded: u64) -> i32 {
    let word = encoded as u32;
    let shifted = (word >> 1) as i32;
    if word & 1 == 0 { shifted } else { (-1int - shifted as int) as i32 }
}

pub open spec fn admission_var_ref_slot_value(
    child: AdmissionFrameState, slot: usize,
) -> Option<i32> {
    if slot >= 2 {
        None
    } else if !child.seen.contains(slot as int) {
        if exists|i: int| 0 <= i < child.scalar_occurrences.len()
            && child.scalar_occurrences[i].slot == slot
        { None } else { Some(0) }
    } else if child.scalar_slots.contains(slot as int)
        && exists|i: int| 0 <= i < child.scalar_occurrences.len()
            && child.scalar_occurrences[i].slot == slot
            && child.scalar_occurrences[i].tag == slot as u32 + 1
            && child.scalar_occurrences[i].scalar_kind == 3
            && 0 < child.scalar_occurrences[i].varint_value <= u32::MAX as u64
            && forall|j: int| 0 <= j < child.scalar_occurrences.len()
                && child.scalar_occurrences[j].slot == slot ==> j == i
    {
        let i = choose|i: int| 0 <= i < child.scalar_occurrences.len()
            && child.scalar_occurrences[i].slot == slot
            && child.scalar_occurrences[i].tag == slot as u32 + 1
            && child.scalar_occurrences[i].scalar_kind == 3
            && 0 < child.scalar_occurrences[i].varint_value <= u32::MAX as u64
            && forall|j: int| 0 <= j < child.scalar_occurrences.len()
                && child.scalar_occurrences[j].slot == slot ==> j == i;
        Some(admission_sint32_value(child.scalar_occurrences[i].varint_value))
    } else {
        None
    }
}

pub open spec fn admission_var_ref_fields(
    child: AdmissionFrameState,
) -> Option<(i32, i32)> {
    if child.frame_kind != 1 || child.program_type_id != 56 || child.slot_count != 2 {
        None
    } else {
        match (
            admission_var_ref_slot_value(child, 0),
            admission_var_ref_slot_value(child, 1),
        ) {
            (Some(index), Some(depth)) => Some((index, depth)),
            _ => None,
        }
    }
}

pub open spec fn admission_unique_scalar_occurrence(
    child: AdmissionFrameState, slot: usize, tag: u32, scalar_kind: u8,
) -> Option<AdmissionScalarOccurrence> {
    if !child.seen.contains(slot as int)
        || !child.scalar_slots.contains(slot as int)
        || !(exists|i: int| 0 <= i < child.scalar_occurrences.len()
            && child.scalar_occurrences[i].slot == slot
            && child.scalar_occurrences[i].tag == tag
            && child.scalar_occurrences[i].scalar_kind == scalar_kind
            && forall|j: int| 0 <= j < child.scalar_occurrences.len()
                && child.scalar_occurrences[j].slot == slot ==> j == i)
    {
        None
    } else {
        let i = choose|i: int| 0 <= i < child.scalar_occurrences.len()
            && child.scalar_occurrences[i].slot == slot
            && child.scalar_occurrences[i].tag == tag
            && child.scalar_occurrences[i].scalar_kind == scalar_kind
            && forall|j: int| 0 <= j < child.scalar_occurrences.len()
                && child.scalar_occurrences[j].slot == slot ==> j == i;
        Some(child.scalar_occurrences[i])
    }
}

pub open spec fn admission_scalar_slot_absent(
    child: AdmissionFrameState, slot: usize,
) -> bool {
    !child.seen.contains(slot as int)
        && !child.scalar_slots.contains(slot as int)
        && forall|i: int| 0 <= i < child.scalar_occurrences.len()
            ==> child.scalar_occurrences[i].slot != slot
}

pub open spec fn admission_optional_bytes_slot(
    child: AdmissionFrameState, slot: usize, tag: u32,
) -> Option<Seq<u8>> {
    if admission_scalar_slot_absent(child, slot) {
        Some(Seq::empty())
    } else {
        match admission_unique_scalar_occurrence(child, slot, tag, 16) {
            Some(occurrence) if occurrence.bytes.len() > 0 => Some(occurrence.bytes),
            _ => None,
        }
    }
}

pub open spec fn admission_optional_u32_slot(
    child: AdmissionFrameState, slot: usize, tag: u32,
) -> Option<u32> {
    if admission_scalar_slot_absent(child, slot) {
        Some(0)
    } else {
        match admission_unique_scalar_occurrence(child, slot, tag, 4) {
            Some(occurrence) if 0 < occurrence.varint_value <= u32::MAX as u64 =>
                Some(occurrence.varint_value as u32),
            _ => None,
        }
    }
}

pub open spec fn admission_big_rational_fields(
    child: AdmissionFrameState,
) -> Option<(Seq<u8>, Seq<u8>)> {
    if child.frame_kind != 1 || child.family != AdmissionFamily::BigRationalFrame
        || child.program_type_id != 25 || child.slot_count != 2
    {
        None
    } else {
        match (
            admission_optional_bytes_slot(child, 0, 1),
            admission_optional_bytes_slot(child, 1, 2),
        ) {
            (Some(numerator), Some(denominator)) => Some((numerator, denominator)),
            _ => None,
        }
    }
}

pub open spec fn admission_fixed_point_fields(
    child: AdmissionFrameState,
) -> Option<(Seq<u8>, u32)> {
    if child.frame_kind != 1 || child.family != AdmissionFamily::FixedPointFrame
        || child.program_type_id != 26 || child.slot_count != 2
    {
        None
    } else {
        match (
            admission_optional_bytes_slot(child, 0, 1),
            admission_optional_u32_slot(child, 1, 2),
        ) {
            (Some(unscaled), Some(scale)) => Some((unscaled, scale)),
            _ => None,
        }
    }
}

pub open spec fn admission_connective_body_handles(
    child: AdmissionFrameState,
) -> Option<Seq<usize>> {
    if child.frame_kind != 1 || child.program_type_id != 57 || child.slot_count != 1
        || child.scalar_occurrences.len() != 0
        || child.seen.contains(0) != (child.returns.len() > 0)
        || !(forall|i: int| 0 <= i < child.returns.len() ==> {
            &&& child.returns[i].slot == 0
            &&& child.returns[i].tag == 1
            &&& match child.returns[i].kind {
                AdmissionReturnedKind::Par { .. } => true,
                _ => false,
            }
            &&& child.returns[i].score_handle.is_some()
            &&& child.returns[i].score_origin_body_id == Some(child.returns[i].body_id)
        })
    {
        None
    } else {
        Some(Seq::new(child.returns.len() as nat,
            |i: int| child.returns[i].score_handle.unwrap()))
    }
}

/// A returned summary is accepted only if its semantic flags follow the
/// completed child frame. Incomplete ReceiveBind/MatchCase children return no
/// score handle, as their production reducers return `score: None`.
pub open spec fn admission_child_summary_agrees(
    child: AdmissionFrameState, summary: AdmissionReturnedKind,
    arena: Seq<AdmissionScoreBinding>,
) -> bool {
    let facts_agree = match summary {
        AdmissionReturnedKind::Par { semantic_default, canonical_path_stable } =>
            child.frame_kind == 1 && child.program_type_id == 1
                && semantic_default == admission_raw_par_semantic_default(child)
                && admission_raw_par_path_stability(child) == Some(canonical_path_stable),
        AdmissionReturnedKind::Component { family_tag, canonical_path_stable } =>
            child.frame_kind == 1
                && admission_raw_par_component_child(family_tag, child)
                && admission_component_path_stability(child) == Some(canonical_path_stable),
        AdmissionReturnedKind::ExprPayload { canonical_path_stable, .. } =>
            admission_expr_payload_path_stability(child) == Some(canonical_path_stable),
        AdmissionReturnedKind::ReceiveBind { source_present } =>
            child.family == AdmissionFamily::ReceiveBindFrame
                && source_present == admission_returned_par(child, 1),
        AdmissionReturnedKind::MatchCase { required_present, canonical_fixed_point } =>
            child.family == AdmissionFamily::MatchCaseFrame
                && required_present == (
                    admission_returned_par(child, 0) && admission_returned_par(child, 1)
                )
                && canonical_fixed_point == !admission_returned_default_par(child, 3),
        AdmissionReturnedKind::CostScore =>
            child.family == AdmissionFamily::CostSignatureFrame
                || child.family == AdmissionFamily::CostRegionFrame
                || (child.frame_kind == 1
                    && (child.program_type_id == 10 || child.program_type_id == 11)),
        AdmissionReturnedKind::BigRational { numerator, denominator } =>
            admission_big_rational_fields(child) == Some((numerator, denominator)),
        AdmissionReturnedKind::FixedPoint { unscaled, scale } =>
            admission_fixed_point_fields(child) == Some((unscaled, scale)),
        AdmissionReturnedKind::VarScore => child.frame_kind == 1
            && child.program_type_id == 6,
        AdmissionReturnedKind::Unit => child.frame_kind == 1
            && (child.program_type_id == 7 || child.program_type_id == 64),
        AdmissionReturnedKind::VarRef { index, depth } =>
            admission_var_ref_fields(child) == Some((index, depth)),
        AdmissionReturnedKind::ConnectiveBody { par_handles } =>
            admission_connective_body_handles(child) == Some(par_handles),
        AdmissionReturnedKind::Bytes { value } =>
            admission_bytes_leaf_payload(child) == Some(value),
        AdmissionReturnedKind::KeyValuePair { key, value_par } =>
            admission_key_value_pair_fields(child) == Some((key, value_par))
                && admission_key_value_pair_bindings(key, value_par, arena),
        AdmissionReturnedKind::Map { key_bytes, value_score_handle } =>
            admission_map_summary_complete(child)
                && admission_map_source_matches(child, key_bytes)
                && value_score_handle == admission_map_value_score_handle(child),
        AdmissionReturnedKind::EPathMapSnapshot { .. } => false,
        AdmissionReturnedKind::Other => child.family == AdmissionFamily::Other
            && ((child.frame_kind == 1
                && (child.program_type_id == 3 || child.program_type_id == 14
                    || child.program_type_id == 17))
                || (child.frame_kind == 3 && child.score_handle.is_none())),
    };
    facts_agree && if admission_summary_has_score(summary) {
        admission_score_body_agrees(child, arena)
    } else {
        child.score_handle.is_none()
    }
}

pub open spec fn admission_raw_par_ordered(
    parent: AdmissionFrameState, child: AdmissionFrameState,
    arena: Seq<AdmissionScoreBinding>, slot: usize,
) -> bool {
    match child.score_handle {
        None => false,
        Some(current_handle) => (current_handle as int) < arena.len()
            && forall|i: int| 0 <= i < parent.returns.len()
                && parent.returns[i].slot == slot
                ==> match (parent.returns[i].kind, parent.returns[i].score_handle) {
                    (AdmissionReturnedKind::Component { .. }, Some(previous_handle)) =>
                        (previous_handle as int) < arena.len()
                            && !admission_bytes_lex_lt(
                                arena[current_handle as int].transcript_bytes,
                                arena[previous_handle as int].transcript_bytes,
                            )
                            && (arena[current_handle as int].transcript_bytes
                                != arena[previous_handle as int].transcript_bytes
                                || !admission_bytes_lex_lt(
                                    child.source_bytes,
                                    parent.returns[i].body_bytes,
                                )),
                    _ => false,
                },
    }
}

pub open spec fn admission_arm_child_summary_agrees(
    parent: AdmissionFrameState, child: AdmissionFrameState,
    summary: AdmissionReturnedKind, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    match parent.waiting_slot {
        None => false,
        Some(slot) => match parent.waiting_tag {
            None => false,
            Some(tag) => admission_arm_summary_matches(parent.family, slot, tag, summary)
                && (if parent.program_type_id == 1 {
                    match summary {
                        AdmissionReturnedKind::Component { family_tag, .. } => family_tag == tag
                            && admission_raw_par_ordered(parent, child, arena, slot),
                        _ => false,
                    }
                } else { true })
                && (match summary {
                    AdmissionReturnedKind::CostScore => match parent.family {
                        AdmissionFamily::CostTokenStack if slot == 0 =>
                            child.family == AdmissionFamily::CostSignatureFrame,
                        AdmissionFamily::Send if slot == 5 =>
                            child.family == AdmissionFamily::CostRegionFrame,
                        AdmissionFamily::Receive if slot == 8 =>
                            child.family == AdmissionFamily::CostRegionFrame,
                        AdmissionFamily::ReceiveBindFrame if slot == 4 =>
                            child.family == AdmissionFamily::CostRegionFrame,
                        AdmissionFamily::CostRegionFrame if slot == 5 =>
                            child.family == AdmissionFamily::CostSignatureFrame,
                        AdmissionFamily::CostSignatureFrame if slot == 0 =>
                            child.program_type_id == 10,
                        AdmissionFamily::Other if parent.program_type_id == 10 && slot == 0 =>
                            child.program_type_id == 11,
                        _ => true,
                    },
                    _ => true,
                })
                && if parent.family == AdmissionFamily::Expr && slot == 0 {
                    if tag == 19 {
                        match summary {
                            AdmissionReturnedKind::ExprPayload {
                                score_present, canonical_fixed_point, ..
                            } => child.family == AdmissionFamily::EVarFrame
                                && score_present == admission_returned_var(child, 0)
                                && canonical_fixed_point,
                            _ => false,
                        }
                    } else if tag == 5 || tag == 6 || tag == 20 || tag == 21
                        || tag == 22 || tag == 23 || tag == 24
                        || tag == 32 || tag == 33
                        || (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31)
                    {
                        admission_expr_structural_payload_with_arena(
                            parent, child, arena,
                        ) == Some(summary)
                    } else {
                        true
                    }
                } else { true }
                && if parent.family == AdmissionFamily::Other
                    && parent.program_type_id == 32 && slot == 0 && tag == 1
                {
                    admission_ezipper_pathmap_payload(parent, child) == Some(summary)
                } else { true }
                && if parent.family == AdmissionFamily::New && slot == 3 && tag == 4 {
                    match summary {
                        AdmissionReturnedKind::Map { key_bytes, .. } =>
                            forall|i: int| 0 <= i < parent.returns.len()
                                && parent.returns[i].slot == 3
                                ==> match parent.returns[i].kind {
                                    AdmissionReturnedKind::Map { key_bytes: previous, .. } =>
                                        admission_bytes_lex_lt(previous, key_bytes),
                                    _ => false,
                                },
                        _ => false,
                    }
                } else { true }
                && match summary {
                    AdmissionReturnedKind::Other => admission_opaque_child_edge(parent, child)
                        || admission_epathmap_child_edge(parent, child),
                    AdmissionReturnedKind::KeyValuePair { .. } =>
                        parent.family == AdmissionFamily::Other
                            && parent.program_type_id == 30
                            && parent.frame_kind == 1
                            && slot == 0 && tag == 1
                            && child.program_type_id == 34,
                    _ => true,
                },
        },
    }
}

pub proof fn admission_rejects_fabricated_receive_source(
    child: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>,
)
    requires
        child.family == AdmissionFamily::ReceiveBindFrame,
        !admission_returned_par(child, 1),
    ensures !admission_child_summary_agrees(
        child, AdmissionReturnedKind::ReceiveBind { source_present: true }, arena
    ),
{}

pub proof fn admission_rejects_fabricated_match_case(
    child: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>,
)
    requires
        child.family == AdmissionFamily::MatchCaseFrame,
        !admission_returned_par(child, 0),
    ensures !admission_child_summary_agrees(
        child, AdmissionReturnedKind::MatchCase {
            required_present: true, canonical_fixed_point: true,
        }, arena
    ),
{}

pub open spec fn admission_receive_guard_fixed(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 7
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::Par { semantic_default, .. } => !semantic_default,
            _ => false,
        }
}

pub open spec fn admission_match_cases_fixed(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 1
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::MatchCase { canonical_fixed_point, .. } => canonical_fixed_point,
            _ => false,
        }
}

pub open spec fn admission_expr_fixed(frame: AdmissionFrameState) -> bool {
    forall|i: int| 0 <= i < frame.returns.len() && frame.returns[i].slot == 0
        ==> match frame.returns[i].kind {
            AdmissionReturnedKind::ExprPayload { canonical_fixed_point, .. } => canonical_fixed_point,
            AdmissionReturnedKind::BigRational { .. }
            | AdmissionReturnedKind::FixedPoint { .. } => true,
            _ => false,
        }
}

pub open spec fn admission_new_uris_sorted(frame: AdmissionFrameState) -> bool {
    forall|i: int, j: int| 0 <= i < j < frame.scalar_occurrences.len()
        && frame.scalar_occurrences[i].slot == 2
        && frame.scalar_occurrences[j].slot == 2
        ==> !admission_bytes_lex_lt(
            frame.scalar_occurrences[j].bytes,
            frame.scalar_occurrences[i].bytes,
        )
}

/// These failures occur only on the complete semantic lane in production.
/// An incomplete component still finalizes with the nonsemantic score.
pub open spec fn admission_family_admissible(frame: AdmissionFrameState) -> bool {
    !admission_family_complete(frame) || match frame.family {
        AdmissionFamily::Receive => admission_receive_guard_fixed(frame),
        AdmissionFamily::New => admission_new_uris_sorted(frame),
        AdmissionFamily::Match => admission_match_cases_fixed(frame),
        AdmissionFamily::Expr => admission_expr_fixed(frame),
        _ => true,
    }
}

pub open spec fn admission_required_descriptor_slot(
    program_type_id: u16, tag: u32, slot: int,
) -> bool {
    match generated::generated_descriptor(program_type_id, tag) {
        Some(descriptor) => descriptor.frame_kind == 3
            && descriptor.slot as int == slot,
        None => false,
    }
}

pub open spec fn admission_descriptor_not_pathmap(
    program_type_id: u16, tag: u32,
) -> bool {
    match generated::generated_descriptor(program_type_id, tag) {
        Some(descriptor) => descriptor.frame_kind != 3,
        None => true,
    }
}

pub open spec fn admission_no_required_descriptor_slot(
    program_type_id: u16, slot: int,
) -> bool {
    forall|tag: u32| !#[trigger] admission_required_descriptor_slot(program_type_id, tag, slot)
}

pub open spec fn admission_required_slots_generated(frame: AdmissionFrameState) -> bool {
    frame.frame_kind != 1 || forall|slot: int| 0 <= slot < frame.slot_count as int
        ==> frame.required.contains(slot) == (exists|tag: u32|
            #[trigger] admission_required_descriptor_slot(frame.program_type_id, tag, slot))
}

pub open spec fn admission_frame_wf(frame: AdmissionFrameState) -> bool {
    frame.region_start == 0
        && frame.source_bytes.len() == frame.region_end as int
        && admission_required_slots_generated(frame)
        && (match admission_family_program_id(frame.family) {
            Some(id) => frame.program_type_id == id,
            None => true,
        })
        && frame.region_start <= frame.cursor
        && frame.cursor <= frame.region_end
        && (match admission_family_slot_count(frame.family) {
            Some(count) => frame.slot_count == count,
            None => true,
        })
        && (match frame.last_slot {
            Some(slot) => slot < frame.slot_count,
            None => true,
        })
        && (frame.waiting_id.is_some() == (frame.waiting_kind != 0))
        && (if frame.frame_kind == 1 || frame.frame_kind == 2 {
            frame.waiting_id.is_some() == frame.waiting_slot.is_some()
                && frame.waiting_id.is_some() == frame.waiting_tag.is_some()
        } else {
            frame.waiting_slot.is_none() && frame.waiting_tag.is_none()
        })
        && (match frame.waiting_slot {
            Some(slot) => slot < frame.slot_count && frame.seen.contains(slot as int),
            None => true,
        })
        && frame.nested_started as int == frame.nested_resolved as int
            + (if frame.waiting_kind == 3 { 1int } else { 0int })
        && (forall|slot: int| frame.seen.contains(slot)
            ==> 0 <= slot < frame.slot_count as int)
        && (forall|slot: int| frame.required.contains(slot)
            ==> 0 <= slot < frame.slot_count as int)
        && (forall|slot: int| frame.scalar_slots.contains(slot)
            ==> frame.seen.contains(slot))
        && (forall|i: int| 0 <= i < frame.scalar_occurrences.len()
            ==> frame.scalar_occurrences[i].slot < frame.slot_count
                && frame.scalar_slots.contains(frame.scalar_occurrences[i].slot as int))
        && (forall|i: int| 0 <= i < frame.returns.len()
            ==> frame.returns[i].slot < frame.slot_count
                && frame.seen.contains(frame.returns[i].slot as int)
                && admission_return_matches(
                    frame.family, frame.returns[i].slot, frame.returns[i].kind
                ))
}

pub open spec fn admission_child_physical_contained(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> bool {
    (child.physical_source == parent.physical_source
        && child.physical_start >= parent.physical_start
        && child.physical_start as int + child.region_end as int
            <= parent.physical_start as int + parent.region_end as int
        && child.source_bytes == parent.source_bytes.subrange(
            child.physical_start as int - parent.physical_start as int,
            child.physical_start as int - parent.physical_start as int + child.region_end as int,
        ))
        || (exists|start: int| 0 <= start
            && start + child.region_end as int <= parent.region_end as int
            && child.source_bytes == #[trigger] parent.source_bytes.subrange(
                start, start + child.region_end as int,
            ))
}

pub open spec fn admission_state_wf(state: AdmissionState) -> bool {
    state.frames.len() > 0
        && state.pushed as int == state.completed as int + state.frames.len()
        && state.root_end == state.frames[0].region_end
        && state.frames[0].return_id.is_none()
        && (forall|handle: int| 0 <= handle < state.arena.len()
            ==> state.arena[handle].encoded_len > 0
                && state.arena[handle].transcript_bytes.len()
                    == state.arena[handle].encoded_len)
        && (forall|index: int| 0 <= index < state.frames.len()
            ==> admission_frame_wf(state.frames[index]))
        && (forall|index: int, i: int|
            0 <= index < state.frames.len()
                && 0 <= i < state.frames[index].returns.len()
                ==> state.frames[index].returns[i].score_handle.is_some()
                    == state.frames[index].returns[i].score_origin_body_id.is_some()
                && match state.frames[index].returns[i].score_handle {
                    Some(handle) => (handle as int) < state.arena.len()
                        && state.arena[handle as int].body_id
                            == state.frames[index].returns[i].score_origin_body_id.unwrap(),
                    None => true,
                })
        && (forall|index: int| 1 <= index < state.frames.len()
            ==> ((state.frames[index - 1].frame_kind == 1
                || state.frames[index - 1].frame_kind == 2)
                ==> #[trigger] admission_child_physical_contained(
                    state.frames[index - 1], state.frames[index],
                )))
}

pub proof fn admission_last_frame_replacement_preserves_wf(
    before: AdmissionState, after: AdmissionState,
)
    requires
        admission_state_wf(before),
        after.frames.len() == before.frames.len(),
        after.arena == before.arena,
        after.root_end == before.root_end,
        after.pushed == before.pushed,
        after.completed == before.completed,
        admission_frame_wf(after.frames[after.frames.len() - 1]),
        after.frames[after.frames.len() - 1].returns
            == before.frames[before.frames.len() - 1].returns,
        after.frames[after.frames.len() - 1].return_id
            == before.frames[before.frames.len() - 1].return_id,
        after.frames[after.frames.len() - 1].physical_start
            == before.frames[before.frames.len() - 1].physical_start,
        after.frames[after.frames.len() - 1].physical_source
            == before.frames[before.frames.len() - 1].physical_source,
        after.frames[after.frames.len() - 1].region_end
            == before.frames[before.frames.len() - 1].region_end,
        after.frames[after.frames.len() - 1].source_bytes
            == before.frames[before.frames.len() - 1].source_bytes,
        forall|i: int| 0 <= i < before.frames.len() - 1
            ==> after.frames[i] == before.frames[i],
    ensures admission_state_wf(after),
{
    let last = before.frames.len() - 1;
    assert(after.frames[0].return_id == before.frames[0].return_id);
    assert forall|i: int| 0 <= i < after.frames.len()
        implies admission_frame_wf(after.frames[i]) by {
        if i != last {
            assert(after.frames[i] == before.frames[i]);
        }
    }
    assert forall|frame_index: int, return_index: int|
        0 <= frame_index < after.frames.len()
            && 0 <= return_index < after.frames[frame_index].returns.len()
        implies after.frames[frame_index].returns[return_index].score_handle.is_some()
            == after.frames[frame_index].returns[return_index]
                .score_origin_body_id.is_some()
            && match after.frames[frame_index].returns[return_index].score_handle {
                Some(handle) => (handle as int) < after.arena.len()
                    && after.arena[handle as int].body_id
                        == after.frames[frame_index].returns[return_index]
                            .score_origin_body_id.unwrap(),
                None => true,
            } by {
        if frame_index != last {
            assert(after.frames[frame_index] == before.frames[frame_index]);
        }
        assert(after.frames[frame_index].returns
            == before.frames[frame_index].returns);
    }
    assert forall|frame_index: int| 1 <= frame_index < after.frames.len()
        implies ((after.frames[frame_index - 1].frame_kind != 1
            && after.frames[frame_index - 1].frame_kind != 2)
            || #[trigger] admission_child_physical_contained(
                after.frames[frame_index - 1], after.frames[frame_index],
            )) by {
        if frame_index != last {
            assert(after.frames[frame_index] == before.frames[frame_index]);
        }
        assert(after.frames[frame_index - 1] == before.frames[frame_index - 1]);
        assert(after.frames[frame_index].physical_start
            == before.frames[frame_index].physical_start);
        assert(after.frames[frame_index].physical_source
            == before.frames[frame_index].physical_source);
        assert(after.frames[frame_index].region_end
            == before.frames[frame_index].region_end);
        assert(after.frames[frame_index].source_bytes
            == before.frames[frame_index].source_bytes);
        if before.frames[frame_index - 1].frame_kind == 1
            || before.frames[frame_index - 1].frame_kind == 2
        {
            assert(admission_child_physical_contained(
                before.frames[frame_index - 1], before.frames[frame_index],
            ));
        }
    }
    assert(admission_state_wf(after));
}

/// The first `push_admission_frame` in `drive_admission` owns exactly one root.
/// Schema-required slots are supplied by the generated `RhoMatchProgram`.
pub open spec fn admission_initial_state(
    root: AdmissionFrameState,
) -> AdmissionState {
    AdmissionState {
        frames: Seq::empty().push(root),
        arena: Seq::empty(),
        root_end: root.region_end,
        pushed: 1,
        completed: 0,
        next_obligation_id: 0,
    }
}

pub open spec fn admission_root_initial(root: AdmissionFrameState) -> bool {
    admission_frame_wf(root)
        && root.frame_kind == 1
        && root.body_id == 0
        && generated::generated_slot_count(root.program_type_id) == Some(root.slot_count)
        && root.cursor == root.region_start
        && root.last_slot.is_none()
        && root.seen == Set::empty()
        && root.scalar_slots == Set::empty()
        && root.scalar_occurrences.len() == 0
        && root.returns.len() == 0
        && root.waiting_id.is_none()
        && root.return_id.is_none()
        && root.score_handle.is_none()
        && root.nested_started == 0
        && root.nested_resolved == 0
}

pub proof fn admission_initial_wf(root: AdmissionFrameState)
    requires admission_root_initial(root),
    ensures admission_state_wf(admission_initial_state(root)),
{}

pub open spec fn admission_slots_complete(frame: AdmissionFrameState) -> bool {
    // Wire/schema-required presence is distinct from component completeness.
    // The generated PathMap snapshot field is one production example.
    forall|slot: int| frame.required.contains(slot) ==> frame.seen.contains(slot)
}

pub open spec fn admission_score_body_agrees(
    frame: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>,
) -> bool {
    match frame.score_handle {
        None => false,
        Some(handle) => {
            &&& (handle as int) < arena.len()
            &&& arena[handle as int].encoded_len > 0
            &&& arena[handle as int].transcript_bytes.len()
                == arena[handle as int].encoded_len
            &&& admission_score_origin_matches(frame, arena, handle)
            &&& arena[handle as int].semantic == admission_family_complete(frame)
        },
    }
}

pub open spec fn admission_score_origin_matches(
    frame: AdmissionFrameState, arena: Seq<AdmissionScoreBinding>, handle: usize,
) -> bool {
    (handle as int) < arena.len()
        && (arena[handle as int].body_id == frame.body_id
            || (frame.program_type_id == 10
                && frame.returns.len() == 1
                && frame.returns[0].slot == 0
                && frame.returns[0].tag == 3
                && frame.returns[0].kind == AdmissionReturnedKind::CostScore
                && frame.returns[0].score_handle == Some(handle)
                && frame.returns[0].score_origin_body_id
                    == Some(arena[handle as int].body_id))
            || (frame.family == AdmissionFamily::Expr
                && frame.program_type_id == 24
                && frame.returns.len() == 1
                && frame.returns[0].slot == 0
                && admission_field_tag_valid(
                    AdmissionFamily::Expr, 0, frame.returns[0].tag, true, false,
                )
                && match frame.returns[0].kind {
                    AdmissionReturnedKind::ExprPayload { score_present, .. } => score_present,
                    _ => false,
                }
                && frame.returns[0].score_handle == Some(handle)
                && frame.returns[0].score_origin_body_id
                    == Some(arena[handle as int].body_id)))
}

pub open spec fn admission_continue_step(
    state: AdmissionState, next_cursor: usize, slot: usize, repeated: bool,
    field_tag: u32,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 {
        None
    } else {
        let index = state.frames.len() - 1;
        let top = state.frames[index];
        let map_key_continue = top.frame_kind == 2
            && top.family == AdmissionFamily::Other
            && top.program_type_id == 1
            && top.slot_count == 2
            && slot == 0
            && field_tag == 1
            && !repeated
            && admission_map_key_field(top, next_cursor);
        // MUTANT_ANCHOR_UNCHECKED_LENGTH
        if (top.frame_kind != 1 && !map_key_continue)
            || next_cursor > top.region_end
            // MUTANT_ANCHOR_NON_PROGRESS
            || next_cursor <= top.cursor
            || top.waiting_id.is_some()
            || top.score_handle.is_some()
            || slot >= top.slot_count
            || admission_slot_requires_child(top.family, slot)
            || !admission_field_tag_valid(top.family, slot, field_tag, false, repeated)
            || !slot_admissible(top.last_slot, slot, top.slot_count, repeated)
            || (!repeated && top.seen.contains(slot as int))
        {
            None
        } else {
            let updated = AdmissionFrameState {
                physical_source: top.physical_source,
                physical_start: top.physical_start,
                program_type_id: top.program_type_id,
                region_start: top.region_start,
                region_end: top.region_end,
                source_bytes: top.source_bytes,
                cursor: next_cursor,
                slot_count: top.slot_count,
                last_slot: Some(slot),
                seen: top.seen.insert(slot as int),
                required: top.required,
                scalar_slots: top.scalar_slots.insert(slot as int),
                scalar_occurrences: top.scalar_occurrences,
                returns: top.returns,
                family: top.family,
                waiting_id: top.waiting_id,
                waiting_kind: top.waiting_kind,
                waiting_slot: top.waiting_slot,
                waiting_tag: top.waiting_tag,
                return_id: top.return_id,
                frame_kind: top.frame_kind,
                body_id: top.body_id,
                score_handle: top.score_handle,
                nested_started: top.nested_started,
                nested_resolved: top.nested_resolved,
            };
            Some(AdmissionState {
                frames: state.frames.update(index, updated),
                arena: state.arena,
                root_end: state.root_end,
                pushed: state.pushed,
                completed: state.completed,
                next_obligation_id: state.next_obligation_id,
            })
        }
    }
}

pub proof fn admission_continue_refines(
    state: AdmissionState, next_cursor: usize, slot: usize, repeated: bool,
    field_tag: u32,
)
    requires state.frames.len() > 0,
    ensures match admission_continue_step(state, next_cursor, slot, repeated, field_tag) {
        Some(next) => {
            &&& next.frames.len() == state.frames.len()
            &&& next.frames[next.frames.len() - 1].cursor > state.frames[state.frames.len() - 1].cursor
            &&& next.frames[next.frames.len() - 1].cursor <= state.frames[state.frames.len() - 1].region_end
            &&& next.frames[next.frames.len() - 1].last_slot == Some(slot)
            &&& next.frames[next.frames.len() - 1].seen.contains(slot as int)
            &&& next.pushed == state.pushed
            &&& next.completed == state.completed
        },
        None => true,
    },
{}

pub proof fn admission_continue_preserves_wf(
    state: AdmissionState, next_cursor: usize, slot: usize, repeated: bool,
    field_tag: u32,
)
    requires admission_state_wf(state),
    ensures match admission_continue_step(state, next_cursor, slot, repeated, field_tag) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if let Some(next) = admission_continue_step(state, next_cursor, slot, repeated, field_tag) {
        let index = state.frames.len() - 1;
        assert(admission_frame_wf(state.frames[index]));
        assert(slot < state.frames[index].slot_count);
        assert(next.frames[index].region_start == state.frames[index].region_start);
        assert(next.frames[index].region_end == state.frames[index].region_end);
        assert(next.frames[index].source_bytes == state.frames[index].source_bytes);
        assert(next.frames[index].physical_start == state.frames[index].physical_start);
        assert(next.frames[index].program_type_id == state.frames[index].program_type_id);
        assert(next.frames[index].frame_kind == state.frames[index].frame_kind);
        assert(next.frames[index].cursor == next_cursor);
        assert(next.frames[index].slot_count == state.frames[index].slot_count);
        assert(next.frames[index].last_slot == Some(slot));
        assert(next.frames[index].seen == state.frames[index].seen.insert(slot as int));
        assert(next.frames[index].required == state.frames[index].required);
        assert(admission_required_slots_generated(next.frames[index]));
        assert(next.frames[index].waiting_kind == state.frames[index].waiting_kind);
        assert(next.frames[index].nested_started == state.frames[index].nested_started);
        assert(next.frames[index].nested_resolved == state.frames[index].nested_resolved);
        assert forall|value: int| state.frames[index].seen.contains(value)
            implies 0 <= value < state.frames[index].slot_count as int by {
            assert(admission_frame_wf(state.frames[index]));
        }
        assert forall|value: int| next.frames[index].seen.contains(value)
            implies 0 <= value < next.frames[index].slot_count as int by {
            if value != slot as int {
                assert(state.frames[index].seen.contains(value));
                assert(0 <= value < state.frames[index].slot_count as int);
            } else {
                assert(0 <= value < state.frames[index].slot_count as int);
            }
        }
        assert forall|value: int| next.frames[index].required.contains(value)
            implies 0 <= value < next.frames[index].slot_count as int by {
            assert(state.frames[index].required.contains(value));
        }
        assert(admission_frame_wf(next.frames[index]));
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_frame_wf(next.frames[i]) by {
            if i != index {
                assert(next.frames[i] == state.frames[i]);
            }
        }
        assert(next.frames.len() == state.frames.len());
        assert(next.arena == state.arena);
        assert(next.root_end == state.root_end);
        assert(next.pushed == state.pushed);
        assert(next.completed == state.completed);
        assert(next.frames[0].return_id == state.frames[0].return_id);
        assert forall|frame_index: int| 0 <= frame_index < next.frames.len()
            implies next.frames[frame_index].returns
                == state.frames[frame_index].returns by {
            if frame_index != index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            }
        }
        assert forall|frame_index: int, return_index: int|
            0 <= frame_index < next.frames.len()
                && 0 <= return_index < next.frames[frame_index].returns.len()
            implies next.frames[frame_index].returns[return_index].score_handle.is_some()
                == next.frames[frame_index].returns[return_index]
                    .score_origin_body_id.is_some()
                && match next.frames[frame_index].returns[return_index].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[frame_index].returns[return_index]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {}
        assert forall|frame_index: int| 1 <= frame_index < next.frames.len()
            implies ((next.frames[frame_index - 1].frame_kind != 1
                && next.frames[frame_index - 1].frame_kind != 2)
                || #[trigger] admission_child_physical_contained(
                    next.frames[frame_index - 1], next.frames[frame_index],
                )) by {
            if frame_index != index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            }
            assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
            assert(next.frames[frame_index].physical_start
                == state.frames[frame_index].physical_start);
            assert(next.frames[frame_index].region_end
                == state.frames[frame_index].region_end);
            assert(next.frames[frame_index].source_bytes
                == state.frames[frame_index].source_bytes);
            if state.frames[frame_index - 1].frame_kind == 1
                || state.frames[frame_index - 1].frame_kind == 2
            {
                assert(admission_child_physical_contained(
                    state.frames[frame_index - 1], state.frames[frame_index],
                ));
            }
        }
        assert(admission_state_wf(next));
    }
}

/// `checked_field_end(top.cursor, top.region_end, ...)` supplies the byte
/// bounds on every `Some(field_end)` result. The generated slot resolver
/// supplies the remaining premises before `FrameAction::Continue`.
pub proof fn admission_continue_from_decoded_field(
    state: AdmissionState, field_end: usize, slot: usize, repeated: bool,
    field_tag: u32,
)
    requires
        admission_state_wf(state),
        state.frames[state.frames.len() - 1].frame_kind == 1,
        state.frames[state.frames.len() - 1].cursor < field_end,
        field_end <= state.frames[state.frames.len() - 1].region_end,
        state.frames[state.frames.len() - 1].waiting_id.is_none(),
        state.frames[state.frames.len() - 1].score_handle.is_none(),
        !admission_slot_requires_child(state.frames[state.frames.len() - 1].family, slot),
        admission_field_tag_valid(
            state.frames[state.frames.len() - 1].family,
            slot, field_tag, false, repeated,
        ),
        slot_admissible(
            state.frames[state.frames.len() - 1].last_slot,
            slot,
            state.frames[state.frames.len() - 1].slot_count,
            repeated,
        ),
        repeated || !state.frames[state.frames.len() - 1].seen.contains(slot as int),
    ensures
        admission_continue_step(state, field_end, slot, repeated, field_tag).is_some(),
        admission_continue_step(state, field_end, slot, repeated, field_tag).unwrap().frames[
            state.frames.len() - 1
        ].cursor == field_end,
{}

/// EVarBody is a message arm (tag 19), so it cannot masquerade as a complete
/// immediate scalar through `FrameAction::Continue`.
pub proof fn admission_evar_message_rejects_scalar_continue(
    state: AdmissionState, next_cursor: usize,
)
    requires
        state.frames.len() > 0,
        state.frames[state.frames.len() - 1].family == AdmissionFamily::Expr,
    ensures admission_continue_step(state, next_cursor, 0, false, 19).is_none(),
{}

pub open spec fn admission_push_step(
    state: AdmissionState, parent_next: usize, child: AdmissionFrameState,
    slot: usize, repeated: bool, field_tag: u32, frame_bytes: usize,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 {
        None
    } else {
        let index = state.frames.len() - 1;
        let parent = state.frames[index];
        let map_value_push = parent.frame_kind == 2
            && parent.family == AdmissionFamily::Other
            && parent.program_type_id == 1
            && parent.slot_count == 2
            && slot == 1
            && field_tag == 2
            && !repeated
            && child.frame_kind == 1
            && child.program_type_id == 1
            && admission_map_value_field(parent, parent_next, child);
        if (parent.frame_kind != 1 && !map_value_push)
            || parent.waiting_id.is_some()
            || parent_next <= parent.cursor
            || parent_next > parent.region_end
            || parent.score_handle.is_some()
            || !admission_field_tag_valid(parent.family, slot, field_tag, true, repeated)
            || !slot_admissible(parent.last_slot, slot, parent.slot_count, repeated)
            || (!repeated && parent.seen.contains(slot as int))
            || !admission_child_physical_contained(parent, child)
            || child.cursor != child.region_start
            || child.waiting_id.is_some()
            || child.return_id.is_some()
            || child.last_slot.is_some()
            || child.seen != Set::empty()
            || child.scalar_slots != Set::empty()
            || child.scalar_occurrences.len() != 0
            || child.returns.len() != 0
            || child.score_handle.is_some()
            || child.nested_started != 0
            || child.nested_resolved != 0
            || child.frame_kind == 0
            || !admission_frame_wf(child)
            || state.next_obligation_id == u64::MAX
            || state.pushed == usize::MAX
            || frame_bytes == 0
            || state.frames.len() + 1 > usize::MAX as int / frame_bytes as int
            || (child.frame_kind == 3 && parent.nested_started == u64::MAX)
        {
            None
        } else {
            let parent_next_frame = AdmissionFrameState {
                physical_source: parent.physical_source,
                physical_start: parent.physical_start,
                program_type_id: parent.program_type_id,
                region_start: parent.region_start,
                region_end: parent.region_end,
                source_bytes: parent.source_bytes,
                cursor: parent_next,
                slot_count: parent.slot_count,
                last_slot: Some(slot),
                seen: parent.seen.insert(slot as int),
                required: parent.required,
                scalar_slots: parent.scalar_slots,
                scalar_occurrences: parent.scalar_occurrences,
                returns: parent.returns,
                family: parent.family,
                waiting_id: Some(state.next_obligation_id),
                waiting_kind: child.frame_kind,
                waiting_slot: Some(slot),
                waiting_tag: Some(field_tag),
                return_id: parent.return_id,
                frame_kind: parent.frame_kind,
                body_id: parent.body_id,
                score_handle: parent.score_handle,
                nested_started: if child.frame_kind == 3 {
                    (parent.nested_started as int + 1) as u64
                } else {
                    parent.nested_started
                },
                nested_resolved: parent.nested_resolved,
            };
            let child_next_frame = AdmissionFrameState {
                physical_source: child.physical_source,
                physical_start: child.physical_start,
                program_type_id: child.program_type_id,
                region_start: child.region_start,
                region_end: child.region_end,
                source_bytes: child.source_bytes,
                cursor: child.cursor,
                slot_count: child.slot_count,
                last_slot: child.last_slot,
                seen: child.seen,
                required: child.required,
                scalar_slots: child.scalar_slots,
                scalar_occurrences: child.scalar_occurrences,
                returns: child.returns,
                family: child.family,
                waiting_id: child.waiting_id,
                waiting_kind: child.waiting_kind,
                waiting_slot: child.waiting_slot,
                waiting_tag: child.waiting_tag,
                return_id: Some(state.next_obligation_id),
                frame_kind: child.frame_kind,
                body_id: (state.next_obligation_id as int + 1) as u64,
                score_handle: child.score_handle,
                nested_started: child.nested_started,
                nested_resolved: child.nested_resolved,
            };
            Some(AdmissionState {
                // MUTANT_ANCHOR_DROPPED_CHILD
                frames: state.frames.update(index, parent_next_frame).push(child_next_frame),
                arena: state.arena,
                root_end: state.root_end,
                pushed: (state.pushed as int + 1) as usize,
                completed: state.completed,
                next_obligation_id: (state.next_obligation_id as int + 1) as u64,
            })
        }
    }
}

pub proof fn admission_push_refines(
    state: AdmissionState, parent_next: usize, child: AdmissionFrameState,
    slot: usize, repeated: bool, field_tag: u32, frame_bytes: usize,
)
    ensures match admission_push_step(state, parent_next, child, slot, repeated, field_tag, frame_bytes) {
        Some(next) => {
            &&& next.frames.len() == state.frames.len() + 1
            &&& next.pushed as int == state.pushed as int + 1
            &&& next.completed == state.completed
            &&& next.frames[next.frames.len() - 1].return_id == Some(state.next_obligation_id)
            &&& next.frames[next.frames.len() - 1].body_id
                == (state.next_obligation_id as int + 1) as u64
            &&& next.frames[next.frames.len() - 1].score_handle.is_none()
            &&& next.frames[next.frames.len() - 2].waiting_id == Some(state.next_obligation_id)
            &&& next.frames[next.frames.len() - 2].cursor == parent_next
            &&& next.frames[next.frames.len() - 2].last_slot == Some(slot)
            &&& next.frames[next.frames.len() - 2].seen.contains(slot as int)
        },
        None => true,
    },
{}

pub proof fn admission_push_preserves_wf(
    state: AdmissionState, parent_next: usize, child: AdmissionFrameState,
    slot: usize, repeated: bool, field_tag: u32, frame_bytes: usize,
)
    requires admission_state_wf(state),
    ensures match admission_push_step(state, parent_next, child, slot, repeated, field_tag, frame_bytes) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if let Some(next) = admission_push_step(
        state, parent_next, child, slot, repeated, field_tag, frame_bytes,
    ) {
        let index = state.frames.len() - 1;
        let parent = state.frames[index];
        let next_parent = next.frames[index];
        let next_child = next.frames[index + 1];
        assert(admission_frame_wf(parent));
        assert(admission_frame_wf(child));
        assert(slot_admissible(parent.last_slot, slot, parent.slot_count, repeated));
        assert(slot < parent.slot_count);
        assert(next_parent.program_type_id == parent.program_type_id);
        assert(next_parent.required == parent.required);
        assert(next_parent.slot_count == parent.slot_count);
        assert(next_parent.frame_kind == parent.frame_kind);
        assert(admission_required_slots_generated(next_parent));
        assert forall|value: int| next_parent.seen.contains(value)
            implies 0 <= value < next_parent.slot_count as int by {
            if value != slot as int {
                assert(parent.seen.contains(value));
            }
        }
        assert forall|value: int| next_parent.required.contains(value)
            implies 0 <= value < next_parent.slot_count as int by {
            assert(parent.required.contains(value));
        }
        assert forall|value: int| next_parent.scalar_slots.contains(value)
            implies next_parent.seen.contains(value) by {
            assert(parent.scalar_slots.contains(value));
            assert(parent.seen.contains(value));
        }
        assert forall|i: int| 0 <= i < next_parent.scalar_occurrences.len()
            implies next_parent.scalar_occurrences[i].slot < next_parent.slot_count
                && next_parent.scalar_slots.contains(
                    next_parent.scalar_occurrences[i].slot as int,
                ) by {
            assert(next_parent.scalar_occurrences[i] == parent.scalar_occurrences[i]);
        }
        assert forall|i: int| 0 <= i < next_parent.returns.len()
            implies next_parent.returns[i].slot < next_parent.slot_count
                && next_parent.seen.contains(next_parent.returns[i].slot as int)
                && admission_return_matches(
                    next_parent.family, next_parent.returns[i].slot,
                    next_parent.returns[i].kind,
                ) by {
            assert(next_parent.returns[i] == parent.returns[i]);
            assert(parent.seen.contains(parent.returns[i].slot as int));
        }
        assert(admission_frame_wf(next_parent));
        assert(next_child.program_type_id == child.program_type_id);
        assert(next_child.required == child.required);
        assert(next_child.slot_count == child.slot_count);
        assert(next_child.frame_kind == child.frame_kind);
        assert(next_child.seen == child.seen);
        assert(next_child.scalar_slots == child.scalar_slots);
        assert(next_child.scalar_occurrences == child.scalar_occurrences);
        assert(next_child.returns == child.returns);
        assert(admission_required_slots_generated(next_child));
        assert(admission_frame_wf(next_child));
        assert(admission_child_physical_contained(parent, child));
        assert(admission_child_physical_contained(next_parent, next_child));
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_frame_wf(next.frames[i]) by {
            if i < index {
                assert(next.frames[i] == state.frames[i]);
            } else if i == index {
                assert(next.frames[i] == next_parent);
            } else {
                assert(i == index + 1);
                assert(next.frames[i] == next_child);
            }
        }
        assert forall|frame_index: int, return_index: int|
            0 <= frame_index < next.frames.len()
                && 0 <= return_index < next.frames[frame_index].returns.len()
            implies next.frames[frame_index].returns[return_index].score_handle.is_some()
                == next.frames[frame_index].returns[return_index]
                    .score_origin_body_id.is_some()
                && match next.frames[frame_index].returns[return_index].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[frame_index].returns[return_index]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {
            if frame_index < index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            } else if frame_index == index {
                assert(next.frames[frame_index].returns == parent.returns);
            } else {
                assert(frame_index == index + 1);
                assert(next_child.returns.len() == 0);
            }
        }
        assert forall|frame_index: int| 1 <= frame_index < next.frames.len()
            implies ((next.frames[frame_index - 1].frame_kind != 1
                && next.frames[frame_index - 1].frame_kind != 2)
                || #[trigger] admission_child_physical_contained(
                    next.frames[frame_index - 1], next.frames[frame_index],
                )) by {
            if frame_index < index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                assert(next.frames[frame_index] == state.frames[frame_index]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
            } else if frame_index == index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
                assert(next.frames[frame_index].physical_start
                    == state.frames[frame_index].physical_start);
                assert(next.frames[frame_index].region_end
                    == state.frames[frame_index].region_end);
                assert(next.frames[frame_index].source_bytes
                    == state.frames[frame_index].source_bytes);
            } else {
                assert(frame_index == index + 1);
                assert(admission_child_physical_contained(next_parent, next_child));
            }
        }
        assert(admission_state_wf(next));
    }
}

pub proof fn admission_push_rejects_bad_slot(
    state: AdmissionState, parent_next: usize, child: AdmissionFrameState,
    slot: usize, repeated: bool, field_tag: u32, frame_bytes: usize,
)
    requires
        state.frames.len() > 0,
        !slot_admissible(
            state.frames[state.frames.len() - 1].last_slot,
            slot,
            state.frames[state.frames.len() - 1].slot_count,
            repeated,
        ) || (!repeated && state.frames[state.frames.len() - 1].seen.contains(slot as int)),
    ensures admission_push_step(
        state, parent_next, child, slot, repeated, field_tag, frame_bytes
    ).is_none(),
{}

pub open spec fn admission_append_score_entry_step(
    state: AdmissionState, binding: AdmissionScoreBinding,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 || state.arena.len() >= usize::MAX as int
        || binding.encoded_len == 0
        || binding.transcript_bytes.len() != binding.encoded_len
    {
        None
    } else {
        Some(AdmissionState {
            frames: state.frames,
            arena: state.arena.push(binding),
            root_end: state.root_end,
            pushed: state.pushed,
            completed: state.completed,
            next_obligation_id: state.next_obligation_id,
        })
    }
}

pub open spec fn admission_attach_score_step(
    state: AdmissionState, handle: usize, body_id: u64,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 || handle as int >= state.arena.len() {
        None
    } else {
        let index = state.frames.len() - 1;
        let top = state.frames[index];
        let binding = state.arena[handle as int];
        if top.score_handle.is_some() || top.body_id != body_id
            || binding.encoded_len == 0
            || binding.transcript_bytes.len() != binding.encoded_len
            || !admission_score_origin_matches(top, state.arena, handle)
            || binding.semantic != admission_family_complete(top)
            || top.cursor != top.region_end || top.waiting_id.is_some()
            || top.nested_started != top.nested_resolved
            || !admission_slots_complete(top)
            || !admission_family_admissible(top)
            || ((top.family == AdmissionFamily::ReceiveBindFrame
                || top.family == AdmissionFamily::MatchCaseFrame)
                && !admission_family_complete(top))
        {
            None
        } else {
            let updated = AdmissionFrameState {
                physical_source: top.physical_source,
                physical_start: top.physical_start,
                program_type_id: top.program_type_id,
                region_start: top.region_start,
                region_end: top.region_end,
                source_bytes: top.source_bytes,
                cursor: top.cursor,
                slot_count: top.slot_count,
                last_slot: top.last_slot,
                seen: top.seen,
                required: top.required,
                scalar_slots: top.scalar_slots,
                scalar_occurrences: top.scalar_occurrences,
                returns: top.returns,
                family: top.family,
                waiting_id: top.waiting_id,
                waiting_kind: top.waiting_kind,
                waiting_slot: top.waiting_slot,
                waiting_tag: top.waiting_tag,
                return_id: top.return_id,
                frame_kind: top.frame_kind,
                body_id: top.body_id,
                score_handle: Some(handle),
                nested_started: top.nested_started,
                nested_resolved: top.nested_resolved,
            };
            Some(AdmissionState {
                frames: state.frames.update(index, updated),
                arena: state.arena,
                root_end: state.root_end,
                pushed: state.pushed,
                completed: state.completed,
                next_obligation_id: state.next_obligation_id,
            })
        }
    }
}

pub open spec fn admission_forward_bound_score_step(
    state: AdmissionState,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 {
        None
    } else {
        let top = state.frames[state.frames.len() - 1];
        if top.program_type_id != 10 || top.returns.len() != 1
            || top.returns[0].slot != 0 || top.returns[0].tag != 3
            || top.returns[0].kind != AdmissionReturnedKind::CostScore
        {
            None
        } else {
            match top.returns[0].score_handle {
                Some(handle) => admission_attach_score_step(state, handle, top.body_id),
                None => None,
            }
        }
    }
}

pub proof fn admission_forward_bound_score_refines(state: AdmissionState)
    ensures match admission_forward_bound_score_step(state) {
        Some(next) => {
            let before = state.frames[state.frames.len() - 1];
            let after = next.frames[next.frames.len() - 1];
            &&& before.program_type_id == 10
            &&& before.returns.len() == 1
            &&& before.returns[0].tag == 3
            &&& after.score_handle == before.returns[0].score_handle
            &&& next.arena == state.arena
            &&& admission_score_body_agrees(after, next.arena)
        },
        None => true,
    },
{
    if let Some(next) = admission_forward_bound_score_step(state) {
        let index = state.frames.len() - 1;
        let before = state.frames[index];
        let handle = before.returns[0].score_handle.unwrap();
        let after = next.frames[index];
        assert(admission_attach_score_step(state, handle, before.body_id) == Some(next));
        assert(after.score_handle == Some(handle));
        assert(after.body_id == before.body_id);
        assert(after.program_type_id == before.program_type_id);
        assert(after.returns == before.returns);
        assert(after.family == before.family);
        assert(after.seen == before.seen);
        assert(after.required == before.required);
        assert(after.scalar_slots == before.scalar_slots);
        assert(admission_family_complete(after) == admission_family_complete(before));
        assert(next.arena[handle as int].encoded_len > 0);
        assert(next.arena[handle as int].transcript_bytes.len()
            == next.arena[handle as int].encoded_len);
        assert(next.arena[handle as int].semantic == admission_family_complete(after));
        assert(admission_score_origin_matches(after, next.arena, handle));
        assert(admission_score_body_agrees(after, next.arena));
    }
}

pub proof fn admission_forward_bound_score_preserves_wf(state: AdmissionState)
    requires admission_state_wf(state),
    ensures match admission_forward_bound_score_step(state) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if state.frames.len() > 0 {
        let top = state.frames[state.frames.len() - 1];
        if top.program_type_id == 10 && top.returns.len() == 1
            && top.returns[0].slot == 0 && top.returns[0].tag == 3
            && top.returns[0].kind == AdmissionReturnedKind::CostScore
        {
            if let Some(handle) = top.returns[0].score_handle {
                admission_attach_score_preserves_wf(state, handle, top.body_id);
            }
        }
    }
}

pub open spec fn admission_record_score_step(
    state: AdmissionState, encoded_len: usize, body_id: u64,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 { None } else {
        let top = state.frames[state.frames.len() - 1];
        let binding = AdmissionScoreBinding {
            body_id,
            semantic: admission_family_complete(top),
            encoded_len,
            transcript_bytes: Seq::new(encoded_len as nat, |i: int| 0u8),
        };
        match admission_append_score_entry_step(state, binding) {
            None => None,
            Some(next) => admission_attach_score_step(
                next, state.arena.len() as usize, body_id,
            ),
        }
    }
}

pub proof fn admission_append_score_entry_preserves_wf(
    state: AdmissionState, binding: AdmissionScoreBinding,
)
    requires admission_state_wf(state),
    ensures match admission_append_score_entry_step(state, binding) {
        Some(next) => admission_state_wf(next)
            && next.arena.len() == state.arena.len() + 1
            && next.frames == state.frames,
        None => true,
    },
{
    if let Some(next) = admission_append_score_entry_step(state, binding) {
        let old_len = state.arena.len();
        assert(next.arena.len() == old_len + 1);
        assert(next.frames == state.frames);
        assert(next.arena[old_len as int] == binding);
        assert forall|handle: int| 0 <= handle < next.arena.len()
            implies next.arena[handle].encoded_len > 0
                && next.arena[handle].transcript_bytes.len()
                    == next.arena[handle].encoded_len by {
            if handle < old_len {
                assert(next.arena[handle] == state.arena[handle]);
            } else {
                assert(handle == old_len);
                assert(next.arena[handle] == binding);
                assert(binding.encoded_len > 0);
                assert(binding.transcript_bytes.len() == binding.encoded_len);
            }
        }
        assert forall|index: int, i: int|
            0 <= index < next.frames.len()
                && 0 <= i < next.frames[index].returns.len()
            implies next.frames[index].returns[i].score_handle.is_some()
                == next.frames[index].returns[i].score_origin_body_id.is_some()
                && match next.frames[index].returns[i].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[index].returns[i]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {
            let returned = state.frames[index].returns[i];
            assert(next.frames[index] == state.frames[index]);
            if let Some(handle) = returned.score_handle {
                assert((handle as int) < old_len);
                assert(next.arena[handle as int] == state.arena[handle as int]);
            }
        }
        assert(admission_state_wf(next));
    }
}

pub proof fn admission_attach_score_preserves_wf(
    state: AdmissionState, handle: usize, body_id: u64,
)
    requires admission_state_wf(state),
    ensures match admission_attach_score_step(state, handle, body_id) {
        Some(next) => admission_state_wf(next)
            && next.arena == state.arena
            && next.frames[next.frames.len() - 1].score_handle == Some(handle)
            && admission_score_body_agrees(
                next.frames[next.frames.len() - 1], next.arena,
            ),
        None => true,
    },
{
    if let Some(next) = admission_attach_score_step(state, handle, body_id) {
        let index = state.frames.len() - 1;
        let before = state.frames[index];
        let after = next.frames[index];
        assert(next.frames.len() == state.frames.len());
        assert(next.arena == state.arena);
        assert(after.score_handle == Some(handle));
        assert(after.returns == before.returns);
        assert(after.return_id == before.return_id);
        assert(after.physical_start == before.physical_start);
        assert(after.program_type_id == before.program_type_id);
        assert(after.region_start == before.region_start);
        assert(after.region_end == before.region_end);
        assert(after.source_bytes == before.source_bytes);
        assert(after.cursor == before.cursor);
        assert(after.slot_count == before.slot_count);
        assert(after.last_slot == before.last_slot);
        assert(after.seen == before.seen);
        assert(after.required == before.required);
        assert(after.scalar_slots == before.scalar_slots);
        assert(after.scalar_occurrences == before.scalar_occurrences);
        assert(after.family == before.family);
        assert(after.waiting_id == before.waiting_id);
        assert(after.waiting_kind == before.waiting_kind);
        assert(after.waiting_slot == before.waiting_slot);
        assert(after.waiting_tag == before.waiting_tag);
        assert(after.frame_kind == before.frame_kind);
        assert(after.body_id == before.body_id);
        assert(after.nested_started == before.nested_started);
        assert(after.nested_resolved == before.nested_resolved);
        assert(admission_frame_wf(before));
        assert(admission_frame_wf(after));
        assert forall|i: int| 0 <= i < state.frames.len() - 1
            implies next.frames[i] == state.frames[i] by {
        }
        admission_last_frame_replacement_preserves_wf(state, next);
        assert(after.body_id == before.body_id);
        assert(after.program_type_id == before.program_type_id);
        assert(after.family == before.family);
        assert(after.seen == before.seen);
        assert(after.required == before.required);
        assert(after.scalar_slots == before.scalar_slots);
        assert(admission_family_complete(after) == admission_family_complete(before));
        assert(next.arena[handle as int].encoded_len > 0);
        assert(next.arena[handle as int].transcript_bytes.len()
            == next.arena[handle as int].encoded_len);
        assert(next.arena[handle as int].semantic == admission_family_complete(after));
        assert(admission_score_origin_matches(after, next.arena, handle));
        assert(admission_score_body_agrees(after, next.arena));
    }
}

pub proof fn admission_record_score_refines(
    state: AdmissionState, encoded_len: usize, body_id: u64,
)
    ensures match admission_record_score_step(state, encoded_len, body_id) {
        Some(next) => {
            &&& next.arena.len() == state.arena.len() + 1
            &&& admission_score_body_agrees(
                next.frames[next.frames.len() - 1], next.arena
            )
            &&& next.frames[next.frames.len() - 1].score_handle == Some(state.arena.len() as usize)
        },
        None => true,
    },
{
    if let Some(next) = admission_record_score_step(state, encoded_len, body_id) {
        let index = state.frames.len() - 1;
        let top = state.frames[index];
        let handle = state.arena.len();
        assert(state.frames.len() > 0);
        assert(state.arena.len() < usize::MAX as int);
        assert(next.frames.len() == state.frames.len());
        assert(next.arena.len() == handle + 1);
        assert(next.frames[index].score_handle == Some(handle as usize));
        assert(next.arena[handle as int].body_id == top.body_id);
        assert(next.arena[handle as int].semantic == admission_family_complete(top));
        assert(next.frames[index].seen == top.seen);
        assert(next.frames[index].required == top.required);
        assert(next.frames[index].scalar_slots == top.scalar_slots);
        assert(next.frames[index].returns == top.returns);
        assert(next.frames[index].family == top.family);
        assert(admission_family_complete(next.frames[index]) == admission_family_complete(top));
        assert(admission_score_body_agrees(next.frames[index], next.arena));
    }
}

pub proof fn admission_record_score_preserves_wf(
    state: AdmissionState, encoded_len: usize, body_id: u64,
)
    requires admission_state_wf(state),
    ensures match admission_record_score_step(state, encoded_len, body_id) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if let Some(next) = admission_record_score_step(state, encoded_len, body_id) {
        let top = state.frames[state.frames.len() - 1];
        let binding = AdmissionScoreBinding {
            body_id,
            semantic: admission_family_complete(top),
            encoded_len,
            transcript_bytes: Seq::new(encoded_len as nat, |i: int| 0u8),
        };
        if let Some(appended) = admission_append_score_entry_step(state, binding) {
            admission_append_score_entry_preserves_wf(state, binding);
            assert(admission_state_wf(appended));
            assert(admission_attach_score_step(
                appended, state.arena.len() as usize, body_id,
            ) == Some(next));
            admission_attach_score_preserves_wf(
                appended, state.arena.len() as usize, body_id,
            );
        } else {
            assert(false);
        }
    }
}

/// A node score is recorded only at the exact checked length of its existing
/// arena children, matching `CanonicalScoreArena::node` and `push`.
pub open spec fn admission_record_node_score_step(
    state: AdmissionState, children: Seq<usize>, body_id: u64,
) -> Option<AdmissionState> {
    if !admission_node_children_valid(state.arena, children)
        || admission_node_children_length(state.arena, children) < 0
        || admission_node_children_length(state.arena, children)
            > usize::MAX as int - 2
    {
        None
    } else {
        admission_record_score_step(
            state,
            (2 + admission_node_children_length(state.arena, children)) as usize,
            body_id,
        )
    }
}

pub proof fn admission_record_node_score_refines(
    state: AdmissionState, children: Seq<usize>, body_id: u64,
)
    requires admission_state_wf(state),
    ensures match admission_record_node_score_step(state, children, body_id) {
        Some(next) => {
            &&& admission_state_wf(next)
            &&& next.arena.len() == state.arena.len() + 1
            &&& next.arena[state.arena.len() as int].encoded_len as int == 2
                + admission_node_children_length(state.arena, children)
            &&& admission_score_body_agrees(
                next.frames[next.frames.len() - 1], next.arena
            )
        },
        None => true,
    },
{
    if admission_node_children_valid(state.arena, children)
        && 0 <= admission_node_children_length(state.arena, children)
        && admission_node_children_length(state.arena, children)
            <= usize::MAX as int - 2
    {
        let encoded_len = (2 + admission_node_children_length(state.arena, children)) as usize;
        admission_record_score_refines(state, encoded_len, body_id);
        admission_record_score_preserves_wf(state, encoded_len, body_id);
        if let Some(next) = admission_record_node_score_step(state, children, body_id) {
            assert(next.arena[state.arena.len() as int].encoded_len == encoded_len);
            assert(admission_state_wf(next));
            assert(admission_score_body_agrees(next.frames[next.frames.len() - 1], next.arena));
        }
    }
}

/// `CanonicalScoreArena::integer` emits exactly ten score bytes for every
/// signed 64-bit value. The value affects the bytes, not their encoded width.
pub open spec fn admission_record_integer_score_step(
    state: AdmissionState, value: i64, body_id: u64,
) -> Option<AdmissionState> {
    admission_record_score_step(state, 10, body_id)
}

pub proof fn admission_record_integer_score_refines(
    state: AdmissionState, value: i64, body_id: u64,
)
    requires admission_state_wf(state),
    ensures match admission_record_integer_score_step(state, value, body_id) {
        Some(next) => {
            &&& admission_state_wf(next)
            &&& next.arena[state.arena.len() as int].encoded_len == 10
            &&& admission_score_body_agrees(
                next.frames[next.frames.len() - 1], next.arena
            )
        },
        None => true,
    },
{
    admission_record_score_refines(state, 10, body_id);
    admission_record_score_preserves_wf(state, 10, body_id);
}

/// `CanonicalScoreArena::bytes` and `string` escape each zero byte once,
/// then add four framing bytes. This count is over the actual input sequence.
pub open spec fn admission_zero_bytes(bytes: Seq<u8>) -> int
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        0
    } else {
        (if bytes[0] == 0 { 1int } else { 0int })
            + admission_zero_bytes(bytes.subrange(1, bytes.len() as int))
    }
}

pub open spec fn admission_record_bytes_score_step(
    state: AdmissionState, bytes: Seq<u8>, body_id: u64,
) -> Option<AdmissionState> {
    let encoded_len = 4 + bytes.len() as int + admission_zero_bytes(bytes);
    if encoded_len < 4 || encoded_len > usize::MAX as int {
        None
    } else {
        admission_record_score_step(state, encoded_len as usize, body_id)
    }
}

pub proof fn admission_record_bytes_score_refines(
    state: AdmissionState, bytes: Seq<u8>, body_id: u64,
)
    requires admission_state_wf(state),
    ensures match admission_record_bytes_score_step(state, bytes, body_id) {
        Some(next) => {
            &&& admission_state_wf(next)
            &&& next.arena[state.arena.len() as int].encoded_len as int
                == 4 + bytes.len() as int + admission_zero_bytes(bytes)
            &&& admission_score_body_agrees(
                next.frames[next.frames.len() - 1], next.arena
            )
        },
        None => true,
    },
{
    let encoded_len = 4 + bytes.len() as int + admission_zero_bytes(bytes);
    if 4 <= encoded_len <= usize::MAX as int {
        admission_record_score_refines(state, encoded_len as usize, body_id);
        admission_record_score_preserves_wf(state, encoded_len as usize, body_id);
    }
}

pub open spec fn admission_complete_child_step(
    state: AdmissionState, returned_id: u64, returned_kind: u8,
    summary: AdmissionReturnedKind,
) -> Option<AdmissionState> {
    if state.frames.len() <= 1 || state.completed == usize::MAX {
        None
    } else {
        let child = state.frames[state.frames.len() - 1];
        let parent_index = state.frames.len() - 2;
        let parent = state.frames[parent_index];
        let map_value_return = parent.frame_kind == 2
            && parent.family == AdmissionFamily::Other
            && parent.program_type_id == 1
            && parent.slot_count == 2
            && parent.waiting_slot == Some(1)
            && parent.waiting_tag == Some(2)
            && child.frame_kind == 1
            && child.program_type_id == 1
            && match summary {
                AdmissionReturnedKind::Par { semantic_default, .. } => !semantic_default,
                _ => false,
            };
        if (parent.frame_kind != 1 && !map_value_return)
            || child.return_id != Some(returned_id)
            || child.frame_kind != returned_kind
            || child.cursor != child.region_end
            || child.waiting_id.is_some()
            || child.nested_started != child.nested_resolved
            || !admission_frame_wf(child)
            || !admission_family_admissible(child)
            || !admission_child_summary_agrees(child, summary, state.arena)
            || !admission_arm_child_summary_agrees(parent, child, summary, state.arena)
            || parent.waiting_id != Some(returned_id)
            || parent.waiting_kind != returned_kind
            || (match parent.waiting_slot {
                Some(slot) => !admission_return_matches(parent.family, slot, summary),
                None => true,
            })
            || (returned_kind == 3 && parent.nested_resolved == u64::MAX)
        {
            None
        } else {
            let slot = parent.waiting_slot.unwrap();
            let parent_next_frame = AdmissionFrameState {
                physical_source: parent.physical_source,
                physical_start: parent.physical_start,
                program_type_id: parent.program_type_id,
                region_start: parent.region_start,
                region_end: parent.region_end,
                source_bytes: parent.source_bytes,
                cursor: parent.cursor,
                slot_count: parent.slot_count,
                last_slot: parent.last_slot,
                seen: parent.seen,
                required: parent.required,
                scalar_slots: parent.scalar_slots,
                scalar_occurrences: parent.scalar_occurrences,
                returns: parent.returns.push(AdmissionReturnedSummary {
                    slot,
                    tag: parent.waiting_tag.unwrap(),
                    body_id: child.body_id,
                    body_bytes: child.source_bytes,
                    score_handle: child.score_handle,
                    score_origin_body_id: match child.score_handle {
                        Some(handle) => if (handle as int) < state.arena.len() {
                            Some(state.arena[handle as int].body_id)
                        } else { None },
                        None => None,
                    },
                    kind: summary,
                }),
                family: parent.family,
                waiting_id: None,
                waiting_kind: 0,
                waiting_slot: None,
                waiting_tag: None,
                return_id: parent.return_id,
                frame_kind: parent.frame_kind,
                body_id: parent.body_id,
                score_handle: parent.score_handle,
                nested_started: parent.nested_started,
                nested_resolved: if returned_kind == 3 {
                    (parent.nested_resolved as int + 1) as u64
                } else {
                    parent.nested_resolved
                },
            };
            Some(AdmissionState {
                frames: state.frames.subrange(0, state.frames.len() - 1)
                    .update(parent_index, parent_next_frame),
                arena: state.arena,
                root_end: state.root_end,
                pushed: state.pushed,
                completed: (state.completed as int + 1) as usize,
                next_obligation_id: state.next_obligation_id,
            })
        }
    }
}

pub proof fn admission_complete_child_preserves_par_source_trace(
    state: AdmissionState, returned_id: u64, returned_kind: u8,
    summary: AdmissionReturnedKind,
)
    requires admission_par_source_trace_invariant(state),
    ensures match admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ) {
        None => true,
        Some(next) => admission_par_source_trace_invariant(next),
    },
{
    if let Some(next) = admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ) {
        let child = state.frames.last();
        let parent = state.frames[state.frames.len() - 2];
        let parent_after = next.frames.last();
        admission_completed_child_summary_has_source_identity(state, summary);
        if parent.program_type_id == 1 && parent.frame_kind == 1
            && parent.family == AdmissionFamily::Other
        {
            let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
                admission_par_source_prefix_fields(parent, fields);
            assert(admission_par_source_prefix_fields(parent_after, fields));
        }
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_par_source_prefix(next.frames[i]) by {
            if i < next.frames.len() - 1 {
                assert(next.frames[i] == state.frames[i]);
            }
        }
        let new_return = parent_after.returns.last();
        assert(new_return.kind == summary);
        assert(new_return.body_bytes == child.source_bytes);
        assert(admission_returned_par_identity_exists(new_return));
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_source_identity_in_returns(next.frames[i]) by {
            if i < next.frames.len() - 1 {
                assert(next.frames[i] == state.frames[i]);
            } else {
                assert forall|j: int| 0 <= j < parent_after.returns.len()
                    implies admission_returned_par_identity_exists(
                        parent_after.returns[j],
                    ) by {
                    if j < parent.returns.len() {
                        assert(parent_after.returns[j] == parent.returns[j]);
                    } else {
                        assert(j == parent.returns.len());
                        assert(parent_after.returns[j] == new_return);
                    }
                }
            }
        }
    }
}

pub proof fn admission_complete_child_refines(
    state: AdmissionState, returned_id: u64, returned_kind: u8,
    summary: AdmissionReturnedKind,
)
    ensures match admission_complete_child_step(state, returned_id, returned_kind, summary) {
        Some(next) => {
            &&& next.frames.len() + 1 == state.frames.len()
            &&& next.completed as int == state.completed as int + 1
            &&& next.frames[next.frames.len() - 1].waiting_id.is_none()
            &&& next.frames[next.frames.len() - 1].waiting_kind == 0
            &&& next.frames[next.frames.len() - 1].waiting_slot.is_none()
            &&& next.frames[next.frames.len() - 1].returns.len()
                == state.frames[state.frames.len() - 2].returns.len() + 1
            &&& next.pushed == state.pushed
        },
        None => true,
    },
{}

pub proof fn admission_complete_child_preserves_wf(
    state: AdmissionState, returned_id: u64, returned_kind: u8,
    summary: AdmissionReturnedKind,
)
    requires admission_state_wf(state),
    ensures match admission_complete_child_step(state, returned_id, returned_kind, summary) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if let Some(next) = admission_complete_child_step(
        state, returned_id, returned_kind, summary,
    ) {
        let parent_index = state.frames.len() - 2;
        let parent = state.frames[parent_index];
        let child = state.frames[parent_index + 1];
        let next_parent = next.frames[parent_index];
        let slot = parent.waiting_slot.unwrap();
        assert(admission_frame_wf(parent));
        assert(admission_frame_wf(child));
        assert(parent.waiting_id == Some(returned_id));
        assert(parent.waiting_slot.is_some());
        assert(parent.waiting_tag.is_some());
        assert(slot < parent.slot_count);
        assert(parent.seen.contains(slot as int));
        assert(admission_return_matches(parent.family, slot, summary));
        assert(admission_child_summary_agrees(child, summary, state.arena));
        assert(next_parent.program_type_id == parent.program_type_id);
        assert(next_parent.required == parent.required);
        assert(next_parent.slot_count == parent.slot_count);
        assert(next_parent.frame_kind == parent.frame_kind);
        assert(next_parent.seen == parent.seen);
        assert(next_parent.scalar_slots == parent.scalar_slots);
        assert(next_parent.scalar_occurrences == parent.scalar_occurrences);
        assert(admission_required_slots_generated(next_parent));
        assert forall|i: int| 0 <= i < next_parent.returns.len()
            implies next_parent.returns[i].slot < next_parent.slot_count
                && next_parent.seen.contains(next_parent.returns[i].slot as int)
                && admission_return_matches(
                    next_parent.family, next_parent.returns[i].slot,
                    next_parent.returns[i].kind,
                ) by {
            if i < parent.returns.len() {
                assert(next_parent.returns[i] == parent.returns[i]);
            } else {
                assert(i == parent.returns.len());
                assert(next_parent.returns[i].slot == slot);
                assert(next_parent.returns[i].kind == summary);
            }
        }
        assert(admission_frame_wf(next_parent));
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_frame_wf(next.frames[i]) by {
            if i < parent_index {
                assert(next.frames[i] == state.frames[i]);
            } else {
                assert(i == parent_index);
                assert(next.frames[i] == next_parent);
            }
        }
        assert forall|frame_index: int, return_index: int|
            0 <= frame_index < next.frames.len()
                && 0 <= return_index < next.frames[frame_index].returns.len()
            implies next.frames[frame_index].returns[return_index].score_handle.is_some()
                == next.frames[frame_index].returns[return_index]
                    .score_origin_body_id.is_some()
                && match next.frames[frame_index].returns[return_index].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[frame_index].returns[return_index]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {
            if frame_index < parent_index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            } else if return_index < parent.returns.len() {
                assert(frame_index == parent_index);
                assert(next_parent.returns[return_index] == parent.returns[return_index]);
            } else {
                assert(frame_index == parent_index);
                assert(return_index == parent.returns.len());
                assert(next_parent.returns[return_index].score_handle
                    == child.score_handle);
                if let Some(handle) = child.score_handle {
                    assert(admission_summary_has_score(summary));
                    assert(admission_score_body_agrees(child, state.arena));
                    assert((handle as int) < state.arena.len());
                    assert(next_parent.returns[return_index].score_origin_body_id
                        == Some(state.arena[handle as int].body_id));
                } else {
                    assert(next_parent.returns[return_index].score_origin_body_id.is_none());
                }
            }
        }
        assert forall|frame_index: int| 1 <= frame_index < next.frames.len()
            implies ((next.frames[frame_index - 1].frame_kind != 1
                && next.frames[frame_index - 1].frame_kind != 2)
                || #[trigger] admission_child_physical_contained(
                    next.frames[frame_index - 1], next.frames[frame_index],
                )) by {
            if frame_index < parent_index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                assert(next.frames[frame_index] == state.frames[frame_index]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
            } else {
                assert(frame_index == parent_index);
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
                assert(next.frames[frame_index].physical_start
                    == state.frames[frame_index].physical_start);
                assert(next.frames[frame_index].region_end
                    == state.frames[frame_index].region_end);
                assert(next.frames[frame_index].source_bytes
                    == state.frames[frame_index].source_bytes);
            }
        }
        assert(admission_state_wf(next));
    }
}

pub open spec fn admission_complete_root(state: AdmissionState) -> bool {
    if state.frames.len() != 1 {
        false
    } else {
        let root = state.frames[0];
        // MUTANT_ANCHOR_SKIPPED_ROOT
        root.cursor == state.root_end
            && root.region_end == state.root_end
            && root.waiting_id.is_none()
            && root.nested_started == root.nested_resolved
            && admission_slots_complete(root)
            && admission_family_admissible(root)
            && admission_score_body_agrees(root, state.arena)
            && state.pushed as int == state.completed as int + 1
    }
}

pub proof fn admission_root_refines(state: AdmissionState)
    ensures admission_complete_root(state) ==> {
        &&& state.frames.len() == 1
        &&& state.frames[0].cursor == state.root_end
        &&& state.frames[0].waiting_id.is_none()
        &&& state.frames[0].nested_started == state.frames[0].nested_resolved
        &&& admission_slots_complete(state.frames[0])
        &&& admission_family_admissible(state.frames[0])
        &&& admission_score_body_agrees(state.frames[0], state.arena)
    },
{}

/// A finalized root can remain incomplete and use the nonsemantic score lane.
/// This is frame finalization, not a proof of all family-specific admissions.
pub proof fn admission_incomplete_root_accepts(state: AdmissionState)
    requires
        state.frames.len() == 1,
        state.frames[0].cursor == state.root_end,
        state.frames[0].region_end == state.root_end,
        state.frames[0].waiting_id.is_none(),
        state.frames[0].nested_started == state.frames[0].nested_resolved,
        admission_slots_complete(state.frames[0]),
        admission_family_admissible(state.frames[0]),
        !admission_family_complete(state.frames[0]),
        admission_score_body_agrees(state.frames[0], state.arena),
        state.pushed as int == state.completed as int + 1,
    ensures
        admission_complete_root(state),
        !state.arena[state.frames[0].score_handle.unwrap() as int].semantic,
{}

/// An empty Send has no channel summary but no missing wire-required slot.
/// It witnesses production-shaped incomplete/nonsemantic finalization.
pub open spec fn admission_incomplete_root_example() -> AdmissionState {
    let root = AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: 15,
        region_start: 0,
        region_end: 0,
        source_bytes: Seq::empty(),
        cursor: 0,
        slot_count: 6,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: AdmissionFamily::Send,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 1,
        body_id: 7,
        score_handle: Some(0),
        nested_started: 0,
        nested_resolved: 0,
    };
    AdmissionState {
        frames: Seq::empty().push(root),
        arena: Seq::empty().push(AdmissionScoreBinding {
            body_id: 7,
            semantic: false,
            encoded_len: 10,
            transcript_bytes: Seq::new(10nat, |i: int| 0u8),
        }),
        root_end: 0,
        pushed: 1,
        completed: 0,
        next_obligation_id: 0,
    }
}

pub proof fn admission_incomplete_root_example_refines()
    ensures
        admission_state_wf(admission_incomplete_root_example()),
        !admission_family_complete(admission_incomplete_root_example().frames[0]),
        admission_complete_root(admission_incomplete_root_example()),
{
    let state = admission_incomplete_root_example();
    let root = state.frames[0];
    assert(!root.seen.contains(0));
    assert(admission_slots_complete(root));
    assert(!admission_family_complete(root));
    assert(admission_score_body_agrees(root, state.arena));
    admission_incomplete_root_accepts(state);
}

/// A mismatched lane cannot be accepted merely because the root consumed its
/// region: the selected arena score must agree with the derived model lane.
pub proof fn admission_root_rejects_wrong_score_lane(state: AdmissionState)
    requires
        state.frames.len() == 1,
        state.frames[0].score_handle.is_some(),
        (state.frames[0].score_handle.unwrap() as int) < state.arena.len(),
        state.arena[state.frames[0].score_handle.unwrap() as int].semantic
            != admission_family_complete(state.frames[0]),
    ensures !admission_complete_root(state),
{}

pub open spec fn admission_wrong_lane_root_example() -> AdmissionState {
    let state = admission_incomplete_root_example();
    AdmissionState {
        frames: state.frames,
        arena: Seq::empty().push(AdmissionScoreBinding {
            body_id: 7,
            semantic: true,
            encoded_len: 10,
            transcript_bytes: Seq::new(10nat, |i: int| 0u8),
        }),
        root_end: state.root_end,
        pushed: state.pushed,
        completed: state.completed,
        next_obligation_id: state.next_obligation_id,
    }
}

pub proof fn admission_wrong_lane_root_example_rejected()
    ensures
        admission_state_wf(admission_wrong_lane_root_example()),
        !admission_complete_root(admission_wrong_lane_root_example()),
{
    let state = admission_wrong_lane_root_example();
    let root = state.frames[0];
    assert(!root.seen.contains(0));
    assert(!admission_family_complete(root));
    assert(state.arena[0].semantic);
    admission_root_rejects_wrong_score_lane(state);
}

pub open spec fn admission_family_witness_frame(
    family: AdmissionFamily, seen: Set<int>, returns: Seq<AdmissionReturnedSummary>,
) -> AdmissionFrameState {
    AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: admission_family_program_id(family).unwrap_or(0),
        region_start: 0,
        region_end: 0,
        source_bytes: Seq::empty(),
        cursor: 0,
        slot_count: admission_family_slot_count(family).unwrap_or(9),
        last_slot: None,
        seen,
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns,
        family,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 1,
        body_id: 7,
        score_handle: Some(0),
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn admission_witness_return(
    slot: usize, kind: AdmissionReturnedKind,
) -> AdmissionReturnedSummary {
    AdmissionReturnedSummary {
        slot,
        tag: 0,
        body_id: 7,
        body_bytes: Seq::empty(),
        score_handle: if admission_summary_has_score(kind) { Some(0) } else { None },
        score_origin_body_id: if admission_summary_has_score(kind) {
            Some(7)
        } else { None },
        kind,
    }
}

pub open spec fn admission_family_witness_state(
    frame: AdmissionFrameState,
) -> AdmissionState {
    AdmissionState {
        frames: Seq::empty().push(frame),
        arena: Seq::empty().push(AdmissionScoreBinding {
            body_id: frame.body_id,
            semantic: admission_family_complete(frame),
            encoded_len: 10,
            transcript_bytes: Seq::new(10nat, |i: int| 0u8),
        }),
        root_end: frame.region_end,
        pushed: 1,
        completed: 0,
        next_obligation_id: 0,
    }
}

pub open spec fn admission_fabricated_child_state(
    parent_family: AdmissionFamily, slot: usize, child_family: AdmissionFamily,
    field_tag: u32,
) -> AdmissionState {
    let parent = AdmissionFrameState {
        last_slot: Some(slot),
        seen: Set::empty().insert(slot as int),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(slot),
        waiting_tag: Some(field_tag),
        score_handle: None,
        ..admission_family_witness_frame(parent_family, Set::empty(), Seq::empty())
    };
    let child = AdmissionFrameState {
        return_id: Some(0),
        body_id: 8,
        score_handle: None,
        ..admission_family_witness_frame(child_family, Set::empty(), Seq::empty())
    };
    AdmissionState {
        frames: Seq::empty().push(parent).push(child),
        arena: Seq::empty(),
        root_end: 0,
        pushed: 2,
        completed: 0,
        next_obligation_id: 1,
    }
}

pub proof fn admission_fabricated_child_physical_contained(
    parent_family: AdmissionFamily, slot: usize, child_family: AdmissionFamily,
    field_tag: u32,
)
    ensures admission_child_physical_contained(
        admission_fabricated_child_state(parent_family, slot, child_family, field_tag).frames[0],
        admission_fabricated_child_state(parent_family, slot, child_family, field_tag).frames[1],
    ),
{
    let state = admission_fabricated_child_state(
        parent_family, slot, child_family, field_tag,
    );
    assert(state.frames[0].physical_start == 0);
    assert(state.frames[1].physical_start == 0);
    assert(state.frames[0].region_end == 0);
    assert(state.frames[1].region_end == 0);
    assert(state.frames[0].source_bytes == Seq::<u8>::empty());
    assert(state.frames[1].source_bytes == Seq::<u8>::empty());
    assert(state.frames[0].source_bytes.subrange(0, 0) == Seq::<u8>::empty());
}

pub proof fn admission_fabricated_child_counterexamples()
{
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 19 || program_type_id == 16
            || program_type_id == 22 || program_type_id == 21
            || program_type_id == 24 || program_type_id == 35
            || program_type_id == 25 || program_type_id == 13
            || program_type_id == 9 || program_type_id == 0)
        implies #[trigger] admission_descriptor_not_pathmap(program_type_id, tag) by {
        if let Some(descriptor) = generated::generated_descriptor(program_type_id, tag) {
            assert(descriptor.frame_kind != 3);
        }
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 19 || program_type_id == 16
            || program_type_id == 22 || program_type_id == 21
            || program_type_id == 24 || program_type_id == 35
            || program_type_id == 25 || program_type_id == 13
            || program_type_id == 9 || program_type_id == 0)
        implies #[trigger] admission_no_required_descriptor_slot(program_type_id, slot) by {
        assert forall|tag: u32|
            !#[trigger] admission_required_descriptor_slot(program_type_id, tag, slot) by {
            assert(admission_descriptor_not_pathmap(program_type_id, tag));
            if let Some(descriptor) = generated::generated_descriptor(program_type_id, tag) {
                assert(descriptor.frame_kind != 3);
            }
        }
    }
    let receive = admission_fabricated_child_state(
        AdmissionFamily::Receive, 0, AdmissionFamily::ReceiveBindFrame, 1,
    );
    let receive_child = receive.frames[1];
    assert forall|frame_index: int| 0 <= frame_index < receive.frames.len()
        implies admission_required_slots_generated(receive.frames[frame_index]) by {
        let frame = receive.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 19u16 } else { 16u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(receive.frames[0]));
    assert(admission_frame_wf(receive.frames[1]));
    admission_fabricated_child_physical_contained(
        AdmissionFamily::Receive, 0, AdmissionFamily::ReceiveBindFrame, 1,
    );
    assert(admission_child_physical_contained(receive.frames[0], receive.frames[1]));
    assert(admission_state_wf(receive));
    assert(!admission_returned_par(receive_child, 1));
    admission_rejects_fabricated_receive_source(receive_child, receive.arena);
    assert(admission_complete_child_step(
        receive, 0, 1, AdmissionReturnedKind::ReceiveBind { source_present: true }
    ).is_none());
    assert(admission_complete_child_step(
        receive, 0, 1, AdmissionReturnedKind::ReceiveBind { source_present: false }
    ).is_some());

    let matching = admission_fabricated_child_state(
        AdmissionFamily::Match, 1, AdmissionFamily::MatchCaseFrame, 2,
    );
    let match_child = matching.frames[1];
    assert forall|frame_index: int| 0 <= frame_index < matching.frames.len()
        implies admission_required_slots_generated(matching.frames[frame_index]) by {
        let frame = matching.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 22u16 } else { 21u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(matching.frames[0]));
    assert(admission_frame_wf(matching.frames[1]));
    assert(admission_child_physical_contained(matching.frames[0], matching.frames[1]));
    assert(admission_state_wf(matching));
    assert(!admission_returned_par(match_child, 0));
    admission_rejects_fabricated_match_case(match_child, matching.arena);
    assert(admission_complete_child_step(
        matching, 0, 1, AdmissionReturnedKind::MatchCase {
            required_present: true, canonical_fixed_point: true,
        }
    ).is_none());
    assert(admission_complete_child_step(
        matching, 0, 1, AdmissionReturnedKind::MatchCase {
            required_present: false, canonical_fixed_point: false,
        }
    ).is_none());
    assert(admission_complete_child_step(
        matching, 0, 1, AdmissionReturnedKind::MatchCase {
            required_present: false, canonical_fixed_point: true,
        }
    ).is_some());

    let evar = admission_fabricated_child_state(
        AdmissionFamily::Expr, 0, AdmissionFamily::EVarFrame, 19,
    );
    assert forall|frame_index: int| 0 <= frame_index < evar.frames.len()
        implies admission_required_slots_generated(evar.frames[frame_index]) by {
        let frame = evar.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { 35u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(evar.frames[0]));
    assert(admission_frame_wf(evar.frames[1]));
    assert(admission_child_physical_contained(evar.frames[0], evar.frames[1]));
    assert(admission_state_wf(evar));
    assert(!admission_returned_var(evar.frames[1], 0));
    assert(!admission_arm_child_summary_agrees(
        evar.frames[0], evar.frames[1],
        AdmissionReturnedKind::ExprPayload {
            score_present: true, canonical_fixed_point: true,
            canonical_path_stable: false,
        },
        evar.arena,
    ));
    assert(admission_complete_child_step(
        evar, 0, 1, AdmissionReturnedKind::ExprPayload {
            score_present: true, canonical_fixed_point: true,
            canonical_path_stable: false,
        }
    ).is_none());
    assert(admission_complete_child_step(
        evar, 0, 1, AdmissionReturnedKind::BigRational {
            numerator: Seq::empty(), denominator: Seq::empty(),
        }
    ).is_none());
    assert(admission_complete_child_step(
        evar, 0, 1, AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        }
    ).is_some());

    let big_rat = admission_fabricated_child_state(
        AdmissionFamily::Expr, 0, AdmissionFamily::BigRationalFrame, 36,
    );
    assert forall|frame_index: int| 0 <= frame_index < big_rat.frames.len()
        implies admission_required_slots_generated(big_rat.frames[frame_index]) by {
        let frame = big_rat.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { 25u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(big_rat.frames[0]));
    assert(admission_frame_wf(big_rat.frames[1]));
    assert(admission_child_physical_contained(big_rat.frames[0], big_rat.frames[1]));
    assert(admission_state_wf(big_rat));
    assert(admission_complete_child_step(
        big_rat, 0, 1, AdmissionReturnedKind::BigRational {
            numerator: Seq::empty(), denominator: Seq::empty(),
        }
    ).is_some());

    let cost = admission_fabricated_child_state(
        AdmissionFamily::CostTokenStack, 0, AdmissionFamily::Other, 1,
    );
    assert forall|frame_index: int| 0 <= frame_index < cost.frames.len()
        implies admission_required_slots_generated(cost.frames[frame_index]) by {
        let frame = cost.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 13u16 } else { 0u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(cost.frames[0]));
    assert(admission_frame_wf(cost.frames[1]));
    assert(admission_child_physical_contained(cost.frames[0], cost.frames[1]));
    assert(admission_state_wf(cost));
    assert(!admission_child_summary_agrees(
        cost.frames[1], AdmissionReturnedKind::CostScore, cost.arena
    ));
    assert(admission_complete_child_step(
        cost, 0, 1, AdmissionReturnedKind::CostScore
    ).is_none());

    let unscored_cost = admission_fabricated_child_state(
        AdmissionFamily::CostTokenStack, 0, AdmissionFamily::CostSignatureFrame, 1,
    );
    assert forall|frame_index: int| 0 <= frame_index < unscored_cost.frames.len()
        implies admission_required_slots_generated(unscored_cost.frames[frame_index]) by {
        let frame = unscored_cost.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 13u16 } else { 9u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission_frame_wf(unscored_cost.frames[0]));
    assert(admission_frame_wf(unscored_cost.frames[1]));
    assert(admission_child_physical_contained(
        unscored_cost.frames[0], unscored_cost.frames[1],
    ));
    assert(admission_state_wf(unscored_cost));
    assert(admission_complete_child_step(
        unscored_cost, 0, 1, AdmissionReturnedKind::CostScore
    ).is_none());
}

pub proof fn admission_first_par_return(frame: AdmissionFrameState, slot: usize)
    requires
        frame.returns.len() > 0,
        frame.returns[0].slot == slot,
        admission_is_par(frame.returns[0].kind),
    ensures admission_returned_par(frame, slot),
{
    assert(exists|i: int| 0 <= i < frame.returns.len()
        && frame.returns[i].slot == slot && admission_is_par(frame.returns[i].kind));
}

/// These finite witnesses cover every current ParComponentTag family with
/// typed returned summaries rather than a supplied `complete` bit.
pub proof fn admission_ten_family_completeness_witnesses()
{
    let par = AdmissionReturnedKind::Par {
        semantic_default: false, canonical_path_stable: false,
    };
    let send = admission_family_witness_frame(
        AdmissionFamily::Send, Set::empty().insert(0),
        Seq::empty().push(admission_witness_return(0, par)),
    );
    admission_first_par_return(send, 0);
    assert(admission_family_complete(send));

    let receive = admission_family_witness_frame(
        AdmissionFamily::Receive, Set::empty().insert(0).insert(1),
        Seq::empty()
            .push(admission_witness_return(0, AdmissionReturnedKind::ReceiveBind {
                source_present: false,
            }))
            .push(admission_witness_return(1, par)),
    );
    assert(receive.returns[0].slot == 0);
    assert(!admission_all_receive_sources(receive));
    assert(!admission_family_complete(receive));
    assert(admission_frame_wf(receive));
    assert(admission_complete_root(admission_family_witness_state(receive)));
    assert(!admission_family_witness_state(receive).arena[0].semantic);

    let new = admission_family_witness_frame(
        AdmissionFamily::New, Set::empty().insert(1),
        Seq::empty().push(admission_witness_return(1, par)),
    );
    admission_first_par_return(new, 1);
    assert(admission_family_complete(new));

    let matching = admission_family_witness_frame(
        AdmissionFamily::Match, Set::empty().insert(0).insert(1),
        Seq::empty().push(admission_witness_return(0, par))
            .push(admission_witness_return(1, AdmissionReturnedKind::MatchCase {
                required_present: false, canonical_fixed_point: true,
            })),
    );
    admission_first_par_return(matching, 0);
    assert(matching.returns[1].slot == 1);
    assert(!admission_all_match_cases_required(matching));
    assert(!admission_family_complete(matching));

    let bundle = admission_family_witness_frame(
        AdmissionFamily::Bundle, Set::empty().insert(0),
        Seq::empty().push(admission_witness_return(0, par)),
    );
    admission_first_par_return(bundle, 0);
    assert(admission_family_complete(bundle));

    let conditional = admission_family_witness_frame(
        AdmissionFamily::If, Set::empty().insert(0).insert(1),
        Seq::empty().push(admission_witness_return(0, par))
            .push(admission_witness_return(1, par)),
    );
    admission_first_par_return(conditional, 0);
    assert(!admission_family_complete(conditional));

    let cost_stack = admission_family_witness_frame(
        AdmissionFamily::CostTokenStack, Set::empty(), Seq::empty(),
    );
    assert(admission_family_complete(cost_stack));
    assert(admission_frame_wf(cost_stack));
    assert(admission_complete_root(admission_family_witness_state(cost_stack)));
    assert(admission_family_witness_state(cost_stack).arena[0].semantic);

    let expr = admission_family_witness_frame(
        AdmissionFamily::Expr, Set::empty().insert(0),
        Seq::empty().push(admission_witness_return(0, AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        })),
    );
    assert(!admission_family_complete(expr));
    assert(admission_frame_wf(expr));
    assert(admission_complete_root(admission_family_witness_state(expr)));
    assert(!admission_family_witness_state(expr).arena[0].semantic);

    let connective = admission_family_witness_frame(
        AdmissionFamily::Connective, Set::empty(), Seq::empty(),
    );
    assert(admission_family_complete(connective));

    let unforgeable = admission_family_witness_frame(
        AdmissionFamily::GUnforgeable, Set::empty(), Seq::empty(),
    );
    assert(admission_family_complete(unforgeable));
}

/// Source-shaped projection of `IndexedCanonicalFieldCursor::next_field_inner`.
/// Offsets are relative to the owned message region, as in `encoded_range`.
/// The width arithmetic is the postcondition of `checked_field_end` above;
/// this layer does not claim to verify the byte decoder itself.
pub struct IndexedCanonicalFieldModel {
    pub number: u32,
    pub encoded_start: usize,
    pub encoded_end: usize,
    pub payload_start: usize,
    pub payload_end: usize,
    pub key_width: usize,
    pub payload_width: usize,
    pub payload_length: u64,
    pub wire: u8,
    pub decoded_varint: u64,
    pub ghost payload_bytes: Seq<u8>,
}

pub struct AdmissionSourceAnchor {
    pub source: usize,
    pub offset: usize,
}

pub struct AdmissionIndexedSourceImage {
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<AdmissionSourceAnchor>,
    pub end: AdmissionSourceAnchor,
}

pub open spec fn admission_indexed_source_wf(
    image: AdmissionIndexedSourceImage,
) -> bool {
    image.positions.len() == image.bytes.len()
}

pub open spec fn admission_indexed_source_position(
    image: AdmissionIndexedSourceImage, at: usize,
) -> Option<AdmissionSourceAnchor> {
    if !admission_indexed_source_wf(image) || at as int > image.bytes.len() {
        None
    } else if at as int == image.bytes.len() {
        Some(image.end)
    } else {
        Some(image.positions[at as int])
    }
}

pub open spec fn admission_indexed_source_subview(
    image: AdmissionIndexedSourceImage, start: usize, end: usize,
) -> Option<AdmissionIndexedSourceImage> {
    if !admission_indexed_source_wf(image)
        || start > end || end as int > image.bytes.len()
    {
        None
    } else {
        Some(AdmissionIndexedSourceImage {
            bytes: image.bytes.subrange(start as int, end as int),
            positions: image.positions.subrange(start as int, end as int),
            end: admission_indexed_source_position(image, end).unwrap(),
        })
    }
}

/// Source-shaped facts at a CanonicalPath `step()` that has consumed one or
/// more input bytes and returned `Push`. The reducer obligation ID belongs to
/// the path reducer; `stack_return_id` belongs to the admission worklist.
/// Neither is inferred from the other. The payload end is explicit because an
/// empty opaque payload can end at its length-byte anchor.
pub enum AdmissionCanonicalPathObligationKind { RawParEscape, NestedEpm1 }

pub struct AdmissionCanonicalPathPushSource {
    pub ghost parent: AdmissionIndexedSourceImage,
    pub ghost child: AdmissionIndexedSourceImage,
    pub payload_start: usize,
    pub payload_end: usize,
    pub terminal_anchor: AdmissionSourceAnchor,
    pub cursor_after_step: usize,
    pub reducer_obligation_id: u64,
    pub stack_return_id: u64,
    pub kind: AdmissionCanonicalPathObligationKind,
}

/// The scalar checks made by `FramePushObservation::after_push`. This is a
/// supplied observation, not an extraction theorem for the production Vec.
pub struct AdmissionCanonicalPathPushStorage {
    pub before_len: usize,
    pub before_capacity: usize,
    pub after_len: usize,
    pub after_capacity: usize,
    pub frame_variant_bytes: usize,
    pub stats_frame_variant_bytes: usize,
    pub used_bytes: usize,
    pub reserved_bytes: usize,
    pub observed_top_kind: u8,
}

pub open spec fn admission_canonical_path_slotless(frame: AdmissionFrameState) -> bool {
    frame.family == AdmissionFamily::Other
        && frame.slot_count == 0
        && frame.last_slot.is_none()
        && frame.seen == Set::empty()
        && frame.required == Set::empty()
        && frame.scalar_slots == Set::empty()
        && frame.scalar_occurrences.len() == 0
        && frame.returns.len() == 0
        && frame.waiting_slot.is_none()
        && frame.waiting_tag.is_none()
        && frame.score_handle.is_none()
}

pub open spec fn admission_canonical_path_push_source_wf(
    state: AdmissionState, source: AdmissionCanonicalPathPushSource,
    child: AdmissionFrameState,
) -> bool {
    if state.frames.len() == 0 { false } else {
        let parent = state.frames[state.frames.len() - 1];
        let first_parent = admission_indexed_source_position(source.parent, 0);
        let first_child = admission_indexed_source_position(source.child, 0);
        &&& parent.frame_kind == 4
        &&& admission_canonical_path_slotless(parent)
        &&& parent.program_type_id == 0
        &&& parent.waiting_id.is_none()
        &&& parent.waiting_kind == 0
        &&& parent.source_bytes == source.parent.bytes
        &&& admission_indexed_source_wf(source.parent)
        &&& first_parent.is_some()
        &&& first_parent.unwrap().source == parent.physical_source
        &&& first_parent.unwrap().offset == parent.physical_start
        &&& parent.cursor < source.cursor_after_step
        &&& source.cursor_after_step <= parent.region_end
        &&& source.payload_start <= source.payload_end
        &&& source.payload_end <= source.cursor_after_step
        &&& source.child.bytes == source.parent.bytes.subrange(
            source.payload_start as int, source.payload_end as int,
        )
        &&& source.child.positions == source.parent.positions.subrange(
            source.payload_start as int, source.payload_end as int,
        )
        &&& source.child.end == source.terminal_anchor
        &&& admission_indexed_source_wf(source.child)
        &&& first_child.is_some()
        &&& child.physical_source == first_child.unwrap().source
        &&& child.physical_start == first_child.unwrap().offset
        &&& child.region_start == 0
        &&& child.region_end as int == source.child.bytes.len()
        &&& child.source_bytes == source.child.bytes
        &&& child.cursor == 0
        &&& child.family == AdmissionFamily::Other
        &&& match source.kind {
            AdmissionCanonicalPathObligationKind::RawParEscape =>
                child.frame_kind == 1 && child.program_type_id == 1
                    && generated::generated_slot_count(1) == Some(child.slot_count),
            AdmissionCanonicalPathObligationKind::NestedEpm1 =>
                child.frame_kind == 3 && child.program_type_id == 1
                    && admission_canonical_path_slotless(child),
        }
        &&& child.waiting_id.is_none()
        &&& child.waiting_kind == 0
        &&& child.return_id.is_none()
        &&& child.last_slot.is_none()
        &&& child.seen == Set::empty()
        &&& child.scalar_slots == Set::empty()
        &&& child.scalar_occurrences.len() == 0
        &&& child.returns.len() == 0
        &&& child.score_handle.is_none()
        &&& child.nested_started == 0
        &&& child.nested_resolved == 0
        &&& admission_frame_wf(child)
        &&& source.stack_return_id == state.next_obligation_id
    }
}

pub open spec fn admission_canonical_path_push_storage_wf(
    state: AdmissionState, child: AdmissionFrameState,
    storage: AdmissionCanonicalPathPushStorage,
) -> bool {
    &&& storage.before_len as int == state.frames.len()
    &&& storage.before_len <= storage.before_capacity
    &&& storage.after_len as int == storage.before_len as int + 1
    &&& storage.after_capacity >= storage.before_capacity
    &&& storage.after_capacity >= storage.after_len
    &&& storage.frame_variant_bytes > 0
    &&& storage.stats_frame_variant_bytes == storage.frame_variant_bytes
    &&& storage.after_len as int * storage.frame_variant_bytes as int <= usize::MAX as int
    &&& storage.after_capacity as int * storage.frame_variant_bytes as int
        <= usize::MAX as int
    &&& storage.used_bytes as int
        == storage.after_len as int * storage.frame_variant_bytes as int
    &&& storage.reserved_bytes as int
        == storage.after_capacity as int * storage.frame_variant_bytes as int
    &&& storage.observed_top_kind == child.frame_kind
}

/// One driver macro-action after `CanonicalPathFrame::step` advances its
/// cursor. It has no protobuf slot or tag: only the path-reducer obligation
/// sidecar and the independent stack return ID are published. The full child
/// position image stays in `source`, since AdmissionFrameState stores only
/// bytes and the first physical anchor.
pub open spec fn admission_canonical_path_push_indexed_step(
    state: AdmissionState, source: AdmissionCanonicalPathPushSource,
    child: AdmissionFrameState, storage: AdmissionCanonicalPathPushStorage,
) -> Option<AdmissionState> {
    if !admission_canonical_path_push_source_wf(state, source, child)
        || !admission_canonical_path_push_storage_wf(state, child, storage)
        || state.next_obligation_id == u64::MAX
        || state.pushed == usize::MAX
        || (child.frame_kind == 3
            && state.frames[state.frames.len() - 1].nested_started == u64::MAX)
    { None } else {
        let index = state.frames.len() - 1;
        let parent = state.frames[index];
        let next_parent = AdmissionFrameState {
            cursor: source.cursor_after_step,
            waiting_id: Some(source.stack_return_id),
            waiting_kind: child.frame_kind,
            nested_started: if child.frame_kind == 3 {
                (parent.nested_started as int + 1) as u64
            } else { parent.nested_started },
            ..parent
        };
        let next_child = AdmissionFrameState {
            return_id: Some(source.stack_return_id),
            body_id: (source.stack_return_id as int + 1) as u64,
            ..child
        };
        Some(AdmissionState {
            frames: state.frames.update(index, next_parent).push(next_child),
            arena: state.arena,
            root_end: state.root_end,
            pushed: (state.pushed as int + 1) as usize,
            completed: state.completed,
            next_obligation_id: (state.next_obligation_id as int + 1) as u64,
        })
    }
}

pub proof fn admission_canonical_path_push_indexed_preserves_wf(
    state: AdmissionState, source: AdmissionCanonicalPathPushSource,
    child: AdmissionFrameState, storage: AdmissionCanonicalPathPushStorage,
)
    requires admission_state_wf(state),
    ensures match admission_canonical_path_push_indexed_step(
        state, source, child, storage,
    ) {
        Some(next) => {
            let parent_index = state.frames.len() - 1;
            let next_parent = next.frames[parent_index];
            let next_child = next.frames[parent_index + 1];
            &&& admission_state_wf(next)
            &&& next.frames.len() == state.frames.len() + 1
            &&& next.arena == state.arena
            &&& next_parent.frame_kind == 4
            &&& next_parent.cursor == source.cursor_after_step
            &&& next_parent.waiting_id == Some(source.stack_return_id)
            &&& next_parent.waiting_kind == child.frame_kind
            &&& next_parent.waiting_slot.is_none()
            &&& next_parent.waiting_tag.is_none()
            &&& next_parent.last_slot == state.frames[parent_index].last_slot
            &&& next_parent.seen == state.frames[parent_index].seen
            &&& next_parent.returns == state.frames[parent_index].returns
            &&& next_child.return_id == Some(source.stack_return_id)
            &&& next_child.source_bytes == source.child.bytes
            &&& next_child.physical_source
                == admission_indexed_source_position(source.child, 0).unwrap().source
            &&& next_child.physical_start
                == admission_indexed_source_position(source.child, 0).unwrap().offset
            &&& source.child.positions == source.parent.positions.subrange(
                source.payload_start as int, source.payload_end as int,
            )
            &&& source.child.end == source.terminal_anchor
            &&& (source.payload_start == source.payload_end
                ==> next_child.physical_source == source.terminal_anchor.source
                    && next_child.physical_start == source.terminal_anchor.offset)
            &&& storage.after_len as int == next.frames.len()
        },
        None => true,
    },
{
    if let Some(next) = admission_canonical_path_push_indexed_step(
        state, source, child, storage,
    ) {
        let index = state.frames.len() - 1;
        let parent = state.frames[index];
        let next_parent = next.frames[index];
        let next_child = next.frames[index + 1];
        assert(admission_canonical_path_push_source_wf(state, source, child));
        assert(admission_canonical_path_push_storage_wf(state, child, storage));
        assert(admission_frame_wf(parent));
        assert(admission_frame_wf(child));
        assert(parent.waiting_kind == 0);
        assert(parent.nested_started == parent.nested_resolved);
        assert(next_parent.waiting_slot.is_none());
        assert(next_parent.waiting_tag.is_none());
        if child.frame_kind == 3 {
            assert(next_parent.nested_started as int
                == parent.nested_started as int + 1);
            assert(next_parent.waiting_kind == 3);
        } else {
            assert(child.frame_kind == 1);
            assert(next_parent.nested_started == parent.nested_started);
            assert(next_parent.waiting_kind == 1);
        }
        assert(next_parent.nested_started as int
            == next_parent.nested_resolved as int
                + (if next_parent.waiting_kind == 3 { 1int } else { 0int }));
        assert(next_parent.source_bytes == parent.source_bytes);
        assert(next_parent.required == parent.required);
        assert(next_parent.scalar_occurrences == parent.scalar_occurrences);
        assert(next_parent.returns == parent.returns);
        assert(admission_frame_wf(next_parent));
        assert(next_child.source_bytes == child.source_bytes);
        assert(next_child.required == child.required);
        assert(next_child.scalar_occurrences == child.scalar_occurrences);
        assert(next_child.returns == child.returns);
        assert(admission_frame_wf(next_child));
        if source.payload_start == source.payload_end {
            assert(source.child.bytes.len() == 0);
            assert(admission_indexed_source_position(source.child, 0)
                == Some(source.terminal_anchor));
            assert(next_child.physical_source == source.terminal_anchor.source);
            assert(next_child.physical_start == source.terminal_anchor.offset);
        }
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_frame_wf(next.frames[i]) by {
            if i < index {
                assert(next.frames[i] == state.frames[i]);
            } else if i == index {
                assert(next.frames[i] == next_parent);
            } else {
                assert(i == index + 1);
                assert(next.frames[i] == next_child);
            }
        }
        assert forall|frame_index: int, return_index: int|
            0 <= frame_index < next.frames.len()
                && 0 <= return_index < next.frames[frame_index].returns.len()
            implies next.frames[frame_index].returns[return_index].score_handle.is_some()
                == next.frames[frame_index].returns[return_index]
                    .score_origin_body_id.is_some()
                && match next.frames[frame_index].returns[return_index].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[frame_index].returns[return_index]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {
            if frame_index < index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            } else if frame_index == index {
                assert(next.frames[frame_index].returns == parent.returns);
            } else {
                assert(frame_index == index + 1);
                assert(next_child.returns.len() == 0);
            }
        }
        assert forall|frame_index: int| 1 <= frame_index < next.frames.len()
            implies ((next.frames[frame_index - 1].frame_kind != 1
                && next.frames[frame_index - 1].frame_kind != 2)
                || #[trigger] admission_child_physical_contained(
                    next.frames[frame_index - 1], next.frames[frame_index],
                )) by {
            if frame_index < index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                assert(next.frames[frame_index] == state.frames[frame_index]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
            } else if frame_index == index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
                assert(next.frames[frame_index].physical_source
                    == state.frames[frame_index].physical_source);
                assert(next.frames[frame_index].physical_start
                    == state.frames[frame_index].physical_start);
                assert(next.frames[frame_index].region_end
                    == state.frames[frame_index].region_end);
                assert(next.frames[frame_index].source_bytes
                    == state.frames[frame_index].source_bytes);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        next.frames[frame_index - 1], next.frames[frame_index],
                    ));
                }
            } else {
                assert(frame_index == index + 1);
                assert(next_parent.frame_kind == 4);
            }
        }
        assert(admission_state_wf(next));
    }
}

/// The source-visible continuation after a CanonicalPath child completes.
/// `push` is the indexed image retained across the child lifetime. The
/// reducer's waiting ID and kind are checked separately from the worklist's
/// return ID. The EPM entry flags are a ghost source witness whose extraction
/// from the EPM reducer remains a separate composition obligation.
pub struct AdmissionCanonicalPathReturnSource {
    pub ghost push: AdmissionCanonicalPathPushSource,
    pub reducer_waiting_id: u64,
    pub reducer_waiting_kind: AdmissionCanonicalPathObligationKind,
    pub stack_return_id: u64,
    pub ghost epm_entry_stability: Seq<bool>,
}

pub enum AdmissionCanonicalPathResolution {
    RawPar { canonical_path_stable: bool },
    NestedEpm1 { mode: u8, entries_canonical_path_stable: bool },
}

pub struct AdmissionCanonicalPathReturnAfter {
    pub ghost parent: AdmissionIndexedSourceImage,
    pub cursor: usize,
    pub waiting_reducer_id: Option<u64>,
    pub waiting_stack_id: Option<u64>,
    pub resolved_reducer_id: u64,
    pub resolution: AdmissionCanonicalPathResolution,
}

pub open spec fn admission_canonical_path_resolution_from_summary(
    kind: AdmissionCanonicalPathObligationKind,
    summary: AdmissionReturnedKind,
) -> Option<AdmissionCanonicalPathResolution> {
    match (kind, summary) {
        (AdmissionCanonicalPathObligationKind::RawParEscape,
            AdmissionReturnedKind::Par { canonical_path_stable: false, .. }) =>
            Some(AdmissionCanonicalPathResolution::RawPar {
                canonical_path_stable: false,
            }),
        (AdmissionCanonicalPathObligationKind::NestedEpm1,
            AdmissionReturnedKind::EPathMapSnapshot {
                mode: 2, entries_canonical_path_stable: true,
            }) => Some(AdmissionCanonicalPathResolution::NestedEpm1 {
                mode: 2, entries_canonical_path_stable: true,
            }),
        _ => None,
    }
}

pub open spec fn admission_canonical_path_return_sidecar_after(
    source: AdmissionCanonicalPathReturnSource,
    summary: AdmissionReturnedKind,
) -> Option<AdmissionCanonicalPathReturnAfter> {
    if source.reducer_waiting_id != source.push.reducer_obligation_id
        || source.reducer_waiting_kind != source.push.kind
        || source.stack_return_id != source.push.stack_return_id
        || admission_canonical_path_resolution_from_summary(
            source.push.kind, summary,
        ).is_none()
    { None } else {
        Some(AdmissionCanonicalPathReturnAfter {
            parent: source.push.parent,
            cursor: source.push.cursor_after_step,
            waiting_reducer_id: None,
            waiting_stack_id: None,
            resolved_reducer_id: source.reducer_waiting_id,
            resolution: admission_canonical_path_resolution_from_summary(
                source.push.kind, summary,
            ).unwrap(),
        })
    }
}

pub open spec fn admission_canonical_path_epm_mode_from_bytes(
    bytes: Seq<u8>,
) -> Option<u8> {
    if bytes.len() < 6 || bytes[0] != 0x45 || bytes[1] != 0x50
        || bytes[2] != 0x4d || bytes[3] != 0x31 || bytes[4] != 1
        || bytes[5] > 2
    { None } else { Some(bytes[5]) }
}

pub open spec fn admission_canonical_path_return_summary_from_source(
    state: AdmissionState, source: AdmissionCanonicalPathReturnSource,
    summary: AdmissionReturnedKind,
) -> bool {
    if state.frames.len() < 2 { false } else {
        let child = state.frames[state.frames.len() - 1];
        match (source.push.kind, summary) {
            (AdmissionCanonicalPathObligationKind::RawParEscape,
                AdmissionReturnedKind::Par {
                    canonical_path_stable: false, ..
                }) => {
                admission_child_summary_agrees(child, summary, state.arena)
            },
            (AdmissionCanonicalPathObligationKind::NestedEpm1,
                AdmissionReturnedKind::EPathMapSnapshot {
                    mode, entries_canonical_path_stable,
                }) => {
                &&& mode == 2
                &&& entries_canonical_path_stable
                &&& admission_canonical_path_epm_mode_from_bytes(
                    source.push.child.bytes,
                ) == Some(mode)
                &&& entries_canonical_path_stable == (forall|i: int|
                    0 <= i < source.epm_entry_stability.len()
                        ==> source.epm_entry_stability[i])
                &&& child.score_handle.is_none()
            },
            _ => false,
        }
    }
}

pub open spec fn admission_canonical_path_return_source_wf(
    state: AdmissionState, source: AdmissionCanonicalPathReturnSource,
    summary: AdmissionReturnedKind,
) -> bool {
    if state.frames.len() < 2 { false } else {
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames[state.frames.len() - 1];
        let push = source.push;
        let parent_start = admission_indexed_source_position(push.parent, 0);
        let child_start = admission_indexed_source_position(push.child, 0);
        &&& parent.frame_kind == 4
        &&& parent.program_type_id == 0
        &&& admission_canonical_path_slotless(parent)
        &&& parent.waiting_id == Some(push.stack_return_id)
        &&& parent.waiting_kind == child.frame_kind
        &&& parent.waiting_slot.is_none()
        &&& parent.waiting_tag.is_none()
        &&& source.stack_return_id == push.stack_return_id
        &&& source.reducer_waiting_id == push.reducer_obligation_id
        &&& source.reducer_waiting_kind == push.kind
        &&& child.return_id == Some(source.stack_return_id)
        &&& source.stack_return_id != u64::MAX
        &&& child.body_id as int == source.stack_return_id as int + 1
        &&& admission_indexed_source_wf(push.parent)
        &&& admission_indexed_source_wf(push.child)
        &&& parent.source_bytes == push.parent.bytes
        &&& parent_start.is_some()
        &&& parent.physical_source == parent_start.unwrap().source
        &&& parent.physical_start == parent_start.unwrap().offset
        &&& parent.cursor == push.cursor_after_step
        &&& push.payload_start <= push.payload_end
        &&& push.payload_end <= parent.cursor
        &&& push.child.bytes == push.parent.bytes.subrange(
            push.payload_start as int, push.payload_end as int,
        )
        &&& push.child.positions == push.parent.positions.subrange(
            push.payload_start as int, push.payload_end as int,
        )
        &&& push.child.end == push.terminal_anchor
        &&& child.source_bytes == push.child.bytes
        &&& child.region_start == 0
        &&& child.region_end as int == push.child.bytes.len()
        &&& child_start.is_some()
        &&& child.physical_source == child_start.unwrap().source
        &&& child.physical_start == child_start.unwrap().offset
        &&& child.cursor == child.region_end
        &&& child.waiting_id.is_none()
        &&& child.waiting_kind == 0
        &&& child.nested_started == child.nested_resolved
        &&& admission_slots_complete(child)
        &&& admission_family_admissible(child)
        &&& admission_frame_wf(child)
        &&& match push.kind {
            AdmissionCanonicalPathObligationKind::RawParEscape =>
                child.frame_kind == 1 && child.program_type_id == 1
                    && child.family == AdmissionFamily::Other
                    && generated::generated_slot_count(1) == Some(child.slot_count),
            AdmissionCanonicalPathObligationKind::NestedEpm1 =>
                child.frame_kind == 3 && child.program_type_id == 1
                    && admission_canonical_path_slotless(child),
        }
        &&& admission_canonical_path_return_summary_from_source(
            state, source, summary,
        )
    }
}

/// One `FrameAction::Complete` and parent `accept` macro-action. CanonicalPath
/// stores neither a protobuf return slot nor an arena score for the child;
/// the path reducer consumes its own obligation sidecar instead.
pub open spec fn admission_canonical_path_return_indexed_step(
    state: AdmissionState, source: AdmissionCanonicalPathReturnSource,
    summary: AdmissionReturnedKind,
) -> Option<AdmissionState> {
    if !admission_canonical_path_return_source_wf(state, source, summary)
        || state.completed == usize::MAX
        || (state.frames[state.frames.len() - 1].frame_kind == 3
            && state.frames[state.frames.len() - 2].nested_resolved == u64::MAX)
    { None } else {
        let parent_index = state.frames.len() - 2;
        let parent = state.frames[parent_index];
        let child = state.frames[parent_index + 1];
        let next_parent = AdmissionFrameState {
            waiting_id: None,
            waiting_kind: 0,
            nested_resolved: if child.frame_kind == 3 {
                (parent.nested_resolved as int + 1) as u64
            } else { parent.nested_resolved },
            ..parent
        };
        Some(AdmissionState {
            frames: state.frames.subrange(0, state.frames.len() - 1)
                .update(parent_index, next_parent),
            arena: state.arena,
            root_end: state.root_end,
            pushed: state.pushed,
            completed: (state.completed as int + 1) as usize,
            next_obligation_id: state.next_obligation_id,
        })
    }
}

pub proof fn admission_canonical_path_return_indexed_preserves_wf(
    state: AdmissionState, source: AdmissionCanonicalPathReturnSource,
    summary: AdmissionReturnedKind,
)
    requires admission_state_wf(state),
    ensures match admission_canonical_path_return_indexed_step(
        state, source, summary,
    ) {
        Some(next) => {
            let parent_index = state.frames.len() - 2;
            let old_parent = state.frames[parent_index];
            let new_parent = next.frames[parent_index];
            &&& admission_state_wf(next)
            &&& next.frames.len() + 1 == state.frames.len()
            &&& next.completed as int == state.completed as int + 1
            &&& next.pushed == state.pushed
            &&& next.arena == state.arena
            &&& next.next_obligation_id == state.next_obligation_id
            &&& new_parent.frame_kind == 4
            &&& new_parent.waiting_id.is_none()
            &&& new_parent.waiting_kind == 0
            &&& new_parent.waiting_slot.is_none()
            &&& new_parent.waiting_tag.is_none()
            &&& new_parent.source_bytes == old_parent.source_bytes
            &&& new_parent.physical_source == old_parent.physical_source
            &&& new_parent.physical_start == old_parent.physical_start
            &&& new_parent.cursor == old_parent.cursor
            &&& new_parent.last_slot == old_parent.last_slot
            &&& new_parent.seen == old_parent.seen
            &&& new_parent.returns == old_parent.returns
            &&& new_parent.score_handle == old_parent.score_handle
            &&& new_parent.nested_started == new_parent.nested_resolved
            &&& source.reducer_waiting_id == source.push.reducer_obligation_id
            &&& source.stack_return_id == source.push.stack_return_id
            &&& admission_canonical_path_return_sidecar_after(source, summary).is_some()
            &&& admission_canonical_path_return_sidecar_after(
                source, summary,
            ).unwrap().waiting_reducer_id.is_none()
            &&& admission_canonical_path_return_sidecar_after(
                source, summary,
            ).unwrap().waiting_stack_id.is_none()
            &&& admission_canonical_path_return_sidecar_after(
                source, summary,
            ).unwrap().resolved_reducer_id == source.reducer_waiting_id
            &&& (source.push.kind == AdmissionCanonicalPathObligationKind::NestedEpm1
                ==> new_parent.nested_resolved as int
                    == old_parent.nested_resolved as int + 1)
            &&& (source.push.kind == AdmissionCanonicalPathObligationKind::RawParEscape
                ==> new_parent.nested_resolved == old_parent.nested_resolved)
        },
        None => true,
    },
{
    if let Some(next) = admission_canonical_path_return_indexed_step(
        state, source, summary,
    ) {
        let parent_index = state.frames.len() - 2;
        let parent = state.frames[parent_index];
        let child = state.frames[parent_index + 1];
        let next_parent = next.frames[parent_index];
        assert(admission_canonical_path_return_source_wf(state, source, summary));
        assert(admission_frame_wf(parent));
        assert(admission_frame_wf(child));
        assert(parent.waiting_kind == child.frame_kind);
        assert(parent.waiting_slot.is_none());
        assert(parent.waiting_tag.is_none());
        if child.frame_kind == 3 {
            assert(parent.nested_started as int == parent.nested_resolved as int + 1);
            assert(next_parent.nested_resolved as int
                == parent.nested_resolved as int + 1);
        } else {
            assert(child.frame_kind == 1);
            assert(parent.nested_started == parent.nested_resolved);
            assert(next_parent.nested_resolved == parent.nested_resolved);
        }
        assert(next_parent.nested_started == next_parent.nested_resolved);
        assert(next_parent.source_bytes == parent.source_bytes);
        assert(next_parent.required == parent.required);
        assert(next_parent.scalar_occurrences == parent.scalar_occurrences);
        assert(next_parent.returns == parent.returns);
        assert(admission_frame_wf(next_parent));
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_frame_wf(next.frames[i]) by {
            if i < parent_index {
                assert(next.frames[i] == state.frames[i]);
            } else {
                assert(i == parent_index);
                assert(next.frames[i] == next_parent);
            }
        }
        assert forall|frame_index: int, return_index: int|
            0 <= frame_index < next.frames.len()
                && 0 <= return_index < next.frames[frame_index].returns.len()
            implies next.frames[frame_index].returns[return_index].score_handle.is_some()
                == next.frames[frame_index].returns[return_index]
                    .score_origin_body_id.is_some()
                && match next.frames[frame_index].returns[return_index].score_handle {
                    Some(handle) => (handle as int) < next.arena.len()
                        && next.arena[handle as int].body_id
                            == next.frames[frame_index].returns[return_index]
                                .score_origin_body_id.unwrap(),
                    None => true,
                } by {
            if frame_index < parent_index {
                assert(next.frames[frame_index] == state.frames[frame_index]);
            } else {
                assert(frame_index == parent_index);
                assert(next_parent.returns == parent.returns);
            }
        }
        assert forall|frame_index: int| 1 <= frame_index < next.frames.len()
            implies ((next.frames[frame_index - 1].frame_kind != 1
                && next.frames[frame_index - 1].frame_kind != 2)
                || #[trigger] admission_child_physical_contained(
                    next.frames[frame_index - 1], next.frames[frame_index],
                )) by {
            if frame_index < parent_index {
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                assert(next.frames[frame_index] == state.frames[frame_index]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
            } else {
                assert(frame_index == parent_index);
                assert(next.frames[frame_index - 1] == state.frames[frame_index - 1]);
                if state.frames[frame_index - 1].frame_kind == 1
                    || state.frames[frame_index - 1].frame_kind == 2
                {
                    assert(admission_child_physical_contained(
                        state.frames[frame_index - 1], state.frames[frame_index],
                    ));
                }
                assert(next.frames[frame_index].physical_source
                    == state.frames[frame_index].physical_source);
                assert(next.frames[frame_index].physical_start
                    == state.frames[frame_index].physical_start);
                assert(next.frames[frame_index].region_end
                    == state.frames[frame_index].region_end);
                assert(next.frames[frame_index].source_bytes
                    == state.frames[frame_index].source_bytes);
            }
        }
        assert(admission_state_wf(next));
    }
}

/// Independent facts at the first `FrameRequest::CanonicalPath` push. The
/// indexed image retains every physical byte position and the terminal end;
/// an empty root starts at that end anchor. No protobuf program pointer or
/// generated slots belong to this root.
pub struct AdmissionCanonicalPathRootSource {
    pub ghost indexed_input: AdmissionIndexedSourceImage,
    pub ghost body_bytes: Seq<u8>,
    pub ghost body_positions: Seq<AdmissionSourceAnchor>,
    pub body_start: AdmissionSourceAnchor,
    pub body_end: AdmissionSourceAnchor,
    pub observed_program_identity: Option<usize>,
    pub request_kind: u8,
    pub observed_root_kind: u8,
    pub cursor_initial: usize,
    pub waiting_initial: Option<u64>,
    pub ghost score_arena_initial: Seq<AdmissionScoreBinding>,
    pub stack_len_before: usize,
    pub stack_capacity_before: usize,
    pub stack_len_after: usize,
    pub stack_capacity_after: usize,
    pub frame_variant_bytes: usize,
    pub stats_frame_variant_bytes: usize,
    pub used_bytes: usize,
    pub reserved_bytes: usize,
}

pub open spec fn admission_canonical_path_root_source_wf(
    source: AdmissionCanonicalPathRootSource,
) -> bool {
    &&& admission_indexed_source_wf(source.indexed_input)
    &&& source.indexed_input.bytes == source.body_bytes
    &&& source.indexed_input.positions == source.body_positions
    &&& source.indexed_input.end == source.body_end
    &&& source.body_bytes.len() <= usize::MAX as int
    &&& admission_indexed_source_position(source.indexed_input, 0)
        == Some(source.body_start)
    &&& source.observed_program_identity.is_none()
    &&& source.request_kind == 4
    &&& source.observed_root_kind == 4
    &&& source.cursor_initial == 0
    &&& source.waiting_initial.is_none()
    &&& source.score_arena_initial == Seq::empty()
    &&& source.stack_len_before == 0
    &&& source.stack_capacity_before >= 1
    &&& source.stack_len_after == 1
    &&& source.stack_capacity_after >= source.stack_capacity_before
    &&& source.stack_capacity_after >= source.stack_len_after
    &&& source.frame_variant_bytes > 0
    &&& source.stats_frame_variant_bytes == source.frame_variant_bytes
    &&& source.stack_len_after as int * source.frame_variant_bytes as int
        <= usize::MAX as int
    &&& source.stack_capacity_after as int * source.frame_variant_bytes as int
        <= usize::MAX as int
    &&& source.used_bytes as int
        == source.stack_len_after as int * source.frame_variant_bytes as int
    &&& source.reserved_bytes as int
        == source.stack_capacity_after as int * source.frame_variant_bytes as int
}

pub open spec fn admission_canonical_path_root_projected_frame(
    source: AdmissionCanonicalPathRootSource,
) -> AdmissionFrameState {
    AdmissionFrameState {
        physical_source: source.body_start.source,
        physical_start: source.body_start.offset,
        program_type_id: 0,
        region_start: 0,
        region_end: source.body_bytes.len() as usize,
        source_bytes: source.body_bytes,
        cursor: source.cursor_initial,
        slot_count: 0,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: AdmissionFamily::Other,
        waiting_id: source.waiting_initial,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 4,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn admission_canonical_path_root_indexed_binding(
    state: AdmissionState, source: AdmissionCanonicalPathRootSource,
) -> bool {
    &&& admission_canonical_path_root_source_wf(source)
    &&& state.frames.len() == 1
    &&& state.frames[0] == admission_canonical_path_root_projected_frame(source)
    &&& state.frames[0].source_bytes == source.indexed_input.bytes
    &&& source.indexed_input.positions == source.body_positions
    &&& source.indexed_input.end == source.body_end
    &&& state.arena == source.score_arena_initial
}

/// This derives a kind-4 root from source-shaped facts without using the
/// existing kind-1 Message root predicate as a premise. Binding the source
/// observation to every production root push remains a separate obligation.
pub proof fn admission_canonical_path_root_source_derives_initial_wf(
    source: AdmissionCanonicalPathRootSource,
)
    requires admission_canonical_path_root_source_wf(source),
    ensures
        admission_frame_wf(admission_canonical_path_root_projected_frame(source)),
        admission_state_wf(admission_initial_state(
            admission_canonical_path_root_projected_frame(source),
        )),
        admission_canonical_path_root_indexed_binding(
            admission_initial_state(admission_canonical_path_root_projected_frame(source)),
            source,
        ),
        admission_initial_state(admission_canonical_path_root_projected_frame(source))
            .frames.len() == source.stack_len_after as int,
        admission_initial_state(admission_canonical_path_root_projected_frame(source))
            .pushed == 1,
        admission_initial_state(admission_canonical_path_root_projected_frame(source))
            .completed == 0,
{
    let root = admission_canonical_path_root_projected_frame(source);
    assert(root.region_start == 0);
    assert(root.source_bytes.len() == root.region_end as int);
    assert(root.cursor == 0);
    assert(root.frame_kind == 4);
    assert(root.waiting_id.is_none());
    assert(admission_canonical_path_slotless(root));
    assert(admission_frame_wf(root));
    let state = admission_initial_state(root);
    assert(admission_state_wf(state));
    assert(admission_canonical_path_root_indexed_binding(state, source));
}

/// A nonvacuous empty input starts at its supplied physical end anchor.
pub open spec fn admission_canonical_path_empty_root_witness()
    -> AdmissionCanonicalPathRootSource {
    let end = AdmissionSourceAnchor { source: 7, offset: 43 };
    AdmissionCanonicalPathRootSource {
        indexed_input: AdmissionIndexedSourceImage {
            bytes: Seq::empty(), positions: Seq::empty(), end,
        },
        body_bytes: Seq::empty(),
        body_positions: Seq::empty(),
        body_start: end,
        body_end: end,
        observed_program_identity: None,
        request_kind: 4,
        observed_root_kind: 4,
        cursor_initial: 0,
        waiting_initial: None,
        score_arena_initial: Seq::empty(),
        stack_len_before: 0,
        stack_capacity_before: 1,
        stack_len_after: 1,
        stack_capacity_after: 1,
        frame_variant_bytes: 1,
        stats_frame_variant_bytes: 1,
        used_bytes: 1,
        reserved_bytes: 1,
    }
}

pub proof fn admission_canonical_path_empty_root_witness_is_initial()
    ensures
        admission_canonical_path_root_source_wf(
            admission_canonical_path_empty_root_witness(),
        ),
        admission_state_wf(admission_initial_state(
            admission_canonical_path_root_projected_frame(
                admission_canonical_path_empty_root_witness(),
            ),
        )),
        admission_canonical_path_root_projected_frame(
            admission_canonical_path_empty_root_witness(),
        ).physical_start == 43,
{
    let source = admission_canonical_path_empty_root_witness();
    assert(admission_canonical_path_root_source_wf(source));
    admission_canonical_path_root_source_derives_initial_wf(source);
}

pub open spec fn admission_indexed_field_child_source(
    frame: AdmissionFrameState,
    image: AdmissionIndexedSourceImage,
    field: IndexedCanonicalFieldModel,
) -> Option<AdmissionIndexedSourceImage> {
    if frame.source_bytes != image.bytes
        || !indexed_field_at_cursor(
            IndexedCanonicalFieldCursorModel {
                region_start: frame.region_start,
                region_end: frame.region_end,
                next: frame.cursor,
                source_bytes: frame.source_bytes,
            }, field,
        )
    {
        None
    } else {
        admission_indexed_source_subview(
            image, field.payload_start, field.payload_end,
        )
    }
}

pub proof fn admission_indexed_field_child_source_exact(
    frame: AdmissionFrameState,
    image: AdmissionIndexedSourceImage,
    field: IndexedCanonicalFieldModel,
)
    requires
        frame.source_bytes == image.bytes,
        admission_indexed_source_wf(image),
        indexed_field_at_cursor(
            IndexedCanonicalFieldCursorModel {
                region_start: frame.region_start,
                region_end: frame.region_end,
                next: frame.cursor,
                source_bytes: frame.source_bytes,
            }, field,
        ),
    ensures
        admission_indexed_field_child_source(frame, image, field).is_some(),
        admission_indexed_field_child_source(frame, image, field).unwrap().bytes
            == field.payload_bytes,
        admission_indexed_source_wf(
            admission_indexed_field_child_source(frame, image, field).unwrap(),
        ),
        admission_indexed_source_position(
            admission_indexed_field_child_source(frame, image, field).unwrap(), 0,
        ) == admission_indexed_source_position(image, field.payload_start),
        admission_indexed_field_child_source(frame, image, field).unwrap().end
            == admission_indexed_source_position(image, field.payload_end).unwrap(),
{
    let child = admission_indexed_field_child_source(frame, image, field).unwrap();
    assert(child.bytes == image.bytes.subrange(
        field.payload_start as int, field.payload_end as int,
    ));
    assert(child.positions == image.positions.subrange(
        field.payload_start as int, field.payload_end as int,
    ));
    if field.payload_start == field.payload_end {
        assert(child.positions.len() == 0);
        assert(child.end == admission_indexed_source_position(
            image, field.payload_start,
        ).unwrap());
    }
}

pub open spec fn admission_discontinuous_empty_child_source()
    -> AdmissionIndexedSourceImage {
    AdmissionIndexedSourceImage {
        bytes: seq![0x0au8, 0x00u8],
        positions: seq![
            AdmissionSourceAnchor { source: 0, offset: 0 },
            AdmissionSourceAnchor { source: 0, offset: 1 },
        ],
        end: AdmissionSourceAnchor { source: 1, offset: 100 },
    }
}

pub open spec fn admission_discontinuous_empty_child_frame()
    -> AdmissionFrameState {
    AdmissionFrameState {
        source_bytes: seq![0x0au8, 0x00u8],
        region_end: 2,
        cursor: 0,
        ..admission_family_witness_frame(
            AdmissionFamily::Send, Set::empty(), Seq::empty(),
        )
    }
}

pub open spec fn admission_discontinuous_empty_child_field()
    -> IndexedCanonicalFieldModel {
    IndexedCanonicalFieldModel {
        number: 1,
        encoded_start: 0,
        encoded_end: 2,
        payload_start: 2,
        payload_end: 2,
        key_width: 1,
        payload_width: 1,
        payload_length: 0,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: Seq::empty(),
    }
}

pub proof fn admission_discontinuous_empty_child_anchor_witness()
    ensures
        admission_indexed_field_child_source(
            admission_discontinuous_empty_child_frame(),
            admission_discontinuous_empty_child_source(),
            admission_discontinuous_empty_child_field(),
        ).is_some(),
        admission_indexed_source_position(
            admission_indexed_field_child_source(
                admission_discontinuous_empty_child_frame(),
                admission_discontinuous_empty_child_source(),
                admission_discontinuous_empty_child_field(),
            ).unwrap(), 0,
        ) == Some(AdmissionSourceAnchor { source: 1, offset: 100 }),
        admission_discontinuous_empty_child_frame().physical_source == 0,
        admission_discontinuous_empty_child_frame().physical_start as int
            + admission_discontinuous_empty_child_field().payload_start as int == 2,
{
    let frame = admission_discontinuous_empty_child_frame();
    let image = admission_discontinuous_empty_child_source();
    let field = admission_discontinuous_empty_child_field();
    assert(decoded_varint_prefix(seq![0x0au8], 0) == 0);
    assert(varint_multiplier(0) == 1);
    assert(decoded_varint_prefix(seq![0x0au8], 1) == 10);
    assert(decoded_varint_prefix(seq![0x00u8], 0) == 0);
    assert(decoded_varint_prefix(seq![0x00u8], 1) == 0);
    assert(canonical_varint_bytes(seq![0x0au8], 10));
    assert(canonical_varint_bytes(seq![0x00u8], 0));
    assert(frame.source_bytes.subrange(0, 1) == seq![0x0au8]);
    assert(frame.source_bytes.subrange(1, 2) == seq![0x00u8]);
    assert(frame.source_bytes.subrange(2, 2) == Seq::<u8>::empty());
    assert(field.number as int * 8 + field.wire as int == 10);
    assert(indexed_field_at_cursor(
        IndexedCanonicalFieldCursorModel {
            region_start: frame.region_start,
            region_end: frame.region_end,
            next: frame.cursor,
            source_bytes: frame.source_bytes,
        }, field,
    ));
    admission_indexed_field_child_source_exact(frame, image, field);
}

pub open spec fn varint_multiplier(groups: int) -> int
    decreases groups,
{
    if groups <= 0 { 1 } else { 128 * varint_multiplier(groups - 1) }
}

pub open spec fn decoded_varint_prefix(bytes: Seq<u8>, groups: int) -> int
    decreases groups,
{
    if groups <= 0 || groups > bytes.len() { 0 }
    else {
        decoded_varint_prefix(bytes, groups - 1)
            + ((bytes[groups - 1] as int) % 128) * varint_multiplier(groups - 1)
    }
}

pub open spec fn canonical_varint_bytes(bytes: Seq<u8>, value: u64) -> bool {
    1 <= bytes.len() <= 10
        && (forall|i: int| 0 <= i < bytes.len() - 1 ==> bytes[i] >= 128)
        && bytes[bytes.len() - 1] < 128
        && (bytes.len() == 1 || bytes[bytes.len() - 1] != 0)
        && (bytes.len() != 10 || bytes[9] <= 1)
        && decoded_varint_prefix(bytes, bytes.len() as int) == value as int
}

pub open spec fn canonical_varint_payload(field: IndexedCanonicalFieldModel) -> bool {
    field.payload_bytes.len() == field.payload_width as int
        && canonical_varint_bytes(field.payload_bytes, field.decoded_varint)
}

pub open spec fn utf8_cont(byte: u8) -> bool { 0x80 <= byte <= 0xbf }

pub open spec fn utf8_valid_at(bytes: Seq<u8>, index: int) -> bool
    decreases bytes.len() - index,
{
    if index == bytes.len() { true }
    else if index < 0 || index > bytes.len() { false }
    else {
        let first = bytes[index];
        if first <= 0x7f {
            utf8_valid_at(bytes, index + 1)
        } else if 0xc2 <= first <= 0xdf {
            index + 1 < bytes.len() && utf8_cont(bytes[index + 1])
                && utf8_valid_at(bytes, index + 2)
        } else if first == 0xe0 {
            index + 2 < bytes.len()
                && 0xa0 <= bytes[index + 1] <= 0xbf
                && utf8_cont(bytes[index + 2])
                && utf8_valid_at(bytes, index + 3)
        } else if (0xe1 <= first <= 0xec) || (0xee <= first <= 0xef) {
            index + 2 < bytes.len()
                && utf8_cont(bytes[index + 1])
                && utf8_cont(bytes[index + 2])
                && utf8_valid_at(bytes, index + 3)
        } else if first == 0xed {
            index + 2 < bytes.len()
                && 0x80 <= bytes[index + 1] <= 0x9f
                && utf8_cont(bytes[index + 2])
                && utf8_valid_at(bytes, index + 3)
        } else if first == 0xf0 {
            index + 3 < bytes.len()
                && 0x90 <= bytes[index + 1] <= 0xbf
                && utf8_cont(bytes[index + 2])
                && utf8_cont(bytes[index + 3])
                && utf8_valid_at(bytes, index + 4)
        } else if 0xf1 <= first <= 0xf3 {
            index + 3 < bytes.len()
                && utf8_cont(bytes[index + 1])
                && utf8_cont(bytes[index + 2])
                && utf8_cont(bytes[index + 3])
                && utf8_valid_at(bytes, index + 4)
        } else if first == 0xf4 {
            index + 3 < bytes.len()
                && 0x80 <= bytes[index + 1] <= 0x8f
                && utf8_cont(bytes[index + 2])
                && utf8_cont(bytes[index + 3])
                && utf8_valid_at(bytes, index + 4)
        } else { false }
    }
}

pub open spec fn fixed_payload_is_zero(bytes: Seq<u8>) -> bool {
    forall|i: int| 0 <= i < bytes.len() ==> bytes[i] == 0
}

pub open spec fn floating_payload_is_zero(bytes: Seq<u8>) -> bool {
    bytes.len() > 0
        && (forall|i: int| 0 <= i < bytes.len() - 1 ==> bytes[i] == 0)
        && bytes[bytes.len() - 1] % 128 == 0
}

pub open spec fn scalar_payload_valid(
    field: IndexedCanonicalFieldModel, scalar_kind: u8, default_forbidden: bool,
) -> bool {
    if scalar_kind == 1 {
        field.wire == 0 && field.decoded_varint <= 1
            && (!default_forbidden || field.decoded_varint != 0)
    } else if scalar_kind == 2 {
        field.wire == 0
            && (field.decoded_varint <= 0x7fff_ffff
                || field.decoded_varint >= 0xffff_ffff_8000_0000)
            && (!default_forbidden || field.decoded_varint != 0)
    } else if scalar_kind == 3 || scalar_kind == 4 {
        field.wire == 0 && field.decoded_varint <= u32::MAX as u64
            && (!default_forbidden || field.decoded_varint != 0)
    } else if 5 <= scalar_kind <= 7 {
        field.wire == 0 && (!default_forbidden || field.decoded_varint != 0)
    } else if scalar_kind == 8 || scalar_kind == 9 {
        field.wire == 5
            && (!default_forbidden || !fixed_payload_is_zero(field.payload_bytes))
    } else if scalar_kind == 10 {
        field.wire == 5
            && (!default_forbidden || !floating_payload_is_zero(field.payload_bytes))
    } else if scalar_kind == 11 || scalar_kind == 12 {
        field.wire == 1
            && (!default_forbidden || !fixed_payload_is_zero(field.payload_bytes))
    } else if scalar_kind == 13 {
        field.wire == 1
            && (!default_forbidden || !floating_payload_is_zero(field.payload_bytes))
    } else if scalar_kind == 14 || scalar_kind == 15 {
        field.wire == 2 && utf8_valid_at(field.payload_bytes, 0)
            && (!default_forbidden || field.payload_bytes.len() > 0)
    } else if scalar_kind == 16 || scalar_kind == 17 {
        field.wire == 2 && (!default_forbidden || field.payload_bytes.len() > 0)
    } else { false }
}

pub struct IndexedCanonicalFieldCursorModel {
    pub region_start: usize,
    pub region_end: usize,
    pub next: usize,
    pub ghost source_bytes: Seq<u8>,
}

pub open spec fn indexed_field_at_cursor(
    cursor: IndexedCanonicalFieldCursorModel, field: IndexedCanonicalFieldModel,
) -> bool {
    cursor.region_start <= cursor.next <= cursor.region_end
        && cursor.source_bytes.len() == cursor.region_end as int
        && field.encoded_start == cursor.next
        && field.encoded_start < field.encoded_end <= cursor.region_end
        && 1 <= field.key_width <= 10
        && field.number > 0 && field.number <= 0x1fff_ffff
        && field.payload_start as int >= field.encoded_start as int + field.key_width as int
        && field.payload_start <= field.payload_end
        && field.payload_end == field.encoded_end
        && canonical_varint_bytes(
            cursor.source_bytes.subrange(
                field.encoded_start as int,
                field.encoded_start as int + field.key_width as int,
            ),
            (field.number as int * 8 + field.wire as int) as u64,
        )
        && field.payload_bytes == cursor.source_bytes.subrange(
            field.payload_start as int, field.payload_end as int,
        )
        && (if field.wire == 0 { canonical_varint_payload(field) }
            else if field.wire == 1 { field.payload_bytes.len() == 8 }
            else if field.wire == 5 { field.payload_bytes.len() == 4 }
            else { field.wire == 2
                && field.payload_bytes.len() == field.payload_length as int
                && canonical_varint_bytes(
                    cursor.source_bytes.subrange(
                        field.encoded_start as int + field.key_width as int,
                        field.payload_start as int,
                    ),
                    field.payload_length,
                ) })
        && if field.wire == 0 {
            1 <= field.payload_width <= 10
                && field.payload_start as int
                    == field.encoded_start as int + field.key_width as int
                && field.encoded_end as int
                    == field.payload_start as int + field.payload_width as int
        } else if field.wire == 1 || field.wire == 5 {
            field.payload_start as int
                == field.encoded_start as int + field.key_width as int
                && field.encoded_end as int == field.payload_start as int
                    + (if field.wire == 1 { 8int } else { 4int })
        } else {
            field.wire == 2 && 1 <= field.payload_width <= 10
                && field.payload_length as int <= usize::MAX as int
                && field.payload_start as int == field.encoded_start as int
                    + field.key_width as int + field.payload_width as int
                && field.encoded_end as int
                    == field.payload_start as int + field.payload_length as int
        }
}

pub open spec fn admission_map_key_field(
    frame: AdmissionFrameState, next_cursor: usize,
) -> bool {
    exists|field: IndexedCanonicalFieldModel|
        indexed_field_at_cursor(
            IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: frame.region_end,
                next: frame.cursor,
                source_bytes: frame.source_bytes,
            },
            field,
        )
            && field.number == 1
            && field.wire == 2
            && field.encoded_end == next_cursor
            && field.payload_bytes.len() > 0
            && utf8_valid_at(field.payload_bytes, 0)
}

pub open spec fn admission_map_value_field(
    frame: AdmissionFrameState, next_cursor: usize, child: AdmissionFrameState,
) -> bool {
    exists|field: IndexedCanonicalFieldModel|
        indexed_field_at_cursor(
            IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: frame.region_end,
                next: frame.cursor,
                source_bytes: frame.source_bytes,
            },
            field,
        )
            && field.number == 2
            && field.wire == 2
            && field.encoded_end == next_cursor
            && child.region_end as int
                == field.payload_end as int - field.payload_start as int
            && child.source_bytes == field.payload_bytes
}

pub open spec fn admission_map_source_matches(
    frame: AdmissionFrameState, key_bytes: Seq<u8>,
) -> bool {
    if frame.seen.contains(0) {
        exists|key_field: IndexedCanonicalFieldModel|
            indexed_field_at_cursor(
                IndexedCanonicalFieldCursorModel {
                    region_start: 0,
                    region_end: frame.region_end,
                    next: 0,
                    source_bytes: frame.source_bytes,
                }, key_field,
            )
                && key_field.number == 1
                && key_field.wire == 2
                && key_field.payload_bytes == key_bytes
                && key_bytes.len() > 0
                && utf8_valid_at(key_bytes, 0)
                && if frame.seen.contains(1) {
                    exists|value_field: IndexedCanonicalFieldModel|
                        indexed_field_at_cursor(
                            IndexedCanonicalFieldCursorModel {
                                region_start: 0,
                                region_end: frame.region_end,
                                next: key_field.encoded_end,
                                source_bytes: frame.source_bytes,
                            }, value_field,
                        )
                            && value_field.number == 2
                            && value_field.wire == 2
                            && value_field.encoded_end == frame.region_end
                } else {
                    key_field.encoded_end == frame.region_end
                }
    } else {
        key_bytes == Seq::empty()
            && if frame.seen.contains(1) {
                exists|value_field: IndexedCanonicalFieldModel|
                    indexed_field_at_cursor(
                        IndexedCanonicalFieldCursorModel {
                            region_start: 0,
                            region_end: frame.region_end,
                            next: 0,
                            source_bytes: frame.source_bytes,
                        }, value_field,
                    )
                        && value_field.number == 2
                        && value_field.wire == 2
                        && value_field.encoded_end == frame.region_end
            } else {
                frame.region_end == 0
            }
    }
}

pub open spec fn admission_map_key_bytes(frame: AdmissionFrameState) -> Seq<u8>
    recommends admission_map_summary_complete(frame),
{
    choose|key_bytes: Seq<u8>| admission_map_source_matches(frame, key_bytes)
}

pub open spec fn admission_map_value_score_handle(
    frame: AdmissionFrameState,
) -> Option<usize> {
    if frame.seen.contains(1) && frame.returns.len() == 1 {
        frame.returns[0].score_handle
    } else {
        None
    }
}

pub open spec fn admission_bytes_lex_lt(left: Seq<u8>, right: Seq<u8>) -> bool
    decreases left.len() + right.len(),
{
    if left.len() == 0 { right.len() > 0 }
    else if right.len() == 0 { false }
    else if left[0] < right[0] { true }
    else if left[0] > right[0] { false }
    else { admission_bytes_lex_lt(
        left.subrange(1, left.len() as int),
        right.subrange(1, right.len() as int),
    ) }
}

pub open spec fn admission_scalar_descriptor_valid(
    program_type_id: u16, field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
) -> bool {
    match generated::generated_descriptor(program_type_id, field.number) {
        Some(descriptor) => descriptor.frame_kind == 0
            && descriptor.slot == slot
            && descriptor.repeated == repeated
            && descriptor.scalar_kind == scalar_kind
            && descriptor.wire == field.wire
            && scalar_payload_valid(
                field, descriptor.scalar_kind, descriptor.default_forbidden,
            ),
        None => false,
    }
}

pub open spec fn admission_record_scalar_occurrence_at_position_step(
    state: AdmissionState, field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
    physical_source: usize, physical_start: int,
) -> Option<AdmissionState> {
    match admission_continue_step(
        state, field.encoded_end, slot, repeated, field.number,
    ) {
        None => None,
        Some(next) => {
            let index = next.frames.len() - 1;
            let top = next.frames[index];
            let prior = state.frames[index];
            let source_cursor = IndexedCanonicalFieldCursorModel {
                region_start: prior.region_start,
                region_end: prior.region_end,
                next: prior.cursor,
                source_bytes: prior.source_bytes,
            };
            let descriptor_valid = admission_scalar_descriptor_valid(
                prior.program_type_id, field, slot, repeated, scalar_kind,
            );
            if !indexed_field_at_cursor(source_cursor, field) || !descriptor_valid {
                None
            } else {
            let occurrence = AdmissionScalarOccurrence {
                slot,
                tag: field.number,
                scalar_kind,
                varint_value: field.decoded_varint,
                physical_source,
                physical_start,
                bytes: field.payload_bytes,
            };
            let updated = AdmissionFrameState {
                physical_source: top.physical_source,
                physical_start: top.physical_start,
                program_type_id: top.program_type_id,
                region_start: top.region_start,
                region_end: top.region_end,
                source_bytes: top.source_bytes,
                cursor: top.cursor,
                slot_count: top.slot_count,
                last_slot: top.last_slot,
                seen: top.seen,
                required: top.required,
                scalar_slots: top.scalar_slots,
                scalar_occurrences: top.scalar_occurrences.push(occurrence),
                returns: top.returns,
                family: top.family,
                waiting_id: top.waiting_id,
                waiting_kind: top.waiting_kind,
                waiting_slot: top.waiting_slot,
                waiting_tag: top.waiting_tag,
                return_id: top.return_id,
                frame_kind: top.frame_kind,
                body_id: top.body_id,
                score_handle: top.score_handle,
                nested_started: top.nested_started,
                nested_resolved: top.nested_resolved,
            };
            Some(AdmissionState {
                frames: next.frames.update(index, updated),
                arena: next.arena,
                root_end: next.root_end,
                pushed: next.pushed,
                completed: next.completed,
                next_obligation_id: next.next_obligation_id,
            })
            }
        },
    }
}

pub open spec fn admission_record_scalar_occurrence_step(
    state: AdmissionState, field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 {
        None
    } else {
        let top = state.frames.last();
        admission_record_scalar_occurrence_at_position_step(
            state, field, slot, repeated, scalar_kind,
            top.physical_source, top.physical_start as int + field.encoded_start as int,
        )
    }
}

pub open spec fn admission_record_scalar_occurrence_indexed_step(
    state: AdmissionState, source: AdmissionIndexedSourceImage,
    field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 || !admission_indexed_source_wf(source)
        || source.bytes != state.frames.last().source_bytes
    {
        None
    } else {
        match admission_indexed_source_position(source, field.encoded_start) {
            None => None,
            Some(position) => admission_record_scalar_occurrence_at_position_step(
                state, field, slot, repeated, scalar_kind,
                position.source, position.offset as int,
            ),
        }
    }
}

pub proof fn admission_record_scalar_occurrence_at_position_preserves_wf(
    state: AdmissionState, field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
    physical_source: usize, physical_start: int,
)
    requires admission_state_wf(state),
    ensures match admission_record_scalar_occurrence_at_position_step(
        state, field, slot, repeated, scalar_kind, physical_source, physical_start,
    ) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    admission_continue_preserves_wf(
        state, field.encoded_end, slot, repeated, field.number,
    );
    if let Some(next) = admission_record_scalar_occurrence_at_position_step(
        state, field, slot, repeated, scalar_kind, physical_source, physical_start,
    ) {
        let index = state.frames.len() - 1;
        let middle = admission_continue_step(
            state, field.encoded_end, slot, repeated, field.number,
        ).unwrap();
        assert(admission_state_wf(middle));
        assert(next.frames.len() == middle.frames.len());
        assert(next.arena == middle.arena);
        assert(next.frames[index].scalar_slots == middle.frames[index].scalar_slots);
        assert(next.frames[index].scalar_occurrences.len()
            == middle.frames[index].scalar_occurrences.len() + 1);
        assert(admission_frame_wf(middle.frames[index]));
        assert(next.frames[index].slot_count == middle.frames[index].slot_count);
        assert(next.frames[index].program_type_id == middle.frames[index].program_type_id);
        assert(next.frames[index].frame_kind == middle.frames[index].frame_kind);
        assert(next.frames[index].required == middle.frames[index].required);
        assert(next.frames[index].returns == middle.frames[index].returns);
        assert(next.frames[index].source_bytes == middle.frames[index].source_bytes);
        assert(next.frames[index].physical_start == middle.frames[index].physical_start);
        assert(next.frames[index].region_end == middle.frames[index].region_end);
        assert(next.frames[index].return_id == middle.frames[index].return_id);
        assert(admission_required_slots_generated(next.frames[index]));
        assert forall|occurrence_index: int|
            0 <= occurrence_index < next.frames[index].scalar_occurrences.len()
            implies next.frames[index].scalar_occurrences[occurrence_index].slot
                < next.frames[index].slot_count
                && next.frames[index].scalar_slots.contains(
                    next.frames[index].scalar_occurrences[occurrence_index].slot as int,
                ) by {
            if occurrence_index < middle.frames[index].scalar_occurrences.len() {
                assert(next.frames[index].scalar_occurrences[occurrence_index]
                    == middle.frames[index].scalar_occurrences[occurrence_index]);
            } else {
                assert(occurrence_index == middle.frames[index].scalar_occurrences.len());
                assert(next.frames[index].scalar_occurrences[occurrence_index].slot == slot);
                assert(slot < middle.frames[index].slot_count);
            }
        }
        assert(admission_frame_wf(next.frames[index]));
        assert forall|i: int| 0 <= i < index
            implies next.frames[i] == middle.frames[i] by {
        }
        admission_last_frame_replacement_preserves_wf(middle, next);
    }
}

pub proof fn admission_record_scalar_occurrence_preserves_wf(
    state: AdmissionState, field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
)
    requires admission_state_wf(state),
    ensures match admission_record_scalar_occurrence_step(
        state, field, slot, repeated, scalar_kind,
    ) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if state.frames.len() > 0 {
        let top = state.frames.last();
        admission_record_scalar_occurrence_at_position_preserves_wf(
            state, field, slot, repeated, scalar_kind,
            top.physical_source, top.physical_start as int + field.encoded_start as int,
        );
    }
}

pub proof fn admission_record_scalar_occurrence_indexed_preserves_wf(
    state: AdmissionState, source: AdmissionIndexedSourceImage,
    field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
)
    requires admission_state_wf(state),
    ensures match admission_record_scalar_occurrence_indexed_step(
        state, source, field, slot, repeated, scalar_kind,
    ) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if state.frames.len() > 0 && admission_indexed_source_wf(source)
        && source.bytes == state.frames.last().source_bytes
    {
        if let Some(position) = admission_indexed_source_position(source, field.encoded_start) {
            admission_record_scalar_occurrence_at_position_preserves_wf(
                state, field, slot, repeated, scalar_kind,
                position.source, position.offset as int,
            );
        }
    }
}

pub proof fn admission_record_scalar_occurrence_indexed_anchor_exact(
    state: AdmissionState, source: AdmissionIndexedSourceImage,
    field: IndexedCanonicalFieldModel,
    slot: usize, repeated: bool, scalar_kind: u8,
)
    requires admission_record_scalar_occurrence_indexed_step(
        state, source, field, slot, repeated, scalar_kind,
    ).is_some(),
    ensures {
        let next = admission_record_scalar_occurrence_indexed_step(
            state, source, field, slot, repeated, scalar_kind,
        ).unwrap();
        let occurrence = next.frames.last().scalar_occurrences.last();
        let position = admission_indexed_source_position(
            source, field.encoded_start,
        ).unwrap();
        &&& occurrence.physical_source == position.source
        &&& occurrence.physical_start == position.offset as int
        &&& occurrence.tag == field.number
        &&& occurrence.bytes == field.payload_bytes
    },
{
    assert(state.frames.len() > 0);
    assert(admission_indexed_source_position(source, field.encoded_start).is_some());
}

/// The concrete message view retains the cursor and generated-slot state
/// stored by `MessageFrame`. Its ghost projection is the existing admission
/// stack; equality below is the simulation relation, not a second parser.
pub struct ConcreteMessageFrameModel {
    pub cursor: IndexedCanonicalFieldCursorModel,
    pub last_slot: Option<usize>,
    pub ghost seen: Set<int>,
    pub waiting_id: Option<u64>,
    pub waiting_slot: Option<usize>,
    pub waiting_arm: Option<u32>,
    pub ghost projection: AdmissionState,
}

pub open spec fn concrete_message_frame_rel(frame: ConcreteMessageFrameModel) -> bool {
    if frame.projection.frames.len() == 0 {
        false
    } else {
        let top = frame.projection.frames[frame.projection.frames.len() - 1];
        top.frame_kind == 1
            && generated::generated_slot_count(top.program_type_id) == Some(top.slot_count)
            && frame.cursor.region_start == top.region_start
            && frame.cursor.region_end == top.region_end
            && frame.cursor.next == top.cursor
            && frame.cursor.source_bytes == top.source_bytes
            && frame.last_slot == top.last_slot
            && frame.seen == top.seen
            && frame.waiting_id == top.waiting_id
            && frame.waiting_slot == top.waiting_slot
            && frame.waiting_arm == top.waiting_tag
    }
}

pub open spec fn concrete_message_view(state: AdmissionState) -> Option<ConcreteMessageFrameModel> {
    if state.frames.len() == 0 || state.frames[state.frames.len() - 1].frame_kind != 1
        || generated::generated_slot_count(
            state.frames[state.frames.len() - 1].program_type_id,
        ) != Some(state.frames[state.frames.len() - 1].slot_count)
    {
        None
    } else {
        let top = state.frames[state.frames.len() - 1];
        Some(ConcreteMessageFrameModel {
            cursor: IndexedCanonicalFieldCursorModel {
                region_start: top.region_start,
                region_end: top.region_end,
                next: top.cursor,
                source_bytes: top.source_bytes,
            },
            last_slot: top.last_slot,
            seen: top.seen,
            waiting_id: top.waiting_id,
            waiting_slot: top.waiting_slot,
            waiting_arm: top.waiting_tag,
            projection: state,
        })
    }
}

pub proof fn concrete_message_view_rel(state: AdmissionState)
    ensures match concrete_message_view(state) {
        Some(frame) => concrete_message_frame_rel(frame),
        None => true,
    },
{}

/// `FrameAction::Continue` after `MessageFrame::step`: the field's encoded
/// range moves the indexed cursor, while `resolve_protobuf_field` determines
/// the sole slot, repetition bit, and selected oneof arm.
pub open spec fn concrete_message_continue(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
) -> Option<ConcreteMessageFrameModel> {
    if !concrete_message_frame_rel(frame)
        || !indexed_field_at_cursor(frame.cursor, field)
    {
        None
    } else {
        let top = frame.projection.frames[frame.projection.frames.len() - 1];
        match generated::generated_descriptor(top.program_type_id, field.number) {
            None => None,
            Some(resolved) => {
                if resolved.frame_kind != 0 || resolved.wire != field.wire
                    || !scalar_payload_valid(
                        field, resolved.scalar_kind, resolved.default_forbidden,
                    )
                {
                    None
                } else {
                    match admission_record_scalar_occurrence_step(
                        frame.projection, field, resolved.slot,
                        resolved.repeated, resolved.scalar_kind,
                    ) {
                        None => None,
                        Some(next) => concrete_message_view(next),
                    }
                }
            },
        }
    }
}

pub open spec fn concrete_message_continue_indexed(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    parent_source: AdmissionIndexedSourceImage,
) -> Option<ConcreteMessageFrameModel> {
    if !concrete_message_frame_rel(frame)
        || !indexed_field_at_cursor(frame.cursor, field)
    {
        None
    } else {
        let top = frame.projection.frames[frame.projection.frames.len() - 1];
        match generated::generated_descriptor(top.program_type_id, field.number) {
            None => None,
            Some(resolved) => {
                if resolved.frame_kind != 0 || resolved.wire != field.wire
                    || !scalar_payload_valid(
                        field, resolved.scalar_kind, resolved.default_forbidden,
                    )
                {
                    None
                } else {
                    match admission_record_scalar_occurrence_indexed_step(
                        frame.projection, parent_source, field, resolved.slot,
                        resolved.repeated, resolved.scalar_kind,
                    ) {
                        None => None,
                        Some(next) => concrete_message_view(next),
                    }
                }
            },
        }
    }
}

pub proof fn concrete_message_continue_indexed_simulates(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    parent_source: AdmissionIndexedSourceImage,
)
    ensures match concrete_message_continue_indexed(frame, field, parent_source) {
        Some(next) => {
            &&& concrete_message_frame_rel(next)
            &&& next.cursor.next == field.encoded_end
            &&& match generated::generated_descriptor(
                frame.projection.frames[frame.projection.frames.len() - 1].program_type_id,
                field.number,
            ) {
                Some(resolved) => admission_record_scalar_occurrence_indexed_step(
                    frame.projection, parent_source, field, resolved.slot,
                    resolved.repeated, resolved.scalar_kind,
                ) == Some(next.projection),
                None => false,
            }
        },
        None => true,
    },
{
    if let Some(next) = concrete_message_continue_indexed(frame, field, parent_source) {
        concrete_message_view_rel(next.projection);
    }
}

pub proof fn concrete_message_continue_indexed_anchor_exact(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    parent_source: AdmissionIndexedSourceImage,
)
    requires concrete_message_continue_indexed(frame, field, parent_source).is_some(),
    ensures {
        let next = concrete_message_continue_indexed(
            frame, field, parent_source,
        ).unwrap();
        let occurrence = next.projection.frames.last().scalar_occurrences.last();
        let position = admission_indexed_source_position(
            parent_source, field.encoded_start,
        ).unwrap();
        &&& occurrence.physical_source == position.source
        &&& occurrence.physical_start == position.offset as int
        &&& occurrence.tag == field.number
        &&& occurrence.bytes == field.payload_bytes
        &&& next.cursor.next == field.encoded_end
    },
{
    concrete_message_continue_indexed_simulates(frame, field, parent_source);
    let top = frame.projection.frames.last();
    let resolved = generated::generated_descriptor(
        top.program_type_id, field.number,
    ).unwrap();
    admission_record_scalar_occurrence_indexed_anchor_exact(
        frame.projection, parent_source, field, resolved.slot,
        resolved.repeated, resolved.scalar_kind,
    );
}

pub proof fn concrete_message_continue_indexed_preserves_par_source_trace(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    parent_source: AdmissionIndexedSourceImage,
)
    requires admission_par_source_trace_invariant(frame.projection),
    ensures match concrete_message_continue_indexed(frame, field, parent_source) {
        None => true,
        Some(next) => admission_par_source_trace_invariant(next.projection),
    },
{
    if let Some(next) = concrete_message_continue_indexed(frame, field, parent_source) {
        let before = frame.projection.frames.last();
        let after = next.projection.frames.last();
        if before.program_type_id == 1 && before.frame_kind == 1
            && before.family == AdmissionFamily::Other
        {
            let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
                admission_par_source_prefix_fields(before, fields);
            let descriptor = generated::generated_descriptor(1, field.number).unwrap();
            admission_par_source_prefix_after_field(
                before, after, fields, field, descriptor,
            );
        }
        assert forall|i: int| 0 <= i < next.projection.frames.len()
            implies admission_par_source_prefix(next.projection.frames[i]) by {
            if i < next.projection.frames.len() - 1 {
                assert(next.projection.frames[i] == frame.projection.frames[i]);
            }
        }
        assert forall|i: int| 0 <= i < next.projection.frames.len()
            implies admission_source_identity_in_returns(next.projection.frames[i]) by {
            if i < next.projection.frames.len() - 1 {
                assert(next.projection.frames[i] == frame.projection.frames[i]);
            } else {
                assert(next.projection.frames[i].returns == before.returns);
            }
        }
    }
}

pub proof fn concrete_message_continue_simulates(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
)
    ensures match concrete_message_continue(frame, field) {
        Some(next) => {
            &&& concrete_message_frame_rel(next)
            &&& next.cursor.next == field.encoded_end
            &&& match generated::generated_descriptor(
                frame.projection.frames[frame.projection.frames.len() - 1].program_type_id,
                field.number,
            ) {
                Some(resolved) => admission_record_scalar_occurrence_step(
                    frame.projection, field, resolved.slot,
                    resolved.repeated, resolved.scalar_kind,
                ) == Some(next.projection),
                None => false,
            }
        },
        None => true,
    },
{
    if let Some(next) = concrete_message_continue(frame, field) {
        concrete_message_view_rel(next.projection);
    }
}

pub proof fn concrete_message_continue_preserves_par_source_trace(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
)
    requires admission_par_source_trace_invariant(frame.projection),
    ensures match concrete_message_continue(frame, field) {
        None => true,
        Some(next) => admission_par_source_trace_invariant(next.projection),
    },
{
    if let Some(next) = concrete_message_continue(frame, field) {
        let before = frame.projection.frames.last();
        let after = next.projection.frames.last();
        if before.program_type_id == 1 && before.frame_kind == 1
            && before.family == AdmissionFamily::Other
        {
            let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
                admission_par_source_prefix_fields(before, fields);
            let descriptor = generated::generated_descriptor(1, field.number).unwrap();
            admission_par_source_prefix_after_field(
                before, after, fields, field, descriptor,
            );
        }
        assert forall|i: int| 0 <= i < next.projection.frames.len()
            implies admission_par_source_prefix(next.projection.frames[i]) by {
            if i < next.projection.frames.len() - 1 {
                assert(next.projection.frames[i] == frame.projection.frames[i]);
            }
        }
        assert forall|i: int| 0 <= i < next.projection.frames.len()
            implies admission_source_identity_in_returns(next.projection.frames[i]) by {
            if i < next.projection.frames.len() - 1 {
                assert(next.projection.frames[i] == frame.projection.frames[i]);
            } else {
                assert(next.projection.frames[i].returns == before.returns);
            }
        }
    }
}

/// `FrameAction::Push` produces a pending typed child. The child region is
/// exactly the decoded length-delimited payload; no caller-supplied slot or
/// oneof arm can override the generated resolver. `frame_bytes` is the
/// production `size_of::<Frame>()` input to the checked stack bound. New's
/// injections field pushes `MapFrame`; all other covered fields, including
/// Expr's EPathMap arm, first push a protobuf `MessageFrame`.
pub open spec fn concrete_message_push(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState, frame_bytes: usize,
) -> Option<AdmissionState> {
    if !concrete_message_frame_rel(frame)
        || !indexed_field_at_cursor(frame.cursor, field)
        || child.physical_source
            != frame.projection.frames[frame.projection.frames.len() - 1].physical_source
        || child.physical_start as int
            != frame.projection.frames[frame.projection.frames.len() - 1].physical_start as int
                + field.payload_start as int
        || child.region_start != 0
        || child.region_end as int
            != field.payload_end as int - field.payload_start as int
        || child.source_bytes != field.payload_bytes
    {
        None
    } else {
        let top = frame.projection.frames[frame.projection.frames.len() - 1];
        match generated::generated_descriptor(top.program_type_id, field.number) {
            None => None,
            Some(resolved) => {
                if resolved.frame_kind == 0 || resolved.wire != field.wire
                    || child.frame_kind != resolved.frame_kind
                    || child.program_type_id != resolved.child_type_id.unwrap_or(0)
                    || (child.frame_kind == 1
                        && generated::generated_slot_count(child.program_type_id)
                            != Some(child.slot_count))
                {
                    None
                } else {
                    admission_push_step(
                        frame.projection, field.encoded_end, child,
                        resolved.slot, resolved.repeated, field.number, frame_bytes,
                    )
                }
            },
        }
    }
}

pub open spec fn concrete_message_push_indexed(
    frame: ConcreteMessageFrameModel,
    field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState,
    parent_source: AdmissionIndexedSourceImage,
    frame_bytes: usize,
) -> Option<AdmissionState> {
    if !concrete_message_frame_rel(frame)
        || !indexed_field_at_cursor(frame.cursor, field)
        || parent_source.bytes != frame.cursor.source_bytes
        || !admission_indexed_source_wf(parent_source)
        || admission_indexed_source_position(parent_source, 0).unwrap().source
            != frame.projection.frames[frame.projection.frames.len() - 1].physical_source
        || admission_indexed_source_position(parent_source, 0).unwrap().offset
            != frame.projection.frames[frame.projection.frames.len() - 1].physical_start
        || child.region_start != 0
        || child.region_end as int
            != field.payload_end as int - field.payload_start as int
        || child.source_bytes != field.payload_bytes
        || child.physical_source != admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().source
        || child.physical_start != admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().offset
    {
        None
    } else {
        let top = frame.projection.frames[frame.projection.frames.len() - 1];
        match generated::generated_descriptor(top.program_type_id, field.number) {
            None => None,
            Some(resolved) => {
                if resolved.frame_kind == 0 || resolved.wire != field.wire
                    || child.frame_kind != resolved.frame_kind
                    || child.program_type_id != resolved.child_type_id.unwrap_or(0)
                    || (child.frame_kind == 1
                        && generated::generated_slot_count(child.program_type_id)
                            != Some(child.slot_count))
                {
                    None
                } else {
                    admission_push_step(
                        frame.projection, field.encoded_end, child,
                        resolved.slot, resolved.repeated, field.number, frame_bytes,
                    )
                }
            },
        }
    }
}

pub proof fn admission_indexed_push_preserves_par_source_trace(
    state: AdmissionState,
    field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState,
    descriptor: generated::GeneratedDescriptor,
    frame_bytes: usize,
)
    requires
        admission_par_source_trace_invariant(state),
        state.frames.len() > 0,
        generated::generated_slot_count(state.frames.last().program_type_id)
            == Some(state.frames.last().slot_count),
        indexed_field_at_cursor(IndexedCanonicalFieldCursorModel {
            region_start: state.frames.last().region_start,
            region_end: state.frames.last().region_end,
            next: state.frames.last().cursor,
            source_bytes: state.frames.last().source_bytes,
        }, field),
        generated::generated_descriptor(
            state.frames.last().program_type_id, field.number,
        ) == Some(descriptor),
        descriptor.frame_kind != 0,
        field.wire == descriptor.wire,
    ensures match admission_push_step(
        state, field.encoded_end, child, descriptor.slot,
        descriptor.repeated, field.number, frame_bytes,
    ) {
        None => true,
        Some(next) => admission_par_source_trace_invariant(next),
    },
{
    if let Some(next) = admission_push_step(
        state, field.encoded_end, child, descriptor.slot,
        descriptor.repeated, field.number, frame_bytes,
    ) {
        let before = state.frames.last();
        let parent_after = next.frames[next.frames.len() - 2];
        let child_after = next.frames.last();
        if before.program_type_id == 1 && before.frame_kind == 1
            && before.family == AdmissionFamily::Other
        {
            assert(before.slot_count == 12);
            let fields = choose|fields: Seq<IndexedCanonicalFieldModel>|
                admission_par_source_prefix_fields(before, fields);
            admission_par_source_prefix_after_field(
                before, parent_after, fields, field, descriptor,
            );
        }
        if child_after.program_type_id == 1 && child_after.frame_kind == 1
            && child_after.family == AdmissionFamily::Other
        {
            admission_par_empty_frame_has_source_prefix(child_after);
        }
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_par_source_prefix(next.frames[i]) by {
            if i < state.frames.len() - 1 {
                assert(next.frames[i] == state.frames[i]);
            }
        }
        assert forall|i: int| 0 <= i < next.frames.len()
            implies admission_source_identity_in_returns(next.frames[i]) by {
            if i < state.frames.len() - 1 {
                assert(next.frames[i] == state.frames[i]);
            } else if i == state.frames.len() - 1 {
                assert(next.frames[i].returns == before.returns);
            } else {
                assert(next.frames[i].returns.len() == 0);
            }
        }
    }
}

pub proof fn concrete_message_push_indexed_source_anchor_exact(
    frame: ConcreteMessageFrameModel,
    field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState,
    parent_source: AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    ensures match concrete_message_push_indexed(
        frame, field, child, parent_source, frame_bytes,
    ) {
        None => true,
        Some(next) => {
            let parent = frame.projection.frames.last();
            let pushed = next.frames.last();
            &&& parent.physical_source
                == admission_indexed_source_position(parent_source, 0).unwrap().source
            &&& parent.physical_start
                == admission_indexed_source_position(parent_source, 0).unwrap().offset
            &&& pushed.physical_source
                == admission_indexed_source_position(
                    parent_source, field.payload_start,
                ).unwrap().source
            &&& pushed.physical_start
                == admission_indexed_source_position(
                    parent_source, field.payload_start,
                ).unwrap().offset
            &&& pushed.source_bytes == field.payload_bytes
        },
    },
{}

pub proof fn concrete_message_push_indexed_rejects_wrong_source(
    frame: ConcreteMessageFrameModel,
    field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState,
    parent_source: AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires
        admission_indexed_source_wf(parent_source),
        (field.payload_start as int) <= parent_source.bytes.len(),
        child.physical_source != admission_indexed_source_position(
            parent_source, field.payload_start,
        ).unwrap().source,
    ensures concrete_message_push_indexed(
        frame, field, child, parent_source, frame_bytes,
    ).is_none(),
{}

pub proof fn concrete_message_push_simulates(
    frame: ConcreteMessageFrameModel, field: IndexedCanonicalFieldModel,
    child: AdmissionFrameState, frame_bytes: usize,
)
    ensures match concrete_message_push(frame, field, child, frame_bytes) {
        Some(next) => {
            &&& next.frames.len() == frame.projection.frames.len() + 1
            &&& next.frames[next.frames.len() - 2].cursor == field.encoded_end
            &&& next.frames[next.frames.len() - 2].waiting_slot.is_some()
            &&& next.frames[next.frames.len() - 2].waiting_tag == Some(field.number)
            &&& next.frames[next.frames.len() - 1].physical_start as int
                == frame.projection.frames[frame.projection.frames.len() - 1].physical_start as int
                    + field.payload_start as int
            &&& next.frames[next.frames.len() - 1].region_start == 0
            &&& next.frames[next.frames.len() - 1].region_end as int
                == field.payload_end as int - field.payload_start as int
            &&& match generated::generated_descriptor(
                frame.projection.frames[frame.projection.frames.len() - 1].program_type_id,
                field.number,
            ) {
                Some(resolved) => {
                    &&& next.frames[next.frames.len() - 1].frame_kind
                        == resolved.frame_kind
                    &&& next.frames[next.frames.len() - 1].program_type_id
                        == resolved.child_type_id.unwrap_or(0)
                },
                None => false,
            }
            &&& next.frames[next.frames.len() - 1].return_id
                == next.frames[next.frames.len() - 2].waiting_id
            &&& match generated::generated_descriptor(
                frame.projection.frames[frame.projection.frames.len() - 1].program_type_id,
                field.number,
            ) {
                Some(resolved) => admission_push_step(
                    frame.projection, field.encoded_end, child,
                    resolved.slot, resolved.repeated, field.number, frame_bytes,
                ) == Some(next),
                None => false,
            }
        },
        None => true,
    },
{}

/// Restricted child-summary extraction. Each listed fact is computed from
/// the finalized child frame, so `FrameAction::Complete` cannot receive an
/// invented source-present, required-present, fixed-point, or score flag.
/// Other `FrameSummary` variants still need a richer body/score projection.
pub open spec fn concrete_derived_summary(
    parent: AdmissionFrameState, child: AdmissionFrameState,
) -> Option<AdmissionReturnedKind> {
    if parent.program_type_id == 1 {
        match parent.waiting_tag {
            Some(tag) if admission_raw_par_component_child(tag, child) =>
                match admission_component_path_stability(child) {
                    Some(canonical_path_stable) => Some(AdmissionReturnedKind::Component {
                        family_tag: tag,
                        canonical_path_stable,
                    }),
                    None => None,
                },
            _ => None,
        }
    } else if child.frame_kind == 1 && child.program_type_id == 1 {
        match admission_raw_par_path_stability(child) {
            Some(canonical_path_stable) => Some(AdmissionReturnedKind::Par {
                semantic_default: admission_raw_par_semantic_default(child),
                canonical_path_stable,
            }),
            None => None,
        }
    } else if parent.family == AdmissionFamily::Other
        && parent.program_type_id == 32
        && parent.waiting_slot == Some(0)
        && parent.waiting_tag == Some(1)
    {
        admission_ezipper_pathmap_payload(parent, child)
    } else if parent.family == AdmissionFamily::Expr
        && parent.waiting_slot == Some(0)
        && (parent.waiting_tag == Some(5) || parent.waiting_tag == Some(6)
            || parent.waiting_tag == Some(20) || parent.waiting_tag == Some(21)
            || parent.waiting_tag == Some(24)
            || parent.waiting_tag == Some(32)
            || parent.waiting_tag == Some(33)
            || match parent.waiting_tag {
                Some(tag) => (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31),
                None => false,
            })
    {
        admission_expr_structural_payload(parent, child)
    } else { match child.family {
        AdmissionFamily::ReceiveBindFrame => Some(AdmissionReturnedKind::ReceiveBind {
            source_present: admission_returned_par(child, 1),
        }),
        AdmissionFamily::MatchCaseFrame => Some(AdmissionReturnedKind::MatchCase {
            required_present: admission_returned_par(child, 0)
                && admission_returned_par(child, 1),
            canonical_fixed_point: !admission_returned_default_par(child, 3),
        }),
        AdmissionFamily::CostSignatureFrame | AdmissionFamily::CostRegionFrame =>
            Some(AdmissionReturnedKind::CostScore),
        AdmissionFamily::BigRationalFrame => match admission_big_rational_fields(child) {
            Some((numerator, denominator)) => Some(AdmissionReturnedKind::BigRational {
                numerator, denominator,
            }),
            None => None,
        },
        AdmissionFamily::FixedPointFrame => match admission_fixed_point_fields(child) {
            Some((unscaled, scale)) => Some(AdmissionReturnedKind::FixedPoint {
                unscaled, scale,
            }),
            None => None,
        },
        AdmissionFamily::EVarFrame if parent.family == AdmissionFamily::Expr
            && parent.waiting_tag == Some(19) => Some(AdmissionReturnedKind::ExprPayload {
                score_present: admission_returned_var(child, 0),
                canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        AdmissionFamily::Other if child.frame_kind == 1
            && child.program_type_id == 34
            && parent.family == AdmissionFamily::Other
            && parent.program_type_id == 30
            && parent.waiting_slot == Some(0)
            && parent.waiting_tag == Some(1) =>
            match admission_key_value_pair_fields(child) {
                Some((key, value_par)) => Some(AdmissionReturnedKind::KeyValuePair {
                    key, value_par,
                }),
                None => None,
            },
        AdmissionFamily::Other if admission_opaque_child_edge(parent, child) =>
            Some(AdmissionReturnedKind::Other),
        AdmissionFamily::Other if child.frame_kind == 2
            && parent.family == AdmissionFamily::New
            && parent.waiting_slot == Some(3)
            && parent.waiting_tag == Some(4)
            && admission_map_summary_complete(child) => Some(AdmissionReturnedKind::Map {
                key_bytes: admission_map_key_bytes(child),
                value_score_handle: admission_map_value_score_handle(child),
            }),
        AdmissionFamily::Other if child.frame_kind == 1 => match child.program_type_id {
            6 => Some(AdmissionReturnedKind::VarScore),
            7 | 64 => Some(AdmissionReturnedKind::Unit),
            10 | 11 => Some(AdmissionReturnedKind::CostScore),
            56 => match admission_var_ref_fields(child) {
                Some((index, depth)) => Some(AdmissionReturnedKind::VarRef { index, depth }),
                None => None,
            },
            57 => match admission_connective_body_handles(child) {
                Some(par_handles) => Some(AdmissionReturnedKind::ConnectiveBody { par_handles }),
                None => None,
            },
            61 | 62 | 63 => match admission_bytes_leaf_payload(child) {
                Some(value) => Some(AdmissionReturnedKind::Bytes { value }),
                None => None,
            },
            _ => None,
        },
        _ => None,
    } }
}

pub open spec fn concrete_derived_summary_with_arena(
    parent: AdmissionFrameState, child: AdmissionFrameState,
    arena: Seq<AdmissionScoreBinding>,
) -> Option<AdmissionReturnedKind> {
    if parent.family == AdmissionFamily::Expr
        && parent.waiting_slot == Some(0)
        && (parent.waiting_tag == Some(22) || parent.waiting_tag == Some(23))
        && parent.program_type_id == 24
    {
        admission_expr_structural_payload_with_arena(parent, child, arena)
    } else {
        concrete_derived_summary(parent, child)
    }
}

pub open spec fn concrete_empty_set_map_score_transcript(constructor: u8) -> Seq<u8> {
    seq![2u8, 1u8, 1u8, 0x80u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, constructor]
        + seq![1u8, 1u8, 0x7fu8, 0xffu8, 0xffu8, 0xffu8, 0xffu8,
            0xffu8, 0xffu8, 0xffu8]
        + seq![1u8, 1u8, 0x80u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8]
        + seq![0u8]
}

pub open spec fn concrete_empty_set_map_parent_pop_state(is_set: bool) -> AdmissionState {
    let tag = if is_set { 22u32 } else { 23u32 };
    let program = if is_set { 29u16 } else { 30u16 };
    let constructor = if is_set { 8u8 } else { 9u8 };
    let parent = AdmissionFrameState {
        region_end: 3,
        source_bytes: if is_set { seq![0xb2u8, 0x01u8, 0u8] }
            else { seq![0xbau8, 0x01u8, 0u8] },
        cursor: 3,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(tag),
        score_handle: None,
        ..admission_family_witness_frame(
            AdmissionFamily::Expr, Set::empty(), Seq::empty(),
        )
    };
    let child = AdmissionFrameState {
        physical_start: 3,
        program_type_id: program,
        slot_count: 4,
        return_id: Some(0),
        body_id: 8,
        ..admission_family_witness_frame(
            AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    AdmissionState {
        frames: seq![parent, child],
        arena: seq![AdmissionScoreBinding {
            body_id: 8,
            semantic: true,
            encoded_len: 32,
            transcript_bytes: concrete_empty_set_map_score_transcript(constructor),
        }],
        root_end: 3,
        pushed: 2,
        completed: 0,
        next_obligation_id: 1,
    }
}

pub proof fn concrete_empty_set_map_parent_pop_is_reachable(is_set: bool)
    ensures
        admission_state_wf(concrete_empty_set_map_parent_pop_state(is_set)),
        concrete_message_complete_child(
            concrete_empty_set_map_parent_pop_state(is_set),
        ).is_some(),
{
    let state = concrete_empty_set_map_parent_pop_state(is_set);
    assert(admission_frame_wf(state.frames[0]));
    assert(admission_frame_wf(state.frames[1]));
    assert(state.frames[1].source_bytes == state.frames[0].source_bytes.subrange(3, 3));
    assert(admission_child_physical_contained(state.frames[0], state.frames[1]));
    assert(admission_state_wf(state));
    assert(concrete_message_complete_child(state).is_some());
}

pub open spec fn concrete_empty_eset_expr_witness(
) -> (AdmissionFrameState, AdmissionFrameState, Seq<AdmissionScoreBinding>) {
    let parent = AdmissionFrameState {
        waiting_slot: Some(0),
        waiting_tag: Some(22),
        ..admission_family_witness_frame(
            AdmissionFamily::Expr, Set::empty(), Seq::empty(),
        )
    };
    let child = AdmissionFrameState {
        program_type_id: 29,
        slot_count: 4,
        ..admission_family_witness_frame(
            AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    let arena = seq![AdmissionScoreBinding {
        body_id: child.body_id,
        semantic: true,
        encoded_len: 32,
        transcript_bytes: concrete_empty_set_map_score_transcript(8u8),
    }];
    (parent, child, arena)
}

pub proof fn concrete_empty_eset_expr_tag22_is_source_derived()
    ensures
        match generated::generated_descriptor(24, 22) {
            Some(descriptor) => descriptor.slot == 0
                && descriptor.child_type_id == Some(29)
                && descriptor.frame_kind == 1,
            None => false,
        },
        {
            let (parent, child, arena) = concrete_empty_eset_expr_witness();
            admission_eset_source_summary(child, arena) == Some(true)
                && concrete_derived_summary_with_arena(parent, child, arena)
                    == Some(AdmissionReturnedKind::ExprPayload {
                        score_present: true,
                        canonical_fixed_point: true,
                        canonical_path_stable: false,
                    })
        },
{
    let (parent, child, arena) = concrete_empty_eset_expr_witness();
    assert(admission_eset_source_summary(child, arena) == Some(true));
    assert(concrete_derived_summary_with_arena(parent, child, arena)
        == Some(AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
}

pub open spec fn concrete_empty_emap_expr_witness(
) -> (AdmissionFrameState, AdmissionFrameState, Seq<AdmissionScoreBinding>) {
    let parent = AdmissionFrameState {
        waiting_slot: Some(0),
        waiting_tag: Some(23),
        ..admission_family_witness_frame(
            AdmissionFamily::Expr, Set::empty(), Seq::empty(),
        )
    };
    let child = AdmissionFrameState {
        program_type_id: 30,
        slot_count: 4,
        ..admission_family_witness_frame(
            AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    let arena = seq![AdmissionScoreBinding {
        body_id: child.body_id,
        semantic: true,
        encoded_len: 32,
        transcript_bytes: concrete_empty_set_map_score_transcript(9u8),
    }];
    (parent, child, arena)
}

pub proof fn concrete_empty_emap_expr_tag23_is_source_derived()
    ensures
        match generated::generated_descriptor(24, 22) {
            Some(descriptor) => descriptor.slot == 0
                && descriptor.child_type_id == Some(29),
            None => false,
        },
        match generated::generated_descriptor(24, 23) {
            Some(descriptor) => descriptor.slot == 0
                && descriptor.child_type_id == Some(30)
                && descriptor.frame_kind == 1,
            None => false,
        },
        {
            let (parent, child, arena) = concrete_empty_emap_expr_witness();
            admission_emap_source_summary(child, arena) == Some((true, true))
                && concrete_derived_summary_with_arena(parent, child, arena)
                    == Some(AdmissionReturnedKind::ExprPayload {
                        score_present: true,
                        canonical_fixed_point: true,
                        canonical_path_stable: false,
                    })
        },
{
    let (parent, child, arena) = concrete_empty_emap_expr_witness();
    assert(admission_emap_source_summary(child, arena) == Some((true, true)));
    assert(concrete_derived_summary_with_arena(parent, child, arena)
        == Some(AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
}

pub proof fn concrete_derived_emap_is_source_derived(
    parent: AdmissionFrameState, child: AdmissionFrameState,
    arena: Seq<AdmissionScoreBinding>,
)
    requires
        parent.family == AdmissionFamily::Expr,
        parent.program_type_id == 24,
        parent.waiting_slot == Some(0),
        parent.waiting_tag == Some(23),
    ensures match concrete_derived_summary_with_arena(parent, child, arena) {
        Some(AdmissionReturnedKind::ExprPayload {
            score_present, canonical_fixed_point, canonical_path_stable,
        }) => admission_emap_source_summary(child, arena)
            == Some((score_present, canonical_fixed_point))
            && !canonical_path_stable,
        _ => true,
    },
{}

pub proof fn concrete_derived_path_stability_is_child_derived(
    parent: AdmissionFrameState, child: AdmissionFrameState,
)
    ensures match concrete_derived_summary(parent, child) {
        Some(AdmissionReturnedKind::Par { canonical_path_stable, .. }) =>
            admission_raw_par_path_stability(child) == Some(canonical_path_stable),
        Some(AdmissionReturnedKind::Component { canonical_path_stable, .. }) =>
            admission_component_path_stability(child) == Some(canonical_path_stable),
        _ => true,
    },
{}

/// Driver `Complete`: pop the finalized child and accept its derived summary
/// into the waiting message parent. The existing abstract guards check the
/// exact obligation ID, kind, slot, selected arm, score, and body identity.
pub open spec fn concrete_message_complete_child(
    state: AdmissionState,
) -> Option<ConcreteMessageFrameModel> {
    if state.frames.len() <= 1 { None } else {
        let child = state.frames[state.frames.len() - 1];
        let parent = state.frames[state.frames.len() - 2];
        if parent.frame_kind != 1 || child.return_id.is_none() { None } else {
            match concrete_derived_summary_with_arena(parent, child, state.arena) {
                None => None,
                Some(summary) => match admission_complete_child_step(
                    state, child.return_id.unwrap(), child.frame_kind, summary,
                ) {
                    None => None,
                    Some(next) => concrete_message_view(next),
                },
            }
        }
    }
}

pub proof fn concrete_message_complete_child_simulates(state: AdmissionState)
    ensures match concrete_message_complete_child(state) {
        Some(next) => {
            &&& concrete_message_frame_rel(next)
            &&& next.projection.frames.len() + 1 == state.frames.len()
            &&& next.projection.completed as int == state.completed as int + 1
            &&& next.waiting_id.is_none()
            &&& next.waiting_slot.is_none()
            &&& next.projection.frames.last().physical_source
                == state.frames[state.frames.len() - 2].physical_source
            &&& next.projection.frames.last().physical_start
                == state.frames[state.frames.len() - 2].physical_start
            &&& match concrete_derived_summary_with_arena(
                state.frames[state.frames.len() - 2],
                state.frames[state.frames.len() - 1],
                state.arena,
            ) {
                Some(summary) => admission_complete_child_step(
                    state,
                    state.frames[state.frames.len() - 1].return_id.unwrap(),
                    state.frames[state.frames.len() - 1].frame_kind,
                    summary,
                ) == Some(next.projection),
                None => false,
            }
        },
        None => true,
    },
{
    if let Some(next) = concrete_message_complete_child(state) {
        concrete_message_view_rel(next.projection);
    }
}

pub proof fn concrete_message_emap_expr_parent_pop_is_source_derived(
    state: AdmissionState,
)
    requires
        state.frames.len() > 1,
        state.frames[state.frames.len() - 2].family == AdmissionFamily::Expr,
        state.frames[state.frames.len() - 2].program_type_id == 24,
        state.frames[state.frames.len() - 2].waiting_slot == Some(0),
        state.frames[state.frames.len() - 2].waiting_tag == Some(23),
        state.frames.last().program_type_id == 30,
    ensures match concrete_message_complete_child(state) {
        None => true,
        Some(next) => {
            let returned = next.projection.frames.last().returns.last();
            match returned.kind {
                AdmissionReturnedKind::ExprPayload {
                    score_present, canonical_fixed_point, canonical_path_stable,
                } => admission_emap_source_summary(
                    state.frames.last(), state.arena,
                ) == Some((score_present, canonical_fixed_point))
                    && !canonical_path_stable
                    && returned.body_bytes == state.frames.last().source_bytes,
                _ => false,
            }
        },
    },
{
    let parent = state.frames[state.frames.len() - 2];
    let child = state.frames.last();
    concrete_derived_emap_is_source_derived(parent, child, state.arena);
    concrete_message_complete_child_simulates(state);
}

/// Root `Complete` has no parent to accept the summary. It is valid only at
/// exact root exhaustion, with all nested obligations resolved and the score
/// lane derived from the root family's semantic completeness.
pub open spec fn concrete_message_root_complete(frame: ConcreteMessageFrameModel) -> bool {
    concrete_message_frame_rel(frame)
        && frame.projection.frames.len() == 1
        && admission_complete_root(frame.projection)
}

pub proof fn concrete_message_root_simulates(frame: ConcreteMessageFrameModel)
    ensures concrete_message_root_complete(frame) ==> {
        &&& frame.cursor.next == frame.projection.root_end
        &&& frame.waiting_id.is_none()
        &&& frame.projection.pushed as int == frame.projection.completed as int + 1
    },
{}

/// Production `FrameAction` projected at the driver boundary. A `Push`
/// carries the newly created child request, while `Complete` carries no
/// caller-made summary: the child state determines it above.
pub enum ConcreteFrameActionModel {
    Continue { field: IndexedCanonicalFieldModel },
    ContinueIndexed {
        field: IndexedCanonicalFieldModel,
        parent_source: AdmissionIndexedSourceImage,
    },
    Push {
        field: IndexedCanonicalFieldModel,
        child: AdmissionFrameState,
        frame_bytes: usize,
    },
    PushIndexed {
        field: IndexedCanonicalFieldModel,
        child: AdmissionFrameState,
        parent_source: AdmissionIndexedSourceImage,
        frame_bytes: usize,
    },
    Complete,
}

pub enum ConcreteFrameOutcomeModel {
    Continued { state: AdmissionState },
    Pushed { state: AdmissionState },
    ParentAccepted { state: AdmissionState },
    RootFinished { summary: ConcreteRootSummaryModel },
}

pub struct ConcreteRootSummaryModel {
    pub program_type_id: u16,
    pub family: AdmissionFamily,
    pub complete: bool,
    pub score_handle: usize,
    pub body_id: u64,
    pub ghost body_bytes: Seq<u8>,
}

pub open spec fn concrete_message_root_summary(
    state: AdmissionState,
) -> Option<ConcreteRootSummaryModel> {
    if !admission_complete_root(state) {
        None
    } else {
        let root = state.frames[0];
        Some(ConcreteRootSummaryModel {
            program_type_id: root.program_type_id,
            family: root.family,
            complete: admission_family_complete(root),
            score_handle: root.score_handle.unwrap(),
            body_id: root.body_id,
            body_bytes: root.source_bytes,
        })
    }
}

pub open spec fn concrete_message_driver_step(
    state: AdmissionState, action: ConcreteFrameActionModel,
) -> Option<ConcreteFrameOutcomeModel> {
    match action {
        ConcreteFrameActionModel::Continue { field } => {
            match concrete_message_view(state) {
                None => None,
                Some(frame) => match concrete_message_continue(frame, field) {
                    None => None,
                    Some(next) => Some(ConcreteFrameOutcomeModel::Continued {
                        state: next.projection,
                    }),
                },
            }
        },
        ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            match concrete_message_view(state) {
                None => None,
                Some(frame) => match concrete_message_continue_indexed(
                    frame, field, parent_source,
                ) {
                    None => None,
                    Some(next) => Some(ConcreteFrameOutcomeModel::Continued {
                        state: next.projection,
                    }),
                },
            }
        },
        ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            match concrete_message_view(state) {
                None => None,
                Some(frame) => match concrete_message_push(frame, field, child, frame_bytes) {
                    None => None,
                    Some(next) => Some(ConcreteFrameOutcomeModel::Pushed { state: next }),
                },
            }
        },
        ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => match concrete_message_view(state) {
            None => None,
            Some(frame) => match concrete_message_push_indexed(
                frame, field, child, parent_source, frame_bytes,
            ) {
                None => None,
                Some(next) => Some(ConcreteFrameOutcomeModel::Pushed { state: next }),
            },
        },
        ConcreteFrameActionModel::Complete => {
            if state.frames.len() == 1 {
                match concrete_message_view(state) {
                    None => None,
                    Some(frame) => if concrete_message_root_complete(frame) {
                        match concrete_message_root_summary(state) {
                            Some(summary) => Some(ConcreteFrameOutcomeModel::RootFinished {
                                summary,
                            }),
                            None => None,
                        }
                    } else { None },
                }
            } else {
                match concrete_message_complete_child(state) {
                    None => None,
                    Some(next) => Some(ConcreteFrameOutcomeModel::ParentAccepted {
                        state: next.projection,
                    }),
                }
            }
        },
    }
}

pub proof fn concrete_message_driver_indexed_continue_anchor_exact(
    state: AdmissionState, field: IndexedCanonicalFieldModel,
    parent_source: AdmissionIndexedSourceImage,
)
    requires concrete_message_driver_step(
        state, ConcreteFrameActionModel::ContinueIndexed { field, parent_source },
    ).is_some(),
    ensures match concrete_message_driver_step(
        state, ConcreteFrameActionModel::ContinueIndexed { field, parent_source },
    ) {
        Some(ConcreteFrameOutcomeModel::Continued { state: next }) => {
            let occurrence = next.frames.last().scalar_occurrences.last();
            let position = admission_indexed_source_position(
                parent_source, field.encoded_start,
            ).unwrap();
            &&& occurrence.physical_source == position.source
            &&& occurrence.physical_start == position.offset as int
            &&& occurrence.tag == field.number
            &&& occurrence.bytes == field.payload_bytes
            &&& next.frames.last().cursor == field.encoded_end
        },
        _ => false,
    },
{
    let frame = concrete_message_view(state).unwrap();
    concrete_message_continue_indexed_anchor_exact(
        frame, field, parent_source,
    );
}

pub proof fn concrete_message_driver_preserves_par_source_trace(
    state: AdmissionState, action: ConcreteFrameActionModel,
)
    requires admission_par_source_trace_invariant(state),
    ensures match concrete_message_driver_step(state, action) {
        Some(ConcreteFrameOutcomeModel::Continued { state: next })
        | Some(ConcreteFrameOutcomeModel::Pushed { state: next })
        | Some(ConcreteFrameOutcomeModel::ParentAccepted { state: next }) =>
            admission_par_source_trace_invariant(next),
        _ => true,
    },
{
    match action {
        ConcreteFrameActionModel::Continue { field } => {
            if let Some(frame) = concrete_message_view(state) {
                concrete_message_continue_preserves_par_source_trace(frame, field);
            }
        },
        ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            if let Some(frame) = concrete_message_view(state) {
                concrete_message_continue_indexed_preserves_par_source_trace(
                    frame, field, parent_source,
                );
            }
        },
        ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            if let Some(frame) = concrete_message_view(state) {
                if let Some(descriptor) = generated::generated_descriptor(
                    frame.projection.frames.last().program_type_id, field.number,
                ) {
                    if concrete_message_push(frame, field, child, frame_bytes).is_some() {
                        admission_indexed_push_preserves_par_source_trace(
                            state, field, child, descriptor, frame_bytes,
                        );
                    }
                }
            }
        },
        ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source: _, frame_bytes,
        } => {
            if let Some(frame) = concrete_message_view(state) {
                if let Some(descriptor) = generated::generated_descriptor(
                    frame.projection.frames.last().program_type_id, field.number,
                ) {
                    if let Some(source) = concrete_message_view(state) {
                        if indexed_field_at_cursor(source.cursor, field)
                            && descriptor.frame_kind != 0
                            && field.wire == descriptor.wire
                        {
                            admission_indexed_push_preserves_par_source_trace(
                                state, field, child, descriptor, frame_bytes,
                            );
                        }
                    }
                }
            }
        },
        ConcreteFrameActionModel::Complete => {
            if state.frames.len() > 1 {
                let child = state.frames.last();
                let parent = state.frames[state.frames.len() - 2];
                if let Some(summary) = concrete_derived_summary_with_arena(
                    parent, child, state.arena,
                ) {
                    if child.return_id.is_some() {
                        admission_complete_child_preserves_par_source_trace(
                            state, child.return_id.unwrap(), child.frame_kind, summary,
                        );
                    }
                }
            }
        },
    }
}

pub proof fn concrete_message_driver_step_simulates(
    state: AdmissionState, action: ConcreteFrameActionModel,
)
    ensures match concrete_message_driver_step(state, action) {
        Some(ConcreteFrameOutcomeModel::Continued { state: next }) => {
            &&& next.frames.len() == state.frames.len()
            &&& next.frames[next.frames.len() - 1].cursor
                > state.frames[state.frames.len() - 1].cursor
            &&& next.completed == state.completed
        },
        Some(ConcreteFrameOutcomeModel::Pushed { state: next }) => {
            &&& next.frames.len() == state.frames.len() + 1
            &&& next.pushed as int == state.pushed as int + 1
            &&& next.frames[next.frames.len() - 1].return_id
                == next.frames[next.frames.len() - 2].waiting_id
        },
        Some(ConcreteFrameOutcomeModel::ParentAccepted { state: next }) => {
            &&& next.frames.len() + 1 == state.frames.len()
            &&& next.completed as int == state.completed as int + 1
            &&& next.frames[next.frames.len() - 1].waiting_id.is_none()
        },
        Some(ConcreteFrameOutcomeModel::RootFinished { summary }) => {
            &&& admission_complete_root(state)
            &&& summary.program_type_id == state.frames[0].program_type_id
            &&& summary.family == state.frames[0].family
            &&& summary.complete == admission_family_complete(state.frames[0])
            &&& summary.score_handle == state.frames[0].score_handle.unwrap()
            &&& summary.body_id == state.frames[0].body_id
            &&& summary.body_bytes == state.frames[0].source_bytes
            &&& state.arena[summary.score_handle as int].semantic == summary.complete
        },
        None => true,
    },
{}

pub proof fn concrete_message_driver_preserves_wf_and_arena(
    state: AdmissionState, action: ConcreteFrameActionModel,
)
    requires admission_state_wf(state),
    ensures match concrete_message_driver_step(state, action) {
        Some(ConcreteFrameOutcomeModel::Continued { state: next })
        | Some(ConcreteFrameOutcomeModel::Pushed { state: next })
        | Some(ConcreteFrameOutcomeModel::ParentAccepted { state: next }) =>
            admission_state_wf(next) && next.arena == state.arena,
        _ => true,
    },
{
    match action {
        ConcreteFrameActionModel::Continue { field } => {
            if let Some(frame) = concrete_message_view(state) {
                let top = frame.projection.frames[frame.projection.frames.len() - 1];
                if let Some(resolved) = generated::generated_descriptor(
                    top.program_type_id, field.number,
                ) {
                    admission_record_scalar_occurrence_preserves_wf(
                        state, field, resolved.slot, resolved.repeated,
                        resolved.scalar_kind,
                    );
                }
            }
        },
        ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            if let Some(frame) = concrete_message_view(state) {
                let top = frame.projection.frames[frame.projection.frames.len() - 1];
                if let Some(resolved) = generated::generated_descriptor(
                    top.program_type_id, field.number,
                ) {
                    admission_record_scalar_occurrence_indexed_preserves_wf(
                        state, parent_source, field, resolved.slot,
                        resolved.repeated, resolved.scalar_kind,
                    );
                }
            }
        },
        ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            if let Some(frame) = concrete_message_view(state) {
                let top = frame.projection.frames[frame.projection.frames.len() - 1];
                if let Some(resolved) = generated::generated_descriptor(
                    top.program_type_id, field.number,
                ) {
                    admission_push_preserves_wf(
                        state, field.encoded_end, child, resolved.slot,
                        resolved.repeated, field.number, frame_bytes,
                    );
                }
            }
        },
        ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source: _, frame_bytes,
        } => {
            if let Some(frame) = concrete_message_view(state) {
                let top = frame.projection.frames[frame.projection.frames.len() - 1];
                if let Some(resolved) = generated::generated_descriptor(
                    top.program_type_id, field.number,
                ) {
                    admission_push_preserves_wf(
                        state, field.encoded_end, child, resolved.slot,
                        resolved.repeated, field.number, frame_bytes,
                    );
                }
            }
        },
        ConcreteFrameActionModel::Complete => {
            if state.frames.len() > 1 {
                let child = state.frames[state.frames.len() - 1];
                let parent = state.frames[state.frames.len() - 2];
                if let Some(returned_id) = child.return_id {
                    if let Some(summary) = concrete_derived_summary_with_arena(
                        parent, child, state.arena,
                    ) {
                        admission_complete_child_preserves_wf(
                            state, returned_id, child.frame_kind, summary,
                        );
                    }
                }
            }
        },
    }
}

pub open spec fn admission_frame_with_cursor(
    frame: AdmissionFrameState, next_cursor: usize,
) -> AdmissionFrameState {
    AdmissionFrameState {
        physical_source: frame.physical_source,
        physical_start: frame.physical_start,
        program_type_id: frame.program_type_id,
        region_start: frame.region_start,
        region_end: frame.region_end,
        source_bytes: frame.source_bytes,
        cursor: next_cursor,
        slot_count: frame.slot_count,
        last_slot: frame.last_slot,
        seen: frame.seen,
        required: frame.required,
        scalar_slots: frame.scalar_slots,
        scalar_occurrences: frame.scalar_occurrences,
        returns: frame.returns,
        family: frame.family,
        waiting_id: frame.waiting_id,
        waiting_kind: frame.waiting_kind,
        waiting_slot: frame.waiting_slot,
        waiting_tag: frame.waiting_tag,
        return_id: frame.return_id,
        frame_kind: frame.frame_kind,
        body_id: frame.body_id,
        score_handle: frame.score_handle,
        nested_started: frame.nested_started,
        nested_resolved: frame.nested_resolved,
    }
}

pub open spec fn admission_aux_advance(
    state: AdmissionState, next_cursor: usize, expected_kind: u8,
) -> Option<AdmissionState> {
    if state.frames.len() == 0 || expected_kind < 3 || expected_kind > 4 {
        None
    } else {
        let index = state.frames.len() - 1;
        let top = state.frames[index];
        if top.frame_kind != expected_kind
            || top.waiting_id.is_some()
            || next_cursor <= top.cursor
            || next_cursor > top.region_end
        {
            None
        } else {
            Some(AdmissionState {
                frames: state.frames.update(index, admission_frame_with_cursor(top, next_cursor)),
                arena: state.arena,
                root_end: state.root_end,
                pushed: state.pushed,
                completed: state.completed,
                next_obligation_id: state.next_obligation_id,
            })
        }
    }
}

pub open spec fn admission_epathmap_advance(
    state: AdmissionState, next_cursor: usize,
) -> Option<AdmissionState> {
    admission_aux_advance(state, next_cursor, 3)
}

pub open spec fn admission_canonical_path_advance(
    state: AdmissionState, next_cursor: usize,
) -> Option<AdmissionState> {
    admission_aux_advance(state, next_cursor, 4)
}

pub proof fn admission_aux_advance_is_separate(
    state: AdmissionState, next_cursor: usize, expected_kind: u8,
)
    ensures match admission_aux_advance(state, next_cursor, expected_kind) {
        Some(next) => {
            &&& next.frames.len() == state.frames.len()
            &&& next.frames[next.frames.len() - 1].frame_kind == expected_kind
            &&& next.frames[next.frames.len() - 1].cursor == next_cursor
            &&& next.frames[next.frames.len() - 1].last_slot
                == state.frames[state.frames.len() - 1].last_slot
            &&& next.frames[next.frames.len() - 1].seen
                == state.frames[state.frames.len() - 1].seen
            &&& admission_continue_step(state, next_cursor, 0, false, 1).is_none()
        },
        None => true,
    },
{}

pub proof fn admission_aux_advance_preserves_wf(
    state: AdmissionState, next_cursor: usize, expected_kind: u8,
)
    requires admission_state_wf(state),
    ensures match admission_aux_advance(state, next_cursor, expected_kind) {
        Some(next) => admission_state_wf(next),
        None => true,
    },
{
    if let Some(next) = admission_aux_advance(state, next_cursor, expected_kind) {
        let index = state.frames.len() - 1;
        assert(admission_frame_wf(state.frames[index]));
        assert(admission_frame_wf(next.frames[index]));
        assert forall|i: int| 0 <= i < index
            implies next.frames[i] == state.frames[i] by {
        }
        admission_last_frame_replacement_preserves_wf(state, next);
    }
}

/// Exhaustion alone is not an auxiliary-frame acceptance proof: the map
/// key/value rules and path topology reducer still have to succeed.
pub open spec fn admission_aux_root_exhausted(state: AdmissionState, expected_kind: u8) -> bool {
    if state.frames.len() != 1 || expected_kind < 3 || expected_kind > 4 {
        false
    } else {
        let root = state.frames[0];
        root.frame_kind == expected_kind
            && root.cursor == state.root_end
            && root.region_end == state.root_end
            && root.waiting_id.is_none()
            && root.nested_started == root.nested_resolved
            && state.pushed as int == state.completed as int + 1
    }
}

}

fn main() {}
