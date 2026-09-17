From Stdlib Require Import Arith.PeanoNat Lists.List Lia NArith ZArith Bool.Bool.
From CostAccountedRho Require Import SignedPhloWire SignedPhloSchedule SignedPhloSource SignedPhloControlsWire.
Import ListNotations.

Record phlo_funding_intent_record := {
  funding_intent_controls : phlo_controls_record;
  funding_intent_schedule : list nat;
  funding_intent_exposure : nat;
  funding_intent_sources : list phlo_source_record
}.

Definition phlo_funding_intent_fields domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width record :=
  [domain;
   phlo_controls_record_bytes controls_domain schedule_domain width word_width count_width scale_width
     (funding_intent_controls record);
   funding_intent_schedule record;
   phlo_word exposure_width (funding_intent_exposure record);
   phlo_record_list_bytes width count_width
     (phlo_source_bytes source_domain resource_domain width word_width count_width) (funding_intent_sources record)].

Definition phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width record :=
  phlo_fields width (phlo_funding_intent_fields domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width record).

Definition phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum record :=
  phlo_controls_record_fits controls_domain schedule_domain width word_width count_width scale_width maximum
    (funding_intent_controls record) /\
  funding_intent_exposure record < 256 ^ exposure_width /\
  Forall (phlo_source_fits source_domain resource_domain width word_width count_width maximum)
    (funding_intent_sources record) /\
  phlo_fields_fit width maximum
    (phlo_record_list_fields count_width
      (phlo_source_bytes source_domain resource_domain width word_width count_width) (funding_intent_sources record)) /\
  phlo_fields_fit width maximum
    (phlo_funding_intent_fields domain controls_domain schedule_domain source_domain resource_domain
      width word_width count_width scale_width exposure_width record).

Theorem phlo_funding_intent_encoding_binds_all_terms :
  forall domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second,
  0 < width ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum second ->
  phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width first =
  phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width second -> first = second.
Proof.
  intros domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second positive
    [first_controls [first_exposure [first_sources [first_source_fields first_fields]]]]
    [second_controls [second_exposure [second_sources [second_source_fields second_fields]]]] same.
  unfold phlo_funding_intent_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first as [controls schedule exposure sources].
  destruct second as [other_controls other_schedule other_exposure other_sources].
  cbn [phlo_funding_intent_fields funding_intent_controls funding_intent_schedule
    funding_intent_exposure funding_intent_sources] in *.
  injection same as control_records schedules exposures source_records.
  apply phlo_word_encoding_is_injective in exposures; auto.
  assert (controls = other_controls) by (eapply phlo_controls_wire_binds_every_term; eauto).
  assert (sources = other_sources).
  { eapply phlo_record_list_encoding_preserves_all_entries with
      (valid := phlo_source_fits source_domain resource_domain width word_width count_width maximum); eauto.
    intros. eapply phlo_source_encoding_binds_every_consent_field; eauto. }
  subst. reflexivity.
Qed.

Theorem phlo_funding_intent_domain_is_bound :
  forall domain other_domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second,
  0 < width ->
  phlo_fields_fit width maximum
    (phlo_funding_intent_fields domain controls_domain schedule_domain source_domain resource_domain
      width word_width count_width scale_width exposure_width first) ->
  phlo_fields_fit width maximum
    (phlo_funding_intent_fields other_domain controls_domain schedule_domain source_domain resource_domain
      width word_width count_width scale_width exposure_width second) ->
  phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width first =
  phlo_funding_intent_bytes other_domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width second -> domain = other_domain.
Proof.
  intros domain other_domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second positive
    first_fields second_fields same.
  unfold phlo_funding_intent_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  exact (f_equal (fun fields => hd [] fields) same).
Qed.

Print Assumptions phlo_funding_intent_encoding_binds_all_terms.
Print Assumptions phlo_funding_intent_domain_is_bound.

Definition funded_deploy_payload width domain version body funding : list nat :=
  [0; 2] ++ phlo_fields width [domain; version; body; funding].

Theorem funded_deploy_payload_binds_all_fields :
  forall width maximum domain version body funding other_domain other_version other_body other_funding,
  0 < width ->
  phlo_fields_fit width maximum [domain; version; body; funding] ->
  phlo_fields_fit width maximum [other_domain; other_version; other_body; other_funding] ->
  funded_deploy_payload width domain version body funding =
  funded_deploy_payload width other_domain other_version other_body other_funding ->
  domain = other_domain /\ version = other_version /\ body = other_body /\ funding = other_funding.
Proof.
  intros width maximum domain version body funding other_domain other_version other_body other_funding
    positive fits other_fits same.
  unfold funded_deploy_payload in same.
  injection same as fields.
  apply (phlo_field_sequences_cannot_alias width maximum) in fields; auto.
  injection fields as domains versions bodies fundings.
  auto.
Qed.

Theorem funded_deploy_payload_cannot_alias_v61 :
  forall width domain version body funding legacy,
  funded_deploy_payload width domain version body funding <> [0; 1] ++ legacy.
Proof. intros. unfold funded_deploy_payload. discriminate. Qed.

Theorem funded_deploy_changed_funding_changes_payload :
  forall width maximum domain version body first second,
  0 < width ->
  phlo_fields_fit width maximum [domain; version; body; first] ->
  phlo_fields_fit width maximum [domain; version; body; second] ->
  first <> second ->
  funded_deploy_payload width domain version body first <>
  funded_deploy_payload width domain version body second.
