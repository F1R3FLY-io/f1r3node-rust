From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import EPM1 ChannelImage.

Import ListNotations.
Set Implicit Arguments.

Module UnaryEListConstruction.

Definition bytes := list nat.

Definition field (tag body : bytes) : bytes :=
  tag ++ EPM1.encode_varint (length body) ++ body.

Definition wrap_expr (child : bytes) : bytes :=
  field [162; 1] (field [10] child).

Definition wrap_par (child : bytes) : bytes := field [42] (wrap_expr child).

Fixpoint resident_reference (depth : nat) (terminal : bytes) : bytes :=
  match depth with
  | 0 => terminal
  | S remaining => resident_reference remaining (wrap_par terminal)
  end.

Record frame := Frame {
  child_length : nat;
  list_length : nat;
  expr_length : nat;
  par_length : nat
}.

Definition make_frame (child : nat) : frame :=
  let list_size := 1 + length (EPM1.encode_varint child) + child in
  let expr_size := 2 + length (EPM1.encode_varint list_size) + list_size in
  let par_size := 1 + length (EPM1.encode_varint expr_size) + expr_size in
  Frame child list_size expr_size par_size.

Definition expr_prefix (current : frame) : bytes :=
  [162; 1] ++ EPM1.encode_varint (list_length current) ++
  [10] ++ EPM1.encode_varint (child_length current).

Definition par_prefix (current : frame) : bytes :=
  [42] ++ EPM1.encode_varint (expr_length current) ++ expr_prefix current.

Fixpoint plan (depth child : nat) : list frame :=
  match depth with
  | 0 => []
  | S remaining =>
      let current := make_frame child in
      current :: plan remaining (par_length current)
  end.

Definition emit_par (frames : list frame) (terminal : bytes) : bytes :=
  concat (map par_prefix (rev frames)) ++ terminal.

Definition emit_expr (frames : list frame) (terminal : bytes) : option bytes :=
  match rev frames with
  | [] => None
  | outer :: inner =>
      Some (expr_prefix outer ++ concat (map par_prefix inner) ++ terminal)
  end.

Lemma field_length : forall tag body,
  length (field tag body) =
  length tag + length (EPM1.encode_varint (length body)) + length body.
Proof. intros. unfold field. repeat rewrite length_app. apply Nat.add_assoc. Qed.

Lemma frame_lengths_exact : forall child,
  list_length (make_frame (length child)) = length (field [10] child) /\
  expr_length (make_frame (length child)) = length (wrap_expr child) /\
  par_length (make_frame (length child)) = length (wrap_par child).
Proof.
  intro child. unfold make_frame, wrap_par, wrap_expr.
  repeat rewrite field_length. cbn [list_length expr_length par_length length].
  repeat split; reflexivity.
Qed.

Lemma expr_prefix_exact : forall child,
  expr_prefix (make_frame (length child)) ++ child = wrap_expr child.
Proof.
  intro child. unfold expr_prefix, make_frame, wrap_expr, field.
  cbn [list_length child_length]. repeat rewrite length_app.
  cbn [length]. repeat rewrite app_assoc. reflexivity.
Qed.

Lemma par_prefix_exact : forall child,
  par_prefix (make_frame (length child)) ++ child = wrap_par child.
Proof.
  intro child. unfold par_prefix, wrap_par, field.
  destruct (frame_lengths_exact child) as [_ [expression _]].
  rewrite expression. rewrite <- app_assoc, <- app_assoc.
  rewrite expr_prefix_exact. reflexivity.
Qed.

Theorem planned_emission_equals_repeated_resident_wrapping : forall depth terminal,
  emit_par (plan depth (length terminal)) terminal = resident_reference depth terminal.
Proof.
  induction depth as [|depth induction]; intro terminal.
  - reflexivity.
  - cbn [plan resident_reference]. unfold emit_par.
    cbn [rev]. rewrite map_app, concat_app. cbn [map concat].
    rewrite app_nil_r. rewrite <- app_assoc, par_prefix_exact.
    destruct (frame_lengths_exact terminal) as [_ [_ parent]].
    rewrite parent. exact (induction (wrap_par terminal)).
