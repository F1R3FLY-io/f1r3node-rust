From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.
Import ListNotations.

Definition rotating_recovery_leader
  (validator_count finalized_height recovery_round : nat)
  : nat :=
  (finalized_height + recovery_round) mod validator_count.

Definition recovery_round_authorized
  (validator_count finalized_height recovery_round proposer : nat)
  : Prop :=
  validator_count > 0 /\
  proposer = rotating_recovery_leader
    validator_count finalized_height recovery_round.

Theorem rotating_recovery_leader_in_committee :
  forall validator_count finalized_height recovery_round,
    validator_count > 0 ->
    rotating_recovery_leader
      validator_count finalized_height recovery_round < validator_count.
Proof.
  intros validator_count finalized_height recovery_round Hcount.
  unfold rotating_recovery_leader.
  apply Nat.mod_upper_bound.
  lia.
Qed.

Theorem recovery_round_authorization_unique :
  forall validator_count finalized_height recovery_round proposer_a proposer_b,
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_a ->
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros validator_count finalized_height recovery_round proposer_a proposer_b
    [_ Ha] [_ Hb].
  rewrite Ha, Hb.
  reflexivity.
Qed.

Definition committee_recovery_leader {Validator : Type}
  (committee : list Validator)
  (finalized_height recovery_round : nat)
  : option Validator :=
  nth_error committee
    (rotating_recovery_leader
      (length committee) finalized_height recovery_round).

Definition committee_recovery_round_authorized {Validator : Type}
  (committee : list Validator)
  (finalized_height recovery_round : nat)
  (proposer : Validator)
  : Prop :=
  committee <> [] /\
  committee_recovery_leader
    committee finalized_height recovery_round = Some proposer.

Theorem committee_recovery_leader_in_committee :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer,
    committee_recovery_leader
      committee finalized_height recovery_round = Some proposer ->
    In proposer committee.
Proof.
  intros
    Validator committee finalized_height recovery_round proposer Hleader.
  unfold committee_recovery_leader in Hleader.
  eapply nth_error_In.
  exact Hleader.
Qed.

Theorem committee_recovery_authorization_unique :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer_a proposer_b,
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer_a ->
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros
    Validator committee finalized_height recovery_round proposer_a proposer_b
    [_ Ha] [_ Hb].
  rewrite Ha in Hb.
  inversion Hb.
  reflexivity.
Qed.

Definition floor_recovery_round_authorized
  {Floor Validator : Type}
  (committee_of_floor : Floor -> list Validator)
  (floor : Floor)
  (finalized_height recovery_round : nat)
  (proposer : Validator)
  : Prop :=
  committee_recovery_round_authorized
    (committee_of_floor floor)
    finalized_height recovery_round proposer.

Definition floor_proposal_eligible
  {Floor Validator : Type}
  (committee_of_floor : Floor -> list Validator)
  (floor : Floor)
  (proposer : Validator)
  : Prop :=
  In proposer (committee_of_floor floor).

Theorem floor_recovery_authorization_implies_floor_eligibility :
  forall
    (Floor Validator : Type)
    (committee_of_floor : Floor -> list Validator)
    (floor : Floor)
    finalized_height recovery_round proposer,
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer ->
    floor_proposal_eligible committee_of_floor floor proposer.
Proof.
  intros
    Floor Validator committee_of_floor floor finalized_height recovery_round
    proposer [_ Hleader].
  unfold floor_proposal_eligible.
  eapply committee_recovery_leader_in_committee.
  exact Hleader.
Qed.

Theorem floor_recovery_authorization_unique_across_head_views :
  forall
    (Floor Validator Head : Type)
    (committee_of_floor : Floor -> list Validator)
    (head_committee : Head -> list Validator)
    (floor : Floor)
    (head_a head_b : Head)
    finalized_height recovery_round proposer_a proposer_b,
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer_a ->
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b.
Proof.
  intros
    Floor Validator Head committee_of_floor head_committee floor head_a head_b
    finalized_height recovery_round proposer_a proposer_b Ha Hb.
  unfold floor_recovery_round_authorized in Ha, Hb.
  eapply committee_recovery_authorization_unique; eauto.
Qed.

Definition recovery_round_at
  (stalled_for stall_timeout recovery_interval : nat)
  : option nat :=
  if stalled_for <? stall_timeout
  then None
  else Some ((stalled_for - stall_timeout) / recovery_interval).

Definition collapsed_recovery_round_at
  (stalled_for stall_timeout : nat)
  : option nat :=
  if stalled_for <? stall_timeout
  then None
  else Some ((stalled_for / stall_timeout) - 1).

Theorem recovery_round_absent_before_stall_timeout :
  forall stalled_for stall_timeout recovery_interval,
    stalled_for < stall_timeout ->
    recovery_round_at
      stalled_for stall_timeout recovery_interval = None.
Proof.
  intros stalled_for stall_timeout recovery_interval Hbefore.
  unfold recovery_round_at.
  apply Nat.ltb_lt in Hbefore.
  rewrite Hbefore.
  reflexivity.
Qed.

Theorem recovery_round_zero_at_stall_timeout :
  forall stall_timeout recovery_interval,
    recovery_round_at
      stall_timeout stall_timeout recovery_interval = Some 0.
Proof.
  intros stall_timeout recovery_interval.
  unfold recovery_round_at.
  rewrite Nat.ltb_irrefl.
  rewrite Nat.sub_diag.
  rewrite Nat.Div0.div_0_l.
  reflexivity.
Qed.

Theorem recovery_round_at_interval_boundary :
  forall stall_timeout recovery_interval recovery_round,
    recovery_interval > 0 ->
    recovery_round_at
      (stall_timeout + recovery_round * recovery_interval)
      stall_timeout
      recovery_interval = Some recovery_round.
Proof.
  intros stall_timeout recovery_interval recovery_round Hinterval.
  unfold recovery_round_at.
  destruct
    (stall_timeout + recovery_round * recovery_interval <? stall_timeout)
    eqn:Hbefore.
  - apply Nat.ltb_lt in Hbefore.
    lia.
  -
  replace
    (stall_timeout + recovery_round * recovery_interval - stall_timeout)
    with (recovery_round * recovery_interval)
    by lia.
  rewrite Nat.div_mul by lia.
  reflexivity.
Qed.

Theorem separated_cadence_repairs_collapsed_timeout :
  recovery_round_at 20 15 5 = Some 1 /\
  collapsed_recovery_round_at 20 15 = Some 0.
Proof.
  vm_compute.
  auto.
Qed.

Fixpoint earliest_uncompleted_round_up_to
  (current_round : nat)
  (completed_rounds : list nat)
  : option nat :=
  match current_round with
  | 0 =>
      if in_dec Nat.eq_dec 0 completed_rounds
      then None
      else Some 0
  | S previous_round =>
      match earliest_uncompleted_round_up_to
        previous_round completed_rounds with
      | Some round => Some round
      | None =>
          if in_dec Nat.eq_dec current_round completed_rounds
          then None
          else Some current_round
      end
  end.

