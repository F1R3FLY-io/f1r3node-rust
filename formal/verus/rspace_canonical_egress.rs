use vstd::prelude::*;

verus! {

/// The fixed-width bincode length prefix used by the frozen RSpace cold-store
/// `Vec<Vec<u8>>` contract. The production encoder writes this as little-endian
/// `u64`; byte-order is covered by the executable byte differential, while this
/// model proves the cardinality and length arithmetic around those words.
pub const BINCODE_U64_BYTES: usize = 8;

/// The frozen bincode-1 outer enum tag and single-field vector length that
/// precede the canonical DATA payload in the final cold-store value.
pub const PERSISTED_DATA_TAG_BYTES: usize = 4;
pub const PERSISTED_DATA_HEADER_BYTES: usize =
    PERSISTED_DATA_TAG_BYTES + BINCODE_U64_BYTES;

/// Exact acceptance boundary for adding the final `PersistedData` envelope to
/// a fully preflighted payload. The payload length must fit both bincode's
/// `u64` field and the platform allocation after the 12-byte prefix.
pub open spec fn final_value_len_spec(payload_len: int) -> Option<int> {
    if payload_len < 0
        || payload_len > u64::MAX as int
        || payload_len > usize::MAX as int - PERSISTED_DATA_HEADER_BYTES as int
    {
        None
    } else {
        Some(payload_len + PERSISTED_DATA_HEADER_BYTES as int)
    }
}

/// Executable counterpart of the final capacity check in
/// `encode_canonical_persisted_data_at_prefix`.
pub fn checked_final_value_len(payload_len: usize) -> (result: Option<usize>)
    ensures
        match result {
            Some(final_len) => final_value_len_spec(payload_len as int)
                == Some(final_len as int),
            None => final_value_len_spec(payload_len as int).is_none(),
        },
{
    assert(usize::MAX as int <= u64::MAX as int);
    assert(payload_len as int <= u64::MAX as int);
    if payload_len > usize::MAX - PERSISTED_DATA_HEADER_BYTES {
        assert(payload_len as int
            > usize::MAX as int - PERSISTED_DATA_HEADER_BYTES as int);
        assert(final_value_len_spec(payload_len as int).is_none());
        None
    } else {
        let final_len = payload_len + PERSISTED_DATA_HEADER_BYTES;
        assert(final_len as int
            == payload_len as int + PERSISTED_DATA_HEADER_BYTES as int);
        assert(final_value_len_spec(payload_len as int) == Some(final_len as int));
        Some(final_len)
    }
}

/// Mathematical acceptance boundary for one preflighted canonical record.
/// A successful step adds `multiplicity` logical datums and that many framed
/// copies of the record to the final output length. Mathematical integers
/// prevent modular arithmetic from hiding either overflow.
pub open spec fn preflight_step_spec(
    datum_count: int,
    encoded_len: int,
    record_len: int,
    multiplicity: int,
) -> Option<(int, int)> {
    if datum_count < 0
        || encoded_len < 0
        || record_len < 0
        || multiplicity < 0
        || datum_count + multiplicity > u64::MAX as int
        || record_len > usize::MAX as int - BINCODE_U64_BYTES as int
        || encoded_len
            + multiplicity * (BINCODE_U64_BYTES as int + record_len)
            > usize::MAX as int
    {
        None
    } else {
        Some((
            datum_count + multiplicity,
            encoded_len
                + multiplicity * (BINCODE_U64_BYTES as int + record_len),
        ))
    }
}

/// Executable arithmetic kernel corresponding to the production
/// `encode_canonical_persisted_data_at_prefix` payload preflight. The implementation performs
/// no write: publication receives an exact cardinality and capacity only after
/// every record has passed this boundary.
#[verifier::spinoff_prover]
pub fn checked_preflight_step(
    datum_count: u64,
    encoded_len: usize,
    record_len: usize,
    multiplicity: u64,
) -> (result: Option<(u64, usize)>)
    requires 0 < multiplicity,
    ensures
        match result {
            Some((next_count, next_len)) => preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                record_len as int,
                multiplicity as int,
            ) == Some((next_count as int, next_len as int)),
            None => preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                record_len as int,
                multiplicity as int,
            ).is_none(),
        },
{
    if multiplicity > u64::MAX - datum_count {
        assert(datum_count as int + multiplicity as int > u64::MAX as int);
        assert(preflight_step_spec(
            datum_count as int,
            encoded_len as int,
            record_len as int,
            multiplicity as int,
        ).is_none());
        None
    } else if record_len > usize::MAX - BINCODE_U64_BYTES {
        assert(record_len as int
            > usize::MAX as int - BINCODE_U64_BYTES as int);
        assert(preflight_step_spec(
            datum_count as int,
            encoded_len as int,
            record_len as int,
            multiplicity as int,
        ).is_none());
        None
    } else {
        let next_count = datum_count + multiplicity;
        assert(next_count as int == datum_count as int + multiplicity as int);
        assert(datum_count as int + multiplicity as int <= u64::MAX as int);
        assert(record_len as int
            <= usize::MAX as int - BINCODE_U64_BYTES as int);
        let framed_len = record_len + BINCODE_U64_BYTES;
        assert(framed_len as int
            == record_len as int + BINCODE_U64_BYTES as int);
        assert(framed_len as int <= usize::MAX as int);
        assert(multiplicity as int <= u64::MAX as int);
        assert(usize::MAX as int <= u64::MAX as int);
        assert(
            (framed_len as int) * (multiplicity as int) <= u128::MAX as int
        ) by (nonlinear_arith);
        let repeated_len = (framed_len as u128) * (multiplicity as u128);
        let remaining = usize::MAX - encoded_len;
        assert(remaining as int == usize::MAX as int - encoded_len as int);
        if repeated_len > remaining as u128 {
            assert(repeated_len as int
                == (BINCODE_U64_BYTES as int + record_len as int)
                    * multiplicity as int);
            assert(repeated_len as int > remaining as int);
            assert(encoded_len as int + repeated_len as int > usize::MAX as int);
            assert(encoded_len as int
                + multiplicity as int
                    * (BINCODE_U64_BYTES as int + record_len as int)
                > usize::MAX as int);
            assert(preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                record_len as int,
                multiplicity as int,
            ).is_none());
            None
        } else {
            let repeated_len_usize = repeated_len as usize;
            assert(repeated_len as int <= remaining as int);
            assert(repeated_len_usize as int == repeated_len as int);
            assert(repeated_len_usize <= remaining);
            let final_len = encoded_len + repeated_len_usize;
            assert(repeated_len as int
                == (BINCODE_U64_BYTES as int + record_len as int)
                    * multiplicity as int);
            assert(final_len as int
                == encoded_len as int
                    + multiplicity as int
                        * (BINCODE_U64_BYTES as int + record_len as int));
            assert(preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                record_len as int,
                multiplicity as int,
            ) == Some((next_count as int, final_len as int)));
            Some((next_count, final_len))
        }
    }
}

