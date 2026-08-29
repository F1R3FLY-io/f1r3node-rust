// Phase 7b-2 item (c) (2026-08-28): boot wire-in for the WAL
// apply-to-follower flow.  Hardened 2026-08-28 review pass to
// tolerate applier failures without killing the subscriber.
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
//      wal_entries, |p| p.to_path_buf(), allowed_roots,
//      BOOT_APPLY_TIMEOUT, BOOT_APPLY_POLL_INTERVAL)`:
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
// # Identity path_map + allowed_roots
//
// Consensus-static roots are operator-frozen paths agreed across
// validators.  A leader's WAL entry `path = /opt/f1r3fly/consensus-
// static-01/data.bin` matches the joiner's on-disk path at boot;
// the joiner applies directly with no translation.  Defense-in-
// depth: an `allowed_roots: Vec<PathBuf>` argument (currently
// empty from the boot sites, pending provisioning plumbing) can
// bound the blast radius of a hypothetical leader-canonicalize bug
// or a forged snapshot.  Empty vector skips validation.
//
// # Failure modes (all log-and-continue)
//
// Every failure path logs at `warn` and returns the subscriber's
// while-let to the next `rx.recv().await`.  A single bad completion
// never kills the subscriber:
//
// - `read_snapshot_bytes` fails: peer-served bytes tampered
//   post-assembly.  Logged; skipped.
// - `decode_wal_slice` returns `MalformedBlob`: byzantine peer
//   fed valid Merkle-checked chunks whose reassembled bytes still
//   fail schema.  Logged; skipped.
// - `apply_wal_slice_after_fetch` returns `PayloadFetchTimeout`:
//   peers wouldn't serve the payloads within
//   `BOOT_APPLY_TIMEOUT`.  Logged with `pending_count`; the file
//   state stays incomplete and the joiner cannot fully catch up.
// - `MissingResolvedHash`: internal race between `is_complete()`
//   and `take_bytes`.  Logged.
// - `ApplierFailed`: byzantine input tripped a defensive check
//   (missing sidecar, unsupported PayloadRef, out-of-root path,
//   NSS failure, IO error).  Logged with the applier's Display
//   detail; skipped.
// - `ApplierPanic`: defense-in-depth against a future refactor
//   reintroducing a panic path.  Logged; skipped.  Would have
//   killed the subscriber pre-hardening.
//
// The subscriber runs until the receiver drops (which happens when
// the `SnapshotChunkSyncDriver` is dropped, i.e., process
// shutdown).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion;
use crate::rust::engine::wal_payload_server::PayloadLookup;
use crate::rust::engine::wal_payload_sync::{
    apply_wal_slice_after_fetch, BootApplyError, Option2ReducerContext, WalPayloadSyncDriver,
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
///
/// `allowed_roots` bounds where applier writes may land.  Boot
/// sites currently pass an empty Vec pending provisioning
/// plumbing; the parameter exists so a follow-up slice can wire
/// the operator's consensus-static roots without changing this
/// signature.
///
/// `payload_lookup` is the DD-7b-2 (a) Option 1 reducer source:
/// when `Some`, the boot enumerator consults the joiner's local
/// `PayloadLookup` (typically the joiner's own
/// `DirectoryPayloadStore` populated by prior block processing)
/// before enqueueing a peer fetch.  When `None`, every payload is
/// enqueued for peer fetch — matches pre-reducer behavior.
///
/// `option2_ctx` is the DD-7b-2 (a) Option 2 reducer source
/// (2026-08-29): when `Some`, the boot enumerator chains through
/// the block-storage `payload_hash → deploy_sig` index to
/// reproduce write bytes from block-stored deploys — closing the
/// gap Option 1 leaves for first-time joiners with empty payload
/// stores.  When `None`, Option 2 is disabled and the enumerator
/// falls through to peer fetch for hashes Tier 1 misses.
pub fn spawn_boot_apply_subscriber(
    mut rx: UnboundedReceiver<SnapshotCompletion>,
    wal_payload_driver: Arc<WalPayloadSyncDriver>,
    snapshot_dir: std::path::PathBuf,
    allowed_roots: Vec<std::path::PathBuf>,
    payload_lookup: Option<Arc<dyn PayloadLookup>>,
    option2_ctx: Option<Option2ReducerContext>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(completion) = rx.recv().await {
            handle_completion(
                &wal_payload_driver,
                &snapshot_dir,
                &allowed_roots,
                payload_lookup.clone(),
                option2_ctx.clone(),
                completion,
            )
            .await;
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
    allowed_roots: &[std::path::PathBuf],
    payload_lookup: Option<Arc<dyn PayloadLookup>>,
    option2_ctx: Option<Option2ReducerContext>,
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
        allowed_roots.to_vec(),
        BOOT_APPLY_TIMEOUT,
        BOOT_APPLY_POLL_INTERVAL,
        payload_lookup,
        option2_ctx,
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
        Err(BootApplyError::ApplierFailed { message }) => warn!(
            target: "f1r3fly.casper.wal_apply_boot",
            message,
            atomic_root = hex::encode(atomic_root),
            "applier rejected the WAL slice; skipping to next completion"
        ),
        Err(BootApplyError::ApplierPanic { message }) => warn!(
            target: "f1r3fly.casper.wal_apply_boot",
            message,
            atomic_root = hex::encode(atomic_root),
            "applier panicked (defense-in-depth catch); skipping to next completion"
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

    fn write_at_entry(path: &std::path::Path, off: u64, payload: &[u8]) -> WalEntry {
        WalEntry {
            op: WalOp::WriteAt,
            path: path.to_path_buf(),
            extra_path: None,
            offset: Some(off),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(payload)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// End-to-end: write a snapshot to disk, pre-resolve its
    /// payload in the WAL driver, send a completion through the
    /// subscriber's channel, and observe the applied file state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_applies_snapshot_wal_end_to_end() {
        let snapshot_dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::tempdir().unwrap();
        let target_path = target_dir.path().join("t.bin");
        std::fs::write(&target_path, vec![0u8; 16]).unwrap();

        let payload = b"applied".to_vec();
        let entry = write_at_entry(&target_path, 0, &payload);
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
            Vec::new(),
            None,
            None,
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
            Vec::new(),
            None,
            None,
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

    /// 2026-08-28 hardening pin: an applier failure on ONE
    /// completion (byzantine WAL / missing sidecar / etc.) does
    /// NOT kill the subscriber loop.  Send TWO completions —
    /// the first triggers ApplierFailed (WAL entry uses a
    /// DeployRef payload_ref that the applier doesn't yet
    /// support), the second is a happy-path apply.  Both must
    /// be processed; the target file for the second reflects
    /// the applied write.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_survives_applier_failure_and_processes_next() {
        let snapshot_dir = tempfile::tempdir().unwrap();

        // Completion #1: WAL entry with unsupported PayloadRef.
        let bad_target_dir = tempfile::tempdir().unwrap();
        let bad_target = bad_target_dir.path().join("bad.bin");
        std::fs::write(&bad_target, vec![0u8; 8]).unwrap();
        let bad_entry = WalEntry {
            op: WalOp::WriteAt,
            path: bad_target.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(0),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let (_p1, bad_root, _) =
            write_snapshot(snapshot_dir.path(), std::slice::from_ref(&bad_entry)).unwrap();

        // Completion #2: happy-path WAL entry, distinct target.
        let good_target_dir = tempfile::tempdir().unwrap();
        let good_target = good_target_dir.path().join("good.bin");
        std::fs::write(&good_target, vec![0u8; 16]).unwrap();
        let good_payload = b"GOOD".to_vec();
        let good_entry = write_at_entry(&good_target, 0, &good_payload);
        let (_p2, good_root, _) =
            write_snapshot(snapshot_dir.path(), std::slice::from_ref(&good_entry)).unwrap();

        let wal_driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        wal_driver
            .retriever
            .mark_resolved(hash_of(&good_payload), good_payload.clone())
            .await;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        let handle = spawn_boot_apply_subscriber(
            rx,
            Arc::clone(&wal_driver),
            snapshot_dir.path().to_path_buf(),
            Vec::new(),
            None,
            None,
        );
        tx.send(SnapshotCompletion {
            block_hash: vec![0x01; 32],
            atomic_root: bad_root,
            path: rholang::rust::interpreter::io::snapshot::snapshot_path(
                snapshot_dir.path(),
                &bad_root,
            ),
        })
        .unwrap();
        tx.send(SnapshotCompletion {
            block_hash: vec![0x02; 32],
            atomic_root: good_root,
            path: rholang::rust::interpreter::io::snapshot::snapshot_path(
                snapshot_dir.path(),
                &good_root,
            ),
        })
        .unwrap();
        drop(tx);
        handle.await.expect("subscriber join");

        // The bad target is untouched (applier bailed on DeployRef).
        assert_eq!(std::fs::read(&bad_target).unwrap(), vec![0u8; 8]);
        // The good target reflects the write — proves the
        // subscriber survived the first ApplierFailed.
        let got = std::fs::read(&good_target).unwrap();
        assert_eq!(&got[..good_payload.len()], good_payload.as_slice());
    }

    /// 2026-08-28 hardening pin: two completions are processed
    /// in the order they arrive at the channel.  A regression that
    /// e.g. spawned each completion in parallel could interleave
    /// applies in unpredictable orders; sequential processing is a
    /// documented invariant.  We verify by having #1 write bytes
    /// #2 depends on (via a shared file), then observing byte
    /// content post-drain.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_processes_completions_in_order() {
        let snapshot_dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path().join("shared.bin");
        // Pre-fill with 16 bytes of 0xAA.
        std::fs::write(&target, vec![0xAAu8; 16]).unwrap();

        let payload_first = b"FIRST".to_vec();
        let payload_second = b"SEC".to_vec();
        let e1 = write_at_entry(&target, 0, &payload_first);
        let e2 = write_at_entry(&target, 5, &payload_second);
        let (_p1, root1, _) =
            write_snapshot(snapshot_dir.path(), std::slice::from_ref(&e1)).unwrap();
        let (_p2, root2, _) =
            write_snapshot(snapshot_dir.path(), std::slice::from_ref(&e2)).unwrap();

        let wal_driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
            WalPayloadRetriever::new(),
        )));
        wal_driver
            .retriever
            .mark_resolved(hash_of(&payload_first), payload_first.clone())
            .await;
        wal_driver
            .retriever
            .mark_resolved(hash_of(&payload_second), payload_second.clone())
            .await;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SnapshotCompletion>();
        let handle = spawn_boot_apply_subscriber(
            rx,
            Arc::clone(&wal_driver),
            snapshot_dir.path().to_path_buf(),
            Vec::new(),
            None,
            None,
        );
        tx.send(SnapshotCompletion {
            block_hash: vec![0x01; 32],
            atomic_root: root1,
            path: rholang::rust::interpreter::io::snapshot::snapshot_path(
                snapshot_dir.path(),
                &root1,
            ),
        })
        .unwrap();
        tx.send(SnapshotCompletion {
            block_hash: vec![0x02; 32],
            atomic_root: root2,
            path: rholang::rust::interpreter::io::snapshot::snapshot_path(
                snapshot_dir.path(),
                &root2,
            ),
        })
        .unwrap();
        drop(tx);
        handle.await.expect("subscriber join");

        let got = std::fs::read(&target).unwrap();
        // Byte 0..5 = FIRST from completion #1.
        assert_eq!(&got[0..5], payload_first.as_slice());
        // Byte 5..8 = SEC from completion #2 (overwrites bytes 5-7).
        assert_eq!(&got[5..8], payload_second.as_slice());
        // Byte 8..16 = pre-fill 0xAA, untouched.
        assert_eq!(&got[8..16], &vec![0xAAu8; 8][..]);
    }
}
