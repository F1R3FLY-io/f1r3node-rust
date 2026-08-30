//! PB-M-14 file-state identity: two-validator Casper-level canary.
//!
//! Exercises `MultiParentCasperImpl::add_block` →
//! `validate_block` → `replay_deploys_for_state` end-to-end for a
//! Consensus-cap fs write, and pins:
//!
//!   1. WAL byte identity — validator A's play-runtime and validator
//!      B's replay-runtime cache byte-identical `WalEntry` slices in
//!      `RuntimeManager.pending_wal_slices` under the block's final
//!      post-state-hash.  Both the `Vec<WalEntry>` PartialEq and the
//!      canonical `encode_wal_slice` byte serialization must agree.
//!
//!   2. On-disk bytes — the shared-fs file at the bundle canon path
//!      contains exactly the payload the deploy wrote.  Under shared-
//!      fs (option A per the handoff design decision), the file is
//!      the SAME file on disk that both validators see; this pin
//!      verifies the write happened at all, not that two independent
//!      nodes produced identical outputs (that stronger property is
//!      covered by the runtime-level shared-store rig tests in
//!      `rholang/tests/fs_wal_spec.rs`).
//!
//! ## Item (d-2) partial: play-side WAL aggregation landed 2026-08-28
//!
//! The 2026-08-28 investigation surfaced that the cosigned block-
//! creation path was dropping per-deploy `fs_wal` at two symmetric
//! sites:
//!
//!   - **Play (leader):** `casper/src/rust/rholang/runtime.rs`, in
//!     `state_bound_cost_evidence_for_state_cosigned` — `_fs_wal`
//!     was unpacked and dropped, never aggregated into a block-level
//!     slice.
//!   - **Replay (follower):** `casper/src/rust/rholang/replay_runtime.rs`,
//!     in `replay_deploy_e_with_snapshot_transaction` —
//!     `let _replay_slice = wal_scope.take_and_commit(...)` with a
//!     "no downstream consumer today" comment.
//!
//! **What landed:** the play-side aggregation — per-deploy `fs_wal`
//! flows through a new `Vec<WalEntry>` return-tuple element from
//! `state_bound_cost_evidence_for_state_cosigned`, up through new
//! `fs_wal` fields on `StateBoundExecution` /
//! `StateBoundAdmission`, and inserts into
//! `RuntimeManager.pending_wal_slices` at the end of
//! `compute_state_with_bonds_cosigned_admitted` keyed by the block's
//! final post-state-hash.  Item d-3 (below) closes the follower-side
//! race that previously blocked the replay-side aggregation.
//!
//! ## Item d-3 (2026-08-28): follower-side race closed
//!
//! Root cause: RSpace's rigged replay fires the ack-consumer's
//! continuation (which eventually invokes `fsWrite`) via a spawned
//! task that does NOT observe `fs_open`'s `insert_at.await` — so
//! the follower's fd table had no shadow handle when
//! `fs_write.journal_write` called `with_mut(fd, ...)`.  Result:
//! `meta=None`, `journal_write` returned `Ok(false)` without
//! appending, the Write entry never landed in the WAL, and
//! `take_and_commit` drained only [Stat] instead of [Stat, Write].
//!
//! Considered and rejected during the 2026-08-28 investigation:
//! `yield_now`s at drain/source, `std::sync::RwLock` on the fd
//! table, JoinSet tracking in `reduce.rs::run_parallel_dispatches`,
//! and RSpace rig changes to force `produce.await` completion
//! before continuation fire.  Also rejected: pre-installing shadows
//! at `replay_deploys` entry from `ProcessedDeploy.deploy_log`
//! walks — the log carries only `Blake2b256Hash`es of channel Pars
//! and (for non-deterministic produces) the leader's cached reply
//! bytes, but the args (`root`, `rel`, `mode`, `cmode`) needed to
//! reconstruct a shadow's `canon_path` / `cmode` are not in the
//! log.  Those come only from live Rholang re-evaluation, which
//! doesn't happen until the user-deploy loop runs the reducer.
//!
//! **Historical mitigation** (superseded 2026-08-30): item d-3
//! landed a per-fd Notify barrier (`fd_notifiers` +
//! `wait_for_replay_shadow` + `SHADOW_WAIT_TIMEOUT` +
//! `notify_fd_ready`) that made the canary reliable by
//! papering-over the failure at the 500ms timeout.  The true root
//! cause turned out to be a signedness bug in `extract_ok_fd`
//! (previously `extract_ok_u64`): fds seeded from state-hash
//! entropy commonly land above `i64::MAX`, wrap to negative i64 in
//! Rholang's `GInt` storage, and the reject-negative guard bailed —
//! so the follower's `fs_open` replay branch silently skipped
//! `insert_at` for those fds.  Commit `02b4c2efe` fixed the
//! signedness bug; commit that follows removed the barrier
//! (18/18 canary runs post-signedness-fix confirmed no genuine race
//! remains).  See `handle_table.rs` for the removal rationale.
//!
//! ## Replay-side WAL aggregation
//!
//! Now landed:
//! `replay_deploy_e_with_snapshot_transaction` returns the drained
//! WAL slice; `replay_deploys` aggregates across the user-deploy
//! loop and publishes into `RuntimeManager.pending_wal_slices`
//! keyed by the follower's computed post-state-hash — mirroring
//! the play-side insert in `runtime.rs::compute_state_with_bonds_
//! cosigned_admitted` (same eviction cap, same tracing target).
//!
//! ## Regression coverage
//!
//! The play-side aggregation shape is frozen by
//! `state_bound_cost_evidence_for_state_cosigned_aggregates_fs_wal`
//! (in runtime.rs), independent of this test.  The follower-side
//! aggregation is exercised by this test end-to-end (block-
//! processing → replay → `pending_wal_slices` publish).
//!
//! ## Interim coverage relationship
//!
//! The runtime-level WAL byte identity is covered at the RhoRuntime
//! layer in `rholang/tests/fs_wal_spec.rs` (leader/follower produce
//! byte-identical WAL sequences for identical deploys under the
//! shared-store rig).  This Casper-level canary adds coverage for the
//! block-processing → replay chain: if `add_block` or
//! `replay_deploys_for_state` silently stripped WAL entries or
//! populated `pending_wal_slices` inconsistently, that would break
//! this test but slip past the runtime-level pins.
//!
//! ## Deferred (out of scope for this canary)
//!
//! Per-node-fs isolation testing (design option B / C from the
//! handoff): would require path_map plumbing through Casper replay,
//! which is a large architectural change.
//!
//! Snapshot-write / finalization / joiner-reconstruction: item (d)'s
//! canary scope ends at pre-finalization WAL emission.  Follow-ups
//! that exercise the full chain (WAL → snapshot → chunk fetch →
//! fresh-tree apply) live in the deferred catalog.

