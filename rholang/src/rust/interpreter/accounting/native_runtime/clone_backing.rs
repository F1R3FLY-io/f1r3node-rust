use models::rust::host_work::HostWorkDimension;
pub(crate) use shared::rust::clone_backing::CloneBacking;
use shared::rust::clone_backing::{self as backing, BackingError, Walker};

use super::recording::work;
use super::{HostWorkBudget, InterpreterError};
use crate::rust::interpreter::accounting::authority::{
    AuthorityByteEventKind, AuthorityStackBirth, ResourceMultiset,
};
use crate::rust::interpreter::accounting::AuthorityRuntimeEvent;

fn meter(host: &HostWorkBudget) -> impl Fn(usize, usize, usize) -> Result<(), BackingError> + '_ {
    |operations, scanned, allocation| {
        work(host, HostWorkDimension::VerificationOperations, operations)
            .map_err(|_| BackingError::Rejected)?;
        work(host, HostWorkDimension::VerificationBytes, scanned)
            .map_err(|_| BackingError::Rejected)?;
        work(host, HostWorkDimension::SearchStateBytes, allocation)
            .map_err(|_| BackingError::Rejected)
    }
}

// Enabled outside tests by D-O5 (DR-89): replay authority prepare charges
// the copy of a shared observation pointer.
// #[cfg(test)]
pub(crate) fn reserve<T: CloneBacking>(
    value: &T,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::reserve(value, &meter(host)).map_err(|_| InterpreterError::HostWorkRejected)
}
pub(crate) fn reserve_copy_and_cleanup<T: CloneBacking>(
    value: &T,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::reserve_copy_and_cleanup(value, &meter(host))
        .map_err(|_| InterpreterError::HostWorkRejected)
}
// Enabled outside tests by D-O4 (DR-89): the result backing charges the
// copy of the byte-observation pointer slice.
// #[cfg(test)]
pub(crate) fn reserve_slice<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::reserve_slice(values, &meter(host)).map_err(|_| InterpreterError::HostWorkRejected)
}
pub(crate) fn reserve_slice_copy_and_cleanup<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::reserve_slice_copy_and_cleanup(values, &meter(host))
        .map_err(|_| InterpreterError::HostWorkRejected)
}
pub(crate) fn inspect<T: CloneBacking>(
    value: &T,
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::inspect(value, &meter(host)).map_err(|_| InterpreterError::HostWorkRejected)
}
/// D-O4 (DR-89): the cleanup of a copied slice of shared pointers whose
/// payload releases were prepaid at birth (the C5 rule, DR-83).
pub(crate) fn inspect_shared_pointer_slice<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::inspect_shared_pointer_slice(values, &meter(host))
        .map_err(|_| InterpreterError::HostWorkRejected)
}
pub(crate) fn inspect_slice<T: CloneBacking>(
    values: &[T],
    host: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    backing::inspect_slice(values, &meter(host)).map_err(|_| InterpreterError::HostWorkRejected)
}

impl<K: CloneBacking> CloneBacking for ResourceMultiset<K> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.0)
    }
}
impl CloneBacking for AuthorityByteEventKind {
    fn children<'a>(&'a self, _: &mut Walker<'a>) -> Result<(), BackingError> {
        let _: Self = *self;
        Ok(())
    }
    fn inline() -> bool { true }
}
impl CloneBacking for AuthorityRuntimeEvent {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            authority,
            debit,
            byte_observation,
        } = self;
        walker.push(authority)?;
        walker.push(debit)?;
        walker.push(byte_observation)
    }
}
impl CloneBacking for crate::rust::interpreter::accounting::byte_receipts::ByteObservation {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            event_id,
            kind,
            authority,
            measurement,
            legacy_amount,
        } = self;
        walker.push(event_id)?;
        walker.push(kind)?;
        walker.push(authority)?;
        walker.push(measurement)?;
        walker.push(legacy_amount)
    }
}
impl CloneBacking for crate::rust::interpreter::accounting::byte_accounting::ByteCharge {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            introduction_bytes,
            transfer_bytes,
            trace_bytes,
        } = self;
        walker.push(introduction_bytes)?;
        walker.push(transfer_bytes)?;
        walker.push(trace_bytes)
    }
}
impl CloneBacking for AuthorityStackBirth {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        let Self {
            produce_hash,
            cells,
        } = self;
        walker.push(produce_hash)?;
        walker.push(cells)
    }
}

#[cfg(test)]
pub(crate) mod tests;
