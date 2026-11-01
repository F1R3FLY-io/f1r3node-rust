use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::rho_path::{RhoPathEncode, RhoPathError, RhoPathV1};

#[derive(Clone)]
pub(crate) struct ReceiveOccurrenceIndex {
    keys: PathMap<()>,
}

impl ReceiveOccurrenceIndex {
    pub(crate) fn new() -> Self {
        Self {
            keys: PathMap::new(),
        }
    }

    pub(crate) fn insert_group(&mut self, group_prefix: &[u8]) -> Result<(), RhoPathError> {
        let group = RhoPathV1::parse_continuation_group(group_prefix)?;
        for frame in group.frames() {
            self.keys.insert(occurrence_key(frame, group_prefix), ());
        }
        Ok(())
    }

    pub(crate) fn remove_group(&mut self, group_prefix: &[u8]) -> Result<(), RhoPathError> {
        let group = RhoPathV1::parse_continuation_group(group_prefix)?;
        for frame in group.frames() {
            self.keys.remove(occurrence_key(frame, group_prefix));
        }
        Ok(())
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read only by library tests until task 12002 adds the HotStore read path"
        )
    )]
    pub(crate) fn try_visit_groups_of<E>(
        &self,
        channel_frame: &[u8],
        mut visitor: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = self.keys.read_zipper_at_borrowed_path(channel_frame);
        if !cursor.path_exists() {
            return Ok(());
        }
        if cursor.val().is_some() {
            visitor(cursor.path())?;
        }
        while cursor.to_next_get_val().is_some() {
            visitor(cursor.path())?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn same_keys(&self, other: &Self) -> bool {
        self.keys
            .iter()
            .map(|(key, _)| key)
            .eq(other.keys.iter().map(|(key, _)| key))
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys = Vec::with_capacity(self.keys.val_count());
        keys.extend(self.keys.iter().map(|(key, _)| key));
        keys
    }
}

fn occurrence_key(frame: &[u8], group_prefix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(frame.len().saturating_add(group_prefix.len()));
    key.extend_from_slice(frame);
    key.extend_from_slice(group_prefix);
    key
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "framed only by library tests until task 12002 adds the HotStore read path"
    )
)]
pub(crate) fn channel_frame<C: RhoPathEncode>(channel: &C) -> Vec<u8> {
    let prefix = RhoPathV1::continuation_prefix(std::slice::from_ref(channel));
    RhoPathV1::parse_continuation_group(&prefix)
        .ok()
        .and_then(|group| group.frames().next())
        .expect("a one-channel continuation prefix carries exactly one channel frame")
        .to_vec()
}

#[cfg(test)]
thread_local! {
    static MAINTENANCE_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn arm_maintenance_fault() { MAINTENANCE_FAULT.with(|armed| armed.set(true)); }

#[cfg(test)]
pub(crate) fn injected_maintenance_fault(before: bool, after: bool) -> bool {
    before != after && MAINTENANCE_FAULT.with(|armed| armed.replace(false))
}

#[cfg(test)]
#[path = "../../tests/receive_accelerator/occurrence_index.rs"]
mod occurrence_index_tests;
