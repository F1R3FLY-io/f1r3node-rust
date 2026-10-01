From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingConsentHistory SignedPhloControls SignedPhloFunding
  EligibleFundingAssignment.
Import ListNotations.

Definition phlo_controls_eq_dec : forall (left right : signed_phlo_controls),
  {left = right} + {left <> right}.
Proof. decide equality; auto using Nat.eq_dec, list_eq_dec, phlo_schedule_eq_dec. Defined.

Definition phlo_environment_eq_dec : forall (left right : phlo_environment),
  {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition funding_terms_eq_dec : forall (left right : funding_terms),
  {left = right} + {left <> right}.
Proof.
  decide equality; auto using Nat.eq_dec.
  - apply list_eq_dec. decide equality.
  - apply list_eq_dec. decide equality; apply Nat.eq_dec.
Defined.

Definition captured_reservation_eq_dec : forall (left right : captured_reservation),
  {left = right} + {left <> right}.
Proof. decide equality; auto using Nat.eq_dec, funding_terms_eq_dec. Defined.

Record phlo_source_consent := {
  consent_hold_cap : nat;
  consent_debit_cap : nat;
  consent_resource_permission : phlo_obligation_key -> bool
}.

Record phlo_intent := {
  intent_controls : signed_phlo_controls;
  intent_schedule_identity : nat;
  intent_total_exposure : nat;
  intent_source_consent : nat -> option phlo_source_consent
}.

Record phlo_snapshot := {
  snapshot_environment : phlo_environment;
  snapshot_machine_max : nat;
  snapshot_controls : signed_phlo_controls;
  snapshot_schedule : phlo_schedule;
  snapshot_bound : nat;
  snapshot_domain : phlo_funding_domain;
  snapshot_branches : list nat;
  snapshot_cases : nat -> phlo_execution_case;
  snapshot_demands : nat -> nat -> nat;
  snapshot_plans : nat -> funding_flow
}.

Definition check_phlo_snapshot snapshot :=
  check_phlo_family (snapshot_environment snapshot) (snapshot_machine_max snapshot)
    (snapshot_controls snapshot) (snapshot_schedule snapshot) (snapshot_bound snapshot)
    (snapshot_domain snapshot) (snapshot_branches snapshot) (snapshot_cases snapshot)
    (snapshot_demands snapshot) (snapshot_plans snapshot).

Definition check_intent_source intent snapshot source :=
  let domain := snapshot_domain snapshot in
  match intent_source_consent intent (funding_custody domain source) with
  | None => false
  | Some consent =>
      (signed_source_exposure domain source <=? consent_hold_cap consent) &&
      (signed_source_debit domain source <=? consent_debit_cap consent) &&
      forallb (fun branch => forallb (fun key =>
        negb (funding_permission domain branch source key) || consent_resource_permission consent key)
        (case_obligation_keys (snapshot_cases snapshot branch))) (snapshot_branches snapshot)
  end.

Definition check_decoded_intent terms intent snapshot :=
  if phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent) then
    if list_eq_dec Nat.eq_dec (required_ceilings terms)
      (signed_required_ceilings (intent_controls intent)) then
      (asset_identity terms =? phlo_asset (snapshot_schedule snapshot)) &&
      (schedule_identity terms =? intent_schedule_identity intent) &&
      (intent_schedule_identity intent =? phlo_schedule_commitment (snapshot_schedule snapshot)) &&
      (signed_total_exposure (snapshot_domain snapshot) <=? intent_total_exposure intent) &&
      forallb (check_intent_source intent snapshot) (seq 0 (funding_sources (snapshot_domain snapshot)))
    else false
  else false.

Definition check_phlo_family_intent intent snapshot :=
  if phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent) then
    (signed_total_exposure (snapshot_domain snapshot) <=? intent_total_exposure intent) &&
    forallb (check_intent_source intent snapshot) (seq 0 (funding_sources (snapshot_domain snapshot)))
  else false.

Theorem phlo_family_intent_check_exact : forall intent snapshot,
  check_phlo_family_intent intent snapshot = true <->
  snapshot_controls snapshot = intent_controls intent /\
  signed_total_exposure (snapshot_domain snapshot) <= intent_total_exposure intent /\
  forall source, source < funding_sources (snapshot_domain snapshot) ->
    check_intent_source intent snapshot source = true.
Proof.
  intros intent snapshot. unfold check_phlo_family_intent.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)) as [same|different].
  - rewrite andb_true_iff, Nat.leb_le, forallb_forall.
    setoid_rewrite in_seq. simpl. split.
    + intros [exposure sources]. repeat split; auto. intros source inside. apply sources. lia.
    + intros [_ [exposure sources]]. split; auto. intros source inside. apply sources. lia.
  - split; [discriminate|intros [same _]; contradiction].
Qed.

Theorem decoded_intent_factorizes_into_family_and_right_checks : forall terms intent snapshot,
  check_decoded_intent terms intent snapshot = true <->
  check_phlo_family_intent intent snapshot = true /\
  required_ceilings terms = signed_required_ceilings (intent_controls intent) /\
  asset_identity terms = phlo_asset (snapshot_schedule snapshot) /\
  schedule_identity terms = intent_schedule_identity intent /\
  intent_schedule_identity intent = phlo_schedule_commitment (snapshot_schedule snapshot).
Proof.
  intros terms intent snapshot. unfold check_decoded_intent, check_phlo_family_intent.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)) as [same|different].
  - destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
      (signed_required_ceilings (intent_controls intent))) as [owners|different_owners].
    + rewrite !andb_true_iff, !Nat.eqb_eq. tauto.
    + split; [discriminate|intros [_ [owners _]]; contradiction].
  - split; [discriminate|intros [impossible _]; discriminate].
Qed.

Print Assumptions decoded_intent_factorizes_into_family_and_right_checks.

Theorem decoded_intent_checks_funding_family : forall terms intent snapshot,
  check_decoded_intent terms intent snapshot = true ->
  check_phlo_family_intent intent snapshot = true.
Proof.
  intros terms intent snapshot checked.
  unfold check_decoded_intent in checked. unfold check_phlo_family_intent.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)); [|discriminate].
  destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
    (signed_required_ceilings (intent_controls intent))); [|discriminate].
  rewrite !andb_true_iff in *. tauto.
Qed.

Theorem family_intent_bounds_every_source : forall intent snapshot source,
  check_phlo_family_intent intent snapshot = true ->
  source < funding_sources (snapshot_domain snapshot) ->
  exists consent,
    intent_source_consent intent (funding_custody (snapshot_domain snapshot) source) = Some consent /\
    signed_source_exposure (snapshot_domain snapshot) source <= consent_hold_cap consent /\
    signed_source_debit (snapshot_domain snapshot) source <= consent_debit_cap consent /\
    forall branch key, In branch (snapshot_branches snapshot) ->
      In key (case_obligation_keys (snapshot_cases snapshot branch)) ->
      funding_permission (snapshot_domain snapshot) branch source key = true ->
      consent_resource_permission consent key = true.
Proof.
  intros intent snapshot source checked inside.
  apply phlo_family_intent_check_exact in checked.
  destruct checked as [_ [_ sources]]. specialize (sources source inside).
  unfold check_intent_source in sources.
  destruct (intent_source_consent intent (funding_custody (snapshot_domain snapshot) source))
    as [consent|] eqn:found; [|discriminate].
  rewrite !andb_true_iff, !Nat.leb_le in sources.
  destruct sources as [[exposure debit] permissions].
  exists consent. repeat split; auto.
  intros branch key branch_in key_in permitted.
  rewrite forallb_forall in permissions. specialize (permissions branch branch_in).
  rewrite forallb_forall in permissions. specialize (permissions key key_in).
  rewrite permitted in permissions. exact permissions.
