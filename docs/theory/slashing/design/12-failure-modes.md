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

### 12.2.1a Unbonded-window record pollution (LIVE — FV audit #6, Tier-0)

**Status: LIVE (open).** Confirmed by DAG-level dynamic tests
`tier0_unbonded_validator_discovery_is_detected_triggering_stamp`,
`tier0_polluted_record_falsely_neglects_honest_block`, and
`tier0_cross_node_observation_order_divergence`
(`equivocation_detector.rs` test module).

**Mechanism.** While an equivocator `V` is stake-0/unbonded, its
`EquivocationRecord` (minted with an empty witness set — e.g. the
`UnauthorizedSlashDeploy` record `EquivocationRecord::new(V, seq-1, {})`)
resolves to `EquivocationDetected` in `get_equivocation_discovery_status`
(`equivocation_detector.rs:280` unbonded / `:311` stake-0). Its caller
`check_neglected_equivocation` (`:214-216`) then **stamps the currently-
validated block's hash** into that record. Every block validated during
the unbonded window therefore pollutes `V`'s
`equivocation_detected_block_hashes`. Once `V` re-bonds (`stake > 0`),
`is_equivocation_detectable` runs and its first check (`:351-356`) returns
`true` for **any** later block whose justifications cite a stamped hash —
including a perfectly honest block — classifying it
`NeglectedEquivocation` (false rejection). Because different nodes stamp
different hashes depending on observation order, two honest nodes can
**disagree** on the same block → consensus divergence.

**Recommended remediation (consensus-critical; design separately).** Do
**not** stamp observer hashes into the record while the offender is
unbonded (suppress the stamp in the `EquivocationDetected`/stake-0 branch),
or clear `equivocation_detected_block_hashes` on re-bond, so only genuine
detectable-equivocation witnesses accumulate. Any fix must carry matching
full-stack FV updates (Rocq `detect_neglected*`, TLA+ `DetectNeglected`)
and multi-node integration coverage; it is **not** bundled with the FV
faithfulness fixes #1–#5 above.

### 12.2.1b BlockException → InvalidTransaction coercion (attribution caveat — FV audit #4)

**Status: reachable; soundness depends on determinism.** `block_processor.rs`
(`:357-376`) coerces a receiver-local `BlockException` (a runtime error raised
while the *receiver* validates a block) into the slashable variant
`InvalidTransaction`, dispatched against the *sender* via the same
record-creation path as a genuine invalid block. The coercion is now modelled in
Rocq (`BugFixDispatcher.v §4`: `coerce_block_outcome`,
`block_exception_coerces_to_slashable`, re-exported
`main_T9_3_block_exception_coerces_to_slashable`), so the taxonomy FV explicitly
covers this control-flow edge and confirms the coerced variant is slashable
(hence T-9.3 dispatch completeness fires a record).

**Caveat (not proven).** Attributing a *receiver-local* exception to the
*sender* is sound only if `BlockException` is **deterministic** across nodes. A
transient/local exception (storage error, non-deterministic replay) on one node
but not another would let the nodes disagree on the sender's slashability — the
same divergence family as §12.2.1a. The dispatch-completeness theorem does not
establish determinism; if a non-deterministic `BlockException` path is
reachable, the coercion needs a determinism guard (design separately).

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
| **Replay determinism break**                    | Block evaluation diverges; consensus splits.                                                              | Bisimilarity / replay determinism (T-15) is a design invariant.                                                  |

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
| `bonds_map` divergence between Rust / Scala nodes        | Bisimilarity violation — should not occur post the sixteen fixes; if seen, investigate as a regression. |
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
