# Finalized-Floor Multi-Parent Merge — Normative Specification

This is the **normative** contract for the finalized-floor multi-parent merge: *what
must hold*, independent of *how it is checked*. The companion
[verification dossier](./finalized-floor-verification.md) records the mechanized
proofs, models, and tests that discharge each requirement; the
[glossary](./finalized-floor-glossary.md) defines every symbol and term used below
(read it first if any notation is unfamiliar). Rendered diagrams are in
[`diagrams/`](./diagrams/).

The local publication and restart boundary after a candidate has passed the
rules below is specified separately in
[Atomic finalization and crash recovery](./finalization-atomicity-and-recovery.md).

Requirement levels **MUST / MUST NOT / SHOULD** are used in the RFC-2119 sense.

---

## 1. Scope

The feature selects the base state on which a new block `B`'s multi-parent merge is
built, and folds the parents' unfinalized writes onto it. It touches floor
derivation (`casper/src/rust/finality/floor.rs`), the clique oracle
(`casper/src/rust/safety/clique_oracle.rs`), the merge driver
(`casper/src/rust/util/rholang/interpreter_util.rs`), and the number-channel
write-algebra (`rspace++/.../merging_logic.rs`,
`casper/src/rust/merging/conflict_set_merger.rs`,
`rholang/.../rholang_merging_logic.rs`). It also specifies last-finalized-block
progress in `casper/src/rust/finality/finalizer.rs` and
`casper/src/rust/engine/multi_parent_casper/finalization_runner.rs`, plus the
exact occurrence and effect projection in
`casper/src/rust/util/rholang/interpreter_util.rs`,
`casper/src/rust/merging/dag_merger.rs`, and
`casper/src/rust/merging/deploy_chain_index.rs`, with the consensus encoding and
persisted projection in `models/src/main/protobuf/CasperMessage.proto`,
`models/src/rust/casper/protocol/casper_message.rs`, and
`models/src/rust/block_metadata.rs`.

The state-effect-preservation rule is a node-consensus refinement, not a statement quoted
from either cost-accounting paper. The source-checkout papers
`../publications/cost-accounting/cost-accounted-rho.tex` and
`../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex` provide
the atomic resource-commitment and conservation obligations. The existing
Casper contract supplies the separate premise that finalized effects are
permanent. R-LFB-STATE is the implementation rule that composes those two
obligations: causal certification remains unchanged, while a separate
state-preserving certificate and current-LFB effect preservation prevent installation of a
state that omits an already committed resource transition.

## 2. The floor rule (normative)

For a block `B` with non-empty parent set `P₁…Pₖ` and frozen justification snapshot
`just(B)`:

- **R-FLOOR.** `floor(B)` MUST be the **maximum state-preserving sound candidate**
  (by block number, tie-broken by hash) over the union of three sources, each a
  **pure function of `B`**:
  1. **Inheritance** — every parent's own floor.
  2. **Advancement** — per parent, the highest main-chain ancestor `A` with
     `ft_witnessed(A, just(B)) > θ` (genesis is finalized by definition).
  3. **Universal certified advancement** — the highest all-parent DAG ancestor
     `U` for which both the causal and state-preserving clique certificates hold
     over `just(B)` and which preserves every inherited parent floor. `U` may be
     a secondary ancestor of every parent even when it is on no parent main spine.
- **R-SOUND.** The chosen `floor(B)` MUST be a **sound merge base**: either
  (**Case-A**) a general DAG-ancestor of every parent, or (**Case-B**) a candidate
  with which every other candidate is compatible (lies in its DAG past, or is
  mergeable via a common-descendant parent). The **highest** sound candidate MUST be
  chosen.
- **R-ERR.** If **no** candidate is a sound base, the derivation MUST return a
  deterministic error (incompatible finalized fork) — it MUST NOT silently pick an
  unsound base.
- **R-SNAP.** Finalization MUST be evaluated over `just(B)` only — never a node-local
  live DAG view.

### 2.1 Last-finalized-block progress

- **R-FINALIZER-SNAPSHOT.** One invocation MUST freeze the latest-message map once.
  Candidate discovery, agreement propagation, and clique decisions MUST all use
  that same map.
- **R-FINALIZER-CLOSURE.** Candidate discovery MUST visit the complete finite
  all-parent causal closure of every frozen latest message above the exact
  current LFB height. For each visited candidate, its propagated supporter set
  MUST equal the validators whose own frozen latest messages causally include
  that candidate. The descending `(block_number, block_hash)` worklist MUST
  process a block only after every higher child can propagate coverage to it,
  and MUST fail closed on a non-descending edge or unreadable metadata.
  A candidate-count cap, elapsed-time budget, or per-candidate timeout MUST NOT
  truncate this consensus search.
- **R-FINALIZER-ORDER.** The complete candidate set MUST be ordered by block number
  descending, then block hash descending. For each candidate, the finalizer MUST
  first run the existing exact
  mutual causal-clique decision without modifying its voters, weights, threshold,
  or strictness. It MUST then run the same exact mutual-clique decision over
  state-preserving support as required by R-STATE-CERT. The first candidate that
  holds both certificates and preserves every active effect of the current LFB is
  the next LFB.
  The current LFB need not lie on the candidate's main-parent spine: a
  multi-parent rebase may preserve it through a secondary parent.
- **R-FINALIZER-ERROR.** Missing or unreadable consensus metadata and failed clique
  evaluation MUST make the invocation inconclusive and return an error. They MUST
  NOT be treated as a negative finality vote or skipped in favor of another block.
- **R-FINALIZER-YIELD.** Cooperative yields MAY bound executor occupancy. Yield
  frequency MUST affect latency only: it MUST NOT change the snapshot, candidate
  closure, candidate order, or selected LFB.

- **R-FINALIZER-LOCAL-VIEW.** A node MAY invoke the finalizer over its current
  locally validated shard DAG, even while asynchronous delivery means that DAG
  is a proper subgraph of another honest node's view. It MUST use every latest
  message present in the frozen local view and the candidate's complete bonded
  stake map. Each active validator MUST retain one exact slot. A canonical
  genesis placeholder contributes no agreement, but its validator's stake MUST
  remain in the committee denominator. The implementation MUST NOT select an
  arbitrary branch, connected component, or validator subset and finalize it
  independently.
- **R-FINALIZER-RESTORE-HORIZON.** Full-history and restored nodes MUST classify
  a canonical genesis placeholder from its immutable identity, not local
  heldness. Both nodes MUST derive the same certified projection and context
  digest. A different missing latest-message body MUST make finalization
  inconclusive and preserve its exact dependency hash.
- **R-RESTORE-STARTUP.** Startup MUST reconcile durable latest-message slots
  before Casper enters the running state. Each resulting slot MUST contain the
  canonical genesis identity or held certified metadata. Reconciliation and
  snapshot capture MUST use one storage synchronization boundary.
- **R-RESTORE-SEQUENCE.** Reconciliation MUST select the greatest certified
  sequence for each retained validator key. Equal sequences MUST select the
  least block hash. A bond-generation change MUST NOT reset the per-key
  sequence. Evidence identity MUST retain the bond generation.
- **R-LATEST-MATERIALIZATION.** Online insertion and startup reconciliation
  MUST use one latest-message selection rule. The canonical genesis placeholder
  MUST compare below every recorded sender message, independent of sequence or
  hash. Higher sequences MUST win. Equal sequences MUST select the least hash.
  A missing noncanonical current entry MUST cause a typed consistency error
  before the node writes candidate state.
- **R-RESTORE-SUPPORT.** A certificate support manifest MUST retain the canonical
  genesis identity. Carrier selection and support traversal MUST NOT require its
  omitted body. Every other missing support block MUST fail closed.
- **R-FINALIZATION-CLOSURE.** The candidate that passes R-FINALIZER-ORDER is the
  one **directly finalized** block and the next shard LFB. Recording that decision
  MUST mark every previously unfinalized all-parent DAG ancestor indirectly
  finalized, because the candidate commits its complete causal history. Indirect
  ancestor marking is causal closure of one decision, not an independent vote or
  a sub-finalization rollup.
- **R-SHARD-FINALITY.** Finality is scoped to one shard DAG and its committee.
  Honest nodes independently derive the same shard LFB after receiving sufficient
  validated evidence. Separate shard LFBs MUST NOT be described or implemented as
  sub-finalizations that are automatically aggregated into a cross-shard global
  LFB; no such rollup exists in this protocol.

### 2.1.1 Heartbeat recovery and validation backpressure

Heartbeat recovery is proposal scheduling, not a finality decision. It MAY ask
the ordinary proposer to create a block, but every emitted block remains subject
to the unchanged Casper proposal, validation, causal-certificate,
state-certificate, and LFB-admissibility rules above.

- **R-HEARTBEAT-SEPARATION.** A heartbeat trigger MUST NOT directly mutate the
  LFB, manufacture support, alter clique membership, change validator weights,
  lower the hard-majority or exact-threshold tests, or bypass block validation.
- **R-PROPOSAL-INTENT.** Every proposal request MUST carry exactly one explicit
  intent: `Manual`, `PendingDeploy`, or `FinalityRecovery(permit)`. `Manual` and
  `PendingDeploy` requests MUST NOT authorize an empty block. Only a
  `FinalityRecovery` request whose permit remains valid at execution MAY request
  an empty block, and only when the node has the heartbeat capability enabled.
  A shared asynchronous flag, current LFB lag, or ambient proposer state MUST
  NOT grant empty-block authority.
