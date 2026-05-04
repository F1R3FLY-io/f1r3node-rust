# 12 · Failure Modes & Recovery

This document catalogs the ways a slash event can *fail* — both
the documented bug failure modes (which are now fixed) and the
boundary-condition failures that are *expected* outside their
preconditions.

## 12.1 Overview

A slashing event involves multiple stages, each of which can fail.
The system is designed so that:

- **Detection failures** are silent — the validator is admitted
  as honest. (Bug #1, #5, #6, #7 sit here.)
- **Storage failures** are atomic — either evidence is preserved or
  not at all. (Bug #2 sits here.)
- **Proposing failures** are deterministic — either a `SlashDeploy`
  is emitted or it is not. (Bug #8 sits here; auth-token guard at
  effect layer rejects malformed deploys.)
- **Effect failures** are deterministic — either the slash succeeds
  with bond-zero or returns `(false, …)` in finite time. (Bug #4
  sits here.)
- **Fork-choice failures** are non-existent by construction — the
  GHOST estimator pulls fresh state every round. (No bug; design
  invariant.)

## 12.2 Failure modes by layer

### 12.2.1 Detection layer

| Failure mode                                                      | Effect                                                   | Resolution                                                 |
|-------------------------------------------------------------------|----------------------------------------------------------|------------------------------------------------------------|
| **Unsolicited equivocation** (no other block cites the bad block) | Pre-fix: silently dropped. **Bug #1.**                   | Post-fix #1: classified `IgnorableEquivocation`, recorded. |
| **Stake-0 bonded validator equivocates**                          | Pre-fix: silent classification, no slash. **Bug #5.**    | Post-fix #5: PoS bond contract enforces `stake > 0`.       |
| **Self-regression with no equivocation**                          | Pre-fix: passes `justification_regressions`. **Bug #6.** | Post-fix #6: drop `filterNot(_._1 == sender)`.             |
| **Skipped sequence number under partition recovery**              | Pre-fix: BFS misses the equivocation. **Bug #7.**        | Post-fix #7: BFS over creator-justification chain.         |

### 12.2.2 Storage layer

| Failure mode                                 | Effect                                           | Resolution                                                    |
|----------------------------------------------|--------------------------------------------------|---------------------------------------------------------------|
| **Race on equivocation insert**              | Pre-fix: one of two witnesses lost. **Bug #2.**  | Post-fix #2: re-route through `access_equivocations_tracker`. |
| **Tracker DB write fails** (disk full, etc.) | Caller sees an error; transaction not committed. | Standard error propagation (out of scope for this doc).       |

### 12.2.3 Proposing layer

| Failure mode                                    | Effect                                                                                                    | Resolution                                                                                                       |
|-------------------------------------------------|-----------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------|
| **Non-equivocation slashable variant detected** | Pre-fix: not recorded; relies on later proposer surfacing. **Bug #3.**                                    | Post-fix #3: dispatcher creates record uniformly.                                                                |
| **Unbonded proposer emits doomed slashes**      | Pre-fix: wasted CPU; the offending block is rejected at replay-time proposer-bond validation. **Bug #8.** | Post-fix #8 (mechanized in Rocq; not yet applied in Rust): short-circuit to `Vec::new()` if proposer's bond = 0. |
| **Replay determinism break**                    | Block evaluation diverges; consensus splits.                                                              | Bisimilarity / replay determinism (T-15) is a design invariant.                                                  |

### 12.2.4 Effect layer

| Failure mode                                  | Effect                                                         | Resolution                                                             |
|-----------------------------------------------|----------------------------------------------------------------|------------------------------------------------------------------------|
| **Spoofed system auth token**                 | Deploy rejected at first guard.                                | T-AuthCheck (Rholang-level observation; verified at PoS.rhox:437-439). |
| **Invalid block hash not in `invalidBlocks`** | Degraded path: `validator ← userPk` (slash whoever submitted). | Documented; spec §3.4.1.                                               |
| **Coop-vault transfer fails**                 | Pre-fix: hangs forever. **Bug #4.**                            | Post-fix #4: deterministic `(false, "transfer failed")` return.        |
| **Slash twice on same validator**             | Second slash is a no-op (T-Idem).                              | Designed-in idempotence; T-Idem at `PoSContract.v:117`.                |

### 12.2.5 Fork-choice layer

| Failure mode (none)                 | Note                                                                                                                                                                               |
|-------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **No failure mode by construction** | The fork-choice estimator pulls bond-map state fresh every round; no notification queue or cache to invalidate. T-10 (`fork_choice_exclusion`, `ForkChoice.v:60`) formalizes this. |

## 12.3 Boundary-condition "failures" (expected, not bugs)

### 12.3.1 More than F validators slashed

If `|closure| > F = ⌊(n−1)/3⌋`, the BFT-quorum precondition of T-12
fails. The active set drops below `n − F`, and consensus liveness
suffers. This is **expected** — if more than ⅓ of the validators
misbehave, no BFT consensus protocol can maintain liveness. The
F-neglectful quorum-liquidation example (§11.2; verification §10.8.2)
walks through n=4, F=1, |closure|=2.

The **system response** is to halt: with quorum below the BFT bound,
no further blocks finalize. Operators must manually intervene
(re-bond honest validators, or update the validator set).

### 12.3.2 All validators equivocate simultaneously

Pathological case: every validator equivocates on the same round.
Each is detected (T-2), each is recorded (T-9.2), each is slashed
(T-7), and the active set is empty. The protocol halts. This is
the protocol's *correct* response to a Byzantine-majority attack —
no consensus is possible, but the slash subsystem leaves a complete
on-chain record of what happened (every offender's bond is in the
Coop vault as forfeited stake).

### 12.3.3 Network partition + post-merge equivocation

A validator participates in two partitions, signing distinct blocks
in each. After merge, both blocks are visible; detection fires
T-9.2 (atomic insert) and standard slashing follows. Use case
UC-46 in spec §12 covers this.

### 12.3.4 Genesis-block invalid sender

If the genesis block declares an invalid sender, the slashing
subsystem cannot recover — the genesis is the only state-0 block,
and slashing assumes bonds are inherited from genesis. This is
an *operational* failure mode (bad bootstrap config), not a
runtime failure mode. Use case UC-49 covers this; the system's
response is to refuse to start (pre-genesis validation).

## 12.4 Recovery procedures

For each failure mode, the recovery is:

| Failure                            | Recovery                                                                                                                                                           |
|------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Detection silently drops bad block | Re-run validation when next proposer surfaces the offender. (Pre-fix only; post-fix this doesn't happen.)                                                          |
| Tracker race loses a hash          | Same — pre-fix only. Post-fix #2 prevents the race entirely.                                                                                                       |
| Dispatcher stub doesn't record     | Same — pre-fix only. Post-fix #3 creates the record uniformly.                                                                                                     |
| PoS transfer hangs                 | Pre-fix: indefinite. Post-fix #4: deterministic timeout returns `(false, "transfer failed")`. Validator returns to `EquivocatorRecorded`; next proposer can retry. |
| Auth-token spoofing detected       | Deploy rejected; no state change. No recovery needed.                                                                                                              |
| `>F` neglectful quorum-drop        | **Manual.** Operators re-bond honest validators or update validator set; the protocol cannot recover automatically.                                                |
| Genesis bad sender                 | **Manual.** Restart with corrected genesis config. Pre-genesis validation should catch this.                                                                       |

## 12.5 Liveness vs safety tradeoffs

The slashing subsystem is designed to be **safety-first** with
**liveness as a secondary goal**:

- **Safety (no honest validator slashed).** This is *unconditional*
  — T-1 (detection soundness) holds for all DAG states.
- **Liveness (every Byzantine action eventually slashed).** This is
  *conditional* on the BFT bound `|closure| ≤ F`. If too many
  validators misbehave, liveness fails; safety still holds.

This matches the standard BFT literature [LSP82, BKM18, ABPT19]:
safety is guaranteed in all conditions; liveness requires the BFT
bound.

## 12.6 Diagnostic signals (operator-facing)

When an operator sees one of these on a node, the following
failure modes are likely:

| Symptom                                                  | Likely failure mode                                                                                  |
|----------------------------------------------------------|------------------------------------------------------------------------------------------------------|
| Validator stuck in `SlashPending` for > N rounds         | Bug #4 (transfer-failure FIXME) — pre-fix only. Post-fix → `EquivocatorRecorded` automatically.      |
| Inconsistent `equivocation_records()` views across nodes | Bug #2 (race) — pre-fix only.                                                                        |
| `JustificationRegression` blocks not surfacing slashes   | Bug #3 (dispatcher stub) — pre-fix only.                                                             |
| Repeated rejected proposer-block submissions             | Bug #8 (unbonded proposer) — pre-fix only.                                                           |
| `bonds_map` divergence between Rust / Scala nodes        | Bisimilarity violation — should not occur post the nine fixes; if seen, investigate as a regression. |
| Validator set size drops below `n − F`                   | F-neglectful quorum-drop (§12.3.1). Manual intervention required.                                    |

## 12.7 Test coverage

Spec §12 enumerates 54 use cases across four tiers:

- **Core (UC-01–UC-25):** baseline scenarios.
- **Tier A (UC-26, 27, 37, 38, 39, 41, 42, 43):** audit blockers.
- **Tier B (UC-28–UC-36):** one entry per remaining slashable
  `InvalidBlock` variant.
- **Tier C (UC-40, UC-44–UC-54):** operational and adversarial.

Each use case has an Outcome column (slashed / not-slashed /
rejected / admitted / error / behavioral) and a test stub path.
Implementing the harness and 54 tests is out of scope for the
spec/verification/design documents; the stubs are normative for
whoever implements them.

---

**Next:** [§13 — References](13-references.md)