Proof.
  intros width maximum domain version body first second positive fits other_fits different same.
  apply different.
  eapply funded_deploy_payload_binds_all_fields in same; eauto.
  tauto.
Qed.

Print Assumptions funded_deploy_payload_binds_all_fields.
Print Assumptions funded_deploy_payload_cannot_alias_v61.
Print Assumptions funded_deploy_changed_funding_changes_payload.

Theorem funded_deploy_payload_binds_decoded_record :
  forall envelope_domain version body other_body
    domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second,
  0 < width ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum second ->
  let encode := phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width in
  phlo_fields_fit width maximum [envelope_domain; version; body; encode first] ->
  phlo_fields_fit width maximum [envelope_domain; version; other_body; encode second] ->
  funded_deploy_payload width envelope_domain version body (encode first) =
  funded_deploy_payload width envelope_domain version other_body (encode second) ->
  body = other_body /\ first = second.
Proof.
  intros envelope_domain version body other_body
    domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second
    positive first_fits second_fits encode first_fields second_fields same.
  apply (funded_deploy_payload_binds_all_fields width maximum) in same; auto.
  destruct same as [_ [_ [bodies records]]].
  split; auto.
  unfold encode in records.
  eapply phlo_funding_intent_encoding_binds_all_terms; eauto.
Qed.

Print Assumptions funded_deploy_payload_binds_decoded_record.

Definition offered_funded_deploy_fields word_width domain version body funding limit price :=
  [domain; version; body; funding; phlo_word word_width limit; phlo_word word_width price].

Definition offered_funded_deploy_payload width word_width domain version body funding limit price :=
  [0; 3] ++ phlo_fields width
    (offered_funded_deploy_fields word_width domain version body funding limit price).

Theorem offered_funded_payload_binds_all_fields :
  forall width word_width maximum domain version body funding limit price
    other_domain other_version other_body other_funding other_limit other_price,
  0 < width ->
  limit < 256 ^ word_width -> price < 256 ^ word_width ->
  other_limit < 256 ^ word_width -> other_price < 256 ^ word_width ->
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width domain version body funding limit price) ->
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width other_domain other_version other_body other_funding other_limit other_price) ->
  offered_funded_deploy_payload width word_width domain version body funding limit price =
  offered_funded_deploy_payload width word_width other_domain other_version other_body other_funding other_limit other_price ->
  domain = other_domain /\ version = other_version /\ body = other_body /\
  funding = other_funding /\ limit = other_limit /\ price = other_price.
Proof.
  intros width word_width maximum domain version body funding limit price
    other_domain other_version other_body other_funding other_limit other_price
    positive limit_fits price_fits other_limit_fits other_price_fits fits other_fits same.
  unfold offered_funded_deploy_payload in same.
  injection same as fields.
  apply (phlo_field_sequences_cannot_alias width maximum) in fields; auto.
  unfold offered_funded_deploy_fields in fields.
  injection fields as domains versions bodies fundings limits prices.
  apply phlo_word_encoding_is_injective in limits; auto.
  apply phlo_word_encoding_is_injective in prices; auto.
  repeat split; assumption.
Qed.

Theorem offered_funded_payload_cannot_alias_v61 :
  forall width word_width domain version body funding limit price legacy,
  offered_funded_deploy_payload width word_width domain version body funding limit price <>
    [0; 1] ++ legacy.
Proof. intros. unfold offered_funded_deploy_payload. discriminate. Qed.

Theorem offered_funded_payload_cannot_alias_funded_v1 :
  forall width word_width domain version body funding limit price
    old_width old_domain old_version old_body old_funding,
  offered_funded_deploy_payload width word_width domain version body funding limit price <>
    funded_deploy_payload old_width old_domain old_version old_body old_funding.
Proof. intros. unfold offered_funded_deploy_payload, funded_deploy_payload. discriminate. Qed.

Theorem offered_funded_changed_offer_changes_payload :
  forall width word_width maximum domain version body funding limit price other_limit other_price,
  0 < width ->
  limit < 256 ^ word_width -> price < 256 ^ word_width ->
  other_limit < 256 ^ word_width -> other_price < 256 ^ word_width ->
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width domain version body funding limit price) ->
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width domain version body funding other_limit other_price) ->
  (limit <> other_limit \/ price <> other_price) ->
  offered_funded_deploy_payload width word_width domain version body funding limit price <>
    offered_funded_deploy_payload width word_width domain version body funding other_limit other_price.
Proof.
  intros width word_width maximum domain version body funding limit price other_limit other_price
    positive limit_fits price_fits other_limit_fits other_price_fits fits other_fits different same.
  eapply offered_funded_payload_binds_all_fields in same; eauto.
  tauto.
Qed.

Print Assumptions offered_funded_payload_binds_all_fields.
Print Assumptions offered_funded_payload_cannot_alias_v61.
Print Assumptions offered_funded_payload_cannot_alias_funded_v1.
Print Assumptions offered_funded_changed_offer_changes_payload.

