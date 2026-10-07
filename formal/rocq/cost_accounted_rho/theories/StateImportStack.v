From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec StateImportCursor StateImportTraversal StateImportPage.
Import ListNotations.

Record ImportTraversalFrame : Type := {
  import_frame_key : list nat;
  import_frame_prefix : list nat;
  import_frame_edges : list ImportWireEdge;
  import_frame_last_slot : option nat
}.

Definition import_frame_at (key prefix : list nat) (edges : list ImportWireEdge)
    (last_slot : option nat) : ImportTraversalFrame :=
  {| import_frame_key := key; import_frame_prefix := prefix;
     import_frame_edges := edges; import_frame_last_slot := last_slot |}.

Fixpoint import_build_history_stack (fuel : nat) (read : ImportHistoryReader)
    (key context path : list nat) : option (list ImportTraversalFrame) :=
  match fuel with
  | 0 => None
  | S remaining =>
      match read key with
      | None => None
      | Some edges =>
          match path with
          | [] => Some [import_frame_at key context edges None]
          | slot :: tail =>
              match import_wire_slot_lookup slot edges with
              | None => None
              | Some edge =>
                  if import_wire_header edge <? 128 then None else
                    match import_strip_exact_prefix (import_wire_prefix edge) tail with
                    | None => None
                    | Some suffix =>
                        match import_build_history_stack remaining read (import_wire_hash edge)
                            (context ++ slot :: import_wire_prefix edge) suffix with
                        | None => None
                        | Some frames => Some (frames ++ [import_frame_at key context edges (Some slot)])
                        end
                    end
              end
          end
      end
  end.

Inductive import_history_stack (read : ImportHistoryReader) :
    list nat -> list nat -> list nat -> list ImportTraversalFrame -> Prop :=
| import_history_stack_here : forall key context edges,
    read key = Some edges ->
    import_history_stack read key context [] [import_frame_at key context edges None]
| import_history_stack_child : forall key context edges edge suffix frames,
    read key = Some edges ->
    import_wire_slot_lookup (import_wire_slot edge) edges = Some edge ->
    128 <= import_wire_header edge ->
    import_history_stack read (import_wire_hash edge)
      (context ++ import_wire_slot edge :: import_wire_prefix edge) suffix frames ->
    import_history_stack read key context
      (import_wire_slot edge :: import_wire_prefix edge ++ suffix)
      (frames ++ [import_frame_at key context edges (Some (import_wire_slot edge))]).

Theorem import_stack_builder_preserves_exact_frames : forall fuel read key context path frames,
  import_build_history_stack fuel read key context path = Some frames ->
  import_history_stack read key context path frames.
Proof.
  induction fuel as [|fuel IH]; intros read key context path frames Built; [discriminate|].
  cbn [import_build_history_stack] in Built.
  destruct (read key) as [edges|] eqn:Read; try discriminate.
  destruct path as [|slot tail].
  - inversion Built; subst frames. now apply import_history_stack_here.
  - destruct (import_wire_slot_lookup slot edges) as [edge|] eqn:Lookup; try discriminate.
    destruct (import_wire_header edge <? 128) eqn:Kind; try discriminate.
    destruct (import_strip_exact_prefix (import_wire_prefix edge) tail) as [suffix|] eqn:Prefix;
      try discriminate.
    destruct (import_build_history_stack fuel read (import_wire_hash edge)
      (context ++ slot :: import_wire_prefix edge) suffix) as [child_frames|] eqn:Child;
      try discriminate.
    inversion Built; subst frames.
    apply import_prefix_strip_is_exact in Prefix. subst tail.
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
    rewrite <- Slot in *. apply import_history_stack_child.
    + exact Read.
    + exact Lookup.
    + now apply Nat.ltb_ge.
    + eapply IH. exact Child.
Qed.

Theorem import_stack_builder_is_complete : forall read key context path frames,
  import_history_stack read key context path frames -> forall fuel,
  length path < fuel -> import_build_history_stack fuel read key context path = Some frames.