Qed.

Theorem planned_frame_count_is_exact : forall depth child,
  length (plan depth child) = depth.
Proof. induction depth; intro child; cbn [plan length]; auto. Qed.

Lemma plan_successor_appends_outer_frame : forall depth terminal,
  plan (S depth) (length terminal) = plan depth (length terminal) ++
    [make_frame (length (resident_reference depth terminal))].
Proof.
  induction depth as [|depth induction]; intro terminal; [reflexivity|].
  change (make_frame (length terminal) ::
    plan (S depth) (par_length (make_frame (length terminal))) =
    (make_frame (length terminal) ::
      plan depth (par_length (make_frame (length terminal)))) ++
    [make_frame (length (resident_reference depth (wrap_par terminal)))]).
  destruct (frame_lengths_exact terminal) as [_ [_ parent]].
  rewrite parent, induction. reflexivity.
Qed.

Theorem planned_expression_is_exact : forall depth terminal,
  emit_expr (plan (S depth) (length terminal)) terminal =
    Some (wrap_expr (resident_reference depth terminal)).
Proof.
  intros depth terminal. unfold emit_expr.
  rewrite plan_successor_appends_outer_frame, rev_app_distr.
  cbn [rev app].
  change (Some (expr_prefix (make_frame (length (resident_reference depth terminal))) ++
    emit_par (plan depth (length terminal)) terminal) =
    Some (wrap_expr (resident_reference depth terminal))).
  rewrite planned_emission_equals_repeated_resident_wrapping, expr_prefix_exact. reflexivity.
Qed.

Theorem zero_depth_preserves_terminal_bytes : forall terminal,
  emit_par (plan 0 (length terminal)) terminal = terminal.
Proof. reflexivity. Qed.

Theorem positive_plan_has_one_outer_expression : forall depth child,
  exists outer inner, rev (plan (S depth) child) = outer :: inner.
Proof.
  intros depth child.
  destruct (rev (plan (S depth) child)) as [|outer inner] eqn:shape.
  - pose proof (f_equal (@length frame) shape) as empty.
    rewrite length_rev, planned_frame_count_is_exact in empty. discriminate.
  - now exists outer, inner.
Qed.

Theorem expression_emission_excludes_only_the_outer_par_header :
  forall frames terminal outer inner,
    rev frames = outer :: inner ->
    exists expression,
      emit_expr frames terminal = Some expression /\
      emit_par frames terminal =
        [42] ++ EPM1.encode_varint (expr_length outer) ++ expression.
Proof.
  intros frames terminal outer inner shape.
  unfold emit_expr, emit_par. rewrite shape. cbn [map concat].
  eexists. split; [reflexivity|]. unfold par_prefix.
  repeat rewrite app_assoc. reflexivity.
Qed.

Definition integer_atom (value : nat) : bytes :=
  [1; 1; 128] ++ ChannelImage.encode_fixed_be 7 value.

Definition minus_one_atom : bytes := [1; 1; 127; 255; 255; 255; 255; 255; 255; 255].
Definition par_head : bytes := [2] ++ integer_atom 999.
Definition elist_head : bytes := [2] ++ integer_atom 6 ++ minus_one_atom.
Definition score_tail (connective : bool) : bytes :=
  integer_atom (if connective then 1 else 0) ++ [0].
Definition close : bytes := score_tail false.

Definition terminal_score (components : bytes) (connective : bool) : bytes :=
  par_head ++ components ++ score_tail connective.

Fixpoint recursive_par_score (depth : nat) (components : bytes) (connective : bool) : bytes :=
  match depth with
  | 0 => terminal_score components connective
  | S remaining =>
      par_head ++ elist_head ++ recursive_par_score remaining components connective ++ close ++ close
  end.

