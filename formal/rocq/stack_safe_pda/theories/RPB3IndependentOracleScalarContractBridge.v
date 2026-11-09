From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction
  RPB3IndependentOracleAdmission.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Scalar := RPB3SourceScalarReduction.
Module Admission := RPB3IndependentOracleAdmission.

(** The scalar reducer [Scalar.generated_scalar_rule] and the independent
    oracle [Admission.source_scalar_contract_for] read the same generated row,
    but they search its arms in different orders: the reducer drops message
    arms before it compares tags, while the oracle compares tags first and
    only then rejects a message arm.  Disagreement therefore requires a slot
    with a duplicated arm tag, and agreement needs exactly one fact about the
    finite table: arm tags are distinct inside every slot. *)
Fixpoint arm_tags_distinct (arms : list Data.projection_arm) : bool :=
  match arms with
  | [] => true
  | arm :: rest =>
      negb (existsb (fun other => Data.arm_tag other =? Data.arm_tag arm) rest)
        && arm_tags_distinct rest
  end.

Definition all_generated_arm_tags_are_distinct : bool :=
  forallb (fun row =>
    forallb (fun slot => arm_tags_distinct (Data.slot_arms slot))
      (Data.program_slots row))
    Data.generated_programs.

Theorem finite_generated_arm_tags_are_distinct :
  all_generated_arm_tags_are_distinct = true.
Proof. vm_compute. reflexivity. Qed.

(** The reducer and the oracle are compared on the only data the admission
    decision reads from either: the protobuf kind and the default-policy bit. *)
Definition rule_signature (rule : Scalar.scalar_rule)
    : Data.protobuf_kind * bool :=
  (Scalar.scalar_kind rule, Scalar.scalar_default_forbidden rule).

Definition contract_signature (contract : Admission.source_scalar_contract)
    : Data.protobuf_kind * bool :=
  (Admission.source_contract_kind contract,
    Admission.source_contract_default_forbidden contract).

(** The per-arm rule generator that [Scalar.slot_scalar_rules] writes inline,
    named so the arm-list induction can state its lemma about it. *)
Definition arm_scalar_rules (arm : Data.projection_arm)
    : list Scalar.scalar_rule :=
  match Data.arm_wire arm, Data.arm_payload arm with
  | Some _, Data.PKMessage => []
  | Some _, _ =>
      [Scalar.ScalarRule (Data.arm_tag arm)
        (Data.arm_protobuf_kind arm) false]
  | None, _ => []
  end.

(** [find] over an append searches the front first. *)
Lemma find_over_append :
  forall (A : Type) (matches : A -> bool) (front back : list A),
    find matches (front ++ back) =
    match find matches front with
    | Some found => Some found
    | None => find matches back
    end.
Proof.
  intros A matches front back.
  induction front as [|head tail IH]; simpl; [reflexivity|].
  destruct (matches head); [reflexivity|exact IH].
Qed.

Lemma find_over_append_with_missing_front :
  forall (A : Type) (matches : A -> bool) (front back : list A),
    find matches front = None ->
    find matches (front ++ back) = find matches back.
Proof.
  intros A matches front back missing.
  rewrite find_over_append, missing; reflexivity.
Qed.

Lemma find_over_append_with_missing_back :
  forall (A : Type) (matches : A -> bool) (front back : list A),
    find matches back = None ->
    find matches (front ++ back) = find matches front.
Proof.
  intros A matches front back missing.
  rewrite find_over_append.
  destruct (find matches front) as [found|]; [reflexivity|exact missing].
Qed.

Lemma arm_scalar_rules_miss_other_tags :
  forall arm tag,
    (Data.arm_tag arm =? tag) = false ->
    find (fun rule => Scalar.scalar_tag rule =? tag)
      (arm_scalar_rules arm) = None.
Proof.
  intros arm tag different.
  unfold arm_scalar_rules.
  destruct (Data.arm_wire arm) as [wire|]; [|reflexivity].
  destruct (Data.arm_payload arm); simpl;
    try reflexivity; rewrite different; reflexivity.
Qed.

Lemma flat_arm_rules_miss_unused_tag :
  forall tag arms,
    existsb (fun arm => Data.arm_tag arm =? tag) arms = false ->
    find (fun rule => Scalar.scalar_tag rule =? tag)
      (flat_map arm_scalar_rules arms) = None.
