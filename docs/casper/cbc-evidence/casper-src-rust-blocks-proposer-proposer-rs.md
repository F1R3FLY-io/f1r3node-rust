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
    "sha256": "3d8a73b53e8873d150e90856c20fa4b3fff7d6f2e953c5cd00e79507ce4f24e5"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "argument-recorded-not-mechanized",
    "binding": "review-recorded-not-accepted"
  },
  "previous_review_record": {
    "git_object": "b87d1dfe29ee6d06d1894aec3db872c63ff6829f:docs/casper/cbc-evidence/history/casper-src-rust-blocks-proposer-proposer-rs-renewal-base-f41f2a8ab-20261019.md",
    "sha256": "7e2f98c2619447cc8a5f973bf1aa5f287efea9675e8c868c6a8e873549dab951",
    "report": {
      "git_object": "b87d1dfe29ee6d06d1894aec3db872c63ff6829f:docs/casper/cbc-evidence/history/heartbeat-h1-verification-renewal-base-f41f2a8ab-20261019.md",
      "sha256": "97c86f2906fb9588e4ca6afea70a34f92a02ab3eb4925c10925fa2c3b3908d43"
    }
  },
  "verification_sources": {
    "node/src/rust/instances/heartbeat_responsive_tests.rs": "edcc7468032f7edb6f605f2b6028c673f052d033224e0f61b7e2a6d24066835c",
    "node/src/rust/instances/heartbeat_additional_tests.rs": "fd73cd87c979683de84c3fdf57921e3a16ed6810d26688cc690eb6a43cf2c1e3",
    "node/src/rust/instances/proposer_instance.rs": "c76ea6698d479db9617bba7cad5f7868a74f217d7582b28ae7ec8c6309aa9431",
    "casper/tests/add_block/h1_proposer_spec.rs": "b9f54549857fa29b0f008210c7e9b685a4304fd88a9eea1a79ec0b00d702b1c9"
  },
  "waiver": null,
  "verified_at": null
}
```