Qed.

Print Assumptions phlo_family_intent_check_exact.
Print Assumptions decoded_intent_checks_funding_family.
Print Assumptions family_intent_bounds_every_source.

Definition equivalent_phlo_source_consent first second :=
  consent_hold_cap first = consent_hold_cap second /\
  consent_debit_cap first = consent_debit_cap second /\
  forall key, consent_resource_permission first key = consent_resource_permission second key.

Definition equivalent_phlo_source_maps first second := forall custody,
  match intent_source_consent first custody, intent_source_consent second custody with
  | None, None => True
  | Some lhs, Some rhs => equivalent_phlo_source_consent lhs rhs
  | _, _ => False
  end.

Lemma phlo_forallb_pointwise : forall A (first second : A -> bool) items,
  (forall item, first item = second item) -> forallb first items = forallb second items.
Proof.
  intros A first second items same. induction items as [|item rest IH]; cbn; [reflexivity|].
  rewrite same, IH. reflexivity.
Qed.

Theorem equivalent_source_encodings_preserve_source_check : forall first second snapshot source,
  equivalent_phlo_source_maps first second ->
  check_intent_source first snapshot source = check_intent_source second snapshot source.
Proof.
  intros first second snapshot source equivalent. unfold check_intent_source.
  specialize (equivalent (funding_custody (snapshot_domain snapshot) source)).
  destruct (intent_source_consent first (funding_custody (snapshot_domain snapshot) source)) as [lhs|];
    destruct (intent_source_consent second (funding_custody (snapshot_domain snapshot) source)) as [rhs|];
    try contradiction; try reflexivity.
  destruct equivalent as [holds [debits permissions]]. rewrite holds, debits. f_equal.
  apply phlo_forallb_pointwise. intros branch. apply phlo_forallb_pointwise. intros key. rewrite permissions. reflexivity.
Qed.

Theorem equivalent_source_encodings_preserve_family_check : forall first second snapshot,
  intent_controls first = intent_controls second ->
  intent_total_exposure first = intent_total_exposure second ->
  equivalent_phlo_source_maps first second ->
  check_phlo_family_intent first snapshot = check_phlo_family_intent second snapshot.
Proof.
  intros first second snapshot controls exposure equivalent. unfold check_phlo_family_intent.
  rewrite controls, exposure.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls second)); [|reflexivity].
  f_equal. apply phlo_forallb_pointwise. intros source.
  now apply equivalent_source_encodings_preserve_source_check.
Qed.

Print Assumptions equivalent_source_encodings_preserve_source_check.
Print Assumptions phlo_forallb_pointwise.
Print Assumptions equivalent_source_encodings_preserve_family_check.

Record accepted_phlo_snapshot := {
  snapshot_reservation : captured_reservation;
  snapshot_intent : phlo_intent;
  snapshot_value : phlo_snapshot
}.

Record phlo_capture_state := {
  capture_consents : consent_state;
  accepted_snapshots : nat -> option accepted_phlo_snapshot;
  settled_phlo_branches : nat -> option nat
}.

Definition capture_phlo (decode_signed_intent : list bool -> option phlo_intent)
  state key generation operation root snapshot : option phlo_capture_state :=
  match accepted_snapshots state operation, live_rights (capture_consents state) key with
  | None, Some current =>
      if settled (capture_consents state) operation then None else
      match decode_signed_intent (signed_limit_terms (right_terms current)) with
      | None => None
      | Some intent =>
          match reserve_right (capture_consents state) key generation operation root
            (phlo_actual_price (snapshot_schedule snapshot))
            (check_decoded_intent (right_terms current) intent snapshot && check_phlo_snapshot snapshot) with
          | None => None
          | Some next =>
              match reservations next operation with
              | None => None
              | Some reservation => Some {|
                  capture_consents := next; settled_phlo_branches := settled_phlo_branches state;
                  accepted_snapshots := replace_at (accepted_snapshots state) operation
                    (Some {| snapshot_reservation := reservation; snapshot_intent := intent; snapshot_value := snapshot |}) |}
              end
          end
      end
  | _, _ => None
  end.

Definition transfer_phlo_history state commands : option phlo_capture_state :=
  match run_transfers (capture_consents state) commands with
  | None => None
  | Some next => Some {| capture_consents := next; accepted_snapshots := accepted_snapshots state;
                       settled_phlo_branches := settled_phlo_branches state |}
  end.

Record phlo_receipt := {
  receipt_operation : nat;
  receipt_reservation : captured_reservation;
  receipt_environment : phlo_environment;
  receipt_controls : signed_phlo_controls;
  receipt_schedule : phlo_schedule;
  receipt_branch : nat;
  receipt_debits : list (nat * nat);
  receipt_refunds : list (nat * nat)
}.

Definition phlo_receipt_eq_dec : forall (left right : phlo_receipt),
  {left = right} + {left <> right}.
Proof.
  decide equality; auto using Nat.eq_dec, captured_reservation_eq_dec,
    phlo_environment_eq_dec, phlo_controls_eq_dec, phlo_schedule_eq_dec;
    apply list_eq_dec; decide equality; apply Nat.eq_dec.
Defined.

Definition captured_phlo_receipt operation captured branch : phlo_receipt :=
  let snapshot := snapshot_value captured in
  let domain := snapshot_domain snapshot in
  {| receipt_operation := operation; receipt_reservation := snapshot_reservation captured;
     receipt_environment := snapshot_environment snapshot;
     receipt_controls := snapshot_controls snapshot; receipt_schedule := snapshot_schedule snapshot;
     receipt_branch := branch;
     receipt_debits := map (fun source => (funding_custody domain source,
       source_draw (funding_obligations domain) (snapshot_plans snapshot branch) source))
       (seq 0 (funding_sources domain));
     receipt_refunds := captured_phlo_refunds domain (snapshot_branches snapshot) (snapshot_plans snapshot) branch |}.

Definition prepare_phlo_receipt state operation branch : option phlo_receipt :=
  match settlement_evidence (capture_consents state) operation, accepted_snapshots state operation with
  | Some reservation, Some captured =>
      if captured_reservation_eq_dec reservation (snapshot_reservation captured) then
        if in_dec Nat.eq_dec branch (snapshot_branches (snapshot_value captured)) then
          Some (captured_phlo_receipt operation captured branch)
        else None
      else None
  | _, _ => None
  end.

Definition reconstruct_phlo_candidate state operation branch : option phlo_receipt :=
  match reservations (capture_consents state) operation, accepted_snapshots state operation with
  | Some reservation, Some captured =>
      if captured_reservation_eq_dec reservation (snapshot_reservation captured) then
        if in_dec Nat.eq_dec branch (snapshot_branches (snapshot_value captured)) then
          Some (captured_phlo_receipt operation captured branch)
        else None
      else None
  | _, _ => None
  end.

Definition replay_phlo_receipt state operation branch : option phlo_receipt :=
  match settled_phlo_branches state operation with
  | None => None
  | Some selected =>
      if settled (capture_consents state) operation && (branch =? selected) then
        reconstruct_phlo_candidate state operation branch
      else None
  end.

Definition settle_phlo state operation branch expected : option (phlo_receipt * phlo_capture_state) :=
  match prepare_phlo_receipt state operation branch with
  | None => None
  | Some actual =>
      if phlo_receipt_eq_dec actual expected then
        match settle_reservation (capture_consents state) operation with
        | None => None
        | Some (_, next) => Some (actual,
            {| capture_consents := next; accepted_snapshots := accepted_snapshots state;
               settled_phlo_branches := replace_at (settled_phlo_branches state) operation (Some branch) |})
        end
      else None
  end.

