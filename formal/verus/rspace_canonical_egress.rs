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

}

fn main() {}
