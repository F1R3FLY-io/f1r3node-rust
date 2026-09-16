# Casper Recovery and Custody Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-004
status: pending
adapter: embedded
decisions: [D-06, D-07]
pre_merge_tasks: [TASK-017-7]
post_merge_tasks: [TASK-018-3, TASK-018-5]
artifacts:
  - node/src/rust/instances/heartbeat_proposer.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
  - casper/src/rust/validate.rs
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are typed proposal intents, permit versions, validator eligibility, frontier observations, exact deploy occurrences, tombstones, carrier senders, and objective heights.

Outputs are proposal decisions, custody sets, retry packaging, expiry decisions, and retained terminal reasons.

The baseline preserves all-eligible interval-paced stale recovery, leader-only convergence, frontier follow, readiness lanes, and the upstream recovery model.

Single-flight coalescing retains one latched wakeup. Permit revalidation must reject stale work before proposal.

Deploy recovery preserves carrier-sender custody, exact occurrence identity, causal closure, one-parent coverage, authenticated index fallback, and current retry leadership.

Reason joining must be commutative, associative, and idempotent. Missing required bodies fail closed.

Lease expiry cannot bypass floor, custody, lifespan, replay, or validation gates. Recovery authority and retry custody remain separate.

## Model and oracle

Audit `formal/tlaplus/deploy_recovery/`, `formal/tlaplus/deploy_occurrence/`, and heartbeat models under `formal/tlaplus/finalized_floor/` before reuse.

The proposed finite instance uses three validators, two deploy identities, two frontiers, two permit versions, and four objective heights.

The oracle records exact occurrences and computes eligibility by lane. It must not impose leader-only policy on stale recovery.

Production bindings cover heartbeat decisions, retry packaging, and validation. TASK-017-7 must identify the functions for each lane and custody transition.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean retries and reason joins | Preserve custody and identical canonical reasons under arrival permutations. |
| Use a stale permit | Violate permit revalidation. |
| Collapse identity to signature only | Lose a distinct occurrence and fail the oracle comparison. |
| Bypass custody after lease expiry | Violate retry authorization. |
| Ignore a missing body | Violate fail-closed recovery. |
| Apply convergence leadership to stale recovery | Violate the baseline lane truth table. |

## Scheduling and experiments

Safety claims must hold during pauses and delayed delivery. Eventual retry requires eventual message delivery, an eligible active proposer, and fair enabled-action scheduling.

Do not claim bounded recovery when a required validator remains paused indefinitely. Record pause duration, delivery delay, and the assumed scheduling bound.

Separate experiments compare rotating leadership, frontier removal, clock variants, collective coverage, and leader-free custody. No passing experiment authorizes activation.

Measure duplicate proposals, custody disagreement, retry completion, expiry, recovery duration, blocks per height, finalization, and resources.

## Tiers and phases

TLC checks the finite scheduling instance. Unbounded custody and reason-algebra claims require Rocq promotion with named theorems and closed assumptions.

Pre-merge work supplies baseline bindings and isolated experiment definitions. Post-merge work repeats the bindings and soak comparisons with new run identities.

Legacy heartbeat evidence contains leader-only stale-recovery statements. That evidence cannot discharge this claim without a property and source audit.

The [harness contract](./casper-soak-harness.md) defines evidence and closure. Refutation, construction, binding, and soak results remain pending.
