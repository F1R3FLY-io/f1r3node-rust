# CbC Evidence: scripts/ci/check-tla-invariants.sh

- **Status:** pending (local and hosted execution green; required-check enforcement open)
- **Adapter:** embedded
- **Claim:** [CLAIM-SOAK-GATE-001](../claims/soak-formal-gate.md)
- **Verified:** locally 2026-09-08 and 2026-09-09; hosted 2026-09-08

## Cycles

| Cycle | Base | RED | GREEN |
| --- | --- | --- | --- |
| B1 classification | `599513d4a` | The gate accepted a clean `MC_CarrierIndex_dag_first_pre_fix` because it never ran controls. | Controls run in CI. Each must exit 12 with its exact invariant. A fixture test checks seven outcomes per control. |
| B2 routing | `7034e2168` | The TLA+ job skipped pull requests. | PR and push run `--soak-pr` (2 workers, 2 m per configuration, 15-minute job). Schedule and dispatch keep the full list and 240 minutes. TLC logs upload on every result. |
| B3 inventory | `43af06dab` | No candidate-bound claim inventory. | Superseded on 2026-09-09. The 256-file digest inventory and its CI step were replaced by [soak-disk-protection.md](../claims/soak-disk-protection.md), because any change to a digested file failed CI for every unrelated pull request. |

The B1 and B2 tests are merged into `scripts/ci/test-check-tla-invariants.sh`. It reads the control registry from the gate and checks 11 controls times seven outcomes plus six routing scenarios.

## Real TLC results

| Configuration | Result |
| --- | --- |
| `MC_ReplayHotLoop` | clean, 9 distinct states |
| `MC_CarrierIndex` | clean, 222 distinct states |
| `MC_CarrierIndex_dag_first_pre_fix` | `IndexCompleteForWindow`, exit 12 |
| `MC_CarrierIndex_read_failure_pre_fix` | `AbsenceProofSound`, exit 12 |
| `soak_disk` configurations | see [the driver record](scripts-run-merge-recovery-soak-sh.md) |

## Hosted execution

| Revision | Run | Job | Result |
| --- | --- | --- | --- |
| `43af06dab` (synthetic checkout `bad72c4c`) | 34228660038 | 102069244605 (TLA+) | 4 configurations as expected in 33 s; artifact 10056856375 |
| `9310ae2ce` | 34232911628 | 102109798473 (Lint) | inventory step passed |
| `d6aaba962` | 34244231314 and 34244230926 | 102122023044 (Lint) and 102121946596 (TLA+) | passed; artifact 10063312912, SHA-256 `67c33849b3cd439fb5e6ded2fd8294b29413086562b66d52d19b9045298252e0` |

## Open

- Rulesets `devProtect` (15773875) and `masterProtect` (14299997) require `Lint` only. `TLA+ invariant check` is not required. The classic-protection endpoint returned 403.
- GitHub treats skipped and neutral statuses as success. Success-only enforcement is unverified.
- Hosted runs do not independently attest the workflow-control SHA.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": "3d2aa7904",
    "id": "scripts-ci-check-tla-invariants-sh"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "behavior-tests+bounded-model-check+hosted-observation",
    "ref": "scripts/ci/test-check-tla-invariants.sh; hosted runs 34228660038, 34244230926",
    "counterexample": "a clean negative control passed the pre-B1 gate",
    "detail": "The fixture tests the shell gate, not the models. Real TLC runs test the models, not the gate. Required-check enforcement on dev remains a maintainer action."
  },
  "waiver": null,
  "verified_at": null
}
```
