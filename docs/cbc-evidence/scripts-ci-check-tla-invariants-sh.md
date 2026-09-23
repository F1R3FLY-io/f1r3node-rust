# CbC Evidence: scripts/ci/check-tla-invariants.sh

- **Status:** pending (local and hosted execution green; required-check enforcement open)
- **Adapter:** embedded
- **Claim:** [CLAIM-SOAK-GATE-001](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/claims/soak-formal-gate.md), retired with the digest inventory and kept in the staging branch history
- **Verified:** locally 2026-09-08 and 2026-09-09, hosted 2026-09-08

## Cycles

| Cycle | Base | RED | GREEN |
| --- | --- | --- | --- |
| B1 classification | `599513d4a` | The gate accepted a clean `MC_CarrierIndex_dag_first_pre_fix` because it never ran controls. | Controls run in CI. Each must exit 12 with its exact invariant. A fixture test checks seven outcomes per control. |
| B2 routing | `7034e2168` | The TLA+ job skipped pull requests. | PR and push run `--soak-pr` (2 workers, 2 m per configuration, 15-minute job). Schedule and dispatch keep the full list and 240 minutes. TLC logs upload on every result. |
| B3 inventory | `43af06dab` | No candidate-bound claim inventory. | Superseded on 2026-09-09. The 256-file digest inventory and its CI step were replaced by [soak-disk-protection.md](../claims/soak-disk-protection.md), because any change to a digested file failed CI for every unrelated pull request. |

The B1 and B2 tests are merged into `scripts/ci/test-check-tla-invariants.sh`. It reads the control registry from the gate and checks 61 registered controls times seven outcomes plus six routing scenarios.

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

## Historical manifests

Retained outside Git by the agent that ran the cycles. The digests bind that raw store to this record. A regenerated manifest is a new record, not renewed verification.

| Manifest | SHA-256 |
| --- | --- |
| `soak-g0-2026-09-08/manifest.jsonc` (B1) | `b4e857a74cf4e091c7cff84a495e0db77d6a5ea3f2234f81078c2bdbe4ce0cde` |
| `soak-g0-b2-2026-09-08/manifest.jsonc` (B2) | `2e132518c37579b90c0c0a143e0ee553278e4b763562ee3b54eb3f6c1ea68a0e` |
| `soak-g0-b3-2026-09-08/manifest.jsonc` (B3) | `b73351d1b39df729dcc01014462249ddf456df896bacfa3865f10ea88810c28e` |
| `soak-g0-b3-2026-09-08/hosted-observation.jsonc` | `0c28cb6b5f83a005845153f0edc28fbc069d7fb85822b643f6faa9afd55927f6` |
| `g0-hosted-d6aaba962-2026-09-08/manifest.json` | `618bd14f61760e12959e97636a6c7a46a4e8cd2c7f15381859058d305db634c8` |
| `repin-962effd-2026-09-08/manifest.json` | `3858c683327fd88fff364fb1974adcadc748cea0464d1e26ea0989e406ca3e9e` |

## Claim separation

Node claims record their gate registration evidence in their own run packages. This record retains its primary claim and historical source identity.

The harness claim keeps its separate record under `docs/casper/cbc-evidence/`. Node claim digests must not replace or extend that record.

## Open

- Rulesets `devProtect` (15773875) and `masterProtect` (14299997) require `Lint` only. `TLA+ invariant check` is not required. The classic-protection endpoint returned 403.
- GitHub treats skipped and neutral statuses as success. Success-only enforcement is unverified.
- Hosted runs do not independently attest the workflow-control SHA.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": "3b1d2465a39b020426b3caa02bf5313d2b9fac5b",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "5ecaa38548460fb58bd5284838f586d53b8e632c6fe9f52e8b11727f91c5ab45",
    "id": "scripts-ci-check-tla-invariants-sh"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001"
  ],
  "previous_record": {
    "commit": "3b1d2465a39b020426b3caa02bf5313d2b9fac5b",
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "sha256": "0aa564b1f65b136429ef9f3291021c31a121c9555180e9958c5432d7bc4e1d7b"
  },
  "verification_scope": "historical-gate-evidence-not-node-claim-renewal",
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
