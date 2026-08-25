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

/// Mathematical acceptance boundary for one preflighted canonical identity.
/// A successful step adds `multiplicity` logical datums and that many framed
/// copies of the identity to the final output length. Mathematical integers
/// prevent modular arithmetic from hiding either overflow.
pub open spec fn preflight_step_spec(
    datum_count: int,
    encoded_len: int,
    identity_len: int,
    multiplicity: int,
) -> Option<(int, int)> {
    if datum_count < 0
        || encoded_len < 0
        || identity_len < 0
        || multiplicity < 0
        || datum_count + multiplicity > u64::MAX as int
        || identity_len > usize::MAX as int - BINCODE_U64_BYTES as int
        || encoded_len
            + multiplicity * (BINCODE_U64_BYTES as int + identity_len)
            > usize::MAX as int
    {
        None
    } else {
        Some((
            datum_count + multiplicity,
            encoded_len
                + multiplicity * (BINCODE_U64_BYTES as int + identity_len),
        ))
    }
}

/// Executable arithmetic kernel corresponding to the production
/// `encode_canonical_persisted_data_at_prefix` payload preflight. The implementation performs
/// no write: publication receives an exact cardinality and capacity only after
/// every record has passed this boundary.
pub fn checked_preflight_step(
    datum_count: u64,
    encoded_len: usize,
    identity_len: usize,
    multiplicity: u64,
) -> (result: Option<(u64, usize)>)
    requires 0 < multiplicity,
    ensures
        match result {
            Some((next_count, next_len)) => preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                identity_len as int,
                multiplicity as int,
            ) == Some((next_count as int, next_len as int)),
            None => preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                identity_len as int,
                multiplicity as int,
            ).is_none(),
        },
{
    if multiplicity > u64::MAX - datum_count {
        assert(datum_count as int + multiplicity as int > u64::MAX as int);
        assert(preflight_step_spec(
            datum_count as int,
            encoded_len as int,
            identity_len as int,
            multiplicity as int,
        ).is_none());
        None
    } else if identity_len > usize::MAX - BINCODE_U64_BYTES {
        assert(identity_len as int
            > usize::MAX as int - BINCODE_U64_BYTES as int);
        assert(preflight_step_spec(
            datum_count as int,
            encoded_len as int,
            identity_len as int,
            multiplicity as int,
        ).is_none());
        None
    } else {
        let next_count = datum_count + multiplicity;
        assert(next_count as int == datum_count as int + multiplicity as int);
        assert(datum_count as int + multiplicity as int <= u64::MAX as int);
        assert(identity_len as int
            <= usize::MAX as int - BINCODE_U64_BYTES as int);
        let framed_len = identity_len + BINCODE_U64_BYTES;
        assert(framed_len as int
            == identity_len as int + BINCODE_U64_BYTES as int);
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
                == (BINCODE_U64_BYTES as int + identity_len as int)
                    * multiplicity as int);
            assert(repeated_len as int > remaining as int);
            assert(encoded_len as int + repeated_len as int > usize::MAX as int);
            assert(encoded_len as int
                + multiplicity as int
                    * (BINCODE_U64_BYTES as int + identity_len as int)
                > usize::MAX as int);
            assert(preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                identity_len as int,
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
                == (BINCODE_U64_BYTES as int + identity_len as int)
                    * multiplicity as int);
            assert(final_len as int
                == encoded_len as int
                    + multiplicity as int
                        * (BINCODE_U64_BYTES as int + identity_len as int));
            assert(preflight_step_spec(
                datum_count as int,
                encoded_len as int,
                identity_len as int,
                multiplicity as int,
            ) == Some((next_count as int, final_len as int)));
            Some((next_count, final_len))
        }
    }
}

/// Repeat one canonical identity exactly as the legacy `Vec<Vec<u8>>`
/// semantics require. `identity` is an injective formal name for the complete
/// resident byte string; Rust differentials establish that equal names mean
/// byte-identical admitted identities.
pub open spec fn repeat_identity(identity: u64, multiplicity: nat) -> Seq<u64>
    decreases multiplicity,
{
    if multiplicity == 0 {
        Seq::empty()
    } else {
        seq![identity] + repeat_identity(identity, (multiplicity - 1) as nat)
    }
}