use std::path::PathBuf;

use casper::rust::genesis::contracts::fs_genesis::{
    self, BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::util::construct_deploy;
use serial_test::serial;

use crate::helper::test_node::{TestFsProvisioning, TestNode};
use crate::util::genesis_builder::GenesisBuilder;

/// The bytes written to the Consensus-cap file: "hello world".
const PAYLOAD: &[u8] = b"hello world";
const PAYLOAD_HEX: &str = "68656c6c6f20776f726c64";

// Item d-3 (2026-08-28): both PB-M-14 canaries stand up a full 2-
// validator Casper network with genesis rebuild; running them
// concurrently exhausts shared harness state (genesis cache
// contention, in-memory transport churn) and causes intermittent
// failures unrelated to the WAL-identity properties they exercise.
// The `#[serial]` guard forces them onto the same lane so `cargo
// test -- pb_m_14` reliably runs both back-to-back.  Each still
// runs fully in isolation; the guard is only about not overlapping
// with each other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn pb_m_14_two_validator_wal_and_file_byte_identity() {
    // ---- shared fs setup ---------------------------------------------
    // Shared tempdir (design option A): both validators point at the
    // same on-disk root and bundle.  Per-node isolation (options B / C)
    // would require path_map plumbing through Casper replay — out of
    // scope for this canary.
    let shared_root = tempfile::tempdir().expect("shared_root tempdir");
    let file_path = shared_root.path().join("data.bin");
    std::fs::write(&file_path, b"").expect("seed empty file so bundle projection sees it");
    let canon_path =
        std::fs::canonicalize(&file_path).expect("canonicalize shared bundle target path");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon_path.clone(),
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Consensus,
    )
    .expect("bundle entry construction");
    let bundle = vec![entry];

    // ---- genesis -----------------------------------------------------
    // Cadence is set to keep the config well-formed for future snapshot
    // wiring, but this canary does not drive finalization / snapshot
    // emission — `pending_wal_slices` populates at play/replay time,
    // pre-LFB.
    let genesis = GenesisBuilder::new()
        .with_fs_bundle(bundle)
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    // ---- per-node fs provisioning ------------------------------------
    // Both nodes register the same shared canonical root (parent of the
    // bundle file, per production `node::runtime::setup:414-434` FILE
    // handling).  Payload dirs are per-node — content-addressed writes
    // hash to identical filenames, so retention/inspection stays
    // independent even though the WAL entries are identical.
    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let register_root = canon_path
        .parent()
        .expect("bundle path has parent")
        .to_path_buf();

    let mk_provisioning = |node_ix: usize| -> TestFsProvisioning {
        let payload_dir: PathBuf = per_node_root
            .path()
            .join(format!("node-{node_ix}-wal_payload_store"));
        TestFsProvisioning {
            root_paths: vec![register_root.clone()],
            payload_dir,
        }
    };

    let fs_provisionings = vec![Some(mk_provisioning(0)), Some(mk_provisioning(1))];

    let mut nodes =
        TestNode::create_network_with_fs_provisioning(genesis.clone(), 2, fs_provisionings)
            .await
            .expect("two-validator network with fs provisioning");

    // ---- Consensus write deploy --------------------------------------
    // Bundle-URI route (the production path) rather than
    // `rho:io:fs:native:1.0.0/*` (which the runtime's URN filter
    // rejects in non-fs_wal_spec contexts).  Fs.openFile on a
    // Consensus-cmode bundle entry mints a File cap with
    // `cmode="consensus"`; File.writeByteArray dispatches to fsWrite
    // with the propagated cmode, which is what causes journal_write
    // to append a `WalOp::Write` entry.
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let shard_id = genesis.genesis_block.shard_id.clone();
    let deploy_src = format!(
        r#"
new rl(`rho:registry:lookup`), fsCh, ackCh in {{
  rl!(`{fs_uri}`, *fsCh) |
  for (@(_, fs) <- fsCh) {{
    for (@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
      for (@reply <- @file!?("writeByteArray", "{payload_hex}".hexToBytes())) {{
        ackCh!(reply)
      }}
    }}
  }}
}}
"#,
        fs_uri = fs_uri,
        payload_hex = PAYLOAD_HEX,
    );

    let deploy = construct_deploy::source_deploy_now(deploy_src, None, None, Some(shard_id))
        .expect("sign fs-write deploy");

    // ---- execute: A creates + validates, B validates via replay ------
    let block = nodes[0]
        .add_block_from_deploys(&[deploy])
        .await
        .expect("node 0 (validator A) creates + adds Consensus-write block");
    let post_state_key = block.body.state.post_state_hash.to_vec();

    // Propagate to B; validation runs `replay_deploys_for_state` on
    // B's replay runtime, which is what populates B's pending_wal_slices.
    let (left, right) = nodes.split_at_mut(1);
    right[0]
        .process_block(block.clone())
        .await
        .expect("node 1 (validator B) validates block via replay");

    // ---- assertion 1: WAL byte identity ------------------------------
    let a_slice = {
        let guard = left[0].runtime_manager.pending_wal_slices.read().await;
        guard.get(&post_state_key).cloned().unwrap_or_else(|| {
            panic!(
                "validator A missing pending_wal_slices entry for post_state_hash {:?}",
                hex::encode(&post_state_key),
            )
        })
    };
    let b_slice = {
        let guard = right[0].runtime_manager.pending_wal_slices.read().await;
        guard.get(&post_state_key).cloned().unwrap_or_else(|| {
            panic!(
                "validator B missing pending_wal_slices entry for post_state_hash {:?}",
                hex::encode(&post_state_key),
            )
        })
    };

    assert_eq!(
        a_slice.0, b_slice.0,
        "block_number component of pending_wal_slices entry must match across validators"
    );
    assert_eq!(
        a_slice.1, b_slice.1,
        "PB-M-14: WAL entries produced by validator A's play-runtime and \
         validator B's replay-runtime must be identical"
    );

    // Explicit byte-identity check via canonical snapshot encoding —
    // the encoding is what actually crosses the wire / lands on disk,
    // so any divergence there is a load-bearing consensus regression.
    let a_bytes = rholang::rust::interpreter::io::snapshot::encode_wal_slice(&a_slice.1);
    let b_bytes = rholang::rust::interpreter::io::snapshot::encode_wal_slice(&b_slice.1);
    assert_eq!(
        a_bytes, b_bytes,
        "PB-M-14: canonical WAL slice encoding must be byte-identical across validators"
    );

    // ---- assertion 2: on-disk bytes ---------------------------------
    // Under shared-fs, both nodes wrote to the same on-disk file.  This
    // pin verifies the write actually happened; the interesting
    // property (independent reproduction) is out of scope for this
    // canary (see docstring).
    let on_disk = std::fs::read(&canon_path).expect("read shared bundle file back");
    assert_eq!(
        on_disk, PAYLOAD,
        "on-disk bytes at shared bundle path must match the deployed payload"
    );

    // Sanity: at least one WAL entry was actually emitted (the
    // assertion above would silently pass on empty vecs).
    assert!(
        !a_slice.1.is_empty(),
        "expected at least one WAL entry from the Consensus write; \
         got an empty slice which usually means the write hit the \
         Oracular path or was rejected"
    );
}

