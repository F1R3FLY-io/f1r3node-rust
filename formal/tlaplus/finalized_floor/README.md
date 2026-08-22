# Finalized-floor TLA+ models

This directory contains the explicit-state and symbolic transition models for
Casper finalized-floor derivation, state preservation, validator recovery, and
parallel validation. The normative protocol description is
[`docs/casper/theory/finalized-floor/finalized-floor-specification.md`](../../../docs/casper/theory/finalized-floor/finalized-floor-specification.md),
and the proof and execution evidence is cataloged in
[`docs/casper/theory/finalized-floor/finalized-floor-verification.md`](../../../docs/casper/theory/finalized-floor/finalized-floor-verification.md).

## Model families

| Module | Boundary |
|---|---|
| `FinalizedFloor.tla` | deterministic floor walk, merge cap, and no lost parent write |
| `FinalizedFloorScan.tla` | complete parent-band scan |
| `FinalizerProgress.tla` | complete candidate search and restart-safe progress |
| `AccountableFinality.tla` | exact weighted asynchronous certificate support and accountable conflicts |
| `StateLineageFinality.tla` | causal certificate, state certificate, and committed-effect lineage |
| `StateEffectProvenance.tla` | exact accepted/rejected effect recurrence and arrival-order convergence |
| `StatePreservingForkChoice.tla` | causal-tip retention and protected-floor replay after LFB advancement |
| `ParallelValidatorConsensus.tla` | split node-local receive, capture, replay, validation, support, crash/restart, and atomic promotion transitions |
| `CertifiedFloorPromotion.tla` | complete causal discovery of dual-certified off-main floors |
| `LatestMessageCoverage.tla` | exact worklist refinement of pairwise supporter reachability |
| `SnapshotFloorMaterialization.tla` | complete provenance closure before snapshot selection |
| `HeartbeatFinalityBackpressure.tla` | bounded asynchronous recovery admission and rotating leadership |

## Parallel-validator contract

`ParallelValidatorConsensus.tla` is the concurrency capstone. It has no global
validation phase. Each validator/candidate pair owns a phase, and every action
updates one local participant or delivers one support message. The shared
history current-root pointer is explicitly modeled so the invariants can prove
that capture and publication depend on the node-local floor root instead.

The safe TLC configurations use three validators, two candidates, exact
$`60/20/15`$ weights, strict majority, and `FTT=0.1`. The baseline exhausts the
finite schedule graph through eight actions without task failure. A second safe
configuration enables capture/replay crashes and restarts over the same bound.
Its eight unsafe configurations each enable one defect and must fail:

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
through routine bound 6. The crash-root-deletion configuration must also produce
the `ReplayRootsRemainLocallyRecorded` counterexample through bound 5. The
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
