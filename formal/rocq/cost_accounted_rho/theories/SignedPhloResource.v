From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From CostAccountedRho Require Import SignedPhloWire SignedPhloSchedule.
Import ListNotations.

Inductive phlo_authority_node :=
| PhloUnit
| PhloGround (identity : list nat)
| PhloQuote (identity : list nat)
| PhloAnd.

Definition phlo_node_fields node :=
  match node with
  | PhloUnit => [[0]; []]
  | PhloGround identity => [[1]; identity]
  | PhloQuote identity => [[2]; identity]
  | PhloAnd => [[3]; []]
  end.

Definition phlo_node_bytes width node := phlo_fields width (phlo_node_fields node).

Theorem phlo_node_encoding_is_injective : forall width maximum first second,
  0 < width ->
  phlo_fields_fit width maximum (phlo_node_fields first) ->
  phlo_fields_fit width maximum (phlo_node_fields second) ->
  phlo_node_bytes width first = phlo_node_bytes width second -> first = second.
Proof.
  intros width maximum first second positive first_fits second_fits same.
  unfold phlo_node_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first; destruct second; cbn in same; congruence.
Qed.

Definition phlo_authority_fields width count_width nodes :=
  phlo_word count_width (length nodes) :: map (phlo_node_bytes width) nodes.

Definition phlo_authority_bytes width count_width nodes :=
  phlo_fields width (phlo_authority_fields width count_width nodes).

Lemma phlo_node_map_is_injective : forall width maximum first second,
  0 < width ->
  Forall (fun node => phlo_fields_fit width maximum (phlo_node_fields node)) first ->
  Forall (fun node => phlo_fields_fit width maximum (phlo_node_fields node)) second ->
  map (phlo_node_bytes width) first = map (phlo_node_bytes width) second -> first = second.
Proof.
  intros width maximum first. induction first as [|head tail IH];
    intros [|other rest] positive first_fits second_fits same; try discriminate; [reflexivity|].
  apply Forall_cons_iff in first_fits. destruct first_fits as [head_fits tail_fits].
  apply Forall_cons_iff in second_fits. destruct second_fits as [other_fits rest_fits].
  cbn in same. injection same as heads tails.
  assert (head = other) by (eapply phlo_node_encoding_is_injective; eauto).
  subst other. f_equal. eapply IH; eauto.
Qed.

Theorem phlo_authority_encoding_preserves_every_node : forall width count_width maximum first second,
  0 < width ->
  Forall (fun node => phlo_fields_fit width maximum (phlo_node_fields node)) first ->
  Forall (fun node => phlo_fields_fit width maximum (phlo_node_fields node)) second ->
  phlo_fields_fit width maximum (phlo_authority_fields width count_width first) ->
  phlo_fields_fit width maximum (phlo_authority_fields width count_width second) ->
  phlo_authority_bytes width count_width first = phlo_authority_bytes width count_width second ->
  first = second.
Proof.
  intros width count_width maximum first second positive first_nodes second_nodes
    first_fits second_fits same.
  unfold phlo_authority_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  unfold phlo_authority_fields in same. injection same as counts nodes.
  eapply phlo_node_map_is_injective; eauto.
Qed.

Fixpoint phlo_scan_authority slots nodes :=
  match nodes with
  | [] => Some slots
  | node :: rest =>
      match slots with
      | 0 => None
      | S remaining => phlo_scan_authority
          (match node with PhloAnd => S (S remaining) | _ => remaining end) rest
      end
  end.

Inductive phlo_authority_tree :=
| PhloTreeUnit
| PhloTreeGround (identity : list nat)
| PhloTreeQuote (identity : list nat)
| PhloTreeAnd (left right : phlo_authority_tree).

Lemma phlo_authority_scan_append : forall prefix slots suffix,
  phlo_scan_authority slots (prefix ++ suffix) =
    match phlo_scan_authority slots prefix with
    | Some remaining => phlo_scan_authority remaining suffix
    | None => None
    end.
Proof.
  induction prefix as [|node rest IH]; intros [|slots] suffix; cbn; auto.
Qed.

Theorem phlo_complete_authority_has_no_accepted_extension : forall prefix node rest,
  phlo_scan_authority 1 prefix = Some 0 ->
  phlo_scan_authority 1 (prefix ++ node :: rest) = None.