/// Repeat one canonical record exactly as the legacy `Vec<Vec<u8>>`
/// semantics require. `record` is an injective formal name for the complete
/// resident byte string; Rust differentials establish that equal names mean
/// byte-identical admitted records.
pub open spec fn repeat_canonical_record(record: u64, multiplicity: nat) -> Seq<u64>
    decreases multiplicity,
{
    if multiplicity == 0 {
        Seq::empty()
    } else {
        seq![record] + repeat_canonical_record(record, (multiplicity - 1) as nat)
    }
}

pub proof fn repeat_canonical_record_length(record: u64, multiplicity: nat)
    ensures repeat_canonical_record(record, multiplicity).len() == multiplicity,
    decreases multiplicity,
{
    if multiplicity > 0 {
        repeat_canonical_record_length(record, (multiplicity - 1) as nat);
    }
}

/// Frozen recursive specification: expand the first sorted canonical record, then the
/// recursively encoded tail.
pub open spec fn legacy_expand(
    records: Seq<u64>,
    multiplicities: Seq<nat>,
) -> Seq<u64>
    recommends records.len() == multiplicities.len(),
    decreases records.len(),
{
    if records.len() == 0 {
        Seq::empty()
    } else {
        repeat_canonical_record(records[0], multiplicities[0])
            + legacy_expand(records.drop_first(), multiplicities.drop_first())
    }
}

/// Cursor/PDA specification: retain the sorted descriptor arrays and advance
/// an explicit record cursor. This is the control structure used by the
/// stack-safe zipper-to-sink implementation.
pub open spec fn pda_expand_suffix(
    records: Seq<u64>,
    multiplicities: Seq<nat>,
    cursor: int,
) -> Seq<u64>
    recommends
        records.len() == multiplicities.len(),
        0 <= cursor <= records.len(),
    decreases records.len() - cursor,
{
    if cursor >= records.len() {
        Seq::empty()
    } else {
        repeat_canonical_record(records[cursor], multiplicities[cursor])
            + pda_expand_suffix(records, multiplicities, cursor + 1)
    }
}

proof fn expansion_suffix_equivalent(
    records: Seq<u64>,
    multiplicities: Seq<nat>,
    cursor: int,
)
    requires
        records.len() == multiplicities.len(),
        0 <= cursor <= records.len(),
    ensures
        pda_expand_suffix(records, multiplicities, cursor)
            == legacy_expand(
                records.subrange(cursor, records.len() as int),
                multiplicities.subrange(cursor, multiplicities.len() as int),
            ),
    decreases records.len() - cursor,
{
    if cursor < records.len() {
        expansion_suffix_equivalent(records, multiplicities, cursor + 1);
        assert(
            records.subrange(cursor, records.len() as int).drop_first()
                == records.subrange(cursor + 1, records.len() as int)
        );
        assert(
            multiplicities.subrange(cursor, multiplicities.len() as int).drop_first()
                == multiplicities.subrange(cursor + 1, multiplicities.len() as int)
        );
    }
}

/// The generated/iterative descriptor machine and the frozen recursive
/// reference produce exactly the same canonical-record sequence, including duplicate
/// expansion from multiplicity.
pub proof fn canonical_egress_expansion_equivalent(
    records: Seq<u64>,
    multiplicities: Seq<nat>,
)
    requires records.len() == multiplicities.len(),
    ensures
        pda_expand_suffix(records, multiplicities, 0)
            == legacy_expand(records, multiplicities),
{
    expansion_suffix_equivalent(records, multiplicities, 0);
    assert(records.subrange(0, records.len() as int) == records);
    assert(multiplicities.subrange(0, multiplicities.len() as int) == multiplicities);
}

/// Sum the logical cardinality independently of the output sequence.
pub open spec fn multiplicity_sum(multiplicities: Seq<nat>) -> nat
    decreases multiplicities.len(),
{
    if multiplicities.len() == 0 {
        0
    } else {
        multiplicities[0] + multiplicity_sum(multiplicities.drop_first())
    }
}

pub proof fn legacy_expansion_cardinality(
    records: Seq<u64>,
    multiplicities: Seq<nat>,
)
    requires records.len() == multiplicities.len(),
    ensures
        legacy_expand(records, multiplicities).len()
            == multiplicity_sum(multiplicities),
    decreases records.len(),
{
    if records.len() > 0 {
        repeat_canonical_record_length(records[0], multiplicities[0]);
        legacy_expansion_cardinality(
            records.drop_first(),
            multiplicities.drop_first(),
        );
    }
}

/// One authoritative canonical-record endpoint in the split continuation/join
/// index. Continuations carry an identity and have multiplicity one; joins
/// carry no continuation identity and compress one or more logical orders.
pub struct CanonicalRecordCellSpec {
    pub group: u64,
    pub canonical_record: u64,
    pub continuation_identity: Option<u64>,
    pub multiplicity: int,
    pub encoded_len: int,
}

/// Exact aggregate metadata published with the group, canonical-record, and
/// continuation-identity roots.
pub struct SplitIndexCountersSpec {
    pub group_count: int,
    pub canonical_record_count: int,
    pub continuation_identity_count: int,
    pub logical_record_count: int,
    pub encoded_len: int,
}

pub open spec fn canonical_record_cell_is_well_formed(
    cell: CanonicalRecordCellSpec,
) -> bool {
    cell.multiplicity > 0
        && cell.encoded_len >= 0
        && (cell.continuation_identity.is_none() || cell.multiplicity == 1)
}

pub open spec fn split_index_counters_are_coherent(
    counters: SplitIndexCountersSpec,
) -> bool {
    0 <= counters.group_count <= counters.canonical_record_count
        && 0 <= counters.continuation_identity_count
        && counters.continuation_identity_count <= counters.canonical_record_count
        && counters.canonical_record_count <= counters.logical_record_count
        && 0 <= counters.encoded_len
}

pub open spec fn first_wins_continuation_identity(
    existing: Option<u64>,
    candidate_canonical_record: u64,
) -> Option<u64> {
    match existing {
        Some(first) => Some(first),
        None => Some(candidate_canonical_record),
    }
}

pub open spec fn last_wins_continuation_identity_mutant(
    existing: Option<u64>,
    candidate_canonical_record: u64,
) -> Option<u64> {
    Some(candidate_canonical_record)
}

/// Repeated continuation identities preserve the first canonical-record
/// binding. This is the secondary-index half of split-index coherence.
pub proof fn continuation_identity_binding_is_first_wins(
    first_canonical_record: u64,
    later_canonical_record: u64,
)
    ensures
        first_wins_continuation_identity(
            Some(first_canonical_record),
            later_canonical_record,
        ) == Some(first_canonical_record),
{}

/// A last-wins implementation is observably different whenever two admitted
/// canonical records share one continuation identity.
pub proof fn last_wins_continuation_identity_mutant_is_rejected()
    ensures
        last_wins_continuation_identity_mutant(Some(11u64), 22u64)
            != first_wins_continuation_identity(Some(11u64), 22u64),
{}

