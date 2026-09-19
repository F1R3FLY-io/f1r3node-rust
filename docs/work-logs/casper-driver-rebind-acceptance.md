# Casper Driver Binding Acceptance

## Approval and source

The user replied `approved` to the request for acceptance of the repaired driver's bounded H01–H10 binding review.

This approval applies only to CLAIM-CASPER-SOAK-001 in the pre-merge phase. It does not approve profile claims, node soaks, or post-merge execution.

The published baseline is `f9273621c8887947b56d0093a71486338312138e`. All 67 source hashes matched the reviewed package before acceptance metadata changed.

The [reviewed package](../casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/report.json) retains its pending labels and exact evidence.

The [acceptance package](../casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01/report.json) records the new approval and source-specific discharge.

The earlier 37 canonical ledgers remain in the reviewed package's `previous.tar.gz`. The new records identify that archive instead of rewriting history.

## Accepted boundary

The acceptance covers the repaired process-control bindings and exact handled-exit marker checks. It retains the original finite model and containment assumptions.

The model bounds are two candidates, two segments, four total iterations, and one active child. Construction is not applicable.

The owner scan does not atomically inventory future processes. B44, kernel assumptions, and Docker daemon containment limits remain unchanged.

Profile claims 002 through 008 remain pending. Soak status remains pending. TASK-017-4 remains complete without a new task closure.

## Acceptance-state packaging

The claim inventory includes `scripts/bench/test-soak-disk-admission.sh`. The container copy list previously omitted this artifact.

The runner now copies that declared input for the source audit. This packaging change does not alter driver behavior or test assertions.

The acceptance package records the exact packaging delta separately from the approved runtime source. Fresh isolated checks verify the accepted state.

## Fixture readiness and final checks

The first acceptance-state run exposed an intermittent OOM fixture failure. The expected setting remained zero.

A separate startup probe ran 1,000 controlled children. Their owner markers were initially unavailable in 28 cases. All 28 markers appeared after a delay.

The fixture now waits for an explicit child readiness receipt before testing process ownership and OOM preference. Its assertions remain unchanged.

Production code outside `cfg(test)` is byte-identical. The owner scan still does not guarantee complete discovery during concurrent process startup or execution changes.

All 50 repeated unprivileged runs passed seven tests each. The separate root run passed all eight tests.

The final accepted-state run passed 29 tests, including all three claim-audit regressions. It verified 48 cases and 91 driver invocations.

The root-only test was intentionally ignored in the unprivileged run. Its separate root run supplies the required permission checks.

The strict CLAIM-CASPER-SOAK-001 audit returns exit 0. The full bundle remains pending and returns exit 4.

The package retains the initial failed run, its source snapshot, the startup probe, and the successful checks. No failure evidence was replaced.

No node campaign, external repin, waiver, commit, or push is authorized by this acceptance.

## Drift after acceptance (2026-09-18)

The harness source `scripts/casper-soak/src/host_control.rs` changed after this acceptance. The `execute` function moved above the test module inside the linux module, because clippy denies items after a test module and CI runs clippy with warnings denied. The change does not alter behavior. The accepted digest is `fb6dafc47a5d…` at commit `f9273621c`. The current digest differs.

The claim audit refuses a discharged record whose source differs, so CLAIM-CASPER-SOAK-001 and the host-control ledger record return to pending. The other records keep their accepted digests, which still match. A new acceptance on the current source restores the discharge. The acceptance package `casper-driver-rebind-acceptance-20260918-01` stays as the record of the earlier acceptance.

## Refresh acceptance (2026-09-19)

The user replied `approved` to the request for acceptance of the refreshed binding on 2026-09-19. The approval covers only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

The refreshed baseline is `ab682eea1760c50867dcf7416e37f155b63e5dbc`. The only accepted source that changed is `scripts/casper-soak/src/host_control.rs`. Its production region, with the test module removed, is byte-identical to the accepted file at commit `f9273621c`.

The [refresh package](../casper/cbc-evidence/runs/casper-driver-refresh-acceptance-20260919-01/report.json) records the approval, the source digests, the production-region comparison, and the isolated binding check. That check passed on this baseline with 91 driver invocations across 48 registered cases.

The model bounds, B44, kernel assumptions, and Docker daemon containment limits remain unchanged. Profile claims 002 through 008 and the soak status remain pending. No node campaign, external repin, waiver, commit, or push is authorized by this acceptance.
