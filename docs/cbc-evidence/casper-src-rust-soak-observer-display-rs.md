# CbC Evidence: casper/src/rust/soak_observer/display.rs

This record registers existing source bytes after their implementation commit.
The registration gap remains explicit.
No verification or acceptance is claimed.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "casper/src/rust/soak_observer/display.rs",
    "id": "casper-src-rust-soak-observer-display-rs",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "e86bf8c6df02c037b70df94e1140f72b5d33d55e7806db7859b5669dfa1d2e0b",
    "sha256_at_registration": "0ebe8feb3acf27c1dd2acf8f2c4b3af0eaf35fbef58a91b7a8f768eae77fe87a",
    "sha256_before_verification_refresh": "0ebe8feb3acf27c1dd2acf8f2c4b3af0eaf35fbef58a91b7a8f768eae77fe87a"
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
    "path": "docs/cbc-evidence/casper-src-rust-soak-observer-display-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "63105f9ea71f1012da3e234cfc61be5e7d45a1ba26d49ef2b44400c2f006401b"
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
