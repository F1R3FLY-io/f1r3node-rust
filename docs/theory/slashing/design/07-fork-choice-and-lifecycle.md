# 07 · Fork-choice & Validator Lifecycle

## 7.1 The fork-choice layer — what it does

GHOST-style fork-choice [SZ15] selects the *heaviest* sub-tree
rooted at a candidate fork point. Each validator's *latest message*
contributes weight equal to its bond. The fork-choice estimator
must therefore be told to ignore the latest messages of validators
whose bond is zero.

In F1R3FLY, this filter is implemented as a *pull*: every
fork-choice round consults `dag.invalid_latest_messages` (see
`casper/src/rust/estimator.rs:65-70`) and excludes any validator
whose latest message is marked invalid. Because slashing forces a
validator's bond to zero (T-7), and subsequent blocks from a
zero-bond validator fail `InvalidBondsCache` validation and are
recorded in `invalid_blocks_index`, this filter is observationally
equivalent to the abstract "re-read `bonds_map` and exclude
validators with `bond = 0`" formulation used by T-10 (see
`slashing-verification.md §6.4`). The on-chain `bonds_map` is still
re-read each round to weight the surviving messages — the difference
is only in *which* channel selects the validators to exclude.

## 7.2 The pull-not-push design choice

| Design alternative                                                         | Tradeoff                                                                                                                 |
|----------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| **Pull**: ForkChoice reads `bonds_map` every round (chosen).               | Simple; no notification queue; bond is the *single source of truth*; race-free between slash and FC.                     |
| **Push**: PoS slash sends a notification to ForkChoice when a slash fires. | Requires an in-memory event bus; slash fires inside Rholang while ForkChoice is in Rust → cross-runtime IPC; race-prone. |
| **Hybrid**: ForkChoice maintains a cache invalidated on slash.             | Caching introduces consistency questions; pull's per-round read is already cheap.                                        |

The pull design is correct because the *bond map is on-chain*.
After a `SlashDeploy` executes, the post-state of the proposer's
block includes `allBonds[v] := 0`. Any validator that later
fork-chooses on a DAG containing that block reads the bond from the
on-chain state and gets `0`. There is no possible inconsistency: the
bond and the fork-choice filter are computed from the same on-chain
record.

## 7.3 Theorem — fork-choice exclusion (T-10)

**Statement.** *(`fork_choice_exclusion`, `ForkChoice.v:60`.)*

```
∀ v ∈ V,  v ∈ slashedSet  ⟹  v's latest message
                              is filtered from the fork-choice estimator.
```

**Intuition.** The `slashedSet` here is the set of validators with
`bond = 0` in the on-chain state (which is the *complement* of the
active validator set). The filter is a simple membership test: if
`v ∈ slashedSet`, drop `v`'s contribution; otherwise count it.

**Proof.** By unfolding the `filter_slashed` function in
`ForkChoice.v` and applying the `In_filter` standard library lemma.
TLC corroborates via `Inv_SlashedExcludedFromFC` in `MC_SlashFlow.tla`.

## 7.4 The validator lifecycle

[![Diagram 06 — Validator lifecycle](../diagrams/06-state-validator-lifecycle.svg)](../diagrams/06-state-validator-lifecycle.svg)

A bonded validator transitions through **six observable states**
plus **one documentation-only state**, `EquivocatorSuspect` —
seven in the lifecycle diagram. In the Rust code, the detector
transitions `Bonded → EquivocatorRecorded` directly in one atomic
step; the suspect state is split out for narrative clarity in the
lifecycle diagram and has no operational witness. The proofs in
`Validator.v` and `ValidatorLifetime.v` quantify only over the
six observable states.

```
Unbonded → Bonded → EquivocatorSuspect → EquivocatorRecorded →
SlashPending → Slashed → Removed
```

### 7.4.1 State definitions