Definition earliest_due_recovery_round
  (stalled_for stall_timeout recovery_interval : nat)
  (completed_rounds : list nat)
  : option nat :=
  match recovery_round_at
    stalled_for stall_timeout recovery_interval with
  | None => None
  | Some current_round =>
      earliest_uncompleted_round_up_to current_round completed_rounds
  end.

Theorem earliest_uncompleted_round_is_due_and_uncompleted :
  forall current_round completed_rounds selected_round,
    earliest_uncompleted_round_up_to
      current_round completed_rounds = Some selected_round ->
    selected_round <= current_round /\
    ~ In selected_round completed_rounds.
Proof.
  induction current_round as [| previous_round IH];
    intros completed_rounds selected_round Hselected.
  - simpl in Hselected.
    destruct (in_dec Nat.eq_dec 0 completed_rounds) as [Hcompleted | Huncompleted].
    + discriminate.
    + inversion Hselected; subst selected_round.
      split; [lia | exact Huncompleted].
  - simpl in Hselected.
    destruct
      (earliest_uncompleted_round_up_to previous_round completed_rounds)
      as [earlier_round |] eqn:Hearlier.
    + inversion Hselected; subst selected_round.
      specialize (IH completed_rounds earlier_round Hearlier)
        as [Hdue Huncompleted].
      split; [lia | exact Huncompleted].
    + destruct
        (in_dec Nat.eq_dec (S previous_round) completed_rounds)
        as [Hcompleted | Huncompleted].
      * discriminate.
      * inversion Hselected; subst selected_round.
        split; [lia | exact Huncompleted].
Qed.

Theorem no_uncompleted_round_exists_when_earliest_is_absent :
  forall current_round completed_rounds,
    earliest_uncompleted_round_up_to
      current_round completed_rounds = None ->
    forall round,
      round <= current_round ->
      In round completed_rounds.
Proof.
  induction current_round as [| previous_round IH];
    intros completed_rounds Hnone round Hdue.
  - assert (round = 0) by lia.
    subst round.
    simpl in Hnone.
    destruct (in_dec Nat.eq_dec 0 completed_rounds) as [Hcompleted | Huncompleted].
    + exact Hcompleted.
    + discriminate.
  - simpl in Hnone.
    destruct
      (earliest_uncompleted_round_up_to previous_round completed_rounds)
      as [earlier_round |] eqn:Hearlier.
    + discriminate.
    + destruct
        (in_dec Nat.eq_dec (S previous_round) completed_rounds)
        as [Hcompleted | Huncompleted].
      * destruct (Nat.eq_dec round (S previous_round)) as [Heq | Hneq].
        -- subst round.
           exact Hcompleted.
        -- apply IH with (completed_rounds := completed_rounds).
           ++ exact Hearlier.
           ++ lia.
      * discriminate.
Qed.

Theorem earliest_uncompleted_round_is_minimal :
  forall current_round completed_rounds selected_round,
    earliest_uncompleted_round_up_to
      current_round completed_rounds = Some selected_round ->
    forall earlier_round,
      earlier_round < selected_round ->
      In earlier_round completed_rounds.
Proof.
  induction current_round as [| previous_round IH];
    intros completed_rounds selected_round Hselected earlier_round Hearlier.
  - simpl in Hselected.
    destruct (in_dec Nat.eq_dec 0 completed_rounds).
    + discriminate.
    + inversion Hselected; subst selected_round.
      lia.
  - simpl in Hselected.
    destruct
      (earliest_uncompleted_round_up_to previous_round completed_rounds)
      as [selected_previous |] eqn:Hprevious.
    + inversion Hselected; subst selected_round.
      eapply IH; eauto.
    + destruct
        (in_dec Nat.eq_dec (S previous_round) completed_rounds)
        as [Hcompleted | Huncompleted].
      * discriminate.
      * inversion Hselected; subst selected_round.
        eapply no_uncompleted_round_exists_when_earliest_is_absent.
        -- exact Hprevious.
        -- lia.
Qed.

Theorem an_uncompleted_due_round_is_never_skipped :
  forall current_round completed_rounds,
    (exists round,
      round <= current_round /\
      ~ In round completed_rounds) ->
    exists selected_round,
      earliest_uncompleted_round_up_to
        current_round completed_rounds = Some selected_round.
Proof.
  intros current_round completed_rounds [round [Hdue Huncompleted]].
  destruct
    (earliest_uncompleted_round_up_to current_round completed_rounds)
    as [selected_round |] eqn:Hselected.
  - exists selected_round.
    reflexivity.
  - exfalso.
    apply Huncompleted.
    eapply no_uncompleted_round_exists_when_earliest_is_absent;
      eauto.
Qed.

Theorem earliest_due_round_retains_skipped_wake_opportunities :
  earliest_due_recovery_round 30 15 5 [0] = Some 1 /\
  recovery_round_at 30 15 5 = Some 3.
Proof.
  vm_compute.
  auto.
Qed.

Theorem earliest_due_round_is_within_elapsed_frontier :
  forall
    stalled_for stall_timeout recovery_interval completed_rounds
    selected_round,
    earliest_due_recovery_round
      stalled_for stall_timeout recovery_interval completed_rounds =
        Some selected_round ->
    exists current_round,
      recovery_round_at
        stalled_for stall_timeout recovery_interval = Some current_round /\
      selected_round <= current_round /\
      ~ In selected_round completed_rounds.
Proof.
  intros
    stalled_for stall_timeout recovery_interval completed_rounds
    selected_round Hselected.
  unfold earliest_due_recovery_round in Hselected.
  destruct
    (recovery_round_at stalled_for stall_timeout recovery_interval)
    as [current_round |] eqn:Hcurrent.
  - exists current_round.
    split; [reflexivity |].
    eapply earliest_uncompleted_round_is_due_and_uncompleted.
    exact Hselected.
  - discriminate.
Qed.

Section RecoveryEvidence.

Context {Validator Block Node : Type}.
Variable validator_eq_dec :
  forall left right : Validator, {left = right} + {left <> right}.
Variable creator : Block -> Validator.
Variable descends_from : Block -> Block -> Prop.
Variable state_descends_from : Block -> Block -> Prop.
Variable latest_at : Node -> Validator -> option Block.
Variable captured_view : Block -> Validator -> option Block.

Hypothesis descends_from_trans :
  forall descendant middle target,
    descends_from descendant middle ->
    descends_from middle target ->
    descends_from descendant target.

Hypothesis state_descends_from_trans :
  forall descendant middle target,
    state_descends_from descendant middle ->
    state_descends_from middle target ->
    state_descends_from descendant target.

Hypothesis state_descends_from_refines_causal :
  forall descendant target,
    state_descends_from descendant target ->
    descends_from descendant target.

Definition delivered_latest_at
  (node : Node)
  (validator : Validator)
  (block : Block)
  : Prop :=
  latest_at node validator = Some block.

Definition captures_latest
  (block : Block)
  (validator : Validator)
  (seen_block : Block)
  : Prop :=
  captured_view block validator = Some seen_block.

