# CbC Evidence: scripts/ci/check-tla-invariants.sh

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "id": "scripts-ci-check-tla-invariants-sh",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "e66529c2356c4cbca0f10da9b3e72495450178f99f5c45699f7750a1083bf0b6",
    "sha256_at_registration": "ca116baad47417a16dc7ad4f60cacf18cdb54fb5e7916bbb174965459632a3ed",
    "sha256_before_refresh": "ca116baad47417a16dc7ad4f60cacf18cdb54fb5e7916bbb174965459632a3ed",
    "sha256_before_verification_refresh": "e66529c2356c4cbca0f10da9b3e72495450178f99f5c45699f7750a1083bf0b6"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "5fe4c4a0e9de7d303e017f121c270ef3df11a00d8d70a7d40e131bc6fafcca59"
  },
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
  "previous_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "c20e890a0430bad7e0ab6d70065368c54e11e8e2d3cd67902bf5d573a995cd41"
  },
  "claim_digests_before_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal",
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "845d77d915ca90f43ce8b3aa9efbed516f45442ef09a247233f3ce6c05403ea2"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  }
}
```