- **R-HEARTBEAT-PROGRESS.** A node MUST measure finality stagnation from the
  task-local monotonic duration for which its observed LFB **hash** has remained
  unchanged. A block's producer-controlled timestamp, wall-clock age, frontier
  timestamp, latest-message churn, or block height alone MUST NOT reset or open a
  finality-recovery round. Observing a different LFB hash MUST reset the stall
  timer and completed-round history. Let
  $`T_0 = \max(\mathtt{max\_lfb\_age},\mathtt{check\_interval})`$. Recovery round
  zero MUST first open after the unchanged hash has been observed for $`T_0`$;
  after that one-time stall
  timeout, successive rounds MUST open every `check_interval`. Thus, for elapsed
  monotonic duration $`d \ge T_0`$, the highest available local recovery round is
  $`\left\lfloor (d-T_0)/\mathtt{check\_interval}\right\rfloor`$. If one or more earlier rounds were not
  completed because the task woke late, the implementation MUST expose the
  earliest uncompleted available round and MUST NOT skip directly to the highest
  round. The implementation MUST NOT reapply `T₀` as the interval between later
  rounds.
- **R-HEARTBEAT-COMMITTEE.** Heartbeat recovery leadership MUST use one
  canonical ordered committee derived from the captured LFB's post-state by the
  same `floor_committee` function that realizes R-AUTHORITY: PoS bonds filtered
  to the active validator set, then sorted and deduplicated. Non-finalized
  parent state, parent ordering, divergent head views, duplicate entries, and a
  single parent's bond list MUST NOT change recovery leadership. Empty
  committees MUST fail closed.
- **R-CARRIER-RETRY-CUSTODY.** Heartbeat recovery leadership MUST NOT authorize
  a rejected deploy retry. The rejected source carrier sender MUST keep custody
  of that carrier after exact settlement. Only that owner MAY retry the carrier
  after the shared floor gate opens. Different carrier owners MAY retry
  independent carriers concurrently.
- **R-HEARTBEAT-ROTATION.** Let `C` be that lexicographically ordered committee,
  `h` the non-negative current-LFB height, and `r` the zero-based local recovery
  round defined by R-HEARTBEAT-PROGRESS. Exactly one validator is authorized for
  a given `(LFB,r)` view:

  ```math
  leader(C,h,r) = C[(h+r) \bmod |C|].
  ```

  Empty committees and invalid negative LFB heights MUST fail closed. Advancing
  `r` MUST rotate past an offline leader without changing the finality
  certificate.
- **R-RECOVERY-PERMIT.** A finality-recovery permit MUST bind the exact observed
  LFB hash, its non-negative height, and the local recovery round. Immediately
  before proposal execution, the serialized proposer MUST obtain a fresh
  `CasperSnapshot` and verify that the permit hash equals the snapshot LFB, that
  the metadata height of that LFB equals the permit height, and that
  R-HEARTBEAT-ROTATION selects the local validator when given the permit's round
  and the fresh LFB-derived committee. The round is a captured input to leader
  selection; there is no independent global or proposer-owned “current round”
  against which to compare it. A non-finalized head-height change by itself MUST
  NOT stale a permit because head height is not LFB height. A stale-LFB,
  malformed, nonleader, or otherwise unauthorized permit MUST be deferred and
  MUST NOT create a block. Request-time validation alone is insufficient because
  the LFB may change while the request waits.
- **R-HEARTBEAT-LOCAL-ROUNDS.** Recovery round and completed-attempt history MUST
  be validator-local observations. Honest validators MUST NOT require equal
  clocks, simultaneous stall detection, or agreement on one global recovery
  round before admitting ordinary proposals.
- **R-HEARTBEAT-ONCE.** One heartbeat task MUST complete idle recovery at most
  once per observed $`(\mathrm{LFB},r)`$ view, and completed rounds MUST form a contiguous
  prefix. A nonleader closes its local round without proposing. A selected leader
  closes the round only after the serialized proposer reports that work started
  or succeeded. An already-busy proposer, deferred recovery, empty result, or
  failed proposal leaves the selected-leader round open for retry. The owning
  heartbeat task awaits the proposal result, so it cannot complete or advance
  that local round while its request is outstanding.
- **R-HEARTBEAT-WORK.** Pending user deploys retain their bounded ordinary
  proposal path. Receiving or observing a peer block, frontier movement, or
  latest-message divergence MUST NOT itself authorize a support proposal.
  Peer blocks contribute causal and state evidence only after ordinary local
  validation; they are not proposal authority. The automatic scheduler may
  propose only for locally admissible pending work or a locally due, selected,
  permit-bound recovery round.
- **R-PENDING-ADMISSIBILITY.** A deploy being retained in local storage MUST be
  distinguished from that deploy being admissible in the proposal's fresh
  snapshot. Future, expired, terminal, already-in-scope, duplicate-bound, or
  capacity-exhausted occurrences MAY remain stored while being inadmissible.
  Stored but inadmissible work MUST NOT mask an otherwise authorized empty
  recovery, and MUST NOT be included merely to make a recovery non-empty.
- **R-PENDING-RECOVERY-COMPOSITION.** When selected recovery and admissible
  pending work coincide, the node MUST submit the `FinalityRecovery(permit)`
  intent and the ordinary block creator MUST include admissible deploys using its
  unchanged deterministic selection rules. Recovery thereby reserves liveness
  service without suppressing useful work. If no deploy is admissible, only the
  still-valid recovery permit may authorize the empty block. Pending work MUST
  NOT consume, complete, or invalidate the selected recovery round before the
  proposer reports `Started` or `Success`.
- **R-PROPOSER-COALESCING.** Proposal execution MUST be single-flight and its
  admission state MUST distinguish idle, active, and active-with-pending-wakeup.
  A `PendingDeploy` request colliding with active work MUST latch at most one
  follow-up; any number of further pending collisions MAY coalesce into that
  same wakeup. Completion of the active proposal MUST atomically either return
  to idle or begin exactly one forced `PendingDeploy` follow-up against the
  current Casper engine and a fresh snapshot. Colliding `Manual` and
  `FinalityRecovery` requests MUST be classified `Busy` and return an empty
  trigger result rather than mutate the active request's intent; heartbeat
  recovery then retries the same uncompleted round.
  If enqueue fails or no current Casper engine exists before execution,
  cancellation MUST clear both active and dirty state because no follow-up can
  execute in that unavailable service instance. Cancellation MAY discard the
  coalesced wake edge, but MUST NOT remove the pending deploy itself; periodic
  heartbeat discovery MUST rescan retained storage after service becomes
  available.
- **R-HEARTBEAT-BACKPRESSURE.** Proposal admission MUST remain bounded by the
  single-flight proposer and its one-bit pending wakeup. Once the validator is
  already ahead, an idle recovery proposal MUST respect the configured
  unfinalized-DAG cap at the exact boundary. Pending-deploy admission remains
  subject to its lag cap, recovery cap, cooldown, and backstop; continuous empty
  recovery MUST NOT outrun block validation and replay.
- **R-HEARTBEAT-EVIDENCE.** A recovery proposal MUST carry ordinary block
  ancestry and the proposer's captured latest-message view. Finality support for
  a candidate MUST be derived from those explicit views: the certificate's
  supporters MUST form a mutual causal clique, and its state supporters MUST
  form a mutual state-preserving clique that refines causal support. Recovery
  leadership, round membership, block creation, or delivery alone MUST NOT count
  as either certificate.
- **R-HEARTBEAT-ASYNC.** Honest nodes MAY observe the same LFB stall at different
  times and therefore occupy different local recovery rounds. Such scheduling
  differences MAY produce ordinary concurrent blocks, but cannot create a local
  finality decision: R-FINALIZER-SNAPSHOT through R-SHARD-FINALITY remain the
  only promotion authority. Safety MUST hold without assuming delivery within a
  recovery round or bounded relative scheduling of heartbeat tasks.
  `DeliveryWithinRound` and `BoundedRecoveryScheduling` are only
  eventual-synchrony liveness assumptions. The latter states that, after the
  synchrony bound applies, an online task cannot complete more than one local
  recovery step ahead of another online task. Under those assumptions, fair
  validation, an online threshold-supporting committee, and finite replay time,
  rotating recovery and bounded admission MUST permit finality progress without
  unbounded backlog.

The idle-recovery portion of the scheduling algorithm below is normative
pseudocode and runs alongside the distinct pending-deploy path described by
R-HEARTBEAT-WORK. `completed` is local to one heartbeat task and one observed LFB
hash. `serialized_propose` is the ordinary Casper proposer; it does not grant
finality.

```text
observe_lfb(hash, now):
  if hash differs from observed_lfb:
    observed_lfb := hash
    observed_since := now
    completed := empty prefix

  elapsed := now - observed_since
  if elapsed < stall_timeout:
    return not_due

  highest := floor((elapsed - stall_timeout) / check_interval)
  round := first nonnegative integer not in completed
  if round > highest:
    return not_due

  if local_validator != leader(committee, lfb_height, round):
    append round to completed
    return nonleader_complete

  permit := (observed_lfb, lfb_height, round)
  result := serialized_propose(FinalityRecovery(permit))
  if result is started or successful:
    append round to completed
    return leader_complete

  return retry_same_round
```

