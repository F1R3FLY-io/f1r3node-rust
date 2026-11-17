use models::rust::casper::protocol::casper_message::StoreItemsCursorProtocol;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::trie_exporter::StateExportError;
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::rust::errors::CasperError;

pub(crate) const MAX_PAGE_WORK_ITEMS: i32 = 1_024;

/// This post-encode ceiling rejects oversized responses before transport, but cannot bound
/// storage reads or materialization because a single authenticated cold object is indivisible.
/// The PathMap-DAG streaming exporter must frame authenticated objects and apply the configured
/// transport byte budget before materialization without silently splitting an object.
pub(crate) const MAX_RESPONSE_BYTES: usize = 255 * 1024 * 1024;

pub(crate) struct StateExportConcurrencyGate {
    slot: Semaphore,
}

impl StateExportConcurrencyGate {
    pub(crate) fn new() -> Self {
        Self {
            slot: Semaphore::new(1),
        }
    }

    pub(crate) fn try_enter(&self) -> Result<SemaphorePermit<'_>, CasperError> {
        self.slot.try_acquire().map_err(|_| {
            CasperError::RuntimeError(
                "state-export capacity is busy; retry the request later".to_string(),
            )
        })
    }
}

fn admission_error(message: impl Into<String>) -> CasperError {
    CasperError::StateExportError(StateExportError::InvalidPath(message.into()))
}

pub(crate) fn unserved_cursor_protocol_error(
    cursor_protocol: StoreItemsCursorProtocol,
) -> CasperError {
    admission_error(format!(
        "state-export cursor protocol {cursor_protocol:?} is not served by this node"
    ))
}

pub(crate) fn validate_request(
    start_path: &[(Blake2b256Hash, Option<u8>)],
    skip: i32,
    take: i32,
    cursor_protocol: StoreItemsCursorProtocol,
) -> Result<(&Blake2b256Hash, StoreItemsCursorProtocol), CasperError> {
    let cursor_protocol = cursor_protocol
        .require_supported()
        .map_err(admission_error)?;
    if take < 1 {
        return Err(admission_error(format!(
            "state-export take must be positive, found {take}"
        )));
    }
    match cursor_protocol {
        StoreItemsCursorProtocol::LegacyEmptyTerminal => {
            if skip < 0 {
                return Err(admission_error(format!(
                    "legacy state-export skip must be nonnegative, found {skip}"
                )));
            }
            let page_work = skip
                .checked_add(take)
                .ok_or_else(|| admission_error("legacy state-export skip + take overflowed"))?;
            if page_work > MAX_PAGE_WORK_ITEMS {
                return Err(admission_error(format!(
                    "legacy state-export skip + take must not exceed the per-page work budget \
                     {MAX_PAGE_WORK_ITEMS}, found {page_work}"
                )));
            }
        }
        StoreItemsCursorProtocol::AuthenticatedTerminal => {
            if skip != 0 {
                return Err(admission_error(format!(
                    "authenticated state-export skip must be 0, found {skip}"
                )));
            }
            if take > MAX_PAGE_WORK_ITEMS {
                return Err(admission_error(format!(
                    "authenticated state-export take must not exceed the per-page work budget \
                     {MAX_PAGE_WORK_ITEMS}, found {take}"
                )));
            }
        }
        StoreItemsCursorProtocol::DagStream => {
            return Err(unserved_cursor_protocol_error(cursor_protocol));
        }
        StoreItemsCursorProtocol::Unspecified | StoreItemsCursorProtocol::Unknown(_) => {
            unreachable!("require_supported rejected an unavailable cursor protocol")
        }
    }
    let root = start_path
        .first()
        .map(|(root, _)| root)
        .ok_or_else(|| admission_error("state-export cursor has no root component"))?;
    if root.bytes().len() != 32 {
        return Err(admission_error(format!(
            "state-export root must contain 32 bytes, found {}",
            root.bytes().len()
        )));
    }
    Ok((root, cursor_protocol))
}

pub(crate) fn require_verified_root(
    root: &Blake2b256Hash,
    contains_root: bool,
) -> Result<(), CasperError> {
    if contains_root {
        Ok(())
    } else {
        Err(admission_error(format!(
            "state-export root {root} is not in the verified-root catalog"
        )))
    }
}

