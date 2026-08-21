(* Native finite located fragment of the OSLF logic described in
   continued-gslt-cost-v2, Sections 9 and 10.  The observation separates
   authenticated supply from demand knowledge.  Exact demand supports the
   graded spend modality; a conservative upper bound supports only safety
   judgments such as sufficiency.  This is the proof-level counterpart of
   accounting/oslf.rs. *)

From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lists.List.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CALocatedPurses.

Inductive oslf_verdict : Type :=
  | OSatisfied
  | OUnsatisfied
  | OIndeterminate.

Definition verdict_and (left right : oslf_verdict) : oslf_verdict :=
  match left, right with
  | OUnsatisfied, _ | _, OUnsatisfied => OUnsatisfied
  | OIndeterminate, _ | _, OIndeterminate => OIndeterminate
  | OSatisfied, OSatisfied => OSatisfied
  end.

Definition verdict_not (verdict : oslf_verdict) : oslf_verdict :=
  match verdict with
  | OSatisfied => OUnsatisfied
  | OUnsatisfied => OSatisfied
  | OIndeterminate => OIndeterminate
  end.

Record oslf_observation : Type := mk_oslf_observation {
  observed_supply : located_purse;
  observed_demand : located_purse;
  demand_is_exact : bool
}.

Definition available_check
    (observation : oslf_observation) (surface : sig) (amount : nat)
    : oslf_verdict :=
  if Nat.leb amount (observed_supply observation surface)
  then OSatisfied
  else OUnsatisfied.

Definition required_check
    (observation : oslf_observation) (surface : sig) (amount : nat)
    : oslf_verdict :=
  if Nat.eqb amount 0 then OSatisfied else
  if demand_is_exact observation then
    if Nat.leb amount (observed_demand observation surface)
    then OSatisfied
    else OUnsatisfied
  else
    if Nat.ltb (observed_demand observation surface) amount
    then OUnsatisfied
    else OIndeterminate.

Definition sufficient_check
    (observation : oslf_observation) (surface : sig) : oslf_verdict :=
  if Nat.leb (observed_demand observation surface)
             (observed_supply observation surface)
  then OSatisfied
  else OUnsatisfied.

Definition spend_check
    (observation : oslf_observation) (surface : sig) (amount : nat)
    : oslf_verdict :=
  if Nat.eqb amount 0 then OUnsatisfied else
  if Nat.ltb (observed_supply observation surface) amount
  then OUnsatisfied else
  if demand_is_exact observation then
    if Nat.leb amount (observed_demand observation surface)
    then OSatisfied
    else OUnsatisfied
  else
    if Nat.ltb (observed_demand observation surface) amount
    then OUnsatisfied
    else OIndeterminate.

Definition after_spend
    (observation : oslf_observation) (surface : sig) (amount : nat)
    : oslf_observation :=
  mk_oslf_observation
    (draw_at (observed_supply observation) surface amount)
    (draw_at (observed_demand observation) surface amount)
    (demand_is_exact observation).

Definition linear_check
    (observation : oslf_observation) (surface : sig) : oslf_verdict :=
  verdict_and (available_check observation surface 1)
    (verdict_and (required_check observation surface 1)
      (verdict_and (verdict_not (required_check observation surface 2))
                   (spend_check observation surface 1))).

Definition copyable_check
    (_ : oslf_observation) (_ : sig) : oslf_verdict := OSatisfied.

Definition relevant_check
    (observation : oslf_observation) (surface : sig) : oslf_verdict :=
  spend_check observation surface 1.

Definition local_observation
    (observation : oslf_observation) (surface : sig) : oslf_observation :=
  mk_oslf_observation
    (fun candidate => if sig_eq_dec surface candidate
                      then observed_supply observation candidate else 0)
    (fun candidate => if sig_eq_dec surface candidate
                      then observed_demand observation candidate else 0)
    (demand_is_exact observation).

Definition spatial_check
    (left_surface right_surface : sig)
    (left_verdict right_verdict : oslf_verdict) : oslf_verdict :=
  if sig_eq_dec left_surface right_surface
  then OUnsatisfied
  else verdict_and left_verdict right_verdict.

