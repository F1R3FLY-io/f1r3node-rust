#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verus_spec(result =>
    ensures
        match result {
            Some((next_forward, next_reverse)) =>
                amount <= forward && reverse as int + amount as int <= u64::MAX as int
                && next_forward as int + amount as int == forward as int
                && next_reverse as int == reverse as int + amount as int
                && next_forward as int + next_reverse as int == forward as int + reverse as int,
            None => amount > forward || reverse as int + amount as int > u64::MAX as int,
        },
))]
pub(super) fn checked_residual_transfer(
    forward: u64,
    reverse: u64,
    amount: u64,
) -> Option<(u64, u64)> {
    Some((forward.checked_sub(amount)?, reverse.checked_add(amount)?))
}