Theorem decoded_intent_binds_controls : forall terms intent snapshot,
  check_decoded_intent terms intent snapshot = true ->
  snapshot_controls snapshot = intent_controls intent /\
  required_ceilings terms = signed_required_ceilings (intent_controls intent).
Proof.
  intros. unfold check_decoded_intent in H.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)); try discriminate.
  destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
    (signed_required_ceilings (intent_controls intent))); try discriminate. auto.
Qed.

Theorem decoded_intent_binds_actual_schedule : forall terms intent snapshot,
  check_decoded_intent terms intent snapshot = true ->
  schedule_identity terms = intent_schedule_identity intent /\
  intent_schedule_identity intent = phlo_schedule_commitment (snapshot_schedule snapshot).
Proof.
  intros terms intent snapshot checked. unfold check_decoded_intent in checked.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)); [|discriminate].
  destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
    (signed_required_ceilings (intent_controls intent))); [|discriminate].
  rewrite !andb_true_iff, !Nat.eqb_eq in checked. tauto.
Qed.

Theorem different_execution_schedule_cannot_capture_consent : forall terms intent snapshot,
  schedule_identity terms <> phlo_schedule_commitment (snapshot_schedule snapshot) ->
  check_decoded_intent terms intent snapshot = false.
Proof.
  intros terms intent snapshot different.
  destruct (check_decoded_intent terms intent snapshot) eqn:checked; [|reflexivity].
  apply decoded_intent_binds_actual_schedule in checked. destruct checked as [declared actual]. congruence.
Qed.

Print Assumptions decoded_intent_binds_actual_schedule.
Print Assumptions different_execution_schedule_cannot_capture_consent.

Theorem decoded_intent_bounds_source_consent : forall terms intent snapshot source,
  check_decoded_intent terms intent snapshot = true ->
  source < funding_sources (snapshot_domain snapshot) ->
  exists consent,
    intent_source_consent intent (funding_custody (snapshot_domain snapshot) source) = Some consent /\
    signed_source_exposure (snapshot_domain snapshot) source <= consent_hold_cap consent /\
    signed_source_debit (snapshot_domain snapshot) source <= consent_debit_cap consent.
Proof.
  intros terms intent snapshot source checked inside.
  unfold check_decoded_intent in checked.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)); try discriminate.
  destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
    (signed_required_ceilings (intent_controls intent))); try discriminate.
  apply andb_true_iff in checked. destruct checked as [_ sources].
  apply funding_forallb_range with (index := source) in sources; [|exact inside].
  unfold check_intent_source in sources.
  destruct (intent_source_consent intent (funding_custody (snapshot_domain snapshot) source)) as [consent|] eqn:found;
    try discriminate.
  rewrite !andb_true_iff, !Nat.leb_le in sources. exists consent. tauto.
Qed.

Theorem decoded_permission_cannot_be_expanded : forall terms intent snapshot source branch key,
  check_decoded_intent terms intent snapshot = true -> source < funding_sources (snapshot_domain snapshot) ->
  In branch (snapshot_branches snapshot) -> In key (case_obligation_keys (snapshot_cases snapshot branch)) ->
  funding_permission (snapshot_domain snapshot) branch source key = true ->
  exists consent, intent_source_consent intent (funding_custody (snapshot_domain snapshot) source) = Some consent /\
    consent_resource_permission consent key = true.
Proof.
  intros terms intent snapshot source branch key checked inside branch_in key_in permitted.
  unfold check_decoded_intent in checked.
  destruct (phlo_controls_eq_dec (snapshot_controls snapshot) (intent_controls intent)); try discriminate.
  destruct (list_eq_dec Nat.eq_dec (required_ceilings terms)
    (signed_required_ceilings (intent_controls intent))); try discriminate.
  apply andb_true_iff in checked. destruct checked as [_ sources].
  apply funding_forallb_range with (index := source) in sources; [|exact inside].
  unfold check_intent_source in sources.
  destruct (intent_source_consent intent (funding_custody (snapshot_domain snapshot) source)) as [consent|] eqn:found;
    try discriminate.
  apply andb_true_iff in sources. destruct sources as [_ branches].
  rewrite forallb_forall in branches. specialize (branches branch branch_in).
  rewrite forallb_forall in branches. specialize (branches key key_in).
  rewrite permitted in branches. simpl in branches. exists consent. auto.
Qed.

Theorem transfers_preserve_phlo_snapshots : forall state commands next,
  transfer_phlo_history state commands = Some next ->
  accepted_snapshots next = accepted_snapshots state.
Proof.
  intros. unfold transfer_phlo_history in H.
  destruct (run_transfers (capture_consents state) commands); try discriminate.
  inversion H. reflexivity.
Qed.

Theorem transfers_preserve_prepared_receipt : forall state commands next operation branch,
  transfer_phlo_history state commands = Some next ->
  prepare_phlo_receipt next operation branch = prepare_phlo_receipt state operation branch.
Proof.
  intros state commands next operation branch transferred.
  unfold transfer_phlo_history in transferred.
  destruct (run_transfers (capture_consents state) commands) as [updated|] eqn:ran; try discriminate.
  inversion transferred; subst. unfold prepare_phlo_receipt. simpl.
  rewrite (arbitrary_transfer_history_preserves_settlement_evidence _ _ _ operation ran). reflexivity.
Qed.

Theorem settlement_returns_only_the_captured_receipt : forall state operation branch expected actual next,
  settle_phlo state operation branch expected = Some (actual, next) ->
  prepare_phlo_receipt state operation branch = Some actual /\ actual = expected /\
  accepted_snapshots next = accepted_snapshots state.
Proof.
  intros. unfold settle_phlo in H.
  destruct (prepare_phlo_receipt state operation branch) as [receipt|] eqn:prepared; try discriminate.
  destruct (phlo_receipt_eq_dec receipt expected); try discriminate.
  destruct (settle_reservation (capture_consents state) operation) as [[reservation updated]|]; try discriminate.
  inversion H; subst. auto.
Qed.

Theorem successful_settlement_closes_the_operation : forall state operation branch expected actual next,
  settle_phlo state operation branch expected = Some (actual, next) ->
  forall later_branch, prepare_phlo_receipt next operation later_branch = None.
Proof.
  intros state operation branch expected actual next result later_branch.
  unfold settle_phlo in result.
  destruct (prepare_phlo_receipt state operation branch); try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  destruct (settle_reservation (capture_consents state) operation) as [[reservation updated]|] eqn:closed; try discriminate.
  inversion result; subst. unfold prepare_phlo_receipt. simpl.
  pose proof (settled_reservation_cannot_settle_twice _ _ _ _ closed) as repeat_closed.
  unfold settle_reservation in repeat_closed.
  destruct (settlement_evidence updated operation); [discriminate|reflexivity].
Qed.

Theorem phlo_settlement_cannot_repeat : forall state operation branch expected actual next later_branch later_receipt,
  settle_phlo state operation branch expected = Some (actual, next) ->
  settle_phlo next operation later_branch later_receipt = None.
Proof.
  intros. unfold settle_phlo.
  rewrite (successful_settlement_closes_the_operation _ _ _ _ _ _ H later_branch). reflexivity.
Qed.

Theorem ownership_changes_cannot_reopen_settlement : forall state operation branch expected actual next commands later
  later_branch later_receipt,
  settle_phlo state operation branch expected = Some (actual, next) ->
  transfer_phlo_history next commands = Some later ->
  settle_phlo later operation later_branch later_receipt = None.