pub open spec fn insert_canonical_record(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
    creates_group: bool,
    creates_identity_binding: bool,
) -> SplitIndexCountersSpec {
    SplitIndexCountersSpec {
        group_count: counters.group_count + if creates_group { 1int } else { 0int },
        canonical_record_count: counters.canonical_record_count + 1,
        continuation_identity_count: counters.continuation_identity_count
            + if creates_identity_binding { 1int } else { 0int },
        logical_record_count: counters.logical_record_count + cell.multiplicity,
        encoded_len: counters.encoded_len + cell.encoded_len,
    }
}

/// Publishing one canonical-record endpoint debits no prior aggregate and
/// credits each exact contribution once. The identity-root count may increase
/// only for a continuation whose first-wins binding did not already exist.
pub proof fn canonical_record_insert_has_exact_counts_and_length(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
    creates_group: bool,
    creates_identity_binding: bool,
)
    requires
        canonical_record_cell_is_well_formed(cell),
        !creates_identity_binding || cell.continuation_identity.is_some(),
    ensures
        insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        ).group_count
            == counters.group_count + if creates_group { 1int } else { 0int },
        insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        ).canonical_record_count == counters.canonical_record_count + 1,
        insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        ).logical_record_count == counters.logical_record_count + cell.multiplicity,
        insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        ).encoded_len == counters.encoded_len + cell.encoded_len,
        insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        ).continuation_identity_count
            == counters.continuation_identity_count
                + if creates_identity_binding { 1int } else { 0int },
{}

pub proof fn canonical_record_insert_preserves_split_index_coherence(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
    creates_group: bool,
    creates_identity_binding: bool,
)
    requires
        split_index_counters_are_coherent(counters),
        canonical_record_cell_is_well_formed(cell),
        !creates_identity_binding || cell.continuation_identity.is_some(),
    ensures
        split_index_counters_are_coherent(insert_canonical_record(
            counters,
            cell,
            creates_group,
            creates_identity_binding,
        )),
{}

/// A multiplicity-compressed join has one canonical endpoint, no continuation
/// identity endpoint, and its full logical and encoded contribution.
pub proof fn join_multiplicity_is_accounted_exactly(
    counters: SplitIndexCountersSpec,
    group: u64,
    canonical_record: u64,
    multiplicity: int,
    encoded_len: int,
)
    requires multiplicity > 0, encoded_len >= 0,
    ensures {
        let join = CanonicalRecordCellSpec {
            group,
            canonical_record,
            continuation_identity: None,
            multiplicity,
            encoded_len,
        };
        let inserted = insert_canonical_record(counters, join, false, false);
        inserted.canonical_record_count == counters.canonical_record_count + 1
            && inserted.continuation_identity_count
                == counters.continuation_identity_count
            && inserted.logical_record_count
                == counters.logical_record_count + multiplicity
            && inserted.encoded_len == counters.encoded_len + encoded_len
    },
{}

pub open spec fn collapsed_join_multiplicity_mutant(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
) -> SplitIndexCountersSpec {
    SplitIndexCountersSpec {
        group_count: counters.group_count,
        canonical_record_count: counters.canonical_record_count + 1,
        continuation_identity_count: counters.continuation_identity_count,
        logical_record_count: counters.logical_record_count + 1,
        encoded_len: counters.encoded_len + cell.encoded_len,
    }
}

pub proof fn collapsed_join_multiplicity_mutant_is_rejected(
    counters: SplitIndexCountersSpec,
    group: u64,
    canonical_record: u64,
    encoded_len: int,
)
    ensures {
        let join = CanonicalRecordCellSpec {
            group,
            canonical_record,
            continuation_identity: None,
            multiplicity: 2,
            encoded_len,
        };
        collapsed_join_multiplicity_mutant(counters, join).logical_record_count
            != insert_canonical_record(counters, join, false, false).logical_record_count
    },
{}

pub open spec fn delete_owned_canonical_record(
    counters: SplitIndexCountersSpec,
    requested_group: u64,
    cell: CanonicalRecordCellSpec,
    removes_group: bool,
    removes_identity_binding: bool,
) -> Option<SplitIndexCountersSpec> {
    if requested_group != cell.group
        || counters.canonical_record_count < 1
        || counters.logical_record_count < cell.multiplicity
        || counters.encoded_len < cell.encoded_len
        || (removes_group && counters.group_count < 1)
        || (removes_identity_binding && counters.continuation_identity_count < 1)
        || (removes_identity_binding && cell.continuation_identity.is_none())
    {
        None
    } else {
        Some(SplitIndexCountersSpec {
            group_count: counters.group_count - if removes_group { 1int } else { 0int },
            canonical_record_count: counters.canonical_record_count - 1,
            continuation_identity_count: counters.continuation_identity_count
                - if removes_identity_binding { 1int } else { 0int },
            logical_record_count: counters.logical_record_count - cell.multiplicity,
            encoded_len: counters.encoded_len - cell.encoded_len,
        })
    }
}

/// A successful owned deletion removes one canonical endpoint and the exact
/// logical-record and byte contributions stored in that endpoint.
pub proof fn owned_deletion_has_exact_counts_and_length(
    counters: SplitIndexCountersSpec,
    requested_group: u64,
    cell: CanonicalRecordCellSpec,
    removes_group: bool,
    removes_identity_binding: bool,
    deleted: SplitIndexCountersSpec,
)
    requires
        canonical_record_cell_is_well_formed(cell),
        delete_owned_canonical_record(
            counters,
            requested_group,
            cell,
            removes_group,
            removes_identity_binding,
        ) == Some(deleted),
    ensures
        requested_group == cell.group,
        deleted.group_count
            == counters.group_count - if removes_group { 1int } else { 0int },
        deleted.canonical_record_count == counters.canonical_record_count - 1,
        deleted.logical_record_count
            == counters.logical_record_count - cell.multiplicity,
        deleted.encoded_len == counters.encoded_len - cell.encoded_len,
        deleted.continuation_identity_count
            == counters.continuation_identity_count
                - if removes_identity_binding { 1int } else { 0int },
{}

pub proof fn owned_deletion_preserves_split_index_coherence(
    counters: SplitIndexCountersSpec,
    requested_group: u64,
    cell: CanonicalRecordCellSpec,
    removes_group: bool,
    removes_identity_binding: bool,
    deleted: SplitIndexCountersSpec,
)
    requires
        split_index_counters_are_coherent(counters),
        canonical_record_cell_is_well_formed(cell),
        delete_owned_canonical_record(
            counters,
            requested_group,
            cell,
            removes_group,
            removes_identity_binding,
        ) == Some(deleted),
        removes_group || counters.group_count < counters.canonical_record_count,
        removes_identity_binding
            || counters.continuation_identity_count < counters.canonical_record_count,
        counters.logical_record_count - cell.multiplicity
            >= counters.canonical_record_count - 1,
    ensures split_index_counters_are_coherent(deleted),
{}