Definition selected_recovery_layer
  (committee : list Validator)
  (finalized_height recovery_round : nat)
  (proposer : Validator)
  (block previous : Block)
  : Prop :=
  committee_recovery_round_authorized
    committee finalized_height recovery_round proposer /\
  creator block = proposer /\
  descends_from block previous.

Definition supports_at
  (relation : Block -> Block -> Prop)
  (node : Node)
  (target : Block)
  (observer subject : Validator)
  : Prop :=
  match latest_at node observer with
  | None => False
  | Some observer_latest =>
      if validator_eq_dec observer subject
      then relation observer_latest target
      else exists seen_block,
        captured_view observer_latest subject = Some seen_block /\
        relation seen_block target
  end.

Definition mutual_clique_at
  (relation : Block -> Block -> Prop)
  (node : Node)
  (target : Block)
  (supporters : list Validator)
  : Prop :=
  forall observer subject,
    In observer supporters ->
    In subject supporters ->
    supports_at relation node target observer subject.

Theorem selected_recovery_layer_uses_height_offset_leader :
  forall
    committee finalized_height recovery_round proposer block previous,
    selected_recovery_layer
      committee finalized_height recovery_round proposer block previous ->
    committee_recovery_leader
      committee finalized_height recovery_round = Some proposer /\
    In proposer committee /\
    creator block = proposer.
Proof.
  intros
    committee finalized_height recovery_round proposer block previous
    [[Hnonempty Hleader] [Hcreator Hdescends]].
  split; [exact Hleader |].
  split.
  - eapply committee_recovery_leader_in_committee.
    exact Hleader.
  - exact Hcreator.
Qed.

Theorem two_validator_support_forms_mutual_clique :
  forall relation node target validator_a validator_b,
    supports_at relation node target validator_a validator_a ->
    supports_at relation node target validator_a validator_b ->
    supports_at relation node target validator_b validator_a ->
    supports_at relation node target validator_b validator_b ->
    mutual_clique_at
      relation node target [validator_a; validator_b].
Proof.
  intros
    relation node target validator_a validator_b
    Haa Hab Hba Hbb observer subject Hobserver Hsubject.
  simpl in Hobserver, Hsubject.
  destruct Hobserver as [Hobserver | [Hobserver | Hfalse]].
  - subst observer.
    destruct Hsubject as [Hsubject | [Hsubject | Hfalse]].
    + subst subject.
      exact Haa.
    + subst subject.
      exact Hab.
    + contradiction.
  - subst observer.
    destruct Hsubject as [Hsubject | [Hsubject | Hfalse]].
    + subst subject.
      exact Hba.
    + subst subject.
      exact Hbb.
    + contradiction.
  - contradiction.
Qed.

Theorem mutual_state_clique_refines_causal_clique :
  forall node target supporters,
    mutual_clique_at
      state_descends_from node target supporters ->
    mutual_clique_at
      descends_from node target supporters.
Proof.
  intros node target supporters Hstate observer subject Hobserver Hsubject.
  specialize (Hstate observer subject Hobserver Hsubject).
  unfold supports_at in Hstate |- *.
  destruct (latest_at node observer) as [observer_latest |].
  - destruct (validator_eq_dec observer subject) as [Hequal | Hdistinct].
    + apply state_descends_from_refines_causal.
      exact Hstate.
    + destruct Hstate as [seen_block [Hview Hstate]].
      exists seen_block.
      split; [exact Hview |].
      apply state_descends_from_refines_causal.
      exact Hstate.
  - contradiction.
Qed.

Theorem delivered_selected_layers_form_dual_mutual_cliques :
  forall
    committee finalized_height
    first_round second_round third_round
    node validator_a validator_b
    target first_layer second_layer third_layer,
    validator_a <> validator_b ->
    selected_recovery_layer
      committee finalized_height first_round
      validator_a first_layer target ->
    selected_recovery_layer
      committee finalized_height second_round
      validator_b second_layer first_layer ->
    selected_recovery_layer
      committee finalized_height third_round
      validator_a third_layer second_layer ->
    delivered_latest_at node validator_a third_layer ->
    delivered_latest_at node validator_b second_layer ->
    captures_latest third_layer validator_b second_layer ->
    captures_latest second_layer validator_a first_layer ->
    state_descends_from first_layer target ->
    state_descends_from second_layer first_layer ->
    state_descends_from third_layer second_layer ->
    In validator_a committee /\
    In validator_b committee /\
    committee_recovery_leader
      committee finalized_height first_round = Some validator_a /\
    committee_recovery_leader
      committee finalized_height second_round = Some validator_b /\
    committee_recovery_leader
      committee finalized_height third_round = Some validator_a /\
    creator first_layer = validator_a /\
    creator second_layer = validator_b /\
    creator third_layer = validator_a /\
    mutual_clique_at
      descends_from node target [validator_a; validator_b] /\
    mutual_clique_at
      state_descends_from node target [validator_a; validator_b].
