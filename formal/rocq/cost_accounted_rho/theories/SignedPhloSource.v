From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Sorting.Permutation Lia.
From CostAccountedRho Require Import SignedPhloWire SignedPhloSchedule SignedPhloResource.
Import ListNotations.

Definition phlo_permission_fields domain width count_width resources :=
  phlo_word count_width (length resources) :: map (phlo_resource_bytes domain width count_width) resources.

Definition phlo_permission_bytes domain width count_width resources :=
  phlo_fields width (phlo_permission_fields domain width count_width resources).

Lemma phlo_resource_map_is_injective : forall domain width count_width maximum first second,
  0 < width ->
  Forall (phlo_resource_fits domain width count_width maximum) first ->
  Forall (phlo_resource_fits domain width count_width maximum) second ->
  map (phlo_resource_bytes domain width count_width) first =
    map (phlo_resource_bytes domain width count_width) second -> first = second.
Proof.
  intros domain width count_width maximum first. induction first as [|head tail IH];
    intros [|other rest] positive first_fits second_fits same; try discriminate; [reflexivity|].
  apply Forall_cons_iff in first_fits. destruct first_fits as [head_fits tail_fits].
  apply Forall_cons_iff in second_fits. destruct second_fits as [other_fits rest_fits].
  cbn in same. injection same as heads tails.
  assert (head = other) by (eapply phlo_resource_encoding_binds_every_component; eauto).
  subst other. f_equal. eapply IH; eauto.
Qed.

Theorem phlo_permission_encoding_preserves_every_key : forall domain width count_width maximum first second,
  0 < width ->
  Forall (phlo_resource_fits domain width count_width maximum) first ->
  Forall (phlo_resource_fits domain width count_width maximum) second ->
  phlo_fields_fit width maximum (phlo_permission_fields domain width count_width first) ->
  phlo_fields_fit width maximum (phlo_permission_fields domain width count_width second) ->
  phlo_permission_bytes domain width count_width first =
    phlo_permission_bytes domain width count_width second -> first = second.
Proof.
  intros domain width count_width maximum first second positive first_keys second_keys
    first_fields second_fields same.
  unfold phlo_permission_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  unfold phlo_permission_fields in same. injection same as counts keys.
  eapply phlo_resource_map_is_injective; eauto.
Qed.

Definition phlo_bool_bytes (value : bool) := [if value then 1 else 0].

Record phlo_source_record := {
  source_record_custody : list nat;
  source_record_hold : nat;
  source_record_debit : nat;
  source_record_fee : bool;
  source_record_permissions : list phlo_resource_record
}.

Definition phlo_source_fields domain resource_domain width word_width count_width record :=
  [domain; source_record_custody record; phlo_word word_width (source_record_hold record);
   phlo_word word_width (source_record_debit record); phlo_bool_bytes (source_record_fee record);
   phlo_permission_bytes resource_domain width count_width (source_record_permissions record)].

Definition phlo_source_bytes domain resource_domain width word_width count_width record :=
  phlo_fields width (phlo_source_fields domain resource_domain width word_width count_width record).

Definition phlo_source_fits domain resource_domain width word_width count_width maximum record :=
  source_record_hold record < 256 ^ word_width /\ source_record_debit record < 256 ^ word_width /\
  Forall (phlo_resource_fits resource_domain width count_width maximum) (source_record_permissions record) /\
  phlo_fields_fit width maximum
    (phlo_permission_fields resource_domain width count_width (source_record_permissions record)) /\
  phlo_fields_fit width maximum (phlo_source_fields domain resource_domain width word_width count_width record).

Theorem phlo_source_encoding_binds_every_consent_field :
  forall domain resource_domain width word_width count_width maximum first second,
  0 < width ->
  phlo_source_fits domain resource_domain width word_width count_width maximum first ->
  phlo_source_fits domain resource_domain width word_width count_width maximum second ->
  phlo_source_bytes domain resource_domain width word_width count_width first =
    phlo_source_bytes domain resource_domain width word_width count_width second -> first = second.
Proof.
  intros domain resource_domain width word_width count_width maximum first second positive
    [first_hold [first_debit [first_keys [first_permissions first_fields]]]]
    [second_hold [second_debit [second_keys [second_permissions second_fields]]]] same.
  unfold phlo_source_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first as [custody hold debit fee permissions].
  destruct second as [other_custody other_hold other_debit other_fee other_permissions].
  cbn [phlo_source_fields source_record_custody source_record_hold source_record_debit
    source_record_fee source_record_permissions] in *.
  injection same as custodies holds debits fees permission_lists.
  apply phlo_word_encoding_is_injective in holds; auto.
  apply phlo_word_encoding_is_injective in debits; auto.
  assert (fee = other_fee) by (destruct fee; destruct other_fee; cbn in fees; congruence).
  assert (permissions = other_permissions) by (eapply phlo_permission_encoding_preserves_every_key; eauto).
  subst. reflexivity.
Qed.

Definition phlo_source_allows record resource := In resource (source_record_permissions record).

Definition phlo_authority_node_eq_dec : forall (lhs rhs : phlo_authority_node),
  {lhs = rhs} + {lhs <> rhs}.
Proof. decide equality; apply list_eq_dec; apply Nat.eq_dec. Defined.

Definition phlo_resource_record_eq_dec : forall (lhs rhs : phlo_resource_record),
  {lhs = rhs} + {lhs <> rhs}.
Proof.
  decide equality; auto using Nat.eq_dec;
    apply list_eq_dec; auto using Nat.eq_dec, phlo_authority_node_eq_dec.
Defined.

Theorem phlo_permission_normalization_preserves_authorization : forall record ordered,
  Permutation (source_record_permissions record) ordered ->
  forall resource, phlo_source_allows record resource <->
    In resource (nodup phlo_resource_record_eq_dec ordered).
Proof.
  intros record ordered reordered resource. unfold phlo_source_allows. rewrite nodup_In.
  split; intro included.
  - exact (Permutation_in resource reordered included).
  - exact (Permutation_in resource (Permutation_sym reordered) included).
Qed.

Theorem phlo_permission_repetition_does_not_expand_authorization : forall record resource candidate,
  In resource (source_record_permissions record) ->
  In candidate (resource :: source_record_permissions record) <-> phlo_source_allows record candidate.
Proof.
  intros record resource candidate present. unfold phlo_source_allows. cbn.
  split; [intros [same|included]; [subst; assumption|assumption]|auto].
Qed.

Theorem phlo_empty_resource_permissions_authorize_no_resources : forall record,
  source_record_permissions record = [] ->
  (forall resource, ~ phlo_source_allows record resource).
Proof. intros record empty resource. unfold phlo_source_allows. rewrite empty. cbn. auto. Qed.

Print Assumptions phlo_resource_map_is_injective.
Print Assumptions phlo_permission_encoding_preserves_every_key.
Print Assumptions phlo_source_encoding_binds_every_consent_field.
Print Assumptions phlo_permission_normalization_preserves_authorization.
Print Assumptions phlo_permission_repetition_does_not_expand_authorization.
Print Assumptions phlo_empty_resource_permissions_authorize_no_resources.
Print Assumptions phlo_authority_node_eq_dec.
Print Assumptions phlo_resource_record_eq_dec.
