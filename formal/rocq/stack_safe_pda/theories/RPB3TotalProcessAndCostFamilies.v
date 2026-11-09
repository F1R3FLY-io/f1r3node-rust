(** RPB3TotalProcessAndCostFamilies: leaf L16 of the rpb3 totality campaign.

    Theorem [process_and_cost_result_on_typed_valid] says the semantic reducer
    returns an acceptable result on every typed-valid node whose score class
    is Par, ProcessComponent, ExprComponent, CostComposite or CostPart.

    The structure of the proof:

    - Section A collects the elementary facts about [scan_child], the scan's
      fold and the forest.
    - Section B records, for every unrefused child, the [child_facts]
      the family proofs consume: descriptor conformance, scalar validity
      (TV2), the re-read grammar (TV3) and the class result of a message
      child.
    - Section C proves the generic totality lemmas of the [exact_*] score
      helpers from per-tag hypotheses.
    - Section D proves the wire-level facts needed for the two re-read
      families: a canonical varint and the body of GBigRational and
      GFixedPoint.
    - Section E discharges the generated tables by computation and proves the
      per-family results.  Families are never enumerated in a fixed order
      inside the Par proof: the Par characterization quantifies over the
      generated Message tags of program 1, as a list.
    - Section F is the main theorem and the red control.

    The theory imports only what it needs; nothing here reads the Rust code
    except the citations in comments (P: protobuf_admission.rs, C:
    canonical_semantics.rs, both under models/src/rust/rholang/). *)

From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import StackSafePDA RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SourceScalarReduction RPB3RawRegionTree
  RPB3SemanticReducer RPB3ScoreClass RPB3TypedValid.

Import ListNotations.

(** * A. Elementary facts *)

Lemma payload_eqb_true :
  forall left right : payload_kind, payload_eqb left right = true -> left = right.
Proof.
  intros left right equal.
  destruct left, right; cbn [payload_eqb] in equal;
    try discriminate equal; reflexivity.
Qed.

Lemma resolve_tag_tag :
  forall fields tag descriptor,
    resolve_tag fields tag = Some descriptor -> descriptor_tag descriptor = tag.
Proof.
  intros fields tag descriptor.
  induction fields as [|field rest IH]; intro resolved;
    cbn [resolve_tag] in resolved.
  - discriminate resolved.
  - destruct (descriptor_tag field =? tag) eqn:same.
    + injection resolved as same_field. subst descriptor.
      apply Nat.eqb_eq. exact same.
    + exact (IH resolved).
Qed.

(** [scan_child_stays_valid_only_if] of RPB3TypedValid, with the wire match and
    the uniqueness clause added. *)
Lemma scan_child_admissible_parts :
  forall program state child next,
    scan_child program state child = next ->
    scan_valid next = true ->
    exists descriptor,
      scan_valid state = true /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      sem_definitely_refused child = false /\
      wire_eqb (sem_wire child) (descriptor_wire descriptor) = true /\
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
      descriptor_accepts_child_type descriptor child = true /\
      (descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb (descriptor_slot descriptor))
          (scan_seen state))) = true /\
      scan_seen next = descriptor_slot descriptor :: scan_seen state.
Proof.
  intros program state child next stepped valid.
  unfold scan_child in stepped.
  destruct (scan_valid state) eqn:state_valid.
  2: { cbv beta iota in stepped.
       subst next.
       rewrite state_valid in valid.
       discriminate valid. }
  destruct (resolve_tag (program_fields program) (sem_tag child))
    as [descriptor|] eqn:resolved.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota zeta in stepped.
  match type of stepped with
  | (if ?test then _ else _) = _ => destruct test eqn:admissible
  end.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota in stepped.
  apply andb_prop in admissible as [first_six unique_ok].
  apply andb_prop in first_six as [first_five follows_ok].
  apply andb_prop in first_five as [first_four range_ok].
  apply andb_prop in first_four as [first_three type_ok].
  apply andb_prop in first_three as [first_two payload_ok].
  apply andb_prop in first_two as [not_refused wire_ok].
  subst next.
  exists descriptor.
  split; [first [exact state_valid|reflexivity]|].
  split; [first [exact resolved|reflexivity]|].
  split; [exact (proj1 (negb_true_iff _) not_refused)|].
  split; [exact wire_ok|].
  split; [exact payload_ok|].
  split; [exact type_ok|].
  split; [exact unique_ok|].
  reflexivity.
Qed.

Lemma scan_valid_fold_start :
  forall program children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    scan_valid state = true.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state valid.
  - exact valid.
  - cbn [fold_left] in valid.
    specialize (IH (scan_child program state child) valid).
    destruct (scan_child_admissible_parts program state child _ eq_refl IH)
      as [descriptor [was_valid _]].
    exact was_valid.
Qed.

Definition scan_member_facts (program : generated_program)
    (child : semantic_summary) : Prop :=
  sem_definitely_refused child = false /\
  exists descriptor,
    resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
    wire_eqb (sem_wire child) (descriptor_wire descriptor) = true /\
    payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
    descriptor_accepts_child_type descriptor child = true.

Lemma scan_fold_member_facts :
  forall program children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    Forall (scan_member_facts program) children.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state valid.
  - constructor.
  - cbn [fold_left] in valid.
    pose proof (scan_valid_fold_start program rest _ valid) as head_valid.
    destruct (scan_child_admissible_parts program state child _ eq_refl
      head_valid)
      as [descriptor [_ [resolved [not_refused [wire_ok [payload_ok
        [type_ok _]]]]]]].
    constructor.
    + split; [exact not_refused|].
      exists descriptor. repeat split; assumption.
    + exact (IH _ valid).
Qed.

Lemma fold_scan_seen_member :
  forall program slot children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    seen_slot (fold_left (scan_child program) children state) slot = true ->
    seen_slot state slot = true \/
    exists child descriptor,
      In child children /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      descriptor_slot descriptor = slot.
Proof.
  intros program slot children.
  induction children as [|child rest IH]; intros state valid seen.
  - left. exact seen.
  - cbn [fold_left] in valid, seen.
    destruct (IH _ valid seen) as [here | [found [descriptor [member
      [resolved same_slot]]]]].
    + pose proof (scan_valid_fold_start program rest _ valid) as head_valid.
      destruct (scan_child_admissible_parts program state child _ eq_refl
        head_valid)
        as [descriptor [_ [resolved [_ [_ [_ [_ [_ seen_after]]]]]]]].
      unfold seen_slot in here.
      rewrite seen_after in here.
      cbn [existsb] in here.
      apply orb_prop in here as [is_new | old].
      * right. exists child, descriptor.
        split; [left; reflexivity|].
        split; [exact resolved|].
        apply Nat.eqb_eq in is_new. symmetry. exact is_new.
      * left. exact old.
    + right. exists found, descriptor.
      split; [right; exact member|].
      split; [exact resolved|exact same_slot].
Qed.

(** [slot_holds_only program slot tag]: every descriptor at [slot] carries
    [tag].  Decidable and checked by computation on the generated tables. *)
Definition slot_holds_only (program : generated_program) (slot tag : nat) : bool :=
  forallb (fun descriptor =>
    negb (descriptor_slot descriptor =? slot) ||
    (descriptor_tag descriptor =? tag)) (program_fields program).

Lemma seen_slot_present_tag :
  forall program children slot tag,
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    seen_slot (fold_left (scan_child program) children empty_scan) slot = true ->
    slot_holds_only program slot tag = true ->
    exists child, In child children /\ sem_tag child = tag.
Proof.
  intros program children slot tag valid seen table.
  destruct (fold_scan_seen_member program slot children empty_scan valid seen)
    as [impossible | [child [descriptor [member [resolved same_slot]]]]].
  - unfold seen_slot in impossible.
    cbn [scan_seen empty_scan existsb] in impossible.
    discriminate impossible.
  - exists child.
    split; [exact member|].
    pose proof (proj1 (forallb_forall _ _) table descriptor
      (resolve_tag_member _ _ _ resolved)) as row.
    cbv beta in row.
    rewrite same_slot, Nat.eqb_refl in row.
    cbn [negb orb] in row.
    apply Nat.eqb_eq in row.
    pose proof (resolve_tag_tag _ _ _ resolved) as tagged.
    rewrite <- tagged, row.
    reflexivity.
Qed.

(** A program whose every field has the same singular (non-repeated) slot
    admits at most one child: a second child occupies a seen slot. *)
Lemma singular_program_at_most_one_child :
  forall program children,
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    (forall descriptor,
      In descriptor (program_fields program) ->
      descriptor_slot descriptor = 0 /\ descriptor_repeated descriptor = false) ->
    length children <= 1.
Proof.
  intros program children valid singular.
  destruct children as [|first [|second rest]].
  - cbn [length]. lia.
  - cbn [length]. lia.
  - exfalso.
    cbn [fold_left] in valid.
    pose proof (scan_valid_fold_start program (second :: rest) _ valid)
      as first_valid.
    pose proof (scan_valid_fold_start program rest
      (scan_child program (scan_child program empty_scan first) second)
      valid) as second_valid.
    destruct (scan_child_admissible_parts program empty_scan first _ eq_refl
      first_valid)
      as [first_descriptor [_ [first_resolved [_ [_ [_ [_ [_ first_seen]]]]]]]].
    destruct (scan_child_admissible_parts program
      (scan_child program empty_scan first) second _ eq_refl second_valid)
      as [second_descriptor [_ [second_resolved [_ [_ [_ [_ [unique _]]]]]]]].
    destruct (singular _ (resolve_tag_member _ _ _ first_resolved))
      as [first_slot _].
    destruct (singular _ (resolve_tag_member _ _ _ second_resolved))
      as [second_slot second_singular].
    rewrite second_singular in unique.
    cbn [orb] in unique.
    rewrite first_seen in unique.
    rewrite first_slot, second_slot in unique.
    cbn [existsb scan_seen empty_scan] in unique.
    rewrite Nat.eqb_refl in unique.
    cbn [negb orb] in unique.
    discriminate unique.
Qed.

(** ** Projections of [reduce_semantic_field] *)

Lemma reduce_semantic_field_tag :
  forall field children,
    sem_tag (reduce_semantic_field field children) = raw_tag field.
Proof. intros field children. reflexivity. Qed.

Lemma reduce_semantic_field_wire :
  forall field children,
    sem_wire (reduce_semantic_field field children) = raw_wire field.
Proof. intros field children. reflexivity. Qed.

Lemma reduce_semantic_field_body :
  forall field children,
    sem_body (reduce_semantic_field field children) = raw_body field.
Proof. intros field children. reflexivity. Qed.

Lemma reduce_semantic_field_evidence :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_score_evidence (reduce_semantic_field field children) =
    score_evidence_for (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) children empty_scan) children)
      children.
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

(** What an unrefused [reduce_semantic_field] exposes. *)
Lemma reduce_not_refused_parts :
  forall field children,
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    exists program,
      generated_wire_program field = Some program /\
      valid_raw_body field = true /\
      scan_valid (fold_left (scan_child program) children empty_scan) = true /\
      complete_receive_has_default_condition (program_family program)
        (component_completeness (program_family program)
          (fold_left (scan_child program) children empty_scan) children)
        children = false /\
      complete_match_has_default_guard (program_family program)
        (component_completeness (program_family program)
          (fold_left (scan_child program) children empty_scan) children)
        children = false.
Proof.
  intros field children not_refused.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  2: { rewrite (no_program_field_is_refused_by_the_current_reducer
         field children resolved) in not_refused.
       discriminate not_refused. }
  exists program.
  unfold reduce_semantic_field in not_refused.
  rewrite resolved in not_refused.
  cbn [sem_definitely_refused] in not_refused.
  repeat rewrite orb_false_iff in not_refused.
  destruct not_refused as
    [[[[[[[[_ body_ok] _] scan_ok] _] _] receive_ok] match_ok] _].
  split; [reflexivity|].
  split; [apply negb_false_iff; exact body_ok|].
  split; [apply negb_false_iff; exact scan_ok|].
  split; [exact receive_ok|exact match_ok].
Qed.

(** * B. Facts of an unrefused child *)

(** Everything a family proof needs from an unrefused child summary.  The
    [program] is the parent's program, the one that chose the descriptor and
    the scalar rule. *)
Record child_facts (program : generated_program)
    (child : semantic_summary) : Prop := ChildFacts {
  child_not_refused : sem_definitely_refused child = false;
  child_descriptor : exists descriptor,
    resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
    wire_eqb (sem_wire child) (descriptor_wire descriptor) = true /\
    payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
    descriptor_accepts_child_type descriptor child = true;
  child_payload_kind : sem_payload child = Scalar \/ sem_payload child = Message;
  child_class_ok : sem_payload child = Message -> class_result_ok child = true;
  child_scalar_rule : sem_payload child = Scalar ->
    exists rule,
      generated_scalar_rule program (sem_tag child) = Some rule /\
      scalar_body_valid rule (sem_body child) = true;
  child_message_bytes : sem_payload child = Message ->
    forallb (fun byte => byte <? 256) (sem_body child) = true;
  child_reread : sem_payload child = Message ->
    forall expected,
      reread_fields_of (sem_type_id child) = Some expected ->
      reread_body_matches expected (sem_body child) = true;
  child_complete_form : exists state kids,
    sem_complete child =
      component_completeness (sem_type_id child) state kids
}.

Lemma child_facts_of_tree :
  forall program subject,
    typed_valid_tree subject ->
    reducer_result_acceptable (semantic_reduce subject) = true ->
    scalar_contract_holds program (tree_field subject) = true ->
    scan_member_facts program (semantic_reduce subject) ->
    child_facts program (semantic_reduce subject).
Proof.
  intros program [field kids] typed acceptable contract [not_refused member].
  cbn [tree_field] in contract.
  cbn [semantic_reduce] in acceptable, not_refused, member |- *.
  destruct (reduce_not_refused_parts field _ not_refused)
    as [child_program [resolved [body_valid _]]].
  destruct (proj1 (typed_valid_tree_node_iff field kids) typed)
    as [_ [_ [reread _]]].
  assert (payload_cases :
    raw_payload field = Scalar \/
    (raw_payload field = Message /\
     exists type_id,
       raw_program_type_id field = Some type_id /\
       lookup_projected_program type_id [] [] = Some child_program)).
  { unfold generated_wire_program in resolved.
    destruct (raw_payload field) eqn:kind;
      destruct (raw_program_type_id field) as [type_id|] eqn:type_found;
      cbv beta iota in resolved;
      try discriminate resolved.
    - left. reflexivity.
    - right. split; [reflexivity|].
      exists type_id. split; [reflexivity|exact resolved]. }
  constructor.
  - exact not_refused.
  - exact member.
  - rewrite reduce_semantic_field_payload.
    destruct payload_cases as [is_scalar | [is_message _]];
      [left|right]; assumption.
  - intro is_message.
    unfold reducer_result_acceptable in acceptable.
    rewrite not_refused, is_message in acceptable.
    cbn [orb payload_eqb] in acceptable.
    exact acceptable.
  - intro is_scalar.
    rewrite reduce_semantic_field_payload in is_scalar.
    rewrite reduce_semantic_field_tag, reduce_semantic_field_body.
    unfold scalar_contract_holds in contract.
    rewrite is_scalar in contract.
    cbv beta iota in contract.
    destruct (resolve_tag (program_fields program) (raw_tag field))
      as [descriptor|] eqn:resolved_tag;
      cbv beta iota in contract; [|discriminate contract].
    destruct (generated_scalar_rule program (raw_tag field))
      as [rule|] eqn:rule_found;
      cbv beta iota in contract; [|discriminate contract].
    exists rule.
    split; [reflexivity|].
    apply andb_prop in contract as [contract _].
    apply andb_prop in contract as [_ rule_valid].
    exact rule_valid.
  - intro is_message.
    rewrite reduce_semantic_field_body.
    unfold valid_raw_body in body_valid.
    apply andb_prop in body_valid as [_ bytes_ok].
    exact bytes_ok.
  - intros is_message expected type_table.
    rewrite reduce_semantic_field_body.
    rewrite reduce_semantic_field_payload in is_message.
    rewrite (reduce_semantic_field_type_id field _ child_program resolved)
      in type_table.
    destruct payload_cases as [is_scalar | [_ [type_id [type_is looked_up]]]].
    { rewrite is_scalar in is_message. discriminate is_message. }
    rewrite (lookup_projected_program_family type_id child_program looked_up)
      in type_table.
    unfold reread_body_canonical in reread.
    rewrite is_message, type_is in reread.
    cbv beta iota in reread.
    rewrite type_table in reread.
    exact reread.
  - exists (fold_left (scan_child child_program)
      (semantic_reduce_forest kids) empty_scan),
      (semantic_reduce_forest kids).
    rewrite (reduce_semantic_field_complete field _ child_program resolved).
    rewrite (reduce_semantic_field_type_id field _ child_program resolved).
    reflexivity.
Qed.

Lemma child_facts_forest :
  forall program children,
    Forall_forest typed_valid_tree children ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    forallb (scalar_contract_holds program) (forest_fields children) = true ->
    Forall (scan_member_facts program) (semantic_reduce_forest children) ->
    Forall (child_facts program) (semantic_reduce_forest children).
Proof.
  intros program children.
  induction children as [|head rest IH];
    intros typed acceptable contract members.
  - constructor.
  - cbn [semantic_reduce_forest] in members |- *.
    inversion typed as [|subject rest_typed typed_head typed_rest]; subst.
    inversion acceptable as [|subject rest_acceptable head_ok rest_ok]; subst.
    cbn [forest_fields forallb] in contract.
    apply andb_prop in contract as [contract_head contract_rest].
    inversion members as [|member_summary member_rest member_head members_rest];
      subst.
    constructor.
    + exact (child_facts_of_tree program head typed_head head_ok contract_head
        member_head).
    + exact (IH typed_rest rest_ok contract_rest members_rest).
Qed.

Lemma not_refused_child_facts :
  forall field children program,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    generated_wire_program field = Some program ->
    scan_valid (fold_left (scan_child program)
      (semantic_reduce_forest children) empty_scan) = true ->
    Forall (child_facts program) (semantic_reduce_forest children).
Proof.
  intros field children program typed acceptable resolved scan_ok.
  destruct (proj1 (typed_valid_tree_node_iff field children) typed)
    as [_ [contract [_ kids]]].
  rewrite (children_scalar_contract_holds_of_program
    field children program resolved) in contract.
  apply child_facts_forest; try assumption.
  exact (scan_fold_member_facts program _ empty_scan scan_ok).
Qed.

(** * C. Totality of the score helpers, from per-tag hypotheses *)

Definition tag_exact (tag : nat) (children : list semantic_summary) : Prop :=
  forall child, In child children -> sem_tag child = tag ->
    exists score, sem_score_evidence child = ExactScore score.

Definition tag_typed_exact (tag type_id : nat)
    (children : list semantic_summary) : Prop :=
  forall child, In child children -> sem_tag child = tag ->
    exists score, exact_typed_message_score type_id child = Some score.

Definition tag_present (tag : nat) (children : list semantic_summary) : Prop :=
  exists child, In child children /\ sem_tag child = tag.

Lemma find_tag_present :
  forall tag children,
    tag_present tag children ->
    exists child, find (fun candidate => sem_tag candidate =? tag) children = Some child.
Proof.
  intros tag children [child [member tag_eq]].
  induction children as [|head rest IH].
  - destruct member.
  - cbn [find].
    destruct (sem_tag head =? tag) eqn:matches.
    + eexists. reflexivity.
    + destruct member as [<- | later].
      * rewrite tag_eq, Nat.eqb_refl in matches. discriminate matches.
      * exact (IH later).
Qed.

Lemma find_tag_member :
  forall tag children child,
    find (fun candidate => sem_tag candidate =? tag) children = Some child ->
    In child children /\ sem_tag child = tag.
Proof.
  intros tag children child found.
  apply find_some in found as [member matches].
  cbv beta in matches.
  apply Nat.eqb_eq in matches.
  split; assumption.
Qed.

Lemma exact_child_score_total :
  forall tag children,
    tag_present tag children ->
    tag_exact tag children ->
    exists score, exact_child_score tag children = Some score.
Proof.
  intros tag children is_present is_exact.
  unfold exact_child_score.
  destruct (find_tag_present tag children is_present) as [child found].
  rewrite found.
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (is_exact child member tag_eq) as [score scored].
  rewrite scored. exists score. reflexivity.
Qed.

Lemma exact_selected_scores_total :
  forall tag children,
    tag_exact tag children ->
    exists scores, exact_selected_scores tag children = Some scores.