Because `completed` is extended only by its first absent integer, it remains a
prefix under every wakeup sequence. Because the selected leader uses that integer
rather than `highest`, elapsed-time jumps cannot skip committee members.

The proposer interprets that intent only after it acquires the current engine
and a fresh snapshot. This second normative algorithm makes the empty-block
authority and pending-work composition explicit.

```text
execute_proposal(intent):
  casper := current_engine_casper()
  snapshot := casper.fresh_snapshot()
  lfb_metadata := snapshot.lookup(snapshot.lfb_hash)

  recovery_valid :=
    intent is FinalityRecovery(permit)
    and permit.lfb_hash = snapshot.lfb_hash
    and permit.lfb_height = lfb_metadata.block_number
    and local_validator = leader(snapshot.committee,
                                 lfb_metadata.block_number,
                                 permit.round)

  if intent is FinalityRecovery and not recovery_valid:
    return deferred

  allow_empty := heartbeat_capability and recovery_valid
  deploys := deterministically_select_admissible_pending(snapshot)
  if deploys is empty and not allow_empty:
    return no_new_deploys

  return create_validate_and_publish(snapshot, deploys, allow_empty)
```

There is deliberately no comparison between `permit.round` and a second current
round. The awaiting heartbeat task owns that local round; the proposer only uses
its captured value to recompute the leader. Likewise, changes to
`snapshot.latest_block_height` do not participate in recovery freshness.

The intent decides only whether an empty body is authorized. Deploy admission,
execution, state-bound cost certification, settlement, replay, validation, and
publication remain the same ordinary block pipeline for every non-empty body.

### 2.1.2 Exact target-deploy observation

Deploy finalization polling is an observer contract, not a second finalizer. The
client MUST consume `deploy_finalization_status` as the authority for the exact
target occurrence. It MUST NOT infer target success merely because the LFB moved,
because an intermediate finalized block can advance the state floor while the
target still lacks the later mutual-knowledge layer needed for terminal status.

- **R-TARGET-EXACT.** Success MUST require the target deploy's exact
  `Finalized` status. An LFB advance, target-block inclusion, a finalized
  descendant, or an unrelated certificate MUST NOT be substituted for that
  status. Exact `Failed` and `Expired` statuses observed before both deadlines
  remain terminal errors. A terminal response returned at or after an expired
  boundary is retained for diagnostics but MUST NOT convert the already-expired
  wait into success or a target rejection.
- **R-TARGET-STALL.** A progress-aware observer MAY renew its no-progress budget
  only after a strict increase in the observed LFB block number. Same-height
  hash changes, latest-message churn, request retries, and successful RPCs MUST
  NOT renew it. The first successful LFB observation establishes the comparison
  baseline and MUST NOT renew it. This differs intentionally from
  R-HEARTBEAT-PROGRESS: the
  heartbeat owns recovery rounds for an exact LFB identity, while the external
  observer is deciding only whether useful finality height continues to advance.
- **R-TARGET-HISTORY.** A lower LFB height or a different hash at the same LFB
  height is a finalized-history anomaly. The observer MUST fail loudly rather
  than treating it as progress or hiding it as a transient request error.
- **R-TARGET-ABSOLUTE.** A separate monotonic absolute deadline MUST bound the
  complete observation. No amount of LFB progress may renew that deadline. Each
  blocking status or LFB request MUST receive a deadline no greater than the
  remaining observation budget, assuming the transport honors its deadline.
  Budget expiry MUST be evaluated immediately after each blocking request and
  before interpreting that request's response or renewing progress.
- **R-TARGET-TIMEOUT.** Exhausting either budget reports an inconclusive timeout,
  not target rejection and not a consensus decision. The diagnostic SHOULD
  include the exhausted budget, elapsed duration, strict progress count, last
  observed LFB height/hash, exact target state, rejection count, and last RPC
  error.
- **R-TARGET-CLOCK.** Both budgets MUST use a monotonic clock. Wall-clock
  adjustment MUST NOT shorten or extend either bound.
- **R-TARGET-DURATION.** Poll, stall, and absolute durations MUST be positive
  and finite, and the absolute duration MUST be at least the stall duration.
  Invalid configuration MUST fail before the first status or LFB request.

For the integration harness, `finalization = 45s` is the no-progress budget and
`deploy_finalization_absolute = 135s` is the non-renewable execution bound. These
are operational test headroom (the latter is three no-progress intervals), not a
protocol-derived bound and not a claim that asynchronous Casper has a universal
45-second or 135-second finality guarantee. The reproduced bridge trace took
approximately 49 seconds: the target was valid and included, a genuine
intermediate LFB advance reset recovery cadence, and later support produced exact
terminal status. A fixed 45-second total deadline rejected that valid trace.

### 2.1.3 Atomic publication and crash recovery

Finalizer evaluations MAY overlap over immutable snapshots. Their durable
publication MUST satisfy all of the following requirements:

- **R-FINALIZATION-APPEND.** A successful round MUST atomically append one
  immutable round record and replace its exact predecessor head. At most one
  candidate may succeed for a given predecessor. A stale worker MUST publish no
  metadata and perform no post-finalization effect.
- **R-FINALIZATION-BASE.** Certificate evaluation, state-lineage validation,
  manifest construction, and compare-and-append MUST bind to one exact durable
  predecessor identity: revision, block hash, height, and record digest. If any
  part of that identity changes before append, the worker MUST discard the old
  result and repeat evaluation from a fresh coherent base. It MUST NOT substitute
  the current head into a certificate evaluated against an older head.
- **R-FINALIZATION-LINEAGE.** A successor MUST strictly increase block height
  and both DAG-descend from and preserve the active state of its exact bound
  durable head. The lineage predicates MUST be revalidated immediately before
  the atomic append. Equal-height siblings, unrelated candidates, state-dropping
  descendants, and regressive candidates MUST fail closed.
- **R-FINALIZATION-PROJECTION.** Committed rounds MUST be projected into block
  metadata in contiguous revision order. No effect may start before its round
  is projected. Restart MUST resume at the first unprojected revision.
- **R-FINALIZATION-EFFECTS.** Deploy removal, cosigner removal, runtime-cache
  eviction, and finalized-event publication MUST be idempotent and independently
  receipted for every block in the round manifest. A round-completion cursor may
  advance only across a contiguous prefix whose complete receipt sets exist.
- **R-FINALIZATION-COMPACTION.** Receipt compaction MUST persist the completed
  prefix before deleting receipts and MUST never advance beyond that prefix.
  A crash at any compaction boundary may retain redundant data but MUST NOT lose
  completion truth.
- **R-FINALIZATION-SCHEDULER.** Every accepted scheduling request MUST be covered
  by a launched evaluation after finite worker progress. Releasing dispatcher
  ownership concurrently with a request MUST NOT lose the wake. A worker error
  or panic MUST NOT complete its covered request; the uncovered request MUST
  become retryable after bounded backoff. A successful newer worker MAY subsume
  an older retry, but completed coverage MUST NOT regress. Worker bounds MUST
  constrain resource use without serializing immutable evaluation.
- **R-FINALIZATION-PROPOSAL-READINESS.** Before deploy selection or replay, a
  proposer MUST derive the exact candidate consensus context from its captured
  parents and latest messages and classify its relation to the durable context.
  Exact equality is ready. A strict state-preserving descendant is
  `FinalizedFloorMaterializationPending`, retains pending deploys, and
  idempotently requests finalization. A candidate ancestor of the durable floor
  is `CandidateFloorRegression`; a same-floor context mismatch is
  `CertifiedContextMismatch`; every other incomparable candidate is
  `CandidateFloorConflict`. Those permanent failures, incomplete committee
  slots, inactive candidate authority, and stale recovery permits fail closed
  without scheduling finalization.

The durable revision $`H`$, projection cursor $`P`$, effects cursor $`E`$, and
compaction cursor $`C`$ MUST always satisfy:

```math
0 \le C \le E \le P \le H.
```

These requirements refine local materialization only. They MUST NOT change
clique membership, stake weights, the exact fault-tolerance threshold, or which
candidate the Casper finalizer certifies.

### 2.1.4 Live minority-fork recovery

An approved genesis is the immutable ceremony-authenticated trust root at block
height zero. A running node's finalization ledger records that node's local
publication history. Its revision and record digest are not portable consensus
identities: honest nodes may finalize the same target through different local
rounds and therefore retain different ledger revisions and digests.

- **R-RECOVERY-GENESIS-SEPARATION.** `ApprovedBlock` MUST contain only canonical
  genesis. Live recovery MUST NOT enter the approved-genesis path, replace the
  canonical genesis hash, or redefine the shard trust root.
- **R-RECOVERY-LOCAL-IDENTITY.** A peer's finalization-ledger revision, record
  digest, or local witness MUST NOT authorize a local state transition. Equality
  of finalized target block and replay state does not imply equality of local
  ledger history, and inequality of local ledger history is not a consensus
  disagreement.
- **R-RECOVERY-TIP-AUTHORITY.** Staleness MAY trigger requests for ordinary
  fork-choice tips from connected peers. A received tip is discovery evidence
  only: it MUST NOT directly replace the LFB, install a peer record, authorize a
  proposal, or count as a finality vote.
