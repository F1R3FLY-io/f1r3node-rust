# CbC Evidence: scripts/ci/check-formal-invariants.sh

- **Status:** pending
- **Adapter:** manual
- **Commit:** 61292b2b750b8a3ed00629cf481e0b4c03a5b014
- **Verified:** 2026-11-27T13:31:37Z

Claim:

> The heartbeat wake models use the verification gate in docs/claims/soak-formal-gate.md.

```json
{
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "61292b2b750b8a3ed00629cf481e0b4c03a5b014",
    "working_tree": true,
    "commit_is_base": true
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
    "detail": "The Rocq gate rejected a scratch Admitted declaration in each of its eight projects and refused a missing source directory. The clean run passed all eight builds and kernel checks. All 75 listed theorem checks reported closed assumptions. Earlier maintainer acceptance applies to the previous record."
  },
  "waiver": null,
  "verified_at": "2026-11-27T13:31:37Z",
  "verification_id": "56924fa9-cbaa-40ec-9927-131039072f1a",
  "inputs": {
    "artifact": "scripts/ci/check-formal-invariants.sh",
    "source_sha256": "902dd2a717231b4867e521fb371caacf069e87232ce120a37dde12792692b6e6",
    "claim": {
      "path": "docs/claims/soak-formal-gate.md",
      "sha256": "8a4ea8738cdeb6e100d475c4f18eb33992a0dbeed56b929f69047ef3b7140b8a"
    }
  },
  "evidence_files": {
    "formal/tlaplus/heartbeat_wake/README.md": "241beccc463e00d52a77c0c2171228fb09321f6c9282243fee8d880a10e3845d"
  }
}
```
