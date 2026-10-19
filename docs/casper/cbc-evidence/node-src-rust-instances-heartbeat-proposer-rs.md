# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

**Status:** pending.

This record identifies the heartbeat source at `da1c12ed1`. Construction and binding review do not establish discharge or maintainer acceptance.

The claim is `docs/claims/heartbeat-proposal-amplification-bound.md`. The review and test record is `docs/casper/validation/heartbeat-h1-verification-2026-10-19.md`.

Both the original leader-only evidence and the pre-implementation pending registration remain unchanged in the history paths below.

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
    "sha256": "1dddefc95b540d4a5873d212d891e385f14137c17e2e2eba229d68427b423304"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "argument-recorded-not-mechanized",
    "binding": "review-recorded-not-accepted"
  },
  "registration_history": {
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-registration-20261018.md",
    "sha256": "4ec41bec62214de8dc500e91abd0fd0920f83e21fc4e2dbde15972058d5ae87a"
  },
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-20260821.md",
    "sha256": "e7ee126e11bf5a83a029bda23216afa7553887462cf1307430eac98d1b9eca7b",
    "commit": "3fd707102"
  },
  "previous_review_record": {
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-renewal-base-f41f2a8ab-20261019.md",
    "sha256": "c846487b25f6596cb1d2dffea3e1574ff5cf3220d0855cadab3a38cac4e84ca9",
    "report": {
      "path": "docs/casper/cbc-evidence/history/heartbeat-h1-verification-renewal-base-f41f2a8ab-20261019.md",
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
