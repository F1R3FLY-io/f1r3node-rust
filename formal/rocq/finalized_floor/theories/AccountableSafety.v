From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import ZArith.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import FtExact.
From FinalizedFloor Require Import CliqueOracle.

Open Scope nat_scope.

Fixpoint validator_stake
  (stake : Validator -> nat)
  (validators : list Validator) : nat :=
  match validators with
  | [] => 0
  | validator :: rest => stake validator + validator_stake stake rest
  end.

Lemma validator_stake_app :
  forall stake left right,
    validator_stake stake (left ++ right) =
    validator_stake stake left + validator_stake stake right.
Proof.
  intros stake left right.
  induction left as [| validator rest IH]; simpl; lia.
Qed.

Lemma nodup_remove_middle :
  forall (validator : Validator) (before after : list Validator),
    NoDup (before ++ validator :: after) ->
    NoDup (before ++ after).
Proof.
  intros validator before.
  induction before as [| head rest IH]; intros after Hnodup.
  - simpl in *. inversion Hnodup. assumption.
  - simpl in *. inversion Hnodup as [| ? ? Hnotin Htail]; subst.
    constructor.
    + intro Hin.
      apply Hnotin.
      apply in_app_or in Hin.
      apply in_or_app.
      destruct Hin as [Hin | Hin].
      * left. exact Hin.
      * right. simpl. right. exact Hin.
    + apply IH. exact Htail.
Qed.

Lemma validator_stake_incl :
  forall stake selected committee,
    NoDup selected ->
    NoDup committee ->
    incl selected committee ->
    validator_stake stake selected <= validator_stake stake committee.
Proof.
  intros stake selected.
  induction selected as [| validator rest IH];
    intros committee Hselected Hcommittee Hincl.
  - simpl. lia.
  - inversion Hselected as [| ? ? Hnotin Hrest]; subst.
    assert (Hmember : In validator committee).
    { apply Hincl. left. reflexivity. }
    apply in_split in Hmember.
    destruct Hmember as [before [after Hcommittee_split]].
    subst committee.
    assert (Hcommittee_without : NoDup (before ++ after)).
    { apply nodup_remove_middle with (validator := validator). exact Hcommittee. }
    assert (Hrest_incl : incl rest (before ++ after)).
    {
      intros member Hmember_rest.
      specialize (Hincl member (or_intror Hmember_rest)).
      apply in_app_or in Hincl.
      apply in_or_app.
      destruct Hincl as [Hbefore | Htail].
      - left. exact Hbefore.
      - simpl in Htail.
        destruct Htail as [Heq | Hafter].
        + subst member. contradiction.
        + right. exact Hafter.
    }
    specialize (IH (before ++ after) Hrest Hcommittee_without Hrest_incl).
    repeat rewrite validator_stake_app in *.
    simpl in *.
    lia.
Qed.

Definition validator_disjoint
  (left right : list Validator) : Prop :=
  forall validator, In validator left -> ~ In validator right.

Lemma nodup_app_of_disjoint :
  forall (left right : list Validator),
    NoDup left ->
    NoDup right ->
    validator_disjoint left right ->
    NoDup (left ++ right).
Proof.
  intros left right Hleft Hright Hdisjoint.
  induction Hleft as [| validator rest Hnotin Hrest IH].
  - simpl. exact Hright.
  - simpl. constructor.
    + intro Hin.
      apply in_app_or in Hin.
      destruct Hin as [Hin | Hin].
      * contradiction.
      * exact ((Hdisjoint validator (or_introl eq_refl)) Hin).
    + apply IH.
      intros member Hmember_rest Hmember_right.
      exact (Hdisjoint member (or_intror Hmember_rest) Hmember_right).
Qed.

Lemma incl_app :
  forall (left right committee : list Validator),
    incl left committee ->
    incl right committee ->
    incl (left ++ right) committee.
Proof.
  intros left right committee Hleft Hright member Hmember.
  apply in_app_or in Hmember.
  destruct Hmember as [Hmember | Hmember].
  - apply Hleft. exact Hmember.
  - apply Hright. exact Hmember.
