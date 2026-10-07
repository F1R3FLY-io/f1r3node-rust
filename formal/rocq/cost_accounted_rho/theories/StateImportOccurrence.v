From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure StateImportCodec StateImportCursor
  StateImportTraversal StateImportStack StateImportPage StateImportExport StateImportExecution StateImportWire
  StateImportCold.
Import ListNotations.

Theorem import_history_paths_compose : forall read root prefix middle,
  import_history_path read root prefix middle -> forall suffix target,
  import_history_path read middle suffix target ->
  import_history_path read root (prefix ++ suffix) target.
Proof.
  intros read root prefix middle Path.
  induction Path as [key edges Read|key edges edge suffix target Read Lookup Kind Child IH];
    intros tail final Tail.
  - exact Tail.
  - cbn [app]. rewrite <- app_assoc.
    eapply import_history_path_child; eauto.
Qed.

Theorem import_frame_spine_authenticates_all_frames : forall read root frames,
  import_frame_spine read root frames ->
  Forall (fun frame =>
    read (import_frame_key frame) = Some (import_frame_edges frame) /\
    import_history_path read root (import_frame_prefix frame) (import_frame_key frame)) frames.
Proof.
  intros read root frames Spine. induction Spine as
    [|edges last Read|parent rest edge edges last Spine IH Last Lookup Kind Read].
  - constructor.
  - constructor; [split; [exact Read|now apply import_history_path_here with (edges := edges)]|constructor].
  - constructor; [|exact IH]. cbn [import_frame_at import_frame_key import_frame_edges import_frame_prefix].
    split; [exact Read|]. inversion IH as [|frame frames [ParentRead ParentPath] Rest]; subst.
    eapply import_history_paths_compose; [exact ParentPath|].
    replace (import_wire_slot edge :: import_wire_prefix edge) with
      (import_wire_slot edge :: import_wire_prefix edge ++ []) by now rewrite app_nil_r.
    eapply import_history_path_child; eauto.
    now apply import_history_path_here with (edges := edges).
Qed.

Record ImportLocatedEntry : Type := {
  import_located_entry : ImportExportEntry;
  import_located_path : list nat
}.

Definition import_located_reference (entry : ImportLocatedEntry) : option ImportReference :=
  match import_located_entry entry with
  | ImportExportHistory key _ => Some (ImportHistoryRef key (import_located_path entry))
  | ImportExportLeaf key => match import_kind_at_path (import_located_path entry) with
    | Some kind => Some (ImportColdRef key kind)
    | None => None
    end
  end.

Definition import_step_occurrence_path (origin : list nat) (frames : list ImportTraversalFrame) :=
  match frames with
  | [] => origin
  | frame :: _ => match import_frame_next frame with
    | None => origin ++ import_frame_prefix frame
    | Some selected => origin ++ import_edge_absolute_prefix (import_frame_prefix frame)
        (import_next_slot selected) (import_next_edge selected)
    end
  end.

Definition import_locate_step_event (origin : list nat) (frames : list ImportTraversalFrame)
    (event : option ImportExportEntry) : list ImportLocatedEntry :=
  match event with
  | None => []
  | Some entry => [{| import_located_entry := entry;
      import_located_path := import_step_occurrence_path origin frames |}]
  end.

Theorem import_step_annotations_erase_exactly : forall origin frames event,
  map import_located_entry (import_locate_step_event origin frames event) = import_event_entries event.
Proof. intros origin frames [entry|]; reflexivity. Qed.

Theorem import_history_step_authenticates_next_singleton_origin :
  forall read original origin wire_root initial key prefix final,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root initial ->
  import_export_step read initial (Some (ImportExportHistory key prefix)) final ->
  exists next_root, key = import_key_to_nat next_root /\
    import_history_path read original (origin ++ prefix) next_root.
