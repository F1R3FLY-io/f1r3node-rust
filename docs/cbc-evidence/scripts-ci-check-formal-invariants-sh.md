# CbC Evidence: scripts/ci/check-formal-invariants.sh

- **Status:** pending
- **Adapter:** manual
- **Commit:** fe64a7fe84eaaf633138cea93ba2b9b4441e5477
- **Verified:** 2026-11-26T23:43:46Z

Claim:

> The heartbeat wake models use the verification gate in docs/claims/soak-formal-gate.md.

```json
{
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "fe64a7fe84eaaf633138cea93ba2b9b4441e5477"
  },
  "claim": "The heartbeat wake models use the verification gate in docs/claims/soak-formal-gate.md.",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003",
    "CLAIM-CASPER-NODE-OBSERVATION-005",
    "CLAIM-SOAK-GATE-001"
  ],
  "adapter": "manual",
  "status": "pending",
  "previous_record": {
    "commit": "24703ab47f8a56f5ca61c2b669a0a9b259e06010",
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "2d532ab5f7410cb2a5c556c5ba2a5b01587ec78085dcb175da86852e78c5cf89",
    "status": "discharged"
  },
  "evidence": {
    "kind": "formal-gate-registration-check",
    "ref": "formal/tlaplus/heartbeat_wake/README.md",
    "detail": "The full local Rocq suite compiled HeartbeatWake, passed the kernel check, and reported all seven added theorems closed. The other registered Rocq projects passed in the same run. These checks cover heartbeat registration. The previous record preserves the maintainer acceptance for the earlier source."
  },
  "waiver": null,
  "verified_at": "2026-11-26T23:43:46Z",
  "verification_id": "901f68c8-4737-46d9-95f7-24ea649584a4",
  "inputs": {
    "artifact": "scripts/ci/check-formal-invariants.sh",
    "source_sha256": "e0ccf0d7ae287f5fe94b8833d6cd86d6062a581617a4336393b6048e50674dba",
    "claim": {
      "path": "docs/claims/soak-formal-gate.md",
      "sha256": "8a4ea8738cdeb6e100d475c4f18eb33992a0dbeed56b929f69047ef3b7140b8a"
    }
  },
  "evidence_files": {
    "formal/tlaplus/heartbeat_wake/README.md": "7eb189905240a3b2c649d1eae109ad4456764f26021b1f0f935c97b3dd0fbd81"
  }
}
```
