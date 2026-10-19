# CbC Evidence: casper/src/rust/blocks/proposer/proposer.rs

**Status:** pending.

This record identifies H1's request enforcement at `ab84edf83`. It does not claim discharge or maintainer acceptance.

The claim is `docs/claims/heartbeat-proposal-amplification-bound.md`. The review record is `docs/casper/validation/heartbeat-h1-verification-2026-10-19.md`.

```json
{
  "artifact": {
    "path": "casper/src/rust/blocks/proposer/proposer.rs",
    "id": "casper-src-rust-blocks-proposer-proposer-rs",
    "commit": "ab84edf83",
    "working_tree": false,
    "sha256": "ccb01fdcf2027f7698066ed5ad166e260fab043846088fc9939c1a3f640c18ac"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b"
  },
  "adapter": null,
  "status": "pending",
  "scope": "h1-request-carrier-predecessor-revalidation-and-nonempty-publication",
  "evidence": {
    "kind": "implementation-review-pending-verification",
    "ref": "docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
    "sha256": "37a9a5b80b2b5f2ceb5f003915acf438192cef771939bdffaba40f2efb21065e"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "argument-recorded-not-mechanized",
    "binding": "review-recorded-not-accepted"
  },
  "previous_review_record": {
    "git_object": "33c0ac7c4af184573977b6b5184b81f4d5da4740:docs/casper/cbc-evidence/casper-src-rust-blocks-proposer-proposer-rs.md",
    "sha256": "18e03a9a409e1f5d1a6d62a53eb847f63bfed6412a9395efd1ad9a4aaa83f578",
    "report": {
      "git_object": "33c0ac7c4af184573977b6b5184b81f4d5da4740:docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
      "sha256": "3d8a73b53e8873d150e90856c20fa4b3fff7d6f2e953c5cd00e79507ce4f24e5"
    }
  },
  "verification_sources": {
    "node/src/rust/instances/heartbeat_responsive_tests.rs": "034ce24f396ee3719013095e10970d3327cf4df5092daf3942dae1f284473ef3",
    "node/src/rust/instances/proposer_instance.rs": "c76ea6698d479db9617bba7cad5f7868a74f217d7582b28ae7ec8c6309aa9431",
    "casper/tests/add_block/h1_proposer_spec.rs": "b9f54549857fa29b0f008210c7e9b685a4304fd88a9eea1a79ec0b00d702b1c9"
  },
  "waiver": null,
  "verified_at": null
}
```
