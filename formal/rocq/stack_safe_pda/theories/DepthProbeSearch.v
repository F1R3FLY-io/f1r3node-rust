From Stdlib Require Import Arith Bool Lia Wf_nat.
Set Implicit Arguments.

Module DepthProbeSearch.

Definition midpoint (lo hi : nat) := lo + (hi - lo) / 2.

Theorem midpoint_strictly_inside : forall lo hi,
  lo + 1 < hi -> lo < midpoint lo hi < hi.
Proof.
  intros lo hi H.
  pose proof (Nat.div_mod (hi - lo) 2 ltac:(lia)).
  pose proof (Nat.mod_upper_bound (hi - lo) 2 ltac:(lia)).
  unfold midpoint; lia.
Qed.

Theorem midpoint_bounded_by_machine_max : forall lo hi maximum,
  lo + 1 < hi -> hi <= maximum -> midpoint lo hi <= maximum.
Proof. intros; pose proof (@midpoint_strictly_inside lo hi H); lia. Qed.

Theorem either_update_strictly_shrinks : forall lo hi,
  lo + 1 < hi ->
  hi - midpoint lo hi < hi - lo /\ midpoint lo hi - lo < hi - lo.
Proof. intros; pose proof (@midpoint_strictly_inside lo hi H); lia. Qed.

Section Search.
Context {E : Type}.

Inductive Observation := Passed | StackFailure | Invalid (error : E) | Transport (error : E).
Inductive Answer := BaseFailed (failure : Observation) | AtLeast (cap : nat)
  | Exact (greatest_survivor : nat) | ProbeError (depth : nat) (failure : Observation).
Inductive Phase := CheckBase (cap : nat) | CheckCap (cap : nat)
  | Search (lo hi : nat) | Finished (answer : Answer).

Definition close_bracket lo hi :=
  if hi - lo <=? 1 then Finished (Exact lo) else Search lo hi.

Definition probe_depth phase :=
  match phase with
  | CheckBase _ => 1 | CheckCap cap => cap
  | Search lo hi => midpoint lo hi | Finished _ => 0
  end.

Definition step phase observed :=
  match phase with
  | CheckBase cap =>
      match observed with
      | Passed => if cap =? 1 then Finished (AtLeast cap) else CheckCap cap
      | Transport _ => Finished (ProbeError 1 observed)
      | _ => Finished (BaseFailed observed)
      end
  | CheckCap cap =>
      match observed with
      | Passed => Finished (AtLeast cap)
      | StackFailure => close_bracket 1 cap
      | _ => Finished (ProbeError cap observed)
      end
  | Search lo hi =>
      let mid := midpoint lo hi in
      match observed with
      | Passed => close_bracket mid hi
      | StackFailure => close_bracket lo mid
      | _ => Finished (ProbeError mid observed)
      end
  | Finished answer => Finished answer
  end.

Definition well_formed phase :=
  match phase with
  | CheckBase cap => 0 < cap
  | CheckCap cap => 1 < cap
  | Search lo hi => 1 <= lo /\ lo + 1 < hi
  | Finished _ => True
  end.

Definition rank phase :=
  match phase with
  | CheckBase cap => cap + 3 | CheckCap cap => cap + 2
  | Search lo hi => hi - lo | Finished _ => 0
  end.

Theorem close_bracket_well_formed : forall lo hi,
  1 <= lo -> well_formed (close_bracket lo hi).
Proof.
  intros; unfold close_bracket; destruct (hi - lo <=? 1) eqn:Htest; cbn; auto.
  apply Nat.leb_gt in Htest; lia.
Qed.

Theorem close_bracket_rank : forall lo hi,
  rank (close_bracket lo hi) <= hi - lo.
Proof. intros; unfold close_bracket; destruct (hi - lo <=? 1); cbn; lia. Qed.

Theorem step_preserves_well_formed : forall phase observed,
  well_formed phase -> well_formed (step phase observed).
