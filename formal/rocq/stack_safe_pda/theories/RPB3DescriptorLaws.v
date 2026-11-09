From Stdlib Require Import List Bool Arith String Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData.

Import ListNotations.
Import GeneratedData.
Open Scope string_scope.
Open Scope nat_scope.

Record tag_binding := TagBinding {
  binding_tag : nat;
  binding_slot : nat;
  binding_arm : option projection_arm
}.

Definition slot_bindings (slot : projection_slot) : list tag_binding :=
  match slot_field_kind slot with
  | FKOneof => map (fun arm => TagBinding (arm_tag arm) (slot_index slot) (Some arm))
                   (slot_arms slot)
  | _ => [TagBinding (slot_min_tag slot) (slot_index slot) None]
  end.

Definition program_bindings (program : projection_program) : list tag_binding :=
  List.concat (map slot_bindings (program_slots program)).

Definition resolve_actual_tag (program : projection_program) (tag : nat)
    : option tag_binding :=
  find (fun binding => Nat.eqb tag (binding_tag binding))
       (program_bindings program).

Fixpoint unique_nat (values : list nat) : bool :=
  match values with
  | [] => true
  | head :: tail => negb (existsb (Nat.eqb head) tail) && unique_nat tail
  end.

Lemma unique_nat_sound : forall values,
  unique_nat values = true -> NoDup values.
Proof.
  induction values as [|head tail IH]; simpl; intro checked.
  - constructor.
  - apply andb_true_iff in checked as [absent remaining].
    constructor.
    + intro present.
      assert (existsb (Nat.eqb head) tail = true) as found.
      { apply existsb_exists. exists head. split.
        - exact present.
        - apply Nat.eqb_refl. }
      rewrite found in absent. discriminate.
    + apply IH. exact remaining.
Qed.

Definition wire_kind_eqb (left right : wire_kind) : bool :=
  match left, right with
  | WTVarint, WTVarint | WTFixed64, WTFixed64
  | WTLengthDelimited, WTLengthDelimited | WTFixed32, WTFixed32 => true
  | _, _ => false
  end.

Definition optional_wire_eqb (left right : option wire_kind) : bool :=
  match left, right with
  | None, None => true
  | Some lhs, Some rhs => wire_kind_eqb lhs rhs
  | _, _ => false
  end.

Definition protobuf_kind_eq_dec (lhs rhs : protobuf_kind)
    : {lhs = rhs} + {lhs <> rhs}.
Proof. decide equality. Defined.

Definition protobuf_kind_eqb (left right : protobuf_kind) : bool :=
  if protobuf_kind_eq_dec left right then true else false.

Definition default_policy_eq_dec (lhs rhs : default_policy)
    : {lhs = rhs} + {lhs <> rhs}.
Proof. decide equality. Defined.

Definition default_policy_eqb (left right : default_policy) : bool :=
  if default_policy_eq_dec left right then true else false.

Definition expected_wire (kind : protobuf_kind) : option wire_kind :=
  match kind with
  | PBBool | PBInt32 | PBSint32 | PBUint32 | PBInt64 | PBSint64
  | PBUint64 => Some WTVarint
  | PBSfixed64 | PBFixed64 | PBDouble => Some WTFixed64
  | PBString | PBBytes | PBRepeatedString | PBRepeatedBytes | PBMessage
  | PBRepeatedMessage | PBMapStringMessage | PBEPathMapSnapshot =>
      Some WTLengthDelimited
  | PBSfixed32 | PBFixed32 | PBFloat => Some WTFixed32
  | PBOneof => None
  end.

Definition child_id_valid (child : option nat) : bool :=
  match child with
  | None => true
  | Some id => (0 <? id) &&
      existsb (fun program => Nat.eqb id (program_type_id program))
              generated_programs
  end.

Definition message_child_required (child : option nat) : bool :=
  match child with Some _ => child_id_valid child | None => false end.

Definition child_absent (child : option nat) : bool :=
  match child with None => true | Some _ => false end.

Definition field_kind_compatible (kind : field_kind) (proto : protobuf_kind)
    : bool :=
  match kind, proto with
  | FKBool, PBBool
  | FKI32, PBInt32 | FKI32, PBSint32 | FKI32, PBSfixed32
  | FKU32, PBUint32 | FKU32, PBFixed32
  | FKI64, PBInt64 | FKI64, PBSint64 | FKI64, PBSfixed64
  | FKU64, PBUint64 | FKU64, PBFixed64
  | FKBytes, PBBytes | FKString, PBString
  | FKStringSequence, PBRepeatedString
  | FKBytesSequence, PBRepeatedBytes
  | FKOptionalMessage, PBMessage
  | FKMessageSequence, PBRepeatedMessage
  | FKStringMessageMap, PBMapStringMessage
  | FKOneof, PBOneof | FKPathMap, PBEPathMapSnapshot => true
  | _, _ => false
  end.