Proof.
  intros prefix node rest complete. rewrite phlo_authority_scan_append, complete. reflexivity.
Qed.

Fixpoint phlo_tree_nodes tree :=
  match tree with
  | PhloTreeUnit => [PhloUnit]
  | PhloTreeGround identity => [PhloGround identity]
  | PhloTreeQuote identity => [PhloQuote identity]
  | PhloTreeAnd lhs rhs => PhloAnd :: phlo_tree_nodes lhs ++ phlo_tree_nodes rhs
  end.

Theorem phlo_tree_scan_consumes_exactly_one_slot : forall tree slots rest,
  phlo_scan_authority (S slots) (phlo_tree_nodes tree ++ rest) = phlo_scan_authority slots rest.
Proof.
  induction tree as [|identity|identity|left IHleft right IHright]; intros slots rest; cbn; auto.
  rewrite <- app_assoc, IHleft, IHright. reflexivity.
Qed.

Theorem phlo_tree_encoding_has_complete_shape : forall tree,
  phlo_scan_authority 1 (phlo_tree_nodes tree) = Some 0.
Proof.
  intros tree. rewrite <- (app_nil_r (phlo_tree_nodes tree)).
  apply (phlo_tree_scan_consumes_exactly_one_slot tree 0 []).
Qed.

Theorem phlo_tree_encoding_rejects_additional_roots : forall tree node rest,
  phlo_scan_authority 1 (phlo_tree_nodes tree ++ node :: rest) = None.
Proof.
  intros. rewrite (phlo_tree_scan_consumes_exactly_one_slot tree 0). reflexivity.
Qed.

Record phlo_resource_record := {
  resource_record_location : list nat;
  resource_record_class : nat;
  resource_record_terms : list nat;
  resource_record_authority : list phlo_authority_node
}.

Definition phlo_resource_fields domain width count_width record :=
  [domain; resource_record_location record;
   phlo_word count_width (resource_record_class record); resource_record_terms record;
   phlo_authority_bytes width count_width (resource_record_authority record)].

Definition phlo_resource_bytes domain width count_width record :=
  phlo_fields width (phlo_resource_fields domain width count_width record).

Definition phlo_resource_fits domain width count_width maximum record :=
  resource_record_class record < 256 ^ count_width /\
  Forall (fun node => phlo_fields_fit width maximum (phlo_node_fields node))
    (resource_record_authority record) /\
  phlo_fields_fit width maximum (phlo_authority_fields width count_width (resource_record_authority record)) /\
  phlo_fields_fit width maximum (phlo_resource_fields domain width count_width record).

Theorem phlo_resource_encoding_binds_every_component : forall domain width count_width maximum first second,
  0 < width -> phlo_resource_fits domain width count_width maximum first ->
  phlo_resource_fits domain width count_width maximum second ->
  phlo_resource_bytes domain width count_width first = phlo_resource_bytes domain width count_width second ->
  first = second.
Proof.
  intros domain width count_width maximum first second positive
    [first_class [first_nodes [first_authority first_fields]]]
    [second_class [second_nodes [second_authority second_fields]]] same.
  unfold phlo_resource_bytes in same.
  apply (phlo_field_sequences_cannot_alias width maximum) in same; auto.
  destruct first as [location class terms authority].
  destruct second as [other_location other_class other_terms other_authority].
  cbn [phlo_resource_fields resource_record_location resource_record_class resource_record_terms
    resource_record_authority] in *.
  injection same as locations classes terms_equal authorities.
  apply phlo_word_encoding_is_injective in classes; auto.
  assert (authority = other_authority) by (eapply phlo_authority_encoding_preserves_every_node; eauto).
  subst. reflexivity.
Qed.

Print Assumptions phlo_node_encoding_is_injective.
Print Assumptions phlo_node_map_is_injective.
Print Assumptions phlo_authority_encoding_preserves_every_node.
Print Assumptions phlo_tree_scan_consumes_exactly_one_slot.
Print Assumptions phlo_tree_encoding_has_complete_shape.
Print Assumptions phlo_tree_encoding_rejects_additional_roots.
Print Assumptions phlo_resource_encoding_binds_every_component.
Print Assumptions phlo_authority_scan_append.
Print Assumptions phlo_complete_authority_has_no_accepted_extension.
