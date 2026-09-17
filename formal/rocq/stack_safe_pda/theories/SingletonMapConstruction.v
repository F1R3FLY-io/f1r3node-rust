From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPM1 UnaryEListConstruction CanonicalEPathMapScore.

Import ListNotations.
Set Implicit Arguments.

Module SingletonMapConstruction.
Module U := UnaryEListConstruction.UnaryEListConstruction.
Module S := CanonicalEPathMapScore.CanonicalEPathMapScore.
Definition bytes := list nat.

Definition snapshot (prefix child : bytes) : bytes :=
  prefix ++ EPM1.encode_varint (length child) ++ child.
Definition expression (prefix child : bytes) : bytes :=
  U.field [130; 2] (U.field [74] (snapshot prefix child)).
Definition wrap (prefix child : bytes) : bytes := U.field [42] (expression prefix child).

Fixpoint resident (prefix : bytes) (depth : nat) (terminal : bytes) : bytes :=
  match depth with
  | 0 => terminal
  | S remaining => resident prefix remaining (wrap prefix terminal)
  end.

Record frame := Frame {
  child_size : nat;
  snapshot_size : nat;
  map_size : nat;
  expression_size : nat;
  par_size : nat
}.

Definition make_frame (prefix : bytes) (child : nat) : frame :=
  let snap := length prefix + length (EPM1.encode_varint child) + child in
  let map := 1 + length (EPM1.encode_varint snap) + snap in
  let expr := 2 + length (EPM1.encode_varint map) + map in
  let par := 1 + length (EPM1.encode_varint expr) + expr in
  Frame child snap map expr par.

Definition header (prefix : bytes) (f : frame) : bytes :=
  [42] ++ EPM1.encode_varint (expression_size f) ++
  [130; 2] ++ EPM1.encode_varint (map_size f) ++
  [74] ++ EPM1.encode_varint (snapshot_size f) ++
  prefix ++ EPM1.encode_varint (child_size f).

Fixpoint plan (prefix : bytes) (depth child : nat) : list frame :=
  match depth with
  | 0 => []
  | S remaining => let f := make_frame prefix child in
      f :: plan prefix remaining (par_size f)
  end.

Definition emit (prefix : bytes) (frames : list frame) (terminal : bytes) : bytes :=
  concat (map (header prefix) (rev frames)) ++ terminal.

Theorem frame_lengths_exact : forall prefix child,
  snapshot_size (make_frame prefix (length child)) = length (snapshot prefix child) /\
  map_size (make_frame prefix (length child)) = length (U.field [74] (snapshot prefix child)) /\
  expression_size (make_frame prefix (length child)) = length (expression prefix child) /\
  par_size (make_frame prefix (length child)) = length (wrap prefix child).
Proof.
  intros. unfold wrap, expression, snapshot, make_frame.
  repeat rewrite U.field_length. repeat rewrite length_app.
  cbn [snapshot_size map_size expression_size par_size length].
  repeat rewrite Nat.add_assoc. repeat split; reflexivity.
Qed.

Lemma header_exact : forall prefix child,
  header prefix (make_frame prefix (length child)) ++ child = wrap prefix child.
Proof.
  intros. unfold header.
  destruct (frame_lengths_exact prefix child) as [Hs [Hm [He Hp]]].
  rewrite Hs, Hm, He. cbn [child_size make_frame].
  unfold wrap, expression, U.field, snapshot. repeat rewrite app_assoc. reflexivity.
Qed.

Theorem planned_emission_refines_resident_wrapping : forall prefix depth terminal,
  emit prefix (plan prefix depth (length terminal)) terminal = resident prefix depth terminal.
Proof.
  intros prefix depth. induction depth as [|depth IH]; intro terminal; [reflexivity|].
  cbn [plan resident]. unfold emit. cbn [rev]. rewrite map_app, concat_app.
  cbn [map concat]. rewrite app_nil_r, <- app_assoc, header_exact.
  destruct (frame_lengths_exact prefix terminal) as [_ [_ [_ H]]].
  rewrite H. exact (IH (wrap prefix terminal)).