Theorem offered_funded_payload_binds_decoded_record :
  forall envelope_domain version body other_body limit price other_limit other_price
    domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second,
  0 < width ->
  limit < 256 ^ word_width -> price < 256 ^ word_width ->
  other_limit < 256 ^ word_width -> other_price < 256 ^ word_width ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first ->
  phlo_funding_intent_fits domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum second ->
  let encode := phlo_funding_intent_bytes domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width in
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width envelope_domain version body (encode first) limit price) ->
  phlo_fields_fit width maximum
    (offered_funded_deploy_fields word_width envelope_domain version other_body (encode second) other_limit other_price) ->
  offered_funded_deploy_payload width word_width envelope_domain version body (encode first) limit price =
  offered_funded_deploy_payload width word_width envelope_domain version other_body (encode second) other_limit other_price ->
  body = other_body /\ first = second /\ limit = other_limit /\ price = other_price.
Proof.
  intros envelope_domain version body other_body limit price other_limit other_price
    domain controls_domain schedule_domain source_domain resource_domain
    width word_width count_width scale_width exposure_width maximum first second
    positive limit_fits price_fits other_limit_fits other_price_fits first_fits second_fits
    encode first_fields second_fields same.
  apply (offered_funded_payload_binds_all_fields width word_width maximum) in same; auto.
  destruct same as [_ [_ [bodies [records [limits prices]]]]].
  split; [exact bodies|].
  split; [|auto].
  unfold encode in records.
  eapply phlo_funding_intent_encoding_binds_all_terms; eauto.
Qed.

Print Assumptions offered_funded_payload_binds_decoded_record.

Inductive deploy_envelope_format := LegacyPayload | BodyEnvelope | FundedEnvelope | OfferedEnvelope.

Definition envelope_format_code format : option N :=
  match format with
  | LegacyPayload => None
  | BodyEnvelope => Some 393217%N
  | FundedEnvelope => Some 393218%N
  | OfferedEnvelope => Some 393219%N
  end.

Definition decode_envelope_format code :=
  match code with
  | None => Some LegacyPayload
  | Some value =>
      if N.eqb value 393217%N then Some BodyEnvelope else
      if N.eqb value 393218%N then Some FundedEnvelope else
      if N.eqb value 393219%N then Some OfferedEnvelope else None
  end.

Theorem envelope_format_dispatch_exact : forall code format,
  decode_envelope_format code = Some format <-> code = envelope_format_code format.
Proof.
  intros [code|] format; [|destruct format; simpl; intuition discriminate].
  unfold decode_envelope_format.
  destruct (N.eqb code 393217%N) eqn:first.
  - apply N.eqb_eq in first. subst. destruct format; simpl; intuition discriminate.
  - apply N.eqb_neq in first. destruct (N.eqb code 393218%N) eqn:second.
    + apply N.eqb_eq in second. subst. destruct format; simpl; intuition discriminate.
    + apply N.eqb_neq in second. destruct (N.eqb code 393219%N) eqn:third.
      * apply N.eqb_eq in third. subst. destruct format; simpl; intuition discriminate.
      * apply N.eqb_neq in third. destruct format; simpl; split; intros impossible;
          try discriminate; inversion impossible; contradiction.
Qed.

Record retained_deploy_envelope := {
  retained_format : deploy_envelope_format;
  retained_body : list nat;
  retained_funding : option (list nat);
  retained_offer : option (nat * nat);
  retained_authorization : list nat;
  retained_primary_index : nat;
  retained_legacy_order : list nat;
  retained_legacy_algorithms : list nat;
  retained_canonical_algorithms : list nat;
  retained_legacy_wire_threshold : Z;
  retained_threshold : nat;
  retained_identity : list nat
}.

Record stored_deploy_envelope := {
  stored_schema : N;
  stored_format : option N;
  stored_wire : list nat
}.

Definition store_deploy_envelope encode envelope :=
  {| stored_schema := 1%N; stored_format := envelope_format_code (retained_format envelope);
     stored_wire := encode envelope |}.

Definition envelope_code_eq_dec : forall (first second : option N), {first = second} + {first <> second}.
Proof. decide equality; apply N.eq_dec. Defined.

Definition restore_deploy_envelope decode expected_identity stored :=
  if N.eqb (stored_schema stored) 1%N then
    match decode (stored_wire stored) with
    | Some envelope =>
        if envelope_code_eq_dec (stored_format stored) (envelope_format_code (retained_format envelope)) then
          if list_eq_dec Nat.eq_dec expected_identity (retained_identity envelope) then Some envelope else None
        else None
    | None => None
    end
  else None.

Theorem stored_envelope_requires_schema_format_identity_and_decode :
  forall decode expected stored envelope,
  restore_deploy_envelope decode expected stored = Some envelope ->
  stored_schema stored = 1%N /\
  decode (stored_wire stored) = Some envelope /\
  stored_format stored = envelope_format_code (retained_format envelope) /\
  expected = retained_identity envelope.
Proof.
  intros decode expected stored envelope checked. unfold restore_deploy_envelope in checked.
  destruct (N.eqb (stored_schema stored) 1%N) eqn:schema; [|discriminate].
  apply N.eqb_eq in schema.
  destruct (decode (stored_wire stored)) as [decoded|] eqn:valid; [|discriminate].
  destruct (envelope_code_eq_dec (stored_format stored) (envelope_format_code (retained_format decoded))) as [format|]; [|discriminate].
  destruct (list_eq_dec Nat.eq_dec expected (retained_identity decoded)) as [identity|]; [|discriminate].
  inversion checked; subst. auto.
