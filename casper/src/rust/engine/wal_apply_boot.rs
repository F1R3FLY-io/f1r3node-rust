// Phase 7b-2 item (c) (2026-08-28): boot wire-in for the WAL
// apply-to-follower flow.
//
// Composes the Phase-7b-1 snapshot chunk fetch with the Phase-7b-2
// payload fetch + fresh-tree applier into one background subscriber
// task.  Both boot sites (`casper_launch::create_casper_and_transition_to_running`
// and `initializing::create_casper_and_transition_to_running`) spawn
// this subscriber after building the snapshot chunk context and
// the WAL payload context.
//
// # Flow
//
//   1. Snapshot chunk fetch completes for block B → the sync
//      driver's completion sink receives a `SnapshotCompletion`
//      (block_hash, atomic_root, path).
//   2. This subscriber reads the completion, opens the snapshot
//      bytes via `read_snapshot_bytes`, and decodes them to
//      `Vec<WalEntry>` via `decode_wal_slice`.
//   3. It invokes `apply_wal_slice_after_fetch(wal_payload_driver,
//      wal_entries, |p| p.to_path_buf(), BOOT_APPLY_TIMEOUT,
//      BOOT_APPLY_POLL_INTERVAL)`:
//      - The enumerator queues each unique payload hash for peer
//        fetch (reducer is `|_| None` per DD-7b-2 (a)'s "no
//        production caller yet" posture — a future reducer slice
//        will reproduce bytes locally from block storage).
//      - The poll loop waits until `driver.is_complete()` returns
//        true (or the timeout fires).
//      - On completion, the applier writes each Consensus WAL
//        entry's mutation to the joiner's filesystem, using the
//        canonical path already recorded in the entry (identity
//        `path_map` — no translation).
//
// # Identity path_map
//
// Consensus-static roots are operator-frozen paths agreed across
// validators.  A leader's WAL entry `path = /opt/f1r3fly/consensus-
// static-01/data.bin` matches the joiner's on-disk path at boot;
// the joiner applies directly with no translation.  Test callers
// wanting isolated dirs use the wal_applier's closure API directly
// (see `apply_wal_translated` in `rholang/tests/fs_wal_spec.rs`).
//
// # Failure modes
//
// - `read_snapshot_bytes` fails: peer-served bytes tampered post-
//   assembly.  Logged; the subscriber loops back to the next
//   completion — a future retry via re-broadcast is future work.
// - `decode_wal_slice` returns `MalformedBlob`: byzantine peer
//   fed valid Merkle-checked chunks whose reassembled bytes still
//   fail schema.  Logged; skipped.  Byzantine-peer defense at the
//   chunk-verify layer means this is exceedingly unlikely.
// - `apply_wal_slice_after_fetch` returns `PayloadFetchTimeout`:
//   peers wouldn't serve the payloads within
//   `BOOT_APPLY_TIMEOUT`.  Logged with `pending_count`; the file
//   state stays incomplete and the joiner cannot fully catch up.
//   Operator-facing signal to widen peer set or wait longer.
// - `apply_wal_slice_after_fetch` returns `MissingResolvedHash`:
//   internal race between `is_complete()` and `take_bytes`.
//   Logged; the subscriber continues.
//
// The subscriber runs until the receiver drops (which happens when
// the `SnapshotChunkSyncDriver` is dropped, i.e., process
// shutdown).  No explicit stop plumbing — this is a boot-side
// composition task, not a live control loop.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion;
use crate::rust::engine::wal_payload_sync::{
    apply_wal_slice_after_fetch, BootApplyError, WalPayloadSyncDriver,
};

/// Default boot-time apply timeout.  Matches `STALE_EVICTION_MS`
/// in the payload retriever — if we can't fetch a payload within
/// that window, the retriever would drop it anyway, so waiting
/// longer is pointless.
pub const BOOT_APPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// Poll interval for `apply_wal_slice_after_fetch`'s completion
/// wait loop.  Small enough to react quickly once fetch settles;
/// large enough that idle polling is cheap.
pub const BOOT_APPLY_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Spawn the boot subscriber.  Reads `SnapshotCompletion`
/// notifications from the passed receiver; for each one, decodes
/// the snapshot bytes and runs the apply flow against
/// `wal_payload_driver`.  Returns the JoinHandle so callers can
/// abort on shutdown (though the natural termination path is via
/// receiver drop when the snapshot driver is dropped).
pub fn spawn_boot_apply_subscriber(
    mut rx: UnboundedReceiver<SnapshotCompletion>,
    wal_payload_driver: Arc<WalPayloadSyncDriver>,
    snapshot_dir: std::path::PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(completion) = rx.recv().await {
            handle_completion(&wal_payload_driver, &snapshot_dir, completion).await;
        }
        info!(
            target: "f1r3fly.casper.wal_apply_boot",
            "completion channel closed; boot apply subscriber exiting"
        );
    })
}