Proof.
  intros read original origin wire_root initial key prefix final Origin Spine Step.
  pose proof (import_frame_spine_authenticates_all_frames _ _ _ Spine) as Frames.
  inversion Step as [| |frame rest selected edges Found History Read]; subst.
  inversion Frames as [|top tail [ParentRead ParentPath] Rest]; subst.
  destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
    as [skipped [_ [_ Lookup]]].
  pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
  exists (import_wire_hash (import_next_edge selected)). split; [reflexivity|].
  unfold import_edge_absolute_prefix. rewrite <- Slot, app_assoc.
  eapply import_history_paths_compose.
  - eapply import_history_paths_compose; eauto.
  - replace (import_wire_slot (import_next_edge selected) :: import_wire_prefix (import_next_edge selected))
      with (import_wire_slot (import_next_edge selected) :: import_wire_prefix (import_next_edge selected) ++ [])
      by now rewrite app_nil_r.
    eapply import_history_path_child.
    + exact ParentRead.
    + now rewrite Slot.
    + now apply Nat.ltb_ge.
    + now apply import_history_path_here with (edges := edges).
Qed.

Definition import_occurrence_shape_matches (entry : ImportExportEntry) (edge : ImportWireEdge) : Prop :=
  match entry with
  | ImportExportHistory key _ =>
      key = import_key_to_nat (import_wire_hash edge) /\ 128 <= import_wire_header edge
  | ImportExportLeaf key =>
      key = import_key_to_nat (import_wire_hash edge) /\ import_wire_header edge < 128
  end.

Definition import_located_witness (read : ImportHistoryReader) (root : list nat)
    (located : ImportLocatedEntry) : Prop :=
  exists parent context edges edge,
    import_history_path read root context parent /\ read parent = Some edges /\
    import_wire_slot_lookup (import_wire_slot edge) edges = Some edge /\
    import_located_path located = context ++ import_wire_slot edge :: import_wire_prefix edge /\
    import_occurrence_shape_matches (import_located_entry located) edge /\
    import_located_reference located = import_edge_reference context (import_codec_edge_to_closure edge).

Theorem import_located_step_has_authenticated_occurrences :
  forall read original origin wire_root initial event final,
  import_history_path read original origin wire_root ->
  import_frame_spine read wire_root initial -> import_export_step read initial event final ->
  Forall (import_located_witness read original) (import_locate_step_event origin initial event).
Proof.
  intros read original origin wire_root initial event final Origin Spine Step.
  pose proof (import_frame_spine_authenticates_all_frames _ _ _ Spine) as Frames.
  destruct Step as [frame rest Empty|frame rest selected Found Leaf|
    frame rest selected child_edges Found History ChildRead].
  - constructor.
  - inversion Frames as [|top tail [Read Path] Rest]; subst.
    destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
      as [skipped [_ [_ Lookup]]].
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
    cbn [import_locate_step_event]. constructor; [|constructor].
    exists (import_frame_key frame), (origin ++ import_frame_prefix frame),
      (import_frame_edges frame), (import_next_edge selected).
    split; [eapply import_history_paths_compose; eauto|]. split; [exact Read|].
    split; [now rewrite Slot|].
    cbn [import_located_path import_step_occurrence_path]. rewrite Found.
    unfold import_edge_absolute_prefix. rewrite <- Slot.
    split; [now rewrite app_assoc|].
    split.
    + cbn [import_occurrence_shape_matches import_located_entry import_leaf_edge_entry].
      split; [reflexivity|now apply Nat.ltb_lt].
    + unfold import_located_reference, import_leaf_edge_entry.
      cbn [import_located_entry import_located_path].
      unfold import_edge_reference, import_codec_edge_to_closure. rewrite Leaf.
      cbn [import_edge_index import_edge_prefix import_edge_target]. now rewrite app_assoc.
  - inversion Frames as [|top tail [Read Path] Rest]; subst.
    destruct (import_next_entry_preserves_all_skipped_slots _ _ _ Found)
      as [skipped [_ [_ Lookup]]].
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
    cbn [import_locate_step_event]. constructor; [|constructor].
    exists (import_frame_key frame), (origin ++ import_frame_prefix frame),
      (import_frame_edges frame), (import_next_edge selected).
    split; [eapply import_history_paths_compose; eauto|]. split; [exact Read|].
    split; [now rewrite Slot|].
    cbn [import_located_path import_step_occurrence_path]. rewrite Found.
    unfold import_edge_absolute_prefix. rewrite <- Slot.
    split; [now rewrite app_assoc|].
    split.
    + cbn [import_occurrence_shape_matches import_located_entry import_history_edge_entry].
      split; [reflexivity|now apply Nat.ltb_ge].
    + unfold import_located_reference, import_history_edge_entry.
      cbn [import_located_entry import_located_path].
      unfold import_edge_reference, import_codec_edge_to_closure. rewrite History.
      cbn [import_edge_index import_edge_prefix import_edge_target]. now rewrite app_assoc.
