# Claim: issue #24 stage metrics observe and do not change consensus behavior

```yaml
claim_id: CLAIM-CASPER-STAGE-METRICS-001
artifacts:
  - casper/src/rust/engine/multi_parent_casper/finalization_runner.rs   # finalizer run, rerun, and timeout metrics
  - casper/src/rust/engine/multi_parent_casper/dispatch.rs              # API-triggered LFB computation metrics
  - casper/src/rust/blocks/proposer/block_creator.rs                    # admission deferral, empty-block skip, deploy age metrics
  - casper/src/rust/blocks/proposer/proposer.rs                         # tagged with this claim; no change planned
  - node/src/rust/instances/heartbeat_proposer.rs                       # heartbeat wake and proposal metrics
  - casper/src/rust/metrics_constants.rs                                # metric names
status: pending
adapter: agentic
mechanization: none
task: TASK-021-13
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # test_load deploys not finalized within 45 s
  - GitHub Actions run 37640235959                          # 24-hour soak: high and sustained phase failures
```

## Context

Soak 37640235959 failed `test_load` in two phases. In the high phase, deploys waited too long for inclusion. In the sustained phase, included deploys waited too long for finalization. The soak metrics cover block replay, repeat-deploy validation, and merge selection only, so they cannot name the stage that holds a deploy.

TASK-021-13 adds counters and histograms for both phases. They cover the deploy age at inclusion, the admission deferral, the empty-block skip, and the heartbeat. They also cover the finalizer runs and the LFB computation of the API. The soak harness reads counters and histograms only. This claim states that the new metrics observe the node and do not change it. The claim is pending, and no code change for these metrics starts before this file exists (EPIC-021 `cbc_policy`).

## Definitions

- **Metric call:** a `metrics::counter!` or `metrics::histogram!` call that records a value.
- **Observable result:** one of these values:
  - the deploys that a block includes, and the block content
  - a validation verdict
  - the last finalized block, the finalized block set, and a finalization effect
  - the response of an API call

## Claim statements

**S1. Non-interference.** Each observable result is the same with a metrics recorder installed and without one. No branch, loop bound, lock, or `await` point depends on a metric call or on a value that only a metric call reads.

**S2. No new shared state.** The metrics add no lock, no shared map, and no field to a consensus type. The deploy age uses the timestamp that the deploy already carries. The overlap count reads the existing `finalizer_task_in_progress` flag without a write.

**S3. Counting accuracy.** Each counter increments once for each event that its name states, and each histogram records one value for each measured interval:
- `block-creator.ordinary-deploys.deferred` adds the number of ordinary candidates that the selection leaves out.
- `finalizer.run.timeouts` adds one for each run that the 15 s backstop abandons.
- `finalizer.api-lfb.overlaps` adds one for each API computation that starts while a background run is in progress.

**S4. Preserved properties.** The metrics do not change the properties of these existing claims on the same files:
- CLAIM-CASPER-NODE-OBSERVATION-003 and -005 (`dispatch.rs`, `finalization_runner.rs`)
- the heartbeat proposal amplification bound (`heartbeat_proposer.rs`)
- the retry packaging record of `block_creator.rs`

## Seam premises (documented, not proven)

- **Recorder behavior.** The `metrics` facade with no recorder installed does nothing, and a recorder that fails does not panic the caller.
- **Clock.** The deploy age uses the wall clock of the node and the deploy timestamp from the client. The value is exact only when the client and the node share a clock, as in the subprocess shard of the soak.

## Discharge plan

1. Tag `finalization_runner.rs` and `proposer.rs` `cbc=mandatory cbc-weight=high` (decision of the maintainer on 2026-10-07). **Done 2026-10-07.**
2. S3: each metric has a test with a `DebuggingRecorder`. The test asserts the counter value or the histogram sample for a known event. **Open.**
3. S1 and S2: the review of the diff shows that each change adds metric calls only. The existing tests of each artifact pass without change. **Open.**
4. S4: the existing tests of the named claims pass without change. **Open.**
5. Record the evidence in `docs/casper/cbc-evidence/` for each changed `cbc=mandatory` artifact and cite this claim id. **Open.**
