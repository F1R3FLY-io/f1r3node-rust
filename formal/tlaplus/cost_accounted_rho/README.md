# TLA+ Model: Cost-Accounted Rho Calculus

Finite-state model checking of the cost-accounted rho calculus token
protocol and eval scheduling, complementing the Rocq mechanization at
`formal/rocq/cost_accounted_rho/`.

## Prerequisites

- Java 17+ (`java -version`)
- TLC through a local `tla2tools.jar`
- Optional Apalache (`apalache-mc version`) for SMT-backed bounded cross-checks

Common TLC jar locations:
- `/usr/share/java/tla2tools.jar`
- `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar`

## Specifications

| File | Purpose | States | Properties |
|---|---|---|---|
| `CostAccountedRho.tla` | Atomic token protocol | 79 (3 procs) | TokenConservation, CostDeterminism, FuelGateSafety |
| `CompoundProtocol.tla` | Full protocol: compound sigs, Splits, nested eval | 63 (4 procs) | TokenConservation, CostDeterminism, FuelGateSafety, SplitOrdering, InnerGateOrdering |
| `EvalScheduling.tla` | Eval loop scheduling comparison | 16 (3 bodies) | InternalizedCostDeterministic, AllEventuallyDone |
| `MC.tla` | Model instance for CostAccountedRho | — | — |
| `MCCompound.tla` | Model instance for CompoundProtocol | — | — |
| `MCEval.tla` | Model instance for EvalScheduling | — | — |
| `FullProtocol.tla` | Generalized protocol: shared channels, arbitrary nesting (depth 0/1/2), Join mediators | 12,960 (7 procs, 12 channels) | TokenConservation, CostDeterminism, FuelGateSafety, GateOrdering, SplitOrdering, NoNegativeTokens |
| `MCFull.tla` | Model instance for FullProtocol | — | — |
| `RuntimeBudgetReplay.tla` | Bounded runtime-budget canonical permit grants, replay trace, invalid-event rejection, post-OOP rejection, deploy reset, finalization-read model, and canonical digest-entry abstraction over Rust event descriptors and occurrence multiplicity | 72 distinct / 203 generated (6 events, including zero-weight, over-source-path, and over-primitive-descriptor invalid events) | NoOverspend, OopCommitsBoundary, ReplayTraceSubset, OopNotLogged, PermitsMatchSuccessfulTrace, NoUnpaidPhysicalWork, CanonicalPermitOrder, FinalizedTraceSequence, FinalizationPreservesActiveBudget, LoggedEventsHavePositiveWeight, LoggedEventsAreValidated, TraceWithinRetentionBound, ResetClearsActiveTraceAfterFinalization, PostOopRejectionsPreserveSingleBoundary, CanonicalDigestEventCountMatches, CanonicalDigestDomainSeparatesOop, CanonicalDigestStableAfterFinalization |
| `MCRuntimeBudgetReplay.tla` | Model instance for RuntimeBudgetReplay | — | — |
| `CostAccountingThreats.tla` | Replay tampering, activation downgrade, unauthorized settlement, evidence-recording, and slash-authorization threat model | 5,408 distinct / 401,025 generated | CostAccountedReplayAcceptsOnlyValidPayload, CostAccountedReplayRejectsMissingCommitment, SettlementNeverAddsRuntimeFuel, CostInvalidEvidenceHasViolation, RecoveredSlashRequiresCurrentEvidence, SlashAuthorizationUsesParentPreState, AmbientBondDoesNotAuthorizeWithoutParent, ParentPositiveAmbientZeroCanAuthorize, SlashNoopPreservesCostBoundary |
| `MCCostAccountingThreats.tla` | Model instance for CostAccountingThreats | — | — |
| `CostAccountingSearchFrontier.tla` | Witness classification and promotion discipline for generated cost-accounting findings | 34,167 distinct / 266,015 generated | NoSourceFixWithoutRustOrInvariantEvidence, ProjectionRiskHasRustGuard, FormalStrengtheningHasInvariantTarget, ConfirmedBugHasSourceTarget, SourceSemanticWitnessHasFacets, SourceGraphSlashingWitnessHasAuthorizationMetadata |
| `MCCostAccountingSearchFrontier.tla` | Model instance for CostAccountingSearchFrontier | — | — |
| `MergeableChannelAccounting.tla` | Typed mergeable-channel diff/merge behavior and cost-boundary isolation | 2,656 distinct / 8,992 generated | BitmaskDiffMergeRoundTrip, IntegerAddDiffMergeRoundTrip, BitmaskMergeDoesNotDropBits, NonNumericPayloadHasNoNumericDiff, MergeableAccountingPreservesUserCost, SlashSystemEffectPreservesCostBoundary |
| `MCMergeableChannelAccounting.tla` | Model instance for MergeableChannelAccounting | — | — |

