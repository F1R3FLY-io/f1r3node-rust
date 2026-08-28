# Finalized-floor TLA+ models

This directory contains the explicit-state and symbolic transition models for
Casper finalized-floor derivation, state preservation, validator recovery, and
parallel validation. The normative protocol description is
[`docs/theory/finalized-floor/finalized-floor-specification.md`](../../../docs/theory/finalized-floor/finalized-floor-specification.md),
and the proof and execution evidence is cataloged in
[`docs/theory/finalized-floor/finalized-floor-verification.md`](../../../docs/theory/finalized-floor/finalized-floor-verification.md).

## Model families

| Module | Boundary |
|---|---|
| `FinalizedFloor.tla` | deterministic floor walk, merge cap, and no lost parent write |
| `FinalizedFloorScan.tla` | complete parent-band scan |
| `FinalizerProgress.tla` | complete candidate search and restart-safe progress |
| `AccountableFinality.tla` | exact weighted asynchronous certificate support and accountable conflicts |
| `StateLineageFinality.tla` | causal certificate, state certificate, and committed-effect lineage |
| `StateEffectProvenance.tla` | exact accepted/rejected effect recurrence and arrival-order convergence |
| `StatePreservingForkChoice.tla` | distinct causal-parent/vote projections, exact floor backstop and evidence roots, GHOST-head preservation, maximal-antichain compaction, recovery coverage, deterministic depth expiry, finite-cap liveness, and protected-floor replay after LFB advancement |
| `StaleSiblingRecovery.tla` | asynchronous accepted-sibling delivery, exact-frontier settlement, source-bound tombstone propagation, rejected-occurrence buffering, unique recovery ownership, and converged rehome finalization |
| `ParallelValidatorConsensus.tla` | split node-local receive, capture, replay, validation, support, crash/restart, and atomic promotion transitions |
| `CertifiedFloorPromotion.tla` | complete causal discovery of dual-certified off-main floors |
| `CertifiedFloorCommitment.tla` | signed target-bound causal/state certificates committed by protocol-6 block headers |
| `FinalizationCertificateRetrieval.tla` | typed bounded sidecar retrieval, response validation, crash reconstruction, and one-time wakeup |
| `DependencyMaintenanceRound.tla` | frozen mixed block/certificate maintenance snapshots, all-obligation dispatch, and deferred first-error return |
| `LatestMessageCoverage.tla` | exact worklist refinement of pairwise supporter reachability |
| `FinalizerFloorMaterialization.tla` | proposal deferral, complete all-parent durable-finalizer discovery, exact target-bound dual certification, and local materialization |
| `SnapshotFloorMaterialization.tla` | complete provenance closure before snapshot selection |
| `HeartbeatFinalityBackpressure.tla` | bounded validator-local recovery, explicit ancestry/latest-message views, mutual causal/state cliques, and rotating leadership |
| `HeartbeatRecoveryCadence.tla` | one-time stall timeout followed by per-`check_interval` local recovery rounds |
| `TargetDeployTerminality.tla` | exact target-status observation with deadline-consuming RPCs, strict-height stall renewal, fail-loud finalized-history anomalies, and a non-renewable absolute bound |
| `PendingDeployHeartbeatComposition.tla` | concurrent pending-deploy ingress composed with selected recovery, retryable outcomes, terminal evidence, and deterministic occurrence disposition |
| `ProposerAdmissionCoalescing.tla` | serialized manual, pending-deploy, and finality-recovery proposal intents with one latched pending follow-up and fresh recovery permits |
| `RecoveryCommitteeTransition.tla` | separation of accepted post-state bond registration from finalized-floor authorization, justification, sequence, and synchrony-weight authority |
| `ObjectiveEquivocation.tla` | opposite-order durable sibling insertion, bond-incarnation-grouped evidence, pre-state authority, deterministic unary selection, filtered finality votes, crash/retry repair, and restart persistence |
| `ObjectiveEvidenceAuthorization.tla` | generation-and-epoch-first pair selection, exact merged-pre-state bond/generation authority, pair-only activation, scoped unary suppression, and proposer/receiver parity |
| `CertifiedCausalAdmission.tla` | opposite-order causal closure, rejected-wrapper traversal, accepted-only evidence propagation, proof-leaf isolation, per-incarnation normalization, complete dependency readiness, and ruleset-bound outcomes |
| `ProtocolV5EndToEnd.tla` | bounded three-replica refinement from parallel proposal and durable admission through finality, validator-incarnation custody, and deterministic cost settlement |
| `FinalizationAtomicity.tla` | parallel immutable evaluation, one-winner compare-and-append publication, stale-worker effect exclusion, and request/release wake ownership |
| `FinalizationWorkerRetry.tla` | failed-worker non-completion, bounded retry readiness, newer-success subsumption, and eventual request coverage with parallel workers |
| `ProposalFloorReadiness.tla` | typed proposal readiness across independent nodes, materialization-only finalizer scheduling, authority-defect isolation, and post-materialization progress |
| `FinalizationBoundHead.tla` | exact predecessor binding across parallel certificate evaluation, state-lineage revalidation, and compare-and-append; includes the DAG-descending but state-regressive late-binding counterexample |
| `FinalizationRecovery.tla` | crash/restart recovery of immutable rounds, ordered projection, independently receipted effects, contiguous completion, and safe compaction |
| `FinalizationGenesisIdentity.tla` | atomic pristine bootstrap, immutable genesis identity, write-free duplicate assertion after arbitrary head advancement, and rooted restart integrity |
| `GenesisApprovalTrust.tla` | local-minimum ceremony policy, candidate-declared threshold enforcement, distinct bonded-signature sufficiency, and write-free rejection |
| `DivergentFinalizationHistories.tla` | same-target convergence with node-local ledger revisions and record digests, and rejection of remote ledger identity as state authority |
| `WitnessEquivalentCarrier.tla` | semantic predecessor proof equivalence across divergent honest witness digests, exact carrier block/digest pairing, and state-bound park/wake behavior |
| `LiveMinorityForkRecovery.tla` | multi-peer tip discovery, dependency-first ordinary admission, local finalizer publication, retry after concurrent admission, and validator/shard-local framing |