pub proof fn repeat_identity_length(identity: u64, multiplicity: nat)
    ensures repeat_identity(identity, multiplicity).len() == multiplicity,
    decreases multiplicity,
{
    if multiplicity > 0 {
        repeat_identity_length(identity, (multiplicity - 1) as nat);
    }
}

/// Frozen recursive specification: expand the first sorted identity, then the
/// recursively encoded tail.
pub open spec fn legacy_expand(
    identities: Seq<u64>,
    multiplicities: Seq<nat>,
) -> Seq<u64>
    recommends identities.len() == multiplicities.len(),
    decreases identities.len(),
{
    if identities.len() == 0 {
        Seq::empty()
    } else {
        repeat_identity(identities[0], multiplicities[0])
            + legacy_expand(identities.drop_first(), multiplicities.drop_first())
    }
}

/// Cursor/PDA specification: retain the sorted descriptor arrays and advance
/// an explicit record cursor. This is the control structure used by the
/// stack-safe zipper-to-sink implementation.
pub open spec fn pda_expand_suffix(
    identities: Seq<u64>,
    multiplicities: Seq<nat>,
    cursor: int,
) -> Seq<u64>
    recommends
        identities.len() == multiplicities.len(),
        0 <= cursor <= identities.len(),
    decreases identities.len() - cursor,
{
    if cursor >= identities.len() {
        Seq::empty()
    } else {
        repeat_identity(identities[cursor], multiplicities[cursor])
            + pda_expand_suffix(identities, multiplicities, cursor + 1)
    }
}

proof fn expansion_suffix_equivalent(
    identities: Seq<u64>,
    multiplicities: Seq<nat>,
    cursor: int,
)
    requires
        identities.len() == multiplicities.len(),
        0 <= cursor <= identities.len(),
    ensures
        pda_expand_suffix(identities, multiplicities, cursor)
            == legacy_expand(
                identities.subrange(cursor, identities.len() as int),
                multiplicities.subrange(cursor, multiplicities.len() as int),
            ),
    decreases identities.len() - cursor,
{
    if cursor < identities.len() {
        expansion_suffix_equivalent(identities, multiplicities, cursor + 1);
        assert(
            identities.subrange(cursor, identities.len() as int).drop_first()
                == identities.subrange(cursor + 1, identities.len() as int)
        );
        assert(
            multiplicities.subrange(cursor, multiplicities.len() as int).drop_first()
                == multiplicities.subrange(cursor + 1, multiplicities.len() as int)
        );
    }
}

/// The generated/iterative descriptor machine and the frozen recursive
/// reference produce exactly the same identity sequence, including duplicate
/// expansion from multiplicity.
pub proof fn canonical_egress_expansion_equivalent(
    identities: Seq<u64>,
    multiplicities: Seq<nat>,
)
    requires identities.len() == multiplicities.len(),
    ensures
        pda_expand_suffix(identities, multiplicities, 0)
            == legacy_expand(identities, multiplicities),
{
    expansion_suffix_equivalent(identities, multiplicities, 0);
    assert(identities.subrange(0, identities.len() as int) == identities);
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
    identities: Seq<u64>,
    multiplicities: Seq<nat>,
)
    requires identities.len() == multiplicities.len(),
    ensures
        legacy_expand(identities, multiplicities).len()
            == multiplicity_sum(multiplicities),
    decreases identities.len(),
{
    if identities.len() > 0 {
        repeat_identity_length(identities[0], multiplicities[0]);
        legacy_expansion_cardinality(
            identities.drop_first(),
            multiplicities.drop_first(),
        );
    }
}

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

/// Extending a private build prefix preserves public invisibility.
pub proof fn private_projection_build_is_invisible(
    old_private_prefix: Seq<u8>,
    next_private_prefix: Seq<u8>,
    complete: Seq<u8>,
)
    ensures
        projection_publication_valid(None, complete),
        None::<Seq<u8>> == None,
{}

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

}

fn main() {}
