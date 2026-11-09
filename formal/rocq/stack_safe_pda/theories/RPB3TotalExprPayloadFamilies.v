(** * Leaf L18: totality of the semantic reducer on the ExprPayload families
    and the parent carriers.

    The ExprPayload class is program types 6, 27, 28, 29, 30, 33 and 35..54
    (Var, EList, ETuple, ESet, EMap, EMethod, EVar, ENot, ENeg and the binary
    operators); the parent carriers are 34, 56 and 57.

    Citations below: P is models/src/rust/rholang/protobuf_admission.rs and C
    is models/src/rust/rholang/canonical_semantics.rs.

    Result in one paragraph.  Every family except EMap (type 30) is total from
    the statement's own premises, so the statement is proved for it with the
    single added premise that the node is not an EMap:
    [expr_payload_result_on_typed_valid_except_emap].  EMap is the exception:
    the statement's premises do not settle it.  Its score reads the key score
    of each KeyValuePair child (P:3183 pushes [key.score] of each pair, and
    P:487-488 with P:521-522 show a present key's score is never absent in
    Rust), the key is a grandchild of the EMap, and the KeyValuePair child is
    a parent carrier whose acceptance is unconditional, so the child premise
    carries no information about the key.  [emap_local_step_is_false] refutes
    the local step on summaries, and [emap_key_premise_is_necessary] shows that
    every complete acceptable EMap consumes the pair key score.  The bare
    statement at EMap is neither proved nor refuted here: a typed-valid
    counterexample would need a key Par without exact evidence, and a proof
    would need the totality of Par, which the tree induction establishes.  The
    strengthened statement with the premise that each pair's key is acceptable
    is [expr_payload_result_on_typed_valid_with_carrier_keys], and it needs the
    premise only at EMap.  The tree induction can supply that premise from a
    motive that also returns the acceptance of the node's children, through
    [kvp_keys_acceptable_of_grandchildren_acceptable]. *)

From Stdlib Require Import List Bool Arith NArith Lia.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3SourceScalarReduction RPB3SemanticReducer RPB3ScoreClass RPB3TypedValid.
From StackSafePDA Require RPB3WireCursor RPB3DescriptorProjectionData.

Import ListNotations.

(** ** Elementary facts over tags, descriptors and the scan *)

Lemma payload_eqb_true_implies_equal :
  forall one other,
    payload_eqb one other = true -> one = other.
Proof.
  intros one other same.
  destruct one, other; try reflexivity; discriminate same.
Qed.

Lemma resolve_tag_tag :
  forall fields tag descriptor,
    resolve_tag fields tag = Some descriptor ->
    descriptor_tag descriptor = tag.
Proof.
  induction fields as [|field rest rest_tag]; intros tag descriptor resolved.
  - discriminate resolved.
  - cbn [resolve_tag] in resolved.
    destruct (descriptor_tag field =? tag) eqn:same.
    + injection resolved as same_descriptor.
      subst descriptor.
      apply Nat.eqb_eq.
      exact same.
    + exact (rest_tag tag descriptor resolved).
Qed.

(** What one valid scan step yields, including the wire equality that
    [scan_child_stays_valid_only_if] (RPB3TypedValid.v) does not export and the
    non-repeated uniqueness clause. *)
Lemma scan_child_valid_facts :
  forall program state child next,
    scan_child program state child = next ->
    scan_valid next = true ->
    exists descriptor,
      scan_valid state = true /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      sem_definitely_refused child = false /\
      sem_wire child = descriptor_wire descriptor /\
      sem_payload child = descriptor_payload descriptor /\
      descriptor_accepts_child_type descriptor child = true /\
      (descriptor_repeated descriptor = true \/
        seen_slot state (descriptor_slot descriptor) = false) /\
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
  split; [exact (wire_eqb_true_implies_equal _ _ wire_ok)|].
  split; [exact (payload_eqb_true_implies_equal _ _ payload_ok)|].
  split; [exact type_ok|].
  split.
  - apply orb_prop in unique_ok as [repeated|fresh].
    + left. exact repeated.
    + right. exact (proj1 (negb_true_iff _) fresh).
  - reflexivity.
Qed.

Lemma fold_scan_valid_prefix :
  forall program children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    scan_valid state = true.
Proof.
  intros program children.
  induction children as [|child rest rest_prefix]; intros state valid.
  - exact valid.
  - cbn [fold_left] in valid.
    pose proof (rest_prefix (scan_child program state child) valid) as next_valid.
    destruct (scan_child_valid_facts program state child _ eq_refl next_valid)
      as [descriptor [state_valid _]].
    exact state_valid.
Qed.

(** A child the scan accepted fits the descriptor of its tag. *)
Definition child_fits (program : generated_program)
    (child : semantic_summary) : Prop :=
  exists descriptor,
    resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
    sem_definitely_refused child = false /\
    sem_wire child = descriptor_wire descriptor /\
    sem_payload child = descriptor_payload descriptor /\
    descriptor_accepts_child_type descriptor child = true.

Lemma fold_scan_children_fit :
  forall program children state,
    scan_valid (fold_left (scan_child program) children state) = true ->
    Forall (child_fits program) children.
Proof.
  intros program children.
  induction children as [|child rest rest_fit]; intros state valid.
  - constructor.
  - cbn [fold_left] in valid.
    constructor.
    + pose proof (fold_scan_valid_prefix program rest _ valid) as next_valid.
      destruct (scan_child_valid_facts program state child _ eq_refl next_valid)
        as [descriptor [_ [resolved [not_refused [wire_ok [payload_ok [type_ok _]]]]]]].
      exists descriptor.
      exact (conj resolved (conj not_refused (conj wire_ok (conj payload_ok type_ok)))).
    + exact (rest_fit _ valid).
Qed.

Lemma fold_scan_seen_source :
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
  induction children as [|child rest rest_source]; intros state valid seen.
  - left. exact seen.
  - cbn [fold_left] in valid, seen.
    destruct (rest_source (scan_child program state child) valid seen)
      as [seen_before|[later [descriptor [member [resolved same_slot]]]]].
    + pose proof (fold_scan_valid_prefix program rest _ valid) as next_valid.
      destruct (scan_child_valid_facts program state child _ eq_refl next_valid)
        as [descriptor [_ [resolved [_ [_ [_ [_ [_ seen_after]]]]]]]].
      unfold seen_slot in seen_before.
      rewrite seen_after in seen_before.
      cbn [existsb] in seen_before.
      apply orb_prop in seen_before as [here|earlier].
      * right. exists child, descriptor.
        split; [left; reflexivity|].
        split; [exact resolved|].
        apply Nat.eqb_eq in here.
        symmetry. exact here.
      * left. exact earlier.
    + right. exists later, descriptor.
      split; [right; exact member|].
      split; [exact resolved|exact same_slot].
Qed.