Qed.

Theorem exactly_one_handle_frame_per_inner_level : forall prefix depth child,
  length (plan prefix depth child) = depth.
Proof. intros prefix depth. induction depth; intro child; cbn [plan length]; auto. Qed.

Definition final_snapshot (prefix : bytes) (inner_depth : nat) (terminal : bytes) : bytes :=
  snapshot prefix (emit prefix (plan prefix inner_depth (length terminal)) terminal).

Theorem one_outer_map_and_exact_inner_depth : forall prefix depth terminal,
  final_snapshot prefix depth terminal = snapshot prefix (resident prefix depth terminal).
Proof. intros. unfold final_snapshot. now rewrite planned_emission_refines_resident_wrapping. Qed.

Theorem depth_one_retains_terminal_and_outer_map : forall prefix terminal,
  final_snapshot prefix 0 terminal = snapshot prefix terminal.
Proof. reflexivity. Qed.

Lemma zero_count_app : forall left right,
  S.zero_count (left ++ right) = S.zero_count left + S.zero_count right.
Proof.
  induction left as [|head tail IH]; intro right; [reflexivity|].
  cbn [app S.zero_count]. rewrite IH. lia.
Qed.

Theorem snapshot_zero_summary_is_exact : forall prefix child,
  S.zero_count (snapshot prefix child) = S.zero_count prefix +
    S.zero_count (EPM1.encode_varint (length child)) + S.zero_count child.
Proof. intros. unfold snapshot. rewrite !zero_count_app. lia. Qed.

Theorem final_component_score_and_key_are_exact : forall prefix depth terminal,
  let actual := final_snapshot prefix depth terminal in
  let expected := snapshot prefix (resident prefix depth terminal) in
  S.direct_score actual false = S.serialize_node (S.rpb3_pathmap_semantic_score expected false) /\
  S.endpoint_key actual false (U.field [130; 2] (U.field [74] actual)) =
    S.endpoint_key expected false (U.field [130; 2] (U.field [74] expected)).
Proof.
  intros prefix depth terminal. cbn zeta.
  rewrite one_outer_map_and_exact_inner_depth. split.
  - apply S.direct_score_refines_fixed_shape_semantic_serialization.
  - reflexivity.
Qed.

Theorem escaped_score_length_is_exact : forall prefix depth terminal,
  length (S.direct_score (final_snapshot prefix depth terminal) false) =
    26 + length (final_snapshot prefix depth terminal) +
      S.zero_count (final_snapshot prefix depth terminal).
Proof. intros. apply S.direct_score_has_exact_length. Qed.

Definition checked_frame (limit : nat) (prefix : bytes) (child : nat) : option frame :=
  let f := make_frame prefix child in if par_size f <=? limit then Some f else None.

Theorem checked_frame_success_has_exact_representable_lengths : forall limit prefix child f,
  checked_frame limit prefix (length child) = Some f ->
  par_size f = length (wrap prefix child) /\ par_size f <= limit /\
  snapshot_size f <= limit /\ map_size f <= limit /\ expression_size f <= limit.
Proof.
  intros limit prefix child f H. unfold checked_frame in H.
  destruct (par_size (make_frame prefix (length child)) <=? limit) eqn:E; try discriminate.
  inversion H; subst. apply Nat.leb_le in E.
  destruct (frame_lengths_exact prefix child) as [_ [_ [_ exact]]].
  split; [exact exact|]. unfold make_frame in *.
  cbn [par_size expression_size map_size snapshot_size] in *. lia.
Qed.

Theorem frame_overflow_refuses : forall limit prefix child,
  limit < par_size (make_frame prefix child) -> checked_frame limit prefix child = None.
Proof.
  intros limit prefix child H. unfold checked_frame.
  destruct (par_size (make_frame prefix child) <=? limit) eqn:E; [apply Nat.leb_le in E; lia|reflexivity].