## State-preserving fork choice

`StatePreservingForkChoice.tla` models proposal construction after a finalized-floor
advance with independently scheduled validators. Each node freezes its exact latest
messages and selected floor before deriving two projections: causal parents `C` and
floor-descending votes `V`. The model then inserts a floor backstop when required,
computes the reachability-maximal parent antichain, preserves the GHOST head at index
zero, roots evidence at both the floor and every exact latest message, applies
deterministic depth expiry, and permits recovery narrowing only after causal coverage
and floor ancestry are both established.

The safe configuration exhausts every local ordering of certificate delivery,
latest-message delivery, floor promotion, recovery observation, and proposal
capture. It generates 17,169 states, finds 808 distinct states, reaches depth 10,
and checks both temporal properties. The zero-depth configuration is the
constructive liveness witness: a permanently old disjoint tip expires from the
live proposal frontier while remaining an evidence root. Its paired no-expiry
control violates proposal liveness. The remaining controls isolate the boundaries
rather than combining defects:

| Configuration suffix | Expected result | Defect isolated |
|---|---|---|
| none | pass | complete concurrent safety and liveness contract |
| `depth_expiry` | pass | zero-depth deterministic expiry remains safe and live |
| `parent_uses_votes_unsafe` | counterexample | using `V` as state dependencies drops an admitted stale sibling |
| `invalid_stale_unsafe` | counterexample | stale status alone admits an intrinsically invalid dependency |
| `deploy_promotion_unsafe` | counterexample | deploy policy replaces the GHOST main parent |
| `omit_floor_evidence_unsafe` | counterexample | an all-stale snapshot loses the certified floor evidence root |
| `skip_antichain_unsafe` | counterexample | parent compaction fails reachability maximality |
| `recovery_floor_unsafe` | counterexample | recovery narrows through a parent outside the selected floor |
| `parent_cap_liveness_unsafe` | temporal counterexample | a finite cap below the live frontier permanently blocks proposals |
| `parent_depth_liveness_unsafe` | temporal counterexample | a depth horizon without deterministic expiry permanently blocks proposals |

TLC exhausts the finite schedules with `TypeOK` enabled and checks both temporal
properties. Apalache checks representation closure separately through bound 3 and
partitions the projection, evidence/state, and depth-expiry invariants across
four-transition proposal phases,
then checks each safety counterexample symbolically. The Apalache configurations begin at an already
certified `F` and reuse the model's unchanged latest-message, recovery, and proposal
actions; certificate delivery and promotion remain in the exhaustive TLC instance.
TLC remains the authority for the two temporal negative controls.

The detailed transition relation is node-local: every mutable component is indexed
by node, one transition changes one index, and another node's update cannot affect
local enablement or a reached local goal. The axiom-free Rocq module
`NodeLocalProductLifting.v` lifts any locally preserved invariant through an
arbitrary finite schedule over an arbitrary node type, proves that distinct-node
updates commute pointwise, proves adjacent independent schedule steps equivalent,
and proves that another node cannot disable local work or erase a reached goal.
`ParallelValidatorConsensus.tla` separately exhausts the interacting three-validator
receive, replay, support, crash, and promotion transitions. This compositional
product proof replaces redundant Cartesian-product enumeration without serializing
validators or removing any local transition.

## Stale-sibling settlement and recovery

`StaleSiblingRecovery.tla` composes the lifecycle seam between causal parent
selection and occurrence recovery. Three validators independently receive
accepted siblings `A` and `B`; after `B` becomes the floor, `A` remains a causal
input but ceases to be a finality vote. An exact `{A, B}` settlement emits a
source-bound tombstone for `A`, every observer records that occurrence in the
rejected buffer, and only the committed-view recovery leader may publish the
fresh rehome. Finalization must preserve `B` and converge on the effects of
`A`, `B`, and the fresh work.

TLC exhausts 1,508 generated / 451 distinct states to depth 20 and checks
`RecoveryCompletes` under weak fairness. Apalache checks every safety invariant
through bound 14, which reaches a complete settlement, rehome, and finalization
path. Seven controls independently drop the accepted stale sibling, truncate
the settlement frontier, replace the source identity with a signature-only
tombstone, omit rejected buffering, suppress selected recovery, regress the
floor effect, or permit a nonleader retry. TLC and Apalache reject every control
for its designated invariant.

The executable refinements are the staged Rust regression
`resolved_asymmetric_frontier_rehomes_excluded_local_deploy` and
`loom_consensus_projection_freeze::parallel_stale_sibling_settlement_authorizes_exactly_one_recovery`.
The former uses a real initialized map cell, observes settlement before retry,
and requires all nodes to finalize the same one-datum map. The latter explores
parallel floor publication, settlement observation, buffering, and competing
leader/nonleader recovery attempts.

## Atomic publication and recovery

`FinalizationAtomicity.tla` begins after the unchanged Casper certificate logic
has selected a candidate. It permits parallel workers to evaluate the same
durable predecessor, but only an exact compare-and-append may publish the next
round. Five negative controls independently reproduce a split head/record write,
an effect before commit, stale overwrite, regressive publication, and a lost
request/release wake. A stale or duplicate append rebinds to the fresh durable
head and reevaluates; it does not falsely complete the request that exposed the
new head. TLC exhausts 173,196 generated / 37,093 distinct states to depth 27.