Qed.

Theorem stored_envelope_roundtrip_retains_complete_authorization : forall encode decode envelope,
  decode (encode envelope) = Some envelope ->
  restore_deploy_envelope decode (retained_identity envelope) (store_deploy_envelope encode envelope) = Some envelope.
Proof.
  intros encode decode envelope roundtrip. unfold restore_deploy_envelope, store_deploy_envelope. simpl.
  rewrite roundtrip.
  destruct (envelope_code_eq_dec (envelope_format_code (retained_format envelope))
    (envelope_format_code (retained_format envelope))); [|contradiction].
  destruct (list_eq_dec Nat.eq_dec (retained_identity envelope) (retained_identity envelope)); congruence.
Qed.

Theorem stored_envelope_rejects_other_schema : forall decode expected stored,
  stored_schema stored <> 1%N -> restore_deploy_envelope decode expected stored = None.
Proof.
  intros decode expected stored different. unfold restore_deploy_envelope.
  destruct (N.eqb (stored_schema stored) 1%N) eqn:schema; [|reflexivity].
  apply N.eqb_eq in schema. contradiction.
Qed.

Theorem stored_envelope_rejects_failed_decode : forall decode expected stored,
  decode (stored_wire stored) = None -> restore_deploy_envelope decode expected stored = None.
Proof.
  intros decode expected stored failed. unfold restore_deploy_envelope.
  destruct (N.eqb (stored_schema stored) 1%N); [now rewrite failed|reflexivity].
Qed.

Definition admit_restored_envelope (permitted : deploy_envelope_format -> bool) (decoded : option retained_deploy_envelope) :=
  match decoded with
  | Some envelope => if permitted (retained_format envelope) then Some envelope else None
  | None => None
  end.

Theorem decoded_format_does_not_imply_admission : forall permitted envelope,
  permitted (retained_format envelope) = false ->
  admit_restored_envelope permitted (Some envelope) = None.
Proof. intros permitted envelope disabled. unfold admit_restored_envelope. now rewrite disabled. Qed.

Print Assumptions envelope_format_dispatch_exact.
Print Assumptions stored_envelope_requires_schema_format_identity_and_decode.
Print Assumptions stored_envelope_roundtrip_retains_complete_authorization.
Print Assumptions stored_envelope_rejects_other_schema.
Print Assumptions stored_envelope_rejects_failed_decode.
Print Assumptions decoded_format_does_not_imply_admission.

Definition stored_lookup_key_size (legacy : bool) identity_size :=
  if legacy then 12 + identity_size else 36.

Definition stored_lookup_key_fits legacy identity_size row_limit :=
  Nat.leb (stored_lookup_key_size legacy identity_size) row_limit.

Theorem stored_lookup_key_limit_exact : forall legacy identity_size row_limit,
  stored_lookup_key_fits legacy identity_size row_limit = true <->
  (if legacy then 12 + identity_size else 36) <= row_limit.
Proof.
  intros. unfold stored_lookup_key_fits, stored_lookup_key_size.
  apply Nat.leb_le.
Qed.

Theorem legacy_lookup_payload_bound : forall identity_size row_limit,
  stored_lookup_key_fits true identity_size row_limit = true ->
  identity_size <= row_limit /\ 12 <= row_limit.
Proof.
  intros identity_size row_limit bounded.
  apply stored_lookup_key_limit_exact in bounded. simpl in bounded. lia.
Qed.

Print Assumptions stored_lookup_key_limit_exact.
Print Assumptions legacy_lookup_payload_bound.

Record retained_pending_deploy := {
  pending_envelope : retained_deploy_envelope;
  pending_identity : list nat
}.

Definition retain_pending_deploy envelope :=
  {| pending_envelope := envelope; pending_identity := retained_identity envelope |}.

Definition restore_pending_deploy decode expected stored :=
  option_map retain_pending_deploy (restore_deploy_envelope decode expected stored).

Definition legacy_metadata_projectable envelope :=
  if list_eq_dec Nat.eq_dec (retained_legacy_algorithms envelope)
    (retained_canonical_algorithms envelope)
  then Z.eqb (retained_legacy_wire_threshold envelope) (Z.of_nat (retained_threshold envelope))
  else false.

Definition pending_body_adapter pending :=
  match retained_format (pending_envelope pending) with
  | LegacyPayload =>
      if Nat.eqb (retained_primary_index (pending_envelope pending)) 0
      then if list_eq_dec Nat.eq_dec
        (retained_legacy_order (pending_envelope pending))
        (seq 0 (length (retained_legacy_order (pending_envelope pending))))
        then if legacy_metadata_projectable (pending_envelope pending)
          then Some (pending_envelope pending) else None
        else None
      else None
  | BodyEnvelope => Some (pending_envelope pending)
  | FundedEnvelope | OfferedEnvelope => None
  end.

Theorem pending_retains_complete_envelope : forall envelope,
  pending_envelope (retain_pending_deploy envelope) = envelope /\
  pending_identity (retain_pending_deploy envelope) = retained_identity envelope.
Proof. intros. split; reflexivity. Qed.

Theorem pending_restore_preserves_authorization : forall encode decode envelope,
  decode (encode envelope) = Some envelope ->
  restore_pending_deploy decode (retained_identity envelope) (store_deploy_envelope encode envelope) =
    Some (retain_pending_deploy envelope).