Qed.

Definition reserve_endpoint (limit score_bound : nat) (snap expr : bytes) : option bytes :=
  let score := 26 + length snap + S.zero_count snap in
  if (5 + score + length expr + 8 <=? limit) && (score <? score_bound)
  then Some (S.endpoint_key snap false expr) else None.

Theorem endpoint_reservation_refusal_cannot_publish : forall limit score_bound snap expr,
  limit < 39 + length snap + S.zero_count snap + length expr ->
  reserve_endpoint limit score_bound snap expr = None.
Proof.
  intros limit score_bound snap expr H. unfold reserve_endpoint.
  assert (5 + (26 + length snap + S.zero_count snap) + length expr + 8 <=? limit = false) as E
    by (apply Nat.leb_gt; lia).
  rewrite E. reflexivity.
Qed.

Theorem endpoint_success_has_exact_capacity_and_representable_trailer : forall limit score_bound snap expr result,
  reserve_endpoint limit score_bound snap expr = Some result ->
  result = S.endpoint_key snap false expr /\ length result <= limit /\
  length (S.direct_score snap false) < score_bound.
Proof.
  intros limit score_bound snap expr result H. unfold reserve_endpoint in H.
  destruct ((5 + (26 + length snap + S.zero_count snap) + length expr + 8 <=? limit) &&
    (26 + length snap + S.zero_count snap <? score_bound)) eqn:E; try discriminate.
  inversion H; subst. apply Bool.andb_true_iff in E as [capacity trailer].
  apply Nat.leb_le in capacity. apply Nat.ltb_lt in trailer.
  split; [reflexivity|]. split.
  - destruct (S.endpoint_has_exact_bytes_and_length snap false expr) as [_ Hlen]. rewrite Hlen. lia.
  - now rewrite S.direct_score_has_exact_length.
Qed.

Fixpoint checked_plan (limit : nat) (prefix : bytes) (depth child : nat) : option (list frame) :=
  match depth with
  | 0 => Some []
  | S remaining =>
      match checked_frame limit prefix child with
      | None => None
      | Some f => match checked_plan limit prefix remaining (par_size f) with
          | None => None
          | Some rest => Some (f :: rest)
          end
      end
  end.

Theorem checked_plan_success_refines_plan : forall limit prefix depth child frames,
  checked_plan limit prefix depth child = Some frames -> frames = plan prefix depth child.
Proof.
  intros limit prefix depth. induction depth as [|depth IH]; intros child frames H.
  - inversion H. reflexivity.
  - cbn [checked_plan] in H. unfold checked_frame in H.
    destruct (par_size (make_frame prefix child) <=? limit); try discriminate.
    destruct (checked_plan limit prefix depth (par_size (make_frame prefix child))) as [rest|] eqn:E;
      try discriminate.
    inversion H; subst. cbn [plan]. f_equal. now apply IH.
Qed.

Definition checked_endpoint (limit score_bound : nat) (prefix : bytes) (depth : nat) (terminal : bytes) : option bytes :=
  match checked_plan limit prefix depth (length terminal) with
  | None => None
  | Some frames =>
      let snap := snapshot prefix (emit prefix frames terminal) in
      reserve_endpoint limit score_bound snap (U.field [130; 2] (U.field [74] snap))
  end.

Theorem checked_endpoint_success_refines_resident : forall limit score_bound prefix depth terminal result,
  checked_endpoint limit score_bound prefix depth terminal = Some result ->
  let snap := snapshot prefix (resident prefix depth terminal) in
  result = S.endpoint_key snap false (U.field [130; 2] (U.field [74] snap)).
Proof.
  intros limit score_bound prefix depth terminal result H. cbn zeta. unfold checked_endpoint in H.
  destruct (checked_plan limit prefix depth (length terminal)) as [frames|] eqn:E; try discriminate.
  apply checked_plan_success_refines_plan in E. subst frames.
  rewrite planned_emission_refines_resident_wrapping in H.
  apply endpoint_success_has_exact_capacity_and_representable_trailer in H. tauto.
