(* ═══════════════════════════════════════════════════════════════════════════
   PoSContract.v — The on-chain Proof-of-Stake slash transition

   Models the slash method of the PoS Rholang contract at
     casper/src/main/resources/PoS.rhox:432-495
   abstractly as a state-transition function on an idealized PoSState.

   Theorems:
     T-7 (slash_zeros_bond)        — successful slash zeros the bond
     T-8 (slash_transfers_stake)   — successful slash transfers to Coop vault
     T-9 (slash_idempotent)        — second slash is a no-op

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition       │ Rholang/Paper                │ Rust Implementation
   ──────────────────────┼──────────────────────────────┼─────────────────────
   PoSState              │ state in PoS.rhox            │ on-chain Rholang state
   ps_allBonds           │ state.allBonds : V → ℕ       │ same
   ps_active             │ state.activeValidators       │ same
   ps_coopVault          │ posVault balance              │ same
   slash                 │ @PoS!("slash", …)            │ SlashDeploy → invokes contract
   ─────────────────────────────────────────────────────────────────────────

   Companion doc: slashing-verification.md §6.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — PoS state record
   ═══════════════════════════════════════════════════════════════════════════ *)

Record PoSState : Type := mkPoSState {
  ps_allBonds  : BondMap;
  ps_active    : list Validator;       (* set, modeled as list *)
  ps_coopVault : nat
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — The slash transition
   ═══════════════════════════════════════════════════════════════════════════

   The transition takes a PoSState and an offender, and returns the new
   state plus a Boolean success flag.

   - If the offender's bond is already 0, slash is a no-op (idempotence).
   - Otherwise, the bond is moved to the Coop vault, the offender is
     removed from active, and the bond map is zeroed.

   We assume the auth-token check has already passed at this entry point. *)

Definition slash (ps : PoSState) (v : Validator) : PoSState * bool :=
  let bond := bm_lookup (ps_allBonds ps) v in
  if Nat.eq_dec bond 0
  then (ps, true)  (* idempotent no-op *)
  else
    (mkPoSState
       (bm_slash (ps_allBonds ps) v)
       (filter (fun v' => if validator_eq_dec v' v then false else true)
               (ps_active ps))
       (ps_coopVault ps + bond),
     true).

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — T-7: slash zeros bond
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_zeros_bond :
  forall ps v,
    let (ps', _) := slash ps v in
    bm_lookup (ps_allBonds ps') v = 0.
Proof.
  intros ps v.
  unfold slash.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE].
  - simpl. assumption.
  - simpl. apply bm_slash_lookup.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — T-8: slash transfers stake to Coop vault
   ═══════════════════════════════════════════════════════════════════════════

   When the offender's bond is positive, the Coop vault balance increases
   by exactly the offender's pre-slash bond. (For a stake-0 offender,
   slash is a no-op and the vault is unchanged.) *)

Theorem slash_transfers_stake :
  forall ps v,
    let bond := bm_lookup (ps_allBonds ps) v in
    let (ps', _) := slash ps v in
    bond > 0 ->
    ps_coopVault ps' = ps_coopVault ps + bond.
Proof.
  intros ps v.
  simpl.
  unfold slash.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl.
  - intro H. lia.
  - intro H. reflexivity.
Qed.

Theorem slash_zero_bond_noop :
  forall ps v,
    bm_lookup (ps_allBonds ps) v = 0 ->
    slash ps v = (ps, true).
Proof.
  intros ps v H.
  unfold slash.
  rewrite H.
  destruct (Nat.eq_dec 0 0) as [_ | Hneq]; [reflexivity | contradiction].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — T-9: slash idempotence
   ═══════════════════════════════════════════════════════════════════════════

   Slashing the same validator twice yields the same state as slashing
   once. *)

Theorem slash_idempotent :
  forall ps v,
    let (ps1, _)  := slash ps  v in
    let (ps2, _)  := slash ps1 v in
    ps_allBonds  ps2 = ps_allBonds  ps1
    /\ ps_coopVault ps2 = ps_coopVault ps1
    /\ ps_active   ps2 = ps_active   ps1.
