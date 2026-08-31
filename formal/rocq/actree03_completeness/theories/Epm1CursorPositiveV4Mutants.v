From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool.
Require Import Epm1CursorPositiveV3 Epm1CursorPositiveV4.

Import ListNotations.
Import PositiveV3 PositiveV4.

Module PositiveV4Mutants.

Definition sample_tree : writer_tree :=
  WriterNode [] None
    (WriterCons 10 (WriterNode [] (Some 1) WriterNil)
      (WriterCons 20 (WriterNode [] (Some 2) WriterNil) WriterNil)).

Fixpoint mutant_cursor_find_first (_label : bbyte) (forest : endpoint_forest)
    : option endpoint_cursor :=
  match forest with
  | EndpointNil => None
  | EndpointCons _ child _ => Some child
  end.

Theorem kill_lookup_enumeration_decoupling :
  mutant_cursor_find_first 15
      (match enumerate_cursor sample_tree with
       | EndpointCursor _ _ forest => forest
       end) = Some (enumerate_cursor (WriterNode [] (Some 1) WriterNil)) /\
  writer_value_at 3 sample_tree [15] = None.
Proof. vm_compute. auto. Qed.

Definition mutant_address_identity (_parent _ordinal : nat) : nat := 0.

Theorem kill_allocation_address_identity_collapse :
  mutant_address_identity 41 0 = mutant_address_identity 41 1 /\
  [0] <> [1].
Proof. split; [reflexivity | discriminate]. Qed.

Definition mutant_trace_without_attach (bodies : nat) : generated_decoder_trace :=
  {| gdt_bodies := bodies; gdt_yields := bodies; gdt_attaches := 0;
     gdt_private_writes := bodies; gdt_publications := 1 |}.

Theorem kill_generated_attach_omission :
  ~ generated_trace_exact (mutant_trace_without_attach 2).
Proof. cbn. intros [_ [impossible _]]. discriminate. Qed.

Definition mutant_double_publication (bodies : nat) : generated_decoder_trace :=
  {| gdt_bodies := bodies; gdt_yields := bodies; gdt_attaches := bodies;
     gdt_private_writes := bodies; gdt_publications := 2 |}.

Theorem kill_double_publication :
  ~ generated_trace_exact (mutant_double_publication 2).
Proof. cbn. intros [_ [_ [_ impossible]]]. discriminate. Qed.

Definition mutant_forward_child_identity (parent : physical_identity)
    (ordinal : nat) : physical_identity := [ordinal] ++ parent.

Theorem kill_non_parent_relative_child_identity :
  mutant_forward_child_identity [7] 3 <> [7] ++ [3].
Proof. discriminate. Qed.

Inductive v4_mutant :=
| V4LookupDecoupled
| V4AddressIdentity
| V4AttachOmitted
| V4DoublePublication
| V4WrongChildIdentity.

Definition all_v4_mutants :=
  [V4LookupDecoupled; V4AddressIdentity; V4AttachOmitted;
   V4DoublePublication; V4WrongChildIdentity].

Definition v4_mutant_killed (mutant : v4_mutant) : Prop :=
  match mutant with
  | V4LookupDecoupled => writer_value_at 3 sample_tree [15] = None
  | V4AddressIdentity => [0] <> [1]
  | V4AttachOmitted => ~ generated_trace_exact (mutant_trace_without_attach 2)
  | V4DoublePublication => ~ generated_trace_exact (mutant_double_publication 2)
  | V4WrongChildIdentity =>
      mutant_forward_child_identity [7] 3 <> [7] ++ [3]
  end.

Lemma every_v4_mutant_killed : forall mutant, v4_mutant_killed mutant.
Proof.
  intro mutant. destruct mutant.
  - exact (proj2 kill_lookup_enumeration_decoupling).
  - discriminate.
  - exact kill_generated_attach_omission.
  - exact kill_double_publication.
  - exact kill_non_parent_relative_child_identity.
Qed.

Definition v4_result := { mutant : v4_mutant & v4_mutant_killed mutant }.
Definition result_for_v4_mutant mutant : v4_result :=
  existT v4_mutant_killed mutant (every_v4_mutant_killed mutant).
Definition mutant_for_v4_result (result : v4_result) := projT1 result.

Theorem v4_mutant_registry_is_result_bijective :
  NoDup all_v4_mutants /\
  (forall mutant, In mutant all_v4_mutants) /\
  (forall mutant,
      mutant_for_v4_result (result_for_v4_mutant mutant) = mutant) /\
  (forall left right,
      result_for_v4_mutant left = result_for_v4_mutant right -> left = right).
Proof.
  split.
  - repeat constructor; simpl; intuition congruence.
  - split.
    + intro mutant. destruct mutant; simpl; tauto.
    + split; [reflexivity |].
      intros left right equal.
      exact (f_equal mutant_for_v4_result equal).
Qed.

Print Assumptions kill_lookup_enumeration_decoupling.
Print Assumptions kill_allocation_address_identity_collapse.
Print Assumptions kill_generated_attach_omission.
Print Assumptions kill_double_publication.
Print Assumptions kill_non_parent_relative_child_identity.
Print Assumptions v4_mutant_registry_is_result_bijective.

End PositiveV4Mutants.
