# Claims: deploy admission and expiry records

```yaml
claim_id: CLAIM-DEPLOY-RETAINED-RESOURCES-001
artifacts:
  - casper/src/rust/deploy_admission.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/casper_conf.rs
  - node/src/rust/web/shared_handlers.rs
status: pending
adapter: agentic
mechanization: none
```

Explicit limits cover retained pool records and unwritten reservations once per signature.
Retained accounting is enabled by default with provisional limits of 512 records and 16 MiB.
Only an explicit null configuration selects the previous comparison policy.
These defaults are resource-budget candidates, not measured service or memory guarantees.
Only confirmed storage removal or rollback of an unwritten reservation releases their resource charge.
Inclusion, expiry, and terminal verdicts do not independently authorize release.
Reconciliation preserves a signature persisted by another handle when an overlapping reservation rolls back.
Restored occupancy can exceed a configured limit without deleting accepted records.
Single-record overflow and temporary aggregate exhaustion have distinct HTTP refusals.
Focused checks passed: 329 distinct configuration, ingress, admission, lifecycle, HTTP, and gRPC tests, with two ignored integration cases.
After removing the numeric concurrency default, 164 configuration, ingress, and diagnostics tests passed again.
Node build, CLI help, formatting, Clippy with warnings denied, and source hash checks also passed.
Workload calibration and validation of the combined external queue remain separate work.
The claim remains pending review and validation on the transferred revision.

The following claims describe the existing comparison policy and expiry bookkeeping.

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
Samples use selection-time candidates and actual inclusions. Later arrivals do not change the sample.
Drained local blocks can raise a separate recovery budget by at most one estimated block per observation.
An increased budget cannot exceed three times the actual inclusion count or three configured block limits.
A new sample with remaining eligible demand resets recovery. Idle blocks and small batches cannot reduce the budget.
New arrivals have a three-height horizon beyond the next block. Previously accepted future entries retain their queue positions.
Verified-record reuse preserves consistent metadata and the existing quarantine rules.

The comparison policy imposes no byte or declared-phlo budget.
Its sample-based regulation applies only to fresh signature count.

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

The independent API submission limit is disabled by default and requires an explicit positive value.
When enabled, HTTP and both gRPC services share this limit before decoding deploy bodies.
Excess submissions receive the existing admission-busy response without waiting for a permit.
The gRPC response retains the existing service error envelope.
Blocking submission work retains its permit after client cancellation until completion or unwinding.
Queries and peer block processing do not acquire submission permits.