Proof.
  intros. unfold settle_phlo.
  rewrite (transfers_preserve_prepared_receipt _ _ _ operation later_branch H0).
  rewrite (successful_settlement_closes_the_operation _ _ _ _ _ _ H later_branch). reflexivity.
Qed.

Theorem mismatched_receipt_cannot_close : forall state operation branch actual expected,
  prepare_phlo_receipt state operation branch = Some actual -> actual <> expected ->
  settle_phlo state operation branch expected = None.
Proof.
  intros. unfold settle_phlo. rewrite H.
  destruct (phlo_receipt_eq_dec actual expected); [contradiction|reflexivity].
Qed.

Definition phlo_capture_valid decode state : Prop :=
  forall operation,
    match accepted_snapshots state operation, reservations (capture_consents state) operation with
    | None, None => settled (capture_consents state) operation = false
    | Some captured, Some reservation =>
        snapshot_reservation captured = reservation /\
        check_phlo_snapshot (snapshot_value captured) = true /\
        decode (signed_limit_terms (reservation_terms reservation)) = Some (snapshot_intent captured) /\
        check_decoded_intent (reservation_terms reservation) (snapshot_intent captured) (snapshot_value captured) = true /\
        reservation_price reservation = phlo_actual_price (snapshot_schedule (snapshot_value captured))
    | _, _ => False
    end.

Theorem reserve_requires_valid_proof : forall state key generation operation root price valid next,
  reserve_right state key generation operation root price valid = Some next -> valid = true.
Proof.
  intros. unfold reserve_right in H.
  destruct (reservations state operation); try discriminate.
  destruct (live_rights state key); try discriminate.
  destruct (FundingPriceConsent.required_price_ceiling (required_ceilings (right_terms l))); try discriminate.
  destruct (valid && (generation =? right_generation l) && (price <=? n)) eqn:guard; try discriminate.
  rewrite !andb_true_iff in guard. tauto.
Qed.

Theorem reserve_preserves_other_records_and_flags : forall state key generation operation root price valid next,
  reserve_right state key generation operation root price valid = Some next ->
  settled next = settled state /\
  forall other, other <> operation -> reservations next other = reservations state other.
Proof.
  intros. unfold reserve_right in H.
  destruct (reservations state operation); try discriminate.
  destruct (live_rights state key); try discriminate.
  destruct (FundingPriceConsent.required_price_ceiling (required_ceilings (right_terms l))); try discriminate.
  destruct (valid && (generation =? right_generation l) && (price <=? n)); try discriminate.
  inversion H; subst. split; [reflexivity|]. intros other distinct. simpl.
  unfold replace_at. apply Nat.eqb_neq in distinct. now rewrite distinct.
Qed.

Theorem capture_preserves_checked_pairing : forall decode state key generation operation root snapshot next,
  phlo_capture_valid decode state ->
  capture_phlo decode state key generation operation root snapshot = Some next ->
  phlo_capture_valid decode next.
Proof.
  intros decode state key generation operation root snapshot next valid captured.
  unfold capture_phlo in captured.
  destruct (accepted_snapshots state operation) eqn:absent; try discriminate.
  destruct (live_rights (capture_consents state) key) as [current|] eqn:current_right; try discriminate.
  destruct (settled (capture_consents state) operation) eqn:open; try discriminate.
  destruct (decode (signed_limit_terms (right_terms current))) as [intent|] eqn:decoded; try discriminate.
  destruct (reserve_right (capture_consents state) key generation operation root
    (phlo_actual_price (snapshot_schedule snapshot))
    (check_decoded_intent (right_terms current) intent snapshot && check_phlo_snapshot snapshot))
    as [reserved|] eqn:reserved_result; try discriminate.
  destruct (reservations reserved operation) as [reservation|] eqn:found; try discriminate.
  pose proof (reserve_requires_valid_proof _ _ _ _ _ _ _ _ reserved_result) as checked.
  apply andb_true_iff in checked. destruct checked as [authorized checked].
  pose proof (reservation_captures_current_consent _ _ _ _ _ _ _ _ reserved_result)
    as [actual [actual_right [actual_capture _]]].
  rewrite current_right in actual_right. inversion actual_right; subst actual.
  rewrite found in actual_capture. inversion actual_capture; subst reservation.
  pose proof (reserve_preserves_other_records_and_flags _ _ _ _ _ _ _ _ reserved_result) as [flags others].
  inversion captured; subst next. unfold phlo_capture_valid. intro queried. simpl.
  unfold replace_at. destruct (Nat.eq_dec queried operation) as [same|different].
  - subst queried. rewrite Nat.eqb_refl, found. simpl. auto.
  - assert (queried =? operation = false) as unequal by (apply Nat.eqb_neq; exact different).
    rewrite unequal, others by exact different. rewrite flags. apply valid.
Qed.

Theorem transfer_history_preserves_checked_pairing : forall decode state commands next,
  phlo_capture_valid decode state -> transfer_phlo_history state commands = Some next ->
  phlo_capture_valid decode next.
Proof.
  intros decode state commands next valid transferred. unfold transfer_phlo_history in transferred.
  destruct (run_transfers (capture_consents state) commands) as [updated|] eqn:ran; try discriminate.
  inversion transferred; subst. unfold phlo_capture_valid. simpl.
  pose proof (arbitrary_transfer_history_preserves_reservations _ _ _ ran) as [records flags].
  rewrite records, flags. exact valid.
Qed.

Theorem settlement_preserves_checked_pairing : forall decode state operation branch expected actual next,
  phlo_capture_valid decode state -> settle_phlo state operation branch expected = Some (actual, next) ->
  phlo_capture_valid decode next.
Proof.
  intros decode state operation branch expected actual next valid closed.
  unfold settle_phlo in closed.
  destruct (prepare_phlo_receipt state operation branch) as [receipt|] eqn:prepared; try discriminate.
  destruct (phlo_receipt_eq_dec receipt expected); try discriminate.
  unfold settle_reservation in closed.
  destruct (settlement_evidence (capture_consents state) operation) as [reservation|] eqn:evidence; try discriminate.
  inversion closed; subst next. unfold phlo_capture_valid. intro queried. simpl.
  specialize (valid queried).
  destruct (accepted_snapshots state queried) as [snapshot|] eqn:snap;
    destruct (reservations (capture_consents state) queried) as [record|] eqn:rec; try contradiction.
  - exact valid.
  - unfold replace_at. destruct (Nat.eq_dec queried operation) as [same|different].
    + subst queried. unfold settlement_evidence in evidence.
      destruct (settled (capture_consents state) operation); rewrite ?rec in evidence; discriminate.
    + apply Nat.eqb_neq in different. rewrite different. exact valid.
Qed.

Definition empty_phlo_capture_state rights : phlo_capture_state :=
  {| capture_consents := {| live_rights := rights; reservations := fun _ => None; settled := fun _ => false |};
     accepted_snapshots := fun _ => None; settled_phlo_branches := fun _ => None |}.

Theorem empty_capture_state_is_valid : forall decode rights,
  phlo_capture_valid decode (empty_phlo_capture_state rights).
Proof. intros. intro operation. reflexivity. Qed.

Inductive phlo_capture_transition decode : phlo_capture_state -> phlo_capture_state -> Prop :=
| PhloCaptureTransition : forall state key generation operation root snapshot next,
    capture_phlo decode state key generation operation root snapshot = Some next ->
    phlo_capture_transition decode state next