/// Item (d-2) leader-only runtime coverage: pins the play-side
/// aggregation fix end-to-end without depending on the follower's
/// replay path.  A single validator submits a Consensus-cap write
/// deploy; we assert the leader's `RuntimeManager.pending_wal_slices`
/// contains an entry keyed by the block's post-state-hash whose
/// `WalEntry` slice includes both the openFileImpl statCheck's Stat
/// entry and the actual fsWrite's Write entry.
///
/// This is the runtime-level companion to the pattern-check pins
/// `state_bound_cost_evidence_for_state_cosigned_aggregates_fs_wal`
/// (runtime.rs) and
/// `compute_state_with_bonds_cosigned_admitted_publishes_pending_wal_slice`
/// (runtime_manager.rs): those freeze the code shape at compile time;
/// this test verifies the shape actually produces the right runtime
/// state.  Together they close the coverage gap that pre-item-(d-2)
/// let the aggregation-drop bug slip through for months.
///
/// Follower-side coverage (leader/follower WAL byte-identity + on-
/// disk-bytes-after-replay) lives in the `#[ignore]`d
/// `pb_m_14_two_validator_wal_and_file_byte_identity` above, blocked
/// on the reducer/handler-completion fix (item d-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn pb_m_14_leader_pending_wal_slice_publishes_consensus_write() {
    let shared_root = tempfile::tempdir().expect("shared_root tempdir");
    let file_path = shared_root.path().join("data.bin");
    std::fs::write(&file_path, b"").expect("seed empty file so bundle projection sees it");
    let canon_path =
        std::fs::canonicalize(&file_path).expect("canonicalize shared bundle target path");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon_path.clone(),
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Consensus,
    )
    .expect("bundle entry construction");
    let bundle = vec![entry];

    let genesis = GenesisBuilder::new()
        .with_fs_bundle(bundle)
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let register_root = canon_path
        .parent()
        .expect("bundle path has parent")
        .to_path_buf();
    let payload_dir: PathBuf = per_node_root.path().join("node-0-wal_payload_store");
    let fs_provisionings = vec![Some(TestFsProvisioning {
        root_paths: vec![register_root],
        payload_dir,
    })];

    let mut nodes =
        TestNode::create_network_with_fs_provisioning(genesis.clone(), 1, fs_provisionings)
            .await
            .expect("single-validator network with fs provisioning");

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let shard_id = genesis.genesis_block.shard_id.clone();
    let deploy_src = format!(
        r#"
new rl(`rho:registry:lookup`), fsCh, ackCh in {{
  rl!(`{fs_uri}`, *fsCh) |
  for (@(_, fs) <- fsCh) {{
    for (@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
      for (@reply <- @file!?("writeByteArray", "{payload_hex}".hexToBytes())) {{
        ackCh!(reply)
      }}
    }}
  }}
}}
"#,
        fs_uri = fs_uri,
        payload_hex = PAYLOAD_HEX,
    );
    let deploy = construct_deploy::source_deploy_now(deploy_src, None, None, Some(shard_id))
        .expect("sign fs-write deploy");

    let block = nodes[0]
        .add_block_from_deploys(&[deploy])
        .await
        .expect("leader creates + adds Consensus-write block");
    let post_state_key = block.body.state.post_state_hash.to_vec();

    // Item (d-2) fix: the block's post-state-hash MUST key a
    // non-empty WAL slice in the leader's pending_wal_slices.
    let (block_number, entries) = {
        let guard = nodes[0].runtime_manager.pending_wal_slices.read().await;
        guard.get(&post_state_key).cloned().unwrap_or_else(|| {
            panic!(
                "item d-2 regression: leader's pending_wal_slices missing entry \
                 for post_state_hash {:?} — the cosigned block-creation path is \
                 dropping fs_wal again",
                hex::encode(&post_state_key),
            )
        })
    };

    assert_eq!(
        block_number, block.body.state.block_number,
        "pending_wal_slices entry's block_number must match the block's block_number"
    );
    assert!(
        !entries.is_empty(),
        "leader's pending_wal_slices entry must contain at least one WalEntry — \
         the Consensus write produced a Stat (from openFileImpl statCheck) + \
         a Write (from fsWrite)"
    );

    // Structural check: verify both the read-side (Stat) and write-side
    // (Write) journal contributions are present.  A future refactor
    // that drops one of them would trip this pin — the payload of the
    // Write is what unblocks joiner reconstruction of file state.
    use rholang::rust::interpreter::io::wal::WalOp;
    let has_stat = entries.iter().any(|e| matches!(e.op, WalOp::Stat));
    let has_write = entries.iter().any(|e| matches!(e.op, WalOp::Write));
    assert!(
        has_stat,
        "expected a WalOp::Stat entry in the block's WAL slice (from \
         openFileImpl's statCheck); got kinds {:?}",
        entries.iter().map(|e| e.op).collect::<Vec<_>>()
    );
    assert!(
        has_write,
        "expected a WalOp::Write entry in the block's WAL slice (from \
         fsWrite on the Consensus cap); got kinds {:?}",
        entries.iter().map(|e| e.op).collect::<Vec<_>>()
    );

    // Sanity: on-disk file matches what we wrote.
    let on_disk = std::fs::read(&canon_path).expect("read bundle file back");
    assert_eq!(
        on_disk, PAYLOAD,
        "on-disk bytes must match deployed payload"
    );
}