pub open spec fn delete_without_group_ownership_mutant(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
) -> Option<SplitIndexCountersSpec> {
    if counters.canonical_record_count < 1
        || counters.logical_record_count < cell.multiplicity
        || counters.encoded_len < cell.encoded_len
    {
        None
    } else {
        Some(SplitIndexCountersSpec {
            group_count: counters.group_count,
            canonical_record_count: counters.canonical_record_count - 1,
            continuation_identity_count: counters.continuation_identity_count,
            logical_record_count: counters.logical_record_count - cell.multiplicity,
            encoded_len: counters.encoded_len - cell.encoded_len,
        })
    }
}

/// The production deletion predicate rejects a canonical path presented under
/// another group; a transition that omits ownership admits the same witness.
pub proof fn cross_group_deletion_mutant_is_rejected()
    ensures {
        let counters = SplitIndexCountersSpec {
            group_count: 1,
            canonical_record_count: 1,
            continuation_identity_count: 0,
            logical_record_count: 2,
            encoded_len: 24,
        };
        let cell = CanonicalRecordCellSpec {
            group: 7,
            canonical_record: 41,
            continuation_identity: None,
            multiplicity: 2,
            encoded_len: 24,
        };
        delete_owned_canonical_record(counters, 8, cell, false, false).is_none()
            && delete_without_group_ownership_mutant(counters, cell).is_some()
    },
{}

pub open spec fn delete_without_encoded_length_debit_mutant(
    counters: SplitIndexCountersSpec,
    cell: CanonicalRecordCellSpec,
) -> SplitIndexCountersSpec {
    SplitIndexCountersSpec {
        group_count: counters.group_count,
        canonical_record_count: counters.canonical_record_count - 1,
        continuation_identity_count: counters.continuation_identity_count,
        logical_record_count: counters.logical_record_count - cell.multiplicity,
        encoded_len: counters.encoded_len,
    }
}

pub proof fn deletion_length_mutant_is_rejected()
    ensures {
        let counters = SplitIndexCountersSpec {
            group_count: 1,
            canonical_record_count: 1,
            continuation_identity_count: 0,
            logical_record_count: 2,
            encoded_len: 24,
        };
        let cell = CanonicalRecordCellSpec {
            group: 7,
            canonical_record: 41,
            continuation_identity: None,
            multiplicity: 2,
            encoded_len: 24,
        };
        delete_without_encoded_length_debit_mutant(counters, cell).encoded_len != 0
            && delete_owned_canonical_record(counters, 7, cell, false, false)
                == Some(SplitIndexCountersSpec {
                    group_count: 1,
                    canonical_record_count: 0,
                    continuation_identity_count: 0,
                    logical_record_count: 0,
                    encoded_len: 0,
                })
    },
{}

/// Canonical protobuf transcript produced by one cold RPB3 zipper traversal.
/// `before` contains component fields 1..8, `root` fields 9/10, and `after`
/// fields 11/12. Executable differential tests bind these abstract bytes to
/// the generated schema and resident component bodies.
pub open spec fn direct_protobuf_transcript(
    before: Seq<u8>,
    root: Seq<u8>,
    after: Seq<u8>,
) -> Seq<u8> {
    before + root + after
}

/// Transcript emitted from the atomic prepared projection. The retained image
/// is exactly `before ++ after`; `split` restores the root-field insertion
/// point without retaining two allocations.
pub open spec fn prepared_protobuf_transcript(
    projection: Seq<u8>,
    split: int,
    root: Seq<u8>,
) -> Seq<u8>
    recommends 0 <= split <= projection.len(),
{
    projection.subrange(0, split)
        + root
        + projection.subrange(split, projection.len() as int)
}

pub proof fn prepared_projection_equals_direct(
    before: Seq<u8>,
    root: Seq<u8>,
    after: Seq<u8>,
)
    ensures
        prepared_protobuf_transcript(
            before + after,
            before.len() as int,
            root,
        ) == direct_protobuf_transcript(before, root, after),
{
    let projection = before + after;
    assert(projection.len() == before.len() + after.len());
    assert(projection.subrange(0, before.len() as int) =~= before);
    assert(projection.subrange(before.len() as int, projection.len() as int) =~= after);
}

/// Calibration witnesses: either moving the root insertion point by one byte
/// or omitting an after-root family changes the transcript. These witnesses
/// keep the sequence-equivalence theorem from passing against a gate that is
/// insensitive to the two principal projection mutants.
pub proof fn projection_mutants_change_the_transcript()
    ensures
        prepared_protobuf_transcript(seq![1u8, 2u8, 11u8], 1, seq![9u8])
            != direct_protobuf_transcript(seq![1u8, 2u8], seq![9u8], seq![11u8]),
        direct_protobuf_transcript(seq![1u8], seq![9u8], seq![11u8])
            != direct_protobuf_transcript(seq![1u8], seq![9u8], Seq::empty()),
{
    assert(prepared_protobuf_transcript(seq![1u8, 2u8, 11u8], 1, seq![9u8])[1]
        == 9u8);
    assert(direct_protobuf_transcript(seq![1u8, 2u8], seq![9u8], seq![11u8])[1]
        == 2u8);
    assert(direct_protobuf_transcript(seq![1u8], seq![9u8], seq![11u8]).len()
        == 3);
    assert(direct_protobuf_transcript(seq![1u8], seq![9u8], Seq::empty()).len()
        == 2);
}

/// Exact admission predicate for caller-budgeted retained projections.
pub open spec fn projection_fits_spec(projected_bytes: int, limit: int) -> bool {
    0 <= projected_bytes && projected_bytes <= limit
}

pub fn projection_fits(projected_bytes: usize, limit: usize) -> (fits: bool)
    ensures fits == projection_fits_spec(projected_bytes as int, limit as int),
{
    projected_bytes <= limit
}

/// A cold exact-cap request is admitted inclusively. A one-byte-smaller cap is
/// refused, while the empty projection fits an empty budget. Production first
/// checks for an existing shared projection; therefore this predicate governs
/// only allocation of a new cold projection, not reuse of already-retained
/// bytes.
pub proof fn cold_projection_cap_is_exact(projected_bytes: int)
    requires projected_bytes > 0,
    ensures
        projection_fits_spec(projected_bytes, projected_bytes),
        !projection_fits_spec(projected_bytes, projected_bytes - 1),
{}

pub proof fn empty_projection_fits_zero_cap()
    ensures projection_fits_spec(0, 0),
{}

/// Abstract CoW identity/cache state. A projection records the exact
/// authoritative RPB3 identity for which its bytes were prepared. `None` is a
/// valid cold state; `Some(id)` is valid only for the current identity.
pub struct ProjectionIdentityState {
    pub writer_identity: u64,
    pub writer_projection: Option<u64>,
    pub clone_identity: u64,
    pub clone_projection: Option<u64>,
}

pub open spec fn projection_cache_valid(
    identity: u64,
    projection: Option<u64>,
) -> bool {
    projection.is_none() || projection == Some(identity)
}

