# TASK-020-3: Deployment log caps

## Status

TASK-020-3 is complete on 2026-09-30. The local implementation passes its checks.
The external single-sink correction merged into system-integration `dev` through PR #146 at `ccd717195`. The [completion section](#completion-on-2026-09-30) records the details. The sections between this status and the completion section record the work in its sequence.

- Implementer: `pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa`.
- Clock checkpoint after claim: `2026-09-30T02:23:57Z`.
- Task: TASK-020-3, task 3 of 4 in EPIC-020.
- Branch: `fix/node-log-and-accept-backoff`.
- Initial head: `b0afbe724c058c2357bf833f6f9ea1ac585f112a`.
- Later source checkpoint: `6e1c8833a02b11d8145f9928fc41798f612b08a0`.

The initial claim record used `2026-09-30T01:59:20Z` without a clock check.
The tracker now records the observed checkpoint and identifies its timestamp source.
This timestamp does not describe a measured test start.

## Implementation

The six base Compose files contain twelve node service definitions.
All twelve commands now select `--log-sink=stdout` before `run`.
This root option overrides the sink in a mounted configuration.

The container caps already existed. This change preserves `json-file`, `max-size: 100m`, and `max-file: "3"`.
The two CI port overlays inherit these limits. No redundant cap block was added to an overlay.

The monitoring Compose file has no blockchain node service. Its collector storage policy is outside this task.

The repository guard uses the pure Rust `yaml-rust2` parser as a test-only dependency.
The lockfile adds four packages without changing an existing package version.
The hashbrown dependency graph also gains the parser's default hasher dependency.

The guard inspects tracked YAML files and resolves anchors, merge keys, and merge-sequence precedence.
It recognizes known node images and node run argument lists.
It also checks service overrides and the two supported CI overlay combinations.
New opaque image or build definitions still require review to establish their node identity.

The guard refuses these conditions:

- Missing driver or cap fields.
- Nonpositive, excessive, overflowing, or interpolated caps.
- Invalid option types.
- Missing, duplicate, dual, or misplaced sink arguments.
- Duplicate YAML keys, unresolved aliases, malformed merges, or multiple documents.
- Unmodeled Compose includes, service extensions, or logging tags.

The guard accepts an escaped `services` key and still rejects a missing cap below that key.
This control prevents a text-only discovery rule from hiding a deployment.

`scripts/ci/test-compose-log-policy.sh` independently renders six base profiles and two CI profiles with Docker Compose.
The script checks eighteen profile-specific node entries, including the six inherited CI entries.
Rendering does not contact a Docker daemon or start a container.
The script stores private rendered configurations below its output directory and prints only profile results.

The new node CLI tests establish the correct root-option position.
They also preserve the `file` and development-only `both` options.

The [logging documentation](../../../node/README.md#deployment-policy) defines one deployment sink and explains duplicate disk use.
It also documents the byte budgets and the limits of configured container caps.

## Verification

| Check | Result |
| --- | --- |
| Initial repository regression | Exit 101. Five guard tests passed and two deployment tests failed. |
| Final supply-chain suite | Exit 0. All 39 tests passed. No test was ignored. |
| Node and shared logging selection | Exit 0. All 33 selected tests passed. Nextest filtered out 352 tests. |
| Supply-chain Clippy | Exit 0 with `-D warnings`. |
| Node CLI-test Clippy | Exit 0 with `-D warnings`. |
| Docker Compose v5.3.1 | Exit 0 for all eight profiles. |
| Baseline comparison | Six comparisons passed against `6e1c8833a`. Only the root sink arguments changed semantically. |
| Cargo-deny 0.19.4 | Exit 0. Advisories, bans, licenses, and sources passed with the cached database. |
| Rust formatting and shell syntax | Passed. |

The native guard tests exercise 48 controlled refusals across policy mutations, YAML failures, inheritance, overlays, and escaped-key discovery.
These controls are repository regressions, not live fault injection.

The first Cargo-deny attempt placed `--disable-fetch` before `check` and returned exit 2.
The corrected command placed the flag after `check` and passed.
Both results remain in the private scratch evidence.

The Cargo-deny result includes permitted warnings for existing duplicate versions and interpreted scripts.
The result does not establish a complete supply-chain claim or a formal discharge.

The initial missing-module diagnostic occurred before the new support file existed.
A fresh compile and primary Language Server Protocol probe confirmed the module, so the stale finding was marked false-positive.

The YAML analyzer selected the Crystal `shard.yml` package schema for the Docker Compose file.
Its package-property findings were marked false-positive. Docker Compose independently accepted the deployment schema.
Existing long YAML lines were wrapped without changing parsed values. The baseline comparisons confirmed those values.

## External source correction

The inspected local system-integration checkout is stale at `962effd17708192627bd249362761c0ccb1fd5fa` and has unrelated edits.
Its missing-cap result does not describe current remote main.

Current remote main is `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.
Remote dev is `ef9844893f19df3e7523bb97e9e0da0ca241bb10`.
Main promotion [PR #145](https://github.com/F1R3FLY-io/system-integration/pull/145) merged on `2026-09-23T19:55:44Z`.

Docker Compose rendered all five remote node variants and confirmed their `100m` and three-file caps.
The variants contain eleven node service definitions.
The initial tracker count of ten was incorrect and is now corrected.

Remote `conf/rust.conf` still selects `sink = "both"` and identifies node files as the diagnostic source.
An external owner must review the log-reader contract before choosing the single sink.
Existing cap availability does not replace that correction or its merge evidence.

The [external handoff](../../../handoffs/task-020-3-system-integration-20260930.md) records the required change and acceptance boundary.
No receiver has claimed the external work. No external source or pin has changed in this session.

## Evidence and limits

The sanitized package is `evidence/task-020-3-20260930-01/`.
Private API responses, source snapshots, rendered configurations, and tool output remain under `target/task-020-3-deployment-log-caps/`.
The package does not publish raw configurations, keys, or private paths.

The source manifests bind this worktree state, not a future commit or CI run.
Configured container caps do not prove exact physical storage bounds.
Large events, filesystem allocation, metadata, and Docker behavior can change actual storage use.
No container fault experiment, hosted Linux run, live deployment, or guardian qualification occurred.

The changed source files have no `cbc=mandatory` attribute.
This work does not discharge `CLAIM-SOAK-001`, the proposed disk-protection claim, or any consensus claim.

TASK-020-1 ownership and verification fields remain unchanged.
TASK-020-2 remains complete. TASK-020-4 remains pending on its designated soak branch.
EPIC-020, US-009, and FLOW-002 remain open.
The external acceptance blocker prevents task completion, so no completion helper has run.

## Git boundary

Another participant committed TASK-020-2 and changed index entries during this session.
This session issued no `git add`, `git commit`, `git push`, checkout, synchronization, or merge command.
The existing task and evidence records were preserved.

## Pare-back on 2026-09-30

The raw records of the evidence package `task-020-3-20260930-01` were removed on 2026-09-30 in the pare-back of the branch. Its `report.json` keeps the results and the digests of the removed files.

## Completion on 2026-09-30

The external single-sink change merged into system-integration `dev`: PR #146, merge revision `ccd717195b35f75cef826f41d96b7028d8a874c0`, receiver `claude-session-fbb1f4d0`. The sink contract, the external verification, and the node checks at `7d64c9d03` are in the tracker entry and in the [hand-off document](../../../handoffs/task-020-3-system-integration-20260930.md). The task is complete. The `main` promotion revision is appended when it lands. TASK-020-4 stays open on `formal/soak-casper-consensus`.
