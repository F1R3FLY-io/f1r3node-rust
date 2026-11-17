# Claims: deploy admission and expiry records

```yaml
claim_id: CLAIM-DEPLOY-POOL-LIMITS-001
artifacts:
  - block-storage/src/rust/deploy/key_value_deploy_storage.rs
  - block-storage/src/rust/deploy/deploy_record_quarantine.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/casper_conf.rs
  - node/src/rust/web/shared_handlers.rs
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/710
```

The deploy store bounds its stored records by count and encoded bytes.
The check and the insert run under the store lock that every insert and deletion takes, so concurrent submissions cannot exceed a limit.
Only a deletion that removed the record, or a quarantine move, releases its charge.
The totals after a restart include every stored record, even above lowered limits, and no record is deleted to meet a limit.
A record larger than the byte limit and a full pool have distinct HTTP refusals.
Duplicate detection precedes both refusals.
Refused deploys produce no pool insertion, lifecycle row, or proposal signal.

```yaml
claim_id: CLAIM-DEPLOY-EXPIRY-001
artifacts:
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
  - block-storage/src/rust/dag/deploy_lifecycle_types.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/finality/deploy_lifecycle.rs
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/710
```

Admission opens the lifecycle row before the pool insert.
A deploy that no block carries settles `Expired` past its window end plus the citability horizon, whether or not the local validator creates blocks.
Admission never reopens a row behind a terminal record.
Rows persist across restart.

The independent API submission limit is disabled by default and requires an explicit positive value.
When enabled, HTTP and both gRPC services share this limit before decoding deploy bodies.
Excess submissions receive the existing admission-busy response without waiting for a permit.
Blocking submission work keeps its permit after client cancellation until completion or unwinding.
Queries and peer block processing do not take submission permits.

The [specification](../specs/deploy-admission.md) defines the bounds and test scenarios.
The claims remain pending until review accepts the test evidence.
