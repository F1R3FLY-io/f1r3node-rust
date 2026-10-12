# TASK-017-12 Local Reservation Controls

**Status:** Local fixture checks passed. Verification and campaign integration remain pending.

**Owner:** `pi-soak-carrier-index-linux`.

**Execution base:** `9dd92a007b74047a03fa8b96135d6599e7575faf` on `formal/soak-casper-consensus`, with uncommitted additions.

## Scope

The user requested further work on the harness branch. The plan clarification preserves independent delivery of node PR #447 below harness PR #436.

The [reservation claim](../claims/casper-campaign-reservation.md) defines this implementation before its first test. Existing directory attributes mark both new Rust artifacts mandatory with high weight.

The standalone command creates a local owner-only store with three fixed slots. One slot covers preflight, and two slots cover the baseline candidates.

The store binds the campaign identifier, identity digest, approval digest, and preflight candidate. Supplied declarations do not authenticate approval.

Reservations reject occupied slots, reused run identifiers, reruns, unsupported stages, identity drift, unsafe roots, and malformed records. A missing preflight reservation blocks baseline reservation.

The command uses nonblocking directory locks and synchronized exclusive record publication. A failed launch or lost response does not refund its slot.

There is no reset, retry, replacement, release, or stability command. The implementation does not launch nodes or cloud instances.

## Verification

| Check | Result | Limit |
| --- | --- | --- |
| Initial missing implementation | Expected failure, exit 101 | The first test exercised a rejecting executable. |
| First implementation build | Failed, exit 101 | The crate does not enable Serde derive macros. |
| Corrected implementation | 15 tests passed | One helper test runs through its parent. |
| Final native suite | 16 tests passed | Tests use local synthetic records and real processes. |
| Final isolated suite | 16 tests passed | The container uses the same host-built arm64 executables. |
| Existing campaign checks | 115 checks passed | Valid declarations still produce blocked dispatch. |
| Existing host-control tests | Seven tests passed in isolation | One separate root-only test remains ignored. |
| All-target Clippy | Passed | This is not formal verification. |
| Workspace format check | Passed | This checks formatting only. |
| Rust language-server checks | Source confirmed clean, test checks timed out | Test diagnostic coverage remains incomplete. |

The implementation uses existing strict JSON and exclusive-publication helpers. No dependency, manifest, lockfile, accepted helper source, or workflow change was necessary.

The native whole-crate attempt stopped at four existing process tests that require a container. All four rejected before creating their fixture process.

The isolated run passed those process tests without weakening their safety check. The whole-crate native attempt remains a failure, not a full-suite pass.

The isolated container had no network, a read-only root, no capabilities, and a non-root user. Limits were 512 MiB, two CPUs, 128 processes, and 128 MiB temporary storage.

The container exited successfully without an out-of-memory event. Removal was confirmed. No independent rebuild or cross-architecture execution is claimed.

The tests cover competing processes, damaged records, discarded responses, and a killed lock holder. They do not simulate power loss or cloud API failures.

## Retained evidence

The [compact report](../casper/cbc-evidence/runs/casper-campaign-reservation-9dd92a007-01/report.json) records source identities, results, limitations, and pending acceptance.

Bulk evidence remains under `target/task-017-12/reservation-controls-9dd92a007-WAZfBH/`. It includes failed attempts, source copies, executable copies, native logs, container records, and campaign fixture artifacts.

Historical reports and existing campaign ledgers remain unchanged. The two new reservation records remain pending, with no waiver.

Default and canonical artifact gates each return exit 4 with two pending records. The STE Check passed without additional legacy findings. No human STE Review is claimed.

## Remaining integration

A local store does not prevent another controller from selecting another root. The future controller needs one authoritative store for the entire approved budget.

The controller must authenticate approvals and verify exact sources, images, workload pins, qualification, and passing prior results. A preflight reservation is not passing preflight evidence.

The controller must reserve before launch submission and retain uncertain outcomes without another submission. Instance lifetime enforcement must survive controller loss.

The command is not connected to the workflow or campaign dispatcher. Existing execution blockers remain unchanged, including global accounting and independent lifetime enforcement.

The source inventory for future execution must include this command and its dependencies. Its local receipt must not substitute for a qualified campaign result.

Node interfaces, claim acceptance, image publication, candidate repinning, OCI readiness, full baseline execution, and TASK-017-13/14 delivery remain separate requirements.

No commit, push, branch change, cloud launch, node launch, prover execution, hosted verification, or claim acceptance occurred.