pub open spec fn projection_state_valid(state: ProjectionIdentityState) -> bool {
    projection_cache_valid(state.writer_identity, state.writer_projection)
        && projection_cache_valid(state.clone_identity, state.clone_projection)
}

pub open spec fn successful_component_mutation_spec(
    state: ProjectionIdentityState,
    new_writer_identity: u64,
) -> ProjectionIdentityState {
    ProjectionIdentityState {
        writer_identity: new_writer_identity,
        writer_projection: None,
        clone_identity: state.clone_identity,
        clone_projection: state.clone_projection,
    }
}

pub proof fn successful_component_mutation_preserves_validity(
    state: ProjectionIdentityState,
    new_writer_identity: u64,
)
    requires
        projection_state_valid(state),
        new_writer_identity != state.writer_identity,
    ensures
        projection_state_valid(successful_component_mutation_spec(
            state,
            new_writer_identity,
        )),
        successful_component_mutation_spec(state, new_writer_identity)
            .writer_projection.is_none(),
        successful_component_mutation_spec(state, new_writer_identity)
            .clone_identity == state.clone_identity,
        successful_component_mutation_spec(state, new_writer_identity)
            .clone_projection == state.clone_projection,
{}

/// Rejected structurally non-mutating operations preserve both authoritative
/// identities and both projection versions. Root metadata changes use the
/// same transition because fields 9/10 are deliberately outside the retained
/// component projection.
pub open spec fn projection_preserving_change_spec(
    state: ProjectionIdentityState,
) -> ProjectionIdentityState {
    ProjectionIdentityState {
        writer_identity: state.writer_identity,
        writer_projection: state.writer_projection,
        clone_identity: state.clone_identity,
        clone_projection: state.clone_projection,
    }
}

pub proof fn rejected_and_metadata_changes_preserve_projection(
    state: ProjectionIdentityState,
)
    requires projection_state_valid(state),
    ensures
        projection_state_valid(projection_preserving_change_spec(state)),
        projection_preserving_change_spec(state).writer_identity
            == state.writer_identity,
        projection_preserving_change_spec(state).writer_projection
            == state.writer_projection,
        projection_preserving_change_spec(state).clone_identity
            == state.clone_identity,
        projection_preserving_change_spec(state).clone_projection
            == state.clone_projection,
{}

/// A stale-cache mutant changes the writer identity but retains bytes prepared
/// for the prior identity. The validity predicate rejects it.
pub proof fn stale_projection_mutant_is_rejected()
    ensures !projection_cache_valid(2u64, Some(1u64)),
{}

/// Atomic publication invariant refined by the production `OnceLock`: a
/// visible value is either absent or the complete projection. Private build
/// prefixes are not supplied to this public state.
pub open spec fn projection_publication_valid(
    visible: Option<Seq<u8>>,
    complete: Seq<u8>,
) -> bool {
    visible.is_none() || visible == Some(complete)
}

/// Publication is legal only after the private image equals the complete
/// projection; publishing it establishes the public invariant.
pub proof fn complete_projection_publication_is_atomic(
    private_image: Seq<u8>,
    complete: Seq<u8>,
)
    requires private_image == complete,
    ensures projection_publication_valid(Some(private_image), complete),
{}

/// Calibration witness for a forbidden partially visible projection.
pub proof fn partial_projection_publication_mutant_is_rejected()
    ensures
        !projection_publication_valid(Some(seq![1u8]), seq![1u8, 2u8]),
{
    assert(seq![1u8].len() == 1);
    assert(seq![1u8, 2u8].len() == 2);
    assert(seq![1u8] != seq![1u8, 2u8]);
    assert(Some(seq![1u8]) != Some(seq![1u8, 2u8]));
}

/// Fallible sinks may contain a private prefix after callback failure, but the
/// owner publishes only a complete successful image. This model separates the
/// private sink from the public transaction boundary.
pub open spec fn public_sink_after_result(
    succeeded: bool,
    old_public: Seq<u8>,
    private_staging: Seq<u8>,
    complete: Seq<u8>,
) -> Seq<u8> {
    if succeeded && private_staging == complete {
        complete
    } else {
        old_public
    }
}

pub proof fn failed_sink_keeps_public_value(
    old_public: Seq<u8>,
    private_prefix: Seq<u8>,
    complete: Seq<u8>,
)
    ensures
        public_sink_after_result(false, old_public, private_prefix, complete)
            == old_public,
{}

pub proof fn complete_sink_publishes_exact_image(
    old_public: Seq<u8>,
    complete: Seq<u8>,
)
    ensures
        public_sink_after_result(true, old_public, complete, complete)
            == complete,
{}

// ---------------------------------------------------------------------------
// Descriptor-certified bounded egress: depth, census, replay, and publication
// ---------------------------------------------------------------------------

/// A generated schema edge is certifiable only when its child's rank is
/// strictly below its parent's. Opaque `Par`/`EPathMap` cut vertices terminate
/// the generated traversal and therefore do not introduce recursive edges.
pub open spec fn certified_rank_edge(parent_rank: nat, child_rank: nat) -> bool {
    child_rank < parent_rank
}

/// The number of statically dispatched message frames on a three-node path is
/// bounded by the root rank plus one. The production maximum really is three:
/// `EPathMap -> Var -> WildcardMsg`; map values reach opaque `Par` at depth two.
pub proof fn certified_rank_path_is_bounded(
    root_rank: nat,
    var_rank: nat,
    wildcard_rank: nat,
)
    requires
        certified_rank_edge(root_rank, var_rank),
        certified_rank_edge(var_rank, wildcard_rank),
        wildcard_rank == 0,
    ensures 3 <= root_rank + 1,
{}

/// A self edge can never receive a strictly descending rank certificate.
pub proof fn self_cycle_has_no_valid_depth_certificate(rank: nat)
    ensures !certified_rank_edge(rank, rank),
{}

/// Nor can both directions of a mutually recursive pair descend.
pub proof fn mutual_cycle_has_no_valid_depth_certificate(left: nat, right: nat)
    ensures
        !(certified_rank_edge(left, right)
          && certified_rank_edge(right, left)),
{}

/// Calibration witness: the concrete EPathMap/Var/WildcardMsg path is valid,
/// has three message frames, and therefore rejects an advertised bound of two.
pub proof fn underreported_root_bound_mutant_is_rejected()
    ensures
        certified_rank_edge(2, 1),
        certified_rank_edge(1, 0),
        !((3 as nat) <= (2 as nat)),
{
    certified_rank_path_is_bounded(2, 1, 0);
}

/// Semantic kind of one length-delimited body in the sealed structural
/// transcript. Keeping the kind independent of the length ensures that
/// equal-sized generated shapes cannot be exchanged silently.
pub enum CertifiedLengthEventKindSpec {
    MessageBody,
    ParBody,
    MapEntryBody,
    EPathMapSnapshotBody,
}

pub struct CertifiedLengthEventSpec {
    pub kind: CertifiedLengthEventKindSpec,
    pub body_len: nat,
    pub completed: bool,
}

