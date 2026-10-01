# CbC Evidence: Bounded Pull-Request Formal Checks

**Claim:** [CLAIM-SOAK-GATE-001](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/claims/soak-formal-gate.md)

**Status:** Pending. Cycle G0/B2 passes local verification. Hosted execution and required-check enforcement remain open.

**Base revision:** `7034e21683044fb1525d213d5a1ae2e16bbb57c0`

**Candidate identity:** The [manifest](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/cbc-evidence/soak-g0-b2-2026-09-08/manifest.jsonc) records the changed files and their content digests.

## RED

The regression test read the actual workflow and found that the TLA+ job excluded pull requests.

```text
FAIL: The TLA+ invariant job is not enabled for every pull request.
```

The [RED log](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/cbc-evidence/soak-g0-b2-2026-09-08/red.log) records exit code 1. This failure concerns workflow execution, not a carrier-model counterexample.

## GREEN

The existing `TLA+ invariant check` job now runs on every event configured for the workflow. Pull requests and pushes select `check-tla-invariants.sh --soak-pr`.

At cycle G0/B2 this tier contained two baseline configurations and both carrier negative controls. It uses two workers, a fixed two-minute configuration limit, and a 15-minute job limit.

The tier refuses exhaustive mode and requires a timeout command. Scheduled and manual runs retain the full default suite and the opt-in exhaustive configurations.

The workflow uploads TLC logs on success or failure. The artifact name includes the run ID and attempt.

The cycle's test read the real workflow command and executed that command with verifier-process fixtures. It checked these six scenarios:

- A pull request selects exactly four bounded configurations.
- A push selects the same bounded configurations.
- A schedule selects all 17 default positive configurations and both controls.
- A manual dispatch selects the default configuration set.
- An exhaustive dispatch includes the three additional configurations.
- A baseline invariant violation fails the pull-request command.

The [GREEN log](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/cbc-evidence/soak-g0-b2-2026-09-08/green.log) records the passing result. The main CI workflow also runs this regression test.

### Current tier

The gate grew after cycle G0/B2. The pull-request tier now runs 13 positive configurations and all 61 registered negative controls. The scheduled and manual tiers run 28 positive configurations and the same controls.

The regression test now reads the control registry from the gate. It checks classification for every control, routing for each workflow event, and registration of every pre-fix configuration in a registered area.

The current tier ran on a development machine in 53 seconds. No configuration took more than two seconds.

## Actual bounded verification

The real gate ran with the pinned TLC jar in an isolated copy of the source files. It completed in approximately 14 seconds.

| Configuration | Result |
| --- | --- |
| `MC_ReplayHotLoop` | The baseline passed with nine distinct states. |
| `MC_CarrierIndex` | The baseline passed with 222 distinct states. |
| `MC_CarrierIndex_dag_first_pre_fix` | The gate recognized the expected `IndexCompleteForWindow` violation. |
| `MC_CarrierIndex_read_failure_pre_fix` | The gate recognized the expected `AbsenceProofSound` violation. |

The [real gate log](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/cbc-evidence/soak-g0-b2-2026-09-08/real-gate.log) records the configuration results and timing. The manifest also links the complete model-checker logs.

The test fixture validates workflow selection and failure propagation. The real model checker validates the existing bounded models.

Neither result proves the whole finalization deadline, disk reserve, or shell implementation. The production models and their assumptions did not change.

## Regression checks

The B1 classification test, shell syntax checks, workflow security checks, release workflow tests, and release-gate tests passed. Language-server checks found no errors in the four checked source files.

The complete nightly suite and Rocq suite were not rerun with real verifiers. No hosted workflow or soak was dispatched for this cycle.

## Ledger record

```json
{
  "artifact": {
    "path": ".github/workflows/slashing-tests.yml",
    "commit": null,
    "base_commit": "7034e21683044fb1525d213d5a1ae2e16bbb57c0",
    "sha256": "460be11512b1f01f79387d7a3dad6c60d3c45627b5790436f9db6e165eac69fb",
    "id": "github-workflows-slashing-tests-yml"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "workflow-tests+bounded-model-check",
    "ref": "docs/cbc-evidence/soak-g0-b2-2026-09-08/manifest.jsonc",
    "counterexample": "docs/cbc-evidence/soak-g0-b2-2026-09-08/red.log",
    "detail": "Direct TLC runs establish bounded baseline results. Hosted execution and the complete gate claim remain open."
  },
  "waiver": null,
  "verified_at": null
}
```
