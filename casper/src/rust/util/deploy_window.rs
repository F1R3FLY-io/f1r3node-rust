use crate::rust::errors::CasperError;

pub fn earliest_valid_after(
    reference_block_number: i64,
    deploy_lifespan: i64,
) -> Result<i64, CasperError> {
    if deploy_lifespan < 0 {
        return Err(CasperError::RuntimeError(format!(
            "deploy lifespan must be non-negative: {}",
            deploy_lifespan
        )));
    }
    Ok(reference_block_number.saturating_sub(deploy_lifespan))
}

pub fn is_open(
    valid_after_block_number: i64,
    reference_block_number: i64,
    deploy_lifespan: i64,
) -> Result<bool, CasperError> {
    Ok(valid_after_block_number > earliest_valid_after(reference_block_number, deploy_lifespan)?)
}

pub fn is_past_expiration_cutoff(
    valid_after_block_number: i64,
    reference_block_number: i64,
    deploy_lifespan: i64,
) -> Result<bool, CasperError> {
    Ok(valid_after_block_number < earliest_valid_after(reference_block_number, deploy_lifespan)?)
}

pub fn repeat_scan_start(
    tip_scan_start: i64,
    floor_block_number: i64,
    deploy_lifespan: i64,
    oldest_valid_after: i64,
) -> Result<i64, CasperError> {
    let lowest_live_copy =
        earliest_valid_after(floor_block_number, deploy_lifespan)?.saturating_add(2);
    let lowest_own_copy = oldest_valid_after.saturating_add(1);
    Ok(tip_scan_start.min(lowest_own_copy.max(lowest_live_copy)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn repeat_scan_covers_the_gap_between_the_floor_and_tip_clocks() {
        assert_eq!(repeat_scan_start(6, 4, 3, 2).unwrap(), 3);
        assert!(is_open(2, 4, 3).unwrap());
        assert!(!is_open(1, 4, 3).unwrap());
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
    fn expiration_starts_after_boundary() {
        assert!(!is_past_expiration_cutoff(5, 55, 50).unwrap());
        assert!(is_past_expiration_cutoff(4, 55, 50).unwrap());
    }
}
