From Stdlib Require Import List Arith Bool.
From CostAccountedRho Require Import StateImportClosure StateImportStorage
  StateImportCodec StateImportCold StateImportEncodedCold StateImportObservations
  StateImportHistoryObservations StateImportTraversal StateImportCursor StateImportWire.
Import ListNotations.

Theorem import_encoded_run_fills_absence_only_with_resolved_content :
  forall first last, import_encoded_run first last -> forall selected query,
  first selected query = None ->
  last selected query = None \/
    exists leaf, import_resolve_encoded_cold last query = Some (Some leaf).
Proof.
  intros first last Run. induction Run as
    [store|first middle last alias key input storage_ok success Run IH Attempt];
    intros selected query Absent.
  - now left.
  - destruct (IH selected query Absent) as [Missing|[leaf Resolved]].
    + destruct success.
      * unfold import_encoded_attempt in Attempt.
        destruct (import_guarded_encoded_insert middle alias key input)
          as [candidate|] eqn:Inserted; [|discriminate].
        destruct storage_ok; [|discriminate]. inversion Attempt; subst candidate.
        destruct (import_guarded_encoded_insert_has_checked_commit_inputs
          _ _ _ _ _ Inserted) as [leaf [Parsed [_ [_ Same]]]].
        destruct (Nat.eq_dec query key) as [Equal|Different].
        -- subst query. right. exists leaf.
           eapply import_guarded_encoded_insert_preserves_bytes_and_resolves_the_inserted_leaf
             in Inserted; eauto. tauto.
        -- left. rewrite Same, import_fill_alias_preserves_other_keys; assumption.
      * apply import_failed_encoded_attempt_preserves_the_entire_byte_store in Attempt.
        subst last. now left.
    + right. exists leaf. eapply import_encoded_run_preserves_strict_resolution; eauto.
      eapply import_encoded_run_step; [constructor|exact Attempt].
Qed.

Definition import_cold_observations_current
    (observations : ImportColdObservations) (store : ImportEncodedColdStore) : Prop :=
  forall alias key, match observations alias key with
  | ImportColdObservedBytes bytes => store alias key = Some bytes
  | ImportColdObservedAbsent => store alias key = None \/
      exists leaf, import_resolve_encoded_cold store key = Some (Some leaf)
  | _ => True
  end.

Theorem import_observed_alias_bytes_or_repair_survive_guarded_writes :
  forall observations first last,
  import_cold_observations_current observations first ->
  import_encoded_run first last -> import_cold_observations_current observations last.
Proof.
  intros observations first last Current Run alias key.
  specialize (Current alias key). destruct (observations alias key); auto.
  - destruct Current as [Absent|[leaf Resolved]].
    + eapply import_encoded_run_fills_absence_only_with_resolved_content; eauto.
    + right. exists leaf. eapply import_encoded_run_preserves_strict_resolution; eauto.
  - destruct (import_arbitrary_encoded_attempts_preserve_bytes_and_logical_bindings
      (fun _ => None) _ _ Run) as [Bytes _]. now apply Bytes.
Qed.

Theorem import_actual_alias_observations_retain_bytes_or_guarded_repairs :
  forall initial records current observations,
  import_cold_observation_trace initial records current observations ->
  import_cold_observations_current observations current.
Proof.
  intros initial records current observations Trace. induction Trace.
  - intros alias key. exact I.
  - intros selected query. unfold import_capture_first_cold_observation.
    destruct (import_cold_alias_eq_dec selected alias) as [Equal|Different];
      [destruct (Nat.eq_dec query key) as [Same|Other]|];
      try apply IHTrace.
    subst selected query. rewrite H. unfold import_successful_cold_lookup.
    destruct (store alias key) as [bytes|] eqn:Read; simpl; auto.
  - intros selected query. unfold import_capture_first_cold_observation.
    destruct (import_cold_alias_eq_dec selected alias) as [Equal|Different];
      [destruct (Nat.eq_dec query key) as [Same|Other]|];
      try apply IHTrace.
    subst selected query. rewrite H. exact I.
  - eapply import_observed_alias_bytes_or_repair_survive_guarded_writes; eauto.
Qed.

Theorem import_strict_resolution_identifies_any_present_decoded_alias :
  forall store key resolved alias bytes leaf,
  import_resolve_encoded_cold store key = Some (Some resolved) ->
  store alias key = Some bytes -> import_read_persisted_leaf bytes = Some leaf ->
  resolved = leaf.
Proof.
  intros store key resolved alias bytes leaf Resolved Present Parsed.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards
    in Resolved as [_ [Raw Legacy]].
  destruct alias.
  - rewrite Present in Raw.
    apply import_encoded_alias_guard_preserves_malformed_presence in Raw. congruence.
  - rewrite Present in Legacy.
    apply import_encoded_alias_guard_preserves_malformed_presence in Legacy. congruence.
Qed.