Qed.

Inductive import_located_page (read : ImportHistoryReader) (origin : list nat) :
    nat -> list ImportTraversalFrame -> list ImportLocatedEntry -> list ImportTraversalFrame -> Prop :=
| import_located_page_limit : forall frames, import_located_page read origin 0 frames [] frames
| import_located_page_done : forall budget, import_located_page read origin budget [] [] []
| import_located_page_next : forall budget initial event next entries final,
    0 < budget -> import_export_step read initial event next ->
    import_located_page read origin (budget - import_event_history_cost event) next entries final ->
    import_located_page read origin budget initial
      (import_locate_step_event origin initial event ++ entries) final.

Theorem import_located_page_erases_without_changing_budget_or_stack :
  forall read origin budget initial entries final,
  import_located_page read origin budget initial entries final ->
  import_export_page read budget initial (map import_located_entry entries) final.
Proof.
  intros read origin budget initial entries final Page. induction Page.
  - constructor.
  - constructor.
  - rewrite map_app, import_step_annotations_erase_exactly.
    eapply import_export_page_next; eauto.
Qed.

Theorem import_every_page_admits_occurrence_annotations : forall read budget initial entries final,
  import_export_page read budget initial entries final -> forall origin,
  exists located, import_located_page read origin budget initial located final /\
    map import_located_entry located = entries.
Proof.
  intros read budget initial entries final Page. induction Page; intros origin.
  - exists []. split; [constructor|reflexivity].
  - exists []. split; [constructor|reflexivity].
  - destruct (IHPage origin) as [located [Located Erase]].
    exists (import_locate_step_event origin initial event ++ located).
    split; [eapply import_located_page_next; eauto|].
    now rewrite map_app, import_step_annotations_erase_exactly, Erase.
Qed.

Theorem import_located_page_authenticates_every_occurrence :
  forall read origin budget initial entries final,
  import_located_page read origin budget initial entries final -> forall original wire_root,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root initial ->
  Forall (import_located_witness read original) entries.
Proof.
  intros read origin budget initial entries final Page. induction Page;
    intros original wire_root Origin Spine; try constructor.
  apply Forall_app. split.
  - eapply import_located_step_has_authenticated_occurrences; eauto.
  - eapply IHPage; eauto using import_operational_step_preserves_frame_spine.
Qed.

Theorem import_located_page_survives_compatible_writes :
  forall first last origin budget initial entries final,
  import_history_reader_extension first last ->
  import_located_page first origin budget initial entries final ->
  import_located_page last origin budget initial entries final.
Proof.
  intros first last origin budget initial entries final Extension Page. induction Page.
  - constructor.
  - constructor.
  - eapply import_located_page_next; eauto using import_export_step_preserves_reader_extension.
Qed.

Theorem import_located_witness_survives_compatible_writes : forall first last root entry,
  import_history_reader_extension first last -> import_located_witness first root entry ->
  import_located_witness last root entry.
Proof.
  intros first last root entry Extension [parent [context [edges [edge [Path [Read Rest]]]]]].
  exists parent, context, edges, edge. split.
  - eapply import_history_path_survives_compatible_writes; eauto.
  - split; [now apply Extension|exact Rest].
Qed.

Theorem import_frame_spine_survives_compatible_writes : forall first last root frames,
  import_history_reader_extension first last -> import_frame_spine first root frames ->
  import_frame_spine last root frames.
Proof.
  intros first last root frames Extension Spine. induction Spine.
  - constructor.
  - constructor. now apply Extension.
  - eapply import_frame_spine_child; eauto.