Proof.
  intros [cap|cap|lo hi|answer] observed H; destruct observed;
    cbn [step well_formed] in *; try exact I.
  - destruct (cap =? 1) eqn:Hcap; cbn; auto.
    apply Nat.eqb_neq in Hcap; lia.
  - apply close_bracket_well_formed; lia.
  - apply close_bracket_well_formed.
    pose proof (@midpoint_strictly_inside lo hi (proj2 H)); lia.
  - apply close_bracket_well_formed; lia.
Qed.

Theorem nonterminal_step_decreases_rank : forall phase observed,
  well_formed phase ->
  (forall answer, phase <> Finished answer) ->
  rank (step phase observed) < rank phase.
Proof.
  intros [cap|cap|lo hi|answer] observed H Hlive; destruct observed;
    cbn [step rank well_formed] in *; try lia;
    try (exfalso; apply (Hlive answer); reflexivity).
  - destruct (cap =? 1); cbn; lia.
  - pose proof (close_bracket_rank 1 cap); lia.
  - pose proof (close_bracket_rank (midpoint lo hi) hi).
    pose proof (@either_update_strictly_shrinks lo hi (proj2 H)); lia.
  - pose proof (close_bracket_rank lo (midpoint lo hi)).
    pose proof (@either_update_strictly_shrinks lo hi (proj2 H)); lia.
Qed.

Definition observation_law boundary depth observed :=
  match observed with Passed => depth <= boundary
  | StackFailure => boundary < depth | _ => True end.

Definition answer_law boundary answer :=
  match answer with AtLeast cap => cap <= boundary
  | Exact survivor => survivor = boundary | _ => True end.

Definition phase_law boundary phase :=
  match phase with
  | CheckBase _ => True | CheckCap _ => 1 <= boundary
  | Search lo hi => lo <= boundary /\ boundary < hi
  | Finished answer => answer_law boundary answer
  end.

Theorem close_bracket_preserves_boundary : forall lo hi boundary,
  lo <= boundary -> boundary < hi -> phase_law boundary (close_bracket lo hi).
Proof.
  intros; unfold close_bracket; destruct (hi - lo <=? 1) eqn:Htest; cbn.
  - apply Nat.leb_le in Htest; lia.
  - auto.
Qed.

Theorem step_preserves_exact_boundary : forall phase observed boundary,
  phase_law boundary phase ->
  observation_law boundary (probe_depth phase) observed ->
  phase_law boundary (step phase observed).
Proof.
  intros [cap|cap|lo hi|answer] observed boundary Hlaw Hobs; destruct observed;
    cbn [step probe_depth observation_law phase_law answer_law] in *; auto.
  - destruct (cap =? 1) eqn:Hcap; cbn [phase_law answer_law]; auto.
    apply Nat.eqb_eq in Hcap; lia.
  - apply close_bracket_preserves_boundary; assumption.
  - apply close_bracket_preserves_boundary; tauto.
  - apply close_bracket_preserves_boundary; tauto.
Qed.

Theorem terminal_is_absorbing : forall answer observed,
  step (Finished answer) observed = Finished answer.
Proof. reflexivity. Qed.

Theorem base_transport_preserves_error : forall cap error,
  step (CheckBase cap) (Transport error) = Finished (ProbeError 1 (Transport error)).
Proof. reflexivity. Qed.

Theorem cap_transport_preserves_error : forall cap error,
  step (CheckCap cap) (Transport error) = Finished (ProbeError cap (Transport error)).
Proof. reflexivity. Qed.

Theorem midpoint_transport_preserves_error : forall lo hi error,
  step (Search lo hi) (Transport error) =
  Finished (ProbeError (midpoint lo hi) (Transport error)).
Proof. reflexivity. Qed.

Theorem invalid_midpoint_is_not_a_ceiling : forall lo hi error,
  step (Search lo hi) (Invalid error) =
  Finished (ProbeError (midpoint lo hi) (Invalid error)).
Proof. reflexivity. Qed.

Theorem broken_base_is_not_a_ceiling : forall cap,
  step (CheckBase cap) StackFailure = Finished (BaseFailed StackFailure).