Proof.
  intros tag arms.
  induction arms as [|arm rest IH]; intros unused; [reflexivity|].
  simpl in unused.
  apply orb_false_iff in unused as [different absent].
  change (flat_map arm_scalar_rules (arm :: rest)) with
    (arm_scalar_rules arm ++ flat_map arm_scalar_rules rest).
  rewrite find_over_append_with_missing_front
    by exact (arm_scalar_rules_miss_other_tags arm tag different).
  exact (IH absent).
Qed.

(** On its own tag, an arm's rule list (at most one rule) carries exactly the
    arm's contract: a message arm or a wireless arm yields neither. *)
Lemma arm_rule_agrees_with_contract :
  forall arm,
    option_map rule_signature
      (find (fun rule => Scalar.scalar_tag rule =? Data.arm_tag arm)
        (arm_scalar_rules arm)) =
    option_map contract_signature (Admission.source_contract_in_arm arm).
Proof.
  intro arm.
  unfold arm_scalar_rules, Admission.source_contract_in_arm,
    Schema.projected_arm_payload.
  destruct (Data.arm_wire arm) as [wire|]; [|reflexivity].
  destruct (Data.arm_payload arm); simpl;
    rewrite ?Nat.eqb_refl; reflexivity.
Qed.

(** The rule search over a whole arm list agrees with the oracle's tag search.
    Distinctness is used once, on the tag-hit branch: after the head arm with
    the tag, no later arm may contribute a rule the oracle would not see. *)
Lemma arm_rules_agree_with_contract_search :
  forall arms tag,
    arm_tags_distinct arms = true ->
    option_map rule_signature
      (find (fun rule => Scalar.scalar_tag rule =? tag)
        (flat_map arm_scalar_rules arms)) =
    option_map contract_signature
      (match find (fun arm => Data.arm_tag arm =? tag) arms with
       | Some arm => Admission.source_contract_in_arm arm
       | None => None
       end).
Proof.
  intros arms tag.
  induction arms as [|arm rest IH]; intros distinct; [reflexivity|].
  simpl in distinct.
  apply andb_true_iff in distinct as [head_tag_unused distinct_rest].
  apply negb_true_iff in head_tag_unused.
  change (flat_map arm_scalar_rules (arm :: rest)) with
    (arm_scalar_rules arm ++ flat_map arm_scalar_rules rest).
  change (find (fun candidate => Data.arm_tag candidate =? tag) (arm :: rest))
    with (if Data.arm_tag arm =? tag then Some arm
          else find (fun candidate => Data.arm_tag candidate =? tag) rest).
  destruct (Data.arm_tag arm =? tag) eqn:tag_hit.
  - apply Nat.eqb_eq in tag_hit.
    subst tag.
    rewrite find_over_append_with_missing_back
      by exact (flat_arm_rules_miss_unused_tag (Data.arm_tag arm) rest
        head_tag_unused).
    exact (arm_rule_agrees_with_contract arm).
  - rewrite find_over_append_with_missing_front
      by exact (arm_scalar_rules_miss_other_tags arm tag tag_hit).
    exact (IH distinct_rest).
Qed.

Lemma slot_rules_agree_with_contract_search :
  forall slot tag,
    arm_tags_distinct (Data.slot_arms slot) = true ->
    option_map rule_signature
      (find (fun rule => Scalar.scalar_tag rule =? tag)
        (Scalar.slot_scalar_rules slot)) =
    option_map contract_signature
      (Admission.source_contract_in_slot slot tag).
Proof.
  intros slot tag distinct.
  revert distinct.
  unfold Scalar.slot_scalar_rules, Admission.source_contract_in_slot.
  destruct (Data.slot_arms slot) as [|first rest]; intros distinct.
  - destruct (Data.slot_wire slot) as [wire|];
      [destruct (Schema.projected_slot_payload slot)|];
      simpl; destruct (Data.slot_min_tag slot =? tag); reflexivity.
  - exact (arm_rules_agree_with_contract_search (first :: rest) tag distinct).
Qed.

Lemma slot_list_rules_agree_with_contract_search :
  forall slots tag,
    forallb (fun slot => arm_tags_distinct (Data.slot_arms slot)) slots = true ->
    option_map rule_signature
      (find (fun rule => Scalar.scalar_tag rule =? tag)
        (flat_map Scalar.slot_scalar_rules slots)) =
    option_map contract_signature
      (Admission.source_contract_in_slots slots tag).
Proof.
  intros slots tag.
  induction slots as [|slot rest IH]; intros distinct; [reflexivity|].
  simpl in distinct.
  apply andb_true_iff in distinct as [slot_distinct rest_distinct].
  specialize (IH rest_distinct).
  pose proof (slot_rules_agree_with_contract_search slot tag slot_distinct)
    as slot_agrees.
  change (flat_map Scalar.slot_scalar_rules (slot :: rest)) with
    (Scalar.slot_scalar_rules slot ++ flat_map Scalar.slot_scalar_rules rest).
  change (Admission.source_contract_in_slots (slot :: rest) tag) with
    (match Admission.source_contract_in_slot slot tag with
     | Some contract => Some contract
     | None => Admission.source_contract_in_slots rest tag
     end).
  rewrite find_over_append.
  revert slot_agrees.
  destruct (find (fun rule => Scalar.scalar_tag rule =? tag)
      (Scalar.slot_scalar_rules slot)) as [rule|];
    destruct (Admission.source_contract_in_slot slot tag) as [contract|];
    intro slot_agrees;
    [exact slot_agrees
    |discriminate slot_agrees
    |discriminate slot_agrees
    |exact IH].
Qed.

Lemma every_generated_row_has_distinct_arm_tags :
  forall row,
    In row Data.generated_programs ->
    forallb (fun slot => arm_tags_distinct (Data.slot_arms slot))
      (Data.program_slots row) = true.
Proof.
  intros row member.
  pose proof finite_generated_arm_tags_are_distinct as certificate.
  unfold all_generated_arm_tags_are_distinct in certificate.
  rewrite forallb_forall in certificate.
  exact (certificate row member).
Qed.

Lemma row_rules_agree_with_contract_search :
  forall row tag,
    In row Data.generated_programs ->
    option_map rule_signature
      (find (fun rule => Scalar.scalar_tag rule =? tag)
        (flat_map Scalar.slot_scalar_rules (Data.program_slots row))) =
    option_map contract_signature
      (Admission.source_contract_in_slots (Data.program_slots row) tag).
Proof.
  intros row tag member.
  apply slot_list_rules_agree_with_contract_search.
  exact (every_generated_row_has_distinct_arm_tags row member).
Qed.

Theorem generated_scalar_rule_matches_source_contract :
  forall type_id program tag,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    option_map (fun r => (Scalar.scalar_kind r, Scalar.scalar_default_forbidden r))
      (Scalar.generated_scalar_rule program tag) =
    option_map (fun c => (Admission.source_contract_kind c,
                          Admission.source_contract_default_forbidden c))
      (Admission.source_scalar_contract_for type_id tag).
Proof.
  intros type_id program tag looked_up.
  unfold Schema.lookup_projected_program in looked_up.
  destruct (find (fun row => Data.program_type_id row =? type_id)
      Data.generated_programs) as [row|] eqn:selected;
    [|discriminate].
  assert (rebuilt : Some (Schema.project_generated_program row [] []) =
      Some program) by exact looked_up.
  injection rebuilt as program_is_row.
  subst program.
  destruct (Scalar.found_generated_row_is_member_and_matches
      type_id Data.generated_programs row selected) as [member same_id].
  assert (family : Schema.program_family
      (Schema.project_generated_program row [] []) = type_id) by exact same_id.
  unfold Scalar.generated_scalar_rule, Admission.source_scalar_contract_for.
  rewrite family, selected.
  exact (row_rules_agree_with_contract_search row tag member).
Qed.

Print Assumptions finite_generated_arm_tags_are_distinct.
Print Assumptions find_over_append.
Print Assumptions find_over_append_with_missing_front.
Print Assumptions find_over_append_with_missing_back.
Print Assumptions arm_scalar_rules_miss_other_tags.
Print Assumptions flat_arm_rules_miss_unused_tag.
Print Assumptions arm_rule_agrees_with_contract.
Print Assumptions arm_rules_agree_with_contract_search.
Print Assumptions slot_rules_agree_with_contract_search.
Print Assumptions slot_list_rules_agree_with_contract_search.
Print Assumptions every_generated_row_has_distinct_arm_tags.
Print Assumptions row_rules_agree_with_contract_search.
Print Assumptions generated_scalar_rule_matches_source_contract.