Qed.

Theorem disjoint_validator_stake_bound :
  forall stake committee left right,
    NoDup committee ->
    NoDup left ->
    NoDup right ->
    incl left committee ->
    incl right committee ->
    validator_disjoint left right ->
    validator_stake stake left + validator_stake stake right <=
    validator_stake stake committee.
Proof.
  intros stake committee left right Hcommittee Hleft Hright
    Hleft_in Hright_in Hdisjoint.
  rewrite <- validator_stake_app.
  apply validator_stake_incl.
  - apply nodup_app_of_disjoint; assumption.
  - exact Hcommittee.
  - apply incl_app; assumption.
Qed.

Definition validator_mem
  (validator : Validator)
  (validators : list Validator) : bool :=
  existsb (Nat.eqb validator) validators.

Lemma validator_mem_true_iff :
  forall validator validators,
    validator_mem validator validators = true <-> In validator validators.
Proof.
  intros validator validators.
  unfold validator_mem.
  rewrite existsb_exists.
  split.
  - intros [member [Hmember Heq]].
    apply Nat.eqb_eq in Heq. subst member. exact Hmember.
  - intros Hmember.
    exists validator. split; [exact Hmember | apply Nat.eqb_refl].
Qed.

Definition validator_intersection
  (left right : list Validator) : list Validator :=
  filter (fun validator => validator_mem validator left) right.

Definition validator_difference
  (left right : list Validator) : list Validator :=
  filter (fun validator => negb (validator_mem validator left)) right.

Lemma nodup_filter_validator :
  forall (predicate : Validator -> bool) (validators : list Validator),
    NoDup validators ->
    NoDup (filter predicate validators).
Proof.
  intros predicate validators Hnodup.
  induction Hnodup as [| validator rest Hnotin Hrest IH]; simpl.
  - constructor.
  - destruct (predicate validator) eqn:Hpredicate.
    + constructor.
      * intro Hin.
        apply filter_In in Hin. destruct Hin as [Hin _]. contradiction.
      * exact IH.
    + exact IH.
Qed.

Lemma validator_stake_partition :
  forall stake selected validators,
    validator_stake stake validators =
    validator_stake stake (validator_intersection selected validators) +
    validator_stake stake (validator_difference selected validators).
Proof.
  intros stake selected validators.
  induction validators as [| validator rest IH]; simpl.
  - reflexivity.
  - unfold validator_intersection, validator_difference in *.
    simpl in *.
    destruct (validator_mem validator selected) eqn:Hmember; simpl in *; lia.
Qed.

Lemma intersection_in_left :
  forall left right,
    incl (validator_intersection left right) left.
Proof.
  intros left right validator Hmember.
  apply filter_In in Hmember.
  destruct Hmember as [_ Hselected].
  apply validator_mem_true_iff. exact Hselected.
Qed.

Lemma intersection_in_right :
  forall left right,
    incl (validator_intersection left right) right.
Proof.
  intros left right validator Hmember.
  apply filter_In in Hmember.
  exact (proj1 Hmember).
Qed.

Lemma difference_in_right :
  forall left right,
    incl (validator_difference left right) right.
Proof.
  intros left right validator Hmember.
  apply filter_In in Hmember.
  exact (proj1 Hmember).
Qed.

Lemma difference_disjoint :
  forall left right,
    validator_disjoint left (validator_difference left right).
Proof.
  intros left right validator Hleft Hdifference.
  apply filter_In in Hdifference.
  destruct Hdifference as [_ Hnot_selected].
  apply Bool.negb_true_iff in Hnot_selected.
  apply validator_mem_true_iff in Hleft.
  congruence.
Qed.

Theorem overlap_validator_stake_bound :
  forall stake committee faulty left right,
    NoDup committee ->
    NoDup faulty ->
    NoDup left ->
    NoDup right ->
    incl faulty committee ->
    incl left committee ->
    incl right committee ->
    (forall validator,
      In validator left ->
      In validator right ->
      In validator faulty) ->
    validator_stake stake left + validator_stake stake right <=
    validator_stake stake committee + validator_stake stake faulty.