- **R-RECOVERY-ADMISSION.** Missing tips, parents, and justifications MUST pass
  the ordinary bounded retrieval, stateless validation, certified admission,
  replay, and DAG publication path. Recovery MUST NOT raw-insert blocks into the
  DAG or bypass any protocol-version rule.
- **R-RECOVERY-LOCAL-FINALIZER.** Only the existing local Casper finalizer may
  publish a recovered floor. It MUST capture one frozen certified consensus
  context, apply both exact clique predicates and current-LFB state
  preservation, and compare-and-append against its exact local durable head.
  Peer responses MUST NOT invoke a second publication rule.
- **R-RECOVERY-RETRY.** While live synchronization is active, every successfully
  admitted block MUST request another idempotent local finalization pass even if
  its height does not meet the normal periodic finalization cadence. Admission
  concurrent with a finalizer capture therefore remains visible to a later pass.
- **R-RECOVERY-LOCALITY.** Recovery MUST leave the node in `Running` and MUST NOT
  globally pause proposal, receipt, replay, validation, finalization, other
  validators, or other shards. Existing proposal-readiness rules independently
  prevent creation from an unmaterialized certified floor.
- **R-RECOVERY-IDENTITY.** Recovery MUST preserve validator sequence number,
  bond generation, signed minority-fork blocks, and objective evidence. Age may
  trigger discovery but never authorizes a state transition.
- **R-RECOVERY-COLD-STATE.** Cold or pruned-state checkpoint synchronization is
  outside the live protocol. Any future checkpoint proof MUST be separately
  versioned and use an identity canonical across nodes rather than a local
  ledger revision or digest.

The live authority chain is:

```math
\operatorname{PeerTip}(B)
\rightarrow \operatorname{OrdinaryAdmission}(B)
\rightarrow \operatorname{LocalFinalizer}(\operatorname{FrozenContext})
\rightarrow \operatorname{CompareAndAppend}(\operatorname{LocalHead}).
```

This recovers an arbitrarily stale live node by normal DAG synchronization and
local recomputation. It neither adds a second finality protocol nor requires
honest nodes to share an identical local audit-log history.

### 2.2 State derivation and LFB admissibility

A **causal clique certificate** answers whether sufficient mutually agreeing
stake causally supports a block. A **state-preserving clique certificate** answers
whether sufficient mutually agreeing stake has retained that block's state in
its frozen latest messages. **LFB admissibility** additionally answers whether
installing the candidate would retain the state already committed by the current
LFB. These are separate predicates. The state-support calculation MUST NOT alter
the causal certificate, and the current-LFB preservation check MUST NOT alter
either certificate.

- **R-EFFECT-ID.** Every successful user or system execution MUST have the
  consensus identity $`E = (source\_block\_hash, execution\_index)`$. Execution
  indices MUST use the block's sequential user-then-system execution order.
  Failed executions MUST NOT originate active effects.
- **R-EFFECT-WIRE.** A current-protocol block MUST serialize the exact,
  lexicographically ordered, duplicate-free set of state-effect identities
  rejected by its parent merge. Validation MUST recompute that set and reject a
  mismatch or non-canonical encoding before replay acceptance. DAG metadata MUST
  persist the block protocol version, successful local indices, and rejected
  identities. Nodes MUST fail closed when those fields are unavailable.
- **R-EFFECT-ACTIVE.** Let `inputs(B)` contain every maximal direct parent of
  `B`, plus `floor(B)` when it is not already present. The active-effect set MUST
  satisfy this recurrence:

  ```math
  Active(B) = \left(Own(B) \cup
    \bigcup_{I \in inputs(B)} Active(I)\right) \setminus Rejected(B).
  ```

  `Own(B)` contains exactly the identities required by R-EFFECT-ID, and
  `Rejected(B)` is the canonical wire set required by R-EFFECT-WIRE. Parent
  ordering MUST NOT affect this set. Removing a parent already covered by
  another state input MUST NOT change the result.
- **R-STATE-PRESERVATION.** `preserves(A,D)` MUST hold exactly when `A = D`, or
  when `A` is a DAG ancestor of `D` and $`Active(A) \subseteq Active(D)`$. This
  is transition provenance rather than tuple-set inclusion: an authorized later
  reduction may consume data while still preserving the earlier transition.
- **R-EFFECT-SCAN.** An implementation MAY collect a height-bounded superset of
  rejected identities from `D`'s causal past when deciding `preserves(A,D)`.
  It MUST test only candidates active at `A`, and MUST return false exactly when
  one such candidate is inactive at `D`. Rejections unrelated to `A` MUST NOT
  change the verdict.
- **R-STATE-CERT.** For candidate `C` and frozen snapshot `just(B)`, state support
  MUST contain exactly the validators whose latest messages both causally include
  `C` and preserve every effect active at `C`. The node MUST run the same hard-majority,
  maximum-clique, exact-threshold decision used for causal certification over
  that restricted support. Causal support through a merge-parent edge MUST NOT
  count as state support when the merge state rejected `C`'s effects.
- **R-STATE-DEPENDENCIES.** Finalized-floor materialization MUST close over both
  DAG parents and the frozen latest-message justifications consulted by
  R-STATE-CERT. Active-effect evaluation MUST close over every recurrence input.
  A missing dependency or cyclic dependency MUST fail the derivation; it MUST NOT be
  interpreted as absent state support.
- **R-SNAPSHOT-PROVENANCE-CLOSURE.** Before parent eligibility, causal support,
  or state-preserving support is evaluated, snapshot construction MUST
  materialize finalized-floor provenance for the union of the captured LFB,
  every frozen latest message, and every declared parent that the evaluation
  may inspect. Each materialization MUST recursively close over its immutable
  block dependencies. Cache writes MAY interleave with finalizer materialization,
  but they MUST be monotone, idempotent, and order-independent. Selection MUST
  observe the complete required closure or fail; it MUST NOT observe a
  parent-only prefix, classify a latest message from missing cache state, or
  re-enqueue the same dependency-free block indefinitely.
- **R-STATE-FRONTIER.** A raw causally certified main-parent frontier MUST first
  be reduced to the highest candidate on that spine that preserves the accepted
  active-effect set, then lowered along that same main-parent spine until it reaches
  a state-certified candidate. A causally certified stale-state descendant or
  rejected parent remains a valid speculative block but MUST NOT become a floor
  advancement.
- **R-UNIVERSAL-FRONTIER.** Universal certified advancement MUST traverse the
  complete all-parent causal closure in deterministic descending
  `(block_number, block_hash)` order. Each declared parent supplies a distinct
  coverage identity. Coverage MUST propagate through every parent edge; a
  candidate is universal exactly when it has received every declared-parent
  identity. The first universal candidate that holds both unchanged exact clique
  certificates and preserves every inherited floor is the highest eligible
  universal frontier. Every traversed edge MUST descend strictly in block number.
  Missing metadata, a non-descending edge, a cycle, or coverage that arrives after
  a candidate was processed MUST fail derivation rather than select a partial or
  node-local result. This traversal adds a block-structural floor candidate; it
  MUST NOT change R-FINALIZER-CLOSURE, agreement propagation, voters, weights,
  threshold arithmetic, or clique selection.
- **R-COVERAGE-EQUIVALENCE.** For frozen latest-message map `J`, candidate `C`,
  and validator `v`, the propagated latest-message coverage predicate MUST be
  extensionally equal to the original pairwise ancestry predicate:

  ```math
  v \in Coverage_J(C) \iff C \preceq_{DAG} J(v).
  ```

  An implementation MAY seed one validator identity at each `J(v)` and propagate
  those identities through the causal closure once. It MUST process blocks in
  descending `(block_number, block_hash)` order, reject every non-descending
  edge, and fail if coverage reaches an already processed block. The weight map
  supplied to the clique oracle MUST remain the candidate main parent's weight
  map, or the candidate's own map for genesis. Filtering that map by
  $`Coverage_J(C)`$ MUST produce exactly the same supporter map and exact clique
  verdict as pairwise `is_dag_ancestor(C,J(v))` evaluation.
- **R-LINEAR-SNAPSHOT-REUSE.** Universal-frontier evaluation MAY reuse a parent's
  already derived result only when the child has exactly that one parent, that
  parent itself has exactly one predecessor, the inherited floor equals the
  parent's cached floor, the frozen latest-message maps are identical, and every
  latest message is strictly older than the parent. Under those premises the
  parent cannot certify itself and every other eligible ancestor was already an
  ancestor of its sole predecessor. A child of a multi-parent merge MUST rescan:
  the merge can make a formerly branch-local certified candidate universal even
  without new latest-message evidence.
- **R-FLOOR-STATE.** A candidate floor at or above an inherited parent floor MUST
  preserve every effect active at that inherited floor. A candidate that bypasses an inherited
  committed state MUST be skipped; an older common sound base MAY be selected.
- **R-LFB-STATE.** A candidate MAY become the next LFB only if its causal
  certificate, state-preserving certificate, and current-LFB effect preservation all
  hold over the same frozen snapshot. Main-parent ancestry MUST NOT be an
  additional admission condition: it describes vote propagation, not
  multi-parent state derivation.
- **R-VALIDATOR-LOCAL-TRANSACTION.** Candidate receipt, floor capture, replay,
  validation, support emission, support delivery, and promotion MUST be
  validator-local transitions. The implementation MUST NOT require a global
  validator phase or assume that honest validators execute these transitions in
  the same order.