Qed.

Theorem import_interleaved_page_retains_authenticated_occurrence_annotations :
  forall first last budget initial entries final original origin wire_root,
  import_interleaved_slice first last 0 budget initial entries final ->
  import_history_path first original origin wire_root -> import_frame_spine first wire_root initial ->
  exists located, import_located_page last origin budget initial located final /\
    map import_located_entry located = entries /\
    Forall (import_located_witness last original) located.
Proof.
  intros first last budget initial entries final original origin wire_root Run Origin Spine.
  pose proof (import_interleaved_slice_preserves_reader_bindings _ _ _ _ _ _ _ Run) as Extension.
  apply import_interleaved_slice_has_exact_final_view_page in Run.
  apply import_zero_skip_slice_is_a_page in Run.
  destruct (import_every_page_admits_occurrence_annotations _ _ _ _ _ Run origin)
    as [located [Page Erase]].
  exists located. split; [exact Page|]. split; [exact Erase|].
  eapply import_located_page_authenticates_every_occurrence; [exact Page| |].
  - eapply import_history_path_survives_compatible_writes; eauto.
  - eapply import_frame_spine_survives_compatible_writes; eauto.
Qed.

Definition import_complete_occurrence_witness (read : ImportHistoryReader) (original : list nat)
    (located : ImportLocatedEntry) : Prop :=
  match import_located_entry located with
  | ImportExportHistory key _ => exists raw_key,
      key = import_key_to_nat raw_key /\
      import_history_path read original (import_located_path located) raw_key
  | ImportExportLeaf _ => import_located_witness read original located
  end.

Theorem import_interleaved_cursor_export_preserves_starting_reads :
  forall first last input skip budget entries final,
  import_interleaved_cursor_export first last input skip budget entries final ->
  import_history_reader_extension first last.
Proof.
  intros first last input skip budget entries final
    [_ [_ [observed [middle [frames [Leading [Stack Export]]]]]]].
  eapply import_history_reader_extension_is_transitive; [exact Leading|].
  eapply import_history_reader_extension_is_transitive with (middle := middle).
  - destruct input as [root|root prefix carrier]; cbn [import_interleaved_cursor_stack] in Stack.
    + eapply import_interleaved_stack_preserves_reader_bindings; eauto.
    + destruct Stack as [Stack _]. eapply import_interleaved_stack_preserves_reader_bindings; eauto.
  - eapply import_interleaved_anchor_preserves_reader_bindings; eauto.
Qed.

Theorem import_history_step_annotation_uses_event_prefix :
  forall read initial key prefix final origin,
  import_export_step read initial (Some (ImportExportHistory key prefix)) final ->
  import_locate_step_event origin initial (Some (ImportExportHistory key prefix)) =
    [{| import_located_entry := ImportExportHistory key prefix; import_located_path := origin ++ prefix |}].
Proof.
  intros read initial key prefix final origin Step.
  inversion Step as [| |frame rest selected edges Found History Read]; subst.
  cbn [import_locate_step_event import_step_occurrence_path]. rewrite Found. reflexivity.
Qed.

Theorem import_located_step_has_complete_occurrences :
  forall read original origin wire_root initial event final,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root initial ->
  import_export_step read initial event final ->
  Forall (import_complete_occurrence_witness read original) (import_locate_step_event origin initial event).
Proof.
  intros read original origin wire_root initial [[key prefix|key]|] final Origin Spine Step.
  - rewrite (import_history_step_annotation_uses_event_prefix _ _ _ _ _ origin Step).
    constructor; [|constructor].
    eapply import_history_step_authenticates_next_singleton_origin; eauto.
  - pose proof (import_located_step_has_authenticated_occurrences _ _ _ _ _ _ _ Origin Spine Step) as Located.
    cbn [import_locate_step_event] in *. inversion Located; subst.
    constructor; [assumption|constructor].
  - constructor.
Qed.

Theorem import_located_page_has_complete_occurrences :
  forall read origin budget initial entries final,
  import_located_page read origin budget initial entries final -> forall original wire_root,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root initial ->
  Forall (import_complete_occurrence_witness read original) entries.
