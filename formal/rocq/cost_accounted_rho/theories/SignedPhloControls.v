From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia Sorting.Permutation.
From CostAccountedRho Require Import CostAccountedSyntax AuthorityResourceValuation
  PrepaidResourceDischarge FundingPriceConsent.
Import ListNotations.

Record phlo_schedule := {
  phlo_schedule_commitment : nat;
  phlo_protocol_version : nat;
  phlo_network : nat;
  phlo_shard : nat;
  phlo_asset : nat;
  phlo_unit : nat;
  phlo_decimal_scale : nat;
  phlo_weights : list nat;
  phlo_actual_price : nat
}.

Definition phlo_schedule_eq_dec : forall (left right : phlo_schedule),
  {left = right} + {left <> right}.
Proof. decide equality; auto using Nat.eq_dec, list_eq_dec. Defined.

Record signed_phlo_controls := {
  signed_phlo_limit : nat;
  signed_phlo_price : nat;
  signed_required_ceilings : list nat;
  signed_phlo_schedules : list phlo_schedule
}.

Record phlo_environment := {
  active_phlo_version : nat;
  native_phlo_network : nat;
  native_phlo_shard : nat;
  native_phlo_asset : nat;
  native_phlo_unit : nat;
  native_phlo_decimal_scale : nat
}.

Definition resource_phlo (schedule : phlo_schedule) (resource : prepaid_resource_key) : nat :=
  nth (prepaid_class resource) (phlo_weights schedule) 0 *
    authority_units (prepaid_authority resource).

Definition execution_phlo (schedule : phlo_schedule) (resources : list prepaid_resource_key) : nat :=
  acquisition_value (resource_phlo schedule) resources.

Definition known_resource_classes (schedule : phlo_schedule)
  (resources : list prepaid_resource_key) : bool :=
  forallb (fun resource => prepaid_class resource <? length (phlo_weights schedule)) resources.

Definition signed_charge_ceiling (terms : signed_phlo_controls) : nat :=
  signed_phlo_limit terms * signed_phlo_price terms + 1.

Definition schedule_charge_bound (schedule : phlo_schedule) (bound : nat) : nat :=
  bound * phlo_actual_price schedule + 1.

Definition admit_phlo_controls (environment : phlo_environment) (machine_max : nat)
  (terms : signed_phlo_controls) (schedule : phlo_schedule) (bound : nat) : bool :=
  if in_dec phlo_schedule_eq_dec schedule (signed_phlo_schedules terms) then
    match required_price_ceiling (signed_required_ceilings terms) with
    | None => false
    | Some ceiling =>
        (phlo_protocol_version schedule =? active_phlo_version environment) &&
        (phlo_network schedule =? native_phlo_network environment) &&
        (phlo_shard schedule =? native_phlo_shard environment) &&
        (phlo_asset schedule =? native_phlo_asset environment) &&
        (phlo_unit schedule =? native_phlo_unit environment) &&
        (phlo_decimal_scale schedule =? native_phlo_decimal_scale environment) &&
        (bound <=? signed_phlo_limit terms) &&
        (phlo_actual_price schedule <=? signed_phlo_price terms) &&
        (phlo_actual_price schedule <=? ceiling) &&
        (signed_charge_ceiling terms <=? machine_max)
    end
  else false.

Theorem admitted_phlo_controls_exact : forall version machine_max terms schedule bound,
  admit_phlo_controls version machine_max terms schedule bound = true <->
  In schedule (signed_phlo_schedules terms) /\
  phlo_protocol_version schedule = active_phlo_version version /\
  phlo_network schedule = native_phlo_network version /\
  phlo_shard schedule = native_phlo_shard version /\
  phlo_asset schedule = native_phlo_asset version /\
  phlo_unit schedule = native_phlo_unit version /\
  phlo_decimal_scale schedule = native_phlo_decimal_scale version /\
  bound <= signed_phlo_limit terms /\
  phlo_actual_price schedule <= signed_phlo_price terms /\
  signed_required_ceilings terms <> [] /\
  Forall (fun ceiling => phlo_actual_price schedule <= ceiling)
    (signed_required_ceilings terms) /\
  signed_charge_ceiling terms <= machine_max.
