# CbC Evidence: formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.cfg

This record registers existing source bytes after their implementation commit.
The registration gap remains explicit.
No verification or acceptance is claimed.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.cfg",
    "id": "formal-tlaplus-node-observation-MC-DisplayProjection-source-unsafe-cfg",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "d4f95ba1d03ee9aef051acb40570c1d8a008eef82b5568ce793fa689e07ec0a8",
    "sha256_at_registration": "d4f95ba1d03ee9aef051acb40570c1d8a008eef82b5568ce793fa689e07ec0a8",
    "sha256_before_verification_refresh": "d4f95ba1d03ee9aef051acb40570c1d8a008eef82b5568ce793fa689e07ec0a8"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "registered_at": "2026-09-30T22:43:21.676Z",
  "registration_gap": "source_existed_before_first_record_registration",
  "source_registration_checkpoint": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
  "previous_record": null,
  "evidence": {
    "path": "docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json",
    "sha256": "52f766491135dcf59b72d935e8ef697251fba1cebbe2a5d3f11ae5d47f87393a",
    "validation": "docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/validation.json",
    "acceptance": "pending"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-source-unsafe-cfg.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "a536c2c4d5b5f4bee92dd338bfcbb93cf314f7fa2979eafab8c86ad1506021cd"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal"
}
```