Proof.
  intros ps v.
  unfold slash at 1.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0) as [E | NE]; simpl.
  - (* First slash is a no-op; second is also no-op since bond still 0. *)
    unfold slash. simpl.
    rewrite E. simpl. repeat split; reflexivity.
  - (* First slash sets bond to 0; second is a no-op since lookup is now 0. *)
    unfold slash. simpl.
    rewrite bm_slash_lookup. simpl. repeat split; reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Slash leaves other validators' bonds unchanged
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slash_other_unchanged :
  forall ps v v',
    v <> v' ->
    let (ps', _) := slash ps v in
    bm_lookup (ps_allBonds ps') v' = bm_lookup (ps_allBonds ps) v'.
Proof.
  intros ps v v' Hne.
  unfold slash.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds ps) v) 0); simpl.
  - reflexivity.
  - apply bm_slash_other. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Stage-C two-effect slash + redemption state (ADDITIVE)
   ═══════════════════════════════════════════════════════════════════════════

   Cost-Accounted Rho Stage C (DR-3 / DR-7;
   docs/theory/cost-accounting-impl/workstream-c-economic.md "Stage C",
   stageb-minting-halt-interface.md Decision 4). The LANDED Rholang `slash`
   (casper/src/main/resources/PoS.rhox:784-889) replaced the legacy "transfer
   the forfeited stake to the Coop vault" effect with a TWO-EFFECT slash:

     (i)   the offender is added to a cross-epoch mint-halt set "mintingHalted"
           (the Stage-B epoch-mint fold skips any v ∈ mintingHalted);
     (ii)  the offender's stake is EARMARKED on a per-offender quarantine
           ("quarantinedStake") pending `redeemSlashed` adjudication — the Coop
           vault is LEFT UNCHANGED (no transfer here; coop grows ONLY in the
           Guilty redemption branch).

   This is an ADDITIVE embedding (the BugFixWithdrawTransferFailure.v `PoSStateW`
   pattern): the legacy `PoSState` / `slash` / T-7 / T-8 (`slash_transfers_stake`)
   / T-9 above are kept VERBATIM so the slashing-tree headline `MainTheorem.v`
   (which consumes them, incl. the coop-transfer clause) stays green and is NOT
   touched. The Stage-C two-effect transition `slashC` operates on a NEW state
   record `PoSStateC` that embeds `PoSState` as `psc_pos` and adds the two new
   sets. `ValidatorRedemption.v` (the redemption adjudication) builds on this. *)

(* ─────────────────────────────────────────────────────────────────────────
   §7.1 — Minting-halt set (Set[PublicKey], modeled as a list with decidable
   membership). Mirrors PoS state key "mintingHalted".
   ───────────────────────────────────────────────────────────────────────── *)

Definition HaltSet := list Validator.

Definition halted_mem (hs : HaltSet) (v : Validator) : bool :=
  if in_dec validator_eq_dec v hs then true else false.

Definition halted_add (hs : HaltSet) (v : Validator) : HaltSet :=
  v :: hs.

Definition halted_remove (hs : HaltSet) (v : Validator) : HaltSet :=
  filter (fun v' => if validator_eq_dec v' v then false else true) hs.

Lemma halted_mem_add_same :
  forall hs v, halted_mem (halted_add hs v) v = true.
Proof.
  intros hs v. unfold halted_mem, halted_add.
  destruct (in_dec validator_eq_dec v (v :: hs)) as [_ | Hni].
  - reflexivity.
  - exfalso. apply Hni. left. reflexivity.
Qed.

Lemma halted_mem_add_other :
  forall hs v u, u <> v -> halted_mem (halted_add hs v) u = halted_mem hs u.
Proof.
  intros hs v u Hne. unfold halted_mem, halted_add.
  destruct (in_dec validator_eq_dec u (v :: hs)) as [Hin | Hni];
  destruct (in_dec validator_eq_dec u hs) as [Hin' | Hni']; try reflexivity.
  - destruct Hin as [Heq | Hin].
    + exfalso. apply Hne. symmetry. exact Heq.
    + contradiction.
  - exfalso. apply Hni. right. assumption.
Qed.

Lemma halted_mem_remove_same :
  forall hs v, halted_mem (halted_remove hs v) v = false.
Proof.
  intros hs v. unfold halted_mem, halted_remove.
  destruct (in_dec validator_eq_dec v
              (filter (fun v' => if validator_eq_dec v' v then false else true) hs))
    as [Hin | Hni].
  - exfalso. apply filter_In in Hin as [_ Hpred].
    destruct (validator_eq_dec v v) as [_ | Hneq]; [discriminate | apply Hneq; reflexivity].
  - reflexivity.
Qed.

Lemma halted_mem_remove_other :
  forall hs v u, u <> v -> halted_mem (halted_remove hs v) u = halted_mem hs u.