pub struct CertifiedLengthSlotSpec {
    pub index: int,
    pub kind: CertifiedLengthEventKindSpec,
}

/// Mathematical counterpart of the sealed `CertifiedLengthTranscript`: the
/// mandatory preflight records every generated message, Par, map-entry, and
/// EPathMap-snapshot body in emission preorder.
pub struct CertifiedLengthTranscriptReplay {
    pub events: Seq<CertifiedLengthEventSpec>,
    pub index: int,
}

pub open spec fn reserve_transcript_slot(
    events: Seq<CertifiedLengthEventSpec>,
    kind: CertifiedLengthEventKindSpec,
) -> (Seq<CertifiedLengthEventSpec>, CertifiedLengthSlotSpec) {
    (
        events.push(CertifiedLengthEventSpec {
            kind,
            body_len: 0,
            completed: false,
        }),
        CertifiedLengthSlotSpec {
            index: events.len() as int,
            kind,
        },
    )
}

pub open spec fn complete_transcript_slot(
    events: Seq<CertifiedLengthEventSpec>,
    slot: CertifiedLengthSlotSpec,
    body_len: nat,
) -> Option<Seq<CertifiedLengthEventSpec>> {
    if 0 <= slot.index < events.len()
        && events[slot.index].kind == slot.kind
        && !events[slot.index].completed
    {
        Some(events.update(
            slot.index,
            CertifiedLengthEventSpec {
                kind: slot.kind,
                body_len,
                completed: true,
            },
        ))
    } else {
        None
    }
}

pub open spec fn transcript_is_sealable(events: Seq<CertifiedLengthEventSpec>) -> bool {
    forall|index: int| 0 <= index < events.len() ==> events[index].completed
}

pub open spec fn certified_transcript_replay_valid(
    replay: CertifiedLengthTranscriptReplay,
) -> bool {
    0 <= replay.index <= replay.events.len()
        && transcript_is_sealable(replay.events)
}

pub open spec fn replay_next_typed_event(
    replay: CertifiedLengthTranscriptReplay,
    expected: CertifiedLengthEventKindSpec,
) -> Option<(CertifiedLengthEventSpec, CertifiedLengthTranscriptReplay)> {
    if certified_transcript_replay_valid(replay)
        && replay.index < replay.events.len()
        && replay.events[replay.index].kind == expected
    {
        Some((
            replay.events[replay.index],
            CertifiedLengthTranscriptReplay {
                events: replay.events,
                index: replay.index + 1,
            },
        ))
    } else {
        None
    }
}

pub open spec fn typed_replay_is_finished(replay: CertifiedLengthTranscriptReplay) -> bool {
    certified_transcript_replay_valid(replay)
        && replay.index == replay.events.len()
}

/// Reserving a parent before descending appends one incomplete event and
/// returns its stable old-length index without changing any earlier slot.
pub proof fn reservation_appends_one_stable_slot(
    events: Seq<CertifiedLengthEventSpec>,
    kind: CertifiedLengthEventKindSpec,
)
    ensures
        reserve_transcript_slot(events, kind).0.len() == events.len() + 1,
        reserve_transcript_slot(events, kind).1.index == events.len(),
        reserve_transcript_slot(events, kind).1.kind == kind,
        reserve_transcript_slot(events, kind).0.subrange(0, events.len() as int) == events,
{}

/// Completing a well-typed slot changes only that slot. The returned event is
/// immutable and complete; all earlier and later events remain equal.
pub proof fn completion_updates_only_the_reserved_slot(
    events: Seq<CertifiedLengthEventSpec>,
    slot: CertifiedLengthSlotSpec,
    body_len: nat,
)
    requires
        0 <= slot.index < events.len(),
        events[slot.index].kind == slot.kind,
        !events[slot.index].completed,
    ensures
        complete_transcript_slot(events, slot, body_len).is_some(),
        complete_transcript_slot(events, slot, body_len).unwrap()[slot.index].kind
            == slot.kind,
        complete_transcript_slot(events, slot, body_len).unwrap()[slot.index].body_len
            == body_len,
        complete_transcript_slot(events, slot, body_len).unwrap()[slot.index].completed,
        forall|index: int| 0 <= index < events.len() && index != slot.index
            ==> complete_transcript_slot(events, slot, body_len).unwrap()[index]
                == events[index],
{}

pub proof fn duplicate_completion_mutant_is_rejected(
    events: Seq<CertifiedLengthEventSpec>,
    slot: CertifiedLengthSlotSpec,
    body_len: nat,
)
    requires
        0 <= slot.index < events.len(),
        events[slot.index].kind == slot.kind,
        !events[slot.index].completed,
    ensures
        complete_transcript_slot(
            complete_transcript_slot(events, slot, body_len).unwrap(),
            slot,
            body_len,
        ).is_none(),
{}

pub proof fn wrong_kind_completion_mutant_is_rejected(
    events: Seq<CertifiedLengthEventSpec>,
    slot: CertifiedLengthSlotSpec,
    wrong_kind: CertifiedLengthEventKindSpec,
    body_len: nat,
)
    requires
        0 <= slot.index < events.len(),
        events[slot.index].kind == slot.kind,
        slot.kind != wrong_kind,
    ensures
        complete_transcript_slot(
            events,
            CertifiedLengthSlotSpec { index: slot.index, kind: wrong_kind },
            body_len,
        ).is_none(),
{}

/// Inline-to-spill transition copies the logical prefix exactly. Appending any
/// descendant suffix therefore preserves every previously returned slot index.
pub open spec fn spill_materialization(
    inline: Seq<CertifiedLengthEventSpec>,
    descendants: Seq<CertifiedLengthEventSpec>,
) -> Seq<CertifiedLengthEventSpec> {
    inline + descendants
}

pub proof fn inline_to_spill_preserves_parent_slot(
    inline: Seq<CertifiedLengthEventSpec>,
    descendants: Seq<CertifiedLengthEventSpec>,
    parent: CertifiedLengthSlotSpec,
)
    requires
        0 <= parent.index < inline.len(),
        inline[parent.index].kind == parent.kind,
    ensures
        spill_materialization(inline, descendants)[parent.index]
            == inline[parent.index],
{}

/// Typed replay returns the exact current event and advances once.
pub proof fn replay_next_consumes_exact_recorded_event(
    replay: CertifiedLengthTranscriptReplay,
)
    requires
        certified_transcript_replay_valid(replay),
        replay.index < replay.events.len(),
    ensures
        replay_next_typed_event(replay, replay.events[replay.index].kind).is_some(),
        replay_next_typed_event(replay, replay.events[replay.index].kind)
            .unwrap().0 == replay.events[replay.index],
        replay_next_typed_event(replay, replay.events[replay.index].kind)
            .unwrap().1.index == replay.index + 1,
{}

/// Successful finish is equivalent to exact structural exhaustion—neither an
/// overrun nor an unconsumed message/map/snapshot/Par body may be committed.
pub proof fn replay_finish_iff_exactly_exhausted(
    replay: CertifiedLengthTranscriptReplay,
)
    requires certified_transcript_replay_valid(replay),
    ensures typed_replay_is_finished(replay) <==> replay.index == replay.events.len(),
{}