Lemma verdict_and_satisfied_iff : forall left right,
  verdict_and left right = OSatisfied <->
  left = OSatisfied /\ right = OSatisfied.
Proof. intros left right. destruct left, right; simpl; split; intuition discriminate. Qed.

Lemma verdict_not_satisfied_iff : forall verdict,
  verdict_not verdict = OSatisfied <-> verdict = OUnsatisfied.
Proof. intros verdict. destruct verdict; simpl; split; intuition discriminate. Qed.

Lemma available_check_sound_complete : forall observation surface amount,
  available_check observation surface amount = OSatisfied <->
  amount <= observed_supply observation surface.
Proof.
  intros observation surface amount. unfold available_check.
  destruct (Nat.leb amount (observed_supply observation surface)) eqn:Hle; simpl.
  - apply Nat.leb_le in Hle. split; intro H; [ assumption | reflexivity ].
  - apply Nat.leb_gt in Hle. split; intro H; [ discriminate | lia ].
Qed.

Theorem exact_required_check_sound_complete : forall supply demand surface amount,
  required_check (mk_oslf_observation supply demand true) surface amount = OSatisfied
  <-> amount <= demand surface.
Proof.
  intros supply demand surface amount. unfold required_check. simpl.
  destruct (Nat.eqb amount 0) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. subst amount. simpl.
    split; intro H; [ lia | reflexivity ].
  - destruct (Nat.leb amount (demand surface)) eqn:Hle; simpl.
    + apply Nat.leb_le in Hle. split; intro H; [ assumption | reflexivity ].
    + apply Nat.leb_gt in Hle. split; intro H; [ discriminate | lia ].
Qed.

Theorem exact_spend_check_sound_complete : forall supply demand surface amount,
  spend_check (mk_oslf_observation supply demand true) surface amount = OSatisfied
  <-> 0 < amount /\ amount <= supply surface /\ amount <= demand surface.
Proof.
  intros supply demand surface amount. unfold spend_check. simpl.
  destruct (Nat.eqb amount 0) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. subst. split; [ discriminate | lia ].
  - apply Nat.eqb_neq in Hzero.
    destruct (Nat.ltb (supply surface) amount) eqn:Hsup; simpl.
    + apply Nat.ltb_lt in Hsup. split; intro H; [ discriminate | lia ].
    + apply Nat.ltb_ge in Hsup.
      destruct (Nat.leb amount (demand surface)) eqn:Hdem; simpl.
      * apply Nat.leb_le in Hdem.
        split; intro H; [ lia | reflexivity ].
      * apply Nat.leb_gt in Hdem. split; intro H; [ discriminate | lia ].
Qed.

Theorem exact_linear_check_sound : forall supply demand surface,
  linear_check (mk_oslf_observation supply demand true) surface = OSatisfied ->
  1 <= supply surface /\ demand surface = 1.
Proof.
  intros supply demand surface H.
  unfold linear_check in H.
  apply verdict_and_satisfied_iff in H as [Havailable Hrest].
  apply verdict_and_satisfied_iff in Hrest as [Hrequired Hone].
  apply verdict_and_satisfied_iff in Hone as [Hsingle _].
  apply available_check_sound_complete in Havailable.
  apply exact_required_check_sound_complete in Hrequired.
  apply verdict_not_satisfied_iff in Hsingle.
  assert (Hnot_two : ~ 2 <= demand surface).
  {
    intro Htwo.
    pose proof
      (proj2 (exact_required_check_sound_complete supply demand surface 2) Htwo)
      as Hsatisfied.
    rewrite Hsatisfied in Hsingle. discriminate.
  }
  split; [ exact Havailable | lia ].
Qed.

Theorem exact_linear_check_complete : forall supply demand surface,
  1 <= supply surface -> demand surface = 1 ->
  linear_check (mk_oslf_observation supply demand true) surface = OSatisfied.
Proof.
  intros supply demand surface Hs Hd.
  unfold linear_check. apply verdict_and_satisfied_iff. split.
  - apply available_check_sound_complete. exact Hs.
  - apply verdict_and_satisfied_iff. split.
    + apply exact_required_check_sound_complete. lia.
    + apply verdict_and_satisfied_iff. split.
      * apply verdict_not_satisfied_iff.
        unfold required_check. simpl. rewrite Hd. reflexivity.
      * apply exact_spend_check_sound_complete. lia.