- **R-LOCAL-ROOT-AUTHORITY.** Floor capture MUST bind one block/root/effect tuple
  from the validator's local state. Replay and promotion MUST use that explicit
  root or the exact locally replayed candidate root. A shared RSpace
  current-root pointer MAY change as other runtimes reset or checkpoint, but it
  MUST NOT authorize capture, validation, support, or publication.
- **R-LOCAL-SUPPORT.** A validator MUST emit support only after its own exact
  replay and final-state validation accept the candidate. Receiving another
  validator's support or mergeable evidence MUST NOT substitute for local
  replay. Delivered support MUST identify an actually emitted signer/candidate
  pair.
- **R-ATOMIC-FLOOR-PUBLICATION.** Promotion MUST atomically publish the
  candidate block, its verified root, and its exact committed-effect set. A
  reader MUST NOT observe a new block with an old state/root or an old block
  with a new state/root.
- **R-PARALLEL-FRAME.** Updating one validator's local phase or promoted state
  MUST leave every other validator's local state unchanged. Promotions by
  distinct validators MUST commute pointwise. Validators promoting the same
  candidate MUST publish identical root and effect values.
- **R-VALIDATION-RESTART.** A crash after capture or replay MUST publish no
  partial support or floor. Restart MUST clear the incomplete phase and
  recapture one current local tuple before replay. Recorded roots needed by an
  active or restarted validation MUST remain available.
- **R-PARENT-CAUSALITY.** Let `L` be the proposer-local LFB captured with a DAG
  snapshot and let `J(v)` be bonded validator `v`'s exact latest-message slot in
  that snapshot. Define `BaseEligible_L(v)` to mean that `v` has positive stake
  and an exact bond generation in `L`'s authority; `J(v)` has certified accepted
  admission; its non-genesis sender and certified generation match `v` and the
  floor generation; and the incarnation has no objective-equivocation evidence.
  Base eligibility MUST be decided before testing descent from `L`. The causal-
  parent and finality-vote projections are:

  ```math
  C_L(J) = \{J(v) \mid v \in \operatorname{dom}(J)
    \land BaseEligible_L(v)\},

  V_L(J) = \{T \in C_L(J) \mid L \preceq_{DAG} T\},

  V_L(J) \subseteq C_L(J),

  Candidates(J,L) = C_L(J) \cup
  \begin{cases}
    \{L\}, & \nexists T \in C_L(J) : L \preceq_{DAG} T,\\
    \varnothing, & \text{otherwise},
  \end{cases}

  DirectParents(J,L) = \max_{\preceq_{DAG}} Candidates(J,L),

  \forall T \in C_L(J),\ \exists P \in DirectParents(J,L) :
    T \preceq_{DAG} P.
  ```

  `V_L(J)` is the only input to LMD-GHOST weights, clique voting, fault
  tolerance, and finality. `C_L(J)` is the only input to declared-parent
  construction. Distinct hashes avoid duplicates, and reachability-maximal
  compaction avoids redundant direct parents without discarding causal
  evidence. Stale and effect-dropping tips in `C_L(J) \setminus V_L(J)` remain
  causal inputs; their above-floor deltas are replayed against the certified
  floor and may be rejected by the deterministic merge. Intrinsically invalid,
  unregistered, sender-mismatched, wrong-generation, or objectively
  equivocating tips are absent from both projections. Configured parent bounds
  MUST fail closed rather than omit an uncovered member of `C_L(J)`. At least
  one declared parent MUST descend from `L`; when no causal tip does, `L` is
  added explicitly before reachability compaction. An empty causal-parent
  projection is the degenerate instance of the same backstop rule. Parent
  construction MUST NOT fall back to genesis.

  Reachability compaction MUST run before bounds. For a finite parent-count cap,
  proposal construction MUST admit the complete frozen frontier exactly when its
  cardinality fits and otherwise return a typed, non-signing deferral. It MUST NOT
  silently truncate an uncovered live tip. `number-of-active-validators + 1`
  remains a sufficient worst-case provisioning bound, reserving capacity for one
  distinct tip per configured validator slot plus an independent floor backstop;
  falling below that bound produces a startup warning rather than rejection because
  the configured maximum is not the live committee or the compacted frontier.
  Finite depth expires a secondary causal tip
  only through the deterministic block-height horizon shared by all validators.
  The selected GHOST head is unconditional, and exact latest messages remain
  evidence roots even after their proposal dependency expires.
- **R-PARENT-EVIDENCE.** The complete latest-message evidence required by
  justification-following and validator sequence accounting MUST remain intact.
  The causal evidence closure MUST always be rooted at the captured floor and
  every exact latest-message hash, independently of the compacted or depth-expired
  declared-parent set.
  A receiver validates and replays the declared parents from block-structural
  evidence and MUST NOT recompute them from its own possibly lagging LFB.
- **R-PARENT-FLOOR.** A protocol-6 non-genesis block MUST declare at least one parent.
  At least one declared parent MUST descend from the block's signed finalized floor.
  Each effective parent floor MUST precede or equal that floor and preserve its admitted state.
  All effective parent floors MUST form one comparable chain.
  A verified certificate cache MUST NOT bypass this candidate-specific check.
  The receiver MUST NOT require equality with its local preferred parent frontier.
  Frozen justifications remain authority inputs even when a replay-safe parent subset omits one justified sibling.
- **R-PARENT-STATE.** Parent selection preserves causality; floor-rebased replay
  preserves state. The produced pre-state MUST include every effect active at the
  selected certified floor. Rejections MAY remove only exact above-floor effect
  identities considered by that merge and MUST NOT remove an effect already
  represented by the floor state.
- **R-REBASE.** If a causally certified speculative block fails R-STATE-CERT or
  R-LFB-STATE, a later child MUST recompute from the certified floor rather than
  reuse the stale covering-parent post-state. The rebase restores the floor's
  accepted effects and eventual LFB progress even when parent selection places
  the old LFB in a secondary-parent branch.
- **R-VALIDITY-STABILITY.** Learning that another block finalized MUST NOT
  retroactively make an otherwise valid speculative block invalid. Consensus
  validity is block-structural; LFB eligibility is evaluated separately when the
  finalizer considers promotion.
- **R-PROVENANCE-ACTIVATION.** Exact state-effect provenance begins with the
  protocol-3 consensus encoding, vault-backed quantitative byte evidence with
  protocol 4, certified validator incarnations with protocol 5, and certified
  finalized-floor commitments and admission outcomes with protocol 6. This
  release supports only protocol 6 and MUST start from a protocol-6 genesis or
  resynchronize complete current metadata; it MUST NOT infer missing consensus
  fields for legacy persisted blocks.

## 3. Determinism (normative)

- **R-DET.** Every honest node MUST derive the **identical** `floor(B)` for the same
  `B`. Since both floor sources are block-structural facts and `just(B)` is frozen,
  the result is node-identical (see S1).
- **R-CACHE.** The persisted frontier cache is an optimization only: the warm
  incremental up-walk MUST yield the **identical** frontier as the cold down-walk.
  When a determinism premise fails (committee change in band, or the pivot no longer
  finalizes over the larger snapshot), the warm path MUST fall back to the cold walk.
- **R-POST-STATE-BONDS.** `B.body.state.bonds` MUST equal the PoS bonds replayed
  from `B.post_state`. It is a consensus-visible post-state cache, not an
  authority declaration. Only an accepted block MAY use this cache to register
  a newly bonded validator's latest-message slot; an invalid block MUST NOT
  create a validator slot from untrusted serialized bonds.
- **R-AUTHORITY.** The authority committee for `B` MUST be the positive active
  bonds of `post_state(floor(B))`, a pure function of immutable block evidence.
  `B`'s justification validators MUST equal that committee exactly, `B.sender`
  MUST be a member with positive stake, and synchrony weights MUST come from the
  same committee. A bond transition in `B.post_state` MUST NOT authorize `B`
  itself. After the accepted block registers the validator and a later floor
  includes the transition, the new committee MAY authorize a later block.
- **R-PROPOSAL-AUTHORITY.** Immediately before replay, a proposer MUST derive
  the prospective structural floor from its selected parents and frozen
  justifications. It MUST defer when that committee differs from the captured
  LFB committee, when justifications are not exact, or when the sender is not a
  positive member. This prevents a locally selected block from being
  deterministically rejected by another honest validator at the same evidence
  boundary.
- **R-ADMISSION-CLOSURE.** A non-genesis block MUST remain buffered until DAG
  metadata is available for its parents, justifications, every historical
  unary-slash evidence hash, both hashes of every objective-equivocation proof,
  and both hashes of every header-certified evidence pair needed by its causal
  closure. Neither a mutable receiver-local equivocation tracker nor the
  derived invalid-block index may satisfy any dependency. Rejected blocks still
  satisfy readiness through their persisted certified DAG metadata.
- **R-CERTIFICATE-DEPENDENCY.** A protocol-6 block whose signed finalized-floor
  commitment names an unavailable certificate MUST be persisted as a detached
  block and buffered on a typed certificate dependency. Certificate dependency
  keys MUST be disjoint from every 32-byte block hash and MUST round-trip to one
  exact 32-byte certificate digest.
- **R-CERTIFICATE-REQUEST.** Certificate requests MUST be content-addressed,
  deduplicated per digest, bounded in count and peer fanout, and retried with a
  monotonic bounded backoff. A transport failure MUST retain the live proof
  obligation. Failure for one digest MUST NOT prevent eligible retries for other
  digests in the same maintenance pass.
