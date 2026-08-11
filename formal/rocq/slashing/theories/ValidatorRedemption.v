(* ═══════════════════════════════════════════════════════════════════════════
   ValidatorRedemption.v — Stage-C validator redemption (adjudication)

   Models the on-chain `redeemSlashed` Rholang contract at
     casper/src/main/resources/PoS.rhox:930-1073
   as a state-transition function on the Stage-C two-effect PoS state
   `PoSStateC` (PoSContract.v §7), the governance-triggered adjudication of a
   quarantined (slashed) validator.

   Cost-Accounted Rho Stage C / DR-3 / DR-7 / DR-12
   (docs/theory/cost-accounting-impl/workstream-c-economic.md "Stage C",
    stageb-minting-halt-interface.md Decision 4). The Rholang `redeemSlashed`
   is DOUBLE-gated (sysAuthToken AND a Rust-verified PoS-multisig quorum). The
   quorum verification is a Rust platform obligation (redeem_deploy.rs
   `verify_multisig_quorum`, DR-12); here we model its verdict as the Boolean
   `authorized` — a false verdict (under-quorum / unauthorized) rejects with NO
   state change. Three outcomes (DR-3 two-effect adjudication):

     Vindicated      — innocent: restore the full quarantined bond to active
                       stake, remove from "mintingHalted" (UN-HALT), clear the
                       quarantine; coop unchanged. (This file's
                       [redeem_vindicated_restores] is the formal anchor for the
                       Stage-C un-halt fix verified end-to-end by the Rust test
                       `redeem_outcomes_and_multisig_gate`.)
     Guilty penalty  — partial: move min(penalty, bond) to the Coop vault (the
                       ONLY coop-growth path) and restore the remainder; un-halt
                       + clear quarantine.
     Burned          — total: destroy the quarantined stake (REV stays in
                       posVault as protocol surplus); the validator STAYS
                       unbonded AND halted (NOT removed from "mintingHalted");
                       only the quarantine record is cleared.

   ─────────────────────────────────────────────────────────────────────────
   INDEPENDENCE (G-coordination). This module imports ONLY Validator and
   PoSContract (+ Stdlib). It is INDEPENDENT of MainTheorem.v (the slashing-tree
   headline) and is NOT added to its composition — the legacy `PoSState`/`slash`
   and the Stage-C `PoSStateC`/`slashC`/`redeem` coexist additively. Everything
   is concrete (no axioms / Section hypotheses): [Print Assumptions] of each
   theorem reports "Closed under the global context".

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq                              │ Rholang `redeemSlashed`
   ───────────────────────────────────┼──────────────────────────────────────
   redeem _ _ Vindicated true         │ ("Vindicated", _) branch (restore+unhalt)
   redeem _ _ (Guilty p) true         │ ("Guilty", penalty) branch (coop+restore)
   redeem _ _ Burned true             │ ("Burned", _) branch (destroy, stay halted)
   authorized = false                 │ ¬(isValid and multiSigVerified) ⇒ reject
   qs_lookup … = None ⇒ reject        │ ¬quarantinedStake.contains(v) ⇒ reject
   ───────────────────────────────────┴──────────────────────────────────────

   Companion docs: workstream-c-economic.md "Stage C"; cost-accounting
   threat/use-case rows TM-CA-157 / UC-CA-155/156.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
From Slashing Require Import Validator PoSContract.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Adjudication outcomes and the redemption transition
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive RedemptionOutcome : Type :=
  | Vindicated
  | Guilty (penalty : nat)
  | Burned.

(* The redemption transition. `authorized` is the Rust DR-12 multisig-quorum
   verdict (true iff a quorum of the configured PoS-multisig keyset authorized
   THIS (validator, outcome)). Redemption REQUIRES (i) authorization AND (ii) an
   active quarantine record; otherwise it rejects with NO state change. *)
Definition redeem
  (psc : PoSStateC) (v : Validator) (o : RedemptionOutcome) (authorized : bool)
  : PoSStateC * bool :=
  if authorized then
    match qs_lookup (psc_quarantined psc) v with
    | None      => (psc, false)   (* not quarantined ⇒ nothing to adjudicate *)
    | Some bond =>
        let ps := psc_pos psc in
        match o with
        | Vindicated =>
            (mkPoSStateC
               (mkPoSState
                  (bm_update (ps_allBonds ps) v bond)      (* restore full bond *)
                  (v :: ps_active ps)                      (* re-activate *)
                  (ps_coopVault ps))                       (* coop unchanged *)
               (halted_remove (psc_mintingHalted psc) v)   (* UN-HALT *)
               (qs_remove (psc_quarantined psc) v),        (* clear quarantine *)
             true)
        | Guilty penalty =>
            let p := Nat.min penalty bond in               (* clamp to [0,bond] *)
            (mkPoSStateC
               (mkPoSState
                  (bm_update (ps_allBonds ps) v (bond - p)) (* restore remainder *)
                  (v :: ps_active ps)
                  (ps_coopVault ps + p))                    (* coop GROWS by p *)
               (halted_remove (psc_mintingHalted psc) v)    (* UN-HALT *)
               (qs_remove (psc_quarantined psc) v),
             true)
        | Burned =>
            (mkPoSStateC
               (mkPoSState
                  (ps_allBonds ps)                          (* bond stays 0 *)
                  (ps_active ps)                            (* stays inactive *)
                  (ps_coopVault ps))                        (* coop unchanged *)
               (psc_mintingHalted psc)                      (* STAYS halted *)
               (qs_remove (psc_quarantined psc) v),         (* clear quarantine *)
             true)
        end
    end
  else (psc, false).

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Vindicated restores the bond AND un-halts (the bug-fix anchor)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem redeem_vindicated_restores :
  forall psc v bond,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    let (psc', ok) := redeem psc v Vindicated true in
    ok = true
    /\ bm_lookup (ps_allBonds (psc_pos psc')) v = bond
    /\ halted_mem (psc_mintingHalted psc') v = false
    /\ qs_lookup (psc_quarantined psc') v = None.
Proof.
  intros psc v bond Hq. unfold redeem. rewrite Hq. simpl.
  repeat split.
  - apply bm_lookup_update_same.
  - apply halted_mem_remove_same.
  - apply qs_lookup_remove_same.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Guilty redistributes (coop grows by exactly the clamped penalty)
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem redeem_guilty_redistributes :
  forall psc v bond penalty,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    let (psc', ok) := redeem psc v (Guilty penalty) true in
    ok = true
    /\ ps_coopVault (psc_pos psc') = ps_coopVault (psc_pos psc) + Nat.min penalty bond
    /\ bm_lookup (ps_allBonds (psc_pos psc')) v = bond - Nat.min penalty bond
    /\ halted_mem (psc_mintingHalted psc') v = false
    /\ qs_lookup (psc_quarantined psc') v = None.
Proof.
  intros psc v bond penalty Hq. unfold redeem. rewrite Hq. simpl.
  repeat split.
  - apply bm_lookup_update_same.
  - apply halted_mem_remove_same.
  - apply qs_lookup_remove_same.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Burned destroys stake and KEEPS the validator halted
   ═══════════════════════════════════════════════════════════════════════════

   Models the Rholang Burned branch: only the quarantine record is cleared; the
   bond stays at its slashed value (0 in any reachable post-slash state), the
   Coop vault is unchanged, and crucially the validator is NOT removed from
   "mintingHalted" — a burned validator cannot resume minting. *)

Theorem redeem_burned_conserves :
  forall psc v bond,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    let (psc', ok) := redeem psc v Burned true in
    ok = true
    /\ ps_allBonds (psc_pos psc') = ps_allBonds (psc_pos psc)
    /\ ps_coopVault (psc_pos psc') = ps_coopVault (psc_pos psc)
    /\ halted_mem (psc_mintingHalted psc') v = halted_mem (psc_mintingHalted psc) v
    /\ qs_lookup (psc_quarantined psc') v = None.
Proof.
  intros psc v bond Hq. unfold redeem. rewrite Hq. simpl.
  repeat split.
  apply qs_lookup_remove_same.
Qed.

(* A post-slash validator that is burned STAYS halted: combined with
   [slashC_halts], a slash-then-burn leaves the offender in "mintingHalted". *)
Theorem redeem_burned_stays_halted :
  forall psc v bond,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    halted_mem (psc_mintingHalted psc) v = true ->
    let (psc', _) := redeem psc v Burned true in
    halted_mem (psc_mintingHalted psc') v = true.
Proof.
  intros psc v bond Hq Hhalt. unfold redeem. rewrite Hq. simpl. assumption.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Authorization / quarantine gates: a rejected redemption is a no-op
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem redeem_requires_quarantine :
  forall psc v o authorized,
    qs_lookup (psc_quarantined psc) v = None ->
    redeem psc v o authorized = (psc, false).
Proof.
  intros psc v o authorized Hq. unfold redeem.
  destruct authorized.
  - rewrite Hq. reflexivity.
  - reflexivity.
Qed.

Theorem redeem_authorized_only :
  forall psc v o,
    redeem psc v o false = (psc, false).
Proof.
  intros psc v o. unfold redeem. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Total-funds conservation across slash → redeem (HEADLINE)
   ═══════════════════════════════════════════════════════════════════════════

   The funds the Stage-C state tracks across the slash/quarantine/redeem
   lifecycle: the bonded stake, the Coop vault, and the quarantined stake. A
   slash moves the offender's bond OUT of `allBonds` (to 0) and INTO the
   quarantine; a Vindicated redeem moves it back; a Guilty redeem splits it
   between bond (remainder) and Coop (penalty). In each conserving outcome the
   tracked total is invariant. *)

Fixpoint sum_bonds (bm : BondMap) : nat :=
  match bm with
  | []            => 0
  | (_, n) :: rest => n + sum_bonds rest
  end.

Fixpoint sum_quarantined (qs : QuarantineMap) : nat :=
  match qs with
  | []            => 0
  | (_, n) :: rest => n + sum_quarantined rest
  end.

Definition total_funds (psc : PoSStateC) : nat :=
  sum_bonds (ps_allBonds (psc_pos psc))
  + ps_coopVault (psc_pos psc)
  + sum_quarantined (psc_quarantined psc).

(* ─── sum_bonds algebra under bm_update (first-binding-wins, matching
   bm_lookup): updating v to n changes the sum by (n − old lookup). ─── *)
Lemma sum_bonds_update :
  forall bm v n,
    sum_bonds (bm_update bm v n) + bm_lookup bm v = sum_bonds bm + n.
Proof.
  induction bm as [| [k m] rest IH]; intros v n; simpl.
  - destruct (validator_eq_dec v v) as [_ | H]; [simpl; lia | contradiction].
  - destruct (validator_eq_dec k v) as [Eq | Neq]; simpl.
    + (* head is v: lookup = m, update replaces head with n *) lia.
    + (* head not v: recurse on the tail *)
      specialize (IH v n). lia.
Qed.

Corollary sum_bonds_slash :
  forall bm v,
    sum_bonds (bm_slash bm v) + bm_lookup bm v = sum_bonds bm.
Proof.
  intros bm v. unfold bm_slash.
  pose proof (sum_bonds_update bm v 0) as H. lia.
Qed.

(* A slashC (positive bond) preserves the tracked total: the bond leaves
   `allBonds` and enters the quarantine, coop unchanged. *)
Lemma slashC_preserves_total :
  forall psc v,
    bm_lookup (ps_allBonds (psc_pos psc)) v > 0 ->
    total_funds (fst (slashC psc v)) = total_funds psc.
Proof.
  intros psc v Hpos. unfold total_funds, slashC.
  destruct (Nat.eq_dec (bm_lookup (ps_allBonds (psc_pos psc)) v) 0) as [E | NE].
  - lia.
  - simpl. unfold qs_insert. simpl.
    pose proof (sum_bonds_slash (ps_allBonds (psc_pos psc)) v) as Hs.
    lia.
Qed.

(* A Vindicated redeem preserves the tracked total: the bond leaves the
   quarantine and re-enters `allBonds` (which is 0 there post-slash), coop
   unchanged. We require the post-slash precondition that the validator's bond
   is currently 0 (true in any state reached by `slashC`). *)
Lemma redeem_vindicated_preserves_total :
  forall psc v bond,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    bm_lookup (ps_allBonds (psc_pos psc)) v = 0 ->
    sum_quarantined (qs_remove (psc_quarantined psc) v) + bond
      = sum_quarantined (psc_quarantined psc) ->
    total_funds (fst (redeem psc v Vindicated true)) = total_funds psc.
Proof.
  intros psc v bond Hq Hzero Hqsum. unfold total_funds, redeem.
  rewrite Hq. simpl.
  pose proof (sum_bonds_update (ps_allBonds (psc_pos psc)) v bond) as Hb.
  rewrite Hzero in Hb. (* bm_lookup = 0 *)
  lia.
Qed.

(* A Guilty redeem preserves the tracked total: the bond splits into the
   restored remainder (into `allBonds`) and the penalty (into Coop); the
   quarantine entry is removed. *)
Lemma redeem_guilty_preserves_total :
  forall psc v bond penalty,
    qs_lookup (psc_quarantined psc) v = Some bond ->
    bm_lookup (ps_allBonds (psc_pos psc)) v = 0 ->
    sum_quarantined (qs_remove (psc_quarantined psc) v) + bond
      = sum_quarantined (psc_quarantined psc) ->
    total_funds (fst (redeem psc v (Guilty penalty) true)) = total_funds psc.
Proof.
  intros psc v bond penalty Hq Hzero Hqsum. unfold total_funds, redeem.
  rewrite Hq. simpl.
  pose proof (sum_bonds_update (ps_allBonds (psc_pos psc)) v (bond - Nat.min penalty bond)) as Hb.
  rewrite Hzero in Hb.
  pose proof (Nat.le_min_r penalty bond) as Hmin. (* min penalty bond <= bond *)
  lia.
Qed.

(* The headline: a slash followed by an authorized CONSERVING redemption
   (Vindicated or Guilty) leaves the tracked total funds exactly as before the
   slash. We thread the structural facts the slash establishes — post-slash the
   offender's bond is 0 and its quarantine entry holds exactly the pre-slash
   bond, with the quarantine-sum delta accounting for that single entry. *)
Theorem slash_then_redeem_conserves_total :
  forall psc v o,
    bm_lookup (ps_allBonds (psc_pos psc)) v > 0 ->
    qs_lookup (psc_quarantined psc) v = None ->   (* not already quarantined *)
    (o = Vindicated \/ exists p, o = Guilty p) ->
    let psc1 := fst (slashC psc v) in
    total_funds (fst (redeem psc1 v o true)) = total_funds psc.
Proof.
  intros psc v o Hpos Hnq Ho. cbv zeta.
  set (b := bm_lookup (ps_allBonds (psc_pos psc)) v) in *.
  (* qs_remove on the original (v absent) is the identity. *)
  assert (Hrm : qs_remove (psc_quarantined psc) v = psc_quarantined psc).
  { clear -Hnq. induction (psc_quarantined psc) as [| [k n] rest IH]; simpl.
    - reflexivity.
    - simpl in Hnq. destruct (validator_eq_dec k v) as [Ek | Ek].
      + subst k. discriminate Hnq.
      + rewrite IH; [reflexivity | exact Hnq]. }
  (* Reduce ONLY `fst (slashC psc v)` to the post-slash record, leaving `redeem`
     and `total_funds` as applied symbols so the generic lemmas' conclusions
     match syntactically. *)
  unfold slashC. fold b.
  destruct (Nat.eq_dec b 0) as [E | NE]; [lia |]. cbn [fst].
  (* psc1 = mkPoSStateC (mkPoSState (bm_slash …) … coop)
                        (halted_add …) (qs_insert … b). The three preconditions
     of the generic conserving-redeem lemmas hold on this state. *)
  set (psc1 := mkPoSStateC
                 (mkPoSState (bm_slash (ps_allBonds (psc_pos psc)) v)
                             (filter (fun v' => if validator_eq_dec v' v then false else true)
                                     (ps_active (psc_pos psc)))
                             (ps_coopVault (psc_pos psc)))
                 (halted_add (psc_mintingHalted psc) v)
                 (qs_insert (psc_quarantined psc) v b)).
  assert (Hq1 : qs_lookup (psc_quarantined psc1) v = Some b)
    by (unfold psc1; simpl; apply qs_lookup_insert_same).
  assert (Hzero1 : bm_lookup (ps_allBonds (psc_pos psc1)) v = 0)
    by (unfold psc1; simpl; apply bm_slash_lookup).
  assert (Hqsum1 : sum_quarantined (qs_remove (psc_quarantined psc1) v) + b
                   = sum_quarantined (psc_quarantined psc1)).
  { unfold psc1; simpl. unfold qs_insert; simpl.
    destruct (validator_eq_dec v v) as [_ | Hc]; [| contradiction].
    rewrite Hrm. lia. }
  (* total_funds psc1 + b accounting: slashC preserves the tracked total, so
     total_funds psc1 = total_funds psc; but we feed the generic lemmas which
     prove total_funds (redeem psc1 …) = total_funds psc1 directly. We then
     close by total_funds psc1 = total_funds psc. *)
  assert (Hpsc1_total : total_funds psc1 = total_funds psc).
  { unfold psc1, total_funds; simpl. unfold qs_insert; simpl.
    pose proof (sum_bonds_slash (ps_allBonds (psc_pos psc)) v) as Hs. fold b in Hs.
    lia. }
  destruct Ho as [HV | [p HG]].
  - subst o.
    rewrite (redeem_vindicated_preserves_total psc1 v Hq1 Hzero1 Hqsum1). exact Hpsc1_total.
  - subst o.
    rewrite (redeem_guilty_preserves_total psc1 v p Hq1 Hzero1 Hqsum1). exact Hpsc1_total.
Qed.

(* The Burned case is intentionally NOT a conservation of the tracked total:
   burning destroys the quarantined stake (it leaves the bonded+coop+quarantine
   set entirely — the underlying REV becomes posVault protocol surplus, which
   `PoSStateC` does not track). The tracked total therefore DROPS by exactly the
   burned bond. We state this honestly rather than pretend conservation. *)
Theorem slash_then_redeem_burned_reduces_total_by_bond :
  forall psc v,
    bm_lookup (ps_allBonds (psc_pos psc)) v > 0 ->
    qs_lookup (psc_quarantined psc) v = None ->
    let b    := bm_lookup (ps_allBonds (psc_pos psc)) v in
    let psc1 := fst (slashC psc v) in
    total_funds (fst (redeem psc1 v Burned true)) + b = total_funds psc.
Proof.
  intros psc v Hpos Hnq. cbv zeta.
  set (b := bm_lookup (ps_allBonds (psc_pos psc)) v) in *.
  assert (Hrm : qs_remove (psc_quarantined psc) v = psc_quarantined psc).
  { clear -Hnq. induction (psc_quarantined psc) as [| [k n] rest IH]; simpl.
    - reflexivity.
    - simpl in Hnq. destruct (validator_eq_dec k v) as [Ek | Ek].
      + subst k. discriminate Hnq.
      + rewrite IH; [reflexivity | exact Hnq]. }
  (* Collapse `fst (slashC psc v)` to the post-slash record and name it psc1. *)
  unfold slashC. fold b.
  destruct (Nat.eq_dec b 0) as [E | NE]; [lia |]. cbn [fst].
  set (psc1 := mkPoSStateC
                 (mkPoSState (bm_slash (ps_allBonds (psc_pos psc)) v)
                             (filter (fun v' => if validator_eq_dec v' v then false else true)
                                     (ps_active (psc_pos psc)))
                             (ps_coopVault (psc_pos psc)))
                 (halted_add (psc_mintingHalted psc) v)
                 (qs_insert (psc_quarantined psc) v b)).
  (* The Burned branch fires: qs_lookup of psc1's quarantine head is Some b. *)
  assert (Hq1 : qs_lookup (psc_quarantined psc1) v = Some b)
    by (unfold psc1; simpl; apply qs_lookup_insert_same).
  assert (Hburn : fst (redeem psc1 v Burned true)
                  = mkPoSStateC
                      (mkPoSState (ps_allBonds (psc_pos psc1))
                                  (ps_active (psc_pos psc1))
                                  (ps_coopVault (psc_pos psc1)))
                      (psc_mintingHalted psc1)
                      (qs_remove (psc_quarantined psc1) v)).
  { unfold redeem. rewrite Hq1. reflexivity. }
  rewrite Hburn.
  (* Discharge by arithmetic: the bond left `allBonds` (sum_bonds_slash), the
     quarantine entry is removed (Hrm), coop unchanged ⇒ post-total + b = pre. *)
  unfold psc1, total_funds. cbn [psc_pos ps_allBonds ps_coopVault psc_quarantined].
  pose proof (sum_bonds_slash (ps_allBonds (psc_pos psc)) v) as Hs. fold b in Hs.
  unfold qs_insert. cbn [qs_remove].
  destruct (validator_eq_dec v v) as [_ | Hc]; [| contradiction].
  rewrite Hrm. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Other validators are untouched by a redemption
   ═══════════════════════════════════════════════════════════════════════════ *)

Theorem redeem_other_unchanged :
  forall psc v u o,
    u <> v ->
    qs_lookup (psc_quarantined psc) v <> None ->
    let (psc', _) := redeem psc v o true in
    bm_lookup (ps_allBonds (psc_pos psc')) u = bm_lookup (ps_allBonds (psc_pos psc)) u
    /\ qs_lookup (psc_quarantined psc') u = qs_lookup (psc_quarantined psc) u.
Proof.
  intros psc v u o Hne Hq. unfold redeem.
  destruct (qs_lookup (psc_quarantined psc) v) as [bond |] eqn:Hl; [| contradiction].
  destruct o; simpl.
  - (* Vindicated: bond restored via bm_update; other key untouched. *)
    split.
    + apply bm_lookup_update_diff. apply not_eq_sym. assumption.
    + apply qs_remove_other. assumption.
  - (* Guilty: remainder restored via bm_update; other key untouched. *)
    split.
    + apply bm_lookup_update_diff. apply not_eq_sym. assumption.
    + apply qs_remove_other. assumption.
  - (* Burned: allBonds unchanged; quarantine other key untouched. *)
    split.
    + reflexivity.
    + apply qs_remove_other. assumption.
Qed.