Proof.
  intros
    committee finalized_height first_round second_round third_round
    node validator_a validator_b target first_layer second_layer third_layer
    Hdistinct
    Hfirst_selected Hsecond_selected Hthird_selected
    Hlatest_a Hlatest_b Hview_a_b Hview_b_a
    Hfirst_state Hsecond_state Hthird_state.
  destruct Hfirst_selected as
    [[Hfirst_nonempty Hfirst_leader] [Hfirst_creator Hfirst_descends]].
  destruct Hsecond_selected as
    [[Hsecond_nonempty Hsecond_leader] [Hsecond_creator Hsecond_descends]].
  destruct Hthird_selected as
    [[Hthird_nonempty Hthird_leader] [Hthird_creator Hthird_descends]].
  assert (Hvalidator_a : In validator_a committee).
  {
    eapply committee_recovery_leader_in_committee.
    exact Hfirst_leader.
  }
  assert (Hvalidator_b : In validator_b committee).
  {
    eapply committee_recovery_leader_in_committee.
    exact Hsecond_leader.
  }
  assert (Hsecond_target : descends_from second_layer target).
  {
    eapply descends_from_trans; eauto.
  }
  assert (Hthird_target : descends_from third_layer target).
  {
    eapply descends_from_trans; eauto.
  }
  assert (Hsecond_state_target : state_descends_from second_layer target).
  {
    eapply state_descends_from_trans; eauto.
  }
  assert (Hthird_state_target : state_descends_from third_layer target).
  {
    eapply state_descends_from_trans; eauto.
  }
  unfold delivered_latest_at in Hlatest_a, Hlatest_b.
  unfold captures_latest in Hview_a_b, Hview_b_a.
  assert
    (Hcausal_aa :
      supports_at descends_from node target validator_a validator_a).
  {
    unfold supports_at.
    rewrite Hlatest_a.
    destruct (validator_eq_dec validator_a validator_a)
      as [Hequal | Hunequal].
    - exact Hthird_target.
    - contradiction.
  }
  assert
    (Hcausal_ab :
      supports_at descends_from node target validator_a validator_b).
  {
    unfold supports_at.
    rewrite Hlatest_a.
    destruct (validator_eq_dec validator_a validator_b)
      as [Hequal | Hunequal].
    - contradiction.
    - exists second_layer.
      split; assumption.
  }
  assert
    (Hcausal_ba :
      supports_at descends_from node target validator_b validator_a).
  {
    unfold supports_at.
    rewrite Hlatest_b.
    destruct (validator_eq_dec validator_b validator_a)
      as [Hequal | Hunequal].
    - exfalso.
      apply Hdistinct.
      symmetry.
      exact Hequal.
    - exists first_layer.
      split; assumption.
  }
  assert
    (Hcausal_bb :
      supports_at descends_from node target validator_b validator_b).
  {
    unfold supports_at.
    rewrite Hlatest_b.
    destruct (validator_eq_dec validator_b validator_b)
      as [Hequal | Hunequal].
    - exact Hsecond_target.
    - contradiction.
  }
  assert
    (Hstate_aa :
      supports_at state_descends_from node target validator_a validator_a).
  {
    unfold supports_at.
    rewrite Hlatest_a.
    destruct (validator_eq_dec validator_a validator_a)
      as [Hequal | Hunequal].
    - exact Hthird_state_target.
    - contradiction.
  }
  assert
    (Hstate_ab :
      supports_at state_descends_from node target validator_a validator_b).
  {
    unfold supports_at.
    rewrite Hlatest_a.
    destruct (validator_eq_dec validator_a validator_b)
      as [Hequal | Hunequal].
    - contradiction.
    - exists second_layer.
      split; assumption.
  }
  assert
    (Hstate_ba :
      supports_at state_descends_from node target validator_b validator_a).
  {
    unfold supports_at.
    rewrite Hlatest_b.
    destruct (validator_eq_dec validator_b validator_a)
      as [Hequal | Hunequal].
    - exfalso.
      apply Hdistinct.
      symmetry.
      exact Hequal.
    - exists first_layer.
      split; assumption.
  }
  assert
    (Hstate_bb :
      supports_at state_descends_from node target validator_b validator_b).
  {
    unfold supports_at.
    rewrite Hlatest_b.
    destruct (validator_eq_dec validator_b validator_b)
      as [Hequal | Hunequal].
    - exact Hsecond_state_target.
    - contradiction.
  }
  repeat split.
  - exact Hvalidator_a.
  - exact Hvalidator_b.
  - exact Hfirst_leader.
  - exact Hsecond_leader.
  - exact Hthird_leader.
  - exact Hfirst_creator.
  - exact Hsecond_creator.
  - exact Hthird_creator.
  - eapply two_validator_support_forms_mutual_clique; eauto.
  - eapply two_validator_support_forms_mutual_clique; eauto.
Qed.

End RecoveryEvidence.

Definition recovery_evidence_contract : Prop :=
  committee_recovery_leader [0; 1] 0 0 = Some 0 /\
  committee_recovery_leader [0; 1] 0 1 = Some 1 /\
  committee_recovery_leader [0; 1] 0 2 = Some 0 /\
  forall
    (Validator Block Node : Type)
    (validator_eq_dec :
      forall left right : Validator, {left = right} + {left <> right})
    (creator : Block -> Validator)
    (descends_from state_descends_from : Block -> Block -> Prop)
    (latest_at : Node -> Validator -> option Block)
    (captured_view : Block -> Validator -> option Block),
    (forall descendant middle target,
      descends_from descendant middle ->
      descends_from middle target ->
      descends_from descendant target) ->
    (forall descendant middle target,
      state_descends_from descendant middle ->
      state_descends_from middle target ->
      state_descends_from descendant target) ->
    forall
      committee finalized_height first_round second_round third_round
      node validator_a validator_b
      target first_layer second_layer third_layer,
      validator_a <> validator_b ->
      selected_recovery_layer
        creator descends_from
        committee finalized_height first_round
        validator_a first_layer target ->
      selected_recovery_layer
        creator descends_from
        committee finalized_height second_round
        validator_b second_layer first_layer ->
      selected_recovery_layer
        creator descends_from
        committee finalized_height third_round
        validator_a third_layer second_layer ->
      delivered_latest_at latest_at node validator_a third_layer ->
      delivered_latest_at latest_at node validator_b second_layer ->
      captures_latest captured_view third_layer validator_b second_layer ->
      captures_latest captured_view second_layer validator_a first_layer ->
      state_descends_from first_layer target ->
      state_descends_from second_layer first_layer ->
      state_descends_from third_layer second_layer ->
      In validator_a committee /\
      In validator_b committee /\
      committee_recovery_leader
        committee finalized_height first_round = Some validator_a /\
      committee_recovery_leader
        committee finalized_height second_round = Some validator_b /\
      committee_recovery_leader
        committee finalized_height third_round = Some validator_a /\
      creator first_layer = validator_a /\
      creator second_layer = validator_b /\
      creator third_layer = validator_a /\
      mutual_clique_at
        validator_eq_dec latest_at captured_view descends_from
        node target [validator_a; validator_b] /\
      mutual_clique_at
        validator_eq_dec latest_at captured_view state_descends_from
        node target [validator_a; validator_b].

Theorem recovery_evidence_end_to_end :
  recovery_evidence_contract.
Proof.
  unfold recovery_evidence_contract.
  split.
  - reflexivity.
  - split.
    + reflexivity.
    + split.
      * reflexivity.
      * intros
          Validator Block Node validator_eq_dec creator
          descends_from state_descends_from latest_at captured_view
          Hcausal_trans Hstate_trans
          committee finalized_height first_round second_round third_round
          node validator_a validator_b
          target first_layer second_layer third_layer
          Hdistinct Hfirst Hsecond Hthird
          Hlatest_a Hlatest_b Hview_a_b Hview_b_a
          Hfirst_state Hsecond_state Hthird_state.
        eapply
          (@delivered_selected_layers_form_dual_mutual_cliques
            Validator Block Node
            validator_eq_dec creator descends_from state_descends_from
            latest_at captured_view Hcausal_trans Hstate_trans);
          eauto.
Qed.

Inductive proposal_result : Type :=
| ProposalEmpty
| ProposalDeferred
| ProposalFailed
| ProposalStarted
| ProposalSuccess.

Definition proposal_result_starts
  (result : proposal_result)
  : bool :=
  match result with
  | ProposalStarted | ProposalSuccess => true
  | ProposalEmpty | ProposalDeferred | ProposalFailed => false
  end.

Inductive proposal_reservation : Type :=
| PendingReservation (recovery_round : option nat)
| RecoveryReservation (recovery_round : nat).

Definition reserved_recovery_round
  (reservation : proposal_reservation)
  : option nat :=
  match reservation with
  | PendingReservation recovery_round => recovery_round
  | RecoveryReservation recovery_round => Some recovery_round
  end.

Inductive automatic_proposal_reason : Type :=
| PendingWork
| RecoverySupport (recovery_round : nat)
| EagerAutomaticSupport.