- **R-CERTIFICATE-RESPONSE.** A certificate response MAY mutate storage only
  when its digest is a live persistent obligation, its encoding and shape are
  bounded, and its canonical certificate digest equals the requested digest.
  Unsolicited, malformed, oversized, or mismatched responses MUST NOT resolve a
  dependency or complete a tracker entry.
- **R-CERTIFICATE-WAKE.** Content-addressed certificate persistence MUST precede
  dependency resolution. Concurrent duplicate responses MUST converge to one
  sidecar, one resolution, and at most one queue insertion for each block.
  Temporary queue backpressure MUST leave the block discoverable by a later
  dependency-free scan.
- **R-CERTIFICATE-RESTART.** Detached blocks, certificate sidecars, and typed
  dependency edges MUST be durable. After restart, the node MUST reconstruct
  bounded volatile requests from the persistent dependency set, resolve already
  stored sidecars, and retry every remaining eligible obligation. Volatile
  cooldown or tracker loss MUST NOT change admission safety.
- **R-CARRIER-EQUIVALENCE.** A non-genesis predecessor certificate carrier MUST
  be an accepted, causal, protocol-compatible member of the bounded certified
  support closure whose signed commitment names the exact predecessor floor and
  replayed post-state. Eligibility MUST NOT require equality with the receiver's
  node-local witness digest, ledger revision, or record digest. Honest proofs
  with different evidence identities but the same certified state are
  semantically interchangeable only as complete proofs.
- **R-CARRIER-PAIR.** Selection MUST return the carrier block hash and the digest
  committed by that block as one pair. The next local witness and durable append
  MUST preserve that pair exactly. Storage and receiver validation MUST reject a
  carrier hash combined with a digest from another carrier, even when both
  carriers separately certify the same floor and post-state.
- **R-CARRIER-WAKE.** A parked predecessor-carrier obligation MUST wake after
  admission of an accepted commitment for the current local head's exact floor
  and post-state. A different local witness digest MUST NOT suppress the wake. A
  floor match with a different post-state MUST NOT wake or satisfy the
  obligation; final selection MUST rerun from a fresh bounded support closure.
- **R-EVIDENCE-TRAVERSAL.** Certified causal-evidence discovery MUST traverse
  the structural parents and justifications of both accepted and rejected
  blocks. Rejection is not an ancestry barrier. An accepted block's certified
  evidence delta is inherited; a rejected block's delta is not. The two block
  hashes named by a proof are terminal evidence facts and MUST NOT recursively
  import the contexts of those blocks.
- **R-EVIDENCE-CANONICAL.** For each validator bond generation, the effective
  evidence context MUST retain the least canonical proof under the stable
  evidence ordering. The join is commutative, associative, and idempotent.
  Evidence discovery and the required candidate delta therefore depend only on
  the complete causal closure, not arrival order, traversal order, or ambient
  tracker contents.
- **R-ADMISSION-OUTCOME.** Every persisted non-genesis metadata record MUST
  carry one typed admission outcome bound to the block hash, protocol version,
  admission-schema version, compiled ruleset digest, certified incoming-context
  digest, and certified sender-authority digest. Storage MUST reject a decision
  whose disposition conflicts with the requested accepted/rejected insertion
  mode, and an existing decision MUST be byte-identical on retry. A rejected
  outcome MUST NOT be finalized.

The normative context construction is:

```text
certify_admission(candidate):
    require complete_metadata_dependency_closure(candidate)
    pending := parents(candidate) union justifications(candidate)
    inherited := empty generation-keyed map
    structural := empty (generation, sequence)-keyed sibling map

    while pending is not empty:
        block := remove_any(pending)
        if block was already visited:
            continue
        require admitted DAG metadata for block
        pending := pending union parents(block) union justifications(block)
        add block identity to structural
        if outcome(block) is accepted:
            join each sound certified evidence delta into inherited

    effective := inherited joined with every sound structural sibling proof
    required_delta := effective minus inherited
    require delta(candidate) equals required_delta
    persist outcome(candidate, digest(effective), certified_sender_authority)
```

## 4. Merge base, scope, and the Δ-backstop (normative)

- **R-BASE.** The merge base MUST be `floor(B).post_state`.
- **R-SCOPE.** The merge scope MUST be `closure(parents) \ closure(floor)`; the
  floor-bounded ancestor scan MUST cover **every** parent write with block number
  `≥ num(floor)` and MUST NOT cut above the floor.
- **R-BACKSTOP.** When the floor distance `Δ = num(maxParent) − num(floor)` exceeds
  the cap, the merge MUST fail with a deterministic error keyed on `Δ` alone (on
  propose it parks the round; on validate the block is invalid). It MUST NOT
  substitute a lossy single-parent post-state. Any non-node-deterministic quantity
  (e.g. branch-width scope size) MUST NOT gate admission (demote to a metric).

## 5. Merge write-algebra (normative)

- **R-BITMASK.** BitmaskOr channels MUST combine by bitwise OR — a semilattice
  (idempotent, commutative, associative); no set bit may be lost, and the fold MUST
  be order-independent.
- **R-INTADD-COMBINE.** IntegerAdd diffs MUST be combined with checked addition;
  an overflow MUST reject the branch (never wrap-then-launder).
- **R-INTADD-APPLY.** The terminal apply `base + Σdiffs` (the consensus-state write)
  MUST use checked addition with a `≥ 0` guard, returning an error on overflow OR a
  negative balance.
- **R-INTADD-DIFF.** The per-deploy diff `end − prev` MUST use wrapping subtraction —
  the group inverse of the wrapping add that language-level execution used — so it
  recovers the deploy's true intended delta. Overflow MUST be caught at combine/apply
  (R-INTADD-COMBINE/APPLY), NOT at the diff (which is on the live execution path and
  must never crash on a deploy that is instead gracefully merge-rejected).

### 5.1 Exact occurrence, effect, and activation projection

The **active protocol version** is the shard-wide version used to construct and
validate the candidate block. The **floor protocol version** is historical
metadata on the finalized block whose post-state is already materialized as the
merge base. An **exact disposition record** identifies one source occurrence,
its winning or rejected reason, and the causal provenance that authorizes the
record.

Exact rejected-deploy dispositions begin at protocol 2; exact per-execution
state-effect provenance begins at protocol 3. Protocol 3 is the only supported
active protocol in this binary. Protocol-1 and protocol-2 structures remain
meaningful as historical encoding metadata in the merge algebra, but neither
historical approved genesis is a runnable shard for this binary. Cross-version
floor composition below is therefore a defensive state-validation property, not
authorization for an in-place protocol upgrade.

- **R-ACTIVE-BASE.** At and after exact-occurrence activation, finalized-base
  receipt precedence MUST be selected by the active protocol version. It MUST
  NOT be disabled because the historical floor block predates activation.
- **R-SCOPE-VERSION.** Every above-floor block admitted to one exact merge scope
  MUST carry the active protocol version. A mixed-version scope MUST fail closed.
- **R-DISPOSITION-ENCODING.** A current exact record MUST carry causal
  provenance and a non-`Unspecified` reason. A legacy record MUST carry neither
  provenance nor a specified reason. Validation MUST use the containing block's
  header version, never inference from record contents.
- **R-STATE-EFFECT-ENCODING.** Before protocol 3, the rejected-state-effect field
  MUST be empty. At protocol 3, it MUST equal the validator's recomputed list and
  be strictly increasing under the canonical `(source_block_hash,
  execution_index)` order, which excludes duplicates.
- **R-REASON-CONFLUENCE.** Multiple causal descendants MAY record different
  valid rejection causes for one exact source occurrence. Reducers MUST combine
  those causes with the canonical precedence join
  `$`r_{\mathrm{duplicate}} \succ r_{\mathrm{merge}} \succ
  r_{\mathrm{collateral}} \succ r_{\bot}`$`, where `$`r_{\bot}`$` is
  `Unspecified`. They MUST NOT reject an otherwise valid merge scope merely
  because concurrent records carry different causes, and MUST NOT use arrival
  or parent iteration order to choose the serialized reason.
- **R-BASE-DOMINANCE.** A winning receipt already materialized in the finalized
  base MUST remain committed even if an above-floor sibling carries a tombstone
  for the same signature. That tombstone is scope-local and MUST NOT authorize a
  retry of the finalized effect.
- **R-CHAIN-ATOMIC.** If an exact tombstone names one member of a dependent
  deploy chain, or one member duplicates a signature already committed in the
  base, rejection MUST reach the complete transitive causal dependency closure.
  Here a **physical effect dependency** means that a target exact effect removes
  the byte-identical ordinary datum or continuation produced by a source exact
  effect on the same channel key. A mergeable number-channel materialization is
  not a physical dependency because its typed contribution is folded
  algebraically. No ordinary state change or mergeable contribution causally
  dependent on a rejected effect may survive.
- **R-EXACT-SURVIVAL.** Rejection of an exact effect MUST NOT spread merely
  because another effect's source block is a DAG descendant of the rejected
  source block. Every exact effect outside the transitive physical dependency
  closure MUST remain eligible for conflict resolution. Whole-block descendant
  expansion MAY be used only for a legacy block that lacks exact per-execution
  state witnesses.
