# Consensus-observable configuration invariants

This document enumerates every configuration knob whose value
shapes the bytes of a WAL entry or a handler's reply `Par` in
the file-I/O subsystem.  All validators in the same shard **must
agree on every value below**; disagreement produces divergent
WALs and fails RSpace rig verification at block replay.

## Why this matters

Consensus depends on all validators producing byte-identical
WALs and reply Pars from the same deploy sequence.  The Shape A
design already establishes this on the filesystem side —
bundle-relative paths in genesis, per-validator subdirs, a
resolver that remaps at syscall time.  This document generalizes
that discipline to every consensus-observable configuration
value.

Once a network runs, any change to any surface below is a hard
fork.  The `f1r3node` codebase does not currently ship a running
network — see `f1r3node_no_running_network` — so changes here
are free today, but they become a real coordination cost the
moment a shard is live.

## Surface

### 1. Per-op re-execute behavior (Phase 1–4)

Under Consensus mode, the follower re-executes each fs syscall
against its own subdir and verifies the fresh reply's hash
matches the leader's cached reply.  A Phase-4 validator running
against a Phase-0 validator would disagree on
`Failure { CONSENSUS_DIVERGENCE }` vs `Success` finalize on the
same divergent state.

Every Phase 1–4 slice sits on this surface:

| Handler | Phase | Reply shape | Verify shape |
|---|---|---|---|
| `fs_stat` | 1 | `[record]` | hash |
| `fs_open` | 2 | `[fd]` | H-6 finalize |
| `fs_close` | 2 | `[true]` | H-6 finalize |
| `fs_read` | 2 | `[true, bytes]` | hash |
| `fs_size` | 2 | `[u64]` | hash |
| `fs_entries` | 2 | `[[entries]]` | hash |
| `fs_seek` | 3 | `[u64]` | H-6 finalize |
| `fs_truncate` | 3 | `[true]` | hash |
| `fs_write` | 3 | `[true, u64]` | hash |
| `fs_write_at` | 3 | `[true, u64]` | hash |
| `fs_chmod` | 4 | `[true]` | hash |
| `fs_remove_file` | 4 | `[true]` | hash |
| `fs_rename` | 4 | `[true]` | hash |
| `fs_copy_file` | 4 | `[true, u64]` | hash |
| `fs_remove_dir` (non-recursive) | 4 | `[true]` | hash |
| `fs_remove_dir` (recursive) | 4-R5 | `[true, [[rel_path, kind], ...]]` | hash |

### 2. Reply-shape decisions

The exact `Par` structure returned by each handler is part of
the consensus protocol.  Changing the shape (e.g., adding a
field, changing element ordering, switching from absolute to
relative paths) is a hard fork.

Notable shape decisions currently in force:

- **fs_remove_dir recursive manifest** (R5): relative paths from
  the requested removeDir root.  Follower walks its own subdir
  and produces byte-identical relative-path manifest for verify.
- **fs_copy_file byte count**: `[true, n]` — content is not
  hashed in the reply (would double WAL entry width; source
  contents are deterministic on prior WAL ops under Shape A).
- **`payload_ref` conventions**: observation-op payload_refs are
  follower verification targets, NEVER peer-fetch targets.  See
  `fileio_observation_wal_semantics`.
- **fs_stat host-transient field strip**: `mtime`, `ctime`,
  `atime`, etc. are cleared from the reply before hashing to
  give follower/leader parity across differently-timestamped
  filesystems.

### 3. WAL entry serialization

The exact byte layout of `WalEntry` (op discriminant, field
ordering, path encoding, outcome encoding) as produced by
`encode_wal_slice` is consensus-observable.  Any change to the
serialization format requires all validators to upgrade in
lockstep.

Related structural invariants:

- Bundle-relative paths for Shape A byte-identity (Task 0.4
  discipline).
- `ack_channel_hash(ack)` as the WAL sidecar key so log-order
  drain can match produce events.
- `per_entry_ack_seed(ack, entry_path)` for recursive removeDir
  per-entry keys.

### 4. FSERR_CODE_* numeric values

The numeric codes in `errors.rs` (e.g., `FSERR_CODE_NOT_FOUND`,
`FSERR_CODE_CONSENSUS_DIVERGENCE = 13`) surface in
`WalOutcome::Failure { code }` and in `err` reply strings.
Renumbering ANY code is a hard fork.

### 5. Byte gates

Values that determine whether a syscall is accepted or rejected
at handler entry:

| Constant | Value | Effect on divergence if mismatched |
|---|---|---|
| `MAX_WRITE_BYTES` | 64 MiB | one side journals, the other returns QUOTA_EXCEEDED |
| `MAX_TRUNCATE_BYTES` | 16 GiB | same |
| `MAX_READ_BYTES` | 64 MiB | one side reads, the other returns QUOTA_EXCEEDED |
| `MAX_ENTRIES` | 65,536 | one side succeeds, the other truncates |

All caps are checked in the handler entry against argument
values.  If a validator upgraded to a higher cap accepts an
oversized syscall the rest of the shard would reject, the WAL
entry (or lack thereof) diverges immediately.

### 6. Cost-metering constants

Every `fs_*_cost()` function in `costs.rs` returns a `Cost` used
by `metering.reserve_primitive(...)`.  If a validator's cost
values differ, a deploy near its budget limit may OOG on one
validator and complete on another → divergent WAL suffix.

Also consensus-observable:

- `fs_remove_dir_per_entry_supplement_cost(n)` — per-entry
  supplement for recursive removeDir.
- `reserve_incremental_primitive` semantics (n=0 tolerance,
  etc.).

### 7. cmode dispatch rules

Whether a given cap + cmode combination:

- Triggers WAL journaling (Consensus writes; Oracular skips).
- Is banned outright, returning `FSERR_UNSUPPORTED` at handler entry.
  Currently banned under Consensus:
    - `fs_entries_stream_open` / `entriesStreamNext` /
      `entriesStreamClose` — readdir order is fs-dependent and not
      stable across per-validator subdirs (D3).
    - `fs_chown` — WAL captures owner/group as caller-supplied
      String values; NSS mapping to uid/gid is host-local and can
      differ across validators, producing silent on-disk uid
      divergence that the reply-hash verify cannot detect.
- Applies mode-differentiated gates like `Consensus + locked →
  FSERR_BUSY` on unlink / removeDir.

Changing any dispatch rule (adding a ban, removing a gate,
switching a per-cmode journal decision) is a hard fork.

## When editing any of the surfaces above

- Flag the change as a hard-fork boundary in the commit message.
- Bundle multiple related changes into one hard-fork slice
  rather than shipping incremental knobs across many commits.
- If a running network exists, coordinate a shard-wide upgrade
  window.  If not (current state), the change is free but
  should still be recorded here for future shard operators.

## Related documents

- `fileio_consensus_fs_shape_a` — bundle-relative paths + Shape
  A resolver.
- `fileio_wal_replay_verification_gap` — the Consensus-mode
  gap Phase 1–4 is closing.
- `fileio_observation_wal_semantics` — payload_ref semantics
  design decision.
- Plan: `plan-consensus-reexecute-verify.md` in the FIPS repo
  work-tracking directory.