Proof.
  intros tag children.
  induction children as [|head rest IH]; intro is_exact;
    cbn [exact_selected_scores].
  - eexists. reflexivity.
  - assert (rest_exact : tag_exact tag rest).
    { intros child member tag_eq.
      exact (is_exact child (or_intror member) tag_eq). }
    destruct (IH rest_exact) as [scores scored].
    rewrite scored.
    destruct (sem_tag head =? tag) eqn:matches.
    + apply Nat.eqb_eq in matches.
      destruct (is_exact head (or_introl eq_refl) matches) as [score head_scored].
      rewrite head_scored. eexists. reflexivity.
    + eexists. reflexivity.
Qed.

Lemma exact_optional_child_score_total :
  forall tag children,
    tag_exact tag children ->
    exists result, exact_optional_child_score tag children = Some result.
Proof.
  intros tag children is_exact.
  unfold exact_optional_child_score.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found.
  - destruct (find_tag_member tag children child found) as [member tag_eq].
    destruct (is_exact child member tag_eq) as [score scored].
    rewrite scored. eexists. reflexivity.
  - eexists. reflexivity.
Qed.

Lemma exact_typed_child_score_total :
  forall tag type_id children,
    tag_present tag children ->
    tag_typed_exact tag type_id children ->
    exists score, exact_typed_child_score tag type_id children = Some score.
Proof.
  intros tag type_id children is_present typed.
  unfold exact_typed_child_score.
  destruct (find_tag_present tag children is_present) as [child found].
  rewrite found.
  destruct (find_tag_member tag children child found) as [member tag_eq].
  exact (typed child member tag_eq).
Qed.

Lemma exact_typed_selected_scores_total :
  forall tag type_id children,
    tag_typed_exact tag type_id children ->
    exists scores, exact_typed_selected_scores tag type_id children = Some scores.
Proof.
  intros tag type_id children.
  induction children as [|head rest IH]; intro typed;
    cbn [exact_typed_selected_scores].
  - eexists. reflexivity.
  - assert (rest_typed : tag_typed_exact tag type_id rest).
    { intros child member tag_eq.
      exact (typed child (or_intror member) tag_eq). }
    destruct (IH rest_typed) as [scores scored].
    rewrite scored.
    destruct (sem_tag head =? tag) eqn:matches.
    + apply Nat.eqb_eq in matches.
      destruct (typed head (or_introl eq_refl) matches) as [score head_scored].
      rewrite head_scored. eexists. reflexivity.
    + eexists. reflexivity.
Qed.

Lemma exact_typed_optional_child_score_total :
  forall tag type_id children,
    tag_typed_exact tag type_id children ->
    exists result, exact_typed_optional_child_score tag type_id children = Some result.
Proof.
  intros tag type_id children typed.
  unfold exact_typed_optional_child_score.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found.
  - destruct (find_tag_member tag children child found) as [member tag_eq].
    destruct (typed child member tag_eq) as [score scored].
    rewrite scored. eexists. reflexivity.
  - eexists. reflexivity.
Qed.

Lemma exact_typed_optional_score_or_total :
  forall tag type_id fallback children,
    tag_typed_exact tag type_id children ->
    exists score,
      exact_typed_optional_score_or tag type_id fallback children = Some score.
Proof.
  intros tag type_id fallback children typed.
  unfold exact_typed_optional_score_or.
  destruct (exact_typed_optional_child_score_total tag type_id children typed)
    as [[score|] result];
    rewrite result; eexists; reflexivity.
Qed.

Lemma exact_typed_message_score_of :
  forall type_id child score,
    sem_type_id child = type_id ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    sem_score_evidence child = ExactScore score ->
    exact_typed_message_score type_id child = Some score.
Proof.
  intros type_id child score type_eq wire_eq payload_eq not_refused scored.
  unfold exact_typed_message_score.
  rewrite type_eq, Nat.eqb_refl, wire_eq, payload_eq, not_refused.
  cbn [wire_eqb payload_eqb negb andb].
  rewrite scored.
  reflexivity.
Qed.

(** ** Scalars *)

Lemma scalar_true_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      sem_payload child = Scalar /\ sem_body child = [1]) ->
    exists flag, scalar_true_or_absent tag children = Some flag.
Proof.
  intros tag children ones.
  unfold scalar_true_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found.
  - destruct (find_tag_member tag children child found) as [member tag_eq].
    destruct (ones child member tag_eq) as [payload_eq body_eq].
    rewrite payload_eq, body_eq. eexists. reflexivity.
  - eexists. reflexivity.
Qed.

(** The semantic value of each varint and fixed kind, from the generated
    validity predicate. *)
Ltac scalar_semantic_tac valid :=
  unfold scalar_semantic_from_source;
  rewrite valid;
  cbn [scalar_kind];
  unfold scalar_body_valid in valid;
  cbn [scalar_kind scalar_default_forbidden] in valid;
  destruct (compact_varint_value _) as [varint|] eqn:compact;
    [|discriminate valid];
  cbn [option_map];
  eexists; reflexivity.

Lemma scalar_semantic_bool :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBBool true) body = true ->
    exists flag, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBBool true) body = Some (ScalarBool flag).
Proof. intros tag body valid. scalar_semantic_tac valid. Qed.

Lemma scalar_semantic_int32 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBInt32 true) body = true ->
    exists value, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBInt32 true) body = Some (ScalarI32 value).
Proof. intros tag body valid. scalar_semantic_tac valid. Qed.

Lemma scalar_semantic_sint32 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBSint32 true) body = true ->
    exists value, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBSint32 true) body = Some (ScalarI32 value).
Proof. intros tag body valid. scalar_semantic_tac valid. Qed.

Lemma scalar_semantic_u32 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBUint32 true) body = true ->
    exists value, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBUint32 true) body = Some (ScalarU32 value).
Proof. intros tag body valid. scalar_semantic_tac valid. Qed.

Lemma scalar_semantic_u64 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBUint64 true) body = true ->
    exists value, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBUint64 true) body = Some (ScalarU64 value).
Proof. intros tag body valid. scalar_semantic_tac valid. Qed.

Lemma scalar_semantic_fixed64 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBFixed64 false) body = true ->
    exists value, scalar_semantic_from_source
      (ScalarRule tag GeneratedData.PBFixed64 false) body = Some (ScalarU64 value).
Proof.
  intros tag body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  cbn [scalar_kind].
  eexists. reflexivity.
Qed.

(** The compact value of a body validated under a varint rule. *)
Lemma compact_of_valid_sint64 :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBSint64 false) body = true ->
    exists value, compact_varint_value body = Some value.
Proof.
  intros tag body valid.
  unfold scalar_body_valid in valid.
  cbn [scalar_kind scalar_default_forbidden] in valid.
  destruct (compact_varint_value body) as [value|] eqn:compact;
    [eexists; reflexivity|discriminate valid].
Qed.

Lemma compact_of_valid_sint32_forbidden :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBSint32 true) body = true ->
    exists value,
      compact_varint_value body = Some value /\
      N.leb value 4294967295 = true.
Proof.
  intros tag body valid.
  unfold scalar_body_valid in valid.
  cbn [scalar_kind scalar_default_forbidden] in valid.
  destruct (compact_varint_value body) as [value|] eqn:compact;
    [|discriminate valid].
  exists value. split; [reflexivity|].
  unfold numeric_scalar_valid in valid.
  cbn [negb orb] in valid.
  apply andb_prop in valid as [bound _].
  exact bound.
Qed.

Lemma exact_bool_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      sem_wire child = Varint /\ sem_payload child = Scalar /\
      sem_definitely_refused child = false /\
      scalar_body_valid (ScalarRule tag GeneratedData.PBBool true)
        (sem_body child) = true) ->
    exists flag, exact_bool_or_absent tag children = Some flag.
Proof.
  intros tag children scalars.
  unfold exact_bool_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (scalars child member tag_eq)
    as [wire_eq [payload_eq [not_refused valid]]].
  rewrite wire_eq, payload_eq, not_refused.
  destruct (scalar_semantic_bool tag (sem_body child) valid) as [flag semantic].
  rewrite semantic. eexists. reflexivity.
Qed.

Lemma exact_pbint32_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      sem_wire child = Varint /\ sem_payload child = Scalar /\
      sem_definitely_refused child = false /\
      scalar_body_valid (ScalarRule tag GeneratedData.PBInt32 true)
        (sem_body child) = true) ->
    exists value, exact_pbint32_or_absent tag children = Some value.
Proof.
  intros tag children scalars.
  unfold exact_pbint32_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (scalars child member tag_eq)
    as [wire_eq [payload_eq [not_refused valid]]].
  rewrite wire_eq, payload_eq, not_refused.
  destruct (scalar_semantic_int32 tag (sem_body child) valid) as [value semantic].
  rewrite semantic. eexists. reflexivity.
Qed.

Lemma exact_i32_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      scalar_body_valid (ScalarRule tag GeneratedData.PBSint32 true)
        (sem_body child) = true) ->
    exists value, exact_i32_or_absent tag children = Some value.
Proof.
  intros tag children scalars.
  unfold exact_i32_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (scalar_semantic_sint32 tag (sem_body child)
    (scalars child member tag_eq)) as [value semantic].
  rewrite semantic. eexists. reflexivity.
Qed.

Lemma exact_u32_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      scalar_body_valid (ScalarRule tag GeneratedData.PBUint32 true)
        (sem_body child) = true) ->
    exists value, exact_u32_or_absent tag children = Some value.
Proof.
  intros tag children scalars.
  unfold exact_u32_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (scalar_semantic_u32 tag (sem_body child)
    (scalars child member tag_eq)) as [value semantic].
  rewrite semantic. eexists. reflexivity.
Qed.

Lemma exact_u64_or_absent_total :
  forall tag children,
    (forall child, In child children -> sem_tag child = tag ->
      scalar_body_valid (ScalarRule tag GeneratedData.PBUint64 true)
        (sem_body child) = true) ->
    exists value, exact_u64_or_absent tag children = Some value.
Proof.
  intros tag children scalars.
  unfold exact_u64_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member tag children child found) as [member tag_eq].
  destruct (scalar_semantic_u64 tag (sem_body child)
    (scalars child member tag_eq)) as [value semantic].
  rewrite semantic. eexists. reflexivity.
Qed.

Lemma new_bind_count_word_total :
  forall children,
    (forall child, In child children -> sem_tag child = 1 ->
      scalar_body_valid (ScalarRule 1 GeneratedData.PBSint32 true)
        (sem_body child) = true) ->
    exists word, new_bind_count_word children = Some word.
Proof.
  intros children scalars.
  unfold new_bind_count_word.
  destruct (find (fun child => sem_tag child =? 1) children)
    as [child|] eqn:found; [|eexists; reflexivity].
  destruct (find_tag_member 1 children child found) as [member tag_eq].
  destruct (compact_of_valid_sint32_forbidden 1 (sem_body child)
    (scalars child member tag_eq)) as [value [compact bound]].
  rewrite compact, bound. eexists. reflexivity.
Qed.


(** * D. Wire-level facts for the re-read families *)

(** ** A list suffix view of the reader *)

Lemma skipn_cons_facts :
  forall (bytes : list nat) offset byte rest,
    skipn offset bytes = byte :: rest ->
    nth_error bytes offset = Some byte /\
    offset < length bytes /\
    skipn (S offset) bytes = rest.
Proof.
  intros bytes offset.
  revert bytes.
  induction offset as [|offset IH]; intros bytes byte rest skipped.
  - destruct bytes as [|first tail]; cbn [skipn] in skipped;
      [discriminate skipped|].
    injection skipped as first_eq tail_eq.
    subst first tail.
    split; [reflexivity|].
    split; [cbn [length]; lia|reflexivity].
  - destruct bytes as [|first tail]; cbn [skipn] in skipped;
      [discriminate skipped|].
    destruct (IH tail byte rest skipped) as [nth [bound next]].
    split; [exact nth|].
    split; [cbn [length]; lia|exact next].
Qed.

(** The independent list-suffix varint reader of TV3 agrees with the
    offset-based cursor reader on the same bytes. *)
Lemma varint_groups_from_agrees :
  forall groups_left (bytes : list nat) offset group (value v : N) rest,
    varint_groups_from groups_left group value (skipn offset bytes) =
      Some (v, rest) ->
    exists stop,
      Wire.varint_groups groups_left bytes (length bytes) offset group value =
        Some (v, stop) /\
      offset < stop /\ stop <= length bytes /\ skipn stop bytes = rest.
Proof.
  induction groups_left as [|groups_left IH];
    intros bytes offset group value v rest decoded.
  - discriminate decoded.
  - cbn [varint_groups_from] in decoded.
    destruct (skipn offset bytes) as [|byte tail] eqn:skipped;
      cbv beta iota in decoded.
    + discriminate decoded.
    + destruct (skipn_cons_facts bytes offset byte tail skipped)
        as [nth [bound next]].
      cbn [Wire.varint_groups].
      rewrite (proj2 (Nat.ltb_lt _ _) bound), nth.
      destruct ((255 <? byte) || ((group =? 9) && (1 <? byte))) eqn:rejected;
        cbv beta iota zeta in decoded.
      * discriminate decoded.
      * apply orb_false_elim in rejected as [small not_ninth].
        assert (byte_valid : Wire.byte_ok byte = true).
        { unfold Wire.byte_ok.
          apply Nat.ltb_lt.
          apply Nat.ltb_ge in small.
          lia. }
        rewrite byte_valid, not_ninth.
        cbv beta iota zeta.
        destruct (byte <? 128) eqn:terminal;
          cbv beta iota in decoded |- *.
        -- destruct ((0 <? group) && (byte =? 0)) eqn:trailing;
             cbv beta iota in decoded |- *.
           ++ discriminate decoded.
           ++ injection decoded as v_eq rest_eq.
              subst v rest.
              exists (S offset).
              split; [reflexivity|].
              split; [lia|].
              split; [lia|exact next].
        -- rewrite <- next in decoded.
           destruct (IH bytes (S offset) (S group) _ v rest decoded)
             as [stop [walked [above [below tail_eq]]]].
           exists stop.
           split; [exact walked|].
           split; [lia|].
           split; [exact below|exact tail_eq].
Qed.

(** A canonical varint read from the head of [input] splits off its own
    bytes. *)
Lemma varint_groups_from_prefix :
  forall groups_left group (value v : N) input rest,
    varint_groups_from groups_left group value input = Some (v, rest) ->
    exists prefix,
      input = prefix ++ rest /\
      prefix <> [] /\
      varint_groups_from groups_left group value prefix = Some (v, []).
Proof.
  induction groups_left as [|groups_left IH];
    intros group value v input rest decoded.
  - discriminate decoded.
  - destruct input as [|byte tail].
    + cbn [varint_groups_from] in decoded. discriminate decoded.
    + cbn [varint_groups_from] in decoded.
      destruct ((255 <? byte) || ((group =? 9) && (1 <? byte))) eqn:rejected;
        cbv beta iota zeta in decoded.
      * discriminate decoded.
      * destruct (byte <? 128) eqn:terminal; cbv beta iota in decoded.
        -- destruct ((0 <? group) && (byte =? 0)) eqn:trailing;
             cbv beta iota in decoded.
           ++ discriminate decoded.
           ++ injection decoded as v_eq rest_eq.
              subst v rest.
              exists [byte].
              split; [reflexivity|].
              split; [discriminate|].
              cbn [varint_groups_from].
              rewrite rejected.
              cbv beta iota zeta.
              rewrite terminal.
              cbv beta iota.
              rewrite trailing.
              reflexivity.
        -- destruct (IH (S group) _ v tail rest decoded)
             as [prefix [tail_eq [nonempty complete]]].
           exists (byte :: prefix).
           split; [rewrite tail_eq; reflexivity|].
           split; [discriminate|].
           cbn [varint_groups_from].
           rewrite rejected.
           cbv beta iota zeta.
           rewrite terminal.
           exact complete.
Qed.

Lemma compact_of_complete_prefix :
  forall prefix v,
    varint_groups_from 10 0 0%N prefix = Some (v, []) ->
    compact_varint_value prefix = Some v.
Proof.
  intros prefix v decoded.
  destruct (varint_groups_from_agrees 10 prefix 0 0 0%N v [] decoded)
    as [stop [walked [_ [below tail_eq]]]].
  assert (stop_eq : stop = length prefix).
  { pose proof (f_equal (@length nat) tail_eq) as len.
    rewrite length_skipn in len.
    cbn [length] in len.
    lia. }
  unfold compact_varint_value, Wire.canonical_varint.
  rewrite walked, stop_eq, Nat.eqb_refl.
  reflexivity.
Qed.

(** A varint whose value is below 128 occupies exactly one byte, so a bool
    or small field body is literally [[0]] or [[1]]. *)
Lemma first_group_guards :
  forall byte,
    ((0 =? 9) && (1 <? byte)) = false /\
    ((0 <? 0) && (byte =? 0)) = false.
Proof. intro byte. split; reflexivity. Qed.

(** [injection] applies [simpl] to the equations it produces, which unfolds
    [byte mod 128] once the modulus is a literal.  This lemma states the same
    injectivity while keeping the term exactly as written. *)
Lemma some_pair_injective :
  forall (A B : Type) (first first' : A) (second second' : B),
    Some (first, second) = Some (first', second') ->
    first = first' /\ second = second'.
Proof.
  intros A B first first' second second' equal.
  injection equal as first_eq second_eq.
  split; assumption.
Qed.

Lemma later_group_lower_bound :
  forall fuel bytes re offset group value v stop,
    0 < group ->
    Wire.varint_groups fuel bytes re offset group value = Some (v, stop) ->
    (128 ^ N.of_nat group <= v)%N.
Proof.
  induction fuel as [|fuel IH];
    intros bytes re offset group value v stop positive decoded.
  - discriminate decoded.
  - cbn [Wire.varint_groups] in decoded.
    destruct (offset <? re); cbv beta iota in decoded; [|discriminate decoded].
    destruct (nth_error bytes offset) as [byte|];
      cbv beta iota in decoded; [|discriminate decoded].
    destruct (Wire.byte_ok byte); cbv beta iota in decoded;
      [|discriminate decoded].
    destruct ((group =? 9) && (1 <? byte)); cbv beta iota zeta in decoded;
      [discriminate decoded|].
    destruct (byte <? 128) eqn:terminal; cbv beta iota in decoded.
    + destruct ((0 <? group) && (byte =? 0)) eqn:trailing;
        cbv beta iota in decoded; [discriminate decoded|].
      destruct (some_pair_injective _ _ _ _ _ _ decoded) as [v_eq stop_eq].
      apply Nat.ltb_lt in terminal.
      rewrite (proj2 (Nat.ltb_lt 0 group) positive) in trailing.
      cbn [andb] in trailing.
      apply Nat.eqb_neq in trailing.
      rewrite <- v_eq.
      rewrite Nat.mod_small by exact terminal.
      assert (byte_pos : (1 <= N.of_nat byte)%N) by lia.
      assert (step : (1 * 128 ^ N.of_nat group <=
        N.of_nat byte * 128 ^ N.of_nat group)%N).
      { apply N.mul_le_mono_r. exact byte_pos. }
      rewrite N.mul_1_l in step.
      lia.
    + pose proof (IH bytes re (S offset) (S group) _ v stop
        (Nat.lt_0_succ group) decoded) as bound.
      rewrite Nat2N.inj_succ, N.pow_succ_r in bound by lia.
      lia.
Qed.

Lemma varint_groups_small_value :
  forall fuel body v stop,
    Wire.varint_groups (S fuel) body (length body) 0 0 0%N = Some (v, stop) ->
    stop = length body ->
    (v <= 127)%N ->
    body = [N.to_nat v].
Proof.
  intros fuel body v stop decoded stop_eq small.
  cbn [Wire.varint_groups] in decoded.
  destruct (0 <? length body) eqn:nonempty;
    cbv beta iota in decoded; [|discriminate decoded].
  apply Nat.ltb_lt in nonempty.
  destruct (nth_error body 0) as [byte|] eqn:first_byte;
    cbv beta iota in decoded; [|discriminate decoded].
  destruct (Wire.byte_ok byte) eqn:byte_valid;
    cbv beta iota in decoded; [|discriminate decoded].
  destruct (first_group_guards byte) as [not_ninth not_trailing].
  rewrite not_ninth in decoded.
  cbv beta iota zeta in decoded.
  destruct (byte <? 128) eqn:terminal; cbv beta iota in decoded.
  - rewrite not_trailing in decoded.
    cbv beta iota in decoded.
    destruct (some_pair_injective _ _ _ _ _ _ decoded) as [v_eq stop_one].
    apply Nat.ltb_lt in terminal.
    assert (value_is_byte : v = N.of_nat byte).
    { rewrite <- v_eq.
      rewrite Nat.mod_small by exact terminal.
      change (128 ^ N.of_nat 0)%N with 1%N.
      lia. }
    rewrite stop_eq in stop_one.
    destruct body as [|head [|second tail]];
      cbn [length] in nonempty, stop_one; try lia.
    cbn [nth_error] in first_byte.
    injection first_byte as head_eq.
    subst head.
    rewrite value_is_byte, Nat2N.id.
    reflexivity.
  - pose proof (later_group_lower_bound fuel body (length body) 1 1 _ v stop
      ltac:(lia) decoded) as bound.
    change (128 ^ N.of_nat 1)%N with 128%N in bound.
    lia.
