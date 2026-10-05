//! The params of a receive query (`ch ~ (q…; k=v…)`).

use std::collections::BTreeMap;

use super::params::{insert_named, SendParam};
use super::validation::{ReifiedSyntaxError, K_KEY, RESULTS_KEY};
use crate::rhoapi::Par;

/// Collects the params of a query. A query takes named params only, each key is a unique
/// param key, and `k` and `results` exclude each other.
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

/// Checks that the named params of a query do not hold both `k` and `results`.
pub fn check_query_params(named: &BTreeMap<String, Par>) -> Result<(), ReifiedSyntaxError> {
    match (named.contains_key(K_KEY), named.contains_key(RESULTS_KEY)) {
        (true, true) => Err(ReifiedSyntaxError::KAndResults),
        (true, false) | (false, true | false) => Ok(()),
    }
}
