# TASK-020-2: Byte-bounded file logging

## Session

- Task: TASK-020-2, task 2 of 4 in EPIC-020.
- Implementer: `pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa`.
- Started: `2026-09-30T01:20:37Z`.
- Branch: `fix/node-log-and-accept-backoff`.
- Initial head: `350f24f5964f40bb6632a034fa647956ff8a8b13`.
- Status: Complete after local acceptance checks and strict completion.

## Plan

1. Record a failing byte-budget regression against the current writer.
2. Add per-file and directory byte limits while preserving period-based rotation.
3. Evict the oldest rotated logs before writes exceed the total budget.
4. Verify sustained writes, restart, retention, invalid configuration, and period-based rotation.
5. Update embedded defaults and check the node configuration.
6. Repeat strict task completion only after the acceptance checks pass.

## Design boundaries

The previous file sink used `tracing_appender` for asynchronous writes. Its rolling appender had no byte limit.

The logging implementation is `shared/src/rust/tracing_init/mod.rs`. `NodeConf` imports that shared configuration.

The corrected task scope includes the shared logging module and its bounded writer. It does not require a second node configuration type.

The [glossary](../../../Glossary.md#byte-budget) now defines byte budget and managed log. The story and flow retain separate local and deployment evidence boundaries.

The implemented defaults are 100 MiB per file and 2 GiB across the log directory. Period-based rotation and the retention count remain available.

The appender preserves unrelated regular files and includes their bytes in the directory budget. It rejects unsupported entries. It does not delete unrelated data.

A lifetime file lock prevents cooperating writers from sharing the same log directory. The data directory must remain trusted and exclusive.

Managed logs can be removed at startup or during retention. Previous oversized managed logs must not remain outside the new bounds.

A write larger than the per-file limit spans bounded files. A record within that limit moves intact to the next file when necessary.

Write failures stop file growth rather than weaken the limit. The existing asynchronous writer can lose output under queue pressure or write failure.

These changes do not cap stdout, container logs, or live soak output. Those obligations remain under TASK-020-3 and TASK-020-4.

## Progress

- [x] Confirm the current branch, task, implementation path, and file attributes.
- [x] Record the failing regression.
- [x] Implement the bounded writer and configuration.
- [x] Run shared and node verification.
- [x] Record final evidence and completion status.

## Git boundary

At implementation start, another participant had staged the TASK-020-1 repair. This session did not change its index entries.

That participant committed the repair as `b0afbe724c058c2357bf833f6f9ea1ac585f112a`. The commit changed documentation, not the source tested here.

This session created no commit, push, merge, or live deployment.

## Configuration and retention

The file sink accepts `logging.file.max-file-size-bytes` and `logging.file.max-total-size-bytes`. Both values must be positive signed-range integers.

Startup also requires the total limit to be at least the per-file limit. Old configurations receive the bounded defaults when the new fields are absent.

`rotation` retains its existing period choices. `retention` limits rotated files, while zero disables only that count limit.

The writer evicts the oldest rotated logs before an append exceeds the total limit. It includes the active file and unrelated regular files in that limit.

Managed names include `node.log`, timestamped legacy node logs, and the new numeric rotation names. The writer can remove oversized managed logs during startup.

Unrelated regular files remain intact. Symlinks, hard links, directories, or an unavailable writer lock cause refusal rather than unrestricted output.

The file lock is outside the log directory. Byte limits measure logical file lengths, not filesystem metadata, block allocation, or externally held deleted files.

The bound assumes that unrelated processes do not modify the trusted directory after startup. The file lock protects cooperating writers, not malicious filesystem changes.

## Verification results

The [evidence package](../../evidence/task-020-2-20260930-01/report.json) records source hashes, results, tool identities, and completion integrity.

| Check | Result |
| --- | --- |
| Original byte-budget regression | Failed with exit 101. One file grew to 40,000 bytes despite the requested 128-byte limit. |
| Initial node configuration checks | 12 passed and 1 failed. HOCON converted a negative size to an unsigned value. |
| Corrected size decoder | Reads a signed integer and rejects non-positive values. The negative HOCON test now passes. |
| Final shared, node, and comm nextest run | 781 passed, zero skipped, exit 0. |
| Shared tests | 123 passed, including 28 logging tests. |
| Node tests | 259 passed, including the embedded defaults and size override checks. |
| Comm tests | 399 passed on macOS. The Linux-only descriptor test is not compiled on this platform. |
| Doctests | One node doctest passed. Shared and comm had no doctests. |
| Direct Clippy | Shared and node, all targets, warnings denied: exit 0. |
| Rust format and Git whitespace checks | Passed. |
| Strict task completion | Grade full, zero gaps, exit 0, force disabled. |

The logging tests cover sustained errors, randomized write sizes, restart, period changes, count retention, oversized writes, intact JSON records, and eviction failures.

The tests also verify lock exclusion, invalid configuration, unrelated-file preservation, and refusal of symlinks and hard links.

Raw logs remain under `target/task-020-2-byte-bounded-logging/`. The evidence keeps the original failures rather than replaces them with passing results.

### Diagnostic limits

The initial missing-module finding occurred before the new source file existed. Fresh compilation and language-server probes confirm that the module now resolves.

The automatic Clippy adapter reported an unknown state. Its cached result remains inconclusive, not a successful check.

A fresh direct `cargo clippy --locked --release -p shared -p node --all-targets -- -D warnings` completed successfully against the final source.

Fresh language-server probes found no primary errors. Auxiliary findings include optional Rust 2024 syntax advice and an unchanged node re-export warning.

The broader cached analyzer sweep has stale line locations. This work does not claim that every auxiliary analyzer passed.

## Completion boundary

The unchanged completion helper ran in strict mode against a tracker copy. Only its final main invocation was omitted for the repository's `TASK-*` identifiers.

The successful completion fields were applied narrowly to TASK-020-2. The original libraries and helper identity remain recorded in the evidence.

TASK-020-1 ownership and verification fields remain unchanged. TASK-020-3 and TASK-020-4 remain pending, and EPIC-020 remains in progress.

These are local macOS results for the current source. No hosted Linux result, container-log cap, full disk-protection discharge, or live resource qualification is inferred.

The next task is TASK-020-3. It requires repository deployment changes and a separately coordinated system-integration change.

## Pare-back on 2026-09-30

The raw records of the evidence package `task-020-2-20260930-01` were removed on 2026-09-30 in the pare-back of the branch. Its `report.json` keeps the results and the digests of the removed files.
