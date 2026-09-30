# CbC Evidence: casper/tests/soak_observer.rs

This record registers the pending Batch D claim before implementation. It replaces the accepted record of the earlier claim, which previous_record names. It contains no acceptance evidence.

```json
{
  "artifact": {
    "path": "casper/tests/soak_observer.rs",
    "id": "casper-tests-soak-observer-rs",
    "commit": "e90e4cffae56fce5ab39eea5878c7772ffa5e880",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": null,
    "sha256_at_registration": "5eb62fddb3517a435cb2597595343bf2d9ed32acf8308911543464387bd556a6"
  },
  "claim": "docs/claims/casper-node-fork-choice-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/casper-tests-soak-observer-rs.md",
    "sha256": "be3c89f7158e85e0625ca45982f9f419ae760e2f734e4a9d82e2414039aa71e5",
    "commit": "e90e4cffae56fce5ab39eea5878c7772ffa5e880"
  },
  "status": "pending",
  "scope": "batch-d-registration",
  "evidence": null,
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null
}
```
