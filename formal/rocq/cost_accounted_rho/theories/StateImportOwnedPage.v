From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure StateImportCodec StateImportCursor StateImportTraversal
  StateImportStack StateImportPage StateImportExport StateImportExecution StateImportWire StateImportOccurrence
  StateImportOverlay StateImportHistoryObservations StateImportReadTape
  StateImportStorage StateImportCold StateImportEncodedCold StateImportReceivedCold StateImportObservations.
Import ListNotations.

Record ImportOwnedCursor : Type := {
  import_owned_original_root : list nat;
  import_owned_origin : list nat;
  import_owned_cursor : ImportCursor
}.

Definition import_tape_check_original_root (owner : ImportOwnedCursor) (tape : list ImportHistoryLogicalRead)
    : ImportTapeResult unit :=
  if import_cursor_word_validb (import_owned_original_root owner) && import_cursor_validb (import_owned_cursor owner) then
    import_tape_bind
      (import_tape_build_stack (S (length (import_owned_origin owner)))
        (import_owned_original_root owner) [] (import_owned_origin owner) tape)
      (fun frames remaining => match frames with
        | [] => ImportTapeFail ImportTapeInvalidStack
        | top :: _ =>
            if list_eq_dec Nat.eq_dec (import_frame_key top) (import_cursor_root (import_owned_cursor owner))
            then ImportTapeReturn tt remaining else ImportTapeFail ImportTapeInvalidCursor
        end)
  else ImportTapeFail ImportTapeInvalidCursor.

Theorem import_consumed_origin_authenticates_the_wire_root :
  forall read owner tape remaining,
  import_tape_reader_agrees read tape ->
  import_tape_check_original_root owner tape = ImportTapeReturn tt remaining ->
  import_history_path read (import_owned_original_root owner) (import_owned_origin owner)
    (import_cursor_root (import_owned_cursor owner)) /\ import_tape_suffix tape remaining.
Proof.
  intros read owner tape remaining Agree Accepted.
  unfold import_tape_check_original_root in Accepted.
  destruct (import_cursor_word_validb (import_owned_original_root owner) && import_cursor_validb (import_owned_cursor owner));
    [|discriminate].
  destruct (import_tape_build_stack (S (length (import_owned_origin owner)))
    (import_owned_original_root owner) [] (import_owned_origin owner) tape)
    as [frames rest|error] eqn:Built; cbn [import_tape_bind] in Accepted; [|discriminate].
  destruct frames as [|top ancestors]; [discriminate|].
  destruct (list_eq_dec Nat.eq_dec (import_frame_key top) (import_cursor_root (import_owned_cursor owner)))
    as [Key|Different]; [|discriminate]. inversion Accepted; subst remaining.
  destruct (import_tape_stack_refines_exact_stack_evaluation _ _ _ _ _ _ _ _ Agree Built)
    as [Stack Suffix]. split; [|exact Suffix].
  apply import_stack_builder_preserves_exact_frames in Stack.
  destruct (import_history_stack_top_retains_absolute_position _ _ _ _ _ Stack)
    as [selected [tail [Frames [_ [_ Path]]]]].
  inversion Frames; subst selected tail. now rewrite Key in Path.
Qed.

Definition import_tape_evaluate_owned_cursor (owner : ImportOwnedCursor) (skip budget : nat)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult ImportTapeSlice :=
  import_tape_bind (import_tape_check_original_root owner tape)
    (fun _ remaining => import_tape_evaluate_cursor (import_owned_cursor owner) skip budget remaining).

Theorem import_owned_cursor_keeps_origin_and_page_phases_separate :
  forall read owner skip budget tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_owned_cursor owner skip budget tape = ImportTapeReturn (entries, final) remaining ->
  exists page_tape,
    import_tape_check_original_root owner tape = ImportTapeReturn tt page_tape /\
    import_tape_evaluate_cursor (import_owned_cursor owner) skip budget page_tape =
      ImportTapeReturn (entries, final) remaining /\
    import_history_path read (import_owned_original_root owner) (import_owned_origin owner)
      (import_cursor_root (import_owned_cursor owner)) /\
    import_export_from_cursor read (import_owned_cursor owner) skip budget entries final /\
    import_tape_suffix tape page_tape /\ import_tape_suffix page_tape remaining.
Proof.
  intros read owner skip budget tape entries final remaining Agree Accepted.
  unfold import_tape_evaluate_owned_cursor in Accepted.
  destruct (import_tape_check_original_root owner tape) as [[] page_tape|error] eqn:Origin;
    cbn [import_tape_bind] in Accepted; [|discriminate].
  destruct (import_consumed_origin_authenticates_the_wire_root _ _ _ _ Agree Origin) as [Path Prefix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Prefix) as PageAgree.
  destruct (import_tape_cursor_refines_operational_export _ _ _ _ _ _ _ _ PageAgree Accepted)
    as [Export Suffix].
  exists page_tape. split; [reflexivity|]. split; [exact Accepted|].
  split; [exact Path|]. split; [exact Export|]. auto.
Qed.

Definition import_tape_checked_owned_attempt (Hash : list nat -> list nat)
    (rows : list ImportReceivedHistoryRow) (owner : ImportOwnedCursor) (skip budget : nat)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult ImportTapeSlice :=
  import_tape_prepared_complete_attempt Hash rows (import_tape_evaluate_owned_cursor owner skip budget) tape.

Theorem import_owned_attempt_requires_preparation_before_origin_reads :
  forall Hash rows initial records logical current observations owner skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_attempt Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  exists first rest candidate prefix suffix before,
    logical = first :: rest /\ records = prefix ++ suffix /\
    import_history_observation_trace Hash rows initial prefix [] before (import_history_logical_cache first) /\
    import_history_store_extension before current /\
    import_prepare_history_overlay Hash (import_project_history_observations (import_history_logical_cache first)) rows =
      ImportHistoryOverlayReady candidate.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final Trace Accepted.
  destruct (import_prepared_complete_attempt_consumes_every_record _ _ _ _ _ _ _ Accepted)
    as [_ [_ [first [rest [candidate [Same Prepared]]]]]].
  destruct (import_history_first_callback_has_an_actual_preparation_prefix _ _ _ _ _ _ _ Trace _ _ Same)
    as [prefix [suffix [before [Records [Prefix Extension]]]]].
  exists first, rest, candidate, prefix, suffix, before. auto.
Qed.

Theorem import_owned_attempt_preserves_original_context_after_history_commit :
  forall Hash rows initial records logical current observations owner skip budget entries final storage_ok result,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_attempt Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_history_path (import_checked_wire_history Hash result)
    (import_owned_original_root owner) (import_owned_origin owner) (import_cursor_root (import_owned_cursor owner)) /\
  import_export_from_cursor (import_checked_wire_history Hash result) (import_owned_cursor owner)
    skip budget entries final /\
  Forall import_tape_successful_record logical /\ length (import_export_history_keys entries) <= budget.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final storage_ok result
    Trace Accepted Commit.
  destruct (import_prepared_complete_attempt_consumes_every_record _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  pose proof (import_actual_history_commit_derives_tape_reader_agreement _ _ _ _ _ _ _ _ _ Trace Commit) as Agree.
  destruct (import_owned_cursor_keeps_origin_and_page_phases_separate _ _ _ _ _ _ _ _ Agree Evaluated)
    as [page_tape [_ [_ [Path [Export [OriginSuffix PageSuffix]]]]]].
  split; [exact Path|]. split; [exact Export|]. split.
  - destruct (import_tape_suffix_trans _ _ _ OriginSuffix PageSuffix) as [prefix [Same Success]].
    rewrite app_nil_r in Same. now subst logical.
  - eapply import_cursor_export_respects_history_budget; eauto.
Qed.

Theorem import_owned_committed_page_has_exact_root_specific_occurrences :
  forall Hash rows initial records logical current observations owner skip budget entries final storage_ok result,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_attempt Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  exists located,
    import_located_cursor_export (import_checked_wire_history Hash result) (import_owned_origin owner)
      (import_owned_cursor owner) skip budget located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness (import_checked_wire_history Hash result)
      (import_owned_original_root owner)) located.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final storage_ok result
    Trace Accepted Commit.
  destruct (import_owned_attempt_preserves_original_context_after_history_commit
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ Trace Accepted Commit) as [Path [Export _]].
  eapply import_cursor_export_has_complete_occurrence_annotations; eauto.