| PhloTransferTransition : forall state commands next,
    transfer_phlo_history state commands = Some next -> phlo_capture_transition decode state next
| PhloSettleTransition : forall state operation branch expected actual next,
    settle_phlo state operation branch expected = Some (actual, next) -> phlo_capture_transition decode state next.

Inductive phlo_capture_history decode : phlo_capture_state -> phlo_capture_state -> Prop :=
| PhloHistoryRefl : forall state, phlo_capture_history decode state state
| PhloHistoryStep : forall first middle last,
    phlo_capture_transition decode first middle -> phlo_capture_history decode middle last ->
    phlo_capture_history decode first last.

Theorem arbitrary_capture_history_preserves_checked_pairing : forall decode first last,
  phlo_capture_history decode first last -> phlo_capture_valid decode first -> phlo_capture_valid decode last.
Proof.
  intros decode first last history. induction history; intros valid; [exact valid|].
  apply IHhistory. destruct H.
  - eapply capture_preserves_checked_pairing; eauto.
  - eapply transfer_history_preserves_checked_pairing; eauto.
  - eapply settlement_preserves_checked_pairing; eauto.
Qed.

Theorem capture_preserves_closure_flags : forall decode state key generation operation root snapshot next,
  capture_phlo decode state key generation operation root snapshot = Some next ->
  settled (capture_consents next) = settled (capture_consents state).
Proof.
  intros. unfold capture_phlo in H.
  destruct (accepted_snapshots state operation); try discriminate.
  destruct (live_rights (capture_consents state) key); try discriminate.
  destruct (settled (capture_consents state) operation); try discriminate.
  destruct (decode (signed_limit_terms (right_terms l))); try discriminate.
  destruct (reserve_right (capture_consents state) key generation operation root
    (phlo_actual_price (snapshot_schedule snapshot))
    (check_decoded_intent (right_terms l) p snapshot && check_phlo_snapshot snapshot)) as [reserved|] eqn:ran;
    try discriminate.
  destruct (reservations reserved operation); try discriminate.
  inversion H; subst. simpl.
  pose proof (reserve_preserves_other_records_and_flags _ _ _ _ _ _ _ _ ran). tauto.
Qed.

Theorem capture_preserves_existing_snapshots : forall decode state key generation operation root snapshot next prior existing,
  capture_phlo decode state key generation operation root snapshot = Some next ->
  accepted_snapshots state prior = Some existing -> accepted_snapshots next prior = Some existing.
Proof.
  intros. unfold capture_phlo in H.
  destruct (accepted_snapshots state operation) eqn:absent; try discriminate.
  destruct (live_rights (capture_consents state) key); try discriminate.
  destruct (settled (capture_consents state) operation); try discriminate.
  destruct (decode (signed_limit_terms (right_terms l))); try discriminate.
  destruct (reserve_right (capture_consents state) key generation operation root
    (phlo_actual_price (snapshot_schedule snapshot))
    (check_decoded_intent (right_terms l) p snapshot && check_phlo_snapshot snapshot)); try discriminate.
  destruct (reservations c operation); try discriminate.
  inversion H; subst. simpl. unfold replace_at.
  destruct (Nat.eq_dec prior operation) as [same|different].
  - subst prior. rewrite absent in H0. discriminate.
  - apply Nat.eqb_neq in different. rewrite different. exact H0.
Qed.

Theorem arbitrary_capture_history_preserves_existing_snapshots : forall decode first last operation captured,
  phlo_capture_history decode first last -> accepted_snapshots first operation = Some captured ->
  accepted_snapshots last operation = Some captured.
Proof.
  intros decode first last operation captured history. induction history; intros found; [exact found|].
  apply IHhistory. destruct H.
  - eapply capture_preserves_existing_snapshots; eauto.
  - rewrite (transfers_preserve_phlo_snapshots _ _ _ H). exact found.
  - pose proof (settlement_returns_only_the_captured_receipt _ _ _ _ _ _ H) as [_ [_ same]].
    rewrite same. exact found.
Qed.

Theorem candidate_reconstruction_uses_the_checked_snapshot : forall decode state operation branch captured,
  phlo_capture_valid decode state -> accepted_snapshots state operation = Some captured ->
  reconstruct_phlo_candidate state operation branch =
    if in_dec Nat.eq_dec branch (snapshot_branches (snapshot_value captured)) then
      Some (captured_phlo_receipt operation captured branch) else None.
Proof.
  intros decode state operation branch captured valid found.
  specialize (valid operation). rewrite found in valid.
  unfold reconstruct_phlo_candidate. rewrite found.
  destruct (reservations (capture_consents state) operation); try contradiction.
  destruct valid as [same _]. subst c.
  destruct (captured_reservation_eq_dec (snapshot_reservation captured) (snapshot_reservation captured));
    [reflexivity|contradiction].
Qed.

Theorem arbitrary_history_preserves_candidate_reconstruction : forall decode first last operation branch captured,
  phlo_capture_valid decode first -> phlo_capture_history decode first last ->
  accepted_snapshots first operation = Some captured ->
  reconstruct_phlo_candidate last operation branch = reconstruct_phlo_candidate first operation branch.
Proof.
  intros decode first last operation branch captured valid history found.
  pose proof (arbitrary_capture_history_preserves_checked_pairing _ _ _ history valid) as later_valid.
  pose proof (arbitrary_capture_history_preserves_existing_snapshots _ _ _ _ _ history found) as later_found.
  rewrite (candidate_reconstruction_uses_the_checked_snapshot _ _ _ _ _ valid found).
  apply candidate_reconstruction_uses_the_checked_snapshot with (decode := decode); assumption.
Qed.

Theorem settlement_preserves_previous_closures : forall state operation branch expected actual next prior,
  settle_phlo state operation branch expected = Some (actual, next) ->
  settled (capture_consents state) prior = true -> settled (capture_consents next) prior = true.
Proof.
  intros. unfold settle_phlo in H.
  destruct (prepare_phlo_receipt state operation branch); try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  unfold settle_reservation in H.
  destruct (settlement_evidence (capture_consents state) operation); try discriminate.
  inversion H; subst. simpl. unfold replace_at.
  destruct (prior =? operation); auto.
Qed.

Theorem arbitrary_capture_history_preserves_closure : forall decode first last operation,
  phlo_capture_history decode first last ->
  settled (capture_consents first) operation = true -> settled (capture_consents last) operation = true.
Proof.
  intros decode first last operation history. induction history; intros closed; [exact closed|].
  apply IHhistory. destruct H.
  - rewrite (capture_preserves_closure_flags _ _ _ _ _ _ _ _ H). exact closed.
  - unfold transfer_phlo_history in H.
    destruct (run_transfers (capture_consents state) commands) as [updated|] eqn:ran; try discriminate.
    inversion H; subst. simpl.
    pose proof (arbitrary_transfer_history_preserves_reservations _ _ _ ran) as [_ flags].
    rewrite flags. exact closed.
  - eapply settlement_preserves_previous_closures; eauto.
Qed.

Theorem closed_operation_cannot_settle_in_any_later_history : forall decode first last operation branch receipt,
  phlo_capture_history decode first last -> settled (capture_consents first) operation = true ->
  settle_phlo last operation branch receipt = None.
Proof.
  intros. pose proof (arbitrary_capture_history_preserves_closure _ _ _ _ H H0) as closed.
  unfold settle_phlo, prepare_phlo_receipt, settlement_evidence. rewrite closed. reflexivity.
Qed.