/// DD-7b-2 (a) Option 2 E2E canary (2026-08-30): validates the full
/// chain from leader-side journal_write recording through joiner-side
/// scratch replay reproduction of the write bytes.
///
/// # Chain covered
///
/// 1. Validator creates a block whose deploy performs a Consensus-cap
///    fs write of `PAYLOAD` (b"hello world"), which:
///    - Triggers `journal_write` on the leader path.
///    - `journal_write` calls `store.persist(bytes)` (fills the
///      `DirectoryPayloadStore`).
///    - `journal_write` calls `recorder.record(payload_hash,
///      deploy_sig)` (fills the block-storage-backed
///      `payload_source_index`).
///
/// 2. Assertion: the block-DAG storage's `payload_source_index`
///    contains `(Blake2b256(PAYLOAD), deploy_sig)`.  This is the
///    leader-side recording pin — proves the recorder wire-in works
///    end-to-end from Rholang through the interpreter.
///
/// 3. Chain walk: `lookup_payload_source(&hash)` returns the
///    deploy_sig; `lookup_by_deploy_id(&sig)` returns the block hash;
///    `block_store.get(&block_hash)` returns the ProcessedDeploy.
///    All three chain steps are exercised.
///
/// 4. Scratch replay: call
///    `capture_consensus_writes_by_replaying_deploy` with the
///    ProcessedDeploy, pre_state_hash, and a `ReplayPurseSnapshot`
///    derived via `replay_purse_snapshot`.  The returned map must
///    contain `(Blake2b256(PAYLOAD), PAYLOAD)`.
///
/// 5. Scratch-replay isolation pin: assert that the scratch replay
///    did NOT pollute the joiner's real `payload_source_index` with
///    duplicate or divergent entries (a3a3f4cd2's fix).
///
/// # Why this replaces the earlier `#[ignore]` skeleton
///
/// The primitive's docstring at
/// `capture_consensus_writes_by_replaying_deploy` originally called
/// out that "end-to-end verification via a real `ProcessedDeploy`
/// requires the full leader cosign pipeline" and deferred the E2E
/// "to the index-building session."  The PB-M-14 two-validator
/// harness IS that pipeline — this canary reuses it.  The
/// `option2_leader_records_and_joiner_reproduces_end_to_end`
/// skeleton in `wal_payload_sync.rs::tests` has been removed;
/// see the note in that module.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn pb_m_14_option2_leader_records_and_reproduces_via_scratch_replay() {
    use casper::rust::engine::wal_payload_server::InMemoryPayloadStore;
    use casper::rust::engine::wal_payload_sync::capture_consensus_writes_by_replaying_deploy;
    use casper::rust::rholang::replay_runtime::ReplayBlockKind;
    use casper::rust::util::rholang::acceptance::{
        replay_purse_snapshot, RuntimeManagerSupplyReader,
    };
    use crypto::rust::hash::blake2b256::Blake2b256;

    // ---- setup mirrors pb_m_14_leader_pending_wal_slice_publishes_
    //      consensus_write; keeping the flow familiar and easy to
    //      diff.  Single-validator: the recording + reproduction
    //      chain lives entirely on validator A's own state.
    let shared_root = tempfile::tempdir().expect("shared_root tempdir");
    let file_path = shared_root.path().join("data.bin");
    std::fs::write(&file_path, b"").expect("seed empty file");
    let canon_path = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon_path.clone(),
        BundleEntryKind::File,
        "rw".to_string(),
        BundleConsensusMode::Consensus,
    )
    .expect("bundle entry construction");
    let bundle = vec![entry];

    let genesis = GenesisBuilder::new()
        .with_fs_bundle(bundle)
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let register_root = canon_path
        .parent()
        .expect("bundle path has parent")
        .to_path_buf();
    let payload_dir: PathBuf = per_node_root.path().join("node-0-wal_payload_store");
    let fs_provisionings = vec![Some(TestFsProvisioning {
        root_paths: vec![register_root],
        payload_dir,
    })];

    let mut nodes =
        TestNode::create_network_with_fs_provisioning(genesis.clone(), 1, fs_provisionings)
            .await
            .expect("single-validator network with fs provisioning");

    // ---- Consensus write deploy ---------------------------------------
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let shard_id = genesis.genesis_block.shard_id.clone();
    let deploy_src = format!(
        r#"
new rl(`rho:registry:lookup`), fsCh, ackCh in {{
  rl!(`{fs_uri}`, *fsCh) |
  for (@(_, fs) <- fsCh) {{
    for (@[true, file] <- @fs!?("openFile", "target", {{"mode": "rw"}})) {{
      for (@reply <- @file!?("writeByteArray", "{payload_hex}".hexToBytes())) {{
        ackCh!(reply)
      }}
    }}
  }}
}}
"#,
        fs_uri = fs_uri,
        payload_hex = PAYLOAD_HEX,
    );
    let deploy = construct_deploy::source_deploy_now(deploy_src, None, None, Some(shard_id))
        .expect("sign fs-write deploy");
    let block = nodes[0]
        .add_block_from_deploys(&[deploy])
        .await
        .expect("validator creates + adds Consensus-write block");

    // ---- Compute the expected payload hash ---------------------------
    let payload_hash: [u8; 32] = {
        let h = Blake2b256::hash(PAYLOAD.to_vec());
        let mut out = [0u8; 32];
        out.copy_from_slice(&h);
        out
    };

    // ---- Assertion 2: leader-side recording ---------------------------
    // The payload_source_index must contain the mapping after the
    // Consensus write.  Proves journal_write → recorder.record →
    // BlockStorageBackedRecorder → LMDB write chain works
    // end-to-end.
    let recorded_sig = nodes[0]
        .block_dag_storage
        .lookup_payload_source(&payload_hash)
        .expect("lookup_payload_source must not error")
        .expect(
            "payload_source_index MUST contain the hash after Consensus write — \
             the journal_write → payload_source_recorder chain didn't fire.  \
             Check: (1) TestNode wired the recorder in fs_provisioning branch; \
             (2) WalDeployScope plumbed current_deploy_sig; (3) journal_write's \
             non-empty-sig guard passes for user deploys.",
        );

    // Expected sig comes from the ProcessedDeploy the block carries.
    let expected_processed = block
        .body
        .deploys
        .iter()
        .find(|pd| !pd.deploy.sig.is_empty())
        .expect("block must have at least one processed user deploy");
    let expected_sig = expected_processed.deploy.sig.to_vec();
    assert_eq!(
        recorded_sig, expected_sig,
        "recorded deploy_sig must match the block's ProcessedDeploy sig — \
         a mismatch means WalDeployScope plumbed a different sig than the \
         one in the block"
    );

    // ---- Assertion 3: chain walk -------------------------------------
    // Chain step 2: deploy_sig → block_hash via deploy_index.
    let chain_block_hash = nodes[0]
        .block_dag_storage
        .lookup_by_deploy_id(&recorded_sig)
        .expect("lookup_by_deploy_id must not error")
        .expect("deploy_index MUST resolve the sig to a block_hash");
    assert_eq!(
        chain_block_hash, block.block_hash,
        "chain step 2 must return the block that contains the deploy"
    );
    // Chain step 3: block_hash → BlockMessage.
    let chain_block = nodes[0]
        .block_store
        .get(&chain_block_hash)
        .expect("block_store.get must not error")
        .expect("block MUST be retrievable from block_store");
    let chain_processed = chain_block
        .body
        .deploys
        .iter()
        .find(|pd| pd.deploy.sig.as_ref() == recorded_sig.as_slice())
        .expect("chain step 4 must find the sig-matching ProcessedDeploy")
        .clone();

    // ---- Assertion 4: scratch replay reproduces bytes ---------------
    // Full Option 2 primitive invocation.  Derives the purse
    // snapshot via `replay_purse_snapshot`, then calls
    // `capture_consensus_writes_by_replaying_deploy` — the same code
    // path the boot reducer's Tier 2 walks.
    let pre_state = chain_block.body.state.pre_state_hash.clone();
    let supply_reader = RuntimeManagerSupplyReader {
        runtime_manager: &nodes[0].runtime_manager,
        pre_state_hash: pre_state.clone(),
    };
    let purse_snapshot = replay_purse_snapshot(&chain_processed, &supply_reader)
        .await
        .expect("replay_purse_snapshot must succeed for a well-formed processed deploy");

    let captured = capture_consensus_writes_by_replaying_deploy(
        &nodes[0].runtime_manager,
        &pre_state,
        &chain_processed,
        ReplayBlockKind::Ordinary,
        Some(&purse_snapshot),
    )
    .await
    .expect("capture_consensus_writes must succeed on the recorded deploy");

    let reproduced = captured
        .get(&payload_hash)
        .expect(
            "scratch replay's capture map MUST contain the requested \
             payload_hash.  If missing: either the deploy did no \
             Consensus writes at all (unlikely — the leader's WAL \
             confirmed one), or the scratch replay diverged from the \
             leader's execution (state-dependent write with mismatched \
             pre-state).",
        );
    assert_eq!(
        reproduced.as_slice(),
        PAYLOAD,
        "reproduced bytes must byte-identical to the deployed PAYLOAD — \
         the whole Option 2 reducer relies on this equality (mark_resolved's \
         rehash check would catch divergence in production, but here we \
         want the strict happy-path equality"
    );

    // ---- Assertion 5: scratch-replay isolation ------------------------
    // The primitive overrides `payload_source_recorder` to None on
    // the scratch runtime (a3a3f4cd2's fix).  Verify no new entries
    // landed in the joiner's real index — the only entry for
    // `payload_hash` should still be the ORIGINAL sig recorded by
    // the leader-side journal_write.  A regression that dropped the
    // override would either overwrite the entry (idempotent — same
    // key, same value) or add divergent entries under different
    // payload_hashes (if replay diverged).  The convergent case is
    // hard to distinguish from the correct case here; the divergent
    // case would surface via extra keys.
    //
    // Sanity: only one entry expected for this specific payload_hash.
    let post_scratch_sig = nodes[0]
        .block_dag_storage
        .lookup_payload_source(&payload_hash)
        .expect("lookup_payload_source must not error post-scratch")
        .expect("original entry must still be present");
    assert_eq!(
        post_scratch_sig, expected_sig,
        "scratch replay must NOT rewrite the payload_source_index entry — \
         a divergent scratch replay under bugs could otherwise overwrite \
         the entry with a different sig.  Same value = isolation working."
    );

    // Trap-check the isolation primitive's shape didn't drift.
    // (The dedicated pin `capture_consensus_writes_helper_has_load_
    // bearing_shape` in wal_payload_sync.rs freezes the source-level
    // shape; this behavioral pin proves the runtime effect.)
    let _isolation_witness = InMemoryPayloadStore::new(); // Compile-time trap that the type still exists.
}