Qed.

Lemma compact_varint_small :
  forall body v,
    compact_varint_value body = Some v ->
    (v <= 127)%N ->
    body = [N.to_nat v].
Proof.
  intros body v compact small.
  unfold compact_varint_value, Wire.canonical_varint in compact.
  destruct (Wire.varint_groups 10 body (length body) 0 0 0%N)
    as [[value stop]|] eqn:walked; [|discriminate compact].
  cbv beta iota in compact.
  destruct (stop =? length body) eqn:stop_eq; [|discriminate compact].
  cbv beta iota in compact.
  injection compact as value_eq.
  subst value.
  apply Nat.eqb_eq in stop_eq.
  exact (varint_groups_small_value 9 body v stop walked stop_eq small).
Qed.

Lemma bool_forbidden_body_is_one :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBBool true) body = true ->
    body = [1].
Proof.
  intros tag body valid.
  unfold scalar_body_valid in valid.
  cbn [scalar_kind scalar_default_forbidden] in valid.
  destruct (compact_varint_value body) as [value|] eqn:compact;
    [|discriminate valid].
  unfold numeric_scalar_valid in valid.
  cbn [negb orb] in valid.
  apply andb_prop in valid as [bound nonzero].
  apply N.leb_le in bound.
  apply negb_true_iff in nonzero.
  apply N.eqb_neq in nonzero.
  assert (value_one : value = 1%N) by lia.
  subst value.
  exact (compact_varint_small body 1 compact ltac:(lia)).
Qed.

Lemma bool_default_body_is_small :
  forall tag body,
    scalar_body_valid (ScalarRule tag GeneratedData.PBBool false) body = true ->
    body = [0] \/ body = [1].
Proof.
  intros tag body valid.
  unfold scalar_body_valid in valid.
  cbn [scalar_kind scalar_default_forbidden] in valid.
  destruct (compact_varint_value body) as [value|] eqn:compact;
    [|discriminate valid].
  unfold numeric_scalar_valid in valid.
  cbn [negb orb] in valid.
  apply andb_prop in valid as [bound _].
  apply N.leb_le in bound.
  pose proof (compact_varint_small body value compact ltac:(lia)) as body_eq.
  rewrite body_eq.
  destruct (N.eq_dec value 0%N) as [zero|nonzero].
  - left. subst value. reflexivity.
  - right. assert (value_one : value = 1%N) by lia.
    subst value. reflexivity.
Qed.

(** ** Framing of one field *)

Lemma varint_groups_literal_key :
  forall fuel bytes re offset key value,
    offset < re ->
    nth_error bytes offset = Some key ->
    key < 128 ->
    Wire.varint_groups (S fuel) bytes re offset 0 value =
      Some ((value + N.of_nat key)%N, S offset).
Proof.
  intros fuel bytes re offset key value bound nth small.
  cbn [Wire.varint_groups].
  rewrite (proj2 (Nat.ltb_lt _ _) bound), nth.
  assert (byte_valid : Wire.byte_ok key = true).
  { unfold Wire.byte_ok. apply Nat.ltb_lt. lia. }
  rewrite byte_valid.
  destruct (first_group_guards key) as [not_ninth not_trailing].
  rewrite not_ninth.
  cbv beta iota zeta.
  rewrite (proj2 (Nat.ltb_lt key 128) small).
  cbv beta iota.
  rewrite not_trailing.
  cbv beta iota.
  rewrite Nat.mod_small by exact small.
  change (128 ^ N.of_nat 0)%N with 1%N.
  rewrite N.mul_1_r.
  reflexivity.
Qed.

Lemma framing_length_field :
  forall bytes re offset last key key_end payload_length body_start number,
    Wire.canonical_varint bytes re offset = Some (key, key_end) ->
    ((0 <? key / 8)%N && Wire.field_number_within_limit (key / 8)%N) = true ->
    N.to_nat (key / 8)%N = number ->
    N.to_nat (key mod 8)%N = 2 ->
    Wire.canonical_varint bytes re key_end = Some (payload_length, body_start) ->
    N.leb payload_length (N.of_nat (re - body_start)) = true ->
    Wire.framing_field bytes re offset last false =
      Some (Wire.FieldSpan number Wire.WireLength offset key_end body_start
        (body_start + N.to_nat payload_length) None).
Proof.
  intros bytes re offset last key key_end payload_length body_start number
    key_read limits number_eq wire_eq body_read fits.
  unfold Wire.framing_field.
  rewrite key_read.
  cbv beta iota zeta.
  rewrite limits.
  cbv beta iota zeta.
  cbn [negb orb].
  rewrite wire_eq.
  cbv beta iota zeta.
  rewrite body_read.
  cbv beta iota zeta.
  rewrite fits.
  cbv beta iota zeta.
  rewrite number_eq.
  reflexivity.
Qed.

Lemma framing_varint_field :
  forall bytes re offset last key key_end value value_end number,
    Wire.canonical_varint bytes re offset = Some (key, key_end) ->
    ((0 <? key / 8)%N && Wire.field_number_within_limit (key / 8)%N) = true ->
    N.to_nat (key / 8)%N = number ->
    N.to_nat (key mod 8)%N = 0 ->
    Wire.canonical_varint bytes re key_end = Some (value, value_end) ->
    Wire.framing_field bytes re offset last false =
      Some (Wire.FieldSpan number Wire.WireVarint offset key_end key_end
        value_end (Some value)).
Proof.
  intros bytes re offset last key key_end value value_end number
    key_read limits number_eq wire_eq value_read.
  unfold Wire.framing_field.
  rewrite key_read.
  cbv beta iota zeta.
  rewrite limits.
  cbv beta iota zeta.
  cbn [negb orb].
  rewrite wire_eq.
  cbv beta iota zeta.
  rewrite value_read.
  cbv beta iota zeta.
  rewrite number_eq.
  reflexivity.
Qed.

Lemma checked_field_of_framing :
  forall bytes re offset last span,
    Wire.framing_field bytes re offset last false = Some span ->
    Wire.span_start span = offset ->
    offset < Wire.span_end span ->
    Wire.span_end span <= re ->
    Wire.checked_field bytes re offset last false = Some span.
Proof.
  intros bytes re offset last span framed start_eq progress bound.
  unfold Wire.checked_field.
  rewrite framed.
  cbv beta iota.
  rewrite start_eq, Nat.eqb_refl.
  rewrite (proj2 (Nat.ltb_lt _ _) progress).
  rewrite (proj2 (Nat.leb_le _ _) bound).
  reflexivity.
Qed.

(** One bytes field: [key] is the literal one-byte key at [offset]; the field
    tail grammar of TV3 gives the checked span, non-empty payload and the
    suffix it leaves. *)
Lemma bytes_field_checked :
  forall (whole : list nat) offset last key after_key suffix number,
    skipn offset whole = key :: after_key ->
    key < 128 ->
    ((0 <? N.of_nat key / 8)%N &&
      Wire.field_number_within_limit (N.of_nat key / 8)%N) = true ->
    N.to_nat (N.of_nat key / 8)%N = number ->
    N.to_nat (N.of_nat key mod 8)%N = 2 ->
    bytes_field_tail after_key = Some suffix ->
    exists body_start count,
      Wire.checked_field whole (length whole) offset last false =
        Some (Wire.FieldSpan number Wire.WireLength offset (S offset)
          body_start (body_start + count) None) /\
      S offset < body_start /\
      0 < count /\
      body_start + count <= length whole /\
      skipn (body_start + count) whole = suffix.
Proof.
  intros whole offset last key after_key suffix number skipped small
    limits number_eq wire_eq tail.
  destruct (skipn_cons_facts whole offset key after_key skipped)
    as [nth [offset_bound after_eq]].
  unfold bytes_field_tail in tail.
  destruct (canonical_varint_prefix after_key) as [[declared rest]|]
    eqn:prefix; cbv beta iota zeta in tail; [|discriminate tail].
  unfold canonical_varint_prefix in prefix.
  destruct (N.leb declared (N.of_nat (length rest))) eqn:fits;
    cbv beta iota zeta in tail; [|discriminate tail].
  destruct ((0 <? N.to_nat declared) &&
    forallb (fun byte => byte <? 256) (firstn (N.to_nat declared) rest))
    eqn:payload_ok;
    cbv beta iota zeta in tail; [|discriminate tail].
  injection tail as suffix_eq.
  apply andb_prop in payload_ok as [positive _].
  apply Nat.ltb_lt in positive.
  rewrite <- after_eq in prefix.
  destruct (varint_groups_from_agrees 10 whole (S offset) 0 0%N
    declared rest prefix) as [stop [walked [above [below rest_eq]]]].
  assert (key_read : Wire.canonical_varint whole (length whole) offset =
      Some (N.of_nat key, S offset)).
  { exact (varint_groups_literal_key 9 whole (length whole) offset key 0%N
      offset_bound nth small). }
  assert (body_read : Wire.canonical_varint whole (length whole) (S offset) =
      Some (declared, stop)).
  { exact walked. }
  assert (rest_length : length rest = length whole - stop).
  { rewrite <- rest_eq, length_skipn. reflexivity. }
  assert (fits_nat : N.to_nat declared <= length rest).
  { apply N.leb_le in fits.
    lia. }
  assert (guard : N.leb declared (N.of_nat (length whole - stop)) = true).
  { rewrite <- rest_length. exact fits. }
  pose proof (framing_length_field whole (length whole) offset last
    (N.of_nat key) (S offset) declared stop number key_read limits number_eq
    wire_eq body_read guard) as framed.
  assert (end_bound : stop + N.to_nat declared <= length whole) by lia.
  assert (progress : offset < stop + N.to_nat declared) by lia.
  exists stop, (N.to_nat declared).
  split.
  { apply (checked_field_of_framing whole (length whole) offset last _ framed).
    - reflexivity.
    - exact progress.
    - exact end_bound. }
  split; [lia|].
  split; [exact positive|].
  split; [exact end_bound|].
  rewrite <- suffix_eq.
  replace (stop + N.to_nat declared) with (N.to_nat declared + stop) by lia.
  rewrite <- skipn_skipn, rest_eq.
  reflexivity.
Qed.

Lemma compact_varint_literal_key_16 :
  ((0 <? N.of_nat 16 / 8)%N &&
    Wire.field_number_within_limit (N.of_nat 16 / 8)%N) = true /\
  N.to_nat (N.of_nat 16 / 8)%N = 2 /\
  N.to_nat (N.of_nat 16 mod 8)%N = 0.
Proof. repeat split; reflexivity. Qed.

Lemma literal_key_10 :
  ((0 <? N.of_nat 10 / 8)%N &&
    Wire.field_number_within_limit (N.of_nat 10 / 8)%N) = true /\
  N.to_nat (N.of_nat 10 / 8)%N = 1 /\
  N.to_nat (N.of_nat 10 mod 8)%N = 2.
Proof. repeat split; reflexivity. Qed.

Lemma literal_key_18 :
  ((0 <? N.of_nat 18 / 8)%N &&
    Wire.field_number_within_limit (N.of_nat 18 / 8)%N) = true /\
  N.to_nat (N.of_nat 18 / 8)%N = 2 /\
  N.to_nat (N.of_nat 18 mod 8)%N = 2.
Proof. repeat split; reflexivity. Qed.

(** One uint32 field with the varint key 16 (number 2, wire 0).  Its payload
    bytes are exactly the varint, so the compact value is the varint value. *)
Lemma u32_field_checked :
  forall (whole : list nat) offset last after_key rest,
    skipn offset whole = 16 :: after_key ->
    u32_field_tail after_key = Some rest ->
    exists value value_end,
      Wire.checked_field whole (length whole) offset last false =
        Some (Wire.FieldSpan 2 Wire.WireVarint offset (S offset) (S offset)
          value_end (Some value)) /\
      S offset < value_end /\
      value_end <= length whole /\
      skipn value_end whole = rest /\
      compact_varint_value
        (firstn (value_end - S offset) (skipn (S offset) whole)) = Some value /\
      (0 < value)%N /\
      (value <= 4294967295)%N.
Proof.
  intros whole offset last after_key rest skipped tail.
  destruct (skipn_cons_facts whole offset 16 after_key skipped)
    as [nth [offset_bound after_eq]].
  unfold u32_field_tail in tail.
  destruct (canonical_varint_prefix after_key) as [[value suffix]|]
    eqn:prefix; cbv beta iota zeta in tail; [|discriminate tail].
  destruct (N.ltb 0%N value && N.leb value 4294967295%N) eqn:bounds;
    cbv beta iota in tail; [|discriminate tail].
  injection tail as suffix_eq.
  subst suffix.
  apply andb_prop in bounds as [positive upper].
  apply N.ltb_lt in positive.
  apply N.leb_le in upper.
  unfold canonical_varint_prefix in prefix.
  destruct (varint_groups_from_prefix 10 0 0%N value after_key rest prefix)
    as [pre [split_eq [nonempty complete]]].
  rewrite <- after_eq in prefix.
  destruct (varint_groups_from_agrees 10 whole (S offset) 0 0%N value rest
    prefix) as [stop [walked [above [below rest_eq]]]].
  assert (key_read : Wire.canonical_varint whole (length whole) offset =
      Some (N.of_nat 16, S offset)).
  { exact (varint_groups_literal_key 9 whole (length whole) offset 16 0%N
      offset_bound nth ltac:(lia)). }
  assert (value_read : Wire.canonical_varint whole (length whole) (S offset) =
      Some (value, stop)).
  { exact walked. }
  destruct compact_varint_literal_key_16 as [limits [number_eq wire_eq]].
  pose proof (framing_varint_field whole (length whole) offset last
    (N.of_nat 16) (S offset) value stop 2 key_read limits number_eq wire_eq
    value_read) as framed.
  exists value, stop.
  split.
  { apply (checked_field_of_framing whole (length whole) offset last _ framed).
    - reflexivity.
    - cbn [Wire.span_end]. lia.
    - cbn [Wire.span_end]. exact below. }
  split; [lia|].
  split; [exact below|].
  split; [exact rest_eq|].
  split.
  - assert (lengths : stop - S offset = length pre).
    { pose proof (f_equal (@length nat) rest_eq) as rest_len.
      rewrite length_skipn in rest_len.
      pose proof (f_equal (@length nat) after_eq) as after_len.
      rewrite length_skipn in after_len.
      pose proof (f_equal (@length nat) split_eq) as split_len.
      rewrite length_app in split_len.
      lia. }
    rewrite lengths, after_eq, split_eq, firstn_app, Nat.sub_diag.
    cbn [firstn].
    rewrite firstn_all, app_nil_r.
    exact (compact_of_complete_prefix pre value complete).
  - split; [exact positive|exact upper].
Qed.

(** ** A chain of checked fields scans to its spans *)

Fixpoint field_chain (bytes : list nat) (re offset last : nat)
    (spans : list Wire.field_span) : Prop :=
  match spans with
  | [] => offset = re
  | span :: rest =>
      offset <> re /\
      Wire.checked_field bytes re offset last false = Some span /\
      field_chain bytes re (Wire.span_end span) (Wire.span_number span) rest
  end.

Lemma reference_remaining_field_chain :
  forall spans fuel bytes re offset last,
    field_chain bytes re offset last spans ->
    length spans <= fuel ->
    Wire.reference_remaining fuel bytes re offset last false = Wire.Parsed spans.
Proof.
  induction spans as [|span rest IH];
    intros fuel bytes re offset last chain fits.
  - cbn [field_chain] in chain.
    subst offset.
    destruct fuel; cbn [Wire.reference_remaining];
      rewrite Nat.eqb_refl; reflexivity.
  - cbn [field_chain] in chain.
    destruct chain as [not_end [checked next]].
    cbn [length] in fits.
    destruct fuel as [|fuel]; [lia|].
    cbn [Wire.reference_remaining].
    apply Nat.eqb_neq in not_end.
    rewrite not_end.
    cbv beta iota.
    rewrite checked.
    cbv beta iota.
    rewrite (IH fuel bytes re (Wire.span_end span) (Wire.span_number span)
      next ltac:(lia)).
    reflexivity.
Qed.

Lemma region_ok_whole :
  forall bytes,
    forallb Wire.byte_ok bytes = true ->
    Wire.region_ok bytes 0 (length bytes) = true.
Proof.
  intros bytes valid.
  unfold Wire.region_ok.
  rewrite Nat.leb_refl, Nat.sub_0_r.
  cbn [skipn].
  rewrite firstn_all, valid.
  reflexivity.
Qed.

Lemma worklist_region_chain :
  forall bytes spans,
    forallb Wire.byte_ok bytes = true ->
    field_chain bytes (length bytes) 0 0 spans ->
    length spans <= length bytes ->
    Wire.worklist_region bytes 0 (length bytes) false = Wire.Parsed spans.
Proof.
  intros bytes spans valid chain fits.
  rewrite Wire.worklist_and_reference_agree_on_every_finite_region.
  unfold Wire.reference_region.
  rewrite (region_ok_whole bytes valid), Nat.sub_0_r.
  exact (reference_remaining_field_chain spans (length bytes) bytes
    (length bytes) 0 0 chain fits).
Qed.

(** ** One level of the region decoder *)

Lemma decode_region_parsed :
  forall fuel bytes program region_start region_end spans,
    Wire.worklist_region bytes region_start region_end false = Wire.Parsed spans ->
    decode_region (S fuel) bytes program region_start region_end =
    option_map (fun state => rev (decode_reversed state))
      (fold_left
        (decode_source_step program
          (fun child_program child_start child_end =>
            decode_region fuel bytes child_program child_start child_end))
        spans (Some (DecodeAccumulator None [] []))).
Proof.
  intros fuel bytes program region_start region_end spans parsed.
  cbn [decode_region].
  rewrite parsed.
  reflexivity.
Qed.

(** ** The two re-read programs, by computation *)

Definition bigrat_numerator_field : field_descriptor :=
  FieldDescriptor 1 0 LengthDelimited false Scalar None None.

Definition bigrat_denominator_field : field_descriptor :=
  FieldDescriptor 2 1 LengthDelimited false Scalar None None.

Definition bigrat_program : generated_program :=
  GeneratedProgram 25 2
    [bigrat_numerator_field; bigrat_denominator_field] [] [].

Definition fixed_point_unscaled_field : field_descriptor :=
  FieldDescriptor 1 0 LengthDelimited false Scalar None None.

Definition fixed_point_scale_field : field_descriptor :=
  FieldDescriptor 2 1 Varint false Scalar None None.

Definition fixed_point_program : generated_program :=
  GeneratedProgram 26 2
    [fixed_point_unscaled_field; fixed_point_scale_field] [] [].

Lemma lookup_bigrat_program :
  lookup_projected_program 25 [] [] = Some bigrat_program.
Proof. vm_compute. reflexivity. Qed.

Lemma lookup_fixed_point_program :
  lookup_projected_program 26 [] [] = Some fixed_point_program.
Proof. vm_compute. reflexivity. Qed.

Lemma bigrat_rule_one :
  generated_scalar_rule bigrat_program 1 =
    Some (ScalarRule 1 GeneratedData.PBBytes true).
Proof. vm_compute. reflexivity. Qed.

Lemma bigrat_rule_two :
  generated_scalar_rule bigrat_program 2 =
    Some (ScalarRule 2 GeneratedData.PBBytes true).
Proof. vm_compute. reflexivity. Qed.

Lemma fixed_point_rule_one :
  generated_scalar_rule fixed_point_program 1 =
    Some (ScalarRule 1 GeneratedData.PBBytes true).
Proof. vm_compute. reflexivity. Qed.

Lemma fixed_point_rule_two :
  generated_scalar_rule fixed_point_program 2 =
    Some (ScalarRule 2 GeneratedData.PBUint32 true).
Proof. vm_compute. reflexivity. Qed.

(** ** Reading the ground scalars of a decoded body *)

Lemma bytes_source_semantic :
  forall tag payload,
    length payload <> 0 ->
    scalar_semantic_from_source (ScalarRule tag GeneratedData.PBBytes true)
      payload = Some (ScalarBytes payload).
Proof.
  intros tag payload nonempty.
  unfold scalar_semantic_from_source, scalar_body_valid.
  cbn [scalar_kind scalar_default_forbidden negb orb].
  destruct (length payload =? 0) eqn:zero.
  - apply Nat.eqb_eq in zero. contradiction.
  - reflexivity.
Qed.

