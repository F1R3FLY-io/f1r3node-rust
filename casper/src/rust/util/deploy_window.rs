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
    fn expiration_starts_after_boundary() {
        assert!(!is_past_expiration_cutoff(5, 55, 50).unwrap());
        assert!(is_past_expiration_cutoff(4, 55, 50).unwrap());
    }
}