Definition automatic_proposal_authorized {Validator : Type}
  (committee : list Validator)
  (finalized_height : nat)
  (proposer : Validator)
  (has_pending_work : bool)
  (reason : automatic_proposal_reason)
  : Prop :=
  match reason with
  | PendingWork => has_pending_work = true
  | RecoverySupport recovery_round =>
      has_pending_work = false /\
      committee_recovery_round_authorized
        committee finalized_height recovery_round proposer
  | EagerAutomaticSupport => False
  end.

Definition proposal_reservation_authorized {Validator : Type}
  (committee : list Validator)
  (finalized_height : nat)
  (proposer : Validator)
  (has_pending_work : bool)
  (reservation : proposal_reservation)
  : Prop :=
  match reservation with
  | PendingReservation None => has_pending_work = true
  | PendingReservation (Some recovery_round) =>
      has_pending_work = true /\
      committee_recovery_round_authorized
        committee finalized_height recovery_round proposer
  | RecoveryReservation recovery_round =>
      has_pending_work = false /\
      committee_recovery_round_authorized
        committee finalized_height recovery_round proposer
  end.

Definition nonleader_skip_authorized {Validator : Type}
  (committee : list Validator)
  (finalized_height recovery_round : nat)
  (proposer : Validator)
  : Prop :=
  In proposer committee /\
  ~ committee_recovery_round_authorized
      committee finalized_height recovery_round proposer.

Record finality_progress := {
  observed_floor : nat;
  last_completed_round : option nat;
  active_reservation : option proposal_reservation
}.

Definition next_uncompleted_round
  (progress : finality_progress)
  : nat :=
  match last_completed_round progress with
  | None => 0
  | Some completed => S completed
  end.

Definition recovery_round_due
  (progress : finality_progress)
  (recovery_round : nat)
  : bool :=
  match active_reservation progress with
  | Some _ => false
  | None => Nat.eqb (next_uncompleted_round progress) recovery_round
  end.

Definition attempt_proposal
  (progress : finality_progress)
  (reservation : proposal_reservation)
  : option finality_progress :=
  match active_reservation progress with
  | Some _ => None
  | None =>
      match reserved_recovery_round reservation with
      | Some recovery_round =>
          if recovery_round_due progress recovery_round
          then Some
            {| observed_floor := observed_floor progress;
               last_completed_round := last_completed_round progress;
               active_reservation := Some reservation |}
          else None
      | None => Some
          {| observed_floor := observed_floor progress;
             last_completed_round := last_completed_round progress;
             active_reservation := Some reservation |}
      end
  end.

Definition clear_proposal_reservation
  (progress : finality_progress)
  : finality_progress :=
  {| observed_floor := observed_floor progress;
     last_completed_round := last_completed_round progress;
     active_reservation := None |}.

Definition record_recovery_completion
  (progress : finality_progress)
  (recovery_round : nat)
  : finality_progress :=
  if recovery_round_due progress recovery_round
  then {| observed_floor := observed_floor progress;
          last_completed_round := Some recovery_round;
          active_reservation := active_reservation progress |}
  else progress.

Definition resolve_proposal
  (progress : finality_progress)
  (result : proposal_result)
  : finality_progress :=
  match active_reservation progress with
  | None => progress
  | Some reservation =>
      let cleared := clear_proposal_reservation progress in
      match reserved_recovery_round reservation with
      | Some recovery_round =>
          if proposal_result_starts result
          then record_recovery_completion cleared recovery_round
          else cleared
      | None => cleared
      end
  end.

Definition record_nonleader_skip
  (progress : finality_progress)
  (recovery_round : nat)
  : finality_progress :=
  record_recovery_completion progress recovery_round.

Definition observe_floor
  (progress : finality_progress)
  (current_floor : nat)
  : finality_progress :=
  if Nat.eqb (observed_floor progress) current_floor
  then progress
  else {| observed_floor := current_floor;
          last_completed_round := None;
          active_reservation := None |}.

Definition completed_round_prefix
  (progress : finality_progress)
  : list nat :=
  match last_completed_round progress with
  | None => []
  | Some completed => seq 0 (S completed)
  end.

Theorem proposal_attempt_reserves_without_completion :
  forall progress reservation attempted,
    attempt_proposal progress reservation = Some attempted ->
    last_completed_round attempted = last_completed_round progress /\
    active_reservation attempted = Some reservation.
Proof.
  intros progress reservation attempted Hattempt.
  destruct progress as [floor completed active].
  unfold attempt_proposal in Hattempt.
  simpl in Hattempt.
  destruct active as [active |]; [discriminate |].
  destruct (reserved_recovery_round reservation) as [recovery_round |].
  - destruct (recovery_round_due
      {| observed_floor := floor;
         last_completed_round := completed;
         active_reservation := None |}
      recovery_round) eqn:Hdue;
      inversion Hattempt; subst attempted; simpl; auto.
  - inversion Hattempt; subst attempted; simpl; auto.
Qed.

Theorem active_reservation_serializes_proposals :
  forall progress reservation attempted next_reservation,
    attempt_proposal progress reservation = Some attempted ->
    attempt_proposal attempted next_reservation = None.
Proof.
  intros progress reservation attempted next_reservation Hattempt.
  pose proof
    (proposal_attempt_reserves_without_completion
      progress reservation attempted Hattempt) as [_ Hactive].
  unfold attempt_proposal.
  rewrite Hactive.
  reflexivity.
Qed.

Theorem attempt_then_clear_restores_progress :
  forall progress reservation attempted,
    attempt_proposal progress reservation = Some attempted ->
    clear_proposal_reservation attempted = progress.
Proof.
  intros progress reservation attempted Hattempt.
  destruct progress as [floor completed active].
  unfold attempt_proposal in Hattempt.
  simpl in Hattempt.
  destruct active as [active |]; [discriminate |].
  destruct (reserved_recovery_round reservation) as [recovery_round |].
  - destruct (recovery_round_due
      {| observed_floor := floor;
         last_completed_round := completed;
         active_reservation := None |}
      recovery_round) eqn:Hdue;
      inversion Hattempt; subst attempted; reflexivity.
  - inversion Hattempt; subst attempted; reflexivity.
Qed.

Theorem nonstarting_results_release_without_completion :
  forall progress reservation attempted,
    attempt_proposal progress reservation = Some attempted ->
    resolve_proposal attempted ProposalEmpty = progress /\
    resolve_proposal attempted ProposalDeferred = progress /\
    resolve_proposal attempted ProposalFailed = progress.
Proof.
  intros progress reservation attempted Hattempt.
  pose proof
    (proposal_attempt_reserves_without_completion
      progress reservation attempted Hattempt) as [_ Hactive].
  pose proof
    (attempt_then_clear_restores_progress
      progress reservation attempted Hattempt) as Hclear.
  unfold resolve_proposal.
  rewrite Hactive.
  destruct (reserved_recovery_round reservation);
    simpl; rewrite Hclear; auto.
