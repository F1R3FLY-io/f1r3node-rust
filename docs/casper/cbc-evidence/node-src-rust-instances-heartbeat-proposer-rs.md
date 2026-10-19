# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

**Status:** discharged for H1, with accepted limits.

This record identifies the heartbeat source at `da1c12ed1`. The maintainer accepted H1 at `0a4acca7a`, with the recorded assumptions and limits.

This discharge records maintainer review. It does not claim a mechanized proof.

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
  "status": "discharged",
  "scope": "h1-bounded-nonempty-cooldown-exemption",
  "evidence": {
    "kind": "implementation-review-pending-verification",
    "ref": "docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
    "sha256": "d63ee22fa382d7ddfa8f344cce7cc7f0dfad47d1972c4b766880468542e53b60"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "reviewed-not-mechanized-accepted-with-limits",
    "binding": "source-bound-checks-reviewed-and-accepted-with-limits"
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
    "git_object": "0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/cbc-evidence/node-src-rust-instances-heartbeat-proposer-rs.md",
    "sha256": "00da430134071695f259331fc341e28cb9324ad70a53a6e178c8dc90e72c93c2",
    "report": {
      "git_object": "0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
      "sha256": "37a9a5b80b2b5f2ceb5f003915acf438192cef771939bdffaba40f2efb21065e"
    }
  },
  "verification_sources": {
    "node/src/rust/instances/heartbeat_responsive_tests.rs": "034ce24f396ee3719013095e10970d3327cf4df5092daf3942dae1f284473ef3",
    "node/src/rust/instances/proposer_instance.rs": "c76ea6698d479db9617bba7cad5f7868a74f217d7582b28ae7ec8c6309aa9431",
    "casper/tests/add_block/h1_proposer_spec.rs": "b9f54549857fa29b0f008210c7e9b685a4304fd88a9eea1a79ec0b00d702b1c9"
  },
  "acceptance": {
    "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
    "claim_sha256": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b",
    "obligations": [
      "H1-1",
      "H1-2",
      "H1-3",
      "H1-4",
      "H1-5"
    ],
    "artifact_scope": "h1-bounded-nonempty-cooldown-exemption",
    "accepted_by": "jeffrey-l-turner",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-6265326408",
    "revision": "0a4acca7a179d69e766e65672480390c02b45fe5",
    "reviewed_at": "2026-10-19T10:40:35Z",
    "approval_type": "maintainer-review",
    "provenance": "shared-maintainer-handoff",
    "source_digests": {
      "node/src/rust/instances/heartbeat_proposer.rs": "3c62915518a13bf20d8c79d0b1da106a3db5e26de6141412ef1f4fe287500576"
    },
    "accepted_evidence": {
      "h1_report": {
        "git_object": "0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
        "sha256": "37a9a5b80b2b5f2ceb5f003915acf438192cef771939bdffaba40f2efb21065e"
      },
      "api_renewal": {
        "git_object": "0a4acca7a179d69e766e65672480390c02b45fe5:docs/casper/validation/api-display-projection-renewal-2026-10-19.md",
        "sha256": "acfd02d5a6b477d453767a9b6b7871eb3c163b660f41f4c89b8ea613a2d46701"
      }
    },
    "accepted_limits": [
      "The construction argument is reviewed, not mechanized.",
      "The bound assumes one live heartbeat task and sequential request awaits.",
      "Block hashes are immutable and snapshots are consistent.",
      "Publication effects precede successful callback completion.",
      "No bound applies across process restarts or globally across excluded proposal paths.",
      "Linux display admission and the Rocq recheck were not executed.",
      "The agentic verifier result is executable-check evidence only."
    ],
    "not_accepted": [
      "H1 performance qualification",
      "CLAIM-CASPER-NODE-OBSERVATION-005"
    ]
  },
  "waiver": null,
  "verified_at": null
}
```