pub open spec fn equal_length_typed_pair() -> Seq<CertifiedLengthEventSpec> {
    seq![
        CertifiedLengthEventSpec {
            kind: CertifiedLengthEventKindSpec::MessageBody,
            body_len: 7,
            completed: true,
        },
        CertifiedLengthEventSpec {
            kind: CertifiedLengthEventKindSpec::ParBody,
            body_len: 7,
            completed: true,
        },
    ]
}

pub open spec fn dropped_typed_event_mutant() -> Seq<CertifiedLengthEventSpec> {
    seq![equal_length_typed_pair()[0]]
}

pub open spec fn duplicated_typed_event_mutant() -> Seq<CertifiedLengthEventSpec> {
    equal_length_typed_pair().push(equal_length_typed_pair()[1])
}

pub open spec fn equal_length_kind_swap_mutant() -> Seq<CertifiedLengthEventSpec> {
    seq![equal_length_typed_pair()[1], equal_length_typed_pair()[0]]
}

pub proof fn dropped_replay_event_mutant_is_rejected()
    ensures dropped_typed_event_mutant() != equal_length_typed_pair(),
{
    assert(dropped_typed_event_mutant().len() == 1);
    assert(equal_length_typed_pair().len() == 2);
}

pub proof fn duplicated_replay_event_mutant_is_rejected()
    ensures duplicated_typed_event_mutant() != equal_length_typed_pair(),
{
    assert(duplicated_typed_event_mutant().len() == 3);
    assert(equal_length_typed_pair().len() == 2);
}

pub proof fn equal_length_kind_swap_mutant_is_rejected()
    ensures
        equal_length_kind_swap_mutant() != equal_length_typed_pair(),
        equal_length_kind_swap_mutant()[0].body_len
            == equal_length_typed_pair()[0].body_len,
{
    assert(equal_length_kind_swap_mutant()[0].kind
        == CertifiedLengthEventKindSpec::ParBody);
    assert(equal_length_typed_pair()[0].kind
        == CertifiedLengthEventKindSpec::MessageBody);
}

/// Policy facts computed by the exact owner census. Component-empty Pars have
/// no reusable owner. A non-empty owner is memo-eligible iff it occurs twice
/// or more in this response.
pub struct CertifiedOwnerCensusFacts {
    pub component_empty: bool,
    pub occurrences: nat,
}

pub open spec fn census_policy_uses_direct(facts: CertifiedOwnerCensusFacts) -> bool {
    facts.component_empty || facts.occurrences <= 1
}

pub open spec fn census_policy_uses_memo(facts: CertifiedOwnerCensusFacts) -> bool {
    !facts.component_empty && facts.occurrences > 1
}

pub proof fn census_policy_directs_empty_and_singleton_owners(
    facts: CertifiedOwnerCensusFacts,
)
    requires facts.component_empty || facts.occurrences == 1,
    ensures
        census_policy_uses_direct(facts),
        !census_policy_uses_memo(facts),
{}

pub proof fn census_policy_memoizes_only_repeated_owners(
    facts: CertifiedOwnerCensusFacts,
)
    requires census_policy_uses_memo(facts),
    ensures !facts.component_empty && facts.occurrences > 1,
{}

pub proof fn empty_owner_memo_mutant_is_rejected()
    ensures
        !census_policy_uses_memo(CertifiedOwnerCensusFacts {
            component_empty: true,
            occurrences: 4,
        }),
{}

pub proof fn singleton_owner_memo_mutant_is_rejected()
    ensures
        !census_policy_uses_memo(CertifiedOwnerCensusFacts {
            component_empty: false,
            occurrences: 1,
        }),
{}

/// A certified frame is publishable only when both independent preflight
/// obligations are exact: the private byte count and the structural transcript
/// replay cursor.
pub open spec fn certified_frame_is_complete(
    written: int,
    expected: int,
    replay_index: int,
    replay_len: int,
) -> bool {
    0 <= written
        && written == expected
        && 0 <= replay_index
        && replay_index == replay_len
}

pub open spec fn certified_frame_public_after_result(
    succeeded: bool,
    old_public: Seq<u8>,
    private_frame: Seq<u8>,
    expected_len: int,
    replay_index: int,
    replay_len: int,
) -> Seq<u8> {
    if succeeded
        && certified_frame_is_complete(
            private_frame.len() as int,
            expected_len,
            replay_index,
            replay_len,
        )
    {
        private_frame
    } else {
        old_public
    }
}

/// Any generation, Par emission, length, or replay error discards the complete
/// private attempt and preserves the previously public frame.
pub proof fn certified_frame_error_discards_private_and_preserves_public(
    old_public: Seq<u8>,
    private_prefix: Seq<u8>,
    expected_len: int,
    replay_index: int,
    replay_len: int,
)
    ensures
        certified_frame_public_after_result(
            false,
            old_public,
            private_prefix,
            expected_len,
            replay_index,
            replay_len,
        ) == old_public,
{}

pub proof fn certified_frame_commit_requires_exact_length_and_replay(
    old_public: Seq<u8>,
    complete: Seq<u8>,
    replay_len: int,
)
    requires 0 <= replay_len,
    ensures
        certified_frame_public_after_result(
            true,
            old_public,
            complete,
            complete.len() as int,
            replay_len,
            replay_len,
        ) == complete,
{}

pub proof fn premature_replay_commit_mutant_is_rejected()
    ensures
        !certified_frame_is_complete(2, 2, 1, 2),
        certified_frame_public_after_result(
            true,
            seq![9u8],
            seq![1u8, 2u8],
            2,
            1,
            2,
        ) == seq![9u8],
{}

/// Semantic component image attached to one opaque response-local CoW owner.
/// Rust differentials bind these sequences to Par fields 1..8 and 11/12.
pub struct InFrameComponentImage {
    pub owner: u64,
    pub before: Seq<u8>,
    pub after: Seq<u8>,
}

/// Component-only ranges recorded in an append-only response frame. Root
/// fields 9/10 occupy the gap and are deliberately absent from both ranges.
pub struct InFrameComponentRanges {
    pub owner: u64,
    pub before_start: int,
    pub before_end: int,
    pub after_start: int,
    pub after_end: int,
}

pub open spec fn in_frame_component_ranges_admitted(
    source: Seq<u8>,
    current: InFrameComponentImage,
    ranges: InFrameComponentRanges,
) -> bool {
    0 <= ranges.before_start <= ranges.before_end <= ranges.after_start
        && ranges.after_start <= ranges.after_end <= source.len()
        && current.owner == ranges.owner
        && source.subrange(ranges.before_start, ranges.before_end) == current.before
        && source.subrange(ranges.after_start, ranges.after_end) == current.after
}

pub open spec fn direct_in_frame_par_append(
    source: Seq<u8>,
    current: InFrameComponentImage,
    current_root: Seq<u8>,
) -> Seq<u8> {
    source + current.before + current_root + current.after
}

