# Claim: Soak Formal Gate Evidence

```yaml
claim_id: CLAIM-SOAK-GATE-001
status: pending
artifacts:
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/slashing-tests.yml
  - .github/workflows/ci.yml
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

Each required check must identify its candidate, specification, configuration, verifier version, assumptions, and evidence files. Candidate changes invalidate evidence that no longer matches those inputs.

## Current evidence

[Cycle B1](../cbc-evidence/scripts-ci-check-tla-invariants-sh.md) adds automatic carrier negative controls and tests the gate's result classification.

The test uses a verifier-process fixture. Real TLC runs separately confirm the carrier baseline and both existing counterexamples.

[Cycle B2](../cbc-evidence/github-workflows-slashing-tests-yml.md) enables the bounded formal tier for pull requests and pushes. Local workflow tests preserve the nightly and exhaustive configuration inventories.

The real bounded gate passed both baseline models and recognized both expected counterexamples. Hosted confirmation of the changed workflow remains pending.

Neither cycle formally proves the shell implementation. Neither cycle establishes a residual finalization repair or a disk resource bound.

## Open obligations

- A hosted pull-request run must confirm the new bounded route.
- Required-check enforcement must reject missing or skipped formal results.
- The claim inventory must identify every required check and pending obligation.
- Required-check results must bind to the tested candidate and attempt.
- The complete gate requires formal discharge or explicit reviewed treatment of its trusted implementation.

This claim remains pending. It does not change the status of `CLAIM-FINALITY-002` or the proposed `CLAIM-SOAK-001`.