Proof.
  intros stake committee faulty left right Hcommittee Hfaulty Hleft Hright
    Hfaulty_in Hleft_in Hright_in Hcommon_faulty.
  pose proof
    (disjoint_validator_stake_bound
      stake committee left (validator_difference left right)
      Hcommittee Hleft
      (nodup_filter_validator _ _ Hright)
      Hleft_in
      (fun validator Hmember =>
        Hright_in validator (difference_in_right left right validator Hmember))
      (difference_disjoint left right)) as Hunion.
  assert (Hintersection_faulty :
    incl (validator_intersection left right) faulty).
  {
    intros validator Hmember.
    apply Hcommon_faulty.
    - exact (intersection_in_left left right validator Hmember).
    - exact (intersection_in_right left right validator Hmember).
  }
  pose proof
    (validator_stake_incl
      stake
      (validator_intersection left right)
      faulty
      (nodup_filter_validator _ _ Hright)
      Hfaulty
      Hintersection_faulty) as Hintersection.
  pose proof (validator_stake_partition stake left right) as Hpartition.
  lia.
Qed.

Definition exact_support_certificate
  (stake : Validator -> nat)
  (committee supporters : list Validator)
  (num den : Z) : Prop :=
  NoDup supporters /\
  incl supporters committee /\
  ft_exact_ge
    (Z.of_nat (validator_stake stake supporters))
    (Z.of_nat (validator_stake stake committee))
    num
    den.

Definition strict_exact_support_certificate
  (stake : Validator -> nat)
  (committee supporters : list Validator)
  (num den : Z) : Prop :=
  NoDup supporters /\
  incl supporters committee /\
  ft_exact_gt
    (Z.of_nat (validator_stake stake supporters))
    (Z.of_nat (validator_stake stake committee))
    num
    den.

Theorem exact_certificates_exceed_fault_budget :
  forall stake committee faulty left right num den,
    NoDup committee ->
    NoDup faulty ->
    incl faulty committee ->
    exact_support_certificate stake committee left num den ->
    exact_support_certificate stake committee right num den ->
    (0 < num)%Z ->
    (0 < den)%Z ->
    (Z.of_nat (validator_stake stake faulty) * den <
      Z.of_nat (validator_stake stake committee) * num)%Z ->
    (forall validator,
      In validator left ->
      In validator right ->
      In validator faulty) ->
    False.
Proof.
  intros stake committee faulty left right num den
    Hcommittee Hfaulty Hfaulty_in
    [Hleft [Hleft_in Hleft_exact]]
    [Hright [Hright_in Hright_exact]]
    Hnum Hden Hbudget Hcommon_faulty.
  pose proof
    (overlap_validator_stake_bound
      stake committee faulty left right
      Hcommittee Hfaulty Hleft Hright Hfaulty_in
      Hleft_in Hright_in Hcommon_faulty) as Hoverlap.
  unfold ft_exact_ge in Hleft_exact, Hright_exact.
  nia.
Qed.

Theorem strict_exact_certificates_exceed_fault_budget :
  forall stake committee faulty left right num den,
    NoDup committee ->
    NoDup faulty ->
    incl faulty committee ->
    strict_exact_support_certificate stake committee left num den ->
    strict_exact_support_certificate stake committee right num den ->
    (0 < den)%Z ->
    (Z.of_nat (validator_stake stake faulty) * den <=
      Z.of_nat (validator_stake stake committee) * num)%Z ->
    (forall validator,
      In validator left ->
      In validator right ->
      In validator faulty) ->
    False.
Proof.
  intros stake committee faulty left right num den
    Hcommittee Hfaulty Hfaulty_in
    [Hleft [Hleft_in Hleft_exact]]
    [Hright [Hright_in Hright_exact]]
    Hden Hbudget Hcommon_faulty.
  pose proof
    (overlap_validator_stake_bound
      stake committee faulty left right
      Hcommittee Hfaulty Hleft Hright Hfaulty_in
      Hleft_in Hright_in Hcommon_faulty) as Hoverlap.
  unfold ft_exact_gt in Hleft_exact, Hright_exact.
  nia.