Theorem prepared_receipt_has_checked_captured_inputs : forall decode state operation branch receipt,
  phlo_capture_valid decode state -> prepare_phlo_receipt state operation branch = Some receipt ->
  exists captured,
    accepted_snapshots state operation = Some captured /\
    receipt = captured_phlo_receipt operation captured branch /\
    In branch (snapshot_branches (snapshot_value captured)) /\
    check_phlo_snapshot (snapshot_value captured) = true /\
    decode (signed_limit_terms (reservation_terms (snapshot_reservation captured))) = Some (snapshot_intent captured).
Proof.
  intros decode state operation branch receipt valid prepared.
  unfold prepare_phlo_receipt in prepared.
  destruct (settlement_evidence (capture_consents state) operation) as [reservation|] eqn:evidence; try discriminate.
  destruct (accepted_snapshots state operation) as [captured|] eqn:found; try discriminate.
  destruct (captured_reservation_eq_dec reservation (snapshot_reservation captured)); try discriminate.
  destruct (in_dec Nat.eq_dec branch (snapshot_branches (snapshot_value captured))); try discriminate.
  inversion prepared; subst receipt. exists captured. repeat split; try assumption; try reflexivity.
  all: specialize (valid operation); rewrite found in valid;
    destruct (reservations (capture_consents state) operation) as [record|] eqn:stored; try contradiction;
    destruct valid as [same [checked [decoded authorized]]]; subst record; assumption.
Qed.

Theorem capture_cannot_replace_an_existing_operation : forall decode state operation captured key generation root snapshot,
  accepted_snapshots state operation = Some captured ->
  capture_phlo decode state key generation operation root snapshot = None.
Proof.
  intros. unfold capture_phlo. rewrite H.
  destruct (live_rights (capture_consents state) key); reflexivity.
Qed.

Definition receipt_with_schedule receipt schedule : phlo_receipt :=
  {| receipt_operation := receipt_operation receipt; receipt_reservation := receipt_reservation receipt;
     receipt_environment := receipt_environment receipt; receipt_controls := receipt_controls receipt;
     receipt_schedule := schedule; receipt_branch := receipt_branch receipt;
     receipt_debits := receipt_debits receipt; receipt_refunds := receipt_refunds receipt |}.

Theorem changing_captured_schedule_rejects_settlement : forall state operation branch receipt schedule,
  prepare_phlo_receipt state operation branch = Some receipt ->
  receipt_schedule receipt <> schedule ->
  settle_phlo state operation branch (receipt_with_schedule receipt schedule) = None.
Proof.
  intros. eapply mismatched_receipt_cannot_close; [exact H|].
  intro same. apply H0. apply (f_equal receipt_schedule) in same. exact same.
Qed.

Theorem successful_settlement_records_selected_branch : forall state operation branch expected actual next,
  settle_phlo state operation branch expected = Some (actual, next) ->
  settled_phlo_branches next operation = Some branch.
Proof.
  intros. unfold settle_phlo in H.
  destruct (prepare_phlo_receipt state operation branch); try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  destruct (settle_reservation (capture_consents state) operation); try discriminate.
  destruct p0. inversion H; subst. simpl. unfold replace_at. now rewrite Nat.eqb_refl.
Qed.

Theorem capture_preserves_settled_selections : forall decode state key generation operation root snapshot next,
  capture_phlo decode state key generation operation root snapshot = Some next ->
  settled_phlo_branches next = settled_phlo_branches state.
Proof.
  intros. unfold capture_phlo in H.
  destruct (accepted_snapshots state operation); try discriminate.
  destruct (live_rights (capture_consents state) key); try discriminate.
  destruct (settled (capture_consents state) operation); try discriminate.
  destruct (decode (signed_limit_terms (right_terms l))); try discriminate.
  destruct (reserve_right (capture_consents state) key generation operation root
    (phlo_actual_price (snapshot_schedule snapshot))
    (check_decoded_intent (right_terms l) p snapshot && check_phlo_snapshot snapshot)); try discriminate.
  destruct (reservations c operation); try discriminate.
  inversion H; reflexivity.
Qed.

Theorem settlement_preserves_closed_selections : forall state operation branch expected actual next prior,
  settle_phlo state operation branch expected = Some (actual, next) ->
  settled (capture_consents state) prior = true ->
  settled_phlo_branches next prior = settled_phlo_branches state prior.
Proof.
  intros state operation branch expected actual next prior ran closed.
  assert (prior <> operation) as distinct.
  { intro same. subst prior. unfold settle_phlo, prepare_phlo_receipt, settlement_evidence in ran.
    rewrite closed in ran. discriminate. }
  unfold settle_phlo in ran.
  destruct (prepare_phlo_receipt state operation branch); try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  destruct (settle_reservation (capture_consents state) operation); try discriminate.
  destruct p0. inversion ran; subst. simpl. unfold replace_at.
  apply Nat.eqb_neq in distinct. now rewrite distinct.
Qed.

Theorem arbitrary_history_preserves_closed_selection : forall decode first last operation,
  phlo_capture_history decode first last -> settled (capture_consents first) operation = true ->
  settled_phlo_branches last operation = settled_phlo_branches first operation.
Proof.
  intros decode first last operation history. induction history; intros closed; [reflexivity|].
  assert (middle_closed : settled (capture_consents middle) operation = true).
  { eapply arbitrary_capture_history_preserves_closure; [|exact closed].
    eapply PhloHistoryStep; [exact H|constructor]. }
  rewrite (IHhistory middle_closed). destruct H.
  - rewrite (capture_preserves_settled_selections _ _ _ _ _ _ _ _ H). reflexivity.
  - unfold transfer_phlo_history in H.
    destruct (run_transfers (capture_consents state) commands); try discriminate. inversion H; reflexivity.
  - eapply settlement_preserves_closed_selections; eauto.
Qed.

Theorem prepared_receipt_matches_candidate_reconstruction : forall state operation branch receipt,
  prepare_phlo_receipt state operation branch = Some receipt ->
  reconstruct_phlo_candidate state operation branch = Some receipt.
Proof.
  intros. unfold prepare_phlo_receipt, settlement_evidence in H.
  destruct (settled (capture_consents state) operation); try discriminate.
  exact H.
Qed.

Theorem successful_settlement_sets_closed_flag : forall state operation branch expected actual next,
  settle_phlo state operation branch expected = Some (actual, next) ->
  settled (capture_consents next) operation = true.
Proof.
  intros. unfold settle_phlo in H.
  destruct (prepare_phlo_receipt state operation branch); try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  unfold settle_reservation in H.
  destruct (settlement_evidence (capture_consents state) operation); try discriminate.
  inversion H; subst. simpl. unfold replace_at. now rewrite Nat.eqb_refl.
Qed.

Theorem successful_settlement_replays_selected_receipt : forall state operation branch expected actual next,
  settle_phlo state operation branch expected = Some (actual, next) ->
  replay_phlo_receipt next operation branch = Some actual.
Proof.
  intros state operation branch expected actual next ran.
  pose proof (successful_settlement_records_selected_branch _ _ _ _ _ _ ran) as selected.
  pose proof (successful_settlement_sets_closed_flag _ _ _ _ _ _ ran) as closed.
  pose proof (settlement_returns_only_the_captured_receipt _ _ _ _ _ _ ran) as [prepared [_ snapshots]].
  unfold replay_phlo_receipt. rewrite selected, closed, Nat.eqb_refl. simpl.
  unfold settle_phlo in ran.
  destruct (prepare_phlo_receipt state operation branch) eqn:preparation; try discriminate.
  destruct (phlo_receipt_eq_dec p expected); try discriminate.
  unfold settle_reservation in ran.
  destruct (settlement_evidence (capture_consents state) operation); try discriminate.
  inversion ran; subst. unfold reconstruct_phlo_candidate. simpl.
  apply prepared_receipt_matches_candidate_reconstruction. exact preparation.
