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
| **Skipped sequence number under partition recovery**              | Pre-fix: exact `baseSeqNum + 1` lookup misses the equivocation. **Bug #7.** | Post-fix #7: canonical visible self-chain child above `baseSeq`, with same-branch collapse. |

### 12.2.1a Unbonded-window record pollution (RESOLVED — FV audit #6, Tier-0)

**Status: RESOLVED (remediation shipped).** The fix returns
`EquivocationOblivious` (not `EquivocationDetected`) for an unbonded /
stake-0 offender, making the caller's stamping arm unreachable so the
witness set can never be polluted. Verified full-stack: post-fix Rust
characterization tests
`tier0_unbonded_validator_discovery_is_oblivious_no_stamp`,
`tier0_polluted_record_falsely_neglects_honest_block`,
`tier0_cross_node_observation_order_converges`
(`equivocation_detector.rs` test module) plus the randomized-interleaving
proptest `unbonded_window_never_pollutes_or_falsely_neglects`
(`casper/tests/slashing/unbonded_window_pollution_determinism.rs`);
axiom-free Rocq (`EquivocationDetector.v`:
`unbonded_offender_oblivious`, `unbonded_stamp_noop`,
`unbonded_witness_order_independent`; re-exported as `MainTheorem.v`
`main_T9_1a_unbonded_oblivious` / `_no_stamp` / `_order_independent`); and
TLA+ (`EquivocationDetector.tla` invariants `Inv_NoStampAgainstUnbonded`,
`Inv_NeglectNotFromUnbondedPollution`; post-fix config
`MC_EquivocationDetector_unbonded_pollution.cfg` PASSES, pre-fix
`..._pre_fix.cfg` reproduces the counterexample; the eager model is
proved inductive by Apalache).

**Mechanism (historical, pre-fix).** While an equivocator `V` was stake-0/
unbonded, its `EquivocationRecord` (minted with an empty witness set — e.g.
the `UnauthorizedSlashDeploy` record `EquivocationRecord::new(V, seq-1, {})`)
resolved to `EquivocationDetected` in `get_equivocation_discovery_status`
(`equivocation_detector.rs:280` unbonded / `:311` stake-0). Its caller
`check_neglected_equivocation` then **stamped the currently-validated
block's hash** into that record. Every block validated during the unbonded
window polluted `V`'s `equivocation_detected_block_hashes`. Once `V`
re-bonded (`stake > 0`), `is_equivocation_detectable` returned `true` for
**any** later block whose justifications cited a stamped hash — including a
perfectly honest block — classifying it `NeglectedEquivocation` (false
rejection). Because different nodes stamped different hashes depending on
observation order, two honest nodes could **disagree** on the same block →
consensus divergence.

