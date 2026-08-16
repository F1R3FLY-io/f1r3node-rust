# Finalized-Floor Multi-Parent Merge — Normative Specification

This is the **normative** contract for the finalized-floor multi-parent merge: *what
must hold*, independent of *how it is checked*. The companion
[verification dossier](./finalized-floor-verification.md) records the mechanized
proofs, models, and tests that discharge each requirement; the
[glossary](./finalized-floor-glossary.md) defines every symbol and term used below
(read it first if any notation is unfamiliar). Rendered diagrams are in
[`diagrams/`](./diagrams/).

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
`casper/src/rust/merging/deploy_chain_index.rs`.

The state-lineage rule is a node-consensus refinement, not a statement quoted
from either cost-accounting paper. The source-checkout papers
`../publications/cost-accounting/cost-accounted-rho.tex` and
`../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex` provide
the atomic resource-commitment and conservation obligations. The existing
Casper contract supplies the separate premise that finalized effects are
permanent. R-LFB-STATE is the implementation rule that composes those two
obligations: causal certification remains unchanged, while a separate
state-preserving certificate and current-LFB ancestry prevent installation of a
state that omits an already committed resource transition.

## 2. The floor rule (normative)

For a block `B` with non-empty parent set `P₁…Pₖ` and frozen justification snapshot
`just(B)`:

- **R-FLOOR.** `floor(B)` MUST be the **maximum state-preserving sound candidate**
  (by block number, tie-broken by hash) over the union of two sources, each a
  **pure function of `B`**:
  1. **Inheritance** — every parent's own floor.
  2. **Advancement** — per parent, the highest main-chain ancestor `A` with
     `ft_witnessed(A, just(B)) ≥ θ` (genesis is finalized by definition).
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
  main-parent closure of every frozen latest message above the exact current LFB
  height. Each `(validator, block)` pair MUST be admitted to the traversal
  frontier at most once, without removing any reachable pair.
  A candidate-count cap, elapsed-time budget, or per-candidate timeout MUST NOT
  truncate this consensus search.
- **R-FINALIZER-ORDER.** The complete candidate set MUST be ordered by block number
  descending, agreeing stake descending, agreeing-set size ascending, then hash
  ascending. For each candidate, the finalizer MUST first run the existing exact
  causal clique decision without modifying its voters, weights, threshold, or
  strictness. It MUST then run the same exact decision over state-preserving
  support as required by R-STATE-CERT. The first candidate that holds both
  certificates and preserves the current LFB's state lineage is the next LFB.
  The current LFB need not lie on the candidate's main-parent spine: a
  multi-parent rebase may preserve it through a secondary parent.
- **R-FINALIZER-ERROR.** Missing or unreadable consensus metadata and failed clique
  evaluation MUST make the invocation inconclusive and return an error. They MUST
  NOT be treated as a negative finality vote or skipped in favor of another block.
- **R-FINALIZER-YIELD.** Cooperative yields MAY bound executor occupancy. Yield
  frequency MUST affect latency only: it MUST NOT change the snapshot, candidate
  closure, candidate order, or selected LFB.

### 2.2 State derivation and LFB admissibility

A **causal clique certificate** answers whether sufficient mutually agreeing
stake causally supports a block. A **state-preserving clique certificate** answers
whether sufficient mutually agreeing stake has retained that block's state in
its frozen latest messages. **LFB admissibility** additionally answers whether
installing the candidate would retain the state already committed by the current
LFB. These are separate predicates. The state-support calculation MUST NOT alter
the causal certificate, and the current-LFB lineage check MUST NOT alter either
certificate.

- **R-STATE-BASE.** Genesis derives from itself. For every non-genesis block `B`,
  its direct state base MUST be the covering parent only when one parent covers
  every parent and `floor(B)` is in that parent's state lineage. Otherwise its
  direct state base MUST be `floor(B)`, because the block is computed by replaying
  the floor-bounded merge.
- **R-STATE-ANCESTRY.** State ancestry MUST be the reflexive, transitive closure
  of the direct state-base relation. It is derivation provenance, not tuple-set
  inclusion: an authorized later reduction may consume data while still deriving
  from its predecessor.
- **R-STATE-CERT.** For candidate `C` and frozen snapshot `just(B)`, state support
  MUST contain exactly the validators whose latest messages both causally include
  `C` and state-descend from `C`. The node MUST run the same hard-majority,
  maximum-clique, exact-threshold decision used for causal certification over
  that restricted support. Causal support through a merge-parent edge MUST NOT
  count as state support when the merge state rejected `C`'s effects.
- **R-STATE-DEPENDENCIES.** State-lineage materialization MUST close over both DAG
  parents and the frozen latest-message justifications consulted by R-STATE-CERT.
  A missing dependency or cyclic lineage MUST fail the derivation; it MUST NOT be
  interpreted as absent state support.
