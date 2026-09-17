From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From CostAccountedRho Require Import SignedPhloWire SignedPhloSchedule SignedPhloResource.
Import ListNotations.

Inductive phlo_obligation_record :=
| PhloWireFee
| PhloWireResource (resource : phlo_resource_record).

Definition phlo_obligation_fields domain resource_domain width count_width record :=
  match record with
  | PhloWireFee => [domain; [0]; []]
  | PhloWireResource resource =>
      [domain; [1]; phlo_resource_bytes resource_domain width count_width resource]
  end.

Definition phlo_obligation_bytes domain resource_domain width count_width record :=
  phlo_fields width (phlo_obligation_fields domain resource_domain width count_width record).

Definition phlo_obligation_fits domain resource_domain width count_width maximum record :=
  phlo_fields_fit width maximum (phlo_obligation_fields domain resource_domain width count_width record) /\
  match record with
  | PhloWireFee => True
  | PhloWireResource resource => phlo_resource_fits resource_domain width count_width maximum resource
  end.

Theorem phlo_obligation_encoding_binds_kind_and_resource : forall domain resource_domain width count_width maximum first second,
  0 < width ->
  phlo_obligation_fits domain resource_domain width count_width maximum first ->
  phlo_obligation_fits domain resource_domain width count_width maximum second ->
  phlo_obligation_bytes domain resource_domain width count_width first =
    phlo_obligation_bytes domain resource_domain width count_width second -> first = second.
Proof.
  intros domain resource_domain width count_width maximum first second positive [first_fields first_resource] [second_fields second_resource] same.
  unfold phlo_obligation_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first; destruct second; simpl in same; try discriminate; [reflexivity|].
  injection same as resources. f_equal.
  eapply phlo_resource_encoding_binds_every_component; eauto.
Qed.

Lemma phlo_field_encoded_length : forall width payload,
  length (phlo_field width payload) = width + length payload.
Proof. intros. unfold phlo_field. rewrite length_app, phlo_word_exact_width. reflexivity. Qed.

Lemma phlo_fields_encoded_length : forall width fields,
  length (phlo_fields width fields) =
    fold_right (fun field total => width + length field + total) 0 fields.
Proof.
  intros width fields. induction fields as [|field rest IHfields]; [reflexivity|].
  change (length (phlo_field width field ++ phlo_fields width rest) =
    width + length field + fold_right (fun field total => width + length field + total) 0 rest).
  rewrite length_app, phlo_field_encoded_length, IHfields. reflexivity.
Qed.

Definition phlo_node_payload node :=
  match node with PhloGround bytes | PhloQuote bytes => bytes | _ => [] end.

Definition phlo_node_encoded_size width node := 2 * width + 1 + length (phlo_node_payload node).

Definition phlo_authority_encoded_size width count_width nodes :=
  width + count_width + fold_right (fun node total => width + phlo_node_encoded_size width node + total) 0 nodes.

Definition phlo_resource_encoded_size (domain : list nat) width count_width record :=
  5 * width + length domain + length (resource_record_location record) + count_width +
    length (resource_record_terms record) +
    phlo_authority_encoded_size width count_width (resource_record_authority record).

Definition phlo_obligation_encoded_size (domain : list nat) resource_domain width count_width record :=
  3 * width + length domain + 1 +
    match record with
    | PhloWireFee => 0
    | PhloWireResource resource => phlo_resource_encoded_size resource_domain width count_width resource
    end.

Theorem phlo_node_size_is_exact : forall width node,
  length (phlo_node_bytes width node) = phlo_node_encoded_size width node.
Proof.
  intros width node. unfold phlo_node_bytes, phlo_node_encoded_size.
  rewrite phlo_fields_encoded_length. destruct node; simpl; lia.
Qed.

Theorem phlo_authority_size_is_exact : forall width count_width nodes,
  length (phlo_authority_bytes width count_width nodes) = phlo_authority_encoded_size width count_width nodes.
Proof.
  intros width count_width nodes. unfold phlo_authority_bytes, phlo_authority_fields, phlo_authority_encoded_size.
  rewrite phlo_fields_encoded_length. simpl. rewrite phlo_word_exact_width.
  f_equal. induction nodes; simpl; [reflexivity|]. rewrite phlo_node_size_is_exact, IHnodes. reflexivity.
Qed.

Theorem phlo_resource_size_is_exact : forall domain width count_width record,
  length (phlo_resource_bytes domain width count_width record) = phlo_resource_encoded_size domain width count_width record.
Proof.
  intros. unfold phlo_resource_bytes, phlo_resource_fields, phlo_resource_encoded_size.
  rewrite phlo_fields_encoded_length. simpl. rewrite phlo_word_exact_width, phlo_authority_size_is_exact. lia.
Qed.

Theorem phlo_obligation_size_is_exact : forall domain resource_domain width count_width record,
  length (phlo_obligation_bytes domain resource_domain width count_width record) =
    phlo_obligation_encoded_size domain resource_domain width count_width record.
Proof.
  intros. unfold phlo_obligation_bytes, phlo_obligation_fields, phlo_obligation_encoded_size.
  destruct record; rewrite phlo_fields_encoded_length; simpl; [lia|].
  rewrite phlo_resource_size_is_exact. lia.
Qed.

Definition phlo_streamed_nodes width nodes :=
  flat_map (fun node => phlo_word width (phlo_node_encoded_size width node) ++ phlo_node_bytes width node) nodes.

Theorem phlo_streamed_nodes_preserve_bytes : forall width nodes,
  phlo_streamed_nodes width nodes = phlo_fields width (map (phlo_node_bytes width) nodes).