pub(crate) fn validate_response(
    history_items: usize,
    data_items: usize,
    take: i32,
    encoded_bytes: usize,
) -> Result<(), CasperError> {
    if take < 1 || history_items > take as usize {
        return Err(admission_error(format!(
            "state-export response contains {history_items} history items for take {take}"
        )));
    }
    let _total_items = history_items
        .checked_add(data_items)
        .ok_or_else(|| admission_error("state-export response item count overflowed"))?;
    if encoded_bytes > MAX_RESPONSE_BYTES {
        return Err(admission_error(format!(
            "state-export response contains {encoded_bytes} encoded bytes; protocol maximum is \
             {MAX_RESPONSE_BYTES}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![7; 32]) }

    #[test]
    fn request_admission_bounds_work_before_export() {
        let path = vec![(root(), None)];
        assert!(
            validate_request(&path, 1, 1, StoreItemsCursorProtocol::AuthenticatedTerminal).is_err()
        );
        assert!(
            validate_request(&path, 0, 0, StoreItemsCursorProtocol::AuthenticatedTerminal).is_err()
        );
        assert!(validate_request(
            &path,
            0,
            MAX_PAGE_WORK_ITEMS + 1,
            StoreItemsCursorProtocol::AuthenticatedTerminal,
        )
        .is_err());
        assert!(validate_request(
            &path,
            0,
            MAX_PAGE_WORK_ITEMS,
            StoreItemsCursorProtocol::AuthenticatedTerminal,
        )
        .is_ok());
    }

    #[test]
    fn legacy_request_admission_preserves_bounded_skip_compatibility() {
        let path = vec![(root(), None)];
        assert!(validate_request(
            &path,
            24,
            MAX_PAGE_WORK_ITEMS - 24,
            StoreItemsCursorProtocol::LegacyEmptyTerminal,
        )
        .is_ok());
        assert!(validate_request(
            &path,
            24,
            MAX_PAGE_WORK_ITEMS - 23,
            StoreItemsCursorProtocol::LegacyEmptyTerminal,
        )
        .is_err());
        assert!(
            validate_request(&path, -1, 1, StoreItemsCursorProtocol::LegacyEmptyTerminal).is_err()
        );
    }

    #[test]
    fn root_admission_requires_verified_catalog_membership() {
        assert!(require_verified_root(&root(), true).is_ok());
        assert!(require_verified_root(&root(), false).is_err());
    }

    #[test]
    fn concurrent_export_admission_rejects_instead_of_queueing() {
        use std::sync::{Arc, Barrier};

        let gate = Arc::new(StateExportConcurrencyGate::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let active_gate = Arc::clone(&gate);
        let active_entered = Arc::clone(&entered);
        let active_release = Arc::clone(&release);
        let active = std::thread::spawn(move || {
            let _permit = active_gate.try_enter().unwrap();
            active_entered.wait();
            active_release.wait();
        });

        entered.wait();
        assert!(gate.try_enter().is_err());
        release.wait();
        active.join().unwrap();
        assert!(gate.try_enter().is_ok());
    }

    #[test]
    fn response_admission_enforces_item_and_encoded_byte_budgets() {
        assert!(validate_response(1, 256, 1, 1).is_ok());
        assert!(validate_response(1_025, 0, 1_024, 1).is_err());
        assert!(validate_response(1, usize::MAX, 1, 1).is_err());
        assert!(validate_response(1, 0, 1, MAX_RESPONSE_BYTES).is_ok());
        assert!(validate_response(1, 0, 1, MAX_RESPONSE_BYTES + 1).is_err());
    }

    #[test]
    fn a_dag_stream_request_is_refused_until_the_protocol_is_served() {
        let path = vec![(root(), None)];
        for take in [1, MAX_PAGE_WORK_ITEMS] {
            assert!(validate_request(
                &path,
                0,
                take,
                StoreItemsCursorProtocol::AuthenticatedTerminal,
            )
            .is_ok());
            match validate_request(&path, 0, take, StoreItemsCursorProtocol::DagStream) {
                Err(CasperError::StateExportError(StateExportError::InvalidPath(message))) => {
                    assert!(
                        message.contains("DagStream is not served"),
                        "unexpected refusal of a DAG stream request: {message}"
                    )
                }
                other => {
                    panic!("a DAG stream request must be refused as not served, found {other:?}")
                }
            }
        }
    }
}