Proof.
  intros hs v u Hne. unfold halted_mem, halted_remove.
  destruct (in_dec validator_eq_dec u
              (filter (fun v' => if validator_eq_dec v' v then false else true) hs))
    as [Hin | Hni];
  destruct (in_dec validator_eq_dec u hs) as [Hin' | Hni']; try reflexivity.
  - apply filter_In in Hin as [Hin _]. contradiction.
  - exfalso. apply Hni. apply filter_In. split; [assumption |].
    destruct (validator_eq_dec u v) as [Heq | _]; [contradiction | reflexivity].
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §7.2 — Quarantine map (Map[PublicKey, Int], the earmarked pre-slash bond).
   Mirrors PoS state key "quarantinedStake". Assoc list keyed by the offender;
   `qs_lookup` returns the first binding (slash prepends, so freshest wins).
   ───────────────────────────────────────────────────────────────────────── *)

Definition QuarantineMap := list (Validator * nat).

Fixpoint qs_lookup (qs : QuarantineMap) (v : Validator) : option nat :=
  match qs with
  | []            => None
  | (k, n) :: rest =>
      if validator_eq_dec k v then Some n else qs_lookup rest v
  end.

Definition qs_insert (qs : QuarantineMap) (v : Validator) (n : nat) : QuarantineMap :=
  (v, n) :: qs.

Fixpoint qs_remove (qs : QuarantineMap) (v : Validator) : QuarantineMap :=
  match qs with
  | []            => []
  | (k, n) :: rest =>
      if validator_eq_dec k v then qs_remove rest v else (k, n) :: qs_remove rest v
  end.

Lemma qs_lookup_insert_same :
  forall qs v n, qs_lookup (qs_insert qs v n) v = Some n.
Proof.
  intros qs v n. unfold qs_insert. simpl.
  destruct (validator_eq_dec v v) as [_ | H]; [reflexivity | contradiction].
Qed.

Lemma qs_lookup_insert_other :
  forall qs v u n, u <> v -> qs_lookup (qs_insert qs v n) u = qs_lookup qs u.
Proof.
  intros qs v u n Hne. unfold qs_insert. simpl.
  destruct (validator_eq_dec v u) as [Heq | _]; [exfalso; apply Hne; symmetry; exact Heq | reflexivity].
Qed.

Lemma qs_in_insert_same :
  forall qs v n, In (v, n) (qs_insert qs v n).
Proof.
  intros qs v n. unfold qs_insert. left. reflexivity.
Qed.

Lemma qs_lookup_remove_same :
  forall qs v, qs_lookup (qs_remove qs v) v = None.
Proof.
  induction qs as [| [k n] rest IH]; intros v; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Heq | Hne].
    + apply IH.
    + simpl. destruct (validator_eq_dec k v) as [E | _]; [contradiction | apply IH].
Qed.

Lemma qs_remove_other :
  forall qs v u, u <> v -> qs_lookup (qs_remove qs v) u = qs_lookup qs u.
Proof.
  induction qs as [| [k n] rest IH]; intros v u Hne; simpl.
  - reflexivity.
  - destruct (validator_eq_dec k v) as [Hkv | Hkv].
    + subst k. destruct (validator_eq_dec v u) as [Hvu | Hvu].
      * exfalso. apply Hne. symmetry. exact Hvu.
      * apply IH; assumption.
    + simpl. destruct (validator_eq_dec k u) as [Hku | Hku].
      * reflexivity.
      * apply IH; assumption.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §7.3 — Stage-C PoS state and the two-effect slash transition
   ───────────────────────────────────────────────────────────────────────── *)

Record PoSStateC : Type := mkPoSStateC {
  psc_pos          : PoSState;       (* embedded legacy state *)
  psc_mintingHalted : HaltSet;       (* "mintingHalted" : Set[PublicKey] *)
  psc_quarantined  : QuarantineMap   (* "quarantinedStake" : Map[Pk,(Int,_)] *)
}.

(* The Stage-C two-effect slash. We assume the auth-token check has passed at
   entry (as for `slash`). For an offender already at bond 0 (already slashed),
   `slashC` is the identity no-op (idempotent) — this also makes the halt /
   quarantine inserts first-slash-only, matching the Rholang `valBond <= 0`
   guard. Otherwise: zero the bond, remove from active, COOP UNCHANGED, add to
   `mintingHalted`, and earmark `(v, bond)` on the quarantine. *)
Definition slashC (psc : PoSStateC) (v : Validator) : PoSStateC * bool :=
  let ps := psc_pos psc in
  let bond := bm_lookup (ps_allBonds ps) v in
  if Nat.eq_dec bond 0
  then (psc, true)  (* idempotent no-op *)
  else
    (mkPoSStateC
       (mkPoSState
          (bm_slash (ps_allBonds ps) v)
          (filter (fun v' => if validator_eq_dec v' v then false else true)
                  (ps_active ps))
          (ps_coopVault ps))                              (* coop UNCHANGED *)
       (halted_add (psc_mintingHalted psc) v)
       (qs_insert (psc_quarantined psc) v bond),
     true).