Proof.
  intros read key context path frames Stack.
  induction Stack as [key context edges Read|
    key context edges edge suffix frames Read Lookup Kind Child IH]; intros fuel Bound;
    destruct fuel as [|fuel]; [simpl in Bound; lia| |simpl in Bound; lia|].
  - cbn [import_build_history_stack]. now rewrite Read.
  - cbn [import_build_history_stack]. rewrite Read, Lookup.
    assert (import_wire_header edge <? 128 = false) as Header by now apply Nat.ltb_ge.
    rewrite Header.
    assert (import_strip_exact_prefix (import_wire_prefix edge)
      (import_wire_prefix edge ++ suffix) = Some suffix) as Prefix.
    { apply import_prefix_strip_is_exact. reflexivity. }
    rewrite Prefix, IH; [reflexivity|].
    cbn [length] in Bound. rewrite length_app in Bound. lia.
Qed.

Theorem import_stack_builder_characterization : forall read key context path frames,
  import_build_history_stack (S (length path)) read key context path = Some frames <->
  import_history_stack read key context path frames.
Proof.
  split; [apply import_stack_builder_preserves_exact_frames|].
  intros Stack. apply import_stack_builder_is_complete; auto.
Qed.

Theorem import_history_stack_is_nonempty : forall read key context path frames,
  import_history_stack read key context path frames -> frames <> [].
Proof.
  intros read key context path frames Stack. induction Stack.
  - discriminate.
  - intros Empty. apply app_eq_nil in Empty. destruct Empty as [_ Empty]. discriminate.
Qed.

Theorem import_history_stack_frames_have_exact_reads : forall read key context path frames,
  import_history_stack read key context path frames ->
  Forall (fun frame => read (import_frame_key frame) = Some (import_frame_edges frame)) frames.
Proof.
  intros read key context path frames Stack. induction Stack.
  - constructor; auto.
  - apply Forall_app. split; auto.
Qed.

Theorem import_history_stack_top_retains_absolute_position : forall read key context path frames,
  import_history_stack read key context path frames ->
  exists top rest,
    frames = top :: rest /\ import_frame_prefix top = context ++ path /\
    import_frame_last_slot top = None /\
    import_history_path read key path (import_frame_key top).
Proof.
  intros read key context path frames Stack.
  induction Stack as [key context edges Read|
    key context edges edge suffix frames Read Lookup Kind Child IH].
  - exists (import_frame_at key context edges None), [].
    split; [reflexivity|]. cbn [import_frame_prefix import_frame_last_slot import_frame_key import_frame_at].
    rewrite app_nil_r. split; [reflexivity|]. split; [reflexivity|].
    now apply import_history_path_here with (edges := edges).
  - destruct IH as [top [rest [Frames [Prefix [Last Path]]]]]. subst frames.
    exists top, (rest ++ [import_frame_at key context edges (Some (import_wire_slot edge))]).
    split; [reflexivity|]. split.
    + rewrite Prefix, <- app_assoc. reflexivity.
    + split; auto. eapply import_history_path_child; eauto.
Qed.

Theorem import_resolved_history_path_has_complete_stack : forall read key path target context,
  import_history_path read key path target ->
  exists frames, import_history_stack read key context path frames.
Proof.
  intros read key path target context Path. revert context.
  induction Path as [key edges Read|
    key edges edge suffix target Read Lookup Kind Child IH]; intros context.
  - exists [import_frame_at key context edges None]. now apply import_history_stack_here.
  - destruct (IH (context ++ import_wire_slot edge :: import_wire_prefix edge)) as [frames Stack].
    exists (frames ++ [import_frame_at key context edges (Some (import_wire_slot edge))]).
    now apply import_history_stack_child.
Qed.

Theorem import_stack_reconstruction_preserves_carrier : forall read key context path target frames,
  import_resolve_history_path (S (length path)) read key path = Some target ->
  import_build_history_stack (S (length path)) read key context path = Some frames ->
  exists top rest, frames = top :: rest /\ import_frame_key top = target.
Proof.
  intros read key context path target frames Resolved Built.
  apply import_path_resolution_characterization in Resolved.
  apply import_stack_builder_characterization in Built.
  apply import_history_stack_top_retains_absolute_position in Built
    as [top [rest [Frames [_ [_ Path]]]]].
  exists top, rest. split; auto. eapply import_history_path_is_deterministic; eauto.