Qed.

Theorem import_owned_committed_reply_preserves_metadata_and_singleton_origin :
  forall Hash rows initial records logical current observations owner skip budget entries final storage_ok result
    computed request start next cold_keys,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_attempt Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_cursor_encode (import_owned_cursor owner) = Some request ->
  import_adapt_wire_result (import_owned_cursor owner) (ImportEvaluationSuccess entries final) = ImportWireSuccess computed ->
  import_check_reply_metadata request start next (map (fun row => import_key_to_nat (fst row)) rows) cold_keys computed = true ->
  exists located,
    import_located_cursor_export (import_checked_wire_history Hash result) (import_owned_origin owner)
      (import_owned_cursor owner) skip budget located final /\
    map import_located_entry located = entries /\
    Forall (import_complete_occurrence_witness (import_checked_wire_history Hash result)
      (import_owned_original_root owner)) located /\
    start = request /\
    import_cursor_encode (import_wire_next_cursor (import_owned_cursor owner) entries final) = Some next /\
    (forall key, In key (map (fun row => import_key_to_nat (fst row)) rows) <-> In key (import_export_history_keys entries)) /\
    (forall key, In key cold_keys <-> In key (import_export_leaf_keys entries)) /\
    (forall selected, final = [] -> import_last_located_history located = Some selected ->
      exists raw_key, import_cursor_encode (ImportCursorStart raw_key) = Some next /\
        import_history_path (import_checked_wire_history Hash result) (import_owned_original_root owner)
          (import_located_path selected) raw_key).
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final storage_ok result
    computed request start next cold_keys Trace Accepted Commit Request Adapted Metadata.
  destruct (import_owned_committed_page_has_exact_root_specific_occurrences
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ Trace Accepted Commit) as [located [Located [Erase Complete]]].
  destruct (import_accepted_reply_matches_checked_export _ _ _ _ _ _ _ _ Adapted Metadata)
    as [actual [frames [Same [Start [Next [History Cold]]]]]].
  inversion Same; subst actual frames.
  exists located. split; [exact Located|]. split; [exact Erase|]. split; [exact Complete|].
  split; [exact Start|]. split; [exact Next|]. split; [exact History|]. split; [exact Cold|].
  intros selected Empty Last. subst final.
  assert (forall key edges, import_checked_wire_history Hash result key = Some edges ->
    import_cursor_word_validb key = true) as Valid.
  { intros key edges Read. apply import_checked_wire_history_has_exact_witness in Read as [Width [Domain _]].
    apply import_cursor_word_validity_characterization. auto. }
  destruct (import_exhausted_cursor_keeps_exact_last_occurrence_origin
    _ (import_owned_cursor owner) _ _ _ Complete Last Valid) as [raw_key [Cursor Path]].
  exists raw_key. split; [|exact Path]. rewrite <- Cursor, Erase. exact Next.
Qed.

Definition import_advance_owned_cursor (owner : ImportOwnedCursor) (entries : list ImportLocatedEntry)
    (final : list ImportTraversalFrame) : ImportOwnedCursor :=
  {| import_owned_original_root := import_owned_original_root owner;
     import_owned_origin := match final with
       | _ :: _ => import_owned_origin owner
       | [] => match import_last_located_history entries with
         | None => import_owned_origin owner
         | Some selected => import_located_path selected
         end
       end;
     import_owned_cursor := import_wire_next_cursor (import_owned_cursor owner) (map import_located_entry entries) final |}.

Theorem import_owned_advance_never_changes_the_original_root : forall owner entries final,
  import_owned_original_root (import_advance_owned_cursor owner entries final) = import_owned_original_root owner.
Proof. reflexivity. Qed.

Theorem import_owned_nonterminal_advance_retains_the_wire_root_and_origin : forall owner entries frame rest,
  import_owned_origin (import_advance_owned_cursor owner entries (frame :: rest)) = import_owned_origin owner /\
  import_cursor_root (import_owned_cursor (import_advance_owned_cursor owner entries (frame :: rest))) =
    import_cursor_root (import_owned_cursor owner).
Proof. intros. split; reflexivity. Qed.

Theorem import_owned_leaf_only_exhaustion_retains_all_owner_coordinates : forall owner entries,
  import_last_located_history entries = None -> import_advance_owned_cursor owner entries [] = owner.
Proof.
  intros owner entries Empty. unfold import_advance_owned_cursor. rewrite Empty.
  rewrite import_no_history_exhaustion_retains_input_cursor
    by now apply import_missing_last_occurrence_has_no_history.
  destruct owner. reflexivity.
Qed.

Theorem import_owned_singleton_advance_retains_the_exact_original_occurrence :
  forall read owner entries selected,
  Forall (import_complete_occurrence_witness read (import_owned_original_root owner)) entries ->
  import_last_located_history entries = Some selected ->
  (forall key edges, read key = Some edges -> import_cursor_word_validb key = true) ->
  exists raw_key,
    import_owned_cursor (import_advance_owned_cursor owner entries []) = ImportCursorStart raw_key /\
    import_owned_origin (import_advance_owned_cursor owner entries []) = import_located_path selected /\
    import_history_path read (import_owned_original_root owner) (import_located_path selected) raw_key.
Proof.
  intros read owner entries selected Complete Last Valid.
  destruct (import_exhausted_cursor_keeps_exact_last_occurrence_origin
    _ (import_owned_cursor owner) _ _ _ Complete Last Valid) as [raw_key [Cursor Path]].
  exists raw_key. unfold import_advance_owned_cursor. rewrite Last. auto.
Qed.

Theorem import_owned_advance_preserves_its_authenticated_origin : forall read owner entries final,
  import_history_path read (import_owned_original_root owner) (import_owned_origin owner)
    (import_cursor_root (import_owned_cursor owner)) ->
  Forall (import_complete_occurrence_witness read (import_owned_original_root owner)) entries ->
  (forall key edges, read key = Some edges -> import_cursor_word_validb key = true) ->
  let next := import_advance_owned_cursor owner entries final in
  import_history_path read (import_owned_original_root next) (import_owned_origin next)
    (import_cursor_root (import_owned_cursor next)).
Proof.
  intros read owner entries [|frame rest] Origin Complete Valid.
  - destruct (import_last_located_history entries) as [selected|] eqn:Last.
    + destruct (import_owned_singleton_advance_retains_the_exact_original_occurrence
        _ _ _ _ Complete Last Valid) as [raw_key [Cursor [Path OriginPath]]].
      cbn zeta. rewrite import_owned_advance_never_changes_the_original_root, Path, Cursor. exact OriginPath.
    + cbn zeta. now rewrite import_owned_leaf_only_exhaustion_retains_all_owner_coordinates by exact Last.
  - exact Origin.
Qed.

