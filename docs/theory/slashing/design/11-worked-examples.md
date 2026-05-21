# 11 · Worked Examples

This file walks eleven end-to-end traces through the slashing pipeline.
Each example is a small, concrete scenario you can replay mentally.
Each cites the diagram(s) and theorems it exercises.

## 11.1 Single AdmissibleEquivocation

**Setup.** 3 validators `{A, B, C}`, each bonded with stake 100. A
equivocates by signing two distinct blocks `b₁, b₁'` at seq 5. B
proposes a block at seq 6 with both `b₁` and `b₁'` in its
justifications.

**Trace.**

```
1. sign(A, 5, b₁)     ⟶  D += b₁
2. sign(A, 5, b₁')    ⟶  D += b₁'
3. sign(B, 6, b₂)     ⟶  D += b₂  ; b₂ cites b₁ and b₁' in justifications
4. requestedAsDep(b₁) ⟶ true
5. detect(b₁) = AdmissibleEquivocation
6. record(A, 4)       ⟶ E += (A, 4, ∅)
7. propose(C, [SlashDeploy(b₁', A, ...)])
8. executeSlash(A, true)
   ⟶ allBonds[A] := 0
   ⟶ activeValidators := {B, C}
   ⟶ coopVaultBalance := 100
9. ForkChoice excludes A's latest message from GHOST.
```

**Final state.** A is slashed; bond moved to Coop vault; B and C
continue as the active set.

**Theorems exercised.** T-1, T-2, T-7, T-10. **Diagram 02.**

## 11.2 Two-level slashing (collusion ends in mutual destruction)

**Setup.** 4 validators `{A, B, C, D}`. A equivocates; B colludes
by citing A's equivocation in B's next block without attaching a
SlashDeploy.

**Trace.**

```
1-6: same as 11.1 (A is detected and recorded)
7.  sign(B, 7, bB)   ⟶  D += bB ; bB cites b₁ (the invalid block)
                          ; bB carries no SlashDeploy
8.  detect(bB)       = NeglectedEquivocation  ; (B is recorded too)
9.  record(B, 6)
10. propose(C, [SlashDeploy(b₁', A, ...), SlashDeploy(bB, B, ...)])
11. executeSlash(A, true)
12. executeSlash(B, true)
    ⟶ allBonds[A] = 0, allBonds[B] = 0
    ⟶ activeValidators = {C, D}
    ⟶ coopVaultBalance = 200
```