async fn handle_completion(
    wal_payload_driver: &Arc<WalPayloadSyncDriver>,
    snapshot_dir: &std::path::Path,
    completion: SnapshotCompletion,
) {
    let SnapshotCompletion {
        block_hash,
        atomic_root,
        path,
    } = completion;
    info!(
        target: "f1r3fly.casper.wal_apply_boot",
        block_hash = ?block_hash,
        atomic_root = hex::encode(atomic_root),
        path = %path.display(),
        "received snapshot completion; starting apply flow"
    );
    // Read + verify + decode.  Read runs in-runtime (small file,
    // ~a few MiB); no need for spawn_blocking here.
    let bytes = match rholang::rust::interpreter::io::snapshot::read_snapshot_bytes(
        snapshot_dir,
        &atomic_root,
    ) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                target: "f1r3fly.casper.wal_apply_boot",
                error = %e,
                atomic_root = hex::encode(atomic_root),
                "read_snapshot_bytes failed; skipping apply"
            );
            return;
        }
    };
    let wal = match rholang::rust::interpreter::io::snapshot::decode_wal_slice(&bytes) {
        Ok(w) => w,
        Err(e) => {
            warn!(
                target: "f1r3fly.casper.wal_apply_boot",
                error = %e,
                atomic_root = hex::encode(atomic_root),
                "decode_wal_slice failed; skipping apply"
            );
            return;
        }
    };
    match apply_wal_slice_after_fetch(
        Arc::clone(wal_payload_driver),
        wal,
        |p| p.to_path_buf(),
        BOOT_APPLY_TIMEOUT,
        BOOT_APPLY_POLL_INTERVAL,
    )
    .await
    {
        Ok(report) => info!(
            target: "f1r3fly.casper.wal_apply_boot",
            wal_entries = report.wal_entries,
            sidecar_populated = report.sidecar_populated,
            atomic_root = hex::encode(atomic_root),
            "apply flow complete for snapshot"
        ),
        Err(BootApplyError::PayloadFetchTimeout { pending_count }) => warn!(
            target: "f1r3fly.casper.wal_apply_boot",
            pending_count,
            atomic_root = hex::encode(atomic_root),
            "apply flow timed out waiting for payloads; some file state may be incomplete"
        ),
        Err(BootApplyError::MissingResolvedHash { hash_hex }) => warn!(
            target: "f1r3fly.casper.wal_apply_boot",
            hash_hex,
            atomic_root = hex::encode(atomic_root),
            "apply flow saw missing resolved hash after is_complete; internal race"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b256::Blake2b256;
    use rholang::rust::interpreter::io::snapshot::write_snapshot;
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;
    use crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion;
    use crate::rust::engine::wal_payload_retriever::WalPayloadRetriever;

    fn hash_of(bytes: &[u8]) -> [u8; 32] {
        let h = Blake2b256::hash(bytes.to_vec());
        let mut out = [0u8; 32];
        out.copy_from_slice(&h);
        out
    }

    /// End-to-end: write a snapshot to disk, pre-resolve its
    /// payload in the WAL driver, send a completion through the
    /// subscriber's channel, and observe the applied file state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_applies_snapshot_wal_end_to_end() {
        let snapshot_dir = tempfile::tempdir().unwrap();

        // Prepare a target file the WAL entry will write into.  The
        // WAL entry's `path` must be the actual on-disk path since
        // the subscriber uses an identity path_map.
        let target_file = tempfile::NamedTempFile::new().unwrap();
        let target_path = target_file.path().to_path_buf();
        drop(target_file); // just want the path; create fresh below
        std::fs::write(&target_path, vec![0u8; 16]).unwrap();

        let payload = b"applied".to_vec();
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: target_path.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(&payload)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let (_path, atomic_root, _merkle) =
            write_snapshot(snapshot_dir.path(), std::slice::from_ref(&entry)).unwrap();

        let wal_driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        wal_driver
            .retriever
            .mark_resolved(hash_of(&payload), payload.clone())
            .await;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        let handle = spawn_boot_apply_subscriber(
            rx,
            Arc::clone(&wal_driver),
            snapshot_dir.path().to_path_buf(),
        );

        tx.send(SnapshotCompletion {
            block_hash: vec![0xAA; 32],
            atomic_root,
            path: rholang::rust::interpreter::io::snapshot::snapshot_path(
                snapshot_dir.path(),
                &atomic_root,
            ),
        })
        .unwrap();

        // Drop the sender to close the channel; the subscriber exits.
        drop(tx);
        handle.await.expect("subscriber join");

        // Target file now carries the payload.
        let got = std::fs::read(&target_path).unwrap();
        assert_eq!(&got[..payload.len()], payload.as_slice());
    }

    /// A malformed snapshot path is handled gracefully: the
    /// subscriber logs and continues to the next message rather
    /// than panicking.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_tolerates_missing_snapshot_bytes() {
        let snapshot_dir = tempfile::tempdir().unwrap();
        let wal_driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        let handle = spawn_boot_apply_subscriber(
            rx,
            Arc::clone(&wal_driver),
            snapshot_dir.path().to_path_buf(),
        );

        // Send a completion for a hash whose file doesn't exist.
        let missing_root = [0xFFu8; 32];
        tx.send(SnapshotCompletion {
            block_hash: vec![0xBB; 32],
            atomic_root: missing_root,
            path: snapshot_dir.path().join("does-not-exist.wal"),
        })
        .unwrap();

        drop(tx);
        handle.await.expect("subscriber join");
        // Test passes if we get here without a panic.
    }
}
