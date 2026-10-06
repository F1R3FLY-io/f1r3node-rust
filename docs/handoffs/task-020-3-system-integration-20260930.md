# TASK-020-3: System-integration single-sink change

## Status

TASK-020-3 is complete on 2026-09-30. The node repository implementation passes its deployment checks at `7d64c9d03`.

The receiver `claude-session-fbb1f4d0` claimed the external change in the system-integration tracker (section "REQUEST: one node log sink per deployment", id `SI-TASK-020-3`). Pull request F1R3FLY-io/system-integration#146 (branch `fix/single-log-sink-per-deployment`, base `dev` at `ef9844893`) merged into `dev` at `ccd717195b35f75cef826f41d96b7028d8a874c0` on 2026-09-30T22:10:44Z. The `main` promotion is pending.

| Deployment | Sink | Mechanism and reader |
|------------|------|----------------------|
| Compose variants and smoke-test CI | `stdout` | `sink = "stdout"` in `conf/rust.conf` and `conf/standalone-dev.conf`. Read by `docker logs` and `shardctl`. Bounded by `json-file` at 100m and 3 files. |
| Integration tests, Docker provider | `file` | `--log-sink=file` before `run` at 6 launch sites. Read from `/var/lib/rnode/logs/node.log*`. |
| Integration tests, subprocess provider | `stdout` | The conf. Read from the captured process output. |
| Development | `both` | Only through an explicit `--log-sink=both` before `run`. |

Verification at the merge revision: `unit-tests/test_log_sink_policy.py` 34 passed, 355 unit tests passed at the PR head, the 5 Compose variants render with 11 capped node services, and the live suites passed (`test_heartbeat` in PR CI, `test_token_metadata` and `test_shard_degradation` 15 of 15 locally). Node checks at `7d64c9d03`: compose log policy 9 services passed, `log_sink_cli` 3 passed, supply-chain repository tests 18 passed.

The byte limits of node commit `6e1c8833a` reach system-integration runs through the next node repin. TASK-020-4 enforces them in the harness. No repin occurred in this session.

## Source identities

Repository: `F1R3FLY-io/system-integration`.

| Source | Revision |
| --- | --- |
| Inspected local `dev` checkout | `962effd17708192627bd249362761c0ccb1fd5fa` |
| Remote `main` | `e3c4e14189f0c6ced2e9674487fcbdeffd93141b` |
| Remote `dev` | `ef9844893f19df3e7523bb97e9e0da0ca241bb10` |
| Main promotion PR | [PR #145](https://github.com/F1R3FLY-io/system-integration/pull/145) |

The local checkout has unrelated edits. Do not replace or synchronize that checkout without separate consent.

## Correction to the initial assessment

The old local checkout lacks the container caps. Current remote `main` already contains the required `json-file` caps.

Docker Compose v5.3.1 rendered all five remote variants successfully without starting a container:

| Compose source | Node services | Configured cap |
| --- | ---: | --- |
| `compose/f1r3node-rust.yml` | 5 | `100m`, three files |
| `compose/f1r3node-rust-shard-light.yml` | 3 | `100m`, three files |
| `compose/f1r3node-rust-standalone.yml` | 1 | `100m`, three files |
| `compose/f1r3node-rust-observer.yml` | 1 | `100m`, three files |
| `compose/f1r3node-rust-validator4.yml` | 1 | `100m`, three files |

These five variants contain eleven node service definitions.

The standard shard source has blob `0883b8148463ff86d82cb13d3eaf590ae38d8b3c` on both inspected remote branches.

The remote `conf/rust.conf` still selects `sink = "both"`.
Its comments identify the file sink as the diagnostic source.
Its blob is `3dd6305060d6712d268c4456475a7864ea85b06e`.
Its SHA256 is `cb916b00e0c9255259c2709b6b67762183f40e5edb404b69b593cfe3912f7c2c`.

PR #145 merged on `2026-09-23T19:55:44Z` at the recorded main revision.
This promotion proves cap availability. It does not prove a merged single-sink correction.

## Required change

1. Review the integration-test log readers and diagnostic scripts before changing their log source.
2. Select one deployment sink in `conf/rust.conf` or the deployment root arguments.
3. If readers require node files, select `file` and preserve their file access.
4. Otherwise, select `stdout` and use the existing bounded container output.
5. Keep `both` available only as an explicit development override.
6. Preserve `100m` and three files in every node Compose variant.
7. Add regression tests for the chosen sink and the log-reader contract.
8. Verify the new node image contains the TASK-020-2 byte-bounded writer before relying on file budgets.
9. Record the reviewed external change and its actual merge revision.
10. Return source hashes, commands, test results, and the selected log-reader contract to the node task owner.

Put `--log-sink` before `run` when the deployment uses a root argument.
The node CLI rejects this flag after the run subcommand.

The node file sink defaults to 100 MiB per file and 2 GiB for its log directory.
Period rotation and archive count do not replace these byte limits.

## Acceptance boundary

Do not mark TASK-020-3 complete until the external single-sink change has a verified merge revision.

Do not substitute a current cap snapshot for the missing single-sink change.
A rendered configuration does not establish live storage use or guardian enforcement.

Commits, pushes, merges, pin updates, and live runs need separate authorization.
TASK-020-4 remains on `formal/soak-casper-consensus` and is not part of this handoff implementation.

## Evidence

- [Node work log](../work-logs/task-020-3-deployment-log-caps-20260930.md).
- [Node logging policy](../node/README.md#deployment-policy).
- [EPIC-020 tracker](../ToDos.md#epic-020-node-log-and-accept-path-self-limits).