Proof.
  intros read origin budget initial entries final Page. induction Page;
    intros original wire_root Origin Spine; try constructor.
  apply Forall_app. split.
  - eapply import_located_step_has_complete_occurrences; eauto.
  - eapply IHPage; eauto using import_operational_step_preserves_frame_spine.
Qed.

Inductive import_located_slice (read : ImportHistoryReader) (origin : list nat) : nat -> nat ->
    list ImportTraversalFrame -> list ImportLocatedEntry -> list ImportTraversalFrame -> Prop :=
| import_located_slice_ready : forall budget initial entries final,
    import_located_page read origin budget initial entries final ->
    import_located_slice read origin 0 budget initial entries final
| import_located_slice_done : forall skip budget, import_located_slice read origin skip budget [] [] []
| import_located_slice_skip : forall skip budget initial event next entries final,
    0 < skip -> import_export_step read initial event next ->
    import_located_slice read origin (skip - import_event_history_cost event) budget next entries final ->
    import_located_slice read origin skip budget initial entries final.

Inductive import_located_anchored_export (read : ImportHistoryReader) (origin : list nat) :
    option (list nat) -> nat -> nat -> list ImportTraversalFrame ->
    list ImportLocatedEntry -> list ImportTraversalFrame -> Prop :=
| import_located_anchored_resume : forall skip budget initial entries final,
    import_located_slice read origin skip budget initial entries final ->
    import_located_anchored_export read origin None skip budget initial entries final
| import_located_anchored_skip_root : forall key skip budget initial entries final,
    import_located_slice read origin skip budget initial entries final ->
    import_located_anchored_export read origin (Some key) (S skip) budget initial entries final
| import_located_anchored_take_root : forall key budget initial entries final,
    import_located_page read origin budget initial entries final ->
    import_located_anchored_export read origin (Some key) 0 (S budget) initial
      ({| import_located_entry := ImportExportHistory (import_key_to_nat key) [];
          import_located_path := origin |} :: entries) final.

Definition import_located_cursor_export (read : ImportHistoryReader) (origin : list nat)
    (cursor : ImportCursor) (skip budget : nat) (entries : list ImportLocatedEntry)
    (final : list ImportTraversalFrame) : Prop :=
  import_cursor_validb cursor = true /\ (0 < skip \/ 0 < budget) /\
  exists frames, import_cursor_initial_stack read cursor frames /\
    import_located_anchored_export read origin (import_cursor_anchor cursor) skip budget frames entries final.

Theorem import_skipped_slice_has_complete_occurrence_annotations :
  forall read skip budget initial entries final,
  import_export_slice read skip budget initial entries final -> forall original origin wire_root,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root initial ->
  exists located, import_located_slice read origin skip budget initial located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness read original) located.
Proof.
  intros read skip budget initial entries final Slice. induction Slice;
    intros original origin wire_root Origin Spine.
  - destruct (import_every_page_admits_occurrence_annotations _ _ _ _ _ H origin)
      as [located [Located Erase]].
    exists located. split; [now constructor|]. split; [exact Erase|].
    eapply import_located_page_has_complete_occurrences; eauto.
  - exists []. split; [constructor|]. split; [reflexivity|constructor].
  - assert (import_frame_spine read wire_root next) as NextSpine
      by (eapply import_operational_step_preserves_frame_spine; eauto).
    destruct (IHSlice original origin wire_root Origin NextSpine) as [located [Located Rest]].
    exists located. split; [eapply import_located_slice_skip; eauto|exact Rest].
Qed.

Theorem import_cursor_export_has_complete_occurrence_annotations :
  forall read input skip budget entries final original origin,
  import_export_from_cursor read input skip budget entries final ->
  import_history_path read original origin (import_cursor_root input) ->
  exists located, import_located_cursor_export read origin input skip budget located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness read original) located.
