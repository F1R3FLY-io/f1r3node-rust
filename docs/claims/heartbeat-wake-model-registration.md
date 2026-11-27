# Heartbeat wake model registration claim

```yaml
claim_id: CLAIM-HEARTBEAT-WAKE-REGISTRATION-001
status: pending
artifacts:
  - scripts/ci/check-tla-invariants.sh
```

The [TLA+ gate](../../scripts/ci/check-tla-invariants.sh) must evaluate all four heartbeat wake configurations in both the full and pull request tiers. The positive configuration must complete with every invariant satisfied. Each negative control must report the invariant in the table.

| Configuration | Expected result |
| --- | --- |
| `HeartbeatWake.cfg` | All invariants hold |
| `WakeOnRejected.cfg` | `WakeOnlyOnAdmission` fails |
| `BypassCooldown.cfg` | `CooldownProtectsPending` fails |
| `RecheckDespiteLag.cfg` | `RecheckRequiresPendingCooldown` fails |

The gate must fail if a configuration is missing, exploration is incomplete, or a result differs from the expected result.

The [gate fixture](../../scripts/ci/test-check-tla-invariants.sh) checks registration in both tiers. The [heartbeat evidence report](../../formal/tlaplus/heartbeat_wake/README.md) records the model results.