Proof.
  intros width nodes. induction nodes as [|node rest IH]; [reflexivity|].
  change ((phlo_word width (phlo_node_encoded_size width node) ++ phlo_node_bytes width node) ++
    phlo_streamed_nodes width rest = phlo_field width (phlo_node_bytes width node) ++
    phlo_fields width (map (phlo_node_bytes width) rest)).
  rewrite IH. unfold phlo_field at 1. rewrite phlo_node_size_is_exact. reflexivity.
Qed.

Definition phlo_streamed_resource domain width count_width record :=
  phlo_fields width [domain; resource_record_location record;
    phlo_word count_width (resource_record_class record); resource_record_terms record] ++
  phlo_word width (phlo_authority_encoded_size width count_width (resource_record_authority record)) ++
  phlo_field width (phlo_word count_width (length (resource_record_authority record))) ++
  phlo_streamed_nodes width (resource_record_authority record).

Theorem phlo_streamed_resource_preserves_bytes : forall domain width count_width record,
  phlo_streamed_resource domain width count_width record = phlo_resource_bytes domain width count_width record.
Proof.
  intros. unfold phlo_streamed_resource, phlo_resource_bytes, phlo_resource_fields.
  rewrite phlo_streamed_nodes_preserve_bytes.
  rewrite <- phlo_authority_size_is_exact.
  unfold phlo_authority_bytes, phlo_authority_fields, phlo_fields.
  cbn [map concat]. unfold phlo_field.
  repeat rewrite app_nil_r. repeat rewrite app_assoc. reflexivity.
Qed.

Definition phlo_streamed_obligation domain resource_domain width count_width record :=
  match record with
  | PhloWireFee => phlo_fields width [domain; [0]; []]
  | PhloWireResource resource => phlo_fields width [domain; [1]] ++
      phlo_word width (phlo_resource_encoded_size resource_domain width count_width resource) ++
      phlo_streamed_resource resource_domain width count_width resource
  end.

Theorem phlo_streamed_obligation_preserves_bytes : forall domain resource_domain width count_width record,
  phlo_streamed_obligation domain resource_domain width count_width record =
    phlo_obligation_bytes domain resource_domain width count_width record.
Proof.
  intros. destruct record; [reflexivity|].
  unfold phlo_streamed_obligation, phlo_obligation_bytes, phlo_obligation_fields.
  rewrite phlo_streamed_resource_preserves_bytes, <- phlo_resource_size_is_exact.
  unfold phlo_fields. cbn [map concat]. unfold phlo_field.
  repeat rewrite app_nil_r. repeat rewrite app_assoc. reflexivity.
Qed.

Definition phlo_push_capacity used capacity :=
  if used <? capacity then capacity else Nat.max 1 (2 * capacity).

Theorem phlo_push_capacity_reserves_before_insertion : forall used capacity,
  used <= capacity -> S used <= phlo_push_capacity used capacity.
Proof.
  intros. unfold phlo_push_capacity. destruct (used <? capacity) eqn:inside.
  - apply Nat.ltb_lt in inside. lia.
  - apply Nat.ltb_ge in inside. destruct capacity; simpl; lia.
Qed.

Theorem phlo_pop_retains_reserved_capacity : forall used capacity,
  used <= capacity -> Nat.pred used <= capacity.
Proof. intros. lia. Qed.

Theorem phlo_push_with_space_needs_no_growth : forall used capacity,
  used < capacity -> phlo_push_capacity used capacity = capacity.
Proof. intros. unfold phlo_push_capacity. now rewrite (proj2 (Nat.ltb_lt _ _) H). Qed.

Theorem phlo_reserved_output_prefix_stays_bounded : forall total prefix remaining,
  prefix + remaining = total -> prefix <= total /\ remaining <= total.
Proof. intros. lia. Qed.

Theorem phlo_fee_and_resource_keys_cannot_alias : forall domain resource_domain width count_width maximum resource,
  0 < width ->
  phlo_obligation_fits domain resource_domain width count_width maximum PhloWireFee ->
  phlo_obligation_fits domain resource_domain width count_width maximum (PhloWireResource resource) ->
  phlo_obligation_bytes domain resource_domain width count_width PhloWireFee <>
    phlo_obligation_bytes domain resource_domain width count_width (PhloWireResource resource).
Proof.
  intros domain resource_domain width count_width maximum resource positive fee_fits resource_fits same.
  pose proof (phlo_obligation_encoding_binds_kind_and_resource _ _ _ _ _ _ _ positive fee_fits resource_fits same). discriminate.
Qed.

Theorem phlo_obligation_encoding_preserves_occurrence_count : forall domain resource_domain width count_width records,
  length (map (phlo_obligation_bytes domain resource_domain width count_width) records) = length records.
Proof. intros. apply length_map. Qed.

Theorem equal_encoded_phlo_obligations_have_equal_properties : forall (property : phlo_obligation_record -> nat)
  domain resource_domain width count_width maximum first second,
  0 < width ->
  phlo_obligation_fits domain resource_domain width count_width maximum first ->
  phlo_obligation_fits domain resource_domain width count_width maximum second ->
  phlo_obligation_bytes domain resource_domain width count_width first =
    phlo_obligation_bytes domain resource_domain width count_width second -> property first = property second.
Proof.
  intros property domain resource_domain width count_width maximum first second positive first_fits second_fits same.
  f_equal. eapply phlo_obligation_encoding_binds_kind_and_resource; eauto.
Qed.