Qed.

Theorem pending_without_recovery_releases_without_completion :
  forall progress attempted,
    attempt_proposal
      progress (PendingReservation None) = Some attempted ->
    resolve_proposal attempted ProposalStarted = progress /\
    resolve_proposal attempted ProposalSuccess = progress.
Proof.
  intros progress attempted Hattempt.
  pose proof
    (proposal_attempt_reserves_without_completion
      progress (PendingReservation None) attempted Hattempt)
    as [_ Hactive].
  pose proof
    (attempt_then_clear_restores_progress
      progress (PendingReservation None) attempted Hattempt)
    as Hclear.
  unfold resolve_proposal.
  rewrite Hactive.
  simpl.
  rewrite Hclear.
  auto.
Qed.

Theorem attempted_reserved_round_was_due :
  forall progress reservation attempted recovery_round,
    attempt_proposal progress reservation = Some attempted ->
    reserved_recovery_round reservation = Some recovery_round ->
    recovery_round_due progress recovery_round = true.
Proof.
  intros progress reservation attempted recovery_round Hattempt Hround.
  destruct progress as [floor completed active].
  unfold attempt_proposal in Hattempt.
  simpl in Hattempt.
  destruct active as [active |]; [discriminate |].
  rewrite Hround in Hattempt.
  destruct (recovery_round_due
    {| observed_floor := floor;
       last_completed_round := completed;
       active_reservation := None |}
    recovery_round) eqn:Hdue;
    [reflexivity | discriminate].
Qed.

Theorem ordered_completion_closes_due_round :
  forall progress recovery_round,
    recovery_round_due progress recovery_round = true ->
    last_completed_round
      (record_recovery_completion progress recovery_round) =
        Some recovery_round /\
    recovery_round_due
      (record_recovery_completion progress recovery_round)
      recovery_round = false.
Proof.
  intros [floor completed active] recovery_round Hdue.
  destruct active as [reservation |]; [discriminate |].
  destruct completed as [completed |].
  - simpl in Hdue.
    apply Nat.eqb_eq in Hdue.
    subst recovery_round.
    unfold record_recovery_completion.
    unfold recovery_round_due, next_uncompleted_round.
    simpl.
    rewrite Nat.eqb_refl.
    split; [reflexivity |].
    change (Nat.eqb (S completed) completed = false).
    apply Nat.eqb_neq.
    exact (Nat.neq_succ_diag_l completed).
  - simpl in Hdue.
    apply Nat.eqb_eq in Hdue.
    subst recovery_round.
    unfold record_recovery_completion.
    unfold recovery_round_due, next_uncompleted_round.
    simpl.
    split; [reflexivity | reflexivity].
Qed.

Theorem completion_outside_due_order_is_ignored :
  forall progress recovery_round,
    recovery_round_due progress recovery_round = false ->
    record_recovery_completion progress recovery_round = progress.
Proof.
  intros progress recovery_round Hnot_due.
  unfold record_recovery_completion.
  rewrite Hnot_due.
  reflexivity.
Qed.

Theorem starting_results_complete_attached_round :
  forall progress reservation attempted recovery_round,
    attempt_proposal progress reservation = Some attempted ->
    reserved_recovery_round reservation = Some recovery_round ->
    (last_completed_round
       (resolve_proposal attempted ProposalStarted) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalStarted)
       recovery_round = false) /\
    (last_completed_round
       (resolve_proposal attempted ProposalSuccess) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalSuccess)
       recovery_round = false).
Proof.
  intros progress reservation attempted recovery_round Hattempt Hround.
  pose proof
    (proposal_attempt_reserves_without_completion
      progress reservation attempted Hattempt) as [_ Hactive].
  pose proof
    (attempt_then_clear_restores_progress
      progress reservation attempted Hattempt) as Hclear.
  pose proof
    (attempted_reserved_round_was_due
      progress reservation attempted recovery_round Hattempt Hround)
    as Hdue.
  pose proof
    (ordered_completion_closes_due_round progress recovery_round Hdue)
    as Hcompleted.
  unfold resolve_proposal.
  rewrite Hactive, Hround.
  simpl.
  rewrite Hclear.
  auto.
Qed.

Theorem authorized_pending_recovery_composes_one_proposal :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height proposer recovery_round
    progress attempted,
    proposal_reservation_authorized
      committee finalized_height proposer true
      (PendingReservation (Some recovery_round)) ->
    attempt_proposal
      progress (PendingReservation (Some recovery_round)) = Some attempted ->
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer /\
    (last_completed_round
       (resolve_proposal attempted ProposalStarted) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalStarted)
       recovery_round = false) /\
    (last_completed_round
       (resolve_proposal attempted ProposalSuccess) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalSuccess)
       recovery_round = false).
Proof.
  intros
    Validator committee finalized_height proposer recovery_round
    progress attempted Hauthorized Hattempt.
  simpl in Hauthorized.
  destruct Hauthorized as [_ Hleader].
  split; [exact Hleader |].
  eapply starting_results_complete_attached_round; eauto.
Qed.

Theorem authorized_nonleader_skip_completes_without_reservation :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer progress,
    nonleader_skip_authorized
      committee finalized_height recovery_round proposer ->
    recovery_round_due progress recovery_round = true ->
    last_completed_round
      (record_nonleader_skip progress recovery_round) = Some recovery_round /\
    active_reservation
      (record_nonleader_skip progress recovery_round) = None.
Proof.
  intros
    Validator committee finalized_height recovery_round proposer progress
    Hnonleader Hdue.
  destruct Hnonleader as [_ Hnot_leader].
  unfold record_nonleader_skip.
  pose proof
    (ordered_completion_closes_due_round progress recovery_round Hdue)
    as [Hcompleted Hclosed].
  split; [exact Hcompleted |].
  destruct progress as [floor completed active].
  unfold recovery_round_due in Hdue.
  destruct active; [discriminate |].
  unfold record_recovery_completion.
  destruct
    (recovery_round_due
      {| observed_floor := floor;
         last_completed_round := completed;
         active_reservation := None |}
      recovery_round);
    reflexivity.
Qed.

Theorem eager_automatic_support_is_never_authorized :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height proposer has_pending_work,
    ~ automatic_proposal_authorized
        committee finalized_height proposer has_pending_work
        EagerAutomaticSupport.
Proof.
  intros.
  unfold automatic_proposal_authorized.
  auto.
Qed.

Theorem authorized_recovery_support_uses_selected_leader :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer has_pending_work,
    automatic_proposal_authorized
      committee finalized_height proposer has_pending_work
      (RecoverySupport recovery_round) ->
    has_pending_work = false /\
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer.
Proof.
  intros.
  exact H.
Qed.

Theorem authorized_recovery_reservation_uses_selected_leader :
  forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer has_pending_work,
    proposal_reservation_authorized
      committee finalized_height proposer has_pending_work
      (RecoveryReservation recovery_round) ->
    has_pending_work = false /\
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer.
Proof.
  intros.
  exact H.
Qed.

