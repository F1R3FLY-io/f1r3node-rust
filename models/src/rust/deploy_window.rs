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
        assert!(last_includable_block(0, -1).is_err());
    }

    #[test]
    fn last_includable_block_is_the_last_open_reference() {
        let last = last_includable_block(5, 50).unwrap();
        assert!(is_open(5, last, 50).unwrap());
        assert!(!is_open(5, last + 1, 50).unwrap());
    }
}
