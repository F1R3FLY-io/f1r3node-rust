From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1.

Import ListNotations.
Set Implicit Arguments.

Module SubstitutionRegionEmission.

Definition bytes := list nat.
Inductive mode := Original | Shallow | Compact.

Definition header (tag size : nat) : bytes :=
  EPM1.encode_varint (8 * tag + 2) ++ EPM1.encode_varint size.

Definition delimited (tag : nat) (body : bytes) : bytes :=
  header tag (length body) ++ body.

Definition keep_child (m : mode) (compact original : nat) (retain : bool) : bool :=
  match m with
  | Compact => negb ((compact =? 0) && negb (original =? 0) && negb retain)
  | _ => true
  end.

Definition value_body (m : mode) (body : bytes) : bytes :=
  match m with Original => body | _ => [] end.

Definition value_frame (m : mode) (body : bytes) : bytes :=
  EPM1.encode_varint (length (value_body m body)) ++ value_body m body.

Inductive piece :=
| Borrowed : bytes -> piece
| Child : nat -> nat -> bool -> piece
| Par : nat -> nat -> piece
| EPathMap : nat -> nat -> piece.

Section Arena.

Variable nodes : nat -> list piece.
Variable reference : nat -> mode -> bytes.
Variable cuts : nat -> bytes.
Variable group_prefix : nat -> bytes.
Variable group_cuts : nat -> list nat.

Definition group_body (m : mode) (g : nat) : bytes :=
  group_prefix g ++ concat (map (fun cut => value_frame m (cuts cut)) (group_cuts g)).

Definition piece_bytes (m : mode) (p : piece) : bytes :=
  match p with
  | Borrowed body => body
  | Child tag node retain =>
      if keep_child m (length (reference node Compact)) (length (reference node Original)) retain
      then delimited tag (reference node m) else []
  | Par tag cut =>
      match m with
      | Original => delimited tag (cuts cut)
      | Shallow => delimited tag []
      | Compact => []
      end
  | EPathMap tag group => delimited tag (group_body m group)
  end.

Definition pieces_bytes (m : mode) (ps : list piece) : bytes :=
  concat (map (piece_bytes m) ps).

Definition reference_sound : Prop :=
  forall node m, reference node m = pieces_bytes m (nodes node).

Inductive task :=
| VisitNode : nat -> task
| VisitPiece : piece -> task.

Definition task_bytes (m : mode) (t : task) : bytes :=
  match t with
  | VisitNode node => reference node m
  | VisitPiece p => piece_bytes m p
  end.

Definition pending_bytes (m : mode) (work : list task) : bytes :=
  concat (map (task_bytes m) work).

Definition expand_piece (m : mode) (p : piece) : bytes * list task :=
  match p with
  | Child tag node retain =>
      if keep_child m (length (reference node Compact)) (length (reference node Original)) retain
      then (header tag (length (reference node m)), [VisitNode node])
      else ([], [])
  | _ => (piece_bytes m p, [])
  end.

Definition step (m : mode) (work : list task) (out : bytes)
    : option (list task * bytes) :=
  match work with
  | [] => None
  | VisitNode node :: rest => Some (map VisitPiece (nodes node) ++ rest, out)
  | VisitPiece p :: rest =>
      let '(prefix, added) := expand_piece m p in
      Some (added ++ rest, out ++ prefix)
  end.

Inductive execution (m : mode) : list task -> bytes -> list task -> bytes -> Prop :=
| Done : forall work out, execution m work out work out
| More : forall work out next written final result,
    step m work out = Some (next, written) ->
    execution m next written final result ->
    execution m work out final result.

Lemma pending_app : forall m left right,
  pending_bytes m (left ++ right) = pending_bytes m left ++ pending_bytes m right.
Proof. intros; unfold pending_bytes; now rewrite map_app, concat_app. Qed.

Lemma mapped_pieces_exact : forall m ps,
  pending_bytes m (map VisitPiece ps) = pieces_bytes m ps.
