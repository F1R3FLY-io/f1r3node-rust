# D-06 Heartbeat Intents and Recovery Leadership

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Node-local liveness policy. It changes no block validity rule.

**Sources.**

- dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 8, [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) ground truth 4, [heartbeat amplification claim](../../../claims/heartbeat-proposal-amplification-bound.md), [`formal/tlaplus/recovery_leader/`](../../../../formal/tlaplus/recovery_leader).
- PR #216 `finalized-floor-specification.md` section 2.1.1 rules R-HEARTBEAT-SEPARATION to R-HEARTBEAT-ASYNC, R-PROPOSAL-INTENT, R-PROPOSER-COALESCING, invariants S34 and S39, liveness L13 and L15, models `HeartbeatRecoveryCadence.tla`, `RecoveryCommitteeTransition.tla`, `ProposerAdmissionCoalescing.tla`, `PendingDeployHeartbeatComposition.tla`.

## 1. Question

Who proposes when the LFB stalls, what authorizes an empty block, and how do proposal requests coalesce?

## 2. Position on dev

The heartbeat decision tree has four lanes.

1. Pending deploys propose under a lag throttle and an interval backstop.
2. Frontier follow proposes when new parents are observed.
3. Stale-LFB recovery: when the LFB is older than `max_lfb_age`, **every bonded validator proposes**, at most once per `stale-recovery-min-interval`. The text says recovery is never leader-gated because certification needs mutual witnessing and one proposer cannot rebuild it alone. A deterministic leader survives only for the one-shot multi-parent convergence proposal.
4. The self-propose cooldown gates lanes 1 and 2, never lane 3.

Ground truth 4 says per-scope inclusion leadership exists on the proposer side with a lease-based escape. It also says the recovery path dropped leader election in favor of owner-scoped buffers plus the floor-paced retry gate. `RecoveryLeader.tla` models cross-view leader agreement for one bonded validator set and is in the CI gate.

## 3. Position on PR #216

- **R-PROPOSAL-INTENT.** Every request carries `Manual`, `PendingDeploy`, or `FinalityRecovery(permit)`. Only a valid recovery permit may authorize an empty block, and only when the heartbeat capability is enabled.
- **R-HEARTBEAT-PROGRESS.** Stagnation is measured from the monotonic duration the observed LFB hash stays unchanged. Round zero opens after `T0 = max(max_lfb_age, check_interval)`. Later rounds open every `check_interval`.
- **R-HEARTBEAT-COMMITTEE and R-HEARTBEAT-ROTATION.** The committee derives from the captured LFB post-state by `floor_committee`. Exactly one validator is authorized per `(LFB, round)`: `leader(C, h, r) = C[(h + r) mod |C|]`. Advancing the round rotates past an offline leader.
- **R-RECOVERY-PERMIT.** The proposer revalidates the permit against a fresh snapshot immediately before execution. A stale or nonleader permit is deferred.
- **R-HEARTBEAT-WORK.** Peer block arrival and frontier movement never authorize a proposal. The frontier-follow lane does not exist.
- **R-PENDING-RECOVERY-COMPOSITION.** A recovery round carries admissible deploys when they exist. Empty-block authority matters only when the admissible selection is empty.
- **R-PROPOSER-COALESCING.** Proposal execution is single-flight with one latched pending wakeup. Colliding manual and recovery requests return busy without changing the active intent.
- **R-CARRIER-RETRY-CUSTODY.** Recovery leadership never authorizes a rejected-deploy retry. Entry D-07 covers custody.
- **R-HEARTBEAT-ASYNC.** Honest nodes may occupy different local rounds. Safety needs no delivery bound. Liveness L13 assumes eventual delivery within a round and bounded relative scheduling.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Stale-LFB proposer | Every bonded validator | One leader per `(LFB, round)` |
| Rationale | Mutual witnessing needs many proposers | Rotation supplies witnesses across rounds without amplification |
| Frontier follow | A proposal lane | Removed. Peer blocks are evidence only. |
| Empty-block authority | Heartbeat lane | A revalidated recovery permit |
| Request coalescing | Not specified | Single-flight with one wakeup bit |
| Progress measure | LFB age | Unchanged LFB hash over monotonic time |
| Recovery leader model | `RecoveryLeader.tla`, gated | `RecoveryCommitteeTransition`, gated. `RecoveryLeader` removed from the gate. |

## 5. Options

- **A. Adopt rotation as written.** One leader per round. Recovery of a stalled LFB takes up to `|C|` rounds before every validator has proposed once.
- **B. Adopt rotation for the first round and all-propose after a bounded number of rounds.** Rotation bounds amplification in the common case. All-propose restores dev's mutual-witnessing argument when rotation stalls.
- **C. Keep all-propose.** Adopt only the intent taxonomy, the coalescing rule, and the permit revalidation.

## 6. Unification proposal

Adopt option B as the position to test, and defer the choice between A and B to soak evidence.

The heartbeat is proposer-side policy. Principle P3 places it in the safe extension surface, and principle P5 requires evidence before a liveness change. The dev text and the PR #216 text make opposite liveness claims. Dev says one proposer cannot rebuild certification. PR #216 says rotation supplies witnesses across rounds and states the assumptions as L13. Neither branch cites a soak comparison.

Four items can ratify now, without the soak. They are the intent taxonomy, the rule that peer block arrival is evidence and not proposal authority, permit revalidation at execution, and single-flight coalescing. These follow principle P2. They do not decide the leader question.

## 7. Ratification checklist

- A soak run on a shard with a paused validator, comparing stalled-LFB recovery time and block count under all-propose and under rotation.
- The heartbeat amplification claim on dev updated to state its bound under each policy.
- Decide whether `RecoveryLeader.tla` stays in the gate or is superseded by `RecoveryCommitteeTransition.tla`.
- After ratification, replace the protocol section 8 decision tree and amend ground truth 4.

## 8. Open questions

1. Under rotation, how many rounds does a stalled shard need before a clique forms when the offline validator is the first leader? L13 gives the assumptions but not the bound.
2. Does removing the frontier-follow lane change block cadence on a shard with no deploys and no stall?
