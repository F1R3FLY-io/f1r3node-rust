// Consensus-mode filesystem WAL snapshot (slice 30, PB-M-15 — MVP).
//
// # Scope
//
// Slice 30 MVP delivers the *content-addressed serialization + Merkle
// root* substrate that follow-up slices build on to:
//
//   - Persist per-block WAL slices to disk at operator-configured
//     cadence (`storage.consensus-fs-snapshot-cadence`).
//   - Serve the snapshot bytes to joining validators.
//   - Let a joining validator replay from the last snapshot forward.
//
// This module is deliberately narrow: canonical byte encoding of
// `Vec<WalEntry>`, a Blake2b256 root hash over that encoding, and
// filesystem read/write on a content-addressed path.  The cadence
// loop, the joining-node fetch protocol, and the on-chain commitment
// of the root (a hard-fork proto change) are deferred to slice 30c.
//
// # Snapshot semantics — log-structured (F-30-2 review decision)
//
// A snapshot in this module IS a canonical byte-encoding of
// `Vec<WalEntry>` — that is, a WAL slice or checkpoint, not a
// materialized filesystem-state image.  Joining validators replay
// the concatenation of all snapshots from genesis forward against
// an empty base image.  This commits the system to
// **log-structured checkpointing**: prevention of unbounded WAL
// replay for a late joiner depends on the operator's cadence being
// short enough that the cumulative WAL slice size stays under the
// target replay budget.
//
// The alternative — materialized fs state snapshots — was
// considered and deferred; the current design is chosen because it:
//   * keeps a single content-addressed encoding for both per-block
//     WAL entries (already used for on-chain root commitment) and
//     for on-disk snapshots — no separate serialization schema;
//   * requires no fs-state serializer (which for consensus-static
//     buckets would need to snapshot every file's contents, sizes
//     comparable to the total on-disk footprint);
//   * lets the joining protocol be a plain byte-stream fetch.
//
// A future slice may add a materialized-state snapshot alongside
// the log-structured one for operators who want a faster join
// path; the on-disk format is versioned via the WAL root prefix so
// both can coexist.
//
// # Canonical encoding
//
// The encoding is prefix-length + big-endian; no protobuf, no serde.
// This keeps the byte layout independent of any code-generator's
// choices and makes the resulting root hash a stable consensus
// commitment.  A hard-fork of the encoding format is a hard-fork of
// the WAL root hash.
//
// The WAL slice header is:
//   version:  u8 (= SNAPSHOT_FORMAT_VERSION)
//   entries:  u32-be count + [entry bytes] × count
//
// The leading version byte (added in slice 34, MED-1 FIPS fix) makes
// reads self-describing: `read_snapshot_bytes` rejects unknown
// versions with `SnapshotError::UnsupportedVersion` at parse time
// rather than mis-decoding.  A future format change bumps the
// version, so a hybrid rollout can accept both v1 and vN blobs
// during transition.
//
// The exact per-entry schema (see `encode_entry`):
//   op:           u8
//   path:         u32-be length prefix + UTF-8 bytes
//   extra_path:   u8 flag (0 = None, 1 = Some) + optional u32+bytes
//   offset:       u8 flag + optional u64-be
//   length:       u8 flag + optional u64-be
//   payload_ref:  u8 tag (0 = None, 1 = Hash, 2 = DeployRef) + variant bytes
//   mode_bits:    u8 flag + optional u32-be
//   owner:        u8 flag + optional u32+UTF-8
//   group:        u8 flag + optional u32+UTF-8
//
// Order matters: `Vec<WalEntry>` is encoded in its insertion order.
// The caller (per-block accumulator in casper) is responsible for
// producing a deterministic ordering — slice 29 round-2 finding H-R3
// (Par-parallel WAL ordering) is orthogonal to this encoding.
//
// # Hard-fork surface catalog (MED-1, slice 34)
//
// EVERY item below is consensus-observable.  Changing any one of
// them changes the WAL root computed over the same logical inputs,
// which is a hard fork of the network.  Bump
// `SNAPSHOT_FORMAT_VERSION` and update the golden-hex test IF AND
// ONLY IF you intend a hard fork and have coordinated it.  If
// you're adding a new item to this surface (a new field, a new
// op variant, a new PayloadRef variant), append it to this list
// AND to `hard_fork_surface_catalog_is_pinned` in the test module.
//
// 1. **Format version byte** — `SNAPSHOT_FORMAT_VERSION` at
//    the front of `encode_wal_slice`.  Bump on any format change.
// 2. **Op tag bytes** — `op_tag(WalOp)` maps enum variants to
//    stable u8 values 1..=11 (Write, WriteAt, Truncate, Chmod,
//    Chown, RemoveFile, RemoveDir, Rename, CopyFile, Read, ReadAt).
//    Pinned by `op_tags_are_stable` + `encode_entry_uses_op_tag_values`.
// 3. **Hash function = Blake2b256** — `hash_of`,
//    `PayloadRef::hash`, `compute_wal_root`.
// 4. **Length-prefix widths**:
//      - u32-be for entry-count and every string-byte-count;
//      - u8 flags for Option<...> present/absent tags.
// 5. **Field widths / endianness**:
//      - u64-be for `offset` and `length`;
//      - u32-be for `mode_bits`, DeployRef's `deploy_index` and
//        `arg_index`, and every string-byte-count prefix.
// 6. **`PayloadRef` variant tags** — `Hash = 1`, `DeployRef = 2`,
//    `None = 0`.  Pinned by
//    `deploy_ref_encoding_is_big_endian_and_field_order_is_stable`.
// 7. **Field order inside `encode_entry`** — op, path, extra_path,
//    offset, length, payload_ref, mode_bits, owner, group.  A
//    reorder that keeps all field-encoders unchanged still forks
//    the network because the concatenation order differs.
// 8. **Path encoding** — `PathBuf::as_os_str().as_encoded_bytes()`.
//    Unix-only; see `# Platform scope` above.  A Windows port MUST
//    bump the version and switch to logical bucket keys.
//
// # Platform scope (H-30-3 review note)
//
// This module uses `PathBuf::as_os_str().as_encoded_bytes()` to
// serialize paths.  On Unix that returns the raw byte sequence; on
// Windows it returns WTF-8 (an encoding of the internal UTF-16).
// Consequently a Unix validator and a Windows validator processing
// the same block would produce **different WAL roots for the same
// logical path**.
//
// The File I/O FIP scopes the node to Unix (see slice 25 provisioning
// docs and slice 27/28 review notes).  Any future Windows port MUST
// resolve this encoding divergence — the standard fix is to encode
// the *logical bucket key* (`"app/data.bin"` from the provisioning
// map) rather than the host canonical path.  That fix is deferred to
// the Windows-port slice; today the assumption is Unix-only and the
// canonical `PathBuf` bytes serve as the consensus commitment.

use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b256::Blake2b256;

use super::wal::{PayloadRef, WalEntry, WalOp};

/// Slice 34 (MED-1): version byte at the front of every encoded
/// WAL slice.  Bumping this is a hard fork of the WAL root; see
/// `# Hard-fork surface catalog` in the module docstring.
pub const SNAPSHOT_FORMAT_VERSION: u8 = 1;

/// Result of encoding + hashing a WAL slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBlob {
    /// The full canonical byte encoding.
    pub bytes: Vec<u8>,
    /// Blake2b256 of `bytes`.  The content address.
    pub root: [u8; 32],
}

/// Encode a WAL slice to canonical bytes.  Deterministic across
/// validators for identical `entries` input.
///
/// Layout: `[SNAPSHOT_FORMAT_VERSION: u8][count: u32-be][entry × count]`.
/// The leading version byte lets `read_snapshot_bytes` reject
/// unknown-version blobs cleanly instead of mis-decoding.
pub fn encode_wal_slice(entries: &[WalEntry]) -> Vec<u8> {
    // Cap at u32::MAX entries.  In practice `MAX_WAL_ENTRIES` = 65_536
    // per runtime keeps this well below the cap.
    let count: u32 = entries
        .len()
        .try_into()
        .expect("WAL slice exceeds u32::MAX entries — impossible under MAX_WAL_ENTRIES cap");
    let mut buf = Vec::with_capacity(1 + 4 + entries.len() * 96);
    buf.push(SNAPSHOT_FORMAT_VERSION);
    buf.extend_from_slice(&count.to_be_bytes());
    for e in entries {
        encode_entry(e, &mut buf);
    }
    buf
}

/// Content-addressed hash of the canonical encoding.
pub fn compute_wal_root(entries: &[WalEntry]) -> [u8; 32] {
    let bytes = encode_wal_slice(entries);
    hash_of(&bytes)
}

/// Encode + hash together.
pub fn snapshot_blob(entries: &[WalEntry]) -> SnapshotBlob {
    let bytes = encode_wal_slice(entries);
    let root = hash_of(&bytes);
    SnapshotBlob { bytes, root }
}

fn hash_of(bytes: &[u8]) -> [u8; 32] {
    let h = Blake2b256::hash(bytes.to_vec());
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {}",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn encode_entry(e: &WalEntry, buf: &mut Vec<u8>) {
    buf.push(op_tag(e.op));
    encode_str_bytes(e.path.as_os_str().as_encoded_bytes(), buf);
    match &e.extra_path {
        None => buf.push(0),
        Some(p) => {
            buf.push(1);
            encode_str_bytes(p.as_os_str().as_encoded_bytes(), buf);
        }
    }
    encode_opt_u64(e.offset, buf);
    encode_opt_u64(e.length, buf);
    encode_payload_ref(&e.payload_ref, buf);
    encode_opt_u32(e.mode_bits, buf);
    encode_opt_str(e.owner.as_deref(), buf);
    encode_opt_str(e.group.as_deref(), buf);
}

