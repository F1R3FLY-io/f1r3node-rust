# CbC Evidence: scripts/ci/check-node-observation-bindings.sh

The corrected binding driver passed the recorded verification checks.
Named maintainer acceptance of the changed source remains pending.

## Source refresh on 2026-09-30

The [refresh report](runs/node-observation-ci-refresh-20260930-01/report.json) binds the current source to the verification results.
The report preserves the earlier record identity and its acceptance.
The current record remains pending until the applicable acceptance requirements are met.

```json
{
  "artifact": {
    "path": "scripts/ci/check-node-observation-bindings.sh",
    "id": "scripts-ci-check-node-observation-bindings-sh",
    "commit": "bc226f89b46366ea4cb0c1907d6781050afb7d0c",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "0894960136677e1efb0245bcb17240a0d6ce40155d5cb54e0e7487f27dcac317"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001"
  ],
  "status": "pending",
  "scope": "agent-b-ci-correction-refresh-20260930-01",
  "adapter": null,
  "evidence": {
    "kind": "source-bound-hosted-checks-and-elf-mutation-controls",
    "ref": "docs/cbc-evidence/runs/node-observation-ci-refresh-20260930-01/report.json",
    "sha256": "7e844fe3c4dfb287de00c093d355ceddf3cbb32209c699577b00eb3b5092bbb3"
  },
  "verification_status": "recorded",
  "acceptance_status": "pending",
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-30T04:44:34.480367+00:00",
  "limits": "The report states the verification limits. Earlier acceptance does not cover the corrected source.",
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-node-observation-bindings-sh.md",
    "commit": "bc226f89b46366ea4cb0c1907d6781050afb7d0c",
    "sha256": "732a0c5be21d878d59282e28e16773eb8a9a5b5abc63dcb1741c2d4ffcec125e"
  },
  "previous_artifact_sha256": "947937192ae8761330cd8b36b437edcd9e3256a53803477e7c076f9cd1c1e5ca"
}
```