Definition recursive_expr_score (depth : nat) (components : bytes) (connective : bool)
    : option bytes :=
  match depth with
  | 0 => None
  | S remaining => Some (elist_head ++ recursive_par_score remaining components connective ++ close)
  end.

Definition direct_expr_score (depth : nat) (components : bytes) (connective : bool)
    : option bytes :=
  match depth with
  | 0 => None
  | S remaining =>
      Some (elist_head ++ concat (repeat (par_head ++ elist_head) remaining) ++
        terminal_score components connective ++
        concat (rev (repeat (close ++ close) remaining)) ++ close)
  end.

Lemma flattened_score_loops_equal_recursive_score : forall depth components connective,
  concat (repeat (par_head ++ elist_head) depth) ++ terminal_score components connective ++
    concat (rev (repeat (close ++ close) depth)) =
  recursive_par_score depth components connective.
Proof.
  induction depth as [|depth induction]; intros components connective.
  - cbn [repeat concat rev recursive_par_score]. apply app_nil_r.
  - cbn [repeat concat rev recursive_par_score]. rewrite concat_app.
    cbn [concat]. rewrite app_nil_r.
    rewrite <- (induction components connective).
    repeat rewrite app_assoc. reflexivity.
Qed.

Theorem direct_score_equals_recursive_semantic_score : forall depth components connective,
  direct_expr_score depth components connective = recursive_expr_score depth components connective.
Proof.
  intros [|depth] components connective; [reflexivity|].
  unfold direct_expr_score, recursive_expr_score.
  rewrite <- (flattened_score_loops_equal_recursive_score depth components connective).
  repeat rewrite app_assoc. reflexivity.
Qed.

Lemma integer_atom_length : forall value, length (integer_atom value) = 10.
Proof.
  intro value. unfold integer_atom. rewrite length_app, ChannelImage.encode_fixed_be_length.
  reflexivity.
Qed.

Lemma score_atom_lengths :
  length par_head = 11 /\ length elist_head = 21 /\
  (forall connective, length (score_tail connective) = 11).
Proof.
  unfold par_head, elist_head, score_tail.
  repeat rewrite length_app. repeat rewrite integer_atom_length.
  repeat split; intro; rewrite length_app, integer_atom_length; reflexivity.
Qed.

Lemma recursive_par_score_length : forall depth components connective,
  length (recursive_par_score depth components connective) = length components + 22 + 54 * depth.
Proof.
  induction depth as [|depth induction]; intros components connective.
  - cbn [recursive_par_score]. unfold terminal_score.
    repeat rewrite length_app.
    destruct score_atom_lengths as [parlen [_ taillen]].
    rewrite parlen, taillen. lia.
  - cbn [recursive_par_score]. repeat rewrite length_app.
    destruct score_atom_lengths as [parlen [listlen taillen]].
    unfold close. rewrite parlen, listlen, taillen, induction. lia.
Qed.

Theorem positive_depth_score_length_is_exact : forall depth components connective score,
  direct_expr_score (S depth) components connective = Some score ->
  length score = length components + 54 * S depth.
Proof.
  intros depth components connective score emitted.
  rewrite direct_score_equals_recursive_semantic_score in emitted.
  cbn [recursive_expr_score] in emitted. inversion emitted; subst score.
  change (length (elist_head ++ recursive_par_score depth components connective ++ close) =
    length components + 54 * S depth).
  repeat rewrite length_app. rewrite recursive_par_score_length.
  destruct score_atom_lengths as [_ [listlen taillen]].
  unfold close. rewrite listlen, taillen. lia.
Qed.

Definition endpoint (score expression : bytes) : bytes :=
  [82; 80; 66; 3; 5] ++ score ++ expression ++ ChannelImage.encode_u64_be (length score).

Theorem endpoint_exact_length : forall score expression,
  length (endpoint score expression) = 13 + length score + length expression.
Proof.
  intros. unfold endpoint. repeat rewrite length_app.
  rewrite ChannelImage.u64_big_endian_word_has_exactly_eight_bytes. cbn [length]. lia.