Qed.

Section AccountableFinality.

Variable Candidate : Type.
Variable supports : Validator -> Candidate -> Prop.
Variable incompatible : Candidate -> Candidate -> Prop.

Definition certified
  (stake : Validator -> nat)
  (committee : list Validator)
  (num den : Z)
  (candidate : Candidate) : Prop :=
  exists supporters,
    exact_support_certificate stake committee supporters num den /\
    forall validator, In validator supporters -> supports validator candidate.

Definition incompatibility_is_accountable
  (faulty : list Validator) : Prop :=
  forall validator left right,
    incompatible left right ->
    supports validator left ->
    supports validator right ->
    In validator faulty.

Theorem certified_incompatible_candidates_exceed_fault_budget :
  forall stake committee faulty num den left right,
    NoDup committee ->
    NoDup faulty ->
    incl faulty committee ->
    incompatibility_is_accountable faulty ->
    certified stake committee num den left ->
    certified stake committee num den right ->
    incompatible left right ->
    (0 < num)%Z ->
    (0 < den)%Z ->
    (Z.of_nat (validator_stake stake faulty) * den <
      Z.of_nat (validator_stake stake committee) * num)%Z ->
    False.
Proof.
  intros stake committee faulty num den left right
    Hcommittee Hfaulty Hfaulty_in Haccountable
    [left_supporters [Hleft_certificate Hleft_supports]]
    [right_supporters [Hright_certificate Hright_supports]]
    Hincompatible Hnum Hden Hbudget.
  eapply exact_certificates_exceed_fault_budget;
    [ exact Hcommittee
    | exact Hfaulty
    | exact Hfaulty_in
    | exact Hleft_certificate
    | exact Hright_certificate
    | exact Hnum
    | exact Hden
    | exact Hbudget
    | ].
  intros validator Hleft_member Hright_member.
  eapply Haccountable.
  - exact Hincompatible.
  - apply Hleft_supports. exact Hleft_member.
  - apply Hright_supports. exact Hright_member.
Qed.

End AccountableFinality.

Fixpoint committee_stake
  (committee : Committee)
  (validator : Validator) : nat :=
  match committee with
  | [] => 0
  | (member, stake) :: rest =>
      if Nat.eqb member validator
      then stake
      else committee_stake rest validator
  end.

Lemma committee_stake_member :
  forall committee validator stake,
    NoDup (map fst committee) ->
    In (validator, stake) committee ->
    committee_stake committee validator = stake.
Proof.
  induction committee as [| [member member_stake] rest IH];
    intros validator stake Hnodup Hmember.
  - contradiction.
  - simpl in *.
    inversion Hnodup as [| ? ? Hmember_notin Hrest_nodup]; subst.
    destruct (Nat.eqb member validator) eqn:Heq.
    + apply Nat.eqb_eq in Heq. subst member.
      destruct Hmember as [Hhead | Htail].
      * inversion Hhead. reflexivity.
      * exfalso. apply Hmember_notin.
        apply in_map with (f := fst) in Htail. simpl in Htail. exact Htail.
    + apply Nat.eqb_neq in Heq.
      destruct Hmember as [Hhead | Htail].
      * inversion Hhead. contradiction.
      * apply IH; assumption.
Qed.

Lemma committee_stake_sum_exact :
  forall committee selected,
    NoDup (map fst committee) ->
    incl selected committee ->
    validator_stake (committee_stake committee) (map fst selected) =
    cweight selected.
Proof.
  intros committee selected Hcommittee Hincl.
  induction selected as [| [validator stake] rest IH].
  - reflexivity.
  - simpl.
    rewrite committee_stake_member with (stake := stake).
    + rewrite IH.
      * reflexivity.
      * intros member Hmember. apply Hincl. right. exact Hmember.
    + exact Hcommittee.
    + apply Hincl. left. reflexivity.
Qed.

Lemma mapped_committee_incl :
  forall (selected committee : Committee),
    incl selected committee ->
    incl (map fst selected) (map fst committee).