Definition import_located_root_event (origin key : list nat) : ImportLocatedEntry :=
  {| import_located_entry := ImportExportHistory (import_key_to_nat key) [];
     import_located_path := origin |}.

Definition import_tape_evaluate_located_cursor (origin : list nat) :=
  import_tape_collect_cursor (import_locate_step_event origin) (import_located_root_event origin).

Theorem import_located_collection_preserves_all_cursor_results : forall origin cursor skip budget tape,
  import_tape_erase_collection import_located_entry
    (import_tape_evaluate_located_cursor origin cursor skip budget tape) =
    import_tape_evaluate_cursor cursor skip budget tape.
Proof.
  intros. apply import_collector_erasure_preserves_cursor_errors_and_reads.
  - apply import_step_annotations_erase_exactly.
  - reflexivity.
Qed.

Theorem import_located_zero_skip_slice_is_a_page : forall read origin budget frames entries final,
  import_located_slice read origin 0 budget frames entries final ->
  import_located_page read origin budget frames entries final.
Proof. intros. inversion H; subst; try assumption; try constructor; lia. Qed.

Theorem import_located_collection_has_exact_step_occurrences :
  forall fuel read origin skip budget frames tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_collect_slice (import_locate_step_event origin) fuel skip budget frames tape =
    ImportTapeReturn (entries, final) remaining ->
  import_located_slice read origin skip budget frames entries final /\ import_tape_suffix tape remaining.
Proof.
  induction fuel as [|fuel IH]; intros read origin skip budget [|frame frames] tape entries final remaining Agree Result.
  - inversion Result; subst. split; [constructor|apply import_tape_suffix_refl].
  - cbn [import_tape_collect_slice] in Result.
    destruct ((skip =? 0) && (budget =? 0)) eqn:Stop; [|discriminate].
    apply andb_true_iff in Stop as [Skip Budget]. apply Nat.eqb_eq in Skip, Budget. subst skip budget.
    inversion Result; subst. split; [constructor; constructor|apply import_tape_suffix_refl].
  - inversion Result; subst. split; [constructor|apply import_tape_suffix_refl].
  - cbn [import_tape_collect_slice] in Result.
    destruct ((skip =? 0) && (budget =? 0)) eqn:Stop.
    { apply andb_true_iff in Stop as [Skip Budget]. apply Nat.eqb_eq in Skip, Budget. subst skip budget.
      inversion Result; subst. split; [constructor; constructor|apply import_tape_suffix_refl]. }
    destruct (import_tape_evaluate_step (frame :: frames) tape) as [step rest|error] eqn:Step;
      cbn [import_tape_bind] in Result; [|discriminate].
    destruct (import_tape_step_refines_exact_step_evaluation _ _ _ _ _ Agree Step) as [Evaluated Suffix].
    destruct step as [|event next|key]; [| |discriminate].
    { apply import_evaluated_exhaustion_has_empty_stack in Evaluated. discriminate. }
    apply import_evaluated_step_is_operational in Evaluated.
    pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Suffix) as RestAgree.
    destruct (skip =? 0) eqn:Skip.
    + apply Nat.eqb_eq in Skip. subst skip.
      assert (0 < budget) as Positive by (destruct budget; [discriminate|lia]).
      destruct (import_tape_collect_slice (import_locate_step_event origin)
        fuel 0 (budget - import_event_history_cost event) next rest) as [[tail last] rest'|error] eqn:Tail;
        cbn [import_tape_bind fst snd] in Result; [|discriminate].
      inversion Result; subst entries final remaining.
      destruct (IH _ _ _ _ _ _ _ _ _ RestAgree Tail) as [Located Last].
      split.
      * apply import_located_slice_ready. eapply import_located_page_next; [exact Positive|exact Evaluated|].
        now apply import_located_zero_skip_slice_is_a_page.
      * eapply import_tape_suffix_trans; eauto.
    + apply Nat.eqb_neq in Skip.
      destruct (IH _ _ _ _ _ _ _ _ _ RestAgree Result) as [Located Last].
      split; [eapply import_located_slice_skip; [lia|exact Evaluated|exact Located]|].
      eapply import_tape_suffix_trans; eauto.
Qed.

Theorem import_located_anchor_has_exact_occurrences :
  forall read origin anchor skip budget frames tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_collect_anchored_export (import_locate_step_event origin) (import_located_root_event origin)
    anchor skip budget frames tape = ImportTapeReturn (entries, final) remaining ->
  import_located_anchored_export read origin anchor skip budget frames entries final /\
    import_tape_suffix tape remaining.
Proof.
  intros read origin [key|] [|skip] budget frames tape entries final remaining Agree Result.
  - destruct budget as [|budget]; [discriminate|]. cbn [import_tape_collect_anchored_export] in Result.
    destruct (import_tape_collect_run_slice (import_locate_step_event origin) 0 budget frames tape)
      as [[tail last] rest|error] eqn:Slice; cbn [import_tape_bind fst snd] in Result; [|discriminate].
    inversion Result; subst entries final remaining.
    destruct (import_located_collection_has_exact_step_occurrences _ _ _ _ _ _ _ _ _ _ Agree Slice)
      as [Located Suffix]. split; [|exact Suffix].
    apply import_located_anchored_take_root. now apply import_located_zero_skip_slice_is_a_page.
  - destruct (import_located_collection_has_exact_step_occurrences _ _ _ _ _ _ _ _ _ _ Agree Result)
      as [Located Suffix]. split; [now apply import_located_anchored_skip_root|exact Suffix].
  - destruct (import_located_collection_has_exact_step_occurrences _ _ _ _ _ _ _ _ _ _ Agree Result)
      as [Located Suffix]. split; [now apply import_located_anchored_resume|exact Suffix].
  - destruct (import_located_collection_has_exact_step_occurrences _ _ _ _ _ _ _ _ _ _ Agree Result)
      as [Located Suffix]. split; [now apply import_located_anchored_resume|exact Suffix].
Qed.

Theorem import_located_cursor_collector_returns_the_operational_occurrences :
  forall read origin cursor skip budget tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_located_cursor origin cursor skip budget tape = ImportTapeReturn (entries, final) remaining ->
  import_located_cursor_export read origin cursor skip budget entries final /\ import_tape_suffix tape remaining.
Proof.
  intros read origin cursor skip budget tape entries final remaining Agree Result.
  unfold import_tape_evaluate_located_cursor, import_tape_collect_cursor in Result.
  destruct (import_cursor_validb cursor) eqn:Valid; [|discriminate].
  destruct ((skip =? 0) && (budget =? 0)) eqn:Budget; [discriminate|].
  destruct (import_consume_history_read (import_cursor_root cursor) tape)
    as [root_edges after_probe|error] eqn:Probe; cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_tape_read_refines_the_checked_reader _ _ _ _ _ Agree Probe) as [Root ProbeSuffix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree ProbeSuffix) as ProbeAgree.
  destruct (import_tape_build_cursor cursor after_probe) as [frames after_stack|error] eqn:Stack;
    cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_tape_cursor_builds_the_exact_initial_stack _ _ _ _ _ ProbeAgree Stack)
    as [Initial StackSuffix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ ProbeAgree StackSuffix) as StackAgree.
  destruct (import_located_anchor_has_exact_occurrences _ _ _ _ _ _ _ _ _ _ StackAgree Result)
    as [Export FinalSuffix]. split.
  - split; [exact Valid|]. split.
    + apply andb_false_iff in Budget. destruct Budget as [Skip|Take];
        apply Nat.eqb_neq in Skip || apply Nat.eqb_neq in Take; lia.
    + exists frames. auto.
  - eapply import_tape_suffix_trans; [exact ProbeSuffix|]. eapply import_tape_suffix_trans; eauto.