Theorem import_observed_strict_leaf_establishes_a_current_binding :
  forall initial records current observations key leaf,
  import_cold_observation_trace initial records current observations ->
  import_cold_observation_ready (observations ImportRawAlias key) = true ->
  import_cold_observation_ready (observations ImportLegacyAlias key) = true ->
  import_resolve_encoded_cold (import_project_cold_observations observations) key =
    Some (Some leaf) ->
  import_resolve_encoded_cold current key = Some (Some leaf).
Proof.
  intros initial records current observations key leaf Trace RawReady LegacyReady Resolved.
  pose proof (import_actual_alias_observations_retain_bytes_or_guarded_repairs
    _ _ _ _ Trace) as Current.
  apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards
    in Resolved as [Read [Raw Legacy]].
  apply import_encoded_success_has_present_decodable_bytes in Read.
  assert (Present : exists alias bytes,
    current alias key = Some bytes /\ import_read_persisted_leaf bytes = Some leaf).
  { destruct Read as [[bytes [Found Parsed]]|[_ [bytes [Found Parsed]]]];
      unfold import_project_cold_observations in Found.
    - specialize (Current ImportRawAlias key).
      destruct (observations ImportRawAlias key); try discriminate.
      inversion Found; subst. exists ImportRawAlias, bytes. auto.
    - specialize (Current ImportLegacyAlias key).
      destruct (observations ImportLegacyAlias key); try discriminate.
      inversion Found; subst. exists ImportLegacyAlias, bytes. auto. }
  assert (Guard : forall alias,
    import_cold_observation_ready (observations alias key) = true ->
    import_encoded_alias_agrees (import_project_cold_observations observations alias key) leaf = true ->
    import_encoded_alias_agrees (current alias key) leaf = true).
  { intros alias Ready Agrees. specialize (Current alias key).
    unfold import_project_cold_observations in Agrees.
    destruct (observations alias key); try discriminate.
    - destruct Current as [Absent|[resolved Resolution]].
      + now rewrite Absent.
      + destruct Present as [selected [bytes [Found Parsed]]].
        pose proof (import_strict_resolution_identifies_any_present_decoded_alias
          _ _ _ _ _ _ Resolution Found Parsed) as Equal. subst resolved.
        apply import_encoded_strict_resolution_requires_actual_reader_and_both_guards
          in Resolution as [_ [R L]]. destruct alias; assumption.
    - now rewrite Current. }
  apply import_encoded_guards_and_presence_establish_strict_resolution.
  - now apply Guard.
  - now apply Guard.
  - destruct Present as [alias [bytes [Found _]]]. now exists alias, bytes.
Qed.

Definition import_read_observed_cold_leaf (observations : ImportColdObservations)
    (key : nat) : option ImportLeaf :=
  if import_cold_observation_ready (observations ImportRawAlias key) &&
     import_cold_observation_ready (observations ImportLegacyAlias key)
  then match import_resolve_encoded_cold (import_project_cold_observations observations) key with
    | Some (Some leaf) => Some leaf
    | _ => None
    end
  else None.

Theorem import_successful_read_only_cold_check_has_a_current_binding :
  forall initial records current observations key leaf,
  import_cold_observation_trace initial records current observations ->
  import_read_observed_cold_leaf observations key = Some leaf ->
  import_resolve_encoded_cold current key = Some (Some leaf).
Proof.
  intros initial records current observations key leaf Trace Read.
  unfold import_read_observed_cold_leaf in Read.
  destruct (_ && _) eqn:Ready; [|discriminate]. apply andb_true_iff in Ready.
  destruct (import_resolve_encoded_cold (import_project_cold_observations observations) key)
    as [[found|]|] eqn:Resolved; try discriminate.
  inversion Read; subst found.
  eapply import_observed_strict_leaf_establishes_a_current_binding; eauto; tauto.
Qed.

Theorem import_failed_or_unread_alias_cannot_pass_read_only_validation :
  forall observations key alias,
  observations alias key = ImportColdUnread \/ observations alias key = ImportColdReadFailure ->
  import_read_observed_cold_leaf observations key = None.
Proof.
  intros observations key alias Failed. unfold import_read_observed_cold_leaf.
  destruct alias; destruct Failed as [Failed|Failed]; rewrite Failed; simpl; auto;
    destruct (import_cold_observation_ready (observations ImportRawAlias key)); reflexivity.
Qed.

Definition import_observed_history_bytes (observations : ImportHistoryObservations)
    : ImportHistoryByteStore :=
  fun key => match observations key with
    | Some (ImportRootLookupFound bytes) => Some bytes
    | _ => None
    end.

Theorem import_read_only_history_check_uses_retained_physical_bytes :
  forall Hash initial records logical current observations key edges,
  import_history_observation_trace Hash [] initial records logical current observations ->
  import_checked_wire_history Hash (import_observed_history_bytes observations) key = Some edges ->
  import_checked_wire_history Hash current key = Some edges.
