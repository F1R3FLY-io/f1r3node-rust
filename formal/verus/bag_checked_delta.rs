use vstd::prelude::*;

verus! {

/// Mathematical result of one exact-key Bag delta cell. Inputs are integers so
/// the specification states the `u64` boundary without modular arithmetic.
pub open spec fn apply_count_spec(
    subject: int,
    additions: int,
    removals: int,
) -> Option<int> {
    if removals < 0 || additions < 0 || removals > subject {
        None
    } else if subject - removals + additions > u64::MAX as int {
        None
    } else {
        Some(subject - removals + additions)
    }
}

/// Executable counterpart of `BagDelta::apply_parts` for one exact key. The
/// complete Bag validates every cell before replacing the PathMap; this scalar
/// function proves the underflow/overflow acceptance boundary of that pass.
pub fn checked_apply_count(
    subject: u64,
    additions: u64,
    removals: u64,
) -> (result: Option<u64>)
    ensures
        match result {
            Some(value) => apply_count_spec(
                subject as int,
                additions as int,
                removals as int,
            ) == Some(value as int),
            None => apply_count_spec(
                subject as int,
                additions as int,
                removals as int,
            ).is_none(),
        },
{
    if removals > subject {
        None
    } else {
        let reduced = subject - removals;
        if additions > u64::MAX - reduced {
            None
        } else {
            Some(reduced + additions)
        }
    }
}

/// Normalization makes addition and removal support disjoint. Any successful
/// checked transition is therefore reversed by swapping the two lanes.
pub proof fn normalized_checked_delta_reversible(
    subject: int,
    additions: int,
    removals: int,
    changed: int,
)
    requires
        0 <= subject <= u64::MAX as int,
        0 <= additions <= u64::MAX as int,
        0 <= removals <= u64::MAX as int,
        additions == 0 || removals == 0,
        apply_count_spec(subject, additions, removals) == Some(changed),
    ensures
        apply_count_spec(changed, removals, additions) == Some(subject),
{
}

}

fn main() {}