Definition payload_kind_compatible (payload : payload_kind)
    (proto : protobuf_kind) : bool :=
  match payload, proto with
  | PKBool, PBBool
  | PKI32, PBInt32 | PKI32, PBSint32 | PKI32, PBSfixed32
  | PKU32, PBUint32 | PKU32, PBFixed32
  | PKI64, PBInt64 | PKI64, PBSint64 | PKI64, PBSfixed64
  | PKU64, PBUint64 | PKU64, PBFixed64
  | PKBytes, PBBytes | PKString, PBString
  | PKMessage, PBMessage => true
  | _, _ => false
  end.

Definition expected_repeated (kind : field_kind) : bool :=
  match kind with
  | FKStringSequence | FKBytesSequence | FKMessageSequence
  | FKStringMessageMap => true
  | _ => false
  end.

Definition expected_default (kind : field_kind) : default_policy :=
  match kind with
  | FKBool | FKI32 | FKU32 | FKI64 | FKU64 | FKBytes | FKString =>
      DefaultForbidden
  | FKStringSequence | FKBytesSequence | FKOneof => DefaultAllowed
  | _ => DefaultNotScalar
  end.

Definition field_child_wf (slot : projection_slot) : bool :=
  match slot_field_kind slot with
  | FKOptionalMessage | FKMessageSequence | FKStringMessageMap =>
      message_child_required (slot_child_id slot)
  | _ => child_absent (slot_child_id slot)
  end.

Definition arm_well_formed (arm : projection_arm) : bool :=
  (0 <? arm_tag arm) &&
  payload_kind_compatible (arm_payload arm) (arm_protobuf_kind arm) &&
  optional_wire_eqb (arm_wire arm) (expected_wire (arm_protobuf_kind arm)) &&
  match arm_payload arm with
  | PKMessage => message_child_required (arm_child_id arm)
  | _ => child_absent (arm_child_id arm)
  end.

Definition arms_empty (arms : list projection_arm) : bool :=
  match arms with [] => true | _ :: _ => false end.

Definition slot_oneof_wf (slot : projection_slot) : bool :=
  match slot_field_kind slot with
  | FKOneof =>
      negb (arms_empty (slot_arms slot)) &&
      forallb (fun arm =>
        (slot_min_tag slot <=? arm_tag arm) && arm_well_formed arm)
        (slot_arms slot) &&
      existsb (fun arm => Nat.eqb (slot_min_tag slot) (arm_tag arm))
        (slot_arms slot)
  | _ => arms_empty (slot_arms slot)
  end.

Definition slot_well_formed (slot : projection_slot) : bool :=
  field_kind_compatible (slot_field_kind slot) (slot_protobuf_kind slot) &&
  optional_wire_eqb (slot_wire slot) (expected_wire (slot_protobuf_kind slot)) &&
  Bool.eqb (slot_repeated slot) (expected_repeated (slot_field_kind slot)) &&
  default_policy_eqb (slot_default slot) (expected_default (slot_field_kind slot)) &&
  field_child_wf slot && slot_oneof_wf slot.

Definition slot_names_well_formed (name : string) (slot : projection_slot) : bool :=
  if String.eqb name "EPathMap" && Nat.eqb (slot_min_tag slot) 9 then
    String.eqb (slot_name slot) "entries" &&
    String.eqb (slot_protobuf_name slot) "trie_snapshot"
  else String.eqb (slot_name slot) (slot_protobuf_name slot).

Fixpoint ordered_slots_from (index previous_tag : nat)
    (slots : list projection_slot) : bool :=
  match slots with
  | [] => true
  | slot :: rest =>
      Nat.eqb (slot_index slot) index &&
      (previous_tag <? slot_min_tag slot) &&
      slot_well_formed slot &&
      ordered_slots_from (S index) (slot_min_tag slot) rest
  end.

Definition program_well_formed (program : projection_program) : bool :=
  (0 <? program_type_id program) &&
  ordered_slots_from 0 0 (program_slots program) &&
  forallb (slot_names_well_formed (program_name program)) (program_slots program) &&
  unique_nat (map binding_tag (program_bindings program)).

Fixpoint contiguous_programs_from (next_id : nat)
    (programs : list projection_program) : bool :=
  match programs with
  | [] => true
  | program :: rest =>
      Nat.eqb (program_type_id program) next_id &&
      contiguous_programs_from (S next_id) rest
  end.

Definition exactly_one_raw_par : bool :=
  match generated_programs with
  | [] => false
  | first :: rest =>
      program_is_raw_par first &&
      String.eqb (program_name first) "Par" &&
      forallb (fun program => negb (program_is_raw_par program)) rest
  end.