Qed.

Theorem exact_linear_is_relevant : forall supply demand surface,
  linear_check (mk_oslf_observation supply demand true) surface = OSatisfied ->
  relevant_check (mk_oslf_observation supply demand true) surface = OSatisfied.
Proof.
  intros supply demand surface Hlinear.
  apply exact_linear_check_sound in Hlinear as [Hs Hd].
  unfold relevant_check. apply exact_spend_check_sound_complete.
  subst. lia.
Qed.

Theorem linear_forbids_contraction : forall supply demand surface,
  2 <= demand surface ->
  linear_check (mk_oslf_observation supply demand true) surface <> OSatisfied.
Proof.
  intros supply demand surface Htwo Hlinear.
  apply exact_linear_check_sound in Hlinear as [_ Hone]. lia.
Qed.

Theorem linear_forbids_weakening : forall supply demand surface,
  demand surface = 0 ->
  linear_check (mk_oslf_observation supply demand true) surface <> OSatisfied.
Proof.
  intros supply demand surface Hzero Hlinear.
  apply exact_linear_check_sound in Hlinear as [_ Hone]. lia.
Qed.

Theorem copyable_permits_weakening_and_contraction : forall observation surface,
  copyable_check observation surface = OSatisfied.
Proof. reflexivity. Qed.

Theorem relevant_permits_multiplicity : forall supply demand surface,
  1 <= supply surface -> 1 <= demand surface ->
  relevant_check (mk_oslf_observation supply demand true) surface = OSatisfied.
Proof.
  intros supply demand surface Hs Hd.
  unfold relevant_check. apply exact_spend_check_sound_complete. lia.
Qed.

Theorem modal_poststate_is_exact : forall observation surface amount,
  observed_supply (after_spend observation surface amount) surface =
    observed_supply observation surface - amount
  /\ observed_demand (after_spend observation surface amount) surface =
    observed_demand observation surface - amount.
Proof.
  intros observation surface amount. unfold after_spend. simpl.
  split; apply draw_at_here.
Qed.

Theorem modal_spend_preserves_other_surface : forall observation surface amount other,
  surface <> other ->
  observed_supply (after_spend observation surface amount) other =
    observed_supply observation other
  /\ observed_demand (after_spend observation surface amount) other =
    observed_demand observation other.
Proof.
  intros observation surface amount other Hneq. unfold after_spend. simpl.
  split; apply draw_disjoint; assumption.
Qed.

Theorem located_observation_isolates_other_surface : forall observation surface other,
  surface <> other ->
  observed_supply (local_observation observation surface) other = 0
  /\ observed_demand (local_observation observation surface) other = 0.
Proof.
  intros observation surface other Hneq. unfold local_observation. simpl.
  destruct (sig_eq_dec surface other) as [Heq | _].
  - contradiction.
  - tauto.
Qed.

Theorem spatial_requires_disjoint_locations : forall surface verdict,
  spatial_check surface surface verdict verdict = OUnsatisfied.
Proof.
  intros surface verdict. unfold spatial_check.
  destruct (sig_eq_dec surface surface); [ reflexivity | contradiction ].
Qed.

Theorem spatial_is_commutative : forall left right left_verdict right_verdict,
  spatial_check left right left_verdict right_verdict =
  spatial_check right left right_verdict left_verdict.
Proof.
  intros left right left_verdict right_verdict. unfold spatial_check.
  destruct (sig_eq_dec left right) as [Heq | Hneq].
  - subst. destruct (sig_eq_dec right right); [ reflexivity | contradiction ].
  - destruct (sig_eq_dec right left) as [Heq | _].
    + symmetry in Heq. contradiction.
    + destruct left_verdict, right_verdict; reflexivity.
Qed.

Theorem spatial_local_sufficiency_composes : forall supply demand locations,
  (forall surface, In surface locations ->
      sufficient_check
        (mk_oslf_observation supply demand false) surface = OSatisfied) ->
  total demand locations <= total supply locations.