**This trace exits T-12's precondition.** With n = 4, the BFT
bound is `F = ⌊(n−1)/3⌋ = 1`. After both A and B are slashed,
`|closure| = |{A, B}| = 2 > F = 1`, so T-12's hypothesis
`|closure| ≤ F` does *not* hold. The remaining active set
`{C, D}` is below the quorum lower bound `n − F = 3`. This is a
**counter-example to naive expectations** — it shows what
happens *outside* T-12's domain. The formal treatment is in
[`../slashing-verification.md` §10.8.2](../slashing-verification.md#1082-two-level-slashing-can-liquidate-quorum-if-the-network-is-more-than-f-neglectful).

**Theorems exercised.** T-3, T-6, T-11, T-12 (and its boundary).
**Diagram 04.**

## 11.3 Lock-free tracker race (bug #2 demo)

**Setup.** Two threads `T1` and `T2` simultaneously process two
distinct equivocating blocks `b₁, b₂` by validator A at the same
`(seq, base)`.

**Pre-fix trace** (TLC counter-example from
`MC_ConcurrentTracker.cfg` with `Locked = FALSE`). The race
unfolds in two phases per the §05 storage breakdown.

*Phase a — `handle_invalid_block` (idempotent ∅-insert; not the
race source — both threads write the same value):*

```
T1: insert_equivocation_record(A, sn-1, ∅)  ⟶ store := {(A, sn-1) ↦ ∅}
T2: insert_equivocation_record(A, sn-1, ∅)  ⟶ store := {(A, sn-1) ↦ ∅}
                                                 (idempotent — same value)
```

*Phase b — `update_equivocation_record` (the actual lossy RMW):*

```
T1: equivocation_records()                  ⟶ view1 = {(A, sn-1) ↦ ∅}
T2: equivocation_records()                  ⟶ view2 = {(A, sn-1) ↦ ∅}
                                                 (T1's update not yet visible)
T1: update_equivocation_record(A, sn-1, b₁.hash)
                                            ⟶ store := {(A, sn-1) ↦ {b₁.hash}}
T2: update_equivocation_record(A, sn-1, b₂.hash)
                                            ⟶ store := {(A, sn-1) ↦ {b₂.hash}}
                                                 ↑↑↑ overwrite — b₁.hash lost
                                                 (T2 computed newSet from stale ∅)
```

**Post-fix.** The lock around the Phase-b RMW serializes T1 before
T2; T2's `equivocation_records()` returns
`view2 = {(A, sn-1) ↦ {b₁.hash}}`; the `update_equivocation_record`
call appends `b₂.hash` to the visible set instead of overwriting.
Final state: `{b₁.hash, b₂.hash}`.

**Theorems exercised.** T-4 (record monotonicity), T-9.2.
**Diagram 09.**

## 11.4 PoS transfer failure (bug #4 demo)

**Setup.** A is detected and recorded; B proposes a SlashDeploy;
the `@posVault!("transfer", …)` call fails (e.g. vault deploy quota
exhausted).

**Pre-fix trace.** The `for (_ ← transferDoneCh)` continuation
never fires; A remains in `SlashPending` indefinitely; the next
proposer tries again on B's next block; same failure; etc. The
validator is effectively quarantined but not slashed, with no
closure.

**Post-fix trace.** The alternate continuation fires after a
deterministic timeout, returning `(false, "transfer failed")` on
`returnCh`. A transitions back to `EquivocatorRecorded`. The next
proposer can retry the slash; or, if the failure is persistent (a
misconfigured vault contract), an operator alert fires.

**Theorems exercised.** T-9.4. **Diagram 07.**

## 11.5 Self-correcting block (bug #9 / Rust widening)

**Setup.** A equivocates and is recorded. B proposes a block at seq
7 that **(i)** cites A's invalid block in justifications, AND
**(ii)** carries a `SlashDeploy` targeting A.

**Pre-fix Scala behavior.** B's block is rejected with
`NeglectedInvalidBlock`. Now C must propose another block to slash
A, delaying enforcement.

**Post-fix Rust behavior.** B's block is admitted (the slash
deploy self-corrects the neglect). A is slashed in B's own block.
Liveness is strictly better.

**Theorems exercised.** T-9.9, T-15 (modulo widening). **Diagram 08.**

## 11.6 Stake-0 bonded validator (bug #5 demo)

**Setup.** A's bond is decremented to 0 by some non-slash mechanism
(e.g. a bond withdrawal). A then equivocates.

**Pre-fix.** Detector reaches the
`if stake ≤ 0 then EquivocationDetected` branch in
`equivocation_detector.rs:217`; A is "detected" but never slashed
(zero stake to forfeit) and never recorded. A's equivocation is
invisible to two-level closure.

**Post-fix.** Option (a) — the PoS bond contract enforces
`stake > 0` as an invariant, so the bonded-with-zero state is
unreachable. Option (b) — the detector returns an explicit
`Err(StakeZero)` which the orchestrator logs and skips slashing.

**Theorems exercised.** T-9.5. **Diagram 06.**

## 11.7 Skipped sequence number (bug #7 demo)

**Setup.** A produces blocks at seq 5, 7, 8 (skips seq 6 due to a
partition recovery). Then A equivocates at seq 9.

**Pre-fix.** The detector's lookup uses `baseSeqNum + 1 = 8`; finds
A's block at seq 8 OK; expects to find A's seq 9 block by following
the creator-justification, but the chain has a gap. Detection fails.