Proof.
  intros version machine_max terms schedule bound. unfold admit_phlo_controls.
  destruct (in_dec phlo_schedule_eq_dec schedule (signed_phlo_schedules terms)) as [present|absent].
  - destruct (required_price_ceiling (signed_required_ceilings terms)) as [ceiling|] eqn:found.
    + rewrite !andb_true_iff, !Nat.eqb_eq, !Nat.leb_le.
      rewrite (required_price_ceiling_checks_every_consent _ _ _ found).
      assert (nonempty : signed_required_ceilings terms <> []).
      { intro empty. rewrite empty in found. discriminate. }
      tauto.
    + apply missing_price_consent_iff_empty in found.
      split; [discriminate|]. rewrite found. tauto.
  - split; [discriminate|]. intros [present _]. contradiction.
Qed.

Theorem admitted_schedule_bound_fits_signed_ceiling : forall version machine_max terms schedule bound,
  admit_phlo_controls version machine_max terms schedule bound = true ->
  schedule_charge_bound schedule bound <= signed_charge_ceiling terms /\
  signed_charge_ceiling terms <= machine_max.
Proof.
  intros version machine_max terms schedule bound admitted.
  apply admitted_phlo_controls_exact in admitted.
  unfold schedule_charge_bound, signed_charge_ceiling in *. nia.
Qed.

Definition check_numeric_phlo_bounds (machine_max limit ceiling price bound : nat)
  (owners : list nat) : bool :=
  match owners with
  | [] => false
  | _ :: _ =>
      (bound <=? limit) && (price <=? ceiling) &&
      forallb (fun owner => price <=? owner) owners &&
      (limit * ceiling + 1 <=? machine_max)
  end.

Definition admit_chain_phlo_controls environment minimum machine_max terms schedule bound :=
  (minimum <=? phlo_actual_price schedule) &&
  admit_phlo_controls environment machine_max terms schedule bound.

Record signed_phlo_offer := {
  offered_phlo_limit : nat;
  offered_phlo_price : nat
}.

Definition admit_offered_phlo_controls environment minimum machine_max
  (offer : signed_phlo_offer) terms schedule bound :=
  (offered_phlo_limit offer =? signed_phlo_limit terms) &&
  (offered_phlo_price offer =? phlo_actual_price schedule) &&
  admit_chain_phlo_controls environment minimum machine_max terms schedule bound.

Theorem offered_phlo_controls_exact :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true <->
  offered_phlo_limit offer = signed_phlo_limit terms /\
  offered_phlo_price offer = phlo_actual_price schedule /\
  admit_chain_phlo_controls environment minimum machine_max terms schedule bound = true.
Proof.
  intros. unfold admit_offered_phlo_controls.
  rewrite !andb_true_iff, !Nat.eqb_eq. tauto.
Qed.

Theorem chain_phlo_controls_exact : forall environment minimum machine_max terms schedule bound,
  admit_chain_phlo_controls environment minimum machine_max terms schedule bound = true <->
  minimum <= phlo_actual_price schedule /\
  admit_phlo_controls environment machine_max terms schedule bound = true.
Proof. intros. unfold admit_chain_phlo_controls. rewrite andb_true_iff, Nat.leb_le. reflexivity. Qed.

Theorem chain_phlo_price_between_minimum_and_every_ceiling :
  forall environment minimum machine_max terms schedule bound,
  admit_chain_phlo_controls environment minimum machine_max terms schedule bound = true ->
  minimum <= phlo_actual_price schedule /\
  phlo_actual_price schedule <= signed_phlo_price terms /\
  Forall (fun ceiling => phlo_actual_price schedule <= ceiling) (signed_required_ceilings terms).