Proof.
  intros; unfold pending_bytes, pieces_bytes.
  rewrite map_map; reflexivity.
Qed.

Lemma expanded_piece_exact : forall m p,
  fst (expand_piece m p) ++ pending_bytes m (snd (expand_piece m p)) = piece_bytes m p.
Proof.
  intros m p; destruct p; cbn [expand_piece].
  - unfold pending_bytes; simpl; apply app_nil_r.
  - unfold piece_bytes; destruct (keep_child _ _ _ _); simpl.
    + unfold pending_bytes, task_bytes, delimited; simpl.
      now rewrite app_nil_r.
    + reflexivity.
  - unfold pending_bytes; simpl; apply app_nil_r.
  - unfold pending_bytes; simpl; apply app_nil_r.
Qed.

Theorem emit_step_preserves_ordered_pending_bytes :
  reference_sound ->
  forall m work out next written,
    step m work out = Some (next, written) ->
    out ++ pending_bytes m work = written ++ pending_bytes m next.
Proof.
  intros Sound m work out next written H.
  destruct work as [|t rest]; [discriminate|].
  destruct t as [node|p].
  - cbn [step] in H; inversion H; subst.
    rewrite pending_app, mapped_pieces_exact.
    unfold pending_bytes at 1; cbn [map concat task_bytes].
    now rewrite Sound.
  - cbn [step] in H.
    destruct (expand_piece m p) as [prefix added] eqn:E.
    inversion H; subst.
    rewrite pending_app.
    unfold pending_bytes at 1; cbn [map concat task_bytes].
    pose proof (expanded_piece_exact m p) as Exact.
    rewrite E in Exact; simpl in Exact.
    rewrite <- Exact; repeat rewrite app_assoc; reflexivity.
Qed.

Theorem execution_preserves_ordered_pending_bytes :
  reference_sound ->
  forall m work out final result,
    execution m work out final result ->
    out ++ pending_bytes m work = result ++ pending_bytes m final.
Proof.
  intros Sound m work out final result Run; induction Run.
  - reflexivity.
  - transitivity (written ++ pending_bytes m next).
    + eapply emit_step_preserves_ordered_pending_bytes; eauto.
    + exact IHRun.
Qed.

Theorem worklist_emission_equals_mode_reference :
  reference_sound ->
  forall m root result,
    execution m [VisitNode root] [] [] result ->
    result = reference root m.
Proof.
  intros Sound m root result Run.
  pose proof (execution_preserves_ordered_pending_bytes Sound Run) as H.
  unfold pending_bytes, task_bytes in H; simpl in H.
  now rewrite !app_nil_r in H.
Qed.

Theorem reverse_push_visits_pieces_in_source_order :
  forall (ps : list piece) rest,
    rev (rest ++ rev (map VisitPiece ps)) = map VisitPiece ps ++ rev rest.
Proof. intros; now rewrite rev_app_distr, rev_involutive. Qed.

Variable rank : task -> nat.
Hypothesis node_rank : forall node,
  rank (VisitNode node) =
    S (fold_right (fun t n => rank t + n) 0 (map VisitPiece (nodes node))).
Hypothesis piece_rank : forall m p,
  rank (VisitPiece p) >
    fold_right (fun t n => rank t + n) 0 (snd (expand_piece m p)).

Definition work_rank (work : list task) : nat :=
  fold_right (fun t n => rank t + n) 0 work.

Lemma work_rank_app : forall left right,
  work_rank (left ++ right) = work_rank left + work_rank right.
Proof. induction left; intros; simpl; unfold work_rank in *; simpl in *; auto; rewrite IHleft; lia. Qed.

Theorem every_emit_step_decreases_rank :
  forall m work out next written,
    step m work out = Some (next, written) ->
    work_rank next < work_rank work.
