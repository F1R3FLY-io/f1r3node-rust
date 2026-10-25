# Soak Segment Cleanup Contract

```yaml
claim_id: CLAIM-SOAK-CLEANUP-001
status: pending
scope: bounded-harness-cleanup
construction: not-applicable
refutation: pending
binding: pending
acceptance: required
soak: pending
```

## Required behavior

A completed segment stops readable owned writers before its owner marker changes.
A completed segment also stops visible same-user writers whose arguments or working directory name its subprocess data directory.
Clearing the pytest PID does not remove either cleanup obligation.
An unrelated process outside both ownership scopes remains alive.

An unconfirmed stop writes writer-stop-failure.txt and makes the saved failure count at least one.
A repeated unconfirmed stop preserves a higher failure count.
A later segment preserves the earlier stop reason and cannot erase failure information.
Evidence capture precedes any deletion of evidence sources.

Launch requires current verification and independent signed acceptance for the actual control sources.
Candidate node sources cannot establish acceptance for control sources.
A source, claim, or evidence change invalidates the corresponding acceptance.
Missing policy, records, trust configuration, or signatures prevents launch.

The launcher uses the pinned shared CbC runtime from an external cache.
Missing or modified runtime files prevent verification and launch.
The runtime lockfile records an immutable revision and the expected source hashes.
Verification binds that lockfile and the validated runtime identity into the evidence.
Repository source contains no shared-runtime copy.

## Bounds and assumptions

The bounded model covers two segments, one owned writer, one residual writer, and one unrelated process.
Each segment can end after a deadline, a checkpoint, finalization, or a protection breach.
The Linux fixture uses pidfds and runs as an unprivileged user in an isolated container.
The host-control implementation can inspect the claimed processes under the stated visibility assumptions.
The kernel schedules the cleanup process and delivers supported signals within the configured budgets.

An owned process with an unreadable environment can escape the owner scan.
A residual process with unreadable arguments and working directory can escape direct termination.
The two-second write check cannot detect every intermittent writer.
After a protection breach, the residual stop skips that write check to preserve the emergency deadline.
These accepted limits remain explicit and do not establish complete host containment.

## Evidence and correction

The verification runner records source hashes before it runs tests.
The report records expected failures for the permanent negative controls.
Each artifact ledger binds this specification and the report to the current source bytes.
A passing fixture is verification evidence, not maintainer acceptance.
The separate acceptance record authenticates the source, claim, and evidence bindings.

The reusable artifacts are the bounded model, two-segment fixture, negative controls, and launch gate.

## Current verification package

The [verification report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/report.json) records the checked source bytes and outcomes.
The [status report](../casper/cbc-evidence/runs/soak-cleanup-20261025-01/status.json) records verification-time acceptance and independent claim gaps.
Unsigned payloads in that package require independent maintainer review and signatures.
