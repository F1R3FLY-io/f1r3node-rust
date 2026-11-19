#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("deploy lifespan must be non-negative: {0}")]
pub struct NegativeDeployLifespan(pub i64);

pub fn earliest_valid_after(
    reference_block_number: i64,
    deploy_lifespan: i64,
) -> Result<i64, NegativeDeployLifespan> {
    if deploy_lifespan < 0 {
        return Err(NegativeDeployLifespan(deploy_lifespan));
    }
    Ok(reference_block_number.saturating_sub(deploy_lifespan))
}

pub fn is_open(
    valid_after_block_number: i64,
    reference_block_number: i64,
    deploy_lifespan: i64,
) -> Result<bool, NegativeDeployLifespan> {
    Ok(valid_after_block_number > earliest_valid_after(reference_block_number, deploy_lifespan)?)
}

/// The highest block number whose block can still include the deploy: the
/// last `reference_block_number` for which [`is_open`] holds.
pub fn last_includable_block(
    valid_after_block_number: i64,
    deploy_lifespan: i64,
) -> Result<i64, NegativeDeployLifespan> {
    if deploy_lifespan < 0 {
        return Err(NegativeDeployLifespan(deploy_lifespan));
    }
    Ok(valid_after_block_number
        .saturating_add(deploy_lifespan)
        .saturating_sub(1))
}

/// The highest `validAfterBlockNumber` a deploy may name when the next block
/// is `next_block_number`. An honest node's tip runs ahead of another honest
/// node's by less than the citable depth, so a client that read the number
/// from any node of the shard names no block past this one.
pub fn latest_valid_after(next_block_number: i64, max_parent_depth: i32) -> i64 {
    next_block_number.saturating_add(i64::from(max_parent_depth))
}

pub fn is_too_far_ahead(
    valid_after_block_number: i64,
    next_block_number: i64,
    max_parent_depth: i32,
) -> bool {
    valid_after_block_number > latest_valid_after(next_block_number, max_parent_depth)
}

pub fn too_far_ahead_message(
    valid_after_block_number: i64,
    next_block_number: i64,
    max_parent_depth: i32,
) -> String {
    format!(
        "Deploy validAfterBlockNumber {} is ahead of the highest accepted block number {} \
         (next block {} plus max parent depth {}).",
        valid_after_block_number,
        latest_valid_after(next_block_number, max_parent_depth),
        next_block_number,
        max_parent_depth
    )
}

pub fn repeat_scan_start(
    tip_scan_start: i64,
    floor_block_number: i64,
    deploy_lifespan: i64,
    oldest_valid_after: i64,
) -> Result<i64, NegativeDeployLifespan> {
    Ok(tip_scan_start.min(lowest_live_carrier_height(
        floor_block_number,
        deploy_lifespan,
        oldest_valid_after,
    )?))
}

/// The lowest height a block carrying a copy of a deploy can sit at while
/// that deploy is live on the floor clock: above its own valid-after block,
/// and above the floor's window.
pub fn lowest_live_carrier_height(
    floor_block_number: i64,
    deploy_lifespan: i64,
    oldest_valid_after: i64,
) -> Result<i64, NegativeDeployLifespan> {
    let lowest_live_copy =
        earliest_valid_after(floor_block_number, deploy_lifespan)?.saturating_add(2);
    let lowest_own_copy = oldest_valid_after.saturating_add(1);
    Ok(lowest_own_copy.max(lowest_live_copy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_carrier_sits_above_its_valid_after_block_and_the_floor_window() {
        assert_eq!(lowest_live_carrier_height(100, 50, 80).unwrap(), 81);
        assert_eq!(lowest_live_carrier_height(100, 50, 0).unwrap(), 52);
    }

    #[test]
    fn boundary_is_closed() {
        assert!(!is_open(5, 55, 50).unwrap());
        assert!(is_open(6, 55, 50).unwrap());
    }

    #[test]
    fn subtraction_saturates() {
        assert_eq!(earliest_valid_after(i64::MIN, 1).unwrap(), i64::MIN);
    }

    #[test]
    fn negative_lifespan_is_rejected() {
        assert!(is_open(0, 0, -1).is_err());
        assert!(last_includable_block(0, -1).is_err());
    }

    #[test]
    fn a_deploy_may_name_a_block_up_to_the_citable_depth_ahead() {
        assert!(!is_too_far_ahead(125, 100, 25));
        assert!(is_too_far_ahead(126, 100, 25));
        assert!(!is_too_far_ahead(i64::MAX, i64::MAX - 1, i32::MAX));
    }

    #[test]
    fn repeat_scan_covers_the_gap_between_the_floor_and_tip_clocks() {
        assert_eq!(repeat_scan_start(6, 4, 3, 2).unwrap(), 3);
    }

    #[test]
    fn repeat_scan_stays_at_the_tip_for_a_recent_deploy() {
        assert_eq!(repeat_scan_start(6, 4, 3, 7).unwrap(), 6);
        assert_eq!(repeat_scan_start(6, 4, 3, 5).unwrap(), 6);
    }

    #[test]
    fn repeat_scan_is_not_deepened_by_an_expired_deploy() {
        assert_eq!(repeat_scan_start(6, 4, 3, 0).unwrap(), 3);
    }

    #[test]
    fn repeat_scan_never_starts_above_the_tip_window() {
        assert_eq!(repeat_scan_start(6, 8, 3, 7).unwrap(), 6);
    }

    #[test]
    fn repeat_scan_rejects_a_negative_lifespan() {
        assert!(repeat_scan_start(6, 4, -1, 2).is_err());
    }

    #[test]
    fn last_includable_block_is_the_last_open_reference() {
        let last = last_includable_block(5, 50).unwrap();
        assert!(is_open(5, last, 50).unwrap());
        assert!(!is_open(5, last + 1, 50).unwrap());
    }
}