Lemma u32_source_valid :
  forall tag payload value,
    compact_varint_value payload = Some value ->
    (0 < value)%N ->
    (value <= 4294967295)%N ->
    scalar_body_valid (ScalarRule tag GeneratedData.PBUint32 true) payload = true.
Proof.
  intros tag payload value compact positive bounded.
  unfold scalar_body_valid.
  cbn [scalar_kind scalar_default_forbidden].
  rewrite compact.
  unfold numeric_scalar_valid.
  cbn [negb orb].
  apply andb_true_intro.
  split.
  - apply N.leb_le. exact bounded.
  - apply negb_true_iff. apply N.eqb_neq. lia.
Qed.

Lemma exact_ground_bytes_present :
  forall program tag body nodes span field rule,
    exact_ground_node_at tag nodes = Some (SourceScalar span field) ->
    generated_scalar_rule program tag = Some rule ->
    (exists bytes, scalar_semantic_from_source rule
      (source_body body (SourceScalar span field)) = Some (ScalarBytes bytes)) ->
    exists bytes, exact_ground_bytes_or_absent program tag body nodes = Some bytes.
Proof.
  intros program tag body nodes span field rule found rule_is [bytes semantic].
  unfold exact_ground_bytes_or_absent.
  rewrite found, rule_is, semantic.
  exists bytes. reflexivity.
Qed.

Lemma exact_ground_bytes_absent :
  forall program tag body nodes,
    exact_ground_node_at tag nodes = None ->
    exists bytes, exact_ground_bytes_or_absent program tag body nodes = Some bytes.
Proof.
  intros program tag body nodes found.
  unfold exact_ground_bytes_or_absent.
  rewrite found.
  exists []. reflexivity.
Qed.

Lemma exact_ground_u32_present :
  forall program tag body nodes span field rule,
    exact_ground_node_at tag nodes = Some (SourceScalar span field) ->
    generated_scalar_rule program tag = Some rule ->
    (exists value, scalar_semantic_from_source rule
      (source_body body (SourceScalar span field)) = Some (ScalarU32 value)) ->
    exists value, exact_ground_u32_or_absent program tag body nodes = Some value.
Proof.
  intros program tag body nodes span field rule found rule_is [value semantic].
  unfold exact_ground_u32_or_absent.
  rewrite found, rule_is, semantic.
  exists value. reflexivity.
Qed.

Lemma exact_ground_u32_absent :
  forall program tag body nodes,
    exact_ground_node_at tag nodes = None ->
    exists value, exact_ground_u32_or_absent program tag body nodes = Some value.
Proof.
  intros program tag body nodes found.
  unfold exact_ground_u32_or_absent.
  rewrite found.
  exists 0%N. reflexivity.
Qed.

(** A present bytes field with a nonempty payload reads back. *)
Lemma ground_bytes_of_node :
  forall program tag body nodes span field count start,
    exact_ground_node_at tag nodes = Some (SourceScalar span field) ->
    generated_scalar_rule program tag =
      Some (ScalarRule tag GeneratedData.PBBytes true) ->
    Wire.span_payload_start span = start ->
    Wire.span_end span = start + count ->
    0 < count ->
    start + count <= length body ->
    exists bytes, exact_ground_bytes_or_absent program tag body nodes = Some bytes.
Proof.
  intros program tag body nodes span field count start found rule_is
    start_eq end_eq positive bound.
  apply (exact_ground_bytes_present program tag body nodes span field _
    found rule_is).
  assert (payload_length :
    length (source_body body (SourceScalar span field)) = count).
  { pose proof (bounded_source_body_has_exact_length body
      (SourceScalar span field)) as exact_length.
    cbn [source_span] in exact_length.
    rewrite start_eq, end_eq in exact_length.
    specialize (exact_length ltac:(lia) bound).
    rewrite exact_length.
    lia. }
  exists (source_body body (SourceScalar span field)).
  apply bytes_source_semantic.
  rewrite payload_length.
  lia.
Qed.

(** A present uint32 field reads back from its canonical value. *)
Lemma ground_u32_of_node :
  forall program tag body nodes span field value payload,
    exact_ground_node_at tag nodes = Some (SourceScalar span field) ->
    generated_scalar_rule program tag =
      Some (ScalarRule tag GeneratedData.PBUint32 true) ->
    source_body body (SourceScalar span field) = payload ->
    compact_varint_value payload = Some value ->
    (0 < value)%N ->
    (value <= 4294967295)%N ->
    exists result, exact_ground_u32_or_absent program tag body nodes = Some result.
Proof.
  intros program tag body nodes span field value payload found rule_is
    payload_eq compact positive bounded.
  apply (exact_ground_u32_present program tag body nodes span field _
    found rule_is).
  rewrite payload_eq.
  exact (scalar_semantic_u32 tag payload
    (u32_source_valid tag payload value compact positive bounded)).
Qed.

(** ** The shapes of a re-read body *)

Lemma reread_body_matches_cons :
  forall field later byte rest,
    reread_body_matches (field :: later) (byte :: rest) =
    if byte =? reread_key field then
      match reread_field_tail field rest with
      | Some suffix => reread_body_matches later suffix
      | None => false
      end
    else reread_body_matches later (byte :: rest).
Proof. intros field later byte rest. reflexivity. Qed.

Lemma reread_two_cases :
  forall first second body,
    reread_body_matches [first; second] body = true ->
    body = [] \/
    (exists rest suffix,
      body = reread_key first :: rest /\
      reread_field_tail first rest = Some suffix /\
      reread_body_matches [second] suffix = true) \/
    (exists byte rest,
      body = byte :: rest /\
      byte <> reread_key first /\
      reread_body_matches [second] (byte :: rest) = true).
Proof.
  intros first second body matches.
  destruct body as [|byte rest].
  - left. reflexivity.
  - right.
    rewrite reread_body_matches_cons in matches.
    destruct (byte =? reread_key first) eqn:is_key.
    + left.
      apply Nat.eqb_eq in is_key.
      destruct (reread_field_tail first rest) as [suffix|] eqn:tail;
        [|discriminate matches].
      exists rest, suffix.
      split; [rewrite is_key; reflexivity|].
      split; [exact tail|exact matches].
    + right.
      apply Nat.eqb_neq in is_key.
      exists byte, rest.
      split; [reflexivity|].
      split; [exact is_key|exact matches].
Qed.

Lemma reread_one_cases :
  forall field suffix,
    reread_body_matches [field] suffix = true ->
    suffix = [] \/
    (exists rest,
      suffix = reread_key field :: rest /\
      reread_field_tail field rest = Some []).
Proof.
  intros field suffix matches.
  destruct suffix as [|byte rest].
  - left. reflexivity.
  - right.
    rewrite reread_body_matches_cons in matches.
    destruct (byte =? reread_key field) eqn:is_key.
    + apply Nat.eqb_eq in is_key.
      destruct (reread_field_tail field rest) as [suffix|] eqn:tail;
        [|discriminate matches].
      destruct suffix as [|x xs].
      * exists rest.
        split; [rewrite is_key; reflexivity|exact tail].
      * cbn [reread_body_matches] in matches.
        discriminate matches.
    + cbn [reread_body_matches] in matches.
      discriminate matches.
Qed.

Lemma suffix_nonempty_not_end :
  forall (whole : list nat) (stop : nat) (byte : nat) (rest : list nat),
    skipn stop whole = byte :: rest ->
    stop < length whole.
Proof.
  intros whole stop byte rest skipped.
  pose proof (f_equal (@length nat) skipped) as lengths.
  rewrite length_skipn in lengths.
  cbn [length] in lengths.
  lia.
Qed.

Lemma suffix_empty_at_end :
  forall (whole : list nat) (stop : nat),
    skipn stop whole = [] ->
    stop <= length whole ->
    stop = length whole.
Proof.
  intros whole stop skipped bound.
  pose proof (f_equal (@length nat) skipped) as lengths.
  rewrite length_skipn in lengths.
  cbn [length] in lengths.
  lia.
Qed.

(** ** Totality of the BigRational re-read *)

Lemma bigrat_ground_total :
  forall body,
    forallb Wire.byte_ok body = true ->
    reread_body_matches
      [RereadField 10 RereadBytes; RereadField 18 RereadBytes] body = true ->
    exists nodes numerator denominator,
      decode_region (S (length body)) body bigrat_program 0 (length body) =
        Some nodes /\
      exact_ground_bytes_or_absent bigrat_program 1 body nodes = Some numerator /\
      exact_ground_bytes_or_absent bigrat_program 2 body nodes = Some denominator.
Proof.
  intros body bytes_ok matches.
  destruct literal_key_10 as [limits_10 [number_10 wire_10]].
  destruct literal_key_18 as [limits_18 [number_18 wire_18]].
  destruct (reread_two_cases _ _ _ matches)
    as [body_empty | [[rest [suffix [body_eq [tail rest_matches]]]]
                     | [byte [rest [body_eq [not_key rest_matches]]]]]].
  - subst body.
    exists [], [], [].
    split; [reflexivity|].
    split; reflexivity.
  - cbn [reread_key reread_kind reread_field_tail] in body_eq, tail.
    destruct (bytes_field_checked body 0 0 10 rest suffix 1 body_eq
      ltac:(lia) limits_10 number_10 wire_10 tail)
      as [start1 [count1 [checked1 [above1 [positive1 [bound1 skip1]]]]]].
    destruct (reread_one_cases _ _ rest_matches)
      as [suffix_empty | [rest2 [suffix_eq tail2]]].
    + rewrite suffix_empty in skip1.
      pose proof (suffix_empty_at_end body (start1 + count1) skip1 bound1) as stop1.
      set (span1 := Wire.FieldSpan 1 Wire.WireLength 0 1 start1
        (start1 + count1) None) in *.
      assert (chain : field_chain body (length body) 0 0 [span1]).
      { cbn [field_chain].
        split; [lia|].
        split; [exact checked1|].
        exact stop1. }
      assert (parsed : Wire.worklist_region body 0 (length body) false =
          Wire.Parsed [span1]).
      { apply worklist_region_chain; [exact bytes_ok|exact chain|].
        cbn [length]. lia. }
      assert (decoded : decode_region (S (length body)) body bigrat_program 0
          (length body) = Some [SourceScalar span1 bigrat_numerator_field]).
      { rewrite (decode_region_parsed (length body) body bigrat_program 0
          (length body) _ parsed).
        reflexivity. }
      destruct (ground_bytes_of_node bigrat_program 1 body
        [SourceScalar span1 bigrat_numerator_field]
        span1 bigrat_numerator_field count1 start1 eq_refl bigrat_rule_one
        eq_refl eq_refl positive1 bound1) as [numerator read1].
      destruct (exact_ground_bytes_absent bigrat_program 2 body
        [SourceScalar span1 bigrat_numerator_field] eq_refl)
        as [denominator read2].
      exists [SourceScalar span1 bigrat_numerator_field], numerator, denominator.
      split; [exact decoded|].
      split; [exact read1|exact read2].
    + cbn [reread_key reread_kind reread_field_tail] in suffix_eq, tail2.
      rewrite suffix_eq in skip1.
      destruct (bytes_field_checked body (start1 + count1) 1 18 rest2 [] 2
        skip1 ltac:(lia) limits_18 number_18 wire_18 tail2)
        as [start2 [count2 [checked2 [above2 [positive2 [bound2 skip2]]]]]].
      pose proof (suffix_empty_at_end body (start2 + count2) skip2 bound2)
        as stop2.
      pose proof (suffix_nonempty_not_end body (start1 + count1) 18 rest2 skip1)
        as not_stop1.
      set (span1 := Wire.FieldSpan 1 Wire.WireLength 0 1 start1
        (start1 + count1) None) in *.
      set (span2 := Wire.FieldSpan 2 Wire.WireLength (start1 + count1)
        (S (start1 + count1)) start2 (start2 + count2) None) in *.
      assert (chain : field_chain body (length body) 0 0 [span1; span2]).
      { cbn [field_chain].
        split; [lia|].
        split; [exact checked1|].
        split; [cbn [Wire.span_end span1]; lia|].
        split; [exact checked2|].
        exact stop2. }
      assert (parsed : Wire.worklist_region body 0 (length body) false =
          Wire.Parsed [span1; span2]).
      { apply worklist_region_chain; [exact bytes_ok|exact chain|].
        cbn [length]. lia. }
      assert (decoded : decode_region (S (length body)) body bigrat_program 0
          (length body) = Some [SourceScalar span1 bigrat_numerator_field;
            SourceScalar span2 bigrat_denominator_field]).
      { rewrite (decode_region_parsed (length body) body bigrat_program 0
          (length body) _ parsed).
        reflexivity. }
      destruct (ground_bytes_of_node bigrat_program 1 body
        [SourceScalar span1 bigrat_numerator_field;
         SourceScalar span2 bigrat_denominator_field]
        span1 bigrat_numerator_field count1 start1 eq_refl bigrat_rule_one
        eq_refl eq_refl positive1 bound1) as [numerator read1].
      destruct (ground_bytes_of_node bigrat_program 2 body
        [SourceScalar span1 bigrat_numerator_field;
         SourceScalar span2 bigrat_denominator_field]
        span2 bigrat_denominator_field count2 start2 eq_refl bigrat_rule_two
        eq_refl eq_refl positive2 bound2) as [denominator read2].
      exists [SourceScalar span1 bigrat_numerator_field;
              SourceScalar span2 bigrat_denominator_field],
        numerator, denominator.
      split; [exact decoded|].
      split; [exact read1|exact read2].
  - destruct (reread_one_cases _ _ rest_matches)
      as [impossible | [rest2 [suffix_eq tail2]]];
      [discriminate impossible|].
    cbn [reread_key reread_kind reread_field_tail] in suffix_eq, tail2.
    assert (body_is : body = 18 :: rest2) by (rewrite body_eq; exact suffix_eq).
    destruct (bytes_field_checked body 0 0 18 rest2 [] 2 body_is
      ltac:(lia) limits_18 number_18 wire_18 tail2)
      as [start2 [count2 [checked2 [above2 [positive2 [bound2 skip2]]]]]].
    pose proof (suffix_empty_at_end body (start2 + count2) skip2 bound2)
      as stop2.
    set (span2 := Wire.FieldSpan 2 Wire.WireLength 0 1 start2
      (start2 + count2) None) in *.
    assert (chain : field_chain body (length body) 0 0 [span2]).
    { cbn [field_chain].
      split; [lia|].
      split; [exact checked2|].
      exact stop2. }
    assert (parsed : Wire.worklist_region body 0 (length body) false =
        Wire.Parsed [span2]).
    { apply worklist_region_chain; [exact bytes_ok|exact chain|].
      cbn [length]. lia. }
    assert (decoded : decode_region (S (length body)) body bigrat_program 0
        (length body) = Some [SourceScalar span2 bigrat_denominator_field]).
    { rewrite (decode_region_parsed (length body) body bigrat_program 0
        (length body) _ parsed).
      reflexivity. }
    destruct (exact_ground_bytes_absent bigrat_program 1 body
      [SourceScalar span2 bigrat_denominator_field] eq_refl)
      as [numerator read1].
    destruct (ground_bytes_of_node bigrat_program 2 body
      [SourceScalar span2 bigrat_denominator_field]
      span2 bigrat_denominator_field count2 start2 eq_refl bigrat_rule_two
      eq_refl eq_refl positive2 bound2) as [denominator read2].
    exists [SourceScalar span2 bigrat_denominator_field], numerator, denominator.
    split; [exact decoded|].
    split; [exact read1|exact read2].
Qed.

(** ** Totality of the FixedPoint re-read *)

Lemma fixed_point_ground_total :
  forall body,
    forallb Wire.byte_ok body = true ->
    reread_body_matches
      [RereadField 10 RereadBytes; RereadField 16 RereadU32] body = true ->
    exists nodes unscaled scale,
      decode_region (S (length body)) body fixed_point_program 0 (length body) =
        Some nodes /\
      exact_ground_bytes_or_absent fixed_point_program 1 body nodes = Some unscaled /\
      exact_ground_u32_or_absent fixed_point_program 2 body nodes = Some scale.
Proof.
  intros body bytes_ok matches.
  destruct literal_key_10 as [limits_10 [number_10 wire_10]].
  destruct (reread_two_cases _ _ _ matches)
    as [body_empty | [[rest [suffix [body_eq [tail rest_matches]]]]
                     | [byte [rest [body_eq [not_key rest_matches]]]]]].
  - subst body.
    exists [], [], 0%N.
    split; [reflexivity|].
    split; reflexivity.
  - cbn [reread_key reread_kind reread_field_tail] in body_eq, tail.
    destruct (bytes_field_checked body 0 0 10 rest suffix 1 body_eq
      ltac:(lia) limits_10 number_10 wire_10 tail)
      as [start1 [count1 [checked1 [above1 [positive1 [bound1 skip1]]]]]].
    destruct (reread_one_cases _ _ rest_matches)
      as [suffix_empty | [rest2 [suffix_eq tail2]]].
    + rewrite suffix_empty in skip1.
      pose proof (suffix_empty_at_end body (start1 + count1) skip1 bound1) as stop1.
      set (span1 := Wire.FieldSpan 1 Wire.WireLength 0 1 start1
        (start1 + count1) None) in *.
      assert (chain : field_chain body (length body) 0 0 [span1]).
      { cbn [field_chain].
        split; [lia|].
        split; [exact checked1|].
        exact stop1. }
      assert (parsed : Wire.worklist_region body 0 (length body) false =
          Wire.Parsed [span1]).
      { apply worklist_region_chain; [exact bytes_ok|exact chain|].
        cbn [length]. lia. }
      assert (decoded : decode_region (S (length body)) body
          fixed_point_program 0 (length body) =
          Some [SourceScalar span1 fixed_point_unscaled_field]).
      { rewrite (decode_region_parsed (length body) body fixed_point_program 0
          (length body) _ parsed).
        reflexivity. }
      destruct (ground_bytes_of_node fixed_point_program 1 body
        [SourceScalar span1 fixed_point_unscaled_field]
        span1 fixed_point_unscaled_field count1 start1 eq_refl
        fixed_point_rule_one eq_refl eq_refl positive1 bound1)
        as [unscaled read1].
      destruct (exact_ground_u32_absent fixed_point_program 2 body
        [SourceScalar span1 fixed_point_unscaled_field] eq_refl)
        as [scale read2].
      exists [SourceScalar span1 fixed_point_unscaled_field], unscaled, scale.
      split; [exact decoded|].
      split; [exact read1|exact read2].
    + cbn [reread_key reread_kind reread_field_tail] in suffix_eq, tail2.
      rewrite suffix_eq in skip1.
      destruct (u32_field_checked body (start1 + count1) 1 rest2 []
        skip1 tail2)
        as [value2 [end2 [checked2 [above2 [bound2 [skip2
          [compact2 [positive2 bounded2]]]]]]]].
      pose proof (suffix_empty_at_end body end2 skip2 bound2) as stop2.
      pose proof (suffix_nonempty_not_end body (start1 + count1) 16 rest2 skip1)
        as not_stop1.
      set (span1 := Wire.FieldSpan 1 Wire.WireLength 0 1 start1
        (start1 + count1) None) in *.
      set (span2 := Wire.FieldSpan 2 Wire.WireVarint (start1 + count1)
        (S (start1 + count1)) (S (start1 + count1)) end2 (Some value2)) in *.
      assert (chain : field_chain body (length body) 0 0 [span1; span2]).
      { cbn [field_chain].
        split; [lia|].
        split; [exact checked1|].
        split; [cbn [Wire.span_end span1]; lia|].
        split; [exact checked2|].
        exact stop2. }
      assert (parsed : Wire.worklist_region body 0 (length body) false =
          Wire.Parsed [span1; span2]).
      { apply worklist_region_chain; [exact bytes_ok|exact chain|].
        cbn [length]. lia. }
      assert (decoded : decode_region (S (length body)) body
          fixed_point_program 0 (length body) =
          Some [SourceScalar span1 fixed_point_unscaled_field;
                SourceScalar span2 fixed_point_scale_field]).
      { rewrite (decode_region_parsed (length body) body fixed_point_program 0
          (length body) _ parsed).
        reflexivity. }
      destruct (ground_bytes_of_node fixed_point_program 1 body
        [SourceScalar span1 fixed_point_unscaled_field;
         SourceScalar span2 fixed_point_scale_field]
        span1 fixed_point_unscaled_field count1 start1 eq_refl
        fixed_point_rule_one eq_refl eq_refl positive1 bound1)
        as [unscaled read1].
      destruct (ground_u32_of_node fixed_point_program 2 body
        [SourceScalar span1 fixed_point_unscaled_field;
         SourceScalar span2 fixed_point_scale_field]
        span2 fixed_point_scale_field value2 _ eq_refl fixed_point_rule_two
        eq_refl compact2 positive2 bounded2) as [scale read2].
      exists [SourceScalar span1 fixed_point_unscaled_field;
              SourceScalar span2 fixed_point_scale_field], unscaled, scale.
      split; [exact decoded|].
      split; [exact read1|exact read2].
  - destruct (reread_one_cases _ _ rest_matches)
      as [impossible | [rest2 [suffix_eq tail2]]];
      [discriminate impossible|].
    cbn [reread_key reread_kind reread_field_tail] in suffix_eq, tail2.
    assert (body_is : body = 16 :: rest2) by (rewrite body_eq; exact suffix_eq).
    destruct (u32_field_checked body 0 0 rest2 [] body_is tail2)
      as [value2 [end2 [checked2 [above2 [bound2 [skip2
        [compact2 [positive2 bounded2]]]]]]]].
    pose proof (suffix_empty_at_end body end2 skip2 bound2) as stop2.
    set (span2 := Wire.FieldSpan 2 Wire.WireVarint 0 1 1 end2 (Some value2)) in *.
    assert (chain : field_chain body (length body) 0 0 [span2]).
    { cbn [field_chain].
      split; [lia|].
      split; [exact checked2|].
      exact stop2. }
    assert (parsed : Wire.worklist_region body 0 (length body) false =
        Wire.Parsed [span2]).
    { apply worklist_region_chain; [exact bytes_ok|exact chain|].
      cbn [length]. lia. }
    assert (decoded : decode_region (S (length body)) body
        fixed_point_program 0 (length body) =
        Some [SourceScalar span2 fixed_point_scale_field]).
    { rewrite (decode_region_parsed (length body) body fixed_point_program 0
        (length body) _ parsed).
      reflexivity. }
    destruct (exact_ground_bytes_absent fixed_point_program 1 body
      [SourceScalar span2 fixed_point_scale_field] eq_refl)
      as [unscaled read1].
    destruct (ground_u32_of_node fixed_point_program 2 body
      [SourceScalar span2 fixed_point_scale_field]
      span2 fixed_point_scale_field value2 _ eq_refl fixed_point_rule_two
      eq_refl compact2 positive2 bounded2) as [scale read2].
    exists [SourceScalar span2 fixed_point_scale_field], unscaled, scale.
    split; [exact decoded|].
    split; [exact read1|exact read2].