Proof.
  intros read input skip budget entries final original origin [Valid [Positive [initial [Stack Export]]]] Origin.
  pose proof (import_cursor_stack_has_frame_spine _ _ _ Stack) as Spine.
  destruct input as [root|root prefix carrier]; cbn [import_cursor_anchor import_cursor_root] in *.
  - inversion Export as [|key remaining cap frames result tail Slice|key cap frames result tail Page]; subst.
    + destruct (import_skipped_slice_has_complete_occurrence_annotations _ _ _ _ _ _ Slice
        original origin root Origin Spine) as [located [Located Rest]].
      exists located. split; [|exact Rest]. split; [exact Valid|]. split; [exact Positive|].
      exists initial. split; [exact Stack|]. now apply import_located_anchored_skip_root.
    + destruct (import_every_page_admits_occurrence_annotations _ _ _ _ _ Page origin)
        as [located [Located Erase]].
      exists ({| import_located_entry := ImportExportHistory (import_key_to_nat root) [];
          import_located_path := origin |} :: located).
      split.
      { split; [exact Valid|]. split; [exact Positive|]. exists initial.
        split; [exact Stack|]. now apply import_located_anchored_take_root. }
      split; [cbn [map import_located_entry]; now rewrite Erase|].
      constructor.
      * exists root. auto.
      * eapply import_located_page_has_complete_occurrences; eauto.
  - inversion Export as [remaining cap frames result tail Slice| |]; subst.
    destruct (import_skipped_slice_has_complete_occurrence_annotations _ _ _ _ _ _ Slice
      original origin root Origin Spine) as [located [Located Rest]].
    exists located. split; [|exact Rest]. split; [exact Valid|]. split; [exact Positive|].
    exists initial. split; [exact Stack|]. now apply import_located_anchored_resume.
Qed.

Fixpoint import_last_located_history (entries : list ImportLocatedEntry) : option ImportLocatedEntry :=
  match entries with
  | [] => None
  | entry :: rest => match import_last_located_history rest with
    | Some last => Some last
    | None => match import_located_entry entry with
      | ImportExportHistory _ _ => Some entry
      | ImportExportLeaf _ => None
      end
    end
  end.

Theorem import_missing_last_occurrence_has_no_history : forall entries,
  import_last_located_history entries = None ->
  import_export_history_keys (map import_located_entry entries) = [].
Proof.
  induction entries as [|[[key prefix|key] path] rest IH]; intros Last; [reflexivity| |].
  - cbn [import_last_located_history import_located_entry] in Last.
    destruct (import_last_located_history rest); discriminate.
  - cbn [import_last_located_history import_located_entry] in Last.
    destruct (import_last_located_history rest) eqn:Tail; [discriminate|].
    cbn [map import_located_entry import_export_history_keys]. apply IH. reflexivity.
Qed.

Theorem import_last_occurrence_has_exact_position : forall entries located,
  import_last_located_history entries = Some located ->
  exists before after key prefix,
    entries = before ++ located :: after /\
    import_located_entry located = ImportExportHistory key prefix /\
    import_export_history_keys (map import_located_entry after) = [].
Proof.
  induction entries as [|head rest IH]; intros located Last; [discriminate|].
  cbn [import_last_located_history] in Last.
  destruct (import_last_located_history rest) as [last|] eqn:Tail.
  - inversion Last; subst last. destruct (IH located eq_refl)
      as [before [after [key [prefix [Decomposition [Entry NoHistory]]]]]].
    exists (head :: before), after, key, prefix. cbn [app]. rewrite Decomposition. auto.
  - destruct (import_located_entry head) as [key prefix|key] eqn:Entry; [|discriminate].
    inversion Last; subst located. exists [], rest, key, prefix.
    split; [reflexivity|]. split; [exact Entry|].
    now apply import_missing_last_occurrence_has_no_history.
Qed.

Theorem import_exhausted_cursor_keeps_exact_last_occurrence_origin :
  forall read input entries located original,
  Forall (import_complete_occurrence_witness read original) entries ->
  import_last_located_history entries = Some located ->
  (forall key edges, read key = Some edges -> import_cursor_word_validb key = true) ->
  exists raw_key,
    import_wire_next_cursor input (map import_located_entry entries) [] = ImportCursorStart raw_key /\
    import_history_path read original (import_located_path located) raw_key.