Proof. reflexivity. Qed.

Theorem passing_unit_cap_uses_base :
  step (CheckBase 1) Passed = Finished (AtLeast 1).
Proof. reflexivity. Qed.

Theorem passing_cap_needs_two_probes : forall cap,
  cap <> 1 -> step (step (CheckBase cap) Passed) Passed = Finished (AtLeast cap).
Proof.
  intros cap H; cbn [step].
  assert (cap =? 1 = false) as Hcap by (apply Nat.eqb_neq; exact H).
  rewrite Hcap; reflexivity.
Qed.

Theorem finished_exact_has_correct_survivor : forall boundary survivor,
  phase_law boundary (Finished (Exact survivor)) -> survivor = boundary.
Proof. intros; exact H. Qed.

Theorem finished_cap_is_only_a_lower_bound : forall boundary cap,
  phase_law boundary (Finished (AtLeast cap)) -> cap <= boundary.
Proof. intros; exact H. Qed.

Inductive Executes (oracle : nat -> Observation) : Phase -> Answer -> Prop :=
  | ExecutesDone : forall answer, Executes oracle (Finished answer) answer
  | ExecutesStep : forall phase answer,
      (forall result, phase <> Finished result) ->
      Executes oracle (step phase (oracle (probe_depth phase))) answer ->
      Executes oracle phase answer.

Theorem search_terminates_without_fuel : forall oracle phase,
  well_formed phase -> exists answer, Executes oracle phase answer.
Proof.
  intros oracle.
  refine (well_founded_induction_type (well_founded_ltof Phase rank)
    (fun phase => well_formed phase -> exists answer, Executes oracle phase answer) _).
  intros phase IH Hwf.
  destruct phase as [cap|cap|lo hi|answer].
  4: exists answer; constructor.
  all: match goal with
  | |- exists answer, Executes ?query ?current answer =>
      let next := constr:(step current (query (probe_depth current))) in
      assert (Hsmaller : rank next < rank current) by
        (apply nonterminal_step_decreases_rank; [exact Hwf | intros; discriminate]);
      assert (Hnext : well_formed next) by
        (apply step_preserves_well_formed; exact Hwf);
      destruct (IH next Hsmaller Hnext) as [answer Hrun];
      exists answer; eapply ExecutesStep; [intros; discriminate | exact Hrun]
  end.
Qed.

Theorem complete_run_preserves_exact_boundary : forall oracle phase answer boundary,
  (forall depth, observation_law boundary depth (oracle depth)) ->
  phase_law boundary phase -> Executes oracle phase answer -> answer_law boundary answer.
Proof.
  intros oracle phase answer boundary Horacle Hphase Hrun.
  revert Hphase; induction Hrun; intros Hphase; auto.
  apply IHHrun, step_preserves_exact_boundary; auto.
Qed.

End Search.

Print Assumptions midpoint_strictly_inside.
Print Assumptions midpoint_bounded_by_machine_max.
Print Assumptions either_update_strictly_shrinks.
Print Assumptions close_bracket_well_formed.
Print Assumptions close_bracket_rank.
Print Assumptions step_preserves_well_formed.
Print Assumptions nonterminal_step_decreases_rank.
Print Assumptions close_bracket_preserves_boundary.
Print Assumptions step_preserves_exact_boundary.
Print Assumptions terminal_is_absorbing.
Print Assumptions base_transport_preserves_error.
Print Assumptions cap_transport_preserves_error.
Print Assumptions midpoint_transport_preserves_error.
Print Assumptions invalid_midpoint_is_not_a_ceiling.
Print Assumptions broken_base_is_not_a_ceiling.
Print Assumptions passing_unit_cap_uses_base.
Print Assumptions passing_cap_needs_two_probes.
Print Assumptions finished_exact_has_correct_survivor.
Print Assumptions finished_cap_is_only_a_lower_bound.
Print Assumptions search_terminates_without_fuel.
Print Assumptions complete_run_preserves_exact_boundary.

End DepthProbeSearch.