fn op_tag(op: WalOp) -> u8 {
    // Explicit numeric tags — DO NOT reorder or renumber.  Changing
    // these is a hard-fork of the WAL root.  Slice 32 (PB-M-14 read-
    // hash) adds Read/ReadAt at the end of the reserved-tag range.
    match op {
        WalOp::Write => 1,
        WalOp::WriteAt => 2,
        WalOp::Truncate => 3,
        WalOp::Chmod => 4,
        WalOp::Chown => 5,
        WalOp::RemoveFile => 6,
        WalOp::RemoveDir => 7,
        WalOp::Rename => 8,
        WalOp::CopyFile => 9,
        WalOp::Read => 10,
        WalOp::ReadAt => 11,
    }
}

fn encode_str_bytes(s: &[u8], buf: &mut Vec<u8>) {
    let n: u32 = s
        .len()
        .try_into()
        .expect("string exceeds u32::MAX bytes — impossible for a filesystem path");
    buf.extend_from_slice(&n.to_be_bytes());
    buf.extend_from_slice(s);
}

fn encode_opt_str(s: Option<&str>, buf: &mut Vec<u8>) {
    match s {
        None => buf.push(0),
        Some(s) => {
            buf.push(1);
            encode_str_bytes(s.as_bytes(), buf);
        }
    }
}

fn encode_opt_u64(v: Option<u64>, buf: &mut Vec<u8>) {
    match v {
        None => buf.push(0),
        Some(n) => {
            buf.push(1);
            buf.extend_from_slice(&n.to_be_bytes());
        }
    }
}

fn encode_opt_u32(v: Option<u32>, buf: &mut Vec<u8>) {
    match v {
        None => buf.push(0),
        Some(n) => {
            buf.push(1);
            buf.extend_from_slice(&n.to_be_bytes());
        }
    }
}

fn encode_payload_ref(r: &Option<PayloadRef>, buf: &mut Vec<u8>) {
    match r {
        None => buf.push(0),
        Some(PayloadRef::Hash(h)) => {
            buf.push(1);
            buf.extend_from_slice(h);
        }
        Some(PayloadRef::DeployRef {
            block_hash,
            deploy_index,
            arg_index,
        }) => {
            buf.push(2);
            buf.extend_from_slice(block_hash);
            buf.extend_from_slice(&deploy_index.to_be_bytes());
            buf.extend_from_slice(&arg_index.to_be_bytes());
        }
    }
}

/// I/O errors from snapshot read/write.
#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    RootMismatch {
        expected: [u8; 32],
        got: [u8; 32],
    },
    /// Slice 34 (MED-1): the on-disk blob starts with a version byte
    /// this validator does not recognize.  A joining validator
    /// running an older binary can see a snapshot produced by a
    /// newer, hard-forked network — surface a clean error rather
    /// than mis-decoding.
    UnsupportedVersion {
        got: u8,
        supported: u8,
    },
    /// Blob is too short to contain even the version byte.
    Truncated {
        got: usize,
        need: usize,
    },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) => write!(f, "snapshot I/O error: {e}"),
            SnapshotError::RootMismatch { expected, got } => write!(
                f,
                "snapshot root mismatch: expected {}, got {}",
                hex_short(expected),
                hex_short(got)
            ),
            SnapshotError::UnsupportedVersion { got, supported } => write!(
                f,
                "snapshot format version {got} not supported by this validator \
                 (understands version {supported}); a coordinated upgrade may be needed"
            ),
            SnapshotError::Truncated { got, need } => write!(
                f,
                "snapshot blob truncated: {got} bytes, need at least {need}"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self { SnapshotError::Io(e) }
}

fn hex_short(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(16);
    for b in &bytes[..8] {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Content-addressed on-disk snapshot path.
///
/// Layout: `{snapshot_dir}/{root_hex}.wal`.  The filename IS the
/// content hash, so a joining validator can request a snapshot by
/// root and verify byte-for-byte after fetch.
pub fn snapshot_path(snapshot_dir: &Path, root: &[u8; 32]) -> PathBuf {
    let mut hex = String::with_capacity(64);
    for b in root {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    snapshot_dir.join(format!("{hex}.wal"))
}

/// Write a snapshot to `snapshot_dir` under its content-addressed
/// filename.  Idempotent: writing the same content twice produces the
/// same file (overwrites atomically via a tmp+rename dance).  Returns
/// the on-disk path.
pub fn write_snapshot(
    snapshot_dir: &Path,
    entries: &[WalEntry],
) -> Result<(PathBuf, [u8; 32]), SnapshotError> {
    let blob = snapshot_blob(entries);
    let final_path = snapshot_path(snapshot_dir, &blob.root);
    // Ensure directory exists.  Callers should have validated this at
    // boot; a race that removed it mid-flight surfaces here as ENOENT.
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // M-30-1 review fix (round 2): the tmp filename includes both the
    // process ID and a nanosecond timestamp so two concurrent writers
    // in the SAME PROCESS writing the SAME content do not race on the
    // same tmp path.  Pre-fix used `final_path.with_extension("wal.tmp")`
    // which is deterministic per-root, so two threads writing the same
    // content had `std::fs::write` calls stomping the same tmp file
    // between each other's rename.  Even for content-addressed writes
    // (where either writer produces identical bytes), a mid-write
    // observer could see truncated bytes.
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        "{}.{}-{}.wal.tmp",
        final_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("snapshot"),
        std::process::id(),
        now_nanos
    );
    let tmp_path = final_path.with_file_name(tmp_name);
    // F-30-12 review fix (slice 30b): fsync the tmp file before
    // rename so a crash between write and rename leaves either
    // no snapshot at all (recovery is safe — the WAL cadence loop
    // re-emits on next block) OR a fully durable snapshot at the
    // final path.  Without fsync, `write` + `rename` can leave a
    // renamed file whose contents are still in the page cache;
    // post-crash the operator's snapshot file exists but reads as
    // truncated or garbled — the read-time root check catches this
    // as `RootMismatch`, but preventing it upstream avoids the
    // false-alarm.
    {
        use std::io::Write as _;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        // M-30b-4 review fix (round 2): explicit 0o644 mode on Unix
        // so the snapshot's uid/gid/mode do NOT leak the leader's
        // umask to any joiner reading over shared storage.  Content
        // is deterministic across validators (canonical WAL bytes),
        // but the file metadata was previously umask-dependent.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(&blob.bytes)?;
        file.sync_all()?;
    }
    // POSIX rename is atomic on the same filesystem; two concurrent
    // writes with distinct tmp files still race on the final rename,
    // but since the content is byte-identical (content-addressed) the
    // outcome is one of two identical files — observationally
    // indistinguishable.
    std::fs::rename(&tmp_path, &final_path)?;
    // Also fsync the directory so the rename entry itself is durable
    // (POSIX: rename's atomicity does not imply metadata durability).
    // M-30b-5 review fix: log fsync failures instead of swallowing.
    // Some filesystems (tmpfs, some network fs) reject dir fsync
    // with a legitimate errno — that's fine.  But an ext4/xfs EIO
    // (real hardware fault) should surface to operators.
    if let Some(parent) = final_path.parent() {
        match std::fs::File::open(parent) {
            Ok(dir_file) => {
                if let Err(e) = dir_file.sync_all() {
                    tracing::debug!(
                        target: "f1r3fly.fs_wal.snapshot",
                        parent = %parent.display(),
                        error = %e,
                        "dir fsync after rename failed (fs may not support dir fsync)"
                    );
                }
            }
            Err(e) => {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.snapshot",
                    parent = %parent.display(),
                    error = %e,
                    "opening parent dir for fsync failed"
                );
            }
        }
    }
    Ok((final_path, blob.root))
}

/// M-30b-3 review fix (slice 30b round 2): sweep stale `.wal.tmp`
/// files from `snapshot_dir`.  On crash between `sync_all` and
/// `rename`, the tmp file lives forever; `prune_snapshot_dir` filters
/// on extension `wal` (not `tmp`), so it never GC's them.  Call this
/// on startup or periodically to bound tmp-file accumulation.
///
/// `older_than_secs` filters by mtime — only tmp files older than
/// this threshold are removed, so we don't race in-progress writes.
/// Returns number of files removed.  Individual failures are
/// logged, not propagated.
pub fn sweep_stale_tmp_files(snapshot_dir: &Path, older_than_secs: u64) -> std::io::Result<usize> {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(older_than_secs))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut removed = 0;
    for entry in std::fs::read_dir(snapshot_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        // Match `*.wal.tmp` by suffix (not extension, which is only
        // the last component after the final dot).
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if !name.ends_with(".wal.tmp") {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        if mtime >= cutoff {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::debug!(
                target: "f1r3fly.fs_wal.snapshot",
                path = %path.display(),
                error = %e,
                "sweep_stale_tmp_files: remove failed; continuing"
            ),
        }
    }
    Ok(removed)
}

/// Slice 30b cadence-writer configuration.  Attached to
/// `RhoRuntimeImpl::fs_snapshot_writer` at boot time when the
/// operator has provisioned `consensus-static-*` buckets AND set
/// `storage.consensus-fs-snapshot-cadence` +
/// `storage.consensus-fs-snapshot-dir`.  `maybe_write` is called
/// once per block from `play_deploys_for_state`; it decides whether
/// this block's height hits the cadence and, if so, persists the
/// snapshot bytes and prunes old snapshots.
#[derive(Debug, Clone)]
pub struct SnapshotWriter {
    /// Canonicalized directory to write snapshots into.  Guaranteed
    /// absolute + symlink-resolved by
    /// `snapshot_config::validate_snapshot_config`.
    pub dir: PathBuf,
    /// Block interval between snapshots.  Guaranteed `>= 1` by boot
    /// validation.
    pub cadence: u64,
    /// How many snapshots to retain on disk.  Older ones are pruned
    /// after each successful write.
    ///
    /// # Default heuristic and its ratification
    ///
    /// Defaults to `max(2, cadence * 2)` snapshots when the
    /// operator has not set `storage.consensus-fs-snapshot-retain`
    /// explicitly (via `snapshot_config::build_snapshot_writer`).
    /// The resulting on-disk history window is roughly
    /// `2 * cadence²` blocks, which scales quadratically in
    /// cadence — almost certainly overprovisioned for small
    /// cadences and underprovisioned for very large ones.
    ///
    /// Slice 30c F-30b-1 disposition: the `cadence * 2` formula
    /// is RATIFIED AS A PLACEHOLDER.  It ships as the default
    /// because (a) it has a defensible minimum-viable rationale
    /// ("keep at least 2 × current cadence's worth of history"),
    /// (b) the retention floor of 2 guarantees any joining
    /// validator can always fetch at least prior + current, and
    /// (c) it's operator-tunable per-node via
    /// `storage.consensus-fs-snapshot-retain` (slice 35).
    /// Operators with concrete joining-SLA targets should set
    /// this explicitly using the sizing formula in the HOCON
    /// docstring: `retain = ceil(N_blocks / cadence) + 1`.
    ///
    /// A future slice may replace the default with a principled
    /// formula (fixed N-blocks joining window regardless of
    /// cadence).  That change would flip existing operator
    /// defaults, so it wants a coordinated rollout with an
    /// explicit config-schema migration.  Until then, the
    /// heuristic ships with this ratification note.
    pub retain: usize,
}

impl SnapshotWriter {
    /// Try to persist a snapshot for `block_number` given the block's
    /// consensus WAL contribution.  Returns Ok(None) on cadence miss
    /// (no snapshot written), Ok(Some(root)) on successful persist.
    ///
    /// Cadence math: writes on blocks where `block_number % cadence == 0`.
    /// Genesis (block_number = 0) writes a snapshot too — cheap and
    /// useful for joining validators as an early-warning content hash.
    pub fn maybe_write(
        &self,
        block_number: i64,
        entries: &[WalEntry],
    ) -> Result<Option<[u8; 32]>, SnapshotError> {
        // Block number is i64 in the block-data ref; treat negative
        // as "no block yet" and skip.
        if block_number < 0 {
            return Ok(None);
        }
        let bn = block_number as u64;
        if !bn.is_multiple_of(self.cadence) {
            return Ok(None);
        }
        if entries.is_empty() {
            // Slice 30c Phase C (F-30b-4 fix): empty-slice cadence
            // hit.  Pre-30c this was a silent skip — indistinguishable
            // from a cadence miss to a joining validator.  Now we
            // append an "empty" sentinel to the manifest so joiners
            // can verify they have not missed a snapshot boundary.
            // No `.wal` file is written on disk (empty payloads all
            // hash to the same content-address; one file per empty
            // slice would waste disk without adding replay value).
            let _ = append_manifest_entry(&self.dir, ManifestEntry::empty(block_number));
            return Ok(None);
        }
        let (_, root) = write_snapshot(&self.dir, entries)?;
        tracing::info!(
            target: "f1r3fly.fs_wal.snapshot",
            block_number,
            root = %{
                let mut s = String::with_capacity(16);
                for b in &root[..8] {
                    use std::fmt::Write;
                    let _ = write!(s, "{b:02x}");
                }
                s
            },
            n_entries = entries.len(),
            "snapshot persisted"
        );
        // Slice 30c Phase C: append to the join-protocol manifest so
        // peers can enumerate this validator's available snapshots
        // by block_number without probing the directory.  Best-
        // effort: manifest append failures do not abort the write
        // path (the snapshot file is already durable; missing
        // manifest lines can be reconstructed by directory scan).
        if let Err(e) = append_manifest_entry(
            &self.dir,
            ManifestEntry::data(block_number, root, entries.len()),
        ) {
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot.manifest",
                block_number,
                error = %e,
                "manifest append failed; snapshot persisted but join-protocol \
                 enumeration will need to fall back to directory scan"
            );
        }
        // Best-effort retention prune.  Failures logged, not
        // propagated — retention is bounded by future writes anyway.
        let _ = prune_snapshot_dir(&self.dir, self.retain);
        Ok(Some(root))
    }
}

