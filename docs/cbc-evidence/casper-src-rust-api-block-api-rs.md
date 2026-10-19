# CbC Evidence: casper/src/rust/api/block_api.rs

**Status:** pending.

H1 converts the two legacy callback booleans into equivalent `ProposeRequest` values. No API response or display-projection rule changes in that conversion.

The historical Batch E record remains byte-identical at `docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md`.
Its acceptance applies only to its recorded source and claim digests. Those digests differ from the current files, so acceptance is not transferred here.

The maintainer accepted the H1 request-conversion scope at `0a4acca7a`. Overall status remains pending because CLAIM-CASPER-NODE-OBSERVATION-005 is separate and unaccepted.

The H1 acceptance names the API renewal as evidence. It does not grant acceptance to the display-projection claim.

```json
{
  "artifact": {
    "path": "casper/src/rust/api/block_api.rs",
    "id": "casper-src-rust-api-block-api-rs",
    "commit": "f41f2a8ab",
    "working_tree": false,
    "sha256": "9bf8f175f132f89b373a10af6eba70a9325a6ec9c4fc15cb8ab72a0a71d080f2"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b",
    "docs/claims/casper-node-display-projection.md": "1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591"
  },
  "status": "pending",
  "adapter": null,
  "scope": "h1-legacy-request-bridge-and-api-display-projection-renewal",
  "scope_discharges": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": {
      "status": "discharged",
      "scope": "h1-request-conversion-only",
      "acceptance_record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-6265326408"
    }
  },
  "evidence": {
    "kind": "source-review-pending-acceptance",
    "ref": "docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
    "sha256": "d63ee22fa382d7ddfa8f344cce7cc7f0dfad47d1972c4b766880468542e53b60"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "legacy-boolean-conversion-review",
    "binding": "pending"
  },
  "related_claims": {
    "docs/claims/casper-node-display-projection.md": {
      "status": "api-scope-renewal-recorded-acceptance-pending",
      "sha256": "1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591",
      "evidence": {
        "ref": "docs/casper/validation/api-display-projection-renewal-2026-10-19.md",
        "sha256": "acfd02d5a6b477d453767a9b6b7871eb3c163b660f41f4c89b8ea613a2d46701"
      },
      "limits": [
        "linux-admission-not-run",
        "rocq-unavailable",
        "no-rust-refinement-proof",
        "historical-acceptance-not-transferred"
      ]
    }
  },
  "previous_ledger": {
    "path": "docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md",
    "sha256": "8d550febbc1e4ad062be64c97af94631c74e281a6758a1dd3fa824c591d606be",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f"
  },
  "previous_review_record": {
    "git_object": "0a4acca7a179d69e766e65672480390c02b45fe5:docs/cbc-evidence/casper-src-rust-api-block-api-rs.md",
    "sha256": "f41053d8b75e133444055a12328ac361a6cd014eefc987c7d2d1a28608cc4fb3",
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
    "artifact_scope": "h1-request-conversion-only",
    "accepted_by": "jeffrey-l-turner",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-6265326408",
    "revision": "0a4acca7a179d69e766e65672480390c02b45fe5",
    "reviewed_at": "2026-10-19T10:40:35Z",
    "approval_type": "maintainer-review",
    "provenance": "shared-maintainer-handoff",
    "source_digests": {
      "casper/src/rust/api/block_api.rs": "9bf8f175f132f89b373a10af6eba70a9325a6ec9c4fc15cb8ab72a0a71d080f2"
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
