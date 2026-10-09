// Phase 7b-2 boot wire-in for the WAL apply-to-follower flow.
//
// Composes the Phase-7b-1 snapshot chunk fetch with the Phase-7b-2
// payload fetch + fresh-tree applier into one background
// subscriber task.  Both joiner-eligible boot sites
// (`casper_launch::handle_restart_from_approved_block_lfs` and
// `initializing::create_casper_and_transition_to_running`) spawn
// this subscriber after installing the Phase-7b contexts on
// `Running`.
//
// # Flow
//
//   1. Snapshot chunk fetch completes for block B → the sync
//      driver's completion sink (slice 5.103's `install_completion_sink`)
//      sends a `SnapshotCompletion { block_hash, atomic_root, path }`
//      down the mpsc.
//   2. This subscriber reads the completion, opens the snapshot
//      bytes via `snapshot::read_snapshot_bytes`, and decodes
//      them to `Vec<WalEntry>` via `snapshot::decode_wal_slice`.
//   3. It invokes `apply_wal_slice_after_fetch` (slice 5.129):
//      - The enumerator queues each unique payload hash for peer
//        fetch (or resolves locally via Tier 1 `PayloadLookup`).
//      - The poll loop waits until `driver.is_complete()` returns
//        true (or `BOOT_APPLY_TIMEOUT` fires).
//      - On completion, the applier writes each Consensus WAL
//        entry's mutation to the joiner's filesystem — Consensus-
//        cap entries are routed through
//        `registry.resolve_wal_entry_root_rel` (bundle-relative →
//        per-validator on-disk); Oracular entries fall through
//        to the parent-basename split.
//
// # `registry` (Shape A resolver) + `allowed_roots`
//
// Consensus WAL entries record bundle-relative paths
// (`/@bundle/<...>`).  The joiner's `RootIdentityRegistry` —
// populated at `node::setup` from the operator's fs bundle (when
// that wiring lands) — rewrites those to the joiner's
// per-validator on-disk subdir before the syscall.  Oracular
// entries have no matching prefix and fall through to identity.
//
// Defense-in-depth: `allowed_roots` bounds where applier writes
// may land.  Empty vector skips validation (pre-Shape-A behavior).
//
// # Failure modes (all log-and-continue)
//
// Every failure path logs at `warn` and returns the subscriber's
// `while let` to the next `rx.recv().await`.  A single bad
// completion never kills the subscriber:
//
//   * `read_snapshot_bytes` fails — peer-served bytes tampered
//     post-assembly OR local disk read error.
//   * `decode_wal_slice` returns `MalformedBlob` — byzantine peer
//     fed valid Merkle-checked chunks whose reassembled bytes
//     still don't parse.
//   * `apply_wal_slice_after_fetch` returns any `BootApplyError`
//     variant — timeout, missing resolved hash, applier failure,
//     applier panic — each gets a dedicated warn-log describing
//     the failure semantics.
//
// # Deferred (future slice)
//
// The `option2_ctx: Option<Option2ReducerContext>` parameter the
// fileio version carries isn't present here — the Option 2
// cluster (block-storage-backed replay reducer) hasn't landed
// on triage.  When it does, this signature grows by one param
// and the inner `apply_wal_slice_after_fetch` call grows a
// trailing `option2_ctx` arg.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use crate::rust::engine::snapshot_chunk_sync::SnapshotCompletion;
use crate::rust::engine::wal_payload_server::PayloadLookup;
use crate::rust::engine::wal_payload_sync::{
    apply_wal_slice_after_fetch, BootApplyError, WalPayloadSyncDriver,
};

/// Default boot-time apply timeout.  Matches `STALE_EVICTION_MS`
/// in the payload retriever (slice 5.118) — if we can't fetch a
/// payload within that window, the retriever would drop it
/// anyway, so waiting longer here is pointless.
pub const BOOT_APPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// Poll interval for `apply_wal_slice_after_fetch`'s completion
/// wait loop.  Small enough to react quickly once fetch settles;
/// large enough that idle polling is cheap.
pub const BOOT_APPLY_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Spawn the boot subscriber.  Reads `SnapshotCompletion`
/// notifications from `rx`; for each one, decodes the snapshot
/// bytes and runs the apply flow against `wal_payload_driver`.
/// Returns the `JoinHandle` so callers can abort on shutdown
/// (though the natural termination path is via receiver drop
/// when the snapshot driver is dropped).
///
/// `allowed_roots` bounds where applier writes may land.  Boot
/// sites currently pass an empty `Vec` pending provisioning
/// plumbing; the parameter exists so a follow-up slice can wire
/// the operator's consensus-static roots without changing this
/// signature.
///
/// `payload_lookup` is the DD-7b-2 (a) Option 1 reducer source:
/// when `Some`, the boot enumerator consults the joiner's local
/// `PayloadLookup` (typically the joiner's own
/// `DirectoryPayloadStore` populated by prior block processing)
/// before enqueueing a peer fetch.  When `None`, every payload
/// is enqueued for peer fetch — matches pre-reducer behavior.
pub fn spawn_boot_apply_subscriber(
    mut rx: UnboundedReceiver<SnapshotCompletion>,
    wal_payload_driver: Arc<WalPayloadSyncDriver>,
    snapshot_dir: std::path::PathBuf,
    registry: rholang::rust::interpreter::io::path::identity::RootIdentityRegistry,
    allowed_roots: Vec<std::path::PathBuf>,
    payload_lookup: Option<Arc<dyn PayloadLookup>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(completion) = rx.recv().await {
            handle_completion(
                &wal_payload_driver,
                &snapshot_dir,
                &registry,
                &allowed_roots,
                payload_lookup.clone(),
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
    registry: &rholang::rust::interpreter::io::path::identity::RootIdentityRegistry,
    allowed_roots: &[std::path::PathBuf],
    payload_lookup: Option<Arc<dyn PayloadLookup>>,
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
        registry.clone(),
        allowed_roots.to_vec(),
        BOOT_APPLY_TIMEOUT,
        BOOT_APPLY_POLL_INTERVAL,
        payload_lookup,
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
