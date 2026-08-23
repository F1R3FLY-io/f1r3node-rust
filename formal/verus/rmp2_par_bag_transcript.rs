use vstd::prelude::*;

verus! {

/// One canonical, multiplicity-compressed semantic component.  `body_id` is a
/// formal identity for the complete canonical protobuf body and `body_len` is
/// the length written into the self-delimiting RMP2 record.  The bridge assumes
/// equality of `body_id` exactly when the admitted body byte regions are equal;
/// generated-cursor differentials exercise that assumption in Rust.  This file
/// does not claim to verify BLAKE2b-256 or generated protobuf parsing.
pub struct SemanticRecord {
    pub family: u32,
    pub multiplicity: u64,
    pub body_len: u64,
    pub body_id: u64,
}

/// Symbolic image of
/// `be32(family) || be64(multiplicity) || be64(body_len) || body`.
/// Keeping the complete body identity beside its explicit length captures the
/// self-delimiting accumulator equation without pretending that Verus has
/// verified the production hash implementation.
pub struct TranscriptFrame {
    pub family: u32,
    pub multiplicity: u64,
    pub body_len: u64,
    pub body_id: u64,
}

/// Schema-semantic observation made by either projection source for one
/// generated field. `kind` identifies the generated field shape; `count`
/// carries sequence/map cardinality; and `payload_id` identifies the complete
/// scalar, oneof payload, child projection, opaque ParBag summary, or PathMap
/// summary.  The Rust event differential establishes these equalities for
/// admitted values. This model begins after that admission boundary.
pub struct SemanticFieldObservation {
    pub tag: u32,
    pub kind: u8,
    pub count: u64,
    pub payload_id: u64,
}

/// Common field frame consumed by the RMP2 encoder after either the resident
/// generated cursor or canonical protobuf-region cursor has performed its
/// source-specific read.
pub struct SemanticFieldFrame {
    pub tag: u32,
    pub kind: u8,
    pub count: u64,
    pub payload_id: u64,
}

pub open spec fn project_resident_field(
    field: SemanticFieldObservation,
) -> SemanticFieldFrame {
    SemanticFieldFrame {
        tag: field.tag,
        kind: field.kind,
        count: field.count,
        payload_id: field.payload_id,
    }
}

pub open spec fn project_region_field(
    field: SemanticFieldObservation,
) -> SemanticFieldFrame {
    SemanticFieldFrame {
        tag: field.tag,
        kind: field.kind,
        count: field.count,
        payload_id: field.payload_id,
    }
}

pub open spec fn semantic_fields_correspond(
    resident: SemanticFieldObservation,
    region: SemanticFieldObservation,
) -> bool {
    resident.tag == region.tag
        && resident.kind == region.kind
        && resident.count == region.count
        && resident.payload_id == region.payload_id
}

/// This is a field-projection theorem, not a protobuf parser theorem. The
/// executable semantic-event differential proves the precondition for actual
/// admitted resident/region pairs across the maintained Expr corpus.
proof fn corresponding_semantic_fields_project_identically(
    resident: SemanticFieldObservation,
    region: SemanticFieldObservation,
)
    requires semantic_fields_correspond(resident, region),
    ensures project_resident_field(resident) == project_region_field(region),
{
}

/// One canonical protobuf component occurrence before adjacent-run
/// coalescing. Complete canonical-body equality is represented symbolically by
/// equal `body_len` and `body_id`; Rust gates compare the actual byte slices.
pub struct SemanticOccurrence {
    pub family: u32,
    pub body_len: u64,
    pub body_id: u64,
}

/// Pending-run state matching `canonical_par_view`: `completed_distinct`
/// counts flushed runs, while the optional previous record has not yet been
/// added to that count. `cardinality` counts every consumed occurrence.
pub struct AdjacentCoalescerState {
    pub has_previous: bool,
    pub previous_family: u32,
    pub previous_body_len: u64,
    pub previous_body_id: u64,
    pub run_multiplicity: u64,
    pub completed_distinct: u64,
    pub cardinality: u64,
}

pub open spec fn same_pending_occurrence(
    state: AdjacentCoalescerState,
    occurrence: SemanticOccurrence,
) -> bool {
    state.has_previous
        && state.previous_family == occurrence.family
        && state.previous_body_len == occurrence.body_len
        && state.previous_body_id == occurrence.body_id
}

/// Executable image of one production coalescer step. Equality extends only
/// the immediately pending run. A different occurrence flushes that run and
/// begins a new exact-multiplicity record; it never searches older runs.
pub fn executable_adjacent_coalescer_step(
    state: AdjacentCoalescerState,
    occurrence: SemanticOccurrence,
) -> (result: AdjacentCoalescerState)
    requires
        state.cardinality < u64::MAX,
        state.completed_distinct <= state.cardinality,
        state.run_multiplicity <= state.cardinality,
        state.has_previous ==> 0 < state.run_multiplicity,
        !state.has_previous ==> state.run_multiplicity == 0,
    ensures
        result.has_previous,
        result.previous_family == occurrence.family,
        result.previous_body_len == occurrence.body_len,
        result.previous_body_id == occurrence.body_id,
        result.cardinality as int == state.cardinality as int + 1,
        result.run_multiplicity as int == if same_pending_occurrence(state, occurrence) {
            state.run_multiplicity as int + 1
        } else {
            1
        },
        result.completed_distinct as int ==
            if state.has_previous && !same_pending_occurrence(state, occurrence) {
                state.completed_distinct as int + 1
            } else {
                state.completed_distinct as int
            },
        result.completed_distinct <= result.cardinality,
        result.run_multiplicity <= result.cardinality,
{
    let same = state.has_previous
        && state.previous_family == occurrence.family
        && state.previous_body_len == occurrence.body_len
        && state.previous_body_id == occurrence.body_id;
    let completed_distinct = if state.has_previous && !same {
        state.completed_distinct + 1
    } else {
        state.completed_distinct
    };
    let run_multiplicity = if same {
        state.run_multiplicity + 1
    } else {
        1
    };
    AdjacentCoalescerState {
        has_previous: true,
        previous_family: occurrence.family,
        previous_body_len: occurrence.body_len,
        previous_body_id: occurrence.body_id,
        run_multiplicity,
        completed_distinct,
        cardinality: state.cardinality + 1,
    }
}

/// Finalization flushes exactly the one pending run. This mirrors the explicit
/// `flush` after the canonical occurrence cursor reaches end-of-input.
pub fn executable_finalize_adjacent_distinct(
    state: AdjacentCoalescerState,
) -> (result: u64)
    requires
        state.completed_distinct <= state.cardinality,
        state.has_previous ==> state.completed_distinct < u64::MAX,
    ensures
        result as int == state.completed_distinct as int
            + if state.has_previous { 1int } else { 0int },
{
    if state.has_previous {
        state.completed_distinct + 1
    } else {
        state.completed_distinct
    }
}

/// Source model for an admitted nested Par. The recursive component identity
/// is deliberately retained so the opacity theorem can quantify over changes
/// that RMP2 must not traverse.
pub struct NestedParSource {
    pub recursive_component_identity: u64,
    pub summary_identity: u64,
    pub locally_free_identity: u64,
    pub connective_used: bool,
}

pub struct NestedParProjection {
    pub summary_identity: u64,
    pub locally_free_identity: u64,
    pub connective_used: bool,
}

pub open spec fn project_nested_par(source: NestedParSource) -> NestedParProjection {
    NestedParProjection {
        summary_identity: source.summary_identity,
        locally_free_identity: source.locally_free_identity,
        connective_used: source.connective_used,
    }
}

/// RMP2 exposes the opaque ParBag summary plus the two root metadata fields.
/// Recursive component identity is absent from the projection by definition.
proof fn nested_par_projection_is_opaque_to_recursive_components(
    left: NestedParSource,
    right: NestedParSource,
)
    requires
        left.summary_identity == right.summary_identity,
        left.locally_free_identity == right.locally_free_identity,
        left.connective_used == right.connective_used,
    ensures project_nested_par(left) == project_nested_par(right),
{
}

pub open spec fn frame_of(record: SemanticRecord) -> TranscriptFrame {
    TranscriptFrame {
        family: record.family,
        multiplicity: record.multiplicity,
        body_len: record.body_len,
        body_id: record.body_id,
    }
}

pub open spec fn framed_transcript(records: Seq<SemanticRecord>) -> Seq<TranscriptFrame> {
    Seq::new(records.len(), |index: int| frame_of(records[index]))
}

pub open spec fn records_correspond(
    resident: Seq<SemanticRecord>,
    coalesced_region_runs: Seq<SemanticRecord>,
) -> bool {
    resident.len() == coalesced_region_runs.len()
        && forall|index: int| 0 <= index < resident.len() ==>
            resident[index].family == coalesced_region_runs[index].family
            && resident[index].multiplicity == coalesced_region_runs[index].multiplicity
            && resident[index].body_len == coalesced_region_runs[index].body_len
            && resident[index].body_id == coalesced_region_runs[index].body_id
}

pub open spec fn saturating_add_spec(left: int, right: int) -> int
    recommends 0 <= left <= u64::MAX as int, 0 <= right <= u64::MAX as int,
{
    if left + right > u64::MAX as int {
        u64::MAX as int
    } else {
        left + right
    }
}

pub open spec fn saturated_cardinality(records: Seq<SemanticRecord>) -> int
    decreases records.len(),
{
    if records.len() == 0 {
        0
    } else {
        saturating_add_spec(
            records[0].multiplicity as int,
            saturated_cardinality(records.drop_first()),
        )
    }
}

proof fn corresponding_sequences_are_extentionally_equal(
    resident: Seq<SemanticRecord>,
    coalesced_region_runs: Seq<SemanticRecord>,
)
    requires records_correspond(resident, coalesced_region_runs),
    ensures resident =~= coalesced_region_runs,
{
    assert forall|index: int| 0 <= index < resident.len() implies
        resident[index] == coalesced_region_runs[index] by {
        assert(resident[index].family == coalesced_region_runs[index].family);
        assert(resident[index].multiplicity == coalesced_region_runs[index].multiplicity);
        assert(resident[index].body_len == coalesced_region_runs[index].body_len);
        assert(resident[index].body_id == coalesced_region_runs[index].body_id);
    }
}

/// Resident PathMap records and canonical protobuf occurrence runs feed the
/// same transcript and summary exactly when canonical admission/coalescing has
/// established record correspondence.  That explicit precondition is paired
/// with the Rust resident/region differential; it is not an assumption about
/// arbitrary protobuf bytes.
proof fn resident_and_region_accumulators_are_equivalent(
    resident: Seq<SemanticRecord>,
    coalesced_region_runs: Seq<SemanticRecord>,
)
    requires
        records_correspond(resident, coalesced_region_runs),
        forall|index: int| 0 <= index < resident.len() ==>
            0 < resident[index].multiplicity,
    ensures
        framed_transcript(resident) == framed_transcript(coalesced_region_runs),
        resident.len() == coalesced_region_runs.len(),
        saturated_cardinality(resident)
            == saturated_cardinality(coalesced_region_runs),
{
    corresponding_sequences_are_extentionally_equal(resident, coalesced_region_runs);
}

/// A concrete one-record witness keeps the bridge precondition visibly
/// satisfiable; the executable nine-family differential supplies the much
/// broader production witness set.
proof fn record_correspondence_precondition_is_non_vacuous(record: SemanticRecord)
    requires 0 < record.multiplicity,
    ensures
        records_correspond(seq![record], seq![record]),
        0 < seq![record][0].multiplicity,
{
}

/// Executable projection of `u64::saturating_add`.  The subtraction guard
/// proves that neither branch performs overflowing arithmetic.
pub fn executable_saturating_add(left: u64, right: u64) -> (result: u64)
    ensures result as int == saturating_add_spec(left as int, right as int),
{
    if right > u64::MAX - left {
        u64::MAX
    } else {
        left + right
    }
}

/// One occurrence-cursor step.  `distinct <= cursor` is maintained by the
/// coalescer: at most one new run begins per consumed occurrence.  Therefore
/// cursor, distinct, and region-cardinality increments cannot overflow.  The
/// reachable region state has `cardinality == cursor`; because the admitted
/// byte length itself fits `u64`, its exact next count also equals the
/// resident Bag's saturated-u64 result.
pub fn executable_occurrence_step(
    cursor: u64,
    input_len: u64,
    distinct: u64,
    cardinality: u64,
    begins_new_run: bool,
) -> (result: (u64, u64, u64))
    requires
        cursor < input_len,
        distinct <= cursor,
        cardinality == cursor,
    ensures
        result.0 as int == cursor as int + 1,
        result.1 as int == if begins_new_run {
            distinct as int + 1
        } else {
            distinct as int
        },
        result.2 as int == cardinality as int + 1,
        result.2 as int == saturating_add_spec(cardinality as int, 1),
        result.1 <= result.0,
{
    let next_cursor = cursor + 1;
    let next_distinct = if begins_new_run { distinct + 1 } else { distinct };
    let next_cardinality = cardinality + 1;
    (next_cursor, next_distinct, next_cardinality)
}

/// A coalesced record's multiplicity is exact transcript data, not a saturated
/// statistic. Before consuming the next equal occurrence, the current run
/// cannot be longer than the consumed prefix. Together with `cursor <
/// input_len`, that makes the checked production increment provably total.
pub fn executable_exact_run_multiplicity_increment(
    run_multiplicity: u64,
    cursor: u64,
    input_len: u64,
) -> (result: u64)
    requires
        run_multiplicity <= cursor,
        cursor < input_len,
    ensures
        result as int == run_multiplicity as int + 1,
{
    run_multiplicity + 1
}

/// The byte-region cursor checks this bound before the lossless cast used by
/// the `be64(body_len)` frame.  Consequently the projection has no truncation
/// and needs no panicking `expect` in the executable model.
pub fn executable_body_length(body_len: usize) -> (result: u64)
    requires body_len as int <= u64::MAX as int,
    ensures result as int == body_len as int,
{
    body_len as u64
}

}

fn main() {}