`FinalizationWorkerRetry.tla` refines the scheduler's worker-exit boundary. A
request is complete only after a successful evaluation covers its ticket. An
error or panic consumes its worker slot without advancing completion and makes
the uncovered ticket retryable after bounded backoff. A newer successful worker
may cover an older failed ticket, so retries never regress completed coverage.
TLC exhausts 658 generated / 311 distinct states to depth 13, including two
parallel workers and bounded repeated failures; Apalache checks the safe model
through length 12. The failure-as-completion control violates the contract after
one request, launch, failed exit, and false completion under both checkers.

`FinalizerFloorMaterialization.tla` closes the discovery seam between the
all-parent proposal floor and the durable finalizer. Two nodes receive four
validator tips independently. A `1/3/5/7` stake topology makes one secondary
candidate exactly dual-certified, leaves one sibling at the strict 8-of-16
boundary, and removes another sibling from state support. The safe model exhausts
9,289 generated / 1,849 distinct states to depth 15 and eventually materializes
the surviving secondary candidate at both nodes. Apalache checks the same
invariants through length 8. The main-parent-only control violates complete
candidate discovery; the causal-only control violates exact selected/requested
target binding by substituting the rejected sibling. Both controls fail under
TLC and Apalache for their named reason.

`ProposalFloorReadiness.tla` connects that scheduler contract to proposal
admission without collapsing distinct failure classes into one retry. A
certified candidate ahead of the locally materialized floor defers and requests
finalization. Incomplete committee slots, inactive candidate authority, and a
stale recovery permit defer without scheduling finalizer work. Creation requires
all four readiness predicates. TLC exhausts 1,612,009 generated / 93,636
distinct states to depth 21 over two independently evolving nodes. Apalache
checks the safe model through length 8. Three mutation controls independently
produce missing-request, authority-defect hot-loop, and readiness-bypass
counterexamples under both checkers.

`FinalizationBoundHead.tla` refines the append identity from a numeric revision
to the exact predecessor block and its state. Its safe model exhausts 101
generated / 71 distinct states to depth 5. The required late-bound control
produces `F0 -> F1 -> C`: both candidates were valid from `F0`, and `C`
DAG-descends from `F1`, but `C` drops `F1`'s active effect. TLC and Apalache both
violate adjacent state preservation when an old certificate is rebound to the
new head; both pass when a changed predecessor makes the worker stale. Apalache
checks the safe and unsafe projections through bound 6.

`FinalizationRecovery.tla` then separates the durable head from metadata
projection, post-finalization effects, and receipt compaction. Projection and
effects may resume after arbitrary crashes. Out-of-order effect completion is
remembered, but the effects cursor closes only a contiguous prefix. The
compaction cursor cannot exceed that prefix. Three negative controls reproduce
a projection gap, an effect before projection, and an effects cursor crossing a
completion gap.

`FinalizationGenesisIdentity.tla` composes two clients and two append workers
over one rooted ledger. Exact concurrent genesis assertions may race, but one
atomic initializer wins and every later exact assertion is a write-free
identity operation. Append and restart preserve the root while the head grows.
Four required controls independently reset an advanced head, overwrite the
root, expose a split bootstrap, and backfill an unrooted historical head. TLC
exhausts the safe finite graph; Apalache checks the symbolic bounded transition
system and reproduces every control.

`GenesisApprovalTrust.tla` covers the authorization boundary immediately before
that atomic installation. A candidate may require more signatures than a node's
local minimum, but never fewer; its declared threshold must be satisfiable by the
positive bonded set and met by valid distinct bonded signatures. Rejection cannot
write genesis, approved-block, or engine state. Controls reproduce validation
against the local minimum, threshold downgrade, counting invalid or duplicate
signatures, and mutation on rejection.

`scripts/check-finalization-atomicity.sh` runs SANY, the exhaustive finite TLC
instances, bounded Apalache checks, and every required unsafe control, including
the worker-failure retry boundary and typed proposal-readiness boundary. The Rocq
refinement is in `formal/rocq/finalized_floor/theories/FinalizationAtomicity.v`,
with proposal refinement in
`formal/rocq/finalized_floor/theories/ProposalFloorReadiness.v`, and the
ceremony-authorization refinement in
`formal/rocq/finalized_floor/theories/GenesisApprovalTrust.v`. The
executable weak-memory refinement is
`formal/loom/cost_accounting/tests/loom_finalization_atomicity.rs`.

## Live minority-fork recovery and local ledger identity

An approved genesis is the immutable ceremony-authorized block at height zero.
A running node's finalization ledger is a local crash-recovery and audit log, not
a portable consensus object. Honest nodes can finalize the same target through
different local rounds, so their ledger revisions and record digests can differ
without a consensus disagreement.

`DivergentFinalizationHistories.tla` makes that distinction explicit. Its safe
model permits two validators to reach the same finalized target through
different local histories while retaining distinct revisions and digests. The
required remote-ledger control violates safety when one node treats another
node's local ledger identity as authority. Apalache checks the safe projection
through length 5 and reproduces the same control.

`WitnessEquivalentCarrier.tla` closes the next induction boundary. A portable
proof is eligible by accepted causal membership, protocol, predecessor floor,
and predecessor post-state—not by equality with the receiver's local witness
digest. The selected carrier hash and its committed digest remain an inseparable
pair. TLC exhausts 961 reachable two-node states. Apalache checks the safe model
through length 5. Exact-local-digest gating, floor-only matching, local-digest
copying, and missed semantic wakeup each violate their named invariant under both
checkers. The axiom-free unbounded refinement is
`formal/rocq/finalized_floor/theories/WitnessEquivalentCarrier.v`; executable
coverage includes the selector property test, the exact asymmetric-frontier
integration regression, and `casper/tests/loom_finalization_carrier_wakeup.rs`.