Proof.
  intros encode decode envelope roundtrip. unfold restore_pending_deploy.
  rewrite (stored_envelope_roundtrip_retains_complete_authorization encode decode envelope roundtrip).
  reflexivity.
Qed.

Theorem pending_body_adapter_preserves_envelope : forall pending envelope,
  pending_body_adapter pending = Some envelope ->
  envelope = pending_envelope pending /\
  (retained_format envelope = LegacyPayload \/ retained_format envelope = BodyEnvelope).
Proof.
  intros pending envelope accepted. unfold pending_body_adapter in accepted.
  destruct (retained_format (pending_envelope pending)) eqn:format.
  - destruct (Nat.eqb (retained_primary_index (pending_envelope pending)) 0);
      try discriminate.
    destruct (list_eq_dec Nat.eq_dec
      (retained_legacy_order (pending_envelope pending))
      (seq 0 (length (retained_legacy_order (pending_envelope pending))))); try discriminate.
    destruct (legacy_metadata_projectable (pending_envelope pending));
      inversion accepted; subst; auto.
  - inversion accepted; subst; auto.
  - discriminate.
  - discriminate.
Qed.

Theorem pending_body_adapter_rejects_funded : forall pending,
  (retained_format (pending_envelope pending) = FundedEnvelope \/
   retained_format (pending_envelope pending) = OfferedEnvelope) ->
  pending_body_adapter pending = None.
Proof.
  intros pending [funded|offered]; unfold pending_body_adapter;
    [rewrite funded|rewrite offered]; reflexivity.
Qed.

Print Assumptions pending_retains_complete_envelope.
Print Assumptions pending_restore_preserves_authorization.
Print Assumptions pending_body_adapter_preserves_envelope.
Print Assumptions pending_body_adapter_rejects_funded.

Theorem pending_body_adapter_preserves_legacy_primary : forall pending envelope,
  pending_body_adapter pending = Some envelope ->
  retained_format envelope = LegacyPayload -> retained_primary_index envelope = 0.
Proof.
  intros pending envelope accepted legacy.
  pose proof (pending_body_adapter_preserves_envelope pending envelope accepted) as [same _].
  subst envelope. unfold pending_body_adapter in accepted. rewrite legacy in accepted.
  destruct (Nat.eqb (retained_primary_index (pending_envelope pending)) 0) eqn:primary;
    [now apply Nat.eqb_eq|discriminate].
Qed.

Print Assumptions pending_body_adapter_preserves_legacy_primary.

Record retained_processed_deploy := {
  processed_envelope : retained_deploy_envelope;
  processed_receipt : list nat
}.

Definition process_retained_envelope envelope receipt :=
  {| processed_envelope := envelope; processed_receipt := receipt |}.

Definition replace_processed_receipt processed receipt :=
  process_retained_envelope (processed_envelope processed) receipt.

Fixpoint apply_receipt_updates processed receipts :=
  match receipts with
  | [] => processed
  | receipt :: rest => apply_receipt_updates (replace_processed_receipt processed receipt) rest
  end.

Definition stored_processed_deploy encode processed :=
  (store_deploy_envelope encode (processed_envelope processed), processed_receipt processed).

Definition restore_processed_deploy decode expected stored :=
  option_map (fun envelope => process_retained_envelope envelope (snd stored))
    (restore_deploy_envelope decode expected (fst stored)).

Theorem processed_receipt_updates_preserve_complete_authorization : forall receipts processed,
  processed_envelope (apply_receipt_updates processed receipts) = processed_envelope processed.
Proof.
  induction receipts as [|receipt rest IH]; intros processed; simpl; auto.
  rewrite IH. reflexivity.
Qed.

Definition update_processed_at (state : nat -> retained_processed_deploy) key receipt query :=
  if Nat.eqb query key then replace_processed_receipt (state query) receipt else state query.

Theorem independent_processed_updates_commute : forall state first second a b query,
  first <> second ->
  update_processed_at (update_processed_at state first a) second b query =
  update_processed_at (update_processed_at state second b) first a query.
Proof.
  intros state first second a b query distinct. unfold update_processed_at.
  destruct (Nat.eqb query first) eqn:at_first;
    destruct (Nat.eqb query second) eqn:at_second; auto.
  apply Nat.eqb_eq in at_first. apply Nat.eqb_eq in at_second. congruence.
Qed.

Theorem processed_restore_preserves_envelope_and_receipt : forall encode decode processed,
  decode (encode (processed_envelope processed)) = Some (processed_envelope processed) ->
  restore_processed_deploy decode (retained_identity (processed_envelope processed))
    (stored_processed_deploy encode processed) = Some processed.
Proof.
  intros encode decode [envelope receipt] roundtrip.
  unfold restore_processed_deploy, stored_processed_deploy. simpl.
  rewrite (stored_envelope_roundtrip_retains_complete_authorization encode decode envelope roundtrip).
  reflexivity.
Qed.

Theorem processed_decode_failure_cannot_retain_a_receipt : forall decode expected wire receipt,
  restore_deploy_envelope decode expected wire = None ->
  restore_processed_deploy decode expected (wire, receipt) = None.
Proof.
  intros decode expected wire receipt failed.
  unfold restore_processed_deploy. simpl. now rewrite failed.
Qed.