Proof.
  intros. apply chain_phlo_controls_exact in H. destruct H as [floor admitted].
  apply admitted_phlo_controls_exact in admitted. tauto.
Qed.

Theorem below_chain_minimum_rejected_regardless_of_ceiling :
  forall environment minimum machine_max terms schedule bound,
  phlo_actual_price schedule < minimum ->
  admit_chain_phlo_controls environment minimum machine_max terms schedule bound = false.
Proof.
  intros. unfold admit_chain_phlo_controls.
  assert (minimum <=? phlo_actual_price schedule = false) by (apply Nat.leb_gt; assumption).
  rewrite H0. reflexivity.
Qed.

Theorem offered_price_respects_chain_and_all_owners :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true ->
  minimum <= offered_phlo_price offer /\
  Forall (fun ceiling => offered_phlo_price offer <= ceiling) (signed_required_ceilings terms).
Proof.
  intros. apply offered_phlo_controls_exact in H. destruct H as [_ [price admitted]].
  apply chain_phlo_price_between_minimum_and_every_ceiling in admitted.
  rewrite price. tauto.
Qed.

Theorem offered_price_charge_uses_offer_not_ceiling :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true ->
  schedule_charge_bound schedule bound = bound * offered_phlo_price offer + 1.
Proof.
  intros. apply offered_phlo_controls_exact in H. destruct H as [_ [price _]].
  unfold schedule_charge_bound. rewrite price. reflexivity.
Qed.

Theorem offered_limit_bounds_resources :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true ->
  bound <= offered_phlo_limit offer.
Proof.
  intros. apply offered_phlo_controls_exact in H. destruct H as [limit [_ admitted]].
  apply chain_phlo_controls_exact in admitted. destruct admitted as [_ admitted].
  apply admitted_phlo_controls_exact in admitted. rewrite limit. tauto.
Qed.

Theorem offered_price_mismatch_rejected :
  forall environment minimum machine_max offer terms schedule bound,
  offered_phlo_price offer <> phlo_actual_price schedule ->
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = false.
Proof.
  intros. unfold admit_offered_phlo_controls.
  apply Nat.eqb_neq in H. rewrite H. destruct (offered_phlo_limit offer =? signed_phlo_limit terms); reflexivity.
Qed.

Theorem offered_limit_mismatch_rejected :
  forall environment minimum machine_max offer terms schedule bound,
  offered_phlo_limit offer <> signed_phlo_limit terms ->
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = false.
Proof.
  intros. unfold admit_offered_phlo_controls. apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

Theorem offered_controls_refine_existing_funding :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true ->
  schedule_charge_bound schedule bound <= signed_charge_ceiling terms /\
  signed_charge_ceiling terms <= machine_max.
Proof.
  intros. apply offered_phlo_controls_exact in H. destruct H as [_ [_ admitted]].
  apply chain_phlo_controls_exact in admitted. destruct admitted as [_ admitted].
  eapply admitted_schedule_bound_fits_signed_ceiling; eauto.
Qed.

Theorem offered_controls_need_owner_consent :
  forall environment minimum machine_max offer terms schedule bound,
  admit_offered_phlo_controls environment minimum machine_max offer terms schedule bound = true ->
  signed_required_ceilings terms <> [].
Proof.
  intros. apply offered_phlo_controls_exact in H. destruct H as [_ [_ admitted]].
  apply chain_phlo_controls_exact in admitted. destruct admitted as [_ admitted].
  apply admitted_phlo_controls_exact in admitted. tauto.
Qed.

Theorem zero_chain_minimum_preserves_controls : forall environment machine_max terms schedule bound,
  admit_chain_phlo_controls environment 0 machine_max terms schedule bound =
  admit_phlo_controls environment machine_max terms schedule bound.
Proof. reflexivity. Qed.

