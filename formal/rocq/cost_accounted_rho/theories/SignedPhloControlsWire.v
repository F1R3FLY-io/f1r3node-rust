From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From CostAccountedRho Require Import SignedPhloWire SignedPhloSchedule.
Import ListNotations.

Definition phlo_record_list_fields {A} count_width (encode : A -> list nat) records :=
  phlo_word count_width (length records) :: map encode records.

Definition phlo_record_list_bytes {A} width count_width (encode : A -> list nat) records :=
  phlo_fields width (phlo_record_list_fields count_width encode records).

Lemma phlo_valid_map_injective : forall A (encode : A -> list nat) valid,
  (forall first second, valid first -> valid second -> encode first = encode second -> first = second) ->
  forall first second, Forall valid first -> Forall valid second ->
  map encode first = map encode second -> first = second.
Proof.
  intros A encode valid injective first. induction first as [|head tail IH];
    intros [|other rest] first_valid second_valid same; try discriminate; [reflexivity|].
  apply Forall_cons_iff in first_valid. apply Forall_cons_iff in second_valid.
  destruct first_valid as [head_valid tail_valid]. destruct second_valid as [other_valid rest_valid].
  cbn in same. injection same as heads tails.
  assert (head = other) by (apply injective; assumption).
  subst. f_equal. apply IH; assumption.
Qed.

Theorem phlo_record_list_encoding_preserves_all_entries :
  forall A (encode : A -> list nat) valid width count_width maximum first second,
  0 < width ->
  (forall lhs rhs, valid lhs -> valid rhs -> encode lhs = encode rhs -> lhs = rhs) ->
  Forall valid first -> Forall valid second ->
  phlo_fields_fit width maximum (phlo_record_list_fields count_width encode first) ->
  phlo_fields_fit width maximum (phlo_record_list_fields count_width encode second) ->
  phlo_record_list_bytes width count_width encode first =
    phlo_record_list_bytes width count_width encode second -> first = second.
Proof.
  intros A encode valid width count_width maximum first second positive injective
    first_valid second_valid first_fields second_fields same.
  unfold phlo_record_list_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  unfold phlo_record_list_fields in same. injection same as counts entries.
  eapply phlo_valid_map_injective; eauto.
Qed.

Record phlo_controls_record := {
  controls_record_limit : nat;
  controls_record_price_ceiling : nat;
  controls_record_owner_ceilings : list nat;
  controls_record_schedules : list phlo_schedule_record
}.

Definition phlo_controls_record_fields domain schedule_domain width word_width count_width scale_width record :=
  [domain; phlo_word word_width (controls_record_limit record);
   phlo_word word_width (controls_record_price_ceiling record);
   phlo_record_list_bytes width count_width (phlo_word word_width) (controls_record_owner_ceilings record);
   phlo_record_list_bytes width count_width
     (phlo_schedule_bytes schedule_domain width word_width count_width scale_width) (controls_record_schedules record)].

Definition phlo_controls_record_bytes domain schedule_domain width word_width count_width scale_width record :=
  phlo_fields width (phlo_controls_record_fields domain schedule_domain width word_width count_width scale_width record).

Definition phlo_controls_record_fits domain schedule_domain width word_width count_width scale_width maximum record :=
  controls_record_limit record < 256 ^ word_width /\
  controls_record_price_ceiling record < 256 ^ word_width /\
  Forall (fun ceiling => ceiling < 256 ^ word_width) (controls_record_owner_ceilings record) /\
  Forall (phlo_schedule_fits schedule_domain width word_width count_width scale_width maximum)
    (controls_record_schedules record) /\
  phlo_fields_fit width maximum
    (phlo_record_list_fields count_width (phlo_word word_width) (controls_record_owner_ceilings record)) /\
  phlo_fields_fit width maximum
    (phlo_record_list_fields count_width
      (phlo_schedule_bytes schedule_domain width word_width count_width scale_width) (controls_record_schedules record)) /\
  phlo_fields_fit width maximum
    (phlo_controls_record_fields domain schedule_domain width word_width count_width scale_width record).

Theorem phlo_controls_wire_binds_every_term :
  forall domain schedule_domain width word_width count_width scale_width maximum first second,
  0 < width ->
  phlo_controls_record_fits domain schedule_domain width word_width count_width scale_width maximum first ->
  phlo_controls_record_fits domain schedule_domain width word_width count_width scale_width maximum second ->
  phlo_controls_record_bytes domain schedule_domain width word_width count_width scale_width first =
    phlo_controls_record_bytes domain schedule_domain width word_width count_width scale_width second -> first = second.
Proof.
  intros domain schedule_domain width word_width count_width scale_width maximum first second positive
    [first_limit [first_price [first_owners [first_schedules [first_owner_fields [first_schedule_fields first_fields]]]]]]
    [second_limit [second_price [second_owners [second_schedules [second_owner_fields [second_schedule_fields second_fields]]]]]] same.
  unfold phlo_controls_record_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first as [limit price owners schedules]. destruct second as [other_limit other_price other_owners other_schedules].
  cbn [phlo_controls_record_fields controls_record_limit controls_record_price_ceiling
    controls_record_owner_ceilings controls_record_schedules] in *.
  injection same as limits prices owner_records schedule_records.
  apply phlo_word_encoding_is_injective in limits; auto.
  apply phlo_word_encoding_is_injective in prices; auto.
  assert (owners = other_owners).
  { eapply phlo_record_list_encoding_preserves_all_entries with
      (valid := fun ceiling => ceiling < 256 ^ word_width); eauto.
    intros. eapply phlo_word_encoding_is_injective; eauto. }
  assert (schedules = other_schedules).
  { eapply phlo_record_list_encoding_preserves_all_entries with
      (valid := phlo_schedule_fits schedule_domain width word_width count_width scale_width maximum); eauto.
    intros. eapply phlo_schedule_wire_binds_every_component; eauto. }
  subst. reflexivity.
Qed.

Theorem phlo_controls_format_domain_is_bound :
  forall domain other_domain schedule_domain width word_width count_width scale_width maximum first second,
  0 < width ->
  phlo_fields_fit width maximum
    (phlo_controls_record_fields domain schedule_domain width word_width count_width scale_width first) ->
  phlo_fields_fit width maximum
    (phlo_controls_record_fields other_domain schedule_domain width word_width count_width scale_width second) ->
  phlo_controls_record_bytes domain schedule_domain width word_width count_width scale_width first =
    phlo_controls_record_bytes other_domain schedule_domain width word_width count_width scale_width second -> domain = other_domain.
Proof.
  intros domain other_domain schedule_domain width word_width count_width scale_width maximum first second
    positive first_fields second_fields same.
  unfold phlo_controls_record_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  exact (f_equal (fun fields => hd [] fields) same).
Qed.

Print Assumptions phlo_valid_map_injective.
Print Assumptions phlo_record_list_encoding_preserves_all_entries.
Print Assumptions phlo_controls_wire_binds_every_term.
Print Assumptions phlo_controls_format_domain_is_bound.