Proof.
  intros read input entries located original Complete Last Valid.
  destruct (import_last_occurrence_has_exact_position _ _ Last)
    as [before [after [key [prefix [Decomposition [Entry NoHistory]]]]]].
  assert (In located entries) as Present by (rewrite Decomposition; apply in_or_app; right; left; reflexivity).
  rewrite Forall_forall in Complete. specialize (Complete located Present).
  unfold import_complete_occurrence_witness in Complete. rewrite Entry in Complete.
  destruct Complete as [raw_key [Key Path]].
  destruct (import_resolved_history_path_has_readable_target _ _ _ _ Path) as [edges Read].
  exists raw_key. split; [|exact Path].
  rewrite Decomposition, map_app. cbn [map]. rewrite Entry, Key.
  apply import_exhausted_singleton_retains_exact_raw_history_key; [eapply Valid; exact Read|exact NoHistory].
Qed.

Inductive ImportOccurrenceCheck : Type :=
| ImportOccurrenceAccepted
| ImportOccurrenceUnknownKind (path : list nat)
| ImportOccurrenceMissing (key : nat)
| ImportOccurrenceStorageFailure (key : nat)
| ImportOccurrenceInvalidCold (key : nat) (kind : ImportLeafKind).

Section OccurrenceConsumption.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable Hash : list nat -> nat.

Definition import_check_cold_occurrence (read : nat -> ImportRootLookup) (entry : ImportLocatedEntry)
    : ImportOccurrenceCheck :=
  match import_located_reference entry with
  | None => ImportOccurrenceUnknownKind (import_located_path entry)
  | Some (ImportHistoryRef _ _) => ImportOccurrenceAccepted
  | Some (ImportColdRef key kind) => match read key with
    | ImportRootLookupAbsent => ImportOccurrenceMissing key
    | ImportRootLookupFailure => ImportOccurrenceStorageFailure key
    | ImportRootLookupFound bytes =>
        match import_authenticate_cold_bytes Value decode_item Hash kind key bytes with
        | Some _ => ImportOccurrenceAccepted
        | None => ImportOccurrenceInvalidCold key kind
        end
    end
  end.

Fixpoint import_check_cold_occurrences (read : nat -> ImportRootLookup) (entries : list ImportLocatedEntry)
    : ImportOccurrenceCheck :=
  match entries with
  | [] => ImportOccurrenceAccepted
  | entry :: rest => match import_check_cold_occurrence read entry with
    | ImportOccurrenceAccepted => import_check_cold_occurrences read rest
    | failure => failure
    end
  end.

Theorem import_occurrence_checks_cover_every_occurrence : forall read entries,
  import_check_cold_occurrences read entries = ImportOccurrenceAccepted <->
  Forall (fun entry => import_check_cold_occurrence read entry = ImportOccurrenceAccepted) entries.
Proof.
  intros read entries. induction entries as [|entry rest IH].
  - split; [constructor|reflexivity].
  - cbn [import_check_cold_occurrences]. rewrite Forall_cons_iff.
    destruct (import_check_cold_occurrence read entry); try (split; intros Bad; [discriminate|destruct Bad; discriminate]).
    rewrite IH. tauto.
Qed.

Theorem import_cold_occurrence_success_is_exact : forall read entry key kind,
  import_located_reference entry = Some (ImportColdRef key kind) ->
  (import_check_cold_occurrence read entry = ImportOccurrenceAccepted <->
  exists bytes leaf, read key = ImportRootLookupFound bytes /\
    import_authenticate_cold_bytes Value decode_item Hash kind key bytes = Some leaf).
Proof.
  intros read entry key kind Reference. unfold import_check_cold_occurrence. rewrite Reference.
  destruct (read key) as [|bytes|] eqn:Read.
  - split; [discriminate|intros [bytes [leaf [Impossible _]]]; discriminate].
  - destruct (import_authenticate_cold_bytes Value decode_item Hash kind key bytes) as [leaf|] eqn:Checked.
    + split; [intros _; eauto|intros _; reflexivity].
    + split; [discriminate|]. intros [other [leaf [Same CheckedOther]]].
      inversion Same; subst other. congruence.
  - split; [discriminate|intros [bytes [leaf [Impossible _]]]; discriminate].
