# Heartbeat wake CI claim

```yaml
claim_id: CLAIM-HEARTBEAT-WAKE-CI-001
status: pending
artifacts:
  - scripts/ci/check-formal-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
```

The [Rocq runner](../../scripts/ci/check-formal-invariants.sh) must build and check the HeartbeatWake project. All seven listed HeartbeatWake theorems must report closed assumptions. Each of the eight source scans must fail on a prohibited declaration or a scan error.

The [TLA+ fixture](../../scripts/ci/test-check-tla-invariants.sh) must check that the heartbeat baseline and its three negative controls run in both tiers. It must check each expected verdict and reject missing or incorrect results.

The [heartbeat evidence report](../../formal/tlaplus/heartbeat_wake/README.md) records the declaration controls, the clean Rocq run, and the TLA+ fixture results.