| State                   | Meaning                                                                                                                                                      | Observable witness                                              |
|-------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| **Unbonded**            | Not currently bonded; `allBonds[v] = 0`; `v ∉ activeValidators`.                                                                                             | `bonds_map[v] = 0` ∧ `v ∉ active`                               |
| **Bonded**              | Bonded with stake > 0; included in active set.                                                                                                               | `bonds_map[v] > 0` ∧ `v ∈ active`                               |
| **EquivocatorSuspect**  | Detector observed a second block at same seq num. (Documentation-only; not observable in code.)                                                              | (no direct observation)                                         |
| **EquivocatorRecorded** | `EquivocationRecord(v, baseSeq, …) ∈ E`; pending slash.                                                                                                      | `(v, baseSeq) ∈ tracker` ∧ `bonds_map[v] > 0`                   |
| **SlashPending**        | A `SlashDeploy(b, v)` has been emitted by some proposer; not yet executed (replay in flight).                                                                | `SlashDeploy(_, v) ∈ block.body.system_deploys` (in some block) |
| **Slashed**             | PoS slash transition succeeded: `bond := 0`, rewards purged. Within the atomic stateUpdate, the bond write precedes (in source order) the active-set delete. | `bonds_map[v] = 0`                                              |
| **Removed**             | PoS removes `v` from `activeValidators` as part of the same atomic stateUpdate.                                                                              | `v ∉ active`                                                    |

### 7.4.2 Transitions