Proof.
  intros m work out next written H.
  destruct work as [|t rest]; [discriminate|].
  destruct t as [node|p].
  - cbn [step] in H; inversion H; subst.
    rewrite work_rank_app; unfold work_rank; simpl; rewrite node_rank; lia.
  - cbn [step] in H; destruct (expand_piece m p) as [prefix added] eqn:E.
    inversion H; subst; rewrite work_rank_app.
    pose proof (piece_rank m p) as R; rewrite E in R; simpl in R.
    unfold work_rank; simpl; lia.
Qed.

Theorem ranked_emission_terminates_without_fuel :
  forall m work out, exists result, execution m work out [] result.
Proof.
  intros m work.
  induction work using (well_founded_induction_type (well_founded_ltof _ work_rank)).
  intros out; destruct work as [|t rest].
  - exists out; constructor.
  - destruct (step m (t :: rest) out) as [[next written]|] eqn:E.
    + assert (D : work_rank next < work_rank (t :: rest)).
      { eapply every_emit_step_decreases_rank; exact E. }
      destruct (H next D written) as [result Run].
      exists result; econstructor; eauto.
    + destruct t; cbn [step] in E; [discriminate|].
      destruct (expand_piece m p); discriminate.
Qed.

End Arena.

Theorem compact_preserves_required_empty_envelopes : forall original,
  keep_child Compact 0 original true = true.
Proof. intros; unfold keep_child; simpl; now rewrite andb_false_r. Qed.

Theorem compact_retains_genuinely_empty_repeated_messages : forall compact,
  keep_child Compact compact 0 false = true.
Proof. intros; unfold keep_child; simpl; now rewrite andb_false_r. Qed.

Theorem compact_omits_only_cut_emptied_unrequired_envelopes : forall compact original retain,
  keep_child Compact compact original retain = false <->
    compact = 0 /\ original <> 0 /\ retain = false.
Proof.
  intros; unfold keep_child.
  rewrite negb_false_iff, !andb_true_iff, Nat.eqb_eq, negb_true_iff, Nat.eqb_neq, negb_true_iff.
  tauto.
Qed.

Theorem original_par_keeps_present_empty_header : forall tag,
  delimited tag [] = header tag 0.
Proof. intros; unfold delimited; simpl; apply app_nil_r. Qed.

Theorem epm_shallow_and_compact_preserve_value_count :
  forall values,
  map (value_frame Shallow) values = repeat [0] (length values) /\
  map (value_frame Compact) values = repeat [0] (length values).
Proof.
  induction values as [|value rest [HS HC]].
  - split; reflexivity.
  - change ([0] :: map (value_frame Shallow) rest = [0] :: repeat [0] (length rest) /\
            [0] :: map (value_frame Compact) rest = [0] :: repeat [0] (length rest)).
    now rewrite HS, HC.
Qed.

Theorem epm_original_value_order : forall left right,
  concat (map (value_frame Original) (left ++ right)) =
    concat (map (value_frame Original) left) ++ concat (map (value_frame Original) right).
Proof. intros; now rewrite map_app, concat_app. Qed.

Definition checked_add (limit a b : nat) : option nat :=
  if a + b <=? limit then Some (a + b) else None.

Theorem checked_add_success_exact : forall limit a b total,
  checked_add limit a b = Some total -> total = a + b /\ total <= limit.
Proof.
  intros; unfold checked_add in H; destruct (a + b <=? limit) eqn:E; inversion H; subst.
  apply Nat.leb_le in E; auto.
Qed.

Theorem checked_add_refuses_overflow : forall limit a b,
  checked_add limit a b = None <-> limit < a + b.
Proof.
  intros; unfold checked_add; destruct (a + b <=? limit) eqn:E.
  - apply Nat.leb_le in E; split; [discriminate|lia].
  - apply Nat.leb_gt in E; split; auto.
Qed.

Fixpoint checked_total (limit total : nat) (sizes : list nat) : option nat :=
  match sizes with
  | [] => Some total
  | size :: rest =>
      match checked_add limit total size with
      | None => None
      | Some next => checked_total limit next rest
      end
  end.

Theorem checked_total_success_exact : forall sizes limit total result,
  checked_total limit total sizes = Some result ->
    result = total + fold_right Nat.add 0 sizes.