Theorem unchanged_floor_preserves_recovery_history :
  forall progress,
    observe_floor progress (observed_floor progress) = progress.
Proof.
  intros progress.
  unfold observe_floor.
  rewrite Nat.eqb_refl.
  reflexivity.
Qed.

Theorem advanced_floor_resets_recovery_history :
  forall progress current_floor,
    observed_floor progress <> current_floor ->
    last_completed_round (observe_floor progress current_floor) = None /\
    active_reservation (observe_floor progress current_floor) = None.
Proof.
  intros progress current_floor Hchanged.
  unfold observe_floor.
  apply Nat.eqb_neq in Hchanged.
  rewrite Hchanged.
  auto.
Qed.

Theorem all_completed_rounds_make_earliest_absent :
  forall current_round completed_rounds,
    (forall recovery_round,
      recovery_round <= current_round ->
      In recovery_round completed_rounds) ->
    earliest_uncompleted_round_up_to
      current_round completed_rounds = None.
Proof.
  induction current_round as [| previous_round IH];
    intros completed_rounds Hcompleted.
  - simpl.
    destruct (in_dec Nat.eq_dec 0 completed_rounds) as [Hin | Hnot_in].
    + reflexivity.
    + exfalso.
      apply Hnot_in.
      apply Hcompleted.
      lia.
  - simpl.
    rewrite IH.
    + destruct
        (in_dec Nat.eq_dec (S previous_round) completed_rounds)
        as [Hin | Hnot_in].
      * reflexivity.
      * exfalso.
        apply Hnot_in.
        apply Hcompleted.
        lia.
    + intros recovery_round Hle.
      apply Hcompleted.
      lia.
Qed.

Theorem completed_prefix_contains_every_completed_round :
  forall completed recovery_round,
    recovery_round <= completed ->
    In recovery_round (seq 0 (S completed)).
Proof.
  intros completed recovery_round Hle.
  apply in_seq.
  lia.
Qed.

Theorem earliest_uncompleted_round_follows_completed_prefix :
  forall completed,
    earliest_uncompleted_round_up_to
      (S completed) (seq 0 (S completed)) = Some (S completed).
Proof.
  intros completed.
  destruct
    (earliest_uncompleted_round_up_to
      (S completed) (seq 0 (S completed)))
    as [selected_round |] eqn:Hselected.
  - pose proof
      (earliest_uncompleted_round_is_due_and_uncompleted
        (S completed) (seq 0 (S completed)) selected_round Hselected)
      as [Hdue Huncompleted].
    assert (selected_round = S completed).
    {
      destruct (Nat.eq_dec selected_round (S completed)) as [Hequal | Hneq].
      - exact Hequal.
      - exfalso.
        apply Huncompleted.
        apply completed_prefix_contains_every_completed_round.
        lia.
    }
    subst selected_round.
    reflexivity.
  - exfalso.
    pose proof
      (no_uncompleted_round_exists_when_earliest_is_absent
        (S completed) (seq 0 (S completed)) Hselected
        (S completed) (le_n (S completed))) as Hin.
    apply in_seq in Hin.
    lia.
Qed.

Theorem due_round_refines_earliest_uncompleted_prefix :
  forall progress recovery_round,
    recovery_round_due progress recovery_round = true ->
    earliest_uncompleted_round_up_to
      recovery_round (completed_round_prefix progress) =
        Some recovery_round.
Proof.
  intros [floor completed active] recovery_round Hdue.
  unfold recovery_round_due in Hdue.
  simpl in Hdue.
  destruct active as [reservation |]; [discriminate |].
  destruct completed as [completed |].
  - apply Nat.eqb_eq in Hdue.
    subst recovery_round.
    apply earliest_uncompleted_round_follows_completed_prefix.
  - apply Nat.eqb_eq in Hdue.
    subst recovery_round.
    reflexivity.
Qed.

Definition proposal_scheduler_contract : Prop :=
  (forall progress reservation attempted,
    attempt_proposal progress reservation = Some attempted ->
    last_completed_round attempted = last_completed_round progress /\
    active_reservation attempted = Some reservation)
  /\
  (forall progress reservation attempted next_reservation,
    attempt_proposal progress reservation = Some attempted ->
    attempt_proposal attempted next_reservation = None)
  /\
  (forall progress reservation attempted,
    attempt_proposal progress reservation = Some attempted ->
    resolve_proposal attempted ProposalEmpty = progress /\
    resolve_proposal attempted ProposalDeferred = progress /\
    resolve_proposal attempted ProposalFailed = progress)
  /\
  (forall progress attempted,
    attempt_proposal
      progress (PendingReservation None) = Some attempted ->
    resolve_proposal attempted ProposalStarted = progress /\
    resolve_proposal attempted ProposalSuccess = progress)
  /\
  (forall progress reservation attempted recovery_round,
    attempt_proposal progress reservation = Some attempted ->
    reserved_recovery_round reservation = Some recovery_round ->
    (last_completed_round
       (resolve_proposal attempted ProposalStarted) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalStarted)
       recovery_round = false) /\
    (last_completed_round
       (resolve_proposal attempted ProposalSuccess) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalSuccess)
       recovery_round = false))
  /\
  (forall progress recovery_round,
    recovery_round_due progress recovery_round = false ->
    record_recovery_completion progress recovery_round = progress)
  /\
  (forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height proposer recovery_round
    progress attempted,
    proposal_reservation_authorized
      committee finalized_height proposer true
      (PendingReservation (Some recovery_round)) ->
    attempt_proposal
      progress (PendingReservation (Some recovery_round)) = Some attempted ->
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer /\
    (last_completed_round
       (resolve_proposal attempted ProposalStarted) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalStarted)
       recovery_round = false) /\
    (last_completed_round
       (resolve_proposal attempted ProposalSuccess) = Some recovery_round /\
     recovery_round_due
       (resolve_proposal attempted ProposalSuccess)
       recovery_round = false))
  /\
  (forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer progress,
    nonleader_skip_authorized
      committee finalized_height recovery_round proposer ->
    recovery_round_due progress recovery_round = true ->
    last_completed_round
      (record_nonleader_skip progress recovery_round) = Some recovery_round /\
    active_reservation
      (record_nonleader_skip progress recovery_round) = None)
  /\
  (forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height proposer has_pending_work,
    ~ automatic_proposal_authorized
        committee finalized_height proposer has_pending_work
        EagerAutomaticSupport)
  /\
  (forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer has_pending_work,
    automatic_proposal_authorized
      committee finalized_height proposer has_pending_work
      (RecoverySupport recovery_round) ->
    has_pending_work = false /\
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer)
  /\
  (forall
    (Validator : Type)
    (committee : list Validator)
    finalized_height recovery_round proposer has_pending_work,
    proposal_reservation_authorized
      committee finalized_height proposer has_pending_work
      (RecoveryReservation recovery_round) ->
    has_pending_work = false /\
    committee_recovery_round_authorized
      committee finalized_height recovery_round proposer)
  /\
  (forall progress,
    observe_floor progress (observed_floor progress) = progress)
  /\
  (forall progress current_floor,
    observed_floor progress <> current_floor ->
    last_completed_round (observe_floor progress current_floor) = None /\
    active_reservation (observe_floor progress current_floor) = None)
  /\
  (forall progress recovery_round,
    recovery_round_due progress recovery_round = true ->
    earliest_uncompleted_round_up_to
      recovery_round (completed_round_prefix progress) =
        Some recovery_round).

