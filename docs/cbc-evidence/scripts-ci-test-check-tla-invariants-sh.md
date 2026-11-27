# CbC Evidence: scripts/ci/test-check-tla-invariants.sh

- **Status:** pending
- **Adapter:** manual
- **Commit:** fe64a7fe84eaaf633138cea93ba2b9b4441e5477
- **Verified:** 2026-11-26T23:43:46Z

Claim:

> The heartbeat wake models use the verification gate in docs/claims/soak-formal-gate.md.

```json
{
  "artifact": {
    "path": "scripts/ci/test-check-tla-invariants.sh",
    "id": "scripts-ci-test-check-tla-invariants-sh",
    "commit": "fe64a7fe84eaaf633138cea93ba2b9b4441e5477"
  },
  "claim": "The heartbeat wake models use the verification gate in docs/claims/soak-formal-gate.md.",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004",
    "CLAIM-CASPER-NODE-OBSERVATION-005",
    "CLAIM-SOAK-GATE-001"
  ],
  "adapter": "manual",
  "status": "pending",
  "previous_record": {
    "commit": "24703ab47f8a56f5ca61c2b669a0a9b259e06010",
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "sha256": "6a71303b85d61e074510868de8daec1a4b905c49605fe36d73f56c65001c3030",
    "status": "pending"
  },
  "evidence": {
    "kind": "formal-gate-registration-check",
    "ref": "formal/tlaplus/heartbeat_wake/README.md",
    "detail": "The local fixture accepted all 101 exact control verdicts and checked that all four heartbeat configurations run in both tiers. The fixture also exercised refusal and workflow routing. The separate pull request tier ran the pinned TLC jar and passed 19 clean configurations and 101 expected violations. These checks cover heartbeat registration. Earlier claim evidence remains at the previous record."
  },
  "waiver": null,
  "verified_at": "2026-11-26T23:43:46Z",
  "verification_id": "f13d0924-5481-4d0a-8a12-80a60da95e24",
  "inputs": {
    "artifact": "scripts/ci/test-check-tla-invariants.sh",
    "source_sha256": "98830ea9f2b08b237b718d39a438db3fd182646dacf0371609c726a7ae926f26",
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