Qed.

Lemma bigrat_parent_score_total :
  forall child,
    sem_type_id child = 25 ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    forallb (fun byte => byte <? 256) (sem_body child) = true ->
    reread_body_matches
      [RereadField 10 RereadBytes; RereadField 18 RereadBytes]
      (sem_body child) = true ->
    exists score, exact_bigrat_parent_score child = Some score.
Proof.
  intros child type_is wire_is payload_is not_refused bytes_ok matches.
  destruct (bigrat_ground_total (sem_body child) bytes_ok matches)
    as [nodes [numerator [denominator [decoded [read1 read2]]]]].
  unfold exact_bigrat_parent_score.
  rewrite type_is, wire_is, payload_is, not_refused.
  cbn [Nat.eqb wire_eqb payload_eqb negb andb].
  rewrite lookup_bigrat_program.
  cbv beta iota.
  rewrite decoded.
  cbv beta iota.
  rewrite read1, read2.
  eexists. reflexivity.
Qed.

Lemma fixed_point_parent_score_total :
  forall child,
    sem_type_id child = 26 ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    forallb (fun byte => byte <? 256) (sem_body child) = true ->
    reread_body_matches
      [RereadField 10 RereadBytes; RereadField 16 RereadU32]
      (sem_body child) = true ->
    exists score, exact_fixed_point_parent_score child = Some score.
Proof.
  intros child type_is wire_is payload_is not_refused bytes_ok matches.
  destruct (fixed_point_ground_total (sem_body child) bytes_ok matches)
    as [nodes [unscaled [scale [decoded [read1 read2]]]]].
  unfold exact_fixed_point_parent_score.
  rewrite type_is, wire_is, payload_is, not_refused.
  cbn [Nat.eqb wire_eqb payload_eqb negb andb].
  rewrite lookup_fixed_point_program.
  cbv beta iota.
  rewrite decoded.
  cbv beta iota.
  rewrite read1, read2.
  eexists. reflexivity.
Qed.

(** * E. The generated tables and the per-family results *)

Ltac table := vm_compute; reflexivity.

Definition tag_row (T tag : nat) :=
  option_map (fun d => (descriptor_wire d, descriptor_payload d,
      descriptor_child_type_id d))
    (resolve_tag (program_fields (generated_program_of T)) tag).

(** The classes whose required-score result is exact. *)
Definition requires_exact (class : score_class) : bool :=
  match class with
  | ClassPar | ClassProcessComponent | ClassExprComponent
  | ClassCostComposite | ClassCostPart | ClassNameComponent => true
  | _ => false
  end.

Lemma exact_evidence_true :
  forall child,
    exact_evidence child = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child evidence.
  unfold exact_evidence in evidence.
  destruct (sem_score_evidence child) as [score|]; [|discriminate evidence].
  exists score. reflexivity.
Qed.

Lemma class_ok_gives_exact :
  forall child,
    requires_exact (score_class_of (sem_type_id child)) = true ->
    class_result_ok child = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child required ok.
  apply exact_evidence_true.
  unfold class_result_ok in ok.
  revert required ok.
  destruct (score_class_of (sem_type_id child)); intros required ok;
    cbn [requires_exact] in required; try discriminate required;
    try exact ok;
    destruct (sem_complete child) as [[|]|]; try exact ok; discriminate ok.
Qed.

Lemma bind_payload_exact :
  forall child,
    sem_type_id child = 16 ->
    sem_bind_source_present child = true ->
    class_result_ok child = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child type_is present ok.
  apply exact_evidence_true.
  unfold class_result_ok, bind_payload_required_child_absent in ok.
  rewrite type_is, present in ok.
  change (score_class_of 16) with ClassBindPayload in ok.
  cbn [Nat.eqb negb orb] in ok.
  rewrite orb_false_r in ok.
  exact ok.
Qed.

Lemma case_payload_exact :
  forall child,
    sem_type_id child = 21 ->
    sem_case_required_present child = true ->
    class_result_ok child = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child type_is present ok.
  apply exact_evidence_true.
  unfold class_result_ok, bind_payload_required_child_absent in ok.
  rewrite type_is, present in ok.
  change (score_class_of 21) with ClassBindPayload in ok.
  cbn [Nat.eqb negb orb] in ok.
  rewrite orb_false_r in ok.
  exact ok.
Qed.

Definition expr_payload_classes (class : score_class) : bool :=
  match class with
  | ClassExprPayload | ClassPathMapPayload => true
  | _ => false
  end.

Lemma class_ok_payload_shape :
  forall c,
    expr_payload_classes (score_class_of (sem_type_id c)) = true ->
    class_result_ok c = true ->
    (sem_complete c = Some true /\
      exists score, sem_score_evidence c = ExactScore score) \/
    (sem_complete c = Some false /\ sem_score_evidence c = ScoreNotDerived).
Proof.
  intros c in_class ok.
  unfold class_result_ok in ok.
  revert in_class ok.
  destruct (score_class_of (sem_type_id c)); intros in_class ok;
    cbn [expr_payload_classes] in in_class; try discriminate in_class;
    destruct (sem_complete c) as [[|]|];
    destruct (sem_score_evidence c) as [s|];
    try discriminate ok;
    first [ (left; split; [reflexivity | eexists; reflexivity])
          | (right; split; reflexivity) ].
Qed.

(** Per-child readings of a generated row. *)
Lemma message_tag_facts :
  forall T tag child_type child,
    child_facts (generated_program_of T) child ->
    sem_tag child = tag ->
    tag_row T tag = Some (LengthDelimited, Message, Some child_type) ->
    sem_definitely_refused child = false /\
    sem_wire child = LengthDelimited /\
    sem_payload child = Message /\
    sem_type_id child = child_type /\
    class_result_ok child = true.
Proof.
  intros T tag child_type child facts tag_eq row.
  destruct (child_descriptor _ _ facts)
    as [d [resolved [wire_ok [payload_ok accepts]]]].
  rewrite tag_eq in resolved.
  unfold tag_row in row.
  rewrite resolved in row.
  cbn [option_map] in row.
  injection row as wire_is payload_is child_is.
  pose proof (wire_eqb_true_implies_equal _ _ wire_ok) as wire_same.
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite wire_is in wire_same.
  rewrite payload_is in payload_same.
  unfold descriptor_accepts_child_type in accepts.
  rewrite payload_is, child_is in accepts.
  apply Nat.eqb_eq in accepts.
  split; [exact (child_not_refused _ _ facts)|].
  split; [exact wire_same|].
  split; [exact payload_same|].
  split; [exact accepts|].
  exact (child_class_ok _ _ facts payload_same).
Qed.

Lemma scalar_tag_facts :
  forall T tag wire rule child,
    child_facts (generated_program_of T) child ->
    sem_tag child = tag ->
    tag_row T tag = Some (wire, Scalar, None) ->
    generated_scalar_rule (generated_program_of T) tag = Some rule ->
    sem_definitely_refused child = false /\
    sem_wire child = wire /\
    sem_payload child = Scalar /\
    scalar_body_valid rule (sem_body child) = true.
Proof.
  intros T tag wire rule child facts tag_eq row rule_is.
  destruct (child_descriptor _ _ facts)
    as [d [resolved [wire_ok [payload_ok _]]]].
  rewrite tag_eq in resolved.
  unfold tag_row in row.
  rewrite resolved in row.
  cbn [option_map] in row.
  injection row as wire_is payload_is _.
  pose proof (wire_eqb_true_implies_equal _ _ wire_ok) as wire_same.
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite wire_is in wire_same.
  rewrite payload_is in payload_same.
  destruct (child_scalar_rule _ _ facts payload_same)
    as [found_rule [found valid]].
  rewrite tag_eq, rule_is in found.
  injection found as same_rule.
  subst found_rule.
  split; [exact (child_not_refused _ _ facts)|].
  split; [exact wire_same|].
  split; [exact payload_same|].
  exact valid.
Qed.

Lemma child_tag_of_fields :
  forall T c,
    child_facts (generated_program_of T) c ->
    In (sem_tag c) (map descriptor_tag (program_fields (generated_program_of T))).
Proof.
  intros T c facts.
  destruct (child_descriptor _ _ facts) as [d [resolved _]].
  pose proof (resolve_tag_tag _ _ _ resolved) as tagged.
  rewrite <- tagged.
  apply in_map.
  exact (resolve_tag_member _ _ _ resolved).
Qed.

(** ** Family-level plumbing *)

Definition family_scan (T : nat) (cs : list semantic_summary) : scan_state :=
  fold_left (scan_child (generated_program_of T)) cs empty_scan.

Definition family_valid (T : nat) (cs : list semantic_summary) : Prop :=
  scan_valid (family_scan T cs) = true.

Definition family_facts (T : nat) (cs : list semantic_summary) : Prop :=
  forall c, In c cs -> child_facts (generated_program_of T) c.

Definition family_known_exact (T : nat) (cs : list semantic_summary) : Prop :=
  exists complete score,
    component_completeness T (family_scan T cs) cs = Some complete /\
    score_evidence_for T (Some complete) cs = ExactScore score.

Definition family_true_exact (T : nat) (cs : list semantic_summary) : Prop :=
  exists score,
    component_completeness T (family_scan T cs) cs = Some true /\
    score_evidence_for T (Some true) cs = ExactScore score.

Definition family_any_exact (T : nat) (cs : list semantic_summary) : Prop :=
  exists score,
    score_evidence_for T (component_completeness T (family_scan T cs) cs) cs =
      ExactScore score.

Lemma tag_present_of_slot :
  forall T cs slot tag,
    family_valid T cs ->
    seen_slot (family_scan T cs) slot = true ->
    slot_holds_only (generated_program_of T) slot tag = true ->
    tag_present tag cs.
Proof.
  intros T cs slot tag valid seen holds.
  exact (seen_slot_present_tag (generated_program_of T) cs slot tag
    valid seen holds).
Qed.

Lemma message_tag_exact :
  forall T tag child_type child,
    child_facts (generated_program_of T) child ->
    sem_tag child = tag ->
    tag_row T tag = Some (LengthDelimited, Message, Some child_type) ->
    requires_exact (score_class_of child_type) = true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros T tag child_type child facts tag_eq row required.
  destruct (message_tag_facts T tag child_type child facts tag_eq row)
    as [_ [_ [_ [type_is ok]]]].
  apply class_ok_gives_exact; [|exact ok].
  rewrite type_is. exact required.
Qed.

Lemma message_tag_typed_exact :
  forall T tag child_type child,
    child_facts (generated_program_of T) child ->
    sem_tag child = tag ->
    tag_row T tag = Some (LengthDelimited, Message, Some child_type) ->
    requires_exact (score_class_of child_type) = true ->
    exists score, exact_typed_message_score child_type child = Some score.
Proof.
  intros T tag child_type child facts tag_eq row required.
  destruct (message_tag_facts T tag child_type child facts tag_eq row)
    as [not_refused [wire_eq [payload_eq [type_is _]]]].
  destruct (message_tag_exact T tag child_type child facts tag_eq row required)
    as [score scored].
  exists score.
  exact (exact_typed_message_score_of child_type child score type_is wire_eq
    payload_eq not_refused scored).
Qed.

Lemma tag_exact_of_row :
  forall T tag child_type cs,
    family_facts T cs ->
    tag_row T tag = Some (LengthDelimited, Message, Some child_type) ->
    requires_exact (score_class_of child_type) = true ->
    tag_exact tag cs.
Proof.
  intros T tag child_type cs facts row required c member tag_eq.
  exact (message_tag_exact T tag child_type c (facts c member) tag_eq row
    required).
Qed.

Lemma tag_typed_exact_of_row :
  forall T tag child_type cs,
    family_facts T cs ->
    tag_row T tag = Some (LengthDelimited, Message, Some child_type) ->
    requires_exact (score_class_of child_type) = true ->
    tag_typed_exact tag child_type cs.
Proof.
  intros T tag child_type cs facts row required c member tag_eq.
  exact (message_tag_typed_exact T tag child_type c (facts c member) tag_eq row
    required).
Qed.

(** A StringMap tag is vacuous on unrefused children: an unrefused child has a
    Scalar or Message payload. *)
Lemma tag_exact_vacuous_auxiliary :
  forall T tag child_type cs,
    family_facts T cs ->
    tag_row T tag = Some (LengthDelimited, StringMap, child_type) ->
    tag_exact tag cs.
Proof.
  intros T tag child_type cs facts row c member tag_eq.
  exfalso.
  pose proof (facts c member) as c_facts.
  destruct (child_descriptor _ _ c_facts) as [d [resolved [_ [payload_ok _]]]].
  rewrite tag_eq in resolved.
  unfold tag_row in row.
  rewrite resolved in row.
  cbn [option_map] in row.
  injection row as _ payload_is _.
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite payload_is in payload_same.
  destruct (child_payload_kind _ _ c_facts) as [bad | bad];
    rewrite bad in payload_same; discriminate payload_same.
Qed.

Lemma bool_true_tag_ones :
  forall T tag cs,
    family_facts T cs ->
    tag_row T tag = Some (Varint, Scalar, None) ->
    generated_scalar_rule (generated_program_of T) tag =
      Some (ScalarRule tag GeneratedData.PBBool true) ->
    forall c, In c cs -> sem_tag c = tag ->
      sem_payload c = Scalar /\ sem_body c = [1].
Proof.
  intros T tag cs facts row rule_is c member tag_eq.
  destruct (scalar_tag_facts T tag Varint _ c (facts c member) tag_eq row
    rule_is) as [_ [_ [payload_is valid]]].
  split; [exact payload_is|].
  exact (bool_forbidden_body_is_one tag (sem_body c) valid).
Qed.

Lemma scalar_premise :
  forall T tag rule cs,
    family_facts T cs ->
    tag_row T tag = Some (Varint, Scalar, None) ->
    generated_scalar_rule (generated_program_of T) tag = Some rule ->
    forall c, In c cs -> sem_tag c = tag ->
      sem_wire c = Varint /\ sem_payload c = Scalar /\
      sem_definitely_refused c = false /\
      scalar_body_valid rule (sem_body c) = true.
Proof.
  intros T tag rule cs facts row rule_is c member tag_eq.
  destruct (scalar_tag_facts T tag Varint rule c (facts c member) tag_eq row
    rule_is) as [not_refused [wire_is [payload_is valid]]].
  repeat split; assumption.
Qed.

Lemma scalar_valid_of_row :
  forall T tag wire rule cs,
    family_facts T cs ->
    tag_row T tag = Some (wire, Scalar, None) ->
    generated_scalar_rule (generated_program_of T) tag = Some rule ->
    forall c, In c cs -> sem_tag c = tag ->
      scalar_body_valid rule (sem_body c) = true.
Proof.
  intros T tag wire rule cs facts row rule_is c member tag_eq.
  destruct (scalar_tag_facts T tag wire rule c (facts c member) tag_eq row
    rule_is) as [_ [_ [_ valid]]].
  exact valid.
Qed.

(** ** Send, type 15 *)

Lemma send_score_total :
  forall cs,
    family_valid 15 cs -> family_facts 15 cs ->
    seen_slot (family_scan 15 cs) 0 = true ->
    exists score, exact_send_score cs = Some score.
Proof.
  intros cs valid facts channel_seen.
  destruct (scalar_true_or_absent_total 3 cs
    (bool_true_tag_ones 15 3 cs facts ltac:(table) ltac:(table)))
    as [persistent persistent_eq].
  destruct (exact_child_score_total 1 cs
    (tag_present_of_slot 15 cs 0 1 valid channel_seen ltac:(table))
    (tag_exact_of_row 15 1 1 cs facts ltac:(table) ltac:(table)))
    as [channel channel_eq].
  destruct (exact_selected_scores_total 2 cs
    (tag_exact_of_row 15 2 1 cs facts ltac:(table) ltac:(table)))
    as [data data_eq].
  destruct (scalar_true_or_absent_total 6 cs
    (bool_true_tag_ones 15 6 cs facts ltac:(table) ltac:(table)))
    as [connective connective_eq].
  destruct (exact_optional_child_score_total 7 cs
    (tag_exact_of_row 15 7 12 cs facts ltac:(table) ltac:(table)))
    as [cost cost_eq].
  unfold exact_send_score.
  rewrite persistent_eq, channel_eq, data_eq, connective_eq, cost_eq.
  eexists. reflexivity.
Qed.

Lemma send_family :
  forall cs, family_valid 15 cs -> family_facts 15 cs -> family_known_exact 15 cs.
Proof.
  intros cs valid facts.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 15 cs) 0) eqn:channel_seen.
  - destruct (send_score_total cs valid facts channel_seen) as [score scored].
    exists true, score.
    split.
    + cbn [component_completeness]. rewrite channel_seen. reflexivity.
    + change (exact_or_unknown (exact_send_score cs) = ExactScore score).
      rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite channel_seen. reflexivity.
    + reflexivity.
Qed.

(** ** Bundle, type 8 *)

Lemma bundle_score_total :
  forall cs,
    family_valid 8 cs -> family_facts 8 cs ->
    seen_slot (family_scan 8 cs) 0 = true ->
    exists score, exact_bundle_score cs = Some score.
Proof.
  intros cs valid facts body_seen.
  destruct (find_tag_present 1 cs
    (tag_present_of_slot 8 cs 0 1 valid body_seen ltac:(table)))
    as [body found].
  destruct (find_tag_member 1 cs body found) as [member tag_eq].
  destruct (tag_exact_of_row 8 1 1 cs facts ltac:(table) ltac:(table)
    body member tag_eq) as [body_score scored].
  destruct (scalar_true_or_absent_total 2 cs
    (bool_true_tag_ones 8 2 cs facts ltac:(table) ltac:(table)))
    as [write write_eq].
  destruct (scalar_true_or_absent_total 3 cs
    (bool_true_tag_ones 8 3 cs facts ltac:(table) ltac:(table)))
    as [read read_eq].
  unfold exact_bundle_score.
  rewrite found, write_eq, read_eq.
  cbv beta iota.
  rewrite scored.
  eexists. reflexivity.
Qed.

Lemma bundle_family :
  forall cs, family_valid 8 cs -> family_facts 8 cs -> family_known_exact 8 cs.
Proof.
  intros cs valid facts.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 8 cs) 0) eqn:body_seen.
  - destruct (bundle_score_total cs valid facts body_seen) as [score scored].
    exists true, score.
    split.
    + cbn [component_completeness]. rewrite body_seen. reflexivity.
    + change ((match exact_bundle_score cs with
        | Some score => ExactScore score
        | None => ScoreNotDerived
        end) = ExactScore score).
      rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite body_seen. reflexivity.
    + reflexivity.