Theorem raising_chain_minimum_cannot_add_admissions :
  forall environment lower higher machine_max terms schedule bound,
  lower <= higher ->
  admit_chain_phlo_controls environment higher machine_max terms schedule bound = true ->
  admit_chain_phlo_controls environment lower machine_max terms schedule bound = true.
Proof.
  intros. apply chain_phlo_controls_exact in H0. destruct H0 as [floor admitted].
  apply chain_phlo_controls_exact. split; [lia|assumption].
Qed.

Print Assumptions chain_phlo_controls_exact.
Print Assumptions chain_phlo_price_between_minimum_and_every_ceiling.
Print Assumptions below_chain_minimum_rejected_regardless_of_ceiling.
Print Assumptions zero_chain_minimum_preserves_controls.
Print Assumptions raising_chain_minimum_cannot_add_admissions.

Theorem numeric_phlo_bounds_exact : forall machine_max limit ceiling price bound owners,
  check_numeric_phlo_bounds machine_max limit ceiling price bound owners = true <->
  owners <> [] /\ bound <= limit /\ price <= ceiling /\
  Forall (fun owner => price <= owner) owners /\
  limit * ceiling + 1 <= machine_max.
Proof.
  intros. unfold check_numeric_phlo_bounds.
  destruct owners as [|owner rest].
  { split; [discriminate|intros [empty _]; contradiction]. }
  rewrite !andb_true_iff, !Nat.leb_le.
  rewrite forallb_forall, Forall_forall.
  assert (owners_nonempty : owner :: rest <> []) by discriminate.
  setoid_rewrite Nat.leb_le. tauto.
Qed.

Theorem admitted_controls_pass_numeric_bounds : forall environment machine_max terms schedule bound,
  admit_phlo_controls environment machine_max terms schedule bound = true ->
  check_numeric_phlo_bounds machine_max (signed_phlo_limit terms)
    (signed_phlo_price terms) (phlo_actual_price schedule) bound
    (signed_required_ceilings terms) = true.
Proof.
  intros. apply numeric_phlo_bounds_exact.
  apply admitted_phlo_controls_exact in H.
  unfold signed_charge_ceiling in H. tauto.
Qed.

Theorem numeric_phlo_bounds_preserve_checked_charge :
  forall machine_max limit ceiling price bound owners,
  check_numeric_phlo_bounds machine_max limit ceiling price bound owners = true ->
  bound * price + 1 <= limit * ceiling + 1 /\
  limit * ceiling + 1 <= machine_max.
Proof.
  intros. apply numeric_phlo_bounds_exact in H. nia.
Qed.

Definition check_phlo_schedule (environment : phlo_environment)
  (terms : signed_phlo_controls) (schedule : phlo_schedule) : bool :=
  if in_dec phlo_schedule_eq_dec schedule (signed_phlo_schedules terms) then
    (phlo_protocol_version schedule =? active_phlo_version environment) &&
    (phlo_network schedule =? native_phlo_network environment) &&
    (phlo_shard schedule =? native_phlo_shard environment) &&
    (phlo_asset schedule =? native_phlo_asset environment) &&
    (phlo_unit schedule =? native_phlo_unit environment) &&
    (phlo_decimal_scale schedule =? native_phlo_decimal_scale environment)
  else false.

Theorem phlo_schedule_check_exact : forall environment terms schedule,
  check_phlo_schedule environment terms schedule = true <->
  In schedule (signed_phlo_schedules terms) /\
  phlo_protocol_version schedule = active_phlo_version environment /\
  phlo_network schedule = native_phlo_network environment /\
  phlo_shard schedule = native_phlo_shard environment /\
  phlo_asset schedule = native_phlo_asset environment /\
  phlo_unit schedule = native_phlo_unit environment /\
  phlo_decimal_scale schedule = native_phlo_decimal_scale environment.