Qed.

Theorem historical_replay_rejects_other_outcomes : forall state operation selected branch,
  settled_phlo_branches state operation = Some selected -> branch <> selected ->
  replay_phlo_receipt state operation branch = None.
Proof.
  intros. unfold replay_phlo_receipt. rewrite H. apply Nat.eqb_neq in H0. rewrite H0.
  destruct (settled (capture_consents state) operation); reflexivity.
Qed.

Theorem arbitrary_history_preserves_settled_replay : forall decode first last operation branch captured,
  phlo_capture_valid decode first -> phlo_capture_history decode first last ->
  accepted_snapshots first operation = Some captured -> settled (capture_consents first) operation = true ->
  replay_phlo_receipt last operation branch = replay_phlo_receipt first operation branch.
Proof.
  intros decode first last operation branch captured valid history found closed.
  unfold replay_phlo_receipt.
  rewrite (arbitrary_history_preserves_closed_selection _ _ _ _ history closed).
  rewrite (arbitrary_capture_history_preserves_closure _ _ _ _ history closed), closed.
  destruct (settled_phlo_branches first operation); [|reflexivity].
  destruct (branch =? n); [|reflexivity]. simpl.
  eapply arbitrary_history_preserves_candidate_reconstruction; eauto.
Qed.

Theorem captured_funding_respects_decoded_source_caps : forall decode state operation captured source branch,
  phlo_capture_valid decode state -> accepted_snapshots state operation = Some captured ->
  source < funding_sources (snapshot_domain (snapshot_value captured)) ->
  In branch (snapshot_branches (snapshot_value captured)) ->
  let snapshot := snapshot_value captured in
  let domain := snapshot_domain snapshot in
  exists consent,
    intent_source_consent (snapshot_intent captured) (funding_custody domain source) = Some consent /\
    phlo_source_holds domain (snapshot_branches snapshot) (snapshot_plans snapshot) source <= consent_hold_cap consent /\
    source_draw (funding_obligations domain) (snapshot_plans snapshot branch) source <= consent_debit_cap consent.
Proof.
  intros decode state operation captured source branch valid found inside included. simpl.
  specialize (valid operation). rewrite found in valid.
  destruct (reservations (capture_consents state) operation) as [record|]; try contradiction.
  destruct valid as [same [checked [decoded [authorized price]]]].
  pose proof (decoded_intent_bounds_source_consent _ _ _ _ authorized inside)
    as [consent [consent_found [hold_cap debit_cap]]].
  unfold check_phlo_snapshot in checked.
  pose proof (accepted_family_bounds_each_hold _ _ _ _ _ _ _ _ _ _ _ checked inside) as [_ hold].
  pose proof (accepted_family_bounds_each_debit _ _ _ _ _ _ _ _ _ _ _ _ checked included inside) as [_ debit].
  exists consent. split; [exact consent_found|]. split; eapply Nat.le_trans; eauto.
Qed.

Theorem checked_capture_binds_reservation_price : forall decode state operation captured,
  phlo_capture_valid decode state -> accepted_snapshots state operation = Some captured ->
  reservation_price (snapshot_reservation captured) =
    phlo_actual_price (snapshot_schedule (snapshot_value captured)).
Proof.
  intros. specialize (H operation). rewrite H0 in H.
  destruct (reservations (capture_consents state) operation); try contradiction.
  destruct H as [same [_ [_ [_ price]]]]. now rewrite same.
Qed.

Theorem arbitrary_capture_history_binds_actual_schedule : forall decode first last operation captured,
  phlo_capture_history decode first last -> phlo_capture_valid decode first ->
  accepted_snapshots last operation = Some captured ->
  schedule_identity (reservation_terms (snapshot_reservation captured)) =
    phlo_schedule_commitment (snapshot_schedule (snapshot_value captured)).
Proof.
  intros decode first last operation captured history valid found.
  pose proof (arbitrary_capture_history_preserves_checked_pairing _ _ _ history valid) as closed.
  specialize (closed operation). rewrite found in closed.
  destruct (reservations (capture_consents last) operation) as [record|]; [|contradiction].
  destruct closed as [same [_ [_ [authorized _]]]].
  apply decoded_intent_binds_actual_schedule in authorized.
  destruct authorized as [declared actual]. rewrite same. congruence.
Qed.

Print Assumptions arbitrary_capture_history_binds_actual_schedule.

Definition phlo_example_initial_terms : funding_terms :=
  {| required_owner_consents := [(10, 3); (11, 3)]; signed_limit_terms := [true];
     asset_identity := 1; schedule_identity := 1 |}.

Definition phlo_example_replacement_terms : funding_terms :=
  {| required_owner_consents := [(20, 1); (21, 1)]; signed_limit_terms := [false];
     asset_identity := 1; schedule_identity := 8 |}.

Definition phlo_example_intent : phlo_intent :=
  {| intent_controls := example_phlo_terms 2; intent_schedule_identity := 1; intent_total_exposure := 5;
     intent_source_consent := fun custody => if custody <? 2 then
       Some {| consent_hold_cap := 3; consent_debit_cap := 3; consent_resource_permission := fun _ => true |}
       else None |}.

Definition phlo_example_replacement_schedule : phlo_schedule :=
  {| phlo_schedule_commitment := 8; phlo_protocol_version := 1; phlo_network := 1; phlo_shard := 1;
     phlo_asset := 1; phlo_unit := 1; phlo_decimal_scale := 8; phlo_weights := [1; 2; 3; 4];
     phlo_actual_price := 1 |}.

Definition phlo_example_replacement_intent : phlo_intent :=
  {| intent_controls := {| signed_phlo_limit := 1; signed_phlo_price := 1;
                          signed_required_ceilings := [1; 1]; signed_phlo_schedules := [phlo_example_replacement_schedule] |};
     intent_schedule_identity := 8; intent_total_exposure := 2;
     intent_source_consent := fun custody => if custody =? 20 then
       Some {| consent_hold_cap := 2; consent_debit_cap := 2; consent_resource_permission := fun _ => true |}
       else None |}.

Definition phlo_example_decoder bits : option phlo_intent :=
  match bits with
  | [true] => Some phlo_example_intent
  | [false] => Some phlo_example_replacement_intent
  | _ => None
  end.

Definition phlo_example_snapshot : phlo_snapshot :=
  {| snapshot_environment := example_phlo_environment; snapshot_machine_max := 31;
     snapshot_controls := example_phlo_terms 2; snapshot_schedule := example_phlo_schedule 2;
     snapshot_bound := 3; snapshot_domain := example_resource_and_fee_domain;
     snapshot_branches := [0; 1]; snapshot_cases := example_partially_prepaid_case;
     snapshot_demands := example_resource_and_fee_demands; snapshot_plans := example_resource_and_fee_plans |}.

Definition phlo_example_start := empty_phlo_capture_state (fun key => if key =? 0 then
  Some {| right_generation := 0; right_terms := phlo_example_initial_terms |} else None).

Definition phlo_example_transfer :=
  {| transfer_key := 0; transfer_expected := 0; transfer_terms := phlo_example_replacement_terms;
     transfer_authorized := true |}.