Qed.

Theorem import_history_stack_reconstruction_is_deterministic : forall read key context path first second,
  import_history_stack read key context path first ->
  import_history_stack read key context path second -> first = second.
Proof.
  intros read key context path first second First Second.
  apply import_stack_builder_characterization in First.
  apply import_stack_builder_characterization in Second. congruence.
Qed.

Theorem import_history_stack_survives_compatible_writes : forall first last key context path frames,
  import_history_reader_extension first last ->
  import_history_stack first key context path frames ->
  import_history_stack last key context path frames.
Proof.
  intros first last key context path frames Extension Stack.
  induction Stack as [key context edges Read|
    key context edges edge suffix frames Read Lookup Kind Child IH].
  - apply import_history_stack_here. exact (Extension key edges Read).
  - apply import_history_stack_child.
    + exact (Extension key edges Read).
    + exact Lookup.
    + exact Kind.
    + exact IH.
Qed.

Inductive import_interleaved_history_stack : ImportHistoryReader -> ImportHistoryReader ->
    list nat -> list nat -> list nat -> list ImportTraversalFrame -> Prop :=
| import_interleaved_stack_here : forall read key context edges,
    read key = Some edges ->
    import_interleaved_history_stack read read key context [] [import_frame_at key context edges None]
| import_interleaved_stack_child : forall first middle last key context edges edge suffix frames,
    first key = Some edges ->
    import_wire_slot_lookup (import_wire_slot edge) edges = Some edge ->
    128 <= import_wire_header edge ->
    import_history_reader_extension first middle ->
    import_interleaved_history_stack middle last (import_wire_hash edge)
      (context ++ import_wire_slot edge :: import_wire_prefix edge) suffix frames ->
    import_interleaved_history_stack first last key context
      (import_wire_slot edge :: import_wire_prefix edge ++ suffix)
      (frames ++ [import_frame_at key context edges (Some (import_wire_slot edge))]).

Theorem import_interleaved_stack_preserves_reader_bindings : forall first last key context path frames,
  import_interleaved_history_stack first last key context path frames ->
  import_history_reader_extension first last.
Proof.
  intros first last key context path frames Run. induction Run.
  - intros query stored Read. exact Read.
  - eapply import_history_reader_extension_is_transitive; eauto.
Qed.

Theorem import_interleaved_stack_has_final_view_witness : forall first last key context path frames,
  import_interleaved_history_stack first last key context path frames ->
  import_history_stack last key context path frames.
Proof.
  intros first last key context path frames Run.
  induction Run as [read key context edges Read|
    first middle last key context edges edge suffix frames Read Lookup Kind Extension Run IH].
  - now apply import_history_stack_here.
  - pose proof (import_interleaved_stack_preserves_reader_bindings _ _ _ _ _ _ Run) as Later.
    apply import_history_stack_child.
    + exact (Later key edges (Extension key edges Read)).
    + exact Lookup.
    + exact Kind.
    + exact IH.
Qed.

Theorem import_interleaved_stack_matches_initial_complete_stack :
  forall initial final key context path expected actual,
  import_history_stack initial key context path expected ->
  import_interleaved_history_stack initial final key context path actual -> actual = expected.
Proof.
  intros initial final key context path expected actual Selected Run.
  pose proof (import_interleaved_stack_preserves_reader_bindings _ _ _ _ _ _ Run) as Extension.
  pose proof (import_history_stack_survives_compatible_writes _ _ _ _ _ _ Extension Selected) as Preserved.
  pose proof (import_interleaved_stack_has_final_view_witness _ _ _ _ _ _ Run) as Actual.
  eapply import_history_stack_reconstruction_is_deterministic; eauto.
Qed.

Theorem import_reconstructed_stack_survives_writes_after_final_read :
  forall initial final later key context path frames,
  import_interleaved_history_stack initial final key context path frames ->
  import_history_reader_extension final later -> import_history_stack later key context path frames.
Proof.
  intros initial final later key context path frames Run Extension.
  eapply import_history_stack_survives_compatible_writes; [exact Extension|].
  now apply import_interleaved_stack_has_final_view_witness in Run.
Qed.