Proof.
  intros. unfold check_phlo_schedule.
  destruct (in_dec phlo_schedule_eq_dec schedule (signed_phlo_schedules terms)).
  - rewrite !andb_true_iff, !Nat.eqb_eq. tauto.
  - split; [discriminate|intros [present _]; contradiction].
Qed.

Theorem phlo_controls_factorization : forall environment machine_max terms schedule bound,
  admit_phlo_controls environment machine_max terms schedule bound =
  (check_phlo_schedule environment terms schedule &&
   check_numeric_phlo_bounds machine_max (signed_phlo_limit terms)
     (signed_phlo_price terms) (phlo_actual_price schedule) bound
     (signed_required_ceilings terms)).
Proof.
  intros. apply eq_true_iff_eq.
  rewrite admitted_phlo_controls_exact, andb_true_iff,
    phlo_schedule_check_exact, numeric_phlo_bounds_exact.
  unfold signed_charge_ceiling. tauto.
Qed.

Theorem unapproved_schedule_rejected_even_at_lower_price : forall version machine_max terms schedule bound,
  ~ In schedule (signed_phlo_schedules terms) ->
  admit_phlo_controls version machine_max terms schedule bound = false.
Proof.
  intros. unfold admit_phlo_controls.
  destruct (in_dec phlo_schedule_eq_dec schedule (signed_phlo_schedules terms)); [contradiction|reflexivity].
Qed.

Theorem signed_schedule_cannot_redefine_native_fee_denomination :
  forall environment machine_max terms schedule bound,
  phlo_asset schedule <> native_phlo_asset environment \/
  phlo_unit schedule <> native_phlo_unit environment ->
  admit_phlo_controls environment machine_max terms schedule bound = false.
Proof.
  intros environment machine_max terms schedule bound mismatch.
  destruct (admit_phlo_controls environment machine_max terms schedule bound) eqn:checked; [|reflexivity].
  apply admitted_phlo_controls_exact in checked. tauto.
Qed.

Theorem admitted_schedule_commitment_has_explicit_consent : forall environment machine_max terms schedule bound,
  admit_phlo_controls environment machine_max terms schedule bound = true ->
  exists permitted, In permitted (signed_phlo_schedules terms) /\
    phlo_schedule_commitment permitted = phlo_schedule_commitment schedule.
Proof.
  intros. apply admitted_phlo_controls_exact in H. exists schedule. tauto.
Qed.

Theorem signed_schedule_cannot_change_native_decimal_scale : forall environment machine_max terms schedule bound,
  phlo_decimal_scale schedule <> native_phlo_decimal_scale environment ->
  admit_phlo_controls environment machine_max terms schedule bound = false.
Proof.
  intros environment machine_max terms schedule bound mismatch.
  destruct (admit_phlo_controls environment machine_max terms schedule bound) eqn:checked; [|reflexivity].
  apply admitted_phlo_controls_exact in checked. tauto.
Qed.

Print Assumptions signed_schedule_cannot_change_native_decimal_scale.

Theorem unconsented_schedule_commitment_is_rejected : forall environment machine_max terms schedule bound,
  (forall permitted, In permitted (signed_phlo_schedules terms) ->
    phlo_schedule_commitment permitted <> phlo_schedule_commitment schedule) ->
  admit_phlo_controls environment machine_max terms schedule bound = false.
Proof.
  intros environment machine_max terms schedule bound absent.
  apply unapproved_schedule_rejected_even_at_lower_price.
  intro present. exact (absent schedule present eq_refl).
Qed.

Print Assumptions admitted_schedule_commitment_has_explicit_consent.
Print Assumptions unconsented_schedule_commitment_is_rejected.

Theorem execution_phlo_permutation : forall schedule left right,
  Permutation left right -> execution_phlo schedule left = execution_phlo schedule right.
Proof. intros. now apply acquisition_value_permutation. Qed.