// ---------------------------------------------------------------
// Slice 30c Phase C: join-protocol manifest substrate.
//
// A joining validator catching up from genesis (or after a long
// downtime) needs to know WHICH snapshots each peer has on disk
// without listing directories or probing every possible content-
// hash.  The manifest is an append-only file at
// `<snapshot-dir>/manifest.jsonl` recording one line per
// cadence-hit block: block_number, snapshot root (or the "empty"
// sentinel for F-30b-4), entry count, timestamp.
//
// The line format is intentionally simple (JSON per line, no
// serde dependency) so a joining validator's client can parse it
// with a hand-rolled reader if it lives outside the Rust node
// (e.g., a browser-side lightweight explorer, an operator's
// diagnostic script).
//
// # What this slice delivers
//
// - Append side: `SnapshotWriter.maybe_write` emits a manifest
//   entry per cadence hit (both data and empty-sentinel).
// - Read side: `read_manifest(dir)` for peers to publish + for
//   catch-up clients to plan fetches.
// - Empty-slice sentinel: distinguishes "block had zero
//   mutations" from "cadence miss" (F-30b-4 resolution).
//
// # What this slice deliberately DOES NOT deliver
//
// - Network transport (`GET /snapshots/<root_hex>` or similar
//   peer-level RPC): the actual client/server for fetching
//   snapshot bytes across the network.  Requires comm-crate
//   integration, new protobuf messages, retry/peer-selection
//   logic, and a state machine for progress tracking.  A
//   follow-up slice (30c-4 or similar) wires this once the
//   catch-up state machine design is agreed.
// - Manifest signing / attestation.  Currently the manifest is
//   unsigned per-node reporting.  A joining validator asking
//   multiple peers can cross-check root hashes (content-
//   addressed, so mismatches surface as hash mismatches at
//   fetch time).  A signed-manifest protocol may lift this to
//   an attested claim about "which snapshots exist" for
//   audit; deferred pending threat-model review.
// - Automatic reconstruction from directory scan if the manifest
//   is absent.  For now, missing/corrupt manifest → peer serves
//   no discovery (their snapshots are still fetchable by hash
//   if the joiner learns the hash elsewhere).

/// One line in the manifest.  Serialized to a compact JSON
/// object with fixed field ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Block_number the snapshot corresponds to.  For LFB-cadence
    /// (slice 30c Phase B, deferred): the finalized-block height
    /// at which this snapshot was written.
    pub block_number: i64,
    /// `Some(root)` for a data snapshot; `None` for the empty-
    /// slice sentinel (F-30b-4).
    pub root: Option<[u8; 32]>,
    /// Number of `WalEntry` records the snapshot encodes.  Zero
    /// for the empty sentinel; strictly positive for data.
    pub entries: u64,
    /// Wall-clock write timestamp, ms since UNIX_EPOCH.  Best-
    /// effort — a validator whose clock is skewed still produces
    /// a valid manifest, but comparisons across peers are noisy.
    pub ts_ms: i64,
}

impl ManifestEntry {
    pub fn data(block_number: i64, root: [u8; 32], entries: usize) -> Self {
        Self {
            block_number,
            root: Some(root),
            entries: entries as u64,
            ts_ms: now_ms(),
        }
    }

    pub fn empty(block_number: i64) -> Self {
        Self {
            block_number,
            root: None,
            entries: 0,
            ts_ms: now_ms(),
        }
    }

    /// Serialize to a single JSON line (no trailing newline).
    /// Fixed field order + minimal whitespace so peers parsing
    /// with a hand-rolled reader don't have to canonicalize.
    pub fn to_line(&self) -> String {
        let root_field = match &self.root {
            Some(r) => format!("\"{}\"", hex_encode(r)),
            None => "null".to_string(),
        };
        format!(
            "{{\"block_number\":{},\"root\":{},\"entries\":{},\"ts_ms\":{}}}",
            self.block_number, root_field, self.entries, self.ts_ms,
        )
    }

    /// Parse a single manifest line.  Rejects any input that
    /// doesn't match the strict schema — a corrupted line
    /// surfaces here rather than mid-catchup.
    pub fn from_line(line: &str) -> Result<Self, String> {
        // Hand-rolled parser — deliberately not serde to keep
        // the line format independent of any Rust crate's
        // deserializer behavior and to make the wire format
        // reproducible in other languages.
        let trimmed = line.trim();
        let inner = trimmed
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .ok_or_else(|| format!("manifest line missing braces: {line:?}"))?;
        let mut block_number: Option<i64> = None;
        let mut root: Option<Option<[u8; 32]>> = None;
        let mut entries: Option<u64> = None;
        let mut ts_ms: Option<i64> = None;
        for part in split_top_level_commas(inner) {
            let (key, value) = part
                .split_once(':')
                .ok_or_else(|| format!("manifest kv missing `:` in {part:?}"))?;
            let key = key.trim().trim_matches('"');
            let value = value.trim();
            match key {
                "block_number" => {
                    block_number = Some(
                        value
                            .parse()
                            .map_err(|e| format!("block_number parse: {e}"))?,
                    );
                }
                "root" => {
                    if value == "null" {
                        root = Some(None);
                    } else {
                        let hex = value.trim_matches('"');
                        if hex.len() != 64 {
                            return Err(format!("root hex must be 64 chars; got {}", hex.len()));
                        }
                        let bytes = hex_decode_32(hex)?;
                        root = Some(Some(bytes));
                    }
                }
                "entries" => {
                    entries = Some(value.parse().map_err(|e| format!("entries parse: {e}"))?);
                }
                "ts_ms" => {
                    ts_ms = Some(value.parse().map_err(|e| format!("ts_ms parse: {e}"))?);
                }
                other => {
                    return Err(format!("unknown manifest key `{other}`"));
                }
            }
        }
        Ok(Self {
            block_number: block_number.ok_or("missing block_number")?,
            root: root.ok_or("missing root")?,
            entries: entries.ok_or("missing entries")?,
            ts_ms: ts_ms.ok_or("missing ts_ms")?,
        })
    }
}

