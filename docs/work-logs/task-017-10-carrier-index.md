# TASK-017-10: Carrier Index Profile

## Ownership

- Task: TASK-017-10.
- Epic: EPIC-017.
- Implementer: `pi-soak-carrier-index-linux`.
- Started: 2026-09-19T06:11:36Z.
- Branch: `formal/soak-casper-consensus`.
- Starting revision: `442e93faa`.
- Claim: `CLAIM-CASPER-SOAK-008`.

## Scope

This task implements and verifies the carrier-index harness profile. It does not change node behavior or discharge `CLAIM-FINALITY-002`.

The profile must compare matched candidate, DAG, scan-window, and availability inputs. Missing path receipts and counters must remain unknown.

Unavailable node interfaces block affected live scenarios. Controlled transcripts do not supply node evidence or authorize a soak run.

## Plan

- [x] Record and publish task ownership before implementation.
- [x] Implement the profile generator, collector, classifier, and executable fixtures.
- [x] Run bounded model controls and matching implementation tests.
- [x] Record source-specific evidence and remaining interface limits.
- [ ] Obtain the required binding acceptance before task completion.

## Coordination

The remote machine owns TASK-017-8. The proposed assignment for TASK-017-11 remains unconfirmed and unclaimed.

Profile-specific files belong to this task. Shared inventory and workflow changes require coordination before editing.

Exchange published commit IDs between machines. Use fast-forward-only pulls from clean checkpoints. Each new commit requires separate user approval.

## Current state

The profile implementation and bounded verification are ready for binding review. TASK-017-10 remains in progress, and CLAIM-CASPER-SOAK-008 remains pending.

The profile checks paired inputs, path engagement, counters, artifact identities, fault receipts, and restart links. Live, post-merge, and typed-identity execution remain blocked.

## Verification

| Check | Result |
| --- | --- |
| Host release fixtures | 23 tests, 88 cases, and 91 invocations passed. |
| Isolated Linux fixtures | The same 23 tests, 88 cases, and 91 invocations passed. |
| Clean bounded model | TLC generated 1,201 states and found 625 distinct states. |
| Three model defect controls | Each produced exit 12 with its registered invariant violation. |
| Shared manifest, model, inventory, and claim tests | 11 tests passed. |
| Renamed required fixture control | The runner rejected the replacement with exit 1. |
| Interrupted runner control | The runner recorded failure with exit 143, not a passing summary. |
| Formatting, targeted Clippy, shell syntax, and diff checks | Passed. |
| Accepted claims 001 through 004 | Each strict audit returned exit 0. |
| Claim 008 | The strict audit returned exit 4 because acceptance remains pending. |

The container used a read-only root, no network, no capabilities, an unprivileged user, and explicit resource limits. The evidence records its image and executable hashes.

Review tests exposed four defects before repair. These concerned malformed measurements, contradictory predecessor copies, absent fault schedules, and comparison without complete counters.

The repaired tests preserve independent product failures and reject unsupported comparisons. The final evidence validator checked 2,182 nested references.

## Retained failures

The first compile found an unsupported `Result` method. Its replacement compiled successfully. The initial debug fixture run exceeded its 180-second tool limit.

The timeout is not a passing test result. Subsequent release runs completed. The initial commit hook also rejected formatting before targeted formatting corrected the files.

TLC 1.8.0 could not run on the installed Java 8 runtime. The verified campaign uses the same pinned TLC 1.7.4 JAR as TASK-017-8.

The evidence retains failure logs and the four failing review tests. Not every earlier attempt has a retained source snapshot.

## Evidence and handoff

The [report](../casper/cbc-evidence/runs/casper-carrier-index-20260919-01/report.json) binds 22 source digests and 12 profile artifacts.

The package retains only its report, validation result, bundle digest, and redaction list in Git. Twelve canonical ledger records retain pending status without waivers.

The bulk bundle remains local at `/tmp/carrier-index-checks/casper-carrier-index-20260919-01.external.tar.gz`. Its SHA-256 is `51e7b6baae754ca393abe4f042dafefb86228a4c2d9a7b34a85ddcbbf43c1543`.

The draft release does not contain this bundle. Evidence publication requires approval and must occur before completion.

The new workflow has not run on hosted CI. Its proposed CbC tag requires human ratification. No shared workflow or attribute file changed.

The remote tracker owner must add the profile artifact inventory to the epic records. Binding review must precede claim discharge and task closure.

No node campaign, claim discharge, or task closure has occurred.