Qed.

(** ** New, type 20 *)

Lemma new_score_total :
  forall cs,
    family_valid 20 cs -> family_facts 20 cs ->
    seen_slot (family_scan 20 cs) 1 = true ->
    exists score, exact_new_score cs = Some score.
Proof.
  intros cs valid facts body_seen.
  destruct (new_bind_count_word_total cs
    (scalar_valid_of_row 20 1 Varint (ScalarRule 1 GeneratedData.PBSint32 true)
      cs facts ltac:(table) ltac:(table))) as [bind_count bind_eq].
  destruct (exact_child_score_total 2 cs
    (tag_present_of_slot 20 cs 1 2 valid body_seen ltac:(table))
    (tag_exact_of_row 20 2 1 cs facts ltac:(table) ltac:(table)))
    as [body body_eq].
  destruct (exact_selected_scores_total 4 cs
    (tag_exact_vacuous_auxiliary 20 4 (Some 1) cs facts ltac:(table)))
    as [injections injections_eq].
  unfold exact_new_score.
  rewrite bind_eq, body_eq, injections_eq.
  eexists. reflexivity.
Qed.

Lemma new_family :
  forall cs, family_valid 20 cs -> family_facts 20 cs -> family_known_exact 20 cs.
Proof.
  intros cs valid facts.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 20 cs) 1) eqn:body_seen.
  - destruct (new_score_total cs valid facts body_seen) as [score scored].
    exists true, score.
    split.
    + cbn [component_completeness]. rewrite body_seen. reflexivity.
    + change (exact_or_unknown (exact_new_score cs) = ExactScore score).
      rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite body_seen. reflexivity.
    + reflexivity.
Qed.

(** ** If, type 23 *)

Lemma if_score_total :
  forall cs,
    family_valid 23 cs -> family_facts 23 cs ->
    seen_slot (family_scan 23 cs) 0 = true ->
    seen_slot (family_scan 23 cs) 1 = true ->
    seen_slot (family_scan 23 cs) 2 = true ->
    exists score, exact_if_score cs = Some score.
Proof.
  intros cs valid facts first_seen second_seen third_seen.
  destruct (exact_child_score_total 1 cs
    (tag_present_of_slot 23 cs 0 1 valid first_seen ltac:(table))
    (tag_exact_of_row 23 1 1 cs facts ltac:(table) ltac:(table)))
    as [condition condition_eq].
  destruct (exact_child_score_total 2 cs
    (tag_present_of_slot 23 cs 1 2 valid second_seen ltac:(table))
    (tag_exact_of_row 23 2 1 cs facts ltac:(table) ltac:(table)))
    as [if_true if_true_eq].
  destruct (exact_child_score_total 3 cs
    (tag_present_of_slot 23 cs 2 3 valid third_seen ltac:(table))
    (tag_exact_of_row 23 3 1 cs facts ltac:(table) ltac:(table)))
    as [if_false if_false_eq].
  destruct (scalar_true_or_absent_total 5 cs
    (bool_true_tag_ones 23 5 cs facts ltac:(table) ltac:(table)))
    as [connective connective_eq].
  unfold exact_if_score.
  rewrite condition_eq, if_true_eq, if_false_eq, connective_eq.
  eexists. reflexivity.
Qed.

Lemma if_family :
  forall cs, family_valid 23 cs -> family_facts 23 cs -> family_known_exact 23 cs.
Proof.
  intros cs valid facts.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 23 cs) 0 && seen_slot (family_scan 23 cs) 1
    && seen_slot (family_scan 23 cs) 2) eqn:all_seen.
  - pose proof all_seen as all_true.
    apply andb_true_iff in all_true as [first_two third].
    apply andb_true_iff in first_two as [first second].
    destruct (if_score_total cs valid facts first second third) as [score scored].
    exists true, score.
    split.
    + cbn [component_completeness]. rewrite all_seen. reflexivity.
    + change (exact_or_unknown (exact_if_score cs) = ExactScore score).
      rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite all_seen. reflexivity.
    + reflexivity.
Qed.

(** ** Receive, type 19 *)

Lemma binds_typed_exact :
  forall cs,
    family_facts 19 cs ->
    all_bind_sources_present cs = true ->
    tag_typed_exact 1 16 cs.
Proof.
  intros cs facts binds_present c member tag_eq.
  pose proof (facts c member) as c_facts.
  destruct (message_tag_facts 19 1 16 c c_facts tag_eq ltac:(table))
    as [not_refused [wire_is [payload_is [type_is ok]]]].
  unfold all_bind_sources_present in binds_present.
  pose proof (proj1 (forallb_forall _ _) binds_present c member) as present.
  cbv beta in present.
  rewrite tag_eq, Nat.eqb_refl in present.
  destruct (bind_payload_exact c type_is present ok) as [score scored].
  exists score.
  exact (exact_typed_message_score_of 16 c score type_is wire_is payload_is
    not_refused scored).
Qed.

Lemma receive_score_total :
  forall cs,
    family_valid 19 cs -> family_facts 19 cs ->
    seen_slot (family_scan 19 cs) 1 = true ->
    all_bind_sources_present cs = true ->
    receive_has_present_default_condition cs = false ->
    exists score, exact_receive_score cs = Some score.
Proof.
  intros cs valid facts body_seen binds_present no_default.
  destruct (exact_bool_or_absent_total 3 cs
    (scalar_premise 19 3 (ScalarRule 3 GeneratedData.PBBool true) cs facts
      ltac:(table) ltac:(table))) as [persistent persistent_eq].
  destruct (exact_bool_or_absent_total 4 cs
    (scalar_premise 19 4 (ScalarRule 4 GeneratedData.PBBool true) cs facts
      ltac:(table) ltac:(table))) as [peek peek_eq].
  destruct (exact_typed_selected_scores_total 1 16 cs
    (binds_typed_exact cs facts binds_present)) as [binds binds_eq].
  destruct (exact_typed_child_score_total 2 1 cs
    (tag_present_of_slot 19 cs 1 2 valid body_seen ltac:(table))
    (tag_typed_exact_of_row 19 2 1 cs facts ltac:(table) ltac:(table)))
    as [body body_eq].
  destruct (exact_pbint32_or_absent_total 5 cs
    (scalar_premise 19 5 (ScalarRule 5 GeneratedData.PBInt32 true) cs facts
      ltac:(table) ltac:(table))) as [bind_count bind_count_eq].
  destruct (exact_bool_or_absent_total 7 cs
    (scalar_premise 19 7 (ScalarRule 7 GeneratedData.PBBool true) cs facts
      ltac:(table) ltac:(table))) as [connective connective_eq].
  destruct (exact_typed_optional_score_or_total 8 1 empty_par_score cs
    (tag_typed_exact_of_row 19 8 1 cs facts ltac:(table) ltac:(table)))
    as [condition condition_eq].
  destruct (exact_typed_optional_child_score_total 9 12 cs
    (tag_typed_exact_of_row 19 9 12 cs facts ltac:(table) ltac:(table)))
    as [cost cost_eq].
  unfold exact_receive_score.
  rewrite no_default, persistent_eq, peek_eq, binds_eq, body_eq, bind_count_eq,
    connective_eq, condition_eq, cost_eq.
  eexists. reflexivity.
Qed.

Lemma receive_family :
  forall cs,
    family_valid 19 cs -> family_facts 19 cs ->
    (component_completeness 19 (family_scan 19 cs) cs = Some true ->
      receive_has_present_default_condition cs = false) ->
    family_known_exact 19 cs.
Proof.
  intros cs valid facts default_absent.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 19 cs) 1 && all_bind_sources_present cs)
    eqn:all_true.
  - assert (completeness :
      component_completeness 19 (family_scan 19 cs) cs = Some true).
    { cbn [component_completeness]. rewrite all_true. reflexivity. }
    pose proof all_true as parts.
    apply andb_true_iff in parts as [body_seen binds_present].
    destruct (receive_score_total cs valid facts body_seen binds_present
      (default_absent completeness)) as [score scored].
    exists true, score.
    split; [exact completeness|].
    change (exact_or_unknown (exact_receive_score cs) = ExactScore score).
    rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite all_true. reflexivity.
    + reflexivity.
Qed.

(** ** Match, type 22 *)

Lemma cases_typed_exact :
  forall cs,
    family_facts 22 cs ->
    all_match_cases_required cs = true ->
    tag_typed_exact 2 21 cs.
Proof.
  intros cs facts cases_present c member tag_eq.
  pose proof (facts c member) as c_facts.
  destruct (message_tag_facts 22 2 21 c c_facts tag_eq ltac:(table))
    as [not_refused [wire_is [payload_is [type_is ok]]]].
  unfold all_match_cases_required in cases_present.
  pose proof (proj1 (forallb_forall _ _) cases_present c member) as present.
  cbv beta in present.
  rewrite tag_eq, Nat.eqb_refl in present.
  destruct (case_payload_exact c type_is present ok) as [score scored].
  exists score.
  exact (exact_typed_message_score_of 21 c score type_is wire_is payload_is
    not_refused scored).
Qed.

Lemma match_score_total :
  forall cs,
    family_valid 22 cs -> family_facts 22 cs ->
    seen_slot (family_scan 22 cs) 0 = true ->
    all_match_cases_required cs = true ->
    existsb match_case_has_present_default_guard cs = false ->
    exists score, exact_match_score cs = Some score.
Proof.
  intros cs valid facts target_seen cases_present no_default.
  destruct (exact_typed_child_score_total 1 1 cs
    (tag_present_of_slot 22 cs 0 1 valid target_seen ltac:(table))
    (tag_typed_exact_of_row 22 1 1 cs facts ltac:(table) ltac:(table)))
    as [target target_eq].
  destruct (exact_typed_selected_scores_total 2 21 cs
    (cases_typed_exact cs facts cases_present)) as [cases cases_eq].
  destruct (exact_bool_or_absent_total 5 cs
    (scalar_premise 22 5 (ScalarRule 5 GeneratedData.PBBool true) cs facts
      ltac:(table) ltac:(table))) as [connective connective_eq].
  unfold exact_match_score.
  rewrite no_default, target_eq, cases_eq, connective_eq.
  eexists. reflexivity.
Qed.

Lemma match_family :
  forall cs,
    family_valid 22 cs -> family_facts 22 cs ->
    (component_completeness 22 (family_scan 22 cs) cs = Some true ->
      existsb match_case_has_present_default_guard cs = false) ->
    family_known_exact 22 cs.
Proof.
  intros cs valid facts default_absent.
  unfold family_known_exact.
  destruct (seen_slot (family_scan 22 cs) 0 && all_match_cases_required cs)
    eqn:all_true.
  - assert (completeness :
      component_completeness 22 (family_scan 22 cs) cs = Some true).
    { cbn [component_completeness]. rewrite all_true. reflexivity. }
    pose proof all_true as parts.
    apply andb_true_iff in parts as [target_seen cases_present].
    destruct (match_score_total cs valid facts target_seen cases_present
      (default_absent completeness)) as [score scored].
    exists true, score.
    split; [exact completeness|].
    change (exact_or_unknown (exact_match_score cs) = ExactScore score).
    rewrite scored. reflexivity.
  - exists false, (score_constructor 2147483647 []).
    split.
    + cbn [component_completeness]. rewrite all_true. reflexivity.
    + reflexivity.
Qed.

(** ** Expr, type 24 *)

Definition singular_field (d : field_descriptor) : bool :=
  (descriptor_slot d =? 0) && negb (descriptor_repeated d).

Lemma singular_fields_of_forallb :
  forall fields,
    forallb singular_field fields = true ->
    forall d, In d fields ->
      descriptor_slot d = 0 /\ descriptor_repeated d = false.
Proof.
  intros fields holds d member.
  pose proof (proj1 (forallb_forall _ _) holds d member) as row.
  unfold singular_field in row.
  apply andb_prop in row as [slot repeated].
  apply Nat.eqb_eq in slot.
  apply negb_true_iff in repeated.
  split; assumption.
Qed.

Lemma expr_completeness_scalar :
  forall state c,
    sem_payload c = Scalar ->
    component_completeness 24 state [c] = Some true.
Proof.
  intros state c is_scalar.
  cbn [component_completeness].
  rewrite is_scalar.
  reflexivity.
Qed.

Lemma expr_completeness_message :
  forall state c,
    sem_payload c = Message ->
    component_completeness 24 state [c] = sem_complete c.
Proof.
  intros state c is_message.
  cbn [component_completeness].
  rewrite is_message.
  reflexivity.
Qed.

Definition expr_field_kind_ok (d : field_descriptor) : bool :=
  negb (payload_eqb (descriptor_payload d) Scalar) ||
  existsb (Nat.eqb (descriptor_tag d)) [1; 2; 3; 4; 25; 34; 35].

Lemma expr_scalar_tags :
  forall c,
    child_facts (generated_program_of 24) c ->
    sem_payload c = Scalar ->
    In (sem_tag c) [1; 2; 3; 4; 25; 34; 35].
Proof.
  intros c facts is_scalar.
  destruct (child_descriptor _ _ facts)
    as [d [resolved [_ [payload_ok _]]]].
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite is_scalar in payload_same.
  pose proof (resolve_tag_tag _ _ _ resolved) as tagged.
  pose proof (resolve_tag_member _ _ _ resolved) as member.
  assert (holds : forallb expr_field_kind_ok
    (program_fields (generated_program_of 24)) = true) by table.
  pose proof (proj1 (forallb_forall _ _) holds d member) as row.
  unfold expr_field_kind_ok in row.
  rewrite <- payload_same in row.
  cbn [payload_eqb negb orb] in row.
  apply existsb_exists in row as [x [x_member x_eq]].
  apply Nat.eqb_eq in x_eq.
  rewrite <- tagged, x_eq.
  exact x_member.
Qed.

Lemma expr_scalar_score_total :
  forall c,
    child_facts (generated_program_of 24) c ->
    sem_payload c = Scalar ->
    exists score, exact_expr_score [c] = Some score.
Proof.
  intros c facts is_scalar.
  destruct (expr_scalar_tags c facts is_scalar)
    as [t | [t | [t | [t | [t | [t | [t | []]]]]]]]; symmetry in t.
  - destruct (scalar_tag_facts 24 1 Varint
      (ScalarRule 1 GeneratedData.PBBool false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [_ valid]]].
    destruct (bool_default_body_is_small 1 (sem_body c) valid)
      as [body_eq | body_eq];
      unfold exact_expr_score; cbv beta iota;
      rewrite is_scalar, t, body_eq; eexists; reflexivity.
  - destruct (scalar_tag_facts 24 2 Varint
      (ScalarRule 2 GeneratedData.PBSint64 false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [_ valid]]].
    destruct (compact_of_valid_sint64 2 (sem_body c) valid) as [value compact].
    unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t, compact.
    eexists. reflexivity.
  - unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t.
    eexists. reflexivity.
  - unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t.
    eexists. reflexivity.
  - unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t.
    eexists. reflexivity.
  - destruct (scalar_tag_facts 24 34 Fixed64
      (ScalarRule 34 GeneratedData.PBFixed64 false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [_ valid]]].
    destruct (scalar_semantic_fixed64 34 (sem_body c) valid) as [bits semantic].
    unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t, semantic.
    eexists. reflexivity.
  - unfold exact_expr_score; cbv beta iota.
    rewrite is_scalar, t.
    eexists. reflexivity.
Qed.

(** Outside tags 36 and 37 the Expr parent takes the message child's score
    evidence verbatim. *)
Lemma exact_expr_other_tag :
  forall c,
    sem_payload c = Message ->
    sem_tag c <> 36 ->
    sem_tag c <> 37 ->
    exact_expr_score [c] =
      match sem_score_evidence c with
      | ExactScore score => Some score
      | ScoreNotDerived => None
      end.
Proof.
  intros c is_message not_36 not_37.
  unfold exact_expr_score.
  cbv beta iota.
  rewrite is_message.
  revert not_36 not_37.
  generalize (sem_tag c) as tag.
  intros tag not_36 not_37.
  do 38 (destruct tag as [|tag]; [first [reflexivity | (exfalso; lia)]|]).
  reflexivity.
Qed.

Definition expr_message_arm_ok (d : field_descriptor) : bool :=
  negb (payload_eqb (descriptor_payload d) Message) ||
  match descriptor_child_type_id d with
  | Some child_type =>
      if descriptor_tag d =? 36 then child_type =? 25
      else if descriptor_tag d =? 37 then child_type =? 26
      else expr_payload_classes (score_class_of child_type)
  | None => false
  end.

Lemma expr_message_child :
  forall c,
    child_facts (generated_program_of 24) c ->
    sem_payload c = Message ->
    (sem_tag c = 36 /\ sem_type_id c = 25) \/
    (sem_tag c = 37 /\ sem_type_id c = 26) \/
    (sem_tag c <> 36 /\ sem_tag c <> 37 /\
     expr_payload_classes (score_class_of (sem_type_id c)) = true).
Proof.
  intros c facts is_message.
  destruct (child_descriptor _ _ facts)
    as [d [resolved [_ [payload_ok accepts]]]].
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite is_message in payload_same.
  pose proof (resolve_tag_tag _ _ _ resolved) as tagged.
  assert (holds : forallb expr_message_arm_ok
    (program_fields (generated_program_of 24)) = true) by table.
  pose proof (proj1 (forallb_forall _ _) holds d
    (resolve_tag_member _ _ _ resolved)) as row.
  unfold expr_message_arm_ok in row.
  unfold descriptor_accepts_child_type in accepts.
  rewrite <- payload_same in row, accepts.
  cbn [payload_eqb negb orb] in row.
  cbv beta iota in accepts.
  revert row accepts.
  destruct (descriptor_child_type_id d) as [child_type|];
    intros row accepts; [|discriminate row].
  rewrite tagged in row.
  apply Nat.eqb_eq in accepts.
  destruct (sem_tag c =? 36) eqn:is_36.
  - try rewrite is_36 in row.
    cbv beta iota in row.
    apply Nat.eqb_eq in is_36.
    apply Nat.eqb_eq in row.
    left.
    split; [exact is_36|].
    rewrite accepts. exact row.
  - try rewrite is_36 in row.
    cbv beta iota in row.
    destruct (sem_tag c =? 37) eqn:is_37.
    + try rewrite is_37 in row.
      cbv beta iota in row.
      apply Nat.eqb_eq in is_37.
      apply Nat.eqb_eq in row.
      right; left.
      split; [exact is_37|].
      rewrite accepts. exact row.
    + try rewrite is_37 in row.
      cbv beta iota in row.
      apply Nat.eqb_neq in is_36.
      apply Nat.eqb_neq in is_37.
      right; right.
      split; [exact is_36|].
      split; [exact is_37|].
      rewrite accepts. exact row.
Qed.

Lemma expr_family :
  forall cs, family_valid 24 cs -> family_facts 24 cs -> family_known_exact 24 cs.