Theorem compound_resource_price_is_additive : forall schedule location class terms left right,
  resource_phlo schedule (prepaid_sample location class terms (SAnd left right)) =
  resource_phlo schedule (prepaid_sample location class terms left) +
  resource_phlo schedule (prepaid_sample location class terms right).
Proof. intros. unfold resource_phlo, prepaid_sample. simpl. nia. Qed.

Theorem prepaid_credit_does_not_remove_execution_usage : forall schedule available required used unused fresh,
  prepaid_discharge available required used unused fresh ->
  execution_phlo schedule required = execution_phlo schedule used + execution_phlo schedule fresh.
Proof.
  intros schedule available required used unused fresh [_ demand].
  unfold execution_phlo. rewrite (acquisition_value_permutation _ _ _ demand).
  apply acquisition_value_append.
Qed.

Definition check_phlo_execution (version : phlo_environment) (machine_max : nat) (terms : signed_phlo_controls)
  (schedule : phlo_schedule) (bound : nat)
  (available required used unused fresh : list prepaid_resource_key) : bool :=
  admit_phlo_controls version machine_max terms schedule bound &&
  known_resource_classes schedule required &&
  (execution_phlo schedule required <=? bound) &&
  check_prepaid_funding available required used unused fresh.

Definition newly_required_charge (schedule : phlo_schedule)
  (fresh : list prepaid_resource_key) : nat :=
  execution_phlo schedule fresh * phlo_actual_price schedule + 1.

Theorem checked_execution_funding_exact : forall version machine_max terms schedule bound
  available required used unused fresh,
  check_phlo_execution version machine_max terms schedule bound available required used unused fresh = true ->
  prepaid_discharge available required used unused fresh /\ prepaid_exhausted unused fresh.
Proof.
  intros. unfold check_phlo_execution in H.
  apply andb_true_iff in H. destruct H as [_ funding].
  unfold check_prepaid_funding in funding. apply andb_true_iff in funding.
  destruct funding as [discharge exhausted].
  split; [now apply prepaid_check_exact|now apply prepaid_exhausted_check_exact].
Qed.

Theorem checked_execution_usage_and_charge_bounds : forall version machine_max terms schedule bound
  available required used unused fresh,
  check_phlo_execution version machine_max terms schedule bound available required used unused fresh = true ->
  execution_phlo schedule required <= bound /\ bound <= signed_phlo_limit terms /\
  newly_required_charge schedule fresh <= schedule_charge_bound schedule bound /\
  schedule_charge_bound schedule bound <= signed_charge_ceiling terms /\
  signed_charge_ceiling terms <= machine_max.
Proof.
  intros version machine_max terms schedule bound available required used unused fresh checked.
  pose proof (checked_execution_funding_exact _ _ _ _ _ _ _ _ _ _ checked) as [discharge _].
  pose proof (prepaid_credit_does_not_remove_execution_usage schedule _ _ _ _ _ discharge) as valued.
  unfold check_phlo_execution in checked. apply andb_true_iff in checked.
  destruct checked as [prefix _]. apply andb_true_iff in prefix.
  destruct prefix as [controls usage]. apply andb_true_iff in controls.
  destruct controls as [admitted _]. apply Nat.leb_le in usage.
  pose proof (admitted_schedule_bound_fits_signed_ceiling _ _ _ _ _ admitted) as bounded.
  apply admitted_phlo_controls_exact in admitted.
  unfold newly_required_charge, schedule_charge_bound, signed_charge_ceiling in *. nia.
Qed.

Theorem checked_execution_never_prices_unknown_class : forall version machine_max terms schedule bound
  available required used unused fresh resource,
  check_phlo_execution version machine_max terms schedule bound available required used unused fresh = true ->
  In resource required -> prepaid_class resource < length (phlo_weights schedule).
