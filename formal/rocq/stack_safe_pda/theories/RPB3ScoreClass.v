From Stdlib Require Import Arith List.
From StackSafePDA Require Import RPB3DescriptorProjectionData RPB3SemanticReducer.
Import ListNotations.

Set Implicit Arguments.

Inductive score_class :=
| ClassPar
| ClassProcessComponent
| ClassExprComponent
| ClassCostComposite
| ClassCostPart
| ClassBindPayload
| ClassNameComponent
| ClassPathMapPayload
| ClassExprPayload
| ClassParentCarrier
| ClassNoStandalone
| ClassOpaque
| Unclassified.

Definition score_class_rank (class : score_class) : nat :=
  match class with
  | ClassPar => 0
  | ClassProcessComponent => 1
  | ClassExprComponent => 2
  | ClassCostComposite => 3
  | ClassCostPart => 4
  | ClassBindPayload => 5
  | ClassNameComponent => 6
  | ClassPathMapPayload => 7
  | ClassExprPayload => 8
  | ClassParentCarrier => 9
  | ClassNoStandalone => 10
  | ClassOpaque => 11
  | Unclassified => 12
  end.

Definition score_class_eqb (left right : score_class) : bool :=
  Nat.eqb (score_class_rank left) (score_class_rank right).

Definition expr_payload_type_ids : list nat :=
  [6; 27; 28; 29; 30; 33] ++ seq 35 20.

Definition score_class_of (type_id : nat) : score_class :=
  if Nat.eqb type_id 1 then ClassPar
  else if listed_score_type type_id [8; 15; 19; 20; 22; 23]
  then ClassProcessComponent
  else if Nat.eqb type_id 24 then ClassExprComponent
  else if Nat.eqb type_id 13 then ClassCostComposite
  else if listed_score_type type_id [9; 10; 11; 12] then ClassCostPart
  else if listed_score_type type_id [16; 21] then ClassBindPayload
  else if listed_score_type type_id [55; 60] then ClassNameComponent
  else if listed_score_type type_id [31; 32] then ClassPathMapPayload
  else if listed_score_type type_id expr_payload_type_ids
  then ClassExprPayload
  else if listed_score_type type_id [34; 56; 57] then ClassParentCarrier
  else if listed_score_type type_id [7; 25; 26; 61; 62; 63; 64]
  then ClassNoStandalone
  else if listed_score_type type_id [2; 3; 4; 5; 14; 17; 18; 58; 59]
  then ClassOpaque
  else Unclassified.

Definition generated_type_ids : list nat :=
  map RPB3DescriptorProjectionData.GeneratedData.program_type_id
    RPB3DescriptorProjectionData.GeneratedData.generated_programs.

Definition unclassified_ids_of (classify : nat -> score_class) : list nat :=
  filter (fun type_id =>
    match classify type_id with
    | Unclassified => true
    | _ => false
    end) generated_type_ids.

Definition unclassified_types : list nat :=
  unclassified_ids_of score_class_of.

Definition classified_ids (class : score_class) : list nat :=
  filter (fun type_id => score_class_eqb (score_class_of type_id) class)
    generated_type_ids.

Definition contract_consistent_with_class (type_id : nat) : bool :=
  match generated_score_contract_for type_id, score_class_of type_id with
  | RequiredComponentScore, ClassProcessComponent
  | RequiredComponentScore, ClassExprComponent
  | RequiredComponentScore, ClassCostComposite
  | RequiredComponentScore, ClassNameComponent
  | OptionalPayloadScore, ClassBindPayload
  | OptionalPayloadScore, ClassPathMapPayload
  | OptionalPayloadScore, ClassExprPayload
  | NoStandaloneScore, ClassParentCarrier
  | NoStandaloneScore, ClassNoStandalone
  | OtherGeneratedType, ClassPar
  | OtherGeneratedType, ClassCostPart
  | OtherGeneratedType, ClassPathMapPayload
  | OtherGeneratedType, ClassExprPayload
  | OtherGeneratedType, ClassOpaque => true
  | _, _ => false
  end.

Definition score_class_without_28 (type_id : nat) : score_class :=
  if Nat.eqb type_id 28 then Unclassified else score_class_of type_id.

Theorem generated_type_ids_are_the_interval :
  generated_type_ids = seq 1 64.
Proof. vm_compute. reflexivity. Qed.

Theorem score_class_total :
  length generated_type_ids = 64 /\
  NoDup generated_type_ids /\
  unclassified_types = [].
Proof.
  split.
  - rewrite generated_type_ids_are_the_interval.
    apply length_seq.
  - split.
    + rewrite generated_type_ids_are_the_interval.
      apply seq_NoDup.
    + vm_compute. reflexivity.
Qed.

Theorem score_class_partition_sizes :
  map (fun class => length (classified_ids class))
    [ClassPar; ClassProcessComponent; ClassExprComponent; ClassCostComposite;
     ClassCostPart; ClassBindPayload; ClassNameComponent; ClassPathMapPayload;
     ClassExprPayload; ClassParentCarrier; ClassNoStandalone; ClassOpaque;
     Unclassified] =
  [1; 6; 1; 1; 4; 2; 2; 2; 26; 3; 7; 9; 0].
Proof. vm_compute. reflexivity. Qed.

Theorem score_class_expr_payload_ids :
  classified_ids ClassExprPayload = expr_payload_type_ids.
Proof. vm_compute. reflexivity. Qed.

Theorem score_class_agrees_with_generated_contract :
  forall type_id,
    In type_id generated_type_ids ->
    contract_consistent_with_class type_id = true.
Proof.
  apply forallb_forall.
  vm_compute.
  reflexivity.
Qed.

Theorem score_class_mutant_reports_the_missing_type :
  unclassified_ids_of score_class_without_28 = [28].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions generated_type_ids_are_the_interval.
Print Assumptions score_class_total.
Print Assumptions score_class_partition_sizes.
Print Assumptions score_class_expr_payload_ids.
Print Assumptions score_class_agrees_with_generated_contract.
Print Assumptions score_class_mutant_reports_the_missing_type.
