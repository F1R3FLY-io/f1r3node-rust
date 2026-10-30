# Claims: deploy admission and expiry records

```yaml
claim_id: CLAIM-DEPLOY-ADMISSION-001
artifacts:
  - casper/src/rust/deploy_admission.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
  - casper/src/rust/casper.rs
  - block-storage/src/rust/deploy/key_value_deploy_storage.rs
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/710
```

Admission reserves capacity before persistence and bounds all fresh pending signatures against the measured local service budget.
Refused deploys produce no pool insertion or proposal signal.
Reservation rollback and deferred exclusion release each slot once.
Duplicate checks keep priority over full-queue errors.
Incompatible pool, rejected-buffer, DAG, or reservation changes during all three attempts produce a retryable refusal without saving the candidate.
Terminal progress alone does not restart an attempt. Its permanent exclusions can only reduce the fresh queue.
Only local blocks with remaining eligible demand contribute service samples.
New arrivals have a three-height horizon beyond the next block. Previously accepted future entries retain their queue positions.
Verified-record reuse preserves consistent metadata and the existing quarantine rules.

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
Terminal evaluation preserves the adopted floor, citation horizon, and existing terminal records.
Storage errors cannot authorize deletion without a lifecycle row.

The [specification](../specs/deploy-admission.md) defines the bounds and test scenarios.
The claims remain pending until review accepts the test evidence.