Proof.
  intros version machine_max terms schedule bound available required used unused fresh resource checked present.
  unfold check_phlo_execution in checked. rewrite !andb_true_iff in checked.
  destruct checked as [[[admitted known] bounded] funding].
  unfold known_resource_classes in known. rewrite forallb_forall in known.
  apply Nat.ltb_lt. now apply known.
Qed.

Inductive phlo_failure :=
| PhloUserFailure
| PhloPlatformFailure
| PhloCertificateFailure
| PhloUnclassifiedFailure.

Definition billable_failure (failure : phlo_failure) : bool :=
  match failure with PhloUserFailure => true | _ => false end.

Inductive phlo_outcome :=
| PhloAdmissionRejected
| PhloAccepted (failures : list phlo_failure).

Definition retained_phlo_charge (schedule : phlo_schedule)
  (fresh : list prepaid_resource_key) (outcome : phlo_outcome) : nat :=
  match outcome with
  | PhloAdmissionRejected => 0
  | PhloAccepted failures =>
      if forallb billable_failure failures then newly_required_charge schedule fresh else 0
  end.

Theorem failure_order_cannot_change_charge : forall schedule fresh left right,
  Permutation left right ->
  retained_phlo_charge schedule fresh (PhloAccepted left) =
  retained_phlo_charge schedule fresh (PhloAccepted right).
Proof.
  intros schedule fresh left right same.
  assert (checked : forallb billable_failure left = forallb billable_failure right).
  { induction same; simpl in *; try rewrite IHsame; try rewrite IHsame1, IHsame2;
      try reflexivity. destruct (billable_failure x), (billable_failure y); reflexivity. }
  simpl. now rewrite checked.
Qed.

Theorem unsafe_failure_prevents_all_candidate_charge : forall schedule fresh failures bad,
  In bad failures -> billable_failure bad = false ->
  retained_phlo_charge schedule fresh (PhloAccepted failures) = 0.
Proof.
  intros schedule fresh failures bad present unsafe. simpl.
  destruct (forallb billable_failure failures) eqn:all; [|reflexivity].
  apply forallb_forall with (x := bad) in all; [congruence|assumption].
Qed.

Theorem checked_retained_charge_bounded : forall version machine_max terms schedule bound
  available required used unused fresh outcome,
  check_phlo_execution version machine_max terms schedule bound available required used unused fresh = true ->
  retained_phlo_charge schedule fresh outcome <= signed_charge_ceiling terms /\
  retained_phlo_charge schedule fresh outcome <= machine_max.
Proof.
  intros. pose proof (checked_execution_usage_and_charge_bounds _ _ _ _ _ _ _ _ _ _ H).
  destruct outcome; simpl; [lia|]. destruct (forallb billable_failure failures); lia.
Qed.

Theorem fully_prepaid_execution_has_only_separate_fee : forall schedule,
  retained_phlo_charge schedule [] (PhloAccepted []) = 1.
Proof. intros. reflexivity. Qed.

Example mixed_user_and_platform_failure_is_not_billable : forall schedule fresh,
  retained_phlo_charge schedule fresh
    (PhloAccepted [PhloUserFailure; PhloPlatformFailure; PhloUserFailure]) = 0.
Proof. intros. reflexivity. Qed.

Example rejected_candidate_has_no_deployment_fee : forall schedule fresh,
  retained_phlo_charge schedule fresh PhloAdmissionRejected = 0.
Proof. intros. reflexivity. Qed.

Definition example_phlo_schedule (price : nat) : phlo_schedule :=
  {| phlo_schedule_commitment := 1; phlo_protocol_version := 1; phlo_network := 1; phlo_shard := 1;
     phlo_asset := 1; phlo_unit := 1; phlo_decimal_scale := 8; phlo_weights := [1; 2; 3; 4];
     phlo_actual_price := price |}.

Definition example_phlo_environment : phlo_environment :=
  {| active_phlo_version := 1; native_phlo_network := 1; native_phlo_shard := 1;
     native_phlo_asset := 1; native_phlo_unit := 1; native_phlo_decimal_scale := 8 |}.

