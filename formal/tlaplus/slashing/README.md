# Slashing — TLA+ Specifications and Model Checking

This directory contains four TLA+ specifications and their TLC model-checking
instances for the slashing subsystem. The verification model complements the
Rocq proofs at `formal/rocq/slashing/` by exhaustively model-checking finite
configurations of the protocol.

## Specifications

| File | Purpose |
|---|---|
| `EquivocationDetector.tla` | Pure detector state machine: validator equivocates → detection → status (admissible / ignorable / neglected). |
| `ConcurrentTracker.tla` | Models the lock-free vs. locked equivocation-tracker access. The lock-free version *demonstrates* the Rust-introduced race condition (Bug #2); the locked version proves the fix restores monotonicity. |
| `SlashFlow.tla` | End-to-end pipeline: detection → record → propose → SlashDeploy → PoS bond zeroing → fork-choice exclusion. |
| `TwoLevelSlashing.tla` | Level 1 + Level 2 slashing closure; proves termination and that the active validator set never falls below `n − ⌊(n−1)/3⌋`. |

Each `*.tla` has a corresponding `MC_*.tla` instance with TLC parameters
(validator count, max DAG depth, max equivocations) calibrated to keep the
state space ≤ 10⁵.

## Running

```sh
# All four model-check passes
tlc -workers 12 MC_EquivocationDetector.tla
tlc -workers 12 MC_ConcurrentTracker.tla     # NB: must FAIL pre-fix, PASS post-fix
tlc -workers 12 MC_SlashFlow.tla
tlc -workers 12 MC_TwoLevelSlashing.tla
```

`MC_ConcurrentTracker.tla` is parameterized by `Locked ∈ BOOLEAN`. With
`Locked = FALSE` the spec violates `Inv_NoOverwrite` (this is the bug); with
`Locked = TRUE` it passes. Both runs must be executed and recorded.

## Invariants

| Spec | Invariant | Meaning |
|---|---|---|
| EquivocationDetector | `Inv_DetectionSound` | Every emitted Admissible/Ignorable/Neglected status corresponds to a real equivocation in the trace. |
| EquivocationDetector | `Inv_DetectionComplete` | Every real equivocation is eventually emitted. |
| EquivocationDetector | `Inv_TaxonomyCorrect` | `is_slashable(s) = TRUE` iff `s ∈ {17 slashable variants}`. |
| ConcurrentTracker | `Inv_NoOverwrite` | The accumulated `equivocationDetectedBlockHashes` set never shrinks. |
| ConcurrentTracker | `Inv_RecordMonotone` | The set of `EquivocationRecord` entries grows monotonically. |
| ConcurrentTracker (temporal) | `[]<>RecordPersists` | Once recorded, a record persists. |
| SlashFlow | `Inv_SlashedExcludedFromFC` | After `SlashDeploy` succeeds, the offender's latest message is filtered from the fork-choice estimator. |
| SlashFlow | `Inv_BondsZeroAfterSlash` | `bondsMap[offender] = 0` after a successful slash. |
| SlashFlow | `Inv_ForfeitedToCoopVault` | `coopVaultBalance` increases by exactly the offender's pre-slash bond. |
| SlashFlow (temporal) | `<>SlashedEventually` | Every detected equivocation eventually results in a slash, given a live proposer schedule. |
| TwoLevelSlashing | `Inv_ActiveSetAboveQuorum` | `|activeValidators| ≥ n − ⌊(n−1)/3⌋` at every reachable state. |
| TwoLevelSlashing | `Inv_LevelClosureTerminates` | Iterated Level-2 slashing reaches a fixed point. |

## Correspondence to Rocq

See `slashing-verification.md` §10.5 for the explicit Rocq↔TLA+ correspondence
table. In summary:

| TLA+ invariant | Rocq theorem |
|---|---|
| `Inv_DetectionSound` | T-1 (`detection_sound` in `EquivocationDetector.v`) |
| `Inv_RecordMonotone` (with Locked=TRUE) | T-9.2 (`t_9_2_atomic_no_overwrite` in `BugFixAtomicTracker.v`) |
| `Inv_BondsZeroAfterSlash` | T-7 (`slash_zeros_bond` in `PoSContract.v`) |
| `Inv_ForfeitedToCoopVault` | T-8 (`slash_transfers_stake` in `PoSContract.v`) |
| `Inv_SlashedExcludedFromFC` | T-10 (`fork_choice_exclusion` in `ForkChoice.v`) |
| `Inv_StakeConservation` | T-7 + T-8 corollary (combination of `slash_zeros_bond` and `slash_transfers_stake`) |
| `Inv_LevelClosureTerminates` | T-11 (`t_11_level_2_termination` in `TwoLevelSlashing.v`) |
| `Inv_LivenessAsSafety` (Eager) | T-2 (`detection_complete` in `EquivocationDetector.v`) |

Note: `Inv_DetectionComplete` is the temporal property `Live_DetectionComplete`
in `EquivocationDetector.tla`; under the eager rewrite
`EquivocationDetectorEager.tla` it becomes the safety invariant
`Inv_LivenessAsSafety` (see §10.4 of the verification doc).
`Inv_NoOverwrite` is defined in `ConcurrentTracker.tla` for documentation
but the actually-checked invariant in both `MC_ConcurrentTracker.cfg` and
`MC_ConcurrentTracker_pre_fix.cfg` is the stronger `Inv_RecordMonotone`.
`Inv_ActiveSetAboveQuorum` is defined in `TwoLevelSlashing.tla` but is a
hypothesis-bearing property; the BFT-style claim is mechanized in Rocq as
`t_12_bft_quorum_preservation`.

## What TLA+ proves and does not

**TLA+ proves:** That for the modeled finite configurations (e.g. up to 4
validators, DAG depth ≤ 6, ≤ 3 equivocations), the protocol satisfies every
listed invariant on every reachable state and every fair execution.

**TLA+ does not prove:** Universal claims for arbitrary `n`, arbitrary DAG
depth, or arbitrary equivocation count. Those are the province of the Rocq
proofs. TLA+ is here to catch specification bugs the Rocq proofs might mask
(e.g. an inadvertently strong hypothesis), not to certify the protocol on
unbounded state.
