use vstd::prelude::*;

verus! {

/// ACTree03 carries occurrence markers, not indexes into the protobuf body
/// stream.  A map endpoint therefore has the same canonical scalar regardless
/// of value identity, equality, sharing, or allocation address.
pub open spec fn positional_act_sentinel(
    _logical_position: usize,
    _storage_address: usize,
) -> u64 {
    0
}

pub proof fn sentinel_is_storage_identity_independent(
    logical_position: usize,
    left_address: usize,
    right_address: usize,
)
    ensures
        positional_act_sentinel(logical_position, left_address)
            == positional_act_sentinel(logical_position, right_address),
{
}

/// Checked cursor advance used by the pausable endpoint/body lockstep.  A
/// successful step consumes exactly one logical occurrence and cannot wrap.
pub fn checked_next_position(position: usize, count: usize)
    -> (next: Option<usize>)
    ensures
        next.is_some() == (position < count),
        match next {
            Some(next_position) => {
                &&& next_position as int == position as int + 1
                &&& position < next_position
                &&& next_position <= count
            },
            None => position >= count,
        },
{
    if position < count {
        Some(position + 1)
    } else {
        None
    }
}

pub struct PositionalPair {
    pub endpoint_position: usize,
    pub body_position: usize,
    pub next_position: usize,
}

/// Bind the body yielded by the generated child PDA to the endpoint retained
/// by the owned zipper.  No key materialization or address-derived ordinal is
/// involved in the transition.
pub fn pair_one_occurrence(position: usize, endpoint_count: usize, body_count: usize)
    -> (pair: Option<PositionalPair>)
    ensures
        pair.is_some() == (position < endpoint_count && position < body_count),
        match pair {
            Some(binding) => {
                &&& binding.endpoint_position == position
                &&& binding.body_position == position
                &&& binding.next_position as int == position as int + 1
                &&& binding.next_position <= endpoint_count
                &&& binding.next_position <= body_count
            },
            None => position >= endpoint_count || position >= body_count,
        },
{
    if position < endpoint_count && position < body_count {
        match checked_next_position(position, endpoint_count) {
            Some(next_position) => {
                Some(PositionalPair {
                    endpoint_position: position,
                    body_position: position,
                    next_position,
                })
            },
            None => {
                assert(false);
                None
            },
        }
    } else {
        None
    }
}

/// The EPM count is physically a u64.  Conversion to a platform cursor is
/// accepted only when it is representable; there is no narrowing wrap on a
/// 32-bit target.
pub fn checked_platform_count(encoded: u64) -> (count: Option<usize>)
    ensures
        count.is_some() == (encoded as int <= usize::MAX as int),
        match count {
            Some(value) => value as int == encoded as int,
            None => encoded as int > usize::MAX as int,
        },
{
    if encoded > usize::MAX as u64 {
        None
    } else {
        Some(encoded as usize)
    }
}

/// Publication is authorized only after exact endpoint/body exhaustion and
/// after both child and deferred structural checks have succeeded.
pub fn publication_authorized(
    endpoint_position: usize,
    endpoint_count: usize,
    body_position: usize,
    body_count: usize,
    child_ok: bool,
    deferred_structure_ok: bool,
    bodies_canonical: bool,
) -> (authorized: bool)
    ensures
        authorized == (
            endpoint_position == endpoint_count
                && body_position == body_count
                && endpoint_count == body_count
                && child_ok
                && deferred_structure_ok
                && bodies_canonical
        ),
{
    endpoint_position == endpoint_count
        && body_position == body_count
        && endpoint_count == body_count
        && child_ok
        && deferred_structure_ok
        && bodies_canonical
}

}

fn main() {}