Definition import_frame_pending_slots (frame : ImportTraversalFrame) : list nat :=
  let start := match import_frame_last_slot frame with None => 0 | Some slot => S slot end in
  seq start (256 - start).

Theorem import_pending_slot_membership_is_exact : forall frame slot,
  In slot (import_frame_pending_slots frame) <->
  slot < 256 /\ match import_frame_last_slot frame with
    None => True | Some previous => previous < slot end.
Proof.
  intros frame slot. unfold import_frame_pending_slots.
  destruct (import_frame_last_slot frame) as [previous|]; rewrite in_seq; lia.
Qed.

Theorem import_new_frame_includes_every_slot : forall key prefix edges slot,
  In slot (import_frame_pending_slots (import_frame_at key prefix edges None)) <-> slot < 256.
Proof. intros. rewrite import_pending_slot_membership_is_exact. simpl. tauto. Qed.

Theorem import_parent_frame_retains_exactly_later_siblings : forall key prefix edges previous slot,
  In slot (import_frame_pending_slots (import_frame_at key prefix edges (Some previous))) <->
  previous < slot /\ slot < 256.
Proof. intros. rewrite import_pending_slot_membership_is_exact. simpl. tauto. Qed.

Theorem import_last_slot_has_no_remaining_siblings : forall key prefix edges,
  import_frame_pending_slots (import_frame_at key prefix edges (Some 255)) = [].
Proof. reflexivity. Qed.

Theorem import_pending_slots_have_no_duplicates : forall frame,
  NoDup (import_frame_pending_slots frame).
Proof. intros frame. apply seq_NoDup. Qed.

Theorem import_pending_slots_are_strictly_increasing : forall frame first_index second_index first second,
  nth_error (import_frame_pending_slots frame) first_index = Some first ->
  nth_error (import_frame_pending_slots frame) second_index = Some second ->
  first_index < second_index -> first < second.
Proof.
  intros frame first_index second_index first second First Second Order.
  unfold import_frame_pending_slots in First, Second.
  rewrite nth_error_seq in First, Second.
  destruct (first_index <? _) eqn:FirstBound; try discriminate.
  destruct (second_index <? _) eqn:SecondBound; try discriminate.
  inversion First; inversion Second; subst. lia.
Qed.

Definition import_frame_remaining_edges (frame : ImportTraversalFrame) : list (option ImportWireEdge) :=
  map (fun slot => import_wire_slot_lookup slot (import_frame_edges frame))
    (import_frame_pending_slots frame).

Theorem import_remaining_edge_has_unvisited_slot : forall frame edge,
  In (Some edge) (import_frame_remaining_edges frame) ->
  In edge (import_frame_edges frame) /\
  In (import_wire_slot edge) (import_frame_pending_slots frame).
Proof.
  intros frame edge Present. unfold import_frame_remaining_edges in Present.
  apply in_map_iff in Present as [slot [Lookup Pending]].
  apply import_wire_slot_lookup_is_sound in Lookup as [Member Same]. split; auto.
  now rewrite Same.
Qed.

Theorem import_every_unvisited_edge_is_retained : forall frame edge,
  NoDup (map import_wire_slot (import_frame_edges frame)) ->
  In edge (import_frame_edges frame) ->
  In (import_wire_slot edge) (import_frame_pending_slots frame) ->
  In (Some edge) (import_frame_remaining_edges frame).
Proof.
  intros frame edge Unique Present Pending. unfold import_frame_remaining_edges.
  apply in_map_iff. exists (import_wire_slot edge). split; auto.
  now apply import_wire_slot_lookup_is_complete.
Qed.

Record ImportNextEntry : Type := {
  import_next_slot : nat;
  import_next_edge : ImportWireEdge;
  import_next_slots : list nat
}.

Fixpoint import_first_occupied_slot (slots : list nat) (edges : list ImportWireEdge)
    : option ImportNextEntry :=
  match slots with
  | [] => None
  | slot :: rest => match import_wire_slot_lookup slot edges with
    | Some edge => Some {| import_next_slot := slot; import_next_edge := edge; import_next_slots := rest |}
    | None => import_first_occupied_slot rest edges
    end
  end.

