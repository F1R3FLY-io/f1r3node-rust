# Native control-directory validation

The native launcher must reject an untrusted control-directory path before it executes workload code.
The check applies to every parent component, not only the immediate parent.
The control directory must be new.

## Selected fault and correction

The fixture creates a root-owned immediate parent below a directory owned by UID 65534.
The committed launcher accepts that immediate parent and executes a harmless workload initialization hook as UID 65534.
The real driver subsequently refuses its run-domain record, but workload initialization has already occurred.
This observation is a launcher failure, not evidence that the driver admitted an iteration.

The corrected launcher checks each parent component from the filesystem root with `lstat`.
Every component must be a directory owned by root, without group or other write permission.
The launcher performs these checks before it creates the control directory or starts the service.
The existing canonical-path check also rejects symbolic-link paths.

The strict policy rejects writable ancestors even when they have the sticky bit.
Use a trusted `/run` directory chain for control records.
The separate source-directory checks remain unchanged.

## Correspondence

| Model element | Runtime observation |
| --- | --- |
| `AncestorTrusted = FALSE` | The immediate parent belongs to root, but one ancestor belongs to UID 65534. |
| `ValidateAncestors = FALSE` | The baseline checks only the immediate parent. |
| `Validate` | The launcher checks the control-parent chain before any launch operation. |
| `Launch` | The real gate drops privileges and executes Bash with the workload environment. |
| `workloadStarted` | The harmless Bash initialization hook records execution with UID 65534. |
| `UnrelatedWriterPreserved` | The unrelated process remains alive and continues to write before fixture cleanup. |

The parent-only control violates `UntrustedControlPreventsWorkload` with TLC exit 12 and three distinct states.
The corrected untrusted-path configuration passes with two distinct states.
The trusted-path configuration passes with three distinct states and fair completion.
The control checks only its named invariant.

## Runtime evidence

The regression fixture is `scripts/bench/test-soak-native-control-path.py`.
It invokes the public Python launcher directly, so the initialization hook cannot execute in a privileged wrapper shell.
The helper, service manager, launch gate, privilege drop, and driver remain real.
Only the harmless workload hook and external workload commands are substitutes.

The unchanged fixture reports RED against commit `f75af9ed3c5b4fbab049328326642f4484dbffb3` and GREEN against the corrected helper.
The query-refusal and native controller-loss fixtures also pass with the committed driver.
All three corrected cases pass with a separate B48 driver snapshot.
Both native controller-loss runs retain two normal recovery summaries with counters `[1,1,0,0]`.

The raw evidence root is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-native-control-WkVIMLiZ`.
The retrieval audit verifies 298 regular members from seven invocations.
The archive digest is `7031d6bfbfaca62db377dea37be00eaee2c7eae0d114d00339d9de628a3d5f07`.
Two FIFO entries are excluded, so the archive is not a complete filesystem image.
The first retrieval attempt had a path-list error and remains separate from runtime results.
The runner was observed `TERMINATED` after successful retrieval verification.

## Limits

The runtime fault covers one non-root ancestor beneath a trusted `/run` chain.
It does not test concurrent directory replacement, every permission combination, filesystem mount changes, or inherited descriptor access.
Trusted administrators must not replace the verified directories during launch.
The finite model abstracts path checks and does not prove filesystem behavior or implementation correctness.

Private Docker containment, creation fencing, durable publication, and aggregate deadline and reserve bounds remain open.
The workload, finalization semantics, 45-second wait, and harness pins remain unchanged.
This cycle does not run an acceptance soak or discharge a correctness claim.
B44, D2, shared gate registration, combined verification, and evidence publication remain with their recorded owners.
