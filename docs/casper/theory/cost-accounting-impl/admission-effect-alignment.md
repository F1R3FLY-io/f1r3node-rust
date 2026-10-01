# Admission-record and runtime-effect alignment

This document specifies the boundary between terminal cost-accounting admission
records and the runtime-effect metadata used by multi-parent merging. It refines
the transaction and zero-effect rejection rules in
`cost-accounted-rho.tex` without changing the calculus, purse settlement, or
consensus voting rule. See DR-53 and CA-P-200.

## Terms

- A **status record** is a `ProcessedDeploy` carried in the block body so peers
  and clients can reproduce and observe a terminal deploy decision.
- An **effect-bearing user record** is a user deploy admitted to runtime
  execution. It owns one ordered merge-metadata entry and one execution-state
  witness position even if runtime execution returns a failure status.
- An **admission-rejected record** is a terminal status record produced before
  user runtime execution. It retains the signed envelope and authenticated
  pre-state decision but has no user event log, cost, state transition, or
  merge-metadata entry.
- A **system execution record** is a processed system deploy. Every processed
  system execution owns one merge-metadata position under the existing system
  execution contract.

## Normative projection

Let $`U`$ be the ordered user status records in a block, $`S`$ its ordered processed
system deploys, and $`M`$ the mergeable-channel evidence reconstructed by local
execution or replay. Define:

```math
E_U = [u \in U \mid u.\operatorname{admissionStatus} \ne \text{Rejected}].
```

The metadata cardinality and ordered split are:

```math
|M| = |E_U| + |S|,
```

```math
M_U = M[0..|E_U|], \qquad M_S = M[|E_U|..|M|].
```

User effect $`E_U[i]`$ aligns with $`M_U[i]`$. System execution $`S[j]`$ aligns
with $`M_S[j]`$ and has global execution index $`|E_U|+j`$. Inserting, removing,
or reordering admission-rejected status records cannot change either effect
projection. Reordering effect-bearing records changes their execution order and
therefore is not permitted by this rule.

`is_failed` is not the projection predicate. An ordinary failed deploy entered
the runtime and retains its metadata position; only
`is_admission_rejected()` identifies the pre-execution zero-effect case.

## End-to-end lifecycle

1. State-bound admission evaluates candidates against the authenticated block
   pre-state.
2. Admitted candidates execute and produce ordered state witnesses and
   mergeable-channel maps.
3. Underfunded candidates become terminal admission-rejected status records.
   They are appended to the block for consensus-visible lifecycle reporting but
   are not sent through the runtime.
4. System deploys execute after the admitted user sequence and append their
   metadata maps.
5. Replay reconstructs the admitted/rejected partition from the same pre-state,
   executes only admitted candidates, and reproduces the same $`M`$.
6. `BlockIndex` projects $`U`$ to $`E_U`$ before validating cardinality, checking
   state-witness adjacency, splitting metadata, and assigning execution indices.
7. A genuine missing or extra effect map still fails closed. A status-only
   admission record cannot make an otherwise valid parent unindexable.

## Failure repaired

The failing block contained one funding-admission rejection and one executed
`closeBlock`. Replay correctly produced one mergeable-channel map. `BlockIndex`
used the raw block-body cardinality and incorrectly required two maps. Every
validator then failed to index the same valid parent during successor proposal,
so heartbeats repeatedly returned a runtime error and later deploys remained
pending. API unavailability, timeouts, and memory pressure were downstream
effects of this proposal-liveness failure.

The defect did not alter majority voting or cause validators to compute
different state from the same executed payload. It violated the refinement from
the consensus-visible lifecycle representation to locally reconstructed runtime
effects. Because all validators eventually encountered the same malformed
cardinality assumption, the result was shard-wide loss of proposal progress.

## Implementation correspondence

| Specification element | Rust realization |
| --- | --- |
| $`U`$ | `BlockMessage.body.deploys` / `usr_processed_deploys` |
| Admission-rejected predicate | `ProcessedDeploy::is_admission_rejected` |
| $`E_U`$ | `block_index::effect_bearing_user_deploys` |
| $`S`$ | `BlockMessage.body.system_deploys` / `sys_processed_deploys` |
| $`M`$ | locally derived `NumberChannelsDiff` vector |
| Ordered split and index construction | `merging::block_index::new` |
| Replay-side projection | `RuntimeManager::verify_state_bound_admission_partition` |

The replay payload key already excludes admission-rejected records because they
did not execute. Aligning `BlockIndex` with the same projection restores one
semantic boundary across proposal, replay, cache identity, merge indexing, and
client-visible terminal status.

## Verification

`AdmissionEffectAlignment.tla` models three validators indexing the concrete
rejection-plus-`closeBlock` parent and then proposing successors. TLC exhausts
every validator interleaving and proves both universal proposal progress and
later deploy finalization. Apalache independently checks the safety invariants.
The unsafe configuration counts status records as effects and must produce the
validator-blocking counterexample.

`AdmissionEffectAlignment.v` proves without assumptions that admission
rejections contribute no effect slot at any sequence position, ordinary
execution failures retain a slot, permutation preserves required cardinality,
and a correctly sized metadata list splits exactly into user and system
segments. The concrete theorem proves that one funding rejection plus one
`closeBlock` requires one map while raw status counting requires two.

Rust examples cover the concrete regression, ordinary execution failure, and
effect-order preservation. A generated property varies admission and execution
failure classifications, user-record order, and system-deploy count. It requires
the production projection to equal the formal cardinality for every generated
case.

No Loom model is attached to this boundary because the projection is a pure,
immutable, single-call transformation with no synchronization or shared mutable
state. TLA+ explores the relevant distributed validator interleavings; adding a
Loom test here would model concurrency absent from the implementation.

## Rejected alternatives

- Adding an empty metadata map for an admission rejection would fabricate a
  runtime execution that never occurred and would contaminate execution-index
  identity.
- Dropping every `is_failed` record would also remove ordinary attempted
  executions and shift later user/system metadata positions.
- Suppressing the cardinality error would permit genuine replay-evidence loss.
- Removing the terminal rejection from the block would restore the indefinitely
  pending client lifecycle defect repaired by state-bound admission.
- Retrying proposals or extending timeouts would only repeat the deterministic
  indexing failure.

## Operational diagnosis

The primary signature is an error with
`mergeable_maps = effect-bearing user records + system executions` but
`raw block-body records + system executions` larger by the number of admission
rejections. Repeated heartbeat proposal failures, pending deploys, unavailable
APIs, and resource-guardian termination should be treated as consequences until
this cardinality is checked.

See also:

- [End-to-end authority settlement](end-to-end-authority-settlement.md)
- [Mergeable evidence authentication](mergeable-evidence-authentication.md)
- [Finalized-floor specification](../finalized-floor/finalized-floor-specification.md)
- [Finalized-floor verification](../finalized-floor/finalized-floor-verification.md)