**Post-fix.** The canonical visible creator/self-justification walk
handles the gap; detection succeeds. If a later view contains both
seq 9 and seq 10 on the same A-branch, they canonicalize to the same
child root and count once.

**Theorems exercised.** T-9.7. **Diagram 02.**

## 11.8 JustificationRegression dispatched (bug #3 demo)

**Setup.** 3 validators `{A, B, C}`, each bonded with stake 100.
Validator V* (one of A/B/C, say A) signs a block bX at seq 5 whose
creator-justification points back to a strictly older sequence
number than A's known latest message (a *third-party-detected*
justification regression — distinct from the *self*-regression of
bug #6).

**Trace.**

```
1. sign(A, 5, bX)             ⟶ D += bX (regression)
2. validate(bX) = JustificationRegression
3. is_slashable(JustificationRegression) = TRUE

   Pre-fix dispatcher (engine/multi_parent_casper/mod.rs:1090-1099):
4. handle_invalid_block_effect(bX, invalid = true)
   ⟶ DAG marks bX invalid; NO EquivocationRecord;
      A continues with bond intact unless a future proposer
      happens to surface A's invalid latest message.

   Post-fix #3 dispatcher:
4'. insert_equivocation_record(A, 4, ∅)
5'. update_equivocation_record(A, 4, bX.hash)
6'. propose(B, [SlashDeploy(bX, A, ...)])
7'. executeSlash(A, true)
    ⟶ allBonds[A] := 0
    ⟶ activeValidators := {B, C}
    ⟶ coopVaultBalance := 100
```

**Final state.** Pre-fix: A unpunished. Post-fix: A is slashed in
B's next block, mirroring the AdmissibleEquivocation flow. This
example exercises the dispatcher uniformity claim of T-9.3.

The same trace generalizes to every other `is_slashable() = ⊤`
variant (`InvalidBondsCache`, `ContainsExpiredDeploy`,
`ContainsTimeExpiredDeploy`, `InvalidBlockNumber`, etc.) — each
populates an EquivocationRecord under the post-fix dispatcher.

**Theorems exercised.** T-9.3. **Diagram 05.**

## 11.9 Self-regression slips through (bug #6 demo)

**Setup.** 3 validators `{A, B, C}`. Validator A signs a block bN at
seq 7. A then signs a block bM at seq 9 whose creator-justification
cites A's *own* prior block at seq 5 (i.e. `m = 5 < 7 = n`). bM is
*not* an equivocation of bN — A only signed one block at seq 9 —
but bM's chain regresses A's own line.

**Trace.**

```
1. sign(A, 7, bN)                  ⟶ D += bN
2. sign(A, 9, bM)                  ⟶ D += bM ; bM cites A's seq-5 block
                                       (skipping bN in A's chain)
3. validate(bM):
     justification_regressions(bM, snapshot)
       — pre-fix: filterNot(_._1 == bM.sender) skips A's own
         justification (Validate.scala:666); only checks others'
         regressions. Returns FALSE.
     check_equivocations(bM): only one bM at seq 9. Returns FALSE.
     ⟶ bM admitted as Valid.

   Post-fix #6:
3'. justification_regressions(bM, snapshot)
       — fix drops the filterNot: A's own creator-justification
         compared against ds_latest_seq(blocks, A) = 7.
         Cited seq 5 < latest seq 7 ⟹ self-regression detected.
     ⟶ JustificationRegression
4'. dispatcher (post-fix #3) creates EquivocationRecord(A, 8, {bM.hash})
5'. propose(B, [SlashDeploy(bM, A, ...)])
6'. executeSlash(A, true)
    ⟶ allBonds[A] := 0
```

**Final state.** Pre-fix: A's chain inconsistency goes unnoticed —
LMD violations can accumulate. Post-fix: A is detected and slashed.
This example exercises T-9.6 and depends on bug #3's dispatcher
fix to propagate the verdict into the tracker.

**Theorems exercised.** T-9.6, T-9.3. **Diagram 08.**

## 11.10 Unbonded proposer no-emit (bug #8 demo)