Proof.
  intros Hash initial records logical current observations key edges Trace Read.
  destruct (import_checked_wire_history_has_exact_witness _ _ _ _ Read)
    as [Width [Domain [bytes [Observed [Hashed Parsed]]]]].
  unfold import_observed_history_bytes in Observed.
  destruct (observations key) as [[|found|]|] eqn:Lookup; try discriminate.
  inversion Observed; subst found.
  destruct (import_history_trace_derives_observation_origins
    _ _ _ _ _ _ _ Trace _ _ Lookup) as [record [Present [Key Result]]].
  pose proof (import_history_trace_preserves_all_successfully_observed_bytes
    _ _ _ _ _ _ _ Trace) as Retained.
  rewrite Forall_forall in Retained.
  specialize (Retained record Present bytes Result). rewrite Key in Retained.
  assert (Valid : import_cursor_word_validb key = true).
  { apply import_cursor_word_validity_characterization. auto. }
  unfold import_checked_wire_history. rewrite Valid, Retained, Hashed.
  destruct (list_eq_dec Nat.eq_dec key key); congruence.
Qed.

Theorem import_history_observation_trace_preserves_initial_bytes :
  forall Hash rows initial records logical current observations,
  import_history_observation_trace Hash rows initial records logical current observations ->
  import_history_store_extension initial current.
Proof.
  intros Hash rows initial records logical current observations Trace. induction Trace;
    try assumption.
  - intros key bytes Read. exact Read.
  - intros key bytes Read. apply H. now apply IHTrace.
Qed.

Record ImportPhysicalReadEpisode : Type := {
  import_episode_history : ImportHistoryByteStore;
  import_episode_cold : ImportEncodedColdStore;
  import_episode_history_observations : ImportHistoryObservations;
  import_episode_cold_observations : ImportColdObservations
}.

Definition import_physical_episode_trace (Hash : list nat -> list nat)
    (history : ImportHistoryByteStore) (cold : ImportEncodedColdStore)
    (episode : ImportPhysicalReadEpisode) : Prop :=
  (exists records logical,
    import_history_observation_trace Hash [] history records logical
      (import_episode_history episode) (import_episode_history_observations episode)) /\
  (exists records,
    import_cold_observation_trace cold records
      (import_episode_cold episode) (import_episode_cold_observations episode)).

Section PhysicalScan.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).
Variable Hash : list nat -> list nat.

Definition import_scan_payload_hash (payload : list nat) : nat :=
  import_key_to_nat (Hash (import_nat_to_key 8 (length payload) ++ payload)).

Definition import_check_physical_reference (episode : ImportPhysicalReadEpisode)
    (reference : ImportReference) : option (list ImportReference) :=
  match reference with
  | ImportHistoryRef key context =>
      if Nat.eqb key (import_key_to_nat (import_nat_to_key 32 key)) then
        match import_checked_wire_history Hash
          (import_observed_history_bytes (import_episode_history_observations episode))
          (import_nat_to_key 32 key) with
        | Some edges => import_contextual_children context (map import_codec_edge_to_closure edges)
        | None => None
        end
      else None
  | ImportColdRef key expected =>
      match import_read_observed_cold_leaf (import_episode_cold_observations episode) key with
      | Some leaf =>
          if Nat.eqb (import_scan_payload_hash (import_leaf_payload leaf)) key then
            if import_leaf_kind_eq_dec (import_leaf_kind leaf) expected then
              if import_validate_cold_leaf Value decode_item expected leaf then Some [] else None
            else None
          else None
      | None => None
      end
  end.

Definition import_physical_store_view (history : ImportHistoryByteStore)
    (cold : ImportEncodedColdStore) : ImportStore :=
  import_encoded_logical_view (import_project_wire_history history) cold.

Theorem import_validated_leaf_refines_consuming_read :
  forall store key expected leaf,
  import_resolve_cold store key = Some leaf ->
  Nat.eqb (import_scan_payload_hash (import_leaf_payload leaf)) key = true ->
  import_leaf_kind leaf = expected ->
  import_validate_cold_leaf Value decode_item expected leaf = true ->
  import_consumer_checked_read Value decode_item (import_project_wire_hash Hash)
    import_scan_payload_hash store (ImportColdRef key expected) = Some [].
Proof.
  intros store key expected leaf Resolved Hashed Kind Valid.
  apply import_consumer_checked_read_accepts_exactly_structural_and_typed_reads.
  - cbn [import_checked_read]. rewrite Resolved, Hashed.
    destruct (import_leaf_kind_eq_dec (import_leaf_kind leaf) expected); congruence.
  - apply import_cold_validation_iff_successful_consumption in Valid as [values Consumed].
    exists values. unfold import_consume_stored_cold. now rewrite Resolved.
Qed.

Theorem import_actual_physical_reference_check_refines_consuming_reads :
  forall history cold episode reference children,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_physical_episode_trace Hash history cold episode ->
  import_check_physical_reference episode reference = Some children ->
  import_consumer_checked_read Value decode_item (import_project_wire_hash Hash)
    import_scan_payload_hash
    (import_physical_store_view (import_episode_history episode) (import_episode_cold episode))
    reference = Some children.