Proof.
  intros cs valid facts.
  unfold family_known_exact.
  pose proof (singular_program_at_most_one_child (generated_program_of 24) cs
    valid (singular_fields_of_forallb (program_fields (generated_program_of 24))
      ltac:(table))) as at_most_one.
  destruct cs as [|c rest].
  - exists true, (score_constructor 0 []).
    split; reflexivity.
  - destruct rest as [|second rest]; [|cbn [length] in at_most_one; lia].
    pose proof (facts c (or_introl eq_refl)) as c_facts.
    destruct (child_payload_kind _ _ c_facts) as [is_scalar | is_message].
    + destruct (expr_scalar_score_total c c_facts is_scalar) as [score scored].
      exists true, score.
      split.
      * exact (expr_completeness_scalar _ c is_scalar).
      * change (exact_or_unknown (exact_expr_score [c]) = ExactScore score).
        rewrite scored. reflexivity.
    + destruct (expr_message_child c c_facts is_message)
        as [[tag_eq type_is] | [[tag_eq type_is] | [not_36 [not_37 in_class]]]].
      * destruct (message_tag_facts 24 36 25 c c_facts tag_eq ltac:(table))
          as [not_refused [wire_is _]].
        destruct (child_complete_form _ _ c_facts) as [state [kids form]].
        assert (complete_true : sem_complete c = Some true)
          by (rewrite form, type_is; reflexivity).
        assert (reread_is : reread_fields_of (sem_type_id c) =
            Some [RereadField 10 RereadBytes; RereadField 18 RereadBytes])
          by (rewrite type_is; reflexivity).
        destruct (bigrat_parent_score_total c type_is wire_is is_message
          not_refused (child_message_bytes _ _ c_facts is_message)
          (child_reread _ _ c_facts is_message _ reread_is)) as [score scored].
        exists true, score.
        split.
        -- rewrite (expr_completeness_message _ c is_message). exact complete_true.
        -- change (exact_or_unknown (exact_expr_score [c]) = ExactScore score).
           unfold exact_expr_score; cbv beta iota.
           rewrite is_message, tag_eq, scored.
           reflexivity.
      * destruct (message_tag_facts 24 37 26 c c_facts tag_eq ltac:(table))
          as [not_refused [wire_is _]].
        destruct (child_complete_form _ _ c_facts) as [state [kids form]].
        assert (complete_true : sem_complete c = Some true)
          by (rewrite form, type_is; reflexivity).
        assert (reread_is : reread_fields_of (sem_type_id c) =
            Some [RereadField 10 RereadBytes; RereadField 16 RereadU32])
          by (rewrite type_is; reflexivity).
        destruct (fixed_point_parent_score_total c type_is wire_is is_message
          not_refused (child_message_bytes _ _ c_facts is_message)
          (child_reread _ _ c_facts is_message _ reread_is)) as [score scored].
        exists true, score.
        split.
        -- rewrite (expr_completeness_message _ c is_message). exact complete_true.
        -- change (exact_or_unknown (exact_expr_score [c]) = ExactScore score).
           unfold exact_expr_score; cbv beta iota.
           rewrite is_message, tag_eq, scored.
           reflexivity.
      * destruct (class_ok_payload_shape c in_class
          (child_class_ok _ _ c_facts is_message))
          as [[complete_true [score scored]] | [complete_false underived]].
        -- exists true, score.
           split.
           ++ rewrite (expr_completeness_message _ c is_message).
              exact complete_true.
           ++ change (exact_or_unknown (exact_expr_score [c]) = ExactScore score).
              rewrite (exact_expr_other_tag c is_message not_36 not_37), scored.
              reflexivity.
        -- exists false, (score_constructor 2147483647 []).
           split.
           ++ rewrite (expr_completeness_message _ c is_message).
              exact complete_false.
           ++ reflexivity.
Qed.

(** ** The cost classes *)

Lemma cost_stack_family :
  forall cs, family_facts 13 cs -> family_true_exact 13 cs.
Proof.
  intros cs facts.
  destruct (exact_selected_scores_total 1 cs
    (tag_exact_of_row 13 1 9 cs facts ltac:(table) ltac:(table)))
    as [scores scored].
  exists (score_constructor 508 scores).
  split.
  - reflexivity.
  - change (exact_or_unknown (exact_cost_collection_score 508 cs) =
      ExactScore (score_constructor 508 scores)).
    unfold exact_cost_collection_score.
    rewrite scored.
    reflexivity.
Qed.

Lemma cost_token_family :
  forall cs, family_facts 9 cs -> family_any_exact 9 cs.
Proof.
  intros cs facts.
  destruct (exact_selected_scores_total 1 cs
    (tag_exact_of_row 9 1 10 cs facts ltac:(table) ltac:(table)))
    as [scores scored].
  exists (score_constructor 501 scores).
  change (exact_or_unknown (exact_cost_collection_score 501 cs) =
    ExactScore (score_constructor 501 scores)).
  unfold exact_cost_collection_score.
  rewrite scored.
  reflexivity.
Qed.

Lemma cost_atom_field_tags :
  map descriptor_tag (program_fields (generated_program_of 10)) = [1; 2; 3; 4].
Proof. table. Qed.

Lemma cost_atom_score_total :
  forall c,
    child_facts (generated_program_of 10) c ->
    exists score, exact_cost_atom_score [c] = Some score.
Proof.
  intros c facts.
  pose proof (child_tag_of_fields 10 c facts) as tag_member.
  rewrite cost_atom_field_tags in tag_member.
  destruct tag_member as [t | [t | [t | [t | []]]]]; symmetry in t.
  - destruct (scalar_tag_facts 10 1 LengthDelimited
      (ScalarRule 1 GeneratedData.PBBytes false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [is_scalar _]]].
    unfold exact_cost_atom_score; cbv beta iota.
    rewrite t, is_scalar.
    eexists. reflexivity.
  - destruct (scalar_tag_facts 10 2 LengthDelimited
      (ScalarRule 2 GeneratedData.PBBytes false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [is_scalar _]]].
    unfold exact_cost_atom_score; cbv beta iota.
    rewrite t, is_scalar.
    eexists. reflexivity.
  - destruct (message_tag_exact 10 3 11 c facts t ltac:(table) ltac:(table))
      as [score scored].
    destruct (message_tag_facts 10 3 11 c facts t ltac:(table))
      as [_ [_ [is_message _]]].
    unfold exact_cost_atom_score; cbv beta iota.
    rewrite t, is_message.
    cbv beta iota.
    rewrite scored.
    eexists. reflexivity.
  - destruct (scalar_tag_facts 10 4 LengthDelimited
      (ScalarRule 4 GeneratedData.PBBytes false) c facts t
      ltac:(table) ltac:(table)) as [_ [_ [is_scalar _]]].
    unfold exact_cost_atom_score; cbv beta iota.
    rewrite t, is_scalar.
    eexists. reflexivity.
Qed.

Lemma cost_atom_family :
  forall cs, family_valid 10 cs -> family_facts 10 cs -> family_any_exact 10 cs.
Proof.
  intros cs valid facts.
  pose proof (singular_program_at_most_one_child (generated_program_of 10) cs
    valid (singular_fields_of_forallb (program_fields (generated_program_of 10))
      ltac:(table))) as at_most_one.
  destruct cs as [|c rest].
  - exists (score_constructor 505 []).
    reflexivity.
  - destruct rest as [|second rest]; [|cbn [length] in at_most_one; lia].
    destruct (cost_atom_score_total c (facts c (or_introl eq_refl)))
      as [score scored].
    exists score.
    change (exact_or_unknown (exact_cost_atom_score [c]) = ExactScore score).
    rewrite scored.
    reflexivity.
Qed.

Lemma cost_bound_family :
  forall cs, family_facts 11 cs -> family_any_exact 11 cs.
Proof.
  intros cs facts.
  destruct (exact_i32_or_absent_total 1 cs
    (scalar_valid_of_row 11 1 Varint (ScalarRule 1 GeneratedData.PBSint32 true)
      cs facts ltac:(table) ltac:(table))) as [index scored].
  exists (score_constructor 504 [ScoreInteger (signed_score_word index)]).
  change (exact_or_unknown (exact_cost_bound_score cs) =
    ExactScore (score_constructor 504 [ScoreInteger (signed_score_word index)])).
  unfold exact_cost_bound_score.
  rewrite scored.
  reflexivity.
Qed.

Lemma cost_region_score_total :
  forall cs, family_facts 12 cs -> exists score, exact_cost_region_score cs = Some score.
Proof.
  intros cs facts.
  destruct (exact_u32_or_absent_total 1 cs
    (scalar_valid_of_row 12 1 Varint (ScalarRule 1 GeneratedData.PBUint32 true)
      cs facts ltac:(table) ltac:(table))) as [version version_eq].
  destruct (exact_u64_or_absent_total 2 cs
    (scalar_valid_of_row 12 2 Varint (ScalarRule 2 GeneratedData.PBUint64 true)
      cs facts ltac:(table) ltac:(table))) as [start_line start_line_eq].
  destruct (exact_u64_or_absent_total 3 cs
    (scalar_valid_of_row 12 3 Varint (ScalarRule 3 GeneratedData.PBUint64 true)
      cs facts ltac:(table) ltac:(table))) as [start_column start_column_eq].
  destruct (exact_u64_or_absent_total 4 cs
    (scalar_valid_of_row 12 4 Varint (ScalarRule 4 GeneratedData.PBUint64 true)
      cs facts ltac:(table) ltac:(table))) as [end_line end_line_eq].
  destruct (exact_u64_or_absent_total 5 cs
    (scalar_valid_of_row 12 5 Varint (ScalarRule 5 GeneratedData.PBUint64 true)
      cs facts ltac:(table) ltac:(table))) as [end_column end_column_eq].
  destruct (exact_optional_child_score_total 6 cs
    (tag_exact_of_row 12 6 9 cs facts ltac:(table) ltac:(table)))
    as [signer signer_eq].
  destruct (exact_u32_or_absent_total 7 cs
    (scalar_valid_of_row 12 7 Varint (ScalarRule 7 GeneratedData.PBUint32 true)
      cs facts ltac:(table) ltac:(table))) as [ordinal ordinal_eq].
  unfold exact_cost_region_score.
  rewrite version_eq, start_line_eq, start_column_eq, end_line_eq, end_column_eq,
    signer_eq, ordinal_eq.
  eexists. reflexivity.
Qed.

Lemma cost_region_family :
  forall cs, family_facts 12 cs -> family_any_exact 12 cs.
Proof.
  intros cs facts.
  destruct (cost_region_score_total cs facts) as [score scored].
  exists score.
  change (exact_or_unknown (exact_cost_region_score cs) = ExactScore score).
  rewrite scored.
  reflexivity.
Qed.

(** ** Par, type 1

    This section, and only this section, reads the definition of
    [exact_par_component_scores]: the theorem below states the sole property of
    that function the rest of the theory uses, quantified over the generated
    Message tags of program 1 as a list.  In particular it does not depend on
    the order in which the Par families are concatenated.  The definition
    collects tagged component scores in child order
    ([exact_par_tagged_component_scores]) and only then arranges them in the
    production PAR_SCORE_TAGS order (C:140 canonical_semantics.rs; loop in
    reduce_raw_par, P:1549 protobuf_admission.rs), so the induction runs over
    the tagged collection, and the arrangement, being [option_map], preserves
    whether a result exists. *)

Definition par_program : generated_program := generated_program_of 1.

Definition par_score_family_tags : list nat :=
  map descriptor_tag
    (filter (fun d => payload_eqb (descriptor_payload d) Message)
      (program_fields par_program)).

Definition par_field_shaped (c : semantic_summary) : Prop :=
  exists d,
    resolve_tag (program_fields par_program) (sem_tag c) = Some d /\
    payload_eqb (sem_payload c) (descriptor_payload d) = true /\
    (sem_payload c = Scalar \/ sem_payload c = Message).

Definition par_tags_determine_payload : bool :=
  forallb (fun left =>
    forallb (fun right =>
      negb (descriptor_tag left =? descriptor_tag right) ||
      payload_eqb (descriptor_payload left) (descriptor_payload right))
      (program_fields par_program))
    (program_fields par_program).

Lemma par_tags_determine_payload_holds : par_tags_determine_payload = true.
Proof. table. Qed.

Lemma par_tag_family_iff_message :
  forall tag d,
    resolve_tag (program_fields par_program) tag = Some d ->
    (In tag par_score_family_tags <-> descriptor_payload d = Message).
Proof.
  intros tag d resolved.
  pose proof (resolve_tag_tag _ _ _ resolved) as tagged.
  pose proof (resolve_tag_member _ _ _ resolved) as member.
  split.
  - intro family.
    unfold par_score_family_tags in family.
    apply in_map_iff in family as [other [other_tag in_filter]].
    apply filter_In in in_filter as [other_member is_message].
    cbv beta in is_message.
    apply payload_eqb_true in is_message.
    pose proof par_tags_determine_payload_holds as holds.
    unfold par_tags_determine_payload in holds.
    pose proof (proj1 (forallb_forall _ _) holds d member) as row.
    cbv beta in row.
    pose proof (proj1 (forallb_forall _ _) row other other_member) as pair.
    cbv beta in pair.
    rewrite other_tag, tagged, Nat.eqb_refl in pair.
    cbn [negb orb] in pair.
    apply payload_eqb_true in pair.
    rewrite pair.
    exact is_message.
  - intro is_message.
    unfold par_score_family_tags.
    apply in_map_iff.
    exists d.
    split; [exact tagged|].
    apply filter_In.
    split; [exact member|].
    rewrite is_message.
    reflexivity.
Qed.

(** The tagged collection exists exactly when every child whose tag is a Par
    Message family has an exact score. *)
Theorem exact_par_tagged_component_scores_some_iff :
  forall cs,
    Forall par_field_shaped cs ->
    ((exists tagged, exact_par_tagged_component_scores cs = Some tagged) <->
     (forall tag, In tag par_score_family_tags ->
       forall c, In c cs -> sem_tag c = tag ->
         exists score, sem_score_evidence c = ExactScore score)).
Proof.
  intros cs shaped.
  induction cs as [|head rest IH].
  - split.
    + intros _ tag _ c member. destruct member.
    + intros _. eexists. reflexivity.
  - inversion shaped as [|h r shape_head shape_rest]; subst.
    specialize (IH shape_rest).
    destruct IH as [forward backward].
    destruct shape_head as [d [resolved [payload_ok kind]]].
    pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
    split.
    + intros [scores some_scores].
      cbn [exact_par_tagged_component_scores] in some_scores.
      destruct (exact_par_tagged_component_scores rest) as [rest_scores|] eqn:rest_eq;
        [|discriminate some_scores].
      specialize (forward (ex_intro _ rest_scores eq_refl)).
      intros tag family c [head_is_c | member] tag_eq.
      * subst c.
        rewrite tag_eq in resolved.
        assert (is_message : sem_payload head = Message).
        { rewrite payload_same.
          exact (proj1 (par_tag_family_iff_message tag d resolved) family). }
        rewrite is_message in some_scores.
        destruct (sem_score_evidence head) as [score|] eqn:scored;
          [|discriminate some_scores].
        exists score. reflexivity.
      * exact (forward tag family c member tag_eq).
    + intros scored_children.
      assert (rest_scored : forall tag, In tag par_score_family_tags ->
          forall c, In c rest -> sem_tag c = tag ->
            exists score, sem_score_evidence c = ExactScore score).
      { intros tag family c member tag_eq.
        exact (scored_children tag family c (or_intror member) tag_eq). }
      destruct (backward rest_scored) as [rest_scores rest_eq].
      cbn [exact_par_tagged_component_scores].
      rewrite rest_eq.
      destruct kind as [is_scalar | is_message].
      * rewrite is_scalar. eexists. reflexivity.
      * assert (family : In (sem_tag head) par_score_family_tags).
        { apply (proj2 (par_tag_family_iff_message (sem_tag head) d resolved)).
          rewrite <- payload_same. exact is_message. }
        destruct (scored_children (sem_tag head) family head
          (or_introl eq_refl) eq_refl) as [score head_scored].
        rewrite is_message, head_scored.
        eexists. reflexivity.
Qed.

(** The Par score: the component scores exist exactly when every child whose
    tag is a Par Message family has an exact score.  Arranging the tagged scores
    in family order is an [option_map], so it preserves existence. *)
Theorem exact_par_component_scores_some_iff :
  forall cs,
    Forall par_field_shaped cs ->
    ((exists scores, exact_par_component_scores cs = Some scores) <->
     (forall tag, In tag par_score_family_tags ->
       forall c, In c cs -> sem_tag c = tag ->
         exists score, sem_score_evidence c = ExactScore score)).
Proof.
  intros cs shaped.
  destruct (exact_par_tagged_component_scores_some_iff cs shaped)
    as [forward backward].
  unfold exact_par_component_scores.
  split.
  - intros [scores arranged].
    destruct (exact_par_tagged_component_scores cs) as [tagged|] eqn:collected.
    + exact (forward (ex_intro _ tagged eq_refl)).
    + discriminate arranged.
  - intro scored_children.
    destruct (backward scored_children) as [tagged collected].
    rewrite collected.
    eexists.
    reflexivity.
Qed.

Lemma par_children_shaped :
  forall cs, family_facts 1 cs -> Forall par_field_shaped cs.
Proof.
  intros cs facts.
  apply Forall_forall.
  intros c member.
  pose proof (facts c member) as c_facts.
  destruct (child_descriptor _ _ c_facts)
    as [d [resolved [_ [payload_ok _]]]].
  exists d.
  split; [exact resolved|].
  split; [exact payload_ok|].
  exact (child_payload_kind _ _ c_facts).
Qed.

Definition par_row_requires_exact (d : field_descriptor) : bool :=
  negb (payload_eqb (descriptor_payload d) Message) ||
  match descriptor_child_type_id d with
  | Some child_type => requires_exact (score_class_of child_type)
  | None => false
  end.

Lemma par_rows_require_exact :
  forallb par_row_requires_exact (program_fields par_program) = true.
Proof. table. Qed.

Lemma par_family_children_scored :
  forall cs, family_facts 1 cs ->
    forall tag, In tag par_score_family_tags ->
    forall c, In c cs -> sem_tag c = tag ->
      exists score, sem_score_evidence c = ExactScore score.
Proof.
  intros cs facts tag family c member tag_eq.
  pose proof (facts c member) as c_facts.
  destruct (child_descriptor _ _ c_facts)
    as [d [resolved [_ [payload_ok accepts]]]].
  assert (is_message_field : descriptor_payload d = Message).
  { apply (proj1 (par_tag_family_iff_message (sem_tag c) d resolved)).
    rewrite tag_eq. exact family. }
  pose proof (payload_eqb_true _ _ payload_ok) as payload_same.
  rewrite is_message_field in payload_same.
  pose proof (proj1 (forallb_forall _ _) par_rows_require_exact d
    (resolve_tag_member _ _ _ resolved)) as row.
  unfold par_row_requires_exact in row.
  rewrite is_message_field in row.
  cbn [payload_eqb negb orb] in row.
  unfold descriptor_accepts_child_type in accepts.
  rewrite is_message_field in accepts.
  cbv beta iota in accepts.
  revert row accepts.
  destruct (descriptor_child_type_id d) as [child_type|];
    intros row accepts; [|discriminate row].
  apply Nat.eqb_eq in accepts.
  apply class_ok_gives_exact.
  - rewrite accepts. exact row.
  - exact (child_class_ok _ _ c_facts payload_same).
Qed.

Lemma par_family :
  forall cs, family_facts 1 cs -> family_any_exact 1 cs.
Proof.
  intros cs facts.
  destruct (proj2 (exact_par_component_scores_some_iff cs
    (par_children_shaped cs facts)) (par_family_children_scored cs facts))
    as [scores scores_eq].
  destruct (scalar_true_or_absent_total 10 cs
    (bool_true_tag_ones 1 10 cs facts ltac:(table) ltac:(table)))
    as [connective connective_eq].
  exists (score_constructor 999
    (scores ++ [ScoreInteger (if connective then 1 else 0)])).
  change ((match exact_par_score cs with
    | Some score => ExactScore score
    | None => ScoreNotDerived
    end) = ExactScore (score_constructor 999
      (scores ++ [ScoreInteger (if connective then 1 else 0)]))).
  rewrite (exact_par_score_preserves_all_component_scores cs scores connective
    scores_eq connective_eq).
  reflexivity.
Qed.

(** * F. The main theorem and the red control *)

Lemma in_classes_type_ids :
  forall T,
    In (score_class_of T)
      [ClassPar; ClassProcessComponent; ClassExprComponent;
       ClassCostComposite; ClassCostPart] ->
    In T [1; 8; 15; 19; 20; 22; 23; 24; 13; 9; 10; 11; 12].
Proof.
  intro T.
  unfold score_class_of.
  destruct (Nat.eqb T 1) eqn:is_1.
  { intros _. apply Nat.eqb_eq in is_1. subst T. cbn [In]. tauto. }
  destruct (listed_score_type T [8; 15; 19; 20; 22; 23]) eqn:is_process.
  { intros _.
    apply listed_score_type_iff in is_process.
    cbn [In] in is_process |- *.
    intuition. }
  destruct (Nat.eqb T 24) eqn:is_24.
  { intros _. apply Nat.eqb_eq in is_24. subst T. cbn [In]. tauto. }
  destruct (Nat.eqb T 13) eqn:is_13.
  { intros _. apply Nat.eqb_eq in is_13. subst T. cbn [In]. tauto. }
  destruct (listed_score_type T [9; 10; 11; 12]) eqn:is_cost.
  { intros _.
    apply listed_score_type_iff in is_cost.
    cbn [In] in is_cost |- *.
    intuition. }
  destruct (listed_score_type T [16; 21]);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T [55; 60]);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T [31; 32]);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T expr_payload_type_ids);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T [34; 56; 57]);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T [7; 25; 26; 61; 62; 63; 64]);
    [intro member; cbn [In] in member; intuition discriminate|].
  destruct (listed_score_type T [2; 3; 4; 5; 14; 17; 18; 58; 59]);
    [intro member; cbn [In] in member; intuition discriminate|].
  intro member. cbn [In] in member. intuition discriminate.
Qed.

Lemma reduced_summary_equations :
  forall field cs program T,
    generated_wire_program field = Some program ->
    program_family program = T ->
    program = generated_program_of T ->
    sem_type_id (reduce_semantic_field field cs) = T /\
    sem_complete (reduce_semantic_field field cs) =
      component_completeness T (family_scan T cs) cs /\
    sem_score_evidence (reduce_semantic_field field cs) =
      score_evidence_for T (component_completeness T (family_scan T cs) cs) cs.
