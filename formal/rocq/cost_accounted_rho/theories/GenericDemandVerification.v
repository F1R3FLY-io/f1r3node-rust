From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.

Import ListNotations.

Inductive demand_evidence (Resource : Type) : Type :=
  | DemandExact : Resource -> demand_evidence Resource
  | DemandUpper : Resource -> list nat -> demand_evidence Resource
  | DemandUnprovable : demand_evidence Resource.

Arguments DemandExact {Resource} _.
Arguments DemandUpper {Resource} _ _.
Arguments DemandUnprovable {Resource}.

Section Authentication.

Context {Resource : Type}.
Variable resource_eq_dec : forall left right : Resource, {left = right} + {left <> right}.

Definition verify_exact_demand
    (claim checked : demand_evidence Resource) : option Resource :=
  match claim, checked with
  | DemandExact declared, DemandExact expected =>
      if resource_eq_dec declared expected then Some expected else None
  | _, _ => None
  end.

Theorem verified_exact_authenticates_claim_and_analysis :
  forall claim checked reservation,
    verify_exact_demand claim checked = Some reservation ->
    claim = DemandExact reservation /\ checked = DemandExact reservation.
Proof.
  intros claim checked reservation Hverified.
  destruct claim as [declared | declared proof |];
    destruct checked as [expected | expected proof' |];
    simpl in Hverified; try discriminate.
  destruct (resource_eq_dec declared expected) as [Hequal | Hdifferent];
    try discriminate.
  injection Hverified as Hreservation.
  subst reservation. subst declared. split; reflexivity.
Qed.

Theorem checked_exact_is_accepted : forall reservation,
  verify_exact_demand (DemandExact reservation) (DemandExact reservation) = Some reservation.
Proof.
  intros reservation. unfold verify_exact_demand.
  destruct (resource_eq_dec reservation reservation); [reflexivity | contradiction].
Qed.

Theorem changed_exact_is_rejected : forall declared expected,
  declared <> expected ->
  verify_exact_demand (DemandExact declared) (DemandExact expected) = None.
Proof.
  intros declared expected Hdifferent. unfold verify_exact_demand.
  destruct (resource_eq_dec declared expected); [contradiction | reflexivity].
Qed.

Theorem unprovable_analysis_rejects_every_claim : forall claim,
  verify_exact_demand claim DemandUnprovable = None.
Proof. intros claim. destruct claim; reflexivity. Qed.

Theorem upper_analysis_cannot_be_promoted : forall claim bound proof,
  verify_exact_demand claim (DemandUpper bound proof) = None.
Proof. intros claim bound proof. destruct claim; reflexivity. Qed.

Theorem finite_claim_requires_explicit_verifier : forall checked bound proof,
  verify_exact_demand (DemandUpper bound proof) checked = None.
Proof. intros checked bound proof. destruct checked; reflexivity. Qed.

End Authentication.

Section Funding.

Context {Program Signature Resource Surface : Type}.
Variable resource_eq_dec : forall left right : Resource, {left = right} + {left <> right}.
Variable units_at : Resource -> Surface -> nat.

Definition resource_dominates (supply demand : Resource) : Prop :=
  forall surface, units_at demand surface <= units_at supply surface.

Definition exact_analyzer_sound
    (analyze : Program -> Signature -> demand_evidence Resource)
    (executes : Program -> Signature -> Resource -> Prop) : Prop :=
  forall program signature expected actual,
    analyze program signature = DemandExact expected ->
    executes program signature actual ->
    forall surface, units_at actual surface = units_at expected surface.

Definition verified_funding
    (claim checked : demand_evidence Resource) (supply : Resource) : Prop :=
  exists reservation,
    verify_exact_demand resource_eq_dec claim checked = Some reservation /\
    resource_dominates supply reservation.

Theorem verified_reservation_bounds_execution : forall analyze executes,
  exact_analyzer_sound analyze executes ->
  forall program signature claim reservation actual,
    verify_exact_demand resource_eq_dec claim (analyze program signature) = Some reservation ->
    executes program signature actual ->
    resource_dominates reservation actual.
Proof.
  intros analyze executes Hsound program signature claim reservation actual Hverified Hrun surface.
  apply verified_exact_authenticates_claim_and_analysis in Hverified as [_ Hchecked].
  rewrite (Hsound program signature reservation actual Hchecked Hrun surface).
  apply Nat.le_refl.
Qed.

Theorem verified_funding_covers_execution : forall analyze executes,
  exact_analyzer_sound analyze executes ->
  forall program signature claim supply actual,
    verified_funding claim (analyze program signature) supply ->
    executes program signature actual ->
    resource_dominates supply actual.
Proof.
  intros analyze executes Hsound program signature claim supply actual
    [reservation [Hverified Hfunded]] Hrun surface.
  pose proof (verified_reservation_bounds_execution analyze executes Hsound
    program signature claim reservation actual Hverified Hrun surface) as Hbound.
  specialize (Hfunded surface). lia.
Qed.

Theorem verified_funding_is_supply_monotone : forall claim checked lower upper,
  verified_funding claim checked lower ->
  resource_dominates upper lower ->
  verified_funding claim checked upper.
Proof.
  intros claim checked lower upper [reservation [Hverified Hfunded]] Hincrease.
  exists reservation. split; [exact Hverified |].
  intros surface. specialize (Hfunded surface). specialize (Hincrease surface). lia.
Qed.

Theorem deficient_surface_rejects_funding : forall claim checked supply reservation surface,
  verify_exact_demand resource_eq_dec claim checked = Some reservation ->
  units_at supply surface < units_at reservation surface ->
  ~ verified_funding claim checked supply.
Proof.
  intros claim checked supply reservation surface Hverified Hshort
    [other [Hother Hfunded]].
  rewrite Hverified in Hother. injection Hother as Hequal. subst other.
  specialize (Hfunded surface). lia.
Qed.

End Funding.

Definition pair_resource_eq_dec (left right : nat * nat) : {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition pair_units (resource : nat * nat) (surface : bool) : nat :=
  if surface then fst resource else snd resource.

Definition add_pair (left right : nat * nat) : nat * nat :=
  (fst left + fst right, snd left + snd right).

Inductive finite_program : Type :=
  | SpendFirst
  | SpendSecond
  | Sequence : finite_program -> finite_program -> finite_program
  | Branch : finite_program -> finite_program -> finite_program.

Inductive finite_execution : finite_program -> (nat * nat) -> Prop :=
  | RunFirst : finite_execution SpendFirst (1, 0)
  | RunSecond : finite_execution SpendSecond (0, 1)
  | RunSequence : forall left right left_cost right_cost,
      finite_execution left left_cost ->
      finite_execution right right_cost ->
      finite_execution (Sequence left right) (add_pair left_cost right_cost)
  | RunBranchLeft : forall left right cost,
      finite_execution left cost -> finite_execution (Branch left right) cost
  | RunBranchRight : forall left right cost,
      finite_execution right cost -> finite_execution (Branch left right) cost.

Fixpoint finite_analysis (program : finite_program) : demand_evidence (nat * nat) :=
  match program with
  | SpendFirst => DemandExact (1, 0)
  | SpendSecond => DemandExact (0, 1)
  | Sequence first second =>
      match finite_analysis first, finite_analysis second with
      | DemandExact left_cost, DemandExact right_cost =>
          DemandExact (add_pair left_cost right_cost)
      | _, _ => DemandUnprovable
      end
  | Branch _ _ => DemandUnprovable
  end.

Theorem finite_analysis_matches_independent_execution : forall program actual,
  finite_execution program actual ->
  forall expected, finite_analysis program = DemandExact expected -> actual = expected.
Proof.
  intros program actual Hrun.
  induction Hrun as [| |left right left_cost right_cost Hleft IHleft Hright IHright
    |left right cost Hleft IHleft |left right cost Hright IHright];
    intros expected Hchecked; simpl in Hchecked; try discriminate.
  - injection Hchecked as Hequal. exact Hequal.
  - injection Hchecked as Hequal. exact Hequal.
  - destruct (finite_analysis left) as [left_bound | left_bound left_proof |] eqn:Hleft_bound in Hchecked;
      try discriminate.
    destruct (finite_analysis right) as [right_bound | right_bound right_proof |] eqn:Hright_bound in Hchecked;
      try discriminate.
    injection Hchecked as Hequal.
    rewrite (IHleft left_bound Hleft_bound), (IHright right_bound Hright_bound).
    exact Hequal.
Qed.

Theorem finite_analyzer_discharge :
  exact_analyzer_sound pair_units
    (fun program (_ : unit) => finite_analysis program)
    (fun program (_ : unit) actual => finite_execution program actual).
Proof.
  intros program signature expected actual Hchecked Hrun surface.
  rewrite (finite_analysis_matches_independent_execution program actual Hrun expected Hchecked).
  reflexivity.
Qed.

Example exact_extension_remains_accepted :
  verify_exact_demand pair_resource_eq_dec (DemandExact (2, 1))
    (finite_analysis (Sequence SpendFirst (Sequence SpendSecond SpendFirst))) = Some (2, 1).
Proof. vm_compute. reflexivity. Qed.

Definition trust_caller_exact {Resource : Type}
    (claim _checked : demand_evidence Resource) : option Resource :=
  match claim with DemandExact reservation => Some reservation | _ => None end.

Example caller_trust_accepts_underfunded_execution :
  finite_execution SpendFirst (1, 0) /\
  trust_caller_exact (DemandExact (0, 0)) DemandUnprovable = Some (0, 0) /\
  ~ resource_dominates pair_units (0, 0) (1, 0).
Proof.
  split; [constructor |]. split; [reflexivity |].
  intros Hfunded. specialize (Hfunded true). simpl in Hfunded. lia.
Qed.

Example repaired_verifier_rejects_unproved_zero :
  verify_exact_demand pair_resource_eq_dec (DemandExact (0, 0)) DemandUnprovable = None.
Proof. reflexivity. Qed.

Example caller_trust_promotes_upper_evidence :
  trust_caller_exact (DemandExact (1, 1)) (DemandUpper (1, 1) [1]) = Some (1, 1) /\
  verify_exact_demand pair_resource_eq_dec (DemandExact (1, 1))
    (DemandUpper (1, 1) [1]) = None.
Proof. split; reflexivity. Qed.

Print Assumptions verified_exact_authenticates_claim_and_analysis.
Print Assumptions verified_reservation_bounds_execution.
Print Assumptions verified_funding_covers_execution.
Print Assumptions verified_funding_is_supply_monotone.
Print Assumptions deficient_surface_rejects_funding.
Print Assumptions finite_analyzer_discharge.
Print Assumptions caller_trust_accepts_underfunded_execution.