Proof.
  intros selected committee Hincl validator Hmember.
  apply in_map_iff in Hmember.
  destruct Hmember as [[member stake] [Heq Hselected]].
  simpl in Heq. subst member.
  apply in_map_iff.
  exists (validator, stake).
  split; [reflexivity |].
  apply Hincl. exact Hselected.
Qed.

Theorem quorum_ft_refines_exact_support_certificate :
  forall committee supporters num den,
    NoDup (map fst committee) ->
    is_quorum_ft committee supporters num den ->
    exact_support_certificate
      (committee_stake committee)
      (map fst committee)
      (map fst supporters)
      num
      den.
Proof.
  intros committee supporters num den Hcommittee
    [Hincl [Hsupporters Hexact]].
  unfold exact_support_certificate.
  split; [exact Hsupporters |].
  split.
  - apply mapped_committee_incl. exact Hincl.
  - rewrite committee_stake_sum_exact with
      (committee := committee) (selected := supporters).
    + rewrite committee_stake_sum_exact with
        (committee := committee) (selected := committee).
      * exact Hexact.
      * exact Hcommittee.
      * apply incl_refl.
    + exact Hcommittee.
    + exact Hincl.
Qed.

Definition is_quorum_ft_gt
  (committee supporters : Committee)
  (num den : Z) : Prop :=
  incl supporters committee /\
  NoDup (map fst supporters) /\
  ft_exact_gt
    (Z.of_nat (cweight supporters))
    (Z.of_nat (cweight committee))
    num
    den.

Definition Finalized_ft_gt
  (dag : DAG)
  (committee : Committee)
  (snapshot : Snapshot)
  (candidate : BlockHash)
  (num den : Z) : Prop :=
  exists supporters,
    is_quorum_ft_gt committee supporters num den /\
    forall validator stake,
      In (validator, stake) supporters ->
      agrees dag snapshot validator candidate.

Theorem quorum_ft_gt_refines_strict_exact_support_certificate :
  forall committee supporters num den,
    NoDup (map fst committee) ->
    is_quorum_ft_gt committee supporters num den ->
    strict_exact_support_certificate
      (committee_stake committee)
      (map fst committee)
      (map fst supporters)
      num
      den.
Proof.
  intros committee supporters num den Hcommittee
    [Hincl [Hsupporters Hexact]].
  unfold strict_exact_support_certificate.
  split; [exact Hsupporters |].
  split.
  - apply mapped_committee_incl. exact Hincl.
  - rewrite committee_stake_sum_exact with
      (committee := committee) (selected := supporters).
    + rewrite committee_stake_sum_exact with
        (committee := committee) (selected := committee).
      * exact Hexact.
      * exact Hcommittee.
      * apply incl_refl.
    + exact Hcommittee.
    + exact Hincl.
Qed.

Theorem finalized_ft_refines_certified :
  forall dag committee snapshot candidate num den,
    NoDup (map fst committee) ->
    Finalized_ft dag committee snapshot candidate num den ->
    @certified
      BlockHash
      (agrees dag snapshot)
      (committee_stake committee)
      (map fst committee)
      num
      den
      candidate.
Proof.
  intros dag committee snapshot candidate num den Hcommittee
    [supporters [Hquorum Hagrees]].
  exists (map fst supporters).
  split.
  - apply quorum_ft_refines_exact_support_certificate.
    + exact Hcommittee.
    + exact Hquorum.
  - intros validator Hmember.
    apply in_map_iff in Hmember.
    destruct Hmember as [[member stake] [Heq Hsupporter]].
    simpl in Heq. subst member.
    exact (Hagrees validator stake Hsupporter).
Qed.

Section CliqueOracleAccountableSafety.

Variable dag : DAG.
Variable snapshot : Snapshot.
Variable incompatible : BlockHash -> BlockHash -> Prop.

Definition causal_incompatibility_is_accountable
  (faulty : list Validator) : Prop :=
  forall validator left right,
    incompatible left right ->
    agrees dag snapshot validator left ->
    agrees dag snapshot validator right ->
    In validator faulty.