Proof.
  induction sizes; intros; simpl in H.
  - inversion H; simpl; lia.
  - destruct (checked_add limit total a) eqn:E; [|discriminate].
    apply checked_add_success_exact in E; destruct E as [-> _].
    specialize (IHsizes _ _ _ H); simpl; lia.
Qed.

Theorem checked_total_complete : forall sizes limit total,
  total + fold_right Nat.add 0 sizes <= limit ->
  checked_total limit total sizes = Some (total + fold_right Nat.add 0 sizes).
Proof.
  induction sizes; intros; simpl in *.
  - now rewrite Nat.add_0_r.
  - unfold checked_add.
    assert (E : (total + a <=? limit) = true) by (apply Nat.leb_le; lia).
    rewrite E; rewrite IHsizes by lia; f_equal; lia.
Qed.

Theorem checked_total_refuses_unrepresentable_nonempty :
  forall sizes limit total,
  sizes <> [] -> limit < total + fold_right Nat.add 0 sizes ->
  checked_total limit total sizes = None.
Proof.
  induction sizes; intros limit total N H; [contradiction|].
  simpl; unfold checked_add; destruct (total + a <=? limit) eqn:E; auto.
  destruct sizes.
  - apply Nat.leb_le in E; simpl in *; lia.
  - apply IHsizes; [discriminate|]; simpl in *; lia.
Qed.

Example dropping_genuine_empty_envelope_is_observable :
  keep_child Compact 0 0 false <> false.
Proof. discriminate. Qed.

Example ignoring_presence_flag_is_observable :
  keep_child Compact 0 1 true <> keep_child Compact 0 1 false.
Proof. discriminate. Qed.

Example reversed_value_order_is_observable :
  concat (map (value_frame Original) [[1]; [2]]) <>
  concat (map (value_frame Original) [[2]; [1]]).
Proof. discriminate. Qed.

Example missing_empty_value_is_observable :
  concat (map (value_frame Shallow) [[]; [7]]) <>
  concat (map (value_frame Shallow) [[7]]).
Proof. discriminate. Qed.

Example unchecked_wrap_is_observable :
  checked_add 255 255 1 = None /\ (255 + 1) mod 256 = 0.
Proof. split; reflexivity. Qed.

Print Assumptions emit_step_preserves_ordered_pending_bytes.
Print Assumptions execution_preserves_ordered_pending_bytes.
Print Assumptions worklist_emission_equals_mode_reference.
Print Assumptions reverse_push_visits_pieces_in_source_order.
Print Assumptions every_emit_step_decreases_rank.
Print Assumptions ranked_emission_terminates_without_fuel.
Print Assumptions compact_preserves_required_empty_envelopes.
Print Assumptions compact_retains_genuinely_empty_repeated_messages.
Print Assumptions compact_omits_only_cut_emptied_unrequired_envelopes.
Print Assumptions original_par_keeps_present_empty_header.
Print Assumptions epm_shallow_and_compact_preserve_value_count.
Print Assumptions epm_original_value_order.
Print Assumptions checked_add_success_exact.
Print Assumptions checked_add_refuses_overflow.
Print Assumptions checked_total_success_exact.
Print Assumptions checked_total_complete.
Print Assumptions checked_total_refuses_unrepresentable_nonempty.
Print Assumptions dropping_genuine_empty_envelope_is_observable.
Print Assumptions ignoring_presence_flag_is_observable.
Print Assumptions reversed_value_order_is_observable.
Print Assumptions missing_empty_value_is_observable.
Print Assumptions unchecked_wrap_is_observable.

Definition checked_delimited (limit word64 tag size : nat) : option nat :=
  if size <=? word64 then
    match checked_add limit (length (EPM1.encode_varint (8 * tag + 2)))
                            (length (EPM1.encode_varint size)) with
    | None => None
    | Some prefix => checked_add limit prefix size
    end
  else None.