Theorem proposal_scheduler_end_to_end :
  proposal_scheduler_contract.
Proof.
  unfold proposal_scheduler_contract.
  split.
  - exact proposal_attempt_reserves_without_completion.
  - split.
    + exact active_reservation_serializes_proposals.
    + split.
      * exact nonstarting_results_release_without_completion.
      * split.
        -- exact pending_without_recovery_releases_without_completion.
        -- split.
           ++ exact starting_results_complete_attached_round.
           ++ split.
              ** exact completion_outside_due_order_is_ignored.
              ** split.
                 --- exact authorized_pending_recovery_composes_one_proposal.
                 --- split.
                     +++ exact authorized_nonleader_skip_completes_without_reservation.
                     +++ split.
                         *** exact eager_automatic_support_is_never_authorized.
                         *** split.
                             { exact authorized_recovery_support_uses_selected_leader. }
                             split.
                             { exact authorized_recovery_reservation_uses_selected_leader. }
                             split.
                             { exact unchanged_floor_preserves_recovery_history. }
                             split.
                             { exact advanced_floor_resets_recovery_history. }
                             exact due_round_refines_earliest_uncompleted_prefix.
Qed.

Definition enqueue_bounded {A : Type}
  (capacity : nat)
  (queue : list A)
  (item : A)
  : list A :=
  if Nat.ltb (length queue) capacity
  then queue ++ [item]
  else queue.

Theorem bounded_enqueue_preserves_capacity :
  forall (A : Type) capacity (queue : list A) item,
    length queue <= capacity ->
    length (enqueue_bounded capacity queue item) <= capacity.
Proof.
  intros A capacity queue item Hbounded.
  unfold enqueue_bounded.
  destruct (Nat.ltb (length queue) capacity) eqn:Hspace.
  - apply Nat.ltb_lt in Hspace.
    rewrite length_app.
    simpl.
    lia.
  - exact Hbounded.
Qed.

Definition recovery_cadence_contract : Prop :=
  (forall current_round completed_rounds selected_round,
    earliest_uncompleted_round_up_to
      current_round completed_rounds = Some selected_round ->
    selected_round <= current_round /\
    ~ In selected_round completed_rounds /\
    forall earlier_round,
      earlier_round < selected_round ->
      In earlier_round completed_rounds)
  /\
  (forall current_round completed_rounds,
    (exists round,
      round <= current_round /\
      ~ In round completed_rounds) ->
    exists selected_round,
      earliest_uncompleted_round_up_to
        current_round completed_rounds = Some selected_round)
  /\
  earliest_due_recovery_round 30 15 5 [0] = Some 1
  /\
  recovery_round_at 30 15 5 = Some 3.

Theorem recovery_cadence_end_to_end :
  recovery_cadence_contract.
Proof.
  unfold recovery_cadence_contract.
  split.
  - intros current_round completed_rounds selected_round Hselected.
    pose proof
      (earliest_uncompleted_round_is_due_and_uncompleted
        current_round completed_rounds selected_round Hselected)
      as [Hdue Huncompleted].
    repeat split.
    + exact Hdue.
    + exact Huncompleted.
    + intros earlier_round Hearlier.
      eapply earliest_uncompleted_round_is_minimal; eauto.
  - split.
    + exact an_uncompleted_due_round_is_never_skipped.
    + exact earliest_due_round_retains_skipped_wake_opportunities.
Qed.

Definition heartbeat_backpressure_contract : Prop :=
  (forall validator_count finalized_height recovery_round,
    validator_count > 0 ->
    rotating_recovery_leader
      validator_count finalized_height recovery_round < validator_count)
  /\
  (forall validator_count finalized_height recovery_round proposer_a proposer_b,
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_a ->
    recovery_round_authorized
      validator_count finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b)
  /\
  (forall
    (Floor Validator Head : Type)
    (committee_of_floor : Floor -> list Validator)
    (head_committee : Head -> list Validator)
    (floor : Floor)
    (head_a head_b : Head)
    finalized_height recovery_round proposer_a proposer_b,
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer_a ->
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer_b ->
    proposer_a = proposer_b)
  /\
  (forall
    (Floor Validator : Type)
    (committee_of_floor : Floor -> list Validator)
    (floor : Floor)
    finalized_height recovery_round proposer,
    floor_recovery_round_authorized
      committee_of_floor floor finalized_height recovery_round proposer ->
    floor_proposal_eligible committee_of_floor floor proposer)
  /\
  (forall stalled_for stall_timeout recovery_interval,
    stalled_for < stall_timeout ->
    recovery_round_at
      stalled_for stall_timeout recovery_interval = None)
  /\
  (forall stall_timeout recovery_interval,
    recovery_round_at
      stall_timeout stall_timeout recovery_interval = Some 0)
  /\
  (forall stall_timeout recovery_interval recovery_round,
    recovery_interval > 0 ->
    recovery_round_at
      (stall_timeout + recovery_round * recovery_interval)
      stall_timeout
      recovery_interval = Some recovery_round)
  /\
  recovery_round_at 20 15 5 = Some 1
  /\
  collapsed_recovery_round_at 20 15 = Some 0
  /\
  recovery_cadence_contract
  /\
  recovery_evidence_contract
  /\
  proposal_scheduler_contract
  /\
  (forall (A : Type) capacity (queue : list A) item,
    length queue <= capacity ->
    length (enqueue_bounded capacity queue item) <= capacity).

Theorem heartbeat_backpressure_end_to_end :
  heartbeat_backpressure_contract.
Proof.
  unfold heartbeat_backpressure_contract.
  split.
  - exact rotating_recovery_leader_in_committee.
  - split.
    + exact recovery_round_authorization_unique.
    + split.
      * exact floor_recovery_authorization_unique_across_head_views.
      * split.
        -- exact floor_recovery_authorization_implies_floor_eligibility.
        -- split.
           ++ exact recovery_round_absent_before_stall_timeout.
           ++ split.
              ** exact recovery_round_zero_at_stall_timeout.
              ** split.
                 --- exact recovery_round_at_interval_boundary.
                 --- split.
                     +++ exact (proj1 separated_cadence_repairs_collapsed_timeout).
                     +++ split.
                         *** exact (proj2 separated_cadence_repairs_collapsed_timeout).
                         *** split.
                             { exact recovery_cadence_end_to_end. }
                             split.
                             { exact recovery_evidence_end_to_end. }
                             split.
                             { exact proposal_scheduler_end_to_end. }
                             exact bounded_enqueue_preserves_capacity.
Qed.

Print Assumptions heartbeat_backpressure_end_to_end.
