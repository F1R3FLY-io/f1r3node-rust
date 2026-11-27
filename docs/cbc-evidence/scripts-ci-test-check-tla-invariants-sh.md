# CbC evidence for scripts/ci/test-check-tla-invariants.sh

The node-observation records retain their pending status for the merged bytes on dev. The earlier Batch E acceptance in PR #447 remains in Git at `e38e7433e`.

## Claim CLAIM-CASPER-NODE-OBSERVATION-004

This record retains the shared 004 and 005 pending record from dev at `3c6a46d2`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-004",
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

## Claim CLAIM-CASPER-NODE-OBSERVATION-005

This record retains the shared 004 and 005 pending record from dev at `3c6a46d2`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-005",
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


## Claim CLAIM-HEARTBEAT-WAKE-CI-001

```json
{
  "artifact": {
    "path": "scripts/ci/test-check-tla-invariants.sh",
    "id": "scripts-ci-test-check-tla-invariants-sh",
    "commit": "60853a71900c38021f0315a9f14fae71d6e1553b"
  },
  "claim": "docs/claims/heartbeat-wake-ci.md",
  "adapter": "manual",
  "status": "discharged",
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "60853a71900c38021f0315a9f14fae71d6e1553b",
    "sha256": "649b0b4a7497f8114b66f79b5fa797cf999c0a8029219d1b17eaefc7a863d9d7"
  },
  "evidence": {
    "kind": "formal-gate-registration-check",
    "ref": "formal/tlaplus/heartbeat_wake/README.md",
    "detail": "The 2026-11-27 review accepted the heartbeat registration checks and the updated negative-control fixture. The fixture classified all 101 negative controls and verified routing to both tiers. The local pull request tier passed 19 positive configurations and 101 expected negative controls."
  },
  "waiver": null,
  "verified_at": "2026-11-26T23:43:46Z",
  "verification_id": "8aec0731-f864-40fd-ae95-f18eb605f144",
  "inputs": {
    "artifact": "scripts/ci/test-check-tla-invariants.sh",
    "source_sha256": "98830ea9f2b08b237b718d39a438db3fd182646dacf0371609c726a7ae926f26",
    "claim": {
      "path": "docs/claims/heartbeat-wake-ci.md",
      "sha256": "f2243fdad69cce95f8e7dc9958bb20364ba8fa1eb693deed119c0e8a5d44b63f"
    }
  },
  "evidence_files": {
    "formal/tlaplus/heartbeat_wake/README.md": "925efffc5914b988249cfeee7de9b41ca3c05a0d61496a37f0a9ca88816a570a"
  },
  "scope": "heartbeat-tla-registration-and-control-fixture",
  "manual_review": {
    "reviewed_on": "2026-11-27",
    "revision": "60853a71900c38021f0315a9f14fae71d6e1553b",
    "technical_review": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-7219211333",
    "approval_ref": "Maintainer approval to proceed with step 7 on 2026-11-27"
  },
  "claim_id": "CLAIM-HEARTBEAT-WAKE-CI-001",
  "schema": "cbc-claim/v1"
}
```