Print Assumptions processed_receipt_updates_preserve_complete_authorization.
Print Assumptions independent_processed_updates_commute.
Print Assumptions processed_restore_preserves_envelope_and_receipt.
Print Assumptions processed_decode_failure_cannot_retain_a_receipt.

Section ProcessedSequence.
Context {Wire : Type}.
Variable decode : Wire -> option retained_processed_deploy.

Fixpoint decode_processed_sequence wires :=
  match wires with
  | [] => Some []
  | wire :: rest =>
      match decode wire with
      | None => None
      | Some record =>
          option_map (cons record) (decode_processed_sequence rest)
      end
  end.

Theorem processed_sequence_preserves_each_checked_record : forall wires records,
  decode_processed_sequence wires = Some records ->
  Forall2 (fun wire record => decode wire = Some record) wires records.
Proof.
  induction wires as [|wire rest IH]; intros records success; simpl in success.
  - inversion success; constructor.
  - destruct (decode wire) as [record|] eqn:checked; try discriminate.
    destruct (decode_processed_sequence rest) as [tail|] eqn:decoded; try discriminate.
    inversion success; subst. constructor; auto.
Qed.

Theorem processed_sequence_preserves_cardinality : forall wires records,
  decode_processed_sequence wires = Some records -> length wires = length records.
Proof.
  intros wires records success.
  apply processed_sequence_preserves_each_checked_record in success.
  induction success; simpl; congruence.
Qed.

Theorem processed_sequence_rejects_any_failed_record : forall wires bad,
  In bad wires -> decode bad = None -> decode_processed_sequence wires = None.
Proof.
  induction wires as [|wire rest IH]; intros bad member failed; simpl in *.
  - contradiction.
  - destruct member as [same|member].
    + subst. now rewrite failed.
    + destruct (decode wire); auto. rewrite (IH bad member failed). reflexivity.
Qed.

Theorem processed_sequence_roundtrip : forall encode records,
  Forall (fun record => decode (encode record) = Some record) records ->
  decode_processed_sequence (map encode records) = Some records.
Proof.
  intros encode records valid. induction valid; simpl; auto.
  rewrite H, IHvalid. reflexivity.
Qed.
End ProcessedSequence.

Print Assumptions processed_sequence_preserves_each_checked_record.
Print Assumptions processed_sequence_preserves_cardinality.
Print Assumptions processed_sequence_rejects_any_failed_record.
Print Assumptions processed_sequence_roundtrip.

Definition envelope_store_route format :=
  match format with
  | LegacyPayload => 0
  | BodyEnvelope => 1
  | FundedEnvelope | OfferedEnvelope => 2
  end.

Theorem funded_route_is_separate : forall format,
  envelope_store_route format = 2 <->
  format = FundedEnvelope \/ format = OfferedEnvelope.
Proof. destruct format; simpl; intuition discriminate. Qed.

Definition routed_envelope_write
    (state : nat -> nat -> option retained_deploy_envelope)
    format key envelope namespace query :=
  if Nat.eqb namespace (envelope_store_route format) && Nat.eqb query key
  then Some envelope else state namespace query.

Theorem routed_write_preserves_other_namespaces : forall state format key envelope namespace query,
  namespace <> envelope_store_route format ->
  routed_envelope_write state format key envelope namespace query = state namespace query.
Proof.
  intros state format key envelope namespace query distinct.
  unfold routed_envelope_write. apply Nat.eqb_neq in distinct. now rewrite distinct.
Qed.

Theorem routed_write_preserves_other_keys : forall state format key envelope namespace query,
  query <> key ->
  routed_envelope_write state format key envelope namespace query = state namespace query.
Proof.
  intros state format key envelope namespace query distinct.
  unfold routed_envelope_write. apply Nat.eqb_neq in distinct. rewrite distinct.
  now destruct (Nat.eqb namespace (envelope_store_route format)).
Qed.

Definition resolve_envelope_locations
    (historical funded : option retained_deploy_envelope) :=
  match historical, funded with
  | Some _, Some _ => None
  | Some envelope, None | None, Some envelope => Some (Some envelope)
  | None, None => Some None
  end.

Theorem duplicate_envelope_locations_reject : forall historical funded,
  resolve_envelope_locations (Some historical) (Some funded) = None.
Proof. reflexivity. Qed.

Theorem unique_envelope_location_preserved : forall envelope,
  resolve_envelope_locations (Some envelope) None = Some (Some envelope) /\
  resolve_envelope_locations None (Some envelope) = Some (Some envelope).
Proof. intros; split; reflexivity. Qed.

Print Assumptions funded_route_is_separate.
Print Assumptions routed_write_preserves_other_namespaces.
Print Assumptions routed_write_preserves_other_keys.
Print Assumptions duplicate_envelope_locations_reject.
Print Assumptions unique_envelope_location_preserved.

Definition legacy_order_projection (signers : list nat) order :=
  map (nth_error signers) order.

Theorem legacy_order_projection_cannot_invent_signers : forall signers order signer,
  In (Some signer) (legacy_order_projection signers order) -> In signer signers.
Proof.
  intros signers order signer present. unfold legacy_order_projection in present.
  apply in_map_iff in present. destruct present as [index [found _]].
  now apply nth_error_In in found.
Qed.

Theorem legacy_order_projection_preserves_length : forall signers order,
  length (legacy_order_projection signers order) = length order.
Proof. intros. apply length_map. Qed.