Qed.

Record root_metadata := RootMetadata {
  locally_free : bytes;
  connective_used : bool;
  cardinality : nat
}.

Definition wrapped_metadata (depth : nat) (terminal : root_metadata) : root_metadata :=
  match depth with 0 => terminal | S _ => RootMetadata [] false 1 end.

Theorem zero_depth_preserves_all_root_metadata : forall terminal,
  wrapped_metadata 0 terminal = terminal.
Proof. reflexivity. Qed.

Theorem positive_depth_has_exact_singleton_outer_metadata : forall depth terminal,
  locally_free (wrapped_metadata (S depth) terminal) = [] /\
  connective_used (wrapped_metadata (S depth) terminal) = false /\
  cardinality (wrapped_metadata (S depth) terminal) = 1.
Proof. intros. repeat split; reflexivity. Qed.

Definition checked_size (limit value : nat) : option nat :=
  if value <=? limit then Some value else None.

Theorem checked_size_success_is_exact : forall limit value actual,
  checked_size limit value = Some actual -> actual = value /\ value <= limit.
Proof.
  intros limit value actual success. unfold checked_size in success.
  destruct (value <=? limit) eqn:fits; [|discriminate].
  inversion success; subst. split; [reflexivity|]. now apply Nat.leb_le.
Qed.

Theorem checked_size_refusal_has_no_value : forall limit value,
  limit < value -> checked_size limit value = None.
Proof.
  intros limit value overflow. unfold checked_size.
  assert (value <=? limit = false) by (apply Nat.leb_gt; exact overflow).
  now rewrite H.
Qed.

Theorem final_endpoint_agrees_with_recursive_specification :
  forall depth body components connective expression score,
    emit_expr (plan (S depth) (length body)) body = Some expression ->
    direct_expr_score (S depth) components connective = Some score ->
    endpoint score expression =
      endpoint (elist_head ++ recursive_par_score depth components connective ++ close)
        (wrap_expr (resident_reference depth body)).
Proof.
  intros depth body components connective expression score expression_ok score_ok.
  rewrite planned_expression_is_exact in expression_ok.
  rewrite direct_score_equals_recursive_semantic_score in score_ok.
  cbn [recursive_expr_score] in score_ok.
  injection expression_ok as expression_ok. injection score_ok as score_ok.
  now rewrite <- expression_ok, <- score_ok.
Qed.

Definition bounded_frame (width : nat) (current : frame) : Prop :=
  length (EPM1.encode_varint (child_length current)) <= width /\
  length (EPM1.encode_varint (list_length current)) <= width /\
  length (EPM1.encode_varint (expr_length current)) <= width.

Lemma bounded_frame_prefix_length : forall width current,
  bounded_frame width current -> length (par_prefix current) <= 4 + 3 * width.
Proof.
  intros width current [child [collection expression]].
  unfold par_prefix, expr_prefix. repeat rewrite length_app. cbn [length].
  eapply Nat.le_trans with (m := 1 + (width + (2 + (width + (1 + width))))).
  - apply Nat.add_le_mono_l. apply Nat.add_le_mono; [exact expression|].
    apply Nat.add_le_mono_l. apply Nat.add_le_mono; [exact collection|].
    apply Nat.add_le_mono_l. exact child.
  - lia.
Qed.

Lemma concatenated_length_bounded : forall (A : Type) (emit : A -> bytes) bound items,
  Forall (fun item => length (emit item) <= bound) items ->
  length (concat (map emit items)) <= bound * length items.
Proof.
  intros A emit bound items bounded. induction bounded.
  - cbn [map concat length]. rewrite Nat.mul_0_r. reflexivity.
  - cbn [map concat length]. rewrite length_app. nia.
Qed.

Theorem protobuf_emitted_byte_length_is_linear : forall depth terminal width,
  Forall (bounded_frame width) (plan depth (length terminal)) ->
  length (emit_par (plan depth (length terminal)) terminal) <=
    (4 + 3 * width) * depth + length terminal.