Theorem checked_delimited_success_exact : forall limit word64 tag body size,
  checked_delimited limit word64 tag (length body) = Some size ->
  size = length (delimited tag body) /\ size <= limit /\ length body <= word64.
Proof.
  intros limit word64 tag body size H; unfold checked_delimited in H.
  destruct (length body <=? word64) eqn:W; [|discriminate].
  destruct (checked_add limit _ _) eqn:A; [|discriminate].
  apply checked_add_success_exact in A; destruct A as [-> _].
  apply checked_add_success_exact in H; destruct H as [-> L].
  apply Nat.leb_le in W.
  unfold delimited, header; repeat rewrite length_app; auto.
Qed.

Theorem checked_delimited_refuses_u64_overflow : forall limit word64 tag size,
  word64 < size -> checked_delimited limit word64 tag size = None.
Proof.
  intros; unfold checked_delimited.
  assert (E : (size <=? word64) = false) by (apply Nat.leb_gt; lia).
  now rewrite E.
Qed.

Fixpoint checked_frames (limit word64 : nat) (m : mode)
    (total : nat) (values : list bytes) : option nat :=
  match values with
  | [] => Some total
  | body :: rest =>
      let size := length (value_body m body) in
      if size <=? word64 then
        match checked_add limit total (length (EPM1.encode_varint size)) with
        | None => None
        | Some prefix =>
            match checked_add limit prefix size with
            | None => None
            | Some next => checked_frames limit word64 m next rest
            end
        end
      else None
  end.

Theorem checked_frames_success_exact : forall values limit word64 m total result,
  checked_frames limit word64 m total values = Some result ->
  result = total + length (concat (map (value_frame m) values)).
Proof.
  induction values as [|body rest IH]; intros limit word64 m total result H.
  - simpl in *; inversion H; lia.
  - cbn [checked_frames] in H.
    destruct (_ <=? word64); [|discriminate].
    destruct (checked_add limit total _) eqn:A; [|discriminate].
    destruct (checked_add limit n _) eqn:B; [|discriminate].
    apply checked_add_success_exact in A; apply checked_add_success_exact in B.
    specialize (IH _ _ _ _ _ H).
    cbn [map concat]; rewrite length_app.
    unfold value_frame at 1; rewrite length_app.
    destruct A as [A _]; destruct B as [B _].
    rewrite IH, B, A, !Nat.add_assoc; reflexivity.
Qed.

Section Planner.

Variable reference : nat -> mode -> bytes.
Variable cuts : nat -> bytes.
Variable group_prefix : nat -> bytes.
Variable group_cuts : nat -> list nat.
Variable cached : nat -> mode -> nat.
Hypothesis cached_correct : forall node m, cached node m = length (reference node m).
Variable limit word64 : nat.

Definition planned_piece_size (m : mode) (p : piece) : option nat :=
  match p with
  | Borrowed body => Some (length body)
  | Child tag node retain =>
      if keep_child m (cached node Compact) (cached node Original) retain
      then checked_delimited limit word64 tag (cached node m)
      else Some 0
  | Par tag cut =>
      match m with
      | Original => checked_delimited limit word64 tag (length (cuts cut))
      | Shallow => checked_delimited limit word64 tag 0
      | Compact => Some 0
      end
  | EPathMap tag group =>
      match checked_frames limit word64 m (length (group_prefix group))
        (map cuts (group_cuts group)) with
      | None => None
      | Some size => checked_delimited limit word64 tag size
      end
  end.

Theorem planned_piece_success_has_exact_mode_length : forall m p size,
  planned_piece_size m p = Some size ->
  size = length (piece_bytes reference cuts group_prefix group_cuts m p).