`LiveMinorityForkRecovery.tla` keeps a stale validator in `Running`. Peers send
ordinary fork-choice tips, missing parents and justifications pass through the
normal bounded block-retrieval and certified-admission path, and only the local
finalizer may publish the next durable floor. Receipt, replay, validation,
proposal, and other validators and shards remain concurrent. Accepted recovery
dependencies schedule another local finalization pass, so a batch arriving
between capture and publication is not lost. The safe bounded instance explores
264,205 generated / 16,984 distinct states to depth 16. Remote-head assignment,
publication before dependencies are admitted, and global proposal pausing each
violate safety in their required controls. Apalache checks the safe projection
through length 6 and reproduces all three controls.

The live recovery authority boundary is therefore:

```math
\operatorname{PeerTip}(B)
\rightarrow \operatorname{OrdinaryAdmission}(B)
\rightarrow \operatorname{LocalFinalizer}(\operatorname{FrozenContext})
\rightarrow \operatorname{CompareAndAppend}(\operatorname{LocalHead}).
```

A peer tip is discovery evidence only. It cannot install a remote finalization
record, replace genesis, manufacture a vote, or pause unrelated proposal work.
Cold or pruned-state checkpoint synchronization would require a separately
versioned proof whose identity is canonical across nodes; no such wire protocol
is implemented by these models.

## Heartbeat recovery contract

`HeartbeatFinalityBackpressure.tla` gives every validator an independent local
round and gives every produced block an explicit ancestry set and captured
latest-message view. Promotion requires an exact certificate over a mutual
causal clique and, independently, an exact certificate over a mutual
state-preserving clique. The latter refines causal support. Leadership authorizes
one proposer for a local `(LFB, round)` view; it does not manufacture certificate
support.

The eventual-synchrony configuration sets `DeliveryWithinRound = TRUE` and
`BoundedRecoveryScheduling = TRUE` for the delivery and bounded-task-scheduling
premises used by `Live_RecoveryRotatesPastOfflineLeader`. TLC exhausts 22,468
generated / 4,194 distinct states to depth 30. The existing-candidate variant
exhausts 22,960 generated / 4,338 distinct states to depth 30. The asynchronous
configuration disables both assumptions, permits validators to advance their
local rounds independently, and checks the complete safety invariant set without
the liveness property; TLC exhausts 113,968 generated / 17,766 distinct states to
depth 30. Thus neither delivery nor bounded relative task scheduling is a safety
premise. Apalache independently checks the composed safe projection through
bound 5, the asynchronous projection through bound 4, and reaches a real
promotion witness at bound 1.

`MC_HeartbeatFinalityBackpressure_asymmetric.cfg` repeats the exhaustive
eventual-synchrony check with stake weights `1/4/5`. Neither online validator's
weight is a hard majority alone; the exact causal and state certificates require
their combined `4+5` mutual clique. It exhausts 22,468 generated / 4,194 distinct
states to depth 30 and preserves liveness past the offline first leader.

`HeartbeatRecoveryCadence.tla` separates the initial stall threshold from the
later retry cadence. For
$`T_0 = \max(\mathtt{max\_lfb\_age},\mathtt{check\_interval})`$, round zero opens
at $`T_0`$, and at elapsed time $`d \ge T_0`$ the highest available local round is
$`(d-T_0) \mathbin{\mathtt{div}} \mathtt{check\_interval}`$. Elapsed clocks may jump arbitrarily; attempts
consume the earliest uncompleted round, and the completed set is proved to be a
prefix. TLC exhausts 1,123,849 generated / 287,496 distinct states to depth 26.
Apalache checks the same invariants through bound 10.
`MC_HeartbeatRecoveryCadence_collapsed_unsafe.cfg` instead reuses the stall
timeout for every round and violates `Inv_CadenceMatchesContract`, reproducing
delayed post-stall recovery under an immediate elapsed-time jump.

## Exact target-deploy observation

`TargetDeployTerminality.tla` treats the node's exact deploy status as an opaque
consensus result and verifies only the external observer policy. Status and LFB
requests may advance the monotonic clock by an arbitrary bounded amount in one
transition. The safe model resolves an expired stall or absolute budget before
interpreting a boundary response, renews the stall budget only for a strict LFB
height increase after the first baseline, fails on finalized-height regression
or same-height revision, and succeeds only on the target's exact `Finalized`
status.

TLC exhausts 19,444 generated / 1,155 distinct states to depth 5. Apalache
checks the same invariant set through length 8. Five independent controls expose
a fixed total timeout during useful progress, hidden finalized-history anomaly,
LFB progress substituted for target success, a terminal response bypassing an
expired boundary, and first-baseline renewal. The late-terminal control is the
deadline-ordering witness: with interpretation before expiry, a blocking status
request can report success at the stall boundary; the safe model reports only an
inconclusive timeout.

## Proposal scheduling and pending-work composition

`PendingDeployHeartbeatComposition.tla` separates work retained in the deploy
pool from work that is currently admissible. A retained deploy whose bounded
attempt or occurrence allowance is exhausted cannot occupy a proposal, but it
also cannot mask an authorized recovery attempt. When the selected recovery
leader has admissible pending work, the model creates one `PendingRecovery`
proposal: the proposal carries the deploy and simultaneously supplies recovery
support. Otherwise recovery may produce an empty block. Pending work leaves the
pool only after globally terminal evidence; `Empty`, `Deferred`, and `Failed`
remain retryable and do not complete the recovery round.