### Phase 1 / 2 / 3 multi-signature + LL-rich algebra specs

| File | Purpose | Properties |
|---|---|---|
| `MultiSignerProtocol.tla` | Phase 1.7 PoS Map-in-MVar refinement: per-cosigner attribution, FIFO drain, atomic soft-checkpoint revert | MapDomainEqualsInFlightSigners, RefundFinalizes, NoRefundCrossAttribution, PartialFailureNoConsumption, NoNegativeAmounts, ChargedAmountBounded, PhloShareConservation, FailureRevertsCharges, TotalRefundConservation; liveness: EventuallyDoneOrReverted, EventuallyAllRefundsComplete |
| `MCMultiSigner.tla` | Phase 4.6 — scaled-up harness (5 cosigners, phlo 8) | (same as base) |
| `ThresholdProtocol.tla` | Phase 2 M-of-N quorum semantics | QuorumThresholdConstraint, QuorumExactness, QuorumNoOverCount, AuthorizedSubsetPresented, PresentedSubsetMembers, RejectionImpliesShortQuorum; liveness: EventuallyTerminates |
| `MCThreshold.tla` | Phase 4.6 — 3-of-5 quorum harness (vs base 2-of-4) | (same) |
| `PlusProtocol.tla` | Phase 3 Sig::Plus additive disjunction (signer's chosen branch) | PlusBranchInRange, PlusBranchWitness, PlusNonChosenUntouched; properties: AdditiveChoiceDeterminism, PlusEventuallyAuthorizes |
| `MCPlus.tla` | Phase 4.6 — PhloPerBranch=8 harness | (same) |
| `WithProtocol.tla` | Phase 3 Sig::With additive conjunction (verifier's chosen branch) | AdditiveCoConservation, WithBothBranchesSigned, WithBranchAvailability, WithUnpickedUntouched; liveness: WithEventuallyPicked |
| `MCWith.tla` | Phase 4.6 — PhloPerBranch=8 harness | (same) |
| `BangProtocol.tla` | Phase 3 Sig::Bang exponential replication (bounded/unbounded) | BangReplicationSafety, BangUsageBound, BangBoundedNonNegative, BangPersistence, BangApprovedBoundedByLimit; liveness: BangBoundedEventuallyExhausts |
| `MCBang.tla` | Phase 4.6 — Bound=5, MaxInvocations=10 harness | (same) |
| `WhyNotProtocol.tla` | Phase 3 Sig::WhyNot exponential optionality | WhyNotOptional, WhyNotEmptyEquiv, WhyNotNoChargeWhenAbsent, WhyNotChargeBounded, WhyNotInvalidImpliesRejection; liveness: WhyNotEventuallyResolves |
| `MCWhyNot.tla` | Phase 4.6 — PhloAvailable=6 harness | (same) |
| `LollyProtocol.tla` | Phase 3 Sig::Lolly linear-implication capability | LollyResourceFlow, LollyNoCreationExNihilo, LollyTransformer, LollyCapabilityRegistered, LollyCapabilityNotRevoked; liveness: LollyEventuallyCompletes |
| `MCLolly.tla` | Phase 4.6 — MaxInvocations=6 harness | (same) |

### Connectives without a dedicated spec (subsumed)

Three of the nine LL connectives have no standalone protocol spec — by design, not omission:

| Connective | Where verified | How |
|---|---|---|
| **And / `⊗`** (tensor) | `MultiSignerProtocol.tla` + `CompoundProtocol.tla` / `FullProtocol.tla` | Cost-additivity *is* the per-cosigner pre-charge fan-out (`PhloShareConservation`, `TotalRefundConservation`); the structural `s₁ & s₂` decomposition is `SplitFires` + `TokenConservation`. |
| **Unit (`1`)** | `CostAccountedRho.tla` / `CompoundProtocol.tla` / `FullProtocol.tla` | Degenerate zero-token case (`TokensPerProc = 0` ⇒ 0 cost). The algebraic unit laws (`1 ⊗ σ ≡ σ`) are checked in Sage / Rocq, not TLA⁺. |
| **atom** | `CostAccountedRho.tla` | The atomic single-token signature is the base case ("atomic signatures"); one-atom ⇒ one-gate ⇒ one-COMM is the generic `FuelGateSafety`. |

So all nine connectives are accounted for: six dedicated specs (Plus, With, Bang, WhyNot, Lolly, Threshold) plus these three subsumptions.

## Running

```bash
cd formal/tlaplus/cost_accounted_rho
TLA2TOOLS="${TLA2TOOLS:-/usr/share/java/tla2tools.jar}"

# Atomic token protocol (3 processes, 3 channels, 3 tokens, all interleavings)
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MC.tla -config CostAccountedRho.cfg -workers auto -nowarning

# Full compound protocol (2 atomic + 1 compound + 1 spawned child,
# Split mediators, nested gates, recursive eval, all interleavings)
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCCompound.tla -config CompoundProtocol.cfg -workers auto -nowarning

# Eval scheduling comparison (3 bodies, all 3! orderings,
# internalized vs externalized cost models side by side)
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCEval.tla -config EvalScheduling.cfg -workers auto -nowarning

# Full generalized protocol (2 atomic sharing 1 channel + 1 compound depth 1 +
# 1 doubly-compound depth 2 + 2 join fuel sources + 1 join mediator,
# shared channels, arbitrary nesting, Join mediators, all interleavings)
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCFull.tla -config FullProtocol.cfg -workers auto -nowarning

# Bounded runtime-budget reservation/replay trace and finalization-read model
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCRuntimeBudgetReplay.tla -config RuntimeBudgetReplay.cfg -workers auto -nowarning

# Replay tampering, activation downgrade, unauthorized settlement, and
# cost-invalid evidence threat model
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCCostAccountingThreats.tla -config CostAccountingThreats.cfg -workers auto -nowarning

# Search-frontier witness classification and promotion discipline
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCCostAccountingSearchFrontier.tla -config CostAccountingSearchFrontier.cfg -workers auto -nowarning

# Typed mergeable-channel diff/merge and cost-boundary isolation
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MCMergeableChannelAccounting.tla -config MergeableChannelAccounting.cfg -workers auto -nowarning

# Phase 1.7 PoS Map-in-MVar refinement
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC MultiSignerProtocol.tla -config MultiSignerProtocol.cfg -workers auto -nowarning

# Phase 2 M-of-N threshold
java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
  tlc2.TLC ThresholdProtocol.tla -config ThresholdProtocol.cfg -workers auto -nowarning

# Phase 3 LL connectives (Plus, With, Bang, WhyNot, Lolly)
for proto in PlusProtocol WithProtocol BangProtocol WhyNotProtocol LollyProtocol; do
  java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
    tlc2.TLC "$proto.tla" -config "$proto.cfg" -workers auto -nowarning
done

# Phase 4.6 scaled-up MC harnesses
for mc in MCMultiSigner MCThreshold MCPlus MCWith MCBang MCWhyNot MCLolly; do
  java -XX:+UseParallelGC -cp "$TLA2TOOLS" \
    tlc2.TLC "$mc.tla" -config "$mc.cfg" -workers auto -nowarning
done
```

### Aggregate runner (local-only)

The companion script `scripts/check-cost-accounted-rho-tla-invariants.sh`
runs ALL of the above (currently 22 specs) sequentially through TLC.
Per the team's "formal verification is local-only, NOT in CI" policy
this script does NOT live under `scripts/ci/`. Invoke directly from
the repo root:

```bash
bash scripts/check-cost-accounted-rho-tla-invariants.sh
# Or filter:
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter MC
```

All eight should report: `Model checking completed. No error has been found.`

When Apalache is installed, the threat and search-frontier models can also
be checked symbolically:

```bash
cd formal/tlaplus/cost_accounted_rho
apalache-mc check --out-dir=/tmp/apalache-MCCostAccountingThreats \
  --config=CostAccountingThreats.cfg MCCostAccountingThreats.tla
apalache-mc check --out-dir=/tmp/apalache-CostAccountingSearchFrontier \
  --config=CostAccountingSearchFrontier.cfg CostAccountingSearchFrontier.tla
```

Both should report `The outcome is: NoError`.

## Verified Properties

### CostAccountedRho (atomic signatures)

- **TokenConservation**: Total tokens in system (pending + consumed) equals the initial total in every reachable state.
- **NoNegativeFuel**: No channel ever has negative pending tokens.
- **FuelGateSafety**: A process completes its inner COMM only if its fuel gate has fired.
- **CostDeterminism**: In every terminal state, `totalConsumed` equals the expected cost (one token per process that had fuel), regardless of which interleaving TLC explored.
- **AllComplete** (liveness): Every process with available fuel eventually completes.

### CompoundProtocol (compound signatures, Splits, recursive eval)

All properties from CostAccountedRho, plus:
- **TokenConservation** (extended): Accounts for Split redistribution (1 compound token becomes 2 atomic tokens). Invariant: `TotalPending + totalCost - SplitsFired = InitialTotal`.
- **SplitOrdering**: A compound process's outer gate fires only after its Split mediator has fired.
- **InnerGateOrdering**: A compound process's inner gate fires only after its outer gate.
- **CostDeterminism**: Terminal cost accounts for compound processes consuming 2 gates each and atomic processes consuming 1 gate each. The cost is identical across all scheduling orders.
- **AllSpawnedComplete** (liveness): All spawned processes (including recursively spawned children) with available fuel eventually complete.

### FullProtocol (shared channels, arbitrary nesting, Join mediators)

All properties from CompoundProtocol, generalized to arbitrary configurations:
- **TokenConservation** (generalized): Accounts for both Splits and Joins. Invariant: `TotalPending + totalCost - TotalSplitsFired + totalJoinsFired = InitialTotal`. Splits add +1 net token (1 in -> 2 out), Joins remove -1 net token (2 in -> 1 out).
- **Shared Channels**: Multiple processes can listen on the same signature channel. The injectivity assumption from CompoundProtocol is removed. When two processes compete for the same token, only one wins non-deterministically, but total cost remains deterministic.
- **Arbitrary Nesting (depth k)**: A depth-k process requires k cascading Splits and (k+1) gate layers. The model instance tests depth 0 (atomic), depth 1 (compound), and depth 2 (doubly-compound with 2 cascading Splits and 3 gates).
- **GateOrdering**: Gates fire in strict order (layer 1, then 2, ..., then k+1), and each gate's prerequisite Split must have fired.
- **SplitOrdering**: Splits fire in cascading order (level 1 before level 2, etc.), with each level's output feeding the next level's input.
- **Join Mediator**: The JoinFires action combines two atomic tokens into one compound token, the inverse of Split. The Join mediator's output feeds another process's gate channel.
- **CostDeterminism**: In terminal states, `totalCost` equals the expected cost regardless of interleaving order. With shared channels, the expected cost depends on the token supply configuration (specified as `ExpectedTerminalCost`).
- **AllComplete** (liveness): All processes with available fuel eventually complete.

### EvalScheduling (scheduling comparison)

- **InternalizedCostDeterministic**: At termination, `totalCost = |Bodies| * CostPerToken` regardless of execution order.
- **InternalizedCostBounded**: Cost never exceeds the theoretical maximum.
- **AllEventuallyDone** (liveness): All bodies eventually execute.

The `extCost` variable tracks what the externalized (buggy) cost model would produce — it is intentionally NOT checked as an invariant because it IS order-dependent (that's the bug this migration fixes).

### CostAccountingThreats (single-deploy replay/security boundary)

- **CostAccountedReplayAcceptsOnlyValidPayload**: in cost-accounted mode,
  accepted replay implies a present cost-trace commitment with matching
  digest and count.
- **CostAccountedReplayRejectsMissingCommitment**: absent trace
  commitments cannot be accepted after activation.
- **SettlementNeverAddsRuntimeFuel**: authorized and unauthorized
  settlement actions cannot increase runtime fuel.
- **CostInvalidEvidenceHasViolation**: evidence recording is enabled only
  for a modeled cost-invalid violation.
- **ReplayTamperCannotStayAccepted**: after digest/count/commitment
  tampering, cost-accounted replay is no longer accepted.
- **RecoveredSlashRequiresCurrentEvidence**: a recovered rejected slash
  is considered only when its evidence epoch and target activation epoch
  are both the current epoch.
- **SlashAuthorizationUsesParentPreState**: slash acceptance is
  authorized by the parent pre-state bond, not by an ambient post-state
  or execution-time bond view.
- **AmbientBondDoesNotAuthorizeWithoutParent**: an ambient bond alone
  cannot authorize a slash when the parent pre-state bond is zero.
- **ParentPositiveAmbientZeroCanAuthorize**: a positive parent pre-state
  bond authorizes current slash evidence even when the ambient bond view
  is zero.
- **SlashNoopPreservesCostBoundary**: a zero execution-bond slash is a
  no-op with respect to the user runtime cost boundary.

### RuntimeBudgetReplay (bounded runtime-budget replay)

- **CanonicalDigestEventCountMatches**: the abstract digest entry set has
  exactly the retained successful trace count plus the single OOP boundary,
  matching the Rust `cost_trace_event_count` contract. Duplicate events with
  the same deploy id, source path, redex id, local index, billable kind,
  primitive descriptor, and weight receive distinct occurrence ordinals.
- **PermitsMatchSuccessfulTrace** and **NoUnpaidPhysicalWork**: successful
  budget commits grant execution permits before modeled physical work
  executes, and OOP does not grant an execution permit for unfunded work.
- **CanonicalPermitOrder**: permits follow the modeled canonical rank, so
  the OOP boundary is not chosen by task completion order.
- **CanonicalDigestDomainSeparatesOop**: the OOP boundary is tagged
  separately from successful events, so boundary evidence cannot collapse
  into a successful reservation with the same event identity.
- **CanonicalDigestStableAfterFinalization**: finalization reads the same
  canonical digest entries that the active runtime budget retained; deploy
  reset may clear active trace state only after the finalization read.

### CostAccountingSearchFrontier (witness classification)

- **NoSourceFixWithoutRustOrInvariantEvidence**: generated witnesses cannot
  directly motivate implementation changes without production Rust reproduction or a
  production-invariant violation.
- **ClassifiedWitnessHasAction**: every terminal classification has a
  non-empty follow-up action.
- **GuardedProjectionDoesNotFixSource**: projection risks promote to guards
  and documentation, not immediate implementation changes.
- **FormalGapDoesNotDirectlyFixSource**: proof/model strengthening witnesses
  promote to formal artifacts before implementation changes.
- **ProjectionRiskHasRustGuard**: projection risks must point at a Rust guard
  target and carry concrete guard evidence.
- **FormalStrengtheningHasInvariantTarget**: proof/model strengthening
  witnesses must carry an expected invariant and promote to Rocq, TLA+, or
  Sage before any implementation action.
- **ConfirmedBugHasSourceTarget**: confirmed current bugs must target a source
  fix and must be backed by Rust reproduction or production-invariant evidence.
- **ClassifiedWitnessHasPromotionTarget**: every terminal classification
  carries a non-empty promotion target, so frontier output is actionable.
- **StatefulCampaignNamesSteps**: v3 stateful campaign witnesses cannot
  terminate without minimized operation steps.
- **ProductionPathWitnessNamesOracle**: source-corpus and production-path
  differential witnesses cannot terminate without a named production path and
  oracle.
- **ExploitCrossProductHasThreatAndSteps**: exploit cross-product witnesses
  cannot terminate without campaign steps, threat-family classification, and
  an expected invariant.
- **TerminalStutter**: once a witness reaches a terminal classification,
  later discovery actions cannot rewrite its action or promotion target.
- **SourceGraphSlashingWitnessHasAuthorizationMetadata**: source-graph
  slashing witnesses cannot terminate without current-evidence and
  parent-pre-state metadata.

### MergeableChannelAccounting (typed mergeable channels)

- **BitmaskDiffMergeRoundTrip**: a `BitmaskOr` diff records newly-set bits
  as `end & !previous`; replaying it with OR reconstructs
  `previous OR end`, not `max(previous, end)`.
- **IntegerAddDiffMergeRoundTrip**: the existing `IntegerAdd` path keeps
  additive diff/merge behavior.
- **BitmaskMergeDoesNotDropBits**: OR merge preserves every bit set in the
  previous value or current value.
- **NonNumericPayloadHasNoNumericDiff**: tagged non-numeric values stay out
  of numeric merge accounting and must use the ordinary conflict path.
- **MergeableAccountingPreservesUserCost** and
  **MergeableAccountingPreservesSettlementCost**: mergeable-channel metadata
  updates do not mutate user runtime cost or fee-settlement cost evidence.
- **SlashSystemEffectPreservesCostBoundary**: slashing system effects compose
  with the typed mergeable-channel model without changing the cost boundary.

## Scope and Limitations

These TLA+ specifications complement the Rocq mechanization at `formal/rocq/cost_accounted_rho/`; neither tool subsumes the other. Readers should understand what TLA+ here establishes, what it does not, and how it relates to the Rocq proofs.

### What these models establish

- **Finite-state reachability**: TLC exhaustively explores every reachable state of each model under every legal scheduling. Any invariant violation or deadlock that can occur within the configured bounds will be reported.
- **Protocol-level correctness at the bounds used**: at the process/channel/token counts listed in each `.cfg`, each model's listed invariants hold in every reachable state. The core protocol models cover the headline token-conservation, fuel-gate-safety, cost-determinism, and nonnegative-token/fuel properties; the replay, threat, and search-frontier models cover their implementation-facing invariants. See the table above for per-model state counts.
- **Scheduling independence of cost**: `EvalScheduling.tla` specifically contrasts the internalized model against the externalized model side-by-side under all 3! = 6 body orderings, confirming that internalized cost is invariant under reordering while externalized cost is not.
- **Compound signature semantics**: `CompoundProtocol.tla` and `FullProtocol.tla` exercise Split-firing ordering, inner/outer gate sequencing, and Join mediators at concrete small depths.

### What these models do NOT establish

- **Properties for unbounded process, channel, or token counts**: TLC is a finite-state model checker. Claims like "cost is deterministic for *every* configuration" are not proven by TLC — only for the configurations in the `.cfg` files. Unbounded results are established in Rocq:
  - `ca_cost_deterministic` (`formal/rocq/cost_accounted_rho/theories/Confluence.v:474`) — deterministic cost for arbitrary systems.
  - `ca_strongly_normalizing` (`StrongNormalization.v:95`) — every system terminates.
  - `token_monotone_reachable` (`TokenConservation.v:98`) — token conservation for arbitrary reachability chains.
  - `fuel_events_consumed_perm` (`FuelEventDecomposition.v:198`) — consumed-event multiset determinism.
- **Refinement to Rust evaluator code**: the TLA+ models are specifications at the *protocol* level; they describe atomic actions (`FuelGateFires`, `InnerCommFires`, `SplitFires`, `JoinFires`, etc.) without modelling substitution, binding, or the RSpace storage layer. Establishing that the actual Rust implementation realizes these specifications is the responsibility of integration tests and property-based testing at implementation time (see migration doc §5.7 for the normalizer validation prescription and §6 for the test plan).
- **Cryptographic assumptions**: signature uniqueness, hash collision resistance, and the three properties of `hash_process` required by Rocq (verification doc §11.1) are assumed as trust-base constants in the models (`sigChannel` is an injective mapping in `CostAccountedRho.tla`). TLC does not verify cryptography.
- **Structural equivalence / normalizer correspondence**: the TLA+ models work with atomic identifiers (process names, channel names) and never encounter `≡`-reordering, so they cannot detect a hypothetical divergence between RSpace's normalizer and the Rocq `≡` relation. That obligation is discharged at implementation time via property-based tests (migration doc §5.7).
- **Unbounded nesting depth**: `FullProtocol.tla` tests depth 0/1/2; arbitrary depth is covered by Rocq induction, not TLC.

### Model-checking bounds used

| Model | Processes | Channels | Max nesting depth | Tokens / proc | Reachable states |
|---|---|---|---|---|---|
| `CostAccountedRho.tla` | 3 | 3 | 0 (atomic only) | 1 | 79 |
| `CompoundProtocol.tla` | 4 (incl. recursive spawn) | 4 | 1 | up to 2 | 63 |
| `FullProtocol.tla` | 7 | 12 | 2 (doubly-compound + Join) | up to 3 | 12,960 |
| `EvalScheduling.tla` | 3 bodies | — | 0 | 1 | 16 |
| `RuntimeBudgetReplay.tla` | 6 events | — | 0 | bounded budget 6 | 72 distinct / 203 generated |
| `CostAccountingThreats.tla` | 1 deploy boundary plus slash authorization view | — | 0 | bounded fuel 5, epochs 0..1, bonds 0..1 | 5,408 distinct / 401,025 generated |
| `CostAccountingSearchFrontier.tla` | 11 witness families | — | 0 | — | 34,167 distinct / 266,015 generated |
| `MergeableChannelAccounting.tla` | typed values over 2-bit bitmaps and bounded integers | — | 0 | bounded values 0..3 | 2,656 |

Running on larger bounds has not been attempted — doubly-compound depth-2 already exercises the cascading-Split + Join interactions and is the deepest scenario anticipated by the design.

### When to extend the models

Extend the TLA+ suite (rather than rely on Rocq alone) when introducing:

- **New atomic protocol actions** (e.g., Out-of-Phlogiston revert, checkpoint rollback interleaved with COMM). These are state-machine-shaped and are best captured in TLA+.
- **New concurrency scenarios** (e.g., shared channels with >2 processes per channel). Finite-state exhaustive search catches ordering bugs that Rocq inductive proofs may miss at the protocol level.
- **New invariants to cross-check** against the Rocq proofs. If a theorem's interpretation at the specification level is unclear, encoding it as a TLA+ invariant and model-checking a small instance is a fast sanity check.

Do **not** use TLA+ as a substitute for Rocq when:

- A property must hold for arbitrary configurations.
- The property concerns binding, substitution, or structural equivalence at a fine grain (the TLA+ models treat channel/process identifiers as opaque atoms).