Theorem processed_updates_preserve_legacy_wire_order : forall receipts processed,
  retained_legacy_order (processed_envelope (apply_receipt_updates processed receipts)) =
    retained_legacy_order (processed_envelope processed).
Proof. intros. now rewrite processed_receipt_updates_preserve_complete_authorization. Qed.

Print Assumptions legacy_order_projection_cannot_invent_signers.
Print Assumptions legacy_order_projection_preserves_length.
Print Assumptions processed_updates_preserve_legacy_wire_order.

Theorem pending_body_adapter_preserves_legacy_order : forall pending envelope,
  pending_body_adapter pending = Some envelope ->
  retained_format envelope = LegacyPayload ->
  retained_legacy_order envelope = seq 0 (length (retained_legacy_order envelope)).
Proof.
  intros pending envelope accepted legacy.
  pose proof (pending_body_adapter_preserves_envelope pending envelope accepted) as [same _].
  subst envelope. unfold pending_body_adapter in accepted. rewrite legacy in accepted.
  destruct (Nat.eqb (retained_primary_index (pending_envelope pending)) 0); try discriminate.
  destruct (list_eq_dec Nat.eq_dec
    (retained_legacy_order (pending_envelope pending))
    (seq 0 (length (retained_legacy_order (pending_envelope pending))))); auto; discriminate.
Qed.

Print Assumptions pending_body_adapter_preserves_legacy_order.

Theorem legacy_metadata_projection_is_exact : forall envelope,
  legacy_metadata_projectable envelope = true <->
  retained_legacy_algorithms envelope = retained_canonical_algorithms envelope /\
  retained_legacy_wire_threshold envelope = Z.of_nat (retained_threshold envelope).
Proof.
  intros envelope. unfold legacy_metadata_projectable.
  destruct (list_eq_dec Nat.eq_dec (retained_legacy_algorithms envelope)
    (retained_canonical_algorithms envelope)) as [same|different].
  - rewrite Z.eqb_eq. tauto.
  - split; intros impossible; [discriminate|destruct impossible; contradiction].
Qed.

Definition historical_processed_threshold members (wire : Z) :=
  if Nat.leb members 1 then 0
  else if Z.leb wire 0 then 0 else Z.to_nat wire.

Theorem historical_single_signer_ignores_threshold : forall wire,
  historical_processed_threshold 1 wire = 0.
Proof. reflexivity. Qed.

Theorem historical_positive_compound_threshold : forall members wire,
  1 < members -> (0 < wire)%Z -> historical_processed_threshold members wire = Z.to_nat wire.
Proof.
  intros members wire many positive. unfold historical_processed_threshold.
  destruct (Nat.leb members 1) eqn:small.
  - apply Nat.leb_le in small. lia.
  - destruct (Z.leb wire 0) eqn:nonpositive; auto.
    apply Z.leb_le in nonpositive. lia.
Qed.

Print Assumptions legacy_metadata_projection_is_exact.
Print Assumptions historical_single_signer_ignores_threshold.
Print Assumptions historical_positive_compound_threshold.

Definition envelope_row_state := nat -> option retained_deploy_envelope.
Definition envelope_row_update := (nat * option retained_deploy_envelope)%type.

Definition apply_envelope_row (state : envelope_row_state) (update : envelope_row_update)
    : envelope_row_state :=
  fun key => if Nat.eqb key (fst update) then snd update else state key.

Fixpoint stage_envelope_batch (state : envelope_row_state)
    (updates : list envelope_row_update) : envelope_row_state :=
  match updates with
  | [] => state
  | update :: rest => stage_envelope_batch (apply_envelope_row state update) rest
  end.

Definition publish_envelope_batch (state : envelope_row_state)
    (prepared : option (list envelope_row_update)) (committed : bool) :=
  match prepared, committed with
  | Some updates, true => stage_envelope_batch state updates
  | _, _ => state
  end.

Theorem rejected_batch_preparation_preserves_store : forall state committed,
  publish_envelope_batch state None committed = state.
Proof. intros; now destruct committed. Qed.

Theorem failed_batch_transaction_preserves_store : forall state prepared,
  publish_envelope_batch state prepared false = state.
Proof. intros; now destruct prepared. Qed.

Theorem committed_batch_publishes_all_rows : forall state updates,
  publish_envelope_batch state (Some updates) true = stage_envelope_batch state updates.
Proof. reflexivity. Qed.

Theorem staged_batch_preserves_untouched_rows : forall updates state key,
  ~ In key (map fst updates) -> stage_envelope_batch state updates key = state key.
Proof.
  induction updates as [|[changed value] rest IH]; intros state key absent; simpl in *; auto.
  rewrite IH by tauto. unfold apply_envelope_row; simpl.
  destruct (Nat.eqb key changed) eqn:same; auto.
  apply Nat.eqb_eq in same. subst; tauto.
Qed.

Theorem staged_batch_append : forall first second state,
  stage_envelope_batch state (first ++ second) =
    stage_envelope_batch (stage_envelope_batch state first) second.
Proof. induction first; intros; simpl; auto. Qed.

Theorem staged_duplicate_rows_use_last_value : forall state key first last,
  stage_envelope_batch state [(key, first); (key, last)] key = last.
Proof. intros; simpl; unfold apply_envelope_row; simpl; now rewrite Nat.eqb_refl. Qed.