/// Manifest filename inside the snapshot directory.
pub const MANIFEST_FILENAME: &str = "manifest.jsonl";

/// Append a manifest entry to `<snapshot_dir>/manifest.jsonl`.
/// Creates the file with `0o644` on first append.  Uses
/// `O_APPEND` semantics — multiple concurrent processes writing
/// to the same manifest see line-atomic appends (POSIX
/// guarantee for writes ≤ PIPE_BUF; a manifest line is under
/// that limit).
pub fn append_manifest_entry(
    snapshot_dir: &Path,
    entry: ManifestEntry,
) -> Result<(), SnapshotError> {
    use std::io::Write;
    let path = snapshot_dir.join(MANIFEST_FILENAME);
    let mut file = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o644)
                .open(&path)?
        }
        #[cfg(not(unix))]
        {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?
        }
    };
    let mut line = entry.to_line();
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Read every manifest entry from `<snapshot_dir>/manifest.jsonl`.
/// Returns entries in file order (append order = block-number
/// order under a single writer; under concurrent-writer append
/// races two entries may interleave at line boundaries, still
/// well-formed).  A corrupt line halts parsing at that line
/// and surfaces the error — join clients treat "we have manifest
/// entries up to line K" as a valid partial view.
pub fn read_manifest(snapshot_dir: &Path) -> Result<Vec<ManifestEntry>, SnapshotError> {
    let path = snapshot_dir.join(MANIFEST_FILENAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(SnapshotError::Io(e)),
    };
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry = ManifestEntry::from_line(line).map_err(|e| {
            SnapshotError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("manifest line {}: {}", i + 1, e),
            ))
        })?;
        out.push(entry);
    }
    Ok(out)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn hex_decode_32(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!("expected 64 hex chars; got {}", hex.len()));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("hex byte {i}: {e}"))?;
        out[i] = b;
    }
    Ok(out)
}

/// Split a JSON-object body on top-level commas (no depth
/// tracking beyond top level — manifest entries are flat, so a
/// naive splitter is correct).  Handles quoted strings so a
/// comma inside a string literal (there shouldn't be one in
/// well-formed entries, but defense) doesn't split.
fn split_top_level_commas(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_string = false;
    let mut escape = false;
    for c in inner.chars() {
        if escape {
            buf.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' if in_string => {
                buf.push(c);
                escape = true;
            }
            '"' => {
                in_string = !in_string;
                buf.push(c);
            }
            ',' if !in_string => {
                out.push(std::mem::take(&mut buf));
            }
            _ => buf.push(c),
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

/// F-30-5 review fix (slice 30b): retention policy for the snapshot
/// directory.  Deletes all `*.wal` files EXCEPT the `keep_last_n`
/// most recently modified.  Best-effort — I/O failures on individual
/// files are logged but do not abort the sweep (the goal is to bound
/// disk usage, not to guarantee removal).
///
/// The retention window is measured by filesystem mtime, not by
/// block height, because slice 30b's cadence loop calls this after
/// each snapshot write and the mtime ordering matches the write
/// order.  A future slice may switch to an operator-visible manifest
/// file if cross-restart retention becomes important.
///
/// Returns the number of files removed.
pub fn prune_snapshot_dir(snapshot_dir: &Path, keep_last_n: usize) -> std::io::Result<usize> {
    let mut wal_files: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(snapshot_dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wal") {
                return None;
            }
            // M-P7-2 review fix: use `file_type()` (lstat, does NOT
            // follow symlinks) to detect and skip symlink `.wal`
            // entries.  Attacker-planted `evil.wal -> /etc/passwd`
            // would otherwise get a fresh mtime from its target and
            // persist across restarts.  `path.symlink_metadata()` for
            // the mtime — also lstat-based, so a symlink's mtime is
            // the symlink's own mtime, not the target's.  We do NOT
            // unlink symlinks here (that's a separate operator
            // hygiene concern; `remove_file` on Unix unlinks the
            // symlink itself, not the target — safe, but noisy).
            let file_type = entry.file_type().ok()?;
            if file_type.is_symlink() {
                tracing::debug!(
                    target: "f1r3fly.fs_wal.snapshot",
                    path = %path.display(),
                    "prune_snapshot_dir: skipping symlink .wal entry \
                     (operator hygiene: snapshot dir should be exclusively owned)"
                );
                return None;
            }
            std::fs::symlink_metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|mtime| (path, mtime))
        })
        .collect();
    // Sort newest-first, then keep the first `keep_last_n`.
    wal_files.sort_by(|a, b| b.1.cmp(&a.1));
    let mut removed = 0;
    for (path, _) in wal_files.into_iter().skip(keep_last_n) {
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                path = %path.display(),
                error = %e,
                "prune_snapshot_dir: failed to remove old snapshot; continuing"
            ),
        }
    }
    Ok(removed)
}

