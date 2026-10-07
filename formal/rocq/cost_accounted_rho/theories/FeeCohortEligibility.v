From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.

Import ListNotations.

Definition fee_authority_eligible (selected candidate : list nat) : bool :=
  negb (Nat.eqb (length candidate) 0) &&
  forallb
    (fun atom => count_occ Nat.eq_dec candidate atom <=? count_occ Nat.eq_dec selected atom)
    candidate.

Definition eligible_fee_presentations
  (selected : list nat) (presentations : list (list nat)) : list (list nat) :=
  filter (fee_authority_eligible selected) presentations.

Lemma fee_absent_atom_has_zero_count :
  forall values atom,
    ~ In atom values -> count_occ Nat.eq_dec values atom = 0.
Proof.
  induction values as [|value rest IH]; intros atom absent; simpl.
  - reflexivity.
  - destruct (Nat.eq_dec value atom) as [same|different].
    + subst. exfalso. apply absent. now left.
    + apply IH. intro present. apply absent. now right.
Qed.

Theorem fee_eligibility_is_exact_multiset_containment :
  forall selected candidate,
    fee_authority_eligible selected candidate = true <->
      candidate <> [] /\
      forall atom, count_occ Nat.eq_dec candidate atom <= count_occ Nat.eq_dec selected atom.
Proof.
  intros selected candidate. unfold fee_authority_eligible.
  rewrite andb_true_iff, negb_true_iff, Nat.eqb_neq, forallb_forall.
  split.
  - intros [nonempty contained]. split.
    + intro empty. subst. contradiction.
    + intros atom. destruct (in_dec Nat.eq_dec atom candidate) as [present|absent].
      * specialize (contained atom present). now apply Nat.leb_le in contained.
      * rewrite fee_absent_atom_has_zero_count by assumption. lia.
  - intros [nonempty contained]. split.
    + destruct candidate; simpl; [contradiction|lia].
    + intros atom present. apply Nat.leb_le. apply contained.
Qed.

Theorem every_emitted_fee_presentation_is_authorized :
  forall selected presentations candidate,
    In candidate (eligible_fee_presentations selected presentations) ->
    In candidate presentations /\ candidate <> [] /\
    forall atom, count_occ Nat.eq_dec candidate atom <= count_occ Nat.eq_dec selected atom.
Proof.
  intros selected presentations candidate accepted.
  unfold eligible_fee_presentations in accepted.
  apply filter_In in accepted. destruct accepted as [present authorized].
  apply fee_eligibility_is_exact_multiset_containment in authorized.
  tauto.
Qed.

Theorem every_authorized_fee_presentation_is_emitted :
  forall selected presentations candidate,
    In candidate presentations -> candidate <> [] ->
    (forall atom, count_occ Nat.eq_dec candidate atom <= count_occ Nat.eq_dec selected atom) ->
    In candidate (eligible_fee_presentations selected presentations).
Proof.
  intros selected presentations candidate present nonempty contained.
  unfold eligible_fee_presentations. apply filter_In. split; [assumption|].
  apply fee_eligibility_is_exact_multiset_containment. tauto.
Qed.

Theorem unauthorized_multiplicity_cannot_fund_fee :
  forall selected presentations candidate atom,
    count_occ Nat.eq_dec selected atom < count_occ Nat.eq_dec candidate atom ->
    ~ In candidate (eligible_fee_presentations selected presentations).
Proof.
  intros selected presentations candidate atom excessive accepted.
  apply every_emitted_fee_presentation_is_authorized in accepted.
  destruct accepted as [_ [_ contained]]. specialize (contained atom). lia.
Qed.

Theorem unrelated_presentations_do_not_change_eligible_membership :
  forall selected presentations unrelated candidate,
    fee_authority_eligible selected unrelated = false ->
    (In candidate (eligible_fee_presentations selected (unrelated :: presentations)) <->
     In candidate (eligible_fee_presentations selected presentations)).
Proof.
  intros selected presentations unrelated candidate rejected.
  unfold eligible_fee_presentations. simpl. now rewrite rejected.
Qed.

Print Assumptions fee_eligibility_is_exact_multiset_containment.
Print Assumptions every_emitted_fee_presentation_is_authorized.
Print Assumptions every_authorized_fee_presentation_is_emitted.
Print Assumptions unauthorized_multiplicity_cannot_fund_fee.
Print Assumptions unrelated_presentations_do_not_change_eligible_membership.