- **R-REJECTION-FIXPOINT.** Exact-effect rejection MUST compute the least fixed
  point generated by direct rejection seeds and physical dependency edges. A
  one-hop scan is insufficient: if `$`e_2`$` depends on `$`e_1`$` and `$`e_1`$`
  depends on rejected `$`e_0`$`, both `$`e_1`$` and `$`e_2`$` MUST be rejected,
  independently of iteration or arrival order.
- **R-EFFECT-COHERENCE.** Every exact chain's per-effect witnesses MUST fold to
  the chain's aggregate ordinary state change and aggregate mergeable
  contributions. One causal effect identity MUST resolve to one byte-identical
  normalized effect. Missing, duplicated, or inconsistent identities MUST fail
  closed.

### 5.2 Protocol-version lifecycle

- **R-GENESIS-VERSION.** The ceremony master MUST construct the genesis
  candidate with the configured current protocol version. Genesis approvers MUST
  compare that header version with their configured current version before
  signing.
- **R-APPROVED-SUPPORT.** Approved-block validation MUST reject every version
  outside the binary's explicit supported set before Casper starts. This release's
  supported set is exactly protocol 6; protocols 1 through 5 and unknown future
  versions MUST fail closed without mutating the running shard configuration.
- **R-VERSION-ADOPTION.** After approved-block validation, every node MUST adopt
  that approved version into the authoritative running shard configuration.
  Recovery and initialization contexts MUST retain the adopted configuration,
  not a stale local copy.
- **R-VERSION-PROPOSAL.** Every proposal MUST carry the authoritative running
  version. No compile-time default or independent local setting may bypass the
  adopted version.
- **R-VERSION-RECEPTION.** Peer-interest filtering and block validation MUST
  compare incoming blocks with the authoritative running version. They MUST NOT
  consult a second version source whose value can drift from proposal.
- **R-FRESH-GENESIS.** Protocol 6 MUST activate through a fresh protocol-6
  genesis approved by every validator. There MUST NOT be a node-local accounting
  switch, an A/B mode, or a block-height window in which this binary accepts both
  externalized and internalized charging, validator-incarnation, or certified-
  floor semantics.

### 5.3 Approved-state replay and local validation recovery

- **R-BLOCK-BOUND-REPLAY.** Historical reconstruction MUST derive the replay
  context from the block being reconstructed: its declared pre-state, block
  data, genesis supply payload, successful slash evidence, and block protocol
  version. The joiner's current approved tip and local startup configuration
  MUST NOT replace any of those inputs.
- **R-ROOT-AGREEMENT.** Replaying a valid historical block MUST reconstruct its
  declared post-state root. A mismatch is a local reconstruction failure until
  the node has proved a byte-level consensus defect; it MUST NOT immediately
  create objective invalidity or slash evidence.
- **R-LOCAL-FAULT-DEFER.** A block whose validation is inconclusive because of a
  local storage, root, availability, or busy failure MUST leave the
  dependency-free ready queue before recovery is requested. A failed request
  MUST NOT restore it to that queue. The buffer MUST retain custody until a
  terminal validation outcome or exact artifact recovery; merely observing the
  block a second time MUST NOT strand request tracking.
- **R-RECOVERY-ARTIFACT-IDENTITY.** An inconclusive validation MUST preserve the
  exact missing artifact through certification and recovery. A missing block is
  named by its block hash; a missing replay state is named by its state root.
  Neither may collapse to a generic missing-dependency status or to the other
  artifact type. A recovered artifact MUST release only its own waiters.
- **R-RECOVERY-HISTORY-GUARD.** On a genesis-rooted node, missing history MUST
  remain a typed local fault because genesis provides no legitimate earlier
  dependency boundary. On a restored node with intentionally truncated
  history, the same typed absence MAY be classified as a missing dependency.
  Both classifications MUST preserve artifact identity and MUST NOT create
  objective invalidity or slash evidence.
- **R-RECOVERY-DEDUPLICATION.** Concurrent blocks on one node that await the
  same artifact MUST share one idempotent request lifecycle. Requests for
  distinct artifacts and requests issued by distinct validators MUST commute;
  recovery MUST NOT introduce a global validation lock or serialize validators.
- **R-DEPENDENT-GATING.** Removing a locally faulted parent from the ready queue
  MUST NOT release an ordinary child. Dependency resolution MUST re-evaluate
  the child's serialized parent set against consensus-terminal state; only a
  validated parent satisfies an ordinary parent edge.

### 5.4 Terminal funding admission

- **R-FUNDING-PRESTATE.** Proposal and validation MUST classify state-bound
  funding from the same authenticated block pre-state and canonical candidate
  sequence. A validator MUST NOT substitute a later live supply view.
- **R-FUNDING-TERMINAL.** An attempted deploy rejected by the state-bound
  funding gate MUST receive a consensus-visible terminal rejection record. It
  MUST NOT remain locally pending for an unrelated later top-up to reclassify.
- **R-FUNDING-NO-EFFECT.** A terminal funding rejection MUST retain the complete
  signed envelope needed for independent revalidation, while committing zero
  user cost, zero user event log, and no user-state transition.
- **R-FUNDING-AUTHENTICITY.** Validation MUST reconstruct the complete admitted
  and rejected partition from the recorded pre-state. A proposer MUST NOT mark
  a fundable deploy rejected or execute an underfunded deploy.
- **R-ADMISSION-EFFECT-PROJECTION.** Merge metadata and adjacent state witnesses
  MUST align with exactly the user records that entered runtime execution plus
  every processed system execution. A terminal admission rejection MUST consume
  no execution slot. An ordinary deploy that entered runtime and failed MUST
  retain its slot.
- **R-ADMISSION-EFFECT-FAIL-CLOSED.** The exact effect projection MUST drive
  cardinality, user/system splitting, and execution indices. Missing or extra
  effect metadata MUST remain an error; status-only records MUST NOT fabricate
  empty effects or prevent indexing an otherwise valid parent.

## 6. Safety invariants — MUST NEVER happen