Proof.
  intros history cold episode reference children HashValid [[records [logical HTrace]] [cold_records CTrace]] Read.
  destruct reference as [key context|key expected]; cbn [import_check_physical_reference] in Read.
  - destruct (key =? import_key_to_nat (import_nat_to_key 32 key)) eqn:Key; [|discriminate].
    apply Nat.eqb_eq in Key.
    destruct (import_checked_wire_history Hash
      (import_observed_history_bytes (import_episode_history_observations episode))
      (import_nat_to_key 32 key)) as [edges|] eqn:History; [|discriminate].
    pose proof (import_checked_wire_history_has_exact_witness _ _ _ _ History)
      as [Width [Domain _]].
    pose proof (import_read_only_history_check_uses_retained_physical_bytes
      _ _ _ _ _ _ _ _ HTrace History) as Current.
    apply import_consumer_checked_read_accepts_exactly_structural_and_typed_reads; [|exact I].
    change (import_checked_read (import_project_wire_hash Hash) import_scan_payload_hash
      (import_project_wire_store (import_episode_history episode)
        (import_cold_raw (import_physical_store_view (import_episode_history episode)
          (import_episode_cold episode))) (fun _ => None))
      (ImportHistoryRef key context) = Some children).
    rewrite Key at 1.
    rewrite import_checked_wire_and_closure_history_reads_correspond; [now rewrite Current|exact HashValid|].
    apply import_cursor_word_validity_characterization. auto.
  - destruct (import_read_observed_cold_leaf (import_episode_cold_observations episode) key)
      as [leaf|] eqn:Cold; [|discriminate].
    destruct (import_scan_payload_hash (import_leaf_payload leaf) =? key) eqn:Hashed; [|discriminate].
    destruct (import_leaf_kind_eq_dec (import_leaf_kind leaf) expected) as [Kind|]; [|discriminate].
    destruct (import_validate_cold_leaf Value decode_item expected leaf) eqn:Valid; [|discriminate].
    inversion Read; subst children.
    pose proof (import_successful_read_only_cold_check_has_a_current_binding
      _ _ _ _ _ _ CTrace Cold) as Resolved.
    apply (import_encoded_logical_resolution_matches_only_strict_success
      (import_project_wire_history (import_episode_history episode))) in Resolved.
    eapply import_validated_leaf_refines_consuming_read; eauto.
Qed.

Theorem import_physical_episode_preserves_all_checked_bindings :
  forall history cold episode,
  import_physical_episode_trace Hash history cold episode ->
  import_binding_extension (import_physical_store_view history cold)
    (import_physical_store_view (import_episode_history episode) (import_episode_cold episode)).
Proof.
  intros history cold episode [[records [logical History]] [cold_records Cold]].
  apply import_encoded_view_preserves_joint_history_and_cold_extensions.
  - apply import_physical_history_extension_preserves_the_decoded_projection.
    eapply import_history_observation_trace_preserves_initial_bytes; eauto.
  - eapply import_observation_trace_preserves_the_concurrent_storage_run; eauto.
Qed.

Record ImportPhysicalScanState : Type := {
  import_physical_scan_history : ImportHistoryByteStore;
  import_physical_scan_cold : ImportEncodedColdStore;
  import_physical_scan_checked : list ImportReference;
  import_physical_scan_frontier : list ImportReference;
  import_physical_scan_failed : bool
}.

Definition import_physical_scan_view (state : ImportPhysicalScanState) : ImportScanState :=
  {| import_scan_store := import_physical_store_view
       (import_physical_scan_history state) (import_physical_scan_cold state);
     import_scan_checked := import_physical_scan_checked state;
     import_scan_frontier := import_physical_scan_frontier state |}.

Definition import_begin_physical_scan (root : list nat)
    (history : ImportHistoryByteStore) (cold : ImportEncodedColdStore)
    : option ImportPhysicalScanState :=
  if import_cursor_word_validb root then
    Some {| import_physical_scan_history := history;
            import_physical_scan_cold := cold;
            import_physical_scan_checked := [];
            import_physical_scan_frontier := [ImportHistoryRef (import_key_to_nat root) []];
            import_physical_scan_failed := false |}
  else None.

Definition import_apply_scan_episode (episode : ImportPhysicalReadEpisode)
    (state : ImportPhysicalScanState) : ImportPhysicalScanState :=
  let progress :=
    if import_physical_scan_failed state then
      (import_physical_scan_checked state, import_physical_scan_frontier state, true)
    else match import_physical_scan_frontier state with
    | [] => (import_physical_scan_checked state, [], false)
    | reference :: rest => match import_check_physical_reference episode reference with
      | None => (import_physical_scan_checked state, reference :: rest, true)
      | Some children =>
          (reference :: import_physical_scan_checked state,
           import_unchecked_frontier (reference :: import_physical_scan_checked state)
             (children ++ rest), false)
      end
    end in
  {| import_physical_scan_history := import_episode_history episode;
     import_physical_scan_cold := import_episode_cold episode;
     import_physical_scan_checked := fst (fst progress);
     import_physical_scan_frontier := snd (fst progress);
     import_physical_scan_failed := snd progress |}.

Fixpoint import_execute_scan_episodes (episodes : list ImportPhysicalReadEpisode)
    (state : ImportPhysicalScanState) : ImportPhysicalScanState :=
  match episodes with
  | [] => state
  | episode :: rest => import_execute_scan_episodes rest (import_apply_scan_episode episode state)
  end.