| Transition                                 | Trigger                                                                                                                                                                                         |
|--------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Unbonded → Bonded`                        | Successful `@PoS!("bond", …)` deploy.                                                                                                                                                           |
| `Bonded → Bonded`                          | Honest activity (proposing, validating).                                                                                                                                                        |
| `Bonded → EquivocatorSuspect`              | Detector observes a second block at same seq num.                                                                                                                                               |
| `EquivocatorSuspect → EquivocatorRecorded` | `insert_equivocation_record(v, s − 1, ∅)` succeeds.                                                                                                                                             |
| `EquivocatorRecorded → SlashPending`       | Next proposer's `prepare_slashing_deploys` includes `v`.                                                                                                                                        |
| `SlashPending → Slashed`                   | `@PoS!("slash", …)` succeeds (atomic stateUpdate at `PoS.rhox:477-486`).                                                                                                                        |
| `Slashed → Removed`                        | PoS removes `v` from `activeValidators` (same atomic stateUpdate; the two states are not separately observable in the implementation but are listed separately to match the spec §6 lifecycle). |
| `SlashPending → EquivocatorRecorded`       | Slash fails (transfer FIXME, bug fix #4 closes this — falls back to `EquivocatorRecorded`).                                                                                                     |
| `Removed → ⊥`                              | Terminal — cannot rejoin without a fresh bond deploy (which transitions to `Unbonded → Bonded`).                                                                                                |

### 7.4.3 Bug-fix notes on the lifecycle

- **Bug fix #2 (T-9.2)** ensures the
  `EquivocatorSuspect → EquivocatorRecorded` transition is atomic
  under concurrent insertions. (See §05 / Diagram 09.)
- **Bug fix #4 (T-9.4)** ensures
  `SlashPending → EquivocatorRecorded` happens deterministically
  when the Coop-vault transfer fails (rather than the validator
  being stuck in `SlashPending` indefinitely).
- **Bug fix #5 (T-9.5)** enforces an invariant that `Bonded` is
  unreachable with `bond = 0` (i.e. the `Bonded` state implies
  `bond > 0`). The detector's pre-fix
  `if stake ≤ 0 then EquivocationDetected` branch is deemed
  unreachable post-fix.

## 7.5 On "Slashed" and "Removed" as separate states

The spec §6 lists `Slashed` and `Removed` as separate states with a
`Slashed → Removed` transition. **Implementation note:** in the Rocq
mechanization and the actual `PoS.rhox` slash contract at lines
473-482, `state.allBonds[v] := 0`, `state.activeValidators \\ {v}`,
and `state.committedRewards \\ {v}` are **all written in one atomic
map-construction step**. There is no observable intermediate state
where the bond is zero but the validator is still in the active
set.

The two states are retained in the lifecycle for *narrative clarity*
and to match the spec §6 model: `Slashed` projects on `bond := 0`,
`Removed` projects on `v ∉ active`. Auditors verifying the
state-machine should treat the `Slashed → Removed` transition as
*conceptually instantaneous* — both are projections of the same
atomic stateUpdate at `PoS.rhox:477-486`. Diagram 06 may visually
combine them or show them separately depending on the renderer; the
spec is the authoritative source for state count.

## 7.6 Re-bonding and replay

A `SlashedRemoved` validator can in principle re-bond by submitting
a fresh `@PoS!("bond", …)` deploy with new stake. This transitions
them through `Unbonded → Bonded` again. *However:*

- Their `EquivocationRecord` is **not** removed from the tracker
  (record monotonicity / T-4 / `EquivocationRecord.v:254`). The
  evidence remains on-chain.
- A future re-equivocation creates a *new* `EquivocationRecord`
  at the new `(v, baseSeq')` key.

This is consistent with Ethereum's stance: "once slashed, always on
record". Re-bonding does not erase past misbehavior; it only
enables future participation.

> **Out-of-scope.** F1R3FLY currently uses a one-strike model:
> 100% of the bond is forfeited on a single slash. Graduated
> penalties (e.g. Cosmos-style fractional slashing) are listed as
> future work in spec §13.

## 7.7 The validator's perspective on a slash

From a single validator's viewpoint, the lifecycle looks like:

```
   bond posted    honest activity     misbehavior         slash fires
        │                │                  │                  │
        ▼                ▼                  ▼                  ▼
   Unbonded ────► Bonded ────► Bonded ────► (lifecycle states 3-5)
                   │                                            │
                   │  re-bond ◄──── new bond deploy ────────────│
                   │  (fresh state)                             │
                   ▼                                            ▼
                  ...                                       SlashedRemoved
```

The validator's *operator* sees: the node loses access to its bond
(the bond field on-chain becomes 0), the node is no longer scheduled
as a proposer (excluded from active set), and **the node's votes no
longer count in the GHOST tally** — fork-choice filters out the
slashed validator's latest message because its bond is zero (the
"pull, not push" design at §7.2). The node's signatures are still
cryptographically valid; other validators are simply not weighting
them. The node's *software* keeps running but has no protocol-level
influence.

## 7.8 Theorems that touch the lifecycle

| Theorem      | Statement                                                                                 | File:line                    |
|--------------|-------------------------------------------------------------------------------------------|------------------------------|
| T-7          | After `slash(ps, v)`, `allBonds[v] = 0`.                                                  | `PoSContract.v:75`           |
| T-8          | If transfer succeeds, `coopVaultBalance += allBonds[v]` (pre-slash).                      | `PoSContract.v:95`           |
| T-Idem (T-9) | A second slash on `v` is a no-op.                                                         | `PoSContract.v:117`          |
| T-10         | `v ∈ slashedSet ⟹ v`'s latest message filtered from GHOST.                                | `ForkChoice.v:60`            |
| T-9.5        | `active_implies_bonded(ps)` is preserved by `slash`.                                      | `BugFixStakeZero.v:36`       |
| T-9.4        | The slash transition either succeeds with bond-zero or returns `false` deterministically. | `BugFixTransferFailure.v:40` |

These six theorems together define the formal semantics of the
lifecycle: *what* happens on each transition (T-7, T-8, T-Idem,
T-10), *under what conditions* the transition is well-formed (T-9.5),
and *what* the failure mode looks like (T-9.4).

---

**Next:** [§08 — Two-level slashing & collusion](08-two-level-and-collusion.md)