Qed.

Theorem import_accepted_occurrence_has_consumable_payload : forall read entries entry key kind,
  import_check_cold_occurrences read entries = ImportOccurrenceAccepted -> In entry entries ->
  import_located_reference entry = Some (ImportColdRef key kind) ->
  exists bytes leaf values,
    read key = ImportRootLookupFound bytes /\ import_read_persisted_leaf bytes = Some leaf /\
    Hash (import_leaf_hash_input leaf) = key /\ import_leaf_kind leaf = kind /\
    import_consume_cold_leaf Value decode_item kind leaf = Some values.
Proof.
  intros read entries entry key kind Checked Present Reference.
  apply import_occurrence_checks_cover_every_occurrence in Checked.
  rewrite Forall_forall in Checked. specialize (Checked entry Present).
  apply (import_cold_occurrence_success_is_exact _ _ _ _ Reference) in Checked
    as [bytes [leaf [Read Authenticated]]].
  apply import_authenticated_cold_bytes_have_hash_kind_and_typed_values in Authenticated
    as [Parsed [Hashed [Kind [values [items [rest [Consumed _]]]]]]].
  exists bytes, leaf, values. auto.
Qed.

Theorem import_one_payload_cannot_satisfy_conflicting_occurrence_kinds :
  forall read entries first second key left right,
  In first entries -> In second entries ->
  import_located_reference first = Some (ImportColdRef key left) ->
  import_located_reference second = Some (ImportColdRef key right) -> left <> right ->
  import_check_cold_occurrences read entries <> ImportOccurrenceAccepted.
Proof.
  intros read entries first second key left right First Second Left Right Different Checked.
  destruct (import_accepted_occurrence_has_consumable_payload _ _ _ _ _ Checked First Left)
    as [bytes [leaf [values [Read [Parsed [_ [Kind _]]]]]]].
  destruct (import_accepted_occurrence_has_consumable_payload _ _ _ _ _ Checked Second Right)
    as [other_bytes [other_leaf [other_values [OtherRead [OtherParsed [_ [OtherKind _]]]]]]].
  assert (bytes = other_bytes) by congruence. subst other_bytes.
  assert (leaf = other_leaf) by congruence. subst other_leaf. congruence.
Qed.

Theorem import_missing_or_failed_occurrence_cannot_be_accepted : forall read entries entry key kind,
  In entry entries -> import_located_reference entry = Some (ImportColdRef key kind) ->
  (read key = ImportRootLookupAbsent \/ read key = ImportRootLookupFailure) ->
  import_check_cold_occurrences read entries <> ImportOccurrenceAccepted.
Proof.
  intros read entries entry key kind Present Reference Missing Checked.
  destruct (import_accepted_occurrence_has_consumable_payload _ _ _ _ _ Checked Present Reference)
    as [bytes [leaf [values [Read _]]]]. destruct Missing; congruence.
Qed.

End OccurrenceConsumption.

Example import_singleton_leaf_kind_requires_original_context :
  let leaf := ImportExportLeaf 7 in
  import_located_reference {| import_located_entry := leaf; import_located_path := [2; 9] |} =
    Some (ImportColdRef 7 ImportJoins) /\
  import_located_reference {| import_located_entry := leaf; import_located_path := [9] |} = None.
Proof. split; reflexivity. Qed.

Example import_same_key_occurrences_must_keep_distinct_kinds :
  let first := {| import_located_entry := ImportExportLeaf 7; import_located_path := [0; 9] |} in
  let second := {| import_located_entry := ImportExportLeaf 7; import_located_path := [2; 9] |} in
  map import_located_entry [first; second] = [ImportExportLeaf 7; ImportExportLeaf 7] /\
  map import_located_reference [first; second] =
    [Some (ImportColdRef 7 ImportData); Some (ImportColdRef 7 ImportJoins)].
Proof. split; reflexivity. Qed.
