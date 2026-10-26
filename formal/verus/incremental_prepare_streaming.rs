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

pub open spec fn full_source_replay_exact_spec(
    first_len: int,
    replay_len: int,
    first_identity: int,
    replay_identity: int,
) -> bool {
    first_len == replay_len && first_identity == replay_identity
}

/// The post-chain source gate authenticates the complete PREPARE CRC domain,
/// including every DATA body.  Length and domain-separated physical identity
/// must both match the first pass before index replay can begin.
pub fn full_source_replay_exact(
    first_len: u64,
    replay_len: u64,
    first_identity: u64,
    replay_identity: u64,
) -> (accepted: bool)
    ensures
        accepted == full_source_replay_exact_spec(
            first_len as int,
            replay_len as int,
            first_identity as int,
            replay_identity as int,
        ),
{
    first_len == replay_len && first_identity == replay_identity
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

pub open spec fn private_candidate_ready_spec(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    source_replay_exact: bool,
    replay_exact: bool,
    source_stable: bool,
) -> bool {
    extent_complete
        && crc_valid
        && semantics_valid
        && commit_bound
        && chain_bound
        && source_replay_exact
        && replay_exact
        && source_stable
}

pub open spec fn validation_accepted_spec(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    source_replay_exact: bool,
    replay_exact: bool,
    source_stable: bool,
    catalog_verified: bool,
) -> bool {
    private_candidate_ready_spec(
        extent_complete,
        crc_valid,
        semantics_valid,
        commit_bound,
        chain_bound,
        source_replay_exact,
        replay_exact,
        source_stable,
    ) && catalog_verified
}

/// The stream publishes authority only after all nine independent gates pass.
/// Keeping this Boolean kernel explicit makes early authority, index, or
/// observer exposure a direct executable refinement obligation.
pub fn validation_accepted(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    source_replay_exact: bool,
    replay_exact: bool,
    source_stable: bool,
    catalog_verified: bool,
) -> (accepted: bool)
    ensures
        accepted == validation_accepted_spec(
            extent_complete,
            crc_valid,
            semantics_valid,
            commit_bound,
            chain_bound,
            source_replay_exact,
            replay_exact,
            source_stable,
            catalog_verified,
        ),
{
    extent_complete
        && crc_valid
        && semantics_valid
        && commit_bound
        && chain_bound
        && source_replay_exact
        && replay_exact
        && source_stable
        && catalog_verified
}

pub proof fn catalog_mismatch_exposes_no_public_state(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    source_replay_exact: bool,
    replay_exact: bool,
    source_stable: bool,
    catalog_verified: bool,
)
    requires !catalog_verified,
    ensures
        !validation_accepted_spec(
            extent_complete,
            crc_valid,
            semantics_valid,
            commit_bound,
            chain_bound,
            source_replay_exact,
            replay_exact,
            source_stable,
            catalog_verified,
        ),
{
}

pub proof fn replay_complete_candidate_remains_private_until_catalog_verification(
    extent_complete: bool,
    crc_valid: bool,
    semantics_valid: bool,
    commit_bound: bool,
    chain_bound: bool,
    source_replay_exact: bool,
    replay_exact: bool,
    source_stable: bool,
    catalog_verified: bool,
)
    requires
        private_candidate_ready_spec(
            extent_complete,
            crc_valid,
            semantics_valid,
            commit_bound,
            chain_bound,
            source_replay_exact,
            replay_exact,
            source_stable,
        ),
        !catalog_verified,
    ensures
        !validation_accepted_spec(
            extent_complete,
            crc_valid,
            semantics_valid,
            commit_bound,
            chain_bound,
            source_replay_exact,
            replay_exact,
            source_stable,
            catalog_verified,
        ),
{
}

/// Step axes of a bounded private ledger trial, in the order the trial checks
/// them (`step_axes` in `IncrementalLedgerTrial.v`).  The two final-inventory
/// limits are checked only on the drained state, so they are not step axes.
pub enum TrialAxis {
    Count,
    UnionObjects,
    UnionBytes,
    Scratch,
    References,
    Expansions,
    QuotedBytes,
}

/// Charges accepted so far.  The first four are peaks of live quantities;
/// the last three are cumulative event counts.
pub struct TrialUsage {
    pub peak_count: u64,
    pub peak_objects: u64,
    pub peak_bytes: u64,
    pub peak_scratch: u64,
    pub references: u64,
    pub expansions: u64,
    pub quoted_bytes: u64,
}

pub struct TrialLimits {
    pub count: u64,
    pub union_objects: u64,
    pub union_bytes: u64,
    pub scratch: u64,
    pub references: u64,
    pub expansions: u64,
    pub quoted_bytes: u64,
}

/// One candidate transition: the live values it would reach and the
/// cumulative events it would add.
pub struct TrialCharge {
    pub count: u64,
    pub objects: u64,
    pub bytes: u64,
    pub live_scratch: u64,
    pub references: u64,
    pub expansions: u64,
    pub quoted_bytes: u64,
}

pub open spec fn trial_reserve_spec(used: int, amount: int, limit: int) -> Option<int> {
    if used + amount <= limit {
        Some(used + amount)
    } else {
        None
    }
}

pub open spec fn raised_peak_spec(peak: int, live: int) -> int {
    if peak < live {
        live
    } else {
        peak
    }
}

pub open spec fn trial_peak_spec(peak: int, live: int, limit: int) -> Option<int> {
    if raised_peak_spec(peak, live) <= limit {
        Some(raised_peak_spec(peak, live))
    } else {
        None
    }
}

pub open spec fn committed_axis_spec(
    usage: TrialUsage,
    charge: TrialCharge,
    axis: TrialAxis,
) -> int {
    match axis {
        TrialAxis::Count => raised_peak_spec(usage.peak_count as int, charge.count as int),
        TrialAxis::UnionObjects => raised_peak_spec(
            usage.peak_objects as int,
            charge.objects as int,
        ),
        TrialAxis::UnionBytes => raised_peak_spec(usage.peak_bytes as int, charge.bytes as int),
        TrialAxis::Scratch => raised_peak_spec(
            usage.peak_scratch as int,
            charge.live_scratch as int,
        ),
        TrialAxis::References => usage.references as int + charge.references as int,
        TrialAxis::Expansions => usage.expansions as int + charge.expansions as int,
        TrialAxis::QuotedBytes => usage.quoted_bytes as int + charge.quoted_bytes as int,
    }
}

pub open spec fn axis_limit_spec(limits: TrialLimits, axis: TrialAxis) -> int {
    match axis {
        TrialAxis::Count => limits.count as int,
        TrialAxis::UnionObjects => limits.union_objects as int,
        TrialAxis::UnionBytes => limits.union_bytes as int,
        TrialAxis::Scratch => limits.scratch as int,
        TrialAxis::References => limits.references as int,
        TrialAxis::Expansions => limits.expansions as int,
        TrialAxis::QuotedBytes => limits.quoted_bytes as int,
    }
}

pub open spec fn axis_refused_spec(
    usage: TrialUsage,
    charge: TrialCharge,
    limits: TrialLimits,
    axis: TrialAxis,
) -> bool {
    committed_axis_spec(usage, charge, axis) > axis_limit_spec(limits, axis)
}

pub open spec fn first_refused_axis_spec(
    usage: TrialUsage,
    charge: TrialCharge,
    limits: TrialLimits,
) -> Option<TrialAxis> {
    if axis_refused_spec(usage, charge, limits, TrialAxis::Count) {
        Some(TrialAxis::Count)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::UnionObjects) {
        Some(TrialAxis::UnionObjects)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::UnionBytes) {
        Some(TrialAxis::UnionBytes)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::Scratch) {
        Some(TrialAxis::Scratch)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::References) {
        Some(TrialAxis::References)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::Expansions) {
        Some(TrialAxis::Expansions)
    } else if axis_refused_spec(usage, charge, limits, TrialAxis::QuotedBytes) {
        Some(TrialAxis::QuotedBytes)
    } else {
        None
    }
}

pub open spec fn committed_usage_spec(
    before: TrialUsage,
    charge: TrialCharge,
    after: TrialUsage,
) -> bool {
    &&& after.peak_count as int == committed_axis_spec(before, charge, TrialAxis::Count)
    &&& after.peak_objects as int == committed_axis_spec(before, charge, TrialAxis::UnionObjects)
    &&& after.peak_bytes as int == committed_axis_spec(before, charge, TrialAxis::UnionBytes)
    &&& after.peak_scratch as int == committed_axis_spec(before, charge, TrialAxis::Scratch)
    &&& after.references as int == committed_axis_spec(before, charge, TrialAxis::References)
    &&& after.expansions as int == committed_axis_spec(before, charge, TrialAxis::Expansions)
    &&& after.quoted_bytes as int == committed_axis_spec(before, charge, TrialAxis::QuotedBytes)
}

/// Cumulative reservation.  The sum is never formed unless it fits the limit,
/// so machine overflow is a limit refusal rather than a wrapped charge.
pub fn checked_trial_reserve(used: u64, amount: u64, limit: u64) -> (result: Option<u64>)
    ensures
        match result {
            Some(total) => trial_reserve_spec(used as int, amount as int, limit as int) == Some(
                total as int,
            ),
            None => trial_reserve_spec(used as int, amount as int, limit as int).is_none(),
        },
{
    if amount > limit || used > limit - amount {
        None
    } else {
        Some(used + amount)
    }
}

/// Peak reservation for a live quantity: the reserved value is the larger of
/// the recorded peak and the candidate live value.
pub fn checked_trial_peak(peak: u64, live: u64, limit: u64) -> (result: Option<u64>)
    ensures
        match result {
            Some(next) => trial_peak_spec(peak as int, live as int, limit as int) == Some(
                next as int,
            ),
            None => trial_peak_spec(peak as int, live as int, limit as int).is_none(),
        },
{
    let next = if peak < live {
        live
    } else {
        peak
    };
    if next <= limit {
        Some(next)
    } else {
        None
    }
}

/// Releasing live scratch or inventory never underflows.
pub fn checked_trial_release(live: u64, released: u64) -> (result: Option<u64>)
    ensures
        match result {
            Some(next) => next as int == live as int - released as int && released <= live,
            None => live < released,
        },
{
    if released <= live {
        Some(live - released)
    } else {
        None
    }
}

/// Failure-atomic reservation of one trial transition.  All seven axes are
/// computed into locals before `usage` is written, so a refusal leaves it
/// unchanged and names the first refused axis in step-axis order.
pub fn reserve_trial_step(
    usage: &mut TrialUsage,
    charge: &TrialCharge,
    limits: &TrialLimits,
) -> (refused: Option<TrialAxis>)
    ensures
        refused == first_refused_axis_spec(*old(usage), *charge, *limits),
        refused.is_some() ==> *final(usage) == *old(usage),
        refused.is_none() ==> committed_usage_spec(*old(usage), *charge, *final(usage)),
{
    let count = match checked_trial_peak(usage.peak_count, charge.count, limits.count) {
        Some(value) => value,
        None => return Some(TrialAxis::Count),
    };
    let objects = match checked_trial_peak(
        usage.peak_objects,
        charge.objects,
        limits.union_objects,
    ) {
        Some(value) => value,
        None => return Some(TrialAxis::UnionObjects),
    };
    let bytes = match checked_trial_peak(usage.peak_bytes, charge.bytes, limits.union_bytes) {
        Some(value) => value,
        None => return Some(TrialAxis::UnionBytes),
    };
    let scratch = match checked_trial_peak(
        usage.peak_scratch,
        charge.live_scratch,
        limits.scratch,
    ) {
        Some(value) => value,
        None => return Some(TrialAxis::Scratch),
    };
    let references = match checked_trial_reserve(
        usage.references,
        charge.references,
        limits.references,
    ) {
        Some(value) => value,
        None => return Some(TrialAxis::References),
    };
    let expansions = match checked_trial_reserve(
        usage.expansions,
        charge.expansions,
        limits.expansions,
    ) {
        Some(value) => value,
        None => return Some(TrialAxis::Expansions),
    };
    let quoted_bytes = match checked_trial_reserve(
        usage.quoted_bytes,
        charge.quoted_bytes,
        limits.quoted_bytes,
    ) {
        Some(value) => value,
        None => return Some(TrialAxis::QuotedBytes),
    };
    usage.peak_count = count;
    usage.peak_objects = objects;
    usage.peak_bytes = bytes;
    usage.peak_scratch = scratch;
    usage.references = references;
    usage.expansions = expansions;
    usage.quoted_bytes = quoted_bytes;
    None
}

pub proof fn exact_trial_limit_is_accepted(used: u64, amount: u64)
    requires
        used as int + amount as int <= u64::MAX as int,
    ensures
        trial_reserve_spec(used as int, amount as int, used as int + amount as int) == Some(
            used as int + amount as int,
        ),
{
}

pub proof fn one_below_trial_limit_is_refused(used: u64, amount: u64)
    requires
        0 < used as int + amount as int,
    ensures
        trial_reserve_spec(used as int, amount as int, used as int + amount as int - 1).is_none(),
{
}

pub proof fn trial_overflow_is_a_limit_refusal(used: u64, amount: u64, limit: u64)
    requires
        used as int + amount as int > u64::MAX as int,
    ensures
        trial_reserve_spec(used as int, amount as int, limit as int).is_none(),
{
}

/// Peak live scratch is not cumulative allocation: scratch released and then
/// reacquired within the recorded peak fits a limit equal to that peak.
pub proof fn released_scratch_is_reusable_under_its_peak(
    peak: u64,
    live: u64,
    released: u64,
    reacquired: u64,
)
    requires
        live <= peak,
        released <= live,
        live as int - released as int + reacquired as int <= peak as int,
    ensures
        trial_peak_spec(
            peak as int,
            live as int - released as int + reacquired as int,
            peak as int,
        ) == Some(peak as int),
{
}

} // verus!

fn main() {}
