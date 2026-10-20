# Claims: deploy admission and expiry records

```yaml
claim_id: CLAIM-DEPLOY-ADMISSION-001
artifacts:
  - casper/src/rust/deploy_admission.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
  - casper/src/rust/casper.rs
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/710
```

Admission checks fresh predecessor count against the specified local service budget under the insertion lock.
Refused deploys produce no pool insertion or proposal signal.
The shared comparator preserves the block selection order.
Demand samples use accepted local blocks and actual first inclusions.

```yaml
claim_id: CLAIM-DEPLOY-EXPIRY-001
artifacts:
  - block-storage/src/rust/dag/deploy_lifecycle_types.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
  - casper/src/rust/finality/deploy_lifecycle.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/710
```

Expiry removal saves the lifecycle row before pool deletion.
Restart recovery and active scheduling retain the row for evaluation.
Terminal evaluation preserves the consensus bound and checks floor membership.
Storage errors cannot authorize deletion without a lifecycle row.

The [specification](../specs/deploy-admission.md) defines the bounds and test scenarios.
The claims remain pending until review accepts the test evidence.
