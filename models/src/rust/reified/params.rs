//! Send params in source order (`x!(d…; p…, k=v…)`).

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use super::encoding::SendParams;
use super::validation::{is_param_key, IdentifierPosition, ReifiedSyntaxError};
use crate::rhoapi::Par;

/// One param of a send or a query, in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendParam {
    Positional(Par),
    Named(String, Par),
}

/// Collects the params of a send. Positional params come first, and each key is a unique
/// param key. The result is empty when the send has no params.
pub fn collect_send_params(params: Vec<SendParam>) -> Result<SendParams, ReifiedSyntaxError> {
    let positional_count = params
        .iter()
        .filter(|param| matches!(param, SendParam::Positional(_)))
        .count();
    let mut positional = Vec::with_capacity(positional_count);
    let mut named = BTreeMap::new();
    for param in params {
        match param {
            SendParam::Positional(value) if named.is_empty() => positional.push(value),
            SendParam::Positional(_) => return Err(ReifiedSyntaxError::PositionalAfterNamed),
            SendParam::Named(key, value) => insert_named(&mut named, key, value)?,
        }
    }
    Ok(SendParams { positional, named })
}

/// Adds a named param. The key must be a param key that `named` does not hold yet.
pub(crate) fn insert_named(
    named: &mut BTreeMap<String, Par>,
    key: String,
    value: Par,
) -> Result<(), ReifiedSyntaxError> {
    if !is_param_key(&key) {
        return Err(ReifiedSyntaxError::InvalidIdentifier {
            name: key,
            position: IdentifierPosition::ParamKey,
        });
    }
    match named.entry(key) {
        Entry::Vacant(slot) => {
            slot.insert(value);
            Ok(())
        }
        Entry::Occupied(slot) => Err(ReifiedSyntaxError::DuplicateKey {
            key: slot.key().clone(),
        }),
    }
}
