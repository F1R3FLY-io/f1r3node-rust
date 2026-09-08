# CbC Evidence: TLA+ Gate Result Classification

**Claim:** [CLAIM-SOAK-GATE-001](../claims/soak-formal-gate.md)

**Status:** Pending. Cycle G0/B1 passes its behavioral checks, but the full gate claim is not discharged.

**Base revision:** `599513d4af1382e87787c2b2c746d0c887d3b738`

**Historical candidate:** The [B1 manifest](soak-g0-2026-09-08/manifest.json) records the content later committed as `7034e21683044fb1525d213d5a1ae2e16bbb57c0`.

B1 evidence applies to that source and claim revision, not to subsequent changes. [Cycle B2](github-workflows-slashing-tests-yml.md) records later workflow and gate verification.

## RED

The test executed the existing gate with a verifier-process fixture that returned a clean result for a carrier negative control.

The gate returned success because it did not execute that control. The regression test failed with this message:

```text
FAIL: The formal gate accepted MC_CarrierIndex_dag_first_pre_fix with result clean.
```

The [RED log](soak-g0-2026-09-08/gate-red.log) records the test failure. The manifest identifies the pre-fix script and the test's content.

## GREEN

The gate now executes both carrier negative controls. Each control requires TLC exit code 12 and its exact expected invariant violation.

One table-driven test checks seven outcomes for each control. The test accepts the expected violation and rejects these six outcomes:

- The control completes without an error.
- The control violates a different invariant.
- The verifier reports a tool error.
- The output names the expected invariant, but the exit code is incorrect.
- The verifier times out.
- The configuration is missing.

The [GREEN log](soak-g0-2026-09-08/gate-green.log) records the passing result. The main CI workflow now runs this regression test.

## Actual formal baseline

The jar matches the repository's pinned release and SHA-256 digest. Each run used one worker and a 60-second timeout in an isolated copy of the model files.

| Configuration | Result |
| --- | --- |
| `MC_CarrierIndex` | The baseline passed with 222 distinct states. |
| `MC_CarrierIndex_dag_first_pre_fix` | TLC reported `IndexCompleteForWindow` and exit code 12. |
| `MC_CarrierIndex_read_failure_pre_fix` | TLC reported `AbsenceProofSound` and exit code 12. |

The manifest links the full TLC logs, including both counterexample traces. These are existing bounded carrier properties, not new finalization or disk proofs.

## Regression checks

The shell syntax checks, workflow security checks, and release workflow tests passed. The manifest records retained check output.

The gate regression test and workflow checks passed again after shell formatting changed. The manifest records the updated source digest and revalidation time.

The full 17-configuration suite and Rocq suite were not rerun with real verifiers in B1. At the B1 checkpoint, the TLA+ workflow still skipped pull requests.

## Ledger record

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": null,
    "base_commit": "599513d4af1382e87787c2b2c746d0c887d3b738",
    "sha256": "1f6e0e4c9bd2bcf886f22801651022d6051bfd9a1d6cd8a4a9a1c55cb7a9a3a1",
    "id": "scripts-ci-check-tla-invariants-sh"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "behavior-tests+bounded-model-check",
    "ref": "docs/cbc-evidence/soak-g0-2026-09-08/manifest.json",
    "counterexample": "docs/cbc-evidence/soak-g0-2026-09-08/gate-red.log",
    "detail": "Direct TLC runs establish the existing carrier baseline only. No adapter discharged the gate implementation."
  },
  "waiver": null,
  "verified_at": null
}
```

## Discharge limit

The verifier-process fixture tests the shell gate, not the carrier algorithm. The real TLC runs test the carrier model, not the shell gate.

This distinction prevents the behavioral test from becoming proof authority. `CLAIM-SOAK-GATE-001` and `CLAIM-FINALITY-002` remain pending.