/// `None` rejects before mutation. An admitted result preserves `source`, copies
/// only its component ranges, and inserts the current root between them.
pub open spec fn try_in_frame_component_range_reuse(
    source: Seq<u8>,
    current: InFrameComponentImage,
    current_root: Seq<u8>,
    ranges: InFrameComponentRanges,
) -> Option<Seq<u8>> {
    if in_frame_component_ranges_admitted(source, current, ranges) {
        Some(
            source
                + source.subrange(ranges.before_start, ranges.before_end)
                + current_root
                + source.subrange(ranges.after_start, ranges.after_end),
        )
    } else {
        None
    }
}

/// Admitted reuse is exactly `before || current-root || after`, and the complete
/// source frame remains an unchanged prefix of the result.
pub proof fn admitted_in_frame_reuse_equals_direct_and_preserves_prefix(
    source: Seq<u8>,
    current: InFrameComponentImage,
    current_root: Seq<u8>,
    ranges: InFrameComponentRanges,
)
    requires in_frame_component_ranges_admitted(source, current, ranges),
    ensures
        try_in_frame_component_range_reuse(source, current, current_root, ranges)
            == Some(direct_in_frame_par_append(source, current, current_root)),
        direct_in_frame_par_append(source, current, current_root)
            .subrange(0, source.len() as int) == source,
{
    let result = direct_in_frame_par_append(source, current, current_root);
    assert(result.subrange(0, source.len() as int) =~= source);
}

spec fn in_frame_witness_source() -> Seq<u8> {
    seq![0u8, 1u8, 9u8, 2u8, 3u8]
}

spec fn in_frame_witness_image(owner: u64) -> InFrameComponentImage {
    InFrameComponentImage { owner, before: seq![1u8], after: seq![2u8, 3u8] }
}

spec fn in_frame_witness_ranges(owner: u64) -> InFrameComponentRanges {
    InFrameComponentRanges {
        owner,
        before_start: 1,
        before_end: 2,
        after_start: 3,
        after_end: 5,
    }
}

spec fn swapped_in_frame_witness_ranges(owner: u64) -> InFrameComponentRanges {
    InFrameComponentRanges {
        owner,
        before_start: 3,
        before_end: 5,
        after_start: 1,
        after_end: 2,
    }
}

spec fn short_after_in_frame_witness_ranges(owner: u64) -> InFrameComponentRanges {
    InFrameComponentRanges {
        owner,
        before_start: 1,
        before_end: 2,
        after_start: 3,
        after_end: 4,
    }
}

spec fn omitted_after_in_frame_witness_ranges(owner: u64) -> InFrameComponentRanges {
    InFrameComponentRanges {
        owner,
        before_start: 1,
        before_end: 2,
        after_start: 5,
        after_end: 5,
    }
}

spec fn whole_prior_par_in_frame_witness_ranges(owner: u64) -> InFrameComponentRanges {
    InFrameComponentRanges {
        owner,
        before_start: 1,
        before_end: 5,
        after_start: 5,
        after_end: 5,
    }
}

spec fn swapped_in_frame_component_ranges_mutant(
    source: Seq<u8>,
    root: Seq<u8>,
    ranges: InFrameComponentRanges,
) -> Seq<u8> {
    source + source.subrange(ranges.after_start, ranges.after_end)
        + root + source.subrange(ranges.before_start, ranges.before_end)
}

spec fn whole_prior_par_stale_root_mutant(
    source: Seq<u8>,
    ranges: InFrameComponentRanges,
) -> Seq<u8> {
    source + source.subrange(ranges.before_start, ranges.after_end)
}

spec fn omitted_after_range_mutant(
    source: Seq<u8>,
    root: Seq<u8>,
    ranges: InFrameComponentRanges,
) -> Seq<u8> {
    source + source.subrange(ranges.before_start, ranges.before_end) + root
}

/// A satisfiable correct memo and five concrete mutants calibrate every
/// admission/equivalence clause: swapped coordinates/order, whole-prior-Par
/// stale-root reuse, wrong owner, omitted after bytes, and an off-by-one end.
proof fn in_frame_component_range_mutants_are_rejected()
    ensures
        in_frame_component_ranges_admitted(
            in_frame_witness_source(),
            in_frame_witness_image(7),
            in_frame_witness_ranges(7),
        ),
        !in_frame_component_ranges_admitted(
            in_frame_witness_source(),
            in_frame_witness_image(7),
            swapped_in_frame_witness_ranges(7),
        ),
        try_in_frame_component_range_reuse(
            in_frame_witness_source(),
            in_frame_witness_image(8),
            seq![8u8],
            in_frame_witness_ranges(7),
        ).is_none(),
        try_in_frame_component_range_reuse(
            in_frame_witness_source(),
            in_frame_witness_image(7),
            seq![8u8],
            short_after_in_frame_witness_ranges(7),
        ).is_none(),
        try_in_frame_component_range_reuse(
            in_frame_witness_source(),
            in_frame_witness_image(7),
            seq![8u8],
            omitted_after_in_frame_witness_ranges(7),
        ).is_none(),
        try_in_frame_component_range_reuse(
            in_frame_witness_source(),
            in_frame_witness_image(7),
            seq![8u8],
            whole_prior_par_in_frame_witness_ranges(7),
        ).is_none(),
        swapped_in_frame_component_ranges_mutant(
            in_frame_witness_source(), seq![8u8], in_frame_witness_ranges(7),
        ) != direct_in_frame_par_append(
            in_frame_witness_source(), in_frame_witness_image(7), seq![8u8],
        ),
        whole_prior_par_stale_root_mutant(
            in_frame_witness_source(), in_frame_witness_ranges(7),
        ) != direct_in_frame_par_append(
            in_frame_witness_source(), in_frame_witness_image(7), seq![8u8],
        ),
        omitted_after_range_mutant(
            in_frame_witness_source(), seq![8u8], in_frame_witness_ranges(7),
        ) != direct_in_frame_par_append(
            in_frame_witness_source(), in_frame_witness_image(7), seq![8u8],
        ),
{
    let source = in_frame_witness_source();
    let ranges = in_frame_witness_ranges(7);
    let direct = direct_in_frame_par_append(source, in_frame_witness_image(7), seq![8u8]);
    assert(source.subrange(1, 2) =~= seq![1u8]);
    assert(source.subrange(3, 5) =~= seq![2u8, 3u8]);
    assert(source.subrange(3, 4).len() == 1);
    assert(in_frame_witness_image(7).after.len() == 2);
    assert(swapped_in_frame_component_ranges_mutant(source, seq![8u8], ranges)[5] == 2u8);
    assert(direct[5] == 1u8);
    assert(whole_prior_par_stale_root_mutant(source, ranges)[6] == 9u8);
    assert(direct[6] == 8u8);
    assert(omitted_after_range_mutant(source, seq![8u8], ranges).len() == 7);
    assert(direct.len() == 9);
}

}

fn main() {}