Inductive import_scan_episode_schedule :
    ImportPhysicalScanState -> list ImportPhysicalReadEpisode -> Prop :=
| import_scan_episode_schedule_nil : forall state, import_scan_episode_schedule state []
| import_scan_episode_schedule_cons : forall state episode rest,
    import_physical_episode_trace Hash
      (import_physical_scan_history state) (import_physical_scan_cold state) episode ->
    import_scan_episode_schedule (import_apply_scan_episode episode state) rest ->
    import_scan_episode_schedule state (episode :: rest).

Definition import_physical_scan_complete (state : ImportPhysicalScanState) : bool :=
  negb (import_physical_scan_failed state) &&
    match import_physical_scan_frontier state with [] => true | _ => false end.

Theorem import_scan_episode_failure_is_sticky : forall episode state,
  import_physical_scan_failed state = true ->
  import_physical_scan_failed (import_apply_scan_episode episode state) = true.
Proof. intros. unfold import_apply_scan_episode. now rewrite H. Qed.

Theorem import_scan_required_read_failure_retains_unresolved_work :
  forall episode state reference rest,
  import_physical_scan_failed state = false ->
  import_physical_scan_frontier state = reference :: rest ->
  import_check_physical_reference episode reference = None ->
  import_physical_scan_failed (import_apply_scan_episode episode state) = true /\
  import_physical_scan_checked (import_apply_scan_episode episode state) =
    import_physical_scan_checked state /\
  import_physical_scan_frontier (import_apply_scan_episode episode state) = reference :: rest.
Proof.
  intros. unfold import_apply_scan_episode. now rewrite H, H0, H1.
Qed.

Theorem import_successful_physical_episode_refines_scan_steps :
  forall state episode,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_physical_episode_trace Hash
    (import_physical_scan_history state) (import_physical_scan_cold state) episode ->
  import_physical_scan_failed (import_apply_scan_episode episode state) = false ->
  import_physical_scan_failed state = false /\
  import_consumer_scan_run Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash
    (import_physical_scan_view state) (import_physical_scan_view (import_apply_scan_episode episode state)).
Proof.
  intros [history cold checked frontier failed] episode HashValid Trace Success.
  cbn [import_physical_scan_failed] in *.
  unfold import_apply_scan_episode in Success |- *.
  cbn [import_physical_scan_frontier import_physical_scan_checked import_physical_scan_failed] in Success |- *.
  destruct failed; [discriminate|]. split; [reflexivity|].
  pose proof (import_physical_episode_preserves_all_checked_bindings _ _ _ Trace) as Extension.
  destruct frontier as [|reference rest].
  - eapply import_consumer_scan_run_cons; [|constructor].
    apply import_consumer_scan_concurrent_commit. exact Extension.
  - destruct (import_check_physical_reference episode reference) as [children|] eqn:Read;
      [|discriminate].
    eapply import_consumer_scan_run_cons.
    + apply import_consumer_scan_concurrent_commit. exact Extension.
    + eapply import_consumer_scan_run_cons; [|constructor].
      apply import_consumer_scan_reference.
      eapply import_actual_physical_reference_check_refines_consuming_reads; eauto.
Qed.

Theorem import_consumer_runs_compose : forall first middle last,
  import_consumer_scan_run Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash first middle ->
  import_consumer_scan_run Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash middle last ->
  import_consumer_scan_run Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash first last.
Proof.
  intros first middle last Run. induction Run; intros Tail; auto.
  eapply import_consumer_scan_run_cons; eauto.
Qed.

Theorem import_successful_physical_scan_refines_consuming_execution :
  forall state episodes,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_scan_episode_schedule state episodes ->
  import_physical_scan_failed (import_execute_scan_episodes episodes state) = false ->
  import_physical_scan_failed state = false /\
  import_consumer_scan_run Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash
    (import_physical_scan_view state)
    (import_physical_scan_view (import_execute_scan_episodes episodes state)).
Proof.
  intros state episodes HashValid Schedule. induction Schedule; intros Success.
  - split; [exact Success|constructor].
  - destruct (IHSchedule Success) as [Active Tail].
    destruct (import_successful_physical_episode_refines_scan_steps _ _ HashValid H Active)
      as [Initial Head]. split; [exact Initial|].
    eapply import_consumer_runs_compose; eauto.
Qed.

Theorem import_complete_physical_scan_establishes_original_root_closure :
  forall root history cold initial episodes,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_begin_physical_scan root history cold = Some initial ->
  import_scan_episode_schedule initial episodes ->
  import_physical_scan_complete (import_execute_scan_episodes episodes initial) = true ->
  import_consumable_closed_root Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash
    (import_scan_store (import_physical_scan_view (import_execute_scan_episodes episodes initial)))
    (ImportHistoryRef (import_key_to_nat root) []).