**Fix (candidate a + caller hardening).** The unbonded/stake-0 branches
(`equivocation_detector.rs:280,311`) now return `EquivocationOblivious`,
matching `slashing-specification.md §11.6` ("stake-0 offender: detected but
never slashed AND never recorded"). Because the caller stamps *only* on
`EquivocationDetected`, that arm is now unreachable and the caller's body
is hardened to a strict no-op with a loud regression `warn!`. Net effect:

- **No witness is ever recorded against an unbonded offender**, so
  `equivocation_detected_block_hashes` stays empty (nothing to slash — an
  unbonded offender has no stake).
- Detectability therefore reduces to the deterministic
  `updated_equivocation_children.len() > 1` mechanism (`:373`), which is a
  pure function of the DAG and the block's justifications — **independent
  of observation order**. All honest nodes compute the same verdict.

**Determinism argument.** Let `R` be `V`'s record and `h₁, h₂` be candidate
observer hashes seen in different orders by two nodes. Pre-fix, node A
computed `stamp(stamp(R, h₁), h₂)` and node B `stamp(stamp(R, h₂), h₁)`,
which could differ in downstream detectability. Post-fix, the discovery
status for the unbonded `V` is `Oblivious`, and `stamp_on_status` mutates
only on `Detected`; hence `stamp_on_status(R, Oblivious, h) = R` for every
`h`, so **both orders yield `R` unchanged** — the two-stamp result is
invariant under the `h₁ ↔ h₂` swap (Rocq
`unbonded_witness_order_independent`). With identical (empty) witness sets,
`is_equivocation_detectable` returns the same value on both nodes, so no
observation-order-dependent `NeglectedEquivocation` divergence can arise.

**Deployment migration (ops step, not consensus logic).** On a network that
ran the pre-fix code, any *already-polluted* witness sets persist in the
`EquivocationTrackerStore`. At a coordinated upgrade boundary, perform a
one-time **deterministic clear** of `equivocation_detected_block_hashes` for
records whose offender is currently unbonded (equivalently: clear all
witness sets and let genuine detectable-equivocation witnesses re-accumulate
from the bonded path). This is a state-migration operation applied
identically on every node at the same height — it is **not** consensus
logic and is **not** needed on a fresh genesis (where no pollution exists).

### 12.2.1b Objective invalidity and local validation faults

**Status: separated and verified.** `block_processor.rs` dispatches only an
explicit `BlockError::Invalid` through invalid-DAG recording and slash-evidence
creation. `BlockException` represents a receiver-local inability to complete
validation, remains outside the DAG, and enters bounded dependency recovery.
Certified recovery preserves the exact missing block hash or replay-state root.
On genesis-rooted history the absence remains a typed local fault; on a restored
node with truncated history it remains a typed missing dependency. Both paths
retain the block, request the named artifact, and acknowledge ready-path
ownership without minting invalidity. Same-artifact requests deduplicate, while
distinct artifacts and validators recover independently.
Deterministic replay mismatches and invalid cost certificates are converted to
the explicit `InvalidTransaction` path before reaching the processor.

Rocq models the boundary in `BugFixDispatcher.v §4`:
`block_exception_is_not_objective_invalidity` proves that an exception has no
invalid-block classification, while `explicit_slashable_invalidity_dispatches`
retains T-9.3 completeness for every explicitly classified slashable variant.
The top-level export is `main_T9_3_block_exception_is_local_fault`. The TLA+
end-to-end cost/consensus model independently checks that a recoverable local
fault never creates slash evidence and includes an expected-refutation control
which fails when that mapping is reintroduced.
`LocalFaultDeferral.v` and the concurrent `LocalValidationRecovery.tla` model
the typed boundary itself; their controls reject block/state identity collapse,
immediate self-requeue, and loss of the inconclusive block.

### 12.2.2 Storage layer

| Failure mode                                 | Effect                                           | Resolution                                                    |
|----------------------------------------------|--------------------------------------------------|---------------------------------------------------------------|
| **Race on equivocation insert**              | Pre-fix: one of two witnesses lost. **Bug #2.**  | Post-fix #2: re-route through `access_equivocations_tracker`. |
| **Tracker DB write fails** (disk full, etc.) | Caller sees an error; transaction not committed. | Standard error propagation (out of scope for this doc).       |

### 12.2.3 Proposing layer

| Failure mode                                    | Effect                                                                                                    | Resolution                                                                                                       |
|-------------------------------------------------|-----------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------|
| **Non-equivocation slashable variant detected** | Pre-fix: not recorded; relies on later proposer surfacing. **Bug #3.**                                    | Post-fix #3: dispatcher creates record uniformly.                                                                |
| **Unbonded proposer emits doomed slashes**      | Pre-fix: wasted CPU; the offending block is rejected at replay-time proposer-bond validation. **Bug #8.** | Post-fix #8: short-circuit to `Vec::new()` if proposer's bond = 0. |
| **Replay determinism break**                    | Block evaluation diverges; consensus splits.                                                              | Current replay-determinism and refinement checks are design invariants.                                         |

### 12.2.4 Effect layer

| Failure mode                                       | Effect                                                                                              | Resolution                                                                            |
|----------------------------------------------------|-----------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------|
| **Spoofed system auth token**                      | Deploy rejected at first guard.                                                                     | T-AuthCheck (`execute_invalid_auth_token_noop`; `Inv_InvalidAuthSlashNoPending`; UC-21). |
| **Invalid block hash not in `invalidBlocks`**      | Slash evidence is rejected without mutation.                                                        | Current PoS returns `(false, "invalid slash evidence")`; receive-side validation also rejects unknown hashes. |
| **Coop-vault slash transfer fails**                | Pre-fix: hangs forever. **Bug #4.**                                                                 | Post-fix #4: deterministic `(false, "transfer failed: ...")` return.                  |
| **Withdrawal `posVault.transfer` fails**           | Pre-fix: validator removed from `withdrawers` without payout — funds silently lost. **Bug #10.**    | Post-fix #10: validator stays in `withdrawers` for retry; `total_funds` invariant preserved. |
| **Slash twice on same validator**                  | Second slash is a no-op (T-Idem).                                                                   | Designed-in idempotence; T-Idem at `PoSContract.v:117`.                               |

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
| `bonds_map` divergence between nodes replaying the same DAG | Replay-determinism violation; investigate as a consensus regression.                                    |
| Validator stuck in `withdrawers` map for > N rounds      | Bug #10 (post-fix retry path). If `posVault.transfer` keeps failing, the validator's withdrawal entry remains intact across blocks; investigate the underlying vault failure cause. |
| Validator set size drops below `n − F`                   | F-neglectful quorum-drop (§12.3.1). Manual intervention required.                                    |
| Detector emits storage `KeyNotFound` for a block view     | Bug #11 pre-fix only. Post-fix, missing latest-message pointers contribute `∅` and traversal continues. |
| Neglect fires from two citations of the same child        | Bug #11 pre-fix only. Post-fix, distinct offender-child hashes are counted before applying `≥ 2`.       |
| Slash deploy executes against an honest, never-detected validator | Bug #12 pre-fix only. Post-fix, `SlashAuthorizedByEvidence` rejects unknown / unbonded / cross-epoch / duplicate-target deploys before replay (`Inv_RejectedSlashWithoutEvidenceNoPending`). |
| Rebonded validator gets slashed for prior-lifetime equivocation | Bug #13 pre-fix only. Post-fix, slash evidence is epoch-scoped: `(v, e₁)` evidence does not authorize a slash for `(v, e₂)` with `e₁ ≠ e₂` (`Inv_StaleEvidenceCannotSlashRebondedKey`). |
| Detected equivocator keeps their bond — no slash deploy emerges | Bug #14 pre-fix only. Post-fix, the proposer derives candidates from the authorized invalid-block evidence index (`Inv_NoInvalidLatestLivenessGap`). |
| Proposer panics or block has negative `seq`               | Bug #15 pre-fix only. Post-fix, `checked_pred`/`checked_succ` reject domain-boundary inputs cleanly. |
| Two different cited hashes for the same validator in one block's justifications | Bug #16 pre-fix only. Post-fix, validation rejects duplicate-validator justifications before detector projection (`Inv_AcceptedProjectionCardinality`). |

## 12.7 Test coverage

Spec §12 enumerates 112 use cases across four tiers:

- **Core (UC-01–UC-25):** baseline scenarios.
- **Tier A (UC-26, 27, 37, 38, 39, 41, 42, 43):** audit blockers.
- **Tier B (UC-28–UC-36):** one entry per remaining slashable
  `InvalidBlock` variant.
- **Tier C (UC-40, UC-44–UC-112):** operational, adversarial, and
  Sage-derived closure edge cases.

Each use case has an Outcome column (slashed / not-slashed /
rejected / admitted / error / behavioral) and a current Rust test module.
The documented harness and integration tests are implemented under
`casper/tests/slashing/`; UC-101 through UC-108 exercise the detector
threats from Sage findings 86 and 87 against the Rust production
detector path, UC-110 exercises the cross-coupled horizon campaign
fixtures from Sage Finding 116, UC-111 exercises the Rust-aligned
horizon-v2 lifecycle and detector-DAG fixtures from Sage Finding 117, and
UC-112 checks the current Rust detector path that retains existing
detected hashes during a record update.

---

**Next:** [§13 — References](13-references.md)
