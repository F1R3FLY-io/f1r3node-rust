# TASK-017-4 Bounded Harness Acceptance

## Authorization

The user approved the bounded H01–H10 binding review in `task-017-4-final-checks.md` after requesting TASK-017-4 completion.

The approval preserves the stated containment limits. Profile verification and node verification remain pending. This approval is not a waiver.

## Accepted evidence

The implementation baseline is `946743a7740e5dd3c0816263c3347e501f2d50d7`.

Publishing changed five Rust files through formatting only. A fresh isolated run verified the committed source with 22 tests and 91 invocations across 48 cases.

The earlier package retains the clean Casper model, ten negative controls, shared gate results, legacy fixtures, disk fixtures, and failure history.

The acceptance combines that evidence with the approved binding review. The inventory checker and claim auditor do not supply formal proof by themselves.

The [acceptance package](../casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/report.json) records exact source hashes and the accepted claim digest.

The original verification plan remains an unchanged input. This acceptance supersedes its pending harness-binding label, not its pending profile-verification labels.

## Acceptance-state checks

The strict claim audit passed after the approval records were installed. The first container run then exposed missing audit inputs.

The container package now includes the claim artifacts, canonical ledgers, and referenced reports. An intermediate run identified one additional missing model-runner wrapper.

The auditor also preserves the existing lowercase Cargo ledger filename on case-sensitive systems. It retains all digest, identity, phase, and tier checks.

The final isolated run passes all 22 tests and 91 invocations with the discharged claim installed. Host regressions and formatting checks also pass.

These two support-file changes do not change the harness runtime. Their ledger records identify the implementation base and exact uncommitted source hashes.

The acceptance package retains the failed attempts and the successful acceptance-state run separately. No test assertion or strict completion gate was removed.

## Boundary

CLAIM-CASPER-SOAK-001 is discharged for bounded pre-merge harness behavior. Construction is not applicable. The soak status remains pending.

The model covers two candidates, two segments, four total iterations, and one active child. It does not prove unbounded liveness or node correctness.

Cooperating terminal writers must use the transition lock. B44, storage assumptions, and inherited containment limits remain unchanged.

Controlled Docker-boundary fixtures do not verify the Docker daemon. This acceptance does not authorize node dispatch, candidate repinning, or post-merge execution.

Claims 002 through 008 and the other tasks retain their existing states. Shared artifact ledgers retain their separate obligations.

## Closure

The strict claim audit and integrity check passed. The repository completion adapter marked TASK-017-4 complete with no completion gaps.

The acceptance package records the completion result separately from verification and user approval. Git publication requires separate authorization.

## Drift after acceptance (2026-09-18)

The driver `scripts/run-merge-recovery-soak.sh` changed after this acceptance. The Python removal replaced its pidfd process control with bash, and a later fix made the owner-marked stop kill processes in numeric pid order. The accepted digest is `7f4ba9b1c907…` at commit `946743a77`. The current digest differs.

The claim audit refuses a discharged record whose source differs, so CLAIM-CASPER-SOAK-001 and the driver's ledger record return to pending. The other 32 records keep their accepted digests, which still match. A new acceptance on the current driver restores the discharge. The acceptance package `casper-task-017-4-acceptance-01` stays as the record of the earlier acceptance.