Proof.
  intros root history cold initial episodes HashValid Begin Schedule Complete.
  unfold import_physical_scan_complete in Complete.
  apply andb_true_iff in Complete as [Active Empty]. apply negb_true_iff in Active.
  pose proof (import_successful_physical_scan_refines_consuming_execution
    _ _ HashValid Schedule Active) as [_ Run].
  unfold import_begin_physical_scan in Begin.
  destruct (import_cursor_word_validb root); [|discriminate]. inversion Begin; subst initial.
  eapply import_completed_consuming_scan_establishes_consumable_closure; [exact Run|].
  change (import_physical_scan_frontier (import_execute_scan_episodes episodes
    {| import_physical_scan_history := history; import_physical_scan_cold := cold;
       import_physical_scan_checked := [];
       import_physical_scan_frontier := [ImportHistoryRef (import_key_to_nat root) []];
       import_physical_scan_failed := false |}) = []).
  destruct (import_physical_scan_frontier _) eqn:Frontier; [reflexivity|discriminate].
Qed.

Theorem import_failed_scan_cannot_recover_with_later_observations :
  forall episodes state,
  import_physical_scan_failed state = true ->
  import_physical_scan_complete (import_execute_scan_episodes episodes state) = false.
Proof.
  induction episodes as [|episode rest IH]; intros state Failed.
  - cbn [import_execute_scan_episodes]. unfold import_physical_scan_complete. now rewrite Failed.
  - apply IH. now apply import_scan_episode_failure_is_sticky.
Qed.

Theorem import_completed_scan_remains_consumable_during_publication :
  forall root history cold initial episodes later_history later_cold,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_begin_physical_scan root history cold = Some initial ->
  import_scan_episode_schedule initial episodes ->
  import_physical_scan_complete (import_execute_scan_episodes episodes initial) = true ->
  import_history_store_extension
    (import_physical_scan_history (import_execute_scan_episodes episodes initial)) later_history ->
  import_encoded_run
    (import_physical_scan_cold (import_execute_scan_episodes episodes initial)) later_cold ->
  import_consumable_closed_root Value decode_item (import_project_wire_hash Hash) import_scan_payload_hash
    (import_physical_store_view later_history later_cold) (ImportHistoryRef (import_key_to_nat root) []).
Proof.
  intros root history cold initial episodes later_history later_cold HashValid Begin Schedule Complete History Cold.
  eapply import_binding_extension_preserves_consumable_closed_root with
    (old := import_physical_store_view
      (import_physical_scan_history (import_execute_scan_episodes episodes initial))
      (import_physical_scan_cold (import_execute_scan_episodes episodes initial))).
  - apply import_encoded_view_preserves_joint_history_and_cold_extensions; [|exact Cold].
    now apply import_physical_history_extension_preserves_the_decoded_projection.
  - eapply import_complete_physical_scan_establishes_original_root_closure; eauto.
Qed.

Theorem import_scan_without_more_episodes_does_not_hide_pending_work : forall state reference rest,
  import_physical_scan_frontier state = reference :: rest ->
  import_physical_scan_complete (import_execute_scan_episodes [] state) = false.
Proof.
  intros state reference rest Pending. cbn [import_execute_scan_episodes].
  unfold import_physical_scan_complete. rewrite Pending. now rewrite andb_false_r.
Qed.

End PhysicalScan.

Definition import_scan_demo_key : list nat := repeat 0 32.
Definition import_scan_demo_hash (_ : list nat) : list nat := import_scan_demo_key.
Definition import_scan_demo_decoder (_ : ImportLeafKind) (_ : list nat) : option unit := Some tt.
Definition import_scan_demo_history : ImportHistoryByteStore :=
  fun key => if list_eq_dec Nat.eq_dec key import_scan_demo_key
    then Some ([2; 0] ++ import_scan_demo_key) else None.
Definition import_scan_demo_cold : ImportEncodedColdStore :=
  fun alias key => match alias with
    | ImportRawAlias => if Nat.eq_dec key 0 then Some import_valid_empty_joins_bytes else None
    | ImportLegacyAlias => None
    end.
Definition import_scan_demo_history_observations : ImportHistoryObservations :=
  import_capture_history_observation import_empty_history_observations import_scan_demo_key
    (ImportRootLookupFound ([2; 0] ++ import_scan_demo_key)).
Definition import_scan_demo_cold_observations : ImportColdObservations :=
  import_capture_first_cold_observation
    (import_capture_first_cold_observation import_empty_cold_observations
      ImportRawAlias 0 (ImportRootLookupFound import_valid_empty_joins_bytes))
    ImportLegacyAlias 0 ImportRootLookupAbsent.
Definition import_scan_demo_history_episode : ImportPhysicalReadEpisode :=
  {| import_episode_history := import_scan_demo_history;
     import_episode_cold := import_scan_demo_cold;
     import_episode_history_observations := import_scan_demo_history_observations;
     import_episode_cold_observations := import_empty_cold_observations |}.
Definition import_scan_demo_cold_episode : ImportPhysicalReadEpisode :=
  {| import_episode_history := import_scan_demo_history;
     import_episode_cold := import_scan_demo_cold;
     import_episode_history_observations := import_empty_history_observations;
     import_episode_cold_observations := import_scan_demo_cold_observations |}.