Definition descriptor_table_check : bool :=
  (60 <=? List.length generated_programs) &&
  contiguous_programs_from 1 generated_programs &&
  unique_nat (map program_type_id generated_programs) &&
  exactly_one_raw_par &&
  forallb program_well_formed generated_programs.

Theorem descriptor_table_well_formed : descriptor_table_check = true.
Proof. vm_compute. reflexivity. Qed.

Theorem raw_par_substituted :
  nth_error generated_programs 0 = Some raw_par_program /\
  program_is_raw_par raw_par_program = true /\
  10 <= List.length (program_slots raw_par_program).
Proof. vm_compute. repeat split; try reflexivity; lia. Qed.

Definition all_generated_arms : list projection_arm :=
  List.concat (map (fun program =>
    List.concat (map slot_arms (program_slots program))) generated_programs).

Theorem descriptor_table_nonvacuous :
  60 <= List.length generated_programs /\ 0 < List.length all_generated_arms /\
  10 <= List.length (program_slots raw_par_program).
Proof. vm_compute. repeat split; try reflexivity; lia. Qed.

Lemma generated_program_well_formed : forall program,
  In program generated_programs -> program_well_formed program = true.
Proof.
  intros program present.
  pose proof descriptor_table_well_formed as checked.
  unfold descriptor_table_check in checked.
  apply andb_true_iff in checked as [_ checked].
  exact ((proj1 (@forallb_forall projection_program program_well_formed
                   generated_programs) checked) program present).
Qed.

Lemma generated_program_tags_unique : forall program,
  In program generated_programs ->
  NoDup (map binding_tag (program_bindings program)).
Proof.
  intros program present.
  pose proof (generated_program_well_formed program present) as checked.
  unfold program_well_formed in checked.
  apply andb_true_iff in checked as [_ checked].
  apply unique_nat_sound. exact checked.
Qed.

Lemma unique_binding_at_tag : forall bindings left right,
  NoDup (map binding_tag bindings) ->
  In left bindings -> In right bindings ->
  binding_tag left = binding_tag right -> left = right.
Proof.
  induction bindings as [|head tail IH]; intros left right nodup in_left in_right tags.
  - contradiction.
  - simpl in nodup. inversion nodup as [|? ? absent tail_nodup]; subst.
    simpl in in_left, in_right.
    destruct in_left as [left_head | left_tail];
      destruct in_right as [right_head | right_tail].
    + congruence.
    + subst left. exfalso. apply absent. rewrite tags.
      apply in_map. exact right_tail.
    + subst right. exfalso. apply absent. rewrite <- tags.
      apply in_map. exact left_tail.
    + apply IH; assumption.
Qed.

Theorem tag_binding_unique : forall program left right,
  In program generated_programs ->
  In left (program_bindings program) ->
  In right (program_bindings program) ->
  binding_tag left = binding_tag right -> left = right.
Proof.
  intros program left right present in_left in_right tags.
  eapply unique_binding_at_tag; eauto using generated_program_tags_unique.
Qed.

Lemma find_binding_sound : forall bindings tag binding,
  find (fun candidate => Nat.eqb tag (binding_tag candidate)) bindings =
    Some binding ->
  In binding bindings /\ binding_tag binding = tag.
Proof.
  induction bindings as [|head tail IH]; intros tag binding found; simpl in found.
  - discriminate.
  - destruct (Nat.eqb tag (binding_tag head)) eqn:matches.
    + inversion found; subst. split.
      * left. reflexivity.
      * apply Nat.eqb_eq in matches. symmetry. exact matches.
    + apply IH in found as [present equal].
      split; [right; exact present | exact equal].
Qed.

Lemma resolve_actual_tag_sound : forall program tag binding,
  resolve_actual_tag program tag = Some binding ->
  In binding (program_bindings program) /\ binding_tag binding = tag.
Proof.
  intros program tag binding found.
  unfold resolve_actual_tag in found.
  eapply find_binding_sound. exact found.
Qed.

Theorem resolved_tag_is_the_only_binding : forall program tag binding other,
  In program generated_programs ->
  resolve_actual_tag program tag = Some binding ->
  In other (program_bindings program) ->
  binding_tag other = tag -> other = binding.
Proof.
  intros program tag binding other present found other_in other_tag.
  apply resolve_actual_tag_sound in found as [binding_in binding_tag_eq].
  eapply tag_binding_unique with (program := program).
  - exact present.
  - exact other_in.
  - exact binding_in.
  - congruence.
Qed.

Print Assumptions descriptor_table_well_formed.
Print Assumptions raw_par_substituted.
Print Assumptions descriptor_table_nonvacuous.
Print Assumptions tag_binding_unique.
Print Assumptions resolved_tag_is_the_only_binding.