**Setup.** 4 validators `{A, B, C, D}`. A was previously slashed
and is no longer in the active set; A's bond = 0. The
proposer-thread scheduler nevertheless picks A as the next proposer
(a corner case that can occur during the next-epoch transition
before the active-set update propagates).

**Trace.**

```
1. propose_thread_scheduler picks A as proposer
2. A.prepare_slashing_deploys(seqM):

   Pre-fix (block_creator.rs:287-332):
3. ilm                  ← dag.invalid_latest_messages ; { (V, bV) }
4. ilm_from_bonded      ← filter (v, _) ∈ ilm where bonds_map[v] > 0
                          ⟶ V kept (V still bonded)
5. slashing_deploys     ← map ilm_from_bonded → SlashDeploy(...)
                          ⟶ Vec of length 1
6. A signs and emits a block bA carrying SlashDeploy(bV, V).
7. Other validators replay bA. The block-validation layer rejects
   bA because the proposer's bond is zero (replay-time
   proposer-bonded check is upstream of `prepare_slashing_deploys`).
   The sys-auth-token check inside the slash deploy itself would
   succeed if reached — the token is unforgeable and bound to the
   system, not to A's identity — but the block itself never gets
   that far.
8. bA is rejected at the block layer; A's CPU and gossip bandwidth
   are wasted preparing it.

   Post-fix #8 (mechanized in Rocq at `BugFixUnbondedProposer.v:44`
   and implemented in Rust):
3'. Guard: if bonds_map[A] = 0 then return Vec::new() ; halt early
4'. A emits no slash-deploys; bA carries no system_deploys
5'. Other validators replay bA cleanly; bA is admitted (or proposer
    rotates and B handles V's slash).
```

**Final state.** Pre-fix: A's block bA is rejected; the slash of V
is delayed by one round; A's proposer-slot wasted. Post-fix: A
short-circuits to no-emit; bA is admitted (modulo unrelated
content); the slash of V proceeds on a subsequent bonded
proposer's block.

**Theorems exercised.** T-9.8, T-AuthCheck. **Diagram 01.**

## 11.11 Withdrawal transfer failure (bug #10 demo)

**Setup.** Validator V is in `state.withdrawers` with
`(bond=50, quarantine=k)`, accumulated rewards 10. The current
block number reaches `k`, so V is now eligible for payout. The
post-quarantine `removeQuarantinedWithdrawers` flow runs:

1. `ListOps!("filter", state.withdrawers.toList(), *isQuarantineFinished, *quarantinedValidatorsCh)`
   produces `[V]`.
2. `ListOps!("unorderedParMap", [V], *payWithdraw, *payRet)` invokes
   `payWithdraw((V, (50, k)), resultCh)`.
3. `payWithdraw` calls `payWithdrawer!((V, 60), *transferResultCh)`,
   which forwards to
   `posVault!("transfer", V_vault, 60, posAuthKey, *transferResultCh)`.
4. **The transfer fails.** `posVault.transfer` emits
   `(false, "insufficient balance")` on `transferResultCh`.

**Pre-fix trace.** `payWithdraw` forwards the
`(false, "insufficient balance")` pair on `resultCh` (the
result-channel naming is the only thing it does — no pattern
match). `for (@_ <- payRet)` accepts any value. `computeRemove`
runs for V unconditionally:
- `state.withdrawers.delete(V)` — V removed from the map.
- `state.committedRewards.delete(V)` — rewards cleared.
- `posBalance` unchanged (the underlying vault rolled back the
  failed transfer at the vault layer).

V has now lost 60 funds. The contract no longer tracks V's
withdrawal entry, so V cannot retry. `total_funds` invariant
violated: `posBalance + Σ payouts_for_withdrawers` decreased by
60.

**Post-fix trace.** `payWithdraw` pattern-matches on
`transferResult`:
- `(true, _) => resultCh!((V, true))` — success
- `(_,    _) => resultCh!((V, false))` — failure

`payRet` collects `[(V, false)]`. `computeRemove` then folds:
- `(V, false) => updatedState` (no mutation)