Definition import_scan_demo_initial : ImportPhysicalScanState :=
  {| import_physical_scan_history := import_scan_demo_history;
     import_physical_scan_cold := import_scan_demo_cold;
     import_physical_scan_checked := [];
     import_physical_scan_frontier := [ImportHistoryRef 0 []];
     import_physical_scan_failed := false |}.

Example import_scan_demo_history_episode_has_actual_read_evidence :
  import_physical_episode_trace import_scan_demo_hash
    import_scan_demo_history import_scan_demo_cold import_scan_demo_history_episode.
Proof.
  split.
  - do 2 eexists. eapply import_history_observation_success; [constructor|reflexivity].
  - eexists. constructor.
Qed.

Example import_scan_demo_cold_episode_has_actual_read_evidence :
  import_physical_episode_trace import_scan_demo_hash
    import_scan_demo_history import_scan_demo_cold import_scan_demo_cold_episode.
Proof.
  split.
  - do 2 eexists. constructor.
  - eexists. eapply import_cold_observation_success; [|reflexivity].
    eapply import_cold_observation_success; [constructor|reflexivity].
Qed.

Example import_scan_nonempty_root_has_a_physical_episode_schedule :
  import_scan_episode_schedule (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_initial [import_scan_demo_history_episode; import_scan_demo_cold_episode].
Proof.
  econstructor.
  - apply import_scan_demo_history_episode_has_actual_read_evidence.
  - econstructor.
    + apply import_scan_demo_cold_episode_has_actual_read_evidence.
    + constructor.
Qed.

Example import_scan_nonempty_root_requires_history_and_typed_cold_reads :
  import_physical_scan_complete (import_execute_scan_episodes
    (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    [import_scan_demo_history_episode; import_scan_demo_cold_episode] import_scan_demo_initial) = true.
Proof. vm_compute. reflexivity. Qed.

Example import_scan_partial_history_does_not_establish_complete_root :
  import_physical_scan_complete (import_execute_scan_episodes
    (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    [import_scan_demo_history_episode] import_scan_demo_initial) = false.
Proof. vm_compute. reflexivity. Qed.

Definition import_scan_demo_missing_cold_episode : ImportPhysicalReadEpisode :=
  {| import_episode_history := import_scan_demo_history;
     import_episode_cold := import_empty_encoded_store;
     import_episode_history_observations := import_empty_history_observations;
     import_episode_cold_observations := fun _ _ => ImportColdObservedAbsent |}.

Example import_scan_missing_cold_stays_failed_after_later_valid_reads :
  let initial := {| import_physical_scan_history := import_scan_demo_history;
                    import_physical_scan_cold := import_empty_encoded_store;
                    import_physical_scan_checked := [];
                    import_physical_scan_frontier := [ImportHistoryRef 0 []];
                    import_physical_scan_failed := false |} in
  let first := {| import_episode_history := import_scan_demo_history;
                  import_episode_cold := import_empty_encoded_store;
                  import_episode_history_observations := import_scan_demo_history_observations;
                  import_episode_cold_observations := import_empty_cold_observations |} in
  let result := import_execute_scan_episodes
    (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    [first; import_scan_demo_missing_cold_episode; import_scan_demo_cold_episode] initial in
  import_physical_scan_complete result = false /\
  import_physical_scan_frontier result = [ImportColdRef 0 ImportJoins].
Proof. vm_compute. auto. Qed.

Example import_scan_same_hash_requires_each_expected_kind :
  import_check_physical_reference (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_cold_episode (ImportColdRef 0 ImportJoins) = Some [] /\
  import_check_physical_reference (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_cold_episode (ImportColdRef 0 ImportData) = None /\
  import_unchecked_frontier [ImportColdRef 0 ImportJoins]
    [ImportColdRef 0 ImportData; ImportColdRef 0 ImportJoins] = [ImportColdRef 0 ImportData].
Proof. vm_compute. auto. Qed.

Example import_scan_same_history_hash_requires_each_context :
  import_unchecked_frontier [ImportHistoryRef 0 [0]]
    [ImportHistoryRef 0 [1]; ImportHistoryRef 0 [0]] = [ImportHistoryRef 0 [1]].
Proof. vm_compute. reflexivity. Qed.

Example import_scan_shared_history_context_changes_the_required_cold_kind :
  import_check_physical_reference (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_history_episode (ImportHistoryRef 0 [0]) = Some [ImportColdRef 0 ImportData] /\
  import_check_physical_reference (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_history_episode (ImportHistoryRef 0 [2]) = Some [ImportColdRef 0 ImportJoins] /\
  import_check_physical_reference (fun _ => unit) import_scan_demo_decoder import_scan_demo_hash
    import_scan_demo_cold_episode (ImportColdRef 0 ImportData) = None.
Proof. vm_compute. auto. Qed.

Definition import_scan_demo_legacy_only : ImportEncodedColdStore :=
  fun alias key => match alias with
    | ImportRawAlias => None
    | ImportLegacyAlias => if Nat.eq_dec key 0 then Some import_valid_empty_joins_bytes else None
    end.
Definition import_scan_demo_both_aliases : ImportEncodedColdStore :=
  import_fill_encoded_alias import_scan_demo_legacy_only ImportRawAlias 0 import_valid_empty_joins_bytes.
Definition import_scan_demo_alias_observations : ImportColdObservations :=
  import_capture_first_cold_observation
    (import_capture_first_cold_observation import_empty_cold_observations
      ImportRawAlias 0 ImportRootLookupAbsent)
    ImportLegacyAlias 0 (ImportRootLookupFound import_valid_empty_joins_bytes).

Example import_scan_alias_insert_between_reads_has_an_actual_trace :
  exists records,
  import_cold_observation_trace import_scan_demo_legacy_only records
    import_scan_demo_both_aliases import_scan_demo_alias_observations.
Proof.
  eexists. eapply import_cold_observation_success; [|reflexivity].
  eapply import_cold_observation_write with (store := import_scan_demo_legacy_only).
  - eapply import_cold_observation_success; [constructor|reflexivity].
  - eapply import_encoded_run_step with (alias := ImportRawAlias) (key := 0)
      (input := import_valid_empty_joins_bytes) (storage_ok := true) (success := true);
      [constructor|]. vm_compute. reflexivity.
Qed.

Example import_scan_compatible_insert_accepts_without_common_alias_snapshot :
  import_read_observed_cold_leaf import_scan_demo_alias_observations 0 =
    Some {| import_leaf_kind := ImportJoins; import_leaf_payload := repeat 0 8 |} /\
  import_project_cold_observations import_scan_demo_alias_observations ImportRawAlias 0 = None /\
  import_scan_demo_both_aliases ImportRawAlias 0 = Some import_valid_empty_joins_bytes.
Proof. vm_compute. auto. Qed.

Example import_scan_a_required_alias_failure_is_not_absence :
  import_read_observed_cold_leaf
    (fun alias _ => match alias with
      | ImportRawAlias => ImportColdReadFailure
      | ImportLegacyAlias => ImportColdObservedBytes import_valid_empty_joins_bytes
      end) 0 = None.
Proof. reflexivity. Qed.

Example import_scan_conflicting_aliases_do_not_establish_binding :
  import_read_observed_cold_leaf
    (fun alias _ => match alias with
      | ImportRawAlias => ImportColdObservedBytes import_valid_empty_data_bytes
      | ImportLegacyAlias => ImportColdObservedBytes import_valid_empty_joins_bytes
      end) 0 = None.
Proof. vm_compute. reflexivity. Qed.

Definition import_scan_demo_inserted_raw : ImportEncodedColdStore :=
  import_fill_encoded_alias import_empty_encoded_store ImportRawAlias 0 import_valid_empty_joins_bytes.
Definition import_scan_demo_inserted_both : ImportEncodedColdStore :=
  import_fill_encoded_alias import_scan_demo_inserted_raw ImportLegacyAlias 0 import_valid_empty_joins_bytes.

Example import_scan_initially_empty_aliases_can_be_populated_between_reads :
  exists records,
  import_cold_observation_trace import_empty_encoded_store records
    import_scan_demo_inserted_both import_scan_demo_alias_observations /\
  import_read_observed_cold_leaf import_scan_demo_alias_observations 0 =
    Some {| import_leaf_kind := ImportJoins; import_leaf_payload := repeat 0 8 |}.
Proof.
  eexists. split; [|vm_compute; reflexivity].
  eapply import_cold_observation_success; [|reflexivity].
  eapply import_cold_observation_write with (store := import_empty_encoded_store).
  - eapply import_cold_observation_success; [constructor|reflexivity].
  - eapply import_encoded_run_step with (middle := import_scan_demo_inserted_raw)
      (alias := ImportLegacyAlias) (key := 0) (input := import_valid_empty_joins_bytes)
      (storage_ok := true) (success := true); [|vm_compute; reflexivity].
    eapply import_encoded_run_step with (alias := ImportRawAlias) (key := 0)
      (input := import_valid_empty_joins_bytes) (storage_ok := true) (success := true);
      [constructor|vm_compute; reflexivity].
Qed.

Example import_scan_two_recorded_absences_do_not_become_a_success_after_insertion :
  let observations := import_capture_first_cold_observation
    (import_capture_first_cold_observation import_empty_cold_observations
      ImportRawAlias 0 ImportRootLookupAbsent)
    ImportLegacyAlias 0 ImportRootLookupAbsent in
  exists records,
    import_cold_observation_trace import_empty_encoded_store records import_scan_demo_inserted_raw observations /\
    import_read_observed_cold_leaf observations 0 = None.
Proof.
  eexists. split; [|reflexivity].
  eapply import_cold_observation_write with (store := import_empty_encoded_store).
  - eapply import_cold_observation_success; [|reflexivity].
    eapply import_cold_observation_success; [constructor|reflexivity].
  - eapply import_encoded_run_step with (alias := ImportRawAlias) (key := 0)
      (input := import_valid_empty_joins_bytes) (storage_ok := true) (success := true);
      [constructor|vm_compute; reflexivity].
Qed.
