# Claim: Soak Formal Gate Evidence

```yaml
claim_id: CLAIM-SOAK-GATE-001
status: pending
artifacts:
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/slashing-tests.yml
  - .github/workflows/ci.yml
  - scripts/ci/test-soak-claim-inventory.sh
  - docs/claims/soak-claim-inventory.json
references:
  - docs/plans/soak-recurrence-prevention-2026-09-08.md
  - docs/tdd-plans/soak-gates-2026-09-08.md
```

## Required behavior

The formal gate must not report success when a required configuration is absent, skipped, incomplete, or inconsistent with its expected result.

A positive configuration must complete without a verifier error. A negative control must produce the specified counterexample, not merely a nonzero exit code.

The carrier negative controls require these results from the pinned TLA+ model checker (TLC):

| Configuration | Exit code | Required invariant violation |
| --- | ---: | --- |
| `MC_CarrierIndex_dag_first_pre_fix` | 12 | `IndexCompleteForWindow` |
| `MC_CarrierIndex_read_failure_pre_fix` | 12 | `AbsenceProofSound` |

A clean negative control fails the gate. A different invariant, tool error, or timeout also fails the gate.

D1 adds `MC_SoakDisk_floor_only_pre_fix`. This control requires TLC exit 12 and the exact `AdmissionRequiresBand` violation.

Each required check must identify its candidate, specification, configuration, verifier version, assumptions, and evidence files. Candidate changes invalidate evidence that no longer matches those inputs.

## Current evidence

[Cycle B1](../cbc-evidence/scripts-ci-check-tla-invariants-sh.md) adds automatic carrier negative controls and tests the gate's result classification.

The test uses a verifier-process fixture. Real TLC runs separately confirm the carrier baseline and both existing counterexamples.

[Cycle B2](../cbc-evidence/github-workflows-slashing-tests-yml.md) enables the bounded formal tier for pull requests and pushes. Local workflow tests preserve the nightly and exhaustive configuration inventories.

The real bounded gate passed both baseline models and recognized both expected counterexamples. The [hosted observation](../cbc-evidence/soak-g0-b3-2026-09-08/hosted-observation.json) confirms the route and retains downloaded TLC logs.

[Cycle B3](../cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md) adds the [claim inventory](soak-claim-inventory.json) and checks its input digests. The inventory distinguishes baseline results from pending obligations.

The [D1 cycle](../cbc-evidence/scripts-run-merge-recovery-soak-sh.md) adds a production admission regression, a corrected disk model, and its historical negative control.

These cycles do not formally prove the shell implementation. They do not establish a residual finalization repair or a disk resource bound.

## Open obligations

- Required-check enforcement must reject missing, skipped, canceled, or stale formal results.
- The inventory must include each new required check when its development cycle starts.
- The complete acceptance identity must bind the workflow, node image, harness, configuration, workload, and attempt.
- Required-check results must bind to the tested candidate and attempt.
- The complete gate requires formal discharge or explicit reviewed treatment of its trusted implementation.

This claim remains pending. It does not change the status of `CLAIM-FINALITY-002` or the proposed `CLAIM-SOAK-001`.
