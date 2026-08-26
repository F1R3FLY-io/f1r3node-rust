# Heartbeat stale-LFB recovery

## Problem

The **last finalized block (LFB)** is the block whose replay state is the
currently committed consensus floor. A shard needs bounded proposal activity to
carry new justification evidence when that LFB stops advancing. Two earlier
recovery policies failed in opposite directions:

- throttling solely by LFB lag left a low-lag window in which no validator
  proposed; and
- letting every validator chase peer/frontier activity turned received blocks
  into proposal authority, creating redundant support siblings and a replay and
  validation backlog.

## Correct behavior

Recovery is proposal scheduling, not finalization. It must produce enough
ordinary blocks for eventual progress without manufacturing votes, weakening
the clique threshold, or allowing peer arrival to trigger a feedback loop.

The scheduler therefore measures task-local monotonic time since the exact LFB
hash last changed. It rotates one deterministic leader per validator-local
recovery round over the canonical committee derived from that LFB's post-state.
Different
validators may observe the stall and rounds at different times; finality safety
does not require synchronized heartbeat clocks.

## Fix

Each selected request carries `FinalityRecovery(FinalityRecoveryPermit)`. The
permit binds the observed LFB hash, height, and round. The serialized proposer
takes a fresh snapshot immediately before execution. It verifies the captured
hash and LFB metadata height, then uses the captured round only to recompute the
leader over the fresh LFB-derived committee. It does not consult an independently current
round, and a change to only the unfinalized head height cannot stale the permit.
The owning heartbeat task awaits the result before completing or retrying its
local round.

The proposer then derives the structural floor of the selected parents and
frozen justifications before replay. Its positive active bond map must equal the
captured LFB committee, its justification validators must be that exact set,
and the sender and synchrony weights must come from the same floor authority.
The block's serialized bonds are not used for these checks: they are an
independently replay-validated cache of the block's own post-state. Consequently
a bond created by the recovery block cannot authorize that block, an invalid
cache cannot register a validator, and an accepted transition becomes
authoritative only after a later floor promotion.

Pending user work composes with recovery. The ordinary block creator first
selects deploys admissible in the fresh snapshot. If any exist, the recovery
block carries them; only a valid recovery permit may authorize an empty block
when the selection is empty. A deploy that merely remains stored but is future,
expired, terminal, already in scope, or otherwise inadmissible cannot mask
empty recovery.

Proposal admission is single-flight. Pending-deploy collisions set one coalesced
wakeup, which becomes exactly one fresh-snapshot follow-up when active work
finishes. Manual and recovery collisions do not change the active request's
intent. A busy, deferred, empty, or failed selected recovery leaves its round
open for retry; only `Started` or `Success` completes it. If enqueue or engine
unavailability cancels execution, the gate and wake edge are cleared, but the
deploy remains stored for a later heartbeat rescan.

The implementation is divided across:

- `node/src/rust/instances/heartbeat_proposer.rs` for round scheduling;
- `node/src/rust/instances/proposer_coalescer.rs` for bounded concurrent
  admission;
- `node/src/rust/instances/proposer_instance.rs` for serialized execution; and
- `casper/src/rust/blocks/proposer/proposer.rs` for fresh permit validation and
  empty-block authorization.

## Live DAG synchronization

Heartbeat proposal recovery and stale-validator DAG synchronization solve
different problems. The heartbeat creates ordinary support blocks under a
permit when validated evidence is present but the LFB is not advancing. DAG
synchronization repairs a validator whose own latest message has become stale
because it is missing the currently supported branch.

The synchronization path keeps the engine in `Running`, requests ordinary
fork-choice tips from every currently connected peer, and admits missing tips,
parents, and justifications through the normal bounded block-retrieval and
certified-admission pipeline. It neither re-enters genesis initialization nor
installs a peer's finalization record. While synchronization is active, each
successfully admitted block schedules the idempotent local finalizer even when
the block height is outside the usual periodic finalization cadence.

Only the local finalizer can advance the durable floor. It freezes the local
eligible latest-message context, applies the existing exact causal and
state-preserving clique checks, verifies current-floor effect preservation, and
compare-and-appends against its local durable head. A peer tip cannot authorize
a proposal, count as a vote, or mutate the LFB. Proposal, receipt, replay,
validation, other validators, and other shards remain concurrent throughout
recovery.

The durable finalization ledger is local audit and crash-recovery state. Honest
nodes may reach the same finalized block through different intermediate local
rounds, so ledger revision and record digest are deliberately excluded from the
wire authority model. Cold or pruned-state checkpoint synchronization would
require a separately versioned canonical proof and is not part of this live
recovery path.

## Safety/Liveness rationale

- **Safety:** leader selection and proposal success are not votes. Every block
  remains subject to ordinary self-validation, independent peer replay, the
  unchanged mutual causal-clique certificate, the state-preserving certificate,
  and current-LFB effect preservation.
- **Liveness:** ordered leader rotation passes an offline validator, while retry
  semantics and one coalesced pending wakeup prevent a normal active completion
  from losing useful work; unavailable-service cancellation leaves stored work
  discoverable on a later tick.
- **Boundedness:** one active proposal plus one pending wakeup prevents recovery
  work from outrunning validation and replay; live synchronization reuses the
  bounded connection table, request tracker, processing queue, and transport
  byte budgets.
- **Cost-accounting independence:** proposal intent contains no purse, supply,
  budget, or settlement data. A deploy included during recovery follows the same
  state-bound funding, execution, RevVault settlement, and replay path as an
  ordinary pending-deploy proposal.

The complete normative contract is
[Heartbeat recovery and validation backpressure](theory/finalized-floor/finalized-floor-specification.md#211-heartbeat-recovery-and-validation-backpressure).
The operational pipeline is
[Liveness (Heartbeat Proposer)](casper/CONSENSUS_PROTOCOL.md#8-liveness-heartbeat-proposer).
