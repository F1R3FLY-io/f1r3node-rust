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
    "sha256": "97c86f2906fb9588e4ca6afea70a34f92a02ab3eb4925c10925fa2c3b3908d43"
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
    "path": "docs/casper/cbc-evidence/history/node-src-rust-instances-heartbeat-proposer-rs-review-3ac876857-20261019.md",
    "sha256": "437f507f22ebd7058faa392a5bab3a7956b52f10d02723bca5c2d1d37eb2563d",
    "report": {
      "path": "docs/casper/cbc-evidence/history/heartbeat-h1-verification-review-3ac876857-20261019.md",
      "sha256": "4466cc80f191cad966914af3d94675658e6f3dc63db84658fc82ef7a4b587d4a"
    }
  },
  "waiver": null,
  "verified_at": null
}
```
