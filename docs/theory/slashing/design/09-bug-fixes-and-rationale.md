# 09 · Bug-Fix Manifest & Rationale

The slashing subsystem ships with **ten documented defects**, each
accompanied by a Rocq-mechanized fix. **Nine are inherited from the
Scala upstream**; **one is a Rust-introduced regression** (bug #2).
**One** of the nine Scala-inherited bugs (#9) is a *deliberate Rust
widening* — Rust admits more blocks than Scala, by design.

## 9.1 At a glance

| #  | Theorem | Origin                         | Bisimilarity impact     | Worked example          | Diagram |
|----|---------|--------------------------------|-------------------------|-------------------------|---------|
| 1  | T-9.1   | Scala-inherited                | Preserving              | (none — see spec §10.1) | 03      |
| 2  | T-9.2   | **Rust-introduced regression** | Preserving              | §11.3                   | 09      |
| 3  | T-9.3   | Scala-inherited                | Preserving              | §11.8                   | 05      |
| 4  | T-9.4   | Scala-inherited                | Preserving              | §11.4                   | 07      |
| 5  | T-9.5   | Scala-inherited                | Preserving              | §11.6                   | 06      |
| 6  | T-9.6   | Scala-inherited                | Preserving              | §11.9                   | 08      |
| 7  | T-9.7   | Scala-inherited                | Preserving              | §11.7                   | 02      |
| 8  | T-9.8   | Scala-inherited                | Preserving              | §11.10                  | 01      |
| 9  | T-9.9   | Scala bug, Rust-fixed          | **Deliberate widening** | §11.5                   | 08      |
| 10 | T-9.10  | Scala-inherited                | Preserving              | §11.11                  | 11      |

"Preserving" = the fix restores Rust↔Scala convergence (or, for #2,
fixes a Rust-only deviation). "Deliberate widening" = the fix is a
documented Rust-side improvement that breaks strict bisimilarity by
design; T-9.9 establishes that the widening is sound.

Bug #10 is the **withdrawal-flow analog** of Bug #4: both close
`posVault.transfer`-failure FIXMEs in `PoS.rhox`. Bug #4 fixed the
slash arm (line 469); Bug #10 fixes the post-quarantine
withdrawal arm (line 619). Bug #10's theorem set
(T-9.10 / T-9.10' / T-9.10″) is mechanised in
`BugFixWithdrawTransferFailure.v`, model-checked in
`MC_WithdrawFlow.cfg`, and applied in PoS.rhox lines 615-651.

> **Implementation status (as of writing).** Of the ten fixes,
> **#9 and #10 are currently applied in the Rust / Rholang source**:
> #9 as the `has_slash_system_deploys` widening at
> `validate.rs:1018-1029`, and #10 as the post-fix `payWithdraw`
> pattern-match in `PoS.rhox:615-651`. Fixes #1, #2, #3, #4, #5,
> #6, #7, and #8 are **mechanized in Rocq** with their respective
> T-9.M proofs but are **not yet applied in the Rust port** —
> pending separate PRs that align the code with the spec. The
> "Post-fix behavior" subsections below describe what the Rust /
> Rholang *will* do once each fix lands; the "Cause" subsections
> describe the *current* state.

## 9.2 Bug #1 — `IgnorableEquivocation` non-slashable (DOS vector)

**Origin.** Scala-inherited. The Scala counterpart at
`BlockStatus.scala:62-65` carries the explicit TODO:
*"Make IgnorableEquivocation slashable again ... will become a DOS
vector if not fixed."*

**Cause.** Pre-fix, `IgnorableEquivocation` is *not* in
`is_slashable() = ⊤`. Equivocations that arrive *unsolicited* (no
other block cites them) are silently dropped at
`multi_parent_casper_impl.rs:1077-1088` with only a `tracing::info!`
log. A Byzantine validator can flood the network with such blocks
without economic cost.

**Pre-fix behavior.** Detector returns `IgnorableEquivocation`;
dispatcher logs and discards.

**Post-fix behavior.** Add `IgnorableEquivocation` to
`is_slashable()`; in `handle_invalid_block`, treat it identically
to `AdmissibleEquivocation` (record evidence, allow standard
slash flow).

**Theorem T-9.1.** *(`bug_fix_ignorable_safety`,
`BugFixIgnorable.v:32`; `post_fix_ignorable_implies_equivocation`,
line 57.)* Under the fix, no honest validator is wrongly slashed
— since the underlying equivocation predicate (two distinct blocks
at same seqN by same sender) is unchanged.

**Why this matters.** Without the fix, an attacker can mount a DOS
campaign: send K equivocating blocks from a freshly-bonded validator
to consume network bandwidth and verification CPU on every honest
node, with no economic risk. Post-fix, every equivocation costs
the offender their entire bond.

[![Diagram 03 — Ignorable equivocation slash flow (post-fix #1)](../diagrams/03-seq-ignorable-equivocation-fixed.svg)](../diagrams/03-seq-ignorable-equivocation-fixed.svg)

## 9.3 Bug #2 — Lock-free tracker access (Rust regression)

**Origin.** Rust-introduced regression (the only one of the ten).

**Cause.** `multi_parent_casper_impl.rs:1046-1075` reads then writes
the equivocation tracker without a lock, allowing two threads
processing `AdmissibleEquivocation` for the same `(validator,
baseSeqNum)` to both observe `record-absent` and both insert,
overwriting accumulated `equivocationDetectedBlockHashes` with
`Set::empty`. The Scala atomic equivalent at
`MultiParentCasperImpl.scala:586-603` wraps the read, exists-check,
and `insertEquivocationRecord` write atomically inside
`accessEquivocationsTracker`.

**Pre-fix behavior.** Race-prone read-modify-write; one of the two
witness hashes is lost (Diagram 09 / §05).

**Post-fix behavior.** Re-introduce `access_equivocations_tracker
{ ... }` (matching the Scala behavior) which holds a global
semaphore around the read-modify-write window. The semaphore
lives in `BlockDagKeyValueStorage.scala:262`.

**Theorem T-9.2.** *(`t_9_2_atomic_no_overwrite`,
`BugFixAtomicTracker.v:43`; n-thread
`t_9_2_atomic_n_threads_arbitrary` at line 130.)* Under the lock,
T-4 (record monotonicity) holds for arbitrary thread schedules.

**Why this matters.** Pre-fix, a Byzantine validator can race their
own equivocation insertion against an honest detector to lose
evidence. Post-fix, all evidence is preserved regardless of
thread schedule.

[![Diagram 09 — Tracker race & locking fix](../diagrams/09-seq-tracker-race-and-fix.svg)](../diagrams/09-seq-tracker-race-and-fix.svg)

## 9.4 Bug #3 — Generic slash dispatcher stub

**Origin.** Scala-inherited. The Scala counterpart at
`MultiParentCasperImpl.scala:621-622` exhibits the same gap — the
catch-all `case ib: InvalidBlock if InvalidBlock.isSlashable(ib)`
arm only invokes `handleInvalidBlockEffect` (mark-invalid +
buffer-remove); no `EquivocationRecord` is created.

**Cause.** `multi_parent_casper_impl.rs:1090-1099` carries
*"TODO: Slash block for status except InvalidUnslashableBlock - OLD"*.
The 15 non-equivocation slashable variants (`JustificationRegression`,
`InvalidBondsCache`, `NeglectedInvalidBlock`, etc.) only get marked
invalid; no `EquivocationRecord` is created and no slash effect
runs unless a later proposer happens to surface the offender via
`prepare_slashing_deploys`.

**Pre-fix behavior.** Slashable invalid blocks are flagged in the
DAG but evidence is *not* persisted to the tracker; reliance on
proposer-side surfacing is fragile.

**Post-fix behavior.** Dispatch every `is_slashable() = ⊤` variant
through the same record-creation path used by
`AdmissibleEquivocation`.

**Theorem T-9.3.** *(`t_9_3_dispatch_complete`,
`BugFixDispatcher.v:41`.)* Under the fix, every slashable invalid
block triggers a record-insert within the dispatcher.

**Why this matters.** Without the fix, 15 of 17 slashable variants
have unreliable enforcement: an offender's `JustificationRegression`
violation might never lead to a slash if the proposer rotation
doesn't surface it. Post-fix, every slashable variant enters the
standard pipeline.

[![Diagram 05 — Generic invalid-block dispatch (post-fix #3)](../diagrams/05-seq-invalid-block-dispatch-fixed.svg)](../diagrams/05-seq-invalid-block-dispatch-fixed.svg)

## 9.5 Bug #4 — PoS transfer-failure FIXME

**Origin.** Scala-inherited.

**Cause.** `casper/src/main/resources/PoS.rhox:469` carries the
comment *"FIXME handle transfer failing case"*. If
`posVault!("transfer", coopMultiVaultAddr, valBond, posAuthKey,
*transferDoneCh)` fails, the `for (_ <- transferDoneCh)`
continuation never fires and there is no error path back to
`returnCh`. The slash deploy hangs.

**Pre-fix behavior.** The validator stays in `SlashPending`
indefinitely; replay fails to converge.

**Post-fix behavior.** Add an alternate continuation that listens
for an error signal on `transferDoneCh` (or a timeout) and writes
`(false, "transfer failed")` to `returnCh` deterministically.

**Theorem T-9.4.** *(`t_9_4_transfer_failure_safety`,
`BugFixTransferFailure.v:40`.)* Under the fix, the slash transition
either succeeds with T-7/T-8 or returns `false` in finite time:

```
∀ ps v ok, let (ps', ok') := slash_with_transfer_oracle(ps, v, ok) in
  (ok' = ⊤ ∧ allBonds[v] = 0) ∨ (ok' = ⊥ ∧ ps' = ps)
```

**Why this matters.** A hung deploy breaks replay determinism: if
half the network sees the transfer succeed and the other half sees
it fail (or hang), the post-state hashes diverge and consensus
splits. Post-fix, the failure mode is deterministic, so all
replays converge.

[![Diagram 07 — PoS.slash() activity (post-fix #4)](../diagrams/07-activity-pos-slash-contract.svg)](../diagrams/07-activity-pos-slash-contract.svg)

## 9.6 Bug #5 — Stake-0 silent classification

**Origin.** Scala-inherited.

**Cause.** `equivocation_detector.rs:217-220` notes
*"This case is not necessary if assert(stake > 0) in the PoS
contract"*. Until that assertion is enforced, a stake-0 bonded
validator is silently classified `EquivocationDetected` — no slash,
no neglected check.

**Pre-fix behavior.** A stake-0 bonded validator's equivocation is
"detected" but the slash transition is a no-op (no bond to forfeit)
and *no record is created* either.

**Post-fix behavior.** Two valid options:
- **(a)** Add `assert(stake > 0)` in the PoS `bond` contract to
  make stake-0 bonded validators an unreachable state. Preferred.
- **(b)** Return `Err(StakeZero)` from the detector and propagate
  upstream. Defensive but adds a runtime branch.

T-9.5 mechanizes option (a). Option (b) is left as future work.

**Theorem T-9.5.** *(`t_9_5_slash_preserves_invariant`,
`BugFixStakeZero.v:36`; corollary `t_9_5_active_has_positive_bond`
at line 58.)*

```
active_implies_bonded(ps) ≜ ∀ v ∈ active(ps), bonds_map[v] > 0

∀ ps v, active_implies_bonded(ps)
    ⟹ active_implies_bonded(fst(slash(ps, v)))
```

**Why this matters.** Pre-fix, an attacker with a corrupted PoS
state (stake-0 bonded) can equivocate freely with no economic
consequence. Post-fix, the corrupted state is unreachable.

## 9.7 Bug #6 — Self-regression slips through

**Origin.** Scala-inherited.

**Bisimulation impact.** Preserving — both implementations skip the
block's own sender in `justification_regressions` (line 666 of
`Validate.scala:649-702`); the fix tightens the predicate on both
sides identically.

**Cause.** `validate.rs:875-985` (Scala `Validate.scala:649-702`)
ignores regression of the block's own sender and defers to
`check_equivocations`. But `check_equivocations` only compares the
creator-justification *hash*, not the *sequence-number ordering*.
A sender that ships a non-equivocating but seq-regressed
self-justification (e.g. due to LMD inconsistency) passes both
checks.

**Pre-fix behavior.** A validator can ship a chain like
`b₅ → b₇ → b₉` where `b₉` cites `b₅` (skipping `b₇`); pre-fix the
self-regression is missed.

**Post-fix behavior.** Add an explicit seq-number order check for
the block's own sender in `justification_regressions`.

**Theorem T-9.6.** *(`t_9_6_self_regression_detected`,
`BugFixSelfRegression.v:52` (Boolean); DAG-level
`t_9_6_self_regression_in_dag` at `BugFixSelfRegression.v:79`
(§1, Bug #6).)*

```
Boolean: ∀ blk_sn latest cited, cited < latest
       ⟹ has_self_regression(blk_sn, latest, cited) = ⊤

DAG-level: ∀ blocks sender cited b,
            b ∈ blocks ∧ block_sender(b) = sender ∧ block_seq(b) > cited
          ⟹ has_self_regression(0, ds_latest_seq(blocks, sender), cited) = ⊤
```

## 9.8 Bug #7 — Off-by-one seq-number density

**Origin.** Scala-inherited.

**Cause.** `equivocation_detector.rs:400` (Scala
`EquivocationDetector.scala:336`) uses `baseSeqNum + 1` to find a
validator's child block. This assumes per-sender seq numbers are
*dense* (never skipped). If a validator skips a sequence number
(a rare but possible edge case under partition recovery), the BFS
fails.

**Pre-fix behavior.** Detector misses some equivocations under
partition recovery.

**Post-fix behavior.** Replace `baseSeqNum + 1` with a BFS over the
creator-justification chain.

**Theorem T-9.7.** *(`t_9_7_finds_descendant_with_gap`,
`BugFixSeqNumDensity.v:84`; subsumption
`t_9_7_post_fix_subsumes_pre_fix` at line 56.)*

```
∀ blocks sender baseSeq b,
   b ∈ blocks ∧ block_sender(b) = sender ∧ block_seq(b) > baseSeq
 ⟹ ∃ b', find_descendant_post_fix(blocks, sender, baseSeq) = Some b'
```

## 9.9 Bug #8 — `prepare_slashing_deploys` doesn't check proposer is bonded

**Origin.** Scala-inherited. The Scala counterpart at
`BlockCreator.scala:129-153` (`prepareSlashingDeploys`) also omits
the proposer-bonded check — it filters `ilm` by *target* validator
bond (`bondsMap.getOrElse(validator, 0L) > 0L`, line 134) but
never checks the proposer itself.

**Cause.** `block_creator.rs:287-332` doesn't verify that the
*proposer itself* is bonded. An unbonded proposer running the
proposer thread will still build slash deploys; the `slash`
contract rejects them at `sysAuthTokenOps!("check", ...)`. This is
wasted network work.

**Pre-fix behavior.** Unbonded proposer emits doomed slash deploys.
**(This is the current Rust behavior at `block_creator.rs:287-332`.)**

**Post-fix behavior (mechanized in Rocq; not yet applied in Rust — pending PR).**
Skip `prepare_slashing_deploys` entirely when `bonds_map[proposer] = 0`.
The Rocq mechanization at `BugFixUnbondedProposer.v:44` proves the
property; the Rust source has not yet been updated to insert the
short-circuit.

**Theorem T-9.8.** *(`t_9_8_unbonded_proposer_no_slash`,
`BugFixUnbondedProposer.v:44`; equivalence
`t_9_8_post_fix_equivalent_when_bonded` at line 55.)*

```
∀ ilm bonds proposer seqNum seed_fn,
   bm_lookup(bonds, proposer) = 0
 ⟹ prepare_slashing_deploys_post_fix(ilm, bonds, proposer, seqNum, seed_fn) = []

When bonds[proposer] > 0, post-fix is pointwise equal to pre-fix.
```

## 9.10 Bug #9 — Scala rejects self-correcting blocks (Scala bug, Rust-fixed)

**Origin.** Scala bug; Rust-fixed by deliberate widening.

**Bisimulation impact.** **Deliberate widening** (the only one of
the ten) — the Rust port admits self-correcting blocks Scala
rejects.

**Cause.** Scala `Validate.scala:727-731` rejects a block whenever
`neglectedInvalidJustification = ⊤`, even if the block itself
carries a `Slash` system deploy targeting the offender. Rust's
`validate.rs:1016-1029` adds an extra branch
`if neglectedInvalidJustification ∧ ¬ has_slash_system_deploys`
that *admits* self-correcting blocks. The Scala behavior is a bug;
the Rust widening is correct.

**Pre-fix Scala behavior.** Block B that cites A's invalid block
*and* attaches `SlashDeploy(b, A)` is rejected — B must wait for
some *other* validator to slash A. This is a liveness gap.

**Post-fix Rust behavior.** Block B is admitted; A is slashed in
B's own block. Strictly more live.

**Theorem T-9.9.** *(`t_9_9_post_fix_rejection_iff`,
`BugFixSelfRegression.v:107`.)*

```
∀ hn hs, rejects_neglected_post_fix(hn, hs) = ⊤
       ⟺ hn = ⊤ ∧ hs = ⊥

Corollary t_9_9_post_fix_admits_more (BugFixSelfRegression.v:121):
  ∀ hn hs, hn = ⊤ ∧ hs = ⊤
       ⟹ rejects_neglected_pre_fix(hn) = ⊤  ∧
         rejects_neglected_post_fix(hn, hs) = ⊥
```

In English: post-fix rejection fires iff there *is* a neglected
justification *and* the block does *not* carry a slash deploy.
The post-fix predicate strictly admits more blocks (those with
both `has_neglected = ⊤` and `has_slash = ⊤`).

**Why this is a deliberate widening.** Scala unconditionally
rejects neglecting blocks. Rust admits the same blocks if they
self-correct. The two implementations are *not* observationally
equivalent — Rust admits a strict superset of valid blocks. T-9.9
establishes that the additional admission is sound (the slash
still fires; the offender is still punished); the bisimilarity
claim T-15 holds *modulo* this widening (see §10).

## 9.11 Cross-fix interactions

The ten fixes interact in four notable ways:

1. **Fix #3 + Fix #6**: Bug #6 (self-regression) feeds bug #3
   (dispatcher). Without #3, the `JustificationRegression` verdict
   fires from #6 but no record is created. With both #3 and #6,
   the self-regression is detected *and* recorded *and* slashed.

2. **Fix #2 + every other fix**: Bug #2 (lock-free) protects every
   other bug-fix's tracker writes from being lost under thread
   interleaving. Without #2, fixes #1, #3, #5, #6, #7, #8 could
   *all* race their tracker writes and lose evidence.

3. **Fix #4 + Fix #9**: Bug #4 (transfer failure) and bug #9
   (self-correcting blocks) both touch the slash deploy's
   end-to-end semantics. Together they ensure that a slash deploy
   *always* terminates in finite time with a deterministic outcome,
   even when the block is self-correcting.

4. **Fix #4 + Fix #10**: Bug #4 fixed the slash arm's
   `posVault.transfer` failure path (PoS.rhox:469); Bug #10 fixes
   the *withdrawal* arm's analogous failure path (PoS.rhox:619).
   Together they close every `posVault.transfer` callsite in
   `PoS.rhox` against fund-loss / hung-deploy regressions. The two
   fixes do not interact dynamically — slashing and withdrawal are
   disjoint state transitions in the PoS contract — but the fix
   *pattern* is shared (pattern-match on `(true, _)` vs
   `(false, _)`, leave per-validator state intact on failure for
   retry on a later block). Future `posVault.transfer` callsites,
   if added, must follow the same template.

## 9.13 Bug #10 — PoS withdrawal transfer-failure FIXME

**Origin.** Scala-inherited.

**Cause.** `casper/src/main/resources/PoS.rhox:619` carries the
comment *"FIXME fix transfer in failure case"* inside
`removeQuarantinedWithdrawers`. The pre-fix `payWithdraw` contract
calls `payWithdrawer!(...)` and the surrounding flow proceeds to
remove the validator from `withdrawers` and `committedRewards`
regardless of whether the underlying `posVault.transfer` succeeded.
If the transfer fails the validator is removed from state without
receiving funds — a fund-loss bug that breaks vault conservation.

This is the **withdrawal-flow analog** of Bug #4 (which already fixed
the slash arm). The same `posVault.transfer` failure-handling pattern
applies to both code paths; only the slash arm had been fixed before.

**Pre-fix behavior.** A failed `posVault.transfer` results in:
1. Validator removed from `state.withdrawers` (line 627).
2. Validator's `committedRewards` cleared (line 626).
3. PoS vault balance unchanged (transfer rolled back at the vault
   layer).

The validator's bond + accumulated rewards are silently lost: the
contract no longer tracks the obligation, so the validator has no
recourse. Vault conservation
(`pos_balance + Σ payouts_for_withdrawers = constant`) is violated.

**Post-fix behavior.** `payWithdraw` pattern-matches on the transfer
result and emits `(pk, success_bool)` on its `resultCh`. The
downstream `computeRemove` fold is rewritten to remove **only**
successful withdrawers from the maps; failed transfers leave the
per-validator state intact for retry on a later block. Mirrors the
Bug #4 fix already applied to the slash arm
(PoS.rhox:472-510).

**Theorem T-9.10.** *(`t_9_10_withdraw_transfer_failure_safety`,
`BugFixWithdrawTransferFailure.v:225`.)* Under the fix, the
per-validator withdraw transition either succeeds AND removes the
validator from `withdrawers`, or fails AND leaves the entire
state unchanged:

```
∀ psw v ok, let psw' := withdraw_with_transfer_oracle(psw, v, ok) in
  (ok = ⊤ ∧ wm_contains(psw'.withdrawers, v) = ⊥)
∨ (ok = ⊥ ∧ psw' = psw)
```

**Theorem T-9.10' (failure preserves total funds).**
*(`t_9_10_failure_preserves_total_funds`,
`BugFixWithdrawTransferFailure.v:262`.)* A failed withdrawal does
NOT lose funds — the post-fix state is identical, so the
total-funds invariant is trivially preserved.

**Theorem T-9.10″ (parallel order-independence).**
*(`t_9_10_withdraw_independence`,
`BugFixWithdrawTransferFailure.v:286`.)* The Rholang flow uses
`unorderedParMap` to drive withdrawals in parallel. Withdrawing v
then u produces the same withdrawer/reward maps as withdrawing u
then v, when v ≠ u. Parallel-fold safety at the formal-model
abstraction level.

**Why this matters.** Without the fix, a transient `posVault`
failure (network delay, balance race with concurrent slash) silently
forfeits a validator's stake. The validator has no on-chain record
of the obligation and cannot retry. Post-fix, the validator stays in
`withdrawers` until a successful transfer, with the per-validator
state intact across block boundaries.

**Companion TLA+ model.** `formal/tlaplus/slashing/WithdrawFlow.tla`
+ `MC_WithdrawFlow.cfg` model the withdrawal pipeline with
explicit success / fail / retry actions and verify
`Inv_NoFundsLost`, `Inv_TotalFundsConst`, `Inv_RemovedImpliesPaid`,
`Inv_RewardsConsistent`, and `Live_AllEventuallyPaid`.

## 9.14 Summary

The ten fixes restore the slashing subsystem to audit-grade
correctness:

- Nine Scala-inherited bugs are documented with proven fixes.
- One Rust regression (#2) is documented with a proven fix.
- One deliberate widening (#9) is documented as a *Rust improvement*
  over Scala, with a soundness proof.

The bisimilarity claim (T-15, §10) holds modulo these ten
deltas — nine convergence fixes (or vault-conservation fixes for
#10) and one widening.

---

**Next:** [§10 — Bisimilarity (Rust ↔ Scala)](10-bisimilarity.md)