Proof.
  intros m p size H; destruct p; cbn [planned_piece_size piece_bytes] in *.
  - now inversion H.
  - rewrite !cached_correct in H.
    destruct (keep_child _ _ _ _).
    + apply checked_delimited_success_exact in H; tauto.
    + now inversion H.
  - destruct m.
    + apply checked_delimited_success_exact in H; tauto.
    + change (checked_delimited limit word64 n (length ([] : bytes)) = Some size) in H.
      apply checked_delimited_success_exact in H; tauto.
    + now inversion H.
  - destruct (checked_frames _ _ _ _ _) eqn:E; [|discriminate].
    apply checked_frames_success_exact in E.
    rewrite E in H; rewrite map_map in H; rewrite <- length_app in H.
    change (checked_delimited limit word64 n
      (length (group_body cuts group_prefix group_cuts m n0)) = Some size) in H.
    apply checked_delimited_success_exact in H; tauto.
Qed.

Fixpoint planned_node_size (m : mode) (total : nat) (ps : list piece) : option nat :=
  match ps with
  | [] => Some total
  | p :: rest =>
      match planned_piece_size m p with
      | None => None
      | Some size =>
          match checked_add limit total size with
          | None => None
          | Some next => planned_node_size m next rest
          end
      end
  end.

Theorem finished_node_success_has_exact_mode_length : forall ps m total result,
  planned_node_size m total ps = Some result ->
  result = total + length (pieces_bytes reference cuts group_prefix group_cuts m ps).
Proof.
  induction ps as [|p rest IH]; intros m total result H.
  - unfold pieces_bytes; simpl in *; inversion H; lia.
  - cbn [planned_node_size] in H.
    destruct (planned_piece_size m p) eqn:E; [|discriminate].
    destruct (checked_add limit total n) eqn:A; [|discriminate].
    apply planned_piece_success_has_exact_mode_length in E.
    apply checked_add_success_exact in A.
    specialize (IH _ _ _ H).
    unfold pieces_bytes in *; cbn [map concat]; rewrite length_app.
    destruct A as [A _]; lia.
Qed.

Theorem finished_node_success_is_representable : forall ps m total result,
  total <= limit ->
  planned_node_size m total ps = Some result -> result <= limit.
Proof.
  induction ps as [|p rest IH]; intros m total result Bound H.
  - simpl in H; inversion H; subst; exact Bound.
  - cbn [planned_node_size] in H.
    destruct (planned_piece_size m p); [|discriminate].
    destruct (checked_add limit total n) eqn:A; [|discriminate].
    apply checked_add_success_exact in A; destruct A as [_ B].
    exact (IH m n0 result B H).
Qed.

Definition publish_lengths (ps : list piece) (prior : list (nat * nat * nat))
    : option (list (nat * nat * nat)) :=
  match planned_node_size Compact 0 ps, planned_node_size Shallow 0 ps,
        planned_node_size Original 0 ps with
  | Some compact, Some shallow, Some original => Some (prior ++ [(compact, shallow, original)])
  | _, _, _ => None
  end.

Theorem failed_mode_does_not_publish : forall ps prior m,
  planned_node_size m 0 ps = None -> publish_lengths ps prior = None.
Proof.
  intros ps prior m H; unfold publish_lengths.
  destruct m; rewrite H;
  repeat match goal with
  | |- context[planned_node_size ?m 0 ps] => destruct (planned_node_size m 0 ps)
  end; reflexivity.
Qed.

End Planner.

Print Assumptions checked_delimited_success_exact.
Print Assumptions checked_delimited_refuses_u64_overflow.
Print Assumptions checked_frames_success_exact.
Print Assumptions planned_piece_success_has_exact_mode_length.
Print Assumptions finished_node_success_has_exact_mode_length.
Print Assumptions finished_node_success_is_representable.
Print Assumptions failed_mode_does_not_publish.


Definition fixture_nodes (node : nat) : list piece :=
  match node with
  | 0 => [Par 1 0; Borrowed [16; 1]]
  | 1 => [Borrowed [8; 1]; Child 3 0 true; EPathMap 4 0]
  | _ => []
  end.

Definition fixture_cuts (cut : nat) : bytes :=
  match cut with 0 => [] | _ => [8; 1] end.
Definition fixture_prefix (_ : nat) : bytes := [69; 80; 77; 49].
Definition fixture_groups (_ : nat) : list nat := [0; 1].