The primary liveness projection exhausts 1,213,239 generated / 296,424 distinct
states to depth 32. It includes two initially pending deploys, transient proposal
failure, rotating recovery across every finalized-height residue, exact
occurrence winners/losers, floor certification, and eventual terminal status.
The projection fixes the observationally irrelevant parent-committee mode while
all safe recovery selection, eligibility, justification, and sequence predicates
are finalized-floor derived. The ingress-safety configuration exhausts all
aligned, self-selected, and disjoint parent-committee modes, and all finalized
height residues: 2,892,275 generated / 551,136 distinct states to depth 27.
The separate ingress-safety configuration permits one new submission per
validator/deploy pair. Its smaller liveness-free state space isolates the queue,
attempt, duplicate-occurrence, deterministic-disposition, pool-removal,
recovery-reservation, and terminal-evidence invariants from the initial-work
liveness proof.

Nine TLC negative controls each remove one premise. Seven violate their named
invariant: retryable outcomes cannot close a round, pool removal requires terminal
evidence, a recovery execution requires its reservation, recovery leadership must
use the finalized-floor committee, a floor-selected leader must remain eligible,
creator justification and sequence-number derivation must use the exact floor
committee, and duplicate occurrences remain bounded. The pending-masks-recovery
and fixed-offline-leader controls violate their sole configured temporal property.
Snowcat and exhaustive TLC discharge the complete state-type obligation. Apalache
separately checks scheduler/occurrence and finalized-floor recovery semantic
projections through bound 6 and checks
`TypeOK` alone through bound 2; this division keeps symbolic checks tractable and
their conjunction is `SemanticSafety`. It does not claim unbounded TypeOK
coverage from Apalache. The symbolic safe projection tracks one deploy submitted
at both validators and three occurrence slots; the exhaustive TLC instances retain
two independently pending deploys. Six symbolic invariant
controls run through bound 6. These bounded checks complement rather than replace
the exhaustive TLC results.

`ProposerAdmissionCoalescing.tla` models the runtime gate as `Idle`, `Active`, or
`ActiveDirty`. A pending-deploy collision changes `Active` to `ActiveDirty`; any
additional pending collision in that dirty epoch is absorbed. Completing the
active proposal schedules exactly one forced, non-empty `PendingDeploy` follow-up.
Manual collisions return busy. Recovery collisions retain one external-heartbeat
retry obligation. An empty proposal requires a `FinalityRecovery` intent whose
captured floor identity and height still match the fresh live LFB immediately
before execution. The captured round is not compared with a global round oracle;
it is reused to recompute the selected leader over the fresh LFB-derived committee. Ordinary
non-finalized head growth does not stale the permit. The model enables the shard
heartbeat empty-block capability explicitly. Cancellation and engine-unavailability
wake loss remain outside this coalescer model: the Rust Loom tests cover the gate
reset, while the persistent-pool retry obligation is covered by the composition
model above.

TLC exhausts 1,582 generated / 646 distinct states to depth 12. Its three
controls independently demonstrate ambient asynchronous empty-block authority,
a lost pending wake, and acceptance of a stale recovery permit. The canonical
gate registers the safe Apalache projection through bound 6 and all three
symbolic controls through bound 6. No configuration needs a same-named wrapper:
the gate passes each configuration directly to its substantive module.

## Committee transition and block-bond roles

`RecoveryCommitteeTransition.tla` distinguishes values that must never be
conflated. Replay derives `bonds_of(post_state(B))`, and an accepted block must
serialize an exactly equal post-state cache so
an accepted block can register newly bonded validators. The authority to create,
justify, sequence, and synchrony-check that same block is instead
`bonds_of(post_state(floor(B)))`. A bond introduced by `B` therefore cannot
authorize `B`; it becomes eligible only after the accepted cache is registered
and a later finalized-floor promotion incorporates it. Invalid blocks may retain
sender evidence outside this abstraction, but their arbitrary bond cache cannot
register validator slots. Creator justifications and next sequence numbers are
packaged from the same unfiltered latest-message metadata, including invalid-block
evidence retained for equivocation detection.

Root and slot creation are modeled as distinct admission paths. Ordinary network
blocks must cite a parent; only the separately approved canonical genesis may be
parentless. A non-genesis justification key must equal the cited block sender,
while the canonical approved-genesis citation alone may use the placeholder key.
Accepted transition registration creates slots only for positive replayed
post-state bonds, seeds every new slot with the immutable canonical genesis, and
cannot derive that identity from local invalid height-zero junk or arrival order.
Invalid blocks from unregistered senders cannot allocate LMM slots. Invalid or
equivocating LMM entries remain available as evidence but are projected out of
agreement and finality-certificate inputs. Re-inserting an already stored approved
genesis backfills a missing legacy canonical index before the duplicate return;
a conflicting approved hash cannot replace that index.

The model holds LFB-relative height fixed until floor promotion, uses finalized-
floor weights for both the synchrony numerator and denominator, and rechecks a
queued recovery against its captured floor before start or validation. Its
fourteen negative controls independently expose same-block post-state authorization,
head-filtered creator justification and sequence loss, promotion before
registration, head-weight synchrony drift, a serialized/replayed cache mismatch,
valid-only next-sequence derivation after invalid latest-message evidence, and
registration from an invalid block, plus each root/key/genesis/LMM/positivity/
legacy-index admission defect above. TLC exhausts 1,061,249 generated / 153,856
distinct states to depth 18. Apalache checks the same safe invariants through
length 6 in 63.64 seconds, and each control is reproduced by both checkers.

## Composed protocol-v5 refinement

`ProtocolV5EndToEnd.tla` composes three independent replicas and validators in
one finite transition system. Equal-sequence sibling proposals may be created
before either reaches durable storage and may then arrive at every replica in an
arbitrary order. Admission records intrinsic validity, the proposal's exact
pre-state generation and stake certificate, and an exact vault reservation.
Durable evidence is grouped by bond generation and is repaired from stored
metadata after a crash or duplicate retry. Finality projects objective offenders
out of delivered votes and records the frozen floor committee rather than the
mutable head committee.

