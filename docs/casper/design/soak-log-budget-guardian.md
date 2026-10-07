# Soak guardian: node log budgets

<!-- claude-session-f3cbc961 -->

Status: design, 2026-10-02. Implemented on 2026-10-04 on `chore/finish-TASK-020-4-log-growth`. See [Implementation notes](#implementation-notes-2026-10-04). Task: TASK-017-17, the EPIC-017 mirror of TASK-020-4.

## Problem

The host guardian in `scripts/run-merge-recovery-soak.sh` samples free disk space on the output directory and free host memory. It does not sample the two log sinks of a node container. EPIC-020 bounded both sinks at the source. The node file sink has a byte budget of 100 MiB for each file and 2 GiB for the log directory (TASK-020-2). The compose profiles cap the container `json-file` log at 3 files of 100 MiB (TASK-020-3).

CLAIM-SOAK-001 records these caps as assumptions. The guardian cannot detect a cap that fails, for example after a configuration drift or a node that writes to an unmanaged file.

## Design

The guardian samples two byte counts for each owned node container and stops the run when either count exceeds its budget.

| Probe | Source | Command |
|-------|--------|---------|
| Container log | The `json-file` log of the container and its rotated files | `docker inspect --format '{{.LogPath}}'`, then `stat -c %s` on the path and on `<path>.1` to `<path>.9`. The guardian uses `sudo -n stat` when the direct read is refused. A rotated file that does not exist is skipped. A rotated file that exists but cannot be read makes the probe unavailable. |
| Node log directory | `<data-dir>/logs` inside the container, the directory of the node file sink | `docker exec <id> du -sb /var/lib/rnode/logs` |

Owned containers are the containers with the label `io.f1r3fly.soak.owner=$SOAK_WRITER_OWNER`, the same selection that `stop_node_writers` uses.

### Budgets

| Variable | Default | Meaning |
|----------|---------|---------|
| `SOAK_CONTAINER_LOG_BUDGET_MB` | 400 | Budget for the container log of one container. The compose cap is 300 MiB. The slack covers one rotation in flight. |
| `SOAK_NODE_LOG_BUDGET_MB` | 2560 | Budget for the node log directory of one container. The node cap is 2048 MiB. The slack covers one rotation in flight. |

A value of 0 disables the probe. The values follow the same validation as the disk floor: a non-negative integer, within the 64-bit range.

### Breach rule

The rule is the disk floor rule. A sample over the budget counts one strike. Three consecutive strikes, or one sample at or above two times the budget, is a breach. A breach writes the host guardian breach record and stops the owned writers. It stamps the health tag with the probe name and the container, and then the guardian exits. The driver then publishes the protection breach and refuses further work, as it does for a disk breach.

The log probes run on every third guardian sample (15 s), the cadence of the OOM mark, because each probe is a Docker round trip.

### Probe availability

When a budget is above 0, the driver checks the probes against the running owned containers. The check runs before the opening benchmark and before each iteration. An unreadable probe refuses admission with a message that names the probe and the variable that disables it. This is the disk probe rule. When no owned container runs at the check, the check passes and the first guardian sample with containers performs it. An unreadable probe during execution is a breach with the reason `log probe unavailable`.

A container that leaves the running owned set between the listing and one of its probes is skipped. It no longer writes, and the next listing does not include it. The probe asks `docker ps` again with the owner label and the container ID. When that call fails, the probe failure stands.

### What stays unchanged

The disk floor, the memory floor, the hygiene band, the emergency deadline, and the stop path are unchanged. The node file sink and the compose caps are unchanged. The guardian adds a check. It does not replace a cap.

## Fixture

`scripts/bench/test-soak-log-budget.sh` runs the real driver in the disposable container of `test-soak-disk-admission.sh`, with fixture `docker`, `stat`, and `du` commands. Scenarios:

| Scenario | Fixture behavior | Expected |
|----------|------------------|----------|
| `within-budget` | Both probes under budget | Iteration admitted and completed |
| `container-soft` | Container log over budget on 3 consecutive samples | Breach record, writers stopped |
| `container-hard` | Container log at two times the budget on one sample | Breach on that sample |
| `node-soft` | Node log directory over budget on 3 consecutive samples | Breach record, writers stopped |
| `probe-missing-active` | `docker inspect` fails during an iteration | Breach with `log probe unavailable` |
| `probe-missing-boundary` | `stat` fails at the admission check | Admission refused, exit 2 |
| `sudo-fallback` | `stat` refuses, `sudo -n stat` answers | Sample accepted |
| `budget-disabled` | Both budgets 0, probes over any value | Iteration admitted |
| `budget-range` | A budget above the 64-bit maximum | Configuration rejected, exit 2 |
| `descriptor-exhaustion` | The fixture node writer holds descriptors up to its limit and keeps appending | The guardian probes stay bounded and sample. The writer's bytes are counted. A budget below the writer's output records a breach. |

The `descriptor-exhaustion` scenario runs the fixture writer under `ulimit -n`. It shows that the guardian does not depend on descriptors of the node process and that the sampling continues while the node is exhausted. The node's own byte budget under exhaustion is a node property with its regression tests in `shared/src/rust/tracing_init` (TASK-020-2). This fixture does not run a node binary.

## Claim update

CLAIM-SOAK-001 (`docs/claims/soak-disk-protection.md`) gets a row for the log caps. The guardian enforces the two budgets as a checked invariant, and the fixture scenarios are the evidence. The row replaces the assumption wording. The claim record of the driver and of the fixture follow the ledger cycle of TASK-017-13.

## Files

| File | Change |
|------|--------|
| `scripts/run-merge-recovery-soak.sh` | Budget parsing, the two probes, the admission check, the guardian samples, the breach path |
| `scripts/bench/test-soak-log-budget.sh` | New fixture, 10 scenarios |
| `.github/workflows/ci.yml` | One step next to `test-soak-disk-admission.sh` |
| `scripts/ci/check-casper-soak-bindings.sh` | The new fixture in the file list |
| `docs/claims/soak-disk-protection.md` | The log cap row |
| `docs/ToDos.md` | TASK-017-17 and the TASK-020-4 mirror pointer |

## Implementation notes (2026-10-04)

<!-- claude-session-aa467dea -->

The implementation follows the design with these changes:

| Topic | Design | Implementation and reason |
|-------|--------|---------------------------|
| Admission refusal | Exit 2 | A protection breach with exit 1, a summary, and `early_exit_reason=host_protection_breach`. This is the rule of the disk probe at the boundary. Exit 2 stays reserved for configuration errors, such as `log-budget-range`. |
| Node log probe | `docker exec <id> du -sb /var/lib/rnode/logs` | `docker exec <id> sh -c` tests the directory first. A missing directory counts as 0 bytes, because a node on the stdout sink writes no file log. |
| Fixture file | New `scripts/bench/test-soak-disk-admission.sh` sibling | The ten scenarios are in `scripts/bench/test-soak-disk-admission.sh` with a `log-` prefix. They reuse its disposable container, fake commands, and harness build. `.github/workflows/ci.yml` and `scripts/ci/check-casper-soak-bindings.sh` need no change. |
| Probe cadence | Every third guardian sample | `SOAK_LOG_PROBE_EVERY` (1 to 3, default 3). The fixture uses 1, so a soft breach takes about 15 seconds instead of 45. |
| Probe deadline | Bounded | `SOAK_LOG_PROBE_SECONDS` (1 to 4, default 4) bounds one sample of all containers. The guardian records progress before and after the sample, so the sample stays inside `SOAK_GUARDIAN_MAX_SILENCE_SECONDS`. |
| Strike count | For each probe | For each probe, on the largest value over the owned containers. |
| Stopped container | Not specified | Added on 2026-10-05 after the PR #622 review. The probe skips a container that stopped during the sample. Before, the failure was a breach, and a normal shard stop in an iteration could end the soak. Scenario `log-probe-vanished`. |
| Unreadable rotated file | Not specified | Added on 2026-10-05 after the PR #622 review. Before, the probe skipped an unreadable rotated file and reported a total that was too small. Scenario `log-rotated-unreadable`. |

The breach and refusal scenarios fail against the driver without the log guardian. The two scenarios of 2026-10-05 fail against the driver at 514eb3026. The disk scenarios run with both budgets at 0, so their behavior is unchanged.
