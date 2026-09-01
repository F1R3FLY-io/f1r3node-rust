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

use crate::helper::test_node::{project_bundle_per_validator, TestFsProvisioning, TestNode};
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
    // ---- fs setup (Shape A per-validator) -----------------------------
    // Phase 0 Stage 2 (2026-08-31): shared-fs (D3-inconsistent) was
    // reworked to per-validator subdirs.  Each validator gets its own
    // `<base>/validator-<ix>/bundle/target` seeded via
    // `project_bundle_per_validator` from the operator's stage bytes,
    // and the composed Rholang source uses `/@bundle/target` as the
    // logical canonRoot (validator-independent → genesis-hash stable).
    // Each validator's `RootIdentityRegistry` remaps `/@bundle` to its
    // own subdir at `resolve_or_identity` time.
    let stage_dir = tempfile::tempdir().expect("operator stage tempdir");
    let stage_file = stage_dir.path().join("target");
    std::fs::write(&stage_file, b"").expect("seed empty file at stage source");
    let canon_path = std::fs::canonicalize(&stage_file).expect("canonicalize stage source");

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
        .with_fs_bundle(bundle.clone())
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    // ---- per-validator projection ------------------------------------
    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let projections = project_bundle_per_validator(
        &bundle,
        2,
        per_node_root.path(),
        "wal_payload_store",
    )
    .expect("per-validator bundle projection");
    // Keep subdir handles for post-execution on-disk assertions.
    let leader_subdir = projections[0].subdir.clone();
    let follower_subdir = projections[1].subdir.clone();
    let fs_provisionings: Vec<Option<TestFsProvisioning>> = projections
        .into_iter()
        .map(|p| Some(p.provisioning))
        .collect();

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

    // ---- assertion 2: on-disk bytes (Shape A per-validator) ---------
    // Leader wrote through the play path — its own copy at
    // `<leader_subdir>/target` holds PAYLOAD.  Follower's on-disk
    // file MIXED-STATE per-op split as of Phase 1 (2026-09-01):
    //
    //   - The openFileImpl `statCheck` on the follower NOW does a
    //     real fs_stat re-execute against `<follower_subdir>/target`
    //     (its own copy of the 0-byte projection-seeded file) and
    //     verifies the hash matches the leader's cached statCheck
    //     reply (also 0 bytes at leader-statCheck time, since
    //     statCheck runs BEFORE the write).  Both replies agree →
    //     Success.  See `fs_stat_reexecute_detects_divergence` in
    //     `rholang/tests/fs_wal_spec.rs` for the divergence-side pin.
    //
    //   - The `fsWrite` on the follower still consumes the leader's
    //     cached reply and skips the actual `libc::write` syscall
    //     (Phase 3's fs_write re-execute is a separate slice per
    //     D2 = deploy source re-evaluation for bytes — reducer bytes
    //     plumbing is out of scope for Phase 1).  So the follower's
    //     own copy of `target` stays at the projection-seeded empty
    //     bytes.
    //
    // When Phase 3 lands the follower's fs_write re-execute, this
    // assertion flips to `== PAYLOAD` (the follower will run
    // `libc::write(bytes)` against its own subdir and produce a
    // Success verification).  Until then, the "== b\"\"" assertion
    // pins the current per-op split and will surface any accidental
    // Phase-3-adjacent scope creep.  The operator's stage source is
    // untouched (projection was a copy).
    let leader_target = leader_subdir.join("target");
    let leader_on_disk = std::fs::read(&leader_target).expect("read leader's own target");
    assert_eq!(
        leader_on_disk, PAYLOAD,
        "leader's per-validator on-disk copy must contain the deployed payload"
    );

    let follower_target = follower_subdir.join("target");
    let follower_on_disk = std::fs::read(&follower_target)
        .expect("read follower's own target (empty until Phase 3 fs_write re-execute lands)");
    assert_eq!(
        follower_on_disk,
        b"",
        "Phase-1-through-Phase-2 EXPECTATION: follower's fs_stat is_replay \
         branch NOW re-executes + verifies (statCheck agrees, Success), but \
         fs_write is_replay still consumes cached reply (no `libc::write` \
         re-execute yet).  Phase 3's fs_write re-execute will flip this to \
         `== PAYLOAD`; the flip is the completion signal for that phase."
    );

    let stage_source =
        std::fs::read(&canon_path).expect("operator stage source stays untouched");
    assert_eq!(
        stage_source,
        b"",
        "Shape A invariant: bundle-projection reads canon_path once at \
         provisioning and never mutates it.  A non-empty stage source here \
         means the projection helper accidentally mirrored writes back."
    );

    // Sanity: at least one WAL entry was actually emitted (the
    // byte-identity assertion above would silently pass on empty vecs).
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
    let stage_dir = tempfile::tempdir().expect("operator stage tempdir");
    let stage_file = stage_dir.path().join("target");
    std::fs::write(&stage_file, b"").expect("seed empty file at stage source");
    let canon_path = std::fs::canonicalize(&stage_file).expect("canonicalize stage source");

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
        .with_fs_bundle(bundle.clone())
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    // Shape A per-validator projection (single-validator case still
    // exercises the per-validator path — the harness uniformity is
    // load-bearing for the multi-validator canaries to share the same
    // setup helper).
    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let projections = project_bundle_per_validator(
        &bundle,
        1,
        per_node_root.path(),
        "wal_payload_store",
    )
    .expect("per-validator bundle projection");
    let leader_subdir = projections[0].subdir.clone();
    let fs_provisionings: Vec<Option<TestFsProvisioning>> = projections
        .into_iter()
        .map(|p| Some(p.provisioning))
        .collect();

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

    // Sanity: leader's own per-validator subdir contains the write.
    // (Shape A: the bundle emitted `/@bundle/target`, the registry
    // resolved that to `<leader_subdir>` at syscall time.)
    let leader_target = leader_subdir.join("target");
    let on_disk = std::fs::read(&leader_target).expect("read leader's target back");
    assert_eq!(
        on_disk, PAYLOAD,
        "leader's per-validator on-disk copy must contain the deployed payload"
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
    let stage_dir = tempfile::tempdir().expect("operator stage tempdir");
    let stage_file = stage_dir.path().join("target");
    std::fs::write(&stage_file, b"").expect("seed empty file at stage source");
    let canon_path = std::fs::canonicalize(&stage_file).expect("canonicalize stage source");

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
        .with_fs_bundle(bundle.clone())
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    // Phase 1 harness note (2026-09-01): this canary previously used
    // a one-call `create_network_with_per_validator_fs` wrapper (now
    // retired) and did not need subdir handles.  Under Phase 1's
    // fs_stat re-execute
    // + verify, the scratch-replay step below needs the fs at the
    // block's PRE-STATE (matching what the leader's play saw at
    // statCheck time).  In production this is naturally true — a
    // joiner boots with a fresh per-validator fs and only replays
    // deploys against pre-block state.  In this single-validator
    // canary the same runtime played the block AND runs the scratch
    // replay, so the leader's own play mutated its subdir (empty →
    // PAYLOAD) before the scratch replay runs.  We therefore need
    // the subdir handle so we can restore the file to its pre-play
    // (empty) state between `add_block_from_deploys` and
    // `capture_consensus_writes_by_replaying_deploy`.  Without the
    // restore, the scratch replay's fs_stat re-execute would fire
    // `FSERR_CONSENSUS_DIVERGENCE` on the mutated file → openFile
    // fails → fs_write never runs → capture returns empty →
    // `ReplayCostMismatch` from the deploy consuming less than the
    // recorded initial_cost.  See auto-memory
    // `fileio_wal_replay_verification_gap.md`.
    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let projections = project_bundle_per_validator(
        &bundle,
        1,
        per_node_root.path(),
        "wal_payload_store",
    )
    .expect("per-validator bundle projection");
    let validator_subdir = projections[0].subdir.clone();
    let fs_provisionings: Vec<Option<TestFsProvisioning>> = projections
        .into_iter()
        .map(|p| Some(p.provisioning))
        .collect();
    let mut nodes =
        TestNode::create_network_with_fs_provisioning(genesis.clone(), 1, fs_provisionings)
            .await
            .expect("single-validator network with per-validator fs provisioning");

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

    // ---- Restore the fs to pre-play state before scratch replay ----
    // Phase 1 (2026-09-01): the scratch replay below runs the deploy
    // on the SAME runtime the leader played it on, and under Phase 1
    // the fs_stat is_replay branch re-executes fstatat.  The leader's
    // play mutated `<validator_subdir>/target` from empty → PAYLOAD;
    // without a restore, scratch replay's statCheck would see PAYLOAD
    // while the RSpace-cached statCheck saw empty → divergence →
    // openFile fails → ReplayCostMismatch.  Truncate the file back
    // to its pre-play state (empty bytes) so the scratch replay's
    // fs_stat re-execute matches the leader's cached reply, exactly
    // as it would in production where the joiner boots with a fresh
    // per-validator fs.  See auto-memory
    // `fileio_wal_replay_verification_gap.md` for the design.
    std::fs::write(validator_subdir.join("target"), b"").expect(
        "restore validator subdir file to pre-play state before scratch replay — \
         Phase 1's fs_stat re-execute requires the fs to match what the leader's \
         play saw at cached-reply time",
    );

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

    let reproduced = captured.get(&payload_hash).expect(
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

/// DD-7b-2 (a) Tier 3 pseudo-joiner E2E canary (2026-08-30): drives
/// the boot subscriber's apply flow on a FRESH joiner whose local
/// PayloadLookup and block_dag_storage are both empty, forcing the
/// enumerator to fall through both Option 1 (Tier 1: local lookup)
/// and Option 2 (Tier 2: block-storage replay) into Tier 3 (peer
/// fetch).  Verifies the full chain:
///
///   producer records a Consensus write
///     → producer's `pending_wal_slices` populates
///     → snapshot round-trips through `write_snapshot` +
///       `read_snapshot_bytes` + `decode_wal_slice`
///     → joiner's `apply_wal_slice_after_fetch` misses Tier 1 (None
///       PayloadLookup) and Tier 2 (empty block_dag_storage), so
///       every unique payload hash is enqueued for peer fetch
///     → pre-injected "peer served" bytes on the retriever satisfy
///       the poll loop
///     → applier writes the payload bytes to disk under the
///       allowed_roots whitelist.
///
/// # Why "pseudo-joiner" not a full two-node cross-transport test
///
/// The handoff explicitly scoped this canary to option (b) — stand
/// up a fresh runtime + block-storage from scratch and drive the
/// boot subscriber directly.  A full two-node wire-protocol variant
/// (producer serves the payload via `HasWalPayloadRequest` /
/// `WalPayloadResponse`) is DD-7b-3 territory (block-processing
/// catch-up detector, larger scope).  The pseudo-joiner harness
/// covers the load-bearing "fresh joiner reconstructs file state
/// from a snapshot + peer bytes" property without needing the wire.
///
/// # Harness shape
///
/// Reuses `create_network_with_fs_provisioning(2, ...)` for cheap
/// cloneable RuntimeManager + BlockDagKeyValueStorage + KVBlockStore.
/// Node 0 is the producer (executes the deploy).  Node 1 is the
/// pseudo-joiner — deliberately NEVER receives the block via
/// `process_block`, so its block_dag_storage / block_store /
/// runtime_manager stay at genesis.  That empty state IS the Tier 2
/// miss the canary exercises.
///
/// # Assertions
///
/// 1. `BootApplyReport.enumerated.resolved_locally == 0` — no local
///    reducer hits.  A regression that either (a) started passing
///    a populated PayloadLookup by default, or (b) let Option 2
///    silently hit against genesis-only block storage, would trip
///    this pin.
///
/// 2. `BootApplyReport.enumerated.enqueued_for_fetch >= 1` — at
///    least one payload fell through to the peer-fetch tier.  This
///    is the canary's core positive — proves the enumerator's
///    fall-through logic works end-to-end.
///
/// 3. `sidecar_populated == enqueued_for_fetch` — every enqueued
///    hash resolved (via the pre-injected "peer bytes").  A
///    regression in `take_bytes` or `is_complete` would leave the
///    sidecar underpopulated and either fail here or bail earlier
///    with `PayloadFetchTimeout` / `MissingResolvedHash`.
///
/// 4. On-disk file bytes match the deployed PAYLOAD after boot —
///    the applier actually consumed the sidecar and wrote to disk.
///    The file is intentionally reset to empty bytes between
///    producer emission and joiner boot so that surviving PAYLOAD
///    at the end proves the applier's write happened (not the
///    producer's earlier write).
///
/// 5. Joiner's block_dag_storage.lookup_payload_source(hash) still
///    returns None after boot — the joiner never inserted the
///    producer's deploy, so its payload_source_index remains
///    empty.  If a future refactor caused the boot flow to
///    accidentally populate the joiner's index (e.g., by chaining
///    through the applier), this pin would trip.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn pb_m_14_pseudo_joiner_boots_via_peer_fetch_tier() {
    use std::sync::Arc;
    use std::time::Duration;

    use casper::rust::engine::wal_payload_retriever::WalPayloadRetriever;
    use casper::rust::engine::wal_payload_sync::{
        apply_wal_slice_after_fetch, Option2ReducerContext, WalPayloadSyncDriver,
    };
    use crypto::rust::hash::blake2b256::Blake2b256;
    use rholang::rust::interpreter::io::snapshot::{
        decode_wal_slice, read_snapshot_bytes, write_snapshot,
    };

    // ---- fs setup (Shape A per-validator, 2026-08-31) ---------------
    // Producer (node 0) and pseudo-joiner (node 1) each get their own
    // `<per_node_root>/validator-<ix>/bundle` subdir seeded from the
    // operator's stage source.  Under Shape A the bundle emits
    // `/@bundle/target` (validator-independent); each node's registry
    // remaps that to its own subdir.  Task 0.4 (2026-08-31) plumbed
    // the joiner's `RootIdentityRegistry` directly to
    // `apply_wal_slice_after_fetch`, so the applier resolves each WAL
    // entry's `/@bundle/target` to `<joiner_subdir>/target` via the
    // same registry-lookup path the reducer uses.
    let stage_dir = tempfile::tempdir().expect("operator stage tempdir");
    let stage_file = stage_dir.path().join("target");
    std::fs::write(&stage_file, b"").expect("seed empty file at stage source");
    let canon_path = std::fs::canonicalize(&stage_file).expect("canonicalize stage source");

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
        .with_fs_bundle(bundle.clone())
        .with_consensus_fs_snapshot_cadence(Some(1))
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis with fs bundle");

    let per_node_root = tempfile::tempdir().expect("per_node_root tempdir");
    let projections = project_bundle_per_validator(
        &bundle,
        2,
        per_node_root.path(),
        "wal_payload_store",
    )
    .expect("per-validator bundle projection");
    let joiner_subdir = projections[1].subdir.clone();
    let fs_provisionings: Vec<Option<TestFsProvisioning>> = projections
        .into_iter()
        .map(|p| Some(p.provisioning))
        .collect();

    let mut nodes =
        TestNode::create_network_with_fs_provisioning(genesis.clone(), 2, fs_provisionings)
            .await
            .expect("two-validator network with fs provisioning");

    // ---- Producer (node 0) executes Consensus write --------------------
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
        .expect("producer creates + adds Consensus-write block");
    let post_state_key = block.body.state.post_state_hash.to_vec();

    // ---- Producer's WAL slice — the input to the snapshot -------------
    let (_producer_block_number, wal_entries) = {
        let guard = nodes[0].runtime_manager.pending_wal_slices.read().await;
        guard.get(&post_state_key).cloned().unwrap_or_else(|| {
            panic!(
                "producer missing pending_wal_slices entry for post_state_hash {:?}",
                hex::encode(&post_state_key),
            )
        })
    };
    assert!(
        !wal_entries.is_empty(),
        "producer's WAL slice must be non-empty (Stat + Write from openFile + writeByteArray)"
    );

    // ---- Reset the joiner's on-disk file to prove the applier's write
    // Under Shape A per-validator subdirs, each node wrote to its OWN
    // copy.  The joiner's copy is at `<joiner_subdir>/target` — seeded
    // to empty at projection time, then (in this test) the joiner
    // never processed the producer's block, so its copy is still
    // untouched.  Reset defensively so a hypothetical future
    // wire-in that populates the joiner's disk pre-boot still exposes
    // a "the applier's write happened" pin.
    let joiner_target = joiner_subdir.join("target");
    std::fs::write(&joiner_target, b"").expect("reset joiner's on-disk file to empty");
    assert_eq!(
        std::fs::read(&joiner_target).unwrap(),
        b"",
        "post-reset joiner file must be empty before boot flow runs"
    );

    // ---- Snapshot round-trip through on-disk format --------------------
    // `write_snapshot` is the same primitive the leader's SnapshotWriter
    // invokes at LFB advance; `read_snapshot_bytes` + `decode_wal_slice`
    // is what the boot subscriber invokes on completion.  Round-tripping
    // through disk (not just passing the Vec<WalEntry> in-memory) proves
    // the encode/decode path handles a real producer's WAL entries.
    let snapshot_dir = tempfile::tempdir().expect("snapshot_dir tempdir");
    let (_snapshot_path, atomic_root, _merkle_root) =
        write_snapshot(snapshot_dir.path(), &wal_entries).expect("write_snapshot");
    let snapshot_bytes =
        read_snapshot_bytes(snapshot_dir.path(), &atomic_root).expect("read_snapshot_bytes");
    let decoded_wal = decode_wal_slice(&snapshot_bytes).expect("decode_wal_slice");
    assert_eq!(
        decoded_wal, wal_entries,
        "decode(encode(wal)) must round-trip byte-identically — a snapshot format \
         drift would surface here before the applier runs"
    );

    // Sanity: the decoded WAL contains at least one Write/WriteAt.
    // The boot flow itself handles observation-only entries (Stat,
    // Read, ...) — the enumerator skips them because their
    // `PayloadRef::Hash` is a consensus-verification target (follower
    // re-executes the syscall + rehashes + compares), NOT a peer-
    // fetch target.  See `WalOp::is_observation_only` in wal.rs and
    // the observation-op skip filter in `enumerate_and_enqueue_
    // payloads` + `apply_wal_slice_after_fetch` in wal_payload_sync.rs.
    use rholang::rust::interpreter::io::wal::WalOp;
    assert!(
        decoded_wal
            .iter()
            .any(|e| matches!(e.op, WalOp::Write | WalOp::WriteAt)),
        "decoded WAL must contain at least one Write/WriteAt (the Consensus \
         write from the deploy) — a missing mutation would defeat the canary"
    );

    // ---- Payload hash — one entry per unique payload_ref --------------
    // The Consensus write contributes one PayloadRef::Hash(Blake2b256(PAYLOAD)).
    let payload_hash: [u8; 32] = {
        let h = Blake2b256::hash(PAYLOAD.to_vec());
        let mut out = [0u8; 32];
        out.copy_from_slice(&h);
        out
    };

    // ---- Joiner-side driver + "peer served bytes" pre-injection -------
    // Fresh WalPayloadSyncDriver with no history of prior fetches.  We
    // pre-inject the payload bytes via `mark_resolved` BEFORE calling
    // the boot flow — this simulates the wire-protocol effect (peer
    // ships bytes in a WalPayloadResponse) without driving the full
    // request/response machinery.  The enumerator will still call
    // `enqueue_payload` for each Tier-3 miss (since payload_lookup =
    // None), but `enqueue`'s `or_insert` skips the entry we already
    // pre-populated with bytes: Some, so `is_complete` returns true
    // on the first poll and the applier runs immediately.
    let joiner_driver = Arc::new(WalPayloadSyncDriver::new(Arc::new(
        WalPayloadRetriever::new(),
    )));
    let injected = joiner_driver
        .retriever
        .mark_resolved(payload_hash, PAYLOAD.to_vec())
        .await;
    assert!(
        injected,
        "pre-injection must succeed — a false return means Blake2b256(PAYLOAD) \
         disagrees with the hash we computed, which would be a bug in this test"
    );

    // ---- Option 2 context uses joiner's (empty) block storage --------
    // Both `block_dag_storage` and `block_store` are TestNode's own —
    // node 1 never processed the producer's block, so its indices are
    // at genesis only.  Tier 2 will miss cleanly on `lookup_payload_
    // source(payload_hash)` returning Ok(None).
    let option2_ctx = Option2ReducerContext {
        block_storage: nodes[1].block_dag_storage.clone(),
        block_store: nodes[1].block_store.clone(),
        runtime_manager: Arc::new(nodes[1].runtime_manager.clone()),
    };

    // Sanity: confirm the joiner's block_dag_storage really is empty
    // for our target hash BEFORE the boot flow runs.  A regression in
    // `create_network_with_fs_provisioning` that accidentally shared
    // block storage across TestNodes would trip this pin.
    let pre_boot_lookup = nodes[1]
        .block_dag_storage
        .lookup_payload_source(&payload_hash)
        .expect("lookup_payload_source must not error");
    assert!(
        pre_boot_lookup.is_none(),
        "joiner's payload_source_index must be empty before boot — got {:?}",
        pre_boot_lookup.as_ref().map(hex::encode)
    );

    // ---- Drive the boot apply flow ------------------------------------
    // Task 0.4 (2026-08-31): the applier's path-resolution is now
    // registry-based.  Under Shape A the producer's WAL entries carry
    // the bundle-relative form `/@bundle/target` in the path field
    // (via `canonicalize_lexical(&root, &rel)` where `root` is the
    // UNRESOLVED Rholang canonRoot).  The joiner's applier joins each
    // entry.path to the joiner's own on-disk root via
    // `RootIdentityRegistry::resolve_wal_entry_path` — the same
    // resolver the per-validator runtime uses on the reducer side.
    // Passing `nodes[1].runtime_manager.root_id_registry` proves the
    // production wire-in path: the applier and the handler layer
    // consult the identical registry populated at TestNode setup.
    // `allowed_roots` mirrors the WAL entry path shape (bundle-
    // relative); the applier's check_path_allowed runs on the raw
    // entry.path before the registry rewrites for the syscall.
    // Tight timeout + poll interval because we pre-injected bytes;
    // the poll loop should exit on the first iteration.
    use casper::rust::genesis::contracts::fs_genesis::BUNDLE_ROOT_PREFIX;
    let joiner_registry = nodes[1].runtime_manager.root_id_registry.clone();
    let report = apply_wal_slice_after_fetch(
        Arc::clone(&joiner_driver),
        decoded_wal,
        joiner_registry,
        vec![PathBuf::from(BUNDLE_ROOT_PREFIX)],
        Duration::from_secs(5),
        Duration::from_millis(25),
        None, // Tier 1: no PayloadLookup → miss for every hash.
        Some(option2_ctx),
    )
    .await
    .expect("boot flow must succeed on the pseudo-joiner happy path");

    // ---- Assertion 1: Tier 1 + Tier 2 both missed --------------------
    assert_eq!(
        report.enumerated.resolved_locally, 0,
        "no local reducer hits expected — payload_lookup was None and \
         option2_ctx's block storage was empty.  A non-zero here means \
         either the reducer wired in a default lookup or Option 2's \
         chain resolved despite genesis-only block storage."
    );

    // ---- Assertion 2: Tier 3 fall-through fired ----------------------
    assert!(
        report.enumerated.enqueued_for_fetch >= 1,
        "expected at least one payload enqueued for peer fetch (Tier 3 \
         fall-through), got {}.  A zero here means either the WAL had no \
         PayloadRef::Hash entries (unlikely — the Consensus write always \
         produces one) or the enumerator short-circuited before enqueueing.",
        report.enumerated.enqueued_for_fetch,
    );

    // ---- Assertion 3: sidecar populated for every enqueued hash ------
    assert_eq!(
        report.sidecar_populated, report.enumerated.enqueued_for_fetch,
        "sidecar must contain bytes for every enqueued hash — the pre-\
         injected retriever state guarantees `is_complete` on the first \
         poll and `take_bytes` succeeds for every hash.  A mismatch \
         indicates a regression in the driver's take_bytes / is_complete \
         contract."
    );

    // ---- Assertion 4: on-disk bytes match PAYLOAD post-boot ----------
    // The applier's Write branch opens with `create: true, truncate:
    // false`, seeks to `entry.offset`, and writes the payload.  Since
    // we reset the joiner's file to empty above, the file's final
    // length equals `entry.offset + PAYLOAD.len()` — and offset is 0
    // for a fresh openFile with mode "rw" (no O_APPEND on the
    // Consensus path).  Under Shape A the applier's write lands at
    // `<joiner_subdir>/target` via the registry's Shape A resolver
    // (`resolve_wal_entry_path` — see Task 0.4).
    let on_disk =
        std::fs::read(&joiner_target).expect("read joiner's applied file back");
    assert_eq!(
        on_disk, PAYLOAD,
        "on-disk bytes after joiner boot must match PAYLOAD — the applier \
         restored the write via the peer-fetch tier"
    );

    // ---- Assertion 5: joiner's payload_source_index stays empty ------
    // The boot flow must not accidentally populate the joiner's index.
    // Option 2's scratch replay isolates via `share_payload_source_
    // recorder(None)` override; the applier itself never touches the
    // index.  A regression that chained the wrong direction would show
    // up as an extra entry here.
    let post_boot_lookup = nodes[1]
        .block_dag_storage
        .lookup_payload_source(&payload_hash)
        .expect("lookup_payload_source must not error post-boot");
    assert!(
        post_boot_lookup.is_none(),
        "joiner's payload_source_index must STAY empty after boot flow — \
         found unexpected entry {:?}.  A non-None here means either \
         Option 2's scratch replay leaked into the joiner's index (the \
         a3a3f4cd2 fix regressed) or the applier gained a side-effect \
         on the index.",
        post_boot_lookup.as_ref().map(hex::encode),
    );
}