- **R-STATE-FRONTIER.** A raw causally certified main-parent frontier MUST first
  be reduced to the highest candidate on that spine that preserves the accepted
  state lineage, then lowered along its direct state-base lineage until it reaches
  a state-certified candidate. A causally certified stale-state descendant or
  rejected parent remains a valid speculative block but MUST NOT become a floor
  advancement.
- **R-FLOOR-STATE.** A candidate floor at or above an inherited parent floor MUST
  state-descend that inherited floor. A candidate that bypasses an inherited
  committed state MUST be skipped; an older common sound base MAY be selected.
- **R-LFB-STATE.** A candidate MAY become the next LFB only if its causal
  certificate, state-preserving certificate, and current-LFB state ancestry all
  hold over the same frozen snapshot. Main-parent ancestry MUST NOT be an
  additional admission condition: it describes vote propagation, not
  multi-parent state derivation.
- **R-REBASE.** If a causally certified speculative block fails R-STATE-CERT or
  R-LFB-STATE, a later child MUST recompute from the certified floor rather than
  reuse the stale covering-parent post-state. The rebase restores state lineage
  and eventual LFB progress even when parent selection places the old LFB in a
  secondary-parent branch.
- **R-VALIDITY-STABILITY.** Learning that another block finalized MUST NOT
  retroactively make an otherwise valid speculative block invalid. Consensus
  validity is block-structural; LFB eligibility is evaluated separately when the
  finalizer considers promotion.

## 3. Determinism (normative)

- **R-DET.** Every honest node MUST derive the **identical** `floor(B)` for the same
  `B`. Since both floor sources are block-structural facts and `just(B)` is frozen,
  the result is node-identical (see S1).
- **R-CACHE.** The persisted frontier cache is an optimization only: the warm
  incremental up-walk MUST yield the **identical** frontier as the cold down-walk.
  When a determinism premise fails (committee change in band, or the pivot no longer
  finalizes over the larger snapshot), the warm path MUST fall back to the cold walk.
- **R-COMM.** The committee used to validate `B`'s bonds MUST be `bonds_of(floor(B))`
  — a pure function of the floor.

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

For this D3 wire format, protocol 2 is the only supported active protocol.
Protocol-1 structures remain meaningful as historical encoding metadata in the
merge algebra, but a protocol-1 approved genesis is not a runnable shard for this
binary. Cross-version floor composition below is therefore a defensive
state-validation property, not authorization for an in-place protocol upgrade.

- **R-ACTIVE-BASE.** At and after exact-occurrence activation, finalized-base
  receipt precedence MUST be selected by the active protocol version. It MUST
  NOT be disabled because the historical floor block predates activation.
- **R-SCOPE-VERSION.** Every above-floor block admitted to one exact merge scope
  MUST carry the active protocol version. A mixed-version scope MUST fail closed.
- **R-DISPOSITION-ENCODING.** A current exact record MUST carry causal
  provenance and a non-`Unspecified` reason. A legacy record MUST carry neither
  provenance nor a specified reason. Validation MUST use the containing block's
  header version, never inference from record contents.
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
  supported set is exactly protocol 2; protocol 1 and unknown future versions
  MUST fail closed without mutating the running shard configuration.
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
- **R-FRESH-GENESIS.** Cost-accounted protocol 2 MUST activate through a fresh
  protocol-2 genesis approved by every validator. There MUST NOT be a node-local
  accounting switch, an A/B mode, or a block-height window in which this binary
  accepts both externalized and internalized charging semantics.

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
  MUST NOT restore it to that queue. At most one recovery lifecycle may be
  outstanding for the hash.
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
| **S8** | Bonds validated against a non-floor committee (violates R-COMM). |
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
| **S25** | A covering-parent fast path reuses a stale post-state after the block's floor advanced past that parent's state lineage (violates R-STATE-BASE/R-REBASE). |
| **S26** | Rejecting one exact effect removes an independent effect solely because its source block descends from the rejected effect's block (violates R-EXACT-SURVIVAL). |
| **S27** | Rejection stops after one dependency hop and retains an effect that transitively consumes rejected state (violates R-CHAIN-ATOMIC/R-REJECTION-FIXPOINT). |

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

## 8. Conformance

An implementation conforms iff every **R-** requirement holds and no **S-** invariant
is reachable, with **L-** invariants holding under the partial-synchrony progress
assumption. The [verification dossier](./finalized-floor-verification.md) maps each
requirement/invariant to its mechanized artifact (Rocq axiom-free capstones, TLA⁺
models, Z3/Sage cross-witnesses) and Rust regression tests; run them locally with
`scripts/check-finalized-floor-ALL.sh` (formal verification is **local-only** — never
wired into CI). Exact occurrence, recovery, and activation checks additionally
run through `scripts/check-deploy-lifecycle-ALL.sh`.