Theorem import_next_entry_preserves_all_skipped_slots : forall slots edges selected,
  import_first_occupied_slot slots edges = Some selected ->
  exists skipped,
    slots = skipped ++ import_next_slot selected :: import_next_slots selected /\
    Forall (fun slot => import_wire_slot_lookup slot edges = None) skipped /\
    import_wire_slot_lookup (import_next_slot selected) edges = Some (import_next_edge selected).
Proof.
  induction slots as [|slot rest IH]; intros edges selected Found; [discriminate|].
  cbn [import_first_occupied_slot] in Found.
  destruct (import_wire_slot_lookup slot edges) as [edge|] eqn:Lookup.
  - inversion Found; subst selected. exists []. split; [reflexivity|]. split; auto.
  - destruct (IH _ _ Found) as [skipped [Partition [Empty Selected]]].
    exists (slot :: skipped). split.
    + simpl. now rewrite Partition.
    + split; auto.
Qed.

Theorem import_no_next_entry_means_all_slots_are_empty : forall slots edges,
  import_first_occupied_slot slots edges = None <->
  Forall (fun slot => import_wire_slot_lookup slot edges = None) slots.
Proof.
  induction slots as [|slot rest IH]; intros edges; cbn [import_first_occupied_slot].
  - split; intros; constructor.
  - destruct (import_wire_slot_lookup slot edges) as [edge|] eqn:Lookup.
    + split; [discriminate|]. intros Empty. inversion Empty. congruence.
    + rewrite IH. split.
      * intros Empty. constructor; auto.
      * intros Empty. now inversion Empty.
Qed.

Theorem import_next_entry_of_sequence_retains_exact_suffix : forall count start edges selected,
  import_first_occupied_slot (seq start count) edges = Some selected ->
  import_next_slots selected = seq (S (import_next_slot selected))
    (start + count - S (import_next_slot selected)).
Proof.
  induction count as [|count IH]; intros start edges selected Found; [discriminate|].
  cbn [seq import_first_occupied_slot] in Found.
  destruct (import_wire_slot_lookup start edges) as [edge|] eqn:Lookup.
  - inversion Found; subst selected. cbn [import_next_slots import_next_slot].
    replace (start + S count - S start) with count by lia. reflexivity.
  - specialize (IH (S start) edges selected Found).
    replace (start + S count) with (S start + count) by lia. exact IH.
Qed.

Theorem import_next_entry_resumes_at_exact_parent_successor : forall frame selected,
  import_first_occupied_slot (import_frame_pending_slots frame) (import_frame_edges frame) = Some selected ->
  import_next_slots selected = import_frame_pending_slots
    (import_frame_at (import_frame_key frame) (import_frame_prefix frame)
      (import_frame_edges frame) (Some (import_next_slot selected))).
Proof.
  intros frame selected Found.
  pose proof (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
    as [skipped [Partition _]].
  assert (In (import_next_slot selected) (import_frame_pending_slots frame)) as Member.
  { rewrite Partition. apply in_or_app. right. now left. }
  apply import_pending_slot_membership_is_exact in Member as [Bound Previous].
  unfold import_frame_pending_slots in Found.
  pose proof (import_next_entry_of_sequence_retains_exact_suffix _ _ _ _ Found) as Suffix.
  unfold import_frame_pending_slots. cbn [import_frame_last_slot import_frame_at].
  rewrite Suffix. destruct (import_frame_last_slot frame) as [last|]; simpl in Previous.
  - replace (S last + (256 - S last)) with 256 by lia. reflexivity.
  - reflexivity.
Qed.

Example import_same_history_hash_at_different_paths_retains_both_frames :
  import_frame_prefix (import_frame_at [7] [1] [] None) <>
    import_frame_prefix (import_frame_at [7] [2] [] None).
Proof. discriminate. Qed.

Example import_resume_does_not_repeat_selected_ancestor_slot :
  ~In 17 (import_frame_pending_slots (import_frame_at [7] [] [] (Some 17))) /\
  In 18 (import_frame_pending_slots (import_frame_at [7] [] [] (Some 17))).
Proof.
  rewrite !import_pending_slot_membership_is_exact. cbn [import_frame_last_slot import_frame_at]. lia.
Qed.