Proof.
  intros supply demand locations. induction locations as [| surface rest IH]; intro Hchecks.
  - simpl. lia.
  - simpl. assert (Hhead : demand surface <= supply surface).
    {
      specialize (Hchecks surface (or_introl eq_refl)).
      unfold sufficient_check in Hchecks. simpl in Hchecks.
      destruct (Nat.leb (demand surface) (supply surface)) eqn:Hfund;
        try discriminate.
      apply Nat.leb_le. exact Hfund.
    }
    assert (Htail : total demand rest <= total supply rest).
    {
      apply IH. intros candidate Hin.
      apply Hchecks. exact (or_intror Hin).
    }
    lia.
Qed.

Theorem conservative_sufficiency_is_sound : forall supply upper actual surface,
  actual surface <= upper surface ->
  sufficient_check (mk_oslf_observation supply upper false) surface = OSatisfied ->
  actual surface <= supply surface.
Proof.
  intros supply upper actual surface Hbound Hcheck.
  unfold sufficient_check in Hcheck. simpl in Hcheck.
  destruct (Nat.leb (upper surface) (supply surface)) eqn:Hfund; try discriminate.
  apply Nat.leb_le in Hfund. lia.
Qed.

Theorem upper_bound_cannot_assert_modal_spend : forall supply upper surface,
  1 <= supply surface -> 1 <= upper surface ->
  spend_check (mk_oslf_observation supply upper false) surface 1 = OIndeterminate.
Proof.
  intros supply upper surface Hsupply Hupper.
  unfold spend_check. simpl.
  destruct (Nat.ltb (supply surface) 1) eqn:Hslt.
  - apply Nat.ltb_lt in Hslt. lia.
  - destruct (Nat.ltb (upper surface) 1) eqn:Hlt.
    + apply Nat.ltb_lt in Hlt. lia.
    + reflexivity.
Qed.

Theorem upper_bound_insufficient_supply_rejects : forall supply upper surface amount,
  supply surface < amount ->
  spend_check (mk_oslf_observation supply upper false) surface amount = OUnsatisfied.
Proof.
  intros supply upper surface amount Hunder.
  unfold spend_check.
  destruct (Nat.eqb amount 0) eqn:Hzero.
  - reflexivity.
  - simpl. destruct (Nat.ltb (supply surface) amount) eqn:Hlt.
    + reflexivity.
    + apply Nat.ltb_ge in Hlt. lia.
Qed.

Theorem authenticated_supply_excludes_candidate_credit :
  forall (authenticated candidate upper : located_purse) surface,
    upper surface > authenticated surface ->
    sufficient_check
      (mk_oslf_observation authenticated upper false) surface = OUnsatisfied.
Proof.
  intros authenticated candidate upper surface Hunder.
  unfold sufficient_check. simpl.
  destruct (Nat.leb (upper surface) (authenticated surface)) eqn:Hfund.
  - apply Nat.leb_le in Hfund. lia.
  - reflexivity.
Qed.

Theorem every_native_oslf_check_is_decidable : forall observation surface,
  linear_check observation surface = OSatisfied
  \/ linear_check observation surface = OUnsatisfied
  \/ linear_check observation surface = OIndeterminate.
Proof.
  intros observation surface. destruct (linear_check observation surface);
    intuition.
Qed.

Print Assumptions exact_spend_check_sound_complete.
Print Assumptions exact_linear_check_sound.
Print Assumptions exact_linear_check_complete.
Print Assumptions linear_forbids_contraction.
Print Assumptions linear_forbids_weakening.
Print Assumptions modal_poststate_is_exact.
Print Assumptions modal_spend_preserves_other_surface.
Print Assumptions located_observation_isolates_other_surface.
Print Assumptions spatial_is_commutative.
Print Assumptions spatial_local_sufficiency_composes.
Print Assumptions conservative_sufficiency_is_sound.
Print Assumptions upper_bound_cannot_assert_modal_spend.
Print Assumptions upper_bound_insufficient_supply_rejects.
Print Assumptions authenticated_supply_excludes_candidate_credit.