Qed.

Theorem import_located_slice_has_complete_witnesses : forall read origin skip budget frames entries final,
  import_located_slice read origin skip budget frames entries final -> forall original wire_root,
  import_history_path read original origin wire_root -> import_frame_spine read wire_root frames ->
  Forall (import_complete_occurrence_witness read original) entries.
Proof.
  intros read origin skip budget frames entries final Slice. induction Slice;
    intros original wire_root Origin Spine.
  - eapply import_located_page_has_complete_occurrences; eauto.
  - constructor.
  - eapply IHSlice; [exact Origin|]. eapply import_operational_step_preserves_frame_spine; eauto.
Qed.

Theorem import_located_cursor_has_complete_witnesses : forall read origin cursor skip budget entries final original,
  import_located_cursor_export read origin cursor skip budget entries final ->
  import_history_path read original origin (import_cursor_root cursor) ->
  Forall (import_complete_occurrence_witness read original) entries.
Proof.
  intros read origin cursor skip budget entries final original [Valid [Positive [frames [Stack Export]]]] Origin.
  pose proof (import_cursor_stack_has_frame_spine _ _ _ Stack) as Spine.
  destruct cursor as [root|root prefix carrier]; cbn [import_cursor_anchor import_cursor_root] in *.
  - inversion Export as [|key remaining cap initial result tail Slice|key cap initial result tail Page]; subst.
    + eapply import_located_slice_has_complete_witnesses; eauto.
    + constructor; [exists root; auto|]. eapply import_located_page_has_complete_occurrences; eauto.
  - inversion Export as [remaining cap initial result tail Slice| |]; subst.
    eapply import_located_slice_has_complete_witnesses; eauto.
Qed.

Definition import_tape_evaluate_owned_located (owner : ImportOwnedCursor) (skip budget : nat)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult (ImportTapeCollectedSlice ImportLocatedEntry) :=
  import_tape_bind (import_tape_check_original_root owner tape)
    (fun _ remaining => import_tape_evaluate_located_cursor (import_owned_origin owner)
      (import_owned_cursor owner) skip budget remaining).

Definition import_tape_checked_owned_located (Hash : list nat -> list nat)
    (rows : list ImportReceivedHistoryRow) (owner : ImportOwnedCursor) (skip budget : nat)
    (tape : list ImportHistoryLogicalRead) : ImportTapeResult (ImportTapeCollectedSlice ImportLocatedEntry) :=
  import_tape_prepared_complete_attempt Hash rows (import_tape_evaluate_owned_located owner skip budget) tape.

Theorem import_owned_location_erasure_preserves_origin_errors_and_reads : forall owner skip budget tape,
  import_tape_erase_collection import_located_entry (import_tape_evaluate_owned_located owner skip budget tape) =
    import_tape_evaluate_owned_cursor owner skip budget tape.
Proof.
  intros. unfold import_tape_evaluate_owned_located, import_tape_evaluate_owned_cursor.
  destruct (import_tape_check_original_root owner tape) as [[] remaining|error];
    cbn [import_tape_bind import_tape_erase_collection]; [|reflexivity].
  apply import_located_collection_preserves_all_cursor_results.
Qed.