(** A program whose every descriptor shares slot 0 and is non-repeated (Var,
    EVar's operand) admits at most one child. *)
Lemma two_children_of_single_slot_are_refused :
  forall program first second rest,
    (forall descriptor,
      In descriptor (program_fields program) ->
      descriptor_slot descriptor = 0 /\ descriptor_repeated descriptor = false) ->
    scan_valid
      (fold_left (scan_child program) (first :: second :: rest) empty_scan) = false.
Proof.
  intros program first second rest shape.
  destruct (scan_valid
    (fold_left (scan_child program) (first :: second :: rest) empty_scan))
    eqn:valid; [|reflexivity].
  exfalso.
  cbn [fold_left] in valid.
  pose proof (fold_scan_valid_prefix program rest _ valid) as second_valid.
  destruct (scan_child_valid_facts program
    (scan_child program empty_scan first) second _ eq_refl second_valid)
    as [second_desc [first_valid [second_resolved
        [_ [_ [_ [_ [second_fresh _]]]]]]]].
  destruct (scan_child_valid_facts program empty_scan first _ eq_refl first_valid)
    as [first_desc [_ [first_resolved [_ [_ [_ [_ [_ first_seen]]]]]]]].
  destruct (shape second_desc (resolve_tag_member _ _ _ second_resolved))
    as [second_slot second_single].
  destruct (shape first_desc (resolve_tag_member _ _ _ first_resolved))
    as [first_slot _].
  destruct second_fresh as [repeated|fresh].
  - congruence.
  - unfold seen_slot in fresh.
    rewrite first_seen, second_slot, first_slot in fresh.
    cbn in fresh.
    discriminate fresh.
Qed.

(** ** Slot ownership: a seen slot names a child with the owning tag *)

Definition slot_owned_by (program : generated_program) (slot tag : nat) : bool :=
  forallb (fun descriptor =>
    negb (descriptor_slot descriptor =? slot) || (descriptor_tag descriptor =? tag))
    (program_fields program).

Lemma seen_slot_gives_tagged_child :
  forall program slot tag children,
    slot_owned_by program slot tag = true ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    seen_slot (fold_left (scan_child program) children empty_scan) slot = true ->
    exists child, In child children /\ sem_tag child = tag.
Proof.
  intros program slot tag children owned valid seen.
  destruct (fold_scan_seen_source program slot children empty_scan valid seen)
    as [impossible|[child [descriptor [member [resolved same_slot]]]]].
  - change (seen_slot empty_scan slot) with false in impossible.
    discriminate impossible.
  - exists child.
    split; [exact member|].
    pose proof (proj1 (forallb_forall _ _) owned descriptor
      (resolve_tag_member _ _ _ resolved)) as checked.
    cbv beta in checked.
    rewrite same_slot, Nat.eqb_refl in checked.
    cbn [negb orb] in checked.
    apply Nat.eqb_eq in checked.
    pose proof (resolve_tag_tag _ _ _ resolved) as tag_matches.
    congruence.
Qed.

(** ** The tree and forest projections *)

Definition tree_children (subject : @tree raw_field) : @forest raw_field :=
  match subject with
  | Node _ children => children
  end.

Lemma Forall_forest_inv :
  forall (P : @tree raw_field -> Prop) head rest,
    Forall_forest P (FCons head rest) ->
    P head /\ Forall_forest P rest.
Proof.
  intros P head rest all.
  inversion all; subst.
  split; assumption.
Qed.

Lemma Forall_forest_and :
  forall (P Q : @tree raw_field -> Prop) subjects,
    Forall_forest P subjects ->
    Forall_forest Q subjects ->
    Forall_forest (fun subject => P subject /\ Q subject) subjects.
Proof.
  intros P Q subjects.
  induction subjects as [|head rest rest_and]; intros all_p all_q.
  - constructor.
  - destruct (Forall_forest_inv P head rest all_p) as [p_here p_rest].
    destruct (Forall_forest_inv Q head rest all_q) as [q_here q_rest].
    constructor.
    + split; assumption.
    + exact (rest_and p_rest q_rest).
Qed.

Lemma forest_member_facts :
  forall (P : @tree raw_field -> Prop) subjects child,
    Forall_forest P subjects ->
    In child (semantic_reduce_forest subjects) ->
    exists subject,
      child = semantic_reduce subject /\
      P subject /\
      In (tree_field subject) (forest_fields subjects).
Proof.
  intros P subjects.
  induction subjects as [|head rest rest_member]; intros child all member.
  - cbn [semantic_reduce_forest In] in member.
    destruct member.
  - destruct (Forall_forest_inv P head rest all) as [here later].
    cbn [semantic_reduce_forest In] in member.
    destruct member as [at_head|at_rest].
    + exists head.
      split; [symmetry; exact at_head|].
      split; [exact here|].
      left. reflexivity.
    + destruct (rest_member child later at_rest)
        as [subject [same [holds in_fields]]].
      exists subject.
      split; [exact same|].
      split; [exact holds|].
      right. exact in_fields.
Qed.

(** ** Summary projections of the reducer *)

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

Lemma unrefused_gives_scan_valid :
  forall field children,
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    exists program,
      generated_wire_program field = Some program /\
      scan_valid (fold_left (scan_child program) children empty_scan) = true.
Proof.
  intros field children not_refused.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - exists program.
    split; [reflexivity|].
    destruct (scan_valid (fold_left (scan_child program) children empty_scan))
      eqn:valid; [reflexivity|].
    rewrite (invalid_scan_is_refused field children program resolved valid)
      in not_refused.
    discriminate not_refused.
  - rewrite (no_program_field_is_refused_by_the_current_reducer
      field children resolved) in not_refused.
    discriminate not_refused.
Qed.

(** ** The class table on the ExprPayload ids and the carriers *)

Lemma score_class_of_expr_payload :
  forall type_id,
    score_class_of type_id = ClassExprPayload ->
    In type_id expr_payload_type_ids.
Proof.
  intros type_id class.
  unfold score_class_of in class.
  destruct (Nat.eqb type_id 1); [discriminate class|].
  destruct (listed_score_type type_id [8; 15; 19; 20; 22; 23]); [discriminate class|].
  destruct (Nat.eqb type_id 24); [discriminate class|].
  destruct (Nat.eqb type_id 13); [discriminate class|].
  destruct (listed_score_type type_id [9; 10; 11; 12]); [discriminate class|].
  destruct (listed_score_type type_id [16; 21]); [discriminate class|].
  destruct (listed_score_type type_id [55; 60]); [discriminate class|].
  destruct (listed_score_type type_id [31; 32]); [discriminate class|].
  destruct (listed_score_type type_id expr_payload_type_ids) eqn:listed.
  - exact (proj1 (listed_score_type_iff _ _) listed).
  - destruct (listed_score_type type_id [34; 56; 57]); [discriminate class|].
    destruct (listed_score_type type_id [7; 25; 26; 61; 62; 63; 64]);
      [discriminate class|].
    destruct (listed_score_type type_id [2; 3; 4; 5; 14; 17; 18; 58; 59]);
      discriminate class.
Qed.

Lemma score_class_of_expr_payload_member :
  forall type_id,
    In type_id expr_payload_type_ids ->
    score_class_of type_id = ClassExprPayload.
Proof.
  intros type_id member.
  assert (checked :
    forallb (fun id => score_class_eqb (score_class_of id) ClassExprPayload)
      expr_payload_type_ids = true) by (vm_compute; reflexivity).
  pose proof (proj1 (forallb_forall _ _) checked type_id member) as same.
  cbv beta in same.
  unfold score_class_eqb in same.
  destruct (score_class_of type_id); try discriminate same; reflexivity.
Qed.

Lemma expr_payload_id_nonzero :
  forall type_id,
    In type_id expr_payload_type_ids -> type_id <> 0.
Proof.
  intros type_id member zero.
  subst type_id.
  assert (checked :
    forallb (fun id => negb (id =? 0)) expr_payload_type_ids = true)
    by (vm_compute; reflexivity).
  pose proof (proj1 (forallb_forall _ _) checked 0 member) as bad.
  vm_compute in bad.
  discriminate bad.
Qed.

Lemma acceptable_of_class_ok :
  forall summary,
    class_result_ok summary = true ->
    reducer_result_acceptable summary = true.
Proof.
  intros summary ok.
  unfold reducer_result_acceptable.
  rewrite ok, orb_true_r.
  reflexivity.
Qed.

(** ** Exact evidence of an acceptable Par child and an acceptable Var child *)

Lemma acceptable_par_has_exact_evidence :
  forall child,
    reducer_result_acceptable child = true ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 1 ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child acceptable not_refused payload_ok type_ok.
  unfold reducer_result_acceptable in acceptable.
  rewrite not_refused, payload_ok in acceptable.
  cbn [orb payload_eqb] in acceptable.
  unfold class_result_ok in acceptable.
  rewrite type_ok in acceptable.
  change (score_class_of 1) with ClassPar in acceptable.
  cbv beta iota in acceptable.
  unfold exact_evidence in acceptable.
  destruct (sem_score_evidence child) as [score|].
  - exists score. reflexivity.
  - discriminate acceptable.
Qed.

Lemma acceptable_var_has_exact_evidence :
  forall child,
    reducer_result_acceptable child = true ->
    sem_definitely_refused child = false ->
    sem_payload child = Message ->
    sem_type_id child = 6 ->
    sem_complete child = Some true ->
    exists score, sem_score_evidence child = ExactScore score.
Proof.
  intros child acceptable not_refused payload_ok type_ok complete.
  unfold reducer_result_acceptable in acceptable.
  rewrite not_refused, payload_ok in acceptable.
  cbn [orb payload_eqb] in acceptable.
  unfold class_result_ok in acceptable.
  rewrite type_ok, complete in acceptable.
  change (score_class_of 6) with ClassExprPayload in acceptable.
  cbv beta iota in acceptable.
  destruct (sem_score_evidence child) as [score|].
  - exists score. reflexivity.
  - discriminate acceptable.
Qed.

Lemma exact_typed_message_score_of_fit :
  forall type_id child,
    sem_type_id child = type_id ->
    sem_wire child = LengthDelimited ->
    sem_payload child = Message ->
    sem_definitely_refused child = false ->
    exact_typed_message_score type_id child =
      match sem_score_evidence child with
      | ExactScore score => Some score
      | ScoreNotDerived => None
      end.
Proof.
  intros type_id child type_ok wire_ok payload_ok not_refused.
  unfold exact_typed_message_score.
  rewrite type_ok, wire_ok, payload_ok, not_refused, Nat.eqb_refl.
  reflexivity.
Qed.

(** A Var's completeness is unconditional: every reduced summary of type 6
    reports [Some true], so the class table leaves it no scoreless row. *)
Lemma reduced_var_is_complete :
  forall subject,
    sem_type_id (semantic_reduce subject) = 6 ->
    sem_complete (semantic_reduce subject) = Some true.
Proof.
  intros [field children] type_ok.
  cbn [semantic_reduce] in type_ok |- *.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - rewrite (reduce_semantic_field_type_id field _ program resolved) in type_ok.
    rewrite (reduce_semantic_field_complete field _ program resolved).
    rewrite type_ok.
    reflexivity.
  - exfalso.
    unfold reduce_semantic_field in type_ok.
    rewrite resolved in type_ok.
    discriminate type_ok.
Qed.

(** ** Canonical varints and the two scalar rules used by the payload families

    A forbidden-default bool scalar must be encoded as the single byte 1, and a
    sint32 rule always yields a level.  The first is what lets the connective
    flag [scalar_true_or_absent] succeed from the typed-validity scalar
    contract (TV2). *)

(** [injection] normalizes the components it extracts, which rewrites
    [byte mod 128] into [Nat.divmod] form; this lemma keeps them as stated. *)
Lemma some_pair_injective :
  forall (A B : Type) (a other_a : A) (b other_b : B),
    Some (a, b) = Some (other_a, other_b) -> a = other_a /\ b = other_b.
Proof.
  intros A B a other_a b other_b same.
  inversion same; subst.
  split; reflexivity.
Qed.

(** One unfolding of the varint loop.  It is stated with [byte mod 128] intact
    so the arithmetic rewrites below apply; [cbn] and [injection] both expose
    the numeral computation inside [Nat.modulo]. *)
Lemma varint_groups_unfold :
  forall rest bytes region_end offset group value,
    RPB3WireCursor.varint_groups (S rest) bytes region_end offset group value =
    if offset <? region_end then
      match nth_error bytes offset with
      | None => None
      | Some byte =>
          if RPB3WireCursor.byte_ok byte then
            if (group =? 9) && (1 <? byte) then None
            else
              if byte <? 128 then
                if (0 <? group) && (byte =? 0) then None
                else Some ((value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N,
                  S offset)
              else RPB3WireCursor.varint_groups rest bytes region_end (S offset)
                (S group) (value + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N
          else None
      end
    else None.
Proof. intros rest bytes region_end offset group value. reflexivity. Qed.

Lemma varint_groups_later_group_is_large :
  forall fuel bytes region_end offset group value result stop,
    (1 <= group)%nat ->
    RPB3WireCursor.varint_groups fuel bytes region_end offset group value =
      Some (result, stop) ->
    (128 <= result)%N.
Proof.
  induction fuel as [|rest rest_large];
    intros bytes region_end offset group value result stop later parsed.
  - discriminate parsed.
  - rewrite varint_groups_unfold in parsed.
    destruct (offset <? region_end); [|discriminate parsed].
    destruct (nth_error bytes offset) as [byte|]; [|discriminate parsed].
    destruct (RPB3WireCursor.byte_ok byte); [|discriminate parsed].
    destruct ((group =? 9) && (1 <? byte)); [discriminate parsed|].
    destruct (byte <? 128) eqn:small.
    + destruct ((0 <? group) && (byte =? 0)) eqn:zero; [discriminate parsed|].
      destruct (some_pair_injective _ _ _ _ _ _ parsed) as [same_result same_stop].
      subst result.
      assert (group_positive : (0 <? group) = true) by (apply Nat.ltb_lt; lia).
      rewrite group_positive in zero.
      cbn [andb] in zero.
      apply Nat.eqb_neq in zero.
      apply Nat.ltb_lt in small.
      rewrite (Nat.mod_small byte 128 small).
      pose proof (N.pow_le_mono_r 128 1 (N.of_nat group)
        ltac:(lia) ltac:(lia)) as power.
      change (128 ^ 1)%N with 128%N in power.
      assert (one_byte : (1 <= N.of_nat byte)%N) by lia.
      assert (scaled : (1 * 128 <= N.of_nat byte * 128 ^ N.of_nat group)%N)
        by (apply N.mul_le_mono; assumption).
      lia.
    + exact (rest_large bytes region_end (S offset) (S group) _ result stop
        (le_S _ _ later) parsed).
Qed.

Lemma compact_varint_one_is_singleton :
  forall body,
    compact_varint_value body = Some 1%N ->
    body = [1].
Proof.
  intros body parsed.
  unfold compact_varint_value, RPB3WireCursor.canonical_varint in parsed.
  destruct body as [|byte rest].
  - discriminate parsed.
  - cbn [length] in parsed.
    destruct (RPB3WireCursor.varint_groups 10 (byte :: rest)
      (S (length rest)) 0 0 0) as [[value stop]|] eqn:groups;
      [|discriminate parsed].
    destruct (stop =? S (length rest)) eqn:at_end; [|discriminate parsed].
    injection parsed as value_one.
    subst value.
    apply Nat.eqb_eq in at_end.
    rewrite varint_groups_unfold in groups.
    destruct (0 <? S (length rest)); [|discriminate groups].
    change (nth_error (byte :: rest) 0) with (Some byte) in groups.
    cbv iota in groups.
    destruct (RPB3WireCursor.byte_ok byte); [|discriminate groups].
    destruct ((0 =? 9) && (1 <? byte)); [discriminate groups|].
    destruct (byte <? 128) eqn:small.
    + destruct ((0 <? 0) && (byte =? 0)); [discriminate groups|].
      destruct (some_pair_injective _ _ _ _ _ _ groups) as [value_one same_stop].
      apply Nat.ltb_lt in small.
      rewrite (Nat.mod_small byte 128 small) in value_one.
      change (N.of_nat 0) with 0%N in value_one.
      change (128 ^ 0)%N with 1%N in value_one.
      assert (byte_one : byte = 1) by lia.
      subst byte.
      destruct rest as [|extra rest].
      * reflexivity.
      * cbn [length] in at_end.
        lia.
    + exfalso.
      pose proof (varint_groups_later_group_is_large _ _ _ _ _ _ _ _
        (le_n 1) groups) as large.
      lia.
Qed.

Lemma bool_forbidden_rule_body_is_one :
  forall rule body,
    scalar_kind rule = RPB3DescriptorProjectionData.GeneratedData.PBBool ->
    scalar_default_forbidden rule = true ->
    scalar_body_valid rule body = true ->
    body = [1].
Proof.
  intros rule body kind forbidden valid.
  unfold scalar_body_valid in valid.
  cbv zeta in valid.
  rewrite kind, forbidden in valid.
  cbv beta iota in valid.
  destruct (compact_varint_value body) as [value|] eqn:parsed;
    [|discriminate valid].
  unfold numeric_scalar_valid in valid.
  cbv beta iota zeta in valid.
  apply andb_prop in valid as [at_most_one nondefault].
  apply orb_prop in nondefault as [impossible|nondefault].
  - discriminate impossible.
  - apply negb_true_iff in nondefault.
    apply N.eqb_neq in nondefault.
    apply N.leb_le in at_most_one.
    assert (value_one : value = 1%N) by lia.
    subst value.
    exact (compact_varint_one_is_singleton body parsed).
Qed.

Lemma sint32_rule_yields_i32 :
  forall tag forbidden body,
    scalar_body_valid
      (ScalarRule tag RPB3DescriptorProjectionData.GeneratedData.PBSint32 forbidden)
      body = true ->
    exists level,
      scalar_semantic_from_source
        (ScalarRule tag RPB3DescriptorProjectionData.GeneratedData.PBSint32 forbidden)
        body = Some (ScalarI32 level).
Proof.
  intros tag forbidden body valid.
  unfold scalar_semantic_from_source.
  rewrite valid.
  cbn [scalar_kind].
  unfold scalar_body_valid in valid.
  cbn [scalar_kind scalar_default_forbidden] in valid.
  destruct (compact_varint_value body) as [value|]; [|discriminate valid].
  eexists.
  reflexivity.
Qed.

(** ** Decidable descriptor shapes, checked by computation per family *)

Definition tag_is_message (program : generated_program)
    (tag child_type : nat) : bool :=
  match resolve_tag (program_fields program) tag with
  | Some descriptor =>
      wire_eqb (descriptor_wire descriptor) LengthDelimited &&
      payload_eqb (descriptor_payload descriptor) Message &&
      match descriptor_child_type_id descriptor with
      | Some id => id =? child_type
      | None => false
      end
  | None => false
  end.

Definition tag_is_ld_scalar (program : generated_program) (tag : nat) : bool :=
  match resolve_tag (program_fields program) tag with
  | Some descriptor =>
      wire_eqb (descriptor_wire descriptor) LengthDelimited &&
      payload_eqb (descriptor_payload descriptor) Scalar
  | None => false
  end.

Definition forbidden_bool_flag (program : generated_program) (tag : nat) : bool :=
  match resolve_tag (program_fields program) tag,
      generated_scalar_rule program tag with
  | Some descriptor, Some rule =>
      payload_eqb (descriptor_payload descriptor) Scalar &&
      match scalar_kind rule with
      | RPB3DescriptorProjectionData.GeneratedData.PBBool =>
          scalar_default_forbidden rule
      | _ => false
      end
  | _, _ => false
  end.

Lemma child_fits_message :
  forall program child tag child_type,
    child_fits program child ->
    sem_tag child = tag ->
    tag_is_message program tag child_type = true ->
    sem_definitely_refused child = false /\
    sem_wire child = LengthDelimited /\
    sem_payload child = Message /\
    sem_type_id child = child_type.
Proof.
  intros program child tag child_type
    [descriptor [resolved [not_refused [wire_ok [payload_ok type_ok]]]]]
    same_tag shape.
  subst tag.
  unfold tag_is_message in shape.
  rewrite resolved in shape.
  apply andb_prop in shape as [first_two type_match].
  apply andb_prop in first_two as [wire_shape payload_shape].
  pose proof (wire_eqb_true_implies_equal _ _ wire_shape) as wire_value.
  pose proof (payload_eqb_true_implies_equal _ _ payload_shape) as payload_value.
  destruct (descriptor_child_type_id descriptor) as [id|] eqn:child_id;
    [|discriminate type_match].
  cbv beta iota in type_match.
  apply Nat.eqb_eq in type_match.
  subst id.
  unfold descriptor_accepts_child_type in type_ok.
  rewrite payload_value, child_id in type_ok.
  cbv beta iota in type_ok.
  apply Nat.eqb_eq in type_ok.
  split; [exact not_refused|].
  split; [rewrite wire_ok; exact wire_value|].
  split; [rewrite payload_ok; exact payload_value|].
  exact type_ok.
Qed.

(** ** The node context: what a typed-valid unrefused node gives its families

    [node_context] bundles the four things every family proof needs and that
    do not depend on the family: the children fit the descriptors of the
    program, each child is acceptable, each Var child is complete, and each
    scalar child's body is accepted by its generated rule. *)

Record node_context (program : generated_program)
    (children : list semantic_summary) : Prop := NodeContext {
  context_fits : Forall (child_fits program) children;
  context_acceptable :
    forall child, In child children -> reducer_result_acceptable child = true;
  context_var_complete :
    forall child, In child children ->
      sem_type_id child = 6 -> sem_complete child = Some true;
  context_scalar_bodies :
    forall child, In child children -> sem_payload child = Scalar ->
      exists rule,
        generated_scalar_rule program (sem_tag child) = Some rule /\
        scalar_body_valid rule (sem_body child) = true
}.

Arguments context_fits {program children} _.
Arguments context_acceptable {program children} _ _ _.
Arguments context_var_complete {program children} _ _ _ _.
Arguments context_scalar_bodies {program children} _ _ _ _.

Lemma context_child_fits :
  forall program children child,
    node_context program children ->
    In child children ->
    child_fits program child.
Proof.
  intros program children child context member.
  exact (proj1 (Forall_forall _ _) (context_fits context) child member).
Qed.

(** The tag-level exactness a family needs from its message children. *)
Definition tagged_children_exact (tag type_id : nat)
    (children : list semantic_summary) : Prop :=
  forall child,
    In child children ->
    sem_tag child = tag ->
    exists score, exact_typed_message_score type_id child = Some score.

Lemma tagged_par_children_exact :
  forall program tag children,
    node_context program children ->
    tag_is_message program tag 1 = true ->
    tagged_children_exact tag 1 children.
Proof.
  intros program tag children context shape child member same_tag.
  destruct (child_fits_message program child tag 1
    (context_child_fits program children child context member) same_tag shape)
    as [not_refused [wire_ok [payload_ok type_ok]]].
  destruct (acceptable_par_has_exact_evidence child
    (context_acceptable context child member) not_refused payload_ok type_ok)
    as [score score_ok].
  exists score.
  rewrite (exact_typed_message_score_of_fit 1 child type_ok wire_ok payload_ok not_refused).
  rewrite score_ok.
  reflexivity.
Qed.

Lemma tagged_var_children_exact :
  forall program tag children,
    node_context program children ->
    tag_is_message program tag 6 = true ->
    tagged_children_exact tag 6 children.
Proof.
  intros program tag children context shape child member same_tag.
  destruct (child_fits_message program child tag 6
    (context_child_fits program children child context member) same_tag shape)
    as [not_refused [wire_ok [payload_ok type_ok]]].
  destruct (acceptable_var_has_exact_evidence child
    (context_acceptable context child member) not_refused payload_ok type_ok
    (context_var_complete context child member type_ok))
    as [score score_ok].
  exists score.
  rewrite (exact_typed_message_score_of_fit 6 child type_ok wire_ok payload_ok not_refused).
  rewrite score_ok.
  reflexivity.
Qed.

(** ** The reader helpers are total on a typed-valid node *)

Lemma exact_typed_child_score_total :
  forall tag type_id children,
    (exists child, In child children /\ sem_tag child = tag) ->
    tagged_children_exact tag type_id children ->
    exists score, exact_typed_child_score tag type_id children = Some score.
Proof.
  intros tag type_id children [present [member same]] exact_children.
  unfold exact_typed_child_score.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [found|] eqn:search.
  - apply find_some in search as [found_member found_same].
    apply Nat.eqb_eq in found_same.
    exact (exact_children found found_member found_same).
  - exfalso.
    pose proof (find_none _ _ search present member) as not_tagged.
    cbv beta in not_tagged.
    rewrite same, Nat.eqb_refl in not_tagged.
    discriminate not_tagged.
Qed.

Lemma exact_typed_optional_score_or_total :
  forall tag type_id fallback children,
    tagged_children_exact tag type_id children ->
    exists score,
      exact_typed_optional_score_or tag type_id fallback children = Some score.
Proof.
  intros tag type_id fallback children exact_children.
  unfold exact_typed_optional_score_or, exact_typed_optional_child_score.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [found|] eqn:search.
  - apply find_some in search as [member same].
    apply Nat.eqb_eq in same.
    destruct (exact_children found member same) as [score score_ok].
    rewrite score_ok.
    exists score.
    reflexivity.
  - exists fallback.
    reflexivity.
Qed.

Lemma exact_typed_selected_scores_total :
  forall tag type_id children,
    tagged_children_exact tag type_id children ->
    exists scores, exact_typed_selected_scores tag type_id children = Some scores.
Proof.
  intros tag type_id children.
  induction children as [|child rest rest_total]; intros exact_children.
  - exists [].
    reflexivity.
  - cbn [exact_typed_selected_scores].
    destruct rest_total as [scores scores_ok].
    { intros later member same.
      exact (exact_children later (or_intror member) same). }
    rewrite scores_ok.
    destruct (sem_tag child =? tag) eqn:same.
    + apply Nat.eqb_eq in same.
      destruct (exact_children child (or_introl eq_refl) same)
        as [score score_ok].
      rewrite score_ok.
      exists (score :: scores).
      reflexivity.
    + exists scores.
      reflexivity.
Qed.

Lemma scalar_true_or_absent_total :
  forall program tag children,
    node_context program children ->
    forbidden_bool_flag program tag = true ->
    exists flag, scalar_true_or_absent tag children = Some flag.
Proof.
  intros program tag children context shape.
  unfold scalar_true_or_absent.
  destruct (find (fun child => sem_tag child =? tag) children)
    as [child|] eqn:search.
  - apply find_some in search as [member same_tag].
    apply Nat.eqb_eq in same_tag.
    unfold forbidden_bool_flag in shape.
    destruct (resolve_tag (program_fields program) tag)
      as [descriptor|] eqn:resolved; [|discriminate shape].
    destruct (generated_scalar_rule program tag)
      as [rule|] eqn:rule_found; [|discriminate shape].
    apply andb_prop in shape as [payload_shape kind_shape].
    apply payload_eqb_true_implies_equal in payload_shape.
    destruct (scalar_kind rule) eqn:kind; try discriminate kind_shape.
    destruct (context_child_fits program children child context member)
      as [fit_descriptor [fit_resolved [_ [_ [payload_ok _]]]]].
    rewrite same_tag, resolved in fit_resolved.
    injection fit_resolved as same_descriptor.
    subst fit_descriptor.
    assert (payload_value : sem_payload child = Scalar) by congruence.
    destruct (context_scalar_bodies context child member payload_value)
      as [body_rule [body_rule_found body_ok]].
    rewrite same_tag, rule_found in body_rule_found.
    injection body_rule_found as same_rule.
    subst body_rule.
    pose proof (bool_forbidden_rule_body_is_one rule (sem_body child)
      kind kind_shape body_ok) as body_value.
    rewrite payload_value, body_value.
    exists true.
    reflexivity.
  - exists false.
    reflexivity.
Qed.

Lemma exact_method_name_total :
  forall program children,
    node_context program children ->
    tag_is_ld_scalar program 1 = true ->
    exists name, exact_method_name_or_empty children = Some name.
Proof.
  intros program children context shape.
  unfold exact_method_name_or_empty.
  destruct (find (fun child => sem_tag child =? 1) children)
    as [child|] eqn:search.
  - apply find_some in search as [member same_tag].
    apply Nat.eqb_eq in same_tag.
    destruct (context_child_fits program children child context member)
      as [descriptor [resolved [not_refused [wire_ok [payload_ok _]]]]].
    rewrite same_tag in resolved.
    unfold tag_is_ld_scalar in shape.
    rewrite resolved in shape.
    apply andb_prop in shape as [wire_shape payload_shape].
    apply wire_eqb_true_implies_equal in wire_shape.
    apply payload_eqb_true_implies_equal in payload_shape.
    rewrite wire_ok, wire_shape, payload_ok, payload_shape, not_refused.
    exists (sem_body child).
    reflexivity.
  - exists [].
    reflexivity.
Qed.

(** ** The tree-level node context *)

Lemma typed_valid_node_context :
  forall field children program,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    generated_wire_program field = Some program ->
    scan_valid (fold_left (scan_child program)
      (semantic_reduce_forest children) empty_scan) = true ->
    node_context program (semantic_reduce_forest children).
Proof.
  intros field children program valid accepted resolved scan_ok.
  constructor.
  - exact (fold_scan_children_fit program _ _ scan_ok).
  - intros child member.
    destruct (forest_member_facts _ children child accepted member)
      as [subject [same [holds _]]].
    subst child.
    exact holds.
  - intros child member type_ok.
    destruct (semantic_reduce_forest_member children child member)
      as [subject same].
    subst child.
    exact (reduced_var_is_complete subject type_ok).
  - intros child member payload_ok.
    destruct (forest_member_facts _ children child accepted member)
      as [subject [same [_ field_member]]].
    subst child.
    pose proof (typed_valid_tree_scalar_child_contract field children program
      (tree_field subject) valid resolved field_member) as contract.
    destruct subject as [sub_field grandchildren].
    cbn [tree_field] in contract.
    change (semantic_reduce (Node sub_field grandchildren)) with
      (reduce_semantic_field sub_field (semantic_reduce_forest grandchildren))
      in payload_ok |- *.
    rewrite reduce_semantic_field_payload in payload_ok.
    rewrite reduce_semantic_field_tag, reduce_semantic_field_body.
    unfold scalar_contract_holds in contract.
    rewrite payload_ok in contract.
    destruct (resolve_tag (program_fields program) (raw_tag sub_field))
      as [descriptor|]; [|discriminate contract].
    destruct (generated_scalar_rule program (raw_tag sub_field))
      as [rule|] eqn:rule_found; [|discriminate contract].
    cbv beta iota in contract.
    apply andb_prop in contract as [first_three _].
    apply andb_prop in first_three as [_ body_ok].
    exists rule.
    split; [reflexivity|exact body_ok].
Qed.

(** ** Var, type 6 (P:1202-1226, C:203-226) *)

Lemma var_fields_single_slot :
  forall descriptor,
    In descriptor (program_fields (generated_program_of 6)) ->
    descriptor_slot descriptor = 0 /\ descriptor_repeated descriptor = false.
Proof.
  intros descriptor member.
  vm_compute in member.
  destruct member as [<-|[<-|[<-|[]]]]; vm_compute; split; reflexivity.
Qed.

Lemma var_descriptor_cases :
  forall descriptor,
    In descriptor (program_fields (generated_program_of 6)) ->
    (descriptor_tag descriptor = 1 /\ descriptor_wire descriptor = Varint /\
     descriptor_payload descriptor = Scalar) \/
    (descriptor_tag descriptor = 2 /\ descriptor_wire descriptor = Varint /\
     descriptor_payload descriptor = Scalar) \/
    (descriptor_tag descriptor = 3 /\
     descriptor_wire descriptor = LengthDelimited /\
     descriptor_payload descriptor = Message /\
     descriptor_child_type_id descriptor = Some 7).
Proof.
  intros descriptor member.
  vm_compute in member.
  destruct member as [<-|[<-|[<-|[]]]].
  - left. vm_compute. repeat split; reflexivity.
  - right. left. vm_compute. repeat split; reflexivity.
  - right. right. vm_compute. repeat split; reflexivity.
Qed.

Example var_scalar_rule_bound :
  generated_scalar_rule (generated_program_of 6) 1 =
    Some (ScalarRule 1 RPB3DescriptorProjectionData.GeneratedData.PBSint32 false).
Proof. vm_compute. reflexivity. Qed.

Example var_scalar_rule_free :
  generated_scalar_rule (generated_program_of 6) 2 =
    Some (ScalarRule 2 RPB3DescriptorProjectionData.GeneratedData.PBSint32 false).
Proof. vm_compute. reflexivity. Qed.

Lemma var_family_total :
  forall children,
    scan_valid (fold_left (scan_child (generated_program_of 6)) children
      empty_scan) = true ->
    node_context (generated_program_of 6) children ->
    exists score, exact_var_score children = Some score.
Proof.
  intros children scan_ok context.
  destruct children as [|head_child [|next_child rest]].
  - exists (ScoreInteger 0%N).
    reflexivity.
  - destruct (context_child_fits _ _ head_child context (or_introl eq_refl))
      as [descriptor [resolved [not_refused [wire_ok [payload_ok type_ok]]]]].
    pose proof (resolve_tag_tag _ _ _ resolved) as tag_value.
    destruct (var_descriptor_cases descriptor (resolve_tag_member _ _ _ resolved))
      as [[tag_one [wire_value payload_value]]
         |[[tag_two [wire_value payload_value]]
          |[tag_three [wire_value [payload_value child_type]]]]].
    + assert (same_tag : sem_tag head_child = 1) by congruence.
      assert (payload_is_scalar : sem_payload head_child = Scalar) by congruence.
      destruct (context_scalar_bodies context head_child (or_introl eq_refl)
        payload_is_scalar) as [rule [rule_found body_ok]].
      rewrite same_tag, var_scalar_rule_bound in rule_found.
      injection rule_found as same_rule.
      subst rule.
      destruct (sint32_rule_yields_i32 1 false (sem_body head_child) body_ok)
        as [level level_ok].
      unfold exact_var_score.
      cbv beta iota.
      rewrite wire_ok, wire_value, payload_ok, payload_value, not_refused.
      cbv beta iota.
      unfold exact_var_level_score.
      rewrite same_tag.
      cbn [Nat.eqb].
      rewrite level_ok.
      exists (score_constructor 50 [ScoreInteger (signed_score_word level)]).
      reflexivity.
    + assert (same_tag : sem_tag head_child = 2) by congruence.
      assert (payload_is_scalar : sem_payload head_child = Scalar) by congruence.
      destruct (context_scalar_bodies context head_child (or_introl eq_refl)
        payload_is_scalar) as [rule [rule_found body_ok]].
      rewrite same_tag, var_scalar_rule_free in rule_found.
      injection rule_found as same_rule.
      subst rule.
      destruct (sint32_rule_yields_i32 2 false (sem_body head_child) body_ok)
        as [level level_ok].
      unfold exact_var_score.
      cbv beta iota.
      rewrite wire_ok, wire_value, payload_ok, payload_value, not_refused.
      cbv beta iota.
      unfold exact_var_level_score.
      rewrite same_tag.
      cbn [Nat.eqb].
      rewrite level_ok.
      exists (score_constructor 51 [ScoreInteger (signed_score_word level)]).
      reflexivity.
    + assert (same_tag : sem_tag head_child = 3) by congruence.
      assert (type_value : sem_type_id head_child = 7).
      { unfold descriptor_accepts_child_type in type_ok.
        rewrite payload_value, child_type in type_ok.
        cbv beta iota in type_ok.
        apply Nat.eqb_eq in type_ok.
        exact type_ok. }
      unfold exact_var_score.
      cbv beta iota.
      rewrite wire_ok, wire_value, payload_ok, payload_value, not_refused.
      cbv beta iota.
      rewrite same_tag, type_value.
      exists (score_constructor 52 []).
      reflexivity.
  - exfalso.
    rewrite (two_children_of_single_slot_are_refused (generated_program_of 6)
      head_child next_child rest var_fields_single_slot) in scan_ok.
    discriminate scan_ok.
Qed.

(** ** The ordered collections EList 27, ETuple 28 and ESet 29 (P:3085-3143) *)

Definition collection_shape (program : generated_program)
    (has_remainder : bool) : bool :=
  tag_is_message program 1 1 &&
  (negb has_remainder || tag_is_message program 5 6) &&
  forbidden_bool_flag program 4.

Lemma ordered_collection_family_total :
  forall program children constructor has_remainder,
    node_context program children ->
    collection_shape program has_remainder = true ->
    exists score,
      exact_ordered_collection_score constructor has_remainder children =
        Some score.
Proof.
  intros program children constructor has_remainder context shape.
  unfold collection_shape in shape.
  apply andb_prop in shape as [first_two flag].
  apply andb_prop in first_two as [elements remainder_shape].
  assert (remainder_ok :
    exists scores, exact_collection_remainder has_remainder children = Some scores).
  { unfold exact_collection_remainder.
    destruct has_remainder.
    - cbn [negb orb] in remainder_shape.
      destruct (exact_typed_optional_score_or_total 5 6
        exact_absent_remainder_score children
        (tagged_var_children_exact program 5 children context remainder_shape))
        as [score score_ok].
      rewrite score_ok.
      exists [score].
      reflexivity.
    - exists [].
      reflexivity. }
  destruct remainder_ok as [remainder_scores remainder_value].
  destruct (exact_typed_selected_scores_total 1 1 children
    (tagged_par_children_exact program 1 children context elements))
    as [element_scores element_value].
  destruct (scalar_true_or_absent_total program 4 children context flag)
    as [connective connective_value].
  unfold exact_ordered_collection_score.
  rewrite remainder_value, element_value, connective_value.
  eexists.
  reflexivity.
Qed.

(** ** EMethod, type 33 (P:3303-3341, C:815-828) *)

Definition emethod_shape (program : generated_program) : bool :=
  tag_is_ld_scalar program 1 &&
  tag_is_message program 2 1 &&
  tag_is_message program 3 1 &&
  forbidden_bool_flag program 6 &&
  slot_owned_by program 1 2.

Lemma emethod_family_total :
  forall program children,
    node_context program children ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    emethod_shape program = true ->
    Some (seen_slot (fold_left (scan_child program) children empty_scan) 1) =
      Some true ->
    exists score, exact_emethod_score children = Some score.
Proof.
  intros program children context scan_ok shape complete.
  injection complete as target_seen.
  unfold emethod_shape in shape.
  apply andb_prop in shape as [first_four owner].
  apply andb_prop in first_four as [first_three flag].
  apply andb_prop in first_three as [first_two arguments].
  apply andb_prop in first_two as [name target].
  destruct (exact_method_name_total program children context name)
    as [method method_value].
  destruct (exact_typed_child_score_total 2 1 children
    (seen_slot_gives_tagged_child program 1 2 children owner scan_ok target_seen)
    (tagged_par_children_exact program 2 children context target))
    as [target_score target_value].
  destruct (exact_typed_selected_scores_total 3 1 children
    (tagged_par_children_exact program 3 children context arguments))
    as [argument_scores arguments_value].
  destruct (scalar_true_or_absent_total program 6 children context flag)
    as [connective connective_value].
  unfold exact_emethod_score.
  rewrite method_value, target_value, arguments_value, connective_value.
  eexists.
  reflexivity.
Qed.

(** ** EVar 35, ENot 36, ENeg 37 and the binary operators 38..54
    (P:1442-1532) *)

Definition evar_shape (program : generated_program) : bool :=
  tag_is_message program 1 6 && slot_owned_by program 0 1.

Lemma evar_family_total :
  forall program children,
    node_context program children ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    evar_shape program = true ->
    Some (seen_slot (fold_left (scan_child program) children empty_scan) 0) =
      Some true ->
    exists score, exact_evar_score children = Some score.
Proof.
  intros program children context scan_ok shape complete.
  injection complete as operand_seen.
  unfold evar_shape in shape.
  apply andb_prop in shape as [operand owner].
  destruct (exact_typed_child_score_total 1 6 children
    (seen_slot_gives_tagged_child program 0 1 children owner scan_ok operand_seen)
    (tagged_var_children_exact program 1 children context operand))
    as [var_score var_value].
  unfold exact_evar_score.
  rewrite var_value.
  eexists.
  reflexivity.
Qed.

Definition unary_shape (program : generated_program) : bool :=
  tag_is_message program 1 1 && slot_owned_by program 0 1.

Lemma unary_family_total :
  forall program children constructor,
    node_context program children ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    unary_shape program = true ->
    Some (seen_slot (fold_left (scan_child program) children empty_scan) 0) =
      Some true ->
    exists score, exact_unary_payload_score constructor children = Some score.
Proof.
  intros program children constructor context scan_ok shape complete.
  injection complete as operand_seen.
  unfold unary_shape in shape.
  apply andb_prop in shape as [operand owner].
  destruct (exact_typed_child_score_total 1 1 children
    (seen_slot_gives_tagged_child program 0 1 children owner scan_ok operand_seen)
    (tagged_par_children_exact program 1 children context operand))
    as [operand_score operand_value].
  unfold exact_unary_payload_score.
  rewrite operand_value.
  eexists.
  reflexivity.
Qed.

Definition binary_shape (program : generated_program) : bool :=
  tag_is_message program 1 1 &&
  tag_is_message program 2 1 &&
  slot_owned_by program 0 1 &&
  slot_owned_by program 1 2.

Lemma binary_family_total :
  forall program children constructor,
    node_context program children ->
    scan_valid (fold_left (scan_child program) children empty_scan) = true ->
    binary_shape program = true ->
    Some (seen_slot (fold_left (scan_child program) children empty_scan) 0 &&
      seen_slot (fold_left (scan_child program) children empty_scan) 1) =
      Some true ->
    exists score, exact_binary_payload_score constructor children = Some score.
Proof.
  intros program children constructor context scan_ok shape complete.
  injection complete as both_seen.
  apply andb_prop in both_seen as [left_seen right_seen].
  unfold binary_shape in shape.
  apply andb_prop in shape as [first_three right_owner].
  apply andb_prop in first_three as [first_two left_owner].
  apply andb_prop in first_two as [left_operand right_operand].
  destruct (exact_typed_child_score_total 1 1 children
    (seen_slot_gives_tagged_child program 0 1 children left_owner scan_ok left_seen)
    (tagged_par_children_exact program 1 children context left_operand))
    as [left_score left_value].
  destruct (exact_typed_child_score_total 2 1 children
    (seen_slot_gives_tagged_child program 1 2 children right_owner scan_ok right_seen)
    (tagged_par_children_exact program 2 children context right_operand))
    as [right_score right_value].
  unfold exact_binary_payload_score.
  rewrite left_value, right_value.
  eexists.
  reflexivity.
Qed.

(** ** EMap, type 30: the KeyValuePair key premise (P:1534-1541, P:3145-3216)

    The score of a complete EMap reads each pair's key score through the
    KeyValuePair carrier.  Rust pushes [key.score] of every pair into the map
    score (P:3145-3216, the pair summary being [SemanticSummary::KeyValuePair]
    of P:1534-1541, which carries the key Par's summary).  The key Par is a
    grandchild of the EMap and the pair's own acceptance is unconditional
    (parent carrier), so the premise on the children says nothing about it.
    [kvp_keys_acceptable] is the additional fact the EMap proof needs: every
    key of every pair child is acceptable. *)

Definition kvp_keys_acceptable (children : @forest raw_field) : Prop :=
  Forall_forest (fun pair =>
    sem_type_id (semantic_reduce pair) = 34 ->
    Forall_forest (fun key =>
      sem_tag (semantic_reduce key) = 1 ->
      reducer_result_acceptable (semantic_reduce key) = true)
      (tree_children pair))
    children.

Lemma Forall_forest_impl :
  forall (P Q : @tree raw_field -> Prop) subjects,
    (forall subject, P subject -> Q subject) ->
    Forall_forest P subjects ->
    Forall_forest Q subjects.
Proof.
  intros P Q subjects implication.
  induction subjects as [|head rest rest_impl]; intros all.
  - constructor.
  - destruct (Forall_forest_inv P head rest all) as [here later].
    constructor.
    + exact (implication head here).
    + exact (rest_impl later).
Qed.

(** The form in which a tree induction supplies the premise: it can carry the
    acceptance of each child's children in its motive. *)
Lemma kvp_keys_acceptable_of_grandchildren_acceptable :
  forall children,
    Forall_forest (fun child =>
      Forall_forest (fun grandchild =>
        reducer_result_acceptable (semantic_reduce grandchild) = true)
        (tree_children child))
      children ->
    kvp_keys_acceptable children.
Proof.
  intros children all.
  unfold kvp_keys_acceptable.
  refine (Forall_forest_impl _ _ children _ all).
  intros child grandchildren_ok _.
  refine (Forall_forest_impl _ _ (tree_children child) _ grandchildren_ok).
  intros key accepted _.
  exact accepted.
Qed.

Example kvp_key_is_par :
  tag_is_message (generated_program_of 34) 1 1 = true.
Proof. vm_compute. reflexivity. Qed.

Example kvp_key_slot_owner :
  slot_owned_by (generated_program_of 34) 0 1 = true.
Proof. vm_compute. reflexivity. Qed.

Lemma kvp_completeness_value :
  forall state children,
    component_completeness 34 state children =
      Some (seen_slot state 0 && seen_slot state 1).
Proof. intros state children. reflexivity. Qed.

(** An unrefused complete pair whose key is acceptable exposes its key score
    through [kvp_carrier_parts]. *)
Lemma kvp_child_parts_exist :
  forall field grandchildren,
    sem_type_id (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) = 34 ->
    sem_definitely_refused (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) = false ->
    sem_wire (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) = LengthDelimited ->
    sem_payload (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) = Message ->
    sem_complete (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) = Some true ->
    Forall_forest (fun key =>
      sem_tag (semantic_reduce key) = 1 ->
      reducer_result_acceptable (semantic_reduce key) = true)
      grandchildren ->
    exists parts,
      kvp_carrier_parts (reduce_semantic_field field
        (semantic_reduce_forest grandchildren)) = Some parts.
Proof.
  intros field grandchildren type_ok not_refused wire_ok payload_ok complete keys.
  destruct (unrefused_gives_scan_valid _ _ not_refused)
    as [program [resolved scan_ok]].
  pose proof (reduce_semantic_field_type_id field
    (semantic_reduce_forest grandchildren) program resolved) as typed.
  assert (family : program_family program = 34) by congruence.
  pose proof (generated_program_is_lookup_of_its_family field program 34
    resolved family ltac:(discriminate)) as same_program.
  subst program.
  pose proof (reduce_semantic_field_complete field
    (semantic_reduce_forest grandchildren) (generated_program_of 34) resolved)
    as completed.
  rewrite complete, family in completed.
  symmetry in completed.
  pose proof completed as completed_both.
  rewrite kvp_completeness_value in completed_both.
  injection completed_both as both_seen.
  apply andb_prop in both_seen as [key_seen _].
  destruct (seen_slot_gives_tagged_child (generated_program_of 34) 0 1 _
    kvp_key_slot_owner scan_ok key_seen) as [witness [witness_member witness_tag]].
  assert (find_key : exists key,
    find (fun child => sem_tag child =? 1)
      (semantic_reduce_forest grandchildren) = Some key).
  { destruct (find (fun child => sem_tag child =? 1)
      (semantic_reduce_forest grandchildren)) as [key|] eqn:search.
    - exists key.
      reflexivity.
    - exfalso.
      pose proof (find_none _ _ search witness witness_member) as not_tagged.
      cbv beta in not_tagged.
      rewrite witness_tag, Nat.eqb_refl in not_tagged.
      discriminate not_tagged. }
  destruct find_key as [key search].
  pose proof search as search_copy.
  apply find_some in search_copy as [key_member key_same].
  apply Nat.eqb_eq in key_same.
  pose proof (proj1 (Forall_forall _ _)
    (fold_scan_children_fit _ _ _ scan_ok) key key_member) as key_fits.
  destruct (child_fits_message (generated_program_of 34) key 1 1
    key_fits key_same kvp_key_is_par)
    as [key_not_refused [key_wire [key_payload key_type]]].
  destruct (forest_member_facts _ grandchildren key keys key_member)
    as [key_subject [same_key [key_acceptable _]]].
  subst key.
  destruct (acceptable_par_has_exact_evidence _ (key_acceptable key_same)
    key_not_refused key_payload key_type) as [score score_value].
  assert (carrier_value :
    exact_kvp_carrier (semantic_reduce_forest grandchildren) =
      Some (score_constructor 0
        [score; ScoreBytes (par_identity_bytes
          (sem_body (semantic_reduce key_subject)))])).
  { unfold exact_kvp_carrier.
    rewrite search.
    rewrite (exact_typed_message_score_of_fit 1 (semantic_reduce key_subject)
      key_type key_wire key_payload key_not_refused).
    rewrite score_value.
    reflexivity. }
  assert (evidence_value :
    sem_score_evidence (reduce_semantic_field field
      (semantic_reduce_forest grandchildren)) =
      ExactScore (score_constructor 0
        [score; ScoreBytes (par_identity_bytes
          (sem_body (semantic_reduce key_subject)))])).
  { rewrite (reduce_semantic_field_evidence field _ (generated_program_of 34)
      resolved).
    rewrite family, completed.
    change (score_evidence_for 34 (Some true)
      (semantic_reduce_forest grandchildren)) with
      (exact_or_unknown (exact_kvp_carrier
        (semantic_reduce_forest grandchildren))).
    rewrite carrier_value.
    reflexivity. }
  unfold kvp_carrier_parts.
  rewrite (exact_typed_message_score_of_fit 34 _ type_ok wire_ok payload_ok
    not_refused).
  rewrite evidence_value.
  exists (score, par_identity_bytes (sem_body (semantic_reduce key_subject))).
  reflexivity.
Qed.

Lemma exact_emap_key_scores_total :
  forall children,
    (forall child, In child children -> sem_tag child = 1 ->
      exists parts, kvp_carrier_parts child = Some parts) ->
    exists keys, exact_emap_key_scores children = Some keys.
Proof.
  induction children as [|child rest rest_total]; intros parts.
  - exists [].
    reflexivity.
  - cbn [exact_emap_key_scores].
    destruct rest_total as [keys keys_ok].
    { intros later member same.
      exact (parts later (or_intror member) same). }
    rewrite keys_ok.
    destruct (sem_tag child =? 1) eqn:same.
    + apply Nat.eqb_eq in same.
      destruct (parts child (or_introl eq_refl) same) as [found found_ok].
      rewrite found_ok.
      exists (fst found :: keys).
      reflexivity.
    + exists keys.
      reflexivity.
Qed.

Lemma emap_family_total :
  forall program subjects,
    node_context program (semantic_reduce_forest subjects) ->
    tag_is_message program 1 34 = true ->
    tag_is_message program 5 6 = true ->
    forbidden_bool_flag program 4 = true ->
    kvp_keys_acceptable subjects ->
    Some (forallb (fun child =>
      if sem_tag child =? 1
      then match sem_complete child with Some true => true | _ => false end
      else true) (semantic_reduce_forest subjects)) = Some true ->
    exists score,
      exact_emap_score (semantic_reduce_forest subjects) = Some score.
Proof.
  intros program subjects context pairs remainder flag keys complete.
  injection complete as all_pairs_complete.
  assert (parts : forall child,
    In child (semantic_reduce_forest subjects) ->
    sem_tag child = 1 ->
    exists parts, kvp_carrier_parts child = Some parts).
  { intros child member same_tag.
    destruct (child_fits_message program child 1 34
      (context_child_fits program _ child context member) same_tag pairs)
      as [not_refused [wire_ok [payload_ok type_ok]]].
    pose proof (proj1 (forallb_forall _ _) all_pairs_complete child member)
      as child_complete.
    cbv beta in child_complete.
    rewrite same_tag in child_complete.
    cbn [Nat.eqb] in child_complete.
    destruct (sem_complete child) as [[|]|] eqn:completed;
      try discriminate child_complete.
    destruct (forest_member_facts _ subjects child keys member)
      as [pair [same_child [pair_keys _]]].
    subst child.
    destruct pair as [pair_field grandchildren].
    cbn [tree_children] in pair_keys.
    pose proof (pair_keys type_ok) as keys_of_pair.
    change (semantic_reduce (Node pair_field grandchildren)) with
      (reduce_semantic_field pair_field (semantic_reduce_forest grandchildren))
      in type_ok, not_refused, wire_ok, payload_ok, completed |- *.
    exact (kvp_child_parts_exist pair_field grandchildren type_ok not_refused
      wire_ok payload_ok completed keys_of_pair). }
  destruct (exact_emap_key_scores_total _ parts) as [key_scores keys_value].
  destruct (exact_typed_optional_score_or_total 5 6 exact_absent_remainder_score
    (semantic_reduce_forest subjects)
    (tagged_var_children_exact program 5 _ context remainder))
    as [remainder_score remainder_value].
  destruct (scalar_true_or_absent_total program 4 _ context flag)
    as [connective connective_value].
  unfold exact_emap_score.
  rewrite remainder_value, keys_value, connective_value.
  eexists.
  reflexivity.
Qed.

(** ** The dispatch over the 26 ExprPayload program types

    [expr_payload_type_ids] is computed to a disjunction and each type id is
    settled by its family lemma.  The reductions of [score_evidence_for] and
    [component_completeness] at each concrete id hold by conversion. *)

Ltac split_membership member :=
  match type of member with
  | False => destruct member
  | _ \/ _ => destruct member as [<-|member]; [|split_membership member]
  end.

Lemma expr_payload_completeness_is_known :
  forall type_id state children,
    In type_id expr_payload_type_ids ->
    exists complete, component_completeness type_id state children = Some complete.
Proof.
  intros type_id state children member.
  vm_compute in member.
  split_membership member.
  all: eexists; reflexivity.
Qed.

Lemma expr_payload_incomplete_evidence :
  forall type_id children,
    In type_id expr_payload_type_ids ->
    score_evidence_for type_id (Some false) children = ScoreNotDerived.
Proof.
  intros type_id children member.
  vm_compute in member.
  split_membership member.
  all: reflexivity.
Qed.

Lemma expr_payload_complete_evidence :
  forall type_id children,
    In type_id expr_payload_type_ids ->
    score_evidence_for type_id (Some true) children =
      exact_or_unknown (exact_expr_payload_score type_id children).
Proof.
  intros type_id children member.
  vm_compute in member.
  split_membership member.
  all: reflexivity.
Qed.

Lemma expr_payload_complete_is_scored :
  forall type_id subjects,
    In type_id expr_payload_type_ids ->
    (type_id = 30 -> kvp_keys_acceptable subjects) ->
    scan_valid (fold_left (scan_child (generated_program_of type_id))
      (semantic_reduce_forest subjects) empty_scan) = true ->
    node_context (generated_program_of type_id)
      (semantic_reduce_forest subjects) ->
    component_completeness type_id
      (fold_left (scan_child (generated_program_of type_id))
        (semantic_reduce_forest subjects) empty_scan)
      (semantic_reduce_forest subjects) = Some true ->
    exists score,
      exact_expr_payload_score type_id (semantic_reduce_forest subjects) =
        Some score.
Proof.
  intros type_id subjects member keys scan_ok context complete.
  vm_compute in member.
  split_membership member.
  (* Var, type 6 *)
  - exact (var_family_total _ scan_ok context).
  (* EList, type 27 *)
  - exact (ordered_collection_family_total _ _ 6 true context
      ltac:(vm_compute; reflexivity)).
  (* ETuple, type 28 *)
  - exact (ordered_collection_family_total _ _ 7 false context
      ltac:(vm_compute; reflexivity)).
  (* ESet, type 29 *)
  - exact (ordered_collection_family_total _ _ 8 true context
      ltac:(vm_compute; reflexivity)).
  (* EMap, type 30 *)
  - exact (emap_family_total _ _ context
      ltac:(vm_compute; reflexivity)
      ltac:(vm_compute; reflexivity)
      ltac:(vm_compute; reflexivity)
      (keys eq_refl) complete).
  (* EMethod, type 33 *)
  - exact (emethod_family_total _ _ context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  (* EVar, type 35 *)
  - exact (evar_family_total _ _ context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  (* ENot 36 and ENeg 37 *)
  - exact (unary_family_total _ _ 112 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (unary_family_total _ _ 101 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  (* The binary operators 38..54, with their constructors. *)
  - exact (binary_family_total _ _ 102 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 103 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 122 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 104 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 105 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 106 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 107 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 108 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 109 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 110 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 111 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 113 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 114 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 118 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 119 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 120 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
  - exact (binary_family_total _ _ 121 context scan_ok
      ltac:(vm_compute; reflexivity) complete).
Qed.

(** ** The class result of a typed-valid unrefused node *)

Lemma expr_payload_node_class_ok :
  forall field children program type_id,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    generated_wire_program field = Some program ->
    program_family program = type_id ->
    In type_id expr_payload_type_ids ->
    (type_id = 30 -> kvp_keys_acceptable children) ->
    scan_valid (fold_left (scan_child program)
      (semantic_reduce_forest children) empty_scan) = true ->
    class_result_ok
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program type_id valid accepted resolved family member
    keys scan_ok.
  pose proof (generated_program_is_lookup_of_its_family field program type_id
    resolved family (expr_payload_id_nonzero type_id member)) as same_program.
  subst program.
  pose proof (typed_valid_node_context field children (generated_program_of type_id)
    valid accepted resolved scan_ok) as context.
  unfold class_result_ok.
  rewrite (reduce_semantic_field_type_id field (semantic_reduce_forest children)
    (generated_program_of type_id) resolved).
  rewrite family.
  rewrite (score_class_of_expr_payload_member type_id member).
  rewrite (reduce_semantic_field_complete field (semantic_reduce_forest children)
    (generated_program_of type_id) resolved).
  rewrite (reduce_semantic_field_evidence field (semantic_reduce_forest children)
    (generated_program_of type_id) resolved).
  rewrite family.
  destruct (expr_payload_completeness_is_known type_id
    (fold_left (scan_child (generated_program_of type_id))
      (semantic_reduce_forest children) empty_scan)
    (semantic_reduce_forest children) member) as [[|] same_complete].
  - rewrite same_complete.
    destruct (expr_payload_complete_is_scored type_id children member keys
      scan_ok context same_complete) as [score score_value].
    rewrite (expr_payload_complete_evidence type_id _ member), score_value.
    reflexivity.
  - rewrite same_complete.
    rewrite (expr_payload_incomplete_evidence type_id _ member).
    reflexivity.
Qed.

Lemma payload_or_carrier_node_acceptable :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    (sem_type_id (semantic_reduce (Node field children)) = 30 ->
      kvp_keys_acceptable children) ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassExprPayload; ClassParentCarrier] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted keys class_member.
  change (semantic_reduce (Node field children)) with
    (reduce_semantic_field field (semantic_reduce_forest children))
    in keys, class_member |- *.
  destruct (sem_definitely_refused
    (reduce_semantic_field field (semantic_reduce_forest children))) eqn:refused.
  { unfold reducer_result_acceptable.
    rewrite refused.
    reflexivity. }
  destruct (unrefused_gives_scan_valid _ _ refused) as [program [resolved scan_ok]].
  pose proof (reduce_semantic_field_type_id field
    (semantic_reduce_forest children) program resolved) as typed.
  rewrite typed in keys, class_member.
  cbn [In] in class_member.
  destruct class_member as [payload_class|[carrier_class|[]]].
  - apply acceptable_of_class_ok.
    assert (member : In (program_family program) expr_payload_type_ids).
    { apply score_class_of_expr_payload.
      symmetry.
      exact payload_class. }
    exact (expr_payload_node_class_ok field children program
      (program_family program) valid accepted resolved eq_refl member keys scan_ok).
  - apply acceptable_of_class_ok.
    unfold class_result_ok.
    rewrite typed, <- carrier_class.
    reflexivity.
Qed.

(** ** The exported statements

    The first statement is the theorem of this leaf for every family except
    EMap.  The second adds the pair-key premise and holds for all 26 ExprPayload
    types and the carriers. *)

Theorem expr_payload_result_on_typed_valid_except_emap :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassExprPayload; ClassParentCarrier] ->
    sem_type_id (semantic_reduce (Node field children)) <> 30 ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted class_member not_emap.
  apply payload_or_carrier_node_acceptable; try assumption.
  intro is_emap.
  contradiction.
Qed.

Theorem expr_payload_result_on_typed_valid_with_carrier_keys :
  forall field children,
    typed_valid_tree (Node field children) ->
    Forall_forest
      (fun child => reducer_result_acceptable (semantic_reduce child) = true)
      children ->
    kvp_keys_acceptable children ->
    In (score_class_of (sem_type_id (semantic_reduce (Node field children))))
      [ClassExprPayload; ClassParentCarrier] ->
    reducer_result_acceptable (semantic_reduce (Node field children)) = true.
Proof.
  intros field children valid accepted keys class_member.
  apply payload_or_carrier_node_acceptable; try assumption.
  intro.
  exact keys.
Qed.

(** ** EMap cannot be settled from the statement's own premises

    Two closed facts locate the gap.

    [emap_key_premise_is_necessary]: a complete, unrefused and acceptable EMap
    node reads a [kvp_carrier_parts] result for every pair child, so the pair
    key score is genuinely consumed; a premise of this kind cannot be avoided.

    [emap_local_step_is_false]: the implication from acceptable child
    summaries to an acceptable EMap summary fails.  The witness is a pair
    summary that is complete and accepted as a parent carrier yet carries no
    key score.  The witness is a summary, not a typed-valid tree: a typed-valid
    tree with this shape would need a key Par without exact evidence, which is
    the global totality statement of the tree induction, and that is not
    available to this leaf.  This theory therefore neither proves nor refutes
    the bare statement at EMap; it proves it with the key premise. *)

Lemma exact_emap_key_scores_requires_parts :
  forall children keys,
    exact_emap_key_scores children = Some keys ->
    forall child,
      In child children ->
      sem_tag child = 1 ->
      exists parts, kvp_carrier_parts child = Some parts.
Proof.
  induction children as [|head rest rest_parts];
    intros keys scored child member same_tag.
  - destruct member.
  - cbn [exact_emap_key_scores] in scored.
    destruct (exact_emap_key_scores rest) as [rest_keys|] eqn:rest_value;
      [|discriminate scored].
    destruct member as [at_head|at_rest].
    + subst child.
      rewrite same_tag in scored.
      cbn [Nat.eqb] in scored.
      destruct (kvp_carrier_parts head) as [parts|].
      * exists parts.
        reflexivity.
      * discriminate scored.
    + exact (rest_parts rest_keys eq_refl child at_rest same_tag).
Qed.

Theorem emap_key_premise_is_necessary :
  forall field children,
    sem_type_id (reduce_semantic_field field children) = 30 ->
    sem_definitely_refused (reduce_semantic_field field children) = false ->
    payload_eqb (sem_payload (reduce_semantic_field field children)) Scalar = false ->
    sem_complete (reduce_semantic_field field children) = Some true ->
    reducer_result_acceptable (reduce_semantic_field field children) = true ->
    forall child,
      In child children ->
      sem_tag child = 1 ->
      exists parts, kvp_carrier_parts child = Some parts.
Proof.
  intros field children type_ok not_refused not_scalar complete acceptable.
  unfold reducer_result_acceptable in acceptable.
  rewrite not_refused, not_scalar in acceptable.
  cbn [orb] in acceptable.
  unfold class_result_ok in acceptable.
  rewrite type_ok in acceptable.
  change (score_class_of 30) with ClassExprPayload in acceptable.
  cbv beta iota in acceptable.
  rewrite complete in acceptable.
  cbv beta iota in acceptable.
  destruct (sem_score_evidence (reduce_semantic_field field children))
    as [score|] eqn:evidence; [|discriminate acceptable].
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  2: { exfalso.
       unfold reduce_semantic_field in type_ok.
       rewrite resolved in type_ok.
       discriminate type_ok. }
  pose proof (reduce_semantic_field_type_id field children program resolved)
    as typed.
  assert (family : program_family program = 30) by congruence.
  rewrite (reduce_semantic_field_evidence field children program resolved)
    in evidence.
  rewrite (reduce_semantic_field_complete field children program resolved)
    in complete.
  rewrite family in evidence, complete.
  rewrite complete in evidence.
  change (score_evidence_for 30 (Some true) children) with
    (exact_or_unknown (exact_expr_payload_score 30 children)) in evidence.
  unfold exact_or_unknown in evidence.
  destruct (exact_expr_payload_score 30 children) as [final|] eqn:scored;
    [|discriminate evidence].
  change (exact_expr_payload_score 30 children) with (exact_emap_score children)
    in scored.
  unfold exact_emap_score in scored.
  destruct (exact_typed_optional_score_or 5 6 exact_absent_remainder_score children);
    [|discriminate scored].
  destruct (exact_emap_key_scores children) as [keys|] eqn:keys_value;
    [|discriminate scored].
  exact (exact_emap_key_scores_requires_parts children keys keys_value).
Qed.

Definition emap_local_step : Prop :=
  forall field summaries,
    raw_program_type_id field = Some 30 ->
    raw_payload field = Message ->
    (forall child, In child summaries -> reducer_result_acceptable child = true) ->
    reducer_result_acceptable (reduce_semantic_field field summaries) = true.

Definition emap_countermodel_field : raw_field :=
  RawField 0 LengthDelimited Message [] 0 (Some 30) [] [].

Definition underived_kvp_summary : semantic_summary :=
  SemanticSummary 1 LengthDelimited Message 34 [] false (Some true)
    ScoreNotDerived false false.

Example underived_kvp_summary_is_acceptable :
  reducer_result_acceptable underived_kvp_summary = true.
Proof. vm_compute. reflexivity. Qed.

Example emap_over_underived_kvp_is_not_acceptable :
  sem_definitely_refused
    (reduce_semantic_field emap_countermodel_field [underived_kvp_summary]) = false /\
  sem_complete
    (reduce_semantic_field emap_countermodel_field [underived_kvp_summary]) = Some true /\
  sem_score_evidence
    (reduce_semantic_field emap_countermodel_field [underived_kvp_summary]) =
    ScoreNotDerived /\
  reducer_result_acceptable
    (reduce_semantic_field emap_countermodel_field [underived_kvp_summary]) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem emap_local_step_is_false : ~ emap_local_step.
Proof.
  intros claimed.
  assert (all_accepted : forall child,
    In child [underived_kvp_summary] ->
    reducer_result_acceptable child = true).
  { intros child [same|[]].
    subst child.
    exact underived_kvp_summary_is_acceptable. }
  pose proof (claimed emap_countermodel_field [underived_kvp_summary]
    eq_refl eq_refl all_accepted) as claim.
  destruct emap_over_underived_kvp_is_not_acceptable as [_ [_ [_ rejected]]].
  rewrite rejected in claim.
  discriminate claim.
Qed.

(** A typed-valid EMap with real pair and key subtrees, on which the
    strengthened statement's premises hold: the premise is satisfiable. *)

Definition emap_pair_witness : @tree raw_field :=
  message_node 1 34 [10; 0; 18; 0]
    (FCons (message_node 1 1 [] FNil)
      (FCons (message_node 2 1 [] FNil) FNil)).

Definition emap_witness_children : @forest raw_field :=
  FCons emap_pair_witness FNil.

Definition emap_witness : @tree raw_field :=
  Node (RawField 0 LengthDelimited Message [10; 4; 10; 0; 18; 0] 6 (Some 30) [] [])
    emap_witness_children.

Theorem emap_witness_is_typed_valid : typed_valid_tree emap_witness.
Proof. typed_valid_by_compute. Qed.

Example emap_witness_keys_premise : kvp_keys_acceptable emap_witness_children.
Proof.
  unfold kvp_keys_acceptable, emap_witness_children.
  constructor.
  - intro is_pair.
    unfold emap_pair_witness, message_node.
    cbn [tree_children].
    constructor; [|constructor; [|constructor]];
      intro tag_one; vm_compute; reflexivity.
  - constructor.
Qed.

Example emap_witness_children_acceptable :
  Forall_forest
    (fun child => reducer_result_acceptable (semantic_reduce child) = true)
    emap_witness_children.
Proof.
  unfold emap_witness_children.
  constructor; [vm_compute; reflexivity|constructor].
Qed.

Example emap_witness_class :
  In (score_class_of (sem_type_id (semantic_reduce emap_witness)))
    [ClassExprPayload; ClassParentCarrier].
Proof. vm_compute. left. reflexivity. Qed.

Corollary emap_witness_is_acceptable_by_the_strengthened_statement :
  reducer_result_acceptable (semantic_reduce emap_witness) = true.
Proof.
  exact (expr_payload_result_on_typed_valid_with_carrier_keys
    (RawField 0 LengthDelimited Message [10; 4; 10; 0; 18; 0] 6 (Some 30) [] [])
    emap_witness_children
    emap_witness_is_typed_valid
    emap_witness_children_acceptable
    emap_witness_keys_premise
    emap_witness_class).
Qed.

Example emap_witness_is_complete_and_scored :
  sem_definitely_refused (semantic_reduce emap_witness) = false /\
  sem_complete (semantic_reduce emap_witness) = Some true /\
  exact_evidence (semantic_reduce emap_witness) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** Red control: dropping one binary operator's arm

    [binary_arm_dropped_evidence] removes the arm of type 38 at [Some true],
    as if the [exact_binary_payload_score] case of [exact_expr_payload_score]
    were absent, and [mutant_semantic_reduce] is the reducer with that
    evidence.  On a typed-valid type-38 node with two operands the reducer is
    acceptable and the mutant is not, and a neighbouring operator is
    untouched.  The statement of this leaf is then false for the mutant, so the
    theorem's proof depends on the arm. *)

Definition binary_arm_dropped_evidence (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  match type_id, complete with
  | 38, Some true => ScoreNotDerived
  | _, _ => score_evidence_for type_id complete children
  end.

Definition mutant_reduce_semantic_field (field : raw_field)
    (children : list semantic_summary) : semantic_summary :=
  let original := reduce_semantic_field field children in
  SemanticSummary (sem_tag original) (sem_wire original) (sem_payload original)
    (sem_type_id original) (sem_body original) (sem_definitely_refused original)
    (sem_complete original)
    (binary_arm_dropped_evidence (sem_type_id original) (sem_complete original)
      children)
    (sem_bind_source_present original) (sem_case_required_present original).

Fixpoint mutant_semantic_reduce (subject : @tree raw_field) : semantic_summary :=
  match subject with
  | Node field children =>
      mutant_reduce_semantic_field field (mutant_semantic_reduce_forest children)
  end
with mutant_semantic_reduce_forest (subjects : @forest raw_field)
    : list semantic_summary :=
  match subjects with
  | FNil => []
  | FCons subject rest =>
      mutant_semantic_reduce subject :: mutant_semantic_reduce_forest rest
  end.

Definition binary_witness_field : raw_field :=
  RawField 0 LengthDelimited Message [10; 0; 18; 0] 4 (Some 38) [] [].

Definition binary_witness_children : @forest raw_field :=
  FCons (message_node 1 1 [] FNil) (FCons (message_node 2 1 [] FNil) FNil).

Definition binary_witness : @tree raw_field :=
  Node binary_witness_field binary_witness_children.

Theorem binary_witness_is_typed_valid : typed_valid_tree binary_witness.
Proof. typed_valid_by_compute. Qed.

Example binary_witness_is_acceptable_under_the_reducer :
  reducer_result_acceptable (semantic_reduce binary_witness) = true.
Proof. vm_compute. reflexivity. Qed.

Example binary_witness_is_not_acceptable_under_the_mutant :
  sem_complete (mutant_semantic_reduce binary_witness) = Some true /\
  sem_score_evidence (mutant_semantic_reduce binary_witness) = ScoreNotDerived /\
  reducer_result_acceptable (mutant_semantic_reduce binary_witness) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition neighbouring_binary_witness : @tree raw_field :=
  Node (RawField 0 LengthDelimited Message [10; 0; 18; 0] 4 (Some 39) [] [])
    binary_witness_children.

Theorem neighbouring_binary_witness_is_typed_valid :
  typed_valid_tree neighbouring_binary_witness.
Proof. typed_valid_by_compute. Qed.

Example mutant_leaves_other_binary_arms_alone :
  reducer_result_acceptable (mutant_semantic_reduce neighbouring_binary_witness) = true.
Proof. vm_compute. reflexivity. Qed.

Theorem mutant_falsifies_the_statement :
  ~ (forall field children,
      typed_valid_tree (Node field children) ->
      Forall_forest
        (fun child => reducer_result_acceptable (mutant_semantic_reduce child) = true)
        children ->
      In (score_class_of
          (sem_type_id (mutant_semantic_reduce (Node field children))))
        [ClassExprPayload; ClassParentCarrier] ->
      reducer_result_acceptable (mutant_semantic_reduce (Node field children)) = true).
Proof.
  intros claimed.
  assert (children_accepted :
    Forall_forest
      (fun child => reducer_result_acceptable (mutant_semantic_reduce child) = true)
      binary_witness_children).
  { unfold binary_witness_children.
    constructor; [vm_compute; reflexivity|].
    constructor; [vm_compute; reflexivity|].
    constructor. }
  assert (class_member :
    In (score_class_of
        (sem_type_id (mutant_semantic_reduce
          (Node binary_witness_field binary_witness_children))))
      [ClassExprPayload; ClassParentCarrier]).
  { vm_compute.
    left.
    reflexivity. }
  pose proof (claimed binary_witness_field binary_witness_children
    binary_witness_is_typed_valid children_accepted class_member) as acceptable.
  destruct binary_witness_is_not_acceptable_under_the_mutant as [_ [_ rejected]].
  change (mutant_semantic_reduce (Node binary_witness_field binary_witness_children))
    with (mutant_semantic_reduce binary_witness) in acceptable.
  rewrite rejected in acceptable.
  discriminate acceptable.
Qed.

(** ** Assumption audit: one line per exported lemma *)

Print Assumptions payload_eqb_true_implies_equal.
Print Assumptions resolve_tag_tag.
Print Assumptions scan_child_valid_facts.
Print Assumptions fold_scan_valid_prefix.
Print Assumptions fold_scan_children_fit.
Print Assumptions fold_scan_seen_source.
Print Assumptions two_children_of_single_slot_are_refused.
Print Assumptions seen_slot_gives_tagged_child.
Print Assumptions Forall_forest_inv.
Print Assumptions Forall_forest_and.
Print Assumptions forest_member_facts.
Print Assumptions reduce_semantic_field_tag.
Print Assumptions reduce_semantic_field_wire.
Print Assumptions reduce_semantic_field_body.
Print Assumptions reduce_semantic_field_evidence.
Print Assumptions unrefused_gives_scan_valid.
Print Assumptions score_class_of_expr_payload.
Print Assumptions score_class_of_expr_payload_member.
Print Assumptions expr_payload_id_nonzero.
Print Assumptions acceptable_of_class_ok.
Print Assumptions acceptable_par_has_exact_evidence.
Print Assumptions acceptable_var_has_exact_evidence.
Print Assumptions exact_typed_message_score_of_fit.
Print Assumptions reduced_var_is_complete.
Print Assumptions some_pair_injective.
Print Assumptions varint_groups_unfold.
Print Assumptions varint_groups_later_group_is_large.
Print Assumptions compact_varint_one_is_singleton.
Print Assumptions bool_forbidden_rule_body_is_one.
Print Assumptions sint32_rule_yields_i32.
Print Assumptions child_fits_message.
Print Assumptions context_child_fits.
Print Assumptions tagged_par_children_exact.
Print Assumptions tagged_var_children_exact.
Print Assumptions exact_typed_child_score_total.
Print Assumptions exact_typed_optional_score_or_total.
Print Assumptions exact_typed_selected_scores_total.
Print Assumptions scalar_true_or_absent_total.
Print Assumptions exact_method_name_total.
Print Assumptions typed_valid_node_context.
Print Assumptions var_fields_single_slot.
Print Assumptions var_descriptor_cases.
Print Assumptions var_scalar_rule_bound.
Print Assumptions var_scalar_rule_free.
Print Assumptions var_family_total.
Print Assumptions ordered_collection_family_total.
Print Assumptions emethod_family_total.
Print Assumptions evar_family_total.
Print Assumptions unary_family_total.
Print Assumptions binary_family_total.
Print Assumptions Forall_forest_impl.
Print Assumptions kvp_keys_acceptable_of_grandchildren_acceptable.
Print Assumptions kvp_key_is_par.
Print Assumptions kvp_key_slot_owner.
Print Assumptions kvp_completeness_value.
Print Assumptions kvp_child_parts_exist.
Print Assumptions exact_emap_key_scores_total.
Print Assumptions emap_family_total.
Print Assumptions expr_payload_completeness_is_known.
Print Assumptions expr_payload_incomplete_evidence.
Print Assumptions expr_payload_complete_evidence.
Print Assumptions expr_payload_complete_is_scored.
Print Assumptions expr_payload_node_class_ok.
Print Assumptions payload_or_carrier_node_acceptable.
Print Assumptions expr_payload_result_on_typed_valid_except_emap.
Print Assumptions expr_payload_result_on_typed_valid_with_carrier_keys.
Print Assumptions exact_emap_key_scores_requires_parts.
Print Assumptions emap_key_premise_is_necessary.
Print Assumptions underived_kvp_summary_is_acceptable.
Print Assumptions emap_over_underived_kvp_is_not_acceptable.
Print Assumptions emap_local_step_is_false.
Print Assumptions emap_witness_is_typed_valid.
Print Assumptions emap_witness_keys_premise.
Print Assumptions emap_witness_children_acceptable.
Print Assumptions emap_witness_class.
Print Assumptions emap_witness_is_acceptable_by_the_strengthened_statement.
Print Assumptions emap_witness_is_complete_and_scored.
Print Assumptions binary_witness_is_typed_valid.
Print Assumptions binary_witness_is_acceptable_under_the_reducer.
Print Assumptions binary_witness_is_not_acceptable_under_the_mutant.
Print Assumptions neighbouring_binary_witness_is_typed_valid.
Print Assumptions mutant_leaves_other_binary_arms_alone.
Print Assumptions mutant_falsifies_the_statement.