V remains in `state.withdrawers` with `(50, k)`; rewards intact.
Next block can retry: `removeQuarantinedWithdrawers` runs again,
calls `payWithdraw((V, (50, k)))`, the underlying vault may now
succeed (e.g. balance restored, deploy quota refreshed). On
success the post-fix path removes V correctly with payout. On
persistent failure V stays in `withdrawers` indefinitely until
operator intervention; the `total_funds` invariant remains
preserved across every retry (T-9.10').

**Theorems exercised.** T-9.10, T-9.10', T-9.10″.
**Diagram 11.**

[![Diagram 11 — Bug #10 withdrawal-flow fix (post-fix `payWithdraw` pattern-match + success-gated `computeRemove`)](../diagrams/11-seq-withdrawal-flow-fix.svg)](../diagrams/11-seq-withdrawal-flow-fix.svg)

```
┌──────────────────┐
│ Block at #N+1    │   removeQuarantinedWithdrawers
│ block_number=N+1 │   triggers (N+1 >= k)
└────────┬─────────┘
         │
         ▼
┌─────────────────────────────────────────────────┐
│ ListOps!("filter", withdrawers.toList(), …) →   │
│   quarantinedValidators = [V]                   │
└────────┬────────────────────────────────────────┘
         │ unorderedParMap [V] payWithdraw payRet
         ▼
┌─────────────────────────────────────────────────┐
│ payWithdraw((V, (50, k)), resultCh)             │
│   → payWithdrawer!((V, 60), transferResultCh)   │
│   → posVault!("transfer", V_vault, 60, …)       │
│   ← transferResultCh: (false, "balance-low")    │
└────────┬────────────────────────────────────────┘
         │
   ┌─────┴─────┐
   │           │
   ▼           ▼
 PRE-FIX     POST-FIX
   │           │
   │  ┌────────┴────────────────────────────────┐
   │  │ match transferResult                    │
   │  │   (true, _) → resultCh!((V, true))      │
   │  │   ( _,  _ ) → resultCh!((V, false)) ←── │
   │  └────────┬────────────────────────────────┘
   │           │
   │           ▼
   │  payRet  ←  [(V, false)]
   │           │
   │           │ fold computeRemove
   │           ▼
   │  ┌─────────────────────────────────────────┐
   │  │ (V, false) → updatedState (NO MUTATION) │
   │  │                                         │
   │  │ V STAYS in withdrawers map with (50, k);│
   │  │ rewards intact; total_funds preserved.  │
   │  └────────┬────────────────────────────────┘
   │           │
   │           │ (next block retries)
   │           ▼
   │     ┌─────────────┐
   │     │ T-9.10 + T- │
   │     │ 9.10' hold. │
   │     └─────────────┘
   ▼
 ┌────────────────────────────────────────────┐
 │ payRet ← [(false, "balance-low")]          │
 │ for (@_ <- payRet) — accepts ANY value     │
 │   fold computeRemove [V]                   │
 │     → state.withdrawers.delete(V)          │
 │     → state.committedRewards.delete(V)     │
 │                                            │
 │ posBalance unchanged (vault rolled back).  │
 │ V has LOST 60 funds. No on-chain record.   │
 │ total_funds invariant VIOLATED.            │
 └────────────────────────────────────────────┘
```

## 11.12 Detector traversal — missing pointer + duplicate child (bug #11 demo)

**Setup.** Validator A signs two distinct blocks `b₁, b₁'` at the
same `(seq=10)`, then proposes `b₂` whose justification cites both.
A third block `b₃` cites `b₂`. Validator B observes `b₃` and must
classify A.

**Pre-fix trace** (TLC counter-example from a hypothetical
`MC_DetectorPartial.cfg`; in practice replayed deterministically by
`uc_101..uc_108`):

```
B.detect(view = {A ↦ b₂})
  fold visit b₂                ⟶ direct ptr = b₁ (present)
                                 nested ptr = b₁' (MISSING in view's HashMap)
  unsafe lookup nested ptr     ⟶ panics / aborts with KeyNotFound
  detection result             ⟶ NONE  (decisive evidence hidden)
```

A second adversarial shape — *duplicate child path* — causes a false
positive:

```
B.detect(view = {A ↦ b₂, A ↦ b₂})   -- duplicate justification entry, pre-fix accepted
  fold over Vec<JustificationEntry>  ⟶ counts b₂.hash twice
  |distinct child hashes|            ⟶ "2" (wrongly)
  detection result                   ⟶ NeglectedEquivocation (false positive)
```

**Post-fix trace.** Detector projects into a deterministic
validator → option<hash> map; missing nested pointers contribute `∅`;
duplicate child paths collapse to one entry:

```
B.detect(view ↾ canonical)
  project view                       ⟶ map = {A ↦ Some(b₂.hash)}
  scan iteratively                   ⟶ missing nested ptr = ∅  (no panic)
                                     ⟶ distinct child hashes = {b₂.hash}  (count 1)
  detection result                   ⟶ NONE — correctly, no neglect
                                       (decisive evidence needs 2 distinct children
                                        OR a detected_hash_seen verdict)
```

**Theorems exercised.** T-9.11 family
(`fixed_detectable_missing_pointer_prefix`,
`fixed_detectable_duplicate_single_child_false`,
`fixed_detectable_two_distinct_children_true`,
`fixed_detectable_detected_hash_true`). TLA+
`Inv_FixedDetectorTotal`, `Inv_MissingPointerNonContributing`,
`Inv_DuplicateChildNeedsDistinctChildren`. Rust UC-101..UC-108.
**Diagram.** Extends Diagram 02 with the deterministic-projection
guard.

## 11.13 Unauthorized slash deploy rejected pre-replay (bug #12 demo)

**Setup.** A Byzantine block sender `Z` includes a `SlashDeploy`
targeting honest validator `H` in a block `b`. The local node has
no invalid-block evidence against `H`.

**Pre-fix trace.**

```
Z.propose(b, [SlashDeploy(target=H, evidence_hash=fake)])
B.replay(b)
  evaluate Rholang body
  ⟶ @PoS!("slash", H, …)            -- replay runtime fires the deploy
  ⟶ slash(ps, H)                     -- PoS state mutates: bonds[H] := 0
                                       active := active \ {H}
                                       coopVault += stake_of(H)
                                     -- H is slashed by Z's unilateral choice
```

**Post-fix trace.** Pre-replay authorization filter intercepts:

```
Z.propose(b, [SlashDeploy(target=H, evidence_hash=fake)])
B.replay(b)
  authorize_slash_deploy(local_view, b.sender, deploy)
    issuer matches block sender?         ⟶ Z = Z  ✓
    evidence_hash ∈ local.invalid_blocks?⟶ fake ∉ I  ✗ — REJECT
  ⟶ deploy ignored; slash NOT fired
```

**Theorems exercised.** T-9.13 (`main_T9_13_unknown_slash_evidence_noop`).
TLA+ `Inv_RejectedSlashWithoutEvidenceNoPending`. Rust
`slash_authorization_regressions`.  **Diagram.** Extends Diagram 05
with the authorize-before-replay guard.

## 11.14 Stale-evidence rebond identity (bug #13 demo)

**Setup.** Validator with public key `K` bonds at epoch `e₁=5`,
equivocates, is slashed, unbonds. Later at epoch `e₂=12` the same key
rebonds. Adversarial proposer `Z` tries to slash `(K, e₂)` by
replaying the still-retained evidence from `(K, e₁)`.

**Pre-fix trace.**

```
Z.propose(b, [SlashDeploy(target=K, evidence=eq_record_e1)])
B.replay(b)
  -- pre-fix: authorization checked key identity only
  K is currently bonded?           ⟶ yes (rebonded at e₂)
  evidence references K?           ⟶ yes (eq_record_e1.target = K)
  ⟶ slash(ps, K)                    -- second slash on innocent (K, e₂)
```

**Post-fix trace.** Authorization is epoch-scoped:

```
Z.propose(b, [SlashDeploy(target=K, evidence=eq_record_e1)])
B.replay(b)
  authorize_slash_deploy(local_view, deploy)
    eq_record_e1.epoch == current_epoch(K)?  ⟶ e₁ ≠ e₂  ✗ — REJECT
  ⟶ deploy ignored; (K, e₂) retains bond
```

**Theorems exercised.** T-9.12 (`main_T9_12_stale_evidence_not_authorized`).
TLA+ `Inv_StaleEvidenceCannotSlashRebondedKey`. Rust
`stale_invalid_evidence_is_not_an_authorized_slash_candidate`.
**Diagram.** Extends Diagram 06 (validator lifecycle) with epoch
tags on each lifetime.

## 11.15 Authorized-invalid-block evidence index (bug #14 demo)

**Setup.** Validator A produces an invalid block `b_inv` at
sequence 7. The DAG inserts `b_inv` with `invalid = true`. The
proposer `B` must decide whether to emit a slash deploy at the next
proposing round.

**Pre-fix trace.** Slash candidates derived from
`invalid_latest_messages()`:

```
B.prepare_slashing_deploys()
  view ← dag.invalid_latest_messages()
  -- invalid_latest_messages reflects only blocks that updated the
     latest-message index; `b_inv` did not (its insertion took the
     "insert-as-invalid" code path)
  candidates                           ⟶ ∅
  emit nothing                         ⟶ liveness gap: A keeps the bond
                                          despite a detected invalid block
```

**Post-fix trace.** Candidates derived from the authorized-invalid-block
evidence index:

```
B.prepare_slashing_deploys()
  view ← dag.authorized_invalid_evidence_index(current_epoch)
  view contains b_inv.hash for offender A  ✓
  -- canonicalize: sort by hash, deduplicate per (offender, epoch)
  candidates                              ⟶ [SlashDeploy(A, b_inv.hash)]
  emit                                    ⟶ slash deploy enters block
```

**Theorems exercised.** T-LivenessGap (`deploy_epoch_matches_target`).
TLA+ `Inv_NoInvalidLatestLivenessGap`. Rust
`current_epoch_invalid_evidence_is_authorized_once_per_offender`.
**Diagram.** Extends Diagram 08 with the new index path.

## 11.16 Checked sequence arithmetic at boundary (bug #15 demo)

**Setup.** Freshly-bonded validator C produces its first equivocating
pair `(b₀, b₀')` at `seq = 0`. Pre-fix, the record-insertion code
computes `baseSeq := seq − 1 = -1` and panics or wraps. Symmetrically,
proposer D at `seq = i32::MAX − 1` attempts `seq + 1` which overflows.

**Pre-fix trace (genesis side).**

```
detector → insert_equivocation_record(C, baseSeq = seq − 1, …)
  seq = 0 (signed) →           seq − 1 = -1  (underflow)
                  (unsigned) →   seq − 1 = usize::MAX  (wrap)
  key (C, baseSeq) is malformed
  store insert silently malfunctions: collision or rejection
```

**Pre-fix trace (overflow side).**

```
proposer → next_block.seq := prev.seq + 1
  prev.seq = i32::MAX           prev.seq + 1 = -i32::MAX − 1  (wrap to negative)
  block_creator emits block with negative seq
  downstream validate rejects it (or worse, accepts it depending on path)
```

**Post-fix trace.**

```
detector → insert_equivocation_record(C, baseSeq = checked_pred(seq), …)
  seq = 0 → checked_pred returns None → record-insert rejects nonpositive domain
                                        evidence still indexed via authorized
                                        invalid-block evidence index (bug #14 fix)
proposer → next_block.seq := checked_succ(prev.seq, i32::MAX)
  prev.seq = i32::MAX           checked_succ returns None → proposer halts
                                                            with a clean error
```

**Theorems exercised.** T-9.14 (`main_T9_14_checked_pred_positive`),
`checked_pred_total_positive`, `checked_succ_bounded_sound`. Rust
`checked_sequence_arithmetic_rejects_boundaries`.  **Diagram.** None
(local arithmetic fix).

## 11.17 Duplicate-justification rejection before projection (bug #16 demo)

**Setup.** Block `b` contains two justification entries naming the
same validator A with different cited hashes `h₁, h₂`. Pre-fix
validation accepts the block; the detector projects into a
`HashMap<V, H>` keyed by validator and silently loses one of the two
entries depending on hash-iteration order.

**Pre-fix trace.**

```
validate(b)
  justifications.iter().map(.validator).collect::<HashSet>().len()
                                            == justifications.len()?
  -- set semantics over validator identity
                                            ⟶ {A}.len() == 2  ✗  -- BUT
  -- pre-fix the check was different; the validity gate was set-based
  -- and accepted A appearing twice
  validation                                ⟶ ACCEPT

detector.project(b.justifications)
  HashMap<V, H> = {A → h₁}      -- first insert
  HashMap<V, H> = {A → h₂}      -- second insert overwrites
                                  (or the reverse, depending on iter order)
  ⟶ projection cardinality 1; adversary chose which hash detector sees
```

**Post-fix trace.** Validation enforces list cardinality:

```
validate(b)
  if justifications.len() != justifications.iter().map(.validator)
                                            .collect::<HashSet>().len()
  ⟶ REJECT b as invalid (DuplicateJustification)
  ⟶ b is recorded as invalid; if its sender is slashable, the dispatch
     catch-all (bug #3 fix) inserts an equivocation record
```

**Theorems exercised.** T-9.15 (`main_T9_15_duplicate_justifications_rejected`),
`duplicate_head_rejected`. TLA+ `Inv_DuplicateJustificationsRejected`,
`Inv_AcceptedProjectionCardinality`. Rust
`duplicate_justification_validators_are_invalid`.  **Diagram.**
Extends Diagram 08 with a validation guard before detector projection.

## 11.18 Cross-example summary

| Example | Bug exercised             | Bisimilarity impact           | Diagram(s) |
|---------|---------------------------|-------------------------------|------------|
| 11.1    | None (baseline)           | Preserving                    | 02         |
| 11.2    | None (boundary)           | Preserving (BFT bound exited) | 04         |
| 11.3    | #2 (race)                 | Preserving                    | 09         |
| 11.4    | #4 (slash transfer)       | Preserving                    | 07         |
| 11.5    | #9 (widening)             | **Deliberate widening**       | 08         |
| 11.6    | #5 (stake-0)              | Preserving                    | 06         |
| 11.7    | #7 (off-by-one)           | Permitted bug-fix delta       | 02         |
| 11.8    | #3 (dispatcher)           | Preserving                    | 05         |
| 11.9    | #6 (self-regression) + #3 | Preserving                    | 08         |
| 11.10   | #8 (unbonded proposer)    | Preserving                    | 01         |
| 11.11   | #10 (withdraw transfer)   | Preserving                    | 11         |
| 11.12   | #11 (detector partial)    | Permitted bug fix             | 02 (extended) |
| 11.13   | #12 (unauthorized slash)  | Permitted bug fix             | 05 (extended) |
| 11.14   | #13 (stale-rebond)        | Permitted bug fix             | 06 (extended) |
| 11.15   | #14 (liveness-gap)        | Permitted bug fix             | 08 (extended) |
| 11.16   | #15 (checked arithmetic)  | Permitted bug fix             | —             |
| 11.17   | #16 (duplicate justif.)   | Permitted bug fix             | 08 (extended) |

Of the **seventeen worked examples**, fifteen are
*bisimilarity-preserving* or *permitted bug-fix deltas*, and one
(11.5) is the *deliberate widening* documented in §10. The §11.7
worked example is also a permitted bug-fix delta (non-dense
sequence numbers, same-branch canonicalization).

---

**Next:** [§12 — Failure modes & recovery](12-failure-modes.md)