| ID | Must never |
|---|---|
| **S1** | Two honest nodes disagree on `floor(B)` (violates R-DET/R-CACHE). |
| **S2** | The floor regresses along ancestry (violates R-FLOOR monotonicity). |
| **S3** | A block finalizes below `θ` or against a shrunk denominator. |
| **S4** | An unsound merge base is used instead of erroring (violates R-SOUND/R-ERR). |
| **S5** | A single-value cell keeps two writes, or a mergeable write is lost/dropped — *the ~400-block bug* (violates R-SCOPE/R-BACKSTOP). |
| **S6** | Non-deterministic merge output → fork (violates R-BITMASK/R-INTADD). |
| **S7** | A negative vault balance or a laundered overflow is committed (violates R-INTADD-APPLY). |
| **S8** | Proposal, sender, justification, or synchrony authority is taken from a non-floor committee; or serialized post-state bonds are treated as same-block authority (violates R-POST-STATE-BONDS/R-AUTHORITY/R-PROPOSAL-AUTHORITY). |
| **S9** | Candidate coverage or selection depends on a node's wall clock, configured timeout, local error, or fixed prefix (violates R-FINALIZER-CLOSURE/ERROR/YIELD). |
| **S10** | A legacy floor version disables current finalized receipts and allows the same signature's effect to materialize twice (violates R-ACTIVE-BASE/R-BASE-DOMINANCE). |
| **S11** | A mixed-version above-floor scope or protocol-incompatible disposition encoding is accepted (violates R-SCOPE-VERSION/R-DISPOSITION-ENCODING). |
| **S12** | Rejection removes only one member of a dependent chain while another member's ordinary or mergeable effect survives (violates R-CHAIN-ATOMIC). |
| **S13** | The exact effect map and aggregate chain state describe different transitions (violates R-EFFECT-COHERENCE). |
| **S14** | Genesis approvers sign a candidate whose version differs from their configured current protocol (violates R-GENESIS-VERSION). |
| **S15** | A protocol-1 or unknown approved block reaches Running under this binary (violates R-APPROVED-SUPPORT/R-FRESH-GENESIS). |
| **S16** | A proposer and receiver derive their expected block versions from different authorities and reject one another's honest blocks (violates R-VERSION-ADOPTION/R-VERSION-PROPOSAL/R-VERSION-RECEPTION). |
| **S17** | A late joiner replays a historical block with current-tip or node-local context and reconstructs a different root (violates R-BLOCK-BOUND-REPLAY/R-ROOT-AGREEMENT). |
| **S18** | A local replay or storage fault is recorded as objective invalidity or slash evidence (violates R-ROOT-AGREEMENT/R-LOCAL-FAULT-DEFER). |
| **S19** | A locally faulted block remains dependency-free and immediately re-enqueues itself, including after a failed recovery request (violates R-LOCAL-FAULT-DEFER). |
| **S20** | An ordinary child is validated before its locally faulted parent reaches a valid terminal state (violates R-DEPENDENT-GATING). |
| **S21** | Proposal and validation classify the same deploy from different supply states (violates R-FUNDING-PRESTATE). |
| **S22** | An attempted underfunded deploy remains pending, produces user effects, or is later resurrected without a new deploy occurrence (violates R-FUNDING-TERMINAL/R-FUNDING-NO-EFFECT). |
| **S23** | A proposer forges either side of the state-bound admitted/rejected partition (violates R-FUNDING-AUTHENTICITY). |
| **S24** | A causally certified block advances the floor or LFB without a state-preserving certificate, or while bypassing a state already committed by the current LFB (violates R-STATE-CERT/R-STATE-FRONTIER/R-LFB-STATE). |
| **S25** | A covering-parent fast path reuses a stale post-state after the block's floor contains an active effect that parent rejected (violates R-STATE-PRESERVATION/R-REBASE). |
| **S26** | Rejecting one exact effect removes an independent effect solely because its source block descends from the rejected effect's block (violates R-EXACT-SURVIVAL). |
| **S27** | Rejection stops after one dependency hop and retains an effect that transitively consumes rejected state (violates R-CHAIN-ATOMIC/R-REJECTION-FIXPOINT). |
| **S28** | A merge, floor, or LFB drops an accepted effect because state provenance collapses to one parent, depends on parent order, aliases two source identities, accepts non-canonical rejection evidence, or omits a rejection-candidate check (violates R-EFFECT-ID/R-EFFECT-WIRE/R-EFFECT-ACTIVE/R-STATE-PRESERVATION/R-EFFECT-SCAN). |
| **S29** | An honest proposer drops a valid causal latest message, falls back to genesis when the valid-tip set is empty, or replays parent deltas without preserving its certified floor state (violates R-PARENT-CAUSALITY/R-PARENT-STATE/R-PARENT-EVIDENCE). |
| **S30** | A dual-certified state remains absent from every block replay floor solely because it is secondary to every parent and main-spine discovery cannot see it (violates R-UNIVERSAL-FRONTIER). |
| **S31** | Optimized latest-message coverage changes a validator support set, weight map, clique verdict, or permits a multi-parent unchanged-snapshot scan reuse (violates R-COVERAGE-EQUIVALENCE/R-LINEAR-SNAPSHOT-REUSE). |
| **S32** | Snapshot selection inspects an off-parent latest message before its recursive floor provenance is materialized, or concurrent cache writes lose a required entry, causing node-local classification, repeated processing, or proposal failure (violates R-STATE-DEPENDENCIES/R-SNAPSHOT-PROVENANCE-CLOSURE). |
| **S33** | A terminal admission rejection is counted as a runtime effect, or an ordinary runtime failure is removed from the effect sequence, shifting metadata and preventing valid-parent indexing (violates R-ADMISSION-EFFECT-PROJECTION/R-ADMISSION-EFFECT-FAIL-CLOSED). |
| **S34** | Heartbeat recovery treats producer timestamps or frontier churn as finality progress, reapplies the one-time stall timeout between later rounds, permits every validator to emit each round, requires a shared global round, fixes recovery to an offline leader, uses a non-canonical committee, admits an unbounded validation backlog, promotes without mutual causal and state cliques, assumes within-round delivery for safety, shrinks the finality denominator to a local subgraph, or treats indirect ancestor closure as an independently voted sub-finalization (violates R-FINALIZER-LOCAL-VIEW/R-FINALIZATION-CLOSURE/R-HEARTBEAT-*). |
| **S35** | A validator captures or publishes a replay root through a shared mutable current-root pointer, so another runtime's reset changes its decision (violates R-LOCAL-ROOT-AUTHORITY). |
| **S36** | Support is emitted before exact local replay, received support substitutes for local validation, or a delivered message lacks an emitted signer/candidate source (violates R-LOCAL-SUPPORT). |
| **S37** | Concurrent promotion exposes a torn block/root/effect tuple, mutates another validator's state, fails to commute for distinct validators, or publishes different state for the same candidate (violates R-ATOMIC-FLOOR-PUBLICATION/R-PARALLEL-FRAME). |
| **S38** | Crash or restart retains a partial validation, publishes support, loses a required recorded root, or combines a newly observed floor block with an older root/effect snapshot (violates R-VALIDATION-RESTART/R-VALIDATOR-LOCAL-TRANSACTION). |
| **S39** | Peer activity or an ambient flag authorizes an empty block, a stale recovery permit executes, stored but inadmissible work masks recovery, selected recovery suppresses admissible deploys, or a pending collision loses its one required follow-up (violates R-PROPOSAL-INTENT/R-RECOVERY-PERMIT/R-HEARTBEAT-WORK/R-PENDING-ADMISSIBILITY/R-PENDING-RECOVERY-COMPOSITION/R-PROPOSER-COALESCING). |
| **S40** | Recovery treats a peer's local ledger revision, digest, witness, or advertised head as state authority; bypasses ordinary dependency admission; publishes outside the local frozen-context Casper finalizer; loses a finalization retry after concurrent admission; replaces genesis; or globally pauses validators or shards (violates R-RECOVERY-*). |
| **S41** | Per-block floor derivation can defer on a dual-certified secondary-parent target that the durable finalizer cannot discover, or materialization substitutes evidence for a different target (violates R-FINALIZER-CLOSURE/R-FINALIZER-ORDER/R-FINALIZATION-BASE). |
| **S42** | An honest node permanently parks because an admitted predecessor carrier commits a different honest local witness digest for the same floor/state; accepts the same floor with a different state; or splices a carrier block and foreign digest (violates R-CARRIER-EQUIVALENCE/R-CARRIER-PAIR/R-CARRIER-WAKE). |
| **S43** | Certified validation erases a missing block hash or replay-state root, requests the wrong artifact type, drops the inconclusive block, leaks ready-path request ownership, releases a child from the wrong recovery, duplicates same-artifact requests, or globally serializes independent validators (violates R-LOCAL-FAULT-DEFER/R-RECOVERY-ARTIFACT-IDENTITY/R-RECOVERY-HISTORY-GUARD/R-RECOVERY-DEDUPLICATION). |
| **S44** | A candidate's declared parents omit all ancestry of its signed finalized floor, or receiver-local fork choice rejects an otherwise replay-safe parent subset (violates R-PARENT-FLOOR). |
| **S45** | Restore-horizon handling deletes an exact slot, removes its stake, changes a certified context from local heldness, drops a live cost effect, or treats an arbitrary missing dependency as an abstention (violates R-FINALIZER-RESTORE-HORIZON). |

## 7. Liveness invariants — MUST eventually happen

| ID | Must eventually |
|---|---|
| **L1** | A common finalized cut ⟹ `derive_floor` returns it. |
| **L2** | Keep-one losers are re-proposed (recovery). |
| **L3** | The floor advances, `Δ` stays bounded, walks/scope stay bounded — *the ratchet driver is neutralized*. |
| **L4** | Non-conflicting writers converge. |
| **L5** | Multi-parent finality does not wedge. |
| **L6** | For a stable finite frozen snapshot with readable metadata, if an exact-current-LFB descendant satisfies the exact threshold, a finalizer invocation selects the highest such candidate. |
| **L7** | Under eventual peer availability and a transient local fault, deferred recovery reopens the parent, validates it, and then releases and validates its descendants. |
| **L8** | Every recorded funding decision eventually finalizes as either executed or rejected. |
| **L9** | After a stale-state certified candidate is skipped, eventual delivery and proposal produce a floor-rebased descendant that all honest validators can promote. |
| **L10** | Complete frozen evidence eventually promotes the highest dual-certified universal floor, and the finite descending coverage traversal completes without a node-local timeout or candidate cap. |
| **L11** | Recording a terminal admission rejection cannot prevent every honest validator from indexing that parent, proposing successors, and finalizing later deploys. |
| **L12** | Every finite frozen snapshot completes its required provenance closure and reaches parent selection despite arbitrary interleaving with idempotent finalizer cache writes. |
| **L13** | If an LFB remains unchanged, at least one threshold-supporting validator remains online, validation is fair, delivery is eventually within a recovery round, online heartbeat tasks are eventually scheduled within one completed recovery step of each other, and replay completes in finite time, the one-time stall timeout followed by ordered `check_interval` rotation eventually admits support and advances finality while the proposal/validation backlog remains bounded. |
| **L14** | Under fair local execution and support delivery, independently scheduled honest validators that accept the same state-preserving candidate eventually publish its identical block/root/effect tuple regardless of unrelated runtime resets. |
| **L15** | Under fair proposer execution, a finite burst of pending-deploy requests colliding with one active proposal produces exactly one forced fresh-snapshot follow-up, while a busy or deferred selected recovery leaves its local round available for retry. |
| **L16** | If a stable frozen view contains an all-parent-reachable candidate above the current LFB that passes its own exact causal certificate, exact state certificate, and current-LFB preservation check, the finalizer eventually materializes the deterministic greatest eligible `(block_number, block_hash)` candidate. |
| **L17** | Once an accepted causal carrier for the current predecessor floor/state enters the bounded support closure, every parked honest finalizer is woken and may extend through that complete proof regardless of its own local witness digest. |

## 8. Conformance

An implementation conforms iff every **R-** requirement holds and no **S-** invariant
is reachable, with **L-** invariants holding under the partial-synchrony progress
assumption. The [verification dossier](./finalized-floor-verification.md) maps each
requirement/invariant to its mechanized artifact (Rocq axiom-free capstones, TLA⁺
models, Z3/Sage cross-witnesses) and Rust regression tests; run them locally with
`scripts/check-finalized-floor-ALL.sh` (formal verification is **local-only** — never
wired into CI). Exact occurrence, recovery, and activation checks additionally
run through `scripts/check-deploy-lifecycle-ALL.sh`.