The same state machine permits withdrawal, rebonding into a fresh generation,
generation-scoped slashing, quarantine custody, guilty or vindicated redemption,
and idempotent receipt retry. Replica-local replay must reproduce the canonical
cost, settlement must consume the exact reservation while conserving vault plus
fee value, and a finalized block must already be admitted, replayed, and settled
at the finalizing replica.

The canonical gate checks nineteen safe invariants and twelve single-defect
controls. The controls cover post-state certification, intrinsic-admission
bypass, order-dependent and generation-blind evidence, mutable-head finality,
unfiltered equivocator votes, missing crash repair, stale-generation slashing,
lifecycle-collapsing redemption, lost custody receipts, replay drift, and split
settlement. Apalache uses defect-specific reachability bounds from 3 through 18;
the safe symbolic projection uses bound 5 by default and completes in about two
minutes on the reference host. These bounded checks are an
integration refinement of the exhaustive component models, not a replacement for
their larger state spaces or the Rocq theorems.

The gate does not ask TLC to enumerate the unconstrained Cartesian product of
every admission, crash/restart, finality, custody, and settlement interleaving in
`FreeNext`. TLC has no partial-order reduction for those independent actions, so
that product repeats equivalent schedules while its frontier continues to grow.
Instead, TLC exhausts each concurrent component model and all twelve guided
cross-boundary defect traces, Apalache checks the composed safe transition system,
and the axiom-free Rocq capstone supplies the unbounded deductive obligations.
This separation is part of the verification contract: component exhaustiveness,
symbolic integration, and deductive composition must all pass.

## Protocol-6 composition

Protocol 6 preserves the `ProtocolV5EndToEnd.tla` cost, validator-incarnation,
admission, and finality core and adds three consensus-visible boundaries. The
signed block header commits the exact finalized-floor certificate through
`CertifiedFloorCommitment.tla`; an unavailable committed certificate is then
resolved through `FinalizationCertificateRetrieval.tla`. The latter separates
block and certificate dependency namespaces, retains bounded obligations across
failed sends and restart, accepts only requested shape-valid content-addressed
responses, and wakes each detached block at most once.
`DependencyMaintenanceRound.tla` closes the production caller boundary by
requiring one maintenance invocation to attempt its entire frozen mixture of
ordinary block and certificate work before returning the first dispatch error.

This is a deliberate compositional proof rather than a renamed or duplicated
version-5 model. TLC exhausts the finite retrieval transition graph and each
component model, Apalache checks their bounded symbolic products and isolated
unsafe controls, Rocq proves the unbounded contracts and capstone composition,
and Rust/Loom regressions bind the modeled transitions to production storage,
retrieval, parsing, restart, and concurrent response paths.

| Configuration suffix | Violated boundary |
|---|---|
| `post_state_certificate_unsafe` | exact proposal-pre-state certificate |
| `intrinsic_admission_unsafe` | intrinsic validity before admission |
| `order_dependent_evidence_unsafe` | sibling-order-independent evidence |
| `generation_blind_evidence_unsafe` | bond-generation evidence grouping |
| `head_committee_unsafe` | frozen floor committee |
| `unfiltered_finality_unsafe` | objective-equivocator vote exclusion |
| `retry_without_repair_unsafe` | duplicate-retry durable-index repair |
| `generation_blind_slash_unsafe` | current-incarnation slash authority |
| `restore_bonded_unsafe` | exact pre-quarantine lifecycle restoration |
| `lost_receipt_unsafe` | idempotent resolution receipt |
| `replay_drift_unsafe` | canonical replica replay cost |
| `split_settlement_unsafe` | replay/settlement charge equality |

### Finalization-certificate retrieval refinement

`FinalizationCertificateRetrieval.tla` gives two detached blocks distinct
certificate obligations while allowing only one live tracker entry. It separates
tracking, failed and successful sends, malformed, mismatched, unsolicited, and
duplicate responses, dependency resolution, queue wakeup, one crash, and restart
reconstruction. Retry attempts use the finite control abstraction `{0, 1, 2+}`;
the abstraction preserves whether an attempt is new, retried, or saturated while
keeping the exhaustive graph finite. Weak fairness covers every transition that
can make a fetchable persistent obligation progress.

TLC exhausts 58,184 generated / 11,879 distinct states to depth 18 and proves
the safety invariants and `AllDetachedBlocksEventuallyQueue`. Apalache checks the
same safety boundary through symbolic length 12. The controls are isolated so
each checker must report one named failure:

| Configuration suffix | Expected invariant violation |
|---|---|
| `untyped_unsafe` | `TypedDependencyNamespaceIsDisjoint` |
| `validation_unsafe` | `OnlyValidResponsesPersist` |
| `unsolicited_unsafe` | `UnsolicitedResponsesDoNotMutate` |
| `failed_send_unsafe` | `FailedSendsRetainObligations` |
| `restart_unsafe` | `RestartNeverStrandsPersistentObligations` |
| `duplicate_wake_unsafe` | `EveryBlockIsQueuedAtMostOnce` |

The axiom-free unbounded contract is
`FinalizationCertificateRetrieval.v`. Production conformance is checked by the
block-store and parser properties, retriever failure/all-digest tests, restart
reconstruction test, async request/response tests, and the Loom duplicate-
response schedule model.

### Dependency-maintenance round refinement

`DependencyMaintenanceRound.tla` freezes two ordinary block obligations and two
certificate obligations, then explores every success/failure and attempt order.
The safe transition removes each attempted obligation from the pending set,
records the first failure, and completes only after the snapshot is exhausted.
This is the caller-level property that the leaf certificate retriever alone
cannot establish: a failed ordinary block send must not prevent certificate
maintenance later in the same production invocation.