Example capture_transfer_settlement_and_duplicate_rejection :
  match capture_phlo phlo_example_decoder phlo_example_start 0 0 9 99 phlo_example_snapshot with
  | None => False
  | Some captured =>
      match transfer_phlo_history captured [phlo_example_transfer] with
      | None => False
      | Some transferred =>
          match prepare_phlo_receipt transferred 9 0 with
          | None => False
          | Some receipt =>
              receipt_schedule receipt = example_phlo_schedule 2 /\
              receipt_debits receipt = [(0, 3); (1, 2)] /\
              settle_phlo transferred 9 0 (receipt_with_schedule receipt (example_phlo_schedule 1)) = None /\
              match settle_phlo transferred 9 0 receipt with
              | None => False
              | Some (_, closed) => settle_phlo closed 9 0 receipt = None /\
                  capture_phlo phlo_example_decoder closed 0 1 9 100 phlo_example_snapshot = None /\
                  replay_phlo_receipt closed 9 0 = Some receipt
              end
          end
      end
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example transferred_owners_cannot_redirect_earlier_refunds :
  match capture_phlo phlo_example_decoder phlo_example_start 0 0 9 99 phlo_example_snapshot with
  | None => False
  | Some captured =>
      match transfer_phlo_history captured [phlo_example_transfer] with
      | None => False
      | Some transferred =>
          match prepare_phlo_receipt transferred 9 1 with
          | None => False
          | Some receipt => receipt_debits receipt = [(0, 0); (1, 0)] /\
              receipt_refunds receipt = [(0, 3); (1, 2)]
          end
      end
  end.
Proof. vm_compute. split; reflexivity. Qed.

Example decoded_new_terms_do_not_authorize_an_old_plan :
  check_decoded_intent phlo_example_replacement_terms phlo_example_replacement_intent phlo_example_snapshot = false /\
  capture_phlo (fun _ => None) phlo_example_start 0 0 9 99 phlo_example_snapshot = None.
Proof. vm_compute. split; reflexivity. Qed.

Example settled_replay_cannot_select_another_outcome :
  match capture_phlo phlo_example_decoder phlo_example_start 0 0 9 99 phlo_example_snapshot with
  | None => False
  | Some captured =>
      match prepare_phlo_receipt captured 9 0 with
      | None => False
      | Some receipt =>
          match settle_phlo captured 9 0 receipt with
          | None => False
          | Some (_, closed) => replay_phlo_receipt closed 9 1 = None
          end
      end
  end.
Proof. vm_compute. reflexivity. Qed.

Example mismatched_execution_schedule_rejects_decoded_intent :
  let mismatched_terms := {| required_owner_consents := [(10, 3); (11, 3)]; signed_limit_terms := [true];
                            asset_identity := 1; schedule_identity := 7 |} in
  let mismatched_intent := {| intent_controls := example_phlo_terms 2; intent_schedule_identity := 7;
                             intent_total_exposure := 5;
                             intent_source_consent := intent_source_consent phlo_example_intent |} in
  check_decoded_intent mismatched_terms mismatched_intent phlo_example_snapshot = false.
Proof. vm_compute. reflexivity. Qed.

Definition check_offered_phlo_family_intent terms intent snapshot minimum offer :=
  check_decoded_intent terms intent snapshot && check_phlo_snapshot snapshot &&
  admit_offered_phlo_controls (snapshot_environment snapshot) minimum
    (snapshot_machine_max snapshot) offer (snapshot_controls snapshot)
    (snapshot_schedule snapshot) (snapshot_bound snapshot).

Theorem offered_family_preserves_all_checks : forall terms intent snapshot minimum offer,
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true <->
  check_decoded_intent terms intent snapshot = true /\
  check_phlo_snapshot snapshot = true /\
  admit_offered_phlo_controls (snapshot_environment snapshot) minimum
    (snapshot_machine_max snapshot) offer (snapshot_controls snapshot)
    (snapshot_schedule snapshot) (snapshot_bound snapshot) = true.
Proof. intros. unfold check_offered_phlo_family_intent. rewrite !andb_true_iff. tauto. Qed.

Theorem offered_family_preserves_chain_and_owners : forall terms intent snapshot minimum offer,
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true ->
  minimum <= offered_phlo_price offer /\
  Forall (fun ceiling => offered_phlo_price offer <= ceiling) (required_ceilings terms).
Proof.
  intros terms intent snapshot minimum offer checked.
  apply offered_family_preserves_all_checks in checked.
  destruct checked as [decoded [_ offered]].
  apply decoded_intent_binds_controls in decoded. destruct decoded as [controls owners].
  apply offered_price_respects_chain_and_all_owners in offered.
  rewrite owners, <- controls. exact offered.
Qed.

Theorem offered_family_bounds_usage_and_prices_charge : forall terms intent snapshot minimum offer,
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true ->
  snapshot_bound snapshot <= offered_phlo_limit offer /\
  schedule_charge_bound (snapshot_schedule snapshot) (snapshot_bound snapshot) =
    snapshot_bound snapshot * offered_phlo_price offer + 1.
Proof.
  intros terms intent snapshot minimum offer checked.
  apply offered_family_preserves_all_checks in checked. destruct checked as [_ [_ offered]].
  split; [eapply offered_limit_bounds_resources | eapply offered_price_charge_uses_offer_not_ceiling];
    eassumption.
Qed.

Theorem offered_family_cannot_rebind_offer : forall terms intent snapshot minimum first second,
  check_offered_phlo_family_intent terms intent snapshot minimum first = true ->
  check_offered_phlo_family_intent terms intent snapshot minimum second = true -> first = second.
Proof.
  intros terms intent snapshot minimum first second a b.
  apply offered_family_preserves_all_checks in a. destruct a as [_ [_ a]].
  apply offered_family_preserves_all_checks in b. destruct b as [_ [_ b]].
  apply offered_phlo_controls_exact in a. apply offered_phlo_controls_exact in b.
  destruct a as [al [ap _]], b as [bl [bp _]].
  destruct first, second. simpl in *. f_equal; congruence.
Qed.

Theorem offered_family_preserves_source_caps : forall terms intent snapshot minimum offer source,
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true ->
  source < funding_sources (snapshot_domain snapshot) ->
  exists consent,
    intent_source_consent intent (funding_custody (snapshot_domain snapshot) source) = Some consent /\
    signed_source_exposure (snapshot_domain snapshot) source <= consent_hold_cap consent /\
    signed_source_debit (snapshot_domain snapshot) source <= consent_debit_cap consent.
Proof.
  intros terms intent snapshot minimum offer source checked inside.
  apply offered_family_preserves_all_checks in checked. destruct checked as [decoded _].
  eapply decoded_intent_bounds_source_consent; eassumption.
Qed.

Theorem offered_family_preserves_permissions : forall terms intent snapshot minimum offer source branch key,
  check_offered_phlo_family_intent terms intent snapshot minimum offer = true ->
  source < funding_sources (snapshot_domain snapshot) ->
  In branch (snapshot_branches snapshot) -> In key (case_obligation_keys (snapshot_cases snapshot branch)) ->
  funding_permission (snapshot_domain snapshot) branch source key = true ->
  exists consent, intent_source_consent intent (funding_custody (snapshot_domain snapshot) source) = Some consent /\
    consent_resource_permission consent key = true.
Proof.
  intros terms intent snapshot minimum offer source branch key checked inside present obligation allowed.
  apply offered_family_preserves_all_checks in checked. destruct checked as [decoded _].
  eapply decoded_permission_cannot_be_expanded; eassumption.
Qed.

Print Assumptions offered_family_preserves_all_checks.
Print Assumptions offered_family_preserves_chain_and_owners.
Print Assumptions offered_family_bounds_usage_and_prices_charge.
Print Assumptions offered_family_cannot_rebind_offer.
Print Assumptions offered_family_preserves_source_caps.
Print Assumptions offered_family_preserves_permissions.