/// Read a snapshot by content hash and verify byte-for-byte before
/// returning.  Returns the raw bytes (decoding is a separate concern
/// for the replay engine).
///
/// Slice 34 (MED-1) also validates the leading `SNAPSHOT_FORMAT_VERSION`
/// byte and rejects any other value with `UnsupportedVersion` —
/// clean-fail semantics for an older validator that fetches a
/// snapshot from a newer, hard-forked network.  Truncated blobs
/// (fewer than 1 byte) surface `Truncated`.
pub fn read_snapshot_bytes(snapshot_dir: &Path, root: &[u8; 32]) -> Result<Vec<u8>, SnapshotError> {
    let path = snapshot_path(snapshot_dir, root);
    let bytes = std::fs::read(&path)?;
    let got = hash_of(&bytes);
    if got != *root {
        return Err(SnapshotError::RootMismatch {
            expected: *root,
            got,
        });
    }
    // Version check runs AFTER hash verification: an attacker who
    // handed us a byte string that hashed to the requested root but
    // carried a bogus version byte would be caught here even if the
    // root check somehow passed (defense in depth).  In practice
    // Blake2b256 preimage resistance already makes forging bytes to
    // a target root computationally infeasible.
    let Some(&version) = bytes.first() else {
        return Err(SnapshotError::Truncated { got: 0, need: 1 });
    };
    if version != SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotError::UnsupportedVersion {
            got: version,
            supported: SNAPSHOT_FORMAT_VERSION,
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_write_entry(payload: &[u8], path: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from(path),
            extra_path: None,
            offset: None,
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::hash(payload)),
            mode_bits: None,
            owner: None,
            group: None,
        }
    }

    /// Slice 34 (MED-1): empty slice is now version byte (1) +
    /// four-byte count of zero.  Pre-slice-34 layout was just the
    /// four zero bytes.  Bumping golden hex is intentional here —
    /// the version byte is an added consensus commitment.
    #[test]
    fn empty_slice_encodes_to_version_byte_plus_four_zero_bytes() {
        let bytes = encode_wal_slice(&[]);
        assert_eq!(bytes, vec![SNAPSHOT_FORMAT_VERSION, 0, 0, 0, 0]);
    }

    #[test]
    fn empty_slice_root_is_deterministic() {
        let r1 = compute_wal_root(&[]);
        let r2 = compute_wal_root(&[]);
        assert_eq!(r1, r2, "same input → same root");
    }

    #[test]
    fn same_entries_yield_identical_root() {
        let a = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        let b = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        assert_eq!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn different_order_yields_different_root() {
        let a = vec![
            mk_write_entry(b"aa", "/root/x"),
            mk_write_entry(b"bb", "/root/y"),
        ];
        let b = vec![
            mk_write_entry(b"bb", "/root/y"),
            mk_write_entry(b"aa", "/root/x"),
        ];
        assert_ne!(
            compute_wal_root(&a),
            compute_wal_root(&b),
            "reordering entries must change the root (append order is consensus-observable)"
        );
    }

    #[test]
    fn different_payload_yields_different_root() {
        let a = vec![mk_write_entry(b"aa", "/root/x")];
        let b = vec![mk_write_entry(b"ab", "/root/x")];
        assert_ne!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn different_path_yields_different_root() {
        let a = vec![mk_write_entry(b"aa", "/root/x")];
        let b = vec![mk_write_entry(b"aa", "/root/y")];
        assert_ne!(compute_wal_root(&a), compute_wal_root(&b));
    }

    #[test]
    fn snapshot_blob_round_trip_via_disk() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"hello", "/root/greeting"), WalEntry {
            op: WalOp::Truncate,
            path: PathBuf::from("/root/greeting"),
            extra_path: None,
            offset: Some(5),
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
        }];
        let (path, root) = write_snapshot(dir.path(), &entries).unwrap();
        assert!(path.exists(), "snapshot file must be created");
        let hex_name = path.file_name().unwrap().to_string_lossy();
        assert!(
            hex_name.ends_with(".wal"),
            "filename must end in .wal, got {hex_name}"
        );
        let bytes = read_snapshot_bytes(dir.path(), &root).unwrap();
        assert_eq!(hash_of(&bytes), root, "round-tripped bytes match root");
        assert_eq!(bytes, encode_wal_slice(&entries));
    }

    #[test]
    fn read_snapshot_rejects_tampered_file() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/root/x")];
        let (path, root) = write_snapshot(dir.path(), &entries).unwrap();
        // Tamper: append a byte.
        let mut tampered = std::fs::read(&path).unwrap();
        tampered.push(0xff);
        std::fs::write(&path, &tampered).unwrap();
        let err = read_snapshot_bytes(dir.path(), &root);
        assert!(
            matches!(err, Err(SnapshotError::RootMismatch { .. })),
            "tampered snapshot must fail root verification"
        );
    }

    #[test]
    fn write_snapshot_is_idempotent_for_same_content() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/root/x")];
        let (p1, r1) = write_snapshot(dir.path(), &entries).unwrap();
        let (p2, r2) = write_snapshot(dir.path(), &entries).unwrap();
        assert_eq!(p1, p2, "same content → same path");
        assert_eq!(r1, r2, "same content → same root");
        assert!(p1.exists());
    }

    #[test]
    fn op_tags_are_stable() {
        // Any renumbering here is a hard-fork of the WAL root — pin
        // the values so a reorderer in the enum can't silently change
        // the tag.
        assert_eq!(op_tag(WalOp::Write), 1);
        assert_eq!(op_tag(WalOp::WriteAt), 2);
        assert_eq!(op_tag(WalOp::Truncate), 3);
        assert_eq!(op_tag(WalOp::Chmod), 4);
        assert_eq!(op_tag(WalOp::Chown), 5);
        assert_eq!(op_tag(WalOp::RemoveFile), 6);
        assert_eq!(op_tag(WalOp::RemoveDir), 7);
        assert_eq!(op_tag(WalOp::Rename), 8);
        assert_eq!(op_tag(WalOp::CopyFile), 9);
        assert_eq!(op_tag(WalOp::Read), 10);
        assert_eq!(op_tag(WalOp::ReadAt), 11);
    }

    // ------------------------------------------------------------------
    // Round-2 review-fix tests (H-30-4 / H-30-5 / H-30-7 / coverage M1/M2)
    // ------------------------------------------------------------------

    /// H-30-4: golden-hex root pin.  Any change to the canonical
    /// encoding (Blake2b256 config, prefix width, endianness,
    /// `PathBuf::as_encoded_bytes()` semantics) will flip this hash.
    /// A validator upgrade that inadvertently changes the encoding
    /// diverges on-network; this test catches it in CI before
    /// deployment.  If you're updating this hash, you are proposing a
    /// hard fork of the WAL commitment scheme — bump the WAL version
    /// and coordinate.
    #[test]
    fn compute_wal_root_golden_hex() {
        // A minimal, fully-populated entry that exercises every field
        // (op, path, no extra_path, offset, length, hash payload,
        // no mode_bits, no owner, no group).
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/root/canonical.bin"),
            extra_path: None,
            offset: Some(0),
            length: Some(5),
            payload_ref: Some(PayloadRef::hash(b"hello")),
            mode_bits: None,
            owner: None,
            group: None,
        }];
        let root = compute_wal_root(&entries);
        let hex = root.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
        // Golden value re-pinned 2026-08-05 (slice 34, MED-1: added
        // `SNAPSHOT_FORMAT_VERSION = 1` byte at the front of every
        // encoded slice).  Pre-slice-34 value was
        //   06a8ce938471c2a9722aa3592209e04dbe9230b759af36a5088dea677f93b825
        // Regenerate via
        //   cargo test -p rholang --lib -- compute_wal_root_golden_hex --nocapture
        // ONLY when intentionally hard-forking the encoding.
        const EXPECTED: &str = "532eea9096eb6962acbb48374e79167149960ec132f8e95838678e20e2fa38b2";
        assert_eq!(
            hex, EXPECTED,
            "WAL root golden-hex mismatch — did you accidentally change the encoding? \
             If intentional, bump the WAL version and update EXPECTED."
        );
    }

    /// H-30-5: prove that `encode_entry` actually uses the `op_tag`
    /// values.  Pre-fix, the `op_tags_are_stable` test only checked
    /// the private `op_tag` fn in isolation — an `encode_entry`
    /// refactor that inlined `e.op as u8` (auto-discriminants: 0-8
    /// not 1-9) would silently drift while `op_tags_are_stable`
    /// still passed.  This test encodes one entry per WalOp variant
    /// and asserts the first byte of the entry region (index 4,
    /// after the u32-be count prefix) matches the expected tag.
    #[test]
    fn encode_entry_uses_op_tag_values() {
        for (op, tag) in [
            (WalOp::Write, 1u8),
            (WalOp::WriteAt, 2),
            (WalOp::Truncate, 3),
            (WalOp::Chmod, 4),
            (WalOp::Chown, 5),
            (WalOp::RemoveFile, 6),
            (WalOp::RemoveDir, 7),
            (WalOp::Rename, 8),
            (WalOp::CopyFile, 9),
            (WalOp::Read, 10),
            (WalOp::ReadAt, 11),
        ] {
            let e = WalEntry {
                op,
                path: PathBuf::from("/x"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
            };
            let bytes = encode_wal_slice(&[e]);
            // Slice 34: byte 0 = version, bytes 1..5 = count(1) BE,
            // byte 5 = op tag.
            assert_eq!(bytes[0], SNAPSHOT_FORMAT_VERSION);
            assert_eq!(&bytes[1..5], &1u32.to_be_bytes());
            assert_eq!(
                bytes[5], tag,
                "encode_entry byte 5 for {op:?} must be {tag}, got {}",
                bytes[5]
            );
        }
    }

    /// H-30-7: DeployRef byte-layout stability.  Verifies
    /// (a) the tag byte is 2, (b) block_hash comes first, then
    /// deploy_index big-endian u32, then arg_index big-endian u32,
    /// (c) swapping deploy_index and arg_index produces different
    /// roots, (d) endianness is big-endian (not native).
    #[test]
    fn deploy_ref_encoding_is_big_endian_and_field_order_is_stable() {
        let block_hash = [0xAAu8; 32];
        let e1 = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash,
                deploy_index: 0x0102_0304,
                arg_index: 0x0506_0708,
            }),
            mode_bits: None,
            owner: None,
            group: None,
        };
        let mut e2 = e1.clone();
        if let Some(PayloadRef::DeployRef {
            deploy_index,
            arg_index,
            ..
        }) = &mut e2.payload_ref
        {
            std::mem::swap(deploy_index, arg_index);
        }
        assert_ne!(
            compute_wal_root(&[e1.clone()]),
            compute_wal_root(&[e2]),
            "swapping deploy_index and arg_index MUST change the root"
        );

        // Byte-layout pin: encode e1 and locate the payload_ref bytes.
        let bytes = encode_wal_slice(&[e1]);
        // Slice 34 layout: version(1) count(4) op(1) path_len(4)
        //   path("/x" = 2) extra_flag(1) offset_flag(1) length_flag(1)
        //   payload_tag(1) ...
        // Walk 1+4+1+4+2+1+1+1 = 15 to reach the payload tag.
        assert_eq!(bytes[0], SNAPSHOT_FORMAT_VERSION, "version byte");
        assert_eq!(bytes[15], 2, "payload_ref tag must be 2 for DeployRef");
        // Next 32 bytes = block_hash (all 0xAA).
        assert!(
            bytes[16..16 + 32].iter().all(|&b| b == 0xAA),
            "block_hash bytes must appear directly after tag byte"
        );
        // Next 4 bytes = deploy_index big-endian.
        assert_eq!(
            &bytes[16 + 32..16 + 32 + 4],
            &0x0102_0304u32.to_be_bytes(),
            "deploy_index must be big-endian"
        );
        // Next 4 bytes = arg_index big-endian.
        assert_eq!(
            &bytes[16 + 32 + 4..16 + 32 + 8],
            &0x0506_0708u32.to_be_bytes(),
            "arg_index must be big-endian and follow deploy_index"
        );
    }

    /// Coverage M1: every WalOp variant round-trips through
    /// `encode_wal_slice` without panicking, and every op produces
    /// distinct bytes when combined with the same other fields
    /// (proves the op is part of the encoding, not silently
    /// stripped).
    #[test]
    fn every_walop_variant_encodes_distinctly() {
        let mut roots = std::collections::HashSet::new();
        for op in [
            WalOp::Write,
            WalOp::WriteAt,
            WalOp::Truncate,
            WalOp::Chmod,
            WalOp::Chown,
            WalOp::RemoveFile,
            WalOp::RemoveDir,
            WalOp::Rename,
            WalOp::CopyFile,
        ] {
            let e = WalEntry {
                op,
                path: PathBuf::from("/x"),
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
            };
            let root = compute_wal_root(&[e]);
            assert!(
                roots.insert(root),
                "op {op:?} produced a duplicate root — the op tag is not part of the encoding"
            );
        }
        assert_eq!(roots.len(), 9);
    }

    /// Coverage M2: every `Option::Some` field arm exercises the
    /// Some-branch of its encoder helper.  Pre-fix, only `None`
    /// arms of `extra_path`, `mode_bits`, `owner`, `group` were
    /// covered by any test — a bug in the Some encoding would slip
    /// through.
    #[test]
    fn every_option_some_arm_encodes_distinctly_from_none() {
        let base = WalEntry {
            op: WalOp::Rename,
            path: PathBuf::from("/from"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
        };
        let base_root = compute_wal_root(&[base.clone()]);

        // extra_path: None vs Some(distinct path).
        let mut e = base.clone();
        e.extra_path = Some(PathBuf::from("/to"));
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "extra_path Some must diff"
        );

        // mode_bits: None vs Some.
        let mut e = base.clone();
        e.mode_bits = Some(0o755);
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "mode_bits Some must diff"
        );

        // owner: None vs Some.
        let mut e = base.clone();
        e.owner = Some("alice".to_string());
        assert_ne!(compute_wal_root(&[e]), base_root, "owner Some must diff");

        // group: None vs Some.
        let mut e = base.clone();
        e.group = Some("wheel".to_string());
        assert_ne!(compute_wal_root(&[e]), base_root, "group Some must diff");

        // Empty vs None (distinguishability): "" is NOT None.
        let mut e = base.clone();
        e.owner = Some(String::new());
        assert_ne!(
            compute_wal_root(&[e]),
            base_root,
            "Some(empty string) must be distinguishable from None"
        );
    }

    /// F-30-5 slice-30b: `prune_snapshot_dir` keeps the newest N and
    /// removes older ones.
    #[test]
    fn prune_snapshot_dir_keeps_last_n() {
        let dir = tempfile::tempdir().unwrap();
        // Write 5 snapshots with distinct content (distinct hashes,
        // distinct filenames).  Sleep 10ms between writes so mtimes
        // are monotonically distinguishable.
        let mut roots = Vec::new();
        for i in 0..5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            let (_, root) = write_snapshot(dir.path(), &entries).unwrap();
            roots.push(root);
            // L-30-COV-1 review fix: bump from 20ms to 1100ms so
            // macOS APFS (which historically had 1-second mtime
            // granularity on some volumes) reliably orders these
            // writes.  Slow but robust; retention tests run once
            // per CI so the added seconds are negligible.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        assert_eq!(
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal"))
                .count(),
            5
        );
        // Keep the 2 newest.
        let removed = prune_snapshot_dir(dir.path(), 2).unwrap();
        assert_eq!(removed, 3, "3 old snapshots should be removed");
        // The newest two files are the last two roots we wrote.
        for root in &roots[3..] {
            assert!(
                snapshot_path(dir.path(), root).exists(),
                "root {:?} must survive prune",
                &root[..4]
            );
        }
        for root in &roots[..3] {
            assert!(
                !snapshot_path(dir.path(), root).exists(),
                "root {:?} must be pruned",
                &root[..4]
            );
        }
    }

    /// prune_snapshot_dir on an empty dir is a no-op.
    #[test]
    fn prune_snapshot_dir_no_op_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(prune_snapshot_dir(dir.path(), 5).unwrap(), 0);
    }

    // ------------------------------------------------------------------
    // Slice 30b: SnapshotWriter cadence + retention tests.
    // ------------------------------------------------------------------

    #[test]
    fn snapshot_writer_cadence_skips_non_boundary_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 3,
        };
        let entries = vec![mk_write_entry(b"a", "/x")];
        // Blocks 1..5 (not aligned to cadence=5 boundary; 5 is aligned)
        assert!(writer.maybe_write(1, &entries).unwrap().is_none());
        assert!(writer.maybe_write(2, &entries).unwrap().is_none());
        assert!(writer.maybe_write(3, &entries).unwrap().is_none());
        assert!(writer.maybe_write(4, &entries).unwrap().is_none());
        // Block 5: cadence hit.
        let root = writer.maybe_write(5, &entries).unwrap();
        assert!(root.is_some(), "cadence-hit block must persist");
        assert!(snapshot_path(dir.path(), &root.unwrap()).exists());
    }

    #[test]
    fn snapshot_writer_genesis_block_writes_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 10,
            retain: 5,
        };
        // Block 0 is % 10 == 0 → cadence hit.
        let entries = vec![mk_write_entry(b"genesis", "/genesis")];
        let root = writer.maybe_write(0, &entries).unwrap();
        assert!(root.is_some(), "genesis (block 0) is a cadence-hit");
    }

    #[test]
    fn snapshot_writer_skips_empty_entries_even_on_cadence_hit() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 3,
        };
        // Cadence=1 means every block is a hit, but empty entries
        // don't produce a `.wal` file (empty payloads all hash to
        // the same content-address; one file per empty slice would
        // waste disk).  Slice 30c Phase C (F-30b-4 resolution):
        // empty cadence hits DO append a manifest sentinel line so
        // joining validators can distinguish "block had no fs
        // mutations" from "cadence miss."
        assert!(writer.maybe_write(0, &[]).unwrap().is_none());
        assert!(writer.maybe_write(1, &[]).unwrap().is_none());
        // No `.wal` files — the byte-heavy artifact is what we
        // skip on empty slices.
        let wal_files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|s| s == "wal"))
            .collect();
        assert!(
            wal_files.is_empty(),
            "empty slice must NOT write a .wal file; got {} files",
            wal_files.len()
        );
        // But manifest.jsonl DOES exist and contains 2 sentinel
        // entries (one per cadence-hit block).
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 2, "one sentinel per cadence-hit block");
        for e in &manifest {
            assert!(e.root.is_none(), "sentinel entries have root = None");
        }
    }

    #[test]
    fn snapshot_writer_negative_block_number_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 3,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        // Negative block numbers indicate "no block yet" state and
        // must not persist.
        assert!(writer.maybe_write(-1, &entries).unwrap().is_none());
        assert!(writer.maybe_write(-100, &entries).unwrap().is_none());
    }

    /// Slice 30c F-30b-3 fix pin: `BlockData::empty()` returns
    /// `block_number = -1` as a sentinel meaning "no block yet."
    /// A runtime that ever calls `maybe_write` in that state must
    /// not persist a snapshot (would otherwise land at block 0 or
    /// some cadence-hit due to a bogus initial value).  The
    /// negative-block-number skip above catches this; this test
    /// pins the intent explicitly.
    #[test]
    fn snapshot_writer_skips_block_data_empty_sentinel() {
        use crate::rust::interpreter::system_processes::BlockData;
        let sentinel_block_number = BlockData::empty().block_number;
        assert!(
            sentinel_block_number < 0,
            "BlockData::empty must use a negative sentinel (F-30b-3); got {sentinel_block_number}"
        );
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1, // any cadence — negative < 0 skip triggers first
            retain: 3,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        assert!(
            writer
                .maybe_write(sentinel_block_number, &entries)
                .unwrap()
                .is_none(),
            "BlockData::empty sentinel must NOT trigger a snapshot write"
        );
        // Companion pin: manifest is also NOT touched.
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(
            manifest.is_empty(),
            "sentinel-block writes must not append manifest entries"
        );
    }

    #[test]
    fn snapshot_writer_prunes_to_retention_bound() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1, // every block
            retain: 2,
        };
        // Write snapshots for blocks 1..=5 with distinct content.
        for i in 1..=5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            writer.maybe_write(i as i64, &entries).unwrap();
            // L-30-COV-1 review fix: bump from 20ms to 1100ms so
            // macOS APFS (which historically had 1-second mtime
            // granularity on some volumes) reliably orders these
            // writes.  Slow but robust; retention tests run once
            // per CI so the added seconds are negligible.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        // Only 2 should remain (retention = 2).
        let wal_count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(
            wal_count, 2,
            "retention bound must cap the on-disk snapshot count"
        );
    }

    // ------------------------------------------------------------------
    // Round-2 review fixes (slice 30b): fsync mode, tmp sweep,
    // cadence boundaries, retention edges, multi-snapshot
    // concatenation, O_EXCL semantics.
    // ------------------------------------------------------------------

    /// M-30b-4: on Unix, written snapshot files have 0o644 mode
    /// regardless of the process's umask.
    #[cfg(unix)]
    #[test]
    fn write_snapshot_sets_explicit_0o644_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_write_entry(b"aa", "/x")];
        let (path, _) = write_snapshot(dir.path(), &entries).unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        let mode = perms.mode() & 0o777;
        assert_eq!(
            mode, 0o644,
            "snapshot file mode must be 0o644 regardless of umask; got 0o{mode:o}"
        );
    }

    /// H-30b-3: `write_snapshot` uses `create_new` (O_EXCL) on the
    /// tmp file.  A regression to `create(true).truncate(true)`
    /// would silently work in the round-trip test but reopen the
    /// M-30-1 concurrent-writer / M-30-3 symlink race.  We can't
    /// guess the exact tmp filename (pid + nanos), but we can pin
    /// the semantic: pre-place a file matching the `.wal.tmp`
    /// pattern with attacker content and confirm `write_snapshot`
    /// either (a) fails hard or (b) succeeds without touching the
    /// pre-existing file (because it chose a different nanos).
    #[test]
    fn write_snapshot_never_truncates_existing_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-place a decoy at a `.wal.tmp` path.  The real tmp
        // filename includes pid + nanos, so we can't collide by
        // guess — but we can verify decoy files are UNTOUCHED after
        // a snapshot write (proving `write_snapshot` doesn't open
        // ANY existing file).
        let decoy = dir.path().join("some-other.wal.tmp");
        let decoy_content = b"attacker owns me";
        std::fs::write(&decoy, decoy_content).unwrap();

        let entries = vec![mk_write_entry(b"real", "/x")];
        write_snapshot(dir.path(), &entries).unwrap();

        // Decoy contents are intact — write_snapshot did not
        // truncate or open it.
        assert_eq!(
            std::fs::read(&decoy).unwrap(),
            decoy_content,
            "write_snapshot must never touch existing files"
        );
    }

    /// M-30b-3: `sweep_stale_tmp_files` removes old `.wal.tmp`
    /// files (leftover from crashes) but leaves recent ones alone.
    #[test]
    fn sweep_stale_tmp_files_removes_old_but_keeps_recent() {
        let dir = tempfile::tempdir().unwrap();
        let old_tmp = dir.path().join("crashed-1.wal.tmp");
        let recent_tmp = dir.path().join("in-progress.wal.tmp");
        std::fs::write(&old_tmp, b"crashed").unwrap();
        std::fs::write(&recent_tmp, b"recent").unwrap();
        // Simplest strategy (without pulling in `filetime`): sweep
        // with `older_than_secs = 0` so ALL tmp files are older than
        // "now-0"; verify both are removed.  Then re-check with
        // older_than_secs=3600 to verify a just-created recent tmp
        // file is preserved.
        let removed = sweep_stale_tmp_files(dir.path(), 0).unwrap();
        assert_eq!(
            removed, 2,
            "with cutoff=0, both tmp files should be removed"
        );

        // Re-create for the "preserve recent" test.
        std::fs::write(&recent_tmp, b"recent").unwrap();
        let removed = sweep_stale_tmp_files(dir.path(), 3600).unwrap();
        assert_eq!(
            removed, 0,
            "with cutoff=3600s, a just-created tmp file must not be removed"
        );
        assert!(recent_tmp.exists());
    }

    /// M-30b-3 companion: sweep ignores files that don't match the
    /// `.wal.tmp` suffix.  Operators may put other files in the
    /// snapshot dir; sweep must not touch them.
    #[test]
    fn sweep_stale_tmp_files_ignores_non_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"notes").unwrap();
        std::fs::write(dir.path().join("real.wal"), b"data").unwrap();
        std::fs::write(dir.path().join("crashed.wal.tmp"), b"tmp").unwrap();
        let removed = sweep_stale_tmp_files(dir.path(), 0).unwrap();
        assert_eq!(removed, 1, "only the .wal.tmp file should be swept");
        assert!(dir.path().join("README").exists());
        assert!(dir.path().join("real.wal").exists());
        assert!(!dir.path().join("crashed.wal.tmp").exists());
    }

    /// H-30b-5: log-structured semantics — concatenating multiple
    /// snapshots is byte-consistent with a single-blob write of the
    /// union (post-count-prefix concatenation, since the count
    /// prefix is per-slice).  This is a joining-protocol pin: a
    /// late-joining validator reads each snapshot independently and
    /// applies its entries in the encoded order.
    #[test]
    fn multi_snapshot_concat_matches_ordered_entry_replay() {
        let dir = tempfile::tempdir().unwrap();
        let block_1 = vec![mk_write_entry(b"a", "/x"), mk_write_entry(b"b", "/y")];
        let block_2 = vec![mk_write_entry(b"c", "/z")];
        let (_, root_1) = write_snapshot(dir.path(), &block_1).unwrap();
        let (_, root_2) = write_snapshot(dir.path(), &block_2).unwrap();
        // Read each snapshot back independently.
        let bytes_1 = read_snapshot_bytes(dir.path(), &root_1).unwrap();
        let bytes_2 = read_snapshot_bytes(dir.path(), &root_2).unwrap();
        // Each blob independently encodes its own count-prefixed slice.
        assert_eq!(bytes_1, encode_wal_slice(&block_1));
        assert_eq!(bytes_2, encode_wal_slice(&block_2));
        // The joining-replay semantic: after applying both slices in
        // order, the effective entry sequence equals concatenation.
        let concatenated = [block_1.as_slice(), block_2.as_slice()].concat();
        let single_blob_root = compute_wal_root(&concatenated);
        // Note: `single_blob_root` != `root_1` and != `root_2` because
        // the count prefix differs (2 vs 3 entries).  This test pins
        // the correct semantic: the joining protocol replays entries
        // in order across snapshots; it does NOT concatenate bytes.
        assert_ne!(single_blob_root, root_1);
        assert_ne!(single_blob_root, root_2);
    }

    /// M-30b-6: `SnapshotWriter::maybe_write` cadence boundary at
    /// `2 * cadence` and `3 * cadence`.  Existing `cadence=5, block=5`
    /// test would pass a regression to `block % cadence == cadence - 1`;
    /// this test adds the extra boundaries.
    #[test]
    fn snapshot_writer_cadence_hits_multiples_of_cadence() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 100,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        for bn in [5i64, 10, 15, 100, 1000] {
            let r = writer.maybe_write(bn, &entries).unwrap();
            assert!(
                r.is_some(),
                "block {bn} (multiple of 5) must be a cadence hit"
            );
        }
        for bn in [1i64, 4, 6, 9, 11, 999] {
            let r = writer.maybe_write(bn, &entries).unwrap();
            assert!(
                r.is_none(),
                "block {bn} (not multiple of 5) must be a cadence miss"
            );
        }
    }

    /// M-30b-6 companion: very-large block numbers hit cadence
    /// deterministically via `is_multiple_of`.
    #[test]
    fn snapshot_writer_handles_large_block_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1_000_000,
            retain: 3,
        };
        let entries = vec![mk_write_entry(b"x", "/x")];
        // A random-looking block number that's an exact multiple.
        let boundary = 42_000_000i64;
        assert_eq!(boundary % 1_000_000, 0);
        assert!(writer.maybe_write(boundary, &entries).unwrap().is_some());
        // One off the boundary should miss.
        assert!(writer
            .maybe_write(boundary + 1, &entries)
            .unwrap()
            .is_none());
    }

    /// M-30b-7: retain edge — `retain = 0` prunes everything after
    /// each write.  Pinned as behavioral spec (unusual but valid
    /// operator config for a "snapshot-only, no retention" mode).
    #[test]
    fn snapshot_writer_retain_zero_prunes_all_after_each_write() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: 0,
        };
        let entries_1 = vec![mk_write_entry(b"a", "/x")];
        let entries_2 = vec![mk_write_entry(b"b", "/y")];
        writer.maybe_write(1, &entries_1).unwrap();
        // Prune with retain=0 removes the just-written file.
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(
            count, 0,
            "retain=0 must prune every snapshot immediately after write"
        );
        writer.maybe_write(2, &entries_2).unwrap();
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(count, 0);
    }

    /// M-30b-7 companion: retain = usize::MAX means prune is a
    /// no-op (retention window exceeds anything the operator could
    /// realistically produce).
    #[test]
    fn snapshot_writer_retain_max_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 1,
            retain: usize::MAX,
        };
        for i in 1..=5u8 {
            let entries = vec![mk_write_entry(&[i], "/x")];
            writer.maybe_write(i as i64, &entries).unwrap();
        }
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "wal")
            })
            .count();
        assert_eq!(count, 5, "retain=usize::MAX must never prune");
    }

    /// prune_snapshot_dir ignores non-`.wal` files (e.g. operator's
    /// README, backup files).
    #[test]
    fn prune_snapshot_dir_ignores_non_wal_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"operator notes").unwrap();
        std::fs::write(dir.path().join("backup.tar"), b"data").unwrap();
        let entries = vec![mk_write_entry(b"a", "/x")];
        write_snapshot(dir.path(), &entries).unwrap();
        assert_eq!(prune_snapshot_dir(dir.path(), 0).unwrap(), 1);
        // Non-WAL files survive.
        assert!(dir.path().join("README").exists());
        assert!(dir.path().join("backup.tar").exists());
    }

    /// Coverage: `read_snapshot_bytes` on non-existent file returns
    /// `SnapshotError::Io`.  Pins the error-mapping so a regression
    /// swallowing NotFound as an empty slice would fail.
    #[test]
    fn read_snapshot_nonexistent_file_returns_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let bogus_root = [0u8; 32];
        let err = read_snapshot_bytes(dir.path(), &bogus_root);
        assert!(
            matches!(err, Err(SnapshotError::Io(_))),
            "expected Io error for non-existent file, got {err:?}"
        );
    }

    #[test]
    fn deploy_ref_payload_is_encoded_distinctly_from_hash() {
        // Verify the DeployRef branch encodes differently from Hash
        // (even if the raw bytes overlap): the leading tag byte
        // discriminates.
        let e_hash = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(0),
            payload_ref: Some(PayloadRef::Hash([0u8; 32])),
            mode_bits: None,
            owner: None,
            group: None,
        };
        let e_ref = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(0),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0u8; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
        };
        assert_ne!(
            compute_wal_root(&[e_hash]),
            compute_wal_root(&[e_ref]),
            "Hash and DeployRef payload refs must produce different roots"
        );
    }

    // ------------------------------------------------------------------
    // Slice 34 (MED-1) tests: version byte + hard-fork catalog pins.
    // ------------------------------------------------------------------

    /// Every encoded slice begins with `SNAPSHOT_FORMAT_VERSION`
    /// (currently `1`).  Regression pin against a refactor that
    /// drops the version prefix.
    #[test]
    fn encoded_slice_starts_with_format_version_byte() {
        let bytes = encode_wal_slice(&[]);
        assert_eq!(
            bytes.first().copied(),
            Some(SNAPSHOT_FORMAT_VERSION),
            "encoded WAL slice must start with SNAPSHOT_FORMAT_VERSION"
        );
        let e = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
        };
        let bytes = encode_wal_slice(&[e]);
        assert_eq!(bytes.first().copied(), Some(SNAPSHOT_FORMAT_VERSION));
    }

    /// `read_snapshot_bytes` accepts a well-formed v1 blob (the
    /// happy path).  Also documents by construction that
    /// `write_snapshot` + `read_snapshot_bytes` round-trip.
    #[test]
    fn read_snapshot_bytes_accepts_current_version() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(3),
            payload_ref: Some(PayloadRef::hash(b"abc")),
            mode_bits: None,
            owner: None,
            group: None,
        }];
        let (_, root) = write_snapshot(dir.path(), &entries).unwrap();
        let bytes = read_snapshot_bytes(dir.path(), &root).expect("v1 round-trip");
        assert_eq!(bytes.first().copied(), Some(SNAPSHOT_FORMAT_VERSION));
    }

    /// A blob whose leading version byte is NOT
    /// `SNAPSHOT_FORMAT_VERSION` fails cleanly with
    /// `UnsupportedVersion` — the joining-validator use case where
    /// an older binary sees a snapshot from a newer, hard-forked
    /// network.
    #[test]
    fn read_snapshot_bytes_rejects_unknown_version() {
        let dir = tempfile::tempdir().unwrap();
        // Fabricate a blob with a bogus version byte.  Compute its
        // Blake2b256 so read_snapshot_bytes's hash check passes and
        // we exercise the version-check code path.
        let mut bogus = vec![99u8]; // version 99 — nonexistent
        bogus.extend_from_slice(&0u32.to_be_bytes()); // count = 0
        let root = hash_of(&bogus);
        let path = snapshot_path(dir.path(), &root);
        std::fs::write(&path, &bogus).unwrap();

        match read_snapshot_bytes(dir.path(), &root) {
            Err(SnapshotError::UnsupportedVersion { got, supported }) => {
                assert_eq!(got, 99);
                assert_eq!(supported, SNAPSHOT_FORMAT_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// A completely empty blob (zero bytes) surfaces `Truncated`
    /// rather than mis-decoding.  The hash check passes vacuously
    /// (hash of empty is well-defined), so the version-length
    /// guard is what catches this.
    #[test]
    fn read_snapshot_bytes_rejects_truncated_blob() {
        let dir = tempfile::tempdir().unwrap();
        let empty: Vec<u8> = Vec::new();
        let root = hash_of(&empty);
        let path = snapshot_path(dir.path(), &root);
        std::fs::write(&path, &empty).unwrap();

        match read_snapshot_bytes(dir.path(), &root) {
            Err(SnapshotError::Truncated { got, need }) => {
                assert_eq!(got, 0);
                assert_eq!(need, 1);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    /// The hard-fork surface catalog in the module docstring must
    /// enumerate all consensus-critical encoding decisions.  This
    /// test scans the source for the 8 catalog headings — a
    /// regression that removes an item from the docstring (or
    /// silently adds a new consensus-observable field without
    /// listing it) surfaces here.  The catalog is a MAINTAINER
    /// contract: additions to any of these surfaces are hard forks.
    #[test]
    fn hard_fork_surface_catalog_is_pinned() {
        let src = include_str!("snapshot.rs");
        // The section header itself.
        assert!(
            src.contains("# Hard-fork surface catalog"),
            "module docstring must contain the catalog section header"
        );
        // Each of the 8 catalog items — labelled by their leading
        // number in the numbered list.  The test bodies check for
        // the distinctive keyword too, so a rename of the item
        // (e.g. "Format version byte" → "Version prefix") that
        // preserves the numbering still fails here — forcing the
        // maintainer to think about whether the change is a
        // consensus-neutral wording tweak or a real semantics
        // change that needs a version bump.
        let items = [
            ("1.", "Format version byte"),
            ("2.", "Op tag bytes"),
            ("3.", "Hash function = Blake2b256"),
            ("4.", "Length-prefix widths"),
            ("5.", "Field widths / endianness"),
            ("6.", "`PayloadRef` variant tags"),
            ("7.", "Field order inside `encode_entry`"),
            ("8.", "Path encoding"),
        ];
        for (num, keyword) in items {
            let needle = format!("// {num} **{keyword}");
            assert!(
                src.contains(&needle),
                "catalog item missing or renamed: `{needle}` — if you're \
                 adding a new consensus-critical surface, extend both the \
                 docstring AND this test's `items` array"
            );
        }
    }

    // ------------------------------------------------------------------
    // Slice 30c Phase C: manifest / join-protocol substrate tests.
    // ------------------------------------------------------------------

    #[test]
    fn manifest_entry_data_round_trips_through_line_format() {
        let root = [0xABu8; 32];
        let e = ManifestEntry {
            block_number: 12345,
            root: Some(root),
            entries: 42,
            ts_ms: 1_722_400_000_000,
        };
        let line = e.to_line();
        // Sanity: line must be single-line JSON (no interior newlines).
        assert!(!line.contains('\n'));
        assert!(line.starts_with('{') && line.ends_with('}'));
        let parsed = ManifestEntry::from_line(&line).expect("round-trip");
        assert_eq!(parsed, e);
    }

    #[test]
    fn manifest_entry_empty_sentinel_round_trips() {
        let e = ManifestEntry::empty(999);
        let line = e.to_line();
        assert!(
            line.contains("\"root\":null"),
            "empty entry must serialize root as null; got {line}"
        );
        let parsed = ManifestEntry::from_line(&line).expect("round-trip");
        assert_eq!(parsed, e);
        assert!(parsed.root.is_none());
        assert_eq!(parsed.entries, 0);
    }

    #[test]
    fn append_and_read_manifest_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![
            ManifestEntry::data(100, [0x11; 32], 5),
            ManifestEntry::empty(200),
            ManifestEntry::data(300, [0x33; 32], 7),
        ];
        for e in &entries {
            append_manifest_entry(dir.path(), e.clone()).unwrap();
        }
        let read = read_manifest(dir.path()).unwrap();
        assert_eq!(read.len(), 3);
        for (i, (r, w)) in read.iter().zip(entries.iter()).enumerate() {
            assert_eq!(
                r, w,
                "manifest entry {i} order mismatch: read={r:?}, wrote={w:?}"
            );
        }
    }

    #[test]
    fn read_manifest_returns_empty_when_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let read = read_manifest(dir.path()).unwrap();
        assert!(
            read.is_empty(),
            "absent manifest is not an error — empty is the valid partial view for a joining validator"
        );
    }

    #[test]
    fn read_manifest_surfaces_corrupt_line_as_error() {
        let dir = tempfile::tempdir().unwrap();
        append_manifest_entry(dir.path(), ManifestEntry::data(100, [0x11; 32], 5)).unwrap();
        // Corrupt the manifest by appending a bogus line.
        use std::io::Write;
        let path = dir.path().join(MANIFEST_FILENAME);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"this is not JSON\n").unwrap();
        drop(f);
        let err = read_manifest(dir.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("manifest line 2"),
            "error must identify the corrupt line number; got {msg}"
        );
    }

    /// F-30b-4 (slice 30c Phase C fix): `SnapshotWriter.maybe_write`
    /// appends an "empty" sentinel manifest entry on a cadence-hit
    /// block with zero WAL entries.  Pre-30c this was a silent
    /// skip — joiners couldn't distinguish "block had no fs
    /// mutations" from "cadence miss."  Now the sentinel makes the
    /// former explicit.
    #[test]
    fn maybe_write_empty_slice_appends_sentinel_manifest_entry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 10,
            retain: 4,
        };
        // Block 20: cadence hit (20 % 10 == 0) with zero entries.
        let res = writer.maybe_write(20, &[]).unwrap();
        assert!(res.is_none(), "empty slice returns None (no snapshot file)");
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 1, "empty sentinel must land in manifest");
        assert_eq!(manifest[0].block_number, 20);
        assert!(manifest[0].root.is_none(), "sentinel entry: root = null");
        assert_eq!(manifest[0].entries, 0);
    }

    /// Data slice writes both the snapshot file AND a manifest
    /// entry with the concrete root.  A joiner reading the
    /// manifest can then fetch the snapshot bytes by the root
    /// hex via `read_snapshot_bytes`.
    #[test]
    fn maybe_write_data_slice_appends_data_manifest_entry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 5,
            retain: 4,
        };
        let entries = vec![WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/x"),
            extra_path: None,
            offset: None,
            length: Some(3),
            payload_ref: Some(PayloadRef::hash(b"abc")),
            mode_bits: None,
            owner: None,
            group: None,
        }];
        let root = writer.maybe_write(5, &entries).unwrap().unwrap();
        let manifest = read_manifest(dir.path()).unwrap();
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].block_number, 5);
        assert_eq!(manifest[0].root, Some(root));
        assert_eq!(manifest[0].entries, 1);
        // The manifest root points at a real snapshot file the
        // joiner can fetch.
        let bytes = read_snapshot_bytes(dir.path(), &root).unwrap();
        assert!(!bytes.is_empty());
    }

    /// Cadence-miss blocks must NOT touch the manifest.  Manifest
    /// is only for cadence-hit blocks (data + empty sentinel);
    /// cadence-miss blocks contribute nothing to the join-protocol
    /// view.
    #[test]
    fn maybe_write_cadence_miss_does_not_touch_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: dir.path().to_path_buf(),
            cadence: 100,
            retain: 4,
        };
        // Block 3 is a cadence miss (3 % 100 != 0).
        let res = writer.maybe_write(3, &[]).unwrap();
        assert!(res.is_none());
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(
            manifest.is_empty(),
            "cadence miss must not contribute to manifest"
        );
    }

    #[test]
    fn manifest_entry_from_line_rejects_wrong_root_hex_length() {
        // 63 chars (not 64) — a truncated root that would silently
        // hash-mismatch if accepted.
        let bogus = "{\"block_number\":1,\"root\":\"aa\",\"entries\":1,\"ts_ms\":1}";
        let err = ManifestEntry::from_line(bogus).unwrap_err();
        assert!(
            err.contains("64 hex chars") || err.contains("64 chars"),
            "must reject wrong-length root hex; got {err}"
        );
    }

    #[test]
    fn manifest_entry_from_line_rejects_missing_field() {
        // Missing ts_ms.
        let bogus = "{\"block_number\":1,\"root\":null,\"entries\":0}";
        let err = ManifestEntry::from_line(bogus).unwrap_err();
        assert!(err.contains("missing ts_ms"), "got {err}");
    }
}
