# Casper Authority Live Executor Claim

```yaml
claim_id: CLAIM-CASPER-AUTHORITY-LIVE-001
status: pending
scope: harness-live-authority-executor
pre_merge_tasks: [TASK-017-12]
artifacts:
  - scripts/casper-soak/src/authority_live.rs
  - scripts/casper-soak/src/bin/casper-authority-live.rs
  - scripts/casper-soak/src/authority_p2p.rs
  - scripts/casper-soak/src/authority_process.rs
  - scripts/casper-soak/src/authority_incarnation.rs
  - scripts/casper-soak/src/bin/casper-authority-p2p.rs
  - scripts/casper-soak/src/bin/casper-authority-process.rs
  - scripts/casper-soak/tests/authority_live.rs
  - scripts/casper-soak/tests/authority_p2p.rs
  - scripts/casper-soak/tests/authority_process.rs
  - scripts/casper-soak/check-authority-live.sh
  - .github/workflows/casper-authority-adapter.yml
  - docs/casper/design/authority-live-executor.md
  - docs/casper/design/authority-provider-adaptation.md
refutation: pending
construction: pending
binding: pending
soak: pending
```

The executor calls a pinned workload driver and captures authority observations through the existing Linux observer client.

The driver receives an exact step request. Its response must bind the request digest, operation, applied input exports, and captured snapshot digest.

The executor checks node process identity through the observer transport. It retains raw captures and constructs observations from mapped node values.

Driver output cannot supply finality decisions, fork-choice heads, or traversal measurements. Missing node observations remain missing.

Each receipt binds the execution request, preceding receipt, step, and retained input exports. The executor publishes partial inventories after completed steps.

The qualification command does not enable campaign admission. The authority profile retains its live qualification barrier.

The block driver submits pinned protobuf bytes through the production peer transport. Transport delivery does not establish block validation or input export identity.

The process owner controls only its native child. The executor connects pause and restart receipts to captures from that child.

Restart readiness requires a verified successor capture. Requests can pin the successor or explicitly enroll its observed incarnation through the bound restart receipt.

Controlled Linux tests verify executable invocation, transport capture, receipt binding, unavailable capabilities, and rejection of changed identities.

Passing these tests does not qualify a blockchain node. Source-bound acceptance remains pending.