Proof.
  intros depth terminal width bounded. unfold emit_par. rewrite length_app.
  assert (prefixes : Forall (fun current => length (par_prefix current) <= 4 + 3 * width)
    (rev (plan depth (length terminal)))).
  { apply Forall_rev. eapply Forall_impl; [|exact bounded].
    intros current frame_bound. now apply bounded_frame_prefix_length. }
  pose proof (concatenated_length_bounded par_prefix prefixes) as bound.
  rewrite length_rev, planned_frame_count_is_exact in bound. lia.
Qed.

Fixpoint checked_plan (limit depth child : nat) : option (list frame) :=
  match depth with
  | 0 => Some []
  | S remaining =>
      let current := make_frame child in
      match checked_size limit (par_length current) with
      | None => None
      | Some _ =>
          match checked_plan limit remaining (par_length current) with
          | None => None
          | Some frames => Some (current :: frames)
          end
      end
  end.

Theorem checked_plan_success_refines_mathematical_plan : forall limit depth child frames,
  checked_plan limit depth child = Some frames -> frames = plan depth child.
Proof.
  intros limit depth. induction depth as [|depth induction]; intros child frames success.
  - cbn [checked_plan] in success. now injection success.
  - cbn [checked_plan] in success.
    destruct (checked_size limit (par_length (make_frame child))); [|discriminate].
    destruct (checked_plan limit depth (par_length (make_frame child))) as [rest|] eqn:child_ok;
      [|discriminate].
    injection success as same. subst frames. cbn [plan].
    now rewrite (induction _ _ child_ok).
Qed.

Definition reserved_key_capacity (depth component_size expression_size : nat) : nat :=
  5 + (component_size + depth * 44 + 64) + expression_size + 8.

Definition checked_positive_endpoint (limit depth : nat) (body components : bytes)
    (connective : bool) : option bytes :=
  match checked_plan limit (S depth) (length body) with
  | None => None
  | Some frames =>
      match emit_expr frames body, direct_expr_score (S depth) components connective with
      | Some expression, Some score =>
          match checked_size limit
            (reserved_key_capacity (S depth) (length components) (length expression)) with
          | None => None
          | Some _ => Some (endpoint score expression)
          end
      | _, _ => None
      end
  end.

Theorem checked_endpoint_success_refines_recursive_specification :
  forall limit depth body components connective result,
    checked_positive_endpoint limit depth body components connective = Some result ->
    result = endpoint
      (elist_head ++ recursive_par_score depth components connective ++ close)
      (wrap_expr (resident_reference depth body)).
Proof.
  intros limit depth body components connective result success.
  unfold checked_positive_endpoint in success.
  destruct (checked_plan limit (S depth) (length body)) as [frames|] eqn:planned;
    [|discriminate].
  pose proof (@checked_plan_success_refines_mathematical_plan _ _ _ _ planned) as exact_plan.
  subst frames.
  destruct (emit_expr (plan (S depth) (length body)) body) as [expression|] eqn:emitted;
    [|discriminate].
  destruct (direct_expr_score (S depth) components connective) as [score|] eqn:scored;
    [|discriminate].
  destruct (checked_size limit
    (reserved_key_capacity (S depth) (length components) (length expression)));
    [|discriminate].
  injection success as same. subst result.
  now apply final_endpoint_agrees_with_recursive_specification.
Qed.

Theorem frame_refusal_cannot_publish_endpoint : forall limit depth body components connective,
  checked_plan limit (S depth) (length body) = None ->
  checked_positive_endpoint limit depth body components connective = None.
Proof. intros. unfold checked_positive_endpoint. now rewrite H. Qed.

Theorem reservation_refusal_cannot_publish_endpoint :
  forall limit depth body components connective frames expression score,
    checked_plan limit (S depth) (length body) = Some frames ->
    emit_expr frames body = Some expression ->
    direct_expr_score (S depth) components connective = Some score ->
    limit < reserved_key_capacity (S depth) (length components) (length expression) ->
    checked_positive_endpoint limit depth body components connective = None.