Definition example_phlo_terms (owners : nat) : signed_phlo_controls :=
  {| signed_phlo_limit := 10; signed_phlo_price := 3;
     signed_required_ceilings := repeat 3 owners;
     signed_phlo_schedules := [example_phlo_schedule 2] |}.

Theorem any_positive_owner_count_can_authorize_the_same_bound : forall owners,
  0 < owners ->
  admit_phlo_controls example_phlo_environment 31 (example_phlo_terms owners) (example_phlo_schedule 2) 10 = true.
Proof.
  intros owners positive. apply admitted_phlo_controls_exact. simpl.
  repeat split; try lia; try (left; reflexivity).
  - destruct owners; [lia|discriminate].
  - apply Forall_forall. intros ceiling present. apply repeat_spec in present. subst. lia.
Qed.

Theorem changing_owner_count_does_not_multiply_charge : forall owners other,
  signed_charge_ceiling (example_phlo_terms owners) =
  signed_charge_ceiling (example_phlo_terms other).
Proof. reflexivity. Qed.

Example lower_price_does_not_authorize_another_schedule :
  admit_phlo_controls example_phlo_environment 31 (example_phlo_terms 3) (example_phlo_schedule 1) 10 = false.
Proof. vm_compute. reflexivity. Qed.

Example signed_arithmetic_overflow_rejects_admission :
  admit_phlo_controls example_phlo_environment 30 (example_phlo_terms 3) (example_phlo_schedule 2) 10 = false.
Proof. vm_compute. reflexivity. Qed.

Definition generated_phlo_case (owners work prepaid : nat) : bool :=
  let required := repeat (prepaid_sample 0 0 0 (SGround [true])) work in
  let used := firstn prepaid required in
  let fresh := skipn prepaid required in
  Bool.eqb
    (check_phlo_execution example_phlo_environment 31 (example_phlo_terms owners) (example_phlo_schedule 2) 10
      used required used [] fresh)
    ((0 <? owners) && (work <=? 10)).

Example generated_owner_usage_and_prepaid_regression :
  forallb (fun owners =>
    forallb (fun work => forallb (generated_phlo_case owners work) (seq 0 14)) (seq 0 13))
    [0; 1; 2; 3; 64; 65; 256] = true.
Proof. vm_compute. reflexivity. Qed.

Example prepaid_usage_still_exhausts_deployment_limit :
  let resources := repeat (prepaid_sample 0 0 0 (SGround [true])) 11 in
  check_phlo_execution example_phlo_environment 31 (example_phlo_terms 3) (example_phlo_schedule 2) 10
    resources resources resources [] [] = false.
Proof. vm_compute. reflexivity. Qed.

Example unknown_resource_class_does_not_become_free :
  let resources := [prepaid_sample 0 4 0 (SGround [true])] in
  execution_phlo (example_phlo_schedule 2) resources = 0 /\
  check_phlo_execution example_phlo_environment 31 (example_phlo_terms 3) (example_phlo_schedule 2) 10
    [] resources [] [] resources = false.
Proof. vm_compute. split; reflexivity. Qed.

Example even_explicit_signed_consent_cannot_redenominate_the_fee :
  let selected := {| phlo_schedule_commitment := 2; phlo_protocol_version := 1; phlo_network := 1; phlo_shard := 1;
                    phlo_asset := 2; phlo_unit := 1; phlo_decimal_scale := 8; phlo_weights := [1; 2; 3; 4];
                    phlo_actual_price := 2 |} in
  let terms := {| signed_phlo_limit := 10; signed_phlo_price := 3;
                 signed_required_ceilings := [3]; signed_phlo_schedules := [selected] |} in
  In selected (signed_phlo_schedules terms) /\
  admit_phlo_controls example_phlo_environment 31 terms selected 10 = false.
Proof. vm_compute. split; [auto|reflexivity]. Qed.
