# Casper Driver Source Repair

## Current status

The user has accepted this bounded binding review. The [acceptance record](./casper-driver-rebind-acceptance.md) records the source-specific discharge.

The sections below retain the pre-acceptance review and its pending results. The original evidence package remains unchanged.

## Scope

The user requested a proper repair of the stale CLAIM-CASPER-SOAK-001 binding.

The published driver rewrite changed process control and trusted-record access. The previous acceptance did not cover those source bytes.

TASK-017-4 remains complete for its accepted source. The repaired source requires a new binding review. No earlier approval is transferred to this repair.

## Repair

The Rust harness now supplies `host-control` operations. The Bash driver uses the same harness executable for these operations.

A process file descriptor (pidfd) identifies a specific Linux process. UID means user ID. OOM means out of memory.

- Process signals use Linux pidfds instead of a PID check followed by a numeric signal.
- Process exit checks use kernel polling. Read errors cannot establish process exit.
- OOM preference writes use the retained process-directory descriptor.
- Owner checks read the effective UID from the process status. A hidden process environment produces an unconfirmed result.
- Domain records use descriptor-relative traversal, root ownership checks, non-writable ancestors, and bounded reads.
- Record parsing rejects duplicate keys and Boolean UIDs.
- Crash monitoring requires a matching process identity before its readiness receipt.
- A handled-exit marker must contain exactly `handled\n`. Extra bytes, including NUL bytes, do not suppress cleanup.
- Lifecycle manifests pin the new helper source through the existing compiled-source inventory.

The driver refuses work if the Rust harness or Linux pidfd support is unavailable. The workflow builds the harness before driver execution.

Disk fixtures copy the static harness into their isolated containers. The binding runner also executes the new host-control tests.

The runner collects all suite results before reporting failure. It does not omit the driver suite after an earlier claim-audit failure.

## Verification history

The initial unprivileged OOM fixture failed. The process-directory ownership check could omit a process. The repaired check reads the effective UID instead.

The first full binding run also found a fixture packaging error. The interruption mock lacked the new harness unit-test artifact.

The mock now supplies that artifact. The interruption tests still require stop, capture, and removal in that order.

The old ledger also caused the expected source-audit failure. That failure remains in the retained history.

The current claim now records pending review instead of an unsupported discharge. No test assertion or strict discharge requirement was removed.

The [evidence package](../casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/report.json) retains exact source hashes and results.

| Check | Result |
| --- | --- |
| Lifecycle and shared regressions | 22 tests passed, 48 cases, 91 driver invocations |
| Unprivileged host controls | Seven tests passed. The root-only test was ignored. |
| Root-owned host controls | All eight tests passed, including the root-only test. |
| Disk fixtures | All 42 isolated scenarios passed. |
| Existing profile regressions | All 21 tests passed on Linux. |
| Lifecycle model | 43,424 distinct states, 66,208 generated states, and ten expected negative controls |
| Exact exit marker | The published version failed. The repaired version passed. |
| Strict claim audits | Claims 001 and 004 return exit 4 with pending reports. |

The source-audit error is resolved. The strict gate remains closed because the new binding review is pending.

Canonical acceptance ledgers remain unchanged. The package supplies separate pending candidate ledgers for the repaired source.

The marker regression executes the extracted crash-monitor function with a controlled exit receipt. It does not test real process death.

Separate kernel tests cover pidfd exit monitoring. They do not force numeric PID reuse or prove the Linux kernel.

## Pending binding review

| Binding group | Repair evidence |
| --- | --- |
| H01 identity | The compiled-source inventory includes `host_control.rs`. The existing executable, configuration, and identity refusal fixtures pass. |
| H02 history and H03 failure retention | The unchanged journal and verdict paths pass the resume and resource-stop fixtures. |
| H04 evidence and H08 unknown observations | Missing measurements remain non-passing. An unknown process-control result cannot establish successful termination. |
| H05 terminal launch control | The transition-lock and active-stop fixtures pass. The driver requires a working pidfd probe before workload launch. |
| H06 capture before cleanup | Both interruption fixtures pass. The exact-marker regression prevents malformed markers from suppressing cleanup. |
| H07 policy and H09 merge gate | Existing policy and post-merge refusal fixtures pass. No live admission rule changes. |
| H10 exact controls | The clean TLC run and ten named negative controls meet their required exits and trace checks. |

These results support a bounded review. They do not transfer the earlier source-specific approval.

## Limits

All process fixtures execute in disposable Linux containers. These containers have no network, host mounts, Docker socket, or host PID namespace.

A separate root-owned container tests trusted-record permissions. It has no capabilities and launches only fixture processes.

These tests do not prove Docker daemon containment, systemd containment, or complete termination after simultaneous driver and monitor death.

The owner scan is not an atomic inventory of all future processes. Kernel pidfd and procfs semantics remain platform assumptions.

The finite lifecycle model and its original bounds remain unchanged. Construction remains not applicable. Node correctness and node soaks remain outside this repair.

No node campaign, external repin, policy activation, waiver, commit, or push is part of this repair.