TLC exhausts 348 generated / 158 distinct states to depth 7 and proves the full
snapshot partition, complete-attempt, first-error provenance, cross-type
non-starvation, and eventual-completion properties. Apalache checks the same
safety boundary through symbolic length 8. The `abort_unsafe` configuration
returns on the first failed send; TLC and Apalache both violate
`FailureNeverDiscardsUnattemptedObligations` by depth 3.

The axiom-free unbounded list induction is `DependencyMaintenanceRound.v`.
Production conformance is exercised in the block processor's ordinary and stale
maintenance paths, the block retriever's mixed retry path, and the LFS
requester's await-all regression. Sequential per-obligation dispatch remains
bounded and local; the LFS requester retains its existing parallel request set.

## Objective equivocation convergence

`ObjectiveEquivocation.tla` fixes three equal-sequence siblings and lets two
replicas durably insert them in opposite orders after stale-snapshot validation.
Sibling `A` belongs to an old immutable PoS bond incarnation; `B` and `C` belong
to the current incarnation. Their attacker-authored block epochs deliberately
disagree with those incarnations. Evidence is grouped by bond incarnation before
hash canonicalization, so a lexicographically earlier cross-incarnation pair and
adversarial block numbers cannot hide the current `B/C` pair. Acceptance ignores
replica-local invalid flags, retains both replay dependencies, and never changes
a sibling's validity retroactively.

DR-1 effects are incarnation- and fault-key-scoped. A cross-incarnation sibling
group suppresses unary fallback only for its exact `(validator, sequence)` key.
It neither authorizes a slash nor permanently retires the public key. Current-
incarnation evidence excludes the validator only for that incarnation; rebonding
restores voting eligibility. Independent unary evidence at another sequence
remains eligible and its selected hash is the deterministic minimum regardless
of observation order.

The candidate-authority projection uses canonical merged pre-state bonds. Thus
opposite local-invalid flags cannot make a same-block unbond/no-slash candidate
diverge. Durable insertion separately models metadata write, evidence-index
write, crash, and duplicate retry: retry repairs a missing evidence index before
restart recovery. Exact creator justifications remain structurally unchanged,
while finality votes project out invalid LMMs and the active objective equivocator.

`CertifiedObjectiveEquivocation.tla` also separates attributable admission from
objective-evidence eligibility at the signed sequence boundary. Its
`NegativeSequenceBlock` has a valid sender certificate and therefore persists as
rejected DAG metadata, but `EvidenceEligible` excludes it from the
generation-and-sequence evidence index. The two-replica
`MC_CertifiedObjectiveEquivocation_sequence_boundary.cfg` exhausts crash,
duplicate-retry, reconciliation, and opposite scheduling without admitting that
block as pair evidence. The `negative_sequence_unsafe` control removes only the
eligibility gate and reaches `Inv_IneligibleSequenceNeverBecomesEvidence` after
the invalid block is durably admitted. This is the signed-integer refinement of
the natural-number sequence domain used by the objective-pair proofs.

Fifteen controls independently expose unary evidence, local-invalid-dependent
acceptance, one-hash dependency closure, objective voting, cross-incarnation
unary fallback, volatile restart loss, permanent raw-key retirement, first-two
selection before incarnation grouping, overbroad unary suppression, block-epoch-
as-incarnation substitution, first-observed unary selection, post-state authority,
missing duplicate-retry repair, negative-sequence evidence indexing, and
unfiltered invalid finality votes. TLC exhausts the main safe model and the
signed-sequence boundary independently, and Apalache checks both safe models
through length 8 while reproducing all fifteen controls through length 8.
`ObjectiveEquivocation.v` proves canonical grouping,
adversarial-block-epoch independence, pair/dependency closure, non-retroactive
validity, incarnation-scoped vote exclusion/restoration, fault-key-scoped unary
precedence, unary-minimum permutation independence, pre-state-authority
convergence, duplicate-index repair, filtered-vote soundness, and restart
persistence without axioms.

## Objective evidence authorization

`ObjectiveEvidenceAuthorization.tla` refines the slash-authority decision that
`ObjectiveEquivocation.tla` previously represented only as a generation-level
abstraction. Each replica receives four hashes in a different order: one from
an old bond generation in the current activation epoch, one from the active
generation in an old epoch, and two eligible current siblings. The model keeps
bond generation, activation epoch, sequence number, canonical pair selection,
and the positive-bond authority root separate.

The safe transition system requires all of the following simultaneously:

1. filter by the canonical pre-state generation;
2. filter both evidence members by the proposed block's activation epoch;
3. choose the canonical pair only after both filters;
4. load positive bond and generation from the same pre-state authority;
5. activate authorization when objective evidence exists even without a local
   invalid-index entry;
6. suppress unary fallback only for its exact structural fault key unless an
   authorized objective candidate already consumes the offender slot; and
7. apply the same predicate at proposal and receipt.

TLC exhausts 769 generated and 256 distinct states to depth 17. The seven
controls restore pair-before-epoch filtering, cross-epoch authorization, stale
snapshot generation, stale snapshot bond, offender-wide unary suppression,
invalid-index-only activation, and receiver predicate drift. Each control must
violate its exact named invariant. Apalache independently checks the safe model
through length 12; its controls run through length 8. The bounded transition
checks are paired with the unbounded authority lemmas in
`ObjectiveEquivocation.v`, the six-case Loom interleaving suite, Rust
example/property regressions, and protocol-v5 differential fuzz targets.

## Certified causal admission