Theorem exact_clique_certificates_are_accountably_safe :
  forall committee faulty num den left right,
    NoDup (map fst committee) ->
    NoDup faulty ->
    incl faulty (map fst committee) ->
    causal_incompatibility_is_accountable faulty ->
    Finalized_ft dag committee snapshot left num den ->
    Finalized_ft dag committee snapshot right num den ->
    incompatible left right ->
    (0 < num)%Z ->
    (0 < den)%Z ->
    (Z.of_nat
      (validator_stake (committee_stake committee) faulty) * den <
      Z.of_nat (cweight committee) * num)%Z ->
    False.
Proof.
  intros committee faulty num den left right
    Hcommittee Hfaulty Hfaulty_in Haccountable
    Hleft Hright Hincompatible Hnum Hden Hbudget.
  eapply (@certified_incompatible_candidates_exceed_fault_budget
    BlockHash
    (agrees dag snapshot)
    incompatible
    (committee_stake committee)
    (map fst committee)
    faulty
    num
    den
    left
    right).
  - exact Hcommittee.
  - exact Hfaulty.
  - exact Hfaulty_in.
  - exact Haccountable.
  - apply finalized_ft_refines_certified; assumption.
  - apply finalized_ft_refines_certified; assumption.
  - exact Hincompatible.
  - exact Hnum.
  - exact Hden.
  - rewrite committee_stake_sum_exact with
      (committee := committee) (selected := committee).
    + exact Hbudget.
    + exact Hcommittee.
    + apply incl_refl.
Qed.

Theorem strict_exact_clique_certificates_are_accountably_safe :
  forall committee faulty num den left right,
    NoDup (map fst committee) ->
    NoDup faulty ->
    incl faulty (map fst committee) ->
    causal_incompatibility_is_accountable faulty ->
    Finalized_ft_gt dag committee snapshot left num den ->
    Finalized_ft_gt dag committee snapshot right num den ->
    incompatible left right ->
    (0 < den)%Z ->
    (Z.of_nat
      (validator_stake (committee_stake committee) faulty) * den <=
      Z.of_nat (cweight committee) * num)%Z ->
    False.
Proof.
  intros committee faulty num den left right
    Hcommittee Hfaulty Hfaulty_in Haccountable
    [left_supporters [Hleft_certificate Hleft_supports]]
    [right_supporters [Hright_certificate Hright_supports]]
    Hincompatible Hden Hbudget.
  eapply strict_exact_certificates_exceed_fault_budget.
  - exact Hcommittee.
  - exact Hfaulty.
  - exact Hfaulty_in.
  - exact (@quorum_ft_gt_refines_strict_exact_support_certificate
      committee left_supporters num den Hcommittee Hleft_certificate).
  - exact (@quorum_ft_gt_refines_strict_exact_support_certificate
      committee right_supporters num den Hcommittee Hright_certificate).
  - exact Hden.
  - rewrite committee_stake_sum_exact with
      (committee := committee) (selected := committee).
    + exact Hbudget.
    + exact Hcommittee.
    + apply incl_refl.
  - intros validator Hleft_member Hright_member.
    apply Haccountable with (left := left) (right := right).
    + exact Hincompatible.
    + apply in_map_iff in Hleft_member.
      destruct Hleft_member as [[member stake] [Heq Hmember]].
      simpl in Heq. subst member.
      exact (Hleft_supports validator stake Hmember).
    + apply in_map_iff in Hright_member.
      destruct Hright_member as [[member stake] [Heq Hmember]].
      simpl in Heq. subst member.
      exact (Hright_supports validator stake Hmember).
Qed.

End CliqueOracleAccountableSafety.

Print Assumptions disjoint_validator_stake_bound.
Print Assumptions overlap_validator_stake_bound.
Print Assumptions exact_certificates_exceed_fault_budget.
Print Assumptions strict_exact_certificates_exceed_fault_budget.
Print Assumptions certified_incompatible_candidates_exceed_fault_budget.
Print Assumptions quorum_ft_refines_exact_support_certificate.
Print Assumptions finalized_ft_refines_certified.
Print Assumptions exact_clique_certificates_are_accountably_safe.
Print Assumptions strict_exact_clique_certificates_are_accountably_safe.