(* ═══════════════════════════════════════════════════════════════════════════
   §8 — T-7C: slashC zeros the offender's bond (embedded state)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slashC_zeros_bond :
  forall psc v,
    let (psc', _) := slashC psc v in
    bm_lookup (ps_allBonds (psc_pos psc')) v = 0.
Proof.
  intros psc v. unfold slashC.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE]; simpl.
  - assumption.
  - apply bm_slash_lookup.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §9 — T-8C: slashC quarantines the stake and leaves the Coop vault UNCHANGED
   ═══════════════════════════════════════════════════════════════════════════

   The defining contrast with the legacy `slash_transfers_stake`: a positive-bond
   slash records `(v, bond)` on the quarantine AND the Coop vault is unchanged
   (no transfer). Coop grows ONLY in the Guilty redemption branch
   (ValidatorRedemption.v `redeem_guilty_redistributes`). *)

Theorem slash_quarantines_stake :
  forall psc v,
    let bond := bm_lookup (ps_allBonds (psc_pos psc)) v in
    let (psc', _) := slashC psc v in
    bond > 0 ->
    qs_lookup (psc_quarantined psc') v = Some bond
    /\ ps_coopVault (psc_pos psc') = ps_coopVault (psc_pos psc).
Proof.
  intros psc v. simpl. unfold slashC.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE]; simpl.
  - intro H. lia.
  - intro H. split.
    + apply qs_lookup_insert_same.
    + reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §10 — slashC halts the offender (positive-bond slash)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slashC_halts :
  forall psc v,
    let bond := bm_lookup (ps_allBonds (psc_pos psc)) v in
    let (psc', _) := slashC psc v in
    bond > 0 ->
    halted_mem (psc_mintingHalted psc') v = true.
Proof.
  intros psc v. simpl. unfold slashC.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE]; simpl.
  - intro H. lia.
  - intro H. apply halted_mem_add_same.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §11 — T-IdemC: slashC idempotence (incl. halted / quarantined stable on the
   second slash)
   ═══════════════════════════════════════════════════════════════════════════

   The first slash drives the offender's bond to 0; the second therefore takes
   the no-op branch and is the identity, so the embedded bonds / coop / active
   AND the new halted / quarantined sets are all unchanged by the second
   slash — a re-slash does NOT double-halt or double-quarantine. *)

Theorem slashC_idempotent :
  forall psc v,
    let (psc1, _) := slashC psc  v in
    let (psc2, _) := slashC psc1 v in
    ps_allBonds  (psc_pos psc2)  = ps_allBonds  (psc_pos psc1)
    /\ ps_coopVault (psc_pos psc2) = ps_coopVault (psc_pos psc1)
    /\ ps_active   (psc_pos psc2)  = ps_active   (psc_pos psc1)
    /\ psc_mintingHalted psc2      = psc_mintingHalted psc1
    /\ psc_quarantined  psc2       = psc_quarantined  psc1.
Proof.
  intros psc v.
  unfold slashC at 1.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE]; simpl.
  - (* First slash is a no-op; the second sees bond still 0, also a no-op. *)
    unfold slashC. simpl. rewrite E. simpl. repeat split; reflexivity.
  - (* First slash zeros the bond; the second takes the no-op branch. *)
    unfold slashC. simpl. rewrite bm_slash_lookup. simpl. repeat split; reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §12 — slashC leaves other validators' bonds / halt / quarantine unchanged
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem slashC_other_unchanged :
  forall psc v v',
    v <> v' ->
    let (psc', _) := slashC psc v in
    bm_lookup (ps_allBonds (psc_pos psc')) v' = bm_lookup (ps_allBonds (psc_pos psc)) v'
    /\ halted_mem (psc_mintingHalted psc') v' = halted_mem (psc_mintingHalted psc) v'
    /\ qs_lookup (psc_quarantined psc') v' = qs_lookup (psc_quarantined psc) v'.
Proof.
  intros psc v v' Hne. unfold slashC.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE]; simpl.
  - repeat split; reflexivity.
  - repeat split.
    + apply bm_slash_other. assumption.
    + apply halted_mem_add_other. apply not_eq_sym. assumption.
    + apply qs_lookup_insert_other. apply not_eq_sym. assumption.
Qed.