Proof.
  intros field cs program T resolved family program_eq.
  split; [|split].
  - rewrite (reduce_semantic_field_type_id field cs program resolved).
    exact family.
  - rewrite (reduce_semantic_field_complete field cs program resolved), family.
    rewrite program_eq.
    reflexivity.
  - rewrite (reduce_semantic_field_evidence field cs program resolved), family.
    rewrite program_eq.
    reflexivity.
Qed.

Lemma acceptable_of_known_exact :
  forall field cs program T class,
    generated_wire_program field = Some program ->
    program_family program = T ->
    program = generated_program_of T ->
    score_class_of T = class ->
    (class = ClassProcessComponent \/ class = ClassExprComponent) ->
    family_known_exact T cs ->
    class_result_ok (reduce_semantic_field field cs) = true.
Proof.
  intros field cs program T class resolved family program_eq class_eq class_is
    [complete [score [completeness scored]]].
  destruct (reduced_summary_equations field cs program T resolved family
    program_eq) as [type_eq [complete_eq evidence_eq]].
  unfold class_result_ok, exact_evidence.
  rewrite type_eq, class_eq.
  destruct class_is as [-> | ->]; cbv beta iota;
    rewrite complete_eq, evidence_eq, completeness, scored;
    reflexivity.
Qed.

Lemma acceptable_of_true_exact :
  forall field cs program T,
    generated_wire_program field = Some program ->
    program_family program = T ->
    program = generated_program_of T ->
    score_class_of T = ClassCostComposite ->
    family_true_exact T cs ->
    class_result_ok (reduce_semantic_field field cs) = true.
Proof.
  intros field cs program T resolved family program_eq class_eq
    [score [completeness scored]].
  destruct (reduced_summary_equations field cs program T resolved family
    program_eq) as [type_eq [complete_eq evidence_eq]].
  unfold class_result_ok, exact_evidence.
  rewrite type_eq, class_eq.
  cbv beta iota.
  rewrite complete_eq, evidence_eq, completeness, scored.
  reflexivity.
Qed.

Lemma acceptable_of_any_exact :
  forall field cs program T class,
    generated_wire_program field = Some program ->
    program_family program = T ->
    program = generated_program_of T ->
    score_class_of T = class ->
    (class = ClassPar \/ class = ClassCostPart) ->
    family_any_exact T cs ->
    class_result_ok (reduce_semantic_field field cs) = true.
Proof.
  intros field cs program T class resolved family program_eq class_eq class_is
    [score scored].
  destruct (reduced_summary_equations field cs program T resolved family
    program_eq) as [type_eq [_ evidence_eq]].
  unfold class_result_ok, exact_evidence.
  rewrite type_eq, class_eq.
  destruct class_is as [-> | ->]; cbv beta iota;
    rewrite evidence_eq, scored;
    reflexivity.
Qed.

Lemma family_premises :
  forall field cs program T,
    generated_wire_program field = Some program ->
    scan_valid (fold_left (scan_child program) cs empty_scan) = true ->
    Forall (child_facts program) cs ->
    program_family program = T ->
    T <> 0 ->
    program = generated_program_of T /\ family_valid T cs /\ family_facts T cs.
Proof.
  intros field cs program T resolved scan_ok child_forall family nonzero.
  pose proof (generated_program_is_lookup_of_its_family field program T
    resolved family nonzero) as program_eq.
  split; [exact program_eq|].
  split.
  - unfold family_valid, family_scan.
    rewrite <- program_eq.
    exact scan_ok.
  - unfold family_facts.
    rewrite <- program_eq.
    exact (proj1 (Forall_forall _ _) child_forall).
Qed.

Lemma receive_default_absent :
  forall cs program,
    program_family program = 19 ->
    program = generated_program_of 19 ->
    complete_receive_has_default_condition (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) cs empty_scan) cs) cs = false ->
    component_completeness 19 (family_scan 19 cs) cs = Some true ->
    receive_has_present_default_condition cs = false.
Proof.
  intros cs program family program_eq no_default complete.
  rewrite family, program_eq in no_default.
  change (fold_left (scan_child (generated_program_of 19)) cs empty_scan)
    with (family_scan 19 cs) in no_default.
  rewrite complete in no_default.
  exact no_default.
Qed.

Lemma match_default_absent :
  forall cs program,
    program_family program = 22 ->
    program = generated_program_of 22 ->
    complete_match_has_default_guard (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) cs empty_scan) cs) cs = false ->
    component_completeness 22 (family_scan 22 cs) cs = Some true ->
    existsb match_case_has_present_default_guard cs = false.
Proof.
  intros cs program family program_eq no_default complete.
  rewrite family, program_eq in no_default.
  change (fold_left (scan_child (generated_program_of 22)) cs empty_scan)
    with (family_scan 22 cs) in no_default.
  rewrite complete in no_default.
  exact no_default.
Qed.

Lemma family_acceptable :
  forall field cs program,
    generated_wire_program field = Some program ->
    scan_valid (fold_left (scan_child program) cs empty_scan) = true ->
    Forall (child_facts program) cs ->
    complete_receive_has_default_condition (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) cs empty_scan) cs) cs = false ->
    complete_match_has_default_guard (program_family program)
      (component_completeness (program_family program)
        (fold_left (scan_child program) cs empty_scan) cs) cs = false ->
    In (score_class_of (program_family program))
      [ClassPar; ClassProcessComponent; ClassExprComponent;
       ClassCostComposite; ClassCostPart] ->
    class_result_ok (reduce_semantic_field field cs) = true.
Proof.
  intros field cs program resolved scan_ok child_forall receive_ok match_ok
    class_member.
  destruct (in_classes_type_ids _ class_member)
    as [t | [t | [t | [t | [t | [t | [t | [t | [t | [t | [t | [t | [t | []]]]]]]]]]]]]].
  - destruct (family_premises field cs program 1 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [_ facts]].
    exact (acceptable_of_any_exact field cs program 1 ClassPar resolved
      (eq_sym t) program_eq eq_refl (or_introl eq_refl) (par_family cs facts)).
  - destruct (family_premises field cs program 8 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 8 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (bundle_family cs valid facts)).
  - destruct (family_premises field cs program 15 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 15 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (send_family cs valid facts)).
  - destruct (family_premises field cs program 19 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 19 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (receive_family cs valid facts
        (receive_default_absent cs program (eq_sym t) program_eq receive_ok))).
  - destruct (family_premises field cs program 20 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 20 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (new_family cs valid facts)).
  - destruct (family_premises field cs program 22 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 22 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (match_family cs valid facts
        (match_default_absent cs program (eq_sym t) program_eq match_ok))).
  - destruct (family_premises field cs program 23 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 23 ClassProcessComponent
      resolved (eq_sym t) program_eq eq_refl (or_introl eq_refl)
      (if_family cs valid facts)).
  - destruct (family_premises field cs program 24 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_known_exact field cs program 24 ClassExprComponent
      resolved (eq_sym t) program_eq eq_refl (or_intror eq_refl)
      (expr_family cs valid facts)).
  - destruct (family_premises field cs program 13 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [_ facts]].
    exact (acceptable_of_true_exact field cs program 13 resolved (eq_sym t)
      program_eq eq_refl (cost_stack_family cs facts)).
  - destruct (family_premises field cs program 9 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [_ facts]].
    exact (acceptable_of_any_exact field cs program 9 ClassCostPart resolved
      (eq_sym t) program_eq eq_refl (or_intror eq_refl)
      (cost_token_family cs facts)).
  - destruct (family_premises field cs program 10 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [valid facts]].
    exact (acceptable_of_any_exact field cs program 10 ClassCostPart resolved
      (eq_sym t) program_eq eq_refl (or_intror eq_refl)
      (cost_atom_family cs valid facts)).
  - destruct (family_premises field cs program 11 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [_ facts]].
    exact (acceptable_of_any_exact field cs program 11 ClassCostPart resolved
      (eq_sym t) program_eq eq_refl (or_intror eq_refl)
      (cost_bound_family cs facts)).
  - destruct (family_premises field cs program 12 resolved scan_ok child_forall
      (eq_sym t) ltac:(lia)) as [program_eq [_ facts]].
    exact (acceptable_of_any_exact field cs program 12 ClassCostPart resolved
      (eq_sym t) program_eq eq_refl (or_intror eq_refl)
      (cost_region_family cs facts)).
Qed.

(** The theorem of leaf L16.  Refused nodes satisfy it through the first
    disjunct of [reducer_result_acceptable]; a refused child refuses the
    parent through [scan_child], so the unrefused case has only unrefused
    children and every family proof applies. *)
Theorem process_and_cost_result_on_typed_valid :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
       [ClassPar; ClassProcessComponent; ClassExprComponent;
        ClassCostComposite; ClassCostPart] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children typed acceptable class_member.
  change (semantic_reduce (Node field children))
    with (reduce_semantic_field field (semantic_reduce_forest children))
    in class_member |- *.
  destruct (sem_definitely_refused
    (reduce_semantic_field field (semantic_reduce_forest children)))
    eqn:refused.
  - unfold reducer_result_acceptable.
    rewrite refused.
    reflexivity.
  - destruct (reduce_not_refused_parts field (semantic_reduce_forest children)
      refused) as [program [resolved [_ [scan_ok [receive_ok match_ok]]]]].
    rewrite (reduce_semantic_field_type_id field _ program resolved)
      in class_member.
    pose proof (not_refused_child_facts field children program typed
      acceptable resolved scan_ok) as child_forall.
    unfold reducer_result_acceptable.
    rewrite (family_acceptable field (semantic_reduce_forest children) program
      resolved scan_ok child_forall receive_ok match_ok class_member).
    apply orb_true_r.
Qed.

(** ** The red control

    The family Send is the witness.  [score_evidence_for_without_send] is
    [score_evidence_for] with one arm removed, the arm that scores a complete
    Send.  Replacing the evidence of a node by the mutant's leaves a
    typed-valid, unrefused, complete Send with no score, which the acceptance
    table rejects: the theorem fails for the mutant, by computation, so the
    theorem is sensitive to this one arm. *)

Definition score_evidence_for_without_send (type_id : nat)
    (complete : option bool) (children : list semantic_summary)
    : score_evidence :=
  match type_id, complete with
  | 15, Some true => ScoreNotDerived
  | _, _ => score_evidence_for type_id complete children
  end.

Definition reduce_with_evidence
    (evidence : nat -> option bool -> list semantic_summary -> score_evidence)
    (field : raw_field) (children : list semantic_summary) : semantic_summary :=
  let unmodified := reduce_semantic_field field children in
  let program :=
    match generated_wire_program field with
    | Some program => program
    | None => empty_program
    end in
  SemanticSummary (sem_tag unmodified) (sem_wire unmodified)
    (sem_payload unmodified) (sem_type_id unmodified) (sem_body unmodified)
    (sem_definitely_refused unmodified) (sem_complete unmodified)
    (evidence (program_family program) (sem_complete unmodified) children)
    (sem_bind_source_present unmodified) (sem_case_required_present unmodified).

Definition send_witness_children : @forest raw_field :=
  FCons (message_node 1 1 [] FNil) FNil.

Definition send_witness_field : raw_field :=
  tree_field (message_node 0 15 [10; 0] send_witness_children).

Definition send_witness : @tree raw_field :=
  Node send_witness_field send_witness_children.

Theorem send_witness_is_typed_valid : typed_valid_tree send_witness.
Proof. typed_valid_by_compute. Qed.

Theorem send_witness_child_is_acceptable :
  Forall_forest
    (fun child => reducer_result_acceptable (semantic_reduce child) = true)
    send_witness_children.
Proof.
  constructor.
  - vm_compute. reflexivity.
  - constructor.
Qed.

Theorem send_witness_class_is_listed :
  In (score_class_of
      (sem_type_id (reduce_with_evidence score_evidence_for_without_send
        send_witness_field (semantic_reduce_forest send_witness_children))))
    [ClassPar; ClassProcessComponent; ClassExprComponent;
     ClassCostComposite; ClassCostPart].
Proof.
  vm_compute.
  right. left. reflexivity.
Qed.

Theorem red_control_send_arm_missing_is_not_acceptable :
  reducer_result_acceptable
    (reduce_with_evidence score_evidence_for_without_send send_witness_field
      (semantic_reduce_forest send_witness_children)) = false.
Proof. vm_compute. reflexivity. Qed.

Theorem green_control_send_arm_present_is_acceptable :
  reducer_result_acceptable (semantic_reduce send_witness) = true.
Proof. vm_compute. reflexivity. Qed.

Theorem red_control_mutant_refutes_the_statement :
  ~ (forall field children,
      typed_valid_tree (Node field children) ->
      Forall_forest
        (fun child => reducer_result_acceptable (semantic_reduce child) = true)
        children ->
      In (score_class_of
          (sem_type_id (reduce_with_evidence score_evidence_for_without_send
            field (semantic_reduce_forest children))))
        [ClassPar; ClassProcessComponent; ClassExprComponent;
         ClassCostComposite; ClassCostPart] ->
      reducer_result_acceptable
        (reduce_with_evidence score_evidence_for_without_send field
          (semantic_reduce_forest children)) = true).
Proof.
  intro claim.
  pose proof (claim send_witness_field send_witness_children
    send_witness_is_typed_valid send_witness_child_is_acceptable
    send_witness_class_is_listed) as accepted.
  rewrite red_control_send_arm_missing_is_not_acceptable in accepted.
  discriminate accepted.
Qed.

(** * Assumption audit: every lemma and theorem of this theory. *)

Print Assumptions payload_eqb_true.
Print Assumptions resolve_tag_tag.
Print Assumptions scan_child_admissible_parts.
Print Assumptions scan_valid_fold_start.
Print Assumptions scan_fold_member_facts.
Print Assumptions fold_scan_seen_member.
Print Assumptions seen_slot_present_tag.
Print Assumptions singular_program_at_most_one_child.
Print Assumptions reduce_semantic_field_tag.
Print Assumptions reduce_semantic_field_wire.
Print Assumptions reduce_semantic_field_body.
Print Assumptions reduce_semantic_field_evidence.
Print Assumptions reduce_not_refused_parts.
Print Assumptions child_facts_of_tree.
Print Assumptions child_facts_forest.
Print Assumptions not_refused_child_facts.
Print Assumptions find_tag_present.
Print Assumptions find_tag_member.
Print Assumptions exact_child_score_total.
Print Assumptions exact_selected_scores_total.
Print Assumptions exact_optional_child_score_total.
Print Assumptions exact_typed_child_score_total.
Print Assumptions exact_typed_selected_scores_total.
Print Assumptions exact_typed_optional_child_score_total.
Print Assumptions exact_typed_optional_score_or_total.
Print Assumptions exact_typed_message_score_of.
Print Assumptions scalar_true_or_absent_total.
Print Assumptions scalar_semantic_bool.
Print Assumptions scalar_semantic_int32.
Print Assumptions scalar_semantic_sint32.
Print Assumptions scalar_semantic_u32.
Print Assumptions scalar_semantic_u64.
Print Assumptions scalar_semantic_fixed64.
Print Assumptions compact_of_valid_sint64.
Print Assumptions compact_of_valid_sint32_forbidden.
Print Assumptions exact_bool_or_absent_total.
Print Assumptions exact_pbint32_or_absent_total.
Print Assumptions exact_i32_or_absent_total.
Print Assumptions exact_u32_or_absent_total.
Print Assumptions exact_u64_or_absent_total.
Print Assumptions new_bind_count_word_total.
Print Assumptions skipn_cons_facts.
Print Assumptions varint_groups_from_agrees.
Print Assumptions varint_groups_from_prefix.
Print Assumptions compact_of_complete_prefix.
Print Assumptions first_group_guards.
Print Assumptions some_pair_injective.
Print Assumptions later_group_lower_bound.
Print Assumptions varint_groups_small_value.
Print Assumptions compact_varint_small.
Print Assumptions bool_forbidden_body_is_one.
Print Assumptions bool_default_body_is_small.
Print Assumptions varint_groups_literal_key.
Print Assumptions framing_length_field.
Print Assumptions framing_varint_field.
Print Assumptions checked_field_of_framing.
Print Assumptions bytes_field_checked.
Print Assumptions compact_varint_literal_key_16.
Print Assumptions literal_key_10.
Print Assumptions literal_key_18.
Print Assumptions u32_field_checked.
Print Assumptions reference_remaining_field_chain.
Print Assumptions region_ok_whole.
Print Assumptions worklist_region_chain.
Print Assumptions decode_region_parsed.
Print Assumptions lookup_bigrat_program.
Print Assumptions lookup_fixed_point_program.
Print Assumptions bigrat_rule_one.
Print Assumptions bigrat_rule_two.
Print Assumptions fixed_point_rule_one.
Print Assumptions fixed_point_rule_two.
Print Assumptions bytes_source_semantic.
Print Assumptions u32_source_valid.
Print Assumptions exact_ground_bytes_present.
Print Assumptions exact_ground_bytes_absent.
Print Assumptions exact_ground_u32_present.
Print Assumptions exact_ground_u32_absent.
Print Assumptions ground_bytes_of_node.
Print Assumptions ground_u32_of_node.
Print Assumptions reread_body_matches_cons.
Print Assumptions reread_two_cases.
Print Assumptions reread_one_cases.
Print Assumptions suffix_nonempty_not_end.
Print Assumptions suffix_empty_at_end.
Print Assumptions bigrat_ground_total.
Print Assumptions fixed_point_ground_total.
Print Assumptions bigrat_parent_score_total.
Print Assumptions fixed_point_parent_score_total.
Print Assumptions exact_evidence_true.
Print Assumptions class_ok_gives_exact.
Print Assumptions bind_payload_exact.
Print Assumptions case_payload_exact.
Print Assumptions class_ok_payload_shape.
Print Assumptions message_tag_facts.
Print Assumptions scalar_tag_facts.
Print Assumptions child_tag_of_fields.
Print Assumptions tag_present_of_slot.
Print Assumptions message_tag_exact.
Print Assumptions message_tag_typed_exact.
Print Assumptions tag_exact_of_row.
Print Assumptions tag_typed_exact_of_row.
Print Assumptions tag_exact_vacuous_auxiliary.
Print Assumptions bool_true_tag_ones.
Print Assumptions scalar_premise.
Print Assumptions scalar_valid_of_row.
Print Assumptions send_score_total.
Print Assumptions send_family.
Print Assumptions bundle_score_total.
Print Assumptions bundle_family.
Print Assumptions new_score_total.
Print Assumptions new_family.
Print Assumptions if_score_total.
Print Assumptions if_family.
Print Assumptions binds_typed_exact.
Print Assumptions receive_score_total.
Print Assumptions receive_family.
Print Assumptions cases_typed_exact.
Print Assumptions match_score_total.
Print Assumptions match_family.
Print Assumptions singular_fields_of_forallb.
Print Assumptions expr_completeness_scalar.
Print Assumptions expr_completeness_message.
Print Assumptions expr_scalar_tags.
Print Assumptions expr_scalar_score_total.
Print Assumptions exact_expr_other_tag.
Print Assumptions expr_message_child.
Print Assumptions expr_family.
Print Assumptions cost_stack_family.
Print Assumptions cost_token_family.
Print Assumptions cost_atom_field_tags.
Print Assumptions cost_atom_score_total.
Print Assumptions cost_atom_family.
Print Assumptions cost_bound_family.
Print Assumptions cost_region_score_total.
Print Assumptions cost_region_family.
Print Assumptions par_tags_determine_payload_holds.
Print Assumptions par_tag_family_iff_message.
Print Assumptions exact_par_tagged_component_scores_some_iff.
Print Assumptions exact_par_component_scores_some_iff.
Print Assumptions par_children_shaped.
Print Assumptions par_rows_require_exact.
Print Assumptions par_family_children_scored.
Print Assumptions par_family.
Print Assumptions in_classes_type_ids.
Print Assumptions reduced_summary_equations.
Print Assumptions acceptable_of_known_exact.
Print Assumptions acceptable_of_true_exact.
Print Assumptions acceptable_of_any_exact.
Print Assumptions family_premises.
Print Assumptions receive_default_absent.
Print Assumptions match_default_absent.
Print Assumptions family_acceptable.
Print Assumptions process_and_cost_result_on_typed_valid.
Print Assumptions send_witness_is_typed_valid.
Print Assumptions send_witness_child_is_acceptable.
Print Assumptions send_witness_class_is_listed.
Print Assumptions red_control_send_arm_missing_is_not_acceptable.
Print Assumptions green_control_send_arm_present_is_acceptable.
Print Assumptions red_control_mutant_refutes_the_statement.