Theorem import_owned_collector_authenticates_its_returned_locations :
  forall read owner skip budget tape entries final remaining,
  import_tape_reader_agrees read tape ->
  import_tape_evaluate_owned_located owner skip budget tape = ImportTapeReturn (entries, final) remaining ->
  import_history_path read (import_owned_original_root owner) (import_owned_origin owner)
    (import_cursor_root (import_owned_cursor owner)) /\
  import_located_cursor_export read (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
  Forall (import_complete_occurrence_witness read (import_owned_original_root owner)) entries /\
  import_tape_suffix tape remaining.
Proof.
  intros read owner skip budget tape entries final remaining Agree Result.
  unfold import_tape_evaluate_owned_located in Result.
  destruct (import_tape_check_original_root owner tape) as [[] page_tape|error] eqn:Origin;
    cbn [import_tape_bind] in Result; [|discriminate].
  destruct (import_consumed_origin_authenticates_the_wire_root _ _ _ _ Agree Origin) as [Path Prefix].
  pose proof (import_tape_suffix_preserves_reader_agreement _ _ _ Agree Prefix) as PageAgree.
  destruct (import_located_cursor_collector_returns_the_operational_occurrences _ _ _ _ _ _ _ _ _ PageAgree Result)
    as [Located Suffix].
  split; [exact Path|]. split; [exact Located|]. split.
  - eapply import_located_cursor_has_complete_witnesses; eauto.
  - eapply import_tape_suffix_trans; eauto.
Qed.

Theorem import_actual_owned_collector_authenticates_locations_before_writes :
  forall Hash rows initial records logical current observations owner skip budget entries final,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_located Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_history_path (import_history_observed_reader Hash rows observations)
    (import_owned_original_root owner) (import_owned_origin owner) (import_cursor_root (import_owned_cursor owner)) /\
  import_located_cursor_export (import_history_observed_reader Hash rows observations)
    (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
  Forall (import_complete_occurrence_witness (import_history_observed_reader Hash rows observations)
    (import_owned_original_root owner)) entries /\ Forall import_tape_successful_record logical.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final Trace Accepted.
  destruct (import_prepared_complete_attempt_consumes_every_record _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  destruct (import_owned_collector_authenticates_its_returned_locations _ _ _ _ _ _ _ _
    (import_physical_history_trace_derives_tape_reader_agreement _ _ _ _ _ _ _ Trace) Evaluated)
    as [Path [Located [Complete [prefix [Same Successful]]]]].
  split; [exact Path|]. split; [exact Located|]. split; [exact Complete|].
  rewrite app_nil_r in Same. now subst logical.
Qed.

Theorem import_actual_owned_collector_preserves_the_same_locations_after_commit :
  forall Hash rows initial records logical current observations owner skip budget entries final storage_ok result,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_located Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_history_path (import_checked_wire_history Hash result)
    (import_owned_original_root owner) (import_owned_origin owner) (import_cursor_root (import_owned_cursor owner)) /\
  import_located_cursor_export (import_checked_wire_history Hash result)
    (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
  Forall (import_complete_occurrence_witness (import_checked_wire_history Hash result)
    (import_owned_original_root owner)) entries.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final storage_ok result
    Trace Accepted Commit.
  destruct (import_prepared_complete_attempt_consumes_every_record _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  destruct (import_owned_collector_authenticates_its_returned_locations _ _ _ _ _ _ _ _
    (import_actual_history_commit_derives_tape_reader_agreement _ _ _ _ _ _ _ _ _ Trace Commit) Evaluated)
    as [Path [Located [Complete _]]]. auto.
Qed.

Theorem import_history_byte_extension_preserves_checked_reads : forall Hash first last,
  import_history_store_extension first last ->
  import_history_reader_extension (import_checked_wire_history Hash first) (import_checked_wire_history Hash last).
Proof.
  intros Hash first last Extension key edges Read. unfold import_checked_wire_history in *.
  destruct (import_cursor_word_validb key); [|discriminate].
  destruct (first key) as [bytes|] eqn:Stored; [|discriminate]. now rewrite (Extension _ _ Stored).
Qed.

Theorem import_owned_locations_survive_later_history_extensions :
  forall Hash rows initial records logical current observations owner skip budget entries final storage_ok result later,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_tape_checked_owned_located Hash rows owner skip budget logical = ImportTapeReturn (entries, final) [] ->
  import_observed_history_batch Hash observations current rows storage_ok = (result, true) ->
  import_history_store_extension result later ->
  import_history_path (import_checked_wire_history Hash later)
    (import_owned_original_root owner) (import_owned_origin owner) (import_cursor_root (import_owned_cursor owner)) /\
  import_located_cursor_export (import_checked_wire_history Hash later)
    (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
  Forall (import_complete_occurrence_witness (import_checked_wire_history Hash later)
    (import_owned_original_root owner)) entries.
Proof.
  intros Hash rows initial records logical current observations owner skip budget entries final storage_ok result later
    Trace Accepted Commit Extension.
  destruct (import_prepared_complete_attempt_consumes_every_record _ _ _ _ _ _ _ Accepted)
    as [_ [Evaluated _]].
  pose proof (import_actual_history_commit_derives_tape_reader_agreement _ _ _ _ _ _ _ _ _ Trace Commit) as Agree.
  pose proof (import_history_byte_extension_preserves_checked_reads Hash _ _ Extension) as ReaderExtension.
  assert (import_tape_reader_agrees (import_checked_wire_history Hash later) logical) as LaterAgree.
  { intros record edges Present Success. apply ReaderExtension. now apply Agree with (record := record). }
  destruct (import_owned_collector_authenticates_its_returned_locations _ _ _ _ _ _ _ _ LaterAgree Evaluated)
    as [Path [Located [Complete _]]]. auto.
Qed.

Record ImportOwnedCommitResult : Type := {
  import_owned_committed_history : ImportHistoryByteStore;
  import_owned_committed_cold : ImportEncodedColdStore;
  import_owned_committed_cursor : ImportOwnedCursor;
  import_owned_commit_success : bool
}.

Section OwnedColdPage.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable Hash : list nat -> list nat.

Definition import_prepare_owned_page (history_rows : list ImportReceivedHistoryRow)
    (cold_rows : list ImportReceivedColdRow) (owner : ImportOwnedCursor) (skip budget : nat)
    (logical : list ImportHistoryLogicalRead) (start next : ImportWirePath)
    : option (ImportTapeCollectedSlice ImportLocatedEntry) :=
  match import_cursor_encode (import_owned_cursor owner) with
  | None => None
  | Some request => match import_tape_checked_owned_located Hash history_rows owner skip budget logical with
    | ImportTapeReturn (entries, final) [] =>
        match import_adapt_wire_result (import_owned_cursor owner)
          (ImportEvaluationSuccess (map import_located_entry entries) final) with
        | ImportWireSuccess computed =>
            match import_check_received_cold_page Value decode_item Hash entries cold_rows with
            | Some _ => if import_check_reply_metadata request start next
                (map (fun row => import_key_to_nat (fst row)) history_rows)
                (map (fun row => import_key_to_nat (fst row)) cold_rows) computed
              then Some (entries, final) else None
            | None => None
            end
        | _ => None
        end
    | _ => None
    end
  end.

Theorem import_owned_preparation_checks_the_returned_list_before_any_commit :
  forall history_rows cold_rows owner skip budget logical start next entries final,
  import_prepare_owned_page history_rows cold_rows owner skip budget logical start next = Some (entries, final) ->
  import_tape_checked_owned_located Hash history_rows owner skip budget logical = ImportTapeReturn (entries, final) [] /\
  exists request computed cold_candidate,
    import_cursor_encode (import_owned_cursor owner) = Some request /\
    import_adapt_wire_result (import_owned_cursor owner)
      (ImportEvaluationSuccess (map import_located_entry entries) final) = ImportWireSuccess computed /\
    import_check_reply_metadata request start next
      (map (fun row => import_key_to_nat (fst row)) history_rows)
      (map (fun row => import_key_to_nat (fst row)) cold_rows) computed = true /\
    import_check_received_cold_page Value decode_item Hash entries cold_rows = Some cold_candidate.
Proof.
  intros history_rows cold_rows owner skip budget logical start next entries final Prepared.
  unfold import_prepare_owned_page in Prepared.
  destruct (import_cursor_encode (import_owned_cursor owner)) as [request|] eqn:Request; [|discriminate].
  destruct (import_tape_checked_owned_located Hash history_rows owner skip budget logical)
    as [[actual frames] [|extra rest]|error] eqn:Collected; try discriminate.
  destruct (import_adapt_wire_result (import_owned_cursor owner)
    (ImportEvaluationSuccess (map import_located_entry actual) frames)) as [computed| | |key|] eqn:Adapted;
    try discriminate.
  destruct (import_check_received_cold_page Value decode_item Hash actual cold_rows) as [candidate|] eqn:Cold;
    [|discriminate].
  destruct (import_check_reply_metadata request start next
    (map (fun row => import_key_to_nat (fst row)) history_rows)
    (map (fun row => import_key_to_nat (fst row)) cold_rows) computed) eqn:Metadata; [|discriminate].
  inversion Prepared; subst actual frames.
  split; [reflexivity|]. exists request, computed, candidate. auto.
Qed.

Definition import_commit_owned_page (history_rows : list ImportReceivedHistoryRow)
    (cold_rows : list ImportReceivedColdRow) (owner : ImportOwnedCursor) (skip budget : nat)
    (logical : list ImportHistoryLogicalRead) (start next : ImportWirePath)
    (history_observations : ImportHistoryObservations) (history_current : ImportHistoryByteStore)
    (cold_observations : ImportColdObservations) (cold_current : ImportEncodedColdStore)
    (history_ok cold_ok : bool) : ImportOwnedCommitResult :=
  match import_prepare_owned_page history_rows cold_rows owner skip budget logical start next with
  | None => {| import_owned_committed_history := history_current; import_owned_committed_cold := cold_current;
               import_owned_committed_cursor := owner; import_owned_commit_success := false |}
  | Some (entries, final) =>
      let '(history_result, history_success) :=
        import_observed_history_batch Hash history_observations history_current history_rows history_ok in
      if history_success then
        let '(cold_result, cold_success) := import_observed_physical_cold_batch cold_observations cold_current
          (map import_compile_received_cold_row cold_rows) cold_ok in
        {| import_owned_committed_history := history_result; import_owned_committed_cold := cold_result;
           import_owned_committed_cursor := if cold_success then import_advance_owned_cursor owner entries final else owner;
           import_owned_commit_success := cold_success |}
      else {| import_owned_committed_history := history_result; import_owned_committed_cold := cold_current;
              import_owned_committed_cursor := owner; import_owned_commit_success := false |}
  end.

Theorem import_invalid_owned_page_has_no_attempt_writes :
  forall history_rows cold_rows owner skip budget logical start next history_observations history_current
    cold_observations cold_current history_ok cold_ok,
  import_prepare_owned_page history_rows cold_rows owner skip budget logical start next = None ->
  import_commit_owned_page history_rows cold_rows owner skip budget logical start next
    history_observations history_current cold_observations cold_current history_ok cold_ok =
    {| import_owned_committed_history := history_current; import_owned_committed_cold := cold_current;
       import_owned_committed_cursor := owner; import_owned_commit_success := false |}.
Proof. intros. unfold import_commit_owned_page. now rewrite H. Qed.

Theorem import_failed_owned_commit_never_advances_the_cursor :
  forall history_rows cold_rows owner skip budget logical start next history_observations history_current
    cold_observations cold_current history_ok cold_ok result,
  import_commit_owned_page history_rows cold_rows owner skip budget logical start next
    history_observations history_current cold_observations cold_current history_ok cold_ok = result ->
  import_owned_commit_success result = false -> import_owned_committed_cursor result = owner.
Proof.
  intros history_rows cold_rows owner skip budget logical start next history_observations history_current
    cold_observations cold_current history_ok cold_ok result Commit Failed.
  unfold import_commit_owned_page in Commit.
  destruct (import_prepare_owned_page history_rows cold_rows owner skip budget logical start next)
    as [[entries final]|]; [|now subst result].
  destruct (import_observed_history_batch Hash history_observations history_current history_rows history_ok)
    as [history_result history_success]. destruct history_success; [|now subst result].
  destruct (import_observed_physical_cold_batch cold_observations cold_current
    (map import_compile_received_cold_row cold_rows) cold_ok) as [cold_result cold_success].
  subst result. cbn in Failed |- *. now rewrite Failed.
Qed.

Theorem import_failed_cold_commit_keeps_history_and_the_previous_owner :
  forall history_rows cold_rows owner skip budget logical start next entries final history_observations history_current
    cold_observations cold_current history_ok cold_ok history_result cold_result,
  import_prepare_owned_page history_rows cold_rows owner skip budget logical start next = Some (entries, final) ->
  import_observed_history_batch Hash history_observations history_current history_rows history_ok = (history_result, true) ->
  import_observed_physical_cold_batch cold_observations cold_current
    (map import_compile_received_cold_row cold_rows) cold_ok = (cold_result, false) ->
  cold_result = cold_current /\
  import_history_store_extension history_current history_result /\
  import_commit_owned_page history_rows cold_rows owner skip budget logical start next
    history_observations history_current cold_observations cold_current history_ok cold_ok =
    {| import_owned_committed_history := history_result; import_owned_committed_cold := cold_current;
       import_owned_committed_cursor := owner; import_owned_commit_success := false |}.
Proof.
  intros history_rows cold_rows owner skip budget logical start next entries final history_observations history_current
    cold_observations cold_current history_ok cold_ok history_result cold_result Prepared History Cold.
  pose proof (import_failed_physical_batch_preserves_current_storage _ _ _ _ _ Cold) as Same.
  split; [exact Same|]. split; [eapply import_history_batch_preserves_all_existing_bytes; exact History|].
  unfold import_commit_owned_page. now rewrite Prepared, History, Cold, Same.
Qed.

Theorem import_owned_commit_success_requires_validation_and_both_commits :
  forall history_rows cold_rows owner skip budget logical start next history_observations history_current
    cold_observations cold_current history_ok cold_ok result,
  import_commit_owned_page history_rows cold_rows owner skip budget logical start next
    history_observations history_current cold_observations cold_current history_ok cold_ok = result ->
  import_owned_commit_success result = true ->
  exists entries final,
    import_prepare_owned_page history_rows cold_rows owner skip budget logical start next = Some (entries, final) /\
    import_observed_history_batch Hash history_observations history_current history_rows history_ok =
      (import_owned_committed_history result, true) /\
    import_observed_physical_cold_batch cold_observations cold_current
      (map import_compile_received_cold_row cold_rows) cold_ok = (import_owned_committed_cold result, true) /\
    import_owned_committed_cursor result = import_advance_owned_cursor owner entries final.
Proof.
  intros history_rows cold_rows owner skip budget logical start next history_observations history_current
    cold_observations cold_current history_ok cold_ok result Commit Success.
  unfold import_commit_owned_page in Commit.
  destruct (import_prepare_owned_page history_rows cold_rows owner skip budget logical start next)
    as [[entries final]|] eqn:Prepared; [|subst result; discriminate].
  destruct (import_observed_history_batch Hash history_observations history_current history_rows history_ok)
    as [history_result history_success] eqn:History.
  destruct history_success; [|subst result; discriminate].
  destruct (import_observed_physical_cold_batch cold_observations cold_current
    (map import_compile_received_cold_row cold_rows) cold_ok) as [cold_result cold_success] eqn:Cold.
  subst result. cbn in Success. subst cold_success. exists entries, final. auto.
Qed.

Theorem import_owned_commit_connects_actual_reads_to_exact_typed_consumption :
  forall history_rows cold_rows owner skip budget logical start next history_initial history_records
    history_observations history_current cold_initial cold_records cold_observations cold_current
    history_ok cold_ok result history_hash,
  import_history_observation_trace Hash history_rows history_initial history_records logical
    history_current history_observations ->
  import_cold_observation_trace cold_initial cold_records cold_current cold_observations ->
  import_commit_owned_page history_rows cold_rows owner skip budget logical start next
    history_observations history_current cold_observations cold_current history_ok cold_ok = result ->
  import_owned_commit_success result = true ->
  exists entries final,
    import_prepare_owned_page history_rows cold_rows owner skip budget logical start next = Some (entries, final) /\
    import_located_cursor_export (import_history_observed_reader Hash history_rows history_observations)
      (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
    import_located_cursor_export (import_checked_wire_history Hash (import_owned_committed_history result))
      (import_owned_origin owner) (import_owned_cursor owner) skip budget entries final /\
    Forall (import_complete_occurrence_witness (import_checked_wire_history Hash (import_owned_committed_history result))
      (import_owned_original_root owner)) entries /\
    import_cursor_encode (import_owned_cursor owner) = Some start /\
    import_cursor_encode (import_owned_cursor (import_owned_committed_cursor result)) = Some next /\
    (forall key, In key (map (fun row => import_key_to_nat (fst row)) history_rows) <->
      In key (import_export_history_keys (map import_located_entry entries))) /\
    (forall key, In key (map (fun row => import_key_to_nat (fst row)) cold_rows) <->
      In key (import_export_leaf_keys (map import_located_entry entries))) /\
    import_history_store_extension history_current (import_owned_committed_history result) /\
    import_encoded_extension cold_current (import_owned_committed_cold result) /\
    NoDup (map import_cold_read_location cold_records) /\
    import_owned_original_root (import_owned_committed_cursor result) = import_owned_original_root owner /\
    import_history_path (import_checked_wire_history Hash (import_owned_committed_history result))
      (import_owned_original_root owner) (import_owned_origin (import_owned_committed_cursor result))
      (import_cursor_root (import_owned_cursor (import_owned_committed_cursor result))) /\
    (forall alias key bytes selected, In (import_encoded_row alias key bytes) (map import_compile_received_cold_row cold_rows) ->
      exists record, In record cold_records /\ import_cold_read_alias record = selected /\
        import_cold_read_key record = key /\ import_cold_read_record_valid record /\
        import_cold_read_result record <> ImportRootLookupFailure /\
        cold_current selected key = import_cold_read_store record selected key) /\
    (forall entry key kind, In entry entries -> import_located_reference entry = Some (ImportColdRef key kind) ->
      import_consumer_checked_read Value decode_item history_hash
        (fun payload => import_received_hash Hash (import_nat_to_key 8 (length payload) ++ payload))
        (import_encoded_logical_view (import_project_wire_history (import_owned_committed_history result))
          (import_owned_committed_cold result)) (ImportColdRef key kind) = Some []).
Proof.
  intros history_rows cold_rows owner skip budget logical start next history_initial history_records
    history_observations history_current cold_initial cold_records cold_observations cold_current
    history_ok cold_ok result history_hash HistoryTrace ColdTrace Commit Success.
  destruct (import_owned_commit_success_requires_validation_and_both_commits
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ Commit Success)
    as [entries [final [Prepared [History [Cold Advance]]]]].
  destruct (import_owned_preparation_checks_the_returned_list_before_any_commit
    _ _ _ _ _ _ _ _ _ _ Prepared)
    as [Collected [request [computed [candidate [Request [Adapted [Metadata Checked]]]]]]].
  destruct (import_actual_owned_collector_authenticates_locations_before_writes
    _ _ _ _ _ _ _ _ _ _ _ _ HistoryTrace Collected) as [_ [Before _]].
  destruct (import_actual_owned_collector_preserves_the_same_locations_after_commit
    _ _ _ _ _ _ _ _ _ _ _ _ _ _ HistoryTrace Collected History) as [Path [After Complete]].
  destruct (import_accepted_reply_matches_checked_export _ _ _ _ _ _ _ _ Adapted Metadata)
    as [actual [frames [Same [Start [Next [HistoryKeys ColdKeys]]]]]].
  inversion Same; subst actual frames.
  destruct (import_received_rows_with_actual_read_trace_preserve_bytes_provenance_and_typed_consumption
    Value decode_item Hash entries cold_rows candidate cold_initial cold_records cold_observations cold_current
    cold_ok (import_owned_committed_cold result) (import_project_wire_history (import_owned_committed_history result))
    history_hash Checked ColdTrace Cold) as [_ [NoDuplicates [ColdExtension [Records Consumed]]]].
  exists entries, final. split; [exact Prepared|]. split; [exact Before|]. split; [exact After|].
  split; [exact Complete|]. split; [now rewrite Start|].
  split; [rewrite Advance; exact Next|]. split; [exact HistoryKeys|]. split; [exact ColdKeys|].
  split; [eapply import_history_batch_preserves_all_existing_bytes; exact History|].
  split; [exact ColdExtension|]. split; [exact NoDuplicates|].
  split; [now rewrite Advance|]. split; [|auto].
  rewrite Advance. eapply import_owned_advance_preserves_its_authenticated_origin; [exact Path|exact Complete|].
  intros key edges Read. apply import_checked_wire_history_has_exact_witness in Read as [Width [Domain _]].
  apply import_cursor_word_validity_characterization. auto.
Qed.

Theorem import_owned_receipt_consumption_survives_compatible_writes :
  forall history_before history_after cold_before cold_after history_hash payload_hash reference children,
  import_history_store_extension history_before history_after ->
  import_encoded_run cold_before cold_after ->
  import_consumer_checked_read Value decode_item history_hash payload_hash
    (import_encoded_logical_view (import_project_wire_history history_before) cold_before) reference = Some children ->
  import_consumer_checked_read Value decode_item history_hash payload_hash
    (import_encoded_logical_view (import_project_wire_history history_after) cold_after) reference = Some children.
Proof.
  intros history_before history_after cold_before cold_after history_hash payload_hash reference children
    History Cold Consumed.
  assert (import_binding_extension
    (import_encoded_logical_view (import_project_wire_history history_before) cold_before)
    (import_encoded_logical_view (import_project_wire_history history_after) cold_after)) as Extension.
  { apply import_encoded_view_preserves_joint_history_and_cold_extensions; [|exact Cold].
    now apply import_physical_history_extension_preserves_the_decoded_projection. }
  destruct (import_consumer_checked_read_binds_structure_and_consumption
    Value decode_item _ _ _ _ _ Consumed) as [Read Typed].
  apply import_consumer_checked_read_accepts_exactly_structural_and_typed_reads.
  - eapply import_binding_extension_preserves_checked_reads; eauto.
  - eapply import_binding_extension_preserves_reference_consumption; eauto.
Qed.

End OwnedColdPage.

Definition import_owned_demo : ImportOwnedCursor :=
  {| import_owned_original_root := import_history_demo_key;
     import_owned_origin := [];
     import_owned_cursor := ImportCursorStart import_history_demo_key |}.

Example import_owned_empty_origin_and_page_share_one_physical_observation :
  import_history_observation_trace import_history_demo_hash [] import_history_demo_store
    [import_history_demo_read] [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical]
    import_history_demo_store import_history_demo_observations /\
  import_tape_checked_owned_attempt import_history_demo_hash [] import_owned_demo 0 1
    [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical] =
    ImportTapeReturn ([ImportExportHistory (import_key_to_nat import_history_demo_key) []],
      [import_frame_at import_history_demo_key [] [] None]) [].
Proof.
  split.
  - change (import_history_observation_trace import_history_demo_hash [] import_history_demo_store
      [import_history_demo_read] ([import_history_demo_logical; import_history_demo_logical] ++
        [{| import_history_logical_key := import_history_demo_key;
            import_history_logical_cache := import_history_demo_observations;
            import_history_logical_result := import_history_cached_access import_history_demo_hash []
              import_history_demo_observations import_history_demo_key |}])
      import_history_demo_store import_history_demo_observations).
    eapply import_history_logical_lookup. exact import_two_history_callbacks_reuse_one_physical_read.
  - vm_compute. reflexivity.
Qed.

Example import_owned_failed_origin_cannot_be_skipped_for_later_page_reads :
  import_tape_checked_owned_attempt import_history_demo_hash [] import_owned_demo 0 1
    [import_tape_example_record import_history_demo_key (ImportHistoryAccessResult ImportCheckedHistoryStorageFailure);
     import_history_demo_logical; import_history_demo_logical] =
    ImportTapeFail (ImportTapeRejected import_history_demo_key (ImportHistoryAccessResult ImportCheckedHistoryStorageFailure)).
Proof. vm_compute. reflexivity. Qed.

Example import_owned_extra_origin_read_cannot_be_hidden :
  import_tape_checked_owned_attempt import_history_demo_hash [] import_owned_demo 0 1
    [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical; import_history_demo_logical] =
    ImportTapeFail ImportTapeTrailing.
Proof. vm_compute. reflexivity. Qed.

Example import_owned_missing_origin_read_cannot_reuse_the_page_setup_twice :
  import_tape_checked_owned_attempt import_history_demo_hash [] import_owned_demo 0 1
    [import_history_demo_logical; import_history_demo_logical] = ImportTapeFail (ImportTapeMissing import_history_demo_key).
Proof. vm_compute. reflexivity. Qed.

Example import_owned_wrong_wire_root_rejects_before_the_page :
  import_tape_check_original_root
    {| import_owned_original_root := import_history_demo_key;
       import_owned_origin := [];
       import_owned_cursor := ImportCursorStart (repeat 1 32) |}
    [import_history_demo_logical] = ImportTapeFail ImportTapeInvalidCursor.
Proof. vm_compute. reflexivity. Qed.

Example import_owned_resume_checks_the_carrier_separately :
  import_tape_checked_owned_attempt import_history_demo_hash []
    {| import_owned_original_root := import_history_demo_key;
       import_owned_origin := [];
       import_owned_cursor := ImportCursorResume import_history_demo_key [] (repeat 1 32) |} 0 1
    [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical] =
    ImportTapeFail ImportTapeInvalidStack.
Proof. vm_compute. reflexivity. Qed.

Example import_owned_leaf_only_exhaustion_keeps_a_resume_cursor :
  let owner := {| import_owned_original_root := repeat 1 32;
                  import_owned_origin := [7];
                  import_owned_cursor := ImportCursorResume import_history_demo_key [2] (repeat 3 32) |} in
  import_advance_owned_cursor owner
    [{| import_located_entry := ImportExportLeaf 9; import_located_path := [7; 2; 0] |}] [] = owner.
Proof. reflexivity. Qed.

Definition import_owned_shared_edge (slot : nat) : ImportWireEdge :=
  {| import_wire_slot := slot; import_wire_header := 129; import_wire_prefix := [5];
     import_wire_hash := import_history_demo_key |}.

Definition import_owned_shared_root (slot : nat) : ImportOwnedCursor :=
  {| import_owned_original_root := repeat slot 32;
     import_owned_origin := [slot; 5];
     import_owned_cursor := ImportCursorStart import_history_demo_key |}.

Definition import_owned_shared_tape (slot : nat) : list ImportHistoryLogicalRead :=
  [import_tape_example_record (repeat slot 32)
     (ImportHistoryAccessResult (ImportCheckedHistoryFound [import_owned_shared_edge slot]));
   import_history_demo_logical; import_history_demo_logical; import_history_demo_logical].

Example import_owned_shared_subtree_discards_both_distinct_ancestor_stacks :
  import_tape_checked_owned_attempt import_history_demo_hash [] (import_owned_shared_root 1) 0 1
    (import_owned_shared_tape 1) =
    ImportTapeReturn ([ImportExportHistory (import_key_to_nat import_history_demo_key) []],
      [import_frame_at import_history_demo_key [] [] None]) [] /\
  import_tape_checked_owned_attempt import_history_demo_hash [] (import_owned_shared_root 2) 0 1
    (import_owned_shared_tape 2) =
    ImportTapeReturn ([ImportExportHistory (import_key_to_nat import_history_demo_key) []],
      [import_frame_at import_history_demo_key [] [] None]) [] /\
  import_owned_original_root (import_owned_shared_root 1) <> import_owned_original_root (import_owned_shared_root 2) /\
  import_owned_origin (import_owned_shared_root 1) <> import_owned_origin (import_owned_shared_root 2).
Proof. vm_compute. repeat split; discriminate. Qed.

Example import_owned_truncated_compressed_origin_rejects :
  import_tape_check_original_root
    {| import_owned_original_root := repeat 1 32;
       import_owned_origin := [1];
       import_owned_cursor := ImportCursorStart import_history_demo_key |}
    (import_owned_shared_tape 1) = ImportTapeFail ImportTapeInvalidStack.
Proof. vm_compute. reflexivity. Qed.

Example import_owned_reordered_ancestor_reads_reject :
  import_tape_check_original_root (import_owned_shared_root 1)
    [import_history_demo_logical;
     import_tape_example_record (repeat 1 32)
       (ImportHistoryAccessResult (ImportCheckedHistoryFound [import_owned_shared_edge 1]))] =
    ImportTapeFail (ImportTapeUnexpected (repeat 1 32) import_history_demo_key).
Proof. vm_compute. reflexivity. Qed.

Example import_owned_repeated_hash_selects_the_last_occurrence_not_the_first :
  let first := {| import_located_entry := ImportExportHistory (import_key_to_nat import_history_demo_key) [1; 5];
                  import_located_path := [7; 1; 5] |} in
  let last := {| import_located_entry := ImportExportHistory (import_key_to_nat import_history_demo_key) [2; 5];
                 import_located_path := [7; 2; 5] |} in
  import_owned_origin (import_advance_owned_cursor import_owned_demo [first; last] []) = [7; 2; 5].
Proof. reflexivity. Qed.

Definition import_owned_demo_tape :=
  [import_history_demo_logical; import_history_demo_logical; import_history_demo_logical].

Example import_owned_collector_empty_root_has_one_exact_origin :
  import_tape_checked_owned_located import_history_demo_hash [(import_history_demo_key, [])]
    import_owned_demo 0 2 import_owned_demo_tape =
    ImportTapeReturn ([import_located_root_event [] import_history_demo_key], []) [].
Proof. vm_compute. reflexivity. Qed.

Example import_owned_preparation_accepts_an_exact_empty_root_page :
  import_prepare_owned_page (fun _ => unit) (fun _ _ => Some tt) import_history_demo_hash
    [(import_history_demo_key, [])] [] import_owned_demo 0 2 import_owned_demo_tape
    [(import_history_demo_key, None)] [(import_history_demo_key, None)] =
    Some ([import_located_root_event [] import_history_demo_key], []).
Proof. vm_compute. reflexivity. Qed.

Example import_owned_preparation_rejects_forged_metadata_before_any_writes :
  import_commit_owned_page (fun _ => unit) (fun _ _ => Some tt) import_history_demo_hash
    [(import_history_demo_key, [])] [] import_owned_demo 0 2 import_owned_demo_tape
    [(repeat 1 32, None)] [(import_history_demo_key, None)]
    import_history_demo_observations import_history_demo_store import_empty_cold_observations
    import_empty_encoded_store true true =
    {| import_owned_committed_history := import_history_demo_store;
       import_owned_committed_cold := import_empty_encoded_store;
       import_owned_committed_cursor := import_owned_demo; import_owned_commit_success := false |}.
Proof. vm_compute. reflexivity. Qed.

Example import_owned_preparation_rejects_unrequested_cold_rows_before_history_write :
  import_prepare_owned_page (fun _ => unit) (fun _ _ => Some tt) import_history_demo_hash
    [(import_history_demo_key, [])] [(import_history_demo_key, import_valid_empty_joins_bytes)]
    import_owned_demo 0 2 import_owned_demo_tape
    [(import_history_demo_key, None)] [(import_history_demo_key, None)] = None.
Proof. vm_compute. reflexivity. Qed.

Definition import_owned_join_edge : ImportWireEdge :=
  {| import_wire_slot := 2; import_wire_header := 0; import_wire_prefix := [];
     import_wire_hash := import_history_demo_key |}.

Definition import_owned_join_history_bytes := [2; 0] ++ import_history_demo_key.

Definition import_owned_join_logical : ImportHistoryLogicalRead :=
  {| import_history_logical_key := import_history_demo_key;
     import_history_logical_cache := import_history_commit_demo_absent;
     import_history_logical_result := ImportHistoryAccessResult (ImportCheckedHistoryFound [import_owned_join_edge]) |}.

Definition import_owned_join_tape :=
  [import_owned_join_logical; import_owned_join_logical; import_owned_join_logical].

Definition import_owned_join_attempt (cold_bytes : list nat) (cold_ok : bool) :=
  import_commit_owned_page (fun _ => unit) (fun _ _ => Some tt) import_history_demo_hash
    [(import_history_demo_key, import_owned_join_history_bytes)] [(import_history_demo_key, cold_bytes)]
    import_owned_demo 0 2 import_owned_join_tape
    [(import_history_demo_key, None)] [(import_history_demo_key, None)]
    import_history_commit_demo_absent import_history_commit_demo_empty
    import_control_absent_observations import_empty_encoded_store true cold_ok.

Example import_owned_nonempty_cold_page_commits_both_domains :
  let result := import_owned_join_attempt import_valid_empty_joins_bytes true in
  import_owned_commit_success result = true /\
  import_owned_committed_history result import_history_demo_key = Some import_owned_join_history_bytes /\
  import_owned_committed_cold result ImportRawAlias 0 = Some import_valid_empty_joins_bytes.
Proof. vm_compute. auto. Qed.

Example import_owned_matching_keys_cannot_hide_a_wrong_cold_kind :
  let result := import_owned_join_attempt import_valid_empty_data_bytes true in
  import_owned_commit_success result = false /\
  import_owned_committed_history result import_history_demo_key = None /\
  import_owned_committed_cold result ImportRawAlias 0 = None /\
  import_owned_committed_cursor result = import_owned_demo.
Proof. vm_compute. auto. Qed.

Example import_owned_cold_failure_retains_history_without_completion :
  let result := import_owned_join_attempt import_valid_empty_joins_bytes false in
  import_owned_commit_success result = false /\
  import_owned_committed_history result import_history_demo_key = Some import_owned_join_history_bytes /\
  import_owned_committed_cold result ImportRawAlias 0 = None /\
  import_owned_committed_cursor result = import_owned_demo.
Proof. vm_compute. auto. Qed.
