# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

**Status:** pending.

This record identifies H1 at `ab84edf83`. Construction and binding review do not establish discharge or maintainer acceptance.

The claim is `docs/claims/heartbeat-proposal-amplification-bound.md`. The review and test record is `docs/casper/validation/heartbeat-h1-verification-2026-10-19.md`.

Both the original leader-only evidence and the pre-implementation pending registration remain unchanged in the history paths below.

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "id": "node-src-rust-instances-heartbeat-proposer-rs",
    "commit": "ab84edf83",
    "working_tree": false,
    "sha256": "77328af2cc336e93299f0dbf25100309359b49825e98f3e4dfdfea3bf8f18c5e"
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
    "sha256": "4466cc80f191cad966914af3d94675658e6f3dc63db84658fc82ef7a4b587d4a"
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
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-ab84edf83-20261019.md",
    "sha256": "4efaac8a57aa9ce5cf616e4928b9bdad7f724e7041251262a09dca7f4cc80816",
    "report": {
      "path": "docs/casper/cbc-evidence/history/heartbeat-h1-verification-ab84edf83-20261019.md",
      "sha256": "2ec8a9362307056af54418fccc09cd0f198362e61fa51912c0790209a7652848"
    }
  },
  "waiver": null,
  "verified_at": null
}
```