Definition fixture_leaf (m : mode) : bytes :=
  match m with Compact => [16; 1] | _ => delimited 1 [] ++ [16; 1] end.

Definition fixture_reference (node : nat) (m : mode) : bytes :=
  match node with
  | 0 => fixture_leaf m
  | 1 => [8; 1] ++ delimited 3 (fixture_leaf m) ++
      delimited 4 (group_body fixture_cuts fixture_prefix fixture_groups m 0)
  | _ => []
  end.

Definition fixture_node_rank (node : nat) : nat :=
  match node with 0 => 3 | 1 => 7 | _ => 1 end.
Definition fixture_rank (t : task) : nat :=
  match t with
  | VisitNode node => fixture_node_rank node
  | VisitPiece (Child _ node _) => S (fixture_node_rank node)
  | VisitPiece _ => 1
  end.

Theorem fixture_reference_is_sound :
  reference_sound fixture_nodes fixture_reference fixture_cuts fixture_prefix fixture_groups.
Proof. intros [|[|node]] m; destruct m; reflexivity. Qed.

Theorem fixture_ranks_are_valid :
  (forall node, fixture_rank (VisitNode node) =
    S (fold_right (fun t n => fixture_rank t + n) 0
      (map VisitPiece (fixture_nodes node)))) /\
  (forall m p, fixture_rank (VisitPiece p) >
    fold_right (fun t n => fixture_rank t + n) 0
      (snd (expand_piece fixture_reference fixture_cuts fixture_prefix fixture_groups m p))).
Proof.
  split.
  - intros [|[|node]]; reflexivity.
  - intros m p; destruct p; cbn [fixture_rank expand_piece snd fold_right]; try lia.
    destruct (keep_child _ _ _ _); cbn [snd fold_right fixture_rank]; lia.
Qed.

Theorem fixture_emits_all_modes_without_fuel : forall m,
  exists result,
    execution fixture_nodes fixture_reference fixture_cuts fixture_prefix fixture_groups
      m [VisitNode 1] [] [] result /\
    result = fixture_reference 1 m.
Proof.
  intro m; destruct fixture_ranks_are_valid as [N P].
  destruct (@ranked_emission_terminates_without_fuel fixture_nodes fixture_reference
    fixture_cuts fixture_prefix fixture_groups fixture_rank N P m [VisitNode 1] [])
    as [result Run].
  exists result; split; [exact Run|].
  eapply worklist_emission_equals_mode_reference; eauto using fixture_reference_is_sound.
Qed.

Theorem fixture_planner_accepts_all_modes : forall m,
  planned_node_size fixture_cuts fixture_prefix fixture_groups
    (fun node mode => length (fixture_reference node mode)) 255 255 m 0 (fixture_nodes 1) =
    Some (length (fixture_reference 1 m)).
Proof. intros []; reflexivity. Qed.

Example reversed_neighbor_pieces_are_observable :
  pieces_bytes fixture_reference fixture_cuts fixture_prefix fixture_groups Original
    [Borrowed [8; 1]; Borrowed [16; 2]] <>
  pieces_bytes fixture_reference fixture_cuts fixture_prefix fixture_groups Original
    [Borrowed [16; 2]; Borrowed [8; 1]].
Proof. discriminate. Qed.

Example stale_child_cache_is_observable :
  planned_piece_size fixture_cuts fixture_prefix fixture_groups
    (fun _ _ => 0) 255 255 Original (Child 3 0 true) <>
  Some (length (piece_bytes fixture_reference fixture_cuts fixture_prefix fixture_groups
    Original (Child 3 0 true))).
Proof. discriminate. Qed.

Print Assumptions fixture_reference_is_sound.
Print Assumptions fixture_ranks_are_valid.
Print Assumptions fixture_emits_all_modes_without_fuel.
Print Assumptions fixture_planner_accepts_all_modes.
Print Assumptions reversed_neighbor_pieces_are_observable.
Print Assumptions stale_child_cache_is_observable.


End SubstitutionRegionEmission.
