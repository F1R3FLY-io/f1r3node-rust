//! The params of a receive query (`ch ~ (q…; k=v…)`).

use std::collections::BTreeMap;

use super::params::{insert_named, SendParam};
use super::validation::{ReifiedSyntaxError, K_KEY, RADIUS_KEY, RESULTS_KEY, THRESHOLD_KEY};
use crate::rhoapi::Par;

/// Collects the params of a query. A query takes named params only, each key is a unique
/// param key, `k` and `results` exclude each other, and so do `threshold` and `radius`.
pub fn collect_query_params(
    params: Vec<SendParam>,
) -> Result<BTreeMap<String, Par>, ReifiedSyntaxError> {
    let mut named = BTreeMap::new();
    for param in params {
        match param {
            SendParam::Positional(_) => return Err(ReifiedSyntaxError::PositionalQueryParam),
            SendParam::Named(key, value) => insert_named(&mut named, key, value)?,
        }
    }
    check_query_params(&named)?;
    Ok(named)
}

/// Checks that the named params of a query hold neither both `k` and `results` nor both
/// `threshold` and `radius` (decision 56).
pub fn check_query_params(named: &BTreeMap<String, Par>) -> Result<(), ReifiedSyntaxError> {
    match (
        named.contains_key(K_KEY) && named.contains_key(RESULTS_KEY),
        named.contains_key(THRESHOLD_KEY) && named.contains_key(RADIUS_KEY),
    ) {
        (true, _) => Err(ReifiedSyntaxError::KAndResults),
        (false, true) => Err(ReifiedSyntaxError::ConflictingParams),
        (false, false) => Ok(()),
    }
}