Theorem atomic_batch_has_no_partial_publication : forall state updates committed,
  publish_envelope_batch state (Some updates) committed = state \/
  publish_envelope_batch state (Some updates) committed = stage_envelope_batch state updates.
Proof. intros; destruct committed; [right|left]; reflexivity. Qed.

Print Assumptions rejected_batch_preparation_preserves_store.
Print Assumptions failed_batch_transaction_preserves_store.
Print Assumptions committed_batch_publishes_all_rows.
Print Assumptions staged_batch_preserves_untouched_rows.
Print Assumptions staged_batch_append.
Print Assumptions staged_duplicate_rows_use_last_value.
Print Assumptions atomic_batch_has_no_partial_publication.

Definition historical_storage_format format :=
  match format with LegacyPayload | BodyEnvelope => true | _ => false end.

Definition check_storable_envelope
    (bounded_check : option (retained_deploy_envelope -> bool)) envelope :=
  match bounded_check with
  | Some check => check envelope
  | None => historical_storage_format (retained_format envelope)
  end.

Theorem historical_storage_rejects_funded_envelopes : forall envelope,
  retained_format envelope = FundedEnvelope \/ retained_format envelope = OfferedEnvelope ->
  check_storable_envelope None envelope = false.
Proof. intros envelope [funded|offered]; unfold check_storable_envelope; now rewrite ?funded, ?offered. Qed.

Theorem bounded_storage_uses_configured_check : forall check envelope,
  check_storable_envelope (Some check) envelope = check envelope.
Proof. reflexivity. Qed.

Definition prepare_block_envelopes check envelopes :=
  if forallb (check_storable_envelope check) envelopes then Some envelopes else None.

Theorem prepared_block_retains_all_envelopes : forall check original prepared,
  prepare_block_envelopes check original = Some prepared -> prepared = original.
Proof.
  intros check original prepared accepted. unfold prepare_block_envelopes in accepted.
  destruct (forallb _ original); congruence.
Qed.

Theorem prepared_block_checks_every_envelope : forall check original prepared,
  prepare_block_envelopes check original = Some prepared ->
  forall envelope, In envelope original -> check_storable_envelope check envelope = true.
Proof.
  intros check original prepared accepted. unfold prepare_block_envelopes in accepted.
  destruct (forallb _ original) eqn:checked; try discriminate.
  exact (proj1 (forallb_forall (check_storable_envelope check) original) checked).
Qed.

Theorem failed_envelope_prevents_block_preparation : forall check original envelope,
  In envelope original -> check_storable_envelope check envelope = false ->
  prepare_block_envelopes check original = None.
Proof.
  intros check original envelope present failed.
  destruct (prepare_block_envelopes check original) as [prepared|] eqn:result; auto.
  pose proof (prepared_block_checks_every_envelope check original prepared result envelope present).
  congruence.
Qed.

Print Assumptions historical_storage_rejects_funded_envelopes.
Print Assumptions bounded_storage_uses_configured_check.
Print Assumptions prepared_block_retains_all_envelopes.
Print Assumptions prepared_block_checks_every_envelope.
Print Assumptions failed_envelope_prevents_block_preparation.

Definition runtime_envelope_identity (authority_of : list nat -> list nat) envelope :=
  (retained_identity envelope, authority_of (retained_authorization envelope)).

Theorem runtime_identity_preserves_envelope_commitment : forall authority_of envelope,
  fst (runtime_envelope_identity authority_of envelope) = retained_identity envelope.
Proof. reflexivity. Qed.

Theorem runtime_identity_preserves_envelope_authorization : forall authority_of envelope,
  snd (runtime_envelope_identity authority_of envelope) = authority_of (retained_authorization envelope).
Proof. reflexivity. Qed.

Theorem different_commitments_have_different_runtime_identities : forall authority_of first second,
  retained_identity first <> retained_identity second ->
  runtime_envelope_identity authority_of first <> runtime_envelope_identity authority_of second.
Proof.
  intros authority_of first second different equal.
  apply different. now apply (f_equal fst) in equal.
Qed.

Theorem unchanged_authorization_keeps_runtime_authority : forall authority_of first second,
  authority_of (retained_authorization first) = authority_of (retained_authorization second) ->
  snd (runtime_envelope_identity authority_of first) = snd (runtime_envelope_identity authority_of second).
Proof. auto. Qed.

Print Assumptions runtime_identity_preserves_envelope_commitment.
Print Assumptions runtime_identity_preserves_envelope_authorization.
Print Assumptions different_commitments_have_different_runtime_identities.
Print Assumptions unchanged_authorization_keeps_runtime_authority.

Definition bound_runtime_seed prefix envelope := prefix ++ retained_identity envelope.

Theorem bound_runtime_seed_binds_identity : forall prefix first second,
  bound_runtime_seed prefix first = bound_runtime_seed prefix second <->
  retained_identity first = retained_identity second.
Proof.
  intros. unfold bound_runtime_seed. split; intro same.
  - now apply app_inv_head in same.
  - now rewrite same.
Qed.

Theorem bound_runtime_seed_length : forall prefix envelope,
  length (bound_runtime_seed prefix envelope) = length prefix + length (retained_identity envelope).
Proof. intros. apply length_app. Qed.

Print Assumptions bound_runtime_seed_binds_identity.
Print Assumptions bound_runtime_seed_length.
