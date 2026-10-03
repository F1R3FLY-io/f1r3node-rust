# CbC Evidence: scripts/ci/test-check-tla-invariants.sh

The merge `c9ca12821` combined the changes of both branches to this script. This record is pending for the merged bytes. The earlier accepted or verified version is in `previous_record`.

```json
{
  "artifact": {
    "path": "scripts/ci/test-check-tla-invariants.sh",
    "id": "scripts-ci-test-check-tla-invariants-sh",
    "commit": "c9ca128214ce137708f852cdc252840e217df7a2",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "a17031a06f349c91dc877cf1acff173b84eddb2d3637b93360fc93a5194e5654",
    "sha256_at_registration": "af7fd80db3397c204e96da3ec706083e9c39e89430dc9970384af075c66025e6",
    "sha256_before_refresh": "af7fd80db3397c204e96da3ec706083e9c39e89430dc9970384af075c66025e6",
    "sha256_before_verification_refresh": "ac647001494500c87fdff20b5c1743508e643fab96cc41505b15762fbf7b8c00"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "c007214aad05afbcd100c0e37fd18b166463147b5fa2e1e797743ab814b15a49",
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "pending",
  "scope": "stack-merge-refresh-20261001-01",
  "previous_record": {
    "artifact": "scripts/ci/test-check-tla-invariants.sh",
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "sha256": "c11b46b862dc89d091e545c4b36133dec7d96c78be14fb9aa4ad3f863ec966ab",
    "commit": "c9ca128214ce137708f852cdc252840e217df7a2",
    "status": "discharged",
    "scope": "batch-e-acceptance-01"
  },
  "evidence": null,
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "previous_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "6d5ec6c42eac1250387178bf620942317162c0d7e1974660ff5a8b6b83bd4cfa"
  },
  "claim_digests_before_refresh": {
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal",
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-fork-choice-observation.md": "c007214aad05afbcd100c0e37fd18b166463147b5fa2e1e797743ab814b15a49",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "4bfd9be2599dc204d2910e77a6d0db64261cbd244e4a106dae630c0b9dd89630"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  },
  "refresh_reason": "The merge c9ca12821 of feature/casper-node-observation into formal/soak-casper-consensus combined the changes of both branches to this script. The accepted record applies to the earlier bytes only."
}
```