`CertifiedCausalAdmission.tla` gives two replicas opposite block-delivery
orders and independent ambient tracker state. Each candidate's certified
context is derived from its complete parent, justification, and evidence-proof
dependency closure. Structural traversal crosses rejected wrappers, but only an
accepted block propagates its stored evidence delta. Proof-root blocks are
checked as leaf facts rather than recursively importing their contexts. The
stable join retains the least proof for each validator incarnation.

The exhaustive TLC configuration uses individual per-replica deliveries and
checks all 12,773 generated / 2,800 distinct states to depth 37, including
eventual certification of both candidates at both replicas. The symbolic safe
configuration delivers the complete closure in one step so a length-5 bound
can reach four independently interleaved validations; Apalache completed that
bound without error in 485.372 seconds. This is a scheduling projection, not a
replacement for TLC's fine-grained delivery graph. The
partial-dependency control instead exposes the ready subset before the complete
closure to exercise premature certification directly.

Six controls remove exactly one boundary:

| Configuration suffix | Required violation |
|---|---|
| `rejected_barrier_unsafe` | rejected wrappers cannot stop structural traversal |
| `rejected_delta_unsafe` | rejected evidence deltas cannot propagate |
| `proof_context_unsafe` | proof roots remain leaf facts |
| `per_sequence_unbounded_unsafe` | one incarnation cannot retain an unbounded proof set |
| `ambient_tracker_unsafe` | receiver-local tracker state cannot alter a certified context |
| `partial_dependencies_unsafe` | validation cannot precede the complete metadata dependency closure |

`CertifiedCausalAdmission.v` proves the context-join laws, canonical bound,
accepted-only propagation, rejected-wrapper traversal, proof-leaf isolation,
ambient-state noninterference, exact evidence-delta classification, and full
outcome identity binding without assumptions. Loom separately explores
opposite concurrent deliveries, tracker races, and exact-versus-tampered
outcome publication.

## Parallel-validator contract

`ParallelValidatorConsensus.tla` is the concurrency capstone. It has no global
validation phase. Each validator/candidate pair owns a phase, and every action
updates one local participant or delivers one support message. The shared
history current-root pointer is explicitly modeled so the invariants can prove
that capture and publication depend on the node-local floor root instead.

The safe TLC configurations use three validators, two candidates, exact
$`40/35/25`$ weights, strict majority, and `FTT=0.1`; no validator can certify a
candidate alone. The baseline exhausts 12,877 generated / 3,411 distinct states
through eight actions to depth 9 without task failure. A second safe configuration
enables capture/replay crashes and restarts over the same bound. A third starts
from a history-consistent concurrency cut in which candidate 1 was accepted before
candidate 2 became the current floor; it exhausts 150 generated / 58 distinct
states to depth 3 and proves that candidate 1 cannot later erase candidate 2's
committed effect. Its eight unsafe configurations each enable one defect and must
fail:

| Configuration | Required violation |
|---|---|
| `MC_ParallelValidatorConsensus_causal_only_unsafe.cfg` | `AcceptedUsesExactReplay` |
| `MC_ParallelValidatorConsensus_early_support_unsafe.cfg` | `SupportRequiresLocalAcceptance` |
| `MC_ParallelValidatorConsensus_local_replay_unsafe.cfg` | `PromotedFloorUsesLocalReplay` |
| `MC_ParallelValidatorConsensus_shared_authority_unsafe.cfg` | `ExplicitFloorAuthority` |
| `MC_ParallelValidatorConsensus_shared_publication_unsafe.cfg` | `FloorPublicationIsAtomic` |
| `MC_ParallelValidatorConsensus_non_atomic_floor_unsafe.cfg` | `FloorPublicationIsAtomic` |
| `MC_ParallelValidatorConsensus_stale_floor_unsafe.cfg` | `CommittedEffectsRemainInFloor` |
| `MC_ParallelValidatorConsensus_crash_root_unsafe.cfg` | `ReplayRootsRemainLocallyRecorded` |

The baseline and crash-enabled Apalache configurations check all safe invariants
through routine bound 6. The stale-window safe configuration checks the same
pre-state through bound 2, while removing only the current-floor preservation
guard violates `CommittedEffectsRemainInFloor` in one step. The
crash-root-deletion configuration must also produce the
`ReplayRootsRemainLocallyRecorded` counterexample through bound 5. The
crash-safe routine receives a 600-second process allowance because its measured
bound-6 symbolic run takes more than five minutes on the reference workstation;
this changes neither its bound nor its invariant set. Bound 8 is retained as a
deep baseline symbolic check. TLC supplies exhaustive coverage of both finite
instances. The axiom-free Rocq module
`formal/rocq/finalized_floor/theories/ParallelValidatorConsensus.v` requires a
candidate root to have been recorded by local replay before promotion, proves
that promotion retains every recorded root, proves that restart preserves the
durable finalized tuple and root set, and supplies the arbitrary-node frame and
commutativity proof.

## Running

Run the focused bounded model with:

```bash
scripts/check-parallel-validator-consensus.sh
```

Run the canonical aggregate with:

```bash
scripts/check-finalized-floor-ALL.sh
```

The runner places state graphs under `target/verification`, uses one TLC worker,
and enforces explicit JVM and process memory ceilings. `/tmp` must not contain
retained model state after either command.

The shared runner emits a source/configuration hash and a recovery identity that
also binds the TLC binary, fingerprint polynomial, fingerprint seed, and worker
count. Checkpoint recovery requires the exact originating identity through
`TLC_RECOVER_IDENTITY`; missing or mismatched bindings are rejected before TLC
starts. This prevents a checkpoint created by one checker/source/fingerprint
instance from being mistaken for exhaustive evidence about another instance.
The same identity is recomputed after each run; concurrent input edits invalidate
the result.