Proof.
  intros limit depth body components connective frames expression score planned emitted scored refusal.
  unfold checked_positive_endpoint. rewrite planned, emitted, scored.
  now rewrite checked_size_refusal_has_no_value.
Qed.

Definition modeled_loop_and_copy_cost (depth : nat) (body components : bytes)
    (connective : bool) : nat :=
  4 * S depth +
  2 * length (emit_par (plan (S depth) (length body)) body) +
  length (elist_head ++ recursive_par_score depth components connective ++ close) + 13.

Theorem modeled_loop_and_copy_cost_is_linear : forall depth body components connective width,
  Forall (bounded_frame width) (plan (S depth) (length body)) ->
  modeled_loop_and_copy_cost depth body components connective <=
    (66 + 6 * width) * S depth + 2 * length body + length components + 13.
Proof.
  intros depth body components connective width bounded.
  pose proof (@protobuf_emitted_byte_length_is_linear _ _ _ bounded) as body_bound.
  pose proof (@positive_depth_score_length_is_exact depth components connective
    (elist_head ++ recursive_par_score depth components connective ++ close)) as score_bound.
  assert (scored : direct_expr_score (S depth) components connective =
    Some (elist_head ++ recursive_par_score depth components connective ++ close)).
  { rewrite direct_score_equals_recursive_semantic_score. reflexivity. }
  specialize (score_bound scored). unfold modeled_loop_and_copy_cost.
  rewrite score_bound. nia.
Qed.

Definition gint_zero_body : bytes := [42; 2; 16; 0].
Definition gint_zero_components : bytes := [2] ++ integer_atom 2 ++ integer_atom 0 ++ [0].

Theorem gint_zero_qualified_endpoint : forall limit depth result,
  checked_positive_endpoint limit depth gint_zero_body gint_zero_components false = Some result ->
  result = endpoint
    (elist_head ++ recursive_par_score depth gint_zero_components false ++ close)
    (wrap_expr (resident_reference depth gint_zero_body)).
Proof. intros. now apply checked_endpoint_success_refines_recursive_specification in H. Qed.

Theorem representable_score_trailer_round_trips : forall (score suffix : bytes),
  length score < ChannelImage.u64_bound ->
  ChannelImage.decode_u64_be (ChannelImage.encode_u64_be (length score) ++ suffix) =
    Some (length score, suffix).
Proof. intros. now apply ChannelImage.u64_big_endian_round_trip. Qed.

Print Assumptions planned_emission_equals_repeated_resident_wrapping.
Print Assumptions planned_frame_count_is_exact.
Print Assumptions planned_expression_is_exact.
Print Assumptions zero_depth_preserves_terminal_bytes.
Print Assumptions positive_plan_has_one_outer_expression.
Print Assumptions expression_emission_excludes_only_the_outer_par_header.
Print Assumptions direct_score_equals_recursive_semantic_score.
Print Assumptions positive_depth_score_length_is_exact.
Print Assumptions endpoint_exact_length.
Print Assumptions zero_depth_preserves_all_root_metadata.
Print Assumptions positive_depth_has_exact_singleton_outer_metadata.
Print Assumptions checked_size_success_is_exact.
Print Assumptions checked_size_refusal_has_no_value.
Print Assumptions final_endpoint_agrees_with_recursive_specification.
Print Assumptions protobuf_emitted_byte_length_is_linear.
Print Assumptions checked_plan_success_refines_mathematical_plan.
Print Assumptions checked_endpoint_success_refines_recursive_specification.
Print Assumptions frame_refusal_cannot_publish_endpoint.
Print Assumptions reservation_refusal_cannot_publish_endpoint.
Print Assumptions modeled_loop_and_copy_cost_is_linear.
Print Assumptions gint_zero_qualified_endpoint.
Print Assumptions representable_score_trailer_round_trips.

End UnaryEListConstruction.
