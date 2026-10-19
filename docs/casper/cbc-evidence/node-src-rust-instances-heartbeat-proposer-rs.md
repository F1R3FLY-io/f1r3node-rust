# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

**Status:** pending.

This record identifies the heartbeat source at `da1c12ed1`. Construction and binding review do not establish discharge or maintainer acceptance.

The claim is `docs/claims/heartbeat-proposal-amplification-bound.md`. The review and test record is `docs/casper/validation/heartbeat-h1-verification-2026-10-19.md`.

The original leader-only evidence remains unchanged in the retained history file. The pre-implementation registration remains unchanged in the checkpoint Git object below.

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "id": "node-src-rust-instances-heartbeat-proposer-rs",
    "commit": "da1c12ed1",
    "working_tree": false,
    "sha256": "3c62915518a13bf20d8c79d0b1da106a3db5e26de6141412ef1f4fe287500576"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b"
  },
  "adapter": null,
  "status": "pending",
  "scope": "h1-bounded-nonempty-cooldown-exemption",
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
  "registration_history": {
    "git_object": "b87d1dfe29ee6d06d1894aec3db872c63ff6829f:docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-registration-20261018.md",
    "sha256": "4ec41bec62214de8dc500e91abd0fd0920f83e21fc4e2dbde15972058d5ae87a"
  },
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-20260821.md",
    "sha256": "e7ee126e11bf5a83a029bda23216afa7553887462cf1307430eac98d1b9eca7b",
    "commit": "3fd707102"
  },
  "previous_review_record": {
    "git_object": "33c0ac7c4af184573977b6b5184b81f4d5da4740:docs/casper/cbc-evidence/node-src-rust-instances-heartbeat-proposer-rs.md",
    "sha256": "8ab1634b099201488ef1e68b0691839ad1aee9450c8757f0a34a4436b1e23bdd",
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