Qed.

Theorem checked_plan_refusal_cannot_publish : forall limit score_bound prefix depth terminal,
  checked_plan limit prefix depth (length terminal) = None ->
  checked_endpoint limit score_bound prefix depth terminal = None.
Proof. intros. unfold checked_endpoint. now rewrite H. Qed.

Theorem fixed_prefix_header_bound : forall prefix f width,
  length (EPM1.encode_varint (expression_size f)) <= width ->
  length (EPM1.encode_varint (map_size f)) <= width ->
  length (EPM1.encode_varint (snapshot_size f)) <= width ->
  length (EPM1.encode_varint (child_size f)) <= width ->
  length (header prefix f) <= 4 + 4 * width + length prefix.
Proof.
  intros prefix f width H H0 H1 H2. unfold header. repeat rewrite length_app. cbn [length].
  eapply Nat.le_trans with
    (m := 1 + (width + (2 + (width + (1 + (width + (length prefix + width))))))).
  - apply Nat.add_le_mono_l. apply Nat.add_le_mono; [exact H|].
    apply Nat.add_le_mono_l. apply Nat.add_le_mono; [exact H0|].
    apply Nat.add_le_mono_l. apply Nat.add_le_mono; [exact H1|].
    apply Nat.add_le_mono_l. exact H2.
  - lia.
Qed.

Theorem emitted_bytes_bound : forall prefix frames terminal bound,
  Forall (fun f => length (header prefix f) <= bound) frames ->
  length (emit prefix frames terminal) <= length frames * bound + length terminal.
Proof.
  intros prefix frames terminal bound H. unfold emit. rewrite length_app.
  assert (Forall (fun f => length (header prefix f) <= bound) (rev frames)) as HR
    by (apply Forall_rev; exact H).
  assert (length (concat (map (header prefix) (rev frames))) <= length (rev frames) * bound) as HC.
  { induction HR; cbn [map concat length]; [lia|]. rewrite length_app. lia. }
  rewrite length_rev in HC. lia.
Qed.

Theorem abstract_prefix_nonvacuity_and_refusal_controls :
  final_snapshot [7; 0; 9] 0 [42; 2; 16; 0] = [7; 0; 9; 4; 42; 2; 16; 0] /\
  final_snapshot [7; 0; 9] 1 [42; 2; 16; 0] <> final_snapshot [7; 0; 9] 0 [42; 2; 16; 0] /\
  checked_endpoint 0 256 [7; 0; 9] 1 [42; 2; 16; 0] = None /\
  checked_endpoint 128 256 [7; 0; 9] 1 [42; 2; 16; 0] <> None /\
  reserve_endpoint 0 256 [1] [2] = None.
Proof. vm_compute. repeat split; discriminate. Qed.

Print Assumptions frame_lengths_exact.
Print Assumptions planned_emission_refines_resident_wrapping.
Print Assumptions exactly_one_handle_frame_per_inner_level.
Print Assumptions one_outer_map_and_exact_inner_depth.
Print Assumptions depth_one_retains_terminal_and_outer_map.
Print Assumptions snapshot_zero_summary_is_exact.
Print Assumptions final_component_score_and_key_are_exact.
Print Assumptions escaped_score_length_is_exact.
Print Assumptions checked_frame_success_has_exact_representable_lengths.
Print Assumptions frame_overflow_refuses.
Print Assumptions endpoint_reservation_refusal_cannot_publish.
Print Assumptions endpoint_success_has_exact_capacity_and_representable_trailer.
Print Assumptions checked_plan_success_refines_plan.
Print Assumptions checked_endpoint_success_refines_resident.
Print Assumptions checked_plan_refusal_cannot_publish.
Print Assumptions fixed_prefix_header_bound.
Print Assumptions emitted_bytes_bound.
Print Assumptions abstract_prefix_nonvacuity_and_refusal_controls.
End SingletonMapConstruction.
