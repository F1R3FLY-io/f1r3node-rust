use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LedgerError {
    Overflow,
    Insufficient,
}

pub(crate) fn validate_add<K: Ord>(
    target: &BTreeMap<K, u64>,
    debit: &BTreeMap<K, u64>,
    capacity: Option<&BTreeMap<K, u64>>,
) -> Result<(), LedgerError> {
    for (key, amount) in debit {
        let next = target
            .get(key)
            .copied()
            .unwrap_or(0)
            .checked_add(*amount)
            .ok_or(LedgerError::Overflow)?;
        if capacity.is_some_and(|limit| next > limit.get(key).copied().unwrap_or(0)) {
            return Err(LedgerError::Insufficient);
        }
    }
    if capacity.is_some_and(|limit| {
        target
            .iter()
            .any(|(key, amount)| *amount > limit.get(key).copied().unwrap_or(0))
    }) {
        return Err(LedgerError::Insufficient);
    }
    Ok(())
}

pub(crate) fn add_assign<K: Ord + Copy>(
    target: &mut BTreeMap<K, u64>,
    debit: &BTreeMap<K, u64>,
) -> Result<(), LedgerError> {
    validate_add(target, debit, None)?;
    for (key, amount) in debit {
        let next = target.get(key).copied().unwrap_or(0) + *amount;
        if next == 0 {
            target.remove(key);
        } else {
            target.insert(*key, next);
        }
    }
    Ok(())
}

pub(crate) fn sub_assign<K: Ord + Copy>(
    target: &mut BTreeMap<K, u64>,
    debit: &BTreeMap<K, u64>,
) -> Result<(), LedgerError> {
    if debit
        .iter()
        .any(|(key, amount)| target.get(key).copied().unwrap_or(0) < *amount)
    {
        return Err(LedgerError::Insufficient);
    }
    for (key, amount) in debit {
        let next = target.get(key).copied().unwrap_or(0) - *amount;
        if next == 0 {
            target.remove(key);
        } else {
            *target.get_mut(key).expect("validated positive balance") = next;
        }
    }
    Ok(())
}
